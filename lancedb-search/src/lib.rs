use serde::{Deserialize, Serialize};

pub mod collect;
pub mod embedding_service;
pub mod embeddings;
pub mod initialize;
pub mod lancedb_store;
pub mod nostr;

#[derive(Debug, Serialize, Deserialize)]
pub struct EventSearchRequest {
    pub language: Option<String>,
    pub author: Option<String>,
    #[serde(deserialize_with = "deserialize_optional_usize_from_string")]
    pub limit: Option<usize>,
    pub event_kinds: Option<Vec<u16>>,
    pub search: Option<String>,
}

fn deserialize_optional_usize_from_string<'de, D>(
    deserializer: D,
) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrUsize {
        String(String),
        Usize(usize),
    }

    match Option::<StringOrUsize>::deserialize(deserializer)? {
        Some(StringOrUsize::String(s)) => s.parse().map(Some).map_err(serde::de::Error::custom),
        Some(StringOrUsize::Usize(u)) => Ok(Some(u)),
        None => Ok(None),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EventSearchResponse {
    pub event_ids: Vec<String>,
    pub total_found: usize,
}

impl EventSearchRequest {
    pub fn get_search_query(&self) -> Option<&str> {
        self.search.as_deref()
    }
}
