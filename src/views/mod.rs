use gtk::prelude::*;
use gdk_pixbuf::prelude::*;
use std::rc::Rc;
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use glib::types::StaticType;
use crate::deck::DeckState;
use crate::config::Config;
use crate::rekordbox::{Library, Track, TrackFilter};
use crate::server::{ServerBridge, WsEvent};

mod utils;
mod browser;
mod gig_sidebar;
mod gig_workspace;
mod contact_view;
mod dialogs;

use utils::fmt_time;
use browser::{browser_populate_playlists, browser_populate_history, browser_populate_tracks};
use gig_sidebar::{populate_gig_sidebar_from_library, populate_contacts_and_gigs};
use gig_workspace::{build_gig_workspace, load_gig_into_workspace, set_match_status, populate_match_results};
use crate::librespot_player::LibrespotPlayer;
use contact_view::{build_contact_view, load_contact_into_view};
use dialogs::show_settings_dialog;

// ─── column indices ──────────────────────────────────────────────────────────

pub(self) const P_NAME:  u32 = 0;  // playlist name
pub(self) const P_COUNT: u32 = 1;  // track count (display string)
pub(self) const P_ID:    u32 = 2;  // id as string, "all" for the catch-all row
pub(self) const P_ATTR:  u32 = 3;  // attribute: "0" = playlist, "1" = folder, "h" = history

pub(self) const T_TITLE:    u32 = 0;
pub(self) const T_ARTIST:   u32 = 1;
pub(self) const T_BPM:      u32 = 2;
pub(self) const T_KEY:      u32 = 3;
pub(self) const T_DURATION: u32 = 4;
pub(self) const T_FILE_PATH: u32 = 5;  // hidden column
pub(self) const T_GENRE:    u32 = 6;
pub(self) const T_RATING:   u32 = 7;
pub(self) const T_LABEL:    u32 = 8;
pub(self) const T_COLOR:    u32 = 9;   // color_id as string, hidden
pub(self) const T_TRACK_ID:      u32 = 10;  // db id as string, hidden
pub(self) const T_BPM_RAW:      u32 = 11;  // raw bpm i32 as string, hidden
pub(self) const T_DURATION_RAW: u32 = 12;  // raw duration seconds i32 as string, hidden

// ─── PlayerView ──────────────────────────────────────────────────────────────

pub struct PlayerView {
    pub container: gtk::Frame,
    pub volume_scale: gtk::Scale,
    pub state: Rc<RefCell<DeckState>>,
    pub queue_fn: Rc<dyn Fn(Track)>,
    pub current_track_db_id: Rc<RefCell<Option<i64>>>,
    pub on_track_end: Rc<RefCell<Option<Rc<dyn Fn(i64)>>>>,
    pub spotify_player: Rc<LibrespotPlayer>,
}

impl PlayerView {
    pub fn new(_window: &gtk::ApplicationWindow, deck_label: &str, bridge: Arc<ServerBridge>, spotify_player: Rc<LibrespotPlayer>, config: Rc<RefCell<crate::config::Config>>) -> Self {
        let state = Rc::new(RefCell::new(DeckState::new()));
        let current_track_db_id: Rc<RefCell<Option<i64>>> = Rc::new(RefCell::new(None));
        let on_track_end: Rc<RefCell<Option<Rc<dyn Fn(i64)>>>> = Rc::new(RefCell::new(None));

        let frame = gtk::Frame::new(Some(deck_label));
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 6);
        vbox.set_border_width(8);

        // ── Info row: [album art] [title / BPM] [artist / Key] ──────────────

        // Album art — shows track/album art when available, grey otherwise
        let art_image = gtk::Image::new();
        art_image.set_size_request(80, 80);

