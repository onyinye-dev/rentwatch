# Contributing

Thanks for looking. This project is small and the gaps are deliberate, so there
is a lot of well-bounded work available.

## Before you start

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

All three run in CI and all three must pass. Rust 1.94 or later;
`rust-toolchain.toml` pins it, so `rustup` will fetch the right one.

Nothing in the test suite touches the network. If you add a test that needs
RPC, mark it `#[ignore]` and say so in the pull request.

## What makes a good change here

**Claims must be checkable.** This tool exists because fee estimates are
reported with more confidence than they deserve, so it holds itself to the same
standard. If a number is a bound, call it a bound. The rent figure says `<=`
because the refundable fee also covers events and the return value, and we do
not know the split — that kind of hedging is a feature, not timidity.

**Measurements need provenance.** A new row in `data/measurements.jsonl` must
carry a real transaction hash and a `provenance` of either `rpc` (this tool
decoded it from a node) or `transcribed` (copied from somewhere, with a `note`
saying where). Never invent a number to fill a column.

**Pure arithmetic belongs in `src/fee.rs`.** That module has no dependencies
and no I/O, which is why it can be tested against independently measured
figures. Keep it that way; network and parsing concerns go in `src/rpc.rs` and
`src/xdr.rs`.

## Where the work is

Roughly in order of usefulness:

- **Derive the non-refundable fee** from the network's fee configuration
  (`getLedgerEntries` on the config entries) applied to the declared resources
  in `SorobanTransactionData`. This is the single biggest gap: it would make
  `--non-refundable` unnecessary and turn the rent bound into something the tool
  computes rather than something you supply.
- **History archive support**, so hashes older than the RPC retention window can
  be reconciled instead of returning `NOT_FOUND`.
- **Separate events and return value from rent**, which would turn the rent
  upper bound into a measurement.
- **Batch mode** — reconcile every Soroban transaction in a ledger range, which
  is what turns this from a one-shot tool into a dataset generator.
- **Backfill the dataset.** The September 2026 figures in the Upstream project's
  walkthrough have no recorded declared resource fee, so they cannot be
  reconciled yet; recovering them from an archive would add a third point to the
  protocol-version series.
- **A GitHub Action** that fails a pull request when a contract's measured fee
  moves by more than a threshold.
- **Surge pricing**: detect the real inclusion fee rather than assuming 100
  stroops.

## Commits and pull requests

Conventional commits (`feat:`, `fix:`, `docs:`, `test:`, `chore:`). One logical
change per pull request, with a note on how you verified it — if you reconciled
a real transaction, include the hash.

## Code of conduct

Be straightforward and courteous. Disagree about the engineering, not about the
person.
