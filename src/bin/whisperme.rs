use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use whisperme::socket::send_command;

const MODELS: &[(&str, &str)] = &[
    ("tiny", "39 MB"),
    ("tiny.en", "39 MB"),
    ("base", "147 MB"),
    ("base.en", "147 MB"),
    ("small", "488 MB"),
    ("small.en", "488 MB"),
    ("medium", "1.5 GB"),
    ("medium.en", "1.5 GB"),
    ("large-v3", "3.1 GB"),
    ("large-v3-turbo", "1.6 GB"),
    ("large-v3-turbo-q8_0", "809 MB"),
    ("large-v3-turbo-q5_0", "547 MB"),
];

const LANGUAGES: &[(&str, &str)] = &[
    ("auto", "Auto-detect"),
    ("en", "English"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("zh", "Chinese"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("pt", "Portuguese"),
    ("ru", "Russian"),
    ("it", "Italian"),
];

const POSITIONS: &[(&str, &str)] = &[
    ("top-left", "Top Left"),
    ("top-center", "Top Center"),
    ("top-right", "Top Right"),
    ("bottom-left", "Bottom Left"),
    ("bottom-center", "Bottom Center"),
    ("bottom-right", "Bottom Right"),
];

const OUTPUT_METHODS: &[(&str, &str)] = &[
    ("xdo", "Xdo (X11 typing)"),
    ("ydotool", "Ydotool (Wayland/X11)"),
    ("clipboard", "Clipboard"),
    ("print", "Print to stdout"),
];

const KEYBOARD_LAYOUTS: &[(&str, &str)] = &[
    ("auto", "Auto-detect"),
    ("us", "US"),
    ("dk", "Danish"),
    ("de", "German"),
    ("fr", "French"),
    ("es", "Spanish"),
    ("uk", "UK"),
];

const DEFAULT_CHUNK_INTERVAL_MS: usize = 1000;
const DEFAULT_EMIT_GRACE_MS: usize = 1200;
const DEFAULT_LANGUAGE_CONFIDENCE: f32 = 0.7;
const DEFAULT_SILENCE_THRESHOLD_DBFS: f32 = -60.0;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: whisperme <start|stop|toggle|status|config>");
        std::process::exit(1);
    }

    let command = &args[1];

    match command.as_str() {
        "start" | "stop" | "toggle" => {
            send_command(command);
        }
        "status" => {
            if let Some(response) = send_command(command) {
                println!("{response}");
            }
        }
        "config" => run_config_gui(),
        _ => {
            eprintln!("Unknown command: {command}");
            eprintln!("Usage: whisperme <start|stop|toggle|status|config>");
            std::process::exit(1);
        }
    }
}

fn run_config_gui() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([420.0, 580.0])
            .with_resizable(false),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "WhisperMe Configuration",
        options,
        Box::new(|_| Ok(Box::new(ConfigApp::load()))),
    );
}

struct ConfigApp {
    model: String,
    language: String,
    ui_enabled: bool,
    position: String,
    continuous_transcription: bool,
    transcription_interval_ms: usize,
    emit_grace_ms: usize,
    language_confidence: f32,
    silence_threshold_dbfs: f32,
    output_method: String,
    keyboard_layout: String,
    models_dir: PathBuf,
    download_status: Arc<Mutex<Option<String>>>,
}

impl ConfigApp {
    fn load() -> Self {
        let cfg = load_config();
        Self {
            model: cfg.model,
            language: cfg.language,
            ui_enabled: cfg.ui_enabled,
            position: cfg.position,
            continuous_transcription: cfg.continuous_transcription,
            transcription_interval_ms: cfg.transcription_interval_ms,
            emit_grace_ms: cfg.emit_grace_ms,
            language_confidence: cfg.language_confidence,
            silence_threshold_dbfs: cfg.silence_threshold_dbfs,
            output_method: cfg.output_method,
            keyboard_layout: cfg.keyboard_layout,
            models_dir: models_dir(),
            download_status: Arc::new(Mutex::new(None)),
        }
    }

