use anyhow::Result;
use std::path::Path;

/// Export data to a file. Format is inferred from the file extension.
/// - `.csv` → CSV
/// - `.json` → JSON (array of objects)
/// - `.xlsx` → Excel
pub fn export(headers: &[String], rows: &[Vec<String>], path: &Path) -> Result<()> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "csv" => export_csv(headers, rows, path),
        "json" => export_json(headers, rows, path),
        "xlsx" => export_xlsx(headers, rows, path),
        other => anyhow::bail!("Unsupported export format: .{}", other),
    }
}

fn export_csv(headers: &[String], rows: &[Vec<String>], path: &Path) -> Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record(headers)?;
    for row in rows {
        writer.write_record(row)?;
    }
    writer.flush()?;
    Ok(())
}

fn export_json(headers: &[String], rows: &[Vec<String>], path: &Path) -> Result<()> {
    let mut arr = Vec::with_capacity(rows.len());
    for row in rows {
        let mut obj = serde_json::Map::new();
        for (i, header) in headers.iter().enumerate() {
            let val = row.get(i).cloned().unwrap_or_default();
            obj.insert(header.clone(), serde_json::Value::String(val));
        }
        arr.push(serde_json::Value::Object(obj));
    }
    let json = serde_json::to_string_pretty(&arr)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn export_xlsx(headers: &[String], rows: &[Vec<String>], path: &Path) -> Result<()> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let worksheet = workbook.add_worksheet();
    for (c, header) in headers.iter().enumerate() {
        worksheet.write_string(0, c as u16, header)?;
    }
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            worksheet.write_string((r + 1) as u32, c as u16, cell)?;
        }
    }
    workbook.save(path)?;
    Ok(())
}
