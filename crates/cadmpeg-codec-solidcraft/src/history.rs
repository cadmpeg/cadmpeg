// SPDX-License-Identifier: Apache-2.0
//! The feature history: features, sketches, and their order.
//!
//! Each feature record becomes one neutral feature, in document order, with
//! its name and suppression. A sketch becomes a neutral sketch on its plane
//! with its curves as entities. Extrude, revolve, and the box, cylinder,
//! cone, sphere, and torus primitives carry their operation: profile,
//! direction, extent, axis, dimensions, placement, and boolean. Lengths and
//! angles are their expressions evaluated over the design's parameters.
//! Every other feature, and one whose inputs do not resolve, is a native
//! operation of its type that keeps its scalar fields; its record stays in
//! the retained document. A feature that made a body of the final cached
//! state names that body as an output.

mod assembly;
mod constraints;
mod expr;
mod fit;
mod operations;
mod sketch;

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::Feature;
use cadmpeg_ir::ids::{BodyId, IdentityKey};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::{Sketch, SketchEntity};
use cadmpeg_ir::Exactness;
use serde_json::{json, Map, Value};

use crate::loss::SolidcraftLossCode;
use expr::{Kind, Scope};
use sketch::{dot, point_json, Frame, V3};

/// What decoding the history gave.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    /// The losses of the history.
    pub(crate) losses: Vec<LossNote>,
}

/// The identity of a design record of `kind` keyed by `key`.
fn id(kind: &'static str, key: &str) -> String {
    let namespace = match kind {
        "feature" => cadmpeg_ir::identity_namespace!("solidcraft", "design", "feature"),
        "sketch" => cadmpeg_ir::identity_namespace!("solidcraft", "design", "sketch"),
        "parameter" => cadmpeg_ir::identity_namespace!("solidcraft", "design", "parameter"),
        "product" => cadmpeg_ir::identity_namespace!("solidcraft", "design", "product"),
        "occurrence" => cadmpeg_ir::identity_namespace!("solidcraft", "design", "occurrence"),
        "joint" => cadmpeg_ir::identity_namespace!("solidcraft", "design", "joint"),
        "constraint" => {
            cadmpeg_ir::identity_namespace!("solidcraft", "design", "sketch-constraint")
        }
        _ => cadmpeg_ir::identity_namespace!("solidcraft", "design", "sketch-entity"),
    };
    cadmpeg_ir::features::FeatureId::compose(&namespace, IdentityKey::encode_segment(key))
        .into_string()
}

fn malformed(error: impl std::fmt::Display) -> CodecError {
    CodecError::malformed(format_args!("solidcraft history record: {error}"))
}

/// Counts of the history's untransferred content.
#[derive(Default)]
struct Tally {
    native: BTreeMap<String, usize>,
    unresolved: usize,
    curves: usize,
    planes: usize,
    native_constraints: usize,
    dropped_constraints: usize,
}

/// A sketch the later features can use: its frame, points, and record.
struct SketchUse {
    frame: Frame,
    points: Vec<Option<sketch::SketchPoint>>,
    body: Value,
    /// The ids of the curves that are neutral entities.
    entities: BTreeSet<String>,
}

/// The state of one walk over the feature records.
struct Walk<'w, 'c> {
    ctx: &'w DecodeContext<'c>,
    scope: Scope,
    ir: &'w mut CadIr,
    annotations: &'w mut AnnotationBuilder,
    tally: Tally,
    sketches: BTreeMap<u64, SketchUse>,
    planes: BTreeMap<u64, Frame>,
    /// The feature that owns each model parameter, by parameter name.
    owners: BTreeMap<String, u64>,
    /// The parameters of dimensions without a design parameter.
    dimension_parameters: Vec<Value>,
    /// Each sketch feature's neutral sketch id and component.
    sketch_owners: Vec<(String, u64)>,
}

