# Rekordbox 6 Database Schema (`master.db`)

This document describes the full schema of the encrypted SQLite database used by Rekordbox 6 (`master.db`).
The database is encrypted with SQLCipher using PBKDF2-HMAC-SHA512 key derivation.

**Database version:** 6000
**Source:** extracted from a real Rekordbox 6.5.1 library (macOS) with 2528 tracks
**Extraction method:** pyrekordbox + SQLAlchemy ORM inspection

---

## Encryption Parameters

```sql
PRAGMA key = '<hex key>';
PRAGMA cipher_page_size = 4096;
PRAGMA kdf_iter = 256000;
PRAGMA cipher_hmac_algorithm = HMAC_SHA512;
PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;
```

---

## Table Overview

| Table | Rows (sample DB) | Purpose |
|---|---|---|
| `djmdContent` | 2528 | Main track/content metadata |
| `djmdArtist` | 2373 | Artist lookup table |
| `djmdAlbum` | 1703 | Album lookup table |
| `djmdGenre` | 172 | Genre lookup table |
| `djmdLabel` | 369 | Record label lookup table |
| `djmdKey` | 59 | Musical key lookup table |
| `djmdColor` | 8 | Track color labels |
| `djmdCue` | 1078 | Cue/loop/hot-cue points per track |
| `djmdMixerParam` | 2528 | Per-track gain/peak analysis data |
| `djmdPlaylist` | 358 | Playlist nodes (folders, regular, smart) |
| `djmdSongPlaylist` | 5900 | Track-to-playlist membership |
| `djmdHistory` | 335 | DJ session history nodes |
| `djmdSongHistory` | 5513 | Track-to-history-session membership |
| `djmdMyTag` | 29 | User-defined tag categories and values |
| `djmdSongMyTag` | 32 | Track-to-MyTag assignments |
| `djmdHotCueBanklist` | 0 | Hot cue bank lists |
| `djmdSongHotCueBanklist` | 0 | Track-to-hot-cue-bank membership |
| `djmdRelatedTracks` | 4 | Related Tracks filter presets |
| `djmdSongRelatedTracks` | 0 | Track-to-related-tracks |
| `djmdSampler` | 5 | Sampler folder hierarchy |
| `djmdSongSampler` | 0 | Track-to-sampler |
| `djmdSongTagList` | 0 | Tag list (purpose unclear, always empty) |
| `djmdMenuItems` | 27 | Browser menu item definitions |
| `djmdCategory` | 21 | Browser category configuration |
| `djmdSort` | 17 | Browser sort order configuration |
| `djmdDevice` | 1 | Rekordbox device/library identity |
| `djmdProperty` | 1 | Database-level properties |
| `djmdCloudProperty` | 1 | Cloud sync properties |
| `djmdSharedPlaylist` | 0 | Shared playlist metadata |
| `djmdSharedPlaylistUser` | 0 | Shared playlist user membership |
| `djmdRecommendLike` | 0 | Recommendation likes |
| `contentCue` | 723 | Cloud-synced cue data (JSON blob) |
| `contentActiveCensor` | 0 | Cloud-synced active censor data |
| `contentFile` | 9506 | Cloud-synced file paths and hashes |
| `hotCueBanklistCue` | 0 | Cloud-synced hot-cue-bank cue blobs |
| `imageFile` | 0 | Cloud-synced image file paths |
| `settingFile` | 4 | Cloud-synced settings files |
| `agentNotification` | 3 | Push notifications from Pioneer cloud |
| `agentNotificationLog` | 0 | Notification delivery log |
| `agentRegistry` | 26 | Local agent key-value store |
| `cloudAgentRegistry` | 0 | Cloud agent key-value store |
| `uuidIDMap` | 0 | UUID-to-ID mapping (cloud sync) |

---

## Common Column Patterns (Mixins)

Every `djmd*` table (except `djmdProperty`) and every `content*` / `imageFile` / `settingFile` / `hotCueBanklistCue` / `uuidIDMap` table carries a set of standard sync-tracking columns:

| Column | Type | Description |
|---|---|---|
| `UUID` | `VARCHAR(255)` | Universally unique identifier for cloud sync. Format is standard UUID v4. |
| `rb_data_status` | `INTEGER` | Sync status bitmask. `0` = local only / not synced, `256` = synced to cloud (base), `257`/`258`/`262` = synced with modifications. |
| `rb_local_data_status` | `INTEGER` | Local sync status; typically 0. |
| `rb_local_deleted` | `TINYINT(1)` | `1` if row is marked for deletion but not yet purged; `0` otherwise. |
| `rb_local_synced` | `TINYINT(1)` | `1` if row has been synced to cloud; `0` otherwise. |
| `usn` | `BIGINT` | Update Sequence Number — server-side version counter for cloud sync. |
| `rb_local_usn` | `BIGINT` | Local USN — local version counter. |
| `created_at` | `DATETIME` | Row creation timestamp. Format: `YYYY-MM-DD HH:MM:SS.mmm +00:00`. |
| `updated_at` | `DATETIME` | Row last-update timestamp. Same format as `created_at`. |

The `ParentID` column in tree tables uses the string literal `"root"` for top-level nodes.

---

## Core Music Tables

### `djmdContent` — Track Metadata

The central table. Every imported track has exactly one row. Foreign-key IDs all use `VARCHAR(255)` even though the underlying values are large integers stored as decimal strings (e.g. `"27898494"`).

