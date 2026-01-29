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
pub enum OutputMethod {
    Xdo,
    Clipboard,
    Ydotool,
    Print,
}

impl std::str::FromStr for OutputMethod {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "xdo" => Self::Xdo,
            "clipboard" => Self::Clipboard,
            "ydotool" => Self::Ydotool,
            "print" => Self::Print,
            _ => Self::default(),
        })
    }
}

impl Default for OutputMethod {
    fn default() -> Self {
        #[cfg(feature = "xdo")]
        return Self::Xdo;
        #[cfg(not(feature = "xdo"))]
        return Self::Print;
    }
}

#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub method: OutputMethod,
    /// Keyboard layout for ydotool (e.g., "us", "dk", "de"). "auto" detects from system.
    pub keyboard_layout: String,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            method: OutputMethod::default(),
            keyboard_layout: "auto".to_string(),
        }
    }
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
    /// RMS threshold in dBFS. Segments with RMS below this level are discarded.
    pub silence_threshold_dbfs: f32,
    /// Precomputed linear RMS threshold (10^(dbfs/20))
    pub silence_rms_threshold: f32,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        let silence_threshold_dbfs = -80.0;
        Self {
            transcription_interval_ms: 1000,
            emit_grace_ms: 1200,
            language_confidence: 0.7,
            silence_threshold_dbfs,
            silence_rms_threshold: 10.0_f32.powf(silence_threshold_dbfs / 20.0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub whisper: WhisperConfig,
    pub ui: UiConfig,
    pub transcription: TranscriptionConfig,
    pub output: OutputConfig,
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
        let output_section = ini.section(Some("output"));
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

        let silence_threshold_dbfs = transcription_section
            .and_then(|s| s.get("silence_threshold_dbfs"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(defaults.silence_threshold_dbfs);

        let output_method = output_section
            .and_then(|s| s.get("method"))
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();

        let keyboard_layout = output_section
            .and_then(|s| s.get("keyboard_layout"))
            .unwrap_or("auto")
            .to_string();

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
                silence_threshold_dbfs,
                silence_rms_threshold: 10.0_f32.powf(silence_threshold_dbfs / 20.0),
            },
            output: OutputConfig {
                method: output_method,
                keyboard_layout,
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
            output: OutputConfig::default(),
        }
    }
}
