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

use cadmpeg_ir::codec::{CodecError, DecodeResult};
use cadmpeg_ir::decode::{DecodeContext, View};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, BlendSupport, Curve, CurveGeometry, IntcurveSupportContext,
    IntcurveSupportSide, NurbsCurve, PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition,
    ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossCode, LossNote, Severity};
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
pub fn scan<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Scan, CodecError> {
    let image = root.window();
    let container = container::scan_bytes(image.to_vec())?;
    let streams = parasolid::extract_streams(ctx, root, &container)?;
    Ok(Scan { container, streams })
}

/// Decode an NX `.prt` into IR and a loss report.
///
/// When [`DecodeContext::container_only`] is set, the returned IR contains source
/// metadata and preserved streams but no typed entities. Otherwise the decoder
/// emits supported geometry and resolvable topology. A valid container can
/// decode successfully with no geometry, including an assembly whose geometry
/// resides in external child parts.
pub fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<DecodeResult, CodecError> {
    let scan = scan(ctx, root)?;

    if ctx.container_only() {
        let (ir, annotations, unknowns) = build_metadata_ir(&scan)?;
        let mut report = build_container_report(&scan, true);
        report_untransferred_streams(&scan, &mut report);
        return decode_result(ir, report, annotations, &unknowns);
    }

    if let Some((ir, report, annotations, unknowns)) = try_decode_geometry(&scan) {
        return decode_result(ir, report, annotations, &unknowns);
    }

    let (ir, annotations, unknowns) = build_metadata_ir(&scan)?;
    let mut report = build_container_report(&scan, false);
    report_untransferred_streams(&scan, &mut report);
    decode_result(ir, report, annotations, &unknowns)
}

fn decode_result(
    mut ir: CadIr,
    report: DecodeReport,
    annotations: cadmpeg_ir::Annotations,
    unknowns: &[UnknownRecord],
) -> Result<DecodeResult, CodecError> {
    let mut source_fidelity = cadmpeg_ir::SourceFidelity {
        annotations,
        ..Default::default()
    };
    source_fidelity.attach_native_unknown_records(&mut ir, "nx", unknowns)?;
    Ok(DecodeResult::with_source_fidelity(
        ir,
        report,
        source_fidelity,
    ))
}

/// Reject strict decodes that omit mandatory, unreconstructable semantics.
/// Reports classified streams that produced no transferable representation.
fn report_untransferred_streams(scan: &Scan, report: &mut DecodeReport) {
    for (index, stream) in scan.streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            report.losses.push(stream_drop_note(index, stream));
        }
    }
}

/// The accountable loss note for a non-Parasolid stream that produced no
/// surviving typed entity. Parasolid streams are preserved as native unknown
/// records and never reach this path.
fn stream_drop_note(stream_index: usize, stream: &Stream) -> LossNote {
    LossNote {
        code: LossCode::PassthroughRecordOmitted,
        category: LossCategory::Other,
        severity: Severity::Info,
        message: format!(
            "Non-Parasolid {} stream #{stream_index} was classified but not transferred.",
            stream.kind.label()
        ),
        provenance: None,
    }
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
    offset_surfaces: usize,
    blend_surfaces: usize,
    lines: usize,
    circles: usize,
    ellipses: usize,
    nurbs_curves: usize,
    intersection_curves: usize,
    unresolved_intersections: usize,
}

impl Counts {
    fn surfaces(&self) -> usize {
        self.planes
            + self.cylinders
            + self.cones
            + self.spheres
            + self.tori
            + self.nurbs_surfaces
            + self.offset_surfaces
            + self.blend_surfaces
    }
    fn curves(&self) -> usize {
        self.lines + self.circles + self.ellipses + self.nurbs_curves + self.intersection_curves
    }
}

