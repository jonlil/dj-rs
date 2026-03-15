use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::process::Stdio;

use axum::{
    body::Body,
    extract::{Path, Query, State, WebSocketUpgrade},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use axum::extract::ws::{Message, WebSocket};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast;
use tokio_util::io::ReaderStream;

// ── Events sent from desktop → TV ────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WsEvent {
    Metadata { title: String, artist: String, duration: f64 },
    Position  { pos: f64 },
    State     { playing: bool },
    /// Tell the TV to stream audio for a local track, starting at `seek` seconds.
    Stream    { id: i64, seek: f64 },
    /// Tell the TV to fetch the live Spotify stream endpoint.
    Spotifystream,
}

// ── Commands sent from TV → desktop ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum WsCommand {
    Seek { pos: f64 },
}

// ── Shared state inside the Axum server ──────────────────────────────────────

struct AppState {
    events:        broadcast::Sender<WsEvent>,
    seek_slot:     Arc<std::sync::Mutex<Option<f64>>>,
    client_count:  Arc<AtomicUsize>,
    config:        crate::config::Config,
    spotify_audio: broadcast::Sender<Vec<f32>>,
}

// ── Handle exposed to the GTK thread ─────────────────────────────────────────

pub struct ServerBridge {
    pub events:        broadcast::Sender<WsEvent>,
    pub seek_slot:     Arc<std::sync::Mutex<Option<f64>>>,
    pub client_count:  Arc<AtomicUsize>,
    pub spotify_audio: broadcast::Sender<Vec<f32>>,
}

impl ServerBridge {
    /// Broadcast an event to all connected TV clients.
    pub fn send(&self, event: WsEvent) {
        let _ = self.events.send(event);
    }

    /// Take a pending seek position requested by a TV client (if any).
    pub fn take_seek(&self) -> Option<f64> {
        self.seek_slot.lock().unwrap().take()
    }

    /// True when at least one TV client is connected.
    pub fn tv_connected(&self) -> bool {
        self.client_count.load(Ordering::Relaxed) > 0
    }
}

// ── Server startup ────────────────────────────────────────────────────────────

pub fn start_server(
    port: u16,
    config: crate::config::Config,
) -> Arc<ServerBridge> {
    let (tx, _)           = broadcast::channel::<WsEvent>(128);
    let (sp_tx, _)        = broadcast::channel::<Vec<f32>>(1024);
    let seek_slot         = Arc::new(std::sync::Mutex::new(Option::<f64>::None));
    let client_count      = Arc::new(AtomicUsize::new(0));

    let bridge = Arc::new(ServerBridge {
        events:        tx.clone(),
        seek_slot:     seek_slot.clone(),
        client_count:  client_count.clone(),
        spotify_audio: sp_tx.clone(),
    });

    let state = Arc::new(AppState {
        events: tx, seek_slot, client_count, config,
        spotify_audio: sp_tx,
    });

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");

        rt.block_on(async move {
            let app = Router::new()
                .route("/ping",            get(ping))
                .route("/ws",              get(ws_handler))
                .route("/stream/:id",      get(stream_handler))
                .route("/spotify-stream",  get(spotify_stream_handler))
                .with_state(state);

            let addr = format!("0.0.0.0:{port}");
            let listener = tokio::net::TcpListener::bind(&addr)
                .await
                .unwrap_or_else(|e| panic!("Cannot bind {}: {}", addr, e));

            eprintln!("[dj-rs] server listening on {addr}");
            axum::serve(listener, app).await.unwrap();
        });
    });

    bridge
}

// ── Route: /ping ─────────────────────────────────────────────────────────────

