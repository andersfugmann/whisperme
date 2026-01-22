//! Text Injection Thread - types transcribed text into focused window using xdotool.

use std::process::Command;

use crate::session::TextReceiver;

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
            eprintln!("hint: ensure a text input is focused and the application accepts synthetic input");
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
pub fn run(text_rx: TextReceiver) {
    check_xdotool();
    while let Ok(text) = text_rx.recv() {
        if !text.is_empty() {
            type_text(&text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Actually types text via xdotool - run manually
    fn test_xdotool_available() {
        check_xdotool();
    }
}
