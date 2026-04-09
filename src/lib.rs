pub mod audio_capture;
pub mod audio_processor;
pub mod config;
pub mod fanout;
pub mod injection;
pub mod socket;
pub mod spectrum;
pub mod transcription;
pub mod recording_indicator;

#[cfg(feature = "ydotool")]
pub mod ydotool;
