use rusqlite::{Connection, OpenFlags, Result, params};

const DB_KEY: &str = "402fd482c38817c35ffa8ffb8c7d93143b749e7d315df7a81732a1ff43608497";

// All ID columns in this database are declared VARCHAR(255) and stored as TEXT.
// rusqlite's get::<_, i64>() returns InvalidType for TEXT cells, so we always
// read them as String and parse.
fn parse_id(s: String) -> i64 {
    s.parse().unwrap_or(0)
}

pub struct Library {
    conn: Connection,
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
pub struct CuePoint {
    pub content_id: i64,
    pub in_msec: i32,
    /// 0 = memory cue, 1 = loop, 3 = hot cue
    pub kind: i32,
    pub color: Option<i32>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MyTag {
    pub id: i64,
    pub name: String,
    pub seq: i32,
}

#[derive(Debug, Clone)]
pub struct HistorySession {
    pub id: i64,
    pub name: String,
    pub track_count: u32,
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
    })
}

impl Library {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(&format!(
            "PRAGMA key = '{DB_KEY}';
             PRAGMA cipher_page_size = 4096;
             PRAGMA kdf_iter = 256000;
             PRAGMA cipher_hmac_algorithm = HMAC_SHA512;
             PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;"
        ))?;
        conn.execute_batch("SELECT count(*) FROM djmdContent;")?;
        Ok(Library { conn })
    }

    pub fn tracks(&self) -> Result<Vec<Track>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.ID, c.Title,
                    a.Name, al.Name, g.Name, k.ScaleName,
                    c.BPM, c.Length, c.Rating, c.DJPlayCount,
                    c.FolderPath, c.TrackNo,
                    l.Name, c.ColorID
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
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>> {
        let mut stmt = self.conn.prepare(
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
    }

    pub fn playlist_tracks(&self, playlist_id: i64) -> Result<Vec<Track>> {
        let id_str = playlist_id.to_string();
        let mut stmt = self.conn.prepare(
            "SELECT c.ID, c.Title,
                    a.Name, al.Name, g.Name, k.ScaleName,
                    c.BPM, c.Length, c.Rating, c.DJPlayCount,
                    c.FolderPath, sp.TrackNo,
                    l.Name, c.ColorID
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
    }

    pub fn cues_for_track(&self, content_id: i64) -> Result<Vec<CuePoint>> {
        let id_str = content_id.to_string();
        let mut stmt = self.conn.prepare(
            "SELECT ContentID, InMsec, Kind, Color, Comment
             FROM djmdCue
             WHERE ContentID = ?1
             ORDER BY InMsec",
        )?;

        let cues = stmt.query_map(params![id_str], |row| {
            Ok(CuePoint {
                content_id: parse_id(row.get(0)?),
                in_msec:    row.get(1)?,
                kind:       row.get(2)?,
                color:      row.get(3)?,
                comment:    row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;

        Ok(cues)
    }

    pub fn move_playlist(&self, id: i64, new_parent_id: Option<i64>) -> Result<()> {
        let id_str     = id.to_string();
        let parent_str = new_parent_id
            .map_or_else(|| "root".to_string(), |p| p.to_string());
        self.conn.execute(
            "UPDATE djmdPlaylist SET ParentID = ?2, updated_at = datetime('now') WHERE ID = ?1",
            params![id_str, parent_str],
        )?;
        Ok(())
    }

    pub fn reorder_playlists(&self, ordered_ids: &[i64]) -> Result<()> {
        self.conn.execute_batch("BEGIN;")?;
        for (i, &id) in ordered_ids.iter().enumerate() {
            let seq    = (i + 1) as i64;
            let id_str = id.to_string();
            self.conn.execute(
                "UPDATE djmdPlaylist SET Seq = ?2, updated_at = datetime('now') WHERE ID = ?1",
                params![id_str, seq],
            )?;
        }
        self.conn.execute_batch("COMMIT;")?;
        Ok(())
    }

    pub fn delete_playlist(&self, playlist_id: i64) -> Result<()> {
        let id_str = playlist_id.to_string();
        self.conn.execute(
            "DELETE FROM djmdSongPlaylist WHERE PlaylistID = ?1",
            params![id_str],
        )?;
        self.conn.execute(
            "DELETE FROM djmdPlaylist WHERE ID = ?1",
            params![id_str],
        )?;
        Ok(())
    }

    pub fn create_playlist(&self, name: &str, parent_id: Option<i64>) -> Result<i64> {
        self.create_playlist_entry(name, 0, parent_id)
    }

    pub fn create_folder(&self, name: &str, parent_id: Option<i64>) -> Result<i64> {
        self.create_playlist_entry(name, 1, parent_id)
    }

    fn create_playlist_entry(&self, name: &str, attribute: i32, parent_id: Option<i64>) -> Result<i64> {
        let new_id: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(CAST(ID AS INTEGER)), 0) + 1 FROM djmdPlaylist",
            [],
            |row| row.get(0),
        )?;
        let new_seq: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(Seq), 0) + 1 FROM djmdPlaylist",
            [],
            |row| row.get(0),
        )?;
        let id_str     = new_id.to_string();
        let parent_str = parent_id.map_or_else(|| "root".to_string(), |p| p.to_string());
        self.conn.execute(
            "INSERT INTO djmdPlaylist (ID, Seq, Name, Attribute, ParentID, rb_local_deleted, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, datetime('now'), datetime('now'))",
            params![id_str, new_seq, name, attribute, parent_str],
        )?;
        Ok(new_id)
    }

    pub fn search_tracks(&self, query: &str) -> Result<Vec<Track>> {
        let pattern = format!("%{}%", query);
        let mut stmt = self.conn.prepare(
            "SELECT c.ID, c.Title,
                    a.Name, al.Name, g.Name, k.ScaleName,
                    c.BPM, c.Length, c.Rating, c.DJPlayCount,
                    c.FolderPath, c.TrackNo,
                    l.Name, c.ColorID
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
    }

    pub fn filter_tracks(&self, f: &TrackFilter) -> Result<Vec<Track>> {
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

        let sql = format!(
            "SELECT c.ID, c.Title,
                    a.Name, al.Name, g.Name, k.ScaleName,
                    c.BPM, c.Length, c.Rating, c.DJPlayCount,
                    c.FolderPath, c.TrackNo,
                    l.Name, c.ColorID
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

        let mut stmt = self.conn.prepare(&sql)?;
        let tracks = stmt.query_map([], map_track_row)?
            .collect::<Result<Vec<_>>>()?;
        Ok(tracks)
    }

    pub fn filter_playlist_tracks(&self, playlist_id: i64, f: &TrackFilter) -> Result<Vec<Track>> {
        let id_str = playlist_id.to_string();
        let mut conditions = vec![
            "c.rb_local_deleted = 0".to_string(),
            format!("sp.PlaylistID = '{}'", id_str),
        ];
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

        let sql = format!(
            "SELECT c.ID, c.Title,
                    a.Name, al.Name, g.Name, k.ScaleName,
                    c.BPM, c.Length, c.Rating, c.DJPlayCount,
                    c.FolderPath, sp.TrackNo,
                    l.Name, c.ColorID
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

        let mut stmt = self.conn.prepare(&sql)?;
        let tracks = stmt.query_map([], map_track_row)?
            .collect::<Result<Vec<_>>>()?;
        Ok(tracks)
    }

    pub fn all_keys(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT ScaleName FROM djmdKey ORDER BY ID")?;
        let keys = stmt.query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>>>()?;
        Ok(keys)
    }

    pub fn all_genres(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT Name FROM djmdGenre WHERE rb_local_deleted = 0 ORDER BY Name",
        )?;
        let genres = stmt.query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>>>()?;
        Ok(genres)
    }

    pub fn my_tags(&self) -> Result<Vec<MyTag>> {
        let mut stmt = self.conn.prepare(
            "SELECT ID, Name, Seq FROM djmdMyTag WHERE rb_local_deleted = 0 ORDER BY Seq",
        )?;
        let tags = stmt.query_map([], |row| {
            Ok(MyTag {
                id:   parse_id(row.get(0)?),
                name: row.get(1)?,
                seq:  row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
        Ok(tags)
    }

    pub fn song_my_tags(&self, content_id: i64) -> Result<Vec<String>> {
        let id_str = content_id.to_string();
        let mut stmt = self.conn.prepare(
            "SELECT mt.Name
             FROM djmdSongMyTag smt
             JOIN djmdMyTag mt ON smt.MyTagID = mt.ID
             WHERE smt.ContentID = ?1
             ORDER BY mt.Seq",
        )?;
        let names = stmt.query_map(params![id_str], |row| row.get(0))?
            .collect::<Result<Vec<String>>>()?;
        Ok(names)
    }

    pub fn history_sessions(&self) -> Result<Vec<HistorySession>> {
        let mut stmt = self.conn.prepare(
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
    }

    pub fn history_tracks(&self, history_id: i64) -> Result<Vec<Track>> {
        let id_str = history_id.to_string();
        let mut stmt = self.conn.prepare(
            "SELECT c.ID, c.Title,
                    a.Name, al.Name, g.Name, k.ScaleName,
                    c.BPM, c.Length, c.Rating, c.DJPlayCount,
                    c.FolderPath, sh.TrackNo,
                    l.Name, c.ColorID
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
    }

    pub fn increment_play_count(&self, content_id: i64) -> Result<()> {
        let id_str = content_id.to_string();
        self.conn.execute(
            "UPDATE djmdContent SET DJPlayCount = COALESCE(DJPlayCount, 0) + 1, \
             updated_at = datetime('now') WHERE ID = ?1",
            params![id_str],
        )?;
        Ok(())
    }

    pub fn set_rating(&self, content_id: i64, rating: i32) -> Result<()> {
        let id_str = content_id.to_string();
        self.conn.execute(
            "UPDATE djmdContent SET Rating = ?2, updated_at = datetime('now') WHERE ID = ?1",
            params![id_str, rating],
        )?;
        Ok(())
    }
}

impl Track {
    pub fn bpm_display(&self) -> Option<f32> {
        self.bpm.map(|b| b as f32 / 100.0)
    }
}

// ─── Camelot wheel ────────────────────────────────────────────────────────────

static CAMELOT_WHEEL: &[(&str, &[&str])] = &[
    ("Am",      &["Am", "Em", "Dm", "C"]),
    ("Em",      &["Em", "Bm", "Am", "G"]),
    ("Bm",      &["Bm", "F#m", "Em", "D"]),
    ("F#m",     &["F#m", "C#m", "Bm", "A"]),
    ("C#m",     &["C#m", "G#m", "F#m", "E"]),
    ("Dbm",     &["C#m", "G#m", "F#m", "E"]),
    ("G#m",     &["G#m", "D#m", "C#m", "B"]),
    ("Abm",     &["G#m", "D#m", "C#m", "B"]),
    ("D#m",     &["D#m", "Bbm", "G#m", "F#"]),
    ("Ebm",     &["D#m", "Bbm", "G#m", "F#"]),
    ("Bbm",     &["Bbm", "Fm", "D#m", "Db"]),
    ("Fm",      &["Fm", "Cm", "Bbm", "Ab"]),
    ("Cm",      &["Cm", "Gm", "Fm", "Eb"]),
    ("Gm",      &["Gm", "Dm", "Cm", "Bb"]),
    ("Dm",      &["Dm", "Am", "Gm", "F"]),
    ("C",       &["C", "G", "Am", "F"]),
    ("G",       &["G", "D", "Em", "C"]),
    ("D",       &["D", "A", "Bm", "G"]),
    ("A",       &["A", "E", "F#m", "D"]),
    ("E",       &["E", "B", "C#m", "A"]),
    ("B",       &["B", "F#", "G#m", "E"]),
    ("F#",      &["F#", "Db", "D#m", "B"]),
    ("Gb",      &["F#", "Db", "D#m", "B"]),
    ("Db",      &["Db", "Ab", "Bbm", "F#"]),
    ("Ab",      &["Ab", "Eb", "Fm", "Db"]),
    ("Eb",      &["Eb", "Bb", "Cm", "Ab"]),
    ("Bb",      &["Bb", "F", "Gm", "Eb"]),
    ("F",       &["F", "C", "Dm", "Bb"]),
];

pub fn compatible_camelot_keys(key_name: &str) -> Vec<&'static str> {
    for (key, compatible) in CAMELOT_WHEEL {
        if *key == key_name {
            return compatible.to_vec();
        }
    }
    vec![]
}
