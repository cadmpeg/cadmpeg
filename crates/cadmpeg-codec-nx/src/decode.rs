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
use cadmpeg_ir::eval::{
    analytic_surface_parameters, curve_point, model_surface_point_by_id, nurbs_surface_partials,
    pcurve_uv, surface_point,
};
use cadmpeg_ir::features::{
    BodyRetentionMode, BodySelection, BodyTrimSide, BooleanOp, ChamferSpec,
    CurveProjectionDirection, CurveProjectionDirectionState, EdgeSelection, ExtrudeExtent,
    FaceSelection, FeatureDefinition, HoleKind, Length, ParameterId, PathRef, PatternKind,
    ProfileRef, RadiusSpec, RibConstruction, RibDraft, SketchSpace, Termination, TrimRegion,
};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, BlendSupport, Curve, CurveGeometry, IntcurveSupportContext,
    IntcurveSupportSide, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceCurveFamily, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossNote};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::wire::hash::sha256_hex;
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::{self, Container};
use crate::geometry;
use crate::loss::NxLossCode;
use crate::native::vector::{cross_vector, dot_vector, unit_vector};
use crate::parasolid::{self, Stream, StreamKind};
use crate::topology::{Graph, Node};

pub(crate) const MISSING_TOLERANCE: f64 = -31_415_800_000_000.0;
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
    let container = container::scan(ctx, root)?;
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
        ..cadmpeg_ir::SourceFidelity::default()
    };
    source_fidelity.attach_native_unknown_records(&mut ir, "nx", unknowns)?;
    Ok(DecodeResult::with_source_fidelity(
        ir,
        report,
        source_fidelity,
    ))
}

fn report_untransferred_streams(scan: &Scan, report: &mut DecodeReport) {
    for (index, stream) in scan.streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            report
                .losses
                .push(NxLossCode::NonParasolidStreamOmitted.note(format!(
                    "Non-Parasolid {} stream #{index} was classified but not transferred.",
                    stream.kind.label()
                )));
        }
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
    intersection_rejections: crate::intersection::RejectionCounts,
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

pub(crate) fn ordered_point_candidates<'a>(
    stream: &[u8],
    graph: &'a Graph,
) -> Vec<(usize, Point3, Option<&'a Node>)> {
    ordered_fixed_candidates(
        geometry::points(stream)
            .into_iter()
            .map(|point| (point.pos, point.position)),
        graph,
        29..=29,
        Node::point_position,
    )
}

pub(crate) fn ordered_surface_candidates<'a>(
    stream: &[u8],
    graph: &'a Graph,
) -> Vec<(usize, SurfaceGeometry, Option<&'a Node>)> {
    ordered_fixed_candidates(
        geometry::surfaces(stream)
            .into_iter()
            .map(|surface| (surface.pos, surface.geometry)),
        graph,
        50..=54,
        Node::surface_geometry,
    )
}

pub(crate) fn ordered_curve_candidates<'a>(
    stream: &[u8],
    graph: &'a Graph,
) -> Vec<(usize, CurveGeometry, Option<&'a Node>)> {
    ordered_fixed_candidates(
        geometry::curves(stream)
            .into_iter()
            .map(|curve| (curve.pos, curve.geometry)),
        graph,
        30..=32,
        Node::curve_geometry,
    )
}

fn ordered_fixed_candidates<T>(
    fallback: impl IntoIterator<Item = (usize, T)>,
    graph: &Graph,
    kinds: std::ops::RangeInclusive<u8>,
    graph_value: impl Fn(&Node) -> Option<T>,
) -> Vec<(usize, T, Option<&Node>)> {
    let mut candidates = BTreeMap::new();
    for (offset, value) in fallback {
        let node = graph
            .at_pos(offset)
            .filter(|node| graph_value(node).is_some());
        candidates.insert(offset, (value, node));
    }
    for node in kinds.flat_map(|kind| graph.of_kind(kind)) {
        if let Some(value) = graph_value(node) {
            candidates.insert(node.pos, (value, Some(node)));
        }
    }
    candidates
        .into_iter()
        .map(|(offset, (value, node))| (offset, value, node))
        .collect()
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
    let parsed = crate::native::ParsedStreams::parse(scan);

    for (si, stream) in scan.streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        let view = parsed.stream(si).view_for_geometry();
        let semantic = parsed.semantic_bytes(si);
        let stream_name = format!("parasolid#{si}:{}", stream.kind.label());
        let source_stream = annotations.stream(format!("nx:{stream_name}"));
        let graph = &view.graph;
        body_node_ids.extend(topology_body_node_ids(si, graph));
        let mut points_by_xmt = BTreeMap::new();
        let mut surfaces_by_xmt = BTreeMap::new();
        let mut curves_by_xmt = BTreeMap::new();
        let mut pcurves_by_xmt = BTreeMap::new();
        let mut pcurve_supports_by_xmt = BTreeMap::new();
        let mut trim_ranges = BTreeMap::new();
        let mut pending_blend_supports = Vec::new();
        let mut pending_blend_spines = Vec::new();
        let mut pending_ext11_support_uv = Vec::new();
        let first_surface = ir.model.surfaces.len();
        let first_curve = ir.model.curves.len();
        for (pi, (position_offset, position, node)) in ordered_point_candidates(semantic, graph)
            .into_iter()
            .enumerate()
        {
            let pid = PointId(format!("nx:s{si}:pt#{pi}"));
            let vid = VertexId(format!("nx:s{si}:v#{pi}"));
            if let Some(node) = node {
                annotate_node(&mut annotations, &pid, source_stream, node, "POINT");
            } else {
                annotations
                    .note(&pid, source_stream, position_offset as u64)
                    .tag("POINT");
            }
            annotations.derived(&pid, "position");
            ir.model.points.push(Point {
                id: pid.clone(),
                position,
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: vid.clone(),
                point: pid.clone(),
                tolerance: None,
            });
            if let Some(node) = node {
                points_by_xmt.insert(node.xmt, pid);
            }
            counts.points += 1;
        }

        for (fi, (offset, geometry, node)) in ordered_surface_candidates(semantic, graph)
            .into_iter()
            .enumerate()
        {
            match &geometry {
                SurfaceGeometry::Plane { .. } => counts.planes += 1,
                SurfaceGeometry::Cylinder { .. } => counts.cylinders += 1,
                SurfaceGeometry::Cone { .. } => counts.cones += 1,
                SurfaceGeometry::Sphere { .. } => counts.spheres += 1,
                SurfaceGeometry::Torus { .. } => counts.tori += 1,
                SurfaceGeometry::Nurbs(_)
                | SurfaceGeometry::Procedural { .. }
                | SurfaceGeometry::Polygonal { .. }
                | SurfaceGeometry::Transformed { .. }
                | SurfaceGeometry::Unknown { .. } => {}
            }
            let id = SurfaceId(format!("nx:s{si}:surf#{fi}"));
            if let Some(node) = node {
                annotate_node(
                    &mut annotations,
                    &id,
                    source_stream,
                    node,
                    surface_tag(&geometry),
                );
            } else {
                annotations
                    .note(&id, source_stream, offset as u64)
                    .tag(surface_tag(&geometry));
            }
            annotations.derived(&id, "geometry");
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry,
                source_object: None,
            });
            if let Some(node) = node {
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

        for (oi, offset) in view.offset_surfaces.iter().copied().enumerate() {
            let Some(support) = surfaces_by_xmt.get(&offset.support).cloned() else {
                continue;
            };
            let surface_id = SurfaceId(format!("nx:s{si}:offset-surf#{oi}"));
            let procedural_id = ProceduralSurfaceId(format!("nx:s{si}:offset#{oi}"));
            annotations
                .note(&surface_id, source_stream, offset.pos as u64)
                .tag("OFFSET_SURF");
            annotations.derived(&surface_id, "geometry");
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: procedural_id.clone(),
                },
                source_object: Some(SourceObjectAssociation {
                    format: "nx".into(),
                    object_id: format!("nx:s{si}:offset-surface-record#{}", offset.xmt),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
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
                    revision_form: None,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            surfaces_by_xmt.insert(offset.xmt, surface_id);
            counts.offset_surfaces += 1;
        }

        for (bi, blend) in view.blend_surfaces.iter().copied().enumerate() {
            let surface_id = SurfaceId(format!("nx:s{si}:blend-surf#{bi}"));
            let procedural_id = ProceduralSurfaceId(format!("nx:s{si}:blend#{bi}"));
            annotations
                .note(&surface_id, source_stream, blend.pos as u64)
                .tag("BLEND_SURF");
            annotations.derived(&surface_id, "geometry");
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: procedural_id.clone(),
                },
                source_object: Some(SourceObjectAssociation {
                    format: "nx".to_string(),
                    object_id: format!("nx:s{si}:blend-surface-record#{}", blend.xmt),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            annotations
                .note(&procedural_id, source_stream, blend.pos as u64)
                .tag("BLEND_SURF");
            annotations.derived(&procedural_id, "definition");
            let procedural_index = ir.model.procedural_surfaces.len();
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: procedural_id,
                surface: surface_id.clone(),
                definition: ProceduralSurfaceDefinition::Blend {
                    supports: [None, None],
                    spine: None,
                    radius: BlendRadiusLaw::Constant {
                        signed_radius: blend.offsets[0],
                    },
                    cross_section: BlendCrossSection::Circular,
                    native: None,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            pending_blend_supports.push((procedural_index, blend.supports, blend.offsets));
            if blend.spine > 1 {
                pending_blend_spines.push((procedural_index, blend.spine));
            }
            surfaces_by_xmt.insert(blend.xmt, surface_id);
            counts.blend_surfaces += 1;
        }

        for (procedural_index, support_xmts, offsets) in pending_blend_supports {
            let supports = [0, 1].map(|side| {
                surfaces_by_xmt
                    .get(&support_xmts[side])
                    .cloned()
                    .map(|surface| BlendSupport {
                        surface,
                        reversed: offsets[side].is_sign_negative(),
                    })
            });
            let Some(ProceduralSurface {
                definition:
                    ProceduralSurfaceDefinition::Blend {
                        supports: slots, ..
                    },
                ..
            }) = ir.model.procedural_surfaces.get_mut(procedural_index)
            else {
                continue;
            };
            *slots = supports;
        }

        for (ci, (offset, geometry, node)) in ordered_curve_candidates(semantic, graph)
            .into_iter()
            .enumerate()
        {
            match &geometry {
                CurveGeometry::Line { .. } => counts.lines += 1,
                CurveGeometry::Circle { .. } => counts.circles += 1,
                CurveGeometry::Ellipse { .. } => counts.ellipses += 1,
                CurveGeometry::Parabola { .. }
                | CurveGeometry::Hyperbola { .. }
                | CurveGeometry::Degenerate { .. }
                | CurveGeometry::Composite { .. }
                | CurveGeometry::Nurbs(_)
                | CurveGeometry::Procedural { .. }
                | CurveGeometry::Polyline { .. }
                | CurveGeometry::Transformed { .. }
                | CurveGeometry::Unknown { .. } => {}
            }
            let id = CurveId(format!("nx:s{si}:crv#{ci}"));
            if let Some(node) = node {
                annotate_node(
                    &mut annotations,
                    &id,
                    source_stream,
                    node,
                    curve_tag(&geometry),
                );
            } else {
                annotations
                    .note(&id, source_stream, offset as u64)
                    .tag(curve_tag(&geometry));
            }
            annotations.derived(&id, "geometry");
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry,
                source_object: None,
            });
            if let Some(node) = node {
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

        for (pi, pcurve) in crate::nurbs::pcurves(semantic).into_iter().enumerate() {
            let id = PcurveId(format!("nx:s{si}:pcurve#{pi}"));
            annotations
                .note(&id, source_stream, pcurve.pos as u64)
                .tag("B_CURVE_2D");
            annotations.derived(&id, "geometry");
            ir.model.pcurves.push(Pcurve {
                id: id.clone(),
                geometry: pcurve.geometry,
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range: None,
                fit_tolerance: None,
            });
            if let Some(node) = graph.at_pos(pcurve.pos) {
                pcurves_by_xmt.insert(node.xmt, id);
            }
        }

        let intersection_scan = view.intersections.clone();
        counts
            .intersection_rejections
            .extend(intersection_scan.rejected);
        let intersection_constructions = intersection_scan.constructions;
        let charted_intersections: BTreeMap<_, _> = intersection_scan
            .curves
            .into_iter()
            .map(|curve| (curve.xmt, curve))
            .collect();
        for (ci, construction) in intersection_constructions.into_iter().enumerate() {
            let curve_id = CurveId(format!("nx:s{si}:intersection-crv#{ci}"));
            let procedural_id = ProceduralCurveId(format!("nx:s{si}:intersection#{ci}"));
            let unknown_id = UnknownId(format!("nx:container:parasolid#{si}"));
            let charted = charted_intersections.get(&construction.xmt);
            if let Some(charted) = charted {
                pending_ext11_support_uv.push((
                    procedural_id.clone(),
                    charted.points.clone(),
                    charted.parameters.clone(),
                    charted.fit_tolerance,
                    charted.ext_support_uv.clone(),
                ));
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
                source_object: Some(SourceObjectAssociation {
                    format: "nx".into(),
                    object_id: format!("nx:s{si}:intersection-record#{}", construction.xmt),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
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
                        native_kind: Some("nx:intersection".into()),
                        record: Some(unknown_id),
                    },
                    |charted| {
                        let mut support_uv = charted.support_uv.clone();
                        if let Some(ext_support_uv) = assign_ext11_support_uv(
                            &ir,
                            &surfaces_by_xmt,
                            charted.supports,
                            &charted.points,
                            charted.fit_tolerance,
                            &charted.ext_support_uv,
                        ) {
                            for side in 0..2 {
                                if support_uv[side].is_none() {
                                    support_uv[side].clone_from(&ext_support_uv[side]);
                                }
                            }
                        }
                        let first = intersection_side(
                            &ir,
                            &surfaces_by_xmt,
                            charted.supports[0],
                            support_uv[0]
                                .as_deref()
                                .filter(|uv| uv.len() == charted.parameters.len())
                                .map(|uv| (uv, charted.parameters.as_slice())),
                        );
                        let second = intersection_side(
                            &ir,
                            &surfaces_by_xmt,
                            charted.supports[1],
                            support_uv[1]
                                .as_deref()
                                .filter(|uv| uv.len() == charted.parameters.len())
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

        for (procedural_index, spine_xmt) in pending_blend_spines {
            let Some(spine) = curves_by_xmt.get(&spine_xmt).cloned() else {
                continue;
            };
            let Some(ProceduralSurface {
                definition: ProceduralSurfaceDefinition::Blend { spine: slot, .. },
                ..
            }) = ir.model.procedural_surfaces.get_mut(procedural_index)
            else {
                continue;
            };
            *slot = Some(spine);
        }

        let trimmed_curves = &view.trimmed_curves;
        let mut normalized_pcurves = BTreeSet::new();
        let surface_curves = &view.surface_curves;
        loop {
            let mapped = curves_by_xmt.len() + pcurves_by_xmt.len() + pcurve_supports_by_xmt.len();
            for trim in trimmed_curves {
                if let Some(basis) = curves_by_xmt.get(&trim.basis).cloned() {
                    let parameters = canonical_trim_range(&ir, &basis, trim.parameters);
                    curves_by_xmt.insert(trim.xmt, basis);
                    if let Some(parameters) = parameters {
                        trim_ranges.insert(trim.xmt, parameters);
                    }
                }
                if let Some(pcurve) = pcurves_by_xmt.get(&trim.basis).cloned() {
                    if let Some(carrier) = ir.model.pcurves.iter_mut().find(|p| p.id == pcurve) {
                        carrier.parameter_range = Some(trim.parameters);
                    }
                    pcurves_by_xmt.insert(trim.xmt, pcurve);
                    if let Some(support) = pcurve_supports_by_xmt.get(&trim.basis).cloned() {
                        pcurve_supports_by_xmt.insert(trim.xmt, support);
                    }
                    trim_ranges.insert(trim.xmt, trim.parameters);
                }
            }
            for surface_curve in surface_curves {
                if let Some(pcurve) = pcurves_by_xmt.get(&surface_curve.pcurve).cloned() {
                    if !normalized_pcurves.contains(&pcurve) {
                        let support = surfaces_by_xmt
                            .get(&surface_curve.surface)
                            .and_then(|id| {
                                ir.model.surfaces.iter().find(|surface| surface.id == *id)
                            })
                            .map(|surface| surface.geometry.clone());
                        let normalized = if let (Some(support), Some(carrier)) = (
                            support,
                            ir.model
                                .pcurves
                                .iter_mut()
                                .find(|candidate| candidate.id == pcurve),
                        ) {
                            normalize_pcurve_parameters(&mut carrier.geometry, &support).is_some()
                        } else {
                            false
                        };
                        if !normalized {
                            pcurves_by_xmt.remove(&surface_curve.pcurve);
                            ir.model.pcurves.retain(|candidate| candidate.id != pcurve);
                            continue;
                        }
                        normalized_pcurves.insert(pcurve.clone());
                    }
                    if let Some(carrier) = ir.model.pcurves.iter_mut().find(|p| p.id == pcurve) {
                        carrier.fit_tolerance = decoded_tolerance(surface_curve.tolerance);
                    }
                    pcurves_by_xmt.insert(surface_curve.xmt, pcurve);
                    if let Some(support) = surfaces_by_xmt.get(&surface_curve.surface).cloned() {
                        pcurve_supports_by_xmt.insert(surface_curve.xmt, support);
                    }
                }
                if let Some(original) = curves_by_xmt.get(&surface_curve.original).cloned() {
                    curves_by_xmt.insert(surface_curve.xmt, original);
                }
            }
            if curves_by_xmt.len() + pcurves_by_xmt.len() + pcurve_supports_by_xmt.len() == mapped {
                break;
            }
        }

        retain_unresolved_topology_carriers(
            &mut ir,
            si,
            graph,
            &mut surfaces_by_xmt,
            &mut curves_by_xmt,
            &pcurves_by_xmt,
            source_stream,
            &mut annotations,
        );

        emit_topology(
            &mut ir,
            si,
            graph,
            &points_by_xmt,
            &surfaces_by_xmt,
            &curves_by_xmt,
            &pcurves_by_xmt,
            &pcurve_supports_by_xmt,
            &trim_ranges,
            source_stream,
            &mut annotations,
        );
        complete_ext11_support_uv(&mut ir, &pending_ext11_support_uv);
        complete_parameterization_equivalent_support_uv(&mut ir);
        complete_support_uv(&mut ir, &pending_ext11_support_uv);
        attach_completed_intersection_pcurves(
            &mut ir,
            graph,
            &format!("nx:s{si}"),
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
        unknowns.push(unknown);
    }

    if counts.points == 0 && counts.surfaces() == 0 && counts.curves() == 0 {
        return None;
    }

    let rmfastload_ids = scan
        .container
        .rmfastload_object_id_table()
        .map(|(_, table)| {
            table
                .object_ids
                .into_iter()
                .map(|object_id| object_id.value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // Extract the native model once, before body selection: terminal-feature
    // body selection and annotation attachment both read it, and extraction is
    // pure, so building it here avoids re-parsing the same container/stream
    // bytes for the seven feature/segment families body selection consumes.
    // This moves extraction slightly earlier on the geometry path — the RFC's
    // accepted memory-high-water cost.
    let model = crate::native::NativeModel::extract(&scan.container, &scan.streams, &parsed);
    let mut active_body_selection = select_active_body(&mut ir, &body_node_ids, &rmfastload_ids);
    if !active_body_selection {
        active_body_selection = select_terminal_feature_bodies(&mut ir, &model);
    }
    classify_body_kinds(&mut ir);
    crate::native::attach_annotations(&mut ir, &model, scan, &mut annotations, &mut unknowns)
        .ok()?;
    prune_unreferenced_unknown_carriers(&mut ir);
    finalize_point_topology(&mut ir, &mut annotations);
    let referenced_pcurves: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|pcurve| pcurve.pcurve.clone()))
        .collect();
    ir.model
        .pcurves
        .retain(|pcurve| referenced_pcurves.contains(&pcurve.id));
    retain_live_unknown_links(&ir, &mut unknowns, &mut annotations);
    let mut annotations = annotations.build();
    retain_live_annotations(&ir, &unknowns, &mut annotations);
    let mut report = build_geometry_report(
        scan,
        &ir,
        &counts,
        !ir.model.faces.is_empty(),
        ir.model.bodies.len() > 1 && !active_body_selection,
        ir.model.tessellations.len(),
    );
    report_untransferred_streams(scan, &mut report);
    Some((ir, report, annotations, unknowns))
}

pub(crate) fn prune_unreferenced_unknown_carriers(ir: &mut CadIr) {
    let mut used_surfaces: BTreeSet<_> = ir
        .model
        .faces
        .iter()
        .map(|face| face.surface.clone())
        .collect();
    let mut used_curves: BTreeSet<_> = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| edge.curve.clone())
        .collect();
    loop {
        let previous = (used_surfaces.len(), used_curves.len());
        for procedural in &ir.model.procedural_surfaces {
            if !used_surfaces.contains(&procedural.surface) {
                continue;
            }
            match &procedural.definition {
                ProceduralSurfaceDefinition::Offset { support, .. } => {
                    used_surfaces.insert(support.clone());
                }
                ProceduralSurfaceDefinition::Blend {
                    supports, spine, ..
                } => {
                    used_surfaces.extend(
                        supports
                            .iter()
                            .flatten()
                            .map(|support| support.surface.clone()),
                    );
                    used_curves.extend(spine.iter().cloned());
                }
                _ => {}
            }
        }
        for procedural in &ir.model.procedural_curves {
            if !used_curves.contains(&procedural.curve) {
                continue;
            }
            match &procedural.definition {
                ProceduralCurveDefinition::Intersection { context, .. }
                | ProceduralCurveDefinition::SurfaceCurve { context, .. } => {
                    used_surfaces
                        .extend(context.sides.iter().filter_map(|side| side.surface.clone()));
                }
                _ => {}
            }
        }
        if previous == (used_surfaces.len(), used_curves.len()) {
            break;
        }
    }
    ir.model.surfaces.retain(|surface| {
        !matches!(surface.geometry, SurfaceGeometry::Unknown { .. })
            || used_surfaces.contains(&surface.id)
    });
    ir.model.curves.retain(|curve| {
        !matches!(curve.geometry, CurveGeometry::Unknown { .. }) || used_curves.contains(&curve.id)
    });
}

fn unmatched_delta_tombstone_count(scan: &Scan) -> usize {
    let pairs = crate::native::paired_delta_streams(scan);
    let mut current = pairs
        .keys()
        .map(|partition| (*partition, scan.streams[*partition].inflated.clone()))
        .collect::<BTreeMap<_, _>>();
    let paired_deltas = pairs.values().flatten().copied().collect::<BTreeSet<_>>();
    let mut unmatched = 0usize;
    for (delta, stream) in scan.streams.iter().enumerate() {
        if stream.kind == StreamKind::Deltas && !paired_deltas.contains(&delta) {
            unmatched += crate::deltas::unmatched_terminal_tombstones(&[], &stream.inflated);
        }
    }
    for (partition, deltas) in pairs {
        for delta in deltas {
            let delta_bytes = &scan.streams[delta].inflated;
            let partition_bytes = current
                .get_mut(&partition)
                .expect("paired partition was initialized");
            unmatched += crate::deltas::unmatched_terminal_tombstones(partition_bytes, delta_bytes);
            *partition_bytes = crate::deltas::merge_full_records(partition_bytes, delta_bytes);
        }
    }
    unmatched
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
        ir.model.features,
    );
    ids.extend(unknowns.iter().map(|unknown| unknown.id.to_string()));
    annotations.provenance.retain(|id, _| ids.contains(id));
    annotations.exactness.retain(|id, _| ids.contains(id));
}

fn retain_live_unknown_links(
    ir: &CadIr,
    unknowns: &mut [UnknownRecord],
    annotations: &mut AnnotationBuilder,
) {
    let mut ids = BTreeSet::new();
    ids.extend(ir.model.surfaces.iter().map(|entity| entity.id.to_string()));
    ids.extend(ir.model.curves.iter().map(|entity| entity.id.to_string()));
    ids.extend(ir.model.pcurves.iter().map(|entity| entity.id.to_string()));
    ids.extend(
        ir.model
            .procedural_surfaces
            .iter()
            .map(|entity| entity.id.to_string()),
    );
    ids.extend(
        ir.model
            .procedural_curves
            .iter()
            .map(|entity| entity.id.to_string()),
    );
    let mut empty_links = Vec::new();
    for unknown in unknowns.iter_mut() {
        unknown.links.retain(|link| ids.contains(link));
        if unknown.links.is_empty() {
            empty_links.push(unknown.id.to_string());
        }
    }
    let _ = (empty_links, annotations);
}

fn topology_body_node_ids(stream_index: usize, graph: &Graph) -> BTreeMap<BodyId, BTreeSet<u32>> {
    let prefix = format!("nx:s{stream_index}");
    let body_xmts: BTreeSet<_> = graph
        .body_shape_shells()
        .into_iter()
        .filter_map(|shell| shell.shell_fields().map(|fields| fields.body))
        .collect();
    body_xmts
        .into_iter()
        .map(|body_xmt| {
            let shells: BTreeSet<_> = graph
                .of_kind(13)
                .filter(|shell| {
                    shell
                        .shell_fields()
                        .is_some_and(|fields| fields.body == body_xmt)
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
            (BodyId(format!("{prefix}:body#{body_xmt}")), ids)
        })
        .collect()
}

fn select_active_body(
    ir: &mut CadIr,
    body_node_ids: &BTreeMap<BodyId, BTreeSet<u32>>,
    rmfastload_ids: &[u32],
) -> bool {
    if rmfastload_ids.is_empty() || ir.model.bodies.len() <= 1 {
        return false;
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
    let Some(&(top_hits, top_count, ref top_body)) = scored.first() else {
        return false;
    };
    let next_hits = scored.get(1).map_or(0, |score| score.0);
    let mut selected: BTreeSet<_> = scored
        .iter()
        .filter(|(hits, count, _)| *hits > 0 && *count > 0 && (*hits as f64 / *count as f64) > 0.10)
        .map(|(_, _, body)| body.clone())
        .collect();
    let dominant = top_hits >= 5 * next_hits.max(1);
    if dominant {
        selected.retain(|body| body == top_body);
    }
    if top_count == 0
        || (top_hits as f64 / top_count as f64) <= 0.10
        || selected.is_empty()
        || (selected.len() == 1 && !dominant)
    {
        return false;
    }
    prune_inactive_topology(ir, &selected);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "active_body_selector".to_string(),
            "rmfastload_object_id_membership".to_string(),
        );
        source
            .attributes
            .insert("rmfastload_hits".to_string(), top_hits.to_string());
        source.attributes.insert(
            "rmfastload_active_body_count".to_string(),
            selected.len().to_string(),
        );
    }
    true
}

fn select_terminal_feature_bodies(ir: &mut CadIr, model: &crate::native::NativeModel) -> bool {
    if ir.model.bodies.len() <= 1 {
        return false;
    }
    // These families are read straight from the pre-built model; extracting
    // them here as well would parse the same container bytes a second time.
    // `feature_operation_body_operands` already folds in the body-member and
    // reference-occurrence families the legacy code computed inline.
    let labels = model.features.feature_operation_labels.as_slice();
    let body_references = model.features.feature_body_references.as_slice();
    let booleans = model.features.feature_boolean_operations.as_slice();
    let bindings = model.segments.segment_body_bindings.as_slice();
    let body_operands = model.features.feature_operation_body_operands.as_slice();
    if booleans.is_empty() && body_operands.is_empty() {
        return false;
    }
    let Some(statuses) = crate::native::segment_body_lineage_statuses(
        labels,
        body_references,
        booleans,
        body_operands,
        bindings,
    ) else {
        return false;
    };
    let mut mapped = BTreeSet::new();
    let mut selected = BTreeSet::new();
    for (binding, status) in bindings.iter().filter_map(|binding| {
        statuses
            .iter()
            .find(|status| status.segment_body_binding == binding.id)
            .map(|status| (binding, status))
    }) {
        let prefix = format!("nx:s{}:", binding.stream_ordinal);
        let stream_bodies = ir
            .model
            .bodies
            .iter()
            .filter(|body| body.id.0.starts_with(&prefix))
            .map(|body| body.id.clone())
            .collect::<Vec<_>>();
        if stream_bodies.is_empty() {
            continue;
        }
        mapped.extend(stream_bodies.iter().cloned());
        if status.terminal {
            selected.extend(stream_bodies);
        }
    }
    let emitted = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<BTreeSet<_>>();
    if mapped != emitted || selected.is_empty() || selected.len() == emitted.len() {
        return false;
    }

    prune_inactive_topology(ir, &selected);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "active_body_selector".to_string(),
            "terminal_feature_body_lineage".to_string(),
        );
        source.attributes.insert(
            "feature_terminal_body_count".to_string(),
            selected.len().to_string(),
        );
    }
    true
}

fn prune_inactive_topology(ir: &mut CadIr, selected: &BTreeSet<BodyId>) {
    ir.model.bodies.retain(|body| selected.contains(&body.id));
    ir.model
        .regions
        .retain(|region| selected.contains(&region.body));
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
    prune_inactive_geometry(ir);
}

fn prune_inactive_geometry(ir: &mut CadIr) {
    let mut surfaces: BTreeSet<_> = ir
        .model
        .faces
        .iter()
        .map(|face| face.surface.clone())
        .collect();
    let mut curves: BTreeSet<_> = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| edge.curve.clone())
        .collect();
    let pcurves: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|pcurve| pcurve.pcurve.clone()))
        .collect();

    loop {
        let old_surface_count = surfaces.len();
        let old_curve_count = curves.len();
        for procedural in &ir.model.procedural_surfaces {
            if !surfaces.contains(&procedural.surface) {
                continue;
            }
            match &procedural.definition {
                ProceduralSurfaceDefinition::Offset { support, .. } => {
                    surfaces.insert(support.clone());
                }
                ProceduralSurfaceDefinition::Blend {
                    supports, spine, ..
                } => {
                    surfaces.extend(
                        supports
                            .iter()
                            .flatten()
                            .map(|support| support.surface.clone()),
                    );
                    curves.extend(spine.iter().cloned());
                }
                _ => {}
            }
        }
        for procedural in &ir.model.procedural_curves {
            if !curves.contains(&procedural.curve) {
                continue;
            }
            match &procedural.definition {
                ProceduralCurveDefinition::Intersection { context, .. }
                | ProceduralCurveDefinition::SurfaceCurve { context, .. } => {
                    surfaces.extend(context.sides.iter().filter_map(|side| side.surface.clone()));
                }
                _ => {}
            }
        }
        if surfaces.len() == old_surface_count && curves.len() == old_curve_count {
            break;
        }
    }

    ir.model
        .procedural_surfaces
        .retain(|procedural| surfaces.contains(&procedural.surface));
    ir.model
        .procedural_curves
        .retain(|procedural| curves.contains(&procedural.curve));
    ir.model
        .surfaces
        .retain(|surface| surfaces.contains(&surface.id));
    ir.model.curves.retain(|curve| curves.contains(&curve.id));
    ir.model
        .pcurves
        .retain(|pcurve| pcurves.contains(&pcurve.id));
}

fn finalize_point_topology(ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    let referenced_points: BTreeSet<_> = ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect();
    if !ir.model.bodies.is_empty() {
        ir.model
            .points
            .retain(|point| referenced_points.contains(&point.id));
        return;
    }

    if ir.model.points.is_empty() {
        return;
    }

    let body_id = BodyId("nx:derived:point-body#0".to_string());
    let region_id = RegionId("nx:derived:point-region#0".to_string());
    let shell_id = ShellId("nx:derived:point-shell#0".to_string());
    let stream = annotations.stream("nx:container");
    for id in [&body_id.0, &region_id.0, &shell_id.0] {
        annotations
            .note(id, stream, 0)
            .tag("derived_point_topology");
        annotations.exactness(id, Exactness::Inferred);
    }

    let mut free_vertices = Vec::with_capacity(ir.model.points.len());
    for (index, point) in ir.model.points.iter().enumerate() {
        let vertex_id = VertexId(format!("nx:derived:point-vertex#{index}"));
        annotations
            .note(&vertex_id, stream, 0)
            .tag("derived_point_topology");
        annotations.exactness(&vertex_id, Exactness::Inferred);
        ir.model.vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point.id.clone(),
            tolerance: None,
        });
        free_vertices.push(vertex_id);
    }
    ir.model.shells.push(Shell {
        id: shell_id.clone(),
        region: region_id.clone(),
        faces: Vec::new(),
        wire_edges: Vec::new(),
        free_vertices,
    });
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id.clone(),
        shells: vec![shell_id],
    });
    ir.model.bodies.push(Body {
        id: body_id,
        kind: BodyKind::General,
        regions: vec![region_id],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
}

fn classify_body_kinds(ir: &mut CadIr) {
    let region_bodies: BTreeMap<_, _> = ir
        .model
        .regions
        .iter()
        .map(|region| (region.id.clone(), region.body.clone()))
        .collect();
    let shell_bodies: BTreeMap<_, _> = ir
        .model
        .shells
        .iter()
        .filter_map(|shell| {
            region_bodies
                .get(&shell.region)
                .cloned()
                .map(|body| (shell.id.clone(), body))
        })
        .collect();
    let face_bodies: BTreeMap<_, _> = ir
        .model
        .faces
        .iter()
        .filter_map(|face| {
            shell_bodies
                .get(&face.shell)
                .cloned()
                .map(|body| (face.id.clone(), body))
        })
        .collect();
    let loop_bodies: BTreeMap<_, _> = ir
        .model
        .loops
        .iter()
        .filter_map(|loop_| {
            face_bodies
                .get(&loop_.face)
                .cloned()
                .map(|body| (loop_.id.clone(), body))
        })
        .collect();
    let coedge_bodies: BTreeMap<_, _> = ir
        .model
        .coedges
        .iter()
        .filter_map(|coedge| {
            loop_bodies
                .get(&coedge.owner_loop)
                .cloned()
                .map(|body| (coedge.id.clone(), body))
        })
        .collect();
    let mut edge_uses = BTreeMap::<BodyId, BTreeMap<EdgeId, usize>>::new();
    for coedge in &ir.model.coedges {
        let Some(body) = coedge_bodies.get(&coedge.id) else {
            continue;
        };
        *edge_uses
            .entry(body.clone())
            .or_default()
            .entry(coedge.edge.clone())
            .or_default() += 1;
    }
    for body in &mut ir.model.bodies {
        body.kind = if edge_uses
            .get(&body.id)
            .is_some_and(|uses| !uses.is_empty() && uses.values().all(|use_count| *use_count == 2))
        {
            BodyKind::Solid
        } else {
            BodyKind::Sheet
        };
    }
}

fn linear_knots(parameters: &[f64]) -> Vec<f64> {
    let mut knots = Vec::with_capacity(parameters.len() + 2);
    knots.push(parameters[0]);
    knots.extend_from_slice(parameters);
    knots.push(*parameters.last().expect("non-empty chart parameters"));
    knots
}

pub(crate) fn assign_ext11_support_uv(
    ir: &CadIr,
    surfaces_by_xmt: &BTreeMap<u32, SurfaceId>,
    supports: [u32; 2],
    points: &[Point3],
    fit_tolerance: f64,
    lanes: &[Option<Vec<[f64; 2]>>; 2],
) -> Option<[Option<Vec<[f64; 2]>>; 2]> {
    let surface_ids = supports.map(|support| surfaces_by_xmt.get(&support).cloned());
    let [Some(first_surface), Some(second_surface)] = surface_ids else {
        return None;
    };
    assign_ext11_support_uv_to_surfaces(
        ir,
        [&first_surface, &second_surface],
        points,
        fit_tolerance,
        lanes,
    )
}

pub(crate) fn assign_ext11_support_uv_to_surfaces(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    points: &[Point3],
    fit_tolerance: f64,
    lanes: &[Option<Vec<[f64; 2]>>; 2],
) -> Option<[Option<Vec<[f64; 2]>>; 2]> {
    let lane_matches_surface = |surface: &SurfaceId, lane: usize| {
        let Some(values) = lanes[lane]
            .as_deref()
            .filter(|values| values.len() == points.len())
        else {
            return false;
        };
        let Some(geometry) = ir
            .model
            .surfaces
            .iter()
            .find(|candidate| &candidate.id == surface)
            .map(|surface| &surface.geometry)
        else {
            return false;
        };
        values.iter().zip(points).all(|(uv, point)| {
            let Some(uv) = surface_parameters(geometry, *uv) else {
                return false;
            };
            model_surface_point_by_id(ir, surface, uv.u, uv.v)
                .is_some_and(|candidate| point_distance(candidate, *point) <= fit_tolerance)
        })
    };
    let matches = [
        [
            lane_matches_surface(surfaces[0], 0),
            lane_matches_surface(surfaces[0], 1),
        ],
        [
            lane_matches_surface(surfaces[1], 0),
            lane_matches_surface(surfaces[1], 1),
        ],
    ];
    let mut assigned = [None, None];
    let mut assigned_lanes = [None, None];
    for lane in 0..2 {
        let support_matches = [matches[0][lane], matches[1][lane]];
        let Some(support) = support_matches
            .iter()
            .position(|matches| *matches)
            .filter(|_| support_matches.iter().filter(|matches| **matches).count() == 1)
        else {
            continue;
        };
        if assigned[support].is_some() {
            return None;
        }
        assigned[support].clone_from(&lanes[lane]);
        assigned_lanes[support] = Some(lane);
    }
    if surfaces[0] != surfaces[1] && assigned.iter().filter(|lane| lane.is_some()).count() == 1 {
        let assigned_support = assigned.iter().position(Option::is_some)?;
        let assigned_lane = assigned_lanes[assigned_support]?;
        let other_support = 1 - assigned_support;
        let other_lane = 1 - assigned_lane;
        if lane_matches_surface(surfaces[other_support], other_lane) {
            assigned[other_support].clone_from(&lanes[other_lane]);
        }
    }
    assigned.iter().any(Option::is_some).then_some(assigned)
}

pub(crate) type PendingExt11SupportUv = (
    ProceduralCurveId,
    Vec<Point3>,
    Vec<f64>,
    f64,
    [Option<Vec<[f64; 2]>>; 2],
);

fn missing_support_parameter(value: f64) -> bool {
    value.to_bits() == MISSING_TOLERANCE.to_bits()
}

fn pcurve_requires_completion(pcurve: Option<&PcurveGeometry>) -> bool {
    match pcurve {
        None => true,
        Some(PcurveGeometry::Nurbs { control_points, .. }) => control_points.iter().any(|point| {
            !point.u.is_finite()
                || !point.v.is_finite()
                || missing_support_parameter(point.u)
                || missing_support_parameter(point.v)
        }),
        Some(PcurveGeometry::Line { origin, direction }) => [origin, direction]
            .into_iter()
            .any(|point| !point.u.is_finite() || !point.v.is_finite()),
        Some(_) => false,
    }
}

fn pcurve_control_point_seed(pcurve: Option<&PcurveGeometry>, index: usize) -> Option<Point2> {
    let PcurveGeometry::Nurbs { control_points, .. } = pcurve? else {
        return None;
    };
    control_points.get(index).copied().filter(|point| {
        point.u.is_finite()
            && point.v.is_finite()
            && !missing_support_parameter(point.u)
            && !missing_support_parameter(point.v)
    })
}

pub(crate) fn complete_ext11_support_uv(ir: &mut CadIr, pending: &[PendingExt11SupportUv]) {
    for (procedural_id, points, parameters, fit_tolerance, lanes) in pending {
        let Some(procedural_index) = ir
            .model
            .procedural_curves
            .iter()
            .position(|procedural| &procedural.id == procedural_id)
        else {
            continue;
        };
        let (surfaces, missing) = match &ir.model.procedural_curves[procedural_index].definition {
            ProceduralCurveDefinition::Intersection { context, .. } => {
                let [Some(first), Some(second)] = &context.sides.clone().map(|side| side.surface)
                else {
                    continue;
                };
                (
                    [first.clone(), second.clone()],
                    context
                        .sides
                        .each_ref()
                        .map(|side| pcurve_requires_completion(side.pcurve.as_ref())),
                )
            }
            _ => continue,
        };
        if !missing.into_iter().any(|missing| missing) {
            continue;
        }
        let Some(assigned) = assign_ext11_support_uv_to_surfaces(
            ir,
            [&surfaces[0], &surfaces[1]],
            points,
            *fit_tolerance,
            lanes,
        ) else {
            continue;
        };
        let replacements: [Option<PcurveGeometry>; 2] = std::array::from_fn(|side| {
            if !missing[side] {
                return None;
            }
            let surface_geometry = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == surfaces[side])
                .map(|surface| &surface.geometry)?;
            let values = assigned[side].as_ref()?;
            if values
                .iter()
                .flatten()
                .any(|value| !value.is_finite() || missing_support_parameter(*value))
            {
                return None;
            }
            let control_points = values
                .iter()
                .map(|uv| surface_parameters(surface_geometry, *uv))
                .collect::<Option<Vec<_>>>()?;
            Some(PcurveGeometry::Nurbs {
                degree: 1,
                knots: linear_knots(parameters),
                control_points,
                weights: None,
                periodic: false,
            })
        });
        let ProceduralCurveDefinition::Intersection { context, .. } =
            &mut ir.model.procedural_curves[procedural_index].definition
        else {
            unreachable!("definition checked above");
        };
        for (side, replacement) in replacements.into_iter().enumerate() {
            if let Some(replacement) = replacement {
                context.sides[side].pcurve = Some(replacement);
            }
        }
    }
}