/// Decodes the feature history of `document` into `ir`. `outputs` maps a
/// feature id to the bodies of the final state that it made.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    document: &Map<String, Value>,
    version: u32,
    outputs: &BTreeMap<u64, Vec<BodyId>>,
    components: &BTreeMap<u64, Vec<BodyId>>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<Outcome, CodecError> {
    let mut outcome = Outcome::default();
    let migrated = crate::migrate::upgrade(version, document);
    let document = migrated.as_ref().unwrap_or(document);
    let Some(features) = document.get("features").and_then(Value::as_array) else {
        return Ok(outcome);
    };
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(features.len()),
        "solidcraft feature history",
    )?;
    let params = document
        .get("params")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let mut walk = Walk {
        ctx,
        scope: Scope::of(params),
        ir,
        tally: Tally::default(),
        sketches: BTreeMap::new(),
        planes: BTreeMap::new(),
        owners: BTreeMap::new(),
        dimension_parameters: Vec::new(),
        annotations,
        sketch_owners: Vec::new(),
    };
    for (ordinal, record) in features.iter().enumerate() {
        let Some(feature_id) = record.get("id").and_then(Value::as_u64) else {
            continue;
        };
        walk.feature(ordinal, feature_id, record, outputs.get(&feature_id))?;
    }
    walk.parameters(params)?;
    let sketch_owners = std::mem::take(&mut walk.sketch_owners);
    let assembled = walk.assembly(
        document,
        &assembly::Owned {
            bodies: components,
            sketches: &sketch_owners,
        },
    )?;
    outcome.losses.extend(assembled.losses);
    let tally = walk.tally;
    if !tally.native.is_empty() {
        let total: usize = tally.native.values().sum();
        outcome
            .losses
            .push(SolidcraftLossCode::HistoryUndecoded.note(format!(
            "{total} feature records are native operations without neutral semantics{}: {}",
            if tally.unresolved > 0 {
                format!(
                    ", {} of them because their inputs do not resolve",
                    tally.unresolved
                )
            } else {
                String::new()
            },
            tally
                .native
                .iter()
                .map(|(kind, count)| format!("{count} {kind}"))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if tally.curves > 0 {
        outcome
            .losses
            .push(SolidcraftLossCode::SketchCurveUntyped.note(format!(
                "{} sketch curves (periodic splines, splines with tangent handles) have no \
             neutral entity and stay in the retained document",
                tally.curves
            )));
    }
    if tally.native_constraints + tally.dropped_constraints > 0 {
        outcome
            .losses
            .push(SolidcraftLossCode::SketchConstraintNative.note(format!(
                "{} sketch constraints have no neutral family and are native relations{}",
                tally.native_constraints,
                if tally.dropped_constraints > 0 {
                    format!(
                        "; {} more refer to geometry without a neutral entity and stay in the \
                         retained document",
                        tally.dropped_constraints
                    )
                } else {
                    String::new()
                }
            )));
    }
    if tally.planes > 0 {
        outcome
            .losses
            .push(SolidcraftLossCode::SketchPlaneUnresolved.note(format!(
                "{} sketches lie on a plane that depends on the model (a datum or a midplane); \
             their placement is unresolved",
                tally.planes
            )));
    }
    Ok(outcome)
}

/// How far past a full turn a revolve angle may round and still be one.
const FULL_TURN_ROUNDING: f64 = 1.0e-12;

/// The operation kinds this decoder gives neutral semantics.
const TYPED: &[&str] = &[
    "sketch",
    "extrude",
    "revolve",
    "box",
    "cylinder",
    "cone",
    "sphere",
    "torus",
    "fillet",
    "chamfer",
    "shell",
    "move",
    "remove",
    "combine",
    "scale",
    "pattern",
    "mirror",
    "sweep",
    "loft",
    "construction_plane",
    "coil",
    "offset_face",
    "delete_face",
    "hole",
    "split_body",
    "split_face",
    "body_transfer",
    "draft",
    "thread",
];

impl Walk<'_, '_> {
    fn feature(
        &mut self,
        ordinal: usize,
        feature_id: u64,
        record: &Value,
        outputs: Option<&Vec<BodyId>>,
    ) -> Result<(), CodecError> {
        let kind = record
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let key = feature_id.to_string();
        let mut dependencies = BTreeSet::new();
        for name in record
            .get("param_names")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
        {
            if let Some(name) = name.as_str() {
                self.owners.entry(name.to_owned()).or_insert(feature_id);
            }
        }
        if let Some(object) = record.as_object() {
            for (field, value) in object {
                if field != "sketch" {
                    self.claim(value, feature_id);
                }
            }
        }
        if kind == "construction_plane" {
            if let Some(frame) = record
                .get("plane")
                .and_then(|plane| sketch::plane(plane, &self.scope, &self.planes, 0))
            {
                self.planes.insert(feature_id, frame);
            }
        }
        let operation = match kind {
            "sketch" => self.sketch(record, &key, feature_id)?,
            "extrude" => extrude(record, &self.scope, &self.sketches, &mut dependencies),
            "revolve" => revolve(record, &self.scope, &self.sketches, &mut dependencies),
            "box" | "cylinder" | "cone" | "sphere" | "torus" => {
                primitive(kind, record, &self.scope)
            }
            _ => operations::Inputs {
                scope: &self.scope,
                planes: &self.planes,
                sketches: &self.sketches,
            }
            .operation(kind, record, &mut dependencies),
        };
        let neutral = operation.is_some();
        let fields = Fields {
            ordinal,
            key: &key,
            kind,
            record,
            dependencies: &dependencies,
            outputs,
        };
        let mut feature = fields.json(&operation.unwrap_or_else(|| native(kind, record)));
        // An operation the neutral model refuses stays native.
        let mut admitted: Result<Feature, _> = serde_json::from_value(feature.clone());
        if admitted.is_err() && neutral {
            feature["definition"]["value"] = native(kind, record);
            feature["dependencies"] = json!([]);
            admitted = serde_json::from_value(feature);
        }
        let admitted = admitted.map_err(malformed)?;
        let is_native = matches!(
            admitted.evaluation.definition(),
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Native { .. }
            )
        );
        if is_native {
            if TYPED.contains(&kind) {
                self.tally.unresolved += 1;
            }
            *self.tally.native.entry(kind.to_owned()).or_default() += 1;
        }
        self.ir.model.features.push(admitted);
        Ok(())
    }

    fn sketch(
        &mut self,
        record: &Value,
        key: &str,
        feature_id: u64,
    ) -> Result<Option<Value>, CodecError> {
        let Some(body) = record.get("sketch") else {
            return Ok(None);
        };
        let sketch_id = id("sketch", key);
        self.sketch_owners.push((
            sketch_id.clone(),
            record.get("component").and_then(Value::as_u64).unwrap_or(0),
        ));
        let frame = record
            .get("plane")
            .and_then(|plane| sketch::plane(plane, &self.scope, &self.planes, 0));
        // A frame the upgrade of an earlier format wrote is derived from the
        // format's own rule, so its placement says so.
        let upgraded = record
            .get("plane")
            .and_then(|plane| plane.get("migrated"))
            .is_some();
        if frame.is_none() {
            self.tally.planes += 1;
        }
        let points = sketch::points(body);
        let curves = body
            .get("curves")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        self.ctx.charge_entities(
            cadmpeg_core::decode::u64_from_index(curves.len() + points.len()),
            "solidcraft sketch",
        )?;
        let neutral: Sketch = serde_json::from_value(json!({
            "id": sketch_id,
            "name": record.get("name").and_then(Value::as_str),
            "placement": frame
                .as_ref()
                .map_or_else(|| json!({"kind": "unresolved"}), Frame::placement),
            "profiles": [],
        }))
        .map_err(malformed)?;
        self.ir.model.sketches.push(neutral);
        if upgraded {
            self.annotations
                .field_exactness(&sketch_id, "placement", Exactness::Derived)?;
        }
        let mut used = BTreeSet::new();
        let mut loci = constraints::Loci::default();
        let mut typed = BTreeSet::new();
        let point_ids: Vec<Option<String>> = points
            .iter()
            .map(|point| point.as_ref().map(|point| point.id.clone()))
            .collect();
        for (index, curve) in (0_u64..).zip(curves) {
            for field in ["a", "b", "c", "m", "apex"] {
                if let Some(index) = curve.get(field).and_then(Value::as_u64) {
                    used.insert(index);
                }
            }
            if let Some(pts) = curve.get("pts").and_then(Value::as_array) {
                used.extend(pts.iter().filter_map(Value::as_u64));
            }
            let entity = curve
                .get("id")
                .and_then(Value::as_str)
                .zip(sketch::curve_geometry(curve, &points));
            let Some((curve_id, geometry)) = entity else {
                self.tally.curves += 1;
                continue;
            };
            let construction = ["construction", "centerline", "infinite"]
                .iter()
                .any(|flag| curve.get(*flag).and_then(Value::as_bool) == Some(true));
            let entity_key = format!("{key}:{curve_id}");
            let refs = constraints::endpoint_refs(curve, &point_ids);
            if push_entity(
                self.ir,
                &sketch_id,
                &entity_key,
                construction,
                &refs,
                &geometry,
            ) {
                loci.curve(index, &id("entity", &entity_key), curve);
                typed.insert(curve_id.to_owned());
            } else {
                self.tally.curves += 1;
            }
        }
        for (index, point) in (0_u64..).zip(&points) {
            let Some(point) = point else {
                continue;
            };
            if point.id == "origin" || used.contains(&index) {
                continue;
            }
            let geometry =
                json!({"kind": "point", "position": {"u": point.pos[0], "v": point.pos[1]}});
            let entity_key = format!("{key}:{}", point.id);
            if push_entity(self.ir, &sketch_id, &entity_key, false, &[], &geometry) {
                loci.point(index, &id("entity", &entity_key));
            } else {
                self.tally.curves += 1;
            }
        }
        let relations = constraints::SketchScope {
            key,
            sketch: &sketch_id,
            loci: &loci,
            scope: &self.scope,
        }
        .relations(body);
        if relations.origin {
            let geometry = json!({"kind": "point", "position": {"u": 0.0, "v": 0.0}});
            push_entity(
                self.ir,
                &sketch_id,
                &format!("{key}:origin"),
                true,
                &[],
                &geometry,
            );
        }
        self.ir
            .model
            .sketch_constraints
            .extend(relations.constraints);
        self.dimension_parameters.extend(relations.parameters);
        for name in relations.driven {
            self.owners.entry(name).or_insert(feature_id);
        }
        self.tally.native_constraints += relations.native;
        self.tally.dropped_constraints += relations.dropped;
        if let Some(frame) = frame {
            self.sketches.insert(
                feature_id,
                SketchUse {
                    frame,
                    points,
                    body: body.clone(),
                    entities: typed,
                },
            );
        }
        Ok(Some(
            json!({"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": sketch_id}}}),
        ))
    }
}

