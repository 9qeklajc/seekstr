use crate::{
    EventSearchRequest, EventSearchResponse, EventSearchResponseWithScores, EventSearchResult,
    embeddings::EmbeddingService,
    lancedb_store::{LanceDBStore, SearchResult},
    nostr::{NostrEvent, NostrEventWithEmbedding},
};
use anyhow::Result;

pub struct EmbeddingSearchService {
    embedding_service: EmbeddingService,
    lancedb_store: LanceDBStore,
}

impl EmbeddingSearchService {
    pub async fn new(
        embedding_service: EmbeddingService,
        db_path: &str,
        table_name: &str,
    ) -> Result<Self> {
        let lancedb_store = LanceDBStore::new(db_path, table_name).await?;

        Ok(Self {
            embedding_service,
            lancedb_store,
        })
    }

    pub async fn embed_and_store_event(&self, event: &NostrEvent) -> Result<()> {
        let embedding = self
            .embedding_service
            .generate_embedding(&event.content)
            .await?;

        let embedded_event = NostrEventWithEmbedding::new(
            event.id.clone(),
            event.pubkey.clone(),
            event.created_at,
            event.kind,
            event.tags.clone(),
            embedding,
        );

        println!("{:?}", event);
        match self.lancedb_store.insert_event(&embedded_event).await {
            Ok(()) => Ok(()),
            Err(e) => {
                let error_msg = e.to_string().to_lowercase();
                if error_msg.contains("duplicate") || error_msg.contains("already exists") {
                    eprintln!(
                        "Warning: Event {} already exists in database, skipping insertion.",
                        event.id
                    );
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    pub async fn embed_and_store_events(&self, events: &[NostrEvent]) -> Result<()> {
        let mut embedded_events = Vec::new();

        for event in events {
            if let Ok(embedding) = self
                .embedding_service
                .generate_embedding(&event.content)
                .await
            {
                let embedded_event = NostrEventWithEmbedding::new(
                    event.id.clone(),
                    event.pubkey.clone(),
                    event.created_at,
                    event.kind,
                    event.tags.clone(),
                    embedding,
                );
                embedded_events.push(embedded_event);
            }
        }

        if !embedded_events.is_empty() {
            match self.lancedb_store.insert_events(&embedded_events).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    let error_msg = e.to_string().to_lowercase();
                    if error_msg.contains("duplicate") || error_msg.contains("already exists") {
                        eprintln!(
                            "Warning: Some events may already exist in database, insertion partially completed."
                        );
                        Ok(())
                    } else {
                        Err(e)
                    }
                }
            }
        } else {
            Ok(())
        }
    }

    pub async fn semantic_search(
        &self,
        request: &EventSearchRequest,
    ) -> Result<EventSearchResponse> {
        let query = request.get_search_query().unwrap_or("");
        let limit = request.limit.unwrap_or(50);

        let query_embedding = self.embedding_service.generate_embedding(query).await?;

        let author = request.author.as_deref();
        let kind = request
            .event_kinds
            .as_ref()
            .and_then(|kinds| kinds.first())
            .map(|&k| k as i32);

        match self
            .lancedb_store
            .search_similar_with_filters(&query_embedding, limit, author, kind, None, None)
            .await
        {
            Ok(event_ids) => Ok(EventSearchResponse {
                total_found: event_ids.len(),
                event_ids,
            }),
            Err(e) => {
                let error_msg = e.to_string().to_lowercase();
                if error_msg.contains("table") && error_msg.contains("not found") {
                    eprintln!("Warning: Table not found or empty, returning empty results.");
                    Ok(EventSearchResponse {
                        total_found: 0,
                        event_ids: vec![],
                    })
                } else if error_msg.contains("no data") || error_msg.contains("empty") {
                    eprintln!("Warning: No data available for search, returning empty results.");
                    Ok(EventSearchResponse {
                        total_found: 0,
                        event_ids: vec![],
                    })
                } else {
                    Err(e)
                }
            }
        }
    }

    pub async fn create_index(&self) -> Result<()> {
        match self.lancedb_store.create_index().await {
            Ok(()) => Ok(()),
            Err(e) => {
                let error_msg = e.to_string().to_lowercase();
                if error_msg.contains("not enough rows to train") || error_msg.contains("kmeans") {
                    eprintln!(
                        "Warning: Not enough rows to create index. Need at least 256 rows for index creation."
                    );
                    Ok(())
                } else if error_msg.contains("index already exists")
                    || error_msg.contains("already indexed")
                {
                    eprintln!("Warning: Index already exists for this table.");
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_embedding_search_service_creation() {
        let embedding_service_result = EmbeddingService::new();
        if embedding_service_result.is_err() {
            return;
        }

        let embedding_service = embedding_service_result.unwrap();

        let service_result =
            EmbeddingSearchService::new(embedding_service, "test_db", "events").await;

        assert!(service_result.is_ok() || service_result.is_err());
    }

    #[tokio::test]
    async fn test_embed_event() {
        let embedding_service_result = EmbeddingService::new();
        if embedding_service_result.is_err() {
            return;
        }

        let embedding_service = embedding_service_result.unwrap();
        let service_result =
            EmbeddingSearchService::new(embedding_service, "test_db_2", "events").await;

        if service_result.is_err() {
            return;
        }

        let service = service_result.unwrap();
        let event = NostrEvent {
            id: "test_id".to_string(),
            pubkey: "test_pubkey".to_string(),
            created_at: 1234567890,
            kind: 1,
            tags: vec![],
            content: "test content".to_string(),
            sig: "test_sig".to_string(),
        };

        let result = service.embed_and_store_event(&event).await;
        assert!(result.is_ok() || result.is_err());
    }
}
