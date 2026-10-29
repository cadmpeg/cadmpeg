// SPDX-License-Identifier: Apache-2.0
//! W-closure-same (§5.1, #232) and §5.4 item 1 over one decoded input.
//!
//! Each check returns findings, so a caller names every failing input in one
//! run. The re-decode is the caller's: the CADIR writer re-reads JSON, every
//! other writer re-reads through its own codec.

use std::collections::BTreeSet;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, SynthesisCause};
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::report::export::{ExportReport, WritePath};
use serde_json::Value;

use crate::comparison::{decodes_agree, stub_extents, unnamed_removals, Allowances, Mode};

/// The STEP target §5.1 requests when the golden's dialect is off-catalog.
pub const STEP_SAME_TARGET: &str = "step:ap242-e3";

/// One decode as the harness compares it.
#[derive(Debug, Clone)]
pub struct Redecoded {
    /// The decoded CADIR document.
    pub ir: Value,
    /// The decode's loss codes, as `namespace/code`.
    pub loss_codes: BTreeSet<String>,
    /// The output's container member names; empty for a single stream.
    pub members: Vec<String>,
}

/// The decode of one written output, by the writer's own format.
pub type Redecode<'a> = dyn Fn(&[u8]) -> Result<Redecoded, String> + 'a;

/// The §5.1 target for a golden: `Inherit` when the catalog has a row equal
/// to the golden's recorded dialect, otherwise the catalog default (for STEP
/// `step:ap242-e3`). The flag is true when the target displaces the source
/// dialect.
#[must_use]
pub fn same_request(encoder: &dyn Encoder, first: &DecodeResult) -> (TargetRequest<'static>, bool) {
    let catalog = encoder.targets();
    let recorded = first
        .ir()
        .source
        .as_ref()
        .filter(|source| source.format() == encoder.id().as_str())
        .and_then(|source| source.dialect())
        .map(|matched| matched.dialect().as_str().to_owned());
    if catalog.is_empty()
        || recorded
            .as_deref()
            .is_some_and(|dialect| catalog.find(dialect).is_some())
    {
        return (TargetRequest::Inherit, false);
    }
    if encoder.id().as_str() == "step" {
        return (TargetRequest::Explicit(STEP_SAME_TARGET), true);
    }
    match catalog.default() {
        Some((_, target)) => (TargetRequest::Explicit(target.id.as_str()), true),
        None => (TargetRequest::Inherit, false),
    }
}

/// One written output and its export report.
pub struct Written {
    /// Output bytes.
    pub bytes: Vec<u8>,
    /// The export report.
    pub report: ExportReport,
}

/// Plan and write one document.
///
/// # Errors
///
/// Returns the plan or write refusal; §5.1 counts every refusal as a failure.
pub fn write(
    encoder: &dyn Encoder,
    first: &DecodeResult,
    with_fidelity: bool,
    request: TargetRequest<'_>,
) -> Result<Written, String> {
    let fidelity = with_fidelity.then(|| first.source_fidelity());
    let plan = encoder
        .plan(EncodeInput::new(first.ir(), fidelity), request)
        .map_err(|error| format!("plan refused: {error}"))?;
    let mut bytes = Vec::new();
    let report = plan
        .write_to(&mut bytes)
        .map_err(|error| format!("write refused: {error}"))?;
    Ok(Written { bytes, report })
}

fn loss_codes(report: &cadmpeg_ir::report::decode::DecodeReport) -> BTreeSet<String> {
    report
        .losses
        .iter()
        .map(|loss| loss.code.to_string())
        .collect()
}

/// §5.1(a): write without fidelity, re-decode, and compare in `Same` mode.
///
/// Findings name a refusal, a disagreement, a removal no loss names, a
/// re-decode loss code absent from the first decode (§5.4 item 1) and a
/// structural name of the application (check 9).
#[must_use]
pub fn without_fidelity(
    first: &DecodeResult,
    encoder: &dyn Encoder,
    redecode: &Redecode<'_>,
) -> Vec<String> {
    let (request, _) = same_request(encoder, first);
    let written = match write(encoder, first, false, request) {
        Ok(written) => written,
        Err(refusal) => return vec![refusal],
    };
    compare_written(first, &written, redecode)
}

