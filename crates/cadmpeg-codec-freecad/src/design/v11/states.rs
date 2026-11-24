// SPDX-License-Identifier: Apache-2.0
//! The states of a `PartDesign::Body` other than its current state (§5.4),
//! and the home of every other shape a Body member stores.
//!
//! `FreeCAD` stores each state of a Body in full. The `Shape` of a member that
//! builds the Body's solid (an object that stores `BaseFeature`) holds the
//! state after that member, and the `AddSubShape` of an additive or
//! subtractive feature or a hole holds its tool body (`AddShape` and
//! `SubShape` in documents that name it by the feature's effect). The Body's own `Shape`
//! repeats its `Tip`'s and is the Body's current state (§5.5, D33).
//! `CadIr.geometry` holds only the current model, so every other stored state
//! is an evaluation: `State(After(m))`, based on the nearest earlier stored
//! state of its Body, with its records' ids minted by the full-snapshot
//! procedure, and `Tool(f)`, based on `Empty` with every record its own.
//!
//! The other shapes a document keeps beside its current model are no states
//! and no geometry of it: the `Tip`'s repeat of the current state, a sketch's
//! shape, a datum's display shape (a datum has no topology, §6.2), a binder's
//! copy, the shape of a pattern's stage or of a member the design states no
//! feature for, a suppressed member's copy of the state before it, and the
//! material a dress-up or a pattern adds or removes (§5.4, Rule N case (b)).
//! Each stays in its object's native record, whose shape payload holds the
//! parsed B-rep.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::design::container::{Container, OrderedKind};
use cadmpeg_ir::design::evaluation::{
    Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ids::EvaluationId;
use cadmpeg_ir::design::refs::{Evaluated, Member, Position, RecordRef};
use cadmpeg_ir::document::{CadIr, Topology};
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::report::loss::LossNote;
use serde_json::{Map, Value};

use super::Source;
use crate::design::IntermediatePayload;
use crate::loss::FreecadLossCode;
use crate::native::{id_key, payload_of, ObjectRecord, PropertyRecord};

/// Where a shape the document keeps beside its current model is stated.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Home {
    /// A definition lists one of its bodies: it stays in the geometry.
    Listed,
    /// The `Tip`'s shape where the current state of Body `.0` holds no body:
    /// it is that current state.
    Current(usize),
    /// The state after member `member` of Body `body`.
    State { body: usize, member: usize },
    /// The tool body of a feature.
    Tool(FeatureId),
    /// No typed record: the object's native record keeps it.
    Native,
}

/// A `PartDesign::Body` the design states: an ordered `Body` container.
struct BodyStates {
    members: Vec<FeatureId>,
    /// The index of its current state in the scope's `current`.
    current: usize,
    /// The member at whose position that current state is, the `Tip`.
    tip: Option<usize>,
}

/// What the design states about the document's Bodies.
struct Design {
    scope: ProductDefinitionId,
    bodies: Vec<BodyStates>,
    /// Each Body member's Body and position.
    member_of: HashMap<FeatureId, (usize, usize)>,
    /// The feature of each object the design states one for.
    feature_of: HashMap<String, FeatureId>,
}

impl Design {
    fn of(ir: &CadIr) -> Option<Self> {
        let scope = ir.design.scopes.first()?;
        let mut bodies = Vec::new();
        let mut member_of = HashMap::new();
        for container in &ir.design.containers {
            let Container::Ordered {
                id,
                kind: OrderedKind::Body,
                members,
                ..
            } = container
            else {
                continue;
            };
            let members = members
                .iter()
                .filter_map(|member| match member {
                    Member::Feature(feature) => Some(feature.clone()),
                    Member::Container(_) => None,
                })
                .collect::<Vec<_>>();
            let Some(current) = scope.current.iter().position(|state| match &state.at {
                Position::After(Member::Feature(feature)) => members.contains(feature),
                Position::Start(container) | Position::After(Member::Container(container)) => {
                    container == id
                }
            }) else {
                continue;
            };
            let tip = match &scope.current[current].at {
                Position::After(Member::Feature(feature)) => {
                    members.iter().position(|member| member == feature)
                }
                _ => None,
            };
            for (index, member) in members.iter().enumerate() {
                member_of.insert(member.clone(), (bodies.len(), index));
            }
            bodies.push(BodyStates {
                members,
                current,
                tip,
            });
        }
        let feature_of = ir
            .design
            .features
            .iter()
            .filter_map(|feature| Some((feature.native_ref.clone()?, feature.id.clone())))
            .collect();
        Some(Self {
            scope: scope.definition.clone(),
            bodies,
            member_of,
            feature_of,
        })
    }
}