/// Decode analytic carriers from every Parasolid stream. Returns `None` when no
/// carrier of any kind passes its gate, so the caller falls back to metadata.
fn try_decode_geometry(
    scan: &Scan,
) -> Option<(
    CadIr,
    DecodeReport,
    cadmpeg_ir::Annotations,
    Vec<UnknownRecord>,
)> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    let mut counts = Counts::default();
    let mut body_node_ids = BTreeMap::new();
    let semantic_streams = semantic_streams(scan);

    for (si, stream) in scan.streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        let semantic = &semantic_streams[si];
        let stream_name = format!("parasolid#{si}:{}", stream.kind.label());
        let source_stream = annotations.stream(format!("nx:{stream_name}"));
        let graph = Graph::parse(semantic);
        body_node_ids.extend(topology_body_node_ids(si, &graph));
        let mut points_by_xmt = BTreeMap::new();
        let mut surfaces_by_xmt = BTreeMap::new();
        let mut curves_by_xmt = BTreeMap::new();
        let mut trim_ranges = BTreeMap::new();
        let first_surface = ir.model.surfaces.len();
        let first_curve = ir.model.curves.len();
        for (pi, pt) in geometry::points(semantic).into_iter().enumerate() {
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
                source_object: None,
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

        for (fi, surf) in geometry::surfaces(semantic).into_iter().enumerate() {
            match &surf.geometry {
                SurfaceGeometry::Plane { .. } => counts.planes += 1,
                SurfaceGeometry::Cylinder { .. } => counts.cylinders += 1,
                SurfaceGeometry::Cone { .. } => counts.cones += 1,
                SurfaceGeometry::Sphere { .. } => counts.spheres += 1,
                SurfaceGeometry::Torus { .. } => counts.tori += 1,
                SurfaceGeometry::Nurbs(_)
                | SurfaceGeometry::Polygonal { .. }
                | SurfaceGeometry::Transformed { .. }
                | SurfaceGeometry::Unknown { .. } => {}
            }
            let id = SurfaceId(format!("nx:s{si}:surf#{fi}"));
            annotations
                .note(&id, source_stream, surf.pos as u64)
                .tag(surface_tag(&surf.geometry));
            annotations.derived(&id, "geometry");
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: surf.geometry,
                source_object: None,
            });
            if let Some(node) = graph.at_pos(surf.pos) {
                surfaces_by_xmt.insert(node.xmt, id);
            }
        }

        for (fi, surf) in crate::nurbs::surfaces(semantic).into_iter().enumerate() {
            counts.nurbs_surfaces += 1;
            let id = SurfaceId(format!("nx:s{si}:nurbs-surf#{fi}"));
            annotations
                .note(&id, source_stream, surf.pos as u64)
                .tag("B_SPLINE_SURFACE");
            annotations.derived(&id, "geometry");
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: surf.geometry,
                source_object: None,
            });
            if let Some(node) = graph.at_pos(surf.pos) {
                surfaces_by_xmt.insert(node.xmt, id);
            }
        }

        for (oi, offset) in crate::topology::offset_surfaces(semantic)
            .into_iter()
            .enumerate()
        {
            let Some(support) = surfaces_by_xmt.get(&offset.support).cloned() else {
                continue;
            };
            let surface_id = SurfaceId(format!("nx:s{si}:offset-surf#{oi}"));
            let procedural_id = ProceduralSurfaceId(format!("nx:s{si}:offset#{oi}"));
            annotations
                .note(&surface_id, source_stream, offset.pos as u64)
                .tag("OFFSET_SURF");
            annotations.exactness(&surface_id, Exactness::Unknown);
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Unknown {
                    record: Some(UnknownId(format!("nx:container:parasolid#{si}"))),
                },
                source_object: None,
            });
            annotations
                .note(&procedural_id, source_stream, offset.pos as u64)
                .tag("OFFSET_SURF");
            annotations.derived(&procedural_id, "definition");
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: procedural_id,
                surface: surface_id.clone(),
                definition: ProceduralSurfaceDefinition::Offset {
                    support,
                    distance: offset.distance,
                    u_sense: Some(0),
                    v_sense: Some(0),
                    extension_flags: Vec::new(),
                },
                cache_fit_tolerance: None,
            });
            surfaces_by_xmt.insert(offset.xmt, surface_id);
            counts.offset_surfaces += 1;
        }

        for (bi, blend) in crate::topology::blend_surfaces(semantic)
            .into_iter()
            .enumerate()
        {
            let supports = blend.supports.map(|support_xmt| {
                surfaces_by_xmt
                    .get(&support_xmt)
                    .cloned()
                    .map(|surface| BlendSupport {
                        surface,
                        reversed: false,
                    })
            });
            if supports.iter().all(Option::is_none) {
                continue;
            }
            let mut supports = supports;
            for (side, offset) in supports.iter_mut().zip(blend.offsets) {
                if let Some(side) = side {
                    side.reversed = offset.is_sign_negative();
                }
            }
            let surface_id = SurfaceId(format!("nx:s{si}:blend-surf#{bi}"));
            let procedural_id = ProceduralSurfaceId(format!("nx:s{si}:blend#{bi}"));
            annotations
                .note(&surface_id, source_stream, blend.pos as u64)
                .tag("BLEND_SURF");
            annotations.exactness(&surface_id, Exactness::Unknown);
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Unknown {
                    record: Some(UnknownId(format!("nx:container:parasolid#{si}"))),
                },
                source_object: None,
            });
            annotations
                .note(&procedural_id, source_stream, blend.pos as u64)
                .tag("BLEND_SURF");
            annotations.derived(&procedural_id, "definition");
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: procedural_id,
                surface: surface_id.clone(),
                definition: ProceduralSurfaceDefinition::Blend {
                    supports,
                    spine: None,
                    radius: BlendRadiusLaw::Constant {
                        signed_radius: blend.offsets[0].abs(),
                    },
                    cross_section: BlendCrossSection::Circular,
                    native: None,
                },
                cache_fit_tolerance: None,
            });
            surfaces_by_xmt.insert(blend.xmt, surface_id);
            counts.blend_surfaces += 1;
        }

        for (ci, crv) in geometry::curves(semantic).into_iter().enumerate() {
            match &crv.geometry {
                CurveGeometry::Line { .. } => counts.lines += 1,
                CurveGeometry::Circle { .. } => counts.circles += 1,
                CurveGeometry::Ellipse { .. } => counts.ellipses += 1,
                CurveGeometry::Parabola { .. }
                | CurveGeometry::Hyperbola { .. }
                | CurveGeometry::Degenerate { .. }
                | CurveGeometry::Composite { .. }
                | CurveGeometry::Nurbs(_)
                | CurveGeometry::Polyline { .. }
                | CurveGeometry::Transformed { .. }
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
                source_object: None,
            });
            if let Some(node) = graph.at_pos(crv.pos) {
                curves_by_xmt.insert(node.xmt, id);
            }
        }

        for (ci, crv) in crate::nurbs::curves(semantic).into_iter().enumerate() {
            counts.nurbs_curves += 1;
            let id = CurveId(format!("nx:s{si}:nurbs-crv#{ci}"));
            annotations
                .note(&id, source_stream, crv.pos as u64)
                .tag("B_SPLINE_CURVE");
            annotations.derived(&id, "geometry");
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry: crv.geometry,
                source_object: None,
            });
            if let Some(node) = graph.at_pos(crv.pos) {
                curves_by_xmt.insert(node.xmt, id);
            }
        }

        let charted_intersections: BTreeMap<_, _> = crate::intersection::curves(semantic)
            .into_iter()
            .map(|curve| (curve.xmt, curve))
            .collect();
        for (ci, construction) in crate::topology::composite_curves(semantic)
            .into_iter()
            .chain(crate::topology::intersection_data_curves(semantic))
            .enumerate()
        {
            let curve_id = CurveId(format!("nx:s{si}:intersection-crv#{ci}"));
            let procedural_id = ProceduralCurveId(format!("nx:s{si}:intersection#{ci}"));
            let unknown_id = UnknownId(format!("nx:container:parasolid#{si}"));
            let charted = charted_intersections.get(&construction.xmt);
            if charted.is_none() {
                counts.unresolved_intersections += 1;
            }
            annotations
                .note(&curve_id, source_stream, construction.pos as u64)
                .tag("INTERSECTION");
            if charted.is_some() {
                annotations.derived(&curve_id, "geometry");
            } else {
                annotations.exactness(&curve_id, Exactness::Unknown);
            }
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: charted.map_or_else(
                    || CurveGeometry::Unknown {
                        record: Some(unknown_id.clone()),
                    },
                    |charted| {
                        CurveGeometry::Nurbs(NurbsCurve {
                            degree: 1,
                            knots: linear_knots(&charted.parameters),
                            control_points: charted.points.clone(),
                            weights: None,
                            periodic: false,
                        })
                    },
                ),
                source_object: None,
            });
            annotations
                .note(&procedural_id, source_stream, construction.pos as u64)
                .tag("INTERSECTION");
            if charted.is_some() {
                annotations.derived(&procedural_id, "definition");
            } else {
                annotations.exactness(&procedural_id, Exactness::Unknown);
            }
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedural_id,
                curve: curve_id.clone(),
                definition: charted.map_or_else(
                    || ProceduralCurveDefinition::Unknown {
                        record: Some(unknown_id),
                    },
                    |charted| {
                        let first = intersection_side(
                            &ir,
                            &surfaces_by_xmt,
                            Some(charted.primary_support),
                            charted.support_uv[0]
                                .as_deref()
                                .map(|uv| (uv, charted.parameters.as_slice())),
                        );
                        let second = intersection_side(
                            &ir,
                            &surfaces_by_xmt,
                            charted.secondary_support,
                            charted.support_uv[1]
                                .as_deref()
                                .map(|uv| (uv, charted.parameters.as_slice())),
                        );
                        ProceduralCurveDefinition::Intersection {
                            context: IntcurveSupportContext {
                                sides: [first, second],
                                parameter_range: [
                                    charted.parameters[0],
                                    *charted
                                        .parameters
                                        .last()
                                        .expect("validated chart has points"),
                                ],
                                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                            },
                            discontinuity_flag: false,
                        }
                    },
                ),
                cache_fit_tolerance: charted.map(|charted| charted.fit_tolerance),
            });
            curves_by_xmt.insert(construction.xmt, curve_id);
            counts.intersection_curves += 1;
        }

        for trim in crate::topology::trimmed_curves(semantic) {
            if let Some(basis) = curves_by_xmt.get(&trim.basis).cloned() {
                curves_by_xmt.insert(trim.xmt, basis);
                trim_ranges.insert(trim.xmt, trim.parameters);
            }
        }
        for (xmt, basis_xmt) in crate::topology::surface_curves(semantic) {
            if let Some(basis) = curves_by_xmt.get(&basis_xmt).cloned() {
                curves_by_xmt.insert(xmt, basis);
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
        unknowns.push(unknown);
    }

    if counts.points == 0 && counts.surfaces() == 0 && counts.curves() == 0 {
        return None;
    }

    attach_native_object_model(&mut ir, scan).ok()?;

    select_active_body(
        &mut ir,
        &body_node_ids,
        &scan.container.rmfastload_object_ids(),
    );
    attach_free_topology(&mut ir, &mut annotations);
    let mut annotations = annotations.build();
    retain_live_annotations(&ir, &unknowns, &mut annotations);
    let mut report = build_geometry_report(scan, &counts, !ir.model.faces.is_empty());
    report_untransferred_streams(scan, &mut report);
    Some((ir, report, annotations, unknowns))
}

