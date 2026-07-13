// SPDX-License-Identifier: Apache-2.0
//! Source-byte ledger validation.

use crate::byte_ledger::ByteSpanClass;
use crate::document::CadIr;
use crate::report::{Check, Finding, Severity};
use std::collections::HashSet;

fn finding(findings: &mut Vec<Finding>, message: impl Into<String>) {
    findings.push(Finding {
        check: Check::ByteAccounting,
        severity: Severity::Error,
        message: message.into(),
        entity: None,
    });
}

pub(super) fn check_byte_ledger(
    ir: &CadIr,
    retained_ids: &HashSet<String>,
    findings: &mut Vec<Finding>,
) {
    let ledger = &ir.byte_ledger;
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
        if span
            .retained_record
            .as_ref()
            .is_some_and(|id| !retained_ids.contains(id))
        {
            finding(
                findings,
                format!(
                    "byte ledger retained record {:?} does not resolve",
                    span.retained_record.as_deref().unwrap_or_default()
                ),
            );
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