impl Walk<'_, '_> {
    /// Records `feature_id` as the owner of each model parameter that a
    /// field of its record names as its whole expression.
    fn claim(&mut self, value: &Value, feature_id: u64) {
        match value {
            Value::String(text) => {
                if self.scope.knows(text) {
                    self.owners.entry(text.clone()).or_insert(feature_id);
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.claim(item, feature_id);
                }
            }
            Value::Object(object) => {
                for item in object.values() {
                    self.claim(item, feature_id);
                }
            }
            _ => {}
        }
    }

    /// Adds the design parameters: the document's `params` records, then
    /// the parameters of dimensions without one. A model parameter that a
    /// feature names is owned by that feature.
    fn parameters(&mut self, params: &[Value]) -> Result<(), CodecError> {
        let mut ordinals: BTreeMap<Option<String>, u32> = BTreeMap::new();
        let mut next = |owner: Option<String>| -> Result<u32, CodecError> {
            let ordinal = ordinals.entry(owner).or_insert(0);
            let current = *ordinal;
            *ordinal = current
                .checked_add(1)
                .ok_or_else(|| malformed("parameter ordinal overflow"))?;
            Ok(current)
        };
        let mut records = Vec::new();
        for param in params {
            let (Some(name), Some(expression)) = (
                param.get("name").and_then(Value::as_str),
                param.get("expr").and_then(Value::as_str),
            ) else {
                continue;
            };
            let model = param.get("model").and_then(Value::as_bool) == Some(true);
            let owner = model
                .then(|| self.owners.get(name))
                .flatten()
                .map(|feature| id("feature", &feature.to_string()));
            let value = self
                .scope
                .parameter_value(name)
                .map(|(value, kind)| match kind {
                    Kind::Length => json!({"kind": "length", "value": value}),
                    Kind::Angle => json!({"kind": "angle", "value": value}),
                    Kind::Unitless => json!({"kind": "real", "value": value}),
                });
            let mut properties = BTreeMap::new();
            for field in ["unit", "comment"] {
                if let Some(text) = param.get(field).and_then(Value::as_str) {
                    if !text.is_empty() {
                        properties.insert(field, text.to_owned());
                    }
                }
            }
            if model {
                properties.insert("model", "true".to_owned());
            }
            records.push(json!({
                "id": id("parameter", name),
                "owner": owner,
                "ordinal": next(owner.clone())?,
                "name": name,
                "expression": expression,
                "value": value,
                "dependencies": self
                    .scope
                    .references(expression)
                    .iter()
                    .filter(|reference| reference.as_str() != name)
                    .map(|reference| id("parameter", reference))
                    .collect::<Vec<_>>(),
                "properties": properties,
            }));
        }
        let mut dimension_ids = BTreeSet::new();
        for mut record in std::mem::take(&mut self.dimension_parameters) {
            let owner = record["owner"].as_str().map(str::to_owned);
            record["ordinal"] = json!(next(owner)?);
            if let Some(id) = record["id"].as_str() {
                dimension_ids.insert(id.to_owned());
            }
            records.push(record);
        }
        self.ctx.charge_entities(
            cadmpeg_core::decode::u64_from_index(records.len()),
            "solidcraft parameters",
        )?;
        for mut record in records {
            if let Some(object) = record.as_object_mut() {
                object.retain(|_, value| !value.is_null());
            }
            let parameter_id = record["id"].as_str().unwrap_or_default().to_owned();
            let from_dimension = dimension_ids.contains(&parameter_id);
            let evaluated = record
                .get("dependencies")
                .and_then(Value::as_array)
                .is_some_and(|d| !d.is_empty());
            let parameter: cadmpeg_ir::features::DesignParameter =
                serde_json::from_value(record).map_err(malformed)?;
            if from_dimension {
                self.annotations.field_exactness(
                    &parameter_id,
                    "expression",
                    Exactness::Inferred,
                )?;
            } else if evaluated {
                self.annotations
                    .field_exactness(&parameter_id, "value", Exactness::Derived)?;
            }
            self.ir.model.parameters.push(parameter);
        }
        Ok(())
    }
}

