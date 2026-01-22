//! UI module: recording indicator window with frequency visualization.

use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use circular_buffer::CircularBuffer;
use eframe::egui;
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};
use spectrum_analyzer::windows::hann_window;
use spectrum_analyzer::scaling::divide_by_N_sqrt;

use crate::audio::{AudioReceiver, SAMPLE_RATE};
use crate::config::UiPosition;

/// UI request messages
pub enum UiRequest {
    Show(AudioReceiver),
}

pub type UiSender = mpsc::Sender<UiRequest>;
pub type UiReceiver = mpsc::Receiver<UiRequest>;

/// FFT size for frequency analysis (must be power of 2)
const FFT_SIZE: usize = 256;

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

/// Frequency bands for each bar (Hz ranges)
const FREQ_BANDS: [(f32, f32); BAR_COUNT] = [
    (62.0, 250.0),    // Low bass
    (250.0, 500.0),   // Voice body
    (500.0, 2000.0),  // Voice clarity
    (2000.0, 4000.0), // Presence
    (4000.0, 8000.0), // Air/brightness
];

/// Spawn the UI thread that waits for Show requests.
pub fn spawn(position: UiPosition) -> UiSender {
    let (tx, rx) = mpsc::channel::<UiRequest>();

    thread::spawn(move || {
        run_ui_thread(rx, position);
    });

    tx
}

fn run_ui_thread(rx: UiReceiver, position: UiPosition) {
    while let Ok(UiRequest::Show(audio_rx)) = rx.recv() {
        run_ui_window(audio_rx, position);
    }
}

fn run_ui_window(audio_rx: AudioReceiver, position: UiPosition) {
    use eframe::EventLoopBuilderHook;
    use winit::platform::wayland::EventLoopBuilderExtWayland;

    // Allow running on non-main thread (required for our thread-based architecture)
    let event_loop_builder: EventLoopBuilderHook = Box::new(|builder| {
        builder.with_any_thread(true);
    });

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_window_type(egui::X11WindowType::Splash)
            .with_inner_size([WINDOW_WIDTH, WINDOW_HEIGHT])
            .with_decorations(false)
            .with_always_on_top()
            .with_titlebar_shown(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(true)
            .with_close_button(false)
            ,
        event_loop_builder: Some(event_loop_builder),
        ..Default::default()
    };

    let app = RecordingIndicator::new(audio_rx, position);

    if let Err(e) = eframe::run_native(
        "WhisperMe",
        native_options,
        Box::new(move |_cc| {
            Ok(Box::new(app))
        }),
    ) {
        eprintln!("UI window error: {e}");
    }
}

struct RecordingIndicator {
    audio_rx: AudioReceiver,
    position: UiPosition,
    start_time: Instant,
    sample_buffer: CircularBuffer<FFT_SIZE, f32>,
    bar_heights: [f32; BAR_COUNT],
    channel_closed: bool,
    positioned: bool,
}

impl RecordingIndicator {
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
        // Drain available samples from the audio receiver into the ring buffer
        loop {
            match self.audio_rx.try_recv() {
                Ok(sample) => {
                    self.sample_buffer.push_back(sample);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.channel_closed = true;
                    break;
                }
            }
        }

