pub mod browser;
pub mod contact;
pub mod gig;
pub mod player;
pub mod settings;
pub mod theme;

use iced::widget::{column, container, text_editor};
use iced::{Element, Fill, Subscription, Task, Theme};
use dj_rs::rekordbox::{CuePoint, Library, Track};
use dj_rs::config::Config;
use dj_rs::deck::DeckState;
use dj_rs::gig::{CustomerType, GigStore, PendingBuyTrack};
use browser::{BrowserState, Selection};
use contact::ContactState;
use gig::{GigState, MatchResultEntry, MatchStatus};
use player::PlayerState;
use settings::SettingsState;

pub use browser::Section;

// ── Messages ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    // Browser
    SectionClicked(Section),
    NodeToggled(i64),
    NodeSelected(Selection),
    SearchChanged(String),
    TracksLoaded(Vec<Track>),
    TrackSelected(i64),
    TrackClicked(i64),
    ToggleTrackInfo,
    TrackInfoDragStart(f32),   // cursor x
    TrackInfoDragMove(f32),    // cursor x
    TrackInfoDragEnd,
    // Spotify browser
    SpotifyPlaylistsLoaded(Vec<dj_rs::spotify::UserPlaylist>),
    SpotifyPlaylistSelected(String), // playlist_id
    SpotifyTracksLoaded(Vec<browser::SpotifyTrackRow>),
    // Contacts
    ContactAdd,
    ContactOpened(String),     // contact_id
    ContactClosed,
    ContactNameChanged(String),
    ContactTypeChanged(CustomerType),
    ContactNotesAction(text_editor::Action),
    ContactSave,
    ContactDelete,
    ContactAddGig,
    // Gig detail
    GigClicked(String),        // gig_id
    GigClosed,
    GigNameChanged(String),
    GigDateChanged(String),
    GigStartTimeChanged(String),
    GigEndTimeChanged(String),
    GigLocationChanged(String),
    GigNotesAction(text_editor::Action),
    GigSpotifyUrlChanged(String),
    GigSave,
    GigRunMatch,
    GigMatchResult(Vec<MatchResultEntry>),
    GigMatchError(String),
    GigAcceptTrack(i64),
    GigBuyTrack(String),       // spotify_id
    GigDenyTrack(String),      // spotify_id
    GigUnskipTrack(String),    // spotify_id — undo a skip
    GigCopyShoppingList,
    // Settings
    SettingsClicked,
    SettingsSave,
    SettingsMappingFromChanged(usize, String),
    SettingsMappingToChanged(usize, String),
    SettingsMappingAdd,
    SettingsMappingRemove(usize),
    SettingsDbPathChanged(String),
    SettingsMusicPathChanged(String),
    SpotifyConnect,
    SpotifyConnectResult(Result<(String, String), String>),
    // Background
    Tick(std::time::Instant),
    SpotifyTokenRefreshed(String, Option<String>), // (access_token, new_refresh_token)
    // Player transport
    CuePressed,
    PlayPressed,
    OverviewSeek(f64), // fraction 0.0–1.0 of track duration
    // Player data loading
    WaveformLoaded(Option<Vec<u8>>, Option<Vec<u8>>),
    CuesLoaded(Vec<CuePoint>),
    // Audio tick (60fps position update)
    AudioTick(std::time::Instant),
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    pub browser: BrowserState,
    pub player: PlayerState,
    pub deck: DeckState,
    pub contact: Option<ContactState>,
    pub gig: Option<GigState>,
    pub settings: Option<SettingsState>,
    config: Config,
    lib: Library,
    anlz_base: Option<String>,
    spotify_token: Option<String>,
    spotify_refresh_token: Option<String>,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let config = Config::load();
        let db_path = config
            .resolved_db_path()
            .unwrap_or_else(|| {
                dirs::data_local_dir()
                    .unwrap_or_default()
                    .join("dj-rs/master.db")
                    .to_string_lossy()
                    .into_owned()
            });

        let lib = Library::open(&db_path).expect("failed to open database");

        let anlz_base = config.anlz_base_dir()
            .map(|p| p.to_string_lossy().into_owned());

        let spotify_token = config.spotify_access_token.clone();
        let spotify_refresh_token = config.spotify_refresh_token.clone();

        let playlists = lib.playlists().unwrap_or_default();
        let all_tracks = lib.tracks().unwrap_or_default();

        let gig_store = GigStore::load();

        let mut browser = BrowserState::new(playlists, gig_store);
        browser.tracks = all_tracks;
        browser.selection = Selection::All;

        let deck = DeckState::new();

        (Self {
            browser,
            player: PlayerState::new(),
            deck,
            contact: None,
            gig: None,
            settings: None,
            config,
            lib,
            anlz_base,
            spotify_token,
            spotify_refresh_token,
        }, Task::done(Message::Tick(std::time::Instant::now())))
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::SectionClicked(section) => {
                if self.browser.section == section {
                    self.browser.sidebar_open = !self.browser.sidebar_open;
                } else {
                    let load_spotify = section == Section::Spotify
                        && self.browser.spotify_playlists.is_empty()
                        && self.spotify_token.is_some();
                    self.browser.section = section;
                    self.browser.sidebar_open = true;
                    if load_spotify {
                        let token = self.spotify_token.clone().unwrap();
                        return Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    dj_rs::spotify::fetch_user_playlists(&token)
                                })
                                .await
                                .map_err(|e| e.to_string())?
                            },
                            |result| match result {
                                Ok(playlists) => Message::SpotifyPlaylistsLoaded(playlists),
                                Err(_) => Message::SpotifyPlaylistsLoaded(vec![]),
                            },
                        );
                    }
                }
                Task::none()
            }

            Message::NodeToggled(id) => {
                if self.browser.expanded.contains(&id) {
                    self.browser.expanded.remove(&id);
                } else {
                    self.browser.expanded.insert(id);
                }
                Task::none()
            }

            Message::NodeSelected(sel) => {
                self.contact = None;
                self.gig = None;
                self.settings = None;
                self.browser.selection = sel.clone();
                self.browser.tracks.clear();
                let lib = self.lib.clone();
                Task::perform(
                    async move { load_tracks(&lib, sel) },
                    Message::TracksLoaded,
                )
            }

            Message::TracksLoaded(tracks) => {
                self.browser.tracks = tracks;
                Task::none()
            }

            Message::SpotifyPlaylistsLoaded(playlists) => {
                self.browser.spotify_playlists = playlists;
                Task::none()
            }

            Message::SpotifyPlaylistSelected(playlist_id) => {
                self.browser.selection = Selection::SpotifyPlaylist(playlist_id.clone());
                self.browser.spotify_tracks.clear();
                self.browser.spotify_loading = true;
                self.contact = None;
                self.gig = None;
                self.settings = None;
                let token = self.spotify_token.clone().unwrap_or_default();
                let lib = self.lib.clone();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            load_spotify_tracks(token, playlist_id, &lib)
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    |result| match result {
                        Ok(rows) => Message::SpotifyTracksLoaded(rows),
                        Err(_) => Message::SpotifyTracksLoaded(vec![]),
                    },
                )
            }

            Message::SpotifyTracksLoaded(rows) => {
                self.browser.spotify_tracks = rows;
                self.browser.spotify_loading = false;
                Task::none()
            }

            Message::SearchChanged(query) => {
                self.browser.search = query.clone();
                let lib = self.lib.clone();
                if query.is_empty() {
                    let sel = self.browser.selection.clone();
                    Task::perform(
                        async move { load_tracks(&lib, sel) },
                        Message::TracksLoaded,
                    )
                } else {
                    Task::perform(
                        async move {
                            lib.search_tracks(&query).unwrap_or_default()
                        },
                        Message::TracksLoaded,
                    )
                }
            }

            Message::TrackSelected(id) => {
                self.browser.selected_track_id = Some(id);
                Task::none()
            }

            Message::ToggleTrackInfo => {
                self.browser.track_info_open = !self.browser.track_info_open;
                Task::none()
            }

            Message::TrackInfoDragStart(_) => {
                self.browser.track_info_dragging = true;
                self.browser.track_info_drag_start_x = 0.0; // set on first move
                self.browser.track_info_drag_start_width = self.browser.track_info_width;
                Task::none()
            }

            Message::TrackInfoDragMove(x) => {
                if self.browser.track_info_dragging {
                    if self.browser.track_info_drag_start_x == 0.0 {
                        // First move — record start position
                        self.browser.track_info_drag_start_x = x;
                    } else {
                        let delta = self.browser.track_info_drag_start_x - x;
                        let new_width = (self.browser.track_info_drag_start_width + delta)
                            .clamp(180.0, 500.0);
                        self.browser.track_info_width = new_width;
                    }
                }
                Task::none()
            }

            Message::TrackInfoDragEnd => {
                self.browser.track_info_dragging = false;
                Task::none()
            }

            Message::TrackClicked(id) => {
                if let Some(track) = self.browser.tracks.iter().find(|t| t.id == id) {
                    // Load audio into the deck
                    if let Some(ref fp) = track.file_path {
                        let resolved = self.config.apply_mappings(fp);
                        let path = std::path::PathBuf::from(&resolved);
                        if path.exists() {
                            match self.deck.load(path) {
                                Ok(_) => {
                                    // Override duration from DB if available
                                    if let Some(dur) = track.duration_secs {
                                        if dur > 0 {
                                            self.deck.duration_secs = dur as f64;
                                        }
                                    }
                                }
                                Err(e) => eprintln!("Failed to load track: {}", e),
                            }
                        }
                    }

                    self.player.load_track(
                        track.id,
                        track.title.clone(),
                        track.artist.clone().unwrap_or_default(),
                        track.duration_secs,
                        track.bpm.map(|b| b as f32 / 100.0),
                        track.key.clone(),
                    );
                    // Sync duration from deck if DB didn't have it
                    if self.player.duration_secs.is_none() && self.deck.duration_secs > 0.0 {
                        self.player.duration_secs = Some(self.deck.duration_secs as i32);
                    }
                }

                // Load cues
                let lib = self.lib.clone();
                let cue_task = Task::perform(
                    async move {
                        lib.load_cues(id).unwrap_or_default()
                    },
                    Message::CuesLoaded,
                );

                // Load waveform if anlz_base is known
                if let Some(base) = self.anlz_base.clone() {
                    let lib = self.lib.clone();
                    let wf_task = Task::perform(
                        async move {
                            let base_path = std::path::Path::new(&base);
                            lib.load_waveform(id, base_path).ok()
                                .unwrap_or((None, None))
                        },
                        |(color, overview)| Message::WaveformLoaded(color, overview),
                    );
                    Task::batch([cue_task, wf_task])
                } else {
                    cue_task
                }
            }

            Message::WaveformLoaded(color, overview) => {
                self.player.color_waveform = color;
                self.player.overview_waveform = overview;
                Task::none()
            }

            Message::CuesLoaded(cues) => {
                // Set CUE position from first memory cue (kind==0)
                if let Some(mem_cue) = cues.iter().find(|c| c.kind == 0) {
                    self.player.cue_pos_secs = mem_cue.in_secs;
                }
                self.player.cue_points = cues;
                Task::none()
            }

            // ── Contact messages ──────────────────────────────────────────────

            Message::ContactAdd => {
                let contact = dj_rs::gig::Contact {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: String::new(),
                    customer_type: CustomerType::Private,
                    notes: String::new(),
                    rekordbox_folder_id: None,
                };
                let contact_id = contact.id.clone();
                self.browser.gig_store.contacts.push(contact);
                self.browser.gig_store.save();
                if let Some(c) = self.browser.gig_store.contacts.iter().find(|c| c.id == contact_id) {
                    self.contact = Some(ContactState::from_contact(c));
                    self.gig = None;
                }
                Task::none()
            }

            Message::ContactOpened(contact_id) => {
                // Skip if already viewing this contact
                if self.contact.as_ref().map(|c| c.contact_id.as_str()) == Some(&contact_id) {
                    return Task::none();
                }
                if let Some(c) = self.browser.gig_store.contacts.iter().find(|c| c.id == contact_id) {
                    self.contact = Some(ContactState::from_contact(c));
                    self.gig = None;
                    self.settings = None;
                }
                Task::none()
            }

            Message::ContactClosed => {
                self.contact = None;
                self.gig = None;
                Task::none()
            }

            Message::ContactNameChanged(name) => {
                if let Some(ref mut cs) = self.contact {
                    cs.name = name;
                    cs.dirty = true;
                }
                Task::none()
            }

            Message::ContactTypeChanged(ct) => {
                if let Some(ref mut cs) = self.contact {
                    cs.customer_type = ct;
                    cs.dirty = true;
                }
                Task::none()
            }

            Message::ContactNotesAction(action) => {
                if let Some(ref mut cs) = self.contact {
                    let is_edit = action.is_edit();
                    cs.notes.perform(action);
                    if is_edit {
                        cs.dirty = true;
                    }
                }
                Task::none()
            }

            Message::ContactSave => {
                if let Some(ref cs) = self.contact {
                    if let Some(c) = self.browser.gig_store.contacts.iter_mut().find(|c| c.id == cs.contact_id) {
                        c.name = cs.name.clone();
                        c.customer_type = cs.customer_type.clone();
                        c.notes = cs.notes_text();
                    }
                    self.browser.gig_store.save();
                }
                if let Some(ref mut cs) = self.contact {
                    cs.dirty = false;
                }
                Task::none()
            }

            Message::ContactDelete => {
                if let Some(ref cs) = self.contact {
                    let id = cs.contact_id.clone();
                    self.browser.gig_store.contacts.retain(|c| c.id != id);
                    self.browser.gig_store.gigs.retain(|g| g.contact_id != id);
                    self.browser.gig_store.save();
                    self.contact = None;
                    self.gig = None;
                }
                Task::none()
            }

            Message::ContactAddGig => {
                if let Some(ref cs) = self.contact {
                    let gig = dj_rs::gig::Gig {
                        id: uuid::Uuid::new_v4().to_string(),
                        contact_id: cs.contact_id.clone(),
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
                    let gig_id = gig.id.clone();
                    self.browser.gig_store.gigs.push(gig);
                    self.browser.gig_store.save();
                    // Open the newly created gig
                    if let Some(g) = self.browser.gig_store.gigs.iter().find(|g| g.id == gig_id) {
                        self.gig = Some(GigState::from_gig(g, &cs.name));
                    }
                }
                Task::none()
            }

            // ── Gig messages ─────────────────────────────────────────────────

            Message::GigClicked(gig_id) => {
                if let Some(g) = self.browser.gig_store.gigs.iter().find(|g| g.id == gig_id) {
                    let contact_name = self.browser.gig_store.contact_for_gig(g)
                        .map(|c| c.name.clone())
                        .unwrap_or_default();
                    self.gig = Some(GigState::from_gig(g, &contact_name));
                }
                Task::none()
            }

            Message::GigClosed => {
                self.gig = None;
                Task::none()
            }

            Message::GigNameChanged(name) => {
                if let Some(ref mut gs) = self.gig {
                    gs.name = name;
                    gs.dirty = true;
                }
                Task::none()
            }

            Message::GigDateChanged(date) => {
                if let Some(ref mut gs) = self.gig {
                    gs.date = date;
                    gs.dirty = true;
                }
                Task::none()
            }

            Message::GigStartTimeChanged(t) => {
                if let Some(ref mut gs) = self.gig {
                    gs.start_time = t;
                    gs.dirty = true;
                }
                Task::none()
            }

            Message::GigEndTimeChanged(t) => {
                if let Some(ref mut gs) = self.gig {
                    gs.end_time = t;
                    gs.dirty = true;
                }
                Task::none()
            }

            Message::GigLocationChanged(loc) => {
                if let Some(ref mut gs) = self.gig {
                    gs.location = loc;
                    gs.dirty = true;
                }
                Task::none()
            }

            Message::GigNotesAction(action) => {
                if let Some(ref mut gs) = self.gig {
                    let is_edit = action.is_edit();
                    gs.notes.perform(action);
                    if is_edit {
                        gs.dirty = true;
                    }
                }
                Task::none()
            }

            Message::GigSpotifyUrlChanged(url) => {
                if let Some(ref mut gs) = self.gig {
                    gs.spotify_url = url;
                    gs.dirty = true;
                }
                Task::none()
            }

            Message::GigSave => {
                if let Some(ref gs) = self.gig {
                    if let Some(g) = self.browser.gig_store.gigs.iter_mut().find(|g| g.id == gs.gig_id) {
                        g.name = gs.name.clone();
                        g.date = if gs.date.is_empty() { None } else { Some(gs.date.clone()) };
                        g.start_time = if gs.start_time.is_empty() { None } else { Some(gs.start_time.clone()) };
                        g.end_time = if gs.end_time.is_empty() { None } else { Some(gs.end_time.clone()) };
                        g.location = if gs.location.is_empty() { None } else { Some(gs.location.clone()) };
                        g.notes = gs.notes_text();
                        g.spotify_playlist_url = if gs.spotify_url.is_empty() { None } else { Some(gs.spotify_url.clone()) };
                        g.accepted_track_ids = gs.accepted_track_ids.iter().cloned().collect();
                        g.pending_buy_tracks = gs.pending_buy_tracks.clone();
                        g.denied_spotify_ids = gs.denied_spotify_ids.iter().cloned().collect();
                    }
                    self.browser.gig_store.save();
                }
                if let Some(ref mut gs) = self.gig {
                    gs.dirty = false;
                }
                Task::none()
            }

            Message::GigRunMatch => {
                if let Some(ref mut gs) = self.gig {
                    gs.match_status = MatchStatus::Running;
                    let url = gs.spotify_url.clone();
                    let token = self.spotify_token.clone().unwrap_or_default();
                    let lib = self.lib.clone();

                    return Task::perform(
                        async move {
                            run_match(token, url, &lib).await
                        },
                        |result| match result {
                            Ok(entries) => Message::GigMatchResult(entries),
                            Err(e) => Message::GigMatchError(e),
                        },
                    );
                }
                Task::none()
            }

            Message::GigMatchResult(results) => {
                if let Some(ref mut gs) = self.gig {
                    gs.match_results = results;
                    gs.match_status = MatchStatus::Done;
                }
                Task::none()
            }

            Message::GigMatchError(err) => {
                if let Some(ref mut gs) = self.gig {
                    gs.match_status = MatchStatus::Error(err);
                }
                Task::none()
            }

            Message::GigAcceptTrack(track_id) => {
                if let Some(ref mut gs) = self.gig {
                    if gs.accepted_track_ids.contains(&track_id) {
                        gs.accepted_track_ids.remove(&track_id);
                    } else {
                        gs.accepted_track_ids.insert(track_id);
                    }
                    gs.dirty = true;
                }
                Task::none()
            }

            Message::GigBuyTrack(spotify_id) => {
                if let Some(ref mut gs) = self.gig {
                    if gs.pending_buy_tracks.iter().any(|b| b.spotify_id == spotify_id) {
                        gs.pending_buy_tracks.retain(|b| b.spotify_id != spotify_id);
                    } else if let Some(entry) = gs.match_results.iter().find(|r| r.spotify.spotify_id == spotify_id) {
                        gs.pending_buy_tracks.push(PendingBuyTrack {
                            spotify_id: entry.spotify.spotify_id.clone(),
                            title: entry.spotify.title.clone(),
                            artist: entry.spotify.artist.clone(),
                        });
                    }
                    gs.dirty = true;
                }
                Task::none()
            }

            Message::GigDenyTrack(spotify_id) => {
                if let Some(ref mut gs) = self.gig {
                    gs.denied_spotify_ids.insert(spotify_id);
                    gs.dirty = true;
                }
                Task::none()
            }

            Message::GigUnskipTrack(spotify_id) => {
                if let Some(ref mut gs) = self.gig {
                    gs.denied_spotify_ids.remove(&spotify_id);
                    gs.dirty = true;
                }
                Task::none()
            }

            Message::GigCopyShoppingList => {
                if let Some(ref gs) = self.gig {
                    let missing: Vec<&dj_rs::spotify::SpotifyTrack> = gs.match_results.iter()
                        .filter(|r| r.matched_track_id.is_none())
                        .filter(|r| gs.pending_buy_tracks.iter().any(|b| b.spotify_id == r.spotify.spotify_id))
                        .map(|r| &r.spotify)
                        .collect();
                    let list = dj_rs::matcher::shopping_list(&missing);
                    return iced::clipboard::write(list);
                }
                Task::none()
            }

            Message::SettingsClicked => {
                if self.settings.is_some() {
                    self.settings = None;
                } else {
                    let config = Config::load();
                    self.settings = Some(SettingsState::from_config(&config));
                    self.contact = None;
                    self.gig = None;
                }
                Task::none()
            }

            Message::SettingsSave => {
                if let Some(ref ss) = self.settings {
                    let mut config = Config::load();
                    config.db_path = if ss.db_path.is_empty() {
                        None
                    } else {
                        Some(ss.db_path.clone())
                    };
                    config.path_mappings = ss.to_mappings();
                    config.music_library_path = if ss.music_library_path.is_empty() {
                        None
                    } else {
                        Some(ss.music_library_path.clone())
                    };
                    config.save();
                    // Update live app state — reopen DB if path changed
                    if let Some(ref db_path) = config.resolved_db_path() {
                        if let Ok(new_lib) = Library::open(db_path) {
                            self.lib = new_lib;
                        }
                    }
                    self.anlz_base = config.anlz_base_dir()
                        .map(|p| p.to_string_lossy().into_owned());
                    self.config = config;
                }
                if let Some(ref mut ss) = self.settings {
                    ss.dirty = false;
                }
                Task::none()
            }

            Message::SettingsMappingFromChanged(idx, val) => {
                if let Some(ref mut ss) = self.settings {
                    if let Some(m) = ss.path_mappings.get_mut(idx) {
                        m.from = val;
                        ss.dirty = true;
                    }
                }
                Task::none()
            }

            Message::SettingsMappingToChanged(idx, val) => {
                if let Some(ref mut ss) = self.settings {
                    if let Some(m) = ss.path_mappings.get_mut(idx) {
                        m.to = val;
                        ss.dirty = true;
                    }
                }
                Task::none()
            }

            Message::SettingsMappingAdd => {
                if let Some(ref mut ss) = self.settings {
                    ss.path_mappings.push(settings::MappingRow {
                        from: String::new(),
                        to: String::new(),
                    });
                    ss.dirty = true;
                }
                Task::none()
            }

            Message::SettingsMappingRemove(idx) => {
                if let Some(ref mut ss) = self.settings {
                    if idx < ss.path_mappings.len() {
                        ss.path_mappings.remove(idx);
                        ss.dirty = true;
                    }
                }
                Task::none()
            }

            Message::SettingsDbPathChanged(val) => {
                if let Some(ref mut ss) = self.settings {
                    ss.db_path = val;
                    ss.dirty = true;
                }
                Task::none()
            }

            Message::SettingsMusicPathChanged(val) => {
                if let Some(ref mut ss) = self.settings {
                    ss.music_library_path = val;
                    ss.dirty = true;
                }
                Task::none()
            }

            Message::SpotifyConnect => {
                if let Some(ref mut ss) = self.settings {
                    ss.spotify_status = "Waiting for browser…".to_string();
                }
                Task::perform(
                    async {
                        tokio::task::spawn_blocking(|| {
                            dj_rs::spotify::authorize()
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    Message::SpotifyConnectResult,
                )
            }

            Message::SpotifyConnectResult(result) => {
                match result {
                    Ok((access, refresh)) => {
                        let mut config = Config::load();
                        config.spotify_access_token = Some(access.clone());
                        config.spotify_refresh_token = Some(refresh.clone());
                        config.save();
                        self.spotify_token = Some(access);
                        self.spotify_refresh_token = Some(refresh);
                        if let Some(ref mut ss) = self.settings {
                            ss.spotify_connected = true;
                            ss.spotify_status = "Connected".to_string();
                        }
                    }
                    Err(e) => {
                        if let Some(ref mut ss) = self.settings {
                            ss.spotify_connected = false;
                            ss.spotify_status = format!("Error: {}", e);
                        }
                    }
                }
                Task::none()
            }

            Message::Tick(_) => {
                // Refresh Spotify token if we have a refresh token
                if let Some(ref rt) = self.spotify_refresh_token {
                    let rt = rt.clone();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                dj_rs::spotify::refresh(&rt)
                            })
                            .await
                            .map_err(|e| e.to_string())?
                        },
                        |result| match result {
                            Ok((token, new_refresh)) => Message::SpotifyTokenRefreshed(token, new_refresh),
                            Err(_) => Message::SpotifyTokenRefreshed(String::new(), None),
                        },
                    );
                }
                Task::none()
            }

            Message::SpotifyTokenRefreshed(token, new_refresh) => {
                if !token.is_empty() {
                    self.spotify_token = Some(token.clone());
                    // Persist to config
                    let mut config = Config::load();
                    config.spotify_access_token = Some(token);
                    if let Some(nr) = new_refresh {
                        config.spotify_refresh_token = Some(nr.clone());
                        self.spotify_refresh_token = Some(nr);
                    }
                    config.save();
                }
                Task::none()
            }

            Message::CuePressed => {
                let cue = self.player.cue_pos_secs;
                let _ = self.deck.seek_to(cue);
                self.deck.pause();
                self.player.play_pos_secs = cue;
                self.player.is_playing = false;
                Task::none()
            }

            Message::PlayPressed => {
                if self.player.is_playing {
                    self.deck.pause();
                    self.player.is_playing = false;
                } else {
                    self.deck.play();
                    self.player.is_playing = true;
                }
                self.player.play_pos_secs = self.deck.current_position_secs();
                Task::none()
            }

            Message::OverviewSeek(frac) => {
                let dur = self.player.duration_secs.unwrap_or(0) as f64;
                if dur > 0.0 {
                    let target = frac * dur;
                    let _ = self.deck.seek_to(target);
                    self.player.play_pos_secs = target;
                }
                Task::none()
            }

            Message::AudioTick(_) => {
                if self.player.is_playing {
                    self.player.play_pos_secs = self.deck.current_position_secs();
                    // Check if track ended
                    let dur = self.deck.duration_secs;
                    if dur > 0.0 && self.player.play_pos_secs >= dur - 0.1 {
                        self.deck.pause();
                        self.player.is_playing = false;
                    }
                    self.deck.check_loop();
                }
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let player = player::view(&self.player);

        let detail: Option<Element<Message>> = if let Some(ref ss) = self.settings {
            Some(settings::view(ss))
        } else if let Some(ref gs) = self.gig {
            Some(gig::view(gs))
        } else if let Some(ref cs) = self.contact {
            let gigs: Vec<_> = self.browser.gig_store.gigs.iter()
                .filter(|g| g.contact_id == cs.contact_id)
                .cloned()
                .collect();
            Some(contact::view(cs, &gigs))
        } else {
            None
        };

        let active_contact_id = self.contact.as_ref().map(|c| c.contact_id.as_str());
        let main_area = browser::view(&self.browser, detail, active_contact_id);

        container(column![player, main_area])
            .width(Fill)
            .height(Fill)
            .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs: Vec<Subscription<Message>> = vec![
            iced::time::every(std::time::Duration::from_secs(300)).map(Message::Tick),
        ];
        // 60fps tick only while playing — avoids idle CPU burn
        if self.player.is_playing {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(16)).map(Message::AudioTick),
            );
        }
        // Track mouse during info panel resize drag
        if self.browser.track_info_dragging {
            subs.push(iced::event::listen_with(|event, _, _| {
                match event {
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) =>
                        Some(Message::TrackInfoDragMove(position.x)),
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) =>
                        Some(Message::TrackInfoDragEnd),
                    _ => None,
                }
            }));
        }
        Subscription::batch(subs)
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn load_tracks(lib: &Library, selection: Selection) -> Vec<Track> {
    match selection {
        Selection::All => lib.tracks().unwrap_or_default(),
        Selection::Playlist(id) => lib.playlist_tracks(id).unwrap_or_default(),
        _ => vec![],
    }
}

