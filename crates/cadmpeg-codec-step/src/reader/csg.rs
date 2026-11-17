// SPDX-License-Identifier: Apache-2.0
//! Constructive solid geometry (ISO 10303-42 §6.4.6 to §6.4.39).
//!
//! Each CSG primitive and swept-area solid becomes a design feature that
//! states the source operation, and, where it is not an operand of a Boolean
//! tree, a solid body derived from it. A `boolean_result` becomes a combine
//! feature; a Boolean tree has no derived body.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::draft::CommitSession;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::report::loss::LossNote;

use self::angles::AngleUnits;
use self::brep::{Axis, SolidBuilder};
use self::frame::Frame;
use self::profile::Profile;
use super::geometry::GeometryData;
use super::topology::TopologyData;
use super::{RecordExt, StageOutcome, ValueExt};
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

mod angles;
mod brep;
mod design;
mod frame;
mod profile;

pub(super) use design::attach as attach_design;

/// The CSG primitive and swept-solid entity types this stage reads.
const SOLID_TYPES: [&str; 9] = [
    "BLOCK",
    "RIGHT_ANGULAR_WEDGE",
    "RIGHT_CIRCULAR_CYLINDER",
    "RIGHT_CIRCULAR_CONE",
    "SPHERE",
    "TORUS",
    "ELLIPSOID",
    "EXTRUDED_AREA_SOLID",
    "REVOLVED_AREA_SOLID",
];

/// A regularised Boolean operator (ISO 10303-42 §6.4.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operator {
    Union,
    Intersection,
    Difference,
}

/// The shape a CSG item states, in model millimetres and radians.
#[derive(Debug, Clone)]
enum Shape {
    /// The box spanning the frame's positive octant.
    Block { frame: Frame, size: [f64; 3] },
    /// The trapezoid `(0,0) (x,0) (ltx,y) (0,y)` of the frame's xy plane,
    /// extruded along z.
    Wedge {
        frame: Frame,
        size: [f64; 3],
        ltx: f64,
    },
    /// The cylinder standing on the frame's xy plane about +z.
    Cylinder {
        frame: Frame,
        radius: f64,
        height: f64,
    },
    /// The cone standing on the frame's xy plane about +z, with the base and
    /// top radii; `source` is the stated placement at the top face.
    Cone {
        frame: Frame,
        radii: [f64; 2],
        height: f64,
        source: Frame,
    },
    /// The ball about the frame origin.
    Sphere { frame: Frame, radius: f64 },
    /// The torus about the frame's z axis.
    Torus {
        frame: Frame,
        major: f64,
        minor: f64,
    },
    /// The ellipsoid with semi-axes along the frame axes.
    Ellipsoid { frame: Frame, semi_axes: [f64; 3] },
    /// A planar area swept along a direction.
    Extruded {
        area: u64,
        profile: Result<Profile, String>,
        direction: Vector3,
        depth: f64,
    },
    /// A planar area turned about an axis.
    Revolved {
        area: u64,
        profile: Result<Profile, String>,
        axis: Frame,
        angle: f64,
    },
    /// A Boolean of two operands.
    Boolean {
        operator: Operator,
        operands: [u64; 2],
    },
}

/// One CSG item read from the exchange.
#[derive(Debug, Clone)]
struct Item {
    id: u64,
    entity: &'static str,
    name: Option<String>,
    shape: Shape,
    /// The frame in which the item's own placement is stated, where a
    /// plant item orientation places it.
    orientation: Option<Frame>,
}

impl Item {
    /// The item's shape in model coordinates.
    fn placed_shape(&self) -> Shape {
        match &self.orientation {
            Some(frame) => self.shape.placed(frame),
            None => self.shape.clone(),
        }
    }
}

