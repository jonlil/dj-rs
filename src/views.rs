use gtk::prelude::*;
use indexmap::IndexMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::sync::Arc;
use glib::types::StaticType;
use crate::deck::DeckState;
use crate::config::{Config, PathMapping};
use crate::gig::{Contact, CustomerType, Gig, GigStore};
use crate::rekordbox::{Library, Track, Playlist, HistorySession, TrackFilter, compatible_camelot_keys};
use crate::server::{ServerBridge, WsEvent};

fn fmt_time(secs: f64) -> String {
    let s = secs as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn rating_stars(r: i32) -> &'static str {
    match r {
        1 => "★",
        2 => "★★",
        3 => "★★★",
        4 => "★★★★",
        5 => "★★★★★",
        _ => "",
    }
}

pub struct PlayerView {
    pub container: gtk::Frame,
    pub volume_scale: gtk::Scale,
    pub state: Rc<RefCell<DeckState>>,
    pub queue_fn: Rc<dyn Fn(Track)>,
    pub current_track_db_id: Rc<RefCell<Option<i64>>>,
    pub on_track_end: Rc<RefCell<Option<Rc<dyn Fn(i64)>>>>,
}

impl PlayerView {
    pub fn new(_window: &gtk::ApplicationWindow, deck_label: &str, bridge: Arc<ServerBridge>) -> Self {
        let state = Rc::new(RefCell::new(DeckState::new()));
        let current_track_db_id: Rc<RefCell<Option<i64>>> = Rc::new(RefCell::new(None));
        let on_track_end: Rc<RefCell<Option<Rc<dyn Fn(i64)>>>> = Rc::new(RefCell::new(None));

        let frame = gtk::Frame::new(Some(deck_label));
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 6);
        vbox.set_border_width(8);

        // ── Info row: [album art] [title / BPM] [artist / Key] ──────────────

        // Album art placeholder — grey square, replaced later with actual art
        let art_placeholder = gtk::DrawingArea::new();
        art_placeholder.set_size_request(80, 80);
        art_placeholder.connect_draw(|w, cr| {
            let alloc = w.get_allocation();
            cr.set_source_rgb(0.25, 0.25, 0.25);
            cr.rectangle(0.0, 0.0, alloc.width as f64, alloc.height as f64);
            cr.fill();
            gtk::Inhibit(false)
        });

        let track_label = gtk::Label::new(Some("No track loaded"));
        track_label.set_xalign(0.0);
        track_label.set_hexpand(true);

        let bpm_label = gtk::Label::new(None::<&str>);
        bpm_label.set_xalign(1.0);

        let artist_label = gtk::Label::new(None::<&str>);
        artist_label.set_xalign(0.0);
        artist_label.set_hexpand(true);

        let key_label = gtk::Label::new(None::<&str>);
        key_label.set_xalign(1.0);

        let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        title_row.pack_start(&track_label, true,  true,  0);
        title_row.pack_end  (&bpm_label,   false, false, 0);

        let artist_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        artist_row.pack_start(&artist_label, true,  true,  0);
        artist_row.pack_end  (&key_label,    false, false, 0);

        let meta_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        meta_box.pack_start(&title_row,  false, false, 0);
        meta_box.pack_start(&artist_row, false, false, 0);

        let info_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        info_row.pack_start(&art_placeholder, false, false, 0);
        info_row.pack_start(&meta_box,        true,  true,  0);

        // ── Waveform row: [waveform placeholder] [-M:SS] ─────────────────────

        // Waveform placeholder — grey bar; replaced later with ANLZ colour waveform
        let waveform_area = gtk::DrawingArea::new();
        waveform_area.set_size_request(-1, 80);
        waveform_area.set_hexpand(true);
        waveform_area.connect_draw(|w, cr| {
            let alloc = w.get_allocation();
            cr.set_source_rgb(0.15, 0.15, 0.15);
            cr.rectangle(0.0, 0.0, alloc.width as f64, alloc.height as f64);
            cr.fill();
            gtk::Inhibit(false)
        });

        // Time display (remaining)
        let time_label = gtk::Label::new(Some("-0:00"));
        time_label.set_xalign(1.0);

        let wave_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        wave_row.pack_start(&waveform_area, true,  true,  0);
        wave_row.pack_end  (&time_label,    false, false, 4);

        // ── Position slider ───────────────────────────────────────────────────

        let pos_adj = gtk::Adjustment::new(0.0, 0.0, 1.0, 0.001, 0.01, 0.0);
        let position_scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&pos_adj));
        position_scale.set_draw_value(false);
        position_scale.set_hexpand(true);
        position_scale.set_sensitive(false);

        // ── Controls: [Cue] [▶/❚❚]  +  TV toggle (right) ────────────────────

        let play_btn = gtk::Button::with_label("▶  Play");
        let cue_btn  = gtk::Button::with_label("Cue");
        let tv_btn   = gtk::ToggleButton::with_label("TV");
        tv_btn.set_sensitive(false);

        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        controls.pack_start(&cue_btn,  false, false, 0);
        controls.pack_start(&play_btn, false, false, 0);
        controls.pack_end  (&tv_btn,   false, false, 0);

        // Volume scale (hidden, used programmatically)
        let vol_adj = gtk::Adjustment::new(1.0, 0.0, 1.5, 0.01, 0.1, 0.0);
        let volume_scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&vol_adj));

        // TV output active state (shared across closures on the GTK thread)
        let tv_output: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

        // Wire volume slider so it respects TV-output mute
        {
            let state = state.clone();
            let tv_output_vol = tv_output.clone();
            volume_scale.connect_value_changed(move |scale| {
                if !*tv_output_vol.borrow() {
                    state.borrow().sink.set_volume(scale.get_value() as f32);
                }
            });
        }

        // ── Assemble ──────────────────────────────────────────────────────────

        vbox.pack_start(&info_row,      false, false, 0);
        vbox.pack_start(&wave_row,      false, false, 0);
        vbox.pack_start(&position_scale, false, false, 0);
        vbox.pack_start(&controls,       false, false, 0);

        frame.add(&vbox);

        // Shared load-track logic (drag-and-drop + queue auto-advance)
        let do_load_track = {
            let state              = state.clone();
            let track_label        = track_label.clone();
            let artist_label       = artist_label.clone();
            let bpm_label          = bpm_label.clone();
            let key_label          = key_label.clone();
            let position_scale     = position_scale.clone();
            let time_label         = time_label.clone();
            let play_btn_load      = play_btn.clone();
            let current_db_id_load = current_track_db_id.clone();
            let bridge_load        = bridge.clone();
            Rc::new(move |track: Track| {
                let path = match track.file_path.as_deref() {
                    Some(p) => std::path::PathBuf::from(p),
                    None    => return,
                };
                let title  = track.title.clone();
                let artist = track.artist.as_deref().unwrap_or("").to_string();
                let bpm_str = track.bpm_display()
                    .map(|b| format!("BPM: {:.1}", b))
                    .unwrap_or_default();
                let key_str = track.key.as_deref()
                    .map(|k| format!("Key: {}", k))
                    .unwrap_or_default();
                let db_duration = track.duration_secs.map(|s| s as f64).unwrap_or(0.0);
                if state.borrow_mut().load(path).is_ok() {
                    // DB duration is more reliable than rodio's total_duration
                    if db_duration > 0.0 {
                        state.borrow_mut().duration_secs = db_duration;
                    }
                    if track.id != 0 {
                        *current_db_id_load.borrow_mut() = Some(track.id);
                    }
                    track_label.set_text(&title);
                    artist_label.set_text(&artist);
                    bpm_label.set_text(&bpm_str);
                    key_label.set_text(&key_str);
                    play_btn_load.set_label("▶  Play");
                    position_scale.set_sensitive(true);
                    let dur = state.borrow().duration_secs;
                    if dur > 0.0 {
                        time_label.set_text(&format!("-{}", fmt_time(dur)));
                    } else {
                        time_label.set_text("-?");
                    }
                    bridge_load.send(WsEvent::Metadata {
                        title,
                        artist,
                        duration: dur,
                    });
                    bridge_load.send(WsEvent::Position { pos: 0.0 });
                    bridge_load.send(WsEvent::State { playing: false });
                } else {
                    track_label.set_text("Error loading file");
                }
            })
        };

        // Drag-and-drop onto the deck frame
        {
            let dnd_targets = [gtk::TargetEntry::new("text/plain", gtk::TargetFlags::empty(), 0)];
            frame.drag_dest_set(gtk::DestDefaults::ALL, &dnd_targets, gdk::DragAction::COPY);
            let do_load = do_load_track.clone();
            frame.connect_drag_data_received(move |_w, _ctx, _x, _y, sel, _info, _time| {
                let path_str = match sel.get_text() {
                    Some(s) => s.to_string(),
                    None    => return,
                };
                let title = std::path::Path::new(&path_str)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Unknown")
                    .to_string();
                do_load(Track {
                    id: 0, title, artist: None, album: None, genre: None,
                    key: None, bpm: None, duration_secs: None, rating: None,
                    play_count: None, file_path: Some(path_str), track_no: None,
                    label: None, color_id: None,
                });
            });
        }

        // Play/Pause button
        {
            let state                 = state.clone();
            let play_btn_ref          = play_btn.clone();
            let bridge_play           = bridge.clone();
            let tv_output_play        = tv_output.clone();
            let current_track_db_play = current_track_db_id.clone();
            play_btn.connect_clicked(move |_| {
                let is_playing = state.borrow().play_started_at.is_some();
                if is_playing {
                    state.borrow_mut().pause();
                    play_btn_ref.set_label("▶  Play");
                    bridge_play.send(WsEvent::State { playing: false });
                } else {
                    state.borrow_mut().play();
                    if state.borrow().play_started_at.is_some() {
                        play_btn_ref.set_label("❚❚  Pause");
                        if *tv_output_play.borrow() {
                            // Keep local audio muted; tell TV to stream
                            state.borrow().sink.set_volume(0.0);
                            let pos = state.borrow().current_position_secs();
                            if let Some(id) = *current_track_db_play.borrow() {
                                bridge_play.send(WsEvent::Stream { id, seek: pos });
                            }
                        }
                        bridge_play.send(WsEvent::State { playing: true });
                    }
                }
            });
        }

        // Cue button: stop and return to beginning
        {
            let state          = state.clone();
            let play_btn       = play_btn.clone();
            let position_scale = position_scale.clone();
            let time_label     = time_label.clone();
            let bridge_cue     = bridge.clone();
            cue_btn.connect_clicked(move |_| {
                state.borrow_mut().stop();
                play_btn.set_label("▶  Play");
                position_scale.set_value(0.0);
                let dur = state.borrow().duration_secs;
                time_label.set_text(&format!(
                    "-{}",
                    if dur > 0.0 { fmt_time(dur) } else { "?".into() }
                ));
                bridge_cue.send(WsEvent::State    { playing: false });
                bridge_cue.send(WsEvent::Position { pos: 0.0 });
            });
        }

        // TV output toggle: mute local sink and stream to TV when active
        {
            let state               = state.clone();
            let tv_output_btn       = tv_output.clone();
            let bridge_tv           = bridge.clone();
            let current_track_db_tv = current_track_db_id.clone();
            let volume_scale_tv     = volume_scale.clone();
            tv_btn.connect_toggled(move |btn| {
                let active = btn.get_active();
                *tv_output_btn.borrow_mut() = active;
                if active {
                    state.borrow().sink.set_volume(0.0);
                    if state.borrow().play_started_at.is_some() {
                        let pos = state.borrow().current_position_secs();
                        if let Some(id) = *current_track_db_tv.borrow() {
                            bridge_tv.send(WsEvent::Stream { id, seek: pos });
                        }
                    }
                } else {
                    state.borrow().sink.set_volume(volume_scale_tv.get_value() as f32);
                }
            });
        }

        // Internal queued-track state (for auto-advance; no UI shown)
        let queued_track: Rc<RefCell<Option<Track>>> = Rc::new(RefCell::new(None));

        let queue_fn: Rc<dyn Fn(Track)> = {
            let queued_track = queued_track.clone();
            Rc::new(move |track: Track| {
                *queued_track.borrow_mut() = Some(track);
            })
        };

        // Position update + track-end + auto-advance timer
        {
            let state                = state.clone();
            let queued_track         = queued_track.clone();
            let do_load              = do_load_track.clone();
            let position_scale       = position_scale.clone();
            let time_label           = time_label.clone();
            let play_btn             = play_btn.clone();
            let current_track_db_id2 = current_track_db_id.clone();
            let on_track_end2        = on_track_end.clone();
            let bridge_timer         = bridge.clone();
            let tv_output_timer      = tv_output.clone();
            let tv_btn_timer         = tv_btn.clone();
            let volume_scale_timer   = volume_scale.clone();
            let mut tick: u32        = 0;
            glib::timeout_add_local(100, move || {
                tick += 1;

                // Keep TV button in sync with connection state
                let tv_live = bridge_timer.tv_connected();
                if tv_btn_timer.get_sensitive() != tv_live {
                    tv_btn_timer.set_sensitive(tv_live);
                }
                // If TV disconnected while it was the active output, fall back to local
                if !tv_live && *tv_output_timer.borrow() {
                    *tv_output_timer.borrow_mut() = false;
                    tv_btn_timer.set_active(false);
                    state.borrow().sink.set_volume(volume_scale_timer.get_value() as f32);
                }

                // Apply any seek requested by the TV
                if let Some(seek_pos) = bridge_timer.take_seek() {
                    let _ = state.borrow_mut().seek_to(seek_pos);
                    let dur = state.borrow().duration_secs;
                    let fraction = if dur > 0.0 { (seek_pos / dur).min(1.0) } else { 0.0 };
                    position_scale.set_value(fraction);
                    let remaining = if dur > 0.0 { (dur - seek_pos).max(0.0) } else { 0.0 };
                    time_label.set_text(&format!("-{}", fmt_time(remaining)));
                    bridge_timer.send(WsEvent::Position { pos: seek_pos });
                    // When TV is the output, restart the stream at the new position
                    if *tv_output_timer.borrow() {
                        if let Some(id) = *current_track_db_id2.borrow() {
                            bridge_timer.send(WsEvent::Stream { id, seek: seek_pos });
                        }
                    }
                }

                let (is_started, sink_empty) = {
                    let st = state.borrow();
                    (st.play_started_at.is_some(), st.sink.empty())
                };

                if is_started && sink_empty {
                    {
                        let maybe_cb = on_track_end2.borrow().clone();
                        if let Some(cb) = maybe_cb {
                            if let Some(id) = *current_track_db_id2.borrow() {
                                cb(id);
                            }
                        }
                    }
                    {
                        let mut st = state.borrow_mut();
                        st.play_started_at = None;
                        st.accumulated_secs = 0.0;
                    }
                    play_btn.set_label("▶  Play");
                    position_scale.set_value(0.0);
                    let dur = state.borrow().duration_secs;
                    time_label.set_text(&format!("-{}", fmt_time(dur)));
                    bridge_timer.send(WsEvent::State    { playing: false });
                    bridge_timer.send(WsEvent::Position { pos: 0.0 });

                    if let Some(track) = queued_track.borrow_mut().take() {
                        do_load(track);
                        state.borrow_mut().play();
                        if state.borrow().play_started_at.is_some() {
                            play_btn.set_label("❚❚  Pause");
                            if *tv_output_timer.borrow() {
                                state.borrow().sink.set_volume(0.0);
                                if let Some(id) = *current_track_db_id2.borrow() {
                                    bridge_timer.send(WsEvent::Stream { id, seek: 0.0 });
                                }
                            }
                            bridge_timer.send(WsEvent::State { playing: true });
                        }
                    }

                    return glib::Continue(true);
                }

                if is_started {
                    let (pos, dur) = {
                        let st = state.borrow();
                        (st.current_position_secs(), st.duration_secs)
                    };
                    let fraction = if dur > 0.0 { (pos / dur).min(1.0) } else { 0.0 };
                    position_scale.set_value(fraction);
                    let remaining = if dur > 0.0 { (dur - pos).max(0.0) } else { 0.0 };
                    time_label.set_text(&format!("-{}", fmt_time(remaining)));

                    // Broadcast position to TV every ~1 second
                    if tick % 10 == 0 {
                        bridge_timer.send(WsEvent::Position { pos });
                    }
                }

                glib::Continue(true)
            });
        }

        PlayerView {
            container: frame,
            volume_scale,
            state,
            queue_fn,
            current_track_db_id,
            on_track_end,
        }
    }
}

pub struct MainView {
    pub container: gtk::Box,
    pub queue_fn: Rc<dyn Fn(Track)>,
    pub current_track_db_id: Rc<RefCell<Option<i64>>>,
    pub on_track_end: Rc<RefCell<Option<Rc<dyn Fn(i64)>>>>,
}

impl MainView {
    pub fn new(window: &gtk::ApplicationWindow, bridge: Arc<ServerBridge>) -> Self {
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.set_border_width(8);

        let player = PlayerView::new(window, "Deck", bridge);
        let queue_fn = player.queue_fn.clone();
        let current_track_db_id = player.current_track_db_id.clone();
        let on_track_end = player.on_track_end.clone();

        container.pack_start(&player.container, true, true, 0);

        MainView { container, queue_fn, current_track_db_id, on_track_end }
    }
}

// ─── column indices ──────────────────────────────────────────────────────────

const P_NAME:  u32 = 0;  // playlist name
const P_COUNT: u32 = 1;  // track count (display string)
const P_ID:    u32 = 2;  // id as string, "all" for the catch-all row
const P_ATTR:  u32 = 3;  // attribute: "0" = playlist, "1" = folder, "h" = history

