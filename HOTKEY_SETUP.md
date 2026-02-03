# Hotkey Setup Guide

WhisperMe uses a socket-based control system, allowing any hotkey daemon or desktop environment to trigger recording.

---

## Quick Start

1. Start the WhisperMe daemon:
   ```bash
   whispermed
   ```

2. Configure your hotkey tool to run:
   ```bash
   whisperme toggle   # Press to start/stop recording
   ```

**Example:** Bind F12 to toggle recording on and off.

**Note:** Hold-to-record (key press to start, key release to stop) only works when `continuous_transcription` is disabled in your config. With continuous transcription enabled, text is typed while you're still holding the hotkey, which interferes with keyboard input. Use toggle mode or disable continuous transcription for hold-to-record.

---

## sxhkd (Simple X Hot Key Daemon)

Works on X11. Popular with tiling window managers (bspwm, i3, etc.)

### Installation

```bash
# Debian/Ubuntu
sudo apt install sxhkd

# Arch
sudo pacman -S sxhkd

# Fedora
sudo dnf install sxhkd
```

### Configuration

Edit `~/.config/sxhkd/sxhkdrc`:

```
# Toggle recording with F12
F12
  whisperme toggle
```

### Apply Changes

```bash
# Reload sxhkd config
pkill -USR1 sxhkd

# Or restart sxhkd
pkill sxhkd && sxhkd &
```

---

## GNOME (Wayland/X11)

### Using Settings GUI

1. Open **Settings → Keyboard → Keyboard Shortcuts**
2. Scroll to bottom, click **Custom Shortcuts**
3. Add shortcut:
   - Name: `WhisperMe Toggle`, Command: `whisperme toggle`, Shortcut: F12

### Using gsettings (CLI)

```bash
# Bind F12 to toggle recording
gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings \
  "['/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whisperme-toggle/']"

gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whisperme-toggle/ \
  name 'WhisperMe Toggle'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whisperme-toggle/ \
  command 'whisperme toggle'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whisperme-toggle/ \
  binding 'F12'
```

---

## KDE Plasma (Wayland/X11)

### Using System Settings GUI

1. Open **System Settings → Shortcuts → Custom Shortcuts**
2. Click **Edit → New → Global Shortcut → Command/URL**
3. Set trigger to F12 and command to `whisperme toggle`

### Using kwriteconfig5 (CLI)

```bash
kwriteconfig5 --file kglobalshortcutsrc --group whisperme \
  --key toggle "whisperme toggle,F12,WhisperMe Toggle"
```

---

## Hyprland (Wayland)

Edit `~/.config/hypr/hyprland.conf`:

```
# Toggle recording with F12
bind = , F12, exec, whisperme toggle
```

---

## i3 / Sway

Edit `~/.config/i3/config` or `~/.config/sway/config`:

```
# Toggle recording with F12
bindsym F12 exec whisperme toggle
```

---

## Generic: Using xdotool for Testing

Quick test without configuring hotkeys:

```bash
# Terminal 1: Start daemon
whispermed

# Terminal 2: Manual control
whisperme start
# speak...
whisperme stop
```

---

## Troubleshooting

### Command not found

Ensure `whisperme` is in your PATH:

```bash
# Add to ~/.bashrc or ~/.zshrc
export PATH="$PATH:/path/to/whisperme/target/release"
```

### Socket connection refused

Daemon not running. The client silently does nothing in this case.

Start the daemon:
```bash
whispermed &
```

Or enable autostart (see INSTALL.md).

### Hotkey not working

1. Check daemon is running: `pgrep whispermed`
2. Test command manually: `whisperme status` (prints "recording" or "idle" if daemon running, nothing if not)
3. Check hotkey daemon is running: `pgrep sxhkd`
4. Check for conflicts with existing shortcuts
5. Verify daemon has socket: `ls $XDG_RUNTIME_DIR/whisperme.sock`

**Note:** If the daemon is not running, `whisperme` commands silently do nothing. This is intentional - hotkeys won't cause errors if you haven't started the daemon.

---

## Socket Location

Default: `$XDG_RUNTIME_DIR/whisperme.sock`

Typically: `/run/user/1000/whisperme.sock`

You can test the socket directly:

```bash
echo "status" | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/whisperme.sock
```