pub(crate) fn complete_support_uv(ir: &mut CadIr, pending: &[PendingExt11SupportUv]) {
    loop {
        let before = pending_support_lanes_requiring_completion(ir, pending);
        complete_support_uv_wave(ir, pending);
        let after = pending_support_lanes_requiring_completion(ir, pending);
        if after >= before {
            break;
        }
    }
}

fn pending_support_lanes_requiring_completion(
    ir: &CadIr,
    pending: &[PendingExt11SupportUv],
) -> usize {
    pending
        .iter()
        .filter_map(|(procedural_id, ..)| {
            ir.model
                .procedural_curves
                .iter()
                .find(|procedural| &procedural.id == procedural_id)
        })
        .filter_map(|procedural| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            Some(
                context
                    .sides
                    .iter()
                    .filter(|side| pcurve_requires_completion(side.pcurve.as_ref()))
                    .count(),
            )
        })
        .sum()
}

fn complete_support_uv_wave(ir: &mut CadIr, pending: &[PendingExt11SupportUv]) {
    let mut replacements = Vec::new();
    let mut blend_parameter_grids = BTreeMap::<SurfaceId, Option<Vec<(Point2, Point3)>>>::new();
    for (procedural_id, points, parameters, fit_tolerance, _) in pending {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter()
            .find(|procedural| &procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            continue;
        };
        for side in 0..2 {
            if !pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
                continue;
            }
            let Some(surface_id) = &context.sides[side].surface else {
                continue;
            };
            let Some(surface) = ir
                .model
                .surfaces
                .iter()
                .find(|surface| &surface.id == surface_id)
            else {
                continue;
            };
            let effective_fit_tolerance =
                blend_spine_cache_fit_tolerance(ir, surface_id, *fit_tolerance);
            let mut uv = Vec::with_capacity(points.len());
            for (point_index, point) in points.iter().enumerate() {
                let seed =
                    pcurve_control_point_seed(context.sides[side].pcurve.as_ref(), point_index)
                        .or_else(|| uv.last().copied());
                let parameters = match &surface.geometry {
                    SurfaceGeometry::Nurbs(nurbs) => nurbs_parameters(nurbs, *point, seed),
                    SurfaceGeometry::Procedural { .. } => {
                        let other_side = &context.sides[1 - side];
                        other_side
                            .surface
                            .as_ref()
                            .zip(other_side.pcurve.as_ref())
                            .and_then(|(other_surface, other_pcurve)| {
                                blend_boundary_parameter_from_support_pcurve(
                                    ir,
                                    surface_id,
                                    other_surface,
                                    other_pcurve,
                                    parameters[point_index],
                                    *point,
                                    effective_fit_tolerance,
                                )
                            })
                            .or_else(|| {
                                offset_surface_parameters_with_tolerance(
                                    ir,
                                    surface_id,
                                    *point,
                                    seed,
                                    Some(effective_fit_tolerance),
                                )
                            })
                            .or_else(|| {
                                blend_surface_parameters_for_fit_with_grid(
                                    ir,
                                    surface_id,
                                    *point,
                                    seed,
                                    effective_fit_tolerance,
                                    BlendParameterGrid::Disabled,
                                )
                            })
                            .or_else(|| {
                                let blend_grid = blend_parameter_grids
                                    .entry(surface_id.clone())
                                    .or_insert_with(|| {
                                        blend_surface_parameter_grid(ir, surface_id, 0)
                                    });
                                blend_surface_parameters_from_grid_for_fit(
                                    ir,
                                    surface_id,
                                    *point,
                                    effective_fit_tolerance,
                                    blend_grid.as_deref()?,
                                )
                            })
                    }
                    geometry => analytic_surface_parameters(geometry, *point),
                };
                let Some(parameters) = parameters else {
                    uv.clear();
                    break;
                };
                uv.push(parameters);
            }
            if uv.len() != points.len() {
                continue;
            }
            if matches!(
                surface.geometry,
                SurfaceGeometry::Cylinder { .. }
                    | SurfaceGeometry::Cone { .. }
                    | SurfaceGeometry::Sphere { .. }
                    | SurfaceGeometry::Torus { .. }
            ) {
                for index in 1..uv.len() {
                    let turns = ((uv[index - 1].u - uv[index].u) / std::f64::consts::TAU).round();
                    uv[index].u += turns * std::f64::consts::TAU;
                }
            }
            let reproduces_chart = uv.iter().zip(points).all(|(uv, point)| {
                decoded_surface_point(ir, surface_id, uv.u, uv.v)
                    .is_some_and(|actual| point_distance(actual, *point) <= effective_fit_tolerance)
            });
            if reproduces_chart {
                replacements.push((
                    procedural_id.clone(),
                    side,
                    PcurveGeometry::Nurbs {
                        degree: 1,
                        knots: linear_knots(parameters),
                        control_points: uv,
                        weights: None,
                        periodic: false,
                    },
                    effective_fit_tolerance,
                ));
            }
        }
    }
    for (procedural_id, side, pcurve, effective_fit_tolerance) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
            context.sides[side].pcurve = Some(pcurve);
            procedural.cache_fit_tolerance = Some(
                procedural
                    .cache_fit_tolerance
                    .unwrap_or(0.0)
                    .max(effective_fit_tolerance),
            );
        }
    }
    complete_coupled_support_uv(ir, pending);
}

pub(crate) fn blend_spine_cache_fit_tolerance(
    ir: &CadIr,
    surface: &SurfaceId,
    fit_tolerance: f64,
) -> f64 {
    blend_surface_definition(ir, surface)
        .and_then(|(_, spine, _, _)| {
            ir.model
                .procedural_curves
                .iter()
                .find(|procedural| procedural.curve == spine)
                .and_then(|procedural| procedural.cache_fit_tolerance)
        })
        .filter(|tolerance| tolerance.is_finite() && *tolerance > 0.0)
        .map_or(fit_tolerance, |tolerance| fit_tolerance + tolerance)
}

fn complete_coupled_support_uv(ir: &mut CadIr, pending: &[PendingExt11SupportUv]) {
    let mut replacements = Vec::new();
    for (procedural_id, points, parameters, fit_tolerance, _) in pending {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter()
            .find(|procedural| &procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            continue;
        };
        let missing = context
            .sides
            .each_ref()
            .map(|side| pcurve_requires_completion(side.pcurve.as_ref()));
        let [Some(first_surface), Some(second_surface)] =
            context.sides.each_ref().map(|side| side.surface.as_ref())
        else {
            continue;
        };
        let surfaces = [first_surface, second_surface];
        let unresolved_procedural_support = (0..2).any(|side| {
            missing[side]
                && pcurve_control_point_seed(context.sides[side].pcurve.as_ref(), 0).is_some()
                && ir.model.surfaces.iter().any(|surface| {
                    &surface.id == surfaces[side]
                        && matches!(surface.geometry, SurfaceGeometry::Procedural { .. })
                })
        });
        if !unresolved_procedural_support {
            continue;
        }
        let seeds = context
            .sides
            .each_ref()
            .map(|side| pcurve_control_point_seed(side.pcurve.as_ref(), 0));
        let Some(lanes) = continue_surface_intersection_parameters_with_seeds(
            ir,
            surfaces,
            points,
            *fit_tolerance,
            seeds,
        ) else {
            continue;
        };
        for side in 0..2 {
            if missing[side] {
                replacements.push((
                    procedural_id.clone(),
                    side,
                    PcurveGeometry::Nurbs {
                        degree: 1,
                        knots: linear_knots(parameters),
                        control_points: lanes[side].clone(),
                        weights: None,
                        periodic: false,
                    },
                ));
            }
        }
    }
    for (procedural_id, side, pcurve) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
            context.sides[side].pcurve = Some(pcurve);
        }
    }
}

pub(crate) fn complete_parameterization_equivalent_support_uv(ir: &mut CadIr) {
    let replacements = ir
        .model
        .procedural_curves
        .iter()
        .enumerate()
        .filter_map(|(procedural_index, procedural)| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            let missing = context
                .sides
                .each_ref()
                .map(|side| pcurve_requires_completion(side.pcurve.as_ref()));
            let target = match missing {
                [true, false] => 0,
                [false, true] => 1,
                _ => return None,
            };
            let source = 1 - target;
            let (Some(target_surface), Some(source_surface), Some(source_pcurve)) = (
                context.sides[target].surface.as_ref(),
                context.sides[source].surface.as_ref(),
                context.sides[source].pcurve.as_ref(),
            ) else {
                return None;
            };
            parameterization_equivalent_surfaces(ir, target_surface, source_surface)
                .then(|| (procedural_index, target, source_pcurve.clone()))
        })
        .collect::<Vec<_>>();
    for (procedural_index, side, pcurve) in replacements {
        let ProceduralCurveDefinition::Intersection { context, .. } =
            &mut ir.model.procedural_curves[procedural_index].definition
        else {
            unreachable!("definition selected above");
        };
        if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
            context.sides[side].pcurve = Some(pcurve);
        }
    }
}

