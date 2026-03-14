# Development Log

## 2026-03-14 — Library tooling & reload button

### Features implemented

**↺ Reload Library button**
- Added to BrowserView topbar, next to "Open Library…"
- Re-opens the currently configured `db_path` from config, refreshing playlists, tracks, history, and filter combos in one click
- Intended for use after external DB modifications (e.g. `create_sandbox.py`, `gig-prep`)

**Library analysis & playlist structure (Python tooling)**
- Audited library for missing/streaming tracks: streaming-only entries (`/v4/catalog/tracks/…`) removed from `djmdContent` and all dependent tables (`djmdSongPlaylist`, `djmdSongHistory`, `djmdCue`, `djmdMixerParam`)
- Built automated playlist structure via Python (`sqlcipher3`) based on cross-gig play frequency and BPM analysis — see `dj_jonas/` for details (gitignored)
- Backup of `dj_jonas/` DB files (excluding `music/`, `share/`, `PIONEER/`) saved as `dj_jonas_backup_YYYYMMDD.tar.gz`; root-level archives are gitignored

### Bugs found and fixed

| Bug | Cause | Fix |
|---|---|---|
| App crash (SIGABRT) on Reload click | `config.borrow()` in `if let Some(path) = config.borrow().db_path.clone()` held an immutable borrow alive across the entire `if let` block; `do_open_library` internally calls `config.borrow_mut()`, causing a double-borrow panic | Separated into two statements: `let path = config.borrow().db_path.clone();` then `if let Some(path) = path { do_open(&path); }` so the borrow is dropped before `do_open` is entered |

### Lessons learned

- **RefCell borrow lifetime in `if let`**: a temporary `Ref<T>` created in the scrutinee of `if let Some(x) = rc.borrow().field.clone()` lives for the entire `if let` block, not just the condition. If any code inside the block re-borrows the same `RefCell` mutably, it panics at runtime. Always assign to a `let` binding first to drop the borrow immediately.

## 2026-03-13 — Feature sprint

### Features implemented

**DLNA casting (Samsung TV fix)**
- Root cause: Samsung Q8 TV's AVTransport DMR ignores audio-only content
- Solution: ffmpeg wraps audio in MPEG-TS with dummy black video, served as `video/mpeg`
- Profile `MPEG_TS_SD_EU_ISO` is the one the TV accepts
- Tried and failed: MPEG-PS (`-f vob`, profile `MPEG_PS_PAL`) → `ERROR_OCCURRED`
- ffmpeg is spawned as a `tokio::process::Command`, stdout piped as streaming HTTP body via `tokio_util::io::ReaderStream`

**Track queue**
- Right-click → Queue in browser
- `Next →` button in player
- Auto-advance in 100ms glib timer when `is_started && sink.empty()`

**Library browser features**
- Track columns: Title, Artist, BPM, Key, Time, Genre, Rating (stars), Label
- Column-header click sorting (GTK `ListStore` + `set_sort_column_id` on each column)
- Filter bar: BPM range spinners, Key/Genre combos, Rating dropdown, Harmonic toggle
- Harmonic filter: Camelot wheel ±1 key matching (`compatible_camelot_keys()`)
- My Tags: selected track's tags shown in label below list (djmdMyTag + djmdSongMyTag)
- History sessions: own **History** tab in a `gtk::Notebook` sidebar
- Extended search: genre, label, key in addition to title/artist/album

**Playlist management**
- Create/delete/rename playlist via right-click context menu
- Create playlist inside folder
- Drag playlist onto folder to reparent
- Drag-to-reorder within list (updates `djmdPlaylist.Seq`)

**Write-back to DB**
- Play count: `DJPlayCount` incremented when track ends (`on_track_end` callback)
- Star rating: 5 buttons per selected track → writes `Rating` column, updates store in place

### Bugs found and fixed

| Bug | Cause | Fix |
|---|---|---|
| Window not visible | Infinite `while` loop in `do_open_library` when `key_combo` was empty on startup | Removed the loop |
| Track list empty (count shows) | `c.Comment` doesn't exist in this rekordbox DB version; query fails silently inside `if let Ok` | Removed `c.Comment` from struct and all queries |
| Track list empty after second bug | `c.ColorID` is `VARCHAR(255)` text; `row.get::<_, Option<i32>>()` returns `InvalidColumnType` | Changed `color_id` field to `Option<String>` |
| Track list empty (different) | `gtk::TreeModelSort` silently disconnects from the view in gtk-rs 0.9 | Removed `TreeModelSort`; `ListStore` sorts natively via `set_sort_column_id` |

### Lessons learned about rekordbox DB

- Every `*ID` column (ArtistID, AlbumID, GenreID, KeyID, LabelID, ColorID, etc.) is stored as `VARCHAR(255)` with decimal integer values as text strings. `rusqlite` refuses to coerce text to integer — always read as `String` and parse.
- `djmdContent.Comment` is documented in the schema but **does not exist** in at least one rekordbox 6 version. Do not query it.
- `ParentID = "root"` is the sentinel for top-level nodes (not NULL).
- `BPM` is ×100 integer. 128.00 BPM → `12800`.

### Lessons learned about GTK (gtk-rs 0.9)

- `gtk::TreeModelSort::new(&store)` compiles and runs but silently produces an empty view. Use `ListStore` directly with `col.set_sort_column_id(n)`.
- `glib::idle_add_local` callbacks must not block. Any infinite loop or blocking call prevents the GTK main loop from processing expose events, so the window never renders.
- `set_active(Some(0))` on a `ComboBoxText` fires `connect_changed` synchronously, even if the library hasn't been opened yet. Guard all filter callbacks with `library.borrow().as_ref()` checks.
- `connect_changed` on selection callbacks: guard with `sel.get_selected()` returning `None` when model is cleared.
- Use `gtk::TreeStore` (not `ListStore`) for hierarchical data — it gives native expand/collapse with expander arrows. Call `tree_view.collapse_all()` after populate to start collapsed.
- `IndexMap` is required when insertion order must be preserved (e.g. DB results ordered by `Seq`). A plain `HashMap` silently shuffles entries.
