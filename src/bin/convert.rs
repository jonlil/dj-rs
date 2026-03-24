use std::path::{Path, PathBuf};

use dj_rs::config::Config;
use dj_rs::rekordbox::Library;
use dj_rs::transcode::{self, ImportAction};

struct ConvertEntry {
    track_id: i64,
    title: String,
    artist: Option<String>,
    db_path: String,
    resolved_path: PathBuf,
    action: ImportAction,
}

impl ConvertEntry {
    fn label(&self) -> String {
        let artist = self.artist.as_deref().unwrap_or("Unknown");
        format!("{artist} — {}", self.title)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let convert = args.iter().any(|a| a == "--no-dry-run");
    let delete_originals = args.iter().any(|a| a == "--delete-originals");
    let config = Config::load();

    let db_path = config.db_path.as_deref().unwrap_or_else(|| {
        eprintln!("No db_path in config");
        std::process::exit(1);
    });

    let lib = Library::open(db_path).unwrap_or_else(|e| {
        eprintln!("Failed to open library: {e}");
        std::process::exit(1);
    });

    let tracks = lib.tracks().unwrap_or_else(|e| {
        eprintln!("Failed to load tracks: {e}");
        std::process::exit(1);
    });

    let mut to_convert = Vec::new();
    let mut keep = 0u32;
    let mut missing = 0u32;
    let mut unsupported = 0u32;

    for track in &tracks {
        let Some(ref fp) = track.file_path else { continue };
        let resolved = config.apply_mappings(fp);
        let path = Path::new(&resolved);

        if !path.exists() {
            missing += 1;
            continue;
        }

        let fmt = transcode::classify(path);
        let action = transcode::import_action(fmt);

        match action {
            ImportAction::Keep => keep += 1,
            ImportAction::Unsupported => {
                unsupported += 1;
                println!("  SKIP  {fmt:?}  {}", path.display());
            }
            ImportAction::ToAiff | ImportAction::ToM4a => {
                to_convert.push(ConvertEntry {
                    track_id: track.id,
                    title: track.title.clone(),
                    artist: track.artist.clone(),
                    db_path: fp.clone(),
                    resolved_path: path.to_path_buf(),
                    action,
                });
            }
        }
    }

    println!("\n--- Library scan ---");
    println!("  Total tracks:  {}", tracks.len());
    println!("  Already OK:    {keep}");
    println!("  Missing file:  {missing}");
    println!("  Unsupported:   {unsupported}");
    println!("  To convert:    {}", to_convert.len());

    for entry in &to_convert {
        let action_str = match entry.action {
            ImportAction::ToAiff => "→ AIFF",
            ImportAction::ToM4a => "→ M4A ",
            _ => unreachable!(),
        };
        println!("  {action_str}  {}  ({})", entry.label(), entry.resolved_path.display());
    }

    if !convert {
        if !to_convert.is_empty() {
            println!("\nDry run. Pass --no-dry-run to transcode. Add --delete-originals to remove source files after verified conversion.");
        }
        return;
    }

    println!("\n--- Converting (with chromaprint verification) ---");
    let mut ok = 0u32;
    let mut err = 0u32;

    for entry in &to_convert {
        let new_ext = match entry.action {
            ImportAction::ToAiff => "aif",
            ImportAction::ToM4a => {
                println!("  TODO  {}  (M4A not yet implemented)", entry.label());
                continue;
            }
            _ => continue,
        };

        let dst_dir = entry.resolved_path.parent().unwrap_or(Path::new("."));

        match transcode::convert_to_aiff(&entry.resolved_path, dst_dir) {
            Ok(result) => {
                // Tags
                if let Err(e) = dj_rs::tags::migrate_tags(&entry.resolved_path, &result.dst_path) {
                    eprintln!("  WARN  tag migration: {e}");
                }

                // Update FolderPath in Rekordbox DB
                let new_db_path = Path::new(&entry.db_path).with_extension(new_ext);
                if let Err(e) = lib.update_track_path(entry.track_id, &new_db_path.to_string_lossy()) {
                    eprintln!("  WARN  DB path update: {e}");
                }

                if delete_originals {
                    if let Err(e) = std::fs::remove_file(&entry.resolved_path) {
                        eprintln!("  WARN  delete original: {e}");
                    }
                }

                println!("  OK    {}  [fp verified]", entry.label());
                ok += 1;
            }
            Err(e) => {
                eprintln!("  FAIL  {}: {e}", entry.label());
                err += 1;
            }
        }
    }

    println!("\n--- Done ---");
    println!("  Converted: {ok}");
    println!("  Failed:    {err}");
}
