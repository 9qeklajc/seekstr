# Seekstr - Nostr Media Processing Service

A Nostr event processor that listens for events containing media file URLs, processes them using the Scribe transcription service, and publishes the results back to Nostr.

## Overview

Seekstr integrates with the EventFlow library to:
1. Listen for Nostr events from configured relays
2. Extract media file URLs from event content and tags
3. Process media files using Scribe (transcription/description)
4. Generate new Nostr events with processing results
5. Publish results to configured sink relays

## Architecture

### Components

- **main.rs**: Entry point that sets up EventFlow relay router and configuration
- **mediaprocessor.rs**: Custom processor implementing the EventFlow `Processor` trait
  - Extracts media URLs using regex patterns
  - Processes media through Scribe backends
  - Creates result events with proper tagging

### Supported Media Types

- **Audio**: mp3, wav, flac, aac, ogg, m4a, webm
- **Video**: mp4, avi, mov, mkv, wmv, m4v, ogv
- **Images**: jpg, jpeg, png, gif, bmp, svg, webp

## Configuration

Seekstr uses a TOML configuration file for all settings. By default, it looks for `config.toml` in the current directory.

### Configuration File

Copy `config.example.toml` to `config.toml` and customize:

```toml
[backend]
# Backend type: "openai", "whisper", or "auto"
type = "auto"
openai_api_key = "sk-..."  # Required for OpenAI backend
# whisper_model_path = "/path/to/model.bin"  # Required for Whisper backend

[relays]
# Source relays to listen for Nostr events
sources = [
    "wss://relay.damus.io",
    "wss://nos.lol",
    "wss://relay.nostr.band"
]

# Sink relays to publish processed results
sinks = [
    "wss://nostr.wine",
    "wss://relay.snort.social"
]

[processing]
state_file = "seekstr_state.json"
timeout_seconds = 30

[logging]
level = "info"
modules = ["seekstr", "eventflow", "scribe"]
```

### Backend Options

- **`openai`**: Uses OpenAI API for transcription and description
- **`whisper`**: Uses local Whisper model for offline processing
- **`auto`**: Automatically selects backend based on available credentials

### Custom Configuration Path

You can specify a custom configuration file location:

```bash
CONFIG_PATH=/path/to/custom/config.toml cargo run --package seekstr
```

## Running

```bash
# Build the project
cargo build --package seekstr

# Copy and configure the config file
cp config.example.toml config.toml
# Edit config.toml with your settings

# Run the application
cargo run --package seekstr

# Or run with a custom config file
CONFIG_PATH=/path/to/custom/config.toml cargo run --package seekstr
```

## Event Processing Flow

1. **Input Event**: Receives Nostr events from source relays
2. **URL Extraction**: Regex pattern matching for media URLs
3. **Media Processing**:
   - Downloads or accesses media file
   - Runs through Scribe processor (transcription/description)
4. **Result Event Creation**:
   - Kind 1 (text note) event
   - References original event with `e` tag
   - Includes processed URL and processor info in tags
   - Contains transcription/description in content
5. **Publication**: Sends result events to sink relays

## Example Result Event

```json
{
  "kind": 1,
  "content": "Media Processing Result\n\nOriginal Event: abc123...\nURL: https://example.com/audio.mp3\nType: mp3\nBackend: openai\n\nTranscript: ...",
  "tags": [
    ["e", "original_event_id"],
    ["processed-url", "https://example.com/audio.mp3"],
    ["processor", "scribe", "openai"]
  ]
}
```

## Dependencies

- **eventflow**: Nostr event routing and processing
- **scribe**: Media transcription/description backends
- **nostr**: Core Nostr protocol implementation
- **nostr-sdk**: High-level Nostr client library

## Future Improvements

- Custom processor configuration in EventFlow
- Batch processing optimization
- Result caching to avoid reprocessing
- Support for additional media types
- Configurable output formats