//! rentwatch -- what Soroban actually charged, against what it predicted.
//!
//! Three tools in the Stellar ecosystem will estimate a contract call's fee for
//! you before you submit it. All of them work the same way: simulate the
//! invocation, read the declared resources back, apply the network's fee rates.
//! That number is a ceiling, and on a call that touches cold storage it can
//! overstate the bill by an order of magnitude.
//!
//! This crate works from the other end. Given a transaction that has already
//! executed, it recovers the fee the network really took, how much of the
//! declared resource fee came back as a refund, and -- when the non-refundable
//! split is known -- an upper bound on what ledger rent cost. None of that is
//! knowable before execution.
//!
//! [`fee`] holds the arithmetic and depends on nothing. [`xdr`] pulls the two
//! declared numbers out of a transaction envelope and the charged number out of
//! a result. [`rpc`] fetches those from a Stellar RPC endpoint. [`dataset`]
//! appends measurements to an append-only record so that fee changes across
//! protocol versions can be read off later.

pub mod dataset;
pub mod fee;
pub mod rpc;
pub mod xdr;
