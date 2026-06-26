pub mod field_select;
pub mod help;
pub mod input;
pub mod metadata_view;
pub mod table_view;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, DEFAULT_SQL, Focus, Mode, OperationLevel};
use crate::engine::SortDirection;

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

    draw_path_bar(f, app, chunks[0]);
    draw_sql_bar(f, app, chunks[1]);
    table_view::draw(f, app, chunks[2]);
    draw_action_bar(f, chunks[3]);

    if app.mode == Mode::ExportInput {
        input::draw(f, app, chunks[4]);
    }
}

fn focus_style(is_focused: bool, color: Color) -> Style {
    if is_focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(color)
    }
}

fn draw_path_bar(f: &mut Frame, app: &App, area: Rect) {
    let path = if app.mode == Mode::OpenInput {
        app.input_buffer.as_str().to_string()
    } else {
        app.path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "no file open".to_string())
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(if app.mode == Mode::OpenInput {
            "[ path — Esc cancel, Enter open ]"
        } else {
            "[ path Ctrl+P ]"
        })
        .style(focus_style(app.focus == Focus::Path, Color::Blue));
    f.render_widget(
        Paragraph::new(path)
            .block(block)
            .style(Style::default().fg(Color::White)),
        area,
    );

    if app.mode == Mode::OpenInput {
        let cursor_x = area
            .x
            .saturating_add(1)
            .saturating_add(app.input_buffer.chars().count() as u16)
            .min(area.right().saturating_sub(2));
        f.set_cursor_position((cursor_x, area.y.saturating_add(1)));
    }
}

fn draw_sql_bar(f: &mut Frame, app: &App, area: Rect) {
    let sql = if app.mode == Mode::SqlInput {
        app.input_buffer.as_str()
    } else if app.sql_query.is_empty() {
        DEFAULT_SQL
    } else {
        &app.sql_query
    };
    let text = sql.to_string();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(if app.mode == Mode::SqlInput {
            "[ sql — Esc cancel, Enter submit ]"
        } else {
            "[ sql / Ctrl+S ]"
        })
        .style(focus_style(app.focus == Focus::Sql, Color::Magenta));
    let paragraph = Paragraph::new(text)
        .block(block)
        .style(Style::default().fg(Color::White));
    f.render_widget(paragraph, area);

    if app.mode == Mode::SqlInput {
        let cursor_x = area
            .x
            .saturating_add(1)
            .saturating_add(app.input_buffer.chars().count() as u16)
            .min(area.right().saturating_sub(2));
        f.set_cursor_position((cursor_x, area.y.saturating_add(1)));
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
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {}  ", label),
            Style::default().fg(Color::DarkGray),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_operations_view(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("[ operations — Ctrl+B/Esc to return ]")
        .style(Style::default().fg(Color::DarkGray));
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
                OperationLevel::Info => Color::DarkGray,
                OperationLevel::Success => Color::Green,
                OperationLevel::Error => Color::Red,
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
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines.push(Line::from(Span::styled(
        status_summary(app),
        Style::default().fg(Color::Cyan),
    )));

    let para = Paragraph::new(lines);
    f.render_widget(para, inner);
}

fn status_summary(app: &App) -> String {
    let mode = if app.sql_query.trim().is_empty() {
        "table"
    } else {
        "sql"
    };
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
