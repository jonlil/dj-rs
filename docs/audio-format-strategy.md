# Audio Format Strategy

## Target formats

The library normalizes all audio to three formats:

| Format | Extension | Type | Metadata |
|---|---|---|---|
| AIFF | `.aif` | Lossless (uncompressed PCM) | ID3v2 |
| MP3 | `.mp3` | Lossy | ID3v2 |
| AAC | `.m4a` | Lossy | MP4 atoms |

### Why these three?

- **AIFF** — Lossless with full hardware compatibility (all CDJs from 2009+). FLAC is smaller but not supported by CDJ-2000NXS which is still common in clubs. AIFF has ID3v2 metadata support, unlike WAV.
- **MP3** — Kept as-is for legacy files. Converting lossy→lossy degrades quality for no gain.
- **M4A/AAC** — Kept as-is for existing AAC files. Used as the transcode target for other lossy formats since AAC is a better codec than MP3 at the same bitrate.

## Import conversion rules

| Source format | Action | Reason |
|---|---|---|
| WAV | → `.aif` | Same PCM, better metadata |
| FLAC | → `.aif` | Decode lossless, gains hardware compat |
| ALAC (`.m4a`) | → `.aif` | Lossless→lossless |
| WavPack (`.wv`) | → `.aif` | Lossless→lossless |
| APE (`.ape`) | → `.aif` | Lossless→lossless |
| AIFF | keep | Already target format |
| MP3 | keep | Avoid lossy→lossy |
| M4A (AAC) | keep | Avoid lossy→lossy |
| `.aac` (raw) | → `.m4a` | Wrap in MP4 container for metadata support |
| OGG Vorbis | → `.m4a` | AAC is better codec at same bitrate |
| Opus | → `.m4a` | Same reasoning |
| WMA | → `.m4a` | Legacy Windows format |

### ALAC detection

ALAC and AAC both use `.m4a` extension. The import pipeline must detect the codec to decide whether to convert to AIFF (lossless) or keep as-is (lossy AAC).

## Metadata strategy

All three target formats support rich metadata via lofty:

- **AIFF / MP3** — ID3v2 tags. Custom fields via TXXX frames (ISRC, AcoustID, MusicBrainz Recording ID).
- **M4A** — MP4 atoms. Custom fields via `----:com.apple.iTunes:*` atoms.

Metadata is always written to both:
1. The audio file tags
2. The Rekordbox database

## Hardware compatibility reference

See [DJ hardware format support](../docs/dj-hardware-formats.md) or the CLAUDE.md for per-model details.

Summary:
- **WAV + AIFF + MP3 + AAC** — all CDJs from 2009 onward
- **FLAC** — CDJ-2000NXS2+, CDJ-3000, all Denon Engine OS players, but NOT CDJ-2000NXS
- **ALAC** — CDJ-2000NXS2+, CDJ-3000, Denon, but limited XDJ support