    fn model_exists(&self, name: &str) -> bool {
        self.models_dir.join(format!("ggml-{}.bin", name)).exists()
    }

    fn is_english_model(&self) -> bool {
        self.model.ends_with(".en")
    }

    fn is_downloading(&self) -> bool {
        self.download_status.lock().unwrap().is_some()
    }

    fn download_text(&self) -> Option<String> {
        self.download_status.lock().unwrap().clone()
    }

    fn start_download(&self) {
        let model = self.model.clone();
        let dir = self.models_dir.clone();
        let status = self.download_status.clone();

        *status.lock().unwrap() = Some("Starting...".to_string());

        thread::spawn(move || {
            let _ = std::fs::create_dir_all(&dir);
            let url = format!(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{}.bin",
                model
            );
            let tmp = dir.join(format!("ggml-{}.bin.tmp", model));
            let dest = dir.join(format!("ggml-{}.bin", model));

            let result = (|| -> Result<(), String> {
                let resp = ureq::get(&url).call().map_err(|e| e.to_string())?;
                let total: Option<u64> = resp
                    .headers()
                    .get("Content-Length")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse().ok());

                let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;

                use std::io::{Read, Write};
                let mut reader = resp.into_body().into_reader();
                let mut buf = [0u8; 65536];
                let mut downloaded = 0u64;

                loop {
                    let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
                    if n == 0 {
                        break;
                    }
                    file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                    downloaded += n as u64;

                    let text = match total {
                        Some(t) => format!(
                            "Downloading... {}% of {} MB",
                            downloaded * 100 / t,
                            t / 1_000_000
                        ),
                        None => format!("Downloading... {} MB", downloaded / 1_000_000),
                    };
                    *status.lock().unwrap() = Some(text);
                }

                std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
                Ok(())
            })();

            if result.is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
            *status.lock().unwrap() = None;
        });
    }

    fn save_to_file(&self) {
        let lang = if self.is_english_model() {
            "en"
        } else {
            &self.language
        };
        save_config(
            &self.model,
            lang,
            self.ui_enabled,
            &self.position,
            self.continuous_transcription,
            self.transcription_interval_ms,
            self.emit_grace_ms,
            self.language_confidence,
            self.silence_threshold_dbfs,
            &self.output_method,
            &self.keyboard_layout,
        );
    }

    fn is_ydotool(&self) -> bool {
        self.output_method == "ydotool"
    }
}

impl eframe::App for ConfigApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.is_downloading() {
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("WhisperMe Configuration");
            ui.add_space(12.0);

            // Model selection
            ui.horizontal(|ui| {
                ui.label("Model:");
                let size = MODELS
                    .iter()
                    .find(|(n, _)| *n == self.model)
                    .map(|(_, s)| *s)
                    .unwrap_or("");
                egui::ComboBox::from_id_salt("model")
                    .selected_text(format!("{} ({})", self.model, size))
                    .show_ui(ui, |ui| {
                        for (name, size) in MODELS {
                            ui.selectable_value(
                                &mut self.model,
                                name.to_string(),
                                format!("{} ({})", name, size),
                            );
                        }
                    });
            });

            // Download status
            ui.horizontal(|ui| {
                ui.add_space(52.0);
                if let Some(text) = self.download_text() {
                    ui.label(text);
                } else if self.model_exists(&self.model) {
                    ui.label(egui::RichText::new("Downloaded").color(egui::Color32::GREEN));
                } else {
                    ui.label(egui::RichText::new("Not downloaded").color(egui::Color32::YELLOW));
                    if ui.button("Download").clicked() {
                        self.start_download();
                    }
                }
            });

            ui.add_space(8.0);

            // Language selection
            ui.horizontal(|ui| {
                ui.label("Language:");
                ui.add_enabled_ui(!self.is_english_model(), |ui| {
                    let display = if self.is_english_model() {
                        "English"
                    } else {
                        LANGUAGES
                            .iter()
                            .find(|(c, _)| *c == self.language)
                            .map(|(_, n)| *n)
                            .unwrap_or(&self.language)
                    };
                    egui::ComboBox::from_id_salt("lang")
                        .selected_text(display)
                        .show_ui(ui, |ui| {
                            for (code, name) in LANGUAGES {
                                ui.selectable_value(&mut self.language, code.to_string(), *name);
                            }
                        });
                });
            });

