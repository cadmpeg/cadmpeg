// SPDX-License-Identifier: Apache-2.0
//! Build IR arenas from parsed Parasolid topology records and carriers.
//!
//! The graph builder walks each face bridge through its loop and coedge rings,
//! resolves edge and vertex uses, closes emitted loops, and groups faces under
//! explicit body records. It derives one body hierarchy when those records are
//! absent. It also derives supported pcurves and periodic seams.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::annotations::{AnnotationBuilder, Annotations};
use cadmpeg_ir::eval::{
    analytic_surface_parameters, nurbs_curve_parameter_domain, nurbs_curve_point,
    nurbs_surface_isocurve, nurbs_surface_parameter_near_point,
    nurbs_surface_parameter_segment_chord_bound, nurbs_surface_parameter_within_tolerance,
    nurbs_surface_partials, nurbs_surface_point, surface_point,
};
use cadmpeg_ir::geometry::{
    knots_nondecreasing, BlendCrossSection, BlendRadiusLaw, BlendSupport, Curve, CurveGeometry,
    Pcurve, PcurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceGeometry, SurfaceParameterAxis,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralSurfaceId,
    RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::Exactness;

use super::attrib;
use super::blend::BlendSupportRef;
use super::entity;
use super::offset::OffsetCarrier;
use super::sweep::{self, SweepKind};
use super::topology::{self, Record};
use super::typed;
use super::{scan_carriers, Carrier, CarrierGeometry, CarrierIndex, LEN_TO_MM};
use crate::parasolid::StreamHeader;

/// Decoded B-rep arenas, provenance, and transfer statistics.
#[derive(Default)]
pub struct Brep {
    /// Source locations for decoded entities.
    pub annotations: Annotations,
    /// Top-level solid or sheet bodies.
    pub bodies: Vec<Body>,
    /// Solid regions / sheet regions owned by each body.
    pub regions: Vec<Region>,
    /// Shells owned by each region.
    pub shells: Vec<Shell>,
    /// Faces reached through face-use bridge records.
    pub faces: Vec<Face>,
    /// Loops reached through `00 0f` loop heads.
    pub loops: Vec<Loop>,
    /// Coedges in loop-ring order.
    pub coedges: Vec<Coedge>,
    /// Edges resolved from edge-use records.
    pub edges: Vec<Edge>,
    /// Vertices resolved from vertex-use and world-point records.
    pub vertices: Vec<Vertex>,
    /// World points converted to millimetres.
    pub points: Vec<Point>,
    /// Analytic, NURBS, or opaque support surfaces.
    pub surfaces: Vec<Surface>,
    /// Exact procedural constructions behind emitted support surfaces.
    pub procedural_surfaces: Vec<ProceduralSurface>,
    /// Analytic, NURBS, or opaque support curves.
    pub curves: Vec<Curve>,
    /// Pcurves derived for supported analytic and NURBS-boundary cases.
    pub pcurves: Vec<Pcurve>,
    /// Records whose carrier kind this codec does not type, retained as
    /// opaque payloads.
    pub unknowns: Vec<UnknownRecord>,
    /// Per-face RGB colors resolved from native entity records.
    pub face_colors: Vec<entity::FaceColor>,
    /// Per-face producing-feature identities resolved from Parasolid attributes.
    pub face_atoms: Vec<attrib::FaceAtom>,
    /// Source-local sequence-to-attribute links carried by face bridge
    /// records. The decode boundary retains this map only for the active
    /// source because SWIFT identifiers resolve in that source namespace.
    pub face_bridge_sequences: Vec<(u32, u16)>,
    /// Source-local sequence-to-attribute links carried by edge-use records.
    /// The decode boundary retains this map only for the active source because
    /// SWIFT identifiers resolve in that source namespace.
    pub edge_use_sequences: Vec<(u32, u16)>,
    /// Source-local sequence-to-attribute links carried by vertex-use records.
    /// The decode boundary retains this map only for the active source because
    /// SWIFT identifiers resolve in that source namespace.
    pub vertex_use_sequences: Vec<(u32, u16)>,
    /// Body-to-history ordinals resolved from Parasolid attributes.
    pub body_modifiers: Vec<attrib::BodyModifier>,
    /// Loss accounting for this decode.
    pub stats: Stats,
}

impl Brep {
    /// Qualify every document-arena identity and internal reference by one site key.
    pub(crate) fn qualify_ids(&mut self, site: &str) {
        let qualify = |value: &str| {
            value.split_once('#').map_or_else(
                || value.to_owned(),
                |(namespace, key)| format!("{namespace}#{key}@{site}"),
            )
        };
        for body in &mut self.bodies {
            body.id.0 = qualify(&body.id.0);
            body.regions.iter_mut().for_each(|id| id.0 = qualify(&id.0));
        }
        for region in &mut self.regions {
            region.id.0 = qualify(&region.id.0);
            region.body.0 = qualify(&region.body.0);
            region
                .shells
                .iter_mut()
                .for_each(|id| id.0 = qualify(&id.0));
        }
        for shell in &mut self.shells {
            shell.id.0 = qualify(&shell.id.0);
            shell.region.0 = qualify(&shell.region.0);
            shell.faces.iter_mut().for_each(|id| id.0 = qualify(&id.0));
            shell
                .wire_edges
                .iter_mut()
                .for_each(|id| id.0 = qualify(&id.0));
            shell
                .free_vertices
                .iter_mut()
                .for_each(|id| id.0 = qualify(&id.0));
        }
        for face in &mut self.faces {
            face.id.0 = qualify(&face.id.0);
            face.shell.0 = qualify(&face.shell.0);
            face.surface.0 = qualify(&face.surface.0);
            face.loops.iter_mut().for_each(|id| id.0 = qualify(&id.0));
        }
        for loop_ in &mut self.loops {
            loop_.id.0 = qualify(&loop_.id.0);
            loop_.face.0 = qualify(&loop_.face.0);
            loop_
                .coedges
                .iter_mut()
                .for_each(|id| id.0 = qualify(&id.0));
        }
        for coedge in &mut self.coedges {
            coedge.id.0 = qualify(&coedge.id.0);
            coedge.owner_loop.0 = qualify(&coedge.owner_loop.0);
            coedge.edge.0 = qualify(&coedge.edge.0);
            coedge.next.0 = qualify(&coedge.next.0);
            coedge.previous.0 = qualify(&coedge.previous.0);
            coedge.radial_next.0 = qualify(&coedge.radial_next.0);
            for use_ in &mut coedge.pcurves {
                use_.pcurve.0 = qualify(&use_.pcurve.0);
            }
        }
        for edge in &mut self.edges {
            edge.id.0 = qualify(&edge.id.0);
            if let Some(curve) = &mut edge.curve {
                curve.0 = qualify(&curve.0);
            }
            edge.start.0 = qualify(&edge.start.0);
            edge.end.0 = qualify(&edge.end.0);
        }
        for vertex in &mut self.vertices {
            vertex.id.0 = qualify(&vertex.id.0);
            vertex.point.0 = qualify(&vertex.point.0);
        }
        self.points
            .iter_mut()
            .for_each(|point| point.id.0 = qualify(&point.id.0));
        for surface in &mut self.surfaces {
            surface.id.0 = qualify(&surface.id.0);
            match &mut surface.geometry {
                SurfaceGeometry::Procedural { construction } => {
                    construction.0 = qualify(&construction.0);
                }
                SurfaceGeometry::Unknown {
                    record: Some(record),
                } => {
                    record.0 = qualify(&record.0);
                }
                _ => {}
            }
        }
        for procedural in &mut self.procedural_surfaces {
            procedural.id.0 = qualify(&procedural.id.0);
            procedural.surface.0 = qualify(&procedural.surface.0);
            match &mut procedural.definition {
                ProceduralSurfaceDefinition::Blend {
                    supports, spine, ..
                } => {
                    for support in supports.iter_mut().flatten() {
                        support.surface.0 = qualify(&support.surface.0);
                    }
                    if let Some(spine) = spine {
                        spine.0 = qualify(&spine.0);
                    }
                }
                ProceduralSurfaceDefinition::Offset { support, .. } => {
                    support.0 = qualify(&support.0);
                }
                _ => {}
            }
        }
        for curve in &mut self.curves {
            curve.id.0 = qualify(&curve.id.0);
            if let CurveGeometry::Unknown {
                record: Some(record),
            } = &mut curve.geometry
            {
                record.0 = qualify(&record.0);
            }
        }
        self.pcurves
            .iter_mut()
            .for_each(|pcurve| pcurve.id.0 = qualify(&pcurve.id.0));
        for record in &mut self.unknowns {
            record.id.0 = qualify(&record.id.0);
            record
                .links
                .iter_mut()
                .for_each(|link| *link = qualify(link));
        }
        for color in &mut self.face_colors {
            if let Some(target) = &mut color.target {
                *target = qualify(target);
            }
        }
        for atom in &mut self.face_atoms {
            if let Some(target) = &mut atom.target {
                *target = qualify(target);
            }
        }
        for modifier in &mut self.body_modifiers {
            if let Some(target) = &mut modifier.target {
                *target = qualify(target);
            }
        }
        self.annotations.provenance = std::mem::take(&mut self.annotations.provenance)
            .into_iter()
            .map(|(id, value)| (qualify(&id), value))
            .collect();
        self.annotations.exactness = std::mem::take(&mut self.annotations.exactness)
            .into_iter()
            .map(|(id, value)| (qualify(&id), value))
            .collect();
    }
}

fn shell_face_components(out: &Brep, native_shell_id: &str) -> Vec<Vec<FaceId>> {
    let candidates = out
        .faces
        .iter()
        .filter(|face| face.shell.0 == native_shell_id)
        .map(|face| face.id.clone())
        .collect::<Vec<_>>();
    let candidate_ids = candidates
        .iter()
        .map(|face| face.0.as_str())
        .collect::<HashSet<_>>();
    let loop_faces = out
        .loops
        .iter()
        .filter(|loop_| candidate_ids.contains(loop_.face.0.as_str()))
        .map(|loop_| (loop_.id.0.as_str(), loop_.face.0.as_str()))
        .collect::<HashMap<_, _>>();
    let mut faces_by_edge = HashMap::<&str, HashSet<&str>>::new();
    for coedge in &out.coedges {
        if let Some(face) = loop_faces.get(coedge.owner_loop.0.as_str()) {
            faces_by_edge
                .entry(coedge.edge.0.as_str())
                .or_default()
                .insert(*face);
        }
    }
    let mut neighbors = HashMap::<&str, HashSet<&str>>::new();
    for edge_faces in faces_by_edge.values() {
        for &face in edge_faces {
            neighbors
                .entry(face)
                .or_default()
                .extend(edge_faces.iter().copied().filter(|other| *other != face));
        }
    }

    let mut assigned = HashSet::new();
    let mut components = Vec::new();
    for face in &candidates {
        if !assigned.insert(face.0.as_str()) {
            continue;
        }
        let mut component = Vec::new();
        let mut pending = vec![face.0.as_str()];
        while let Some(current) = pending.pop() {
            component.push(FaceId(current.to_string()));
            for &neighbor in neighbors.get(current).into_iter().flatten() {
                if assigned.insert(neighbor) {
                    pending.push(neighbor);
                }
            }
        }
        component.sort_by(|left, right| left.0.cmp(&right.0));
        components.push(component);
    }
    components
}

#[derive(Debug, Clone)]
struct BodyRecord {
    attr: u16,
    kind: BodyKind,
    refs: Vec<u16>,
    offset: usize,
    regions: Vec<RegionRecord>,
}

#[derive(Debug, Clone)]
struct RegionRecord {
    attr: u16,
    offset: usize,
    shells: Vec<ShellRecord>,
}

#[derive(Debug, Clone)]
struct ShellRecord {
    attr: u16,
    offset: usize,
    refs: Vec<u16>,
}

/// Transfer limitations found while building a [`Brep`].
#[derive(Default)]
pub struct Stats {
    /// Framed top-level model entity records across the selected stream site.
    pub source_entity_records: usize,
    /// Face-color bindings withheld because current records conflict.
    pub unresolved_face_colors: usize,
    /// Face owners with multiple non-equivalent bridge uses.
    pub ambiguous_face_owners: usize,
    /// Canonical faces that no explicit body record claims.
    pub unclaimed_faces: usize,
    /// Faces on a support surface this codec does not type; emitted with an
    /// unknown-geometry carrier.
    pub unknown_surface_faces: usize,
    /// Hidden procedural support surfaces whose carrier geometry remains opaque.
    pub unknown_procedural_supports: usize,
    /// Edges whose support curve is an untyped carrier (emitted with no curve).
    pub unknown_curve_edges: usize,
    /// Pcurves withheld because geometric inverse selection was ambiguous.
    pub ambiguous_pcurve_parameters: usize,
    /// NURBS edge carriers whose vertex range is off their bound surface.
    pub off_surface_nurbs_pcurves: usize,
    /// No explicit body record was available, so one body hierarchy was derived.
    pub synthetic_body_grouping: bool,
}

fn id_face(a: u16) -> String {
    format!("sldprt:brep:face#{a}")
}
fn id_surf(a: u16) -> String {
    format!("sldprt:brep:surf#{a}")
}
fn id_loop(a: u16) -> String {
    format!("sldprt:brep:loop#{a}")
}
fn id_coedge(a: u16) -> String {
    format!("sldprt:brep:coedge#{a}")
}
fn id_edge(a: u16) -> String {
    format!("sldprt:brep:edge#{a}")
}
fn id_curve(a: u16) -> String {
    format!("sldprt:brep:curve#{a}")
}
fn id_vertex(a: u16) -> String {
    format!("sldprt:brep:vertex#{a}")
}
fn id_point(a: u16) -> String {
    format!("sldprt:brep:point#{a}")
}
fn id_closed_point(edge: u16) -> String {
    format!("sldprt:brep:point#closed-circle-{edge}")
}
fn id_closed_vertex(edge: u16) -> String {
    format!("sldprt:brep:vertex#closed-circle-{edge}")
}

/// One face-use's decoded loops: ordered coedge rings, keyed by loop attr.
#[derive(Clone, PartialEq, Eq)]
struct WalkedFace {
    bridge_attr: u16,
    surface_attr: u16,
    marker: u8,
    /// `(loop_attr, ordered_coedge_attrs)` in sibling order.
    loops: Vec<(u16, Vec<u16>)>,
}

/// Follow the sibling loop-head chain of a bridge and each loop's coedge ring,
/// returning the ordered structure with cycles guarded.
/// Resolve a face whose `refs[4]` carrier is a swept/spun construction to a
/// solved NURBS patch. Returns `(geometry, record offset, annotation tag,
/// derived exactness)`. A spun surface is exact for an exact profile; a swept
/// surface patch is derived because its ruling extent comes from the face's
/// vertex points rather than a stored interval.
fn resolve_sweep_surface(
    carriers: &CarrierIndex,
    tables: &topology::Tables,
    face: &WalkedFace,
) -> Option<(SurfaceGeometry, usize, &'static str, bool)> {
    let construction = carriers.sweep(face.surface_attr)?;
    let profile = carriers.curve(construction.profile_attr)?;
    let CarrierGeometry::Curve(profile_geometry) = &profile.geometry else {
        return None;
    };
    let curve = sweep::profile_nurbs(profile_geometry)?;
    let profile_derived = carriers.curve_is_derived(construction.profile_attr);
    match &construction.kind {
        SweepKind::Spun { base, axis } => Some((
            SurfaceGeometry::Nurbs(sweep::spun_nurbs(&curve, *base, *axis)),
            construction.offset,
            "00_44",
            profile_derived,
        )),
        SweepKind::Swept { direction } => {
            // Ruling extent: face vertex travel bracketed by the profile poles'
            // own travel along the sweep direction, in millimetres.
            let project = |p: &cadmpeg_ir::math::Point3| {
                p.x * direction.x + p.y * direction.y + p.z * direction.z
            };
            let mut point_lo = f64::INFINITY;
            let mut point_hi = f64::NEG_INFINITY;
            for (_, ring) in &face.loops {
                for ce_attr in ring {
                    let Some(vuse) = tables
                        .coedges
                        .get(ce_attr)
                        .and_then(|ce| ce.refs.get(4).copied())
                    else {
                        continue;
                    };
                    let Some(coordinates) = tables
                        .vertex_uses
                        .get(&vuse)
                        .and_then(|vu| vu.refs.get(4).copied())
                        .and_then(|pa| tables.points.get(&pa))
                        .and_then(|p| p.xyz_m)
                    else {
                        continue;
                    };
                    let travel = coordinates[0] * LEN_TO_MM * direction.x
                        + coordinates[1] * LEN_TO_MM * direction.y
                        + coordinates[2] * LEN_TO_MM * direction.z;
                    point_lo = point_lo.min(travel);
                    point_hi = point_hi.max(travel);
                }
            }
            if point_lo > point_hi {
                return None;
            }
            let pole_travel: Vec<f64> = curve.control_points.iter().map(project).collect();
            let pole_lo = pole_travel.iter().copied().fold(f64::INFINITY, f64::min);
            let pole_hi = pole_travel
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            let v_start = point_lo - pole_hi;
            let v_end = point_hi - pole_lo;
            let pad = 1.0e-6_f64.max((v_end - v_start) * 1.0e-3);
            Some((
                SurfaceGeometry::Nurbs(sweep::swept_nurbs(
                    &curve,
                    *direction,
                    v_start - pad,
                    v_end + pad,
                )?),
                construction.offset,
                "00_43",
                true,
            ))
        }
    }
}

fn id_hidden_support_surface(attr: u16) -> SurfaceId {
    SurfaceId(format!("sldprt:brep:hidden-support-surf#{attr}"))
}

fn id_offset_construction(attr: u16) -> ProceduralSurfaceId {
    ProceduralSurfaceId(format!("sldprt:brep:offset-support-construction#{attr}"))
}

fn emit_offset_surface(
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    surface: SurfaceId,
    construction: ProceduralSurfaceId,
    support: SurfaceId,
    offset: &OffsetCarrier,
) {
    annotations
        .note(&surface, source_stream, offset.offset as u64)
        .tag("00_3c");
    out.procedural_surfaces.push(ProceduralSurface {
        id: construction.clone(),
        surface: surface.clone(),
        definition: ProceduralSurfaceDefinition::Offset {
            support,
            distance: offset.distance,
            u_sense: None,
            v_sense: None,
            extension_flags: Vec::new(),
            revision_form: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    out.surfaces.push(Surface {
        id: surface,
        source_object: None,
        geometry: SurfaceGeometry::Procedural { construction },
    });
}

/// Whether one referenced support can be emitted without a procedural cycle.
///
/// An untyped reference is still a valid opaque support surface. Offset
/// carriers recurse because a cycle cannot define a surface.
fn support_is_acyclic(attr: u16, carriers: &CarrierIndex, resolving: &mut HashSet<u16>) -> bool {
    if !resolving.insert(attr) {
        return false;
    }
    let acyclic = carriers.surface(attr).is_some()
        || carriers
            .offset(attr)
            .is_none_or(|offset| support_is_acyclic(offset.support, carriers, resolving));
    resolving.remove(&attr);
    acyclic
}

/// Resolve one carrier as a support of a procedural construction. A carrier
/// owned by an emitted face reuses that face's surface identity. Otherwise the
/// decoder emits a hidden analytic, NURBS, recursive offset, or opaque support
/// surface. Offset cycles invalidate the complete construction.
fn ensure_surface_support(
    attr: u16,
    carriers: &CarrierIndex,
    emitted_face_surface_by_carrier: &HashMap<u16, u16>,
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    resolving: &mut HashSet<u16>,
) -> Option<SurfaceId> {
    if !resolving.insert(attr) {
        return None;
    }
    let result = (|| {
        if let Some(carrier) = carriers.surface(attr) {
            let id = emitted_face_surface_by_carrier.get(&attr).map_or_else(
                || id_hidden_support_surface(attr),
                |bridge| SurfaceId(id_surf(*bridge)),
            );
            if !out.surfaces.iter().any(|surface| surface.id == id)
                && !emitted_face_surface_by_carrier.contains_key(&attr)
            {
                let CarrierGeometry::Surface(geometry) = &carrier.geometry else {
                    unreachable!("surface index contains only surface carriers");
                };
                let mut geometry = geometry.clone();
                if let Some((_, u_reference, v_reference)) = carrier.frame {
                    fold_surface_frame(&mut geometry, u_reference, v_reference);
                    annotate_surface_frame(annotations, &id.0, &geometry);
                }
                annotations
                    .note(&id, source_stream, carrier.offset as u64)
                    .tag("procedural_support");
                out.surfaces.push(Surface {
                    id: id.clone(),
                    source_object: None,
                    geometry,
                });
            }
            Some(id)
        } else if let Some(offset) = carriers.offset(attr) {
            let support = ensure_surface_support(
                offset.support,
                carriers,
                emitted_face_surface_by_carrier,
                out,
                annotations,
                source_stream,
                resolving,
            )?;
            let surface = emitted_face_surface_by_carrier.get(&attr).map_or_else(
                || id_hidden_support_surface(attr),
                |bridge| SurfaceId(id_surf(*bridge)),
            );
            if !emitted_face_surface_by_carrier.contains_key(&attr)
                && !out.surfaces.iter().any(|candidate| candidate.id == surface)
            {
                let construction = id_offset_construction(attr);
                emit_offset_surface(
                    out,
                    annotations,
                    source_stream,
                    surface.clone(),
                    construction,
                    support,
                    offset,
                );
            }
            Some(surface)
        } else {
            let surface = emitted_face_surface_by_carrier.get(&attr).map_or_else(
                || id_hidden_support_surface(attr),
                |bridge| SurfaceId(id_surf(*bridge)),
            );
            if !emitted_face_surface_by_carrier.contains_key(&attr)
                && !out.surfaces.iter().any(|candidate| candidate.id == surface)
            {
                annotations.exactness(&surface, Exactness::Unknown);
                out.stats.unknown_procedural_supports += 1;
                out.surfaces.push(Surface {
                    id: surface.clone(),
                    source_object: None,
                    geometry: SurfaceGeometry::Unknown { record: None },
                });
            }
            Some(surface)
        }
    })();
    resolving.remove(&attr);
    result
}

fn walk_face(bridge: &Record, t: &topology::Tables) -> WalkedFace {
    let surface_attr = *bridge.refs.get(4).unwrap_or(&0);
    let mut loops = Vec::new();
    let mut loop_ref = *bridge.refs.get(2).unwrap_or(&0);
    let mut loop_guard = HashSet::new();
    while loop_ref != 0 && loop_guard.insert(loop_ref) {
        let Some(lp) = t.loops.get(&loop_ref) else {
            break;
        };
        let owner_bridge = lp.refs.get(2).copied().unwrap_or(0);
        let same_face_use = owner_bridge == bridge.attr
            || bridge.owner.is_some_and(|owner| {
                t.bridges
                    .get(&owner_bridge)
                    .and_then(|candidate| candidate.owner)
                    == Some(owner)
            });
        if !same_face_use {
            break;
        }
        let first = *lp.refs.get(1).unwrap_or(&0);
        let mut ring = Vec::new();
        let mut ce_ref = first;
        let mut ce_guard = HashSet::new();
        let mut ring_closed = false;
        while ce_ref != 0 && ce_guard.insert(ce_ref) {
            let Some(ce) = t.coedges.get(&ce_ref) else {
                break;
            };
            if ce.refs.get(1).copied() != Some(loop_ref) {
                break;
            }
            ring.push(ce_ref);
            ce_ref = *ce.refs.get(3).unwrap_or(&0);
            if ce_ref == first {
                ring_closed = true;
                break;
            }
        }
        if ring_closed {
            loops.push((loop_ref, ring));
        }
        loop_ref = *lp.refs.get(3).unwrap_or(&0);
    }
    WalkedFace {
        bridge_attr: bridge.attr,
        surface_attr,
        marker: bridge.marker.unwrap_or(0x2b),
        loops,
    }
}

fn sense_of(marker: u8) -> Sense {
    if marker == 0x2d {
        Sense::Reversed
    } else {
        Sense::Forward
    }
}

fn edge_parameter_range(
    carrier: &Carrier,
    endpoints: Option<[cadmpeg_ir::math::Point3; 2]>,
) -> Option<([f64; 2], bool)> {
    const TOLERANCE_MM: f64 = 1.0e-7;

    let range = carrier.parameter_range?;
    let range = match &carrier.geometry {
        CarrierGeometry::Curve(CurveGeometry::Line { .. }) => {
            range.map(|parameter| parameter * LEN_TO_MM)
        }
        _ => range,
    };
    let range = if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
    };
    let Some(endpoints) = endpoints else {
        return Some((range, false));
    };
    let CarrierGeometry::Curve(geometry) = &carrier.geometry else {
        return None;
    };
    let evaluated = range.map(|parameter| cadmpeg_ir::eval::curve_point(geometry, parameter));
    let [Some(first), Some(second)] = evaluated else {
        return None;
    };
    let distance = |left: cadmpeg_ir::math::Point3, right: cadmpeg_ir::math::Point3| {
        (left.x - right.x)
            .hypot(left.y - right.y)
            .hypot(left.z - right.z)
    };
    if distance(first, endpoints[0]).max(distance(second, endpoints[1])) <= TOLERANCE_MM {
        Some((range, false))
    } else if distance(first, endpoints[1]).max(distance(second, endpoints[0])) <= TOLERANCE_MM {
        Some((range, true))
    } else {
        None
    }
}

/// Resolve the coedge that defines an edge's stored direction.
///
/// Bare records carry an explicit coedge attr in `refs[0]`. Prefixed records
/// carry no such slot, so a zero/sentinel slot is resolved only when exactly
/// one same-edge forward coedge exists. A non-sentinel explicit reference is
/// authoritative: a dangling, cross-edge, or reversed reference is rejected.
fn canonical_coedge_attr(
    edge_attr: u16,
    edge_use: Option<&topology::Record>,
    coedges: &HashMap<u16, topology::Record>,
) -> Option<u16> {
    if let Some(explicit) = edge_use
        .and_then(|record| record.refs.first().copied())
        .filter(|attr| *attr > 1)
    {
        let coedge = coedges.get(&explicit)?;
        return (coedge.refs.get(6) == Some(&edge_attr) && coedge.marker == Some(0x2b))
            .then_some(explicit);
    }

    let mut candidates = coedges.iter().filter(|(_, coedge)| {
        coedge.refs.get(6) == Some(&edge_attr) && coedge.marker == Some(0x2b)
    });
    let (&attr, _) = candidates.next()?;
    candidates.next().is_none().then_some(attr)
}

fn edge_end_vuse(canonical: u16, ring_end: u16, coedges: &HashMap<u16, topology::Record>) -> u16 {
    let Some(twin) = coedges
        .get(&canonical)
        .and_then(|coedge| coedge.refs.get(5).copied())
        .filter(|twin| *twin != canonical)
    else {
        return ring_end;
    };
    let Some(twin_record) = coedges.get(&twin) else {
        return ring_end;
    };
    if twin_record.refs.get(5) != Some(&canonical) {
        return ring_end;
    }
    twin_record.refs.get(4).copied().unwrap_or(ring_end)
}

fn surface_sense(marker: u8, orientation_reversed: bool) -> Sense {
    match (sense_of(marker), orientation_reversed) {
        (Sense::Forward, true) => Sense::Reversed,
        (Sense::Reversed, true) => Sense::Forward,
        (sense, false) => sense,
    }
}

/// Decode one parsed Parasolid stream into B-rep arenas.
///
/// `stream` names the provenance stream recorded in [`Brep::annotations`].
pub fn decode(payload: &[u8], header: &StreamHeader, stream: &str) -> Brep {
    decode_body(&payload[header.body_offset.min(payload.len())..], stream)
}

/// Decode related partition and deltas streams as one record source.
///
/// Partition records are the base set. Deltas records fill missing subordinate
/// records and point updates, but do not replace a same-identity partition
/// topology or carrier record. `stream` names the combined provenance source.
pub fn decode_bodies(bodies: &[(&[u8], &StreamHeader)], stream: &str) -> Brep {
    let mut carriers = CarrierIndex::default();
    let mut tables = topology::Tables::default();
    let mut facts = entity::Facts::default();
    let mut typed_facts = typed::Facts::default();
    let mut initialized = false;
    let mut ordered = bodies.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|(_, header)| header.description.to_ascii_lowercase().contains("deltas"));
    let entity_streams = ordered
        .iter()
        .map(|(payload, header)| {
            let body = &payload[header.body_offset.min(payload.len())..];
            let is_deltas = header.description.to_ascii_lowercase().contains("deltas");
            (body, is_deltas)
        })
        .collect::<Vec<_>>();
    let typed_streams = entity_streams
        .iter()
        .map(|(body, _)| typed::scan(body))
        .collect::<Vec<_>>();
    for stream_typed_facts in &typed_streams {
        typed_facts.merge_missing(stream_typed_facts.clone());
    }
    let typed_bridge_attrs = typed_facts
        .faces
        .iter()
        .map(|face| face.attr)
        .collect::<HashSet<_>>();
    let selected_bridge_attrs = typed_facts
        .has_valid_ownership()
        .then_some(&typed_bridge_attrs);
    let typed_ownership_valid = typed_facts.has_valid_ownership();
    for (stream_order, ((payload, header), stream_typed_facts)) in
        ordered.into_iter().zip(typed_streams).enumerate()
    {
        let body = &payload[header.body_offset.min(payload.len())..];
        let is_deltas = header.description.to_ascii_lowercase().contains("deltas");
        let typed_face_offsets = if typed_ownership_valid {
            stream_typed_facts
                .faces
                .iter()
                .map(|face| face.offset)
                .collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };
        typed_facts.merge_missing(stream_typed_facts);
        carriers.merge_missing(scan_carriers(body));
        let curve_attrs = carriers.curve_attrs();
        let scanned_tables = if is_deltas {
            topology::scan_deltas_with_curve_attrs_excluding(
                body,
                &curve_attrs,
                &typed_face_offsets,
            )
        } else {
            topology::scan_with_curve_attrs_excluding(body, &curve_attrs, &typed_face_offsets)
        };
        let mut scanned_facts = entity::scan_metadata(body, is_deltas);
        for color in &mut scanned_facts.face_colors {
            color.stream_order = stream_order;
        }
        for version in &mut scanned_facts.face_color_versions {
            version.stream_order = stream_order;
        }
        if !initialized || !is_deltas {
            if initialized {
                tables.merge_deltas(scanned_tables, selected_bridge_attrs);
                facts.face_colors.append(&mut scanned_facts.face_colors);
                facts
                    .face_color_versions
                    .append(&mut scanned_facts.face_color_versions);
                facts.face_atoms.append(&mut scanned_facts.face_atoms);
                facts
                    .body_modifiers
                    .append(&mut scanned_facts.body_modifiers);
                facts.entity_count += scanned_facts.entity_count;
                facts.unresolved_face_colors += scanned_facts.unresolved_face_colors;
            } else {
                tables = scanned_tables;
                facts = scanned_facts;
                initialized = true;
            }
        } else {
            tables.merge_deltas(scanned_tables, selected_bridge_attrs);
            facts.face_colors.append(&mut scanned_facts.face_colors);
            facts
                .face_color_versions
                .append(&mut scanned_facts.face_color_versions);
            facts.face_atoms.append(&mut scanned_facts.face_atoms);
            facts
                .body_modifiers
                .append(&mut scanned_facts.body_modifiers);
            facts.entity_count += scanned_facts.entity_count;
            facts.unresolved_face_colors += scanned_facts.unresolved_face_colors;
        }
    }
    decode_graph(&carriers, &tables, facts, &typed_facts, stream)
}

fn decode_body(body: &[u8], stream: &str) -> Brep {
    let carriers = scan_carriers(body);
    let curve_attrs = carriers.curve_attrs();
    let typed_facts = typed::scan(body);
    let typed_face_offsets = if typed_facts.has_valid_ownership() {
        typed_facts
            .faces
            .iter()
            .map(|face| face.offset)
            .collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };
    let t = topology::scan_with_curve_attrs_excluding(body, &curve_attrs, &typed_face_offsets);
    let entity_facts = entity::scan_metadata(body, false);
    decode_graph(&carriers, &t, entity_facts, &typed_facts, stream)
}

fn unique_body_modifiers(modifiers: Vec<attrib::BodyModifier>) -> Vec<attrib::BodyModifier> {
    let mut by_attr = HashMap::<u16, Option<attrib::BodyModifier>>::new();
    for modifier in modifiers {
        match by_attr.entry(modifier.body_attr) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Some(modifier));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry
                    .get()
                    .as_ref()
                    .is_some_and(|previous| previous.history_ordinal != modifier.history_ordinal)
                {
                    *entry.get_mut() = None;
                }
            }
        }
    }
    let mut out = by_attr.into_values().flatten().collect::<Vec<_>>();
    out.sort_by_key(|modifier| modifier.body_attr);
    out
}

