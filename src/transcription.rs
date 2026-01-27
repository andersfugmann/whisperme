//! Whisper transcription thread.

use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperSegment,
    WhisperState,
};

use crate::UnwrapOrExit;
use crate::audio::{AudioReceiver, TextSender};
use crate::audio_processor::SAMPLE_RATE;
use crate::config::{TranscriptionConfig, WhisperConfig};
use std::sync::Arc;

/// Minimum audio samples required by Whisper (1.2 seconds at 16kHz).
const MIN_AUDIO_SAMPLES: usize = (1.2 * SAMPLE_RATE as f32) as usize;

/// Transcription thread that loads the Whisper model and processes audio.
pub struct Transcription {
    ctx : Arc<WhisperContext>,
    language : String,
    config: TranscriptionConfig,
}
impl Transcription {
    /// Spawns a new transcription thread with the model loaded.
    pub fn new(whisper_config: &WhisperConfig, config: TranscriptionConfig) -> Self {
        let language = whisper_config.language.clone();
        // Load the Whisper model on the main thread
        let ctx = Arc::new(load_model(whisper_config));
        Self { ctx, language, config }
    }

    pub fn spawn(&self, audio_rx: Receiver<f32>, text_tx: Sender<String>) -> JoinHandle<()> {
        let ctx = Arc::clone(&self.ctx);
        let language = self.language.clone();
        let config = self.config.clone();
        thread::spawn(move || {
            process_audio(
                &ctx,
                audio_rx,
                text_tx,
                &language,
                &config);
        })
    }
}

fn load_model(config: &WhisperConfig) -> WhisperContext {
    let model_path = &config.model_path;
    if !model_path.exists() {
        eprintln!("error: model file not found: {}", model_path.display());
        std::process::exit(1);
    }

    // Suppress whisper.cpp debug output
    whisper_rs::install_logging_hooks();

    println!("Loading Whisper model from: {}", model_path.display());
    let model_path_str = model_path
        .to_str()
        .unwrap_or_exit("model path contains invalid UTF-8 characters");
    let ctx = WhisperContext::new_with_params(model_path_str, WhisperContextParameters::default())
        .unwrap_or_exit(&format!(
            "failed to load Whisper model from {} - file may be corrupted, try re-downloading",
            model_path.display()
        ));
    println!("Whisper model loaded successfully");

    ctx
}

