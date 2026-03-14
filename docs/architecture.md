# dj-rs Architecture

A GTK3 desktop DJ library application written in Rust. Reads a Rekordbox 6
encrypted SQLite database, plays audio locally, and can cast to DLNA renderers
(Samsung TVs, etc.).

---

## Module overview

```
src/
  main.rs       — GTK Application + Widgets root, wires views together
  views.rs      — All GTK UI (PlayerView, MainView, BrowserView)
  rekordbox.rs  — Rekordbox DB access (SQLCipher via rusqlite)
  deck.rs       — Local audio playback (rodio + cpal)
  dlna.rs       — DLNA casting, SSDP discovery, HTTP server, MPEG-TS transcoding
  config.rs     — JSON config (~/.config/dj-rs/config.json): db_path + path mappings
```

---

## UI layout

```
┌─ MenuBar (File → Quit) ───────────────────────────────────┐
├─ MainView ────────────────────────────────────────────────┤
│   PlayerView (Deck)                                        │
│   [Track info] [position slider] [time]                   │
│   [Load] [Play/Pause] [Stop]                              │
│   [Vol slider]  [Output device combo]                     │
│   [Cast…] [Stop Cast] [cast status]                       │
│   [Next: —]                              [Next →]         │
├─ BrowserView ─────────────────────────────────────────────┤
│   [Open Library…] [Settings…] [N tracks]  [Search…]      │
│   [BPM: min–max] [Key:▾] [Genre:▾] [Rating:▾] [Harmonic] │
│   ┌──────────────┬──────────────────────────────────────┐ │
│   │ ★ All Tracks │ Title Artist BPM Key Time Genre ★ Lbl│ │
│   │ ▸ Folder     │ …                                    │ │
│   │   Playlist   │                                      │ │
│   │ — History —  │                                      │ │
│   │   Session 1  │                                      │ │
│   └──────────────┴──────────────────────────────────────┘ │
│   Tags: —                                                 │
│   Set rating: ★ ★★ ★★★ ★★★★ ★★★★★  ✕                    │
└───────────────────────────────────────────────────────────┘
```

---

## State and data flow

### PlayerView shared state
`PlayerView` holds `Rc<RefCell<DeckState>>`. Several `Rc`-cloned handles are
shared with callbacks:

| Field | Type | Purpose |
|---|---|---|
| `state` | `Rc<RefCell<DeckState>>` | audio sink + position tracking |
| `queue_fn` | `Rc<dyn Fn(PathBuf)>` | called by BrowserView to queue next track |
| `current_track_db_id` | `Rc<RefCell<Option<i64>>>` | DB id of currently loaded track |
| `on_track_end` | `Rc<RefCell<Option<Rc<dyn Fn(i64)>>>>` | fired when track ends naturally |

`MainView` exposes `queue_fn`, `current_track_db_id`, and `on_track_end` so
`main.rs` can pass them to `BrowserView::new`.

`BrowserView` sets `on_track_end` to call `lib.increment_play_count(id)` once
the library is open.

### 100ms glib timer (inside PlayerView)
- Updates position slider + time label while playing
- When `is_started && sink.empty()`: track ended → fires `on_track_end`, then
  auto-advances to queued track if one is set

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
   cell. Always read as `String` and `parse()`. This applies to every `*ID`
   column including `ColorID`, `LabelID`, etc.

2. **`c.Comment` does not exist** in this version of the rekordbox schema
   (despite being documented). Querying it causes an `OperationalError` that
   silently fails the whole `tracks()` call (wrapped in `if let Ok`), resulting
   in an empty track list with the count still showing from the status label.

3. **`ParentID = "root"`** is used as sentinel for top-level playlist nodes
   (not NULL, not 0).

4. **`BPM` is stored ×100**: 128.00 BPM → stored as `12800`. Display with
   `bpm as f32 / 100.0`.

5. **`djmdPlaylist.Attribute`**: `0` = regular playlist, `1` = folder,
   `2` = smart playlist.

6. **`djmdColor`** has 8 rows (0–8). ColorID `"0"` = no colour.

---

## DLNA casting to Samsung TV

See [`dlna-samsung-casting.md`](dlna-samsung-casting.md) for full details.

**Short version**: Samsung Q-series TVs ignore audio-only DLNA content
(AVTransport stays `TRANSITIONING` forever). The fix is to wrap audio in an
MPEG-TS container with a dummy black video track via ffmpeg, served as
`video/mpeg` with profile `MPEG_TS_SD_EU_ISO`. The TV's video decoder then
works correctly.

---

## Path mappings

Rekordbox stores paths as recorded on macOS (e.g. `/Volumes/muzika/...`). On
Linux the drive may be mounted at `/run/media/jonas/muzika`. The Settings
dialog lets the user add prefix-rewrite rules saved in `config.json`.

---

## Column sorting

`ListStore` implements `TreeSortable` natively in GTK3. Calling
`col.set_sort_column_id(n)` on each `TreeViewColumn` makes column headers
clickable for sort. **Do not wrap in `TreeModelSort`** — in gtk-rs 0.9 the
sort proxy silently disconnects from the view, producing an empty track list.

---

## Playlist sidebar

The playlist panel uses a `gtk::TreeStore` (not `ListStore`) so folders can be
expanded and collapsed natively. Folders start collapsed via `pl_view.collapse_all()`
after each populate.

`browser_populate_playlists` builds the tree using an `IndexMap<Option<i64>,
Vec<&Playlist>>` keyed by `parent_id`. `IndexMap` is required — a plain
`HashMap` loses the `ORDER BY Seq` ordering from the DB query.

History sessions live in a separate **History** tab (`gtk::Notebook`). They are
populated by `browser_populate_history` into their own `ListStore` / `TreeView`.

---

## Known issues / TODO

- Cue point display in player (data is read via `cues_for_track()`, not shown)
- Smart playlists (`Attribute = 2`) not evaluated — shown as regular playlists
- Drag track from browser into a playlist not yet implemented
- History sessions are in their own sidebar tab; selecting a session loads its tracks
