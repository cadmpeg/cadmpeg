// SPDX-License-Identifier: Apache-2.0
//! The §4.5 cut-short contract (#215): cutting a source at 50% and 90% either
//! refuses with `Truncated` when no part survives, or decodes a prefix whose
//! every held record agrees with the uncut decode under
//! `Allowances::given_up`.

use std::collections::BTreeSet;

use cadmpeg_test_support::comparison::Allowances;

use super::support;

/// The cut fractions of the source length the rule names.
const CUTS: [usize; 2] = [50, 90];

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

/// Whether a report JSON loss charges the one part give-up the helper reports.
fn is_unreadable(loss: &serde_json::Value) -> bool {
    match loss.get("code") {
        Some(serde_json::Value::String(code)) => code == "decode/record.unreadable",
        Some(serde_json::Value::Object(code)) => {
            code.get("code").and_then(serde_json::Value::as_str) == Some(super::support::UNREADABLE)
        }
        _ => false,
    }
}

/// Whether an array element refers to a removed record, mirroring the
/// comparison's list pruning: a string equal to a removed identity, or an
/// object keyed by one or holding such a reference.
fn refers_to_removed(value: &serde_json::Value, removed: &BTreeSet<String>) -> bool {
    match value {
        serde_json::Value::String(id) => removed.contains(id),
        serde_json::Value::Array(_) => false,
        serde_json::Value::Object(fields) => fields
            .iter()
            .any(|(key, value)| removed.contains(key) || refers_to_removed(value, removed)),
        _ => false,
    }
}

/// Drop every list entry that refers to a removed record, mirroring the
/// comparison's pruning of the allowance set.
fn filter_lists(value: &mut serde_json::Value, removed: &BTreeSet<String>) {
    match value {
        serde_json::Value::Array(values) => {
            values.retain(|value| !refers_to_removed(value, removed));
            for value in values {
                filter_lists(value, removed);
            }
        }
        serde_json::Value::Object(fields) => {
            for value in fields.values_mut() {
                filter_lists(value, removed);
            }
        }
        _ => {}
    }
}

