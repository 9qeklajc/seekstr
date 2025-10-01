# Scribe

A dead-simple Rust program that monitors directories for media files and generates transcripts or descriptions.

## Prerequisites

### Option 1: Using Nix (Recommended)
The easiest way to get a working development environment:

```bash
# Enter development shell with all dependencies
nix develop

# Or with direnv
direnv allow

# Build with Nix
nix build  # Builds with Whisper support
nix build .#scribe-no-whisper  # Without Whisper support
```

### Option 2: Manual Installation
For Whisper backend support:
- Install libclang: `sudo apt-get install libclang-dev` (Ubuntu/Debian) or `brew install llvm` (macOS)
- Download a Whisper model from https://huggingface.co/ggerganov/whisper.cpp

## Features

- Recursively watches a specified folder for new media files
- Supports multiple file types:
  - Audio: mp3, wav, flac, aac, ogg, m4a, webm
  - Video: mp4, avi, mov, mkv, wmv
  - Images: jpg, jpeg, png, gif, bmp, webp
- Multiple processing backends:
  - **OpenAI**: Full implementation using OpenAI API for transcription and image description
  - **Whisper**: Local whisper.cpp integration for offline audio/video transcription (optional feature)
  - **ORT**: Placeholder for ONNX Runtime integration
- Outputs results as JSON files with `-scribe.json` suffix
- Comprehensive logging for monitoring file processing pipeline

## Usage

```bash
# With OpenAI backend (default)
OPENAI_API_KEY=your-key-here cargo run -- /path/to/watch

# Or pass API key as argument
cargo run -- /path/to/watch --api-key your-key-here

# Use Whisper backend (local, no API needed)
cargo run -- /path/to/watch --backend whisper --model-path /path/to/ggml-base.bin

# Use different backend
cargo run -- /path/to/watch --backend ort

# Enable debug logging
RUST_LOG=debug cargo run -- /path/to/watch
```

### Whisper Model Setup

1. Download a model (e.g., base model):
```bash
mkdir -p ~/.cache/whisper
wget https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin -O ~/.cache/whisper/ggml-base.bin
```

2. Use with the whisper backend:
```bash
cargo run -- /path/to/watch --backend whisper
# Or specify custom model path:
cargo run -- /path/to/watch --backend whisper --model-path /custom/path/model.bin
```

## Build

```bash
# Basic build (without Whisper support)
cargo build --release

# With Whisper support (requires libclang-dev)
cargo build --release --features whisper
```

## Example Output

For a file `audio.mp3`, creates `audio-scribe.json`:

```json
{
  "file_path": "/path/to/audio.mp3",
  "file_type": "mp3",
  "backend_used": "openai",
  "timestamp": "2024-01-01T12:00:00Z",
  "content": {
    "text": "Transcribed text here...",
    "language": null,
    "duration_ms": null
  }
}
```

## Logging

The program provides detailed logging at each stage of processing:

1. **File Detection**: Logs when files are created or modified
2. **Queue Management**: Logs when files are queued for processing
3. **Backend Processing**: Logs when files are sent to and processed by backends
4. **Result Handling**: Logs when results are ready and saved

Enable different log levels:
```bash
# Info level (default) - shows major events
RUST_LOG=info cargo run -- /path/to/watch

# Debug level - shows detailed processing steps
RUST_LOG=debug cargo run -- /path/to/watch

# Trace level - shows everything
RUST_LOG=trace cargo run -- /path/to/watch

# Module-specific logging
RUST_LOG=scribe::watcher=debug,scribe::processor=info cargo run -- /path/to/watch
```

## Architecture

- **File Watcher**: Uses `notify` crate to monitor directories
- **Processor Interface**: Async trait for backend implementations
- **Channel-based**: Decoupled watcher and processor via Tokio channels
- **Graceful Shutdown**: Handles Ctrl+C for clean exit