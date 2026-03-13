use gtk::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use glib::types::StaticType;
use crate::deck::{DeckState, list_output_devices, default_device_index};
use crate::config::{Config, PathMapping};
use crate::rekordbox::{Library, Track, Playlist};
use crate::dlna::{DlnaClient, Renderer, ssdp_blocked_by_vpn, lan_ip};

fn fmt_time(secs: f64) -> String {
    let s = secs as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub struct PlayerView {
    pub container: gtk::Frame,
    pub volume_scale: gtk::Scale,
    pub state: Rc<RefCell<DeckState>>,
    pub queue_fn: Rc<dyn Fn(std::path::PathBuf)>,
}

impl PlayerView {
    pub fn new(window: &gtk::ApplicationWindow, deck_label: &str) -> Self {
        let state = Rc::new(RefCell::new(DeckState::new()));

        let frame = gtk::Frame::new(Some(deck_label));
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 6);
        vbox.set_border_width(8);

        // Track info
        let track_label = gtk::Label::new(Some("No track loaded"));

        // Time display
        let time_label = gtk::Label::new(Some("0:00 / 0:00"));

        // Position slider (display only, fraction 0.0-1.0)
        let pos_adj = gtk::Adjustment::new(0.0, 0.0, 1.0, 0.001, 0.01, 0.0);
        let position_scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&pos_adj));
        position_scale.set_draw_value(false);
        position_scale.set_hexpand(true);
        position_scale.set_sensitive(false);

        // Controls
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        controls.set_homogeneous(true);
        let load_btn = gtk::Button::with_label("Load");
        let play_btn = gtk::Button::with_label("Play");
        let stop_btn = gtk::Button::with_label("Stop");
        controls.pack_start(&load_btn, true, true, 0);
        controls.pack_start(&play_btn, true, true, 0);
        controls.pack_start(&stop_btn, true, true, 0);

        // Volume
        let vol_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        vol_row.pack_start(&gtk::Label::new(Some("Vol")), false, false, 0);
        let vol_adj = gtk::Adjustment::new(1.0, 0.0, 1.5, 0.01, 0.1, 0.0);
        let volume_scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&vol_adj));
        volume_scale.set_hexpand(true);
        volume_scale.set_draw_value(false);
        vol_row.pack_start(&volume_scale, true, true, 0);

        // Device selector
        let device_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        device_row.pack_start(&gtk::Label::new(Some("Out")), false, false, 0);
        let device_combo = gtk::ComboBoxText::new();
        let devices = Rc::new(list_output_devices());
        for entry in devices.iter() {
            device_combo.append_text(&entry.display);
        }
        let default_idx = default_device_index(&devices);
        device_combo.set_active(Some(default_idx as u32));
        device_combo.set_hexpand(true);
        device_row.pack_start(&device_combo, true, true, 0);

        // DLNA cast row
        let dlna_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let cast_btn        = gtk::Button::with_label("Cast…");
        let cast_off_btn    = gtk::Button::with_label("Stop Cast");
        let dlna_status_lbl = gtk::Label::new(Some("Not casting"));
        dlna_status_lbl.set_hexpand(true);
        cast_off_btn.set_sensitive(false);
        dlna_row.pack_start(&gtk::Label::new(Some("Cast:")), false, false, 0);
        dlna_row.pack_start(&cast_btn,        false, false, 0);
        dlna_row.pack_start(&cast_off_btn,    false, false, 0);
        dlna_row.pack_start(&dlna_status_lbl, true,  true,  0);

        // Queue / next track row
        let next_row  = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let next_label = gtk::Label::new(Some("Next: —"));
        next_label.set_hexpand(true);
        next_label.set_xalign(0.0);
        let next_btn = gtk::Button::with_label("Next →");
        next_btn.set_sensitive(false);
        next_row.pack_start(&next_label, true,  true,  0);
        next_row.pack_start(&next_btn,   false, false, 0);

        vbox.pack_start(&track_label,   false, false, 0);
        vbox.pack_start(&position_scale,false, false, 0);
        vbox.pack_start(&time_label,    false, false, 0);
        vbox.pack_start(&controls,      false, false, 0);
        vbox.pack_start(&vol_row,       false, false, 4);
        vbox.pack_start(&device_row,    false, false, 0);
        vbox.pack_start(&dlna_row,      false, false, 0);
        vbox.pack_start(&next_row,      false, false, 0);

        frame.add(&vbox);

        // Shared load-track logic used by both the Load button and drag-and-drop
        let do_load_track = {
            let state          = state.clone();
            let track_label    = track_label.clone();
            let position_scale = position_scale.clone();
            let time_label     = time_label.clone();
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
                } else {
                    track_label.set_text("Error loading file");
                }
            })
        };

        // --- Drop target on the deck frame ---
        {
            let dnd_targets = [gtk::TargetEntry::new(
                "text/plain",
                gtk::TargetFlags::empty(),
                0,
            )];
            frame.drag_dest_set(
                gtk::DestDefaults::ALL,
                &dnd_targets,
                gdk::DragAction::COPY,
            );
            let do_load = do_load_track.clone();
            frame.connect_drag_data_received(move |_w, _ctx, _x, _y, sel, _info, _time| {
                let path_str = match sel.get_text() {
                    Some(s) => s.to_string(),
                    None    => return,
                };
                do_load(std::path::PathBuf::from(path_str));
            });
        }

        // --- Load button ---
        {
            let do_load = do_load_track.clone();
            let window = window.clone();
            load_btn.connect_clicked(move |_| {
                let dialog = gtk::FileChooserDialog::new(
                    Some("Open Audio File"),
                    Some(&window),
                    gtk::FileChooserAction::Open,
                );
                let filter = gtk::FileFilter::new();
                filter.set_name(Some("Audio Files"));
                filter.add_pattern("*.mp3");
                filter.add_pattern("*.wav");
                filter.add_pattern("*.ogg");
                filter.add_pattern("*.flac");
                dialog.add_filter(&filter);
                dialog.add_button("Cancel", gtk::ResponseType::Cancel);
                dialog.add_button("Open", gtk::ResponseType::Accept);

                let response = dialog.run();
                dialog.close();

                if response == gtk::ResponseType::Accept {
                    if let Some(path) = dialog.get_filename() {
                        do_load(path);
                    }
                }
            });
        }

        // Shared DLNA state — renderer selected via Cast button, used by play/pause/stop
        let dlna            = Rc::new(DlnaClient::new());
        let active_renderer: Rc<RefCell<Option<Renderer>>> = Rc::new(RefCell::new(None));

        // --- Play/Pause button ---
        {
            let state           = state.clone();
            let play_btn_ref    = play_btn.clone();
            let dlna            = dlna.clone();
            let active_renderer = active_renderer.clone();
            let dlna_status_lbl = dlna_status_lbl.clone();
            play_btn.connect_clicked(move |_| {
                let is_playing = state.borrow().play_started_at.is_some();
                if is_playing {
                    state.borrow_mut().pause();
                    play_btn_ref.set_label("Play");
                    if let Some(r) = active_renderer.borrow().clone() {
                        if let Err(e) = dlna.pause_renderer(r.location.clone()) {
                            dlna_status_lbl.set_text(&format!("Cast pause error: {}", e));
                        }
                    }
                } else {
                    state.borrow_mut().play();
                    if state.borrow().play_started_at.is_some() {
                        play_btn_ref.set_label("Pause");
                        if let Some(r) = active_renderer.borrow().clone() {
                            if let Err(e) = dlna.resume_renderer(r.location.clone()) {
                                dlna_status_lbl.set_text(&format!("Cast play error: {}", e));
                            }
                        }
                    }
                }
            });
        }

        // --- Stop button ---
        {
            let state           = state.clone();
            let play_btn        = play_btn.clone();
            let position_scale  = position_scale.clone();
            let time_label      = time_label.clone();
            let dlna            = dlna.clone();
            let active_renderer = active_renderer.clone();
            let dlna_status_lbl = dlna_status_lbl.clone();
            stop_btn.connect_clicked(move |_| {
                state.borrow_mut().stop();
                play_btn.set_label("Play");
                position_scale.set_value(0.0);
                let dur = state.borrow().duration_secs;
                time_label.set_text(&format!("0:00 / {}", if dur > 0.0 { fmt_time(dur) } else { "?".into() }));
                if let Some(r) = active_renderer.borrow().clone() {
                    let _ = dlna.stop_renderer(r.location.clone());
                    dlna_status_lbl.set_text(&format!("⏹ {}", r.friendly_name));
                }
            });
        }

        // --- Device selector ---
        {
            let state = state.clone();
            let devices = devices.clone();
            device_combo.connect_changed(move |combo| {
                let idx = match combo.get_active() {
                    Some(i) => i as usize,
                    None => return,
                };
                if let Some(entry) = devices.get(idx) {
                    let _ = state.borrow_mut().change_device(entry.host_id, &entry.device_name);
                }
            });
        }

        // --- Cast button: discover renderers, pick one, start HTTP server ---
        {
            let dlna            = dlna.clone();
            let active_renderer = active_renderer.clone();
            let state           = state.clone();
            let dlna_status_lbl = dlna_status_lbl.clone();
            let cast_off_btn    = cast_off_btn.clone();
            let play_btn        = play_btn.clone();
            let window          = window.clone();
            cast_btn.connect_clicked(move |_| {
                if ssdp_blocked_by_vpn() {
                    let lan = lan_ip().map(|ip| ip.to_string()).unwrap_or_else(|_| "?".into());
                    let d = gtk::MessageDialog::new(
                        Some(&window),
                        gtk::DialogFlags::MODAL,
                        gtk::MessageType::Warning,
                        gtk::ButtonsType::OkCancel,
                        &format!(
                            "A VPN is active — SSDP discovery will likely fail.\n\n\
                             Your LAN IP is {}.\n\nContinue anyway?",
                            lan
                        ),
                    );
                    let resp = d.run(); d.close();
                    if resp != gtk::ResponseType::Ok { return; }
                }

                dlna_status_lbl.set_text("Discovering…");
                while gtk::events_pending() { gtk::main_iteration_do(false); }

                let renderers = match dlna.discover_renderers() {
                    Ok(r) => r,
                    Err(e) => {
                        dlna_status_lbl.set_text("Discovery failed");
                        let d = gtk::MessageDialog::new(Some(&window), gtk::DialogFlags::MODAL,
                            gtk::MessageType::Error, gtk::ButtonsType::Ok,
                            &format!("Discovery failed:\n{}", e));
                        d.run(); d.close(); return;
                    }
                };
                if renderers.is_empty() {
                    dlna_status_lbl.set_text("No renderers found");
                    let d = gtk::MessageDialog::new(Some(&window), gtk::DialogFlags::MODAL,
                        gtk::MessageType::Info, gtk::ButtonsType::Ok,
                        "No DLNA renderers found.\n\nMake sure your TV is on and DLNA is enabled.");
                    d.run(); d.close(); return;
                }

                // Renderer picker
                let dialog = gtk::Dialog::new();
                dialog.set_title("Select Renderer");
                dialog.set_transient_for(Some(&window));
                dialog.set_modal(true);
                dialog.set_default_size(360, -1);
                dialog.add_button("Cancel", gtk::ResponseType::Cancel);
                dialog.add_button("Select", gtk::ResponseType::Accept);
                let combo = gtk::ComboBoxText::new();
                for r in &renderers { combo.append_text(&r.friendly_name); }
                combo.set_active(Some(0));
                let content = dialog.get_content_area();
                content.set_border_width(12);
                content.set_spacing(8);
                content.pack_start(&gtk::Label::new(Some("Choose a renderer:")), false, false, 0);
                content.pack_start(&combo, false, false, 0);
                content.show_all();
                let response = dialog.run();
                let idx = combo.get_active().unwrap_or(0) as usize;
                dialog.close();
                if response != gtk::ResponseType::Accept {
                    dlna_status_lbl.set_text("Not casting");
                    return;
                }

                let renderer = renderers[idx].clone();

                // Start HTTP server for the current track (if one is loaded)
                let file_path = state.borrow().file_path.clone();
                if let Some(path) = file_path {
                    match dlna.start_http_server(path.clone()) {
                        Ok(url) => {
                            // If already playing, also start playback on the renderer
                            let is_playing = state.borrow().play_started_at.is_some();
                            if is_playing {
                                match dlna.play_on_renderer(renderer.location.clone(), url) {
                                    Ok(()) => dlna_status_lbl.set_text(&format!("▶ {}", renderer.friendly_name)),
                                    Err(e) => dlna_status_lbl.set_text(&format!("Cast error: {}", e)),
                                }
                            } else {
                                dlna_status_lbl.set_text(&format!("⏸ {} (press Play)", renderer.friendly_name));
                                // Pre-load the track on the renderer so Play works immediately
                                let _ = dlna.set_uri_on_renderer(renderer.location.clone(), url);
                            }
                        }
                        Err(e) => dlna_status_lbl.set_text(&format!("Server error: {}", e)),
                    }
                } else {
                    dlna_status_lbl.set_text(&format!("⏸ {} (load a track to play)", renderer.friendly_name));
                }

                *active_renderer.borrow_mut() = Some(renderer);
                cast_off_btn.set_sensitive(true);
                play_btn.set_label("Play");
            });
        }

        // --- Stop Cast button ---
        {
            let dlna            = dlna.clone();
            let active_renderer = active_renderer.clone();
            let dlna_status_lbl = dlna_status_lbl.clone();
            let cast_off_btn_ref = cast_off_btn.clone();
            cast_off_btn.connect_clicked(move |_| {
                if let Some(r) = active_renderer.borrow().clone() {
                    let _ = dlna.stop_renderer(r.location.clone());
                }
                dlna.stop_http_server();
                *active_renderer.borrow_mut() = None;
                dlna_status_lbl.set_text("Not casting");
                cast_off_btn_ref.set_sensitive(false);
            });
        }

        // Shared queued-track state
        let queued_path: Rc<RefCell<Option<std::path::PathBuf>>> = Rc::new(RefCell::new(None));

        // queue_fn: called by BrowserView when user right-clicks → Queue
        let queue_fn: Rc<dyn Fn(std::path::PathBuf)> = {
            let queued_path = queued_path.clone();
            let next_label  = next_label.clone();
            let next_btn    = next_btn.clone();
            Rc::new(move |path: std::path::PathBuf| {
                let name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string();
                next_label.set_text(&format!("Next: {}", name));
                next_btn.set_sensitive(true);
                *queued_path.borrow_mut() = Some(path);
            })
        };

        // --- Next → button: load queued track and play it ---
        {
            let queued_path     = queued_path.clone();
            let do_load         = do_load_track.clone();
            let state           = state.clone();
            let dlna            = dlna.clone();
            let active_renderer = active_renderer.clone();
            let play_btn        = play_btn.clone();
            let next_label      = next_label.clone();
            let next_btn_ref    = next_btn.clone();
            let dlna_status_lbl = dlna_status_lbl.clone();
            next_btn.connect_clicked(move |_| {
                let path = match queued_path.borrow_mut().take() {
                    Some(p) => p,
                    None => return,
                };
                next_label.set_text("Next: —");
                next_btn_ref.set_sensitive(false);

                // Load and play locally
                do_load(path.clone());
                state.borrow_mut().play();
                if state.borrow().play_started_at.is_some() {
                    play_btn.set_label("Pause");
                }

                // Play on renderer if one is active
                if let Some(r) = active_renderer.borrow().clone() {
                    match dlna.start_http_server(path) {
                        Ok(url) => {
                            match dlna.set_uri_on_renderer(r.location.clone(), url) {
                                Ok(()) => {
                                    match dlna.resume_renderer(r.location.clone()) {
                                        Ok(()) => dlna_status_lbl.set_text(&format!("▶ {}", r.friendly_name)),
                                        Err(e) => dlna_status_lbl.set_text(&format!("Cast play error: {}", e)),
                                    }
                                }
                                Err(e) => dlna_status_lbl.set_text(&format!("Cast URI error: {}", e)),
                            }
                        }
                        Err(e) => dlna_status_lbl.set_text(&format!("Server error: {}", e)),
                    }
                }
            });
        }

        // --- Position update timer (always running) ---
        {
            let state           = state.clone();
            let queued_path     = queued_path.clone();
            let do_load         = do_load_track.clone();
            let position_scale  = position_scale.clone();
            let time_label      = time_label.clone();
            let play_btn        = play_btn.clone();
            let next_label      = next_label.clone();
            let next_btn        = next_btn.clone();
            let dlna            = dlna.clone();
            let active_renderer = active_renderer.clone();
            let dlna_status_lbl = dlna_status_lbl.clone();
            glib::timeout_add_local(100, move || {

                // Check if track ended naturally
                let (is_started, sink_empty) = {
                    let st = state.borrow();
                    (st.play_started_at.is_some(), st.sink.empty())
                };

                if is_started && sink_empty {
                    {
                        let mut st = state.borrow_mut();
                        st.play_started_at = None;
                        st.accumulated_secs = 0.0;
                    }
                    play_btn.set_label("Play");
                    position_scale.set_value(0.0);
                    let dur = state.borrow().duration_secs;
                    time_label.set_text(&format!("0:00 / {}", fmt_time(dur)));

                    // Auto-advance to queued track if one is set
                    if let Some(path) = queued_path.borrow_mut().take() {
                        next_label.set_text("Next: —");
                        next_btn.set_sensitive(false);
                        do_load(path.clone());
                        state.borrow_mut().play();
                        if state.borrow().play_started_at.is_some() {
                            play_btn.set_label("Pause");
                        }
                        if let Some(r) = active_renderer.borrow().clone() {
                            if let Ok(url) = dlna.start_http_server(path) {
                                let _ = dlna.set_uri_on_renderer(r.location.clone(), url.clone());
                                match dlna.resume_renderer(r.location.clone()) {
                                    Ok(()) => dlna_status_lbl.set_text(&format!("▶ {}", r.friendly_name)),
                                    Err(e) => dlna_status_lbl.set_text(&format!("Cast error: {}", e)),
                                }
                            }
                        }
                    }

                    return glib::Continue(true);
                }

                // Update display while playing
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
                }

                glib::Continue(true)
            });
        }

        PlayerView { container: frame, volume_scale, state, queue_fn }
    }
}

