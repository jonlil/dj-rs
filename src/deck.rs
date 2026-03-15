use std::path::PathBuf;
use std::time::{Duration, Instant};
use std::io::Cursor;
use rodio::{Decoder, Sink, Source, OutputStream, OutputStreamHandle};
use rodio::source::Zero;
use cpal::traits::{DeviceTrait, HostTrait};

/// Open an audio output stream on the default device, skipping devices that
/// look like cameras/webcams. Never iterates all devices to avoid probing USB
/// audio on webcams (which lights up the camera indicator).
pub fn open_audio_stream() -> Result<(OutputStream, OutputStreamHandle), String> {
    let bad_keywords = ["cam", "webcam", "video", "capture"];
    let host = cpal::default_host();

    // Try each available host's default device — PipeWire/PulseAudio first
    for host_id in cpal::available_hosts() {
        if let Ok(h) = cpal::host_from_id(host_id) {
            if let Some(dev) = h.default_output_device() {
                let name = dev.name().unwrap_or_default().to_lowercase();
                if bad_keywords.iter().any(|k| name.contains(k)) {
                    continue;
                }
                if let Ok(pair) = OutputStream::try_from_device(&dev) {
                    return Ok(pair);
                }
            }
        }
    }

    OutputStream::try_default().map_err(|e| e.to_string())
}

/// Read a file into memory for decoding.
/// Returns `(cursor, warning)` — warning is set for formats that need conversion.
/// M4A/AAC are pre-transcoded to WAV via ffmpeg because symphonia's ISOMP4 prober
/// returns a SeekError that rodio treats as unreachable!() — causing a hard crash.
fn read_file(path: &PathBuf) -> Result<(Cursor<Vec<u8>>, Option<String>), String> {
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if matches!(ext.as_str(), "m4a" | "aac" | "m4p") {
        let out = std::process::Command::new("ffmpeg")
            .args([
                "-i", path.to_str().ok_or("invalid path")?,
                "-f", "wav", "-loglevel", "error", "pipe:1",
            ])
            .output()
            .map_err(|e| format!("ffmpeg: {e}"))?;
        if !out.status.success() {
            return Err("Failed to decode M4A with ffmpeg".to_string());
        }
        let warning = Some(format!(
            "⚠ M4A/AAC file — playing via ffmpeg. Convert to FLAC for USB export (Pioneer CDJ/XDJ may not support this format)."
        ));
        return Ok((Cursor::new(out.stdout), warning));
    }

    std::fs::read(path)
        .map(|b| (Cursor::new(b), None))
        .map_err(|e| e.to_string())
}

fn make_decoder(cursor: Cursor<Vec<u8>>) -> Result<Decoder<Cursor<Vec<u8>>>, String> {
    Decoder::new(cursor).map_err(|e| e.to_string())
}

// --- Deck audio state ---

pub struct DeckState {
    pub stream: OutputStream,
    pub stream_handle: OutputStreamHandle,
    pub sink: Sink,
    _keepalive: Sink,
    pub file_path: Option<PathBuf>,
    pub duration_secs: f64,
    pub play_started_at: Option<Instant>,
    pub accumulated_secs: f64,
}

impl DeckState {
    pub fn new() -> Self {
        let (stream, stream_handle) = open_audio_stream()
            .expect("Failed to open audio output");
        let sink = Sink::try_new(&stream_handle)
            .expect("Failed to create audio sink");
        sink.pause();
        // Keep a silent source playing at all times so PipeWire never suspends
        // the audio device — prevents the resume-glitch on first play.
        let keepalive = Sink::try_new(&stream_handle)
            .expect("Failed to create keepalive sink");
        keepalive.append(Zero::<f32>::new(2, 48000));
        DeckState {
            stream,
            stream_handle,
            sink,
            _keepalive: keepalive,
            file_path: None,
            duration_secs: 0.0,
            play_started_at: None,
            accumulated_secs: 0.0,
        }
    }

    /// Returns `Ok(Some(warning))` on success with a format warning, `Ok(None)` on clean success.
    pub fn load(&mut self, path: PathBuf) -> Result<Option<String>, String> {
        let (cursor, warning) = read_file(&path)?;
        let decoder = make_decoder(cursor)
            .map_err(|e| e.to_string())?;

        self.duration_secs = decoder.total_duration()
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        // If playing, fade out briefly before swapping so there's no hard cut
        let vol = self.sink.volume();
        if !self.sink.is_paused() {
            for i in (0..=4).rev() {
                self.sink.set_volume(vol * i as f32 / 4.0);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        // clear() blocks until the audio thread has processed the clear — no race with append
        self.sink.clear();
        self.sink.set_volume(vol);
        self.sink.append(decoder);

        self.file_path = Some(path);
        self.accumulated_secs = 0.0;
        self.play_started_at = None;
        Ok(warning)
    }

    pub fn play(&mut self) {
        if self.file_path.is_some() && self.sink.is_paused() {
            self.play_started_at = Some(Instant::now());
            self.sink.play();
        }
    }

    pub fn pause(&mut self) {
        if !self.sink.is_paused() {
            self.accumulated_secs = self.current_position_secs();
            self.play_started_at = None;
            self.sink.pause();
        }
    }

    pub fn stop(&mut self) {
        self.accumulated_secs = 0.0;
        self.play_started_at = None;
        self.sink.clear();
        if let Some(path) = self.file_path.clone() {
            if let Ok((cursor, _)) = read_file(&path) {
                if let Ok(decoder) = make_decoder(cursor) {
                    self.sink.append(decoder);
                }
            }
        }
        self.sink.pause();
    }

    /// Seek to `pos` seconds, preserving play/pause state.
    pub fn seek_to(&mut self, pos: f64) -> Result<(), String> {
        let was_playing = self.play_started_at.is_some();
        if self.sink.try_seek(Duration::from_secs_f64(pos)).is_err() {
            // Sink was empty (track ended) — reload and seek
            let path = self.file_path.clone().ok_or("No track loaded")?;
            let (cursor, _) = read_file(&path)?;
            let decoder = make_decoder(cursor).map_err(|e| e.to_string())?;
            self.sink.clear();
            self.sink.append(decoder);
            let _ = self.sink.try_seek(Duration::from_secs_f64(pos));
        }
        if was_playing {
            self.sink.play();
        }
        self.accumulated_secs = pos;
        self.play_started_at = if was_playing { Some(Instant::now()) } else { None };
        Ok(())
    }

    pub fn current_position_secs(&self) -> f64 {
        match self.play_started_at {
            Some(t) => {
                let pos = self.accumulated_secs + t.elapsed().as_secs_f64();
                if self.duration_secs > 0.0 { pos.min(self.duration_secs) } else { pos }
            }
            None => self.accumulated_secs,
        }
    }
}
