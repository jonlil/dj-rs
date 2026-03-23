use rusqlite::{Result, params};
use std::convert::TryInto;
use crate::db::DbHandle;

// All ID columns in this database are declared VARCHAR(255) and stored as TEXT.
// rusqlite's get::<_, i64>() returns InvalidType for TEXT cells, so we always
// read them as String and parse.
fn parse_id(s: String) -> i64 {
    s.parse().unwrap_or(0)
}

/// Rekordbox library access. Owns a `DbHandle` for thread-safe, serialized
/// database access. Clone is cheap (shared connection via Arc).
#[derive(Clone)]
pub struct Library {
    db: DbHandle,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub key: Option<String>,
    /// Stored ×100 in the DB (e.g. 12800 = 128.00 BPM)
    pub bpm: Option<i32>,
    pub duration_secs: Option<i32>,
    pub rating: Option<i32>,
    pub play_count: Option<i32>,
    pub file_path: Option<String>,
    pub track_no: Option<i32>,
    pub label: Option<String>,
    pub color_id: Option<String>,
    pub image_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    /// 0 = playlist, 1 = folder
    pub attribute: i32,
    pub track_count: u32,
}


#[derive(Debug, Clone)]
pub struct HistorySession {
    pub id: i64,
    pub name: String,
    pub track_count: u32,
}

/// A cue point, loop, or hot cue entry from djmdCue.
///
/// Kind mapping:
///   0 = Memory cue (main CUE button)
///   1 = Hot cue H1
///   2 = Hot cue H2
///   3 = Hot cue H3
///   4 = Hot cue H4
///   5 = Hot cue H5 (and above)
#[derive(Debug, Clone)]
pub struct CuePoint {
    /// 0 = memory cue, 1–8 = hot cue slots H1–H8
    pub kind: i32,
    /// Start position in seconds
    pub in_secs: f64,
    /// Loop end position in seconds; None if this is a point cue (OutMsec == -1)
    pub out_secs: Option<f64>,
    /// Color index: -1 = none, 255 = default
    pub color: i32,
    /// Optional user label
    pub comment: String,
}

pub struct TrackFilter {
    pub bpm_min: Option<f32>,
    pub bpm_max: Option<f32>,
    pub key: Option<String>,
    pub genre: Option<String>,
    pub min_rating: Option<i32>,
}

/// Map a row (columns 0..14) to a Track.  Used by every SELECT that returns
/// the full 15-column projection.
fn map_track_row(row: &rusqlite::Row) -> rusqlite::Result<Track> {
    Ok(Track {
        id:            parse_id(row.get(0)?),
        title:         row.get(1)?,
        artist:        row.get(2)?,
        album:         row.get(3)?,
        genre:         row.get(4)?,
        key:           row.get(5)?,
        bpm:           row.get(6)?,
        duration_secs: row.get(7)?,
        rating:        row.get(8)?,
        play_count:    row.get(9)?,
        file_path:     row.get(10)?,
        track_no:      row.get(11)?,
        label:         row.get(12)?,
        color_id:      row.get(13)?,
        image_path:    row.get(14)?,
    })
}

/// Extract the data payload of the first ANLZ section matching `tag` (e.g. b"PWAV", b"PWV3").
///
/// ANLZ section layout (all big-endian u32):
///   [0..4]   tag          4-byte ASCII
///   [4..8]   header_len   bytes from tag start to data start
///   [8..12]  section_len  total section size including header
///   [12..header_len] extra header fields (ignored)
///   [header_len..section_len] data
fn anlz_extract_section(file: &[u8], tag: &[u8; 4]) -> Option<Vec<u8>> {
    let file_len = file.len();
    let mut offset = 0usize;

    // Skip the file-level PMAI header (header_len bytes)
    if file_len < 8 || &file[0..4] != b"PMAI" {
        return None;
    }
    let pmai_hdr = u32::from_be_bytes(file[4..8].try_into().ok()?) as usize;
    offset = pmai_hdr;

    while offset + 12 <= file_len {
        let sec_tag = &file[offset..offset + 4];
        let hdr_len = u32::from_be_bytes(file[offset+4..offset+8].try_into().ok()?) as usize;
        let sec_len = u32::from_be_bytes(file[offset+8..offset+12].try_into().ok()?) as usize;

        if hdr_len < 12 || sec_len < hdr_len || offset + sec_len > file_len {
            break;
        }

        if sec_tag == tag {
            let data_start = offset + hdr_len;
            let data_end   = offset + sec_len;
            return Some(file[data_start..data_end].to_vec());
        }

        offset += sec_len;
    }
    None
}

