use crate::processor::{
    FileType, ProcessedContent, Processor, generate_summary, get_file_type_from_url,
};
use anyhow::Result;
use async_trait::async_trait;
use rusty_ytdl::{Video, VideoOptions, VideoQuality, VideoSearchOptions};
use tracing::info;
use yt_transcript_rs::YouTubeTranscriptApi;

#[derive(Debug)]
struct VideoInfo {
    duration_seconds: u64,
    estimated_size_mb: f64,
}

pub struct YouTubeBackend;

impl YouTubeBackend {
    pub fn new() -> Self {
        Self
    }

    async fn get_youtube_transcript(&self, url: &str) -> Result<String> {
        info!("YouTube backend: Getting transcript for URL: {}", url);

        let video_id = self.extract_video_id(url)?;
        info!("YouTube video ID: {}", video_id);

        let video_info = self.get_video_info(&video_id).await?;
        let duration_seconds = video_info.duration_seconds;
        let video_size_mb = video_info.estimated_size_mb;

        info!(
            "Video duration: {}s, estimated size: {}MB",
            duration_seconds, video_size_mb
        );

        const MAX_SIZE_MB: f64 = 100.0;
        const MAX_DURATION_SECONDS: u64 = 300;

        if video_size_mb > MAX_SIZE_MB || duration_seconds > MAX_DURATION_SECONDS {
            info!(
                "Video too large ({:.1}MB) or long ({}s), using transcript API",
                video_size_mb, duration_seconds
            );
            self.fetch_youtube_transcript(&video_id).await
        } else {
            info!("Video size acceptable, downloading and using Whisper");
            match self
                .download_and_transcribe_with_whisper(url, &video_id)
                .await
            {
                Ok(transcript) => Ok(transcript),
                Err(e) => {
                    info!(
                        "Whisper transcription failed: {}, falling back to transcript API",
                        e
                    );
                    self.fetch_youtube_transcript(&video_id).await
                }
            }
        }
    }

    async fn get_video_info(&self, video_id: &str) -> Result<VideoInfo> {
        let video_options = VideoOptions {
            quality: VideoQuality::Lowest,
            filter: VideoSearchOptions::Audio,
            ..Default::default()
        };

        let video = Video::new_with_options(video_id, video_options)?;
        let info = video.get_info().await?;

        let duration_seconds = info
            .video_details
            .length_seconds
            .parse::<u64>()
            .unwrap_or(0);

        let audio_formats = info
            .formats
            .iter()
            .filter(|f| {
                f.mime_type.container.contains("audio")
                    || f.mime_type
                        .codecs
                        .iter()
                        .any(|codec| codec.contains("audio"))
            })
            .collect::<Vec<_>>();

        let estimated_size_mb = if let Some(format) = audio_formats.first() {
            if let Some(content_length) = &format.content_length {
                content_length.parse::<u64>().unwrap_or(0) as f64 / 1_048_576.0
            } else {
                duration_seconds as f64 * 0.5
            }
        } else {
            duration_seconds as f64 * 0.5
        };

        Ok(VideoInfo {
            duration_seconds,
            estimated_size_mb,
        })
    }

    async fn download_and_transcribe_with_whisper(
        &self,
        _url: &str,
        video_id: &str,
    ) -> Result<String> {
        let video_options = VideoOptions {
            quality: VideoQuality::Lowest,
            filter: VideoSearchOptions::Audio,
            ..Default::default()
        };

        let video = Video::new_with_options(video_id, video_options)?;

        let temp_file = tempfile::NamedTempFile::with_suffix(".webm")?;
        let temp_path = temp_file.path();

        info!("Downloading audio to temporary file: {:?}", temp_path);

        video.download(temp_path).await?;

        info!("Download complete, processing with Whisper");

        #[cfg(feature = "whisper")]
        {
            println!("wisper running");
            use crate::backends::whisper::WhisperBackend;
            let whisper_backend = WhisperBackend::new(None);
            let transcript = whisper_backend.transcribe_file(temp_path).await?;
            Ok(transcript)
        }

        #[cfg(not(feature = "whisper"))]
        {
            Err(anyhow::anyhow!(
                "Whisper not available, cannot transcribe downloaded audio"
            ))
        }
    }