/// The fields of one neutral feature record.
struct Fields<'f> {
    ordinal: usize,
    key: &'f str,
    kind: &'f str,
    record: &'f Value,
    dependencies: &'f BTreeSet<u64>,
    outputs: Option<&'f Vec<BodyId>>,
}

impl Fields<'_> {
    /// The wire record of the feature with `operation`.
    fn json(&self, operation: &Value) -> Value {
        json!({
            "id": id("feature", self.key),
            "ordinal": self.ordinal,
            "name": self.record.get("name").and_then(Value::as_str),
            "suppressed": self.record.get("suppressed").and_then(Value::as_bool),
            "dependencies": self
                .dependencies
                .iter()
                .map(|dependency| id("feature", &dependency.to_string()))
                .collect::<Vec<_>>(),
            "source_properties": {},
            "source_tag": self.kind,
            "source_content": [],
            "definition": {"kind": "operation", "value": operation},
            "outputs": self
                .outputs
                .map(|bodies| bodies.iter().map(ToString::to_string).collect::<Vec<_>>())
                .unwrap_or_default(),
        })
    }
}

/// Adds a sketch entity; `false` when the neutral model refuses it.
fn push_entity(
    ir: &mut CadIr,
    sketch_id: &str,
    key: &str,
    construction: bool,
    endpoint_refs: &[String],
    geometry: &Value,
) -> bool {
    let entity: Result<SketchEntity, _> = serde_json::from_value(json!({
        "id": id("entity", key),
        "sketch": sketch_id,
        "construction": construction,
        "endpoint_refs": endpoint_refs,
        "geometry": geometry,
    }));
    match entity {
        Ok(entity) => {
            ir.model.sketch_entities.push(entity);
            true
        }
        Err(_) => false,
    }
}

