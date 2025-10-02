use anyhow::Result;
use eventflow::Processor;
use nostr::{Event, EventBuilder, Keys, Kind, Tag};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, error, info};

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessingResult {
    pub original_event_id: String,
    pub url: String,
    pub file_type: String,
    pub backend_used: String,
    pub content: String,
    pub timestamp_ms: u64,
}

pub struct MediaProcessor {
    url_regex: Regex,
    scribe_backend: Arc<dyn scribe::processor::Processor + Send + Sync>,
    keys: Keys,
}

impl MediaProcessor {
    pub fn new(
        scribe_backend: Box<dyn scribe::processor::Processor + Send + Sync>,
    ) -> Result<Self> {
        let keys = Keys::generate();

        let pattern = r#"https?://[^\s<>"']+\.(?:mp3|wav|flac|aac|ogg|m4a|webm|mp4|avi|mov|mkv|wmv|m4v|ogv|jpg|jpeg|png|gif|bmp|svg|webp)(?:\?[^\s<>"']*)?"#;
        let url_regex = Regex::new(pattern)?;

        Ok(Self {
            url_regex,
            scribe_backend: Arc::from(scribe_backend),
            keys,
        })
    }

    fn extract_media_urls(&self, event: &Event) -> Vec<String> {
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

    fn process_media_url_sync(&self, url: &str, original_event: &Event) -> Result<Event> {
        info!("Processing media URL: {}", url);

        // Use block_in_place to run async code in sync context
        let result = tokio::task::block_in_place(|| {
            // Get a handle to the current runtime
            let handle = tokio::runtime::Handle::current();
            // Run the async operation
            handle.block_on(async {
                scribe::processor::process_single_url_direct(url, &*self.scribe_backend).await
            })
        })?;

        // Extract content text based on the result type
        let content_text = match &result.content {
            scribe::processor::ProcessedContent::Transcript { text, summary, .. } => {
                if let Some(summary) = summary {
                    format!("Transcript Summary: {}\n\nFull Transcript: {}", summary, text)
                } else {
                    format!("Transcript: {}", text)
                }
            },
            scribe::processor::ProcessedContent::Description { description, tags } => {
                format!("Description: {}\nTags: {}", description, tags.join(", "))
            }
        };

        // Create processing result
        let processing_result = ProcessingResult {
            original_event_id: original_event.id.to_hex(),
            url: url.to_string(),
            file_type: result.file_type,
            backend_used: result.backend_used,
            content: content_text.clone(),
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
        };

        // Build new event with processing results
        let event_content = format!(
            "Media Processing Result\n\nOriginal Event: {}\nURL: {}\nType: {}\nBackend: {}\n\n{}",
            processing_result.original_event_id,
            processing_result.url,
            processing_result.file_type,
            processing_result.backend_used,
            content_text
        );

        // Create new event with tags (Kind 1 is a text note)
        let event = EventBuilder::new(Kind::from(1u16), event_content)
            .tag(Tag::event(original_event.id))
            .tag(Tag::parse(vec!["processed-url", url])?)
            .tag(Tag::parse(vec!["processor", "scribe", &processing_result.backend_used])?)
            .sign_with_keys(&self.keys)?;

        Ok(event)
    }
}

impl Processor for MediaProcessor {
    fn process(&self, event: &Event) -> Vec<Event> {
        let urls = self.extract_media_urls(event);

        if urls.is_empty() {
            debug!("No media URLs found in event {}", event.id);
            // Pass through the original event
            return vec![event.clone()];
        }

        info!("Found {} media URLs in event {}", urls.len(), event.id);

        let mut results = vec![event.clone()];

        // Process each URL synchronously
        for url in urls {
            match self.process_media_url_sync(&url, event) {
                Ok(processed_event) => {
                    info!("Successfully processed URL: {}", url);
                    results.push(processed_event);
                }
                Err(e) => {
                    error!("Failed to process URL {}: {}", url, e);
                }
            }
        }

        results
    }

    fn name(&self) -> &str {
        "MediaProcessor"
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn shutdown(&self) {
        info!("MediaProcessor shutting down");
    }
}