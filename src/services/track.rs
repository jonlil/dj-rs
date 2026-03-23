use std::path::Path;
use crate::config::Config;
use crate::rekordbox::{Library, Track, TrackUpdate};
use crate::spotify::SpotifyTrack;
use crate::tags::TagUpdate;

// ── Query types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum TrackQuery {
    All,
    Playlist(i64),
}

// ── Result types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SpotifyPlaylistMatch {
    pub spotify: SpotifyTrack,
    pub in_library: bool,
    pub library_track_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct GigMatchEntry {
    pub spotify: SpotifyTrack,
    pub matched_track_id: Option<i64>,
    pub matched_title: Option<String>,
    pub matched_artist: Option<String>,
}

// ── Service functions ───────────────────────────────────────────────────────

/// Save track metadata to both the Rekordbox DB and the audio file tags.
/// Returns an error message if either write fails (but attempts both).
pub fn save_track_metadata(
    lib: &Library,
    track_id: i64,
    update: &TrackUpdate,
    config: &Config,
) -> Result<(), String> {
    // Write to Rekordbox DB
    lib.update_track(track_id, update)
        .map_err(|e| format!("DB update failed: {e}"))?;

    // Write to file tags if we can resolve the path
    if let Some(track) = lib.track_by_id(track_id).ok().flatten() {
        if let Some(ref fp) = track.file_path {
            let resolved = config.apply_mappings(fp);
            let path = Path::new(&resolved);
            if path.exists() {
                let tag_update = TagUpdate {
                    title: update.title.clone(),
                    artist: update.artist.clone().flatten(),
                    album: update.album.clone().flatten(),
                    genre: update.genre.clone().flatten(),
                    label: update.label.clone().flatten(),
                    key: update.key.clone().flatten(),
                    remixer: update.remixer.clone().flatten(),
                    year: update.year.flatten(),
                    bpm: update.bpm.flatten(),
                    isrc: update.isrc.clone().flatten(),
                    acoustid_id: update.acoustid_id.clone().flatten(),
                    musicbrainz_recording_id: update.musicbrainz_recording_id.clone().flatten(),
                };
                if let Err(e) = crate::tags::write_tags(path, &tag_update) {
                    return Err(format!("Tag write failed: {e}"));
                }
            }
        }
    }

    Ok(())
}

pub fn query_tracks(lib: &Library, query: TrackQuery) -> Vec<Track> {
    match query {
        TrackQuery::All => lib.tracks().unwrap_or_default(),
        TrackQuery::Playlist(id) => lib.playlist_tracks(id).unwrap_or_default(),
    }
}

pub fn search_tracks(lib: &Library, query: &str) -> Vec<Track> {
    lib.search_tracks(query).unwrap_or_default()
}

pub fn match_spotify_playlist(
    token: &str,
    playlist_id: &str,
    lib: &Library,
) -> Result<Vec<SpotifyPlaylistMatch>, String> {
    let spotify_tracks = crate::spotify::fetch_playlist(token, playlist_id)?;
    let library_tracks = lib.tracks().map_err(|e| e.to_string())?;
    let results = crate::matcher::match_tracks(&spotify_tracks, &library_tracks);

    Ok(results.into_iter().map(|r| SpotifyPlaylistMatch {
        in_library: r.matched.is_some(),
        library_track_id: r.matched.as_ref().map(|t| t.id),
        spotify: r.spotify,
    }).collect())
}

pub fn match_gig_playlist(
    token: &str,
    playlist_url: &str,
    lib: &Library,
) -> Result<Vec<GigMatchEntry>, String> {
    let spotify_tracks = crate::spotify::fetch_playlist(token, playlist_url)?;
    let library_tracks = lib.tracks().map_err(|e| e.to_string())?;
    let results = crate::matcher::match_tracks(&spotify_tracks, &library_tracks);

    Ok(results.into_iter().map(|r| GigMatchEntry {
        matched_track_id: r.matched.as_ref().map(|t| t.id),
        matched_title: r.matched.as_ref().map(|t| t.title.clone()),
        matched_artist: r.matched.as_ref().and_then(|t| t.artist.clone()),
        spotify: r.spotify,
    }).collect())
}
