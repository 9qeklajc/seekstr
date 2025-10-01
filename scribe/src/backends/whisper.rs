#[cfg(feature = "whisper")]
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::processor::{ProcessedContent, Processor};
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tracing::info;
#[cfg(feature = "whisper")]
use tracing::warn;

pub struct WhisperBackend {
    #[allow(dead_code)]
    model_path: PathBuf,
}

#[cfg(feature = "whisper")]
fn convert_audio_to_pcm(file_path: &Path) -> Result<Vec<f32>> {
    use std::process::Command;

    // Use ffmpeg to convert to raw PCM: 16kHz, mono, f32le
    let output = Command::new("ffmpeg")
        .args(&[
            "-i",
            file_path.to_str().unwrap(),
            "-f",
            "f32le",
            "-ar",
            "16000",
            "-ac",
            "1",
            "-acodec",
            "pcm_f32le",
            "-",
        ])
        .output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "ffmpeg failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Convert bytes to f32 samples
    let mut samples = Vec::new();
    let mut chunks = output.stdout.chunks_exact(4);

    for chunk in chunks.by_ref() {
        let sample = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        samples.push(sample);
    }

    if !chunks.remainder().is_empty() {
        warn!("Audio data had {} extra bytes", chunks.remainder().len());
    }

    Ok(samples)
}

impl WhisperBackend {
    pub fn new(model_path: Option<PathBuf>) -> Self {
        let model_path = model_path.unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".cache/whisper/ggml-base.bin")
        });

        Self { model_path }
    }

    #[cfg(feature = "whisper")]
    async fn transcribe_file(&self, file_path: &Path) -> Result<String> {
        let model_path = self.model_path.clone();
        let file_path = file_path.to_path_buf();

        tokio::task::spawn_blocking(move || {
            // Convert audio file to PCM samples using ffmpeg
            let audio_data = convert_audio_to_pcm(&file_path)?;

            let ctx = WhisperContext::new_with_params(
                &model_path.to_string_lossy(),
                WhisperContextParameters::default(),
            )?;

            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            params.set_n_threads(4);
            params.set_translate(false);
            params.set_language(Some("auto"));
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);

            let mut state = ctx.create_state()?;
            state.full(params, &audio_data)?;

            let num_segments = state.full_n_segments()?;
            let mut text = String::new();

            for i in 0..num_segments {
                let segment = state.full_get_segment_text(i)?;
                text.push_str(&segment);
                text.push(' ');
            }

            Ok(text.trim().to_string())
        })
        .await?
    }

    #[cfg(not(feature = "whisper"))]
    async fn transcribe_file(&self, _file_path: &Path) -> Result<String> {
        Err(anyhow::anyhow!(
            "Whisper support not compiled. Build with --features whisper (requires libclang-dev)"
        ))
    }
}

#[async_trait]
impl Processor for WhisperBackend {
    async fn process(&self, file_path: &Path) -> Result<ProcessedContent> {
        info!("Whisper backend processing: {:?}", file_path);

        #[cfg(feature = "whisper")]
        {
            if !self.model_path.exists() {
                warn!(
                    "Whisper model not found at {:?}. Please download a ggml model from https://huggingface.co/ggerganov/whisper.cpp",
                    self.model_path
                );
                return Err(anyhow::anyhow!(
                    "Whisper model not found at {:?}",
                    self.model_path
                ));
            }
        }

        let extension = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

        match extension {
            "mp3" | "mp4" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "webm" | "avi" | "mov"
            | "mkv" | "wmv" => {
                let text = self.transcribe_file(file_path).await?;
                Ok(ProcessedContent::Transcript {
                    text,
                    language: Some("auto-detected".to_string()),
                    duration_ms: None,
                })
            }
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" => Ok(ProcessedContent::Description {
                description: "Whisper cannot process image files".to_string(),
                tags: vec!["unsupported".to_string()],
            }),
            _ => Err(anyhow::anyhow!("Unsupported file type: {}", extension)),
        }
    }

    fn name(&self) -> &str {
        "whisper"
    }
}
