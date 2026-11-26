// SPDX-License-Identifier: Apache-2.0
//! Part Studio history and assembly projection.

use crate::archive;
mod assembly;
pub(super) mod expression;
mod features;
mod intersection;
mod planes;
pub(super) mod query;
mod regions;
mod sketch;
mod sweep;
mod thin;
pub(super) mod topology;

use serde_json::{json, Value};

use crate::error::Error;
use crate::loss::OnshapeLossCode;
use archive::{Bundle, SCHEMA};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_ir::codec::Decoded;
use cadmpeg_ir::unknown::part::{record_given_up, GivenUpRecord};
use cadmpeg_ir::SourceProvenance;

type Mapping<T> = Result<T, String>;
pub(super) const ROWS: usize = 10_000;

fn text<'a>(row: &'a Value, key: &str) -> Mapping<&'a str> {
    row.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{key} is missing"))
}

fn rows<'a>(row: &'a Value, key: &str) -> Mapping<&'a [Value]> {
    row.get(key)
        .and_then(Value::as_array)
        .filter(|a| a.len() <= ROWS)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{key} is missing or exceeds the row limit"))
}

fn number(row: &Value, key: &str) -> Mapping<f64> {
    row.get(key)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("{key} has no finite value"))
}
fn coordinate(row: &Value, key: &str) -> Mapping<f64> {
    if row.get(key).is_none() {
        Ok(0.0)
    } else {
        number(row, key)
    }
}

fn optional_rows<'a>(row: &'a Value, key: &str) -> Mapping<&'a [Value]> {
    if row.get(key).is_none() {
        Ok(&[])
    } else {
        rows(row, key)
    }
}

pub(super) fn microversion(row: &Value, key: &str, expected: &str) -> Mapping<()> {
    if let Some(value) = row.get(key) {
        let actual = value
            .as_str()
            .or_else(|| value.get("theId").and_then(Value::as_str));
        if actual != Some(expected) {
            return Err(format!(
                "{key} does not match the pinned microversion; fetch a new bundle"
            ));
        }
    }
    Ok(())
}

pub(super) fn id(kind: &str, native: &str) -> String {
    format!(
        "onshape:model:{kind}#{}",
        cadmpeg_ir::ids::IdentityKey::encode_key_text(native)
    )
}

fn push<T: serde::de::DeserializeOwned>(arena: &mut Vec<T>, row: Value) -> Mapping<()> {
    arena.push(
        cadmpeg_core::serde_erase::json::from_value(row)
            .map_err(|e| format!("the resolved construction is not supported yet ({e})"))?,
    );
    Ok(())
}
fn definition(kind: &str, value: Value) -> Value {
    let operation = serde_json::Map::from_iter([
        ("kind".into(), Value::String(kind.into())),
        ("value".into(), value),
    ]);
    Value::Object(serde_json::Map::from_iter([
        ("kind".into(), Value::String("operation".into())),
        ("value".into(), Value::Object(operation)),
    ]))
}

fn mapping_loss_code(
    feature: &Value,
    region_budget_exhausted: bool,
    valid_suppression: bool,
    reason: &str,
) -> OnshapeLossCode {
    if !region_budget_exhausted
        && feature.get("featureType").and_then(Value::as_str) == Some("extrude")
        && features::value(feature, "bodyType") == Some("THIN")
    {
        OnshapeLossCode::ThinNotSupported
    } else if !region_budget_exhausted
        && valid_suppression
        && feature.get("featureType").and_then(Value::as_str) == Some("sweep")
        && sweep::validate_record(feature).is_ok()
        && !reason.starts_with(sweep::PATH_BUDGET_EXHAUSTED)
    {
        OnshapeLossCode::SweepNotSupported
    } else if feature.get("featureType").and_then(Value::as_str) == Some("cPlane") {
        planes::loss_code(feature, reason)
    } else if valid_suppression
        && feature.get("featureType").and_then(Value::as_str) == Some("shell")
        && features::shell_record_valid(feature)
        && reason.starts_with(features::SHELL_REFUSAL)
    {
        OnshapeLossCode::ShellNotSupported
    } else {
        OnshapeLossCode::HistoryNotSupported
    }
}

