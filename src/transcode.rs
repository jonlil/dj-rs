use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use rusty_chromaprint::{Configuration, Fingerprinter, FingerprintCompressor};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

// ── Format classification ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Aiff,
    Mp3,
    M4aAac,
    M4aAlac,
    Wav,
    Flac,
    OggVorbis,
    Opus,
    WavPack,
    Aac,
    Ape,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportAction {
    Keep,
    ToAiff,
    ToM4a,
    Unsupported,
}

pub fn classify(path: &Path) -> AudioFormat {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("aif" | "aiff") => AudioFormat::Aiff,
        Some("mp3") => AudioFormat::Mp3,
        Some("m4a") => AudioFormat::M4aAac, // ALAC detection requires codec probing
        Some("wav") => AudioFormat::Wav,
        Some("flac") => AudioFormat::Flac,
        Some("ogg" | "oga") => AudioFormat::OggVorbis,
        Some("opus") => AudioFormat::Opus,
        Some("wv") => AudioFormat::WavPack,
        Some("aac") => AudioFormat::Aac,
        Some("ape") => AudioFormat::Ape,
        _ => AudioFormat::Unknown,
    }
}

pub fn import_action(fmt: AudioFormat) -> ImportAction {
    match fmt {
        AudioFormat::Aiff | AudioFormat::Mp3 | AudioFormat::M4aAac => ImportAction::Keep,
        AudioFormat::Wav | AudioFormat::Flac | AudioFormat::M4aAlac | AudioFormat::WavPack => {
            ImportAction::ToAiff
        }
        AudioFormat::OggVorbis | AudioFormat::Opus | AudioFormat::Aac => ImportAction::ToM4a,
        AudioFormat::Ape => ImportAction::ToAiff,
        AudioFormat::Unknown => ImportAction::Unsupported,
    }
}

// ── Chromaprint fingerprinting ─────────────────────────────────────────────

/// Generate a chromaprint fingerprint string for an audio file.
/// Uses the standard AcoustID preset (test2). Returns the compressed
/// base64 fingerprint compatible with the AcoustID lookup API.
pub fn fingerprint(path: &Path) -> Result<String, String> {
    let (samples, sample_rate, channels) = decode_to_i16(path)?;

    let config = Configuration::preset_test2();
    let mut printer = Fingerprinter::new(&config);
    printer.start(sample_rate, channels)
        .map_err(|e| format!("chromaprint start: {e:?}"))?;
    printer.consume(&samples);
    printer.finish();

    let raw = printer.fingerprint();
    let compressed = FingerprintCompressor::from(&config).compress(raw);
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        &compressed,
    ))
}

/// Decode an audio file to interleaved i16 samples via symphonia.
fn decode_to_i16(path: &Path) -> Result<(Vec<i16>, u32, u32), String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("probe: {e}"))?;

    let mut reader = probed.format;
    let track = reader.tracks().iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("no audio track")?;

    let track_id = track.id;
    let params = track.codec_params.clone();
    let sample_rate = params.sample_rate.ok_or("unknown sample rate")?;
    let channels = params.channels.map(|ch| ch.count() as u32).unwrap_or(2);

    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| format!("decoder: {e}"))?;

    let mut all_samples = Vec::new();
    let mut buf: Option<SampleBuffer<i16>> = None;

    loop {
        let packet = match reader.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("read packet: {e}")),
        };
        if packet.track_id() != track_id { continue; }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(format!("decode: {e}")),
        };

        let spec = *decoded.spec();
        let frames = decoded.frames() as u64;
        let b = buf.get_or_insert_with(|| SampleBuffer::<i16>::new(frames, spec));
        if b.capacity() < frames as usize * channels as usize {
            *b = SampleBuffer::<i16>::new(frames, spec);
        }
        b.copy_interleaved_ref(decoded);
        all_samples.extend_from_slice(b.samples());
    }

    Ok((all_samples, sample_rate, channels))
}

// ── Lossless transcode (symphonia → AIFF) ──────────────────────────────────

/// Result of a verified conversion.
pub struct ConvertResult {
    pub dst_path: PathBuf,
    pub fingerprint: String,
}