pub struct MainView {
    pub container: gtk::Box,
    pub queue_fn: Rc<dyn Fn(std::path::PathBuf)>,
}

impl MainView {
    pub fn new(window: &gtk::ApplicationWindow) -> Self {
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.set_border_width(8);

        let player = PlayerView::new(window, "Deck");
        let queue_fn = player.queue_fn.clone();

        {
            let state = player.state.clone();
            player.volume_scale.connect_value_changed(move |scale| {
                state.borrow().sink.set_volume(scale.get_value() as f32);
            });
        }

        container.pack_start(&player.container, true, true, 0);

        MainView { container, queue_fn }
    }
}

// ─── column indices ──────────────────────────────────────────────────────────

const P_NAME:  u32 = 0;  // playlist name
const P_COUNT: u32 = 1;  // track count (display string)
const P_ID:    u32 = 2;  // id as string, "all" for the catch-all row
const P_ATTR:  u32 = 3;  // attribute: "0" = playlist, "1" = folder

const T_TITLE:     u32 = 0;
const T_ARTIST:    u32 = 1;
const T_BPM:       u32 = 2;
const T_KEY:       u32 = 3;
const T_DURATION:  u32 = 4;
const T_FILE_PATH: u32 = 5;  // hidden column, not shown in TreeView

