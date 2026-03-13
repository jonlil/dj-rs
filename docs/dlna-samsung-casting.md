# DLNA Casting to Samsung TV

## Problem: Samsung Q-series audio DMR is broken

Samsung Q-series TVs (tested: QE55Q8DNA) expose a DLNA renderer (DMR) via
AVTransport. When you push an audio-only URI via `SetAVTransportURI` + `Play`,
the TV accepts the SOAP commands with `200 OK`, sets
`CurrentTransportStatus: OK`, but the transport state never leaves
`TRANSITIONING`. The audio never plays. This is a firmware bug — the TV's
audio-only DMR is broken for external DMC push.

Things that do **not** fix it:
- Returning 200 instead of 206 for range requests
- Including or omitting `contentFeatures.dlna.org` header
- Various `DLNA.ORG_FLAGS` combinations
- Using `audio/mpeg` vs `audio/x-mpeg`
- Serving raw MP3 vs WAV vs FLAC
- Using a ContentDirectory with DIDL-Lite metadata
- Different transfer modes (`Streaming` / `Interactive` / `Background`)

## Solution: MPEG-TS video container via ffmpeg

Wrap the audio in an MPEG-TS container with a dummy black video track. The
TV's video decoder handles this correctly and playback works.

### ffmpeg command

```
ffmpeg \
  -i <audio_file> \
  -f lavfi -i color=c=black:size=352x288:rate=25 \
  -map 1:v -map 0:a \
  -c:v mpeg2video -b:v 500k -r 25 \
  -c:a mp2 -b:a 192k \
  -shortest \
  -f mpegts \
  pipe:1
```

- `-shortest` ensures the muxer stops when the audio ends
- `pipe:1` streams to stdout, which is piped as the HTTP response body
- `352x288` is a valid SD resolution for `MPEG_TS_SD_EU_ISO`

### HTTP headers

```
Content-Type: video/mpeg
contentFeatures.dlna.org:
  DLNA.ORG_PN=MPEG_TS_SD_EU_ISO;
  DLNA.ORG_OP=01;
  DLNA.ORG_CI=1;
  DLNA.ORG_FLAGS=ED100000000000000000000000000000
```

- `DLNA.ORG_PN=MPEG_TS_SD_EU_ISO` — profile that the Samsung TV recognises
- `DLNA.ORG_CI=1` — signals transcoded content
- `DLNA.ORG_FLAGS` — `ED10...` = server-side pacing, limited operations, dlna v1.5

### DIDL-Lite metadata (for SetAVTransportURI)

```xml
<item id="1" parentID="0" restricted="1">
  <dc:title>Track Title</dc:title>
  <res protocolInfo="http-get:*:video/mpeg:DLNA.ORG_PN=MPEG_TS_SD_EU_ISO;DLNA.ORG_OP=01;DLNA.ORG_CI=1"
       duration="0:04:30.000">
    http://<lan_ip>:7878/track.ts
  </res>
  <upnp:class>object.item.videoItem</upnp:class>
</item>
```

Use `object.item.videoItem`, not `object.item.audioItem` — the TV routes on
the UPnP class.

## Architecture in dlna.rs

### Components

1. **SSDP `ssdp_alive` broadcast** — announces the app as both a
   `MediaRenderer:1` and a `MediaServer:1 / ContentDirectory:1` device, so the
   TV can discover it. Sends 4 NOTIFY packets:
   - `upnp:rootdevice`
   - `uuid:<UUID>`
   - `urn:schemas-upnp-org:device:MediaServer:1`
   - `urn:schemas-upnp-org:service:ContentDirectory:1`

2. **Axum HTTP server** (tokio, port 7878) — spawned once in a background
   tokio runtime. Routes:
   - `GET /track.ts` — spawns ffmpeg, streams stdout as `StreamBody`
   - `GET /device.xml` — UPnP device description XML
   - `GET /cd.xml` — ContentDirectory SCPD XML
   - `POST /cd` — ContentDirectory Browse SOAP (returns DIDL-Lite)
   - `POST /cd/events` — no-op subscription endpoint

3. **`DlnaClient`** — synchronous wrapper using `tokio::runtime::Runtime`
   methods. Key methods:
   - `discover_renderers()` — SSDP M-SEARCH, parses location + friendly name
   - `start_http_server(path)` — starts Axum server, returns `http://lan_ip:7878/track.ts`
   - `play_on_renderer(location, url)` — SetAVTransportURI + Play
   - `set_uri_on_renderer(location, url)` — SetAVTransportURI only
   - `resume_renderer(location)` — Play (resumes after pause)
   - `pause_renderer(location)` — Pause
   - `stop_renderer(location)` — Stop
   - `stop_http_server()` — shuts down the Axum server

### LAN IP detection

```rust
pub fn lan_ip() -> Result<IpAddr, String>
```

Prefers `192.168.x.x` addresses over VPN tunnel interfaces. The HTTP server
must advertise a LAN IP, not a VPN IP, for the TV to reach it.

`ssdp_blocked_by_vpn()` checks if the default route is a VPN — if so, the UI
shows a warning before discovery.

## Troubleshooting

| Symptom | Cause |
|---|---|
| TV stays `TRANSITIONING` | Audio-only content pushed to Samsung DMR — use MPEG-TS wrapper |
| `ERROR_OCCURRED` after `Play` | Wrong container format (e.g. MPEG-PS `-f vob`) — use `-f mpegts` |
| TV shows "file format not supported" | Wrong DLNA profile or MIME type |
| Discovery finds no renderers | VPN is active and blocking multicast — disable VPN or continue anyway |
| Cast works but no audio | ffmpeg not installed, or audio codec issue — verify `mp2` encoding works |