```sql
CREATE TABLE `djmdContent` (
  `ID`               VARCHAR(255) PRIMARY KEY,
  `FolderPath`       VARCHAR(255) DEFAULT NULL,
  `FileNameL`        VARCHAR(255) DEFAULT NULL,
  `FileNameS`        VARCHAR(255) DEFAULT NULL,
  `Title`            VARCHAR(255) DEFAULT NULL,
  `ArtistID`         VARCHAR(255) DEFAULT NULL,  -- FK -> djmdArtist.ID
  `AlbumID`          VARCHAR(255) DEFAULT NULL,  -- FK -> djmdAlbum.ID
  `GenreID`          VARCHAR(255) DEFAULT NULL,  -- FK -> djmdGenre.ID
  `BPM`              INTEGER      DEFAULT NULL,
  `Length`           INTEGER      DEFAULT NULL,
  `TrackNo`          INTEGER      DEFAULT NULL,
  `BitRate`          INTEGER      DEFAULT NULL,
  `BitDepth`         INTEGER      DEFAULT NULL,
  `Commnt`           TEXT         DEFAULT NULL,
  `FileType`         INTEGER      DEFAULT NULL,
  `Rating`           INTEGER      DEFAULT NULL,
  `ReleaseYear`      INTEGER      DEFAULT NULL,
  `RemixerID`        VARCHAR(255) DEFAULT NULL,  -- FK -> djmdArtist.ID
  `LabelID`          VARCHAR(255) DEFAULT NULL,  -- FK -> djmdLabel.ID
  `OrgArtistID`      VARCHAR(255) DEFAULT NULL,  -- FK -> djmdArtist.ID
  `KeyID`            VARCHAR(255) DEFAULT NULL,  -- FK -> djmdKey.ID
  `StockDate`        VARCHAR(255) DEFAULT NULL,
  `ColorID`          VARCHAR(255) DEFAULT NULL,  -- FK -> djmdColor.ID
  `DJPlayCount`      INTEGER      DEFAULT NULL,
  `ImagePath`        VARCHAR(255) DEFAULT NULL,
  `MasterDBID`       VARCHAR(255) DEFAULT NULL,
  `MasterSongID`     VARCHAR(255) DEFAULT NULL,
  `AnalysisDataPath` VARCHAR(255) DEFAULT NULL,
  `SearchStr`        VARCHAR(255) DEFAULT NULL,
  `FileSize`         INTEGER      DEFAULT NULL,
  `DiscNo`           INTEGER      DEFAULT NULL,
  `ComposerID`       VARCHAR(255) DEFAULT NULL,  -- FK -> djmdArtist.ID
  `Subtitle`         VARCHAR(255) DEFAULT NULL,
  `SampleRate`       INTEGER      DEFAULT NULL,
  `DisableQuantize`  INTEGER      DEFAULT NULL,
  `Analysed`         INTEGER      DEFAULT NULL,
  `ReleaseDate`      VARCHAR(255) DEFAULT NULL,
  `DateCreated`      VARCHAR(255) DEFAULT NULL,
  `ContentLink`      INTEGER      DEFAULT NULL,
  `Tag`              VARCHAR(255) DEFAULT NULL,
  `ModifiedByRBM`    VARCHAR(255) DEFAULT NULL,
  `HotCueAutoLoad`   VARCHAR(255) DEFAULT NULL,
  `DeliveryControl`  VARCHAR(255) DEFAULT NULL,
  `DeliveryComment`  VARCHAR(255) DEFAULT NULL,
  `CueUpdated`       VARCHAR(255) DEFAULT NULL,
  `AnalysisUpdated`  VARCHAR(255) DEFAULT NULL,
  `TrackInfoUpdated` VARCHAR(255) DEFAULT NULL,
  `Lyricist`         VARCHAR(255) DEFAULT NULL,  -- FK -> djmdArtist.ID
  `ISRC`             VARCHAR(255) DEFAULT NULL,
  `SamplerTrackInfo` INTEGER      DEFAULT NULL,
  `SamplerPlayOffset`INTEGER      DEFAULT NULL,
  `SamplerGain`      FLOAT        DEFAULT NULL,
  `VideoAssociate`   VARCHAR(255) DEFAULT NULL,
  `LyricStatus`      INTEGER      DEFAULT NULL,
  `ServiceID`        INTEGER      DEFAULT NULL,
  `OrgFolderPath`    VARCHAR(255) DEFAULT NULL,
  `Reserved1`        TEXT         DEFAULT NULL,
  `Reserved2`        TEXT         DEFAULT NULL,
  `Reserved3`        TEXT         DEFAULT NULL,
  `Reserved4`        TEXT         DEFAULT NULL,
  `ExtInfo`          TEXT         DEFAULT NULL,
  `rb_file_id`       VARCHAR(255) DEFAULT NULL,
  `DeviceID`         VARCHAR(255) DEFAULT NULL,  -- FK -> djmdDevice.ID
  `rb_LocalFolderPath` VARCHAR(255) DEFAULT NULL,
  `SrcID`            VARCHAR(255) DEFAULT NULL,
  `SrcTitle`         VARCHAR(255) DEFAULT NULL,
  `SrcArtistName`    VARCHAR(255) DEFAULT NULL,
  `SrcAlbumName`     VARCHAR(255) DEFAULT NULL,
  `SrcLength`        INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

#### Column Details

| Column | Notes |
|---|---|
| `ID` | Decimal integer stored as string. Unique across the database. Also used as `ContentID` in related tables. |
| `FolderPath` | Absolute path of the audio file on the source volume (e.g. `/Volumes/muzika/iTunes/...`). |
| `FileNameL` | Long filename including extension (e.g. `Problem (Intro).mp3`). |
| `FileNameS` | Short/8.3 filename. In practice often an empty string for modern files. |
| `Title` | Track title. |
| `ArtistID` | → `djmdArtist.ID`. NULL if no artist. |
| `AlbumID` | → `djmdAlbum.ID`. NULL if no album. |
| `GenreID` | → `djmdGenre.ID`. NULL if no genre. |
| `BPM` | BPM **multiplied by 100**. E.g. `10300` = 103.00 BPM, `9800` = 98.00 BPM. |
| `Length` | Track duration in **whole seconds**. |
| `TrackNo` | Track number within the album. `0` if not set. |
| `BitRate` | Audio bitrate in kbps (e.g. `320`, `256`). |
| `BitDepth` | Audio bit depth (e.g. `16`, `24`). |
| `Commnt` | Free-text comment field. |
| `FileType` | Integer enum: `1`=MP3, `4`=M4A (AAC), `5`=FLAC, `11`=WAV, `12`=AIFF/AIF. |
| `Rating` | User star rating: `0`=no rating, `1`–`5`=stars. |
| `ReleaseYear` | Four-digit release year. `0` if not set. |
| `RemixerID` | → `djmdArtist.ID`. NULL if no remixer. |
| `LabelID` | → `djmdLabel.ID`. NULL if no label. |
| `OrgArtistID` | → `djmdArtist.ID`. Original artist (for remixes). |
| `KeyID` | → `djmdKey.ID`. Musical key (Camelot / traditional notation). NULL if not analyzed. |
| `StockDate` | Date string, format `YYYY-MM-DD`, representing when the track was added to the collection. |
| `ColorID` | → `djmdColor.ID`. NULL = no color, `1`=Pink, `2`=Red, `3`=Orange, `4`=Yellow, `5`=Green, `6`=Aqua, `7`=Blue, `8`=Purple. |
| `DJPlayCount` | Number of times the track has been played. |
| `ImagePath` | Relative path (from USB root or Pioneer data root) to the embedded artwork JPEG, e.g. `/PIONEER/Artwork/08e/32a79-.../artwork.jpg`. Split into subdirectories based on the first 3 chars of the UUID. |
| `MasterDBID` | Integer ID of the source Rekordbox database device (`djmdProperty.DBID`). |
| `MasterSongID` | Sequential integer assigned when the track is first imported. |
| `AnalysisDataPath` | Relative path to the `.DAT` analysis file, e.g. `/PIONEER/USBANLZ/08e/32a79-.../ANLZ0000.DAT`. |
| `SearchStr` | Denormalized search string; NULL in practice (search is done by other means). |
| `FileSize` | File size in bytes. |
| `DiscNo` | Disc number within a multi-disc album. `0` if not set. |
| `ComposerID` | → `djmdArtist.ID`. NULL if no composer. |
| `Subtitle` | Subtitle/version string (e.g. "Radio Edit"). Often empty. |
| `SampleRate` | Audio sample rate in Hz (e.g. `44100`, `48000`). |
| `DisableQuantize` | If non-NULL, quantize is disabled for this track. NULL = use global setting. |
| `Analysed` | Analysis state integer. `72` = partially analyzed, `105` = fully analyzed (waveform + beat grid + key). |
| `ReleaseDate` | Full release date string, format `YYYY-MM-DD`. |
| `DateCreated` | Date the track was added to Rekordbox, format `YYYY-MM-DD`. |
| `ContentLink` | Integer; purpose unclear. May encode a link type or sync state. Observed values: `94`, `787982`, `1550`, `2885134`, `2950670`. |
| `Tag` | Legacy tag field; rarely populated. |
| `ModifiedByRBM` | Flag string; set when track was modified by Rekordbox Mobile. |
| `HotCueAutoLoad` | String `"on"` if hot cues auto-load when the track is loaded onto a CDJ; absent/NULL otherwise. |
| `DeliveryControl` | Cloud delivery control flag. |
| `DeliveryComment` | Cloud delivery comment. |
| `CueUpdated` | Counter string incremented each time cue points are saved. Observed values: `"2"`, `"3"`, `"4"`. |
| `AnalysisUpdated` | Counter string incremented each time analysis is re-run. Observed values: `"2"`. |
| `TrackInfoUpdated` | Counter string incremented each time track metadata is edited. Observed values: `"1"`, `"2"`, `"3"`, `"4"`. |
| `Lyricist` | → `djmdArtist.ID`. Lyricist credit. |
| `ISRC` | ISRC code string (e.g. `"SE5HU1600101"`, `"USUM71502647"`). NULL if not set. |
| `SamplerTrackInfo` | Integer bitmask for sampler track flags. `0` = not a sampler track. |
| `SamplerPlayOffset` | Playback offset in milliseconds for sampler use. |
| `SamplerGain` | Floating-point gain multiplier for sampler use. |
| `VideoAssociate` | Path or ID of an associated video file. |
| `LyricStatus` | Lyric data status. `0`=none, `1`=loading, `10`–`15`=various loaded/error states. |
| `ServiceID` | `0` for locally-imported tracks; non-zero for streaming service tracks. |
| `OrgFolderPath` | Original folder path before any relocation. Often equals `FolderPath`. |
| `Reserved1`–`Reserved4` | Reserved for future use. |
| `ExtInfo` | Extended JSON metadata. Often `"null"` (the string). |
| `rb_file_id` | Rekordbox-internal file identifier (decimal integer string). |
| `DeviceID` | → `djmdDevice.ID` (UUID string). |
| `rb_LocalFolderPath` | Local folder path as seen by this Rekordbox instance. |
| `SrcID` | Source ID for streaming / cloud-imported tracks. |
| `SrcTitle` | Source title from streaming service. |
| `SrcArtistName` | Source artist name from streaming service. |
| `SrcAlbumName` | Source album name from streaming service. |
| `SrcLength` | Source track length in seconds from streaming service. |

---

### `djmdArtist` — Artist Lookup

```sql
CREATE TABLE `djmdArtist` (
  `ID`        VARCHAR(255) PRIMARY KEY,
  `Name`      VARCHAR(255) DEFAULT NULL,
  `SearchStr` VARCHAR(255) DEFAULT NULL,
  -- + standard sync columns
)
```

| Column | Notes |
|---|---|
| `ID` | Decimal integer string. Referenced by `djmdContent.ArtistID`, `RemixerID`, `OrgArtistID`, `ComposerID`, `Lyricist`; also `djmdAlbum.AlbumArtistID`. |
| `Name` | Artist display name (e.g. `"Ariana Grande ft. Iggy Azalea"`). |
| `SearchStr` | Phonetic/normalized search string. NULL in practice. |

---

### `djmdAlbum` — Album Lookup

```sql
CREATE TABLE `djmdAlbum` (
  `ID`            VARCHAR(255) PRIMARY KEY,
  `Name`          VARCHAR(255) DEFAULT NULL,
  `AlbumArtistID` VARCHAR(255) DEFAULT NULL,  -- FK -> djmdArtist.ID
  `ImagePath`     VARCHAR(255) DEFAULT NULL,
  `Compilation`   INTEGER      DEFAULT NULL,
  `SearchStr`     VARCHAR(255) DEFAULT NULL,
  -- + standard sync columns
)
```

| Column | Notes |
|---|---|
| `AlbumArtistID` | → `djmdArtist.ID`. The album-level artist (not the track artist). |
| `ImagePath` | Path to album artwork. NULL if not set separately from track artwork. |
| `Compilation` | `0` = not a compilation, `1` = compilation album. |
| `SearchStr` | Phonetic/normalized search string. NULL in practice. |

---

### `djmdGenre` — Genre Lookup

```sql
CREATE TABLE `djmdGenre` (
  `ID`   VARCHAR(255) PRIMARY KEY,
  `Name` VARCHAR(255) DEFAULT NULL,
  -- + standard sync columns
)
```

Sample values: `Dance`, `Dance/Electronic`, `Pop`, `House`, `Techno`, `Trap`, `Dans/Electro`.

---

### `djmdLabel` — Record Label Lookup

```sql
CREATE TABLE `djmdLabel` (
  `ID`   VARCHAR(255) PRIMARY KEY,
  `Name` VARCHAR(255) DEFAULT NULL,
  -- + standard sync columns
)
```

Sample values: `SPRS`, `Samui Recordings`, `Virgin EMI`, `Enormous Tunes`.

---

### `djmdKey` — Musical Key Lookup

```sql
CREATE TABLE `djmdKey` (
  `ID`        VARCHAR(255) PRIMARY KEY,
  `ScaleName` VARCHAR(255) DEFAULT NULL,
  `Seq`       INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

`Seq` is NULL in all observed rows; ordering may not be guaranteed.

The `ScaleName` values include multiple notation systems all stored in the same table:

**Traditional notation:** `C`, `G`, `D`, `A`, `E`, `B`, `F#`/`Gb`, `Db`, `Ab`, `Eb`, `Bb`, `F`
**Minor traditional:** `Am`, `Em`, `Bm`, `F#m`, `Cm`, `Gm`, `Dm`, `Abm`, `Ebm`, `Bbm`, `Fm`, `Dbm`
**Camelot wheel:** `1A`–`12A`, `1B`–`12B` (though only some are present in this dataset)
**Alternative spellings:** `Amaj`, `Fmaj`, `Dbmaj`, `Gbmaj`, `Abmaj`, `Dmaj`, `Bmaj`, `Gmaj`, `Cmaj`, `Amin`, `Dmin`, `Gmin`, `Bmin`, `Emin`, `Fmin`, `Gbmin`, `Abmin`, `Dbmin`, `Ebmin`, `D#min`, `A#min`

The key system is user-extensible; IDs are hashed from the name.

---

### `djmdColor` — Track Color Labels

```sql
CREATE TABLE `djmdColor` (
  `ID`        VARCHAR(255) PRIMARY KEY,
  `ColorCode` INTEGER      DEFAULT NULL,
  `SortKey`   INTEGER      DEFAULT NULL,
  `Commnt`    VARCHAR(255) DEFAULT NULL,
  -- + standard sync columns
)
```

`ColorCode` is NULL for the built-in colors (they use RGB values stored elsewhere in the application).

Fixed set of 8 colors:

| ID | SortKey | Commnt (name) |
|---|---|---|
| 1 | 1 | Pink |
| 2 | 2 | Red |
| 3 | 3 | Orange |
| 4 | 4 | Yellow |
| 5 | 5 | Green |
| 6 | 6 | Aqua |
| 7 | 7 | Blue |
| 8 | 8 | Purple |

---

## Cue / Loop Points

### `djmdCue` — Cue Points and Loops

One row per cue/loop point per track. A track can have many rows (memory cues, hot cues, loops).

```sql
CREATE TABLE `djmdCue` (
  `ID`              VARCHAR(255) PRIMARY KEY,
  `ContentID`       VARCHAR(255) DEFAULT NULL,  -- FK -> djmdContent.ID
  `InMsec`          INTEGER      DEFAULT NULL,
  `InFrame`         INTEGER      DEFAULT NULL,
  `InMpegFrame`     INTEGER      DEFAULT NULL,
  `InMpegAbs`       INTEGER      DEFAULT NULL,
  `OutMsec`         INTEGER      DEFAULT NULL,
  `OutFrame`        INTEGER      DEFAULT NULL,
  `OutMpegFrame`    INTEGER      DEFAULT NULL,
  `OutMpegAbs`      INTEGER      DEFAULT NULL,
  `Kind`            INTEGER      DEFAULT NULL,
  `Color`           INTEGER      DEFAULT NULL,
  `ColorTableIndex` INTEGER      DEFAULT NULL,
  `ActiveLoop`      INTEGER      DEFAULT NULL,
  `Comment`         VARCHAR(255) DEFAULT NULL,
  `BeatLoopSize`    INTEGER      DEFAULT NULL,
  `CueMicrosec`     INTEGER      DEFAULT NULL,
  `InPointSeekInfo` VARCHAR(255) DEFAULT NULL,
  `OutPointSeekInfo`VARCHAR(255) DEFAULT NULL,
  `ContentUUID`     VARCHAR(255) DEFAULT NULL,  -- FK -> djmdContent.UUID
  -- + standard sync columns
)
```

#### Column Details

| Column | Notes |
|---|---|
| `ContentID` | → `djmdContent.ID`. |
| `InMsec` | Start position of the cue point in **milliseconds** from the start of the track. |
| `InFrame` | Start position in audio frames (sample-accurate). For 44.1 kHz: `InMsec * 0.15` frames approximately. |
| `InMpegFrame` | MPEG frame index for MP3 files. `0` for non-MP3 files. `-1` if not applicable. |
| `InMpegAbs` | Absolute byte offset within MP3 file. `0` if not applicable. |
| `OutMsec` | End position in milliseconds (for loops). `-1` if this is a point cue (no out point). |
| `OutFrame` | End position in frames (for loops). `0` if no out point. |
| `OutMpegFrame` | MPEG frame for out point. |
| `OutMpegAbs` | Absolute byte for out point. |
| `Kind` | Cue type: `0`=Memory Cue, `1`=Hot Cue A, `2`=Hot Cue B, `3`=Hot Cue C (Load point / fade), `4`=Loop, `5`=Hot Cue (numbered > 3). In practice: `0`=memory cue, `1`–`5`=hot cue slots. |
| `Color` | Color integer. `-1` = no color assigned. `255` = white/default. For specific colors the value maps to an RGB or palette index. |
| `ColorTableIndex` | Index into an application-defined color table. `0`=none, observed values: `0`, `21`, `36`. |
| `ActiveLoop` | `1` if this loop is currently active on deck load; `0` otherwise. |
| `Comment` | User-defined label/name for the cue point (e.g. `"Drop"`, `"Intro"`). Empty string if not named. |
| `BeatLoopSize` | Beat loop length in beats (for beat loops). `0` for non-beat-loop cues. |
| `CueMicrosec` | Fine-resolution cue position in microseconds (sub-millisecond precision). Usually `0`. |
| `InPointSeekInfo` | Opaque seek info string for fast seeking; NULL in most cases. |
| `OutPointSeekInfo` | Opaque seek info string for loop out point; NULL in most cases. |
| `ContentUUID` | → `djmdContent.UUID`. Redundant reference to the parent track via UUID (used in cloud sync). |

---

### `contentCue` — Cloud-Synced Cue Blob

A cloud-sync companion to `djmdCue`. Each row contains all cues for one track serialized as a JSON array.

```sql
CREATE TABLE `contentCue` (
  `ID`           VARCHAR(255) PRIMARY KEY,  -- = djmdContent.UUID
  `ContentID`    VARCHAR(255) DEFAULT NULL, -- FK -> djmdContent.ID (numeric)
  `Cues`         TEXT         DEFAULT NULL, -- JSON array of cue objects
  `rb_cue_count` INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

The `Cues` column is a JSON array where each element mirrors all the fields of `djmdCue`. Example:
```json
[
  {
    "ID": "1557985039",
    "ContentID": "27898494",
    "InMsec": 6280,
    "InFrame": 942,
    "OutMsec": 8609,
    "Kind": 0,
    "Color": -1,
    "ColorTableIndex": 0,
    "ActiveLoop": 0,
    "BeatLoopSize": 0,
    "ContentUUID": "08e32a79-4d6a-4053-8418-19e9e708ae47",
    "UUID": "975bcf94-...",
    "created_at": "2020-04-14T19:32:10.519+00:00",
    "updated_at": "2020-04-14T19:32:10.543+00:00"
  }
]
```

---

## Playlists

### `djmdPlaylist` — Playlist Tree Nodes

Represents both folders and playlists in a tree hierarchy. Root-level nodes have `ParentID = "root"`.

```sql
CREATE TABLE `djmdPlaylist` (
  `ID`        VARCHAR(255) PRIMARY KEY,
  `Seq`       INTEGER      DEFAULT NULL,
  `Name`      VARCHAR(255) DEFAULT NULL,
  `ImagePath` VARCHAR(255) DEFAULT NULL,
  `Attribute` INTEGER      DEFAULT NULL,
  `ParentID`  VARCHAR(255) DEFAULT NULL,  -- FK -> djmdPlaylist.ID, or "root"
  `SmartList` TEXT         DEFAULT NULL,
  -- + standard sync columns
)
```

| Column | Notes |
|---|---|
| `Seq` | Sort order within the parent folder. |
| `Name` | Display name of the playlist or folder. |
| `ImagePath` | Path to a custom playlist icon image. NULL for most playlists. |
| `Attribute` | Node type: `0`=Regular playlist, `1`=Folder, `4`=Smart playlist. |
| `ParentID` | ID of the parent folder node, or the string `"root"` for top-level items. |
| `SmartList` | XML string defining the smart playlist filter rules. Only set when `Attribute=4`. See below. |

#### Smart Playlist XML Format

The `SmartList` field contains an XML document:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<NODE Id="-26004859" LogicalOperator="1" AutomaticUpdate="0">
  <CONDITION PropertyName="bpm" Operator="5" ValueUnit="0" ValueLeft="11900" ValueRight="12500"/>
</NODE>
```

- `LogicalOperator`: `1`=AND, `2`=OR
- `AutomaticUpdate`: `0`=manual refresh, `1`=auto-refresh
- `PropertyName`: field to filter on (e.g. `"bpm"`, `"stockDate"`, `"rating"`, `"genre"`, `"key"`)
- `Operator`: comparison: `1`=less than, `2`=less than or equal, `3`=greater than, `4`=greater than or equal, `5`=between, `6`=within last N days
- `ValueLeft`/`ValueRight`: bounds for the condition. BPM values use the same ×100 encoding as the database.

---

### `djmdSongPlaylist` — Track-to-Playlist Mapping

```sql
CREATE TABLE `djmdSongPlaylist` (
  `ID`         VARCHAR(255) PRIMARY KEY,
  `PlaylistID` VARCHAR(255) DEFAULT NULL,  -- FK -> djmdPlaylist.ID
  `ContentID`  VARCHAR(255) DEFAULT NULL,  -- FK -> djmdContent.ID
  `TrackNo`    INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

| Column | Notes |
|---|---|
| `PlaylistID` | → `djmdPlaylist.ID`. |
| `ContentID` | → `djmdContent.ID`. |
| `TrackNo` | 1-based position of the track within the playlist. |

---

## History

### `djmdHistory` — DJ Session History Tree

Hierarchical structure: Year → Month → Session (leaf).

```sql
CREATE TABLE `djmdHistory` (
  `ID`          VARCHAR(255) PRIMARY KEY,
  `Seq`         INTEGER      DEFAULT NULL,
  `Name`        VARCHAR(255) DEFAULT NULL,
  `Attribute`   INTEGER      DEFAULT NULL,
  `ParentID`    VARCHAR(255) DEFAULT NULL,  -- FK -> djmdHistory.ID, or "root"
  `DateCreated` VARCHAR(255) DEFAULT NULL,
  -- + standard sync columns
)
```

| Column | Notes |
|---|---|
| `Attribute` | `1`=folder/group node, `0`=leaf session node. |
| `ParentID` | Parent node ID or `"root"`. Year nodes have `ParentID="root"`, month nodes have `ParentID=<year>`, sessions have `ParentID=<month>`. |
| `DateCreated` | Timestamp when the session was created; format `YYYY-MM-DD HH:MM:SS`. |

Observed hierarchy example:
```
2017 (Attribute=1, ParentID="root")
  └── 2 (Attribute=1, ParentID=2017)  -- month 2
        ├── LINK HISTORY 2017-02-04 (Attribute=0, ParentID=201702)
        ├── HISTORY 2017-02-09 (Attribute=0)
        └── ...
```

`LINK HISTORY` entries come from CDJ/XDJ link exports; `HISTORY` entries from Rekordbox desktop sessions.

---

### `djmdSongHistory` — Track-to-History Mapping

```sql
CREATE TABLE `djmdSongHistory` (
  `ID`        VARCHAR(255) PRIMARY KEY,
  `HistoryID` VARCHAR(255) DEFAULT NULL,  -- FK -> djmdHistory.ID
  `ContentID` VARCHAR(255) DEFAULT NULL,  -- FK -> djmdContent.ID
  `TrackNo`   INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

`TrackNo` is the play order within the session (1-based).

---

## Tags

### `djmdMyTag` — User-Defined Tag Hierarchy

User-created custom tag categories and values. Hierarchical: root-level categories → leaf tag values.

```sql
CREATE TABLE `djmdMyTag` (
  `ID`        VARCHAR(255) PRIMARY KEY,
  `Seq`       INTEGER      DEFAULT NULL,
  `Name`      VARCHAR(255) DEFAULT NULL,
  `Attribute` INTEGER      DEFAULT NULL,
  `ParentID`  VARCHAR(255) DEFAULT NULL,  -- FK -> djmdMyTag.ID, or "root"
  -- + standard sync columns
)
```

| Column | Notes |
|---|---|
| `Attribute` | `1`=category node (top-level), `0`=tag value (leaf). |
| `ParentID` | `"root"` for top-level categories; otherwise the ID of the parent category. |

Sample hierarchy from the real database:
```
1 "Genre" (Attribute=1, root)
  ├── 2471719367 "Acid House"
  ├── 1729031917 "Deep House"
  ├── 3543532698 "Techno"
  └── ...
2 "Components" (Attribute=1, root)
  ├── 1243170409 "Synth"
  ├── 670820764  "Vocal"
  └── ...
3 "Situation" (Attribute=1, root)
  ├── 274722503  "Main Floor"
  ├── 48818875   "Second Floor"
  └── ...
4 "Untitled Column" (Attribute=1, root)
  └── 3206699690 "My Comment"
```

---

### `djmdSongMyTag` — Track-to-MyTag Assignment

```sql
CREATE TABLE `djmdSongMyTag` (
  `ID`       VARCHAR(255) PRIMARY KEY,
  `MyTagID`  VARCHAR(255) DEFAULT NULL,  -- FK -> djmdMyTag.ID (leaf only)
  `ContentID`VARCHAR(255) DEFAULT NULL,  -- FK -> djmdContent.ID
  `TrackNo`  INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

`TrackNo` here is usually NULL; the field appears unused for MyTag assignments.

---

## Mixer / Analysis

### `djmdMixerParam` — Per-Track Gain and Peak Analysis

One row per track. Stores the auto-gain / peak analysis result used by Rekordbox's trim/EQ engine.

```sql
CREATE TABLE `djmdMixerParam` (
  `ID`        VARCHAR(255) PRIMARY KEY,  -- UUID
  `ContentID` VARCHAR(255) DEFAULT NULL, -- FK -> djmdContent.ID
  `GainHigh`  INTEGER      DEFAULT NULL,
  `GainLow`   INTEGER      DEFAULT NULL,
  `PeakHigh`  INTEGER      DEFAULT NULL,
  `PeakLow`   INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

#### Encoding

`GainHigh` and `GainLow` together encode a 32-bit IEEE 754 float:
`Gain = struct.unpack('>f', struct.pack('>HH', GainHigh, GainLow))[0]`

Similarly for `PeakHigh` / `PeakLow`.

Observed ranges:
- `GainHigh`: 16119–16579
- `GainLow`: 125–65531
- `Gain` (decoded): approximately 0.69–1.01 (linear scale, 1.0 = 0 dB)
- `PeakHigh`: always 16039 or 16256 in this dataset
- `PeakLow`: 0–65024
- `Peak` when `PeakHigh=16256, PeakLow=0`: exactly 1.0

---

## Hot Cue Banks

### `djmdHotCueBanklist` — Hot Cue Bank Containers

```sql
CREATE TABLE `djmdHotCueBanklist` (
  `ID`        VARCHAR(255) PRIMARY KEY,
  `Seq`       INTEGER      DEFAULT NULL,
  `Name`      VARCHAR(255) DEFAULT NULL,
  `ImagePath` VARCHAR(255) DEFAULT NULL,
  `Attribute` INTEGER      DEFAULT NULL,
  `ParentID`  VARCHAR(255) DEFAULT NULL,  -- FK -> djmdHotCueBanklist.ID, or "root"
  -- + standard sync columns
)
```

Tree structure like `djmdPlaylist`. Empty in the sample database.

---

### `djmdSongHotCueBanklist` — Hot Cue in a Bank

```sql
CREATE TABLE `djmdSongHotCueBanklist` (
  `ID`                VARCHAR(255) PRIMARY KEY,
  `HotCueBanklistID`  VARCHAR(255) DEFAULT NULL, -- FK -> djmdHotCueBanklist.ID
  `ContentID`         VARCHAR(255) DEFAULT NULL, -- FK -> djmdContent.ID
  `TrackNo`           INTEGER      DEFAULT NULL,
  `CueID`             VARCHAR(255) DEFAULT NULL, -- FK -> djmdCue.ID
  `InMsec`            INTEGER      DEFAULT NULL,
  `InFrame`           INTEGER      DEFAULT NULL,
  `InMpegFrame`       INTEGER      DEFAULT NULL,
  `InMpegAbs`         INTEGER      DEFAULT NULL,
  `OutMsec`           INTEGER      DEFAULT NULL,
  `OutFrame`          INTEGER      DEFAULT NULL,
  `OutMpegFrame`      INTEGER      DEFAULT NULL,
  `OutMpegAbs`        INTEGER      DEFAULT NULL,
  `Color`             INTEGER      DEFAULT NULL,
  `ColorTableIndex`   INTEGER      DEFAULT NULL,
  `ActiveLoop`        INTEGER      DEFAULT NULL,
  `Comment`           VARCHAR(255) DEFAULT NULL,
  `BeatLoopSize`      INTEGER      DEFAULT NULL,
  `CueMicrosec`       INTEGER      DEFAULT NULL,
  `InPointSeekInfo`   VARCHAR(255) DEFAULT NULL,
  `OutPointSeekInfo`  VARCHAR(255) DEFAULT NULL,
  `HotCueBanklistUUID`VARCHAR(255) DEFAULT NULL,
  -- + standard sync columns
)
```

This is essentially a snapshot of a hot cue point (`djmdCue`) within a named bank, copying all timing fields directly.

---

## Related Tracks

### `djmdRelatedTracks` — Related Tracks Filter Presets

```sql
CREATE TABLE `djmdRelatedTracks` (
  `ID`        VARCHAR(255) PRIMARY KEY,
  `Seq`       INTEGER      DEFAULT NULL,
  `Name`      VARCHAR(255) DEFAULT NULL,
  `Attribute` INTEGER      DEFAULT NULL,
  `ParentID`  VARCHAR(255) DEFAULT NULL,  -- FK -> djmdRelatedTracks.ID, or "root"
  `Criteria`  TEXT         DEFAULT NULL,
  -- + standard sync columns
)
```

| Column | Notes |
|---|---|
| `Attribute` | `10`=root node, `11`=user-defined preset node. |
| `Criteria` | JSON object defining the filter parameters. |

#### Criteria JSON Schema

```json
{
  "Ver": 1,
  "Hist": {"Diff": 3},
  "BPM":  {"True": 1, "Type": 1, "Diff": {"Diff": 5, "HaDo": 1}, "Rang": {"Type": 1, "Min": 10000, "Max": 12000}},
  "Key":  {"True": 1, "Typ1": 1, "Typ2": 2},
  "DAdd": {"True": 1, "Days": 30},
  "Genr": {"True": 1},
  "Arti": {"True": 1, "Titl": 1},
  "Comm": {"Type": 1, "Word": []},
  "Year": {"Type": 1, "Diff": {"Diff": 0}, "Rang": {"Min": 2017, "Max": 2018}},
  "Form": {"Form": 2047, "BitR": -1},
  "Rate": {"Rate": 0},
  "MTag": {"Type": 0},
  "Matc": {},
  "Comp": {},
  "Remi": {},
  "Labe": {},
  "Colo": {}
}
```

- `BPM.Rang.Min`/`Max`: BPM range in ×100 encoding (e.g. `10000`=100.00 BPM)
- `True: 1` on a field means that criterion is active/enabled
- `Form.Form`: bitmask for file format filter; `8191` = all formats, `2047` = some subset

---

### `djmdSongRelatedTracks` — Track-to-RelatedTracks Mapping

```sql
CREATE TABLE `djmdSongRelatedTracks` (
  `ID`              VARCHAR(255) PRIMARY KEY,
  `RelatedTracksID` VARCHAR(255) DEFAULT NULL,  -- FK -> djmdRelatedTracks.ID
  `ContentID`       VARCHAR(255) DEFAULT NULL,  -- FK -> djmdContent.ID
  `TrackNo`         INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

---

## Sampler

### `djmdSampler` — Sampler Folder Hierarchy

```sql
CREATE TABLE `djmdSampler` (
  `ID`        VARCHAR(255) PRIMARY KEY,
  `Seq`       INTEGER      DEFAULT NULL,
  `Name`      VARCHAR(255) DEFAULT NULL,
  `Attribute` INTEGER      DEFAULT NULL,
  `ParentID`  VARCHAR(255) DEFAULT NULL,  -- FK -> djmdSampler.ID, or "root"
  -- + standard sync columns
)
```

`Attribute` observed values:
- `3`=Sampler root node
- `5`=All Samples (virtual folder)
- `6`=Capture folder
- `2`=OSC Sampler bank
- `2`=Preset bank

Built-in hierarchy:
```
1 "Sampler Root" (Attribute=3, root)
  ├── 2 "All Samples"   (Attribute=5)
  ├── 3 "Capture"       (Attribute=6)
  └── 1370356172 "OSC SAMPLER" (Attribute=3)
        └── 3965907873 "PRESET ONESHOT" (Attribute=2)
```

---

### `djmdSongSampler` — Track-to-Sampler Mapping

```sql
CREATE TABLE `djmdSongSampler` (
  `ID`        VARCHAR(255) PRIMARY KEY,
  `SamplerID` VARCHAR(255) DEFAULT NULL,  -- FK -> djmdSampler.ID
  `ContentID` VARCHAR(255) DEFAULT NULL,  -- FK -> djmdContent.ID
  `TrackNo`   INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

---

## Browser Configuration

### `djmdMenuItems` — Browser Menu Item Definitions

Defines the categories available in the Rekordbox browser menu.

```sql
CREATE TABLE `djmdMenuItems` (
  `ID`    VARCHAR(255) PRIMARY KEY,
  `Class` INTEGER      DEFAULT NULL,
  `Name`  VARCHAR(255) DEFAULT NULL,
  -- + standard sync columns
)
```

Complete set:

| ID | Class | Name |
|---|---|---|
| 1 | -128 | GENRE |
| 2 | -127 | ARTIST |
| 3 | -126 | ALBUM |
| 4 | -125 | TRACK |
| 5 | -123 | BPM |
| 6 | -122 | RATING |
| 7 | -121 | YEAR |
| 8 | -120 | REMIXER |
| 9 | -119 | LABEL |
| 10 | -118 | ORIGINAL ARTIST |
| 11 | -117 | KEY |
| 12 | -115 | CUE |
| 13 | -114 | COLOR |
| 14 | -110 | TIME |
| 15 | -109 | BITRATE |
| 16 | -108 | FILE NAME |
| 17 | -124 | PLAYLIST |
| 18 | -104 | HOT CUE BANK |
| 19 | -107 | HISTORY |
| 20 | -111 | SEARCH |
| 21 | -106 | COMMENTS |
| 22 | -116 | DATE ADDED |
| 23 | -105 | DJ PLAY COUNT |
| 24 | -112 | FOLDER |
| 25 | -95 | DEFAULT |
| 26 | -94 | ALPHABET |
| 27 | -86 | MATCHING |

---

### `djmdCategory` — Browser Category Visibility/Order

Controls which `djmdMenuItems` are shown and in what order in the browser sidebar.

```sql
CREATE TABLE `djmdCategory` (
  `ID`         VARCHAR(255) PRIMARY KEY,
  `MenuItemID` VARCHAR(255) DEFAULT NULL,  -- FK -> djmdMenuItems.ID
  `Seq`        INTEGER      DEFAULT NULL,
  `Disable`    INTEGER      DEFAULT NULL,
  `InfoOrder`  INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

| Column | Notes |
|---|---|
| `MenuItemID` | → `djmdMenuItems.ID`. |
| `Seq` | Display position in the browser (0-based). |
| `Disable` | `0`=visible, `1`=hidden, `2`=special state. |
| `InfoOrder` | Secondary ordering hint; `99`=not shown in info panel. |

---

### `djmdSort` — Browser Sort Order Configuration

```sql
CREATE TABLE `djmdSort` (
  `ID`         VARCHAR(255) PRIMARY KEY,
  `MenuItemID` VARCHAR(255) DEFAULT NULL,  -- FK -> djmdMenuItems.ID
  `Seq`        INTEGER      DEFAULT NULL,
  `Disable`    INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

Controls how track lists are sorted under each category. `Seq` is the sort priority; `Disable=1` means this sort option is hidden.

---

## Device and Properties

### `djmdDevice` — Library Device Identity

One row per Rekordbox installation/device that contributed tracks to this database.

```sql
CREATE TABLE `djmdDevice` (
  `ID`         VARCHAR(255) PRIMARY KEY,  -- UUID
  `MasterDBID` VARCHAR(255) DEFAULT NULL,
  `Name`       VARCHAR(255) DEFAULT NULL,
  -- + standard sync columns
)
```

| Column | Notes |
|---|---|
| `ID` | UUID identifying this device. Referenced by `djmdContent.DeviceID`. |
| `MasterDBID` | Numeric ID matching `djmdProperty.DBID`. |
| `Name` | Human-readable device name (e.g. `"Jonass-MBP-2"`). |

---

### `djmdProperty` — Database-Level Properties

Exactly one row, describing the database itself.

```sql
CREATE TABLE `djmdProperty` (
  `DBID`           VARCHAR(255) PRIMARY KEY,
  `DBVersion`      VARCHAR(255) DEFAULT NULL,
  `BaseDBDrive`    VARCHAR(255) DEFAULT NULL,
  `CurrentDBDrive` VARCHAR(255) DEFAULT NULL,
  `DeviceID`       VARCHAR(255) DEFAULT NULL,  -- FK -> djmdDevice.ID
  `Reserved1`      TEXT         DEFAULT NULL,
  `Reserved2`      TEXT         DEFAULT NULL,
  `Reserved3`      TEXT         DEFAULT NULL,
  `Reserved4`      TEXT         DEFAULT NULL,
  `Reserved5`      TEXT         DEFAULT NULL,
  `created_at`     DATETIME     NOT NULL,
  `updated_at`     DATETIME     NOT NULL
)
```

| Column | Notes |
|---|---|
| `DBID` | Numeric database ID (e.g. `1827296556`). This is the `MasterDBID` referenced throughout the schema. |
| `DBVersion` | Schema version string (e.g. `"6000"` for Rekordbox 6). |
| `BaseDBDrive` | Root path of the storage volume at creation time (e.g. `"/Volumes/muzika/"`). |
| `CurrentDBDrive` | Current root path (may differ from `BaseDBDrive` if the volume was renamed/moved). |
| `DeviceID` | → `djmdDevice.ID`. The device that owns this database. |

---

### `djmdCloudProperty` — Cloud Sync Properties

One row. Stores cloud service configuration.

```sql
CREATE TABLE `djmdCloudProperty` (
  `ID`        VARCHAR(255) PRIMARY KEY,
  `Reserved1` TEXT         DEFAULT NULL,  -- Service type (e.g. "SyncService")
  `Reserved2` TEXT         DEFAULT NULL,  -- Provider (e.g. "Google")
  `Reserved3` TEXT         DEFAULT NULL,
  `Reserved4` TEXT         DEFAULT NULL,
  `Reserved5` TEXT         DEFAULT NULL,
  -- + standard sync columns
)
```

Despite being named `Reserved`, `Reserved1` contains `"SyncService"` and `Reserved2` contains `"Google"` in the observed database.

---

## Cloud Sync Support Tables

### `contentFile` — Cloud File Tracking

Tracks file sync state for cloud-stored audio files. One row per file (audio + artwork).

```sql
CREATE TABLE `contentFile` (
  `ID`                 VARCHAR(255) PRIMARY KEY,
  `ContentID`          VARCHAR(255) DEFAULT NULL,  -- FK -> djmdContent.ID
  `Path`               VARCHAR(255) DEFAULT NULL,
  `Hash`               VARCHAR(255) DEFAULT NULL,
  `Size`               INTEGER      DEFAULT NULL,
  `rb_local_path`      VARCHAR(255) DEFAULT NULL,
  `rb_insync_hash`     VARCHAR(255) DEFAULT NULL,
  `rb_insync_local_usn`BIGINT       DEFAULT NULL,
  `rb_file_hash_dirty` INTEGER      DEFAULT 0,
  `rb_local_file_status` INTEGER    DEFAULT 0,
  `rb_in_progress`     TINYINT(1)   DEFAULT 0,
  `rb_process_type`    INTEGER      DEFAULT 0,
  `rb_temp_path`       VARCHAR(255) DEFAULT NULL,
  `rb_priority`        INTEGER      DEFAULT 50,
  `rb_file_size_dirty` INTEGER      DEFAULT 0,
  -- + standard sync columns
)
```

The `ID` format is `<UUID>_<URL-encoded path>`, e.g. `08e32a79-..._%2FPIONEER%2FArtwork%2F...`.

`Path` is URL-decoded and relative to the USB/Pioneer root, e.g. `/PIONEER/Artwork/00b/...`.
`rb_local_path` is the absolute local path on the host machine.
`Hash` is an MD5 hash of the file content.

---

### `contentActiveCensor` — Active Censor Regions

```sql
CREATE TABLE `contentActiveCensor` (
  `ID`                   VARCHAR(255) PRIMARY KEY,
  `ContentID`            VARCHAR(255) DEFAULT NULL, -- FK -> djmdContent.ID
  `ActiveCensors`        TEXT         DEFAULT NULL,
  `rb_activecensor_count`INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

`ActiveCensors` is a JSON blob describing sections of the track that should be censored during playback. Empty in the sample database.

---

### `hotCueBanklistCue` — Cloud Hot Cue Bank Blobs

```sql
CREATE TABLE `hotCueBanklistCue` (
  `ID`               VARCHAR(255) PRIMARY KEY,
  `HotCueBanklistID` VARCHAR(255) DEFAULT NULL, -- FK -> djmdHotCueBanklist.ID
  `Cues`             TEXT         DEFAULT NULL,
  `rb_cue_count`     INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

JSON array of hot cue points for a bank, analogous to `contentCue`.

---

### `imageFile` — Cloud Image Tracking

```sql
CREATE TABLE `imageFile` (
  `ID`                 VARCHAR(255) PRIMARY KEY,
  `TableName`          VARCHAR(255) DEFAULT NULL,
  `TargetUUID`         VARCHAR(255) DEFAULT NULL,
  `TargetID`           VARCHAR(255) DEFAULT NULL,
  `Path`               VARCHAR(255) DEFAULT NULL,
  `Hash`               VARCHAR(255) DEFAULT NULL,
  `Size`               INTEGER      DEFAULT NULL,
  `rb_local_path`      VARCHAR(255) DEFAULT NULL,
  `rb_insync_hash`     VARCHAR(255) DEFAULT NULL,
  `rb_insync_local_usn`BIGINT       DEFAULT NULL,
  `rb_file_hash_dirty` INTEGER      DEFAULT 0,
  `rb_local_file_status` INTEGER    DEFAULT 0,
  `rb_in_progress`     TINYINT(1)   DEFAULT 0,
  `rb_process_type`    INTEGER      DEFAULT 0,
  `rb_temp_path`       VARCHAR(255) DEFAULT NULL,
  `rb_priority`        INTEGER      DEFAULT 50,
  `rb_file_size_dirty` INTEGER      DEFAULT 0,
  -- + standard sync columns
)
```

Tracks artwork images for cloud sync. `TableName` identifies which table the image belongs to (e.g. `"djmdContent"`, `"djmdAlbum"`).

---

### `settingFile` — Cloud Settings File Tracking

```sql
CREATE TABLE `settingFile` (
  `ID`                 VARCHAR(255) PRIMARY KEY,  -- URL-encoded path
  `Path`               VARCHAR(255) DEFAULT NULL,
  `Hash`               VARCHAR(255) DEFAULT NULL,
  `Size`               INTEGER      DEFAULT NULL,
  `rb_local_path`      VARCHAR(255) DEFAULT NULL,
  `rb_insync_hash`     VARCHAR(255) DEFAULT NULL,
  `rb_insync_local_usn`BIGINT       DEFAULT NULL,
  `rb_file_hash_dirty` INTEGER      DEFAULT 0,
  `rb_file_size_dirty` INTEGER      DEFAULT 0,
  -- + standard sync columns
)
```

Observed paths: `/MYSETTING.DAT`, `/MYSETTING2.DAT`, `/DJMMYSETTING.DAT`, `/cue_personal_trend.json`.

---

## Agent / Cloud Registry Tables

### `agentRegistry` — Local Key-Value Store

Used internally by the Rekordbox agent process to store configuration and credentials.

```sql
CREATE TABLE `agentRegistry` (
  `registry_id` VARCHAR(255) PRIMARY KEY,
  `id_1`        VARCHAR(255) DEFAULT NULL,
  `id_2`        VARCHAR(255) DEFAULT NULL,
  `int_1`       BIGINT       DEFAULT NULL,
  `int_2`       BIGINT       DEFAULT NULL,
  `str_1`       VARCHAR(255) DEFAULT NULL,
  `str_2`       VARCHAR(255) DEFAULT NULL,
  `date_1`      DATETIME     DEFAULT NULL,
  `date_2`      DATETIME     DEFAULT NULL,
  `text_1`      TEXT         DEFAULT NULL,
  `text_2`      TEXT         DEFAULT NULL,
  `created_at`  DATETIME     NOT NULL,
  `updated_at`  DATETIME     NOT NULL
)
```

The schema is generic. Observed `registry_id` values include: `localUpdateCount`, `SyncAnalysisDataRootPath`, `SyncSettingsRootPath`, `LangPath`, `agentCredentials`, etc.

---

### `cloudAgentRegistry` — Cloud Key-Value Store

Same structure as `agentRegistry` but for cloud-synced agent data. Includes the standard sync columns.

---

### `agentNotification` — Push Notifications

Stores push notifications received from Pioneer's cloud service.

```sql
CREATE TABLE `agentNotification` (
  `ID`                     BIGINT       PRIMARY KEY,
  `graphic_area`           TINYINT(1)   DEFAULT 0,
  `text_area`              TINYINT(1)   DEFAULT 0,
  `os_notification`        TINYINT(1)   DEFAULT 0,
  `start_datetime`         DATETIME     DEFAULT NULL,
  `end_datetime`           DATETIME     DEFAULT NULL,
  `display_datetime`       DATETIME     DEFAULT NULL,
  `interval`               INTEGER      DEFAULT 0,
  `category`               VARCHAR(255) DEFAULT NULL,
  `category_color`         VARCHAR(255) DEFAULT NULL,
  `title`                  TEXT         DEFAULT NULL,
  `description`            TEXT         DEFAULT NULL,
  `url`                    VARCHAR(255) DEFAULT NULL,
  `image`                  VARCHAR(255) DEFAULT NULL,
  `image_path`             VARCHAR(255) DEFAULT NULL,
  `read_status`            INTEGER      DEFAULT 0,
  `last_displayed_datetime`DATETIME     DEFAULT NULL,
  `created_at`             DATETIME     NOT NULL,
  `updated_at`             DATETIME     NOT NULL
)
```

`category` values observed: `"VIDEO"`, `"OTHER"`, `"TUTORIAL"`.
`category_color` is a hex color string (e.g. `"#06a088"`).

---

### `agentNotificationLog`

Auto-increment log of notification delivery events. Empty in the sample database.

---

### `uuidIDMap` — UUID-to-ID Mapping

Used to reconcile cloud UUIDs with local integer IDs during sync.

```sql
CREATE TABLE `uuidIDMap` (
  `ID`          VARCHAR(255) PRIMARY KEY,
  `TableName`   VARCHAR(255) DEFAULT NULL,
  `TargetUUID`  VARCHAR(255) DEFAULT NULL,
  `CurrentID`   VARCHAR(255) DEFAULT NULL,
  -- + standard sync columns
)
```

---

### `djmdSongTagList`

```sql
CREATE TABLE `djmdSongTagList` (
  `ID`        VARCHAR(255) PRIMARY KEY,
  `ContentID` VARCHAR(255) DEFAULT NULL,  -- FK -> djmdContent.ID
  `TrackNo`   INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

Always empty in the observed database. Purpose is unclear; may be a legacy or future-use table.

---

## Shared Playlists

### `djmdSharedPlaylist`

```sql
CREATE TABLE `djmdSharedPlaylist` (
  `ID`            VARCHAR(255) PRIMARY KEY,
  `data_selection`TINYINT      DEFAULT 0,
  `edited_at`     DATETIME     DEFAULT NULL,
  `int_1`         INTEGER      DEFAULT NULL,
  `int_2`         INTEGER      DEFAULT NULL,
  `str_1`         VARCHAR(255) DEFAULT NULL,
  `str_2`         VARCHAR(255) DEFAULT NULL,
  `text_1`        TEXT         DEFAULT NULL,
  `text_2`        TEXT         DEFAULT NULL,
  `created_at`    DATETIME     NOT NULL,
  `updated_at`    DATETIME     NOT NULL
)
```

### `djmdSharedPlaylistUser`

```sql
CREATE TABLE `djmdSharedPlaylistUser` (
  `ID`                  VARCHAR(255) NOT NULL,    -- FK -> djmdSharedPlaylist.ID
  `member_type`         TINYINT      DEFAULT 0,
  `member_id`           VARCHAR(255) NOT NULL,
  `status`              TINYINT      DEFAULT 0,
  `invitation_expires_at` DATETIME   DEFAULT NULL,
  `invited_at`          DATETIME     DEFAULT NULL,
  `joined_at`           DATETIME     DEFAULT NULL,
  `int_1`               INTEGER      DEFAULT NULL,
  `int_2`               INTEGER      DEFAULT NULL,
  `str_1`               VARCHAR(255) DEFAULT NULL,
  `str_2`               VARCHAR(255) DEFAULT NULL,
  `text_1`              TEXT         DEFAULT NULL,
  `text_2`              TEXT         DEFAULT NULL,
  `created_at`          DATETIME     NOT NULL,
  `updated_at`          DATETIME     NOT NULL,
  PRIMARY KEY (`ID`, `member_id`)
)
```

---

## Recommendation

### `djmdRecommendLike`

```sql
CREATE TABLE `djmdRecommendLike` (
  `ID`           VARCHAR(255) PRIMARY KEY,
  `ContentID1`   VARCHAR(255) DEFAULT NULL,  -- FK -> djmdContent.ID
  `ContentID2`   VARCHAR(255) DEFAULT NULL,  -- FK -> djmdContent.ID
  `LikeRate`     INTEGER      DEFAULT NULL,
  `DataCreatedH` INTEGER      DEFAULT NULL,
  `DataCreatedL` INTEGER      DEFAULT NULL,
  -- + standard sync columns
)
```

Stores user feedback pairs for the track recommendation engine. `DataCreatedH`/`DataCreatedL` likely encode a 64-bit timestamp split into two 32-bit integers.

---

## Auxiliary XML Files

These XML files live alongside `master.db` and provide additional playlist sync data.

### `masterPlaylists6.xml`

Contains a flat list of all playlist nodes (including their IDs, parent IDs, and timestamps) for multi-device sync via LINK. Each node has:

- `Id`: hex playlist ID (matches `djmdPlaylist.ID` but in hex format)
- `ParentId`: hex parent ID (`"0"` for root-level)
- `Attribute`: same values as `djmdPlaylist.Attribute` (`0`=playlist, `1`=folder, `4`=smart)
- `Timestamp`: milliseconds since epoch of last modification
- `Lib_Type`: library type flag (always `"0"` in observed data)
- `CheckType`: check type flag

### `smartlist.xml`

Contains the smart playlist filter definitions for version 1 of the smart list format (older rekordbox):

```xml
<SMARTLIST Version="1.0.0">
  <PLAYLISTS>
    <NODE Id="28" LogicalOperator="1" AutomaticUpdate="0">
      <CONDITION PropertyName="stockDate" Operator="6" ValueUnit="day" ValueLeft="60" ValueRight=""/>
    </NODE>
  </PLAYLISTS>
</SMARTLIST>
```

Node `Id` values here are decimal integers that correspond to `djmdPlaylist.ID`.

### `automixPlaylist6.xml`

Tracks the automix (auto-DJ) queue order:

```xml
<AUTOMIX version="1" browse_type="1">
  <PLAYLIST contents_dbid="1827296556" contents_id="188906518"
            master_dbid="1827296556" master_contents_id="1462"/>
</AUTOMIX>
```

- `contents_dbid` = `djmdProperty.DBID` of the source device
- `contents_id` = `djmdContent.ID` of the track
- `master_dbid` = DBID of the master/authoritative device
- `master_contents_id` = content ID on the master device (may differ if track was copied)

---

## Key Encoding Quirks Summary

| Field | Encoding | Example |
|---|---|---|
| `djmdContent.BPM` | Integer × 100 | `10300` = 103.00 BPM |
| `djmdContent.Length` | Integer seconds | `235` = 3:55 |
| `djmdContent.FileType` | Integer enum | `1`=MP3, `4`=M4A, `5`=FLAC, `11`=WAV, `12`=AIFF |
| `djmdContent.Rating` | 0–5 integer | `0`=none, `5`=5 stars |
| `djmdContent.Analysed` | Integer | `105`=fully analyzed |
| `djmdCue.Kind` | Integer enum | `0`=memory cue, `1`–`5`=hot cue slots |
| `djmdCue.OutMsec` | Integer ms or -1 | `-1`=no out point (point cue, not loop) |
| `djmdCue.Color` | Integer | `-1`=no color, `255`=default |
| `djmdMixerParam.GainHigh/GainLow` | 32-bit IEEE 754 float split into two uint16 | `struct.unpack('>f', struct.pack('>HH', GainHigh, GainLow))` |
| `djmdPlaylist.Attribute` | Integer enum | `0`=playlist, `1`=folder, `4`=smart playlist |
| `djmdHistory.Attribute` | Integer enum | `0`=session leaf, `1`=folder node |
| `djmdMyTag.Attribute` | Integer enum | `0`=tag value, `1`=category |
| All `ID` / `*ID` columns | Decimal integer stored as VARCHAR | `"27898494"` |
| `SmartList.bpm` condition values | × 100 encoding | `"11900"` = 119.00 BPM |
| `created_at` / `updated_at` | `YYYY-MM-DD HH:MM:SS.mmm +00:00` | UTC timezone |
| `ParentID` root sentinel | String literal | `"root"` |

---

## Entity Relationship Diagram (Conceptual)

```
djmdContent ──── ArtistID ────────────────────► djmdArtist
            ──── AlbumID  ────────────────────► djmdAlbum ──── AlbumArtistID ──► djmdArtist
            ──── GenreID  ────────────────────► djmdGenre
            ──── LabelID  ────────────────────► djmdLabel
            ──── KeyID    ────────────────────► djmdKey
            ──── ColorID  ────────────────────► djmdColor
            ──── RemixerID ───────────────────► djmdArtist
            ──── OrgArtistID ─────────────────► djmdArtist
            ──── ComposerID ──────────────────► djmdArtist
            ──── Lyricist  ───────────────────► djmdArtist
            ──── DeviceID  ───────────────────► djmdDevice

djmdCue         ── ContentID ────────────────► djmdContent
djmdMixerParam  ── ContentID ────────────────► djmdContent
djmdActiveCensor── ContentID ────────────────► djmdContent
contentCue      ── ContentID ────────────────► djmdContent
contentFile     ── ContentID ────────────────► djmdContent

djmdSongPlaylist── ContentID ────────────────► djmdContent
                ── PlaylistID ───────────────► djmdPlaylist (tree: parent→child)

djmdSongHistory ── ContentID ────────────────► djmdContent
                ── HistoryID ────────────────► djmdHistory (tree: year→month→session)

djmdSongMyTag   ── ContentID ────────────────► djmdContent
                ── MyTagID ──────────────────► djmdMyTag (tree: category→tag value)

djmdSongHotCueBanklist ── ContentID ─────────► djmdContent
                       ── HotCueBanklistID ──► djmdHotCueBanklist (tree)

djmdSongRelatedTracks  ── ContentID ─────────► djmdContent
                       ── RelatedTracksID ───► djmdRelatedTracks

djmdSongSampler ── ContentID ────────────────► djmdContent
                ── SamplerID ────────────────► djmdSampler (tree)

djmdCategory    ── MenuItemID ───────────────► djmdMenuItems
djmdSort        ── MenuItemID ───────────────► djmdMenuItems
```

**Notes on foreign keys:** Rekordbox does not declare `FOREIGN KEY` constraints in SQLite. All relationships are enforced at the application layer only. The `ID` columns are `VARCHAR(255)` even for integer IDs. Joining requires `CAST` or string comparison.
