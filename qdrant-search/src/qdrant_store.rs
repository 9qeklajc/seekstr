use crate::nostr::NostrEventWithEmbedding;
use anyhow::Result;
use qdrant_client::{
    qdrant::{
        vectors_config::Config, CreateCollectionBuilder, Distance, Filter, PointId, PointStruct,
        Range, SearchPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder, VectorsConfig,
    },
    Payload, Qdrant,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const MIN_RELEVANCE_THRESHOLD: f32 = 0.5;

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
            let vectors_config = VectorsConfig {
                config: Some(Config::Params(
                    VectorParamsBuilder::new(1024, Distance::Cosine).build(),
                )),
            };

            self.client
                .create_collection(
                    CreateCollectionBuilder::new(&self.collection_name)
                        .vectors_config(vectors_config),
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
        _limit: usize,
    ) -> Result<Vec<String>> {
        self.search_similar_with_range(query_embedding, 1000, None, Some(0.8))
            .await
    }

    pub async fn search_similar_with_range(
        &self,
        query_embedding: &[f32],
        limit: usize,
        _lower_bound: Option<f32>,
        _upper_bound: Option<f32>,
    ) -> Result<Vec<String>> {
        let search_request = SearchPointsBuilder::new(
            &self.collection_name,
            query_embedding.to_vec(),
            limit as u64,
        )
        .with_payload(true);

        let search_result = self.client.search_points(search_request).await?;

        let event_ids: Vec<String> = search_result
            .result
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
        self.search_similar_with_filters_and_range(
            query_embedding,
            limit,
            author,
            kind,
            min_created_at,
            max_created_at,
            None,
            Some(0.8),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn search_similar_with_filters_and_range(
        &self,
        query_embedding: &[f32],
        limit: usize,
        author: Option<&str>,
        kind: Option<i32>,
        min_created_at: Option<i64>,
        max_created_at: Option<i64>,
        _lower_bound: Option<f32>,
        _upper_bound: Option<f32>,
    ) -> Result<Vec<String>> {
        let mut filter_conditions = Vec::new();

        if let Some(author) = author {
            use qdrant_client::qdrant::{Condition, FieldCondition, Match};
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
            use qdrant_client::qdrant::{Condition, FieldCondition, Match};
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
            use qdrant_client::qdrant::{Condition, FieldCondition};
            filter_conditions.push(Condition {
                condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::Field(
                    FieldCondition {
                        key: "created_at".to_string(),
                        range: Some(Range {
                            gte: Some(min_created as f64),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                )),
            });
        }

        if let Some(max_created) = max_created_at {
            use qdrant_client::qdrant::{Condition, FieldCondition};
            filter_conditions.push(Condition {
                condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::Field(
                    FieldCondition {
                        key: "created_at".to_string(),
                        range: Some(Range {
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
        .with_payload(true);

        if !filter_conditions.is_empty() {
            let filter = Filter::must(filter_conditions);
            search_request = search_request.filter(filter);
        }

        let search_result = self.client.search_points(search_request).await?;
        println!("{:?}", search_result);

        let mut results_with_scores: Vec<(String, f32, f32)> = Vec::new();

        for point in search_result.result.iter() {
            let id = match point.payload.get("id").and_then(|v| v.as_str()) {
                Some(id_str) => id_str.to_string(),
                None => continue,
            };

            let distance = point.score;

            if point.score > MIN_RELEVANCE_THRESHOLD {
                results_with_scores.push((id, distance, point.score));
            }
        }

        results_with_scores
            .sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        println!(
            "Results sorted by relevance (highest first), filtered by relevance > {:.2}:",
            MIN_RELEVANCE_THRESHOLD
        );

        let mut event_ids = Vec::new();
        for (i, (id, distance, relevance_score)) in results_with_scores.iter().enumerate() {
            println!(
                "  {}: {} (distance: {:.4}, relevance: {:.4})",
                i + 1,
                id,
                distance,
                relevance_score
            );
            event_ids.push(id.clone());
        }

        println!("{:?}", event_ids);
        Ok(event_ids)
    }

    pub async fn create_index(&self) -> Result<()> {
        Ok(())
    }

    pub async fn create_index_with_type(&self, _index_type: String) -> Result<()> {
        Ok(())
    }

    pub async fn create_ivf_flat_index(&self, _num_partitions: u32) -> Result<()> {
        Ok(())
    }

    pub async fn create_ivf_flat_index_with_distance(
        &self,
        _num_partitions: u32,
        _distance_type: String,
    ) -> Result<()> {
        Ok(())
    }
}