fn compare_written(
    first: &DecodeResult,
    written: &Written,
    redecode: &Redecode<'_>,
) -> Vec<String> {
    let mut findings = Vec::new();
    let before = match serde_json::to_value(first.ir()) {
        Ok(value) => value,
        Err(error) => return vec![format!("the first decode does not serialize: {error}")],
    };
    let fidelity = serde_json::to_value(first.source_fidelity()).unwrap_or(Value::Null);
    let after = match redecode(&written.bytes) {
        Ok(after) => after,
        Err(error) => return vec![format!("the output does not decode: {error}")],
    };
    match Allowances::export(&before)
        .and_then(|allowances| decodes_agree(&before, &after.ir, &allowances, Mode::Same))
    {
        Ok(removed) => {
            let stubs = stub_extents(&before, &fidelity);
            for id in unnamed_removals(&removed, &stubs, &written.report.losses) {
                findings.push(format!("removed record {id} is named by no loss"));
            }
        }
        Err(error) => findings.push(format!("the re-decode disagrees: {error}")),
    }
    let original = loss_codes(first.report());
    for code in after.loss_codes.difference(&original) {
        findings.push(format!(
            "the re-decode charges {code}, which the first decode does not"
        ));
    }
    findings.extend(structural_name_findings(&after.ir, &after.members));
    findings
}

/// §5.1(b): write with fidelity, unedited, for a writer that patches or replays.
///
/// With an empty `unrepresented` the plan patches or replays with no export
/// loss and the re-decode agrees in both directions. Otherwise it synthesizes
/// with `SourceUnrepresented { classes == unrepresented }` and (a) applies.
#[must_use]
pub fn with_fidelity(
    first: &DecodeResult,
    encoder: &dyn Encoder,
    redecode: &Redecode<'_>,
) -> Vec<String> {
    let (request, _) = same_request(encoder, first);
    let written = match write(encoder, first, true, request) {
        Ok(written) => written,
        Err(refusal) => return vec![refusal],
    };
    let unrepresented = first.source_fidelity().unrepresented();
    if unrepresented.is_empty() {
        let mut findings = Vec::new();
        if !matches!(
            written.report.write_path(),
            WritePath::Patched { .. } | WritePath::VerbatimReplay { .. }
        ) {
            findings.push(format!(
                "an unrepresented-free decode was not patched or replayed: {:?}",
                written.report.write_path()
            ));
        }
        if !written.report.losses.is_empty() {
            findings.push(format!(
                "an unedited patch or replay charges {} export loss(es)",
                written.report.losses.len()
            ));
        }
        findings.extend(both_directions(first, &written, redecode));
        return findings;
    }
    let mut findings = Vec::new();
    match written.report.synthesis_cause() {
        Some(SynthesisCause::SourceUnrepresented { classes }) if *classes == unrepresented => {}
        cause => findings.push(format!(
            "unrepresented {unrepresented} must synthesize with SourceUnrepresented, found {cause:?}"
        )),
    }
    findings.extend(compare_written(first, &written, redecode));
    findings
}

fn both_directions(
    first: &DecodeResult,
    written: &Written,
    redecode: &Redecode<'_>,
) -> Vec<String> {
    let Ok(before) = serde_json::to_value(first.ir()) else {
        return vec!["the first decode does not serialize".to_owned()];
    };
    let after = match redecode(&written.bytes) {
        Ok(after) => after,
        Err(error) => return vec![format!("the output does not decode: {error}")],
    };
    let mut findings = Vec::new();
    for (label, left, right) in [
        ("forward", &before, &after.ir),
        ("backward", &after.ir, &before),
    ] {
        if let Err(error) = decodes_agree(left, right, &Allowances::default(), Mode::Same) {
            findings.push(format!("the {label} comparison disagrees: {error}"));
        }
    }
    findings
}

