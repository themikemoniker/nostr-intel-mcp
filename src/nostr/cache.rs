use anyhow::Context;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

pub struct Cache {
    pool: SqlitePool,
    profile_ttl: i64,
    relay_ttl: i64,
}

#[derive(Debug, Clone)]
pub struct CachedProfile {
    pub pubkey: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub about: Option<String>,
    pub picture: Option<String>,
    pub banner: Option<String>,
    pub nip05: Option<String>,
    pub lud16: Option<String>,
    pub website: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CachedRelayInfo {
    pub relay_url: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub supported_nips: Vec<u32>,
    pub software: Option<String>,
    pub version: Option<String>,
    pub online: bool,
    pub latency_ms: Option<i64>,
}

impl Cache {
    pub async fn new(
        database_path: &str,
        profile_ttl_seconds: u64,
        relay_info_ttl_seconds: u64,
    ) -> anyhow::Result<Self> {
        let options = SqliteConnectOptions::from_str(&format!("sqlite:{database_path}"))
            .context("Invalid database path")?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .context("Failed to connect to SQLite")?;

        let cache = Self {
            pool,
            profile_ttl: profile_ttl_seconds as i64,
            relay_ttl: relay_info_ttl_seconds as i64,
        };

        cache.init_schema().await?;
        Ok(cache)
    }

    async fn init_schema(&self) -> anyhow::Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS profiles (
                pubkey TEXT PRIMARY KEY NOT NULL,
                name TEXT,
                display_name TEXT,
                about TEXT,
                picture TEXT,
                banner TEXT,
                nip05 TEXT,
                lud16 TEXT,
                website TEXT,
                cached_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_profiles_expires ON profiles(expires_at)")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS relay_info (
                relay_url TEXT PRIMARY KEY NOT NULL,
                name TEXT,
                description TEXT,
                supported_nips TEXT,
                software TEXT,
                version TEXT,
                online BOOLEAN NOT NULL DEFAULT 1,
                latency_ms INTEGER,
                cached_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_relay_info_expires ON relay_info(expires_at)")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS rate_limits (
                client_id TEXT NOT NULL,
                day_ordinal INTEGER NOT NULL,
                count INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (client_id, day_ordinal)
            )",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    fn now() -> i64 {
        chrono::Utc::now().timestamp()
    }

