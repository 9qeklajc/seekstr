mod backends;
mod config;
mod processor;
mod watcher;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::{BackendConfig, Config, FileTypeConfig};
#[allow(unused_imports)]
use processor::ProcessedContent as _;
use std::path::PathBuf;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "scribe")]
#[command(
    about = "Processes media files to generate transcripts or descriptions with automatic backend selection"
)]
struct Args {
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

    let api_key = args
        .api_key
        .or_else(|| std::env::var("OPENAI_API_KEY").ok());

    info!("Starting scribe with automatic backend selection");

    match args.command {
        Commands::Path { directory } => {
            if !directory.exists() {
                error!("Watch directory does not exist: {:?}", directory);
                return Err(anyhow::anyhow!("Watch directory does not exist"));
            }

            // For directory watching, we'll use OpenAI as the default backend
            // since we can't determine file types until we see actual files
            let backend = backends::create_backend("openai", api_key.clone(), args.model_path)?;

            let config = Config {
                watch_dir: directory.clone(),
                backend: BackendConfig {
                    backend_type: "openai".to_string(),
                    api_key: api_key.clone(),
                },
                file_types: FileTypeConfig::default(),
            };

            info!("Watching directory: {:?}", config.watch_dir);
            info!(
                "Supported audio extensions: {:?}",
                config.file_types.audio_extensions
            );
            info!(
                "Supported video extensions: {:?}",
                config.file_types.video_extensions
            );
            info!(
                "Supported image extensions: {:?}",
                config.file_types.image_extensions
            );

            let (tx, rx) = tokio::sync::mpsc::channel::<String>(100);

            let file_types = config.file_types.clone();
            let watch_dir = config.watch_dir.clone();
            let watcher_handle = tokio::spawn(async move {
                if let Err(e) = watcher::watch_directory(watch_dir, tx, file_types).await {
                    error!("Watcher error: {}", e);
                }
            });

            let processor_handle = tokio::spawn(async move {
                processor::process_urls(rx, &*backend).await;
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
            let file_str = file.to_string_lossy();
            let url = if file_str.starts_with("http://") || file_str.starts_with("https://") {
                // It's already a URL
                file_str.to_string()
            } else {
                // It's a file path, check if it exists and convert to file URL
                if !file.exists() {
                    error!("File does not exist: {:?}", file);
                    return Err(anyhow::anyhow!("File does not exist"));
                }
                format!("file://{}", file.to_string_lossy())
            };

            info!("Processing file/URL: {}", url);

            // Automatically select backend based on file type
            let backend = backends::create_backend_auto(&url, api_key.clone(), args.model_path)?;
            let result = processor::process_single_url_direct(&url, &*backend).await?;

            let (parent, stem) = if url.starts_with("http") {
                // For URLs, extract filename and save in current directory
                let url_path = url.split('/').next_back().unwrap_or("output");
                let stem = if let Some(pos) = url_path.rfind('.') {
                    &url_path[..pos]
                } else {
                    url_path
                };
                (std::path::Path::new("."), stem)
            } else {
                // For file URLs, use original file path logic
                let parent = file.parent().unwrap_or(std::path::Path::new("."));
                let stem = file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("output");
                (parent, stem)
            };

            // Save JSON output
            let json_path = parent.join(format!("{}-scribe.json", stem));
            std::fs::write(&json_path, serde_json::to_string_pretty(&result)?)?;
            info!("JSON output saved to: {:?}", json_path);

            // Save Markdown output
            let md_path = parent.join(format!("{}-scribe.md", stem));
            let markdown = processor::format_as_markdown(&result);
            std::fs::write(&md_path, markdown)?;
            info!("Markdown output saved to: {:?}", md_path);

            // Also print to stdout for immediate feedback
            println!("\n=== Processing Result ===");
            println!("{}", serde_json::to_string_pretty(&result.content)?);
        }
    }

    Ok(())
}
