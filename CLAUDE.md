# dj-rs — Claude context

## What this project is
A GTK3 desktop DJ application in Rust. Core jobs:
- Play local audio from a Rekordbox library (rodio)
- Display CDJ-style waveforms from Rekordbox ANLZ analysis files
- Manage gigs/events, match Spotify playlists against the local library
- Stream audio to a Samsung TV at 192.168.1.44 via WebSocket (Tizen sideload)

---

## File layout
```
src/
  main.rs               — App entry, top-level Widgets struct, menu bar
  config.rs             — Config (JSON), path mappings, anlz_base_dir()
  deck.rs               — DeckState: rodio Sink + play/pause/seek/loop
  rekordbox.rs          — SQLCipher reader for master.db + ANLZ binary parser
  gig.rs                — Gig, Contact, GigStore (gigs.json persistence)
  spotify.rs            — PKCE OAuth, fetch_playlist, Spotify Web API
  librespot_player.rs   — Full Spotify playback via librespot → rodio
  matcher.rs            — Jaro-Winkler fuzzy match: Spotify tracks ↔ library
  server.rs             — axum HTTP/WS server (port 7879), TV stream via ffmpeg
  dlna.rs               — DLNA renderer discovery
  tags.rs               — lofty tag read/write (ISRC, MusicBrainz, AcoustID)
  bin/
    dj_iced.rs          — iced app entry point
    ui/
      mod.rs            — App struct, Message enum, update(), view(), subscriptions
      browser.rs        — Icon bar, sidebar tree, track list, Spotify browser
      player.rs         — PlayerState (display), overview+zoomed waveform canvases
      contact.rs        — ContactState + contact detail view
      gig.rs            — GigState + gig detail view with Spotify match UI
      settings.rs       — SettingsState + path mappings / Spotify OAuth view
      theme.rs          — Color constants, layout dimensions
  views/
    mod.rs              — PlayerView + MainView (all GTK wiring, ~3400 lines)
    browser.rs          — Library/playlist browser tree + track list
    gig_workspace.rs    — Gig editor (5-tab Notebook)
    gig_sidebar.rs      — Contacts/Gigs sidebar tree
    contact_view.rs     — Contact detail + pool playlists
    dialogs.rs          — Settings dialog, new-gig dialog
    utils.rs            — find_widget helper
```

---

## Standard data paths (Linux/XDG)
| Purpose | Path |
|---|---|
| Config | `~/.config/dj-rs/config.json` |
| Rekordbox DB | `~/.local/share/dj-rs/master.db` (SQLCipher) |
| ANLZ waveform files | `~/.local/share/dj-rs/PIONEER/USBANLZ/…` |
| Gig data | `~/.config/dj-rs/gigs.json` |
| Music (mapped) | `~/Music/` |

---

## PlayerView — UI layout (src/views/mod.rs)

### Widget tree (top → bottom inside Frame)
```
Frame("Player")
  VBox
    info_row (HBox)
      art_image           48×48 px, hidden when no art loaded
      meta_box (VBox)
        title_row         track_label | source_badge | bpm_label
        artist_row        artist_label | key_label | time_label
    main_hbox (HBox)
      left_col (VBox, 90px)
        cue_btn           round (border-radius 22px), label "CUE"
        play_btn          round, label "▶" / "❚❚"
        out_label         "Out"
        out_row (HBox)    [● Loc] [TV]  — mutually exclusive toggles
      center_col (VBox, hexpand)
        zoomed_area       DrawingArea 120px tall — CDJ zoomed waveform
        overview_area     DrawingArea  32px tall — full-track overview + seek
      cue_list_box (VBox, 130px)
        8 × row_btn       colored letter (A–H) + timestamp; click to seek
    cue_loop_row          [A][B][C][D][E][F][G][H]  hot cue trigger buttons
    convert_btn           hidden; shown only for M4A/AAC tracks
    error_label           hidden; shown on load errors
```

### Waveform shared state (Rc<Cell<>> / Rc<RefCell<>>)
| Variable | Type | Purpose |
|---|---|---|
| `waveform_pos_secs` | `Rc<Cell<f64>>` | Current playhead position in seconds |
| `waveform_dur` | `Rc<Cell<f64>>` | Track duration in seconds |
| `seeking` | `Rc<Cell<bool>>` | True while user is scrubbing — suppresses timer position updates |
| `waveform_cues` | `Rc<RefCell<Vec<(f64, Option<f64>, usize)>>>` | (in_secs, out_secs, slot 1–8) |
| `color_waveform` | `Rc<RefCell<Option<Vec<u8>>>>` | PWV7 data: 3 bytes/col (bass, mid, high) |
| `overview_waveform` | `Rc<RefCell<Option<Vec<u8>>>>` | PWAV data: 1 byte/col (monochrome) |
| `waveform_dragging` | `Rc<Cell<bool>>` | True during zoomed CDJ scratch drag |
| `scratch_was_playing` | `Rc<Cell<bool>>` | Was deck playing when zoomed drag started |
| `scratch_start_x` | `Rc<Cell<f64>>` | Pixel X at drag start |
| `scratch_start_pos` | `Rc<Cell<f64>>` | Track position at drag start |