const T_TITLE:    u32 = 0;
const T_ARTIST:   u32 = 1;
const T_BPM:      u32 = 2;
const T_KEY:      u32 = 3;
const T_DURATION: u32 = 4;
const T_FILE_PATH: u32 = 5;  // hidden column
const T_GENRE:    u32 = 6;
const T_RATING:   u32 = 7;
const T_LABEL:    u32 = 8;
const T_COLOR:    u32 = 9;   // color_id as string, hidden
const T_TRACK_ID:      u32 = 10;  // db id as string, hidden
const T_BPM_RAW:      u32 = 11;  // raw bpm i32 as string, hidden
const T_DURATION_RAW: u32 = 12;  // raw duration seconds i32 as string, hidden

// ─── BrowserView ─────────────────────────────────────────────────────────────

pub struct BrowserView {
    pub container: gtk::Box,
}

impl BrowserView {
    pub fn new(
        window: &gtk::ApplicationWindow,
        config: Rc<RefCell<Config>>,
        on_queue: Option<Rc<dyn Fn(Track)>>,
        current_track_db_id: Rc<RefCell<Option<i64>>>,
        on_track_end: Rc<RefCell<Option<Rc<dyn Fn(i64)>>>>,
    ) -> Self {
        let library: Rc<RefCell<Option<Library>>> = Rc::new(RefCell::new(None));
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);

        // Current playlist selection: None = All Tracks, Some(id) = playlist
        let current_playlist_id: Rc<RefCell<Option<i64>>> = Rc::new(RefCell::new(None));
        // Current key for harmonic mode
        let harmonic_key: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

        // ── top bar ──────────────────────────────────────────────────────────
        let topbar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        topbar.set_border_width(4);

        // ⋮ menu button for Open Library + Settings
        let menu_btn = gtk::MenuButton::new();
        menu_btn.set_label("☰");
        menu_btn.set_relief(gtk::ReliefStyle::None);
        menu_btn.set_tooltip_text(Some("Menu"));
        let menu = gtk::Menu::new();
        let open_btn     = gtk::MenuItem::with_label("Open Library…");
        let settings_btn = gtk::MenuItem::with_label("Settings…");
        menu.append(&open_btn);
        menu.append(&settings_btn);
        menu.show_all();
        menu_btn.set_popup(Some(&menu));

        // Small reload button
        let reload_btn = gtk::Button::with_label("↺");
        reload_btn.set_relief(gtk::ReliefStyle::None);
        reload_btn.set_tooltip_text(Some("Reload library"));

        let status_lbl   = gtk::Label::new(Some("No library loaded"));
        let search_entry = gtk::Entry::new();
        search_entry.set_placeholder_text(Some("Search tracks…"));
        search_entry.set_hexpand(true);

        topbar.pack_start(&menu_btn,     false, false, 0);
        topbar.pack_start(&reload_btn,   false, false, 0);
        topbar.pack_start(&status_lbl,   false, false, 8);
        topbar.pack_end(&search_entry,   false, false, 0);

        // ── filter bar ───────────────────────────────────────────────────────
        let filter_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        filter_bar.set_border_width(4);

        filter_bar.pack_start(&gtk::Label::new(Some("BPM:")), false, false, 0);
        let bpm_min_spin = gtk::SpinButton::with_range(0.0, 250.0, 1.0);
        bpm_min_spin.set_value(0.0);
        bpm_min_spin.set_tooltip_text(Some("Min BPM (0 = no filter)"));
        filter_bar.pack_start(&bpm_min_spin, false, false, 0);
        filter_bar.pack_start(&gtk::Label::new(Some("–")), false, false, 0);
        let bpm_max_spin = gtk::SpinButton::with_range(0.0, 250.0, 1.0);
        bpm_max_spin.set_value(0.0);
        bpm_max_spin.set_tooltip_text(Some("Max BPM (0 = no filter)"));
        filter_bar.pack_start(&bpm_max_spin, false, false, 0);

        filter_bar.pack_start(&gtk::Label::new(Some("Key:")), false, false, 0);
        let key_combo = gtk::ComboBoxText::new();
        key_combo.append_text("Any");
        filter_bar.pack_start(&key_combo, false, false, 0);

        filter_bar.pack_start(&gtk::Label::new(Some("Genre:")), false, false, 0);
        let genre_combo = gtk::ComboBoxText::new();
        genre_combo.append_text("Any");
        filter_bar.pack_start(&genre_combo, false, false, 0);

        filter_bar.pack_start(&gtk::Label::new(Some("Rating:")), false, false, 0);
        let rating_combo = gtk::ComboBoxText::new();
        for label in &["Any", "★+", "★★+", "★★★+", "★★★★+", "★★★★★"] {
            rating_combo.append_text(label);
        }
        rating_combo.set_active(Some(0));
        filter_bar.pack_start(&rating_combo, false, false, 0);

        let harmonic_btn = gtk::ToggleButton::with_label("Harmonic");
        let harmonic_key_lbl = gtk::Label::new(Some(""));
        harmonic_key_lbl.set_hexpand(true);
        filter_bar.pack_start(&harmonic_btn, false, false, 0);
        filter_bar.pack_start(&harmonic_key_lbl, false, false, 0);

        let clear_btn = gtk::Button::with_label("Clear");
        filter_bar.pack_end(&clear_btn, false, false, 0);

        // ── stores ───────────────────────────────────────────────────────────
        let str_t = String::static_type();
        let pl_store = gtk::TreeStore::new(&[str_t, str_t, str_t, str_t]);
        // 13 columns: title, artist, bpm, key, duration, file_path, genre, rating, label, color_id, track_id, bpm_raw, duration_raw
        let track_store = gtk::ListStore::new(&[
            str_t, str_t, str_t, str_t, str_t, str_t, str_t, str_t, str_t, str_t, str_t, str_t, str_t,
        ]);

        // ── playlist panel ───────────────────────────────────────────────────
        let pl_view = gtk::TreeView::new();
        pl_view.set_model(Some(&pl_store));
        pl_view.set_headers_visible(true);
        pl_view.set_enable_search(false);

        for &(title, idx, expand) in &[
            ("Playlist", P_NAME as i32, true),
            ("#",        P_COUNT as i32, false),
        ] {
            let col = gtk::TreeViewColumn::new();
            let cell = gtk::CellRendererText::new();
            col.pack_start(&cell, true);
            col.add_attribute(&cell, "text", idx);
            col.set_title(title);
            col.set_expand(expand);
            pl_view.append_column(&col);
        }

        let pl_scroll = gtk::ScrolledWindow::new(
            gtk::NONE_ADJUSTMENT,
            gtk::NONE_ADJUSTMENT,
        );
        pl_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        pl_scroll.set_min_content_height(300);
        pl_scroll.add(&pl_view);

        // ── history panel ─────────────────────────────────────────────────────
        let hist_store = gtk::ListStore::new(&[str_t, str_t, str_t, str_t]);
        let hist_view  = gtk::TreeView::new();
        hist_view.set_model(Some(&hist_store));
        hist_view.set_headers_visible(false);
        hist_view.set_enable_search(false);

        for &(title, idx, expand) in &[
            ("Session", P_NAME as i32,  true),
            ("#",       P_COUNT as i32, false),
        ] {
            let col  = gtk::TreeViewColumn::new();
            let cell = gtk::CellRendererText::new();
            col.pack_start(&cell, true);
            col.add_attribute(&cell, "text", idx);
            col.set_title(title);
            col.set_expand(expand);
            hist_view.append_column(&col);
        }

        let hist_scroll = gtk::ScrolledWindow::new(
            gtk::NONE_ADJUSTMENT,
            gtk::NONE_ADJUSTMENT,
        );
        hist_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        hist_scroll.add(&hist_view);

        // ── sidebar (unified collapsible sections) ────────────────────────────
        let pl_expander = gtk::Expander::new(Some("Playlists"));
        pl_expander.set_expanded(true);
        pl_expander.add(&pl_scroll);

        let hist_expander = gtk::Expander::new(Some("History"));
        hist_expander.set_expanded(false);
        hist_expander.add(&hist_scroll);

        // Gigs section
        let expanded_contacts: Rc<RefCell<std::collections::HashSet<String>>> =
            Rc::new(RefCell::new(std::collections::HashSet::new()));

        let gig_list_box = gtk::ListBox::new();
        gig_list_box.set_selection_mode(gtk::SelectionMode::Single);
        gig_list_box.set_size_request(-1, 120);

        let new_gig_btn = gtk::Button::with_label("+");
        new_gig_btn.set_relief(gtk::ReliefStyle::None);
        new_gig_btn.set_tooltip_text(Some("New Contact"));

        let gigs_footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        gigs_footer.pack_start(&new_gig_btn, false, false, 0);
        gigs_footer.get_style_context().add_class("inline-toolbar");

        let gigs_vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        gigs_vbox.pack_start(&gig_list_box, false, false, 0);
        gigs_vbox.pack_start(&gigs_footer,  false, false, 0);
        let gigs_expander = gtk::Expander::new(Some("Contacts"));
        gigs_expander.set_expanded(true);
        gigs_expander.add(&gigs_vbox);

        let sidebar_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar_box.pack_start(&pl_expander,   true, true, 4);
        sidebar_box.pack_start(&gtk::Separator::new(gtk::Orientation::Horizontal), false, false, 0);
        sidebar_box.pack_start(&hist_expander, false, false, 4);
        sidebar_box.pack_start(&gtk::Separator::new(gtk::Orientation::Horizontal), false, false, 0);
        sidebar_box.pack_start(&gigs_expander, true, true, 4);

        let sidebar_scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
        sidebar_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        sidebar_scroll.set_size_request(240, -1);
        sidebar_scroll.add(&sidebar_box);

        // ── track panel ──────────────────────────────────────────────────────
        let track_view = gtk::TreeView::new();
        track_view.set_model(Some(&track_store));
        track_view.set_headers_visible(true);
        track_view.set_enable_search(false);

        for &(title, idx, expand) in &[
            ("Title",  T_TITLE as i32,    true),
            ("Artist", T_ARTIST as i32,   true),
            ("BPM",    T_BPM as i32,      false),
            ("Key",    T_KEY as i32,      false),
            ("Time",   T_DURATION as i32, false),
            ("Genre",  T_GENRE as i32,    false),
            ("Rating", T_RATING as i32,   false),
            ("Label",  T_LABEL as i32,    false),
        ] {
            let col = gtk::TreeViewColumn::new();
            let cell = gtk::CellRendererText::new();
            col.pack_start(&cell, true);
            col.add_attribute(&cell, "text", idx);
            col.set_title(title);
            col.set_expand(expand);
            col.set_resizable(true);
            col.set_sort_column_id(idx);
            track_view.append_column(&col);
        }

        // Tags label (below track list)
        let tags_label = gtk::Label::new(Some("Tags: —"));
        tags_label.set_xalign(0.0);
        tags_label.set_margin_start(4);
        tags_label.set_margin_end(4);

        // Rating row for selected track
        let rating_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        rating_row.set_margin_start(4);
        rating_row.pack_start(&gtk::Label::new(Some("Set rating:")), false, false, 0);
        let mut star_btns: Vec<gtk::Button> = Vec::new();
        for i in 1..=5i32 {
            let lbl: String = (0..i).map(|_| "★").collect();
            let btn = gtk::Button::with_label(&lbl);
            rating_row.pack_start(&btn, false, false, 0);
            star_btns.push(btn);
        }
        let clear_rating_btn = gtk::Button::with_label("✕");
        rating_row.pack_start(&clear_rating_btn, false, false, 0);

        let track_scroll = gtk::ScrolledWindow::new(
            gtk::NONE_ADJUSTMENT,
            gtk::NONE_ADJUSTMENT,
        );
        track_scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        track_scroll.add(&track_view);
        track_scroll.set_hexpand(true);
        track_scroll.set_vexpand(true);

        // Track panel vbox: scroll + tags + rating
        let track_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        track_panel.pack_start(&track_scroll, true, true, 0);
        track_panel.pack_start(&tags_label, false, false, 2);
        track_panel.pack_start(&rating_row, false, false, 2);

        // ── drag source on track list ─────────────────────────────────────────
        {
            let dnd_targets = [gtk::TargetEntry::new(
                "text/plain",
                gtk::TargetFlags::empty(),
                0,
            )];
            track_view.drag_source_set(
                gdk::ModifierType::BUTTON1_MASK,
                &dnd_targets,
                gdk::DragAction::COPY,
            );

            let config    = config.clone();
            let cur_db_id = current_track_db_id.clone();
            track_view.connect_drag_data_get(move |view, _ctx, sel, _info, _time| {
                let selection = view.get_selection();
                if let Some((model, iter)) = selection.get_selected() {
                    let raw: String = model
                        .get_value(&iter, T_FILE_PATH as i32)
                        .get::<String>().ok().flatten().unwrap_or_default();
                    let id_str: String = model
                        .get_value(&iter, T_TRACK_ID as i32)
                        .get::<String>().ok().flatten().unwrap_or_default();
                    if let Ok(id) = id_str.parse::<i64>() {
                        *cur_db_id.borrow_mut() = Some(id);
                    }
                    let mapped = config.borrow().apply_mappings(&raw);
                    sel.set_text(&mapped);
                }
            });
        }

        // ── track right-click context menu ───────────────────────────────────
        if let Some(on_queue) = on_queue {
            let config    = config.clone();
            let cur_db_id = current_track_db_id.clone();
            track_view.connect_button_press_event(move |view, event| {
                if event.get_button() != 3 { return gtk::Inhibit(false); }
                let selection = view.get_selection();
                if let Some((path, _, _, _)) = view.get_path_at_pos(
                    event.get_position().0 as i32,
                    event.get_position().1 as i32,
                ) {
                    if let Some(p) = path {
                        selection.select_path(&p);
                    }
                }
                let menu = gtk::Menu::new();
                let queue_item = gtk::MenuItem::with_label("Queue");
                {
                    let on_queue   = on_queue.clone();
                    let config     = config.clone();
                    let view       = view.clone();
                    let cur_db_id2 = cur_db_id.clone();
                    queue_item.connect_activate(move |_| {
                        let sel = view.get_selection();
                        if let Some((model, iter)) = sel.get_selected() {
                            let get_str = |col: u32| -> String {
                                model.get_value(&iter, col as i32)
                                    .get::<String>().ok().flatten().unwrap_or_default()
                            };
                            let raw      = get_str(T_FILE_PATH);
                            let id_str   = get_str(T_TRACK_ID);
                            let title    = get_str(T_TITLE);
                            let artist   = get_str(T_ARTIST);
                            let key      = get_str(T_KEY);
                            let genre    = get_str(T_GENRE);
                            let bpm_raw  = get_str(T_BPM_RAW).parse::<i32>().unwrap_or(0);
                            let dur_raw  = get_str(T_DURATION_RAW).parse::<i32>().unwrap_or(0);
                            let id: i64  = id_str.parse().unwrap_or(0);
                            if id != 0 { *cur_db_id2.borrow_mut() = Some(id); }
                            let mapped = config.borrow().apply_mappings(&raw);
                            on_queue(Track {
                                id,
                                title,
                                artist: if artist.is_empty() { None } else { Some(artist) },
                                album:  None,
                                genre:  if genre.is_empty() { None } else { Some(genre) },
                                key:    if key.is_empty() { None } else { Some(key) },
                                bpm:    if bpm_raw > 0 { Some(bpm_raw) } else { None },
                                duration_secs: if dur_raw > 0 { Some(dur_raw) } else { None },
                                rating: None, play_count: None,
                                file_path: Some(mapped),
                                track_no: None, label: None, color_id: None,
                            });
                        }
                    });
                }
                menu.append(&queue_item);
                menu.show_all();
                menu.popup_at_pointer(Some(event));
                gtk::Inhibit(true)
            });
        }

        // ── gig workspace ────────────────────────────────────────────────────
        let gig_workspace    = build_gig_workspace();
        let contact_view     = build_contact_view();

        // Right panel: stack switching between track list, contact view, and gig workspace
        let right_stack = gtk::Stack::new();
        right_stack.add_named(&track_panel,    "tracks");
        right_stack.add_named(&contact_view,   "contact");
        right_stack.add_named(&gig_workspace,  "gig");
        right_stack.set_visible_child_name("tracks");

        // Wire up back button in gig workspace — returns to contact view
        if let Some(back_btn) = find_widget(&gig_workspace, "gig_back_btn") {
            let right_stack_c   = right_stack.clone();
            let gig_workspace_c = gig_workspace.clone();
            let contact_view_c  = contact_view.clone();
            let library_c       = library.clone();
            back_btn.downcast::<gtk::Button>().unwrap()
                .connect_clicked(move |_| {
                    // The workspace widget name is "gig_workspace:{gig_id}" when a gig is loaded
                    let wname = gig_workspace_c.get_widget_name().to_string();
                    if let Some(gig_id) = wname.strip_prefix("gig_workspace:") {
                        let store = crate::gig::GigStore::load();
                        if let Some(gig) = store.gigs.iter().find(|g| g.id == gig_id) {
                            let contact_id = gig.contact_id.clone();
                            if let Some(contact) = store.contacts.iter().find(|c| c.id == contact_id) {
                                let gigs = store.gigs_for_contact(&contact_id);
                                let playlists = library_c.borrow().as_ref()
                                    .and_then(|lib| lib.playlists().ok())
                                    .unwrap_or_default();
                                load_contact_into_view(&contact_view_c, contact, &gigs, &playlists);
                                right_stack_c.set_visible_child_name("contact");
                                return;
                            }
                        }
                    }
                    right_stack_c.set_visible_child_name("tracks");
                });
        }

        // Wire up back button in contact view
        if let Some(back_btn) = find_widget(&contact_view, "contact_back_btn") {
            let right_stack_c  = right_stack.clone();
            let gig_list_box_c = gig_list_box.clone();
            back_btn.downcast::<gtk::Button>().unwrap()
                .connect_clicked(move |_| {
                    gig_list_box_c.unselect_all();
                    right_stack_c.set_visible_child_name("tracks");
                });
        }