impl Library {
    pub fn open(path: &str) -> Result<Self> {
        let db = DbHandle::open(path)?;
        Ok(Library { db })
    }

    pub fn track_by_id(&self, content_id: i64) -> Result<Option<Track>> {
        self.db.with_conn(move |conn| {
            let id_str = content_id.to_string();
            let mut stmt = conn.prepare(
                "SELECT c.ID, c.Title,
                        a.Name, al.Name, g.Name, k.ScaleName,
                        c.BPM, c.Length, c.Rating, c.DJPlayCount,
                        c.FolderPath, c.TrackNo,
                        l.Name, c.ColorID, c.ImagePath
                 FROM djmdContent c
                 LEFT JOIN djmdArtist  a  ON c.ArtistID  = a.ID
                 LEFT JOIN djmdAlbum   al ON c.AlbumID   = al.ID
                 LEFT JOIN djmdGenre   g  ON c.GenreID   = g.ID
                 LEFT JOIN djmdKey     k  ON c.KeyID     = k.ID
                 LEFT JOIN djmdLabel   l  ON c.LabelID   = l.ID
                 WHERE c.ID = ?1",
            )?;
            let track = stmt.query_row([&id_str], map_track_row).ok();
            Ok(track)
        })
    }

    pub fn tracks(&self) -> Result<Vec<Track>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT c.ID, c.Title,
                        a.Name, al.Name, g.Name, k.ScaleName,
                        c.BPM, c.Length, c.Rating, c.DJPlayCount,
                        c.FolderPath, c.TrackNo,
                        l.Name, c.ColorID, c.ImagePath
                 FROM djmdContent c
                 LEFT JOIN djmdArtist  a  ON c.ArtistID  = a.ID
                 LEFT JOIN djmdAlbum   al ON c.AlbumID   = al.ID
                 LEFT JOIN djmdGenre   g  ON c.GenreID   = g.ID
                 LEFT JOIN djmdKey     k  ON c.KeyID     = k.ID
                 LEFT JOIN djmdLabel   l  ON c.LabelID   = l.ID
                 WHERE c.rb_local_deleted = 0
                 ORDER BY a.Name, al.Name, c.TrackNo",
            )?;
            let tracks = stmt.query_map([], map_track_row)?
                .collect::<Result<Vec<_>>>()?;
            Ok(tracks)
        })
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT p.ID, p.Name, p.ParentID, p.Attribute,
                        COUNT(sp.ID) as track_count
                 FROM djmdPlaylist p
                 LEFT JOIN djmdSongPlaylist sp ON p.ID = sp.PlaylistID
                 WHERE p.rb_local_deleted = 0
                 GROUP BY p.ID
                 ORDER BY p.Seq",
            )?;
            let playlists = stmt.query_map([], |row| {
                let parent_str: Option<String> = row.get(2)?;
                let parent_id = parent_str
                    .as_deref()
                    .filter(|s| *s != "root")
                    .and_then(|s| s.parse::<i64>().ok());
                Ok(Playlist {
                    id:          parse_id(row.get(0)?),
                    name:        row.get(1)?,
                    parent_id,
                    attribute:   row.get(3)?,
                    track_count: row.get::<_, u32>(4)?,
                })
            })?
            .collect::<Result<Vec<_>>>()?;
            Ok(playlists)
        })
    }

    pub fn update_track_path(&self, id: i64, new_path: &str) -> Result<()> {
        let new_path = new_path.to_string();
        self.db.with_conn(move |conn| {
            conn.execute(
                "UPDATE djmdContent SET FolderPath = ? WHERE ID = ?",
                rusqlite::params![new_path, id.to_string()],
            )?;
            Ok(())
        })
    }

    pub fn track_id_by_path(&self, path: &str) -> Option<i64> {
        let path = path.to_string();
        self.db.with_conn(move |conn| {
            conn.query_row(
                "SELECT ID FROM djmdContent WHERE FolderPath = ?1 AND rb_local_deleted = 0 LIMIT 1",
                params![path],
                |row| row.get::<_, String>(0),
            )
        }).ok().and_then(|s| s.parse().ok())
    }

    pub fn track_file_path(&self, id: i64) -> Option<String> {
        self.db.with_conn(|conn| {
            conn.query_row(
                "SELECT FolderPath FROM djmdContent WHERE ID = ? AND rb_local_deleted = 0",
                rusqlite::params![id.to_string()],
                |row| row.get(0),
            )
        }).ok().flatten()
    }

    pub fn playlist_tracks(&self, playlist_id: i64) -> Result<Vec<Track>> {
        self.db.with_conn(move |conn| {
            let id_str = playlist_id.to_string();
            let mut stmt = conn.prepare(
                "SELECT c.ID, c.Title,
                        a.Name, al.Name, g.Name, k.ScaleName,
                        c.BPM, c.Length, c.Rating, c.DJPlayCount,
                        c.FolderPath, sp.TrackNo,
                        l.Name, c.ColorID, c.ImagePath
                 FROM djmdSongPlaylist sp
                 JOIN  djmdContent  c  ON sp.ContentID = c.ID
                 LEFT JOIN djmdArtist  a  ON c.ArtistID  = a.ID
                 LEFT JOIN djmdAlbum   al ON c.AlbumID   = al.ID
                 LEFT JOIN djmdGenre   g  ON c.GenreID   = g.ID
                 LEFT JOIN djmdKey     k  ON c.KeyID     = k.ID
                 LEFT JOIN djmdLabel   l  ON c.LabelID   = l.ID
                 WHERE sp.PlaylistID = ?1 AND c.rb_local_deleted = 0
                 ORDER BY sp.TrackNo",
            )?;
            let tracks = stmt.query_map(params![id_str], map_track_row)?
                .collect::<Result<Vec<_>>>()?;
            Ok(tracks)
        })
    }

    pub fn move_playlist(&self, id: i64, new_parent_id: Option<i64>) -> Result<()> {
        self.db.with_conn(move |conn| {
            let id_str     = id.to_string();
            let parent_str = new_parent_id
                .map_or_else(|| "root".to_string(), |p| p.to_string());
            conn.execute(
                "UPDATE djmdPlaylist SET ParentID = ?2, updated_at = datetime('now') WHERE ID = ?1",
                params![id_str, parent_str],
            )?;
            Ok(())
        })
    }

    pub fn reorder_playlists(&self, ordered_ids: &[i64]) -> Result<()> {
        let ordered_ids = ordered_ids.to_vec();
        self.db.with_conn(move |conn| {
            conn.execute_batch("BEGIN;")?;
            for (i, &id) in ordered_ids.iter().enumerate() {
                let seq    = (i + 1) as i64;
                let id_str = id.to_string();
                conn.execute(
                    "UPDATE djmdPlaylist SET Seq = ?2, updated_at = datetime('now') WHERE ID = ?1",
                    params![id_str, seq],
                )?;
            }
            conn.execute_batch("COMMIT;")?;
            Ok(())
        })
    }

    pub fn delete_playlist(&self, playlist_id: i64) -> Result<()> {
        self.db.with_conn(move |conn| {
            let id_str = playlist_id.to_string();
            conn.execute(
                "DELETE FROM djmdSongPlaylist WHERE PlaylistID = ?1",
                params![id_str],
            )?;
            conn.execute(
                "DELETE FROM djmdPlaylist WHERE ID = ?1",
                params![id_str],
            )?;
            Ok(())
        })
    }

    pub fn delete_subtree(&self, root_id: i64) -> Result<()> {
        self.db.with_conn(move |conn| {
            let root_str = root_id.to_string();
            let ids: Vec<i64> = {
                let mut stmt = conn.prepare(
                    "WITH RECURSIVE tree(id) AS ( \
                       SELECT ID FROM djmdPlaylist WHERE ID = ?1 \
                       UNION ALL \
                       SELECT p.ID FROM djmdPlaylist p JOIN tree t ON p.ParentID = CAST(t.id AS TEXT) \
                     ) SELECT id FROM tree",
                )?;
                let rows: Vec<rusqlite::Result<i64>> = stmt
                    .query_map(params![root_str], |row| row.get::<_, i64>(0))?
                    .collect();
                rows.into_iter().filter_map(|r| r.ok()).collect()
            };
            for id in ids {
                let id_str = id.to_string();
                conn.execute(
                    "DELETE FROM djmdSongPlaylist WHERE PlaylistID = ?1",
                    params![id_str],
                )?;
                conn.execute(
                    "DELETE FROM djmdPlaylist WHERE ID = ?1",
                    params![id_str],
                )?;
            }
            Ok(())
        })
    }

    pub fn create_playlist(&self, name: &str, parent_id: Option<i64>) -> Result<i64> {
        self.create_playlist_entry(name, 0, parent_id)
    }

    pub fn create_folder(&self, name: &str, parent_id: Option<i64>) -> Result<i64> {
        self.create_playlist_entry(name, 1, parent_id)
    }

    fn create_playlist_entry(&self, name: &str, attribute: i32, parent_id: Option<i64>) -> Result<i64> {
        let name = name.to_string();
        self.db.with_conn(move |conn| {
            let new_id: i64 = conn.query_row(
                "SELECT COALESCE(MAX(CAST(ID AS INTEGER)), 0) + 1 FROM djmdPlaylist",
                [],
                |row| row.get(0),
            )?;
            let new_seq: i64 = conn.query_row(
                "SELECT COALESCE(MAX(Seq), 0) + 1 FROM djmdPlaylist",
                [],
                |row| row.get(0),
            )?;
            let id_str     = new_id.to_string();
            let parent_str = parent_id.map_or_else(|| "root".to_string(), |p| p.to_string());
            conn.execute(
                "INSERT INTO djmdPlaylist (ID, Seq, Name, Attribute, ParentID, rb_local_deleted, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, datetime('now'), datetime('now'))",
                params![id_str, new_seq, name, attribute, parent_str],
            )?;
            Ok(new_id)
        })
    }

    pub fn search_tracks(&self, query: &str) -> Result<Vec<Track>> {
        let pattern = format!("%{}%", query);
        self.db.with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT c.ID, c.Title,
                        a.Name, al.Name, g.Name, k.ScaleName,
                        c.BPM, c.Length, c.Rating, c.DJPlayCount,
                        c.FolderPath, c.TrackNo,
                        l.Name, c.ColorID, c.ImagePath
                 FROM djmdContent c
                 LEFT JOIN djmdArtist  a  ON c.ArtistID  = a.ID
                 LEFT JOIN djmdAlbum   al ON c.AlbumID   = al.ID
                 LEFT JOIN djmdGenre   g  ON c.GenreID   = g.ID
                 LEFT JOIN djmdKey     k  ON c.KeyID     = k.ID
                 LEFT JOIN djmdLabel   l  ON c.LabelID   = l.ID
                 WHERE c.rb_local_deleted = 0
                   AND (c.Title   LIKE ?1 OR a.Name  LIKE ?1 OR al.Name LIKE ?1
                        OR g.Name LIKE ?1 OR l.Name  LIKE ?1
                        OR k.ScaleName LIKE ?1)
                 ORDER BY a.Name, c.Title
                 LIMIT 200",
            )?;
            let tracks = stmt.query_map(params![pattern], map_track_row)?
                .collect::<Result<Vec<_>>>()?;
            Ok(tracks)
        })
    }

    pub fn filter_tracks(&self, f: &TrackFilter) -> Result<Vec<Track>> {
        let conditions = build_filter_conditions(f);
        self.db.with_conn(move |conn| {
            let sql = format!(
                "SELECT c.ID, c.Title,
                        a.Name, al.Name, g.Name, k.ScaleName,
                        c.BPM, c.Length, c.Rating, c.DJPlayCount,
                        c.FolderPath, c.TrackNo,
                        l.Name, c.ColorID, c.ImagePath
                 FROM djmdContent c
                 LEFT JOIN djmdArtist  a  ON c.ArtistID  = a.ID
                 LEFT JOIN djmdAlbum   al ON c.AlbumID   = al.ID
                 LEFT JOIN djmdGenre   g  ON c.GenreID   = g.ID
                 LEFT JOIN djmdKey     k  ON c.KeyID     = k.ID
                 LEFT JOIN djmdLabel   l  ON c.LabelID   = l.ID
                 WHERE {}
                 ORDER BY a.Name, al.Name, c.TrackNo",
                conditions.join(" AND ")
            );
            let mut stmt = conn.prepare(&sql)?;
            let tracks = stmt.query_map([], map_track_row)?
                .collect::<Result<Vec<_>>>()?;
            Ok(tracks)
        })
    }

    pub fn filter_playlist_tracks(&self, playlist_id: i64, f: &TrackFilter) -> Result<Vec<Track>> {
        let mut conditions = build_filter_conditions(f);
        let id_str = playlist_id.to_string();
        conditions.push(format!("sp.PlaylistID = '{}'", id_str));
        self.db.with_conn(move |conn| {
            let sql = format!(
                "SELECT c.ID, c.Title,
                        a.Name, al.Name, g.Name, k.ScaleName,
                        c.BPM, c.Length, c.Rating, c.DJPlayCount,
                        c.FolderPath, sp.TrackNo,
                        l.Name, c.ColorID, c.ImagePath
                 FROM djmdSongPlaylist sp
                 JOIN  djmdContent  c  ON sp.ContentID = c.ID
                 LEFT JOIN djmdArtist  a  ON c.ArtistID  = a.ID
                 LEFT JOIN djmdAlbum   al ON c.AlbumID   = al.ID
                 LEFT JOIN djmdGenre   g  ON c.GenreID   = g.ID
                 LEFT JOIN djmdKey     k  ON c.KeyID     = k.ID
                 LEFT JOIN djmdLabel   l  ON c.LabelID   = l.ID
                 WHERE {}
                 ORDER BY sp.TrackNo",
                conditions.join(" AND ")
            );
            let mut stmt = conn.prepare(&sql)?;
            let tracks = stmt.query_map([], map_track_row)?
                .collect::<Result<Vec<_>>>()?;
            Ok(tracks)
        })
    }

    pub fn all_keys(&self) -> Result<Vec<String>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT ScaleName FROM djmdKey ORDER BY ID")?;
            let keys = stmt.query_map([], |row| row.get(0))?
                .collect::<Result<Vec<String>>>()?;
            Ok(keys)
        })
    }

    pub fn all_genres(&self) -> Result<Vec<String>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT Name FROM djmdGenre WHERE rb_local_deleted = 0 ORDER BY Name",
            )?;
            let genres = stmt.query_map([], |row| row.get(0))?
                .collect::<Result<Vec<String>>>()?;
            Ok(genres)
        })
    }

    pub fn song_my_tags(&self, content_id: i64) -> Result<Vec<String>> {
        self.db.with_conn(move |conn| {
            let id_str = content_id.to_string();
            let mut stmt = conn.prepare(
                "SELECT mt.Name
                 FROM djmdSongMyTag smt
                 JOIN djmdMyTag mt ON smt.MyTagID = mt.ID
                 WHERE smt.ContentID = ?1
                 ORDER BY mt.Seq",
            )?;
            let names = stmt.query_map(params![id_str], |row| row.get(0))?
                .collect::<Result<Vec<String>>>()?;
            Ok(names)
        })
    }

    pub fn history_sessions(&self) -> Result<Vec<HistorySession>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT h.ID, h.Name, COUNT(sh.ID) as track_count
                 FROM djmdHistory h
                 LEFT JOIN djmdSongHistory sh ON h.ID = sh.HistoryID
                 WHERE h.rb_local_deleted = 0
                 GROUP BY h.ID
                 ORDER BY h.Seq",
            )?;
            let sessions = stmt.query_map([], |row| {
                Ok(HistorySession {
                    id:          parse_id(row.get(0)?),
                    name:        row.get(1)?,
                    track_count: row.get::<_, u32>(2)?,
                })
            })?
            .collect::<Result<Vec<_>>>()?;
            Ok(sessions)
        })
    }

    pub fn history_tracks(&self, history_id: i64) -> Result<Vec<Track>> {
        self.db.with_conn(move |conn| {
            let id_str = history_id.to_string();
            let mut stmt = conn.prepare(
                "SELECT c.ID, c.Title,
                        a.Name, al.Name, g.Name, k.ScaleName,
                        c.BPM, c.Length, c.Rating, c.DJPlayCount,
                        c.FolderPath, sh.TrackNo,
                        l.Name, c.ColorID, c.ImagePath
                 FROM djmdSongHistory sh
                 JOIN  djmdContent  c  ON sh.ContentID = c.ID
                 LEFT JOIN djmdArtist  a  ON c.ArtistID  = a.ID
                 LEFT JOIN djmdAlbum   al ON c.AlbumID   = al.ID
                 LEFT JOIN djmdGenre   g  ON c.GenreID   = g.ID
                 LEFT JOIN djmdKey     k  ON c.KeyID     = k.ID
                 LEFT JOIN djmdLabel   l  ON c.LabelID   = l.ID
                 WHERE sh.HistoryID = ?1 AND c.rb_local_deleted = 0
                 ORDER BY sh.TrackNo",
            )?;
            let tracks = stmt.query_map(params![id_str], map_track_row)?
                .collect::<Result<Vec<_>>>()?;
            Ok(tracks)
        })
    }

    pub fn increment_play_count(&self, content_id: i64) -> Result<()> {
        self.db.with_conn(move |conn| {
            let id_str = content_id.to_string();
            conn.execute(
                "UPDATE djmdContent SET DJPlayCount = COALESCE(DJPlayCount, 0) + 1, \
                 updated_at = datetime('now') WHERE ID = ?1",
                params![id_str],
            )?;
            Ok(())
        })
    }

    pub fn set_rating(&self, content_id: i64, rating: i32) -> Result<()> {
        self.db.with_conn(move |conn| {
            let id_str = content_id.to_string();
            conn.execute(
                "UPDATE djmdContent SET Rating = ?2, updated_at = datetime('now') WHERE ID = ?1",
                params![id_str, rating],
            )?;
            Ok(())
        })
    }

    pub fn add_tracks_to_playlist(&self, playlist_id: i64, track_ids: &[i64]) -> Result<()> {
        let track_ids = track_ids.to_vec();
        self.db.with_conn(move |conn| {
            let pl_str = playlist_id.to_string();
            for (i, &track_id) in track_ids.iter().enumerate() {
                let new_sp_id: i64 = conn.query_row(
                    "SELECT COALESCE(MAX(CAST(ID AS INTEGER)), 0) + 1 FROM djmdSongPlaylist",
                    [], |row| row.get(0),
                )?;
                conn.execute(
                    "INSERT INTO djmdSongPlaylist \
                     (ID, PlaylistID, ContentID, TrackNo, rb_local_deleted, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, 0, datetime('now'), datetime('now'))",
                    params![new_sp_id.to_string(), pl_str, track_id.to_string(), (i + 1) as i32],
                )?;
            }
            Ok(())
        })
    }

    pub fn find_folder_by_name(&self, name: &str) -> Option<i64> {
        let name = name.to_string();
        self.db.with_conn(move |conn| {
            conn.query_row(
                "SELECT ID FROM djmdPlaylist \
                 WHERE Name = ?1 AND Attribute = 1 AND rb_local_deleted = 0",
                params![name],
                |row| row.get::<_, String>(0),
            )
        }).ok().and_then(|s| s.parse().ok())
    }

    pub fn find_or_create_folder(&self, name: &str) -> Result<i64> {
        if let Some(id) = self.find_folder_by_name(name) {
            return Ok(id);
        }
        self.create_folder(name, None)
    }

    pub fn find_or_create_subfolder(&self, name: &str, parent_id: i64) -> Result<i64> {
        let name_owned = name.to_string();
        let existing: Option<i64> = self.db.with_conn(move |conn| {
            let parent_str = parent_id.to_string();
            conn.query_row(
                "SELECT ID FROM djmdPlaylist \
                 WHERE Name = ?1 AND Attribute = 1 AND ParentID = ?2 AND rb_local_deleted = 0",
                params![name_owned, parent_str],
                |row| row.get::<_, String>(0),
            )
        }).ok().and_then(|s| s.parse().ok());
        match existing {
            Some(id) => Ok(id),
            None     => self.create_folder(name, Some(parent_id)),
        }
    }
}

