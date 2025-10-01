mod watcher;
mod processor;
mod backends;
mod config;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use tracing::{info, error};
use config::{Config, BackendConfig, FileTypeConfig};
use processor::{Processor, ProcessedContent};
#[allow(unused_imports)]
use processor::ProcessedContent as _;

#[derive(Parser)]
#[command(name = "scribe")]
#[command(about = "Processes media files to generate transcripts or descriptions")]
struct Args {
    #[arg(short, long, default_value = "openai", help = "Backend to use (openai, whisper, ort, vision)")]
    backend: String,

    #[arg(short, long, help = "OpenAI API key (or set OPENAI_API_KEY env var)")]
    api_key: Option<String>,

    #[arg(short, long, help = "Path to Whisper model file (for whisper backend)")]
    model_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Watch a directory for media files and process them
    Path {
        /// Directory to watch for media files
        directory: PathBuf,
    },
    /// Process a single file and exit
    File {
        /// File to process
        file: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if it exists
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    let api_key = args.api_key.or_else(|| std::env::var("OPENAI_API_KEY").ok());

    info!("Starting scribe with backend: {}", args.backend);

    let backend = backends::create_backend(&args.backend, api_key.clone(), args.model_path)?;

    match args.command {
        Commands::Path { directory } => {
            if !directory.exists() {
                error!("Watch directory does not exist: {:?}", directory);
                return Err(anyhow::anyhow!("Watch directory does not exist"));
            }

            let config = Config {
                watch_dir: directory.clone(),
                backend: BackendConfig {
                    backend_type: args.backend.clone(),
                    api_key: api_key.clone(),
                },
                file_types: FileTypeConfig::default(),
            };

            info!("Watching directory: {:?}", config.watch_dir);
            info!("Supported audio extensions: {:?}", config.file_types.audio_extensions);
            info!("Supported video extensions: {:?}", config.file_types.video_extensions);
            info!("Supported image extensions: {:?}", config.file_types.image_extensions);

            let (tx, rx) = tokio::sync::mpsc::channel(100);

            let file_types = config.file_types.clone();
            let watch_dir = config.watch_dir.clone();
            let watcher_handle = tokio::spawn(async move {
                if let Err(e) = watcher::watch_directory(watch_dir, tx, file_types).await {
                    error!("Watcher error: {}", e);
                }
            });

            let processor_handle = tokio::spawn(async move {
                processor::process_files(rx, backend).await;
            });

            tokio::select! {
                _ = watcher_handle => {
                    info!("Watcher stopped");
                }
                _ = processor_handle => {
                    info!("Processor stopped");
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Received Ctrl+C, shutting down...");
                }
            }
        }
        Commands::File { file } => {
            if !file.exists() {
                error!("File does not exist: {:?}", file);
                return Err(anyhow::anyhow!("File does not exist"));
            }

            info!("Processing single file: {:?}", file);

            let result = backend.process(&file).await?;

            // Prepare output data
            let timestamp = chrono::Utc::now().to_rfc3339();
            let output = serde_json::json!({
                "file": file.to_string_lossy(),
                "backend": backend.name(),
                "timestamp": &timestamp,
                "content": &result,
            });

            let parent = file.parent().unwrap_or(std::path::Path::new("."));
            let stem = file.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("output");

            // Save JSON output
            let json_path = parent.join(format!("{}-scribe.json", stem));
            std::fs::write(&json_path, serde_json::to_string_pretty(&output)?)?;
            info!("JSON output saved to: {:?}", json_path);

            // Save Markdown output
            let md_path = parent.join(format!("{}-scribe.md", stem));
            let markdown = format_file_result_as_markdown(&file, backend.name(), &timestamp, &result);
            std::fs::write(&md_path, markdown)?;
            info!("Markdown output saved to: {:?}", md_path);

            // Also print to stdout for immediate feedback
            println!("\n=== Processing Result ===");
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }

    Ok(())
}

fn format_file_result_as_markdown(
    file_path: &Path,
    backend_name: &str,
    timestamp: &str,
    content: &ProcessedContent,
) -> String {
    let mut markdown = String::new();

    // Header
    markdown.push_str("# Scribe Processing Result\n\n");

    // Metadata
    markdown.push_str("## Metadata\n\n");
    markdown.push_str(&format!("- **File**: `{}`\n", file_path.display()));
    let file_type = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown");
    markdown.push_str(&format!("- **File Type**: {}\n", file_type));
    markdown.push_str(&format!("- **Backend**: {}\n", backend_name));
    markdown.push_str(&format!("- **Timestamp**: {}\n\n", timestamp));

    // Content
    markdown.push_str("## Content\n\n");

    match content {
        ProcessedContent::Transcript { text, language, duration_ms } => {
            markdown.push_str("### Transcript\n\n");
            if let Some(lang) = language {
                markdown.push_str(&format!("**Language**: {}\n\n", lang));
            }
            if let Some(duration) = duration_ms {
                let seconds = duration / 1000;
                let minutes = seconds / 60;
                let remaining_seconds = seconds % 60;
                markdown.push_str(&format!("**Duration**: {}:{:02}\n\n", minutes, remaining_seconds));
            }
            markdown.push_str("---\n\n");
            markdown.push_str(text);
            markdown.push_str("\n");
        }
        ProcessedContent::Description { description, tags } => {
            markdown.push_str("### Image Description\n\n");
            markdown.push_str(description);
            markdown.push_str("\n\n");

            if !tags.is_empty() {
                markdown.push_str("### Tags\n\n");
                for tag in tags {
                    markdown.push_str(&format!("- {}\n", tag));
                }
            }
        }
    }

    markdown
}