fn loss(report: &mut Report, subject: &str, reason: &str, has_step: bool, code: OnshapeLossCode) {
    report.push_coded(
        subject,
        code,
        format!(
            "{reason}; kept {}",
            if has_step {
                "the final STEP geometry"
            } else {
                "the supported sketch geometry"
            }
        ),
    );
}
pub(super) fn part_studio_bodies(model: &mut Model) {
    use cadmpeg_ir::products::owner;
    let rows = &model.ir.model;
    if rows.occurrences.len() < 2 || !rows.assembly_joints.is_empty() {
        return;
    }
    let owner = rows
        .occurrences
        .first()
        .and_then(|o| owner(&rows.product_definitions, &o.id));
    if owner.is_some_and(|root| !root.is_root() || root.occurrences.len() != rows.occurrences.len())
    {
        return;
    }
    let owner_id = owner.map(|root| root.id.clone());
    let mut placed = std::collections::BTreeSet::new();
    for occurrence in &rows.occurrences {
        if cadmpeg_ir::products::owner(&rows.product_definitions, &occurrence.id)
            .map(|root| &root.id)
            != owner_id.as_ref()
            || occurrence.transform != Some(cadmpeg_ir::transform::Transform::identity())
            || occurrence.suppressed == Some(true)
            || occurrence.visible == Some(false)
            || occurrence.representation.is_some()
            || !occurrence.overrides.is_empty()
            || !placed.insert(occurrence.prototype.as_str())
        {
            return;
        }
    }
    if rows
        .product_definitions
        .iter()
        .filter(|d| Some(&d.id) != owner_id.as_ref())
        .count()
        != placed.len()
        || rows
            .product_definitions
            .iter()
            .filter(|d| Some(&d.id) != owner_id.as_ref())
            .any(|d| {
                // A part body is an embedded leaf definition, active, with no own
                // occurrences, instance bodies or representations; anything else is a
                // real insertion and keeps its occurrence.
                !placed.contains(d.id.as_str())
                    || !matches!(d.storage, cadmpeg_ir::products::Storage::Embedded)
                    || !d.occurrences.is_empty()
                    || !d.instance_bodies.is_empty()
                    || !d.representations.is_empty()
                    || d.active == Some(false)
            })
    {
        return;
    }
    let Ok(text) = cadmpeg_core::serde_erase::json::to_string(&model.ir) else {
        return;
    };
    let named = |id: &str| text.matches(&format!("\"{id}\"")).count();
    if rows
        .occurrences
        .iter()
        .any(|occurrence| named(occurrence.id.as_str()) != if owner_id.is_some() { 2 } else { 1 })
    {
        return;
    }
    model.ir.model.occurrences.clear();
    if let Some(owner_id) = owner_id {
        if let Some(root) = model
            .ir
            .model
            .product_definitions
            .iter_mut()
            .find(|d| d.id == owner_id)
        {
            root.occurrences.clear();
        }
    }
    let names: std::collections::BTreeMap<String, String> = model
        .ir
        .model
        .product_definitions
        .iter()
        .filter_map(|definition| {
            let [body] = definition.bodies.as_slice() else {
                return None;
            };
            let name = definition
                .label
                .as_ref()
                .or(definition.source_name.as_ref())
                .filter(|name| !name.trim().is_empty())?;
            Some((body.as_str().to_owned(), name.clone()))
        })
        .collect();
    for body in &mut model.ir.geometry.bodies {
        if body.name.as_ref().is_none_or(|name| name.trim().is_empty()) {
            if let Some(name) = names.get(body.id.as_str()) {
                body.name = Some(name.clone());
            }
        }
    }
}
pub(super) fn part_bodies(
    creators: &[usize],
    inputs: &topology::Inputs<'_>,
    model: &Model,
) -> std::collections::BTreeMap<usize, cadmpeg_ir::ids::BodyId> {
    let mut named: std::collections::BTreeMap<&str, Vec<&cadmpeg_ir::ids::BodyId>> =
        std::collections::BTreeMap::new();
    for body in &model.ir.geometry.bodies {
        if let Some(name) = body.name.as_deref() {
            named.entry(name).or_default().push(&body.id);
        }
    }
    let mut claims: std::collections::BTreeMap<usize, &cadmpeg_ir::ids::BodyId> =
        std::collections::BTreeMap::new();
    for &ordinal in creators {
        let (Ok(before), Ok(after)) = (inputs.parts(ordinal), inputs.parts(ordinal + 1)) else {
            continue;
        };
        let made: Vec<&Option<String>> = after
            .iter()
            .filter(|(id, _)| !before.contains_key(*id))
            .map(|(_, name)| name)
            .collect();
        let [Some(name)] = made.as_slice() else {
            continue;
        };
        if after.values().filter(|n| n.as_ref() == Some(name)).count() != 1 {
            continue;
        }
        if let Some([body]) = named.get(name.as_str()).map(Vec::as_slice) {
            claims.insert(ordinal, body);
        }
    }
    claims
        .iter()
        .filter(|(_, body)| claims.values().filter(|b| b == body).count() == 1)
        .map(|(ordinal, body)| (*ordinal, (*body).clone()))
        .collect()
}

