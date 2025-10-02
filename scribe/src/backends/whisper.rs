#[cfg(feature = "whisper")]
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::processor::{
    FileType, ProcessedContent, Processor, generate_summary, get_file_type_from_url,
};
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
            PathBuf::from(home).join(".cache/whisper/ggml-large-v3.bin")
        });

        Self { model_path }
    }

    #[cfg(feature = "whisper")]
    async fn transcribe_file(&self, file_path: &Path) -> Result<String> {
        info!("Starting transcription of file: {:?}", file_path);

        // First, check the duration of the audio/video file
        let duration = self.get_file_duration(file_path).await?;
        info!("File duration: {:.2} seconds", duration);

        if duration <= 30.0 {
            // File is short enough, process directly
            info!("File is short (<= 30s), processing directly");
            self.transcribe_single_file(file_path).await
        } else {
            // File is too long, split into chunks
            info!("File is long (> 30s), splitting into 30-second chunks");
            self.transcribe_chunked_file(file_path, duration).await
        }
    }

    #[cfg(feature = "whisper")]
    async fn transcribe_single_file(&self, file_path: &Path) -> Result<String> {
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

    #[cfg(feature = "whisper")]
    async fn transcribe_chunked_file(&self, file_path: &Path, duration: f64) -> Result<String> {
        let chunk_duration = 30.0; // 30 seconds per chunk
        let num_chunks = (duration / chunk_duration).ceil() as usize;

        info!("Splitting into {} chunks of {} seconds each", num_chunks, chunk_duration);

        let mut all_transcriptions = Vec::new();

        for chunk_index in 0..num_chunks {
            let start_time = chunk_index as f64 * chunk_duration;
            let end_time = ((chunk_index + 1) as f64 * chunk_duration).min(duration);

            info!("Processing chunk {} ({:.1}s - {:.1}s)", chunk_index + 1, start_time, end_time);

            // Create chunk file
            let chunk_file = self.create_audio_chunk(file_path, start_time, end_time, chunk_index).await?;

            // Transcribe the chunk
            let chunk_transcription = self.transcribe_single_file(chunk_file.path()).await?;

            let transcription_len = chunk_transcription.len();
            if !chunk_transcription.trim().is_empty() {
                all_transcriptions.push(chunk_transcription);
            }

            info!("Chunk {} transcribed: {} characters", chunk_index + 1, transcription_len);
        }

        // Combine all transcriptions
        let combined_transcription = all_transcriptions.join(" ");
        info!("Combined transcription: {} characters total", combined_transcription.len());

        Ok(combined_transcription)
    }

    #[cfg(feature = "whisper")]
    async fn get_file_duration(&self, file_path: &Path) -> Result<f64> {
        use std::process::Command;

        let output = Command::new("ffprobe")
            .args(&[
                "-v", "quiet",
                "-show_entries", "format=duration",
                "-of", "csv=p=0",
                file_path.to_str().unwrap(),
            ])
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "ffprobe failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let duration_str = String::from_utf8(output.stdout)?;
        let duration: f64 = duration_str.trim().parse()?;

        Ok(duration)
    }

    #[cfg(feature = "whisper")]
    async fn create_audio_chunk(&self, file_path: &Path, start_time: f64, end_time: f64, chunk_index: usize) -> Result<tempfile::NamedTempFile> {
        use std::process::Command;

        let duration = end_time - start_time;
        let chunk_file = tempfile::NamedTempFile::with_suffix(&format!("_chunk_{}.wav", chunk_index))?;

        let output = Command::new("ffmpeg")
            .args(&[
                "-i", file_path.to_str().unwrap(),
                "-ss", &start_time.to_string(),
                "-t", &duration.to_string(),
                "-acodec", "pcm_s16le",
                "-ar", "16000",
                "-ac", "1",
                "-y", // Overwrite output file
                chunk_file.path().to_str().unwrap(),
            ])
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "ffmpeg chunk creation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(chunk_file)
    }

    #[cfg(not(feature = "whisper"))]
    async fn transcribe_file(&self, _file_path: &Path) -> Result<String> {
        Err(anyhow::anyhow!(
            "Whisper support not compiled. Build with --features whisper (requires libclang-dev)"
        ))
    }

    async fn transcribe_url(&self, url: &str) -> Result<String> {
        info!("Whisper backend: processing audio from URL: {}", url);

        // Check if we have a working Whisper model
        if !self.model_path.exists() {
            return Err(anyhow::anyhow!(
                "Whisper model not found at {:?}. Please download a model from https://huggingface.co/ggerganov/whisper.cpp or use OpenAI backend for transcription",
                self.model_path
            ));
        }

        #[cfg(feature = "whisper")]
        {
            info!("Whisper model found at: {:?}", self.model_path);

            // Download the audio file from URL
            info!("Downloading audio file from URL: {}", url);
            let client = reqwest::Client::new();
            let response = client.get(url).send().await?;

            if !response.status().is_success() {
                return Err(anyhow::anyhow!(
                    "Failed to download audio file: HTTP {}",
                    response.status()
                ));
            }

            let bytes = response.bytes().await?;
            info!("Downloaded {} bytes from URL", bytes.len());

            // Create temporary file with appropriate extension
            let file_extension = self.extract_extension_from_url(url);
            let temp_file = tempfile::NamedTempFile::with_suffix(&format!(".{}", file_extension))?;
            let temp_path = temp_file.path();

            // Write downloaded content to temporary file
            tokio::fs::write(temp_path, &bytes).await?;
            info!("Saved audio to temporary file: {:?}", temp_path);

            // Process the temporary file using existing transcribe_file method
            let transcription = self.transcribe_file(temp_path).await?;

            info!(
                "Transcription completed, {} characters",
                transcription.len()
            );
            Ok(transcription)
        }
        #[cfg(not(feature = "whisper"))]
        {
            Err(anyhow::anyhow!(
                "Whisper support not compiled. Build with --features whisper to enable local transcription, or use OpenAI backend"
            ))
        }
    }

    fn extract_extension_from_url(&self, url: &str) -> String {
        if let Ok(parsed_url) = url::Url::parse(url) {
            if let Some(path) = parsed_url.path_segments() {
                if let Some(filename) = path.last() {
                    if let Some(dot_pos) = filename.rfind('.') {
                        return filename[dot_pos + 1..].to_string();
                    }
                }
            }
        }
        // Default to mp3 if we can't determine the extension
        "mp3".to_string()
    }
}

