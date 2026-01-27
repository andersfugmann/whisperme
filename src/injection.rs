//! Text Injection Thread - types transcribed text into focused window using xdotool.

use std::thread::{self, JoinHandle};
use std::process::Command;
use crossbeam_channel::Receiver;

/// Verify xdotool is available, exit with error if missing.
fn check_xdotool() {
    let ok = Command::new("xdotool")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !ok {
        eprintln!("error: xdotool not found or not working correctly");
        eprintln!();
        eprintln!("To fix, install xdotool:");
        eprintln!("  Ubuntu/Debian: sudo apt install xdotool");
        eprintln!("  Arch Linux:    sudo pacman -S xdotool");
        eprintln!("  Fedora:        sudo dnf install xdotool");
        std::process::exit(1);
    }
}

/// Type text using xdotool. Fails fast on error.
fn type_text(text: &str) {
    let status = Command::new("xdotool")
        .args(["type", "--clearmodifiers", "--", text])
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("error: xdotool type failed with exit code {:?}", s.code());
            eprintln!(
                "hint: ensure a text input is focused and the application accepts synthetic input"
            );
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("error: xdotool execution failed: {}", e);
            std::process::exit(1);
        }
    }
}

/// Run the text injection thread.
///
/// Types each text segment immediately as it arrives.
pub fn spawn(text_rx: Receiver<String>) -> JoinHandle<()> {
    check_xdotool();
    thread::spawn(move || {
        text_rx
            .iter()
            .filter(|text| !text.is_empty())
            .for_each(|text| type_text(&text))
    })
}

#[cfg(test)]
mod tests {
    /// Requires X11 and xdotool - run with: make test-hardware
    #[test]
    #[cfg(feature = "hardware")]
    fn test_xdotool_available() {
        use super::*;
        check_xdotool();
    }
}
