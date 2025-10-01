use crate::processor::{ProcessedContent, Processor};
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;
use tracing::info;

pub struct OrtBackend;

impl OrtBackend {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Processor for OrtBackend {
    async fn process(&self, file_path: &Path) -> Result<ProcessedContent> {
        info!("ORT (ONNX Runtime) backend processing: {:?}", file_path);

        let extension = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

        match extension {
            "mp3" | "mp4" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "webm" | "avi" | "mov"
            | "mkv" | "wmv" => Ok(ProcessedContent::Transcript {
                text: format!(
                    "ORT backend placeholder - would process audio/video: {}",
                    file_path.display()
                ),
                language: Some("unknown".to_string()),
                duration_ms: None,
            }),
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" => Ok(ProcessedContent::Description {
                description: format!(
                    "ORT backend placeholder - would process image: {}",
                    file_path.display()
                ),
                tags: vec!["ort".to_string(), "placeholder".to_string()],
            }),
            _ => Err(anyhow::anyhow!("Unsupported file type: {}", extension)),
        }
    }

    fn name(&self) -> &str {
        "ort"
    }
}
