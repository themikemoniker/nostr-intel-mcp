use nostr_sdk::prelude::*;
use std::time::Duration;

pub struct NostrClient {
    client: Client,
}

impl NostrClient {
    pub fn client(&self) -> &Client {
        &self.client
    }

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
        let filter = Filter::new().kind(Kind::Metadata).author(*pubkey).limit(1);

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

    /// Fetch kind:10002 (NIP-65 relay list metadata) for a pubkey
    pub async fn fetch_relay_list(&self, pubkey: &PublicKey) -> anyhow::Result<Vec<Event>> {
        let filter = Filter::new().kind(Kind::RelayList).author(*pubkey).limit(1);

        let timeout = Duration::from_secs(10);
        let events = self.client.fetch_events(filter, timeout).await?;
        Ok(events.into_iter().collect())
    }

    /// Fetch kind:3 (contact list) for a pubkey
    pub async fn fetch_contact_list(&self, pubkey: &PublicKey) -> anyhow::Result<Option<Event>> {
        let filter = Filter::new()
            .kind(Kind::ContactList)
            .author(*pubkey)
            .limit(1);

        let timeout = Duration::from_secs(10);
        let events = self.client.fetch_events(filter, timeout).await?;
        Ok(events.into_iter().next())
    }

    /// Fetch events by their IDs
    #[allow(dead_code)]
    pub async fn fetch_events_by_ids(&self, ids: Vec<EventId>) -> anyhow::Result<Vec<Event>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let filter = Filter::new().ids(ids);
        let timeout = Duration::from_secs(10);
        let events = self.client.fetch_events(filter, timeout).await?;
        Ok(events.into_iter().collect())
    }

    /// Fetch kind:7 reactions referencing the given event IDs
    pub async fn fetch_reactions(
        &self,
        event_ids: &[EventId],
        since: Option<Timestamp>,
    ) -> anyhow::Result<Vec<Event>> {
        if event_ids.is_empty() {
            return Ok(vec![]);
        }
        let mut filter = Filter::new()
            .kind(Kind::Reaction)
            .events(event_ids.to_vec());
        if let Some(since) = since {
            filter = filter.since(since);
        }
        let timeout = Duration::from_secs(15);
        let events = self.client.fetch_events(filter, timeout).await?;
        Ok(events.into_iter().collect())
    }

    /// Fetch kind:6 reposts referencing the given event IDs
    pub async fn fetch_reposts(
        &self,
        event_ids: &[EventId],
        since: Option<Timestamp>,
    ) -> anyhow::Result<Vec<Event>> {
        if event_ids.is_empty() {
            return Ok(vec![]);
        }
        let mut filter = Filter::new().kind(Kind::Repost).events(event_ids.to_vec());
        if let Some(since) = since {
            filter = filter.since(since);
        }
        let timeout = Duration::from_secs(15);
        let events = self.client.fetch_events(filter, timeout).await?;
        Ok(events.into_iter().collect())
    }

    /// Fetch kind:9735 zap receipts where the `p` tag matches the pubkey
    pub async fn fetch_zap_receipts(
        &self,
        pubkey: &PublicKey,
        since: Option<Timestamp>,
    ) -> anyhow::Result<Vec<Event>> {
        let mut filter = Filter::new().kind(Kind::ZapReceipt).pubkey(*pubkey);
        if let Some(since) = since {
            filter = filter.since(since);
        }
        let timeout = Duration::from_secs(15);
        let events = self.client.fetch_events(filter, timeout).await?;
        Ok(events.into_iter().collect())
    }

    /// Fetch kind:1 text notes from the given timeframe
    pub async fn fetch_recent_notes(
        &self,
        since: Timestamp,
        limit: usize,
    ) -> anyhow::Result<Vec<Event>> {
        let filter = Filter::new().kind(Kind::TextNote).since(since).limit(limit);
        let timeout = Duration::from_secs(15);
        let events = self.client.fetch_events(filter, timeout).await?;
        Ok(events.into_iter().collect())
    }

    /// Reconnect to all relays in the pool. Called by background health check.
    pub async fn reconnect(&self) {
        tracing::debug!("Reconnecting to relay pool");
        self.client.connect().await;
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