        // ── contact view: auto-save on field change ───────────────────────────
        {
            let contact_view2 = contact_view.clone();
            let save = Rc::new(move || {
                // Derive contact ID from the view's widget name
                let view_name = contact_view2.get_widget_name().to_string();
                let contact_id = match view_name.strip_prefix("contact_view:") {
                    Some(id) if !id.is_empty() => id.to_string(),
                    _ => return,
                };
                let mut store = crate::gig::GigStore::load();
                if let Some(contact) = store.contacts.iter_mut().find(|c| c.id == contact_id) {
                    // Read name
                    if let Some(w) = find_widget(&contact_view2, "contact_name") {
                        if let Ok(e) = w.downcast::<gtk::Entry>() {
                            contact.name = e.get_text().to_string();
                        }
                    }
                    // Read type
                    if let Some(w) = find_widget(&contact_view2, "contact_type") {
                        if let Ok(combo) = w.downcast::<gtk::ComboBoxText>() {
                            contact.customer_type = match combo.get_active_id().as_deref() {
                                Some("corporate") => crate::gig::CustomerType::Corporate,
                                Some("venue")     => crate::gig::CustomerType::Venue,
                                _                 => crate::gig::CustomerType::Private,
                            };
                        }
                    }
                    // Read notes
                    if let Some(w) = find_widget(&contact_view2, "contact_notes") {
                        if let Ok(tv) = w.downcast::<gtk::TextView>() {
                            if let Some(buf) = tv.get_buffer() {
                                contact.notes = buf.get_text(
                                    &buf.get_start_iter(),
                                    &buf.get_end_iter(),
                                    false,
                                ).map(|s| s.to_string()).unwrap_or_default();
                            }
                        }
                    }
                    store.save();
                    // Flash "Saved" indicator
                    if let Some(w) = find_widget(&contact_view2, "contact_saved_lbl") {
                        if let Ok(lbl) = w.downcast::<gtk::Label>() {
                            lbl.set_text("✓ Saved");
                            let lbl_c = lbl.clone();
                            glib::timeout_add_local(2000, move || {
                                lbl_c.set_text("");
                                glib::Continue(false)
                            });
                        }
                    }
                }
            });

            if let Some(w) = find_widget(&contact_view, "contact_name") {
                if let Ok(e) = w.downcast::<gtk::Entry>() {
                    let save = save.clone();
                    e.connect_changed(move |_| save());
                }
            }
            if let Some(w) = find_widget(&contact_view, "contact_type") {
                if let Ok(combo) = w.downcast::<gtk::ComboBoxText>() {
                    let save = save.clone();
                    combo.connect_changed(move |_| save());
                }
            }
            if let Some(w) = find_widget(&contact_view, "contact_notes") {
                if let Ok(tv) = w.downcast::<gtk::TextView>() {
                    if let Some(buf) = tv.get_buffer() {
                        let save = save.clone();
                        buf.connect_changed(move |_| save());
                    }
                }
            }
        }

        // ── gig workspace: auto-save on field change ─────────────────────────
        {
            let gig_workspace2 = gig_workspace.clone();
            let save = Rc::new(move || {
                let wname = gig_workspace2.get_widget_name().to_string();
                let gig_id = match wname.strip_prefix("gig_workspace:") {
                    Some(id) if !id.is_empty() => id.to_string(),
                    _ => return,
                };
                let mut store = crate::gig::GigStore::load();
                if let Some(gig) = store.gigs.iter_mut().find(|g| g.id == gig_id) {
                    macro_rules! read_entry { ($name:expr, $field:expr) => {
                        if let Some(w) = find_widget(&gig_workspace2, $name) {
                            if let Ok(e) = w.downcast::<gtk::Entry>() {
                                let v = e.get_text().to_string();
                                $field = if v.is_empty() { None } else { Some(v) };
                            }
                        }
                    }; }
                    macro_rules! read_entry_str { ($name:expr, $field:expr) => {
                        if let Some(w) = find_widget(&gig_workspace2, $name) {
                            if let Ok(e) = w.downcast::<gtk::Entry>() {
                                $field = e.get_text().to_string();
                            }
                        }
                    }; }
                    read_entry_str!("gig_name",       gig.name);
                    read_entry!("gig_date",        gig.date);
                    read_entry!("gig_start_time",  gig.start_time);
                    read_entry!("gig_end_time",    gig.end_time);
                    read_entry!("gig_location",    gig.location);
                    read_entry!("gig_spotify_url", gig.spotify_playlist_url);
                    if let Some(w) = find_widget(&gig_workspace2, "gig_notes") {
                        if let Ok(tv) = w.downcast::<gtk::TextView>() {
                            if let Some(buf) = tv.get_buffer() {
                                gig.notes = buf.get_text(
                                    &buf.get_start_iter(),
                                    &buf.get_end_iter(),
                                    false,
                                ).map(|s| s.to_string()).unwrap_or_default();
                            }
                        }
                    }
                    store.save();
                    // Flash "Saved" indicator
                    if let Some(w) = find_widget(&gig_workspace2, "gig_saved_lbl") {
                        if let Ok(lbl) = w.downcast::<gtk::Label>() {
                            lbl.set_text("✓ Saved");
                            let lbl_c = lbl.clone();
                            glib::timeout_add_local(2000, move || {
                                lbl_c.set_text("");
                                glib::Continue(false)
                            });
                        }
                    }
                }
            });

            for name in &["gig_name", "gig_date", "gig_start_time", "gig_end_time", "gig_location", "gig_spotify_url"] {
                if let Some(w) = find_widget(&gig_workspace, name) {
                    if let Ok(e) = w.downcast::<gtk::Entry>() {
                        let save = save.clone();
                        e.connect_changed(move |_| save());
                    }
                }
            }
            if let Some(w) = find_widget(&gig_workspace, "gig_notes") {
                if let Ok(tv) = w.downcast::<gtk::TextView>() {
                    if let Some(buf) = tv.get_buffer() {
                        let save = save.clone();
                        buf.connect_changed(move |_| save());
                    }
                }
            }
        }

        // ── gig workspace: Run Match button ───────────────────────────────────
        {
            let gig_workspace2 = gig_workspace.clone();
            let config2        = config.clone();
            let library2       = library.clone();
            let window2        = window.clone();

            if let Some(w) = find_widget(&gig_workspace, "gig_run_match") {
                w.downcast::<gtk::Button>().unwrap()
                    .connect_clicked(move |_| {
                        let wname = gig_workspace2.get_widget_name().to_string();
                        let gig_id = match wname.strip_prefix("gig_workspace:") {
                            Some(id) if !id.is_empty() => id.to_string(),
                            _ => return,
                        };
                        let store = crate::gig::GigStore::load();
                        let gig = match store.gigs.iter().find(|g| g.id == gig_id) {
                            Some(g) => g.clone(),
                            None    => return,
                        };
                        let spotify_url = match &gig.spotify_playlist_url {
                            Some(u) if !u.is_empty() => u.clone(),
                            _ => {
                                set_match_status(&gig_workspace2, "Add a Spotify URL in Brief first");
                                return;
                            }
                        };
                        // Refresh token if available before making the API call
                        let token = {
                            let refresh_token = config2.borrow().spotify_refresh_token.clone();
                            if let Some(rt) = refresh_token {
                                match crate::spotify::refresh(&rt) {
                                    Ok((new_access, new_refresh)) => {
                                        let mut cfg = config2.borrow_mut();
                                        cfg.spotify_access_token  = Some(new_access.clone());
                                        if let Some(nr) = new_refresh {
                                            cfg.spotify_refresh_token = Some(nr);
                                        }
                                        cfg.save();
                                        new_access
                                    }
                                    Err(_) => {
                                        // Fall back to stored token
                                        match config2.borrow().spotify_access_token.clone() {
                                            Some(t) => t,
                                            None => {
                                                set_match_status(&gig_workspace2, "Spotify not connected — connect via Settings…");
                                                return;
                                            }
                                        }
                                    }
                                }
                            } else {
                                match config2.borrow().spotify_access_token.clone() {
                                    Some(t) => t,
                                    None => {
                                        set_match_status(&gig_workspace2, "Spotify not connected — connect via Settings…");
                                        return;
                                    }
                                }
                            }
                        };
                        let lib_opt = library2.borrow();
                        let lib = match lib_opt.as_ref() {
                            Some(l) => l,
                            None => {
                                set_match_status(&gig_workspace2, "Open a library first");
                                return;
                            }
                        };
                        set_match_status(&gig_workspace2, "Running match…");
                        match crate::spotify::fetch_playlist(&token, &spotify_url) {
                            Err(e) => {
                                set_match_status(&gig_workspace2, &format!("Spotify fetch failed: {e}"));
                            }
                            Ok(spotify_tracks) => {
                                let all_tracks = lib.tracks().unwrap_or_default();
                                let results    = crate::matcher::match_tracks(&spotify_tracks, &all_tracks);
                                populate_match_results(&gig_workspace2, &gig_id, &results, &window2);
                            }
                        }
                    });
            }
        }

        // ── gig workspace: Create Playlist button ─────────────────────────────
        {
            let gig_workspace2 = gig_workspace.clone();
            let library2       = library.clone();
            let window2        = window.clone();

            if let Some(w) = find_widget(&gig_workspace, "gig_create_btn") {
                w.downcast::<gtk::Button>().unwrap()
                    .connect_clicked(move |_| {
                        let wname = gig_workspace2.get_widget_name().to_string();
                        let gig_id = match wname.strip_prefix("gig_workspace:") {
                            Some(id) if !id.is_empty() => id.to_string(),
                            _ => return,
                        };
                        let mut store = crate::gig::GigStore::load();
                        let gig = match store.gigs.iter().find(|g| g.id == gig_id).cloned() {
                            Some(g) => g,
                            None    => return,
                        };
                        let contact = match store.contact_for_gig(&gig).cloned() {
                            Some(c) => c,
                            None    => return,
                        };
                        if gig.accepted_track_ids.is_empty() {
                            let d = gtk::MessageDialog::new(
                                Some(&window2),
                                gtk::DialogFlags::MODAL,
                                gtk::MessageType::Warning,
                                gtk::ButtonsType::Ok,
                                "No accepted tracks — run Match first and accept tracks.",
                            );
                            d.run();
                            d.close();
                            return;
                        }
                        let lib_opt = library2.borrow();
                        let lib = match lib_opt.as_ref() {
                            Some(l) => l,
                            None    => return,
                        };
                        let folder_name = contact.customer_type.playlist_folder();
                        let result = lib.find_or_create_folder(folder_name)
                            .and_then(|type_folder| lib.find_or_create_subfolder(&contact.name, type_folder))
                            .and_then(|contact_folder| lib.find_or_create_subfolder(&gig.name, contact_folder))
                            .and_then(|gig_folder| {
                                let pl_name = if gig.name.is_empty() { "Set".to_string() } else { gig.name.clone() };
                                lib.create_playlist(&pl_name, Some(gig_folder))
                            })
                            .and_then(|pl_id| {
                                lib.add_tracks_to_playlist(pl_id, &gig.accepted_track_ids)?;
                                Ok(pl_id)
                            });
                        match result {
                            Ok(_) => {
                                if let Some(gig_mut) = store.gigs.iter_mut().find(|g| g.id == gig_id) {
                                    // rekordbox_folder_id set by find_or_create_subfolder above;
                                    // mark gig as having a playlist created
                                    gig_mut.rekordbox_folder_id = Some(-1); // placeholder
                                    store.save();
                                }
                                if let Some(w) = find_widget(&gig_workspace2, "gig_create_status") {
                                    if let Ok(lbl) = w.downcast::<gtk::Label>() {
                                        lbl.set_text("Playlist created ✓");
                                    }
                                }
                            }
                            Err(e) => {
                                let d = gtk::MessageDialog::new(
                                    Some(&window2),
                                    gtk::DialogFlags::MODAL,
                                    gtk::MessageType::Error,
                                    gtk::ButtonsType::Ok,
                                    &format!("Failed to create playlist: {e}"),
                                );
                                d.run();
                                d.close();
                            }
                        }
                    });
            }
        }

        // ── contact view: "Add New Gig" button ───────────────────────────────
        {
            let contact_view2     = contact_view.clone();
            let gig_workspace2    = gig_workspace.clone();
            let right_stack2      = right_stack.clone();
            let gig_list_box2     = gig_list_box.clone();
            let expanded_contacts2 = expanded_contacts.clone();
            let library2          = library.clone();

            if let Some(w) = find_widget(&contact_view, "contact_add_gig_btn") {
                w.downcast::<gtk::Button>().unwrap()
                    .connect_clicked(move |_| {
                        let view_name = contact_view2.get_widget_name().to_string();
                        let contact_id = match view_name.strip_prefix("contact_view:") {
                            Some(id) if !id.is_empty() => id.to_string(),
                            _ => return,
                        };
                        let gig = crate::gig::Gig {
                            id:                   uuid::Uuid::new_v4().to_string(),
                            contact_id:           contact_id.clone(),
                            name:                 String::new(),
                            date:                 None,
                            start_time:           None,
                            end_time:             None,
                            location:             None,
                            tags:                 Vec::new(),
                            notes:                String::new(),
                            spotify_playlist_url: None,
                            accepted_track_ids:   Vec::new(),
                            rekordbox_folder_id:  None,
                        };
                        let mut store = crate::gig::GigStore::load();
                        store.gigs.push(gig.clone());
                        store.save();
                        // Refresh sidebar
                        let playlists = library2.borrow().as_ref()
                            .and_then(|lib| lib.playlists().ok())
                            .unwrap_or_default();
                        populate_gig_sidebar_from_library(
                            &gig_list_box2,
                            &store,
                            &playlists,
                            &expanded_contacts2.borrow(),
                        );
                        // Open gig workspace
                        if let Some(contact) = store.contacts.iter().find(|c| c.id == contact_id) {
                            load_gig_into_workspace(&gig_workspace2, &gig, contact);
                            right_stack2.set_visible_child_name("gig");
                        }
                    });
            }
        }

        // ── contact view: delete contact button ──────────────────────────────
        {
            let contact_view2      = contact_view.clone();
            let right_stack2       = right_stack.clone();
            let gig_list_box2      = gig_list_box.clone();
            let expanded_contacts2 = expanded_contacts.clone();
            let library2           = library.clone();
            let window2            = window.clone();

            if let Some(w) = find_widget(&contact_view, "contact_delete_btn") {
                w.downcast::<gtk::Button>().unwrap()
                    .connect_clicked(move |_| {
                        let view_name  = contact_view2.get_widget_name().to_string();
                        let contact_id = match view_name.strip_prefix("contact_view:") {
                            Some(id) if !id.is_empty() => id.to_string(),
                            _ => return,
                        };
                        let store = crate::gig::GigStore::load();
                        let contact = match store.contacts.iter().find(|c| c.id == contact_id) {
                            Some(c) => c.clone(),
                            None    => return,
                        };
                        let gigs = store.gigs_for_contact(&contact_id);

                        // Check whether there's anything to warn about
                        let playlists = library2.borrow().as_ref()
                            .and_then(|lib| lib.playlists().ok())
                            .unwrap_or_default();
                        let has_rb_children = contact.rekordbox_folder_id.map_or(false, |cid| {
                            playlists.iter().any(|pl| pl.parent_id == Some(cid))
                        });
                        let needs_confirm = !gigs.is_empty() || has_rb_children;

                        if needs_confirm {
                            let mut msg = format!(
                                "Delete contact \"{}\"?\n\nThis will also remove:",
                                contact.name,
                            );
                            if !gigs.is_empty() {
                                msg.push_str(&format!("\n• {} gig(s) from the app", gigs.len()));
                            }
                            if has_rb_children {
                                msg.push_str("\n• The contact folder and all its playlists from Rekordbox");
                            }
                            msg.push_str("\n\nThis cannot be undone.");

                            let dialog = gtk::MessageDialog::new(
                                Some(&window2),
                                gtk::DialogFlags::MODAL,
                                gtk::MessageType::Warning,
                                gtk::ButtonsType::None,
                                &msg,
                            );
                            dialog.add_button("Cancel", gtk::ResponseType::Cancel);
                            dialog.add_button("Delete", gtk::ResponseType::Accept);
                            let response = dialog.run();
                            dialog.close();
                            if response != gtk::ResponseType::Accept {
                                return;
                            }
                        }

                        // Delete from Rekordbox
                        if let Some(folder_id) = contact.rekordbox_folder_id {
                            if let Some(lib) = library2.borrow().as_ref() {
                                let _ = lib.delete_subtree(folder_id);
                            }
                        }

                        // Delete from gigs.json
                        let mut store = crate::gig::GigStore::load();
                        store.gigs.retain(|g| g.contact_id != contact_id);
                        store.contacts.retain(|c| c.id != contact_id);
                        store.save();

                        // Refresh sidebar
                        expanded_contacts2.borrow_mut().remove(&contact_id);
                        populate_contacts_and_gigs(&gig_list_box2, &store, &expanded_contacts2.borrow());

                        right_stack2.set_visible_child_name("tracks");
                    });
            }
        }

        // ── contact view: gig list selection ─────────────────────────────────
        {
            let gig_workspace2 = gig_workspace.clone();
            let right_stack2   = right_stack.clone();

            if let Some(w) = find_widget(&contact_view, "contact_gig_list") {
                if let Ok(lb) = w.downcast::<gtk::ListBox>() {
                    lb.connect_row_selected(move |_, row| {
                        if let Some(row) = row {
                            let name = row.get_widget_name().to_string();
                            if let Some(gig_id) = name.strip_prefix("gig:") {
                                let store = crate::gig::GigStore::load();
                                if let Some(gig) = store.gigs.iter().find(|g| g.id == gig_id) {
                                    if let Some(contact) = store.contact_for_gig(gig) {
                                        load_gig_into_workspace(&gig_workspace2, gig, contact);
                                        right_stack2.set_visible_child_name("gig");
                                    }
                                }
                            }
                        }
                    });
                }
            }
        }

