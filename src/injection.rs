//! Text output - emits transcribed text via configured method (xdo, clipboard, ydotool, print)

use std::io::{self, Write};
use std::thread::{self, JoinHandle};

use crossbeam_channel::Receiver;

use crate::config::{OutputConfig, OutputMethod};

/// Spawn text output thread.
///
/// Emits each text segment immediately as it arrives.
pub fn spawn(text_rx: Receiver<String>, config: &OutputConfig) -> JoinHandle<()> {
    let method = config.method;
    let keyboard_layout = config.keyboard_layout.clone();
    thread::spawn(move || {
        // Create emitter inside thread (some backends are not Send)
        let emitter = create_emitter(method, &keyboard_layout);
        text_rx
            .iter()
            .filter(|text| !text.is_empty())
            .for_each(|text| emitter(&text))
    })
}

/// Closure that emits text
type Emitter = Box<dyn Fn(&str)>;

/// Create emitter based on method - init once, return closure
fn create_emitter(method: OutputMethod, keyboard_layout: &str) -> Emitter {
    match method {
        #[cfg(feature = "xdo")]
        OutputMethod::Xdo => create_xdo_emitter(),
        #[cfg(not(feature = "xdo"))]
        OutputMethod::Xdo => {
            eprintln!("warning: xdo output requested but not compiled in, falling back to print");
            create_print_emitter()
        }

        #[cfg(feature = "clipboard")]
        OutputMethod::Clipboard => create_clipboard_emitter(),
        #[cfg(not(feature = "clipboard"))]
        OutputMethod::Clipboard => {
            eprintln!("warning: clipboard output requested but not compiled in, falling back to print");
            create_print_emitter()
        }

        #[cfg(feature = "ydotool")]
        OutputMethod::Ydotool => create_ydotool_emitter(keyboard_layout),
        #[cfg(not(feature = "ydotool"))]
        OutputMethod::Ydotool => {
            let _ = keyboard_layout;
            eprintln!("warning: ydotool output requested but not compiled in, falling back to print");
            create_print_emitter()
        }

        OutputMethod::Print => create_print_emitter(),
    }
}

#[cfg(feature = "xdo")]
fn create_xdo_emitter() -> Emitter {
    use libxdo::{XDo, OpError};
    let xdo = XDo::new(None).unwrap_or_else(|e| {
        eprintln!("error: failed to initialize xdo: {}", e);
        std::process::exit(1);
    });
    Box::new(move |text| {
        if let Err(e) = xdo.enter_text(text, 0) {
            eprintln!("error: xdo typing failed: {}", e);
            let code = match e {
                OpError::Ffi(c) => c,
                OpError::Nul(_) => 1,
            };
            std::process::exit(code);
        }
    })
}

#[cfg(feature = "clipboard")]
fn create_clipboard_emitter() -> Emitter {
    use arboard::Clipboard;
    use std::cell::RefCell;

    let clipboard = RefCell::new(Clipboard::new().unwrap_or_else(|e| {
        eprintln!("error: failed to initialize clipboard: {}", e);
        std::process::exit(1);
    }));
    let buffer = RefCell::new(String::new());

    Box::new(move |text| {
        buffer.borrow_mut().push_str(text);
        if let Err(e) = clipboard.borrow_mut().set_text(buffer.borrow().as_str()) {
            eprintln!("error: clipboard set failed: {}", e);
            std::process::exit(1);
        }
    })
}

#[cfg(feature = "ydotool")]
fn create_ydotool_emitter(keyboard_layout: &str) -> Emitter {
    use crate::ydotool::VirtualKeyboard;

    let keyboard = VirtualKeyboard::new(keyboard_layout).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        std::process::exit(1);
    });

    Box::new(move |text| {
        keyboard.type_text(text);
    })
}

fn create_print_emitter() -> Emitter {
    Box::new(|text| {
        print!("{}", text);
        let _ = io::stdout().flush();
    })
}

#[cfg(test)]
mod tests {
    /// Requires X11 and libxdo - run with: make test-system
    #[test]
    #[cfg(all(feature = "system", feature = "xdo"))]
    fn test_xdo_init() {
        use libxdo::XDo;
        XDo::new(None).expect("failed to initialize xdo");
    }