    pub async fn get_profile(&self, pubkey: &str) -> anyhow::Result<Option<CachedProfile>> {
        let now = Self::now();
        let row = sqlx::query(
            "SELECT pubkey, name, display_name, about, picture, banner, nip05, lud16, website
             FROM profiles WHERE pubkey = ? AND expires_at > ?",
        )
        .bind(pubkey)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| CachedProfile {
            pubkey: r.get("pubkey"),
            name: r.get("name"),
            display_name: r.get("display_name"),
            about: r.get("about"),
            picture: r.get("picture"),
            banner: r.get("banner"),
            nip05: r.get("nip05"),
            lud16: r.get("lud16"),
            website: r.get("website"),
        }))
    }

    pub async fn set_profile(&self, profile: &CachedProfile) -> anyhow::Result<()> {
        let now = Self::now();
        let expires_at = now + self.profile_ttl;

        sqlx::query(
            "INSERT OR REPLACE INTO profiles
             (pubkey, name, display_name, about, picture, banner, nip05, lud16, website, cached_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&profile.pubkey)
        .bind(&profile.name)
        .bind(&profile.display_name)
        .bind(&profile.about)
        .bind(&profile.picture)
        .bind(&profile.banner)
        .bind(&profile.nip05)
        .bind(&profile.lud16)
        .bind(&profile.website)
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_relay_info(&self, relay_url: &str) -> anyhow::Result<Option<CachedRelayInfo>> {
        let now = Self::now();
        let row = sqlx::query(
            "SELECT relay_url, name, description, supported_nips, software, version, online, latency_ms
             FROM relay_info WHERE relay_url = ? AND expires_at > ?",
        )
        .bind(relay_url)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            let nips_json: Option<String> = r.get("supported_nips");
            let supported_nips = nips_json
                .and_then(|s| serde_json::from_str::<Vec<u32>>(&s).ok())
                .unwrap_or_default();

            CachedRelayInfo {
                relay_url: r.get("relay_url"),
                name: r.get("name"),
                description: r.get("description"),
                supported_nips,
                software: r.get("software"),
                version: r.get("version"),
                online: r.get("online"),
                latency_ms: r.get("latency_ms"),
            }
        }))
    }

    pub async fn set_relay_info(&self, info: &CachedRelayInfo) -> anyhow::Result<()> {
        let now = Self::now();
        let expires_at = now + self.relay_ttl;
        let nips_json = serde_json::to_string(&info.supported_nips)?;

        sqlx::query(
            "INSERT OR REPLACE INTO relay_info
             (relay_url, name, description, supported_nips, software, version, online, latency_ms, cached_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&info.relay_url)
        .bind(&info.name)
        .bind(&info.description)
        .bind(&nips_json)
        .bind(&info.software)
        .bind(&info.version)
        .bind(info.online)
        .bind(info.latency_ms)
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Atomically check and increment a rate limit counter.
    /// Returns `true` if the call is allowed (under the limit), `false` if exhausted.
    pub async fn check_and_increment_rate(
        &self,
        client_id: &str,
        day_ordinal: u32,
        limit: u32,
    ) -> anyhow::Result<bool> {
        // Ensure a row exists for this client+day
        sqlx::query(
            "INSERT OR IGNORE INTO rate_limits (client_id, day_ordinal, count) VALUES (?, ?, 0)",
        )
        .bind(client_id)
        .bind(day_ordinal)
        .execute(&self.pool)
        .await?;

        // Conditionally increment only if under the limit
        let result = sqlx::query(
            "UPDATE rate_limits SET count = count + 1
             WHERE client_id = ? AND day_ordinal = ? AND count < ?",
        )
        .bind(client_id)
        .bind(day_ordinal)
        .bind(limit)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Get the current rate limit count for a client on a given day.
    pub async fn get_rate_count(&self, client_id: &str, day_ordinal: u32) -> anyhow::Result<u32> {
        let row =
            sqlx::query("SELECT count FROM rate_limits WHERE client_id = ? AND day_ordinal = ?")
                .bind(client_id)
                .bind(day_ordinal)
                .fetch_optional(&self.pool)
                .await?;

        Ok(row.map(|r| r.get::<u32, _>("count")).unwrap_or(0))
    }

    pub async fn cleanup_expired(&self) -> anyhow::Result<()> {
        let now = Self::now();
        sqlx::query("DELETE FROM profiles WHERE expires_at < ?")
            .bind(now)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM relay_info WHERE expires_at < ?")
            .bind(now)
            .execute(&self.pool)
            .await?;
        // Clean up rate limit rows from previous days
        let today = current_day_ordinal();
        sqlx::query("DELETE FROM rate_limits WHERE day_ordinal < ?")
            .bind(today)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn current_day_ordinal() -> u32 {
    use chrono::Datelike;
    chrono::Utc::now().ordinal()
}

#[cfg(test)]
impl Cache {
    /// Create an in-memory SQLite cache for tests.
    pub async fn new_in_memory() -> Self {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .expect("valid memory DSN")
            .journal_mode(SqliteJournalMode::Wal);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("connect in-memory SQLite");

        let cache = Self {
            pool,
            profile_ttl: 3600,
            relay_ttl: 600,
        };
        cache.init_schema().await.expect("init schema");
        cache
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allows_under_limit() {
        let cache = Cache::new_in_memory().await;
        for _ in 0..10 {
            let allowed = cache
                .check_and_increment_rate("client1", 1, 10)
                .await
                .unwrap();
            assert!(allowed);
        }
    }

    #[tokio::test]
    async fn blocks_over_limit() {
        let cache = Cache::new_in_memory().await;
        for _ in 0..10 {
            cache
                .check_and_increment_rate("client1", 1, 10)
                .await
                .unwrap();
        }
        let allowed = cache
            .check_and_increment_rate("client1", 1, 10)
            .await
            .unwrap();
        assert!(!allowed);
    }

    #[tokio::test]
    async fn count_tracks_usage() {
        let cache = Cache::new_in_memory().await;
        assert_eq!(cache.get_rate_count("client1", 1).await.unwrap(), 0);

        for i in 1..=5 {
            cache
                .check_and_increment_rate("client1", 1, 10)
                .await
                .unwrap();
            assert_eq!(cache.get_rate_count("client1", 1).await.unwrap(), i);
        }
    }

    #[tokio::test]
    async fn per_client_isolation() {
        let cache = Cache::new_in_memory().await;
        for _ in 0..3 {
            cache
                .check_and_increment_rate("alice", 1, 10)
                .await
                .unwrap();
        }
        for _ in 0..5 {
            cache.check_and_increment_rate("bob", 1, 10).await.unwrap();
        }
        assert_eq!(cache.get_rate_count("alice", 1).await.unwrap(), 3);
        assert_eq!(cache.get_rate_count("bob", 1).await.unwrap(), 5);
    }

    #[tokio::test]
    async fn per_day_isolation() {
        let cache = Cache::new_in_memory().await;
        for _ in 0..3 {
            cache
                .check_and_increment_rate("client1", 100, 10)
                .await
                .unwrap();
        }
        for _ in 0..7 {
            cache
                .check_and_increment_rate("client1", 101, 10)
                .await
                .unwrap();
        }
        assert_eq!(cache.get_rate_count("client1", 100).await.unwrap(), 3);
        assert_eq!(cache.get_rate_count("client1", 101).await.unwrap(), 7);
    }
}
