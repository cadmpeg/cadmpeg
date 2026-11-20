// SPDX-License-Identifier: Apache-2.0
//! Components, occurrences, and joints.
//!
//! The design is its root component, product 0, the definition of the root
//! file. Each other component is a product definition that owns the bodies,
//! sketches, and features that were authored in its frame. An occurrence
//! places a component with a rigid transform and belongs to the definition
//! of the component it sits in, in source order. One occurrence stands for
//! each placement, however many times the component that holds it is itself
//! placed. A placement of a component the document does not hold, or one that
//! would make a component contain itself, is not placed. A hidden path of
//! occurrences hides the outermost occurrence, or overrides the nested one.
//!
//! A joint mates a frame of one occurrence to a frame of another. An
//! explicit frame or point snap is a connector frame; a snap on geometry
//! (the centre of a face or of a circular edge) has no frame without
//! evaluating the model, so its joint stays in the retained document. A
//! grounded occurrence is a grounded connector of its own.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::products::{AssemblyJoint, Occurrence, ProductDefinition};
use cadmpeg_ir::report::loss::LossNote;
use serde_json::{json, Map, Value};

use super::sketch::{cross, dot, V3};
use super::{id, malformed};
use crate::loss::SolidcraftLossCode;

/// How far from zero a frame axis may be and still be absent.
const AXIS_EPSILON: f64 = 1.0e-12;
/// How far from zero a transform's last column may be.
const AFFINE_EPSILON: f64 = 1.0e-9;

/// What the assembly decode gave.
#[derive(Debug, Default)]
pub(super) struct Outcome {
    /// The losses of the assembly.
    pub(super) losses: Vec<LossNote>,
    /// The placements that name no component or would make a component
    /// contain itself.
    pub(super) unplaced: usize,
}

/// Whether `from` places `to`, directly or through the components it places.
fn reaches(places: &BTreeMap<u64, BTreeSet<u64>>, from: u64, to: u64) -> bool {
    let mut seen = BTreeSet::new();
    let mut pending = vec![from];
    while let Some(component) = pending.pop() {
        if component == to {
            return true;
        }
        if seen.insert(component) {
            pending.extend(places.get(&component).into_iter().flatten().copied());
        }
    }
    false
}

/// What the assembly reads of the walk over the features.
pub(super) struct Owned<'o> {
    /// The bodies each component owns.
    pub(super) bodies: &'o BTreeMap<u64, Vec<BodyId>>,
    /// Each sketch feature's neutral sketch id and component.
    pub(super) sketches: &'o [(String, u64)],
}

fn component_of(record: &Value) -> u64 {
    record.get("component").and_then(Value::as_u64).unwrap_or(0)
}

/// The rows of a column-major 4×4 affine transform, as the neutral 3×4.
pub(super) fn rows(matrix: &Value) -> Option<Value> {
    let columns: Vec<Vec<f64>> = matrix
        .as_array()?
        .iter()
        .map(|column| column.as_array()?.iter().map(Value::as_f64).collect())
        .collect::<Option<_>>()?;
    let [x, y, z, t] = columns.as_slice() else {
        return None;
    };
    if [x, y, z, t].iter().any(|column| column.len() != 4)
        || [x[3], y[3], z[3]].iter().any(|v| v.abs() > AFFINE_EPSILON)
    {
        return None;
    }
    Some(json!((0..3)
        .map(|row| [x[row], y[row], z[row], t[row]])
        .collect::<Vec<_>>()))
}

fn identity() -> Value {
    json!([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0]
    ])
}

fn vector(value: Option<&Value>) -> Option<V3> {
    let items = value?.as_array()?;
    let [x, y, z] = items.as_slice() else {
        return None;
    };
    Some([x.as_f64()?, y.as_f64()?, z.as_f64()?])
}

fn unit(v: V3) -> Option<V3> {
    let norm = dot(v, v).sqrt();
    (norm.is_finite() && norm > AXIS_EPSILON).then(|| v.map(|c| c / norm))
}

