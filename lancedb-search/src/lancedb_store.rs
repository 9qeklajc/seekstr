use crate::nostr::NostrEventWithEmbedding;
use anyhow::Result;
use arrow_array::{
    Array, FixedSizeListArray, Float32Array, Int64Array, RecordBatch, RecordBatchIterator,
    StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{Connection, connect};
use std::sync::Arc;

const MIN_RELEVANCE_THRESHOLD: f32 = 0.50;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub event_id: String,
    pub distance: f32,
    pub relevance_score: f32,
}

pub struct LanceDBStore {
    connection: Connection,
    table_name: String,
}

impl LanceDBStore {
    pub async fn new(db_path: &str, table_name: &str) -> Result<Self> {
        let connection = connect(db_path).execute().await?;

        let store = Self {
            connection,
            table_name: table_name.to_string(),
        };

        store.create_table_if_not_exists().await?;
        Ok(store)
    }

    async fn create_table_if_not_exists(&self) -> Result<()> {
        let table_names = self.connection.table_names().execute().await?;

        if !table_names.contains(&self.table_name) {
            let schema = self.get_schema();
            let empty_batch = RecordBatch::new_empty(schema.clone());
            let batches = RecordBatchIterator::new(vec![empty_batch].into_iter().map(Ok), schema);

            self.connection
                .create_table(&self.table_name, Box::new(batches))
                .execute()
                .await?;
        }

        Ok(())
    }