fn boolean_field(row: &Value, key: &str) -> Mapping<bool> {
    match row.get(key) {
        None => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(format!("{key} must be Boolean")),
    }
}
/// The stored rollback bar: the IDs of the features at and after the bar,
/// in timeline order. Empty states the bar after the last feature.
#[derive(Debug, Default)]
pub(super) struct RollbackBar(pub(super) Vec<String>);

/// Reads `rollbackIndex`, the count of features before the rollback bar.
fn rollback_bar(
    response: &Value,
    features: &[Value],
    report: &mut Report,
    path: &str,
) -> RollbackBar {
    let Some(index) = response.get("rollbackIndex") else {
        if features.is_empty() {
            return RollbackBar::default();
        }
        report.push_coded(
            path,
            OnshapeLossCode::RollbackUnread,
            "rollbackIndex is absent; stated the rollback bar after the last feature",
        );
        return RollbackBar::default();
    };
    let Some(rest) = index
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| features.get(index..))
    else {
        report.push_coded(
            path,
            OnshapeLossCode::RollbackUnread,
            format!(
                "rollbackIndex {index} is not a position in the {} features; stated the rollback bar after the last feature",
                features.len()
            ),
        );
        return RollbackBar::default();
    };
    RollbackBar(
        rest.iter()
            .filter_map(|feature| feature.get("featureId").and_then(Value::as_str))
            .map(str::to_owned)
            .collect(),
    )
}

fn suppression(feature: &Value) -> Mapping<bool> {
    Ok(boolean_field(feature, "suppressed")? | boolean_field(feature, "parentSuppressed")?)
}
fn reserve_history<T>(rows: &mut Vec<T>, count: usize) -> Result<(), Error> {
    rows.try_reserve(count)
        .map_err(|_| Error::Decode("Onshape history allocation failed".into()))
}
fn admit_history(
    ctx: &DecodeContext<'_>,
    feature: &Value,
    model: &mut cadmpeg_ir::document::Model,
) -> Result<(), Error> {
    let count = |key| optional_rows(feature, key).map_or(0, <[Value]>::len);
    let entities = count("entities");
    let parameters = count("parameters");
    let mut generated_entities = entities;
    let mut generated_constraints = 0usize;
    let mut generated_parameters = parameters;
    let add = |left: usize, right: usize| {
        left.checked_add(right)
            .ok_or_else(|| Error::Decode("history count overflow".into()))
    };
    for constraint in optional_rows(feature, "constraints").unwrap_or(&[]) {
        if let Ok(output) = sketch::constraint_output(constraint) {
            generated_entities = add(generated_entities, output.entities)?;
            generated_constraints = add(generated_constraints, output.constraints)?;
            generated_parameters = add(generated_parameters, output.parameters)?;
        }
    }
    let total = add(
        add(
            add(generated_entities, generated_constraints)?,
            generated_parameters,
        )?,
        2,
    )?;
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(total),
        "Onshape history admission",
    )?;
    reserve_history(&mut model.features, 1)?;
    reserve_history(&mut model.sketches, 1)?;
    reserve_history(&mut model.sketch_entities, generated_entities)?;
    reserve_history(&mut model.sketch_constraints, generated_constraints)?;
    reserve_history(&mut model.parameters, generated_parameters)?;
    Ok(())
}