// ─── BrowserView ─────────────────────────────────────────────────────────────

pub struct BrowserView {
    pub container: gtk::Box,
}

impl BrowserView {
    pub fn new(window: &gtk::ApplicationWindow, config: Rc<RefCell<Config>>, on_queue: Option<Rc<dyn Fn(std::path::PathBuf)>>) -> Self {
        let library: Rc<RefCell<Option<Library>>> = Rc::new(RefCell::new(None));
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);

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

        // ── stores ───────────────────────────────────────────────────────────
        let str_t = String::static_type();
        let pl_store = gtk::ListStore::new(&[str_t, str_t, str_t, str_t]);
        let track_store = gtk::ListStore::new(&[str_t, str_t, str_t, str_t, str_t, str_t]);

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
            ("Title",   T_TITLE as i32,    true),
            ("Artist",  T_ARTIST as i32,   true),
            ("BPM",     T_BPM as i32,      false),
            ("Key",     T_KEY as i32,      false),
            ("Time",    T_DURATION as i32, false),
        ] {
            let col = gtk::TreeViewColumn::new();
            let cell = gtk::CellRendererText::new();
            col.pack_start(&cell, true);
            col.add_attribute(&cell, "text", idx);
            col.set_title(title);
            col.set_expand(expand);
            col.set_resizable(true);
            track_view.append_column(&col);
        }

        let track_scroll = gtk::ScrolledWindow::new(
            gtk::NONE_ADJUSTMENT,
            gtk::NONE_ADJUSTMENT,
        );
        track_scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        track_scroll.add(&track_view);
        track_scroll.set_hexpand(true);
        track_scroll.set_vexpand(true);

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

            let config = config.clone();
            track_view.connect_drag_data_get(move |view, _ctx, sel, _info, _time| {
                let selection = view.get_selection();
                if let Some((model, iter)) = selection.get_selected() {
                    let raw: String = model
                        .get_value(&iter, T_FILE_PATH as i32)
                        .get::<String>()
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    let mapped = config.borrow().apply_mappings(&raw);
                    sel.set_text(&mapped);
                }
            });
        }

        // ── track right-click context menu ───────────────────────────────────
        if let Some(on_queue) = on_queue {
            let config = config.clone();
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
                    let on_queue = on_queue.clone();
                    let config   = config.clone();
                    let view     = view.clone();
                    queue_item.connect_activate(move |_| {
                        let sel = view.get_selection();
                        if let Some((model, iter)) = sel.get_selected() {
                            let raw: String = model
                                .get_value(&iter, T_FILE_PATH as i32)
                                .get::<String>()
                                .ok()
                                .flatten()
                                .unwrap_or_default();
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
        paned.pack2(&track_scroll, true, true);
        paned.set_position(220);

        container.pack_start(&topbar, false, false, 0);
        container.pack_start(&paned, true, true, 0);

        // ── shared open-library logic ─────────────────────────────────────────
        let do_open_library = {
            let library     = library.clone();
            let pl_store    = pl_store.clone();
            let track_store = track_store.clone();
            let status_lbl  = status_lbl.clone();
            let config      = config.clone();
            let window      = window.clone();

            Rc::new(move |path_str: &str| {
                match Library::open(path_str) {
                    Ok(lib) => {
                        if let Ok(lists) = lib.playlists() {
                            browser_populate_playlists(&pl_store, &lists);
                        }
                        if let Ok(tracks) = lib.tracks() {
                            browser_populate_tracks(&track_store, &tracks);
                            status_lbl.set_text(&format!("{} tracks", tracks.len()));
                        }
                        config.borrow_mut().db_path = Some(path_str.to_string());
                        config.borrow().save();
                        *library.borrow_mut() = Some(lib);
                    }
                    Err(e) => {
                        let d = gtk::MessageDialog::new(
                            Some(&window),
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

        // ── playlist right-click context menu ────────────────────────────────
        {
            let library  = library.clone();
            let pl_store = pl_store.clone();
            let pl_view2 = pl_view.clone();
            let window   = window.clone();

            pl_view.connect_button_press_event(move |view, event| {
                if event.get_button() != 3 {
                    return gtk::Inhibit(false);
                }
                if library.borrow().is_none() {
                    return gtk::Inhibit(false);
                }

                // Find what row (if any) was right-clicked, and its attribute
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
                            if id_val == "all" {
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
                    let pl_store = pl_store.clone();
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
                                if let Ok(lists) = library.borrow().as_ref().unwrap().playlists() {
                                    browser_populate_playlists(&pl_store, &lists);
                                }
                            }
                            Err(e) => {
                                let d = gtk::MessageDialog::new(
                                    Some(&window),
                                    gtk::DialogFlags::MODAL,
                                    gtk::MessageType::Error,
                                    gtk::ButtonsType::Ok,
                                    &format!("Failed to create playlist:\n{}", e),
                                );
                                d.run();
                                d.close();
                            }
                        }
                    });
                }
                menu.append(&new_item);

                // ── New Folder ──
                let new_folder_item = gtk::MenuItem::with_label("New Folder…");
                {
                    let library  = library.clone();
                    let pl_store = pl_store.clone();
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
                                if let Ok(lists) = library.borrow().as_ref().unwrap().playlists() {
                                    browser_populate_playlists(&pl_store, &lists);
                                }
                            }
                            Err(e) => {
                                let d = gtk::MessageDialog::new(
                                    Some(&window),
                                    gtk::DialogFlags::MODAL,
                                    gtk::MessageType::Error,
                                    gtk::ButtonsType::Ok,
                                    &format!("Failed to create folder:\n{}", e),
                                );
                                d.run();
                                d.close();
                            }
                        }
                    });
                }
                menu.append(&new_folder_item);

                // ── New Playlist in Folder (only when right-clicking a folder) ──
                if clicked_is_folder {
                    if let Some(folder_id) = clicked_id {
                        let new_in_folder_item = gtk::MenuItem::with_label("New Playlist in Folder…");
                        {
                            let library  = library.clone();
                            let pl_store = pl_store.clone();
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
                                let result = library.borrow().as_ref().unwrap().create_playlist(name.trim(), Some(folder_id));
                                match result {
                                    Ok(_) => {
                                        if let Ok(lists) = library.borrow().as_ref().unwrap().playlists() {
                                            browser_populate_playlists(&pl_store, &lists);
                                        }
                                    }
                                    Err(e) => {
                                        let d = gtk::MessageDialog::new(
                                            Some(&window),
                                            gtk::DialogFlags::MODAL,
                                            gtk::MessageType::Error,
                                            gtk::ButtonsType::Ok,
                                            &format!("Failed to create playlist:\n{}", e),
                                        );
                                        d.run();
                                        d.close();
                                    }
                                }
                            });
                        }
                        menu.append(&new_in_folder_item);
                    }
                }

                // ── Delete Playlist (only when right-clicking a real playlist) ──
                if let Some(pid) = clicked_id {
                    // Select the row so the user sees what's being deleted
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
                        let pl_store = pl_store.clone();
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
                                    if let Ok(lists) = library.borrow().as_ref().unwrap().playlists() {
                                        browser_populate_playlists(&pl_store, &lists);
                                    }
                                }
                                Err(e) => {
                                    let d = gtk::MessageDialog::new(
                                        Some(&window),
                                        gtk::DialogFlags::MODAL,
                                        gtk::MessageType::Error,
                                        gtk::ButtonsType::Ok,
                                        &format!("Failed to delete playlist:\n{}", e),
                                    );
                                    d.run();
                                    d.close();
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
                    if id != "all" {
                        sel.set_text(&id);
                    }
                }
            });

            {
                let library  = library.clone();
                let pl_store = pl_store.clone();
                let pl_view2 = pl_view.clone();

                pl_view.connect_drag_data_received(move |_view, ctx, x, y, sel, _info, time| {
                    let src_id_str = match sel.get_text() {
                        Some(s) => s.to_string(),
                        None    => { ctx.drag_finish(false, false, time); return; }
                    };
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

                    if dest_id_str == "all" || dest_id_str == src_id_str {
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
                                if let Ok(lists) = library.borrow().as_ref().unwrap().playlists() {
                                    browser_populate_playlists(&pl_store, &lists);
                                }
                                ctx.drag_finish(true, false, time);
                            }
                            Err(_) => { ctx.drag_finish(false, false, time); }
                        }
                    } else {
                        let dest_id: i64 = match dest_id_str.parse() {
                            Ok(v)  => v,
                            Err(_) => { ctx.drag_finish(false, false, time); return; }
                        };

                        // Collect all IDs from the model in visual order,
                        // excluding "all" and the item being dragged
                        let mut ordered: Vec<i64> = Vec::new();
                        if let Some(iter) = model.get_iter_first() {
                            loop {
                                let id_s: String = model
                                    .get_value(&iter, P_ID as i32)
                                    .get::<String>()
                                    .ok()
                                    .flatten()
                                    .unwrap_or_default();
                                if id_s != "all" {
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
                                if let Ok(lists) = library.borrow().as_ref().unwrap().playlists() {
                                    browser_populate_playlists(&pl_store, &lists);
                                }
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
                // Defer until the main loop is running so the window is visible
                glib::idle_add_local(move || {
                    do_open(&path);
                    glib::Continue(false)
                });
            }
        }

        // ── playlist selection ────────────────────────────────────────────────
        {
            let library     = library.clone();
            let track_store = track_store.clone();
            let status_lbl  = status_lbl.clone();

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

                if let Some(lib) = library.borrow().as_ref() {
                    let result = if id == "all" {
                        lib.tracks()
                    } else {
                        match id.parse::<i64>() {
                            Ok(pid) => lib.playlist_tracks(pid),
                            Err(_)  => return,
                        }
                    };
                    if let Ok(tracks) = result {
                        let n = tracks.len();
                        browser_populate_tracks(&track_store, &tracks);
                        status_lbl.set_text(&format!("{} tracks", n));
                    }
                }
            });
        }

        // ── search ────────────────────────────────────────────────────────────
        {
            let library     = library.clone();
            let track_store = track_store.clone();
            let status_lbl  = status_lbl.clone();

            search_entry.connect_changed(move |entry| {
                let text: String = entry.get_text().to_string();

                if let Some(lib) = library.borrow().as_ref() {
                    let result: rusqlite::Result<Vec<crate::rekordbox::Track>> =
                        if text.is_empty() {
                            lib.tracks()
                        } else {
                            lib.search_tracks(&text)
                        };
                    if let Ok(tracks) = result {
                        browser_populate_tracks(&track_store, &tracks);
                        status_lbl.set_text(&format!("{} tracks", tracks.len()));
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

    // ── heading ──
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

    // ── scrolled rows area ──
    let rows_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_min_content_height(120);
    scroll.add(&rows_box);
    content.pack_start(&scroll, true, true, 0);

    // Track entry pairs so we can collect values on Save
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

    // Populate existing mappings
    for m in &config.borrow().path_mappings {
        add_row(&m.from, &m.to);
    }

    // ── Add row button ──
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

fn browser_populate_playlists(store: &gtk::ListStore, playlists: &[Playlist]) {
    store.clear();
    store.insert_with_values(
        None,
        &[P_NAME, P_COUNT, P_ID, P_ATTR],
        &[&"★ All Tracks", &"", &"all", &"0"],
    );
    for pl in playlists {
        let name = if pl.attribute == 1 {
            format!("▸ {}", pl.name)
        } else {
            format!("  {}", pl.name)
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
        store.insert_with_values(
            None,
            &[T_TITLE, T_ARTIST, T_BPM, T_KEY, T_DURATION, T_FILE_PATH],
            &[
                &t.title.as_str(),
                &artist.as_str(),
                &bpm.as_str(),
                &key.as_str(),
                &duration.as_str(),
                &file_path.as_str(),
            ],
        );
    }
}