            if self.is_english_model() {
                ui.horizontal(|ui| {
                    ui.add_space(52.0);
                    ui.label(egui::RichText::new("English-only model").italics().weak());
                });
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // UI settings
            ui.checkbox(&mut self.ui_enabled, "Show recording indicator");

            ui.add_enabled_ui(self.ui_enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Position:");
                    let display = POSITIONS
                        .iter()
                        .find(|(c, _)| *c == self.position)
                        .map(|(_, n)| *n)
                        .unwrap_or(&self.position);
                    egui::ComboBox::from_id_salt("pos")
                        .selected_text(display)
                        .show_ui(ui, |ui| {
                            for (code, name) in POSITIONS {
                                ui.selectable_value(&mut self.position, code.to_string(), *name);
                            }
                        });
                });
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // Transcription settings
            ui.label(egui::RichText::new("Transcription Settings").strong());
            ui.add_space(4.0);

            ui.checkbox(&mut self.continuous_transcription, "Continuous transcription (experimental)")
                .on_hover_text("Transcribe audio periodically while recording. Lower quality than transcribing after recording ends.");

            ui.add_enabled_ui(self.continuous_transcription, |ui| {
                egui::Grid::new("transcription_grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Transcription interval (ms):")
                            .on_hover_text("How often to run transcription on buffered audio.");
                        let mut interval_val = self.transcription_interval_ms as f32;
                        ui.add(
                            egui::DragValue::new(&mut interval_val)
                                .range(100.0..=5000.0)
                                .speed(50.0),
                        );
                        self.transcription_interval_ms = interval_val as usize;
                        ui.end_row();

                        ui.label("Emit grace (ms):")
                            .on_hover_text("Delay before emitting text to avoid incomplete words.");
                        let mut grace_val = self.emit_grace_ms as f32;
                        ui.add(
                            egui::DragValue::new(&mut grace_val)
                                .range(0.0..=3000.0)
                                .speed(50.0),
                        );
                        self.emit_grace_ms = grace_val as usize;
                        ui.end_row();
                    });
            });

            egui::Grid::new("transcription_grid_common")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Language confidence:")
                        .on_hover_text("Minimum confidence for language detection.");
                    ui.add(
                        egui::DragValue::new(&mut self.language_confidence)
                            .range(0.0..=1.0)
                            .speed(0.01)
                            .fixed_decimals(2),
                    );
                    ui.end_row();

                    ui.label("Silence threshold (dBFS):").on_hover_text(
                        "RMS threshold in dBFS. Segments below this level are discarded.",
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.silence_threshold_dbfs)
                            .range(-120.0..=0.0)
                            .speed(1.0)
                            .fixed_decimals(0),
                    );
                    ui.end_row();
                });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // Output settings
            ui.label(egui::RichText::new("Text Output").strong());
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Output method:");
                let display = OUTPUT_METHODS
                    .iter()
                    .find(|(c, _)| *c == self.output_method)
                    .map(|(_, n)| *n)
                    .unwrap_or(&self.output_method);
                egui::ComboBox::from_id_salt("output")
                    .selected_text(display)
                    .show_ui(ui, |ui| {
                        for (code, name) in OUTPUT_METHODS {
                            ui.selectable_value(&mut self.output_method, code.to_string(), *name);
                        }
                    });
            });

            ui.add_enabled_ui(self.is_ydotool(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("Keyboard layout:");
                    let display = KEYBOARD_LAYOUTS
                        .iter()
                        .find(|(c, _)| *c == self.keyboard_layout)
                        .map(|(_, n)| *n)
                        .unwrap_or(&self.keyboard_layout);
                    egui::ComboBox::from_id_salt("kbd")
                        .selected_text(display)
                        .show_ui(ui, |ui| {
                            for (code, name) in KEYBOARD_LAYOUTS {
                                ui.selectable_value(
                                    &mut self.keyboard_layout,
                                    code.to_string(),
                                    *name,
                                );
                            }
                        });
                });
            });

            if !self.is_ydotool() {
                ui.horizontal(|ui| {
                    ui.add_space(52.0);
                    ui.label(
                        egui::RichText::new("Keyboard layout only applies to ydotool")
                            .italics()
                            .weak(),
                    );
                });
            }

            // Buttons
            ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Save").clicked() {
                        self.save_to_file();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        });
    }
}