fn semantic_streams(scan: &Scan) -> Vec<Vec<u8>> {
    let mut semantic = scan
        .streams
        .iter()
        .map(|stream| stream.inflated.clone())
        .collect::<Vec<_>>();
    let mut index = 0;
    while index < scan.streams.len() {
        if scan.streams[index].kind != StreamKind::Partition {
            index += 1;
            continue;
        }
        let mut next = index + 1;
        while next < scan.streams.len()
            && scan.streams[next].kind == StreamKind::Deltas
            && scan.streams[next].schema == scan.streams[index].schema
        {
            semantic[index] = crate::deltas::merge_full_records(&semantic[index], &semantic[next]);
            semantic[next].clear();
            next += 1;
        }
        index = next;
    }
    semantic
}

fn retain_live_annotations(
    ir: &CadIr,
    unknowns: &[UnknownRecord],
    annotations: &mut cadmpeg_ir::Annotations,
) {
    let mut ids = BTreeSet::new();
    macro_rules! add_ids {
        ($($arena:expr),+ $(,)?) => {
            $(ids.extend($arena.iter().map(|entity| entity.id.to_string()));)+
        };
    }
    add_ids!(
        ir.model.bodies,
        ir.model.regions,
        ir.model.shells,
        ir.model.faces,
        ir.model.loops,
        ir.model.coedges,
        ir.model.edges,
        ir.model.vertices,
        ir.model.points,
        ir.model.surfaces,
        ir.model.curves,
        ir.model.pcurves,
        ir.model.procedural_surfaces,
        ir.model.procedural_curves,
    );
    ids.extend(unknowns.iter().map(|unknown| unknown.id.to_string()));
    annotations.provenance.retain(|id, _| ids.contains(id));
    annotations.exactness.retain(|id, _| ids.contains(id));
}

