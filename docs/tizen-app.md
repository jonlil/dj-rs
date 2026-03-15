# dj-rs Tizen TV App

A Samsung Smart TV receiver app for **dj-rs**, written as a single plain HTML/JS file.

---

## Architecture Overview

```
Samsung TV (Tizen)               Desktop (dj-rs server)
┌───────────────────────────┐    ┌────────────────────────────┐
│  Tizen Chromium WebView   │    │  HTTP server :7879         │
│  app.html (plain JS)      │    │                            │
│                      ◄────┼────┼─ WS   /ws   (JSON msgs)   │
│  <audio> element ─────────┼────┼──► GET /stream/{id}?seek=N │
│                       ────┼────┼──► GET /spotify-stream     │
└───────────────────────────┘    └────────────────────────────┘

Discovery (on launch):
  Scans 192.168.1.1-254:7879/ping in parallel (800 ms timeout each)
  → first host that responds with body containing "dj-rs" wins
  → opens WebSocket to ws://{host}:7879/ws
```

---

## Files

| File | Purpose |
|------|---------|
| `app.html` | The entire app — HTML, CSS, and JS in one file |
| `build.sh` | Packages `app.html` into a signed `.wgt` for sideloading |
| `public/config.xml` | Tizen app manifest (id, name, icons, permissions) |
| `.env.tizen` | Local cert credentials (not committed); sets `CERT_PROFILE` |

---

## Discovery Mechanism

The app performs a **brute-force subnet scan** since browser-based apps cannot send mDNS queries on Tizen.

1. Fires 254 concurrent `XMLHttpRequest` calls to `http://192.168.1.{1-254}:7879/ping`, each with an 800 ms timeout.
2. First host that returns HTTP 200 with `"dj-rs"` in the body is used.
3. Opens a WebSocket to `ws://{host}:7879/ws`.
4. If no host is found, retries after 5 seconds.

---

## WebSocket Protocol

All messages are JSON objects with a `"type"` discriminant field.

### Desktop → TV

| Message | Fields | Description |
|---------|--------|-------------|
| `metadata` | `title`, `artist`, `duration` | New track loaded |
| `position` | `pos: number` | Playback position in seconds |
| `state` | `playing: bool` | Play/pause state change |
| `stream` | `id: i64`, `seek: f64` | Play a local rekordbox track from the given position |
| `spotifystream` | — | Switch audio to the live Spotify stream |

### TV → Desktop

| Message | Fields | Description |
|---------|--------|-------------|
| `seek` | `pos: number` | User clicked seek bar |

---

## Audio Playback

Two stream types are supported via a hidden `<audio id="player">` element:

- **Local track**: `GET /stream/{id}?seek={pos}` — ffmpeg pipes the file as AAC ADTS
- **Spotify**: `GET /spotify-stream` — live PCM from librespot → ffmpeg → AAC ADTS

When the browser blocks autoplay, a tap-to-play overlay is shown. Any click or remote key press dismisses it and starts playback.

---

## Build & Sideload

### Prerequisites

- Tizen Studio installed at `~/tizen-studio` (for the `tizen` CLI and certificate manager)
- A Tizen developer certificate profile configured in Certificate Manager
- Certificate profile name exported in `.env.tizen`:
  ```
  CERT_PROFILE=your-profile-name
  ```

### Build

```bash
cd tizen-app
./build.sh
```

Produces `tizen-app/dj-rs.wgt`.

### Sideload on Samsung TV

1. **Enable Developer Mode** on the TV: Home → Apps → press `1 2 3 4 5` → toggle Developer mode ON, enter your PC's IP, restart.

2. **Connect via sdb**:
   ```bash
   sdb connect <TV_IP>
   sdb devices
   ```

3. **Install**:
   ```bash
   tizen install -n dj-rs.wgt -t <device_id>
   ```

The app appears in **Home → Apps → My Apps** as "dj-rs".

---

## Known Limitations

| Limitation | Reason | Workaround |
|------------|--------|------------|
| Fixed subnet `192.168.1.x` | JS can't do mDNS | Hardcoded; edit `app.html` if subnet differs |
| Autoplay may be blocked | Browser autoplay policy | Tap-to-play overlay handles this |
| No remote key navigation | Not implemented | Click/OK key dismisses the play overlay |
