//! Audio processing: noise cancellation and resampling.
//!
//! Pipeline: 48kHz raw → RNNoise → Rubato → 16kHz clean

use audioadapter_buffers::direct::SequentialSliceOfVecs;
use crossbeam_channel::{Receiver, Sender};
use nnnoiseless::DenoiseState;
use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};
use std::thread::{self, JoinHandle};

/// Input sample rate (required by RNNoise)
pub const CAPTURE_RATE: usize = 48000;

/// Output sample rate (required by Whisper)
pub const SAMPLE_RATE: u32 = 16000;

#[allow(dead_code)]
pub fn spawn(rx: Receiver<f32>, tx: Sender<f32>) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut first_sample = true;
        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            oversampling_factor: 256,
            interpolation: SincInterpolationType::Linear,
            window: WindowFunction::BlackmanHarris2,
        };

        let mut resampler = Async::<f32>::new_sinc(
            SAMPLE_RATE as f64 / CAPTURE_RATE as f64,
            2.0,
            &params,
            DenoiseState::FRAME_SIZE,
            1,
            FixedAsync::Input,
        )
        .expect("failed to create resampler");

        let mut denoise = DenoiseState::new();
        // let output_buffer :Vec<f32> = vec![0.0; DenoiseState::FRAME_SIZE];
        let mut output_buffer: Vec<f32> = vec![];
        output_buffer.resize(DenoiseState::FRAME_SIZE, 0.0);
        loop {
            let scaled_sample: Vec<f32> = rx
                .iter()
                .take(DenoiseState::FRAME_SIZE)
                .map(|s| s * i16::MAX as f32)
                .collect();

            // Test if we are done. This may drop the last samples, but thats ok.
            if scaled_sample.len() < DenoiseState::FRAME_SIZE {
                break;
            }

            denoise.process_frame(&mut output_buffer, &scaled_sample);

            // First sample to heat up the denoise. Throw away.
            if first_sample {
                first_sample = false;
                continue;
            }

            output_buffer
                .iter_mut()
                .for_each(|x| *x = *x / i16::MAX as f32);

            // Resample from 48kHz to 16kHz using high-quality sinc interpolation
            let input_vecs = vec![output_buffer.clone()];
            let input_adapter =
                SequentialSliceOfVecs::new(&input_vecs, 1, DenoiseState::FRAME_SIZE)
                    .expect("Unable to create slice of samples");
            let resampled = resampler
                .process(&input_adapter, 0, None)
                .expect("Unable to resample slice");
            resampled.take_data().iter().for_each(|s| {
                let _ = tx.send(*s);
            });
        }
    })
}

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
        )
        .expect("failed to create resampler");

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
            let frame: Vec<f32> = self
                .input_buffer
                .drain(..DenoiseState::FRAME_SIZE)
                .collect();

            // Convert from [-1, 1] to i16 range for RNNoise
            let scaled_input: Vec<f32> = frame.iter().map(|&s| s * i16::MAX as f32).collect();

            self.denoise
                .process_frame(&mut self.output_buffer, &scaled_input);

            // Skip first frame (contains artifacts from uninitialized state)
            if self.first_frame {
                self.first_frame = false;
                continue;
            }

            // Convert back to [-1, 1] range
            let denoised: Vec<f32> = self
                .output_buffer
                .iter()
                .map(|&s| s / i16::MAX as f32)
                .collect();

            // Resample from 48kHz to 16kHz using high-quality sinc interpolation
            let input_vecs = vec![denoised];
            let input_adapter =
                SequentialSliceOfVecs::new(&input_vecs, 1, DenoiseState::FRAME_SIZE).unwrap();
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
    use crossbeam_channel::RecvError;
    use crossbeam_channel::TryRecvError::Empty;

    #[test]
    fn test_process_empty() {
        let (audio_tx, audio_rx) = crossbeam_channel::unbounded::<f32>();
        let (processed_tx, processed_rx) = crossbeam_channel::unbounded::<f32>();
        spawn(audio_rx, processed_tx);
        assert_eq!(processed_rx.try_recv(), Err(Empty));
        drop(audio_tx);
        assert_eq!(processed_rx.recv(), Err(RecvError));
    }

    #[test]
    fn test_process_partial_frame() {
        // Less than one frame - should accumulate but not output
        let (audio_tx, audio_rx) = crossbeam_channel::unbounded::<f32>();
        let (processed_tx, processed_rx) = crossbeam_channel::unbounded::<f32>();
        spawn(audio_rx, processed_tx);

        let input = vec![0.0; DenoiseState::FRAME_SIZE - 1];
        input.iter().for_each(|x| {
            let _ = audio_tx.send(*x);
        });
        drop(audio_tx);
        assert!(processed_rx.is_empty());
    }

    #[test]
    fn test_process_full_frame() {
        // Two full frames - first is discarded (warmup), second produces output
        let (audio_tx, audio_rx) = crossbeam_channel::unbounded::<f32>();
        let (processed_tx, processed_rx) = crossbeam_channel::unbounded::<f32>();
        spawn(audio_rx, processed_tx);

        let input = vec![0.0; DenoiseState::FRAME_SIZE * 7];
        input.iter().for_each(|x| {
            let _ = audio_tx.send(*x);
        });
        drop(audio_tx);
        assert_eq!(
            processed_rx.iter().count() + 1,
            DenoiseState::FRAME_SIZE * 2
        );
    }
}
