use crate::indexer::Indexer;
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct Persistence {
    indexer: Arc<RwLock<Indexer>>,
    path: String,
}

impl Persistence {
    pub fn new(indexer: Arc<RwLock<Indexer>>, path: String) -> Self {
        Persistence { indexer, path }
    }

    pub async fn run(&self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Err(e) = self.save_to_file().await {
                anyhow::anyhow!("Failed to save tokens: {:?}", e);
            }
        }
    }

    async fn save_to_file(&self) -> anyhow::Result<()> {
        let tokens = self.indexer.read().await.get_all_tokens();
        let json = serde_json::to_string(&tokens)?;
        let mut file = File::create(&self.path)?;
        file.write_all(json.as_bytes())?;
        Ok(())
    }
}
