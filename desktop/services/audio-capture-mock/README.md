# audio-capture-mock

Mock implementation of audio capture service for testing.

## Setup

Before building, you need to place a WAV file at:

```
src/assets/audio.wav
```

**Requirements:**
- Sample rate: 48000Hz
- Channels: 2 (stereo)
- Format: Any (will be automatically converted to f32)

### Example: Creating a test WAV file with ffmpeg

```bash
# Generate 1 second of 440Hz sine wave
ffmpeg -f lavfi -i "sine=frequency=440:duration=1" -ar 48000 -ac 2 src/assets/audio.wav
```

## Usage

```rust
let (frame_tx, frame_rx) = mpsc::channel(100);
let (command_tx, command_rx) = mpsc::channel(10);

let service = AudioCaptureService::new(frame_tx, command_rx);
tokio::spawn(async move { service.run().await });

// Start capturing
command_tx.send(AudioCaptureMessage::Start { hwnd: 12345 }).await?;

// Receive frames...
while let Some(frame) = frame_rx.recv().await {
    // Process audio frame
}
```
