use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn draw(f: &mut Frame, area: Rect) {
    let popup = centered_rect(70, 80, area);
    f.render_widget(Clear, popup);

    let help_text = vec![
        ("parquet-tui", true),
        (
            "A cross-platform terminal viewer for Apache Parquet files.",
            false,
        ),
        ("", false),
        ("Keybindings:", true),
        (
            "  Esc        Quit in normal mode; cancel/close overlays",
            false,
        ),
        ("  Tab        Switch focus: path / sql / table", false),
        ("  Ctrl+R     Reload data", false),
        ("  Ctrl+X     Toggle table cursor", false),
        ("  /          Edit SQL", false),
        ("  ?          Help (this screen)", false),
        ("", false),
        ("Other bindings:", true),
        ("  Ctrl+P     Edit/open path", false),
        ("  Ctrl+L     Clear SQL query", false),
        ("  Ctrl+F     Select fields", false),
        ("  Ctrl+T     Sort selected column", false),
        ("  Ctrl+D     Toggle dense/fill table", false),
        ("  Ctrl+E     Export current view (CSV/JSON/XLSX)", false),
        ("  Ctrl+A     Load all rows", false),
        ("  Ctrl+Y     View metadata & schema", false),
        ("  Ctrl+B     Toggle dev operations view", false),
        ("  ↑/↓ ←/→    Move cursor / scroll table", false),
        ("", false),
        (
            "Tip: pass a parquet file path as a CLI argument to open directly.",
            false,
        ),
        ("     e.g. parquet-tui data.parquet", false),
    ];

    let lines: Vec<ratatui::text::Line> = help_text
        .into_iter()
        .map(|(text, is_header)| {
            let style = if is_header {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ratatui::text::Line::from(ratatui::text::Span::styled(text, style))
        })
        .collect();

    let para = Paragraph::new(lines).alignment(Alignment::Left).block(
        Block::default()
            .borders(Borders::ALL)
            .title("[ Help — press Esc or ? to close ]")
            .style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(para, popup);
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
