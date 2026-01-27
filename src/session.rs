//! Recording session management.

use std::thread;
use std::sync::mpsc;

use crate::audio::{AudioCapture, AudioReceiver};
use crate::audio_processor::AudioProcessor;

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
    /// - AudioReceiver for transcription (16kHz processed)
    /// - AudioReceiver for UI visualization (16kHz processed)
    pub fn start() -> (Self, AudioReceiver, AudioReceiver) {
        // Create channels
        let (raw_tx, raw_rx) = mpsc::channel::<f32>();           // 48kHz raw
        let (processed_tx, processed_rx) = mpsc::channel::<f32>(); // 16kHz processed
        let (transc_tx, transc_rx) = mpsc::channel::<f32>();
        let (ui_tx, ui_rx) = mpsc::channel::<f32>();

        // Spawn processor thread: raw 48kHz → processed 16kHz
        thread::spawn(move || {
            let mut processor = AudioProcessor::new();
            while let Ok(sample) = raw_rx.recv() {
                let processed = processor.process(&[sample]);
                for s in processed {
                    let _ = processed_tx.send(s);
                }
            }
        });

        // Spawn distributor thread: fan out processed audio to both receivers
        thread::spawn(move || {
            while let Ok(sample) = processed_rx.recv() {
                let _ = transc_tx.send(sample);
                let _ = ui_tx.send(sample);
            }
        });

        let capture = AudioCapture::new(raw_tx);

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
    use crate::audio_processor::SAMPLE_RATE;
    use std::time::{Duration, Instant};

    /// Requires audio hardware - run with: cargo test -- --ignored
    #[test]
    #[ignore]
    fn test_session_capture_3_seconds() {
        let (_session, transc_rx, ui_rx) = RecordingSession::start();

        // Allow startup time
        thread::sleep(Duration::from_millis(100));

        let expected_samples = SAMPLE_RATE as u64 * 2; // 2 seconds worth at 16kHz
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
