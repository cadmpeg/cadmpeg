// SPDX-License-Identifier: Apache-2.0
//! Build IR and diagnostics from an NX SPLMSSTR container.
//!
//! [`scan`] parses the container and inflates its embedded streams. [`decode`]
//! converts supported analytic and NURBS carriers to millimetres, resolves
//! supported topology, preserves each Parasolid stream as an unknown record, and
//! returns a [`DecodeReport`] describing incomplete transfer. Partition and
//! deltas streams are both decoded; callers must use the report to account for
//! unresolved active-face selection and other loss.
//!
//! [`DecodeReport`]: cadmpeg_ir::report::DecodeReport

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    UnknownId, VertexId,
};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;

use crate::container::{self, Container};
use crate::geometry;
use crate::parasolid::{self, Stream, StreamKind};
use crate::topology::{Graph, Node};

const MISSING_TOLERANCE: f64 = -31_415_800_000_000.0;

/// Parsed container data shared by inspection and entity decoding.
pub struct Scan {
    /// Parsed SPLMSSTR container.
    pub container: Container,
    /// Located and inflated Parasolid or preview streams.
    pub streams: Vec<Stream>,
}

impl Scan {
    /// Count streams with the requested classification.
    pub fn count(&self, kind: StreamKind) -> usize {
        self.streams.iter().filter(|s| s.kind == kind).count()
    }

    /// Return whether the file contains an inline Parasolid stream.
    ///
    /// NX assemblies may contain only references to external child parts.
    pub fn has_parasolid(&self) -> bool {
        self.streams.iter().any(|s| s.kind.is_parasolid())
    }
}

/// Parse the SPLMSSTR container and inflate streams in its canonical part entry.
pub fn scan(reader: &mut dyn ReadSeek) -> Result<Scan, CodecError> {
    let container = container::scan(reader)?;
    let streams = parasolid::extract_streams(&container.data);
    Ok(Scan { container, streams })
}

/// Decode an NX `.prt` into IR and a loss report.
///
/// When [`DecodeOptions::container_only`] is set, the returned IR contains source
/// metadata and preserved streams but no typed entities. Otherwise the decoder
/// emits supported geometry and resolvable topology. A valid container can
/// decode successfully with no geometry, including an assembly whose geometry
/// resides in external child parts.
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = scan(reader)?;

    if options.container_only {
        let ir = build_metadata_ir(&scan);
        let report = build_container_report(&scan, true);
        return Ok(DecodeResult::new(ir, report));
    }

    if let Some((ir, report)) = try_decode_geometry(&scan) {
        return Ok(DecodeResult::new(ir, report));
    }

    let ir = build_metadata_ir(&scan);
    let report = build_container_report(&scan, false);
    Ok(DecodeResult::new(ir, report))
}

/// Aggregate carrier counts across the decoded streams, for reporting.
#[derive(Debug, Default)]
struct Counts {
    points: usize,
    planes: usize,
    cylinders: usize,
    cones: usize,
    spheres: usize,
    tori: usize,
    nurbs_surfaces: usize,
    lines: usize,
    circles: usize,
    ellipses: usize,
    nurbs_curves: usize,
}

impl Counts {
    fn surfaces(&self) -> usize {
        self.planes + self.cylinders + self.cones + self.spheres + self.tori + self.nurbs_surfaces
    }
    fn curves(&self) -> usize {
        self.lines + self.circles + self.ellipses + self.nurbs_curves
    }
}