impl Track {
    pub fn bpm_display(&self) -> Option<f32> {
        self.bpm.map(|b| b as f32 / 100.0)
    }
}

/// Fields that can be updated on a track. `None` = don't change.
#[derive(Debug, Clone, Default)]
pub struct TrackUpdate {
    pub title: Option<String>,
    pub artist: Option<Option<String>>,
    pub album: Option<Option<String>>,
    pub genre: Option<Option<String>>,
    pub label: Option<Option<String>>,
    pub key: Option<Option<String>>,
    pub remixer: Option<Option<String>>,
    pub year: Option<Option<i32>>,
    /// BPM as displayed (e.g. 128.0), stored ×100 in DB
    pub bpm: Option<Option<f32>>,
    pub rating: Option<Option<i32>>,
    pub color_id: Option<Option<String>>,
    // Enrichment fields (not in Rekordbox DB, only in file tags)
    pub isrc: Option<Option<String>>,
    pub acoustid_id: Option<Option<String>>,
    pub musicbrainz_recording_id: Option<Option<String>>,
}

fn build_filter_conditions(f: &TrackFilter) -> Vec<String> {
    let mut conditions = vec!["c.rb_local_deleted = 0".to_string()];
    if let Some(min) = f.bpm_min {
        if min > 0.0 {
            conditions.push(format!("c.BPM >= {}", (min * 100.0) as i32));
        }
    }
    if let Some(max) = f.bpm_max {
        if max > 0.0 {
            conditions.push(format!("c.BPM <= {}", (max * 100.0) as i32));
        }
    }
    if let Some(ref key) = f.key {
        if !key.is_empty() {
            conditions.push(format!("k.ScaleName = '{}'", key.replace('\'', "''")));
        }
    }
    if let Some(ref genre) = f.genre {
        if !genre.is_empty() {
            conditions.push(format!("g.Name = '{}'", genre.replace('\'', "''")));
        }
    }
    if let Some(min_r) = f.min_rating {
        if min_r > 0 {
            conditions.push(format!("c.Rating >= {}", min_r));
        }
    }
    conditions
}

