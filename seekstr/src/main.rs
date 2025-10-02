mod config;
mod mediaprocessor;

use anyhow::Result;
use config::{BackendType, Config};
use eventflow::{Config as EventFlowConfig, ProcessingState, RelayRouter, SubFilter};
use mediaprocessor::MediaProcessor;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize rustls crypto provider before any TLS connections
    // This is needed to avoid "Could not automatically determine the process-level CryptoProvider" error
    _ = rustls::crypto::ring::default_provider().install_default();

    // Load configuration from TOML file
    let config_path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "config.toml".to_string());
    let config = Config::load_or_default(&config_path)?;

    // Save default config if file didn't exist
    if !PathBuf::from(&config_path).exists() {
        info!("Creating default configuration file at: {}", config_path);
        config.save(&config_path)?;
    }

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(config.build_rust_log())
        .init();

    info!("Starting Seekstr Media Processor for Nostr");
    info!("Configuration loaded from: {}", config_path);

    // Determine which scribe backend to use based on configuration
    let backend = match config.backend.backend_type {
        BackendType::OpenAI => {
            let api_key = config.backend.openai_api_key
                .ok_or_else(|| anyhow::anyhow!("OpenAI API key not configured"))?;
            info!("Using OpenAI backend for media processing");
            scribe::backends::create_backend("openai", Some(api_key), None)?
        }
        BackendType::Whisper => {
            let model_path = config.backend.whisper_model_path
                .ok_or_else(|| anyhow::anyhow!("Whisper model path not configured"))?;
            info!("Using Whisper backend with model at: {}", model_path);
            scribe::backends::create_backend("whisper", None, Some(PathBuf::from(model_path)))?
        }
        BackendType::Auto => {
            if let Some(api_key) = config.backend.openai_api_key {
                info!("Auto mode: Using OpenAI backend");
                scribe::backends::create_backend("openai", Some(api_key), None)?
            } else if let Some(model_path) = config.backend.whisper_model_path {
                info!("Auto mode: Using Whisper backend with model at: {}", model_path);
                scribe::backends::create_backend("whisper", None, Some(PathBuf::from(model_path)))?
            } else {
                anyhow::bail!("Auto backend requires either openai_api_key or whisper_model_path to be configured");
            }
        }
    };

    // Create media processor
    let media_processor = Arc::new(MediaProcessor::new(backend)?);

    // Convert our filters to eventflow SubFilter format if they exist
    let event_filters = config.relays.filters.as_ref().map(|filters| {
        filters.iter().map(|f| {
            SubFilter {
                kinds: f.kinds.clone(),
                authors: f.authors.clone(),
                tags: HashMap::new(), // Could be extended to support tag filters
            }
        }).collect()
    });

    // Create EventFlow configuration with only sources (no sinks in config)
    let eventflow_config = EventFlowConfig {
        sources: config.relays.sources.clone(),
        filters: event_filters,
        sinks: vec![], // We'll add our custom processor via builder
        state_file: config.processing.state_file.clone(),
    };

    // Load or create processing state
    let state_path = PathBuf::from(&eventflow_config.state_file);
    let state = ProcessingState::load(&state_path).await.unwrap_or_else(|_| {
        info!("Creating new state file");
        ProcessingState::new()
    });

    // Create the relay router using builder pattern with custom processor
    let router = RelayRouter::builder(eventflow_config)
        .with_state(state)
        .add_processor(media_processor, config.relays.sinks.clone())
        .build()
        .await?;

    // Connect to all relays
    info!("Connecting to source relays: {:?}", config.relays.sources);
    info!("Will publish to sink relays: {:?}", config.relays.sinks);
    router.connect().await;

    // Set up graceful shutdown
    let router_clone = router.clone();
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("Received shutdown signal, saving state...");
                if let Err(e) = router_clone.save_state().await {
                    eprintln!("Error saving state: {}", e);
                }
                if let Err(e) = router_clone.disconnect().await {
                    eprintln!("Error disconnecting: {}", e);
                }
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("Error setting up signal handler: {}", e);
            }
        }
    });

    // Stream events continuously
    info!("Starting event stream...");

    loop {
        match router.stream_events().await {
            Ok(()) => {
                info!("Stream completed normally");
            }
            Err(e) => {
                eprintln!("Stream error: {}, retrying in 5 seconds...", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
    }
}