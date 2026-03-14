# dj-rs

A DJ application written in Rust with a GTK3 desktop UI, Rekordbox library browser,
and a built-in HTTP/WebSocket server for streaming audio to a Samsung Tizen TV.

## Features

- Browse and filter your Rekordbox library (playlists, history, search, BPM/key filter)
- Deck player with waveform placeholder, cue point, and time-remaining display
- Audio streaming to a connected Samsung TV via WebSocket + ffmpeg AAC transcoding
- Path mapping to remap Rekordbox file paths to local mount points

## Building

See [docs/build.md](docs/build.md) for full build and deploy instructions.

```bash
cargo run
```

## Disclaimer

**This project is not affiliated with Pioneer DJ Corp. or its related companies in any
way and has been developed independently.**

dj-rs reads the Rekordbox local SQLite database (`master.db`). This database is
encrypted with SQLCipher using a key that is publicly known from community
reverse-engineering efforts (see projects such as
[pyrekordbox](https://github.com/dylanljones/pyrekordbox)). The key is present on
any machine running Rekordbox and is used here solely to read your own locally stored
data in a read-mostly fashion.

Using this software may be subject to the Rekordbox End User License Agreement.
You are responsible for ensuring your use complies with the terms of any applicable
EULA. **Always back up your Rekordbox collection before using third-party tools that
access the database.**

The maintainers are not liable for any damage to your Rekordbox library or data.

## License

MIT — see [LICENSE](LICENSE).