/// The rows of the frame at `origin` whose z axis is `z` and whose x axis
/// is `x` laid square to z, or any axis square to z.
fn frame(origin: V3, z: V3, x: Option<V3>) -> Option<Value> {
    let z = unit(z)?;
    let helper = x.filter(|x| unit(*x).is_some()).unwrap_or_else(|| {
        if z[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        }
    });
    let along = dot(helper, z);
    let x = unit([
        helper[0] - z[0] * along,
        helper[1] - z[1] * along,
        helper[2] - z[2] * along,
    ])?;
    let y = cross(z, x);
    Some(json!([
        [x[0], y[0], z[0], origin[0]],
        [x[1], y[1], z[1], origin[1]],
        [x[2], y[2], z[2], origin[2]],
    ]))
}

/// The frame a joint origin's snap names, in component coordinates.
fn snap_frame(origin: &Value) -> Option<Value> {
    let snap = origin.get("snap")?;
    match snap.get("type")?.as_str()? {
        "frame" => frame(
            vector(snap.get("origin"))?,
            vector(snap.get("z"))?,
            vector(snap.get("x")),
        ),
        "point" => frame(
            vector(snap.get("point"))?,
            vector(snap.get("z")).unwrap_or([0.0, 0.0, 1.0]),
            vector(snap.get("x")),
        ),
        _ => None,
    }
}

/// Neutral limits of a joint value.
fn limits(range: Option<&Value>) -> Option<Value> {
    let range = range?.as_array()?;
    let [minimum, maximum] = range.as_slice() else {
        return None;
    };
    let (minimum, maximum) = (minimum.as_f64()?, maximum.as_f64()?);
    (minimum <= maximum).then(|| json!({"kind": "range", "minimum": minimum, "maximum": maximum}))
}

/// A translation, and a turn about z first.
fn offset(translation: V3, turn: f64) -> Value {
    let (sin, cos) = (turn.portable_sin(), turn.portable_cos());
    json!([
        [cos, -sin, 0.0, translation[0]],
        [sin, cos, 0.0, translation[1]],
        [0.0, 0.0, 1.0, translation[2]]
    ])
}

/// The paired joint kind of a joint record, or `None` for a kind that has
/// no neutral family.
fn pair_kind(record: &Value) -> Option<Value> {
    let value = |index: usize| {
        record
            .get("values")
            .and_then(Value::as_array)
            .and_then(|values| values.get(index))
            .and_then(Value::as_f64)
    };
    let range = |index: usize| {
        limits(
            record
                .get("limits")
                .and_then(Value::as_array)
                .and_then(|limits| limits.get(index)),
        )
    };
    let mut kind = Map::new();
    let mut set = |key: &str, field: Option<Value>| {
        if let Some(field) = field {
            kind.insert(key.to_owned(), field);
        }
    };
    let name = match record.get("kind")?.as_str()? {
        "rigid" => "fixed",
        "revolute" => {
            set("angle", value(0).map(Value::from));
            set("angular_limits", range(0));
            "revolute"
        }
        "slider" => {
            set("distance", value(0).map(Value::from));
            set("linear_limits", range(0));
            "slider"
        }
        "cylindrical" => {
            set("angle", value(0).map(Value::from));
            set("distance", value(1).map(Value::from));
            set("angular_limits", range(0));
            set("linear_limits", range(1));
            "cylindrical"
        }
        "ball" => "ball",
        "parallel" => "parallel",
        "perpendicular" => "perpendicular",
        "angle" => {
            // The tilt of B's z from the mated position, about their common x.
            set(
                "angle",
                record.get("angle").and_then(Value::as_f64).map(Value::from),
            );
            "angle"
        }
        _ => return None,
    };
    kind.insert("kind".to_owned(), json!(name));
    Some(Value::Object(kind))
}

impl super::Walk<'_, '_> {
    /// Decodes the components, occurrences, and joints of `document`.
    pub(super) fn assembly(
        &mut self,
        document: &Map<String, Value>,
        owned: &Owned<'_>,
    ) -> Result<Outcome, CodecError> {
        let mut outcome = Outcome::default();
        let components = array(document, "components");
        let occurrences = array(document, "occurrences");
        let mut known = BTreeSet::from([0_u64]);
        known.extend(components.iter().filter_map(|c| c.get("id")?.as_u64()));
        self.ctx.charge_entities(
            u64_from_index(components.len() + occurrences.len()),
            "solidcraft components",
        )?;
        self.definitions(document, components, &known, owned)?;
        let placed = self.occurrences(document, occurrences, &known, &mut outcome)?;
        if outcome.unplaced > 0 {
            outcome
                .losses
                .push(SolidcraftLossCode::AssemblyOccurrenceUnplaced.note(format!(
                    "{} placements name no component the document holds, or would make a                      component contain itself, so they are not placed",
                    outcome.unplaced
                )));
        }
        self.ownership(document, &known, owned);
        self.joints(document, &placed, &mut outcome)?;
        Ok(outcome)
    }

