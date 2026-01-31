//! Test binary that records audio and prints dBFS levels per 200ms sample.

use crossbeam_channel as channel;
use whisperme::audio_capture;
use whisperme::audio_processor;

const SAMPLE_RATE: usize = 16000;
const WINDOW_MS: usize = 200;
const SAMPLES_PER_WINDOW: usize = SAMPLE_RATE * WINDOW_MS / 1000;

fn main() {
    println!("Recording audio... Press Ctrl+C to stop.");
    println!("dBFS levels (200ms windows):");
    println!();

    let (processed_tx, processed_rx) = channel::unbounded::<f32>();

    let (audio_rx, _stop_capture) = audio_capture::start();
    audio_processor::spawn(audio_rx, processed_tx);

    loop {
        let samples: Vec<f32> = processed_rx.iter().take(SAMPLES_PER_WINDOW).collect();
        let rms = calculate_rms(&samples);
        let dbfs = rms_to_dbfs(rms);
        print_level(dbfs);
    }
}

fn calculate_rms(samples: &[f32]) -> f32 {
    let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
    (sum_squares / samples.len() as f32).sqrt()
}

fn rms_to_dbfs(rms: f32) -> f32 {
    20.0 * rms.max(0.0).log10()
}

fn print_level(dbfs: f32) {
    let bar_width = 60;
    let normalized = ((dbfs + 80.0) / 80.0).clamp(0.0, 1.0);
    let filled = (normalized * bar_width as f32) as usize;
    let bar: String = "█".repeat(filled) + &" ".repeat(bar_width - filled);
    print!("\r[{}] {:6.1} dBFS", bar, dbfs);
    let _ = std::io::Write::flush(&mut std::io::stdout());
}