fn topology_body_node_ids(stream_index: usize, graph: &Graph) -> BTreeMap<BodyId, BTreeSet<u32>> {
    let prefix = format!("nx:s{stream_index}");
    graph
        .of_kind(12)
        .map(|body| {
            let shells: BTreeSet<_> = graph
                .of_kind(13)
                .filter(|shell| {
                    shell
                        .shell_fields()
                        .is_some_and(|fields| fields.body == body.xmt)
                })
                .map(|shell| shell.xmt)
                .collect();
            let faces: Vec<_> = graph
                .of_kind(14)
                .filter(|face| {
                    face.face_fields()
                        .is_some_and(|fields| shells.contains(&fields.shell))
                })
                .collect();
            let face_xmts: BTreeSet<_> = faces.iter().map(|face| face.xmt).collect();
            let loops: BTreeSet<_> = graph
                .of_kind(15)
                .filter(|loop_| {
                    loop_
                        .loop_fields()
                        .is_some_and(|fields| face_xmts.contains(&fields.face))
                })
                .map(|loop_| loop_.xmt)
                .collect();
            let fins: Vec<_> = graph
                .of_kind(17)
                .filter(|fin| {
                    fin.fin_fields()
                        .is_some_and(|fields| loops.contains(&fields.loop_xmt))
                })
                .collect();
            let edge_xmts: BTreeSet<_> = fins
                .iter()
                .filter_map(|fin| fin.fin_fields().map(|fields| fields.edge))
                .collect();
            let vertex_xmts: BTreeSet<_> = fins
                .iter()
                .filter_map(|fin| fin.fin_fields().map(|fields| fields.vertex))
                .collect();
            let ids = faces
                .into_iter()
                .filter_map(|face| face.u32_at(4))
                .chain(
                    graph
                        .of_kind(16)
                        .filter(|edge| edge_xmts.contains(&edge.xmt))
                        .filter_map(|edge| edge.u32_at(4)),
                )
                .chain(
                    graph
                        .of_kind(18)
                        .filter(|vertex| vertex_xmts.contains(&vertex.xmt))
                        .filter_map(|vertex| vertex.u32_at(4)),
                )
                .collect();
            (BodyId(format!("{prefix}:body#{}", body.xmt)), ids)
        })
        .collect()
}