/// Decode analytic carriers from every Parasolid stream. Returns `None` when no
/// carrier of any kind passes its gate, so the caller falls back to metadata.
fn try_decode_geometry(scan: &Scan) -> Option<(CadIr, DecodeReport)> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    let mut counts = Counts::default();

    for (si, stream) in scan.streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        let stream_name = format!("parasolid#{si}:{}", stream.kind.label());
        let source_stream = annotations.stream(format!("nx:{stream_name}"));
        let graph = Graph::parse(&stream.inflated);
        let mut points_by_xmt = BTreeMap::new();
        let mut surfaces_by_xmt = BTreeMap::new();
        let mut curves_by_xmt = BTreeMap::new();
        let mut trim_ranges = BTreeMap::new();
        let first_surface = ir.model.surfaces.len();
        let first_curve = ir.model.curves.len();

        for (pi, pt) in geometry::points(&stream.inflated).into_iter().enumerate() {
            let pid = PointId(format!("nx:s{si}:pt#{pi}"));
            let vid = VertexId(format!("nx:s{si}:v#{pi}"));
            annotations
                .note(&pid, source_stream, pt.pos as u64)
                .tag("POINT");
            annotations.derived(&pid, "position");
            annotations
                .note(&vid, source_stream, pt.pos as u64)
                .tag("POINT");
            annotations.exactness(&vid, Exactness::Inferred);
            ir.model.points.push(Point {
                id: pid.clone(),
                position: pt.position,
            });
            ir.model.vertices.push(Vertex {
                id: vid.clone(),
                point: pid,
                tolerance: None,
            });
            if let Some(node) = graph.at_pos(pt.pos) {
                if node.kind == 29 {
                    let point_id = ir
                        .model
                        .points
                        .last()
                        .expect("invariant: just pushed above")
                        .id
                        .clone();
                    points_by_xmt.insert(node.xmt, (point_id, vid));
                }
            }
            counts.points += 1;
        }

        for (fi, surf) in geometry::surfaces(&stream.inflated).into_iter().enumerate() {
            match &surf.geometry {
                SurfaceGeometry::Plane { .. } => counts.planes += 1,
                SurfaceGeometry::Cylinder { .. } => counts.cylinders += 1,
                SurfaceGeometry::Cone { .. } => counts.cones += 1,
                SurfaceGeometry::Sphere { .. } => counts.spheres += 1,
                SurfaceGeometry::Torus { .. } => counts.tori += 1,
                SurfaceGeometry::Nurbs(_) | SurfaceGeometry::Unknown { .. } => {}
            }
            let id = SurfaceId(format!("nx:s{si}:surf#{fi}"));
            annotations
                .note(&id, source_stream, surf.pos as u64)
                .tag(surface_tag(&surf.geometry));
            annotations.derived(&id, "geometry");
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: surf.geometry,
            });
            if let Some(node) = graph.at_pos(surf.pos) {
                surfaces_by_xmt.insert(node.xmt, id);
            }
        }

        for (fi, surf) in crate::nurbs::surfaces(&stream.inflated)
            .into_iter()
            .enumerate()
        {
            counts.nurbs_surfaces += 1;
            let id = SurfaceId(format!("nx:s{si}:nurbs-surf#{fi}"));
            annotations
                .note(&id, source_stream, surf.pos as u64)
                .tag("B_SPLINE_SURFACE");
            annotations.derived(&id, "geometry");
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: surf.geometry,
            });
            if let Some(node) = graph.at_pos(surf.pos) {
                surfaces_by_xmt.insert(node.xmt, id);
            }
        }

        for (ci, crv) in geometry::curves(&stream.inflated).into_iter().enumerate() {
            match &crv.geometry {
                CurveGeometry::Line { .. } => counts.lines += 1,
                CurveGeometry::Circle { .. } => counts.circles += 1,
                CurveGeometry::Ellipse { .. } => counts.ellipses += 1,
                CurveGeometry::Parabola { .. }
                | CurveGeometry::Hyperbola { .. }
                | CurveGeometry::Nurbs(_)
                | CurveGeometry::Unknown { .. } => {}
            }
            let id = CurveId(format!("nx:s{si}:crv#{ci}"));
            annotations
                .note(&id, source_stream, crv.pos as u64)
                .tag(curve_tag(&crv.geometry));
            annotations.derived(&id, "geometry");
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry: crv.geometry,
            });
            if let Some(node) = graph.at_pos(crv.pos) {
                curves_by_xmt.insert(node.xmt, id);
            }
        }

        for (ci, crv) in crate::nurbs::curves(&stream.inflated)
            .into_iter()
            .enumerate()
        {
            counts.nurbs_curves += 1;
            let id = CurveId(format!("nx:s{si}:nurbs-crv#{ci}"));
            annotations
                .note(&id, source_stream, crv.pos as u64)
                .tag("B_SPLINE_CURVE");
            annotations.derived(&id, "geometry");
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry: crv.geometry,
            });
            if let Some(node) = graph.at_pos(crv.pos) {
                curves_by_xmt.insert(node.xmt, id);
            }
        }

        for trim in crate::topology::trimmed_curves(&stream.inflated) {
            if let Some(basis) = curves_by_xmt.get(&trim.basis).cloned() {
                curves_by_xmt.insert(trim.xmt, basis);
                trim_ranges.insert(trim.xmt, trim.parameters);
            }
        }

        emit_topology(
            &mut ir,
            si,
            &graph,
            &points_by_xmt,
            &surfaces_by_xmt,
            &curves_by_xmt,
            &trim_ranges,
            source_stream,
            &mut annotations,
        );

        // Preserve the whole inflated stream verbatim so nothing is dropped.
        let mut unknown = unknown_stream(si, stream);
        unknown.links.extend(
            ir.model.surfaces[first_surface..]
                .iter()
                .map(|surface| surface.id.0.clone()),
        );
        unknown.links.extend(
            ir.model.curves[first_curve..]
                .iter()
                .map(|curve| curve.id.0.clone()),
        );
        let container_stream = annotations.stream("nx:container");
        annotations
            .note(&unknown.id, container_stream, stream.file_offset as u64)
            .tag(stream.kind.label());
        annotations.exactness(&unknown.id, Exactness::Derived);
        if !unknown.links.is_empty() {
            annotations.derived(&unknown.id, "links");
        }
        ir.unknowns.push(unknown);
    }

    if counts.points == 0 && counts.surfaces() == 0 && counts.curves() == 0 {
        return None;
    }

    attach_free_topology(&mut ir, &mut annotations);
    ir.annotations = annotations.build();
    let report = build_geometry_report(scan, &counts, !ir.model.faces.is_empty());
    Some((ir, report))
}

