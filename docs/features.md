# dj-rs — Features & Business Rules

> Merged from iced UI (primary) and GTK UI. Iced rules take precedence where
> they differ. Features marked **[GTK only]** exist in the GTK build but are
> not yet ported to iced.

---

## 1. Library Browser

### 1.1 Sections
- Three icon-bar sections: **Library**, **Spotify**, **Contacts**
- Click toggles sidebar open/closed if already on that section
- Settings cog at bottom of icon bar, opens inline settings panel

### 1.2 Playlist Tree
- Recursive folder/playlist hierarchy from Rekordbox DB
- "All Tracks" root entry always available
- Folders expand/collapse (▸/▾); playlists show track count badge
- Clicking a playlist loads its tracks; clicking a folder only toggles it
- Library shows **all** playlists including gig output folders (CORPORATE/VENUES/PRIVATE)
- **[GTK only]** Gig output folders hidden via GIG_FOLDERS filter

### 1.3 Track List
- Columns: Title, Artist, BPM (1 decimal), Key, Duration (MM:SS)
- **[GTK only]** Additional columns: Genre, Rating (stars), Label, Color
- Alternating row backgrounds for readability
- Click loads track into player deck
- **[GTK only]** Drag-and-drop onto player deck

### 1.4 Search
- Text input searches across all tracks in DB
- Empty query reverts to currently selected playlist
- Each keystroke triggers a fresh search

### 1.5 History
- **[GTK only]** Browse past DJ sessions from Rekordbox history table

---

## 2. Player & Playback

### 2.1 Audio Engine
- rodio `DeckState` handles all audio: load, play, pause, seek, loop
- `sink.pause()` always called after `sink.append()` — `clear()` can leave sink unpaused
- M4A/AAC transcoded to WAV via ffmpeg in memory (symphonia ISOMP4 seek bug)
- PipeWire keepalive: silent `Zero` source prevents device suspension
- Device selection skips webcam/capture devices (keyword filter)

### 2.2 Track Loading
1. Resolve file path via config path mappings (`from` → `to`)
2. Load file into deck (transcode if M4A/AAC)
3. DB duration overrides decoded duration if > 0
4. Load cues from DB async → set CUE position from first memory cue (kind=0)
5. Load waveform from ANLZ files async (PWV7 from `.2EX`, PWAV from `.DAT`)
6. **[GTK only]** Load album art in background thread (48x48)
7. **[GTK only]** If TV output active: mute local sink, push metadata to TV
8. **[GTK only]** Stop any active Spotify/librespot playback first

