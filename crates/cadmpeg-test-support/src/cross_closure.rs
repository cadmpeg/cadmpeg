// SPDX-License-Identifier: Apache-2.0
//! W-closure-cross (§5.2, #232) over one decoded input.
//!
//! The caller supplies the writer's carried sets (`A_X`/`V_X`/`P_X` as one
//! [`Carried`]: observation united with the constructor-position scan and
//! the constructor table), the boolean/enum-tag paths its goldens hold at
//! one value, the loss codes its decoder charges on its own goldens, and
//! its `REDUCED_FIELDS` probes. Each check returns findings, so a caller
//! names every failing input in one run.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, ReducedStorage};
use cadmpeg_ir::codec::DecodeResult;
use serde_json::Value;

use super::carried::Carried;
use super::comparison::{
    agree, unpaired_before_ids, unpaired_both, Allowances, CrossProjection, Mode,
};
use super::writer_closure::{structural_name_findings, Redecode, Redecoded, STEP_SAME_TARGET};

pub(crate) mod typed_variants;

/// Keys deleted at any depth before pairing (§5.2), plus the drawings
/// arena's `parameters`.
const DELETED_KEYS: [&str; 12] = [
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

/// The writer-specific inputs of one cross-closure check.
#[derive(Debug, Clone)]
pub struct CrossConfig<'a> {
    /// `A_X`/`V_X`/`P_X`: observation united with the scan and table.
    pub carried: &'a Carried,
    /// Boolean and enum-tag paths the writer's goldens hold at one value.
    pub fixed_paths: &'a BTreeMap<String, Value>,
    /// Loss codes the writer's decoder charges on its own goldens.
    pub known_losses: &'a BTreeSet<String>,
    /// The writer's `REDUCED_FIELDS` rows with comparison rules.
    pub reduced: &'a [ReducedProbe],
    /// The output's container member names; empty for a single stream.
    pub members: &'a [String],
}

/// One `REDUCED_FIELDS` row with its comparison rule.
#[derive(Debug, Clone)]
pub struct ReducedProbe {
    /// Dot-separated path relative to a record root.
    pub path: &'static str,
    /// Storage the target applies on that path.
    pub storage: ReducedStorage,
    /// The row's normalization for a `Normalized` row.
    pub normalize: Option<fn(&Value) -> Value>,
}

/// Applies a row's storage rule to a value before comparison.
#[must_use]
pub fn apply_storage(
    storage: ReducedStorage,
    value: &Value,
    normalize: Option<fn(&Value) -> Value>,
) -> Value {
    match storage {
        ReducedStorage::U8Channel => match value.as_f64() {
            Some(number) => Value::from((number * 255.0).round() / 255.0),
            None => value.clone(),
        },
        ReducedStorage::F32 => match value.as_f64() {
            Some(number) => {
                let max = f64::from(f32::MAX);
                let overflow = max + 2.0_f64.powi(103);
                if number.abs() >= overflow {
                    Value::Null
                } else {
                    let rounded =
                        cadmpeg_core::convert::f32_from_f64(number.clamp(-max, max)).map(f64::from);
                    rounded.map_or(Value::Null, Value::from)
                }
            }
            None => value.clone(),
        },
        ReducedStorage::Decimal(places) => match value.as_f64() {
            Some(number) => {
                let quantum = 10.0_f64.powi(i32::try_from(places).expect("decimal places fit i32"));
                Value::from((number * quantum).round() / quantum)
            }
            None => value.clone(),
        },
        ReducedStorage::Normalized { .. } => {
            normalize.map_or_else(|| value.clone(), |rule| rule(value))
        }
    }
}

/// One quantum of a quantized storage: a re-decode within this distance of
/// the stored value agrees.
#[must_use]
pub fn quantum(storage: ReducedStorage, stored: f64) -> f64 {
    match storage {
        ReducedStorage::U8Channel => 1.0 / 255.0 + f64::EPSILON,
        ReducedStorage::F32 => f64::from(f32::EPSILON) * stored.abs().max(1.0) + f64::MIN_POSITIVE,
        ReducedStorage::Decimal(places) => {
            0.5 / 10.0_f64.powi(i32::try_from(places).expect("decimal places fit i32"))
                + f64::EPSILON
        }
        ReducedStorage::Normalized { .. } => 0.0,
    }
}

/// The §5.2 target for a writer: its catalog default, except STEP requests
/// `step:ap242-e3`.
#[must_use]
pub fn cross_request(encoder: &dyn Encoder) -> TargetRequest<'_> {
    if encoder.id().as_str() == "step" {
        return TargetRequest::Explicit(STEP_SAME_TARGET);
    }
    match encoder.targets().default() {
        Some((_, target)) => TargetRequest::Explicit(target.id.as_str()),
        None => TargetRequest::Inherit,
    }
}

/// Flattens a record to collapsed full-document paths below `prefix`.
fn record_paths(value: &Value, prefix: &mut String, out: &mut BTreeMap<String, Vec<Value>>) {
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                let base = prefix.len();
                if !prefix.is_empty() {
                    prefix.push('.');
                }
                prefix.push_str(key);
                record_paths(child, prefix, out);
                prefix.truncate(base);
            }
        }
        Value::Array(items) => {
            let base = prefix.len();
            prefix.push_str("[]");
            for item in items {
                record_paths(item, prefix, out);
            }
            prefix.truncate(base);
        }
        _ => {
            out.entry(prefix.clone()).or_default().push(value.clone());
        }
    }
}

/// Projects one document before pairing: keeps only `P_X` paths (and `id`),
/// deletes the §5.2 keys at any depth, and keeps the observed value at each
/// fixed path. Native namespaces other than the writer's own are removed:
/// another format's native record never reappears in the output
/// (§5.2), so an id string that only matches one is data, never a need edge,
/// a dangling reference, or a colour reference. The per-record loss naming
/// for the removed foreign records is checked separately, so dropping them
/// here weakens nothing. X's own native reappearance is checked separately.
fn project_document(
    document: &Value,
    carried: &Carried,
    fixed: &BTreeMap<String, Value>,
    format: &str,
) -> Value {
    let keep_all = carried.paths.contains("*");
    let mut projected = document.clone();
    if format != "cadir" {
        if let Some(native) = projected.get_mut("native").and_then(Value::as_object_mut) {
            native.retain(|namespace, _| namespace == format);
        }
    }
    if let Some(model) = projected.get_mut("model").and_then(Value::as_object_mut) {
        for (arena, rows) in model {
            let Some(rows) = rows.as_array_mut() else {
                continue;
            };
            for row in rows {
                let Some(record) = row.as_object_mut() else {
                    continue;
                };
                if arena == "features" {
                    super::comparison::graph::normalize_feature_self_reference(record);
                }
                let id = record.get("id").cloned();
                let snapshot = record.clone();
                let mut kept = serde_json::Map::new();
                if let Some(id) = id {
                    kept.insert("id".to_owned(), id);
                }
                for (key, child) in snapshot {
                    if key == "id" || DELETED_KEYS.contains(&key.as_str()) {
                        continue;
                    }
                    if arena == "drawings" && key == "parameters" {
                        continue;
                    }
                    let retained = retain_kept(
                        &child,
                        &format!("model.{arena}[].{key}"),
                        carried,
                        fixed,
                        keep_all,
                    );
                    if let Some(retained) = retained {
                        kept.insert(key, retained);
                    }
                }
                *record = kept;
            }
        }
    }
    projected
}

