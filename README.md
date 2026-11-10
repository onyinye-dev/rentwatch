# rentwatch

[![CI](https://github.com/onyinye-dev/rentwatch/actions/workflows/ci.yml/badge.svg)](https://github.com/onyinye-dev/rentwatch/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Stellar](https://img.shields.io/badge/Stellar-Soroban-black)](https://stellar.org)

**What Soroban actually charged, against what it predicted.**

Every tool that quotes you a Soroban fee works the same way: simulate the
invocation, read back the declared resources, apply the network's rates. That
number is a **ceiling**. The refundable half of a resource fee — ledger rent,
events, the return value — is reconciled against real usage during execution,
and the unused part is returned. Simulation cannot know how much that will be,
because the refund does not exist until the transaction runs.

`rentwatch` works from the other end. Give it a transaction that has already
executed and it recovers what was really taken, what came back, and how far off
the prediction was.

## The number this exists to show you

A deposit into a live testnet contract, submitted 10 November 2026
([`a1424067…`](https://stellar.expert/explorer/testnet/tx/a14240679eb9d4b99849071cb5b66e51f9f4482db898f3ee3004a4f7bd730dcf)):

```
$ rentwatch inspect a14240679eb9d4b99849071cb5b66e51f9f4482db898f3ee3004a4f7bd730dcf

                               stroops             XLM
  tx.fee (bid)                13326518       1.3326518
  declared resource           13326418       1.3326418
  ----------------------------------------------------
  inclusion                        100       0.0000100
  fee charged                 11585332       1.1585332
  refunded                     1741186       0.1741186

  refund            13.07% of the declared resource fee
  over-declared     1.15x
```

Simulation declared 1.3326418 XLM. The network took 1.1585332 XLM. **Thirteen
percent of the declared resource fee was never spent** — and no pre-flight tool
reports that, because pre-flight is the wrong side of execution to see it from.

It is not a one-off. A first-ever deposit against the same contract on
23 October 2026 cost 41.9854310 XLM — 36 times more, because it was paying to
put cold entries into storage — and refunded **13.04%**:

| call | date | charged XLM | refund |
|---|---|---:|---:|
| `deposit` (steady state) | 2026-11-10 | 1.1585332 | 13.1% |
| `deposit` (first ever) | 2026-10-23 | 41.9854310 | 13.0% |

Two transactions, eighteen days apart, fees differing by a factor of 36, both
over-declaring by about an eighth. That stability is the interesting part, and
it is the kind of claim that needs a record rather than an anecdote — which is
what [`data/measurements.jsonl`](data/measurements.jsonl) is for.

## Install

```sh
cargo install --path .
```

Requires Rust 1.94 or later. No network access is needed to build.

## Use

```sh
# Decompose one executed transaction.
rentwatch inspect <TX_HASH> --network testnet

# Split the charge into computation and rent. The non-refundable figure comes
# from a simulation of the same call; without it the refund still reconciles
# exactly, but the rent bound cannot be computed.
rentwatch inspect <TX_HASH> --non-refundable 8290

# How wrong was an estimate, in stroops?
rentwatch reconcile <TX_HASH> --predicted 13326418

# Add it to the record, then read the record back.
rentwatch record <TX_HASH> --label "deposit (steady state)" --sdk 28.0.0
rentwatch report
```

`--rpc <URL>` overrides the endpoint. `--network mainnet` is supported.
`record` is idempotent: the same hash under the same label is never written
twice.

## How the arithmetic works

A Soroban transaction declares `tx.fee`, which is an inclusion bid plus
`sorobanData.resourceFee`. That resource fee is itself a non-refundable part
(CPU instructions, ledger I/O, bandwidth) and a refundable part (rent, events,
return value). The result carries one scalar, `feeCharged`. So:

```
resource_fee_charged = feeCharged − inclusion_fee
refunded             = declared_resource_fee − resource_fee_charged
```

Both inputs come from the chain: the envelope and the result, as
[`src/xdr.rs`](src/xdr.rs) decodes them. The arithmetic lives in
[`src/fee.rs`](src/fee.rs) and depends on nothing, so it is unit-tested against
figures measured independently of this code.

## What this does not do

Stated plainly, because the gaps are real and most of them are good first
issues.

- **It cannot split the charge by itself.** Separating computation from rent
  needs the non-refundable fee, which means applying the network's fee
  configuration to the declared resources. Today you pass `--non-refundable`
  by hand or go without.
- **Rent is reported as an upper bound, never a measurement.** The refundable
  fee also pays for events and the return value. Those are usually small beside
  rent, but this tool does not guess at the split, so it says `<=`.
- **It only reaches back as far as the RPC node does.** Public endpoints retain
  roughly a day of transaction history. An older hash returns `NOT_FOUND`, and
  reaching it needs a history archive, which is not implemented. The error says
  so rather than looking like a failure.
- **The inclusion fee is assumed to be the 100 stroop minimum.** The result does
  not break it out. Under surge pricing, pass `--inclusion-fee`.
- **One transaction at a time.** No batch mode, no watching an address.
- **Not published to crates.io yet**, and the dataset holds two measurements.

## Related work

Three Stellar projects estimate fees *before* submission, and they are the
right tool for budgeting a contract you have not deployed:
[`soroban-cost-estimator`](https://github.com/Stellar-Cost-Labs/soroban-cost-estimator)
wraps `simulateTransaction` to report resources and fees from a `.wasm`;
[`sorark`](https://github.com/sorark-labs/sorark) verifies TTL and ledger-entry
limits statically; [`soroban-state-sentinel`](https://github.com/stellar-archival-labs/soroban-state-sentinel)
watches deployed contracts for archival risk and emits remediation transactions.

`rentwatch` deliberately does not compete with any of them. It takes the
post-execution side, which none of them can reach, and the two halves are
complementary: an estimator tells you the ceiling, `rentwatch` tells you what
the ceiling was worth.

## Contributing

Issues and pull requests are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).
`cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo test` all run in
CI and all must pass.

## License

[MIT](LICENSE).
