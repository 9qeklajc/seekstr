mod openai;
mod ort;
mod vision;
mod whisper;

use crate::processor::{FileType, Processor, get_file_type_from_url};
use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

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

/// Automatically select the best backend based on the URL/file type
pub fn create_backend_auto(
    url: &str,
    api_key: Option<String>,
    model_path: Option<PathBuf>,
) -> Result<Box<dyn Processor>> {
    let file_type = get_file_type_from_url(url);

    let backend_type = match file_type {
        FileType::Image => {
            // Check if vision backend is configured, otherwise fall back to OpenAI
            if std::env::var("VISION_API_KEY").is_ok()
                && std::env::var("VISION_API_URL").is_ok()
                && std::env::var("VISION_MODEL").is_ok()
            {
                "vision"
            } else {
                "openai"
            }
        }
        FileType::Audio | FileType::Video => {
            // Try Whisper first, but check if it's viable
            // let whisper_path = model_path.clone().unwrap_or_else(|| {
            //     std::path::Path::new(&std::env::var("HOME").unwrap_or_default())
            //         .join(".cache/whisper/ggml-base.bin")
            // });

            // Check if Whisper is compiled and model exists
            #[cfg(feature = "whisper")]
            {
                "whisper"
            }
            #[cfg(not(feature = "whisper"))]
            {
                info!("Whisper not compiled, using OpenAI for audio/video");
                "openai"
            }
        }
        FileType::Unknown => {
            return Err(anyhow::anyhow!(
                "Cannot determine file type from URL: {}",
                url
            ));
        }
    };

    info!(
        "Auto-selected backend '{}' for file type: {:?}",
        backend_type, file_type
    );
    create_backend(backend_type, api_key, model_path)
}