fn select_active_body(
    ir: &mut CadIr,
    body_node_ids: &BTreeMap<BodyId, BTreeSet<u32>>,
    rmfastload_ids: &[u32],
) {
    if rmfastload_ids.is_empty() || ir.model.bodies.len() <= 1 {
        return;
    }
    let active: BTreeSet<_> = rmfastload_ids.iter().copied().collect();
    let mut scored: Vec<_> = ir
        .model
        .bodies
        .iter()
        .map(|body| {
            let ids = body_node_ids.get(&body.id);
            let count = ids.map_or(0, BTreeSet::len);
            let hits = ids.map_or(0, |ids| ids.intersection(&active).count());
            (hits, count, body.id.clone())
        })
        .collect();
    scored.sort_by(|first, second| second.0.cmp(&first.0).then(second.1.cmp(&first.1)));
    let Some(&(top_hits, top_count, ref winner)) = scored.first() else {
        return;
    };
    let next_hits = scored.get(1).map_or(0, |score| score.0);
    if top_count == 0
        || (top_hits as f64 / top_count as f64) < 0.10
        || top_hits < 50
        || top_hits < 5 * next_hits.max(1)
    {
        return;
    }
    prune_inactive_topology(ir, winner);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "active_body_selector".to_string(),
            "rmfastload_object_id_membership".to_string(),
        );
        source
            .attributes
            .insert("rmfastload_hits".to_string(), top_hits.to_string());
    }
}