### Zoomed waveform (zoomed_area)
- **Height**: 120px, `hexpand = true`
- **Window**: 6 seconds total visible (`ZOOM_WINDOW = 6.0`)
- **Playhead**: 25% from left edge (`PLAYHEAD_FRAC = 0.25`) — white vertical line + red downward triangle
- **Data source**: `color_waveform` — PWV7 bytes from ANLZ `.2EX` file
- **Rendering**: column iteration (not per-pixel). Each column draws 3 overlapping rectangles (bottom→top: bass, mid, high):
  - Bass: warm amber → white (`upper 3 bits` = whiteness 0–7)
  - Mid: lime green → white, 80% of bass height
  - High: steel blue → white, 60% of bass height
  - Byte encoding: `lower 5 bits` = height 0–31, `upper 3 bits` = whiteness 0–7
- **Cue markers**: colored vertical lines per hot-cue slot; loop regions as tinted rectangles
- **Interaction** (CDJ scratch style):
  - `button_press` → pause deck, record `(start_x, start_pos)`, set `dragging=true`, `seeking=true`
  - `motion_notify` (BUTTON1_MOTION_MASK, guarded by `dragging`) → `seek_to(start_pos + Δx / px_per_s)`
  - `button_release` → clear `dragging`, clear `seeking`, resume if `scratch_was_playing`

### Overview waveform (overview_area)
- **Height**: 32px, `hexpand = true`
- **Data source**: `overview_waveform` — PWAV bytes from ANLZ `.DAT` file (PWV7 color used for pre-rendered surface)
- **Rendering**: pre-rendered to a `cairo::ImageSurface` once per track load (3-band color, same palette as zoomed). Blit the surface each frame, then overlay: darker region left of playhead, white position marker + red triangle, colored cue ticks
- **Fallback**: grey center line (0.5 alpha) when no waveform data; position marker always drawn
- **Interaction**:
  - `button_press` → set `seeking=true`, visual update only
  - `motion_notify` (BUTTON1_MASK) → visual update only
  - `button_release` → `seek_to(frac * dur)`, debounced `WsEvent::Stream` (300ms, cancellable via `glib::source_remove`)
  - Note: `seek_to()` handles its own play-state restoration internally; do NOT pause before calling it

### Timers
**100ms timer** (main):
```
tick % 3000 → refresh Spotify token
Spotify active? → update title/art/pos labels, early return
detect new TV client (prev_client_count) → push full Metadata+Position+State to TV
tv_live changed → update tv_btn sensitivity; if TV dropped while active → fall back to local
is_started && sink_empty → track ended: fire on_track_end, auto-advance queue
is_started && !seeking → update position_scale, time_label, waveform_pos, queue_draw both areas
tv_connected → broadcast WsEvent::Position every tick
```

**16ms timer** (fast, zoomed waveform only):
```
is_playing || is_seeking → update pos_cell, queue_draw zoomed_area
```
Dedicated 60fps timer ensures smooth zoomed waveform scrolling independent of the 100ms main tick.

### do_load_track closure
Called on drag-and-drop (id=0) and queue auto-advance. For tracks from the browser, `track.id` is the Rekordbox DB id.

Key steps:
1. Stop Spotify if active
2. `state.load(path)` → rodio decoder, sets `duration_secs`
3. If `db_duration > 0` from Rekordbox, override `duration_secs`
4. Set `current_db_id` if `track_id != 0`
5. Load cues from `lib.load_cues(track_id)` → populate `waveform_cues` + color hot-cue buttons + update cue list panel timestamps
6. Load waveform: resolve `track_id` (or look up by path for D&D), call `lib.load_waveform(id, &anlz_base)`; invalidates `overview_wf_surface` cache
7. Store `last_metadata` for re-sending to newly connected TV clients
8. If TV output active, keep local sink muted (volume = 0.0)
9. `queue_draw` both waveform areas

