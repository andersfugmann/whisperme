# WhisperMe

Linux speech-to-text using OpenAI Whisper. Press a hotkey, speak, release to transcribe into any application.

## Features

- Local Whisper inference (tiny to large-v3 models)
- RNNoise background noise removal
- Auto language detection or manual selection
- Text injection via libxdo (X11)
- Allow Hotkey-triggered transciption (Handled by the system)

## Components

| Binary | Description |
|--------|-------------|
| `whispermed` | Daemon for recording and transcription |
| `whisperme` | CLI to control the daemon (start/stop/toggle/status/config) |

## Pipeline

```
Hotkey -> PipeWire -> RNNoise -> Whisper -> libxdo -> Text
```

## Build Dependencies

```bash
sudo apt install libpipewire-0.3-dev libclang-dev libvulkan-dev glslc libxdo-dev
```

## Building

```bash
make build
```

To build with specific output methods:
```bash
cargo build --no-default-features                    # print only
cargo build --features xdo                           # xdo (default)
cargo build --features clipboard                     # clipboard
cargo build --features uinput                        # uinput (Wayland)
cargo build --features "xdo,clipboard,uinput"        # all methods
```

## Testing

```bash
make test          # Unit tests
make test-system   # Tests requiring X11/display
make test-slow     # Transcription tests (downloads model)
```

## Configuration

Configuration file: `$XDG_CONFIG_HOME/whisperme/config.ini`

Default paths (when XDG variables are unset):
- Config: `~/.config/whisperme/config.ini`
- Models: `~/.local/share/whisperme/models/`

### [whisper]

| Option | Default | Description |
|--------|---------|-------------|
| `model` | `base.en` | Model name, relative path (`./`), or absolute path |
| `language` | `auto` | Language code (auto, en, es, fr, de, zh, ja, ko, pt, ru, it) |

Model path resolution:
- Absolute path (`/path/to/model.bin`): used as-is
- Relative path (`./models/model.bin`): relative to current directory
- Model name (`base.en`): resolves to `$XDG_DATA_HOME/whisperme/models/ggml-{name}.bin`

Available models: tiny, tiny.en, base, base.en, small, small.en, medium, medium.en, large-v3, large-v3-turbo

### [ui]

| Option | Default | Description |
|--------|---------|-------------|
| `enabled` | `true` | Show recording indicator window |
| `position` | `top-center` | Window position (top-left, top-center, top-right, bottom-left, bottom-center, bottom-right) |

### [transcription]

| Option | Default | Description |
|--------|---------|-------------|
| `transcription_interval_ms` | `1000` | How often to run transcription on buffered audio (ms) |
| `emit_grace_ms` | `1200` | Delay before emitting text to avoid incomplete words (ms) |
| `language_confidence` | `0.7` | Minimum confidence for language detection (0.0-1.0) |
| `silence_threshold_dbfs` | `-80` | RMS threshold in dBFS per transcribed segment. Segments below this are discarded |

### [output]

| Option | Default | Description |
|--------|---------|-------------|
| `method` | `xdo` | Output method (see below) |
| `keyboard_layout` | `auto` | Keyboard layout for ydotool (auto, us, dk, de, gb, etc.) |

Output methods:
- `xdo` - Type into focused X11 window (requires libxdo)
- `clipboard` - Copy to system clipboard
- `ydotool` - Simulate keyboard via ydotoold (works on X11 and Wayland)
- `print` - Print to stdout

**ydotool setup:** The `ydotool` method requires the ydotoold daemon. Install and enable it:

```bash
# Install ydotool (Debian/Ubuntu)
sudo apt install ydotool

# Enable and start the systemd service
sudo systemctl enable ydotoold
sudo systemctl start ydotoold

# Allow your user to access the socket
sudo usermod -aG input $USER
# Log out and back in for group change to take effect
```

Alternatively, run ydotoold manually with permissive socket:
```bash
sudo ydotoold --socket-perm 0666 &
```

**ydotool keyboard layout:** When using `ydotool`, set `keyboard_layout` to match your system layout. Use `auto` to detect from localectl or setxkbmap. Common layouts: `us`, `dk`, `de`, `gb`, `fr`, `es`.

### Example

```ini
[whisper]
model = base.en
language = auto

[ui]
enabled = true
position = top-center

[transcription]
transcription_interval_ms = 1000
emit_grace_ms = 1200
language_confidence = 0.7
silence_threshold_dbfs = -80

[output]
method = xdo
keyboard_layout = auto
```

## License

MIT
