//! Audio capture using PipeWire with noise cancellation.
//!
//! Pipeline: PipeWire (48kHz) → RNNoise → Resample (16kHz) → Output
//!
//! Architecture:
//! - PipeWire thread: runs MainLoop, callback copies samples to raw_tx
//! - Processor thread: blocks on raw_rx, applies RNNoise + Rubato, sends to audio_tx

use std::mem;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use audioadapter_buffers::direct::SequentialSliceOfVecs;
use nnnoiseless::DenoiseState;
use pipewire as pw;
use pw::spa::param::audio::{AudioFormat, AudioInfoRaw};
use pw::spa::pod::Pod;
use pw::spa::utils::Direction;
use pw::stream::StreamFlags;
use rubato::{Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType, WindowFunction};

pub type AudioSample = f32;
pub type AudioSender = mpsc::Sender<AudioSample>;
pub type AudioReceiver = mpsc::Receiver<AudioSample>;

/// Target output format: 16kHz mono f32
pub const SAMPLE_RATE: u32 = 16000;

/// Capture sample rate for RNNoise (must be 48kHz)
const CAPTURE_RATE: u32 = 48000;

/// Control message for PipeWire thread
#[derive(Debug)]
enum ControlMessage {
    Stop,
}

/// Handle for audio capture.
pub struct AudioCapture {
    stop_sender: pw::channel::Sender<ControlMessage>,
    pipewire_handle: Option<JoinHandle<()>>,
    processor_handle: Option<JoinHandle<()>>,
}

impl AudioCapture {
    /// Creates audio capture with noise cancellation.
    /// Sends 16kHz mono f32 samples to the provided sender.
    pub fn new(audio_tx: AudioSender) -> Self {
        pw::init();

        let (stop_sender, stop_receiver) = pw::channel::channel::<ControlMessage>();
        let (raw_tx, raw_rx) = mpsc::channel::<f32>();

        let pipewire_handle = thread::spawn(move || {
            run_pipewire_capture(stop_receiver, raw_tx);
        });

        let processor_handle = thread::spawn(move || {
            run_processor(raw_rx, audio_tx);
        });

        Self {
            stop_sender,
            pipewire_handle: Some(pipewire_handle),
            processor_handle: Some(processor_handle),
        }
    }

    /// Explicitly stop audio capture and wait for threads to exit.
    /// Idempotent - safe to call multiple times.
    pub fn stop(&mut self) {
        if self.pipewire_handle.is_some() {
            self.stop_sender.send(ControlMessage::Stop).expect("pipewire thread died");
        }

        if let Some(handle) = self.pipewire_handle.take() {
            handle.join().expect("pipewire thread panicked");
        }
        if let Some(handle) = self.processor_handle.take() {
            handle.join().expect("processor thread panicked");
        }
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

/// PipeWire capture thread - runs MainLoop, sends individual samples to raw_tx
fn run_pipewire_capture(stop_receiver: pw::channel::Receiver<ControlMessage>, raw_tx: mpsc::Sender<f32>) {
    let mainloop = pw::main_loop::MainLoopRc::new(None).expect("failed to create PipeWire MainLoop");
    let context = pw::context::ContextRc::new(&mainloop, None).expect("failed to create PipeWire context");
    let core = context.connect_rc(None).expect("failed to connect to PipeWire");

    // Attach stop receiver to mainloop
    let _stop_listener = stop_receiver.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |_msg| {
            mainloop.quit();
        }
    });

    // Create stream with capture properties
    let props = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Communication",
    };

    let stream = pw::stream::StreamBox::new(&core, "whisperme-capture", props)
        .expect("failed to create PipeWire stream");

    // Set up process callback - send individual samples
    let _listener = stream
        .add_local_listener_with_user_data(raw_tx)
        .process(|stream, raw_tx| {
            if let Some(mut buffer) = stream.dequeue_buffer() {
                let data = buffer.datas_mut().first_mut().expect("no data plane");
                let n_samples = data.chunk().size() / (mem::size_of::<f32>() as u32);
                let bytes = data.data().expect("no buffer data");
                (0..n_samples as usize).for_each(|n| {
                    let start = n * mem::size_of::<f32>();
                    let end = start + mem::size_of::<f32>();
                    let sample = f32::from_le_bytes(bytes[start..end].try_into().unwrap());
                    raw_tx.send(sample).expect("processor thread died");
                });
            }
        })
        .register()
        .expect("failed to register stream listener");

    // Build audio format parameters - request 48kHz mono f32
    let mut audio_info = AudioInfoRaw::new();
    audio_info.set_format(AudioFormat::F32LE);
    audio_info.set_rate(CAPTURE_RATE);
    audio_info.set_channels(1);

    let obj = pw::spa::pod::Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pw::spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .expect("failed to serialize audio format")
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values).expect("failed to create Pod")];

    stream
        .connect(
            Direction::Input,
            None,
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
            &mut params,
        )
        .expect("failed to connect PipeWire stream");

    mainloop.run();
}

/// Processor thread - blocks on raw_rx, accumulates 10ms of samples, applies noise cancellation and resampling
fn run_processor(raw_rx: mpsc::Receiver<f32>, audio_tx: AudioSender) {
    let mut processor = AudioProcessor::new();
    let mut buffer = Vec::with_capacity(DenoiseState::FRAME_SIZE);

    while let Ok(sample) = raw_rx.recv() {
        buffer.push(sample);

        // Process when we have 10ms of audio (480 samples at 48kHz)
        if buffer.len() >= DenoiseState::FRAME_SIZE {
            let processed = processor.process(&buffer);
            buffer.clear();

            for sample in processed {
                audio_tx.send(sample).expect("transcription thread died");
            }
        }
    }
}

/// Noise cancellation and resampling processor
struct AudioProcessor {
    denoise: Box<DenoiseState<'static>>,
    resampler: Async<f32>,
    input_buffer: Vec<f32>,
    output_buffer: Vec<f32>,
    first_frame: bool,
}

impl AudioProcessor {
    fn new() -> Self {
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

    /// Process incoming audio samples (48kHz) and return denoised 16kHz samples
    fn process(&mut self, samples: &[f32]) -> Vec<f32> {
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
