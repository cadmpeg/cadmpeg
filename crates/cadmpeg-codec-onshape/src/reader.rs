// SPDX-License-Identifier: Apache-2.0
//! Part Studio history projection.

use crate::archive;
pub(super) mod expression;
mod features;
pub(super) mod query;
mod regions;
mod sketch;
mod thin;
pub(super) mod topology;

use serde_json::{json, Value};

use crate::error::Error;
use crate::loss::OnshapeLossCode;
use archive::{Bundle, SCHEMA};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_ir::codec::Decoded;

type Mapping<T> = Result<T, String>;
const ROWS: usize = 10_000;

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

fn microversion(row: &Value, key: &str, expected: &str) -> Mapping<()> {
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
        serde_json::from_value(row)
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
            .any(|d| !placed.contains(d.id.as_str()))
    {
        return;
    }
    let Ok(text) = serde_json::to_string(&model.ir) else {
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

pub(super) fn read(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    step_decoder: crate::StepDecoder,
    report: &mut Report,
) -> Result<Model, Error> {
    let bundle = Bundle::open(ctx, root)?;
    let manifest = bundle.json("manifest.json")?;
    if manifest.get("schema").and_then(Value::as_str) != Some(SCHEMA) {
        return Err(Error::Decode(
            "Onshape bundle schema is not supported yet; fetch a format 1 bundle".into(),
        ));
    }
    match manifest.get("consistent") {
        Some(Value::Bool(false)) => return Err(Error::Decode(
            "Onshape bundle is marked inconsistent: its files do not reflect one microversion; fetch it again, or use a version link".into(),
        )),
        Some(Value::Bool(true)) | None => {},
        Some(_) => return Err(Error::Decode(
            "Onshape bundle consistency flag must be true or false; fetch a new bundle".into(),
        )),
    }
    if ctx.container_only() {
        return Ok(Model::from_decoded(empty(ctx, root), None));
    }
    let document = manifest.get("document").unwrap_or(&Value::Null);
    let mid = text(document, "microversion").map_err(Error::Decode)?;
    let elements = rows(&manifest, "elements").map_err(Error::Decode)?;
    if elements.len() != 1
        || elements
            .first()
            .and_then(|e| e.get("type"))
            .and_then(Value::as_str)
            != Some("PARTSTUDIO")
    {
        return Err(Error::Decode("Onshape bundles with assemblies or multiple elements are not supported yet; fetch one Part Studio".into()));
    }
    let element = elements
        .first()
        .ok_or_else(|| Error::Decode("Onshape bundle has no Part Studio".into()))?;
    let eid = text(element, "id").map_err(Error::Decode)?;
    if !eid
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(Error::Decode(
            "Onshape element id cannot name a bundle folder; fetch a new bundle".into(),
        ));
    }
    let prefix = format!("partstudios/{eid}");
    let response = bundle.json(&format!("{prefix}/features.json"))?;
    microversion(&response, "sourceMicroversion", mid).map_err(Error::Decode)?;
    let features = rows(&response, "features").map_err(Error::Decode)?;
    let inputs = topology::Inputs::new(&bundle, &prefix, mid, features).map_err(Error::Decode)?;
    let step_path = format!("{prefix}/export.step");
    let mut model = if bundle.contains(&step_path) {
        let source = bundle.view(&step_path)?;
        let _depth = ctx.enter_nested("Onshape STEP member")?;
        let decoded = step_decoder(ctx, source)?;
        let source_step = Some(
            std::str::from_utf8(source.window())
                .map_err(|_| Error::Decode("Onshape STEP export is not UTF-8".into()))?
                .to_owned(),
        );
        let mut model = Model::from_decoded(decoded, source_step);
        part_studio_bodies(&mut model);
        model
    } else if !features.is_empty()
        && features.iter().all(|feature| {
            feature.get("suppressed").and_then(Value::as_bool) == Some(true)
                || feature.get("parentSuppressed").and_then(Value::as_bool) == Some(true)
                || matches!(
                    feature.get("featureType").and_then(Value::as_str),
                    Some("newSketch" | "assignVariable")
                )
        })
    {
        Model::from_decoded(empty(ctx, root), None)
    } else {
        return Err(Error::Decode(
            "Onshape bundle has body-building features but no STEP export; fetch a new bundle"
                .into(),
        ));
    };
    if ctx.depth() == cadmpeg_core::decode::DecodeDepth::Geometry {
        report
            .notes
            .extend(bundle.retain_assets(ctx, &mut model.ir)?);
        return Ok(model);
    }
    let creators: Vec<usize> = features
        .iter()
        .enumerate()
        .filter(|(_, feature)| {
            matches!(
                feature.get("featureType").and_then(Value::as_str),
                Some("extrude" | "revolve")
            ) && features::value(feature, "operationType") == Some("NEW")
                && feature.get("suppressed").and_then(Value::as_bool) != Some(true)
                && feature.get("parentSuppressed").and_then(Value::as_bool) != Some(true)
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
    let mut seen = std::collections::BTreeSet::new();
    let mut previous = None;
    let mut admitted = cadmpeg_core::decode::u64_from_index(model.ir.entity_count());
    for (ordinal, feature) in features.iter().enumerate() {
        let native = text(feature, "featureId").map_err(Error::Decode)?;
        if !seen.insert(native) {
            return Err(Error::Decode(
                "Onshape feature ids repeat; fetch a new bundle".into(),
            ));
        }
        let feature_id = id("feature", native);
        let name = feature
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(native);
        let suppressed = feature
            .get("suppressed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || feature
                .get("parentSuppressed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        let sizes = (
            model.ir.model.sketches.len(),
            model.ir.model.sketch_entities.len(),
            model.ir.model.sketch_constraints.len(),
            model.ir.model.parameters.len(),
            model.ir.model.feature_input_topologies.len(),
            model.ir.design.features.len(),
        );
        let mapped = if suppressed {
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
                &mut model.ir.design,
                report,
            )
            .and_then(|definition| {
                serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(
                    definition.clone(),
                )
                .map_err(|e| format!("the resolved construction is not supported yet ({e})"))?;
                Ok(definition)
            })
        };
        let definition = match mapped {
            Ok(definition) => {
                if !suppressed
                    && !matches!(
                        feature.get("featureType").and_then(Value::as_str),
                        Some("newSketch" | "assignVariable")
                    )
                {
                    features::report_evaluated_variables(feature, &model.ir.model, report);
                }
                definition
            }
            Err(reason) => {
                model.ir.model.sketches.truncate(sizes.0);
                model.ir.model.sketch_entities.truncate(sizes.1);
                model.ir.model.sketch_constraints.truncate(sizes.2);
                model.ir.model.parameters.truncate(sizes.3);
                model.ir.model.feature_input_topologies.truncate(sizes.4);
                model.ir.design.features.truncate(sizes.5);
                if !suppressed {
                    let region_budget_exhausted = inputs
                        .regions
                        .try_borrow()
                        .is_ok_and(|recovery| recovery.exhausted());
                    let code = if !region_budget_exhausted
                        && feature.get("featureType").and_then(Value::as_str) == Some("extrude")
                        && features::value(feature, "bodyType") == Some("THIN")
                    {
                        OnshapeLossCode::ThinNotSupported
                    } else {
                        OnshapeLossCode::HistoryNotSupported
                    };
                    loss(report, name, &reason, model.source_step.is_some(), code);
                }
                definition(
                    "native",
                    json!({"kind": format!("Onshape {}: {reason}", feature.get("featureType").and_then(Value::as_str).unwrap_or("feature")), "parameters": {}}),
                )
            }
        };
        let mut source_properties = features::source_properties(feature, &definition);
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
        let dependencies: Vec<_> = previous.iter().cloned().collect();
        let outputs: Vec<_> = single_body
            .iter()
            .chain(part_bodies.get(&ordinal))
            .filter(|_| {
                definition
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
        push(&mut model.ir.model.features, json!({"id": feature_id, "ordinal": ordinal, "name": name, "suppressed": suppressed, "dependencies": dependencies, "source_properties": source_properties, "source_content": [], "outputs": outputs, "definition": definition})).map_err(Error::Decode)?;
        ctx.admit_entities(
            cadmpeg_core::decode::u64_from_index(model.ir.entity_count()),
            &mut admitted,
            "Onshape history",
        )?;
        if !suppressed {
            previous = Some(feature_id);
        }
    }
    annotate(&mut model, features)?;
    report
        .notes
        .extend(bundle.retain_assets(ctx, &mut model.ir)?);
    model.ir.finalize();
    Ok(model)
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
    for source in features {
        let native = text(source, "featureId").map_err(Error::Decode)?;
        let sketch_id = id("sketch", native);
        if model
            .ir
            .model
            .sketches
            .iter()
            .any(|s| s.id.as_str() == sketch_id)
        {
            let queries = features::queries(source, "sketchPlane").map_err(Error::Decode)?;
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