        // ── layout ───────────────────────────────────────────────────────────
        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.pack1(&sidebar_scroll, false, false);
        paned.pack2(&right_stack, true, true);
        paned.set_position(240);

        container.pack_start(&topbar,     false, false, 0);
        container.pack_start(&filter_bar, false, false, 0);
        container.pack_start(&paned,      true,  true,  0);

        // ── helper: build TrackFilter from current UI state ───────────────────
        let make_filter = {
            let bpm_min_spin2  = bpm_min_spin.clone();
            let bpm_max_spin2  = bpm_max_spin.clone();
            let key_combo2     = key_combo.clone();
            let genre_combo2   = genre_combo.clone();
            let rating_combo2  = rating_combo.clone();
            let harmonic_btn2  = harmonic_btn.clone();
            let harmonic_key2  = harmonic_key.clone();
            Rc::new(move || -> TrackFilter {
                let bpm_min_v = bpm_min_spin2.get_value() as f32;
                let bpm_max_v = bpm_max_spin2.get_value() as f32;

                // Key filter: harmonic mode overrides the key combo
                let key_val: Option<String> = if harmonic_btn2.get_active() {
                    // In harmonic mode we can't set a single key; caller handles it separately
                    let k = harmonic_key2.borrow().clone();
                    k
                } else {
                    let txt = key_combo2.get_active_text()
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                    if txt == "Any" || txt.is_empty() { None } else { Some(txt) }
                };

                let genre_val: Option<String> = {
                    let txt = genre_combo2.get_active_text()
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                    if txt == "Any" || txt.is_empty() { None } else { Some(txt) }
                };

                let min_rating: Option<i32> = match rating_combo2.get_active() {
                    Some(0) | None => None,
                    Some(n) => Some(n as i32),
                };

                TrackFilter {
                    bpm_min:    if bpm_min_v > 0.0 { Some(bpm_min_v) } else { None },
                    bpm_max:    if bpm_max_v > 0.0 { Some(bpm_max_v) } else { None },
                    key:        key_val,
                    genre:      genre_val,
                    min_rating,
                }
            })
        };

        // ── shared reload-tracks logic ────────────────────────────────────────
        let do_reload_tracks = {
            let library              = library.clone();
            let track_store2         = track_store.clone();
            let status_lbl2          = status_lbl.clone();
            let current_playlist_id2 = current_playlist_id.clone();
            let make_filter2         = make_filter.clone();
            Rc::new(move || {
                if let Some(lib) = library.borrow().as_ref() {
                    let f = make_filter2();
                    let is_all_active = f.bpm_min.is_none()
                        && f.bpm_max.is_none()
                        && f.key.is_none()
                        && f.genre.is_none()
                        && f.min_rating.is_none();

                    let result = match *current_playlist_id2.borrow() {
                        None => {
                            if is_all_active {
                                lib.tracks()
                            } else {
                                lib.filter_tracks(&f)
                            }
                        }
                        Some(pid) => {
                            if is_all_active {
                                lib.playlist_tracks(pid)
                            } else {
                                lib.filter_playlist_tracks(pid, &f)
                            }
                        }
                    };
                    if let Ok(tracks) = result {
                        let n = tracks.len();
                        browser_populate_tracks(&track_store2, &tracks);
                        status_lbl2.set_text(&format!("{} tracks", n));
                    }
                }
            })
        };

        // ── shared open-library logic ─────────────────────────────────────────
        let do_open_library = {
            let library              = library.clone();
            let pl_store2            = pl_store.clone();
            let hist_store2          = hist_store.clone();
            let track_store2         = track_store.clone();
            let status_lbl2          = status_lbl.clone();
            let config2              = config.clone();
            let window2              = window.clone();
            let on_track_end2        = on_track_end.clone();
            let key_combo2           = key_combo.clone();
            let genre_combo2         = genre_combo.clone();
            let pl_view2             = pl_view.clone();
            let gig_list_box2          = gig_list_box.clone();
            let expanded_contacts2     = expanded_contacts.clone();

            Rc::new(move |path_str: &str| {
                match Library::open(path_str) {
                    Ok(lib) => {
                        // Populate key combo
                        if let Ok(keys) = lib.all_keys() {
                            key_combo2.remove_all();
                            key_combo2.append_text("Any");
                            for k in &keys {
                                key_combo2.append_text(k);
                            }
                            key_combo2.set_active(Some(0));
                        }
                        // Populate genre combo
                        if let Ok(genres) = lib.all_genres() {
                            genre_combo2.remove_all();
                            genre_combo2.append_text("Any");
                            for g in &genres {
                                genre_combo2.append_text(g);
                            }
                            genre_combo2.set_active(Some(0));
                        }

                        let lists    = lib.playlists().unwrap_or_default();
                        let sessions = lib.history_sessions().unwrap_or_default();
                        browser_populate_playlists(&pl_store2, &lists);
                        browser_populate_history(&hist_store2, &sessions);
                        pl_view2.collapse_all();
                        populate_gig_sidebar_from_library(
                            &gig_list_box2,
                            &crate::gig::GigStore::load(),
                            &lists,
                            &expanded_contacts2.borrow(),
                        );

                        if let Ok(tracks) = lib.tracks() {
                            browser_populate_tracks(&track_store2, &tracks);
                            status_lbl2.set_text(&format!("{} tracks", tracks.len()));
                        }
                        config2.borrow_mut().db_path = Some(path_str.to_string());
                        config2.borrow().save();

                        // Wire on_track_end callback now that library is available
                        let lib_rc: Rc<RefCell<Option<Library>>> = {
                            // We need a separate reference – we'll just use the outer library Rc
                            // which we'll update below, then use a weak-ish pattern via a new Rc
                            Rc::new(RefCell::new(None))
                        };
                        // We'll set this after inserting into the outer library below
                        // (The callback will be set at the bottom of open-library)

                        *library.borrow_mut() = Some(lib);

                        // Now set the on_track_end callback to use the library
                        let lib_ref = library.clone();
                        *on_track_end2.borrow_mut() = Some(Rc::new(move |id: i64| {
                            if let Some(lib) = lib_ref.borrow().as_ref() {
                                let _ = lib.increment_play_count(id);
                            }
                        }));
                    }
                    Err(e) => {
                        let d = gtk::MessageDialog::new(
                            Some(&window2),
                            gtk::DialogFlags::MODAL,
                            gtk::MessageType::Error,
                            gtk::ButtonsType::Ok,
                            &format!("Could not open library:\n{}", e),
                        );
                        d.run();
                        d.close();
                    }
                }
            })
        };

        // ── reload library button ─────────────────────────────────────────────
        {
            let do_open = do_open_library.clone();
            let config  = config.clone();

            reload_btn.connect_clicked(move |_| {
                let path = config.borrow().db_path.clone();
                if let Some(path) = path {
                    do_open(&path);
                }
            });
        }

        // ── open library menu item ────────────────────────────────────────────
        {
            let do_open = do_open_library.clone();
            let window  = window.clone();

            open_btn.connect_activate(move |_| {
                let dialog = gtk::FileChooserDialog::new(
                    Some("Open Rekordbox Database"),
                    Some(&window),
                    gtk::FileChooserAction::Open,
                );
                let filter = gtk::FileFilter::new();
                filter.set_name(Some("Rekordbox database (*.db)"));
                filter.add_pattern("*.db");
                dialog.add_filter(&filter);
                dialog.add_button("Cancel", gtk::ResponseType::Cancel);
                dialog.add_button("Open",   gtk::ResponseType::Accept);

                let response = dialog.run();
                dialog.close();

                if response != gtk::ResponseType::Accept {
                    return;
                }
                if let Some(path) = dialog.get_filename() {
                    if let Some(s) = path.to_str() {
                        do_open(s);
                    }
                }
            });
        }

        // ── settings menu item ────────────────────────────────────────────────
        {
            let config = config.clone();
            let window = window.clone();

            settings_btn.connect_activate(move |_| {
                show_settings_dialog(&window, &config);
            });
        }

        // ── gig list: populate from store ─────────────────────────────────────
        {
            let gig_list_box = gig_list_box.clone();
            populate_contacts_and_gigs(
                &gig_list_box,
                &crate::gig::GigStore::load(),
                &expanded_contacts.borrow(),
            );
        }

        // ── new contact button ────────────────────────────────────────────────
        {
            let gig_list_box       = gig_list_box.clone();
            let right_stack        = right_stack.clone();
            let contact_view2      = contact_view.clone();
            let expanded_contacts2 = expanded_contacts.clone();

            new_gig_btn.connect_clicked(move |_| {
                let contact = crate::gig::Contact {
                    id:                  uuid::Uuid::new_v4().to_string(),
                    name:                String::new(),
                    customer_type:       crate::gig::CustomerType::Private,
                    notes:               String::new(),
                    rekordbox_folder_id: None,
                };
                let mut store = crate::gig::GigStore::load();
                store.contacts.push(contact.clone());
                store.save();
                // Show the contact row in the sidebar (collapsed)
                populate_contacts_and_gigs(&gig_list_box, &store, &expanded_contacts2.borrow());
                // Select the contact row so the user can see it
                let row_name = format!("contact:{}", contact.id);
                for child in gig_list_box.get_children() {
                    if let Ok(row) = child.downcast::<gtk::ListBoxRow>() {
                        if row.get_widget_name() == row_name {
                            gig_list_box.select_row(Some(&row));
                            break;
                        }
                    }
                }
                // Open the contact view so the user fills in the details
                load_contact_into_view(&contact_view2, &contact, &[], &[]);
                right_stack.set_visible_child_name("contact");
            });
        }

        // ── gig list selection ────────────────────────────────────────────────
        {
            let right_stack          = right_stack.clone();
            let gig_workspace        = gig_workspace.clone();
            let contact_view2        = contact_view.clone();
            let library              = library.clone();
            let track_store2         = track_store.clone();
            let status_lbl2          = status_lbl.clone();
            let current_playlist_id2 = current_playlist_id.clone();
            let gig_list_box2        = gig_list_box.clone();
            let expanded_contacts2   = expanded_contacts.clone();

            gig_list_box.connect_row_selected(move |lb, row| {
                if let Some(row) = row {
                    let name = row.get_widget_name().to_string();
                    if let Some(contact_id) = name.strip_prefix("contact:") {
                        // Toggle expand/collapse for this contact
                        let mut expanded = expanded_contacts2.borrow_mut();
                        if expanded.contains(contact_id) {
                            expanded.remove(contact_id);
                        } else {
                            expanded.insert(contact_id.to_string());
                        }
                        drop(expanded);
                        lb.unselect_all();
                        let store = crate::gig::GigStore::load();
                        let playlists = library.borrow().as_ref()
                            .and_then(|lib| lib.playlists().ok())
                            .unwrap_or_default();
                        populate_gig_sidebar_from_library(
                            &gig_list_box2,
                            &store,
                            &playlists,
                            &expanded_contacts2.borrow(),
                        );
                        // Open contact view
                        if let Some(contact) = store.contacts.iter().find(|c| c.id == contact_id) {
                            let gigs = store.gigs_for_contact(contact_id);
                            load_contact_into_view(&contact_view2, contact, &gigs, &playlists);
                            right_stack.set_visible_child_name("contact");
                        }
                    } else if let Some(gig_id) = name.strip_prefix("gig:") {
                        // Gig event folder → show gig workspace
                        let store = crate::gig::GigStore::load();
                        if let Some(gig) = store.gigs.iter().find(|g| g.id == gig_id) {
                            if let Some(contact) = store.contact_for_gig(gig) {
                                load_gig_into_workspace(&gig_workspace, gig, contact);
                                right_stack.set_visible_child_name("gig");
                            }
                        }
                    } else if let Some(id_str) = name.strip_prefix("pl:")
                        .or_else(|| name.strip_prefix("pool:"))
                    {
                        // Set playlist or pool → show tracks
                        if let Ok(pid) = id_str.parse::<i64>() {
                            if let Some(lib) = library.borrow().as_ref() {
                                *current_playlist_id2.borrow_mut() = Some(pid);
                                if let Ok(tracks) = lib.playlist_tracks(pid) {
                                    let n = tracks.len();
                                    browser_populate_tracks(&track_store2, &tracks);
                                    status_lbl2.set_text(&format!("{} tracks", n));
                                    right_stack.set_visible_child_name("tracks");
                                }
                            }
                        }
                    }
                }
            });
        }

        // ── filter bar callbacks ──────────────────────────────────────────────
        {
            let reload = do_reload_tracks.clone();
            bpm_min_spin.connect_value_changed(move |_| reload());
        }
        {
            let reload = do_reload_tracks.clone();
            bpm_max_spin.connect_value_changed(move |_| reload());
        }
        {
            let reload = do_reload_tracks.clone();
            key_combo.connect_changed(move |_| reload());
        }
        {
            let reload = do_reload_tracks.clone();
            genre_combo.connect_changed(move |_| reload());
        }
        {
            let reload = do_reload_tracks.clone();
            rating_combo.connect_changed(move |_| reload());
        }
        {
            let reload            = do_reload_tracks.clone();
            let harmonic_key2     = harmonic_key.clone();
            let harmonic_key_lbl2 = harmonic_key_lbl.clone();
            harmonic_btn.connect_toggled(move |btn| {
                if !btn.get_active() {
                    *harmonic_key2.borrow_mut() = None;
                    harmonic_key_lbl2.set_text("");
                }
                reload();
            });
        }
        {
            let reload          = do_reload_tracks.clone();
            let bpm_min_spin2   = bpm_min_spin.clone();
            let bpm_max_spin2   = bpm_max_spin.clone();
            let key_combo2      = key_combo.clone();
            let genre_combo2    = genre_combo.clone();
            let rating_combo2   = rating_combo.clone();
            let harmonic_btn2   = harmonic_btn.clone();
            let harmonic_key2   = harmonic_key.clone();
            let harmonic_key_lbl2 = harmonic_key_lbl.clone();
            clear_btn.connect_clicked(move |_| {
                bpm_min_spin2.set_value(0.0);
                bpm_max_spin2.set_value(0.0);
                key_combo2.set_active(Some(0));
                genre_combo2.set_active(Some(0));
                rating_combo2.set_active(Some(0));
                if harmonic_btn2.get_active() {
                    harmonic_btn2.set_active(false);
                }
                *harmonic_key2.borrow_mut() = None;
                harmonic_key_lbl2.set_text("");
                reload();
            });
        }

