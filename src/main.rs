mod app;
mod engine;
mod export;
mod settings;
mod ui;

use std::io::{self, stdout};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::{App, DEFAULT_SQL, Focus, Mode, OperationKind};

/// A cross-platform terminal viewer for Apache Parquet files.
#[derive(Parser, Debug)]
#[command(name = "parquet-tui", version, about)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Path to a parquet file or folder to open on launch.
    path: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Open a parquet file or folder.
    Open { path: String },
    /// Print version and exit.
    Version,
    /// Manage this executable.
    #[command(name = "self")]
    SelfCmd {
        #[command(subcommand)]
        command: SelfCommand,
    },
}

#[derive(Subcommand, Debug)]
enum SelfCommand {
    /// Build and install from source.
    Update {
        /// Use the current source tree via cargo install --path.
        #[arg(long)]
        dev: bool,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();
    match &args.command {
        Some(Command::Version) => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(Command::SelfCmd { command }) => dispatch_self(command),
        Some(Command::Open { path }) => run_tui(Some(path)),
        None => run_tui(args.path.as_ref()),
    }
}

fn dispatch_self(cmd: &SelfCommand) -> Result<()> {
    match cmd {
        SelfCommand::Update { dev } => {
            if *dev {
                println!("Building and installing parquet-tui from source...");
                let status = std::process::Command::new("cargo")
                    .args(["install", "--path", env!("CARGO_MANIFEST_DIR")])
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .status()
                    .context("Failed to run cargo install")?;
                if !status.success() {
                    anyhow::bail!("cargo install failed");
                }
                println!("\x1b[32m Completed\x1b[0m");
            } else {
                anyhow::bail!("Release mode not implemented. Use --dev to build from source.");
            }
            Ok(())
        }
    }
}

fn run_tui(path: Option<&String>) -> Result<()> {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
        let _ = crossterm::cursor::Show;
        original_hook(info);
    }));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Initialize app
    let settings = settings::Settings::load();
    let mut app = App::new(settings);

    // Open file if provided as argument
    if let Some(path) = path {
        let path = unquote_path(path);
        match app.open_path(&path) {
            Ok(_) => {}
            Err(e) => {
                app.set_message(format!("Failed to open: {}", e), true);
            }
        }
    }

    // Main event loop
    let result = run(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if !event::poll(std::time::Duration::from_millis(100))? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };

        if key.kind != KeyEventKind::Press {
            continue;
        }

        match handle_key(app, key) {
            Action::Continue => {}
            Action::Quit => break,
        }
    }
    Ok(())
}

fn unquote_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Continue,
    Quit,
}

fn enter_path_input(app: &mut App) {
    app.log_started(OperationKind::Open, "Opened path input");
    app.focus = Focus::Path;
    app.enter_input_mode(Mode::OpenInput, "Open path");
    app.input_buffer = app
        .path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
}

fn enter_sql_input(app: &mut App) {
    if app.engine.is_some() {
        app.log_started(OperationKind::Sql, "Opened SQL input");
        app.focus = Focus::Sql;
        app.enter_input_mode(Mode::SqlInput, "SQL query against pv_data");
        app.input_buffer = if app.sql_query.is_empty() {
            DEFAULT_SQL.to_string()
        } else {
            app.sql_query.clone()
        };
    } else {
        app.set_message("Open a file first (press Ctrl+P)", true);
    }
}

fn ctrl_char(key: KeyEvent, target: char) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char(c) if c.eq_ignore_ascii_case(&target))
}

fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    if ctrl_char(key, 'b') {
        if app.mode == Mode::Operations {
            app.mode = Mode::Normal;
            app.log_cancelled(OperationKind::System, "Closed operations view");
        } else {
            app.mode = Mode::Operations;
            app.log_started(OperationKind::System, "Opened operations view");
        }
        return Action::Continue;
    }

    // Handle input modes first
    match app.mode {
        Mode::SqlInput | Mode::OpenInput | Mode::ExportInput => {
            return handle_input_mode(app, key);
        }
        Mode::FieldSelect => {
            return handle_field_select(app, key);
        }
        Mode::MetadataView => {
            return handle_metadata_view(app, key);
        }
        Mode::Operations => {
            return handle_operations_view(app, key);
        }
        Mode::Help => {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) || ctrl_char(key, 'h') {
                app.log_cancelled(OperationKind::Help, "Closed help");
                app.mode = Mode::Normal;
            }
            return Action::Continue;
        }
        Mode::Normal => {}
    }

    if matches!(key.code, KeyCode::Tab) {
        app.next_focus();
        return Action::Continue;
    }

    if matches!(key.code, KeyCode::Enter) {
        match app.focus {
            Focus::Path => enter_path_input(app),
            Focus::Sql => enter_sql_input(app),
            Focus::Table => {}
        }
        return Action::Continue;
    }

    if matches!(key.code, KeyCode::Esc) {
        app.log_started(OperationKind::Quit, "Quit requested");
        return Action::Quit;
    }

    if matches!(key.code, KeyCode::Char('?')) || ctrl_char(key, 'h') {
        app.log_started(OperationKind::Help, "Opened help");
        app.mode = Mode::Help;
        return Action::Continue;
    }

    if ctrl_char(key, 'p') {
        enter_path_input(app);
        return Action::Continue;
    }

    if ctrl_char(key, 'f') {
        if app.engine.is_some() {
            app.log_started(OperationKind::FieldSelect, "Opened field selection");
            app.meta_scroll = 0;
            app.mode = Mode::FieldSelect;
        } else {
            app.set_message("Open a file first (press Ctrl+P)", true);
        }
        return Action::Continue;
    }

    if matches!(key.code, KeyCode::Char('/')) || ctrl_char(key, 's') {
        enter_sql_input(app);
        return Action::Continue;
    }

    if ctrl_char(key, 'l') {
        if let Err(e) = app.clear_sql_query() {
            app.set_message(format!("{}", e), true);
        } else {
            app.log_succeeded(OperationKind::Clear, "Cleared SQL");
        }
        return Action::Continue;
    }

    if ctrl_char(key, 'a') {
        if let Err(e) = app.load_all() {
            app.set_message(format!("{}", e), true);
        } else {
            app.log_succeeded(OperationKind::Page, "Loaded all rows");
        }
        return Action::Continue;
    }

    if ctrl_char(key, 't') {
        if let Err(e) = app.toggle_sort_current_column() {
            app.set_message(format!("Sort error: {}", e), true);
        } else {
            app.log_succeeded(OperationKind::Sort, "Toggled sort");
        }
        return Action::Continue;
    }

    if ctrl_char(key, 'x') {
        app.toggle_cursor();
        app.log_succeeded(OperationKind::Crosshair, "Toggled cursor");
        return Action::Continue;
    }

    if ctrl_char(key, 'd') {
        app.toggle_table_density();
        app.log_succeeded(OperationKind::Display, "Toggled table density");
        return Action::Continue;
    }

    if ctrl_char(key, 'y') {
        if app.engine.is_some() {
            match app.load_metadata() {
                Ok(_) => {
                    app.log_succeeded(OperationKind::Metadata, "Opened metadata view");
                    app.meta_scroll = 0;
                    app.mode = Mode::MetadataView;
                }
                Err(e) => app.set_message(format!("Metadata error: {}", e), true),
            }
        } else {
            app.set_message("Open a file first (press Ctrl+P)", true);
        }
        return Action::Continue;
    }

    if ctrl_char(key, 'e') {
        if app.rows.is_empty() {
            app.set_message("No data to export", true);
        } else {
            app.log_started(OperationKind::Export, "Opened export input");
            app.enter_input_mode(Mode::ExportInput, "Export path (.csv/.json/.xlsx)");
        }
        return Action::Continue;
    }

    if ctrl_char(key, 'r') {
        if let Err(e) = app.reload() {
            app.set_message(format!("{}", e), true);
        } else {
            app.log_succeeded(OperationKind::Reload, "Reloaded data");
        }
        return Action::Continue;
    }

    match key.code {
        KeyCode::Down => {
            if app.cursor_visible {
                if let Err(e) = app.move_cursor(1, 0) {
                    app.set_message(format!("Scroll error: {}", e), true);
                }
            } else {
                app.row_scroll = app.row_scroll.saturating_add(1);
            }
            app.log_succeeded(OperationKind::Scroll, "Scrolled rows down");
        }
        KeyCode::Up => {
            if app.cursor_visible {
                if let Err(e) = app.move_cursor(-1, 0) {
                    app.set_message(format!("Scroll error: {}", e), true);
                }
            } else {
                app.row_scroll = app.row_scroll.saturating_sub(1);
            }
            app.log_succeeded(OperationKind::Scroll, "Scrolled rows up");
        }
        KeyCode::Right => {
            if app.cursor_visible {
                if let Err(e) = app.move_cursor(0, 1) {
                    app.set_message(format!("Scroll error: {}", e), true);
                }
            } else {
                let prev = app.visible_col_start;
                app.col_scroll = app.col_scroll.saturating_add(1);
                app.ensure_column_window();
                if app.visible_col_start != prev
                    && let Err(e) = app.reload()
                {
                    app.set_message(format!("Scroll error: {}", e), true);
                }
            }
            app.log_succeeded(OperationKind::Scroll, "Scrolled columns right");
        }
        KeyCode::Left => {
            if app.cursor_visible {
                if let Err(e) = app.move_cursor(0, -1) {
                    app.set_message(format!("Scroll error: {}", e), true);
                }
            } else {
                let prev = app.visible_col_start;
                app.col_scroll = app.col_scroll.saturating_sub(1);
                app.ensure_column_window();
                if app.visible_col_start != prev
                    && let Err(e) = app.reload()
                {
                    app.set_message(format!("Scroll error: {}", e), true);
                }
            }
            app.log_succeeded(OperationKind::Scroll, "Scrolled columns left");
        }
        _ => {}
    }
    Action::Continue
}