### Hot cues
- Slots 1–8, stored as `(in_secs, Option<out_secs>, slot)` in `waveform_cues`
- `kind == 0` in `djmdCue` = memory cue (used as CUE button position)
- `kind 1–8` = hot cues; trigger buttons labeled A–H, colored via inline GTK CSS
- Loop region: cue has `out_secs = Some(t)` → tinted rectangle in zoomed view
- **Cue list panel** (right side): 8 clickable rows showing colored letter + timestamp; enabled only for slots that have a cue loaded; click seeks to that position

### Source badge
- `"♦ LIBRARY"` — local Rekordbox track
- `"♫ SPOTIFY"` — Spotify/librespot active

---

## DeckState (src/deck.rs)

```rust
pub struct DeckState {
    pub stream: OutputStream,
    pub stream_handle: OutputStreamHandle,
    pub sink: Sink,
    _keepalive: Sink,        // silent Zero source keeps PipeWire from suspending
    pub file_path: Option<PathBuf>,
    pub duration_secs: f64,
    pub play_started_at: Option<Instant>,
    pub accumulated_secs: f64,
    pub cue_position: f64,   // CUE button target
    pub loop_in: Option<f64>,
    pub loop_out: Option<f64>,
    pub loop_active: bool,
}
```

Key methods:
- `load(path)` → reads file, creates decoder, appends to sink, **always calls `sink.pause()` after append** (clear() can leave sink un-paused)
- `seek_to(pos)` → `sink.try_seek(pos)`, if that fails (sink empty) reloads file and re-seeks; always updates `accumulated_secs` and `play_started_at`
- `current_position_secs()` → `accumulated_secs + elapsed_since_play_start`
- M4A/AAC files are transcoded via ffmpeg to WAV in memory before decoding (symphonia ISOMP4 seek bug)
- Audio device selection skips devices containing "cam/webcam/video/capture" to avoid lighting up webcam LED

---

## Rekordbox ANLZ waveform files (src/rekordbox.rs)

### DB column
`djmdContent.AnalysisDataPath` holds a relative path e.g.:
`/PIONEER/USBANLZ/08e/32a79-4d6a-4053-8418-19e9e708ae47/ANLZ0000.DAT`

### File types
| Extension | Tag | Format | Use |
|---|---|---|---|
| `.DAT` | `PWAV` | 1 byte/col | Overview (monochrome preview) |
| `.EXT` | `PWV3` | 1 byte/col (color index, NOT 3-band) | Not used |
| `.2EX` | `PWV7` | 3 bytes/col (bass, mid, high) | Zoomed CDJ color waveform |

**PWV3 is NOT 3 bytes per column** — it is 1 byte/col with a packed color index. Use PWV7 from `.2EX` for the 3-band color waveform.

### ANLZ binary section format (big-endian)
```
[0..4]   fourcc tag          e.g. b"PWAV", b"PWV7"
[4..8]   header_length       bytes from section start to data start
[8..12]  section_length      total section size including header
[12..]   extra header fields (entry_count, flags — varies by tag)
[header_length..section_length]  data
```
Parser: `anlz_extract_section(file: &[u8], tag: &[u8; 4]) -> Option<Vec<u8>>`
Skips PMAI file header (length at bytes 4–8), then walks sections.

### Byte encoding (PWV7 per-column bytes, and PWAV)
```
bits 4–0 (lower 5): height 0–31
bits 7–5 (upper 3): whiteness 0–7  (0 = pure color, 7 = white)
```

### ANLZ base directory (src/config.rs `anlz_base_dir()`)
1. Primary: directory of `master.db` (e.g. `~/.local/share/dj-rs/`) if `PIONEER/` subdir exists
2. Fallback: parent of first path-mapping `from` + `/share` (dev tree layout)

### D&D track lookup
If `track_id == 0` (drag-and-drop), `lib.track_id_by_path(path)` looks up by `FolderPath` in `djmdContent`. Tries both raw path and `config.reverse_mappings(path)`.

---

## Rekordbox DB (src/rekordbox.rs)

- **Encryption**: SQLCipher, key = `402fd482c38817c35ffa8ffb8c7d93143b749e7d315df7a81732a1ff43608497`
- **All ID columns are VARCHAR(255)** — read as String, parse to i64 via `parse_id()`
- **Key tables**: `djmdContent` (tracks), `djmdPlaylist`/`djmdSongPlaylist` (playlists), `djmdCue` (cue points), `djmdArtist/Album/Genre/Key/Label`
- **Cue kinds**: `kind=0` = memory cue (used as CUE button), `kind=1–8` = hot cues

---

## Config (src/config.rs)

