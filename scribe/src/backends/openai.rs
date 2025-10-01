use crate::processor::{
    FileType, ProcessedContent, Processor, generate_summary, get_file_type_from_url,
};
use anyhow::Result;
use async_trait::async_trait;
use base64::Engine;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

pub struct OpenAIBackend {
    api_key: String,
    client: reqwest::Client,
}

impl OpenAIBackend {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    async fn transcribe_audio(&self, url: &str) -> Result<String> {
        info!("OpenAI: Transcribing audio from URL: {}", url);
        let file_bytes = self.download_file(url).await?;
        info!("OpenAI: File downloaded, size: {} bytes", file_bytes.len());

        let form = reqwest::multipart::Form::new()
            .text("model", "whisper-1")
            .part(
                "file",
                reqwest::multipart::Part::bytes(file_bytes)
                    .file_name(self.extract_filename_from_url(url))
                    .mime_str("audio/mpeg")?,
            );

        info!("OpenAI: Sending request to Whisper API");
        let response = self
            .client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await?;

        info!("OpenAI: Response received, parsing transcript");
        let result: TranscriptionResponse = response.json().await?;
        info!("OpenAI: Transcript ready, {} characters", result.text.len());
        Ok(result.text)
    }

    async fn describe_image(&self, url: &str) -> Result<String> {
        info!("OpenAI: Describing image from URL: {}", url);
        let image_bytes = self.download_file(url).await?;
        info!(
            "OpenAI: Image downloaded, size: {} bytes",
            image_bytes.len()
        );
        let base64_image = base64::engine::general_purpose::STANDARD.encode(&image_bytes);

        let mime_type = self.get_mime_type_from_url(url);

        let request_body = VisionRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![VisionMessage {
                role: "user".to_string(),
                content: vec![
                    VisionContent::Text {
                        text: "Describe this image in detail. Include objects, people, text, colors, and scene context.".to_string(),
                    },
                    VisionContent::ImageUrl {
                        image_url: ImageUrl {
                            url: format!("data:{};base64,{}", mime_type, base64_image),
                        },
                    },
                ],
            }],
            max_tokens: 500,
        };

        info!("OpenAI: Sending request to Vision API");
        let response = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request_body)
            .send()
            .await?;

        info!("OpenAI: Response received, parsing description");
        let result: VisionResponse = response.json().await?;
        let description = result.choices[0].message.content.clone();
        info!(
            "OpenAI: Description ready, {} characters",
            description.len()
        );
        Ok(description)
    }

    async fn download_file(&self, url: &str) -> Result<Vec<u8>> {
        info!("Getting file from URL: {}", url);

        if url.starts_with("file://") {
            // Handle local file URLs
            let file_path = &url[7..]; // Remove "file://" prefix
            let bytes = tokio::fs::read(file_path).await?;
            Ok(bytes)
        } else {
            // Handle HTTP/HTTPS URLs
            let response = self.client.get(url).send().await?;

            if !response.status().is_success() {
                return Err(anyhow::anyhow!(
                    "Failed to download file: HTTP {}",
                    response.status()
                ));
            }

            let bytes = response.bytes().await?;
            Ok(bytes.to_vec())
        }
    }

    fn extract_filename_from_url(&self, url: &str) -> String {
        if let Ok(parsed_url) = url::Url::parse(url) {
            if let Some(path) = parsed_url.path_segments() {
                if let Some(filename) = path.last() {
                    if !filename.is_empty() {
                        return filename.to_string();
                    }
                }
            }
        }
        "file".to_string()
    }

    fn get_mime_type_from_url(&self, url: &str) -> &'static str {
        let url_lower = url.to_lowercase();
        if url_lower.contains(".jpg") || url_lower.contains(".jpeg") {
            "image/jpeg"
        } else if url_lower.contains(".png") {
            "image/png"
        } else if url_lower.contains(".gif") {
            "image/gif"
        } else if url_lower.contains(".webp") {
            "image/webp"
        } else if url_lower.contains(".bmp") {
            "image/bmp"
        } else {
            "image/jpeg"
        }
    }
}

#[async_trait]
impl Processor for OpenAIBackend {
    async fn process(&self, url: &str) -> Result<ProcessedContent> {
        let file_type = get_file_type_from_url(url);

        debug!("Processing URL with OpenAI: {}", url);

        match file_type {
            FileType::Audio | FileType::Video => {
                let text = self.transcribe_audio(url).await?;

                // Generate summary for the transcription
                let summary = match generate_summary(&text, &self.api_key).await {
                    Ok(summary) => {
                        info!("Generated summary for transcription");
                        Some(summary)
                    }
                    Err(e) => {
                        info!("Failed to generate summary: {}", e);
                        None
                    }
                };

                Ok(ProcessedContent::Transcript {
                    text,
                    language: None,
                    duration_ms: None,
                    summary,
                })
            }
            FileType::Image => {
                let description = self.describe_image(url).await?;
                Ok(ProcessedContent::Description {
                    description,
                    tags: vec![],
                })
            }
            FileType::Unknown => Err(anyhow::anyhow!("Unsupported file type for URL: {}", url)),
        }
    }

    fn name(&self) -> &str {
        "openai"
    }
}

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

#[derive(Serialize)]
struct VisionRequest {
    model: String,
    messages: Vec<VisionMessage>,
    max_tokens: u32,
}

#[derive(Serialize)]
struct VisionMessage {
    role: String,
    content: Vec<VisionContent>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum VisionContent {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Serialize)]
struct ImageUrl {
    url: String,
}

#[derive(Deserialize)]
struct VisionResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}
