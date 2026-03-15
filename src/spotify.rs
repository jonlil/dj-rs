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