fn unique_face_colors(
    colors: Vec<entity::FaceColor>,
    versions: Vec<entity::FaceColorVersion>,
) -> (Vec<entity::FaceColor>, usize) {
    let mut current_versions = HashMap::<u16, (u32, usize)>::new();
    for version in versions {
        current_versions
            .entry(version.face_attr)
            .and_modify(|current| {
                *current = (*current).max((version.seq, version.stream_order));
            })
            .or_insert((version.seq, version.stream_order));
    }

    let mut by_face = HashMap::<u16, Vec<entity::FaceColor>>::new();
    for color in colors {
        if current_versions.get(&color.face_attr) == Some(&(color.face_seq, color.stream_order)) {
            by_face.entry(color.face_attr).or_default().push(color);
        }
    }

    let mut unresolved = 0;
    let mut selected = Vec::new();
    for candidates in by_face.into_values() {
        let first = &candidates[0];
        if candidates.iter().all(|candidate| {
            candidate.color_attr == first.color_attr && candidate.color == first.color
        }) {
            selected.push(first.clone());
        } else {
            unresolved += 1;
        }
    }

    let mut by_color = HashMap::<u16, Vec<entity::FaceColor>>::new();
    for color in selected {
        by_color.entry(color.color_attr).or_default().push(color);
    }
    let mut out = Vec::new();
    for candidates in by_color.into_values() {
        let first = &candidates[0];
        if candidates
            .iter()
            .all(|candidate| candidate.color == first.color)
        {
            out.extend(candidates);
        } else {
            unresolved += candidates.len();
        }
    }
    out.sort_by_key(|color| (color.face_attr, color.offset));
    (out, unresolved)
}

fn typed_body_records(facts: &typed::Facts, tables: &topology::Tables) -> Option<Vec<BodyRecord>> {
    let bridge_attrs = tables.bridges.keys().copied().collect::<HashSet<_>>();
    let hierarchies = facts.hierarchies(&bridge_attrs)?;
    let mut records = Vec::with_capacity(hierarchies.len());
    for hierarchy in hierarchies {
        let mut body_refs = hierarchy
            .regions
            .iter()
            .map(|region| region.attr)
            .chain(hierarchy.shells.iter().map(|shell| shell.attr))
            .chain(hierarchy.faces.iter().map(|(face, _)| *face))
            .collect::<Vec<_>>();
        body_refs.sort_unstable();
        body_refs.dedup();
        let mut regions = hierarchy
            .regions
            .iter()
            .map(|region| {
                let mut shells = hierarchy
                    .shells
                    .iter()
                    .filter(|shell| u16::try_from(shell.refs[6]).ok() == Some(region.attr))
                    .map(|shell| {
                        let mut refs = hierarchy
                            .faces
                            .iter()
                            .filter(|(_, shell_attr)| *shell_attr == shell.attr)
                            .map(|(face_attr, _)| *face_attr)
                            .collect::<Vec<_>>();
                        refs.sort_unstable();
                        refs.dedup();
                        ShellRecord {
                            attr: shell.attr,
                            offset: shell.offset,
                            refs,
                        }
                    })
                    .collect::<Vec<_>>();
                shells.sort_by_key(|shell| shell.attr);
                RegionRecord {
                    attr: region.attr,
                    offset: region.offset,
                    shells,
                }
            })
            .collect::<Vec<_>>();
        regions.sort_by_key(|region| region.attr);
        records.push(BodyRecord {
            attr: hierarchy.body.attr,
            kind: hierarchy.kind,
            refs: body_refs,
            offset: hierarchy.body.offset,
            regions,
        });
    }
    records.sort_by_key(|record| record.attr);
    (!records.is_empty()).then_some(records)
}

