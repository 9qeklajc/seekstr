mod openai;
mod ort;
mod vision;
mod whisper;

use crate::processor::Processor;
use anyhow::Result;
use std::path::PathBuf;

pub fn create_backend(
    backend_type: &str,
    api_key: Option<String>,
    model_path: Option<PathBuf>,
) -> Result<Box<dyn Processor>> {
    match backend_type.to_lowercase().as_str() {
        "openai" => {
            let api_key = api_key.ok_or_else(|| {
                anyhow::anyhow!(
                    "OpenAI backend requires an API key. Set OPENAI_API_KEY or use --api-key"
                )
            })?;
            Ok(Box::new(openai::OpenAIBackend::new(api_key)))
        }
        "whisper" => Ok(Box::new(whisper::WhisperBackend::new(model_path))),
        "ort" => Ok(Box::new(ort::OrtBackend::new())),
        "vision" => {
            let api_key = api_key
                .or_else(|| std::env::var("VISION_API_KEY").ok())
                .ok_or_else(|| {
                    anyhow::anyhow!("Vision backend requires VISION_API_KEY in .env or --api-key")
                })?;

            let api_url = std::env::var("VISION_API_URL").map_err(|_| {
                anyhow::anyhow!("Vision backend requires VISION_API_URL in .env file")
            })?;

            let model = std::env::var("VISION_MODEL").map_err(|_| {
                anyhow::anyhow!("Vision backend requires VISION_MODEL in .env file")
            })?;

            Ok(Box::new(vision::VisionBackend::new(
                api_key, api_url, model,
            )))
        }
        _ => Err(anyhow::anyhow!(
            "Unknown backend: {}. Available backends: openai, whisper, ort, vision",
            backend_type
        )),
    }
}
