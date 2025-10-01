use crate::processor::{ProcessedContent, Processor};
use anyhow::Result;
use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{debug, info};

pub struct VisionBackend {
    api_key: String,
    api_url: String,
    model: String,
}

impl VisionBackend {
    pub fn new(api_key: String, api_url: String, model: String) -> Self {
        info!("Initializing Vision backend:");
        info!("  API URL: {}", api_url);
        info!("  Model: {}", model);
        info!("  API Key: {}...", &api_key[..api_key.len().min(10)]);

        Self {
            api_key,
            api_url,
            model,
        }
    }

    async fn prepare_image(&self, file_path: &Path) -> Result<Vec<u8>> {
        use std::process::Command;

        info!("Resizing image to max 1120x1120 for vision model compatibility");

        // Always resize to ensure image fits within 1120x1120
        // The ">" flag means only shrink if larger than specified size
        let output = Command::new("magick")
            .args(&[
                file_path.to_str().unwrap(),
                "-resize",
                "1120x1120>", // Resize only if larger than 1120px
                "-quality",
                "90",    // Keep good quality
                "PNG:-", // Output to stdout as PNG
            ])
            .output()?;

        if !output.status.success() {
            // Fallback to convert command
            let output = Command::new("convert")
                .args(&[
                    file_path.to_str().unwrap(),
                    "-resize",
                    "1120x1120>",
                    "-quality",
                    "90",
                    "PNG:-",
                ])
                .output()?;

            if !output.status.success() {
                return Err(anyhow::anyhow!(
                    "Failed to resize image: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }

            Ok(output.stdout)
        } else {
            Ok(output.stdout)
        }
    }

    async fn describe_image(&self, file_path: &Path) -> Result<String> {
        // Read and potentially resize the image
        let image_data = self.prepare_image(file_path).await?;
        let base64_image = STANDARD.encode(&image_data);

        let extension = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png");

        let mime_type = match extension {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            _ => "image/jpeg",
        };

        let client = reqwest::Client::new();

        let request_body = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "What is in this image? Describe it in detail."
                        },
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:{};base64,{}", mime_type, base64_image)
                            }
                        }
                    ]
                }
            ],
            "max_tokens": 1000,
            "temperature": 0
        });

        let url = if self.api_url.ends_with("/") {
            format!("{}v1/chat/completions", self.api_url)
        } else if self.api_url.ends_with("/v1") {
            format!("{}/chat/completions", self.api_url)
        } else if self.api_url.ends_with("/chat/completions") {
            self.api_url.clone()
        } else {
            format!("{}/v1/chat/completions", self.api_url)
        };

        info!("Making vision API request:");
        info!("  URL: {}", url);
        info!("  Model: {}", self.model);
        info!("  Image size: {} bytes", image_data.len());
        info!("  MIME type: {}", mime_type);
        debug!("  Base64 length: {} chars", base64_image.len());

        // Log a sample of the request for debugging
        let request_json = serde_json::to_string_pretty(&request_body)?;
        debug!(
            "Request body (first 500 chars): {}",
            &request_json[..request_json.len().min(500)]
        );

        let response = client
            .post(url.clone())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await?;
            info!("Error response from API: {}", error_text);
            info!("Request URL was: {}", url);
            return Err(anyhow::anyhow!(
                "Vision API request failed (status {}): {}",
                status,
                error_text
            ));
        }

        let response_data: VisionResponse = response.json().await?;

        Ok(response_data
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_else(|| "No description generated".to_string()))
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct VisionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Deserialize, Serialize)]
struct Message {
    content: String,
}

#[async_trait]
impl Processor for VisionBackend {
    async fn process(&self, file_path: &Path) -> Result<ProcessedContent> {
        info!("Vision backend processing: {:?}", file_path);

        let extension = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

        match extension {
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" => {
                let description = self.describe_image(file_path).await?;
                Ok(ProcessedContent::Description {
                    description,
                    tags: vec![],
                })
            }
            "mp3" | "mp4" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "webm" | "avi" | "mov"
            | "mkv" | "wmv" => Ok(ProcessedContent::Description {
                description: "Vision backend cannot process audio/video files".to_string(),
                tags: vec!["unsupported".to_string()],
            }),
            _ => Err(anyhow::anyhow!("Unsupported file type: {}", extension)),
        }
    }

    fn name(&self) -> &str {
        "vision"
    }
}
