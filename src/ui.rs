//! UI module: recording indicator window with frequency visualization.
//!
//! Creates a fresh egui event loop per recording session.
//! The UI thread and all associated resources (wgpu, GPU context) are
//! created when recording starts and destroyed when it stops,
//! keeping the idle daemon lightweight.

use std::thread::JoinHandle;
use std::time::Instant;

use circular_buffer::CircularBuffer;
use crossbeam_channel as channel;
use eframe::egui;

use crate::audio_capture::AudioReceiver;
use crate::audio_processor::SAMPLE_RATE;
use crate::config::UiPosition;
use crate::spectrum::{FFT_SIZE, band_db_levels};

/// Show the recording indicator for the duration of the audio stream.
/// Spawns a UI thread that runs until the audio channel closes.
/// Returns a join handle for the UI thread.
pub fn show(audio_rx: AudioReceiver, position: UiPosition) -> JoinHandle<()> {
    std::thread::spawn(move || {
        run_ui(audio_rx, position);
    })
}

/// Window dimensions
const WINDOW_WIDTH: f32 = 50.0;
const WINDOW_HEIGHT: f32 = 32.0;
const CORNER_RADIUS: f32 = 10.0;
const PADDING: f32 = 6.0;

/// Recording dot properties
const DOT_RADIUS: f32 = 3.0;
const DOT_PULSE_PERIOD: f32 = 1.5;

/// Frequency bar properties
const BAR_COUNT: usize = 5;
const BAR_WIDTH: f32 = 3.0;
const BAR_GAP: f32 = 2.0;
const BAR_MIN_HEIGHT: f32 = 3.0;
const BAR_MAX_HEIGHT: f32 = 18.0;
const BAR_CORNER_RADIUS: f32 = 1.5;

/// Smoothing factor for exponential moving average
const SMOOTHING_ALPHA: f32 = 0.3;

/// Screen margin
const SCREEN_MARGIN: f32 = 16.0;

/// Background opacity (0 = fully transparent, 255 = fully opaque)
const BG_OPACITY: u8 = 240;

/// Colors
fn bg_color() -> egui::Color32 {
    // Aluminium grey (slightly darker)
    egui::Color32::from_rgba_unmultiplied(0x6A, 0x6E, 0x72, BG_OPACITY)
}
fn dot_color() -> egui::Color32 {
    // Brighter red
    egui::Color32::from_rgb(0xFF, 0x20, 0x20)
}
fn bar_color() -> egui::Color32 {
    // Silver white
    egui::Color32::from_rgb(0xF0, 0xF0, 0xF4)
}

fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
}

fn run_ui(audio_rx: AudioReceiver, position: UiPosition) {
    use eframe::EventLoopBuilderHook;

    let event_loop_builder: EventLoopBuilderHook = Box::new(|builder| {
        match is_wayland() {
            true => winit::platform::wayland::EventLoopBuilderExtWayland::with_any_thread(builder, true),
            false => winit::platform::x11::EventLoopBuilderExtX11::with_any_thread(builder, true),
        };
    });

    let viewport = build_viewport(WINDOW_WIDTH, WINDOW_HEIGHT);
    let native_options = eframe::NativeOptions {
        viewport,
        event_loop_builder: Some(event_loop_builder),
        ..Default::default()
    };

    let app = RecordingIndicator::new(audio_rx, position);

    if let Err(e) = eframe::run_native(
        "WhisperMe",
        native_options,
        Box::new(move |_cc| Ok(Box::new(app))),
    ) {
        eprintln!("UI error: {e}");
    }
}

/// Recording indicator app. Renders directly in the root viewport.
/// Closes when the audio channel disconnects.
struct RecordingIndicator {
    session: RecordingSession,
}

impl RecordingIndicator {
    fn new(audio_rx: AudioReceiver, position: UiPosition) -> Self {
        Self {
            session: RecordingSession::new(audio_rx, position),
        }
    }
}

impl eframe::App for RecordingIndicator {
    fn ui(&mut self, _ui: &mut egui::Ui, _frame: &mut eframe::Frame) {}

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.session.process_audio();

        if self.session.channel_closed {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Position window on first frame
        if !self.session.positioned {
            if let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size) {
                let pos = self.session.calculate_position(monitor_size);
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                self.session.positioned = true;
            }
        }

        // Request repaint at ~30 FPS for animation
        ctx.request_repaint_after(std::time::Duration::from_millis(33));

        let elapsed = self.session.start_time.elapsed().as_secs_f32();
        let pulse_phase = (elapsed / DOT_PULSE_PERIOD) * 2.0 * std::f32::consts::PI;
        let pulse_opacity = 0.5 + 0.5 * pulse_phase.sin();

        #[expect(deprecated)] // CentralPanel::show(ctx) has no non-deprecated replacement yet
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                render_bars(ui, &self.session, pulse_opacity);
            });
    }
}

