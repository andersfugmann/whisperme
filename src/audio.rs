//! Audio capture using PulseAudio with noise cancellation.
//! 
//! Pipeline: PulseAudio (48kHz) → RNNoise → Resample (16kHz) → Output

use pulseaudio::protocol::{ChannelMap, ChannelPosition, RecordStreamParams, SampleFormat, SampleSpec};
use pulseaudio::protocol::stream::BufferAttr;
use pulseaudio::Client;
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use nnnoiseless::DenoiseState;

use crate::UnwrapOrExit;

pub type AudioSample = f32;
pub type AudioSender = mpsc::Sender<AudioSample>;
pub type AudioReceiver = mpsc::Receiver<AudioSample>;

/// Target output format: 16kHz mono f32
pub const SAMPLE_RATE: u32 = 16000;

/// Capture sample rate for RNNoise (must be 48kHz)
const CAPTURE_RATE: u32 = 48000;

/// Fragment size in milliseconds - how often PulseAudio delivers audio
const FRAGMENT_MS: u32 = 20;

/// Handle for audio capture.
pub struct AudioCapture {
    stop_flag: Arc<AtomicBool>,
    _handle: thread::JoinHandle<()>,
}

impl AudioCapture {
    /// Creates audio capture with noise cancellation.
    /// Sends 16kHz mono f32 samples to the provided sender.
    pub fn new(sample_tx: AudioSender) -> Self {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = stop_flag.clone();

        let handle = thread::spawn(move || {
            run_capture(sample_tx, stop_flag_clone);
        });

        Self {
            stop_flag,
            _handle: handle,
        }
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }
}

/// Noise cancellation and resampling processor
struct AudioProcessor {
    denoise: Box<DenoiseState<'static>>,
    input_buffer: Vec<f32>,
    output_buffer: Vec<f32>,
    resample_buffer: Vec<f32>,
    first_frame: bool,
}

impl AudioProcessor {
    fn new() -> Self {
        Self {
            denoise: DenoiseState::new(),
            input_buffer: Vec::with_capacity(DenoiseState::FRAME_SIZE * 2),
            output_buffer: vec![0.0; DenoiseState::FRAME_SIZE],
            resample_buffer: Vec::new(),
            first_frame: true,
        }
    }

    /// Process incoming audio samples (48kHz) and return denoised 16kHz samples
    fn process(&mut self, samples: &[f32]) -> Vec<f32> {
        // Add samples to input buffer
        self.input_buffer.extend_from_slice(samples);
        
        let mut output_16k = Vec::new();
        
        // Process complete frames
        while self.input_buffer.len() >= DenoiseState::FRAME_SIZE {
            // Extract frame
            let frame: Vec<f32> = self.input_buffer
                .drain(..DenoiseState::FRAME_SIZE)
                .collect();
            
            // Convert from [-1, 1] to i16 range for RNNoise
            let scaled_input: Vec<f32> = frame.iter()
                .map(|&s| s * i16::MAX as f32)
                .collect();
            
            // Apply noise cancellation
            self.denoise.process_frame(&mut self.output_buffer, &scaled_input);
            
            // Skip first frame (contains artifacts)
            if self.first_frame {
                self.first_frame = false;
                continue;
            }
            
            // Convert back to [-1, 1] range
            let denoised: Vec<f32> = self.output_buffer.iter()
                .map(|&s| s / i16::MAX as f32)
                .collect();
            
            // Add to resample buffer
            self.resample_buffer.extend_from_slice(&denoised);
        }
        
        // Simple 3:1 downsampling from 48kHz to 16kHz
        // (A proper resampler would be better, but this works for speech)
        while self.resample_buffer.len() >= 3 {
            let sum: f32 = self.resample_buffer.drain(..3).sum();
            output_16k.push(sum / 3.0);
        }
        
        output_16k
    }
}

fn run_capture(sample_tx: AudioSender, stop_flag: Arc<AtomicBool>) {
    // Create a minimal tokio runtime for the async PulseAudio client
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_exit("failed to create runtime for audio capture");

    rt.block_on(async move {
        let client_name = CString::new("whisperme").unwrap();
        let client = Client::from_env(&client_name)
            .unwrap_or_exit("failed to connect to PulseAudio - is it running?");

        let sample_spec = SampleSpec {
            format: SampleFormat::Float32Le,
            sample_rate: CAPTURE_RATE, // 48kHz for RNNoise
            channels: 1,
        };

        let channel_map = ChannelMap::new(vec![ChannelPosition::Mono]);

        // Calculate fragment size in bytes for low latency
        let samples_per_fragment = (CAPTURE_RATE * FRAGMENT_MS / 1000) as usize;
        let fragment_size = (samples_per_fragment * std::mem::size_of::<f32>()) as u32;

        let buffer_attr = BufferAttr {
            fragment_size,
            max_length: fragment_size * 4,
            ..Default::default()
        };

        let params = RecordStreamParams {
            sample_spec,
            channel_map,
            buffer_attr,
            ..Default::default()
        };

        // Create audio processor for noise cancellation
        let processor = std::sync::Mutex::new(AudioProcessor::new());

        // Use closure as RecordSink - receives raw bytes
        let sink = move |data: &[u8]| {
            // Convert bytes to f32 samples (little-endian)
            let samples: Vec<f32> = data.chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();
            
            // Process through noise cancellation and resampling
            let mut proc = processor.lock().unwrap();
            let output = proc.process(&samples);
            
            // Send processed samples
            for sample in output {
                if sample_tx.send(sample).is_err() {
                    return; // Receiver dropped
                }
            }
        };

        let _stream = client
            .create_record_stream(params, sink)
            .await
            .unwrap_or_exit("failed to create record stream");

        // Poll stop flag - check every 100ms
        while !stop_flag.load(Ordering::SeqCst) {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_capture_3_seconds() {
        let (tx, rx) = mpsc::channel();
        let _capture = AudioCapture::new(tx);

        // Allow startup time
        thread::sleep(Duration::from_millis(100));

        let expected_samples = SAMPLE_RATE as u64 * 2; // 2 seconds worth
        let mut sample_count = 0u64;
        let start = Instant::now();
        let timeout = Duration::from_secs(4);

        while sample_count < expected_samples && start.elapsed() < timeout {
            match rx.recv_timeout(Duration::from_millis(10)) {
                Ok(_) => sample_count += 1,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        println!(
            "Captured {} samples in {:?} (expected ~{})",
            sample_count,
            start.elapsed(),
            expected_samples
        );
        // Allow 20% margin for timing variations
        assert!(
            sample_count >= expected_samples * 80 / 100,
            "Too few samples: {}",
            sample_count
        );
    }
    
    #[test]
    fn test_denoise_frame_size() {
        // RNNoise frame size is 480 samples at 48kHz = 10ms
        assert_eq!(DenoiseState::FRAME_SIZE, 480);
    }
}
