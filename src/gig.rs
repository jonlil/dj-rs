use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CustomerType {
    Corporate,
    Venue,
    Private,
}

impl CustomerType {
    pub fn label(&self) -> &'static str {
        match self {
            CustomerType::Corporate => "Corporate",
            CustomerType::Venue     => "Venue",
            CustomerType::Private   => "Private",
        }
    }

    pub fn playlist_folder(&self) -> &'static str {
        match self {
            CustomerType::Corporate => "CORPORATE",
            CustomerType::Venue     => "VENUES",
            CustomerType::Private   => "PRIVATE",
        }
    }
}

/// Top-level Rekordbox folder names that belong to gig output — hidden from
/// the main playlist browser.
pub const GIG_FOLDERS: &[&str] = &["CORPORATE", "VENUES", "PRIVATE"];

/// Represents the ongoing relationship with a client, venue, or company.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    pub name: String,
    pub customer_type: CustomerType,
    /// General music preferences / vibe notes for this contact
    #[serde(default)]
    pub notes: String,
    /// ID of the contact folder in djmdPlaylist
    pub rekordbox_folder_id: Option<i64>,
}

/// A missing track the DJ has decided to purchase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingBuyTrack {
    pub spotify_id: String,
    pub title:      String,
    pub artist:     String,
}

/// Represents one specific gig / event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gig {
    pub id: String,
    pub contact_id: String,
    /// Event name, e.g. "Wedding", "Kick-off 2026"
    pub name: String,
    /// YYYY-MM-DD or null
    #[serde(default)]
    pub date: Option<String>,
    /// Get-in / start time, HH:MM
    #[serde(default)]
    pub start_time: Option<String>,
    /// End time, HH:MM
    #[serde(default)]
    pub end_time: Option<String>,
    /// Venue name or address
    #[serde(default)]
    pub location: Option<String>,
    /// Free-form tags, e.g. ["wedding", "outdoor"]
    #[serde(default)]
    pub tags: Vec<String>,
    /// Music preferences, vibe notes, client wishes
    #[serde(default)]
    pub notes: String,
    pub spotify_playlist_url: Option<String>,
    /// Cached Spotify playlist tracks (last successful fetch)
    #[serde(default)]
    pub cached_spotify_tracks: Vec<crate::spotify::SpotifyTrack>,
    /// Track IDs (djmdContent) accepted in the Match tab (matched tracks → playlist)
    #[serde(default)]
    pub accepted_track_ids: Vec<i64>,
    /// Missing tracks the DJ has decided to buy
    #[serde(default)]
    pub pending_buy_tracks: Vec<PendingBuyTrack>,
    /// Spotify IDs of missing tracks reviewed and skipped (so re-runs don't re-prompt)
    #[serde(default)]
    pub denied_spotify_ids: Vec<String>,
    /// ID of the event folder (or playlist) in djmdPlaylist
    pub rekordbox_folder_id: Option<i64>,
}

impl Gig {
    /// Short display label: "Name (date)", "Name", or the date alone when name is empty.
    pub fn format_label(&self) -> String {
        if self.name.is_empty() {
            self.date.as_deref().unwrap_or("New Gig").to_string()
        } else if let Some(date) = &self.date {
            format!("{} ({})", self.name, date)
        } else {
            self.name.clone()
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GigStore {
    #[serde(default)]
    pub contacts: Vec<Contact>,
    #[serde(default)]
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

    pub fn add_contact(&mut self, contact: Contact) {
        self.contacts.push(contact);
        self.save();
    }

    pub fn add_gig(&mut self, gig: Gig) {
        self.gigs.push(gig);
        self.save();
    }

    pub fn update_contact(&mut self, contact: Contact) {
        if let Some(existing) = self.contacts.iter_mut().find(|c| c.id == contact.id) {
            *existing = contact;
            self.save();
        }
    }

    pub fn update_gig(&mut self, gig: Gig) {
        if let Some(existing) = self.gigs.iter_mut().find(|g| g.id == gig.id) {
            *existing = gig;
            self.save();
        }
    }

    pub fn gigs_for_contact<'a>(&'a self, contact_id: &str) -> Vec<&'a Gig> {
        self.gigs.iter().filter(|g| g.contact_id == contact_id).collect()
    }

    pub fn contact_for_gig(&self, gig: &Gig) -> Option<&Contact> {
        self.contacts.iter().find(|c| c.id == gig.contact_id)
    }
}

fn gigs_path() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("dj-rs");
    path.push("gigs.json");
    Some(path)
}