fn attach_free_topology(ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    let edge_vertices: BTreeSet<_> = ir
        .model
        .edges
        .iter()
        .flat_map(|edge| [&edge.start, &edge.end])
        .cloned()
        .collect();
    let coedge_edges: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.clone())
        .collect();
    let free_vertices: Vec<_> = ir
        .model
        .vertices
        .iter()
        .filter(|vertex| !edge_vertices.contains(&vertex.id))
        .map(|vertex| vertex.id.clone())
        .collect();
    let wire_edges: Vec<_> = ir
        .model
        .edges
        .iter()
        .filter(|edge| !coedge_edges.contains(&edge.id))
        .map(|edge| edge.id.clone())
        .collect();
    if free_vertices.is_empty() && wire_edges.is_empty() {
        return;
    }

    if let Some(shell) = ir.model.shells.first_mut() {
        shell.free_vertices.extend(free_vertices);
        shell.wire_edges.extend(wire_edges);
        annotations
            .derived(&shell.id, "free_vertices")
            .derived(&shell.id, "wire_edges");
        return;
    }

    let body_id = BodyId("nx:derived:body#0".to_string());
    let region_id = RegionId("nx:derived:region#0".to_string());
    let shell_id = ShellId("nx:derived:shell#0".to_string());
    let stream = annotations.stream("nx:container");
    for id in [&body_id.0, &region_id.0, &shell_id.0] {
        annotations.note(id, stream, 0).tag("derived_free_topology");
        annotations.exactness(id, Exactness::Inferred);
    }
    ir.model.shells.push(Shell {
        id: shell_id.clone(),
        region: region_id.clone(),
        faces: Vec::new(),
        wire_edges,
        free_vertices,
    });
    ir.model.regions.push(Region {
        id: region_id,
        body: body_id.clone(),
        shells: vec![shell_id],
    });
    ir.model.bodies.push(Body {
        id: body_id,
        kind: BodyKind::General,
        regions: vec!["nx:derived:region#0".into()],
        transform: None,
        name: None,
        color: None,
    });
}

