//! Pulling the three fee numbers out of base64 XDR.
//!
//! Two come from the transaction envelope (`tx.fee` and
//! `sorobanData.resourceFee`) and one from the result (`feeCharged`). Stellar
//! RPC hands all of them over as base64 strings.

use stellar_xdr::{
    FeeBumpTransactionInnerTx, Limits, ReadXdr, TransactionEnvelope, TransactionExt,
    TransactionResult, TransactionResultResult,
};

use crate::fee::Declared;

#[derive(Debug)]
pub enum XdrError {
    Decode(stellar_xdr::Error),
    /// A pre-Soroban (v0) envelope carries no resource fee.
    NotSoroban,
}

impl std::fmt::Display for XdrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode(e) => write!(f, "could not decode XDR: {e}"),
            Self::NotSoroban => write!(
                f,
                "this transaction has no Soroban resource fee, so there is nothing to reconcile"
            ),
        }
    }
}

impl std::error::Error for XdrError {}

impl From<stellar_xdr::Error> for XdrError {
    fn from(e: stellar_xdr::Error) -> Self {
        Self::Decode(e)
    }
}

/// Read the declared fee ceiling out of a base64 transaction envelope.
///
/// For a fee-bump envelope the outer `fee` is what the network may take, while
/// the Soroban resource fee lives on the inner transaction; both are reported.
pub fn declared_from_envelope(b64: &str) -> Result<Declared, XdrError> {
    let envelope = TransactionEnvelope::from_xdr_base64(b64, Limits::none())?;

    let (tx_fee, ext) = match envelope {
        TransactionEnvelope::Tx(e) => (i64::from(e.tx.fee), e.tx.ext),
        TransactionEnvelope::TxV0(_) => return Err(XdrError::NotSoroban),
        TransactionEnvelope::TxFeeBump(e) => {
            let outer = e.tx.fee;
            let FeeBumpTransactionInnerTx::Tx(inner) = e.tx.inner_tx;
            (outer, inner.tx.ext)
        }
    };

    let resource_fee = match ext {
        TransactionExt::V1(data) => data.resource_fee,
        TransactionExt::V0 => return Err(XdrError::NotSoroban),
    };

    Ok(Declared {
        tx_fee,
        resource_fee,
    })
}

/// What the network charged, and whether the transaction succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub fee_charged: i64,
    pub succeeded: bool,
}

/// Read `feeCharged` out of a base64 transaction result.
///
/// A failed Soroban transaction is still charged its non-refundable fee, so a
/// failure is reported rather than rejected -- those are often the most
/// interesting measurements.
pub fn outcome_from_result(b64: &str) -> Result<Outcome, XdrError> {
    let result = TransactionResult::from_xdr_base64(b64, Limits::none())?;
    Ok(Outcome {
        fee_charged: result.fee_charged,
        succeeded: matches!(
            result.result,
            TransactionResultResult::TxSuccess(_)
                | TransactionResultResult::TxFeeBumpInnerSuccess(_)
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_garbage_envelope() {
        assert!(matches!(
            declared_from_envelope("not base64 at all!!"),
            Err(XdrError::Decode(_))
        ));
    }

    #[test]
    fn rejects_garbage_result() {
        assert!(matches!(
            outcome_from_result("////"),
            Err(XdrError::Decode(_))
        ));
    }

    #[test]
    fn empty_input_is_a_decode_error_not_a_panic() {
        assert!(declared_from_envelope("").is_err());
        assert!(outcome_from_result("").is_err());
    }
}