fn prune_inactive_topology(ir: &mut CadIr, winner: &BodyId) {
    ir.model.bodies.retain(|body| &body.id == winner);
    ir.model.regions.retain(|region| &region.body == winner);
    let regions: BTreeSet<_> = ir
        .model
        .regions
        .iter()
        .map(|region| region.id.clone())
        .collect();
    ir.model
        .shells
        .retain(|shell| regions.contains(&shell.region));
    let shells: BTreeSet<_> = ir
        .model
        .shells
        .iter()
        .map(|shell| shell.id.clone())
        .collect();
    ir.model.faces.retain(|face| shells.contains(&face.shell));
    let faces: BTreeSet<_> = ir.model.faces.iter().map(|face| face.id.clone()).collect();
    ir.model.loops.retain(|loop_| faces.contains(&loop_.face));
    let loops: BTreeSet<_> = ir
        .model
        .loops
        .iter()
        .map(|loop_| loop_.id.clone())
        .collect();
    ir.model
        .coedges
        .retain(|coedge| loops.contains(&coedge.owner_loop));
    let edges: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.clone())
        .chain(
            ir.model
                .shells
                .iter()
                .flat_map(|shell| shell.wire_edges.iter().cloned()),
        )
        .collect();
    ir.model.edges.retain(|edge| edges.contains(&edge.id));
    let vertices: BTreeSet<_> = ir
        .model
        .edges
        .iter()
        .flat_map(|edge| [edge.start.clone(), edge.end.clone()])
        .chain(
            ir.model
                .shells
                .iter()
                .flat_map(|shell| shell.free_vertices.iter().cloned()),
        )
        .collect();
    ir.model
        .vertices
        .retain(|vertex| vertices.contains(&vertex.id));
    let points: BTreeSet<_> = ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect();
    ir.model.points.retain(|point| points.contains(&point.id));
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
        visible: None,
    });
}

fn linear_knots(parameters: &[f64]) -> Vec<f64> {
    let mut knots = Vec::with_capacity(parameters.len() + 2);
    knots.push(parameters[0]);
    knots.extend_from_slice(parameters);
    knots.push(*parameters.last().expect("non-empty chart parameters"));
    knots
}

fn intersection_side(
    ir: &CadIr,
    surfaces_by_xmt: &BTreeMap<u32, SurfaceId>,
    surface_xmt: Option<u32>,
    uv: Option<(&[[f64; 2]], &[f64])>,
) -> IntcurveSupportSide {
    let surface = surface_xmt.and_then(|xmt| surfaces_by_xmt.get(&xmt).cloned());
    let pcurve = surface.as_ref().and_then(|surface_id| {
        let geometry = ir
            .model
            .surfaces
            .iter()
            .find(|candidate| &candidate.id == surface_id)
            .map(|surface| &surface.geometry)?;
        let (uv, parameters) = uv?;
        Some(PcurveGeometry::Nurbs {
            degree: 1,
            knots: linear_knots(parameters),
            control_points: uv
                .iter()
                .map(|pair| surface_parameters(geometry, *pair))
                .collect(),
            weights: None,
            periodic: false,
        })
    });
    IntcurveSupportSide {
        surface,
        pcurve,
        pcurve_parameter_range: None,
    }
}