pub(super) fn read(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    step_decoder: crate::StepDecoder,
    report: &mut Report,
) -> Result<Model, Error> {
    let mut bundle = Bundle::open(ctx, root)?;
    report.notes.append(&mut bundle.losses);
    let manifest = bundle.json("manifest.json")?;
    if manifest.get("schema").and_then(Value::as_str) != Some(SCHEMA) {
        return Err(Error::Decode(
            "Onshape bundle schema is not supported yet; fetch a format 1 bundle".into(),
        ));
    }
    match manifest.get("consistent") {
        Some(Value::Bool(false)) => report.push_coded(
            "manifest.json",
            OnshapeLossCode::RecordMalformed,
            "bundle consistency is false; each response uses its pinned microversion for admission",
        ),
        Some(Value::Bool(true)) | None => {}
        Some(_) => {
            return Err(Error::Decode(
                "Onshape bundle consistency flag must be true or false".into(),
            ))
        }
    }
    if ctx.container_only() {
        let mut model = Model::from_decoded(empty(ctx, root), None);
        let given_up = bundle.commit_given_up(ctx, &mut model.ir, &mut model.source_fidelity)?;
        report.notes.extend(given_up);
        return Ok(model);
    }
    let document = manifest.get("document").unwrap_or(&Value::Null);
    let mid = text(document, "microversion").map_err(Error::Decode)?;
    let mid = mid.to_owned();
    for key in ["sourceMicroversion", "microversionId"] {
        microversion(manifest, key, &mid).map_err(Error::Decode)?;
    }
    bundle.admit_microversions(ctx, &mid)?;
    bundle.admit_topology(ctx)?;
    report.notes.append(&mut bundle.losses);
    let manifest = bundle.json("manifest.json")?;
    let elements = rows(manifest, "elements").map_err(Error::Decode)?;
    let mut model = if elements.len() == 1
        && elements
            .first()
            .and_then(|e| e.get("type"))
            .and_then(Value::as_str)
            == Some("PARTSTUDIO")
    {
        studio(
            ctx,
            root,
            step_decoder,
            report,
            &bundle,
            &mid,
            text(
                elements
                    .first()
                    .ok_or_else(|| Error::Decode("element missing".into()))?,
                "id",
            )
            .map_err(Error::Decode)?,
        )?
    } else {
        assembly::read(ctx, root, step_decoder, report, &bundle, &mid, manifest)?
    };
    report
        .notes
        .extend(bundle.retain_assets(ctx, &mut model.ir)?);
    let given_up = bundle.commit_given_up(ctx, &mut model.ir, &mut model.source_fidelity)?;
    report.notes.extend(given_up);
    Ok(model)
}

