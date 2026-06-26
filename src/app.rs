use crate::engine::{
    ParquetEngine, SortDirection, SortSpec, make_column_safe, metadata::ParquetMetadata,
};
use crate::settings::Settings;
use anyhow::Result;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Which panel/input mode is active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    SqlInput,
    OpenInput,
    ExportInput,
    FieldSelect,
    MetadataView,
    Operations,
    Help,
}

pub const DEFAULT_SQL: &str = "SELECT * FROM pv_data LIMIT 100";
const COLUMN_QUERY_WINDOW_SIZE: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationLevel {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationOutcome {
    Started,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    Open,
    Sql,
    Clear,
    Page,
    Scroll,
    FieldSelect,
    Metadata,
    Sort,
    Export,
    Reload,
    Input,
    Help,
    Quit,
    Crosshair,
    Display,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationLog {
    pub id: u64,
    pub timestamp_ms: u128,
    pub kind: OperationKind,
    pub outcome: OperationOutcome,
    pub level: OperationLevel,
    pub summary: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableDensity {
    Fill,
    Dense,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Path,
    Sql,
    Table,
}

/// Application state.
pub struct App {
    pub engine: Option<ParquetEngine>,
    pub metadata: Option<ParquetMetadata>,
    pub settings: Settings,

    pub path: Option<PathBuf>,
    pub selected_fields: Vec<String>,
    pub all_fields: Vec<String>,
    pub source_record_count: i64,
    pub record_count: i64,
    pub partition_count: i64,

    pub offset: i64,
    pub page_size: i64,
    pub sql_query: String,
    pub sort: Option<SortSpec>,

    /// Cached loaded rows (string-formatted cells).
    pub rows: Vec<Vec<String>>,
    /// Column headers for current selection.
    pub headers: Vec<String>,
    pub visible_col_start: usize,

    pub mode: Mode,
    pub focus: Focus,
    pub input_buffer: String,
    pub input_prompt: String,

    /// Status / error message shown to user.
    pub message: String,
    pub message_is_error: bool,

    /// Table scroll state
    pub row_scroll: usize,
    pub col_scroll: usize,
    pub cursor_visible: bool,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub table_density: TableDensity,

    /// Field select cursor
    pub field_select_cursor: usize,
    /// Metadata view scroll
    pub meta_scroll: usize,

    pub operation_log: VecDeque<OperationLog>,
    pub next_operation_id: u64,
    pub operation_log_capacity: usize,
    pub operation_log_scroll: usize,
}

impl Default for App {
    fn default() -> Self {
        Self::new(Settings::load())
    }
}

impl App {
    pub fn new(settings: Settings) -> Self {
        Self {
            engine: None,
            metadata: None,
            settings,
            path: None,
            selected_fields: Vec::new(),
            all_fields: Vec::new(),
            source_record_count: 0,
            record_count: 0,
            partition_count: 0,
            offset: 0,
            page_size: 1000,
            sql_query: String::new(),
            sort: None,
            rows: Vec::new(),
            headers: Vec::new(),
            visible_col_start: 0,
            mode: Mode::Normal,
            focus: Focus::Table,
            input_buffer: String::new(),
            input_prompt: String::new(),
            message: String::new(),
            message_is_error: false,
            row_scroll: 0,
            col_scroll: 0,
            cursor_visible: true,
            cursor_row: 0,
            cursor_col: 0,
            table_density: TableDensity::Fill,
            field_select_cursor: 0,
            meta_scroll: 0,
            operation_log: VecDeque::new(),
            next_operation_id: 1,
            operation_log_capacity: 500,
            operation_log_scroll: 0,
        }
    }

    /// Open a parquet file or folder.
    pub fn reset_scroll_state(&mut self) {
        self.row_scroll = 0;
        self.col_scroll = 0;
        self.visible_col_start = 0;
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    pub fn open_path(&mut self, path: &str) -> Result<()> {
        let engine = crate::engine::open_engine(path)?;
        self.all_fields = engine.fields().iter().map(|f| f.name.clone()).collect();
        self.record_count = engine.record_count();
        self.source_record_count = self.record_count;
        self.partition_count = engine.partition_count();
        self.path = Some(engine.path().to_path_buf());

        self.selected_fields = self.all_fields.clone();

        self.offset = 0;
        self.sql_query.clear();
        self.sort = None;
        self.reset_scroll_state();

        if self.settings.always_load_all {
            self.page_size = self.record_count;
        } else {
            self.page_size = self.settings.page_size;
        }

        self.engine = Some(engine);

        if self.selected_fields.is_empty() {
            self.set_message("File opened. Press Ctrl+F to select fields.", false);
        } else {
            self.reload()?;
            if self.selected_fields.len() > COLUMN_QUERY_WINDOW_SIZE {
                self.set_message(
                    format!(
                        "Wide table: loaded visible {} of {} selected columns.",
                        self.headers.len(),
                        self.selected_fields.len()
                    ),
                    false,
                );
            }
        }
        Ok(())
    }

    /// Reload rows from the engine with current settings.
    pub fn reload(&mut self) -> Result<()> {
        self.ensure_column_window();
        let Some(engine) = &self.engine else {
            return Ok(());
        };

        let limit = if self.page_size <= 0 {
            1000
        } else {
            self.page_size
        };

        if self.is_table_mode() {
            if self.selected_fields.is_empty() {
                self.rows.clear();
                self.headers.clear();
                return Ok(());
            }
            invalidate_sort_if_stale(&mut self.sort, &self.selected_fields);
            let visible_fields = self.visible_fields();
            let result = engine.read_rows_query(
                &visible_fields,
                None,
                self.offset,
                limit,
                self.sort.as_ref(),
            )?;
            self.headers = result.headers;
            self.rows = result.rows;
            self.record_count = self.source_record_count;
        } else {
            invalidate_sort_if_stale(&mut self.sort, &self.headers);
            let result = engine.query_sql(&self.sql_query, self.offset, limit, None)?;
            self.headers = result.headers;
            self.rows = result.rows;
        }

        self.row_scroll = 0;
        self.cursor_row = 0;
        self.cursor_col = self.cursor_col.min(self.total_columns().saturating_sub(1));
        self.col_scroll = self.col_scroll.min(self.total_columns().saturating_sub(1));

        if self.rows.is_empty() {
            self.set_message("No rows returned for the current query.", false);
        } else {
            self.set_message(
                format!("Loaded {} rows (offset {})", self.rows.len(), self.offset),
                false,
            );
        }
        Ok(())
    }

    fn visible_fields(&self) -> Vec<String> {
        let end = self
            .visible_col_start
            .saturating_add(COLUMN_QUERY_WINDOW_SIZE)
            .min(self.selected_fields.len());
        self.selected_fields[self.visible_col_start..end].to_vec()
    }

    pub fn ensure_column_window(&mut self) {
        if self.is_sql_mode() || self.selected_fields.is_empty() {
            self.visible_col_start = 0;
            return;
        }
        let max_start = self
            .selected_fields
            .len()
            .saturating_sub(COLUMN_QUERY_WINDOW_SIZE);
        self.visible_col_start = self.visible_col_start.min(max_start);
        let visible_end = self
            .visible_col_start
            .saturating_add(COLUMN_QUERY_WINDOW_SIZE)
            .min(self.selected_fields.len());
        if self.cursor_col < self.visible_col_start {
            self.visible_col_start = self.cursor_col.min(max_start);
        } else if self.cursor_col >= visible_end {
            self.visible_col_start = self
                .cursor_col
                .saturating_sub(COLUMN_QUERY_WINDOW_SIZE.saturating_sub(1))
                .min(max_start);
        }
    }

    pub fn require_engine(&self) -> bool {
        self.engine.is_some()
    }

    pub fn is_table_mode(&self) -> bool {
        self.sql_query.trim().is_empty()
    }

    pub fn is_sql_mode(&self) -> bool {
        !self.sql_query.trim().is_empty()
    }

    pub fn total_columns(&self) -> usize {
        if self.is_table_mode() {
            self.selected_fields.len()
        } else {
            self.headers.len()
        }
    }

    /// Load metadata for the current engine.
    pub fn load_metadata(&mut self) -> Result<()> {
        let Some(engine) = &self.engine else {
            anyhow::bail!("No file is open");
        };
        let metadata = engine.load_metadata()?;
        self.metadata = Some(metadata);
        Ok(())
    }

    pub fn apply_sql_query(&mut self, sql: String) -> Result<()> {
        self.sql_query = sql;
        self.offset = 0;
        self.reset_scroll_state();
        self.sort = None;
        let Some(engine) = &self.engine else {
            anyhow::bail!("No file is open");
        };
        self.record_count = engine.count_sql(&self.sql_query)?;
        self.reload()
    }

    pub fn clear_sql_query(&mut self) -> Result<()> {
        self.sql_query.clear();
        self.record_count = self.source_record_count;
        self.offset = 0;
        self.reset_scroll_state();
        self.sort = None;
        self.reload()
    }

    pub fn load_all(&mut self) -> Result<()> {
        self.page_size = self.record_count;
        self.offset = 0;
        self.cursor_row = 0;
        self.reload()
    }

    pub fn set_fields(&mut self, fields: Vec<String>) -> Result<()> {
        self.selected_fields = fields;
        self.sql_query.clear();
        self.record_count = self.source_record_count;
        self.sort = None;
        self.offset = 0;
        self.reset_scroll_state();
        self.reload()
    }

    pub fn toggle_sort_current_column(&mut self) -> Result<()> {
        if self.headers.is_empty() && self.selected_fields.is_empty() {
            self.set_message("No column to sort", true);
            return Ok(());
        }
        let total = self.total_columns();
        let idx = if self.cursor_visible {
            self.cursor_col.min(total.saturating_sub(1))
        } else {
            self.col_scroll.min(total.saturating_sub(1))
        };
        let name = if self.is_table_mode() {
            self.selected_fields.get(idx).cloned().unwrap_or_default()
        } else {
            self.headers.get(idx).cloned().unwrap_or_default()
        };
        let direction = match &self.sort {
            Some(sort)
                if sort.column_index == idx
                    && sort.column_name == name
                    && sort.direction == SortDirection::Asc =>
            {
                Some(SortDirection::Desc)
            }
            Some(sort)
                if sort.column_index == idx
                    && sort.column_name == name
                    && sort.direction == SortDirection::Desc =>
            {
                None
            }
            _ => Some(SortDirection::Asc),
        };

        self.sql_query =
            apply_order_by_to_sql(current_sql(self.sql_query.as_str()), &name, direction);
        self.sort = direction.map(|direction| SortSpec {
            column_index: idx,
            column_name: name,
            direction,
        });
        self.offset = 0;
        self.row_scroll = 0;
        self.cursor_row = 0;
        if let Some(engine) = &self.engine {
            self.record_count = engine.count_sql(&self.sql_query)?;
        }
        self.reload()
    }

    pub fn move_cursor(&mut self, row_delta: isize, col_delta: isize) -> Result<()> {
        if self.rows.is_empty() || self.headers.is_empty() {
            return Ok(());
        }
        let max_row = self.rows.len().saturating_sub(1);
        let max_col = self.total_columns().saturating_sub(1);
        self.cursor_row = self
            .cursor_row
            .saturating_add_signed(row_delta)
            .min(max_row);
        self.cursor_col = self
            .cursor_col
            .saturating_add_signed(col_delta)
            .min(max_col);
        let prev_window = self.visible_col_start;
        self.ensure_column_window();
        if self.visible_col_start != prev_window {
            self.reload()?;
        }
        Ok(())
    }

    pub fn toggle_cursor(&mut self) {
        self.cursor_visible = !self.cursor_visible;
    }

    pub fn toggle_table_density(&mut self) {
        self.table_density = match self.table_density {
            TableDensity::Fill => TableDensity::Dense,
            TableDensity::Dense => TableDensity::Fill,
        };
    }

    pub fn next_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Path => Focus::Sql,
            Focus::Sql => Focus::Table,
            Focus::Table => Focus::Path,
        };
    }

    pub fn set_message(&mut self, msg: impl Into<String>, is_error: bool) {
        let msg = msg.into();
        self.message = msg.clone();
        self.message_is_error = is_error;
        self.record_operation(
            OperationKind::System,
            if is_error {
                OperationOutcome::Failed
            } else {
                OperationOutcome::Succeeded
            },
            msg,
            None,
        );
    }

    pub fn record_operation(
        &mut self,
        kind: OperationKind,
        outcome: OperationOutcome,
        summary: impl Into<String>,
        detail: Option<String>,
    ) {
        let level = match outcome {
            OperationOutcome::Failed => OperationLevel::Error,
            OperationOutcome::Succeeded => OperationLevel::Success,
            OperationOutcome::Started | OperationOutcome::Cancelled => OperationLevel::Info,
        };
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let entry = OperationLog {
            id: self.next_operation_id,
            timestamp_ms,
            kind,
            outcome,
            level,
            summary: summary.into(),
            detail,
        };
        self.next_operation_id += 1;
        self.operation_log.push_back(entry);
        while self.operation_log.len() > self.operation_log_capacity {
            self.operation_log.pop_front();
        }
        self.operation_log_scroll = 0;
    }

    pub fn log_started(&mut self, kind: OperationKind, summary: impl Into<String>) {
        self.record_operation(kind, OperationOutcome::Started, summary, None);
    }

    pub fn log_succeeded(&mut self, kind: OperationKind, summary: impl Into<String>) {
        self.record_operation(kind, OperationOutcome::Succeeded, summary, None);
    }

    pub fn log_succeeded_detail(
        &mut self,
        kind: OperationKind,
        summary: impl Into<String>,
        detail: impl Into<String>,
    ) {
        self.record_operation(
            kind,
            OperationOutcome::Succeeded,
            summary,
            Some(detail.into()),
        );
    }

    pub fn log_cancelled(&mut self, kind: OperationKind, summary: impl Into<String>) {
        self.record_operation(kind, OperationOutcome::Cancelled, summary, None);
    }

    pub fn enter_input_mode(&mut self, mode: Mode, prompt: &str) {
        self.mode = mode;
        self.input_prompt = prompt.to_string();
        self.input_buffer.clear();
    }

    pub fn exit_input_mode(&mut self) {
        self.mode = Mode::Normal;
        self.input_buffer.clear();
    }
}

fn invalidate_sort_if_stale(sort: &mut Option<SortSpec>, col_names: &[String]) {
    if let Some(s) = sort
        && (s.column_index >= col_names.len() || col_names[s.column_index] != s.column_name)
    {
        *sort = None;
    }
}

fn current_sql(sql: &str) -> &str {
    if sql.trim().is_empty() {
        DEFAULT_SQL
    } else {
        sql
    }
}

fn apply_order_by_to_sql(sql: &str, column_name: &str, direction: Option<SortDirection>) -> String {
    let sql = sql.trim().trim_end_matches(';').trim();
    let (base, tail) = split_limit_clause(sql);
    let base = strip_order_by_clause(base).trim();
    match direction {
        Some(direction) => {
            let direction = match direction {
                SortDirection::Asc => "ASC",
                SortDirection::Desc => "DESC",
            };
            format!(
                "{} ORDER BY {} {}{}",
                base,
                make_column_safe(column_name),
                direction,
                tail
            )
        }
        None => format!("{}{}", base, tail),
    }
}

fn split_limit_clause(sql: &str) -> (&str, &str) {
    let upper = sql.to_ascii_uppercase();
    if let Some(index) = upper.rfind(" LIMIT ") {
        (&sql[..index], &sql[index..])
    } else {
        (sql, "")
    }
}

fn strip_order_by_clause(sql: &str) -> &str {
    let upper = sql.to_ascii_uppercase();
    if let Some(index) = upper.rfind(" ORDER BY ") {
        &sql[..index]
    } else {
        sql
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_operation_appends_incrementing_ids() {
        let mut app = App::new(Settings::default());
        app.record_operation(OperationKind::Open, OperationOutcome::Started, "one", None);
        app.record_operation(OperationKind::Sql, OperationOutcome::Succeeded, "two", None);

        assert_eq!(app.operation_log.len(), 2);
        assert_eq!(app.operation_log[0].id, 1);
        assert_eq!(app.operation_log[1].id, 2);
    }

    #[test]
    fn operation_log_respects_capacity() {
        let mut app = App::new(Settings::default());
        app.operation_log_capacity = 2;
        app.record_operation(OperationKind::Open, OperationOutcome::Started, "one", None);
        app.record_operation(OperationKind::Sql, OperationOutcome::Succeeded, "two", None);
        app.record_operation(
            OperationKind::Export,
            OperationOutcome::Failed,
            "three",
            None,
        );

        assert_eq!(app.operation_log.len(), 2);
        assert_eq!(app.operation_log[0].summary, "two");
        assert_eq!(app.operation_log[1].summary, "three");
    }

    #[test]
    fn set_message_records_success_and_error() {
        let mut app = App::new(Settings::default());
        app.set_message("ok", false);
        app.set_message("bad", true);

        assert_eq!(app.operation_log.len(), 2);
        assert_eq!(app.operation_log[0].level, OperationLevel::Success);
        assert_eq!(app.operation_log[1].level, OperationLevel::Error);
        assert_eq!(app.operation_log[1].outcome, OperationOutcome::Failed);
    }
}
