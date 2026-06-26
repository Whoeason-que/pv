use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};

use crate::app::App;

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    // We store scroll in col_scroll repurposed; use a local state via app.meta_scroll
    let popup = centered_rect(80, 80, area);

    // Clear the background
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = app
        .all_fields
        .iter()
        .map(|field| {
            let selected = app.selected_fields.iter().any(|sf| sf == field);
            let check = if selected { "[x]" } else { "[ ]" };
            let style = if selected {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            ListItem::new(format!("{}  {}", check, field)).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(
                    "[ Select Fields — Space: toggle  a: all  n: none  Enter: apply  Esc: cancel ]",
                )
                .style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut state = ListState::default();
    let idx = (app.meta_scroll as usize).min(app.all_fields.len().saturating_sub(1));
    state.select(Some(idx));
    f.render_stateful_widget(list, popup, &mut state);

    // Footer hint
    let footer_area = Rect {
        x: popup.x,
        y: popup.bottom().saturating_sub(1),
        width: popup.width,
        height: 1,
    };
    let _ = footer_area;
}

pub fn move_cursor(app: &mut App, delta: i32) {
    let max = app.all_fields.len() as i32;
    if max == 0 {
        return;
    }
    let cur = app.meta_scroll as i32;
    let new = (cur + delta).clamp(0, max - 1);
    app.meta_scroll = new as u16;
}

pub fn toggle_current(app: &mut App) {
    let idx = app.meta_scroll as usize;
    if let Some(field) = app.all_fields.get(idx).cloned() {
        if let Some(pos) = app.selected_fields.iter().position(|f| f == &field) {
            app.selected_fields.remove(pos);
        } else {
            app.selected_fields.push(field);
        }
    }
}

pub fn select_all(app: &mut App) {
    app.selected_fields = app.all_fields.clone();
}

pub fn select_none(app: &mut App) {
    app.selected_fields.clear();
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Percentage((100 - percent_y) / 2),
            ratatui::layout::Constraint::Percentage(percent_y),
            ratatui::layout::Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([
            ratatui::layout::Constraint::Percentage((100 - percent_x) / 2),
            ratatui::layout::Constraint::Percentage(percent_x),
            ratatui::layout::Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
