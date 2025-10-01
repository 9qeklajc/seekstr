use anyhow::Result;
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelaySearchConfig {
    pub relays: Vec<String>,
    #[serde(with = "duration_serde")]
    pub timeout: Duration,
}

mod duration_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

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

impl EventSearchRequest {
    pub fn get_search_query(&self) -> Option<&str> {
        self.search.as_deref()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EventSearchResponse {
    pub event_ids: Vec<String>,
    pub total_found: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PostEventRequest {
    pub content: String,
    pub kind: Option<u16>,
    pub tags: Option<Vec<Vec<String>>>,
}

impl Default for RelaySearchConfig {
    fn default() -> Self {
        Self {
            relays: vec![
                "wss://relay.damus.io".to_string(),
                "wss://purplepag.es".to_string(),
                "wss://relay.current.fyi".to_string(),
                "wss://relay.nostr.band".to_string(),
                "wss://nos.lol".to_string(),
                "wss://relay.snort.social".to_string(),
            ],
            timeout: Duration::from_secs(10),
        }
    }
}

pub struct RelaySearcher {
    config: RelaySearchConfig,
}

impl RelaySearcher {
    pub fn new(config: RelaySearchConfig) -> Self {
        Self { config }
    }

    pub fn with_default_config() -> Self {
        Self {
            config: RelaySearchConfig::default(),
        }
    }

    pub async fn search_relay_events(
        &self,
        request: &EventSearchRequest,
    ) -> Result<EventSearchResponse> {
        let limit = request.limit.unwrap_or(50);
        let event_kinds = request
            .event_kinds
            .as_ref()
            .map(|kinds| kinds.iter().map(|k| Kind::from(*k)).collect());

        let events = self
            .search_relay_events_with_kinds(
                request.language.as_deref(),
                request.author.as_deref(),
                request.search.as_deref(),
                limit,
                event_kinds,
            )
            .await?;

        println!("{:?}", events);

        let event_ids: Vec<String> = events.iter().map(|e| e.id.to_hex()).collect();

        Ok(EventSearchResponse {
            event_ids,
            total_found: events.len(),
        })
    }

    async fn search_relay_events_with_kinds(
        &self,
        language: Option<&str>,
        author: Option<&str>,
        search: Option<&str>,
        limit: usize,
        event_kinds: Option<Vec<Kind>>,
    ) -> Result<Vec<Event>> {
        let kinds = event_kinds.unwrap_or_else(|| {
            vec![
                Kind::from(1),     // Short text note
                Kind::from(6),     // Repost
                Kind::from(7),     // Reaction
                Kind::from(1111),  // Comment
                Kind::from(30023), // Long-form content
            ]
        });

        let mut filter = Filter::new().kinds(kinds).limit(limit);

        if let Some(q) = search {
            filter = filter.search(q);
        }

        if let Some(lang) = language {
            filter =
                filter.custom_tag(SingleLetterTag::lowercase(Alphabet::L), lang.to_lowercase());
        }

        if let Some(auth) = author {
            let pubkey = PublicKey::from_hex(auth)?;
            filter = filter.author(pubkey);
        }

        let mut events = Vec::new();

        for relay_url in &self.config.relays {
            match self
                .search_single_relay(relay_url, &filter, search, limit - events.len())
                .await
            {
                Ok(mut relay_events) => {
                    events.append(&mut relay_events);
                    if events.len() >= limit {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("Failed to search relay {}: {}", relay_url, e);
                    continue;
                }
            }
        }

        Ok(events)
    }

    async fn search_single_relay(
        &self,
        relay_url: &str,
        filter: &Filter,
        search: Option<&str>,
        remaining_limit: usize,
    ) -> Result<Vec<Event>> {
        let client = Client::default();
        client.add_relay(relay_url).await?;
        client.connect().await;

        let events = client
            .fetch_events(filter.clone(), self.config.timeout)
            .await?;

        let mut filtered_events = Vec::new();
        for event in events {
            if search.is_none() || self.matches_query(&event, search.unwrap()) {
                filtered_events.push(event);
                if filtered_events.len() >= remaining_limit {
                    break;
                }
            }
        }

        client.disconnect().await;
        Ok(filtered_events)
    }

    fn matches_query(&self, event: &Event, query: &str) -> bool {
        let content = event.content.to_lowercase();
        let query_lower = query.to_lowercase();

        content.contains(&query_lower)
    }

    pub async fn post_event(&self, request: &PostEventRequest) -> Result<String> {
        let keys = Keys::generate();
        let client = Client::builder().signer(keys).build();

        for relay_url in &self.config.relays {
            if let Err(e) = client.add_relay(relay_url).await {
                eprintln!("Failed to add relay {}: {}", relay_url, e);
                continue;
            }
        }

        client.connect().await;

        let kind = Kind::from(request.kind.unwrap_or(1));
        let mut event_builder = EventBuilder::new(kind, &request.content);

        if let Some(ref tags) = request.tags {
            for tag_parts in tags {
                if !tag_parts.is_empty() {
                    let tag = Tag::parse(tag_parts)?;
                    event_builder = event_builder.tags(vec![tag]);
                }
            }
        }

        let event = client.sign_event_builder(event_builder).await?;
        let event_id = event.id.to_hex();

        client.send_event(&event).await?;
        client.disconnect().await;

        Ok(event_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_relay_searcher_creation() {
        let searcher = RelaySearcher::with_default_config();
        assert_eq!(searcher.config.relays.len(), 6);
        assert_eq!(searcher.config.timeout, Duration::from_secs(10));
    }

    #[tokio::test]
    async fn test_event_search_request() {
        let request = EventSearchRequest {
            language: None,
            author: None,
            search: Some("test".to_string()),
            limit: Some(10),
            event_kinds: Some(vec![1]),
        };
        assert_eq!(request.limit, Some(10));
        assert_eq!(request.search, Some("test".to_string()));
    }

    #[tokio::test]
    async fn test_custom_config() {
        let config = RelaySearchConfig {
            relays: vec!["wss://test.relay.com".to_string()],
            timeout: Duration::from_secs(5),
        };
        let searcher = RelaySearcher::new(config);
        assert_eq!(searcher.config.relays.len(), 1);
        assert_eq!(searcher.config.timeout, Duration::from_secs(5));
    }

    #[test]
    fn test_matches_query() {
        let _searcher = RelaySearcher::with_default_config();
        let content = "This is a Rust code snippet";

        assert!(content.to_lowercase().contains("rust"));
        assert!(content.to_lowercase().contains("code"));
        assert!(!content.to_lowercase().contains("python"));
    }
}