        // ── playlist right-click context menu ────────────────────────────────
        {
            let library    = library.clone();
            let pl_store2  = pl_store.clone();
            let hist_store2 = hist_store.clone();
            let pl_view_rc = pl_view.clone();
            let pl_view2   = pl_view.clone();
            let window     = window.clone();

            pl_view.connect_button_press_event(move |view, event| {
                if event.get_button() != 3 {
                    return gtk::Inhibit(false);
                }
                if library.borrow().is_none() {
                    return gtk::Inhibit(false);
                }

                let (clicked_id, clicked_is_folder): (Option<i64>, bool) = {
                    let (x, y) = event.get_position();
                    let result = view.get_path_at_pos(x as i32, y as i32)
                        .and_then(|(path, _, _, _)| path)
                        .and_then(|path| {
                            let model = view.get_model()?;
                            let iter  = model.get_iter(&path)?;
                            let id_val: String = model
                                .get_value(&iter, P_ID as i32)
                                .get::<String>()
                                .ok()
                                .flatten()
                                .unwrap_or_default();
                            let attr_val: String = model
                                .get_value(&iter, P_ATTR as i32)
                                .get::<String>()
                                .ok()
                                .flatten()
                                .unwrap_or_default();
                            if id_val == "all" || id_val == "history_header" || attr_val == "h" {
                                None
                            } else {
                                id_val.parse::<i64>().ok().map(|id| (id, attr_val == "1"))
                            }
                        });
                    match result {
                        Some((id, is_folder)) => (Some(id), is_folder),
                        None => (None, false),
                    }
                };

                let menu = gtk::Menu::new();

                // ── New Playlist ──
                let new_item = gtk::MenuItem::with_label("New Playlist…");
                {
                    let library     = library.clone();
                    let pl_store3   = pl_store2.clone();
                    let hist_store3 = hist_store2.clone();
                    let pl_view3    = pl_view_rc.clone();
                    let window      = window.clone();
                    new_item.connect_activate(move |_| {
                        let dialog = gtk::Dialog::new();
                        dialog.set_title("New Playlist");
                        dialog.set_transient_for(Some(&window));
                        dialog.set_modal(true);
                        dialog.set_default_size(360, -1);
                        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
                        dialog.add_button("Create", gtk::ResponseType::Accept);
                        dialog.set_default_response(gtk::ResponseType::Accept);

                        let content = dialog.get_content_area();
                        content.set_border_width(12);
                        content.set_spacing(8);
                        content.pack_start(&gtk::Label::new(Some("Playlist name:")), false, false, 0);
                        let name_entry = gtk::Entry::new();
                        name_entry.set_activates_default(true);
                        content.pack_start(&name_entry, false, false, 0);
                        content.show_all();

                        let response = dialog.run();
                        let name = name_entry.get_text().to_string();
                        dialog.close();

                        if response != gtk::ResponseType::Accept || name.trim().is_empty() {
                            return;
                        }
                        let result = library.borrow().as_ref().unwrap().create_playlist(name.trim(), None);
                        match result {
                            Ok(_) => {
                                let lib = library.borrow();
                                let lib_ref = lib.as_ref().unwrap();
                                let lists = lib_ref.playlists().unwrap_or_default();
                                let sessions = lib_ref.history_sessions().unwrap_or_default();
                                browser_populate_playlists(&pl_store3, &lists);
                                browser_populate_history(&hist_store3, &sessions);
                                pl_view3.collapse_all();
                            }
                            Err(e) => {
                                let d = gtk::MessageDialog::new(Some(&window), gtk::DialogFlags::MODAL,
                                    gtk::MessageType::Error, gtk::ButtonsType::Ok,
                                    &format!("Failed to create playlist:\n{}", e));
                                d.run(); d.close();
                            }
                        }
                    });
                }
                menu.append(&new_item);

                // ── New Folder ──
                let new_folder_item = gtk::MenuItem::with_label("New Folder…");
                {
                    let library     = library.clone();
                    let pl_store3   = pl_store2.clone();
                    let hist_store3 = hist_store2.clone();
                    let pl_view3    = pl_view_rc.clone();
                    let window      = window.clone();
                    new_folder_item.connect_activate(move |_| {
                        let dialog = gtk::Dialog::new();
                        dialog.set_title("New Folder");
                        dialog.set_transient_for(Some(&window));
                        dialog.set_modal(true);
                        dialog.set_default_size(360, -1);
                        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
                        dialog.add_button("Create", gtk::ResponseType::Accept);
                        dialog.set_default_response(gtk::ResponseType::Accept);

                        let content = dialog.get_content_area();
                        content.set_border_width(12);
                        content.set_spacing(8);
                        content.pack_start(&gtk::Label::new(Some("Folder name:")), false, false, 0);
                        let name_entry = gtk::Entry::new();
                        name_entry.set_activates_default(true);
                        content.pack_start(&name_entry, false, false, 0);
                        content.show_all();

                        let response = dialog.run();
                        let name = name_entry.get_text().to_string();
                        dialog.close();

                        if response != gtk::ResponseType::Accept || name.trim().is_empty() {
                            return;
                        }
                        let result = library.borrow().as_ref().unwrap().create_folder(name.trim(), None);
                        match result {
                            Ok(_) => {
                                let lib = library.borrow();
                                let lib_ref = lib.as_ref().unwrap();
                                let lists = lib_ref.playlists().unwrap_or_default();
                                let sessions = lib_ref.history_sessions().unwrap_or_default();
                                browser_populate_playlists(&pl_store3, &lists);
                                browser_populate_history(&hist_store3, &sessions);
                                pl_view3.collapse_all();
                            }
                            Err(e) => {
                                let d = gtk::MessageDialog::new(Some(&window), gtk::DialogFlags::MODAL,
                                    gtk::MessageType::Error, gtk::ButtonsType::Ok,
                                    &format!("Failed to create folder:\n{}", e));
                                d.run(); d.close();
                            }
                        }
                    });
                }
                menu.append(&new_folder_item);

                // ── New Playlist in Folder ──
                if clicked_is_folder {
                    if let Some(folder_id) = clicked_id {
                        let new_in_folder_item = gtk::MenuItem::with_label("New Playlist in Folder…");
                        {
                            let library     = library.clone();
                            let pl_store3   = pl_store2.clone();
                            let hist_store3 = hist_store2.clone();
                            let pl_view3    = pl_view_rc.clone();
                            let window      = window.clone();
                            new_in_folder_item.connect_activate(move |_| {
                                let dialog = gtk::Dialog::new();
                                dialog.set_title("New Playlist in Folder");
                                dialog.set_transient_for(Some(&window));
                                dialog.set_modal(true);
                                dialog.set_default_size(360, -1);
                                dialog.add_button("Cancel", gtk::ResponseType::Cancel);
                                dialog.add_button("Create", gtk::ResponseType::Accept);
                                dialog.set_default_response(gtk::ResponseType::Accept);

                                let content = dialog.get_content_area();
                                content.set_border_width(12);
                                content.set_spacing(8);
                                content.pack_start(&gtk::Label::new(Some("Playlist name:")), false, false, 0);
                                let name_entry = gtk::Entry::new();
                                name_entry.set_activates_default(true);
                                content.pack_start(&name_entry, false, false, 0);
                                content.show_all();

                                let response = dialog.run();
                                let name = name_entry.get_text().to_string();
                                dialog.close();

                                if response != gtk::ResponseType::Accept || name.trim().is_empty() {
                                    return;
                                }
                                let result = library.borrow().as_ref().unwrap()
                                    .create_playlist(name.trim(), Some(folder_id));
                                match result {
                                    Ok(_) => {
                                        let lib = library.borrow();
                                        let lib_ref = lib.as_ref().unwrap();
                                        let lists = lib_ref.playlists().unwrap_or_default();
                                        let sessions = lib_ref.history_sessions().unwrap_or_default();
                                        browser_populate_playlists(&pl_store3, &lists);
                                        browser_populate_history(&hist_store3, &sessions);
                                        pl_view3.collapse_all();
                                    }
                                    Err(e) => {
                                        let d = gtk::MessageDialog::new(Some(&window), gtk::DialogFlags::MODAL,
                                            gtk::MessageType::Error, gtk::ButtonsType::Ok,
                                            &format!("Failed to create playlist:\n{}", e));
                                        d.run(); d.close();
                                    }
                                }
                            });
                        }
                        menu.append(&new_in_folder_item);
                    }
                }

                // ── Delete Playlist ──
                if let Some(pid) = clicked_id {
                    if let Some((path, _, _, _)) = pl_view2.get_path_at_pos(
                        event.get_position().0 as i32,
                        event.get_position().1 as i32,
                    ) {
                        if let Some(p) = path {
                            pl_view2.get_selection().select_path(&p);
                        }
                    }

                    let del_item = gtk::MenuItem::with_label("Delete Playlist");
                    {
                        let library     = library.clone();
                        let pl_store3   = pl_store2.clone();
                        let hist_store3 = hist_store2.clone();
                        let pl_view3    = pl_view_rc.clone();
                        let window      = window.clone();
                        del_item.connect_activate(move |_| {
                            let confirm = gtk::MessageDialog::new(
                                Some(&window),
                                gtk::DialogFlags::MODAL,
                                gtk::MessageType::Question,
                                gtk::ButtonsType::OkCancel,
                                "Delete this playlist? The tracks themselves are not removed.",
                            );
                            let response = confirm.run();
                            confirm.close();
                            if response != gtk::ResponseType::Ok {
                                return;
                            }
                            let result = library.borrow().as_ref().unwrap().delete_playlist(pid);
                            match result {
                                Ok(_) => {
                                    let lib = library.borrow();
                                    let lib_ref = lib.as_ref().unwrap();
                                    let lists = lib_ref.playlists().unwrap_or_default();
                                    let sessions = lib_ref.history_sessions().unwrap_or_default();
                                    browser_populate_playlists(&pl_store3, &lists);
                                    browser_populate_history(&hist_store3, &sessions);
                                    pl_view3.collapse_all();
                                }
                                Err(e) => {
                                    let d = gtk::MessageDialog::new(Some(&window), gtk::DialogFlags::MODAL,
                                        gtk::MessageType::Error, gtk::ButtonsType::Ok,
                                        &format!("Failed to delete playlist:\n{}", e));
                                    d.run(); d.close();
                                }
                            }
                        });
                    }
                    menu.append(&del_item);
                }

                menu.show_all();
                menu.popup_at_pointer(Some(event));
                gtk::Inhibit(true)
            });
        }

        // ── playlist drag-and-drop ────────────────────────────────────────────
        {
            let dnd_targets = [gtk::TargetEntry::new(
                "text/plain",
                gtk::TargetFlags::empty(),
                0,
            )];

            pl_view.drag_source_set(
                gdk::ModifierType::BUTTON1_MASK,
                &dnd_targets,
                gdk::DragAction::MOVE,
            );
            pl_view.drag_dest_set(
                gtk::DestDefaults::ALL,
                &dnd_targets,
                gdk::DragAction::MOVE,
            );

            pl_view.connect_drag_data_get(move |view, _ctx, sel, _info, _time| {
                let selection = view.get_selection();
                if let Some((model, iter)) = selection.get_selected() {
                    let id: String = model
                        .get_value(&iter, P_ID as i32)
                        .get::<String>()
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    if id != "all" && id != "history_header" {
                        sel.set_text(&id);
                    }
                }
            });

            {
                let library     = library.clone();
                let pl_store2   = pl_store.clone();
                let hist_store2 = hist_store.clone();
                let pl_view2    = pl_view.clone();
                let pl_view_rc2 = pl_view.clone();

                pl_view.connect_drag_data_received(move |_view, ctx, x, y, sel, _info, time| {
                    let src_id_str = match sel.get_text() {
                        Some(s) => s.to_string(),
                        None    => { ctx.drag_finish(false, false, time); return; }
                    };
                    // Ignore history and special rows
                    if src_id_str.starts_with("h:") || src_id_str == "history_header" {
                        ctx.drag_finish(false, false, time);
                        return;
                    }
                    let src_id: i64 = match src_id_str.parse() {
                        Ok(v)  => v,
                        Err(_) => { ctx.drag_finish(false, false, time); return; }
                    };

                    let (dest_path, drop_pos) = match pl_view2.get_dest_row_at_pos(x, y) {
                        Some((Some(path), pos)) => (path, pos),
                        _ => { ctx.drag_finish(false, false, time); return; }
                    };

                    let model = pl_view2.get_model().unwrap();
                    let dest_iter = match model.get_iter(&dest_path) {
                        Some(i) => i,
                        None    => { ctx.drag_finish(false, false, time); return; }
                    };

                    let dest_id_str: String = model
                        .get_value(&dest_iter, P_ID as i32)
                        .get::<String>()
                        .ok()
                        .flatten()
                        .unwrap_or_default();

                    if dest_id_str == "all" || dest_id_str == src_id_str
                        || dest_id_str == "history_header" || dest_id_str.starts_with("h:")
                    {
                        ctx.drag_finish(false, false, time);
                        return;
                    }

                    let dest_attr: String = model
                        .get_value(&dest_iter, P_ATTR as i32)
                        .get::<String>()
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    let dest_is_folder = dest_attr == "1";

                    let is_reparent = dest_is_folder
                        && matches!(
                            drop_pos,
                            gtk::TreeViewDropPosition::IntoOrBefore
                            | gtk::TreeViewDropPosition::IntoOrAfter
                        );

                    if is_reparent {
                        let dest_id: i64 = match dest_id_str.parse() {
                            Ok(v)  => v,
                            Err(_) => { ctx.drag_finish(false, false, time); return; }
                        };
                        let result = library.borrow().as_ref().unwrap()
                            .move_playlist(src_id, Some(dest_id));
                        match result {
                            Ok(()) => {
                                let lib = library.borrow();
                                let lib_ref = lib.as_ref().unwrap();
                                let lists = lib_ref.playlists().unwrap_or_default();
                                let sessions = lib_ref.history_sessions().unwrap_or_default();
                                browser_populate_playlists(&pl_store2, &lists);
                                browser_populate_history(&hist_store2, &sessions);
                                pl_view_rc2.collapse_all();
                                ctx.drag_finish(true, false, time);
                            }
                            Err(_) => { ctx.drag_finish(false, false, time); }
                        }
                    } else {
                        let dest_id: i64 = match dest_id_str.parse() {
                            Ok(v)  => v,
                            Err(_) => { ctx.drag_finish(false, false, time); return; }
                        };

                        let mut ordered: Vec<i64> = Vec::new();
                        if let Some(iter) = model.get_iter_first() {
                            loop {
                                let id_s: String = model
                                    .get_value(&iter, P_ID as i32)
                                    .get::<String>()
                                    .ok()
                                    .flatten()
                                    .unwrap_or_default();
                                if id_s != "all" && id_s != "history_header" && !id_s.starts_with("h:") {
                                    if let Ok(id) = id_s.parse::<i64>() {
                                        if id != src_id {
                                            ordered.push(id);
                                        }
                                    }
                                }
                                if !model.iter_next(&iter) { break; }
                            }
                        }

                        let insert_pos = match ordered.iter().position(|&id| id == dest_id) {
                            Some(idx) => match drop_pos {
                                gtk::TreeViewDropPosition::Before
                                | gtk::TreeViewDropPosition::IntoOrBefore => idx,
                                _ => idx + 1,
                            },
                            None => { ctx.drag_finish(false, false, time); return; }
                        };
                        ordered.insert(insert_pos, src_id);

                        let result = library.borrow().as_ref().unwrap()
                            .reorder_playlists(&ordered);
                        match result {
                            Ok(()) => {
                                let lib = library.borrow();
                                let lib_ref = lib.as_ref().unwrap();
                                let lists = lib_ref.playlists().unwrap_or_default();
                                let sessions = lib_ref.history_sessions().unwrap_or_default();
                                browser_populate_playlists(&pl_store2, &lists);
                                browser_populate_history(&hist_store2, &sessions);
                                pl_view_rc2.collapse_all();
                                ctx.drag_finish(true, false, time);
                            }
                            Err(_) => { ctx.drag_finish(false, false, time); }
                        }
                    }
                });
            }
        }

        // ── auto-load saved library ───────────────────────────────────────────
        {
            let do_open    = do_open_library.clone();
            let saved_path = config.borrow().db_path.clone();

            if let Some(path) = saved_path {
                glib::idle_add_local(move || {
                    do_open(&path);
                    glib::Continue(false)
                });
            }
        }

        // ── playlist selection ────────────────────────────────────────────────
        {
            let library              = library.clone();
            let track_store2         = track_store.clone();
            let status_lbl2          = status_lbl.clone();
            let current_playlist_id2 = current_playlist_id.clone();
            let make_filter2         = make_filter.clone();
            let pl_store2            = pl_store.clone();
            let right_stack2         = right_stack.clone();
            let gig_list_box2        = gig_list_box.clone();

            pl_view.get_selection().connect_changed(move |sel| {
                let (model, iter) = match sel.get_selected() {
                    Some(pair) => pair,
                    None       => return,
                };
                let id: String = model
                    .get_value(&iter, P_ID as i32)
                    .get::<String>()
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let attr: String = model
                    .get_value(&iter, P_ATTR as i32)
                    .get::<String>()
                    .ok()
                    .flatten()
                    .unwrap_or_default();

                // Not selectable
                if id == "history_header" {
                    return;
                }

                if let Some(lib) = library.borrow().as_ref() {
                    let f = make_filter2();
                    let result = if id == "all" {
                        *current_playlist_id2.borrow_mut() = None;
                        lib.tracks()
                    } else if attr == "h" {
                        // History session — parse id "h:123"
                        let hid: i64 = id.trim_start_matches("h:").parse().unwrap_or(0);
                        *current_playlist_id2.borrow_mut() = None; // History isn't a playlist
                        lib.history_tracks(hid)
                    } else {
                        match id.parse::<i64>() {
                            Ok(pid) => {
                                *current_playlist_id2.borrow_mut() = Some(pid);
                                lib.playlist_tracks(pid)
                            }
                            Err(_)  => return,
                        }
                    };
                    if let Ok(tracks) = result {
                        let n = tracks.len();
                        browser_populate_tracks(&track_store2, &tracks);
                        status_lbl2.set_text(&format!("{} tracks", n));
                        // Switch back to track browser if gig workspace is open
                        right_stack2.set_visible_child_name("tracks");
                        gig_list_box2.unselect_all();
                    }
                }
            });
        }

        // ── history tab selection ─────────────────────────────────────────────
        {
            let library       = library.clone();
            let track_store2  = track_store.clone();
            let status_lbl2   = status_lbl.clone();
            let right_stack2  = right_stack.clone();
            let gig_list_box2 = gig_list_box.clone();

            hist_view.get_selection().connect_changed(move |sel| {
                let (model, iter) = match sel.get_selected() {
                    Some(pair) => pair,
                    None       => return,
                };
                let id: String = model
                    .get_value(&iter, P_ID as i32)
                    .get::<String>()
                    .ok()
                    .flatten()
                    .unwrap_or_default();

                if let Some(hid_str) = id.strip_prefix("h:") {
                    if let Ok(hid) = hid_str.parse::<i64>() {
                        if let Some(lib) = library.borrow().as_ref() {
                            if let Ok(tracks) = lib.history_tracks(hid) {
                                let n = tracks.len();
                                browser_populate_tracks(&track_store2, &tracks);
                                status_lbl2.set_text(&format!("{} tracks", n));
                                right_stack2.set_visible_child_name("tracks");
                                gig_list_box2.unselect_all();
                            }
                        }
                    }
                }
            });
        }

        // ── track selection: My Tags + Rating ────────────────────────────────
        {
            let library           = library.clone();
            let track_store2      = track_store.clone();
            let tags_label2       = tags_label.clone();
            let star_btns_rc: Vec<gtk::Button> = star_btns.clone();
            let clear_rating_btn2 = clear_rating_btn.clone();

            track_view.get_selection().connect_changed(move |sel| {
                if let Some((model, iter)) = sel.get_selected() {
                    let id_str: String = model
                        .get_value(&iter, T_TRACK_ID as i32)
                        .get::<String>().ok().flatten().unwrap_or_default();
                    let track_id: i64 = id_str.parse().unwrap_or(0);

                    if let Some(lib) = library.borrow().as_ref() {
                        if let Ok(tags) = lib.song_my_tags(track_id) {
                            if tags.is_empty() {
                                tags_label2.set_text("Tags: —");
                            } else {
                                tags_label2.set_text(&format!("Tags: {}", tags.join(", ")));
                            }
                        }
                    }

                    for (i, btn) in star_btns_rc.iter().enumerate() {
                        let rating_val   = (i + 1) as i32;
                        let library2     = library.clone();
                        let track_store3 = track_store2.clone();
                        let sel2         = sel.clone();
                        btn.connect_clicked(move |_| {
                            if let Some((_, si)) = sel2.get_selected() {
                                let tid_str: String = track_store3
                                    .get_value(&si, T_TRACK_ID as i32)
                                    .get::<String>().ok().flatten().unwrap_or_default();
                                let tid: i64 = tid_str.parse().unwrap_or(0);
                                if let Some(lib) = library2.borrow().as_ref() {
                                    let _ = lib.set_rating(tid, rating_val);
                                    track_store3.set_value(&si, T_RATING, &rating_stars(rating_val).to_value());
                                }
                            }
                        });
                    }
                    {
                        let library2     = library.clone();
                        let track_store3 = track_store2.clone();
                        let sel2         = sel.clone();
                        clear_rating_btn2.connect_clicked(move |_| {
                            if let Some((_, si)) = sel2.get_selected() {
                                let tid_str: String = track_store3
                                    .get_value(&si, T_TRACK_ID as i32)
                                    .get::<String>().ok().flatten().unwrap_or_default();
                                let tid: i64 = tid_str.parse().unwrap_or(0);
                                if let Some(lib) = library2.borrow().as_ref() {
                                    let _ = lib.set_rating(tid, 0);
                                    track_store3.set_value(&si, T_RATING, &"".to_value());
                                }
                            }
                        });
                    }
                } else {
                    tags_label.set_text("Tags: —");
                }
            });
        }

