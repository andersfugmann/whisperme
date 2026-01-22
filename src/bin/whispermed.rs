use std::sync::mpsc;
use std::thread;
use whisperme::config::Config;
use whisperme::injection;
use whisperme::session::{RecordingSession, TextSender};
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
    let (text_tx, text_rx) = mpsc::channel::<String>();

    // Spawn text injection thread with global receiver
    thread::spawn(move || {
        injection::run(text_rx);
    });

    // Spawn UI thread if enabled
    let ui_tx: Option<UiSender> = if config.ui.enabled {
        Some(ui::spawn(config.ui.position))
    } else {
        None
    };

    // Create socket event channel
    let (socket_tx, socket_rx) = mpsc::channel::<SocketEvent>();
    
    // Spawn socket listener thread
    let listener = SocketListener::bind(socket_tx);
    thread::spawn(move || {
        listener.run();
    });

    let mut session: Option<RecordingSession> = None;

    println!("Listening for commands...");

    while let Ok(event) = socket_rx.recv() {
        match event {
            SocketEvent::Command(msg) => match msg {
                SocketMessage::Start => {
                    if session.is_none() {
                        start_recording(&mut session, &transcription, &text_tx, &ui_tx);
                    }
                }
                SocketMessage::Stop => {
                    if session.take().is_some() {
                        println!("Recording stopped, transcribing...");
                    }
                }
                SocketMessage::Toggle => {
                    if session.is_some() {
                        session = None;
                        println!("Recording stopped, transcribing...");
                    } else {
                        start_recording(&mut session, &transcription, &text_tx, &ui_tx);
                    }
                }
                SocketMessage::Status => {
                    // Status without response channel is ignored (handled by StatusRequest)
                }
            },
            SocketEvent::StatusRequest(req) => {
                let state = if session.is_some() {
                    RecordingState::Recording
                } else {
                    RecordingState::Idle
                };
                let _ = req.response_tx.send(state);
            }
        }
    }
}

/// Start a new recording session and wire up all threads.
fn start_recording(
    session: &mut Option<RecordingSession>,
    transcription: &TranscriptionThread,
    text_tx: &TextSender,
    ui_tx: &Option<UiSender>,
) {
    let (new_session, audio_rx, ui_rx) = RecordingSession::start();

    // Send audio to transcription thread with cloned text sender
    transcription.send(TranscriptionRequest::ProcessAudio(audio_rx, text_tx.clone()));

    // Send audio receiver to UI thread
    if let Some(tx) = ui_tx {
        let _ = tx.send(UiRequest::Show(ui_rx));
    }

    *session = Some(new_session);
    println!("Recording started");
}