        // Channel for background image fetches → GTK thread display
        let (art_tx, art_rx) = glib::MainContext::channel::<Option<Vec<u8>>>(glib::PRIORITY_LOW);
        {
            let art_image_rx = art_image.clone();
            art_rx.attach(None, move |bytes_opt| {
                match bytes_opt {
                    None => art_image_rx.clear(),
                    Some(bytes) => {
                        let loader = gdk_pixbuf::PixbufLoader::new();
                        let _ = loader.write(&bytes);
                        let _ = loader.close();
                        if let Some(pb) = loader.get_pixbuf() {
                            let scaled = pb.scale_simple(80, 80, gdk_pixbuf::InterpType::Bilinear);
                            art_image_rx.set_from_pixbuf(scaled.as_ref());
                        }
                    }
                }
                glib::Continue(true)
            });
        }

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
        info_row.pack_start(&art_image, false, false, 0);
        info_row.pack_start(&meta_box,  true,  true,  0);

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

        // ── Controls: [Cue] [▶/❚❚]  +  Convert/TV toggle (right) ───────────

        let play_btn    = gtk::Button::with_label("▶  Play");
        let cue_btn     = gtk::Button::with_label("Cue");
        let tv_btn      = gtk::ToggleButton::with_label("TV");
        let convert_btn = gtk::Button::with_label("Convert to FLAC");
        tv_btn.set_sensitive(false);
        convert_btn.set_no_show_all(true);

        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        controls.pack_start(&cue_btn,     false, false, 0);
        controls.pack_start(&play_btn,    false, false, 0);
        controls.pack_end  (&tv_btn,      false, false, 0);
        controls.pack_end  (&convert_btn, false, false, 0);

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

        // ── Error label (hidden until a load fails) ───────────────────────────

        let error_label = gtk::Label::new(None::<&str>);
        error_label.set_xalign(0.0);
        error_label.set_line_wrap(true);
        error_label.set_no_show_all(true);

        // ── Assemble ──────────────────────────────────────────────────────────