/// Native record fields that hold a structural name of the target format.
const STRUCTURAL_FIELDS: [&str; 10] = [
    "name",
    "type",
    "type_name",
    "group",
    "key",
    "entity_type",
    "entity_name",
    "schema",
    "member",
    "stream",
];
const PRIVATE_NAMES: [&str; 2] = ["cadmpeg", "cadir"];

/// Check 9 (#246 IV.2): no structural name of a written output names the
/// application. `members` are the output's container member names.
///
/// The structural names read are the re-decode's native name, type, group,
/// key, schema and stream fields and its attribute names; the document
/// `source` (the creating application) is not read.
#[must_use]
pub fn structural_name_findings(document: &Value, members: &[String]) -> Vec<String> {
    let private = |text: &str| {
        let lower = text.to_ascii_lowercase();
        PRIVATE_NAMES.iter().any(|name| lower.contains(name))
    };
    let mut findings: Vec<String> = members
        .iter()
        .filter(|member| private(member))
        .map(|member| format!("container member {member:?} names the application"))
        .collect();
    let native = document
        .get("native")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|namespaces| namespaces.iter())
        .filter_map(|(namespace, arenas)| Some((namespace, arenas.as_object()?)));
    for (namespace, arenas) in native {
        for (arena, rows) in arenas {
            for row in rows.as_array().into_iter().flatten() {
                for field in STRUCTURAL_FIELDS {
                    if let Some(text) = row
                        .get(field)
                        .and_then(Value::as_str)
                        .filter(|text| private(text))
                    {
                        findings.push(format!(
                            "native.{namespace}.{arena} {field} {text:?} names the application"
                        ));
                    }
                }
            }
        }
    }
    for attribute in document
        .pointer("/model/attributes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(name) = attribute
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| private(name))
        {
            findings.push(format!("attribute name {name:?} names the application"));
        }
    }
    findings
}

/// Record keys §5.1(c) does not edit: ordinals, identity, and the paths §5.2
/// projects out before pairing.
const EDIT_SKIPPED: [&str; 14] = [
    "id",
    "ordinal",
    "native_ref",
    "source_object",
    "source_entity_id",
    "source_tag",
    "source_text",
    "source_content",
    "source_properties",
    "properties",
    "schema",
    "object_type",
    "runtime_type",
    "object",
];

/// For each neutral arena, the first record whose `native_ref` names a record
/// of the `namespace` native namespace, as (arena, index).
#[must_use]
pub fn edit_targets(document: &Value, namespace: &str) -> Vec<(String, usize)> {
    let native: BTreeSet<&str> = document
        .pointer(&format!("/native/{namespace}"))
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|arenas| arenas.values())
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|row| row.get("id")?.as_str())
        .collect();
    document
        .get("model")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(arena, rows)| {
            let index = rows.as_array()?.iter().position(|row| {
                row.get("native_ref")
                    .and_then(Value::as_str)
                    .is_some_and(|native_ref| native.contains(native_ref))
            })?;
            Some((arena.clone(), index))
        })
        .collect()
}

