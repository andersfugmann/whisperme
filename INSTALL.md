# Installation Guide

This document describes how to install and set up WhisperMe.

---

## Installation

### From Source

```bash
# Clone repository
git clone https://github.com/andersfugmann/whisperme.git
cd whisperme

# Install build dependencies
sudo make deps

# Build release binaries
make release

# Install to ~/.local/bin (optional)
mkdir -p ~/.local/bin
cp target/release/whispermed ~/.local/bin/
cp target/release/whisperme ~/.local/bin/

# Ensure ~/.local/bin is in PATH
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

### From Debian Package

```bash
sudo apt install ./whisperme_*.deb
```

### Configure WhisperMe

Start the configuration dialog, to select and download models
```bash
whisperme config
```

### Configure Hotkey

See [HOTKEY_SETUP.md](HOTKEY_SETUP.md) for detailed instructions.

```bash
# Start daemon (foreground)
whispermed

# Or start in background
whispermed &

# Or use a process manager (see Autostart section)
```

### Test Recording

1. Open a text editor or terminal
2. Press your hotkey (e.g., F12) to start recording
3. Speak into your microphone
4. Press the hotkey again to stop recording
5. Text should appear in the focused input text field

### Check Status

```bash
whisperme status
# Output: "recording" or "idle"
```

### Desktop Autostart

Create `~/.config/autostart/whispermed.desktop`:

```ini
[Desktop Entry]
Type=Application
Name=WhisperMe Daemon
Exec=whispermed
Hidden=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
```
