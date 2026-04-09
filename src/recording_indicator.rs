//! Recording indicator: frequency visualization overlay during recording.
//!
//! A persistent UI thread runs a winit event loop. It sleeps with zero CPU
//! when idle (ControlFlow::Wait), and wakes via EventLoopProxy when a new
//! recording starts. Rendering uses tiny-skia (CPU) + softbuffer (present).

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use circular_buffer::CircularBuffer;
use crossbeam_channel as channel;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

use crate::audio_capture::AudioReceiver;
use crate::audio_processor::SAMPLE_RATE;
use crate::config::UiPosition;
use crate::spectrum::{FFT_SIZE, band_db_levels};

/// Event sent from Handle to the UI thread.
enum UserEvent {
    NewSession(AudioReceiver, UiPosition),
}

/// Handle for sending recording sessions to the persistent UI thread.
#[derive(Clone)]
pub struct Handle {
    proxy: EventLoopProxy<UserEvent>,
}

impl Handle {
    pub fn start(&self, audio_rx: AudioReceiver, position: UiPosition) {
        let _ = self.proxy.send_event(UserEvent::NewSession(audio_rx, position));
    }
}

/// Spawn the persistent UI thread. Call once at daemon startup.
pub fn spawn() -> Handle {
    let (proxy_tx, proxy_rx) = channel::bounded(1);

    std::thread::spawn(move || {
        let mut builder = EventLoop::<UserEvent>::with_user_event();

        match is_wayland() {
            true => {
                use winit::platform::wayland::EventLoopBuilderExtWayland;
                builder.with_any_thread(true);
            }
            false => {
                use winit::platform::x11::EventLoopBuilderExtX11;
                builder.with_any_thread(true);
            }
        }

        let event_loop = builder.build().expect("failed to create event loop");
        proxy_tx.send(event_loop.create_proxy()).unwrap();
        let mut app = App::new();
        event_loop.run_app(&mut app).expect("event loop error");
    });

    let proxy = proxy_rx.recv().expect("UI thread failed to start");
    Handle { proxy }
}

/// Window dimensions
const WINDOW_WIDTH: u32 = 50;
const WINDOW_HEIGHT: u32 = 32;
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

/// Background color (RGBA)
const BG_COLOR: (u8, u8, u8, u8) = (0x6A, 0x6E, 0x72, 0xF0);
/// Recording dot color (RGB)
const DOT_COLOR: (u8, u8, u8) = (0xFF, 0x20, 0x20);
/// Bar color (RGB)
const BAR_COLOR: (u8, u8, u8) = (0xF0, 0xF0, 0xF4);

/// Frame interval (~30 fps)
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
}

struct App {
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    session: Option<RecordingSession>,
}

impl App {
    fn new() -> Self {
        Self {
            window: None,
            surface: None,
            session: None,
        }
    }

    fn show_window(&self, session: &RecordingSession) {
        let Some(window) = &self.window else { return };

        // Position based on monitor size (in logical coordinates)
        if let Some(monitor) = window.current_monitor() {
            let scale = window.scale_factor();
            let monitor_size = monitor.size();
            let logical_w = monitor_size.width as f64 / scale;
            let logical_h = monitor_size.height as f64 / scale;
            let pos = calculate_position(
                session.position,
                logical_w as f32,
                logical_h as f32,
            );
            window.set_outer_position(LogicalPosition::new(pos.0, pos.1));
        }

        window.set_visible(true);
        window.request_redraw();
    }

    fn hide_window(&self) {
        if let Some(window) = &self.window {
            window.set_visible(false);
        }
    }

