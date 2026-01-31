//! Ydotool virtual keyboard with xkbcommon-based keymap support.
//!
//! Connects to ydotoold socket and sends input events.
//! Detects keyboard layout from system and uses xkbcommon to map characters to keycodes.
//! Requires ydotoold daemon running and socket access.

use std::collections::HashMap;
use std::io;
use std::os::unix::net::UnixDatagram;
use std::process::Command;
use std::thread;
use std::time::Duration;

use xkbcommon::xkb::{self, Keycode, Keysym};

// Linux input event constants
const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const SYN_REPORT: u16 = 0x00;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_LEFTCTRL: u16 = 29;
const KEY_V: u16 = 47;

/// Linux input_event struct (matches kernel definition)
#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    tv_sec: i64,
    tv_usec: i64,
    type_: u16,
    code: u16,
    value: i32,
}

impl InputEvent {
    /// Serialize to bytes (little-endian, matching native Linux layout)
    fn to_bytes(&self) -> [u8; 24] {
        let mut buf = [0u8; 24];
        buf[0..8].copy_from_slice(&self.tv_sec.to_ne_bytes());
        buf[8..16].copy_from_slice(&self.tv_usec.to_ne_bytes());
        buf[16..18].copy_from_slice(&self.type_.to_ne_bytes());
        buf[18..20].copy_from_slice(&self.code.to_ne_bytes());
        buf[20..24].copy_from_slice(&self.value.to_ne_bytes());
        buf
    }
}

/// Virtual keyboard that sends keystrokes via ydotoold socket.
pub struct VirtualKeyboard {
    socket: UnixDatagram,
    keymap: HashMap<char, (u16, bool)>, // char -> (linux_keycode, needs_shift)
}

impl VirtualKeyboard {
    /// Create a new virtual keyboard connecting to ydotoold.
    pub fn new(keyboard_layout: &str) -> Result<Self, String> {
        let socket_path = Self::find_socket_path();
        let socket =
            UnixDatagram::unbound().map_err(|e| format!("failed to create socket: {}", e))?;
        socket
            .connect(&socket_path)
            .map_err(|e| format!("failed to connect to ydotoold at '{}': {}", socket_path, e))?;

        let layout = match keyboard_layout {
            "auto" => detect_keyboard_layout(),
            s => s.to_string(),
        };

        eprintln!("ydotool: using keyboard layout '{}'", layout);
        let keymap = build_xkb_keymap(&layout)?;

        Ok(Self { socket, keymap })
    }

    /// Find ydotoold socket path.
    fn find_socket_path() -> String {
        if let Ok(xrd) = std::env::var("XDG_RUNTIME_DIR") {
            let path = format!("{}/.ydotool_socket", xrd);
            if std::path::Path::new(&path).exists() {
                return path;
            }
        }
        "/tmp/.ydotool_socket".to_string()
    }

    /// Type a string by sending keystrokes to ydotoold.
    pub fn type_text(&self, text: &str) {
        text.chars().for_each(|c| {
            if let Some(&(keycode, shift)) = self.keymap.get(&c) {
                if shift {
                    self.send_key(KEY_LEFTSHIFT, 1);
                }
                self.send_key(keycode, 1);
                thread::sleep(Duration::from_millis(1));
                self.send_key(keycode, 0);

                if shift {
                    self.send_key(KEY_LEFTSHIFT, 0);
                }
                thread::sleep(Duration::from_millis(1));
            }
        });
    }

    /// Send Ctrl+V key combination (for pasting from clipboard).
    pub fn send_paste(&self) {
        self.send_key(KEY_LEFTCTRL, 1);
        self.send_key(KEY_V, 1);
        thread::sleep(Duration::from_millis(1));
        self.send_key(KEY_V, 0);
        self.send_key(KEY_LEFTCTRL, 0);
        thread::sleep(Duration::from_millis(1));
    }

    /// Send a single key event to ydotoold.
    fn send_key(&self, code: u16, value: i32) {
        let key_event = InputEvent {
            tv_sec: 0,
            tv_usec: 0,
            type_: EV_KEY,
            code,
            value,
        };
        let syn_event = InputEvent {
            tv_sec: 0,
            tv_usec: 0,
            type_: EV_SYN,
            code: SYN_REPORT,
            value: 0,
        };

        let _ = self.send_event(&key_event);
        let _ = self.send_event(&syn_event);
    }

    /// Send raw input_event to socket.
    fn send_event(&self, event: &InputEvent) -> io::Result<()> {
        self.socket.send(&event.to_bytes())?;
        Ok(())
    }
}

fn query_env() -> Option<String> {
    std::env::var("XKB_DEFAULT_LAYOUT").ok()
}

/// Detect keyboard layout from system.
fn detect_keyboard_layout() -> String {
    [query_env, query_localectl, query_setxkbmap]
        .iter()
        .find_map(|f| f().filter(|s| !s.is_empty()))
        .unwrap_or("us".to_string())
}

