// SPDX-License-Identifier: Apache-2.0
//! Build IR arenas from parsed Parasolid topology records and carriers.
//!
//! The graph builder walks each face bridge through its loop and coedge rings,
//! resolves edge and vertex uses, closes emitted loops, and groups faces under
//! explicit body records. It derives one body hierarchy when those records are
//! absent. It also derives supported pcurves and periodic seams.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::annotations::{AnnotationBuilder, Annotations};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, Pcurve, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::Exactness;

use super::entity;
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
            if let SurfaceGeometry::Unknown {
                record: Some(record),
            } = &mut surface.geometry
            {
                record.0 = qualify(&record.0);
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

/// Transfer limitations found while building a [`Brep`].
#[derive(Default)]
pub struct Stats {
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
                facts.face_colors.append(&mut scanned_facts.face_colors);
            } else {
                tables = scanned_tables;
                facts = scanned_facts;
                initialized = true;
            }
        } else {
            carriers.merge_missing(scan_carriers(body));
            tables.merge_deltas(scanned_tables);
            facts.face_colors.append(&mut scanned_facts.face_colors);
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
    let body_records = entity_facts.bodies;

    let mut out = Brep {
        face_colors: entity_facts.face_colors,
        ..Brep::default()
    };
    let mut annotations = AnnotationBuilder::new();
    let source_stream = annotations.stream(stream);
    out.stats.synthetic_body_grouping = body_records.is_empty();
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
                    pcurves: Vec::new(),
                    use_curve: None,
                    use_curve_parameter_range: None,
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
    let mut bridge_group = HashMap::new();
    let mut bridge_shell = HashMap::new();
    for (group, body_record) in body_records.iter().enumerate() {
        for face in &faces {
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
    if !body_records.is_empty() {
        faces.retain(|face| bridge_group.contains_key(&face.bridge_attr));
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
        match carriers.surface(f.surface_attr).map(|c| (c, &c.geometry)) {
            Some((c, CarrierGeometry::Surface(geo))) => {
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
            sense: sense_of(f.marker),
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
    derive_spherical_pcurves(&mut out, &mut annotations, source_stream);
    derive_nurbs_boundary_pcurves(&mut out, &mut annotations, source_stream);
    prune_rejected_topology(&mut out);

    if out.faces.is_empty() {
        return Brep::default();
    }

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
            let shell_id = format!("sldprt:brep:shell#{group}");
            annotate_group(&shell_id, None);
            annotate_group(&region_id, None);
            out.shells.push(Shell {
                id: ShellId(shell_id.clone()),
                region: RegionId(region_id.clone()),
                faces: out
                    .faces
                    .iter()
                    .filter(|face| face.shell.0 == shell_id)
                    .map(|face| face.id.clone())
                    .collect(),
                wire_edges: Vec::new(),
                free_vertices: Vec::new(),
            });
            out.regions.push(Region {
                id: RegionId(region_id.clone()),
                body: BodyId(body_id.clone()),
                shells: vec![ShellId(shell_id)],
            });
            body_regions.push(RegionId(region_id));
        } else {
            for region in native_regions {
                let region_id = format!("sldprt:brep:region#{}", region.attr);
                annotate_group(&region_id, Some((region.offset, "00_51_region")));
                let mut region_shells = Vec::new();
                for shell in &region.shells {
                    let shell_id = format!("sldprt:brep:shell#{}", shell.attr);
                    annotate_group(&shell_id, Some((shell.offset, "00_51_shell")));
                    out.shells.push(Shell {
                        id: ShellId(shell_id.clone()),
                        region: RegionId(region_id.clone()),
                        faces: out
                            .faces
                            .iter()
                            .filter(|face| face.shell.0 == shell_id)
                            .map(|face| face.id.clone())
                            .collect(),
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

    let kept_curves = out
        .edges
        .iter()
        .filter_map(|edge| edge.curve.clone())
        .collect::<HashSet<_>>();
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
        if !edge.curve.as_ref().is_some_and(|curve_id| {
            curves
                .get(curve_id)
                .is_some_and(|curve| matches!(curve.geometry, CurveGeometry::Line { .. }))
        }) {
            continue;
        }
        let position =
            |vertex_id: &VertexId| vertex_points.get(vertex_id).map(|point| point.position);
        let (Some(start), Some(end)) = (position(&edge.start), position(&edge.end)) else {
            continue;
        };
        let uv = |point: cadmpeg_ir::math::Point3| {
            let d = [point.x - origin.x, point.y - origin.y, point.z - origin.z];
            cadmpeg_ir::math::Point2::new(
                d[0] * u_reference.x + d[1] * u_reference.y + d[2] * u_reference.z,
                d[0] * v_reference.x + d[1] * v_reference.y + d[2] * v_reference.z,
            )
        };
        let start = uv(start);
        let end = uv(end);
        let (du, dv) = (end.u - start.u, end.v - start.v);
        let norm = (du * du + dv * dv).sqrt();
        if norm == 0.0 {
            continue;
        }
        let id = PcurveId(format!(
            "sldprt:brep:pcurve#{}",
            coedge.id.0.rsplit('#').next().unwrap_or("0")
        ));
        let pcurve = Pcurve {
            id: id.clone(),
            geometry: PcurveGeometry::Line {
                origin: start,
                direction: cadmpeg_ir::math::Point2::new(du / norm, dv / norm),
            },
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
        let geometry = match &curve.geometry {
            CurveGeometry::Circle {
                center,
                axis: circle_axis,
                radius: circle_radius,
                ..
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
                PcurveGeometry::Line {
                    origin: cadmpeg_ir::math::Point2::new(0.0, axial),
                    direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
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
            .tag("derived_cylindrical_pcurve");
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

fn derive_nurbs_boundary_pcurves(
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
        let Some(CurveGeometry::Nurbs(curve)) = edge
            .curve
            .as_ref()
            .and_then(|id| curves.get(id).copied())
            .map(|item| &item.geometry)
        else {
            continue;
        };
        if surface.weights.is_some() || curve.weights.is_some() {
            continue;
        }
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
        let geometry = if curve.degree == surface.v_degree
            && curve.knots == surface.v_knots
            && same_points(&curve.control_points, &row(0))
        {
            PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(u_min, v_min),
                direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
            }
        } else if curve.degree == surface.v_degree
            && curve.knots == surface.v_knots
            && same_points(&curve.control_points, &row(uc - 1))
        {
            PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(u_max, v_min),
                direction: cadmpeg_ir::math::Point2::new(0.0, 1.0),
            }
        } else if curve.degree == surface.u_degree
            && curve.knots == surface.u_knots
            && same_points(&curve.control_points, &column(0))
        {
            PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(u_min, v_min),
                direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
            }
        } else if curve.degree == surface.u_degree
            && curve.knots == surface.u_knots
            && same_points(&curve.control_points, &column(vc - 1))
        {
            PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(u_min, v_max),
                direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
            }
        } else {
            continue;
        };
        let id = PcurveId(format!(
            "sldprt:brep:pcurve#nurbs-boundary:{}",
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
            .tag("derived_nurbs_boundary_pcurve");
        annotations.exactness(&id, Exactness::Derived);
        out.pcurves.push(pcurve);
    }
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
            pcurves: Vec::new(),
            use_curve: None,
            use_curve_parameter_range: None,
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
            pcurves: Vec::new(),
            use_curve: None,
            use_curve_parameter_range: None,
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
            candidates.push((
                face.id.clone(),
                lp.id.clone(),
                lp.coedges.clone(),
                center,
                radius,
                axis,
            ));
        }
    }
    let mut coedge_indices = out
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| (coedge.id.clone(), index))
        .collect::<HashMap<_, _>>();
    for (face, loop_id, mut ring, center, radius, axis) in candidates {
        let suffix = face.0.rsplit('#').next().unwrap_or("0");
        let point_id = PointId(format!("sldprt:brep:point#sphere-seam:{suffix}"));
        let vertex_id = VertexId(format!("sldprt:brep:vertex#sphere-seam:{suffix}"));
        let edge_id = EdgeId(format!("sldprt:brep:edge#sphere-seam:{suffix}"));
        let coedge_id = CoedgeId(format!("sldprt:brep:coedge#sphere-seam:{suffix}"));
        for id in [&point_id.0, &vertex_id.0, &edge_id.0, &coedge_id.0] {
            annotations
                .note(id, source_stream, 0)
                .tag("derived_sphere_seam");
            annotations.exactness(id, Exactness::Derived);
        }
        out.points.push(Point {
            id: point_id.clone(),
            position: cadmpeg_ir::math::Point3::new(
                center.x + radius * axis.x,
                center.y + radius * axis.y,
                center.z + radius * axis.z,
            ),
            source_object: None,
        });
        out.vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point_id,
            tolerance: None,
        });
        out.edges.push(Edge {
            id: edge_id.clone(),
            curve: None,
            start: vertex_id.clone(),
            end: vertex_id,
            param_range: None,
            tolerance: None,
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
            pcurves: Vec::new(),
            use_curve: None,
            use_curve_parameter_range: None,
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