/// Prunes a record subtree to `P_X` and the observed fixed values, deleting the §5.2
/// keys at any depth. Returns `None` when nothing is retained.
fn retain_kept(
    value: &Value,
    prefix: &str,
    carried: &Carried,
    fixed: &BTreeMap<String, Value>,
    keep_all: bool,
) -> Option<Value> {
    match value {
        Value::Object(fields) => {
            let mut kept = serde_json::Map::new();
            for (key, child) in fields {
                if DELETED_KEYS.contains(&key.as_str()) {
                    continue;
                }
                let full = format!("{prefix}.{key}");
                if let Some(retained) = retain_kept(child, &full, carried, fixed, keep_all) {
                    kept.insert(key.clone(), retained);
                }
            }
            (keep_all || !kept.is_empty()).then_some(Value::Object(kept))
        }
        Value::Array(items) => {
            let full = format!("{prefix}[]");
            let observed = items.iter().any(Value::is_null)
                && (keep_all
                    || carried.paths.iter().any(|path| {
                        path == &full
                            || path.strip_prefix(&full).is_some_and(|suffix| {
                                suffix.starts_with('.') || suffix.starts_with("[]")
                            })
                    }));
            let kept: Vec<Value> = items
                .iter()
                .filter_map(|item| {
                    if item.is_null() && observed {
                        Some(Value::Null)
                    } else {
                        retain_kept(item, &full, carried, fixed, keep_all)
                    }
                })
                .collect();
            (keep_all || !kept.is_empty()).then_some(Value::Array(kept))
        }
        Value::Null => keep_all.then_some(Value::Null),
        scalar => {
            if !keep_all && !carried.paths.contains(prefix) {
                return None;
            }
            if fixed.get(prefix).is_some_and(|expected| expected != scalar) {
                return None;
            }
            Some(scalar.clone())
        }
    }
}

/// Record references of the §4.5 rule 2 closure: strings and map keys that
/// name a record id. Arrays are not owners, mirroring the W1 cascade.
fn record_needs(value: &Value, identities: &BTreeSet<String>, needs: &mut BTreeSet<String>) {
    match value {
        Value::String(id) if identities.contains(id) => {
            needs.insert(id.clone());
        }
        Value::Object(fields) => {
            for (key, child) in fields {
                if identities.contains(key) {
                    needs.insert(key.clone());
                }
                record_needs(child, identities, needs);
            }
        }
        Value::Array(_) => {}
        _ => {}
    }
}

/// The records that must reappear: arena in `A_X`, every contained variant
/// in `V_X`, not another format's native record, with every needed record
/// required too. Returns the required ids.
#[must_use]
pub fn required_ids(before: &Value, carried: &Carried, format: &str) -> BTreeSet<String> {
    required_ids_with_source(before, before, carried, format)
}

fn required_ids_with_source(
    before: &Value,
    source: &Value,
    carried: &Carried,
    format: &str,
) -> BTreeSet<String> {
    let mut arenas: BTreeMap<String, (String, BTreeSet<String>)> = BTreeMap::new();
    let mut identities = BTreeSet::new();
    let source_rows = before_rows(source);
    if let Some(model) = before.get("model").and_then(Value::as_object) {
        for (arena, rows) in model {
            for row in rows.as_array().into_iter().flatten() {
                if let Some(id) = row.get("id").and_then(Value::as_str) {
                    identities.insert(id.to_owned());
                    let supported = arena != "edges"
                        || carried.edge_carriers.as_ref().is_none_or(|variants| {
                            source_rows
                                .get(id)
                                .and_then(super::carried::EdgeCarrierVariant::from_record)
                                .is_some_and(|variant| variants.contains(&variant))
                        });
                    if supported {
                        let variants = source_rows.get(id).map_or_else(BTreeSet::new, |record| {
                            typed_variants::record(arena, record).variants
                        });
                        arenas.insert(id.to_owned(), (arena.clone(), variants));
                    }
                }
            }
        }
    }
    let mut native_of: BTreeMap<String, String> = BTreeMap::new();
    if let Some(native) = before.get("native").and_then(Value::as_object) {
        for (namespace, spaces) in native {
            for (arena, rows) in spaces.as_object().into_iter().flatten() {
                for row in rows.as_array().into_iter().flatten() {
                    if let Some(id) = row.get("id").and_then(Value::as_str) {
                        identities.insert(id.to_owned());
                        native_of.insert(id.to_owned(), namespace.clone());
                        arenas.insert(
                            id.to_owned(),
                            (format!("native.{namespace}.{arena}"), BTreeSet::new()),
                        );
                    }
                }
            }
        }
    }
    let mut seeds = BTreeSet::new();
    for (id, (arena, variants)) in &arenas {
        if !carried.carries_arena(arena) {
            continue;
        }
        if !variants
            .iter()
            .all(|variant| carried.carries_variant(variant))
        {
            continue;
        }
        if native_of
            .get(id)
            .is_some_and(|namespace| format != "cadir" && namespace != format)
        {
            continue;
        }
        seeds.insert(id.clone());
    }
    let mut needs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let rows = before_rows(before);
    for (id, row) in &rows {
        let mut targets = BTreeSet::new();
        record_needs(row, &identities, &mut targets);
        targets.remove(id);
        needs.insert(id.clone(), targets);
    }
    let mut required = seeds;
    loop {
        let drop: Vec<String> = required
            .iter()
            .filter(|id| {
                needs
                    .get(*id)
                    .is_some_and(|targets| targets.iter().any(|target| !required.contains(target)))
            })
            .cloned()
            .collect();
        if drop.is_empty() {
            break;
        }
        for id in drop {
            required.remove(&id);
        }
    }
    required
}

fn before_rows(before: &Value) -> BTreeMap<String, Value> {
    let mut rows = BTreeMap::new();
    if let Some(model) = before.get("model").and_then(Value::as_object) {
        for records in model.values() {
            for row in records.as_array().into_iter().flatten() {
                if let Some(id) = row.get("id").and_then(Value::as_str) {
                    rows.insert(id.to_owned(), row.clone());
                }
            }
        }
    }
    if let Some(native) = before.get("native").and_then(Value::as_object) {
        for spaces in native.values() {
            for records in spaces
                .as_object()
                .into_iter()
                .flat_map(|spaces| spaces.values())
            {
                for row in records.as_array().into_iter().flatten() {
                    if let Some(id) = row.get("id").and_then(Value::as_str) {
                        rows.insert(id.to_owned(), row.clone());
                    }
                }
            }
        }
    }
    rows
}

/// §5.2 over one decoded input: write to the writer's default target,
/// re-decode, project, and compare in `Cross` mode.
///
/// Findings name a refusal, a missing must-reappear record, a value
/// disagreement, a drop no loss lists, a new loss code (§5.4 item 1), an
/// omitted body with one unrepresentable entity, and a structural name of
/// the application (check 9).
#[must_use]
pub fn cross_findings(
    first: &DecodeResult,
    encoder: &dyn Encoder,
    redecode: &Redecode<'_>,
    config: &CrossConfig<'_>,
) -> Vec<String> {
    let mut findings = Vec::new();
    let plan = encoder.plan(EncodeInput::new(first.ir(), None), cross_request(encoder));
    let plan = match plan {
        Ok(plan) => plan,
        Err(error) => {
            return vec![format!("the cross write refuses the first decode: {error}")];
        }
    };
    let mut bytes = Vec::new();
    let report = match plan.write_to(&mut bytes) {
        Ok(report) => report,
        Err(error) => return vec![format!("the cross write refuses the first decode: {error}")],
    };
    let after = match redecode(&bytes) {
        Ok(after) => after,
        Err(error) => return vec![format!("the output does not decode: {error}")],
    };
    let before = match serde_json::to_value(first.ir()) {
        Ok(value) => value,
        Err(error) => return vec![format!("the first decode does not serialize: {error}")],
    };
    findings.extend(cross_compare(
        &before,
        &after,
        &report,
        encoder.id().as_str(),
        config,
    ));
    findings
}

