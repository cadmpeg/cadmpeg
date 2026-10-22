// SPDX-License-Identifier: Apache-2.0
//! Extrusion profile selections resolved through their profile wire bodies.

use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::TAU;

use cadmpeg_asm::brep::AsmBrep;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::features::{PlanarProfileRef, SketchProfileRegion};
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchEntityUse, SketchGeometryDefinition};

use super::{PmDcEntityStyleLink, PmDcKernelEntityLink};
use crate::sketch::PmDcSketchEntity;

/// ASM attribute chain that names a profile wire edge after its sketch curve.
const NAMING_ATTRIBUTE: &str =
    "NMx_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib";
/// Topology value of a naming attribute on an edge.
const NAMED_EDGE: i64 = 2;
/// Entity type of an entity-style link that names a sketch curve.
const CURVE_LINK_ENTITY_TYPE: u32 = 2;
/// Largest distance, in millimetres, between a profile wire vertex and the
/// sketch-curve end or plane it lies on: one kernel linear tolerance.
const EPS_PROFILE_WIRE_POINT_MM: f64 = 1.0e-5;

/// The sketch curve that a profile wire edge is generated from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EdgeName {
    naming_type: i64,
    associative_id: i64,
}

/// One edge of a profile wire, in model-space millimetres.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ProfileEdge {
    name: Option<EdgeName>,
    start: Point3,
    end: Point3,
}

/// One wire body of a profile-body record, keyed by its ASM body key.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ProfileWire {
    key: u64,
    edges: Vec<ProfileEdge>,
}

/// The wire bodies of one `PmDc` kernel-body record.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ProfileBody {
    segment_token: IdentityKey,
    record_ordinal: u32,
    wires: Vec<ProfileWire>,
}

/// Decode a `PmDc` kernel-body record and keep its keyed wire bodies.
pub(super) fn decode_profile_body(
    ctx: &DecodeContext<'_>,
    payload: View<'_>,
    segment_token: &IdentityKey,
    record_ordinal: u32,
    payload_offset: u64,
    version: u8,
) -> Result<ProfileBody, CodecError> {
    let carrier = crate::kernel::parse_carrier(
        payload,
        segment_token,
        record_ordinal,
        payload_offset,
        version,
    )?;
    let header = carrier.header.as_deref().map_err(CodecError::malformed)?;
    let decoded = crate::kernel::decode_kernel_carrier(ctx, &carrier, header)?;
    Ok(ProfileBody {
        segment_token: segment_token.clone(),
        record_ordinal,
        wires: profile_wires(&decoded.brep),
    })
}

fn profile_wires(brep: &AsmBrep) -> Vec<ProfileWire> {
    let points = brep
        .points
        .iter()
        .map(|point| (point.id.as_str(), *point.position().as_raw()))
        .collect::<HashMap<_, _>>();
    let vertices = brep
        .vertices
        .iter()
        .filter_map(|vertex| Some((vertex.id.as_str(), *points.get(vertex.point.as_str())?)))
        .collect::<HashMap<_, _>>();
    let mut names = HashMap::<&str, Vec<Option<EdgeName>>>::new();
    for attribute in &brep.attributes {
        if let (AttributeTarget::Edge(edge), NAMING_ATTRIBUTE) =
            (&attribute.target, attribute.name.as_str())
        {
            names
                .entry(edge.as_str())
                .or_default()
                .push(edge_name(&attribute.values));
        }
    }
    let region_bodies = brep
        .regions
        .iter()
        .map(|region| (region.id.as_str(), region.body.as_str()))
        .collect::<HashMap<_, _>>();
    let shell_bodies = brep
        .shells
        .iter()
        .filter_map(|shell| {
            Some((
                shell.id.as_str(),
                *region_bodies.get(shell.region.as_str())?,
            ))
        })
        .collect::<HashMap<_, _>>();
    let keys = brep
        .body_native_keys
        .iter()
        .filter_map(|key| Some((key.body.as_str(), key.asm_body_key?)))
        .collect::<HashMap<_, _>>();
    let edges = brep
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let mut wires = Vec::new();
    for wire in &brep.wire_topologies {
        let Some(key) = shell_bodies
            .get(wire.shell.as_str())
            .and_then(|body| keys.get(body))
        else {
            continue;
        };
        let mut wire_edges = Vec::new();
        for id in wire.members.edges() {
            let Some((start, end)) = edges.get(id.as_str()).and_then(|edge| {
                Some((
                    *vertices.get(edge.start.as_str())?,
                    *vertices.get(edge.end.as_str())?,
                ))
            }) else {
                wire_edges.clear();
                break;
            };
            let name = match names.get(id.as_str()).map(Vec::as_slice) {
                Some([name]) => *name,
                _ => None,
            };
            wire_edges.push(ProfileEdge { name, start, end });
        }
        wires.push(ProfileWire {
            key: *key,
            edges: wire_edges,
        });
    }
    wires
}

