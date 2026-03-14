# Player Queue

## Goals

- A DJ should be able to mark one track as **Next Up** while the current track is still playing.
- The queue is intentionally shallow: one confirmed "next" slot, not a deep playlist.
  A DJ works track-by-track and needs to change their mind at the last second — a long
  auto-advancing playlist would fight that workflow.
- The queued track should be visible in both the player UI and the browser list so there is
  no ambiguity about what will play next.

---

## Concepts

### Next Up slot

A single `Option<Track>` held by the player. When the current track ends, the Next Up track
is loaded and playback starts automatically.

Setting a new Next Up track replaces whatever was there before (no confirmation needed —
the DJ is making an active decision).

Clearing Next Up (e.g. pressing a cancel button) returns to manual mode: the track ends and
the player stops.

### Manual mode vs queued mode

| State | After current track ends |
|---|---|
| No Next Up set | Player stops, waiting for manual action |
| Next Up is set | Player loads and plays the queued track |

---

## Player UI changes

```
┌────────────────────────────────────────────────────────────────┐
│ ┌──────┐  Track Title                        BPM: 128.0       │
│ │ art  │  Artist                             Key:  8A         │
│ └──────┘                                                       │
│  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓│░░░░░░░░░░░░░░░░░░░░░░░░░░  -2:34          │
│         [ CUE ]       [ ▶ / ❚❚ ]                              │
├────────────────────────────────────────────────────────────────┤
│ NEXT UP: Artist — Track Title                         [ × ]   │
└────────────────────────────────────────────────────────────────┘
```

### Next Up row (bottom of player frame)

- Shown only when a track is queued; hidden otherwise.
- Format: `NEXT UP: {Artist} — {Title}`  (artist may be empty → just the title)
- `[ × ]` button clears the slot.
- The row uses a distinct background (e.g. slightly tinted) so it reads as a separate zone.

---

## Browser integration

### "Queue next" action

Right-click a track → **"Queue next"** (rename the existing "Queue" item, or add it alongside).

When triggered:
1. Fills the Next Up slot on the active player.
2. Highlights the row in the browser with a marker (e.g. a colored dot in a status column,
   or row background tint) so the DJ can see at a glance which track is queued.
3. If another track was already in the Next Up slot, its highlight is cleared first.

### Status column (future)

Add a narrow hidden-by-default column to the track list:

| Symbol | Meaning |
|---|---|
| `▶` | Currently playing |
| `→` | Next Up |

---

## Data model

```rust
pub struct Player {
    // ... existing fields ...
    pub next_up: Option<Track>,
}
```

`next_up` is set by the UI when the DJ queues a track. `tick()` checks it on track end:

```rust
fn on_track_ended(&mut self) -> Vec<PlayerEvent> {
    let mut events = vec![PlayerEvent::TrackEnded];
    if let Some(track) = self.next_up.take() {
        self.load_and_play(track);
        events.push(PlayerEvent::TrackLoaded);
    }
    events
}
```

---

## Interaction with the sink / TV output

When the Next Up track auto-loads:
- If TV output is active: send `WsEvent::Stream { id, seek: 0.0 }` immediately.
- Send `WsEvent::Metadata` and `WsEvent::State { playing: true }` as with any load + play.

---

## What is out of scope (for now)

- **Queue depth > 1**: deliberately not supported. The DJ picks one next track.
- **Saving the queue across app restarts**: volatile, lives only in memory.
- **Reordering a queue**: there is nothing to reorder with a single slot.
- **Multiple decks sharing one queue**: each `PlayerView` has its own Next Up slot.

---

## TODO

- [ ] Add `next_up: Option<Track>` to player state (`DeckState` or future `Player`)
- [ ] Add Next Up row widget to `PlayerView` (hidden by default, shown when slot is filled)
- [ ] Add `[ × ]` button wired to clear the slot and hide the row
- [ ] Rename/add "Queue next" to the browser right-click context menu
- [ ] On `on_track_end` callback: if Next Up is set, call `do_load_track(track)` + `play()`
- [ ] Browser status column: show `→` next to the queued track row
- [ ] Clear the browser highlight when Next Up is consumed or cancelled