pub(crate) fn parameterization_equivalent_surfaces(
    ir: &CadIr,
    first: &SurfaceId,
    second: &SurfaceId,
) -> bool {
    fn equivalent(
        ir: &CadIr,
        first: &SurfaceId,
        second: &SurfaceId,
        visited: &mut BTreeSet<(SurfaceId, SurfaceId)>,
    ) -> bool {
        if first == second {
            return true;
        }
        if !visited.insert((first.clone(), second.clone())) {
            return false;
        }
        let geometry = |id: &SurfaceId| {
            ir.model
                .surfaces
                .iter()
                .find(|surface| &surface.id == id)
                .map(|surface| &surface.geometry)
        };
        let (Some(first_geometry), Some(second_geometry)) = (geometry(first), geometry(second))
        else {
            return false;
        };
        if first_geometry == second_geometry {
            return true;
        }
        let construction = |geometry: &SurfaceGeometry| {
            let SurfaceGeometry::Procedural { construction } = geometry else {
                return None;
            };
            ir.model
                .procedural_surfaces
                .iter()
                .find(|procedural| &procedural.id == construction)
                .map(|procedural| &procedural.definition)
        };
        let (
            Some(ProceduralSurfaceDefinition::Offset {
                support: first_support,
                distance: first_distance,
                u_sense: first_u_sense,
                v_sense: first_v_sense,
                extension_flags: first_extensions,
                ..
            }),
            Some(ProceduralSurfaceDefinition::Offset {
                support: second_support,
                distance: second_distance,
                u_sense: second_u_sense,
                v_sense: second_v_sense,
                extension_flags: second_extensions,
                ..
            }),
        ) = (construction(first_geometry), construction(second_geometry))
        else {
            return false;
        };
        first_distance.to_bits() == second_distance.to_bits()
            && first_u_sense == second_u_sense
            && first_v_sense == second_v_sense
            && first_extensions == second_extensions
            && equivalent(ir, first_support, second_support, visited)
    }

    equivalent(ir, first, second, &mut BTreeSet::new())
}

pub(crate) fn attach_completed_intersection_pcurves(
    ir: &mut CadIr,
    graph: &Graph,
    prefix: &str,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) {
    let loop_faces = ir
        .model
        .loops
        .iter()
        .map(|loop_| (&loop_.id, &loop_.face))
        .collect::<BTreeMap<_, _>>();
    let face_surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| (&face.id, &face.surface))
        .collect::<BTreeMap<_, _>>();
    let edge_curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| Some((&edge.id, edge.curve.as_ref()?)))
        .collect::<BTreeMap<_, _>>();
    let mut candidates =
        BTreeMap::<(CurveId, SurfaceId), Vec<(PcurveGeometry, [f64; 2], Option<f64>)>>::new();
    for procedural in &ir.model.procedural_curves {
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            continue;
        };
        for side in &context.sides {
            let (Some(surface), Some(pcurve)) = (&side.surface, &side.pcurve) else {
                continue;
            };
            let values = candidates
                .entry((procedural.curve.clone(), surface.clone()))
                .or_default();
            let candidate = (
                pcurve.clone(),
                context.parameter_range,
                procedural.cache_fit_tolerance,
            );
            if !values.contains(&candidate) {
                values.push(candidate);
            }
        }
    }

    let replacements = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.pcurves.is_empty() && coedge.id.0.starts_with(prefix))
        .filter_map(|coedge| {
            let surface = loop_faces
                .get(&coedge.owner_loop)
                .and_then(|face| face_surfaces.get(*face))?;
            let curve = edge_curves.get(&coedge.edge)?;
            let [candidate] = candidates
                .get(&((*curve).clone(), (*surface).clone()))?
                .as_slice()
            else {
                return None;
            };
            pcurve_matches_edge(ir, &coedge.edge, surface, &candidate.0, candidate.2)
                .then(|| (coedge.id.clone(), candidate.clone()))
        })
        .collect::<Vec<_>>();
    for (coedge_id, (geometry, parameter_range, fit_tolerance)) in replacements {
        let Some(fin_xmt) = coedge_id
            .0
            .rsplit_once('#')
            .and_then(|(_, value)| value.parse::<u32>().ok())
        else {
            continue;
        };
        let pcurve_id = PcurveId(format!("{prefix}:intersection-pcurve-completed#{fin_xmt}"));
        if ir.model.pcurves.iter().any(|pcurve| pcurve.id == pcurve_id) {
            continue;
        }
        let source_offset = graph.get(17, fin_xmt).map_or(0, |node| node.pos as u64);
        annotations
            .note(&pcurve_id, source_stream, source_offset)
            .tag("INTERSECTION_PCURVE");
        annotations.derived(&pcurve_id, "geometry");
        annotations.derived(&pcurve_id, "parameter_range");
        if fit_tolerance.is_some() {
            annotations.derived(&pcurve_id, "fit_tolerance");
        }
        ir.model.pcurves.push(Pcurve {
            id: pcurve_id.clone(),
            geometry,
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: Some(parameter_range),
            fit_tolerance,
        });
        if let Some(coedge) = ir
            .model
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == coedge_id && coedge.pcurves.is_empty())
        {
            coedge.pcurves.push(cadmpeg_ir::topology::PcurveUse {
                pcurve: pcurve_id,
                isoparametric: None,
                parameter_range: None,
            });
        }
    }
}

fn decoded_surface_point(ir: &CadIr, surface: &SurfaceId, u: f64, v: f64) -> Option<Point3> {
    decoded_surface_point_inner(ir, surface, u, v, 0)
}

fn decoded_surface_point_inner(
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
) -> Option<Point3> {
    (depth < 32).then_some(())?;
    model_surface_point_by_id(ir, surface, u, v)
        .or_else(|| blend_surface_point_inner(ir, surface, u, v, depth + 1))
}

#[cfg(test)]
pub(crate) fn blend_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
) -> Option<Point2> {
    blend_surface_parameters_inner(ir, surface, point, seed, None, BlendParameterGrid::Build, 0)
}

pub(crate) fn blend_surface_parameters_for_fit(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: f64,
) -> Option<Point2> {
    blend_surface_parameters_for_fit_with_grid(
        ir,
        surface,
        point,
        seed,
        fit_tolerance,
        BlendParameterGrid::Build,
    )
}

#[derive(Clone, Copy)]
enum BlendParameterGrid {
    Build,
    Disabled,
}

fn blend_surface_parameters_for_fit_with_grid(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: f64,
    grid: BlendParameterGrid,
) -> Option<Point2> {
    blend_surface_parameters_inner(ir, surface, point, seed, Some(fit_tolerance), grid, 0)
}

fn blend_surface_parameters_inner(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
    grid: BlendParameterGrid,
    depth: usize,
) -> Option<Point2> {
    (depth < 32).then_some(())?;
    let (_, spine, _, _) = blend_surface_definition(ir, surface)?;
    if let (Some(seed), Some(fit_tolerance)) = (seed, fit_tolerance) {
        if let Some(parameters) =
            refine_blend_surface_parameters(ir, surface, point, seed, depth + 1).filter(
                |parameters| {
                    blend_surface_point_inner(ir, surface, parameters.u, parameters.v, depth + 1)
                        .is_some_and(|candidate| point_distance(candidate, point) <= fit_tolerance)
                },
            )
        {
            return Some(parameters);
        }
    }
    if let Some(fit_tolerance) = fit_tolerance {
        let boundary_parameters = [0usize, 1usize].map(|boundary| {
            blend_boundary_parameter(ir, surface, point, boundary, depth + 1).filter(|parameter| {
                blend_boundary_point(ir, surface, *parameter, boundary, depth + 1)
                    .is_some_and(|candidate| point_distance(candidate, point) <= fit_tolerance)
            })
        });
        if let Some((parameter, boundary)) = match boundary_parameters {
            [Some(parameter), None] => Some((parameter, 0usize)),
            [None, Some(parameter)] => Some((parameter, 1usize)),
            _ => None,
        } {
            return Some(Point2::new(parameter, boundary as f64));
        }
    }
    let angular =
        closest_spine_parameter(ir, &spine, point, seed.map(|seed| seed.u)).and_then(|u| {
            let (center, tangent, first, second, _) =
                blend_surface_frame(ir, surface, u, depth + 1)?;
            let radial = unit_vector(Vector3::new(
                point.x - center.x,
                point.y - center.y,
                point.z - center.z,
            ))?;
            let alpha = signed_angle(first, second, tangent);
            if !alpha.is_finite() || alpha.abs() <= 1.0e-12 {
                return None;
            }
            let theta = signed_angle(first, radial, tangent);
            (-2..=2)
                .filter_map(|turn| {
                    let v = (theta + f64::from(turn) * std::f64::consts::TAU) / alpha;
                    let candidate = blend_surface_point_inner(ir, surface, u, v, depth + 1)?;
                    let branch_distance = seed.map_or(v.abs(), |seed| (v - seed.v).abs());
                    Some((
                        Point2::new(u, v),
                        point_distance(candidate, point),
                        branch_distance,
                    ))
                })
                .min_by(|first, second| {
                    if (first.1 - second.1).abs() <= 1.0e-12 {
                        first.2.total_cmp(&second.2)
                    } else {
                        first.1.total_cmp(&second.1)
                    }
                })
                .map(|(parameters, _, _)| parameters)
        });
    if let Some(initial) = angular {
        let parameters = refine_blend_surface_parameters(ir, surface, point, initial, depth + 1)
            .unwrap_or(initial);
        if let Some(candidate) =
            blend_surface_point_inner(ir, surface, parameters.u, parameters.v, depth + 1)
        {
            let distance = point_distance(candidate, point);
            if fit_tolerance.is_none_or(|tolerance| distance <= tolerance) {
                return Some(parameters);
            }
        }
    }
    let initial = match grid {
        BlendParameterGrid::Build => coarse_blend_surface_parameters(ir, surface, point, depth + 1),
        BlendParameterGrid::Disabled => None,
    }?;
    let parameters =
        refine_blend_surface_parameters(ir, surface, point, initial, depth + 1).unwrap_or(initial);
    if !(0.0..=1.0).contains(&parameters.v) {
        return None;
    }
    let candidate = blend_surface_point_inner(ir, surface, parameters.u, parameters.v, depth + 1)?;
    let distance = point_distance(candidate, point);
    fit_tolerance
        .is_none_or(|tolerance| distance <= tolerance)
        .then_some(parameters)
}

pub(crate) fn coarse_blend_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    depth: usize,
) -> Option<Point2> {
    let grid = blend_surface_parameter_grid(ir, surface, depth)?;
    closest_blend_surface_grid_parameters(&grid, point)
}

fn blend_surface_parameter_grid(
    ir: &CadIr,
    surface: &SurfaceId,
    depth: usize,
) -> Option<Vec<(Point2, Point3)>> {
    (depth < 32).then_some(())?;
    let (_, spine, _, _) = blend_surface_definition(ir, surface)?;
    let curve = ir.model.curves.iter().find(|curve| curve.id == spine)?;
    let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        return None;
    };
    let degree = usize::try_from(nurbs.degree).ok()?;
    let count = nurbs.control_points.len();
    let domain = [*nurbs.knots.get(degree)?, *nurbs.knots.get(count)?];
    if !domain.into_iter().all(f64::is_finite) || domain[0] >= domain[1] {
        return None;
    }
    let mut grid = Vec::with_capacity(9 * 5);
    for u_index in 0..=8 {
        let u = domain[0] + (domain[1] - domain[0]) * f64::from(u_index) / 8.0;
        let frame = blend_surface_frame(ir, surface, u, depth + 1);
        for v_index in 0..=4 {
            let parameters = Point2::new(u, f64::from(v_index) / 4.0);
            let point = match v_index {
                0 => blend_boundary_point(ir, surface, u, 0, depth + 1),
                4 => blend_boundary_point(ir, surface, u, 1, depth + 1),
                _ => frame.map(|frame| blend_surface_point_from_frame(frame, parameters.v)),
            };
            let Some(point) = point else {
                continue;
            };
            grid.push((parameters, point));
        }
    }
    (!grid.is_empty()).then_some(grid)
}

fn closest_blend_surface_grid_parameters(
    grid: &[(Point2, Point3)],
    point: Point3,
) -> Option<Point2> {
    grid.iter()
        .min_by(|(_, first), (_, second)| {
            point_distance(*first, point).total_cmp(&point_distance(*second, point))
        })
        .map(|(parameters, _)| *parameters)
}

fn blend_surface_parameters_from_grid_for_fit(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    fit_tolerance: f64,
    grid: &[(Point2, Point3)],
) -> Option<Point2> {
    let initial = closest_blend_surface_grid_parameters(grid, point)?;
    let parameters =
        refine_blend_surface_parameters(ir, surface, point, initial, 0).unwrap_or(initial);
    (0.0..=1.0).contains(&parameters.v).then_some(())?;
    let candidate = blend_surface_point_inner(ir, surface, parameters.u, parameters.v, 0)?;
    (point_distance(candidate, point) <= fit_tolerance).then_some(parameters)
}

pub(crate) fn refine_blend_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    mut parameters: Point2,
    depth: usize,
) -> Option<Point2> {
    (depth < 32).then_some(())?;
    let (_, spine, _, _) = blend_surface_definition(ir, surface)?;
    let u_domain = ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == spine)
        .and_then(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => {
                let degree = usize::try_from(nurbs.degree).ok()?;
                let count = nurbs.control_points.len();
                Some([*nurbs.knots.get(degree)?, *nurbs.knots.get(count)?])
            }
            _ => None,
        });
    if let Some(domain) = u_domain {
        parameters.u = parameters.u.clamp(domain[0], domain[1]);
    }
    let squared_distance = |candidate: Point3| {
        (candidate.x - point.x).powi(2)
            + (candidate.y - point.y).powi(2)
            + (candidate.z - point.z).powi(2)
    };
    for _ in 0..16 {
        let position =
            blend_surface_point_inner(ir, surface, parameters.u, parameters.v, depth + 1)?;
        let residual = Vector3::new(
            position.x - point.x,
            position.y - point.y,
            position.z - point.z,
        );
        let current_distance = squared_distance(position);
        let u_step = parameter_derivative_step(parameters.u, u_domain);
        let v_step = parameter_derivative_step(parameters.v, None);
        let derivative = |along_u: bool, step: f64| {
            let mut before = parameters;
            let mut after = parameters;
            if along_u {
                before.u -= step;
                after.u += step;
                if let Some(domain) = u_domain {
                    before.u = before.u.clamp(domain[0], domain[1]);
                    after.u = after.u.clamp(domain[0], domain[1]);
                }
            } else {
                before.v -= step;
                after.v += step;
            }
            let width = if along_u {
                after.u - before.u
            } else {
                after.v - before.v
            };
            if !width.is_finite() || width == 0.0 {
                return None;
            }
            let first = blend_surface_point_inner(ir, surface, before.u, before.v, depth + 1)?;
            let second = blend_surface_point_inner(ir, surface, after.u, after.v, depth + 1)?;
            Some(Vector3::new(
                (second.x - first.x) / width,
                (second.y - first.y) / width,
                (second.z - first.z) / width,
            ))
        };
        let du = derivative(true, u_step)?;
        let dv = derivative(false, v_step)?;
        let Some((step_u, step_v)) = least_squares_step(du, dv, residual) else {
            break;
        };
        let mut scale = 1.0;
        let mut accepted = None;
        for _ in 0..8 {
            let mut candidate =
                Point2::new(parameters.u - scale * step_u, parameters.v - scale * step_v);
            if let Some(domain) = u_domain {
                candidate.u = candidate.u.clamp(domain[0], domain[1]);
            }
            if let Some(position) =
                blend_surface_point_inner(ir, surface, candidate.u, candidate.v, depth + 1)
            {
                if squared_distance(position) < current_distance {
                    accepted = Some(candidate);
                    break;
                }
            }
            scale *= 0.5;
        }
        let Some(candidate) = accepted else {
            break;
        };
        let converged = (candidate.u - parameters.u).abs() <= 1.0e-12 * (1.0 + parameters.u.abs())
            && (candidate.v - parameters.v).abs() <= 1.0e-12 * (1.0 + parameters.v.abs());
        parameters = candidate;
        if converged {
            break;
        }
    }
    Some(parameters)
}

#[cfg(test)]
pub(crate) fn blend_surface_point(
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
) -> Option<Point3> {
    blend_surface_point_inner(ir, surface, u, v, 0)
}

fn blend_surface_point_inner(
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
) -> Option<Point3> {
    (depth < 32).then_some(())?;
    if v.to_bits() == 0.0f64.to_bits() {
        return blend_boundary_point(ir, surface, u, 0, depth + 1);
    }
    if v.to_bits() == 1.0f64.to_bits() {
        return blend_boundary_point(ir, surface, u, 1, depth + 1);
    }
    let frame = blend_surface_frame(ir, surface, u, depth + 1)?;
    Some(blend_surface_point_from_frame(frame, v))
}

type BlendSurfaceFrame = (Point3, Vector3, Vector3, Vector3, f64);

fn blend_surface_point_from_frame(
    (center, tangent, first, second, radius): BlendSurfaceFrame,
    v: f64,
) -> Point3 {
    let alpha = signed_angle(first, second, tangent);
    let radial = rodrigues_rotate(first, tangent, v * alpha);
    Point3::new(
        center.x + radius * radial.x,
        center.y + radius * radial.y,
        center.z + radius * radial.z,
    )
}

fn blend_surface_frame(
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    depth: usize,
) -> Option<BlendSurfaceFrame> {
    (depth < 32).then_some(())?;
    let (supports, spine, radius, _) = blend_surface_definition(ir, surface)?;
    let center = model_curve_point(ir, &spine, u)?;
    let tangent = model_curve_tangent(ir, &spine, u)?;
    let first = spine_contact_direction(ir, &supports[0], &spine, u, center, radius, depth + 1)
        .or_else(|| surface_contact_direction(ir, &supports[0], center, depth + 1))?;
    let second = spine_contact_direction(ir, &supports[1], &spine, u, center, radius, depth + 1)
        .or_else(|| surface_contact_direction(ir, &supports[1], center, depth + 1))?;
    Some((center, tangent, first, second, radius))
}

fn spine_contact_direction(
    ir: &CadIr,
    support: &SurfaceId,
    spine: &CurveId,
    parameter: f64,
    center: Point3,
    radius: f64,
    depth: usize,
) -> Option<Vector3> {
    let contact = spine_contact_point(ir, support, spine, parameter, radius, depth + 1)?;
    unit_vector(Vector3::new(
        contact.x - center.x,
        contact.y - center.y,
        contact.z - center.z,
    ))
}

fn blend_boundary_point(
    ir: &CadIr,
    surface: &SurfaceId,
    parameter: f64,
    boundary: usize,
    depth: usize,
) -> Option<Point3> {
    (depth < 32).then_some(())?;
    let (supports, spine, radius, _) = blend_surface_definition(ir, surface)?;
    spine_contact_point(
        ir,
        supports.get(boundary)?,
        &spine,
        parameter,
        radius,
        depth + 1,
    )
}

fn blend_boundary_parameter(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    boundary: usize,
    depth: usize,
) -> Option<f64> {
    (depth < 32).then_some(())?;
    let (supports, spine, radius, _) = blend_surface_definition(ir, surface)?;
    let support = supports.get(boundary)?;
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == support)?;
    let uv = match &carrier.geometry {
        SurfaceGeometry::Nurbs(nurbs) => nurbs_parameters(nurbs, point, None),
        SurfaceGeometry::Procedural { .. } => offset_surface_parameters(ir, support, point, None),
        geometry => analytic_surface_parameters(geometry, point),
    }?;
    let pcurve = spine_contact_pcurve(ir, support, &spine, radius, depth + 1)?;
    closest_pcurve_parameter(pcurve, uv)
}

fn blend_boundary_parameter_from_support_pcurve(
    ir: &CadIr,
    blend: &SurfaceId,
    support: &SurfaceId,
    support_pcurve: &PcurveGeometry,
    curve_parameter: f64,
    point: Point3,
    fit_tolerance: f64,
) -> Option<Point2> {
    let (supports, spine, radius, _) = blend_surface_definition(ir, blend)?;
    let boundary = supports
        .iter()
        .position(|candidate| parameterization_equivalent_surfaces(ir, candidate, support))?;
    if supports
        .iter()
        .filter(|candidate| parameterization_equivalent_surfaces(ir, candidate, support))
        .count()
        != 1
    {
        return None;
    }
    let support_uv = pcurve_uv(support_pcurve, curve_parameter)?;
    let contact_pcurve = spine_contact_pcurve(ir, support, &spine, radius, 0)?;
    let parameter = closest_pcurve_parameter(contact_pcurve, support_uv)?;
    blend_boundary_point(ir, blend, parameter, boundary, 0)
        .filter(|candidate| point_distance(*candidate, point) <= fit_tolerance)
        .map(|_| Point2::new(parameter, boundary as f64))
}

pub(crate) fn closest_pcurve_parameter(pcurve: &PcurveGeometry, point: Point2) -> Option<f64> {
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        ..
    } = pcurve
    else {
        return None;
    };
    let degree = usize::try_from(*degree).ok()?;
    let count = control_points.len();
    if count <= degree || knots.len() != count.checked_add(degree)?.checked_add(1)? {
        return None;
    }
    let domain = [*knots.get(degree)?, *knots.get(count)?];
    if !domain[0].is_finite() || !domain[1].is_finite() || domain[0] >= domain[1] {
        return None;
    }
    if degree != 1 || weights.is_some() {
        let squared_distance = |parameter| {
            let position = pcurve_uv(pcurve, parameter)?;
            Some((position.u - point.u).powi(2) + (position.v - point.v).powi(2))
        };
        let samples = knot_domain_samples(knots, degree, domain);
        let distances = samples
            .iter()
            .map(|parameter| squared_distance(*parameter))
            .collect::<Option<Vec<_>>>()?;
        let mut best = samples[0];
        let mut best_distance = distances[0];
        for (index, &distance) in distances.iter().enumerate() {
            if distance < best_distance {
                best = samples[index];
                best_distance = distance;
            }
            if index > 0
                && index + 1 < samples.len()
                && distance <= distances[index - 1]
                && distance <= distances[index + 1]
            {
                let (parameter, distance) = golden_section_minimum(
                    samples[index - 1],
                    samples[index + 1],
                    &squared_distance,
                )?;
                if distance < best_distance {
                    best = parameter;
                    best_distance = distance;
                }
            }
        }
        return Some(best);
    }
    let mut candidates = control_points
        .windows(2)
        .enumerate()
        .filter_map(|(index, segment)| {
            let start = segment[0];
            let end = segment[1];
            let direction = Point2::new(end.u - start.u, end.v - start.v);
            let squared_length = direction.u * direction.u + direction.v * direction.v;
            if !squared_length.is_finite() || squared_length == 0.0 {
                return None;
            }
            let fraction = (((point.u - start.u) * direction.u
                + (point.v - start.v) * direction.v)
                / squared_length)
                .clamp(0.0, 1.0);
            let span_start = *knots.get(index + 1)?;
            let span_end = *knots.get(index + 2)?;
            if !span_start.is_finite() || !span_end.is_finite() || span_start >= span_end {
                return None;
            }
            let projected = Point2::new(
                start.u + fraction * direction.u,
                start.v + fraction * direction.v,
            );
            let squared_distance =
                (projected.u - point.u).powi(2) + (projected.v - point.v).powi(2);
            Some((
                span_start + fraction * (span_end - span_start),
                squared_distance,
            ))
        });
    let first = candidates.next()?;
    let best = candidates.fold(first, |best, candidate| {
        if candidate.1 < best.1 {
            candidate
        } else {
            best
        }
    });
    Some(best.0)
}

fn spine_contact_point(
    ir: &CadIr,
    support: &SurfaceId,
    spine: &CurveId,
    parameter: f64,
    radius: f64,
    depth: usize,
) -> Option<Point3> {
    (depth < 32).then_some(())?;
    let pcurve = spine_contact_pcurve(ir, support, spine, radius, depth + 1)?;
    let uv = pcurve_uv(pcurve, parameter)?;
    decoded_surface_point_inner(ir, support, uv.u, uv.v, depth + 1)
}

fn spine_contact_pcurve<'a>(
    ir: &'a CadIr,
    support: &SurfaceId,
    spine: &CurveId,
    radius: f64,
    depth: usize,
) -> Option<&'a PcurveGeometry> {
    (depth < 32).then_some(())?;
    let procedural = ir.model.procedural_curves.iter().find(|candidate| {
        candidate.curve == *spine
            && matches!(
                candidate.definition,
                ProceduralCurveDefinition::Intersection { .. }
            )
    })?;
    let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
        unreachable!("definition selected above");
    };
    let candidates = context.sides.iter().filter_map(|side| {
        let side_surface = side.surface.as_ref()?;
        let pcurve = side.pcurve.as_ref()?;
        let offset = constant_surface_offset_between(ir, support, side_surface, depth + 1)?;
        if !blend_contact_offset_matches(0.0, offset, radius) {
            return None;
        }
        Some(pcurve)
    });
    let candidates = candidates.collect::<Vec<_>>();
    let [pcurve] = candidates.as_slice() else {
        return None;
    };
    Some(*pcurve)
}

pub(crate) fn constant_surface_offset_between(
    ir: &CadIr,
    support: &SurfaceId,
    offset_surface: &SurfaceId,
    depth: usize,
) -> Option<f64> {
    let (support_base, support_offset) = surface_offset_lineage(ir, support, depth + 1)?;
    let (offset_base, offset_distance) = surface_offset_lineage(ir, offset_surface, depth + 1)?;
    if support_base == offset_base {
        return Some(offset_distance - support_offset);
    }
    let support_geometry = &ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == support_base)?
        .geometry;
    let offset_geometry = &ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == offset_base)?
        .geometry;
    let base_offset = analytic_surface_offset(support_geometry, offset_geometry)
        .or_else(|| blend_surface_offset(ir, &support_base, &offset_base, depth + 1))?;
    Some(base_offset + offset_distance - support_offset)
}

