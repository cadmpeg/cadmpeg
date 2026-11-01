// SPDX-License-Identifier: Apache-2.0
//! The §4.5 damaged-part contract (#215): a decoder gives up only the part it
//! cannot read and keeps the rest.
//!
//! Each codec with a damaged fixture decodes the damaged input and its
//! undamaged input and asserts the rule; a damaged input without a declared
//! base asserts the base-free checks. Codecs without a table entry below have
//! no damaged fixture yet.

use std::collections::BTreeSet;
use std::path::Path;

use cadmpeg_ir::report::Severity;
use cadmpeg_test_support::comparison::Allowances;

use super::support::{self, Fixture};

/// One damaged input and its undamaged input, as path suffixes below the
/// fixture root. A `None` base is a synthesized damaged input: it asserts
/// success, give-up losses and loss-code hygiene without a comparison.
struct Pair {
    base: Option<String>,
    damaged: String,
}

fn ends_with(path: &Path, suffix: &str) -> bool {
    path.to_string_lossy().replace('\\', "/").ends_with(suffix)
}

fn first_difference(base: &[u8], damaged: &[u8]) -> Option<usize> {
    base.iter()
        .zip(damaged.iter())
        .position(|(left, right)| left != right)
        .or_else(|| (base.len() != damaged.len()).then(|| base.len().min(damaged.len())))
}

/// Every record id the document holds, across neutral and native arenas.
fn record_ids(document: &serde_json::Value) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let walk = |value: &serde_json::Value, ids: &mut BTreeSet<String>| {
        if let Some(rows) = value.as_array() {
            for row in rows {
                if let Some(id) = row.get("id").and_then(serde_json::Value::as_str) {
                    ids.insert(id.to_owned());
                }
            }
        }
    };
    if let Some(model) = document.get("model").and_then(serde_json::Value::as_object) {
        for value in model.values() {
            walk(value, &mut ids);
        }
    }
    if let Some(native) = document
        .get("native")
        .and_then(serde_json::Value::as_object)
    {
        for namespace in native.values() {
            if let Some(arenas) = namespace.as_object() {
                for value in arenas.values() {
                    walk(value, &mut ids);
                }
            }
        }
    }
    ids
}

/// The ids of every `unknowns` stub the document holds.
fn stub_ids(document: &serde_json::Value) -> BTreeSet<String> {
    let mut stubs = BTreeSet::new();
    if let Some(native) = document
        .get("native")
        .and_then(serde_json::Value::as_object)
    {
        for namespace in native.values() {
            if let Some(rows) = namespace
                .get("unknowns")
                .and_then(serde_json::Value::as_array)
            {
                for row in rows {
                    if let Some(id) = row.get("id").and_then(serde_json::Value::as_str) {
                        stubs.insert(id.to_owned());
                    }
                }
            }
        }
    }
    stubs
}