fn surface_parameters(surface: &SurfaceGeometry, uv: [f64; 2]) -> Point2 {
    match surface {
        SurfaceGeometry::Plane { .. } => Point2::new(uv[0] * 1000.0, uv[1] * 1000.0),
        SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. } => {
            Point2::new(uv[0], uv[1] * 1000.0)
        }
        SurfaceGeometry::Sphere { .. }
        | SurfaceGeometry::Torus { .. }
        | SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Unknown { .. } => Point2::new(uv[0], uv[1]),
        SurfaceGeometry::Transformed { basis, .. } => surface_parameters(basis, uv),
    }
}

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
            visible: None,
        });
    }

    let mut shells = BTreeMap::new();
    for node in graph.of_kind(13) {
        let Some(fields) = node.shell_fields() else {
            continue;
        };
        let Some(body) = bodies.get(&fields.body).cloned() else {
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
        let Some(fields) = node.vertex_fields() else {
            continue;
        };
        let Some((_, vertex)) = points.get(&fields.point) else {
            continue;
        };
        if let Some(decoded_vertex) = ir
            .model
            .vertices
            .iter_mut()
            .find(|candidate| candidate.id == *vertex)
        {
            decoded_vertex.tolerance = decoded_tolerance(fields.tolerance);
        }
        annotate_node(annotations, vertex, source_stream, node, "VERTEX");
        annotations.derived(vertex, "tolerance");
        vertices.insert(node.xmt, vertex.clone());
    }

    let mut edges = BTreeMap::new();
    for node in graph.of_kind(16) {
        let Some(fields) = node.edge_fields() else {
            continue;
        };
        let Some(fin) = graph.get(17, fields.fin) else {
            continue;
        };
        let Some(fin_fields) = fin.fin_fields() else {
            continue;
        };
        let Some(start) = vertices.get(&fin_fields.vertex).cloned() else {
            continue;
        };
        let end = graph
            .get(17, fin_fields.forward)
            .and_then(Node::fin_fields)
            .and_then(|next| vertices.get(&next.vertex))
            .cloned()
            .unwrap_or_else(|| start.clone());
        let curve_xmt = Some(fields.curve);
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
            tolerance: decoded_tolerance(fields.tolerance),
        });
        edges.insert(node.xmt, id);
    }

    let mut faces = BTreeMap::new();
    for node in graph.of_kind(14) {
        let Some(fields) = node.face_fields() else {
            continue;
        };
        let Some(shell) = shells.get(&fields.shell).cloned() else {
            continue;
        };
        let Some(surface) = surfaces.get(&fields.surface).cloned() else {
            continue;
        };
        let id = FaceId(format!("{prefix}:face#{}", node.xmt));
        annotate_node(annotations, &id, source_stream, node, "FACE");
        annotations.derived(&id, "tolerance");
        ir.model.faces.push(Face {
            id: id.clone(),
            shell: shell.clone(),
            surface,
            sense: sense(Some(fields.sense)),
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: decoded_tolerance(fields.tolerance),
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
        let Some(fields) = node.loop_fields() else {
            continue;
        };
        let Some(face) = faces.get(&fields.face).cloned() else {
            continue;
        };
        let id = LoopId(format!("{prefix}:loop#{}", node.xmt));
        annotate_node(annotations, &id, source_stream, node, "LOOP");
        ir.model.loops.push(Loop {
            id: id.clone(),
            face: face.clone(),
            boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
            coedges: Vec::new(),
            vertex_uses: Vec::new(),
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
            node.fin_fields()
                .map(|fields| fields.loop_xmt)
                .filter(|loop_xmt| loops.contains_key(loop_xmt))
                .map(|_| (node.xmt, CoedgeId(format!("{prefix}:fin#{}", node.xmt))))
        })
        .collect();
    for node in graph.of_kind(17) {
        let Some(fields) = node.fin_fields() else {
            continue;
        };
        let Some(loop_id) = loops.get(&fields.loop_xmt).cloned() else {
            continue;
        };
        let Some(edge) = edges.get(&fields.edge).cloned() else {
            continue;
        };
        let id = fin_ids.get(&node.xmt).cloned().expect("filtered above");
        annotate_node(annotations, &id, source_stream, node, "FIN");
        let next = fin_ids
            .get(&fields.forward)
            .cloned()
            .unwrap_or_else(|| id.clone());
        let previous = fin_ids
            .get(&fields.backward)
            .cloned()
            .unwrap_or_else(|| id.clone());
        let partner = fin_ids.get(&fields.other).cloned();
        let radial_next = partner.clone().unwrap_or_else(|| id.clone());
        ir.model.coedges.push(Coedge {
            id: id.clone(),
            owner_loop: loop_id.clone(),
            edge,
            next,
            previous,
            radial_next,
            sense: sense(Some(fields.sense)),
            pcurves: Vec::new(),
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
        SurfaceGeometry::Polygonal { .. } => "POLYGONAL_SURFACE",
        SurfaceGeometry::Transformed { basis, .. } => surface_tag(basis),
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
        CurveGeometry::Degenerate { .. } => "DEGENERATE_CURVE",
        CurveGeometry::Composite { .. } => "COMPOSITE_CURVE",
        CurveGeometry::Nurbs(_) => "B_SPLINE_CURVE",
        CurveGeometry::Polyline { .. } => "POLYLINE",
        CurveGeometry::Transformed { basis, .. } => curve_tag(basis),
        CurveGeometry::Unknown { .. } => "UNKNOWN_CURVE",
    }
}

fn decoded_tolerance(value: f64) -> Option<f64> {
    match value {
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
    for (index, stream) in scan
        .streams
        .iter()
        .filter(|stream| stream.kind == StreamKind::Deltas)
        .enumerate()
    {
        let census = crate::deltas::walk(&stream.inflated);
        attributes.insert(
            format!("deltas.{index}.grammar"),
            "status_byte_framed_topology".to_string(),
        );
        attributes.insert(
            format!("deltas.{index}.bytes_decoded"),
            census.bytes_decoded.to_string(),
        );
        for (name, count) in census.full_counts {
            attributes.insert(format!("deltas.{index}.full.{name}"), count.to_string());
        }
        for (name, count) in census.tombstone_counts {
            attributes.insert(
                format!("deltas.{index}.tombstone.{name}"),
                count.to_string(),
            );
        }
    }
    SourceMeta {
        format: "nx".to_string(),
        attributes,
    }
}

fn build_geometry_report(scan: &Scan, counts: &Counts, has_topology: bool) -> DecodeReport {
    let mut losses = Vec::new();

    losses.push(LossNote {
        code: LossCode::CarrierSummary,
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
            code: LossCode::TopologyNotTransferred,
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message: "The B-rep topology graph (body→shell→face→loop→fin→edge→vertex) was not \
                      reconstructed because the surviving typed records did not form a complete \
                      connected ownership graph. Exact-key supported partition↔deltas replacements \
                      and deletions were applied before graph construction. Required unresolved \
                      records prevent their dependent incidence from being emitted; decoded geometry \
                      then remains unattached."
                .to_string(),
            provenance: None,
        });
    }

    if counts.unresolved_intersections != 0 {
        losses.push(LossNote {
            code: LossCode::GeometryNotTransferred,
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{} surface-intersection record(s) lacked a complete validated CHART_s, \
                 term-endpoint, and support-UV witness and remain opaque constructions.",
                counts.unresolved_intersections
            ),
            provenance: None,
        });
    }

    if scan.count(StreamKind::Deltas) > 0 {
        losses.push(LossNote {
            code: LossCode::TopologyGaugeSubstituted,
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: format!(
                "{} Parasolid deltas stream(s) were paired by adjacency and equal schema. Exact-key \
                 BODY, SHELL, FACE, LOOP, FIN, EDGE, VERTEX, REGION, POINT, LINE, CIRCLE, ELLIPSE, PLANE, CYLINDER, CONE, SPHERE, TORUS, B_SURFACE, and B_CURVE full records and compact \
                 tombstones were applied in source order. Tombstones \
                 without an exact partition key remain unresolved.",
                scan.count(StreamKind::Deltas)
            ),
            provenance: None,
        });
    }

    if scan.count(StreamKind::Partition) > 1 {
        losses.push(LossNote {
            code: LossCode::TopologyGaugeSubstituted,
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
        code: LossCode::AttributesNotTransferred,
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
        coverage: std::collections::BTreeMap::new(),
        losses,
        notes: summary_notes(scan),
    }
}

fn build_metadata_ir(
    scan: &Scan,
) -> Result<(CadIr, cadmpeg_ir::Annotations, Vec<UnknownRecord>), CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    for (si, stream) in scan.streams.iter().enumerate() {
        if stream.kind.is_parasolid() {
            let unknown = unknown_stream(si, stream);
            let source_stream = annotations.stream("nx:container");
            annotations
                .note(&unknown.id, source_stream, stream.file_offset as u64)
                .tag(stream.kind.label());
            annotations.exactness(&unknown.id, Exactness::Derived);
            unknowns.push(unknown);
        }
    }
    attach_native_object_model(&mut ir, scan)
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    Ok((ir, annotations.build(), unknowns))
}

fn attach_native_object_model(
    ir: &mut CadIr,
    scan: &Scan,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let expressions = crate::native::expressions(&scan.container);
    let classes = crate::native::class_definitions(&scan.container);
    let configurations = crate::native::configurations(&scan.container);
    if expressions.is_empty() && classes.is_empty() && configurations.is_empty() {
        return Ok(());
    }
    let namespace = ir.native.namespace_mut("nx");
    namespace.version = namespace.version.max(1);
    if !expressions.is_empty() {
        namespace.set_arena("expressions", &expressions)?;
    }
    if !classes.is_empty() {
        namespace.set_arena("class_definitions", &classes)?;
    }
    if !configurations.is_empty() {
        namespace.set_arena("configurations", &configurations)?;
    }
    Ok(())
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
            code: LossCode::AssemblyComponentsExternal,
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
            code: LossCode::GeometryNotTransferred,
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
            code: LossCode::ContainerOnly,
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
        coverage: std::collections::BTreeMap::new(),
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
    let om_sections = c.indexed_om_sections();
    if !om_sections.is_empty() {
        let entities = om_sections
            .iter()
            .map(|(_, section)| section.records.len())
            .sum::<usize>();
        notes.push(format!(
            "NX object model: {} indexed section(s), {} bounded entity record(s)",
            om_sections.len(),
            entities
        ));
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