fn handle_input_mode(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => {
            app.log_cancelled(OperationKind::Input, "Cancelled input");
            app.exit_input_mode();
        }
        KeyCode::Enter => {
            let input = app.input_buffer.clone();
            let mode = app.mode.clone();
            app.exit_input_mode();

            match mode {
                Mode::OpenInput => {
                    if input.trim().is_empty() {
                        app.set_message("No path entered", true);
                    } else {
                        match app.open_path(&unquote_path(&input)) {
                            Ok(_) => {
                                app.focus = Focus::Table;
                                app.log_succeeded(OperationKind::Open, format!("Opened {}", input))
                            }
                            Err(e) => app.set_message(format!("Failed to open: {}", e), true),
                        }
                    }
                }
                Mode::SqlInput => {
                    if input.trim().is_empty() {
                        if let Err(e) = app.clear_sql_query() {
                            app.set_message(format!("Query clear error: {}", e), true);
                        } else {
                            app.set_message("SQL mode cleared", false);
                            app.log_succeeded(OperationKind::Sql, "Cleared SQL mode");
                        }
                    } else if let Err(e) = app.apply_sql_query(input.clone()) {
                        app.set_message(format!("SQL query error: {}", e), true);
                    } else {
                        app.focus = Focus::Table;
                        app.log_succeeded_detail(OperationKind::Sql, "Applied SQL query", input);
                    }
                }
                Mode::ExportInput => {
                    let path = std::path::PathBuf::from(&input);
                    match export::export(&app.headers, &app.rows, &path) {
                        Ok(_) => {
                            app.set_message(format!("Exported to {}", path.display()), false);
                            app.log_succeeded(
                                OperationKind::Export,
                                format!("Exported to {}", path.display()),
                            );
                        }
                        Err(e) => app.set_message(format!("Export failed: {}", e), true),
                    }
                }
                _ => {}
            }
        }
        KeyCode::Backspace => {
            app.input_buffer.pop();
        }
        KeyCode::Char(c) => {
            // Ctrl+C quits
            if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'c' {
                return Action::Quit;
            }
            app.input_buffer.push(c);
        }
        _ => {}
    }
    Action::Continue
}

