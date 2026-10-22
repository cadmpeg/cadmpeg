// SPDX-License-Identifier: Apache-2.0
//! Native linkage checks for dumped goldens and decoded writer outputs.
//!
//! Neutral identities come only from the closed model arena registry. Native
//! identities come only from actual native arena rows. Identity spelling does
//! not determine ownership.

use std::collections::BTreeSet;

use cadmpeg_ir::ArenaName;
use serde_json::Value;

/// Evidence from one whole-document linkage check.
#[derive(Debug, Default)]
pub struct NativeLinkageAudit {
    /// Registered neutral rows inspected, including rows without native links.
    pub neutral_records: usize,
    /// Actual native arena rows inspected.
    pub native_records: usize,
    /// Present neutral row `native_ref` fields checked for native ownership.
    pub checked_native_refs: usize,
    /// Exact JSON pointers and identities for each violation.
    pub findings: Vec<String>,
}

/// Check the full native rows of a real document before snapshot elision.
///
/// # Panics
///
/// Panics if the complete document cannot be inspected or any link violates
/// check 8. Each failure states the exact JSON pointer and identity.
pub fn assert_native_linkage(document: &cadmpeg_ir::CadIr) -> NativeLinkageAudit {
    check_native_linkage(document).unwrap_or_else(|failure| panic!("{failure}"))
}