/// Convert a lossless audio file to AIFF with chromaprint verification.
///
/// Pipeline:
///   1. Fingerprint source file
///   2. Decode via symphonia, write AIFF via aifc (preserving bit depth)
///   3. Fingerprint output file
///   4. If fingerprints differ → delete output, return error
///
/// Returns the output path and verified fingerprint on success.
pub fn convert_to_aiff(src: &Path, dst_dir: &Path) -> Result<ConvertResult, String> {
    let src_fp = fingerprint(src)?;
    let dst_path = to_aiff(src, dst_dir)?;

    let dst_fp = match fingerprint(&dst_path) {
        Ok(fp) => fp,
        Err(e) => {
            let _ = fs::remove_file(&dst_path);
            return Err(format!("fingerprint dst failed: {e}"));
        }
    };

    if src_fp != dst_fp {
        let _ = fs::remove_file(&dst_path);
        return Err("chromaprint mismatch — conversion is not lossless".to_string());
    }

    Ok(ConvertResult {
        dst_path,
        fingerprint: src_fp,
    })
}

/// Decode a lossless audio file via symphonia and write it as AIFF.
/// Preserves source bit depth: 16-bit → 16-bit, 24-bit → 24-bit.
fn to_aiff(src: &Path, dst_dir: &Path) -> Result<PathBuf, String> {
    let file = File::open(src).map_err(|e| format!("open {}: {e}", src.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = src.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("probe {}: {e}", src.display()))?;

    let mut reader = probed.format;
    let track = reader.tracks().iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("no audio track found")?;

    let track_id = track.id;
    let params = track.codec_params.clone();
    let sample_rate = params.sample_rate.ok_or("unknown sample rate")?;
    let channels = params.channels.map(|ch| ch.count() as i16).unwrap_or(2);
    let bits = params.bits_per_sample.unwrap_or(16);

    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| format!("create decoder: {e}"))?;

    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("track");
    let dst_path = dst_dir.join(format!("{stem}.aif"));

    let out_file =
        BufWriter::new(File::create(&dst_path).map_err(|e| format!("create {}: {e}", dst_path.display()))?);

    let (aiff_fmt, bit_depth) = match bits {
        ..=16 => (aifc::SampleFormat::I16, BitDepth::I16),
        17..=24 => (aifc::SampleFormat::I24, BitDepth::I24),
        _ => (aifc::SampleFormat::I32, BitDepth::I32),
    };

    let mut writer = aifc::AifcWriter::new(out_file, &aifc::AifcWriteInfo {
        file_format: aifc::FileFormat::Aiff,
        channels,
        sample_rate: sample_rate as f64,
        sample_format: aiff_fmt,
    }).map_err(|e| format!("aifc writer: {e:?}"))?;

    let mut buf_i16: Option<SampleBuffer<i16>> = None;
    let mut buf_i32: Option<SampleBuffer<i32>> = None;

    loop {
        let packet = match reader.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("read packet: {e}")),
        };
        if packet.track_id() != track_id { continue; }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(format!("decode: {e}")),
        };

        let spec = *decoded.spec();
        let frames = decoded.frames() as u64;

        match bit_depth {
            BitDepth::I16 => {
                let buf = buf_i16.get_or_insert_with(|| SampleBuffer::<i16>::new(frames, spec));
                if buf.capacity() < frames as usize * channels as usize {
                    *buf = SampleBuffer::<i16>::new(frames, spec);
                }
                buf.copy_interleaved_ref(decoded);
                writer.write_samples_i16(buf.samples())
                    .map_err(|e| format!("write i16: {e:?}"))?;
            }
            BitDepth::I24 => {
                let buf = buf_i32.get_or_insert_with(|| SampleBuffer::<i32>::new(frames, spec));
                if buf.capacity() < frames as usize * channels as usize {
                    *buf = SampleBuffer::<i32>::new(frames, spec);
                }
                buf.copy_interleaved_ref(decoded);
                // SampleBuffer<i32> left-shifts i24 by 8 to fill i32 range.
                // aifc write_samples_i24 uses the lower 24 bits, so shift back.
                let shifted: Vec<i32> = buf.samples().iter().map(|s| s >> 8).collect();
                writer.write_samples_i24(&shifted)
                    .map_err(|e| format!("write i24: {e:?}"))?;
            }
            BitDepth::I32 => {
                let buf = buf_i32.get_or_insert_with(|| SampleBuffer::<i32>::new(frames, spec));
                if buf.capacity() < frames as usize * channels as usize {
                    *buf = SampleBuffer::<i32>::new(frames, spec);
                }
                buf.copy_interleaved_ref(decoded);
                writer.write_samples_i32(buf.samples())
                    .map_err(|e| format!("write i32: {e:?}"))?;
            }
        }
    }

    writer.finalize().map_err(|e| format!("finalize aiff: {e:?}"))?;
    Ok(dst_path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BitDepth {
    I16,
    I24,
    I32,
}
