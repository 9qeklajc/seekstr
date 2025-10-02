use crate::processor::{FileType, ProcessedContent, Processor, get_file_type_from_url};
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

    #[allow(dead_code)]
    async fn prepare_image(&self, file_path: &Path) -> Result<Vec<u8>> {
        use std::process::Command;

        info!("Resizing image to max 1120x1120 for vision model compatibility");

        // Always resize to ensure image fits within 1120x1120
        // The ">" flag means only shrink if larger than specified size
        let output = Command::new("magick")
            .args([
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
                .args([
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

    #[allow(dead_code)]
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

    async fn describe_image_from_url(&self, url: &str) -> Result<String> {
        info!("Vision backend: processing image from URL: {}", url);

        // Download the image
        let image_bytes = self.download_file(url).await?;
        info!(
            "Vision backend: Image downloaded, size: {} bytes",
            image_bytes.len()
        );

        // Encode as base64
        let base64_image = STANDARD.encode(&image_bytes);

        // Determine MIME type from URL
        let mime_type = self.get_mime_type_from_url(url);

        // Create the request payload
        let request_body = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "Describe this image in detail. Include objects, people, text, colors, and scene context."
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
            "max_tokens": 500
        });

        info!("Vision backend: Sending request to API: {}", self.api_url);

        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/v1/chat/completions", self.api_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        let response_text = response.text().await?;

        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Vision API request failed (status {}): {}",
                status,
                response_text
            ));
        }

        let response_data: VisionResponse = serde_json::from_str(&response_text)?;

        info!("Vision backend: Description generated successfully");

        Ok(response_data
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_else(|| "No description generated".to_string()))
    }

    async fn download_file(&self, url: &str) -> Result<Vec<u8>> {
        info!("Vision backend: downloading file from URL: {}", url);

        let client = reqwest::Client::new();

        if url.starts_with("file://") {
            // Handle local file URLs
            let file_path = url.strip_prefix("file://").unwrap();
            let bytes = tokio::fs::read(file_path).await?;
            Ok(bytes)
        } else {
            // Handle HTTP/HTTPS URLs
            let response = client.get(url).send().await?;

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
    async fn process(&self, url: &str) -> Result<ProcessedContent> {
        info!("Vision backend processing: {}", url);

        let file_type = get_file_type_from_url(url);

        match file_type {
            FileType::Image => {
                let description = self.describe_image_from_url(url).await?;
                Ok(ProcessedContent::Description {
                    description,
                    tags: vec![],
                })
            }
            FileType::Audio | FileType::Video => Ok(ProcessedContent::Description {
                description: "Vision backend cannot process audio/video files".to_string(),
                tags: vec!["unsupported".to_string()],
            }),
            FileType::YouTube => Err(anyhow::anyhow!(
                "Vision backend cannot process YouTube URLs. Use the YouTube backend instead."
            )),
            FileType::Unknown => Err(anyhow::anyhow!("Unsupported file type for URL: {}", url)),
        }
    }

    fn name(&self) -> &str {
        "vision"
    }
}