fn decode_graph(
    carriers: &CarrierIndex,
    t: &topology::Tables,
    entity_facts: entity::Facts,
    typed_facts: &typed::Facts,
    stream: &str,
) -> Brep {
    let typed_records = typed_body_records(typed_facts, t);
    let body_records = typed_records.unwrap_or_default();
    let body_modifiers = unique_body_modifiers(entity_facts.body_modifiers);
    let (face_colors, conflicting_face_colors) =
        unique_face_colors(entity_facts.face_colors, entity_facts.face_color_versions);
    let mut face_bridge_sequences = t
        .bridges
        .values()
        .filter_map(|bridge| bridge.sequence.map(|sequence| (sequence, bridge.attr)))
        .collect::<Vec<_>>();
    face_bridge_sequences.sort_unstable();
    face_bridge_sequences.dedup();
    let mut edge_use_sequences = t
        .edge_uses
        .values()
        .filter_map(|edge_use| edge_use.sequence.map(|sequence| (sequence, edge_use.attr)))
        .collect::<Vec<_>>();
    edge_use_sequences.sort_unstable();
    edge_use_sequences.dedup();
    let mut vertex_use_sequences = t
        .vertex_uses
        .values()
        .filter_map(|vertex_use| {
            vertex_use
                .sequence
                .map(|sequence| (sequence, vertex_use.attr))
        })
        .collect::<Vec<_>>();
    vertex_use_sequences.sort_unstable();
    vertex_use_sequences.dedup();

    let mut out = Brep {
        face_colors,
        face_atoms: entity_facts.face_atoms,
        face_bridge_sequences,
        edge_use_sequences,
        vertex_use_sequences,
        body_modifiers,
        stats: Stats {
            source_entity_records: entity_facts.entity_count,
            unresolved_face_colors: entity_facts.unresolved_face_colors + conflicting_face_colors,
            ..Stats::default()
        },
        ..Brep::default()
    };
    let mut annotations = AnnotationBuilder::new();
    let source_stream = annotations.stream(stream);
    if t.bridges.is_empty() {
        return out;
    }

    // Walk every face-use bridge to collect its ordered loop/coedge structure.
    // A bridge owner identifies the canonical face entity, not an additional
    // face identity. Equivalent bridge payloads are duplicate uses; distinct
    // payloads have no source selector and must remain unresolved together.
    let mut faces = Vec::new();
    let mut owned_faces = HashMap::<u16, Vec<(&topology::Record, WalkedFace)>>::new();
    for bridge in t.bridges.values() {
        let face = walk_face(bridge, t);
        if let Some(owner) = bridge.owner {
            owned_faces.entry(owner).or_default().push((bridge, face));
        } else {
            faces.push(face);
        }
    }
    let mut ambiguous_face_owners = 0;
    for mut uses in owned_faces.into_values() {
        uses.sort_by_key(|(bridge, _)| (bridge.offset, bridge.attr));
        let Some((first_bridge, first_face)) = uses.first() else {
            continue;
        };
        let equivalent = uses.iter().skip(1).all(|(bridge, face)| {
            bridge.refs == first_bridge.refs
                && bridge.marker == first_bridge.marker
                && face.surface_attr == first_face.surface_attr
                && face.marker == first_face.marker
                && face.loops == first_face.loops
        });
        if equivalent {
            faces.push(first_face.clone());
        } else {
            ambiguous_face_owners += 1;
        }
    }
    faces.sort_by_key(|face| face.bridge_attr);
    out.stats.ambiguous_face_owners += ambiguous_face_owners;

    // Edge attr -> [(coedge attr, start vuse, next coedge's start vuse)] from
    // the ring walk. The ring order supplies a boundary edge's second endpoint;
    // a reciprocal twin supplies it for a two-sided edge.
    let mut edge_incidence: HashMap<u16, Vec<(u16, u16, u16)>> = HashMap::new();

    for f in &faces {
        for (_loop_attr, ring) in &f.loops {
            let k = ring.len();
            for (i, &ce_attr) in ring.iter().enumerate() {
                let Some(ce) = t.coedges.get(&ce_attr) else {
                    continue;
                };
                let next_attr = ring[(i + 1) % k];
                let start_vuse = ce.refs.get(4).copied().unwrap_or(0);
                let next_vuse = t
                    .coedges
                    .get(&next_attr)
                    .and_then(|next| next.refs.get(4).copied())
                    .unwrap_or(0);
                let edge_attr = ce.refs.get(6).copied().unwrap_or(0);
                if edge_attr != 0 {
                    edge_incidence
                        .entry(edge_attr)
                        .or_default()
                        .push((ce_attr, start_vuse, next_vuse));
                }
            }
        }
    }

    // Kept-entity sets, so only chain-reachable records are emitted.
    let mut kept_vertices: HashSet<u16> = HashSet::new();
    let mut kept_points: HashSet<u16> = HashSet::new();
    // Edge attr -> (canonical start vuse, canonical end vuse, curve carrier attr).
    let mut edge_ends: HashMap<u16, (u16, u16, u16)> = HashMap::new();

    for (edge_attr, incidences) in edge_incidence {
        let canonical = canonical_coedge_attr(edge_attr, t.edge_uses.get(&edge_attr), &t.coedges);
        let Some(canonical) = canonical else {
            continue;
        };
        let Some((_, start_vuse, ring_end_vuse)) = incidences
            .iter()
            .find(|(coedge_attr, _, _)| *coedge_attr == canonical)
        else {
            continue;
        };
        let end_vuse = edge_end_vuse(canonical, *ring_end_vuse, &t.coedges);
        let curve_attr = t
            .edge_uses
            .get(&edge_attr)
            .and_then(|edge_use| edge_use.refs.get(3).copied())
            .unwrap_or(0);
        edge_ends.insert(edge_attr, (*start_vuse, end_vuse, curve_attr));
        for vuse in [*start_vuse, end_vuse] {
            if vuse == 0 {
                continue;
            }
            if let Some(vu) = t.vertex_uses.get(&vuse) {
                let point_attr = vu.refs.get(4).copied().unwrap_or(0);
                if t.points.contains_key(&point_attr) {
                    kept_vertices.insert(vuse);
                    kept_points.insert(point_attr);
                }
            }
        }
    }

    // Points.
    let mut point_attrs: Vec<u16> = kept_points.iter().copied().collect();
    point_attrs.sort_unstable();
    for a in point_attrs {
        let rec = &t.points[&a];
        annotations
            .note(id_point(a), source_stream, rec.offset as u64)
            .tag("00_1d");
        let [x, y, z] = rec.xyz_m.unwrap_or([0.0, 0.0, 0.0]);
        out.points.push(Point {
            id: PointId(id_point(a)),
            position: cadmpeg_ir::math::Point3::new(x * LEN_TO_MM, y * LEN_TO_MM, z * LEN_TO_MM),
            source_object: None,
        });
    }

    // Vertices.
    let mut vuse_attrs: Vec<u16> = kept_vertices.iter().copied().collect();
    vuse_attrs.sort_unstable();
    for a in vuse_attrs {
        let rec = &t.vertex_uses[&a];
        let point_attr = *rec.refs.get(4).unwrap_or(&0);
        annotations
            .note(id_vertex(a), source_stream, rec.offset as u64)
            .tag("00_12");
        out.vertices.push(Vertex {
            id: VertexId(id_vertex(a)),
            point: PointId(id_point(point_attr)),
            tolerance: None,
        });
    }

    // Curves and edges. An edge keeps a curve only when its carrier decodes to a
    // curve kind; a nonzero-but-untyped carrier is counted as loss.
    let mut emitted_curves: HashSet<u16> = HashSet::new();
    let mut edge_endpoint_positions = HashMap::<u16, [cadmpeg_ir::math::Point3; 2]>::new();
    let mut reversed_edge_orientation = HashSet::<u16>::new();
    let mut edge_attrs: Vec<u16> = edge_ends.keys().copied().collect();
    edge_attrs.sort_unstable();
    for e in edge_attrs {
        let (start_v, end_v, curve_attr) = edge_ends[&e];
        let resolved_endpoints = kept_vertices.contains(&start_v) && kept_vertices.contains(&end_v);
        let closed_circle_point = (!resolved_endpoints && start_v <= 1 && end_v <= 1)
            .then(|| carriers.curve(curve_attr))
            .flatten()
            .and_then(|carrier| match &carrier.geometry {
                CarrierGeometry::Curve(CurveGeometry::Circle {
                    center,
                    ref_direction,
                    radius,
                    ..
                }) => Some(cadmpeg_ir::math::Point3::new(
                    center.x + ref_direction.x * radius,
                    center.y + ref_direction.y * radius,
                    center.z + ref_direction.z * radius,
                )),
                _ => None,
            });
        if !resolved_endpoints && closed_circle_point.is_none() {
            continue;
        }
        let (mut start_id, mut end_id) = if let Some(position) = closed_circle_point {
            let point_id = id_closed_point(e);
            let vertex_id = id_closed_vertex(e);
            annotations
                .note(&point_id, source_stream, 0)
                .tag("derived_closed_circle_seam");
            annotations.exactness(&point_id, Exactness::Derived);
            annotations
                .note(&vertex_id, source_stream, 0)
                .tag("derived_closed_circle_seam");
            annotations.exactness(&vertex_id, Exactness::Derived);
            out.points.push(Point {
                id: PointId(point_id.clone()),
                position,
                source_object: None,
            });
            out.vertices.push(Vertex {
                id: VertexId(vertex_id.clone()),
                point: PointId(point_id),
                tolerance: None,
            });
            (VertexId(vertex_id.clone()), VertexId(vertex_id))
        } else {
            (VertexId(id_vertex(start_v)), VertexId(id_vertex(end_v)))
        };
        if resolved_endpoints {
            let position = |vertex_use: u16| {
                let point_attr = t.vertex_uses.get(&vertex_use)?.refs.get(4)?;
                let [x, y, z] = t.points.get(point_attr)?.xyz_m?;
                Some(cadmpeg_ir::math::Point3::new(
                    x * LEN_TO_MM,
                    y * LEN_TO_MM,
                    z * LEN_TO_MM,
                ))
            };
            if let (Some(start), Some(end)) = (position(start_v), position(end_v)) {
                edge_endpoint_positions.insert(e, [start, end]);
            }
        }
        let parameter_range = carriers.curve(curve_attr).and_then(|carrier| {
            edge_parameter_range(carrier, edge_endpoint_positions.get(&e).copied())
        });
        if parameter_range.is_some_and(|(_, reversed)| reversed) {
            std::mem::swap(&mut start_id, &mut end_id);
            if let Some(endpoints) = edge_endpoint_positions.get_mut(&e) {
                endpoints.swap(0, 1);
            }
            reversed_edge_orientation.insert(e);
        }
        let eu = t.edge_uses.get(&e);
        let mut curve = None;
        if curve_attr != 0 {
            match carriers.curve(curve_attr).map(|c| &c.geometry) {
                Some(CarrierGeometry::Curve(_)) => {
                    if emitted_curves.insert(curve_attr) {
                        emit_curve(
                            &mut out,
                            carriers.curve(curve_attr).expect("matched curve carrier"),
                        );
                        if carriers.curve_is_derived(curve_attr) {
                            let offset = carriers
                                .curve(curve_attr)
                                .expect("matched curve carrier")
                                .offset;
                            annotations
                                .note(id_curve(curve_attr), source_stream, offset as u64)
                                .tag("surface_intersection");
                            annotations.exactness(id_curve(curve_attr), Exactness::Derived);
                        }
                    }
                    curve = Some(CurveId(id_curve(curve_attr)));
                }
                _ => {
                    if emitted_curves.insert(curve_attr) {
                        let offset = eu.map_or(0, |record| record.offset);
                        annotations
                            .note(id_curve(curve_attr), source_stream, offset as u64)
                            .tag("unknown_curve");
                        annotations.exactness(id_curve(curve_attr), Exactness::Unknown);
                        out.curves.push(Curve {
                            id: CurveId(id_curve(curve_attr)),
                            source_object: None,
                            geometry: CurveGeometry::Unknown { record: None },
                        });
                    }
                    curve = Some(CurveId(id_curve(curve_attr)));
                    out.stats.unknown_curve_edges += 1;
                }
            }
        }
        let off = eu.map_or(0, |r| r.offset);
        annotations
            .note(id_edge(e), source_stream, off as u64)
            .tag("00_10");
        out.edges.push(Edge {
            id: EdgeId(id_edge(e)),
            curve,
            start: start_id,
            end: end_id,
            param_range: parameter_range.map(|(range, _)| range),
            tolerance: None,
        });
    }
    let edge_set: HashSet<u16> = out
        .edges
        .iter()
        .map(|e| {
            e.id.0
                .rsplit('#')
                .next()
                .expect("invariant: id_edge always emits a '#'-separated suffix")
                .parse()
                .expect("invariant: id_edge suffix is the u16 attr formatted with {}")
        })
        .collect();

    // A loop is kept only when its whole ring resolves: every coedge exists and
    // its edge was emitted. A partial ring is dropped whole, so an emitted
    // coedge's `next`/`prev` never dangle and every emitted loop closes.
    let mut kept_loops: HashSet<u16> = HashSet::new();
    for f in &faces {
        for (loop_attr, ring) in &f.loops {
            let ok = !ring.is_empty()
                && ring.iter().all(|c| {
                    t.coedges
                        .get(c)
                        .is_some_and(|ce| edge_set.contains(ce.refs.get(6).unwrap_or(&0)))
                });
            if ok {
                kept_loops.insert(*loop_attr);
            }
        }
    }
    let emitted_coedges: HashSet<u16> = faces
        .iter()
        .flat_map(|f| f.loops.iter())
        .filter(|(la, _)| kept_loops.contains(la))
        .flat_map(|(_, ring)| ring.iter().copied())
        .collect();

    // Coedges of kept loops: `next`/`prev` from the ring order, partner from a
    // mutual twin that is itself emitted.
    for f in &faces {
        for (loop_attr, ring) in &f.loops {
            if !kept_loops.contains(loop_attr) {
                continue;
            }
            let k = ring.len();
            for (i, &ce_attr) in ring.iter().enumerate() {
                let ce = &t.coedges[&ce_attr];
                let edge_attr = *ce.refs.get(6).unwrap_or(&0);
                let next = ring[(i + 1) % k];
                let prev = ring[(i + k - 1) % k];
                let twin = *ce.refs.get(5).unwrap_or(&0);
                let partner = t
                    .coedges
                    .get(&twin)
                    .filter(|tw| tw.refs.get(5) == Some(&ce_attr))
                    .filter(|_| emitted_coedges.contains(&twin))
                    .map(|_| CoedgeId(id_coedge(twin)));
                annotations
                    .note(id_coedge(ce_attr), source_stream, ce.offset as u64)
                    .tag("00_11");
                let pcurves = edge_ends
                    .get(&edge_attr)
                    .and_then(|(_, _, curve_attr)| {
                        let support_data = carriers.intersection_support_data(*curve_attr)?;
                        let curve_carrier = carriers.curve(*curve_attr)?;
                        let CarrierGeometry::Curve(CurveGeometry::Nurbs(curve)) =
                            &curve_carrier.geometry
                        else {
                            return None;
                        };
                        let surface = carriers.surface(f.surface_attr)?;
                        let CarrierGeometry::Surface(surface) = &surface.geometry else {
                            return None;
                        };
                        let (geometry, parameter_range, source) = intersection_support_pcurve(
                            support_data,
                            curve,
                            f.surface_attr,
                            surface,
                            *edge_endpoint_positions.get(&edge_attr)?,
                        )?;
                        let id = PcurveId(format!("sldprt:brep:pcurve#intersection:{ce_attr}"));
                        let offset = curve_carrier.offset;
                        annotations
                            .note(&id, source_stream, offset as u64)
                            .tag(match source {
                                IntersectionPcurveSource::StoredCache => "surface_intersection_uv",
                                IntersectionPcurveSource::AnalyticInverse => {
                                    "derived_intersection_analytic_uv"
                                }
                                IntersectionPcurveSource::NurbsInverse => {
                                    "derived_intersection_nurbs_uv"
                                }
                            });
                        annotations.exactness(&id, Exactness::Derived);
                        out.pcurves.push(Pcurve {
                            id: id.clone(),
                            geometry,
                            wrapper_reversed: None,
                            native_tail_flags: None,
                            parameter_range: Some(parameter_range),
                            fit_tolerance: Some(support_data.fit_tolerance_mm),
                        });
                        Some(vec![cadmpeg_ir::topology::PcurveUse {
                            pcurve: id,
                            isoparametric: None,
                            parameter_range: Some(parameter_range),
                        }])
                    })
                    .unwrap_or_default();
                let mut sense = sense_of(ce.marker.unwrap_or(0x2b));
                if reversed_edge_orientation.contains(&edge_attr) {
                    sense = match sense {
                        Sense::Forward => Sense::Reversed,
                        Sense::Reversed => Sense::Forward,
                    };
                }
                out.coedges.push(Coedge {
                    id: CoedgeId(id_coedge(ce_attr)),
                    owner_loop: LoopId(id_loop(*loop_attr)),
                    edge: EdgeId(id_edge(edge_attr)),
                    next: CoedgeId(id_coedge(next)),
                    previous: CoedgeId(id_coedge(prev)),
                    radial_next: partner.unwrap_or_else(|| CoedgeId(id_coedge(ce_attr))),
                    sense,
                    use_curve: None,
                    use_curve_parameter_range: None,
                    pcurves,
                });
            }
        }
    }

    // Loops.
    for f in &faces {
        for (loop_attr, ring) in &f.loops {
            if !kept_loops.contains(loop_attr) {
                continue;
            }
            let coedges: Vec<CoedgeId> = ring.iter().map(|a| CoedgeId(id_coedge(*a))).collect();
            let off = t.loops.get(loop_attr).map_or(0, |r| r.offset);
            annotations
                .note(id_loop(*loop_attr), source_stream, off as u64)
                .tag("00_0f");
            out.loops.push(Loop {
                id: LoopId(id_loop(*loop_attr)),
                face: FaceId(id_face(f.bridge_attr)),
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                coedges,
                vertex_uses: Vec::new(),
            });
        }
    }
    let loop_set = kept_loops;

    // Surfaces + faces.
    let bind_bridges = |body_records: &[BodyRecord],
                        faces: &[WalkedFace]|
     -> (HashMap<u16, usize>, HashMap<u16, u16>) {
        let mut bridge_group = HashMap::new();
        let mut bridge_shell = HashMap::new();
        for (group, body_record) in body_records.iter().enumerate() {
            for face in faces {
                let owner = t.bridges.get(&face.bridge_attr).and_then(|r| r.owner);
                if body_record.refs.contains(&face.bridge_attr)
                    || owner.is_some_and(|owner| body_record.refs.contains(&owner))
                {
                    bridge_group.insert(face.bridge_attr, group);
                    if let Some(shell) = body_record
                        .regions
                        .iter()
                        .flat_map(|region| &region.shells)
                        .find(|shell| {
                            shell.refs.contains(&face.bridge_attr)
                                || owner.is_some_and(|owner| shell.refs.contains(&owner))
                        })
                    {
                        bridge_shell.insert(face.bridge_attr, shell.attr);
                    }
                }
            }
        }
        (bridge_group, bridge_shell)
    };
    let (bridge_group, bridge_shell) = bind_bridges(&body_records, &faces);
    if !body_records.is_empty() {
        out.stats.unclaimed_faces += faces
            .iter()
            .filter(|face| !bridge_group.contains_key(&face.bridge_attr))
            .count();
        faces.retain(|face| bridge_group.contains_key(&face.bridge_attr));
    }
    let mut face_edges_by_surface_carrier = HashMap::<u16, Vec<HashSet<u16>>>::new();
    for face in &faces {
        let edges = face
            .loops
            .iter()
            .flat_map(|(_, ring)| ring)
            .filter_map(|coedge| t.coedges.get(coedge))
            .filter_map(|coedge| coedge.refs.get(6).copied())
            .filter(|edge| *edge != 0)
            .collect();
        face_edges_by_surface_carrier
            .entry(face.surface_attr)
            .or_default()
            .push(edges);
    }
    let mut emitted_face_surface_by_carrier = HashMap::<u16, u16>::new();
    for face in &faces {
        if face
            .loops
            .iter()
            .any(|(loop_attr, _)| loop_set.contains(loop_attr))
        {
            emitted_face_surface_by_carrier
                .entry(face.surface_attr)
                .and_modify(|bridge| *bridge = (*bridge).min(face.bridge_attr))
                .or_insert(face.bridge_attr);
        }
    }
    for f in &faces {
        let loops: Vec<LoopId> = f
            .loops
            .iter()
            .filter(|(la, _)| loop_set.contains(la))
            .map(|(la, _)| LoopId(id_loop(*la)))
            .collect();
        if loops.is_empty() {
            continue;
        }
        // Support surface: a decoded surface carrier, else an opaque carrier.
        let surf_off = t.bridges.get(&f.bridge_attr).map_or(0, |r| r.offset);
        let mut surface_orientation_reversed = false;
        match carriers.surface(f.surface_attr).map(|c| (c, &c.geometry)) {
            Some((c, CarrierGeometry::Surface(geo))) => {
                surface_orientation_reversed = c.orientation_reversed;
                annotations
                    .note(id_surf(f.bridge_attr), source_stream, c.offset as u64)
                    .tag("compact_surface");
                let mut geometry = geo.clone();
                if let Some((_, u_reference, v_reference)) = c.frame {
                    fold_surface_frame(&mut geometry, u_reference, v_reference);
                    annotate_surface_frame(&mut annotations, &id_surf(f.bridge_attr), &geometry);
                }
                out.surfaces.push(Surface {
                    id: SurfaceId(id_surf(f.bridge_attr)),
                    source_object: None,
                    geometry,
                });
            }
            _ => {
                let resolved_offset = carriers.offset(f.surface_attr).and_then(|offset| {
                    if !support_is_acyclic(offset.support, carriers, &mut HashSet::new()) {
                        return None;
                    }
                    let support = ensure_surface_support(
                        offset.support,
                        carriers,
                        &emitted_face_surface_by_carrier,
                        &mut out,
                        &mut annotations,
                        source_stream,
                        &mut HashSet::new(),
                    )?;
                    Some((offset, support))
                });
                let resolved_blend = carriers.blend(f.surface_attr).and_then(|blend| {
                    let face_edges: HashSet<u16> = f
                        .loops
                        .iter()
                        .flat_map(|(_, ring)| ring)
                        .filter_map(|coedge| t.coedges.get(coedge))
                        .filter_map(|coedge| coedge.refs.get(6).copied())
                        .filter(|edge| *edge != 0)
                        .collect();
                    let [Some(first_attr), Some(second_attr)] =
                        blend.supports.map(|support| match support {
                            BlendSupportRef::Surface(attr) => Some(attr),
                            BlendSupportRef::Pair(attr) => {
                                let pair = carriers.blend_support_pair(attr)?;
                                carriers.curve(pair.intersection)?;
                                let mut adjacent = pair.supports.iter().filter_map(|candidate| {
                                    face_edges_by_surface_carrier
                                        .get(candidate)?
                                        .iter()
                                        .any(|edges| !face_edges.is_disjoint(edges))
                                        .then_some(*candidate)
                                });
                                let support = adjacent.next()?;
                                if adjacent.next().is_some() {
                                    return None;
                                }
                                Some(support)
                            }
                        })
                    else {
                        return None;
                    };
                    if ![first_attr, second_attr]
                        .into_iter()
                        .all(|attr| support_is_acyclic(attr, carriers, &mut HashSet::new()))
                    {
                        return None;
                    }
                    let first = ensure_surface_support(
                        first_attr,
                        carriers,
                        &emitted_face_surface_by_carrier,
                        &mut out,
                        &mut annotations,
                        source_stream,
                        &mut HashSet::new(),
                    )?;
                    let second = ensure_surface_support(
                        second_attr,
                        carriers,
                        &emitted_face_surface_by_carrier,
                        &mut out,
                        &mut annotations,
                        source_stream,
                        &mut HashSet::new(),
                    )?;
                    Some((blend, first, second))
                });
                if let Some((offset, support)) = resolved_offset {
                    let construction = ProceduralSurfaceId(format!(
                        "sldprt:brep:offset-construction#{}",
                        f.bridge_attr
                    ));
                    emit_offset_surface(
                        &mut out,
                        &mut annotations,
                        source_stream,
                        SurfaceId(id_surf(f.bridge_attr)),
                        construction,
                        support,
                        offset,
                    );
                } else if let Some((blend, first, second)) = resolved_blend {
                    let spine = carriers.curve(blend.spine).map(|carrier| {
                        if emitted_curves.insert(blend.spine) {
                            emit_curve(&mut out, carrier);
                            annotations
                                .note(id_curve(blend.spine), source_stream, carrier.offset as u64)
                                .tag("blend_spine");
                        }
                        CurveId(id_curve(blend.spine))
                    });
                    let procedural_id = ProceduralSurfaceId(format!(
                        "sldprt:brep:blend-construction#{}",
                        f.bridge_attr
                    ));
                    out.procedural_surfaces.push(ProceduralSurface {
                        id: procedural_id.clone(),
                        surface: SurfaceId(id_surf(f.bridge_attr)),
                        definition: ProceduralSurfaceDefinition::Blend {
                            supports: [
                                Some(BlendSupport {
                                    surface: first,
                                    reversed: blend.reversed[0],
                                }),
                                Some(BlendSupport {
                                    surface: second,
                                    reversed: blend.reversed[1],
                                }),
                            ],
                            spine,
                            radius: BlendRadiusLaw::Constant {
                                signed_radius: blend.signed_radius,
                            },
                            cross_section: BlendCrossSection::Circular,
                            native: None,
                        },
                        cache_fit_tolerance: None,
                        record_bounds: None,
                    });
                    annotations
                        .note(id_surf(f.bridge_attr), source_stream, blend.offset as u64)
                        .tag("00_38");
                    out.surfaces.push(Surface {
                        id: SurfaceId(id_surf(f.bridge_attr)),
                        source_object: None,
                        geometry: SurfaceGeometry::Procedural {
                            construction: procedural_id,
                        },
                    });
                } else if let Some((geometry, offset, tag, derived)) =
                    resolve_sweep_surface(carriers, t, f)
                {
                    annotations
                        .note(id_surf(f.bridge_attr), source_stream, offset as u64)
                        .tag(tag);
                    if derived {
                        annotations.exactness(id_surf(f.bridge_attr), Exactness::Derived);
                    }
                    out.surfaces.push(Surface {
                        id: SurfaceId(id_surf(f.bridge_attr)),
                        source_object: None,
                        geometry,
                    });
                } else {
                    out.stats.unknown_surface_faces += 1;
                    annotations
                        .note(id_surf(f.bridge_attr), source_stream, surf_off as u64)
                        .tag("unknown_surface");
                    annotations.exactness(id_surf(f.bridge_attr), Exactness::Unknown);
                    out.surfaces.push(Surface {
                        id: SurfaceId(id_surf(f.bridge_attr)),
                        source_object: None,
                        geometry: SurfaceGeometry::Unknown { record: None },
                    });
                }
            }
        }
        annotations
            .note(id_face(f.bridge_attr), source_stream, surf_off as u64)
            .tag("00_0e");
        out.faces.push(Face {
            id: FaceId(id_face(f.bridge_attr)),
            shell: ShellId(format!(
                "sldprt:brep:shell#{}",
                bridge_shell
                    .get(&f.bridge_attr)
                    .copied()
                    .or_else(|| bridge_group.get(&f.bridge_attr).copied().map(|v| v as u16))
                    .unwrap_or(0)
            )),
            surface: SurfaceId(id_surf(f.bridge_attr)),
            sense: surface_sense(f.marker, surface_orientation_reversed),
            loops,
            name: None,
            color: t
                .bridges
                .get(&f.bridge_attr)
                .and_then(|bridge| bridge.owner)
                .and_then(|owner| {
                    out.face_colors
                        .iter()
                        .find(|entry| entry.face_attr == owner)
                })
                .map(|entry| entry.color),
            tolerance: None,
        });
    }
    let emitted_faces = out
        .faces
        .iter()
        .map(|face| face.id.0.as_str())
        .collect::<HashSet<_>>();
    for appearance in &mut out.face_colors {
        appearance.target = faces
            .iter()
            .find(|face| {
                t.bridges
                    .get(&face.bridge_attr)
                    .and_then(|bridge| bridge.owner)
                    == Some(appearance.face_attr)
            })
            .map(|face| id_face(face.bridge_attr))
            .filter(|face| emitted_faces.contains(face.as_str()));
    }
    for atom in &mut out.face_atoms {
        atom.target =
            Some(id_face(atom.face_attr)).filter(|face| emitted_faces.contains(face.as_str()));
    }
    let mut bound_faces = HashSet::new();
    out.face_atoms
        .retain(|atom| atom.target.is_some() && bound_faces.insert(atom.face_attr));
    solve_face_orientation(&mut out);
    synthesize_cylinder_seams(&mut out, &mut annotations, source_stream);
    synthesize_sphere_seams(&mut out, &mut annotations, source_stream);
    derive_planar_pcurves(&mut out, &mut annotations, source_stream);
    derive_cylindrical_pcurves(&mut out, &mut annotations, source_stream);
    derive_revolved_circle_pcurves(&mut out, &mut annotations, source_stream);
    derive_spherical_pcurves(&mut out, &mut annotations, source_stream);
    derive_nurbs_isoparametric_pcurves(&mut out, &mut annotations, source_stream);
    prune_rejected_topology(&mut out);

    if out.faces.is_empty() {
        return Brep {
            stats: out.stats,
            ..Brep::default()
        };
    }
    out.stats.synthetic_body_grouping = body_records.is_empty();

    let group_count = body_records.len().max(1);
    for group in 0..group_count {
        let body_record = body_records.get(group);
        let body_id = body_record.map_or_else(
            || "sldprt:brep:body#0".to_string(),
            |r| format!("sldprt:brep:body#{}", r.attr),
        );
        let mut annotate_group = |id: &str, source: Option<(usize, &str)>| {
            let (offset, tag, exactness) = source.map_or(
                (0, "synthetic_grouping", Exactness::Derived),
                |(offset, tag)| (offset, tag, Exactness::ByteExact),
            );
            annotations.note(id, source_stream, offset as u64).tag(tag);
            annotations.exactness(id, exactness);
        };
        annotate_group(
            &body_id,
            body_record.map(|record| (record.offset, "00_51_body")),
        );
        let native_regions = body_record.map_or(&[][..], |record| record.regions.as_slice());
        let mut body_regions = Vec::new();
        if native_regions.is_empty() {
            let region_id = format!("sldprt:brep:region#{group}");
            let native_shell_id = format!("sldprt:brep:shell#{group}");
            annotate_group(&region_id, None);
            let mut region_shells = Vec::new();
            for (component, faces) in shell_face_components(&out, &native_shell_id)
                .into_iter()
                .enumerate()
            {
                let shell_id = if component == 0 {
                    native_shell_id.clone()
                } else {
                    format!("{native_shell_id}.component-{component}")
                };
                annotate_group(&shell_id, None);
                let face_ids = faces
                    .iter()
                    .map(|face| face.0.as_str())
                    .collect::<HashSet<_>>();
                for face in &mut out.faces {
                    if face_ids.contains(face.id.0.as_str()) {
                        face.shell = ShellId(shell_id.clone());
                    }
                }
                out.shells.push(Shell {
                    id: ShellId(shell_id.clone()),
                    region: RegionId(region_id.clone()),
                    faces,
                    wire_edges: Vec::new(),
                    free_vertices: Vec::new(),
                });
                region_shells.push(ShellId(shell_id));
            }
            out.regions.push(Region {
                id: RegionId(region_id.clone()),
                body: BodyId(body_id.clone()),
                shells: region_shells,
            });
            body_regions.push(RegionId(region_id));
        } else {
            for region in native_regions {
                let region_id = format!("sldprt:brep:region#{}", region.attr);
                annotate_group(&region_id, Some((region.offset, "00_51_region")));
                let mut region_shells = Vec::new();
                for shell in &region.shells {
                    let native_shell_id = format!("sldprt:brep:shell#{}", shell.attr);
                    for (component, faces) in shell_face_components(&out, &native_shell_id)
                        .into_iter()
                        .enumerate()
                    {
                        let shell_id = if component == 0 {
                            native_shell_id.clone()
                        } else {
                            format!("{native_shell_id}.component-{component}")
                        };
                        annotate_group(
                            &shell_id,
                            (component == 0).then_some((shell.offset, "00_51_shell")),
                        );
                        let face_ids = faces
                            .iter()
                            .map(|face| face.0.as_str())
                            .collect::<HashSet<_>>();
                        for face in &mut out.faces {
                            if face_ids.contains(face.id.0.as_str()) {
                                face.shell = ShellId(shell_id.clone());
                            }
                        }
                        out.shells.push(Shell {
                            id: ShellId(shell_id.clone()),
                            region: RegionId(region_id.clone()),
                            faces,
                            wire_edges: Vec::new(),
                            free_vertices: Vec::new(),
                        });
                        region_shells.push(ShellId(shell_id));
                    }
                }
                out.regions.push(Region {
                    id: RegionId(region_id.clone()),
                    body: BodyId(body_id.clone()),
                    shells: region_shells,
                });
                body_regions.push(RegionId(region_id));
            }
        }
        out.bodies.push(Body {
            id: BodyId(body_id),
            kind: body_record.map_or(BodyKind::Solid, |record| record.kind),
            regions: body_regions,
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
    }

    let mut body_ids_by_attr = HashMap::<u16, Option<String>>::new();
    for body in &out.bodies {
        let Some(attr) = body
            .id
            .0
            .strip_prefix("sldprt:brep:body#")
            .and_then(|value| value.parse::<u16>().ok())
        else {
            continue;
        };
        match body_ids_by_attr.entry(attr) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Some(body.id.0.clone()));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                *entry.get_mut() = None;
            }
        }
    }
    for modifier in &mut out.body_modifiers {
        modifier.target = body_ids_by_attr.get(&modifier.body_attr).cloned().flatten();
    }

    for curve in &out.curves {
        let Some(attr) = curve
            .id
            .0
            .strip_prefix("sldprt:brep:curve#")
            .and_then(|value| value.parse::<u16>().ok())
        else {
            continue;
        };
        if let Some(carrier) = carriers.curve(attr) {
            annotations
                .note(&curve.id, source_stream, carrier.offset as u64)
                .tag("compact_curve");
            if matches!(curve.geometry, CurveGeometry::Unknown { .. }) {
                annotations.exactness(&curve.id, Exactness::Unknown);
            }
        }
    }
    out.bodies.sort_by(|a, b| a.id.cmp(&b.id));
    out.regions.sort_by(|a, b| a.id.cmp(&b.id));
    out.shells.sort_by(|a, b| a.id.cmp(&b.id));
    out.faces.sort_by(|a, b| a.id.cmp(&b.id));
    out.loops.sort_by(|a, b| a.id.cmp(&b.id));
    out.coedges.sort_by(|a, b| a.id.cmp(&b.id));
    out.edges.sort_by(|a, b| a.id.cmp(&b.id));
    out.vertices.sort_by(|a, b| a.id.cmp(&b.id));
    out.points.sort_by(|a, b| a.id.cmp(&b.id));
    out.surfaces.sort_by(|a, b| a.id.cmp(&b.id));
    out.procedural_surfaces.sort_by(|a, b| a.id.cmp(&b.id));
    out.curves.sort_by(|a, b| a.id.cmp(&b.id));
    out.pcurves.sort_by(|a, b| a.id.cmp(&b.id));
    out.annotations = annotations.build();
    let retained_ids = out
        .bodies
        .iter()
        .map(|entity| entity.id.0.as_str())
        .chain(out.regions.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.shells.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.faces.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.loops.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.coedges.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.edges.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.vertices.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.points.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.surfaces.iter().map(|entity| entity.id.0.as_str()))
        .chain(
            out.procedural_surfaces
                .iter()
                .map(|entity| entity.id.0.as_str()),
        )
        .chain(out.curves.iter().map(|entity| entity.id.0.as_str()))
        .chain(out.pcurves.iter().map(|entity| entity.id.0.as_str()))
        .collect::<HashSet<_>>();
    out.annotations
        .provenance
        .retain(|id, _| retained_ids.contains(id.as_str()));
    out.annotations
        .exactness
        .retain(|id, _| retained_ids.contains(id.as_str()));
    out
}

fn prune_rejected_topology(out: &mut Brep) {
    let kept_loops = out
        .faces
        .iter()
        .flat_map(|face| &face.loops)
        .cloned()
        .collect::<HashSet<_>>();
    out.loops.retain(|loop_| kept_loops.contains(&loop_.id));

    let kept_coedges = out
        .loops
        .iter()
        .flat_map(|loop_| &loop_.coedges)
        .cloned()
        .collect::<HashSet<_>>();
    out.coedges
        .retain(|coedge| kept_coedges.contains(&coedge.id));
    for coedge in &mut out.coedges {
        if !kept_coedges.contains(&coedge.radial_next) {
            coedge.radial_next = coedge.id.clone();
        }
    }

    let kept_pcurves = out
        .coedges
        .iter()
        .flat_map(|coedge| &coedge.pcurves)
        .map(|use_| &use_.pcurve)
        .cloned()
        .collect::<HashSet<_>>();
    out.pcurves
        .retain(|pcurve| kept_pcurves.contains(&pcurve.id));

    let kept_edges = out
        .coedges
        .iter()
        .map(|coedge| coedge.edge.clone())
        .collect::<HashSet<_>>();
    out.edges.retain(|edge| kept_edges.contains(&edge.id));

    let kept_vertices = out
        .edges
        .iter()
        .flat_map(|edge| [&edge.start, &edge.end])
        .cloned()
        .collect::<HashSet<_>>();
    out.vertices
        .retain(|vertex| kept_vertices.contains(&vertex.id));

    let kept_points = out
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect::<HashSet<_>>();
    out.points.retain(|point| kept_points.contains(&point.id));

    let mut kept_curves = out
        .edges
        .iter()
        .filter_map(|edge| edge.curve.clone())
        .collect::<HashSet<_>>();
    kept_curves.extend(out.procedural_surfaces.iter().filter_map(|surface| {
        if let ProceduralSurfaceDefinition::Blend { spine, .. } = &surface.definition {
            spine.clone()
        } else {
            None
        }
    }));
    out.curves.retain(|curve| kept_curves.contains(&curve.id));
    out.stats.unknown_curve_edges = out
        .edges
        .iter()
        .filter(|edge| {
            edge.curve.as_ref().is_some_and(|curve_id| {
                out.curves.iter().any(|curve| {
                    curve.id == *curve_id && matches!(curve.geometry, CurveGeometry::Unknown { .. })
                })
            })
        })
        .count();
}

fn fold_surface_frame(
    mut geometry: &mut SurfaceGeometry,
    u_reference: cadmpeg_ir::math::Vector3,
    v_reference: cadmpeg_ir::math::Vector3,
) {
    loop {
        match geometry {
            SurfaceGeometry::Plane { u_axis, .. } => {
                *u_axis = u_reference;
                break;
            }
            SurfaceGeometry::Cylinder { ref_direction, .. }
            | SurfaceGeometry::Cone { ref_direction, .. }
            | SurfaceGeometry::Torus { ref_direction, .. } => {
                *ref_direction = u_reference;
                break;
            }
            SurfaceGeometry::Sphere {
                axis,
                ref_direction,
                ..
            } => {
                *axis = v_reference;
                *ref_direction = u_reference;
                break;
            }
            SurfaceGeometry::Transformed { basis, .. } => geometry = basis,
            SurfaceGeometry::Nurbs(_)
            | SurfaceGeometry::Procedural { .. }
            | SurfaceGeometry::Polygonal { .. }
            | SurfaceGeometry::Unknown { .. } => break,
        }
    }
}

fn annotate_surface_frame(
    annotations: &mut AnnotationBuilder,
    id: &str,
    mut geometry: &SurfaceGeometry,
) {
    loop {
        match geometry {
            SurfaceGeometry::Plane { .. } => {
                annotations.derived(id.to_owned(), "geometry.u_axis");
                break;
            }
            SurfaceGeometry::Cylinder { .. }
            | SurfaceGeometry::Cone { .. }
            | SurfaceGeometry::Torus { .. } => {
                annotations.derived(id.to_owned(), "geometry.ref_direction");
                break;
            }
            SurfaceGeometry::Sphere { .. } => {
                annotations
                    .derived(id, "geometry.axis")
                    .derived(id.to_owned(), "geometry.ref_direction");
                break;
            }
            SurfaceGeometry::Transformed { basis, .. } => geometry = basis,
            SurfaceGeometry::Nurbs(_)
            | SurfaceGeometry::Procedural { .. }
            | SurfaceGeometry::Polygonal { .. }
            | SurfaceGeometry::Unknown { .. } => break,
        }
    }
}

fn derive_planar_pcurves(
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
) {
    let loop_faces: HashMap<_, _> = out
        .loops
        .iter()
        .map(|lp| (lp.id.clone(), lp.face.clone()))
        .collect();
    let faces: HashMap<_, _> = out.faces.iter().map(|face| (&face.id, face)).collect();
    let surfaces: HashMap<_, _> = out
        .surfaces
        .iter()
        .map(|surface| (&surface.id, surface))
        .collect();
    let edges: HashMap<_, _> = out.edges.iter().map(|edge| (&edge.id, edge)).collect();
    let curves: HashMap<_, _> = out.curves.iter().map(|curve| (&curve.id, curve)).collect();
    let mut derived = Vec::new();
    for coedge in &out.coedges {
        let Some(face_id) = loop_faces.get(&coedge.owner_loop) else {
            continue;
        };
        let Some(face) = faces.get(face_id) else {
            continue;
        };
        let Some(surface) = surfaces.get(&face.surface) else {
            continue;
        };
        if !matches!(surface.geometry, SurfaceGeometry::Plane { .. }) {
            continue;
        }
        let SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis: u_reference,
        } = surface.geometry
        else {
            continue;
        };
        let v_reference = cadmpeg_ir::math::Vector3::new(
            normal.y * u_reference.z - normal.z * u_reference.y,
            normal.z * u_reference.x - normal.x * u_reference.z,
            normal.x * u_reference.y - normal.y * u_reference.x,
        );
        let Some(edge) = edges.get(&coedge.edge) else {
            continue;
        };
        let Some(curve) = edge.curve.as_ref().and_then(|id| curves.get(id).copied()) else {
            continue;
        };
        let uv = |point: cadmpeg_ir::math::Point3| {
            let d = [point.x - origin.x, point.y - origin.y, point.z - origin.z];
            cadmpeg_ir::math::Point2::new(
                d[0] * u_reference.x + d[1] * u_reference.y + d[2] * u_reference.z,
                d[0] * v_reference.x + d[1] * v_reference.y + d[2] * v_reference.z,
            )
        };
        let project_direction = |direction: cadmpeg_ir::math::Vector3| {
            let projected = cadmpeg_ir::math::Point2::new(
                direction.x * u_reference.x
                    + direction.y * u_reference.y
                    + direction.z * u_reference.z,
                direction.x * v_reference.x
                    + direction.y * v_reference.y
                    + direction.z * v_reference.z,
            );
            let norm = (projected.u * projected.u + projected.v * projected.v).sqrt();
            (norm > 1e-12)
                .then(|| cadmpeg_ir::math::Point2::new(projected.u / norm, projected.v / norm))
        };
        let plane_distance = |point: cadmpeg_ir::math::Point3| {
            (point.x - origin.x) * normal.x
                + (point.y - origin.y) * normal.y
                + (point.z - origin.z) * normal.z
        };
        let geometry = match &curve.geometry {
            CurveGeometry::Line {
                origin: curve_origin,
                direction,
            } => {
                if plane_distance(*curve_origin).abs() > 1e-6
                    || (direction.x * normal.x + direction.y * normal.y + direction.z * normal.z)
                        .abs()
                        > 1e-9
                {
                    continue;
                }
                let Some(direction) = project_direction(*direction) else {
                    continue;
                };
                PcurveGeometry::Line {
                    origin: uv(*curve_origin),
                    direction,
                }
            }
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                let axis_dot = axis.x * normal.x + axis.y * normal.y + axis.z * normal.z;
                if axis_dot.abs() < 1.0 - 1e-9
                    || plane_distance(*center).abs() > 1e-6
                    || *radius <= 0.0
                {
                    continue;
                }
                let Some(ref_direction) = project_direction(*ref_direction) else {
                    continue;
                };
                PcurveGeometry::Circle {
                    center: uv(*center),
                    x_axis: ref_direction,
                    y_axis: if axis_dot < 0.0 {
                        cadmpeg_ir::math::Point2::new(ref_direction.v, -ref_direction.u)
                    } else {
                        cadmpeg_ir::math::Point2::new(-ref_direction.v, ref_direction.u)
                    },
                    radius: *radius,
                }
            }
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } => {
                let axis_dot = axis.x * normal.x + axis.y * normal.y + axis.z * normal.z;
                if axis_dot.abs() < 1.0 - 1e-9
                    || plane_distance(*center).abs() > 1e-6
                    || *major_radius <= 0.0
                    || *minor_radius <= 0.0
                {
                    continue;
                }
                let Some(major_direction) = project_direction(*major_direction) else {
                    continue;
                };
                PcurveGeometry::Ellipse {
                    center: uv(*center),
                    x_axis: major_direction,
                    y_axis: if axis_dot < 0.0 {
                        cadmpeg_ir::math::Point2::new(major_direction.v, -major_direction.u)
                    } else {
                        cadmpeg_ir::math::Point2::new(-major_direction.v, major_direction.u)
                    },
                    major_radius: *major_radius,
                    minor_radius: *minor_radius,
                }
            }
            _ => continue,
        };
        let id = PcurveId(format!(
            "sldprt:brep:pcurve#{}",
            coedge.id.0.rsplit('#').next().unwrap_or("0")
        ));
        let pcurve = Pcurve {
            id: id.clone(),
            geometry,
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: None,
            fit_tolerance: None,
        };
        derived.push((coedge.id.clone(), id, pcurve));
    }
    let coedge_indices = out
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| (coedge.id.clone(), index))
        .collect::<HashMap<_, _>>();
    for (coedge_id, id, pcurve) in derived {
        if let Some(index) = coedge_indices.get(&coedge_id) {
            out.coedges[*index].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
                pcurve: id.clone(),
                isoparametric: None,
                parameter_range: None,
            }];
        }
        annotations
            .note(&id, source_stream, 0)
            .tag("derived_planar_pcurve");
        annotations.exactness(&id, Exactness::Derived);
        out.pcurves.push(pcurve);
    }
}

