pub mod metadata;
pub mod sql_utils;
pub mod types;

pub use sql_utils::{escape_sql_string, make_column_safe};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use duckdb::Connection;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const SOURCE_VIEW_NAME: &str = "pv_data";

/// A single field/column in the parquet file.
#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortSpec {
    pub column_index: usize,
    pub column_name: String,
    pub direction: SortDirection,
}

/// The parquet engine backed by DuckDB.
pub struct ParquetEngine {
    conn: Connection,
    path: PathBuf,
    is_folder: bool,
    fields: Vec<Field>,
    record_count: i64,
    partition_count: i64,
}

impl ParquetEngine {
    /// Open a single parquet file or a folder of partitioned parquet files.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open_in_memory()?;

        let (is_folder, read_path) = if path.is_dir() {
            (
                true,
                format!(
                    "'{}/**/*.parquet'",
                    escape_sql_string(&path.display().to_string())
                ),
            )
        } else if path.is_file() {
            (
                false,
                format!("'{}'", escape_sql_string(&path.display().to_string())),
            )
        } else {
            bail!("Path does not exist: {}", path.display());
        };

        let fields = describe_fields(&conn, &read_path)?;
        let record_count = count_rows(&conn, &read_path)?;
        register_source_view(&conn, &read_path)?;
        let partition_count = if is_folder {
            count_partitions(&path)?
        } else {
            1
        };

        Ok(Self {
            conn,
            path,
            is_folder,
            fields,
            record_count,
            partition_count,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub fn record_count(&self) -> i64 {
        self.record_count
    }

    pub fn partition_count(&self) -> i64 {
        self.partition_count
    }

    /// The DuckDB read_parquet path spec used in queries.
    pub fn read_path_spec(&self) -> String {
        let escaped = escape_sql_string(&self.path.display().to_string());
        if self.is_folder {
            format!("'{}/**/*.parquet'", escaped)
        } else {
            format!("'{}'", escaped)
        }
    }

    /// Load metadata for the current parquet file/folder.
    pub fn load_metadata(&self) -> Result<metadata::ParquetMetadata> {
        metadata::ParquetMetadata::load(&self.conn, &self.read_path_spec())
    }

    pub fn read_rows_query(
        &self,
        selected_fields: &[String],
        where_filter: Option<&str>,
        offset: i64,
        limit: i64,
        sort: Option<&SortSpec>,
    ) -> Result<QueryResult> {
        if selected_fields.is_empty() {
            return Ok(QueryResult {
                headers: Vec::new(),
                rows: Vec::new(),
            });
        }

        let field_list: Vec<String> = selected_fields
            .iter()
            .map(|f| make_column_safe(f))
            .collect();
        let fields_sql = field_list.join(", ");

        let mut query = format!(
            "SELECT {} FROM {}",
            fields_sql,
            make_column_safe(SOURCE_VIEW_NAME)
        );

        if let Some(filter) = where_filter {
            let filter = strip_optional_where_prefix(filter.trim());
            if !filter.is_empty() {
                query.push_str(" WHERE ");
                query.push_str(filter);
            }
        }

        push_sort_clause(&mut query, sort, selected_fields.len())?;
        query.push_str(&format!(" LIMIT {} OFFSET {}", limit.max(0), offset.max(0)));

        let mut result = run_query(&self.conn, &query)?;
        result.headers = selected_fields.to_vec();
        Ok(result)
    }

    pub fn query_sql(
        &self,
        sql: &str,
        offset: i64,
        limit: i64,
        sort: Option<&SortSpec>,
    ) -> Result<QueryResult> {
        let sql = validate_and_trim_sql(sql)?;
        let mut query = format!("SELECT * FROM ({}) AS pv_query", sql);
        let col_count = query_column_count(&self.conn, &query)?;
        push_sort_clause(&mut query, sort, col_count)?;
        query.push_str(&format!(" LIMIT {} OFFSET {}", limit.max(0), offset.max(0)));
        run_query(&self.conn, &query)
    }

    pub fn count_sql(&self, sql: &str) -> Result<i64> {
        let sql = validate_and_trim_sql(sql)?;
        let query = format!("SELECT count(*) FROM ({}) AS pv_count", sql);
        let count: i64 = self.conn.query_row(&query, [], |row| row.get(0))?;
        Ok(count)
    }
}

fn register_source_view(conn: &Connection, read_path: &str) -> Result<()> {
    let query = format!(
        "CREATE TEMP VIEW {} AS SELECT * FROM read_parquet({})",
        make_column_safe(SOURCE_VIEW_NAME),
        read_path
    );
    conn.execute_batch(&query)?;
    Ok(())
}

fn run_query(conn: &Connection, query: &str) -> Result<QueryResult> {
    let mut stmt = conn.prepare(query)?;
    let mut rows = stmt.query([])?;
    let stmt = rows
        .as_ref()
        .context("query statement should be available")?;
    let col_count = stmt.column_count();
    let headers = stmt.column_names();

    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        let mut values = Vec::with_capacity(col_count);
        for i in 0..col_count {
            let val = row.get::<_, duckdb::types::Value>(i)?;
            values.push(format_value(&val));
        }
        result.push(values);
    }

    Ok(QueryResult {
        headers,
        rows: result,
    })
}

fn query_column_count(conn: &Connection, query: &str) -> Result<usize> {
    let wrapped = format!("SELECT * FROM ({}) AS pv_columns LIMIT 0", query);
    let mut stmt = conn.prepare(&wrapped)?;
    let rows = stmt.query([])?;
    let stmt = rows
        .as_ref()
        .context("query statement should be available")?;
    Ok(stmt.column_count())
}

fn push_sort_clause(query: &mut String, sort: Option<&SortSpec>, col_count: usize) -> Result<()> {
    if let Some(sort) = sort {
        if sort.column_index >= col_count {
            bail!("Sort column is out of range");
        }
        let direction = match sort.direction {
            SortDirection::Asc => "ASC",
            SortDirection::Desc => "DESC",
        };
        query.push_str(&format!(
            " ORDER BY {} {} NULLS LAST",
            sort.column_index + 1,
            direction
        ));
    }
    Ok(())
}

fn validate_and_trim_sql(sql: &str) -> Result<String> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    if trimmed.is_empty() {
        bail!("SQL query is empty");
    }
    let upper = trimmed.to_ascii_uppercase();
    if !(upper.starts_with("SELECT ") || upper.starts_with("WITH ")) {
        bail!("Only SELECT/WITH queries are supported in SQL mode");
    }
    Ok(trimmed.to_string())
}

