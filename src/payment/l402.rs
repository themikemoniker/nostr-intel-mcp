use base64::prelude::*;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, thiserror::Error)]
pub enum L402Error {
    #[error("Invalid secret: must be at least 32 bytes hex-encoded")]
    InvalidSecret,
    #[error("Invalid token: {0}")]
    InvalidToken(String),
    #[error("Token expired")]
    Expired,
    #[error("Signature verification failed")]
    BadSignature,
    #[error("Invalid preimage")]
    BadPreimage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L402TokenData {
    pub payment_hash: String,
    pub caveats: L402Caveats,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L402Caveats {
    pub tool: String,
    pub expires: u64,
}

pub struct L402Manager {
    secret: Vec<u8>,
}

impl L402Manager {
    /// Create a new L402Manager from a hex-encoded secret (min 32 bytes).
    pub fn new(secret_hex: &str) -> Result<Self, L402Error> {
        let secret = hex::decode(secret_hex).map_err(|_| L402Error::InvalidSecret)?;
        if secret.len() < 32 {
            return Err(L402Error::InvalidSecret);
        }
        Ok(Self { secret })
    }

    /// Create a signed L402 token for a given payment.
    pub fn create_token(
        &self,
        payment_hash: &str,
        tool: &str,
        expires: u64,
    ) -> String {
        let caveats = L402Caveats {
            tool: tool.to_string(),
            expires,
        };

        let signature = self.sign(payment_hash, &caveats);

        let token = L402TokenData {
            payment_hash: payment_hash.to_string(),
            caveats,
            signature,
        };

        let json = serde_json::to_string(&token).expect("L402TokenData serialization cannot fail");
        BASE64_STANDARD.encode(json.as_bytes())
    }

    /// Verify a base64-encoded L402 token. Returns the token data if valid.
    pub fn verify_token(&self, token_base64: &str) -> Result<L402TokenData, L402Error> {
        let json_bytes = BASE64_STANDARD
            .decode(token_base64)
            .map_err(|_| L402Error::InvalidToken("invalid base64".into()))?;

        let token: L402TokenData = serde_json::from_slice(&json_bytes)
            .map_err(|_| L402Error::InvalidToken("invalid JSON".into()))?;

        // Check expiry
        let now = chrono::Utc::now().timestamp() as u64;
        if token.caveats.expires > 0 && now > token.caveats.expires {
            return Err(L402Error::Expired);
        }

        // Verify HMAC signature
        let expected = self.sign(&token.payment_hash, &token.caveats);
        if token.signature != expected {
            return Err(L402Error::BadSignature);
        }

        Ok(token)
    }

    /// Verify that a preimage hashes to the given payment_hash (both hex-encoded).
    pub fn verify_preimage(payment_hash_hex: &str, preimage_hex: &str) -> bool {
        let Ok(preimage) = hex::decode(preimage_hex) else {
            return false;
        };
        let Ok(expected_hash) = hex::decode(payment_hash_hex) else {
            return false;
        };

        use sha2::Digest;
        let computed = Sha256::digest(&preimage);
        computed.as_slice() == expected_hash.as_slice()
    }

    /// Create a WWW-Authenticate header value for an L402 challenge.
    pub fn create_challenge(
        &self,
        invoice: &str,
        payment_hash: &str,
        tool: &str,
        expires: u64,
    ) -> String {
        let token = self.create_token(payment_hash, tool, expires);
        format!(
            "L402 invoice=\"{invoice}\", token=\"{token}\""
        )
    }

    /// Parse an Authorization header: "L402 <token>:<preimage>"
    pub fn parse_authorization(header: &str) -> Result<(String, String), L402Error> {
        let rest = header
            .strip_prefix("L402 ")
            .ok_or_else(|| L402Error::InvalidToken("missing L402 prefix".into()))?;

        let (token, preimage) = rest
            .split_once(':')
            .ok_or_else(|| L402Error::InvalidToken("missing colon separator".into()))?;

        Ok((token.to_string(), preimage.to_string()))
    }

    fn sign(&self, payment_hash: &str, caveats: &L402Caveats) -> String {
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC can take key of any size");
        mac.update(payment_hash.as_bytes());
        mac.update(caveats.tool.as_bytes());
        mac.update(caveats.expires.to_be_bytes().as_ref());
        hex::encode(mac.finalize().into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_secret() -> String {
        hex::encode([0xab_u8; 32])
    }

    #[test]
    fn test_create_and_verify_token() {
        let mgr = L402Manager::new(&test_secret()).unwrap();
        let token = mgr.create_token("abc123", "search_events", u64::MAX);
        let data = mgr.verify_token(&token).unwrap();
        assert_eq!(data.payment_hash, "abc123");
        assert_eq!(data.caveats.tool, "search_events");
    }

    #[test]
    fn test_expired_token() {
        let mgr = L402Manager::new(&test_secret()).unwrap();
        // expires = 1 (long in the past)
        let token = mgr.create_token("abc123", "search_events", 1);
        let err = mgr.verify_token(&token).unwrap_err();
        assert!(matches!(err, L402Error::Expired));
    }

    #[test]
    fn test_tampered_token() {
        let mgr = L402Manager::new(&test_secret()).unwrap();
        let token_b64 = mgr.create_token("abc123", "search_events", u64::MAX);

        // Decode, tamper, re-encode
        let json_bytes = BASE64_STANDARD.decode(&token_b64).unwrap();
        let mut token: L402TokenData = serde_json::from_slice(&json_bytes).unwrap();
        token.caveats.tool = "free_tool".to_string(); // tamper
        let tampered_json = serde_json::to_string(&token).unwrap();
        let tampered_b64 = BASE64_STANDARD.encode(tampered_json.as_bytes());

        let err = mgr.verify_token(&tampered_b64).unwrap_err();
        assert!(matches!(err, L402Error::BadSignature));
    }

    #[test]
    fn test_verify_preimage() {
        use sha2::Digest;
        let preimage = [0x01_u8; 32];
        let hash = Sha256::digest(&preimage);
        let preimage_hex = hex::encode(preimage);
        let hash_hex = hex::encode(hash);

        assert!(L402Manager::verify_preimage(&hash_hex, &preimage_hex));
        assert!(!L402Manager::verify_preimage(&hash_hex, &hex::encode([0x02_u8; 32])));
    }

    #[test]
    fn test_parse_authorization() {
        let (token, preimage) =
            L402Manager::parse_authorization("L402 dG9rZW4=:abc123").unwrap();
        assert_eq!(token, "dG9rZW4=");
        assert_eq!(preimage, "abc123");

        assert!(L402Manager::parse_authorization("Bearer xyz").is_err());
        assert!(L402Manager::parse_authorization("L402 no_colon").is_err());
    }
}