fn derive_cylindrical_pcurves(
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
) {
    let loop_faces: HashMap<_, _> = out
        .loops
        .iter()
        .map(|lp| (lp.id.clone(), lp.face.clone()))
        .collect();
    let faces: HashMap<_, _> = out.faces.iter().map(|face| (&face.id, face)).collect();
    let surfaces: HashMap<_, _> = out
        .surfaces
        .iter()
        .map(|surface| (&surface.id, surface))
        .collect();
    let edges: HashMap<_, _> = out.edges.iter().map(|edge| (&edge.id, edge)).collect();
    let curves: HashMap<_, _> = out.curves.iter().map(|curve| (&curve.id, curve)).collect();
    let points: HashMap<_, _> = out.points.iter().map(|point| (&point.id, point)).collect();
    let vertex_points: HashMap<_, _> = out
        .vertices
        .iter()
        .filter_map(|vertex| points.get(&vertex.point).map(|point| (&vertex.id, *point)))
        .collect();
    let position = |vertex_id: &VertexId| vertex_points.get(vertex_id).map(|point| point.position);
    let mut derived = Vec::new();
    for coedge in &out.coedges {
        if !coedge.pcurves.is_empty() {
            continue;
        }
        let Some(face_id) = loop_faces.get(&coedge.owner_loop) else {
            continue;
        };
        let Some(face) = faces.get(face_id) else {
            continue;
        };
        let Some(surface) = surfaces.get(&face.surface) else {
            continue;
        };
        let SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction: u_reference,
            radius,
        } = &surface.geometry
        else {
            continue;
        };
        let Some(edge) = edges.get(&coedge.edge) else {
            continue;
        };
        let Some(curve) = edge.curve.as_ref().and_then(|id| curves.get(id).copied()) else {
            continue;
        };
        let cross = cadmpeg_ir::math::Vector3::new(
            axis.y * u_reference.z - axis.z * u_reference.y,
            axis.z * u_reference.x - axis.x * u_reference.z,
            axis.x * u_reference.y - axis.y * u_reference.x,
        );
        let dot = |a: [f64; 3], b: cadmpeg_ir::math::Vector3| a[0] * b.x + a[1] * b.y + a[2] * b.z;
        let mut parameter_range = None;
        let geometry = match &curve.geometry {
            CurveGeometry::Circle {
                center,
                axis: circle_axis,
                ref_direction: circle_reference,
                radius: circle_radius,
            } if (circle_radius.abs() - radius.abs()).abs() < 1e-6
                && (circle_axis.x * axis.x + circle_axis.y * axis.y + circle_axis.z * axis.z)
                    .abs()
                    > 1.0 - 1e-9 =>
            {
                let d = [
                    center.x - origin.x,
                    center.y - origin.y,
                    center.z - origin.z,
                ];
                let axial = dot(d, *axis);
                let radial = [
                    d[0] - axial * axis.x,
                    d[1] - axial * axis.y,
                    d[2] - axial * axis.z,
                ];
                if dot(
                    radial,
                    cadmpeg_ir::math::Vector3::new(radial[0], radial[1], radial[2]),
                )
                .sqrt()
                    > 1e-6
                {
                    continue;
                }
                let Some((phase, sense)) =
                    circle_azimuth_parameter(*axis, *u_reference, *circle_axis, *circle_reference)
                else {
                    continue;
                };
                PcurveGeometry::Line {
                    origin: cadmpeg_ir::math::Point2::new(phase, axial),
                    direction: cadmpeg_ir::math::Point2::new(sense, 0.0),
                }
            }
            CurveGeometry::Line { direction, .. }
                if (direction.x * axis.x + direction.y * axis.y + direction.z * axis.z).abs()
                    > 1.0 - 1e-9 =>
            {
                let Some(start) = position(&edge.start) else {
                    continue;
                };
                let d = [start.x - origin.x, start.y - origin.y, start.z - origin.z];
                let v = dot(d, *axis);
                let radial = [d[0] - v * axis.x, d[1] - v * axis.y, d[2] - v * axis.z];
                let u = dot(radial, cross).atan2(dot(radial, *u_reference));
                PcurveGeometry::Line {
                    origin: cadmpeg_ir::math::Point2::new(u, v),
                    direction: cadmpeg_ir::math::Point2::new(
                        0.0,
                        if dot([direction.x, direction.y, direction.z], *axis) >= 0.0 {
                            1.0
                        } else {
                            -1.0
                        },
                    ),
                }
            }
            CurveGeometry::Ellipse {
                center,
                axis: ellipse_axis,
                major_direction,
                major_radius,
                minor_radius,
            } => {
                let minor_direction = cadmpeg_ir::math::Vector3::new(
                    ellipse_axis.y * major_direction.z - ellipse_axis.z * major_direction.y,
                    ellipse_axis.z * major_direction.x - ellipse_axis.x * major_direction.z,
                    ellipse_axis.x * major_direction.y - ellipse_axis.y * major_direction.x,
                );
                let relative = [
                    center.x - origin.x,
                    center.y - origin.y,
                    center.z - origin.z,
                ];
                let major = [
                    major_radius * major_direction.x,
                    major_radius * major_direction.y,
                    major_radius * major_direction.z,
                ];
                let minor = [
                    minor_radius * minor_direction.x,
                    minor_radius * minor_direction.y,
                    minor_radius * minor_direction.z,
                ];
                let radial_center = cadmpeg_ir::math::Point2::new(
                    dot(relative, *u_reference),
                    dot(relative, cross),
                );
                let radial_cos =
                    cadmpeg_ir::math::Point2::new(dot(major, *u_reference), dot(major, cross));
                let radial_sin =
                    cadmpeg_ir::math::Point2::new(dot(minor, *u_reference), dot(minor, cross));
                let norm = |value: cadmpeg_ir::math::Point2| value.u.hypot(value.v);
                let product = |a: cadmpeg_ir::math::Point2, b: cadmpeg_ir::math::Point2| {
                    a.u * b.u + a.v * b.v
                };
                let tolerance = 1e-6_f64.max(radius.abs() * 1e-9);
                if norm(radial_center) > tolerance
                    || (norm(radial_cos) - radius.abs()).abs() > tolerance
                    || (norm(radial_sin) - radius.abs()).abs() > tolerance
                    || product(radial_cos, radial_sin).abs() > tolerance * radius.abs()
                {
                    continue;
                }
                PcurveGeometry::PolarHarmonic {
                    radial_center,
                    radial_cos,
                    radial_sin,
                    axial_origin: dot(relative, *axis),
                    axial_cos: dot(major, *axis),
                    axial_sin: dot(minor, *axis),
                }
            }
            CurveGeometry::Nurbs(nurbs) => {
                let radial_control_points = nurbs
                    .control_points
                    .iter()
                    .map(|point| {
                        let relative = [point.x - origin.x, point.y - origin.y, point.z - origin.z];
                        cadmpeg_ir::math::Point2::new(
                            dot(relative, *u_reference),
                            dot(relative, cross),
                        )
                    })
                    .collect::<Vec<_>>();
                if !quadratic_nurbs_has_constant_radius(
                    &radial_control_points,
                    nurbs.weights.as_deref(),
                    &nurbs.knots,
                    radius.abs(),
                ) {
                    continue;
                }
                parameter_range = if let Some(range) = edge.param_range {
                    Some(range)
                } else {
                    let (Some(start), Some(end)) = (position(&edge.start), position(&edge.end))
                    else {
                        continue;
                    };
                    match (
                        nurbs_parameter_at_point(nurbs, start),
                        nurbs_parameter_at_point(nurbs, end),
                    ) {
                        (InverseResolution::Unique(start), InverseResolution::Unique(end)) => {
                            Some([start.min(end), start.max(end)])
                        }
                        (InverseResolution::Ambiguous, _) | (_, InverseResolution::Ambiguous) => {
                            out.stats.ambiguous_pcurve_parameters += 1;
                            continue;
                        }
                        _ => continue,
                    }
                };
                let axial_control_points = nurbs
                    .control_points
                    .iter()
                    .map(|point| {
                        dot(
                            [point.x - origin.x, point.y - origin.y, point.z - origin.z],
                            *axis,
                        )
                    })
                    .collect();
                PcurveGeometry::PolarNurbs {
                    degree: nurbs.degree,
                    knots: nurbs.knots.clone(),
                    radial_control_points,
                    axial_control_points,
                    weights: nurbs.weights.clone(),
                    periodic: nurbs.periodic,
                }
            }
            _ => continue,
        };
        let id = PcurveId(format!(
            "sldprt:brep:pcurve#cylinder:{}",
            coedge.id.0.rsplit('#').next().unwrap_or("0")
        ));
        derived.push((
            coedge.id.clone(),
            id.clone(),
            Pcurve {
                id,
                geometry,
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range,
                fit_tolerance: None,
            },
        ));
    }
    let coedge_indices = out
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| (coedge.id.clone(), index))
        .collect::<HashMap<_, _>>();
    for (coedge_id, id, pcurve) in derived {
        if let Some(index) = coedge_indices.get(&coedge_id) {
            out.coedges[*index].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
                pcurve: id.clone(),
                isoparametric: None,
                parameter_range: None,
            }];
        }
        annotations
            .note(&id, source_stream, 0)
            .tag("derived_cylindrical_pcurve");
        annotations.exactness(&id, Exactness::Derived);
        out.pcurves.push(pcurve);
    }
}

enum InverseResolution<T> {
    NoMatch,
    Unique(T),
    Ambiguous,
}

const INVERSE_SAMPLE_COUNT: usize = 32;
const INVERSE_PARAMETER_TOLERANCE: f64 = 1.0e-10;
const INVERSE_ABSOLUTE_TOLERANCE_MM: f64 = 1.0e-6;
const INVERSE_RELATIVE_TOLERANCE: f64 = 8.0 * f64::EPSILON;
const NURBS_POLE_ROUNDOFF_FACTOR: f64 = 256.0 * f64::EPSILON;
const NURBS_CACHE_SAMPLES_PER_SPAN: usize = 8;
const NURBS_ENDPOINT_TOLERANCE_MM: f64 = 1.0e-6;

fn inverse_coordinate_tolerance(points: impl IntoIterator<Item = cadmpeg_ir::math::Point3>) -> f64 {
    let scale = points.into_iter().fold(1.0_f64, |scale, point| {
        scale
            .max(point.x.abs())
            .max(point.y.abs())
            .max(point.z.abs())
    });
    INVERSE_ABSOLUTE_TOLERANCE_MM.max(scale * INVERSE_RELATIVE_TOLERANCE)
}

fn golden_section_minimum<F>(mut left: f64, mut right: f64, objective: &mut F) -> Option<(f64, f64)>
where
    F: FnMut(f64) -> Option<f64>,
{
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    let mut a = right - ratio * (right - left);
    let mut b = left + ratio * (right - left);
    let mut da = objective(a)?;
    let mut db = objective(b)?;
    for _ in 0..80 {
        if da <= db {
            right = b;
            b = a;
            db = da;
            a = right - ratio * (right - left);
            da = objective(a)?;
        } else {
            left = a;
            a = b;
            da = db;
            b = left + ratio * (right - left);
            db = objective(b)?;
        }
    }
    let parameter = (left + right) * 0.5;
    Some((parameter, objective(parameter)?))
}

/// Find one representative for every sampled local minimum of an objective on
/// each nonzero knot span. Endpoints are always retained, so a candidate at a
/// knot is not lost when adjacent spans share it.
fn sampled_parameter_minima<F>(
    knots: &[f64],
    domain: [f64; 2],
    mut objective: F,
) -> Option<Vec<(f64, f64)>>
where
    F: FnMut(f64) -> Option<f64>,
{
    let mut candidates = Vec::new();
    for span in knots.windows(2).filter(|span| span[0] < span[1]) {
        let start = span[0].max(domain[0]);
        let end = span[1].min(domain[1]);
        if start >= end {
            continue;
        }
        let step = (end - start) / INVERSE_SAMPLE_COUNT as f64;
        let mut samples = Vec::with_capacity(INVERSE_SAMPLE_COUNT + 1);
        for index in 0..=INVERSE_SAMPLE_COUNT {
            let parameter = if index == INVERSE_SAMPLE_COUNT {
                end
            } else {
                start + step * index as f64
            };
            samples.push((parameter, objective(parameter)?));
        }
        candidates.extend(samples.iter().copied());
        for index in 1..INVERSE_SAMPLE_COUNT {
            if samples[index].1 <= samples[index - 1].1 && samples[index].1 <= samples[index + 1].1
            {
                candidates.push(golden_section_minimum(
                    samples[index - 1].0,
                    samples[index + 1].0,
                    &mut objective,
                )?);
            }
        }
    }
    Some(candidates)
}

fn unique_inverse_parameter(
    mut candidates: Vec<(f64, f64)>,
    tolerance: f64,
    parameter_domain: [f64; 2],
) -> InverseResolution<f64> {
    let tolerance_squared = tolerance * tolerance;
    candidates.retain(|(parameter, error)| {
        parameter.is_finite() && error.is_finite() && *error <= tolerance_squared
    });
    candidates.sort_by(|left, right| left.0.total_cmp(&right.0));
    let parameter_tolerance =
        INVERSE_PARAMETER_TOLERANCE * (1.0 + (parameter_domain[1] - parameter_domain[0]).abs());
    let mut unique = Vec::<(f64, f64)>::new();
    for candidate in candidates {
        if let Some(previous) = unique.last_mut() {
            if (candidate.0 - previous.0).abs() <= parameter_tolerance {
                if candidate.1 < previous.1 {
                    *previous = candidate;
                }
                continue;
            }
        }
        unique.push(candidate);
    }
    match unique.as_slice() {
        [] => InverseResolution::NoMatch,
        [(parameter, _)] => InverseResolution::Unique(*parameter),
        _ => InverseResolution::Ambiguous,
    }
}

fn nurbs_parameter_at_point(
    nurbs: &cadmpeg_ir::geometry::NurbsCurve,
    target: cadmpeg_ir::math::Point3,
) -> InverseResolution<f64> {
    let squared_distance = |parameter: f64| {
        let point = nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            parameter,
        )?;
        Some(
            (point.x - target.x).powi(2)
                + (point.y - target.y).powi(2)
                + (point.z - target.z).powi(2),
        )
    };
    let Some(domain) = nurbs_curve_parameter_domain(nurbs) else {
        return InverseResolution::NoMatch;
    };
    let Some(candidates) = sampled_parameter_minima(&nurbs.knots, domain, squared_distance) else {
        return InverseResolution::NoMatch;
    };
    unique_inverse_parameter(
        candidates,
        inverse_coordinate_tolerance(
            nurbs
                .control_points
                .iter()
                .copied()
                .chain(std::iter::once(target)),
        ),
        domain,
    )
}

fn quadratic_nurbs_has_constant_radius(
    radial_control_points: &[cadmpeg_ir::math::Point2],
    weights: Option<&[f64]>,
    knots: &[f64],
    radius: f64,
) -> bool {
    if radial_control_points.len() < 3
        || radial_control_points.len().is_multiple_of(2)
        || knots.len() != radial_control_points.len() + 3
        || weights.is_some_and(|weights| weights.len() != radial_control_points.len())
        || !radius.is_finite()
        || radius <= 0.0
    {
        return false;
    }
    let mut runs = Vec::new();
    for knot in knots {
        if !knot.is_finite() {
            return false;
        }
        if let Some((value, count)) = runs.last_mut() {
            if *value == *knot {
                *count += 1;
                continue;
            }
            if *knot <= *value {
                return false;
            }
        }
        runs.push((*knot, 1usize));
    }
    if runs.len() < 2
        || runs.first().is_none_or(|(_, count)| *count != 3)
        || runs.last().is_none_or(|(_, count)| *count != 3)
        || runs[1..runs.len() - 1].iter().any(|(_, count)| *count != 2)
        || runs.len() - 1 != (radial_control_points.len() - 1) / 2
    {
        return false;
    }
    let weight = |index: usize| weights.map_or(1.0, |weights| weights[index]);
    let choose_2 = [1.0, 2.0, 1.0];
    let choose_4 = [1.0, 4.0, 6.0, 4.0, 1.0];
    let tolerance = 1e-6_f64.max(radius * radius * 1e-9);
    for start in (0..radial_control_points.len() - 1).step_by(2) {
        let homogeneous = (0..3)
            .map(|offset| {
                let weight = weight(start + offset);
                let point = radial_control_points[start + offset];
                (point.u * weight, point.v * weight, weight)
            })
            .collect::<Vec<_>>();
        for (degree, &denominator) in choose_4.iter().enumerate() {
            let mut identity = 0.0_f64;
            for i in 0usize..=2 {
                let Some(j) = degree.checked_sub(i) else {
                    continue;
                };
                if j > 2 {
                    continue;
                }
                let factor = choose_2[i] * choose_2[j] / denominator;
                identity += factor
                    * (homogeneous[i].0 * homogeneous[j].0 + homogeneous[i].1 * homogeneous[j].1
                        - radius * radius * homogeneous[i].2 * homogeneous[j].2);
            }
            if identity.abs() > tolerance {
                return false;
            }
        }
    }
    true
}

fn circle_azimuth_parameter(
    surface_axis: cadmpeg_ir::math::Vector3,
    surface_reference: cadmpeg_ir::math::Vector3,
    circle_axis: cadmpeg_ir::math::Vector3,
    circle_reference: cadmpeg_ir::math::Vector3,
) -> Option<(f64, f64)> {
    let axis_dot = surface_axis.x * circle_axis.x
        + surface_axis.y * circle_axis.y
        + surface_axis.z * circle_axis.z;
    if axis_dot.abs() < 1.0 - 1e-9 {
        return None;
    }
    let surface_tangent = cadmpeg_ir::math::Vector3::new(
        surface_axis.y * surface_reference.z - surface_axis.z * surface_reference.y,
        surface_axis.z * surface_reference.x - surface_axis.x * surface_reference.z,
        surface_axis.x * surface_reference.y - surface_axis.y * surface_reference.x,
    );
    let phase = (circle_reference.x * surface_tangent.x
        + circle_reference.y * surface_tangent.y
        + circle_reference.z * surface_tangent.z)
        .atan2(
            circle_reference.x * surface_reference.x
                + circle_reference.y * surface_reference.y
                + circle_reference.z * surface_reference.z,
        );
    Some((phase, axis_dot.signum()))
}

fn derive_revolved_circle_pcurves(
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
) {
    let loop_faces: HashMap<_, _> = out
        .loops
        .iter()
        .map(|lp| (lp.id.clone(), lp.face.clone()))
        .collect();
    let faces: HashMap<_, _> = out.faces.iter().map(|face| (&face.id, face)).collect();
    let surfaces: HashMap<_, _> = out
        .surfaces
        .iter()
        .map(|surface| (&surface.id, surface))
        .collect();
    let edges: HashMap<_, _> = out.edges.iter().map(|edge| (&edge.id, edge)).collect();
    let curves: HashMap<_, _> = out.curves.iter().map(|curve| (&curve.id, curve)).collect();
    let dot = |a: [f64; 3], b: cadmpeg_ir::math::Vector3| a[0] * b.x + a[1] * b.y + a[2] * b.z;
    let mut derived = Vec::new();
    for coedge in &out.coedges {
        if !coedge.pcurves.is_empty() {
            continue;
        }
        let Some(surface) = loop_faces
            .get(&coedge.owner_loop)
            .and_then(|face_id| faces.get(face_id))
            .and_then(|face| surfaces.get(&face.surface))
        else {
            continue;
        };
        let Some(CurveGeometry::Circle {
            center: circle_center,
            axis: circle_axis,
            ref_direction: circle_reference,
            radius: circle_radius,
        }) = edges
            .get(&coedge.edge)
            .and_then(|edge| edge.curve.as_ref())
            .and_then(|curve_id| curves.get(curve_id))
            .map(|curve| &curve.geometry)
        else {
            continue;
        };
        let (surface_axis, surface_reference, v) = match &surface.geometry {
            SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction,
                radius,
                ratio,
                half_angle,
            } if (*ratio - 1.0).abs() < 1e-12 => {
                let d = [
                    circle_center.x - origin.x,
                    circle_center.y - origin.y,
                    circle_center.z - origin.z,
                ];
                let v = dot(d, *axis);
                let radial = [d[0] - v * axis.x, d[1] - v * axis.y, d[2] - v * axis.z];
                let expected_radius = radius + v * half_angle.tan();
                if dot(
                    radial,
                    cadmpeg_ir::math::Vector3::new(radial[0], radial[1], radial[2]),
                )
                .sqrt()
                    > 1e-6
                    || (circle_radius.abs() - expected_radius.abs()).abs() > 1e-6
                {
                    continue;
                }
                (*axis, *ref_direction, v)
            }
            SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            } => {
                let d = [
                    circle_center.x - center.x,
                    circle_center.y - center.y,
                    circle_center.z - center.z,
                ];
                let height = dot(d, *axis);
                let radial = [
                    d[0] - height * axis.x,
                    d[1] - height * axis.y,
                    d[2] - height * axis.z,
                ];
                if dot(
                    radial,
                    cadmpeg_ir::math::Vector3::new(radial[0], radial[1], radial[2]),
                )
                .sqrt()
                    > 1e-6
                    || ((circle_radius.abs() - major_radius).hypot(height) - minor_radius.abs())
                        .abs()
                        > 1e-6_f64.max(minor_radius.abs() * 1e-9)
                {
                    continue;
                }
                (
                    *axis,
                    *ref_direction,
                    height.atan2(circle_radius.abs() - major_radius),
                )
            }
            _ => continue,
        };
        let Some((phase, sense)) = circle_azimuth_parameter(
            surface_axis,
            surface_reference,
            *circle_axis,
            *circle_reference,
        ) else {
            continue;
        };
        let id = PcurveId(format!(
            "sldprt:brep:pcurve#revolved-circle:{}",
            coedge.id.0.rsplit('#').next().unwrap_or("0")
        ));
        derived.push((
            coedge.id.clone(),
            id.clone(),
            Pcurve {
                id,
                geometry: PcurveGeometry::Line {
                    origin: cadmpeg_ir::math::Point2::new(phase, v),
                    direction: cadmpeg_ir::math::Point2::new(sense, 0.0),
                },
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range: None,
                fit_tolerance: None,
            },
        ));
    }
    let coedge_indices = out
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| (coedge.id.clone(), index))
        .collect::<HashMap<_, _>>();
    for (coedge_id, id, pcurve) in derived {
        if let Some(index) = coedge_indices.get(&coedge_id) {
            out.coedges[*index].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
                pcurve: id.clone(),
                isoparametric: None,
                parameter_range: None,
            }];
        }
        annotations
            .note(&id, source_stream, 0)
            .tag("derived_revolved_circle_pcurve");
        annotations.exactness(&id, Exactness::Derived);
        out.pcurves.push(pcurve);
    }
}

