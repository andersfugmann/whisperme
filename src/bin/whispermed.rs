use std::thread;

use crossbeam_channel as channel;

use whisperme::audio_capture::{self, AudioCapture};
use whisperme::audio_processor;
use whisperme::config::{Config, OutputConfig, UiPosition};
use whisperme::fanout;
use whisperme::injection;
use whisperme::socket::{RecordingState, SocketEvent, SocketListener, SocketMessage};
use whisperme::transcription::Transcription;
use whisperme::ui::{self, UiSender};

fn main() {
    let config = Config::load();
    println!("WhisperMe Daemon starting...");
    println!("Config: {config:#?}");

    // Spawn transcription thread (loads model)
    let transcription = Transcription::new(&config.whisper, config.transcription);

    // Create socket event channel
    let (socket_tx, socket_rx) = channel::unbounded::<SocketEvent>();

    // Spawn socket listener thread
    let listener = SocketListener::bind(socket_tx);
    thread::spawn(move || {
        listener.run();
    });

    // Spawn persistent UI thread (if UI is enabled)
    let ui_tx = ui::spawn(config.ui.position);

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
                    Some(ui_position),
                    &output_config,
                    &ui_tx,
                ))
            }),
            SocketEvent::Command(SocketMessage::Stop) => {
                capture.map(|c| {
                    drop(c);
                    println!("Recording stopped, transcribing...");
                });
                None
            }
            SocketEvent::Command(SocketMessage::Toggle) => match capture {
                Some(c) => {
                    drop(c);
                    println!("Recording stopped, transcribing...");
                    None
                }
                None => Some(start_recording(
                    &transcription,
                    Some(ui_position),
                    &output_config,
                    &ui_tx,
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
    ui_position: Option<UiPosition>,
    output_config: &OutputConfig,
    ui_tx: &UiSender,
) -> AudioCapture {
    // Start audio capture
    let (audio_rx, capture) = audio_capture::start();

    // Create processed audio channel
    let processed_rx = audio_processor::start(audio_rx);

    // Spawn fanout thread: distribute processed audio to transcription and UI
    let transcription_rx = match ui_position {
        Some(ui_position) => {
            let (transcription_rx, ui_audio_rx) = fanout::duplicate(processed_rx);
            // Send show request to persistent UI thread
            ui_tx
                .send(ui::UiRequest::Show(ui_audio_rx, ui_position))
                .ok();
            transcription_rx
        }
        None => processed_rx,
    };
    println!("Recording started");

    // Send audio to transcription thread
    let text_rx = transcription.start(transcription_rx);

    // Spawn text output thread
    injection::spawn(text_rx, output_config);

    capture
}
