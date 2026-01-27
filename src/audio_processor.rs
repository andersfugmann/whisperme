//! Audio processing: noise cancellation and resampling.
//!
//! Pipeline: 48kHz raw → RNNoise → Rubato → 16kHz clean

use audioadapter_buffers::direct::SequentialSliceOfVecs;
use nnnoiseless::DenoiseState;
use rubato::{Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType, WindowFunction};

/// Input sample rate (required by RNNoise)
pub const CAPTURE_RATE: u32 = 48000;

/// Output sample rate (required by Whisper)
pub const SAMPLE_RATE: u32 = 16000;

/// Noise cancellation and resampling processor.
/// Takes 48kHz audio, applies RNNoise denoising, resamples to 16kHz.
pub struct AudioProcessor {
    denoise: Box<DenoiseState<'static>>,
    resampler: Async<f32>,
    input_buffer: Vec<f32>,
    output_buffer: Vec<f32>,
    first_frame: bool,
}

impl AudioProcessor {
    /// Create processor for 48kHz → 16kHz conversion with noise cancellation.
    pub fn new() -> Self {
        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            oversampling_factor: 256,
            interpolation: SincInterpolationType::Linear,
            window: WindowFunction::BlackmanHarris2,
        };

        let resampler = Async::<f32>::new_sinc(
            SAMPLE_RATE as f64 / CAPTURE_RATE as f64,
            2.0,
            &params,
            DenoiseState::FRAME_SIZE,
            1,
            FixedAsync::Input,
        ).expect("failed to create resampler");

        Self {
            denoise: DenoiseState::new(),
            resampler,
            input_buffer: Vec::with_capacity(DenoiseState::FRAME_SIZE * 2),
            output_buffer: vec![0.0; DenoiseState::FRAME_SIZE],
            first_frame: true,
        }
    }

    /// Process incoming audio samples (48kHz) and return denoised 16kHz samples.
    /// Accumulates samples internally until a 10ms frame (480 samples) is ready.
    pub fn process(&mut self, samples: &[f32]) -> Vec<f32> {
        self.input_buffer.extend_from_slice(samples);

        let mut output_16k = Vec::new();

        while self.input_buffer.len() >= DenoiseState::FRAME_SIZE {
            let frame: Vec<f32> = self.input_buffer
                .drain(..DenoiseState::FRAME_SIZE)
                .collect();

            // Convert from [-1, 1] to i16 range for RNNoise
            let scaled_input: Vec<f32> = frame.iter()
                .map(|&s| s * i16::MAX as f32)
                .collect();

            self.denoise.process_frame(&mut self.output_buffer, &scaled_input);

            // Skip first frame (contains artifacts from uninitialized state)
            if self.first_frame {
                self.first_frame = false;
                continue;
            }

            // Convert back to [-1, 1] range
            let denoised: Vec<f32> = self.output_buffer.iter()
                .map(|&s| s / i16::MAX as f32)
                .collect();

            // Resample from 48kHz to 16kHz using high-quality sinc interpolation
            let input_vecs = vec![denoised];
            let input_adapter = SequentialSliceOfVecs::new(&input_vecs, 1, DenoiseState::FRAME_SIZE).unwrap();
            match self.resampler.process(&input_adapter, 0, None) {
                Ok(resampled) => {
                    output_16k.extend(resampled.take_data());
                }
                Err(e) => {
                    eprintln!("Resampling error: {}", e);
                }
            }
        }
        output_16k
    }
}

impl Default for AudioProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_size() {
        // RNNoise frame size is 480 samples at 48kHz = 10ms
        assert_eq!(DenoiseState::FRAME_SIZE, 480);
    }

    #[test]
    fn test_sample_rates() {
        assert_eq!(CAPTURE_RATE, 48000);
        assert_eq!(SAMPLE_RATE, 16000);
    }

    #[test]
    fn test_process_empty() {
        let mut processor = AudioProcessor::new();
        let output = processor.process(&[]);
        assert!(output.is_empty());
    }

    #[test]
    fn test_process_partial_frame() {
        let mut processor = AudioProcessor::new();
        // Less than one frame - should accumulate but not output
        let input = vec![0.0; 100];
        let output = processor.process(&input);
        assert!(output.is_empty());
    }

    #[test]
    fn test_process_full_frame() {
        let mut processor = AudioProcessor::new();
        // Two full frames - first is discarded (warmup), second produces output
        let input = vec![0.0; DenoiseState::FRAME_SIZE * 2];
        let output = processor.process(&input);
        // 480 samples at 48kHz → 160 samples at 16kHz (3:1 ratio)
        assert!(!output.is_empty());
        assert!(output.len() >= 150 && output.len() <= 170); // Allow some variance
    }
}
