use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};

use crate::app::App;
use crate::engine::types::format_bytes;
use crate::ui::centered_rect;
use crate::ui::theme::*;

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let popup = centered_rect(90, 85, area);
    f.render_widget(Clear, popup);

    let mut lines: Vec<String> = Vec::new();

    if let Some(meta) = &app.metadata {
        lines.push("=== Schema Tree ===".to_string());
        render_schema(&meta.schema_tree, 0, &mut lines);
        lines.push(String::new());
        lines.push(format!("=== Row Groups ({}) ===", meta.row_groups.len()));
        for rg in &meta.row_groups {
            lines.push(format!(
                "  #{}: {} rows, {} ({})",
                rg.row_group_id,
                rg.row_num,
                format_bytes(rg.total_byte_size),
                format_bytes(rg.total_compressed_size)
            ));
        }
        lines.push(String::new());
        lines.push(format!(
            "=== Key-Value Metadata ({}) ===",
            meta.kv_metadata.len()
        ));
        for kv in &meta.kv_metadata {
            let val_preview = if kv.value.chars().count() > 80 {
                format!("{}...", kv.value.chars().take(80).collect::<String>())
            } else {
                kv.value.clone()
            };
            lines.push(format!("  {}: {}", kv.key, val_preview));
        }
    } else {
        lines.push("Loading metadata...".to_string());
    }

    let items: Vec<ListItem> = lines
        .iter()
        .map(|l| ListItem::new(l.as_str()).style(Style::default().fg(TEXT_PRIMARY)))
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("[ Metadata — j/k or ↑/↓ to scroll, Esc to close ]")
                .style(Style::default().fg(BLOCK_TITLE)),
        )
        .highlight_style(Style::default().add_modifier(MOD_HIGHLIGHT));

    let mut state = ListState::default();
    state.select(Some(app.meta_scroll));
    f.render_stateful_widget(list, popup, &mut state);
}

fn render_schema(
    node: &crate::engine::metadata::SchemaNode,
    depth: usize,
    lines: &mut Vec<String>,
) {
    let indent = "  ".repeat(depth);
    let marker = if depth == 0 { "" } else { "├─ " };
    lines.push(format!(
        "{}{}{} [{}] {} (children: {})",
        indent, marker, node.name, node.type_name, node.repetition_type, node.num_children
    ));
    for child in &node.children {
        render_schema(child, depth + 1, lines);
    }
}

pub fn scroll(app: &mut App, delta: i32) {
    let cur = app.meta_scroll as i32;
    let new = (cur + delta).max(0);
    app.meta_scroll = new as usize;
}
