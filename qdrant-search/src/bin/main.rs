use anyhow::Result;
use qdrant_search::{
    embedding_service::EmbeddingSearchService, embeddings::EmbeddingService, nostr::NostrEvent,
    EventSearchRequest,
};

#[tokio::main]
async fn main() -> Result<()> {
    let embedding_service = EmbeddingService::new()?;
    let search_service =
        EmbeddingSearchService::new(embedding_service, "http://localhost:6334", "nostr_events")
            .await?;

    let sample_event = NostrEvent {
        id: "abcd1234567890ef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
        pubkey: "npub1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcd".to_string(),
        created_at: chrono::Utc::now().timestamp(),
        kind: 1,
        tags: vec![vec!["t".to_string(), "rust".to_string()]],
        content: "This is a sample Nostr event about Rust programming and vector databases."
            .to_string(),
        sig: "signature_placeholder_1234567890abcdef1234567890abcdef1234567890".to_string(),
    };

    let sample_event2 = NostrEvent {
        id: "ef1234567890abcd1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
        pubkey: "npub9876543210fedcba9876543210fedcba9876543210fedcba9876543210fe".to_string(),
        created_at: chrono::Utc::now().timestamp() - 3600,
        kind: 1,
        tags: vec![vec!["t".to_string(), "database".to_string()]],
        content: "Vector databases are fascinating for semantic search and AI applications."
            .to_string(),
        sig: "signature_placeholder_9876543210fedcba9876543210fedcba9876543210".to_string(),
    };

    println!("Storing sample events...");
    search_service.embed_and_store_event(&sample_event).await?;
    search_service.embed_and_store_event(&sample_event2).await?;

    println!("Creating index...");
    search_service.create_index().await?;

    let search_request = EventSearchRequest {
        language: None,
        author: None,
        limit: Some(10),
        event_kinds: Some(vec![1]),
        search: Some("vector databases".to_string()),
    };

    println!("Performing semantic search for 'vector databases'...");
    let results = search_service.semantic_search(&search_request).await?;
    println!("Search results: {:?}", results);

    let search_request2 = EventSearchRequest {
        language: None,
        author: None,
        limit: Some(10),
        event_kinds: Some(vec![1]),
        search: Some("Rust programming language".to_string()),
    };

    println!("\nPerforming semantic search for 'Rust programming language'...");
    let results2 = search_service.semantic_search(&search_request2).await?;
    println!("Search results: {:?}", results2);

    Ok(())
}
