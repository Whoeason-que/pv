use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};

use crate::app::App;
use crate::ui::centered_rect;
use crate::ui::theme::*;

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    // We store scroll in col_scroll repurposed; use a local state via app.field_select_cursor
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
                Style::default().fg(SELECTED).add_modifier(MOD_FOCUSED)
            } else {
                Style::default().fg(UNSELECTED)
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
                .style(Style::default().fg(BLOCK_TITLE)),
        )
        .highlight_style(
            Style::default()
                .bg(BG_HIGHLIGHT)
                .add_modifier(MOD_HIGHLIGHT),
        )
        .highlight_symbol("> ");

    let mut state = ListState::default();
    let idx = app
        .field_select_cursor
        .min(app.all_fields.len().saturating_sub(1));
    state.select(Some(idx));
    f.render_stateful_widget(list, popup, &mut state);
}

pub fn move_cursor(app: &mut App, delta: i32) {
    let max = app.all_fields.len();
    if max == 0 {
        return;
    }
    let cur = app.field_select_cursor as i32;
    let new = (cur + delta).clamp(0, max as i32 - 1);
    app.field_select_cursor = new as usize;
}

pub fn toggle_current(app: &mut App) {
    let idx = app.field_select_cursor;
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
