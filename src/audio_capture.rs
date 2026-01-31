//! Audio capture using PipeWire.
//!
//! Captures raw 48kHz mono audio from PipeWire and sends samples to a channel.
//! Processing (noise cancellation, resampling) is handled separately by AudioProcessor.

use std::mem;
use std::thread;

use crossbeam_channel as channel;
use pipewire as pw;
use pw::spa::param::audio::{AudioFormat, AudioInfoRaw};
use pw::spa::pod::Pod;
use pw::spa::utils::Direction;
use pw::stream::StreamFlags;

use crate::audio_processor::CAPTURE_RATE;

pub type AudioSample = f32;
pub type AudioSender = channel::Sender<AudioSample>;
pub type AudioReceiver = channel::Receiver<AudioSample>;

pub type TextSender = channel::Sender<String>;
pub type TextReceiver = channel::Receiver<String>;

/// Control message for PipeWire thread
#[derive(Debug)]
enum ControlMessage {
    Stop,
}

/// Handle for audio capture.
/// Captures raw 48kHz audio and sends samples to the provided channel.

pub fn start() -> (channel::Receiver<f32>, impl FnOnce()) {
    let (audio_tx, audio_rx) = channel::unbounded::<f32>();
    pw::init();

    let (stop_sender, stop_receiver) = pw::channel::channel::<ControlMessage>();

    let thread = thread::spawn(move || {
        run_pipewire_capture(stop_receiver, audio_tx);
    });
    let stop_fn = move || {
        stop_sender
            .send(ControlMessage::Stop)
            .expect("pipewire thread died");
        thread.join().expect("pipewire thread panicked");
    };
    (audio_rx, stop_fn)
}

/// PipeWire capture thread - runs MainLoop, sends individual samples to raw_tx
fn run_pipewire_capture(
    stop_receiver: pw::channel::Receiver<ControlMessage>,
    raw_tx: channel::Sender<f32>,
) {
    let mainloop =
        pw::main_loop::MainLoopRc::new(None).expect("failed to create PipeWire MainLoop");
    let context =
        pw::context::ContextRc::new(&mainloop, None).expect("failed to create PipeWire context");
    let core = context
        .connect_rc(None)
        .expect("failed to connect to PipeWire");

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
    audio_info.set_rate(CAPTURE_RATE as u32);
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

#[cfg(test)]
mod tests {
    /// Requires audio hardware - run with: make test-hardware
    #[test]
    #[cfg(feature = "system")]
    fn test_capture_3_seconds() {
        use super::*;
        use crate::audio_processor::CAPTURE_RATE;
        use std::time::{Duration, Instant};

        let (capture_rx, stop_capture) = start();

        // Allow startup time
        thread::sleep(Duration::from_millis(100));

        let expected_samples = CAPTURE_RATE as u64 * 2; // 2 seconds worth at 48kHz
        let mut sample_count = 0u64;
        let start = Instant::now();
        let timeout = Duration::from_secs(4);

        while sample_count < expected_samples && start.elapsed() < timeout {
            match capture_rx.recv_timeout(Duration::from_millis(10)) {
                Ok(_) => sample_count += 1,
                Err(channel::RecvTimeoutError::Timeout) => continue,
                Err(channel::RecvTimeoutError::Disconnected) => break,
            }
        }
        stop_capture();

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
}
