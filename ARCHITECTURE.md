# WhisperMe Architecture Design

**Version:** 2.0  
**Date:** 2026-01-22  
**Purpose:** Voice-to-text transcription with real-time text injection using XDO

---

## Overview

WhisperMe is a multi-threaded Rust application that captures audio from a microphone, transcribes it using Whisper, and injects the text into the focused application using XDO tools. The application uses a stream-based architecture with minimal message passing.

**Core Principles:**
- Stream-based audio distribution (not chunk-based)
- Request-response communication (no events)
- Fail-fast error handling (no recovery)
- Minimal state management
- Fixed audio format: 32kHz mono

---

## System Architecture

### Thread Overview

The application consists of 6 independent threads:

1. **Main Thread** - Central coordinator
2. **Audio Capture Thread** - Microphone input (wavy)
3. **Transcription Thread** - Speech-to-text (whisper-rs)
4. **XDO Injection Thread** - Text output (ydotool/xdotool)
5. **UI Thread** - Visualization (egui with FFT)
6. **Hotkey Monitor Thread** - Global hotkey listener (evdev)

### Architecture Diagram

```
┌──────────────────────────────────────────────────────────────┐
│                    MAIN THREAD                                │
│                                                                │
│  on HotkeyMessage::Pressed:                                   │
│    audio_req → Start                                          │
│    audio_resp ← Started(stream)                               │
│    stream1, stream2 = clone(stream)                           │
│    transc_req → ProcessStream(stream1)                        │
│    transc_resp ← TokenQueue(queue)                            │
│    ui_req → Show(stream2)                                     │
│    loop: queue ← Token → injection_req → TypeText(token.text) │
│                                                                │
│  on HotkeyMessage::Released:                                  │
│    audio_req → Stop                                           │
│    (streams close → UI auto-hides, token queue closes)        │
│                                                                │
│  on SIGTERM: exit(0)                                          │
│  on ANY ERROR: eprintln!() + exit(1)                          │
└────────┬───────────────────────────┬───────────┬─────────────┘
         │                           │           │
         ▼                           ▼           ▼
  ┌─────────────┐          ┌─────────────┐  ┌──────────┐
  │   AUDIO     │          │TRANSCRIPTION│  │ XDO INJ  │
  │   (wavy)    │          │ (whisper-rs)│  │(ydotool) │
  │             │          │             │  │          │
  │ ← Start     │          │ ← Process   │  │ ← Type   │
  │ → Started   │          │   Stream    │  │   Text   │
  │   (stream)  │          │ → Token     │  │          │
  │             │          │   Queue     │  └──────────┘
  │ ← Stop      │          │             │
  │ → Stopped   │          │ Load model  │
  │             │          │ Read stream │
  │ (closes     │          │ → tokens    │
  │  stream)    │          │ Close queue │
  └─────────────┘          └─────────────┘
         │
         │ stream2
         ▼
  ┌───────────┐
  │    UI     │
  │  (egui)   │
  │           │
  │ ← Show    │
  │   (stream)│
  │           │
  │ Show,     │
  │ FFT + viz │
  │ On stream │
  │ close:    │
  │ Auto-hide │
  └───────────┘

  ┌─────────────┐
  │   HOTKEY    │
  │   (evdev)   │
  │             │
  │ → Pressed   │
  │ → Released  │
  └─────────────┘
```

---

## Message Definitions

All messages are defined as simple enums. No state management or complex error handling.

### Audio Messages

```
enum AudioRequest =
  | Start
  | Stop

enum AudioResponse =
  | Started(AudioStream)  // Wavy's native stream (32kHz mono)
  | Stopped
```

### Transcription Messages

```
enum TranscriptionRequest =
  | ProcessStream(AudioStream)

enum TranscriptionResponse =
  | TokenQueue(Receiver<Token>)

struct Token =
  text: String
```

### Text Injection Messages

```
enum InjectionRequest =
  | TypeText(text)
```

### UI Messages

```
enum UiRequest =
  | Show(AudioStream)  // Show window with stream, auto-hide when stream closes
```

