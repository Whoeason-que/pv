use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::app::{App, Focus, TableDensity};
use crate::ui::theme::*;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    if draw_empty_state(f, area, app) {
        return;
    }

    // Calculate column widths based on content
    let available_width = area.width.saturating_sub(2) as usize; // borders
    let col_count = app.total_columns().max(1);

    // Determine display columns (horizontal scroll)
    let visible_cols = compute_visible_cols(app, available_width);
    let row_start = compute_row_start(app, area);
    let col_start = visible_cols.0;
    let col_end = visible_cols.1;

    let widths: Vec<usize> = compute_widths(app, col_start, col_end, available_width);

    let constraints: Vec<ratatui::layout::Constraint> = widths
        .iter()
        .map(|w| ratatui::layout::Constraint::Length(*w as u16))
        .collect();

    let header_cells: Vec<Cell> = (col_start..col_end)
        .map(|i| {
            let mut cell = Cell::from(header_label(app, i));
            if app.cursor_visible && global_col_index(app, i) == app.cursor_col {
                cell = cell.style(Style::default().fg(TEXT_BLACK).bg(BG_CURSOR));
            }
            cell
        })
        .collect();

    let table = Table::new(
        app.rows
            .iter()
            .enumerate()
            .skip(row_start)
            .map(|(row_index, row)| {
                let cells: Vec<Cell> = (col_start..col_end)
                    .map(|col_index| {
                        let mut style = Style::default();
                        if app.cursor_visible {
                            let is_row = row_index == app.cursor_row;
                            let is_col = global_col_index(app, col_index) == app.cursor_col;
                            style = match (is_row, is_col) {
                                (true, true) => Style::default()
                                    .fg(TEXT_BLACK)
                                    .bg(BG_CURSOR)
                                    .add_modifier(MOD_CURSOR),
                                (true, false) => Style::default().bg(BG_HIGHLIGHT),
                                (false, true) => Style::default().fg(SORTED_COL),
                                (false, false) => Style::default(),
                            };
                        }
                        Cell::from(row.get(col_index).map(|s| s.as_str()).unwrap_or_default())
                            .style(style)
                    })
                    .collect();
                Row::new(cells)
            }),
        constraints,
    )
    .header(
        Row::new(header_cells).style(
            Style::default()
                .fg(TEXT_BLACK)
                .bg(BG_HEADER)
                .add_modifier(MOD_HEADER),
        ),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(
                "[ data — {} cols — {}{} ]",
                col_count,
                match app.table_density {
                    TableDensity::Fill => "fill d",
                    TableDensity::Dense => "dense d",
                },
                if app.cursor_visible {
                    " — cursor x"
                } else {
                    ""
                }
            ))
            .style(if app.focus == Focus::Table {
                Style::default().fg(FOCUSED).add_modifier(MOD_FOCUSED)
            } else {
                Style::default().fg(FOCUS_TABLE_UNFOCUSED)
            }),
    )
    .row_highlight_style(Style::default().bg(BG_HIGHLIGHT));

    f.render_widget(table, area);
}

fn global_col_index(app: &App, local: usize) -> usize {
    if app.is_table_mode() {
        app.visible_col_start.saturating_add(local)
    } else {
        local
    }
}

fn local_cursor_col(app: &App) -> usize {
    if app.is_table_mode() {
        app.cursor_col.saturating_sub(app.visible_col_start)
    } else {
        app.cursor_col
    }
    .min(app.headers.len().saturating_sub(1))
}

fn local_col_scroll(app: &App) -> usize {
    if app.is_table_mode() {
        app.col_scroll.saturating_sub(app.visible_col_start)
    } else {
        app.col_scroll
    }
    .min(app.headers.len().saturating_sub(1))
}

fn column_content_width(app: &App, index: usize) -> usize {
    app.column_widths.get(index).copied().unwrap_or(12)
}

fn compute_widths(
    app: &App,
    col_start: usize,
    col_end: usize,
    available_width: usize,
) -> Vec<usize> {
    let mut widths: Vec<usize> = (col_start..col_end)
        .map(|index| column_content_width(app, index))
        .collect();

    if app.table_density == TableDensity::Fill && !widths.is_empty() {
        let current: usize = widths.iter().sum();
        let extra = available_width.saturating_sub(current);
        let per_col = extra / widths.len();
        let mut remainder = extra % widths.len();
        for width in &mut widths {
            *width += per_col;
            if remainder > 0 {
                *width += 1;
                remainder -= 1;
            }
        }
    }

    widths
}