```json
{
  "db_path": null,
  "path_mappings": [
    { "from": "/local/dev/path/music", "to": "/home/user/Music" }
  ],
  "spotify_client_id": null,
  "spotify_access_token": "...",
  "spotify_refresh_token": "...",
  "acoustid_api_key": null
}
```

- `apply_mappings(path)`: replaces `from` prefix with `to` (used for playback paths)
- `reverse_mappings(path)`: replaces `to` prefix with `from` (used for DB lookups by path)
- `anlz_base_dir()`: resolves ANLZ file root (see above)

---

## TV streaming (src/server.rs)

- Server on port 7879
- TV connects via WebSocket `/ws` → receives `WsEvent` JSON
- `WsEvent::Stream { id, seek }` → TV fetches `/stream/{id}?seek={s}` → ffmpeg pipes AAC 256k to TV
- `WsEvent::Position { pos }` → TV updates its playhead display
- `WsEvent::State { playing }` → TV play/pause indicator
- `WsEvent::Metadata { title, artist, duration }` → TV track info
- TV can send `{ "type": "Seek", "pos": N }` back → stored in `seek_slot`, picked up by timer
- TV button only becomes sensitive when `bridge.tv_connected()` (client_count > 0)

---

## Spotify / librespot

- Token auto-refreshes every 5 minutes (tick % 3000 in timer)
- `LibrespotPlayer` runs its own tokio runtime in a background thread
- Audio path: librespot OGG → f64 samples → glib channel → GTK thread → rodio `SamplesBuffer`
- When Spotify active: local deck is paused, play button controls librespot, timer returns early (no waveform rendering for Spotify)
- Required OAuth scopes: `streaming playlist-read-private playlist-read-collaborative user-modify-playback-state user-read-playback-state`

---

## Iced UI architecture (src/bin/dj_iced.rs + src/bin/ui/)

### Two-layer player design
- **`PlayerState`** (in `player.rs`): display-only state — title, artist, BPM, key, `play_pos_secs`, waveform data, cue points. No audio.
- **`DeckState`** (in `deck.rs`, shared with GTK): actual rodio audio engine — Sink, play/pause/seek/loop.
- `App` owns both. `TrackClicked` loads audio into the deck AND updates PlayerState display fields. A 60fps `AudioTick` subscription syncs `play_pos_secs` from `deck.current_position_secs()`.

### View routing
Detail views (contact/gig/settings) replace the track list area; icon bar + sidebar tree stay visible:
```
main_area = if settings → settings::view()
            elif gig    → gig::view()
            elif contact → contact::view()
            else        → browser track list
```
Detail is passed as `Option<Element>` into `browser::view()`.

### Performance rule
`SectionClicked` (icon bar) does NOT clear contact/gig state — dropping `text_editor::Content` and re-rendering the track list is expensive. Only explicit navigation (`NodeSelected`, `ContactOpened`) clears detail views.

### Subscriptions
- **16ms** (`AudioTick`): updates `play_pos_secs` from deck, checks loop bounds, detects track end
- **5min** (`Tick`): refreshes Spotify OAuth token via `spawn_blocking`

### Overview waveform interaction
Canvas `Program::update` handles mouse press/move/release → sends `OverviewSeek(frac)` → `deck.seek_to(frac * duration)`. Supports click-to-seek and drag-to-scrub.

### Text clipping
All dynamic text in sidebar, track list, and detail views must be wrapped in `container(...).width(Fill).clip(true)` to prevent overflow. Tree/list rows use `container(...).height(TREE_ROW_H).align_y(Alignment::Center)` for vertical centering.

---

## Known gotchas / non-obvious decisions

- **`sink.pause()` after `append()`**: `sink.clear()` can leave the sink un-paused; always call `pause()` explicitly after loading a new track.
- **`seeking` flag**: when true, the 100ms timer skips the position-update block. Set on zoomed drag start + overview click; cleared on release events.
- **PWV3 ≠ 3-byte waveform**: PWV3 in `.EXT` is 1 byte/col with color index encoding, not 3-band. Use PWV7 from `.2EX`.
- **pango version mismatch**: gtk 0.9 uses pango 0.9.1; adding pango 0.22 causes conflict. `set_ellipsize` is skipped with a comment.
- **M4A/AAC via ffmpeg**: symphonia's ISOMP4 prober causes `unreachable!()` panic on seek. Transcode to WAV in-memory via ffmpeg before passing to rodio.
- **PipeWire keepalive**: a silent `Zero` source plays continuously to prevent PipeWire from suspending the device (which causes a resume-glitch on first play).
- **No Co-Authored-By in commits** (user preference).
- **All IDs in Rekordbox DB are VARCHAR** — never `get::<_, i64>()` directly, always read as String and parse.

---

