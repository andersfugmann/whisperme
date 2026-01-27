use ini::Ini;
use std::path::PathBuf;

use crate::UnwrapOrExit;

#[derive(Debug, Clone)]
pub struct WhisperConfig {
    pub model: String,
    pub model_path: PathBuf,
    pub language: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPosition {
    BottomRight,
    BottomCenter,
    BottomLeft,
    TopRight,
    TopCenter,
    TopLeft,
}

impl std::str::FromStr for UiPosition {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "bottom-left" => Self::BottomLeft,
            "bottom-center" => Self::BottomCenter,
            "bottom-right" => Self::BottomRight,
            "top-right" => Self::TopRight,
            "top-center" => Self::TopCenter,
            "top-left" => Self::TopLeft,
            _ => Self::TopCenter,
        })
    }
}

#[derive(Debug, Clone)]
pub struct UiConfig {
    pub enabled: bool,
    pub position: UiPosition,
}

#[derive(Debug, Clone)]
pub struct TranscriptionConfig {
    /// How long to buffer audio before each transcription attempt (ms).
    pub transcription_interval_ms: usize,
    /// Segments must end this many ms before audio end to be emitted.
    pub emit_grace_ms: usize,
    /// Language detection confidence threshold (0.0 - 1.0).
    pub language_confidence: f32,
    /// RMS threshold below which segments are considered silence.
    pub silence_rms_threshold: f32,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            transcription_interval_ms: 1000,
            emit_grace_ms: 1200,
            language_confidence: 0.7,
            silence_rms_threshold: 0.00001,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub whisper: WhisperConfig,
    pub ui: UiConfig,
    pub transcription: TranscriptionConfig,
}

impl Config {
    pub fn load() -> Self {
        let config_path = Self::config_path();

        if !config_path.exists() {
            return Self::default();
        }

        let ini = Ini::load_from_file(&config_path).unwrap_or_exit(&format!(
            "failed to load config from {} - check file syntax",
            config_path.display()
        ));

        Self::from_ini(&ini)
    }

    fn config_path() -> PathBuf {
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home =
                    std::env::var("HOME").unwrap_or_exit("HOME environment variable not set");
                PathBuf::from(home).join(".config")
            });
        config_dir.join("whisperme").join("config.ini")
    }

    fn models_dir() -> PathBuf {
        let data_dir = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home =
                    std::env::var("HOME").unwrap_or_exit("HOME environment variable not set");
                PathBuf::from(home).join(".local/share")
            });
        data_dir.join("whisperme").join("models")
    }

    /// Resolve model path:
    /// - Absolute path (starts with `/`): use as-is
    /// - Relative to cwd (starts with `./`): use as-is
    /// - Otherwise: treat as model name, look in ~/.local/share/whisperme/models/ggml-{name}.bin
    fn resolve_model_path(model: &str) -> PathBuf {
        match model {
            s if s.starts_with('/') => PathBuf::from(s),
            s if s.starts_with("./") => PathBuf::from(s),
            name => Self::models_dir().join(format!("ggml-{name}.bin")),
        }
    }

    fn from_ini(ini: &Ini) -> Self {
        let whisper_section = ini.section(Some("whisper"));
        let ui_section = ini.section(Some("ui"));
        let transcription_section = ini.section(Some("transcription"));
        let defaults = TranscriptionConfig::default();

        let model = whisper_section
            .and_then(|s| s.get("model"))
            .unwrap_or("base.en")
            .to_string();

        let language = whisper_section
            .and_then(|s| s.get("language"))
            .unwrap_or("auto")
            .to_string();

        let model_path = Self::resolve_model_path(&model);

        let ui_enabled = ui_section
            .and_then(|s| s.get("enabled"))
            .map(|v| v == "true")
            .unwrap_or(true);

        let ui_position = ui_section
            .and_then(|s| s.get("position"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(UiPosition::TopCenter);

        let transcription_interval_ms = transcription_section
            .and_then(|s| s.get("transcription_interval_ms"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(defaults.transcription_interval_ms);

        let emit_grace_ms = transcription_section
            .and_then(|s| s.get("emit_grace_ms"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(defaults.emit_grace_ms);

        let language_confidence = transcription_section
            .and_then(|s| s.get("language_confidence"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(defaults.language_confidence);

        let silence_rms_threshold = transcription_section
            .and_then(|s| s.get("silence_rms_threshold"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(defaults.silence_rms_threshold);

        Self {
            whisper: WhisperConfig {
                model,
                model_path,
                language,
            },
            ui: UiConfig {
                enabled: ui_enabled,
                position: ui_position,
            },
            transcription: TranscriptionConfig {
                transcription_interval_ms,
                emit_grace_ms,
                language_confidence,
                silence_rms_threshold,
            },
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let model = "base.en".to_string();
        let model_path = Self::models_dir().join(format!("ggml-{model}.bin"));

        Self {
            whisper: WhisperConfig {
                model,
                model_path,
                language: "auto".to_string(),
            },
            ui: UiConfig {
                enabled: true,
                position: UiPosition::TopCenter,
            },
            transcription: TranscriptionConfig::default(),
        }
    }
}