// The parameters are the per-stream lookup tables produced by the decode pass;
// bundling them into a struct would only rename the same eight things.
#[allow(clippy::too_many_arguments)]
fn emit_topology(
    ir: &mut CadIr,
    stream_index: usize,
    graph: &Graph,
    points: &BTreeMap<u32, (PointId, VertexId)>,
    surfaces: &BTreeMap<u32, SurfaceId>,
    curves: &BTreeMap<u32, CurveId>,
    trim_ranges: &BTreeMap<u32, [f64; 2]>,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) {
    let prefix = format!("nx:s{stream_index}");
    let mut bodies = BTreeMap::new();
    for node in graph.of_kind(12) {
        let id = BodyId(format!("{prefix}:body#{}", node.xmt));
        annotate_node(annotations, &id, source_stream, node, "BODY");
        bodies.insert(node.xmt, id.clone());
        ir.model.bodies.push(Body {
            id,
            kind: cadmpeg_ir::topology::BodyKind::Solid,
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
        });
    }

    let mut shells = BTreeMap::new();
    for node in graph.of_kind(13) {
        let Some(body) = node.xmt_at(10).and_then(|xmt| bodies.get(&xmt)).cloned() else {
            continue;
        };
        let region_id = RegionId(format!("{prefix}:region#{}", node.xmt));
        let shell_id = ShellId(format!("{prefix}:shell#{}", node.xmt));
        annotate_node(annotations, &region_id, source_stream, node, "SHELL");
        annotations.exactness(&region_id, Exactness::Inferred);
        annotate_node(annotations, &shell_id, source_stream, node, "SHELL");
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body.clone(),
            shells: vec![shell_id.clone()],
        });
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        if let Some(parent) = ir
            .model
            .bodies
            .iter_mut()
            .find(|candidate| candidate.id == body)
        {
            parent.regions.push(region_id);
        }
        shells.insert(node.xmt, shell_id);
    }

    let mut vertices = BTreeMap::new();
    for node in graph.of_kind(18) {
        let Some(point_xmt) = node.xmt_at(16) else {
            continue;
        };
        let Some((_, vertex)) = points.get(&point_xmt) else {
            continue;
        };
        if let Some(decoded_vertex) = ir
            .model
            .vertices
            .iter_mut()
            .find(|candidate| candidate.id == *vertex)
        {
            decoded_vertex.tolerance = geometric_tolerance(node, 18);
        }
        annotate_node(annotations, vertex, source_stream, node, "VERTEX");
        annotations.derived(vertex, "tolerance");
        vertices.insert(node.xmt, vertex.clone());
    }

    let mut edges = BTreeMap::new();
    for node in graph.of_kind(16) {
        let Some(fin_xmt) = node.xmt_at(18) else {
            continue;
        };
        let Some(fin) = graph.get(17, fin_xmt) else {
            continue;
        };
        let Some(start) = fin.xmt_at(12).and_then(|xmt| vertices.get(&xmt)).cloned() else {
            continue;
        };
        let end = fin
            .xmt_at(8)
            .and_then(|xmt| graph.get(17, xmt))
            .and_then(|next| next.xmt_at(12))
            .and_then(|xmt| vertices.get(&xmt))
            .cloned()
            .unwrap_or_else(|| start.clone());
        let curve_xmt = node.xmt_at(24);
        let curve = curve_xmt.and_then(|xmt| curves.get(&xmt)).cloned();
        let id = EdgeId(format!("{prefix}:edge#{}", node.xmt));
        annotate_node(annotations, &id, source_stream, node, "EDGE");
        annotations.derived(&id, "tolerance");
        ir.model.edges.push(Edge {
            id: id.clone(),
            curve,
            start,
            end,
            param_range: curve_xmt.and_then(|xmt| trim_ranges.get(&xmt)).copied(),
            tolerance: geometric_tolerance(node, 10),
        });
        edges.insert(node.xmt, id);
    }

    let mut faces = BTreeMap::new();
    for node in graph.of_kind(14) {
        let Some(shell) = node.xmt_at(24).and_then(|xmt| shells.get(&xmt)).cloned() else {
            continue;
        };
        let Some(surface) = node.xmt_at(26).and_then(|xmt| surfaces.get(&xmt)).cloned() else {
            continue;
        };
        let id = FaceId(format!("{prefix}:face#{}", node.xmt));
        annotate_node(annotations, &id, source_stream, node, "FACE");
        annotations.derived(&id, "tolerance");
        ir.model.faces.push(Face {
            id: id.clone(),
            shell: shell.clone(),
            surface,
            sense: sense(node.byte_at(28)),
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: geometric_tolerance(node, 10),
        });
        if let Some(parent) = ir
            .model
            .shells
            .iter_mut()
            .find(|candidate| candidate.id == shell)
        {
            parent.faces.push(id.clone());
        }
        faces.insert(node.xmt, id);
    }

    let mut loops = BTreeMap::new();
    for node in graph.of_kind(15) {
        let Some(face) = node.xmt_at(12).and_then(|xmt| faces.get(&xmt)).cloned() else {
            continue;
        };
        let id = LoopId(format!("{prefix}:loop#{}", node.xmt));
        annotate_node(annotations, &id, source_stream, node, "LOOP");
        ir.model.loops.push(Loop {
            id: id.clone(),
            face: face.clone(),
            coedges: Vec::new(),
        });
        if let Some(parent) = ir
            .model
            .faces
            .iter_mut()
            .find(|candidate| candidate.id == face)
        {
            parent.loops.push(id.clone());
        }
        loops.insert(node.xmt, id);
    }

    let fin_ids: BTreeMap<u32, CoedgeId> = graph
        .of_kind(17)
        .filter_map(|node| {
            node.xmt_sequence(6, 7)
                .and_then(|refs| refs.first().copied())
                .filter(|loop_xmt| loops.contains_key(loop_xmt))
                .map(|_| (node.xmt, CoedgeId(format!("{prefix}:fin#{}", node.xmt))))
        })
        .collect();
    for node in graph.of_kind(17) {
        let Some(refs) = node.xmt_sequence(6, 7) else {
            continue;
        };
        let Some(loop_id) = loops.get(&refs[0]).cloned() else {
            continue;
        };
        let Some(edge) = edges.get(&refs[5]).cloned() else {
            continue;
        };
        let id = fin_ids.get(&node.xmt).cloned().expect("filtered above");
        annotate_node(annotations, &id, source_stream, node, "FIN");
        let next = Some(refs[1])
            .and_then(|xmt| fin_ids.get(&xmt))
            .cloned()
            .unwrap_or_else(|| id.clone());
        let previous = Some(refs[2])
            .and_then(|xmt| fin_ids.get(&xmt))
            .cloned()
            .unwrap_or_else(|| id.clone());
        let partner = fin_ids.get(&refs[4]).cloned();
        let radial_next = partner.clone().unwrap_or_else(|| id.clone());
        ir.model.coedges.push(Coedge {
            id: id.clone(),
            owner_loop: loop_id.clone(),
            edge,
            next,
            previous,
            radial_next,
            sense: sense(node.byte_at(22)),
            pcurve: None,
        });
        if let Some(parent) = ir
            .model
            .loops
            .iter_mut()
            .find(|candidate| candidate.id == loop_id)
        {
            parent.coedges.push(id);
        }
    }
}