fn blend_surface_offset(
    ir: &CadIr,
    support: &SurfaceId,
    offset: &SurfaceId,
    depth: usize,
) -> Option<f64> {
    (depth < 32).then_some(())?;
    let (support_carriers, support_spine, support_radius, support_reversed) =
        blend_surface_definition(ir, support)?;
    let (offset_carriers, offset_spine, offset_radius, offset_reversed) =
        blend_surface_definition(ir, offset)?;
    (support_spine == offset_spine).then_some(())?;

    let distance = offset_radius - support_radius;
    let magnitude = distance.abs();
    let matches = [[0usize, 1usize], [1usize, 0usize]]
        .into_iter()
        .filter(|permutation| {
            permutation
                .iter()
                .enumerate()
                .all(|(support_index, &offset_index)| {
                    support_reversed[support_index] == offset_reversed[offset_index]
                        && constant_surface_offset_between(
                            ir,
                            &support_carriers[support_index],
                            &offset_carriers[offset_index],
                            depth + 1,
                        )
                        .is_some_and(|carrier_distance| {
                            blend_contact_offset_matches(0.0, carrier_distance, magnitude)
                        })
                })
        })
        .count();
    (matches == 1).then_some(distance)
}

pub(crate) fn analytic_surface_offset(
    support: &SurfaceGeometry,
    offset: &SurfaceGeometry,
) -> Option<f64> {
    match (support, offset) {
        (
            SurfaceGeometry::Plane {
                origin: support_origin,
                normal: support_normal,
                u_axis: support_u,
            },
            SurfaceGeometry::Plane {
                origin: offset_origin,
                normal: offset_normal,
                u_axis: offset_u,
            },
        ) if support_normal == offset_normal && support_u == offset_u => {
            let delta = Vector3::new(
                offset_origin.x - support_origin.x,
                offset_origin.y - support_origin.y,
                offset_origin.z - support_origin.z,
            );
            let distance = dot_vector(delta, *support_normal);
            let residual = Vector3::new(
                delta.x - distance * support_normal.x,
                delta.y - distance * support_normal.y,
                delta.z - distance * support_normal.z,
            );
            let scale = [
                support_origin.x,
                support_origin.y,
                support_origin.z,
                offset_origin.x,
                offset_origin.y,
                offset_origin.z,
                distance,
            ]
            .into_iter()
            .fold(1.0_f64, |scale, value| scale.max(value.abs()));
            let tolerance = 64.0 * f64::EPSILON * scale;
            (dot_vector(residual, residual) <= tolerance * tolerance).then_some(distance)
        }
        (
            SurfaceGeometry::Cylinder {
                origin: support_origin,
                axis: support_axis,
                ref_direction: support_ref,
                radius: support_radius,
            },
            SurfaceGeometry::Cylinder {
                origin: offset_origin,
                axis: offset_axis,
                ref_direction: offset_ref,
                radius: offset_radius,
            },
        ) if support_origin == offset_origin
            && support_axis == offset_axis
            && support_ref == offset_ref =>
        {
            Some(offset_radius - support_radius)
        }
        (
            SurfaceGeometry::Cone {
                origin: support_origin,
                axis: support_axis,
                ref_direction: support_ref,
                radius: support_radius,
                ratio: support_ratio,
                half_angle: support_angle,
            },
            SurfaceGeometry::Cone {
                origin: offset_origin,
                axis: offset_axis,
                ref_direction: offset_ref,
                radius: offset_radius,
                ratio: offset_ratio,
                half_angle: offset_angle,
            },
        ) if support_axis == offset_axis
            && support_ref == offset_ref
            && support_ratio.to_bits() == 1.0_f64.to_bits()
            && offset_ratio.to_bits() == 1.0_f64.to_bits()
            && support_angle.to_bits() == offset_angle.to_bits() =>
        {
            let delta = Vector3::new(
                offset_origin.x - support_origin.x,
                offset_origin.y - support_origin.y,
                offset_origin.z - support_origin.z,
            );
            let axial_delta = dot_vector(delta, *support_axis);
            let residual = Vector3::new(
                delta.x - axial_delta * support_axis.x,
                delta.y - axial_delta * support_axis.y,
                delta.z - axial_delta * support_axis.z,
            );
            let radial_delta = offset_radius - support_radius;
            let distance = radial_delta * support_angle.cos() - axial_delta * support_angle.sin();
            let tangent_residual =
                radial_delta * support_angle.sin() + axial_delta * support_angle.cos();
            let scale = [
                support_origin.x,
                support_origin.y,
                support_origin.z,
                offset_origin.x,
                offset_origin.y,
                offset_origin.z,
                *support_radius,
                *offset_radius,
                axial_delta,
                distance,
                tangent_residual,
            ]
            .into_iter()
            .fold(1.0_f64, |scale, value| scale.max(value.abs()));
            let tolerance = 64.0 * f64::EPSILON * scale;
            (distance.is_finite()
                && dot_vector(residual, residual) <= tolerance * tolerance
                && tangent_residual.abs() <= tolerance)
                .then_some(distance)
        }
        (
            SurfaceGeometry::Sphere {
                center: support_center,
                axis: support_axis,
                ref_direction: support_ref,
                radius: support_radius,
            },
            SurfaceGeometry::Sphere {
                center: offset_center,
                axis: offset_axis,
                ref_direction: offset_ref,
                radius: offset_radius,
            },
        ) if support_center == offset_center
            && support_axis == offset_axis
            && support_ref == offset_ref
            && support_radius.signum().to_bits() == offset_radius.signum().to_bits() =>
        {
            Some((offset_radius - support_radius) * support_radius.signum())
        }
        (
            SurfaceGeometry::Torus {
                center: support_center,
                axis: support_axis,
                ref_direction: support_ref,
                major_radius: support_major,
                minor_radius: support_minor,
            },
            SurfaceGeometry::Torus {
                center: offset_center,
                axis: offset_axis,
                ref_direction: offset_ref,
                major_radius: offset_major,
                minor_radius: offset_minor,
            },
        ) if support_center == offset_center
            && support_axis == offset_axis
            && support_ref == offset_ref
            && support_major.to_bits() == offset_major.to_bits()
            && support_minor.signum().to_bits() == offset_minor.signum().to_bits()
            && *support_major > support_minor.abs()
            && *offset_major > offset_minor.abs() =>
        {
            Some((offset_minor - support_minor) * support_minor.signum())
        }
        _ => None,
    }
}

pub(crate) fn blend_contact_offset_matches(
    support_offset: f64,
    spine_side_offset: f64,
    radius: f64,
) -> bool {
    let actual = (spine_side_offset - support_offset).abs();
    let expected = radius.abs();
    let scale = actual.max(expected).max(1.0);
    actual.is_finite()
        && expected.is_finite()
        && (actual - expected).abs() <= 64.0 * f64::EPSILON * scale
}

fn surface_offset_lineage(
    ir: &CadIr,
    surface: &SurfaceId,
    depth: usize,
) -> Option<(SurfaceId, f64)> {
    (depth < 32).then_some(())?;
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let SurfaceGeometry::Procedural { construction } = &carrier.geometry else {
        return Some((surface.clone(), 0.0));
    };
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|candidate| candidate.id == *construction && candidate.surface == *surface)?;
    let ProceduralSurfaceDefinition::Offset {
        support, distance, ..
    } = &procedural.definition
    else {
        return Some((surface.clone(), 0.0));
    };
    let (base, accumulated) = surface_offset_lineage(ir, support, depth + 1)?;
    Some((base, accumulated + distance))
}

fn blend_surface_definition(
    ir: &CadIr,
    surface: &SurfaceId,
) -> Option<([SurfaceId; 2], CurveId, f64, [bool; 2])> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let SurfaceGeometry::Procedural { construction } = &carrier.geometry else {
        return None;
    };
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|candidate| &candidate.id == construction && &candidate.surface == surface)?;
    let ProceduralSurfaceDefinition::Blend {
        supports: [Some(first), Some(second)],
        spine: Some(spine),
        radius: BlendRadiusLaw::Constant { signed_radius },
        cross_section: BlendCrossSection::Circular,
        ..
    } = &procedural.definition
    else {
        return None;
    };
    let radius = signed_radius.abs();
    (radius.is_finite() && radius > 0.0).then(|| {
        (
            [first.surface.clone(), second.surface.clone()],
            spine.clone(),
            radius,
            [first.reversed, second.reversed],
        )
    })
}

fn surface_contact_direction(
    ir: &CadIr,
    surface: &SurfaceId,
    center: Point3,
    depth: usize,
) -> Option<Vector3> {
    (depth < 32).then_some(())?;
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let parameters = match &carrier.geometry {
        SurfaceGeometry::Nurbs(nurbs) => nurbs_parameters(nurbs, center, None),
        SurfaceGeometry::Procedural { .. } => offset_surface_parameters(ir, surface, center, None)
            .or_else(|| {
                blend_surface_parameters_inner(
                    ir,
                    surface,
                    center,
                    None,
                    None,
                    BlendParameterGrid::Build,
                    depth + 1,
                )
            }),
        geometry => analytic_surface_parameters(geometry, center),
    }?;
    let contact = decoded_surface_point_inner(ir, surface, parameters.u, parameters.v, depth + 1)?;
    unit_vector(Vector3::new(
        contact.x - center.x,
        contact.y - center.y,
        contact.z - center.z,
    ))
}

fn model_curve_point(ir: &CadIr, curve: &CurveId, parameter: f64) -> Option<Point3> {
    let carrier = ir
        .model
        .curves
        .iter()
        .find(|candidate| &candidate.id == curve)?;
    curve_point(&carrier.geometry, parameter)
}

fn model_curve_tangent(ir: &CadIr, curve: &CurveId, parameter: f64) -> Option<Vector3> {
    let step = 1.0e-6 * (1.0 + parameter.abs());
    let center = model_curve_point(ir, curve, parameter)?;
    let before = model_curve_point(ir, curve, parameter - step);
    let after = model_curve_point(ir, curve, parameter + step);
    let (before, after) = match (before, after) {
        (Some(before), Some(after)) => (before, after),
        (Some(before), None) => (before, center),
        (None, Some(after)) => (center, after),
        (None, None) => return None,
    };
    unit_vector(Vector3::new(
        after.x - before.x,
        after.y - before.y,
        after.z - before.z,
    ))
}

pub(crate) fn closest_spine_parameter(
    ir: &CadIr,
    curve: &CurveId,
    point: Point3,
    seed: Option<f64>,
) -> Option<f64> {
    let carrier = ir
        .model
        .curves
        .iter()
        .find(|candidate| &candidate.id == curve)?;
    match &carrier.geometry {
        CurveGeometry::Line { origin, direction } => Some(
            (point.x - origin.x) * direction.x
                + (point.y - origin.y) * direction.y
                + (point.z - origin.z) * direction.z,
        ),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            ..
        }
        | CurveGeometry::Ellipse {
            center,
            axis,
            major_direction: ref_direction,
            ..
        } => closest_periodic_analytic_curve_parameter(
            &carrier.geometry,
            *center,
            *axis,
            *ref_direction,
            point,
            seed,
        ),
        CurveGeometry::Nurbs(nurbs) => {
            let degree = usize::try_from(nurbs.degree).ok()?;
            let count = nurbs.control_points.len();
            let domain = [*nurbs.knots.get(degree)?, *nurbs.knots.get(count)?];
            if domain[0] >= domain[1] {
                return None;
            }
            closest_nurbs_curve_parameter(
                &carrier.geometry,
                &nurbs.knots,
                degree,
                domain,
                point,
                seed,
            )
        }
        _ => None,
    }
}

fn closest_periodic_analytic_curve_parameter(
    geometry: &CurveGeometry,
    center: Point3,
    axis: Vector3,
    reference: Vector3,
    point: Point3,
    seed: Option<f64>,
) -> Option<f64> {
    let transverse = cross_vector(axis, reference);
    let delta = Vector3::new(point.x - center.x, point.y - center.y, point.z - center.z);
    let phase = dot_vector(delta, transverse).atan2(dot_vector(delta, reference));
    phase.is_finite().then_some(())?;
    let anchor = seed.map_or(phase, |seed| {
        phase + ((seed - phase) / std::f64::consts::TAU).round() * std::f64::consts::TAU
    });
    let lower = anchor - std::f64::consts::PI;
    let step = std::f64::consts::TAU / 64.0;
    let squared_distance = |parameter| {
        let position = curve_point(geometry, parameter)?;
        Some(
            (position.x - point.x).powi(2)
                + (position.y - point.y).powi(2)
                + (position.z - point.z).powi(2),
        )
    };
    let samples = (0..=64)
        .map(|index| lower + f64::from(index) * step)
        .collect::<Vec<_>>();
    let distances = samples
        .iter()
        .map(|parameter| squared_distance(*parameter))
        .collect::<Option<Vec<_>>>()?;
    let mut best_index = 0;
    for index in 1..distances.len() {
        if distances[index] < distances[best_index]
            || distances[index] == distances[best_index]
                && (samples[index] - anchor).abs() < (samples[best_index] - anchor).abs()
        {
            best_index = index;
        }
    }
    let bracket_center = match best_index {
        0 => samples[0] + std::f64::consts::TAU,
        64 => samples[64] - std::f64::consts::TAU,
        _ => samples[best_index],
    };
    let (parameter, _) = golden_section_minimum(
        bracket_center - step,
        bracket_center + step,
        &squared_distance,
    )?;
    Some(parameter + ((anchor - parameter) / std::f64::consts::TAU).round() * std::f64::consts::TAU)
}

fn closest_nurbs_curve_parameter(
    geometry: &CurveGeometry,
    knots: &[f64],
    degree: usize,
    domain: [f64; 2],
    point: Point3,
    seed: Option<f64>,
) -> Option<f64> {
    let squared_distance = |parameter| {
        let position = curve_point(geometry, parameter)?;
        Some(
            (position.x - point.x).powi(2)
                + (position.y - point.y).powi(2)
                + (position.z - point.z).powi(2),
        )
    };
    let samples = knot_domain_samples(knots, degree, domain);
    let distances = samples
        .iter()
        .map(|parameter| squared_distance(*parameter))
        .collect::<Option<Vec<_>>>()?;
    let mut best = samples[0];
    let mut best_distance = distances[0];
    let mut best_seed_distance = seed.map_or(best.abs(), |seed| (best - seed).abs());
    let mut consider = |parameter: f64, distance: f64| {
        let seed_distance = seed.map_or(parameter.abs(), |seed| (parameter - seed).abs());
        let same_point = (distance - best_distance).abs()
            <= f64::EPSILON * 64.0 * distance.abs().max(best_distance.abs()).max(1.0);
        if distance < best_distance && !same_point
            || same_point && seed_distance < best_seed_distance
        {
            best = parameter;
            best_distance = distance;
            best_seed_distance = seed_distance;
        }
    };
    for (index, &distance) in distances.iter().enumerate() {
        consider(samples[index], distance);
        if index > 0
            && index + 1 < samples.len()
            && distance <= distances[index - 1]
            && distance <= distances[index + 1]
        {
            let (parameter, distance) =
                golden_section_minimum(samples[index - 1], samples[index + 1], &squared_distance)?;
            consider(parameter, distance);
        }
    }
    if let Some(seed) = seed {
        let seed = seed.clamp(domain[0], domain[1]);
        let insertion = samples.partition_point(|parameter| *parameter < seed);
        let lower = samples[insertion.saturating_sub(1)];
        let upper = samples[insertion.min(samples.len() - 1)];
        if lower < upper {
            let (parameter, distance) = golden_section_minimum(lower, upper, &squared_distance)?;
            consider(parameter, distance);
        } else {
            consider(seed, squared_distance(seed)?);
        }
    }
    Some(best)
}

fn knot_domain_samples(knots: &[f64], degree: usize, domain: [f64; 2]) -> Vec<f64> {
    let subdivisions = 2 * (degree + 1).max(2);
    let mut samples = vec![domain[0]];
    for span in knots[degree..].windows(2) {
        let start = span[0].max(domain[0]);
        let end = span[1].min(domain[1]);
        if start >= end {
            continue;
        }
        for index in 1..=subdivisions {
            samples.push(start + (end - start) * index as f64 / subdivisions as f64);
        }
        if end >= domain[1] {
            break;
        }
    }
    samples.sort_by(f64::total_cmp);
    samples.dedup_by(|left, right| *left == *right);
    samples
}

fn golden_section_minimum(
    mut lower: f64,
    mut upper: f64,
    value: &impl Fn(f64) -> Option<f64>,
) -> Option<(f64, f64)> {
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    let mut left = upper - ratio * (upper - lower);
    let mut right = lower + ratio * (upper - lower);
    let mut left_value = value(left)?;
    let mut right_value = value(right)?;
    for _ in 0..64 {
        if left_value <= right_value {
            upper = right;
            right = left;
            right_value = left_value;
            left = upper - ratio * (upper - lower);
            left_value = value(left)?;
        } else {
            lower = left;
            left = right;
            left_value = right_value;
            right = lower + ratio * (upper - lower);
            right_value = value(right)?;
        }
    }
    if left_value <= right_value {
        Some((left, left_value))
    } else {
        Some((right, right_value))
    }
}

fn signed_angle(first: Vector3, second: Vector3, axis: Vector3) -> f64 {
    dot_vector(cross_vector(first, second), axis).atan2(dot_vector(first, second))
}

fn rodrigues_rotate(vector: Vector3, axis: Vector3, angle: f64) -> Vector3 {
    let cross = cross_vector(axis, vector);
    let dot = dot_vector(axis, vector);
    Vector3::new(
        vector.x * angle.cos() + cross.x * angle.sin() + axis.x * dot * (1.0 - angle.cos()),
        vector.y * angle.cos() + cross.y * angle.sin() + axis.y * dot * (1.0 - angle.cos()),
        vector.z * angle.cos() + cross.z * angle.sin() + axis.z * dot * (1.0 - angle.cos()),
    )
}

pub(crate) fn offset_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
) -> Option<Point2> {
    offset_surface_parameters_with_tolerance(ir, surface, point, seed, None)
}

pub(crate) fn offset_surface_parameters_with_tolerance(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
) -> Option<Point2> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let SurfaceGeometry::Procedural { construction } = &carrier.geometry else {
        return None;
    };
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|candidate| &candidate.id == construction && &candidate.surface == surface)?;
    let ProceduralSurfaceDefinition::Offset { support, .. } = &procedural.definition else {
        return None;
    };
    let domain = surface_parameter_domain(ir, support);
    let mut parameters = seed
        .or_else(|| initial_surface_parameters(ir, support, point, None))
        .or_else(|| {
            domain.and_then(|domain| coarse_model_surface_parameters(ir, surface, point, domain))
        })?;
    clamp_surface_parameters(&mut parameters, domain);
    for _ in 0..32 {
        let position = model_surface_point_by_id(ir, surface, parameters.u, parameters.v)?;
        let residual = Vector3::new(
            position.x - point.x,
            position.y - point.y,
            position.z - point.z,
        );
        if fit_tolerance.is_some_and(|tolerance| {
            tolerance.is_finite()
                && tolerance >= 0.0
                && dot_vector(residual, residual) <= tolerance * tolerance
        }) {
            break;
        }
        let u_step = parameter_derivative_step(parameters.u, domain.map(|domain| domain.0));
        let v_step = parameter_derivative_step(parameters.v, domain.map(|domain| domain.1));
        let du =
            model_surface_derivative(ir, surface, parameters, u_step, true, domain, [None, None])?;
        let dv =
            model_surface_derivative(ir, surface, parameters, v_step, false, domain, [None, None])?;
        let Some((step_u, step_v)) = least_squares_step(du, dv, residual) else {
            break;
        };
        parameters.u -= step_u;
        parameters.v -= step_v;
        clamp_surface_parameters(&mut parameters, domain);
        if step_u.abs() <= 1.0e-12 * (1.0 + parameters.u.abs())
            && step_v.abs() <= 1.0e-12 * (1.0 + parameters.v.abs())
        {
            break;
        }
    }
    Some(parameters)
}

fn coarse_model_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    domain: ([f64; 2], [f64; 2]),
) -> Option<Point2> {
    let (u_domain, v_domain) = domain;
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    for ui in 0..=8 {
        for vi in 0..=8 {
            let parameters = Point2::new(
                u_domain[0] + (u_domain[1] - u_domain[0]) * f64::from(ui) / 8.0,
                v_domain[0] + (v_domain[1] - v_domain[0]) * f64::from(vi) / 8.0,
            );
            let Some(candidate) =
                model_surface_point_by_id(ir, surface, parameters.u, parameters.v)
            else {
                continue;
            };
            let distance = (candidate.x - point.x).powi(2)
                + (candidate.y - point.y).powi(2)
                + (candidate.z - point.z).powi(2);
            if distance < best_distance {
                best = Some(parameters);
                best_distance = distance;
            }
        }
    }
    best
}

fn initial_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
) -> Option<Point2> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    match &carrier.geometry {
        SurfaceGeometry::Nurbs(nurbs) => nurbs_parameters(nurbs, point, seed),
        SurfaceGeometry::Procedural { construction } => {
            let procedural =
                ir.model.procedural_surfaces.iter().find(|candidate| {
                    &candidate.id == construction && &candidate.surface == surface
                })?;
            let ProceduralSurfaceDefinition::Offset { support, .. } = &procedural.definition else {
                return None;
            };
            initial_surface_parameters(ir, support, point, seed)
        }
        geometry => analytic_surface_parameters(geometry, point),
    }
}

fn surface_parameter_domain(ir: &CadIr, surface: &SurfaceId) -> Option<([f64; 2], [f64; 2])> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    match &carrier.geometry {
        SurfaceGeometry::Nurbs(nurbs) => {
            let u_degree = usize::try_from(nurbs.u_degree).ok()?;
            let v_degree = usize::try_from(nurbs.v_degree).ok()?;
            let u_count = usize::try_from(nurbs.u_count).ok()?;
            let v_count = usize::try_from(nurbs.v_count).ok()?;
            Some((
                [*nurbs.u_knots.get(u_degree)?, *nurbs.u_knots.get(u_count)?],
                [*nurbs.v_knots.get(v_degree)?, *nurbs.v_knots.get(v_count)?],
            ))
        }
        SurfaceGeometry::Procedural { construction } => {
            let procedural =
                ir.model.procedural_surfaces.iter().find(|candidate| {
                    &candidate.id == construction && &candidate.surface == surface
                })?;
            let ProceduralSurfaceDefinition::Offset { support, .. } = &procedural.definition else {
                return None;
            };
            surface_parameter_domain(ir, support)
        }
        _ => None,
    }
}

fn clamp_surface_parameters(parameters: &mut Point2, domain: Option<([f64; 2], [f64; 2])>) {
    if let Some((u_domain, v_domain)) = domain {
        parameters.u = parameters.u.clamp(u_domain[0], u_domain[1]);
        parameters.v = parameters.v.clamp(v_domain[0], v_domain[1]);
    }
}

fn parameter_derivative_step(parameter: f64, domain: Option<[f64; 2]>) -> f64 {
    domain.map_or_else(
        || 1.0e-6 * (1.0 + parameter.abs()),
        |domain| 1.0e-6 * (domain[1] - domain[0]).abs().max(1.0),
    )
}

fn model_surface_derivative(
    ir: &CadIr,
    surface: &SurfaceId,
    parameters: Point2,
    step: f64,
    along_u: bool,
    domain: Option<([f64; 2], [f64; 2])>,
    periods: [Option<f64>; 2],
) -> Option<Vector3> {
    let mut before = parameters;
    let mut after = parameters;
    if along_u {
        before.u -= step;
        after.u += step;
    } else {
        before.v -= step;
        after.v += step;
    }
    clamp_surface_parameters_with_periods(&mut before, domain, periods);
    clamp_surface_parameters_with_periods(&mut after, domain, periods);
    let width = if along_u {
        after.u - before.u
    } else {
        after.v - before.v
    };
    if !width.is_finite() || width == 0.0 {
        return None;
    }
    let first = model_surface_point_by_id(ir, surface, before.u, before.v)?;
    let second = model_surface_point_by_id(ir, surface, after.u, after.v)?;
    Some(Vector3::new(
        (second.x - first.x) / width,
        (second.y - first.y) / width,
        (second.z - first.z) / width,
    ))
}

/// Continue one chart-selected surface-intersection branch in both support
/// parameter spaces. The chart seeds and orders the branch; corrected points
/// satisfy the two support surfaces rather than interpolating chart samples.
#[cfg(test)]
pub(crate) fn continue_surface_intersection_parameters(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    chart: &[Point3],
    fit_tolerance: f64,
) -> Option<[Vec<Point2>; 2]> {
    continue_surface_intersection_parameters_with_seeds(
        ir,
        surfaces,
        chart,
        fit_tolerance,
        [None, None],
    )
}

