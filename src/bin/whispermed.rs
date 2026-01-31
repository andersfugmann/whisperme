use std::thread;

use crossbeam_channel as channel;

use whisperme::audio_capture;
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
        .fold(None::<Box<dyn FnOnce()>>, |stop_fn, event| match event {
            SocketEvent::Command(SocketMessage::Start) => stop_fn.or_else(|| {
                Some(start_recording(
                    &transcription,
                    Some(ui_position),
                    &output_config,
                    &ui_tx,
                ))
            }),
            SocketEvent::Command(SocketMessage::Stop) => {
                stop_fn.map(|f| {
                    f();
                    println!("Recording stopped, transcribing...");
                });
                None
            }
            SocketEvent::Command(SocketMessage::Toggle) => match stop_fn {
                Some(f) => {
                    f();
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
            SocketEvent::Command(SocketMessage::Status) => stop_fn,
            SocketEvent::StatusRequest(req) => {
                let state = if stop_fn.is_some() {
                    RecordingState::Recording
                } else {
                    RecordingState::Idle
                };
                let _ = req.response_tx.send(state);
                stop_fn
            }
        });
}

/// Start audio capture pipeline and wire up all threads.
/// Returns a closure that stops recording when called.
fn start_recording(
    transcription: &Transcription,
    ui_position: Option<UiPosition>,
    output_config: &OutputConfig,
    ui_tx: &UiSender,
) -> Box<dyn FnOnce()> {
    // Start audio capture
    let (audio_rx, stop_capture) = audio_capture::start();

    // Create processed audio channel
    let (processed_tx, processed_rx) = channel::unbounded::<f32>(); // 16kHz processed
    audio_processor::spawn(audio_rx, processed_tx);

    // Spawn fanout thread: distribute processed audio to transcription and UI
    let transcription_rx = match ui_position {
        Some(ui_position) => {
            let (transcription_tx, transcription_rx) = channel::unbounded::<f32>();
            let (ui_audio_tx, ui_audio_rx) = channel::unbounded::<f32>();
            fanout::spawn(processed_rx, vec![transcription_tx, ui_audio_tx]);
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

    Box::new(stop_capture)
}
