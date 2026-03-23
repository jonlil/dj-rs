# Architecture Conventions

## Layers

```
┌─────────────────────────────────┐
│  App (src/bin/ui/)              │  Iced-specific: views, Message, update()
│  Knows about: services, domain  │
│  Owns: DeckState (temporary*)   │
├─────────────────────────────────┤
│  Services (src/services/)       │  Business logic, orchestration
│  Knows about: domain, infra     │  No iced dependency. Usable from CLI/lib.
├─────────────────────────────────┤
│  Domain (src/)                  │  Data types: Track, Playlist, CuePoint
│  Knows about: nothing           │
├─────────────────────────────────┤
│  Infrastructure (src/)          │  DbHandle, DeckState*, file I/O,
│  Knows about: nothing above     │  Spotify API client
└─────────────────────────────────┘

* DeckState lives in the app layer today for pragmatic reasons.
  Target: infrastructure, behind its own channel, with sample-
  accurate position from the audio callback.
```

## Rules

### 1. Layer direction
App → Services → Infrastructure. Never skip layers for writes. Reads may shortcut
where performance matters.

### 2. Services are plain functions
Not traits or structs. They take what they need as arguments (`&DbHandle`, config
values, paths). No iced types (`Message`, `Task`, `Element`). A service must be
callable from a CLI tool or a test without spinning up a GUI.

### 3. DbHandle is a thin wrapper
Owns the Connection, serializes access via mpsc. Exposes `with_conn(closure)` — no
domain types, no knowledge of what a "track" or "playlist" is. SQL lives in services,
not in DbHandle.

### 4. Domain types are standalone
`Track`, `Playlist`, `CuePoint` etc. live in `src/rekordbox.rs` today. They may split
into `src/domain/` later. Domain types have no dependencies on infrastructure or
services.

### 5. Iced update() handles synchronous work
`update()` does everything that must be synchronous:
- **UI state**: toggle a panel, highlight a row, map form fields to a model.
- **Real-time audio control**: play, pause, seek, scratch via DeckState.

Asynchronous I/O (DB queries, file reads, API calls) and multi-step orchestration
(import pipeline) are delegated to services via `Task::perform`.

### 6. DeckState: real-time playback lives near the hardware
DeckState currently lives in the app layer and is called synchronously from `update()`.
This is a pragmatic shortcut — long-term, audio control moves to infrastructure behind
its own channel, with sample-accurate position derived from the audio callback rather
than wall-clock time.

Services handle audio *processing* (decode, encode FLAC, BPM analysis, ANLZ
generation) but never real-time playback. The distinction: processing can take seconds
and runs on a background thread; playback is latency-critical and tied to the audio
device.

### 7. GigStore is infrastructure
GigStore (JSON file persistence) is infrastructure, not a service. Gig business logic
(save with cascades, matching) lives in `services/gig.rs`.

### 8. Read/write separation
DbHandle serializes writes through a single connection. Reads use separate connections
and may run concurrently. A background import must never block playlist browsing.

### 9. Sync-first, async-enrich
When loading a track for playback, audio starts synchronously — no round-trip through
services. Metadata, cues, and waveforms load asynchronously and update the UI as they
arrive. Never delay audio for data. This pattern is critical for a DJ workflow where
pressing play must feel instant.

## File mapping

| Layer | Location | Examples |
|---|---|---|
| App | `src/bin/ui/` | `mod.rs`, `browser.rs`, `player.rs` |
| Services | `src/services/` | `track.rs`, `gig.rs`, `playlist.rs`, `import.rs`, `analysis.rs` |
| Domain | `src/` | `rekordbox.rs` (types), `gig.rs` (types) |
| Infra | `src/` | `db.rs`, `spotify.rs`, `config.rs`, `tags.rs`, `deck.rs` |

## What belongs where?

| Question | Layer | Location |
|---|---|---|
| Highlight selected row | App | `update()` |
| Play/pause/seek/scratch | App | `update()` → `DeckState` |
| Load track for playback | App | `update()` → `DeckState` (sync), then async cues/waveform |
| Fetch tracks for a playlist | Service | `services/track.rs` |
| Import: convert → analyze → tag → insert | Service | `services/import.rs` |
| Save gig + cascade-delete orphans | Service | `services/gig.rs` |
| Decode audio / encode FLAC | Service | `services/import.rs` |
| BPM detection / ANLZ generation | Service | `services/analysis.rs` |
| Open SQLCipher connection | Infra | `db.rs` |
| Read/write ID3 tags | Infra | `tags.rs` |
| Parse ANLZ binary format | Infra | `rekordbox.rs` |
| Persist gig data to JSON | Infra | `gig.rs` (GigStore) |

## Future: extracting a library crate

Services, domain, and infrastructure have no iced dependency. If we later want a
standalone `rekordbox-lib` crate (for CLI tools, other frontends), these layers can
move out with no changes. Only `src/bin/ui/` is iced-specific.