fn continue_surface_intersection_parameters_with_seeds(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    chart: &[Point3],
    fit_tolerance: f64,
    seeds: [Option<Point2>; 2],
) -> Option<[Vec<Point2>; 2]> {
    if chart.len() < 2
        || surfaces[0] == surfaces[1]
        || !fit_tolerance.is_finite()
        || fit_tolerance <= 0.0
    {
        return None;
    }
    let fit_parameters = |surface: &SurfaceId, point: Point3, seed: Option<Point2>| {
        let geometry = &ir
            .model
            .surfaces
            .iter()
            .find(|candidate| &candidate.id == surface)?
            .geometry;
        match geometry {
            SurfaceGeometry::Nurbs(nurbs) => nurbs_parameters(nurbs, point, seed),
            SurfaceGeometry::Procedural { .. } => offset_surface_parameters_with_tolerance(
                ir,
                surface,
                point,
                seed,
                Some(fit_tolerance),
            )
            .or_else(|| blend_surface_parameters_for_fit(ir, surface, point, seed, fit_tolerance)),
            geometry => analytic_surface_parameters(geometry, point),
        }
    };
    let first = [
        fit_parameters(surfaces[0], chart[0], seeds[0])?,
        fit_parameters(surfaces[1], chart[0], seeds[1])?,
    ];
    let space = IntersectionParameterSpace {
        domains: surfaces.map(|surface| surface_parameter_domain(ir, surface)),
        periods: surfaces.map(|surface| surface_parameter_periods(ir, surface)),
    };
    let seed = [first[0].u, first[0].v, first[1].u, first[1].v];
    let first_chord = Vector3::new(
        chart[1].x - chart[0].x,
        chart[1].y - chart[0].y,
        chart[1].z - chart[0].z,
    );
    let seed_tangent = intersection_parameter_tangent(ir, surfaces, seed, space, first_chord)?;
    let mut current = correct_intersection_parameters(
        ir,
        surfaces,
        seed,
        seed_tangent,
        space,
        fit_tolerance,
        1.0,
    )?;
    let first_point = model_surface_point_by_id(ir, surfaces[0], current[0], current[1])?;
    if point_distance(first_point, chart[0]) > fit_tolerance {
        return None;
    }
    let mut lanes = [
        vec![Point2::new(current[0], current[1])],
        vec![Point2::new(current[2], current[3])],
    ];

    for chart_pair in chart.windows(2) {
        let jacobian = intersection_parameter_jacobian(ir, surfaces, current, space)?;
        let chord = Vector3::new(
            chart_pair[1].x - chart_pair[0].x,
            chart_pair[1].y - chart_pair[0].y,
            chart_pair[1].z - chart_pair[0].z,
        );
        let tangent = intersection_parameter_tangent(ir, surfaces, current, space, chord)?;
        let spatial_tangent = Vector3::new(
            jacobian[0][0] * tangent[0] + jacobian[0][1] * tangent[1],
            jacobian[1][0] * tangent[0] + jacobian[1][1] * tangent[1],
            jacobian[2][0] * tangent[0] + jacobian[2][1] * tangent[1],
        );
        let target = [
            fit_parameters(
                surfaces[0],
                chart_pair[1],
                Some(Point2::new(current[0], current[1])),
            )?,
            fit_parameters(
                surfaces[1],
                chart_pair[1],
                Some(Point2::new(current[2], current[3])),
            )?,
        ];
        let mut predictor = [target[0].u, target[0].v, target[1].u, target[1].v];
        for (side, surface_periods) in space.periods.into_iter().enumerate() {
            for (coordinate, period) in surface_periods.into_iter().enumerate() {
                let index = side * 2 + coordinate;
                if let Some(period) = period {
                    predictor[index] =
                        lift_periodic_parameter(predictor[index], current[index], period);
                }
            }
        }
        let scale = (0..4)
            .map(|index| (predictor[index] - current[index]) * tangent[index])
            .sum::<f64>();
        if !scale.is_finite() || scale == 0.0 || dot_vector(spatial_tangent, chord) * scale <= 0.0 {
            return None;
        }
        let corrected = correct_intersection_parameters(
            ir,
            surfaces,
            predictor,
            tangent,
            space,
            fit_tolerance,
            scale,
        )?;
        let point = model_surface_point_by_id(ir, surfaces[0], corrected[0], corrected[1])?;
        if point_distance(point, chart_pair[1]) > fit_tolerance {
            return None;
        }
        current = corrected;
        lanes[0].push(Point2::new(current[0], current[1]));
        lanes[1].push(Point2::new(current[2], current[3]));
    }
    Some(lanes)
}

fn lift_periodic_parameter(value: f64, reference: f64, period: f64) -> f64 {
    value + ((reference - value) / period).round() * period
}

/// Return supported parameter periods while rejecting cyclic procedural support graphs.
pub(crate) fn surface_parameter_periods(ir: &CadIr, surface: &SurfaceId) -> [Option<f64>; 2] {
    surface_parameter_periods_inner(ir, surface, &mut BTreeSet::new())
}

fn surface_parameter_periods_inner(
    ir: &CadIr,
    surface: &SurfaceId,
    visiting: &mut BTreeSet<SurfaceId>,
) -> [Option<f64>; 2] {
    if !visiting.insert(surface.clone()) {
        return [None, None];
    }
    let Some(carrier) = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)
    else {
        visiting.remove(surface);
        return [None, None];
    };
    let periods = match &carrier.geometry {
        SurfaceGeometry::Cylinder { .. }
        | SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Sphere { .. } => [Some(std::f64::consts::TAU), None],
        SurfaceGeometry::Torus { .. } => [Some(std::f64::consts::TAU), Some(std::f64::consts::TAU)],
        SurfaceGeometry::Nurbs(nurbs) => {
            let period = |periodic: bool, knots: &[f64], degree: u32, count: u32| {
                periodic.then(|| {
                    let degree = usize::try_from(degree).ok()?;
                    let count = usize::try_from(count).ok()?;
                    let period = knots.get(count)? - knots.get(degree)?;
                    (period.is_finite() && period > 0.0).then_some(period)
                })?
            };
            [
                period(
                    nurbs.u_periodic,
                    &nurbs.u_knots,
                    nurbs.u_degree,
                    nurbs.u_count,
                ),
                period(
                    nurbs.v_periodic,
                    &nurbs.v_knots,
                    nurbs.v_degree,
                    nurbs.v_count,
                ),
            ]
        }
        SurfaceGeometry::Procedural { construction } => ir
            .model
            .procedural_surfaces
            .iter()
            .find(|candidate| &candidate.id == construction && &candidate.surface == surface)
            .and_then(|procedural| match &procedural.definition {
                ProceduralSurfaceDefinition::Offset { support, .. } => {
                    Some(surface_parameter_periods_inner(ir, support, visiting))
                }
                _ => None,
            })
            .unwrap_or([None, None]),
        _ => [None, None],
    };
    visiting.remove(surface);
    periods
}

fn correct_intersection_parameters(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    predictor: [f64; 4],
    tangent: [f64; 4],
    space: IntersectionParameterSpace,
    fit_tolerance: f64,
    scale: f64,
) -> Option<[f64; 4]> {
    let mut corrected = predictor;
    clamp_intersection_parameters(&mut corrected, space);
    for _ in 0..32 {
        let first = model_surface_point_by_id(ir, surfaces[0], corrected[0], corrected[1])?;
        let second = model_surface_point_by_id(ir, surfaces[1], corrected[2], corrected[3])?;
        let residual = [
            first.x - second.x,
            first.y - second.y,
            first.z - second.z,
            (0..4)
                .map(|index| (corrected[index] - predictor[index]) * tangent[index])
                .sum(),
        ];
        let equality_error = residual[..3]
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        if equality_error <= fit_tolerance * 1.0e-6
            && residual[3].abs() <= 1.0e-11 * (1.0 + scale.abs())
        {
            return Some(corrected);
        }
        let jacobian = intersection_parameter_jacobian(ir, surfaces, corrected, space)?;
        let matrix = [jacobian[0], jacobian[1], jacobian[2], tangent];
        let step = solve_4x4(matrix, residual.map(|value| -value))?;
        for index in 0..4 {
            corrected[index] += step[index];
        }
        clamp_intersection_parameters(&mut corrected, space);
    }
    None
}

#[derive(Clone, Copy)]
struct IntersectionParameterSpace {
    domains: [Option<([f64; 2], [f64; 2])>; 2],
    periods: [[Option<f64>; 2]; 2],
}

fn intersection_parameter_tangent(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    parameters: [f64; 4],
    space: IntersectionParameterSpace,
    chord: Vector3,
) -> Option<[f64; 4]> {
    let jacobian = intersection_parameter_jacobian(ir, surfaces, parameters, space)?;
    if let Some(tangent) = null_vector_3x4(jacobian) {
        return Some(tangent);
    }
    let chord = unit_vector(chord)?;
    let derivatives = [
        [
            Vector3::new(jacobian[0][0], jacobian[1][0], jacobian[2][0]),
            Vector3::new(jacobian[0][1], jacobian[1][1], jacobian[2][1]),
        ],
        [
            Vector3::new(-jacobian[0][2], -jacobian[1][2], -jacobian[2][2]),
            Vector3::new(-jacobian[0][3], -jacobian[1][3], -jacobian[2][3]),
        ],
    ];
    let mut tangent = [0.0; 4];
    for side in 0..2 {
        let (u, v) = least_squares_step(derivatives[side][0], derivatives[side][1], chord)?;
        let mapped = unit_vector(Vector3::new(
            derivatives[side][0].x * u + derivatives[side][1].x * v,
            derivatives[side][0].y * u + derivatives[side][1].y * v,
            derivatives[side][0].z * u + derivatives[side][1].z * v,
        ))?;
        if dot_vector(mapped, chord) < 1.0 - 1.0e-8 {
            return None;
        }
        tangent[side * 2] = u;
        tangent[side * 2 + 1] = v;
    }
    let norm = tangent
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    (norm.is_finite() && norm > 1.0e-14).then(|| tangent.map(|value| value / norm))
}

fn intersection_parameter_jacobian(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    parameters: [f64; 4],
    space: IntersectionParameterSpace,
) -> Option<[[f64; 4]; 3]> {
    let pairs = [
        Point2::new(parameters[0], parameters[1]),
        Point2::new(parameters[2], parameters[3]),
    ];
    let derivatives = std::array::from_fn(|side| {
        let u_step =
            parameter_derivative_step(pairs[side].u, space.domains[side].map(|value| value.0));
        let v_step =
            parameter_derivative_step(pairs[side].v, space.domains[side].map(|value| value.1));
        Some([
            model_surface_derivative(
                ir,
                surfaces[side],
                pairs[side],
                u_step,
                true,
                space.domains[side],
                space.periods[side],
            )?,
            model_surface_derivative(
                ir,
                surfaces[side],
                pairs[side],
                v_step,
                false,
                space.domains[side],
                space.periods[side],
            )?,
        ])
    });
    let [Some(first), Some(second)] = derivatives else {
        return None;
    };
    Some([
        [first[0].x, first[1].x, -second[0].x, -second[1].x],
        [first[0].y, first[1].y, -second[0].y, -second[1].y],
        [first[0].z, first[1].z, -second[0].z, -second[1].z],
    ])
}

fn clamp_intersection_parameters(parameters: &mut [f64; 4], space: IntersectionParameterSpace) {
    for side in 0..2 {
        let mut pair = Point2::new(parameters[side * 2], parameters[side * 2 + 1]);
        clamp_surface_parameters_with_periods(&mut pair, space.domains[side], space.periods[side]);
        parameters[side * 2] = pair.u;
        parameters[side * 2 + 1] = pair.v;
    }
}

fn clamp_surface_parameters_with_periods(
    parameters: &mut Point2,
    domain: Option<([f64; 2], [f64; 2])>,
    periods: [Option<f64>; 2],
) {
    if let Some((u_domain, v_domain)) = domain {
        if periods[0].is_none() {
            parameters.u = parameters.u.clamp(u_domain[0], u_domain[1]);
        }
        if periods[1].is_none() {
            parameters.v = parameters.v.clamp(v_domain[0], v_domain[1]);
        }
    }
}

fn determinant_3x3(matrix: [[f64; 3]; 3]) -> f64 {
    matrix[0][0] * (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1])
        - matrix[0][1] * (matrix[1][0] * matrix[2][2] - matrix[1][2] * matrix[2][0])
        + matrix[0][2] * (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0])
}

fn null_vector_3x4(matrix: [[f64; 4]; 3]) -> Option<[f64; 4]> {
    let mut vector = [0.0; 4];
    for (omitted, component) in vector.iter_mut().enumerate() {
        let minor = std::array::from_fn(|row| {
            let mut column = 0;
            std::array::from_fn(|_| {
                while column == omitted {
                    column += 1;
                }
                let value = matrix[row][column];
                column += 1;
                value
            })
        });
        *component = if omitted % 2 == 0 { 1.0 } else { -1.0 } * determinant_3x3(minor);
    }
    let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    (norm.is_finite() && norm > 1.0e-14).then(|| vector.map(|value| value / norm))
}

fn solve_4x4(mut matrix: [[f64; 4]; 4], mut rhs: [f64; 4]) -> Option<[f64; 4]> {
    for pivot in 0..4 {
        let row = (pivot..4).max_by(|first, second| {
            matrix[*first][pivot]
                .abs()
                .total_cmp(&matrix[*second][pivot].abs())
        })?;
        if !matrix[row][pivot].is_finite() || matrix[row][pivot].abs() <= 1.0e-14 {
            return None;
        }
        matrix.swap(pivot, row);
        rhs.swap(pivot, row);
        let pivot_row = matrix[pivot];
        for row in pivot + 1..4 {
            let factor = matrix[row][pivot] / matrix[pivot][pivot];
            for (value, pivot_value) in matrix[row][pivot..].iter_mut().zip(&pivot_row[pivot..]) {
                *value -= factor * pivot_value;
            }
            rhs[row] -= factor * rhs[pivot];
        }
    }
    let mut solution = [0.0; 4];
    for row in (0..4).rev() {
        let known = (row + 1..4)
            .map(|column| matrix[row][column] * solution[column])
            .sum::<f64>();
        solution[row] = (rhs[row] - known) / matrix[row][row];
    }
    solution
        .iter()
        .all(|value| value.is_finite())
        .then_some(solution)
}

fn least_squares_step(du: Vector3, dv: Vector3, residual: Vector3) -> Option<(f64, f64)> {
    let dot =
        |left: Vector3, right: Vector3| left.x * right.x + left.y * right.y + left.z * right.z;
    let du_squared = dot(du, du);
    let mixed = dot(du, dv);
    let dv_squared = dot(dv, dv);
    let determinant = du_squared * dv_squared - mixed * mixed;
    if !determinant.is_finite()
        || determinant.abs() <= f64::EPSILON * du_squared.max(dv_squared).powi(2)
    {
        return None;
    }
    let du_residual = dot(du, residual);
    let dv_residual = dot(dv, residual);
    Some((
        (dv_squared * du_residual - mixed * dv_residual) / determinant,
        (du_squared * dv_residual - mixed * du_residual) / determinant,
    ))
}

pub(crate) fn nurbs_parameters(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
) -> Option<Point2> {
    let seed = seed.filter(|seed| seed.u.is_finite() && seed.v.is_finite());
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    let u_domain = [
        *surface.u_knots.get(u_degree)?,
        *surface.u_knots.get(u_count)?,
    ];
    let v_domain = [
        *surface.v_knots.get(v_degree)?,
        *surface.v_knots.get(v_count)?,
    ];
    if u_domain[0] >= u_domain[1] || v_domain[0] >= v_domain[1] {
        return None;
    }
    let squared_distance = |candidate: Point3| point_distance(candidate, point).powi(2);
    let mut coarse = vec![None; 81];
    for ui in 0..=8 {
        for vi in 0..=8 {
            let ui_value = f64::from(u32::try_from(ui).ok()?);
            let vi_value = f64::from(u32::try_from(vi).ok()?);
            let parameters = Point2::new(
                u_domain[0] + (u_domain[1] - u_domain[0]) * ui_value / 8.0,
                v_domain[0] + (v_domain[1] - v_domain[0]) * vi_value / 8.0,
            );
            let Some(position) =
                cadmpeg_ir::eval::nurbs_surface_point(surface, parameters.u, parameters.v)
            else {
                continue;
            };
            coarse[ui * 9 + vi] = Some((parameters, squared_distance(position)));
        }
    }
    let mut starts = Vec::new();
    if let Some(seed) = seed {
        starts.push(seed);
    }
    for ui in 0..=8 {
        for vi in 0..=8 {
            let index = ui * 9 + vi;
            let Some((parameters, distance)) = coarse[index] else {
                continue;
            };
            let local_minimum = ui.saturating_sub(1)..=(ui + 1).min(8);
            if local_minimum
                .flat_map(|neighbor_u| {
                    (vi.saturating_sub(1)..=(vi + 1).min(8))
                        .map(move |neighbor_v| neighbor_u * 9 + neighbor_v)
                })
                .all(|neighbor| coarse[neighbor].is_none_or(|(_, value)| distance <= value))
            {
                starts.push(parameters);
            }
        }
    }
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    let mut best_seed_distance = f64::INFINITY;
    for start in starts {
        let Some(parameters) = refine_nurbs_surface_parameters(
            surface,
            point,
            start,
            u_domain,
            v_domain,
            &squared_distance,
        ) else {
            continue;
        };
        let Some(position) =
            cadmpeg_ir::eval::nurbs_surface_point(surface, parameters.u, parameters.v)
        else {
            continue;
        };
        let distance = squared_distance(position);
        let seed_distance = seed.map_or(parameters.u.abs() + parameters.v.abs(), |seed| {
            (parameters.u - seed.u).hypot(parameters.v - seed.v)
        });
        let same_point = (distance - best_distance).abs()
            <= f64::EPSILON * 64.0 * distance.abs().max(best_distance.abs()).max(1.0);
        if distance < best_distance && !same_point
            || same_point && seed_distance < best_seed_distance
        {
            best = Some(parameters);
            best_distance = distance;
            best_seed_distance = seed_distance;
        }
    }
    best
}

fn refine_nurbs_surface_parameters(
    surface: &NurbsSurface,
    point: Point3,
    mut parameters: Point2,
    u_domain: [f64; 2],
    v_domain: [f64; 2],
    squared_distance: &impl Fn(Point3) -> f64,
) -> Option<Point2> {
    parameters.u = parameters.u.clamp(u_domain[0], u_domain[1]);
    parameters.v = parameters.v.clamp(v_domain[0], v_domain[1]);
    for _ in 0..32 {
        let position = cadmpeg_ir::eval::nurbs_surface_point(surface, parameters.u, parameters.v)?;
        let residual = Vector3::new(
            position.x - point.x,
            position.y - point.y,
            position.z - point.z,
        );
        let partials = nurbs_surface_partials(surface, parameters.u, parameters.v)?;
        let (du, dv) = (partials.du, partials.dv);
        let dot =
            |left: Vector3, right: Vector3| left.x * right.x + left.y * right.y + left.z * right.z;
        let du_squared = dot(du, du);
        let mixed = dot(du, dv);
        let dv_squared = dot(dv, dv);
        let determinant = du_squared * dv_squared - mixed * mixed;
        if !determinant.is_finite()
            || determinant.abs() <= f64::EPSILON * du_squared.max(dv_squared).powi(2)
        {
            break;
        }
        let du_residual = dot(du, residual);
        let dv_residual = dot(dv, residual);
        let step = Point2::new(
            (dv_squared * du_residual - mixed * dv_residual) / determinant,
            (du_squared * dv_residual - mixed * du_residual) / determinant,
        );
        let current_distance = squared_distance(position);
        let mut scale = 1.0;
        let mut accepted = None;
        for _ in 0..16 {
            let candidate = Point2::new(
                (parameters.u - scale * step.u).clamp(u_domain[0], u_domain[1]),
                (parameters.v - scale * step.v).clamp(v_domain[0], v_domain[1]),
            );
            let candidate_position =
                cadmpeg_ir::eval::nurbs_surface_point(surface, candidate.u, candidate.v)?;
            if squared_distance(candidate_position) <= current_distance {
                accepted = Some(candidate);
                break;
            }
            scale *= 0.5;
        }
        let Some(candidate) = accepted else {
            break;
        };
        parameters = candidate;
        if scale * step.u.abs() <= 1.0e-12 * (1.0 + parameters.u.abs())
            && scale * step.v.abs() <= 1.0e-12 * (1.0 + parameters.v.abs())
        {
            break;
        }
    }
    Some(parameters)
}

fn point_distance(first: Point3, second: Point3) -> f64 {
    ((first.x - second.x).powi(2) + (first.y - second.y).powi(2) + (first.z - second.z).powi(2))
        .sqrt()
}