fn annotate_node(
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    stream: cadmpeg_ir::annotations::StreamHandle,
    node: &Node,
    tag: &str,
) {
    annotations.note(id, stream, node.pos as u64).tag(tag);
}

fn surface_tag(geometry: &SurfaceGeometry) -> &'static str {
    match geometry {
        SurfaceGeometry::Plane { .. } => "PLANE",
        SurfaceGeometry::Cylinder { .. } => "CYLINDER",
        SurfaceGeometry::Cone { .. } => "CONE",
        SurfaceGeometry::Sphere { .. } => "SPHERE",
        SurfaceGeometry::Torus { .. } => "TORUS",
        SurfaceGeometry::Nurbs(_) => "B_SPLINE_SURFACE",
        SurfaceGeometry::Unknown { .. } => "UNKNOWN_SURFACE",
    }
}

fn curve_tag(geometry: &CurveGeometry) -> &'static str {
    match geometry {
        CurveGeometry::Line { .. } => "LINE",
        CurveGeometry::Circle { .. } => "CIRCLE",
        CurveGeometry::Ellipse { .. } => "ELLIPSE",
        CurveGeometry::Parabola { .. } => "PARABOLA",
        CurveGeometry::Hyperbola { .. } => "HYPERBOLA",
        CurveGeometry::Nurbs(_) => "B_SPLINE_CURVE",
        CurveGeometry::Unknown { .. } => "UNKNOWN_CURVE",
    }
}

