use std::path::{Path, PathBuf};
use std::process::Command;

use dj_rs::transcode::{self, AudioFormat, ImportAction};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture(name: &str) -> PathBuf {
    Path::new(FIXTURES).join(name)
}

/// Generate a test tone via ffmpeg. Skips (returns None) if ffmpeg is not installed.
fn ffmpeg_tone(name: &str, duration_secs: u32, extra_args: &[&str]) -> Option<PathBuf> {
    let out = fixture(name);
    if out.exists() {
        return Some(out);
    }
    let status = Command::new("ffmpeg")
        .args(["-f", "lavfi", "-i", &format!("sine=frequency=440:duration={duration_secs}")])
        .args(["-ar", "44100", "-ac", "2"])
        .args(extra_args)
        .args(["-y"])
        .arg(&out)
        .output()
        .ok()?;
    status.status.success().then_some(out)
}

/// Generate all short fixtures needed by the test suite (1 second each).
fn ensure_fixtures() {
    let specs: &[(&str, &[&str])] = &[
        ("tone.wav", &["-sample_fmt", "s16"]),
        ("tone_24bit.wav", &["-c:a", "pcm_s24le"]),
        ("tone.flac", &["-sample_fmt", "s16"]),
        ("tone.aif", &[]),
        ("tone.mp3", &["-c:a", "libmp3lame", "-q:a", "9"]),
        ("tone.m4a", &["-c:a", "aac", "-b:a", "64k"]),
        ("tone.ogg", &["-c:a", "libvorbis", "-q:a", "0"]),
        ("tone.opus", &["-c:a", "libopus", "-b:a", "64k"]),
        ("tone.aac", &["-c:a", "aac", "-b:a", "64k", "-f", "adts"]),
        ("tone.wv", &["-c:a", "wavpack"]),
    ];
    for (name, args) in specs {
        ffmpeg_tone(name, 1, args);
    }
}

// ── classify ────────────────────────────────────────────────────────────────

#[test]
fn classify_all_formats() {
    let cases = [
        ("tone.aif", AudioFormat::Aiff),
        ("tone.mp3", AudioFormat::Mp3),
        ("tone.m4a", AudioFormat::M4aAac),
        ("tone.wav", AudioFormat::Wav),
        ("tone.flac", AudioFormat::Flac),
        ("tone.ogg", AudioFormat::OggVorbis),
        ("tone.opus", AudioFormat::Opus),
        ("tone.wv", AudioFormat::WavPack),
        ("tone.aac", AudioFormat::Aac),
    ];
    for (file, expected) in cases {
        assert_eq!(transcode::classify(Path::new(file)), expected, "classify({file})");
    }
}

// ── import_action ───────────────────────────────────────────────────────────

#[test]
fn import_action_keep() {
    for fmt in [AudioFormat::Aiff, AudioFormat::Mp3, AudioFormat::M4aAac] {
        assert_eq!(transcode::import_action(fmt), ImportAction::Keep, "{fmt:?}");
    }
}

#[test]
fn import_action_to_aiff() {
    for fmt in [AudioFormat::Wav, AudioFormat::Flac, AudioFormat::WavPack, AudioFormat::Ape] {
        assert_eq!(transcode::import_action(fmt), ImportAction::ToAiff, "{fmt:?}");
    }
}

#[test]
fn import_action_to_m4a() {
    for fmt in [AudioFormat::OggVorbis, AudioFormat::Opus, AudioFormat::Aac] {
        assert_eq!(transcode::import_action(fmt), ImportAction::ToM4a, "{fmt:?}");
    }
}

// ── fingerprint ─────────────────────────────────────────────────────────────

#[test]
fn fingerprint_wav() {
    ensure_fixtures();
    let fp = transcode::fingerprint(&fixture("tone.wav")).unwrap();
    assert!(!fp.is_empty());
}

#[test]
fn fingerprint_flac() {
    ensure_fixtures();
    let fp = transcode::fingerprint(&fixture("tone.flac")).unwrap();
    assert!(!fp.is_empty());
}

#[test]
fn fingerprint_same_content_matches() {
    ensure_fixtures();
    let fp_wav = transcode::fingerprint(&fixture("tone.wav")).unwrap();
    let fp_flac = transcode::fingerprint(&fixture("tone.flac")).unwrap();
    assert_eq!(fp_wav, fp_flac, "WAV and FLAC of same source should have identical fingerprints");
}

// ── convert_to_aiff ─────────────────────────────────────────────────────────

fn convert_and_verify(src_name: &str) {
    ensure_fixtures();
    let src = fixture(src_name);
    let tmp = tempfile::tempdir().unwrap();
    let result = transcode::convert_to_aiff(&src, tmp.path()).unwrap();
    assert!(result.dst_path.exists());
    assert_eq!(result.dst_path.extension().unwrap(), "aif");
    assert!(!result.fingerprint.is_empty());
}

#[test]
fn convert_wav_to_aiff() {
    convert_and_verify("tone.wav");
}

#[test]
fn convert_flac_to_aiff() {
    convert_and_verify("tone.flac");
}

#[test]
fn convert_24bit_wav_to_aiff() {
    convert_and_verify("tone_24bit.wav");
}

#[test]
#[ignore] // symphonia doesn't support WavPack decoding yet
fn convert_wavpack_to_aiff() {
    convert_and_verify("tone.wv");
}

#[test]
fn convert_preserves_fingerprint() {
    ensure_fixtures();
    let src = fixture("tone.wav");
    let tmp = tempfile::tempdir().unwrap();
    let result = transcode::convert_to_aiff(&src, tmp.path()).unwrap();

    let dst_fp = transcode::fingerprint(&result.dst_path).unwrap();
    assert_eq!(result.fingerprint, dst_fp);
}

// ── benchmark fixture generator ─────────────────────────────────────────────

#[test]
#[ignore] // run manually: cargo test --test transcode -- --ignored generate_bench_fixtures
fn generate_bench_fixtures() {
    let specs: &[(&str, u32, &[&str])] = &[
        ("bench_3min.flac", 180, &["-sample_fmt", "s16"]),
        ("bench_3min_24bit.flac", 180, &["-c:a", "flac", "-sample_fmt", "s32"]),
    ];
    for (name, dur, args) in specs {
        let path = ffmpeg_tone(name, *dur, args).expect("ffmpeg required for benchmark fixtures");
        eprintln!("generated {}", path.display());
    }
}