fn derive_spherical_pcurves(
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
) {
    let loop_faces: HashMap<_, _> = out
        .loops
        .iter()
        .map(|lp| (lp.id.clone(), lp.face.clone()))
        .collect();
    let faces: HashMap<_, _> = out.faces.iter().map(|face| (&face.id, face)).collect();
    let surfaces: HashMap<_, _> = out
        .surfaces
        .iter()
        .map(|surface| (&surface.id, surface))
        .collect();
    let edges: HashMap<_, _> = out.edges.iter().map(|edge| (&edge.id, edge)).collect();
    let curves: HashMap<_, _> = out.curves.iter().map(|curve| (&curve.id, curve)).collect();
    let mut derived = Vec::new();
    for coedge in &out.coedges {
        if !coedge.pcurves.is_empty() {
            continue;
        }
        let Some(face_id) = loop_faces.get(&coedge.owner_loop) else {
            continue;
        };
        let Some(face) = faces.get(face_id) else {
            continue;
        };
        let Some(surface) = surfaces.get(&face.surface) else {
            continue;
        };
        let SurfaceGeometry::Sphere {
            center: sphere_center,
            axis: v_reference,
            ref_direction: u_reference,
            radius,
        } = &surface.geometry
        else {
            continue;
        };
        let Some(edge) = edges.get(&coedge.edge) else {
            continue;
        };
        let Some(CurveGeometry::Circle {
            center,
            axis,
            radius: circle_radius,
            ..
        }) = edge
            .curve
            .as_ref()
            .and_then(|id| curves.get(id).copied())
            .map(|curve| &curve.geometry)
        else {
            continue;
        };
        let axis_dot = axis.x * v_reference.x + axis.y * v_reference.y + axis.z * v_reference.z;
        let geometry = if axis_dot.abs() > 1.0 - 1e-9 {
            let d = [
                center.x - sphere_center.x,
                center.y - sphere_center.y,
                center.z - sphere_center.z,
            ];
            let height = d[0] * v_reference.x + d[1] * v_reference.y + d[2] * v_reference.z;
            if ((radius * radius - height * height).max(0.0).sqrt() - circle_radius.abs()).abs()
                > 1e-6
            {
                continue;
            }
            PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(
                    0.0,
                    (height / radius).clamp(-1.0, 1.0).asin(),
                ),
                direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
            }
        } else if axis_dot.abs() < 1e-9 && (circle_radius.abs() - radius.abs()).abs() < 1e-6 {
            let equator = cadmpeg_ir::math::Vector3::new(
                axis.y * v_reference.z - axis.z * v_reference.y,
                axis.z * v_reference.x - axis.x * v_reference.z,
                axis.x * v_reference.y - axis.y * v_reference.x,
            );
            let tangent = cadmpeg_ir::math::Vector3::new(
                v_reference.y * u_reference.z - v_reference.z * u_reference.y,
                v_reference.z * u_reference.x - v_reference.x * u_reference.z,
                v_reference.x * u_reference.y - v_reference.y * u_reference.x,
            );
            let u = (equator.x * tangent.x + equator.y * tangent.y + equator.z * tangent.z).atan2(
                equator.x * u_reference.x + equator.y * u_reference.y + equator.z * u_reference.z,
            );
            PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(u, 0.0),
                direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
            }
        } else {
            continue;
        };
        let id = PcurveId(format!(
            "sldprt:brep:pcurve#sphere:{}",
            coedge.id.0.rsplit('#').next().unwrap_or("0")
        ));
        derived.push((
            coedge.id.clone(),
            id.clone(),
            Pcurve {
                id,
                geometry,
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range: None,
                fit_tolerance: None,
            },
        ));
    }
    let coedge_indices = out
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| (coedge.id.clone(), index))
        .collect::<HashMap<_, _>>();
    for (coedge_id, id, pcurve) in derived {
        if let Some(index) = coedge_indices.get(&coedge_id) {
            out.coedges[*index].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
                pcurve: id.clone(),
                isoparametric: None,
                parameter_range: None,
            }];
        }
        annotations
            .note(&id, source_stream, 0)
            .tag("derived_spherical_pcurve");
        annotations.exactness(&id, Exactness::Derived);
        out.pcurves.push(pcurve);
    }
}

fn derive_nurbs_isoparametric_pcurves(
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
) {
    let loop_faces: HashMap<_, _> = out
        .loops
        .iter()
        .map(|lp| (lp.id.clone(), lp.face.clone()))
        .collect();
    let faces: HashMap<_, _> = out.faces.iter().map(|face| (&face.id, face)).collect();
    let surfaces: HashMap<_, _> = out
        .surfaces
        .iter()
        .map(|surface| (&surface.id, surface))
        .collect();
    let edges: HashMap<_, _> = out.edges.iter().map(|edge| (&edge.id, edge)).collect();
    let curves: HashMap<_, _> = out.curves.iter().map(|curve| (&curve.id, curve)).collect();
    let vertices: HashMap<_, _> = out
        .vertices
        .iter()
        .map(|vertex| (&vertex.id, vertex))
        .collect();
    let points: HashMap<_, _> = out.points.iter().map(|point| (&point.id, point)).collect();
    let mut derived = Vec::new();
    for coedge in &out.coedges {
        if !coedge.pcurves.is_empty() {
            continue;
        }
        let Some(face_id) = loop_faces.get(&coedge.owner_loop) else {
            continue;
        };
        let Some(face) = faces.get(face_id) else {
            continue;
        };
        let Some(SurfaceGeometry::Nurbs(surface)) =
            surfaces.get(&face.surface).map(|item| &item.geometry)
        else {
            continue;
        };
        let Some(edge) = edges.get(&coedge.edge) else {
            continue;
        };
        let Some(curve) = edge
            .curve
            .as_ref()
            .and_then(|id| curves.get(id).copied())
            .map(|item| &item.geometry)
        else {
            continue;
        };
        let endpoints = [edge.start.clone(), edge.end.clone()].map(|vertex_id| {
            let vertex = vertices.get(&vertex_id)?;
            Some(points.get(&vertex.point)?.position)
        });
        let endpoints = match endpoints {
            [Some(start), Some(end)] => Some([start, end]),
            _ => None,
        };
        let (geometry, parameter_range, fit_tolerance, cache) = match curve {
            CurveGeometry::Nurbs(curve) => {
                let Some(parameter_range) = nurbs_edge_parameter_range(edge, curve, endpoints)
                else {
                    continue;
                };
                match derive_nurbs_edge_pcurve(surface, curve, parameter_range) {
                    NurbsPcurveResolution::Exact(geometry) => {
                        (geometry, Some(parameter_range), None, false)
                    }
                    NurbsPcurveResolution::Cache {
                        geometry,
                        fit_tolerance,
                    } => (geometry, Some(parameter_range), Some(fit_tolerance), true),
                    NurbsPcurveResolution::OffSurface => {
                        out.stats.off_surface_nurbs_pcurves += 1;
                        continue;
                    }
                    NurbsPcurveResolution::Ambiguous => {
                        out.stats.ambiguous_pcurve_parameters += 1;
                        continue;
                    }
                    NurbsPcurveResolution::NoMatch => continue,
                }
            }
            CurveGeometry::Line { origin, direction } => {
                let resolution = resolve_axis_candidates([
                    ruled_surface_line_pcurve(
                        surface,
                        SurfaceParameterAxis::U,
                        *origin,
                        *direction,
                    ),
                    ruled_surface_line_pcurve(
                        surface,
                        SurfaceParameterAxis::V,
                        *origin,
                        *direction,
                    ),
                ]);
                match resolution {
                    InverseResolution::Unique(geometry) => (geometry, None, None, false),
                    InverseResolution::Ambiguous => {
                        out.stats.ambiguous_pcurve_parameters += 1;
                        continue;
                    }
                    InverseResolution::NoMatch => continue,
                }
            }
            _ => continue,
        };
        let id = PcurveId(format!(
            "sldprt:brep:pcurve#{}:{}",
            if cache {
                "nurbs-surface-cache"
            } else {
                "nurbs-isoparametric"
            },
            coedge.id.0.rsplit('#').next().unwrap_or("0")
        ));
        derived.push((
            coedge.id.clone(),
            id.clone(),
            Pcurve {
                id,
                geometry,
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range,
                fit_tolerance,
            },
            cache,
        ));
    }
    let coedge_indices = out
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| (coedge.id.clone(), index))
        .collect::<HashMap<_, _>>();
    for (coedge_id, id, pcurve, cache) in derived {
        if let Some(index) = coedge_indices.get(&coedge_id) {
            out.coedges[*index].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
                pcurve: id.clone(),
                isoparametric: None,
                parameter_range: pcurve.parameter_range,
            }];
        }
        annotations.note(&id, source_stream, 0).tag(if cache {
            "derived_nurbs_surface_cache_pcurve"
        } else {
            "derived_nurbs_isoparametric_pcurve"
        });
        annotations.exactness(&id, Exactness::Derived);
        out.pcurves.push(pcurve);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntersectionPcurveSource {
    StoredCache,
    AnalyticInverse,
    NurbsInverse,
}

/// Conservative maximum separation between one analytic surface image of a
/// linear UV segment and the straight chord through its endpoints.
fn analytic_pcurve_chord_bound(
    surface: &SurfaceGeometry,
    start: cadmpeg_ir::math::Point2,
    end: cadmpeg_ir::math::Point2,
) -> Option<f64> {
    let du = (end.u - start.u).abs();
    let dv = (end.v - start.v).abs();
    let second_derivative_bound = match surface {
        SurfaceGeometry::Plane { .. } => 0.0,
        SurfaceGeometry::Cylinder { radius, .. } => radius.abs() * du * du,
        SurfaceGeometry::Cone {
            radius,
            ratio,
            half_angle,
            ..
        } => {
            let slope = half_angle.tan().abs();
            let radial_scale = 1.0f64.max(ratio.abs());
            let max_radius = (radius + start.v * half_angle.tan())
                .abs()
                .max((radius + end.v * half_angle.tan()).abs());
            radial_scale * (max_radius * du * du + 2.0 * slope * du * dv)
        }
        SurfaceGeometry::Sphere { radius, .. } => radius.abs() * (du + dv).powi(2),
        SurfaceGeometry::Torus {
            major_radius,
            minor_radius,
            ..
        } => {
            (major_radius.abs() + minor_radius.abs()) * du * du
                + 2.0 * minor_radius.abs() * du * dv
                + minor_radius.abs() * dv * dv
        }
        _ => return None,
    };
    let bound = second_derivative_bound / 8.0;
    bound.is_finite().then_some(bound)
}

fn intersection_support_pcurve(
    support_data: &super::intersection::IntersectionSupportData,
    chart: &cadmpeg_ir::geometry::NurbsCurve,
    surface_attr: u16,
    surface: &SurfaceGeometry,
    edge_endpoints: [cadmpeg_ir::math::Point3; 2],
) -> Option<(PcurveGeometry, [f64; 2], IntersectionPcurveSource)> {
    if chart.degree != 1
        || chart.weights.is_some()
        || chart.periodic
        || chart.control_points.len() < 2
        || chart.knots.len() != chart.control_points.len() + 2
        || chart.knots.iter().any(|knot| !knot.is_finite())
        || !knots_nondecreasing(&chart.knots)
        || !support_data.fit_tolerance_mm.is_finite()
        || support_data.fit_tolerance_mm <= 0.0
    {
        return None;
    }
    let parameter_range = nurbs_curve_parameter_domain(chart)?;
    let support_index = match support_data.supports.map(|support| support == surface_attr) {
        [true, false] => 0,
        [false, true] => 1,
        _ => return None,
    };
    let squared_distance = |left: cadmpeg_ir::math::Point3, right: cadmpeg_ir::math::Point3| {
        (left.x - right.x).powi(2) + (left.y - right.y).powi(2) + (left.z - right.z).powi(2)
    };
    let model_endpoints = [
        *chart.control_points.first()?,
        *chart.control_points.last()?,
    ];
    let direct_error = squared_distance(model_endpoints[0], edge_endpoints[0])
        + squared_distance(model_endpoints[1], edge_endpoints[1]);
    let reverse_error = squared_distance(model_endpoints[0], edge_endpoints[1])
        + squared_distance(model_endpoints[1], edge_endpoints[0]);
    let targets = if direct_error <= reverse_error {
        edge_endpoints
    } else {
        [edge_endpoints[1], edge_endpoints[0]]
    };
    let (mut control_points, source) = if let Some(support_uv) = &support_data.support_uv {
        let mut control_points = support_uv[support_index].clone();
        match surface {
            SurfaceGeometry::Plane { .. } => {
                for point in &mut control_points {
                    point.u *= LEN_TO_MM;
                    point.v *= LEN_TO_MM;
                }
            }
            SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. } => {
                for point in &mut control_points {
                    point.v *= LEN_TO_MM;
                }
            }
            SurfaceGeometry::Sphere { .. }
            | SurfaceGeometry::Torus { .. }
            | SurfaceGeometry::Nurbs(_) => {}
            _ => return None,
        }
        (control_points, IntersectionPcurveSource::StoredCache)
    } else {
        match surface {
            SurfaceGeometry::Nurbs(surface) => {
                let mut control_points = Vec::with_capacity(chart.control_points.len());
                for point in &chart.control_points {
                    let parameters = nurbs_surface_parameter_within_tolerance(
                        surface,
                        *point,
                        control_points.last().copied(),
                        support_data.fit_tolerance_mm,
                    )?;
                    control_points.push(parameters);
                }
                (control_points, IntersectionPcurveSource::NurbsInverse)
            }
            _ => {
                let mut control_points = chart
                    .control_points
                    .iter()
                    .copied()
                    .map(|point| analytic_surface_parameters(surface, point))
                    .collect::<Option<Vec<_>>>()?;
                for index in 1..control_points.len() {
                    let previous = control_points[index - 1];
                    match surface {
                        SurfaceGeometry::Cylinder { .. }
                        | SurfaceGeometry::Cone { .. }
                        | SurfaceGeometry::Sphere { .. }
                        | SurfaceGeometry::Torus { .. } => {
                            control_points[index].u += ((previous.u - control_points[index].u)
                                / std::f64::consts::TAU)
                                .round()
                                * std::f64::consts::TAU;
                        }
                        SurfaceGeometry::Plane { .. } => {}
                        _ => return None,
                    }
                    if matches!(surface, SurfaceGeometry::Torus { .. }) {
                        control_points[index].v += ((previous.v - control_points[index].v)
                            / std::f64::consts::TAU)
                            .round()
                            * std::f64::consts::TAU;
                    }
                }
                (control_points, IntersectionPcurveSource::AnalyticInverse)
            }
        }
    };
    if control_points.len() != chart.control_points.len() {
        return None;
    }
    if let SurfaceGeometry::Nurbs(surface) = surface {
        let tolerance = inverse_coordinate_tolerance(edge_endpoints);
        let last = control_points.len() - 1;
        for (index, target) in [(0, targets[0]), (last, targets[1])] {
            control_points[index] = nurbs_surface_parameter_within_tolerance(
                surface,
                target,
                Some(control_points[index]),
                tolerance,
            )?;
        }
    } else {
        let adjust_periodic = |parameter: f64, reference: f64| {
            parameter
                + ((reference - parameter) / std::f64::consts::TAU).round() * std::f64::consts::TAU
        };
        let last = control_points.len() - 1;
        for (index, target) in [(0, targets[0]), (last, targets[1])] {
            let reference = control_points[index];
            let mut parameters = analytic_surface_parameters(surface, target)?;
            match surface {
                SurfaceGeometry::Cylinder { .. }
                | SurfaceGeometry::Cone { .. }
                | SurfaceGeometry::Sphere { .. }
                | SurfaceGeometry::Torus { .. } => {
                    parameters.u = adjust_periodic(parameters.u, reference.u);
                }
                SurfaceGeometry::Plane { .. } => {}
                _ => return None,
            }
            if matches!(surface, SurfaceGeometry::Torus { .. }) {
                parameters.v = adjust_periodic(parameters.v, reference.v);
            }
            control_points[index] = parameters;
        }
    }
    let tolerance = inverse_coordinate_tolerance(edge_endpoints);
    if [control_points.first()?, control_points.last()?]
        .into_iter()
        .zip(targets)
        .any(|(parameters, target)| {
            surface_point(surface, parameters.u, parameters.v)
                .is_none_or(|point| squared_distance(point, target) > tolerance * tolerance)
        })
    {
        return None;
    }
    let mapped_points = control_points
        .iter()
        .map(|parameters| surface_point(surface, parameters.u, parameters.v))
        .collect::<Option<Vec<_>>>()?;
    let control_errors = mapped_points
        .iter()
        .zip(&chart.control_points)
        .map(|(point, target)| squared_distance(*point, *target).sqrt())
        .collect::<Vec<_>>();
    if control_errors
        .iter()
        .any(|error| !error.is_finite() || *error > support_data.fit_tolerance_mm)
    {
        return None;
    }
    if control_points
        .windows(2)
        .zip(chart.control_points.windows(2))
        .zip(control_errors.windows(2))
        .any(|((parameters, chord), endpoint_errors)| match surface {
            SurfaceGeometry::Nurbs(surface) => nurbs_surface_parameter_segment_chord_bound(
                surface,
                [parameters[0], parameters[1]],
                [chord[0], chord[1]],
            )
            .is_none_or(|error| error > support_data.fit_tolerance_mm),
            _ => analytic_pcurve_chord_bound(surface, parameters[0], parameters[1]).is_none_or(
                |curvature_error| {
                    curvature_error + endpoint_errors[0].max(endpoint_errors[1])
                        > support_data.fit_tolerance_mm
                },
            ),
        })
    {
        return None;
    }
    Some((
        PcurveGeometry::Nurbs {
            degree: 1,
            knots: chart.knots.clone(),
            control_points,
            weights: None,
            periodic: false,
        },
        parameter_range,
        source,
    ))
}

fn resolve_axis_candidates<T, const N: usize>(
    candidates: [InverseResolution<T>; N],
) -> InverseResolution<T> {
    let mut unique = None;
    for candidate in candidates {
        match candidate {
            InverseResolution::NoMatch => {}
            InverseResolution::Ambiguous => return InverseResolution::Ambiguous,
            InverseResolution::Unique(value) if unique.is_none() => unique = Some(value),
            InverseResolution::Unique(_) => return InverseResolution::Ambiguous,
        }
    }
    unique.map_or(InverseResolution::NoMatch, InverseResolution::Unique)
}

fn nurbs_boundary_pcurve(
    surface: &cadmpeg_ir::geometry::NurbsSurface,
    curve: &cadmpeg_ir::geometry::NurbsCurve,
    fixed_axis: SurfaceParameterAxis,
) -> InverseResolution<PcurveGeometry> {
    let (fixed_degree, fixed_count, fixed_knots) = match fixed_axis {
        SurfaceParameterAxis::U => (
            surface.u_degree as usize,
            surface.u_count as usize,
            &surface.u_knots,
        ),
        SurfaceParameterAxis::V => (
            surface.v_degree as usize,
            surface.v_count as usize,
            &surface.v_knots,
        ),
    };
    let (varying_degree, varying_knots) = match fixed_axis {
        SurfaceParameterAxis::U => (surface.v_degree as usize, &surface.v_knots),
        SurfaceParameterAxis::V => (surface.u_degree as usize, &surface.u_knots),
    };
    let (Some(&fixed_min), Some(&fixed_max), Some(&varying_min)) = (
        fixed_knots.get(fixed_degree),
        fixed_knots.get(fixed_count),
        varying_knots.get(varying_degree),
    ) else {
        return InverseResolution::NoMatch;
    };
    if !fixed_min.is_finite()
        || !fixed_max.is_finite()
        || fixed_min >= fixed_max
        || !varying_min.is_finite()
    {
        return InverseResolution::NoMatch;
    }
    let tolerance = inverse_coordinate_tolerance(
        surface
            .control_points
            .iter()
            .copied()
            .chain(curve.control_points.iter().copied()),
    );
    let same_curve = |candidate: &cadmpeg_ir::geometry::NurbsCurve| {
        candidate.degree == curve.degree
            && candidate.knots == curve.knots
            && candidate.periodic == curve.periodic
            && candidate.control_points.len() == curve.control_points.len()
            && candidate
                .control_points
                .iter()
                .zip(&curve.control_points)
                .all(|(candidate, actual)| {
                    (candidate.x - actual.x).powi(2)
                        + (candidate.y - actual.y).powi(2)
                        + (candidate.z - actual.z).powi(2)
                        <= tolerance * tolerance
                })
            && match (candidate.weights.as_deref(), curve.weights.as_deref()) {
                (None, None) => true,
                (Some(candidate), Some(actual)) => {
                    candidate.len() == actual.len()
                        && candidate
                            .iter()
                            .zip(actual)
                            .all(|(candidate, actual)| (candidate - actual).abs() <= 1e-12)
                }
                _ => false,
            }
    };
    let mut candidates = [fixed_min, fixed_max]
        .into_iter()
        .filter(|parameter| parameter.is_finite())
        .filter(|parameter| {
            nurbs_surface_isocurve(surface, fixed_axis, *parameter)
                .is_some_and(|candidate| same_curve(&candidate))
        });
    let Some(fixed) = candidates.next() else {
        return InverseResolution::NoMatch;
    };
    if candidates.next().is_some() {
        return InverseResolution::Ambiguous;
    }
    InverseResolution::Unique(match fixed_axis {
        SurfaceParameterAxis::U => PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(fixed, varying_min),
            direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
        },
        SurfaceParameterAxis::V => PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(varying_min, fixed),
            direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
        },
    })
}

fn nurbs_strict_isocurve_pcurve(
    surface: &cadmpeg_ir::geometry::NurbsSurface,
    curve: &cadmpeg_ir::geometry::NurbsCurve,
) -> InverseResolution<PcurveGeometry> {
    let axis_candidate = |fixed_axis| {
        let (uc, vc) = (surface.u_count as usize, surface.v_count as usize);
        if surface.control_points.len() != uc.saturating_mul(vc)
            || surface
                .weights
                .as_ref()
                .is_some_and(|weights| weights.len() != surface.control_points.len())
        {
            return InverseResolution::NoMatch;
        }
        let (fixed_degree, fixed_count, fixed_knots, fixed_periodic) = match fixed_axis {
            SurfaceParameterAxis::U => (surface.u_degree, uc, &surface.u_knots, surface.u_periodic),
            SurfaceParameterAxis::V => (surface.v_degree, vc, &surface.v_knots, surface.v_periodic),
        };
        let (varying_degree, varying_count, varying_knots, varying_periodic) = match fixed_axis {
            SurfaceParameterAxis::U => (surface.v_degree, vc, &surface.v_knots, surface.v_periodic),
            SurfaceParameterAxis::V => (surface.u_degree, uc, &surface.u_knots, surface.u_periodic),
        };
        if curve.degree != varying_degree
            || curve.knots != *varying_knots
            || curve.periodic != varying_periodic
            || curve.control_points.len() != varying_count
        {
            return InverseResolution::NoMatch;
        }
        let (Some(&fixed_min), Some(&fixed_max)) = (fixed_knots.get(1), fixed_knots.get(2)) else {
            return nurbs_boundary_pcurve(surface, curve, fixed_axis);
        };
        if fixed_degree != 1
            || fixed_count != 2
            || fixed_periodic
            || fixed_knots.as_slice() != [fixed_min, fixed_min, fixed_max, fixed_max]
            || !fixed_min.is_finite()
            || !fixed_max.is_finite()
            || fixed_min >= fixed_max
        {
            return nurbs_boundary_pcurve(surface, curve, fixed_axis);
        }
        let pole_indices = |varying: usize| match fixed_axis {
            SurfaceParameterAxis::U => (varying, vc + varying),
            SurfaceParameterAxis::V => (varying * vc, varying * vc + 1),
        };
        let expected_weights = surface.weights.as_ref().map(|weights| {
            (0..varying_count)
                .map(|varying| weights[pole_indices(varying).0])
                .collect::<Vec<_>>()
        });
        if surface.weights.as_ref().is_some_and(|weights| {
            (0..varying_count).any(|varying| {
                let (a, b) = pole_indices(varying);
                (weights[a] - weights[b]).abs() > 1e-12
            })
        }) || match (curve.weights.as_deref(), expected_weights.as_deref()) {
            (None, None) => false,
            (Some(actual), Some(expected)) => {
                actual.len() != expected.len()
                    || actual
                        .iter()
                        .zip(expected)
                        .any(|(actual, expected)| (actual - expected).abs() > 1e-12)
            }
            _ => true,
        } {
            return InverseResolution::NoMatch;
        }
        let mut delta_squared = 0.0;
        let mut relative_dot_delta = 0.0;
        for (varying, point) in curve.control_points.iter().enumerate() {
            let (a_index, b_index) = pole_indices(varying);
            let a = surface.control_points[a_index];
            let b = surface.control_points[b_index];
            let delta = [b.x - a.x, b.y - a.y, b.z - a.z];
            let relative = [point.x - a.x, point.y - a.y, point.z - a.z];
            delta_squared += delta.iter().map(|value| value * value).sum::<f64>();
            relative_dot_delta += relative
                .iter()
                .zip(delta)
                .map(|(relative, delta)| relative * delta)
                .sum::<f64>();
        }
        let tolerance = inverse_coordinate_tolerance(
            surface
                .control_points
                .iter()
                .copied()
                .chain(curve.control_points.iter().copied()),
        );
        if delta_squared <= f64::EPSILON {
            let all_equal = (0..varying_count).all(|varying| {
                let a = surface.control_points[pole_indices(varying).0];
                let point = curve.control_points[varying];
                (point.x - a.x).powi(2) + (point.y - a.y).powi(2) + (point.z - a.z).powi(2)
                    <= tolerance * tolerance
            });
            return if all_equal {
                InverseResolution::Ambiguous
            } else {
                InverseResolution::NoMatch
            };
        }
        let factor = relative_dot_delta / delta_squared;
        let residual_squared = (0..varying_count)
            .map(|varying| {
                let (a_index, b_index) = pole_indices(varying);
                let a = surface.control_points[a_index];
                let b = surface.control_points[b_index];
                let point = curve.control_points[varying];
                (point.x - (a.x + factor * (b.x - a.x))).powi(2)
                    + (point.y - (a.y + factor * (b.y - a.y))).powi(2)
                    + (point.z - (a.z + factor * (b.z - a.z))).powi(2)
            })
            .fold(0.0_f64, f64::max);
        let parameter_tolerance =
            INVERSE_PARAMETER_TOLERANCE * (1.0 + (fixed_max - fixed_min).abs());
        if !factor.is_finite()
            || factor < -parameter_tolerance
            || factor > 1.0 + parameter_tolerance
            || residual_squared > tolerance * tolerance
        {
            return InverseResolution::NoMatch;
        }
        let fixed = fixed_min + factor.clamp(0.0, 1.0) * (fixed_max - fixed_min);
        let Some(varying_degree) = usize::try_from(varying_degree).ok() else {
            return InverseResolution::NoMatch;
        };
        let (Some(&varying_min), Some(&varying_max)) = (
            varying_knots.get(varying_degree),
            varying_knots.get(varying_count),
        ) else {
            return InverseResolution::NoMatch;
        };
        if !varying_min.is_finite() || !varying_max.is_finite() || varying_min >= varying_max {
            return InverseResolution::NoMatch;
        }
        InverseResolution::Unique(match fixed_axis {
            SurfaceParameterAxis::U => PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(fixed, varying_min),
                direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
            },
            SurfaceParameterAxis::V => PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(varying_min, fixed),
                direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
            },
        })
    };
    resolve_axis_candidates([
        axis_candidate(SurfaceParameterAxis::U),
        axis_candidate(SurfaceParameterAxis::V),
    ])
}

fn nurbs_active_domain(knots: &[f64], degree: u32, count: u32) -> Option<[f64; 2]> {
    let degree = usize::try_from(degree).ok()?;
    let count = usize::try_from(count).ok()?;
    if count <= degree || knots.len() != count.checked_add(degree)?.checked_add(1)? {
        return None;
    }
    if knots.iter().any(|knot| !knot.is_finite()) || !knots_nondecreasing(knots) {
        return None;
    }
    let domain = [*knots.get(degree)?, *knots.get(count)?];
    (domain[0] < domain[1]).then_some(domain)
}

fn nurbs_roundoff_equal(left: f64, right: f64, scale: f64) -> bool {
    left.is_finite()
        && right.is_finite()
        && (left - right).abs()
            <= NURBS_POLE_ROUNDOFF_FACTOR
                * 1.0_f64.max(scale.abs()).max(left.abs()).max(right.abs())
}