/// State each intermediate payload where the specification puts it, and
/// take every one that is no part of the current model out of
/// `CadIr.geometry`, with the model-class records that name its records.
///
/// # Errors
///
/// Returns a codec error where a moved record's id or a state's records do
/// not round-trip, or the decode's resource policy refuses the work.
pub(crate) fn state(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    intermediate: &[IntermediatePayload<'_>],
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if intermediate.is_empty() {
        return Ok(());
    }
    let source = Source::of(objects, properties);
    let types = objects
        .iter()
        .map(|object| (object.id().as_str(), object.type_name.as_str()))
        .collect::<HashMap<_, _>>();
    let design = Design::of(ir);
    let listed = ir
        .model
        .product_definitions
        .iter()
        .flat_map(|definition| {
            definition.bodies.iter().chain(
                definition
                    .instance_bodies
                    .iter()
                    .flat_map(|instance| &instance.bodies),
            )
        })
        .map(BodyId::as_str)
        .collect::<HashSet<_>>();
    let keys = intermediate
        .iter()
        .map(|payload| id_key(&payload.payload.id))
        .collect::<HashSet<_>>();
    let held =
        ir.geometry
            .bodies
            .iter()
            .fold(HashMap::<&str, Vec<&str>>::new(), |mut held, body| {
                if let Some(key) = payload_of(body.id.as_str(), |key| keys.contains(key)) {
                    held.entry(key).or_default().push(body.id.as_str());
                }
                held
            });
    let mut homes = BTreeMap::new();
    for payload in intermediate {
        let key = id_key(&payload.payload.id);
        let bodies = held.get(key).map_or(&[][..], Vec::as_slice);
        let home = if bodies.iter().any(|body| listed.contains(body)) {
            Home::Listed
        } else {
            home(ir, design.as_ref(), payload, &types, &source)
        };
        homes.insert(key.to_owned(), (home, payload));
    }

    // The `Tip`'s shape becomes its Body's current state where the Body's
    // own shape holds none.
    for (key, (home, _)) in &homes {
        if let (Home::Current(body), Some(design)) = (home, &design) {
            let current = &mut ir.design.scopes[0].current[design.bodies[*body].current];
            current.bodies.extend(
                held.get(key.as_str())
                    .into_iter()
                    .flatten()
                    .map(|body| BodyId::mint(*body).map_err(CodecError::malformed))
                    .collect::<Result<Vec<_>, _>>()?,
            );
            current.bodies.sort();
            current.bodies.dedup();
        }
    }
    let moving = homes
        .iter()
        .filter(|(_, (home, _))| !matches!(home, Home::Listed | Home::Current(_)))
        .map(|(key, _)| key.as_str())
        .collect::<HashSet<_>>();
    if moving.is_empty() {
        return Ok(());
    }
    let mut drained = drain(&mut ir.geometry, &moving);

    let mut evaluations = Vec::new();
    // Each record a state holds, by its id in the payload, and its id in
    // the state.
    let mut moved = HashMap::<String, String>::new();
    // The payloads whose shapes are states.
    let mut payload_states = HashSet::<String>::new();
    if let Some(design) = &design {
        let states = homes
            .iter()
            .filter_map(|(key, (home, payload))| match home {
                Home::State { body, member } => Some(((*body, *member), (key.as_str(), *payload))),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        for (index, body) in design.bodies.iter().enumerate() {
            let current = &ir.design.scopes[0].current[body.current];
            let current_base = |ir: &CadIr| -> Result<Option<Base>, CodecError> {
                let bodies = &ir.design.scopes[0].current[body.current].bodies;
                if bodies.is_empty() {
                    return Ok(None);
                }
                Ok(Some(Base::current(
                    ir,
                    ir.design.scopes[0].current[body.current].at.clone(),
                    bodies,
                )?))
            };
            let mut base = if matches!(current.at, Position::After(Member::Feature(_))) {
                None
            } else {
                current_base(ir)?
            };
            for (member, feature) in body.members.iter().enumerate() {
                if body.tip == Some(member) {
                    base = current_base(ir)?;
                }
                let Some((key, payload)) = states.get(&(index, member)) else {
                    continue;
                };
                let Some(topology) = drained.remove(*key) else {
                    continue;
                };
                if topology.bodies.is_empty() {
                    continue;
                }
                ctx.charge_entities(1, "state a FreeCAD Body state")?;
                let snapshot = snapshot(&topology, base.as_ref())?;
                let of = Evaluated::State(Position::After(Member::Feature(feature.clone())));
                let id = evaluation_id(key)?;
                moved.extend(snapshot.renamed);
                payload_states.insert((*key).to_owned());
                evaluations.push(Evaluation {
                    id: id.clone(),
                    scope: design.scope.clone(),
                    configuration: None,
                    of,
                    status: status(&source, payload.owner),
                    content: Some(StateContent {
                        base: base
                            .as_ref()
                            .map_or(StateBase::Empty, |base| base.reference.clone()),
                        delta: StateDelta {
                            records: snapshot.records,
                            removed: snapshot.removed,
                            ..StateDelta::default()
                        },
                    }),
                    native_ref: Some(payload.payload.id.clone()),
                });
                base = Some(Base {
                    reference: StateBase::Evaluation(id),
                    resolved: snapshot.resolved,
                });
            }
        }
        for (key, (home, payload)) in &homes {
            let Home::Tool(feature) = home else {
                continue;
            };
            let Some(topology) = drained.remove(key.as_str()) else {
                continue;
            };
            if topology.bodies.is_empty() {
                continue;
            }
            ctx.charge_entities(1, "state a FreeCAD tool body")?;
            let of = Evaluated::Tool(feature.clone());
            moved.extend(record_ids(&topology).into_iter().map(|id| (id.clone(), id)));
            payload_states.insert(key.clone());
            evaluations.push(Evaluation {
                id: evaluation_id(key)?,
                scope: design.scope.clone(),
                configuration: None,
                of,
                status: status(&source, payload.owner),
                content: Some(StateContent {
                    base: StateBase::Empty,
                    delta: StateDelta {
                        records: topology,
                        ..StateDelta::default()
                    },
                }),
                native_ref: Some(payload.payload.id.clone()),
            });
        }
    }
    // Every payload not stated as a state is kept by its native record.
    let native = moving
        .iter()
        .filter(|key| !payload_states.contains(**key))
        .map(|key| (*key).to_owned())
        .collect::<HashSet<_>>();
    ir.design.evaluations.extend(evaluations);
    ir.design.evaluations.sort_by(|a, b| a.id.cmp(&b.id));
    restate_references(
        ir,
        &moved,
        &payload_states,
        &native,
        design.as_ref(),
        &homes,
        losses,
    )
}

/// Where the specification states intermediate payload `payload`, which no
/// definition lists.
fn home(
    ir: &CadIr,
    design: Option<&Design>,
    payload: &IntermediatePayload<'_>,
    types: &HashMap<&str, &str>,
    source: &Source<'_>,
) -> Home {
    let Some(design) = design else {
        return Home::Native;
    };
    let Some(feature) = design.feature_of.get(payload.owner) else {
        return Home::Native;
    };
    let Some(&(body, member)) = design.member_of.get(feature) else {
        return Home::Native;
    };
    let stateless = ir.is_stateless(feature, None).unwrap_or(true);
    match payload.property {
        "Shape" if design.bodies[body].tip == Some(member) => {
            let current = &ir.design.scopes[0].current[design.bodies[body].current];
            if current.bodies.is_empty() {
                Home::Current(body)
            } else {
                Home::Native
            }
        }
        "Shape" if !stateless && source.stored.contains(&(payload.owner, "BaseFeature")) => {
            Home::State { body, member }
        }
        "AddSubShape" | "AddShape" | "SubShape"
            if !stateless
                && types
                    .get(payload.owner)
                    .is_some_and(|kind| crate::design::stores_tool_body(kind))
                && !ir.design.features.iter().any(|candidate| {
                    &candidate.id == feature
                        && matches!(
                            candidate.definition.operation,
                            FeatureOperation::Stored { .. }
                        )
                }) =>
        {
            Home::Tool(feature.clone())
        }
        _ => Home::Native,
    }
}

/// The status of a state an object stores: `Stale` where `FreeCAD` marks the
/// object `Touched` (§5.4).
fn status(source: &Source<'_>, object: &str) -> EvaluationStatus {
    if source.touched.contains(object) {
        EvaluationStatus::Stale
    } else {
        EvaluationStatus::Current
    }
}

/// The evaluation of the state a payload holds, keyed by the payload.
fn evaluation_id(payload: &str) -> Result<EvaluationId, CodecError> {
    EvaluationId::mint(format!("fcstd:design:evaluation#{payload}")).map_err(CodecError::malformed)
}

/// Take the records of the payloads `keys` out of `geometry`, by payload.
fn drain(geometry: &mut Topology, keys: &HashSet<&str>) -> HashMap<String, Topology> {
    let mut out = HashMap::<String, Topology>::new();
    macro_rules! split {
        ($($arena:ident),*) => {$(
            for record in std::mem::take(&mut geometry.$arena) {
                match payload_of(record.id.as_str(), |key| keys.contains(key)) {
                    Some(key) => {
                        let key = key.to_owned();
                        out.entry(key).or_default().$arena.push(record);
                    }
                    None => geometry.$arena.push(record),
                }
            }
        )*};
    }
    split!(
        bodies,
        regions,
        shells,
        faces,
        loops,
        coedges,
        edges,
        vertices,
        points,
        surfaces,
        curves,
        subds,
        pcurves,
        procedural_surfaces,
        procedural_curves
    );
    out
}

/// The id of every record of `topology`.
pub(crate) fn record_ids(topology: &Topology) -> Vec<String> {
    let mut ids = Vec::new();
    macro_rules! collect {
        ($($arena:ident),*) => {$(
            ids.extend(topology.$arena.iter().map(|record| record.id.as_str().to_owned()));
        )*};
    }
    collect!(
        bodies,
        regions,
        shells,
        faces,
        loops,
        coedges,
        edges,
        vertices,
        points,
        surfaces,
        curves,
        subds,
        pcurves,
        procedural_surfaces,
        procedural_curves
    );
    ids
}

/// The arenas of a `Topology`, by their wire names, with the `RecordRef` kind
/// of each.
const ARENAS: [(&str, &str); 15] = [
    ("bodies", "body"),
    ("regions", "region"),
    ("shells", "shell"),
    ("faces", "face"),
    ("loops", "loop"),
    ("coedges", "coedge"),
    ("edges", "edge"),
    ("vertices", "vertex"),
    ("points", "point"),
    ("surfaces", "surface"),
    ("curves", "curve"),
    ("subds", "subd"),
    ("pcurves", "pcurve"),
    ("procedural_surfaces", "procedural_surface"),
    ("procedural_curves", "procedural_curve"),
];
/// The arenas whose records match by content alone once the ids they hold
/// are mapped (§5.4, Full snapshots).
const CARRIERS: [&str; 9] = [
    "points",
    "pcurves",
    "curves",
    "surfaces",
    "procedural_curves",
    "procedural_surfaces",
    "subds",
    "vertices",
    "edges",
];
/// The arenas whose records refer to one another and match together.
const COUPLED: [&str; 3] = ["loops", "coedges", "faces"];
/// The carrier arenas whose records name one another: a procedural carrier
/// its construction, and a construction the carriers it is built from.
const CARRIER_LINKS: [&str; 4] = [
    "surfaces",
    "curves",
    "procedural_surfaces",
    "procedural_curves",
];

/// Every string `value` holds.
fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(fields) => fields.values().for_each(|field| strings(field, out)),
        _ => {}
    }
}

/// A state's records by arena and id, each without its `id` field, and the
/// order of its bodies.
#[derive(Default)]
struct Resolved {
    arenas: BTreeMap<&'static str, BTreeMap<String, Value>>,
    bodies: Vec<String>,
}

/// A stored state a later state is based on.
struct Base {
    reference: StateBase,
    resolved: Resolved,
}

impl Base {
    /// The extent of the current state at `at`: its bodies and the records
    /// of the payloads that hold them.
    fn current(ir: &CadIr, at: Position, bodies: &[BodyId]) -> Result<Self, CodecError> {
        let keys = bodies
            .iter()
            .filter_map(|body| id_key(body.as_str()).rsplit_once(':'))
            .map(|(key, _)| key)
            .collect::<HashSet<_>>();
        let mut topology = Topology::default();
        macro_rules! select {
            ($($arena:ident),*) => {$(
                topology.$arena = ir
                    .geometry
                    .$arena
                    .iter()
                    .filter(|record| payload_of(record.id.as_str(), |key| keys.contains(key)).is_some())
                    .cloned()
                    .collect();
            )*};
        }
        select!(
            bodies,
            regions,
            shells,
            faces,
            loops,
            coedges,
            edges,
            vertices,
            points,
            surfaces,
            curves,
            subds,
            pcurves,
            procedural_surfaces,
            procedural_curves
        );
        let mut resolved = Resolved::default();
        for (arena, records) in arenas(&topology)? {
            resolved
                .arenas
                .insert(arena, records.into_iter().collect::<BTreeMap<_, _>>());
        }
        resolved.bodies = bodies.iter().map(|body| body.as_str().to_owned()).collect();
        Ok(Self {
            reference: StateBase::Current(at),
            resolved,
        })
    }
}

/// The records of one arena, each as its id and its wire value without the
/// `id` field.
type Records = Vec<(String, Value)>;

/// The records of `topology` by arena, each as its id and its wire value
/// without the `id` field.
fn arenas(topology: &Topology) -> Result<Vec<(&'static str, Records)>, CodecError> {
    let Value::Object(mut wire) = serde_json::to_value(topology).map_err(CodecError::malformed)?
    else {
        return Err(CodecError::malformed("topology is not a JSON object"));
    };
    let mut out = Vec::new();
    for (arena, _) in ARENAS {
        let Some(Value::Array(records)) = wire.remove(arena) else {
            continue;
        };
        let mut rows = Vec::with_capacity(records.len());
        for record in records {
            let Value::Object(mut fields) = record else {
                return Err(CodecError::malformed(
                    "topology record is not a JSON object",
                ));
            };
            let Some(Value::String(id)) = fields.remove("id") else {
                return Err(CodecError::malformed("topology record has no id"));
            };
            rows.push((id, Value::Object(fields)));
        }
        out.push((arena, rows));
    }
    Ok(out)
}

/// The fields of record `value` that matching compares: every field but
/// `source_object`, which names the object whose stored shape holds the
/// record (§5.4, Full snapshots).
fn compared(value: &Value) -> Value {
    let mut value = value.clone();
    if let Value::Object(fields) = &mut value {
        fields.remove("source_object");
    }
    value
}

/// `value` with every string that `map` maps replaced by its image.
fn mapped(value: &Value, map: &HashMap<String, String>) -> Value {
    match value {
        Value::String(text) => map
            .get(text)
            .map_or_else(|| value.clone(), |image| Value::String(image.clone())),
        Value::Array(items) => Value::Array(items.iter().map(|item| mapped(item, map)).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(name, field)| (name.clone(), mapped(field, map)))
                .collect::<Map<_, _>>(),
        ),
        _ => value.clone(),
    }
}