fn boolean(record: &Value) -> &'static str {
    match record.get("operation").and_then(Value::as_str) {
        Some("join") => "join",
        Some("cut") => "cut",
        Some("intersect") => "intersect",
        _ => "new_body",
    }
}

/// The profile of an extrude or revolve on the sketch of `sketch`.
fn profile(record: &Value, sketch: u64) -> Value {
    let sketch_id = id("sketch", &sketch.to_string());
    match record
        .get("profiles")
        .and_then(|profiles| profiles.get("type"))
        .and_then(Value::as_str)
    {
        None | Some("all") => json!({"kind": "sketch", "value": sketch_id}),
        Some(_) => json!({"kind": "sketch_selection", "value": {
            "sketch": sketch_id,
            "selections": [record.get("profiles").map(ToString::to_string).unwrap_or_default()],
        }}),
    }
}

fn blind(length: f64) -> Value {
    json!({"termination": {"kind": "blind", "length": length}})
}

fn extrude(
    record: &Value,
    scope: &Scope,
    sketches: &BTreeMap<u64, SketchUse>,
    dependencies: &mut BTreeSet<u64>,
) -> Option<Value> {
    let sketch = record.get("sketch")?.as_u64()?;
    if record.get("thin").is_some_and(|thin| !thin.is_null()) || !sketches.contains_key(&sketch) {
        return None;
    }
    dependencies.insert(sketch);
    let extent = record.get("extent")?;
    let through_all = extent.get("through_all").and_then(Value::as_bool) == Some(true);
    let to_object = extent.get("to").is_some_and(|to| !to.is_null());
    let mut reversed = extent.get("direction").and_then(Value::as_str) == Some("negative");
    let symmetric = extent.get("direction").and_then(Value::as_str) == Some("symmetric");
    let total = extent.get("convention").and_then(Value::as_str) == Some("total_distance");
    let termination = |distance: Option<f64>| -> Option<Value> {
        Some(if through_all {
            json!({"termination": {"kind": "through_all"}})
        } else if to_object {
            json!({"termination": {"kind": "unresolved"}})
        } else {
            blind(distance?.abs())
        })
    };
    let distance = if through_all || to_object {
        None
    } else {
        let distance = scope.eval_json(extent.get("distance")?, Kind::Length)?;
        if distance == 0.0 {
            return None;
        }
        Some(distance)
    };
    if distance.is_some_and(|distance| distance < 0.0) && !symmetric {
        reversed = !reversed;
    }
    let second = extent
        .get("distance2")
        .filter(|value| !value.is_null())
        .map(|value| scope.eval_json(value, Kind::Length));
    let extent = if symmetric {
        let total_length = distance.map(|distance| if total { distance } else { 2.0 * distance });
        json!({"kind": "symmetric", "side": termination(total_length)?})
    } else if let Some(second) = second {
        let second = second?;
        if second == 0.0 {
            return None;
        }
        match distance {
            Some(first) => {
                // Direction fixes the sides, and a sign only turns its own
                // side over; the second side follows the first across the
                // sketch plane only for a design that says so.
                let negative = extent.get("direction").and_then(Value::as_str) == Some("negative");
                let first = if negative { -first } else { first };
                let follows =
                    extent.get("second_follows_first").and_then(Value::as_bool) == Some(true);
                let other = if follows {
                    if first < 0.0 {
                        second
                    } else {
                        -second
                    }
                } else if negative {
                    second
                } else {
                    -second
                };
                if other < 0.0 && first > 0.0 {
                    reversed = false;
                    json!({"kind": "two_sided", "first": blind(first), "second": blind(-other)})
                } else if other > 0.0 && first < 0.0 {
                    reversed = true;
                    json!({"kind": "two_sided", "first": blind(-first), "second": blind(other)})
                } else {
                    return None;
                }
            }
            None => {
                json!({"kind": "two_sided", "first": termination(distance)?, "second": blind(second.abs())})
            }
        }
    } else {
        json!({"kind": "one_sided", "side": termination(distance)?})
    };
    let start = start(record, scope)?;
    Some(json!({"kind": "extrude", "value": {
        "profile": {"kind": "planar", "value": profile(record, sketch)},
        "direction": {"kind": if reversed { "reversed_profile_normal" } else { "profile_normal" }},
        "start": start,
        "extent": extent,
        "op": boolean(record),
    }}))
}