    fn render(&mut self) {
        let Some(session) = &self.session else { return };
        let Some(surface) = &mut self.surface else {
            return;
        };
        let Some(window) = &self.window else { return };

        // Render at the actual physical pixel size (accounts for HiDPI scaling)
        let physical: PhysicalSize<u32> = window.inner_size();
        let width = physical.width;
        let height = physical.height;
        let scale = window.scale_factor() as f32;

        let Some(mut pixmap) = tiny_skia::Pixmap::new(width, height) else {
            return;
        };

        // Scale all drawing to match physical pixels
        let transform = tiny_skia::Transform::from_scale(scale, scale);

        // Draw background rounded rect (in logical coordinates)
        let bg_path = rounded_rect_path(
            0.0,
            0.0,
            WINDOW_WIDTH as f32,
            WINDOW_HEIGHT as f32,
            CORNER_RADIUS,
        );
        let mut bg_paint = tiny_skia::Paint::default();
        bg_paint.set_color_rgba8(BG_COLOR.0, BG_COLOR.1, BG_COLOR.2, BG_COLOR.3);
        bg_paint.anti_alias = true;
        pixmap.fill_path(&bg_path, &bg_paint, tiny_skia::FillRule::Winding, transform, None);

        // Draw pulsing recording dot
        let elapsed = session.start_time.elapsed().as_secs_f32();
        let pulse_phase = (elapsed / DOT_PULSE_PERIOD) * 2.0 * std::f32::consts::PI;
        let pulse_opacity = 0.5 + 0.5 * pulse_phase.sin();

        let dot_cx = PADDING + DOT_RADIUS;
        let dot_cy = WINDOW_HEIGHT as f32 / 2.0;
        let dot_path = circle_path(dot_cx, dot_cy, DOT_RADIUS);
        let mut dot_paint = tiny_skia::Paint::default();
        dot_paint.set_color_rgba8(
            DOT_COLOR.0,
            DOT_COLOR.1,
            DOT_COLOR.2,
            (pulse_opacity * 255.0) as u8,
        );
        dot_paint.anti_alias = true;
        pixmap.fill_path(&dot_path, &dot_paint, tiny_skia::FillRule::Winding, transform, None);

        // Draw frequency bars
        let bars_start_x = dot_cx + DOT_RADIUS + PADDING;
        let bars_center_y = WINDOW_HEIGHT as f32 / 2.0;
        let mut bar_paint = tiny_skia::Paint::default();
        bar_paint.set_color_rgba8(BAR_COLOR.0, BAR_COLOR.1, BAR_COLOR.2, 0xFF);
        bar_paint.anti_alias = true;

        session
            .bar_heights
            .iter()
            .enumerate()
            .for_each(|(i, &bar_height)| {
                let bar_x = bars_start_x + i as f32 * (BAR_WIDTH + BAR_GAP);
                let bar_y = bars_center_y - bar_height / 2.0;
                let bar_path =
                    rounded_rect_path(bar_x, bar_y, BAR_WIDTH, bar_height, BAR_CORNER_RADIUS);
                pixmap.fill_path(
                    &bar_path,
                    &bar_paint,
                    tiny_skia::FillRule::Winding,
                    transform,
                    None,
                );
            });

        // Present via softbuffer
        let Ok(()) = surface.resize(
            NonZeroU32::new(width).unwrap(),
            NonZeroU32::new(height).unwrap(),
        ) else {
            return;
        };
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };

        // Copy tiny-skia RGBA pixels to softbuffer ARGB u32 format
        pixmap
            .pixels()
            .iter()
            .enumerate()
            .for_each(|(i, pixel)| {
                buffer[i] = u32::from_be_bytes([
                    pixel.alpha(),
                    pixel.red(),
                    pixel.green(),
                    pixel.blue(),
                ]);
            });

        let _ = buffer.present();
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let mut attrs = WindowAttributes::default()
            .with_inner_size(LogicalSize::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32))
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_visible(false)
            .with_window_level(WindowLevel::AlwaysOnTop);

        // X11: set splash window type to avoid taskbar/focus
        if !is_wayland() {
            use winit::platform::x11::WindowAttributesExtX11;
            attrs = attrs.with_x11_window_type(vec![winit::platform::x11::WindowType::Splash]);
        }

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        let _ = window.set_cursor_hittest(false);

        let context =
            softbuffer::Context::new(window.clone()).expect("failed to create softbuffer context");
        let surface = softbuffer::Surface::new(&context, window.clone())
            .expect("failed to create softbuffer surface");

        self.window = Some(window);
        self.surface = Some(surface);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::NewSession(audio_rx, position) => {
                let session = RecordingSession::new(audio_rx, position);
                self.show_window(&session);
                self.session = Some(session);
            }
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::RedrawRequested => {
                if let Some(session) = &mut self.session {
                    session.process_audio();

                    if session.channel_closed {
                        self.session = None;
                        self.hide_window();
                        return;
                    }

                    self.render();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match &self.session {
            Some(_) => {
                event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + FRAME_INTERVAL));
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            None => {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        }
    }
}

// --- Geometry helpers ---

fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> tiny_skia::Path {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish().expect("invalid rounded rect path")
}

fn circle_path(cx: f32, cy: f32, r: f32) -> tiny_skia::Path {
    let mut pb = tiny_skia::PathBuilder::new();
    pb.push_circle(cx, cy, r);
    pb.finish().expect("invalid circle path")
}

fn calculate_position(position: UiPosition, screen_w: f32, screen_h: f32) -> (f32, f32) {
    let w = WINDOW_WIDTH as f32;
    let h = WINDOW_HEIGHT as f32;
    match position {
        UiPosition::TopLeft => (SCREEN_MARGIN, SCREEN_MARGIN),
        UiPosition::TopCenter => ((screen_w - w) / 2.0, SCREEN_MARGIN),
        UiPosition::TopRight => (screen_w - w - SCREEN_MARGIN, SCREEN_MARGIN),
        UiPosition::BottomLeft => (SCREEN_MARGIN, screen_h - h - SCREEN_MARGIN),
        UiPosition::BottomCenter => ((screen_w - w) / 2.0, screen_h - h - SCREEN_MARGIN),
        UiPosition::BottomRight => (screen_w - w - SCREEN_MARGIN, screen_h - h - SCREEN_MARGIN),
    }
}

// --- Recording session ---

struct RecordingSession {
    audio_rx: AudioReceiver,
    position: UiPosition,
    start_time: Instant,
    sample_buffer: CircularBuffer<FFT_SIZE, f32>,
    bar_heights: [f32; BAR_COUNT],
    channel_closed: bool,
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
            self.bar_heights[i] =
                self.bar_heights[i] * (1.0 - SMOOTHING_ALPHA) + target * SMOOTHING_ALPHA;
        });
    }
}
