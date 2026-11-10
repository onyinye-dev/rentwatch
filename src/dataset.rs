//! The append-only record.
//!
//! One JSON object per line, newest appended last. The point of keeping it in
//! the repository rather than a database is that a fee change across protocol
//! versions then shows up as a diff, and anyone can re-derive the numbers from
//! the transaction hashes.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fee::Reconciliation;

/// Where a measurement's numbers came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provenance {
    /// Decoded from XDR this tool fetched from an RPC endpoint.
    Rpc,
    /// Copied from a published document, with the hash recorded for re-checking.
    Transcribed,
}

/// One reconciled transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measurement {
    /// What was invoked, in a form that groups across runs: `deposit (first)`.
    pub label: String,
    pub network: String,
    pub tx_hash: String,
    pub provenance: Provenance,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ledger: Option<u32>,
    /// ISO-8601 date, so the record reads chronologically without parsing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdk: Option<String>,

    pub tx_fee_bid: i64,
    pub declared_resource_fee: i64,
    pub inclusion_fee: i64,
    pub fee_charged: i64,
    pub resource_fee_charged: i64,
    pub refunded: i64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub non_refundable: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refundable_charged: Option<i64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Measurement {
    /// Build a record from a reconciliation plus its identifying metadata.
    pub fn from_reconciliation(
        r: &Reconciliation,
        label: impl Into<String>,
        network: impl Into<String>,
        tx_hash: impl Into<String>,
        provenance: Provenance,
    ) -> Self {
        Self {
            label: label.into(),
            network: network.into(),
            tx_hash: tx_hash.into(),
            provenance,
            ledger: None,
            observed_on: None,
            sdk: None,
            tx_fee_bid: r.tx_fee_bid,
            declared_resource_fee: r.declared_resource_fee,
            inclusion_fee: r.inclusion_fee,
            fee_charged: r.fee_charged,
            resource_fee_charged: r.resource_fee_charged,
            refunded: r.refunded,
            non_refundable: r.non_refundable,
            refundable_charged: r.refundable_charged,
            note: None,
        }
    }
}

#[derive(Debug)]
pub enum DatasetError {
    Io(std::io::Error),
    /// A line would not parse. Carries the 1-based line number.
    Parse {
        line: usize,
        source: String,
    },
}

impl std::fmt::Display for DatasetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Parse { line, source } => {
                write!(f, "line {line} of the dataset is not valid: {source}")
            }
        }
    }
}

impl std::error::Error for DatasetError {}

impl From<std::io::Error> for DatasetError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Read every measurement. A missing file is an empty dataset, not an error.
pub fn load(path: &Path) -> Result<Vec<Measurement>, DatasetError> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let mut out = Vec::new();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let m = serde_json::from_str(&line).map_err(|e| DatasetError::Parse {
            line: i + 1,
            source: e.to_string(),
        })?;
        out.push(m);
    }
    Ok(out)
}

/// Append one measurement, creating the file if it does not exist.
///
/// Returns `false` without writing if this exact hash and label are already
/// recorded, so re-running the tool over the same transaction is harmless.
pub fn append(path: &Path, m: &Measurement) -> Result<bool, DatasetError> {
    let existing = load(path)?;
    if existing
        .iter()
        .any(|e| e.tx_hash == m.tx_hash && e.label == m.label)
    {
        return Ok(false);
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(m).map_err(|e| DatasetError::Parse {
        line: 0,
        source: e.to_string(),
    })?;
    writeln!(file, "{line}")?;
    Ok(true)
}

/// Measurements sharing a label, which is how a fee change becomes visible.
pub fn group_by_label(measurements: &[Measurement]) -> Vec<(String, Vec<&Measurement>)> {
    let mut labels: Vec<String> = Vec::new();
    for m in measurements {
        if !labels.contains(&m.label) {
            labels.push(m.label.clone());
        }
    }
    labels
        .into_iter()
        .map(|label| {
            let rows = measurements.iter().filter(|m| m.label == label).collect();
            (label, rows)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fee::{reconcile, Context, Declared};

    fn sample(label: &str, hash: &str, charged: i64) -> Measurement {
        let r = reconcile(
            Declared {
                tx_fee: 1_100,
                resource_fee: 1_000,
            },
            charged,
            Context::default(),
        )
        .unwrap();
        Measurement::from_reconciliation(&r, label, "testnet", hash, Provenance::Rpc)
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "rentwatch-test-{}-{name}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn a_missing_file_is_an_empty_dataset() {
        let p = tmp("missing");
        assert_eq!(load(&p).unwrap(), Vec::new());
    }

    #[test]
    fn appends_then_reads_back_identically() {
        let p = tmp("roundtrip");
        let m = sample("deposit (first)", "aa", 600);
        assert!(append(&p, &m).unwrap());

        let back = load(&p).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0], m);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn appending_the_same_measurement_twice_is_a_no_op() {
        let p = tmp("dedupe");
        let m = sample("deposit (first)", "bb", 600);
        assert!(append(&p, &m).unwrap());
        assert!(!append(&p, &m).unwrap(), "second append should be skipped");
        assert_eq!(load(&p).unwrap().len(), 1);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn the_same_hash_under_a_different_label_is_a_separate_row() {
        let p = tmp("relabel");
        assert!(append(&p, &sample("deposit", "cc", 600)).unwrap());
        assert!(append(&p, &sample("collect", "cc", 600)).unwrap());
        assert_eq!(load(&p).unwrap().len(), 2);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_corrupt_line_names_its_line_number() {
        let p = tmp("corrupt");
        append(&p, &sample("deposit", "dd", 600)).unwrap();
        let mut f = OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, "{{ not json").unwrap();
        drop(f);

        match load(&p) {
            Err(DatasetError::Parse { line, .. }) => assert_eq!(line, 2),
            other => panic!("expected a parse error, got {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn blank_lines_are_skipped() {
        let p = tmp("blank");
        append(&p, &sample("deposit", "ee", 600)).unwrap();
        let mut f = OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f).unwrap();
        drop(f);
        assert_eq!(load(&p).unwrap().len(), 1);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn grouping_preserves_first_appearance_order() {
        let rows = vec![
            sample("deposit", "a", 600),
            sample("collect", "b", 600),
            sample("deposit", "c", 700),
        ];
        let grouped = group_by_label(&rows);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].0, "deposit");
        assert_eq!(grouped[0].1.len(), 2);
        assert_eq!(grouped[1].0, "collect");
    }

    #[test]
    fn optional_fields_stay_out_of_the_json_when_unset() {
        let m = sample("deposit", "ff", 600);
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("ledger"), "{json}");
        assert!(!json.contains("note"), "{json}");
        assert!(json.contains("\"provenance\":\"rpc\""), "{json}");
    }
}
