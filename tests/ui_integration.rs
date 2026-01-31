//! Integration test: microphone capture to UI visualization.
//!
//! This test verifies that:
//! 1. Audio can be captured from the microphone
//! 2. Audio samples flow through channels
//! 3. The UI window can receive and process audio
//!
//! Run with: make test-hardware
//! (requires audio hardware and display)

#![cfg(feature = "system")]

use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel as channel;

use whisperme::audio_capture;
use whisperme::audio_processor;
use whisperme::audio_processor::{CAPTURE_RATE, SAMPLE_RATE};
use whisperme::config::UiPosition;

/// Test that audio capture works and produces samples.
#[test]
fn test_audio_capture_produces_samples() {
    let (rx, _capture) = audio_capture::start();

    // Allow capture thread to initialize
    thread::sleep(Duration::from_millis(500));

    let mut sample_count = 0u64;
    let start = Instant::now();
    let timeout = Duration::from_secs(3);
    let expected = CAPTURE_RATE as u64 * 2; // 2 seconds worth at 48kHz

    while sample_count < expected && start.elapsed() < timeout {
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(_) => sample_count += 1,
            Err(channel::RecvTimeoutError::Timeout) => continue,
            Err(channel::RecvTimeoutError::Disconnected) => break,
        }
    }

    println!("Captured {} samples in {:?}", sample_count, start.elapsed());
    // Allow 20% margin for timing
    assert!(
        sample_count >= expected * 80 / 100,
        "Expected at least {} samples, got {}",
        expected * 80 / 100,
        sample_count
    );
}

/// Test that audio flows to multiple receivers via fan-out.
#[test]
fn test_audio_flows_to_multiple_receivers() {
    use whisperme::audio_processor::AudioProcessor;
    use whisperme::fanout;

    // Create pipeline: capture → processor → fanout → receivers
    let (processed_tx, processed_rx) = channel::unbounded::<f32>();
    let (raw_rx, _capture) = audio_capture::start();

    // Spawn processor thread
    thread::spawn(move || {
        let mut processor = AudioProcessor::new();
        raw_rx.iter().for_each(|sample| {
            processor.process(&[sample]).into_iter().for_each(|s| {
                let _ = processed_tx.send(s);
            });
        });
    });

    // Spawn fanout
    let (transc_rx, ui_rx) = fanout::duplicate(processed_rx);

    // Allow capture to initialize
    thread::sleep(Duration::from_millis(500));

    let expected = SAMPLE_RATE as u64; // 1 second worth at 16kHz (processed)
    let timeout = Duration::from_secs(3);
    let start = Instant::now();

    // Count samples on UI receiver
    let ui_handle = thread::spawn(move || {
        let mut count = 0u64;
        while count < expected && start.elapsed() < timeout {
            match ui_rx.recv_timeout(Duration::from_millis(10)) {
                Ok(_) => count += 1,
                Err(_) => continue,
            }
        }
        count
    });

    // Count samples on transcription receiver
    let transc_handle = thread::spawn(move || {
        let mut count = 0u64;
        while count < expected && start.elapsed() < timeout {
            match transc_rx.recv_timeout(Duration::from_millis(10)) {
                Ok(_) => count += 1,
                Err(_) => continue,
            }
        }
        count
    });

    let ui_count = ui_handle.join().unwrap();
    let transc_count = transc_handle.join().unwrap();

    println!(
        "UI receiver got {} samples, Transcription got {}",
        ui_count, transc_count
    );
    // Allow 20% margin
    assert!(
        ui_count >= expected * 80 / 100,
        "Expected at least {} UI samples, got {}",
        expected * 80 / 100,
        ui_count
    );
    assert!(
        transc_count >= expected * 80 / 100,
        "Expected at least {} transc samples, got {}",
        expected * 80 / 100,
        transc_count
    );
}

/// Integration test: spawn UI with real audio for visual inspection.
#[test]
fn test_ui_window_with_audio() {
    use whisperme::ui;

    // Spawn UI thread
    let (capture_rx, _capture) = audio_capture::start();
    let ui_audio_rx = audio_processor::start(capture_rx);
    ui::show(ui_audio_rx, UiPosition::BottomRight);

    // Let it run for 5 seconds for visual inspection
    thread::sleep(Duration::from_secs(5));

    println!("UI test complete - window should have appeared and closed");
}