fn intersection_side(
    ir: &CadIr,
    surfaces_by_xmt: &BTreeMap<u32, SurfaceId>,
    surface_xmt: u32,
    uv: Option<(&[[f64; 2]], &[f64])>,
) -> IntcurveSupportSide {
    let surface = surfaces_by_xmt.get(&surface_xmt).cloned();
    let pcurve = surface.as_ref().and_then(|surface_id| {
        let geometry = ir
            .model
            .surfaces
            .iter()
            .find(|candidate| &candidate.id == surface_id)
            .map(|surface| &surface.geometry)?;
        let (uv, parameters) = uv?;
        let control_points = uv
            .iter()
            .map(|pair| surface_parameters(geometry, *pair))
            .collect::<Option<Vec<_>>>()?;
        Some(PcurveGeometry::Nurbs {
            degree: 1,
            knots: linear_knots(parameters),
            control_points,
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

fn surface_parameters(surface: &SurfaceGeometry, uv: [f64; 2]) -> Option<Point2> {
    let point = match surface {
        SurfaceGeometry::Plane { .. } => Point2::new(uv[0] * 1000.0, uv[1] * 1000.0),
        SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. } => {
            Point2::new(uv[0], uv[1] * 1000.0)
        }
        SurfaceGeometry::Sphere { .. }
        | SurfaceGeometry::Torus { .. }
        | SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Unknown { .. } => Point2::new(uv[0], uv[1]),
        SurfaceGeometry::Transformed { basis, .. } => return surface_parameters(basis, uv),
    };
    [point.u, point.v]
        .into_iter()
        .all(f64::is_finite)
        .then_some(point)
}

fn normalize_pcurve_parameters(
    pcurve: &mut PcurveGeometry,
    surface: &SurfaceGeometry,
) -> Option<()> {
    match pcurve {
        PcurveGeometry::Line { origin, direction } => {
            let end = Point2::new(origin.u + direction.u, origin.v + direction.v);
            let converted_origin = surface_parameters(surface, [origin.u, origin.v])?;
            let converted_end = surface_parameters(surface, [end.u, end.v])?;
            *origin = converted_origin;
            *direction = Point2::new(
                converted_end.u - converted_origin.u,
                converted_end.v - converted_origin.v,
            );
        }
        PcurveGeometry::Nurbs { control_points, .. } => {
            let converted = control_points
                .iter()
                .map(|point| surface_parameters(surface, [point.u, point.v]))
                .collect::<Option<Vec<_>>>()?;
            *control_points = converted;
        }
        _ => {}
    }
    Some(())
}

// The parameters are the per-stream lookup tables produced by the decode pass;
// bundling them into a struct would only rename the same lookup tables.
#[allow(clippy::too_many_arguments)]
fn emit_topology(
    ir: &mut CadIr,
    stream_index: usize,
    graph: &Graph,
    points: &BTreeMap<u32, PointId>,
    surfaces: &BTreeMap<u32, SurfaceId>,
    curves: &BTreeMap<u32, CurveId>,
    pcurves: &BTreeMap<u32, PcurveId>,
    pcurve_supports: &BTreeMap<u32, SurfaceId>,
    trim_ranges: &BTreeMap<u32, [f64; 2]>,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) {
    let prefix = format!("nx:s{stream_index}");
    let body_shape_shells = graph.body_shape_shells();
    let valid_face_xmts: BTreeSet<u32> = body_shape_shells
        .iter()
        .filter_map(|shell| graph.shell_face_xmts(shell))
        .flatten()
        .collect();
    let valid_loop_rings: BTreeMap<u32, Vec<u32>> = valid_face_xmts
        .iter()
        .filter_map(|face_xmt| graph.face_loop_rings(*face_xmt))
        .flatten()
        .collect();
    let valid_fin_xmts: BTreeSet<u32> = valid_loop_rings
        .values()
        .flat_map(|ring| ring.iter().copied())
        .collect();
    let valid_edge_xmts: BTreeSet<u32> = valid_fin_xmts
        .iter()
        .filter_map(|xmt| graph.get(17, *xmt)?.fin_fields().map(|fields| fields.edge))
        .collect();
    let valid_vertex_xmts: BTreeSet<u32> = valid_fin_xmts
        .iter()
        .flat_map(|xmt| {
            let fields = graph.get(17, *xmt).and_then(Node::fin_fields);
            let partner_vertex = fields
                .filter(|fields| fields.other > 1)
                .and_then(|fields| graph.get(17, fields.other))
                .and_then(Node::fin_fields)
                .map(|fields| fields.vertex);
            [fields.map(|fields| fields.vertex), partner_vertex]
                .into_iter()
                .flatten()
        })
        .filter(|xmt| *xmt > 1)
        .collect();
    let body_xmts: BTreeSet<_> = body_shape_shells
        .iter()
        .filter_map(|shell| shell.shell_fields().map(|fields| fields.body))
        .collect();
    let mut bodies = BTreeMap::new();
    for body_xmt in body_xmts {
        let id = BodyId(format!("{prefix}:body#{body_xmt}"));
        if let Some(node) = graph.get(12, body_xmt) {
            annotate_node(annotations, &id, source_stream, node, "BODY");
        } else if let Some(shell) = body_shape_shells.iter().find(|shell| {
            shell
                .shell_fields()
                .is_some_and(|fields| fields.body == body_xmt)
        }) {
            annotations
                .note(&id, source_stream, shell.pos as u64)
                .tag("UNRESOLVED_BODY_REFERENCE");
            annotations.exactness(&id, Exactness::Unknown);
        }
        bodies.insert(body_xmt, id.clone());
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

    let mut regions: BTreeMap<u32, (RegionId, BodyId)> = BTreeMap::new();
    let mut shells = BTreeMap::new();
    for node in body_shape_shells {
        let Some(fields) = node.shell_fields() else {
            continue;
        };
        let Some(body) = bodies.get(&fields.body).cloned() else {
            continue;
        };
        let region_id = if let Some((region, owner)) = regions.get(&fields.region) {
            if owner != &body {
                continue;
            }
            region.clone()
        } else {
            let region = RegionId(format!("{prefix}:region#{}", fields.region));
            if let Some(region_node) = graph.get(19, fields.region) {
                annotate_node(annotations, &region, source_stream, region_node, "REGION");
            } else {
                annotations
                    .note(&region, source_stream, node.pos as u64)
                    .tag("UNRESOLVED_REGION_REFERENCE");
                annotations.exactness(&region, Exactness::Unknown);
            }
            annotations.derived(&region, "body");
            ir.model.regions.push(Region {
                id: region.clone(),
                body: body.clone(),
                shells: Vec::new(),
            });
            if let Some(parent) = ir
                .model
                .bodies
                .iter_mut()
                .find(|candidate| candidate.id == body)
            {
                parent.regions.push(region.clone());
            }
            regions.insert(fields.region, (region.clone(), body.clone()));
            region
        };
        let shell_id = ShellId(format!("{prefix}:shell#{}", node.xmt));
        annotate_node(annotations, &shell_id, source_stream, node, "SHELL");
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        if let Some(parent) = ir
            .model
            .regions
            .iter_mut()
            .find(|candidate| candidate.id == region_id)
        {
            parent.shells.push(shell_id.clone());
        }
        shells.insert(node.xmt, shell_id);
    }

    let mut vertices = BTreeMap::new();
    for node in graph
        .of_kind(18)
        .filter(|node| valid_vertex_xmts.contains(&node.xmt))
    {
        let Some(fields) = node.vertex_fields() else {
            continue;
        };
        let Some(point) = points.get(&fields.point).cloned() else {
            continue;
        };
        let tolerance = decoded_tolerance(fields.tolerance);
        let vertex = VertexId(format!("{prefix}:vertex#{}", node.xmt));
        annotate_node(annotations, &vertex, source_stream, node, "VERTEX");
        if tolerance.is_some() {
            annotations.derived(&vertex, "tolerance");
        }
        ir.model.vertices.push(Vertex {
            id: vertex.clone(),
            point,
            tolerance,
        });
        vertices.insert(node.xmt, vertex.clone());
    }

    let mut edges = BTreeMap::new();
    for node in graph
        .of_kind(16)
        .filter(|node| valid_edge_xmts.contains(&node.xmt))
    {
        let Some(fields) = node.edge_fields() else {
            continue;
        };
        let Some(fin) = graph.get(17, fields.fin) else {
            continue;
        };
        let Some(fin_fields) = fin.fin_fields() else {
            continue;
        };
        let curve_xmt = [fields.curve, fin_fields.curve_xmt]
            .into_iter()
            .find(|xmt| *xmt > 1);
        let mut curve = curve_xmt.and_then(|xmt| curves.get(&xmt)).cloned();
        let mut param_range = curve_xmt.and_then(|xmt| trim_ranges.get(&xmt)).copied();
        if curve.is_none() {
            let lifted = curve_xmt
                .and_then(|xmt| pcurves.get(&xmt))
                .and_then(|pcurve_id| {
                    let pcurve = ir
                        .model
                        .pcurves
                        .iter()
                        .find(|pcurve| &pcurve.id == pcurve_id)?;
                    let surface = pcurve_supports.get(&curve_xmt?)?.clone();
                    let parameter_range = pcurve
                        .parameter_range
                        .or(param_range)
                        .or_else(|| pcurve_parameter_range(&pcurve.geometry))?;
                    let parameter_range = ordered_parameter_range(parameter_range)?;
                    Some((
                        surface,
                        pcurve.geometry.clone(),
                        parameter_range,
                        pcurve.fit_tolerance,
                    ))
                });
            if let Some((surface, pcurve, parameter_range, _fit_tolerance)) = lifted {
                let carrier = CurveId(format!("{prefix}:edge-parametric-curve#{}", node.xmt));
                let construction = ProceduralCurveId(format!(
                    "{prefix}:edge-parametric-construction#{}",
                    node.xmt
                ));
                annotations
                    .note(&carrier, source_stream, node.pos as u64)
                    .tag("PARAMETRIC_SURFACE_CURVE");
                annotations.derived(&carrier, "geometry");
                ir.model.curves.push(Curve {
                    id: carrier.clone(),
                    geometry: CurveGeometry::Procedural {
                        construction: construction.clone(),
                    },
                    source_object: None,
                });
                ir.model.procedural_curves.push(ProceduralCurve {
                    id: construction,
                    curve: carrier.clone(),
                    definition: ProceduralCurveDefinition::SurfaceCurve {
                        family: SurfaceCurveFamily::Parametric,
                        context: IntcurveSupportContext {
                            sides: [
                                IntcurveSupportSide {
                                    surface: Some(surface),
                                    pcurve: Some(pcurve),
                                    pcurve_parameter_range: None,
                                },
                                IntcurveSupportSide {
                                    surface: None,
                                    pcurve: None,
                                    pcurve_parameter_range: None,
                                },
                            ],
                            parameter_range,
                            discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                        },
                        tail: None,
                    },
                    // The pcurve carries this fit contract; this construction has no
                    // independent solved 3D cache to qualify.
                    cache_fit_tolerance: None,
                });
                curve = Some(carrier);
                param_range = None;
            }
        }
        let start = vertices.get(&fin_fields.vertex).cloned().or_else(|| {
            (fin_fields.vertex == 1
                && fin_fields.forward == fin.xmt
                && fin_fields.backward == fin.xmt)
                .then(|| {
                    synthesize_closed_edge_vertex(
                        ir,
                        annotations,
                        &prefix,
                        node,
                        curve.as_ref()?,
                        param_range,
                        source_stream,
                        decoded_tolerance(fields.tolerance),
                    )
                })
                .flatten()
        });
        let Some(start) = start else {
            continue;
        };
        let end_fin = if fin_fields.other > 1 {
            fin_fields.other
        } else {
            fin_fields.forward
        };
        let end = graph
            .get(17, end_fin)
            .and_then(Node::fin_fields)
            .and_then(|next| vertices.get(&next.vertex))
            .cloned()
            .unwrap_or_else(|| start.clone());
        let (mut start, mut end) = (start, end);
        let id = EdgeId(format!("{prefix}:edge#{}", node.xmt));
        annotate_node(annotations, &id, source_stream, node, "EDGE");
        if decoded_tolerance(fields.tolerance).is_some() {
            annotations.derived(&id, "tolerance");
        }
        if let (Some(carrier), Some(range)) = (&curve, param_range) {
            match orient_edge_range(
                ir,
                carrier,
                range,
                &start,
                &end,
                decoded_tolerance(fields.tolerance),
            ) {
                Some((oriented, reverse_edge)) => {
                    param_range = Some(oriented);
                    if reverse_edge {
                        std::mem::swap(&mut start, &mut end);
                    }
                }
                None => {
                    param_range = None;
                }
            }
        }
        ir.model.edges.push(Edge {
            id: id.clone(),
            curve,
            start,
            end,
            param_range,
            tolerance: decoded_tolerance(fields.tolerance),
        });
        edges.insert(node.xmt, id);
    }

    let mut faces = BTreeMap::new();
    for node in graph
        .of_kind(14)
        .filter(|node| valid_face_xmts.contains(&node.xmt))
    {
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
        if decoded_tolerance(fields.tolerance).is_some() {
            annotations.derived(&id, "tolerance");
        }
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
    for &loop_xmt in valid_loop_rings.keys() {
        let ring_resolves = valid_loop_rings[&loop_xmt].iter().all(|fin_xmt| {
            graph
                .get(17, *fin_xmt)
                .and_then(Node::fin_fields)
                .is_some_and(|fields| edges.contains_key(&fields.edge))
        });
        if !ring_resolves {
            continue;
        }
        let Some(node) = graph.get(15, loop_xmt) else {
            continue;
        };
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

    let fin_ids: BTreeMap<u32, CoedgeId> = valid_fin_xmts
        .iter()
        .filter(|xmt| {
            graph
                .get(17, **xmt)
                .and_then(Node::fin_fields)
                .is_some_and(|fields| loops.contains_key(&fields.loop_xmt))
        })
        .map(|xmt| (*xmt, CoedgeId(format!("{prefix}:fin#{xmt}"))))
        .collect();
    let intersection_pcurves: BTreeMap<_, _> = ir
        .model
        .procedural_curves
        .iter()
        .filter_map(|procedural| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            Some(context.sides.iter().filter_map(move |side| {
                Some((
                    (procedural.curve.clone(), side.surface.clone()?),
                    (
                        side.pcurve.clone()?,
                        context.parameter_range,
                        procedural.cache_fit_tolerance,
                    ),
                ))
            }))
        })
        .flatten()
        .collect();
    for &fin_xmt in fin_ids.keys() {
        let Some(node) = graph.get(17, fin_xmt) else {
            continue;
        };
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
            .expect("validated FIN ring resolves forward link");
        let previous = fin_ids
            .get(&fields.backward)
            .cloned()
            .expect("validated FIN ring resolves backward link");
        let partner = fin_ids.get(&fields.other).cloned();
        let radial_next = partner.clone().unwrap_or_else(|| id.clone());
        let support = graph
            .get(15, fields.loop_xmt)
            .and_then(Node::loop_fields)
            .and_then(|loop_| graph.get(14, loop_.face))
            .and_then(Node::face_fields)
            .and_then(|face| surfaces.get(&face.surface))
            .cloned();
        let mut pcurve = pcurves.get(&fields.curve_xmt).cloned().filter(|id| {
            let Some((carrier, support)) = ir
                .model
                .pcurves
                .iter()
                .find(|carrier| &carrier.id == id)
                .zip(support.as_ref())
            else {
                return false;
            };
            pcurve_matches_edge_range(
                ir,
                &edge,
                support,
                &carrier.geometry,
                carrier.parameter_range,
                carrier.fit_tolerance,
            )
        });
        if pcurve.is_none() {
            let carrier = ir
                .model
                .edges
                .iter()
                .find(|candidate| candidate.id == edge)
                .and_then(|edge| edge.curve.clone());
            if let Some((_support, geometry, parameter_range, fit_tolerance)) = carrier
                .zip(support)
                .and_then(|key| {
                    intersection_pcurves
                        .get(&key)
                        .cloned()
                        .map(|value| (key.1, value.0, value.1, value.2))
                })
                .filter(|(support, geometry, _, fit_tolerance)| {
                    pcurve_matches_edge(ir, &edge, support, geometry, *fit_tolerance)
                })
            {
                let pcurve_id = PcurveId(format!("{prefix}:intersection-pcurve#{fin_xmt}"));
                annotations
                    .note(&pcurve_id, source_stream, node.pos as u64)
                    .tag("INTERSECTION_PCURVE");
                annotations.derived(&pcurve_id, "geometry");
                annotations.derived(&pcurve_id, "parameter_range");
                if fit_tolerance.is_some() {
                    annotations.derived(&pcurve_id, "fit_tolerance");
                }
                ir.model.pcurves.push(Pcurve {
                    id: pcurve_id.clone(),
                    geometry,
                    wrapper_reversed: None,
                    native_tail_flags: None,
                    parameter_range: Some(parameter_range),
                    fit_tolerance,
                });
                pcurve = Some(pcurve_id);
            }
        }
        ir.model.coedges.push(Coedge {
            id: id.clone(),
            owner_loop: loop_id.clone(),
            edge,
            next,
            previous,
            radial_next,
            sense: sense(Some(fields.sense)),
            pcurves: pcurve
                .into_iter()
                .map(|pcurve| cadmpeg_ir::topology::PcurveUse {
                    pcurve,
                    isoparametric: None,
                    parameter_range: None,
                })
                .collect(),
            use_curve: None,
            use_curve_parameter_range: None,
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

    attach_tolerant_edge_intersections(ir, graph, &edges, &prefix, source_stream, annotations);
    complete_intersection_supports_from_edge_incidence(ir);
    complete_intersection_pcurves_from_coedge_incidence(ir);
    complete_isoparametric_intersection_pcurves(ir);
    complete_intersection_pcurves_from_opposite_charts(ir);

    let owned_edges: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.clone())
        .collect();
    let candidate_edges: BTreeSet<_> = edges.into_values().collect();
    ir.model
        .edges
        .retain(|edge| !candidate_edges.contains(&edge.id) || owned_edges.contains(&edge.id));
    let retained_vertices: BTreeSet<_> = ir
        .model
        .edges
        .iter()
        .flat_map(|edge| [edge.start.clone(), edge.end.clone()])
        .collect();
    ir.model.vertices.retain(|vertex| {
        !vertex.id.0.starts_with(&prefix) || retained_vertices.contains(&vertex.id)
    });
}

fn pcurve_parameter_range(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    let PcurveGeometry::Nurbs { knots, .. } = geometry else {
        return None;
    };
    ordered_parameter_range([*knots.first()?, *knots.last()?])
}

fn ordered_parameter_range(mut range: [f64; 2]) -> Option<[f64; 2]> {
    if !range.iter().all(|value| value.is_finite()) || range[0] == range[1] {
        return None;
    }
    if range[0] > range[1] {
        range.swap(0, 1);
    }
    Some(range)
}

pub(crate) fn complete_intersection_supports_from_edge_incidence(ir: &mut CadIr) {
    let loop_faces = ir
        .model
        .loops
        .iter()
        .map(|loop_| (loop_.id.clone(), loop_.face.clone()))
        .collect::<BTreeMap<_, _>>();
    let face_surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| (face.id.clone(), face.surface.clone()))
        .collect::<BTreeMap<_, _>>();
    let edge_curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| Some((edge.id.clone(), edge.curve.clone()?)))
        .collect::<BTreeMap<_, _>>();
    let mut incident_surfaces = BTreeMap::<CurveId, Vec<SurfaceId>>::new();
    for coedge in &ir.model.coedges {
        let Some(curve) = edge_curves.get(&coedge.edge) else {
            continue;
        };
        let Some(surface) = loop_faces
            .get(&coedge.owner_loop)
            .and_then(|face| face_surfaces.get(face))
        else {
            continue;
        };
        let surfaces = incident_surfaces.entry(curve.clone()).or_default();
        if !surfaces.contains(surface) {
            surfaces.push(surface.clone());
        }
    }

    for procedural in &mut ir.model.procedural_curves {
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        let missing = context
            .sides
            .iter()
            .enumerate()
            .filter_map(|(index, side)| side.surface.is_none().then_some(index))
            .collect::<Vec<_>>();
        if missing.len() != 1 {
            continue;
        }
        let Some(incident) = incident_surfaces.get(&procedural.curve) else {
            continue;
        };
        let candidates = incident
            .iter()
            .filter(|surface| {
                !context
                    .sides
                    .iter()
                    .any(|side| side.surface.as_ref() == Some(surface))
            })
            .collect::<Vec<_>>();
        let [surface] = candidates.as_slice() else {
            continue;
        };
        context.sides[missing[0]].surface = Some((*surface).clone());
    }
}

pub(crate) fn complete_intersection_pcurves_from_coedge_incidence(ir: &mut CadIr) {
    let loop_faces = ir
        .model
        .loops
        .iter()
        .map(|loop_| (loop_.id.clone(), loop_.face.clone()))
        .collect::<BTreeMap<_, _>>();
    let face_surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| (face.id.clone(), face.surface.clone()))
        .collect::<BTreeMap<_, _>>();
    let edge_curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| Some((edge.id.clone(), edge.curve.clone()?)))
        .collect::<BTreeMap<_, _>>();
    let mut incident_pcurves = BTreeMap::<(CurveId, SurfaceId), Vec<PcurveId>>::new();
    for coedge in &ir.model.coedges {
        let Some(curve) = edge_curves.get(&coedge.edge) else {
            continue;
        };
        let Some(surface) = loop_faces
            .get(&coedge.owner_loop)
            .and_then(|face| face_surfaces.get(face))
        else {
            continue;
        };
        let pcurves = incident_pcurves
            .entry((curve.clone(), surface.clone()))
            .or_default();
        for pcurve in &coedge.pcurves {
            if !pcurves.contains(&pcurve.pcurve) {
                pcurves.push(pcurve.pcurve.clone());
            }
        }
    }

    for procedural in &mut ir.model.procedural_curves {
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        for side in &mut context.sides {
            if side.pcurve.is_some() {
                continue;
            }
            let Some(surface) = &side.surface else {
                continue;
            };
            let Some([pcurve]) = incident_pcurves
                .get(&(procedural.curve.clone(), surface.clone()))
                .map(Vec::as_slice)
            else {
                continue;
            };
            let Some(carrier) = ir
                .model
                .pcurves
                .iter()
                .find(|carrier| &carrier.id == pcurve)
            else {
                continue;
            };
            side.pcurve = Some(carrier.geometry.clone());
        }
    }
}

pub(crate) fn complete_intersection_pcurves_from_opposite_charts(ir: &mut CadIr) {
    let edge_tolerances = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| {
            Some((
                edge.curve.clone()?,
                edge.tolerance
                    .filter(|value| value.is_finite() && *value >= 0.0)?,
            ))
        })
        .fold(
            BTreeMap::<CurveId, f64>::new(),
            |mut values, (curve, tolerance)| {
                values
                    .entry(curve)
                    .and_modify(|current| *current = current.min(tolerance))
                    .or_insert(tolerance);
                values
            },
        );
    let replacements = ir
        .model
        .procedural_curves
        .iter()
        .filter_map(|procedural| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            let missing = context
                .sides
                .each_ref()
                .map(|side| pcurve_requires_completion(side.pcurve.as_ref()));
            let target = match missing {
                [true, false] => 0,
                [false, true] => 1,
                _ => return None,
            };
            let source = 1 - target;
            let source_surface = context.sides[source].surface.as_ref()?;
            let source_pcurve = context.sides[source].pcurve.as_ref()?;
            let target_surface = context.sides[target].surface.as_ref()?;
            let tolerance = procedural
                .cache_fit_tolerance
                .or_else(|| edge_tolerances.get(&procedural.curve).copied())?;
            let tolerance = blend_spine_cache_fit_tolerance(ir, target_surface, tolerance);
            let pcurve = transfer_intersection_pcurve(
                ir,
                &procedural.curve,
                source_surface,
                source_pcurve,
                target_surface,
                context.parameter_range,
                tolerance,
            )?;
            Some((procedural.id.clone(), target, pcurve, tolerance))
        })
        .collect::<Vec<_>>();
    for (procedural_id, side, pcurve, tolerance) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
            context.sides[side].pcurve = Some(pcurve);
            procedural.cache_fit_tolerance =
                Some(procedural.cache_fit_tolerance.unwrap_or(0.0).max(tolerance));
        }
    }
}

pub(crate) fn complete_isoparametric_intersection_pcurves(ir: &mut CadIr) {
    let vertex_points = ir
        .model
        .vertices
        .iter()
        .filter_map(|vertex| {
            let point = ir
                .model
                .points
                .iter()
                .find(|point| point.id == vertex.point)?;
            Some((vertex.id.clone(), point.position))
        })
        .collect::<BTreeMap<_, _>>();
    let replacements = ir
        .model
        .procedural_curves
        .iter()
        .filter_map(|procedural| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            if !context
                .sides
                .iter()
                .all(|side| pcurve_requires_completion(side.pcurve.as_ref()))
            {
                return None;
            }
            let [Some(first_surface), Some(second_surface)] =
                context.sides.each_ref().map(|side| side.surface.as_ref())
            else {
                return None;
            };
            let edges = ir
                .model
                .edges
                .iter()
                .filter(|edge| edge.curve.as_ref() == Some(&procedural.curve))
                .collect::<Vec<_>>();
            let [edge] = edges.as_slice() else {
                return None;
            };
            let tolerance = edge
                .tolerance
                .filter(|value| value.is_finite() && *value >= 0.0)?;
            let endpoints = [
                *vertex_points.get(&edge.start)?,
                *vertex_points.get(&edge.end)?,
            ];
            let candidates = [first_surface, second_surface].map(|surface| {
                isoparametric_boundary_pcurve(
                    ir,
                    surface,
                    endpoints,
                    context.parameter_range,
                    tolerance,
                )
            });
            let pcurves = match candidates {
                [Some(first), Some(second)] => coincident_pcurve_pair(
                    ir,
                    [first_surface, second_surface],
                    [&first, &second],
                    context.parameter_range,
                    tolerance,
                )
                .then_some([first, second])?,
                [Some(first), None] => [
                    first.clone(),
                    transfer_intersection_pcurve(
                        ir,
                        &procedural.curve,
                        first_surface,
                        &first,
                        second_surface,
                        context.parameter_range,
                        tolerance,
                    )?,
                ],
                [None, Some(second)] => [
                    transfer_intersection_pcurve(
                        ir,
                        &procedural.curve,
                        second_surface,
                        &second,
                        first_surface,
                        context.parameter_range,
                        tolerance,
                    )?,
                    second,
                ],
                [None, None] => return None,
            };
            Some((procedural.id.clone(), pcurves, tolerance))
        })
        .collect::<Vec<_>>();
    for (procedural_id, pcurves, tolerance) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        if context
            .sides
            .iter()
            .all(|side| pcurve_requires_completion(side.pcurve.as_ref()))
        {
            for (side, pcurve) in context.sides.iter_mut().zip(pcurves) {
                side.pcurve = Some(pcurve);
            }
            procedural.cache_fit_tolerance = Some(tolerance);
        }
    }
}