/// Run check 8 over one codec's golden inputs, each decoded with
/// `DecodeOptions::default()`, as the golden snapshots decode them.
///
/// A golden whose decode is refused holds no document, so it has no native row
/// to check. Every decoded golden is checked; none is exempt.
///
/// # Panics
///
/// Panics when `inputs` is empty or no input decodes, so the check cannot pass
/// vacuously. Panics listing every golden whose document violates check 8 at
/// once rather than stopping at the first.
pub fn assert_golden_native_linkage(
    codec: &dyn cadmpeg_ir::codec::Codec,
    inputs: &[(String, Vec<u8>)],
) {
    assert!(
        !inputs.is_empty(),
        "no golden inputs; check 8 would pass vacuously"
    );
    let mut decoded = 0_usize;
    let mut failures = Vec::new();
    for (name, bytes) in inputs {
        let Ok(result) = codec.decode(
            &mut std::io::Cursor::new(bytes.as_slice()),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        ) else {
            continue;
        };
        decoded += 1;
        if let Err(failure) = check_native_linkage(result.ir()) {
            failures.push(format!("golden `{name}`: {failure}"));
        }
    }
    assert!(
        decoded > 0,
        "no golden input decodes; check 8 would pass vacuously"
    );
    assert!(
        failures.is_empty(),
        "check 8 failed on {} golden(s):\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

fn check_native_linkage(document: &cadmpeg_ir::CadIr) -> Result<NativeLinkageAudit, String> {
    let value = serde_json::to_value(document)
        .map_err(|error| format!("serialize the complete golden document: {error}"))?;
    let audit = audit_native_linkage(&value)
        .map_err(|error| format!("inspect every golden native row: {error}"))?;
    if audit.neutral_records != document.model.entity_count() {
        return Err(format!(
            "every neutral row must be inspected: {} of {}",
            audit.neutral_records,
            document.model.entity_count()
        ));
    }
    let native_records: usize = document
        .native
        .0
        .values()
        .flat_map(|namespace| namespace.arenas().values())
        .map(Vec::len)
        .sum();
    if audit.native_records != native_records {
        return Err(format!(
            "every actual native row must be inspected: {} of {native_records}",
            audit.native_records
        ));
    }
    if !audit.findings.is_empty() {
        return Err(format!(
            "check 8 found {} native linkage violations:\n{}",
            audit.findings.len(),
            audit.findings.join("\n")
        ));
    }
    Ok(audit)
}

/// Check #213 linkage and #246 IV.2 check 8 on one serialized CADIR document.
///
/// Every string value and map key inside each native row is checked, at every
/// depth. A string that is also an actual native row identity denotes that
/// native row, as check 8 requires. No namespace prefix or field name grants
/// an exemption. Present neutral row `native_ref` fields must resolve to an
/// actual native row; an absent optional link states no association.
///
/// Use the same function on documents decoded from §5.1 and §5.2 outputs.
///
/// # Errors
///
/// Returns the exact structural path that could not be inspected. In
/// particular, an elided native snapshot is not a successful empty audit.
pub fn audit_native_linkage(document: &Value) -> Result<NativeLinkageAudit, String> {
    let model = document
        .get("model")
        .and_then(Value::as_object)
        .ok_or_else(|| "/model is not an object".to_owned())?;
    let native = document
        .get("native")
        .and_then(Value::as_object)
        .ok_or_else(|| "/native is not an object".to_owned())?;
    let mut audit = NativeLinkageAudit::default();
    let mut neutral_ids = BTreeSet::new();
    let mut inward_links = Vec::new();
    for name in model.keys() {
        if ArenaName::parse(name).is_none() {
            return Err(format!(
                "/model/{} is not a registered neutral arena",
                escape(name)
            ));
        }
    }
    for arena in ArenaName::ALL {
        let Some(rows) = model.get(arena.as_str()) else {
            continue;
        };
        let pointer = format!("/model/{}", arena.as_str());
        let rows = rows
            .as_array()
            .ok_or_else(|| format!("{pointer} is not an array"))?;
        for (index, row) in rows.iter().enumerate() {
            let pointer = format!("{pointer}/{index}");
            let id = row
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{pointer}/id is not a string"))?;
            neutral_ids.insert(id);
            audit.neutral_records += 1;
            if let Some(reference) = row.get("native_ref") {
                let reference = reference
                    .as_str()
                    .ok_or_else(|| format!("{pointer}/native_ref is not a string"))?;
                inward_links.push((format!("{pointer}/native_ref"), reference));
            }
        }
    }
    let mut native_ids = BTreeSet::new();
    let mut native_rows = Vec::new();
    for (namespace, arenas) in native {
        let pointer = format!("/native/{}", escape(namespace));
        let arenas = arenas.as_object().ok_or_else(|| format!("{pointer} is not an arena map; native content is unavailable (including elided snapshots)"))?;
        for (arena, rows) in arenas {
            let pointer = format!("{pointer}/{}", escape(arena));
            let rows = rows
                .as_array()
                .ok_or_else(|| format!("{pointer} is not an array"))?;
            for (index, row) in rows.iter().enumerate() {
                let pointer = format!("{pointer}/{index}");
                let id = row
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("{pointer}/id is not a string"))?;
                native_ids.insert(id);
                native_rows.push((pointer, row));
                audit.native_records += 1;
            }
        }
    }
    for (pointer, reference) in inward_links {
        audit.checked_native_refs += 1;
        if !native_ids.contains(reference) {
            audit.findings.push(format!(
                "{pointer}: native_ref `{reference}` has no native record"
            ));
        }
    }
    for (pointer, row) in native_rows {
        inspect_strings(
            row,
            &pointer,
            &neutral_ids,
            &native_ids,
            &mut audit.findings,
        );
    }
    audit.findings.sort();
    Ok(audit)
}

fn inspect_strings(
    value: &Value,
    pointer: &str,
    neutral: &BTreeSet<&str>,
    native: &BTreeSet<&str>,
    findings: &mut Vec<String>,
) {
    match value {
        Value::String(text) => inspect_string(text, pointer, false, neutral, native, findings),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                inspect_strings(
                    item,
                    &format!("{pointer}/{index}"),
                    neutral,
                    native,
                    findings,
                );
            }
        }
        Value::Object(fields) => {
            for (key, child) in fields {
                let pointer = format!("{pointer}/{}", escape(key));
                inspect_string(key, &pointer, true, neutral, native, findings);
                inspect_strings(child, &pointer, neutral, native, findings);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn inspect_string(
    text: &str,
    pointer: &str,
    key: bool,
    neutral: &BTreeSet<&str>,
    native: &BTreeSet<&str>,
    findings: &mut Vec<String>,
) {
    if neutral.contains(text) && !native.contains(text) {
        let role = if key { "map key" } else { "string" };
        findings.push(format!(
            "{pointer}: native {role} equals neutral id `{text}`"
        ));
    }
}

fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests;