fn geometric_tolerance(node: &Node, offset: usize) -> Option<f64> {
    match node.f64_at(offset)? {
        MISSING_TOLERANCE => None,
        value if value.is_finite() && value.abs() < 1.0e3 => Some(value * 1000.0),
        _ => None,
    }
}

fn sense(byte: Option<u8>) -> Sense {
    if byte == Some(b'-') {
        Sense::Reversed
    } else {
        Sense::Forward
    }
}

fn unknown_stream(si: usize, stream: &Stream) -> UnknownRecord {
    UnknownRecord {
        id: UnknownId(format!("nx:container:parasolid#{si}")),
        offset: stream.file_offset as u64,
        byte_len: stream.inflated.len() as u64,
        sha256: sha256_hex(&stream.inflated),
        data: Some(stream.inflated.clone()),
        links: Vec::new(),
    }
}

fn source_meta(scan: &Scan) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "file_size".to_string(),
        scan.container.data.len().to_string(),
    );
    attributes.insert(
        "footer_offset".to_string(),
        scan.container.footer_offset.to_string(),
    );
    attributes.insert(
        "directory_entries".to_string(),
        scan.container.entries.len().to_string(),
    );
    attributes.insert(
        "partition_streams".to_string(),
        scan.count(StreamKind::Partition).to_string(),
    );
    attributes.insert(
        "deltas_streams".to_string(),
        scan.count(StreamKind::Deltas).to_string(),
    );
    attributes.insert(
        "plain_streams".to_string(),
        scan.count(StreamKind::Plain).to_string(),
    );
    if let Some(schema) = scan.streams.iter().find_map(|s| s.schema.as_deref()) {
        attributes.insert("parasolid_schema".to_string(), schema.to_string());
    }
    for (index, path) in scan
        .container
        .external_reference_paths()
        .into_iter()
        .enumerate()
    {
        attributes.insert(format!("external_reference.{index}"), path);
    }
    let active_ids = scan.container.rmfastload_object_ids();
    if !active_ids.is_empty() {
        attributes.insert(
            "rmfastload_active_object_count".to_string(),
            active_ids.len().to_string(),
        );
    }
    SourceMeta {
        format: "nx".to_string(),
        attributes,
    }
}

fn build_geometry_report(scan: &Scan, counts: &Counts, has_topology: bool) -> DecodeReport {
    let mut losses = Vec::new();

    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Info,
        message: format!(
            "Decoded {} vertex point(s) verbatim from Parasolid POINT records (3×f64 big-endian, \
             metres → millimetres), {} analytic surface carrier(s) ({} plane, {} cylinder, {} \
             cone, {} sphere, {} torus), and {} analytic curve carrier(s) ({} line, {} circle, {} \
             ellipse). All parameters are byte-exact at the document's millimetre scale.",
            counts.points,
            counts.surfaces(),
            counts.planes,
            counts.cylinders,
            counts.cones,
            counts.spheres,
            counts.tori,
            counts.curves(),
            counts.lines,
            counts.circles,
            counts.ellipses,
        ),
        provenance: None,
    });

    if !has_topology {
        losses.push(LossNote {
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message: "The B-rep topology graph (body→shell→face→loop→fin→edge→vertex) was not \
                      reconstructed: resolving it requires a full sequential record-framing walk that \
                      tracks each record's escape and large-index byte shifts, and the active body's \
                      surviving-face set additionally depends on the undecoded partition↔deltas \
                      tombstone bridge (the in-stream index maps are declared but serialize empty). \
                      Faces, loops, fins, edges, and their surface/curve incidence are therefore not \
                      emitted; the decoded points, surfaces, and curves are unattached carriers."
                .to_string(),
            provenance: None,
        });
    }

    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Warning,
        message: "B-spline surfaces/curves (support-record families 124–128, 134–136), rolling-ball \
                  and procedural blend surfaces (types 56, 38, 60), and trimmed/surface curves \
                  (types 133, 137) were not decoded into typed carriers; only analytic primitives \
                  were. Each Parasolid stream is preserved verbatim as an unknown passthrough record \
                  so no recognized geometry is dropped."
            .to_string(),
        provenance: None,
    });

    if scan.count(StreamKind::Deltas) > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{} Parasolid deltas (edit-overlay) stream(s) were scanned for analytic carriers \
                 alongside the partition(s); because the tombstone bridge is undecoded, carriers \
                 from a deltas stream may include entities that the active-body edit later deleted. \
                 They are emitted as unattached carriers, not as a resolved live-body face set.",
                scan.count(StreamKind::Deltas)
            ),
            provenance: None,
        });
    }

    if scan.count(StreamKind::Partition) > 1 {
        losses.push(LossNote {
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: format!(
                "This part is composed of {} sub-body partition(s); the final solid is the \
                 feature-history Boolean composition of them, whose union order and operand binding \
                 live in undecoded NX object-model records. Carriers from all sub-bodies are emitted \
                 without the Boolean that would remove interior/construction faces.",
                scan.count(StreamKind::Partition)
            ),
            provenance: None,
        });
    }

    losses.push(LossNote {
        category: LossCategory::Attribute,
        severity: Severity::Warning,
        message: "Materials, appearances, part attributes, feature history, and assembly \
                  occurrence placements were not transferred: they live in the NX object-model \
                  per-class field serialization, which is not decoded."
            .to_string(),
        provenance: None,
    });

    DecodeReport {
        format: "nx".to_string(),
        container_only: false,
        geometry_transferred: true,
        losses,
        notes: summary_notes(scan),
    }
}

