// SPDX-License-Identifier: Apache-2.0
//! Source-byte ledger validation.

use crate::byte_ledger::ByteLedger;
use crate::byte_ledger::ByteSpanClass;
use crate::report::{Check, Finding, Severity};
use crate::source_fidelity::RetainedSourceRecord;
use std::collections::BTreeMap;

fn finding(findings: &mut Vec<Finding>, message: impl Into<String>) {
    findings.push(Finding {
        check: Check::ByteAccounting,
        severity: Severity::Error,
        message: message.into(),
        entity: None,
    });
}

pub(super) fn check_byte_ledger(
    ledger: &ByteLedger,
    retained_records: &[RetainedSourceRecord],
    findings: &mut Vec<Finding>,
) {
    let mut retained_by_id = BTreeMap::new();
    for record in retained_records {
        if record.id.is_empty() {
            finding(findings, "retained source record has an empty id");
        }
        if retained_by_id.insert(record.id.as_str(), record).is_some() {
            finding(
                findings,
                format!("duplicate retained source record {:?}", record.id),
            );
        }
        if record.stream.is_empty() {
            finding(
                findings,
                format!("retained source record {:?} has an empty stream", record.id),
            );
        }
        if let Some(data) = &record.data {
            if record.byte_len != data.len() as u64 {
                finding(
                    findings,
                    format!(
                        "retained source record {:?} byte length disagrees with its data",
                        record.id
                    ),
                );
            }
            if crate::hash::sha256_hex(data) != record.sha256 {
                finding(
                    findings,
                    format!(
                        "retained source record {:?} digest disagrees with its data",
                        record.id
                    ),
                );
            }
        }
    }
    if ledger.source_length == 0 {
        if !ledger.spans.is_empty() {
            finding(findings, "empty source byte ledger contains spans");
        }
        return;
    }
    if ledger.spans.is_empty() {
        finding(findings, "nonempty source byte ledger has no spans");
        return;
    }

    let mut expected_start = 0;
    for span in &ledger.spans {
        if span.start > expected_start {
            finding(
                findings,
                format!("byte ledger has a gap before offset {}", span.start),
            );
        } else if span.start < expected_start {
            finding(
                findings,
                format!("byte ledger has an overlap at offset {}", span.start),
            );
        }
        if span.start >= span.end {
            finding(
                findings,
                format!(
                    "byte ledger span at offset {} is empty or reversed",
                    span.start
                ),
            );
        }
        if span.end > ledger.source_length {
            finding(
                findings,
                format!(
                    "byte ledger span ending at {} exceeds source length {}",
                    span.end, ledger.source_length
                ),
            );
        }
        if span.owner.is_empty() {
            finding(findings, "byte ledger span has an empty owner");
        }
        if span.meaning.is_empty() {
            finding(findings, "byte ledger span has an empty meaning");
        }
        match (span.class, span.retained_record.as_deref()) {
            (ByteSpanClass::Opaque, None | Some("")) => {
                finding(findings, "opaque byte ledger span has no retained record");
            }
            (ByteSpanClass::Typed | ByteSpanClass::Structural, Some(_)) => finding(
                findings,
                "typed or structural byte ledger span names a retained record",
            ),
            _ => {}
        }
        if let Some(id) = span.retained_record.as_deref() {
            if let Some(record) = retained_by_id.get(id) {
                let record_end = record.offset.checked_add(record.byte_len);
                if record.stream != "source"
                    || record.offset != span.start
                    || record_end != Some(span.end)
                {
                    finding(
                        findings,
                        format!(
                            "opaque byte ledger span and retained record {id:?} ranges disagree"
                        ),
                    );
                }
                if record.data.is_none() {
                    finding(
                        findings,
                        format!(
                            "opaque byte ledger span retained record {id:?} has no recovery bytes"
                        ),
                    );
                }
            } else {
                finding(
                    findings,
                    format!("byte ledger retained record {id:?} does not resolve"),
                );
            }
        }
        expected_start = expected_start.max(span.end);
    }
    if expected_start < ledger.source_length {
        finding(
            findings,
            format!("byte ledger ends before source length at offset {expected_start}"),
        );
    }
}
