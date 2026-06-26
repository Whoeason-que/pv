pub mod metadata;
pub mod types;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use duckdb::Connection;
use std::path::{Path, PathBuf};

pub const SOURCE_VIEW_NAME: &str = "pv_data";

/// A single field/column in the parquet file.
#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    #[allow(dead_code)]
    pub duckdb_type: String,
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

    #[allow(dead_code)]
    pub fn is_folder(&self) -> bool {
        self.is_folder
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

    /// Borrow the underlying connection (for metadata queries).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Read rows with optional field selection, WHERE filter, offset and limit.
    /// Returns a vector of rows, each row being a vector of string-formatted cell values.
    #[allow(dead_code)]
    pub fn read_rows(
        &self,
        selected_fields: &[String],
        where_filter: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Vec<String>>> {
        Ok(self
            .read_rows_query(selected_fields, where_filter, offset, limit, None)?
            .rows)
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
            let filter = strip_where_prefix(filter.trim());
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
        let sql = normalize_user_select_sql(sql)?;
        let mut query = format!("SELECT * FROM ({}) AS pv_query", sql);
        let col_count = query_column_count(&self.conn, &query)?;
        push_sort_clause(&mut query, sort, col_count)?;
        query.push_str(&format!(" LIMIT {} OFFSET {}", limit.max(0), offset.max(0)));
        run_query(&self.conn, &query)
    }

    pub fn count_sql(&self, sql: &str) -> Result<i64> {
        let sql = normalize_user_select_sql(sql)?;
        let query = format!("SELECT count(*) FROM ({}) AS pv_count", sql);
        let count: i64 = self.conn.query_row(&query, [], |row| row.get(0))?;
        Ok(count)
    }

    /// Generate a CREATE TABLE SQL script from the current schema.
    #[allow(dead_code)]
    pub fn create_table_script(&self, table_name: &str) -> Result<String> {
        let table_name = make_column_safe(table_name);
        let mut sql = format!("CREATE TABLE {} (\n", table_name);
        let lines: Vec<String> = self
            .fields
            .iter()
            .map(|f| format!("    {} {}", make_column_safe(&f.name), f.duckdb_type))
            .collect();
        sql.push_str(&lines.join(",\n"));
        sql.push_str("\n);");
        Ok(sql)
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
    let stmt = rows.as_ref().expect("query statement should be available");
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
    Ok(rows
        .as_ref()
        .expect("query statement should be available")
        .column_count())
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

fn normalize_user_select_sql(sql: &str) -> Result<String> {
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

fn strip_where_prefix(filter: &str) -> &str {
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
        let type_name: String = row.get(1)?;
        Ok(Field {
            name,
            duckdb_type: type_name,
        })
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
    while let Some(d) = stack.pop() {
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

/// Escape single quotes in a string for safe use inside a DuckDB single-quoted string literal.
fn escape_sql_string(s: &str) -> String {
    s.replace('\'', "''")
}

/// Escape a column name for safe use in DuckDB SQL by wrapping in double quotes.
pub fn make_column_safe(column_name: &str) -> String {
    let escaped = column_name.replace('"', "\"\"");
    format!("\"{}\"", escaped)
}

/// Format a DuckDB value into a display string.
fn format_value(v: &duckdb::types::Value) -> String {
    use duckdb::types::{TimeUnit, Value};
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
        Value::Float(f) => f.to_string(),
        Value::Double(f) => f.to_string(),
        Value::Decimal(d) => d.to_string(),
        Value::Text(s) => s.clone(),
        Value::Blob(b) => format!("<blob {} bytes>", b.len()),
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
            let total_ns: i64 = match unit {
                TimeUnit::Second => val.saturating_mul(1_000_000_000),
                TimeUnit::Millisecond => val.saturating_mul(1_000_000),
                TimeUnit::Microsecond => val.saturating_mul(1_000),
                TimeUnit::Nanosecond => *val,
            };
            let seconds = (total_ns / 1_000_000_000) as u32;
            let nanos = (total_ns % 1_000_000_000) as u32;
            match NaiveTime::from_num_seconds_from_midnight_opt(seconds, nanos) {
                Some(t) => t.format("%H:%M:%S").to_string(),
                None => format!("time({:?}, {})", unit, val),
            }
        }
        Value::Timestamp(unit, val) => {
            let total_ns: i64 = match unit {
                TimeUnit::Second => val.saturating_mul(1_000_000_000),
                TimeUnit::Millisecond => val.saturating_mul(1_000_000),
                TimeUnit::Microsecond => val.saturating_mul(1_000),
                TimeUnit::Nanosecond => *val,
            };
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
