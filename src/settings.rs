use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_page_size")]
    pub page_size: i64,
    #[serde(default)]
    pub always_load_all: bool,
    #[serde(default = "default_true")]
    pub dark_mode: bool,
    #[serde(default)]
    pub always_select_all_fields: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            page_size: default_page_size(),
            always_load_all: false,
            dark_mode: true,
            always_select_all_fields: false,
        }
    }
}

fn default_page_size() -> i64 {
    1000
}

fn default_true() -> bool {
    true
}

fn config_path() -> Result<PathBuf> {
    let proj = directories::ProjectDirs::from("dev", "parquet-tui", "parquet-tui")
        .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
    Ok(proj.config_dir().join("settings.json"))
}

impl Settings {
    pub fn load() -> Self {
        match config_path().and_then(|p| {
            if p.exists() {
                let contents = fs::read_to_string(&p)?;
                Ok(serde_json::from_str::<Settings>(&contents)?)
            } else {
                Ok(Settings::default())
            }
        }) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Warning: could not load settings: {}. Using defaults.", e);
                Settings::default()
            }
        }
    }

}