fn strip_optional_where_prefix(filter: &str) -> &str {
    let trimmed = filter.trim();
    if trimmed.len() >= 5
        && trimmed
            .get(..5)
            .is_some_and(|s| s.eq_ignore_ascii_case("WHERE"))
    {
        trimmed[5..].trim()
    } else {
        trimmed
    }
}

fn describe_fields(conn: &Connection, read_path: &str) -> Result<Vec<Field>> {
    let query = format!("DESCRIBE SELECT * FROM read_parquet({})", read_path);
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(0)?;
        Ok(Field { name })
    })?;

    let mut fields = Vec::new();
    for row in rows {
        fields.push(row?);
    }
    if fields.is_empty() {
        bail!("No fields found in parquet file(s)");
    }
    Ok(fields)
}

fn count_rows(conn: &Connection, read_path: &str) -> Result<i64> {
    let query = format!("SELECT count(*) FROM read_parquet({})", read_path);
    let count: i64 = conn.query_row(&query, [], |row| row.get(0))?;
    Ok(count)
}

fn count_partitions(dir: &Path) -> Result<i64> {
    let mut count = 0i64;
    for entry in walkdir(dir)? {
        if entry.extension().and_then(|e| e.to_str()) == Some("parquet") {
            count += 1;
        }
    }
    Ok(count.max(1))
}

