// SPDX-License-Identifier: Apache-2.0
//! Decode a Siemens NX `.prt` into an IR document, transferring the analytic
//! geometry carriers this codec understands and reporting every unrecovered layer
//! as explicit, counted loss.
//!
//! The container (SPLMSSTR header + directory) and the embedded Parasolid streams
//! are located by [`crate::container`] and [`crate::parasolid`]. From every
//! partition/deltas/plain Parasolid stream this module reads the gate-passing
//! POINT, analytic-surface, and analytic-curve carriers ([`crate::geometry`]) into
//! free carrier arenas, and preserves each stream verbatim as an [`UnknownRecord`].
//! Where the stream's topology records resolve ([`crate::topology`]), the
//! body→shell→face→loop→fin→edge→vertex graph is reconstructed and attached to
//! those carriers; a stream that yields no topology is reported as a counted
//! loss instead.

use std::collections::BTreeMap;

use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{Curve, Surface};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, LumpId, PointId, ShellId, SurfaceId,
    UnknownId, VertexId,
};
use cadmpeg_ir::provenance::{EntityMeta, Exactness, Provenance};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::topology::{Body, Coedge, Edge, Face, Loop, Lump, Point, Sense, Shell, Vertex};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;

use crate::container::{self, Container};
use crate::geometry;
use crate::parasolid::{self, Stream, StreamKind};
use crate::topology::Graph;

/// Everything read from a `.prt`, shared by `inspect` and `decode`.
pub struct Scan {
    /// Parsed SPLMSSTR container.
    pub container: Container,
    /// Located, inflated Parasolid/preview streams.
    pub streams: Vec<Stream>,
}

impl Scan {
    /// Count of streams of a given kind.
    pub fn count(&self, kind: StreamKind) -> usize {
        self.streams.iter().filter(|s| s.kind == kind).count()
    }

    /// Whether this file carries any inline Parasolid geometry. An assembly `.prt`
    /// carries none (its geometry lives in external child parts).
    pub fn has_parasolid(&self) -> bool {
        self.streams.iter().any(|s| s.kind.is_parasolid())
    }
}

/// Read and inflate everything from a reader.
pub fn scan(reader: &mut dyn ReadSeek) -> Result<Scan, CodecError> {
    let container = container::scan(reader)?;
    let streams = parasolid::extract_streams(&container.data);
    Ok(Scan { container, streams })
}