fn build_viewport(width: f32, height: f32) -> egui::ViewportBuilder {
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([width, height])
        .with_decorations(false)
        .with_always_on_top()
        .with_titlebar_shown(false)
        .with_resizable(false)
        .with_transparent(true)
        .with_mouse_passthrough(true)
        .with_close_button(false);

    // X11-specific: set splash window type to avoid taskbar/focus
    match is_wayland() {
        false => viewport.with_window_type(egui::X11WindowType::Splash),
        true => viewport,
    }
}

fn render_bars(ui: &mut egui::Ui, session: &RecordingSession, pulse_opacity: f32) {
    let painter = ui.painter();
    let rect = ui.available_rect_before_wrap();

    // Draw rounded background
    painter.rect_filled(rect, CORNER_RADIUS, bg_color());

    // Draw recording dot (pulsing)
    let dot_center = egui::pos2(rect.left() + PADDING + DOT_RADIUS, rect.center().y);
    let base_dot = dot_color();
    let pulsing_dot = egui::Color32::from_rgba_unmultiplied(
        base_dot.r(),
        base_dot.g(),
        base_dot.b(),
        (pulse_opacity * 255.0) as u8,
    );
    painter.circle_filled(dot_center, DOT_RADIUS, pulsing_dot);

    // Draw frequency bars
    let bars_start_x = dot_center.x + DOT_RADIUS + PADDING;
    let bars_center_y = rect.center().y;

    session
        .bar_heights
        .iter()
        .enumerate()
        .for_each(|(i, &height)| {
            let bar_x = bars_start_x + i as f32 * (BAR_WIDTH + BAR_GAP);
            let bar_rect = egui::Rect::from_center_size(
                egui::pos2(bar_x + BAR_WIDTH / 2.0, bars_center_y),
                egui::vec2(BAR_WIDTH, height),
            );
            painter.rect_filled(bar_rect, BAR_CORNER_RADIUS, bar_color());
        });
}

/// State for an active recording session
struct RecordingSession {
    audio_rx: AudioReceiver,
    position: UiPosition,
    start_time: Instant,
    sample_buffer: CircularBuffer<FFT_SIZE, f32>,
    bar_heights: [f32; BAR_COUNT],
    channel_closed: bool,
    positioned: bool,
}

impl RecordingSession {
    fn new(audio_rx: AudioReceiver, position: UiPosition) -> Self {
        Self {
            audio_rx,
            position,
            start_time: Instant::now(),
            sample_buffer: CircularBuffer::new(),
            bar_heights: [BAR_MIN_HEIGHT; BAR_COUNT],
            channel_closed: false,
            positioned: false,
        }
    }

    fn process_audio(&mut self) {
        loop {
            match self.audio_rx.try_recv() {
                Ok(sample) => {
                    self.sample_buffer.push_back(sample);
                }
                Err(channel::TryRecvError::Empty) => break,
                Err(channel::TryRecvError::Disconnected) => {
                    self.channel_closed = true;
                    break;
                }
            }
        }

        if self.sample_buffer.is_full() {
            self.run_fft();
        }
    }

    fn run_fft(&mut self) {
        let samples: [f32; FFT_SIZE] = std::array::from_fn(|i| self.sample_buffer[i]);
        let db_levels = match band_db_levels(&samples, SAMPLE_RATE) {
            Some(levels) => levels,
            None => return,
        };

        db_levels.iter().enumerate().for_each(|(i, &db)| {
            let normalized = ((db + 80.0) / 100.0).clamp(0.0, 1.0).powi(2);
            let target = BAR_MIN_HEIGHT + normalized * (BAR_MAX_HEIGHT - BAR_MIN_HEIGHT);
            self.bar_heights[i] = self.bar_heights[i] * (1.0 - SMOOTHING_ALPHA) + target * SMOOTHING_ALPHA;
        });
    }

    fn calculate_position(&self, monitor_size: egui::Vec2) -> egui::Pos2 {
        let screen_w = monitor_size.x;
        let screen_h = monitor_size.y;

        match self.position {
            UiPosition::TopLeft => egui::pos2(SCREEN_MARGIN, SCREEN_MARGIN),
            UiPosition::TopCenter => egui::pos2((screen_w - WINDOW_WIDTH) / 2.0, SCREEN_MARGIN),
            UiPosition::TopRight => {
                egui::pos2(screen_w - WINDOW_WIDTH - SCREEN_MARGIN, SCREEN_MARGIN)
            }
            UiPosition::BottomLeft => {
                egui::pos2(SCREEN_MARGIN, screen_h - WINDOW_HEIGHT - SCREEN_MARGIN)
            }
            UiPosition::BottomCenter => egui::pos2(
                (screen_w - WINDOW_WIDTH) / 2.0,
                screen_h - WINDOW_HEIGHT - SCREEN_MARGIN,
            ),
            UiPosition::BottomRight => egui::pos2(
                screen_w - WINDOW_WIDTH - SCREEN_MARGIN,
                screen_h - WINDOW_HEIGHT - SCREEN_MARGIN,
            ),
        }
    }
}
