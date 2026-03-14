use gtk::prelude::*;
use rodio::{OutputStream, Sink, buffer::SamplesBuffer};
use std::rc::Rc;
use std::cell::{Cell, RefCell};
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use std::time::Instant;

use librespot_playback::audio_backend::{Sink as LibrespotSink, SinkResult};
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;

// ── Custom audio sink: librespot → glib channel → rodio ──────────────────────

struct GlibSink {
    sender:     glib::Sender<(u64, Vec<f32>)>,
    generation: Arc<AtomicU64>,
}

impl LibrespotSink for GlibSink {
    fn write(&mut self, packet: AudioPacket, _converter: &mut Converter) -> SinkResult<()> {
        if let AudioPacket::Samples(samples) = packet {
            let gen = self.generation.load(Ordering::Relaxed);
            let f32_samples: Vec<f32> = samples.iter().map(|&s| s as f32).collect();
            let _ = self.sender.send((gen, f32_samples));
        }
        Ok(())
    }
}

// ── Player commands ───────────────────────────────────────────────────────────

enum PlayerCmd {
    Connect(String),         // Spotify access token
    Play(String),            // Spotify URI, start from beginning
    PlayAt(String, u32),     // Spotify URI + start position in ms
    Stop,
    Seek(u32),               // seek to position in ms (while playing)
}

// ── LibrespotPlayer ───────────────────────────────────────────────────────────

pub struct LibrespotPlayer {
    cmd_tx:              tokio::sync::mpsc::UnboundedSender<PlayerCmd>,
    playback:            Rc<RefCell<Option<(OutputStream, Sink)>>>,
    active_btn:          Rc<RefCell<Option<gtk::Button>>>,
    /// True when a track is loaded (may be paused or playing).
    loaded:              Rc<Cell<bool>>,
    /// True when playback is paused.
    paused:              Rc<Cell<bool>>,
    /// When the current timing period started (None when paused).
    play_started_at:     Rc<RefCell<Option<Instant>>>,
    /// Accumulated position before the current timing period (seconds).
    seek_offset_secs:    Rc<Cell<f64>>,
    track_duration_secs: Rc<Cell<f64>>,
    track_title:         Rc<RefCell<String>>,
    track_artist:        Rc<RefCell<String>>,
    current_uri:         Rc<RefCell<String>>,
    /// Incremented on every seek/play so stale buffered packets are discarded.
    generation:          Arc<AtomicU64>,
}

