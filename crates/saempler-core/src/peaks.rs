use saempler_audio::SampleBuffer;

/// Frames summarised by one peak at the finest level.
///
/// At 48 kHz this makes level 0 about 5 ms per peak, which is finer than any
/// pixel the waveform is ever drawn at, while costing 1/256th of the audio in
/// memory.
pub const BASE_FRAMES_PER_PEAK: u64 = 256;

/// Smallest number of peaks a level may have before the pyramid stops growing.
const MIN_LEVEL_LEN: usize = 2;

/// The minimum and maximum sample value over a range of frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    pub min: f32,
    pub max: f32,
}

impl Peak {
    pub const SILENT: Peak = Peak { min: 0.0, max: 0.0 };

    /// Combine two peaks into one covering both ranges.
    fn merged(self, other: Peak) -> Peak {
        Peak {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

/// A pyramid of pre-computed minimum/maximum pairs over the sample.
///
/// Drawing a waveform from raw samples costs time proportional to the length
/// of the audio. Reading it from the level that matches the current zoom costs
/// time proportional to the number of pixels instead.
///
/// Level 0 summarises [`BASE_FRAMES_PER_PEAK`] frames per peak; every further
/// level halves the resolution.
#[derive(Debug, Default)]
pub struct PeakCache {
    levels: Vec<Vec<Peak>>,
    frames: u64,
}

impl PeakCache {
    /// Build the pyramid. This walks the audio once per level and belongs on a
    /// background thread, never in the audio callback or a draw call.
    pub fn build(buffer: &SampleBuffer) -> Self {
        let frames = buffer.frames() as u64;
        if frames == 0 {
            return Self::default();
        }

        let base_len = frames.div_ceil(BASE_FRAMES_PER_PEAK) as usize;
        let mut base = Vec::with_capacity(base_len);
        for index in 0..base_len {
            let start = index as u64 * BASE_FRAMES_PER_PEAK;
            let end = (start + BASE_FRAMES_PER_PEAK).min(frames);
            base.push(peak_of(buffer, start, end));
        }

        let mut levels = vec![base];
        while levels
            .last()
            .map(|level| level.len() > MIN_LEVEL_LEN)
            .unwrap_or(false)
        {
            let previous = levels.last().expect("the loop condition read this level");
            let mut next = Vec::with_capacity(previous.len().div_ceil(2));
            for pair in previous.chunks(2) {
                let merged = match pair {
                    [single] => *single,
                    [first, second] => first.merged(*second),
                    _ => unreachable!("chunks(2) yields one or two elements"),
                };
                next.push(merged);
            }
            levels.push(next);
        }

        Self { levels, frames }
    }

    /// Number of frames the cache was built from.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Whether the cache holds any data.
    pub fn is_empty(&self) -> bool {
        self.frames == 0 || self.levels.is_empty()
    }

    /// Number of levels in the pyramid.
    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    /// Frames covered by one peak at `level`.
    pub fn frames_per_peak(&self, level: usize) -> u64 {
        BASE_FRAMES_PER_PEAK << level
    }

    /// Choose the coarsest level that still has at least one peak per pixel.
    ///
    /// Reading a coarser level than that would visibly lose detail; reading a
    /// finer one would do work the display cannot show.
    pub fn level_for(&self, visible_frames: u64, pixels: f32) -> usize {
        if self.levels.is_empty() || pixels <= 0.0 || visible_frames == 0 {
            return 0;
        }

        let frames_per_pixel = (visible_frames as f64 / pixels as f64).max(1.0);
        let mut level = 0;
        while level + 1 < self.levels.len()
            && (self.frames_per_peak(level + 1) as f64) <= frames_per_pixel
        {
            level += 1;
        }
        level
    }

    /// The peak covering `start_frame..end_frame`, read from `level`.
    ///
    /// Ranges outside the sample read as silence, so a widget can ask for any
    /// range without clamping first.
    pub fn peak_in(&self, level: usize, start_frame: u64, end_frame: u64) -> Peak {
        let Some(peaks) = self.levels.get(level) else {
            return Peak::SILENT;
        };
        if peaks.is_empty() || end_frame <= start_frame {
            return Peak::SILENT;
        }

        let per_peak = self.frames_per_peak(level);
        let first = (start_frame / per_peak) as usize;
        // Inclusive index of the last peak that overlaps the range.
        let last = ((end_frame - 1) / per_peak) as usize;
        if first >= peaks.len() {
            return Peak::SILENT;
        }
        let last = last.min(peaks.len() - 1);

        peaks[first..=last]
            .iter()
            .copied()
            .fold(peaks[first], Peak::merged)
    }
}

/// Minimum and maximum across all channels over `start..end`.
fn peak_of(buffer: &SampleBuffer, start: u64, end: u64) -> Peak {
    let mut peak = Peak {
        min: f32::MAX,
        max: f32::MIN,
    };

    for channel in 0..buffer.channel_count() {
        let samples = buffer.channel(channel);
        let from = (start as usize).min(samples.len());
        let to = (end as usize).min(samples.len());
        for sample in &samples[from..to] {
            peak.min = peak.min.min(*sample);
            peak.max = peak.max.max(*sample);
        }
    }

    if peak.min > peak.max {
        Peak::SILENT
    } else {
        peak
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(frames: usize) -> SampleBuffer {
        let data: Vec<f32> = (0..frames)
            .map(|index| index as f32 / frames as f32)
            .collect();
        SampleBuffer::new(vec![data], 48_000)
    }

    fn square(frames: usize) -> SampleBuffer {
        let data: Vec<f32> = (0..frames)
            .map(|index| if index % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        SampleBuffer::new(vec![data], 48_000)
    }

    #[test]
    fn an_empty_buffer_produces_an_empty_cache() {
        let cache = PeakCache::build(&SampleBuffer::new(Vec::new(), 48_000));

        assert!(cache.is_empty());
        assert_eq!(cache.frames(), 0);
        assert_eq!(cache.peak_in(0, 0, 100), Peak::SILENT);
    }

    #[test]
    fn silence_stays_silent_at_every_level() {
        let buffer = SampleBuffer::new(vec![vec![0.0; 100_000]], 48_000);
        let cache = PeakCache::build(&buffer);

        for level in 0..cache.level_count() {
            assert_eq!(cache.peak_in(level, 0, 100_000), Peak::SILENT);
        }
    }

    #[test]
    fn the_pyramid_halves_resolution_per_level() {
        let cache = PeakCache::build(&ramp(100_000));

        assert!(cache.level_count() > 1);
        for level in 0..cache.level_count() {
            assert_eq!(cache.frames_per_peak(level), BASE_FRAMES_PER_PEAK << level);
        }
    }

    #[test]
    fn coarser_levels_never_lose_extremes() {
        let buffer = square(100_000);
        let cache = PeakCache::build(&buffer);

        for level in 0..cache.level_count() {
            let peak = cache.peak_in(level, 0, 100_000);
            assert_eq!(peak.max, 1.0, "level {level} lost the maximum");
            assert_eq!(peak.min, -1.0, "level {level} lost the minimum");
        }
    }

    #[test]
    fn a_coarse_level_covers_at_least_what_a_fine_level_covers() {
        let cache = PeakCache::build(&ramp(200_000));
        let range = 50_000..150_000;

        let fine = cache.peak_in(0, range.start, range.end);
        for level in 1..cache.level_count() {
            let coarse = cache.peak_in(level, range.start, range.end);
            assert!(
                coarse.min <= fine.min + 1e-6 && coarse.max >= fine.max - 1e-6,
                "level {level} must not report less than level 0"
            );
        }
    }

    #[test]
    fn a_marked_region_is_found_at_its_position() {
        let mut data = vec![0.0f32; 100_000];
        data[60_000] = 1.0;
        let cache = PeakCache::build(&SampleBuffer::new(vec![data], 48_000));

        assert_eq!(cache.peak_in(0, 59_000, 61_000).max, 1.0);
        assert_eq!(cache.peak_in(0, 0, 50_000).max, 0.0);
        assert_eq!(cache.peak_in(0, 70_000, 100_000).max, 0.0);
    }

    #[test]
    fn ranges_outside_the_sample_read_as_silence() {
        let cache = PeakCache::build(&ramp(10_000));

        assert_eq!(cache.peak_in(0, 1_000_000, 2_000_000), Peak::SILENT);
        assert_eq!(cache.peak_in(0, 100, 100), Peak::SILENT);
        assert_eq!(cache.peak_in(999, 0, 10_000), Peak::SILENT);
    }

    #[test]
    fn zooming_out_selects_coarser_levels() {
        let cache = PeakCache::build(&ramp(5_000_000));

        let zoomed_in = cache.level_for(10_000, 800.0);
        let zoomed_out = cache.level_for(5_000_000, 800.0);

        assert_eq!(zoomed_in, 0, "a close zoom must use the finest level");
        assert!(
            zoomed_out > zoomed_in,
            "a wide view must use a coarser level"
        );
        assert!(zoomed_out < cache.level_count());
    }

    #[test]
    fn the_chosen_level_still_has_a_peak_per_pixel() {
        let cache = PeakCache::build(&ramp(5_000_000));

        for pixels in [100.0f32, 400.0, 1_600.0] {
            let level = cache.level_for(5_000_000, pixels);
            let frames_per_pixel = 5_000_000.0 / pixels as f64;
            assert!(
                cache.frames_per_peak(level) as f64 <= frames_per_pixel
                    || level + 1 == cache.level_count(),
                "level {level} is coarser than one peak per pixel at {pixels} px"
            );
        }
    }

    #[test]
    fn degenerate_view_parameters_fall_back_to_the_finest_level() {
        let cache = PeakCache::build(&ramp(100_000));

        assert_eq!(cache.level_for(0, 800.0), 0);
        assert_eq!(cache.level_for(100_000, 0.0), 0);
        assert_eq!(cache.level_for(100_000, -5.0), 0);
    }

    #[test]
    fn a_sample_shorter_than_one_peak_still_works() {
        let cache = PeakCache::build(&SampleBuffer::new(vec![vec![0.5; 10]], 48_000));

        assert!(!cache.is_empty());
        assert_eq!(cache.level_count(), 1);
        assert_eq!(cache.peak_in(0, 0, 10).max, 0.5);
    }
}