    fn definitions(
        &mut self,
        document: &Map<String, Value>,
        components: &[Value],
        known: &BTreeSet<u64>,
        owned: &Owned<'_>,
    ) -> Result<(), CodecError> {
        let name = document.get("name").and_then(Value::as_str);
        let mut records = vec![(0_u64, name.map(str::to_owned), Map::new())];
        for component in components {
            let Some(component_id) = component.get("id").and_then(Value::as_u64) else {
                continue;
            };
            let mut properties = Map::new();
            if let Some(index) = component
                .get("identification_color_index")
                .and_then(Value::as_u64)
            {
                properties.insert(
                    "identification_color_index".to_owned(),
                    json!(index.to_string()),
                );
            }
            let label = component.get("name").and_then(Value::as_str);
            records.push((component_id, label.map(str::to_owned), properties));
        }
        for (component_id, label, properties) in records {
            if !known.contains(&component_id) {
                continue;
            }
            let bodies: Vec<String> = owned
                .bodies
                .get(&component_id)
                .map(|bodies| bodies.iter().map(ToString::to_string).collect())
                .unwrap_or_default();
            let sketches: Vec<&String> = owned
                .sketches
                .iter()
                .filter(|(_, component)| *component == component_id)
                .map(|(sketch, _)| sketch)
                .collect();
            // The design is the root file's definition, which holds no
            // identity of its own; a component is stored in that file.
            let storage = if component_id == 0 {
                json!({"kind": "file", "document": {}})
            } else {
                json!({"kind": "embedded"})
            };
            let mut record = json!({
                "id": id("product", &component_id.to_string()),
                "kind": "part",
                "storage": storage,
                "occurrences": [],
                "instance_bodies": [],
                "representations": [],
                "bom_properties": properties,
                "bodies": bodies,
                "sketches": sketches,
                "tessellations": [],
            });
            if let Some(label) = label {
                record["source_name"] = json!(label);
                record["label"] = json!(label);
            }
            let definition: ProductDefinition =
                serde_json::from_value(record).map_err(malformed)?;
            self.ir.model.product_definitions.push(definition);
        }
        Ok(())
    }