fn isoparametric_boundary_pcurve(
    ir: &CadIr,
    surface: &SurfaceId,
    endpoints: [Point3; 2],
    range: [f64; 2],
    tolerance: f64,
) -> Option<PcurveGeometry> {
    (range[0].is_finite() && range[1].is_finite() && range[0] < range[1]).then_some(())?;
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let SurfaceGeometry::Nurbs(nurbs) = &carrier.geometry else {
        return None;
    };
    let domain = surface_parameter_domain(ir, surface)?;
    let parameters = [
        nurbs_parameters(nurbs, endpoints[0], None)?,
        nurbs_parameters(nurbs, endpoints[1], None)?,
    ];
    for index in 0..2 {
        let point =
            cadmpeg_ir::eval::nurbs_surface_point(nurbs, parameters[index].u, parameters[index].v)?;
        if point_distance(point, endpoints[index]) > tolerance {
            return None;
        }
    }
    let axes = [
        ([parameters[0].u, parameters[1].u], domain.0),
        ([parameters[0].v, parameters[1].v], domain.1),
    ];
    let candidates = axes
        .into_iter()
        .enumerate()
        .filter_map(|(constant_axis, (values, axis_domain))| {
            let scale = (axis_domain[1] - axis_domain[0]).abs().max(1.0);
            let parameter_tolerance = 1.0e-8 * scale;
            let boundary = axis_domain.into_iter().find(|boundary| {
                values
                    .iter()
                    .all(|value| (*value - *boundary).abs() <= parameter_tolerance)
            })?;
            let varying = if constant_axis == 0 {
                [parameters[0].v, parameters[1].v]
            } else {
                [parameters[0].u, parameters[1].u]
            };
            ((varying[1] - varying[0]).abs() > parameter_tolerance).then(|| {
                let delta = (varying[1] - varying[0]) / (range[1] - range[0]);
                let (origin, direction) = if constant_axis == 0 {
                    (
                        Point2::new(boundary, varying[0] - delta * range[0]),
                        Point2::new(0.0, delta),
                    )
                } else {
                    (
                        Point2::new(varying[0] - delta * range[0], boundary),
                        Point2::new(delta, 0.0),
                    )
                };
                PcurveGeometry::Line { origin, direction }
            })
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn coincident_pcurve_pair(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    pcurves: [&PcurveGeometry; 2],
    range: [f64; 2],
    tolerance: f64,
) -> bool {
    (0..=32).all(|index| {
        let fraction = f64::from(index) / 32.0;
        let parameter = range[0] + fraction * (range[1] - range[0]);
        let points = [0usize, 1usize].map(|side| {
            let uv = pcurve_uv(pcurves[side], parameter)?;
            decoded_surface_point(ir, surfaces[side], uv.u, uv.v)
        });
        matches!(points, [Some(first), Some(second)] if point_distance(first, second) <= tolerance)
    })
}

fn transfer_intersection_pcurve(
    ir: &CadIr,
    curve: &CurveId,
    source_surface: &SurfaceId,
    source_pcurve: &PcurveGeometry,
    target_surface: &SurfaceId,
    parameter_range: [f64; 2],
    tolerance: f64,
) -> Option<PcurveGeometry> {
    (parameter_range[0].is_finite()
        && parameter_range[1].is_finite()
        && parameter_range[0] < parameter_range[1]
        && tolerance.is_finite()
        && tolerance >= 0.0)
        .then_some(())?;
    let first = transferred_pcurve_sample(
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        parameter_range[0],
        None,
        tolerance,
    )?;
    let last = transferred_pcurve_sample(
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        parameter_range[1],
        Some(first.1),
        tolerance,
    )?;
    let mut samples = vec![first];
    append_transferred_pcurve_segment(
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        first,
        last,
        tolerance,
        0,
        &mut samples,
    )?;
    Some(PcurveGeometry::Nurbs {
        degree: 1,
        knots: linear_knots(&samples.iter().map(|sample| sample.0).collect::<Vec<_>>()),
        control_points: samples.iter().map(|sample| sample.1).collect(),
        weights: None,
        periodic: false,
    })
}

type TransferredPcurveSample = (f64, Point2, Point3);

#[allow(clippy::too_many_arguments)]
fn transferred_pcurve_sample(
    ir: &CadIr,
    curve: &CurveId,
    source_surface: &SurfaceId,
    source_pcurve: &PcurveGeometry,
    target_surface: &SurfaceId,
    parameter: f64,
    seed: Option<Point2>,
    tolerance: f64,
) -> Option<TransferredPcurveSample> {
    let source_uv = pcurve_uv(source_pcurve, parameter)?;
    let point = decoded_surface_point(ir, source_surface, source_uv.u, source_uv.v)
        .or_else(|| model_curve_point(ir, curve, parameter))?;
    let target_uv = blend_boundary_parameter_from_support_pcurve(
        ir,
        target_surface,
        source_surface,
        source_pcurve,
        parameter,
        point,
        tolerance,
    )
    .or_else(|| {
        blend_boundary_parameter_from_support_spine(
            ir,
            target_surface,
            source_surface,
            point,
            seed,
            tolerance,
        )
    })
    .or_else(|| surface_parameters_for_fit(ir, target_surface, point, seed, tolerance))?;
    (decoded_surface_point(ir, target_surface, target_uv.u, target_uv.v)
        .is_some_and(|candidate| point_distance(candidate, point) <= tolerance)
        || blend_boundary_spine_geometry_matches(ir, target_surface, target_uv, point, tolerance))
    .then_some((parameter, target_uv, point))
}

pub(crate) fn blend_boundary_parameter_from_support_spine(
    ir: &CadIr,
    blend: &SurfaceId,
    support: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    tolerance: f64,
) -> Option<Point2> {
    let (supports, spine, _, _) = blend_surface_definition(ir, blend)?;
    let matches = supports
        .iter()
        .enumerate()
        .filter(|(_, candidate)| parameterization_equivalent_surfaces(ir, candidate, support))
        .map(|(boundary, _)| boundary)
        .collect::<Vec<_>>();
    let [boundary] = matches.as_slice() else {
        return None;
    };
    let parameter = closest_spine_parameter(ir, &spine, point, seed.map(|seed| seed.u))?;
    let parameters = Point2::new(parameter, *boundary as f64);
    (blend_surface_point_inner(ir, blend, parameters.u, parameters.v, 0)
        .is_some_and(|candidate| point_distance(candidate, point) <= tolerance)
        || blend_boundary_spine_geometry_matches(ir, blend, parameters, point, tolerance))
    .then_some(parameters)
}

fn blend_boundary_spine_geometry_matches(
    ir: &CadIr,
    blend: &SurfaceId,
    parameters: Point2,
    point: Point3,
    tolerance: f64,
) -> bool {
    if parameters.v.to_bits() != 0.0f64.to_bits() && parameters.v.to_bits() != 1.0f64.to_bits() {
        return false;
    }
    let Some((_, spine, radius, _)) = blend_surface_definition(ir, blend) else {
        return false;
    };
    let Some(center) = model_curve_point(ir, &spine, parameters.u) else {
        return false;
    };
    let radial = Vector3::new(point.x - center.x, point.y - center.y, point.z - center.z);
    let distance = (radial.x * radial.x + radial.y * radial.y + radial.z * radial.z).sqrt();
    if !distance.is_finite() || (distance - radius).abs() > tolerance {
        return false;
    }
    let Some(radial) = unit_vector(radial) else {
        return false;
    };
    let Some(tangent) = model_curve_tangent(ir, &spine, parameters.u) else {
        return false;
    };
    let angular_tolerance = (tolerance / radius).max(1.0e-8);
    (radial.x * tangent.x + radial.y * tangent.y + radial.z * tangent.z).abs() <= angular_tolerance
}

#[allow(clippy::too_many_arguments)]
fn append_transferred_pcurve_segment(
    ir: &CadIr,
    curve: &CurveId,
    source_surface: &SurfaceId,
    source_pcurve: &PcurveGeometry,
    target_surface: &SurfaceId,
    first: TransferredPcurveSample,
    last: TransferredPcurveSample,
    tolerance: f64,
    depth: usize,
    samples: &mut Vec<TransferredPcurveSample>,
) -> Option<()> {
    let midpoint_parameter = f64::midpoint(first.0, last.0);
    let midpoint_seed = Point2::new(
        f64::midpoint(first.1.u, last.1.u),
        f64::midpoint(first.1.v, last.1.v),
    );
    let midpoint = transferred_pcurve_sample(
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        midpoint_parameter,
        Some(midpoint_seed),
        tolerance,
    )?;
    let fits = [0.25, 0.5, 0.75].into_iter().all(|fraction| {
        let parameter = first.0 + fraction * (last.0 - first.0);
        let uv = Point2::new(
            first.1.u + fraction * (last.1.u - first.1.u),
            first.1.v + fraction * (last.1.v - first.1.v),
        );
        let Some(source_uv) = pcurve_uv(source_pcurve, parameter) else {
            return false;
        };
        let Some(source_point) =
            decoded_surface_point(ir, source_surface, source_uv.u, source_uv.v)
                .or_else(|| model_curve_point(ir, curve, parameter))
        else {
            return false;
        };
        decoded_surface_point(ir, target_surface, uv.u, uv.v)
            .is_some_and(|target_point| point_distance(source_point, target_point) <= tolerance)
            || blend_boundary_spine_geometry_matches(
                ir,
                target_surface,
                uv,
                source_point,
                tolerance,
            )
    });
    if fits {
        samples.push(last);
        return Some(());
    }
    (depth < 16).then_some(())?;
    append_transferred_pcurve_segment(
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        first,
        midpoint,
        tolerance,
        depth + 1,
        samples,
    )?;
    append_transferred_pcurve_segment(
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        midpoint,
        last,
        tolerance,
        depth + 1,
        samples,
    )
}

fn surface_parameters_for_fit(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    tolerance: f64,
) -> Option<Point2> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    match &carrier.geometry {
        SurfaceGeometry::Nurbs(nurbs) => nurbs_parameters(nurbs, point, seed),
        SurfaceGeometry::Procedural { .. } => {
            offset_surface_parameters_with_tolerance(ir, surface, point, seed, Some(tolerance))
                .or_else(|| blend_surface_parameters_for_fit(ir, surface, point, seed, tolerance))
        }
        geometry => analytic_surface_parameters(geometry, point),
    }
}

pub(crate) fn attach_tolerant_edge_intersections(
    ir: &mut CadIr,
    graph: &Graph,
    edges: &BTreeMap<u32, EdgeId>,
    prefix: &str,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) {
    let mut candidates = Vec::new();
    for (&xmt, edge_id) in edges {
        let Some(edge) = ir
            .model
            .edges
            .iter()
            .find(|candidate| &candidate.id == edge_id)
        else {
            continue;
        };
        if edge.curve.is_some() || edge.tolerance.is_none() {
            continue;
        }
        let mut supports = ir
            .model
            .coedges
            .iter()
            .filter(|coedge| &coedge.edge == edge_id)
            .filter_map(|coedge| {
                let face = ir
                    .model
                    .loops
                    .iter()
                    .find(|loop_| loop_.id == coedge.owner_loop)?
                    .face
                    .clone();
                ir.model
                    .faces
                    .iter()
                    .find(|candidate| candidate.id == face)
                    .map(|face| face.surface.clone())
            })
            .collect::<BTreeSet<_>>();
        if supports.len() != 2 {
            continue;
        }
        let second = supports.pop_last().expect("two supports");
        let first = supports.pop_first().expect("two supports");
        candidates.push((xmt, edge_id.clone(), [first, second]));
    }

    for (xmt, edge_id, supports) in candidates {
        let curve_id = CurveId(format!("{prefix}:tolerant-curve#{xmt}"));
        let procedural_id = ProceduralCurveId(format!("{prefix}:tolerant-intersection#{xmt}"));
        let Some(edge) = ir
            .model
            .edges
            .iter_mut()
            .find(|candidate| candidate.id == edge_id)
        else {
            continue;
        };
        edge.curve = Some(curve_id.clone());
        edge.param_range = Some([0.0, 1.0]);
        annotations.derived(&edge_id, "curve");
        annotations.derived(&edge_id, "param_range");
        if let Some(node) = graph.get(16, xmt) {
            annotations
                .note(&curve_id, source_stream, node.pos as u64)
                .tag("TOLERANT_EDGE_INTERSECTION");
            annotations
                .note(&procedural_id, source_stream, node.pos as u64)
                .tag("TOLERANT_EDGE_INTERSECTION");
        }
        annotations.derived(&curve_id, "geometry");
        annotations.derived(&procedural_id, "definition");
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Procedural {
                construction: procedural_id.clone(),
            },
            source_object: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: procedural_id,
            curve: curve_id,
            definition: ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext {
                    sides: supports.map(|surface| IntcurveSupportSide {
                        surface: Some(surface),
                        pcurve: None,
                        pcurve_parameter_range: None,
                    }),
                    parameter_range: [0.0, 1.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                },
                discontinuity_flag: false,
            },
            cache_fit_tolerance: None,
        });
    }
}

pub(crate) fn pcurve_matches_edge(
    ir: &CadIr,
    edge_id: &EdgeId,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    fit_tolerance: Option<f64>,
) -> bool {
    pcurve_matches_edge_range(ir, edge_id, surface_id, geometry, None, fit_tolerance)
}

fn pcurve_matches_edge_range(
    ir: &CadIr,
    edge_id: &EdgeId,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    parameter_range: Option<[f64; 2]>,
    fit_tolerance: Option<f64>,
) -> bool {
    let Some(edge) = ir.model.edges.iter().find(|edge| &edge.id == edge_id) else {
        return false;
    };
    let Some(coincident_surface) = ir
        .model
        .surfaces
        .iter()
        .find(|surface| &surface.id == surface_id)
        .and_then(|surface| {
            let [t0, t1] = parameter_range.or_else(|| pcurve_parameter_range(geometry))?;
            let uv = [pcurve_uv(geometry, t0)?, pcurve_uv(geometry, t1)?];
            Some([
                surface_point(&surface.geometry, uv[0].u, uv[0].v)?,
                surface_point(&surface.geometry, uv[1].u, uv[1].v)?,
            ])
        })
    else {
        return false;
    };
    let vertex = |id: &VertexId| {
        let vertex = ir.model.vertices.iter().find(|vertex| &vertex.id == id)?;
        let point = ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)?;
        Some((point.position, vertex.tolerance))
    };
    let (Some((start, start_tolerance)), Some((end, end_tolerance))) =
        (vertex(&edge.start), vertex(&edge.end))
    else {
        return false;
    };
    let allowance = [
        edge.tolerance,
        start_tolerance,
        end_tolerance,
        fit_tolerance,
    ]
    .into_iter()
    .flatten()
    .fold(0.01_f64, f64::max);
    let distance = |a: cadmpeg_ir::math::Point3, b: cadmpeg_ir::math::Point3| {
        ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
    };
    (distance(coincident_surface[0], start) <= allowance
        && distance(coincident_surface[1], end) <= allowance)
        || (distance(coincident_surface[0], end) <= allowance
            && distance(coincident_surface[1], start) <= allowance)
}

#[allow(clippy::too_many_arguments)]
fn retain_unresolved_topology_carriers(
    ir: &mut CadIr,
    stream_index: usize,
    graph: &Graph,
    surfaces: &mut BTreeMap<u32, SurfaceId>,
    curves: &mut BTreeMap<u32, CurveId>,
    pcurves: &BTreeMap<u32, PcurveId>,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) {
    let unknown = UnknownId(format!("nx:container:parasolid#{stream_index}"));
    for face in graph.of_kind(14) {
        let Some(surface_xmt) = face.face_fields().map(|fields| fields.surface) else {
            continue;
        };
        if surface_xmt <= 1 || surfaces.contains_key(&surface_xmt) {
            continue;
        }
        let id = SurfaceId(format!("nx:s{stream_index}:surface#unknown-{surface_xmt}"));
        annotations
            .note(&id, source_stream, face.pos as u64)
            .tag("UNRESOLVED_SURFACE_REFERENCE");
        annotations.exactness(&id, Exactness::Unknown);
        ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Unknown {
                record: Some(unknown.clone()),
            },
            source_object: None,
        });
        surfaces.insert(surface_xmt, id);
    }

    for edge in graph.of_kind(16) {
        let Some(curve_xmt) = edge.edge_fields().map(|fields| fields.curve) else {
            continue;
        };
        if curve_xmt <= 1 || curves.contains_key(&curve_xmt) || pcurves.contains_key(&curve_xmt) {
            continue;
        }
        let id = CurveId(format!("nx:s{stream_index}:curve#unknown-{curve_xmt}"));
        annotations
            .note(&id, source_stream, edge.pos as u64)
            .tag("UNRESOLVED_CURVE_REFERENCE");
        annotations.exactness(&id, Exactness::Unknown);
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry: CurveGeometry::Unknown {
                record: Some(unknown.clone()),
            },
            source_object: None,
        });
        curves.insert(curve_xmt, id);
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
        SurfaceGeometry::Procedural { .. } => "PROCEDURAL_SURFACE",
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
        CurveGeometry::Nurbs(_) => "B_SPLINE_CURVE",
        CurveGeometry::Procedural { .. } => "PROCEDURAL_CURVE",
        CurveGeometry::Composite { .. } => "COMPOSITE_CURVE",
        CurveGeometry::Polyline { .. } => "POLYLINE",
        CurveGeometry::Transformed { basis, .. } => curve_tag(basis),
        CurveGeometry::Unknown { .. } => "UNKNOWN_CURVE",
    }
}

