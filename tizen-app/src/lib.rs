mod discovery;

use discovery::find_desktop;
use gloo_timers::future::IntervalStream;
use futures::StreamExt;
use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

// ── JS WebSocket bridge (declared in index.html) ─────────────────────────────

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = wsConnect)]
    fn ws_connect(url: &str);

    #[wasm_bindgen(js_name = wsSend)]
    fn ws_send(msg: &str);

    #[wasm_bindgen(js_name = wsState)]
    fn ws_state() -> i32;

    #[wasm_bindgen(js_name = wsOnOpen)]
    fn ws_on_open(cb: &Closure<dyn Fn()>);

    #[wasm_bindgen(js_name = wsOnClose)]
    fn ws_on_close(cb: &Closure<dyn Fn()>);

    #[wasm_bindgen(js_name = wsOnMessage)]
    fn ws_on_message(cb: &Closure<dyn Fn(String)>);
}

// ── Domain types ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum ConnectionStatus {
    Scanning,
    Connected(String),
    Disconnected,
}

impl ConnectionStatus {
    fn label(&self) -> &'static str {
        match self {
            ConnectionStatus::Scanning => "Scanning…",
            ConnectionStatus::Connected(_) => "Connected",
            ConnectionStatus::Disconnected => "Disconnected",
        }
    }

    fn css_class(&self) -> &'static str {
        match self {
            ConnectionStatus::Connected(_) => "connected",
            ConnectionStatus::Disconnected => "disconnected",
            ConnectionStatus::Scanning => "",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct TrackInfo {
    pub title: String,
    pub artist: String,
    pub duration: f64,
}

// ── Incoming WebSocket message types ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum WsMessage {
    Metadata {
        title: String,
        artist: String,
        duration: f64,
    },
    Position {
        pos: f64,
    },
    State {
        playing: bool,
    },
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn format_time(secs: f64) -> String {
    let s = secs as u64;
    let m = s / 60;
    let s = s % 60;
    format!("{:02}:{:02}", m, s)
}

fn seek_percent(position: f64, duration: f64) -> f64 {
    if duration > 0.0 {
        (position / duration * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    }
}

// ── Root component ────────────────────────────────────────────────────────────

#[component]
pub fn App() -> impl IntoView {
    let connection_status = RwSignal::new(ConnectionStatus::Scanning);
    let track = RwSignal::new(Option::<TrackInfo>::None);
    let position = RwSignal::new(0.0f64);
    let duration = RwSignal::new(0.0f64);
    let playing = RwSignal::new(false);

    // ── Discovery + WebSocket setup on mount ─────────────────────────────────
    Effect::new(move |_| {
        spawn_local(async move {
            connection_status.set(ConnectionStatus::Scanning);

            let base_url = match find_desktop().await {
                Some(url) => url,
                None => {
                    connection_status.set(ConnectionStatus::Disconnected);
                    return;
                }
            };

            let ws_url = base_url
                .replacen("http://", "ws://", 1)
                .replacen("https://", "wss://", 1)
                + "/ws";

            // Register message handler before connecting so we don't miss
            // the first frame.
            let on_message = Closure::<dyn Fn(String)>::new(move |raw: String| {
                if let Ok(msg) = serde_json::from_str::<WsMessage>(&raw) {
                    match msg {
                        WsMessage::Metadata {
                            title,
                            artist,
                            duration: d,
                        } => {
                            track.set(Some(TrackInfo {
                                title,
                                artist,
                                duration: d,
                            }));
                            duration.set(d);
                        }
                        WsMessage::Position { pos } => {
                            position.set(pos);
                        }
                        WsMessage::State { playing: p } => {
                            playing.set(p);
                        }
                    }
                }
            });
            ws_on_message(&on_message);
            on_message.forget();

            let base_for_open = base_url.clone();
            let on_open = Closure::<dyn Fn()>::new(move || {
                connection_status.set(ConnectionStatus::Connected(base_for_open.clone()));
            });
            ws_on_open(&on_open);
            on_open.forget();

            let on_close = Closure::<dyn Fn()>::new(move || {
                connection_status.set(ConnectionStatus::Disconnected);
            });
            ws_on_close(&on_close);
            on_close.forget();

            ws_connect(&ws_url);
        });
    });

    // ── Position ticker (every 1 s) when playing ─────────────────────────────
    Effect::new(move |_| {
        if playing.get() {
            spawn_local(async move {
                let mut stream = IntervalStream::new(1_000);
                while let Some(_) = stream.next().await {
                    // Stop ticking if no longer playing or ws disconnected
                    if !playing.get() || ws_state() != 1 {
                        break;
                    }
                    position.update(|p| {
                        let dur = duration.get_untracked();
                        if dur > 0.0 {
                            *p = (*p + 1.0).min(dur);
                        }
                    });
                }
            });
        }
    });

    // ── Seek handler ─────────────────────────────────────────────────────────
    let handle_seek = move |evt: web_sys::MouseEvent| {
        let target: web_sys::EventTarget = evt.target().unwrap();
        let el: web_sys::Element = target.unchecked_into();
        let rect = el.get_bounding_client_rect();
        let frac = (evt.client_x() as f64 - rect.left()) / rect.width();
        let dur = duration.get_untracked();
        if dur > 0.0 {
            let seek_pos = (frac * dur).clamp(0.0, dur);
            position.set(seek_pos);
            let msg = format!(r#"{{"type":"seek","pos":{:.3}}}"#, seek_pos);
            ws_send(&msg);
        }
    };

    // ── Derived display values ────────────────────────────────────────────────
    let pct = move || seek_percent(position.get(), duration.get());
    let current_time_str = move || format_time(position.get());
    let total_time_str = move || format_time(duration.get());

    let title_text = move || {
        track
            .get()
            .map(|t| t.title)
            .unwrap_or_else(|| "No track loaded".to_string())
    };
    let artist_text = move || {
        track
            .get()
            .map(|t| t.artist)
            .unwrap_or_else(|| "—".to_string())
    };

    let scanning = move || connection_status.get() == ConnectionStatus::Scanning;
    let status_class = move || connection_status.get().css_class();
    let status_label = move || connection_status.get().label().to_string();
    let connected_host = move || match connection_status.get() {
        ConnectionStatus::Connected(h) => h,
        _ => String::new(),
    };

    let thumb_left = move || format!("{}%", pct());
    let progress_width = move || format!("{}%", pct());

    view! {
        // ── Scanning overlay ────────────────────────────────────────────────
        <div id="scanning-overlay" class:hidden=move || !scanning()>
            <div id="scanning-logo">"dj-rs"</div>
            <div class="spinner"></div>
            <p id="scanning-message">
                "Scanning for dj-rs on local network…"
                <br/>
                <small style="font-size:0.7rem;color:#535353">
                    "(checking 192.168.x.1-254 on port 7879)"
                </small>
            </p>
        </div>

        // ── Main player UI ──────────────────────────────────────────────────
        <div id="top-bar">
            <span id="logo">"dj-rs"</span>
            <div id="connection-status" class=status_class>
                <span class="dot"></span>
                <span>{status_label}</span>
                <span style="font-size:0.7rem;color:#535353">{connected_host}</span>
            </div>
        </div>

        <div id="main">
            <div id="cover-art">
                // Placeholder music-note SVG
                <svg width="80" height="80" viewBox="0 0 24 24" fill="white">
                    <path d="M12 3v10.55A4 4 0 1 0 14 17V7h4V3z"/>
                </svg>
            </div>

            <div id="track-info">
                <div id="track-title">{title_text}</div>
                <div id="track-artist">{artist_text}</div>
            </div>
        </div>

        <div id="bottom-bar">
            <div id="seek-bar-container">
                <span id="time-current">{current_time_str}</span>
                <div id="seek-bar-track" on:click=handle_seek>
                    <div
                        id="seek-bar-progress"
                        style:width=progress_width
                    ></div>
                    <div
                        id="seek-bar-thumb"
                        style:left=thumb_left
                    ></div>
                </div>
                <span id="time-total">{total_time_str}</span>
            </div>

            <div id="playback-state">
                <div id="play-icon">
                    {move || if playing.get() {
                        // Pause icon (two bars)
                        view! {
                            <svg width="40" height="40" viewBox="0 0 24 24" fill="currentColor">
                                <rect x="6" y="4" width="4" height="16"/>
                                <rect x="14" y="4" width="4" height="16"/>
                            </svg>
                        }.into_any()
                    } else {
                        // Play icon (triangle)
                        view! {
                            <svg width="40" height="40" viewBox="0 0 24 24" fill="currentColor">
                                <polygon points="5,3 19,12 5,21"/>
                            </svg>
                        }.into_any()
                    }}
                </div>
            </div>
        </div>
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook_setup();
    leptos::mount::mount_to_body(App);
}

fn console_error_panic_hook_setup() {
    use wasm_bindgen::JsValue;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info.to_string();
        web_sys::console::error_1(&JsValue::from_str(&msg));
        // Also show on screen so we can see it on the TV
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            if let Some(el) = doc.get_element_by_id("status-msg") {
                let _ = el.set_attribute("style", "color:red;font-size:28px;position:fixed;top:10px;left:10px;right:10px;z-index:9999;background:#000;padding:20px;word-break:break-all");
                el.set_text_content(Some(&msg));
            }
        }
        hook(info);
    }));
}
