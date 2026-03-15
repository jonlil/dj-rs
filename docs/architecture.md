# dj-rs Architecture

A GTK3 desktop DJ library application written in Rust. Reads a Rekordbox 6
encrypted SQLite database, plays audio locally, and can stream to a Samsung TV
via WebSocket.

---

## Module overview

```
src/
  main.rs               — GTK Application + Widgets root, wires views together
  config.rs             — JSON config (~/.config/dj-rs/config.json): db_path, path mappings
  deck.rs               — DeckState: rodio Sink + play/pause/seek/loop
  rekordbox.rs          — Rekordbox DB access (SQLCipher via rusqlite) + ANLZ binary parser
  gig.rs                — Gig, Contact, GigStore (gigs.json persistence)
  spotify.rs            — PKCE OAuth, fetch_playlist, Spotify Web API
  librespot_player.rs   — Full Spotify playback via librespot → rodio
  matcher.rs            — Jaro-Winkler fuzzy match: Spotify tracks ↔ library
  server.rs             — axum HTTP/WS server (port 7879), TV stream via ffmpeg
  dlna.rs               — DLNA renderer discovery
  tags.rs               — lofty tag read/write (ISRC, MusicBrainz, AcoustID)
  views/
    mod.rs              — PlayerView + MainView (all GTK wiring)
    browser.rs          — Library/playlist browser tree + track list
    gig_workspace.rs    — Gig editor (5-tab Notebook)
    gig_sidebar.rs      — Contacts/Gigs sidebar tree
    contact_view.rs     — Contact detail + pool playlists
    dialogs.rs          — Settings dialog, new-gig dialog
    utils.rs            — find_widget helper
```

---

## UI layout

```
┌─ MenuBar (File → Quit, etc.) ─────────────────────────────────────┐
├─ MainView (HPaned) ───────────────────────────────────────────────┤
│  ┌─ Left sidebar ──────────┐  ┌─ Right pane ────────────────────┐ │
│  │ GigSidebar              │  │ PlayerView (Deck)               │ │
│  │ Contacts / Gigs tree    │  │  [art | info row]               │ │
│  │                         │  │  [left_col | waveforms | cues]  │ │
│  │ (click gig → workspace) │  │  [A][B][C][D][E][F][G][H]      │ │
│  │                         │  ├─ BrowserView ──────────────────┤ │
│  │                         │  │ [Open Library] [Reload] [Settings] │
│  │                         │  │ [BPM:] [Key:] [Genre:] [Rating:] [Harmonic] │
│  │                         │  │ ┌──────────────┬──────────────┐ │ │
│  │                         │  │ │ All Tracks   │ track list   │ │ │
│  │                         │  │ │ ▸ Folder     │              │ │ │
│  │                         │  │ │   Playlist   │              │ │ │
│  │                         │  │ │ — History —  │              │ │ │
│  │                         │  │ └──────────────┴──────────────┘ │ │
│  │                         │  │ Tags: —   ★ rating row          │ │
│  └─────────────────────────┘  └────────────────────────────────┘ │
└───────────────────────────────────────────────────────────────────┘
```

---

## State and data flow

### PlayerView shared state

`PlayerView` holds `Rc<RefCell<DeckState>>`. Key Rc handles shared across callbacks:

| Handle | Type | Purpose |
|---|---|---|
| `state` | `Rc<RefCell<DeckState>>` | audio sink + position tracking |
| `current_track_db_id` | `Rc<RefCell<Option<i64>>>` | DB id of currently loaded track |
| `waveform_cues` | `Rc<RefCell<Vec<(f64, Option<f64>, usize)>>>` | hot cue data (in_secs, out_secs, slot 1–8) |
| `tv_output` | `Rc<RefCell<bool>>` | whether TV output is the active sink |
| `last_metadata` | `Rc<RefCell<Option<(String, String, f64)>>>` | track info for re-sending on TV connect |
| `pending_tv_stream` | `Rc<RefCell<Option<glib::SourceId>>>` | debounce handle for overview seek WsEvent |
| `overview_wf_surface` | `Rc<RefCell<Option<(i32, i32, cairo::ImageSurface)>>>` | pre-rendered overview surface cache |

`MainView` exposes `queue_fn`, `current_track_db_id`, and `on_track_end` so
`main.rs` can wire them to `BrowserView`.

`BrowserView` sets `on_track_end` to call `lib.increment_play_count(id)` once
the library is open.

### Timer loops (inside PlayerView)
- **100ms**: position updates, Spotify token refresh, TV client detection, track-end auto-advance
- **16ms**: zoomed waveform redraws at 60fps (only runs when playing or seeking)

---

## Rekordbox database

See [`rekordbox-schema.md`](rekordbox-schema.md) for the full schema.

### Opening
```rust
PRAGMA key = '<hex>';
PRAGMA cipher_page_size = 4096;
PRAGMA kdf_iter = 256000;
PRAGMA cipher_hmac_algorithm = HMAC_SHA512;
PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;
```

### Critical quirks

1. **All ID columns are `VARCHAR(255)` storing decimal integers as text.**
   `rusqlite`'s `row.get::<_, i64>()` returns `InvalidColumnType` on a text
   cell. Always read as `String` and `parse()`.

2. **`c.Comment` does not exist** in this version of the rekordbox schema.
   Querying it causes a silent `OperationalError` and an empty track list.

3. **`ParentID = "root"`** is the sentinel for top-level playlist nodes (not NULL, not 0).

4. **`BPM` is stored ×100**: 128.00 BPM → stored as `12800`.

5. **`djmdPlaylist.Attribute`**: `0` = regular playlist, `1` = folder, `2` = smart playlist.

6. **`djmdColor`** has 8 rows (0–8). ColorID `"0"` = no colour.

---

## TV streaming (WebSocket)

See [`tizen-app.md`](tizen-app.md) for the Tizen app side.

- axum server on port 7879
- TV connects via WebSocket `/ws` → receives `WsEvent` JSON
- Audio served at `/stream/{id}?seek={s}` → ffmpeg transcodes to AAC 256k ADTS and pipes it
- `WsEvent::Stream { id, seek }` → TV fetches the stream URL
- `WsEvent::Position`, `State`, `Metadata` keep the TV UI in sync
- TV can send `{ "type": "seek", "pos": N }` → stored in `seek_slot`, picked up by 100ms timer

---

## Path mappings

Rekordbox stores paths as recorded on macOS. On Linux the drive may be at a different
mount point. The Settings dialog lets the user add prefix-rewrite rules saved in `config.json`.
`apply_mappings(path)` rewrites for playback; `reverse_mappings(path)` rewrites back for DB lookups.

---

## Column sorting

`ListStore` sorts natively via `col.set_sort_column_id(n)`. Do not wrap in `TreeModelSort`
— in gtk-rs 0.9 the sort proxy silently disconnects from the view, producing an empty track list.

---

## Playlist sidebar

Uses `gtk::TreeStore` (not `ListStore`) for native folder expand/collapse.
`IndexMap` is required for children lists — a plain `HashMap` loses the `ORDER BY Seq` ordering.
History sessions live in a separate **History** tab (`gtk::Notebook`).