fn nurbs_representation_matches(
    expected: &cadmpeg_ir::geometry::NurbsCurve,
    actual: &cadmpeg_ir::geometry::NurbsCurve,
) -> bool {
    if expected.degree != actual.degree
        || expected.periodic != actual.periodic
        || expected.knots.len() != actual.knots.len()
        || expected.control_points.len() != actual.control_points.len()
    {
        return false;
    }
    let scale = expected
        .control_points
        .iter()
        .chain(&actual.control_points)
        .flat_map(|point| [point.x.abs(), point.y.abs(), point.z.abs()])
        .chain(
            expected
                .weights
                .iter()
                .flat_map(|weights| weights.iter().copied().map(f64::abs)),
        )
        .chain(
            actual
                .weights
                .iter()
                .flat_map(|weights| weights.iter().copied().map(f64::abs)),
        )
        .fold(1.0_f64, f64::max);
    expected
        .knots
        .iter()
        .zip(&actual.knots)
        .all(|(left, right)| nurbs_roundoff_equal(*left, *right, scale))
        && expected
            .control_points
            .iter()
            .zip(&actual.control_points)
            .all(|(left, right)| {
                nurbs_roundoff_equal(left.x, right.x, scale)
                    && nurbs_roundoff_equal(left.y, right.y, scale)
                    && nurbs_roundoff_equal(left.z, right.z, scale)
            })
        && match (expected.weights.as_deref(), actual.weights.as_deref()) {
            (None, None) => true,
            (Some(expected), Some(actual)) => {
                expected.len() == actual.len()
                    && expected
                        .iter()
                        .zip(actual)
                        .all(|(left, right)| nurbs_roundoff_equal(*left, *right, scale))
            }
            _ => false,
        }
}

fn nurbs_homogeneous_controls(curve: &cadmpeg_ir::geometry::NurbsCurve) -> Option<Vec<[f64; 4]>> {
    if curve.control_points.is_empty()
        || curve.knots.len()
            != curve
                .control_points
                .len()
                .checked_add(usize::try_from(curve.degree).ok()?)?
                .checked_add(1)?
        || curve.knots.iter().any(|knot| !knot.is_finite())
        || !knots_nondecreasing(&curve.knots)
        || curve
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != curve.control_points.len())
    {
        return None;
    }
    curve
        .control_points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let weight = curve.weights.as_ref().map_or(1.0, |weights| weights[index]);
            (point.x.is_finite()
                && point.y.is_finite()
                && point.z.is_finite()
                && weight.is_finite()
                && weight > 0.0)
                .then_some([point.x * weight, point.y * weight, point.z * weight, weight])
        })
        .collect()
}

fn insert_nurbs_homogeneous_knot(
    degree: usize,
    knots: &[f64],
    controls: &[[f64; 4]],
    value: f64,
) -> Option<(Vec<f64>, Vec<[f64; 4]>)> {
    if degree == 0
        || controls.is_empty()
        || knots.len() != controls.len().checked_add(degree)?.checked_add(1)?
        || !value.is_finite()
        || knots.iter().any(|knot| !knot.is_finite())
        || !knots_nondecreasing(knots)
    {
        return None;
    }
    let n = controls.len() - 1;
    let domain = [*knots.get(degree)?, *knots.get(n + 1)?];
    if value < domain[0] || value > domain[1] {
        return None;
    }
    let span = if value == domain[1] {
        n
    } else {
        (degree..=n).find(|index| knots[*index] <= value && value < knots[*index + 1])?
    };
    let multiplicity = knots.iter().filter(|knot| **knot == value).count();
    if multiplicity > degree {
        return None;
    }
    let mut inserted_knots = Vec::with_capacity(knots.len() + 1);
    inserted_knots.extend_from_slice(&knots[..=span]);
    inserted_knots.push(value);
    inserted_knots.extend_from_slice(&knots[span + 1..]);

    let mut inserted_controls = vec![[0.0; 4]; controls.len() + 1];
    let prefix_end = span - degree + 1;
    inserted_controls[..prefix_end].copy_from_slice(&controls[..prefix_end]);
    let suffix_start = span.saturating_sub(multiplicity);
    inserted_controls[(suffix_start + 1)..].copy_from_slice(&controls[suffix_start..]);
    let middle_start = span - degree + 1;
    let middle_end = span - multiplicity;
    if middle_start <= middle_end {
        for index in middle_start..=middle_end {
            let denominator = knots[index + degree] - knots[index];
            if !denominator.is_finite() || denominator <= 0.0 {
                return None;
            }
            let alpha = (value - knots[index]) / denominator;
            inserted_controls[index] = std::array::from_fn(|axis| {
                alpha * controls[index][axis] + (1.0 - alpha) * controls[index - 1][axis]
            });
        }
    }
    Some((inserted_knots, inserted_controls))
}

fn clamp_nurbs_curve_to_domain(
    curve: &cadmpeg_ir::geometry::NurbsCurve,
    domain: [f64; 2],
) -> Option<cadmpeg_ir::geometry::NurbsCurve> {
    if curve.periodic || !domain[0].is_finite() || !domain[1].is_finite() || domain[0] >= domain[1]
    {
        return None;
    }
    let degree = usize::try_from(curve.degree).ok()?;
    let original_domain = nurbs_curve_parameter_domain(curve)?;
    if domain[0] < original_domain[0] || domain[1] > original_domain[1] {
        return None;
    }
    let mut knots = curve.knots.clone();
    let mut controls = nurbs_homogeneous_controls(curve)?;
    let full_multiplicity = degree.checked_add(1)?;
    for value in domain {
        let multiplicity = knots.iter().filter(|knot| **knot == value).count();
        if multiplicity > full_multiplicity {
            return None;
        }
        for _ in multiplicity..full_multiplicity {
            (knots, controls) = insert_nurbs_homogeneous_knot(degree, &knots, &controls, value)?;
        }
    }
    let start = knots.iter().position(|knot| *knot == domain[0])?;
    let end = knots.iter().position(|knot| *knot == domain[1])?;
    let end_last = knots.iter().rposition(|knot| *knot == domain[1])?;
    if end <= start || end_last < end || end - start == 0 {
        return None;
    }
    let segment_controls = controls.get(start..end)?.to_vec();
    let segment_knots = knots.get(start..=end_last)?.to_vec();
    if segment_knots.len() != segment_controls.len().checked_add(degree)?.checked_add(1)? {
        return None;
    }
    let rational = curve.weights.is_some();
    let mut control_points = Vec::with_capacity(segment_controls.len());
    let mut weights = rational.then(Vec::new);
    for [x, y, z, weight] in segment_controls {
        if !weight.is_finite() || weight <= 0.0 {
            return None;
        }
        control_points.push(cadmpeg_ir::math::Point3::new(
            x / weight,
            y / weight,
            z / weight,
        ));
        if let Some(weights) = &mut weights {
            weights.push(weight);
        }
    }
    Some(cadmpeg_ir::geometry::NurbsCurve {
        degree: curve.degree,
        knots: segment_knots,
        control_points,
        weights,
        periodic: false,
    })
}

fn extended_nurbs_isocurve_axis_candidate(
    surface: &cadmpeg_ir::geometry::NurbsSurface,
    curve: &cadmpeg_ir::geometry::NurbsCurve,
    fixed_axis: SurfaceParameterAxis,
) -> InverseResolution<PcurveGeometry> {
    let (fixed_degree, fixed_count, fixed_knots, fixed_periodic) = match fixed_axis {
        SurfaceParameterAxis::U => (
            surface.u_degree,
            surface.u_count,
            &surface.u_knots,
            surface.u_periodic,
        ),
        SurfaceParameterAxis::V => (
            surface.v_degree,
            surface.v_count,
            &surface.v_knots,
            surface.v_periodic,
        ),
    };
    let (varying_degree, varying_count, varying_knots, varying_periodic) = match fixed_axis {
        SurfaceParameterAxis::U => (
            surface.v_degree,
            surface.v_count,
            &surface.v_knots,
            surface.v_periodic,
        ),
        SurfaceParameterAxis::V => (
            surface.u_degree,
            surface.u_count,
            &surface.u_knots,
            surface.u_periodic,
        ),
    };
    let Some(expected_control_count) = usize::try_from(surface.u_count).ok().and_then(|u| {
        usize::try_from(surface.v_count)
            .ok()
            .and_then(|v| u.checked_mul(v))
    }) else {
        return InverseResolution::NoMatch;
    };
    if fixed_periodic
        || varying_periodic
        || curve.periodic
        || curve.degree != varying_degree
        || surface.control_points.len() != expected_control_count
    {
        return InverseResolution::NoMatch;
    }
    let Some(fixed_domain) = nurbs_active_domain(fixed_knots, fixed_degree, fixed_count) else {
        return InverseResolution::NoMatch;
    };
    let Some(varying_domain) = nurbs_active_domain(varying_knots, varying_degree, varying_count)
    else {
        return InverseResolution::NoMatch;
    };
    let Some(curve_domain) = nurbs_curve_parameter_domain(curve) else {
        return InverseResolution::NoMatch;
    };
    if curve_domain[0] > varying_domain[0] || curve_domain[1] < varying_domain[1] {
        return InverseResolution::NoMatch;
    }

    let mut fixed_values = vec![fixed_domain[0], fixed_domain[1]];
    let overlap = [
        curve_domain[0].max(varying_domain[0]),
        curve_domain[1].min(varying_domain[1]),
    ];
    if overlap[0] < overlap[1] {
        let parameter = (overlap[0] + overlap[1]) * 0.5;
        if let Some(point) = nurbs_curve_point(
            curve.degree,
            &curve.knots,
            &curve.control_points,
            curve.weights.as_deref(),
            parameter,
        ) {
            let tolerance = inverse_coordinate_tolerance(
                surface
                    .control_points
                    .iter()
                    .copied()
                    .chain(std::iter::once(point)),
            );
            if let Some(parameters) =
                nurbs_seeded_surface_projection(surface, point, None).filter(|parameters| {
                    nurbs_surface_point(surface, parameters.u, parameters.v)
                        .is_some_and(|mapped| point_distance(point, mapped) <= tolerance)
                })
            {
                fixed_values.push(match fixed_axis {
                    SurfaceParameterAxis::U => parameters.u,
                    SurfaceParameterAxis::V => parameters.v,
                });
            }
        }
    }
    let parameter_tolerance =
        INVERSE_PARAMETER_TOLERANCE * (1.0 + (fixed_domain[1] - fixed_domain[0]).abs());
    let mut unique_fixed_values = Vec::new();
    for value in fixed_values {
        if value.is_finite()
            && value >= fixed_domain[0] - parameter_tolerance
            && value <= fixed_domain[1] + parameter_tolerance
            && !unique_fixed_values
                .iter()
                .any(|known: &f64| (value - *known).abs() <= parameter_tolerance)
        {
            unique_fixed_values.push(value.clamp(fixed_domain[0], fixed_domain[1]));
        }
    }
    let mut matches = unique_fixed_values.into_iter().filter(|fixed| {
        let Some(clamped) = clamp_nurbs_curve_to_domain(curve, varying_domain) else {
            return false;
        };
        nurbs_surface_isocurve(surface, fixed_axis, *fixed)
            .is_some_and(|expected| nurbs_representation_matches(&expected, &clamped))
    });
    let Some(fixed) = matches.next() else {
        return InverseResolution::NoMatch;
    };
    if matches.next().is_some() {
        return InverseResolution::Ambiguous;
    }
    InverseResolution::Unique(match fixed_axis {
        SurfaceParameterAxis::U => PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(fixed, varying_domain[0]),
            direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
        },
        SurfaceParameterAxis::V => PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(varying_domain[0], fixed),
            direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
        },
    })
}

fn extended_nurbs_isocurve_pcurve(
    surface: &cadmpeg_ir::geometry::NurbsSurface,
    curve: &cadmpeg_ir::geometry::NurbsCurve,
) -> InverseResolution<PcurveGeometry> {
    resolve_axis_candidates([
        extended_nurbs_isocurve_axis_candidate(surface, curve, SurfaceParameterAxis::U),
        extended_nurbs_isocurve_axis_candidate(surface, curve, SurfaceParameterAxis::V),
    ])
}

fn nurbs_isocurve_pcurve(
    surface: &cadmpeg_ir::geometry::NurbsSurface,
    curve: &cadmpeg_ir::geometry::NurbsCurve,
) -> InverseResolution<PcurveGeometry> {
    match nurbs_strict_isocurve_pcurve(surface, curve) {
        InverseResolution::NoMatch => extended_nurbs_isocurve_pcurve(surface, curve),
        other => other,
    }
}

enum NurbsPcurveResolution {
    Exact(PcurveGeometry),
    Cache {
        geometry: PcurveGeometry,
        fit_tolerance: f64,
    },
    OffSurface,
    NoMatch,
    Ambiguous,
}

fn nurbs_curve_sample_parameters(
    curve: &cadmpeg_ir::geometry::NurbsCurve,
    range: [f64; 2],
) -> Option<Vec<f64>> {
    let domain = nurbs_curve_parameter_domain(curve)?;
    if !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] >= range[1]
        || range[0] < domain[0]
        || range[1] > domain[1]
    {
        return None;
    }
    let mut parameters = vec![range[0], range[1]];
    for span in curve.knots.windows(2) {
        let start = span[0].max(range[0]);
        let end = span[1].min(range[1]);
        if !span[0].is_finite() || !span[1].is_finite() || start >= end {
            continue;
        }
        for index in 0..=NURBS_CACHE_SAMPLES_PER_SPAN {
            let fraction = index as f64 / NURBS_CACHE_SAMPLES_PER_SPAN as f64;
            parameters.push(start + fraction * (end - start));
        }
    }
    parameters.sort_by(f64::total_cmp);
    parameters.dedup_by(|left, right| {
        (*left - *right).abs()
            <= INVERSE_PARAMETER_TOLERANCE * (1.0 + range[1].abs().max(range[0].abs()))
    });
    (!parameters.is_empty()).then_some(parameters)
}

fn point_distance(left: cadmpeg_ir::math::Point3, right: cadmpeg_ir::math::Point3) -> f64 {
    (left.x - right.x)
        .hypot(left.y - right.y)
        .hypot(left.z - right.z)
}

fn nurbs_edge_endpoint_parameters(
    surface: &cadmpeg_ir::geometry::NurbsSurface,
    curve: &cadmpeg_ir::geometry::NurbsCurve,
    range: [f64; 2],
) -> Option<[cadmpeg_ir::math::Point2; 2]> {
    let curve_points = range.map(|parameter| {
        nurbs_curve_point(
            curve.degree,
            &curve.knots,
            &curve.control_points,
            curve.weights.as_deref(),
            parameter,
        )
    });
    let [Some(first), Some(last)] = curve_points else {
        return None;
    };
    let tolerance =
        inverse_coordinate_tolerance(surface.control_points.iter().copied().chain([first, last]))
            .max(NURBS_ENDPOINT_TOLERANCE_MM);
    let project = |point| {
        let parameters = nurbs_seeded_surface_projection(surface, point, None)?;
        let mapped = nurbs_surface_point(surface, parameters.u, parameters.v)?;
        (point_distance(point, mapped) <= tolerance).then_some(parameters)
    };
    Some([project(first)?, project(last)?])
}

fn nurbs_seeded_surface_projection(
    surface: &cadmpeg_ir::geometry::NurbsSurface,
    point: cadmpeg_ir::math::Point3,
    seed: Option<cadmpeg_ir::math::Point2>,
) -> Option<cadmpeg_ir::math::Point2> {
    nurbs_surface_parameter_near_point(surface, point, seed)
}

fn nurbs_curve_surface_deviation(
    surface: &cadmpeg_ir::geometry::NurbsSurface,
    curve: &cadmpeg_ir::geometry::NurbsCurve,
    range: [f64; 2],
) -> Option<f64> {
    let parameters = nurbs_curve_sample_parameters(curve, range)?;
    let mut seed = None;
    let mut maximum = 0.0_f64;
    for parameter in parameters {
        let point = nurbs_curve_point(
            curve.degree,
            &curve.knots,
            &curve.control_points,
            curve.weights.as_deref(),
            parameter,
        )?;
        let parameters = seed
            .and_then(|seed| nurbs_seeded_surface_projection(surface, point, Some(seed)))
            .or_else(|| nurbs_seeded_surface_projection(surface, point, None))?;
        let surface_point = nurbs_surface_point(surface, parameters.u, parameters.v)?;
        seed = Some(parameters);
        maximum = maximum.max(point_distance(point, surface_point));
    }
    maximum.is_finite().then_some(maximum)
}

fn nurbs_degree_one_cache_pcurve(
    surface: &cadmpeg_ir::geometry::NurbsSurface,
    curve: &cadmpeg_ir::geometry::NurbsCurve,
    range: [f64; 2],
) -> Option<(PcurveGeometry, f64)> {
    if curve.degree != 1
        || curve.weights.is_some()
        || curve.periodic
        || curve.control_points.len() < 2
        || curve.knots.len() != curve.control_points.len() + 2
        || !knots_nondecreasing(&curve.knots)
    {
        return None;
    }
    let mut control_points = Vec::with_capacity(curve.control_points.len());
    let mut seed = None;
    for point in &curve.control_points {
        let parameters = seed
            .and_then(|seed| nurbs_seeded_surface_projection(surface, *point, Some(seed)))
            .or_else(|| nurbs_seeded_surface_projection(surface, *point, None))?;
        if !parameters.u.is_finite() || !parameters.v.is_finite() {
            return None;
        }
        seed = Some(parameters);
        control_points.push(cadmpeg_ir::math::Point2::new(parameters.u, parameters.v));
    }
    let uv_control_points = control_points
        .iter()
        .map(|point| cadmpeg_ir::math::Point3::new(point.u, point.v, 0.0))
        .collect::<Vec<_>>();
    let parameters = nurbs_curve_sample_parameters(curve, range)?;
    let mut fit_tolerance = 0.0_f64;
    for parameter in parameters {
        let model_point = nurbs_curve_point(
            curve.degree,
            &curve.knots,
            &curve.control_points,
            None,
            parameter,
        )?;
        let uv = nurbs_curve_point(1, &curve.knots, &uv_control_points, None, parameter)?;
        let mapped_point = nurbs_surface_point(surface, uv.x, uv.y)?;
        fit_tolerance = fit_tolerance.max(point_distance(model_point, mapped_point));
    }
    if !fit_tolerance.is_finite() {
        return None;
    }
    Some((
        PcurveGeometry::Nurbs {
            degree: 1,
            knots: curve.knots.clone(),
            control_points,
            weights: None,
            periodic: false,
        },
        fit_tolerance,
    ))
}

fn nurbs_edge_parameter_range(
    edge: &Edge,
    curve: &cadmpeg_ir::geometry::NurbsCurve,
    endpoints: Option<[cadmpeg_ir::math::Point3; 2]>,
) -> Option<[f64; 2]> {
    let domain = nurbs_curve_parameter_domain(curve)?;
    let range = if let Some(range) = edge.param_range {
        range
    } else {
        let [start, end] = endpoints?;
        match (
            nurbs_parameter_at_point(curve, start),
            nurbs_parameter_at_point(curve, end),
        ) {
            (InverseResolution::Unique(start), InverseResolution::Unique(end)) => [start, end],
            _ => return None,
        }
    };
    if !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] == range[1]
        || range
            .iter()
            .any(|parameter| *parameter < domain[0] || *parameter > domain[1])
    {
        return None;
    }
    Some([range[0].min(range[1]), range[0].max(range[1])])
}

fn derive_nurbs_edge_pcurve(
    surface: &cadmpeg_ir::geometry::NurbsSurface,
    curve: &cadmpeg_ir::geometry::NurbsCurve,
    range: [f64; 2],
) -> NurbsPcurveResolution {
    if nurbs_edge_endpoint_parameters(surface, curve, range).is_none() {
        return NurbsPcurveResolution::OffSurface;
    }
    if nurbs_curve_surface_deviation(surface, curve, range).is_none() {
        return NurbsPcurveResolution::NoMatch;
    }
    match nurbs_isocurve_pcurve(surface, curve) {
        InverseResolution::Unique(geometry) => NurbsPcurveResolution::Exact(geometry),
        InverseResolution::Ambiguous => NurbsPcurveResolution::Ambiguous,
        InverseResolution::NoMatch => match nurbs_degree_one_cache_pcurve(surface, curve, range) {
            Some((geometry, fit_tolerance)) => NurbsPcurveResolution::Cache {
                geometry,
                fit_tolerance,
            },
            None => NurbsPcurveResolution::NoMatch,
        },
    }
}

fn ruled_surface_line_pcurve(
    surface: &cadmpeg_ir::geometry::NurbsSurface,
    fixed_axis: SurfaceParameterAxis,
    line_origin: cadmpeg_ir::math::Point3,
    line_direction: cadmpeg_ir::math::Vector3,
) -> InverseResolution<PcurveGeometry> {
    let (uc, vc) = (surface.u_count as usize, surface.v_count as usize);
    if surface.control_points.len() != uc.saturating_mul(vc)
        || surface
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != surface.control_points.len())
    {
        return InverseResolution::NoMatch;
    }
    let (varying_degree, varying_count, varying_knots, varying_periodic) = match fixed_axis {
        SurfaceParameterAxis::U => (surface.v_degree, vc, &surface.v_knots, surface.v_periodic),
        SurfaceParameterAxis::V => (surface.u_degree, uc, &surface.u_knots, surface.u_periodic),
    };
    let (fixed_degree, fixed_count, fixed_knots) = match fixed_axis {
        SurfaceParameterAxis::U => (surface.u_degree, uc, &surface.u_knots),
        SurfaceParameterAxis::V => (surface.v_degree, vc, &surface.v_knots),
    };
    let (Some(&varying_min), Some(&varying_max)) = (varying_knots.get(1), varying_knots.get(2))
    else {
        return InverseResolution::NoMatch;
    };
    if varying_degree != 1
        || varying_count != 2
        || varying_periodic
        || varying_knots.as_slice() != [varying_min, varying_min, varying_max, varying_max]
        || !varying_min.is_finite()
        || !varying_max.is_finite()
        || varying_min >= varying_max
        || surface.weights.as_ref().is_some_and(|weights| {
            (0..fixed_count).any(|fixed| {
                let (a, b) = match fixed_axis {
                    SurfaceParameterAxis::U => (fixed * vc, fixed * vc + 1),
                    SurfaceParameterAxis::V => (fixed, vc + fixed),
                };
                (weights[a] - weights[b]).abs() > 1e-12
            })
        })
    {
        return InverseResolution::NoMatch;
    }
    let Some(fixed_degree) = usize::try_from(fixed_degree).ok() else {
        return InverseResolution::NoMatch;
    };
    let (Some(&fixed_min), Some(&fixed_max)) =
        (fixed_knots.get(fixed_degree), fixed_knots.get(fixed_count))
    else {
        return InverseResolution::NoMatch;
    };
    if !fixed_min.is_finite() || !fixed_max.is_finite() || fixed_min >= fixed_max {
        return InverseResolution::NoMatch;
    }
    let evaluate_ruling = |fixed: f64| {
        let parameters = |varying| match fixed_axis {
            SurfaceParameterAxis::U => (fixed, varying),
            SurfaceParameterAxis::V => (varying, fixed),
        };
        let (u0, v0) = parameters(varying_min);
        let (u1, v1) = parameters(varying_max);
        Some((
            nurbs_surface_point(surface, u0, v0)?,
            nurbs_surface_point(surface, u1, v1)?,
        ))
    };
    let direction_squared = line_direction.x * line_direction.x
        + line_direction.y * line_direction.y
        + line_direction.z * line_direction.z;
    if direction_squared <= f64::EPSILON {
        return InverseResolution::NoMatch;
    }
    let perpendicular_squared = |point: cadmpeg_ir::math::Point3| {
        let relative = [
            point.x - line_origin.x,
            point.y - line_origin.y,
            point.z - line_origin.z,
        ];
        let along = (relative[0] * line_direction.x
            + relative[1] * line_direction.y
            + relative[2] * line_direction.z)
            / direction_squared;
        (relative[0] - along * line_direction.x).powi(2)
            + (relative[1] - along * line_direction.y).powi(2)
            + (relative[2] - along * line_direction.z).powi(2)
    };
    let objective = |parameter: f64| {
        let (a, b) = evaluate_ruling(parameter)?;
        Some(perpendicular_squared(a).max(perpendicular_squared(b)))
    };
    let Some(candidates) = sampled_parameter_minima(fixed_knots, [fixed_min, fixed_max], objective)
    else {
        return InverseResolution::NoMatch;
    };
    let resolution = unique_inverse_parameter(
        candidates,
        inverse_coordinate_tolerance(
            surface
                .control_points
                .iter()
                .copied()
                .chain(std::iter::once(line_origin)),
        ),
        [fixed_min, fixed_max],
    );
    let fixed = match resolution {
        InverseResolution::Unique(fixed) => fixed,
        InverseResolution::NoMatch => return InverseResolution::NoMatch,
        InverseResolution::Ambiguous => return InverseResolution::Ambiguous,
    };
    let Some((a, b)) = evaluate_ruling(fixed) else {
        return InverseResolution::NoMatch;
    };
    let delta = [b.x - a.x, b.y - a.y, b.z - a.z];
    let delta_squared = delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2];
    if delta_squared <= f64::EPSILON {
        return InverseResolution::NoMatch;
    }
    let project = |value: [f64; 3]| {
        (value[0] * delta[0] + value[1] * delta[1] + value[2] * delta[2]) / delta_squared
    };
    let offset = project([
        line_origin.x - a.x,
        line_origin.y - a.y,
        line_origin.z - a.z,
    ]);
    let rate = project([line_direction.x, line_direction.y, line_direction.z]);
    if rate == 0.0 {
        return InverseResolution::NoMatch;
    }
    let domain = varying_max - varying_min;
    InverseResolution::Unique(match fixed_axis {
        SurfaceParameterAxis::U => PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(fixed, varying_min + offset * domain),
            direction: cadmpeg_ir::math::Point2::new(0.0, rate * domain),
        },
        SurfaceParameterAxis::V => PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(varying_min + offset * domain, fixed),
            direction: cadmpeg_ir::math::Point2::new(rate * domain, 0.0),
        },
    })
}

