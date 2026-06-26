use parquet_tui::engine::metadata::ParquetMetadata;
use parquet_tui::engine::{ParquetEngine, SortDirection, SortSpec, make_column_safe};
use parquet_tui::export;
use std::path::Path;

fn testdata(path: &str) -> String {
    format!("{}/testdata/{}", env!("CARGO_MANIFEST_DIR"), path)
}

fn field_names(engine: &ParquetEngine) -> Vec<String> {
    engine.fields().iter().map(|f| f.name.clone()).collect()
}

#[test]
fn open_basic_types_has_6_fields_and_1000_records() {
    let engine = ParquetEngine::open(testdata("basic_types.parquet"))
        .expect("failed to open basic_types.parquet");
    assert_eq!(engine.fields().len(), 6, "basic_types should have 6 fields");
    assert_eq!(
        engine.record_count(),
        1000,
        "basic_types should have 1000 records"
    );
}

#[test]
fn read_first_10_rows_all_fields() {
    let engine = ParquetEngine::open(testdata("basic_types.parquet")).unwrap();
    let fields = field_names(&engine);
    let rows = engine
        .read_rows(&fields, None, 0, 10)
        .expect("read_rows failed");
    assert_eq!(rows.len(), 10, "should read exactly 10 rows");
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(
            row.len(),
            fields.len(),
            "row {} should have one cell per field",
            i
        );
    }
}

#[test]
fn read_rows_with_where_filter_age_gt_50() {
    let engine = ParquetEngine::open(testdata("basic_types.parquet")).unwrap();
    let fields = field_names(&engine);
    let age_idx = fields
        .iter()
        .position(|f| f == "age")
        .expect("age field should exist");
    let rows = engine
        .read_rows(&fields, Some("age > 50"), 0, 1000)
        .expect("filtered read_rows failed");
    assert!(!rows.is_empty(), "filter age > 50 should return some rows");
    for row in &rows {
        let age_str = &row[age_idx];
        let age: i64 = age_str
            .parse()
            .unwrap_or_else(|_| panic!("age value '{}' should be an integer", age_str));
        assert!(age > 50, "row age {} should be > 50", age);
    }
}

#[test]
fn field_selection_id_and_name_yields_two_columns() {
    let engine = ParquetEngine::open(testdata("basic_types.parquet")).unwrap();
    let selected = vec!["id".to_string(), "name".to_string()];
    let rows = engine
        .read_rows(&selected, None, 0, 10)
        .expect("read_rows with field selection failed");
    assert!(!rows.is_empty(), "should return some rows");
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(row.len(), 2, "row {} should have exactly 2 columns", i);
    }
}

#[test]
fn open_nested_types_has_5_fields_and_100_records() {
    let engine = ParquetEngine::open(testdata("nested_types.parquet"))
        .expect("failed to open nested_types.parquet");
    assert_eq!(
        engine.fields().len(),
        5,
        "nested_types should have 5 fields"
    );
    assert_eq!(
        engine.record_count(),
        100,
        "nested_types should have 100 records"
    );
}

#[test]
fn read_nested_types_does_not_panic() {
    let engine = ParquetEngine::open(testdata("nested_types.parquet")).unwrap();
    let fields = field_names(&engine);
    let rows = engine
        .read_rows(&fields, None, 0, 100)
        .expect("read_rows on nested types failed");
    assert_eq!(rows.len(), 100, "should read all 100 nested rows");
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(
            row.len(),
            5,
            "nested row {} should have 5 formatted cells",
            i
        );
        for cell in row {
            let _ = cell.len();
        }
    }
}

#[test]
fn open_nullable_types_has_200_records() {
    let engine = ParquetEngine::open(testdata("nullable_types.parquet"))
        .expect("failed to open nullable_types.parquet");
    assert_eq!(
        engine.record_count(),
        200,
        "nullable_types should have 200 records"
    );
    let fields = field_names(&engine);
    let rows = engine
        .read_rows(&fields, None, 0, 200)
        .expect("read_rows on nullable types failed");
    assert_eq!(rows.len(), 200);
}

