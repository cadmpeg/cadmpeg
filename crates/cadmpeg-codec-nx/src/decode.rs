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

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::eval::{curve_point, pcurve_uv, surface_point};
use cadmpeg_ir::features::{
    Angle, BodySelection, BooleanOp, ConfigurationId, DesignConfiguration, DesignParameter,
    Feature, FeatureDefinition, FeatureId, FeatureSourceContent, FeatureTreeNodeRole, HoleKind,
    Length, ParameterId, ParameterValue, SketchSpace,
};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, BlendSupport, Curve, CurveGeometry, IntcurveSupportContext,
    IntcurveSupportSide, NurbsCurve, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceCurveFamily, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    AttributeId, BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId,
    ProceduralCurveId, ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::Point2;
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
        let ir = build_metadata_ir(&scan)?;
        let report = build_container_report(&scan, true);
        return Ok(DecodeResult::new(ir, report));
    }

    if let Some((ir, report)) = try_decode_geometry(&scan) {
        return Ok(DecodeResult::new(ir, report));
    }

    let ir = build_metadata_ir(&scan)?;
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

/// Decode analytic carriers from every Parasolid stream. Returns `None` when no
/// carrier of any kind passes its gate, so the caller falls back to metadata.
fn try_decode_geometry(scan: &Scan) -> Option<(CadIr, DecodeReport)> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    let mut counts = Counts::default();
    let mut body_node_ids = BTreeMap::new();
    let semantic_streams = semantic_streams(scan);
    let topology_streams = topology_streams(scan);

    for (si, stream) in scan.streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        let semantic = &semantic_streams[si];
        let stream_name = format!("parasolid#{si}:{}", stream.kind.label());
        let source_stream = annotations.stream(format!("nx:{stream_name}"));
        let graph = Graph::parse(&topology_streams[si]);
        body_node_ids.extend(topology_body_node_ids(si, &graph));
        let mut points_by_xmt = BTreeMap::new();
        let mut surfaces_by_xmt = BTreeMap::new();
        let mut curves_by_xmt = BTreeMap::new();
        let mut pcurves_by_xmt = BTreeMap::new();
        let mut pcurve_supports_by_xmt = BTreeMap::new();
        let mut trim_ranges = BTreeMap::new();
        let mut pending_blend_supports = Vec::new();
        let mut pending_blend_spines = Vec::new();
        let first_surface = ir.model.surfaces.len();
        let first_curve = ir.model.curves.len();
        let mut point_ordinal = 0usize;
        for pt in geometry::points(semantic) {
            let pi = point_ordinal;
            point_ordinal += 1;
            let pid = PointId(format!("nx:s{si}:pt#{pi}"));
            annotations
                .note(&pid, source_stream, pt.pos as u64)
                .tag("POINT");
            annotations.derived(&pid, "position");
            ir.model.points.push(Point {
                id: pid.clone(),
                position: pt.position,
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
                    points_by_xmt.insert(node.xmt, point_id);
                }
            }
            counts.points += 1;
        }
        for node in graph.of_kind(29) {
            if points_by_xmt.contains_key(&node.xmt) {
                continue;
            }
            let Some(position) = node.point_position() else {
                continue;
            };
            let pi = point_ordinal;
            point_ordinal += 1;
            let pid = PointId(format!("nx:s{si}:pt#{pi}"));
            annotate_node(&mut annotations, &pid, source_stream, node, "POINT");
            annotations.derived(&pid, "position");
            ir.model.points.push(Point {
                id: pid.clone(),
                position,
            });
            points_by_xmt.insert(node.xmt, pid);
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
                | SurfaceGeometry::Procedural { .. }
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
        for node in (50..=54).flat_map(|kind| graph.of_kind(kind)) {
            if surfaces_by_xmt.contains_key(&node.xmt) {
                continue;
            }
            let Some(geometry) = node.surface_geometry() else {
                continue;
            };
            match &geometry {
                SurfaceGeometry::Plane { .. } => counts.planes += 1,
                SurfaceGeometry::Cylinder { .. } => counts.cylinders += 1,
                SurfaceGeometry::Cone { .. } => counts.cones += 1,
                SurfaceGeometry::Sphere { .. } => counts.spheres += 1,
                SurfaceGeometry::Torus { .. } => counts.tori += 1,
                _ => unreachable!("fixed analytic surface family"),
            }
            let id = SurfaceId(format!("nx:s{si}:graph-surf#{}", node.xmt));
            annotate_node(
                &mut annotations,
                &id,
                source_stream,
                node,
                surface_tag(&geometry),
            );
            annotations.derived(&id, "geometry");
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry,
                source_object: None,
            });
            surfaces_by_xmt.insert(node.xmt, id);
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
            annotations.derived(&surface_id, "geometry");
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: procedural_id.clone(),
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
                    u_sense: 0,
                    v_sense: 0,
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
                source_object: None,
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
                        signed_radius: blend.offsets[0].abs(),
                    },
                    cross_section: BlendCrossSection::Circular,
                    native: None,
                },
                cache_fit_tolerance: None,
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

        for (ci, crv) in geometry::curves(semantic).into_iter().enumerate() {
            match &crv.geometry {
                CurveGeometry::Line { .. } => counts.lines += 1,
                CurveGeometry::Circle { .. } => counts.circles += 1,
                CurveGeometry::Ellipse { .. } => counts.ellipses += 1,
                CurveGeometry::Parabola { .. }
                | CurveGeometry::Hyperbola { .. }
                | CurveGeometry::Degenerate { .. }
                | CurveGeometry::Nurbs(_)
                | CurveGeometry::Procedural { .. }
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
        for node in (30..=32).flat_map(|kind| graph.of_kind(kind)) {
            if curves_by_xmt.contains_key(&node.xmt) {
                continue;
            }
            let Some(geometry) = node.curve_geometry() else {
                continue;
            };
            match &geometry {
                CurveGeometry::Line { .. } => counts.lines += 1,
                CurveGeometry::Circle { .. } => counts.circles += 1,
                CurveGeometry::Ellipse { .. } => counts.ellipses += 1,
                _ => unreachable!("fixed analytic curve family"),
            }
            let id = CurveId(format!("nx:s{si}:graph-crv#{}", node.xmt));
            annotate_node(
                &mut annotations,
                &id,
                source_stream,
                node,
                curve_tag(&geometry),
            );
            annotations.derived(&id, "geometry");
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry,
                source_object: None,
            });
            curves_by_xmt.insert(node.xmt, id);
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

        let intersection_scan = crate::intersection::scan(semantic);
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
                            charted.supports[0],
                            charted.support_uv[0]
                                .as_deref()
                                .filter(|uv| uv.len() == charted.parameters.len())
                                .map(|uv| (uv, charted.parameters.as_slice())),
                        );
                        let second = intersection_side(
                            &ir,
                            &surfaces_by_xmt,
                            charted.supports[1],
                            charted.support_uv[1]
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

        let trimmed_curves = crate::topology::trimmed_curves(semantic);
        let mut normalized_pcurves = BTreeSet::new();
        let surface_curves = crate::topology::surface_curves(semantic);
        loop {
            let mapped = curves_by_xmt.len() + pcurves_by_xmt.len() + pcurve_supports_by_xmt.len();
            for trim in &trimmed_curves {
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
            for surface_curve in &surface_curves {
                if let Some(pcurve) = pcurves_by_xmt.get(&surface_curve.pcurve).cloned() {
                    if normalized_pcurves.insert(pcurve.clone()) {
                        let support = surfaces_by_xmt
                            .get(&surface_curve.surface)
                            .and_then(|id| {
                                ir.model.surfaces.iter().find(|surface| surface.id == *id)
                            })
                            .map(|surface| surface.geometry.clone());
                        if let (Some(support), Some(carrier)) = (
                            support,
                            ir.model
                                .pcurves
                                .iter_mut()
                                .find(|candidate| candidate.id == pcurve),
                        ) {
                            normalize_pcurve_parameters(&mut carrier.geometry, &support);
                        }
                    }
                    if let Some(carrier) = ir.model.pcurves.iter_mut().find(|p| p.id == pcurve) {
                        carrier.fit_tolerance = Some(surface_curve.tolerance * 1000.0);
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
            &graph,
            &mut surfaces_by_xmt,
            &mut curves_by_xmt,
            &pcurves_by_xmt,
            source_stream,
            &mut annotations,
        );

        emit_topology(
            &mut ir,
            si,
            &graph,
            &points_by_xmt,
            &surfaces_by_xmt,
            &curves_by_xmt,
            &pcurves_by_xmt,
            &pcurve_supports_by_xmt,
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
        ir.push_native_unknown("nx", unknown).ok()?;
    }

    if counts.points == 0 && counts.surfaces() == 0 && counts.curves() == 0 {
        return None;
    }

    let mut active_body_selection = select_active_body(
        &mut ir,
        &body_node_ids,
        &scan.container.rmfastload_object_ids(),
    );
    if !active_body_selection {
        active_body_selection = select_terminal_feature_bodies(&mut ir, scan);
    }
    attach_native_object_model(&mut ir, scan, &mut annotations).ok()?;
    prune_unreferenced_unknown_carriers(&mut ir);
    classify_body_kinds(&mut ir);
    finalize_point_topology(&mut ir, &mut annotations);
    let referenced_pcurves: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .filter_map(|coedge| coedge.pcurve.clone())
        .collect();
    ir.model
        .pcurves
        .retain(|pcurve| referenced_pcurves.contains(&pcurve.id));
    ir.annotations = annotations.build();
    retain_live_annotations(&mut ir);
    retain_live_unknown_links(&mut ir);
    let report = build_geometry_report(
        scan,
        &counts,
        !ir.model.faces.is_empty(),
        ir.model.bodies.len() > 1 && !active_body_selection,
    );
    Some((ir, report))
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
            if let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            {
                used_surfaces.extend(context.sides.iter().filter_map(|side| side.surface.clone()));
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

pub(crate) fn semantic_streams(scan: &Scan) -> Vec<Vec<u8>> {
    let mut semantic = topology_streams(scan);
    for (partition, deltas) in paired_delta_streams(scan) {
        for delta in deltas {
            semantic[partition].extend_from_slice(&crate::deltas::procedural_residual(
                &scan.streams[delta].inflated,
            ));
            semantic[delta].clear();
        }
    }
    semantic
}

pub(crate) fn topology_streams(scan: &Scan) -> Vec<Vec<u8>> {
    let mut semantic = scan
        .streams
        .iter()
        .map(|stream| stream.inflated.clone())
        .collect::<Vec<_>>();
    for (partition, deltas) in paired_delta_streams(scan) {
        for delta in deltas {
            semantic[partition] =
                crate::deltas::merge_full_records(&semantic[partition], &semantic[delta]);
            semantic[delta].clear();
        }
    }
    semantic
}

fn paired_delta_streams(scan: &Scan) -> BTreeMap<usize, Vec<usize>> {
    let links = crate::native::segment_stream_links(&scan.container, &scan.streams);
    let linked_deltas = links
        .iter()
        .filter(|link| link.stream_kind == "deltas")
        .map(|link| link.stream_ordinal as usize)
        .collect::<BTreeSet<_>>();
    pair_stream_indices(&scan.streams, (!links.is_empty()).then_some(&linked_deltas))
}

pub(crate) fn pair_stream_indices(
    streams: &[Stream],
    eligible_deltas: Option<&BTreeSet<usize>>,
) -> BTreeMap<usize, Vec<usize>> {
    let mut pairs = BTreeMap::<usize, Vec<usize>>::new();
    for (delta, stream) in streams.iter().enumerate() {
        if stream.kind != StreamKind::Deltas
            || eligible_deltas.is_some_and(|eligible| !eligible.contains(&delta))
        {
            continue;
        }
        let partition = streams[..delta]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, candidate)| {
                candidate.kind == StreamKind::Partition && candidate.schema == stream.schema
            })
            .map(|(partition, _)| partition);
        if let Some(partition) = partition {
            pairs.entry(partition).or_default().push(delta);
        }
    }
    pairs
}

fn retain_live_annotations(ir: &mut CadIr) {
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
    if let Ok(unknowns) = ir.native_unknowns("nx") {
        ids.extend(unknowns.iter().map(|unknown| unknown.id.to_string()));
    }
    ir.annotations.provenance.retain(|id, _| ids.contains(id));
    ir.annotations.exactness.retain(|id, _| ids.contains(id));
}

fn retain_live_unknown_links(ir: &mut CadIr) {
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
    let Ok(mut unknowns) = ir.native_unknowns("nx") else {
        return;
    };
    let mut empty_links = Vec::new();
    for unknown in &mut unknowns {
        unknown.links.retain(|link| ids.contains(link));
        if unknown.links.is_empty() {
            empty_links.push(unknown.id.to_string());
        }
    }
    let _ = ir.set_native_unknowns("nx", &unknowns);
    for id in empty_links {
        if let Some(note) = ir.annotations.exactness.get_mut(&id) {
            note.fields.remove("links");
        }
    }
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

fn select_terminal_feature_bodies(ir: &mut CadIr, scan: &Scan) -> bool {
    if ir.model.bodies.len() <= 1 {
        return false;
    }
    let labels = crate::native::feature_operation_labels(&scan.container);
    let body_references = crate::native::feature_body_references(&scan.container);
    let booleans = crate::native::feature_boolean_operations(&scan.container);
    let bindings = crate::native::segment_body_bindings(&scan.container, &scan.streams);
    let body_reference_occurrences =
        crate::native::feature_body_reference_occurrences(&scan.container);
    let body_members = crate::native::feature_operation_body_members(&scan.container);
    let body_operands = crate::native::feature_operation_body_operands(
        &body_members,
        &body_reference_occurrences,
        &bindings,
    );
    if booleans.is_empty() && body_operands.is_empty() {
        return false;
    }
    let Some(terminal) = crate::native::terminal_feature_body_indices(
        &labels,
        &body_references,
        &booleans,
        &body_operands,
        &bindings,
    ) else {
        return false;
    };
    let mut mapped = BTreeSet::new();
    let mut selected = BTreeSet::new();
    for binding in bindings
        .iter()
        .filter(|binding| binding.stream_kind == "partition")
    {
        let identities = [binding.body_object_index, binding.body_alias_object_index];
        let statuses = identities
            .into_iter()
            .map(|identity| terminal.contains(&identity))
            .collect::<BTreeSet<_>>();
        if statuses.len() != 1 {
            return false;
        }
        let is_terminal = *statuses.first().expect("one terminal status");
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
        if is_terminal {
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
        .filter_map(|coedge| coedge.pcurve.clone())
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
            if let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            {
                surfaces.extend(context.sides.iter().filter_map(|side| side.surface.clone()));
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
    IntcurveSupportSide { surface, pcurve }
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
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Unknown { .. } => Point2::new(uv[0], uv[1]),
    }
}

fn normalize_pcurve_parameters(pcurve: &mut PcurveGeometry, surface: &SurfaceGeometry) {
    match pcurve {
        PcurveGeometry::Line { origin, direction } => {
            let end = Point2::new(origin.u + direction.u, origin.v + direction.v);
            let converted_origin = surface_parameters(surface, [origin.u, origin.v]);
            let converted_end = surface_parameters(surface, [end.u, end.v]);
            *origin = converted_origin;
            *direction = Point2::new(
                converted_end.u - converted_origin.u,
                converted_end.v - converted_origin.v,
            );
        }
        PcurveGeometry::Nurbs { control_points, .. } => {
            for point in control_points {
                *point = surface_parameters(surface, [point.u, point.v]);
            }
        }
    }
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
            if let Some((surface, pcurve, parameter_range, fit_tolerance)) = lifted {
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
                                },
                                IntcurveSupportSide {
                                    surface: None,
                                    pcurve: None,
                                },
                            ],
                            parameter_range,
                            discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                        },
                    },
                    cache_fit_tolerance: fit_tolerance,
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
            pcurve,
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
        CurveGeometry::Unknown { .. } => "UNKNOWN_CURVE",
    }
}

fn decoded_tolerance(value: f64) -> Option<f64> {
    match value {
        MISSING_TOLERANCE => None,
        value if value.is_finite() && value > 0.0 && value < 1.0e3 => Some(value * 1000.0),
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
        CurveGeometry::Line { .. } => Some([raw[0] * 1000.0, raw[1] * 1000.0]),
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

fn build_geometry_report(
    scan: &Scan,
    counts: &Counts,
    has_topology: bool,
    has_unresolved_sub_bodies: bool,
) -> DecodeReport {
    let mut losses = Vec::new();

    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Info,
        message: format!(
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
        ),
        provenance: None,
    });

    if !has_topology {
        losses.push(LossNote {
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

    if counts.intersection_rejections.total() > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
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
            ),
            provenance: None,
        });
    }

    if scan.count(StreamKind::Deltas) > 0 {
        losses.push(LossNote {
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: format!(
                "{} Parasolid deltas stream(s) were paired with the preceding equal-schema partition \
                 in validated UG_PART segment order. Exact-key \
                 BODY, SHELL, FACE, LOOP, FIN, EDGE, VERTEX, REGION, POINT, LINE, CIRCLE, ELLIPSE, PLANE, CYLINDER, CONE, SPHERE, TORUS, B_SURFACE, and B_CURVE full records and compact \
                 non-topology replacements and tombstones were applied using the last event for \
                 each key. Validated partition topology remained authoritative, including any \
                 point, curve, or surface carrier still referenced by surviving topology. \
                 Tombstones without an exact partition key remain unresolved.",
                scan.count(StreamKind::Deltas)
            ),
            provenance: None,
        });
    }

    if has_unresolved_sub_bodies {
        losses.push(LossNote {
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: format!(
                "This part is composed of {} sub-body partition(s); its decoded feature-history \
                 Booleans do not resolve every intermediate body object to a partition image. \
                 Carriers from all sub-bodies are emitted without the unresolved composition that \
                 would remove interior/construction faces.",
                scan.count(StreamKind::Partition)
            ),
            provenance: None,
        });
    }

    losses.push(LossNote {
        category: LossCategory::Attribute,
        severity: Severity::Warning,
        message: "Materials, appearances, entity-owned attributes, complete feature parameters, \
                  sketch geometry, constraints, and assembly occurrence placements were not transferred: \
                  they live in NX object-model per-class field serialization that is not decoded."
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

fn build_metadata_ir(scan: &Scan) -> Result<CadIr, CodecError> {
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
            ir.push_native_unknown("nx", unknown)?;
        }
    }
    attach_native_object_model(&mut ir, scan, &mut annotations)
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    ir.annotations = annotations.build();
    Ok(ir)
}

fn attach_native_object_model(
    ir: &mut CadIr,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let segment_index_rows = crate::native::segment_index_rows(&scan.container);
    let segment_om_links = crate::native::segment_om_links(&scan.container);
    let segment_stream_links = crate::native::segment_stream_links(&scan.container, &scan.streams);
    let segment_body_bindings =
        crate::native::segment_body_bindings(&scan.container, &scan.streams);
    let parasolid_attribute_definitions =
        crate::native::parasolid_attribute_definitions(&scan.streams);
    let om_record_areas = crate::native::om_record_areas(&scan.container);
    let feature_operation_labels = crate::native::feature_operation_labels(&scan.container);
    let feature_operation_records = crate::native::feature_operation_records(&scan.container);
    let feature_payload_strings = crate::native::feature_payload_strings(&scan.container);
    let feature_simple_hole_templates = crate::native::feature_simple_hole_templates(
        &feature_operation_labels,
        &feature_operation_records,
        &feature_payload_strings,
    );
    let feature_simple_hole_placements_2d =
        crate::native::feature_simple_hole_placements_2d(&scan.container);
    let feature_simple_hole_placement_block_references =
        crate::native::feature_simple_hole_placement_block_references(&scan.container);
    let feature_body_references = crate::native::feature_body_references(&scan.container);
    let feature_body_reference_occurrences =
        crate::native::feature_body_reference_occurrences(&scan.container);
    let feature_input_blocks = crate::native::feature_input_blocks(&scan.container);
    let feature_sketch_references = crate::native::feature_sketch_references(&scan.container);
    let feature_extrude_profile_references =
        crate::native::feature_extrude_profile_references(&scan.container);
    let feature_extrude_payload_headers =
        crate::native::feature_extrude_payload_headers(&scan.container);
    let feature_extrude_payload_scalar_triples =
        crate::native::feature_extrude_payload_scalar_triples(&scan.container);
    let feature_operation_body_scalar_triples =
        crate::native::feature_operation_body_scalar_triples(&scan.container);
    let feature_operation_body_members =
        crate::native::feature_operation_body_members(&scan.container);
    let feature_operation_body_operands = crate::native::feature_operation_body_operands(
        &feature_operation_body_members,
        &feature_body_reference_occurrences,
        &segment_body_bindings,
    );
    let feature_operation_body_11_continuations =
        crate::native::feature_operation_body_11_continuations(&scan.container);
    let feature_operation_body_reference_lanes =
        crate::native::feature_operation_body_reference_lanes(&scan.container);
    let feature_extrude_construction_profiles =
        crate::native::feature_extrude_construction_profiles(
            &feature_extrude_profile_references,
            &feature_operation_body_reference_lanes,
        );
    let feature_extrude_payload_32_branches =
        crate::native::feature_extrude_payload_32_branches(&scan.container);
    let feature_extrude_32_constructions = crate::native::feature_extrude_32_constructions(
        &feature_extrude_profile_references,
        &feature_extrude_payload_32_branches,
    );
    let feature_block_construction_references =
        crate::native::feature_block_construction_references(&scan.container);
    let feature_block_constructions =
        crate::native::feature_block_constructions(&feature_block_construction_references);
    let feature_sketch_records = crate::native::feature_sketch_records(
        &feature_operation_labels,
        &feature_operation_records,
        &feature_input_blocks,
        &feature_sketch_references,
    );
    let feature_sketch_construction_inputs = crate::native::feature_sketch_construction_inputs(
        &feature_sketch_records,
        &feature_sketch_references,
    );
    let feature_sketch_construction_payloads = crate::native::feature_sketch_construction_payloads(
        &scan.container,
        &feature_sketch_construction_inputs,
    );
    let feature_sketch_payload_scalars = crate::native::feature_sketch_payload_scalars(
        &scan.container,
        &feature_sketch_construction_inputs,
    );
    let feature_sketch_payload_names = crate::native::feature_sketch_payload_names(
        &scan.container,
        &feature_sketch_construction_inputs,
    );
    let feature_sketch_payload_named_records = crate::native::feature_sketch_payload_named_records(
        &feature_sketch_construction_payloads,
        &feature_sketch_payload_names,
        &feature_sketch_payload_scalars,
    );
    let feature_sketch_points = crate::native::feature_sketch_points(
        &feature_sketch_payload_named_records,
        &feature_sketch_payload_names,
        &feature_sketch_payload_scalars,
    );
    let feature_boolean_operations = crate::native::feature_boolean_operations(&scan.container);
    let expression_declarations = crate::native::expression_declarations(&scan.container);
    let expressions = crate::native::expressions(&scan.container);
    let classes = crate::native::class_definitions(&scan.container);
    let fields = crate::native::field_definitions(&scan.container);
    let object_records = crate::native::object_records(&scan.container);
    let data_blocks = crate::native::data_blocks(&scan.container);
    let data_block_control_values = crate::native::data_block_control_values(&scan.container);
    let data_block_control_index_values =
        crate::native::data_block_control_index_values(&scan.container);
    let data_block_control_references =
        crate::native::data_block_control_references(&scan.container);
    let data_block_control_handle_pairs =
        crate::native::data_block_control_handle_pairs(&data_block_control_references);
    let data_block_references = crate::native::data_block_references(&scan.container);
    let data_block_counted_index_lanes =
        crate::native::data_block_counted_index_lanes(&scan.container);
    let feature_parameter_bindings = crate::native::feature_parameter_bindings(
        &feature_input_blocks,
        &data_block_references,
        &expressions,
    );
    let store_headers = crate::native::store_headers(&scan.container);
    let string_values = crate::native::string_values(&scan.container);
    let object_references = crate::native::object_references(&scan.container);
    let configurations = crate::native::configurations(&scan.container);
    let part_attributes = crate::native::part_attributes(&scan.container);
    let external_references = crate::native::external_references(&scan.container);
    let external_reference_records = crate::native::external_reference_records(&scan.container);
    let persistent_handles = crate::native::persistent_handles(
        &object_references,
        &data_block_control_references,
        &external_reference_records,
    );
    let object_sections = scan.container.indexed_om_sections();
    if segment_index_rows.is_empty()
        && segment_om_links.is_empty()
        && segment_stream_links.is_empty()
        && segment_body_bindings.is_empty()
        && parasolid_attribute_definitions.is_empty()
        && om_record_areas.is_empty()
        && feature_operation_labels.is_empty()
        && feature_operation_records.is_empty()
        && feature_payload_strings.is_empty()
        && feature_simple_hole_templates.is_empty()
        && feature_simple_hole_placements_2d.is_empty()
        && feature_simple_hole_placement_block_references.is_empty()
        && feature_body_references.is_empty()
        && feature_input_blocks.is_empty()
        && feature_sketch_references.is_empty()
        && feature_extrude_profile_references.is_empty()
        && feature_extrude_payload_headers.is_empty()
        && feature_extrude_payload_scalar_triples.is_empty()
        && feature_operation_body_scalar_triples.is_empty()
        && feature_operation_body_members.is_empty()
        && feature_operation_body_operands.is_empty()
        && feature_operation_body_11_continuations.is_empty()
        && feature_operation_body_reference_lanes.is_empty()
        && feature_extrude_construction_profiles.is_empty()
        && feature_extrude_payload_32_branches.is_empty()
        && feature_extrude_32_constructions.is_empty()
        && feature_block_construction_references.is_empty()
        && feature_block_constructions.is_empty()
        && feature_sketch_records.is_empty()
        && feature_sketch_construction_inputs.is_empty()
        && feature_sketch_construction_payloads.is_empty()
        && feature_sketch_payload_scalars.is_empty()
        && feature_sketch_payload_names.is_empty()
        && feature_sketch_payload_named_records.is_empty()
        && feature_sketch_points.is_empty()
        && feature_boolean_operations.is_empty()
        && expression_declarations.is_empty()
        && expressions.is_empty()
        && classes.is_empty()
        && fields.is_empty()
        && object_records.is_empty()
        && data_blocks.is_empty()
        && data_block_control_values.is_empty()
        && data_block_control_index_values.is_empty()
        && data_block_control_references.is_empty()
        && data_block_control_handle_pairs.is_empty()
        && data_block_references.is_empty()
        && data_block_counted_index_lanes.is_empty()
        && feature_parameter_bindings.is_empty()
        && store_headers.is_empty()
        && string_values.is_empty()
        && object_references.is_empty()
        && persistent_handles.is_empty()
        && configurations.is_empty()
        && part_attributes.is_empty()
        && external_references.is_empty()
        && external_reference_records.is_empty()
        && object_sections.is_empty()
    {
        return Ok(());
    }
    let annotation_stream = annotations.stream("nx:container");
    for row in &segment_index_rows {
        annotations
            .note(&row.id, annotation_stream, row.source_offset)
            .tag("UG_PART_SEGMENT_INDEX_ROW");
        annotations.exactness(&row.id, Exactness::ByteExact);
    }
    for link in &segment_stream_links {
        annotations
            .note(&link.id, annotation_stream, link.source_offset)
            .tag("UG_PART_SEGMENT_STREAM_LINK");
        annotations.exactness(&link.id, Exactness::ByteExact);
    }
    for binding in &segment_body_bindings {
        annotations
            .note(&binding.id, annotation_stream, binding.source_offset)
            .tag("UG_PART_SEGMENT_BODY_BINDING");
        annotations.exactness(&binding.id, Exactness::ByteExact);
    }
    for definition in &parasolid_attribute_definitions {
        let source_stream = annotations.stream(format!("nx:s{}", definition.stream_ordinal));
        annotations
            .note(&definition.id, source_stream, definition.inflated_offset)
            .tag("ATTRIBUTE_DEFINITION");
        annotations.exactness(&definition.id, Exactness::ByteExact);
    }
    for link in &segment_om_links {
        annotations
            .note(&link.id, annotation_stream, link.source_offset)
            .tag("UG_PART_SEGMENT_OM_LINK");
        annotations.exactness(&link.id, Exactness::ByteExact);
    }
    for area in &om_record_areas {
        annotations
            .note(&area.id, annotation_stream, area.source_offset)
            .tag("OM_RECORD_AREA");
        annotations.exactness(&area.id, Exactness::ByteExact);
    }
    for label in &feature_operation_labels {
        annotations
            .note(&label.id, annotation_stream, label.source_offset)
            .tag("FEATURE_OPERATION_LABEL");
        annotations.exactness(&label.id, Exactness::ByteExact);
    }
    for sketch in &feature_sketch_records {
        annotations
            .note(&sketch.id, annotation_stream, sketch.source_offset)
            .tag("FEATURE_SKETCH_RECORD");
        annotations.exactness(&sketch.id, Exactness::Derived);
    }
    for record in &feature_operation_records {
        annotations
            .note(&record.id, annotation_stream, record.source_offset)
            .tag("FEATURE_OPERATION_RECORD");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for value in &feature_payload_strings {
        annotations
            .note(&value.id, annotation_stream, value.source_offset)
            .tag("FEATURE_PAYLOAD_STRING");
        annotations.exactness(&value.id, Exactness::ByteExact);
    }
    for reference in &feature_body_references {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("FEATURE_BODY_REFERENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for reference in &feature_body_reference_occurrences {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("FEATURE_BODY_REFERENCE_OCCURRENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for input in &feature_input_blocks {
        annotations
            .note(&input.id, annotation_stream, input.source_offset)
            .tag("FEATURE_INPUT_BLOCK");
        annotations.exactness(&input.id, Exactness::ByteExact);
    }
    for operation in &feature_boolean_operations {
        annotations
            .note(&operation.id, annotation_stream, operation.source_offset)
            .tag("FEATURE_BOOLEAN_OPERATION");
        annotations.exactness(&operation.id, Exactness::ByteExact);
    }
    for declaration in &expression_declarations {
        annotations
            .note(
                &declaration.id,
                annotation_stream,
                declaration.source_offset,
            )
            .tag("EXPRESSION_DECLARATION");
        annotations.exactness(&declaration.id, Exactness::ByteExact);
    }
    for value in &data_block_control_values {
        annotations
            .note(&value.id, annotation_stream, value.source_offset)
            .tag("OM_DATA_BLOCK_CONTROL_VALUE");
        annotations.exactness(&value.id, Exactness::ByteExact);
    }
    for value in &data_block_control_index_values {
        annotations
            .note(&value.id, annotation_stream, value.source_offset)
            .tag("OM_DATA_BLOCK_CONTROL_INDEX_VALUE");
        annotations.exactness(&value.id, Exactness::ByteExact);
    }
    for reference in &data_block_control_references {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("OM_DATA_BLOCK_CONTROL_REFERENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for pair in &data_block_control_handle_pairs {
        annotations
            .note(&pair.id, annotation_stream, pair.source_offset)
            .tag("OM_DATA_BLOCK_CONTROL_HANDLE_PAIR");
        annotations.exactness(&pair.id, Exactness::ByteExact);
    }
    for reference in &data_block_references {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("OM_DATA_BLOCK_REFERENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for binding in &feature_parameter_bindings {
        annotations
            .note(&binding.id, annotation_stream, binding.source_offset)
            .tag("FEATURE_PARAMETER_BINDING");
        annotations.exactness(&binding.id, Exactness::Derived);
    }
    for header in &store_headers {
        annotations
            .note(&header.id, annotation_stream, header.source_offset)
            .tag("OM_STORE_VERSION");
        annotations.exactness(&header.id, Exactness::ByteExact);
    }
    for reference in &external_references {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("EXTREFSTREAM_STRING");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for attribute in &part_attributes {
        annotations
            .note(&attribute.id, annotation_stream, attribute.source_offset)
            .tag("Attribute");
        annotations.exactness(&attribute.id, Exactness::ByteExact);
        let id = AttributeId(format!("{}:neutral", attribute.id));
        annotations
            .note(&id.0, annotation_stream, attribute.source_offset)
            .tag("Attribute");
        annotations.derived(&id.0, "target");
        annotations.derived(&id.0, "name");
        annotations.derived(&id.0, "values");
        ir.model.attributes.push(SourceAttribute {
            id,
            target: AttributeTarget::Document,
            name: attribute.title.clone(),
            values: vec![AttributeValue::String(attribute.value.clone())],
        });
    }
    for record in &external_reference_records {
        annotations
            .note(&record.id, annotation_stream, record.source_offset)
            .tag("EXTREFSTREAM_RECORD");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    let mut unknowns = ir.native_unknowns("nx")?;
    for (section_index, (entry, section)) in object_sections.iter().enumerate() {
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (record_index, record) in section
            .control
            .iter()
            .chain(section.records.iter())
            .enumerate()
        {
            let kind = if record.object_id.is_some() {
                "record"
            } else {
                "block"
            };
            let id = UnknownId(format!(
                "nx:om-section-{section_index}:{kind}#{record_index}"
            ));
            let offset = entry_offset + record.offset as u64;
            annotations
                .note(&id, annotation_stream, offset)
                .tag(if record.object_id.is_some() {
                    "OM_ENTITY_RECORD"
                } else {
                    "OM_DATA_BLOCK"
                });
            annotations.exactness(&id, Exactness::ByteExact);
            unknowns.push(UnknownRecord {
                id,
                offset,
                byte_len: record.bytes.len() as u64,
                sha256: sha256_hex(record.bytes),
                data: Some(record.bytes.to_vec()),
                links: Vec::new(),
            });
        }
    }
    ir.set_native_unknowns("nx", &unknowns)?;
    if !configurations.is_empty() {
        for (ordinal, configuration) in configurations.iter().enumerate() {
            let id = ConfigurationId(format!("nx:arrangements:configuration#{ordinal}"));
            annotations
                .note(&id.0, annotation_stream, configuration.source_offset)
                .tag("Arrangement");
            annotations.derived(&id.0, "ordinal");
            annotations.derived(&id.0, "active");
            annotations.derived(&id.0, "source_index");
            annotations.derived(&id.0, "name");
            annotations.derived(&id.0, "native_ref");
            ir.model.configurations.push(DesignConfiguration {
                id,
                ordinal: ordinal as u32,
                active: configuration.active,
                source_index: Some(ordinal as u32),
                name: configuration.name.clone(),
                material: None,
                properties: BTreeMap::new(),
                bodies: Vec::new(),
                native_ref: Some(configuration.id.clone()),
            });
        }
    }
    attach_feature_operations(
        ir,
        &FeatureOperationSources {
            labels: &feature_operation_labels,
            booleans: &feature_boolean_operations,
            body_references: &feature_body_references,
            body_reference_occurrences: &feature_body_reference_occurrences,
            input_blocks: &feature_input_blocks,
            sketch_references: &feature_sketch_references,
            extrude_profile_references: &feature_extrude_profile_references,
            extrude_construction_profiles: &feature_extrude_construction_profiles,
            operation_body_operands: &feature_operation_body_operands,
            sketch_construction_inputs: &feature_sketch_construction_inputs,
            block_constructions: &feature_block_constructions,
            extrude_32_constructions: &feature_extrude_32_constructions,
            parameter_bindings: &feature_parameter_bindings,
            expressions: &expressions,
            operation_records: &feature_operation_records,
            payload_strings: &feature_payload_strings,
            simple_hole_templates: &feature_simple_hole_templates,
            simple_hole_placements_2d: &feature_simple_hole_placements_2d,
            simple_hole_placement_block_references: &feature_simple_hole_placement_block_references,
            body_bindings: &segment_body_bindings,
        },
        annotations,
    );
    attach_expression_parameters(ir, &expressions, &expression_declarations, annotations);
    ir.model
        .features
        .sort_by(|first, second| first.id.cmp(&second.id));
    let namespace = ir.native.namespace_mut("nx");
    namespace.version = namespace.version.max(71);
    if !segment_index_rows.is_empty() {
        namespace.set_arena("segment_index_rows", &segment_index_rows)?;
    }
    if !segment_stream_links.is_empty() {
        namespace.set_arena("segment_stream_links", &segment_stream_links)?;
    }
    if !segment_body_bindings.is_empty() {
        namespace.set_arena("segment_body_bindings", &segment_body_bindings)?;
    }
    if !parasolid_attribute_definitions.is_empty() {
        namespace.set_arena(
            "parasolid_attribute_definitions",
            &parasolid_attribute_definitions,
        )?;
    }
    if !segment_om_links.is_empty() {
        namespace.set_arena("segment_om_links", &segment_om_links)?;
    }
    if !om_record_areas.is_empty() {
        namespace.set_arena("om_record_areas", &om_record_areas)?;
    }
    if !feature_operation_labels.is_empty() {
        namespace.set_arena("feature_operation_labels", &feature_operation_labels)?;
    }
    if !feature_operation_records.is_empty() {
        namespace.set_arena("feature_operation_records", &feature_operation_records)?;
    }
    if !feature_payload_strings.is_empty() {
        namespace.set_arena("feature_payload_strings", &feature_payload_strings)?;
    }
    if !feature_simple_hole_templates.is_empty() {
        namespace.set_arena(
            "feature_simple_hole_templates",
            &feature_simple_hole_templates,
        )?;
    }
    if !feature_simple_hole_placements_2d.is_empty() {
        namespace.set_arena(
            "feature_simple_hole_placements_2d",
            &feature_simple_hole_placements_2d,
        )?;
    }
    if !feature_simple_hole_placement_block_references.is_empty() {
        namespace.set_arena(
            "feature_simple_hole_placement_block_references",
            &feature_simple_hole_placement_block_references,
        )?;
    }
    if !feature_body_references.is_empty() {
        namespace.set_arena("feature_body_references", &feature_body_references)?;
    }
    if !feature_body_reference_occurrences.is_empty() {
        namespace.set_arena(
            "feature_body_reference_occurrences",
            &feature_body_reference_occurrences,
        )?;
    }
    if !feature_input_blocks.is_empty() {
        namespace.set_arena("feature_input_blocks", &feature_input_blocks)?;
    }
    if !feature_sketch_references.is_empty() {
        namespace.set_arena("feature_sketch_references", &feature_sketch_references)?;
    }
    if !feature_extrude_profile_references.is_empty() {
        namespace.set_arena(
            "feature_extrude_profile_references",
            &feature_extrude_profile_references,
        )?;
    }
    if !feature_extrude_payload_headers.is_empty() {
        namespace.set_arena(
            "feature_extrude_payload_headers",
            &feature_extrude_payload_headers,
        )?;
    }
    if !feature_extrude_payload_scalar_triples.is_empty() {
        namespace.set_arena(
            "feature_extrude_payload_scalar_triples",
            &feature_extrude_payload_scalar_triples,
        )?;
    }
    if !feature_operation_body_scalar_triples.is_empty() {
        namespace.set_arena(
            "feature_operation_body_scalar_triples",
            &feature_operation_body_scalar_triples,
        )?;
    }
    if !feature_operation_body_members.is_empty() {
        namespace.set_arena(
            "feature_operation_body_members",
            &feature_operation_body_members,
        )?;
    }
    if !feature_operation_body_operands.is_empty() {
        namespace.set_arena(
            "feature_operation_body_operands",
            &feature_operation_body_operands,
        )?;
    }
    if !feature_operation_body_11_continuations.is_empty() {
        namespace.set_arena(
            "feature_operation_body_11_continuations",
            &feature_operation_body_11_continuations,
        )?;
    }
    if !feature_operation_body_reference_lanes.is_empty() {
        namespace.set_arena(
            "feature_operation_body_reference_lanes",
            &feature_operation_body_reference_lanes,
        )?;
    }
    if !feature_extrude_construction_profiles.is_empty() {
        namespace.set_arena(
            "feature_extrude_construction_profiles",
            &feature_extrude_construction_profiles,
        )?;
    }
    if !feature_extrude_payload_32_branches.is_empty() {
        namespace.set_arena(
            "feature_extrude_payload_32_branches",
            &feature_extrude_payload_32_branches,
        )?;
    }
    if !feature_extrude_32_constructions.is_empty() {
        namespace.set_arena(
            "feature_extrude_32_constructions",
            &feature_extrude_32_constructions,
        )?;
    }
    if !feature_block_construction_references.is_empty() {
        namespace.set_arena(
            "feature_block_construction_references",
            &feature_block_construction_references,
        )?;
    }
    if !feature_block_constructions.is_empty() {
        namespace.set_arena("feature_block_constructions", &feature_block_constructions)?;
    }
    if !feature_sketch_records.is_empty() {
        namespace.set_arena("feature_sketch_records", &feature_sketch_records)?;
    }
    if !feature_sketch_construction_inputs.is_empty() {
        namespace.set_arena(
            "feature_sketch_construction_inputs",
            &feature_sketch_construction_inputs,
        )?;
    }
    if !feature_sketch_construction_payloads.is_empty() {
        namespace.set_arena(
            "feature_sketch_construction_payloads",
            &feature_sketch_construction_payloads,
        )?;
    }
    if !feature_sketch_payload_scalars.is_empty() {
        namespace.set_arena(
            "feature_sketch_payload_scalars",
            &feature_sketch_payload_scalars,
        )?;
    }
    if !feature_sketch_payload_names.is_empty() {
        namespace.set_arena(
            "feature_sketch_payload_names",
            &feature_sketch_payload_names,
        )?;
    }
    if !feature_sketch_payload_named_records.is_empty() {
        namespace.set_arena(
            "feature_sketch_payload_named_records",
            &feature_sketch_payload_named_records,
        )?;
    }
    if !feature_sketch_points.is_empty() {
        namespace.set_arena("feature_sketch_points", &feature_sketch_points)?;
    }
    if !feature_boolean_operations.is_empty() {
        namespace.set_arena("feature_boolean_operations", &feature_boolean_operations)?;
    }
    if !expression_declarations.is_empty() {
        namespace.set_arena("expression_declarations", &expression_declarations)?;
    }
    if !expressions.is_empty() {
        namespace.set_arena("expressions", &expressions)?;
    }
    if !classes.is_empty() {
        namespace.set_arena("class_definitions", &classes)?;
    }
    if !fields.is_empty() {
        namespace.set_arena("field_definitions", &fields)?;
    }
    if !object_records.is_empty() {
        namespace.set_arena("object_records", &object_records)?;
    }
    if !data_blocks.is_empty() {
        namespace.set_arena("data_blocks", &data_blocks)?;
    }
    if !data_block_control_values.is_empty() {
        namespace.set_arena("data_block_control_values", &data_block_control_values)?;
    }
    if !data_block_control_index_values.is_empty() {
        namespace.set_arena(
            "data_block_control_index_values",
            &data_block_control_index_values,
        )?;
    }
    if !data_block_control_references.is_empty() {
        namespace.set_arena(
            "data_block_control_references",
            &data_block_control_references,
        )?;
    }
    if !data_block_control_handle_pairs.is_empty() {
        namespace.set_arena(
            "data_block_control_handle_pairs",
            &data_block_control_handle_pairs,
        )?;
    }
    if !data_block_references.is_empty() {
        namespace.set_arena("data_block_references", &data_block_references)?;
    }
    if !data_block_counted_index_lanes.is_empty() {
        namespace.set_arena(
            "data_block_counted_index_lanes",
            &data_block_counted_index_lanes,
        )?;
    }
    if !feature_parameter_bindings.is_empty() {
        namespace.set_arena("feature_parameter_bindings", &feature_parameter_bindings)?;
    }
    if !store_headers.is_empty() {
        namespace.set_arena("store_headers", &store_headers)?;
    }
    if !string_values.is_empty() {
        namespace.set_arena("string_values", &string_values)?;
    }
    if !object_references.is_empty() {
        namespace.set_arena("object_references", &object_references)?;
    }
    if !persistent_handles.is_empty() {
        namespace.set_arena("persistent_handles", &persistent_handles)?;
    }
    if !configurations.is_empty() {
        namespace.set_arena("configurations", &configurations)?;
    }
    if !part_attributes.is_empty() {
        namespace.set_arena("part_attributes", &part_attributes)?;
    }
    if !external_references.is_empty() {
        namespace.set_arena("external_references", &external_references)?;
    }
    if !external_reference_records.is_empty() {
        namespace.set_arena("external_reference_records", &external_reference_records)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct FeatureOperationSources<'a> {
    labels: &'a [crate::native::FeatureOperationLabel],
    booleans: &'a [crate::native::FeatureBooleanOperation],
    body_references: &'a [crate::native::FeatureBodyReference],
    body_reference_occurrences: &'a [crate::native::FeatureBodyReferenceOccurrence],
    input_blocks: &'a [crate::native::FeatureInputBlock],
    sketch_references: &'a [crate::native::FeatureSketchReference],
    extrude_profile_references: &'a [crate::native::FeatureExtrudeProfileReference],
    extrude_construction_profiles: &'a [crate::native::FeatureExtrudeConstructionProfile],
    operation_body_operands: &'a [crate::native::FeatureOperationBodyOperand],
    sketch_construction_inputs: &'a [crate::native::FeatureSketchConstructionInputs],
    block_constructions: &'a [crate::native::FeatureBlockConstruction],
    extrude_32_constructions: &'a [crate::native::FeatureExtrude32Construction],
    parameter_bindings: &'a [crate::native::FeatureParameterBinding],
    expressions: &'a [crate::native::Expression],
    operation_records: &'a [crate::native::FeatureOperationRecord],
    payload_strings: &'a [crate::native::FeaturePayloadString],
    simple_hole_templates: &'a [crate::native::FeatureSimpleHoleTemplate],
    simple_hole_placements_2d: &'a [crate::native::FeatureSimpleHolePlacement2d],
    simple_hole_placement_block_references:
        &'a [crate::native::FeatureSimpleHolePlacementBlockReferences],
    body_bindings: &'a [crate::native::SegmentBodyBinding],
}

fn attach_feature_operations(
    ir: &mut CadIr,
    sources: &FeatureOperationSources<'_>,
    annotations: &mut AnnotationBuilder,
) {
    let FeatureOperationSources {
        labels,
        booleans,
        body_references,
        body_reference_occurrences,
        input_blocks,
        sketch_references,
        extrude_profile_references,
        extrude_construction_profiles,
        operation_body_operands,
        sketch_construction_inputs,
        block_constructions,
        extrude_32_constructions,
        parameter_bindings,
        expressions,
        operation_records,
        payload_strings,
        simple_hole_templates,
        simple_hole_placements_2d,
        simple_hole_placement_block_references,
        body_bindings,
    } = *sources;
    let stream = annotations.stream("nx:container");
    let base_ordinal = ir.model.features.len() as u64;
    let booleans = booleans
        .iter()
        .map(|operation| (operation.operation_label.as_str(), operation))
        .collect::<BTreeMap<_, _>>();
    let body_references = body_references
        .iter()
        .map(|reference| {
            (
                reference.operation_label.as_str(),
                reference.body_object_index,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut body_reference_occurrences_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureBodyReferenceOccurrence>>::new();
    for reference in body_reference_occurrences {
        body_reference_occurrences_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut last_body_writer = BTreeMap::<u32, FeatureId>::new();
    let body_alias_roots = crate::native::body_alias_roots(body_bindings).unwrap_or_default();
    let canonical_body =
        |identity: u32| body_alias_roots.get(&identity).copied().unwrap_or(identity);
    let mut input_blocks_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureInputBlock>>::new();
    for input in input_blocks {
        input_blocks_by_operation
            .entry(input.operation_label.as_str())
            .or_default()
            .push(input);
    }
    let mut sketch_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchReference>>::new();
    for reference in sketch_references {
        sketch_references_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut extrude_profile_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureExtrudeProfileReference>>::new();
    for reference in extrude_profile_references {
        extrude_profile_references_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let extrude_construction_profiles_by_operation = extrude_construction_profiles
        .iter()
        .map(|profile| (profile.operation_label.as_str(), profile))
        .collect::<BTreeMap<_, _>>();
    let mut operation_body_operands_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureOperationBodyOperand>>::new();
    for operand in operation_body_operands {
        operation_body_operands_by_operation
            .entry(operand.operation_label.as_str())
            .or_default()
            .push(operand);
    }
    let sketch_construction_inputs_by_operation = sketch_construction_inputs
        .iter()
        .map(|inputs| (inputs.operation_label.as_str(), inputs))
        .collect::<BTreeMap<_, _>>();
    let block_constructions_by_operation = block_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        .collect::<BTreeMap<_, _>>();
    let extrude_32_constructions_by_operation = extrude_32_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        .collect::<BTreeMap<_, _>>();
    let mut parameter_bindings_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureParameterBinding>>::new();
    for binding in parameter_bindings {
        parameter_bindings_by_operation
            .entry(binding.operation_label.as_str())
            .or_default()
            .push(binding);
    }
    let operation_labels_by_record = operation_records
        .iter()
        .map(|record| (record.id.as_str(), record.operation_label.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut payload_strings_by_operation = BTreeMap::<&str, Vec<&str>>::new();
    for value in payload_strings {
        let Some(operation) = operation_labels_by_record.get(value.operation_record.as_str())
        else {
            continue;
        };
        payload_strings_by_operation
            .entry(operation)
            .or_default()
            .push(value.value.as_str());
    }
    let mut bodies_by_object_index = BTreeMap::<u32, Vec<BodyId>>::new();
    for binding in body_bindings {
        let prefix = format!("nx:s{}:", binding.stream_ordinal);
        let mut stream_bodies = Vec::new();
        for body in ir
            .model
            .bodies
            .iter()
            .filter(|body| body.id.0.starts_with(&prefix))
        {
            if !stream_bodies.contains(&body.id) {
                stream_bodies.push(body.id.clone());
            }
        }
        for identity in [binding.body_object_index, binding.body_alias_object_index] {
            let bodies = bodies_by_object_index.entry(identity).or_default();
            for body in &stream_bodies {
                if !bodies.contains(body) {
                    bodies.push(body.clone());
                }
            }
        }
    }
    for (ordinal, label) in labels.iter().enumerate() {
        let key = label.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let id = FeatureId(format!("nx:feature-history:feature#{key}"));
        let mut dependencies = Vec::new();
        if let Some(body) = body_references.get(label.id.as_str()) {
            if let Some(writer) = last_body_writer.get(&canonical_body(*body)) {
                dependencies.push(writer.clone());
            }
        }
        if let Some(operation) = booleans.get(label.id.as_str()) {
            for body in &operation.tool_object_indices {
                if let Some(writer) = last_body_writer.get(&canonical_body(*body)) {
                    if !dependencies.contains(writer) {
                        dependencies.push(writer.clone());
                    }
                }
            }
        }
        for operand in operation_body_operands_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            if let Some(writer) =
                last_body_writer.get(&canonical_body(operand.operand_object_index))
            {
                if !dependencies.contains(writer) {
                    dependencies.push(writer.clone());
                }
            }
        }
        let mut source_properties = BTreeMap::new();
        let outputs = body_references
            .get(label.id.as_str())
            .map_or_else(Vec::new, |body| {
                feature_body_outputs(*body, &bodies_by_object_index)
            });
        if let Some(body) = body_references.get(label.id.as_str()) {
            source_properties.insert("primary_body_object_index".to_string(), body.to_string());
        }
        for reference in body_reference_occurrences_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("body_reference.{}", reference.ordinal),
                reference.body_object_index.to_string(),
            );
        }
        if let Some(inputs) = sketch_construction_inputs_by_operation.get(label.id.as_str()) {
            source_properties.insert("sketch_construction_inputs".to_string(), inputs.id.clone());
        }
        if let Some(construction) = block_constructions_by_operation.get(label.id.as_str()) {
            source_properties.insert("block_construction".to_string(), construction.id.clone());
        }
        if let Some(construction) = extrude_32_constructions_by_operation.get(label.id.as_str()) {
            source_properties.insert(
                "extrude_32_construction".to_string(),
                construction.id.clone(),
            );
        }
        source_properties.extend(simple_hole_native_properties(
            &label.id,
            simple_hole_templates,
            simple_hole_placements_2d,
            simple_hole_placement_block_references,
        ));
        for (slot, value) in label.object_indices.iter().enumerate() {
            source_properties.insert(
                format!("object_index.{slot}"),
                value.map_or_else(|| "null".to_string(), |value| value.to_string()),
            );
        }
        for input in input_blocks_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("input_block.{}", input.input_slot),
                input.data_block.clone(),
            );
        }
        for reference in sketch_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("sketch_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        for reference in extrude_profile_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("extrude_profile_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        if let Some(profile) = extrude_construction_profiles_by_operation.get(label.id.as_str()) {
            source_properties.insert(
                "extrude_construction_profile".to_string(),
                profile.id.clone(),
            );
        }
        for operand in operation_body_operands_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("operation_body_operand.{}", operand.ordinal),
                operand.operand_object_index.to_string(),
            );
        }
        for binding in parameter_bindings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "input_parameter_declaration.{}.{}",
                    binding.input_slot, binding.reference_ordinal
                ),
                binding.expression_declaration.clone(),
            );
        }
        let operation_payload_strings = payload_strings_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let definition = booleans.get(label.id.as_str()).map_or_else(
            || non_boolean_feature_definition(&label.value, operation_payload_strings),
            |operation| FeatureDefinition::Combine {
                target: feature_body_selection(
                    &[operation.target_object_index],
                    &bodies_by_object_index,
                    format!("nx:om-object-index#{}", operation.target_object_index),
                ),
                tools: feature_body_selection(
                    &operation.tool_object_indices,
                    &bodies_by_object_index,
                    format!(
                        "nx:om-object-indices#{}",
                        operation
                            .tool_object_indices
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                ),
                op: match operation.kind {
                    crate::native::FeatureBooleanKind::Unite => BooleanOp::Join,
                    crate::native::FeatureBooleanKind::Subtract => BooleanOp::Cut,
                    crate::native::FeatureBooleanKind::Intersect => BooleanOp::Intersect,
                },
            },
        );
        annotations
            .note(&id, stream, label.source_offset)
            .tag("FEATURE_OPERATION");
        annotations.exactness(&id, Exactness::Derived);
        let mut source_content = Vec::new();
        source_content.extend(
            feature_parameter_content(
                parameter_bindings_by_operation
                    .get(label.id.as_str())
                    .map_or([].as_slice(), Vec::as_slice),
                expressions,
            )
            .into_iter()
            .map(FeatureSourceContent::Parameter),
        );
        source_content.extend(
            operation_payload_strings
                .iter()
                .map(|value| FeatureSourceContent::Text((*value).to_string())),
        );
        if source_content
            .iter()
            .any(|content| matches!(content, FeatureSourceContent::Parameter(_)))
        {
            annotations.derived(&id, "source_content");
        }
        ir.model.features.push(Feature {
            id: id.clone(),
            ordinal: base_ordinal + ordinal as u64,
            name: Some(label.value.clone()),
            suppressed: false,
            parent: None,
            dependencies,
            source_properties,
            source_tag: Some(label.value.clone()),
            source_text: None,
            source_content,
            outputs,
            definition,
            native_ref: Some(label.id.clone()),
        });
        if let Some(body) = body_references.get(label.id.as_str()) {
            last_body_writer.insert(canonical_body(*body), id);
        }
    }
}

pub(crate) fn simple_hole_native_properties(
    operation_label: &str,
    templates: &[crate::native::FeatureSimpleHoleTemplate],
    placements: &[crate::native::FeatureSimpleHolePlacement2d],
    block_references: &[crate::native::FeatureSimpleHolePlacementBlockReferences],
) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::new();
    if let Some(template) = templates
        .iter()
        .find(|template| template.operation_label == operation_label)
    {
        properties.insert("simple_hole_template".to_string(), template.id.clone());
    }
    if let Some(placement) = placements
        .iter()
        .find(|placement| placement.operation_label == operation_label)
    {
        properties.insert("simple_hole_placement_2d".to_string(), placement.id.clone());
    }
    if let Some(references) = block_references
        .iter()
        .find(|references| references.operation_label == operation_label)
    {
        properties.insert(
            "simple_hole_placement_block_references".to_string(),
            references.id.clone(),
        );
    }
    properties
}

pub(crate) fn feature_parameter_content(
    bindings: &[&crate::native::FeatureParameterBinding],
    expressions: &[crate::native::Expression],
) -> Vec<ParameterId> {
    let parameters_by_expression = expressions
        .iter()
        .filter_map(|expression| {
            let (section, key) = expression.id.rsplit_once(":expression#")?;
            Some((
                expression.id.as_str(),
                ParameterId(format!("{section}:parameter#{key}")),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    bindings
        .iter()
        .filter_map(|binding| {
            parameters_by_expression
                .get(binding.expression.as_deref()?)
                .filter(|parameter| seen.insert((*parameter).clone()))
                .cloned()
        })
        .collect()
}

pub(crate) fn non_boolean_feature_definition(
    kind: &str,
    payload_strings: &[&str],
) -> FeatureDefinition {
    match kind {
        "SKETCH" => FeatureDefinition::Sketch {
            space: SketchSpace::Planar,
            sketch: None,
        },
        "SIMPLE HOLE" => FeatureDefinition::Hole {
            face: None,
            position: None,
            direction: None,
            kind: HoleKind::Simple,
            diameter: None,
            extent: simple_hole_extent(payload_strings),
        },
        _ => FeatureDefinition::Native {
            kind: kind.to_string(),
            parameters: BTreeMap::new(),
            properties: BTreeMap::new(),
        },
    }
}

fn simple_hole_extent(payload_strings: &[&str]) -> Option<cadmpeg_ir::features::Extent> {
    payload_strings
        .iter()
        .find_map(|value| crate::native::parse_simple_hole_template(value))
        .map(|_| cadmpeg_ir::features::Extent::ThroughAll)
}

pub(crate) fn feature_body_selection(
    object_indices: &[u32],
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> BodySelection {
    let mut bodies = Vec::new();
    for index in object_indices {
        let Some(bound) = bodies_by_object_index
            .get(index)
            .filter(|bound| !bound.is_empty())
        else {
            return BodySelection::Native(native);
        };
        for body in bound {
            if !bodies.contains(body) {
                bodies.push(body.clone());
            }
        }
    }
    BodySelection::Resolved { bodies, native }
}

pub(crate) fn feature_body_outputs(
    object_index: u32,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Vec<BodyId> {
    bodies_by_object_index
        .get(&object_index)
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn attach_expression_parameters(
    ir: &mut CadIr,
    expressions: &[crate::native::Expression],
    declarations: &[crate::native::ExpressionDeclaration],
    annotations: &mut AnnotationBuilder,
) {
    let declarations = declarations
        .iter()
        .map(|declaration| (declaration.id.as_str(), declaration))
        .collect::<BTreeMap<_, _>>();
    let mut sections = BTreeMap::<String, Vec<&crate::native::Expression>>::new();
    for expression in expressions {
        let Some((section, _)) = expression.id.split_once(":expression#") else {
            continue;
        };
        sections
            .entry(section.to_string())
            .or_default()
            .push(expression);
    }
    let stream = annotations.stream("nx:container");
    let base_ordinal = ir.model.features.len() as u64;
    for (section_ordinal, (section, expressions)) in sections.into_iter().enumerate() {
        let feature_id = FeatureId(format!("{section}:feature#equations"));
        let first_offset = expressions
            .iter()
            .map(|expression| expression.source_offset)
            .min()
            .unwrap_or(0);
        annotations
            .note(&feature_id, stream, first_offset)
            .tag("hostglobalvariables");
        annotations.exactness(&feature_id, Exactness::Derived);
        ir.model.features.push(Feature {
            id: feature_id.clone(),
            ordinal: base_ordinal + section_ordinal as u64,
            name: Some("NX expressions".to_string()),
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("hostglobalvariables".to_string()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::Equations,
            },
            native_ref: None,
        });
        let mut parameter_ids = BTreeMap::<String, Vec<ParameterId>>::new();
        for expression in &expressions {
            let key = expression
                .id
                .rsplit_once('#')
                .map_or("unknown", |(_, key)| key);
            parameter_ids
                .entry(expression.name.clone())
                .or_default()
                .push(ParameterId(format!("{section}:parameter#{key}")));
        }
        for (ordinal, expression) in expressions.into_iter().enumerate() {
            let key = expression
                .id
                .rsplit_once('#')
                .map_or("unknown", |(_, key)| key);
            let id = ParameterId(format!("{section}:parameter#{key}"));
            annotations
                .note(&id.0, stream, expression.source_offset)
                .tag("Number");
            annotations.derived(&id.0, "owner");
            annotations.derived(&id.0, "ordinal");
            annotations.derived(&id.0, "value");
            annotations.derived(&id.0, "native_ref");
            let mut seen_dependencies = BTreeSet::new();
            let dependencies = crate::native::expression_parameter_names(&expression.expression)
                .into_iter()
                .filter_map(|name| {
                    let candidates = parameter_ids.get(name)?;
                    (candidates.len() == 1).then(|| candidates[0].clone())
                })
                .filter(|dependency| seen_dependencies.insert(dependency.clone()))
                .collect::<Vec<_>>();
            if !dependencies.is_empty() {
                annotations.derived(&id.0, "dependencies");
            }
            let value = expression.value.map(|value| match expression.unit {
                crate::native::ExpressionUnit::Millimeter => ParameterValue::Length(Length(value)),
                crate::native::ExpressionUnit::Degree => {
                    ParameterValue::Angle(Angle(value.to_radians()))
                }
            });
            let mut properties = BTreeMap::new();
            if let Some(declaration) = expression
                .declaration
                .as_deref()
                .and_then(|id| declarations.get(id))
            {
                properties.insert("declaration".to_string(), declaration.id.clone());
                properties.insert(
                    "declaration_object_id".to_string(),
                    declaration.object_id.to_string(),
                );
                annotations.derived(&id.0, "properties");
            }
            ir.model.parameters.push(DesignParameter {
                id,
                owner: feature_id.clone(),
                ordinal: ordinal as u32,
                name: expression.name.clone(),
                expression: expression.expression.clone(),
                display: None,
                value,
                dependencies,
                properties,
                pmi: None,
                native_ref: Some(expression.id.clone()),
            });
        }
    }
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
