use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            "[ {} — Esc to cancel, Enter to submit ]",
            app.input_prompt
        ))
        .style(Style::default().fg(Color::Yellow));
    let para = Paragraph::new(app.input_buffer.as_str())
        .block(block)
        .style(Style::default().fg(Color::White));
    f.render_widget(para, area);

    // Place cursor at end of input
    let cursor_x = area.x + 1 + app.input_buffer.chars().count() as u16;
    let cursor_y = area.y + 1;
    f.set_cursor_position((cursor_x, cursor_y));
}
