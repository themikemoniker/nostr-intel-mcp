use serde::Deserialize;

const PRIMAL_API_URL: &str = "https://cache1.primal.net/api";

pub struct ProfileSearchClient {
    http: reqwest::Client,
}

pub struct ProfileSearchHit {
    pub pubkey: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub about: Option<String>,
    pub picture: Option<String>,
    pub nip05: Option<String>,
    pub lud16: Option<String>,
    pub website: Option<String>,
    pub followers_count: Option<u64>,
}

#[derive(Deserialize)]
struct PrimalEvent {
    kind: u32,
    pubkey: Option<String>,
    content: Option<String>,
}

#[derive(Deserialize)]
struct ProfileMetadata {
    name: Option<String>,
    display_name: Option<String>,
    about: Option<String>,
    picture: Option<String>,
    nip05: Option<String>,
    lud16: Option<String>,
    website: Option<String>,
}

impl ProfileSearchClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
        }
    }

    pub async fn search_profiles(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<ProfileSearchHit>, String> {
        let body = serde_json::json!(["user_search", {"query": query, "limit": limit}]);

        let resp = self
            .http
            .post(PRIMAL_API_URL)
            .json(&body)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| format!("Primal API request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("Primal API HTTP error: {}", resp.status()));
        }

        let events: Vec<PrimalEvent> = resp
            .json()
            .await
            .map_err(|e| format!("Primal API JSON parse error: {e}"))?;

        // Collect kind:0 profiles
        let mut hits: Vec<ProfileSearchHit> = Vec::new();
        for event in &events {
            if event.kind == 0 {
                let pubkey = match &event.pubkey {
                    Some(pk) => pk.clone(),
                    None => continue,
                };
                let meta: ProfileMetadata = match &event.content {
                    Some(content) => serde_json::from_str(content).unwrap_or(ProfileMetadata {
                        name: None,
                        display_name: None,
                        about: None,
                        picture: None,
                        nip05: None,
                        lud16: None,
                        website: None,
                    }),
                    None => continue,
                };
                hits.push(ProfileSearchHit {
                    pubkey,
                    name: meta.name,
                    display_name: meta.display_name,
                    about: meta.about,
                    picture: meta.picture,
                    nip05: meta.nip05,
                    lud16: meta.lud16,
                    website: meta.website,
                    followers_count: None,
                });
            }
        }

        // Collect kind:10000108 follower counts and merge
        for event in &events {
            if event.kind == 10000108 {
                if let Some(content) = &event.content {
                    if let Ok(counts) =
                        serde_json::from_str::<std::collections::HashMap<String, u64>>(content)
                    {
                        for hit in &mut hits {
                            if let Some(&count) = counts.get(&hit.pubkey) {
                                hit.followers_count = Some(count);
                            }
                        }
                    }
                }
            }
        }

        Ok(hits)
    }
}