fn studio(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    step_decoder: crate::StepDecoder,
    report: &mut Report,
    bundle: &Bundle<'_>,
    mid: &str,
    eid: &str,
) -> Result<Model, Error> {
    if !eid
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(Error::Decode(
            "Onshape element id cannot name a bundle folder; fetch a new bundle".into(),
        ));
    }
    let prefix = format!("partstudios/{eid}");
    let history_path = format!("{prefix}/features.json");
    let (features, bar) = match bundle.json(&history_path) {
        Ok(response) => match rows(response, "features") {
            Ok(features) => (
                features,
                rollback_bar(response, features, report, &history_path),
            ),
            Err(reason) => {
                let detail = format!("{reason}; feature history is missing");
                if response
                    .get("features")
                    .and_then(Value::as_array)
                    .is_some_and(|features| features.len() > ROWS)
                {
                    report.push_coded(&history_path, OnshapeLossCode::HistoryNotSupported, detail);
                } else {
                    report.give_up(
                        bundle,
                        &history_path,
                        OnshapeLossCode::HistoryNotSupported,
                        detail,
                    );
                }
                (&[][..], RollbackBar::default())
            }
        },
        Err(error) => {
            report.push_coded(
                &history_path,
                OnshapeLossCode::HistoryNotSupported,
                format!("{error}; feature history is missing"),
            );
            (&[][..], RollbackBar::default())
        }
    };
    let inputs = topology::Inputs::new(bundle, &prefix, mid, features, report);
    let step_path = format!("{prefix}/export.step");
    let step = if bundle.contains(&step_path) {
        let source = bundle.view(&step_path)?;
        let decoded = ctx.run_part(|| {
            let _depth = ctx.enter_nested("Onshape STEP member")?;
            std::str::from_utf8(source.window())
                .map_err(|_| {
                    cadmpeg_core::CodecError::malformed("Onshape STEP export is not UTF-8")
                })
                .and_then(|text| {
                    step_decoder(ctx, source).map(|decoded| (decoded, text.to_owned()))
                })
        })?;
        match decoded {
            Ok((decoded, text)) => Some(Model::from_decoded(decoded, Some(text))),
            Err(error) if features.is_empty() => return Err(error.into()),
            Err(error) => {
                let detail = format!("{error}; final geometry is missing");
                if matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)) {
                    report.push_coded(&step_path, OnshapeLossCode::StepUnreadable, detail);
                } else {
                    report.give_up(bundle, &step_path, OnshapeLossCode::StepUnreadable, detail);
                }
                None
            }
        }
    } else {
        if features.is_empty() {
            return Err(Error::Decode(
                "Onshape bundle has no history or final geometry".into(),
            ));
        }
        if features.iter().any(|feature| {
            suppression(feature) != Ok(true)
                && !matches!(
                    feature.get("featureType").and_then(Value::as_str),
                    Some("newSketch" | "assignVariable")
                )
        }) && !report
            .notes
            .iter()
            .any(|note| OnshapeLossCode::StepUnreadable.matches(note))
        {
            report.push_coded(
                &step_path,
                OnshapeLossCode::StepUnreadable,
                "STEP export is missing; final geometry is missing",
            );
        }
        None
    };
    let mut model = step.unwrap_or_else(|| Model::from_decoded(empty(ctx, root), None));
    if model.source_step.is_some() {
        part_studio_bodies(&mut model);
    }
    if ctx.depth() == cadmpeg_core::decode::DecodeDepth::Geometry {
        return Ok(model);
    }
    let creators: Vec<usize> = features
        .iter()
        .enumerate()
        .filter(|(_, feature)| {
            matches!(
                feature.get("featureType").and_then(Value::as_str),
                Some("extrude" | "revolve" | "sweep")
            ) && features::value(feature, "operationType") == Some("NEW")
                && suppression(feature) == Ok(false)
        })
        .map(|(ordinal, _)| ordinal)
        .collect();
    let single_body = (creators.len() == 1 && model.ir.geometry.bodies.len() == 1)
        .then(|| model.ir.geometry.bodies.first().map(|body| body.id.clone()))
        .flatten();
    let parts = model.ir.geometry.bodies.len();
    let part_bodies = if single_body.is_none() && parts > 1 && !creators.is_empty() {
        let bound = part_bodies(&creators, &inputs, &model);
        if bound.len() < parts {
            report.push_coded(
                "part names",
                OnshapeLossCode::PartNotMatched,
                format!(
                    "{} of {} parts could not be matched by name to the feature that made \
                     them; part identities and colours remain unbound",
                    parts
                        .checked_sub(bound.len())
                        .ok_or_else(|| Error::Decode("part claim count exceeds bodies".into()))?,
                    parts
                ),
            );
        }
        bound
    } else {
        std::collections::BTreeMap::new()
    };
    let original_design = model.ir.design.clone();
    let original_design_features = original_design.features.len();
    if features
        .iter()
        .any(|feature| features::value(feature, "bodyType") == Some("THIN"))
    {
        ctx.charge_entities(
            cadmpeg_core::decode::u64_from_index(features.len()) + 2,
            "Onshape thin timeline admission",
        )?;
        reserve_history(&mut model.ir.design.features, features.len())?;
        reserve_history(&mut model.ir.design.containers, 1)?;
        reserve_history(&mut model.ir.design.scopes, 1)?;
    }
    let mut seen = std::collections::BTreeMap::<&str, &Value>::new();
    let mut unresolved_variables = std::collections::BTreeSet::new();
    for (ordinal, feature) in features.iter().enumerate() {
        let design_length = model.ir.design.features.len();
        let note_start = report.notes.len();
        let native = match text(feature, "featureId") {
            Ok(native) => match seen.entry(native) {
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert(feature);
                    native
                }
                std::collections::btree_map::Entry::Occupied(first) if *first.get() == feature => {
                    report.push_coded(
                        native,
                        OnshapeLossCode::RecordMalformed,
                        "a feature repeats an earlier feature with the same id and content; kept the earlier feature",
                    );
                    continue;
                }
                std::collections::btree_map::Entry::Occupied(_) => {
                    report.give_up_as(
                        &format!("{history_path}#/features/{ordinal}"),
                        native,
                        OnshapeLossCode::RecordMalformed,
                        "a feature repeats an earlier feature id with different content; omitted the feature",
                    );
                    continue;
                }
            },
            Err(reason) => {
                report.give_up_as(
                    &format!("{history_path}#/features/{ordinal}"),
                    &format!("feature #{ordinal}"),
                    OnshapeLossCode::RecordMalformed,
                    format!("{reason}; omitted the feature"),
                );
                continue;
            }
        };
        let feature_id = id("feature", native);
        let name = feature
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(native);
        let suppressed = suppression(feature);
        let bindings: Vec<_> =
            if feature.get("featureType").and_then(Value::as_str) == Some("assignVariable") {
                optional_rows(feature, "parameters")
                    .unwrap_or(&[])
                    .iter()
                    .filter(|p| p.get("parameterId").and_then(Value::as_str) == Some("name"))
                    .filter_map(|p| p.get("value").and_then(Value::as_str))
                    .collect()
            } else {
                Vec::new()
            };
        if suppressed.as_ref().is_ok_and(|s| !s) {
            for name in &bindings {
                if model
                    .ir
                    .model
                    .parameters
                    .iter()
                    .any(|p| p.owner.is_none() && p.name == *name)
                {
                    unresolved_variables.insert((*name).to_owned());
                }
            }
        }
        admit_history(ctx, feature, &mut model.ir.model)?;
        let sizes = (
            model.ir.model.sketches.len(),
            model.ir.model.sketch_entities.len(),
            model.ir.model.sketch_constraints.len(),
            model.ir.model.parameters.len(),
            model.ir.model.feature_input_topologies.len(),
            model.ir.design.features.len(),
        );
        let mut dependencies = Vec::new();
        let mapped = if let Err(reason) = &suppressed {
            Err(reason.clone())
        } else if suppressed == Ok(true) {
            Ok(definition(
                "native",
                json!({"kind": "Suppressed Onshape feature", "parameters": {}}),
            ))
        } else {
            features::map(
                feature,
                ordinal,
                &inputs,
                &mut model.ir.model,
                report,
                &unresolved_variables,
            )
            .and_then(|definition| {
                cadmpeg_core::serde_erase::json::from_value::<cadmpeg_ir::features::FeatureDefinition>(
                    definition.clone(),
                )
                .map_err(|e| format!("the resolved construction is not supported yet ({e})"))?;
                dependencies = features::dependencies(
                    feature,
                    &definition,
                    ordinal,
                    &inputs,
                    &model.ir.model,
                )?;
                if features::value(feature, "bodyType") == Some("THIN")
                    && feature.get("featureType").and_then(Value::as_str) == Some("extrude")
                {
                    thin::owner(&model.ir)?;
                    let (design, assumed) = thin::feature(
                        feature,
                        &definition,
                        &model.ir.model,
                        &inputs,
                        ordinal,
                        &unresolved_variables,
                    )?;
                    push(&mut model.ir.design.features, design)?;
                    if let Some(assumed) = assumed {
                        report.push_coded(name, OnshapeLossCode::ThinNotSupported, assumed);
                    }
                    report.push_coded(name, OnshapeLossCode::ThinModelNotSupported,
                        "model construction retains the native thin operation; design construction carries the proved wall operands");
                    return Ok(self::definition("native", json!({"kind":"Onshape thin extrusion", "parameters":{}})));
                }
                Ok(definition)
            })
        };
        let definition = match mapped {
            Ok(definition) => {
                if suppressed != Ok(true)
                    && !matches!(
                        feature.get("featureType").and_then(Value::as_str),
                        Some("newSketch" | "assignVariable")
                    )
                {
                    if let Err(reason) =
                        features::report_evaluated_variables(feature, &model.ir.model, report)
                    {
                        report.push_coded(
                            name,
                            OnshapeLossCode::RecordMalformed,
                            format!("{reason}; evaluated variable facts are not reported"),
                        );
                    }
                }
                definition
            }
            Err(reason) => {
                let mut tail = report.notes.split_off(note_start);
                tail.retain(|note| !OnshapeLossCode::SketchPlaneAssumed.matches(note));
                report.notes.extend(tail);
                model.ir.model.sketches.truncate(sizes.0);
                model.ir.model.sketch_entities.truncate(sizes.1);
                model.ir.model.sketch_constraints.truncate(sizes.2);
                model.ir.model.parameters.truncate(sizes.3);
                model.ir.model.feature_input_topologies.truncate(sizes.4);
                model.ir.design.features.truncate(sizes.5);
                if suppressed != Ok(true) {
                    let region_budget_exhausted = inputs
                        .regions
                        .try_borrow()
                        .is_ok_and(|recovery| recovery.exhausted());
                    let code = mapping_loss_code(
                        feature,
                        region_budget_exhausted,
                        suppressed.is_ok(),
                        &reason,
                    );
                    loss(report, name, &reason, model.source_step.is_some(), code);
                }
                definition(
                    "native",
                    json!({"kind": format!("Onshape {}: {reason}", feature.get("featureType").and_then(Value::as_str).unwrap_or("feature")), "parameters": {}}),
                )
            }
        };
        if definition.pointer("/value/kind").and_then(Value::as_str) == Some("native")
            && suppressed != Ok(true)
        {
            if bindings.is_empty()
                && feature.get("featureType").and_then(Value::as_str) == Some("assignVariable")
            {
                unresolved_variables.extend(
                    model
                        .ir
                        .model
                        .parameters
                        .iter()
                        .filter(|p| p.owner.is_none())
                        .map(|p| p.name.clone()),
                );
            }
            for name in &bindings {
                unresolved_variables.insert((*name).to_owned());
            }
        }
        if suppressed == Ok(false)
            && definition.pointer("/value/kind").and_then(Value::as_str) == Some("tree_node")
            && feature.get("featureType").and_then(Value::as_str) == Some("assignVariable")
        {
            for name in &bindings {
                unresolved_variables.remove(*name);
            }
        }
        let mut source_properties = features::source_properties(feature, &definition)
            .unwrap_or_else(|reason| {
                report.push_coded(
                    name,
                    OnshapeLossCode::RecordMalformed,
                    format!("{reason}; source properties are not retained"),
                );
                json!({})
            });
        if definition.pointer("/value/kind").and_then(Value::as_str) == Some("extrude")
            && definition
                .pointer("/value/value/extent/kind")
                .and_then(Value::as_str)
                == Some("symmetric")
        {
            if let Some(properties) = source_properties.as_object_mut() {
                properties.insert("symmetric_distance_parameter".into(), json!("total"));
            }
        }
        let outputs: Vec<_> = single_body
            .iter()
            .chain(part_bodies.get(&ordinal))
            .filter(|_| {
                (model
                    .ir
                    .design
                    .features
                    .iter()
                    .any(|design| design.id.as_str() == feature_id)
                    && features::value(feature, "operationType") == Some("NEW"))
                    || definition
                        .pointer("/value/value/shape/op")
                        .and_then(Value::as_str)
                        == Some("new_body")
                        && definition.pointer("/value/kind").and_then(Value::as_str)
                            == Some("sweep")
                    || definition
                        .pointer("/value/value/op")
                        .and_then(Value::as_str)
                        == Some("new_body")
                        && matches!(
                            definition.pointer("/value/kind").and_then(Value::as_str),
                            Some("extrude" | "revolve")
                        )
            })
            .cloned()
            .collect();
        let mut row = json!({"id": feature_id, "ordinal": ordinal, "name": name, "dependencies": dependencies, "source_properties": source_properties, "source_content": [], "outputs": outputs, "definition": definition});
        if let Ok(suppressed) = suppressed {
            row["suppressed"] = json!(suppressed);
        }
        if let Err(reason) = push(&mut model.ir.model.features, row) {
            model.ir.design.features.truncate(design_length);
            let mut tail = report.notes.split_off(note_start);
            tail.retain(|note| {
                !OnshapeLossCode::SketchPlaneAssumed.matches(note)
                    && !OnshapeLossCode::ThinModelNotSupported.matches(note)
                    && !OnshapeLossCode::ThinNotSupported.matches(note)
            });
            report.notes.extend(tail);
            report.give_up_as(
                &format!("{history_path}#/features/{ordinal}"),
                name,
                OnshapeLossCode::RecordMalformed,
                format!("{reason}; omitted the feature"),
            );
        }
    }
    if model.ir.design.features.len() > original_design_features {
        install_thin_scope(ctx, &mut model.ir, report, original_design, &bar)?;
    }
    report
        .notes
        .extend(inputs.sampling_report.into_inner().notes);
    annotate(&mut model, features)?;
    Ok(model)
}

