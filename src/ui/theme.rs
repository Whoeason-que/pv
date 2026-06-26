use ratatui::style::{Color, Modifier};

pub const FOCUSED: Color = Color::Yellow;
pub const FOCUS_PATH_UNFOCUSED: Color = Color::Blue;
pub const FOCUS_SQL_UNFOCUSED: Color = Color::Magenta;
pub const FOCUS_TABLE_UNFOCUSED: Color = Color::Blue;

pub const TEXT_PRIMARY: Color = Color::White;
pub const TEXT_BLACK: Color = Color::Black;

pub const BG_CURSOR: Color = Color::Yellow;
pub const BG_HEADER: Color = Color::Cyan;
pub const BG_HIGHLIGHT: Color = Color::DarkGray;

pub const BLOCK_TITLE: Color = Color::Cyan;
pub const SELECTED: Color = Color::Green;
pub const UNSELECTED: Color = Color::DarkGray;
pub const INPUT_TITLE: Color = Color::Yellow;
pub const ACTION_KEY: Color = Color::Cyan;
pub const ACTION_LABEL: Color = Color::DarkGray;
pub const OPERATIONS_INFO: Color = Color::DarkGray;
pub const OPERATIONS_SUCCESS: Color = Color::Green;
pub const OPERATIONS_ERROR: Color = Color::Red;
pub const STATUS_LINE: Color = Color::Cyan;
pub const SORTED_COL: Color = Color::Yellow;
pub const EMPTY_DATA: Color = Color::DarkGray;
pub const EMPTY_BLOCK: Color = Color::Blue;
pub const HELP_HEADER: Color = Color::Yellow;
pub const HELP_BODY: Color = Color::White;

pub const MOD_FOCUSED: Modifier = Modifier::BOLD;
pub const MOD_HEADER: Modifier = Modifier::BOLD;
pub const MOD_CURSOR: Modifier = Modifier::BOLD;
pub const MOD_HIGHLIGHT: Modifier = Modifier::BOLD;
