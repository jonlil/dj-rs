use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

pub const CLIENT_ID: &str = "40c148a5aa614c38b6032a73ba2f030f";
const REDIRECT_URI: &str = "http://127.0.0.1:8888/callback";
const SCOPES: &str = "streaming playlist-read-private playlist-read-collaborative user-modify-playback-state user-read-playback-state";

// ── PKCE helpers ─────────────────────────────────────────────────────────────

fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 64];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn code_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

// ── OAuth flow ────────────────────────────────────────────────────────────────

/// Opens the browser for Spotify PKCE auth and blocks until the callback is
/// received on localhost:8888. Returns (access_token, refresh_token).
pub fn authorize() -> Result<(String, String), String> {
    let verifier  = generate_code_verifier();
    let challenge = code_challenge(&verifier);

    let auth_url = format!(
        "https://accounts.spotify.com/authorize\
         ?client_id={}\
         &response_type=code\
         &redirect_uri={}\
         &scope={}\
         &code_challenge_method=S256\
         &code_challenge={}",
        CLIENT_ID,
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(SCOPES),
        challenge,
    );

    webbrowser::open(&auth_url).map_err(|e| format!("Failed to open browser: {e}"))?;

    // Listen for the callback
    let code = wait_for_callback()?;

    // Exchange code for tokens
    exchange_code(&code, &verifier)
}

/// Refreshes the access token using the stored refresh token.
pub fn refresh(refresh_token: &str) -> Result<(String, Option<String>), String> {
    let client = Client::new();
    let mut params = HashMap::new();
    params.insert("grant_type",    "refresh_token");
    params.insert("refresh_token", refresh_token);
    params.insert("client_id",     CLIENT_ID);

    let resp: TokenResponse = client
        .post("https://accounts.spotify.com/api/token")
        .form(&params)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    Ok((resp.access_token, resp.refresh_token))
}

fn exchange_code(code: &str, verifier: &str) -> Result<(String, String), String> {
    let client = Client::new();
    let mut params = HashMap::new();
    params.insert("grant_type",    "authorization_code");
    params.insert("code",          code);
    params.insert("redirect_uri",  REDIRECT_URI);
    params.insert("client_id",     CLIENT_ID);
    params.insert("code_verifier", verifier);

    let resp: TokenResponse = client
        .post("https://accounts.spotify.com/api/token")
        .form(&params)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    let refresh = resp.refresh_token
        .ok_or_else(|| "Spotify did not return a refresh token".to_string())?;

    Ok((resp.access_token, refresh))
}

/// Spins up a temporary TCP listener on port 8888 and waits for Spotify's
/// redirect, returning the `code` query parameter.
fn wait_for_callback() -> Result<String, String> {
    let listener = TcpListener::bind("127.0.0.1:8888")
        .map_err(|e| format!("Could not bind 127.0.0.1:8888: {e}"))?;

    let (stream, _) = listener.accept()
        .map_err(|e| format!("Accept failed: {e}"))?;

    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)
        .map_err(|e| format!("Read failed: {e}"))?;

    // Send a minimal HTML response so the browser shows something friendly
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
        <html><body><h2>Spotify connected!</h2><p>You can close this tab.</p></body></html>";
    let mut writer: &std::net::TcpStream = &stream;
    let _ = writer.write_all(response);

    // Parse code from "GET /callback?code=xxx&state=yyy HTTP/1.1"
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or("Malformed HTTP request")?;

    let query = path.split('?').nth(1).unwrap_or("");
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        if kv.next() == Some("code") {
            return kv.next()
                .map(|s| s.to_string())
                .ok_or_else(|| "Empty code in callback".to_string());
        }
    }

    Err("No code found in Spotify callback".to_string())
}

// ── Token response ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TokenResponse {
    access_token:  String,
    refresh_token: Option<String>,
}

// ── Spotify API types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpotifyTrack {
    pub spotify_id:  String,
    pub title:       String,
    pub artist:      String,   // primary artist only
    pub duration_ms: u32,
    pub preview_url: Option<String>,
}

#[derive(Deserialize)]
struct PlaylistTracksPage {
    items: Vec<PlaylistItem>,
    next:  Option<String>,
}

#[derive(Deserialize)]
struct PlaylistItem {
    track: Option<TrackObject>,
}

#[derive(Deserialize)]
struct TrackObject {
    id:           Option<String>,  // null for local tracks
    name:         String,
    duration_ms:  u32,
    artists:      Vec<ArtistObject>,
    preview_url:  Option<String>,
}

