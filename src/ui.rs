//! UI module: recording indicator window with frequency visualization.
//!
//! Uses a persistent event loop architecture:
//! - The UI thread and event loop are created once at daemon startup
//! - Recording indicator windows are created/destroyed dynamically via child viewports
//! - This avoids winit's "EventLoop can't be recreated" limitation

use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use circular_buffer::CircularBuffer;
use crossbeam_channel as channel;
use eframe::egui;

use crate::audio_capture::AudioReceiver;
use crate::audio_processor::SAMPLE_RATE;
use crate::config::UiPosition;
use crate::spectrum::{FFT_SIZE, band_db_levels};

/// UI request messages
pub enum UiRequest {
    Show(AudioReceiver, UiPosition),
}

type UiSender = channel::Sender<UiRequest>;
type UiReceiver = channel::Receiver<UiRequest>;

/// Handle for sending requests to the UI thread.
/// Wakes the event loop on send so it doesn't need to poll.
#[derive(Clone)]
pub struct UiHandle {
    tx: UiSender,
    ctx: Arc<OnceLock<egui::Context>>,
}

impl UiHandle {
    pub fn show(&self, audio_rx: AudioReceiver, position: UiPosition) {
        if self.tx.send(UiRequest::Show(audio_rx, position)).is_ok() {
            if let Some(ctx) = self.ctx.get() {
                ctx.request_repaint();
            }
        }
    }
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

/// Spawn the persistent UI thread. Call once at daemon startup.
/// Returns a handle to request showing the recording indicator.
pub fn spawn(position: UiPosition) -> UiHandle {
    let (tx, rx) = channel::unbounded::<UiRequest>();
    let ctx = Arc::new(OnceLock::new());
    let ctx_clone = Arc::clone(&ctx);
    thread::spawn(move || {
        run_ui_loop(rx, ctx_clone, position);
    });
    UiHandle { tx, ctx }
}

/// Legacy API for showing a one-shot recording indicator.
/// Deprecated: prefer spawn() + UiHandle::show().
pub fn show(audio_rx: AudioReceiver, position: UiPosition) -> JoinHandle<()> {
    thread::spawn(move || {
        let (tx, rx) = channel::unbounded::<UiRequest>();
        let ctx = Arc::new(OnceLock::new());
        tx.send(UiRequest::Show(audio_rx, position)).ok();
        run_ui_loop(rx, ctx, position);
    })
}

fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
}

fn run_ui_loop(request_rx: UiReceiver, ctx_shared: Arc<OnceLock<egui::Context>>, default_position: UiPosition) {
    use eframe::EventLoopBuilderHook;

    // Allow running on non-main thread (required for our thread-based architecture)
    let event_loop_builder: EventLoopBuilderHook = Box::new(|builder| {
        match is_wayland() {
            true => winit::platform::wayland::EventLoopBuilderExtWayland::with_any_thread(builder, true),
            false => winit::platform::x11::EventLoopBuilderExtX11::with_any_thread(builder, true),
        };
    });

    // Root viewport is invisible - we only show child viewports
    let viewport = build_viewport(1.0, 1.0).with_visible(false);
    let native_options = eframe::NativeOptions {
        viewport,
        event_loop_builder: Some(event_loop_builder),
        ..Default::default()
    };

    let app = UiController::new(request_rx, ctx_shared, default_position);

    if let Err(e) = eframe::run_native(
        "WhisperMe",
        native_options,
        Box::new(move |_cc| Ok(Box::new(app))),
    ) {
        eprintln!("UI event loop error: {e}");
    }
}

/// Viewport ID for the recording indicator child window
const INDICATOR_VIEWPORT_ID: &str = "recording_indicator";

/// Main UI controller that runs the persistent event loop.
/// Creates/destroys recording indicator viewports on demand.
struct UiController {
    request_rx: UiReceiver,
    ctx_shared: Arc<OnceLock<egui::Context>>,
    /// Active recording session state, if any
    active_session: Option<Arc<Mutex<RecordingSession>>>,
}

impl UiController {
    fn new(request_rx: UiReceiver, ctx_shared: Arc<OnceLock<egui::Context>>, _default_position: UiPosition) -> Self {
        Self {
            request_rx,
            ctx_shared,
            active_session: None,
        }
    }

    fn check_requests(&mut self) {
        // Check for new show requests (non-blocking)
        match self.request_rx.try_recv() {
            Ok(UiRequest::Show(audio_rx, position)) => {
                self.active_session = Some(Arc::new(Mutex::new(RecordingSession::new(
                    audio_rx, position,
                ))));
            }
            Err(channel::TryRecvError::Empty) => {}
            Err(channel::TryRecvError::Disconnected) => {
                // Request channel closed - daemon shutting down
                self.active_session = None;
            }
        }
    }

    fn update_session(&mut self) {
        // Check if active session's audio channel is closed
        if let Some(session) = &self.active_session {
            let closed = session.lock().unwrap().channel_closed;
            if closed {
                self.active_session = None;
            }
        }
    }
}

impl eframe::App for UiController {
    fn ui(&mut self, _ui: &mut egui::Ui, _frame: &mut eframe::Frame) {}

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Publish context so UiHandle can wake us on demand
        let _ = self.ctx_shared.set(ctx.clone());

        self.check_requests();
        self.update_session();

        // No timer-based repaint when idle — UiHandle wakes us via ctx.request_repaint()

        // Show recording indicator viewport if we have an active session
        if let Some(session) = &self.active_session {
            let session_clone = Arc::clone(session);
            let viewport_builder = build_viewport(WINDOW_WIDTH, WINDOW_HEIGHT);

            ctx.show_viewport_deferred(
                egui::ViewportId::from_hash_of(INDICATOR_VIEWPORT_ID),
                viewport_builder,
                move |ctx, _class| {
                    render_recording_indicator(ctx, &session_clone);
                },
            );
        }
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

#[expect(deprecated)] // CentralPanel::show(ctx) has no non-deprecated replacement for viewport callbacks
fn render_recording_indicator(ctx: &egui::Context, session: &Arc<Mutex<RecordingSession>>) {
    let mut session = session.lock().unwrap();

    // Position window on first frame
    if !session.positioned {
        if let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size) {
            let pos = session.calculate_position(monitor_size);
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            session.positioned = true;
        }
    }

    // Process incoming audio
    session.process_audio();

    // Close viewport if channel is closed
    if session.channel_closed {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        return;
    }

    // Request repaint at ~30 FPS
    ctx.request_repaint_after(std::time::Duration::from_millis(33));

    // Calculate pulse opacity for recording dot
    let elapsed = session.start_time.elapsed().as_secs_f32();
    let pulse_phase = (elapsed / DOT_PULSE_PERIOD) * 2.0 * std::f32::consts::PI;
    let pulse_opacity = 0.5 + 0.5 * pulse_phase.sin();

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
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
