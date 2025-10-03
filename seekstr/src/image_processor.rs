use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use eventflow::Processor;
use image::ImageFormat;
use nostr::{Event, EventBuilder, Keys, Kind, Tag};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Cursor;
use std::time::Duration;
use tracing::{debug, error, info};

pub struct ImageProcessor {
    api_url: String,
    api_key: String,
    model: String,
    client: reqwest::Client,
    url_regex: Regex,
    keys: Keys,
}

impl ImageProcessor {
    pub fn new(
        api_url: String,
        api_key: String,
        model: String,
        nsec: Option<String>,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        // Use provided nsec or generate new keys
        let keys = if let Some(nsec) = nsec {
            Keys::parse(&nsec)?
        } else {
            Keys::generate()
        };

        // Only match image extensions
        let pattern = r#"https?://[^\s<>"']+\.(?:jpg|jpeg|png|gif|bmp|svg|webp)(?:\?[^\s<>"']*)?"#;
        let url_regex = Regex::new(pattern)?;

        Ok(Self {
            api_url,
            api_key,
            model,
            client,
            url_regex,
            keys,
        })
    }

    fn resize_image_if_needed(&self, image_bytes: &[u8]) -> Result<Vec<u8>> {
        // Load the image
        let img = image::load_from_memory(image_bytes)?;

        // Check if resizing is needed
        let (width, height) = (img.width(), img.height());
        const MAX_SIZE: u32 = 1120;

        let resized_img = if width > MAX_SIZE || height > MAX_SIZE {
            // Calculate new dimensions maintaining aspect ratio
            let ratio = (MAX_SIZE as f32 / width.max(height) as f32).min(1.0);
            let new_width = (width as f32 * ratio) as u32;
            let new_height = (height as f32 * ratio) as u32;
            info!("Resizing image {}x{} -> {}x{}", width, height, new_width, new_height);

            // Resize the image with fast filtering
            img.resize(new_width, new_height, image::imageops::FilterType::Nearest)
        } else {
            img
        };

        // Convert back to bytes (as JPEG for efficiency)
        let mut output = Vec::new();
        let mut cursor = Cursor::new(&mut output);
        resized_img.write_to(&mut cursor, ImageFormat::Jpeg)?;
        Ok(output)
    }

    async fn process_image_url(&self, image_url: &str) -> Result<String> {
        info!("Processing image from URL: {}", image_url);

        // Download image
        let image_response = self.client.get(image_url).send().await?;
        let image_bytes = image_response.bytes().await?;
        info!("Downloaded image, size: {} bytes", image_bytes.len());

        // Resize if needed and convert to base64
        let processed_bytes = self.resize_image_if_needed(&image_bytes)?;
        let base64_image = STANDARD.encode(&processed_bytes);

        // Determine MIME type from URL
        let mime_type = self.get_mime_type_from_url(image_url);

        // Prepare the vision API request
        let request_body = json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "Describe image contents."
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

        // Build the API endpoint URL
        let url = if self.api_url.ends_with("/") {
            format!("{}v1/chat/completions", self.api_url)
        } else if self.api_url.ends_with("/v1") {
            format!("{}/chat/completions", self.api_url)
        } else if self.api_url.ends_with("/chat/completions") {
            self.api_url.clone()
        } else {
            format!("{}/v1/chat/completions", self.api_url)
        };

        info!("Sending request to vision API: {}", url);

        // Send request to vision API
        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Vision API request failed (status {}): {}", status, error_text);
        }

        let response_data: VisionResponse = response.json().await?;

        info!("Image description generated successfully");

        Ok(response_data
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_else(|| "No description generated".to_string()))
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

    fn extract_image_urls(&self, event: &Event) -> Vec<String> {
        let mut urls = Vec::new();

        // Check event content
        for mat in self.url_regex.find_iter(&event.content) {
            urls.push(mat.as_str().to_string());
        }

        // Check tags for URLs
        for tag in event.tags.iter() {
            let tag_content = tag.clone().to_vec();
            for part in tag_content.iter() {
                for mat in self.url_regex.find_iter(part) {
                    urls.push(mat.as_str().to_string());
                }
            }
        }

        // Deduplicate
        urls.sort();
        urls.dedup();
        urls
    }

    fn process_image_sync(&self, url: &str, original_event: &Event) -> Result<Event> {
        info!("Processing image URL: {}", url);

        // Use block_in_place to run async code in sync context
        let description = tokio::task::block_in_place(|| {
            let handle = tokio::runtime::Handle::current();
            handle.block_on(async {
                self.process_image_url(url).await
            })
        })?;

        // Create new event with tags (Kind 1 is a text note)
        let event = EventBuilder::new(Kind::from(1u16), description)
            .tag(Tag::event(original_event.id))
            .tag(Tag::parse(vec!["url", url])?)
            .sign_with_keys(&self.keys)?;

        info!("Created processed event {} for image {}", event.id, url);
        Ok(event)
    }
}

impl Processor for ImageProcessor {
    fn process(&self, event: &Event) -> Vec<Event> {
        let urls = self.extract_image_urls(event);

        if urls.is_empty() {
            debug!("No image URLs found in event {}, dropping", event.id);
            // Drop events without images
            return vec![];
        }

        info!("Found {} image URLs in event {}", urls.len(), event.id);

        // Start with the original event
        let mut results = vec![event.clone()];

        // Process each URL synchronously and add generated events
        for url in urls {
            match self.process_image_sync(&url, event) {
                Ok(processed_event) => {
                    info!("Successfully processed image: {}", url);
                    results.push(processed_event);
                }
                Err(e) => {
                    error!("Failed to process image {}: {}", url, e);
                }
            }
        }

        // Return original event plus any generated events
        results
    }

    fn name(&self) -> &str {
        "ImageProcessor"
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn shutdown(&self) {
        info!("ImageProcessor shutting down");
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