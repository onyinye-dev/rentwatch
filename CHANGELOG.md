# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `rentwatch inspect` — decompose an executed Soroban transaction's fee into the
  declared ceiling, the inclusion fee, what was charged and what was refunded,
  with an optional split into computation and rent when the non-refundable fee
  is supplied.
- `rentwatch reconcile` — report how far a prediction was from the charge, as a
  stroop difference and a ratio.
- `rentwatch record` and `rentwatch report` — append measurements to an
  append-only JSON Lines record and read back how a call's fee has moved over
  time. Recording is idempotent on `(tx_hash, label)`.
- `src/fee.rs`, the reconciliation arithmetic, with no dependencies and no I/O.
  Its tests pin the maths against figures measured independently of this code:
  the published decomposition of a 23 October 2026 testnet deposit, and the
  12x gap between the SDK's rent estimate (12,000 stroops/KB) and the live
  network's floor rate (1,000 stroops/KB).
- `data/measurements.jsonl` with the first two records. A `deposit` on
  2026-11-10 (`a1424067…`, charged 1.1585332 XLM) refunded 13.07% of its
  declared resource fee; a first-ever `deposit` on 2026-10-23
  (`69df2ef6…`, charged 41.9854310 XLM) refunded 13.04%. Fees 36x apart,
  over-declaration within 0.03 points of each other.
- CI: formatting, clippy with warnings denied, tests, and a check that every
  dataset line parses as JSON.

### Known limitations

- The non-refundable fee is not derived; it must be passed with
  `--non-refundable` or the rent bound is unavailable.
- Rent is an upper bound, since the refundable fee also covers events and the
  return value.
- Transactions older than the RPC node's retention window cannot be reached;
  history archive support is not implemented.
- The inclusion fee is assumed to be the 100 stroop network minimum unless
  `--inclusion-fee` is given.
