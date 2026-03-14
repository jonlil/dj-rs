use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub db_path: Option<String>,
    pub path_mappings: Vec<PathMapping>,
    pub spotify_client_id: Option<String>,
    pub spotify_access_token: Option<String>,
    pub spotify_refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathMapping {
    pub from: String,
    pub to: String,
}

impl Config {
    pub fn load() -> Self {
        config_path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(path) = config_path() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = fs::write(path, json);
            }
        }
    }

    /// Apply stored path mappings to a file path.
    /// Returns the first match, or the original path if nothing matches.
    pub fn apply_mappings(&self, path: &str) -> String {
        for m in &self.path_mappings {
            if !m.from.is_empty() && path.starts_with(&m.from) {
                return format!("{}{}", m.to, &path[m.from.len()..]);
            }
        }
        path.to_string()
    }
}

fn config_path() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("dj-rs");
    path.push("config.json");
    Some(path)
}
