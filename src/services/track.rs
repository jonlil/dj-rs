use crate::rekordbox::{Library, Track};
use crate::spotify::SpotifyTrack;

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
