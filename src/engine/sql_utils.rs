/// Escape single quotes in a string for safe use inside a DuckDB single-quoted string literal.
pub fn escape_sql_string(s: &str) -> String {
    s.replace('\'', "''")
}

/// Escape a column name for safe use in DuckDB SQL by wrapping in double quotes.
pub fn make_column_safe(column_name: &str) -> String {
    let escaped = column_name.replace('"', "\"\"");
    format!("\"{}\"", escaped)
}
