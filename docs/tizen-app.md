# dj-rs Tizen TV App

A Samsung Smart TV receiver app for **dj-rs**, written in Rust/Leptos compiled to WebAssembly.

---

## Architecture Overview

```
Samsung TV (Tizen)               Desktop (dj-rs server)
┌───────────────────────────┐    ┌────────────────────────────┐
│  Tizen Chromium WebView   │    │  HTTP server :7879         │
│  ┌───────────────────┐    │    │                            │
│  │  Leptos 0.7 (CSR) │    │    │  GET  /ping → "dj-rs OK"  │
│  │  compiled to WASM │◄───┼────┼─ WS   /ws   (JSON msgs)   │
│  └───────────────────┘    │    │  GET  /stream/{id}?seek=N  │
│          │                │    │    (FLAC audio stream)     │
│   <audio> element ────────┼────┼──► ffmpeg -ss N -i ...    │
│                           │    └────────────────────────────┘
└───────────────────────────┘

Discovery (on launch):
  TV gets its own IP via WebRTC ICE
  → scans 192.168.x.1-254:7879/ping in parallel (800 ms timeout each)
  → first host that responds "dj-rs" wins
  → opens WebSocket to ws://{host}:7879/ws
```

---

## Why FLAC

FLAC is a lossless audio codec well-suited for a local-network DJ setup:

- **No quality loss** from re-encoding; the source library stays the master.
- **Tizen Chromium supports FLAC** via the `<audio>` element natively (no plugin needed).
- **LAN bandwidth is ample** — a 24-bit/96 kHz stereo FLAC stream is roughly 3–7 Mbit/s,
  well within a 100 Mbit home network.
- ffmpeg can pipe FLAC in real time with arbitrary `-ss` seek positions, enabling the
  seek-by-restart protocol described below.

---

## Discovery Mechanism

### Why not mDNS?

Browser-based web apps cannot send raw mDNS (multicast UDP) queries.
The Web MDNS API is not available on Tizen 3/4/5.
Hence the app performs a **brute-force subnet scan** instead.

### How it works

1. **Local IP detection** — A tiny `RTCPeerConnection` trick (no STUN server required)
   exposes the local candidate, e.g. `192.168.1.42`.
   Source: `index.html` → `window.getLocalIp()`.

2. **Parallel sweep** — The Rust/WASM code in `src/discovery.rs` fires 254 concurrent
   `fetch` requests to `http://192.168.1.{1-254}:7879/ping`, each with an 800 ms timeout.

3. **Fingerprint check** — Any host that responds HTTP 200 with a body containing the
   string `"dj-rs"` is accepted as the desktop server.

4. The first match is used. Subsequent matches (if any) are discarded.

---

## WebSocket Protocol

All messages are JSON objects with a `"type"` discriminant field.

### Desktop → TV

| Message | Fields | Description |
|---------|--------|-------------|
| `metadata` | `title: string`, `artist: string`, `duration: number` | New track loaded |
| `position` | `pos: number` | Playback position in seconds (sent ~1 Hz) |
| `state` | `playing: bool` | Play/pause state change |

Examples:
```json
{"type":"metadata","title":"Blue Lines","artist":"Massive Attack","duration":298.4}
{"type":"position","pos":45.2}
{"type":"state","playing":true}
```

### TV → Desktop

| Message | Fields | Description |
|---------|--------|-------------|
| `seek` | `pos: number` | User clicked seek bar; desktop restarts ffmpeg with `-ss pos` |

Example:
```json
{"type":"seek","pos":123.456}
```

### Seek protocol (seek-by-restart)

When the TV sends a `seek` message the desktop:

1. Kills the running ffmpeg process.
2. Restarts ffmpeg: `ffmpeg -ss {pos} -i {file} -c:a flac -f flac pipe:1`
3. The HTTP `/stream/{id}` handler starts serving the new pipe.
4. The TV `<audio>` element's `src` is updated to `http://{host}:7879/stream/{id}?seek={pos}`
   which forces a new HTTP request to the restarted stream.

---

## Build Instructions

### Prerequisites

```bash
# 1. Install Rust (stable)
curl https://sh.rustup.rs -sSf | sh

# 2. Add the WASM target
rustup target add wasm32-unknown-unknown

# 3. Install trunk
cargo install trunk

# 4. (Optional) Install ImageMagick for icon generation
# Arch:   sudo pacman -S imagemagick
# Ubuntu: sudo apt install imagemagick
```

### Build

```bash
cd tizen-app
./build.sh
```

This produces `tizen-app/dj-rs.wgt`.

For incremental development with hot-reload (on desktop browser):

```bash
cd tizen-app
trunk serve
```

---

## Sideloading on Samsung TV

### 1. Enable Developer Mode on the TV

1. Go to **Home > Apps**.
2. Press **1 2 3 4 5** on the remote — a dialog appears.
3. Toggle Developer mode **ON** and enter your **development PC's IP address**.
4. Restart the TV when prompted.

After restart, a "DEVELOP MODE" banner appears in the Apps screen.

### 2. Install sdb

`sdb` (Smart Development Bridge, analogous to `adb`) ships with **Tizen Studio**.

- Download Tizen Studio: <https://developer.samsung.com/smarttv/develop/getting-started/setting-up-sdk/installing-tv-sdk.html>
- Or grab the standalone CLI tools from the same page.

Add `{tizen-studio}/tools` to your `PATH`.

### 3. Connect to the TV

```bash
sdb connect <TV_IP>
sdb devices          # confirm device appears
```

### 4. Sign the .wgt package

Samsung requires a **Tizen developer certificate** for sideloading.

1. Open **Tizen Studio > Tools > Certificate Manager**.
2. Create a new certificate profile (Samsung account required).
3. Sign the package:

```bash
tizen package -t wgt -s <profile_name> -- dj-rs.wgt
```

### 5. Install on the TV

```bash
tizen install -n dj-rs.wgt -t <device_id>
```

`<device_id>` is the identifier shown by `sdb devices` (e.g. `emulator-26101`).

Alternatively, use **Tizen Studio > Device Manager** GUI: right-click the connected
TV and choose "Install Application".

### 6. Launch

The app appears in **Home > Apps > My Apps** as "dj-rs". Select it to launch.

---

## Known Limitations on Tizen TV

| Limitation | Reason | Workaround |
|------------|--------|------------|
| No mDNS from web app | Browser sandbox blocks multicast UDP | Subnet scan (implemented) |
| WebRTC may be restricted | Depends on Tizen firmware version | Fall back to `null` → show manual IP entry (future) |
| CORS on fetch during discovery | Desktop must send `Access-Control-Allow-Origin: *` on `/ping` | Already required by the desktop server spec |
| No localhost `127.0.0.1` self-skip | The scan will try all 254 addresses | First match wins; TV's own IP returns no server |

---

## Future Work

- **Manual IP entry** fallback when WebRTC returns `null` (Tizen 2.x compatibility).
- **`<audio>` gapless transition** — buffer the next track before the current one ends.
- **Remote control key events** — map Samsung remote D-pad to play/pause/seek using
  the `tizen.tvinputdevice` API (registered keys in `config.xml`).
- **Cover art** — serve artwork from desktop; display in the 400×400 card.
- **Volume control** — WS message `{"type":"volume","level":0.8}` → Tizen system volume API.
