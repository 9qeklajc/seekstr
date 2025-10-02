use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub backend: BackendConfig,
    pub relays: RelayConfig,
    pub processing: ProcessingConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    #[serde(rename = "type")]
    pub backend_type: BackendType,
    pub openai_api_key: Option<String>,
    pub whisper_model_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendType {
    OpenAI,
    Whisper,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    pub sources: Vec<String>,
    pub sinks: Vec<String>,
    /// Optional filters to apply when subscribing to source relays
    /// This allows filtering for specific kinds of events (e.g., only media events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<EventFilter>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFilter {
    /// List of event kinds to match (e.g., 1 for text notes)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<u16>>,

    /// List of pubkeys (authors) to match
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingConfig {
    pub state_file: String,
    pub batch_size: Option<usize>,
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub modules: Option<Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            backend: BackendConfig {
                backend_type: BackendType::Auto,
                openai_api_key: None,
                whisper_model_path: None,
            },
            relays: RelayConfig {
                sources: vec![
                    "wss://relay.damus.io".to_string(),
                    "wss://nos.lol".to_string(),
                    "wss://relay.nostr.band".to_string(),
                ],
                sinks: vec![
                    "wss://nostr.wine".to_string(),
                    "wss://relay.snort.social".to_string(),
                ],
                filters: None,
            },
            processing: ProcessingConfig {
                state_file: "seekstr_state.json".to_string(),
                batch_size: None,
                timeout_seconds: Some(30),
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                modules: Some(vec![
                    "seekstr".to_string(),
                    "eventflow".to_string(),
                    "scribe".to_string(),
                ]),
            },
        }
    }
}

impl Config {
    /// Load configuration from a TOML file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Load configuration from a TOML file, or use defaults if file doesn't exist
    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Result<Self> {
        if path.as_ref().exists() {
            Self::load(path)
        } else {
            Ok(Self::default())
        }
    }

    /// Save configuration to a TOML file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        // Validate backend configuration
        match self.backend.backend_type {
            BackendType::OpenAI => {
                if self.backend.openai_api_key.is_none() {
                    anyhow::bail!("OpenAI backend requires openai_api_key to be set");
                }
            }
            BackendType::Whisper => {
                if self.backend.whisper_model_path.is_none() {
                    anyhow::bail!("Whisper backend requires whisper_model_path to be set");
                }
            }
            BackendType::Auto => {
                if self.backend.openai_api_key.is_none() && self.backend.whisper_model_path.is_none() {
                    anyhow::bail!("Auto backend requires either openai_api_key or whisper_model_path to be set");
                }
            }
        }

        // Validate relay configuration
        if self.relays.sources.is_empty() {
            anyhow::bail!("At least one source relay must be configured");
        }
        if self.relays.sinks.is_empty() {
            anyhow::bail!("At least one sink relay must be configured");
        }

        Ok(())
    }

    /// Build the RUST_LOG environment variable string from logging configuration
    pub fn build_rust_log(&self) -> String {
        if let Some(modules) = &self.logging.modules {
            modules
                .iter()
                .map(|module| format!("{}={}", module, self.logging.level))
                .collect::<Vec<_>>()
                .join(",")
        } else {
            self.logging.level.clone()
        }
    }
}