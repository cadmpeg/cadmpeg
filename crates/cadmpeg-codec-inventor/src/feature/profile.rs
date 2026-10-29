// SPDX-License-Identifier: Apache-2.0
//! Extrusion profile selections resolved through stored boundary loops.

use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::TAU;

use cadmpeg_asm::brep::AsmBrep;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::features::{PlanarProfileRef, SketchProfileBoundaryUse, SketchProfileRegion};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{DirectedParameterRange, SolvedCurveGeometry};
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchEntityUse, SketchGeometryDefinition};
use cadmpeg_ir::topology::EdgeCarrier;

use super::{PmDcEntityStyleLink, PmDcKernelEntityLink};

mod membership;
mod selection;
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};
use crate::sketch::{PmDcSketchEntity, PmDcSketchEntityKind};

/// ASM attribute chain that names a profile wire edge after its sketch curve.
const NAMING_ATTRIBUTE: &str =
    "NMx_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib";
/// Topology value of a naming attribute on an edge.
const NAMED_EDGE: i64 = 2;
const CUSTOM_NAMING_ATTRIBUTE: &str = "ATTRIB_CUSTOM-attrib";
const CUSTOM_NAMING_FAMILY: &str = "INV_NMX_GEN_TAG";
/// Entity type of an entity-style link that names a sketch curve.
const CURVE_LINK_ENTITY_TYPE: u32 = 2;
/// Entity type of an entity-style link that names a sketch block instance.
const BLOCK_INSTANCE_LINK_ENTITY_TYPE: u32 = 0x0800_0000;
/// Offset from a block instance's associative id to the first id of the
/// geometry it places.
const BLOCK_CURVE_FIRST_ID_OFFSET: i64 = 2;
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
    /// The carrier point at the middle of the edge's parameter interval.
    middle: Option<Point3>,
    spline: Option<Box<(NurbsCurve, [f64; 2])>>,
}

/// One profile boundary loop, keyed by its ASM body key.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ProfileWire {
    key: u64,
    edges: Vec<ProfileEdge>,
}

/// The boundary loops of one `PmDc` kernel-body record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProfileBody {
    segment_token: IdentityKey,
    record_ordinal: u32,
    wires: Vec<ProfileWire>,
}

/// Decode a `PmDc` kernel-body record and keep its keyed wire bodies.
/// The profile bodies of a document's `PmDc` kernel-body records.
pub(crate) struct ProfileBodies {
    pub(crate) bodies: Vec<ProfileBody>,
    /// Kernel-body records that do not decode. Profile selections that link
    /// to them keep their native selection.
    pub(crate) undecoded: usize,
}

/// Decode the profile bodies that extrusion profiles resolve through. Only
/// design projection uses them, so a record that does not decode is counted
/// and skipped; a session resource refusal still ends the decode.
pub(crate) fn decode_profile_bodies(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
) -> Result<ProfileBodies, CodecError> {
    let mut decoded = ProfileBodies {
        bodies: Vec::new(),
        undecoded: 0,
    };
    for segment in &document.segments {
        if segment.kind != SegmentKind::PmDc {
            continue;
        }
        let Some(version) = segment.registry.map(|join| join.version_major) else {
            continue;
        };
        if !crate::pmdc::record_version_supported(version) {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        for record in table
            .records
            .iter()
            .filter(|record| record.type_id == crate::kernel::KERNEL_RECORD_TYPE_ID)
        {
            match decode_profile_body(
                ctx,
                record.payload,
                segment.pair.token.key(),
                record.ordinal,
                record.payload_offset,
                version,
            ) {
                Ok(body) => {
                    ctx.charge_entities(1, "admit Inventor PmDc profile body")?;
                    decoded.bodies.push(body);
                }
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(_) => decoded.undecoded += 1,
            }
        }
    }
    Ok(decoded)
}

fn decode_profile_body(
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
        wires: profile_wires(ctx, &decoded.brep)?,
    })
}

