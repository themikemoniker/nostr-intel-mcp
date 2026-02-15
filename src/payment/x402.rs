use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct X402PaymentDetails {
    pub payment_address: String,
    pub amount_usdc: String,
    pub chain_id: u64,
    pub token_address: String,
    pub network: String,
}

/// Create x402 payment details for a given amount in cents.
pub fn create_payment_details(amount_cents: u64, address: &str) -> X402PaymentDetails {
    X402PaymentDetails {
        payment_address: address.to_string(),
        amount_usdc: format!("{}.{:02}", amount_cents / 100, amount_cents % 100),
        chain_id: 8453, // Base mainnet
        token_address: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(), // USDC on Base
        network: "base".to_string(),
    }
}

/// Create HTTP headers for an x402 payment-required response.
pub fn create_x402_headers(details: &X402PaymentDetails) -> Vec<(String, String)> {
    let json = serde_json::to_string(details).unwrap_or_default();
    vec![
        ("X-Payment-Required".to_string(), "true".to_string()),
        ("X-Payment-Protocol".to_string(), "x402".to_string()),
        ("X-Payment-Details".to_string(), json),
    ]
}

/// Verify an x402 payment transaction. Stub — always returns false.
pub fn verify_payment(_tx_hash: &str) -> bool {
    tracing::warn!("x402 payment verification is not yet implemented (stub)");
    false
}