fn handle_field_select(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => {
            app.log_cancelled(OperationKind::FieldSelect, "Cancelled field selection");
            app.mode = Mode::Normal;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            ui::field_select::move_cursor(app, -1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            ui::field_select::move_cursor(app, 1);
        }
        KeyCode::Char(' ') => {
            ui::field_select::toggle_current(app);
            app.log_succeeded(OperationKind::FieldSelect, "Toggled field");
        }
        KeyCode::Char('a') => {
            ui::field_select::select_all(app);
            app.log_succeeded(OperationKind::FieldSelect, "Selected all fields");
        }
        KeyCode::Char('n') => {
            ui::field_select::select_none(app);
            app.log_succeeded(OperationKind::FieldSelect, "Cleared field selection");
        }
        KeyCode::Enter => {
            if app.selected_fields.is_empty() {
                app.set_message("Select at least one field", true);
            } else {
                let fields = app.selected_fields.clone();
                if let Err(e) = app.set_fields(fields) {
                    app.set_message(format!("{}", e), true);
                } else {
                    app.log_succeeded(OperationKind::FieldSelect, "Applied field selection");
                }
                app.mode = Mode::Normal;
            }
        }
        _ => {}
    }
    Action::Continue
}

fn handle_metadata_view(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('m') => {
            app.log_cancelled(OperationKind::Metadata, "Closed metadata view");
            app.mode = Mode::Normal;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            ui::metadata_view::scroll(app, 1);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            ui::metadata_view::scroll(app, -1);
        }
        _ => {}
    }
    Action::Continue
}

fn handle_operations_view(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.log_cancelled(OperationKind::System, "Closed operations view");
            app.mode = Mode::Normal;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let max_scroll = app.operation_log.len().saturating_sub(1);
            app.operation_log_scroll = app.operation_log_scroll.saturating_add(1).min(max_scroll);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.operation_log_scroll = app.operation_log_scroll.saturating_sub(1);
        }
        _ => {}
    }
    Action::Continue
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn unquote_path_removes_matching_quotes() {
        assert_eq!(unquote_path("'data/file.parquet'"), "data/file.parquet");
        assert_eq!(unquote_path("\"data/file.parquet\""), "data/file.parquet");
        assert_eq!(unquote_path(" data/file.parquet "), "data/file.parquet");
        assert_eq!(unquote_path("'data/file.parquet\""), "'data/file.parquet\"");
    }

    #[test]
    fn esc_quits_from_normal_mode() {
        let mut app = App::default();
        assert_eq!(handle_key(&mut app, key(KeyCode::Esc)), Action::Quit);
    }

    #[test]
    fn esc_cancels_sql_input_not_quit() {
        let mut app = App {
            mode: Mode::SqlInput,
            input_buffer: "select * from pv_data".to_string(),
            ..Default::default()
        };

        assert_eq!(handle_key(&mut app, key(KeyCode::Esc)), Action::Continue);
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.input_buffer.is_empty());
    }

    #[test]
    fn slash_opens_sql_input_when_file_open() {
        let mut app = App::default();
        let path = format!(
            "{}/testdata/basic_types.parquet",
            env!("CARGO_MANIFEST_DIR")
        );
        app.open_path(&path).unwrap();

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('/'))),
            Action::Continue
        );
        assert_eq!(app.mode, Mode::SqlInput);
    }

    #[test]
    fn ctrl_s_also_opens_sql_input_when_file_open() {
        let mut app = App::default();
        let path = format!(
            "{}/testdata/basic_types.parquet",
            env!("CARGO_MANIFEST_DIR")
        );
        app.open_path(&path).unwrap();

        assert_eq!(handle_key(&mut app, ctrl_key('s')), Action::Continue);
        assert_eq!(app.mode, Mode::SqlInput);
    }

    #[test]
    fn plain_letters_do_not_open_sql_input() {
        let mut app = App::default();
        let path = format!(
            "{}/testdata/basic_types.parquet",
            env!("CARGO_MANIFEST_DIR")
        );
        app.open_path(&path).unwrap();

        assert_eq!(
            handle_key(&mut app, key(KeyCode::Char('s'))),
            Action::Continue
        );
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn ctrl_b_toggles_operations_view() {
        let mut app = App::default();

        assert_eq!(handle_key(&mut app, ctrl_key('b')), Action::Continue);
        assert_eq!(app.mode, Mode::Operations);

        assert_eq!(handle_key(&mut app, ctrl_key('b')), Action::Continue);
        assert_eq!(app.mode, Mode::Normal);
    }
}
