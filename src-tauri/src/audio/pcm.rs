use super::resampler::SincResampler;

/// Mixdown, anti-aliased resampling and s16le encoding for one capture stream. Stateful
/// because the resampler inside is; a stream whose source format changes builds a new one.
pub struct PcmConverter {
    channels: usize,
    resampler: SincResampler,
}

impl PcmConverter {
    pub fn new(source_rate: u32, channels: usize, target_rate: u32) -> Result<Self, String> {
        if channels == 0 {
            return Err("Invalid channel count".into());
        }
        Ok(Self {
            channels,
            resampler: SincResampler::new(source_rate, target_rate)?,
        })
    }

    pub fn convert_f32(&mut self, interleaved: &[f32]) -> Vec<u8> {
        if interleaved.is_empty() {
            return Vec::new();
        }
        let mono = interleaved
            .chunks_exact(self.channels)
            .map(|frame| frame.iter().sum::<f32>() / self.channels as f32)
            .collect::<Vec<_>>();
        encode_s16le(&self.resampler.process(&mono))
    }

    pub fn convert_i16(&mut self, interleaved: &[i16]) -> Vec<u8> {
        if interleaved.is_empty() {
            return Vec::new();
        }
        let mono = interleaved
            .chunks_exact(self.channels)
            .map(|frame| {
                frame.iter().map(|sample| f32::from(*sample)).sum::<f32>()
                    / (self.channels as f32 * 32768.0)
            })
            .collect::<Vec<_>>();
        encode_s16le(&self.resampler.process(&mono))
    }
}

pub fn encode_s16le(input: &[f32]) -> Vec<u8> {
    input
        .iter()
        .flat_map(|sample| {
            let value = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
            value.to_le_bytes()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> Vec<i16> {
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&chunk| i16::from_le_bytes(chunk))
            .collect()
    }

    #[test]
    fn mixes_stereo_and_downsamples() {
        let mut converter = PcmConverter::new(48_000, 2, 16_000).unwrap();
        // One second of DC so the sinc filter reaches steady state.
        let input = (0..48_000).flat_map(|_| [0.5, 0.5]).collect::<Vec<_>>();
        let output = decode(&converter.convert_f32(&input));

        // 3:1 decimation, minus at most one buffered chunk.
        assert!(output.len() > 15_000 && output.len() <= 16_000);
        // DC passes the filter unchanged; check away from the warm-up edge.
        let middle = output[output.len() / 2];
        assert!((middle - 16383).abs() <= 50, "middle={middle}");
    }

    #[test]
    fn matching_rates_pass_through_untouched() {
        let mut converter = PcmConverter::new(16_000, 1, 16_000).unwrap();
        let output = decode(&converter.convert_f32(&[0.5; 160]));
        assert_eq!(output.len(), 160);
        assert_eq!(output[0], 16383);
    }

    /// The reason the resampler is a sinc filter at all: content above the target's
    /// Nyquist must be removed, not folded back into the audible band.
    #[test]
    fn downsampling_removes_content_above_the_target_band() {
        let mut converter = PcmConverter::new(48_000, 1, 16_000).unwrap();
        // A 12 kHz tone: inaudible at 16 kHz (Nyquist 8 kHz); a linear interpolator
        // would alias it to 4 kHz at nearly full amplitude.
        let input = (0..48_000)
            .map(|n| (2.0 * std::f32::consts::PI * 12_000.0 * n as f32 / 48_000.0).sin())
            .collect::<Vec<_>>();
        let output = decode(&converter.convert_f32(&input));

        let steady = &output[output.len() / 4..output.len() * 3 / 4];
        let rms = (steady
            .iter()
            .map(|sample| f64::from(*sample) * f64::from(*sample))
            .sum::<f64>()
            / steady.len() as f64)
            .sqrt();
        assert!(rms < 32767.0 * 0.02, "rms={rms}");
    }

    #[test]
    fn rejects_invalid_formats() {
        assert!(PcmConverter::new(48_000, 0, 16_000).is_err());
        assert!(PcmConverter::new(0, 1, 16_000).is_err());
        let mut converter = PcmConverter::new(48_000, 1, 16_000).unwrap();
        assert!(converter.convert_f32(&[]).is_empty());
        assert!(converter.convert_i16(&[]).is_empty());
    }
}
