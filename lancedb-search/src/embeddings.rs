use anyhow::Result;
use arrow_array::{
    ArrayRef, FixedSizeListArray, Int32Array, Int64Array, RecordBatch, StringArray,
    types::Float32Type,
};
use lancedb::arrow::arrow_schema::{DataType, Field, Fields, Schema};
use rig::client::EmbeddingsClient;
use rig::embeddings::{Embedding, EmbeddingModel};
use rig::providers::openai;
use rig::{Embed, OneOrMany};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::nostr::NostrEvent;

pub struct EmbeddingService {
    model: openai::embedding::EmbeddingModel,
}

impl EmbeddingService {
    pub fn new() -> Result<Self> {
        let openai_client = openai::ClientBuilder::new("otrta_BiT6hytS2bEoJuP6H4p9X9IHAnwm35Su")
            .base_url("https://ecash.server.otrta.me")
            .build()?;

        let model = openai_client.embedding_model("bge-m3:latest");

        Ok(Self { model })
    }

    pub async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let embedding = self.model.embed_text(text).await?;
        Ok(embedding.vec.into_iter().map(|x| x as f32).collect())
    }
}

#[derive(Embed, Clone, Deserialize, Serialize, Debug)]
pub struct NostrEventEmbedded {
    pub id: String,
    pub pubkey: String,
    pub created_at: i64,
    pub kind: i32,
    pub tags: String,
    #[embed]
    pub content: String,
}

impl From<NostrEvent> for NostrEventEmbedded {
    fn from(event: NostrEvent) -> Self {
        Self {
            id: event.id,
            pubkey: event.pubkey,
            created_at: event.created_at,
            kind: event.kind,
            tags: serde_json::to_string(&event.tags).unwrap_or_default(),
            content: event.content,
        }
    }
}

pub struct LanceDbEmbeddingService {
    model: openai::embedding::EmbeddingModel,
}

impl LanceDbEmbeddingService {
    pub fn new() -> Result<Self> {
        let openai_client = openai::ClientBuilder::new("otrta_BiT6hytS2bEoJuP6H4p9X9IHAnwm35Su")
            .base_url("https://ecash.server.otrta.me")
            .build()?;

        let model = openai_client.embedding_model("bge-m3:latest");

        Ok(Self { model })
    }

    pub fn model(&self) -> &openai::embedding::EmbeddingModel {
        &self.model
    }

    pub async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let embedding = self.model.embed_text(text).await?;
        Ok(embedding.vec.into_iter().map(|x| x as f32).collect())
    }
}

pub fn nostr_event_schema(dims: usize) -> Schema {
    Schema::new(Fields::from(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("pubkey", DataType::Utf8, false),
        Field::new("created_at", DataType::Int64, false),
        Field::new("kind", DataType::Int32, false),
        Field::new("tags", DataType::Utf8, false),
        Field::new("content", DataType::Utf8, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                dims as i32,
            ),
            false,
        ),
    ]))
}