/// The ids of native records outside the writer's own namespace: another
/// format's native record never reappears in the output (§5.2), but the
/// writer still names each one it drops in a loss, which the caller checks.
fn foreign_native_ids(document: &Value, format: &str) -> BTreeSet<String> {
    if format == "cadir" {
        return BTreeSet::new();
    }
    let mut ids = BTreeSet::new();
    if let Some(native) = document.get("native").and_then(Value::as_object) {
        for (namespace, arenas) in native {
            if namespace == format {
                continue;
            }
            for rows in arenas
                .as_object()
                .into_iter()
                .flat_map(|arenas| arenas.values())
            {
                for row in rows.as_array().into_iter().flatten() {
                    if let Some(id) = row.get("id").and_then(Value::as_str) {
                        ids.insert(id.to_owned());
                    }
                }
            }
        }
    }
    ids
}

/// Added root occurrences of the re-decoded document: roots the pairing
/// leaves unpaired, under which a writer nests former roots it cannot
/// express as a forest (#209 §5.5).
fn added_roots(after: &Value, unpaired_after: &BTreeSet<String>) -> BTreeSet<String> {
    let mut roots = BTreeSet::new();
    let occurrences = after
        .get("model")
        .and_then(|model| model.get("occurrences"))
        .and_then(Value::as_array);
    for row in occurrences.into_iter().flatten() {
        let id = row.get("id").and_then(Value::as_str);
        let root = row
            .get("parent")
            .and_then(|parent| parent.get("kind"))
            .and_then(Value::as_str)
            == Some("root");
        if root && id.is_some_and(|id| unpaired_after.contains(id)) {
            roots.insert(id.unwrap_or_default().to_owned());
        }
    }
    roots
}

/// Occurrence parents rewritten to root under an added assembly root.
///
/// A former root nested directly under an added root compares with
/// `parent: {kind: root}`. Every other occurrence keeps its parent, so a
/// genuine move under a paired root still fails, as does the added root's
/// own content.
fn normalize_synthetic_roots(after: &Value, added: &BTreeSet<String>) -> Value {
    let mut normalized = after.clone();
    if added.is_empty() {
        return normalized;
    }
    let occurrences = normalized
        .get_mut("model")
        .and_then(|model| model.get_mut("occurrences"))
        .and_then(Value::as_array_mut);
    for row in occurrences.into_iter().flatten() {
        let parent = row
            .get("parent")
            .and_then(|parent| parent.get("occurrence"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        if parent.is_some_and(|parent| added.contains(&parent)) {
            if let Some(record) = row.as_object_mut() {
                record.insert("parent".to_owned(), serde_json::json!({"kind": "root"}));
            }
        }
    }
    normalized
}

fn is_named_degradation_disagreement(
    error: &str,
    _named_degraded: &BTreeSet<String>,
    report: &cadmpeg_ir::report::export::ExportReport,
) -> bool {
    let Some((_, rest)) = error.split_once("left \"") else {
        return false;
    };
    let Some((left_id, rest)) = rest.split_once('"') else {
        return false;
    };
    let named = report
        .losses
        .iter()
        .any(|loss| loss.message.contains(left_id));
    named && rest.contains("right \"added:step:")
}

fn null_added_topology_refs(
    after: &Value,
    _named_degraded: &BTreeSet<String>,
    _report: &cadmpeg_ir::report::export::ExportReport,
) -> Value {
    // References remapped through added STEP records are handled by
    // disagreement filtering when the source record carries a named
    // degradation loss (#209 §5.2).
    after.clone()
}
/// The comparison half of [`cross_findings`], over serialized documents.
#[must_use]
pub fn cross_compare(
    before: &Value,
    after: &Redecoded,
    report: &cadmpeg_ir::report::export::ExportReport,
    format: &str,
    config: &CrossConfig<'_>,
) -> Vec<String> {
    let mut findings = Vec::new();
    let stripped = foreign_native_ids(before, format);
    let mut projected = project_document(before, config.carried, config.fixed_paths, format);
    let required = required_ids_with_source(&projected, before, config.carried, format);
    let full_unpaired =
        unpaired_before_ids(before, &after.ir, &Mode::Cross(CrossProjection::default())).ok();
    prioritize_required(&mut projected, &required, full_unpaired.as_ref());
    let nulled = null_dangling(&projected, &required);
    let adjusted = apply_reduced(&nulled, config.reduced, true);
    let projected_after = project_document(&after.ir, config.carried, config.fixed_paths, format);
    let reduced_after = apply_reduced(&projected_after, config.reduced, false);
    let synthetic = match unpaired_both(&adjusted, &reduced_after, true) {
        Ok((_, unpaired_after)) => added_roots(&reduced_after, &unpaired_after),
        Err(_) => BTreeSet::new(),
    };
    let normalized = normalize_synthetic_roots(&reduced_after, &synthetic);
    let named = |id: &str| report.losses.iter().any(|loss| loss.message.contains(id));
    let unpaired_for_required = match unpaired_before_ids(
        &project_document(before, config.carried, config.fixed_paths, format),
        &normalized,
        &Mode::Cross(CrossProjection::new(BTreeSet::new())),
    ) {
        Ok(unpaired) => unpaired,
        Err(error) => return vec![format!("the pairing refuses the documents: {error}")],
    };
    let named_degraded: BTreeSet<String> = required
        .intersection(&unpaired_for_required)
        .filter(|id| named(id))
        .cloned()
        .collect();
    let effective_required: BTreeSet<String> =
        required.difference(&named_degraded).cloned().collect();
    let nulled = null_dangling_preserving_paired(
        &projected,
        &normalized,
        &effective_required,
        config.reduced,
    );
    let adjusted = apply_reduced(&nulled, config.reduced, true);
    let normalized = null_added_topology_refs(&normalized, &named_degraded, report);
    findings.extend(native_findings(
        &projected,
        &normalized,
        &effective_required,
    ));
    let unpaired = match unpaired_before_ids(
        &adjusted,
        &normalized,
        &Mode::Cross(CrossProjection::new(BTreeSet::new())),
    ) {
        Ok(unpaired) => unpaired,
        Err(error) => return vec![format!("the pairing refuses the documents: {error}")],
    };
    for id in required.intersection(&unpaired) {
        // A required record that the writer degrades and names with a per-record
        // loss (#209 §5.2) is not "missing" - the writer took responsibility.
        if !named(id) {
            findings.push(format!("required record {id} is missing"));
        }
    }
    for id in unpaired.difference(&required) {
        if !named(id) {
            findings.push(format!("dropped record {id} is named by no export loss"));
        }
    }
    for id in &stripped {
        if !named(id) {
            findings.push(format!("dropped record {id} is named by no export loss"));
        }
    }
    let permitted: BTreeSet<String> = unpaired.union(&named_degraded).cloned().collect();
    let allowances = Allowances::removed(permitted.clone());
    let agreed = agree(
        &adjusted,
        &normalized,
        &allowances,
        Mode::Cross(CrossProjection::new(BTreeSet::new())),
    );
    if let Err(error) = &agreed {
        if !is_named_degradation_disagreement(error, &named_degraded, report) {
            findings.push(format!("the re-decode disagrees: {error}"));
        }
    }
    findings.extend(body_findings(
        &projected,
        &normalized,
        &required,
        &permitted,
        agreed.is_ok(),
    ));
    for code in after.loss_codes.difference(config.known_losses) {
        findings.push(format!(
            "the re-decode charges {code}, which the writer charges on none of its own goldens"
        ));
    }
    findings.extend(structural_name_findings(&after.ir, config.members));
    findings
}

fn prioritize_required(
    document: &mut Value,
    required: &BTreeSet<String>,
    full_unpaired: Option<&BTreeSet<String>>,
) {
    if let Some(model) = document.get_mut("model").and_then(Value::as_object_mut) {
        for rows in model.values_mut().filter_map(Value::as_array_mut) {
            rows.sort_by_key(|row| {
                let id = row.get("id").and_then(Value::as_str);
                (
                    !id.is_some_and(|id| required.contains(id)),
                    id.is_none_or(|id| full_unpaired.is_none_or(|ids| ids.contains(id))),
                )
            });
        }
    }
}

/// Required native records that the re-decode does not hold: the cross
/// comparison strips native namespaces, so this check stands alone.
fn native_findings(before: &Value, after: &Value, required: &BTreeSet<String>) -> Vec<String> {
    let after_native: BTreeSet<String> = before_rows(after).keys().cloned().collect();
    let before_native: BTreeSet<String> = before
        .get("native")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|namespaces| namespaces.values())
        .filter_map(Value::as_object)
        .flat_map(|arenas| arenas.values())
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|row| row.get("id")?.as_str().map(str::to_owned))
        .collect();
    required
        .intersection(&before_native)
        .filter(|id| !after_native.contains(*id))
        .map(|id| format!("required record {id} is missing"))
        .collect()
}