fn check_pair(format: &str, fixtures: &[Fixture], pair: &Pair) -> Vec<String> {
    let mut findings = Vec::new();
    let Some(damaged) = fixtures
        .iter()
        .find(|fixture| ends_with(&fixture.path, &pair.damaged))
    else {
        return vec![format!(
            "{format}: damaged input {} is missing",
            pair.damaged
        )];
    };
    let damaged_decoded = match support::decode(format, &damaged.bytes) {
        Ok(decoded) => decoded,
        Err(error) => {
            return vec![format!(
                "{format}: {} refuses its damaged input: {error}",
                damaged.path.display()
            )];
        }
    };
    if !damaged_decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == super::support::UNREADABLE)
    {
        findings.push(format!(
            "{format}: {} charges no record.unreadable",
            damaged.path.display()
        ));
    }
    for loss in &damaged_decoded.report().losses {
        if loss.code.local_code() != super::support::UNREADABLE {
            continue;
        }
        if loss.severity != Severity::Error {
            findings.push(format!(
                "{format}: {}: a record.unreadable loss is {:?}, not Error",
                damaged.path.display(),
                loss.severity
            ));
        }
        if loss.provenance.is_none() {
            findings.push(format!(
                "{format}: {}: a record.unreadable loss names no source span",
                damaged.path.display()
            ));
        }
    }
    let Some(base_suffix) = pair.base.as_deref() else {
        return findings;
    };
    let Some(base) = fixtures.iter().find(|fixture| {
        fixture
            .path
            .file_name()
            .is_some_and(|name| name == base_suffix)
    }) else {
        findings.push(format!(
            "{format}: undamaged input {base_suffix} is missing"
        ));
        return findings;
    };
    let base_decoded = match support::decode(format, &base.bytes) {
        Ok(decoded) => decoded,
        Err(error) => {
            findings.push(format!(
                "{format}: {} refuses its undamaged input: {error}",
                base.path.display()
            ));
            return findings;
        }
    };
    let base_document = support::document(&base_decoded);
    let damaged_document = support::document(&damaged_decoded);
    let damaged_report = support::report(&damaged_decoded);
    if let Some(first) = first_difference(&base.bytes, &damaged.bytes) {
        for loss in &damaged_decoded.report().losses {
            if loss.code.local_code() != super::support::UNREADABLE {
                continue;
            }
            if let Some(span) = loss.provenance.as_ref() {
                if span.stream_name().is_none() {
                    if span.offset < first as u64 {
                        findings.push(format!(
                            "{format}: {}: a given-up part at offset {} predates the damage at byte {first}",
                            damaged.path.display(),
                            span.offset
                        ));
                    }
                    if let Some(length) = span.byte_len() {
                        if span.offset + length > damaged.bytes.len() as u64 {
                            findings.push(format!(
                                "{format}: {}: a given-up part runs past the end of the source",
                                damaged.path.display()
                            ));
                        }
                    }
                }
            }
        }
    }
    match Allowances::given_up(&base_document, &damaged_report) {
        Ok(allowances) => {
            if let Some(mismatch) =
                support::check_agree(&base_document, &damaged_document, &allowances)
            {
                findings.push(format!(
                    "{format}: {} disagrees with {}: {mismatch}",
                    damaged.path.display(),
                    base.path.display()
                ));
            }
        }
        Err(error) => findings.push(format!(
            "{format}: {}: Allowances::given_up refuses the damaged report: {error}",
            damaged.path.display()
        )),
    }
    let stubs = stub_ids(&damaged_document);
    let before = record_ids(&base_document);
    let after = record_ids(&damaged_document);
    for added in after.difference(&before) {
        if !stubs.contains(added) {
            findings.push(format!(
                "{format}: {} adds non-stub record {added}",
                damaged.path.display()
            ));
        }
    }
    let base_codes: BTreeSet<&str> = base_decoded
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code())
        .collect();
    for loss in &damaged_decoded.report().losses {
        let code = loss.code.local_code();
        if code != super::support::UNREADABLE && !base_codes.contains(code) {
            findings.push(format!(
                "{format}: {} charges {code}, which the undamaged decode does not charge",
                damaged.path.display()
            ));
        }
    }
    findings
}

fn check_format(format: &str, pairs: Vec<Pair>) {
    let fixtures = support::fixtures(format);
    let mut findings = Vec::new();
    for pair in &pairs {
        findings.extend(check_pair(format, &fixtures, pair));
    }
    let damaged = support::damaged_fixtures(format);
    for fixture in &damaged {
        if pairs
            .iter()
            .any(|pair| ends_with(&fixture.path, &pair.damaged))
        {
            continue;
        }
        findings.extend(check_pair(
            format,
            &fixtures,
            &Pair {
                base: None,
                damaged: fixture.path.to_string_lossy().into_owned(),
            },
        ));
    }
    if pairs.is_empty() && damaged.is_empty() {
        findings.push(format!("{format}: no damaged input covers the §4.5 rule"));
    }
    support::assert_no_findings(&findings);
}

macro_rules! damaged_part {
    ($($module:ident => $format:literal, $test:ident, [$($base:expr => $damaged:expr),* $(,)?]);* $(;)?) => {
        $(#[cfg(feature = $format)]
        mod $module {
            #[test]
            fn $test() {
                super::check_format(
                    $format,
                    vec![$(super::Pair {
                        base: ($base).map(str::to_owned),
                        damaged: $damaged.to_owned(),
                    }),*],
                );
            }
        })*
    };
}

damaged_part! {
    iges_damaged_part => "iges", iges_damaged_parts_give_up_only_the_damaged_part, [Some("point.igs") => "damaged/point_bad_number.igs"];
    step_damaged_part => "step", step_damaged_parts_give_up_only_the_damaged_part, [Some("ap214_sheet.p21") => "damaged/ap214_sheet_bad_real.p21"];
    freecad_damaged_part => "fcstd", freecad_damaged_parts_give_up_only_the_damaged_part, [None::<&str> => "damaged/bad_integer_property.FCStd"];
}
