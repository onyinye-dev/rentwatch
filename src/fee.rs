//! Fee reconciliation for Soroban transactions.
//!
//! A Soroban transaction's fee has halves that the network reports
//! asymmetrically, and that asymmetry is the whole reason this crate exists.
//!
//! Before execution the submitter declares a ceiling. `tx.fee` is an inclusion
//! bid plus `sorobanData.resourceFee`, and that resource fee is itself a
//! non-refundable part (CPU instructions, ledger I/O, bandwidth) plus a
//! refundable part (ledger rent, events, return value). Both are ceilings:
//! simulation computes them from declared resources, so a tool that only
//! simulates can only ever show you the ceiling.
//!
//! After execution the result carries `feeCharged` -- one scalar. The refund is
//! the difference between what was declared and what was taken, and nothing in
//! the result spells it out. Recovering it is arithmetic, but it needs both
//! sides of the transaction, which is why it cannot be done pre-flight.

use std::fmt;

/// Stroops in one XLM.
pub const STROOPS_PER_XLM: i64 = 10_000_000;

/// The network's minimum inclusion fee, per operation, in stroops.
///
/// A Soroban transaction carries exactly one operation unless it is fee-bumped,
/// so absent surge pricing this is the whole inclusion fee.
pub const BASE_FEE: i64 = 100;

/// Render stroops as XLM with the conventional 7 decimal places.
pub fn xlm(stroops: i64) -> String {
    format!("{:.7}", stroops as f64 / STROOPS_PER_XLM as f64)
}

/// What the submitter declared, read from the transaction envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Declared {
    /// `tx.fee` -- the ceiling the submitter bid.
    pub tx_fee: i64,
    /// `sorobanData.resourceFee` -- the declared resource fee, also a ceiling.
    pub resource_fee: i64,
}

/// Everything that cannot be derived from the envelope and result alone.
///
/// `non_refundable` has to come from somewhere else: a simulation of the same
/// invocation, or the network's fee configuration applied to the declared
/// resources. Without it the refund still reconciles exactly; only the split of
/// the charged resource fee stays unknown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Context {
    /// Inclusion fee actually paid. Defaults to [`BASE_FEE`].
    pub inclusion_fee: Option<i64>,
    /// Non-refundable resource fee, if known.
    pub non_refundable: Option<i64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FeeError {
    /// `feeCharged` came in below the inclusion fee, which cannot happen.
    ChargedBelowInclusion {
        fee_charged: i64,
        inclusion_fee: i64,
    },
    /// More resource fee was charged than declared, which the protocol forbids.
    ChargedAboveDeclared { declared: i64, charged: i64 },
    /// The non-refundable part exceeds the resource fee actually charged.
    NonRefundableTooLarge { non_refundable: i64, charged: i64 },
    /// A fee field was negative.
    Negative(&'static str),
}

impl fmt::Display for FeeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChargedBelowInclusion {
                fee_charged,
                inclusion_fee,
            } => write!(
                f,
                "feeCharged ({fee_charged}) is below the inclusion fee ({inclusion_fee}); \
                 pass the real inclusion fee with --inclusion-fee"
            ),
            Self::ChargedAboveDeclared { declared, charged } => write!(
                f,
                "resource fee charged ({charged}) exceeds the declared ceiling ({declared}); \
                 the envelope and result may be from different transactions"
            ),
            Self::NonRefundableTooLarge {
                non_refundable,
                charged,
            } => write!(
                f,
                "non-refundable fee ({non_refundable}) exceeds the resource fee charged ({charged})"
            ),
            Self::Negative(which) => write!(f, "{which} is negative"),
        }
    }
}

impl std::error::Error for FeeError {}

/// A transaction's fee, decomposed as far as the available evidence allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reconciliation {
    pub tx_fee_bid: i64,
    pub declared_resource_fee: i64,
    pub inclusion_fee: i64,
    pub fee_charged: i64,
    pub resource_fee_charged: i64,
    pub refunded: i64,
    pub non_refundable: Option<i64>,
    pub refundable_declared: Option<i64>,
    pub refundable_charged: Option<i64>,
}

