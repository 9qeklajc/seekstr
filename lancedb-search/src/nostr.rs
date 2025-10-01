use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrEvent {
    pub id: String,
    pub pubkey: String,
    pub created_at: i64,
    pub kind: i32,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrEventWithEmbedding {
    pub id: String,
    pub pubkey: String,
    pub created_at: i64,
    pub kind: i32,
    pub tags: String,
    pub content_embedding: Vec<f32>,
}

impl NostrEventWithEmbedding {
    pub fn new(
        id: String,
        pubkey: String,
        created_at: i64,
        kind: i32,
        tags: Vec<Vec<String>>,
        content_embedding: Vec<f32>,
    ) -> Self {
        Self {
            id,
            pubkey,
            created_at,
            kind,
            tags: serde_json::to_string(&tags).unwrap_or_default(),
            content_embedding,
        }
    }

    pub fn get_tags(&self) -> Result<Vec<Vec<String>>, serde_json::Error> {
        serde_json::from_str(&self.tags)
    }
}

impl NostrEventWithEmbedding {
    pub fn from_event_with_embedding(event: NostrEvent, embedding: Vec<f32>) -> Self {
        Self {
            id: event.id,
            pubkey: event.pubkey,
            created_at: event.created_at,
            kind: event.kind,
            tags: serde_json::to_string(&event.tags).unwrap_or_default(),
            content_embedding: embedding,
        }
    }
}
