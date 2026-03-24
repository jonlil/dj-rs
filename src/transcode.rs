use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use rusty_chromaprint::{Configuration, Fingerprinter, FingerprintCompressor};
use symphonia::core::audio::{AudioBufferRef, SampleBuffer, SignalSpec};
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

// ── Symphonia decoder wrapper ───────────────────────────────────────────────

struct AudioReader {
    reader: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub bits_per_sample: u32,
}

impl AudioReader {
    fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| format!("probe {}: {e}", path.display()))?;

        let reader = probed.format;
        let track = reader.tracks().iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or("no audio track found")?;

        let track_id = track.id;
        let params = track.codec_params.clone();
        let sample_rate = params.sample_rate.ok_or("unknown sample rate")?;
        let channels = params.channels.map(|ch| ch.count() as u32).unwrap_or(2);
        let bits_per_sample = params.bits_per_sample.unwrap_or(16);

        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|e| format!("create decoder: {e}"))?;

        Ok(Self { reader, decoder, track_id, sample_rate, channels, bits_per_sample })
    }

    fn decode_packets(&mut self, mut on_decoded: impl FnMut(AudioBufferRef<'_>) -> Result<(), String>) -> Result<(), String> {
        loop {
            let packet = match self.reader.next_packet() {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(e) => return Err(format!("read packet: {e}")),
            };
            if packet.track_id() != self.track_id { continue; }

            match self.decoder.decode(&packet) {
                Ok(decoded) => on_decoded(decoded)?,
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(e) => return Err(format!("decode: {e}")),
            }
        }
    }
}

// ── Reusable sample buffer ──────────────────────────────────────────────────

fn ensure_buf<S: symphonia::core::sample::Sample>(
    buf: &mut Option<SampleBuffer<S>>,
    frames: u64,
    spec: SignalSpec,
) -> &mut SampleBuffer<S> {
    let channels = spec.channels.count();
    let needed = frames as usize * channels;
    if !matches!(buf, Some(ref b) if b.capacity() >= needed) {
        *buf = Some(SampleBuffer::new(frames, spec));
    }
    buf.as_mut().unwrap()
}

// ── Chromaprint fingerprinting ─────────────────────────────────────────────

pub fn fingerprint(path: &Path) -> Result<String, String> {
    let mut reader = AudioReader::open(path)?;
    let (sample_rate, channels) = (reader.sample_rate, reader.channels);

    let config = Configuration::preset_test2();
    let mut printer = Fingerprinter::new(&config);
    printer.start(sample_rate, channels)
        .map_err(|e| format!("chromaprint start: {e:?}"))?;

    let mut buf: Option<SampleBuffer<i16>> = None;
    reader.decode_packets(|decoded| {
        let b = ensure_buf(&mut buf, decoded.frames() as u64, *decoded.spec());
        b.copy_interleaved_ref(decoded);
        printer.consume(b.samples());
        Ok(())
    })?;

    printer.finish();
    let raw = printer.fingerprint();
    let compressed = FingerprintCompressor::from(&config).compress(raw);
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        &compressed,
    ))
}

// ── Lossless transcode (symphonia → AIFF) ──────────────────────────────────

pub struct ConvertResult {
    pub dst_path: PathBuf,
    pub fingerprint: String,
}

/// Convert a lossless audio file to AIFF with chromaprint verification.
///
/// Pipeline: fingerprint(src) → decode+write AIFF → fingerprint(dst) → compare.
/// On mismatch the output is deleted and an error returned.
pub fn convert_to_aiff(src: &Path, dst_dir: &Path) -> Result<ConvertResult, String> {
    let src_fp = fingerprint(src)?;
    let dst_path = to_aiff(src, dst_dir)?;

    let dst_fp = fingerprint(&dst_path).map_err(|e| {
        let _ = fs::remove_file(&dst_path);
        format!("fingerprint dst failed: {e}")
    })?;

    if src_fp != dst_fp {
        let _ = fs::remove_file(&dst_path);
        return Err("chromaprint mismatch — conversion is not lossless".to_string());
    }

    Ok(ConvertResult { dst_path, fingerprint: src_fp })
}

/// Decode via symphonia, write AIFF via aifc. Preserves source bit depth.
fn to_aiff(src: &Path, dst_dir: &Path) -> Result<PathBuf, String> {
    let mut reader = AudioReader::open(src)?;

    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("track");
    let dst_path = dst_dir.join(format!("{stem}.aif"));
    let out_file = BufWriter::new(
        File::create(&dst_path).map_err(|e| format!("create {}: {e}", dst_path.display()))?
    );

    let (aiff_fmt, use_i16) = match reader.bits_per_sample {
        ..=16 => (aifc::SampleFormat::I16, true),
        17..=24 => (aifc::SampleFormat::I24, false),
        _ => (aifc::SampleFormat::I32, false),
    };

    let mut writer = aifc::AifcWriter::new(out_file, &aifc::AifcWriteInfo {
        file_format: aifc::FileFormat::Aiff,
        channels: reader.channels as i16,
        sample_rate: reader.sample_rate as f64,
        sample_format: aiff_fmt,
    }).map_err(|e| format!("aifc writer: {e:?}"))?;

    let bits = reader.bits_per_sample;
    let mut buf_i16: Option<SampleBuffer<i16>> = None;
    let mut buf_i32: Option<SampleBuffer<i32>> = None;

    reader.decode_packets(|decoded| {
        let frames = decoded.frames() as u64;
        let spec = *decoded.spec();

        if use_i16 {
            let b = ensure_buf(&mut buf_i16, frames, spec);
            b.copy_interleaved_ref(decoded);
            writer.write_samples_i16(b.samples()).map_err(|e| format!("write: {e:?}"))
        } else {
            let b = ensure_buf(&mut buf_i32, frames, spec);
            b.copy_interleaved_ref(decoded);
            let samples = b.samples();
            if bits <= 24 {
                // SampleBuffer<i32> left-shifts i24 by 8 to fill i32 range.
                // aifc write_samples_i24 uses the lower 24 bits, so shift back.
                let shifted: Vec<i32> = samples.iter().map(|s| s >> 8).collect();
                writer.write_samples_i24(&shifted).map_err(|e| format!("write: {e:?}"))
            } else {
                writer.write_samples_i32(samples).map_err(|e| format!("write: {e:?}"))
            }
        }
    })?;

    writer.finalize().map_err(|e| format!("finalize aiff: {e:?}"))?;
    Ok(dst_path)
}
