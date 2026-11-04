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

use super::carried::{json_variants, Carried};
use super::comparison::{
    agree, unpaired_before_ids, unpaired_both, Allowances, CrossProjection, Mode,
};
use super::writer_closure::{structural_name_findings, Redecode, Redecoded, STEP_SAME_TARGET};

/// Keys deleted at any depth before pairing (§5.2), plus the drawings
/// arena's `parameters`.
const DELETED_KEYS: [&str; 13] = [
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
    "parameters",
];

/// The writer-specific inputs of one cross-closure check.
#[derive(Debug, Clone)]
pub struct CrossConfig<'a> {
    /// `A_X`/`V_X`/`P_X`: observation united with the scan and table.
    pub carried: &'a Carried,
    /// Boolean and enum-tag paths the writer's goldens hold at one value.
    pub fixed_paths: &'a BTreeSet<String>,
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
            Some(number) => Value::from(f64::from(number as f32)),
            None => value.clone(),
        },
        ReducedStorage::Decimal(places) => match value.as_f64() {
            Some(number) => {
                let quantum = 10.0_f64.powi(places as i32);
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
        ReducedStorage::F32 => f32::EPSILON as f64 * stored.abs().max(1.0) + f64::MIN_POSITIVE,
        ReducedStorage::Decimal(places) => 0.5 / 10.0_f64.powi(places as i32) + f64::EPSILON,
        ReducedStorage::Normalized { .. } => 0.0,
    }
}

/// The §5.2 target for a writer: its catalog default, except STEP requests
/// `step:ap242-e3`.
#[must_use]
pub fn cross_request<'a>(encoder: &'a dyn Encoder) -> TargetRequest<'a> {
    if encoder.id().as_str() == "step" {
        return TargetRequest::Explicit(STEP_SAME_TARGET);
    }
    match encoder.targets().default() {
        Some((_, target)) => TargetRequest::Explicit(target.id.as_str()),
        None => TargetRequest::Inherit,
    }
}

/// Flattens a record to collapsed full-document paths below `prefix`.
fn record_paths(value: &Value, prefix: &mut String, out: &mut BTreeMap<String, Value>) {
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
            out.insert(prefix.clone(), value.clone());
        }
    }
}