/// `value` with every string that `map` maps replaced by its image and every
/// string in `open` replaced by a placeholder, and the replaced strings of
/// `open` in the order the value holds them.
fn placeheld(
    value: &Value,
    map: &HashMap<String, String>,
    open: &HashSet<&str>,
    held: &mut Vec<String>,
) -> Value {
    match value {
        Value::String(text) if open.contains(text.as_str()) => {
            held.push(text.clone());
            Value::Null
        }
        Value::String(text) => map
            .get(text)
            .map_or_else(|| value.clone(), |image| Value::String(image.clone())),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| placeheld(item, map, open, held))
                .collect(),
        ),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(name, field)| (name.clone(), placeheld(field, map, open, held)))
                .collect::<Map<_, _>>(),
        ),
        _ => value.clone(),
    }
}

/// A state as a delta on its base.
struct Snapshot {
    records: Topology,
    removed: Vec<RecordRef>,
    /// Each record's id in the payload and in the state.
    renamed: Vec<(String, String)>,
    resolved: Resolved,
}

/// The state whose stored shape is `topology`, as a delta on `base` (`None`
/// is `Empty`), with ids minted by the full-snapshot procedure (§5.4): a
/// body keeps its base body's id by position, and so do the regions of that
/// body and the shells of those regions; every other record that equals a
/// base record but for `source_object` once the ids it holds are mapped
/// keeps the base record's id and `source_object` and is shared; every other
/// record is new and keeps the key of the shape that holds it.
fn snapshot(topology: &Topology, base: Option<&Base>) -> Result<Snapshot, CodecError> {
    let empty = Resolved::default();
    let base = base.map_or(&empty, |base| &base.resolved);
    let records = arenas(topology)?
        .into_iter()
        .collect::<BTreeMap<&'static str, Vec<(String, Value)>>>();
    let rows = |arena: &str| records.get(arena).map_or(&[][..], Vec::as_slice);
    let base_rows = |arena: &str| base.arenas.get(arena);
    let mut map = HashMap::<String, String>::new();
    let mut used = HashSet::<String>::new();
    let pair =
        |map: &mut HashMap<String, String>, used: &mut HashSet<String>, new: &str, old: &str| {
            if used.insert(old.to_owned()) {
                map.insert(new.to_owned(), old.to_owned());
            }
        };

    // Bodies, their regions, and those regions' shells, by position.
    let field = |value: &Value, name: &str| value.get(name).cloned();
    for (body, base_body) in rows("bodies")
        .iter()
        .map(|(id, _)| id)
        .zip(base.bodies.iter())
    {
        pair(&mut map, &mut used, body, base_body);
        let regions = rows("regions")
            .iter()
            .filter(|(_, value)| field(value, "body") == Some(Value::String(body.clone())))
            .collect::<Vec<_>>();
        let base_regions = base_rows("regions")
            .into_iter()
            .flatten()
            .filter(|(_, value)| field(value, "body") == Some(Value::String(base_body.clone())))
            .collect::<Vec<_>>();
        for ((region, value), (base_region, base_value)) in regions.iter().zip(&base_regions) {
            pair(&mut map, &mut used, region, base_region);
            let shells = |value: &Value| -> Vec<String> {
                value
                    .get("shells")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|shell| shell.as_str().map(str::to_owned))
                    .collect()
            };
            for (shell, base_shell) in shells(value).iter().zip(shells(base_value)) {
                pair(&mut map, &mut used, shell, &base_shell);
            }
        }
    }

    // Carriers, vertices, and edges, by content, until no further match.
    loop {
        let mut matched = false;
        for arena in CARRIERS {
            let Some(base_records) = base_rows(arena) else {
                continue;
            };
            let mut by_content = HashMap::<String, Vec<&String>>::new();
            for (id, value) in base_records {
                if !used.contains(id) {
                    by_content
                        .entry(compared(value).to_string())
                        .or_default()
                        .push(id);
                }
            }
            for (id, value) in rows(arena) {
                if map.contains_key(id) {
                    continue;
                }
                let content = compared(&mapped(value, &map)).to_string();
                let Some(old) = by_content.get_mut(&content).and_then(|candidates| {
                    candidates.retain(|candidate| !used.contains(*candidate));
                    candidates.first().copied()
                }) else {
                    continue;
                };
                pair(&mut map, &mut used, id, old);
                matched = true;
            }
        }
        if !matched {
            break;
        }
    }

    // Loops, coedges, and faces together: the largest set of pairs under
    // which every pair is equal with its references mapped through the
    // earlier matches and through the set itself.
    let open = COUPLED
        .iter()
        .flat_map(|arena| {
            rows(arena).iter().map(|(id, _)| id.as_str()).chain(
                base_rows(arena)
                    .into_iter()
                    .flatten()
                    .map(|(id, _)| id.as_str()),
            )
        })
        .collect::<HashSet<_>>();
    let mut candidates = HashMap::<&str, Vec<(&str, Vec<String>, Vec<String>)>>::new();
    for arena in COUPLED {
        let mut by_shape = HashMap::<String, Vec<(&str, Vec<String>)>>::new();
        for (id, value) in base_rows(arena).into_iter().flatten() {
            let mut held = Vec::new();
            let shape = placeheld(&compared(value), &HashMap::new(), &open, &mut held).to_string();
            by_shape.entry(shape).or_default().push((id.as_str(), held));
        }
        for (id, value) in rows(arena) {
            let mut held = Vec::new();
            let shape = placeheld(&compared(value), &map, &open, &mut held).to_string();
            if let Some(bases) = by_shape.get(&shape) {
                candidates.insert(
                    id.as_str(),
                    bases
                        .iter()
                        .map(|(old, base_held)| (*old, held.clone(), base_held.clone()))
                        .collect(),
                );
            }
        }
    }
    let consistent = |pairs: &HashMap<&str, Vec<&str>>, new: &[String], old: &[String]| {
        new.iter().zip(old).all(|(new, old)| {
            pairs
                .get(new.as_str())
                .is_some_and(|images| images.contains(&old.as_str()))
        })
    };
    let mut pairs = candidates
        .iter()
        .map(|(new, bases)| (*new, bases.iter().map(|(old, _, _)| *old).collect()))
        .collect::<HashMap<&str, Vec<&str>>>();
    loop {
        let mut removed = false;
        for (new, bases) in &candidates {
            for (old, held, base_held) in bases {
                let present = pairs.get(new).is_some_and(|images| images.contains(old));
                if present && !consistent(&pairs, held, base_held) {
                    if let Some(images) = pairs.get_mut(new) {
                        images.retain(|image| image != old);
                    }
                    removed = true;
                }
            }
        }
        if !removed {
            break;
        }
    }
    let mut chosen = BTreeMap::<&str, &str>::new();
    let mut taken = HashSet::<&str>::new();
    let mut order = pairs.keys().copied().collect::<Vec<_>>();
    order.sort_unstable();
    for new in order {
        if let Some(old) = pairs[new]
            .iter()
            .copied()
            .find(|old| !taken.contains(old) && !used.contains(*old))
        {
            chosen.insert(new, old);
            taken.insert(old);
        }
    }
    loop {
        let unchosen = chosen
            .iter()
            .filter(|&(&new, &old)| {
                let Some((_, held, base_held)) = candidates[new]
                    .iter()
                    .find(|(candidate, _, _)| *candidate == old)
                else {
                    return true;
                };
                !held
                    .iter()
                    .zip(base_held)
                    .all(|(new, old)| chosen.get(new.as_str()) == Some(&old.as_str()))
            })
            .map(|(&new, _)| new)
            .collect::<Vec<_>>();
        if unchosen.is_empty() {
            break;
        }
        for new in unchosen {
            chosen.remove(new);
        }
    }
    for (new, old) in chosen {
        pair(&mut map, &mut used, new, old);
    }

    // The state: every record under its id in the state; a record equal to
    // the base's record of that id but for `source_object` is shared and
    // keeps the base's, every other is in the delta.
    let mut resolved = Resolved::default();
    let mut stated = BTreeMap::<&'static str, BTreeMap<String, Value>>::new();
    let mut renamed = Vec::new();
    for (arena, _) in ARENAS {
        let stated = stated.entry(arena).or_default();
        let mut state = BTreeMap::new();
        for (id, value) in rows(arena) {
            let id_in_state = map.get(id).cloned().unwrap_or_else(|| id.clone());
            let mut value = mapped(value, &map);
            let shared = base_rows(arena)
                .and_then(|records| records.get(&id_in_state))
                .filter(|base_value| compared(base_value) == compared(&value));
            if let Some(base_value) = shared {
                value = base_value.clone();
            } else {
                stated.insert(id_in_state.clone(), value.clone());
            }
            renamed.push((id.clone(), id_in_state.clone()));
            state.insert(id_in_state, value);
        }
        if arena == "bodies" {
            resolved.bodies = rows(arena)
                .iter()
                .map(|(id, _)| map.get(id).cloned().unwrap_or_else(|| id.clone()))
                .collect();
        }
        resolved.arenas.insert(arena, state);
    }
    // A topology holds every carrier a carrier of it names: a procedural
    // construction's basis and a procedural carrier's construction. A
    // shared carrier that a stated one names is restated as the base
    // holds it.
    let mut pending = CARRIER_LINKS
        .iter()
        .flat_map(|arena| stated[arena].keys().map(move |id| (*arena, id.clone())))
        .collect::<Vec<_>>();
    while let Some((arena, id)) = pending.pop() {
        let mut named = Vec::new();
        strings(&stated[arena][&id], &mut named);
        for target in named {
            for linked in CARRIER_LINKS {
                if stated[linked].contains_key(&target) {
                    continue;
                }
                if let Some(value) = resolved.arenas[linked].get(&target) {
                    stated
                        .entry(linked)
                        .or_default()
                        .insert(target.clone(), value.clone());
                    pending.push((linked, target.clone()));
                }
            }
        }
    }
    let mut delta = Map::new();
    for (arena, _) in ARENAS {
        let records = stated
            .remove(arena)
            .unwrap_or_default()
            .into_iter()
            .map(|(id, mut record)| {
                if let Value::Object(fields) = &mut record {
                    fields.insert("id".to_owned(), Value::String(id));
                }
                record
            })
            .collect();
        delta.insert(arena.to_owned(), Value::Array(records));
    }
    let mut removed = Vec::new();
    for (arena, kind) in ARENAS {
        let state = &resolved.arenas[arena];
        for id in base_rows(arena).into_iter().flatten().map(|(id, _)| id) {
            if !state.contains_key(id) {
                removed.push(
                    serde_json::from_value::<RecordRef>(serde_json::json!({
                        "kind": kind,
                        "value": id,
                    }))
                    .map_err(CodecError::malformed)?,
                );
            }
        }
    }
    Ok(Snapshot {
        records: serde_json::from_value(Value::Object(delta)).map_err(CodecError::malformed)?,
        removed,
        renamed,
        resolved,
    })
}

