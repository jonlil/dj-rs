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

| Source format | Target | Pipeline | Reason |
|---|---|---|---|
| WAV | `.aif` | symphonia → aifc | Same PCM, better metadata |
| FLAC | `.aif` | symphonia → aifc | Decode lossless, gains hardware compat |
| ALAC (`.m4a`) | `.aif` | symphonia → aifc | Lossless→lossless |
| WavPack (`.wv`) | `.aif` | symphonia → aifc | Lossless→lossless |
| APE (`.ape`) | `.aif` | ape-decoder → aifc | Lossless→lossless |
| OGG Vorbis | `.m4a` | symphonia → fdk-aac | Lossy→lossy (better codec) |
| Opus | `.m4a` | symphonia → fdk-aac | Lossy→lossy (better codec) |
| `.aac` (raw) | `.m4a` | MP4 container wrap | Add container for metadata |
| AIFF | keep | — | Already target format |
| MP3 | keep | — | Avoid lossy→lossy |
| M4A (AAC) | keep | — | Avoid lossy→lossy |

### Not supported

- **WMA** — No pure Rust decoder available. Extremely rare in DJ libraries.

### ALAC detection

ALAC and AAC both use `.m4a` extension. The import pipeline must detect the codec to decide whether to convert to AIFF (lossless) or keep as-is (lossy AAC).

### Bit depth

Source bit depth is preserved during lossless conversion. A 24-bit FLAC becomes 24-bit AIFF. A 16-bit WAV becomes 16-bit AIFF.

## Transcode dependencies

| Crate | Type | Binary overhead | Purpose |
|---|---|---|---|
| `aifc` | pure Rust | negligible | Write AIFF files |
| `symphonia` | pure Rust | already in project | Decode WAV/FLAC/ALAC/WavPack/OGG/Opus |
| `ape-decoder` | pure Rust | ~50 KB | Decode APE (Monkey's Audio) |
| `fdk-aac` | C++ (compiled in) | ~500 KB–1 MB | AAC encoding (Fraunhofer) |

No runtime dependencies. All libraries compile into the binary. Cross-platform (Linux, macOS, Windows).

## Metadata strategy

All three target formats support rich metadata via lofty:

- **AIFF / MP3** — ID3v2 tags. Custom fields via TXXX frames (ISRC, AcoustID, MusicBrainz Recording ID).
- **M4A** — MP4 atoms. Custom fields via `----:com.apple.iTunes:*` atoms.

Metadata is always written to both:
1. The audio file tags
2. The Rekordbox database

## Hardware compatibility

Summary (see docs/ memory for full per-model table):
- **WAV + AIFF + MP3 + AAC** — all CDJs from 2009 onward
- **FLAC** — CDJ-2000NXS2+, CDJ-3000, all Denon Engine OS players, but NOT CDJ-2000NXS
- **ALAC** — CDJ-2000NXS2+, CDJ-3000, Denon, but limited XDJ support
