//! Test binary that records audio and prints dBFS levels per 200ms sample.

use whisperme::audio_capture;
use whisperme::audio_processor::{self, SAMPLE_RATE};
use whisperme::spectrum::{FFT_SIZE, FREQ_BANDS, band_db_levels};

const WINDOW_MS: usize = 200;
const SAMPLES_PER_WINDOW: usize = SAMPLE_RATE as usize * WINDOW_MS / 1000;

fn main() {
    println!("Recording audio... Press Ctrl+C to stop.");
    println!("dBFS levels (200ms windows):");
    println!();

    let (audio_rx, _stop_capture) = audio_capture::start();
    let processed_rx = audio_processor::start(audio_rx);

    loop {
        let samples: Vec<f32> = processed_rx.iter().take(SAMPLES_PER_WINDOW).collect();
        let rms = calculate_rms(&samples);
        let dbfs = rms_to_dbfs(rms);
        let bands = fft_window(&samples);
        print_level(dbfs, &bands);
    }
}

fn calculate_rms(samples: &[f32]) -> f32 {
    let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
    (sum_squares / samples.len() as f32).sqrt()
}

fn rms_to_dbfs(rms: f32) -> f32 {
    20.0 * rms.max(1e-10).log10()
}

fn fft_window(samples: &[f32]) -> [f32; 5] {
    let window: [f32; FFT_SIZE] = std::array::from_fn(|i| samples.get(i).copied().unwrap_or(0.0));
    band_db_levels(&window, SAMPLE_RATE).unwrap_or([-80.0; 5])
}

fn print_level(dbfs: f32, bands: &[f32; 5]) {
    let bar = |db: f32| {
        let n = ((db + 80.0) / 80.0).clamp(0.0, 1.0);
        let filled = (n * 10.0) as usize;
        "█".repeat(filled) + &"░".repeat(10 - filled)
    };
    let band_labels: Vec<String> = FREQ_BANDS
        .iter()
        .zip(bands.iter())
        .map(|(&(lo, hi), &db)| format!("{:4.0}-{:4.0}Hz [{}] {:5.1}", lo, hi, bar(db), db))
        .collect();
    print!("\r{:6.1} dBFS | {} ", dbfs, band_labels.join(" | "));
    let _ = std::io::Write::flush(&mut std::io::stdout());
}