impl Library {
    pub fn analysis_data_path(&self, content_id: i64) -> Option<String> {
        self.db.with_conn(move |conn| {
            let id_str = content_id.to_string();
            conn.query_row(
                "SELECT AnalysisDataPath FROM djmdContent WHERE ID = ?1",
                params![id_str],
                |row| row.get::<_, Option<String>>(0),
            )
        }).ok().flatten()
    }

    /// Load waveform data by parsing the ANLZ binary files on disk.
    ///
    /// Returns `(color_waveform, overview_waveform)` where:
    /// - `color_waveform`: PWV7 section from `.2EX` file — 3 bytes/col (bass, mid, high).
    /// - `overview_waveform`: PWAV section from `.DAT` file — 1 byte/col.
    ///
    /// `anlz_base` is the directory under which PIONEER/USBANLZ/… lives.
    pub fn load_waveform(&self, content_id: i64, anlz_base: &std::path::Path)
        -> Result<(Option<Vec<u8>>, Option<Vec<u8>>)>
    {
        let rel_path = match self.analysis_data_path(content_id) {
            Some(p) => p,
            None => return Ok((None, None)),
        };

        let rel = rel_path.trim_start_matches('/');
        let dat_path  = anlz_base.join(rel);
        let ex2_path  = dat_path.with_extension("2EX");

        let overview = dat_path.exists()
            .then(|| std::fs::read(&dat_path).ok())
            .flatten()
            .and_then(|data| anlz_extract_section(&data, b"PWAV"));

        let color = if ex2_path.exists() {
            std::fs::read(&ex2_path).ok()
                .and_then(|data| anlz_extract_section(&data, b"PWV7"))
        } else {
            None
        };

        Ok((color, overview))
    }