        // Run FFT if buffer is full
        if self.sample_buffer.is_full() {
            self.run_fft();
        }
    }

    fn run_fft(&mut self) {
        // Convert circular buffer to contiguous array for FFT
        let samples: [f32; FFT_SIZE] = std::array::from_fn(|i| self.sample_buffer[i]);

        // Apply Hanning window
        let windowed = hann_window(&samples);

        // Get frequency spectrum using spectrum-analyzer
        let spectrum = match samples_fft_to_spectrum(
            &windowed,
            SAMPLE_RATE,
            FrequencyLimit::Range(62.0.into(), 8000.0.into()),
            Some(&divide_by_N_sqrt),
        ) {
            Ok(s) => s,
            Err(_) => return, // Skip this frame on error
        };

        // Calculate magnitude for each frequency band
        let new_heights: Vec<f32> = FREQ_BANDS.iter().enumerate().map(|(bar_idx, &(low_hz, high_hz))| {
            // Sum magnitudes in frequency range
            let (magnitude_sum, count) = spectrum.data().iter().fold((0.0f32, 0u32), |(sum, cnt), (freq, val)| {
                let freq_hz = freq.val();
                if freq_hz >= low_hz && freq_hz < high_hz {
                    (sum + val.val(), cnt + 1)
                } else {
                    (sum, cnt)
                }
            });

            let avg_magnitude = if count > 0 {
                magnitude_sum / count as f32
            } else {
                0.0
            };

            // Convert to dB scale
            let db = if avg_magnitude > 0.0 {
                20.0 * avg_magnitude.log10()
            } else {
                -60.0
            };

            // Normalize to 0-1 range (assuming -60dB to 0dB range)
            let normalized = ((db + 60.0) / 60.0).clamp(0.0, 1.0);

            // Map to pixel height
            let target_height = BAR_MIN_HEIGHT + normalized * (BAR_MAX_HEIGHT - BAR_MIN_HEIGHT);

            // Apply exponential smoothing
            self.bar_heights[bar_idx] * (1.0 - SMOOTHING_ALPHA) + target_height * SMOOTHING_ALPHA
        }).collect();

        // Update bar heights
        self.bar_heights.copy_from_slice(&new_heights);
    }

    fn calculate_position(&self, monitor_size: egui::Vec2) -> egui::Pos2 {
        let screen_w = monitor_size.x;
        let screen_h = monitor_size.y;

        match self.position {
            UiPosition::TopLeft => egui::pos2(SCREEN_MARGIN, SCREEN_MARGIN),
            UiPosition::TopCenter => egui::pos2(
                (screen_w - WINDOW_WIDTH) / 2.0,
                SCREEN_MARGIN,
            ),
            UiPosition::TopRight => egui::pos2(
                screen_w - WINDOW_WIDTH - SCREEN_MARGIN,
                SCREEN_MARGIN,
            ),
            UiPosition::BottomLeft => egui::pos2(
                SCREEN_MARGIN,
                screen_h - WINDOW_HEIGHT - SCREEN_MARGIN,
            ),
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

impl eframe::App for RecordingIndicator {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Position window on first frame
        if !self.positioned {
            if let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size) {
                let pos = self.calculate_position(monitor_size);
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                self.positioned = true;
            }
        }

        // Process incoming audio
        self.process_audio();

        // Close window if channel is closed
        if self.channel_closed {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Request repaint at ~30 FPS
        ctx.request_repaint_after(std::time::Duration::from_millis(33));

        // Calculate pulse opacity for recording dot
        let elapsed = self.start_time.elapsed().as_secs_f32();
        let pulse_phase = (elapsed / DOT_PULSE_PERIOD) * 2.0 * std::f32::consts::PI;
        let pulse_opacity = 0.5 + 0.5 * pulse_phase.sin(); // 0.5 to 1.0

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let painter = ui.painter();
                let rect = ui.available_rect_before_wrap();

                // Draw rounded background
                painter.rect_filled(
                    rect,
                    CORNER_RADIUS,
                    bg_color(),
                );

                // Draw recording dot (pulsing)
                let dot_center = egui::pos2(
                    rect.left() + PADDING + DOT_RADIUS,
                    rect.center().y,
                );
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

                self.bar_heights.iter().enumerate().for_each(|(i, &height)| {
                    let bar_x = bars_start_x + i as f32 * (BAR_WIDTH + BAR_GAP);
                    let bar_rect = egui::Rect::from_center_size(
                        egui::pos2(bar_x + BAR_WIDTH / 2.0, bars_center_y),
                        egui::vec2(BAR_WIDTH, height),
                    );
                    painter.rect_filled(bar_rect, BAR_CORNER_RADIUS, bar_color());
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freq_bands_coverage() {
        // Verify frequency bands don't overlap and are in order
        for window in FREQ_BANDS.windows(2) {
            assert!(window[0].1 <= window[1].0,
                "Bands should not overlap: {:?} and {:?}",
                window[0], window[1]);
        }

        // Verify last band doesn't exceed Nyquist (SAMPLE_RATE / 2)
        assert!(FREQ_BANDS[BAR_COUNT - 1].1 <= (SAMPLE_RATE / 2) as f32,
            "Last band should not exceed Nyquist frequency");
    }

    #[test]
    fn test_smoothing_alpha_valid() {
        assert!(SMOOTHING_ALPHA > 0.0 && SMOOTHING_ALPHA <= 1.0);
    }
}