impl Shape {
    /// The shape whose placements are stated in `frame`, in model
    /// coordinates.
    fn placed(&self, frame: &Frame) -> Self {
        let place = |point: cadmpeg_ir::math::Point3| frame.point(point.x, point.y, point.z);
        let turn = |vector: Vector3| {
            frame.x.scale(vector.x) + frame.y.scale(vector.y) + frame.z.scale(vector.z)
        };
        let moved = |inner: &Frame| Frame {
            origin: place(inner.origin),
            x: turn(inner.x),
            y: turn(inner.y),
            z: turn(inner.z),
        };
        match self {
            Self::Block { frame, size } => Self::Block {
                frame: moved(frame),
                size: *size,
            },
            Self::Wedge { frame, size, ltx } => Self::Wedge {
                frame: moved(frame),
                size: *size,
                ltx: *ltx,
            },
            Self::Cylinder {
                frame,
                radius,
                height,
            } => Self::Cylinder {
                frame: moved(frame),
                radius: *radius,
                height: *height,
            },
            Self::Cone {
                frame,
                radii,
                height,
                source,
            } => Self::Cone {
                frame: moved(frame),
                radii: *radii,
                height: *height,
                source: moved(source),
            },
            Self::Sphere { frame, radius } => Self::Sphere {
                frame: moved(frame),
                radius: *radius,
            },
            Self::Torus {
                frame,
                major,
                minor,
            } => Self::Torus {
                frame: moved(frame),
                major: *major,
                minor: *minor,
            },
            Self::Ellipsoid { frame, semi_axes } => Self::Ellipsoid {
                frame: moved(frame),
                semi_axes: *semi_axes,
            },
            Self::Extruded {
                area,
                profile,
                direction,
                depth,
            } => Self::Extruded {
                area: *area,
                profile: profile
                    .as_ref()
                    .map(|profile| profile.moved(&place, &turn))
                    .map_err(Clone::clone),
                direction: turn(*direction),
                depth: *depth,
            },
            Self::Revolved {
                area,
                profile,
                axis,
                angle,
            } => Self::Revolved {
                area: *area,
                profile: profile
                    .as_ref()
                    .map(|profile| profile.moved(&place, &turn))
                    .map_err(Clone::clone),
                axis: moved(axis),
                angle: *angle,
            },
            Self::Boolean { .. } => self.clone(),
        }
    }
}

/// What the stage hands to the design attachment after product decoding.
pub(super) struct CsgData {
    items: BTreeMap<u64, Item>,
    /// The body derived from each item, by item id.
    bodies: BTreeMap<u64, BodyId>,
    /// The feature item ids each representation holds, operands before the
    /// Boolean that uses them.
    representations: BTreeMap<u64, Vec<u64>>,
    /// Every identity of a derived body.
    pub(super) derived: Vec<String>,
}