        vbox.pack_start(&info_row,       false, false, 0);
        vbox.pack_start(&wave_row,       false, false, 0);
        vbox.pack_start(&position_scale, false, false, 0);
        vbox.pack_start(&controls,       false, false, 0);
        vbox.pack_start(&error_label,    false, false, 0);

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
            let spotify_load       = spotify_player.clone();
            let error_label_load   = error_label.clone();
            let convert_btn_load   = convert_btn.clone();
            let art_tx_load        = art_tx.clone();
            Rc::new(move |track: Track| {
                // Stop Spotify if it was playing so the deck takes over
                if spotify_load.is_active() {
                    spotify_load.stop();
                }
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
                let image_path  = track.image_path.clone();
                let load_result = state.borrow_mut().load(path);
                match load_result {
                    Ok(warning) => {
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
                        if let Some(w) = warning {
                            error_label_load.set_text(&w);
                            error_label_load.show();
                            convert_btn_load.set_label("Convert to FLAC");
                            convert_btn_load.set_sensitive(true);
                            convert_btn_load.show();
                        } else {
                            error_label_load.set_text("");
                            error_label_load.hide();
                            convert_btn_load.hide();
                        };
                        // Load album art from rekordbox image path in background
                        let tx = art_tx_load.clone();
                        std::thread::spawn(move || {
                            let bytes = image_path.and_then(|p| std::fs::read(&p).ok());
                            let _ = tx.send(bytes);
                        });
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
                    }
                    Err(e) => {
                        track_label.set_text(&title);
                        error_label_load.set_text(&format!("⚠ {e}"));
                        error_label_load.show();
                        convert_btn_load.hide();
                        let _ = art_tx_load.send(None);
                    }
                }
            })
        };

        // Drag-and-drop onto the deck frame
        {
            let dnd_targets = [gtk::TargetEntry::new("text/plain", gtk::TargetFlags::empty(), 0)];
            frame.drag_dest_set(gtk::DestDefaults::ALL, &dnd_targets, gdk::DragAction::COPY);
            let do_load = do_load_track.clone();
            frame.connect_drag_data_received(move |_w, _ctx, _x, _y, sel, _info, _time| {
                let path_str = match std::str::from_utf8(&sel.get_data()) {
                    Ok(s) => s.to_string(),
                    Err(_) => return,
                };
                let title = {
                    let p = std::path::Path::new(&path_str);
                    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown");
                    stem.to_string()
                };
                do_load(Track {
                    id: 0, title, artist: None, album: None, genre: None,
                    key: None, bpm: None, duration_secs: None, rating: None,
                    play_count: None, file_path: Some(path_str), track_no: None,
                    label: None, color_id: None, image_path: None,
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
            let spotify_play          = spotify_player.clone();
            play_btn.connect_clicked(move |_| {
                // When Spotify is active, the deck button controls librespot
                if spotify_play.is_active() {
                    if spotify_play.is_paused() {
                        spotify_play.resume();
                        play_btn_ref.set_label("❚❚  Pause");
                    } else {
                        spotify_play.pause();
                        play_btn_ref.set_label("▶  Play");
                    }
                    return;
                }
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

        // Convert to FLAC button
        {
            let state           = state.clone();
            let current_db_id   = current_track_db_id.clone();
            let do_load         = do_load_track.clone();
            let error_label_c   = error_label.clone();
            let convert_btn_c   = convert_btn.clone();
            let config_conv     = config.clone();

            convert_btn.connect_clicked(move |btn| {
                let file_path = match state.borrow().file_path.clone() {
                    Some(p) => p,
                    None => return,
                };
                let db_id   = *current_db_id.borrow();
                let db_path = config_conv.borrow().resolved_db_path();

                btn.set_sensitive(false);
                btn.set_label("Converting…");

                let (tx, rx) = glib::MainContext::channel::<Result<std::path::PathBuf, String>>(glib::PRIORITY_DEFAULT);

                std::thread::spawn(move || {
                    let flac_path = file_path.with_extension("flac");
                    let result = std::process::Command::new("ffmpeg")
                        .args([
                            "-i",  file_path.to_str().unwrap_or(""),
                            "-c:a", "flac", "-loglevel", "error",
                            flac_path.to_str().unwrap_or(""),
                        ])
                        .output();

                    let outcome: Result<std::path::PathBuf, String> = match result {
                        Ok(out) if out.status.success() => Ok(flac_path),
                        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).into_owned()),
                        Err(e)  => Err(e.to_string()),
                    };

                    let outcome = outcome.and_then(|flac| {
                        if let (Some(id), Some(db)) = (db_id, db_path) {
                            crate::rekordbox::Library::open(&db)
                                .and_then(|lib| lib.update_track_path(id, flac.to_str().unwrap_or("")))
                                .map_err(|e| e.to_string())?;
                        }
                        Ok(flac)
                    });

                    let _ = tx.send(outcome);
                });

                let do_load2       = do_load.clone();
                let error_label2   = error_label_c.clone();
                let convert_btn2   = convert_btn_c.clone();

                rx.attach(None, move |outcome| {
                    match outcome {
                        Ok(flac) => {
                            do_load2(crate::rekordbox::Track {
                                id: db_id.unwrap_or(0),
                                title: flac.file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("Unknown")
                                    .to_string(),
                                artist: None,
                                album: None, genre: None, key: None,
                                bpm: None, duration_secs: None,
                                track_no: None, label: None, color_id: None,
                                rating: None, play_count: None,
                                file_path: Some(flac.to_str().unwrap_or("").to_string()),
                                image_path: None,
                            });
                        }
                        Err(e) => {
                            error_label2.set_text(&format!("⚠ Conversion failed: {e}"));
                            error_label2.show();
                            convert_btn2.set_label("Convert to FLAC");
                            convert_btn2.set_sensitive(true);
                        }
                    }
                    glib::Continue(false)
                });
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

        // Seek slider — user-initiated scrub
        {
            let state                = state.clone();
            let time_label           = time_label.clone();
            let bridge_seek          = bridge.clone();
            let tv_output_sk         = tv_output.clone();
            let current_db_sk        = current_track_db_id.clone();
            let spotify_player_seek  = spotify_player.clone();
            let time_label_seek      = time_label.clone();
            position_scale.connect_change_value(move |_scale, _scroll, value| {
                if spotify_player_seek.is_active() {
                    let dur = spotify_player_seek.track_info().2;
                    if dur <= 0.0 { return gtk::Inhibit(false); }
                    let pos = (value * dur).clamp(0.0, dur);
                    spotify_player_seek.seek(pos);
                    let remaining = (dur - pos).max(0.0);
                    time_label_seek.set_text(&format!("-{}", fmt_time(remaining)));
                    return gtk::Inhibit(false);
                }
                let dur = state.borrow().duration_secs;
                if dur <= 0.0 { return gtk::Inhibit(false); }
                let pos = (value * dur).clamp(0.0, dur);
                let _ = state.borrow_mut().seek_to(pos);
                let remaining = (dur - pos).max(0.0);
                time_label.set_text(&format!("-{}", fmt_time(remaining)));
                bridge_seek.send(WsEvent::Position { pos });
                if *tv_output_sk.borrow() {
                    if let Some(id) = *current_db_sk.borrow() {
                        bridge_seek.send(WsEvent::Stream { id, seek: pos });
                    }
                }
                gtk::Inhibit(false)
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
            let track_label          = track_label.clone();
            let artist_label         = artist_label.clone();
            let current_track_db_id2 = current_track_db_id.clone();
            let on_track_end2        = on_track_end.clone();
            let bridge_timer         = bridge.clone();
            let tv_output_timer      = tv_output.clone();
            let tv_btn_timer         = tv_btn.clone();
            let volume_scale_timer   = volume_scale.clone();
            let spotify_player_timer = spotify_player.clone();
            let art_tx_timer         = art_tx.clone();
            let config_timer         = config.clone();
            let mut tick: u32        = 0;
            glib::timeout_add_local(100, move || {
                tick += 1;

                // Every 5 minutes, refresh the Spotify token and reconnect librespot
                if tick % 3000 == 0 {
                    let refresh_token = config_timer.borrow().spotify_refresh_token.clone();
                    if let Some(rt) = refresh_token {
                        let config_ref    = config_timer.clone();
                        let player_ref    = spotify_player_timer.clone();
                        let (tx, rx) = glib::MainContext::channel::<(String, Option<String>)>(glib::PRIORITY_DEFAULT);
                        std::thread::spawn(move || {
                            if let Ok((new_access, new_refresh)) = crate::spotify::refresh(&rt) {
                                let _ = tx.send((new_access, new_refresh));
                            }
                        });
                        rx.attach(None, move |(new_access, new_refresh)| {
                            {
                                let mut cfg = config_ref.borrow_mut();
                                cfg.spotify_access_token = Some(new_access.clone());
                                if let Some(nr) = new_refresh {
                                    cfg.spotify_refresh_token = Some(nr);
                                }
                                cfg.save();
                            }
                            player_ref.set_token(new_access);
                            glib::Continue(false)
                        });
                    }
                }

                // If Spotify (librespot) is the active audio source, update deck display from it
                if spotify_player_timer.is_active() {
                    // Pause local deck if it's still playing
                    if !state.borrow().sink.is_paused() {
                        state.borrow_mut().pause();
                        play_btn.set_label("▶  Play");
                    }
                    let (title, artist, dur) = spotify_player_timer.track_info();
                    let cur_text = track_label.get_text();
                    if cur_text != title {
                        track_label.set_text(&title);
                        artist_label.set_text(&artist);
                        position_scale.set_sensitive(true);
                        // Fetch album art for the new Spotify track
                        let uri   = spotify_player_timer.current_uri();
                        let token = spotify_player_timer.access_token();
                        if let Some(token) = token {
                            if let Some(track_id) = uri.split(':').nth(2).map(|s| s.to_string()) {
                                let tx = art_tx_timer.clone();
                                std::thread::spawn(move || {
                                    let bytes = crate::spotify::fetch_track_image_url(&token, &track_id)
                                        .and_then(|url| reqwest::blocking::get(&url).ok())
                                        .and_then(|r| r.bytes().ok())
                                        .map(|b| b.to_vec());
                                    let _ = tx.send(bytes);
                                });
                            }
                        }
                    }
                    // Sync play/pause button label with librespot state
                    let expected_label = if spotify_player_timer.is_paused() { "▶  Play" } else { "❚❚  Pause" };
                    if play_btn.get_label().as_deref() != Some(expected_label) {
                        play_btn.set_label(expected_label);
                    }
                    let pos = spotify_player_timer.current_position_secs();
                    let fraction = if dur > 0.0 { (pos / dur).min(1.0) } else { 0.0 };
                    position_scale.set_value(fraction);
                    let remaining = if dur > 0.0 { (dur - pos).max(0.0) } else { 0.0 };
                    time_label.set_text(&format!("-{}", fmt_time(remaining)));
                    return glib::Continue(true);
                }

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
            spotify_player,
        }
    }
}

pub struct MainView {
    pub container: gtk::Box,
    pub queue_fn: Rc<dyn Fn(Track)>,
    pub current_track_db_id: Rc<RefCell<Option<i64>>>,
    pub on_track_end: Rc<RefCell<Option<Rc<dyn Fn(i64)>>>>,
    pub spotify_player: Rc<LibrespotPlayer>,
}

impl MainView {
    pub fn new(window: &gtk::ApplicationWindow, bridge: Arc<ServerBridge>, config: Rc<RefCell<crate::config::Config>>) -> Self {
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.set_border_width(8);

        let spotify_player = LibrespotPlayer::new();
        let player = PlayerView::new(window, "Player", bridge, spotify_player.clone(), config);
        let queue_fn = player.queue_fn.clone();
        let current_track_db_id = player.current_track_db_id.clone();
        let on_track_end = player.on_track_end.clone();

        container.pack_start(&player.container, true, true, 0);

        MainView { container, queue_fn, current_track_db_id, on_track_end, spotify_player }
    }
}

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
        spotify_player: Rc<LibrespotPlayer>,
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
            let _ = cell.set_property("xpad", &6u32);
            let _ = cell.set_property("ypad", &4u32);
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
                    // Use raw bytes to avoid GTK's text encoding mangling non-ASCII paths
                    let atom = gdk::Atom::intern("text/plain");
                    sel.set(&atom, 8, mapped.as_bytes());
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
                                track_no: None, label: None, color_id: None, image_path: None,
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
        let gig_workspace  = build_gig_workspace();
        let preview_player = spotify_player;
        let contact_view   = build_contact_view();

        // Auto-populate Match tab from cache when the user switches to it
        {
            let gig_workspace_c  = gig_workspace.clone();
            let library_c        = library.clone();
            let config_c         = config.clone();
            let preview_player_c = preview_player.clone();
            let window_c         = window.clone();

            if let Some(nb) = utils::find_widget(&gig_workspace, "gig_notebook") {
                if let Ok(notebook) = nb.downcast::<gtk::Notebook>() {
                    notebook.connect_switch_page(move |_, _, page_num| {
                        if page_num != 2 { return; } // Match tab is index 2

                        let wname  = gig_workspace_c.get_widget_name().to_string();
                        let gig_id = match wname.strip_prefix("gig_workspace:") {
                            Some(id) if !id.is_empty() => id.to_string(),
                            _ => return,
                        };

                        // Only load from cache if the list is currently empty
                        let is_empty = utils::find_widget(&gig_workspace_c, "gig_match_list")
                            .and_then(|w| w.downcast::<gtk::ListBox>().ok())
                            .map(|lb| lb.get_children().is_empty())
                            .unwrap_or(true);
                        if !is_empty { return; }

                        let store = crate::gig::GigStore::load();
                        let gig   = match store.gigs.iter().find(|g| g.id == gig_id) {
                            Some(g) => g.clone(),
                            None    => return,
                        };
                        if gig.cached_spotify_tracks.is_empty() { return; }

                        let lib_opt = library_c.borrow();
                        let lib = match lib_opt.as_ref() {
                            Some(l) => l,
                            None    => return,
                        };
                        let all_tracks = lib.tracks().unwrap_or_default();
                        let results    = crate::matcher::match_tracks(&gig.cached_spotify_tracks, &all_tracks);

                        set_match_status(&gig_workspace_c, &format!(
                            "Cached: {} tracks — click Run Match to refresh",
                            gig.cached_spotify_tracks.len(),
                        ));
                        populate_match_results(&gig_workspace_c, &gig_id, &results, &window_c, &preview_player_c);

                        // Refresh token in background then connect librespot
                        let refresh_token = config_c.borrow().spotify_refresh_token.clone();
                        let access_token  = config_c.borrow().spotify_access_token.clone();
                        let config_ref    = config_c.clone();
                        let player_ref    = preview_player_c.clone();
                        let (tx, rx) = glib::MainContext::channel::<(String, Option<String>, bool)>(glib::PRIORITY_DEFAULT);
                        std::thread::spawn(move || {
                            if let Some(rt) = refresh_token {
                                if let Ok((new_access, new_refresh)) = crate::spotify::refresh(&rt) {
                                    let _ = tx.send((new_access, new_refresh, true));
                                    return;
                                }
                            }
                            if let Some(t) = access_token { let _ = tx.send((t, None, false)); }
                        });
                        rx.attach(None, move |(new_access, new_refresh, did_refresh)| {
                            if did_refresh {
                                let mut cfg = config_ref.borrow_mut();
                                cfg.spotify_access_token = Some(new_access.clone());
                                if let Some(nr) = new_refresh {
                                    cfg.spotify_refresh_token = Some(nr);
                                }
                                cfg.save();
                            }
                            player_ref.set_token(new_access);
                            glib::Continue(false)
                        });
                    });
                }
            }
        }

        // Right panel: stack switching between track list, contact view, and gig workspace
        let right_stack = gtk::Stack::new();
        right_stack.add_named(&track_panel,    "tracks");
        right_stack.add_named(&contact_view,   "contact");
        right_stack.add_named(&gig_workspace,  "gig");
        right_stack.set_visible_child_name("tracks");

        // Wire up back button in gig workspace — returns to contact view
        if let Some(back_btn) = utils::find_widget(&gig_workspace, "gig_back_btn") {
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
        if let Some(back_btn) = utils::find_widget(&contact_view, "contact_back_btn") {
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
            let flash_timer: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));
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
                    if let Some(w) = utils::find_widget(&contact_view2, "contact_name") {
                        if let Ok(e) = w.downcast::<gtk::Entry>() {
                            contact.name = e.get_text().to_string();
                        }
                    }
                    // Read type
                    if let Some(w) = utils::find_widget(&contact_view2, "contact_type") {
                        if let Ok(combo) = w.downcast::<gtk::ComboBoxText>() {
                            contact.customer_type = match combo.get_active_id().as_deref() {
                                Some("corporate") => crate::gig::CustomerType::Corporate,
                                Some("venue")     => crate::gig::CustomerType::Venue,
                                _                 => crate::gig::CustomerType::Private,
                            };
                        }
                    }
                    // Read notes
                    if let Some(w) = utils::find_widget(&contact_view2, "contact_notes") {
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
                    // Flash "Saved" indicator — cancel any pending timer first
                    if let Some(w) = utils::find_widget(&contact_view2, "contact_saved_lbl") {
                        if let Ok(lbl) = w.downcast::<gtk::Label>() {
                            lbl.set_text("✓ Saved");
                            if let Some(src) = flash_timer.take() {
                                glib::source_remove(src);
                            }
                            let lbl_c = lbl.clone();
                            let src = glib::timeout_add_local(2000, move || {
                                lbl_c.set_text("");
                                glib::Continue(false)
                            });
                            flash_timer.set(Some(src));
                        }
                    }
                }
            });

            if let Some(w) = utils::find_widget(&contact_view, "contact_name") {
                if let Ok(e) = w.downcast::<gtk::Entry>() {
                    let save = save.clone();
                    e.connect_changed(move |_| save());
                }
            }
            if let Some(w) = utils::find_widget(&contact_view, "contact_type") {
                if let Ok(combo) = w.downcast::<gtk::ComboBoxText>() {
                    let save = save.clone();
                    combo.connect_changed(move |_| save());
                }
            }
            if let Some(w) = utils::find_widget(&contact_view, "contact_notes") {
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
            let flash_timer: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));
            let save = Rc::new(move || {
                let wname = gig_workspace2.get_widget_name().to_string();
                let gig_id = match wname.strip_prefix("gig_workspace:") {
                    Some(id) if !id.is_empty() => id.to_string(),
                    _ => return,
                };
                let mut store = crate::gig::GigStore::load();
                if let Some(gig) = store.gigs.iter_mut().find(|g| g.id == gig_id) {
                    macro_rules! read_entry { ($name:expr, $field:expr) => {
                        if let Some(w) = utils::find_widget(&gig_workspace2, $name) {
                            if let Ok(e) = w.downcast::<gtk::Entry>() {
                                let v = e.get_text().to_string();
                                $field = if v.is_empty() { None } else { Some(v) };
                            }
                        }
                    }; }
                    macro_rules! read_entry_str { ($name:expr, $field:expr) => {
                        if let Some(w) = utils::find_widget(&gig_workspace2, $name) {
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
                    if let Some(w) = utils::find_widget(&gig_workspace2, "gig_notes") {
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
                    // Flash "Saved" indicator — cancel any pending timer first
                    if let Some(w) = utils::find_widget(&gig_workspace2, "gig_saved_lbl") {
                        if let Ok(lbl) = w.downcast::<gtk::Label>() {
                            lbl.set_text("✓ Saved");
                            if let Some(src) = flash_timer.take() {
                                glib::source_remove(src);
                            }
                            let lbl_c = lbl.clone();
                            let src = glib::timeout_add_local(2000, move || {
                                lbl_c.set_text("");
                                glib::Continue(false)
                            });
                            flash_timer.set(Some(src));
                        }
                    }
                }
            });

            for name in &["gig_name", "gig_date", "gig_start_time", "gig_end_time", "gig_location", "gig_spotify_url"] {
                if let Some(w) = utils::find_widget(&gig_workspace, name) {
                    if let Ok(e) = w.downcast::<gtk::Entry>() {
                        let save = save.clone();
                        e.connect_changed(move |_| save());
                    }
                }
            }
            if let Some(w) = utils::find_widget(&gig_workspace, "gig_notes") {
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
            let preview2       = preview_player.clone();

            if let Some(w) = utils::find_widget(&gig_workspace, "gig_run_match") {
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
                                // Persist tracks to cache so the Match tab loads instantly next time
                                {
                                    let mut store = crate::gig::GigStore::load();
                                    if let Some(g) = store.gigs.iter_mut().find(|g| g.id == gig_id) {
                                        g.cached_spotify_tracks = spotify_tracks.clone();
                                        store.save();
                                    }
                                }
                                let all_tracks = lib.tracks().unwrap_or_default();
                                let results    = crate::matcher::match_tracks(&spotify_tracks, &all_tracks);
                                preview2.set_token(token.clone());
                                populate_match_results(&gig_workspace2, &gig_id, &results, &window2, &preview2);
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

            if let Some(w) = utils::find_widget(&gig_workspace, "gig_create_btn") {
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
                                if let Some(w) = utils::find_widget(&gig_workspace2, "gig_create_status") {
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

            if let Some(w) = utils::find_widget(&contact_view, "contact_add_gig_btn") {
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
                            cached_spotify_tracks: Vec::new(),
                            accepted_track_ids:    Vec::new(),
                            pending_buy_tracks:    Vec::new(),
                            denied_spotify_ids:    Vec::new(),
                            rekordbox_folder_id:   None,
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

            if let Some(w) = utils::find_widget(&contact_view, "contact_delete_btn") {
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

            if let Some(w) = utils::find_widget(&contact_view, "contact_gig_list") {
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

                        drop(lib_rc);
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
                if let Some(path) = config.borrow().resolved_db_path() {
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
            let saved_path = config.borrow().resolved_db_path();

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

                // Suppress unused variable warning
                let _ = &pl_store2;
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
                                    track_store3.set_value(&si, T_RATING, &utils::rating_stars(rating_val).to_value());
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