#[test]
fn open_large_file_has_100000_records() {
    let engine = ParquetEngine::open(testdata("large_file.parquet"))
        .expect("failed to open large_file.parquet");
    assert_eq!(
        engine.record_count(),
        100000,
        "large_file should have 100000 records"
    );
}

#[test]
fn pagination_offset_500_limit_50_returns_50_rows() {
    let engine = ParquetEngine::open(testdata("basic_types.parquet")).unwrap();
    let fields = field_names(&engine);
    let rows = engine
        .read_rows(&fields, None, 500, 50)
        .expect("paginated read_rows failed");
    assert_eq!(rows.len(), 50, "pagination should return exactly 50 rows");
}

#[test]
fn metadata_schema_tree_has_children() {
    let engine = ParquetEngine::open(testdata("basic_types.parquet")).unwrap();
    let metadata = ParquetMetadata::load(engine.conn(), &engine.read_path_spec())
        .expect("metadata load failed");
    assert!(
        !metadata.schema_tree.children.is_empty(),
        "schema_tree root should have children"
    );
    assert_eq!(
        metadata.schema_tree.children.len(),
        6,
        "schema_tree should have 6 children for basic_types"
    );
    assert!(!metadata.row_groups.is_empty(), "should have row groups");
}

#[test]
fn create_table_script_contains_create_table() {
    let engine = ParquetEngine::open(testdata("basic_types.parquet")).unwrap();
    let script = engine
        .create_table_script("my_table")
        .expect("create_table_script failed");
    assert!(
        script.contains("CREATE TABLE"),
        "script should contain 'CREATE TABLE': {}",
        script
    );
    assert!(
        script.contains("\"my_table\""),
        "script should contain the quoted table name: {}",
        script
    );
}

#[test]
fn export_csv_writes_nonempty_file() {
    let engine = ParquetEngine::open(testdata("basic_types.parquet")).unwrap();
    let fields = field_names(&engine);
    let rows = engine
        .read_rows(&fields, None, 0, 25)
        .expect("read_rows for export failed");
    assert_eq!(rows.len(), 25);

    let path = Path::new("/tmp/test_export.csv");
    let _ = std::fs::remove_file(path);
    export::export(&fields, &rows, path).expect("csv export failed");

    let metadata = std::fs::metadata(path).expect("exported csv file should exist");
    assert!(metadata.len() > 0, "exported csv should be non-empty");

    let content = std::fs::read_to_string(path).expect("should read exported csv");
    assert!(
        content.contains("id") && content.contains("name"),
        "csv should contain header fields: {}",
        content
    );
    let line_count = content.lines().count();
    assert!(
        line_count >= 26,
        "csv should have at least 26 lines, got {}",
        line_count
    );
}

#[test]
fn export_json_writes_valid_json() {
    let engine = ParquetEngine::open(testdata("basic_types.parquet")).unwrap();
    let fields = field_names(&engine);
    let rows = engine
        .read_rows(&fields, None, 0, 15)
        .expect("read_rows for json export failed");
    assert_eq!(rows.len(), 15);

    let path = Path::new("/tmp/test_export.json");
    let _ = std::fs::remove_file(path);
    export::export(&fields, &rows, path).expect("json export failed");

    let content = std::fs::read_to_string(path).expect("should read exported json");
    let parsed: serde_json::Value =
        serde_json::from_str(&content).expect("exported json should be valid JSON");
    let arr = parsed
        .as_array()
        .expect("exported json should be an array of objects");
    assert_eq!(arr.len(), 15, "json array should have 15 entries");
    for (i, obj) in arr.iter().enumerate() {
        let map = obj
            .as_object()
            .unwrap_or_else(|| panic!("entry {} should be a JSON object", i));
        assert_eq!(map.len(), fields.len(), "entry {} field count mismatch", i);
    }
}

