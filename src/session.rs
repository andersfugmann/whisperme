//! Recording session management.

use std::thread;
use std::sync::mpsc;

use crate::audio::{AudioCapture, AudioReceiver};

pub type TextSender = mpsc::Sender<String>;
pub type TextReceiver = mpsc::Receiver<String>;

/// Encapsulates all resources for a single recording session.
/// Dropping the session stops the capture.
pub struct RecordingSession {
    #[allow(dead_code)]
    capture: AudioCapture,
}

impl RecordingSession {
    /// Starts a new recording session.
    ///
    /// Returns:
    /// - The session itself (owns resources)
    /// - AudioReceiver for transcription
    /// - AudioReceiver for UI visualization
    pub fn start() -> (Self, AudioReceiver, AudioReceiver) {
        // Create fan-out: one sender from capture, two receivers via distributor
        let (capture_tx, capture_rx) = mpsc::channel::<f32>();
        let (transc_tx, transc_rx) = mpsc::channel::<f32>();
        let (ui_tx, ui_rx) = mpsc::channel::<f32>();

        // Spawn distributor thread to fan out audio to both receivers
        thread::spawn(move || {
            while let Ok(sample) = capture_rx.recv() {
                // Send to both; ignore errors if receiver dropped
                let _ = transc_tx.send(sample);
                let _ = ui_tx.send(sample);
            }
        });

        let capture = AudioCapture::new(capture_tx);

        (
            Self { capture },
            transc_rx,
            ui_rx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::SAMPLE_RATE;
    use std::time::{Duration, Instant};

    #[test]
    fn test_session_capture_3_seconds() {
        let (_session, transc_rx, ui_rx) = RecordingSession::start();

        // Allow startup time
        thread::sleep(Duration::from_millis(100));

        let expected_samples = SAMPLE_RATE as u64 * 2; // 2 seconds worth
        let timeout = Duration::from_secs(4);
        let start = Instant::now();

        let transc_handle = thread::spawn(move || {
            let mut count = 0u64;
            while count < expected_samples && start.elapsed() < timeout {
                match transc_rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(_) => count += 1,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            count
        });

        let ui_handle = thread::spawn(move || {
            let mut count = 0u64;
            while count < expected_samples && start.elapsed() < timeout {
                match ui_rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(_) => count += 1,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            count
        });

        let t_count = transc_handle.join().unwrap();
        let u_count = ui_handle.join().unwrap();

        println!(
            "Transcription rx: {} samples, UI rx: {} samples (expected ~{})",
            t_count, u_count, expected_samples
        );
        // Allow 20% margin for timing variations
        assert!(
            t_count >= expected_samples * 80 / 100,
            "Too few transc samples: {}",
            t_count
        );
        assert!(
            u_count >= expected_samples * 80 / 100,
            "Too few ui samples: {}",
            u_count
        );
    }
}