/// The start of an extrude: its profile plane, or that plane at its start
/// offset. A From Object start, or an offset that does not resolve, has no
/// neutral start.
fn start(record: &Value, scope: &Scope) -> Option<Value> {
    let extent = record.get("extent")?;
    if extent.get("from").is_some_and(|from| !from.is_null()) {
        return None;
    }
    Some(match extent.get("start_offset") {
        None | Some(Value::Null) => json!({"kind": "profile_plane"}),
        Some(offset) => {
            let offset = scope.eval_json(offset, Kind::Length)?;
            json!({"kind": "offset_profile_plane", "offset": offset})
        }
    })
}

/// The model-space axis of a revolve.
fn revolve_axis(
    axis: &Value,
    frame: &Frame,
    body: &Value,
    points: &[Option<sketch::SketchPoint>],
) -> Option<(V3, V3)> {
    match axis.get("type")?.as_str()? {
        "sketch_axis" => match axis.get("axis")?.as_str()? {
            "x" => Some((frame.origin, frame.x)),
            "y" => Some((frame.origin, frame.y)),
            _ => None,
        },
        "world" => {
            let direction = match axis.get("axis")?.as_str()?.to_ascii_lowercase().as_str() {
                "x" => [1.0, 0.0, 0.0],
                "y" => [0.0, 1.0, 0.0],
                "z" => [0.0, 0.0, 1.0],
                _ => return None,
            };
            Some(([0.0; 3], direction))
        }
        "sketch_line" => {
            if axis.get("sketch").is_some_and(|sketch| !sketch.is_null()) {
                return None;
            }
            let name = axis.get("curve")?.as_str()?;
            let curve = body
                .get("curves")?
                .as_array()?
                .iter()
                .find(|curve| curve.get("id").and_then(Value::as_str) == Some(name))?;
            let at = |key: &str| -> Option<[f64; 2]> {
                Some(
                    points
                        .get(usize::try_from(curve.get(key)?.as_u64()?).ok()?)?
                        .as_ref()?
                        .pos,
                )
            };
            let (start, end) = (at("a")?, at("b")?);
            Some((
                frame.world(start),
                frame.direction([end[0] - start[0], end[1] - start[1]]),
            ))
        }
        _ => None,
    }
}

