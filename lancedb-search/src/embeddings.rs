use anyhow::Result;
use rig::client::EmbeddingsClient;
use rig::embeddings::EmbeddingModel;
use rig::providers::openai;

pub struct EmbeddingService {
    model: openai::embedding::EmbeddingModel,
}

impl EmbeddingService {
    pub fn new() -> Result<Self> {
        let openai_client = openai::ClientBuilder::new("otrta_BiT6hytS2bEoJuP6H4p9X9IHAnwm35Su")
            .base_url("https://ecash.server.otrta.me")
            .build()?;

        let model = openai_client.embedding_model("nomic-embed-text:latest");

        Ok(Self { model })
    }

    pub async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let embedding = self.model.embed_text(text).await?;
        Ok(embedding.vec.into_iter().map(|x| x as f32).collect())
    }
}