#[async_trait]
impl Processor for WhisperBackend {
    async fn process(&self, url: &str) -> Result<ProcessedContent> {
        info!("Whisper backend processing: {}", url);

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

        let file_type = get_file_type_from_url(url);

        match file_type {
            FileType::Audio | FileType::Video => {
                let text = self.transcribe_url(url).await?;

                // Generate summary if OpenAI API key is available
                let summary = if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
                    match generate_summary(&text, &api_key).await {
                        Ok(summary) => {
                            info!("Generated summary for transcription");
                            Some(summary)
                        }
                        Err(e) => {
                            info!("Failed to generate summary: {}", e);
                            None
                        }
                    }
                } else {
                    info!("No OPENAI_API_KEY found, skipping summary generation");
                    None
                };

                Ok(ProcessedContent::Transcript {
                    text,
                    language: Some("auto-detected".to_string()),
                    duration_ms: None,
                    summary,
                })
            }
            FileType::Image => Ok(ProcessedContent::Description {
                description: "Whisper cannot process image files".to_string(),
                tags: vec!["unsupported".to_string()],
            }),
            FileType::Unknown => Err(anyhow::anyhow!("Unsupported file type for URL: {}", url)),
        }
    }

    fn name(&self) -> &str {
        "whisper"
    }
}