fn revolve(
    record: &Value,
    scope: &Scope,
    sketches: &BTreeMap<u64, SketchUse>,
    dependencies: &mut BTreeSet<u64>,
) -> Option<Value> {
    let sketch = record.get("sketch")?.as_u64()?;
    let SketchUse {
        frame,
        points,
        body,
        ..
    } = sketches.get(&sketch)?;
    if record.get("to").is_some_and(|to| !to.is_null()) {
        return None;
    }
    dependencies.insert(sketch);
    let (origin, axis) = revolve_axis(record.get("axis")?, frame, body, points)?;
    // The sweep leaves the profile along the sketch normal: the rotation
    // axis is the axis or its reverse, whichever turns the profile's centre
    // toward the normal.
    let profile_points: Vec<V3> = points
        .iter()
        .flatten()
        .filter(|point| point.id != "origin")
        .map(|point| frame.world(point.pos))
        .collect();
    let count = profile_points.iter().fold(0.0, |count, _| count + 1.0);
    if count == 0.0 {
        return None;
    }
    let centre = profile_points.iter().fold([0.0; 3], |sum, point| {
        [
            sum[0] + point[0] / count,
            sum[1] + point[1] / count,
            sum[2] + point[2] / count,
        ]
    });
    let length = dot(axis, axis).sqrt();
    if length.is_nan() || length <= 0.0 {
        return None;
    }
    let unit = [axis[0] / length, axis[1] / length, axis[2] / length];
    let offset = [
        centre[0] - origin[0],
        centre[1] - origin[1],
        centre[2] - origin[2],
    ];
    let along = dot(offset, unit);
    let radial = [
        offset[0] - unit[0] * along,
        offset[1] - unit[1] * along,
        offset[2] - unit[2] * along,
    ];
    let rotation = if dot(sketch::cross(unit, radial), frame.normal()) < 0.0 {
        [-unit[0], -unit[1], -unit[2]]
    } else {
        unit
    };
    let angle = scope.eval_json(record.get("angle")?, Kind::Angle)?;
    let turn = |angle: f64| {
        (angle > 0.0 && angle <= TAU + FULL_TURN_ROUNDING)
            .then(|| json!({"kind": "angle", "angle": angle.min(TAU)}))
    };
    let extent = if record.get("symmetric").and_then(Value::as_bool) == Some(true) {
        json!({"kind": "symmetric", "termination": turn(angle)?})
    } else if let Some(second) = record.get("angle2").filter(|value| !value.is_null()) {
        let second = scope.eval_json(second, Kind::Angle)?;
        json!({"kind": "two_sided", "first": turn(angle)?, "second": turn(second)?})
    } else {
        json!({"kind": "one_sided", "termination": turn(angle)?})
    };
    Some(json!({"kind": "revolve", "value": {
        "construction": {"kind": "resolved", "value": {
            "profile": profile(record, sketch),
            "axis": {"origin": point_json(origin), "direction": point_json(rotation)},
            "extent": extent,
        }},
        "op": boolean(record),
    }}))
}

