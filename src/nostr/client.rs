use nostr_sdk::prelude::*;
use std::time::Duration;

pub struct NostrClient {
    client: Client,
}

impl NostrClient {
    pub async fn new(relay_urls: Vec<String>) -> anyhow::Result<Self> {
        let client = Client::default();

        for url in &relay_urls {
            if let Err(e) = client.add_relay(url).await {
                tracing::warn!("Failed to add relay {url}: {e}");
            }
        }

        client.connect().await;
        tracing::info!("Nostr client connected to relay pool");

        Ok(Self { client })
    }

    pub async fn get_metadata(&self, pubkey: &PublicKey) -> anyhow::Result<Option<Metadata>> {
        let filter = Filter::new()
            .kind(Kind::Metadata)
            .author(*pubkey)
            .limit(1);

        let timeout = Duration::from_secs(10);
        let events = self.client.fetch_events(filter, timeout).await?;

        if let Some(event) = events.first() {
            let metadata = Metadata::from_json(&event.content)?;
            Ok(Some(metadata))
        } else {
            Ok(None)
        }
    }

    pub async fn search_events(
        &self,
        authors: Option<Vec<PublicKey>>,
        kinds: Option<Vec<Kind>>,
        search: Option<String>,
        since: Option<Timestamp>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<Event>> {
        let mut filter = Filter::new();

        if let Some(authors) = authors {
            filter = filter.authors(authors);
        }
        if let Some(kinds) = kinds {
            filter = filter.kinds(kinds);
        }
        if let Some(search) = search {
            filter = filter.search(search);
        }
        if let Some(since) = since {
            filter = filter.since(since);
        }

        let limit = limit.unwrap_or(20).min(100);
        filter = filter.limit(limit as usize);

        let timeout = Duration::from_secs(15);
        let events = self.client.fetch_events(filter, timeout).await?;

        Ok(events.into_iter().collect())
    }

    pub fn parse_pubkey(input: &str) -> anyhow::Result<PublicKey> {
        // Try npub (bech32)
        if let Ok(pk) = PublicKey::from_bech32(input) {
            return Ok(pk);
        }
        // Try hex
        if let Ok(pk) = PublicKey::from_hex(input) {
            return Ok(pk);
        }
        anyhow::bail!("Invalid pubkey format: {input}")
    }
}
