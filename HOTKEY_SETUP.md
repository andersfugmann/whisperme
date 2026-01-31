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
   whisperme start   # On key press
   whisperme stop    # On key release
   ```

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
# Hold to record, release to stop
super + shift + space
  whisperme start

@super + shift + space
  whisperme stop
```

The `@` prefix means "on key release".

### Toggle Mode (Alternative)

If you prefer press-once-to-start, press-again-to-stop:

```
super + alt + space
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
3. Add two shortcuts:
   - Name: `WhisperMe Start`, Command: `whisperme start`, Shortcut: your choice
   - Name: `WhisperMe Stop`, Command: `whisperme stop`, Shortcut: your choice

### Using gsettings (CLI)

```bash
# Example: Bind to Super+Shift+Space
gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings \
  "['/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whisperme-start/', \
    '/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whisperme-stop/']"

gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whisperme-start/ \
  name 'WhisperMe Start'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whisperme-start/ \
  command 'whisperme start'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whisperme-start/ \
  binding '<Super><Shift>space'
```

**Note:** GNOME doesn't support key-release bindings, so use toggle mode or separate keys for start/stop.

---

## KDE Plasma (Wayland/X11)

### Using System Settings GUI

1. Open **System Settings → Shortcuts → Custom Shortcuts**
2. Click **Edit → New → Global Shortcut → Command/URL**
3. Set trigger and command

### Using kwriteconfig5 (CLI)

```bash
kwriteconfig5 --file kglobalshortcutsrc --group whisperme \
  --key start "whisperme start,none,WhisperMe Start"
```

---

## Hyprland (Wayland)

Edit `~/.config/hypr/hyprland.conf`:

```
# Hold to record
bind = SUPER SHIFT, space, exec, whisperme start
bindr = SUPER SHIFT, space, exec, whisperme stop

# Or toggle mode
bind = SUPER ALT, space, exec, whisperme toggle
```

`bindr` triggers on key release.

---

## i3 / Sway

Edit `~/.config/i3/config` or `~/.config/sway/config`:

```
# Toggle mode (i3/sway don't support key release)
bindsym $mod+Shift+space exec whisperme toggle
```

For hold-to-record on i3, use sxhkd alongside i3.

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
