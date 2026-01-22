//! Whisper transcription thread.

use std::sync::mpsc as std_mpsc;
use std::thread::{self, JoinHandle};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperSegment, WhisperState};

use crate::audio::{AudioReceiver, SAMPLE_RATE};
use crate::config::{TranscriptionConfig, WhisperConfig};
use crate::session::TextSender;
use crate::UnwrapOrExit;

/// Minimum audio samples required by Whisper (1.2 seconds at 16kHz).
const MIN_AUDIO_SAMPLES: usize = (1.2 * SAMPLE_RATE as f32) as usize;

/// Request sent to the transcription thread.
pub enum TranscriptionRequest {
    ProcessAudio(AudioReceiver, TextSender),
}

/// Transcription thread that loads the Whisper model and processes audio.
pub struct TranscriptionThread {
    request_tx: std_mpsc::Sender<TranscriptionRequest>,
    handle: JoinHandle<()>,
}

impl TranscriptionThread {
    /// Spawns a new transcription thread with the model loaded.
    pub fn new(whisper_config: &WhisperConfig, transcription_config: TranscriptionConfig) -> Self {
        let language = whisper_config.language.clone();
        // Load the Whisper model on the main thread
        let ctx = load_model(whisper_config);

        let (request_tx, request_rx) = std_mpsc::channel::<TranscriptionRequest>();

        let handle = thread::spawn(move || {
            run_transcription_thread(ctx, &language, transcription_config, request_rx);
        });

        Self { request_tx, handle }
    }

    /// Sends a transcription request to the thread.
    pub fn send(&self, request: TranscriptionRequest) {
        if self.request_tx.send(request).is_err() {
            eprintln!("error: transcription thread has exited unexpectedly");
            std::process::exit(1);
        }
    }

    /// Waits for the transcription thread to finish.
    pub fn join(self) {
        let _ = self.handle.join();
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
    let model_path_str = model_path.to_str().unwrap_or_exit(
        "model path contains invalid UTF-8 characters"
    );
    let ctx = WhisperContext::new_with_params(
        model_path_str,
        WhisperContextParameters::default(),
    )
    .unwrap_or_exit(&format!(
        "failed to load Whisper model from {} - file may be corrupted, try re-downloading",
        model_path.display()
    ));
    println!("Whisper model loaded successfully");

    ctx
}

fn run_transcription_thread(
    ctx: WhisperContext,
    language: &str,
    config: TranscriptionConfig,
    request_rx: std_mpsc::Receiver<TranscriptionRequest>,
) {
    while let Ok(request) = request_rx.recv() {
        let TranscriptionRequest::ProcessAudio(audio_rx, text_tx) = request;
        process_audio(&ctx, audio_rx, text_tx, language, &config);
    }
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
        let record_for_ms =
            if audio_buffer.len() < MIN_AUDIO_SAMPLES {
                let samples_needed = MIN_AUDIO_SAMPLES - audio_buffer.len();
                let samples_needed_ms = samples_needed * 1000 / SAMPLE_RATE as usize;
                std::cmp::max(samples_needed_ms, config.chunk_interval_ms)
            } else {
                config.chunk_interval_ms
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
            match detect_language(ctx, &audio_buffer) {
                Ok((detected_lang, confidence)) => {
                    if confidence >= config.language_confidence
                        || !recording {
                            println!("*** => Language detected: {} (confidence: {:.0}%)", detected_lang, confidence * 100.0);
                            language = Some(detected_lang);
                        } else {
                            eprintln!("*** => Language detection in progress");
                            continue;
                        }
                }
                Err(e) => {
                    eprintln!("*** => Language detection failed: {}, defaulting to English", e);
                    language = Some("en".to_string());
                }
            }
        }
        let lang = language.as_ref().unwrap();

        let grace_ms : usize = if recording { config.emit_grace_ms } else { 0 };
        let emit_threshold_ms = current_audio_duration_ms - grace_ms;
        eprintln!("*** => Grace ms = {}. Total_time: {}", emit_threshold_ms, total_samples as f32 / SAMPLE_RATE as f32);
        // Should be a fold to find the last value
        let last_emitted_end_ms = transcribe(ctx, &audio_buffer, lang)
            .as_iter()
            .fold(0, |acc: usize, segment : WhisperSegment| {
                let start_ms = segment.start_timestamp() as usize * 10;
                let end_ms = segment.end_timestamp() as usize * 10;
                let silence_prob = segment.no_speech_probability();
                let rms = calculate_rms(&audio_buffer, start_ms, end_ms);
                let text = segment.to_str_lossy().unwrap();
                eprintln!("*** => Segment [{} - {}] P: {}, RMS: {} : {}",
                          start_ms,
                          end_ms,
                          silence_prob,
                          rms,
                          text);

                if end_ms <= emit_threshold_ms {
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
            });

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
    let mut chunk : Vec<f32> = audio_rx.iter()
        .take(samples_needed)
        .collect();
    chunk.extend(audio_rx.try_iter());
    let closed = chunk.len() < samples_needed;
    (chunk, closed)
}

/// Detects language from audio buffer.
/// Returns (language_code, confidence).
fn detect_language(ctx: &WhisperContext, audio: &[f32]) -> Result<(String, f32), String> {
    let mut state = ctx.create_state().map_err(|e| format!("{:?}", e))?;

    // Convert to mel spectrogram first
    state.pcm_to_mel(audio, 1).map_err(|e| format!("{:?}", e))?;

    // Detect language
    let (lang_id, probs) = state.lang_detect(0, 1).map_err(|e| format!("{:?}", e))?;

    let lang_str = whisper_rs::get_lang_str(lang_id)
        .ok_or_else(|| format!("Unknown language id: {}", lang_id))?;

    let confidence = probs.get(lang_id as usize).copied().unwrap_or(0.0);

    Ok((lang_str.to_string(), confidence))
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
        .map(|s| s*s)
        .sum::<f32>();
    rs / samples as f32
}

fn no_samples(ms: usize) -> usize {
    ms * SAMPLE_RATE as usize / 1000
}