#[derive(Deserialize)]
struct ArtistObject {
    name: String,
}

// ── User playlists ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserPlaylist {
    pub id: String,
    pub name: String,
    pub track_count: u32,
    pub owner: String,
}

#[derive(Deserialize)]
struct PlaylistsPage {
    items: Vec<PlaylistObject>,
    next:  Option<String>,
}

#[derive(Deserialize)]
struct PlaylistObject {
    id:     String,
    name:   String,
    tracks: PlaylistTracksRef,
    owner:  PlaylistOwner,
}

#[derive(Deserialize)]
struct PlaylistTracksRef {
    total: u32,
}

#[derive(Deserialize)]
struct PlaylistOwner {
    display_name: Option<String>,
}

/// Fetches the authenticated user's playlists.
pub fn fetch_user_playlists(access_token: &str) -> Result<Vec<UserPlaylist>, String> {
    let client = Client::new();
    let mut playlists = Vec::new();
    let mut url = "https://api.spotify.com/v1/me/playlists?limit=50".to_string();

    loop {
        let resp = client
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .map_err(|e| e.to_string())?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            return Err(format!("Spotify API error {}: {}", status, body));
        }

        let page: PlaylistsPage = resp.json().map_err(|e| e.to_string())?;

        for pl in page.items {
            playlists.push(UserPlaylist {
                id: pl.id,
                name: pl.name,
                track_count: pl.tracks.total,
                owner: pl.owner.display_name.unwrap_or_default(),
            });
        }

        match page.next {
            Some(next_url) => url = next_url,
            None => break,
        }
    }

    Ok(playlists)
}

// ── Playlist fetch ────────────────────────────────────────────────────────────

/// Fetches all tracks from a Spotify playlist URL.
/// `playlist_url` can be a full URL or just the playlist ID.
pub fn fetch_playlist(access_token: &str, playlist_url: &str) -> Result<Vec<SpotifyTrack>, String> {
    let playlist_id = extract_playlist_id(playlist_url)?;
    let client = Client::new();
    let mut tracks = Vec::new();
    let mut url = format!(
        "https://api.spotify.com/v1/playlists/{}/tracks?limit=100&fields=items(track(id,name,duration_ms,artists,preview_url)),next",
        playlist_id
    );

    loop {
        let resp = client
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .map_err(|e| e.to_string())?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            return Err(format!("Spotify API error {}: {}", status, body));
        }

        let page: PlaylistTracksPage = resp.json().map_err(|e| e.to_string())?;

        for item in page.items {
            if let Some(track) = item.track {
                // Skip local tracks (no Spotify ID)
                let spotify_id = match track.id {
                    Some(id) => id,
                    None     => continue,
                };
                let artist = track.artists
                    .into_iter()
                    .next()
                    .map(|a| a.name)
                    .unwrap_or_default();

                tracks.push(SpotifyTrack {
                    spotify_id,
                    title:       track.name,
                    artist,
                    duration_ms: track.duration_ms,
                    preview_url: track.preview_url,
                });
            }
        }

        match page.next {
            Some(next_url) => url = next_url,
            None => break,
        }
    }

    Ok(tracks)
}

fn extract_playlist_id(input: &str) -> Result<String, String> {
    // Handles:
    //   https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=...
    //   spotify:playlist:37i9dQZF1DXcBWIGoYBM5M
    //   37i9dQZF1DXcBWIGoYBM5M  (bare ID)
    if let Some(pos) = input.find("/playlist/") {
        let rest = &input[pos + "/playlist/".len()..];
        return Ok(rest.split('?').next().unwrap_or(rest).to_string());
    }
    if input.starts_with("spotify:playlist:") {
        return Ok(input["spotify:playlist:".len()..].to_string());
    }
    if input.chars().all(|c| c.is_alphanumeric()) {
        return Ok(input.to_string());
    }
    Err(format!("Could not extract playlist ID from: {input}"))
}

// ── Track image ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TrackImageResponse {
    album: AlbumImages,
}

#[derive(Deserialize)]
struct AlbumImages {
    images: Vec<ImageObject>,
}

#[derive(Deserialize)]
struct ImageObject {
    url: String,
}

