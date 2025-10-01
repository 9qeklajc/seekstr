use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::{error, info};

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessingResult {
    pub file_path: String,
    pub file_type: String,
    pub backend_used: String,
    pub timestamp: String,
    pub content: ProcessedContent,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProcessedContent {
    Transcript {
        text: String,
        language: Option<String>,
        duration_ms: Option<u64>,
    },
    Description {
        description: String,
        tags: Vec<String>,
    },
}

#[async_trait]
pub trait Processor: Send + Sync {
    async fn process(&self, file_path: &Path) -> Result<ProcessedContent>;
    fn name(&self) -> &str;
}

pub async fn process_files(
    mut rx: mpsc::Receiver<PathBuf>,
    backend: Box<dyn Processor>,
) {
    info!("Processor started with backend: {}", backend.name());

    while let Some(file_path) = rx.recv().await {
        info!("Received file from queue: {:?}", file_path);
        info!("Passing to backend '{}': {:?}", backend.name(), file_path);

        match process_single_file(&file_path, &backend).await {
            Ok(output_path) => {
                info!("✓ Processing complete: {:?}", file_path);
                info!("  Output saved to: {:?}", output_path);
            }
            Err(e) => {
                error!("✗ Processing failed for {:?}: {}", file_path, e);
            }
        }
    }

    info!("Processor shutting down");
}

async fn process_single_file(
    file_path: &Path,
    backend: &Box<dyn Processor>,
) -> Result<PathBuf> {
    info!("Backend processing started: {:?}", file_path);
    let start_time = std::time::Instant::now();

    let content = backend.process(file_path).await?;

    let processing_time_ms = start_time.elapsed().as_millis();
    info!("Backend processing completed in {}ms", processing_time_ms);

    let file_type = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown")
        .to_string();

    let result = ProcessingResult {
        file_path: file_path.to_string_lossy().to_string(),
        file_type,
        backend_used: backend.name().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        content,
    };

    info!("Preparing output files for: {:?}", file_path);

    // Save JSON output
    let json_output_path = get_output_path(file_path, "json");
    let json = serde_json::to_string_pretty(&result)?;
    info!("Writing JSON to: {:?}", json_output_path);
    tokio::fs::write(&json_output_path, json).await?;

    // Save Markdown output
    let md_output_path = get_output_path(file_path, "md");
    let markdown = format_as_markdown(&result);
    info!("Writing Markdown to: {:?}", md_output_path);
    tokio::fs::write(&md_output_path, markdown).await?;

    info!("Results successfully saved ({}ms total)", start_time.elapsed().as_millis());

    Ok(json_output_path)
}

fn get_output_path(input_path: &Path, extension: &str) -> PathBuf {
    let parent = input_path.parent().unwrap_or(Path::new("."));
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    parent.join(format!("{}-scribe.{}", stem, extension))
}

fn format_as_markdown(result: &ProcessingResult) -> String {
    let mut markdown = String::new();

    // Header
    markdown.push_str(&format!("# Scribe Processing Result\n\n"));

    // Metadata
    markdown.push_str("## Metadata\n\n");
    markdown.push_str(&format!("- **File**: `{}`\n", result.file_path));
    markdown.push_str(&format!("- **File Type**: {}\n", result.file_type));
    markdown.push_str(&format!("- **Backend**: {}\n", result.backend_used));
    markdown.push_str(&format!("- **Timestamp**: {}\n\n", result.timestamp));

    // Content
    markdown.push_str("## Content\n\n");

    match &result.content {
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