    fn get_schema(&self) -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("pubkey", DataType::Utf8, false),
            Field::new("created_at", DataType::Int64, false),
            Field::new("kind", DataType::Int64, false),
            Field::new("tags", DataType::Utf8, false),
            Field::new(
                "content_embedding",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    1024,
                ),
                false,
            ),
        ]))
    }

    pub async fn insert_event(&self, event: &NostrEventWithEmbedding) -> Result<()> {
        let schema = self.get_schema();

        let id_array = StringArray::from(vec![event.id.clone()]);
        let pubkey_array = StringArray::from(vec![event.pubkey.clone()]);
        let created_at_array = Int64Array::from(vec![event.created_at]);
        let kind_array = Int64Array::from(vec![event.kind as i64]);
        let tags_array = StringArray::from(vec![event.tags.clone()]);

        let embedding_array =
            FixedSizeListArray::from_iter_primitive::<arrow_array::types::Float32Type, _, _>(
                std::iter::once(Some(
                    event
                        .content_embedding
                        .iter()
                        .map(|&x| Some(x))
                        .collect::<Vec<_>>(),
                )),
                1024,
            );

        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(id_array),
                Arc::new(pubkey_array),
                Arc::new(created_at_array),
                Arc::new(kind_array),
                Arc::new(tags_array),
                Arc::new(embedding_array),
            ],
        )?;

        let table = self
            .connection
            .open_table(&self.table_name)
            .execute()
            .await?;
        let batches = RecordBatchIterator::new(vec![batch].into_iter().map(Ok), self.get_schema());
        table.add(Box::new(batches)).execute().await?;

        Ok(())
    }

    pub async fn insert_events(&self, events: &[NostrEventWithEmbedding]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        let schema = self.get_schema();

        let ids: Vec<String> = events.iter().map(|e| e.id.clone()).collect();
        let pubkeys: Vec<String> = events.iter().map(|e| e.pubkey.clone()).collect();
        let created_ats: Vec<i64> = events.iter().map(|e| e.created_at).collect();
        let kinds: Vec<i64> = events.iter().map(|e| e.kind as i64).collect();
        let tags: Vec<String> = events.iter().map(|e| e.tags.clone()).collect();

        let embeddings: Vec<Vec<Option<f32>>> = events
            .iter()
            .map(|e| e.content_embedding.iter().map(|&x| Some(x)).collect())
            .collect();

        let id_array = StringArray::from(ids);
        let pubkey_array = StringArray::from(pubkeys);
        let created_at_array = Int64Array::from(created_ats);
        let kind_array = Int64Array::from(kinds);
        let tags_array = StringArray::from(tags);

        let embedding_array = FixedSizeListArray::from_iter_primitive::<
            arrow_array::types::Float32Type,
            _,
            _,
        >(embeddings.into_iter().map(Some), 1024);

        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(id_array),
                Arc::new(pubkey_array),
                Arc::new(created_at_array),
                Arc::new(kind_array),
                Arc::new(tags_array),
                Arc::new(embedding_array),
            ],
        )?;

        let table = self
            .connection
            .open_table(&self.table_name)
            .execute()
            .await?;
        let batches = RecordBatchIterator::new(vec![batch].into_iter().map(Ok), self.get_schema());
        table.add(Box::new(batches)).execute().await?;

        Ok(())
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
        _limit: usize,
        _lower_bound: Option<f32>,
        _upper_bound: Option<f32>,
    ) -> Result<Vec<String>> {
        let table = self
            .connection
            .open_table(&self.table_name)
            .execute()
            .await?;

        let results = table.query().nearest_to(query_embedding)?.execute().await?;

        let mut event_ids = Vec::new();
        let batches = results.try_collect::<Vec<_>>().await?;

        for batch in batches {
            if let Some(id_column) = batch.column_by_name("id")
                && let Some(string_array) = id_column.as_any().downcast_ref::<StringArray>()
            {
                for i in 0..string_array.len() {
                    let id = string_array.value(i).to_string();
                    event_ids.push(id);
                }
            }
        }

        Ok(event_ids)
    }

    pub async fn search_similar_with_filters(
        &self,
        query_embedding: &[f32],
        _limit: usize,
        author: Option<&str>,
        kind: Option<i32>,
        min_created_at: Option<i64>,
        max_created_at: Option<i64>,
    ) -> Result<Vec<String>> {
        self.search_similar_with_filters_and_range(
            query_embedding,
            1000,
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
        _limit: usize,
        author: Option<&str>,
        kind: Option<i32>,
        min_created_at: Option<i64>,
        max_created_at: Option<i64>,
        _lower_bound: Option<f32>,
        _upper_bound: Option<f32>,
    ) -> Result<Vec<String>> {
        let table = self
            .connection
            .open_table(&self.table_name)
            .execute()
            .await?;

        let mut vector_query = table
            .query()
            .nearest_to(query_embedding)?
            .column("content_embedding");

        let mut filter_clauses = Vec::new();

        if let Some(author) = author {
            filter_clauses.push(format!("pubkey = '{}'", author));
        }

        if let Some(kind) = kind {
            filter_clauses.push(format!("kind = {}", kind));
        }

        if let Some(min_created) = min_created_at {
            filter_clauses.push(format!("created_at >= {}", min_created));
        }

        if let Some(max_created) = max_created_at {
            filter_clauses.push(format!("created_at <= {}", max_created));
        }

        if !filter_clauses.is_empty() {
            let filter_condition = filter_clauses.join(" AND ");
            vector_query = vector_query.only_if(&filter_condition);
        }

        let results = vector_query.execute().await?;

        let mut event_ids = Vec::new();
        let batches = results.try_collect::<Vec<_>>().await?;

        for batch in batches {
            if let (Some(id_column), Some(distance_column)) = (
                batch.column_by_name("id"),
                batch.column_by_name("_distance"),
            ) {
                if let (Some(string_array), Some(distance_array)) = (
                    id_column.as_any().downcast_ref::<StringArray>(),
                    distance_column.as_any().downcast_ref::<Float32Array>(),
                ) {
                    let mut results_with_scores: Vec<(String, f32)> = Vec::new();

                    for i in 0..string_array.len() {
                        let id = string_array.value(i).to_string();
                        let distance = distance_array.value(i);
                        results_with_scores.push((id, distance));
                    }

                    results_with_scores
                        .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

                    println!(
                        "Results sorted by relevance (distance), filtered by relevance > 0.54:"
                    );
                    for (_i, (id, distance)) in results_with_scores.iter().enumerate() {
                        let relevance_score = (1.0 / (1.0 + distance)).max(0.0).min(1.0);

                        if relevance_score > MIN_RELEVANCE_THRESHOLD {
                            println!(
                                "  {}: {} (distance: {:.4}, relevance: {:.4})",
                                event_ids.len() + 1,
                                id,
                                distance,
                                relevance_score
                            );
                            event_ids.push(id.clone());
                        } else {
                            println!(
                                "  Filtered out: {} (distance: {:.4}, relevance: {:.4}) - below threshold",
                                id, distance, relevance_score
                            );
                        }
                    }
                } else if let Some(string_array) = id_column.as_any().downcast_ref::<StringArray>()
                {
                    for i in 0..string_array.len() {
                        let id = string_array.value(i).to_string();
                        event_ids.push(id);
                    }
                }
            }
        }

        println!("{:?}", event_ids);
        Ok(event_ids)
    }

    pub async fn create_index(&self) -> Result<()> {
        self.create_index_with_type(lancedb::index::Index::Auto)
            .await
    }

    pub async fn create_index_with_type(&self, index_type: lancedb::index::Index) -> Result<()> {
        let table = self
            .connection
            .open_table(&self.table_name)
            .execute()
            .await?;
        table
            .create_index(&["content_embedding"], index_type)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn create_ivf_flat_index(&self, _num_partitions: u32) -> Result<()> {
        let table = self
            .connection
            .open_table(&self.table_name)
            .execute()
            .await?;
        table
            .create_index(&["content_embedding"], lancedb::index::Index::Auto)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn create_ivf_flat_index_with_distance(
        &self,
        _num_partitions: u32,
        _distance_type: lancedb::DistanceType,
    ) -> Result<()> {
        self.create_index().await
    }
}
