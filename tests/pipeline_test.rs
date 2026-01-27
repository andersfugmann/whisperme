//! Pipeline integration test: audio file → processing → transcription.
//!
//! Tests the audio processing pipeline using a pre-recorded audio file.
//! Full transcription test requires Whisper model and is slow.
//!
//! Run with: cargo test --test pipeline_test
//! Run slow tests: make test-slow

use std::collections::HashSet;
use std::path::Path;

use hound::WavReader;
use whisperme::audio_processor::{AudioProcessor, SAMPLE_RATE};

/// Load WAV file and return samples as f32 in [-1, 1] range.
fn load_wav(path: &Path) -> Vec<f32> {
    let reader = WavReader::open(path).expect("failed to open WAV file");
    let spec = reader.spec();

    assert_eq!(spec.channels, 1, "expected mono audio");
    assert_eq!(spec.sample_rate, 48000, "expected 48kHz sample rate");

    match spec.sample_format {
        hound::SampleFormat::Int => {
            let max_val = (1 << (spec.bits_per_sample - 1)) as f32;
            reader
                .into_samples::<i32>()
                .map(|s| s.expect("failed to read sample") as f32 / max_val)
                .collect()
        }
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .map(|s| s.expect("failed to read sample"))
            .collect(),
    }
}

/// Calculate word similarity between two strings.
/// Returns a value between 0.0 and 1.0.
fn word_similarity(actual: &str, expected: &str) -> f32 {
    let actual_lower = actual.to_lowercase();
    let expected_lower = expected.to_lowercase();

    let actual_words: HashSet<&str> = actual_lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    let expected_words: HashSet<&str> = expected_lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();

    if expected_words.is_empty() {
        return if actual_words.is_empty() { 1.0 } else { 0.0 };
    }

    let intersection = actual_words.intersection(&expected_words).count();
    intersection as f32 / expected_words.len() as f32
}

/// Test that AudioProcessor produces expected output size.
#[test]
fn test_audio_processor_output_ratio() {
    let mut processor = AudioProcessor::new();

    // 1 second of 48kHz audio
    let input = vec![0.0f32; 48000];
    let output = processor.process(&input);

    // Should produce approximately 1 second of 16kHz audio (minus first 10ms warmup frame)
    // Expected: ~16000 - 160 = ~15840 samples
    let expected_min = 15000;
    let expected_max = 16500;

    assert!(
        output.len() >= expected_min && output.len() <= expected_max,
        "Expected {} to {} samples, got {}",
        expected_min,
        expected_max,
        output.len()
    );
}

/// Test processing a real audio file.
#[test]
fn test_process_jfk_audio() {
    let audio_path = Path::new("tests/fixtures/jfk_48k.wav");
    assert!(
        audio_path.exists(),
        "Test audio file not found: {:?}",
        audio_path
    );

    let audio_48k = load_wav(audio_path);
    println!("Loaded {} samples from {:?}", audio_48k.len(), audio_path);

    // Process through AudioProcessor
    let mut processor = AudioProcessor::new();
    let audio_16k = processor.process(&audio_48k);
    println!(
        "Processed to {} samples at {}Hz",
        audio_16k.len(),
        SAMPLE_RATE
    );

    // 11 seconds at 48kHz → ~11 seconds at 16kHz
    // Input: ~528000 samples, Output: ~176000 samples (minus warmup)
    assert!(
        audio_16k.len() > 170000,
        "Expected at least 170k samples, got {}",
        audio_16k.len()
    );

    // Check samples are in valid range
    for &sample in &audio_16k {
        assert!(
            sample >= -1.1 && sample <= 1.1,
            "Sample out of range: {}",
            sample
        );
    }
}

/// Full pipeline test: WAV file → AudioProcessor → TranscriptionThread → text.
/// Requires Whisper model - run with: make test-slow
#[test]
#[cfg(feature = "slow_tests")]
fn test_audio_to_text_pipeline() {
    use crossbeam_channel as channel;
    use whisperme::config::{TranscriptionConfig, WhisperConfig};
    use whisperme::transcription::{TranscriptionRequest, TranscriptionThread};

    // Load test audio
    let audio_path = Path::new("tests/fixtures/jfk_48k.wav");
    assert!(
        audio_path.exists(),
        "Test audio file not found: {:?}",
        audio_path
    );

    let audio_48k = load_wav(audio_path);
    println!("Loaded {} samples from {:?}", audio_48k.len(), audio_path);

    // Process through AudioProcessor (48kHz → 16kHz with noise cancellation)
    let mut processor = AudioProcessor::new();
    let audio_16k = processor.process(&audio_48k);
    println!(
        "Processed to {} samples at {}Hz",
        audio_16k.len(),
        SAMPLE_RATE
    );

    // Use tiny.en model for testing (downloaded by make test-slow)
    // Path starts with "./" so it's resolved relative to cwd
    let model_path = Path::new("./models/ggml-medium.en.bin");

    assert!(
        model_path.exists(),
        "Whisper model not found: {:?}. Run 'make download-model-tiny' first.",
        model_path
    );

    let whisper_config = WhisperConfig {
        model: model_path
            .file_name()
            .expect("")
            .to_str()
            .expect("")
            .to_string(),
        model_path: model_path.to_path_buf(),
        language: "en".to_string(),
    };

    println!("Loading Whisper model: {:?}", whisper_config.model_path);
    let transcription = TranscriptionThread::new(&whisper_config, TranscriptionConfig::default());

    // Create channels for audio and text
    let (audio_tx, audio_rx) = channel::unbounded::<f32>();
    let (text_tx, text_rx) = channel::unbounded::<String>();

    // Send transcription request
    transcription.send(TranscriptionRequest::ProcessAudio(audio_rx, text_tx));

    // Send all audio samples
    for sample in audio_16k {
        audio_tx.send(sample).expect("failed to send audio sample");
    }
    drop(audio_tx); // Signal end of audio

    // Collect transcribed text
    let mut transcribed_text = String::new();
    while let Ok(text) = text_rx.recv() {
        transcribed_text.push_str(&text);
    }

    println!("Transcription: {}", transcribed_text);

    // Expected JFK quote
    let expected = "And so my fellow Americans, ask not what your country can do for you, ask what you can do for your country.";

    // Fuzzy compare
    let similarity = word_similarity(&transcribed_text, expected);
    println!("Similarity: {:.1}%", similarity * 100.0);

    assert!(
        similarity >= 0.7,
        "Expected at least 70% word similarity, got {:.1}%\nActual: {}\nExpected: {}",
        similarity * 100.0,
        transcribed_text,
        expected
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_similarity_identical() {
        assert_eq!(word_similarity("hello world", "hello world"), 1.0);
    }

    #[test]
    fn test_word_similarity_case_insensitive() {
        assert_eq!(word_similarity("Hello World", "hello world"), 1.0);
    }

    #[test]
    fn test_word_similarity_partial() {
        let sim = word_similarity("hello there world", "hello world");
        assert!(sim >= 0.99, "expected 1.0, got {}", sim); // all expected words present
    }

    #[test]
    fn test_word_similarity_none() {
        assert_eq!(word_similarity("foo bar", "hello world"), 0.0);
    }
}
