# Player UI design

## Layout

```
┌────────────────────────────────────────────────────────────────┐
│ ┌──────┐  Track Title                        BPM: 128.0       │
│ │ art  │  Artist                             Key:  8A         │
│ └──────┘                                                       │
│                                                                │
│  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓│░░░░░░░░░░░░░░░░░░░░░░░░░░  -2:34          │
│                  ↑ playhead                                    │
│                                                                │
│         [ CUE ]       [ ▶ / ❚❚ ]                              │
└────────────────────────────────────────────────────────────────┘
```

## Elements

### Track info row (top)

| Element | Source | Notes |
|---|---|---|
| Album art | Audio file tags (ID3/FLAC) or Rekordbox artwork | Square, ~80×80 px; placeholder music note if unavailable |
| Title | `djmdContent.Title` | Truncate with ellipsis if too long |
| Artist | `djmdArtist.Name` | |
| BPM | `djmdContent.BPM ÷ 100` | One decimal, e.g. `128.0` |
| Key | `djmdKey.ScaleName` | Camelot notation, e.g. `8A` |

### Waveform

- Full-width `DrawingArea`, ~80 px tall
- Color waveform parsed from Rekordbox ANLZ `.EXT` file (`PWV5` tag)
- Vertical playhead line tracks current position
- Background scrolls so playhead stays centered (or fixed playhead, scrolling waveform)
- **Placeholder**: solid grey bar until ANLZ parsing is implemented

Data source: `djmdContent.AnalysisDataPath` → `PIONEER/USBANLZ/…{id}.EXT`

### Time display

- Shows **time remaining**: `duration − position`, formatted as `-M:SS`
- Displayed to the right of the waveform
- Requires reliable duration detection (see `docs/player-redesign.md` Phase 4)

### Controls row (bottom)

#### Play/Pause button
- Single button, label and icon change with playing state
- `▶ Play` when paused, `❚❚ Pause` when playing

#### Cue button

See [Cue functionality](#cue-functionality) below.

---

## Cue functionality

> **Status: placeholder only.** Button exists in the UI but full behaviour is deferred.

### What a DJ cue does

A cue point marks a specific position in the track — typically the first beat of a phrase.
Pressing Cue:
- If **stopped/paused**: jumps to the cue point. Does **not** start playback.
- If **playing**: jumps back to the cue point and **pauses** there (ready to re-launch).

This gives the DJ a "home base" to return to and relaunch from on beat.

### Cue point resolution (priority order)

1. **Stored hot cue A** — if `djmdCue` has a record for this track with `Kind = 0`
   (memory cue) or `Kind = 3` (hot cue), use the first one's `InMsec` position.
2. **First beat of beat grid** — if beat grid data is available in the ANLZ `.DAT` file
   (`PQTZ` tag), use the position of beat 1.
3. **Start of track** — fallback, position 0.

### What gets stored
Cue points are read-only for now (sourced from Rekordbox analysis). Future work: allow
setting and saving custom cues.

### Implementation notes
- `djmdCue.InMsec` → divide by 1000 for seconds
- Beat grid: parse `PQTZ` tag from `.DAT` ANLZ file (separate ANLZ work)
- The cue position should be stored in `Player` and reset on track load

---

## Data availability

| Element | Available now | Requires future work |
|---|---|---|
| Title, Artist | ✓ DB | |
| BPM, Key | ✓ DB | |
| Duration | ✗ (`queue_fn` drops `Track`, only `PathBuf` passed through) | Pass full `Track` through `queue_fn` |
| Album art | ✗ | ID3/FLAC tag extraction |
| Waveform | ✗ | ANLZ `.EXT` parsing (`PWV5` tag) |
| Cue from stored points | ✗ | `djmdCue` query + wire into Player |
| Cue from beat grid | ✗ | ANLZ `.DAT` parsing (`PQTZ` tag) |

---

## Sink selector (from player-redesign.md)

Below the controls row, a compact sink row:

```
Output:  [● Local]  [ Samsung TV]
```

Buttons update live from `SinkRegistry`. Switching mid-playback is seamless.

---

## TODO

- [ ] Wire BPM and Key into `PlayerView` (data already in DB, just not displayed)
- [ ] Replace time-elapsed label with time-remaining
- [ ] Album art: add `id3` or `audiotags` crate, extract cover, render in `gtk::Image`
- [ ] Waveform `DrawingArea`: placeholder grey bar now; ANLZ rendering later
- [ ] Cue button: resolve cue position on load (fallback chain above), jump on press
- [ ] Pass BPM, Key, Artist properly through `do_load_track` (currently only filename used)