/// Projects one document before pairing: keeps only `P_X` paths (and `id`),
/// deletes the §5.2 keys at any depth, and deletes the caller-supplied
/// single-value paths. Native namespaces other than the writer's own are
/// removed: another format's native record never reappears in the output
/// (§5.2), so an id string that only matches one is data, never a need edge,
/// a dangling reference, or a colour reference. The per-record loss naming
/// for the removed foreign records is checked separately, so dropping them
/// here weakens nothing. X's own native reappearance is checked separately.
fn project_document(
    document: &Value,
    carried: &Carried,
    fixed: &BTreeSet<String>,
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

/// Prunes a record subtree to `P_X` and non-fixed paths, deleting the §5.2
/// keys at any depth. Returns `None` when nothing is retained.
fn retain_kept(
    value: &Value,
    prefix: &str,
    carried: &Carried,
    fixed: &BTreeSet<String>,
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
            (!kept.is_empty()).then_some(Value::Object(kept))
        }
        Value::Array(items) => {
            let full = format!("{prefix}[]");
            let kept: Vec<Value> = items
                .iter()
                .filter_map(|item| retain_kept(item, &full, carried, fixed, keep_all))
                .collect();
            (!kept.is_empty()).then_some(Value::Array(kept))
        }
        Value::Null => None,
        scalar => {
            if !keep_all && !carried.paths.contains(prefix) {
                return None;
            }
            if fixed.contains(prefix) {
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
    let mut arenas: BTreeMap<String, (String, BTreeSet<String>)> = BTreeMap::new();
    let mut identities = BTreeSet::new();
    if let Some(model) = before.get("model").and_then(Value::as_object) {
        for (arena, rows) in model {
            for row in rows.as_array().into_iter().flatten() {
                if let Some(id) = row.get("id").and_then(Value::as_str) {
                    identities.insert(id.to_owned());
                    arenas.insert(id.to_owned(), (arena.clone(), json_variants(row)));
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
    let projected = project_document(before, config.carried, config.fixed_paths, format);
    let required = required_ids(&projected, config.carried, format);
    let nulled = null_dangling(&projected, &required);
    let adjusted = apply_reduced(&nulled, config.reduced);
    let synthetic = match unpaired_both(&adjusted, &after.ir, true) {
        Ok((_, unpaired_after)) => added_roots(&after.ir, &unpaired_after),
        Err(_) => BTreeSet::new(),
    };
    let normalized = normalize_synthetic_roots(&after.ir, &synthetic);
    findings.extend(native_findings(&projected, &normalized, &required));
    let unpaired = match unpaired_before_ids(
        &adjusted,
        &normalized,
        &Mode::Cross(CrossProjection::new(BTreeSet::new())),
    ) {
        Ok(unpaired) => unpaired,
        Err(error) => return vec![format!("the pairing refuses the documents: {error}")],
    };
    for id in required.intersection(&unpaired) {
        findings.push(format!("required record {id} is missing"));
    }
    for id in unpaired.difference(&required) {
        let named = report.losses.iter().any(|loss| loss.message.contains(id));
        if !named {
            findings.push(format!("dropped record {id} is named by no export loss"));
        }
    }
    for id in &stripped {
        let named = report.losses.iter().any(|loss| loss.message.contains(id));
        if !named {
            findings.push(format!("dropped record {id} is named by no export loss"));
        }
    }
    let permitted: BTreeSet<String> = unpaired;
    let allowances = Allowances::removed(permitted.clone());
    let agreed = agree(
        &adjusted,
        &normalized,
        &allowances,
        Mode::Cross(CrossProjection::new(BTreeSet::new())),
    );
    if let Err(error) = &agreed {
        findings.push(format!("the re-decode disagrees: {error}"));
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
    for id in before_ids.iter() {
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

/// Applies each reduced row's storage rule to the matching record paths.
///
/// Paths use the collapsed `[]` spelling of [`record_paths`] and
/// [`project_document`] (`model.bodies[].color`). A row names a value
/// position; when that value is a container, the row's storage applies to
/// every scalar below it, so per-channel numbers quantize while tags and
/// other non-numeric leaves pass through [`apply_storage`] unchanged.
fn apply_reduced(before: &Value, reduced: &[ReducedProbe]) -> Value {
    fn apply_row(value: &mut Value, row: &ReducedProbe) {
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
    fn walk(value: &mut Value, trail: &mut String, reduced: &[ReducedProbe]) {
        let position = trail.clone();
        if !position.is_empty()
            && reduced
                .iter()
                .any(|row| position == row.path || position.ends_with(&format!(".{}", row.path)))
        {
            for row in reduced {
                if position == row.path || position.ends_with(&format!(".{}", row.path)) {
                    apply_row(value, row);
                }
            }
            return;
        }
        match value {
            Value::Array(items) => {
                let base = trail.len();
                trail.push_str("[]");
                for item in items {
                    walk(item, trail, reduced);
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
                    walk(child, trail, reduced);
                    trail.truncate(base);
                }
            }
            _ => {}
        }
    }
    let mut adjusted = before.clone();
    walk(&mut adjusted, &mut String::new(), reduced);
    adjusted
}

/// Boolean and enum-tag paths at one value across golden decodes: the
/// comparison at those paths checks only that value (§5.2).
#[must_use]
pub fn single_value_paths(documents: &[Value]) -> BTreeSet<String> {
    let mut values: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for document in documents {
        let mut paths = BTreeMap::new();
        record_paths(document, &mut String::new(), &mut paths);
        for (path, value) in paths {
            let scalar = match &value {
                Value::Bool(flag) => Some(flag.to_string()),
                Value::String(text) => Some(text.clone()),
                _ => None,
            };
            if let Some(scalar) = scalar {
                values.entry(path).or_default().insert(scalar);
            }
        }
    }
    values
        .into_iter()
        .filter_map(|(path, distinct)| (distinct.len() == 1).then_some(path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::apply_storage;
    use super::single_value_paths;
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
    fn single_value_paths_keep_stable_paths_only() {
        let documents = [json!({"a": true, "b": "x"}), json!({"a": true, "b": "y"})];
        let paths = single_value_paths(&documents);
        assert!(paths.contains("a"), "{paths:?}");
        assert!(!paths.contains("b"), "{paths:?}");
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

    /// The product_assembly.FCStd record that exposed the occurrence
    /// questions: the occurrence name is also a foreign native record id,
    /// and the writer nests former roots under one added synthetic root.
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
    fn projection_strips_foreign_natives_but_keeps_own() {
        use super::project_document;
        use crate::carried::Carried;

        let before = assembly_before();
        let carried = Carried::all();
        let fixed = BTreeSet::new();
        let step = project_document(&before, &carried, &fixed, "step");
        assert!(step
            .get("native")
            .and_then(|native| native.as_object())
            .is_some_and(|native| native.is_empty()));
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
        let projected = project_document(&before, &Carried::all(), &BTreeSet::new(), "step");
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
        let projected = project_document(&before, &Carried::all(), &BTreeSet::new(), "step");
        let required = required_ids(&projected, &Carried::all(), "step");
        let nulled = null_dangling(&projected, &required);
        assert_eq!(
            nulled.pointer("/model/occurrences/0/name"),
            Some(&json!("fcstd:native:object#Assembly"))
        );
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
        let projected = project_document(&before, &Carried::all(), &BTreeSet::new(), "step");
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
