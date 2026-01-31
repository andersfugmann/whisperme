//! Text output - emits transcribed text via configured method (xdo, clipboard, ydotool, print)

use std::io::{self, Write};
use std::thread::{self, JoinHandle};

use crossbeam_channel::Receiver;

use crate::config::{OutputConfig, OutputMethod};

/// Global clipboard - lives for program duration to maintain X11 selection ownership.
#[cfg(feature = "clipboard")]
static CLIPBOARD: std::sync::LazyLock<std::sync::Mutex<arboard::Clipboard>> =
    std::sync::LazyLock::new(|| {
        std::sync::Mutex::new(arboard::Clipboard::new().expect("failed to initialize clipboard"))
    });

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
            eprintln!(
                "warning: clipboard output requested but not compiled in, falling back to print"
            );
            create_print_emitter()
        }

        #[cfg(feature = "ydotool")]
        OutputMethod::Ydotool => create_ydotool_emitter(keyboard_layout),
        #[cfg(not(feature = "ydotool"))]
        OutputMethod::Ydotool => {
            let _ = keyboard_layout;
            eprintln!(
                "warning: ydotool output requested but not compiled in, falling back to print"
            );
            create_print_emitter()
        }

        OutputMethod::Print => create_print_emitter(),
    }
}

#[cfg(feature = "xdo")]
fn create_xdo_emitter() -> Emitter {
    use libxdo::{OpError, XDo};
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
    // Clear clipboard at start of new session
    {
        let mut clipboard = CLIPBOARD.lock().unwrap();
        let _ = clipboard.clear();
    }

    let buffer = std::cell::RefCell::new(String::new());

    Box::new(move |text| {
        buffer.borrow_mut().push_str(text);
        let mut clipboard = CLIPBOARD.lock().unwrap();
        if let Err(e) = clipboard.set_text(buffer.borrow().as_str()) {
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
    use std::sync::Mutex;

    /// Global lock to prevent zenity tests from running in parallel
    #[allow(dead_code)]
    static ZENITY_LOCK: Mutex<()> = Mutex::new(());

    #[allow(dead_code)]
    fn test_zenity(name: &str, tx: crossbeam_channel::Sender<String>) {
        use std::thread;
        use std::time::Duration;

        let _guard = ZENITY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let zenity = std::process::Command::new("zenity")
            .args([
                "--entry",
                &format!("--title=WhisperMe Injection Test for {}", name),
                "--text=Waiting for input...",
                "--timeout=1",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect(&format!("{}: failed to start zenity", name));

        // Wait for zenity to start
        thread::sleep(Duration::from_millis(500));

        let segments = [
            name,
            " : ",
            "Hello, world!",
            " ",
            "What's this? It's a test: 1, 2, 3.",
        ];
        segments
            .iter()
            .for_each(|s| tx.send(s.to_string()).unwrap());
        drop(tx);

        let output = zenity
            .wait_with_output()
            .expect(&format!("{}: failed to read zenity output", name));
        let captured = String::from_utf8_lossy(&output.stdout)
            .trim_end_matches('\n')
            .to_string();
        let expected: String = segments.iter().copied().collect();
        assert_eq!(
            captured, expected,
            "{}: Captured input does not match",
            name
        );
    }

    #[test]
    #[cfg(all(feature = "system", feature = "xdo"))]
    fn test_xdo_init() {
        use libxdo::XDo;
        XDo::new(None).expect("failed to initialize xdo");
    }

    #[test]
    #[cfg(all(feature = "system", feature = "xdo"))]
    fn test_xdo_injection_with_zenity() {
        use crossbeam_channel as channel;

        use crate::config::{OutputConfig, OutputMethod};
        use crate::injection;

        let (tx, rx) = channel::unbounded::<String>();
        let config = OutputConfig {
            method: OutputMethod::Xdo,
            keyboard_layout: "auto".to_string(),
        };

        let handle = injection::spawn(rx, &config);
        test_zenity("Xdo", tx);
        let _ = handle.join();
    }

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

    #[test]
    #[cfg(all(feature = "system", feature = "ydotool"))]
    fn test_ydotool_injection_with_zenity() {
        use crossbeam_channel as channel;

        use crate::config::{OutputConfig, OutputMethod};
        use crate::injection;

        let (tx, rx) = channel::unbounded::<String>();
        let config = OutputConfig {
            method: OutputMethod::Ydotool,
            keyboard_layout: "auto".to_string(),
        };
        let handle = injection::spawn(rx, &config);
        test_zenity("Ydo", tx);
        let _ = handle.join();
    }

    #[test]
    #[cfg(all(feature = "system", feature = "clipboard", feature = "ydotool"))]
    fn test_clipboard_injection_with_zenity() {
        use std::thread;

        use crossbeam_channel as channel;

        use crate::config::{OutputConfig, OutputMethod};
        use crate::injection;
        use crate::ydotool::VirtualKeyboard;

        let (tx, rx) = channel::unbounded::<String>();
        let config = OutputConfig {
            method: OutputMethod::Clipboard,
            keyboard_layout: "auto".to_string(),
        };
        let injection_handle = injection::spawn(rx, &config);

        let keyboard =
            VirtualKeyboard::new("auto").expect("Clipboard: failed to create VirtualKeyboard");
        let thread = thread::spawn(move || {
            let _ = injection_handle.join();
            println!("Send paste to keyboard");
            keyboard.send_paste();
        });
        test_zenity("Clipboard", tx);
        let _ = thread.join();
    }
}
