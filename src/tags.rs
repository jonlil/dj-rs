/// Audio file tag reading, writing, and migration.
///
/// When converting between formats (e.g. FLAC → AIFF), custom tags like
/// AcoustID and MusicBrainz Recording ID are not automatically carried over.
/// `migrate_tags` explicitly copies the tags we care about from source to
/// destination. If the destination has no primary tag, one is created (ID3v2).
use std::path::Path;
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{ItemKey, ItemValue, TagItem};

const MIGRATE_CUSTOM_KEYS: &[&str] = &[
    "Acoustid Id",
    "MusicBrainz Recording Id",
];

fn read_custom(tag: &lofty::tag::Tag, key: &str) -> Option<String> {
    tag.get_string(&ItemKey::Unknown(key.to_string()))
        .map(|s| s.to_string())
}

fn write_item(tag: &mut lofty::tag::Tag, key: ItemKey, value: &str) {
    tag.push(TagItem::new(key, ItemValue::Text(value.to_string())));
}

/// Fields that can be written to audio file tags.
#[derive(Debug, Clone, Default)]
pub struct TagUpdate {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub label: Option<String>,
    pub key: Option<String>,
    pub remixer: Option<String>,
    pub year: Option<i32>,
    pub bpm: Option<f32>,
    pub isrc: Option<String>,
    pub acoustid_id: Option<String>,
    pub musicbrainz_recording_id: Option<String>,
}

/// Write metadata tags to an audio file. Only fields that are `Some` are written;
/// existing tags for `None` fields are left untouched.
pub fn write_tags(path: &Path, update: &TagUpdate) -> Result<(), String> {
    let mut file = Probe::open(path)
        .map_err(|e| format!("lofty open: {e}"))?
        .read()
        .map_err(|e| format!("lofty read: {e}"))?;

    if file.primary_tag().is_none() {
        file.insert_tag(lofty::tag::Tag::new(lofty::tag::TagType::Id3v2));
    }
    let tag = file.primary_tag_mut().unwrap();

    if let Some(ref v) = update.title {
        tag.set_title(v.clone());
    }
    if let Some(ref v) = update.artist {
        tag.set_artist(v.clone());
    }
    if let Some(ref v) = update.album {
        tag.set_album(v.clone());
    }
    if let Some(ref v) = update.genre {
        tag.set_genre(v.clone());
    }
    if let Some(ref v) = update.label {
        write_item(tag, ItemKey::Label, v);
    }
    if let Some(ref v) = update.key {
        write_item(tag, ItemKey::InitialKey, v);
    }
    if let Some(ref v) = update.remixer {
        write_item(tag, ItemKey::Remixer, v);
    }
    if let Some(year) = update.year {
        write_item(tag, ItemKey::Year, &year.to_string());
    }
    if let Some(bpm) = update.bpm {
        write_item(tag, ItemKey::Bpm, &format!("{:.0}", bpm));
    }
    if let Some(ref v) = update.isrc {
        write_item(tag, ItemKey::Isrc, v);
    }
    if let Some(ref v) = update.acoustid_id {
        write_item(tag, ItemKey::Unknown("Acoustid Id".to_string()), v);
    }
    if let Some(ref v) = update.musicbrainz_recording_id {
        write_item(tag, ItemKey::Unknown("MusicBrainz Recording Id".to_string()), v);
    }

    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("open for write: {e}"))?;

    file.save_to(&mut f, lofty::config::WriteOptions::default())
        .map_err(|e| format!("lofty save: {e}"))
}

/// Migrate ISRC, AcoustID, and MusicBrainz Recording ID from `src` to `dst`.
pub fn migrate_tags(src: &Path, dst: &Path) -> Result<(), String> {
    let src_file = Probe::open(src)
        .map_err(|e| format!("lofty open src: {e}"))?
        .read()
        .map_err(|e| format!("lofty read src: {e}"))?;

    let src_tag = match src_file.primary_tag() {
        Some(t) => t,
        None => return Ok(()),
    };

    let isrc: Option<String> = src_tag.get_string(&ItemKey::Isrc).map(|s| s.to_string());
    let custom: Vec<(String, String)> = MIGRATE_CUSTOM_KEYS.iter()
        .filter_map(|&k| read_custom(src_tag, k).map(|v| (k.to_string(), v)))
        .collect();

    if isrc.is_none() && custom.is_empty() {
        return Ok(());
    }

    let mut dst_file = Probe::open(dst)
        .map_err(|e| format!("lofty open dst: {e}"))?
        .read()
        .map_err(|e| format!("lofty read dst: {e}"))?;

    {
        if dst_file.primary_tag().is_none() {
            dst_file.insert_tag(lofty::tag::Tag::new(lofty::tag::TagType::Id3v2));
        }
        let dst_tag = dst_file.primary_tag_mut().unwrap();

        if let Some(ref v) = isrc {
            write_item(dst_tag, ItemKey::Isrc, v);
        }
        for (key, value) in &custom {
            write_item(dst_tag, ItemKey::Unknown(key.clone()), value);
        }
    }

    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dst)
        .map_err(|e| format!("open dst for write: {e}"))?;

    dst_file.save_to(&mut f, lofty::config::WriteOptions::default())
        .map_err(|e| format!("lofty save: {e}"))
}
