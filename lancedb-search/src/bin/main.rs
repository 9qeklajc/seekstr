use anyhow::Result;
use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
};
use lancedb_search::{
    EventSearchRequest,
    embedding_service::EmbeddingSearchService,
    embeddings::EmbeddingService,
    event_queue::{EventProcessor, EventQueue},
    nostr::NostrEvent,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

#[derive(Clone)]
struct AppState {
    embedding_service: Arc<EmbeddingSearchService>,
    event_queue: EventQueue,
}

#[derive(Debug, Serialize, Deserialize)]
struct SemanticSearchRequest {
    query: String,
    limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SemanticSearchResponse {
    event_ids: Vec<String>,
    total_found: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let embedding_service = EmbeddingService::new()?;

    let embedding_service = Arc::new(
        EmbeddingSearchService::new(embedding_service, "./lancedb_data", "nostr_events").await?,
    );

    embedding_service.create_index().await.ok();

    let (event_queue, receiver) = EventQueue::new();
    let processor = EventProcessor::new(embedding_service.clone(), receiver);

    tokio::spawn(async move {
        processor.start_processing().await;
    });

    let state = AppState {
        embedding_service,
        event_queue,
    };

    let app = Router::new()
        .route("/events", get(get_events))
        .route("/events", post(post_event))
        .route("/search", get(semantic_search))
        .with_state(state)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3009").await?;
    println!("Server running on http://0.0.0.0:3009");

    axum::serve(listener, app).await?;

    Ok(())
}

async fn get_events(
    State(state): State<AppState>,
    Query(params): Query<serde_json::Value>,
) -> Result<Json<SemanticSearchResponse>, StatusCode> {
    let request: EventSearchRequest =
        serde_json::from_value(params).map_err(|_| StatusCode::BAD_REQUEST)?;

    match state.embedding_service.semantic_search(&request).await {
        Ok(response) => {
            let search_response = SemanticSearchResponse {
                total_found: response.total_found,
                event_ids: response.event_ids,
            };
            Ok(Json(search_response))
        }
        Err(e) => {
            eprintln!("Search error: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn post_event(
    State(state): State<AppState>,
    Json(request): Json<NostrEvent>,
) -> Result<(), StatusCode> {
    println!("Received event for queueing: {}", request.id);

    match state.event_queue.enqueue(request) {
        Ok(()) => {
            println!("Event queued successfully");
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to queue event: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn semantic_search(
    State(state): State<AppState>,
    Query(params): Query<serde_json::Value>,
) -> Result<Json<SemanticSearchResponse>, StatusCode> {
    let request: SemanticSearchRequest = serde_json::from_value(params).map_err(|e| {
        eprintln!("Failed to parse SemanticSearchRequest: {}", e);
        eprintln!("Expected fields: query, limit");
        StatusCode::BAD_REQUEST
    })?;

    println!("Parsed semantic search request: {:?}", request);

    let search_request = EventSearchRequest {
        language: None,
        author: None,
        limit: request.limit,
        event_kinds: None,
        search: Some(request.query),
    };

    match state
        .embedding_service
        .semantic_search(&search_request)
        .await
    {
        Ok(response) => {
            let search_response = SemanticSearchResponse {
                total_found: response.total_found,
                event_ids: response.event_ids,
            };
            Ok(Json(search_response))
        }
        Err(e) => {
            eprintln!("Semantic search error: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