fn load_spotify_tracks(
    token: String,
    playlist_id: String,
    lib: &Library,
) -> Result<Vec<browser::SpotifyTrackRow>, String> {
    let spotify_tracks = dj_rs::spotify::fetch_playlist(&token, &playlist_id)?;

    let library_tracks = lib.tracks().map_err(|e| e.to_string())?;

    let match_results = dj_rs::matcher::match_tracks(&spotify_tracks, &library_tracks);

    Ok(match_results.into_iter().map(|r| {
        browser::SpotifyTrackRow {
            in_library: r.matched.is_some(),
            library_track_id: r.matched.as_ref().map(|t| t.id),
            spotify: r.spotify,
        }
    }).collect())
}

async fn run_match(
    token: String,
    playlist_url: String,
    lib: &Library,
) -> Result<Vec<MatchResultEntry>, String> {
    let spotify_tracks = dj_rs::spotify::fetch_playlist(&token, &playlist_url)
        .map_err(|e| e.to_string())?;

    let library_tracks = lib.tracks().map_err(|e| e.to_string())?;

    let results = dj_rs::matcher::match_tracks(&spotify_tracks, &library_tracks);

    Ok(results.into_iter().map(|r| {
        MatchResultEntry {
            matched_track_id: r.matched.as_ref().map(|t| t.id),
            matched_title: r.matched.as_ref().map(|t| t.title.clone()),
            matched_artist: r.matched.as_ref().and_then(|t| t.artist.clone()),
            spotify: r.spotify,
        }
    }).collect())
}
