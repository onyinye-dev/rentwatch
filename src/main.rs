//! The `rentwatch` command line.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use rentwatch::dataset::{self, Measurement, Provenance};
use rentwatch::fee::{self, xlm, Context, EstimateError};
use rentwatch::rpc;
use rentwatch::xdr;

const DEFAULT_DATASET: &str = "data/measurements.jsonl";

#[derive(Parser)]
#[command(
    name = "rentwatch",
    version,
    about = "What Soroban actually charged, against what it predicted.",
    long_about = "Pre-flight tools estimate a Soroban fee from declared resources, which is a \
                  ceiling. rentwatch works from an executed transaction instead: it recovers the \
                  fee the network really took, how much of the declared resource fee was refunded, \
                  and an upper bound on ledger rent."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Decompose one executed transaction's fee.
    Inspect {
        #[command(flatten)]
        tx: TxArgs,
        /// Emit JSON instead of a table.
        #[arg(long)]
        json: bool,
    },

    /// Compare a prediction against what was charged.
    Reconcile {
        #[command(flatten)]
        tx: TxArgs,
        /// The fee, in stroops, that an estimator predicted.
        #[arg(long, value_name = "STROOPS")]
        predicted: i64,
    },

    /// Append a transaction to the dataset.
    Record {
        #[command(flatten)]
        tx: TxArgs,
        /// Groups runs of the same call together, e.g. "deposit (first)".
        #[arg(long)]
        label: String,
        /// ISO-8601 date the measurement belongs to. Defaults to the ledger time.
        #[arg(long, value_name = "YYYY-MM-DD")]
        observed_on: Option<String>,
        /// soroban-sdk version the contract was built with.
        #[arg(long)]
        sdk: Option<String>,
        #[arg(long, default_value = DEFAULT_DATASET)]
        dataset: PathBuf,
    },

    /// Show how fees for the same call have moved over time.
    Report {
        #[arg(long, default_value = DEFAULT_DATASET)]
        dataset: PathBuf,
    },
}

#[derive(clap::Args)]
struct TxArgs {
    /// Transaction hash, 64 hex characters.
    hash: String,
    /// Named network. Ignored when --rpc is given.
    #[arg(long, default_value = "testnet")]
    network: String,
    /// RPC endpoint, overriding the network default.
    #[arg(long)]
    rpc: Option<String>,
    /// Non-refundable resource fee in stroops, if known from a simulation.
    ///
    /// Without it the refund still reconciles exactly, but the charged resource
    /// fee cannot be split into computation and rent.
    #[arg(long, value_name = "STROOPS")]
    non_refundable: Option<i64>,
    /// Inclusion fee actually paid. Defaults to the 100 stroop network minimum.
    #[arg(long, value_name = "STROOPS")]
    inclusion_fee: Option<i64>,
}

/// Everything one fetch-and-reconcile produces.
struct Observed {
    fetched: rpc::FetchedTransaction,
    reconciliation: fee::Reconciliation,
}

fn observe(tx: &TxArgs) -> Result<Observed, Box<dyn std::error::Error>> {
    if tx.hash.len() != 64 || !tx.hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "'{}' is not a transaction hash; expected 64 hex characters",
            tx.hash
        )
        .into());
    }

    let endpoint = match &tx.rpc {
        Some(url) => url.clone(),
        None => rpc::default_endpoint(&tx.network)
            .ok_or_else(|| {
                format!(
                    "no default endpoint for network '{}'; pass --rpc with a URL",
                    tx.network
                )
            })?
            .to_string(),
    };

    let fetched = rpc::get_transaction(&endpoint, &tx.hash)?;
    let declared = xdr::declared_from_envelope(&fetched.envelope_xdr)?;
    let outcome = xdr::outcome_from_result(&fetched.result_xdr)?;

    let reconciliation = fee::reconcile(
        declared,
        outcome.fee_charged,
        Context {
            inclusion_fee: tx.inclusion_fee,
            non_refundable: tx.non_refundable,
        },
    )?;

    Ok(Observed {
        fetched,
        reconciliation,
    })
}

