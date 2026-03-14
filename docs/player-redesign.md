# Player redesign

## Goals

- **Separation of concerns**: player state (what is loaded, position, playing) must be
  independent of where audio is rendered.
- **Pluggable audio sinks**: local speakers and a connected TV are both just sinks. Adding
  a third sink in the future should require no changes to the player or the UI logic.
- **Bidirectional sink control**: a sink can send commands back to the player (e.g. the TV
  remote seeking). This is first-class, not an afterthought.
- **Single event loop**: the 100 ms GTK timer calls one method and reacts to events. No
  scattered state checks across closures.

---

## Architecture

### `AudioSink` trait

The only thing a sink needs to know: load a file, play from a position, pause, and
optionally push commands back.

```rust
pub trait AudioSink {
    /// Load a track. Must not start playback.
    fn load(&mut self, path: &Path, track_id: Option<i64>) -> Result<(), String>;

    /// Start or restart playback from `pos` seconds.
    fn play_from(&mut self, pos: f64);

    /// Pause playback.
    fn pause(&mut self);

    /// Poll for a pending command from this sink (e.g. TV remote seek).
    /// Called every timer tick. Returns None if nothing pending.
    fn poll_command(&mut self) -> Option<SinkCommand>;

    /// Label shown in the sink selector UI.
    fn name(&self) -> &str;
}

pub enum SinkCommand {
    Seek(f64),
    // extensible: Play, Pause, Next, …
}
```

### Implementations

| Sink | `play_from` | `poll_command` |
|---|---|---|
| `LocalSink` | rodio: reload decoder, skip_duration, sink.play() | always None |
| `TvSink` | sends `WsEvent::Stream { id, seek }` via bridge | drains `bridge.take_seek()` |

### `Player`

Owns all playback state. Delegates audio output to the active sink.

```rust
pub struct Player {
    pub file_path:     Option<PathBuf>,
    pub track_id:      Option<i64>,
    pub duration_secs: f64,
    accumulated_secs:  f64,
    play_started_at:   Option<Instant>,
    sink:              Box<dyn AudioSink>,
}
```

Key methods:

- `load(path, track_id)` — loads into sink, resets position, does not play
- `play()` — records Instant, calls `sink.play_from(current_pos)`
- `pause()` — snapshots position, calls `sink.pause()`
- `seek(pos)` — updates position, calls `sink.play_from(pos)` if playing
- `change_sink(new)` — pauses old, loads into new, resumes if was playing
- `tick() -> Vec<PlayerEvent>` — called every 100 ms; polls sink commands,
  advances position, detects track end

```rust
pub enum PlayerEvent {
    PositionChanged(f64),
    TrackEnded,
    Seeked(f64),
}
```

### `SinkRegistry`

Maintains the list of available sinks. Polled by the timer to detect TV
connect/disconnect.

```rust
pub struct SinkRegistry {
    bridge: Arc<ServerBridge>,
}

impl SinkRegistry {
    /// Returns available sinks at this moment.
    pub fn available(&self) -> Vec<SinkEntry>;

    /// Build a boxed sink for the given entry.
    pub fn build(&self, entry: &SinkEntry) -> Box<dyn AudioSink>;
}

pub struct SinkEntry {
    pub id:   &'static str,  // "local", "tv"
    pub name: String,        // "Local", "Samsung TV"
}
```

### Event flow: TV-initiated seek

```
TV remote click
  → WS message {"type":"seek","pos":45}
  → stored in bridge.seek_slot

100 ms tick
  → player.tick()
  → TvSink::poll_command() → Some(Seek(45))
  → Player::seek(45) → sink.play_from(45)   [TvSink sends WsEvent::Stream]
  → PlayerEvent::Seeked(45)
  → PlayerView: update slider + time label + broadcast WsEvent::Position
```

---

## UI changes

Current controls: `[Play] [Cue] [TV toggle]`

After redesign:
```
[Play] [Cue]
──────────────────────────────
Sink:  [● Local]  [ Samsung TV]
```

The sink row is updated every timer tick from `SinkRegistry::available()`. Selected sink
is highlighted. TV button appears on connect, disappears on disconnect. Switching sink
mid-playback is seamless via `Player::change_sink()`.

---

## TODO

### Phase 1 — core abstractions

- [ ] Create `src/audio_sink.rs`
  - [ ] Define `AudioSink` trait
  - [ ] Define `SinkCommand` enum
  - [ ] Implement `LocalSink` (extract from current `DeckState`)
  - [ ] Implement `TvSink` (extract from current TV toggle logic)
- [ ] Rewrite `src/deck.rs` as `Player`
  - [ ] Position tracking moved here from `DeckState`
  - [ ] `tick() -> Vec<PlayerEvent>`
  - [ ] `change_sink()`

### Phase 2 — sink registry

- [ ] Add `SinkRegistry` to `src/sink_registry.rs`
- [ ] Wire into `ServerBridge` (TV connect/disconnect updates registry)

### Phase 3 — UI

- [ ] Refactor `PlayerView`
  - [ ] Replace `Rc<RefCell<DeckState>>` with `Rc<RefCell<Player>>`
  - [ ] Replace TV toggle with sink selector row
  - [ ] Timer becomes: `player.tick()` → handle `Vec<PlayerEvent>`
  - [ ] Remove all scattered `tv_output` borrow checks

### Phase 4 — other player fixes (do alongside or after Phase 3)

- [ ] Duration detection: use `ffprobe` instead of `rodio` total_duration
      (fixes "0:00 / ?" and broken TV metadata duration)
- [ ] Track load resets play button to "Play" and sends `State{playing:false}`
- [ ] Playlist folder grouping: investigate and fix Seq ordering

---

## Out of scope (future)

- Multiple simultaneous sinks (local + TV at same time)
- TV-initiated play/pause (currently TV can only seek)
- Waveform display (separate ANLZ parsing work)