/// Read every CSG item, derive the bodies of those outside Boolean trees,
/// and index them as representation items.
pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryData,
    topology: &mut TopologyData,
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<CsgData>, CodecError> {
    let mut losses = Vec::new();
    let mut claims = HashSet::new();
    let uncertainty = ir.tolerances.linear.get();
    let angles = AngleUnits::new(geometry);
    let reader = ShapeReader {
        exchange,
        geometry,
        angles: &angles,
        uncertainty,
    };
    let mut gap = (0.0_f64, None);
    let mut items = BTreeMap::new();
    for (id, record) in exchange.entities_any(&SOLID_TYPES) {
        let Some(entity) = record
            .simple_name()
            .and_then(|name| SOLID_TYPES.iter().copied().find(|known| *known == name))
        else {
            continue;
        };
        let mut item_gap = 0.0_f64;
        let read = reader.shape(id, entity, record, &mut item_gap);
        if item_gap > gap.0 {
            gap = (item_gap, Some(id));
        }
        match read {
            Ok(shape) => {
                let name = item_name(exchange, record, id, &mut losses);
                items.insert(
                    id,
                    Item {
                        id,
                        entity,
                        name,
                        shape,
                        orientation: None,
                    },
                );
            }
            Err(message) => losses
                .push(StepLossCode::CsgItemUnresolved.note(format!("{entity} #{id}: {message}"))),
        }
    }
    for (id, record) in exchange.entities("BOOLEAN_RESULT") {
        match read_boolean(record) {
            Some((operator, operands)) => {
                let name = item_name(exchange, record, id, &mut losses);
                items.insert(
                    id,
                    Item {
                        id,
                        entity: "BOOLEAN_RESULT",
                        name,
                        shape: Shape::Boolean { operator, operands },
                        orientation: None,
                    },
                );
            }
            None => losses.push(StepLossCode::CsgItemUnresolved.note(format!(
                "BOOLEAN_RESULT #{id} has no operator or no operands"
            ))),
        }
    }
    let solids = csg_solid_roots(exchange);
    let resolve = |id: u64| solids.get(&id).copied().unwrap_or(id);
    for item in items.values_mut() {
        if let Shape::Boolean { operands, .. } = &mut item.shape {
            *operands = operands.map(resolve);
        }
    }
    let refused = refuse_cycles(&items, &mut losses);
    for (item, frame) in plant_orientations(exchange, geometry, &items, &solids, &mut losses) {
        if let Some(item) = items.get_mut(&item) {
            item.orientation = Some(frame);
        }
    }
    let mut operands = BTreeSet::new();
    for item in items.values() {
        if let Shape::Boolean { operands: pair, .. } = item.shape {
            operands.extend(pair);
        }
    }
    let mut bodies = BTreeMap::new();
    let mut derived = Vec::new();
    let tolerance = ir.tolerances.linear.get();
    {
        let mut session = CommitSession::new(&mut *ir, ctx, None)?;
        for item in items.values() {
            if matches!(item.shape, Shape::Boolean { .. }) || operands.contains(&item.id) {
                continue;
            }
            let mut builder = SolidBuilder::new(item.id, tolerance);
            if let Err(message) = build(&mut builder, &item.placed_shape()) {
                losses.push(StepLossCode::CsgSolidUnsynthesized.note(format!(
                    "{} #{} has a design feature and no derived body: {message}",
                    item.entity, item.id
                )));
                continue;
            }
            let built = match builder.finish(item.name.clone()) {
                Ok(built) => built,
                Err(message) => {
                    losses.push(StepLossCode::CsgSolidUnsynthesized.note(format!(
                        "{} #{} has a design feature and no derived body: {message}",
                        item.entity, item.id
                    )));
                    continue;
                }
            };
            if built.faces == 0 {
                losses.push(StepLossCode::CsgSolidUnsynthesized.note(format!(
                    "{} #{} has a design feature and no derived body: its boundary has no face",
                    item.entity, item.id
                )));
                continue;
            }
            if let Err(error) = session.commit_model(built.draft)? {
                losses.push(StepLossCode::CsgSolidUnsynthesized.note(format!(
                    "{} #{} has a design feature and no derived body: {error}",
                    item.entity, item.id
                )));
                continue;
            }
            derived.extend(built.identities);
            bodies.insert(item.id, built.body);
        }
    }
    for (&item, body) in &bodies {
        topology.body_by_root.insert(item, vec![body.clone()]);
    }
    for (&solid, &root) in &solids {
        if let Some(body) = bodies.get(&root) {
            topology.body_by_root.insert(solid, vec![body.clone()]);
        }
        if items.contains_key(&root) {
            claims.insert(solid);
        }
    }
    for (&id, item) in &items {
        claims.insert(id);
        if let Shape::Boolean { .. } = item.shape {
            if !operands.contains(&id) && !refused.contains(&id) {
                losses.push(StepLossCode::CsgBooleanUnevaluated.note(format!(
                    "BOOLEAN_RESULT #{id} has design features and no derived body; \
                     evaluating a regularised Boolean needs a solid modelling kernel"
                )));
            }
        }
    }
    let mut representations = representation_members(exchange, &items, &solids);
    for members in representations.values_mut() {
        members.retain(|member| !refused.contains(member));
    }
    representations.retain(|_, members| !members.is_empty());
    items.retain(|id, _| !refused.contains(id));
    for &representation in representations.keys() {
        claims.insert(representation);
    }
    if let (distance, Some(owner)) = gap {
        if distance > uncertainty {
            losses.push(StepLossCode::CsgProfileGapClosed.note(format!(
                "swept-area profiles close boundary curves whose segment ends lie up to \
                 {distance:.6e} mm apart, above the linear uncertainty {uncertainty:e} mm; \
                 the largest gap is at #{owner}"
            )));
        }
    }
    Ok(StageOutcome {
        value: CsgData {
            items,
            bodies,
            representations,
            derived,
        },
        claims,
        losses,
        notes: Vec::new(),
    })
}

fn item_name(
    exchange: &Exchange,
    record: &RawRecord,
    id: u64,
    losses: &mut Vec<LossNote>,
) -> Option<String> {
    record
        .parameter(0)
        .and_then(|value| {
            super::decode_text(
                exchange,
                value,
                losses,
                id,
                "representation item name",
                StepLossCode::MetadataStringInvalid,
            )
        })
        .filter(|name| !name.is_empty())
}