pub(crate) fn decoded_tolerance(value: f64) -> Option<f64> {
    match value {
        MISSING_TOLERANCE => None,
        value if value.is_finite() && value > 0.0 && (value * 1000.0).is_finite() => {
            Some(value * 1000.0)
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn synthesize_closed_edge_vertex(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    prefix: &str,
    edge: &Node,
    curve: &CurveId,
    range: Option<[f64; 2]>,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    tolerance: Option<f64>,
) -> Option<VertexId> {
    let geometry = &ir
        .model
        .curves
        .iter()
        .find(|candidate| candidate.id == *curve)?
        .geometry;
    let parameter = range.map_or_else(
        || match geometry {
            CurveGeometry::Nurbs(nurbs) => nurbs.knots.first().copied().unwrap_or(0.0),
            _ => 0.0,
        },
        |range| range[0],
    );
    let position = curve_point(geometry, parameter)?;
    let point = PointId(format!("{prefix}:point#closed-edge-{}", edge.xmt));
    let vertex = VertexId(format!("{prefix}:vertex#closed-edge-{}", edge.xmt));
    annotations
        .note(&point, source_stream, edge.pos as u64)
        .tag("CLOSED_EDGE_POINT");
    annotations.exactness(&point, Exactness::Inferred);
    annotations
        .note(&vertex, source_stream, edge.pos as u64)
        .tag("CLOSED_EDGE_VERTEX");
    annotations.exactness(&vertex, Exactness::Inferred);
    ir.model.points.push(Point {
        id: point.clone(),
        position,
        source_object: None,
    });
    ir.model.vertices.push(Vertex {
        id: vertex.clone(),
        point,
        tolerance,
    });
    Some(vertex)
}

fn canonical_trim_range(ir: &CadIr, basis: &CurveId, raw: [f64; 2]) -> Option<[f64; 2]> {
    let curve = ir.model.curves.iter().find(|curve| curve.id == *basis)?;
    match &curve.geometry {
        CurveGeometry::Line { .. } => {
            let range = [raw[0] * 1000.0, raw[1] * 1000.0];
            range.into_iter().all(f64::is_finite).then_some(range)
        }
        CurveGeometry::Nurbs(nurbs) => {
            let domain = [*nurbs.knots.first()?, *nurbs.knots.last()?];
            let epsilon = 1.0e-6 * (1.0 + domain[0].abs().max(domain[1].abs()));
            if raw
                .iter()
                .any(|value| *value < domain[0] - epsilon || *value > domain[1] + epsilon)
            {
                None
            } else {
                Some([
                    raw[0].clamp(domain[0], domain[1]),
                    raw[1].clamp(domain[0], domain[1]),
                ])
            }
        }
        _ => Some(raw),
    }
}

fn orient_edge_range(
    ir: &CadIr,
    curve: &CurveId,
    range: [f64; 2],
    start: &VertexId,
    end: &VertexId,
    edge_tolerance: Option<f64>,
) -> Option<([f64; 2], bool)> {
    let geometry = &ir
        .model
        .curves
        .iter()
        .find(|candidate| candidate.id == *curve)?
        .geometry;
    let range = if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
    };
    let range = match geometry {
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => {
            let sweep = range[1] - range[0];
            (0.0..=std::f64::consts::TAU)
                .contains(&sweep)
                .then_some(())?;
            let start = range[0].rem_euclid(std::f64::consts::TAU);
            [start, start + sweep]
        }
        _ => range,
    };
    let at = match (
        curve_point(geometry, range[0]),
        curve_point(geometry, range[1]),
    ) {
        (Some(start), Some(end)) => [start, end],
        _ if ir
            .model
            .procedural_curves
            .iter()
            .any(|procedural| procedural.curve == *curve) =>
        {
            return Some((range, false));
        }
        _ => return None,
    };
    let vertex_position = |vertex: &VertexId| {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|candidate| candidate.id == *vertex)?;
        let point = ir
            .model
            .points
            .iter()
            .find(|candidate| candidate.id == vertex.point)?;
        Some((point.position, vertex.tolerance))
    };
    let (start_position, start_tolerance) = vertex_position(start)?;
    let (end_position, end_tolerance) = vertex_position(end)?;
    let cache_tolerance = ir
        .model
        .procedural_curves
        .iter()
        .find(|procedural| procedural.curve == *curve)
        .and_then(|procedural| procedural.cache_fit_tolerance);
    let allowance = [
        edge_tolerance,
        start_tolerance,
        end_tolerance,
        cache_tolerance,
    ]
    .into_iter()
    .flatten()
    .fold(0.01_f64, f64::max);
    let distance = |a: cadmpeg_ir::math::Point3, b: cadmpeg_ir::math::Point3| {
        ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
    };
    if distance(at[0], start_position) <= allowance && distance(at[1], end_position) <= allowance {
        Some((range, false))
    } else if distance(at[1], start_position) <= allowance
        && distance(at[0], end_position) <= allowance
    {
        Some((range, true))
    } else {
        None
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
    if let Some((_, table)) = scan.container.rmfastload_object_id_table() {
        attributes.insert(
            "rmfastload_active_object_count".to_string(),
            table.object_ids.len().to_string(),
        );
    }
    let mut preview_count = 0usize;
    for entry in scan
        .container
        .entries
        .iter()
        .filter(|entry| entry.name == "/Root/images/preview")
    {
        let Some((offset, size)) = entry.file_span else {
            continue;
        };
        let (Ok(start), Ok(size)) = (usize::try_from(offset), usize::try_from(size)) else {
            continue;
        };
        let Some(payload) = scan.container.data.get(start..start.saturating_add(size)) else {
            continue;
        };
        let Some((width, height, precision, components)) = jpeg_dimensions(payload) else {
            continue;
        };
        let prefix = format!("jpeg_preview_{preview_count}");
        attributes.insert(format!("{prefix}_width"), width.to_string());
        attributes.insert(format!("{prefix}_height"), height.to_string());
        attributes.insert(format!("{prefix}_precision"), precision.to_string());
        attributes.insert(format!("{prefix}_components"), components.to_string());
        attributes.insert(format!("{prefix}_byte_len"), payload.len().to_string());
        attributes.insert(format!("{prefix}_sha256"), sha256_hex(payload));
        preview_count += 1;
    }
    attributes.insert("jpeg_preview_count".to_string(), preview_count.to_string());
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

pub(crate) fn jpeg_dimensions(payload: &[u8]) -> Option<(u16, u16, u8, u8)> {
    if payload.get(..2)? != [0xff, 0xd8] {
        return None;
    }
    let mut offset = 2usize;
    while offset < payload.len() {
        while payload.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *payload.get(offset)?;
        offset += 1;
        if marker == 0xd9 || marker == 0xda {
            return None;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length = usize::from(u16::from_be_bytes([
            *payload.get(offset)?,
            *payload.get(offset + 1)?,
        ]));
        if length < 2 {
            return None;
        }
        let segment_start = offset + 2;
        let segment_end = offset.checked_add(length)?;
        let segment = payload.get(segment_start..segment_end)?;
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            let precision = *segment.first()?;
            let height = u16::from_be_bytes([*segment.get(1)?, *segment.get(2)?]);
            let width = u16::from_be_bytes([*segment.get(3)?, *segment.get(4)?]);
            let components = *segment.get(5)?;
            if width == 0
                || height == 0
                || components == 0
                || segment.len() != 6 + 3 * usize::from(components)
            {
                return None;
            }
            return Some((width, height, precision, components));
        }
        offset = segment_end;
    }
    None
}

fn build_geometry_report(
    scan: &Scan,
    ir: &CadIr,
    counts: &Counts,
    has_topology: bool,
    has_unresolved_sub_bodies: bool,
    tessellation_count: usize,
) -> DecodeReport {
    let mut losses = Vec::new();

    losses.push(NxLossCode::CarrierSummary.note(format!(
        "Decoded {} POINT carrier(s) verbatim from Parasolid POINT records (3×f64 big-endian, \
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
    )));

    if tessellation_count != 0 {
        losses.push(NxLossCode::TessellationSummary.note(format!(
            "Decoded {tessellation_count} embedded JT display tessellation(s) with scene-node ownership, model-space coordinates, topological triangle connectivity, and corner normals when bound."
        )));
    }

    if !has_topology {
        losses.push(NxLossCode::TopologyGraphNotReconstructed.note(
            "The B-rep topology graph (body→shell→face→loop→fin→edge→vertex) was not \
             reconstructed because the surviving typed records did not form a complete \
             connected ownership graph. Exact-key supported partition↔deltas replacements \
             and deletions were applied before graph construction. Required unresolved \
             records prevent their dependent incidence from being emitted; decoded geometry \
             then remains unattached.",
        ));
    }

    if counts.intersection_rejections.total() > 0 {
        losses.push(NxLossCode::IntersectionRecordsOpaque.note(format!(
            "{} surface-intersection record(s) without a complete validated CHART_s and \
             term-endpoint witness remain opaque constructions. Support-UV values govern \
             optional pcurve attachment and do not invalidate a witnessed 3D carrier. Each \
             Parasolid stream is preserved verbatim as an unknown passthrough record so the \
             unresolved source bytes remain available. Rejections: {} missing chart, {} missing \
             start term, {} missing end term, {} endpoint mismatch.",
            counts.intersection_rejections.total(),
            counts.intersection_rejections.missing_chart,
            counts.intersection_rejections.missing_start_term,
            counts.intersection_rejections.missing_end_term,
            counts.intersection_rejections.endpoint_mismatch,
        )));
    }

    if scan.count(StreamKind::Deltas) > 0 {
        let unmatched_tombstones = unmatched_delta_tombstone_count(scan);
        let note = if unmatched_tombstones == 0 {
            NxLossCode::DeltasApplied.note(format!(
                "{} Parasolid deltas stream(s) were processed in validated UG_PART segment order. \
                 Equal-schema deltas were paired with the preceding partition. Exact-key \
                 BODY, SHELL, FACE, LOOP, FIN, EDGE, VERTEX, REGION, POINT, LINE, CIRCLE, ELLIPSE, PLANE, CYLINDER, CONE, SPHERE, TORUS, BLEND_SURF, OFFSET_SURF, B_SURFACE, TRIMMED_CURVE, B_CURVE, and SP_CURVE full records and compact \
                 non-topology replacements and tombstones were applied using the last event for \
                 each key. Validated partition topology remained authoritative, including any \
                 point, curve, or surface carrier still referenced by surviving topology. Every \
                 terminal tombstone resolved to an exact current or earlier-added key.",
                scan.count(StreamKind::Deltas)
            ))
        } else {
            NxLossCode::DeltasTombstonesUnresolved.note(format!(
                "{} Parasolid deltas stream(s) were processed in validated UG_PART segment order. \
                 Equal-schema deltas were paired with the preceding partition. Exact-key revisions were applied using the last \
                 event for each key, but {unmatched_tombstones} terminal tombstone(s) have no exact \
                 current or earlier-added key and remain unresolved.",
                scan.count(StreamKind::Deltas)
            ))
        };
        losses.push(note);
    }

    if has_unresolved_sub_bodies {
        losses.push(NxLossCode::SubBodyCompositionUnresolved.note(format!(
            "This part is composed of {} sub-body partition(s); its decoded feature-history \
             Booleans do not resolve every intermediate body object to a partition image. \
             Carriers from all sub-bodies are emitted without the unresolved composition that \
             would remove interior/construction faces.",
            scan.count(StreamKind::Partition)
        )));
    }

    append_design_intent_losses(ir, &mut losses);

    losses.push(NxLossCode::MaterialMetadataNotTransferred.note(
        "Material and appearance assignment, class-specific entity attribute fields, and \
         assembly occurrence placements were not transferred: their remaining NX \
         object-model and Parasolid field serialization is not decoded.",
    ));

    DecodeReport {
        format: "nx".to_string(),
        container_only: false,
        geometry_transferred: true,
        coverage: std::collections::BTreeMap::new(),
        losses,
        notes: summary_notes(scan),
    }
}

pub(crate) fn append_design_intent_losses(ir: &CadIr, losses: &mut Vec<LossNote>) {
    let unresolved_suppression_count = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.suppressed.is_none())
        .count();
    if unresolved_suppression_count != 0 {
        losses.push(NxLossCode::FeatureSuppressionUnresolved.note(format!(
            "Suppression state remains unresolved for {unresolved_suppression_count} NX \
             feature history operation(s)."
        )));
    }

    let active_configuration_count = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.active)
        .count();
    let current_bodies = ir
        .model
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<BTreeSet<_>>();
    let incomplete_configuration_count = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| {
            configuration.bodies.is_unresolved()
                || active_configuration_count != 1
                || (configuration.active
                    && configuration.bodies.resolved().is_none_or(|bodies| {
                        bodies.len() != current_bodies.len()
                            || bodies.iter().collect::<BTreeSet<_>>() != current_bodies
                    }))
        })
        .count();
    if incomplete_configuration_count != 0 {
        losses.push(NxLossCode::ConfigurationActivationUnresolved.note(format!(
            "Activation or complete body membership remains unresolved for \
             {incomplete_configuration_count} NX design configuration(s)."
        )));
    }

    let incomplete_expression_count = incomplete_expression_parameters(ir).len();
    if incomplete_expression_count != 0 {
        losses.push(NxLossCode::ExpressionParameterIncomplete.note(format!(
            "Neutral evaluation or dependency semantics remain incomplete for \
             {incomplete_expression_count} NX expression parameter(s)."
        )));
    }

    let mut native_feature_kinds = BTreeMap::<&str, usize>::new();
    for feature in &ir.model.features {
        if let FeatureDefinition::Native { kind, .. } = &feature.definition {
            *native_feature_kinds.entry(kind.as_str()).or_default() += 1;
        }
    }
    if !native_feature_kinds.is_empty() {
        let kinds = native_feature_kinds
            .into_iter()
            .map(|(kind, count)| format!("{kind} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(NxLossCode::FeatureNativeOnly.note(format!(
            "NX feature-history operation(s) remain native-only because their complete neutral \
             operation semantics are not decoded: {kinds}."
        )));
    }

    let mut unresolved_feature_families = BTreeMap::<&str, usize>::new();
    for feature in &ir.model.features {
        let family = match feature.definition {
            FeatureDefinition::DatumPlaneUnresolved => "datum plane",
            FeatureDefinition::DatumPointUnresolved => "datum point",
            FeatureDefinition::DatumCoordinateSystemUnresolved => "datum coordinate system",
            FeatureDefinition::LoftUnresolved => "loft",
            FeatureDefinition::FreeformSurfaceUnresolved => "freeform surface",
            FeatureDefinition::DraftUnresolved => "draft",
            _ => continue,
        };
        *unresolved_feature_families.entry(family).or_default() += 1;
    }
    if !unresolved_feature_families.is_empty() {
        let families = unresolved_feature_families
            .into_iter()
            .map(|(family, count)| format!("{family} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(
            NxLossCode::FeatureFamilyConstructionUnresolved.note(format!(
                "NX feature family identities were transferred, but their neutral construction \
             semantics remain unresolved: {families}."
            )),
        );
    }

    let mut incomplete_feature_families = BTreeMap::<&str, usize>::new();
    for feature in &ir.model.features {
        if feature.suppressed != Some(true) {
            if let Some(family) = body_output_feature_family(&feature.definition).filter(|_| {
                feature.outputs.is_empty()
                    || feature.outputs.iter().collect::<BTreeSet<_>>().len()
                        != feature.outputs.len()
                    || feature
                        .outputs
                        .iter()
                        .any(|output| !ir.model.bodies.iter().any(|body| body.id == *output))
            }) {
                *incomplete_feature_families.entry(family).or_default() += 1;
                continue;
            }
        }
        let family = match &feature.definition {
            FeatureDefinition::Block {
                dimensions,
                placement,
            } if dimensions.is_none() || placement.is_none() => "block",
            FeatureDefinition::DatumOffsetPlane {
                reference,
                distance,
            } if !distance.0.is_finite()
                || reference.as_ref().is_none_or(|reference| {
                    ir.model
                        .features
                        .iter()
                        .find(|candidate| candidate.id == *reference)
                        .is_none_or(|source| source.ordinal >= feature.ordinal)
                        || !feature.dependencies.contains(reference)
                }) =>
            {
                "datum plane"
            }
            FeatureDefinition::ExtractBody { source } if body_selection_is_incomplete(source) => {
                "extract body"
            }
            FeatureDefinition::Sketch { space, sketch }
                if !matches!(space, SketchSpace::Planar) || sketch.is_none() =>
            {
                "sketch"
            }
            FeatureDefinition::Loft {
                sections,
                guides,
                op,
                ..
            } if sections.len() < 2
                || sections.iter().any(|section| match section {
                    cadmpeg_ir::features::LoftSection::Profile(profile) => {
                        profile_ref_is_incomplete(profile)
                    }
                    cadmpeg_ir::features::LoftSection::Point { .. } => false,
                })
                || guides.iter().any(path_ref_is_incomplete)
                || matches!(op, BooleanOp::Unresolved) =>
            {
                "loft"
            }
            FeatureDefinition::ProjectedCurve {
                source,
                target_faces,
                direction,
                bidirectional,
            } if path_ref_is_incomplete(source)
                || face_selection_is_incomplete(target_faces)
                || matches!(
                    direction,
                    CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved)
                )
                || bidirectional.is_none() =>
            {
                "projected curve"
            }
            FeatureDefinition::TrimSurface { faces, tool, keep }
                if face_selection_is_incomplete(faces)
                    || path_ref_is_incomplete(tool)
                    || matches!(keep, TrimRegion::Unresolved) =>
            {
                "trim surface"
            }
            FeatureDefinition::ExtendSurface {
                faces,
                distance,
                method,
            } if face_selection_is_incomplete(faces)
                || distance.is_none()
                || matches!(method, cadmpeg_ir::features::SurfaceExtension::Unresolved) =>
            {
                "extend surface"
            }
            FeatureDefinition::Hole {
                profile,
                face,
                position,
                direction,
                kind,
                exit_kind,
                diameter,
                extent,
                ..
            } if hole_feature_is_incomplete(
                profile.as_ref(),
                face.as_ref(),
                *position,
                *direction,
                (kind, exit_kind.as_ref()),
                *diameter,
                extent.as_ref(),
            ) =>
            {
                "hole"
            }
            FeatureDefinition::Rib { construction, op }
                if rib_feature_is_incomplete(construction, *op) =>
            {
                "rib"
            }
            FeatureDefinition::Chamfer { groups, .. }
                if groups.is_empty()
                    || groups.iter().any(|group| {
                        edge_selection_is_incomplete(&group.edges)
                            || matches!(group.spec, ChamferSpec::Unresolved { .. })
                    }) =>
            {
                "chamfer"
            }
            FeatureDefinition::Fillet { groups }
                if groups.is_empty()
                    || groups.iter().any(|group| {
                        edge_selection_is_incomplete(&group.edges)
                            || radius_spec_is_incomplete(&group.radius)
                    }) =>
            {
                "fillet"
            }
            FeatureDefinition::FaceBlend {
                first_faces,
                second_faces,
                radius,
            } if face_selection_is_incomplete(first_faces)
                || face_selection_is_incomplete(second_faces)
                || face_selections_overlap(first_faces, second_faces)
                || radius_spec_is_incomplete(radius) =>
            {
                "face blend"
            }
            FeatureDefinition::SewBodies { bodies, .. }
                if body_selection_is_incomplete(bodies)
                    || explicit_body_ids(bodies).is_some_and(|bodies| bodies.len() < 2) =>
            {
                "sew bodies"
            }
            FeatureDefinition::TrimBodies {
                targets,
                tools,
                keep,
            } if body_selection_is_incomplete(targets)
                || body_selection_is_incomplete(tools)
                || body_selections_overlap(targets, tools)
                || matches!(keep, BodyTrimSide::Unresolved) =>
            {
                "trim bodies"
            }
            FeatureDefinition::Extrude {
                profile,
                extent,
                op,
                ..
            } if profile_ref_is_incomplete(profile)
                || extrude_extent_is_incomplete(extent)
                || matches!(op, BooleanOp::Unresolved) =>
            {
                "extrude"
            }
            FeatureDefinition::OffsetSurface { faces, distance }
                if face_selection_is_incomplete(faces) || distance.is_none() =>
            {
                "offset surface"
            }
            FeatureDefinition::Thicken {
                faces,
                thickness,
                side,
            } if face_selection_is_incomplete(faces) || thickness.is_none() || side.is_none() => {
                "thicken"
            }
            FeatureDefinition::Draft {
                faces,
                neutral_plane,
                ..
            } if face_selection_is_incomplete(faces)
                || face_selection_is_incomplete(neutral_plane) =>
            {
                "draft"
            }
            FeatureDefinition::Pattern { seeds, pattern }
                if pattern_feature_is_incomplete(seeds, pattern) =>
            {
                "pattern"
            }
            FeatureDefinition::Combine { target, tools, op }
                if body_selection_is_incomplete(target)
                    || body_selection_is_incomplete(tools)
                    || body_selections_overlap(target, tools)
                    || matches!(op, BooleanOp::Unresolved) =>
            {
                "body combine"
            }
            FeatureDefinition::DeleteBody { bodies, mode }
                if body_selection_is_incomplete(bodies)
                    || matches!(mode, BodyRetentionMode::Unresolved) =>
            {
                "delete body"
            }
            _ => continue,
        };
        *incomplete_feature_families.entry(family).or_default() += 1;
    }
    if !incomplete_feature_families.is_empty() {
        let families = incomplete_feature_families
            .into_iter()
            .map(|(family, count)| format!("{family} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(NxLossCode::FeatureFamilyLineageUnresolved.note(format!(
            "NX feature families were transferred as typed neutral operations, but \
             construction fields or output lineage remain unresolved or native-only: \
             {families}."
        )));
    }

    let sketch_feature_count = ir
        .model
        .features
        .iter()
        .filter(|feature| matches!(feature.definition, FeatureDefinition::Sketch { .. }))
        .count();
    let unresolved_sketch_feature_count = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            matches!(
                feature.definition,
                FeatureDefinition::Sketch { sketch: None, .. }
            )
        })
        .count();
    if unresolved_sketch_feature_count != 0 {
        losses.push(NxLossCode::SketchGraphUnresolved.note(format!(
            "Decoded {sketch_feature_count} NX sketch history feature(s), of which \
             {unresolved_sketch_feature_count} have no neutral sketch graph because complete \
             sketch placement and entity semantics are unresolved."
        )));
    } else if sketch_feature_count != 0 && ir.model.sketch_constraints.is_empty() {
        losses.push(NxLossCode::SketchConstraintsUntransferred.note(format!(
            "Decoded {} NX sketch record(s), but no sketch constraints were transferred because \
             their object-model field serialization and operand roles are unresolved.",
            ir.model.sketches.len()
        )));
    }

    let native_sketch_entity_count = ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| {
            matches!(
                entity.geometry,
                cadmpeg_ir::sketches::SketchGeometry::Native { .. }
            )
        })
        .count();
    let native_sketch_constraint_count = ir
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| {
            matches!(
                constraint.definition,
                cadmpeg_ir::sketches::SketchConstraintDefinition::Native { .. }
            )
        })
        .count();
    if native_sketch_entity_count != 0 || native_sketch_constraint_count != 0 {
        losses.push(NxLossCode::SketchRecordsNative.note(format!(
            "Neutral semantics remain unresolved for {native_sketch_entity_count} NX sketch \
             geometry record(s) and {native_sketch_constraint_count} sketch constraint \
             record(s)."
        )));
    }
}

pub(crate) fn body_output_feature_family(definition: &FeatureDefinition) -> Option<&'static str> {
    match definition {
        FeatureDefinition::Block { .. } => Some("block"),
        FeatureDefinition::ExtractBody { .. } => Some("extract body"),
        FeatureDefinition::Loft { .. } => Some("loft"),
        FeatureDefinition::TrimSurface { .. } => Some("trim surface"),
        FeatureDefinition::ExtendSurface { .. } => Some("extend surface"),
        FeatureDefinition::Hole { .. } => Some("hole"),
        FeatureDefinition::Rib { .. } => Some("rib"),
        FeatureDefinition::Chamfer { .. } => Some("chamfer"),
        FeatureDefinition::Fillet { .. } => Some("fillet"),
        FeatureDefinition::FaceBlend { .. } => Some("face blend"),
        FeatureDefinition::SewBodies { .. } => Some("sew bodies"),
        FeatureDefinition::TrimBodies { .. } => Some("trim bodies"),
        FeatureDefinition::Extrude { .. } => Some("extrude"),
        FeatureDefinition::OffsetSurface { .. } => Some("offset surface"),
        FeatureDefinition::Thicken { .. } => Some("thicken"),
        FeatureDefinition::Draft { .. } => Some("draft"),
        FeatureDefinition::Pattern { .. } => Some("pattern"),
        FeatureDefinition::Combine { .. } => Some("body combine"),
        _ => None,
    }
}

pub(crate) fn incomplete_expression_parameters(ir: &CadIr) -> BTreeSet<ParameterId> {
    let parameter_owners = ir
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.owner.clone())
        .collect::<BTreeSet<_>>();
    let mut incomplete = BTreeSet::new();
    for owner in parameter_owners {
        let parameters = ir
            .model
            .parameters
            .iter()
            .filter(|parameter| parameter.owner == owner)
            .collect::<Vec<_>>();
        let mut ids_by_name = BTreeMap::<&str, Vec<&ParameterId>>::new();
        for parameter in &parameters {
            ids_by_name
                .entry(parameter.name.as_str())
                .or_default()
                .push(&parameter.id);
        }
        let expected = parameters
            .iter()
            .map(|parameter| {
                let [_] = ids_by_name.get(parameter.name.as_str())?.as_slice() else {
                    return None;
                };
                let mut seen = BTreeSet::new();
                let dependencies = crate::native::expression_parameter_names(&parameter.expression)
                    .into_iter()
                    .map(|name| {
                        let [dependency] = ids_by_name.get(name)?.as_slice() else {
                            return None;
                        };
                        Some((*dependency).clone())
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(
                    dependencies
                        .into_iter()
                        .filter(|dependency| seen.insert(dependency.clone()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        let indices = parameters
            .iter()
            .enumerate()
            .map(|(index, parameter)| (&parameter.id, index))
            .collect::<BTreeMap<_, _>>();
        let mut emitted = BTreeSet::new();
        while let Some(index) = (0..parameters.len()).find(|index| {
            !emitted.contains(index)
                && expected[*index].as_ref().is_some_and(|dependencies| {
                    dependencies.iter().all(|dependency| {
                        indices
                            .get(dependency)
                            .is_some_and(|dependency| emitted.contains(dependency))
                    })
                })
        }) {
            emitted.insert(index);
        }
        for (index, parameter) in parameters.into_iter().enumerate() {
            if expected[index].as_ref() != Some(&parameter.dependencies)
                || !emitted.contains(&index)
                || parameter.value.is_none()
            {
                incomplete.insert(parameter.id.clone());
            }
        }
    }
    incomplete
}

pub(crate) fn hole_feature_is_incomplete(
    profile: Option<&ProfileRef>,
    face: Option<&FaceSelection>,
    position: Option<Point3>,
    direction: Option<Vector3>,
    treatments: (&HoleKind, Option<&HoleKind>),
    diameter: Option<Length>,
    extent: Option<&Termination>,
) -> bool {
    let (kind, exit_kind) = treatments;
    let profile_incomplete = profile.is_some_and(profile_ref_is_incomplete);
    let face_incomplete = face.is_some_and(face_selection_is_incomplete);
    let location_unresolved = position.is_none() && profile.is_none_or(profile_ref_is_incomplete);
    let orientation_unresolved =
        direction.is_none() && face.is_none_or(face_selection_is_incomplete);
    profile_incomplete
        || face_incomplete
        || location_unresolved
        || orientation_unresolved
        || matches!(kind, HoleKind::Unresolved { .. })
        || exit_kind.is_some_and(|kind| matches!(kind, HoleKind::Unresolved { .. }))
        || diameter.is_none()
        || extent.is_none_or(termination_is_incomplete)
}

pub(crate) fn extrude_extent_is_incomplete(extent: &ExtrudeExtent) -> bool {
    match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
            termination_is_incomplete(&side.termination)
        }
        ExtrudeExtent::TwoSided { first, second } => {
            termination_is_incomplete(&first.termination)
                || termination_is_incomplete(&second.termination)
        }
    }
}

pub(crate) fn termination_is_incomplete(termination: &Termination) -> bool {
    match termination {
        Termination::Unresolved => true,
        Termination::ToFace { face, .. } => face_selection_is_incomplete(face),
        Termination::ToShape { target } => face_selection_is_incomplete(target),
        Termination::Blind { .. }
        | Termination::ThroughAll
        | Termination::ThroughNext
        | Termination::ToFirst
        | Termination::ToLast
        | Termination::ToVertex { .. }
        | Termination::OffsetFromFace { .. }
        | Termination::Angle { .. } => false,
    }
}

pub(crate) fn rib_feature_is_incomplete(construction: &RibConstruction, op: BooleanOp) -> bool {
    construction
        .profile
        .as_ref()
        .is_none_or(profile_ref_is_incomplete)
        || construction.direction.is_none()
        || construction.thickness.is_none()
        || construction.side.is_none()
        || matches!(construction.draft, RibDraft::Unresolved)
        || matches!(op, BooleanOp::Unresolved)
}

pub(crate) fn pattern_is_incomplete(pattern: &PatternKind) -> bool {
    match pattern {
        PatternKind::Unresolved { .. } => true,
        PatternKind::Linear { direction, .. } => direction.is_none(),
        PatternKind::LinearOffsets { direction, offsets } => {
            direction.is_none() || offsets.is_empty()
        }
        PatternKind::Circular { .. } | PatternKind::Mirror { .. } => false,
        PatternKind::CircularAngles { angles, .. } => angles.is_empty(),
        PatternKind::CurveDriven { path, .. } => path.as_ref().is_none_or(path_ref_is_incomplete),
        PatternKind::Scale { center, .. } => {
            matches!(center, cadmpeg_ir::features::PatternScaleCenter::Native(_))
        }
        PatternKind::Composite { stages } => {
            stages.is_empty()
                || stages
                    .iter()
                    .any(|stage| pattern_is_incomplete(&stage.pattern))
        }
    }
}

pub(crate) fn pattern_feature_is_incomplete(
    seeds: &[cadmpeg_ir::features::PatternSeed],
    pattern: &PatternKind,
) -> bool {
    seeds.is_empty()
        || seeds
            .iter()
            .enumerate()
            .any(|(index, seed)| seeds[..index].contains(seed))
        || pattern_is_incomplete(pattern)
}

pub(crate) fn radius_spec_is_incomplete(radius: &RadiusSpec) -> bool {
    match radius {
        RadiusSpec::Unresolved { .. } => true,
        RadiusSpec::Constant { .. } => false,
        RadiusSpec::Chordal { .. } => false,
        RadiusSpec::Variable { points } => points.len() < 2,
    }
}

pub(crate) fn body_selection_is_incomplete(selection: &BodySelection) -> bool {
    explicit_body_ids(selection).is_none_or(selection_ids_are_incomplete)
}

pub(crate) fn body_selections_overlap(first: &BodySelection, second: &BodySelection) -> bool {
    explicit_body_ids(first).is_some_and(|first| {
        explicit_body_ids(second)
            .is_some_and(|second| first.iter().any(|body| second.contains(body)))
    })
}

fn explicit_body_ids(selection: &BodySelection) -> Option<&[BodyId]> {
    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => Some(bodies),
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Local { .. }
        | BodySelection::Native(_) => None,
    }
}

pub(crate) fn face_selection_is_incomplete(selection: &FaceSelection) -> bool {
    match selection {
        FaceSelection::Unresolved
        | FaceSelection::Generated { .. }
        | FaceSelection::Native(_)
        | FaceSelection::Historical { .. }
        | FaceSelection::HistoricalPartial { .. } => true,
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => {
            selection_ids_are_incomplete(faces)
        }
    }
}

pub(crate) fn face_selections_overlap(first: &FaceSelection, second: &FaceSelection) -> bool {
    let first = match first {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => faces,
        FaceSelection::Unresolved
        | FaceSelection::Generated { .. }
        | FaceSelection::Native(_)
        | FaceSelection::Historical { .. }
        | FaceSelection::HistoricalPartial { .. } => return false,
    };
    let second = match second {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => faces,
        FaceSelection::Unresolved
        | FaceSelection::Generated { .. }
        | FaceSelection::Native(_)
        | FaceSelection::Historical { .. }
        | FaceSelection::HistoricalPartial { .. } => return false,
    };
    first.iter().any(|face| second.contains(face))
}

pub(crate) fn edge_selection_is_incomplete(selection: &EdgeSelection) -> bool {
    match selection {
        EdgeSelection::Unresolved
        | EdgeSelection::Generated { .. }
        | EdgeSelection::Native(_)
        | EdgeSelection::Historical { .. }
        | EdgeSelection::HistoricalPartial { .. } => true,
        EdgeSelection::All => false,
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => {
            selection_ids_are_incomplete(edges)
        }
    }
}

pub(crate) fn profile_ref_is_incomplete(profile: &ProfileRef) -> bool {
    match profile {
        ProfileRef::Unresolved(_) | ProfileRef::Native(_) => true,
        ProfileRef::Sketch(_) => false,
        ProfileRef::Feature(_) | ProfileRef::Generated { .. } => false,
        ProfileRef::Faces(faces) => selection_ids_are_incomplete(faces),
        ProfileRef::SketchProfiles { .. }
        | ProfileRef::SketchRegions { .. }
        | ProfileRef::SketchSelection { .. }
        | ProfileRef::SpatialSketchProfiles { .. }
        | ProfileRef::SpatialSketchSelection { .. }
        | ProfileRef::HistoricalFaces { .. } => false,
    }
}

fn selection_ids_are_incomplete<T: Ord>(ids: &[T]) -> bool {
    ids.is_empty() || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
}

pub(crate) fn path_ref_is_incomplete(path: &PathRef) -> bool {
    match path {
        PathRef::Unresolved(_) | PathRef::Native(_) => true,
        PathRef::HistoricalEdges { edges, .. } => selection_ids_are_incomplete(edges),
        PathRef::Sketch(_) => false,
        PathRef::SpatialSketchSelection { selections, .. } => {
            selection_ids_are_incomplete(selections)
        }
        PathRef::Edges(edges) => selection_ids_are_incomplete(edges),
        PathRef::Curves(curves) => selection_ids_are_incomplete(curves),
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
    let parsed = crate::native::ParsedStreams::parse(scan);
    let model = crate::native::NativeModel::extract(&scan.container, &scan.streams, &parsed);
    crate::native::attach_annotations(&mut ir, &model, scan, &mut annotations, &mut unknowns)
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    Ok((ir, annotations.build(), unknowns))
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
        losses.push(NxLossCode::AssemblyComponentsExternal.note(
            "No inline Parasolid geometry: this is an assembly .prt. Component geometry \
             lives in external child .prt files named in EXTREFSTREAM, and the assembled \
             solid's inputs (child partitions + constraint solve) are absent from this \
             file. This is an external-dependency boundary, not a decode gap.",
        ));
    } else {
        losses.push(NxLossCode::GeometryNotTransferred.note(
            "No B-rep geometry was transferred: no gate-passing analytic carrier was found \
             in the embedded Parasolid streams (they may hold only B-spline/procedural \
             geometry this codec does not yet type). The streams are preserved verbatim as \
             unknown passthrough records.",
        ));
    }

    if container_only {
        losses.push(
            NxLossCode::ContainerOnlyDecode
                .note("Container-only decode requested; entity decode was not attempted."),
        );
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
    let framed_om_sections = c.om_sections();
    if !framed_om_sections.is_empty() {
        let declarations = framed_om_sections
            .iter()
            .map(|(_, section)| section.types.len())
            .sum::<usize>();
        let fields = framed_om_sections
            .iter()
            .map(|(_, section)| section.fields.len())
            .sum::<usize>();
        notes.push(format!(
            "NX object model: {} size-framed section(s), {} class declaration(s), {} field declaration(s)",
            framed_om_sections.len(),
            declarations,
            fields
        ));
    }
    let om_sections = c.indexed_om_sections();
    if !om_sections.is_empty() {
        let entities = om_sections
            .iter()
            .filter(|(_, section)| {
                section
                    .records
                    .first()
                    .is_some_and(|record| record.object_id.is_some())
            })
            .map(|(_, section)| section.records.len())
            .sum::<usize>();
        let blocks = om_sections
            .iter()
            .filter(|(_, section)| {
                section
                    .records
                    .first()
                    .is_some_and(|record| record.object_id.is_none())
            })
            .map(|(_, section)| section.records.len() + usize::from(section.control.is_some()))
            .sum::<usize>();
        if blocks == 0 {
            notes.push(format!(
                "NX object model: {} indexed section(s), {} bounded entity record(s)",
                om_sections.len(),
                entities
            ));
        } else {
            notes.push(format!(
                "NX object model: {} indexed section(s), {} ID-bounded entity record(s), {} offset-only data block(s)",
                om_sections.len(),
                entities,
                blocks
            ));
        }
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
