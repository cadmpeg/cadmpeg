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
use super::{scan_carriers, Carrier, CarrierGeometry, LEN_TO_MM};
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

/// Transfer limitations found while building a [`Brep`].
#[derive(Default)]
pub struct Stats {
    /// Faces on a support surface this codec does not type; emitted with an
    /// unknown-geometry carrier.
    pub unknown_surface_faces: usize,
    /// Edges whose support curve is an untyped carrier (emitted with no curve).
    pub unknown_curve_edges: usize,
    /// Cone/torus carriers decoded from a single observed field layout.
    pub single_sample_carriers: usize,
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
    let mut combined = Vec::new();
    for (payload, header) in bodies {
        combined.extend_from_slice(&payload[header.body_offset.min(payload.len())..]);
    }
    decode_body(&combined, stream)
}

fn decode_body(body: &[u8], stream: &str) -> Brep {
    let carriers = scan_carriers(body);
    let t = topology::scan(body);
    let entity_facts = entity::scan(body);
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
    let mut faces: Vec<WalkedFace> = t.bridges.values().map(|b| walk_face(b, &t)).collect();
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
        if !kept_vertices.contains(&start_v) || !kept_vertices.contains(&end_v) {
            continue;
        }
        let eu = t.edge_uses.get(&e);
        let mut curve = None;
        if curve_attr != 0 {
            match carriers.get(&curve_attr).map(|c| &c.geometry) {
                Some(CarrierGeometry::Curve(_)) => {
                    if emitted_curves.insert(curve_attr) {
                        emit_curve(&mut out, &carriers[&curve_attr]);
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
            start: VertexId(id_vertex(start_v)),
            end: VertexId(id_vertex(end_v)),
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
                    pcurve: None,
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
                coedges,
            });
        }
    }
    let loop_set = kept_loops;

    // Surfaces + faces.
    let mut bridge_group = HashMap::new();
    for (group, body_record) in body_records.iter().enumerate() {
        for face in &faces {
            let owner = t.bridges.get(&face.bridge_attr).and_then(|r| r.owner);
            if owner.is_some_and(|owner| body_record.refs.contains(&owner)) {
                bridge_group.insert(face.bridge_attr, group);
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
        match carriers.get(&f.surface_attr).map(|c| (c, &c.geometry)) {
            Some((c, CarrierGeometry::Surface(geo))) => {
                if c.single_sample {
                    out.stats.single_sample_carriers += 1;
                }
                annotations
                    .note(id_surf(f.bridge_attr), source_stream, c.offset as u64)
                    .tag("compact_surface");
                let mut geometry = geo.clone();
                if let Some((_, u_reference, v_reference)) = c.frame {
                    fold_surface_frame(&mut geometry, u_reference, v_reference);
                    annotate_surface_frame(&mut annotations, id_surf(f.bridge_attr), &geometry);
                }
                out.surfaces.push(Surface {
                    id: SurfaceId(id_surf(f.bridge_attr)),
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
                    geometry: SurfaceGeometry::Unknown { record: None },
                });
            }
        }
        annotations
            .note(id_face(f.bridge_attr), source_stream, surf_off as u64)
            .tag("00_0e");
        out.faces.push(Face {
            id: FaceId(id_face(f.bridge_attr)),
            shell: ShellId(
                bridge_group
                    .get(&f.bridge_attr)
                    .and_then(|group| body_records.get(*group))
                    .and_then(|record| record.shell)
                    .map_or_else(
                        || {
                            format!(
                                "sldprt:brep:shell#{}",
                                bridge_group.get(&f.bridge_attr).copied().unwrap_or(0)
                            )
                        },
                        |(attr, _)| format!("sldprt:brep:shell#{attr}"),
                    ),
            ),
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
    for appearance in &mut out.face_colors {
        appearance.target = faces
            .iter()
            .find(|face| {
                t.bridges
                    .get(&face.bridge_attr)
                    .and_then(|bridge| bridge.owner)
                    == Some(appearance.face_attr)
            })
            .map(|face| id_face(face.bridge_attr));
    }
    solve_face_orientation(&mut out);
    synthesize_cylinder_seams(&mut out, &mut annotations, source_stream);
    synthesize_sphere_seams(&mut out, &mut annotations, source_stream);
    derive_planar_pcurves(&mut out, &mut annotations, source_stream);
    derive_cylindrical_pcurves(&mut out, &mut annotations, source_stream);
    derive_spherical_pcurves(&mut out, &mut annotations, source_stream);
    derive_nurbs_boundary_pcurves(&mut out, &mut annotations, source_stream);

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
        let region_id = body_record.and_then(|record| record.region).map_or_else(
            || format!("sldprt:brep:region#{group}"),
            |(attr, _)| format!("sldprt:brep:region#{attr}"),
        );
        let shell_id = body_record.and_then(|record| record.shell).map_or_else(
            || format!("sldprt:brep:shell#{group}"),
            |(attr, _)| format!("sldprt:brep:shell#{attr}"),
        );
        let faces = out
            .faces
            .iter()
            .filter(|face| face.shell.0 == shell_id)
            .map(|face| face.id.clone())
            .collect();
        let mut annotate_group = |id: &str, source: Option<(usize, &str)>| {
            let (offset, tag, exactness) = source.map_or(
                (0, "synthetic_grouping", Exactness::Derived),
                |(offset, tag)| (offset, tag, Exactness::ByteExact),
            );
            annotations.note(id, source_stream, offset as u64).tag(tag);
            annotations.exactness(id, exactness);
        };
        annotate_group(
            &shell_id,
            body_record
                .and_then(|record| record.shell)
                .map(|(_, offset)| (offset, "00_51_shell")),
        );
        annotate_group(
            &region_id,
            body_record
                .and_then(|record| record.region)
                .map(|(_, offset)| (offset, "00_51_region")),
        );
        annotate_group(
            &body_id,
            body_record.map(|record| (record.offset, "00_51_body")),
        );
        out.shells.push(Shell {
            id: ShellId(shell_id.clone()),
            region: RegionId(region_id.clone()),
            faces,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        out.regions.push(Region {
            id: RegionId(region_id.clone()),
            body: BodyId(body_id.clone()),
            shells: vec![ShellId(shell_id)],
        });
        out.bodies.push(Body {
            id: BodyId(body_id),
            kind: body_record.map_or(BodyKind::Solid, |record| record.kind),
            regions: vec![RegionId(region_id)],
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
        if let Some(carrier) = carriers.get(&attr) {
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

fn fold_surface_frame(
    geometry: &mut SurfaceGeometry,
    u_reference: cadmpeg_ir::math::Vector3,
    v_reference: cadmpeg_ir::math::Vector3,
) {
    match geometry {
        SurfaceGeometry::Plane { u_axis, .. } => *u_axis = u_reference,
        SurfaceGeometry::Cylinder { ref_direction, .. }
        | SurfaceGeometry::Cone { ref_direction, .. }
        | SurfaceGeometry::Torus { ref_direction, .. } => *ref_direction = u_reference,
        SurfaceGeometry::Sphere {
            axis,
            ref_direction,
            ..
        } => {
            *axis = v_reference;
            *ref_direction = u_reference;
        }
        SurfaceGeometry::Nurbs(_) | SurfaceGeometry::Unknown { .. } => {}
    }
}

fn annotate_surface_frame(
    annotations: &mut AnnotationBuilder,
    id: String,
    geometry: &SurfaceGeometry,
) {
    match geometry {
        SurfaceGeometry::Plane { .. } => {
            annotations.derived(id, "geometry.u_axis");
        }
        SurfaceGeometry::Cylinder { .. }
        | SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Torus { .. } => {
            annotations.derived(id, "geometry.ref_direction");
        }
        SurfaceGeometry::Sphere { .. } => {
            annotations
                .derived(&id, "geometry.axis")
                .derived(id, "geometry.ref_direction");
        }
        SurfaceGeometry::Nurbs(_) | SurfaceGeometry::Unknown { .. } => {}
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
    let mut derived = Vec::new();
    for coedge in &out.coedges {
        let Some(face_id) = loop_faces.get(&coedge.owner_loop) else {
            continue;
        };
        let Some(face) = out.faces.iter().find(|face| face.id == *face_id) else {
            continue;
        };
        let Some(surface) = out
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
        else {
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
        let Some(edge) = out.edges.iter().find(|edge| edge.id == coedge.edge) else {
            continue;
        };
        if !edge.curve.as_ref().is_some_and(|curve_id| {
            out.curves.iter().any(|curve| {
                curve.id == *curve_id && matches!(curve.geometry, CurveGeometry::Line { .. })
            })
        }) {
            continue;
        }
        let position = |vertex_id: &VertexId| {
            out.vertices
                .iter()
                .find(|vertex| vertex.id == *vertex_id)
                .and_then(|vertex| out.points.iter().find(|point| point.id == vertex.point))
                .map(|point| point.position)
        };
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
            parameter_range: None,
            fit_tolerance: None,
        };
        derived.push((coedge.id.clone(), id, pcurve));
    }
    for (coedge_id, id, pcurve) in derived {
        if let Some(coedge) = out.coedges.iter_mut().find(|coedge| coedge.id == coedge_id) {
            coedge.pcurve = Some(id.clone());
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
    let position = |out: &Brep, vertex_id: &VertexId| {
        out.vertices
            .iter()
            .find(|vertex| vertex.id == *vertex_id)
            .and_then(|vertex| out.points.iter().find(|point| point.id == vertex.point))
            .map(|point| point.position)
    };
    let mut derived = Vec::new();
    for coedge in &out.coedges {
        if coedge.pcurve.is_some() {
            continue;
        }
        let Some(face_id) = loop_faces.get(&coedge.owner_loop) else {
            continue;
        };
        let Some(face) = out.faces.iter().find(|face| face.id == *face_id) else {
            continue;
        };
        let Some(surface) = out
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
        else {
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
        let Some(edge) = out.edges.iter().find(|edge| edge.id == coedge.edge) else {
            continue;
        };
        let Some(curve) = edge
            .curve
            .as_ref()
            .and_then(|id| out.curves.iter().find(|curve| curve.id == *id))
        else {
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
                let Some(start) = position(out, &edge.start) else {
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
                parameter_range: None,
                fit_tolerance: None,
            },
        ));
    }
    for (coedge_id, id, pcurve) in derived {
        if let Some(coedge) = out.coedges.iter_mut().find(|coedge| coedge.id == coedge_id) {
            coedge.pcurve = Some(id.clone());
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
    let mut derived = Vec::new();
    for coedge in &out.coedges {
        if coedge.pcurve.is_some() {
            continue;
        }
        let Some(face_id) = loop_faces.get(&coedge.owner_loop) else {
            continue;
        };
        let Some(face) = out.faces.iter().find(|face| face.id == *face_id) else {
            continue;
        };
        let Some(surface) = out
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
        else {
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
        let Some(edge) = out.edges.iter().find(|edge| edge.id == coedge.edge) else {
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
            .and_then(|id| out.curves.iter().find(|curve| curve.id == *id))
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
                parameter_range: None,
                fit_tolerance: None,
            },
        ));
    }
    for (coedge_id, id, pcurve) in derived {
        if let Some(coedge) = out.coedges.iter_mut().find(|coedge| coedge.id == coedge_id) {
            coedge.pcurve = Some(id.clone());
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
    let same_points = |a: &[cadmpeg_ir::math::Point3], b: &[cadmpeg_ir::math::Point3]| {
        a.len() == b.len()
            && a.iter().zip(b).all(|(a, b)| {
                (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9 && (a.z - b.z).abs() < 1e-9
            })
    };
    let mut derived = Vec::new();
    for coedge in &out.coedges {
        if coedge.pcurve.is_some() {
            continue;
        }
        let Some(face_id) = loop_faces.get(&coedge.owner_loop) else {
            continue;
        };
        let Some(face) = out.faces.iter().find(|face| face.id == *face_id) else {
            continue;
        };
        let Some(SurfaceGeometry::Nurbs(surface)) = out
            .surfaces
            .iter()
            .find(|item| item.id == face.surface)
            .map(|item| &item.geometry)
        else {
            continue;
        };
        let Some(edge) = out.edges.iter().find(|edge| edge.id == coedge.edge) else {
            continue;
        };
        let Some(CurveGeometry::Nurbs(curve)) = edge
            .curve
            .as_ref()
            .and_then(|id| out.curves.iter().find(|item| item.id == *id))
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
                parameter_range: None,
                fit_tolerance: None,
            },
        ));
    }
    for (coedge_id, id, pcurve) in derived {
        if let Some(coedge) = out.coedges.iter_mut().find(|coedge| coedge.id == coedge_id) {
            coedge.pcurve = Some(id.clone());
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
    let mut candidates = Vec::new();
    for face in &out.faces {
        let Some(surface) = out
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
        else {
            continue;
        };
        if !matches!(surface.geometry, SurfaceGeometry::Cylinder { .. }) || face.loops.len() != 2 {
            continue;
        }
        let Some(a) = out.loops.iter().find(|lp| lp.id == face.loops[0]) else {
            continue;
        };
        let Some(b) = out.loops.iter().find(|lp| lp.id == face.loops[1]) else {
            continue;
        };
        if a.coedges.len() != 1 || b.coedges.len() != 1 {
            continue;
        }
        let Some(ca) = out.coedges.iter().find(|ce| ce.id == a.coedges[0]) else {
            continue;
        };
        let Some(cb) = out.coedges.iter().find(|ce| ce.id == b.coedges[0]) else {
            continue;
        };
        let Some(ea) = out.edges.iter().find(|edge| edge.id == ca.edge) else {
            continue;
        };
        let Some(eb) = out.edges.iter().find(|edge| edge.id == cb.edge) else {
            continue;
        };
        let circular = |edge: &Edge| {
            edge.start == edge.end
                && edge.curve.as_ref().is_some_and(|id| {
                    out.curves.iter().any(|curve| {
                        curve.id == *id && matches!(curve.geometry, CurveGeometry::Circle { .. })
                    })
                })
        };
        if circular(ea) && circular(eb) {
            candidates.push((
                face.id.clone(),
                a.id.clone(),
                b.id.clone(),
                ca.id.clone(),
                cb.id.clone(),
                ea.start.clone(),
                eb.start.clone(),
            ));
        }
    }

    let mut removed = HashSet::new();
    for (face_id, loop_a, loop_b, circle_a, circle_b, vertex_a, vertex_b) in candidates {
        let point_for = |vertex: &Vertex| {
            out.points
                .iter()
                .find(|point| point.id == vertex.point)
                .map(|point| point.position)
        };
        let Some(pa) = out
            .vertices
            .iter()
            .find(|vertex| vertex.id == vertex_a)
            .and_then(point_for)
        else {
            continue;
        };
        let Some(pb) = out
            .vertices
            .iter()
            .find(|vertex| vertex.id == vertex_b)
            .and_then(point_for)
        else {
            continue;
        };
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
        out.coedges.push(Coedge {
            id: seam_a.clone(),
            owner_loop: loop_a.clone(),
            edge: edge_id.clone(),
            next: circle_b.clone(),
            previous: circle_a.clone(),
            radial_next: seam_b.clone(),
            sense: Sense::Forward,
            pcurve: None,
        });
        out.coedges.push(Coedge {
            id: seam_b.clone(),
            owner_loop: loop_a.clone(),
            edge: edge_id,
            next: circle_a.clone(),
            previous: circle_b.clone(),
            radial_next: seam_a.clone(),
            sense: Sense::Reversed,
            pcurve: None,
        });
        let ring = [circle_a.clone(), seam_a, circle_b.clone(), seam_b];
        for (index, id) in ring.iter().enumerate() {
            if let Some(coedge) = out.coedges.iter_mut().find(|coedge| coedge.id == *id) {
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
    let mut candidates = Vec::new();
    for face in &out.faces {
        let Some(surface) = out
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
        else {
            continue;
        };
        let SurfaceGeometry::Sphere { center, radius, .. } = surface.geometry else {
            continue;
        };
        if face.loops.len() != 1 {
            continue;
        }
        let Some(lp) = out.loops.iter().find(|lp| lp.id == face.loops[0]) else {
            continue;
        };
        if lp.coedges.len() != 3 {
            continue;
        }
        let all_circles = lp.coedges.iter().all(|id| {
            out.coedges
                .iter()
                .find(|coedge| coedge.id == *id)
                .and_then(|coedge| out.edges.iter().find(|edge| edge.id == coedge.edge))
                .and_then(|edge| edge.curve.as_ref())
                .is_some_and(|curve_id| {
                    out.curves.iter().any(|curve| {
                        curve.id == *curve_id
                            && matches!(curve.geometry, CurveGeometry::Circle { .. })
                    })
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
        out.coedges.push(Coedge {
            id: coedge_id.clone(),
            owner_loop: loop_id.clone(),
            edge: edge_id,
            next: ring[0].clone(),
            previous: ring[2].clone(),
            radial_next: coedge_id.clone(),
            sense: Sense::Forward,
            pcurve: None,
        });
        for (index, id) in ring.iter().enumerate() {
            if let Some(coedge) = out.coedges.iter_mut().find(|coedge| coedge.id == *id) {
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
            geometry: geo.clone(),
        });
    }
}
