use crate::{
    embeddings::EmbeddingService,
    nostr::{NostrEvent, NostrEventWithEmbedding},
    qdrant_store::QdrantStore,
    EventSearchRequest, EventSearchResponse,
};
use anyhow::Result;

pub struct EmbeddingSearchService {
    embedding_service: EmbeddingService,
    qdrant_store: QdrantStore,
}

impl EmbeddingSearchService {
    pub async fn new(
        embedding_service: EmbeddingService,
        qdrant_url: &str,
        collection_name: &str,
    ) -> Result<Self> {
        let qdrant_store = QdrantStore::new(qdrant_url, collection_name).await?;

        Ok(Self {
            embedding_service,
            qdrant_store,
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
        match self.qdrant_store.insert_event(&embedded_event).await {
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
            match self.qdrant_store.insert_events(&embedded_events).await {
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
        let limit = request.limit.unwrap_or(200);

        let query_embedding = self.embedding_service.generate_embedding(query).await?;

        let author = request.author.as_deref();
        let kind = request
            .event_kinds
            .as_ref()
            .and_then(|kinds| kinds.first())
            .map(|&k| k as i32);

        match self
            .qdrant_store
            .search_similar_with_filters(&query_embedding, limit, author, kind, None, None)
            .await
        {
            Ok(event_ids) => Ok(EventSearchResponse {
                total_found: event_ids.len(),
                event_ids,
            }),
            Err(e) => {
                let error_msg = e.to_string().to_lowercase();
                if error_msg.contains("collection") && error_msg.contains("not found") {
                    eprintln!("Warning: Collection not found or empty, returning empty results.");
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
        match self.qdrant_store.create_index().await {
            Ok(()) => Ok(()),
            Err(e) => {
                let error_msg = e.to_string().to_lowercase();
                if error_msg.contains("not enough") || error_msg.contains("insufficient") {
                    eprintln!(
                        "Warning: Not enough data to optimize index. Index optimization will happen automatically as more data is added."
                    );
                    Ok(())
                } else if error_msg.contains("index already")
                    || error_msg.contains("already optimized")
                {
                    eprintln!("Warning: Collection is already optimized.");
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }
}