fn profile_wires(ctx: &DecodeContext<'_>, brep: &AsmBrep) -> Result<Vec<ProfileWire>, CodecError> {
    let curves = brep
        .curves
        .iter()
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect::<HashMap<_, _>>();
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
        if let AttributeTarget::Edge(edge) = &attribute.target {
            let naming = attribute.name == NAMING_ATTRIBUTE
                || (attribute.name == CUSTOM_NAMING_ATTRIBUTE
                    && matches!(attribute.values.get(5), Some(AttributeValue::String(family))
                        if family == CUSTOM_NAMING_FAMILY));
            if !naming {
                continue;
            }
            names
                .entry(edge.as_str())
                .or_default()
                .push(edge_name(&attribute.name, &attribute.values));
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
    let mut rings = Vec::new();
    for wire in &brep.wire_topologies {
        let Some(key) = shell_bodies
            .get(wire.shell.as_str())
            .and_then(|body| keys.get(body))
        else {
            continue;
        };
        rings.push((*key, wire.members.edges().iter().collect::<Vec<_>>()));
    }
    let faces = brep
        .faces
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<HashMap<_, _>>();
    let coedges = brep
        .coedges
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<HashMap<_, _>>();
    for loop_ in &brep.loops {
        let Some(key) = faces
            .get(loop_.face.as_str())
            .and_then(|face| shell_bodies.get(face.shell.as_str()))
            .and_then(|body| keys.get(body))
        else {
            continue;
        };
        let members = loop_
            .coedges()
            .iter()
            .map(|id| coedges.get(id.as_str()).map(|coedge| &coedge.edge))
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default();
        rings.push((*key, members));
    }
    let mut wires = Vec::new();
    for (key, members) in rings {
        let mut wire_edges = Vec::new();
        for id in members {
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
            let middle = match edges.get(id.as_str()).map(|edge| &edge.carrier) {
                Some(EdgeCarrier::Bounded(curve, interval)) => match curves.get(curve.as_str()) {
                    Some(geometry) => {
                        let [first, last] = interval.endpoints();
                        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::curve_point(
                            ctx,
                            geometry,
                            first.midpoint(last),
                        ))?
                        .map(|point| *point.as_raw())
                    }
                    None => None,
                },
                _ => None,
            };
            let spline = edges.get(id.as_str()).and_then(|edge| {
                let EdgeCarrier::Bounded(curve, interval) = &edge.carrier else {
                    return None;
                };
                let SolvedCurveGeometry::Nurbs(nurbs) = curves.get(curve.as_str())?.solved()?
                else {
                    return None;
                };
                Some(Box::new((nurbs.clone(), interval.endpoints())))
            });
            wire_edges.push(ProfileEdge {
                name,
                start,
                end,
                middle,
                spline,
            });
        }
        wires.push(ProfileWire {
            key,
            edges: wire_edges,
        });
    }
    Ok(wires)
}

