use crate::nostr::NostrEvent;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct EventQueue {
    sender: mpsc::UnboundedSender<NostrEvent>,
}

impl EventQueue {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<NostrEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (Self { sender }, receiver)
    }

    pub fn enqueue(&self, event: NostrEvent) -> Result<()> {
        self.sender
            .send(event)
            .map_err(|_| anyhow::anyhow!("Failed to enqueue event: channel closed"))?;
        Ok(())
    }
}

pub struct EventProcessor {
    embedding_service: Arc<crate::embedding_service::EmbeddingSearchService>,
    receiver: mpsc::UnboundedReceiver<NostrEvent>,
}

impl EventProcessor {
    pub fn new(
        embedding_service: Arc<crate::embedding_service::EmbeddingSearchService>,
        receiver: mpsc::UnboundedReceiver<NostrEvent>,
    ) -> Self {
        Self {
            embedding_service,
            receiver,
        }
    }

    pub async fn start_processing(mut self) {
        println!("Event processor started");

        while let Some(event) = self.receiver.recv().await {
            println!("Processing event: {}", event.id);

            match self.embedding_service.embed_and_store_event(&event).await {
                Ok(()) => {
                    println!("Successfully processed event: {}", event.id);
                }
                Err(e) => {
                    eprintln!("Failed to process event {}: {}", event.id, e);
                }
            }
        }

        println!("Event processor stopped");
    }
}