/// Returns the URL of the best-fit album art (prefers ≤300 px, falls back to largest).
pub fn fetch_track_image_url(access_token: &str, track_id: &str) -> Option<String> {
    let client = Client::new();
    let resp = client
        .get(&format!("https://api.spotify.com/v1/tracks/{track_id}"))
        .bearer_auth(access_token)
        .send().ok()?
        .json::<TrackImageResponse>().ok()?;

    // Spotify returns images largest-first; take the last one that is still
    // at least 150px so it looks sharp at 80×80 (retina). Fall back to first.
    let images = resp.album.images;
    images.last().or_else(|| images.first()).map(|i| i.url.clone())
}

// ── Audio features (BPM + key) ────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct AudioFeatures {
    pub tempo: f32,
    pub key:   i32,   // pitch class 0–11, -1 = unknown
    pub mode:  i32,   // 0 = minor, 1 = major
}

pub fn fetch_audio_features(access_token: &str, track_id: &str) -> Option<AudioFeatures> {
    Client::new()
        .get(&format!("https://api.spotify.com/v1/audio-features/{track_id}"))
        .bearer_auth(access_token)
        .send().ok()?
        .json::<AudioFeatures>().ok()
}

/// Convert Spotify pitch class + mode to Camelot notation (e.g. "8A", "9B").
pub fn to_camelot(key: i32, mode: i32) -> String {
    if !(0..=11).contains(&key) { return String::new(); }
    // pitch class: 0=C, 1=C#, 2=D, 3=D#, 4=E, 5=F, 6=F#, 7=G, 8=G#, 9=A, 10=A#, 11=B
    const MAJOR: [&str; 12] = ["8B","3B","10B","5B","12B","7B","2B","9B","4B","11B","6B","1B"];
    const MINOR: [&str; 12] = ["5A","12A","7A","2A","9A","4A","11A","6A","1A","8A","3A","10A"];
    if mode == 1 { MAJOR[key as usize].to_string() }
    else          { MINOR[key as usize].to_string() }
}

// ── Audio analysis → waveform ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct AudioAnalysisResponse {
    segments: Vec<AnalysisSegment>,
    track:    AnalysisTrackMeta,
}

#[derive(Deserialize)]
struct AnalysisTrackMeta {
    duration: f64,
}

#[derive(Deserialize)]
struct AnalysisSegment {
    start:        f64,
    loudness_max: f32,
    timbre:       Vec<f32>,
}

pub struct SpotifyWaveform {
    /// PWV7: 3 bytes/col (bass, mid, high). lower 5 bits = height 0–31.
    pub color:    Vec<u8>,
    /// PWAV: 1 byte/col, same encoding.
    pub overview: Vec<u8>,
    pub duration: f64,
}

/// Fetch audio analysis and build PWV7/PWAV waveform buffers.
pub fn fetch_audio_analysis(access_token: &str, track_id: &str) -> Option<SpotifyWaveform> {
    let resp: AudioAnalysisResponse = Client::new()
        .get(&format!("https://api.spotify.com/v1/audio-analysis/{track_id}"))
        .bearer_auth(access_token)
        .send().ok()?
        .json().ok()?;

    let duration = resp.track.duration;
    let segments = resp.segments;
    if segments.is_empty() || duration <= 0.0 { return None; }

    // ~10 columns/second, capped to 10 000
    let n_cols = ((duration * 10.0) as usize).clamp(100, 10_000);
    let secs_per_col = duration / n_cols as f64;

    // Pre-build sorted start-time list for binary search
    let starts: Vec<f64> = segments.iter().map(|s| s.start).collect();

    let mut color    = Vec::with_capacity(n_cols * 3);
    let mut overview = Vec::with_capacity(n_cols);

    for col in 0..n_cols {
        let t   = col as f64 * secs_per_col;
        let idx = starts.partition_point(|&s| s <= t).saturating_sub(1);
        let seg = &segments[idx];

        // Normalise loudness_max from dB (−60..0) → 0..1
        let energy = ((seg.loudness_max + 60.0) / 60.0).clamp(0.0, 1.0);

        // timbre[1] ≈ spectral brightness (range ~−100..+100)
        let brightness = seg.timbre.get(1)
            .map(|&v| ((v + 100.0) / 200.0).clamp(0.0, 1.0))
            .unwrap_or(0.5);

        let bass_h = (energy              * 31.0) as u8;
        let mid_h  = (energy * 0.75       * 31.0) as u8;
        let high_h = (energy * brightness  * 31.0) as u8;

        color.push(bass_h & 0x1F);
        color.push(mid_h  & 0x1F);
        color.push(high_h & 0x1F);
        overview.push(bass_h & 0x1F);
    }

    Some(SpotifyWaveform { color, overview, duration })
}

