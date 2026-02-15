use std::collections::HashMap;
use tokio::sync::RwLock;

use nostr_sdk::prelude::*;
use nwc::NWC;

struct PendingInvoice {
    #[allow(dead_code)]
    tool_name: String,
    #[allow(dead_code)]
    amount_sats: u64,
    #[allow(dead_code)]
    expires_at: i64,
}

#[allow(dead_code)]
pub struct InvoiceResponse {
    pub invoice: String,
    pub payment_hash: String,
    pub amount_sats: u64,
    pub expires_at: Option<i64>,
}

pub struct NwcGateway {
    nwc: NWC,
    pending_invoices: RwLock<HashMap<String, PendingInvoice>>,
}

impl NwcGateway {
    pub fn new(nwc_url: &str) -> anyhow::Result<Self> {
        let uri: NostrWalletConnectURI =
            nwc_url
                .parse()
                .map_err(|e: nostr_sdk::prelude::nip47::Error| {
                    anyhow::anyhow!("Failed to parse NWC URI: {e}")
                })?;
        let nwc = NWC::new(uri);

        Ok(Self {
            nwc,
            pending_invoices: RwLock::new(HashMap::new()),
        })
    }

    pub async fn create_invoice(
        &self,
        tool_name: &str,
        amount_sats: u64,
        description: &str,
        expiry_secs: u64,
    ) -> anyhow::Result<InvoiceResponse> {
        let request = MakeInvoiceRequest {
            amount: amount_sats * 1000, // convert to msats
            description: Some(description.to_string()),
            description_hash: None,
            expiry: Some(expiry_secs),
        };

        let response = self
            .nwc
            .make_invoice(request)
            .await
            .map_err(|e| anyhow::anyhow!("NWC make_invoice failed: {e}"))?;

        let payment_hash = response
            .payment_hash
            .ok_or_else(|| anyhow::anyhow!("No payment_hash in make_invoice response"))?;

        let expires_at = response.expires_at.map(|t| t.as_secs() as i64);

        // Track pending invoice
        {
            let mut pending = self.pending_invoices.write().await;
            pending.insert(
                payment_hash.clone(),
                PendingInvoice {
                    tool_name: tool_name.to_string(),
                    amount_sats,
                    expires_at: expires_at.unwrap_or(0),
                },
            );
        }

        Ok(InvoiceResponse {
            invoice: response.invoice,
            payment_hash,
            amount_sats,
            expires_at,
        })
    }

    pub async fn verify_payment(&self, payment_hash: &str) -> anyhow::Result<bool> {
        let request = LookupInvoiceRequest {
            payment_hash: Some(payment_hash.to_string()),
            invoice: None,
        };

        let response = self
            .nwc
            .lookup_invoice(request)
            .await
            .map_err(|e| anyhow::anyhow!("NWC lookup_invoice failed: {e}"))?;

        let settled = response.settled_at.is_some();

        if settled {
            let mut pending = self.pending_invoices.write().await;
            pending.remove(payment_hash);
        }

        Ok(settled)
    }
}