fn print_table(o: &Observed, network: &str, hash: &str) {
    let r = &o.reconciliation;
    println!("{hash}");
    println!("  network           {network}");
    if let Some(l) = o.fetched.ledger {
        println!("  ledger            {l}");
    }
    println!("  status            {}", o.fetched.status);
    println!();
    println!("  {:<18}{:>18}  {:>14}", "", "stroops", "XLM");
    let row = |name: &str, v: i64| {
        println!("  {:<18}{:>18}  {:>14}", name, v, xlm(v));
    };
    row("tx.fee (bid)", r.tx_fee_bid);
    row("declared resource", r.declared_resource_fee);
    println!("  {:-<52}", "");
    row("inclusion", r.inclusion_fee);
    if let Some(nr) = r.non_refundable {
        row("non-refundable", nr);
    }
    if let Some(rc) = r.refundable_charged {
        row("refundable", rc);
    }
    row("fee charged", r.fee_charged);
    row("refunded", r.refunded);
    println!();
    println!(
        "  refund            {:.2}% of the declared resource fee",
        r.refund_ratio() * 100.0
    );
    if let Some(f) = r.over_declaration() {
        println!("  over-declared     {f:.2}x");
    }
    match (r.rent_upper_bound(), r.refundable_share()) {
        (Some(rent), Some(share)) => {
            println!(
                "  rent             <={} XLM ({:.2}% of the charge, less events and return value)",
                xlm(rent),
                share * 100.0
            );
        }
        _ => {
            println!("  rent              unknown without --non-refundable");
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Command::Inspect { tx, json } => {
            let o = observe(&tx)?;
            if json {
                let m = Measurement::from_reconciliation(
                    &o.reconciliation,
                    "inspect",
                    &tx.network,
                    &tx.hash,
                    Provenance::Rpc,
                );
                println!("{}", serde_json::to_string_pretty(&m)?);
            } else {
                print_table(&o, &tx.network, &tx.hash);
            }
        }

        Command::Reconcile { tx, predicted } => {
            let o = observe(&tx)?;
            let actual = o.reconciliation.fee_charged;
            let e = EstimateError::new(predicted, actual);
            println!(
                "predicted   {:>18} stroops  {:>14} XLM",
                predicted,
                xlm(predicted)
            );
            println!(
                "charged     {:>18} stroops  {:>14} XLM",
                actual,
                xlm(actual)
            );
            println!(
                "difference  {:>18} stroops  {:>14} XLM",
                e.delta(),
                xlm(e.delta())
            );
            match e.ratio() {
                Some(ratio) if ratio > 1.0 => {
                    println!("\nThe prediction was {ratio:.2}x too high.");
                }
                Some(ratio) if ratio < 1.0 => {
                    println!("\nThe prediction was {:.2}x too low.", 1.0 / ratio);
                }
                Some(_) => println!("\nThe prediction was exact."),
                None => println!("\nNothing was charged, so there is no ratio to report."),
            }
        }

        Command::Record {
            tx,
            label,
            observed_on,
            sdk,
            dataset: path,
        } => {
            let o = observe(&tx)?;
            let mut m = Measurement::from_reconciliation(
                &o.reconciliation,
                &label,
                &tx.network,
                &tx.hash,
                Provenance::Rpc,
            );
            m.ledger = o.fetched.ledger;
            m.sdk = sdk;
            m.observed_on = observed_on.or_else(|| o.fetched.created_at.map(iso_date));

            if dataset::append(&path, &m)? {
                println!(
                    "recorded {label} ({}) in {}",
                    xlm(m.fee_charged),
                    path.display()
                );
            } else {
                println!(
                    "{} is already recorded under '{label}'; nothing written",
                    tx.hash
                );
            }
        }

        Command::Report { dataset: path } => {
            let rows = dataset::load(&path)?;
            if rows.is_empty() {
                println!("{} holds no measurements yet.", path.display());
                return Ok(());
            }
            println!(
                "{:<34}{:>12}{:>16}{:>12}",
                "call", "date", "charged XLM", "refund"
            );
            for (label, group) in dataset::group_by_label(&rows) {
                for m in group {
                    println!(
                        "{:<34}{:>12}{:>16}{:>11.1}%",
                        label,
                        m.observed_on.as_deref().unwrap_or("-"),
                        xlm(m.fee_charged),
                        if m.declared_resource_fee == 0 {
                            0.0
                        } else {
                            m.refunded as f64 / m.declared_resource_fee as f64 * 100.0
                        }
                    );
                }
            }
            println!("\n{} measurements in {}", rows.len(), path.display());
        }
    }
    Ok(())
}

/// Unix seconds to `YYYY-MM-DD`, via days-since-epoch civil arithmetic.
fn iso_date(unix_seconds: i64) -> String {
    let days = unix_seconds.div_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rentwatch: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::iso_date;

    #[test]
    fn converts_unix_seconds_to_civil_dates() {
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(1_761_177_600), "2025-10-23");
        // A leap day, to catch off-by-one era arithmetic.
        assert_eq!(iso_date(1_583_020_800), "2020-03-01");
        assert_eq!(iso_date(1_582_934_400), "2020-02-29");
    }
}
