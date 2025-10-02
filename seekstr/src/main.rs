mod config;
mod image_processor;

use anyhow::Result;
use config::Config;
use eventflow::{Config as EventFlowConfig, ProcessingState, RelayRouter, SubFilter};
use image_processor::ImageProcessor;
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

    info!("Starting Seekstr Image Processor for Nostr");
    info!("Configuration loaded from: {}", config_path);
    info!("Using Vision backend at: {}", config.backend.vision_api_url);

    // Create image processor with vision backend configuration
    let image_processor = Arc::new(ImageProcessor::new(
        config.backend.vision_api_url.clone(),
        config.backend.vision_api_key.clone(),
        config.backend.vision_model.clone(),
        config.backend.nsec.clone(),
    )?);

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
        .add_processor(image_processor, config.relays.sinks.clone())
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