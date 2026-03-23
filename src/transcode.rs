use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Audio format detected from file extension / codec probing.
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
    Ape,
    Unknown,
}

/// The action to take for a given source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportAction {
    /// Already a target format — copy or link as-is.
    Keep,
    /// Lossless → AIFF conversion.
    ToAiff,
    /// Lossy → M4A/AAC conversion (not yet implemented).
    ToM4a,
    /// Format not supported.
    Unsupported,
}

pub fn classify(path: &Path) -> AudioFormat {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("aif" | "aiff") => AudioFormat::Aiff,
        Some("mp3") => AudioFormat::Mp3,
        Some("m4a") => AudioFormat::M4aAac, // ALAC detection requires probing
        Some("wav") => AudioFormat::Wav,
        Some("flac") => AudioFormat::Flac,
        Some("ogg" | "oga") => AudioFormat::OggVorbis,
        Some("opus") => AudioFormat::Opus,
        Some("wv") => AudioFormat::WavPack,
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
        AudioFormat::OggVorbis | AudioFormat::Opus => ImportAction::ToM4a,
        AudioFormat::Ape => ImportAction::ToAiff, // ape-decoder, future
        AudioFormat::Unknown => ImportAction::Unsupported,
    }
}

/// Decode a lossless audio file via symphonia and write it as AIFF.
///
/// Preserves source bit depth: 16-bit → 16-bit AIFF, 24-bit → 24-bit AIFF.
/// Returns the path of the newly created `.aif` file.
pub fn to_aiff(src: &Path, dst_dir: &Path) -> Result<PathBuf, String> {
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

    let track = reader
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("no audio track found")?;

    let track_id = track.id;
    let params = track.codec_params.clone();

    let sample_rate = params.sample_rate.ok_or("unknown sample rate")?;
    let channels = params
        .channels
        .map(|ch| ch.count() as i16)
        .unwrap_or(2);
    let bits = params.bits_per_sample.unwrap_or(16);

    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| format!("create decoder: {e}"))?;

    // Determine output path
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("track");
    let dst_path = dst_dir.join(format!("{stem}.aif"));

    let out_file =
        BufWriter::new(File::create(&dst_path).map_err(|e| format!("create {}: {e}", dst_path.display()))?);

    let (aiff_fmt, write_fn) = match bits {
        ..=16 => (aifc::SampleFormat::I16, WriteFn::I16),
        17..=24 => (aifc::SampleFormat::I24, WriteFn::I24),
        _ => (aifc::SampleFormat::I32, WriteFn::I32),
    };

    let info = aifc::AifcWriteInfo {
        file_format: aifc::FileFormat::Aiff,
        channels,
        sample_rate: sample_rate as f64,
        sample_format: aiff_fmt,
    };

    let mut writer = aifc::AifcWriter::new(out_file, &info)
        .map_err(|e| format!("aifc writer: {e:?}"))?;

    // Reusable sample buffers — created on first packet
    let mut buf_i16: Option<SampleBuffer<i16>> = None;
    let mut buf_i32: Option<SampleBuffer<i32>> = None;

    loop {
        let packet = match reader.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(e) => return Err(format!("read packet: {e}")),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(format!("decode: {e}")),
        };

        let spec = *decoded.spec();
        let frames = decoded.frames() as u64;

        match write_fn {
            WriteFn::I16 => {
                let buf = buf_i16.get_or_insert_with(|| SampleBuffer::<i16>::new(frames, spec));
                if buf.capacity() < frames as usize * channels as usize {
                    *buf = SampleBuffer::<i16>::new(frames, spec);
                }
                buf.copy_interleaved_ref(decoded);
                writer
                    .write_samples_i16(buf.samples())
                    .map_err(|e| format!("write i16: {e:?}"))?;
            }
            WriteFn::I24 | WriteFn::I32 => {
                let buf = buf_i32.get_or_insert_with(|| SampleBuffer::<i32>::new(frames, spec));
                if buf.capacity() < frames as usize * channels as usize {
                    *buf = SampleBuffer::<i32>::new(frames, spec);
                }
                buf.copy_interleaved_ref(decoded);
                if write_fn == WriteFn::I24 {
                    writer
                        .write_samples_i24(buf.samples())
                        .map_err(|e| format!("write i24: {e:?}"))?;
                } else {
                    writer
                        .write_samples_i32(buf.samples())
                        .map_err(|e| format!("write i32: {e:?}"))?;
                }
            }
        }
    }

    writer.finalize().map_err(|e| format!("finalize aiff: {e:?}"))?;

    Ok(dst_path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteFn {
    I16,
    I24,
    I32,
}