/// Decode a `.prt` reader into an IR + report.
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = scan(reader)?;

    if options.container_only {
        let ir = build_metadata_ir(&scan);
        let report = build_container_report(&scan, true);
        return Ok(DecodeResult { ir, report });
    }

    if let Some((ir, report)) = try_decode_geometry(&scan) {
        return Ok(DecodeResult { ir, report });
    }

    let ir = build_metadata_ir(&scan);
    let report = build_container_report(&scan, false);
    Ok(DecodeResult { ir, report })
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
    use cadmpeg_ir::geometry::CurveGeometry;
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(source_meta(scan));
    let mut counts = Counts::default();

    for (si, stream) in scan.streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        let stream_name = format!("parasolid#{si}:{}", stream.kind.label());
        let graph = Graph::parse(&stream.inflated);
        let mut points_by_xmt = BTreeMap::new();
        let mut surfaces_by_xmt = BTreeMap::new();
        let mut curves_by_xmt = BTreeMap::new();
        let mut trim_ranges = BTreeMap::new();

        for (pi, pt) in geometry::points(&stream.inflated).into_iter().enumerate() {
            let pid = PointId(format!("nx:s{si}:pt#{pi}"));
            let vid = VertexId(format!("nx:s{si}:v#{pi}"));
            ir.points.push(Point {
                id: pid.clone(),
                position: pt.position,
                meta: byte_exact(&stream_name, pt.pos as u64, "point"),
            });
            ir.vertices.push(Vertex {
                id: vid.clone(),
                point: pid,
                meta: byte_exact(&stream_name, pt.pos as u64, "point"),
            });
            if let Some(node) = graph.at_pos(pt.pos) {
                if node.kind == 29 {
                    points_by_xmt.insert(node.xmt, (ir.points.last().unwrap().id.clone(), vid));
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
                _ => {}
            }
            let id = SurfaceId(format!("nx:s{si}:surf#{fi}"));
            ir.surfaces.push(Surface {
                id: id.clone(),
                geometry: surf.geometry,
                meta: byte_exact(&stream_name, surf.pos as u64, "analytic_surface"),
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
            ir.surfaces.push(Surface {
                id: id.clone(),
                geometry: surf.geometry,
                meta: byte_exact(&stream_name, surf.pos as u64, "bspline_surface"),
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
                _ => {}
            }
            let id = CurveId(format!("nx:s{si}:crv#{ci}"));
            ir.curves.push(Curve {
                id: id.clone(),
                geometry: crv.geometry,
                meta: byte_exact(&stream_name, crv.pos as u64, "analytic_curve"),
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
            ir.curves.push(Curve {
                id: id.clone(),
                geometry: crv.geometry,
                meta: byte_exact(&stream_name, crv.pos as u64, "bspline_curve"),
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
            &stream_name,
        );

        // Preserve the whole inflated stream verbatim so nothing is dropped.
        ir.unknowns.push(unknown_stream(si, stream));
    }

    if counts.points == 0 && counts.surfaces() == 0 && counts.curves() == 0 {
        return None;
    }

    let report = build_geometry_report(scan, &counts, !ir.faces.is_empty());
    Some((ir, report))
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
    stream_name: &str,
) {
    let prefix = format!("nx:s{stream_index}");
    let mut bodies = BTreeMap::new();
    for node in graph.of_kind(12) {
        let id = BodyId(format!("{prefix}:body#{}", node.xmt));
        bodies.insert(node.xmt, id.clone());
        ir.bodies.push(Body {
            id,
            kind: cadmpeg_ir::topology::BodyKind::Solid,
            lumps: Vec::new(),
            transform: None,
            name: None,
            color: None,
            meta: byte_exact(stream_name, node.pos as u64, "body"),
        });
    }

    let mut shells = BTreeMap::new();
    for node in graph.of_kind(13) {
        let Some(body) = node.xmt_at(10).and_then(|xmt| bodies.get(&xmt)).cloned() else {
            continue;
        };
        let lump_id = LumpId(format!("{prefix}:lump#{}", node.xmt));
        let shell_id = ShellId(format!("{prefix}:shell#{}", node.xmt));
        ir.lumps.push(Lump {
            id: lump_id.clone(),
            body: body.clone(),
            shells: vec![shell_id.clone()],
            meta: byte_exact(stream_name, node.pos as u64, "lump"),
        });
        ir.shells.push(Shell {
            id: shell_id.clone(),
            lump: lump_id.clone(),
            faces: Vec::new(),
            meta: byte_exact(stream_name, node.pos as u64, "shell"),
        });
        if let Some(parent) = ir.bodies.iter_mut().find(|candidate| candidate.id == body) {
            parent.lumps.push(lump_id);
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
        ir.edges.push(Edge {
            id: id.clone(),
            curve,
            start,
            end,
            param_range: curve_xmt.and_then(|xmt| trim_ranges.get(&xmt)).copied(),
            meta: byte_exact(stream_name, node.pos as u64, "edge"),
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
        ir.faces.push(Face {
            id: id.clone(),
            shell: shell.clone(),
            surface,
            sense: sense(node.byte_at(28)),
            loops: Vec::new(),
            name: None,
            color: None,
            meta: byte_exact(stream_name, node.pos as u64, "face"),
        });
        if let Some(parent) = ir.shells.iter_mut().find(|candidate| candidate.id == shell) {
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
        ir.loops.push(Loop {
            id: id.clone(),
            face: face.clone(),
            coedges: Vec::new(),
            meta: byte_exact(stream_name, node.pos as u64, "loop"),
        });
        if let Some(parent) = ir.faces.iter_mut().find(|candidate| candidate.id == face) {
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
        let next = Some(refs[1])
            .and_then(|xmt| fin_ids.get(&xmt))
            .cloned()
            .unwrap_or_else(|| id.clone());
        let previous = Some(refs[2])
            .and_then(|xmt| fin_ids.get(&xmt))
            .cloned()
            .unwrap_or_else(|| id.clone());
        let partner = fin_ids.get(&refs[4]).cloned();
        ir.coedges.push(Coedge {
            id: id.clone(),
            owner_loop: loop_id.clone(),
            edge,
            next,
            previous,
            partner,
            sense: sense(node.byte_at(22)),
            pcurve: None,
            meta: byte_exact(stream_name, node.pos as u64, "fin"),
        });
        if let Some(parent) = ir
            .loops
            .iter_mut()
            .find(|candidate| candidate.id == loop_id)
        {
            parent.coedges.push(id);
        }
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
        id: UnknownId(format!("nx:parasolid#{si}")),
        offset: stream.file_offset as u64,
        byte_len: stream.inflated.len() as u64,
        sha256: sha256_hex(&stream.inflated),
        data: Some(stream.inflated.clone()),
        links: Vec::new(),
        meta: EntityMeta {
            provenance: Provenance {
                format: "nx".to_string(),
                stream: format!("parasolid#{si}:{}", stream.kind.label()),
                offset: stream.file_offset as u64,
                tag: stream.schema.clone(),
            },
            exactness: Exactness::Unknown,
        },
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
    ir.source = Some(source_meta(scan));
    for (si, stream) in scan.streams.iter().enumerate() {
        if stream.kind.is_parasolid() {
            ir.unknowns.push(unknown_stream(si, stream));
        }
    }
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

/// Container-level informational notes shared by every report and the inspect
/// summary.
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

fn byte_exact(stream: &str, offset: u64, tag: &str) -> EntityMeta {
    EntityMeta {
        provenance: Provenance {
            format: "nx".to_string(),
            stream: stream.to_string(),
            offset,
            tag: Some(tag.to_string()),
        },
        exactness: Exactness::ByteExact,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