fn walkdir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    let mut visited = HashSet::new();
    while let Some(d) = stack.pop() {
        let canon = d.canonicalize().unwrap_or_else(|_| d.clone());
        if !visited.insert(canon) {
            continue;
        }
        for entry in
            std::fs::read_dir(&d).with_context(|| format!("Reading dir: {}", d.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                result.push(path);
            }
        }
    }
    Ok(result)
}

fn time_unit_to_nanos(unit: &duckdb::types::TimeUnit, val: &i64) -> i64 {
    use duckdb::types::TimeUnit;
    match unit {
        TimeUnit::Second => val.saturating_mul(1_000_000_000),
        TimeUnit::Millisecond => val.saturating_mul(1_000_000),
        TimeUnit::Microsecond => val.saturating_mul(1_000),
        TimeUnit::Nanosecond => *val,
    }
}

/// Format a DuckDB value into a display string.
fn format_value(v: &duckdb::types::Value) -> String {
    use duckdb::types::Value;
    match v {
        Value::Null => String::new(),
        Value::Boolean(b) => b.to_string(),
        Value::TinyInt(i) => i.to_string(),
        Value::SmallInt(i) => i.to_string(),
        Value::Int(i) => i.to_string(),
        Value::BigInt(i) => i.to_string(),
        Value::HugeInt(i) => i.to_string(),
        Value::UTinyInt(i) => i.to_string(),
        Value::USmallInt(i) => i.to_string(),
        Value::UInt(i) => i.to_string(),
        Value::UBigInt(i) => i.to_string(),
        Value::UHugeInt(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(f) => f.to_string(),
        Value::Decimal(d) => d.to_string(),
        Value::Text(s) => s.clone(),
        Value::Blob(b) => format!("<blob {} bytes>", b.len()),
        Value::Geometry(wkb) => format!("<geometry {} bytes>", wkb.len()),
        Value::Date32(days) => {
            if let Some(epoch) = NaiveDate::from_ymd_opt(1970, 1, 1)
                && let Some(date) = epoch.checked_add_signed(chrono::Duration::days(*days as i64))
            {
                date.format("%Y-%m-%d").to_string()
            } else {
                format!("date({})", days)
            }
        }
        Value::Time64(unit, val) => {
            let total_ns = time_unit_to_nanos(unit, val);
            let seconds = total_ns / 1_000_000_000;
            let nanos = u32::try_from(total_ns % 1_000_000_000).unwrap_or(0);
            match NaiveTime::from_num_seconds_from_midnight_opt(
                u32::try_from(seconds).unwrap_or(0),
                nanos,
            ) {
                Some(t) => t.format("%H:%M:%S").to_string(),
                None => format!("time({:?}, {})", unit, val),
            }
        }
        Value::Timestamp(unit, val) => {
            let total_ns = time_unit_to_nanos(unit, val);
            let secs = total_ns / 1_000_000_000;
            let nsecs = (total_ns % 1_000_000_000) as u32;
            #[allow(deprecated)]
            match NaiveDateTime::from_timestamp_opt(secs, nsecs) {
                Some(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
                None => format!("ts({:?}, {})", unit, val),
            }
        }
        Value::Interval {
            months,
            days,
            nanos,
        } => {
            format!(
                "interval(months={}, days={}, nanos={})",
                months, days, nanos
            )
        }
        Value::List(items) => {
            let parts: Vec<String> = items.iter().map(format_value).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Struct(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{}: {}", k, format_value(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        Value::Map(m) => {
            let parts: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{}: {}", format_value(k), format_value(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(format_value).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Enum(s) => s.clone(),
        Value::Union(inner) => format!("union({})", format_value(inner)),
        other => format!("{other:?}"),
    }
}

/// Try to open a path; on failure returns a user-friendly error.
pub fn open_engine(path: &str) -> Result<ParquetEngine> {
    ParquetEngine::open(path).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("Could not convert") || msg.contains("Conversion") {
            anyhow!("Failed to read parquet file. The file may be corrupt or use an unsupported feature.\n\nDetails: {}", msg)
        } else {
            e
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use duckdb::types::{TimeUnit, Value};

    #[test]
    fn format_value_null() {
        assert_eq!(format_value(&Value::Null), "");
    }

    #[test]
    fn format_value_boolean() {
        assert_eq!(format_value(&Value::Boolean(true)), "true");
        assert_eq!(format_value(&Value::Boolean(false)), "false");
    }

    #[test]
    fn format_value_integers() {
        assert_eq!(format_value(&Value::TinyInt(42)), "42");
        assert_eq!(format_value(&Value::SmallInt(-1)), "-1");
        assert_eq!(format_value(&Value::Int(0)), "0");
        assert_eq!(format_value(&Value::BigInt(i64::MAX)), i64::MAX.to_string());
        assert_eq!(format_value(&Value::UTinyInt(255)), "255");
        assert_eq!(format_value(&Value::USmallInt(65535)), "65535");
        assert_eq!(format_value(&Value::UInt(4_294_967_295u32)), "4294967295");
        assert_eq!(
            format_value(&Value::UBigInt(u64::MAX)),
            u64::MAX.to_string()
        );
    }

    #[test]
    fn format_value_float_double() {
        let float_val = format_value(&Value::Float(3.25));
        assert!(float_val.starts_with("3.25"), "got {}", float_val);
        let double_val = format_value(&Value::Double(0.57721));
        assert!(double_val.starts_with("0.57721"), "got {}", double_val);
        assert_eq!(format_value(&Value::Float(f32::INFINITY)), "inf");
        assert_eq!(format_value(&Value::Float(f32::NEG_INFINITY)), "-inf");
    }

    #[test]
    fn format_value_text() {
        assert_eq!(format_value(&Value::Text("hello".to_string())), "hello");
        assert_eq!(format_value(&Value::Text(String::new())), "");
    }

    #[test]
    fn format_value_blob() {
        assert_eq!(format_value(&Value::Blob(vec![1, 2, 3])), "<blob 3 bytes>");
        assert_eq!(format_value(&Value::Blob(vec![])), "<blob 0 bytes>");
    }

    #[test]
    fn format_value_date32() {
        assert_eq!(format_value(&Value::Date32(0)), "1970-01-01");
        assert_eq!(format_value(&Value::Date32(365)), "1971-01-01");
    }

    #[test]
    fn format_value_time64() {
        assert_eq!(
            format_value(&Value::Time64(TimeUnit::Second, 0)),
            "00:00:00"
        );
        assert_eq!(
            format_value(&Value::Time64(TimeUnit::Second, 3600)),
            "01:00:00"
        );
        assert_eq!(
            format_value(&Value::Time64(TimeUnit::Millisecond, 5_000)),
            "00:00:05"
        );
        assert_eq!(
            format_value(&Value::Time64(TimeUnit::Microsecond, 5_000_000)),
            "00:00:05"
        );
        assert_eq!(
            format_value(&Value::Time64(TimeUnit::Nanosecond, 5_000_000_000)),
            "00:00:05"
        );
    }

    #[test]
    fn format_value_timestamp() {
        assert_eq!(
            format_value(&Value::Timestamp(TimeUnit::Second, 0)),
            "1970-01-01 00:00:00"
        );
        assert_eq!(
            format_value(&Value::Timestamp(TimeUnit::Millisecond, 1_000)),
            "1970-01-01 00:00:01"
        );
        assert_eq!(
            format_value(&Value::Timestamp(TimeUnit::Microsecond, 1_000_000)),
            "1970-01-01 00:00:01"
        );
        assert_eq!(
            format_value(&Value::Timestamp(TimeUnit::Nanosecond, 1_000_000_000)),
            "1970-01-01 00:00:01"
        );
    }

    #[test]
    fn format_value_interval() {
        let val = Value::Interval {
            months: 1,
            days: 2,
            nanos: 3,
        };
        assert_eq!(format_value(&val), "interval(months=1, days=2, nanos=3)");
    }

    #[test]
    fn format_value_list() {
        assert_eq!(
            format_value(&Value::List(vec![Value::Int(1), Value::Int(2)])),
            "[1, 2]"
        );
        assert_eq!(format_value(&Value::List(vec![])), "[]");
    }

    #[test]
    fn format_value_struct() {
        assert_eq!(
            format_value(&Value::Struct(
                vec![
                    ("x".to_string(), Value::Int(10)),
                    ("y".to_string(), Value::Text("z".to_string())),
                ]
                .into()
            )),
            "{x: 10, y: z}"
        );
        assert_eq!(format_value(&Value::Struct(vec![].into())), "{}");
    }

    #[test]
    fn format_value_enum() {
        assert_eq!(format_value(&Value::Enum("RED".to_string())), "RED");
    }

    #[test]
    fn format_value_array() {
        assert_eq!(
            format_value(&Value::Array(vec![
                Value::Boolean(true),
                Value::Boolean(false)
            ])),
            "[true, false]"
        );
    }

    #[test]
    fn format_value_union() {
        assert_eq!(
            format_value(&Value::Union(Box::new(Value::Int(42)))),
            "union(42)"
        );
    }

    #[test]
    fn escape_sql_string_no_special_chars() {
        assert_eq!(escape_sql_string("hello"), "hello");
    }

    #[test]
    fn escape_sql_string_with_single_quotes() {
        assert_eq!(escape_sql_string("it's"), "it''s");
        assert_eq!(escape_sql_string("'hello'"), "''hello''");
        assert_eq!(escape_sql_string("'"), "''");
    }

    #[test]
    fn escape_sql_string_empty() {
        assert_eq!(escape_sql_string(""), "");
    }

    #[test]
    fn validate_and_trim_sql_valid_select() {
        assert_eq!(
            validate_and_trim_sql("SELECT * FROM t").unwrap(),
            "SELECT * FROM t"
        );
    }

    #[test]
    fn validate_and_trim_sql_valid_with() {
        let sql = "WITH cte AS (SELECT 1) SELECT * FROM cte";
        assert_eq!(validate_and_trim_sql(sql).unwrap(), sql);
    }

    #[test]
    fn validate_and_trim_sql_rejects_ddl() {
        let err = validate_and_trim_sql("DELETE FROM t").unwrap_err();
        assert!(
            err.to_string()
                .contains("Only SELECT/WITH queries are supported"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_and_trim_sql_empty() {
        let err = validate_and_trim_sql("").unwrap_err();
        assert!(
            err.to_string().contains("SQL query is empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_and_trim_sql_whitespace_only() {
        let err = validate_and_trim_sql("   ").unwrap_err();
        assert!(
            err.to_string().contains("SQL query is empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_and_trim_sql_trailing_semicolon() {
        assert_eq!(
            validate_and_trim_sql("SELECT * FROM t;").unwrap(),
            "SELECT * FROM t"
        );
    }

    #[test]
    fn validate_and_trim_sql_leading_trailing_whitespace() {
        assert_eq!(
            validate_and_trim_sql("  SELECT * FROM t  ").unwrap(),
            "SELECT * FROM t"
        );
    }

    #[test]
    fn strip_optional_where_prefix_present() {
        assert_eq!(strip_optional_where_prefix("WHERE age > 50"), "age > 50");
    }

    #[test]
    fn strip_optional_where_prefix_absent() {
        assert_eq!(strip_optional_where_prefix("age > 50"), "age > 50");
    }

    #[test]
    fn strip_optional_where_prefix_mixed_case() {
        assert_eq!(strip_optional_where_prefix("where age > 50"), "age > 50");
        assert_eq!(strip_optional_where_prefix("WHERE age > 50"), "age > 50");
        assert_eq!(strip_optional_where_prefix("Where age > 50"), "age > 50");
    }

    #[test]
    fn strip_optional_where_prefix_trimmed() {
        assert_eq!(
            strip_optional_where_prefix("  WHERE age > 50  "),
            "age > 50"
        );
    }

    #[test]
    fn strip_optional_where_prefix_exact_where() {
        assert_eq!(strip_optional_where_prefix("WHERE"), "");
    }
}
