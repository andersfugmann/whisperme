# WhisperMe

Linux speech-to-text using OpenAI Whisper. Press a hotkey to start recording, press again to transcribe into any application.

## Features

- Local Whisper inference with Vulkan acceleration (tiny to large-v3 models)
- PipeWire audio capture
- RNNoise background noise removal
- Auto language detection or manual selection
- Multiple output methods: xdo (X11), ydotool (X11/Wayland), clipboard
- GUI configuration tool (`whisperme config`)
- Recording indicator with frequency visualization

## Components

| Binary | Description |
|--------|-------------|
| `whispermed` | Daemon for recording and transcription |
| `whisperme` | CLI to control the daemon (start/stop/toggle/status/config) |

## Build Dependencies

```bash
sudo make deps
```

## Building

```bash
make build
```

To build with specific output methods:
```bash
make build FEATURES=                              # print only
make build FEATURES=xdo                           # xdo (default)
make build FEATURES=clipboard                     # clipboard
make build FEATURES=ydotool                       # ydotool (Wayland)
make build FEATURES="xdo,clipboard,ydotool"       # all methods
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
| `continuous_transcription` | `false` | ⚠️ **Experimental** - Enable continuous transcription (lower quality) |
| `transcription_interval_ms` | `1000` | How often to run transcription on buffered audio (ms), when continuous transcription is enabled |
| `emit_grace_ms` | `1200` | Delay before emitting text to avoid incomplete words (ms) |
| `language_confidence` | `0.7` | Minimum confidence for language detection (0.0-1.0) |
| `silence_threshold_dbfs` | `-60` | Threshold in dBFS per transcribed segment. Segments below this are discarded |

**⚠️ Experimental:** Continuous transcription provides real-time visibility of what's being transcribed, but produces lower quality output compared to transcribing all audio after recording ends (the default). Use with caution.

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
sudo systemctl --user enable ydotoold --now
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
continuous_transcription = false
transcription_interval_ms = 1000
emit_grace_ms = 1200
language_confidence = 0.7
silence_threshold_dbfs = -60

[output]
method = ydotool
keyboard_layout = auto
```

## License

MIT