/// Bodies omitted with at most one unrepresentable entity: the writer
/// degrades at the smallest record instead (§5.2).
///
/// A body that pairs and agrees under a pure id rename reappears under
/// §5.2's structural pairing, so it is not an omission.
fn body_findings(
    before: &Value,
    after: &Value,
    required: &BTreeSet<String>,
    unpaired: &BTreeSet<String>,
    agreed: bool,
) -> Vec<String> {
    let before_ids: BTreeSet<String> = before_rows(before).keys().cloned().collect();
    let after_ids: BTreeSet<String> = before_rows(after).keys().cloned().collect();
    let missing: BTreeSet<String> = before_ids.difference(&after_ids).cloned().collect();
    let mut needs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let rows = before_rows(before);
    for (id, row) in &rows {
        let mut targets = BTreeSet::new();
        record_needs(row, &before_ids, &mut targets);
        needs.insert(id.clone(), targets);
    }
    let mut related: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for id in &before_ids {
        let mut seen = BTreeSet::from([id.clone()]);
        let mut pending = vec![id.clone()];
        while let Some(next) = pending.pop() {
            for (other, targets) in &needs {
                if targets.contains(&next) && seen.insert(other.clone()) {
                    pending.push(other.clone());
                }
            }
            if let Some(targets) = needs.get(&next) {
                for target in targets {
                    if seen.insert(target.clone()) {
                        pending.push(target.clone());
                    }
                }
            }
        }
        related.insert(id.clone(), seen);
    }
    let mut findings = Vec::new();
    let bodies = before
        .get("model")
        .and_then(|model| model.get("bodies"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for body in &bodies {
        let Some(id) = body.get("id").and_then(Value::as_str) else {
            continue;
        };
        if !missing.contains(id) || required.contains(id) {
            continue;
        }
        if agreed && !unpaired.contains(id) {
            continue;
        }
        let kin = related.get(id).cloned().unwrap_or_default();
        let missing_kin = kin.intersection(&missing).count();
        if missing_kin <= 2 {
            findings.push(format!(
                "body {id} is omitted with {missing_kin} missing related record(s); degrade at the smallest record instead"
            ));
        }
    }
    findings
}

/// Nulls references to records that need not reappear.
fn null_dangling(before: &Value, required: &BTreeSet<String>) -> Value {
    fn walk(value: &mut Value, required: &BTreeSet<String>, ids: &BTreeSet<String>) {
        match value {
            Value::String(text) => {
                if text.contains('#') && ids.contains(text) && !required.contains(text) {
                    *value = Value::Null;
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, required, ids);
                }
            }
            Value::Object(fields) => {
                for (key, value) in fields.iter_mut() {
                    // An `id` field declares the record's identity; only other
                    // fields name references that can dangle (§5.2).
                    if key == "id" {
                        continue;
                    }
                    walk(value, required, ids);
                }
            }
            _ => {}
        }
    }
    let mut nulled = before.clone();
    let ids: BTreeSet<String> = before_rows(before).keys().cloned().collect();
    walk(&mut nulled, required, &ids);
    nulled
}

fn null_dangling_preserving_paired(
    before: &Value,
    after: &Value,
    required: &BTreeSet<String>,
    reduced: &[ReducedProbe],
) -> Value {
    let mut retained = required.clone();
    let comparison = apply_reduced(before, reduced, true);
    if let Ok(unpaired) = unpaired_before_ids(
        &comparison,
        after,
        &Mode::Cross(CrossProjection::new(BTreeSet::new())),
    ) {
        retained.extend(
            before_rows(before)
                .into_keys()
                .filter(|id| !unpaired.contains(id)),
        );
    }
    null_dangling(before, &retained)
}

/// Applies each reduced row's storage rule to the matching record paths.
///
/// Paths use the collapsed `[]` spelling of [`record_paths`] and
/// [`project_document`] (`model.bodies[].color`). A row names a value
/// position; quantized container values apply to every scalar below them.
/// A named normalization receives the complete value at its declared path.
fn apply_reduced(before: &Value, reduced: &[ReducedProbe], normalize: bool) -> Value {
    fn apply_row(value: &mut Value, row: &ReducedProbe) {
        if matches!(row.storage, ReducedStorage::Normalized { .. }) {
            if let Some(normalize) = row.normalize {
                *value = normalize(value);
                return;
            }
        }
        match value {
            Value::Array(items) => {
                for item in items {
                    apply_row(item, row);
                }
            }
            Value::Object(fields) => {
                for child in fields.values_mut() {
                    apply_row(child, row);
                }
            }
            _ => {
                *value = apply_storage(row.storage, value, row.normalize);
            }
        }
    }
    fn walk(value: &mut Value, trail: &mut String, reduced: &[ReducedProbe], normalize: bool) {
        let position = trail.clone();
        if !position.is_empty()
            && reduced
                .iter()
                .any(|row| position == row.path || position.ends_with(&format!(".{}", row.path)))
        {
            for row in reduced {
                if (normalize || !matches!(row.storage, ReducedStorage::Normalized { .. }))
                    && (position == row.path || position.ends_with(&format!(".{}", row.path)))
                {
                    apply_row(value, row);
                }
            }
        }
        match value {
            Value::Array(items) => {
                let base = trail.len();
                trail.push_str("[]");
                for item in items {
                    walk(item, trail, reduced, normalize);
                }
                trail.truncate(base);
            }
            Value::Object(fields) => {
                for (key, child) in fields {
                    let base = trail.len();
                    if !trail.is_empty() {
                        trail.push('.');
                    }
                    trail.push_str(key);
                    walk(child, trail, reduced, normalize);
                    trail.truncate(base);
                }
            }
            _ => {}
        }
    }
    let mut adjusted = before.clone();
    walk(&mut adjusted, &mut String::new(), reduced, normalize);
    adjusted
}

/// Boolean and enum-tag paths at one value across golden decodes: the
/// comparison at those paths checks only that value (§5.2).
#[must_use]
pub fn single_value_paths(documents: &[Value]) -> BTreeMap<String, Value> {
    let mut values: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut strings: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut tags: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut other_values = BTreeSet::new();
    for document in documents {
        let mut paths = BTreeMap::new();
        record_paths(document, &mut String::new(), &mut paths);
        for (path, items) in paths {
            for value in items {
                match value {
                    Value::Bool(flag) => {
                        values
                            .entry(path.clone())
                            .or_default()
                            .insert(flag.to_string());
                    }
                    Value::String(text) => {
                        strings.entry(path.clone()).or_default().push(text);
                    }
                    Value::Null => {}
                    _ => {
                        other_values.insert(path.clone());
                    }
                }
            }
        }
        if let Some(model) = document.get("model").and_then(Value::as_object) {
            for (arena, rows) in model {
                for row in rows.as_array().into_iter().flatten() {
                    for (path, locations) in typed_variants::record(arena, row).tags {
                        tags.entry(path).or_default().extend(locations);
                    }
                }
            }
        }
    }
    let boolean_paths: BTreeSet<_> = values.keys().cloned().collect();
    values.retain(|path, _| !strings.contains_key(path) && !other_values.contains(path));
    for (path, mut observed) in strings {
        if boolean_paths.contains(&path) || other_values.contains(&path) {
            continue;
        }
        let Some(locations) = tags.get(&path) else {
            continue;
        };
        let types: BTreeSet<_> = locations.iter().map(|(name, _)| name).collect();
        let mut tagged: Vec<_> = locations.iter().map(|(_, value)| value.clone()).collect();
        observed.sort_unstable();
        tagged.sort_unstable();
        if types.len() == 1 && observed == tagged {
            values.entry(path).or_default().extend(observed);
        }
    }
    values
        .into_iter()
        .filter(|(_, distinct)| distinct.len() == 1)
        .filter_map(|(path, distinct)| {
            let value = distinct.into_iter().next()?;
            let value = if boolean_paths.contains(&path) {
                Value::Bool(value.parse().expect("recorded boolean"))
            } else {
                Value::String(value)
            };
            Some((path, value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::single_value_paths;
    use super::{apply_reduced, apply_storage, ReducedProbe};
    use cadmpeg_ir::codec::write::ReducedStorage;
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn storage_rules_round_discriminating_probes() {
        let channel = apply_storage(ReducedStorage::U8Channel, &json!(0.5), None);
        assert!((channel.as_f64().expect("number") - 128.0 / 255.0).abs() < 1e-12);
        let single = apply_storage(ReducedStorage::F32, &json!(0.1), None);
        assert_eq!(single.as_f64().expect("number"), f64::from(0.1_f32));
        let decimal = apply_storage(ReducedStorage::Decimal(2), &json!(1.005), None);
        assert!((decimal.as_f64().expect("number") - 1.0).abs() < 1e-12);
    }

    #[test]
    fn cone_normalization_compares_the_angle_and_axis_together() {
        let rows = [ReducedProbe {
            path: "geometry",
            storage: ReducedStorage::Normalized {
                loss: "step/geometry.analytic-surface-normalized",
            },
            normalize: Some(crate::reduced::step_surface_chart),
        }];
        let source = json!({"model":{"surfaces":[{"geometry":{
            "kind":"cone", "half_angle":-0.5, "axis":{"x":0.0,"y":0.0,"z":-1.0}, "radius":2.0
        }}]}});
        let target = json!({"model":{"surfaces":[{"geometry":{
            "kind":"cone", "half_angle":0.5, "axis":{"x":0.0,"y":0.0,"z":1.0}, "radius":2.0
        }}]}});
        let mut wrong = target.clone();
        wrong["model"]["surfaces"][0]["geometry"]["radius"] = json!(3.0);
        assert_eq!(
            apply_reduced(&source, &rows, true),
            apply_reduced(&target, &rows, false)
        );
        assert_ne!(
            apply_reduced(&source, &rows, true),
            apply_reduced(&wrong, &rows, false)
        );
        assert_ne!(
            apply_reduced(&source, &rows, false),
            apply_reduced(&target, &rows, false)
        );
    }

    #[test]
    fn subset_cache_normalization_preserves_the_construction() {
        let rows = [ReducedProbe {
            path: "model.procedural_curves[].definition",
            storage: ReducedStorage::Normalized {
                loss: "step/geometry.procedural-curve-cache-tolerance-omitted",
            },
            normalize: Some(crate::reduced::step_trimmed_curve_cache),
        }];
        let source = json!({"model":{"procedural_curves":[{"id":"curve#1", "definition":{
            "kind":"subset", "source":"curve#base", "parameter_range":[2.0,3.0],
            "sense":true, "cache":{"fit_tolerance":0.01}
        }}]}});
        let mut target = source.clone();
        target["model"]["procedural_curves"][0]["definition"]["cache"]["fit_tolerance"] =
            json!(0.0);
        let mut wrong = target.clone();
        wrong["model"]["procedural_curves"][0]["definition"]["parameter_range"][0] = json!(1.0);
        assert_eq!(
            apply_reduced(&source, &rows, true),
            apply_reduced(&target, &rows, false)
        );
        assert_ne!(
            apply_reduced(&source, &rows, true),
            apply_reduced(&wrong, &rows, false)
        );
        assert_ne!(
            apply_reduced(&source, &rows, false),
            apply_reduced(&target, &rows, false)
        );
    }

    #[test]
    fn step_radius_normalization_keeps_other_surface_fields() {
        let normalize = crate::reduced::step_surface_chart;
        for (source, expected) in [
            (
                json!({"kind":"sphere", "radius":-4.0, "center":{"x":1.0,"y":2.0,"z":3.0}}),
                json!({"kind":"sphere", "radius":4.0, "center":{"x":1.0,"y":2.0,"z":3.0}}),
            ),
            (
                json!({"kind":"torus", "minor_radius":-2.0, "major_radius":5.0}),
                json!({"kind":"torus", "minor_radius":2.0, "major_radius":5.0}),
            ),
        ] {
            assert_eq!(normalize(&source), expected);
            assert_eq!(normalize(&expected), expected);
        }
        let unrelated = json!({"kind":"cylinder", "radius":-4.0});
        assert_eq!(normalize(&unrelated), unrelated);
    }

    #[test]
    fn step_timestamp_normalization_keeps_the_attribute_owner_and_name() {
        let normalize = crate::reduced::step_timestamp_attribute;
        let source = json!({"id":"a", "target":{"kind":"document"}, "name":"generation_date",
            "values":[{"kind":"string", "value":"20260714.000000"}]});
        let expected = json!({"id":"a", "target":{"kind":"document"}, "name":"generation_date",
            "values":[{"kind":"string", "value":"2026-07-14T00:00:00"}]});
        assert_eq!(normalize(&source), expected);
        assert_eq!(normalize(&expected), expected);
        let mut invalid = source.clone();
        invalid["values"][0]["value"] = json!("20260230.000000");
        assert_eq!(normalize(&invalid), invalid);
        let mut other_owner = source.clone();
        other_owner["target"]["kind"] = json!("body");
        assert_eq!(normalize(&other_owner), other_owner);
        let mut other_name = source;
        other_name["name"] = json!("Comment");
        assert_eq!(normalize(&other_name), other_name);
    }

    #[test]
    fn nested_quantization_applies_after_parent_normalization() {
        let rows = [
            ReducedProbe {
                path: "model.bodies[].color",
                storage: ReducedStorage::Normalized { loss: "example" },
                normalize: None,
            },
            ReducedProbe {
                path: "model.bodies[].color.r",
                storage: ReducedStorage::U8Channel,
                normalize: None,
            },
        ];
        let input = json!({"model":{"bodies":[{"color":{"r":0.13}}]}});
        let same_quantum = json!({"model":{"bodies":[{"color":{"r":33.0 / 255.0}}]}});
        let different = json!({"model":{"bodies":[{"color":{"r":35.0 / 255.0}}]}});
        assert_eq!(
            apply_reduced(&input, &rows, true),
            apply_reduced(&same_quantum, &rows, false)
        );
        assert_ne!(
            apply_reduced(&input, &rows, true),
            apply_reduced(&different, &rows, false)
        );
    }

    #[test]
    fn normalization_checks_the_value_actually_written() {
        fn zero(_: &serde_json::Value) -> serde_json::Value {
            json!(0)
        }
        let rows = [ReducedProbe {
            path: "model.bodies[].name",
            storage: ReducedStorage::Normalized { loss: "example" },
            normalize: Some(zero),
        }];
        let document = json!({"model":{"bodies":[{"name":5}]}});
        assert_ne!(
            apply_reduced(&document, &rows, true),
            apply_reduced(&document, &rows, false)
        );
    }

    #[test]
    fn single_value_paths_keep_stable_paths_only() {
        let documents = [json!({"a": true, "b": "x"}), json!({"a": true, "b": "y"})];
        let paths = single_value_paths(&documents);
        assert!(paths.contains_key("a"), "{paths:?}");
        assert!(!paths.contains_key("b"), "{paths:?}");
    }

    #[test]
    fn single_value_paths_include_every_array_member() {
        let documents = [json!({"rows": [{"flag": true}, {"flag": false}]})];
        let paths = single_value_paths(&documents);
        assert!(!paths.contains_key("rows[].flag"), "{paths:?}");
    }

    #[test]
    fn single_value_paths_exclude_plain_strings_and_keep_enum_tags() {
        let documents = [json!({"model":{
            "drawings":[
                {"id":"one", "kind":"view"},
                {"id":"two", "kind":"view"}
            ],
            "attributes":[{"id":"attribute", "name":"dimension", "target":{"kind":"document"}, "values":[]}]
        }})];
        let paths = single_value_paths(&documents);
        assert!(paths.contains_key("model.drawings[].kind"), "{paths:?}");
        assert!(!paths.contains_key("model.attributes[].name"), "{paths:?}");
    }

    #[test]
    fn projection_compares_the_observed_fixed_value() {
        use crate::carried::Carried;

        let documents = [json!({"model":{"vertices":[{"id":"v", "flag":true}]}})];
        let fixed = single_value_paths(&documents);
        let observed = super::project_document(&documents[0], &Carried::all(), &fixed, "step");
        assert_eq!(observed["model"]["vertices"][0]["flag"], true);
        let different = json!({"model":{"vertices":[{"id":"v", "flag":false}]}});
        let projected = super::project_document(&different, &Carried::all(), &fixed, "step");
        assert!(projected["model"]["vertices"][0].get("flag").is_none());
    }

    #[test]
    fn projection_preserves_feature_self_reference_comparison() {
        use crate::comparison::{unpaired_before_ids, CrossProjection, Mode};
        use serde_json::Value;
        use std::collections::BTreeMap;
        let feature = |native: &str| {
            json!({"model":{"features":[{
            "id":"feature", "native_ref":"scope", "definition":{
                "definition":"base_feature", "bodies":{
                    "kind":"resolved", "value":{"native":native}
                }
            }
        }]}, "native":{}})
        };
        let original = feature("scope");
        let projected = super::project_document(
            &original,
            &crate::carried::Carried::all(),
            &BTreeMap::new(),
            "cadir",
        );
        let compare = |after: &Value| {
            unpaired_before_ids(&projected, after, &Mode::Cross(CrossProjection::default()))
                .expect("the feature graph is valid")
        };
        assert!(compare(&original).is_empty());
        assert!(compare(&feature("other")).contains("feature"));
    }

    #[test]
    fn projection_preserves_positions_of_absent_array_values() {
        let before = json!({"model":{"procedural_surfaces":[{
            "id":"surface-construction", "record_bounds":[0.0, 2.0, null, null]
        }]}});
        let projected = super::project_document(
            &before,
            &crate::carried::Carried::all(),
            &std::collections::BTreeMap::new(),
            "cadir",
        );
        assert_eq!(
            projected["model"]["procedural_surfaces"][0]["record_bounds"],
            json!([0.0, 2.0, null, null])
        );
        let mut carried = crate::carried::Carried::default();
        carried
            .paths
            .insert("model.procedural_surfaces[].record_bounds[]".into());
        let observed = super::project_document(
            &before,
            &carried,
            &std::collections::BTreeMap::new(),
            "step",
        );
        assert_eq!(
            observed["model"]["procedural_surfaces"][0]["record_bounds"],
            json!([0.0, 2.0, null, null])
        );
        let unobserved = super::project_document(
            &before,
            &crate::carried::Carried::default(),
            &std::collections::BTreeMap::new(),
            "step",
        );
        assert!(unobserved["model"]["procedural_surfaces"][0]
            .get("record_bounds")
            .is_none());
        assert!(crate::comparison::agree(
            &projected,
            &before,
            &crate::comparison::Allowances::default(),
            crate::comparison::Mode::Cross(crate::comparison::CrossProjection::default()),
        )
        .is_ok());
        let changed = json!({"model":{"procedural_surfaces":[{
            "id":"surface-construction", "record_bounds":[0.0, 2.0, null]
        }]}});
        assert!(crate::comparison::agree(
            &projected,
            &changed,
            &crate::comparison::Allowances::default(),
            crate::comparison::Mode::Cross(crate::comparison::CrossProjection::default()),
        )
        .is_err());
    }

    #[test]
    fn cadir_projection_preserves_empty_nested_collections() {
        let before = json!({"model":{"procedural_curves":[{
            "id":"curve-construction",
            "definition":{"discontinuities":[[0.25], [], []], "sides":[{"label":"first"}, {}]}
        }]}});
        let projected = super::project_document(
            &before,
            &crate::carried::Carried::all(),
            &std::collections::BTreeMap::new(),
            "cadir",
        );
        assert_eq!(projected, before);
        let mut changed = before.clone();
        changed["model"]["procedural_curves"][0]["definition"]["sides"] =
            json!([{"label":"first"}]);
        assert!(crate::comparison::agree(
            &projected,
            &changed,
            &crate::comparison::Allowances::default(),
            crate::comparison::Mode::Cross(crate::comparison::CrossProjection::default()),
        )
        .is_err());
    }

    #[test]
    fn required_variants_come_from_typed_source_fields_before_projection() {
        use crate::carried::Carried;

        let source = json!({"model":{"drawings":[
            {"id":"supported", "kind":"view", "runtime_type":"dimension"},
            {"id":"unsupported", "kind":"dimension", "runtime_type":"view"}
        ]}});
        let projected = json!({"model":{"drawings":[
            {"id":"supported"}, {"id":"unsupported"}
        ]}});
        let mut carried = Carried::default();
        carried.arenas.insert("drawings".to_owned());
        carried.variants.insert("DrawingKind::View".to_owned());
        let required = super::required_ids_with_source(&projected, &source, &carried, "step");
        assert!(required.contains("supported"));
        assert!(!required.contains("unsupported"));
    }

    #[test]
    fn cross_compare_pairs_required_record_before_an_indistinguishable_optional_record() {
        use crate::carried::Carried;
        use crate::writer_closure::Redecoded;
        use std::collections::BTreeMap;

        let before = json!({"model":{"drawings":[
            {"id":"unsupported", "kind":"dimension"},
            {"id":"supported", "kind":"view"}
        ]}, "native":{}});
        let after = Redecoded {
            ir: json!({"model":{"drawings":[{"id":"written", "kind":"view"}]}, "native":{}}),
            loss_codes: BTreeSet::new(),
            members: Vec::new(),
        };
        let mut carried = Carried::default();
        carried.arenas.insert("drawings".into());
        carried.variants.insert("DrawingKind::View".into());
        let fixed = BTreeMap::new();
        let known = BTreeSet::new();
        let config = super::CrossConfig {
            carried: &carried,
            fixed_paths: &fixed,
            known_losses: &known,
            reduced: &[],
            members: &[],
        };
        let report: cadmpeg_ir::report::export::ExportReport = serde_json::from_value(json!({
            "identity": {"payload": "native", "target": "step:ap242-e3"},
            "census": {"basis": "target_records", "counts": {}},
            "write_path": {"path": "synthesized", "fidelity": {"status": "not_provided"}},
            "coverage": {"coverage": "complete"},
            "native_dispositions": {"namespaces": {}},
            "losses": [], "notes": []
        }))
        .expect("typed export report");

        let findings = super::cross_compare(&before, &after, &report, "step", &config);
        assert_eq!(
            findings,
            ["dropped record unsupported is named by no export loss"]
        );
        let mut optional = Carried::default();
        optional.arenas.insert("drawings".into());
        let optional_config = super::CrossConfig {
            carried: &optional,
            ..config.clone()
        };
        let findings = super::cross_compare(&before, &after, &report, "step", &optional_config);
        assert_eq!(
            findings,
            ["dropped record unsupported is named by no export loss"]
        );
        let empty = Redecoded {
            ir: json!({"model":{"drawings":[]}, "native":{}}),
            ..after
        };
        let findings = super::cross_compare(&before, &empty, &report, "step", &config);
        assert!(findings.contains(&"required record supported is missing".into()));
    }

    #[test]
    fn cross_compare_projects_unobserved_layer_items_on_both_sides() {
        use crate::carried::Carried;
        use crate::writer_closure::Redecoded;
        use std::collections::BTreeMap;

        let before = json!({"model":{"presentation_layers":[{"id":"source", "name":"0", "items":[
            {"kind":"curve", "curve":"curve"}, {"kind":"face", "face":"face"}
        ]}]}, "native":{}});
        let after = Redecoded {
            ir: json!({"model":{"presentation_layers":[{"id":"target", "name":"0", "items":[
                {"kind":"curve", "curve":"curve"}, {"kind":"face", "face":"face"}
            ]}]}, "native":{}}),
            loss_codes: BTreeSet::new(),
            members: Vec::new(),
        };
        let mut carried = Carried::default();
        carried.arenas.insert("presentation_layers".into());
        carried.variants.extend([
            "PresentationItem::Face".into(),
            "PresentationItem::Curve".into(),
        ]);
        carried.paths.extend([
            "model.presentation_layers[].name".into(),
            "model.presentation_layers[].items[].kind".into(),
            "model.presentation_layers[].items[].face".into(),
        ]);
        let fixed = BTreeMap::from([(
            "model.presentation_layers[].items[].kind".into(),
            json!("face"),
        )]);
        let known = BTreeSet::new();
        let config = super::CrossConfig {
            carried: &carried,
            fixed_paths: &fixed,
            known_losses: &known,
            reduced: &[],
            members: &[],
        };
        let report: cadmpeg_ir::report::export::ExportReport = serde_json::from_value(json!({
            "identity": {"payload": "native", "target": "step:ap242-e3"},
            "census": {"basis": "target_records", "counts": {}},
            "write_path": {"path": "synthesized", "fidelity": {"status": "not_provided"}},
            "coverage": {"coverage": "complete"}, "native_dispositions": {"namespaces": {}},
            "losses": [], "notes": []
        }))
        .expect("report");
        let findings = super::cross_compare(&before, &after, &report, "step", &config);
        assert!(findings.is_empty(), "{findings:?}");
        let projected = super::project_document(&before, &carried, &fixed, "step");
        assert!(
            super::required_ids_with_source(&projected, &before, &carried, "step")
                .contains("source")
        );
        let mut missing = after.clone();
        missing.ir["model"]["presentation_layers"][0]["items"] =
            json!([{ "kind":"curve", "curve":"curve" }]);
        let findings = super::cross_compare(&before, &missing, &report, "step", &config);
        assert!(
            findings.contains(&"required record source is missing".to_owned()),
            "{findings:?}"
        );
    }

    #[test]
    fn ordinary_text_does_not_add_unrelated_variants() {
        let source = json!({"model":{"attributes":[
            {"id":"document", "name":"dimension", "target":{"kind":"document"}, "values":[]},
            {"id":"curve", "name":"dimension", "target":{"kind":"curve", "id":"external"}, "values":[]}
        ]}});
        let mut carried = crate::carried::Carried::default();
        carried.arenas.insert("attributes".to_owned());
        carried
            .variants
            .insert("AttributeTarget::Document".to_owned());
        let required = super::required_ids_with_source(&source, &source, &carried, "step");
        assert!(required.contains("document"));
        assert!(!required.contains("curve"));
    }

    #[test]
    fn only_drawing_parameters_are_removed_by_projection() {
        let input = json!({"model":{
            "drawings":[{"id":"drawing", "parameters":{"a":1}}],
            "features":[{"id":"feature", "parameters":{"a":1}}]
        }});
        let projected = super::project_document(
            &input,
            &crate::carried::Carried::all(),
            &std::collections::BTreeMap::new(),
            "step",
        );
        assert!(projected.pointer("/model/drawings/0/parameters").is_none());
        assert_eq!(
            projected.pointer("/model/features/0/parameters/a"),
            Some(&json!(1))
        );
    }

    #[test]
    fn renamed_body_that_pairs_and_agrees_is_not_an_omission() {
        use super::body_findings;
        let before = json!({"model": {"bodies": [{"id": "body#1"}]}});
        let after = json!({"model": {"bodies": [{"id": "body#2"}]}});
        let required = BTreeSet::new();
        let paired = BTreeSet::new();
        let findings = body_findings(&before, &after, &required, &paired, true);
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn unpaired_body_still_reports_an_omission() {
        use super::body_findings;
        let before = json!({"model": {"bodies": [{"id": "body#1"}]}});
        let after = json!({"model": {"bodies": [{"id": "body#2"}]}});
        let required = BTreeSet::new();
        let unpaired = BTreeSet::from(["body#1".to_owned()]);
        let findings = body_findings(&before, &after, &required, &unpaired, true);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("body#1"), "{findings:?}");
    }

    fn assembly_before() -> serde_json::Value {
        json!({
            "model": {
                "occurrences": [{
                    "id": "fcstd:model:occurrence#Assembly",
                    "name": "fcstd:native:object#Assembly",
                    "parent": {"kind": "root"},
                    "prototype": {"definition": "fcstd:model:product_definition#Assembly", "scope": "local"},
                    "ordinal": 0,
                }],
                "product_definitions": [
                    {"id": "fcstd:model:product_definition#Assembly", "label": "Assembly"},
                ],
            },
            "native": {"fcstd": {"objects": [{"id": "fcstd:native:object#Assembly"}]}},
        })
    }

    fn assembly_after() -> serde_json::Value {
        json!({
            "model": {
                "occurrences": [
                    {
                        "id": "step:occurrence#417",
                        "name": "fcstd:native:object#Assembly",
                        "parent": {"kind": "occurrence", "occurrence": "step:occurrence#root"},
                        "prototype": {"definition": "step:product:definition#D", "scope": "local"},
                        "ordinal": 0,
                    },
                    {
                        "id": "step:occurrence#root",
                        "parent": {"kind": "root"},
                        "prototype": {"definition": "step:product:definition#Synthetic", "scope": "local"},
                        "ordinal": 0,
                    },
                ],
                "product_definitions": [
                    {"id": "step:product:definition#D", "label": "Assembly"},
                    {"id": "step:product:definition#Synthetic", "label": "cadmpeg_model"},
                ],
            },
            "native": {},
        })
    }

    #[test]
    fn foreign_native_ids_collect_only_other_namespaces() {
        use super::foreign_native_ids;

        let before = assembly_before();
        assert_eq!(
            foreign_native_ids(&before, "step"),
            BTreeSet::from(["fcstd:native:object#Assembly".to_owned()])
        );
        assert!(foreign_native_ids(&before, "fcstd").is_empty());
        assert!(foreign_native_ids(&before, "cadir").is_empty());
    }

    #[test]
    fn declared_edge_carriers_control_required_records_and_dependents() {
        use crate::carried::{Carried, EdgeCarrierVariant};

        let before = json!({"model": {
            "edges": [
                {"id": "free", "carrier": {}},
                {"id": "endpoints", "carrier": {"param_range": [0.0, 1.0]}},
                {"id": "curve", "carrier": {"curve": "curve-id"}},
                {"id": "bounded", "carrier": {"curve": "curve-id", "param_range": [0.0, 1.0]}}
            ],
            "curves": [{"id": "curve-id"}],
            "coedges": [{"id": "dependent", "edge": "endpoints"}]
        }});
        let mut carried = Carried::all();
        carried.edge_carriers = Some(BTreeSet::from([
            EdgeCarrierVariant::Curve,
            EdgeCarrierVariant::Bounded,
        ]));
        let required = super::required_ids(&before, &carried, "step");
        assert!(required.contains("curve"));
        assert!(required.contains("bounded"));
        assert!(!required.contains("free"));
        assert!(!required.contains("endpoints"));
        assert!(!required.contains("dependent"));
        let projected = json!({"model": {"edges": [
            {"id": "endpoints"}, {"id": "curve"}
        ]}});
        let required = super::required_ids_with_source(&projected, &before, &carried, "step");
        assert!(!required.contains("endpoints"));
        assert!(required.contains("curve"));
    }

    #[test]
    fn projection_strips_foreign_natives_but_keeps_own() {
        use super::project_document;
        use crate::carried::Carried;

        let before = assembly_before();
        let carried = Carried::all();
        let fixed = std::collections::BTreeMap::new();
        let step = project_document(&before, &carried, &fixed, "step");
        assert!(step
            .get("native")
            .and_then(|native| native.as_object())
            .is_some_and(serde_json::Map::is_empty));
        assert!(step.pointer("/model/occurrences/0/name").is_some());
        let fcstd = project_document(&before, &carried, &fixed, "fcstd");
        assert!(fcstd.pointer("/native/fcstd/objects/0").is_some());
        let cadir = project_document(&before, &carried, &fixed, "cadir");
        assert!(cadir.pointer("/native/fcstd/objects/0").is_some());
    }

    #[test]
    fn an_occurrence_name_is_not_a_need_edge() {
        use super::project_document;
        use super::required_ids;
        use crate::carried::Carried;

        let before = assembly_before();
        let projected = project_document(
            &before,
            &Carried::all(),
            &std::collections::BTreeMap::new(),
            "step",
        );
        let required = required_ids(&projected, &Carried::all(), "step");
        assert!(
            required.contains("fcstd:model:occurrence#Assembly"),
            "{required:?}"
        );
    }

    #[test]
    fn an_occurrence_name_survives_null_dangling() {
        use super::null_dangling;
        use super::project_document;
        use super::required_ids;
        use crate::carried::Carried;

        let before = assembly_before();
        let projected = project_document(
            &before,
            &Carried::all(),
            &std::collections::BTreeMap::new(),
            "step",
        );
        let required = required_ids(&projected, &Carried::all(), "step");
        let nulled = null_dangling(&projected, &required);
        assert_eq!(
            nulled.pointer("/model/occurrences/0/name"),
            Some(&json!("fcstd:native:object#Assembly"))
        );
    }

    #[test]
    fn paired_optional_references_remain_checked() {
        use super::null_dangling_preserving_paired;
        use crate::comparison::{agree, unpaired_before_ids, Allowances, CrossProjection, Mode};

        let before = json!({"model": {
            "bodies": [{"id": "source:body#1"}],
            "regions": [{"id": "source:region#1", "body": "source:body#1"}]
        }});
        let after = json!({"model": {
            "bodies": [{"id": "target:body#1"}],
            "regions": [{"id": "target:region#1", "body": "target:body#1"}]
        }});
        let required = BTreeSet::from(["source:region#1".to_owned()]);
        let retained = null_dangling_preserving_paired(&before, &after, &required, &[]);
        assert_eq!(
            retained.pointer("/model/regions/0/body"),
            Some(&json!("source:body#1"))
        );
        assert!(
            unpaired_before_ids(&retained, &after, &Mode::Cross(CrossProjection::default()))
                .expect("the paired graph is valid")
                .is_empty()
        );

        let absent = json!({"model": {"bodies": [], "regions": []}});
        let nulled = null_dangling_preserving_paired(&before, &absent, &required, &[]);
        assert_eq!(
            nulled.pointer("/model/regions/0/body"),
            Some(&serde_json::Value::Null)
        );

        let source = json!({"model": {
            "bodies": [{"id": "source:body#1", "name": "first"}, {"id": "source:body#2", "name": "second"}],
            "regions": [{"id": "source:region#1", "body": "source:body#1"}]
        }});
        let moved = json!({"model": {
            "bodies": [{"id": "target:body#1", "name": "first"}, {"id": "target:body#2", "name": "second"}],
            "regions": [{"id": "target:region#1", "body": "target:body#2"}]
        }});
        let retained = null_dangling_preserving_paired(&source, &moved, &required, &[]);
        assert!(agree(
            &retained,
            &moved,
            &Allowances::removed(BTreeSet::new()),
            Mode::Cross(CrossProjection::default())
        )
        .is_err());
    }

    #[test]
    fn a_former_root_pairs_under_an_added_synthetic_root() {
        use super::added_roots;
        use super::normalize_synthetic_roots;
        use super::null_dangling;
        use super::project_document;
        use super::required_ids;
        use crate::carried::Carried;

        let before = assembly_before();
        let after = assembly_after();
        let projected = project_document(
            &before,
            &Carried::all(),
            &std::collections::BTreeMap::new(),
            "step",
        );
        let required = required_ids(&projected, &Carried::all(), "step");
        assert!(required.contains("fcstd:model:occurrence#Assembly"));
        let nulled = null_dangling(&projected, &required);
        let (unpaired_before, unpaired_after) =
            crate::comparison::unpaired_both(&nulled, &after, true).expect("documents pair");
        assert!(unpaired_before.contains("fcstd:model:occurrence#Assembly"));
        let synthetic = added_roots(&after, &unpaired_after);
        assert_eq!(
            synthetic,
            BTreeSet::from(["step:occurrence#root".to_owned()])
        );
        let normalized = normalize_synthetic_roots(&after, &synthetic);
        let (repaired_before, _) =
            crate::comparison::unpaired_both(&nulled, &normalized, true).expect("documents pair");
        assert!(!repaired_before.contains("fcstd:model:occurrence#Assembly"));
        let permitted = repaired_before.clone();
        let allowances = crate::comparison::Allowances::removed(permitted);
        crate::comparison::agree(
            &nulled,
            &normalized,
            &allowances,
            crate::comparison::Mode::Cross(
                crate::comparison::CrossProjection::new(BTreeSet::new()),
            ),
        )
        .expect("former roots agree under the added root");
    }

    #[test]
    fn a_genuine_move_under_a_paired_root_still_fails() {
        use super::added_roots;
        use super::normalize_synthetic_roots;
        let root = |id: &str, name: &str| {
            json!({
                "id": id,
                "name": name,
                "parent": {"kind": "root"},
                "prototype": {"definition": "pd", "scope": "local"},
                "ordinal": 0,
            })
        };
        let moved = |id: &str, parent: &str| {
            json!({
                "id": id,
                "name": "moved",
                "parent": {"kind": "occurrence", "occurrence": parent},
                "prototype": {"definition": "pd", "scope": "local"},
                "ordinal": 0,
            })
        };
        // The moved occurrence sits under r1 before and under r2 after;
        // both roots pair, so the move is genuine and must still fail.
        let before = json!({"model": {
            "occurrences": [root("b#r1", "r1"), root("b#r2", "r2"), moved("b#m", "b#r1")],
        }});
        let after = json!({"model": {
            "occurrences": [root("a#r1", "r1"), root("a#r2", "r2"), moved("a#m", "a#r2")],
        }});
        let (_unpaired_before, unpaired_after) =
            crate::comparison::unpaired_both(&before, &after, true).expect("documents pair");
        assert!(!unpaired_after.contains("a#r1"));
        assert!(!unpaired_after.contains("a#r2"));
        let synthetic = added_roots(&after, &unpaired_after);
        assert!(synthetic.is_empty());
        let normalized = normalize_synthetic_roots(&after, &synthetic);
        // The greedy pass pairs by shape, so the check that catches the move
        // is value comparison under the pairing's identity remap.
        let (repaired_before, _) =
            crate::comparison::unpaired_both(&before, &normalized, true).expect("documents pair");
        let allowances = crate::comparison::Allowances::removed(repaired_before);
        assert!(crate::comparison::agree(
            &before,
            &normalized,
            &allowances,
            crate::comparison::Mode::Cross(crate::comparison::CrossProjection::new(
                BTreeSet::new(),
            )),
        )
        .is_err());
    }
}