/// The naming-type and associative-id integers of an edge naming attribute.
fn edge_name(attribute: &str, values: &[AttributeValue]) -> Option<EdgeName> {
    let [_, _, _, _, _, rest @ ..] = values else {
        return None;
    };
    let rest = match (attribute, rest) {
        (NAMING_ATTRIBUTE, rest) => rest,
        (
            CUSTOM_NAMING_ATTRIBUTE,
            [AttributeValue::String(family), AttributeValue::Integer(1), rest @ ..],
        ) if family == CUSTOM_NAMING_FAMILY => rest,
        _ => return None,
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
#[derive(Default)]
pub(super) struct ProfileIndex<'a> {
    bodies: HashMap<(&'a str, u32), Option<&'a ProfileBody>>,
    kernel_links: HashMap<(&'a str, u32), &'a PmDcKernelEntityLink>,
    /// Neutral curve entity id by segment, sketch ordinal, and associative
    /// id; `None` when the id names more than one curve.
    curves: HashMap<(&'a str, u32, i64), Option<String>>,
    by_id: HashMap<&'a str, &'a SketchEntity>,
    incomplete_sketches: std::collections::HashSet<(&'a str, u32)>,
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
        let mut name = |key, id: Option<String>| {
            if let Some(id) = id {
                curves
                    .entry(key)
                    .and_modify(|slot| *slot = None)
                    .or_insert(Some(id));
            }
        };
        for link in entity_style_links {
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
            let associative_id = i64::from(link.associative_id);
            match (link.entity_type, &entity.kind) {
                (CURVE_LINK_ENTITY_TYPE, _) => name(
                    (token, sketch, associative_id),
                    crate::sketch::entity_id(entity).map(|id| id.as_str().to_owned()),
                ),
                (BLOCK_INSTANCE_LINK_ENTITY_TYPE, PmDcSketchEntityKind::BlockInstance { .. }) => {
                    // A block instance names the curves it places with ids
                    // of its own, which no entity-style link stores.
                    let definition = crate::sketch::block_definition_curves(entity, raw_entities);
                    let Some(offsets) = crate::sketch::block_curve_offsets(&definition) else {
                        continue;
                    };
                    for (curve, offset) in definition.into_iter().zip(offsets) {
                        name(
                            (
                                token,
                                sketch,
                                associative_id + BLOCK_CURVE_FIRST_ID_OFFSET + i64::from(offset),
                            ),
                            crate::sketch::block_curve_id(entity, curve)
                                .map(|id| id.as_str().to_owned()),
                        );
                    }
                }
                _ => {}
            }
        }
        let neutral_ids = entities
            .iter()
            .map(SketchEntity::id)
            .collect::<std::collections::HashSet<_>>();
        let incomplete_sketches = raw_entities
            .iter()
            .filter(|entity| {
                matches!(
                    entity.kind,
                    PmDcSketchEntityKind::Line { .. }
                        | PmDcSketchEntityKind::Circle { .. }
                        | PmDcSketchEntityKind::Ellipse { .. }
                        | PmDcSketchEntityKind::Spline { .. }
                )
            })
            .filter(|entity| {
                crate::sketch::entity_id(entity).is_none_or(|id| !neutral_ids.contains(&id))
            })
            .filter_map(|entity| {
                Some((
                    entity.identity.segment_token.as_str(),
                    entity.sketch.record_ordinal()?,
                ))
            })
            .collect();
        Self {
            bodies: body_index,
            incomplete_sketches,
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
    /// selection order. Each selection resolves to one closed ring of pieces
    /// of curves of `sketch`. When every ring runs along whole curves, the
    /// regions use loops of the sketch profile table, and a loop that the
    /// table lacks is appended to it. Otherwise the regions state their
    /// trimmed boundary rings.
    pub(super) fn regions(
        &self,
        token: &str,
        sketch_ordinal: u32,
        sketch: &Sketch,
        links: &[u32],
    ) -> Option<PlanarProfileRef> {
        let frame = SketchFrame::new(sketch)?;
        if let Some(profile) =
            self.closed_spline_region(token, sketch_ordinal, sketch, &frame, links)
        {
            return Some(profile);
        }
        let mut rings = Vec::new();
        for ordinal in links {
            let link = self.kernel_links.get(&(token, *ordinal))?;
            let body = self
                .bodies
                .get(&(token, link.body.index().checked_sub(1)?))
                .copied()
                .flatten()?;
            // A selection can run through several wire loops of one body,
            // such as the outer boundary and the holes of a plate face.
            let known = rings.len();
            for wire in body
                .wires
                .iter()
                .filter(|wire| wire.key == u64::from(link.entity_id))
            {
                rings.push(self.ring(token, sketch_ordinal, sketch, &frame, link, wire)?);
            }
            if rings.len() == known {
                return None;
            }
        }
        if rings
            .iter()
            .all(|ring| ring.iter().all(|piece| piece.whole))
        {
            self.loop_regions(sketch, &rings)
        } else {
            trimmed_regions(sketch, &rings)
        }
    }

    fn closed_spline_region(
        &self,
        token: &str,
        sketch_ordinal: u32,
        sketch: &Sketch,
        frame: &SketchFrame,
        links: &[u32],
    ) -> Option<PlanarProfileRef> {
        let [ordinal] = links else {
            return None;
        };
        let link = self.kernel_links.get(&(token, *ordinal))?;
        let body = self
            .bodies
            .get(&(token, link.body.record_ordinal()?))?
            .as_ref()?;
        let mut wires = body
            .wires
            .iter()
            .filter(|wire| wire.key == u64::from(link.entity_id));
        let wire = wires.next()?;
        if wires.next().is_some() {
            return None;
        }
        let [edge] = wire.edges.as_slice() else {
            return None;
        };
        let name = edge.name?;
        if name.naming_type != i64::from(link.entity_type) {
            return None;
        }
        let id = self
            .curves
            .get(&(token, sketch_ordinal, name.associative_id))?
            .as_deref()?;
        let entity = self.by_id.get(id)?;
        if entity.sketch != sketch.id || entity.construction {
            return None;
        }
        let SketchGeometryDefinition::Nurbs { curve } = entity.geometry.definition() else {
            return None;
        };
        let (carrier, interval) = edge.spline.as_deref()?;
        if !closed_spline_matches(frame, curve, carrier, *interval) {
            return None;
        }
        let seam = curve.control_points().first()?.get();
        if !near(frame.uv(edge.start)?, seam) || !near(frame.uv(edge.end)?, seam) {
            return None;
        }
        let profile = sketch.profiles.iter().position(
            |profile| matches!(profile.as_slice(), [use_] if use_.entity == *entity.id()),
        )?;
        PlanarProfileRef::sketch_regions(
            sketch.id.clone(),
            vec![SketchProfileRegion::loops(u32::try_from(profile).ok()?, Vec::new()).ok()?],
        )
        .ok()
    }

    /// Whole-curve rings as regions over loops of the sketch profile table.
    fn loop_regions(&self, sketch: &Sketch, rings: &[Vec<Piece<'a>>]) -> Option<PlanarProfileRef> {
        let mut table = sketch.profiles.iter().cloned().collect::<Vec<_>>();
        if let Some(added) = self.added.borrow().get(sketch.id.as_str()) {
            table.extend(added.iter().cloned());
        }
        let known = table.len();
        let mut loops = Vec::new();
        for ring in rings {
            let mut curves = Vec::<&SketchEntity>::new();
            for piece in ring {
                if !curves.iter().any(|curve| curve.id() == piece.entity.id()) {
                    curves.push(piece.entity);
                }
            }
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
        let geometry = loops
            .iter()
            .map(|index| self.loop_curves(table.get(*index)?))
            .collect::<Option<Vec<_>>>()?;
        let mut regions = Vec::new();
        for (outer, holes) in nest(&geometry)? {
            let holes = holes
                .into_iter()
                .map(|hole| u32::try_from(loops[hole]).ok())
                .collect::<Option<Vec<_>>>()?;
            regions
                .push(SketchProfileRegion::loops(u32::try_from(loops[outer]).ok()?, holes).ok()?);
        }
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

    /// The pieces of sketch curves a wire runs along, as one closed ring in
    /// wire order.
    fn ring(
        &self,
        token: &str,
        sketch_ordinal: u32,
        sketch: &Sketch,
        frame: &SketchFrame,
        link: &PmDcKernelEntityLink,
        wire: &ProfileWire,
    ) -> Option<Vec<Piece<'a>>> {
        let mut named = Vec::new();
        for edge in &wire.edges {
            let name = edge.name?;
            if name.naming_type != i64::from(link.entity_type) {
                return None;
            }
            let id = self
                .curves
                .get(&(token, sketch_ordinal, name.associative_id))?
                .as_deref()?;
            let entity = *self.by_id.get(id)?;
            if entity.sketch != sketch.id || entity.construction {
                return None;
            }
            named.push((entity, edge));
        }
        let &(first, _) = named.first()?;
        // A full circle bounds a ring by itself; it can run through more
        // than one edge, and only the edge ends need to lie on it.
        if let Some(curve @ LoopCurve::Circle { center, radius }) = LoopCurve::new(first, false) {
            if named.iter().all(|(entity, _)| entity.id() == first.id()) {
                for (_, edge) in &named {
                    on_circle(frame.uv(edge.start)?, center, radius)?;
                    on_circle(frame.uv(edge.end)?, center, radius)?;
                }
                let start = polar_point(curve, 0.0);
                return Some(vec![Piece {
                    entity: first,
                    curve,
                    range: [0.0, TAU],
                    whole: true,
                    ends: [start; 2],
                    reversed: false,
                }]);
            }
        }
        let mut pieces = named
            .into_iter()
            .map(|(entity, edge)| Piece::new(frame, entity, edge))
            .collect::<Option<Vec<_>>>()?;
        orient(&mut pieces)?;
        Some(pieces)
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

fn closed_spline_matches(
    frame: &SketchFrame,
    curve: &cadmpeg_ir::geometry::pcurve::PcurveNurbs,
    carrier: &NurbsCurve,
    interval: [f64; 2],
) -> bool {
    let points = curve.control_points();
    let poles = carrier.control_points();
    let knots = curve.knots().as_slice();
    let Ok(degree) = usize::try_from(curve.degree()) else {
        return false;
    };
    let Some(last) = degree
        .checked_add(1)
        .and_then(|order| knots.len().checked_sub(order))
    else {
        return false;
    };
    let domain = knots
        .get(degree)
        .zip(knots.get(last))
        .map(|(first, last)| [*first, *last]);
    curve.periodic()
        && carrier.periodic()
        && curve.degree() == carrier.degree()
        && knots == carrier.knots().as_slice()
        && curve.weights().is_none()
        && carrier.weights().is_none()
        && domain == Some(interval)
        && points.len() == poles.len()
        && points
            .first()
            .is_some_and(|first| Some(first) == points.last())
        && points.iter().zip(poles).all(|(point, pole)| {
            frame
                .uv(*pole.as_raw())
                .is_some_and(|uv| near(point.get(), uv))
        })
}

/// Rings that use parts of curves as regions with trimmed boundary rings.
fn trimmed_regions(sketch: &Sketch, rings: &[Vec<Piece<'_>>]) -> Option<PlanarProfileRef> {
    let geometry = rings
        .iter()
        .map(|ring| ring.iter().map(Piece::loop_curve).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let boundary = |ring: &[Piece<'_>]| {
        ring.iter()
            .map(Piece::boundary_use)
            .collect::<Option<Vec<_>>>()
    };
    let mut regions = Vec::new();
    for (outer, holes) in nest(&geometry)? {
        let holes = holes
            .into_iter()
            .map(|hole| boundary(&rings[hole]))
            .collect::<Option<Vec<_>>>()?;
        regions.push(SketchProfileRegion::trimmed(boundary(&rings[outer])?, holes).ok()?);
    }
    PlanarProfileRef::sketch_regions(sketch.id.clone(), regions).ok()
}

/// Selected rings nested by containment: a ring inside an odd number of
/// other selected rings is a hole of the innermost ring that contains it.
/// Each result pairs an outer ring with its holes, in source order.
fn nest(rings: &[Vec<LoopCurve>]) -> Option<Vec<(usize, Vec<usize>)>> {
    let mut contains = vec![vec![false; rings.len()]; rings.len()];
    for (inner, inner_curves) in rings.iter().enumerate() {
        for (outer, outer_curves) in rings.iter().enumerate() {
            if outer == inner {
                continue;
            }
            // A ring that shares a boundary with another is tested at a
            // point of its own boundary off the other ring.
            let point = inner_curves
                .iter()
                .map(|curve| curve.middle())
                .find(|point| !outer_curves.iter().any(|curve| curve.touches(*point)))?;
            contains[outer][inner] = encloses(outer_curves, point)?;
        }
    }
    let depth = (0..rings.len())
        .map(|inner| contains.iter().filter(|row| row[inner]).count())
        .collect::<Vec<_>>();
    let mut regions = Vec::new();
    for (outer, outer_depth) in depth.iter().enumerate() {
        if outer_depth % 2 != 0 {
            continue;
        }
        let holes = (0..rings.len())
            .filter(|hole| depth[*hole] == outer_depth + 1 && contains[outer][*hole])
            .collect();
        regions.push((outer, holes));
    }
    Some(regions)
}

/// The part of one sketch curve that one wire edge runs along.
#[derive(Debug, Clone, Copy)]
struct Piece<'a> {
    entity: &'a SketchEntity,
    /// The whole curve in its stored direction.
    curve: LoopCurve,
    /// Ascending parameters of the piece on the curve: a normalized line
    /// parameter or an angle.
    range: [f64; 2],
    /// Whether the piece is the whole curve.
    whole: bool,
    /// The piece ends in traversal order.
    ends: [Point2; 2],
    /// Whether traversal runs from the upper parameter to the lower one.
    reversed: bool,
}

impl<'a> Piece<'a> {
    fn new(frame: &SketchFrame, entity: &'a SketchEntity, edge: &ProfileEdge) -> Option<Self> {
        let start = frame.uv(edge.start)?;
        let end = frame.uv(edge.end)?;
        let curve = LoopCurve::new(entity, false)?;
        let (range, whole) = match curve {
            LoopCurve::Line {
                start: first,
                end: last,
            } => {
                let length = distance(first, last);
                let tolerance = EPS_PROFILE_WIRE_POINT_MM / length;
                let parameter = |point: Point2| {
                    let along = (point.u - first.u)
                        .mul_add(last.u - first.u, (point.v - first.v) * (last.v - first.v))
                        / (length * length);
                    let off = ((point.u - first.u) * (last.v - first.v)
                        - (point.v - first.v) * (last.u - first.u))
                        .abs()
                        / length;
                    (off <= EPS_PROFILE_WIRE_POINT_MM
                        && (-tolerance..=1.0 + tolerance).contains(&along))
                    .then(|| along.clamp(0.0, 1.0))
                };
                let range = ascending(parameter(start)?, parameter(end)?, tolerance)?;
                let whole = range[0] <= tolerance && range[1] >= 1.0 - tolerance;
                (if whole { [0.0, 1.0] } else { range }, whole)
            }
            LoopCurve::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } => {
                let tolerance = EPS_PROFILE_WIRE_POINT_MM / radius;
                let angle = |point: Point2| {
                    on_circle(point, center, radius)?;
                    let offset = ((point.v - center.v).atan2(point.u - center.u) - start_angle)
                        .rem_euclid(TAU);
                    // An end within the tolerance below the start angle
                    // wraps to just under a full turn.
                    let offset = if offset > TAU - tolerance {
                        0.0
                    } else {
                        offset
                    };
                    (offset <= sweep + tolerance).then(|| start_angle + offset.min(sweep))
                };
                let range = ascending(angle(start)?, angle(end)?, tolerance)?;
                let whole = range[0] <= start_angle + tolerance
                    && range[1] >= start_angle + sweep - tolerance;
                (
                    if whole {
                        [start_angle, start_angle + sweep]
                    } else {
                        range
                    },
                    whole,
                )
            }
            LoopCurve::Circle { center, radius } => {
                on_circle(start, center, radius)?;
                on_circle(end, center, radius)?;
                let tolerance = EPS_PROFILE_WIRE_POINT_MM / radius;
                let at = |point: Point2| (point.v - center.v).atan2(point.u - center.u);
                if distance(start, end) <= EPS_PROFILE_WIRE_POINT_MM {
                    ([0.0, TAU], true)
                } else {
                    // Two ends split a circle into two arcs; the edge middle
                    // lies on the one the edge runs along.
                    let middle = frame.uv(edge.middle?)?;
                    on_circle(middle, center, radius)?;
                    let (first, last) = (at(start), at(end));
                    let ccw = |from: f64, to: f64| (to - from).rem_euclid(TAU);
                    let from = if ccw(first, at(middle)) < ccw(first, last) {
                        first
                    } else {
                        last
                    };
                    let to = if from == first { last } else { first };
                    let from = from.rem_euclid(TAU);
                    let span = ccw(from, to);
                    (span > tolerance).then_some(())?;
                    ([from, from + span], false)
                }
            }
        };
        Some(Self {
            entity,
            curve,
            range,
            whole,
            ends: [start, end],
            reversed: false,
        })
    }

    /// The piece as an oriented loop curve.
    fn loop_curve(&self) -> LoopCurve {
        match self.curve {
            LoopCurve::Line { .. } => LoopCurve::Line {
                start: self.ends[0],
                end: self.ends[1],
            },
            LoopCurve::Arc { center, radius, .. } | LoopCurve::Circle { center, radius }
                if !(self.whole && matches!(self.curve, LoopCurve::Circle { .. })) =>
            {
                let [low, high] = self.range;
                if self.reversed {
                    LoopCurve::Arc {
                        center,
                        radius,
                        start_angle: high,
                        sweep: low - high,
                    }
                } else {
                    LoopCurve::Arc {
                        center,
                        radius,
                        start_angle: low,
                        sweep: high - low,
                    }
                }
            }
            whole => whole,
        }
    }

    fn boundary_use(&self) -> Option<SketchProfileBoundaryUse> {
        Some(SketchProfileBoundaryUse {
            entity: self.entity.id().clone(),
            parameter_range: DirectedParameterRange::new(self.range).ok()?,
            reversed: self.reversed,
        })
    }

    /// The point of this piece's curve at a parameter.
    fn point_at(&self, parameter: f64) -> Point2 {
        match self.curve {
            LoopCurve::Line { start, end } => Point2::new(
                parameter.mul_add(end.u - start.u, start.u),
                parameter.mul_add(end.v - start.v, start.v),
            ),
            LoopCurve::Arc { .. } | LoopCurve::Circle { .. } => polar_point(self.curve, parameter),
        }
    }
}

/// Orient a ring's pieces so that each one ends where the next one starts
/// and the last ends where the first starts.
fn orient(pieces: &mut [Piece<'_>]) -> Option<()> {
    let count = pieces.len();
    if count < 2 {
        return None;
    }
    let touches = |point: Point2, piece: &Piece<'_>| piece.ends.iter().any(|end| near(point, *end));
    if !touches(pieces[0].ends[1], &pieces[1]) {
        pieces[0].ends.swap(0, 1);
    }
    for index in 1..count {
        let at = pieces[index - 1].ends[1];
        let piece = &mut pieces[index];
        if near(piece.ends[1], at) && !near(piece.ends[0], at) {
            piece.ends.swap(0, 1);
        }
        near(piece.ends[0], at).then_some(())?;
    }
    near(pieces[count - 1].ends[1], pieces[0].ends[0]).then_some(())?;
    for piece in pieces.iter_mut() {
        let [low, high] = piece.range;
        // A traversal start at the upper end runs the piece backwards. The
        // ends are compared as points: an angle a hair below the lower end
        // would read as nearly a full turn above it.
        let start = piece.ends[0];
        piece.reversed =
            distance(start, piece.point_at(high)) < distance(start, piece.point_at(low));
    }
    Some(())
}

fn ascending(first: f64, second: f64, tolerance: f64) -> Option<[f64; 2]> {
    let range = [first.min(second), first.max(second)];
    (range[1] - range[0] > tolerance).then_some(range)
}

fn on_circle(point: Point2, center: Point2, radius: f64) -> Option<()> {
    ((distance(point, center) - radius).abs() <= EPS_PROFILE_WIRE_POINT_MM).then_some(())
}

fn polar_point(curve: LoopCurve, angle: f64) -> Point2 {
    match curve {
        LoopCurve::Arc { center, radius, .. } | LoopCurve::Circle { center, radius } => {
            polar(center, radius, angle)
        }
        LoopCurve::Line { start, .. } => start,
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
        return is_full_circle(first).then(|| vec![use_(first, false)]);
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

    /// The point halfway along the curve.
    fn middle(self) -> Point2 {
        match self {
            Self::Line { start, end } => {
                Point2::new(start.u.midpoint(end.u), start.v.midpoint(end.v))
            }
            Self::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } => polar(center, radius, sweep.mul_add(0.5, start_angle)),
            Self::Circle { center, radius } => polar(center, radius, 0.0),
        }
    }

    /// Whether `point` lies on the curve.
    fn touches(self, point: Point2) -> bool {
        match self {
            Self::Line { start, end } => {
                let length = distance(start, end);
                let along = (point.u - start.u)
                    .mul_add(end.u - start.u, (point.v - start.v) * (end.v - start.v))
                    / length;
                let off = ((point.u - start.u) * (end.v - start.v)
                    - (point.v - start.v) * (end.u - start.u))
                    .abs()
                    / length;
                off <= EPS_PROFILE_WIRE_POINT_MM
                    && (-EPS_PROFILE_WIRE_POINT_MM..=length + EPS_PROFILE_WIRE_POINT_MM)
                        .contains(&along)
            }
            Self::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } => {
                let (low, span) = if sweep < 0.0 {
                    (start_angle + sweep, -sweep)
                } else {
                    (start_angle, sweep)
                };
                let offset = ((point.v - center.v).atan2(point.u - center.u) - low).rem_euclid(TAU);
                let tolerance = EPS_PROFILE_WIRE_POINT_MM / radius;
                on_circle(point, center, radius).is_some()
                    && (offset <= span + tolerance || offset >= TAU - tolerance)
            }
            Self::Circle { center, radius } => on_circle(point, center, radius).is_some(),
        }
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
            // An arc turns as its chord does, plus one whole turn in the
            // arc's sense for a point in the circular segment between them.
            Self::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } => {
                let (start, end) = self.ends();
                let side = |at: Point2| {
                    (end.u - start.u)
                        .mul_add(at.v - start.v, -((end.v - start.v) * (at.u - start.u)))
                };
                let middle = polar(center, radius, sweep.mul_add(0.5, start_angle));
                let in_segment =
                    distance(point, center) < radius && side(point) * side(middle) > 0.0;
                chord_turn(start, end, point) + if in_segment { TAU.copysign(sweep) } else { 0.0 }
            }
            Self::Line { start, end } => chord_turn(start, end, point),
        }
    }
}

/// The angle the segment from `start` to `end` turns about `point`.
fn chord_turn(start: Point2, end: Point2, point: Point2) -> f64 {
    let (su, sv) = (start.u - point.u, start.v - point.v);
    let (eu, ev) = (end.u - point.u, end.v - point.v);
    su.mul_add(ev, -(sv * eu)).atan2(su.mul_add(eu, sv * ev))
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
