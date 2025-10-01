use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub watch_dir: PathBuf,
    pub backend: BackendConfig,
    pub file_types: FileTypeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    pub backend_type: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTypeConfig {
    pub audio_extensions: Vec<String>,
    pub video_extensions: Vec<String>,
    pub image_extensions: Vec<String>,
}

impl Default for FileTypeConfig {
    fn default() -> Self {
        Self {
            audio_extensions: vec![
                "mp3".to_string(),
                "wav".to_string(),
                "flac".to_string(),
                "aac".to_string(),
                "ogg".to_string(),
                "m4a".to_string(),
                "webm".to_string(),
            ],
            video_extensions: vec![
                "mp4".to_string(),
                "avi".to_string(),
                "mov".to_string(),
                "mkv".to_string(),
                "wmv".to_string(),
            ],
            image_extensions: vec![
                "jpg".to_string(),
                "jpeg".to_string(),
                "png".to_string(),
                "gif".to_string(),
                "bmp".to_string(),
                "webp".to_string(),
            ],
        }
    }
}
