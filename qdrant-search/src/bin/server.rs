use anyhow::Result;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use qdrant_search::{
    embedding_service::EmbeddingSearchService,
    embeddings::EmbeddingService,
    event_queue::{EventProcessor, EventQueue},
    nostr::NostrEvent,
    EventSearchRequest,
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

    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());

    let server_host = std::env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let server_port = std::env::var("SERVER_PORT")
        .unwrap_or_else(|_| "3009".to_string())
        .parse::<u16>()
        .unwrap_or(3009);

    let embedding_service = Arc::new(
        EmbeddingSearchService::new(embedding_service, &qdrant_url, "nostr_events").await?,
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
        .route("/health", get(health_check))
        .with_state(state)
        .layer(CorsLayer::permissive());

    let bind_address = format!("{}:{}", server_host, server_port);
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    println!("Qdrant Search Server running on http://{}", bind_address);
    println!("Endpoints:");
    println!("  GET  /health - Health check");
    println!("  GET  /events - Search events with filters");
    println!("  POST /events - Submit new event");
    println!("  GET  /search - Semantic search");

    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "qdrant-search",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

async fn get_events(
    State(state): State<AppState>,
    Query(params): Query<serde_json::Value>,
) -> Result<Json<SemanticSearchResponse>, StatusCode> {
    let request: EventSearchRequest = serde_json::from_value(params).map_err(|e| {
        eprintln!("Failed to parse EventSearchRequest: {}", e);
        StatusCode::BAD_REQUEST
    })?;

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
