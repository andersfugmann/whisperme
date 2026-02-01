//! Unix socket IPC for daemon communication.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::thread;

use crossbeam_channel as channel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketMessage {
    Start,
    Stop,
    Toggle,
    Status,
}

impl std::str::FromStr for SocketMessage {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "start" => Ok(Self::Start),
            "stop" => Ok(Self::Stop),
            "toggle" => Ok(Self::Toggle),
            "status" => Ok(Self::Status),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingState {
    Recording,
    Idle,
}

impl RecordingState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Idle => "idle",
        }
    }
}

pub struct StatusRequest {
    pub response_tx: channel::Sender<RecordingState>,
}

pub enum SocketEvent {
    Command(SocketMessage),
    StatusRequest(StatusRequest),
}

pub type EventSender = channel::Sender<SocketEvent>;
pub type EventReceiver = channel::Receiver<SocketEvent>;

fn socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join("whisperme.sock")
}

pub struct SocketListener {
    listener: UnixListener,
    message_tx: EventSender,
}

impl SocketListener {
    pub fn bind(message_tx: EventSender) -> Self {
        let path = socket_path();

        // Remove stale socket file if exists
        if path.exists() {
            std::fs::remove_file(&path).expect("failed to remove stale socket - check permissions");
        }

        let listener =
            UnixListener::bind(&path).expect("failed to bind socket - check directory permissions");

        Self {
            listener,
            message_tx,
        }
    }

    /// Run the socket listener in a blocking loop.
    /// This spawns a thread per connection.
    pub fn run(self) {
        for stream in self.listener.incoming() {
            match stream {
                Ok(stream) => {
                    let tx = self.message_tx.clone();
                    thread::spawn(move || {
                        Self::handle_connection(stream, tx);
                    });
                }
                Err(e) => {
                    eprintln!("Socket accept error: {e}");
                }
            }
        }
    }

    fn handle_connection(mut stream: UnixStream, tx: EventSender) {
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();

        if reader.read_line(&mut line).is_err() {
            return;
        }

        let Ok(msg) = line.parse::<SocketMessage>() else {
            return;
        };

        if msg == SocketMessage::Status {
            let (response_tx, response_rx) = channel::unbounded();
            let _ = tx.send(SocketEvent::StatusRequest(StatusRequest { response_tx }));
            if let Ok(state) = response_rx.recv() {
                let _ = writeln!(stream, "{}", state.as_str());
            }
        } else {
            let _ = tx.send(SocketEvent::Command(msg));
        }
    }
}

impl Drop for SocketListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(socket_path());
    }
}

/// Send a command to the daemon and optionally get a response.
pub fn send_command(command: &str) -> Option<String> {
    let path = socket_path();

    if !path.exists() {
        return None;
    }

    let mut stream = UnixStream::connect(&path).ok()?;
    writeln!(stream, "{command}").ok()?;

    if command == "status" {
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response).ok()?;
        Some(response.trim().to_string())
    } else {
        None
    }
}
