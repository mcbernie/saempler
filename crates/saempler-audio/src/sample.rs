/// Decoded audio, ready for the audio thread.
///
/// Immutable once built. The engine only ever holds an `Arc` to it, so the
/// same audio can be referenced by any number of slices and voices without
/// being copied.
///
/// Channels are stored separately rather than interleaved because playback
/// reads one frame from every channel and never needs the interleaved layout.
#[derive(Debug)]
pub struct SampleBuffer {
    channels: Vec<Vec<f32>>,
    frames: usize,
    sample_rate: u32,
}

impl SampleBuffer {
    /// Build a buffer from per-channel data.
    ///
    /// Channels of differing length are truncated to the shortest one, so
    /// `frames` is valid for every channel and playback needs no bounds check
    /// per channel.
    pub fn new(mut channels: Vec<Vec<f32>>, sample_rate: u32) -> Self {
        let frames = channels.iter().map(Vec::len).min().unwrap_or(0);
        for channel in &mut channels {
            channel.truncate(frames);
        }

        Self {
            channels,
            frames,
            sample_rate,
        }
    }

    /// Number of frames in the buffer.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Number of channels. Zero for an empty buffer.
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Sample rate the audio was decoded at.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Whether the buffer carries any audio.
    pub fn is_empty(&self) -> bool {
        self.frames == 0 || self.channels.is_empty()
    }

    /// All samples of one channel.
    pub fn channel(&self, channel: usize) -> &[f32] {
        self.channels
            .get(channel)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// One frame, as a left/right pair.
    ///
    /// Mono buffers are read as centred, and buffers with more than two
    /// channels contribute their first two. Out-of-range frames read as
    /// silence so the audio thread never indexes out of bounds.
    pub fn frame(&self, frame: usize) -> (f32, f32) {
        if frame >= self.frames {
            return (0.0, 0.0);
        }

        match self.channels.len() {
            0 => (0.0, 0.0),
            1 => {
                let value = self.channels[0][frame];
                (value, value)
            }
            _ => (self.channels[0][frame], self.channels[1][frame]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer_reads_as_silence() {
        let buffer = SampleBuffer::new(Vec::new(), 48_000);

        assert!(buffer.is_empty());
        assert_eq!(buffer.frames(), 0);
        assert_eq!(buffer.frame(0), (0.0, 0.0));
    }

    #[test]
    fn mono_is_read_as_centred() {
        let buffer = SampleBuffer::new(vec![vec![0.5, -0.5]], 48_000);

        assert_eq!(buffer.channel_count(), 1);
        assert_eq!(buffer.frame(0), (0.5, 0.5));
        assert_eq!(buffer.frame(1), (-0.5, -0.5));
    }

    #[test]
    fn stereo_keeps_the_channels_apart() {
        let buffer = SampleBuffer::new(vec![vec![1.0, 0.0], vec![0.0, 1.0]], 44_100);

        assert_eq!(buffer.frame(0), (1.0, 0.0));
        assert_eq!(buffer.frame(1), (0.0, 1.0));
        assert_eq!(buffer.sample_rate(), 44_100);
    }

    #[test]
    fn reading_past_the_end_is_silent_rather_than_a_panic() {
        let buffer = SampleBuffer::new(vec![vec![1.0]], 48_000);

        assert_eq!(buffer.frame(1), (0.0, 0.0));
        assert_eq!(buffer.frame(usize::MAX), (0.0, 0.0));
    }

    #[test]
    fn ragged_channels_are_truncated_to_the_shortest() {
        let buffer = SampleBuffer::new(vec![vec![1.0, 1.0, 1.0], vec![1.0]], 48_000);

        assert_eq!(buffer.frames(), 1);
        assert_eq!(buffer.channel(0).len(), 1);
        assert_eq!(buffer.channel(1).len(), 1);
    }

    #[test]
    fn more_than_two_channels_fall_back_to_the_first_pair() {
        let buffer = SampleBuffer::new(vec![vec![1.0], vec![2.0], vec![3.0]], 48_000);

        assert_eq!(buffer.frame(0), (1.0, 2.0));
    }
}