### Hotkey Messages

```
enum HotkeyMessage =
  | Pressed
  | Released
```

---

## Channel Ownership

### Main Thread

**Owns (Request senders):**
- `→ audio_requests: Sender<AudioRequest>`
- `→ ui_requests: Sender<UiRequest>`
- `→ transcription_requests: Sender<TranscriptionRequest>`
- `→ injection_requests: Sender<InjectionRequest>`

**Owns (Response receivers):**
- `← audio_responses: Receiver<AudioResponse>`
- `← transcription_responses: Receiver<TranscriptionResponse>`
- `← hotkey_messages: Receiver<HotkeyMessage>`

**Owns (Dynamic during recording):**
- `← token_queue: Receiver<Token>`

### Audio Capture Thread

**Owns:**
- `← audio_requests: Receiver<AudioRequest>`
- `→ audio_responses: Sender<AudioResponse>`

### Transcription Thread

**Owns:**
- `← transcription_requests: Receiver<TranscriptionRequest>`
- `→ transcription_responses: Sender<TranscriptionResponse>`

### XDO Injection Thread

**Owns:**
- `← injection_requests: Receiver<InjectionRequest>`

### UI Thread

**Owns:**
- `← ui_requests: Receiver<UiRequest>`

### Hotkey Monitor Thread

**Owns:**
- `→ hotkey_messages: Sender<HotkeyMessage>`

---

## Thread Behaviors

### Main Thread

```
on_startup():
  spawn hotkey_thread() → hotkey_rx
  spawn audio_thread() → (audio_req_tx, audio_resp_rx)
  spawn transcription_thread() → (transc_req_tx, transc_resp_rx)
  spawn ui_thread() → ui_req_tx
  spawn xdo_thread() → inj_req_tx
  
  loop:
    select:
      on hotkey_rx receives Pressed:
        // Get audio stream
        audio_req_tx.send(Start).unwrap_or_exit()
        stream = audio_resp_rx.recv().unwrap_or_exit()  // Started(stream)
        
        // Duplicate stream
        stream1 = stream.clone()
        stream2 = stream.clone()
        
        // Get token queue from transcription
        transc_req_tx.send(ProcessStream(stream1)).unwrap_or_exit()
        queue = transc_resp_rx.recv().unwrap_or_exit()  // TokenQueue(queue)
        
        // Start UI with stream
        ui_req_tx.send(Show(stream2)).unwrap_or_exit()
        
        // Relay tokens to XDO
        spawn async:
          loop:
            token = queue.recv()
            if token is None:
              break
            
            inj_req_tx.send(TypeText(token.text)).unwrap_or_exit()
      
      on hotkey_rx receives Released:
        audio_req_tx.send(Stop).unwrap_or_exit()
        // UI auto-hides when stream closes
        // Token relay loop exits when queue closes
      
      on SIGTERM:
        exit(0)
```

### Audio Capture Thread

```
on_startup():
  loop:
    request = audio_requests.recv().unwrap_or_exit()
    
    match request:
      Start:
        mic = wavy::Microphone::default()
        mic.configure(32000, mono)
        stream = mic.record()
        
        audio_responses.send(Started(stream)).unwrap_or_exit()
        
        // Wait for Stop
        loop:
          if audio_requests.try_recv() == Stop:
            break
          sleep(100ms)
        
        drop(mic)  // Closes all stream clones
        audio_responses.send(Stopped).unwrap_or_exit()
```

### Transcription Thread

```
on_startup():
  model = load_whisper_model("ggml-base.en.bin").unwrap_or_exit()
  
  loop:
    request = transcription_requests.recv().unwrap_or_exit()
    
    match request:
      ProcessStream(stream):
        (token_tx, token_rx) = mpsc::channel()
        
        // Return queue immediately
        transcription_responses.send(TokenQueue(token_rx)).unwrap_or_exit()
        
        // Process stream
        spawn async:
          buffer = []
          
          loop:
            samples = stream.next()
            if samples is None:
              break
            
            buffer.extend(samples)
            
            if buffer.duration() >= 3.0:
              result = model.transcribe(buffer).unwrap_or_exit()
              
              for token in result.tokens:
                token_tx.send(Token { text: token.text }).unwrap_or_exit()
              
              buffer.clear()
          
          // Final flush
          if not buffer.empty():
            result = model.transcribe(buffer).unwrap_or_exit()
            for token in result.tokens:
              token_tx.send(token).unwrap_or_exit()
          
          drop(token_tx)  // Close queue
```

