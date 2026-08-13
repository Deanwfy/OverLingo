use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

const CHUNK_SIZE: usize = 1024;

/// Windowed-sinc resampling for one mono stream. Stateful — the filter keeps history
/// across calls — so each capture stream owns one instance. When downsampling, everything
/// above the target's Nyquist is removed before decimation instead of aliasing back in.
pub struct SincResampler {
    /// `None` when the rates already match; audio passes through untouched.
    inner: Option<SincFixedIn<f32>>,
    input_buf: Vec<f32>,
}

impl SincResampler {
    pub fn new(source_rate: u32, target_rate: u32) -> Result<Self, String> {
        if source_rate == 0 || target_rate == 0 {
            return Err("Invalid sample rate".into());
        }
        if source_rate == target_rate {
            return Ok(Self {
                inner: None,
                input_buf: Vec::new(),
            });
        }
        let params = SincInterpolationParameters {
            sinc_len: 128,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 128,
            window: WindowFunction::BlackmanHarris2,
        };
        let resampler = SincFixedIn::<f32>::new(
            f64::from(target_rate) / f64::from(source_rate),
            2.0,
            params,
            CHUNK_SIZE,
            1,
        )
        .map_err(|e| format!("resampler init failed: {e}"))?;

        Ok(Self {
            inner: Some(resampler),
            input_buf: Vec::with_capacity(CHUNK_SIZE * 2),
        })
    }

    /// Up to one chunk stays buffered; continuous capture streams never notice.
    pub fn process(&mut self, mono: &[f32]) -> Vec<f32> {
        let Some(resampler) = self.inner.as_mut() else {
            return mono.to_vec();
        };
        self.input_buf.extend_from_slice(mono);

        let mut output = Vec::new();
        while self.input_buf.len() >= CHUNK_SIZE {
            let frame: Vec<f32> = self.input_buf.drain(..CHUNK_SIZE).collect();
            match resampler.process(&[frame], None) {
                Ok(mut chunk) => output.append(&mut chunk[0]),
                Err(error) => {
                    crate::diagnostics::log("audio", format!("resample failed: {error}"));
                    return output;
                }
            }
        }
        output
    }
}

/// OpenAI expects 24 kHz; the pipeline carries 16 kHz s16le, so provider sessions upsample
/// on the way in.
pub struct UpsamplerTo24k(SincResampler);

impl UpsamplerTo24k {
    pub fn new() -> Result<Self, String> {
        Ok(Self(SincResampler::new(super::TARGET_SAMPLE_RATE, 24_000)?))
    }

    pub fn push(&mut self, pcm_s16le: &[u8]) -> Result<Vec<u8>, String> {
        let mono = pcm_s16le
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&chunk| f32::from(i16::from_le_bytes(chunk)) / 32768.0)
            .collect::<Vec<_>>();
        Ok(super::pcm::encode_s16le(&self.0.process(&mono)))
    }
}
