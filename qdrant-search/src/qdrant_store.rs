use crate::nostr::NostrEventWithEmbedding;
use anyhow::Result;
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, Distance, Filter, PointId, PointStruct,
    ScalarQuantizationBuilder, SearchParamsBuilder, SearchPointsBuilder, UpsertPointsBuilder,
    VectorParamsBuilder,
};
use qdrant_client::{Payload, Qdrant};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const MIN_RELEVANCE_THRESHOLD: f32 = 0.4;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub event_id: String,
    pub distance: f32,
    pub relevance_score: f32,
}

pub struct QdrantStore {
    client: Qdrant,
    collection_name: String,
}

impl QdrantStore {
    fn string_to_point_id(s: &str) -> PointId {
        let mut hasher = DefaultHasher::new();
        s.hash(&mut hasher);
        let hash = hasher.finish();
        PointId::from(hash)
    }

    pub async fn new(url: &str, collection_name: &str) -> Result<Self> {
        let mut client_builder = Qdrant::from_url(url);

        if let Ok(api_key) = std::env::var("QDRANT_API_KEY") {
            client_builder = client_builder.api_key(api_key);
        }

        let client = client_builder.build()?;

        let store = Self {
            client,
            collection_name: collection_name.to_string(),
        };

        store.create_collection_if_not_exists().await?;
        Ok(store)
    }

    async fn create_collection_if_not_exists(&self) -> Result<()> {
        let collections = self.client.list_collections().await?;
        let collection_exists = collections
            .collections
            .iter()
            .any(|c| c.name == self.collection_name);

        if !collection_exists {
            self.client
                .create_collection(
                    CreateCollectionBuilder::new(&self.collection_name)
                        .vectors_config(VectorParamsBuilder::new(768, Distance::Cosine))
                        .quantization_config(ScalarQuantizationBuilder::default()),
                )
                .await?;
        }

        Ok(())
    }

    pub async fn insert_event(&self, event: &NostrEventWithEmbedding) -> Result<()> {
        let payload = self.create_payload(event)?;

        let point = PointStruct::new(
            Self::string_to_point_id(&event.id),
            event.content_embedding.clone(),
            payload,
        );

        self.client
            .upsert_points(UpsertPointsBuilder::new(&self.collection_name, vec![point]))
            .await?;

        Ok(())
    }

    pub async fn insert_events(&self, events: &[NostrEventWithEmbedding]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        let mut points = Vec::new();
        for event in events {
            let payload = self.create_payload(event)?;
            let point = PointStruct::new(
                Self::string_to_point_id(&event.id),
                event.content_embedding.clone(),
                payload,
            );
            points.push(point);
        }

        self.client
            .upsert_points(UpsertPointsBuilder::new(&self.collection_name, points))
            .await?;

        Ok(())
    }

    fn create_payload(&self, event: &NostrEventWithEmbedding) -> Result<Payload> {
        let mut payload = Payload::new();

        payload.insert("id", event.id.clone());
        payload.insert("pubkey", event.pubkey.clone());
        payload.insert("created_at", event.created_at);
        payload.insert("kind", event.kind as i64);
        payload.insert("tags", event.tags.clone());

        Ok(payload)
    }

    pub async fn search_similar(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<String>> {
        let search_result = self
            .client
            .search_points(
                SearchPointsBuilder::new(
                    &self.collection_name,
                    query_embedding.to_vec(),
                    limit as u64,
                )
                .with_payload(true)
                .params(SearchParamsBuilder::default().exact(false)),
            )
            .await?;

        let mut filtered_results: Vec<_> = search_result
            .result
            .iter()
            .filter(|point| point.score > MIN_RELEVANCE_THRESHOLD)
            .collect();

        filtered_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let event_ids: Vec<String> = filtered_results
            .iter()
            .filter_map(|point| {
                point
                    .payload
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .collect();

        Ok(event_ids)
    }

    pub async fn search_similar_with_filters(
        &self,
        query_embedding: &[f32],
        limit: usize,
        author: Option<&str>,
        kind: Option<i32>,
        min_created_at: Option<i64>,
        max_created_at: Option<i64>,
    ) -> Result<Vec<String>> {
        let mut filter_conditions = Vec::new();

        if let Some(author) = author {
            use qdrant_client::qdrant::{FieldCondition, Match};
            filter_conditions.push(Condition {
                condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::Field(
                    FieldCondition {
                        key: "pubkey".to_string(),
                        r#match: Some(Match {
                            match_value: Some(qdrant_client::qdrant::r#match::MatchValue::Text(
                                author.to_string(),
                            )),
                        }),
                        ..Default::default()
                    },
                )),
            });
        }

        if let Some(kind) = kind {
            use qdrant_client::qdrant::{FieldCondition, Match};
            filter_conditions.push(Condition {
                condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::Field(
                    FieldCondition {
                        key: "kind".to_string(),
                        r#match: Some(Match {
                            match_value: Some(qdrant_client::qdrant::r#match::MatchValue::Integer(
                                kind as i64,
                            )),
                        }),
                        ..Default::default()
                    },
                )),
            });
        }

        if let Some(min_created) = min_created_at {
            use qdrant_client::qdrant::FieldCondition;
            filter_conditions.push(Condition {
                condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::Field(
                    FieldCondition {
                        key: "created_at".to_string(),
                        range: Some(qdrant_client::qdrant::Range {
                            gte: Some(min_created as f64),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                )),
            });
        }

        if let Some(max_created) = max_created_at {
            use qdrant_client::qdrant::FieldCondition;
            filter_conditions.push(Condition {
                condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::Field(
                    FieldCondition {
                        key: "created_at".to_string(),
                        range: Some(qdrant_client::qdrant::Range {
                            lte: Some(max_created as f64),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                )),
            });
        }

        let mut search_request = SearchPointsBuilder::new(
            &self.collection_name,
            query_embedding.to_vec(),
            limit as u64,
        )
        .with_payload(true)
        .params(SearchParamsBuilder::default().exact(false));

        if !filter_conditions.is_empty() {
            let filter = Filter::should(filter_conditions);
            search_request = search_request.filter(filter);
        }

        let search_result = self.client.search_points(search_request).await?;

        let mut filtered_results: Vec<_> = search_result
            .result
            .iter()
            .filter(|point| point.score > MIN_RELEVANCE_THRESHOLD)
            .collect();

        filtered_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        println!("{:?}", filtered_results);
        let event_ids: Vec<String> = filtered_results
            .iter()
            .filter_map(|point| {
                point
                    .payload
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .collect();

        Ok(event_ids)
    }

    pub async fn create_index(&self) -> Result<()> {
        Ok(())
    }
}