        // ── search ────────────────────────────────────────────────────────────
        {
            let library              = library.clone();
            let track_store2         = track_store.clone();
            let status_lbl2          = status_lbl.clone();
            let current_playlist_id2 = current_playlist_id.clone();

            search_entry.connect_changed(move |entry| {
                let text: String = entry.get_text().to_string();

                if let Some(lib) = library.borrow().as_ref() {
                    let result: rusqlite::Result<Vec<crate::rekordbox::Track>> =
                        if text.is_empty() {
                            match *current_playlist_id2.borrow() {
                                None      => lib.tracks(),
                                Some(pid) => lib.playlist_tracks(pid),
                            }
                        } else {
                            lib.search_tracks(&text)
                        };
                    if let Ok(tracks) = result {
                        browser_populate_tracks(&track_store2, &tracks);
                        status_lbl2.set_text(&format!("{} tracks", tracks.len()));
                    }
                }
            });
        }

        BrowserView { container }
    }
}

fn show_settings_dialog(window: &gtk::ApplicationWindow, config: &Rc<RefCell<Config>>) {
    let dialog = gtk::Dialog::new();
    dialog.set_title("Settings");
    dialog.set_transient_for(Some(window));
    dialog.set_modal(true);
    dialog.set_destroy_with_parent(true);
    dialog.set_default_size(560, 300);
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Save",   gtk::ResponseType::Accept);

    let content = dialog.get_content_area();
    content.set_spacing(6);
    content.set_border_width(12);

    let heading = gtk::Label::new(Some("Path Mappings"));
    heading.set_xalign(0.0);
    content.pack_start(&heading, false, false, 0);

    let hint = gtk::Label::new(Some(
        "Rewrite path prefixes stored in the database to match your local file system.\n\
         Example:  /Volumes/muzika  →  /run/media/jonas/muzika",
    ));
    hint.set_xalign(0.0);
    hint.set_line_wrap(true);
    content.pack_start(&hint, false, false, 0);

    let rows_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_min_content_height(120);
    scroll.add(&rows_box);
    content.pack_start(&scroll, true, true, 0);

    let pairs: Rc<RefCell<Vec<(gtk::Entry, gtk::Entry)>>> = Rc::new(RefCell::new(Vec::new()));

    let add_row = {
        let rows_box = rows_box.clone();
        let pairs    = pairs.clone();
        Rc::new(move |from: &str, to: &str| {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            let from_entry = gtk::Entry::new();
            from_entry.set_placeholder_text(Some("From prefix (e.g. /Volumes/muzika)"));
            from_entry.set_text(from);
            from_entry.set_hexpand(true);
            let arrow = gtk::Label::new(Some("→"));
            let to_entry = gtk::Entry::new();
            to_entry.set_placeholder_text(Some("To prefix (e.g. /run/media/jonas/muzika)"));
            to_entry.set_text(to);
            to_entry.set_hexpand(true);
            let del_btn = gtk::Button::with_label("✕");

            row.pack_start(&from_entry, true, true, 0);
            row.pack_start(&arrow,      false, false, 4);
            row.pack_start(&to_entry,   true, true, 0);
            row.pack_start(&del_btn,    false, false, 0);

            rows_box.pack_start(&row, false, false, 0);
            rows_box.show_all();

            let row_clone  = row.clone();
            let pairs_del  = pairs.clone();
            let fe = from_entry.clone();
            let te = to_entry.clone();
            del_btn.connect_clicked(move |_| {
                row_clone.hide();
                pairs_del.borrow_mut().retain(|(f, t)| {
                    f.as_ptr() != fe.as_ptr() || t.as_ptr() != te.as_ptr()
                });
            });

            pairs.borrow_mut().push((from_entry, to_entry));
        })
    };

    for m in &config.borrow().path_mappings {
        add_row(&m.from, &m.to);
    }

    let add_btn = gtk::Button::with_label("+ Add mapping");
    add_btn.set_halign(gtk::Align::Start);
    {
        let add_row = add_row.clone();
        add_btn.connect_clicked(move |_| add_row("", ""));
    }
    content.pack_start(&add_btn, false, false, 0);

    // ── Spotify section ───────────────────────────────────────────────────────
    content.pack_start(&gtk::Separator::new(gtk::Orientation::Horizontal), false, false, 4);

    let spotify_heading = gtk::Label::new(Some("Spotify"));
    spotify_heading.set_xalign(0.0);
    content.pack_start(&spotify_heading, false, false, 0);

    let spotify_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let connected   = config.borrow().spotify_access_token.is_some();
    let status_text = if connected { "✓ Connected" } else { "Not connected" };
    let spotify_status = gtk::Label::new(Some(status_text));
    spotify_status.set_xalign(0.0);
    spotify_status.set_hexpand(true);
    let connect_btn = gtk::Button::with_label("Connect with Spotify");
    spotify_row.pack_start(&spotify_status, true,  true,  0);
    spotify_row.pack_start(&connect_btn,    false, false, 0);
    content.pack_start(&spotify_row, false, false, 0);

    {
        let config         = config.clone();
        let spotify_status = spotify_status.clone();

        connect_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            spotify_status.set_text("Waiting for browser…");

            let (tx, rx) = std::sync::mpsc::channel::<Result<(String, String), String>>();
            std::thread::spawn(move || {
                let _ = tx.send(crate::spotify::authorize());
            });

            let config         = config.clone();
            let spotify_status = spotify_status.clone();
            let btn            = btn.clone();
            glib::timeout_add_local(200, move || {
                match rx.try_recv() {
                    Ok(Ok((access, refresh))) => {
                        config.borrow_mut().spotify_access_token  = Some(access);
                        config.borrow_mut().spotify_refresh_token = Some(refresh);
                        config.borrow().save();
                        spotify_status.set_text("✓ Connected");
                        btn.set_sensitive(true);
                        glib::Continue(false)
                    }
                    Ok(Err(e)) => {
                        spotify_status.set_text(&format!("Error: {e}"));
                        btn.set_sensitive(true);
                        glib::Continue(false)
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::Continue(true),
                    Err(_) => glib::Continue(false),
                }
            });
        });
    }

    content.show_all();

    let response = dialog.run();
    dialog.close();

    if response == gtk::ResponseType::Accept {
        let mappings: Vec<PathMapping> = pairs.borrow().iter()
            .map(|(f, t)| PathMapping {
                from: f.get_text().to_string(),
                to:   t.get_text().to_string(),
            })
            .filter(|m| !m.from.is_empty())
            .collect();

        config.borrow_mut().path_mappings = mappings;
        config.borrow().save();
    }
}

fn browser_populate_playlists(store: &gtk::TreeStore, playlists: &[Playlist]) {
    store.clear();
    store.insert_with_values(
        None, None,
        &[P_NAME, P_COUNT, P_ID, P_ATTR],
        &[&"★ All Tracks", &"", &"all", &"0"],
    );

    // IndexMap preserves insertion order, which matches the DB's ORDER BY Seq
    let mut children: IndexMap<Option<i64>, Vec<&Playlist>> = IndexMap::new();
    for pl in playlists {
        children.entry(pl.parent_id).or_default().push(pl);
    }

    fn insert_node(
        store: &gtk::TreeStore,
        children: &IndexMap<Option<i64>, Vec<&Playlist>>,
        parent_id: Option<i64>,
        parent_iter: Option<&gtk::TreeIter>,
    ) {
        if let Some(nodes) = children.get(&parent_id) {
            for pl in nodes {
                // Hide top-level gig output folders from the browsing tree
                if parent_id.is_none() && crate::gig::GIG_FOLDERS.contains(&pl.name.as_str()) {
                    continue;
                }
                let name = if pl.attribute == 1 {
                    format!("▸ {}", pl.name)
                } else {
                    pl.name.clone()
                };
                let count = if pl.attribute == 1 {
                    String::new()
                } else {
                    pl.track_count.to_string()
                };
                let iter = store.insert_with_values(
                    parent_iter, None,
                    &[P_NAME, P_COUNT, P_ID, P_ATTR],
                    &[&name.as_str(), &count.as_str(), &pl.id.to_string().as_str(), &pl.attribute.to_string().as_str()],
                );
                if pl.attribute == 1 {
                    insert_node(store, children, Some(pl.id), Some(&iter));
                }
            }
        }
    }

    insert_node(store, &children, None, None);
}

fn browser_populate_history(store: &gtk::ListStore, sessions: &[HistorySession]) {
    store.clear();
    for s in sessions {
        let id  = format!("h:{}", s.id);
        let cnt = s.track_count.to_string();
        store.insert_with_values(
            None,
            &[P_NAME, P_COUNT, P_ID, P_ATTR],
            &[&s.name.as_str(), &cnt.as_str(), &id.as_str(), &"h"],
        );
    }
}

