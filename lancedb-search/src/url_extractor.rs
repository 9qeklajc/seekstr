use crate::nostr::NostrEvent;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct Dimensions {
    pub width: u32,
    pub height: u32,
}

fn is_http_url(url: &str) -> bool {
    if let Ok(parsed_url) = url::Url::parse(url) {
        matches!(parsed_url.scheme(), "http" | "https")
    } else {
        false
    }
}

pub fn extract_imeta_image_urls(event: &NostrEvent) -> Vec<String> {
    let mut urls = Vec::new();

    for tag in &event.tags {
        if tag.is_empty() || tag[0] != "imeta" {
            continue;
        }

        for entry in tag.iter().skip(1) {
            if let Some(url) = entry.strip_prefix("url ") {
                let url = url.trim();
                if !url.is_empty() && is_http_url(url) {
                    urls.push(url.to_string());
                }
            }
        }
    }

    let unique_urls: Vec<String> = urls
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    unique_urls
}

pub fn extract_imeta_video_urls(event: &NostrEvent) -> Vec<String> {
    let mut urls = Vec::new();
    let video_mime_regex = regex::Regex::new(r"^(video/|application/x-mpegURL)").unwrap();
    let video_file_regex =
        regex::Regex::new(r"\.(mp4|webm|ogg|ogv|mov|m4v|m3u8)(?:[?#].*)?$").unwrap();

    for tag in &event.tags {
        if tag.is_empty() || tag[0] != "imeta" {
            continue;
        }

        let mut has_video_mime = false;
        let mut local_urls = Vec::new();

        for entry in tag.iter().skip(1) {
            if let Some(mime) = entry.strip_prefix("m ") {
                let mime = mime.trim();
                if video_mime_regex.is_match(mime) {
                    has_video_mime = true;
                }
            } else if let Some(url) = entry.strip_prefix("url ") {
                let url = url.trim();
                if !url.is_empty() && is_http_url(url) {
                    local_urls.push(url.to_string());
                }
            } else if let Some(fallback_url) = entry.strip_prefix("fallback ") {
                let fallback_url = fallback_url.trim();
                if !fallback_url.is_empty() && is_http_url(fallback_url) {
                    local_urls.push(fallback_url.to_string());
                }
            }
        }

        for url in local_urls {
            if has_video_mime || video_file_regex.is_match(&url) {
                urls.push(url);
            }
        }
    }

    let unique_urls: Vec<String> = urls
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    unique_urls
}

pub fn extract_imeta_blurhashes(event: &NostrEvent) -> Vec<String> {
    let mut hashes = Vec::new();

    for tag in &event.tags {
        if tag.is_empty() || tag[0] != "imeta" {
            continue;
        }

        for entry in tag.iter().skip(1) {
            if let Some(hash) = entry.strip_prefix("blurhash ") {
                let hash = hash.trim();
                if !hash.is_empty() {
                    hashes.push(hash.to_string());
                }
            }
        }
    }

    let unique_hashes: Vec<String> = hashes
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    unique_hashes
}

pub fn extract_imeta_dimensions(event: &NostrEvent) -> Vec<Dimensions> {
    let mut dimensions = Vec::new();
    let dim_regex = regex::Regex::new(r"^(\d+)x(\d+)$").unwrap();

    for tag in &event.tags {
        if tag.is_empty() || tag[0] != "imeta" {
            continue;
        }

        for entry in tag.iter().skip(1) {
            if let Some(dim_str) = entry.strip_prefix("dim ") {
                let dim_str = dim_str.trim();
                if let Some(captures) = dim_regex.captures(dim_str)
                    && let (Ok(width), Ok(height)) =
                        (captures[1].parse::<u32>(), captures[2].parse::<u32>())
                    && width > 0
                    && height > 0
                {
                    dimensions.push(Dimensions { width, height });
                }
            }
        }
    }

    dimensions
}

pub fn extract_imeta_hashes(event: &NostrEvent) -> Vec<String> {
    let mut hashes = Vec::new();

    for tag in &event.tags {
        if tag.is_empty() || tag[0] != "imeta" {
            continue;
        }

        let mut found: Option<String> = None;
        for entry in tag.iter().skip(1) {
            if let Some(hash) = entry.strip_prefix("x ") {
                let hash = hash.trim();
                if !hash.is_empty() {
                    found = Some(hash.to_string());
                    break;
                }
            }
        }

        if let Some(hash) = found {
            hashes.push(hash);
        }
    }

    if hashes.is_empty() {
        for tag in &event.tags {
            if tag.len() >= 2 && tag[0] == "x" {
                let hash = tag[1].trim();
                if !hash.is_empty() {
                    hashes.push(hash.to_string());
                }
            }
        }
    }

    let unique_hashes: Vec<String> = hashes
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    unique_hashes
}

pub fn extract_all_urls(event: &NostrEvent) -> Vec<String> {
    let mut all_urls = Vec::new();

    all_urls.extend(extract_imeta_image_urls(event));
    all_urls.extend(extract_imeta_video_urls(event));

    let unique_urls: Vec<String> = all_urls
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    unique_urls
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_event() -> NostrEvent {
        NostrEvent {
            id: "test_id".to_string(),
            pubkey: "test_pubkey".to_string(),
            created_at: 1234567890,
            kind: 1,
            tags: vec![
                vec![
                    "imeta".to_string(),
                    "url https://example.com/image.jpg".to_string(),
                    "m image/jpeg".to_string(),
                    "dim 1920x1080".to_string(),
                    "blurhash LKO2?U%2Tw=w]~RBVZRi};RPxuwH".to_string(),
                    "x abcd1234hash".to_string(),
                ],
                vec![
                    "imeta".to_string(),
                    "url https://example.com/video.mp4".to_string(),
                    "m video/mp4".to_string(),
                    "fallback https://example.com/fallback.webm".to_string(),
                ],
                vec!["x".to_string(), "fallback_hash".to_string()],
            ],
            content: "test content".to_string(),
            sig: "test_sig".to_string(),
        }
    }

    #[test]
    fn test_extract_imeta_image_urls() {
        let event = create_test_event();
        let urls = extract_imeta_image_urls(&event);
        assert!(urls.contains(&"https://example.com/image.jpg".to_string()));
        assert!(urls.contains(&"https://example.com/video.mp4".to_string()));
    }

    #[test]
    fn test_extract_imeta_video_urls() {
        let event = create_test_event();
        let urls = extract_imeta_video_urls(&event);
        assert!(urls.contains(&"https://example.com/video.mp4".to_string()));
        assert!(urls.contains(&"https://example.com/fallback.webm".to_string()));
    }

    #[test]
    fn test_extract_imeta_blurhashes() {
        let event = create_test_event();
        let hashes = extract_imeta_blurhashes(&event);
        assert!(hashes.contains(&"LKO2?U%2Tw=w]~RBVZRi};RPxuwH".to_string()));
    }

    #[test]
    fn test_extract_imeta_dimensions() {
        let event = create_test_event();
        let dimensions = extract_imeta_dimensions(&event);
        assert_eq!(dimensions.len(), 1);
        assert_eq!(dimensions[0].width, 1920);
        assert_eq!(dimensions[0].height, 1080);
    }

    #[test]
    fn test_extract_imeta_hashes() {
        let event = create_test_event();
        let hashes = extract_imeta_hashes(&event);
        assert!(hashes.contains(&"abcd1234hash".to_string()));
    }
}
