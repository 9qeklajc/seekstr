use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{error, info};
use url::Url;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessingResult {
    pub url: String,
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
        summary: Option<String>,
    },
    Description {
        description: String,
        tags: Vec<String>,
    },
}

/// File type classification for URL-based processing
#[derive(Debug, Clone, PartialEq)]
pub enum FileType {
    Audio,
    Video,
    Image,
    YouTube,
    Unknown,
}

/// Check if a string is a valid HTTP/HTTPS URL
fn is_http_url(url: &str) -> bool {
    match Url::parse(url) {
        Ok(u) => u.scheme() == "http" || u.scheme() == "https",
        Err(_) => false,
    }
}

/// Check if a string is a valid file:// URL
fn is_file_url(url: &str) -> bool {
    match Url::parse(url) {
        Ok(u) => u.scheme() == "file",
        Err(_) => false,
    }
}

/// Check if a URL is a YouTube URL
fn is_youtube_url(url: &str) -> bool {
    let url_lower = url.to_lowercase();
    url_lower.contains("youtube.com/watch")
        || url_lower.contains("youtu.be/")
        || url_lower.contains("youtube.com/embed/")
        || url_lower.contains("youtube.com/v/")
}

/// Determine file type from URL based on extension and MIME type patterns
pub fn get_file_type_from_url(url: &str) -> FileType {
    if !is_http_url(url) && !is_file_url(url) {
        return FileType::Unknown;
    }

    // Check for YouTube URLs first
    if is_youtube_url(url) {
        return FileType::YouTube;
    }

    let url_lower = url.to_lowercase();

    // Audio extensions
    if url_lower.contains(".mp3")
        || url_lower.contains(".wav")
        || url_lower.contains(".flac")
        || url_lower.contains(".aac")
        || url_lower.contains(".ogg")
        || url_lower.contains(".m4a")
        || url_lower.contains(".webm")
    {
        return FileType::Audio;
    }

    // Video extensions and patterns
    if url_lower.contains(".mp4")
        || url_lower.contains(".avi")
        || url_lower.contains(".mov")
        || url_lower.contains(".mkv")
        || url_lower.contains(".wmv")
        || url_lower.contains(".m4v")
        || url_lower.contains(".ogv")
        || url_lower.contains(".m3u8")
    {
        return FileType::Video;
    }

    // Image extensions
    if url_lower.contains(".jpg")
        || url_lower.contains(".jpeg")
        || url_lower.contains(".png")
        || url_lower.contains(".gif")
        || url_lower.contains(".bmp")
        || url_lower.contains(".webp")
    {
        return FileType::Image;
    }

    FileType::Unknown
}

/// Extract file type string for output
pub fn get_file_type_string(url: &str) -> String {
    match get_file_type_from_url(url) {
        FileType::Audio => "audio".to_string(),
        FileType::Video => "video".to_string(),
        FileType::Image => "image".to_string(),
        FileType::YouTube => "youtube".to_string(),
        FileType::Unknown => {
            // Try to extract extension from URL path
            if let Ok(parsed_url) = Url::parse(url)
                && let Some(mut path) = parsed_url.path_segments()
                && let Some(last_segment) = path.next_back()
                && let Some(dot_pos) = last_segment.rfind('.')
            {
                return last_segment[dot_pos + 1..].to_string();
            }
            "unknown".to_string()
        }
    }
}

#[async_trait]
pub trait Processor: Send + Sync {
    async fn process(&self, url: &str) -> Result<ProcessedContent>;
    fn name(&self) -> &str;
}

/// Generate a descriptive summary of transcribed content for better searchability
pub async fn generate_summary(transcript: &str, api_key: &str) -> Result<String> {
    let client = reqwest::Client::new();

    let prompt = format!(
        "Please create a descriptive and comprehensive summary of the following transcript. Focus on key topics, important details, and themes that would help someone find this content through search. Use descriptive language and include specific details mentioned in the content. Make the summary searchable by including relevant keywords and context.\n\nTranscript:\n{}",
        transcript
    );

    let payload = serde_json::json!({
        "model": "gpt-3.5-turbo",
        "messages": [
            {
                "role": "user",
                "content": prompt
            }
        ],
        "max_tokens": 300,
        "temperature": 0.7
    });

    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!("OpenAI API error: {}", error_text));
    }

    let response_json: serde_json::Value = response.json().await?;

    let summary = response_json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Failed to extract summary from OpenAI response"))?
        .trim()
        .to_string();

    Ok(summary)
}

pub async fn process_urls(mut rx: mpsc::Receiver<String>, backend: &dyn Processor) {
    info!("Processor started with backend: {}", backend.name());

    while let Some(url) = rx.recv().await {
        info!("Received URL from queue: {}", url);
        info!("Passing to backend '{}': {}", backend.name(), url);

        match process_single_url(&url, backend).await {
            Ok(result) => {
                info!("✓ Processing complete: {}", url);
                info!("  Result: {:?}", result);
            }
            Err(e) => {
                error!("✗ Processing failed for {}: {}", url, e);
            }
        }
    }

    info!("Processor shutting down");
}

async fn process_single_url(url: &str, backend: &dyn Processor) -> Result<ProcessingResult> {
    info!("Backend processing started: {}", url);
    let start_time = std::time::Instant::now();

    let content = backend.process(url).await?;

    let processing_time_ms = start_time.elapsed().as_millis();
    info!("Backend processing completed in {}ms", processing_time_ms);

    let file_type = get_file_type_string(url);

    let result = ProcessingResult {
        url: url.to_string(),
        file_type,
        backend_used: backend.name().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        content,
    };

    info!(
        "Processing complete ({}ms total)",
        start_time.elapsed().as_millis()
    );

    Ok(result)
}

/// Process a single URL and return the result directly (for single-file processing)
pub async fn process_single_url_direct(
    url: &str,
    backend: &dyn Processor,
) -> Result<ProcessingResult> {
    process_single_url(url, backend).await
}

pub fn format_as_markdown(result: &ProcessingResult) -> String {
    let mut markdown = String::new();

    // Header
    markdown.push_str("# Scribe Processing Result\n\n");

    // Metadata
    markdown.push_str("## Metadata\n\n");
    markdown.push_str(&format!("- **URL**: `{}`\n", result.url));
    markdown.push_str(&format!("- **File Type**: {}\n", result.file_type));
    markdown.push_str(&format!("- **Backend**: {}\n", result.backend_used));
    markdown.push_str(&format!("- **Timestamp**: {}\n\n", result.timestamp));

    // Content
    markdown.push_str("## Content\n\n");

    match &result.content {
        ProcessedContent::Transcript {
            text,
            language,
            duration_ms,
            summary,
        } => {
            if let Some(summary_text) = summary {
                markdown.push_str("### Summary\n\n");
                markdown.push_str(summary_text);
                markdown.push_str("\n\n");
            }

            markdown.push_str("### Transcript\n\n");
            if let Some(lang) = language {
                markdown.push_str(&format!("**Language**: {}\n\n", lang));
            }
            if let Some(duration) = duration_ms {
                let seconds = duration / 1000;
                let minutes = seconds / 60;
                let remaining_seconds = seconds % 60;
                markdown.push_str(&format!(
                    "**Duration**: {}:{:02}\n\n",
                    minutes, remaining_seconds
                ));
            }
            markdown.push_str("---\n\n");
            markdown.push_str(text);
            markdown.push('\n');
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