fn browser_fmt_duration(secs: i32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn browser_populate_tracks(store: &gtk::ListStore, tracks: &[Track]) {
    store.clear();
    for t in tracks {
        let bpm       = t.bpm_display().map(|b| format!("{:.1}", b)).unwrap_or_default();
        let key       = t.key.as_deref().unwrap_or("").to_string();
        let artist    = t.artist.as_deref().unwrap_or("").to_string();
        let duration  = t.duration_secs.map(browser_fmt_duration).unwrap_or_default();
        let file_path = t.file_path.as_deref().unwrap_or("").to_string();
        let genre     = t.genre.as_deref().unwrap_or("").to_string();
        let rating    = rating_stars(t.rating.unwrap_or(0)).to_string();
        let label     = t.label.as_deref().unwrap_or("").to_string();
        let color_id  = t.color_id.as_deref().unwrap_or("").to_string();
        let track_id  = t.id.to_string();
        let bpm_raw   = t.bpm.unwrap_or(0).to_string();
        let dur_raw   = t.duration_secs.unwrap_or(0).to_string();
        store.insert_with_values(
            None,
            &[T_TITLE, T_ARTIST, T_BPM, T_KEY, T_DURATION,
              T_FILE_PATH, T_GENRE, T_RATING, T_LABEL, T_COLOR, T_TRACK_ID,
              T_BPM_RAW, T_DURATION_RAW],
            &[
                &t.title.as_str(),
                &artist.as_str(),
                &bpm.as_str(),
                &key.as_str(),
                &duration.as_str(),
                &file_path.as_str(),
                &genre.as_str(),
                &rating.as_str(),
                &label.as_str(),
                &color_id.as_str(),
                &track_id.as_str(),
                &bpm_raw.as_str(),
                &dur_raw.as_str(),
            ],
        );
    }
}

// ── Gig Prep dialog (legacy — replaced by inline workspace) ──────────────────

#[allow(dead_code)]
fn show_gig_prep_dialog(
    window:  &gtk::ApplicationWindow,
    config:  &Rc<RefCell<Config>>,
    library: &Rc<RefCell<Option<Library>>>,
) {
    let dialog = gtk::Dialog::new();
    dialog.set_title("Gig Prep");
    dialog.set_transient_for(Some(window));
    dialog.set_modal(true);
    dialog.set_destroy_with_parent(true);
    dialog.set_default_size(560, 520);
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Save",   gtk::ResponseType::Accept);

    let content = dialog.get_content_area();
    content.set_spacing(6);
    content.set_border_width(12);

    let grid = gtk::Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(8);

    macro_rules! lbl {
        ($text:expr) => {{
            let l = gtk::Label::new(Some($text));
            l.set_xalign(1.0);
            l
        }};
    }
    macro_rules! entry {
        ($placeholder:expr) => {{
            let e = gtk::Entry::new();
            e.set_placeholder_text(Some($placeholder));
            e.set_hexpand(true);
            e
        }};
    }

    // Row 0: Gig type
    let type_combo = gtk::ComboBoxText::new();
    type_combo.append(Some("corporate"), "Corporate");
    type_combo.append(Some("venue"),     "Venue");
    type_combo.append(Some("private"),   "Private");
    type_combo.set_active_id(Some("private"));
    grid.attach(&lbl!("Type"),    0, 0, 1, 1);
    grid.attach(&type_combo,      1, 0, 1, 1);

    // Row 1: Contact
    let contact_entry = entry!("Contact person or client name");
    grid.attach(&lbl!("Contact"), 0, 1, 1, 1);
    grid.attach(&contact_entry,   1, 1, 1, 1);

    // Row 2: Event name
    let name_entry = entry!("Event name (e.g. Wedding, Kick-off 2026)");
    grid.attach(&lbl!("Name"),    0, 2, 1, 1);
    grid.attach(&name_entry,      1, 2, 1, 1);

    // Row 3: Date
    let date_entry = entry!("YYYY-MM-DD");
    grid.attach(&lbl!("Date"),    0, 3, 1, 1);
    grid.attach(&date_entry,      1, 3, 1, 1);

    // Row 4: Start / End time
    let time_box    = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let start_entry = entry!("HH:MM");
    start_entry.set_width_chars(6);
    start_entry.set_hexpand(false);
    let sep_lbl     = gtk::Label::new(Some("–"));
    let end_entry   = entry!("HH:MM");
    end_entry.set_width_chars(6);
    end_entry.set_hexpand(false);
    time_box.pack_start(&start_entry, false, false, 0);
    time_box.pack_start(&sep_lbl,     false, false, 0);
    time_box.pack_start(&end_entry,   false, false, 0);
    grid.attach(&lbl!("Time"),    0, 4, 1, 1);
    grid.attach(&time_box,        1, 4, 1, 1);

    // Row 5: Location
    let location_entry = entry!("Venue name or address");
    grid.attach(&lbl!("Location"),  0, 5, 1, 1);
    grid.attach(&location_entry,    1, 5, 1, 1);

    // Row 6: Spotify playlist URL
    let spotify_entry = entry!("https://open.spotify.com/playlist/…");
    grid.attach(&lbl!("Spotify"),   0, 6, 1, 1);
    grid.attach(&spotify_entry,     1, 6, 1, 1);

    // Row 7: Notes (multi-line)
    let notes_view   = gtk::TextView::new();
    notes_view.set_wrap_mode(gtk::WrapMode::Word);
    let notes_scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
    notes_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    notes_scroll.set_min_content_height(100);
    notes_scroll.add(&notes_view);
    notes_scroll.set_hexpand(true);
    let notes_lbl = lbl!("Notes");
    notes_lbl.set_valign(gtk::Align::Start);
    grid.attach(&notes_lbl,      0, 7, 1, 1);
    grid.attach(&notes_scroll,   1, 7, 1, 1);

    // Spotify connection status hint
    let spotify_hint = if config.borrow().spotify_access_token.is_some() {
        gtk::Label::new(Some("Spotify: ✓ Connected"))
    } else {
        gtk::Label::new(Some("Spotify: not connected — connect via Settings…"))
    };
    spotify_hint.set_xalign(0.0);

    content.pack_start(&grid,        true,  true,  0);
    content.pack_start(&spotify_hint, false, false, 0);
    content.show_all();

    let response = dialog.run();
    dialog.close();

    if response != gtk::ResponseType::Accept {
        return;
    }

    let customer_type = match type_combo.get_active_id().as_deref() {
        Some("corporate") => CustomerType::Corporate,
        Some("venue")     => CustomerType::Venue,
        _                 => CustomerType::Private,
    };

    let notes_buf  = notes_view.get_buffer().unwrap();
    let notes_text = notes_buf.get_text(
        &notes_buf.get_start_iter(),
        &notes_buf.get_end_iter(),
        false,
    ).map(|s| s.to_string()).unwrap_or_default();

    let contact = Contact {
        id:                  uuid::Uuid::new_v4().to_string(),
        name:                contact_entry.get_text().to_string(),
        customer_type,
        notes:               String::new(),
        rekordbox_folder_id: None,
    };

    let spotify_url = {
        let url = spotify_entry.get_text().to_string();
        if url.is_empty() { None } else { Some(url) }
    };

    let mut gig = Gig {
        id:                   uuid::Uuid::new_v4().to_string(),
        contact_id:           contact.id.clone(),
        name:                 name_entry.get_text().to_string(),
        date:                 { let d = date_entry.get_text().to_string(); if d.is_empty() { None } else { Some(d) } },
        start_time:           { let t = start_entry.get_text().to_string(); if t.is_empty() { None } else { Some(t) } },
        end_time:             { let t = end_entry.get_text().to_string(); if t.is_empty() { None } else { Some(t) } },
        location:             { let l = location_entry.get_text().to_string(); if l.is_empty() { None } else { Some(l) } },
        tags:                 Vec::new(),
        notes:                notes_text,
        spotify_playlist_url: spotify_url.clone(),
        accepted_track_ids:   Vec::new(),
        rekordbox_folder_id:  None,
    };

    // If a Spotify URL was given and the library is open, run the match flow
    if let (Some(url), Some(lib)) = (spotify_url, library.borrow().as_ref()) {
        let access_token = config.borrow().spotify_access_token.clone();
        match access_token {
            None => {
                let d = gtk::MessageDialog::new(
                    Some(window),
                    gtk::DialogFlags::MODAL,
                    gtk::MessageType::Warning,
                    gtk::ButtonsType::Ok,
                    "Spotify not connected. Connect via Settings… first.",
                );
                d.run();
                d.close();
            }
            Some(token) => {
                match crate::spotify::fetch_playlist(&token, &url) {
                    Err(e) => {
                        let d = gtk::MessageDialog::new(
                            Some(window),
                            gtk::DialogFlags::MODAL,
                            gtk::MessageType::Error,
                            gtk::ButtonsType::Ok,
                            &format!("Spotify fetch failed: {e}"),
                        );
                        d.run();
                        d.close();
                    }
                    Ok(spotify_tracks) => {
                        let all_tracks = lib.tracks().unwrap_or_default();
                        let results    = crate::matcher::match_tracks(&spotify_tracks, &all_tracks);
                        let folder_id = lib.find_or_create_folder(contact.customer_type.playlist_folder()).ok();
                        gig.rekordbox_folder_id = folder_id;
                        let _ = show_gig_match_results(window, &gig, &results, lib);
                    }
                }
            }
        }
    }

    let mut store = GigStore::load();
    store.contacts.push(contact);
    store.gigs.push(gig);
    store.save();
}

#[allow(dead_code)]
fn show_gig_match_results(
    window:  &gtk::ApplicationWindow,
    gig:     &Gig,
    results: &[crate::matcher::MatchResult],
    lib:     &Library,
) -> Option<i64> {
    let matched: Vec<_> = results.iter().filter(|r| r.matched.is_some()).collect();
    let missing: Vec<_> = results.iter().filter(|r| r.matched.is_none()).collect();

    let dialog = gtk::Dialog::new();
    dialog.set_title(&format!("Gig Prep — {}", gig.name));
    dialog.set_transient_for(Some(window));
    dialog.set_modal(true);
    dialog.set_default_size(660, 560);
    dialog.add_button("Close",            gtk::ResponseType::Cancel);
    dialog.add_button("Create Playlist",  gtk::ResponseType::Accept);

    let content = dialog.get_content_area();
    content.set_border_width(12);
    content.set_spacing(8);

    // ── Matched section ───────────────────────────────────────────────────────
    let matched_lbl = gtk::Label::new(Some(&format!("✅ Matched ({} tracks)", matched.len())));
    matched_lbl.set_xalign(0.0);
    content.pack_start(&matched_lbl, false, false, 0);

    let matched_store = gtk::ListStore::new(&[
        glib::types::Type::String, // Spotify title
        glib::types::Type::String, // Spotify artist
        glib::types::Type::String, // Local match title
    ]);
    for r in &matched {
        let local = r.matched.as_ref().unwrap();
        matched_store.insert_with_values(None, &[0, 1, 2], &[
            &r.spotify.title.as_str(),
            &r.spotify.artist.as_str(),
            &local.title.as_str(),
        ]);
    }
    let matched_view = gtk::TreeView::with_model(&matched_store);
    for (i, title) in ["Spotify Title", "Artist", "Local Match"].iter().enumerate() {
        let col  = gtk::TreeViewColumn::new();
        let cell = gtk::CellRendererText::new();
        col.set_title(title);
        col.pack_start(&cell, true);
        col.add_attribute(&cell, "text", i as i32);
        matched_view.append_column(&col);
    }
    let matched_scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
    matched_scroll.set_min_content_height(160);
    matched_scroll.add(&matched_view);
    content.pack_start(&matched_scroll, true, true, 0);

    // ── Missing section ───────────────────────────────────────────────────────
    let missing_lbl = gtk::Label::new(Some(&format!("❌ Missing ({} tracks)", missing.len())));
    missing_lbl.set_xalign(0.0);
    content.pack_start(&missing_lbl, false, false, 0);

    let missing_store = gtk::ListStore::new(&[
        glib::types::Type::String,
        glib::types::Type::String,
    ]);
    for r in &missing {
        missing_store.insert_with_values(None, &[0, 1], &[
            &r.spotify.title.as_str(),
            &r.spotify.artist.as_str(),
        ]);
    }
    let missing_view = gtk::TreeView::with_model(&missing_store);
    for (i, title) in ["Title", "Artist"].iter().enumerate() {
        let col  = gtk::TreeViewColumn::new();
        let cell = gtk::CellRendererText::new();
        col.set_title(title);
        col.pack_start(&cell, true);
        col.add_attribute(&cell, "text", i as i32);
        missing_view.append_column(&col);
    }
    let missing_scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
    missing_scroll.set_min_content_height(120);
    missing_scroll.add(&missing_view);
    content.pack_start(&missing_scroll, true, true, 0);

    // ── Copy shopping list button ─────────────────────────────────────────────
    if !missing.is_empty() {
        let copy_btn = gtk::Button::with_label("Copy shopping list to clipboard");
        copy_btn.set_halign(gtk::Align::Start);
        let missing_spotify: Vec<_> = missing.iter().map(|r| &r.spotify).collect();
        let shopping = crate::matcher::shopping_list(&missing_spotify);
        copy_btn.connect_clicked(move |btn| {
            let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
            {
                clipboard.set_text(&shopping);
                btn.set_label("✓ Copied!");
            }
        });
        content.pack_start(&copy_btn, false, false, 0);
    }

    content.show_all();

    let response = dialog.run();
    dialog.close();

    if response != gtk::ResponseType::Accept || matched.is_empty() {
        return None;
    }

    // Create the Rekordbox playlist under the right folder
    let playlist_id = lib.find_or_create_folder("PRIVATE")
        .and_then(|folder_id| lib.create_playlist(&gig.name, Some(folder_id)))
        .and_then(|pl_id| {
            let track_ids: Vec<i64> = matched.iter()
                .map(|r| r.matched.as_ref().unwrap().id)
                .collect();
            lib.add_tracks_to_playlist(pl_id, &track_ids)?;
            Ok(pl_id)
        })
        .ok();

    playlist_id
}

// ── Gig sidebar helpers ───────────────────────────────────────────────────────

/// Populate the gig sidebar from the full Rekordbox playlist tree.
/// Only expanded contacts show their gigs/pools.
fn populate_gig_sidebar_from_library(
    list_box:          &gtk::ListBox,
    store:             &crate::gig::GigStore,
    playlists:         &[crate::rekordbox::Playlist],
    expanded_contacts: &std::collections::HashSet<String>,
) {
    for child in list_box.get_children() {
        list_box.remove(&child);
    }

    for contact in &store.contacts {
        let expanded = expanded_contacts.contains(&contact.id);
        list_box.add(&make_contact_header_row(contact, expanded));

        if !expanded {
            continue;
        }

        if let Some(folder_id) = contact.rekordbox_folder_id {
            let mut children: Vec<_> = playlists.iter()
                .filter(|pl| pl.parent_id == Some(folder_id))
                .collect();
            children.sort_by_key(|pl| pl.id);

            for child_pl in children {
                if child_pl.attribute == 1 {
                    let gig = store.gigs.iter()
                        .find(|g| g.rekordbox_folder_id == Some(child_pl.id));
                    list_box.add(&make_gig_folder_row(child_pl, gig));

                    let mut set_pls: Vec<_> = playlists.iter()
                        .filter(|pl| pl.parent_id == Some(child_pl.id))
                        .collect();
                    set_pls.sort_by_key(|pl| pl.id);
                    for set_pl in set_pls {
                        list_box.add(&make_set_playlist_row(set_pl));
                    }
                } else {
                    list_box.add(&make_pool_row(child_pl));
                }
            }
        } else {
            for gig in store.gigs_for_contact(&contact.id) {
                list_box.add(&make_gig_row_simple(gig));
            }
        }
    }

    list_box.show_all();
}

/// Fallback used before library is loaded and when creating new gigs.
fn populate_contacts_and_gigs(
    list_box:          &gtk::ListBox,
    store:             &crate::gig::GigStore,
    expanded_contacts: &std::collections::HashSet<String>,
) {
    for child in list_box.get_children() {
        list_box.remove(&child);
    }
    for contact in &store.contacts {
        let expanded = expanded_contacts.contains(&contact.id);
        list_box.add(&make_contact_header_row(contact, expanded));
        if expanded {
            for gig in store.gigs_for_contact(&contact.id) {
                list_box.add(&make_gig_row_simple(gig));
            }
        }
    }
    list_box.show_all();
}

fn make_contact_header_row(contact: &crate::gig::Contact, expanded: bool) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(&format!("contact:{}", contact.id));
    let arrow = if expanded { "▼" } else { "▶" };
    let lbl = gtk::Label::new(None);
    lbl.set_markup(&format!(
        "{} <b>{}</b>  <small>{}</small>",
        arrow,
        glib::markup_escape_text(&contact.name),
        contact.customer_type.label(),
    ));
    lbl.set_xalign(0.0);
    lbl.set_margin_start(6);
    lbl.set_margin_top(5);
    lbl.set_margin_bottom(5);
    row.add(&lbl);
    row
}

fn make_gig_folder_row(
    pl:  &crate::rekordbox::Playlist,
    gig: Option<&crate::gig::Gig>,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let widget_name = gig
        .map(|g| format!("gig:{}", g.id))
        .unwrap_or_else(|| format!("rb_folder:{}", pl.id));
    row.set_widget_name(&widget_name);
    let lbl = gtk::Label::new(Some(&pl.name));
    lbl.set_xalign(0.0);
    lbl.set_margin_start(18);
    lbl.set_margin_top(4);
    lbl.set_margin_bottom(4);
    row.add(&lbl);
    row
}

fn make_set_playlist_row(pl: &crate::rekordbox::Playlist) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(&format!("pl:{}", pl.id));
    let label_text = if pl.track_count > 0 {
        format!("  {} ({})", pl.name, pl.track_count)
    } else {
        format!("  {}", pl.name)
    };
    let lbl = gtk::Label::new(Some(&label_text));
    lbl.set_xalign(0.0);
    lbl.set_margin_start(32);
    lbl.set_margin_top(2);
    lbl.set_margin_bottom(2);
    row.add(&lbl);
    row
}

fn make_pool_row(pl: &crate::rekordbox::Playlist) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(&format!("pool:{}", pl.id));
    let label_text = if pl.track_count > 0 {
        format!("{} ({})", pl.name, pl.track_count)
    } else {
        pl.name.clone()
    };
    let lbl = gtk::Label::new(Some(&label_text));
    lbl.set_xalign(0.0);
    lbl.set_margin_start(18);
    lbl.set_margin_top(3);
    lbl.set_margin_bottom(3);
    row.add(&lbl);
    row
}

fn make_gig_row_simple(gig: &crate::gig::Gig) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(&format!("gig:{}", gig.id));
    let label_text = if gig.name.is_empty() {
        gig.date.as_deref().unwrap_or("New Gig").to_string()
    } else if let Some(date) = &gig.date {
        format!("{} ({})", gig.name, date)
    } else {
        gig.name.clone()
    };
    let lbl = gtk::Label::new(Some(&label_text));
    lbl.set_xalign(0.0);
    lbl.set_margin_start(18);
    lbl.set_margin_top(3);
    lbl.set_margin_bottom(3);
    row.add(&lbl);
    row
}

// ── Gig workspace ─────────────────────────────────────────────────────────────
//
// Widget names are used as stable keys to retrieve child widgets for
// load_gig_into_workspace. Each named widget corresponds to a Gig field.

fn build_gig_workspace() -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);

    // ── Header bar ────────────────────────────────────────────────────────────
    let header_bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    header_bar.set_margin_start(6);
    header_bar.set_margin_top(6);
    header_bar.set_margin_bottom(4);

    let back_btn = gtk::Button::with_label("← Contact");
    back_btn.set_relief(gtk::ReliefStyle::None);
    back_btn.set_widget_name("gig_back_btn");

    let header = gtk::Label::new(Some("Gig"));
    header.set_widget_name("gig_header");
    header.set_xalign(0.0);
    header.set_use_markup(true);
    header.set_hexpand(true);

    let saved_lbl = gtk::Label::new(None);
    saved_lbl.set_widget_name("gig_saved_lbl");
    saved_lbl.set_margin_end(6);

    header_bar.pack_start(&back_btn,  false, false, 0);
    header_bar.pack_start(&header,    true,  true,  4);
    header_bar.pack_end  (&saved_lbl, false, false, 0);

    outer.pack_start(&header_bar, false, false, 0);
    outer.pack_start(&gtk::Separator::new(gtk::Orientation::Horizontal), false, false, 0);

    // ── Notebook ──────────────────────────────────────────────────────────────
    let notebook = gtk::Notebook::new();
    notebook.set_widget_name("gig_notebook");

    macro_rules! field_lbl { ($t:expr) => {{
        let l = gtk::Label::new(Some($t));
        l.set_xalign(1.0);
        l
    }}; }
    macro_rules! field_entry { ($name:expr, $ph:expr) => {{
        let e = gtk::Entry::new();
        e.set_widget_name($name);
        e.set_placeholder_text(Some($ph));
        e.set_hexpand(true);
        e
    }}; }

    // ── Tab 1: Info ───────────────────────────────────────────────────────────
    {
        let scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

        let grid = gtk::Grid::new();
        grid.set_row_spacing(8);
        grid.set_column_spacing(8);
        grid.set_border_width(12);

        let contact_lbl = gtk::Label::new(None);
        contact_lbl.set_widget_name("gig_contact_label");
        contact_lbl.set_xalign(0.0);
        contact_lbl.set_use_markup(true);

        let time_box    = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let start_entry = gtk::Entry::new();
        start_entry.set_widget_name("gig_start_time");
        start_entry.set_placeholder_text(Some("HH:MM"));
        start_entry.set_width_chars(7);
        start_entry.set_hexpand(false);
        let sep_lbl     = gtk::Label::new(Some("–"));
        let end_entry   = gtk::Entry::new();
        end_entry.set_widget_name("gig_end_time");
        end_entry.set_placeholder_text(Some("HH:MM"));
        end_entry.set_width_chars(7);
        end_entry.set_hexpand(false);
        time_box.pack_start(&start_entry, false, false, 0);
        time_box.pack_start(&sep_lbl,     false, false, 2);
        time_box.pack_start(&end_entry,   false, false, 0);

        grid.attach(&field_lbl!("Contact"),  0, 0, 1, 1);
        grid.attach(&contact_lbl,            1, 0, 1, 1);
        grid.attach(&field_lbl!("Name"),     0, 1, 1, 1);
        grid.attach(&field_entry!("gig_name", "Event name"), 1, 1, 1, 1);
        grid.attach(&field_lbl!("Date"),     0, 2, 1, 1);
        grid.attach(&field_entry!("gig_date", "YYYY-MM-DD"), 1, 2, 1, 1);
        grid.attach(&field_lbl!("Time"),     0, 3, 1, 1);
        grid.attach(&time_box,               1, 3, 1, 1);
        grid.attach(&field_lbl!("Location"), 0, 4, 1, 1);
        grid.attach(&field_entry!("gig_location", "Venue name or address"), 1, 4, 1, 1);

        scroll.add(&grid);
        notebook.append_page(&scroll, Some(&gtk::Label::new(Some("Info"))));
    }

    // ── Tab 2: Brief ──────────────────────────────────────────────────────────
    {
        let scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
        vbox.set_border_width(12);

        let spotify_lbl = gtk::Label::new(Some("Spotify reference playlist (paste URL, then go to Match tab)"));
        spotify_lbl.set_xalign(0.0);
        spotify_lbl.set_use_markup(true);
        let spotify_entry = gtk::Entry::new();
        spotify_entry.set_widget_name("gig_spotify_url");
        spotify_entry.set_placeholder_text(Some("https://open.spotify.com/playlist/…"));
        spotify_entry.set_hexpand(true);

        let notes_lbl = gtk::Label::new(Some("Vibe / music preferences / client notes"));
        notes_lbl.set_xalign(0.0);
        let notes_view = gtk::TextView::new();
        notes_view.set_widget_name("gig_notes");
        notes_view.set_wrap_mode(gtk::WrapMode::Word);
        notes_view.set_accepts_tab(false);
        let notes_scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
        notes_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        notes_scroll.set_min_content_height(120);
        notes_scroll.set_vexpand(true);
        notes_scroll.add(&notes_view);

        vbox.pack_start(&spotify_lbl,   false, false, 0);
        vbox.pack_start(&spotify_entry, false, false, 0);
        vbox.pack_start(&notes_lbl,     false, false, 0);
        vbox.pack_start(&notes_scroll,  true,  true,  0);

        scroll.add(&vbox);
        notebook.append_page(&scroll, Some(&gtk::Label::new(Some("Brief"))));
    }

    // ── Tab 3: Match ──────────────────────────────────────────────────────────
    {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 6);
        vbox.set_border_width(12);

        let top_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let match_status = gtk::Label::new(Some("No match run yet"));
        match_status.set_widget_name("gig_match_status");
        match_status.set_xalign(0.0);
        match_status.set_hexpand(true);
        let run_match_btn = gtk::Button::with_label("Run Match");
        run_match_btn.set_widget_name("gig_run_match");
        top_bar.pack_start(&match_status,  true,  true,  0);
        top_bar.pack_start(&run_match_btn, false, false, 0);

        let match_list = gtk::ListBox::new();
        match_list.set_widget_name("gig_match_list");
        match_list.set_selection_mode(gtk::SelectionMode::None);
        let match_scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
        match_scroll.set_vexpand(true);
        match_scroll.add(&match_list);

        vbox.pack_start(&top_bar,      false, false, 0);
        vbox.pack_start(&match_scroll, true,  true,  0);

        notebook.append_page(&vbox, Some(&gtk::Label::new(Some("Match"))));
    }

    // ── Tab 4: Finalize ───────────────────────────────────────────────────────
    {
        let scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
        vbox.set_border_width(12);

        let dest_lbl = gtk::Label::new(Some("Destination: —"));
        dest_lbl.set_widget_name("gig_dest_label");
        dest_lbl.set_xalign(0.0);

        let create_status = gtk::Label::new(None);
        create_status.set_widget_name("gig_create_status");
        create_status.set_xalign(0.0);

        let create_btn = gtk::Button::with_label("Create Playlist in Rekordbox");
        create_btn.set_widget_name("gig_create_btn");
        create_btn.set_halign(gtk::Align::Start);

        let shopping_btn = gtk::Button::with_label("Copy shopping list");
        shopping_btn.set_widget_name("gig_shopping_btn");
        shopping_btn.set_halign(gtk::Align::Start);

        vbox.pack_start(&dest_lbl,      false, false, 0);
        vbox.pack_start(&create_status, false, false, 0);
        vbox.pack_start(&create_btn,    false, false, 0);
        vbox.pack_start(&shopping_btn,  false, false, 0);

        scroll.add(&vbox);
        notebook.append_page(&scroll, Some(&gtk::Label::new(Some("Finalize"))));
    }

    outer.pack_start(&notebook, true, true, 0);
    outer
}