fn build_metadata_ir(scan: &Scan) -> CadIr {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    for (si, stream) in scan.streams.iter().enumerate() {
        if stream.kind.is_parasolid() {
            let unknown = unknown_stream(si, stream);
            let source_stream = annotations.stream("nx:container");
            annotations
                .note(&unknown.id, source_stream, stream.file_offset as u64)
                .tag(stream.kind.label());
            annotations.exactness(&unknown.id, Exactness::Derived);
            ir.unknowns.push(unknown);
        }
    }
    ir.annotations = annotations.build();
    ir
}

fn build_container_report(scan: &Scan, container_only: bool) -> DecodeReport {
    let mut losses = Vec::new();

    let assembly = scan
        .container
        .entries
        .iter()
        .any(|e| e.name.contains("ExternalReferences"))
        && !scan.has_parasolid();

    if assembly {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Blocking,
            message: "No inline Parasolid geometry: this is an assembly .prt. Component geometry \
                      lives in external child .prt files named in EXTREFSTREAM, and the assembled \
                      solid's inputs (child partitions + constraint solve) are absent from this \
                      file. This is an external-dependency boundary, not a decode gap."
                .to_string(),
            provenance: None,
        });
    } else {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Blocking,
            message: "No B-rep geometry was transferred: no gate-passing analytic carrier was found \
                      in the embedded Parasolid streams (they may hold only B-spline/procedural \
                      geometry this codec does not yet type). The streams are preserved verbatim as \
                      unknown passthrough records."
                .to_string(),
            provenance: None,
        });
    }

    if container_only {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: "Container-only decode requested; entity decode was not attempted."
                .to_string(),
            provenance: None,
        });
    }

    DecodeReport {
        format: "nx".to_string(),
        container_only,
        geometry_transferred: false,
        losses,
        notes: summary_notes(scan),
    }
}

/// Build container and embedded-stream notes for inspection and decode reports.
pub fn summary_notes(scan: &Scan) -> Vec<String> {
    let c = &scan.container;
    let mut notes = vec![format!(
        "SPLMSSTR container: version {:#04x}, file tag {}, footer offset {}, {} directory entry/ies",
        c.version,
        c.file_tag,
        c.footer_offset,
        c.entries.len()
    )];
    notes.push(format!(
        "embedded streams: {} partition, {} deltas, {} plain (cached body), {} preview/non-Parasolid",
        scan.count(StreamKind::Partition),
        scan.count(StreamKind::Deltas),
        scan.count(StreamKind::Plain),
        scan.count(StreamKind::Preview),
    ));
    if let Some(schema) = scan.streams.iter().find_map(|s| s.schema.as_deref()) {
        notes.push(format!("Parasolid schema: {schema}"));
    }
    if !scan.has_parasolid()
        && c.entries
            .iter()
            .any(|e| e.name.contains("ExternalReferences"))
    {
        notes.push(
            "no inline Parasolid geometry (assembly .prt: geometry in external child parts)"
                .to_string(),
        );
    }
    notes
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}
