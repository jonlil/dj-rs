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
    pub acoustid_api_key: Option<String>,
    pub music_library_path: Option<String>,
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

    /// Resolve the library database path.
    /// Uses `db_path` if explicitly configured, otherwise falls back to
    /// `~/.local/share/dj-rs/master.db` if that file exists.
    pub fn resolved_db_path(&self) -> Option<String> {
        if let Some(ref p) = self.db_path {
            return Some(p.clone());
        }
        let default = default_db_path();
        if default.exists() {
            Some(default.to_string_lossy().into_owned())
        } else {
            None
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

    /// Reverse-apply mappings: convert a local path back to a rekordbox-stored path.
    /// (Swaps `to` → `from` direction.)
    pub fn reverse_mappings(&self, path: &str) -> String {
        for m in &self.path_mappings {
            if !m.to.is_empty() && path.starts_with(&m.to) {
                return format!("{}{}", m.from, &path[m.to.len()..]);
            }
        }
        path.to_string()
    }

    /// Return the local directory that is the root for ANLZ analysis files.
    ///
    /// Primary: the directory containing master.db (e.g. `~/.local/share/dj-rs/`).
    /// Fallback: derived from the first path mapping's `from` parent + `share`
    ///           (for dev setups where ANLZ files live in the project tree).
    /// Return the music library directory.
    /// Uses `music_library_path` if set, otherwise `~/Music/`.
    pub fn music_library_dir(&self) -> PathBuf {
        self.music_library_path
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Music"))
    }

    pub fn anlz_base_dir(&self) -> Option<PathBuf> {
        // Primary: co-located with master.db
        if let Some(db) = self.resolved_db_path() {
            let db_dir = std::path::Path::new(&db).parent()?;
            let pioneer = db_dir.join("PIONEER");
            if pioneer.exists() {
                return Some(db_dir.to_path_buf());
            }
        }
        // Fallback: project-tree share dir next to music mapping
        for m in &self.path_mappings {
            if m.from.is_empty() { continue; }
            let from = std::path::Path::new(&m.from);
            let base = from.parent().unwrap_or(from).join("share");
            if base.join("PIONEER").exists() {
                return Some(base);
            }
        }
        None
    }
}

/// Default location for the Rekordbox library database on Linux:
/// `~/.local/share/dj-rs/master.db`
pub fn default_db_path() -> PathBuf {
    let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("/home/.local/share"));
    path.push("dj-rs");
    path.push("master.db");
    path
}

fn config_path() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("dj-rs");
    path.push("config.json");
    Some(path)
}
