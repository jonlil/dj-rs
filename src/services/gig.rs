use crate::gig::{Contact, CustomerType, Gig, GigStore, PendingBuyTrack};

// ── DTOs for updating from UI ───────────────────────────────────────────────

pub struct ContactUpdate {
    pub name: String,
    pub customer_type: CustomerType,
    pub notes: String,
}

pub struct GigUpdate {
    pub name: String,
    pub date: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub location: Option<String>,
    pub notes: String,
    pub spotify_playlist_url: Option<String>,
    pub accepted_track_ids: Vec<i64>,
    pub pending_buy_tracks: Vec<PendingBuyTrack>,
    pub denied_spotify_ids: Vec<String>,
}

// ── Service functions ───────────────────────────────────────────────────────

/// Creates a new empty contact, persists, and returns its id.
pub fn create_contact(store: &mut GigStore) -> String {
    let contact = Contact {
        id: uuid::Uuid::new_v4().to_string(),
        name: String::new(),
        customer_type: CustomerType::Private,
        notes: String::new(),
        rekordbox_folder_id: None,
    };
    let id = contact.id.clone();
    store.contacts.push(contact);
    store.save();
    id
}

/// Updates an existing contact by id. Returns false if not found.
pub fn save_contact(store: &mut GigStore, contact_id: &str, update: ContactUpdate) -> bool {
    if let Some(c) = store.contacts.iter_mut().find(|c| c.id == contact_id) {
        c.name = update.name;
        c.customer_type = update.customer_type;
        c.notes = update.notes;
        store.save();
        true
    } else {
        false
    }
}

/// Deletes a contact and all its gigs (cascade). Returns false if not found.
pub fn delete_contact(store: &mut GigStore, contact_id: &str) -> bool {
    let before = store.contacts.len();
    store.contacts.retain(|c| c.id != contact_id);
    store.gigs.retain(|g| g.contact_id != contact_id);
    store.save();
    store.contacts.len() < before
}

/// Creates a new empty gig for a contact, persists, and returns its id.
pub fn create_gig(store: &mut GigStore, contact_id: &str) -> String {
    let gig = Gig {
        id: uuid::Uuid::new_v4().to_string(),
        contact_id: contact_id.to_string(),
        name: String::new(),
        date: None,
        start_time: None,
        end_time: None,
        location: None,
        tags: Vec::new(),
        notes: String::new(),
        spotify_playlist_url: None,
        cached_spotify_tracks: Vec::new(),
        accepted_track_ids: Vec::new(),
        pending_buy_tracks: Vec::new(),
        denied_spotify_ids: Vec::new(),
        rekordbox_folder_id: None,
    };
    let id = gig.id.clone();
    store.gigs.push(gig);
    store.save();
    id
}

/// Updates an existing gig by id. Returns false if not found.
pub fn save_gig(store: &mut GigStore, gig_id: &str, update: GigUpdate) -> bool {
    if let Some(g) = store.gigs.iter_mut().find(|g| g.id == gig_id) {
        g.name = update.name;
        g.date = update.date;
        g.start_time = update.start_time;
        g.end_time = update.end_time;
        g.location = update.location;
        g.notes = update.notes;
        g.spotify_playlist_url = update.spotify_playlist_url;
        g.accepted_track_ids = update.accepted_track_ids;
        g.pending_buy_tracks = update.pending_buy_tracks;
        g.denied_spotify_ids = update.denied_spotify_ids;
        store.save();
        true
    } else {
        false
    }
}