/// The name of the placement that states the frame of a plant CSG shape's
/// items.
const PLANT_ITEM_ORIENTATION: &str = "plant item orientation";

/// The frame of the items of each `PLANT_CSG_SHAPE_REPRESENTATION` that
/// lists one `AXIS2_PLACEMENT_3D` named [`PLANT_ITEM_ORIENTATION`], by item;
/// a Boolean tree's operands take its root's frame. A representation with
/// several such placements places no item.
fn plant_orientations(
    exchange: &Exchange,
    geometry: &GeometryData,
    items: &BTreeMap<u64, Item>,
    solids: &BTreeMap<u64, u64>,
    losses: &mut Vec<LossNote>,
) -> BTreeMap<u64, Frame> {
    let mut frames = BTreeMap::new();
    for (representation, record) in exchange.entities("PLANT_CSG_SHAPE_REPRESENTATION") {
        let Some(values) = super::topology::representation_item_values(record) else {
            continue;
        };
        let members: Vec<u64> = values.iter().filter_map(Value::reference).collect();
        let orientations: Vec<u64> = members
            .iter()
            .copied()
            .filter(|member| {
                exchange.records().get(member).is_some_and(|candidate| {
                    candidate.simple_name() == Some("AXIS2_PLACEMENT_3D")
                        && matches!(
                            candidate.parameter(0),
                            Some(Value::String(bytes)) if exchange
                                .decode_string(bytes)
                                .is_ok_and(|name| name.trim() == PLANT_ITEM_ORIENTATION)
                        )
                })
            })
            .collect();
        let frame = match orientations.as_slice() {
            [] => continue,
            [only] => match geometry.placements.get(only) {
                Some(&placement) => Frame::from_placement(placement),
                None => continue,
            },
            _ => {
                losses.push(StepLossCode::CsgPlantOrientationAmbiguous.note(format!(
                    "PLANT_CSG_SHAPE_REPRESENTATION #{representation} lists {} placements \
                     named '{PLANT_ITEM_ORIENTATION}'; its items keep their own placements",
                    orientations.len()
                )));
                continue;
            }
        };
        let mut tree = Vec::new();
        let mut seen = BTreeSet::new();
        for member in members {
            let root = solids.get(&member).copied().unwrap_or(member);
            collect_tree(root, items, &mut seen, &mut tree);
        }
        for item in tree {
            frames.entry(item).or_insert(frame);
        }
    }
    frames
}

/// The tree root of each `CSG_SOLID`, by solid id.
fn csg_solid_roots(exchange: &Exchange) -> BTreeMap<u64, u64> {
    let mut roots = BTreeMap::new();
    for (id, record) in exchange.entities("CSG_SOLID") {
        if record.simple_name() != Some("CSG_SOLID") {
            continue;
        }
        if let Some(root) = record.parameter(1).and_then(Value::reference) {
            roots.insert(id, root);
        }
    }
    roots
}

fn read_boolean(record: &RawRecord) -> Option<(Operator, [u64; 2])> {
    if record.simple_name() != Some("BOOLEAN_RESULT") {
        return None;
    }
    let operator = match record.parameter(1)?.enumeration()? {
        "UNION" => Operator::Union,
        "INTERSECTION" => Operator::Intersection,
        "DIFFERENCE" => Operator::Difference,
        _ => return None,
    };
    let first = record.parameter(2)?.reference()?;
    let second = record.parameter(3)?.reference()?;
    Some((operator, [first, second]))
}

/// The feature items of each representation that lists a CSG item, a
/// `CSG_SOLID` of one, or both, with each Boolean's operands before it.
fn representation_members(
    exchange: &Exchange,
    items: &BTreeMap<u64, Item>,
    solids: &BTreeMap<u64, u64>,
) -> BTreeMap<u64, Vec<u64>> {
    let mut representations = BTreeMap::new();
    for (&id, record) in exchange.records() {
        let Some(values) = super::topology::representation_item_values(record) else {
            continue;
        };
        let mut members = Vec::new();
        let mut seen = BTreeSet::new();
        for value in values {
            let Some(item) = value.reference() else {
                continue;
            };
            let root = solids.get(&item).copied().unwrap_or(item);
            collect_tree(root, items, &mut seen, &mut members);
        }
        if !members.is_empty() {
            representations.insert(id, members);
        }
    }
    representations
}