fn config_path() -> PathBuf {
    let dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
        });
    dir.join("whisperme/config.ini")
}

fn models_dir() -> PathBuf {
    let dir = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
        });
    dir.join("whisperme/models")
}

struct LoadedConfig {
    model: String,
    language: String,
    ui_enabled: bool,
    position: String,
    continuous_transcription: bool,
    transcription_interval_ms: usize,
    emit_grace_ms: usize,
    language_confidence: f32,
    silence_threshold_dbfs: f32,
    output_method: String,
    keyboard_layout: String,
}

fn load_config() -> LoadedConfig {
    let path = config_path();
    let ini = ini::Ini::load_from_file(&path).unwrap_or_default();
    let w = ini.section(Some("whisper"));
    let u = ini.section(Some("ui"));
    let t = ini.section(Some("transcription"));
    let o = ini.section(Some("output"));
    LoadedConfig {
        model: w
            .and_then(|s| s.get("model"))
            .unwrap_or("base.en")
            .to_string(),
        language: w
            .and_then(|s| s.get("language"))
            .unwrap_or("auto")
            .to_string(),
        ui_enabled: u
            .and_then(|s| s.get("enabled"))
            .map(|v| v == "true")
            .unwrap_or(true),
        position: u
            .and_then(|s| s.get("position"))
            .unwrap_or("top-center")
            .to_string(),
        continuous_transcription: t
            .and_then(|s| s.get("continuous_transcription"))
            .map(|v| v == "true")
            .unwrap_or(false),
        transcription_interval_ms: t
            .and_then(|s| s.get("transcription_interval_ms"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_CHUNK_INTERVAL_MS),
        emit_grace_ms: t
            .and_then(|s| s.get("emit_grace_ms"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_EMIT_GRACE_MS),
        language_confidence: t
            .and_then(|s| s.get("language_confidence"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_LANGUAGE_CONFIDENCE),
        silence_threshold_dbfs: t
            .and_then(|s| s.get("silence_threshold_dbfs"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_SILENCE_THRESHOLD_DBFS),
        output_method: o.and_then(|s| s.get("method")).unwrap_or("xdo").to_string(),
        keyboard_layout: o
            .and_then(|s| s.get("keyboard_layout"))
            .unwrap_or("auto")
            .to_string(),
    }
}

fn save_config(
    model: &str,
    language: &str,
    ui_enabled: bool,
    position: &str,
    continuous_transcription: bool,
    transcription_interval_ms: usize,
    emit_grace_ms: usize,
    language_confidence: f32,
    silence_threshold_dbfs: f32,
    output_method: &str,
    keyboard_layout: &str,
) {
    let path = config_path();
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let mut ini = ini::Ini::new();
    ini.with_section(Some("whisper"))
        .set("model", model)
        .set("language", language);
    ini.with_section(Some("ui"))
        .set("enabled", if ui_enabled { "true" } else { "false" })
        .set("position", position);
    ini.with_section(Some("transcription"))
        .set(
            "continuous_transcription",
            if continuous_transcription { "true" } else { "false" },
        )
        .set(
            "transcription_interval_ms",
            transcription_interval_ms.to_string(),
        )
        .set("emit_grace_ms", emit_grace_ms.to_string())
        .set("language_confidence", language_confidence.to_string())
        .set("silence_threshold_dbfs", silence_threshold_dbfs.to_string());
    ini.with_section(Some("output"))
        .set("method", output_method)
        .set("keyboard_layout", keyboard_layout);
    let _ = ini.write_to_file(&path);
}