fn find_widget(parent: &gtk::Box, name: &str) -> Option<gtk::Widget> {
    find_in_widget(parent.upcast_ref(), name)
}

fn find_in_widget(widget: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    if widget.get_widget_name() == name {
        return Some(widget.clone());
    }
    if let Some(container) = widget.downcast_ref::<gtk::Container>() {
        for child in container.get_children() {
            if let Some(found) = find_in_widget(&child, name) {
                return Some(found);
            }
        }
    }
    None
}

fn set_match_status(workspace: &gtk::Box, msg: &str) {
    if let Some(w) = find_widget(workspace, "gig_match_status") {
        if let Ok(lbl) = w.downcast::<gtk::Label>() {
            lbl.set_text(msg);
        }
    }
}

/// Populate the Match tab with results and wire up Accept/Skip toggles.
fn populate_match_results(
    workspace: &gtk::Box,
    gig_id:    &str,
    results:   &[crate::matcher::MatchResult],
    window:    &gtk::ApplicationWindow,
) {
    let match_list = match find_widget(workspace, "gig_match_list") {
        Some(w) => match w.downcast::<gtk::ListBox>() {
            Ok(lb) => lb,
            Err(_) => return,
        },
        None => return,
    };

    for child in match_list.get_children() {
        match_list.remove(&child);
    }

    let matched: Vec<_> = results.iter().filter(|r| r.matched.is_some()).collect();
    let missing: Vec<_> = results.iter().filter(|r| r.matched.is_none()).collect();

    // Section header for matched
    if !matched.is_empty() {
        let hdr = gtk::Label::new(Some(&format!("Matched ({} tracks)", matched.len())));
        hdr.set_xalign(0.0);
        hdr.set_margin_top(4);
        hdr.set_margin_bottom(2);
        let row = gtk::ListBoxRow::new();
        row.set_selectable(false);
        row.set_activatable(false);
        row.add(&hdr);
        match_list.add(&row);
    }

    // One row per matched track with Accept/Skip toggle
    for r in &matched {
        let local = r.matched.as_ref().unwrap();
        let track_id = local.id;

        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row_box.set_margin_start(4);
        row_box.set_margin_end(4);

        let label = gtk::Label::new(Some(&format!(
            "{} – {}",
            r.spotify.title,
            r.spotify.artist,
        )));
        label.set_xalign(0.0);
        label.set_hexpand(true);

        let local_lbl = gtk::Label::new(Some(&local.title));
        local_lbl.set_xalign(0.0);
        local_lbl.set_width_chars(20);

        // Load current accepted state
        let accepted = {
            let store = crate::gig::GigStore::load();
            store.gigs.iter().find(|g| g.id == gig_id)
                .map(|g| g.accepted_track_ids.contains(&track_id))
                .unwrap_or(true)
        };

        let toggle = gtk::ToggleButton::new();
        toggle.set_label(if accepted { "✓ Accept" } else { "Skip" });
        toggle.set_active(accepted);

        let gig_id_c = gig_id.to_string();
        toggle.connect_toggled(move |btn| {
            let active = btn.get_active();
            btn.set_label(if active { "✓ Accept" } else { "Skip" });
            let mut store = crate::gig::GigStore::load();
            if let Some(gig) = store.gigs.iter_mut().find(|g| g.id == gig_id_c) {
                if active {
                    if !gig.accepted_track_ids.contains(&track_id) {
                        gig.accepted_track_ids.push(track_id);
                    }
                } else {
                    gig.accepted_track_ids.retain(|&id| id != track_id);
                }
                store.save();
            }
        });

        row_box.pack_start(&label,      true,  true,  0);
        row_box.pack_start(&local_lbl,  false, false, 0);
        row_box.pack_start(&toggle,     false, false, 0);

        let row = gtk::ListBoxRow::new();
        row.set_selectable(false);
        row.add(&row_box);
        match_list.add(&row);
    }

    // Section header for missing
    if !missing.is_empty() {
        let hdr = gtk::Label::new(Some(&format!("Missing ({} tracks)", missing.len())));
        hdr.set_xalign(0.0);
        hdr.set_margin_top(8);
        hdr.set_margin_bottom(2);
        let row = gtk::ListBoxRow::new();
        row.set_selectable(false);
        row.set_activatable(false);
        row.add(&hdr);
        match_list.add(&row);
    }

    for r in &missing {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row_box.set_margin_start(4);
        row_box.set_margin_end(4);

        let label = gtk::Label::new(Some(&format!(
            "{} – {}",
            r.spotify.title,
            r.spotify.artist,
        )));
        label.set_xalign(0.0);
        label.set_hexpand(true);

        let missing_lbl = gtk::Label::new(Some("not in library"));
        missing_lbl.set_xalign(0.0);

        row_box.pack_start(&label,       true,  true,  0);
        row_box.pack_start(&missing_lbl, false, false, 0);

        let row = gtk::ListBoxRow::new();
        row.set_selectable(false);
        row.add(&row_box);
        match_list.add(&row);
    }

    // Wire up initial accepted_track_ids: add all matched tracks
    {
        let mut store = crate::gig::GigStore::load();
        if let Some(gig) = store.gigs.iter_mut().find(|g| g.id == gig_id) {
            let current: std::collections::HashSet<i64> = gig.accepted_track_ids.iter().cloned().collect();
            // Add newly matched tracks that aren't already tracked
            for r in &matched {
                let id = r.matched.as_ref().unwrap().id;
                if !current.contains(&id) {
                    gig.accepted_track_ids.push(id);
                }
            }
            store.save();
        }
    }

    let matched_count = matched.len();
    let missing_count = missing.len();
    set_match_status(workspace, &format!(
        "{} matched, {} missing",
        matched_count, missing_count,
    ));

    // Shopping list button in Finalize tab
    if !missing.is_empty() {
        let missing_spotify: Vec<_> = missing.iter().map(|r| &r.spotify).collect();
        let shopping = crate::matcher::shopping_list(&missing_spotify);
        if let Some(w) = find_widget(workspace, "gig_shopping_btn") {
            if let Ok(btn) = w.downcast::<gtk::Button>() {
                btn.connect_clicked(move |b| {
                    let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
                    clipboard.set_text(&shopping);
                    b.set_label("✓ Copied!");
                });
            }
        }
    }

    match_list.show_all();
}

fn load_gig_into_workspace(
    workspace: &gtk::Box,
    gig: &crate::gig::Gig,
    contact: &crate::gig::Contact,
) {
    // Stamp gig ID on the workspace so the back button and auto-save can use it
    workspace.set_widget_name(&format!("gig_workspace:{}", gig.id));

    // Update header
    if let Some(w) = find_widget(workspace, "gig_header") {
        if let Ok(lbl) = w.downcast::<gtk::Label>() {
            let title = if gig.name.is_empty() {
                contact.name.clone()
            } else {
                format!("{} – {}", contact.name, gig.name)
            };
            lbl.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(&title)));
        }
    }

    // Contact label (Info tab)
    if let Some(w) = find_widget(workspace, "gig_contact_label") {
        if let Ok(lbl) = w.downcast::<gtk::Label>() {
            lbl.set_markup(&format!(
                "{}  <small>{}</small>",
                glib::markup_escape_text(&contact.name),
                contact.customer_type.label(),
            ));
        }
    }

    macro_rules! set_entry { ($name:expr, $val:expr) => {
        if let Some(w) = find_widget(workspace, $name) {
            if let Ok(e) = w.downcast::<gtk::Entry>() {
                e.set_text($val);
            }
        }
    }; }

    // Info tab fields
    set_entry!("gig_name",       &gig.name);
    set_entry!("gig_date",       gig.date.as_deref().unwrap_or(""));
    set_entry!("gig_start_time", gig.start_time.as_deref().unwrap_or(""));
    set_entry!("gig_end_time",   gig.end_time.as_deref().unwrap_or(""));
    set_entry!("gig_location",   gig.location.as_deref().unwrap_or(""));

    // Brief tab fields
    set_entry!("gig_spotify_url", gig.spotify_playlist_url.as_deref().unwrap_or(""));
    if let Some(w) = find_widget(workspace, "gig_notes") {
        if let Ok(tv) = w.downcast::<gtk::TextView>() {
            if let Some(buf) = tv.get_buffer() {
                buf.set_text(&gig.notes);
            }
        }
    }

    // Match tab status
    if let Some(w) = find_widget(workspace, "gig_match_status") {
        if let Ok(lbl) = w.downcast::<gtk::Label>() {
            let status = if !gig.accepted_track_ids.is_empty() {
                format!("{} tracks accepted — ready to finalize", gig.accepted_track_ids.len())
            } else if gig.spotify_playlist_url.is_some() {
                "Spotify URL set — click Run Match".to_string()
            } else {
                "Add a Spotify URL in Brief, then run Match".to_string()
            };
            lbl.set_text(&status);
        }
    }

    // Clear previous match results
    if let Some(w) = find_widget(workspace, "gig_match_list") {
        if let Ok(lb) = w.downcast::<gtk::ListBox>() {
            for child in lb.get_children() {
                lb.remove(&child);
            }
        }
    }

    // Finalize tab
    if let Some(w) = find_widget(workspace, "gig_dest_label") {
        if let Ok(lbl) = w.downcast::<gtk::Label>() {
            lbl.set_text(&format!(
                "Destination: {}/{}/{}/",
                contact.customer_type.playlist_folder(),
                contact.name,
                gig.name,
            ));
        }
    }
    if let Some(w) = find_widget(workspace, "gig_create_status") {
        if let Ok(lbl) = w.downcast::<gtk::Label>() {
            lbl.set_text(if gig.rekordbox_folder_id.is_some() { "Playlist created ✓" } else { "" });
        }
    }
}

// ── Contact view ──────────────────────────────────────────────────────────────

fn build_contact_view() -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);

    // Header bar
    let header_bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    header_bar.set_margin_start(6);
    header_bar.set_margin_top(6);
    header_bar.set_margin_bottom(4);

    let back_btn = gtk::Button::with_label("← Library");
    back_btn.set_relief(gtk::ReliefStyle::None);
    back_btn.set_widget_name("contact_back_btn");

    let header = gtk::Label::new(Some("Contact"));
    header.set_widget_name("contact_header");
    header.set_xalign(0.0);
    header.set_use_markup(true);
    header.set_hexpand(true);

    let add_gig_btn = gtk::Button::with_label("+ New Gig");
    add_gig_btn.set_widget_name("contact_add_gig_btn");
    add_gig_btn.set_relief(gtk::ReliefStyle::None);

    let delete_btn = gtk::Button::with_label("Delete");
    delete_btn.set_widget_name("contact_delete_btn");
    delete_btn.set_relief(gtk::ReliefStyle::None);

    let saved_lbl = gtk::Label::new(None);
    saved_lbl.set_widget_name("contact_saved_lbl");
    saved_lbl.set_margin_end(4);

    header_bar.pack_start(&back_btn,    false, false, 0);
    header_bar.pack_start(&header,      true,  true,  4);
    header_bar.pack_end  (&add_gig_btn, false, false, 0);
    header_bar.pack_end  (&delete_btn,  false, false, 0);
    header_bar.pack_end  (&saved_lbl,   false, false, 0);

    outer.pack_start(&header_bar, false, false, 0);
    outer.pack_start(&gtk::Separator::new(gtk::Orientation::Horizontal), false, false, 0);

    let scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_border_width(12);

    // ── Fields ────────────────────────────────────────────────────────────────
    let grid = gtk::Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(8);

    macro_rules! field_lbl { ($t:expr) => {{
        let l = gtk::Label::new(Some($t));
        l.set_xalign(1.0);
        l
    }}; }

    let name_entry = gtk::Entry::new();
    name_entry.set_widget_name("contact_name");
    name_entry.set_placeholder_text(Some("Contact name"));
    name_entry.set_hexpand(true);

    let type_combo = gtk::ComboBoxText::new();
    type_combo.set_widget_name("contact_type");
    type_combo.append(Some("private"),   "Private");
    type_combo.append(Some("venue"),     "Venue");
    type_combo.append(Some("corporate"), "Corporate");
    type_combo.set_active_id(Some("private"));

    grid.attach(&field_lbl!("Name"),         0, 0, 1, 1);
    grid.attach(&name_entry,                 1, 0, 1, 1);
    grid.attach(&field_lbl!("Type"),         0, 1, 1, 1);
    grid.attach(&type_combo,                 1, 1, 1, 1);

    content.pack_start(&grid, false, false, 0);

    // ── Notes ─────────────────────────────────────────────────────────────────
    let notes_lbl = gtk::Label::new(Some("Music preferences / notes"));
    notes_lbl.set_xalign(0.0);

    let notes_view = gtk::TextView::new();
    notes_view.set_widget_name("contact_notes");
    notes_view.set_wrap_mode(gtk::WrapMode::Word);
    notes_view.set_accepts_tab(false);

    let notes_scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
    notes_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    notes_scroll.set_min_content_height(100);
    notes_scroll.add(&notes_view);

    content.pack_start(&notes_lbl,    false, false, 0);
    content.pack_start(&notes_scroll, false, false, 0);

    // ── Gig list ──────────────────────────────────────────────────────────────
    let gigs_lbl = gtk::Label::new(Some("Gigs"));
    gigs_lbl.set_xalign(0.0);
    gigs_lbl.set_margin_top(4);

    let gig_list = gtk::ListBox::new();
    gig_list.set_widget_name("contact_gig_list");
    gig_list.set_selection_mode(gtk::SelectionMode::Single);

    content.pack_start(&gigs_lbl, false, false, 0);
    content.pack_start(&gig_list, false, false, 0);

    scroll.add(&content);
    outer.pack_start(&scroll, true, true, 0);
    outer
}

fn load_contact_into_view(
    view:      &gtk::Box,
    contact:   &crate::gig::Contact,
    gigs:      &[&crate::gig::Gig],
    playlists: &[crate::rekordbox::Playlist],
) {
    // Header
    if let Some(w) = find_widget(view, "contact_header") {
        if let Ok(lbl) = w.downcast::<gtk::Label>() {
            lbl.set_markup(&format!(
                "<b>{}</b>  <small>{}</small>",
                glib::markup_escape_text(&contact.name),
                contact.customer_type.label(),
            ));
        }
    }

    // Store the contact ID on the view widget for auto-save
    view.set_widget_name(&format!("contact_view:{}", contact.id));

    if let Some(w) = find_widget(view, "contact_name") {
        if let Ok(e) = w.downcast::<gtk::Entry>() {
            e.set_text(&contact.name);
        }
    }

    if let Some(w) = find_widget(view, "contact_type") {
        if let Ok(combo) = w.downcast::<gtk::ComboBoxText>() {
            let id = match contact.customer_type {
                crate::gig::CustomerType::Corporate => "corporate",
                crate::gig::CustomerType::Venue     => "venue",
                crate::gig::CustomerType::Private   => "private",
            };
            combo.set_active_id(Some(id));
        }
    }

    if let Some(w) = find_widget(view, "contact_notes") {
        if let Ok(tv) = w.downcast::<gtk::TextView>() {
            if let Some(buf) = tv.get_buffer() {
                buf.set_text(&contact.notes);
            }
        }
    }

    // Build a set of pool playlist IDs (attribute=0, direct child of contact folder)
    let pool_ids: std::collections::HashSet<i64> = {
        let contact_folder_id = contact.rekordbox_folder_id;
        playlists.iter()
            .filter(|pl| pl.attribute == 0 && contact_folder_id.map_or(false, |cid| pl.parent_id == Some(cid)))
            .map(|pl| pl.id)
            .collect()
    };

    // Populate gig list (exclude pool playlists)
    if let Some(w) = find_widget(view, "contact_gig_list") {
        if let Ok(lb) = w.downcast::<gtk::ListBox>() {
            for child in lb.get_children() { lb.remove(&child); }
            for gig in gigs.iter().filter(|g| g.rekordbox_folder_id.map_or(true, |rid| !pool_ids.contains(&rid))) {
                let row = gtk::ListBoxRow::new();
                row.set_widget_name(&format!("gig:{}", gig.id));
                let label_text = if gig.name.is_empty() {
                    gig.date.as_deref().unwrap_or("Unnamed gig").to_string()
                } else if let Some(date) = &gig.date {
                    format!("{} ({})", gig.name, date)
                } else {
                    gig.name.clone()
                };
                let lbl = gtk::Label::new(Some(&label_text));
                lbl.set_xalign(0.0);
                lbl.set_margin_start(8);
                lbl.set_margin_top(4);
                lbl.set_margin_bottom(4);
                row.add(&lbl);
                lb.add(&row);
            }
            lb.show_all();
        }
    }
}
