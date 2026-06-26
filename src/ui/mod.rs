pub mod field_select;
pub mod help;
pub mod input;
pub mod metadata_view;
pub mod table_view;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, DEFAULT_SQL, Focus, Mode, OperationLevel};
use crate::engine::SortDirection;
use crate::ui::theme::*;

pub mod theme;

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Draw the entire UI based on current app state.
pub fn draw(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // If in an overlay/input mode that takes full screen, draw it
    match app.mode {
        Mode::FieldSelect => {
            field_select::draw(f, app, size);
            return;
        }
        Mode::MetadataView => {
            metadata_view::draw(f, app, size);
            return;
        }
        Mode::Operations => {
            draw_operations_view(f, app, size);
            return;
        }
        Mode::Help => {
            help::draw(f, size);
            return;
        }
        _ => {}
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Length(if app.mode == Mode::ExportInput { 3 } else { 0 }),
        ])
        .split(size);

    draw_input_bar(f, app, chunks[0], BarKind::Path);
    draw_input_bar(f, app, chunks[1], BarKind::Sql);
    table_view::draw(f, app, chunks[2]);
    draw_action_bar(f, chunks[3]);

    if app.mode == Mode::ExportInput {
        input::draw(f, app, chunks[4]);
    }
}

fn focus_style(is_focused: bool, color: Color) -> Style {
    if is_focused {
        Style::default().fg(FOCUSED).add_modifier(MOD_FOCUSED)
    } else {
        Style::default().fg(color)
    }
}

enum BarKind {
    Path,
    Sql,
}

fn set_input_cursor(f: &mut Frame, area: Rect, input_buffer: &str) {
    let cursor_x = area
        .x
        .saturating_add(1)
        .saturating_add(input_buffer.chars().count() as u16)
        .min(area.right().saturating_sub(2));
    f.set_cursor_position((cursor_x, area.y.saturating_add(1)));
}

fn draw_input_bar(f: &mut Frame, app: &App, area: Rect, kind: BarKind) {
    let (text, title, focus, cursor) = match kind {
        BarKind::Path => {
            let text = if app.mode == Mode::OpenInput {
                app.input_buffer.as_str().to_string()
            } else {
                app.path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "no file open".to_string())
            };
            let title = if app.mode == Mode::OpenInput {
                "[ path — Esc cancel, Enter open ]"
            } else {
                "[ path Ctrl+P ]"
            };
            let cursor = app.mode == Mode::OpenInput;
            (
                text,
                title,
                focus_style(app.focus == Focus::Path, FOCUS_PATH_UNFOCUSED),
                cursor,
            )
        }
        BarKind::Sql => {
            let text = if app.mode == Mode::SqlInput {
                app.input_buffer.as_str().to_string()
            } else if app.is_table_mode() {
                DEFAULT_SQL.to_string()
            } else {
                app.sql_query.clone()
            };
            let title = if app.mode == Mode::SqlInput {
                "[ sql — Esc cancel, Enter submit ]"
            } else {
                "[ sql / Ctrl+S ]"
            };
            let cursor = app.mode == Mode::SqlInput;
            (
                text,
                title,
                focus_style(app.focus == Focus::Sql, FOCUS_SQL_UNFOCUSED),
                cursor,
            )
        }
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(focus);
    let paragraph = Paragraph::new(text)
        .block(block)
        .style(Style::default().fg(TEXT_PRIMARY));
    f.render_widget(paragraph, area);

    if cursor {
        set_input_cursor(f, area, &app.input_buffer);
    }
}

fn draw_action_bar(f: &mut Frame, area: Rect) {
    let actions = [
        ("Esc", "quit"),
        ("Tab", "focus"),
        ("Ctrl+R", "reload"),
        ("Ctrl+X", "cursor"),
        ("/", "edit SQL"),
        ("?", "help"),
    ];
    let mut spans = Vec::new();
    for (key, label) in actions {
        spans.push(Span::styled(
            key,
            Style::default().fg(ACTION_KEY).add_modifier(MOD_FOCUSED),
        ));
        spans.push(Span::styled(
            format!(" {}  ", label),
            Style::default().fg(ACTION_LABEL),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_operations_view(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("[ operations — Ctrl+B/Esc to return ]")
        .style(Style::default().fg(OPERATIONS_INFO));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let visible_logs = inner.height.saturating_sub(1) as usize;
    let mut lines: Vec<Line> = app
        .operation_log
        .iter()
        .rev()
        .skip(app.operation_log_scroll)
        .take(visible_logs)
        .map(|entry| {
            let color = match entry.level {
                OperationLevel::Info => OPERATIONS_INFO,
                OperationLevel::Success => OPERATIONS_SUCCESS,
                OperationLevel::Error => OPERATIONS_ERROR,
            };
            let detail = entry
                .detail
                .as_ref()
                .map(|d| format!(" — {}", d))
                .unwrap_or_default();
            Line::from(vec![Span::styled(
                format!(
                    "#{:03} {:?} {:?}: {}{}",
                    entry.id, entry.kind, entry.outcome, entry.summary, detail
                ),
                Style::default().fg(color),
            )])
        })
        .collect();

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No operations yet",
            Style::default().fg(OPERATIONS_INFO),
        )));
    }
    lines.push(Line::from(Span::styled(
        status_summary(app),
        Style::default().fg(STATUS_LINE),
    )));

    let para = Paragraph::new(lines);
    f.render_widget(para, inner);
}

fn status_summary(app: &App) -> String {
    let mode = if app.is_table_mode() { "table" } else { "sql" };
    let sort_info = app
        .sort
        .as_ref()
        .map(|sort| {
            let direction = match sort.direction {
                SortDirection::Asc => "asc",
                SortDirection::Desc => "desc",
            };
            format!(" | sort: {} {}", sort.column_name, direction)
        })
        .unwrap_or_default();
    format!(
        "mode: {} | rows: {} | total: {} | fields: {} | offset: {} | partitions: {} | page: {}{}",
        mode,
        app.rows.len(),
        app.record_count,
        app.headers.len(),
        app.offset,
        app.partition_count,
        app.page_size,
        sort_info,
    )
}
