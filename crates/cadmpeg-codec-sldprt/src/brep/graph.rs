// SPDX-License-Identifier: Apache-2.0
//! Build IR arenas from parsed Parasolid topology records and carriers.
//!
//! The graph builder walks each face bridge through its loop and coedge rings,
//! resolves edge and vertex uses, closes emitted loops, and groups faces under
//! explicit body records. It derives one body hierarchy when those records are
//! absent. It also derives supported pcurves and periodic seams.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::annotations::{AnnotationBuilder, Annotations};
use cadmpeg_ir::eval::nurbs_curve_point;
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, BlendSupport, Curve, CurveGeometry, Pcurve, PcurveGeometry,
    ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
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

use super::blend::BlendSupportRef;
use super::entity;
use super::sweep::{self, SweepKind};
use super::topology::{self, Record};
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
            if let ProceduralSurfaceDefinition::Blend {
                supports, spine, ..
            } = &mut procedural.definition
            {
                for support in supports.iter_mut().flatten() {
                    support.surface.0 = qualify(&support.surface.0);
                }
                if let Some(spine) = spine {
                    spine.0 = qualify(&spine.0);
                }
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

/// Transfer limitations found while building a [`Brep`].
#[derive(Default)]
pub struct Stats {
    /// Framed top-level model entity records across the selected stream site.
    pub source_entity_records: usize,
    /// Faces on a support surface this codec does not type; emitted with an
    /// unknown-geometry carrier.
    pub unknown_surface_faces: usize,
    /// Edges whose support curve is an untyped carrier (emitted with no curve).
    pub unknown_curve_edges: usize,
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
    let CarrierGeometry::Curve(CurveGeometry::Nurbs(curve)) = &profile.geometry else {
        return None;
    };
    let profile_derived = carriers.curve_is_derived(construction.profile_attr);
    match &construction.kind {
        SweepKind::Spun { base, axis } => Some((
            SurfaceGeometry::Nurbs(sweep::spun_nurbs(curve, *base, *axis)),
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
                    curve,
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

fn walk_face(bridge: &Record, t: &topology::Tables) -> WalkedFace {
    let surface_attr = *bridge.refs.get(4).unwrap_or(&0);
    let mut loops = Vec::new();
    let mut loop_ref = *bridge.refs.get(2).unwrap_or(&0);
    let mut loop_guard = HashSet::new();
    while loop_ref != 0 && loop_guard.insert(loop_ref) {
        let Some(lp) = t.loops.get(&loop_ref) else {
            break;
        };
        let first = *lp.refs.get(1).unwrap_or(&0);
        let mut ring = Vec::new();
        let mut ce_ref = first;
        let mut ce_guard = HashSet::new();
        while ce_ref != 0 && ce_guard.insert(ce_ref) {
            let Some(ce) = t.coedges.get(&ce_ref) else {
                break;
            };
            ring.push(ce_ref);
            ce_ref = *ce.refs.get(3).unwrap_or(&0);
            if ce_ref == first {
                break;
            }
        }
        if !ring.is_empty() {
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
/// Input order determines override order for topology records with the same
/// attribute id. `stream` names the combined provenance source.
pub fn decode_bodies(bodies: &[(&[u8], &StreamHeader)], stream: &str) -> Brep {
    let mut carriers = CarrierIndex::default();
    let mut tables = topology::Tables::default();
    let mut facts = entity::Facts::default();
    let mut initialized = false;
    for (payload, header) in bodies {
        let body = &payload[header.body_offset.min(payload.len())..];
        let is_deltas = header.description.to_ascii_lowercase().contains("deltas");
        let scanned_tables = if is_deltas {
            topology::scan_deltas(body)
        } else {
            topology::scan(body)
        };
        let mut scanned_facts = entity::scan(body);
        if !initialized || !is_deltas {
            carriers.merge_missing(scan_carriers(body));
            if initialized {
                tables.merge_deltas(scanned_tables);
                if facts.bodies.is_empty() {
                    facts.bodies = scanned_facts.bodies;
                }
                if facts.cluster_bodies.is_empty() {
                    facts.cluster_bodies = scanned_facts.cluster_bodies;
                }
                facts.face_colors.append(&mut scanned_facts.face_colors);
                facts.entity_count += scanned_facts.entity_count;
            } else {
                tables = scanned_tables;
                facts = scanned_facts;
                initialized = true;
            }
        } else {
            carriers.merge_missing(scan_carriers(body));
            tables.merge_deltas(scanned_tables);
            facts.face_colors.append(&mut scanned_facts.face_colors);
            facts.entity_count += scanned_facts.entity_count;
        }
    }
    decode_graph(&carriers, &tables, facts, stream)
}

fn decode_body(body: &[u8], stream: &str) -> Brep {
    let carriers = scan_carriers(body);
    let t = topology::scan(body);
    let entity_facts = entity::scan(body);
    decode_graph(&carriers, &t, entity_facts, stream)
}

fn decode_graph(
    carriers: &CarrierIndex,
    t: &topology::Tables,
    entity_facts: entity::Facts,
    stream: &str,
) -> Brep {
    let mut body_records = entity_facts.bodies;
    let cluster_bodies = entity_facts.cluster_bodies;

    let mut out = Brep {
        face_colors: entity_facts.face_colors,
        stats: Stats {
            source_entity_records: entity_facts.entity_count,
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
    let mut faces: Vec<WalkedFace> = t.bridges.values().map(|b| walk_face(b, t)).collect();
    faces.sort_by_key(|f| f.bridge_attr);
    let mut face_owners = HashSet::new();
    faces.retain(|face| {
        t.bridges
            .get(&face.bridge_attr)
            .and_then(|bridge| bridge.owner)
            .is_none_or(|owner| face_owners.insert(owner))
    });

    // Kept-entity sets, so only chain-reachable records are emitted.
    let mut kept_vertices: HashSet<u16> = HashSet::new();
    let mut kept_points: HashSet<u16> = HashSet::new();
    // Edge attr -> (start vuse, end vuse, curve carrier attr) from the ring walk.
    let mut edge_ends: HashMap<u16, (u16, u16, u16)> = HashMap::new();

    for f in &faces {
        for (_loop_attr, ring) in &f.loops {
            let k = ring.len();
            for (i, &ce_attr) in ring.iter().enumerate() {
                let Some(ce) = t.coedges.get(&ce_attr) else {
                    continue;
                };
                let next_attr = ring[(i + 1) % k];
                let start_vuse = *ce.refs.get(4).unwrap_or(&0);
                let end_vuse = t
                    .coedges
                    .get(&next_attr)
                    .and_then(|n| n.refs.get(4).copied())
                    .unwrap_or(0);
                let edge_attr = *ce.refs.get(6).unwrap_or(&0);
                if edge_attr != 0 {
                    let curve_attr = t
                        .edge_uses
                        .get(&edge_attr)
                        .and_then(|e| e.refs.get(3).copied())
                        .unwrap_or(0);
                    edge_ends
                        .entry(edge_attr)
                        .or_insert((start_vuse, end_vuse, curve_attr));
                }
                for vuse in [start_vuse, end_vuse] {
                    if vuse == 0 {
                        continue;
                    }
                    if let Some(vu) = t.vertex_uses.get(&vuse) {
                        let point_attr = *vu.refs.get(4).unwrap_or(&0);
                        if t.points.contains_key(&point_attr) {
                            kept_vertices.insert(vuse);
                            kept_points.insert(point_attr);
                        }
                    }
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
        let (start_id, end_id) = if let Some(position) = closed_circle_point {
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
            param_range: None,
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
                out.coedges.push(Coedge {
                    id: CoedgeId(id_coedge(ce_attr)),
                    owner_loop: LoopId(id_loop(*loop_attr)),
                    edge: EdgeId(id_edge(edge_attr)),
                    next: CoedgeId(id_coedge(next)),
                    previous: CoedgeId(id_coedge(prev)),
                    radial_next: partner.unwrap_or_else(|| CoedgeId(id_coedge(ce_attr))),
                    sense: sense_of(ce.marker.unwrap_or(0x2b)),
                    use_curve: None,
                    use_curve_parameter_range: None,
                    pcurves: Vec::new(),
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
    let bind_bridges = |body_records: &[entity::BodyRecord],
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
    let (mut bridge_group, mut bridge_shell) = bind_bridges(&body_records, &faces);
    // Primary body records that own no face are superseded by cluster-key
    // chain bodies; a sole chain owns every canonical face in the site.
    if (body_records.is_empty() || bridge_group.is_empty()) && !cluster_bodies.is_empty() {
        let (cluster_group, cluster_shell) = bind_bridges(&cluster_bodies, &faces);
        if !cluster_group.is_empty() || cluster_bodies.len() == 1 {
            body_records = cluster_bodies;
            bridge_group = cluster_group;
            bridge_shell = cluster_shell;
            if let [sole] = body_records.as_slice() {
                let shell_attr = sole
                    .regions
                    .first()
                    .and_then(|region| region.shells.first())
                    .map(|shell| shell.attr);
                for face in &faces {
                    bridge_group.entry(face.bridge_attr).or_insert(0);
                    if let Some(shell_attr) = shell_attr {
                        bridge_shell.entry(face.bridge_attr).or_insert(shell_attr);
                    }
                }
            }
        }
    }
    if !body_records.is_empty() {
        faces.retain(|face| bridge_group.contains_key(&face.bridge_attr));
    }
    let mut surface_ids_by_carrier = HashMap::<u16, u16>::new();
    let mut face_edges_by_surface_carrier = HashMap::<u16, Vec<(u16, HashSet<u16>)>>::new();
    for face in &faces {
        surface_ids_by_carrier
            .entry(face.surface_attr)
            .and_modify(|bridge| *bridge = (*bridge).min(face.bridge_attr))
            .or_insert(face.bridge_attr);
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
            .push((face.bridge_attr, edges));
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
                let resolved_blend = carriers.blend(f.surface_attr).and_then(|blend| {
                    let face_edges: HashSet<u16> = f
                        .loops
                        .iter()
                        .flat_map(|(_, ring)| ring)
                        .filter_map(|coedge| t.coedges.get(coedge))
                        .filter_map(|coedge| coedge.refs.get(6).copied())
                        .filter(|edge| *edge != 0)
                        .collect();
                    let [Some(first), Some(second)] = blend.supports.map(|support| {
                        let bridge = match support {
                            BlendSupportRef::Surface(attr) => {
                                surface_ids_by_carrier.get(&attr).copied()
                            }
                            BlendSupportRef::Pair(attr) => {
                                let pair = carriers.blend_support_pair(attr)?;
                                carriers.curve(pair.intersection)?;
                                let mut adjacent = pair.supports.iter().filter_map(|candidate| {
                                    face_edges_by_surface_carrier
                                        .get(candidate)?
                                        .iter()
                                        .filter(|(_, edges)| !face_edges.is_disjoint(edges))
                                        .map(|(bridge, _)| *bridge)
                                        .min()
                                });
                                let bridge = adjacent.next()?;
                                if adjacent.next().is_some() {
                                    return None;
                                }
                                Some(bridge)
                            }
                        }?;
                        Some(SurfaceId(id_surf(bridge)))
                    }) else {
                        return None;
                    };
                    Some((blend, first, second))
                });
                if let Some((blend, first, second)) = resolved_blend {
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
        return Brep::default();
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
    let points: HashMap<_, _> = out.points.iter().map(|point| (&point.id, point)).collect();
    let vertex_points: HashMap<_, _> = out
        .vertices
        .iter()
        .filter_map(|vertex| points.get(&vertex.point).map(|point| (&vertex.id, *point)))
        .collect();
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
        let position =
            |vertex_id: &VertexId| vertex_points.get(vertex_id).map(|point| point.position);
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
            CurveGeometry::Line { .. } => {
                let (Some(start), Some(end)) = (position(&edge.start), position(&edge.end)) else {
                    continue;
                };
                let start = uv(start);
                let end = uv(end);
                let (du, dv) = (end.u - start.u, end.v - start.v);
                let norm = (du * du + dv * dv).sqrt();
                if norm == 0.0 {
                    continue;
                }
                PcurveGeometry::Line {
                    origin: start,
                    direction: cadmpeg_ir::math::Point2::new(du / norm, dv / norm),
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
                let (Some(start), Some(end)) = (position(&edge.start), position(&edge.end)) else {
                    continue;
                };
                let (Some(start_parameter), Some(end_parameter)) = (
                    nurbs_parameter_at_point(nurbs, start),
                    nurbs_parameter_at_point(nurbs, end),
                ) else {
                    continue;
                };
                parameter_range = Some([
                    start_parameter.min(end_parameter),
                    start_parameter.max(end_parameter),
                ]);
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

fn nurbs_parameter_at_point(
    nurbs: &cadmpeg_ir::geometry::NurbsCurve,
    target: cadmpeg_ir::math::Point3,
) -> Option<f64> {
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
    let mut best = None::<(f64, f64)>;
    for span in nurbs.knots.windows(2).filter(|span| span[0] < span[1]) {
        let (mut left, mut right) = (span[0], span[1]);
        let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
        let mut a = right - ratio * (right - left);
        let mut b = left + ratio * (right - left);
        let mut da = squared_distance(a)?;
        let mut db = squared_distance(b)?;
        for _ in 0..80 {
            if da <= db {
                right = b;
                b = a;
                db = da;
                a = right - ratio * (right - left);
                da = squared_distance(a)?;
            } else {
                left = a;
                a = b;
                da = db;
                b = left + ratio * (right - left);
                db = squared_distance(b)?;
            }
        }
        for parameter in [span[0], (left + right) * 0.5, span[1]] {
            let distance = squared_distance(parameter)?;
            if best.is_none_or(|(_, best_distance)| distance < best_distance) {
                best = Some((parameter, distance));
            }
        }
    }
    let (parameter, squared_distance) = best?;
    (squared_distance.sqrt() <= 0.01).then_some(parameter)
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
    let same_points = |a: &[cadmpeg_ir::math::Point3], b: &[cadmpeg_ir::math::Point3]| {
        a.len() == b.len()
            && a.iter().zip(b).all(|(a, b)| {
                (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9 && (a.z - b.z).abs() < 1e-9
            })
    };
    let same_weights = |a: Option<&[f64]>, b: Option<&[f64]>| match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-12)
        }
        _ => false,
    };
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
        let (u_min, u_max) = (
            *surface.u_knots.first().unwrap_or(&0.0),
            *surface.u_knots.last().unwrap_or(&1.0),
        );
        let (v_min, v_max) = (
            *surface.v_knots.first().unwrap_or(&0.0),
            *surface.v_knots.last().unwrap_or(&1.0),
        );
        let (uc, vc) = (surface.u_count as usize, surface.v_count as usize);
        if uc == 0 || vc == 0 {
            continue;
        }
        let row = |u: usize| {
            (0..vc)
                .map(|v| surface.control_points[u * vc + v])
                .collect::<Vec<_>>()
        };
        let column = |v: usize| {
            (0..uc)
                .map(|u| surface.control_points[u * vc + v])
                .collect::<Vec<_>>()
        };
        let row_weights = |u: usize| {
            surface
                .weights
                .as_ref()
                .map(|weights| &weights[u * vc..(u + 1) * vc])
        };
        let column_weights = |v: usize| {
            surface
                .weights
                .as_ref()
                .map(|weights| (0..uc).map(|u| weights[u * vc + v]).collect::<Vec<_>>())
        };
        let geometry = match curve {
            CurveGeometry::Nurbs(curve) => {
                if curve.degree == surface.v_degree
                    && curve.knots == surface.v_knots
                    && same_points(&curve.control_points, &row(0))
                    && same_weights(curve.weights.as_deref(), row_weights(0))
                {
                    PcurveGeometry::Line {
                        origin: cadmpeg_ir::math::Point2::new(u_min, v_min),
                        direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
                    }
                } else if curve.degree == surface.v_degree
                    && curve.knots == surface.v_knots
                    && same_points(&curve.control_points, &row(uc - 1))
                    && same_weights(curve.weights.as_deref(), row_weights(uc - 1))
                {
                    PcurveGeometry::Line {
                        origin: cadmpeg_ir::math::Point2::new(u_max, v_min),
                        direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
                    }
                } else if curve.degree == surface.u_degree
                    && curve.knots == surface.u_knots
                    && same_points(&curve.control_points, &column(0))
                    && same_weights(curve.weights.as_deref(), column_weights(0).as_deref())
                {
                    PcurveGeometry::Line {
                        origin: cadmpeg_ir::math::Point2::new(u_min, v_min),
                        direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
                    }
                } else if curve.degree == surface.u_degree
                    && curve.knots == surface.u_knots
                    && same_points(&curve.control_points, &column(vc - 1))
                    && same_weights(curve.weights.as_deref(), column_weights(vc - 1).as_deref())
                {
                    PcurveGeometry::Line {
                        origin: cadmpeg_ir::math::Point2::new(u_min, v_max),
                        direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
                    }
                } else {
                    continue;
                }
            }
            CurveGeometry::Line { origin, direction }
                if surface.u_degree == 1 && surface.u_knots == [u_min, u_min, u_max, u_max] =>
            {
                let line_pcurve = |v_index: usize, v: f64| {
                    let points = column(v_index);
                    let weights_equal = surface.weights.as_ref().is_none_or(|weights| {
                        (weights[v_index] - weights[(uc - 1) * vc + v_index]).abs() < 1e-12
                    });
                    if points.len() != 2 || !weights_equal || u_min == u_max {
                        return None;
                    }
                    let delta = [
                        points[1].x - points[0].x,
                        points[1].y - points[0].y,
                        points[1].z - points[0].z,
                    ];
                    let squared = delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2];
                    if squared <= f64::EPSILON {
                        return None;
                    }
                    let relative = [
                        origin.x - points[0].x,
                        origin.y - points[0].y,
                        origin.z - points[0].z,
                    ];
                    let project = |value: [f64; 3]| {
                        (value[0] * delta[0] + value[1] * delta[1] + value[2] * delta[2]) / squared
                    };
                    let offset = project(relative);
                    let rate = project([direction.x, direction.y, direction.z]);
                    let residual = |value: [f64; 3], factor: f64| {
                        ((value[0] - factor * delta[0]).powi(2)
                            + (value[1] - factor * delta[1]).powi(2)
                            + (value[2] - factor * delta[2]).powi(2))
                        .sqrt()
                    };
                    if residual(relative, offset) > 1e-6
                        || residual([direction.x, direction.y, direction.z], rate) > 1e-9
                        || rate == 0.0
                    {
                        return None;
                    }
                    let domain = u_max - u_min;
                    Some(PcurveGeometry::Line {
                        origin: cadmpeg_ir::math::Point2::new(u_min + offset * domain, v),
                        direction: cadmpeg_ir::math::Point2::new(rate * domain, 0.0),
                    })
                };
                if let Some(geometry) = line_pcurve(0, v_min) {
                    geometry
                } else if let Some(geometry) = line_pcurve(vc - 1, v_max) {
                    geometry
                } else if let Some(geometry) =
                    ruled_surface_line_pcurve(surface, *origin, *direction, u_min, u_max)
                {
                    geometry
                } else {
                    continue;
                }
            }
            _ => continue,
        };
        let id = PcurveId(format!(
            "sldprt:brep:pcurve#nurbs-isoparametric:{}",
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
            .tag("derived_nurbs_isoparametric_pcurve");
        annotations.exactness(&id, Exactness::Derived);
        out.pcurves.push(pcurve);
    }
}

fn ruled_surface_line_pcurve(
    surface: &cadmpeg_ir::geometry::NurbsSurface,
    line_origin: cadmpeg_ir::math::Point3,
    line_direction: cadmpeg_ir::math::Vector3,
    u_min: f64,
    u_max: f64,
) -> Option<PcurveGeometry> {
    let (uc, vc) = (surface.u_count as usize, surface.v_count as usize);
    if uc != 2
        || surface.u_degree != 1
        || surface.u_knots != [u_min, u_min, u_max, u_max]
        || u_min == u_max
        || surface
            .weights
            .as_ref()
            .is_some_and(|weights| (0..vc).any(|v| (weights[v] - weights[vc + v]).abs() > 1e-12))
    {
        return None;
    }
    let row = |u: usize| &surface.control_points[u * vc..(u + 1) * vc];
    let row_weights = |u: usize| {
        surface
            .weights
            .as_ref()
            .map(|weights| &weights[u * vc..(u + 1) * vc])
    };
    let evaluate_rows = |parameter: f64| {
        Some((
            nurbs_curve_point(
                surface.v_degree,
                &surface.v_knots,
                row(0),
                row_weights(0),
                parameter,
            )?,
            nurbs_curve_point(
                surface.v_degree,
                &surface.v_knots,
                row(1),
                row_weights(1),
                parameter,
            )?,
        ))
    };
    let direction_squared = line_direction.x * line_direction.x
        + line_direction.y * line_direction.y
        + line_direction.z * line_direction.z;
    if direction_squared <= f64::EPSILON {
        return None;
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
        let (a, b) = evaluate_rows(parameter)?;
        Some(perpendicular_squared(a).max(perpendicular_squared(b)))
    };
    let mut best = None::<(f64, f64)>;
    for span in surface.v_knots.windows(2).filter(|span| span[0] < span[1]) {
        let (mut left, mut right) = (span[0], span[1]);
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
        for parameter in [span[0], (left + right) * 0.5, span[1]] {
            let error = objective(parameter)?;
            if best.is_none_or(|(_, best_error)| error < best_error) {
                best = Some((parameter, error));
            }
        }
    }
    let (v, error) = best?;
    if error.sqrt() > 0.01 {
        return None;
    }
    let (a, b) = evaluate_rows(v)?;
    let delta = [b.x - a.x, b.y - a.y, b.z - a.z];
    let delta_squared = delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2];
    if delta_squared <= f64::EPSILON {
        return None;
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
        return None;
    }
    let domain = u_max - u_min;
    Some(PcurveGeometry::Line {
        origin: cadmpeg_ir::math::Point2::new(u_min + offset * domain, v),
        direction: cadmpeg_ir::math::Point2::new(rate * domain, 0.0),
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
            let edge = &out.edges[*edge_index];
            let north = cadmpeg_ir::math::Point3::new(
                center.x + radius * axis.x,
                center.y + radius * axis.y,
                center.z + radius * axis.z,
            );
            let south = cadmpeg_ir::math::Point3::new(
                center.x - radius * axis.x,
                center.y - radius * axis.y,
                center.z - radius * axis.z,
            );
            let squared_distance =
                |left: cadmpeg_ir::math::Point3, right: cadmpeg_ir::math::Point3| {
                    (left.x - right.x).powi(2)
                        + (left.y - right.y).powi(2)
                        + (left.z - right.z).powi(2)
                };
            let point = vertex_points
                .get(&edge.start)
                .or_else(|| vertex_points.get(&edge.end))
                .map_or(north, |endpoint| {
                    if squared_distance(*endpoint, north) <= squared_distance(*endpoint, south) {
                        north
                    } else {
                        south
                    }
                });
            existing.push((*edge_index, point));
        }
    }
    for (edge_index, point) in existing {
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
        let geometry = super::ruled_surface_line_pcurve(
            &surface,
            cadmpeg_ir::math::Point3::new(0.0, 0.5, 0.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            0.0,
            1.0,
        )
        .expect("interior ruling");
        let cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction } = geometry else {
            panic!("expected affine line pcurve");
        };
        assert!(origin.u.abs() < 1e-12);
        assert!((origin.v - 0.5).abs() < 1e-12);
        assert!((direction.u - 2.0 / 3.0).abs() < 1e-12);
        assert!(direction.v.abs() < 1e-12);
    }
}