/// Reconcile a declared fee against what was charged.
pub fn reconcile(
    declared: Declared,
    fee_charged: i64,
    ctx: Context,
) -> Result<Reconciliation, FeeError> {
    if declared.tx_fee < 0 {
        return Err(FeeError::Negative("tx.fee"));
    }
    if declared.resource_fee < 0 {
        return Err(FeeError::Negative("sorobanData.resourceFee"));
    }
    if fee_charged < 0 {
        return Err(FeeError::Negative("feeCharged"));
    }

    let inclusion_fee = ctx.inclusion_fee.unwrap_or(BASE_FEE);
    if inclusion_fee < 0 {
        return Err(FeeError::Negative("inclusion fee"));
    }
    if fee_charged < inclusion_fee {
        return Err(FeeError::ChargedBelowInclusion {
            fee_charged,
            inclusion_fee,
        });
    }

    let resource_fee_charged = fee_charged - inclusion_fee;
    if resource_fee_charged > declared.resource_fee {
        return Err(FeeError::ChargedAboveDeclared {
            declared: declared.resource_fee,
            charged: resource_fee_charged,
        });
    }
    let refunded = declared.resource_fee - resource_fee_charged;

    let (refundable_declared, refundable_charged) = match ctx.non_refundable {
        Some(nr) => {
            if nr < 0 {
                return Err(FeeError::Negative("non-refundable fee"));
            }
            if nr > resource_fee_charged {
                return Err(FeeError::NonRefundableTooLarge {
                    non_refundable: nr,
                    charged: resource_fee_charged,
                });
            }
            (
                Some(declared.resource_fee - nr),
                Some(resource_fee_charged - nr),
            )
        }
        None => (None, None),
    };

    Ok(Reconciliation {
        tx_fee_bid: declared.tx_fee,
        declared_resource_fee: declared.resource_fee,
        inclusion_fee,
        fee_charged,
        resource_fee_charged,
        refunded,
        non_refundable: ctx.non_refundable,
        refundable_declared,
        refundable_charged,
    })
}

impl Reconciliation {
    /// Fraction of the declared resource fee that came back, in `0.0..=1.0`.
    pub fn refund_ratio(&self) -> f64 {
        if self.declared_resource_fee == 0 {
            return 0.0;
        }
        self.refunded as f64 / self.declared_resource_fee as f64
    }

    /// How many times larger the declared resource fee was than the charged one.
    ///
    /// This is the number a pre-flight estimator cannot show you: the factor by
    /// which simulation overstated the bill.
    pub fn over_declaration(&self) -> Option<f64> {
        if self.resource_fee_charged == 0 {
            return None;
        }
        Some(self.declared_resource_fee as f64 / self.resource_fee_charged as f64)
    }

    /// Upper bound on what rent cost, in stroops.
    ///
    /// Returns `None` unless the non-refundable fee is known. It is an upper
    /// bound and not a measurement, because the refundable fee also pays for
    /// contract events and the return value. Those are usually small next to
    /// rent, but this crate does not guess at the split.
    pub fn rent_upper_bound(&self) -> Option<i64> {
        self.refundable_charged
    }

    /// Share of the total charged fee that the refundable half accounts for.
    pub fn refundable_share(&self) -> Option<f64> {
        let r = self.refundable_charged?;
        if self.fee_charged == 0 {
            return None;
        }
        Some(r as f64 / self.fee_charged as f64)
    }
}

/// How wrong a prediction was about a fee that has since been charged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EstimateError {
    pub predicted: i64,
    pub actual: i64,
}

impl EstimateError {
    pub fn new(predicted: i64, actual: i64) -> Self {
        Self { predicted, actual }
    }

    /// `predicted / actual`. Above 1.0 means the prediction was too high.
    pub fn ratio(&self) -> Option<f64> {
        if self.actual == 0 {
            return None;
        }
        Some(self.predicted as f64 / self.actual as f64)
    }

