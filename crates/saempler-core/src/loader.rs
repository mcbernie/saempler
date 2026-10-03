use std::fmt;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use saempler_audio::SampleBuffer;
use saempler_model::SampleRef;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::peaks::PeakCache;

/// Upper bound on the audio a single import may produce, in frames.
///
/// Roughly 30 minutes at 48 kHz. The decoded buffer is held in memory for the
/// lifetime of the project, so dropping a long mix into the plugin has to fail
/// with a message rather than exhaust memory.
pub const MAX_FRAMES: u64 = 30 * 60 * 48_000;

/// A decoded sample together with everything derived from it.
///
/// Produced on a background thread. The buffer goes to the audio engine, the
/// peaks to the user interface, and the reference into the project state.
#[derive(Debug)]
pub struct LoadedSample {
    pub buffer: Arc<SampleBuffer>,
    pub peaks: PeakCache,
    pub source: SampleRef,
}

/// Why an import failed.
///
/// The variants carry text rather than the decoder's own error types, so that
/// nothing outside this crate has to know which decoder is in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The file could not be opened or read.
    Io(String),
    /// The container or codec is not supported by this build.
    Unsupported(String),
    /// The file was recognised but could not be decoded.
    Decode(String),
    /// The file decoded to no audio at all.
    Empty,
    /// The file is longer than [`MAX_FRAMES`].
    TooLong { frames: u64 },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(message) => write!(f, "Datei konnte nicht gelesen werden: {message}"),
            LoadError::Unsupported(message) => {
                write!(f, "Format wird nicht unterstützt: {message}")
            }
            LoadError::Decode(message) => {
                write!(f, "Datei konnte nicht dekodiert werden: {message}")
            }
            LoadError::Empty => write!(f, "Datei enthält keine Audiodaten"),
            LoadError::TooLong { frames } => write!(
                f,
                "Datei ist zu lang ({frames} Frames, Grenze {MAX_FRAMES})"
            ),
        }
    }
}

impl std::error::Error for LoadError {}