fn install_thin_scope(
    ctx: &DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    report: &mut Report,
    original: cadmpeg_ir::document::Design,
    bar: &RollbackBar,
) -> Result<(), Error> {
    let notes = report.notes.len();
    let result = thin::sketches::project(ctx, ir)
        .and_then(|()| thin::install_scope(ir, report, bar).map_err(Error::Decode));
    if let Err(reason) = result {
        ir.design = original;
        report.notes.truncate(notes);
        if matches!(
            &reason,
            Error::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.scope == cadmpeg_core::decode::LimitScope::Session
        ) {
            return Err(reason);
        }
        for note in &mut report.notes {
            if OnshapeLossCode::ThinModelNotSupported.matches(note) {
                *note = OnshapeLossCode::ThinNotSupported.note(format!(
                    "{reason}; thin construction retains its native model operation"
                ));
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
pub(super) struct Model {
    pub(super) ir: cadmpeg_ir::CadIr,
    pub(super) body: cadmpeg_ir::codec::DecodeBody,
    pub(super) source_fidelity: cadmpeg_ir::SourceFidelity,
    pub(super) source_step: Option<String>,
}

#[derive(Default)]
pub(super) struct Report {
    pub(super) notes: Vec<cadmpeg_ir::report::loss::LossNote>,
}
impl Report {
    fn push_coded(&mut self, subject: &str, code: OnshapeLossCode, detail: impl Into<String>) {
        self.notes
            .push(code.note(format!("{}; subject: {subject}", detail.into())));
    }
    /// Pushes the coded note for `record`, a member path of `bundle`, and
    /// its `record.unreadable` at the member's stored bytes. The member
    /// stays an asset; the give-up is its typed content.
    fn give_up(
        &mut self,
        bundle: &Bundle<'_>,
        record: &str,
        code: OnshapeLossCode,
        detail: impl Into<String>,
    ) {
        let note = code.note(format!("{}; subject: {record}", detail.into()));
        let Some((offset, length)) = bundle.stored_span(record) else {
            self.notes.extend(record_given_up(record, note));
            return;
        };
        let note = note.with_provenance(SourceProvenance::root("onshape", offset));
        let unreadable = GivenUpRecord::caused_by(record, &note)
            .spanning(length)
            .loss();
        self.notes.extend([note, unreadable]);
    }
    /// Pushes the coded note for `subject` and the `record.unreadable` of
    /// `record`, its source address.
    fn give_up_as(
        &mut self,
        record: &str,
        subject: &str,
        code: OnshapeLossCode,
        detail: impl Into<String>,
    ) {
        let note = code.note(format!("{}; subject: {subject}", detail.into()));
        self.notes.extend(record_given_up(record, note));
    }
}
fn empty(ctx: &DecodeContext<'_>, root: View<'_>) -> Decoded {
    Decoded {
        ir: cadmpeg_ir::CadIr::empty(),
        body: cadmpeg_ir::codec::DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer::at(
            ctx.depth(),
            false,
        )),
        source_fidelity: cadmpeg_ir::SourceFidelity::for_root(root),
    }
}

impl Model {
    pub(super) fn from_decoded(decoded: Decoded, source_step: Option<String>) -> Self {
        Self {
            ir: decoded.ir,
            body: decoded.body,
            source_fidelity: decoded.source_fidelity,
            source_step,
        }
    }
    pub(super) fn into_decoded(self) -> Decoded {
        Decoded {
            ir: self.ir,
            body: self.body,
            source_fidelity: self.source_fidelity,
        }
    }
}

fn annotate(model: &mut Model, features: &[Value]) -> Result<(), Error> {
    use cadmpeg_ir::annotations::AnnotationBuilder;
    use cadmpeg_ir::Exactness;
    let mut annotations =
        AnnotationBuilder::resume(std::mem::take(&mut model.source_fidelity.annotations));
    for topology in &model.ir.model.feature_input_topologies {
        annotations.exactness(&topology.id, Exactness::Derived);
    }
    for entity in &model.ir.model.sketch_entities {
        if !matches!(
            entity.geometry.definition(),
            cadmpeg_ir::sketches::SketchGeometryDefinition::Point { .. }
        ) {
            annotations
                .field_exactness(entity.id(), "geometry", Exactness::Derived)
                .map_err(cadmpeg_core::CodecError::from)?;
        }
    }
    for parameter in &model.ir.model.parameters {
        if !parameter.dependencies.is_empty() || parameter.expression.contains('(') {
            annotations
                .field_exactness(&parameter.id, "value", Exactness::Derived)
                .map_err(cadmpeg_core::CodecError::from)?;
        }
    }
    for feature in &model.ir.model.features {
        annotations
            .field_exactness(&feature.id, "definition", Exactness::Derived)
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .field_exactness(&feature.id, "dependencies", Exactness::Derived)
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .field_exactness(&feature.id, "outputs", Exactness::Derived)
            .map_err(cadmpeg_core::CodecError::from)?;
    }
    let mut seen = std::collections::BTreeSet::new();
    for source in features {
        let Ok(native) = text(source, "featureId") else {
            continue;
        };
        let feature_id = id("feature", native);
        if !seen.insert(native)
            || !model
                .ir
                .model
                .features
                .iter()
                .any(|feature| feature.id.as_str() == feature_id)
        {
            continue;
        }
        let sketch_id = id("sketch", native);
        if model
            .ir
            .model
            .sketches
            .iter()
            .any(|s| s.id.as_str() == sketch_id)
        {
            let queries = features::queries(source, "sketchPlane").unwrap_or(&[]);
            let inferred = queries.iter().all(|q| {
                q.get("deterministicIds")
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty)
                    && q.get("queryString")
                        .and_then(Value::as_str)
                        .is_none_or(|text| {
                            !text.contains("qCompressed(")
                                || query::resolved(q).is_ok_and(|v| v["queryType"] == "DUMMY")
                        })
            });
            annotations
                .field_exactness(
                    &sketch_id,
                    "placement",
                    if inferred {
                        Exactness::Inferred
                    } else {
                        Exactness::Derived
                    },
                )
                .map_err(cadmpeg_core::CodecError::from)?;
        }
        if source.get("suppressed").is_none() && source.get("parentSuppressed").is_none() {
            annotations
                .field_exactness(id("feature", native), "suppressed", Exactness::Inferred)
                .map_err(cadmpeg_core::CodecError::from)?;
        }
    }
    model.source_fidelity.annotations = annotations.build();
    Ok(())
}

#[cfg(test)]
mod tests;
