//! Simple UI test: displays the recording indicator window.
//! Press Ctrl+C or wait 10 seconds to close.

use std::thread;
use std::time::Duration;

use crossbeam_channel as channel;

use whisperme::config::UiPosition;
use whisperme::ui::{self, UiRequest};

fn main() {
    // Parse position from command line or default to BottomRight
    let position = std::env::args()
        .nth(1)
        .map(|s| match s.to_lowercase().as_str() {
            "top-left" | "tl" => UiPosition::TopLeft,
            "top-center" | "tc" => UiPosition::TopCenter,
            "top-right" | "tr" => UiPosition::TopRight,
            "bottom-left" | "bl" => UiPosition::BottomLeft,
            "bottom-center" | "bc" => UiPosition::BottomCenter,
            _ => UiPosition::BottomRight,
        })
        .unwrap_or(UiPosition::BottomRight);

    let position_name = match position {
        UiPosition::TopLeft => "Top-Left",
        UiPosition::TopCenter => "Top-Center",
        UiPosition::TopRight => "Top-Right",
        UiPosition::BottomLeft => "Bottom-Left",
        UiPosition::BottomCenter => "Bottom-Center",
        UiPosition::BottomRight => "Bottom-Right",
    };

    println!("Starting UI test window...");
    println!("Position: {}", position_name);
    println!("Press Ctrl+C or wait 10 seconds to close.");
    println!();
    println!("Usage: ui-test [position]");
    println!("  Positions: top-left (tl), top-center (tc), top-right (tr),");
    println!("             bottom-left (bl), bottom-center (bc), bottom-right (br)");

    // Create a fake audio channel that sends silence
    let (audio_tx, audio_rx) = channel::unbounded::<f32>();

    // Spawn audio generator thread (sends silence with occasional "activity")
    let audio_handle = thread::spawn(move || {
        let sample_rate = 16000;
        let mut phase: f32 = 0.0;

        loop {
            // Generate a simple sine wave to show bar activity
            let sample =
                (phase * 2.0 * std::f32::consts::PI * 440.0 / sample_rate as f32).sin() * 0.3;
            phase += 1.0;

            if audio_tx.send(sample).is_err() {
                break;
            }

            // Simulate real-time audio rate
            thread::sleep(Duration::from_micros(1000000 / sample_rate as u64));
        }
    });

    // Spawn UI
    let ui_tx = ui::spawn(position);

    // Show the window
    if ui_tx.send(UiRequest::Show(audio_rx)).is_err() {
        eprintln!("Failed to send show request to UI");
        return;
    }

    // Wait 10 seconds then exit
    thread::sleep(Duration::from_secs(10));

    println!("Closing UI test...");
    drop(audio_handle);
}