fn solve_face_orientation(out: &mut Brep) {
    let loop_faces: HashMap<_, _> = out
        .loops
        .iter()
        .map(|lp| (lp.id.clone(), lp.face.clone()))
        .collect();
    let mut uses: HashMap<EdgeId, Vec<(FaceId, bool)>> = HashMap::new();
    for coedge in &out.coedges {
        if let Some(face) = loop_faces.get(&coedge.owner_loop) {
            uses.entry(coedge.edge.clone())
                .or_default()
                .push((face.clone(), coedge.sense == Sense::Reversed));
        }
    }
    let mut adjacency: HashMap<FaceId, Vec<(FaceId, bool)>> = HashMap::new();
    for edge_uses in uses.values().filter(|uses| uses.len() == 2) {
        let (a, a_reversed) = &edge_uses[0];
        let (b, b_reversed) = &edge_uses[1];
        let parity = *a_reversed == *b_reversed;
        adjacency
            .entry(a.clone())
            .or_default()
            .push((b.clone(), parity));
        adjacency
            .entry(b.clone())
            .or_default()
            .push((a.clone(), parity));
    }
    let initial: HashMap<_, _> = out
        .faces
        .iter()
        .map(|face| (face.id.clone(), face.sense == Sense::Reversed))
        .collect();
    let mut solved = HashMap::new();
    for root in out.faces.iter().map(|face| face.id.clone()) {
        if solved.contains_key(&root) {
            continue;
        }
        solved.insert(root.clone(), initial[&root]);
        let mut pending = vec![root];
        while let Some(face) = pending.pop() {
            let sense = solved[&face];
            for (neighbor, parity) in adjacency.get(&face).into_iter().flatten() {
                if !solved.contains_key(neighbor) {
                    solved.insert(neighbor.clone(), sense ^ parity);
                    pending.push(neighbor.clone());
                }
            }
        }
    }
    for face in &mut out.faces {
        face.sense = if solved.get(&face.id).copied().unwrap_or(false) {
            Sense::Reversed
        } else {
            Sense::Forward
        };
    }
}

fn synthesize_cylinder_seams(
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
) {
    let surfaces: HashMap<_, _> = out
        .surfaces
        .iter()
        .map(|surface| (&surface.id, surface))
        .collect();
    let loops: HashMap<_, _> = out.loops.iter().map(|lp| (&lp.id, lp)).collect();
    let coedges: HashMap<_, _> = out
        .coedges
        .iter()
        .map(|coedge| (&coedge.id, coedge))
        .collect();
    let edges: HashMap<_, _> = out.edges.iter().map(|edge| (&edge.id, edge)).collect();
    let curves: HashMap<_, _> = out.curves.iter().map(|curve| (&curve.id, curve)).collect();
    let mut candidates = Vec::new();
    for face in &out.faces {
        let Some(surface) = surfaces.get(&face.surface) else {
            continue;
        };
        let SurfaceGeometry::Cylinder { ref_direction, .. } = surface.geometry else {
            continue;
        };
        if face.loops.len() != 2 {
            continue;
        }
        let Some(a) = loops.get(&face.loops[0]) else {
            continue;
        };
        let Some(b) = loops.get(&face.loops[1]) else {
            continue;
        };
        if a.coedges.len() != 1 || b.coedges.len() != 1 {
            continue;
        }
        let Some(ca) = coedges.get(&a.coedges[0]) else {
            continue;
        };
        let Some(cb) = coedges.get(&b.coedges[0]) else {
            continue;
        };
        let Some(ea) = edges.get(&ca.edge) else {
            continue;
        };
        let Some(eb) = edges.get(&cb.edge) else {
            continue;
        };
        let seam_point = |edge: &Edge| {
            if edge.start != edge.end {
                return None;
            }
            let curve = curves.get(edge.curve.as_ref()?)?;
            let CurveGeometry::Circle { center, radius, .. } = curve.geometry else {
                return None;
            };
            Some(cadmpeg_ir::math::Point3::new(
                center.x - ref_direction.x * radius,
                center.y - ref_direction.y * radius,
                center.z - ref_direction.z * radius,
            ))
        };
        if let (Some(pa), Some(pb)) = (seam_point(ea), seam_point(eb)) {
            candidates.push((
                face.id.clone(),
                a.id.clone(),
                b.id.clone(),
                ca.id.clone(),
                cb.id.clone(),
                ea.start.clone(),
                eb.start.clone(),
                pa,
                pb,
            ));
        }
    }

    let mut removed = HashSet::new();
    let mut coedge_indices = out
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| (coedge.id.clone(), index))
        .collect::<HashMap<_, _>>();
    for (face_id, loop_a, loop_b, circle_a, circle_b, vertex_a, vertex_b, pa, pb) in candidates {
        for (vertex_id, position) in [(&vertex_a, pa), (&vertex_b, pb)] {
            let Some(point_id) = out
                .vertices
                .iter()
                .find(|vertex| vertex.id == *vertex_id)
                .map(|vertex| vertex.point.clone())
            else {
                continue;
            };
            if let Some(point) = out.points.iter_mut().find(|point| point.id == point_id) {
                point.position = position;
            }
        }
        let direction = cadmpeg_ir::math::Vector3::new(pb.x - pa.x, pb.y - pa.y, pb.z - pa.z);
        let norm = direction.norm();
        if norm == 0.0 {
            continue;
        }
        let direction = cadmpeg_ir::math::Vector3::new(
            direction.x / norm,
            direction.y / norm,
            direction.z / norm,
        );
        let suffix = face_id.0.rsplit('#').next().unwrap_or("0");
        let curve_id = CurveId(format!("sldprt:brep:curve#seam:{suffix}"));
        let edge_id = EdgeId(format!("sldprt:brep:edge#seam:{suffix}"));
        let seam_a = CoedgeId(format!("sldprt:brep:coedge#seam:{suffix}:0"));
        let seam_b = CoedgeId(format!("sldprt:brep:coedge#seam:{suffix}:1"));
        for id in [&curve_id.0, &edge_id.0, &seam_a.0, &seam_b.0] {
            annotations
                .note(id, source_stream, 0)
                .tag("derived_periodic_seam");
            annotations.exactness(id, Exactness::Derived);
        }
        out.curves.push(Curve {
            id: curve_id.clone(),
            source_object: None,
            geometry: CurveGeometry::Line {
                origin: pa,
                direction,
            },
        });
        out.edges.push(Edge {
            id: edge_id.clone(),
            curve: Some(curve_id),
            start: vertex_a,
            end: vertex_b,
            param_range: Some([0.0, norm]),
            tolerance: None,
        });
        coedge_indices.insert(seam_a.clone(), out.coedges.len());
        out.coedges.push(Coedge {
            id: seam_a.clone(),
            owner_loop: loop_a.clone(),
            edge: edge_id.clone(),
            next: circle_b.clone(),
            previous: circle_a.clone(),
            radial_next: seam_b.clone(),
            sense: Sense::Forward,
            use_curve: None,
            use_curve_parameter_range: None,
            pcurves: Vec::new(),
        });
        coedge_indices.insert(seam_b.clone(), out.coedges.len());
        out.coedges.push(Coedge {
            id: seam_b.clone(),
            owner_loop: loop_a.clone(),
            edge: edge_id,
            next: circle_a.clone(),
            previous: circle_b.clone(),
            radial_next: seam_a.clone(),
            sense: Sense::Reversed,
            use_curve: None,
            use_curve_parameter_range: None,
            pcurves: Vec::new(),
        });
        let ring = [circle_a.clone(), seam_a, circle_b.clone(), seam_b];
        for (index, id) in ring.iter().enumerate() {
            if let Some(coedge_index) = coedge_indices.get(id) {
                let coedge = &mut out.coedges[*coedge_index];
                coedge.owner_loop = loop_a.clone();
                coedge.previous = ring[(index + 3) % 4].clone();
                coedge.next = ring[(index + 1) % 4].clone();
            }
        }
        if let Some(lp) = out.loops.iter_mut().find(|lp| lp.id == loop_a) {
            lp.coedges = ring.to_vec();
        }
        if let Some(face) = out.faces.iter_mut().find(|face| face.id == face_id) {
            face.loops = vec![loop_a];
        }
        removed.insert(loop_b);
    }
    out.loops.retain(|lp| !removed.contains(&lp.id));
}