### 2.3 Transport Controls
- **CUE**: Seek to memory cue position, pause, update display
- **Play/Pause**: Toggle deck play state; button shows ▶ when paused, ■ when playing
- Both disabled when no track loaded
- Pioneer-style colors: CUE = amber (#E0AD00), Play = green (#1ACD38)

### 2.4 Position Tracking
- `accumulated_secs + elapsed_since_play_start` = current position
- 60fps `AudioTick` subscription syncs display from deck
- Auto-stop when position reaches end of track (within 0.1s)
- Loop check every tick: if past loop-out, seek back to loop-in
- Remaining time shown as `-MM:SS.S`

### 2.5 Waveform Display

**Overview (28px, full track)**:
- Data: PWV7 (3-band color) preferred, PWAV (monochrome) fallback
- White playhead marker, dark overlay on played region
- Hot cue markers as colored vertical lines (skip memory cues)
- **Interactive**: Click or drag to seek (fraction → `deck.seek_to`)
- Pointer cursor on hover
- **[GTK only]** Seek debounced 300ms for TV Stream event

**Zoomed (100px, 6-second window)**:
- Playhead fixed at 25% from left (`PLAYHEAD_FRAC = 0.25`)
- 18px cue strip at top with colored triangles
- Loop regions: tinted rectangles from in to out
- Red triangle + white line at playhead position
- Only renders columns in visible range (optimized)
- **[GTK only]** Drag-to-scratch: press pauses, drag scrubs, release resumes

**Waveform byte encoding (PWV7 per column, 3 bytes: bass/mid/high)**:
- Bits 0–4: height 0–31
- Bits 5–7: whiteness 0–7 (0 = pure color, 7 = white)

**Color blending**:
- Bass fraction → amber/warm, High fraction → blue/steel, Mid → green
- Whiteness averaged across bands, modulates brightness

### 2.6 Cue Points
- Memory cue (kind=0): CUE button target, blue marker
- Hot cues (kind 1–8): Labeled A–H, 8-color palette
- Colors: A=red, B=orange, C=yellow, D=green, E=cyan, F=blue, G=purple, H=grey
- Loop: cue with `out_secs` draws tinted region in waveform
- Side panel (180px): colored badge + timestamp + comment, scrollable

### 2.7 Volume & Output
- **[GTK only]** Volume slider, local/TV output toggle (mutually exclusive)
- **[GTK only]** TV button only sensitive when WebSocket client connected

---

## 3. Contacts (CRM)

### 3.1 Contact Types
- **Private** — blue badge (#6699FF)
- **Corporate** — orange badge (#FF9933)
- **Venue** — green badge (#77DD77)

### 3.2 Contact List (Sidebar)
- Flat list grouped by type (headers: PRIVATE / CORPORATE / VENUES)
- "+ New Contact" button at top
- Each row: colored dot, name (clipped), gig count badge
- Active contact highlighted
- **[GTK only]** Expandable tree with Rekordbox pool playlists under each contact

### 3.3 Contact Detail View
- Replaces track list area; sidebar stays visible
- Header: name, type badge, dirty indicator, Save / Delete / + New Gig buttons
- Fields: Name (text input), Type (3-toggle: Private/Corporate/Venue), Notes (text editor)
- Gig list below: clickable rows with name, date, location
- Dirty tracking: "● unsaved" (amber) / "✓ saved" (green)

### 3.4 Contact Lifecycle
- Create: generates UUID, defaults to Private, empty name/notes
- Save: writes back to GigStore, persists to gigs.json
- Delete: **cascade** — removes contact AND all associated gigs

---

## 4. Gigs & Event Planning

### 4.1 Gig Fields
- Name, Date (YYYY-MM-DD), Start Time (HH:MM), End Time (HH:MM), Location, Notes
- Spotify Playlist URL
- All optional except implicit contact_id link
- Empty strings stored as None

### 4.2 Gig Detail View
- "← ContactName" back button returns to contact view
- Header: back, title, dirty indicator, Save button
- Info section: all fields as text inputs + notes editor
- Spotify section: URL input + Run Match button

### 4.3 Spotify Match Workflow
1. Enter Spotify playlist URL
2. Click "Run Match" → status: Running (shows "Matching…")
3. Async: fetch playlist → load library → Jaro-Winkler fuzzy match
4. Results split into three sections:

**Matched** (has local library match):
- Shows: "Artist – Title" (Spotify) → "Artist – Title" (local)
- Accept toggle (green = accepted, default for new matches)
- Stored in `accepted_track_ids`

**Missing** (no local match):
- Shows: "Artist – Title" + duration
- Buy button (amber; green when in buy list)
- Skip button (sends to Skipped section)
- Stored in `pending_buy_tracks` or `denied_spotify_ids`

**Skipped** (user denied):
- Shows: dimmed "Artist – Title"
- Undo button to reverse denial

5. Re-running match preserves accept/deny/buy state
6. **[GTK only]** Spotify track preview playback button in match rows

### 4.4 Matching Algorithm (`src/matcher.rs`)
- **Score**: 65% title weight + 35% artist weight (Jaro-Winkler similarity)
- **Threshold**: 0.82 combined score
- **Pre-filter**: Duration tolerance ±20 seconds
- **Normalization**:
  - Strip version suffixes: `(remastered)`, `[radio edit]`, etc.
  - Strip trailing dash suffixes: `- remaster`, `- radio edit`
  - Title: remove `feat.` entirely
  - Artist: take primary only (before `feat`, `ft.`, `&`, `,`)
  - Lowercase all

### 4.5 Shopping List (Buy List)
- Auto-generated from tracks marked "Buy" in missing section
- "Copy Shopping List" button → clipboard
- Section hidden if no pending buy tracks
- **[GTK only]** Formatted with Beatport/Traxsource search links

### 4.6 Gig Finalization
- **[GTK only]** "Finalize" tab creates Rekordbox folder hierarchy:
  - `{ContactType}/{ContactName}/{GigName}/` folder in DB
  - Populates playlist with all accepted track IDs
  - Stores `rekordbox_folder_id` in gig JSON

### 4.7 Gig Persistence
- `gigs.json` stores: contacts array + gigs array
- Each gig: id, contact_id, name, date, times, location, notes, spotify URL
- Match state: cached_spotify_tracks, accepted_track_ids, pending_buy_tracks, denied_spotify_ids
- rekordbox_folder_id (set after finalization)

---

## 5. Spotify Integration

### 5.1 Authentication
- PKCE OAuth flow — opens browser for user authorization
- Scopes: `streaming playlist-read-private playlist-read-collaborative user-modify-playback-state user-read-playback-state`
- Tokens persisted in config.json (access_token + refresh_token)
- Auto-refresh every 5 minutes via background subscription
- Refresh uses `spawn_blocking` (reqwest blocking client)

### 5.2 Spotify Playlist Browser
- Icon bar "Spotify" section shows user's playlists in sidebar
- Lazy-loaded on first click (requires token)
- Selecting a playlist: fetches tracks, matches against library, shows in main area
- Track rows: green dot = in library, amber dot = missing
- Summary bar: total / in-library / missing counts

### 5.3 Spotify Playback
- **[GTK only]** Full track playback via librespot
- **[GTK only]** Audio path: OGG → f32 samples → glib channel → rodio SamplesBuffer
- **[GTK only]** Source badge: "♫ SPOTIFY" vs "♦ LIBRARY"
- **[GTK only]** When active: local deck paused, timer returns early (no waveform)
- **[GTK only]** Preview playback from match rows

---

## 6. TV Streaming

**[GTK only]** — entire feature

- Samsung TV at 192.168.1.44 via WebSocket (port 7879)
- Desktop → TV events: Metadata, Position (every 100ms), State, Stream
- TV → Desktop: Seek commands (stored in seek_slot)
- Audio: ffmpeg AAC 256k for local tracks; F32 for Spotify
- TV button only sensitive when client connected
- Local sink muted when TV output active
- New client detection: timer pushes full state on connect

---

## 7. Settings

### 7.1 Path Mappings
- Prefix rewrite pairs: From (DB path prefix) → To (local path prefix)
- Applied when loading tracks for playback
- Reverse-applied when looking up tracks by path
- Add/remove rows dynamically; empty rows filtered on save

### 7.2 Spotify Connection
- Status: "✓ Connected" / "Not connected" / "Waiting for browser…" / "Error: …"
- Connect button triggers PKCE auth flow
- Settings panel: inline view, toggles with ⚙ icon, close with ✕

### 7.3 Settings Lifecycle
- Opens as inline panel (replaces track list area)
- Dirty tracking with explicit Save
- Saving refreshes stored config (path mappings take effect immediately)

---

## 8. Tags & Metadata

**[GTK only]** — ISRC enrichment pipeline

- AcoustID fingerprint → MusicBrainz lookup → ISRC tag write
- Supported formats: FLAC (Vorbis comments), MP3 (ID3v2), M4A (iTunes atoms)
- Confidence thresholds for accepting matches
- Batch scanning with progress UI

---

## 9. Rekordbox Database

- SQLCipher encrypted (`master.db`)
- Key: `402fd482c38817c35ffa8ffb8c7d93143b749e7d315df7a81732a1ff43608497`
- **All ID columns are VARCHAR(255)** — read as String, parse to i64
- Key tables: djmdContent, djmdPlaylist, djmdSongPlaylist, djmdCue, djmdArtist/Album/Genre/Key/Label
- Cue kinds: 0 = memory cue, 1–8 = hot cues
- Playlists: attribute=1 = folder, attribute=0 = playlist
- Filtered: `rb_local_deleted = 0`, ordered by `Seq`

### ANLZ Waveform Files
- Location: `{anlz_base}/PIONEER/USBANLZ/{uuid}/ANLZ0000.{DAT,EXT,2EX}`
- `.DAT` PWAV: 1 byte/col (overview)
- `.2EX` PWV7: 3 bytes/col (zoomed color) ← used
- `.EXT` PWV3: 1 byte/col (NOT 3-band, not used)

---

## 10. Data Paths

| Purpose | Path |
|---|---|
| Config | `~/.config/dj-rs/config.json` |
| Rekordbox DB | `~/.local/share/dj-rs/master.db` |
| ANLZ waveforms | `~/.local/share/dj-rs/PIONEER/USBANLZ/…` |
| Gig data | `~/.config/dj-rs/gigs.json` |
| Music (mapped) | `~/Music/` |

---

## 11. UI Rules

### Navigation
- Detail views (contact/gig/settings) replace the track list area; sidebar stays visible
- Icon bar clicks do NOT clear detail views (performance: avoids dropping text_editor::Content)
- Only explicit navigation (NodeSelected, ContactOpened) clears detail views
- Back from gig → contact stays open; back from contact → browser

### Text Handling
- All dynamic text: `container(...).width(Fill).clip(true)` to prevent overflow
- Tree/list rows: `container(...).height(ROW_H).align_y(Alignment::Center)` for vertical centering

### Dirty State
- Per-entity dirty flag (contact, gig, settings)
- Visual: "● unsaved" (amber) vs "✓ saved" (green)
- Marked on any field edit; cleared on explicit Save