    /// Update track metadata in the Rekordbox DB. Handles upsert into
    /// lookup tables (djmdArtist, djmdAlbum, etc.) and updates djmdContent.
    pub fn update_track(&self, content_id: i64, update: &TrackUpdate) -> Result<()> {
        self.db.with_conn(move |conn| {
            let id_str = content_id.to_string();
            let mut sets: Vec<String> = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(ref title) = update.title {
                sets.push("Title = ?".into());
                params.push(Box::new(title.clone()));
            }

            if let Some(ref artist) = update.artist {
                let fk = upsert_lookup(conn, "djmdArtist", artist.as_deref())?;
                sets.push("ArtistID = ?".into());
                params.push(Box::new(fk));
            }

            if let Some(ref album) = update.album {
                let fk = upsert_lookup(conn, "djmdAlbum", album.as_deref())?;
                sets.push("AlbumID = ?".into());
                params.push(Box::new(fk));
            }

            if let Some(ref genre) = update.genre {
                let fk = upsert_lookup(conn, "djmdGenre", genre.as_deref())?;
                sets.push("GenreID = ?".into());
                params.push(Box::new(fk));
            }

            if let Some(ref label) = update.label {
                let fk = upsert_lookup(conn, "djmdLabel", label.as_deref())?;
                sets.push("LabelID = ?".into());
                params.push(Box::new(fk));
            }

            if let Some(ref key) = update.key {
                let fk = upsert_key(conn, key.as_deref())?;
                sets.push("KeyID = ?".into());
                params.push(Box::new(fk));
            }

            if let Some(ref remixer) = update.remixer {
                sets.push("Remixer = ?".into());
                params.push(Box::new(remixer.clone()));
            }

            if let Some(ref year) = update.year {
                sets.push("ReleaseYear = ?".into());
                params.push(Box::new(*year));
            }

            if let Some(ref bpm) = update.bpm {
                let db_bpm = bpm.map(|b| (b * 100.0) as i32);
                sets.push("BPM = ?".into());
                params.push(Box::new(db_bpm));
            }

            if let Some(ref rating) = update.rating {
                sets.push("Rating = ?".into());
                params.push(Box::new(*rating));
            }

            if let Some(ref color_id) = update.color_id {
                sets.push("ColorID = ?".into());
                params.push(Box::new(color_id.clone()));
            }

            if sets.is_empty() {
                return Ok(());
            }

            let sql = format!(
                "UPDATE djmdContent SET {} WHERE ID = ?",
                sets.join(", ")
            );
            params.push(Box::new(id_str));

            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();
            conn.execute(&sql, param_refs.as_slice())?;
            Ok(())
        })
    }

