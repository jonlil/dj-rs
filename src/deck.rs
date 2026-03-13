use std::path::PathBuf;
use std::time::{Duration, Instant};
use std::fs::File;
use std::io::BufReader;
use rodio::{Decoder, Sink, Source, OutputStream, OutputStreamHandle};
use cpal::traits::{DeviceTrait, HostTrait};

// --- Device enumeration ---

#[derive(Clone)]
pub struct DeviceEntry {
    pub display: String,
    pub host_id: cpal::HostId,
    pub device_name: String,
}

pub fn list_output_devices() -> Vec<DeviceEntry> {
    let mut entries = Vec::new();
    for host_id in cpal::available_hosts() {
        if let Ok(host) = cpal::host_from_id(host_id) {
            if let Ok(devices) = host.output_devices() {
                for device in devices {
                    if let Ok(name) = device.name() {
                        entries.push(DeviceEntry {
                            display: format!("{:?} › {}", host_id, name),
                            host_id,
                            device_name: name,
                        });
                    }
                }
            }
        }
    }
    entries
}

pub fn find_device(host_id: cpal::HostId, device_name: &str) -> Option<cpal::Device> {
    let host = cpal::host_from_id(host_id).ok()?;
    host.output_devices()
        .ok()?
        .find(|d| d.name().ok().as_deref() == Some(device_name))
}

pub fn default_device_index(entries: &[DeviceEntry]) -> usize {
    let host = cpal::default_host();
    if let Some(device) = host.default_output_device() {
        if let Ok(name) = device.name() {
            if let Some(idx) = entries.iter().position(|e| e.device_name == name) {
                return idx;
            }
        }
    }
    0
}

// --- Deck audio state ---

pub struct DeckState {
    pub stream: OutputStream,
    pub stream_handle: OutputStreamHandle,
    pub sink: Sink,
    pub file_path: Option<PathBuf>,
    pub duration_secs: f64,
    pub play_started_at: Option<Instant>,
    pub accumulated_secs: f64,
}

impl DeckState {
    pub fn new() -> Self {
        let (stream, stream_handle) = OutputStream::try_default()
            .expect("Failed to open audio output");
        let sink = Sink::try_new(&stream_handle)
            .expect("Failed to create audio sink");
        sink.pause();
        DeckState {
            stream,
            stream_handle,
            sink,
            file_path: None,
            duration_secs: 0.0,
            play_started_at: None,
            accumulated_secs: 0.0,
        }
    }

    pub fn from_device(device: &cpal::Device) -> Result<Self, String> {
        let (stream, stream_handle) = OutputStream::try_from_device(device)
            .map_err(|e| e.to_string())?;
        let sink = Sink::try_new(&stream_handle)
            .map_err(|e| e.to_string())?;
        sink.pause();
        Ok(DeckState {
            stream,
            stream_handle,
            sink,
            file_path: None,
            duration_secs: 0.0,
            play_started_at: None,
            accumulated_secs: 0.0,
        })
    }

    pub fn load(&mut self, path: PathBuf) -> Result<(), String> {
        let new_sink = Sink::try_new(&self.stream_handle)
            .map_err(|e| e.to_string())?;
        new_sink.pause();

        let file = File::open(&path).map_err(|e| e.to_string())?;
        let decoder = Decoder::new(BufReader::new(file))
            .map_err(|e| e.to_string())?;

        self.duration_secs = decoder.total_duration()
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        new_sink.append(decoder);
        self.sink = new_sink;

        self.file_path = Some(path);
        self.accumulated_secs = 0.0;
        self.play_started_at = None;
        Ok(())
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
        if let Some(path) = self.file_path.clone() {
            if let Ok(new_sink) = Sink::try_new(&self.stream_handle) {
                new_sink.pause();
                if let Ok(file) = File::open(&path) {
                    if let Ok(decoder) = Decoder::new(BufReader::new(file)) {
                        new_sink.append(decoder);
                    }
                }
                self.sink = new_sink;
            }
        }
    }

    /// Switch to a different output device, preserving playback position.
    pub fn change_device(&mut self, host_id: cpal::HostId, device_name: &str) -> Result<(), String> {
        let device = find_device(host_id, device_name)
            .ok_or_else(|| format!("Device not found: {}", device_name))?;

        let was_playing = self.play_started_at.is_some();
        let position = self.current_position_secs();
        let file_path = self.file_path.clone();

        let (new_stream, new_handle) = OutputStream::try_from_device(&device)
            .map_err(|e| e.to_string())?;
        let new_sink = Sink::try_new(&new_handle)
            .map_err(|e| e.to_string())?;
        new_sink.pause();

        if let Some(ref path) = file_path {
            if let Ok(file) = File::open(path) {
                if let Ok(decoder) = Decoder::new(BufReader::new(file)) {
                    // Skip to preserved position
                    let positioned = decoder.skip_duration(Duration::from_secs_f64(position));
                    new_sink.append(positioned);
                }
            }
        }

        // Replace resources — sink first so the old stream can safely drop last
        self.sink = new_sink;
        self.stream_handle = new_handle;
        self.stream = new_stream;
        self.accumulated_secs = position;

        if was_playing && file_path.is_some() {
            self.play_started_at = Some(Instant::now());
            self.sink.play();
        } else {
            self.play_started_at = None;
        }

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