/// Append the items of the tree at `root`, operands first, each once.
fn collect_tree(
    root: u64,
    items: &BTreeMap<u64, Item>,
    seen: &mut BTreeSet<u64>,
    members: &mut Vec<u64>,
) {
    let mut pending = vec![(root, false)];
    while let Some((id, expanded)) = pending.pop() {
        if expanded {
            members.push(id);
            continue;
        }
        let Some(item) = items.get(&id) else {
            continue;
        };
        if !seen.insert(id) {
            continue;
        }
        pending.push((id, true));
        if let Shape::Boolean { operands, .. } = item.shape {
            pending.push((operands[1], false));
            pending.push((operands[0], false));
        }
    }
}

/// The Boolean results that lie on an operand cycle or have one in their
/// operand graph. A Boolean tree is acyclic (ISO 10303-42 §6.4.6); each
/// cycle found and each Boolean that uses one has a `csg.item-unresolved`
/// loss.
fn refuse_cycles(items: &BTreeMap<u64, Item>, losses: &mut Vec<LossNote>) -> BTreeSet<u64> {
    let operands_of = |id: u64| match items.get(&id).map(|item| &item.shape) {
        Some(Shape::Boolean { operands, .. }) => Some(*operands),
        _ => None,
    };
    let mut finished = BTreeSet::new();
    let mut on_path = BTreeSet::new();
    let mut refused = BTreeSet::new();
    let mut dependents = BTreeSet::new();
    for &start in items.keys() {
        let Some(pair) = operands_of(start) else {
            continue;
        };
        if finished.contains(&start) {
            continue;
        }
        let mut path = vec![(start, pair, 0_usize)];
        on_path.insert(start);
        while let Some(top) = path.last_mut() {
            let (id, pair) = (top.0, top.1);
            let Some(&operand) = pair.get(top.2) else {
                path.pop();
                on_path.remove(&id);
                finished.insert(id);
                if !refused.contains(&id) && pair.iter().any(|operand| refused.contains(operand)) {
                    refused.insert(id);
                    dependents.insert(id);
                }
                continue;
            };
            top.2 += 1;
            let Some(next) = operands_of(operand) else {
                continue;
            };
            if finished.contains(&operand) {
                continue;
            }
            if on_path.contains(&operand) {
                let entry = path
                    .iter()
                    .rposition(|&(member, ..)| member == operand)
                    .unwrap_or_default();
                let cycle: Vec<u64> = path[entry..].iter().map(|&(member, ..)| member).collect();
                let chain = cycle
                    .iter()
                    .chain(std::iter::once(&operand))
                    .map(|member| format!("#{member}"))
                    .collect::<Vec<_>>()
                    .join(" -> ");
                losses.push(StepLossCode::CsgItemUnresolved.note(format!(
                    "BOOLEAN_RESULT #{operand} is its own operand through {chain}; \
                     a Boolean tree is acyclic (ISO 10303-42 §6.4.6), so the cycle has \
                     no feature"
                )));
                refused.extend(cycle);
                continue;
            }
            path.push((operand, next, 0));
            on_path.insert(operand);
        }
    }
    for id in dependents {
        losses.push(StepLossCode::CsgItemUnresolved.note(format!(
            "BOOLEAN_RESULT #{id} has an operand cycle in its tree, so it has no feature"
        )));
    }
    refused
}

/// A positive finite length attribute, scaled to millimetres.
fn length(record: &RawRecord, index: usize, scale: f64, what: &str) -> Result<f64, String> {
    record
        .parameter(index)
        .and_then(Value::number)
        .map(|value| value * scale)
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| format!("{what} is not a positive length"))
}

fn placement(record: &RawRecord, index: usize, geometry: &GeometryData) -> Result<Frame, String> {
    let id = record
        .parameter(index)
        .and_then(Value::reference)
        .ok_or("the position is not a reference")?;
    geometry
        .placements
        .get(&id)
        .map(|&placement| Frame::from_placement(placement))
        .ok_or_else(|| format!("the position #{id} does not resolve"))
}

