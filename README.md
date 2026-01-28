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

To build without xdo (prints to stdout instead):
```bash
cargo build --no-default-features
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
| `method` | `xdo` | Output method: `xdo` (type into focused window) or `print` (stdout) |

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
```

## License

MIT
