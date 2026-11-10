//! Fetching an executed transaction from Stellar RPC.
//!
//! Only `getTransaction` is needed. It returns the envelope and result as
//! base64 XDR, which is everything [`crate::xdr`] needs.
//!
//! Note the retention window: RPC nodes keep transaction history for a limited
//! period (commonly around a day on public endpoints), so a hash older than
//! that will come back `NOT_FOUND` even though it is permanently in the ledger.
//! Recovering those needs a history archive, which this version does not do.

use serde::Deserialize;

/// Public RPC endpoint for a named network.
pub fn default_endpoint(network: &str) -> Option<&'static str> {
    match network {
        "testnet" => Some("https://soroban-testnet.stellar.org"),
        "mainnet" | "public" => Some("https://mainnet.sorobanrpc.com"),
        _ => None,
    }
}

#[derive(Debug)]
pub enum RpcError {
    Transport(String),
    /// The node answered, but with a JSON-RPC error object.
    Rpc {
        code: i64,
        message: String,
    },
    /// The node has no record of this hash within its retention window.
    NotFound {
        status: String,
    },
    /// The transaction exists but the node withheld the XDR we need.
    MissingField(&'static str),
    Decode(String),
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "could not reach the RPC endpoint: {e}"),
            Self::Rpc { code, message } => write!(f, "RPC error {code}: {message}"),
            Self::NotFound { status } => write!(
                f,
                "the endpoint reports this transaction as {status}; RPC nodes retain only recent \
                 history, so an older hash needs a history archive"
            ),
            Self::MissingField(name) => {
                write!(f, "the RPC response had no {name} field")
            }
            Self::Decode(e) => write!(f, "could not parse the RPC response: {e}"),
        }
    }
}

impl std::error::Error for RpcError {}

/// The parts of a `getTransaction` response this tool uses.
#[derive(Debug, Clone)]
pub struct FetchedTransaction {
    pub status: String,
    pub envelope_xdr: String,
    pub result_xdr: String,
    pub ledger: Option<u32>,
    /// Unix seconds, as the node reported them.
    pub created_at: Option<i64>,
}

#[derive(Deserialize)]
struct Envelope {
    result: Option<GetTransaction>,
    error: Option<RpcErrorBody>,
}

#[derive(Deserialize)]
struct RpcErrorBody {
    code: i64,
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetTransaction {
    status: String,
    envelope_xdr: Option<String>,
    result_xdr: Option<String>,
    ledger: Option<u32>,
    // RPC sends this as a string of unix seconds.
    created_at: Option<String>,
}

/// Call `getTransaction` for one hash.
pub fn get_transaction(endpoint: &str, tx_hash: &str) -> Result<FetchedTransaction, RpcError> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTransaction",
        "params": { "hash": tx_hash },
    });

    let mut response = ureq::post(endpoint)
        .send_json(&body)
        .map_err(|e| RpcError::Transport(e.to_string()))?;

    let parsed: Envelope = response
        .body_mut()
        .read_json()
        .map_err(|e| RpcError::Decode(e.to_string()))?;

    if let Some(err) = parsed.error {
        return Err(RpcError::Rpc {
            code: err.code,
            message: err.message,
        });
    }

    let result = parsed.result.ok_or(RpcError::MissingField("result"))?;

    if result.status != "SUCCESS" && result.status != "FAILED" {
        return Err(RpcError::NotFound {
            status: result.status,
        });
    }

    Ok(FetchedTransaction {
        envelope_xdr: result
            .envelope_xdr
            .ok_or(RpcError::MissingField("envelopeXdr"))?,
        result_xdr: result
            .result_xdr
            .ok_or(RpcError::MissingField("resultXdr"))?,
        ledger: result.ledger,
        created_at: result.created_at.and_then(|s| s.parse().ok()),
        status: result.status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_the_public_endpoints() {
        assert!(default_endpoint("testnet").is_some());
        assert!(default_endpoint("mainnet").is_some());
        assert_eq!(default_endpoint("mainnet"), default_endpoint("public"));
        assert_eq!(default_endpoint("futurenet"), None);
    }

    #[test]
    fn not_found_explains_the_retention_window() {
        let msg = RpcError::NotFound {
            status: "NOT_FOUND".into(),
        }
        .to_string();
        assert!(msg.contains("history archive"), "{msg}");
    }
}