    /// Places the occurrences. Each placement is one occurrence, owned by the
    /// definition of the component it sits in, in source order. A placement of
    /// a component the document does not hold, and one that would make a
    /// component contain itself, is not placed and is counted as unresolved.
    /// Returns the occurrence and the component of each placement.
    fn occurrences(
        &mut self,
        document: &Map<String, Value>,
        occurrences: &[Value],
        known: &BTreeSet<u64>,
        outcome: &mut Outcome,
    ) -> Result<BTreeMap<u64, (String, String)>, CodecError> {
        // The occurrence ids along each hidden path, outermost first.
        let hidden: Vec<Vec<u64>> = document
            .get("visibility")
            .and_then(|visibility| visibility.get("hidden_occurrences"))
            .and_then(Value::as_array)
            .map(|paths| {
                paths
                    .iter()
                    .filter_map(|path| {
                        path.as_array()?
                            .iter()
                            .map(Value::as_u64)
                            .collect::<Option<Vec<u64>>>()
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.ctx
            .charge_entities(u64_from_index(occurrences.len()), "solidcraft occurrences")?;
        let mut first = BTreeMap::new();
        let mut owner: BTreeMap<u64, Vec<String>> = BTreeMap::new();
        // The components each component places, to refuse a cycle.
        let mut places: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
        let mut unplaced = 0_usize;
        for occurrence in occurrences {
            let Some(occurrence_id) = occurrence.get("id").and_then(Value::as_u64) else {
                continue;
            };
            let parent = occurrence
                .get("parent")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let target = occurrence
                .get("component")
                .and_then(Value::as_u64)
                .filter(|target| known.contains(target));
            let Some(target) = target.filter(|target| !reaches(&places, *target, parent)) else {
                unplaced += 1;
                continue;
            };
            let ir_id = id("occurrence", &occurrence_id.to_string());
            let mut record = json!({
                "id": ir_id,
                "prototype": id("product", &target.to_string()),
                "transform": occurrence.get("transform").and_then(rows).unwrap_or_else(identity),
                "overrides": [],
            });
            if let Some(name) = occurrence.get("name").and_then(Value::as_str) {
                record["name"] = json!(name);
            }
            let mut overrides = Vec::new();
            for path in hidden
                .iter()
                .filter(|path| path.first() == Some(&occurrence_id))
            {
                if path.len() == 1 {
                    record["visible"] = json!(false);
                } else {
                    let steps: Vec<Value> = path[1..]
                        .iter()
                        .map(|step| {
                            json!({"occurrence": {
                                "kind": "occurrence",
                                "value": id("occurrence", &step.to_string()),
                            }})
                        })
                        .collect();
                    overrides.push(json!({"path": steps, "visible": false}));
                }
            }
            if !overrides.is_empty() {
                record["overrides"] = Value::Array(overrides);
            }
            let placed: Occurrence = serde_json::from_value(record).map_err(malformed)?;
            first
                .entry(occurrence_id)
                .or_insert_with(|| (ir_id.clone(), target.to_string()));
            places.entry(parent).or_default().insert(target);
            owner.entry(parent).or_default().push(ir_id);
            self.ir.model.occurrences.push(placed);
        }
        for definition in &mut self.ir.model.product_definitions {
            let component = definition
                .id
                .as_str()
                .rsplit('#')
                .next()
                .and_then(|key| key.parse::<u64>().ok());
            let Some(ids) = component.and_then(|component| owner.remove(&component)) else {
                continue;
            };
            definition.occurrences = ids
                .into_iter()
                .map(|occurrence| serde_json::from_value(json!(occurrence)).map_err(malformed))
                .collect::<Result<_, _>>()?;
        }
        outcome.unplaced = unplaced;
        Ok(first)
    }

    /// Gives each feature and model parameter its component's product when
    /// the component owns everything the feature made.
    fn ownership(
        &mut self,
        document: &Map<String, Value>,
        known: &BTreeSet<u64>,
        owned: &Owned<'_>,
    ) {
        let mut component_of_feature = BTreeMap::new();
        for record in array(document, "features") {
            if let Some(feature) = record.get("id").and_then(Value::as_u64) {
                component_of_feature
                    .insert(id("feature", &feature.to_string()), component_of(record));
            }
        }
        let mut definition_of = BTreeMap::new();
        for feature in &mut self.ir.model.features {
            let Some(component) = component_of_feature
                .get(feature.id.as_str())
                .filter(|component| known.contains(component))
            else {
                continue;
            };
            let owns = owned.bodies.get(component).map_or(&[][..], Vec::as_slice);
            if feature
                .evaluation
                .outputs()
                .iter()
                .all(|body| owns.contains(body))
            {
                let definition =
                    serde_json::from_value(json!(id("product", &component.to_string())));
                if let Ok(definition) = definition {
                    definition_of.insert(feature.id.clone(), definition);
                }
            }
        }
        for feature in &mut self.ir.model.features {
            feature.product_definition = definition_of.get(&feature.id).cloned();
        }
        for parameter in &mut self.ir.model.parameters {
            parameter.product_definition = parameter
                .owner
                .as_ref()
                .and_then(|owner| definition_of.get(owner))
                .cloned();
        }
    }

    fn joints(
        &mut self,
        document: &Map<String, Value>,
        placed: &BTreeMap<u64, (String, String)>,
        outcome: &mut Outcome,
    ) -> Result<(), CodecError> {
        let assembly = document.get("assembly");
        let joints = assembly
            .and_then(|assembly| assembly.get("joints"))
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        self.ctx
            .charge_entities(u64_from_index(joints.len()), "solidcraft joints")?;
        let connector = |origin: &Value| -> Option<Value> {
            let occurrence = origin.get("occurrence")?.as_u64()?;
            let (ir_id, _) = placed.get(&occurrence)?;
            Some(json!({
                "operand": {
                    "container": {"kind": "occurrence", "occurrence": ir_id, "occurrence_path": []},
                    "object": origin.get("snap")?.get("type")?,
                    "subelements": [],
                },
                "frame": snap_frame(origin)?,
                "detached": false,
            }))
        };
        let mut unresolved = 0_usize;
        let mut unnamed_kind = BTreeSet::new();
        for joint in joints {
            let Some(joint_id) = joint.get("id").and_then(Value::as_u64) else {
                unresolved += 1;
                continue;
            };
            let connectors = ["a", "b"].map(|side| joint.get(side).and_then(connector));
            let pair = pair_kind(joint);
            let (Some(pair), [Some(a), Some(b)]) = (&pair, &connectors) else {
                unresolved += 1;
                if pair.is_none() {
                    unnamed_kind.insert(
                        joint
                            .get("kind")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .to_owned(),
                    );
                }
                continue;
            };
            let number = |key: &str| joint.get(key).and_then(Value::as_f64).unwrap_or(0.0);
            let lateral = joint
                .get("offset_xy")
                .and_then(Value::as_array)
                .and_then(|xy| Some([xy.first()?.as_f64()?, xy.get(1)?.as_f64()?]))
                .unwrap_or([0.0; 2]);
            // A turn about z; the angle of an angle joint is a tilt, which
            // the joint kind carries.
            let turn = if joint.get("kind").and_then(Value::as_str) == Some("angle") {
                0.0
            } else {
                number("angle")
            };
            let sense = if joint.get("flip").and_then(Value::as_bool) == Some(true) {
                "aligned"
            } else {
                "opposed"
            };
            let record = json!({
                "id": id("joint", &joint_id.to_string()),
                "operands": {"kind": "pair", "value": {
                    "kind": pair,
                    "connectors": [a, b],
                    "offset_frames": [
                        offset([lateral[0], lateral[1], 0.0], 0.0),
                        offset([0.0, 0.0, number("offset")], turn),
                    ],
                    "frame_sense": sense,
                }},
                "suppressed": joint.get("suppressed").and_then(Value::as_bool) == Some(true),
                "drives": [],
            });
            let joint: AssemblyJoint = serde_json::from_value(record).map_err(malformed)?;
            self.ir.model.assembly_joints.push(joint);
        }
        for occurrence in array(document, "occurrences") {
            let Some(occurrence_id) = occurrence.get("id").and_then(Value::as_u64) else {
                continue;
            };
            if occurrence.get("grounded").and_then(Value::as_bool) != Some(true) {
                continue;
            }
            let Some((ir_id, _)) = placed.get(&occurrence_id) else {
                continue;
            };
            let record = json!({
                "id": id("joint", &format!("grounded:{occurrence_id}")),
                "operands": {"kind": "grounded", "value": {"connector": {
                    "operand": {
                        "container": {"kind": "occurrence", "occurrence": ir_id, "occurrence_path": []},
                        "object": "occurrence",
                        "subelements": [],
                    },
                    "frame": identity(),
                    "detached": false,
                }}},
                "suppressed": false,
                "drives": [],
            });
            let joint: AssemblyJoint = serde_json::from_value(record).map_err(malformed)?;
            // The grounded frame is the identity, which the document does not state.
            self.annotations.field_exactness(
                joint.id.as_str(),
                "operands",
                cadmpeg_ir::Exactness::Inferred,
            )?;
            self.ir.model.assembly_joints.push(joint);
        }
        if unresolved > 0 {
            outcome
                .losses
                .push(SolidcraftLossCode::AssemblyJointUnresolved.note(format!(
                    "{unresolved} joints have no neutral form and stay in the retained document: \
                 their snap is on geometry, or their kind has no neutral family{}",
                    if unnamed_kind.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " ({})",
                            unnamed_kind.into_iter().collect::<Vec<_>>().join(", ")
                        )
                    }
                )));
        }
        let links = assembly
            .and_then(|assembly| assembly.get("links"))
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let studies = assembly
            .and_then(|assembly| assembly.get("studies"))
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        if links + studies > 0 {
            outcome
                .losses
                .push(SolidcraftLossCode::AssemblyLinksUndecoded.note(format!(
                "{links} joint links and {studies} motion studies are retained without decoding"
            )));
        }
        Ok(())
    }
}

fn array<'a>(document: &'a Map<String, Value>, key: &str) -> &'a [Value] {
    document
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
}
