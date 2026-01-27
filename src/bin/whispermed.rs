use std::thread;

use crossbeam_channel as channel;

use whisperme::audio::{AudioCapture, TextSender};
use whisperme::config::Config;
use whisperme::fanout;
use whisperme::audio_processor;
use whisperme::injection;
use whisperme::socket::{RecordingState, SocketEvent, SocketListener, SocketMessage};
use whisperme::transcription::{TranscriptionRequest, TranscriptionThread};
use whisperme::ui::{self, UiRequest, UiSender};

fn main() {
    let config = Config::load();
    println!("WhisperMe Daemon starting...");
    println!("Config: {config:#?}");

    // Spawn transcription thread (loads model)
    let transcription = TranscriptionThread::new(&config.whisper, config.transcription);

    // Create global text channel for injection
    let (text_tx, text_rx) = channel::unbounded::<String>();

    // Spawn text injection thread with global receiver
    thread::spawn(move || {
        injection::run(text_rx);
    });

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
                capture.or_else(|| Some(start_recording(&transcription, &text_tx, &ui_tx)))
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
                None => Some(start_recording(&transcription, &text_tx, &ui_tx)),
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
    transcription: &TranscriptionThread,
    text_tx: &TextSender,
    ui_tx: &Option<UiSender>,
) -> AudioCapture {
    // Create channels
    let (raw_tx, raw_rx) = channel::unbounded::<f32>(); // 48kHz raw
    let (processed_tx, processed_rx) = channel::unbounded::<f32>(); // 16kHz processed
    let (transc_tx, transc_rx) = channel::unbounded::<f32>();
    let (ui_tx_audio, ui_rx) = channel::unbounded::<f32>();

    audio_processor::spawn(raw_rx, processed_tx);

    // Spawn fanout thread: distribute processed audio to transcription and UI
    fanout::spawn(processed_rx, vec![transc_tx, ui_tx_audio]);

    // Send audio to transcription thread
    transcription.send(TranscriptionRequest::ProcessAudio(
        transc_rx,
        text_tx.clone(),
    ));

    // Send audio receiver to UI thread
    ui_tx.as_ref().map(|tx| tx.send(UiRequest::Show(ui_rx)));

    println!("Recording started");
    AudioCapture::new(raw_tx)
}
