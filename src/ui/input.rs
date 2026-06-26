use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::ui::theme::*;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            "[ {} — Esc to cancel, Enter to submit ]",
            app.input_prompt
        ))
        .style(Style::default().fg(INPUT_TITLE));
    let para = Paragraph::new(app.input_buffer.as_str())
        .block(block)
        .style(Style::default().fg(TEXT_PRIMARY));
    f.render_widget(para, area);

    super::set_input_cursor(f, area, &app.input_buffer);
}
