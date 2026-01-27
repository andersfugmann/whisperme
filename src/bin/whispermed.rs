use std::thread;

use crossbeam_channel as channel;

use whisperme::audio::AudioCapture;
use whisperme::config::Config;
use whisperme::fanout;
use whisperme::audio_processor;
use whisperme::injection;
use whisperme::socket::{RecordingState, SocketEvent, SocketListener, SocketMessage};
use whisperme::transcription::Transcription;
use whisperme::ui::{self, UiRequest, UiSender};

fn main() {
    let config = Config::load();
    println!("WhisperMe Daemon starting...");
    println!("Config: {config:#?}");

    // Spawn transcription thread (loads model)
    let transcription = Transcription::new(&config.whisper, config.transcription);

    // Spawn UI thread if enabled
    let ui_tx: Option<UiSender> = config.ui.enabled.then(|| ui::spawn(config.ui.position));

    // Create socket event channel
    let (socket_tx, socket_rx) = channel::unbounded::<SocketEvent>();

    // Spawn socket listener thread
    let listener = SocketListener::bind(socket_tx);
    thread::spawn(move || {
        listener.run();
    });

    println!("Listening for commands...");

    // Event loop with immutable state transitions
    socket_rx
        .iter()
        .fold(None::<AudioCapture>, |capture, event| match event {
            SocketEvent::Command(SocketMessage::Start) => {
                capture.or_else(|| Some(start_recording(&transcription, &ui_tx)))
            }
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
                None => Some(start_recording(&transcription, &ui_tx)),
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
    ui_cmd: &Option<UiSender>,
) -> AudioCapture {
    // Create channels
    let (audio_tx, audio_rx) = channel::unbounded::<f32>(); // 48kHz raw
    let (processed_tx, processed_rx) = channel::unbounded::<f32>(); // 16kHz processed
    let (text_tx, text_rx) = channel::unbounded::<String>();
    audio_processor::spawn(audio_rx, processed_tx);
    // Create global text channel for injection
    injection::spawn(text_rx);

    // Spawn fanout thread: distribute processed audio to transcription and UI
    let transcription_rx =
        match ui_cmd {
            Some(ui_cmd) => {
                // Send audio receiver to UI thread (If there is to be a UI)
                // Should just show the UI - and not clone.
                let (transcription_tx, transcription_rx) = channel::unbounded::<f32>();
                let (ui_tx, ui_rx) = channel::unbounded::<f32>();
                fanout::spawn(processed_rx, vec![transcription_tx, ui_tx]);
                let _ = ui_cmd.send(UiRequest::Show(ui_rx));
                transcription_rx
            }
            None => processed_rx
        };

    // Send audio to transcription thread
    transcription.spawn(transcription_rx, text_tx /* Clone???? */);
    println!("Recording started");
    AudioCapture::new(audio_tx)
}
