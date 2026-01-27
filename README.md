# WhisperMe

A Linux speech-to-text input system powered by OpenAI's Whisper. Press a hotkey, speak, and your words are typed into any application.

## Features

- **Hotkey-triggered** – Press `Super+Shift+Space` to record, release to transcribe
- **Local AI** – Runs Whisper models locally (tiny to large-v3), no cloud required
- **Noise cancellation** – RNNoise removes background noise for cleaner transcription
- **Multi-language** – Auto-detects language or select from 11+ supported languages
- **Works everywhere** – Injects text into any focused application via xdotool

## Components

| Binary | Description |
|--------|-------------|
| `whispermed` | Background daemon that handles recording and transcription |
| `whisperme` | CLI client to control the daemon (start/stop/toggle) |
| `whisperme-config` | GUI for model selection, language settings, and configuration |

## How It Works

```
Hotkey → PipeWire capture → RNNoise → Whisper → xdotool → Text appears
```

## Requirements

- Linux with PipeWire
- X11 with xdotool (for text injection)
- Wayland support in progress (using ydotool)

## Building

```bash
make build
```

## License

MIT License - see [LICENSE](LICENSE) for details.
