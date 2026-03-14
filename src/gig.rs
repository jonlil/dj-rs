use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GigType {
    Corporate,
    Venue,
    Private,
}

impl GigType {
    pub fn label(&self) -> &'static str {
        match self {
            GigType::Corporate => "Corporate",
            GigType::Venue     => "Venue",
            GigType::Private   => "Private",
        }
    }

    /// Top-level Rekordbox folder name for this gig type.
    pub fn playlist_folder(&self) -> &'static str {
        match self {
            GigType::Corporate => "CORPORATE",
            GigType::Venue     => "VENUES",
            GigType::Private   => "PRIVATE",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gig {
    pub id: String,
    pub gig_type: GigType,
    /// Event name, e.g. "Kick-off 2026" or "Wedding"
    pub name: String,
    /// Contact person or client — used as playlist prefix
    pub contact: String,
    /// YYYY-MM-DD
    pub date: String,
    /// HH:MM
    pub start_time: String,
    /// HH:MM
    pub end_time: String,
    /// Venue name or address, free text
    pub location: String,
    /// Music preferences, vibe notes, client wishes
    pub notes: String,
    pub spotify_playlist_url: Option<String>,
    /// ID of the auto-created Rekordbox playlist, once generated
    pub rekordbox_playlist_id: Option<i64>,
}

impl Gig {
    /// Playlist name used inside Rekordbox: "{contact} - {name}"
    pub fn playlist_name(&self) -> String {
        format!("{} - {}", self.contact, self.name)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GigStore {
    pub gigs: Vec<Gig>,
}

impl GigStore {
    pub fn load() -> Self {
        gigs_path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(path) = gigs_path() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = fs::write(path, json);
            }
        }
    }

    pub fn add(&mut self, gig: Gig) {
        self.gigs.push(gig);
        self.save();
    }

    pub fn update(&mut self, gig: Gig) {
        if let Some(existing) = self.gigs.iter_mut().find(|g| g.id == gig.id) {
            *existing = gig;
            self.save();
        }
    }
}

fn gigs_path() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("dj-rs");
    path.push("gigs.json");
    Some(path)
}
