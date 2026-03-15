pub mod browser;
pub mod player;
pub mod theme;

use iced::widget::{column, container};
use iced::{Element, Fill, Task, Theme};
use dj_rs::rekordbox::{CuePoint, Library, Track};
use dj_rs::config::Config;
use dj_rs::gig::GigStore;
use browser::{BrowserState, Selection};
use player::PlayerState;

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
    TrackClicked(i64),
    // Contacts / Gigs
    ContactToggled(String),
    GigSelected(String, Option<i64>),
    // Player transport
    CuePressed,
    PlayPressed,
    // Player data loading
    WaveformLoaded(Option<Vec<u8>>, Option<Vec<u8>>),
    CuesLoaded(Vec<CuePoint>),
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    pub browser: BrowserState,
    pub player: PlayerState,
    db_path: String,
    anlz_base: Option<String>,
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

        let anlz_base = config.anlz_base_dir()
            .map(|p| p.to_string_lossy().into_owned());

        let playlists = Library::open(&db_path)
            .ok()
            .and_then(|lib| lib.playlists().ok())
            .unwrap_or_default();

        let all_tracks = Library::open(&db_path)
            .ok()
            .and_then(|lib| lib.tracks().ok())
            .unwrap_or_default();

        let gig_store = GigStore::load();

        let mut browser = BrowserState::new(playlists, gig_store);
        browser.tracks = all_tracks;
        browser.selection = Selection::All;

        (Self { browser, player: PlayerState::new(), db_path, anlz_base }, Task::none())
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::SectionClicked(section) => {
                if self.browser.section == section {
                    self.browser.sidebar_open = !self.browser.sidebar_open;
                } else {
                    self.browser.section = section;
                    self.browser.sidebar_open = true;
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
                self.browser.selection = sel.clone();
                let db_path = self.db_path.clone();
                Task::perform(
                    async move { load_tracks(db_path, sel).await },
                    Message::TracksLoaded,
                )
            }

            Message::TracksLoaded(tracks) => {
                self.browser.tracks = tracks;
                Task::none()
            }

            Message::SearchChanged(query) => {
                self.browser.search = query.clone();
                let db_path = self.db_path.clone();
                if query.is_empty() {
                    let sel = self.browser.selection.clone();
                    Task::perform(
                        async move { load_tracks(db_path, sel).await },
                        Message::TracksLoaded,
                    )
                } else {
                    Task::perform(
                        async move {
                            Library::open(&db_path)
                                .ok()
                                .and_then(|lib| lib.search_tracks(&query).ok())
                                .unwrap_or_default()
                        },
                        Message::TracksLoaded,
                    )
                }
            }

            Message::TrackClicked(id) => {
                if let Some(track) = self.browser.tracks.iter().find(|t| t.id == id) {
                    self.player.load_track(
                        track.id,
                        track.title.clone(),
                        track.artist.clone().unwrap_or_default(),
                        track.duration_secs,
                        track.bpm.map(|b| b as f32 / 100.0),
                        track.key.clone(),
                    );
                }

                // Load cues
                let db = self.db_path.clone();
                let cue_task = Task::perform(
                    async move {
                        Library::open(&db)
                            .ok()
                            .and_then(|lib| lib.load_cues(id).ok())
                            .unwrap_or_default()
                    },
                    Message::CuesLoaded,
                );

                // Load waveform if anlz_base is known
                if let Some(base) = self.anlz_base.clone() {
                    let db = self.db_path.clone();
                    let wf_task = Task::perform(
                        async move {
                            let base_path = std::path::Path::new(&base);
                            Library::open(&db)
                                .ok()
                                .and_then(|lib| lib.load_waveform(id, base_path).ok())
                                .map(|(c, o)| (c, o))
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
                self.player.cue_points = cues;
                Task::none()
            }

            Message::ContactToggled(contact_id) => {
                if self.browser.expanded_contacts.contains(&contact_id) {
                    self.browser.expanded_contacts.remove(&contact_id);
                } else {
                    self.browser.expanded_contacts.insert(contact_id);
                }
                Task::none()
            }

            Message::GigSelected(_gig_id, playlist_id) => {
                if let Some(folder_id) = playlist_id {
                    self.browser.selection = Selection::Playlist(folder_id);
                    let db_path = self.db_path.clone();
                    Task::perform(
                        async move { load_tracks(db_path, Selection::Playlist(folder_id)).await },
                        Message::TracksLoaded,
                    )
                } else {
                    Task::none()
                }
            }

            Message::CuePressed => {
                self.player.play_pos_secs = self.player.cue_pos_secs;
                Task::none()
            }

            Message::PlayPressed => {
                self.player.is_playing = !self.player.is_playing;
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let player  = player::view(&self.player);
        let browser = browser::view(&self.browser);

        container(column![player, browser])
            .width(Fill)
            .height(Fill)
            .into()
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }
}

// ── DB helpers ────────────────────────────────────────────────────────────────

async fn load_tracks(db_path: String, selection: Selection) -> Vec<Track> {
    match selection {
        Selection::All => Library::open(&db_path)
            .ok()
            .and_then(|lib| lib.tracks().ok())
            .unwrap_or_default(),
        Selection::Playlist(id) => Library::open(&db_path)
            .ok()
            .and_then(|lib| lib.playlist_tracks(id).ok())
            .unwrap_or_default(),
        _ => vec![],
    }
}