async fn ping() -> impl IntoResponse {
    (
        [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
        axum::Json(serde_json::json!({ "service": "dj-rs" })),
    )
}

// ── Route: /ws ────────────────────────────────────────────────────────────────

async fn ws_handler(
    ws:             WebSocketUpgrade,
    State(state):   State<Arc<AppState>>,
) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    state.client_count.fetch_add(1, Ordering::Relaxed);
    let mut rx = state.events.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            if socket.send(Message::Text(json)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(WsCommand::Seek { pos }) = serde_json::from_str::<WsCommand>(&text) {
                            *state.seek_slot.lock().unwrap() = Some(pos);
                        }
                    }
                    None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }

    state.client_count.fetch_sub(1, Ordering::Relaxed);
}

// ── Route: /stream/:id?seek=N ────────────────────────────────────────────────

#[derive(Deserialize)]
struct StreamParams {
    seek: Option<f64>,
}

async fn stream_handler(
    Path(id):       Path<i64>,
    Query(params):  Query<StreamParams>,
    State(state):   State<Arc<AppState>>,
) -> Response {
    let seek = params.seek.unwrap_or(0.0);
    eprintln!("[stream] request id={id} seek={seek}");

    let file_path = match resolve_track_path(&state, id) {
        Some(p) => p,
        None => {
            eprintln!("[stream] track not found for id={id}");
            return (StatusCode::NOT_FOUND, "Track not found").into_response();
        }
    };

    let mut child = match tokio::process::Command::new("ffmpeg")
        .args([
            "-ss",        &format!("{seek:.3}"),
            "-i",         &file_path,
            "-c:a",       "aac",
            "-b:a",       "256k",
            "-f",         "adts",
            "-loglevel",  "error",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c)  => c,
        Err(e) => {
            eprintln!("[dj-rs] ffmpeg spawn failed: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "ffmpeg unavailable").into_response();
        }
    };

    let stdout = child.stdout.take().unwrap();
    tokio::spawn(async move { let _ = child.wait().await; });

    (
        [
            (header::CONTENT_TYPE,                    "audio/aac"),
            (header::CACHE_CONTROL,                   "no-cache"),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN,     "*"),
        ],
        Body::from_stream(ReaderStream::new(stdout)),
    )
        .into_response()
}

fn resolve_track_path(state: &AppState, id: i64) -> Option<String> {
    let db_path = state.config.resolved_db_path()?;
    let lib = crate::rekordbox::Library::open(&db_path).ok()?;
    let raw = lib.track_file_path(id)?;
    let mapped = state.config.apply_mappings(&raw);
    eprintln!("[stream] id={id} raw={raw:?} mapped={mapped:?}");
    Some(mapped)
}

// ── Route: /spotify-stream  (live librespot audio → AAC) ─────────────────────

async fn spotify_stream_handler(
    State(state): State<Arc<AppState>>,
) -> Response {
    let mut rx = state.spotify_audio.subscribe();

    let mut child = match tokio::process::Command::new("ffmpeg")
        .args([
            "-f",        "f32le",
            "-ar",       "44100",
            "-ac",       "2",
            "-i",        "pipe:0",
            "-c:a",      "aac",
            "-b:a",      "192k",
            "-f",        "adts",
            "-loglevel", "error",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c)  => c,
        Err(e) => {
            eprintln!("[dj-rs] ffmpeg spawn for spotify stream failed: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "ffmpeg unavailable").into_response();
        }
    };

    let mut stdin  = child.stdin.take().unwrap();
    let stdout     = child.stdout.take().unwrap();
    tokio::spawn(async move { let _ = child.wait().await; });

    // Pipe librespot f32 samples → ffmpeg stdin
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(samples) => {
                    let bytes: Vec<u8> = samples.iter()
                        .flat_map(|&s: &f32| s.to_le_bytes())
                        .collect();
                    if stdin.write_all(&bytes).await.is_err() { break; }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    (
        [
            (header::CONTENT_TYPE,                "audio/aac"),
            (header::CACHE_CONTROL,               "no-cache"),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
        ],
        Body::from_stream(ReaderStream::new(stdout)),
    )
        .into_response()
}