    pub fn load_cues(&self, content_id: i64) -> Result<Vec<CuePoint>> {
        self.db.with_conn(move |conn| {
            let id_str = content_id.to_string();
            let mut stmt = conn.prepare(
                "SELECT Kind, InMsec, OutMsec, Color, Comment
                 FROM djmdCue
                 WHERE ContentID = ?1
                 ORDER BY Kind, InMsec",
            )?;
            let cues = stmt.query_map([&id_str], |row| {
                let out_msec: i32 = row.get(2)?;
                Ok(CuePoint {
                    kind:     row.get(0)?,
                    in_secs:  row.get::<_, i32>(1)? as f64 / 1000.0,
                    out_secs: if out_msec >= 0 { Some(out_msec as f64 / 1000.0) } else { None },
                    color:    row.get(3)?,
                    comment:  row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
            Ok(cues)
        })
    }
}

/// Find or create a row in a simple lookup table (djmdArtist, djmdAlbum, etc.)
/// that has `ID` (VARCHAR) and `Name` columns. Returns the ID as Option<String>.
/// If `name` is None, returns None (clears the FK).
fn upsert_lookup(conn: &rusqlite::Connection, table: &str, name: Option<&str>) -> Result<Option<String>> {
    let name = match name {
        Some(n) if !n.is_empty() => n,
        _ => return Ok(None),
    };

    // Try to find existing
    let sql = format!("SELECT ID FROM {} WHERE Name = ?1", table);
    let existing: Option<String> = conn
        .query_row(&sql, [name], |row| row.get(0))
        .ok();

    if let Some(id) = existing {
        return Ok(Some(id));
    }

    // Insert new — generate a numeric ID from max existing + 1
    let max_sql = format!("SELECT MAX(CAST(ID AS INTEGER)) FROM {}", table);
    let max_id: i64 = conn
        .query_row(&max_sql, [], |row| row.get::<_, Option<i64>>(0))
        .unwrap_or(None)
        .unwrap_or(0);
    let new_id = (max_id + 1).to_string();

    let insert_sql = format!("INSERT INTO {} (ID, Name) VALUES (?1, ?2)", table);
    conn.execute(&insert_sql, params![&new_id, name])?;

    Ok(Some(new_id))
}

/// Find or create a key in djmdKey. Uses `ScaleName` column.
fn upsert_key(conn: &rusqlite::Connection, scale_name: Option<&str>) -> Result<Option<String>> {
    let name = match scale_name {
        Some(n) if !n.is_empty() => n,
        _ => return Ok(None),
    };

    let existing: Option<String> = conn
        .query_row("SELECT ID FROM djmdKey WHERE ScaleName = ?1", [name], |row| row.get(0))
        .ok();

    if let Some(id) = existing {
        return Ok(Some(id));
    }

    let max_id: i64 = conn
        .query_row("SELECT MAX(CAST(ID AS INTEGER)) FROM djmdKey", [], |row| row.get::<_, Option<i64>>(0))
        .unwrap_or(None)
        .unwrap_or(0);
    let new_id = (max_id + 1).to_string();

    conn.execute(
        "INSERT INTO djmdKey (ID, ScaleName) VALUES (?1, ?2)",
        params![&new_id, name],
    )?;

    Ok(Some(new_id))
}