impl LibrespotPlayer {
    pub fn new() -> Rc<Self> {
        let (audio_tx, audio_rx) = glib::MainContext::channel::<(u64, Vec<f32>)>(glib::PRIORITY_DEFAULT);
        let (cmd_tx, cmd_rx)     = tokio::sync::mpsc::unbounded_channel::<PlayerCmd>();
        let playback             = Rc::new(RefCell::new(None::<(OutputStream, Sink)>));
        let active_btn           = Rc::new(RefCell::new(None::<gtk::Button>));
        let loaded               = Rc::new(Cell::new(false));
        let paused               = Rc::new(Cell::new(false));
        let play_started_at      = Rc::new(RefCell::new(None::<Instant>));
        let seek_offset_secs     = Rc::new(Cell::new(0.0f64));
        let track_duration_secs  = Rc::new(Cell::new(0.0f64));
        let track_title          = Rc::new(RefCell::new(String::new()));
        let track_artist         = Rc::new(RefCell::new(String::new()));
        let current_uri          = Rc::new(RefCell::new(String::new()));
        let generation           = Arc::new(AtomicU64::new(0));

        let player = Rc::new(Self {
            cmd_tx, playback, active_btn, loaded, paused,
            play_started_at, seek_offset_secs,
            track_duration_secs, track_title, track_artist, current_uri,
            generation,
        });

        // GTK main thread: receive decoded f32 samples and append to rodio Sink
        let playback_c   = player.playback.clone();
        let loaded_c     = player.loaded.clone();
        let paused_c     = player.paused.clone();
        let generation_c = player.generation.clone();
        audio_rx.attach(None, move |(gen, samples)| {
            // Discard packets that belong to a previous seek or play
            if !loaded_c.get() || paused_c.get() || gen != generation_c.load(Ordering::Relaxed) {
                return glib::Continue(true);
            }
            let mut pb = playback_c.borrow_mut();
            if let Some((_, sink)) = pb.as_ref() {
                sink.append(SamplesBuffer::new(2, 44100, samples));
            } else if let Ok((stream, handle)) = OutputStream::try_default() {
                if let Ok(sink) = Sink::try_new(&handle) {
                    sink.append(SamplesBuffer::new(2, 44100, samples));
                    *pb = Some((stream, sink));
                }
            }
            glib::Continue(true)
        });

        // Background thread: owns the tokio runtime and librespot session/player
        let gen_arc = player.generation.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime for librespot");
            rt.block_on(run_player_loop(cmd_rx, audio_tx, gen_arc));
        });

        player
    }

    /// Update the Spotify access token; creates a new librespot session asynchronously.
    pub fn set_token(&self, token: String) {
        let _ = self.cmd_tx.send(PlayerCmd::Connect(token));
    }

    /// Fully stop playback and reset all state.
    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.loaded.set(false);
        self.paused.set(false);
        *self.playback.borrow_mut() = None;
        *self.play_started_at.borrow_mut() = None;
        self.seek_offset_secs.set(0.0);
        if let Some(btn) = self.active_btn.borrow_mut().take() {
            btn.set_label("▶");
        }
        let _ = self.cmd_tx.send(PlayerCmd::Stop);
    }

    /// Play a Spotify track by URI (`spotify:track:<id>`) from the beginning.
    pub fn play(&self, track_uri: String, title: String, artist: String, duration_ms: u32, btn: gtk::Button) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.loaded.set(false);
        self.paused.set(false);
        *self.playback.borrow_mut() = None;
        if let Some(old) = self.active_btn.borrow_mut().take() {
            old.set_label("▶");
        }
        btn.set_label("■");
        *self.active_btn.borrow_mut() = Some(btn);
        self.loaded.set(true);
        self.seek_offset_secs.set(0.0);
        *self.play_started_at.borrow_mut() = Some(Instant::now());
        self.track_duration_secs.set(duration_ms as f64 / 1000.0);
        *self.track_title.borrow_mut() = title;
        *self.track_artist.borrow_mut() = artist;
        *self.current_uri.borrow_mut() = track_uri.clone();
        let _ = self.cmd_tx.send(PlayerCmd::Play(track_uri));
    }

    /// Pause playback, preserving position for resume.
    pub fn pause(&self) {
        if !self.loaded.get() || self.paused.get() { return; }
        self.generation.fetch_add(1, Ordering::Relaxed);
        if let Some(t) = *self.play_started_at.borrow() {
            self.seek_offset_secs.set(self.seek_offset_secs.get() + t.elapsed().as_secs_f64());
        }
        *self.play_started_at.borrow_mut() = None;
        self.paused.set(true);
        *self.playback.borrow_mut() = None;
        let _ = self.cmd_tx.send(PlayerCmd::Stop);
    }

    /// Resume from the paused position.
    pub fn resume(&self) {
        if !self.loaded.get() || !self.paused.get() { return; }
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.paused.set(false);
        *self.play_started_at.borrow_mut() = Some(Instant::now());
        let uri    = self.current_uri.borrow().clone();
        let pos_ms = (self.seek_offset_secs.get() * 1000.0) as u32;
        let _ = self.cmd_tx.send(PlayerCmd::PlayAt(uri, pos_ms));
    }

    /// Seek to a position in seconds. Works while playing or paused.
    pub fn seek(&self, pos_secs: f64) {
        let pos = pos_secs.max(0.0);
        self.seek_offset_secs.set(pos);
        if self.paused.get() {
            // Stay paused; position will be used on next resume
            return;
        }
        // Bump generation so in-flight packets from the old position are discarded
        self.generation.fetch_add(1, Ordering::Relaxed);
        *self.playback.borrow_mut() = None;
        *self.play_started_at.borrow_mut() = Some(Instant::now());
        // Re-load at the new position (more reliable than p.seek() for large jumps)
        let uri    = self.current_uri.borrow().clone();
        let pos_ms = (pos * 1000.0) as u32;
        let _ = self.cmd_tx.send(PlayerCmd::PlayAt(uri, pos_ms));
    }

    /// True when a track is loaded (playing or paused).
    pub fn is_active(&self) -> bool { self.loaded.get() }

    /// True when paused.
    pub fn is_paused(&self) -> bool { self.paused.get() }

    pub fn current_position_secs(&self) -> f64 {
        let base = self.seek_offset_secs.get();
        let elapsed = match *self.play_started_at.borrow() {
            Some(t) => t.elapsed().as_secs_f64(),
            None    => 0.0,
        };
        let pos = base + elapsed;
        let dur = self.track_duration_secs.get();
        if dur > 0.0 { pos.min(dur) } else { pos }
    }

    pub fn track_info(&self) -> (String, String, f64) {
        (
            self.track_title.borrow().clone(),
            self.track_artist.borrow().clone(),
            self.track_duration_secs.get(),
        )
    }
}

// ── Async player loop (runs on background tokio thread) ──────────────────────

async fn run_player_loop(
    mut cmd_rx:  tokio::sync::mpsc::UnboundedReceiver<PlayerCmd>,
    audio_tx:    glib::Sender<(u64, Vec<f32>)>,
    generation:  Arc<AtomicU64>,
) {
    use librespot_core::{Session, SessionConfig, authentication::Credentials, SpotifyUri};
    use librespot_playback::{
        config::PlayerConfig,
        mixer::NoOpVolume,
        player::Player,
    };

    let mut player: Option<std::sync::Arc<Player>> = None;

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            PlayerCmd::Connect(token) => {
                let session = Session::new(SessionConfig::default(), None);
                match session.connect(Credentials::with_access_token(token), false).await {
                    Ok(()) => {
                        let tx  = audio_tx.clone();
                        let gen = generation.clone();
                        let p = Player::new(
                            PlayerConfig::default(),
                            session,
                            Box::new(NoOpVolume),
                            move || Box::new(GlibSink { sender: tx.clone(), generation: gen.clone() }),
                        );
                        player = Some(p);
                    }
                    Err(e) => eprintln!("[librespot] session connect failed: {e}"),
                }
            }
            PlayerCmd::Play(uri) => {
                if let Some(ref p) = player {
                    match SpotifyUri::from_uri(&uri) {
                        Ok(spotify_uri) => p.load(spotify_uri, true, 0),
                        Err(e) => eprintln!("[librespot] invalid URI '{uri}': {e}"),
                    }
                } else {
                    eprintln!("[librespot] play called but no session — call set_token() first");
                }
            }
            PlayerCmd::PlayAt(uri, pos_ms) => {
                if let Some(ref p) = player {
                    match SpotifyUri::from_uri(&uri) {
                        Ok(spotify_uri) => p.load(spotify_uri, true, pos_ms),
                        Err(e) => eprintln!("[librespot] invalid URI '{uri}': {e}"),
                    }
                }
            }
            PlayerCmd::Stop => {
                if let Some(ref p) = player {
                    p.stop();
                }
            }
            PlayerCmd::Seek(ms) => {
                if let Some(ref p) = player {
                    p.seek(ms);
                }
            }
        }
    }
}