    pub fn delta(&self) -> i64 {
        self.predicted - self.actual
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first-ever `deposit` against the Upstream contract on testnet,
    /// 23 October 2026, as published in that project's testnet walkthrough:
    ///
    /// ```text
    /// Inclusion fee:       100 stroops
    /// Non-refundable:    8,290 stroops
    /// Refundable:  482,827,645 stroops
    /// Charged:     419,845,920 stroops of refundable, 62,981,725 refunded
    /// Fee charged: 419,854,310 stroops = 41.9854 XLM
    /// ```
    ///
    /// Those numbers were measured independently of this code, so they pin the
    /// arithmetic rather than merely restating it.
    fn upstream_first_deposit() -> (Declared, i64, Context) {
        let non_refundable = 8_290;
        let refundable_declared = 482_827_645;
        (
            Declared {
                tx_fee: non_refundable + refundable_declared + BASE_FEE,
                resource_fee: non_refundable + refundable_declared,
            },
            419_854_310,
            Context {
                inclusion_fee: Some(100),
                non_refundable: Some(non_refundable),
            },
        )
    }

    #[test]
    fn reproduces_the_published_upstream_decomposition() {
        let (declared, charged, ctx) = upstream_first_deposit();
        let r = reconcile(declared, charged, ctx).unwrap();

        assert_eq!(r.inclusion_fee, 100);
        assert_eq!(r.non_refundable, Some(8_290));
        assert_eq!(r.refundable_declared, Some(482_827_645));
        assert_eq!(r.refundable_charged, Some(419_845_920));
        assert_eq!(r.refunded, 62_981_725);
        assert_eq!(r.fee_charged, 419_854_310);
        assert_eq!(r.resource_fee_charged, 419_854_210);
    }

    #[test]
    fn the_refundable_half_dominates_a_first_call() {
        let (declared, charged, ctx) = upstream_first_deposit();
        let r = reconcile(declared, charged, ctx).unwrap();
        // Published as "over 99.99% is storage rent, not computation".
        assert!(r.refundable_share().unwrap() > 0.9999);
        assert_eq!(r.rent_upper_bound(), Some(419_845_920));
    }

    #[test]
    fn an_eighth_of_the_declared_resource_fee_came_back() {
        let (declared, charged, ctx) = upstream_first_deposit();
        let r = reconcile(declared, charged, ctx).unwrap();
        let ratio = r.refund_ratio();
        assert!((0.13..0.131).contains(&ratio), "refund ratio was {ratio}");
    }

    #[test]
    fn reconciles_without_knowing_the_non_refundable_split() {
        let (declared, charged, _) = upstream_first_deposit();
        let r = reconcile(
            declared,
            charged,
            Context {
                inclusion_fee: Some(100),
                non_refundable: None,
            },
        )
        .unwrap();
        // The refund is exact either way; only the split goes missing.
        assert_eq!(r.refunded, 62_981_725);
        assert_eq!(r.refundable_charged, None);
        assert_eq!(r.rent_upper_bound(), None);
        assert_eq!(r.refundable_share(), None);
    }

    #[test]
    fn inclusion_fee_defaults_to_the_network_base_fee() {
        let r = reconcile(
            Declared {
                tx_fee: 1_100,
                resource_fee: 1_000,
            },
            600,
            Context::default(),
        )
        .unwrap();
        assert_eq!(r.inclusion_fee, BASE_FEE);
        assert_eq!(r.resource_fee_charged, 500);
        assert_eq!(r.refunded, 500);
    }

    #[test]
    fn a_fully_consumed_resource_fee_refunds_nothing() {
        let r = reconcile(
            Declared {
                tx_fee: 1_100,
                resource_fee: 1_000,
            },
            1_100,
            Context::default(),
        )
        .unwrap();
        assert_eq!(r.refunded, 0);
        assert_eq!(r.refund_ratio(), 0.0);
        assert_eq!(r.over_declaration(), Some(1.0));
    }

    #[test]
    fn rejects_a_charge_above_the_declared_ceiling() {
        let err = reconcile(
            Declared {
                tx_fee: 1_100,
                resource_fee: 1_000,
            },
            2_000,
            Context::default(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            FeeError::ChargedAboveDeclared {
                declared: 1_000,
                charged: 1_900,
            }
        );
    }

    #[test]
    fn rejects_a_charge_below_the_inclusion_fee() {
        let err = reconcile(
            Declared {
                tx_fee: 1_100,
                resource_fee: 1_000,
            },
            50,
            Context::default(),
        )
        .unwrap_err();
        assert!(matches!(err, FeeError::ChargedBelowInclusion { .. }));
    }

    #[test]
    fn rejects_a_non_refundable_part_larger_than_the_charge() {
        let err = reconcile(
            Declared {
                tx_fee: 1_100,
                resource_fee: 1_000,
            },
            600,
            Context {
                inclusion_fee: Some(100),
                non_refundable: Some(900),
            },
        )
        .unwrap_err();
        assert_eq!(
            err,
            FeeError::NonRefundableTooLarge {
                non_refundable: 900,
                charged: 500,
            }
        );
    }

    /// The SDK's own estimator prices rent at 12,000 stroops/KB while the live
    /// network charged the 1,000 stroops/KB floor -- a 12x overstatement the
    /// Upstream project found by comparing the two.
    #[test]
    fn quantifies_a_twelve_fold_overprediction() {
        let e = EstimateError::new(12_000, 1_000);
        assert_eq!(e.ratio(), Some(12.0));
        assert_eq!(e.delta(), 11_000);
    }

    #[test]
    fn estimate_error_declines_to_divide_by_zero() {
        assert_eq!(EstimateError::new(10, 0).ratio(), None);
    }

    #[test]
    fn formats_stroops_as_xlm() {
        assert_eq!(xlm(419_854_310), "41.9854310");
        assert_eq!(xlm(100), "0.0000100");
        assert_eq!(xlm(0), "0.0000000");
    }
}
