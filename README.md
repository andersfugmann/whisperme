# WhisperMe

Local speech-to-text for Linux. Press a hotkey, speak, press again -- transcribed text is typed into the focused window.

Whisper inference with Vulkan acceleration, PipeWire audio capture, RNNoise noise removal, auto language detection, recording indicator with frequency visualization.

## Install

From source:
```bash
sudo make deps && make release
cp target/release/whisperme target/release/whispermed ~/.local/bin/
```

From .deb: `sudo apt install ./whisperme_*.deb`

Then run `whisperme config` to select and download a model.

## Usage

```bash
whispermed              # start daemon
whisperme toggle        # start/stop recording (bind to a hotkey)
whisperme status        # print current state
whisperme config        # open config GUI
```

## Building

```bash
sudo make deps                                    # install build dependencies
make build                                        # debug build
make release                                      # release build
make build FEATURES=xdo                           # xdo only (default)
make build FEATURES=ydotool                       # ydotool (Wayland)
make build FEATURES="xdo,clipboard,ydotool"       # all output methods
make test                                         # unit tests
make test-system                                  # tests requiring X11
make test-slow                                    # transcription tests (downloads model)
```

## Hotkey Setup

Bind `whisperme toggle` to a key. Any hotkey tool works. Examples below all use F12.

Hold-to-record (press=start, release=stop) requires `continuous_transcription = false`.

### xbindkeys (X11)

`sudo apt install xbindkeys`, then create `~/.xbindkeysrc`:
```
"whisperme toggle"
  F12
```
Run `xbindkeys`. Reload after edits: `xbindkeys --poll-rc`. Add to session autostart.

### sxhkd (X11)

`sudo apt install sxhkd`, then edit `~/.config/sxhkd/sxhkdrc`:
```
F12
  whisperme toggle
```
Reload: `pkill -USR1 sxhkd`

### GNOME

Settings > Keyboard > Keyboard Shortcuts > Custom Shortcuts. Name: `WhisperMe Toggle`, Command: `whisperme toggle`, Shortcut: `F12`.

Or via CLI:
```bash
gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings \
  "['/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whisperme-toggle/']"

PREFIX=org.gnome.settings-daemon.plugins.media-keys.custom-keybinding
P=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/whisperme-toggle/
gsettings set $PREFIX:$P name 'WhisperMe Toggle'
gsettings set $PREFIX:$P command 'whisperme toggle'
gsettings set $PREFIX:$P binding 'F12'
```

### KDE Plasma

System Settings > Shortcuts > Custom Shortcuts > Edit > New > Global Shortcut > Command/URL. Trigger: `F12`, command: `whisperme toggle`.

Or: `kwriteconfig5 --file kglobalshortcutsrc --group whisperme --key toggle "whisperme toggle,F12,WhisperMe Toggle"`

### Hyprland

In `~/.config/hypr/hyprland.conf`:
```
bind = , F12, exec, whisperme toggle
```

### i3 / Sway

In `~/.config/i3/config` or `~/.config/sway/config`:
```
bindsym F12 exec whisperme toggle
```

### Socket

The daemon listens on `$XDG_RUNTIME_DIR/whisperme.sock`. Direct access:
```bash
echo "status" | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/whisperme.sock
```

## Configuration

Config file: `~/.config/whisperme/config.ini` (or `$XDG_CONFIG_HOME/whisperme/config.ini`).
Models directory: `~/.local/share/whisperme/models/`

### [whisper]

| Option | Default | Description |
|--------|---------|-------------|
| `model` | `base.en` | Model name, relative path (`./`), or absolute path |
| `language` | `auto` | Language code (auto, en, es, fr, de, zh, ja, ko, pt, ru, it) |

Model names resolve to `$XDG_DATA_HOME/whisperme/models/ggml-{name}.bin`.
Available: tiny, tiny.en, base, base.en, small, small.en, medium, medium.en, large-v3, large-v3-turbo

### [ui]

| Option | Default | Description |
|--------|---------|-------------|
| `enabled` | `true` | Show recording indicator |
| `position` | `top-center` | top-left, top-center, top-right, bottom-left, bottom-center, bottom-right |

### [transcription]

| Option | Default | Description |
|--------|---------|-------------|
| `continuous_transcription` | `false` | Experimental: transcribe while recording (lower quality) |
| `transcription_interval_ms` | `1000` | Interval for continuous transcription (ms) |
| `emit_grace_ms` | `1200` | Delay before emitting text to avoid incomplete words (ms) |
| `language_confidence` | `0.7` | Minimum confidence for language detection (0.0-1.0) |
| `silence_threshold_dbfs` | `-60` | dBFS threshold; segments below are discarded |

### [output]

| Option | Default | Description |
|--------|---------|-------------|
| `method` | `xdo` | Output method: xdo, clipboard, ydotool, print |
| `keyboard_layout` | `auto` | Keyboard layout for ydotool (auto, us, dk, de, gb, fr, es) |

`ydotool` requires the ydotoold daemon: `sudo apt install ydotool && sudo systemctl --user enable ydotoold --now`. Set `keyboard_layout` to match your system layout; `auto` detects via localectl/setxkbmap.

### Example config

```ini
[whisper]
model = base.en
language = auto

[ui]
enabled = true
position = top-center

[transcription]
continuous_transcription = false

[output]
method = ydotool
keyboard_layout = auto
```

## Autostart

Create `~/.config/autostart/whispermed.desktop`:
```ini
[Desktop Entry]
Type=Application
Name=WhisperMe Daemon
Exec=whispermed
NoDisplay=true
X-GNOME-Autostart-enabled=true
```

## License

MIT