/// The §5.1(c) edits of one record: each scalar leaf as a JSON pointer below
/// the record and its edited value. A number gains 1, a string gains "x" and a
/// boolean is negated.
#[must_use]
pub fn scalar_edits(record: &Value, arena: &str) -> Vec<(String, Value)> {
    fn walk(
        value: &Value,
        pointer: &mut String,
        top: bool,
        arena: &str,
        edits: &mut Vec<(String, Value)>,
    ) {
        match value {
            Value::Object(fields) => {
                for (key, child) in fields {
                    if top
                        && (EDIT_SKIPPED.contains(&key.as_str())
                            || (arena == "drawings" && key == "parameters"))
                    {
                        continue;
                    }
                    let length = pointer.len();
                    pointer.push('/');
                    pointer.push_str(&key.replace('~', "~0").replace('/', "~1"));
                    walk(child, pointer, false, arena, edits);
                    pointer.truncate(length);
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    let length = pointer.len();
                    pointer.push('/');
                    pointer.push_str(&index.to_string());
                    walk(child, pointer, false, arena, edits);
                    pointer.truncate(length);
                }
            }
            Value::Number(number) => {
                let edited = number
                    .as_i64()
                    .and_then(|value| value.checked_add(1).map(Value::from))
                    .or_else(|| {
                        number
                            .as_u64()
                            .and_then(|value| value.checked_add(1).map(Value::from))
                    })
                    .or_else(|| number.as_f64().map(|value| Value::from(value + 1.0)));
                if let Some(edited) = edited {
                    edits.push((pointer.clone(), edited));
                }
            }
            Value::String(text) => edits.push((pointer.clone(), Value::String(format!("{text}x")))),
            Value::Bool(flag) => edits.push((pointer.clone(), Value::Bool(!flag))),
            Value::Null => {}
        }
    }
    let mut edits = Vec::new();
    walk(record, &mut String::new(), true, arena, &mut edits);
    edits
}

/// §5.1(c): every edit of the target records reaches the written output.
///
/// Arenas in `covered` are skipped; each arena this input edits is added, so
/// a caller walking goldens in path order edits the first golden of each.
/// An edit the document model or `validate_neutral` rejects with an Error is
/// skipped. Each other edit is written with and without fidelity as
/// `fidelity` lists, re-decoded, and compared in `Same` mode with the export
/// allowances of the edited document.
#[must_use]
pub fn edit_findings(
    first: &DecodeResult,
    encoder: &dyn Encoder,
    redecode: &Redecode<'_>,
    namespace: &str,
    fidelity: &[bool],
    covered: &mut BTreeSet<String>,
) -> Vec<String> {
    let Ok(document) = serde_json::to_value(first.ir()) else {
        return vec!["the first decode does not serialize".to_owned()];
    };
    let (request, _) = same_request(encoder, first);
    let mut findings = Vec::new();
    for (arena, index) in edit_targets(&document, namespace) {
        if !covered.insert(arena.clone()) {
            continue;
        }
        let record = &document["model"][arena.as_str()][index];
        for (pointer, value) in scalar_edits(record, &arena) {
            let mut edited = document.clone();
            let at = format!("/model/{arena}/{index}{pointer}");
            let Some(slot) = edited.pointer_mut(&at) else {
                continue;
            };
            *slot = value;
            let Ok(ir) = cadmpeg_ir::CadIr::from_json(&edited.to_string()) else {
                continue;
            };
            let valid = cadmpeg_ir::validate_neutral(&ir, Vec::new())
                .is_ok_and(|report| report.error_count() == 0);
            if !valid {
                continue;
            }
            let Ok(expected) = serde_json::to_value(&ir) else {
                continue;
            };
            for with in fidelity {
                let input = EncodeInput::new(&ir, with.then(|| first.source_fidelity()));
                let written = encoder
                    .plan(input, request)
                    .map_err(|error| format!("plan refused: {error}"))
                    .and_then(|plan| {
                        let mut bytes = Vec::new();
                        plan.write_to(&mut bytes)
                            .map(|_| bytes)
                            .map_err(|error| format!("write refused: {error}"))
                    });
                let outcome = written
                    .and_then(|bytes| redecode(&bytes))
                    .and_then(|after| {
                        Allowances::export(&expected).and_then(|allowances| {
                            decodes_agree(&expected, &after.ir, &allowances, Mode::Same)
                        })
                    });
                if let Err(error) = outcome {
                    findings.push(format!(
                        "edit {at} (fidelity {with}) does not reach the output: {error}"
                    ));
                }
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests;