fn compute_row_start(app: &App, area: Rect) -> usize {
    let visible_rows = area.height.saturating_sub(3) as usize;
    if !app.cursor_visible || visible_rows == 0 {
        return app.row_scroll.min(app.rows.len().saturating_sub(1));
    }
    let max_start = app.rows.len().saturating_sub(visible_rows);
    let current_start = app.row_scroll.min(max_start);
    if app.cursor_row < current_start {
        app.cursor_row
    } else if app.cursor_row >= current_start.saturating_add(visible_rows) {
        app.cursor_row
            .saturating_sub(visible_rows.saturating_sub(1))
    } else {
        current_start
    }
    .min(max_start)
}

fn compute_visible_cols(app: &App, available_width: usize) -> (usize, usize) {
    let total = app.headers.len();
    if total == 0 {
        return (0, 0);
    }

    let mut col_start = local_col_scroll(app);
    let mut col_end = visible_col_end(app, col_start, available_width);

    if app.cursor_visible {
        let lc = local_cursor_col(app);
        if lc < col_start {
            col_start = lc;
            col_end = visible_col_end(app, col_start, available_width);
        } else if lc >= col_end {
            col_start = lc;
            loop {
                let prev = col_start.saturating_sub(1);
                if prev == col_start {
                    break;
                }
                let next_end = visible_col_end(app, prev, available_width);
                if next_end <= lc {
                    break;
                }
                col_start = prev;
            }
            col_end = visible_col_end(app, col_start, available_width);
        }
    }

    if col_end == col_start {
        col_end = (col_start + 1).min(total);
    }
    (col_start, col_end)
}

fn visible_col_end(app: &App, col_start: usize, available_width: usize) -> usize {
    let total = app.headers.len();
    let mut width_used = 0;
    let mut col_end = col_start;
    while col_end < total {
        let w = column_content_width(app, col_end);
        if width_used + w > available_width && col_end > col_start {
            break;
        }
        width_used += w;
        col_end += 1;
    }
    col_end
}

fn header_label(app: &App, index: usize) -> String {
    let mut label = app.headers.get(index).cloned().unwrap_or_default();
    if let Some(sort) = &app.sort
        && sort.column_index == global_col_index(app, index)
    {
        match sort.direction {
            crate::engine::SortDirection::Asc => label.push_str(" ↑"),
            crate::engine::SortDirection::Desc => label.push_str(" ↓"),
        }
    }
    label
}

fn draw_empty_state(f: &mut Frame, area: Rect, app: &App) -> bool {
    if let Some(ref _engine) = app.engine {
        if app.total_columns() == 0 {
            let text = "No fields selected — press Ctrl+F to select fields";
            let block = Block::default()
                .borders(Borders::ALL)
                .style(Style::default().fg(EMPTY_BLOCK));
            let paragraph = Paragraph::new(text)
                .block(block)
                .alignment(Alignment::Center)
                .style(Style::default().fg(EMPTY_DATA));
            f.render_widget(paragraph, area);
            return true;
        }
        if app.headers.is_empty() {
            let text = "No data — check your query or try pressing Ctrl+L to reset";
            let block = Block::default()
                .borders(Borders::ALL)
                .style(Style::default().fg(EMPTY_BLOCK));
            let paragraph = Paragraph::new(text)
                .block(block)
                .alignment(Alignment::Center)
                .style(Style::default().fg(EMPTY_DATA));
            f.render_widget(paragraph, area);
            return true;
        }
    } else {
        let text = "Open a file to start browsing — press Ctrl+P";
        let block = Block::default()
            .borders(Borders::ALL)
            .style(Style::default().fg(EMPTY_BLOCK));
        let paragraph = Paragraph::new(text)
            .block(block)
            .alignment(Alignment::Center)
            .style(Style::default().fg(EMPTY_DATA));
        f.render_widget(paragraph, area);
        return true;
    }
    false
}
