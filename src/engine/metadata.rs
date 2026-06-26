use anyhow::Result;
use duckdb::Connection;
use serde::Serialize;
use std::collections::VecDeque;

/// A node in the parquet schema tree.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaNode {
    pub name: String,
    pub path: String,
    pub type_name: String,
    pub repetition_type: String,
    pub num_children: i64,
    pub children: Vec<SchemaNode>,
}

/// Row group metadata.
#[derive(Debug, Clone, Serialize)]
pub struct RowGroupInfo {
    pub row_group_id: i64,
    pub row_num: i64,
    pub total_byte_size: i64,
    pub total_compressed_size: i64,
}

/// Key-value metadata entry.
#[derive(Debug, Clone, Serialize)]
pub struct KvMetadata {
    pub key: String,
    pub value: String,
}

/// Full metadata for a parquet file/folder.
#[derive(Debug, Clone, Serialize)]
pub struct ParquetMetadata {
    pub schema_tree: SchemaNode,
    pub row_groups: Vec<RowGroupInfo>,
    pub kv_metadata: Vec<KvMetadata>,
}

impl ParquetMetadata {
    /// Load metadata from DuckDB using parquet_schema/parquet_metadata/parquet_kv_metadata functions.
    pub fn load(conn: &Connection, read_path: &str) -> Result<Self> {
        let schema_tree = load_schema_tree(conn, read_path)?;
        let row_groups = load_row_groups(conn, read_path)?;
        let kv_metadata = load_kv_metadata(conn, read_path)?;
        Ok(Self {
            schema_tree,
            row_groups,
            kv_metadata,
        })
    }
}

fn load_schema_tree(conn: &Connection, read_path: &str) -> Result<SchemaNode> {
    let query = format!(
        "SELECT name, name AS path, COALESCE(type, ''), COALESCE(repetition_type, ''), num_children FROM parquet_schema({})",
        read_path
    );
    let mut stmt = conn.prepare(&query)?;

    let rows: Vec<(String, String, String, String, Option<i64>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    if rows.is_empty() {
        anyhow::bail!("Failed to retrieve parquet schema");
    }

    let mut nodes: VecDeque<SchemaNode> = rows
        .into_iter()
        .map(
            |(name, path, type_name, repetition_type, num_children)| SchemaNode {
                name,
                path,
                type_name,
                repetition_type,
                num_children: num_children.unwrap_or(0),
                children: Vec::new(),
            },
        )
        .collect();

    let root = nodes.pop_front().expect("at least root node in schema");
    let root = build_tree(root, &mut nodes);

    Ok(root)
}

fn build_tree(mut parent: SchemaNode, remaining: &mut VecDeque<SchemaNode>) -> SchemaNode {
    let target_children = parent.num_children as usize;
    for _ in 0..target_children {
        if remaining.is_empty() {
            break;
        }
        let node = remaining.pop_front().expect("already checked non-empty");
        let node = if node.num_children > 0 {
            build_tree(node, remaining)
        } else {
            node
        };
        parent.children.push(node);
    }
    parent
}

fn load_row_groups(conn: &Connection, read_path: &str) -> Result<Vec<RowGroupInfo>> {
    let query = format!(
        "SELECT DISTINCT row_group_id, row_group_num_rows, row_group_bytes, row_group_compressed_bytes FROM parquet_metadata({})",
        read_path
    );
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], |row| {
        Ok(RowGroupInfo {
            row_group_id: row.get(0)?,
            row_num: row.get(1)?,
            total_byte_size: row.get(2)?,
            total_compressed_size: row.get(3)?,
        })
    })?;

    let mut groups = Vec::new();
    for row in rows {
        groups.push(row?);
    }
    Ok(groups)
}

fn load_kv_metadata(conn: &Connection, read_path: &str) -> Result<Vec<KvMetadata>> {
    let query = format!("SELECT key, value FROM parquet_kv_metadata({})", read_path);
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], |row| {
        let key: Vec<u8> = row.get(0)?;
        let value: Vec<u8> = row.get(1)?;
        Ok(KvMetadata {
            key: String::from_utf8_lossy(&key).into_owned(),
            value: String::from_utf8_lossy(&value).into_owned(),
        })
    })?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
}
