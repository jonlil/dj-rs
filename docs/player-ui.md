# Player UI

## Layout

```
┌────────────────────────────────────────────────────────────────────────┐
│ ┌──────┐  Track Title              ♦ LIBRARY      128.0 BPM            │
│ │ art  │  Artist                                  8A        -3:45       │
│ └──────┘                                                                │
│ ┌────────┬────────────────────────────────────────┬──────────────────┐  │
│ │ (CUE)  │                                        │ A  00:12.3       │  │
│ │        │   zoomed waveform (6s window)          │ B  01:45.0       │  │
│ │  (▶)   │                                        │ C  ──            │  │
│ │        │────────────────────────────────────────│ D  ──            │  │
│ │  Out   │   overview (full track)                │ E  ──            │  │
│ │ [L][TV]│                                        │ F  ──            │  │
│ └────────┴────────────────────────────────────────┴──────────────────┘  │
│  [ A ]  [ B ]  [ C ]  [ D ]  [ E ]  [ F ]  [ G ]  [ H ]                │
└────────────────────────────────────────────────────────────────────────┘
```

## Columns

| Column | Width | Contents |
|---|---|---|
| Left | 90px fixed | CUE button, Play/Pause button, output selector |
| Center | expands | Zoomed waveform + overview stacked |
| Right | 130px fixed | Cue list panel (8 rows) |

---

## Info row

| Element | Source |
|---|---|
| Album art | Rekordbox image path (`djmdContent`), loaded in background thread |
| Title | `djmdContent.Title` |
| Artist | `djmdArtist.Name` |
| Source badge | `"♦ LIBRARY"` for local tracks, `"♫ SPOTIFY"` for librespot |
| BPM | `djmdContent.BPM ÷ 100`, one decimal |
| Key | `djmdKey.ScaleName` (Camelot notation, e.g. `8A`) |
| Time remaining | `duration − position`, formatted as `-M:SS` |

---

## Left column

### CUE button
- Round button (`border-radius: 22px; min-width: 52px; min-height: 44px`)
- **Paused**: jumps to the stored cue position (does not start playback)
- **Playing**: jumps to stored cue position and pauses (ready to re-launch)
- Cue position set from `djmdCue` `kind=0` (memory cue) on track load; defaults to 0

### Play/Pause button
- Round, same dimensions as CUE
- Label: `▶` when paused, `❚❚` when playing
- When Spotify is active: controls librespot play/pause instead of the local deck

### Output selector
- `● Loc` — local rodio sink (default)
- `TV` — Samsung TV via WebSocket; only sensitive when at least one TV client is connected
- Mutually exclusive toggles; switching mid-playback is seamless
- If the TV disconnects while active, automatically falls back to local

---

## Waveform area

### Zoomed waveform
- 120px tall, full width of center column
- Shows a 6-second window (`ZOOM_WINDOW = 6.0`); playhead at 25% from left (`PLAYHEAD_FRAC = 0.25`)
- CDJ-style 3-band color rendering from PWV7 data (`.2EX` ANLZ file):
  - Bass: warm amber
  - Mid: lime green (80% of bass height)
  - High: steel blue (60% of bass height)
  - Whiteness (upper 3 bits) fades toward white
- Playhead: white vertical line + red downward triangle at top
- Cue markers: colored vertical lines; loop regions as tinted rectangles
- **Interaction (CDJ scratch)**: drag left/right to scrub; deck pauses on press, resumes on release if it was playing
- Redraws at 60fps via a dedicated 16ms glib timer

### Overview waveform
- 32px tall, full width of center column
- Pre-rendered to a `cairo::ImageSurface` once per track load (same 3-band colors)
- Darker overlay left of playhead (played portion); white position marker + red triangle; colored cue ticks
- Falls back to a grey centre line when no waveform data; marker always shown
- **Interaction**: click or drag to seek; TV `WsEvent::Stream` sent on release with 300ms debounce

---

## Cue list panel

Eight rows, one per hot cue slot (A–H). Each row shows:
- Colored letter matching `HOT_CUE_COLORS` palette
- Timestamp (`M:SS.s`) of the cue position
- Shows `──` when the slot has no cue

Clicking a row seeks the deck to that cue position.
Rows are insensitive when the slot is empty.

---

## Hot cue trigger row

Eight buttons labeled A–H below the main 3-column area, colored with the slot color. Clicking jumps to the stored cue position (same as clicking the cue list row).

Color palette (`HOT_CUE_COLORS`):

| Slot | Color |
|---|---|
| A | Red |
| B | Orange |
| C | Yellow |
| D | Green |
| E | Cyan |
| F | Blue |
| G | Purple |
| H | Light grey |

---

## Cue functionality

Cue points are read from `djmdCue` on track load:

- `kind = 0` — memory cue: used as the CUE button target position
- `kind = 1–8` — hot cues: stored in `waveform_cues`, shown in cue list and trigger row

CUE button behavior:
- **Paused at cue**: stays at cue position (no play)
- **Paused elsewhere**: jumps to cue position
- **Playing**: jumps to cue and pauses (CDJ "back to cue" behavior)

---

## TV / WebSocket output

See `docs/tizen-app.md` for the Tizen app. From the player's perspective:

- `WsEvent::Metadata` — sent on track load
- `WsEvent::Position` — sent every 100ms tick while playing
- `WsEvent::State` — sent on play/pause changes
- `WsEvent::Stream { id, seek }` — tells TV to fetch `/stream/{id}?seek=N`; sent when play starts or overview seek completes (debounced)
- Full state (Metadata + Position + State + Stream if playing) pushed to TV on toggle or on new client connect

When TV output is active, the local rodio sink volume is set to 0.0 (muted).
