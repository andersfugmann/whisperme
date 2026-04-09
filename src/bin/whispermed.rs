use std::thread;

use crossbeam_channel as channel;

use whisperme::audio_capture::{self, AudioCapture};
use whisperme::audio_processor;
use whisperme::config::{Config, OutputConfig, UiPosition};
use whisperme::fanout;
use whisperme::injection;
use whisperme::socket::{RecordingState, SocketEvent, SocketListener, SocketMessage};
use whisperme::transcription::Transcription;
use whisperme::recording_indicator;

fn main() {
    let config = Config::load();
    println!("WhisperMe Daemon starting...");
    println!("Config: {config:#?}");

    // Spawn transcription thread (loads model)
    let transcription = Transcription::new(&config.whisper, config.transcription);

    // Spawn persistent UI thread
    let ui_handle = recording_indicator::spawn();

    // Create socket event channel
    let (socket_tx, socket_rx) = channel::unbounded::<SocketEvent>();

    // Spawn socket listener thread
    let listener = SocketListener::bind(socket_tx);
    thread::spawn(move || {
        listener.run();
    });

    println!("Listening for commands...");

    // Event loop with immutable state transitions
    let ui_position = config.ui.position;
    let output_config = config.output.clone();
    socket_rx
        .iter()
        .fold(None::<AudioCapture>, |capture, event| match event {
            SocketEvent::Command(SocketMessage::Start) => capture.or_else(|| {
                Some(start_recording(
                    &transcription,
                    &ui_handle,
                    ui_position,
                    &output_config,
                ))
            }),
            SocketEvent::Command(SocketMessage::Stop) => {
                capture.map(|c| {
                    drop(c);
                    println!("Recording stopped.");
                });
                None
            }
            SocketEvent::Command(SocketMessage::Toggle) => match capture {
                Some(c) => {
                    drop(c);
                    println!("Recording stopped.");
                    None
                }
                None => Some(start_recording(
                    &transcription,
                    &ui_handle,
                    ui_position,
                    &output_config,
                )),
            },
            SocketEvent::Command(SocketMessage::Status) => capture,
            SocketEvent::StatusRequest(req) => {
                let state = if capture.is_some() {
                    RecordingState::Recording
                } else {
                    RecordingState::Idle
                };
                let _ = req.response_tx.send(state);
                capture
            }
        });
}

/// Start audio capture pipeline and wire up all threads.
/// Returns AudioCapture handle - dropping it stops recording.
fn start_recording(
    transcription: &Transcription,
    ui_handle: &recording_indicator::Handle,
    ui_position: UiPosition,
    output_config: &OutputConfig,
) -> AudioCapture {
    // Start audio capture
    let (audio_rx, capture) = audio_capture::start();

    // Create processed audio channel
    let processed_rx = audio_processor::start(audio_rx);

    // Fanout processed audio to transcription and UI
    let (transcription_rx, ui_audio_rx) = fanout::duplicate(processed_rx);
    ui_handle.start(ui_audio_rx, ui_position);

    println!("Recording started");

    // Send audio to transcription thread
    let text_rx = transcription.start(transcription_rx);

    // Spawn text output thread
    injection::spawn(text_rx, output_config);

    capture
}