fn synthesize_sphere_seams(
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
) {
    let surface_geometry = out
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect::<HashMap<_, _>>();
    let loop_coedges = out
        .loops
        .iter()
        .map(|lp| (&lp.id, &lp.coedges))
        .collect::<HashMap<_, _>>();
    let coedge_edges = out
        .coedges
        .iter()
        .map(|coedge| (&coedge.id, &coedge.edge))
        .collect::<HashMap<_, _>>();
    let edge_indices = out
        .edges
        .iter()
        .enumerate()
        .map(|(index, edge)| (&edge.id, index))
        .collect::<HashMap<_, _>>();
    let curve_geometry = out
        .curves
        .iter()
        .map(|curve| (&curve.id, &curve.geometry))
        .collect::<HashMap<_, _>>();
    let vertex_points = out
        .vertices
        .iter()
        .filter_map(|vertex| {
            out.points
                .iter()
                .find(|point| point.id == vertex.point)
                .map(|point| (&vertex.id, point.position))
        })
        .collect::<HashMap<_, _>>();
    let mut existing = Vec::new();
    for face in &out.faces {
        let Some(SurfaceGeometry::Sphere {
            center,
            radius,
            axis,
            ..
        }) = surface_geometry.get(&face.surface).copied()
        else {
            continue;
        };
        let [loop_id] = face.loops.as_slice() else {
            continue;
        };
        let Some(coedge_ids) = loop_coedges.get(loop_id).copied() else {
            continue;
        };
        if coedge_ids.len() != 4 {
            continue;
        }
        let seam_edges = coedge_ids
            .iter()
            .filter_map(|coedge| coedge_edges.get(coedge).copied())
            .filter_map(|edge| edge_indices.get(edge).map(|index| (edge.clone(), *index)))
            .filter(|(_, index)| out.edges[*index].curve.is_none())
            .collect::<Vec<_>>();
        let circle_count = coedge_ids
            .iter()
            .filter_map(|coedge| coedge_edges.get(coedge).copied())
            .filter_map(|edge| edge_indices.get(edge).copied())
            .filter(|index| {
                out.edges[*index]
                    .curve
                    .as_ref()
                    .and_then(|curve| curve_geometry.get(curve))
                    .is_some_and(|geometry| matches!(geometry, CurveGeometry::Circle { .. }))
            })
            .count();
        if let [(_, edge_index)] = seam_edges.as_slice() {
            if circle_count != 3 {
                continue;
            }
            let north = cadmpeg_ir::math::Point3::new(
                center.x + radius * axis.x,
                center.y + radius * axis.y,
                center.z + radius * axis.z,
            );
            // The analytic sphere axis fixes the seam pole. An existing
            // endpoint is a topology carrier, not a pole selector; choosing
            // the nearer pole lets a stale or reversed endpoint change the
            // sphere parameterization.
            let point = north;
            existing.push((*edge_index, point));
        }
    }
    for (edge_index, point) in existing {
        let seam_vertices = [
            out.edges[edge_index].start.clone(),
            out.edges[edge_index].end.clone(),
        ];
        for vertex_id in seam_vertices {
            let Some(point_id) = out
                .vertices
                .iter()
                .find(|vertex| vertex.id == vertex_id)
                .map(|vertex| vertex.point.clone())
            else {
                continue;
            };
            if let Some(vertex_point) = out.points.iter_mut().find(|item| item.id == point_id) {
                vertex_point.position = point;
            }
        }
        let suffix = out.edges[edge_index].id.0.rsplit('#').next().unwrap_or("0");
        let curve_id = CurveId(format!("sldprt:brep:curve#sphere-seam:{suffix}"));
        annotations
            .note(&curve_id.0, source_stream, 0)
            .tag("derived_sphere_seam");
        annotations.exactness(&curve_id.0, Exactness::Derived);
        out.curves.push(Curve {
            id: curve_id.clone(),
            source_object: None,
            geometry: CurveGeometry::Degenerate { point },
        });
        out.edges[edge_index].curve = Some(curve_id);
    }

    let surfaces: HashMap<_, _> = out
        .surfaces
        .iter()
        .map(|surface| (&surface.id, surface))
        .collect();
    let loops: HashMap<_, _> = out.loops.iter().map(|lp| (&lp.id, lp)).collect();
    let coedges: HashMap<_, _> = out
        .coedges
        .iter()
        .map(|coedge| (&coedge.id, coedge))
        .collect();
    let edges: HashMap<_, _> = out.edges.iter().map(|edge| (&edge.id, edge)).collect();
    let curves: HashMap<_, _> = out.curves.iter().map(|curve| (&curve.id, curve)).collect();
    let mut candidates = Vec::new();
    for (face_index, face) in out.faces.iter().enumerate() {
        let Some(surface) = surfaces.get(&face.surface) else {
            continue;
        };
        let SurfaceGeometry::Sphere { center, radius, .. } = surface.geometry else {
            continue;
        };
        if face.loops.len() != 1 {
            continue;
        }
        let Some(lp) = loops.get(&face.loops[0]) else {
            continue;
        };
        if lp.coedges.len() != 3 {
            continue;
        }
        let all_circles = lp.coedges.iter().all(|id| {
            coedges
                .get(id)
                .and_then(|coedge| edges.get(&coedge.edge))
                .and_then(|edge| edge.curve.as_ref())
                .is_some_and(|curve_id| {
                    curves
                        .get(curve_id)
                        .is_some_and(|curve| matches!(curve.geometry, CurveGeometry::Circle { .. }))
                })
        });
        let SurfaceGeometry::Sphere { axis, .. } = surface.geometry else {
            continue;
        };
        if all_circles {
            let seam_point = cadmpeg_ir::math::Point3::new(
                center.x + radius * axis.x,
                center.y + radius * axis.y,
                center.z + radius * axis.z,
            );
            let mut pole_vertices = lp
                .coedges
                .iter()
                .filter_map(|id| coedges.get(id))
                .filter_map(|coedge| edges.get(&coedge.edge))
                .flat_map(|edge| [&edge.start, &edge.end])
                .filter(|vertex| {
                    vertex_points.get(vertex).is_some_and(|point| {
                        let dx = point.x - seam_point.x;
                        let dy = point.y - seam_point.y;
                        let dz = point.z - seam_point.z;
                        dx * dx + dy * dy + dz * dz <= 1e-12
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            pole_vertices.sort_by(|left, right| left.0.cmp(&right.0));
            pole_vertices.dedup();
            candidates.push((
                face_index,
                face.id.clone(),
                lp.id.clone(),
                lp.coedges.clone(),
                seam_point,
                pole_vertices.first().cloned(),
            ));
        }
    }
    let mut coedge_indices = out
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| (coedge.id.clone(), index))
        .collect::<HashMap<_, _>>();
    for (face_index, _face, loop_id, mut ring, seam_point, pole_vertex) in candidates {
        let curve_id = CurveId(format!("sldprt:brep:curve#sphere-seam-face:{face_index}"));
        let edge_id = EdgeId(format!("sldprt:brep:edge#sphere-seam-face:{face_index}"));
        let coedge_id = CoedgeId(format!("sldprt:brep:coedge#sphere-seam-face:{face_index}"));
        let pcurve_id = PcurveId(format!("sldprt:brep:pcurve#sphere-seam-face:{face_index}"));
        let pole_vertex = pole_vertex.unwrap_or_else(|| {
            let point_id = PointId(format!("sldprt:brep:point#sphere-seam-face:{face_index}"));
            let vertex_id = VertexId(format!("sldprt:brep:vertex#sphere-seam-face:{face_index}"));
            for id in [&point_id.0, &vertex_id.0] {
                annotations
                    .note(id, source_stream, 0)
                    .tag("derived_sphere_seam");
                annotations.exactness(id, Exactness::Derived);
            }
            out.points.push(Point {
                id: point_id.clone(),
                position: seam_point,
                source_object: None,
            });
            out.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            vertex_id
        });
        for id in [&curve_id.0, &edge_id.0, &coedge_id.0, &pcurve_id.0] {
            annotations
                .note(id, source_stream, 0)
                .tag("derived_sphere_seam");
            annotations.exactness(id, Exactness::Derived);
        }
        out.curves.push(Curve {
            id: curve_id.clone(),
            source_object: None,
            geometry: CurveGeometry::Degenerate { point: seam_point },
        });
        out.edges.push(Edge {
            id: edge_id.clone(),
            curve: Some(curve_id),
            start: pole_vertex.clone(),
            end: pole_vertex,
            param_range: None,
            tolerance: None,
        });
        out.pcurves.push(Pcurve {
            id: pcurve_id.clone(),
            geometry: PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(0.0, std::f64::consts::FRAC_PI_2),
                direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
            },
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: Some([0.0, std::f64::consts::TAU]),
            fit_tolerance: None,
        });
        ring.push(coedge_id.clone());
        coedge_indices.insert(coedge_id.clone(), out.coedges.len());
        out.coedges.push(Coedge {
            id: coedge_id.clone(),
            owner_loop: loop_id.clone(),
            edge: edge_id,
            next: ring[0].clone(),
            previous: ring[2].clone(),
            radial_next: coedge_id.clone(),
            sense: Sense::Forward,
            use_curve: None,
            use_curve_parameter_range: None,
            pcurves: vec![cadmpeg_ir::topology::PcurveUse {
                pcurve: pcurve_id,
                isoparametric: None,
                parameter_range: Some([0.0, std::f64::consts::TAU]),
            }],
        });
        for (index, id) in ring.iter().enumerate() {
            if let Some(coedge_index) = coedge_indices.get(id) {
                let coedge = &mut out.coedges[*coedge_index];
                coedge.next = ring[(index + 1) % ring.len()].clone();
                coedge.previous = ring[(index + ring.len() - 1) % ring.len()].clone();
            }
        }
        if let Some(lp) = out.loops.iter_mut().find(|lp| lp.id == loop_id) {
            lp.coedges = ring;
        }
    }
}

fn emit_curve(out: &mut Brep, carrier: &Carrier) {
    if let CarrierGeometry::Curve(geo) = &carrier.geometry {
        out.curves.push(Curve {
            id: CurveId(id_curve(carrier.attr)),
            source_object: None,
            geometry: geo.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::unique_face_colors;
    use crate::brep::entity;
    use crate::brep::topology::{Record, Tables};
    use cadmpeg_ir::topology::Color;

    #[test]
    fn line_edge_parameters_convert_from_metres_to_millimetres() {
        let carrier = crate::brep::Carrier {
            attr: 1,
            offset: 0,
            end: 0,
            geometry: crate::brep::CarrierGeometry::Curve(
                cadmpeg_ir::geometry::CurveGeometry::Line {
                    origin: cadmpeg_ir::math::Point3::new(0.0, 17.5, 0.0),
                    direction: cadmpeg_ir::math::Vector3::new(0.0, -1.0, 0.0),
                },
            ),
            frame: None,
            parameter_range: Some([-0.014, 0.0165]),
            orientation_reversed: false,
        };

        let endpoints = [
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 31.5, 0.0),
        ];
        assert_eq!(
            super::edge_parameter_range(&carrier, Some(endpoints)),
            Some(([-14.0, 16.5], true))
        );
    }

    fn topology_record(attr: u16, refs: Vec<u16>) -> Record {
        Record {
            attr,
            sequence: None,
            refs,
            marker: None,
            xyz_m: None,
            xyz_offset: None,
            owner: None,
            offset: 0,
        }
    }

    #[test]
    fn face_walk_rejects_a_loop_owned_by_another_bridge() {
        let bridge = topology_record(10, vec![0, 0, 20, 0, 30]);
        let mut tables = Tables::default();
        tables
            .loops
            .insert(20, topology_record(20, vec![0, 40, 11, 0]));
        tables
            .coedges
            .insert(40, topology_record(40, vec![0, 0, 0, 40]));

        let face = super::walk_face(&bridge, &tables);

        assert!(face.loops.is_empty());
    }

    #[test]
    fn face_walk_rejects_a_ring_owned_by_another_loop() {
        let bridge = topology_record(10, vec![0, 0, 20, 0, 30]);
        let mut tables = Tables::default();
        tables
            .loops
            .insert(20, topology_record(20, vec![0, 40, 10, 0]));
        tables
            .coedges
            .insert(40, topology_record(40, vec![0, 21, 0, 40]));

        let face = super::walk_face(&bridge, &tables);

        assert!(face.loops.is_empty());
    }

    fn face_color(
        face_attr: u16,
        color_attr: u16,
        face_seq: u32,
        rgb: [f32; 3],
    ) -> entity::FaceColor {
        entity::FaceColor {
            face_attr,
            color_attr,
            face_seq,
            stream_order: 0,
            color: Color {
                r: rgb[0],
                g: rgb[1],
                b: rgb[2],
                a: 1.0,
            },
            offset: usize::from(face_attr),
            target: None,
        }
    }

    fn face_color_version(
        face_attr: u16,
        seq: u32,
        stream_order: usize,
    ) -> entity::FaceColorVersion {
        entity::FaceColorVersion {
            face_attr,
            seq,
            stream_order,
        }
    }

    #[test]
    fn current_uncolored_face_version_removes_an_older_color() {
        let colors = vec![face_color(700, 900, 1, [0.25, 0.5, 0.75])];

        let (resolved, unresolved) = unique_face_colors(
            colors,
            vec![face_color_version(700, 1, 0), face_color_version(700, 2, 0)],
        );

        assert!(resolved.is_empty());
        assert_eq!(unresolved, 0);
    }

    #[test]
    fn later_stream_replaces_an_equal_sequence_face_color() {
        let old = face_color(700, 900, 2, [0.25, 0.5, 0.75]);
        let mut current = face_color(700, 901, 2, [0.75, 0.5, 0.25]);
        current.stream_order = 1;

        let (resolved, unresolved) = unique_face_colors(
            vec![old, current],
            vec![face_color_version(700, 2, 0), face_color_version(700, 2, 1)],
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].color_attr, 901);
        assert_eq!(unresolved, 0);
    }

    #[test]
    fn conflicting_current_face_colors_remain_unresolved() {
        let colors = vec![
            face_color(700, 900, 2, [0.25, 0.5, 0.75]),
            face_color(700, 901, 2, [0.75, 0.5, 0.25]),
        ];

        let (resolved, unresolved) = unique_face_colors(
            colors,
            vec![face_color_version(700, 2, 0), face_color_version(700, 2, 0)],
        );

        assert!(resolved.is_empty());
        assert_eq!(unresolved, 1);
    }

    #[test]
    fn conflicting_reuse_of_one_color_identity_remains_unresolved() {
        let colors = vec![
            face_color(700, 900, 2, [0.25, 0.5, 0.75]),
            face_color(701, 900, 2, [0.75, 0.5, 0.25]),
        ];

        let (resolved, unresolved) = unique_face_colors(
            colors,
            vec![face_color_version(700, 2, 0), face_color_version(701, 2, 0)],
        );

        assert!(resolved.is_empty());
        assert_eq!(unresolved, 2);
    }

    #[test]
    fn intersection_uv_converts_length_parameters_and_exact_endpoints() {
        let surface = cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };
        let endpoints = [
            cadmpeg_ir::eval::surface_point(&surface, 0.0, 3.0).expect("cylinder start"),
            cadmpeg_ir::eval::surface_point(&surface, 0.5, 2.0).expect("cylinder end"),
        ];
        let chart = cadmpeg_ir::geometry::NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: endpoints.to_vec(),
            weights: None,
            periodic: false,
        };
        let support_data = super::super::intersection::IntersectionSupportData {
            supports: [10, 11],
            fit_tolerance_mm: 0.2,
            support_uv: Some([
                vec![
                    cadmpeg_ir::math::Point2::new(0.0, 0.0029),
                    cadmpeg_ir::math::Point2::new(0.5, 0.0018),
                ],
                vec![
                    cadmpeg_ir::math::Point2::new(0.0, 0.0),
                    cadmpeg_ir::math::Point2::new(1.0, 0.0),
                ],
            ]),
        };
        let (geometry, range, source) =
            super::intersection_support_pcurve(&support_data, &chart, 10, &surface, endpoints)
                .expect("support parameterization");
        let cadmpeg_ir::geometry::PcurveGeometry::Nurbs { control_points, .. } = geometry else {
            panic!("expected solved UV NURBS");
        };
        assert_eq!(control_points[0], cadmpeg_ir::math::Point2::new(0.0, 3.0));
        assert_eq!(control_points[1], cadmpeg_ir::math::Point2::new(0.5, 2.0));
        assert_eq!(range, [0.0, 1.0]);
        assert_eq!(source, super::IntersectionPcurveSource::StoredCache);

        let ambiguous = super::super::intersection::IntersectionSupportData {
            supports: [10, 10],
            ..support_data.clone()
        };
        assert!(
            super::intersection_support_pcurve(&ambiguous, &chart, 10, &surface, endpoints)
                .is_none()
        );

        let malformed = super::super::intersection::IntersectionSupportData {
            support_uv: Some([Vec::new(), Vec::new()]),
            ..support_data
        };
        assert!(
            super::intersection_support_pcurve(&malformed, &chart, 10, &surface, endpoints)
                .is_none()
        );
    }

    #[test]
    fn analytic_intersection_chart_derives_continuous_uv_without_cache() {
        let surface = cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };
        let model_points = [(3.0, 1.0), (3.2, 2.0), (3.4, 3.0)]
            .map(|(u, v)| cadmpeg_ir::eval::surface_point(&surface, u, v).expect("cylinder point"))
            .to_vec();
        let endpoints = [model_points[0], model_points[2]];
        let chart = cadmpeg_ir::geometry::NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
            control_points: model_points,
            weights: None,
            periodic: false,
        };
        let support_data = super::super::intersection::IntersectionSupportData {
            supports: [10, 11],
            fit_tolerance_mm: 0.011,
            support_uv: None,
        };

        let (geometry, _, source) =
            super::intersection_support_pcurve(&support_data, &chart, 10, &surface, endpoints)
                .expect("analytic support inversion");
        let cadmpeg_ir::geometry::PcurveGeometry::Nurbs { control_points, .. } = geometry else {
            panic!("expected solved UV NURBS");
        };
        for (point, expected) in control_points
            .iter()
            .zip([(3.0, 1.0), (3.2, 2.0), (3.4, 3.0)])
        {
            assert!((point.u - expected.0).abs() < 1.0e-12);
            assert!((point.v - expected.1).abs() < 1.0e-12);
        }
        assert_eq!(source, super::IntersectionPcurveSource::AnalyticInverse);

        let under_toleranced = super::super::intersection::IntersectionSupportData {
            fit_tolerance_mm: 0.009,
            ..support_data
        };
        assert!(super::intersection_support_pcurve(
            &under_toleranced,
            &chart,
            10,
            &surface,
            endpoints
        )
        .is_none());
    }

    #[test]
    fn analytic_torus_chart_unwraps_both_periodic_parameters() {
        let surface = cadmpeg_ir::geometry::SurfaceGeometry::Torus {
            center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            major_radius: 5.0,
            minor_radius: 1.0,
        };
        let expected = [(3.0, 3.0), (3.2, 3.2), (3.4, 3.4)];
        let model_points = expected
            .map(|(u, v)| cadmpeg_ir::eval::surface_point(&surface, u, v).expect("torus point"))
            .to_vec();
        let endpoints = [model_points[0], model_points[2]];
        let chart = cadmpeg_ir::geometry::NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
            control_points: model_points,
            weights: None,
            periodic: false,
        };
        let support_data = super::super::intersection::IntersectionSupportData {
            supports: [10, 11],
            fit_tolerance_mm: 0.08,
            support_uv: None,
        };

        let (geometry, _, _) =
            super::intersection_support_pcurve(&support_data, &chart, 10, &surface, endpoints)
                .expect("torus support inversion");
        let cadmpeg_ir::geometry::PcurveGeometry::Nurbs { control_points, .. } = geometry else {
            panic!("expected solved UV NURBS");
        };
        for (point, expected) in control_points.iter().zip(expected) {
            assert!((point.u - expected.0).abs() < 1.0e-12);
            assert!((point.v - expected.1).abs() < 1.0e-12);
        }
    }

    #[test]
    fn nurbs_intersection_chart_inverts_with_continuation_seeds() {
        let nurbs = cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        };
        let surface = cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs.clone());
        let expected = [(0.2, 0.1), (0.5, 0.4), (0.8, 0.7)];
        let model_points = expected
            .map(|(u, v)| {
                cadmpeg_ir::eval::nurbs_surface_point(&nurbs, u, v).expect("surface point")
            })
            .to_vec();
        let endpoints = [model_points[0], model_points[2]];
        let chart = cadmpeg_ir::geometry::NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
            control_points: model_points,
            weights: None,
            periodic: false,
        };
        let support_data = super::super::intersection::IntersectionSupportData {
            supports: [10, 11],
            fit_tolerance_mm: 1.0e-9,
            support_uv: None,
        };

        let (geometry, _, source) =
            super::intersection_support_pcurve(&support_data, &chart, 10, &surface, endpoints)
                .expect("NURBS support inversion");
        let cadmpeg_ir::geometry::PcurveGeometry::Nurbs { control_points, .. } = geometry else {
            panic!("expected solved UV NURBS");
        };
        for (point, expected) in control_points.iter().zip(expected) {
            assert!((point.u - expected.0).abs() < 1.0e-10);
            assert!((point.v - expected.1).abs() < 1.0e-10);
        }
        assert_eq!(source, super::IntersectionPcurveSource::NurbsInverse);
    }

    #[test]
    fn nurbs_intersection_chart_requires_a_complete_chord_certificate() {
        let nurbs = cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        };
        let surface = cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs);
        let endpoints = [
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
        ];
        let chart = cadmpeg_ir::geometry::NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: endpoints.to_vec(),
            weights: None,
            periodic: false,
        };
        let support_data = |fit_tolerance_mm| super::super::intersection::IntersectionSupportData {
            supports: [10, 11],
            fit_tolerance_mm,
            support_uv: None,
        };

        assert!(super::intersection_support_pcurve(
            &support_data(0.3),
            &chart,
            10,
            &surface,
            endpoints,
        )
        .is_none());
        assert!(super::intersection_support_pcurve(
            &support_data(0.34),
            &chart,
            10,
            &surface,
            endpoints,
        )
        .is_some());
    }

    #[test]
    fn canonical_edge_direction_uses_explicit_or_unique_forward_coedge() {
        use std::collections::HashMap;

        let record = |attr, refs, marker| super::Record {
            attr,
            sequence: None,
            refs,
            marker,
            xyz_m: None,
            xyz_offset: None,
            owner: None,
            offset: 0,
        };
        let mut coedges = HashMap::from([
            (10, record(10, vec![0, 0, 0, 0, 101, 0, 7], Some(0x2d))),
            (11, record(11, vec![0, 0, 0, 0, 102, 10, 7], Some(0x2b))),
        ]);
        let prefixed_edge = record(7, vec![0, 0, 0, 300, 0, 0], None);

        assert_eq!(
            super::canonical_coedge_attr(7, Some(&prefixed_edge), &coedges),
            Some(11)
        );

        let bare_edge = record(7, vec![11, 0, 0, 300, 0, 0], None);
        assert_eq!(
            super::canonical_coedge_attr(7, Some(&bare_edge), &coedges),
            Some(11)
        );

        let sentinel_edge = record(7, vec![1, 0, 0, 300, 0, 0], None);
        assert_eq!(
            super::canonical_coedge_attr(7, Some(&sentinel_edge), &coedges),
            Some(11)
        );

        let reversed_edge = record(7, vec![10, 0, 0, 300, 0, 0], None);
        assert_eq!(
            super::canonical_coedge_attr(7, Some(&reversed_edge), &coedges),
            None
        );

        coedges.insert(12, record(12, vec![0, 0, 0, 0, 103, 0, 7], Some(0x2b)));
        assert_eq!(
            super::canonical_coedge_attr(7, Some(&prefixed_edge), &coedges),
            None
        );
    }

    #[test]
    fn boundary_coedge_uses_ring_endpoint_but_reciprocal_twin_supplies_edge_end() {
        use std::collections::HashMap;

        let record = |attr, refs| super::Record {
            attr,
            sequence: None,
            refs,
            marker: Some(0x2b),
            xyz_m: None,
            xyz_offset: None,
            owner: None,
            offset: 0,
        };
        let boundary = HashMap::from([(10, record(10, vec![0, 0, 0, 0, 101, 10, 7]))]);
        assert_eq!(super::edge_end_vuse(10, 102, &boundary), 102);

        let reciprocal = HashMap::from([
            (10, record(10, vec![0, 0, 0, 0, 101, 11, 7])),
            (11, record(11, vec![0, 0, 0, 0, 102, 10, 7])),
        ]);
        assert_eq!(super::edge_end_vuse(10, 103, &reciprocal), 102);
    }

    #[test]
    fn normalized_surface_parameter_reversal_toggles_face_sense() {
        use cadmpeg_ir::topology::Sense;

        assert_eq!(super::surface_sense(0x2b, false), Sense::Forward);
        assert_eq!(super::surface_sense(0x2d, false), Sense::Reversed);
        assert_eq!(super::surface_sense(0x2b, true), Sense::Reversed);
        assert_eq!(super::surface_sense(0x2d, true), Sense::Forward);
    }

    #[test]
    fn shared_edge_coedge_parity_orients_connected_faces() {
        use cadmpeg_ir::ids::{CoedgeId, EdgeId, FaceId, LoopId, ShellId, SurfaceId};
        use cadmpeg_ir::topology::{Coedge, Face, Loop, Sense};

        let face = |id: &str, lp: &str| Face {
            id: FaceId(id.into()),
            shell: ShellId("shell".into()),
            surface: SurfaceId(format!("surface-{id}")),
            sense: Sense::Forward,
            loops: vec![LoopId(lp.into())],
            name: None,
            color: None,
            tolerance: None,
        };
        let lp = |id: &str, face: &str, coedge: &str| Loop {
            id: LoopId(id.into()),
            face: FaceId(face.into()),
            boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
            coedges: vec![CoedgeId(coedge.into())],
            vertex_uses: Vec::new(),
        };
        let coedge = |id: &str, lp: &str, radial: &str, sense| Coedge {
            id: CoedgeId(id.into()),
            owner_loop: LoopId(lp.into()),
            edge: EdgeId("edge".into()),
            next: CoedgeId(id.into()),
            previous: CoedgeId(id.into()),
            radial_next: CoedgeId(radial.into()),
            sense,
            use_curve: None,
            use_curve_parameter_range: None,
            pcurves: Vec::new(),
        };
        let mut brep = super::Brep {
            faces: vec![face("face-a", "loop-a"), face("face-b", "loop-b")],
            loops: vec![
                lp("loop-a", "face-a", "coedge-a"),
                lp("loop-b", "face-b", "coedge-b"),
            ],
            coedges: vec![
                coedge("coedge-a", "loop-a", "coedge-b", Sense::Forward),
                coedge("coedge-b", "loop-b", "coedge-a", Sense::Forward),
            ],
            ..Default::default()
        };

        super::solve_face_orientation(&mut brep);
        assert_eq!(brep.faces[0].sense, Sense::Forward);
        assert_eq!(brep.faces[1].sense, Sense::Reversed);

        brep.faces[1].sense = Sense::Reversed;
        brep.coedges[1].sense = Sense::Reversed;
        super::solve_face_orientation(&mut brep);
        assert_eq!(brep.faces[0].sense, Sense::Forward);
        assert_eq!(brep.faces[1].sense, Sense::Forward);
    }

    #[test]
    fn geometry_free_stream_does_not_report_synthetic_body_grouping() {
        let decoded = super::decode_body(&[], "empty");

        assert!(decoded.faces.is_empty());
        assert!(!decoded.stats.synthetic_body_grouping);
    }

    #[test]
    fn typed_body_records_preserve_stored_sheet_kind_and_links() {
        use crate::brep::typed::{BodyNode, FaceNode, Facts, RegionNode, ShellNode};
        use cadmpeg_ir::topology::BodyKind;
        use std::collections::HashSet;

        let facts = Facts {
            bodies: vec![BodyNode {
                attr: 3,
                node_id: 7,
                topology_refs: [7, 8, 9, 10, 1, 12, 11],
                ownership_refs: Vec::new(),
                body_type: 3,
                offset: 1,
                end: 2,
            }],
            shells: vec![ShellNode {
                attr: 7,
                node_id: 814,
                refs: [1, 3, 1, 38, 1, 1, 39, 1],
                offset: 3,
                end: 4,
            }],
            regions: vec![
                RegionNode {
                    attr: 11,
                    node_id: 244,
                    refs: [1, 3, 39, 1, 44],
                    kind: b'V',
                    offset: 5,
                    end: 6,
                },
                RegionNode {
                    attr: 39,
                    node_id: 815,
                    refs: [1, 3, 1, 11, 7],
                    kind: b'S',
                    offset: 7,
                    end: 8,
                },
            ],
            faces: vec![FaceNode {
                attr: 100,
                node_id: 900,
                attribute_chain: 1,
                refs: [1, 1, 49, 7, 8],
                sense: 0x2b,
                offset: 9,
                end: 10,
            }],
        };
        let mut tables = Tables::default();
        tables.bridges.insert(
            100,
            Record {
                attr: 100,
                sequence: None,
                refs: vec![1, 1, 49, 7, 8],
                marker: Some(0x2b),
                xyz_m: None,
                xyz_offset: None,
                owner: None,
                offset: 11,
            },
        );

        let records = super::typed_body_records(&facts, &tables).expect("typed body records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, BodyKind::Sheet);
        assert_eq!(records[0].regions.len(), 2);
        assert!(records[0].refs.contains(&100));
        assert_eq!(records[0].regions[1].shells[0].refs, vec![100]);
        assert_eq!(
            facts
                .hierarchies(&HashSet::from([100]))
                .expect("typed hierarchy")[0]
                .kind,
            BodyKind::Sheet
        );
    }

    #[test]
    fn ambiguous_face_owner_stats_survive_when_all_uses_are_withheld() {
        let bridge = |attr, surface, offset| super::Record {
            attr,
            sequence: None,
            refs: vec![0, 0, 0, 0, surface],
            marker: Some(0x2b),
            xyz_m: None,
            xyz_offset: None,
            owner: Some(700),
            offset,
        };
        let mut tables = super::topology::Tables::default();
        tables.bridges.insert(10, bridge(10, 100, 20));
        tables.bridges.insert(11, bridge(11, 200, 10));
        let decoded = super::decode_graph(
            &super::CarrierIndex::default(),
            &tables,
            super::entity::Facts {
                entity_count: 1,
                ..Default::default()
            },
            &super::typed::Facts::default(),
            "empty",
        );

        assert!(decoded.faces.is_empty());
        assert_eq!(decoded.stats.ambiguous_face_owners, 1);
    }

    #[test]
    fn topology_pruning_retains_a_procedural_blend_spine() {
        use cadmpeg_ir::geometry::{
            BlendCrossSection, BlendRadiusLaw, Curve, CurveGeometry, ProceduralSurface,
            ProceduralSurfaceDefinition,
        };
        use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};

        let spine = CurveId("spine".into());
        let mut brep = super::Brep {
            curves: vec![Curve {
                id: spine.clone(),
                geometry: CurveGeometry::Line {
                    origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            }],
            procedural_surfaces: vec![ProceduralSurface {
                id: ProceduralSurfaceId("blend".into()),
                surface: SurfaceId("surface".into()),
                definition: ProceduralSurfaceDefinition::Blend {
                    supports: [None, None],
                    spine: Some(spine.clone()),
                    radius: BlendRadiusLaw::Constant { signed_radius: 0.5 },
                    cross_section: BlendCrossSection::Circular,
                    native: None,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            }],
            ..Default::default()
        };

        super::prune_rejected_topology(&mut brep);
        assert_eq!(brep.curves.first().map(|curve| &curve.id), Some(&spine));
    }

    #[test]
    fn homogeneous_quadratic_identity_proves_constant_radius() {
        let radius = 2.0;
        let controls = [
            cadmpeg_ir::math::Point2::new(radius, 0.0),
            cadmpeg_ir::math::Point2::new(radius, radius),
            cadmpeg_ir::math::Point2::new(0.0, radius),
        ];
        let weights = [1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0];
        let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        assert!(super::quadratic_nurbs_has_constant_radius(
            &controls,
            Some(&weights),
            &knots,
            radius,
        ));

        let mut invalid = controls;
        invalid[1].u += 0.01;
        assert!(!super::quadratic_nurbs_has_constant_radius(
            &invalid,
            Some(&weights),
            &knots,
            radius,
        ));
    }

    #[test]
    fn interior_ruled_surface_line_has_affine_isoparametric_inverse() {
        let surface = cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(2.0, 1.0, 0.0),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        };
        let geometry = match super::ruled_surface_line_pcurve(
            &surface,
            cadmpeg_ir::geometry::SurfaceParameterAxis::V,
            cadmpeg_ir::math::Point3::new(0.0, 0.5, 0.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        ) {
            super::InverseResolution::Unique(geometry) => geometry,
            super::InverseResolution::NoMatch => panic!("interior ruling did not match"),
            super::InverseResolution::Ambiguous => panic!("interior ruling was ambiguous"),
        };
        let cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction } = geometry else {
            panic!("expected affine line pcurve");
        };
        assert!(origin.u.abs() < 1e-12);
        assert!((origin.v - 0.5).abs() < 1e-12);
        assert!((direction.u - 2.0 / 3.0).abs() < 1e-12);
        assert!(direction.v.abs() < 1e-12);
    }

    #[test]
    fn interior_linear_axis_rational_nurbs_isocurve_has_exact_pcurve() {
        let surface = cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 2,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            v_knots: vec![-0.1, -0.1, 0.9, 0.9],
            u_count: 3,
            v_count: 2,
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, -1.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 3.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, -1.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 3.0),
                cadmpeg_ir::math::Point3::new(2.0, 0.0, -1.0),
                cadmpeg_ir::math::Point3::new(2.0, 0.0, 3.0),
            ],
            weights: Some(vec![1.0, 1.0, 2.0, 2.0, 1.0, 1.0]),
            u_periodic: false,
            v_periodic: false,
        };
        let curve = cadmpeg_ir::geometry::NurbsCurve {
            degree: 2,
            knots: surface.u_knots.clone(),
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
            ],
            weights: Some(vec![1.0, 2.0, 1.0]),
            periodic: false,
        };
        let geometry = match super::nurbs_isocurve_pcurve(&surface, &curve) {
            super::InverseResolution::Unique(geometry) => geometry,
            super::InverseResolution::NoMatch => panic!("interior isocurve did not match"),
            super::InverseResolution::Ambiguous => panic!("interior isocurve was ambiguous"),
        };
        let cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction } = geometry else {
            panic!("expected isoparametric line pcurve");
        };
        assert!(origin.u.abs() < 1e-12);
        assert!((origin.v - 0.15).abs() < 1e-12);
        assert!((direction.u - 1.0).abs() < 1e-12);
        assert!(direction.v.abs() < 1e-12);
    }

    #[test]
    fn extended_nurbs_isocurve_clamps_the_carrier_before_matching() {
        let surface = cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        };
        let curve = cadmpeg_ir::geometry::NurbsCurve {
            degree: 1,
            knots: vec![-1.0, -1.0, 2.0, 2.0],
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.5, -1.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.5, 2.0, 0.0),
            ],
            weights: None,
            periodic: false,
        };
        let resolution = super::derive_nurbs_edge_pcurve(&surface, &curve, [0.2, 0.8]);
        let super::NurbsPcurveResolution::Exact(cadmpeg_ir::geometry::PcurveGeometry::Line {
            origin,
            direction,
        }) = resolution
        else {
            panic!("extended isocurve was not certified");
        };
        assert!((origin.u - 0.5).abs() < 1e-12);
        assert!(origin.v.abs() < 1e-12);
        assert!(direction.u.abs() < 1e-12);
        assert!((direction.v - 1.0).abs() < 1e-12);
        let clamped =
            super::clamp_nurbs_curve_to_domain(&curve, [0.0, 1.0]).expect("clamped segment");
        let expected = cadmpeg_ir::eval::nurbs_surface_isocurve(
            &surface,
            cadmpeg_ir::geometry::SurfaceParameterAxis::U,
            0.5,
        )
        .expect("surface isocurve");
        assert!(super::nurbs_representation_matches(&expected, &clamped));
    }

    #[test]
    fn extended_quadratic_isocurve_preserves_the_inserted_homogeneous_segment() {
        let surface = cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 1,
            v_degree: 2,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            u_count: 2,
            v_count: 3,
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.5, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.5, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        };
        let curve = cadmpeg_ir::geometry::NurbsCurve {
            degree: 2,
            knots: vec![-1.0, -1.0, -1.0, 2.0, 2.0, 2.0],
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.5, -1.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.5, 0.5, 0.0),
                cadmpeg_ir::math::Point3::new(0.5, 2.0, 0.0),
            ],
            weights: None,
            periodic: false,
        };
        let resolution = super::derive_nurbs_edge_pcurve(&surface, &curve, [0.1, 0.9]);
        assert!(matches!(
            resolution,
            super::NurbsPcurveResolution::Exact(
                cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction }
            ) if (origin.u - 0.5).abs() <= f64::EPSILON * 64.0
                && origin.v.abs() <= f64::EPSILON * 64.0
                && direction.u.abs() <= f64::EPSILON * 64.0
                && (direction.v - 1.0).abs() <= f64::EPSILON * 64.0
        ));
        let clamped = super::clamp_nurbs_curve_to_domain(&curve, [0.0, 1.0])
            .expect("clamped quadratic segment");
        let expected = cadmpeg_ir::eval::nurbs_surface_isocurve(
            &surface,
            cadmpeg_ir::geometry::SurfaceParameterAxis::U,
            0.5,
        )
        .expect("quadratic surface isocurve");
        assert!(super::nurbs_representation_matches(&expected, &clamped));
    }

    #[test]
    fn extended_rational_isocurve_compares_weights_after_homogeneous_clamping() {
        let surface = cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ],
            weights: Some(vec![1.0, 1.2, 1.0, 1.2]),
            u_periodic: false,
            v_periodic: false,
        };
        let curve = cadmpeg_ir::geometry::NurbsCurve {
            degree: 1,
            knots: vec![-1.0, -1.0, 2.0, 2.0],
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.5, -1.5, 0.0),
                cadmpeg_ir::math::Point3::new(0.5, 2.4 / 1.4, 0.0),
            ],
            weights: Some(vec![0.8, 1.4]),
            periodic: false,
        };
        let resolution = super::derive_nurbs_edge_pcurve(&surface, &curve, [0.2, 0.8]);
        assert!(matches!(
            resolution,
            super::NurbsPcurveResolution::Exact(
                cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction }
            ) if (origin.u - 0.5).abs() <= f64::EPSILON * 64.0
                && origin.v.abs() <= f64::EPSILON * 64.0
                && direction.u.abs() <= f64::EPSILON * 64.0
                && (direction.v - 1.0).abs() <= f64::EPSILON * 64.0
        ));
        let clamped = super::clamp_nurbs_curve_to_domain(&curve, [0.0, 1.0])
            .expect("clamped rational segment");
        let expected = cadmpeg_ir::eval::nurbs_surface_isocurve(
            &surface,
            cadmpeg_ir::geometry::SurfaceParameterAxis::U,
            0.5,
        )
        .expect("rational surface isocurve");
        assert!(super::nurbs_representation_matches(&expected, &clamped));
    }

    #[test]
    fn degree_one_nurbs_cache_pcurve_keeps_measured_chordal_error() {
        let surface = cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 2,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 3,
            v_count: 2,
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.5, 0.0, 1.0),
                cadmpeg_ir::math::Point3::new(0.5, 1.0, 1.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        };
        let curve = cadmpeg_ir::geometry::NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            ],
            weights: None,
            periodic: false,
        };
        let resolution = super::derive_nurbs_edge_pcurve(&surface, &curve, [0.0, 1.0]);
        let super::NurbsPcurveResolution::Cache {
            geometry:
                cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
                    degree,
                    knots,
                    control_points,
                    weights,
                    periodic,
                },
            fit_tolerance,
        } = resolution
        else {
            panic!("degree-one cache was not accepted");
        };
        assert_eq!(degree, 1);
        assert_eq!(knots, curve.knots);
        assert_eq!(control_points.len(), 2);
        assert!(weights.is_none());
        assert!(!periodic);
        assert!(fit_tolerance > 0.4);
        assert!(fit_tolerance < 0.6);
    }

    #[test]
    fn off_surface_nurbs_edge_is_classified_before_cache_inversion() {
        let surface = cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        };
        let curve = cadmpeg_ir::geometry::NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 10.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 10.0),
            ],
            weights: None,
            periodic: false,
        };
        assert!(matches!(
            super::derive_nurbs_edge_pcurve(&surface, &curve, [0.0, 1.0]),
            super::NurbsPcurveResolution::OffSurface
        ));
    }

    #[test]
    fn v_linear_surface_line_has_axis_symmetric_inverse() {
        let surface = cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 2,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 3,
            v_count: 2,
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.5, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.5, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        };
        let geometry = match super::ruled_surface_line_pcurve(
            &surface,
            cadmpeg_ir::geometry::SurfaceParameterAxis::U,
            cadmpeg_ir::math::Point3::new(0.5, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        ) {
            super::InverseResolution::Unique(geometry) => geometry,
            super::InverseResolution::NoMatch => panic!("transposed ruling did not match"),
            super::InverseResolution::Ambiguous => panic!("transposed ruling was ambiguous"),
        };
        let cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction } = geometry else {
            panic!("expected affine line pcurve");
        };
        assert!((origin.u - 0.5).abs() < 1e-8);
        assert!(origin.v.abs() < 1e-12);
        assert!(direction.u.abs() < 1e-12);
        assert!((direction.v - 1.0).abs() < 1e-12);
    }

    #[test]
    fn repeated_ruled_surface_line_candidates_are_ambiguous() {
        let surface = cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 1,
            v_degree: 2,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            u_count: 2,
            v_count: 3,
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        };
        assert!(matches!(
            super::ruled_surface_line_pcurve(
                &surface,
                cadmpeg_ir::geometry::SurfaceParameterAxis::V,
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            ),
            super::InverseResolution::Ambiguous
        ));
    }

    #[test]
    fn repeated_nurbs_endpoint_candidates_are_ambiguous() {
        let curve = cadmpeg_ir::geometry::NurbsCurve {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            ],
            weights: None,
            periodic: false,
        };
        assert!(matches!(
            super::nurbs_parameter_at_point(&curve, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),),
            super::InverseResolution::Ambiguous
        ));
    }

    #[test]
    fn ambiguous_cylindrical_endpoint_withholds_the_derived_pcurve() {
        use cadmpeg_ir::annotations::AnnotationBuilder;
        use cadmpeg_ir::geometry::{Curve, NurbsCurve, Surface};
        use cadmpeg_ir::ids::{CurveId, EdgeId, FaceId, LoopId, PointId, SurfaceId, VertexId};
        use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, Point, Sense, Vertex};

        let surface_id = SurfaceId("surface".into());
        let curve_id = CurveId("curve".into());
        let loop_id = LoopId("loop".into());
        let edge_id = EdgeId("edge".into());
        let start_vertex = VertexId("start-vertex".into());
        let end_vertex = VertexId("end-vertex".into());
        let start_point = PointId("start-point".into());
        let end_point = PointId("end-point".into());
        let coedge_id = cadmpeg_ir::ids::CoedgeId("coedge".into());
        let mut brep = super::Brep {
            surfaces: vec![Surface {
                id: surface_id.clone(),
                geometry: cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
                    origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                    radius: 1000.0,
                },
                source_object: None,
            }],
            curves: vec![Curve {
                id: curve_id.clone(),
                geometry: cadmpeg_ir::geometry::CurveGeometry::Nurbs(NurbsCurve {
                    degree: 2,
                    knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    control_points: vec![
                        cadmpeg_ir::math::Point3::new(1000.0, 0.0, 0.0),
                        cadmpeg_ir::math::Point3::new(1000.0, 0.0, 1000.0),
                        cadmpeg_ir::math::Point3::new(1000.0, 0.0, 0.0),
                    ],
                    weights: None,
                    periodic: false,
                }),
                source_object: None,
            }],
            faces: vec![Face {
                id: FaceId("face".into()),
                shell: cadmpeg_ir::ids::ShellId("shell".into()),
                surface: surface_id,
                sense: Sense::Forward,
                loops: vec![loop_id.clone()],
                name: None,
                color: None,
                tolerance: None,
            }],
            loops: vec![Loop {
                id: loop_id.clone(),
                face: FaceId("face".into()),
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::default(),
                coedges: vec![coedge_id.clone()],
                vertex_uses: Vec::new(),
            }],
            coedges: vec![Coedge {
                id: coedge_id,
                owner_loop: loop_id,
                edge: edge_id.clone(),
                next: cadmpeg_ir::ids::CoedgeId("coedge".into()),
                previous: cadmpeg_ir::ids::CoedgeId("coedge".into()),
                radial_next: cadmpeg_ir::ids::CoedgeId("coedge".into()),
                sense: Sense::Forward,
                pcurves: Vec::new(),
                use_curve: None,
                use_curve_parameter_range: None,
            }],
            edges: vec![Edge {
                id: edge_id,
                curve: Some(curve_id),
                start: start_vertex.clone(),
                end: end_vertex.clone(),
                param_range: None,
                tolerance: None,
            }],
            vertices: vec![
                Vertex {
                    id: start_vertex,
                    point: start_point.clone(),
                    tolerance: None,
                },
                Vertex {
                    id: end_vertex,
                    point: end_point.clone(),
                    tolerance: None,
                },
            ],
            points: vec![
                Point {
                    id: start_point,
                    position: cadmpeg_ir::math::Point3::new(1000.0, 0.0, 0.0),
                    source_object: None,
                },
                Point {
                    id: end_point,
                    position: cadmpeg_ir::math::Point3::new(1000.0, 0.0, 0.0),
                    source_object: None,
                },
            ],
            ..Default::default()
        };
        let mut annotations = AnnotationBuilder::new();
        let source_stream = annotations.stream("test");
        super::derive_cylindrical_pcurves(&mut brep, &mut annotations, source_stream);

        assert!(brep.pcurves.is_empty());
        assert_eq!(brep.stats.ambiguous_pcurve_parameters, 1);
    }
}