/// The naming-type and associative-id integers of an edge naming attribute.
fn edge_name(values: &[AttributeValue]) -> Option<EdgeName> {
    let [_, _, _, _, _, rest @ ..] = values else {
        return None;
    };
    let integers = rest
        .iter()
        .map(|value| match value {
            AttributeValue::Integer(value) => Some(*value),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    match integers.as_slice() {
        [naming_type, 0, associative_id, 0, NAMED_EDGE] => Some(EdgeName {
            naming_type: *naming_type,
            associative_id: *associative_id,
        }),
        _ => None,
    }
}

/// Joins that resolve profile selections in one document.
pub(super) struct ProfileIndex<'a> {
    bodies: HashMap<(&'a str, u32), Option<&'a ProfileBody>>,
    kernel_links: HashMap<(&'a str, u32), &'a PmDcKernelEntityLink>,
    /// Curve entity by segment, sketch ordinal, and associative id; `None`
    /// when the id names more than one curve.
    curves: HashMap<(&'a str, u32, i64), Option<String>>,
    entities: HashMap<&'a str, &'a SketchEntity>,
    by_id: HashMap<&'a str, &'a SketchEntity>,
    /// Loops that resolved selections append to sketch profile tables.
    added: RefCell<HashMap<String, Vec<Vec<SketchEntityUse>>>>,
}

impl<'a> ProfileIndex<'a> {
    pub(super) fn new(
        bodies: &'a [ProfileBody],
        kernel_links: &'a [PmDcKernelEntityLink],
        entity_style_links: &'a [PmDcEntityStyleLink],
        raw_entities: &'a [PmDcSketchEntity],
        entities: &'a [SketchEntity],
    ) -> Self {
        let mut body_index = HashMap::new();
        for body in bodies {
            body_index
                .entry((body.segment_token.as_str(), body.record_ordinal))
                .and_modify(|slot| *slot = None)
                .or_insert(Some(body));
        }
        let raw = raw_entities
            .iter()
            .map(|entity| {
                (
                    (
                        entity.identity.segment_token.as_str(),
                        entity.identity.record_ordinal,
                    ),
                    entity,
                )
            })
            .collect::<HashMap<_, _>>();
        let mut curves = HashMap::new();
        for link in entity_style_links {
            if link.entity_type != CURVE_LINK_ENTITY_TYPE {
                continue;
            }
            let token = link.identity.segment_token.as_str();
            let Some(entity) = link
                .header
                .owner
                .index()
                .checked_sub(1)
                .and_then(|ordinal| raw.get(&(token, ordinal)))
            else {
                continue;
            };
            let Some(sketch) = entity.sketch.index().checked_sub(1) else {
                continue;
            };
            curves
                .entry((token, sketch, i64::from(link.associative_id)))
                .and_modify(|slot| *slot = None)
                .or_insert_with(|| Some(entity.id()));
        }
        Self {
            bodies: body_index,
            kernel_links: kernel_links
                .iter()
                .map(|link| {
                    (
                        (
                            link.identity.segment_token.as_str(),
                            link.identity.record_ordinal,
                        ),
                        link,
                    )
                })
                .collect(),
            curves,
            entities: entities
                .iter()
                .filter_map(|entity| Some((entity.native_ref.as_deref()?, entity)))
                .collect(),
            by_id: entities
                .iter()
                .map(|entity| (entity.id().as_str(), entity))
                .collect(),
            added: RefCell::default(),
        }
    }

    /// The regions that the profile selections of one extrusion select.
    ///
    /// `links` holds the ordinal of each selection's entity link, in source
    /// selection order. A selection resolves only when its wire edges run the
    /// whole length of the curves of one closed loop of `sketch`. A loop that
    /// the sketch profile table lacks is appended to it.
    pub(super) fn regions(
        &self,
        token: &str,
        sketch_ordinal: u32,
        sketch: &Sketch,
        links: &[u32],
    ) -> Option<PlanarProfileRef> {
        let frame = SketchFrame::new(sketch)?;
        let mut table = sketch.profiles.iter().cloned().collect::<Vec<_>>();
        if let Some(added) = self.added.borrow().get(sketch.id.as_str()) {
            table.extend(added.iter().cloned());
        }
        let known = table.len();
        let mut loops = Vec::new();
        for ordinal in links {
            let link = self.kernel_links.get(&(token, *ordinal))?;
            let body = self
                .bodies
                .get(&(token, link.body.index().checked_sub(1)?))
                .copied()
                .flatten()?;
            let mut wires = body
                .wires
                .iter()
                .filter(|wire| wire.key == u64::from(link.entity_id));
            let (Some(wire), None) = (wires.next(), wires.next()) else {
                return None;
            };
            let curves = self.wire_curves(token, sketch_ordinal, sketch, &frame, link, wire)?;
            let index = match table.iter().position(|profile| {
                profile.len() == curves.len()
                    && profile
                        .iter()
                        .all(|use_| curves.iter().any(|curve| curve.id() == &use_.entity))
            }) {
                Some(index) => index,
                None => {
                    table.push(chain(&curves)?);
                    table.len() - 1
                }
            };
            if loops.contains(&index) {
                return None;
            }
            loops.push(index);
        }
        let regions = self.nest(&table, &loops)?;
        let profile = PlanarProfileRef::sketch_regions(sketch.id.clone(), regions).ok()?;
        if table.len() > known {
            self.added
                .borrow_mut()
                .entry(sketch.id.as_str().to_owned())
                .or_default()
                .extend(table.drain(known..));
        }
        Some(profile)
    }

    /// Loops appended to sketch profile tables, by sketch id, in table order.
    pub(super) fn into_added_profiles(self) -> HashMap<String, Vec<Vec<SketchEntityUse>>> {
        self.added.into_inner()
    }

    /// The distinct sketch curves a wire runs along, in wire order.
    fn wire_curves(
        &self,
        token: &str,
        sketch_ordinal: u32,
        sketch: &Sketch,
        frame: &SketchFrame,
        link: &PmDcKernelEntityLink,
        wire: &ProfileWire,
    ) -> Option<Vec<&'a SketchEntity>> {
        let mut curves = Vec::new();
        for edge in &wire.edges {
            let name = edge.name?;
            if name.naming_type != i64::from(link.entity_type) {
                return None;
            }
            let native = self
                .curves
                .get(&(token, sketch_ordinal, name.associative_id))?
                .as_deref()?;
            let entity = *self.entities.get(native)?;
            if entity.sketch != sketch.id
                || entity.construction
                || !spans_curve(frame, entity, edge)
            {
                return None;
            }
            if curves
                .iter()
                .any(|curve: &&SketchEntity| curve.id() == entity.id())
            {
                // Only a full circle runs through more than one edge.
                if !is_full_circle(entity) {
                    return None;
                }
                continue;
            }
            curves.push(entity);
        }
        // A full circle bounds a loop by itself.
        if curves.len() > 1 && curves.iter().any(|curve| is_full_circle(curve)) {
            return None;
        }
        (!curves.is_empty()).then_some(curves)
    }

    /// Selected loops nested by containment: a loop inside an odd number of
    /// other selected loops is a hole of the innermost loop that contains it.
    fn nest(
        &self,
        table: &[Vec<SketchEntityUse>],
        loops: &[usize],
    ) -> Option<Vec<SketchProfileRegion>> {
        let geometry = loops
            .iter()
            .map(|index| self.loop_curves(table.get(*index)?))
            .collect::<Option<Vec<_>>>()?;
        let mut contains = vec![vec![false; loops.len()]; loops.len()];
        for (inner, inner_curves) in geometry.iter().enumerate() {
            let point = inner_curves.first()?.point_on();
            for (outer, outer_curves) in geometry.iter().enumerate() {
                contains[outer][inner] = outer != inner && encloses(outer_curves, point)?;
            }
        }
        let depth = (0..loops.len())
            .map(|inner| contains.iter().filter(|row| row[inner]).count())
            .collect::<Vec<_>>();
        let mut regions = Vec::new();
        for (outer, outer_depth) in depth.iter().enumerate() {
            if outer_depth % 2 != 0 {
                continue;
            }
            let holes = (0..loops.len())
                .filter(|hole| depth[*hole] == outer_depth + 1 && contains[outer][*hole])
                .map(|hole| u32::try_from(loops[hole]).ok())
                .collect::<Option<Vec<_>>>()?;
            regions
                .push(SketchProfileRegion::loops(u32::try_from(loops[outer]).ok()?, holes).ok()?);
        }
        Some(regions)
    }

    fn loop_curves(&self, profile: &[SketchEntityUse]) -> Option<Vec<LoopCurve>> {
        profile
            .iter()
            .map(|use_| {
                let entity = self.by_id.get(use_.entity.as_str())?;
                LoopCurve::new(entity, use_.reversed)
            })
            .collect()
    }
}

fn is_full_circle(entity: &SketchEntity) -> bool {
    matches!(
        entity.geometry.definition(),
        SketchGeometryDefinition::Circle { .. }
    )
}

/// The curves of a closed loop as an oriented chain that starts along the
/// first curve.
fn chain(curves: &[&SketchEntity]) -> Option<Vec<SketchEntityUse>> {
    let use_ = |entity: &SketchEntity, reversed| SketchEntityUse {
        entity: entity.id().clone(),
        reversed,
    };
    let (first, rest) = curves.split_first()?;
    let start = LoopCurve::new(first, false)?;
    if rest.is_empty() {
        return matches!(start, LoopCurve::Circle { .. }).then(|| vec![use_(first, false)]);
    }
    let (origin, mut at) = start.ends();
    let mut uses = vec![use_(first, false)];
    let mut remaining = rest
        .iter()
        .map(|entity| Some((*entity, LoopCurve::new(entity, false)?.ends())))
        .collect::<Option<Vec<_>>>()?;
    while !remaining.is_empty() {
        let next = remaining
            .iter()
            .enumerate()
            .filter_map(|(index, (_, ends))| {
                if near(ends.0, at) {
                    Some((index, false))
                } else if near(ends.1, at) {
                    Some((index, true))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let [(index, reversed)] = next[..] else {
            return None;
        };
        let (entity, ends) = remaining.swap_remove(index);
        at = if reversed { ends.0 } else { ends.1 };
        uses.push(use_(entity, reversed));
    }
    near(at, origin).then_some(uses)
}

fn near(left: Point2, right: Point2) -> bool {
    distance(left, right) <= EPS_PROFILE_WIRE_POINT_MM
}

/// The model-space frame of a resolved sketch placement.
struct SketchFrame {
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
    v_axis: Vector3,
}

impl SketchFrame {
    fn new(sketch: &Sketch) -> Option<Self> {
        let (origin, normal, u_axis) = sketch.resolved_placement()?;
        let normal = *normal.as_raw();
        let u_axis = *u_axis.as_raw();
        Some(Self {
            origin: *origin.as_raw(),
            normal,
            u_axis,
            v_axis: normal.cross(u_axis),
        })
    }

    /// Sketch coordinates of a model point that lies in the sketch plane.
    fn uv(&self, point: Point3) -> Option<Point2> {
        let offset = Vector3::new(
            point.x - self.origin.x,
            point.y - self.origin.y,
            point.z - self.origin.z,
        );
        (offset.dot(self.normal).abs() <= EPS_PROFILE_WIRE_POINT_MM)
            .then(|| Point2::new(offset.dot(self.u_axis), offset.dot(self.v_axis)))
    }
}

/// Whether a wire edge runs the full length of a sketch curve.
fn spans_curve(frame: &SketchFrame, entity: &SketchEntity, edge: &ProfileEdge) -> bool {
    let (Some(start), Some(end)) = (frame.uv(edge.start), frame.uv(edge.end)) else {
        return false;
    };
    let same = near;
    let ends = |first: Point2, second: Point2| {
        (same(start, first) && same(end, second)) || (same(start, second) && same(end, first))
    };
    match LoopCurve::new(entity, false) {
        Some(LoopCurve::Line { start: a, end: b }) => ends(a, b),
        Some(arc @ LoopCurve::Arc { .. }) => {
            let (a, b) = arc.ends();
            ends(a, b)
        }
        Some(LoopCurve::Circle { center, radius }) => [start, end]
            .iter()
            .all(|point| (distance(*point, center) - radius).abs() <= EPS_PROFILE_WIRE_POINT_MM),
        None => false,
    }
}

/// One oriented curve of a profile loop in sketch millimetres.
#[derive(Debug, Clone, Copy)]
enum LoopCurve {
    Line {
        start: Point2,
        end: Point2,
    },
    /// Counterclockwise from `start_angle` through `sweep`, negated when the
    /// loop runs the arc backwards.
    Arc {
        center: Point2,
        radius: f64,
        start_angle: f64,
        sweep: f64,
    },
    Circle {
        center: Point2,
        radius: f64,
    },
}

impl LoopCurve {
    fn new(entity: &SketchEntity, reversed: bool) -> Option<Self> {
        Some(match *entity.geometry.definition() {
            SketchGeometryDefinition::Line { start, end } => {
                let (start, end) = (start.get(), end.get());
                if reversed {
                    Self::Line {
                        start: end,
                        end: start,
                    }
                } else {
                    Self::Line { start, end }
                }
            }
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let sweep = end_angle.get() - start_angle.get();
                if reversed {
                    Self::Arc {
                        center: center.get(),
                        radius: radius.get(),
                        start_angle: end_angle.get(),
                        sweep: -sweep,
                    }
                } else {
                    Self::Arc {
                        center: center.get(),
                        radius: radius.get(),
                        start_angle: start_angle.get(),
                        sweep,
                    }
                }
            }
            SketchGeometryDefinition::Circle { center, radius } => Self::Circle {
                center: center.get(),
                radius: radius.get(),
            },
            _ => return None,
        })
    }

    fn ends(self) -> (Point2, Point2) {
        match self {
            Self::Line { start, end } => (start, end),
            Self::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } => (
                polar(center, radius, start_angle),
                polar(center, radius, start_angle + sweep),
            ),
            Self::Circle { center, radius } => {
                let point = polar(center, radius, 0.0);
                (point, point)
            }
        }
    }

    /// A point on the curve.
    fn point_on(self) -> Point2 {
        self.ends().0
    }

    /// The angle this curve turns about `point`, which lies off the curve.
    fn turn(self, point: Point2) -> f64 {
        match self {
            Self::Circle { center, radius } => {
                if distance(point, center) < radius {
                    TAU
                } else {
                    0.0
                }
            }
            Self::Arc {
                center,
                radius,
                sweep,
                ..
            } if distance(point, center) < radius => sweep,
            // Outside its circle, an arc turns as its chord does: the region
            // between them lies inside the circle.
            Self::Line { .. } | Self::Arc { .. } => {
                let (start, end) = self.ends();
                let (su, sv) = (start.u - point.u, start.v - point.v);
                let (eu, ev) = (end.u - point.u, end.v - point.v);
                (su * ev - sv * eu).atan2(su * eu + sv * ev)
            }
        }
    }
}

fn polar(center: Point2, radius: f64, angle: f64) -> Point2 {
    Point2::new(
        radius.mul_add(angle.cos(), center.u),
        radius.mul_add(angle.sin(), center.v),
    )
}

fn distance(left: Point2, right: Point2) -> f64 {
    (left.u - right.u).hypot(left.v - right.v)
}

/// Whether a closed loop winds around a point off the loop.
fn encloses(curves: &[LoopCurve], point: Point2) -> Option<bool> {
    let turns = curves.iter().map(|curve| curve.turn(point)).sum::<f64>() / TAU;
    // A closed loop turns a whole number of times; the rounding absorbs only
    // floating-point error.
    turns.is_finite().then(|| turns.round() != 0.0)
}

#[cfg(test)]
mod tests;