    async fn fetch_youtube_transcript(&self, video_id: &str) -> Result<String> {
        info!("Fetching YouTube transcript for video ID: {}", video_id);

        let api = YouTubeTranscriptApi::new(None, None, None)
            .map_err(|e| anyhow::anyhow!("Failed to initialize YouTube API: {}", e))?;

        let languages = &["en", "en-US", "auto"];

        let transcript = match api.fetch_transcript(video_id, languages, false).await {
            Ok(transcript) => transcript,
            Err(e) => {
                info!(
                    "Failed to fetch transcript with preferred languages, trying any available: {}",
                    e
                );

                match api.list_transcripts(video_id).await {
                    Ok(transcript_list) => {
                        let first_transcript =
                            if !transcript_list.manually_created_transcripts.is_empty() {
                                transcript_list.manually_created_transcripts.iter().next()
                            } else if !transcript_list.generated_transcripts.is_empty() {
                                transcript_list.generated_transcripts.iter().next()
                            } else {
                                None
                            };

                        match first_transcript {
                            Some((language_code, transcript_info)) => {
                                info!(
                                    "Using transcript in language: {} ({})",
                                    transcript_info.language, language_code
                                );

                                let lang_codes = &[language_code.as_str()];
                                api.fetch_transcript(video_id, lang_codes, false)
                                    .await
                                    .map_err(|e| {
                                        anyhow::anyhow!(
                                            "Failed to fetch available transcript: {}",
                                            e
                                        )
                                    })?
                            }
                            None => {
                                return Err(anyhow::anyhow!(
                                    "No transcripts available for YouTube video: {}. The video may not have captions enabled.",
                                    video_id
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!(
                            "No transcripts available for YouTube video {}: {}",
                            video_id,
                            e
                        ));
                    }
                }
            }
        };

        let mut full_transcript = String::new();
        for snippet in &transcript.snippets {
            full_transcript.push_str(&snippet.text);
            full_transcript.push(' ');
        }

        let clean_transcript = full_transcript.trim().to_string();

        if clean_transcript.is_empty() {
            return Err(anyhow::anyhow!(
                "Retrieved transcript is empty for YouTube video: {}",
                video_id
            ));
        }

        info!(
            "Successfully retrieved YouTube transcript: {} characters from {} snippets",
            clean_transcript.len(),
            transcript.snippets.len()
        );

        Ok(clean_transcript)
    }

    fn extract_video_id(&self, url: &str) -> Result<String> {
        let url_lower = url.to_lowercase();

        if let Ok(parsed_url) = url::Url::parse(url) {
            match parsed_url.host_str() {
                Some("www.youtube.com") | Some("youtube.com") => {
                    if let Some(query_pairs) = parsed_url.query_pairs().find(|(key, _)| key == "v")
                    {
                        return Ok(query_pairs.1.to_string());
                    }
                    if url_lower.contains("/embed/")
                        && let Some(mut path_segments) = parsed_url.path_segments()
                        && let Some(video_id) = path_segments.nth(1)
                    {
                        return Ok(video_id.to_string());
                    }
                    if url_lower.contains("/v/")
                        && let Some(mut path_segments) = parsed_url.path_segments()
                        && let Some(video_id) = path_segments.nth(1)
                    {
                        return Ok(video_id.to_string());
                    }
                }
                Some("youtu.be") => {
                    if let Some(mut path_segments) = parsed_url.path_segments()
                        && let Some(video_id) = path_segments.next()
                    {
                        return Ok(video_id.to_string());
                    }
                }
                _ => {}
            }
        }

        Err(anyhow::anyhow!(
            "Could not extract video ID from URL: {}",
            url
        ))
    }
}

#[async_trait]
impl Processor for YouTubeBackend {
    async fn process(&self, url: &str) -> Result<ProcessedContent> {
        info!("YouTube backend processing: {}", url);

        let file_type = get_file_type_from_url(url);

        match file_type {
            FileType::YouTube => {
                let text = self.get_youtube_transcript(url).await?;

                let summary = if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
                    match generate_summary(&text, &api_key).await {
                        Ok(summary) => {
                            info!("Generated summary for YouTube transcript");
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
            _ => Err(anyhow::anyhow!(
                "YouTube backend can only process YouTube URLs, got: {}",
                url
            )),
        }
    }

    fn name(&self) -> &str {
        "youtube"
    }
}
