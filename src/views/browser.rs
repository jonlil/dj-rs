use gtk::prelude::*;
use indexmap::IndexMap;
use crate::rekordbox::{Track, Playlist, HistorySession};
use super::utils::{rating_stars, browser_fmt_duration};

// Column indices (mirrored from mod.rs)
use super::{P_NAME, P_COUNT, P_ID, P_ATTR};
use super::{T_TITLE, T_ARTIST, T_BPM, T_KEY, T_DURATION, T_FILE_PATH, T_GENRE, T_RATING, T_LABEL, T_COLOR, T_TRACK_ID, T_BPM_RAW, T_DURATION_RAW};

pub(super) fn browser_populate_playlists(store: &gtk::TreeStore, playlists: &[Playlist]) {
    store.clear();
    store.insert_with_values(
        None, None,
        &[P_NAME, P_COUNT, P_ID, P_ATTR],
        &[&"★ All Tracks", &"", &"all", &"0"],
    );

    // IndexMap preserves insertion order, which matches the DB's ORDER BY Seq
    let mut children: IndexMap<Option<i64>, Vec<&Playlist>> = IndexMap::new();
    for pl in playlists {
        children.entry(pl.parent_id).or_default().push(pl);
    }

    fn insert_node(
        store: &gtk::TreeStore,
        children: &IndexMap<Option<i64>, Vec<&Playlist>>,
        parent_id: Option<i64>,
        parent_iter: Option<&gtk::TreeIter>,
    ) {
        if let Some(nodes) = children.get(&parent_id) {
            for pl in nodes {
                // Hide top-level gig output folders from the browsing tree
                if parent_id.is_none() && crate::gig::GIG_FOLDERS.contains(&pl.name.as_str()) {
                    continue;
                }
                let name = if pl.attribute == 1 {
                    format!("▸ {}", pl.name)
                } else {
                    pl.name.clone()
                };
                let count = if pl.attribute == 1 {
                    String::new()
                } else {
                    pl.track_count.to_string()
                };
                let iter = store.insert_with_values(
                    parent_iter, None,
                    &[P_NAME, P_COUNT, P_ID, P_ATTR],
                    &[&name.as_str(), &count.as_str(), &pl.id.to_string().as_str(), &pl.attribute.to_string().as_str()],
                );
                if pl.attribute == 1 {
                    insert_node(store, children, Some(pl.id), Some(&iter));
                }
            }
        }
    }

    insert_node(store, &children, None, None);
}

pub(super) fn browser_populate_history(store: &gtk::ListStore, sessions: &[HistorySession]) {
    store.clear();
    for s in sessions {
        let id  = format!("h:{}", s.id);
        let cnt = s.track_count.to_string();
        store.insert_with_values(
            None,
            &[P_NAME, P_COUNT, P_ID, P_ATTR],
            &[&s.name.as_str(), &cnt.as_str(), &id.as_str(), &"h"],
        );
    }
}

pub(super) fn browser_populate_tracks(store: &gtk::ListStore, tracks: &[Track]) {
    store.clear();
    for t in tracks {
        let bpm       = t.bpm_display().map(|b| format!("{:.1}", b)).unwrap_or_default();
        let key       = t.key.as_deref().unwrap_or("").to_string();
        let artist    = t.artist.as_deref().unwrap_or("").to_string();
        let duration  = t.duration_secs.map(browser_fmt_duration).unwrap_or_default();
        let file_path = t.file_path.as_deref().unwrap_or("").to_string();
        let genre     = t.genre.as_deref().unwrap_or("").to_string();
        let rating    = rating_stars(t.rating.unwrap_or(0)).to_string();
        let label     = t.label.as_deref().unwrap_or("").to_string();
        let color_id  = t.color_id.as_deref().unwrap_or("").to_string();
        let track_id  = t.id.to_string();
        let bpm_raw   = t.bpm.unwrap_or(0).to_string();
        let dur_raw   = t.duration_secs.unwrap_or(0).to_string();
        store.insert_with_values(
            None,
            &[T_TITLE, T_ARTIST, T_BPM, T_KEY, T_DURATION,
              T_FILE_PATH, T_GENRE, T_RATING, T_LABEL, T_COLOR, T_TRACK_ID,
              T_BPM_RAW, T_DURATION_RAW],
            &[
                &t.title.as_str(),
                &artist.as_str(),
                &bpm.as_str(),
                &key.as_str(),
                &duration.as_str(),
                &file_path.as_str(),
                &genre.as_str(),
                &rating.as_str(),
                &label.as_str(),
                &color_id.as_str(),
                &track_id.as_str(),
                &bpm_raw.as_str(),
                &dur_raw.as_str(),
            ],
        );
    }
}