#[test]
fn open_partitioned_folder_returns_records() {
    let engine =
        ParquetEngine::open(testdata("partitioned")).expect("failed to open partitioned folder");
    assert!(
        engine.record_count() > 0,
        "partitioned folder should have records > 0"
    );
    assert!(
        engine.partition_count() >= 3,
        "partitioned folder should have at least 3 parquet files, got {}",
        engine.partition_count()
    );
    let fields = field_names(&engine);
    let rows = engine
        .read_rows(&fields, None, 0, 10)
        .expect("read_rows on partitioned folder failed");
    assert_eq!(rows.len(), 10);
}

#[test]
fn query_sql_returns_dynamic_headers_and_rows() {
    let engine = ParquetEngine::open(testdata("basic_types.parquet")).unwrap();
    let result = engine
        .query_sql(
            "SELECT id AS user_id, name FROM pv_data WHERE age > 50",
            0,
            5,
            None,
        )
        .expect("query_sql failed");
    assert_eq!(
        result.headers,
        vec!["user_id".to_string(), "name".to_string()]
    );
    assert_eq!(result.rows.len(), 5);
    for row in &result.rows {
        assert_eq!(row.len(), 2);
        assert!(!row[0].is_empty());
        assert!(!row[1].is_empty());
    }
}

#[test]
fn count_sql_returns_filtered_count() {
    let engine = ParquetEngine::open(testdata("basic_types.parquet")).unwrap();
    let count = engine
        .count_sql("SELECT id FROM pv_data WHERE age > 50")
        .expect("count_sql failed");
    let rows = engine
        .read_rows(&field_names(&engine), Some("age > 50"), 0, 1000)
        .expect("read_rows failed");
    assert_eq!(count, rows.len() as i64);
    assert!(count > 0);
}

#[test]
fn query_sql_rejects_non_select() {
    let engine = ParquetEngine::open(testdata("basic_types.parquet")).unwrap();
    let err = engine
        .query_sql("DELETE FROM pv_data", 0, 10, None)
        .expect_err("non-SELECT query should fail");
    assert!(
        err.to_string()
            .contains("Only SELECT/WITH queries are supported"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn read_rows_query_sorts_desc_by_age() {
    let engine = ParquetEngine::open(testdata("basic_types.parquet")).unwrap();
    let selected = vec!["id".to_string(), "age".to_string(), "name".to_string()];
    let sort = SortSpec {
        column_index: 1,
        column_name: "age".to_string(),
        direction: SortDirection::Desc,
    };
    let result = engine
        .read_rows_query(&selected, None, 0, 25, Some(&sort))
        .expect("read_rows_query failed");
    assert_eq!(result.headers, selected);
    assert_eq!(result.rows.len(), 25);
    let ages: Vec<i64> = result
        .rows
        .iter()
        .map(|row| row[1].parse::<i64>().expect("age should parse"))
        .collect();
    assert!(ages.windows(2).all(|pair| pair[0] >= pair[1]), "{:?}", ages);
}

#[test]
fn query_sql_sorts_asc_by_age() {
    let engine = ParquetEngine::open(testdata("basic_types.parquet")).unwrap();
    let sort = SortSpec {
        column_index: 1,
        column_name: "age".to_string(),
        direction: SortDirection::Asc,
    };
    let result = engine
        .query_sql("SELECT id, age, name FROM pv_data", 0, 25, Some(&sort))
        .expect("query_sql failed");
    assert_eq!(
        result.headers,
        vec!["id".to_string(), "age".to_string(), "name".to_string()]
    );
    assert_eq!(result.rows.len(), 25);
    let ages: Vec<i64> = result
        .rows
        .iter()
        .map(|row| row[1].parse::<i64>().expect("age should parse"))
        .collect();
    assert!(ages.windows(2).all(|pair| pair[0] <= pair[1]), "{:?}", ages);
}

#[test]
fn make_column_safe_escapes_double_quotes() {
    let safe = make_column_safe("col\"name");
    assert_eq!(safe, "\"col\"\"name\"");

    assert_eq!(make_column_safe("id"), "\"id\"");

    assert_eq!(make_column_safe(""), "\"\"");
}