fn process_audio(
    ctx: &WhisperContext,
    audio_rx: AudioReceiver,
    text_tx: TextSender,
    config_language: &str,
    config: &TranscriptionConfig,
) {
    let mut total_samples: usize = 0;
    let mut audio_buffer: Vec<f32> = Vec::new();
    let mut language: Option<String> = None;

    if config_language != "auto" {
        language = Some(config_language.to_string());
    }

    let mut recording = true;
    let mut first_element = true;
    while recording {
        let record_for_ms = if audio_buffer.len() < MIN_AUDIO_SAMPLES {
            let samples_needed = MIN_AUDIO_SAMPLES - audio_buffer.len();
            let samples_needed_ms = samples_needed * 1000 / SAMPLE_RATE as usize;
            std::cmp::max(samples_needed_ms, config.transcription_interval_ms)
        } else {
            config.transcription_interval_ms
        };
        let (new_chunk, closed) = collect_audio_for(&audio_rx, record_for_ms);
        recording = !closed;
        if !recording {
            println!("*** Recording has stopped");
        }
        total_samples += new_chunk.len();
        audio_buffer.extend(new_chunk);

        let current_audio_duration_ms = audio_buffer.len() * 1000 / SAMPLE_RATE as usize;

        // Prepare audio for Whisper, padding with silence if too short
        // Hard pad with silence. This will only be needed if recording has stopped. So its ok to just extend the existing buffer.
        if audio_buffer.len() < MIN_AUDIO_SAMPLES {
            audio_buffer.resize(MIN_AUDIO_SAMPLES, 0.0);
        }

        if language.is_none() {
            let (detected_lang, confidence) = detect_language(ctx, &audio_buffer);
            if confidence >= config.language_confidence || !recording {
                println!(
                    "*** => Language detected: {} (confidence: {:.0}%)",
                    detected_lang,
                    confidence * 100.0
                );
                language = Some(detected_lang);
            } else {
                eprintln!("*** => Language detection in progress");
                continue;
            }
        }
        let lang = language.as_ref().unwrap();

        // Text segments can be into the future!
        let emit_threshold_ms : usize =
            if current_audio_duration_ms < config.emit_grace_ms {
                0
            } else {
                current_audio_duration_ms - config.emit_grace_ms
            };


        eprintln!(
            "*** => Grace ms = {}. Total_time: {}",
            emit_threshold_ms,
            total_samples as f32 / SAMPLE_RATE as f32
        );

        // Todo
        // Should hold all segments not emitted.
        // When getting new segments, test if the segment has been extended
        // and reuse previously calculated segment if the additional tokens are silence.
        // Example: Input one, two, thee, <silence>
        // Transcribed 1, 2, 3.
        // Transcribed 1, 2, 3, 4, 5.
        let last_emitted_end_ms = transcribe(ctx, &audio_buffer, lang).as_iter().fold(
            0,
            |acc: usize, segment: WhisperSegment| {
                let start_ms = segment.start_timestamp() as usize * 10;
                let end_ms = segment.end_timestamp() as usize * 10;
                let silence_prob = segment.no_speech_probability();
                let rms = calculate_rms(&audio_buffer, start_ms, end_ms);
                let text = segment.to_str_lossy().unwrap();
                eprintln!(
                    "*** => Segment [{} - {}] P: {}, RMS: {} : {}",
                    start_ms, end_ms, silence_prob, rms, text
                );

                if end_ms <= emit_threshold_ms || !recording {
                    eprintln!("*** => Emit segment");
                    // Ignore silence
                    if rms > config.silence_rms_threshold {
                        let text = text.to_string();
                        let text = if first_element {
                            first_element = false;
                            text.trim_start().to_string()
                        } else {
                            text
                        };
                        let _ = text_tx.send(text);
                    } else {
                        println!("Segment dropped as silence");
                    }
                    end_ms
                } else {
                    acc
                }
            },
        );

        let samples_to_remove = std::cmp::min(no_samples(last_emitted_end_ms), audio_buffer.len());
        audio_buffer.drain(..samples_to_remove);
    }

    println!("Streaming transcription complete");
}

/// Collects audio samples for at least the given duration.
/// First drains any queued samples, then waits for remaining time if needed.
/// Returns (samples, recording_stopped).
fn collect_audio_for(audio_rx: &AudioReceiver, time_ms: usize) -> (Vec<f32>, bool) {
    let samples_needed = no_samples(time_ms);
    let mut chunk: Vec<f32> = audio_rx.iter().take(samples_needed).collect();
    chunk.extend(audio_rx.try_iter());
    let closed = chunk.len() < samples_needed;
    (chunk, closed)
}

/// Detects language from audio buffer.
/// Returns (language_code, confidence).
fn detect_language(ctx: &WhisperContext, audio: &[f32]) -> (String, f32) {
    let mut state = ctx
        .create_state()
        .expect("Could not create state for language detection");
    // Convert to mel spectrogram first
    state
        .pcm_to_mel(audio, 1)
        .expect("Cannot send audio to model");

    // Detect language
    let (lang_id, probs) = state.lang_detect(0, 1).expect("Language detection failed");
    let lang_str = whisper_rs::get_lang_str(lang_id).expect("Unknown language");

    let confidence = probs.get(lang_id as usize).copied().unwrap_or(0.0);
    (lang_str.to_string(), confidence)
}

/// Transcribes audio buffer and returns segments with timestamps.
fn transcribe(ctx: &WhisperContext, audio: &[f32], language: &str) -> WhisperState {
    let mut state = ctx.create_state().unwrap();
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(language));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_split_on_word(true);
    params.set_token_timestamps(true);
    state.full(params, audio).unwrap();

    state
}
fn calculate_rms(audio: &[f32], start_ms: usize, end_ms: usize) -> f32 {
    let samples = no_samples(end_ms - start_ms);
    let rs = audio
        .iter()
        .skip(no_samples(start_ms))
        .take(samples)
        .map(|s| s * s)
        .sum::<f32>();
    rs / samples as f32
}

fn no_samples(ms: usize) -> usize {
    ms * SAMPLE_RATE as usize / 1000
}