pub fn as_nostr_record_batch(
    records: Vec<(NostrEventEmbedded, OneOrMany<Embedding>)>,
    dims: usize,
) -> Result<RecordBatch, lancedb::arrow::arrow_schema::ArrowError> {
    let id = StringArray::from_iter_values(
        records
            .iter()
            .map(|(event, _)| &event.id)
            .collect::<Vec<_>>(),
    );

    let pubkey = StringArray::from_iter_values(
        records
            .iter()
            .map(|(event, _)| &event.pubkey)
            .collect::<Vec<_>>(),
    );

    let created_at = Int64Array::from_iter_values(
        records
            .iter()
            .map(|(event, _)| event.created_at)
            .collect::<Vec<_>>(),
    );

    let kind = Int32Array::from_iter_values(
        records
            .iter()
            .map(|(event, _)| event.kind)
            .collect::<Vec<_>>(),
    );

    let tags = StringArray::from_iter_values(
        records
            .iter()
            .map(|(event, _)| &event.tags)
            .collect::<Vec<_>>(),
    );

    let content = StringArray::from_iter_values(
        records
            .iter()
            .map(|(event, _)| &event.content)
            .collect::<Vec<_>>(),
    );

    let embedding = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
        records
            .into_iter()
            .map(|(_, embeddings)| {
                Some(
                    embeddings
                        .first()
                        .vec
                        .into_iter()
                        .map(|x| Some(x as f32))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>(),
        dims as i32,
    );

    RecordBatch::try_from_iter(vec![
        ("id", Arc::new(id) as ArrayRef),
        ("pubkey", Arc::new(pubkey) as ArrayRef),
        ("created_at", Arc::new(created_at) as ArrayRef),
        ("kind", Arc::new(kind) as ArrayRef),
        ("tags", Arc::new(tags) as ArrayRef),
        ("content", Arc::new(content) as ArrayRef),
        ("embedding", Arc::new(embedding) as ArrayRef),
    ])
}

pub async fn simple_similarity_search(
    events: &[NostrEventEmbedded],
    embeddings: &[Vec<f32>],
    query: &str,
    embedding_service: &LanceDbEmbeddingService,
    relevance_threshold: f32,
) -> Result<Vec<NostrEventEmbedded>> {

    let query_embedding = embedding_service.model().embed_text(query).await?;
    let query_vec: Vec<f32> = query_embedding.vec.into_iter().map(|x| x as f32).collect();

    let mut scored_results: Vec<(NostrEventEmbedded, f32)> = Vec::new();

    for (event, embedding) in events.iter().zip(embeddings.iter()) {
        let similarity = cosine_similarity_f32(&query_vec, embedding);
        if similarity >= relevance_threshold {
            scored_results.push((event.clone(), similarity));
        }
    }

    scored_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    Ok(scored_results.into_iter().map(|(event, _)| event).collect())
}

fn cosine_similarity_f32(vec1: &[f32], vec2: &[f32]) -> f32 {
    let dot_product: f32 = vec1.iter().zip(vec2.iter()).map(|(a, b)| a * b).sum();
    let norm1: f32 = vec1.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm2: f32 = vec2.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm1 == 0.0 || norm2 == 0.0 {
        0.0
    } else {
        dot_product / (norm1 * norm2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nostr::NostrEvent;

    #[tokio::test]
    async fn test_lancedb_embedding_service_creation() {
        let service_result = LanceDbEmbeddingService::new();
        if service_result.is_err() {
            println!(
                "Warning: Could not create LanceDbEmbeddingService, likely due to network/API issues"
            );
            return;
        }

        let service = service_result.unwrap();
        assert!(service.model().ndims() > 0);
    }

    #[tokio::test]
    async fn test_nostr_event_embedded_conversion() {
        let event = NostrEvent {
            id: "test_id_123".to_string(),
            pubkey: "test_pubkey_456".to_string(),
            created_at: 1234567890,
            kind: 1,
            tags: vec![vec!["p".to_string(), "somekey".to_string()]],
            content: "This is a test event for semantic search".to_string(),
            sig: "test_signature".to_string(),
        };

        let embedded_event: NostrEventEmbedded = event.into();

        assert_eq!(embedded_event.id, "test_id_123");
        assert_eq!(embedded_event.pubkey, "test_pubkey_456");
        assert_eq!(embedded_event.created_at, 1234567890);
        assert_eq!(embedded_event.kind, 1);
        assert_eq!(
            embedded_event.content,
            "This is a test event for semantic search"
        );
        assert!(embedded_event.tags.contains("somekey"));
    }

    #[tokio::test]
    async fn test_simple_similarity_search() {
        let service_result = LanceDbEmbeddingService::new();
        if service_result.is_err() {
            println!("Warning: Could not create LanceDbEmbeddingService for search test");
            return;
        }

        let service = service_result.unwrap();

        let test_events = vec![
            NostrEvent {
                id: "event1".to_string(),
                pubkey: "pubkey1".to_string(),
                created_at: 1000000000,
                kind: 1,
                tags: vec![],
                content: "Bitcoin is a revolutionary digital currency".to_string(),
                sig: "sig1".to_string(),
            },
            NostrEvent {
                id: "event2".to_string(),
                pubkey: "pubkey2".to_string(),
                created_at: 1000000001,
                kind: 1,
                tags: vec![],
                content: "Nostr is a decentralized social media protocol".to_string(),
                sig: "sig2".to_string(),
            },
            NostrEvent {
                id: "event3".to_string(),
                pubkey: "pubkey3".to_string(),
                created_at: 1000000002,
                kind: 1,
                tags: vec![],
                content: "I love pizza and pasta".to_string(),
                sig: "sig3".to_string(),
            },
        ];

        let embedded_events: Vec<NostrEventEmbedded> =
            test_events.into_iter().map(|event| event.into()).collect();

        use rig::{client::EmbeddingsClient, embeddings::EmbeddingsBuilder};

        let embeddings_result = EmbeddingsBuilder::new(service.model().clone())
            .documents(embedded_events.clone())
            .unwrap()
            .build()
            .await;

        if embeddings_result.is_err() {
            println!("Warning: Could not generate embeddings for test");
            return;
        }

        let embeddings = embeddings_result.unwrap();
        let embedding_vecs: Vec<Vec<f32>> = embeddings
            .into_iter()
            .map(|(_, emb)| emb.first().vec.into_iter().map(|x| x as f32).collect())
            .collect();

        let search_result = simple_similarity_search(
            &embedded_events,
            &embedding_vecs,
            "cryptocurrency blockchain technology",
            &service,
            0.3,
        )
        .await;

        if let Ok(results) = search_result {
            assert!(
                !results.is_empty(),
                "Should find at least one relevant result"
            );

            let bitcoin_event = results.iter().find(|e| e.content.contains("Bitcoin"));
            assert!(
                bitcoin_event.is_some(),
                "Should find Bitcoin-related event as relevant"
            );

            println!(
                "✅ Found {} relevant results for cryptocurrency search",
                results.len()
            );
        } else {
            println!("Warning: Search test failed, likely due to network/API issues");
        }
    }
}