/// The exchange context that reads a CSG item's attributes.
struct ShapeReader<'a> {
    exchange: &'a Exchange,
    geometry: &'a GeometryData,
    angles: &'a AngleUnits<'a>,
    uncertainty: f64,
}

impl ShapeReader<'_> {
    /// The planar area of a swept-area solid; `gap` takes the largest
    /// junction gap it closed.
    fn profile(&self, area: u64, gap: &mut f64) -> Result<Profile, String> {
        let read = profile::read(
            area,
            self.exchange,
            self.geometry,
            self.angles,
            self.uncertainty,
        )?;
        *gap = gap.max(read.gap);
        Ok(read.profile)
    }

    /// The shape of item `id` of type `entity`.
    fn shape(
        &self,
        id: u64,
        entity: &str,
        record: &RawRecord,
        gap: &mut f64,
    ) -> Result<Shape, String> {
        let geometry = self.geometry;
        read_shape(id, entity, record, geometry, self, gap)
    }
}

fn read_shape(
    id: u64,
    entity: &str,
    record: &RawRecord,
    geometry: &GeometryData,
    reader: &ShapeReader<'_>,
    gap: &mut f64,
) -> Result<Shape, String> {
    let scale = geometry.units.length([id]).get();
    match entity {
        "BLOCK" => Ok(Shape::Block {
            frame: placement(record, 1, geometry)?,
            size: [
                length(record, 2, scale, "x")?,
                length(record, 3, scale, "y")?,
                length(record, 4, scale, "z")?,
            ],
        }),
        "RIGHT_ANGULAR_WEDGE" => {
            let x = length(record, 2, scale, "x")?;
            let ltx = record
                .parameter(5)
                .and_then(Value::number)
                .map(|value| value * scale)
                .filter(|ltx| ltx.is_finite() && (0.0..x).contains(ltx))
                .ok_or("ltx is not in [0, x) (WR1)")?;
            Ok(Shape::Wedge {
                frame: placement(record, 1, geometry)?,
                size: [
                    x,
                    length(record, 3, scale, "y")?,
                    length(record, 4, scale, "z")?,
                ],
                ltx,
            })
        }
        "RIGHT_CIRCULAR_CYLINDER" => Ok(Shape::Cylinder {
            frame: placement(record, 1, geometry)?,
            height: length(record, 2, scale, "height")?,
            radius: length(record, 3, scale, "radius")?,
        }),
        "RIGHT_CIRCULAR_CONE" => {
            let source = placement(record, 1, geometry)?;
            let height = length(record, 2, scale, "height")?;
            let top = record
                .parameter(3)
                .and_then(Value::number)
                .map(|value| value * scale)
                .filter(|radius| radius.is_finite() && *radius >= 0.0)
                .ok_or("radius is negative (WR1)")?;
            let semi_angle = record
                .parameter(4)
                .and_then(Value::number)
                .map(|value| reader.angles.radians(id, value))
                .filter(|angle| *angle > 0.0 && *angle < std::f64::consts::FRAC_PI_2)
                .ok_or("semi_angle is not between 0 and 90 degrees (IP1)")?;
            let base = top + height * semi_angle.tan();
            Ok(Shape::Cone {
                frame: source.shifted(-height),
                radii: [base, top],
                height,
                source,
            })
        }
        "SPHERE" => {
            let radius = length(record, 1, scale, "radius")?;
            let centre = record
                .parameter(2)
                .and_then(Value::reference)
                .and_then(|point| geometry.points.get(&point))
                .ok_or("the centre does not resolve")?;
            let frame = Frame {
                origin: centre.get(),
                x: Vector3::new(1.0, 0.0, 0.0),
                y: Vector3::new(0.0, 1.0, 0.0),
                z: Vector3::new(0.0, 0.0, 1.0),
            };
            Ok(Shape::Sphere { frame, radius })
        }
        "TORUS" => {
            let major = length(record, 2, scale, "major_radius")?;
            let minor = length(record, 3, scale, "minor_radius")?;
            if major <= minor {
                return Err("major_radius is not greater than minor_radius (WR1)".to_owned());
            }
            Ok(Shape::Torus {
                frame: placement(record, 1, geometry)?,
                major,
                minor,
            })
        }
        "ELLIPSOID" => Ok(Shape::Ellipsoid {
            frame: placement(record, 1, geometry)?,
            semi_axes: [
                length(record, 2, scale, "semi_axis_1")?,
                length(record, 3, scale, "semi_axis_2")?,
                length(record, 4, scale, "semi_axis_3")?,
            ],
        }),
        "EXTRUDED_AREA_SOLID" => {
            let area = record
                .parameter(1)
                .and_then(Value::reference)
                .ok_or("swept_area is not a reference")?;
            let direction = record
                .parameter(2)
                .and_then(Value::reference)
                .and_then(|direction| geometry.directions.get(&direction))
                .map(|direction| *direction.as_raw())
                .ok_or("extruded_direction does not resolve")?;
            Ok(Shape::Extruded {
                area,
                profile: reader.profile(area, gap),
                direction,
                depth: length(record, 3, scale, "depth")?,
            })
        }
        "REVOLVED_AREA_SOLID" => {
            let area = record
                .parameter(1)
                .and_then(Value::reference)
                .ok_or("swept_area is not a reference")?;
            let angle = record
                .parameter(3)
                .and_then(Value::number)
                .filter(|value| *value > 0.0)
                .and_then(|value| reader.angles.sweep(id, value))
                .ok_or("angle is not in (0, 360] degrees (IP3)")?;
            Ok(Shape::Revolved {
                area,
                profile: reader.profile(area, gap),
                axis: placement(record, 2, geometry)?,
                angle,
            })
        }
        _ => Err(format!("{entity} is not a CSG primitive")),
    }
}