/// Decode an audio file into memory and build its peak cache.
///
/// This reads the whole file and must never be called from the audio thread or
/// from a draw call.
pub fn load_sample(path: &Path) -> Result<LoadedSample, LoadError> {
    let file = File::open(path).map_err(|error| LoadError::Io(error.to_string()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    // The extension is only a hint; the probe falls back to content sniffing.
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(extension);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| LoadError::Unsupported(error.to_string()))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| LoadError::Unsupported("keine Audiospur enthalten".to_owned()))?;
    let track_id = track.id;
    let Some(CodecParameters::Audio(codec_params)) = track.codec_params.as_ref() else {
        return Err(LoadError::Unsupported(
            "Audiospur ohne Codec-Parameter".to_owned(),
        ));
    };

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(codec_params, &Default::default())
        .map_err(|error| LoadError::Unsupported(error.to_string()))?;

    let mut channels: Vec<Vec<f32>> = Vec::new();
    let mut planes: Vec<Vec<f32>> = Vec::new();
    let mut sample_rate = codec_params.sample_rate.unwrap_or(0);
    let mut frames: u64 = 0;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(error) => return Err(LoadError::Decode(error.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A damaged packet in the middle of a file should not lose the
            // whole import; the decoder recovers on the next one.
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(error) => return Err(LoadError::Decode(error.to_string())),
        };

        if sample_rate == 0 {
            sample_rate = decoded.spec().rate();
        }

        decoded.copy_to_vecs_planar(&mut planes);
        if channels.len() < planes.len() {
            channels.resize(planes.len(), Vec::new());
        }
        for (target, plane) in channels.iter_mut().zip(planes.iter()) {
            target.extend_from_slice(plane);
        }

        frames += planes.first().map(Vec::len).unwrap_or(0) as u64;
        if frames > MAX_FRAMES {
            return Err(LoadError::TooLong { frames });
        }
    }

    let buffer = SampleBuffer::new(channels, sample_rate);
    if buffer.is_empty() {
        return Err(LoadError::Empty);
    }

    let peaks = PeakCache::build(&buffer);
    let source = SampleRef {
        path: path.to_path_buf(),
        frames: buffer.frames() as u64,
        sample_rate: buffer.sample_rate(),
        channels: buffer.channel_count() as u16,
    };

    Ok(LoadedSample {
        buffer: Arc::new(buffer),
        peaks,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Write a minimal 16-bit PCM WAV file and return its path.
    ///
    /// Generating the file here keeps the test self-contained: no binary
    /// fixture has to be committed, and the expected contents are visible.
    fn write_wav(path: &Path, channels: u16, sample_rate: u32, frames: &[Vec<i16>]) {
        let frame_count = frames.len() as u32;
        let block_align = channels * 2;
        let data_len = frame_count * u32::from(block_align);

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for frame in frames {
            for sample in frame {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
        }

        let mut file = File::create(path).expect("the temp directory must be writable");
        file.write_all(&bytes).expect("writing the fixture");
    }

    /// A unique path inside the system temp directory.
    fn temp_path(name: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("saempler-{name}-{unique}.wav"))
    }

    struct TempWav(std::path::PathBuf);

    impl Drop for TempWav {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn a_missing_file_reports_an_io_error() {
        let error = load_sample(Path::new("definitiv-nicht-vorhanden.wav"))
            .expect_err("a missing file must not load");

        assert!(matches!(error, LoadError::Io(_)));
    }

    #[test]
    fn a_file_that_is_not_audio_is_rejected() {
        let path = temp_path("garbage");
        std::fs::write(&path, b"this is not audio at all, not even close")
            .expect("the temp directory must be writable");
        let _cleanup = TempWav(path.clone());

        let error = load_sample(&path).expect_err("garbage must not load");

        assert!(
            matches!(error, LoadError::Unsupported(_) | LoadError::Decode(_)),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn a_stereo_wav_loads_with_its_channels_intact() {
        let path = temp_path("stereo");
        // Left runs to full scale, right to negative full scale.
        let frames: Vec<Vec<i16>> = (0..1_000)
            .map(|index| {
                let value = (index as f32 / 1_000.0 * i16::MAX as f32) as i16;
                vec![value, -value]
            })
            .collect();
        write_wav(&path, 2, 44_100, &frames);
        let _cleanup = TempWav(path.clone());

        let loaded = load_sample(&path).expect("a valid wav must load");

        assert_eq!(loaded.source.sample_rate, 44_100);
        assert_eq!(loaded.source.channels, 2);
        assert_eq!(loaded.source.frames, 1_000);
        assert_eq!(loaded.buffer.frames(), 1_000);

        let (left, right) = loaded.buffer.frame(999);
        assert!(left > 0.9, "left channel should approach full scale");
        assert!(right < -0.9, "right channel should be inverted");
    }

    #[test]
    fn a_mono_wav_loads_as_one_channel() {
        let path = temp_path("mono");
        let frames: Vec<Vec<i16>> = (0..500).map(|_| vec![i16::MAX / 2]).collect();
        write_wav(&path, 1, 48_000, &frames);
        let _cleanup = TempWav(path.clone());

        let loaded = load_sample(&path).expect("a valid wav must load");

        assert_eq!(loaded.source.channels, 1);
        assert_eq!(loaded.buffer.channel_count(), 1);
        let (left, right) = loaded.buffer.frame(0);
        assert_eq!(left, right, "mono must be read as centred");
    }

    #[test]
    fn the_peak_cache_matches_the_decoded_audio() {
        let path = temp_path("peaks");
        let mut frames: Vec<Vec<i16>> = (0..100_000).map(|_| vec![0i16]).collect();
        frames[70_000] = vec![i16::MAX];
        write_wav(&path, 1, 48_000, &frames);
        let _cleanup = TempWav(path.clone());

        let loaded = load_sample(&path).expect("a valid wav must load");

        assert_eq!(loaded.peaks.frames(), 100_000);
        assert!(loaded.peaks.peak_in(0, 69_000, 71_000).max > 0.9);
        assert_eq!(loaded.peaks.peak_in(0, 0, 60_000).max, 0.0);
    }

    #[test]
    fn a_wav_without_audio_data_is_rejected() {
        let path = temp_path("empty");
        write_wav(&path, 2, 48_000, &[]);
        let _cleanup = TempWav(path.clone());

        let error = load_sample(&path).expect_err("an empty file has nothing to play");

        assert_eq!(error, LoadError::Empty);
    }

    #[test]
    fn the_sample_reference_describes_the_file() {
        let path = temp_path("reference");
        let frames: Vec<Vec<i16>> = (0..48_000).map(|_| vec![0i16, 0]).collect();
        write_wav(&path, 2, 48_000, &frames);
        let _cleanup = TempWav(path.clone());

        let loaded = load_sample(&path).expect("a valid wav must load");

        assert_eq!(loaded.source.path, path);
        assert!((loaded.source.duration_seconds() - 1.0).abs() < 1e-3);
        assert!(loaded
            .source
            .display_name()
            .starts_with("saempler-reference-"));
    }
}