    /// Integration test: spawn zenity, inject text via xdo, verify output.
    /// Requires X11, libxdo, and zenity - run with: make test-system
    #[test]
    #[cfg(all(feature = "system", feature = "xdo"))]
    fn test_xdo_injection_with_zenity() {
        use std::process::{Command, Stdio};
        use std::thread;
        use std::time::{Duration, Instant};

        use crossbeam_channel as channel;

        use crate::config::{OutputConfig, OutputMethod};
        use crate::injection;

        // Start zenity entry dialog with timeout
        let mut zenity = Command::new("zenity")
            .args([
                "--entry",
                "--title=WhisperMe XDo Test",
                "--text=Waiting for input...",
                "--timeout=2",
            ])
            .stdout(Stdio::piped())
            .spawn()
            .expect("failed to start zenity - install with: sudo apt install zenity");

        // Give zenity time to open
        thread::sleep(Duration::from_millis(500));

        /*
        // Focus the zenity window using xdotool CLI
        let focus_result = Command::new("xdotool")
            .args(["search", "--name", "WhisperMe XDo Test", "windowactivate", "--sync"])
            .status();
        assert!(
            focus_result.map(|s| s.success()).unwrap_or(false),
            "failed to focus zenity window with xdotool"
        );
        thread::sleep(Duration::from_millis(200));
*/
        // Create channel and spawn injection thread
        let (tx, rx) = channel::unbounded::<String>();
        let config = OutputConfig {
            method: OutputMethod::Xdo,
            keyboard_layout: "auto".to_string(),
        };
        let handle = injection::spawn(rx, &config);

        // Send multiple text segments
        let segments = ["Hello", " ", "from", " ", "WhisperMe", "!"];
        segments.iter().for_each(|s| tx.send(s.to_string()).unwrap());

        // Close channel and wait for injection thread
        drop(tx);
        handle.join().expect("injection thread panicked");

        // Send Return key via libxdo to submit (enter_text doesn't handle \n)
        use libxdo::XDo;
        let xdo = XDo::new(None).expect("failed to init xdo");
        xdo.send_keysequence("Return", 0).expect("failed to send Return key");

        // Wait for zenity to exit with timeout
        let start = Instant::now();
        let timeout = Duration::from_secs(10);
        let output = loop {
            match zenity.try_wait() {
                Ok(Some(_)) => break zenity.wait_with_output().expect("failed to read zenity output"),
                Ok(None) if start.elapsed() < timeout => {
                    thread::sleep(Duration::from_millis(100));
                }
                Ok(None) => {
                    let _ = zenity.kill();
                    panic!("test timed out - xdo injection may have failed");
                }
                Err(e) => panic!("error waiting for zenity: {}", e),
            }
        };

        // Check zenity exit status (timeout returns exit code 5)
        assert!(
            output.status.success(),
            "zenity exited with error (code {:?}) - likely hit timeout before receiving input",
            output.status.code()
        );

        // Verify
        let captured = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let expected: String = segments.iter().copied().collect();
        assert_eq!(
            captured, expected,
            "xdo injection mismatch: expected '{}', got '{}'",
            expected, captured
        );
    }

    /// Integration test: connect to ydotoold and verify it initializes.
    /// Requires ydotoold running - run with: make test-system
    #[test]
    #[cfg(all(feature = "system", feature = "ydotool"))]
    fn test_ydotool_connection() {
        use crate::ydotool::VirtualKeyboard;
        let keyboard = VirtualKeyboard::new("auto");
        assert!(
            keyboard.is_ok(),
            "failed to connect to ydotoold: {:?}",
            keyboard.err()
        );
    }

    /// Integration test: spawn zenity, inject text via ydotool, verify output.
    /// Requires ydotoold running and display - run with: make test-system
    #[test]
    #[cfg(all(feature = "system", feature = "ydotool"))]
    fn test_ydotool_injection_with_zenity() {
        use std::process::{Command, Stdio};
        use std::thread;
        use std::time::{Duration, Instant};

        use crossbeam_channel as channel;

        use crate::config::{OutputConfig, OutputMethod};
        use crate::injection;

        // Start zenity entry dialog with timeout
        let mut zenity = Command::new("zenity")
            .args([
                "--entry",
                "--title=WhisperMe Ydotool Test",
                "--text=Waiting for input...",
                "--timeout=5",
            ])
            .stdout(Stdio::piped())
            .spawn()
            .expect("failed to start zenity - install with: sudo apt install zenity");

        // Give zenity time to open and focus
        thread::sleep(Duration::from_millis(800));

        // Create channel and spawn injection thread
        let (tx, rx) = channel::unbounded::<String>();
        let config = OutputConfig {
            method: OutputMethod::Ydotool,
            keyboard_layout: "auto".to_string(),
        };
        let handle = injection::spawn(rx, &config);

        // Test text with punctuation commonly emitted by Whisper
        let segments = ["Hello, world!", " ", "What's this? It's a test: 1, 2, 3."];
        segments.iter().for_each(|s| tx.send(s.to_string()).unwrap());

        // Close channel and wait for injection thread
        drop(tx);
        handle.join().expect("injection thread panicked");

        // Small delay for events to be processed
        thread::sleep(Duration::from_millis(100));

        // Send Enter key via ydotool to submit
        use crate::ydotool::VirtualKeyboard;
        let enter_keyboard = VirtualKeyboard::new("auto").unwrap();
        enter_keyboard.type_text("\n");

        // Wait for zenity to exit with timeout
        let start = Instant::now();
        let timeout = Duration::from_secs(10);
        let output = loop {
            match zenity.try_wait() {
                Ok(Some(_)) => break zenity.wait_with_output().expect("failed to read zenity output"),
                Ok(None) if start.elapsed() < timeout => {
                    thread::sleep(Duration::from_millis(100));
                }
                Ok(None) => {
                    let _ = zenity.kill();
                    panic!("test timed out - ydotool injection may have failed");
                }
                Err(e) => panic!("error waiting for zenity: {}", e),
            }
        };

        // Check zenity exit status (timeout returns exit code 5)
        assert!(
            output.status.success(),
            "zenity exited with error (code {:?}) - likely hit timeout before receiving input",
            output.status.code()
        );

        // Verify
        let captured = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let expected: String = segments.iter().copied().collect();
        assert_eq!(
            captured, expected,
            "ydotool injection mismatch: expected '{}', got '{}'",
            expected, captured
        );
    }
}
