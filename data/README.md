# The measurement record

One JSON object per line in [`measurements.jsonl`](measurements.jsonl), appended
in the order they were taken. It lives in the repository rather than a database
so that a change in network fee behaviour shows up as a diff, and so anyone can
re-derive a row from its transaction hash.

## Fields

| field | meaning |
|---|---|
| `label` | Groups runs of the same call, e.g. `deposit (first ever)`. `rentwatch report` groups on this. |
| `network` | `testnet` or `mainnet`. Rent rates differ sharply between them. |
| `tx_hash` | The transaction, so the row can be re-derived or disputed. |
| `provenance` | `rpc` if this tool decoded it from a node, `transcribed` if copied from elsewhere. |
| `ledger` | Ledger sequence, when known. |
| `observed_on` | ISO-8601 date, derived from the ledger close time. |
| `sdk` | `soroban-sdk` version the contract was built with. |
| `tx_fee_bid` | `tx.fee` — the ceiling the submitter bid. |
| `declared_resource_fee` | `sorobanData.resourceFee` — what simulation declared. |
| `inclusion_fee` | Assumed to be the 100 stroop minimum unless recorded otherwise. |
| `fee_charged` | `feeCharged` from the transaction result. |
| `resource_fee_charged` | `fee_charged − inclusion_fee`. |
| `refunded` | `declared_resource_fee − resource_fee_charged`. |
| `non_refundable` | CPU, I/O and bandwidth, when known. Optional. |
| `refundable_charged` | Rent plus events plus return value, when `non_refundable` is known. Optional. |
| `note` | Free text. Required on `transcribed` rows to say where the figures came from. |

All amounts are stroops. 1 XLM = 10,000,000 stroops.

## What is in here so far

Two measurements, both of the same `deposit` entry point on the same testnet
contract, 18 days apart. The fees differ by a factor of 36 — the October row is
a first-ever call that paid to move cold entries into storage — yet both
over-declared the resource fee by about an eighth.

That is the observation worth extending, and it is currently resting on two
points, which is not enough to call it a rule. Rows from other contracts, other
entry points and mainnet would all help.

## Adding a row

Prefer letting the tool do it, which guarantees the arithmetic and the
provenance:

```sh
rentwatch record <TX_HASH> --label "collect (first)" --sdk 28.0.0
```

A hand-written row is acceptable when the transaction has aged out of RPC
retention, but it must carry `"provenance":"transcribed"` and a `note` naming
the source. Do not invent a value to fill a column — leave the optional field
out instead.

## Known gaps

- The September 2026 figures published in the Upstream project's testnet
  walkthrough record a total fee and a non-refundable part, but not the declared
  resource fee, so they cannot be reconciled and are deliberately absent.
- No mainnet rows yet. Mainnet rent sits at the floor rate while testnet's has
  drifted with state growth, so the two are not comparable.
