use gtk::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use std::sync::Arc;
use glib::types::StaticType;
use crate::deck::DeckState;
use crate::config::{Config, PathMapping};
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
    pub queue_fn: Rc<dyn Fn(std::path::PathBuf)>,
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

        // Track info
        let track_label = gtk::Label::new(Some("No track loaded"));
        track_label.set_xalign(0.0);

        // Waveform area — placeholder until ANLZ parsing is implemented
        let waveform_area = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        waveform_area.set_size_request(-1, 80);

        // Position slider (fraction 0.0–1.0)
        let pos_adj = gtk::Adjustment::new(0.0, 0.0, 1.0, 0.001, 0.01, 0.0);
        let position_scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&pos_adj));
        position_scale.set_draw_value(false);
        position_scale.set_hexpand(true);
        position_scale.set_sensitive(false);

        // Time display
        let time_label = gtk::Label::new(Some("0:00 / 0:00"));

        // Controls: Play/Pause + Cue + TV output toggle
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        controls.set_homogeneous(true);
        let play_btn = gtk::Button::with_label("Play");
        let cue_btn  = gtk::Button::with_label("Cue");
        let tv_btn   = gtk::ToggleButton::with_label("TV");
        tv_btn.set_sensitive(false); // enabled only when a TV client is connected
        controls.pack_start(&play_btn, true, true, 0);
        controls.pack_start(&cue_btn,  true, true, 0);
        controls.pack_start(&tv_btn,   true, true, 0);

        // Volume scale — not shown in UI but available for level control
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

        vbox.pack_start(&track_label,    false, false, 0);
        vbox.pack_start(&waveform_area,  false, false, 0);
        vbox.pack_start(&position_scale, false, false, 0);
        vbox.pack_start(&time_label,     false, false, 0);
        vbox.pack_start(&controls,       false, false, 0);

        frame.add(&vbox);

        // Shared load-track logic (drag-and-drop + queue auto-advance)
        let do_load_track = {
            let state          = state.clone();
            let track_label    = track_label.clone();
            let position_scale = position_scale.clone();
            let time_label     = time_label.clone();
            let bridge_load    = bridge.clone();
            Rc::new(move |path: std::path::PathBuf| {
                let name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Unknown")
                    .to_string();
                if state.borrow_mut().load(path).is_ok() {
                    track_label.set_text(&name);
                    let dur = state.borrow().duration_secs;
                    position_scale.set_sensitive(true);
                    if dur > 0.0 {
                        time_label.set_text(&format!("0:00 / {}", fmt_time(dur)));
                    } else {
                        time_label.set_text("0:00 / ?");
                    }
                    bridge_load.send(WsEvent::Metadata {
                        title:    name,
                        artist:   String::new(),
                        duration: dur,
                    });
                    bridge_load.send(WsEvent::Position { pos: 0.0 });
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
                do_load(std::path::PathBuf::from(path_str));
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
                    play_btn_ref.set_label("Play");
                    bridge_play.send(WsEvent::State { playing: false });
                } else {
                    state.borrow_mut().play();
                    if state.borrow().play_started_at.is_some() {
                        play_btn_ref.set_label("Pause");
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
                play_btn.set_label("Play");
                position_scale.set_value(0.0);
                let dur = state.borrow().duration_secs;
                time_label.set_text(&format!(
                    "0:00 / {}",
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
        let queued_path: Rc<RefCell<Option<std::path::PathBuf>>> = Rc::new(RefCell::new(None));

        let queue_fn: Rc<dyn Fn(std::path::PathBuf)> = {
            let queued_path = queued_path.clone();
            Rc::new(move |path: std::path::PathBuf| {
                *queued_path.borrow_mut() = Some(path);
            })
        };

        // Position update + track-end + auto-advance timer
        {
            let state                = state.clone();
            let queued_path          = queued_path.clone();
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
                    time_label.set_text(&format!(
                        "{} / {}",
                        fmt_time(seek_pos),
                        if dur > 0.0 { fmt_time(dur) } else { "?".into() }
                    ));
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
                    play_btn.set_label("Play");
                    position_scale.set_value(0.0);
                    let dur = state.borrow().duration_secs;
                    time_label.set_text(&format!("0:00 / {}", fmt_time(dur)));
                    bridge_timer.send(WsEvent::State    { playing: false });
                    bridge_timer.send(WsEvent::Position { pos: 0.0 });

                    if let Some(path) = queued_path.borrow_mut().take() {
                        do_load(path);
                        state.borrow_mut().play();
                        if state.borrow().play_started_at.is_some() {
                            play_btn.set_label("Pause");
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
                    time_label.set_text(&format!(
                        "{} / {}",
                        fmt_time(pos),
                        if dur > 0.0 { fmt_time(dur) } else { "?".into() }
                    ));

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
    pub queue_fn: Rc<dyn Fn(std::path::PathBuf)>,
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
const T_TRACK_ID: u32 = 10;  // db id as string, hidden

// ─── BrowserView ─────────────────────────────────────────────────────────────

pub struct BrowserView {
    pub container: gtk::Box,
}

impl BrowserView {
    pub fn new(
        window: &gtk::ApplicationWindow,
        config: Rc<RefCell<Config>>,
        on_queue: Option<Rc<dyn Fn(std::path::PathBuf)>>,
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
        let open_btn     = gtk::Button::with_label("Open Library…");
        let settings_btn = gtk::Button::with_label("Settings…");
        let status_lbl   = gtk::Label::new(Some("No library loaded"));
        let search_entry = gtk::Entry::new();
        search_entry.set_placeholder_text(Some("Search tracks…"));
        search_entry.set_hexpand(true);
        topbar.pack_start(&open_btn,     false, false, 0);
        topbar.pack_start(&settings_btn, false, false, 0);
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
        let pl_store = gtk::ListStore::new(&[str_t, str_t, str_t, str_t]);
        // 11 columns: title, artist, bpm, key, duration, file_path, genre, rating, label, color_id, track_id
        let track_store = gtk::ListStore::new(&[
            str_t, str_t, str_t, str_t, str_t, str_t, str_t, str_t, str_t, str_t, str_t,
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
        pl_scroll.set_min_content_width(220);
        pl_scroll.add(&pl_view);

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
                            let raw: String = model
                                .get_value(&iter, T_FILE_PATH as i32)
                                .get::<String>().ok().flatten().unwrap_or_default();
                            let id_str: String = model
                                .get_value(&iter, T_TRACK_ID as i32)
                                .get::<String>().ok().flatten().unwrap_or_default();
                            if let Ok(id) = id_str.parse::<i64>() {
                                *cur_db_id2.borrow_mut() = Some(id);
                            }
                            let mapped = config.borrow().apply_mappings(&raw);
                            on_queue(std::path::PathBuf::from(mapped));
                        }
                    });
                }
                menu.append(&queue_item);
                menu.show_all();
                menu.popup_at_pointer(Some(event));
                gtk::Inhibit(true)
            });
        }

        // ── layout ───────────────────────────────────────────────────────────
        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.pack1(&pl_scroll, false, false);
        paned.pack2(&track_panel, true, true);
        paned.set_position(220);

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
            let track_store2         = track_store.clone();
            let status_lbl2          = status_lbl.clone();
            let config2              = config.clone();
            let window2              = window.clone();
            let on_track_end2        = on_track_end.clone();
            let key_combo2           = key_combo.clone();
            let genre_combo2         = genre_combo.clone();

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

                        // Populate playlists (including history)
                        let lists = lib.playlists().unwrap_or_default();
                        let sessions = lib.history_sessions().unwrap_or_default();
                        browser_populate_playlists(&pl_store2, &lists, &sessions);

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

        // ── open library button ───────────────────────────────────────────────
        {
            let do_open = do_open_library.clone();
            let window  = window.clone();

            open_btn.connect_clicked(move |_| {
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

        // ── settings button ───────────────────────────────────────────────────
        {
            let config = config.clone();
            let window = window.clone();

            settings_btn.connect_clicked(move |_| {
                show_settings_dialog(&window, &config);
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
            let library  = library.clone();
            let pl_store2 = pl_store.clone();
            let pl_view2 = pl_view.clone();
            let window   = window.clone();

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
                    let library  = library.clone();
                    let pl_store3 = pl_store2.clone();
                    let window   = window.clone();
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
                                browser_populate_playlists(&pl_store3, &lists, &sessions);
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
                    let library  = library.clone();
                    let pl_store3 = pl_store2.clone();
                    let window   = window.clone();
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
                                browser_populate_playlists(&pl_store3, &lists, &sessions);
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
                            let library  = library.clone();
                            let pl_store3 = pl_store2.clone();
                            let window   = window.clone();
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
                                        browser_populate_playlists(&pl_store3, &lists, &sessions);
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
                        let library  = library.clone();
                        let pl_store3 = pl_store2.clone();
                        let window   = window.clone();
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
                                    browser_populate_playlists(&pl_store3, &lists, &sessions);
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
                let library  = library.clone();
                let pl_store2 = pl_store.clone();
                let pl_view2 = pl_view.clone();

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
                                browser_populate_playlists(&pl_store2, &lists, &sessions);
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
                                browser_populate_playlists(&pl_store2, &lists, &sessions);
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

fn browser_populate_playlists(
    store: &gtk::ListStore,
    playlists: &[Playlist],
    sessions: &[HistorySession],
) {
    store.clear();
    store.insert_with_values(
        None,
        &[P_NAME, P_COUNT, P_ID, P_ATTR],
        &[&"★ All Tracks", &"", &"all", &"0"],
    );

    // Build parent → children map (sorted by Seq via the DB ordering)
    let mut children: std::collections::HashMap<Option<i64>, Vec<&Playlist>> =
        std::collections::HashMap::new();
    for pl in playlists {
        children.entry(pl.parent_id).or_default().push(pl);
    }

    // Recursive tree walk: insert folder then its children
    fn insert_node(
        store: &gtk::ListStore,
        children: &std::collections::HashMap<Option<i64>, Vec<&Playlist>>,
        parent_id: Option<i64>,
        depth: usize,
    ) {
        let indent = "  ".repeat(depth);
        if let Some(nodes) = children.get(&parent_id) {
            for pl in nodes {
                let name = if pl.attribute == 1 {
                    format!("{}▸ {}", indent, pl.name)
                } else {
                    format!("{}{}", indent, pl.name)
                };
                let count = if pl.attribute == 1 {
                    String::new()
                } else {
                    pl.track_count.to_string()
                };
                let id   = pl.id.to_string();
                let attr = pl.attribute.to_string();
                store.insert_with_values(
                    None,
                    &[P_NAME, P_COUNT, P_ID, P_ATTR],
                    &[&name.as_str(), &count.as_str(), &id.as_str(), &attr.as_str()],
                );
                // Recurse into folder children
                if pl.attribute == 1 {
                    insert_node(store, children, Some(pl.id), depth + 1);
                }
            }
        }
    }

    insert_node(store, &children, None, 0);

    if !sessions.is_empty() {
        store.insert_with_values(
            None,
            &[P_NAME, P_COUNT, P_ID, P_ATTR],
            &[&"— History —", &"", &"history_header", &"h"],
        );
        for s in sessions {
            let id   = format!("h:{}", s.id);
            let name = format!("  {}", s.name);
            let cnt  = s.track_count.to_string();
            store.insert_with_values(
                None,
                &[P_NAME, P_COUNT, P_ID, P_ATTR],
                &[&name.as_str(), &cnt.as_str(), &id.as_str(), &"h"],
            );
        }
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
        store.insert_with_values(
            None,
            &[T_TITLE, T_ARTIST, T_BPM, T_KEY, T_DURATION,
              T_FILE_PATH, T_GENRE, T_RATING, T_LABEL, T_COLOR, T_TRACK_ID],
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
            ],
        );
    }
}