/// Restate the model-class and design records that name a record the pass
/// moved out of the geometry: an appearance binding or a tessellation of it
/// stays in its source record, with a loss; a legacy feature lists only the
/// bodies of the current model it produces; and a stored-geometry feature
/// names the bodies of the state after it (§5.10).
fn restate_references(
    ir: &mut CadIr,
    moved: &HashMap<String, String>,
    payload_states: &HashSet<String>,
    native: &HashSet<String>,
    design: Option<&Design>,
    homes: &BTreeMap<String, (Home, &IntermediatePayload<'_>)>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let dropped = |id: &str| payload_of(id, |key| native.contains(key)).is_some();
    // The presentation records of a moved shape stay in the GUI document and
    // the shape payload record: the validator resolves a model-class record's
    // target in the current geometry.
    let away = |id: &str| {
        payload_of(id, |key| {
            native.contains(key) || payload_states.contains(key)
        })
        .is_some()
    };
    let before = ir.model.appearance_bindings.len();
    ir.model.appearance_bindings.retain(|binding| {
        let entity = match &binding.target.entity {
            AppearanceTarget::Body(id) => id.as_str(),
            AppearanceTarget::Face(id) => id.as_str(),
            AppearanceTarget::Edge(id) => id.as_str(),
            AppearanceTarget::Vertex(id) => id.as_str(),
            AppearanceTarget::Surface(id) => id.as_str(),
            AppearanceTarget::Curve(id) => id.as_str(),
            AppearanceTarget::Point(id) => id.as_str(),
            AppearanceTarget::Tessellation(id)
            | AppearanceTarget::TriangleGroup {
                tessellation: id, ..
            } => id.as_str(),
            _ => return true,
        };
        !away(entity)
    });
    let bindings = before - ir.model.appearance_bindings.len();
    let before = ir.model.tessellations.len();
    ir.model
        .tessellations
        .retain(|tessellation| !away(tessellation.id.as_str()));
    let tessellations = before - ir.model.tessellations.len();
    if bindings + tessellations > 0 {
        losses.push(FreecadLossCode::AppearanceStateNotTransferred.note(format!(
            "{bindings} appearance binding(s) and {tessellations} tessellation(s) of shapes held outside the current geometry remain in the GUI document and the shape payload records"
        )));
    }

    let bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    for feature in &mut ir.model.features {
        if feature
            .evaluation
            .outputs()
            .iter()
            .all(|body| bodies.contains(body.as_str()))
        {
            continue;
        }
        let outputs = feature
            .evaluation
            .outputs()
            .iter()
            .filter(|body| bodies.contains(body.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        feature.evaluation.set_outputs(
            outputs
                .try_into()
                .map_err(|_| CodecError::malformed("feature outputs repeat a body"))?,
        );
    }

    // A stored-geometry feature names the bodies of the state after it: an
    // evaluation's, or the current state's where it is the `Tip`.
    let tip_repeats = homes
        .iter()
        .filter(|(_, (home, payload))| {
            *home == Home::Native
                && payload.property == "Shape"
                && design.is_some_and(|design| {
                    design.feature_of.get(payload.owner).is_some_and(|feature| {
                        design
                            .member_of
                            .get(feature)
                            .is_some_and(|(body, member)| design.bodies[*body].tip == Some(*member))
                    })
                })
        })
        .filter_map(|(key, (_, payload))| {
            let design = design?;
            let feature = design.feature_of.get(payload.owner)?;
            let (body, _) = design.member_of.get(feature)?;
            Some((key.as_str(), design.bodies[*body].current))
        })
        .collect::<HashMap<_, _>>();
    let current = ir
        .design
        .scopes
        .first()
        .map(|scope| scope.current.clone())
        .unwrap_or_default();
    for feature in &mut ir.design.features {
        let FeatureOperation::Stored {
            bodies: Some(named),
            ..
        } = &mut feature.definition.operation
        else {
            continue;
        };
        let mut restated = Vec::new();
        for body in named.iter() {
            if let Some(state) = payload_of(body.as_str(), |key| tip_repeats.contains_key(key))
                .map(|key| tip_repeats[key])
            {
                restated.extend(current[state].bodies.iter().cloned());
            } else if let Some(id) = moved.get(body.as_str()) {
                restated.push(mint(id.clone())?);
            } else if !dropped(body.as_str()) {
                restated.push(body.clone());
            }
        }
        restated.dedup();
        *named = restated;
    }
    Ok(())
}

fn mint<T: TryFrom<String>>(id: String) -> Result<T, CodecError>
where
    T::Error: std::fmt::Display,
{
    T::try_from(id).map_err(|error| CodecError::malformed(error.to_string()))
}

#[cfg(test)]
mod tests;