/// The uncut document restricted to the records the cut decode holds, with
/// every list entry that names a record the cut decode lacks removed, so the
/// held records compare under the rule 2 list loss alone.
fn restrict_to_held(
    uncut: &serde_json::Value,
    held: &BTreeSet<String>,
    missing: &BTreeSet<String>,
) -> serde_json::Value {
    let mut restricted = uncut.clone();
    let mut restrict_rows = |rows: &mut serde_json::Value| {
        if let Some(rows) = rows.as_array_mut() {
            rows.retain(|row| {
                row.get("id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| held.contains(id))
            });
            for row in rows.iter_mut() {
                filter_lists(row, missing);
            }
        }
    };
    if let Some(model) = restricted
        .get_mut("model")
        .and_then(|model| model.as_object_mut())
    {
        model.values_mut().for_each(&mut restrict_rows);
    }
    if let Some(native) = restricted
        .get_mut("native")
        .and_then(|native| native.as_object_mut())
    {
        for arenas in native
            .values_mut()
            .filter_map(|arenas| arenas.as_object_mut())
        {
            arenas.values_mut().for_each(&mut restrict_rows);
        }
    }
    restricted
}

fn check_format(format: &str) {
    let sampled = super::support::sampled_fixtures(format);
    let mut fixtures: Vec<super::support::Fixture> = sampled
        .into_iter()
        .filter(|fixture| !fixture.damaged)
        .collect();
    for damaged in super::support::damaged_fixtures(format) {
        if !fixtures.iter().any(|fixture| fixture.path == damaged.path) {
            fixtures.push(damaged);
        }
    }
    fixtures.sort_by(|left, right| left.path.cmp(&right.path));
    let mut findings = Vec::new();
    for fixture in &fixtures {
        if fixture.bytes.is_empty() {
            continue;
        }
        let uncut = match super::support::decode(format, &fixture.bytes) {
            Ok(decoded) => decoded,
            Err(_) if super::support::pins_decode_error(&fixture.path) => continue,
            Err(error) => {
                findings.push(format!(
                    "{format}: {} refuses its uncut input: {error}",
                    fixture.path.display()
                ));
                continue;
            }
        };
        let uncut_document = super::support::document(&uncut);
        let uncut_ids = record_ids(&uncut_document);
        for percent in CUTS {
            let length = fixture.bytes.len() * percent / 100;
            let cut = &fixture.bytes[..length];
            match super::support::decode(format, cut) {
                Ok(decoded) => {
                    let document = super::support::document(&decoded);
                    let report = super::support::report(&decoded);
                    let held = record_ids(&document);
                    let stubs = support::stub_ids(format, &document);
                    for added in held.difference(&uncut_ids) {
                        if !stubs.contains(added) {
                            findings.push(format!(
                                "{format}: {} cut at {percent}% adds non-stub record {added}",
                                fixture.path.display()
                            ));
                        }
                    }
                    let missing: BTreeSet<String> = uncut_ids.difference(&held).cloned().collect();
                    let unreadable = report
                        .get("losses")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|losses| losses.iter().any(is_unreadable));
                    if !missing.is_empty() && !unreadable {
                        findings.push(format!(
                            "{format}: {} cut at {percent}% lacks {} uncut record(s) but charges no record.unreadable",
                            fixture.path.display(),
                            missing.len()
                        ));
                    }
                    let comparable: BTreeSet<String> = held.difference(&stubs).cloned().collect();
                    let restricted = restrict_to_held(&uncut_document, &comparable, &missing);
                    match Allowances::given_up(&uncut_document, &report) {
                        Ok(allowances) => {
                            if let Some(mismatch) =
                                super::support::check_agree(&restricted, &document, &allowances)
                            {
                                findings.push(format!(
                                    "{format}: {} cut at {percent}% disagrees with the uncut decode: {mismatch}",
                                    fixture.path.display()
                                ));
                            }
                        }
                        Err(error) => findings.push(format!(
                            "{format}: {} cut at {percent}%: Allowances::given_up refuses the cut report: {error}",
                            fixture.path.display()
                        )),
                    }
                }
                Err(error) if super::support::is_truncated(&error) => {}
                Err(error) => findings.push(format!(
                    "{format}: {} cut at {percent}% refuses without Truncated: {error}",
                    fixture.path.display()
                )),
            }
        }
    }
    support::assert_no_findings(&findings);
}

macro_rules! cut_short {
    ($($module:ident => $format:literal, $test:ident);* $(;)?) => {
        $(#[cfg(feature = $format)]
        mod $module {
            #[test]
            fn $test() {
                super::check_format($format);
            }
        })*
    };
}

cut_short! {
    step_cut_short => "step", step_cut_short_files_decode_their_prefix_or_are_truncated;
    iges_cut_short => "iges", iges_cut_short_files_decode_their_prefix_or_are_truncated;
    rhino_cut_short => "rhino", rhino_cut_short_files_decode_their_prefix_or_are_truncated;
    freecad_cut_short => "fcstd", freecad_cut_short_files_decode_their_prefix_or_are_truncated;
    f3d_cut_short => "f3d", f3d_cut_short_files_decode_their_prefix_or_are_truncated;
    sldprt_cut_short => "sldprt", sldprt_cut_short_files_decode_their_prefix_or_are_truncated;
    catia_cut_short => "catia", catia_cut_short_files_decode_their_prefix_or_are_truncated;
    creo_cut_short => "creo", creo_cut_short_files_decode_their_prefix_or_are_truncated;
    usd_cut_short => "usd", usd_cut_short_files_decode_their_prefix_or_are_truncated;
    inventor_cut_short => "inventor", inventor_cut_short_files_decode_their_prefix_or_are_truncated;
}