/// Query localectl for keyboard layout.
fn query_localectl() -> Option<String> {
    let output = Command::new("localectl").arg("status").output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find(|line| line.trim().starts_with("X11 Layout:"))
        .and_then(|line| line.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Query setxkbmap for keyboard layout (X11).
fn query_setxkbmap() -> Option<String> {
    let output = Command::new("setxkbmap").arg("-query").output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find(|line| line.starts_with("layout:"))
        .and_then(|line| line.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Build character-to-keycode mapping using xkbcommon.
fn build_xkb_keymap(layout: &str) -> Result<HashMap<char, (u16, bool)>, String> {
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);

    let keymap = xkb::Keymap::new_from_names(
        &context,
        "",     // rules (empty = default "evdev")
        "",     // model (empty = default)
        layout, // layout
        "",     // variant
        None,   // options
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .ok_or_else(|| format!("failed to create keymap for layout '{}'", layout))?;

    let state = xkb::State::new(&keymap);
    let mut map = HashMap::new();

    // Scan all keycodes (8-255 is typical range for evdev)
    for xkb_keycode in 8u32..=255u32 {
        let linux_keycode = xkb_keycode.saturating_sub(8) as u16;

        // Try without shift
        if let Some(c) = get_char_for_keycode(&state, xkb_keycode, false) {
            map.entry(c).or_insert((linux_keycode, false));
        }

        // Try with shift
        if let Some(c) = get_char_for_keycode(&state, xkb_keycode, true) {
            map.entry(c).or_insert((linux_keycode, true));
        }
    }

    // Always add control characters
    map.insert('\n', (28, false)); // KEY_ENTER
    map.insert('\t', (15, false)); // KEY_TAB
    map.insert(' ', (57, false)); // KEY_SPACE

    Ok(map)
}

/// Get character produced by keycode with optional shift.
fn get_char_for_keycode(state: &xkb::State, keycode: u32, shift: bool) -> Option<char> {
    let xkb_keycode = Keycode::new(keycode);

    // Create a temporary state to test with shift
    let keymap = state.get_keymap();
    let mut test_state = xkb::State::new(&keymap);

    if shift {
        // Press shift (keycode 50 = Left Shift in XKB)
        test_state.update_key(Keycode::new(50), xkb::KeyDirection::Down);
    }

    let keysym = test_state.key_get_one_sym(xkb_keycode);
    if keysym == Keysym::NoSymbol {
        return None;
    }

    // Convert keysym to unicode
    let ch = xkb::keysym_to_utf32(keysym);
    if ch == 0 || ch > 0x10FFFF {
        return None;
    }

    char::from_u32(ch).filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_layout() {
        let layout = detect_keyboard_layout();
        println!("Detected layout: {}", layout);
        assert!(!layout.is_empty());
    }

    #[test]
    fn test_build_xkb_keymap_us() {
        let map = build_xkb_keymap("us").expect("failed to build US keymap");
        // Basic letters
        assert!(map.contains_key(&'a'));
        assert!(map.contains_key(&'A'));
        assert!(map.contains_key(&'z'));
        assert!(map.contains_key(&'Z'));

        // Numbers
        assert!(map.contains_key(&'0'));
        assert!(map.contains_key(&'9'));

        // Punctuation commonly emitted by Whisper
        let whisper_punctuation = [
            '.', ',', '!', '?', // sentence endings
            ':', ';', // colons
            '\'', '"', // quotes
            '-', '_', // dashes
            '(', ')', // parentheses
            '[', ']', // brackets
            '/', '\\', // slashes
            '@', '#', '$', '%', '&', '*', // symbols
        ];
        for c in whisper_punctuation {
            assert!(map.contains_key(&c), "missing punctuation: '{}'", c);
        }

        // Whitespace
        assert!(map.contains_key(&' '));
        assert!(map.contains_key(&'\n'));
        assert!(map.contains_key(&'\t'));
    }

    #[test]
    fn test_build_xkb_keymap_dk() {
        let map = build_xkb_keymap("dk").expect("failed to build Danish keymap");
        assert!(map.contains_key(&'a'));
        // Danish-specific characters
        assert!(map.contains_key(&'æ'));
        assert!(map.contains_key(&'ø'));
        assert!(map.contains_key(&'å'));

        // Punctuation should also work on Danish layout
        let whisper_punctuation = ['.', ',', '!', '?', ':', ';', '\'', '"', '-'];
        for c in whisper_punctuation {
            assert!(
                map.contains_key(&c),
                "missing punctuation on dk layout: '{}'",
                c
            );
        }
    }

    #[test]
    fn test_query_localectl() {
        // May or may not work depending on system
        let result = query_localectl();
        println!("localectl result: {:?}", result);
    }
}