/// A placement whose z axis is `axis` and whose origin is `origin`.
fn placement(origin: V3, axis: V3) -> Option<Value> {
    let length = dot(axis, axis).sqrt();
    if !length.is_finite() || length <= 0.0 {
        return None;
    }
    let z = [axis[0] / length, axis[1] / length, axis[2] / length];
    let helper = if z[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let x = {
        let along = dot(helper, z);
        let raw = [
            helper[0] - z[0] * along,
            helper[1] - z[1] * along,
            helper[2] - z[2] * along,
        ];
        let norm = dot(raw, raw).sqrt();
        [raw[0] / norm, raw[1] / norm, raw[2] / norm]
    };
    let y = sketch::cross(z, x);
    Some(json!([
        [x[0], y[0], z[0], origin[0]],
        [x[1], y[1], z[1], origin[1]],
        [x[2], y[2], z[2], origin[2]],
    ]))
}

fn primitive(kind: &str, record: &Value, scope: &Scope) -> Option<Value> {
    let length = |key: &str| {
        scope
            .eval_json(record.get(key)?, Kind::Length)
            .filter(|value| *value > 0.0)
    };
    let (solid, placement) = match kind {
        "box" => {
            let (l, w, h) = (length("length")?, length("width")?, length("height")?);
            let mut corner = sketch::point(record.get("corner"), scope)?;
            if record.get("centred").and_then(Value::as_bool) == Some(true) {
                corner = [
                    corner[0] - l / 2.0,
                    corner[1] - w / 2.0,
                    corner[2] - h / 2.0,
                ];
            }
            (
                json!({"kind": "box", "length": l, "width": w, "height": h}),
                json!([
                    [1.0, 0.0, 0.0, corner[0]],
                    [0.0, 1.0, 0.0, corner[1]],
                    [0.0, 0.0, 1.0, corner[2]]
                ]),
            )
        }
        "cylinder" => (
            json!({"kind": "cylinder", "radius": length("radius")?, "height": length("height")?, "angle": TAU}),
            placement(
                sketch::point(record.get("base"), scope)?,
                sketch::direction(record.get("axis"))?,
            )?,
        ),
        "cone" => {
            let top = scope
                .eval_json(record.get("top_radius")?, Kind::Length)
                .filter(|value| *value >= 0.0)?;
            (
                json!({"kind": "cone", "radius1": length("radius")?, "radius2": top, "height": length("height")?, "angle": TAU}),
                placement(
                    sketch::point(record.get("base"), scope)?,
                    sketch::direction(record.get("axis"))?,
                )?,
            )
        }
        "sphere" => (
            json!({"kind": "sphere", "radius": length("radius")?, "latitude1": -FRAC_PI_2, "latitude2": FRAC_PI_2, "longitude": TAU}),
            placement(sketch::point(record.get("center"), scope)?, [0.0, 0.0, 1.0])?,
        ),
        "torus" => (
            json!({"kind": "torus", "major_radius": length("major")?, "minor_radius": length("minor")?, "latitude1": -PI, "latitude2": PI, "longitude": TAU}),
            placement(sketch::point(record.get("center"), scope)?, [0.0, 0.0, 1.0])?,
        ),
        _ => return None,
    };
    Some(
        json!({"kind": "primitive", "value": {"solid": solid, "placement": placement, "op": boolean(record)}}),
    )
}

/// The longest list or record that a native operation keeps as text.
const NATIVE_TEXT_LIMIT: usize = 1024;

/// A native operation of `kind` with the record's scalar fields and its
/// short lists.
fn native(kind: &str, record: &Value) -> Value {
    let mut parameters = BTreeMap::new();
    if let Some(object) = record.as_object() {
        for (key, value) in object {
            if matches!(key.as_str(), "id" | "name" | "type" | "suppressed") || key.is_empty() {
                continue;
            }
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Number(_) | Value::Bool(_) => value.to_string(),
                // A list or a record is kept as its text when it is short.
                Value::Array(_) | Value::Object(_) => {
                    let text = value.to_string();
                    if text.len() > NATIVE_TEXT_LIMIT {
                        continue;
                    }
                    text
                }
                Value::Null => continue,
            };
            parameters.insert(key.clone(), text);
        }
    }
    json!({"kind": "native", "value": {"kind": format!("solidcraft:{kind}"), "parameters": parameters}})
}
