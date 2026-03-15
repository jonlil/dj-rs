/// Tag migration for transcoded audio files.
///
/// When transcoding (e.g. M4A → FLAC), ffmpeg copies standard tags but
/// custom atoms like AcoustID and MusicBrainz Recording ID may be lost or
/// renamed. This module explicitly migrates the tags we care about.
///
/// Tags migrated:
///   ISRC                      — International Standard Recording Code
///   Acoustid Id               — AcoustID fingerprint lookup result
///   MusicBrainz Recording Id  — MusicBrainz recording UUID
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
        let dst_tag = match dst_file.primary_tag_mut() {
            Some(t) => t,
            None => return Err("destination file has no primary tag".to_string()),
        };

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
