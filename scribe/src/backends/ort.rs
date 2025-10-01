use crate::processor::{FileType, ProcessedContent, Processor, get_file_type_from_url};
use anyhow::Result;
use async_trait::async_trait;
use tracing::info;

pub struct OrtBackend;

impl OrtBackend {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Processor for OrtBackend {
    async fn process(&self, url: &str) -> Result<ProcessedContent> {
        info!("ORT (ONNX Runtime) backend processing: {}", url);

        let file_type = get_file_type_from_url(url);

        match file_type {
            FileType::Audio | FileType::Video => Ok(ProcessedContent::Transcript {
                text: format!(
                    "ORT backend placeholder - would process audio/video: {}",
                    url
                ),
                language: Some("unknown".to_string()),
                duration_ms: None,
                summary: None,
            }),
            FileType::Image => Ok(ProcessedContent::Description {
                description: format!("ORT backend placeholder - would process image: {}", url),
                tags: vec!["ort".to_string(), "placeholder".to_string()],
            }),
            FileType::Unknown => Err(anyhow::anyhow!("Unsupported file type for URL: {}", url)),
        }
    }

    fn name(&self) -> &str {
        "ort"
    }
}