/// Build the boundary of `shape`.
fn build(builder: &mut SolidBuilder, shape: &Shape) -> Result<(), String> {
    match shape {
        Shape::Block { frame, size } => {
            let [x, y, z] = *size;
            let base = brep::base_polygon(frame, &[[0.0, 0.0], [x, 0.0], [x, y], [0.0, y]]);
            brep::extrude(builder, &base, frame.z, z)
        }
        Shape::Wedge { frame, size, ltx } => {
            let [x, y, z] = *size;
            let corners: Vec<[f64; 2]> = if *ltx > 0.0 {
                vec![[0.0, 0.0], [x, 0.0], [*ltx, y], [0.0, y]]
            } else {
                vec![[0.0, 0.0], [x, 0.0], [0.0, y]]
            };
            let base = brep::base_polygon(frame, &corners);
            brep::extrude(builder, &base, frame.z, z)
        }
        Shape::Cylinder {
            frame,
            radius,
            height,
        } => brep::extrude(
            builder,
            &brep::disc_profile(frame, *radius),
            frame.z,
            *height,
        ),
        Shape::Cone {
            frame,
            radii: [base, top],
            height,
            ..
        } => {
            let corners: Vec<[f64; 2]> = if *top > 0.0 {
                vec![[0.0, 0.0], [*base, 0.0], [*top, *height], [0.0, *height]]
            } else {
                vec![[0.0, 0.0], [*base, 0.0], [0.0, *height]]
            };
            let profile = brep::polygon_profile(frame, &corners);
            brep::revolve(
                builder,
                &profile,
                brep::frame_axis(frame),
                std::f64::consts::TAU,
            )
        }
        Shape::Sphere { frame, radius } => brep::revolve(
            builder,
            &brep::half_disc_profile(frame, *radius),
            brep::frame_axis(frame),
            std::f64::consts::TAU,
        ),
        Shape::Torus {
            frame,
            major,
            minor,
        } => brep::revolve(
            builder,
            &brep::tube_profile(frame, *major, *minor),
            brep::frame_axis(frame),
            std::f64::consts::TAU,
        ),
        Shape::Ellipsoid { .. } => {
            Err("the IR has no ellipsoid surface to bound the solid".to_owned())
        }
        Shape::Extruded {
            profile,
            direction,
            depth,
            ..
        } => brep::extrude(
            builder,
            profile.as_ref().map_err(Clone::clone)?,
            *direction,
            *depth,
        ),
        Shape::Revolved {
            profile,
            axis,
            angle,
            ..
        } => brep::revolve(
            builder,
            profile.as_ref().map_err(Clone::clone)?,
            Axis {
                point: axis.origin,
                direction: axis.z,
            },
            *angle,
        ),
        Shape::Boolean { .. } => Err("a Boolean has no primitive boundary".to_owned()),
    }
}
