use crate::processor::{ProcessedContent, Processor};
use anyhow::Result;
use async_trait::async_trait;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::Path;
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

    async fn transcribe_audio(&self, file_path: &Path) -> Result<String> {
        info!("OpenAI: Transcribing audio file: {:?}", file_path);
        let file_bytes = tokio::fs::read(file_path).await?;
        info!("OpenAI: File loaded, size: {} bytes", file_bytes.len());

        let form = reqwest::multipart::Form::new()
            .text("model", "whisper-1")
            .part(
                "file",
                reqwest::multipart::Part::bytes(file_bytes)
                    .file_name(file_path.file_name().unwrap().to_string_lossy().to_string())
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

    async fn describe_image(&self, file_path: &Path) -> Result<String> {
        info!("OpenAI: Describing image file: {:?}", file_path);
        let image_bytes = tokio::fs::read(file_path).await?;
        info!("OpenAI: Image loaded, size: {} bytes", image_bytes.len());
        let base64_image = base64::engine::general_purpose::STANDARD.encode(&image_bytes);

        let mime_type = match file_path.extension().and_then(|e| e.to_str()) {
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("png") => "image/png",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            _ => "image/jpeg",
        };

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
}

#[async_trait]
impl Processor for OpenAIBackend {
    async fn process(&self, file_path: &Path) -> Result<ProcessedContent> {
        let extension = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

        debug!("Processing file with OpenAI: {:?}", file_path);

        match extension {
            "mp3" | "mp4" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "webm" => {
                let text = self.transcribe_audio(file_path).await?;
                Ok(ProcessedContent::Transcript {
                    text,
                    language: None,
                    duration_ms: None,
                })
            }
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" => {
                let description = self.describe_image(file_path).await?;
                Ok(ProcessedContent::Description {
                    description,
                    tags: vec![],
                })
            }
            "avi" | "mov" | "mkv" | "wmv" => Ok(ProcessedContent::Description {
                description: "Video file processing not yet implemented for OpenAI backend"
                    .to_string(),
                tags: vec!["video".to_string()],
            }),
            _ => Err(anyhow::anyhow!("Unsupported file type: {}", extension)),
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