### XDO Injection Thread

```
on_startup():
  method = detect_method()  // ydotool or xdotool or clipboard
  
  loop:
    request = injection_requests.recv().unwrap_or_exit()
    
    match request:
      TypeText(text):
        match method:
          YDoTool:
            exec("ydotool type '" + escape(text) + "'").unwrap_or_exit()
          XDoTool:
            exec("xdotool type '" + escape(text) + "'").unwrap_or_exit()
          Clipboard:
            clipboard_set(text).unwrap_or_exit()
            exec("ydotool key ctrl+v").unwrap_or_exit()
```

### UI Thread

```
on_startup():
  fft = FFT::new(1024)
  
  loop:
    request = ui_requests.recv().unwrap_or_exit()
    
    match request:
      Show(stream):
        window = egui::Window::new()
        window.show()
        
        waveform = CircularBuffer::new(2s)
        spectrum = [0.0; 50]
        
        loop:
          select:
            on stream.next_timeout(16ms):
              samples = stream.next_timeout(16ms)
              
              if samples is None:  // Stream closed
                break
              
              waveform.push(samples)
              spectrum = fft.process(samples).to_bars(50)
              request_redraw()
            
            on render_frame:
              draw_waveform(waveform)
              draw_spectrum_bars(spectrum)
        
        // Stream closed - hide window
        window.hide()
        drop(window)
```

### Hotkey Monitor Thread

```
on_startup():
  device = evdev::Device::open("/dev/input/event*").unwrap_or_exit()
  hotkey = parse_hotkey("Ctrl+Shift+Space")
  
  loop:
    event = device.read_event().unwrap_or_exit()
    
    if event.matches(hotkey):
      if event.is_press():
        hotkey_messages.send(Pressed).unwrap_or_exit()
      elif event.is_release():
        hotkey_messages.send(Released).unwrap_or_exit()
```

---

## Data Flow

### Recording Start Sequence

1. User presses hotkey
2. Hotkey thread sends `Pressed` to Main
3. Main sends `Start` to Audio thread
4. Audio thread starts wavy microphone, returns `Started(stream)`
5. Main clones stream into `stream1` and `stream2`
6. Main sends `ProcessStream(stream1)` to Transcription thread
7. Transcription creates token queue, returns `TokenQueue(queue)`
8. Main sends `Show(stream2)` to UI thread
9. Main spawns relay loop: reads tokens from queue → sends to XDO
10. UI shows window and visualizes audio from stream
11. Transcription processes stream → emits tokens to queue
12. Main relays tokens to XDO thread
13. XDO types text into focused application

### Recording Stop Sequence

1. User releases hotkey
2. Hotkey thread sends `Released` to Main
3. Main sends `Stop` to Audio thread
4. Audio thread stops wavy, closes all streams
5. UI detects closed stream → hides window automatically
6. Transcription detects closed stream → closes token queue
7. Main's relay loop exits (queue closed)
8. System returns to idle state

---

## Error Handling

**Philosophy:** Fail-fast with no recovery.

All operations use `.unwrap_or_exit()` which:
- Prints error message to stderr
- Calls `exit(1)`
- Terminates entire process immediately

**No error recovery:**
- No retry logic
- No fallback mechanisms
- No error propagation
- Clean failure is better than undefined state

---

## Technology Stack

### Core Libraries

| Component | Library | Purpose |
|-----------|---------|---------|
| Audio Capture | `wavy` | High-level async microphone access (32kHz mono) |
| Transcription | `whisper-rs` | Rust bindings to whisper.cpp |
| UI | `egui` + `eframe` | Immediate-mode GUI with waveform/spectrum |
| FFT | `rustfft` | Fast Fourier Transform for spectrum analysis |
| Hotkey | `evdev` | Linux input device monitoring |
| XDO | `ydotool` / `xdotool` CLI | Text injection (X11/Wayland) |
| Clipboard | `arboard` | Fallback text injection method |
| Async Runtime | `tokio` | Multi-threaded async runtime |
| Channels | `tokio::sync::mpsc` | Message passing between threads |

### Audio Format

- **Sample Rate:** 32kHz
- **Channels:** Mono
- **Format:** Native wavy stream type (likely f32 samples)
- **Stream Type:** Async broadcast stream (clonable)

---

## Design Decisions

### Why Stream-Based Instead of Chunk Messages?

- **Natural backpressure:** Each consumer reads at its own pace
- **Simpler lifecycle:** Stream close = end of recording
- **Zero-copy sharing:** `Arc` or stream cloning avoids copying audio data
- **Cleaner code:** No need to model individual chunks as messages

### Why Transcription Returns Token Queue?

- **Main thread visibility:** All data flow goes through Main
- **Decoupling:** Transcription doesn't know about XDO
- **Clean separation:** Producer (Transcription) → Relay (Main) → Consumer (XDO)
- **Natural completion:** Queue closure signals end of transcription

### Why UI Auto-Hides on Stream Close?

- **Fewer messages:** No explicit Hide command needed
- **Automatic cleanup:** UI lifecycle tied to stream lifecycle
- **Simpler logic:** Stream close is the only signal needed

### Why No Error Handling?

- **Simplicity:** Less code, easier to understand
- **Reliability:** Fail-fast prevents undefined states
- **Development speed:** Focus on happy path, not edge cases
- **Recovery unlikely:** Most errors are non-recoverable anyway

### Why 32kHz Mono?

- **Whisper requirement:** Whisper models expect 16kHz, 32kHz is 2x oversampling
- **Quality vs size:** Good balance for speech recognition
- **Simplicity:** One fixed format, no configuration needed
- **Compatibility:** Works with all microphones

---

## Implementation Notes

### Thread Spawning

All threads should be spawned at startup and run for the entire application lifetime. Use `std::thread::spawn` or `tokio::task::spawn` depending on async requirements.

### Channel Types

- Use `tokio::sync::mpsc` for request-response pairs
- Use `tokio::sync::oneshot` if only single response expected
- Stream cloning handled by wavy library

### Stream Handling

Audio streams are wavy's native type. The exact type depends on the library but should support:
- `clone()` - Create independent consumers
- `next()` - Async iteration
- Automatic closure when audio stops

### FFT Visualization

UI thread should:
- Read audio at 60fps (16ms timeout)
- Perform FFT on each chunk
- Convert to log-scale frequency bins (50 bars)
- Update waveform circular buffer (last 2 seconds)

### Token Buffering

Transcription should:
- Buffer 3 seconds of audio before processing
- Send tokens as they become available
- Flush remaining buffer on stream close

---

## Future Enhancements

Potential improvements (not part of v1):

- **VAD optimization:** Skip silence before sending to Whisper
- **Model selection:** Runtime model switching (tiny/base/small)
- **Configuration:** Hotkey customization, audio device selection
- **Streaming transcription:** Incremental Whisper decoding
- **Multi-language:** Automatic language detection
- **Punctuation:** Add punctuation to transcribed text

---

## Building & Running

### Dependencies

```toml
[dependencies]
wavy = "0.9"
whisper-rs = "0.11"
egui = "0.28"
eframe = "0.28"
rustfft = "6.2"
evdev = "0.12"
tokio = { version = "1", features = ["full"] }
arboard = "3.3"
```

### Build

```bash
cargo build --release
```

### Run

```bash
# Ensure ydotool is running
sudo systemctl start ydotool

# Run application
./target/release/whisperme
```

### Hotkey

Default: `Ctrl+Shift+Space`

- Press to start recording
- Release to stop and transcribe
- Text appears in focused application

---

## License

See LICENSE file in repository.
