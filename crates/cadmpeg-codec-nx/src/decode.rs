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
    analytic_surface_parameters, curve_point, curve_second_derivative, curve_tangent,
    model_surface_partials_by_id, model_surface_point_by_id, nurbs_curve_speed_bound,
    nurbs_surface_isocurve, nurbs_surface_partials, pcurve_tangent, pcurve_uv, surface_partials,
    surface_second_partials,
};
use cadmpeg_ir::features::{
    BodyRetentionMode, BodySelection, BodyTrimSide, BooleanOp, ChamferSpec,
    CurveProjectionDirection, CurveProjectionDirectionState, DatumPlaneReference, EdgeSelection,
    ExtrudeExtent, ExtrudeStart, FaceSelection, FeatureDefinition, FeatureId, HoleKind, Length,
    LoftPointSection, LoftSection, ParameterId, PathRef, PatternKind, ProfileRef, RadiusSpec,
    RevolutionConstruction, RevolveExtent, RibConstruction, RibDraft, SketchSpace, SweepMode,
    SweepOrientation, Termination, TrimRegion, VertexSelection,
};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, BlendSupport, Curve, CurveGeometry, IntcurveSupportContext,
    IntcurveSupportSide, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceCurveFamily, SurfaceGeometry, SurfaceParameterAxis,
    TolerantIntersectionParameterization,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossCode, LossNote, Severity};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::{self, Container};
use crate::geometry;
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
    let container = container::scan_bytes(root.window().to_vec())?;
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
        return decode_result(ir, report, annotations, unknowns);
    }

    if let Some((ir, report, annotations, unknowns)) = try_decode_geometry(&scan) {
        return decode_result(ir, report, annotations, unknowns);
    }

    let (ir, annotations, unknowns) = build_metadata_ir(&scan)?;
    let mut report = build_container_report(&scan, false);
    report_untransferred_streams(&scan, &mut report);
    decode_result(ir, report, annotations, unknowns)
}

fn decode_result(
    mut ir: CadIr,
    report: DecodeReport,
    annotations: cadmpeg_ir::Annotations,
    unknowns: Vec<UnknownRecord>,
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
    let (control_count, classified_control_count) = offset_store_control_counts(&scan.container);
    if classified_control_count != control_count {
        report.losses.push(LossNote {
            code: LossCode::RecordNotTyped,
            category: LossCategory::DesignIntent,
            severity: Severity::Warning,
            message: format!(
                "{} of {control_count} bounded offset-store control block(s) have no admitted complete grammar.",
                control_count - classified_control_count
            ),
            provenance: None,
        });
    }
    for entry in &scan.container.entries {
        let content = entry.content();
        if content.retains_opaque_payload() {
            report.losses.push(LossNote {
                code: LossCode::RecordNotTyped,
                category: LossCategory::Other,
                severity: Severity::Info,
                message: format!(
                    "Named container stream {} is classified as {} and retained byte-exact; its field semantics are not typed.",
                    entry.name,
                    content.label()
                ),
                provenance: None,
            });
        }
    }
    for (index, stream) in scan.streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            report.losses.push(LossNote {
                code: LossCode::PassthroughRecordOmitted,
                category: LossCategory::Other,
                severity: Severity::Info,
                message: format!(
                    "Non-Parasolid {} stream #{index} was classified but not transferred.",
                    stream.kind.label()
                ),
                provenance: None,
            });
        }
    }
}

fn offset_store_control_counts(container: &Container) -> (usize, usize) {
    container
        .indexed_om_sections()
        .into_iter()
        .filter_map(|(_, section)| section.control)
        .fold((0, 0), |(total, classified), control| {
            (
                total + 1,
                classified
                    + usize::from(crate::om::offset_store_control_form(control.bytes).is_some()),
            )
        })
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

fn saved_offset_carriers(
    ir: &CadIr,
    graph: &Graph,
    offsets: &[crate::topology::OffsetSurface],
    surfaces_by_xmt: &BTreeMap<u32, SurfaceId>,
    tolerance: f64,
) -> BTreeMap<u32, (SurfaceId, f64)> {
    if !tolerance.is_finite() || tolerance < 0.0 {
        return BTreeMap::new();
    }
    let face_surfaces = graph
        .of_kind(14)
        .filter_map(Node::face_fields)
        .map(|face| face.surface)
        .collect::<BTreeSet<_>>();
    let candidates = face_surfaces
        .iter()
        .filter_map(|xmt| surfaces_by_xmt.get(xmt))
        .filter_map(|id| {
            let geometry = &ir
                .model
                .surfaces
                .iter()
                .find(|surface| &surface.id == id)?
                .geometry;
            matches!(geometry, SurfaceGeometry::Nurbs(_)).then_some((id, geometry))
        })
        .collect::<Vec<_>>();

    let mut matches = BTreeMap::<u32, Vec<(SurfaceId, f64)>>::new();
    let mut candidate_owners = BTreeMap::<SurfaceId, Vec<u32>>::new();
    for offset in offsets
        .iter()
        .filter(|offset| !face_surfaces.contains(&offset.xmt))
    {
        let Some(support_id) = surfaces_by_xmt.get(&offset.support) else {
            continue;
        };
        let Some(support) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| &surface.id == support_id)
            .map(|surface| &surface.geometry)
        else {
            continue;
        };
        for (candidate_id, candidate) in &candidates {
            if *candidate_id == support_id {
                continue;
            }
            if let Some(fit) =
                certified_offset_cache_fit(support, candidate, offset.distance, tolerance)
            {
                matches
                    .entry(offset.xmt)
                    .or_default()
                    .push(((*candidate_id).clone(), fit));
                candidate_owners
                    .entry((*candidate_id).clone())
                    .or_default()
                    .push(offset.xmt);
            }
        }
    }

    matches
        .into_iter()
        .filter_map(|(offset, candidates)| {
            let [(candidate, fit)] = candidates.as_slice() else {
                return None;
            };
            (candidate_owners.get(candidate).map(Vec::as_slice) == Some(&[offset][..]))
                .then(|| (offset, (candidate.clone(), *fit)))
        })
        .collect()
}

fn certified_offset_cache_fit(
    support: &SurfaceGeometry,
    candidate: &SurfaceGeometry,
    distance: f64,
    tolerance: f64,
) -> Option<f64> {
    let (SurfaceGeometry::Nurbs(support), SurfaceGeometry::Nurbs(candidate)) = (support, candidate)
    else {
        return None;
    };
    let compatible_parameterization = support.u_degree > 0
        && support.v_degree > 0
        && candidate.u_degree > 0
        && candidate.v_degree > 0
        && support.u_periodic == candidate.u_periodic
        && support.v_periodic == candidate.v_periodic
        && nurbs_active_domain(support)
            .zip(nurbs_active_domain(candidate))
            .is_some_and(|(support, candidate)| support == candidate)
        && support
            .weights
            .as_ref()
            .is_none_or(|weights| weights.len() == support.control_points.len())
        && candidate
            .weights
            .as_ref()
            .is_none_or(|weights| weights.len() == candidate.control_points.len())
        && positive_weights(support.weights.as_deref())
        && positive_weights(candidate.weights.as_deref());
    if !compatible_parameterization
        || !distance.is_finite()
        || !tolerance.is_finite()
        || tolerance < 0.0
    {
        return None;
    }
    let same_basis = candidate.u_degree == support.u_degree
        && candidate.v_degree == support.v_degree
        && support.u_knots == candidate.u_knots
        && support.v_knots == candidate.v_knots
        && support.u_count == candidate.u_count
        && support.v_count == candidate.v_count
        && support.weights == candidate.weights
        && support.control_points.len() == candidate.control_points.len();
    if same_basis {
        if let Some(normal) = translation_net_normal(support) {
            let translation = Vector3::new(
                distance * normal.x,
                distance * normal.y,
                distance * normal.z,
            );
            let maximum_error = support
                .control_points
                .iter()
                .zip(&candidate.control_points)
                .map(|(support, candidate)| {
                    let expected = Point3::new(
                        support.x + translation.x,
                        support.y + translation.y,
                        support.z + translation.z,
                    );
                    point_distance(expected, *candidate)
                })
                .try_fold(0.0_f64, |maximum, error| {
                    error.is_finite().then(|| maximum.max(error))
                })?;
            return (maximum_error <= tolerance).then_some(maximum_error);
        }
    }
    certified_curved_offset_cache_fit(support, candidate, distance, tolerance, same_basis)
}

fn nurbs_active_domain(surface: &NurbsSurface) -> Option<[[u64; 2]; 2]> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    Some([
        [
            surface.u_knots.get(u_degree)?.to_bits(),
            surface.u_knots.get(u_count)?.to_bits(),
        ],
        [
            surface.v_knots.get(v_degree)?.to_bits(),
            surface.v_knots.get(v_count)?.to_bits(),
        ],
    ])
}

#[derive(Clone)]
struct HomogeneousSurfaceNet {
    u_degree: usize,
    v_degree: usize,
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    u_count: usize,
    v_count: usize,
    controls: Vec<[f64; 4]>,
}

impl HomogeneousSurfaceNet {
    fn from_homogeneous_surface(surface: &NurbsSurface) -> Option<Self> {
        Self::from_components(surface, |point, weight| {
            [point.x * weight, point.y * weight, point.z * weight, weight]
        })
    }

    fn from_homogeneous_residual(support: &NurbsSurface, candidate: &NurbsSurface) -> Option<Self> {
        Self::from_homogeneous_surface(candidate)?;
        let mut net = Self::from_components(support, |point, weight| {
            [point.x * weight, point.y * weight, point.z * weight, weight]
        })?;
        for ((control, support), candidate) in net
            .controls
            .iter_mut()
            .zip(&support.control_points)
            .zip(&candidate.control_points)
        {
            control[0] = (candidate.x - support.x) * control[3];
            control[1] = (candidate.y - support.y) * control[3];
            control[2] = (candidate.z - support.z) * control[3];
        }
        net.controls
            .iter()
            .flatten()
            .all(|component| component.is_finite())
            .then_some(net)
    }

    fn from_components(
        surface: &NurbsSurface,
        components: impl Fn(Point3, f64) -> [f64; 4],
    ) -> Option<Self> {
        let u_degree = usize::try_from(surface.u_degree).ok()?;
        let v_degree = usize::try_from(surface.v_degree).ok()?;
        let u_count = usize::try_from(surface.u_count).ok()?;
        let v_count = usize::try_from(surface.v_count).ok()?;
        let control_count = u_count.checked_mul(v_count)?;
        if u_degree == 0
            || v_degree == 0
            || u_degree >= u_count
            || v_degree >= v_count
            || surface.control_points.len() != control_count
            || surface.u_knots.len() != u_count.checked_add(u_degree)?.checked_add(1)?
            || surface.v_knots.len() != v_count.checked_add(v_degree)?.checked_add(1)?
            || surface
                .u_knots
                .iter()
                .chain(&surface.v_knots)
                .any(|knot| !knot.is_finite())
            || surface.u_knots.windows(2).any(|pair| pair[0] > pair[1])
            || surface.v_knots.windows(2).any(|pair| pair[0] > pair[1])
            || surface
                .control_points
                .iter()
                .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
            || !positive_weights(surface.weights.as_deref())
        {
            return None;
        }
        let controls = surface
            .control_points
            .iter()
            .enumerate()
            .map(|(index, point)| {
                components(
                    *point,
                    surface
                        .weights
                        .as_ref()
                        .map_or(1.0, |weights| weights[index]),
                )
            })
            .collect::<Vec<_>>();
        if controls
            .iter()
            .flatten()
            .any(|component| !component.is_finite())
        {
            return None;
        }
        Some(Self {
            u_degree,
            v_degree,
            u_knots: surface.u_knots.clone(),
            v_knots: surface.v_knots.clone(),
            u_count,
            v_count,
            controls,
        })
    }

    fn derivative(&self, u_axis: bool) -> Option<Self> {
        let (degree, count, knots) = if u_axis {
            (self.u_degree, self.u_count, &self.u_knots)
        } else {
            (self.v_degree, self.v_count, &self.v_knots)
        };
        if degree == 0 || count < 2 {
            return None;
        }
        let next_u_count = self.u_count - usize::from(u_axis);
        let next_v_count = self.v_count - usize::from(!u_axis);
        let mut controls = Vec::with_capacity(next_u_count.checked_mul(next_v_count)?);
        for u in 0..next_u_count {
            for v in 0..next_v_count {
                let index = |u, v| u * self.v_count + v;
                let (first, second, derivative_index) = if u_axis {
                    (
                        self.controls[index(u, v)],
                        self.controls[index(u + 1, v)],
                        u,
                    )
                } else {
                    (
                        self.controls[index(u, v)],
                        self.controls[index(u, v + 1)],
                        v,
                    )
                };
                let denominator =
                    knots[derivative_index + degree + 1] - knots[derivative_index + 1];
                if !denominator.is_finite() || denominator < 0.0 {
                    return None;
                }
                if denominator == 0.0 {
                    controls.push([0.0; 4]);
                    continue;
                }
                let factor = degree as f64 / denominator;
                controls.push(std::array::from_fn(|axis| {
                    factor * (second[axis] - first[axis])
                }));
            }
        }
        Some(Self {
            u_degree: self.u_degree - usize::from(u_axis),
            v_degree: self.v_degree - usize::from(!u_axis),
            u_knots: if u_axis {
                self.u_knots[1..self.u_knots.len() - 1].to_vec()
            } else {
                self.u_knots.clone()
            },
            v_knots: if u_axis {
                self.v_knots.clone()
            } else {
                self.v_knots[1..self.v_knots.len() - 1].to_vec()
            },
            u_count: next_u_count,
            v_count: next_v_count,
            controls,
        })
    }

    fn active_control_bounds(
        &self,
        u: f64,
        v: f64,
        origin: [f64; 3],
    ) -> Option<HomogeneousControlBounds> {
        let u_controls = active_spline_controls(&self.u_knots, self.u_degree, self.u_count, u)?;
        let v_controls = active_spline_controls(&self.v_knots, self.v_degree, self.v_count, v)?;
        let mut bounds = HomogeneousControlBounds {
            minimum_weight: f64::INFINITY,
            maximum_position_norm: 0.0,
            maximum_weight_magnitude: 0.0,
        };
        for u in u_controls {
            for v in v_controls.clone() {
                let control = self.controls[u * self.v_count + v];
                let position_norm = control[..3]
                    .iter()
                    .zip(origin)
                    .map(|(coordinate, origin)| {
                        let coordinate = coordinate - origin * control[3];
                        coordinate * coordinate
                    })
                    .sum::<f64>()
                    .sqrt();
                if !position_norm.is_finite() || !control[3].is_finite() {
                    return None;
                }
                bounds.minimum_weight = bounds.minimum_weight.min(control[3]);
                bounds.maximum_position_norm = bounds.maximum_position_norm.max(position_norm);
                bounds.maximum_weight_magnitude =
                    bounds.maximum_weight_magnitude.max(control[3].abs());
            }
        }
        Some(bounds)
    }
}

#[derive(Clone, Copy)]
struct HomogeneousControlBounds {
    minimum_weight: f64,
    maximum_position_norm: f64,
    maximum_weight_magnitude: f64,
}

fn active_spline_controls(
    knots: &[f64],
    degree: usize,
    count: usize,
    parameter: f64,
) -> Option<std::ops::RangeInclusive<usize>> {
    let lower = *knots.get(degree)?;
    let upper = *knots.get(count)?;
    if !parameter.is_finite() || parameter < lower || parameter > upper || lower >= upper {
        return None;
    }
    let span = if parameter == upper {
        count.checked_sub(1)?
    } else {
        knots
            .partition_point(|knot| *knot <= parameter)
            .checked_sub(1)?
    };
    (span >= degree && span < count).then_some(span - degree..=span)
}

fn certified_curved_offset_cache_fit(
    support: &NurbsSurface,
    candidate: &NurbsSurface,
    distance: f64,
    tolerance: f64,
    same_basis: bool,
) -> Option<f64> {
    let support_net = HomogeneousSurfaceNet::from_homogeneous_surface(support)?;
    let candidate_net = HomogeneousSurfaceNet::from_homogeneous_surface(candidate)?;
    let residual_net = if same_basis {
        Some(HomogeneousSurfaceNet::from_homogeneous_residual(
            support, candidate,
        )?)
    } else {
        None
    };

    let mut u_breaks = support_net.u_knots[support_net.u_degree..=support_net.u_count].to_vec();
    u_breaks.extend(&candidate_net.u_knots[candidate_net.u_degree..=candidate_net.u_count]);
    u_breaks.sort_by(f64::total_cmp);
    u_breaks.dedup();
    let mut v_breaks = support_net.v_knots[support_net.v_degree..=support_net.v_count].to_vec();
    v_breaks.extend(&candidate_net.v_knots[candidate_net.v_degree..=candidate_net.v_count]);
    v_breaks.sort_by(f64::total_cmp);
    v_breaks.dedup();
    let mut rectangles = u_breaks
        .windows(2)
        .filter(|span| span[0] < span[1])
        .flat_map(|u| {
            v_breaks
                .windows(2)
                .filter(|span| span[0] < span[1])
                .map(move |v| [u[0], u[1], v[0], v[1]])
        })
        .collect::<Vec<_>>();
    if rectangles.is_empty() {
        return None;
    }
    let mut certified_bound = 0.0_f64;
    while let Some([u0, u1, v0, v1]) = rectangles.pop() {
        let u = u0 + (u1 - u0) * 0.5;
        let v = v0 + (v1 - v0) * 0.5;
        let support_bounds = rational_surface_derivative_bounds(&support_net, u, v)?;
        let (residual_u_bound, residual_v_bound) = if let Some(residual_net) = &residual_net {
            let bounds = rational_surface_derivative_bounds(residual_net, u, v)?;
            (bounds.u, bounds.v)
        } else {
            let candidate_bounds = rational_surface_derivative_bounds(&candidate_net, u, v)?;
            (
                support_bounds.u + candidate_bounds.u,
                support_bounds.v + candidate_bounds.v,
            )
        };
        let normal_u_numerator =
            support_bounds.uu * support_bounds.v + support_bounds.u * support_bounds.uv;
        let normal_v_numerator =
            support_bounds.uv * support_bounds.v + support_bounds.u * support_bounds.vv;
        if !normal_u_numerator.is_finite() || !normal_v_numerator.is_finite() {
            return None;
        }
        let support_point = cadmpeg_ir::eval::nurbs_surface_point(support, u, v)?;
        let candidate_point = cadmpeg_ir::eval::nurbs_surface_point(candidate, u, v)?;
        let partials = nurbs_surface_partials(support, u, v)?;
        let normal_vector = cross_vector(partials.du, partials.dv);
        let normal_size = normal_vector.norm();
        let half_u = (u1 - u0) * 0.5;
        let half_v = (v1 - v0) * 0.5;
        let minimum_normal =
            normal_size - normal_u_numerator * half_u - normal_v_numerator * half_v;
        if !minimum_normal.is_finite() || minimum_normal <= 0.0 {
            let split_u = normal_u_numerator * (u1 - u0) >= normal_v_numerator * (v1 - v0);
            if !subdivide_offset_rectangle(&mut rectangles, [u0, u1, v0, v1], [u, v], split_u) {
                return None;
            }
            continue;
        }
        let normal = unit_vector(normal_vector)?;
        let u_lipschitz = residual_u_bound + distance.abs() * normal_u_numerator / minimum_normal;
        let v_lipschitz = residual_v_bound + distance.abs() * normal_v_numerator / minimum_normal;
        let expected = Point3::new(
            support_point.x + distance * normal.x,
            support_point.y + distance * normal.y,
            support_point.z + distance * normal.z,
        );
        let midpoint_error = point_distance(expected, candidate_point);
        let bound = midpoint_error + u_lipschitz * half_u + v_lipschitz * half_v;
        if !bound.is_finite() {
            return None;
        }
        if bound <= tolerance {
            certified_bound = certified_bound.max(bound);
            continue;
        }
        let split_u = u_lipschitz * (u1 - u0) >= v_lipschitz * (v1 - v0);
        if !subdivide_offset_rectangle(&mut rectangles, [u0, u1, v0, v1], [u, v], split_u) {
            return None;
        }
    }
    Some(certified_bound)
}

#[derive(Clone, Copy)]
struct RationalSurfaceDerivativeBounds {
    u: f64,
    v: f64,
    uu: f64,
    uv: f64,
    vv: f64,
}

fn rational_surface_derivative_bounds(
    net: &HomogeneousSurfaceNet,
    u: f64,
    v: f64,
) -> Option<RationalSurfaceDerivativeBounds> {
    let u_controls = active_spline_controls(&net.u_knots, net.u_degree, net.u_count, u)?;
    let v_controls = active_spline_controls(&net.v_knots, net.v_degree, net.v_count, v)?;
    let reference = net.controls[*u_controls.start() * net.v_count + *v_controls.start()];
    let origin = (reference[3] > 0.0).then(|| {
        [
            reference[0] / reference[3],
            reference[1] / reference[3],
            reference[2] / reference[3],
        ]
    })?;
    if origin.iter().any(|coordinate| !coordinate.is_finite()) {
        return None;
    }
    let base_bounds = net.active_control_bounds(u, v, origin)?;
    let weight_floor = (base_bounds.minimum_weight > 0.0).then_some(base_bounds.minimum_weight)?;
    let a = base_bounds.maximum_position_norm;
    let u_net = net.derivative(true)?;
    let v_net = net.derivative(false)?;
    let u_bounds = u_net.active_control_bounds(u, v, origin)?;
    let v_bounds = v_net.active_control_bounds(u, v, origin)?;
    let au = u_bounds.maximum_position_norm;
    let av = v_bounds.maximum_position_norm;
    let wu = u_bounds.maximum_weight_magnitude;
    let wv = v_bounds.maximum_weight_magnitude;
    let uv_net = u_net.derivative(false)?;
    let (auu, wuu) = if u_net.u_degree == 0 {
        (0.0, 0.0)
    } else {
        let bounds = u_net
            .derivative(true)?
            .active_control_bounds(u, v, origin)?;
        (
            bounds.maximum_position_norm,
            bounds.maximum_weight_magnitude,
        )
    };
    let (avv, wvv) = if v_net.v_degree == 0 {
        (0.0, 0.0)
    } else {
        let bounds = v_net
            .derivative(false)?
            .active_control_bounds(u, v, origin)?;
        (
            bounds.maximum_position_norm,
            bounds.maximum_weight_magnitude,
        )
    };
    let uv_bounds = uv_net.active_control_bounds(u, v, origin)?;
    let auv = uv_bounds.maximum_position_norm;
    let wuv = uv_bounds.maximum_weight_magnitude;
    let inverse_weight = weight_floor.recip();
    let inverse_weight_squared = inverse_weight * inverse_weight;
    let inverse_weight_cubed = inverse_weight_squared * inverse_weight;
    let u = au * inverse_weight + a * wu * inverse_weight_squared;
    let v = av * inverse_weight + a * wv * inverse_weight_squared;
    let uu = auu * inverse_weight
        + (a * wuu + 2.0 * au * wu) * inverse_weight_squared
        + 2.0 * a * wu * wu * inverse_weight_cubed;
    let uv = auv * inverse_weight
        + (au * wv + av * wu + a * wuv) * inverse_weight_squared
        + 2.0 * a * wu * wv * inverse_weight_cubed;
    let vv = avv * inverse_weight
        + (a * wvv + 2.0 * av * wv) * inverse_weight_squared
        + 2.0 * a * wv * wv * inverse_weight_cubed;
    [u, v, uu, uv, vv]
        .iter()
        .all(|bound| bound.is_finite())
        .then_some(RationalSurfaceDerivativeBounds { u, v, uu, uv, vv })
}

fn subdivide_offset_rectangle(
    rectangles: &mut Vec<[f64; 4]>,
    [u0, u1, v0, v1]: [f64; 4],
    [u, v]: [f64; 2],
    split_u: bool,
) -> bool {
    let u_divisible = u != u0 && u != u1;
    let v_divisible = v != v0 && v != v1;
    if u_divisible && (split_u || !v_divisible) {
        rectangles.extend([[u0, u, v0, v1], [u, u1, v0, v1]]);
        true
    } else if v_divisible {
        rectangles.extend([[u0, u1, v0, v], [u0, u1, v, v1]]);
        true
    } else {
        false
    }
}

fn translation_net_normal(surface: &NurbsSurface) -> Option<Vector3> {
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    if u_count < 2
        || v_count < 2
        || u_degree >= u_count
        || v_degree >= v_count
        || surface.control_points.len() != u_count.checked_mul(v_count)?
        || surface
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != surface.control_points.len())
        || surface.u_knots.len() != u_count.checked_add(u_degree)?.checked_add(1)?
        || surface.v_knots.len() != v_count.checked_add(v_degree)?.checked_add(1)?
    {
        return None;
    }
    let point = |u: usize, v: usize| surface.control_points[u * v_count + v];
    let difference = |end: Point3, start: Point3| {
        Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z)
    };
    let u_direction = difference(point(1, 0), point(0, 0));
    let v_direction = difference(point(0, 1), point(0, 0));
    let normal = unit_vector(cross_vector(u_direction, v_direction))?;

    let positive_collinear = |increment: Vector3, direction: Vector3| {
        increment.x.is_finite()
            && increment.y.is_finite()
            && increment.z.is_finite()
            && cross_vector(increment, direction) == Vector3::new(0.0, 0.0, 0.0)
            && dot_vector(increment, direction) > 0.0
    };
    for u in 0..u_count - 1 {
        let denominator = surface.u_knots[u + u_degree + 1] - surface.u_knots[u + 1];
        if !denominator.is_finite()
            || denominator <= 0.0
            || !positive_collinear(difference(point(u + 1, 0), point(u, 0)), u_direction)
        {
            return None;
        }
    }
    for v in 0..v_count - 1 {
        let denominator = surface.v_knots[v + v_degree + 1] - surface.v_knots[v + 1];
        if !denominator.is_finite()
            || denominator <= 0.0
            || !positive_collinear(difference(point(0, v + 1), point(0, v)), v_direction)
        {
            return None;
        }
    }
    let origin = point(0, 0);
    for u in 0..u_count {
        for v in 0..v_count {
            let v_displacement = difference(point(0, v), origin);
            if difference(point(u, v), point(u, 0)) != v_displacement {
                return None;
            }
        }
    }
    Some(normal)
}

fn positive_weights(weights: Option<&[f64]>) -> bool {
    let Some(weights) = weights else {
        return true;
    };
    !weights.is_empty()
        && weights
            .iter()
            .all(|weight| weight.is_finite() && *weight > 0.0)
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

        let saved_offset_carriers = saved_offset_carriers(
            &ir,
            graph,
            &view.offset_surfaces,
            &surfaces_by_xmt,
            ir.tolerances.linear,
        );
        for (oi, offset) in view.offset_surfaces.iter().copied().enumerate() {
            let Some(support) = surfaces_by_xmt.get(&offset.support).cloned() else {
                continue;
            };
            let procedural_id = ProceduralSurfaceId(format!("nx:s{si}:offset#{oi}"));
            let (surface_id, cache_fit_tolerance) =
                if let Some((surface, fit_tolerance)) = saved_offset_carriers.get(&offset.xmt) {
                    (surface.clone(), Some(*fit_tolerance))
                } else {
                    let surface_id = SurfaceId(format!("nx:s{si}:offset-surf#{oi}"));
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
                    (surface_id, None)
                };
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
                cache_fit_tolerance,
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
        let uncharted_intersections: BTreeMap<_, _> = intersection_scan
            .uncharted
            .into_iter()
            .map(|curve| (curve.xmt, curve))
            .collect();
        for (ci, construction) in intersection_constructions.into_iter().enumerate() {
            let curve_id = CurveId(format!("nx:s{si}:intersection-crv#{ci}"));
            let procedural_id = ProceduralCurveId(format!("nx:s{si}:intersection#{ci}"));
            let unknown_id = UnknownId(format!("nx:container:parasolid#{si}"));
            let charted = charted_intersections.get(&construction.xmt);
            let uncharted = uncharted_intersections
                .get(&construction.xmt)
                .and_then(|uncharted| {
                    let supports = uncharted
                        .supports
                        .each_ref()
                        .map(|xmt| surfaces_by_xmt.get(xmt).cloned());
                    let [Some(first), Some(second)] = supports else {
                        return None;
                    };
                    (first != second).then_some((
                        [first, second],
                        uncharted.endpoints,
                        uncharted.tolerance * 1000.0,
                    ))
                });
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
            if charted.is_some() || uncharted.is_some() {
                annotations.derived(&curve_id, "geometry");
            } else {
                annotations.exactness(&curve_id, Exactness::Unknown);
            }
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: if let Some(charted) = charted {
                    CurveGeometry::Nurbs(NurbsCurve {
                        degree: 1,
                        knots: linear_knots(&charted.parameters),
                        control_points: charted.points.clone(),
                        weights: None,
                        periodic: false,
                    })
                } else if uncharted.is_some() {
                    CurveGeometry::Procedural {
                        construction: procedural_id.clone(),
                    }
                } else {
                    CurveGeometry::Unknown {
                        record: Some(unknown_id.clone()),
                    }
                },
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
            if charted.is_some() || uncharted.is_some() {
                annotations.derived(&procedural_id, "definition");
            } else {
                annotations.exactness(&procedural_id, Exactness::Unknown);
            }
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedural_id,
                curve: curve_id.clone(),
                definition: if let Some(charted) = charted {
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
                } else if let Some((supports, endpoints, tolerance)) = uncharted {
                    ProceduralCurveDefinition::TolerantIntersection {
                        supports,
                        endpoints,
                        tolerance,
                        parameterization: None,
                    }
                } else {
                    ProceduralCurveDefinition::Unknown {
                        native_kind: Some("nx:intersection".into()),
                        record: Some(unknown_id),
                    }
                },
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
        invalidate_inconsistent_support_uv(&mut ir, &pending_ext11_support_uv);
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

fn unmatched_delta_tombstone_counts(scan: &Scan) -> BTreeMap<&'static str, usize> {
    let pairs = crate::native::paired_delta_streams(scan);
    let mut current = pairs
        .keys()
        .map(|partition| (*partition, scan.streams[*partition].inflated.clone()))
        .collect::<BTreeMap<_, _>>();
    let paired_deltas = pairs.values().flatten().copied().collect::<BTreeSet<_>>();
    let mut unmatched = BTreeMap::new();
    let mut add_counts = |counts: BTreeMap<&'static str, usize>| {
        for (family, count) in counts {
            *unmatched.entry(family).or_default() += count;
        }
    };
    for (delta, stream) in scan.streams.iter().enumerate() {
        if stream.kind == StreamKind::Deltas && !paired_deltas.contains(&delta) {
            add_counts(crate::deltas::unmatched_terminal_tombstones_by_family(
                &[],
                &stream.inflated,
            ));
        }
    }
    for (partition, deltas) in pairs {
        for delta in deltas {
            let delta_bytes = &scan.streams[delta].inflated;
            let partition_bytes = current
                .get_mut(&partition)
                .expect("paired partition was initialized");
            add_counts(crate::deltas::unmatched_terminal_tombstones_by_family(
                partition_bytes,
                delta_bytes,
            ));
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
    for unknown in unknowns.iter_mut() {
        unknown.links.retain(|link| ids.contains(link));
        if !unknown.links.is_empty() {
            annotations.derived(&unknown.id, "links");
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
    let body_data_block_uses = model.features.feature_body_data_block_uses.as_slice();
    let booleans = model.features.feature_boolean_operations.as_slice();
    let bindings = model.segments.segment_body_bindings.as_slice();
    let body_operands = model.features.feature_operation_body_operands.as_slice();
    let Some(statuses) = crate::native::segment_body_lineage_statuses(
        labels,
        body_references,
        body_data_block_uses,
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

pub(crate) fn invalidate_inconsistent_support_uv(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
) {
    let mut invalid = Vec::new();
    for (procedural_id, points, parameters, fit_tolerance, _) in pending {
        let Some(procedural_index) = ir
            .model
            .procedural_curves
            .iter()
            .position(|procedural| &procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } =
            &ir.model.procedural_curves[procedural_index].definition
        else {
            continue;
        };
        for (side, support) in context.sides.iter().enumerate() {
            let (Some(surface), Some(pcurve)) = (&support.surface, &support.pcurve) else {
                continue;
            };
            let tolerance = blend_spine_cache_fit_tolerance(ir, surface, *fit_tolerance);
            let inconsistent = parameters
                .iter()
                .zip(points)
                .filter_map(|(parameter, point)| {
                    let uv = pcurve_uv(pcurve, *parameter)?;
                    decoded_surface_point(ir, surface, uv.u, uv.v)
                        .map(|actual| point_distance(actual, *point) > tolerance)
                })
                .any(|inconsistent| inconsistent);
            if inconsistent {
                invalid.push((procedural_index, side));
            }
        }
    }
    for (procedural_index, side) in invalid {
        let ProceduralCurveDefinition::Intersection { context, .. } =
            &mut ir.model.procedural_curves[procedural_index].definition
        else {
            unreachable!("definition selected above");
        };
        context.sides[side].pcurve = None;
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
                    SurfaceGeometry::Nurbs(nurbs) => nurbs_parameters_with_tolerance(
                        nurbs,
                        *point,
                        seed,
                        Some(effective_fit_tolerance),
                    ),
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
                                    BoundaryInverseTarget {
                                        point: *point,
                                        seed,
                                        tolerance: effective_fit_tolerance,
                                    },
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
    let cache_backed_curves = ir
        .model
        .curves
        .iter()
        .filter(|curve| !matches!(&curve.geometry, CurveGeometry::Procedural { .. }))
        .map(|curve| curve.id.clone())
        .collect::<BTreeSet<_>>();
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
            if cache_backed_curves.contains(&procedural.curve) {
                procedural.cache_fit_tolerance = Some(
                    procedural
                        .cache_fit_tolerance
                        .unwrap_or(0.0)
                        .max(effective_fit_tolerance),
                );
            }
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
        ) = (
            procedural_surface_for_carrier(ir, first).map(|surface| &surface.definition),
            procedural_surface_for_carrier(ir, second).map(|surface| &surface.definition),
        )
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

fn procedural_surface_for_carrier<'a>(
    ir: &'a CadIr,
    surface: &SurfaceId,
) -> Option<&'a ProceduralSurface> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    if let SurfaceGeometry::Procedural { construction } = &carrier.geometry {
        return ir
            .model
            .procedural_surfaces
            .iter()
            .find(|candidate| candidate.id == *construction && candidate.surface == *surface);
    }
    let mut producers = ir.model.procedural_surfaces.iter().filter(|candidate| {
        candidate.surface == *surface && candidate.cache_fit_tolerance.is_some()
    });
    let producer = producers.next()?;
    producers.next().is_none().then_some(producer)
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
            let fit_tolerance = candidate.2.or_else(|| {
                ir.model
                    .edges
                    .iter()
                    .find(|edge| edge.id == coedge.edge)
                    .and_then(|edge| edge.tolerance)
            });
            pcurve_matches_edge(ir, &coedge.edge, surface, &candidate.0, fit_tolerance).then(|| {
                (
                    coedge.id.clone(),
                    (candidate.0.clone(), candidate.1, fit_tolerance),
                )
            })
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
            blend_boundary_parameter(
                ir,
                surface,
                point,
                boundary,
                seed.map(|seed| seed.u),
                fit_tolerance,
                depth + 1,
            )
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
        let derivative = |step: f64| {
            let mut before = parameters;
            let mut after = parameters;
            before.u -= step;
            after.u += step;
            if let Some(domain) = u_domain {
                before.u = before.u.clamp(domain[0], domain[1]);
                after.u = after.u.clamp(domain[0], domain[1]);
            }
            let width = after.u - before.u;
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
        let du = blend_surface_u_derivative(ir, surface, parameters.u, parameters.v, depth + 1)
            .or_else(|| derivative(u_step))?;
        let (_, tangent, first, second, radius) =
            blend_surface_frame(ir, surface, parameters.u, depth + 1)?;
        let alpha = signed_angle(first, second, tangent);
        let radial = rodrigues_rotate(first, tangent, parameters.v * alpha);
        let section_tangent = cross_vector(tangent, radial);
        let dv = Vector3::new(
            radius * alpha * section_tangent.x,
            radius * alpha * section_tangent.y,
            radius * alpha * section_tangent.z,
        );
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

pub(crate) fn blend_surface_u_derivative(
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
) -> Option<Vector3> {
    (depth < 32).then_some(())?;
    let (supports, spine, radius, _) = blend_surface_definition(ir, surface)?;
    let carrier = ir
        .model
        .curves
        .iter()
        .find(|candidate| candidate.id == spine)?;
    let center = curve_point(&carrier.geometry, u)?;
    let velocity = curve_tangent(&carrier.geometry, u)?;
    let acceleration = curve_second_derivative(&carrier.geometry, u)?;
    let speed = velocity.norm();
    if !speed.is_finite() || speed == 0.0 {
        return None;
    }
    let tangent = Vector3::new(velocity.x / speed, velocity.y / speed, velocity.z / speed);
    let tangential_acceleration = dot_vector(tangent, acceleration);
    let tangent_derivative = Vector3::new(
        (acceleration.x - tangential_acceleration * tangent.x) / speed,
        (acceleration.y - tangential_acceleration * tangent.y) / speed,
        (acceleration.z - tangential_acceleration * tangent.z) / speed,
    );
    let contact_context = BlendContactDerivativeContext {
        ir,
        spine: &spine,
        parameter: u,
        center,
        center_derivative: velocity,
        radius,
        depth: depth + 1,
    };
    let (first, first_derivative) = contact_context.direction_derivative(&supports[0])?;
    let (second, second_derivative) = contact_context.direction_derivative(&supports[1])?;

    let cross = cross_vector(first, second);
    let cosine = dot_vector(first, second);
    let sine = dot_vector(cross, tangent);
    let cosine_derivative =
        dot_vector(first_derivative, second) + dot_vector(first, second_derivative);
    let cross_derivative =
        cross_vector(first_derivative, second) + cross_vector(first, second_derivative);
    let sine_derivative =
        dot_vector(cross_derivative, tangent) + dot_vector(cross, tangent_derivative);
    let angle_denominator = cosine * cosine + sine * sine;
    if !angle_denominator.is_finite() || angle_denominator == 0.0 {
        return None;
    }
    let alpha = sine.atan2(cosine);
    let alpha_derivative =
        (cosine * sine_derivative - sine * cosine_derivative) / angle_denominator;
    let theta = v * alpha;
    let theta_derivative = v * alpha_derivative;
    let theta_cosine = theta.cos();
    let theta_sine = theta.sin();
    let tangent_cross_first = cross_vector(tangent, first);
    let tangent_cross_first_derivative =
        cross_vector(tangent_derivative, first) + cross_vector(tangent, first_derivative);
    let tangent_dot_first = dot_vector(tangent, first);
    let tangent_dot_first_derivative =
        dot_vector(tangent_derivative, first) + dot_vector(tangent, first_derivative);
    let radial_component = |first: f64,
                            first_derivative: f64,
                            tangent_cross_first: f64,
                            tangent_cross_first_derivative: f64,
                            tangent: f64,
                            tangent_derivative: f64| {
        first_derivative * theta_cosine - first * theta_sine * theta_derivative
            + tangent_cross_first_derivative * theta_sine
            + tangent_cross_first * theta_cosine * theta_derivative
            + tangent_derivative * tangent_dot_first * (1.0 - theta_cosine)
            + tangent * tangent_dot_first_derivative * (1.0 - theta_cosine)
            + tangent * tangent_dot_first * theta_sine * theta_derivative
    };
    let radial_derivative = Vector3::new(
        radial_component(
            first.x,
            first_derivative.x,
            tangent_cross_first.x,
            tangent_cross_first_derivative.x,
            tangent.x,
            tangent_derivative.x,
        ),
        radial_component(
            first.y,
            first_derivative.y,
            tangent_cross_first.y,
            tangent_cross_first_derivative.y,
            tangent.y,
            tangent_derivative.y,
        ),
        radial_component(
            first.z,
            first_derivative.z,
            tangent_cross_first.z,
            tangent_cross_first_derivative.z,
            tangent.z,
            tangent_derivative.z,
        ),
    );
    Some(Vector3::new(
        velocity.x + radius * radial_derivative.x,
        velocity.y + radius * radial_derivative.y,
        velocity.z + radius * radial_derivative.z,
    ))
}

struct BlendContactDerivativeContext<'a> {
    ir: &'a CadIr,
    spine: &'a CurveId,
    parameter: f64,
    center: Point3,
    center_derivative: Vector3,
    radius: f64,
    depth: usize,
}

impl BlendContactDerivativeContext<'_> {
    fn direction_derivative(&self, support: &SurfaceId) -> Option<(Vector3, Vector3)> {
        (self.depth < 32).then_some(())?;
        let pcurve =
            spine_contact_pcurve(self.ir, support, self.spine, self.radius, self.depth + 1)?;
        let uv = pcurve_uv(pcurve, self.parameter)?;
        let uv_derivative = pcurve_tangent(pcurve, self.parameter)?;
        let support = model_surface_partials_by_id(self.ir, support, uv.u, uv.v)?;
        let contact_derivative = Vector3::new(
            support.du.x * uv_derivative.u + support.dv.x * uv_derivative.v,
            support.du.y * uv_derivative.u + support.dv.y * uv_derivative.v,
            support.du.z * uv_derivative.u + support.dv.z * uv_derivative.v,
        );
        let offset = Vector3::new(
            support.point.x - self.center.x,
            support.point.y - self.center.y,
            support.point.z - self.center.z,
        );
        let magnitude = offset.norm();
        if !magnitude.is_finite() || magnitude == 0.0 {
            return None;
        }
        let direction = Vector3::new(
            offset.x / magnitude,
            offset.y / magnitude,
            offset.z / magnitude,
        );
        let offset_derivative = Vector3::new(
            contact_derivative.x - self.center_derivative.x,
            contact_derivative.y - self.center_derivative.y,
            contact_derivative.z - self.center_derivative.z,
        );
        let radial_derivative = dot_vector(direction, offset_derivative);
        let direction_derivative = Vector3::new(
            (offset_derivative.x - radial_derivative * direction.x) / magnitude,
            (offset_derivative.y - radial_derivative * direction.y) / magnitude,
            (offset_derivative.z - radial_derivative * direction.z) / magnitude,
        );
        Some((direction, direction_derivative))
    }
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
    seed: Option<f64>,
    fit_tolerance: f64,
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
        SurfaceGeometry::Nurbs(nurbs) => {
            nurbs_parameters_with_tolerance(nurbs, point, None, Some(fit_tolerance))
        }
        SurfaceGeometry::Procedural { .. } => offset_surface_parameters(ir, support, point, None),
        geometry => analytic_surface_parameters(geometry, point),
    }?;
    let pcurve = spine_contact_pcurve(ir, support, &spine, radius, depth + 1)?;
    closest_pcurve_parameters(pcurve, uv, seed)?
        .into_iter()
        .find(|parameter| {
            blend_boundary_point(ir, surface, *parameter, boundary, depth + 1)
                .is_some_and(|candidate| point_distance(candidate, point) <= fit_tolerance)
        })
}

#[derive(Clone, Copy)]
struct BoundaryInverseTarget {
    point: Point3,
    seed: Option<Point2>,
    tolerance: f64,
}

fn blend_boundary_parameter_from_support_pcurve(
    ir: &CadIr,
    blend: &SurfaceId,
    support: &SurfaceId,
    support_pcurve: &PcurveGeometry,
    curve_parameter: f64,
    target: BoundaryInverseTarget,
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
    closest_pcurve_parameters(contact_pcurve, support_uv, target.seed.map(|seed| seed.u))?
        .into_iter()
        .find(|parameter| {
            blend_boundary_point(ir, blend, *parameter, boundary, 0).is_some_and(|candidate| {
                point_distance(candidate, target.point) <= target.tolerance
            })
        })
        .map(|parameter| Point2::new(parameter, boundary as f64))
}

pub(crate) fn closest_pcurve_parameters(
    pcurve: &PcurveGeometry,
    point: Point2,
    seed: Option<f64>,
) -> Option<Vec<f64>> {
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
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
    if seed.is_some_and(|seed| !seed.is_finite()) {
        return None;
    }
    let search_seed = seed.map(|seed| canonical_periodic_parameter(domain, *periodic, seed));
    let homogeneous =
        homogeneous_pcurve_spans(degree, knots, control_points, weights.as_deref(), point)?;
    let candidates = if degree != 1 || weights.is_some() {
        closest_parameter_candidates(
            stationary_rational_distance_candidates(&homogeneous, search_seed)?,
            search_seed,
        )?
    } else {
        let candidates = control_points
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
            })
            .collect::<Vec<_>>();
        closest_parameter_candidates(candidates, search_seed)?
    };
    Some(lift_periodic_parameters(
        candidates, domain, *periodic, seed,
    ))
}

struct HomogeneousCurveSpans<const DIMENSION: usize> {
    spans: Vec<BezierSpan<DIMENSION>>,
    coordinate_tolerance: f64,
}

fn homogeneous_pcurve_spans(
    degree: usize,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    point: Point2,
) -> Option<HomogeneousCurveSpans<3>> {
    let count = control_points.len();
    if degree == 0
        || count <= degree
        || knots.len() != count.checked_add(degree)?.checked_add(1)?
        || knots.iter().any(|knot| !knot.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
        || control_points
            .iter()
            .any(|control| !control.u.is_finite() || !control.v.is_finite())
        || !point.u.is_finite()
        || !point.v.is_finite()
    {
        return None;
    }
    let weights = match weights {
        Some(weights)
            if weights.len() == count
                && weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0) =>
        {
            weights.to_vec()
        }
        Some(_) => return None,
        None => vec![1.0; count],
    };
    let coordinate_scale = control_points
        .iter()
        .flat_map(|control| [control.u, control.v])
        .chain([point.u, point.v])
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    let controls = control_points
        .iter()
        .zip(weights)
        .map(|(control, weight)| {
            [
                weight * (control.u - point.u),
                weight * (control.v - point.v),
                weight,
            ]
        })
        .collect::<Vec<_>>();
    if controls.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }
    let spans = bezier_spans(degree, knots, controls)?;
    Some(HomogeneousCurveSpans {
        spans,
        coordinate_tolerance: 64.0 * f64::EPSILON * coordinate_scale,
    })
}

fn stationary_rational_distance_candidates<const DIMENSION: usize>(
    homogeneous: &HomogeneousCurveSpans<DIMENSION>,
    seed: Option<f64>,
) -> Option<Vec<(f64, f64)>> {
    let mut candidates = Vec::new();
    for span in &homogeneous.spans {
        let derivative = rational_squared_distance_derivative(&span.controls)?;
        let roots = scalar_bezier_roots(ScalarBezierSpan {
            domain: span.domain,
            controls: derivative,
        })?;
        let mut parameters = vec![span.domain[0], span.domain[1]];
        match roots {
            ScalarBezierRoots::Constant => parameters
                .extend(seed.filter(|seed| (span.domain[0]..=span.domain[1]).contains(seed))),
            ScalarBezierRoots::Isolated(roots) => parameters.extend(roots),
        }
        candidates.extend(parameters.into_iter().map(|parameter| {
            let distance = homogeneous_residual_distance(&span.controls, parameter, span.domain);
            (
                parameter,
                if distance <= homogeneous.coordinate_tolerance {
                    0.0
                } else {
                    distance * distance
                },
            )
        }));
    }
    Some(candidates)
}

fn rational_squared_distance_derivative<const DIMENSION: usize>(
    controls: &[[f64; DIMENSION]],
) -> Option<Vec<f64>> {
    // For residual R/W, half the squared-distance derivative has numerator
    // ((R·R')W - (R·R)W'). Positive weights make its roots exactly the finite
    // stationary parameters of the rational span.
    let weight = controls
        .iter()
        .map(|control| control[DIMENSION - 1])
        .collect::<Vec<_>>();
    let derivative = |values: &[f64]| {
        values
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect::<Vec<_>>()
    };
    let weight_derivative = derivative(&weight);
    let residuals = (0..DIMENSION - 1)
        .map(|axis| {
            controls
                .iter()
                .map(|control| control[axis])
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let residual_squared = sum_bernstein_polynomials(
        residuals
            .iter()
            .map(|residual| bernstein_product(residual, residual)),
    )?;
    let residual_derivative = sum_bernstein_polynomials(
        residuals
            .iter()
            .map(|residual| bernstein_product(residual, &derivative(residual))),
    )?;
    let first = bernstein_product(&residual_derivative, &weight)?;
    let second = bernstein_product(&residual_squared, &weight_derivative)?;
    subtract_bernstein_polynomials(first, second)
}

fn bernstein_product(first: &[f64], second: &[f64]) -> Option<Vec<f64>> {
    let first_degree = first.len().checked_sub(1)?;
    let second_degree = second.len().checked_sub(1)?;
    let degree = first_degree.checked_add(second_degree)?;
    (0..=degree)
        .map(|index| {
            let denominator = binomial_coefficient(degree, index)?;
            let lower = index.saturating_sub(second_degree);
            let upper = index.min(first_degree);
            (lower..=upper)
                .map(|first_index| {
                    let second_index = index - first_index;
                    Some(
                        first[first_index]
                            * second[second_index]
                            * binomial_coefficient(first_degree, first_index)?
                            * binomial_coefficient(second_degree, second_index)?
                            / denominator,
                    )
                })
                .sum::<Option<f64>>()
                .filter(|value| value.is_finite())
        })
        .collect()
}

fn binomial_coefficient(n: usize, k: usize) -> Option<f64> {
    let k = k.min(n.checked_sub(k)?);
    (1..=k).try_fold(1.0, |value, index| {
        let next = value * (n - k + index) as f64 / index as f64;
        next.is_finite().then_some(next)
    })
}

fn add_bernstein_polynomials(first: Vec<f64>, second: Vec<f64>) -> Option<Vec<f64>> {
    let result = (first.len() == second.len()).then(|| {
        first
            .into_iter()
            .zip(second)
            .map(|(a, b)| a + b)
            .collect::<Vec<_>>()
    })?;
    result
        .iter()
        .all(|value| value.is_finite())
        .then_some(result)
}

fn sum_bernstein_polynomials(
    polynomials: impl IntoIterator<Item = Option<Vec<f64>>>,
) -> Option<Vec<f64>> {
    polynomials.into_iter().try_fold(None, |sum, polynomial| {
        let polynomial = polynomial?;
        Some(Some(match sum {
            Some(sum) => add_bernstein_polynomials(sum, polynomial)?,
            None => polynomial,
        }))
    })?
}

fn subtract_bernstein_polynomials(first: Vec<f64>, second: Vec<f64>) -> Option<Vec<f64>> {
    let result = (first.len() == second.len()).then(|| {
        first
            .into_iter()
            .zip(second)
            .map(|(a, b)| a - b)
            .collect::<Vec<_>>()
    })?;
    result
        .iter()
        .all(|value| value.is_finite())
        .then_some(result)
}

enum ScalarBezierRoots {
    Constant,
    Isolated(Vec<f64>),
}

#[derive(Clone)]
struct ScalarBezierSpan {
    domain: [f64; 2],
    controls: Vec<f64>,
}

fn scalar_bezier_roots(span: ScalarBezierSpan) -> Option<ScalarBezierRoots> {
    const MAX_INTERVALS: usize = 100_000;

    let scale = span
        .controls
        .iter()
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    let tolerance = 64.0 * f64::EPSILON * scale;
    let constant = span.controls.iter().all(|value| *value == 0.0);
    if constant {
        return Some(ScalarBezierRoots::Constant);
    }
    let mut parameters = Vec::new();
    if span
        .controls
        .first()
        .is_some_and(|value| value.abs() <= tolerance)
    {
        parameters.push(span.domain[0]);
    }
    if span
        .controls
        .last()
        .is_some_and(|value| value.abs() <= tolerance)
    {
        parameters.push(span.domain[1]);
    }
    let mut intervals = vec![span];
    let mut examined = 0usize;
    while let Some(span) = intervals.pop() {
        examined += 1;
        if examined > MAX_INTERVALS {
            return None;
        }
        if scalar_bernstein_sign_variations(&span.controls) == 0 {
            continue;
        }
        let middle = span.domain[0] + (span.domain[1] - span.domain[0]) * 0.5;
        if middle == span.domain[0] || middle == span.domain[1] {
            let parameter =
                [span.domain[0], span.domain[1]]
                    .into_iter()
                    .min_by(|first, second| {
                        scalar_bezier_value(&span.controls, *first, span.domain)
                            .abs()
                            .total_cmp(
                                &scalar_bezier_value(&span.controls, *second, span.domain).abs(),
                            )
                    })?;
            if scalar_bezier_value(&span.controls, parameter, span.domain).abs() <= tolerance {
                parameters.push(parameter);
            }
            continue;
        }
        let (first, second) = subdivide_scalar_bezier_span(span, middle);
        if first.controls.last().is_some_and(|value| *value == 0.0) {
            parameters.push(middle);
        }
        intervals.push(second);
        intervals.push(first);
    }
    parameters.sort_by(f64::total_cmp);
    parameters.dedup_by(|first, second| {
        (*first - *second).abs() <= 64.0 * f64::EPSILON * first.abs().max(second.abs()).max(1.0)
    });
    Some(ScalarBezierRoots::Isolated(parameters))
}

fn scalar_bernstein_sign_variations(controls: &[f64]) -> usize {
    // Bernstein-form Descartes variation bounds the roots in the open span.
    // Exact zero controls do not contribute a sign.
    controls
        .iter()
        .copied()
        .filter(|value| *value != 0.0)
        .map(f64::is_sign_positive)
        .fold((None, 0), |(previous, variations), positive| {
            (
                Some(positive),
                variations + usize::from(previous.is_some_and(|previous| previous != positive)),
            )
        })
        .1
}

fn subdivide_scalar_bezier_span(
    span: ScalarBezierSpan,
    middle: f64,
) -> (ScalarBezierSpan, ScalarBezierSpan) {
    let mut levels = vec![span.controls];
    while levels.last().is_some_and(|level| level.len() > 1) {
        let next = levels
            .last()
            .expect("nonempty Bézier subdivision level")
            .windows(2)
            .map(|pair| (pair[0] + pair[1]) * 0.5)
            .collect();
        levels.push(next);
    }
    let first = levels.iter().map(|level| level[0]).collect();
    let second = levels
        .iter()
        .rev()
        .map(|level| *level.last().expect("nonempty Bézier subdivision level"))
        .collect();
    (
        ScalarBezierSpan {
            domain: [span.domain[0], middle],
            controls: first,
        },
        ScalarBezierSpan {
            domain: [middle, span.domain[1]],
            controls: second,
        },
    )
}

fn scalar_bezier_value(controls: &[f64], parameter: f64, domain: [f64; 2]) -> f64 {
    let fraction = (parameter - domain[0]) / (domain[1] - domain[0]);
    let mut values = controls.to_vec();
    while values.len() > 1 {
        values = values
            .windows(2)
            .map(|pair| (1.0 - fraction) * pair[0] + fraction * pair[1])
            .collect();
    }
    values[0]
}

#[derive(Clone)]
struct BezierSpan<const DIMENSION: usize> {
    domain: [f64; 2],
    controls: Vec<[f64; DIMENSION]>,
}

fn bezier_spans<const DIMENSION: usize>(
    degree: usize,
    knots: &[f64],
    mut controls: Vec<[f64; DIMENSION]>,
) -> Option<Vec<BezierSpan<DIMENSION>>> {
    let mut knots = knots.to_vec();
    let domain = [*knots.get(degree)?, *knots.get(controls.len())?];
    let mut internal = knots[degree + 1..controls.len()]
        .iter()
        .copied()
        .filter(|knot| domain[0] < *knot && *knot < domain[1])
        .collect::<Vec<_>>();
    internal.sort_by(f64::total_cmp);
    internal.dedup();
    for knot in internal {
        while knots.iter().filter(|candidate| **candidate == knot).count() < degree {
            insert_homogeneous_curve_knot(degree, &mut knots, &mut controls, knot)?;
        }
    }
    let mut boundaries = knots[degree..=controls.len()].to_vec();
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup();
    let spans = boundaries
        .windows(2)
        .enumerate()
        .filter_map(|(index, domain)| {
            (domain[0] < domain[1]).then(|| {
                let start = index.checked_mul(degree)?;
                Some(BezierSpan {
                    domain: [domain[0], domain[1]],
                    controls: controls.get(start..=start + degree)?.to_vec(),
                })
            })?
        })
        .collect::<Vec<_>>();
    (!spans.is_empty()).then_some(spans)
}

fn insert_homogeneous_curve_knot<const DIMENSION: usize>(
    degree: usize,
    knots: &mut Vec<f64>,
    controls: &mut Vec<[f64; DIMENSION]>,
    knot: f64,
) -> Option<()> {
    let count = controls.len();
    let span = knots
        .windows(2)
        .position(|pair| pair[0] <= knot && knot < pair[1])?;
    let multiplicity = knots.iter().filter(|candidate| **candidate == knot).count();
    if multiplicity >= degree {
        return Some(());
    }
    let mut inserted = vec![[0.0; DIMENSION]; count + 1];
    inserted[..=span - degree].copy_from_slice(&controls[..=span - degree]);
    inserted[span - multiplicity + 1..].copy_from_slice(&controls[span - multiplicity..]);
    for index in span - degree + 1..=span - multiplicity {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return None;
        }
        let alpha = (knot - knots[index]) / denominator;
        inserted[index] = std::array::from_fn(|axis| {
            alpha * controls[index][axis] + (1.0 - alpha) * controls[index - 1][axis]
        });
    }
    knots.insert(span + 1, knot);
    *controls = inserted;
    Some(())
}

fn homogeneous_residual_distance<const DIMENSION: usize>(
    controls: &[[f64; DIMENSION]],
    parameter: f64,
    domain: [f64; 2],
) -> f64 {
    let fraction = (parameter - domain[0]) / (domain[1] - domain[0]);
    let mut values = controls.to_vec();
    while values.len() > 1 {
        values = values
            .windows(2)
            .map(|pair| {
                std::array::from_fn(|axis| {
                    (1.0 - fraction) * pair[0][axis] + fraction * pair[1][axis]
                })
            })
            .collect();
    }
    values[0][..DIMENSION - 1]
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt()
        / values[0][DIMENSION - 1]
}

fn closest_parameter_candidates(
    candidates: impl IntoIterator<Item = (f64, f64)>,
    seed: Option<f64>,
) -> Option<Vec<f64>> {
    let candidates = candidates.into_iter().collect::<Vec<_>>();
    let minimum_distance = candidates
        .iter()
        .map(|candidate| candidate.1)
        .min_by(f64::total_cmp)?;
    let mut nearest = candidates
        .into_iter()
        .filter(|candidate| {
            let scale = candidate
                .1
                .abs()
                .max(minimum_distance.abs())
                .max(f64::MIN_POSITIVE);
            (candidate.1 - minimum_distance).abs() <= 128.0 * f64::EPSILON * scale
        })
        .map(|candidate| candidate.0)
        .collect::<Vec<_>>();
    nearest.sort_by(|first, second| {
        seed.map_or_else(
            || first.total_cmp(second),
            |seed| {
                (first - seed)
                    .abs()
                    .total_cmp(&(second - seed).abs())
                    .then_with(|| first.total_cmp(second))
            },
        )
    });
    nearest.dedup_by(|first, second| first.to_bits() == second.to_bits());
    (!nearest.is_empty()).then_some(nearest)
}

fn canonical_periodic_parameter(domain: [f64; 2], periodic: bool, parameter: f64) -> f64 {
    if !periodic {
        return parameter;
    }
    let period = domain[1] - domain[0];
    domain[0] + (parameter - domain[0]).rem_euclid(period)
}

fn lift_periodic_parameters(
    mut parameters: Vec<f64>,
    domain: [f64; 2],
    periodic: bool,
    seed: Option<f64>,
) -> Vec<f64> {
    let Some(seed) = seed.filter(|_| periodic) else {
        return parameters;
    };
    let period = domain[1] - domain[0];
    for parameter in &mut parameters {
        *parameter += ((seed - *parameter) / period).round() * period;
    }
    parameters.sort_by(|first, second| {
        (first - seed)
            .abs()
            .total_cmp(&(second - seed).abs())
            .then_with(|| first.total_cmp(second))
    });
    parameters.dedup_by(|first, second| first.to_bits() == second.to_bits());
    parameters
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
    ir.model
        .surfaces
        .iter()
        .any(|candidate| &candidate.id == surface)
        .then_some(())?;
    let Some(procedural) = procedural_surface_for_carrier(ir, surface) else {
        return Some((surface.clone(), 0.0));
    };
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
    let procedural = procedural_surface_for_carrier(ir, surface)?;
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
    if let Some(direction) = blend_surface_contact_direction(ir, surface, center, depth + 1) {
        return Some(direction);
    }
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
                    BlendParameterGrid::Disabled,
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

fn blend_surface_contact_direction(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    depth: usize,
) -> Option<Vector3> {
    (depth < 32).then_some(())?;
    let (_, spine, _, _) = blend_surface_definition(ir, surface)?;
    let u = closest_spine_parameter(ir, &spine, point, None)?;
    let frame = blend_surface_frame(ir, surface, u, depth + 1)?;
    let radial = unit_vector(Vector3::new(
        point.x - frame.0.x,
        point.y - frame.0.y,
        point.z - frame.0.z,
    ))?;
    let sweep = signed_angle(frame.2, frame.3, frame.1);
    if !sweep.is_finite() || sweep.abs() <= 1.0e-12 {
        return None;
    }
    let angle = signed_angle(frame.2, radial, frame.1);
    let candidate = (-2..=2)
        .map(|turn| (angle + f64::from(turn) * std::f64::consts::TAU) / sweep)
        .filter(|v| (0.0..=1.0).contains(v))
        .map(|v| blend_surface_point_from_frame(frame, v))
        .chain([
            blend_surface_point_from_frame(frame, 0.0),
            blend_surface_point_from_frame(frame, 1.0),
        ])
        .min_by(|first, second| {
            point_distance(*first, point).total_cmp(&point_distance(*second, point))
        })?;
    unit_vector(Vector3::new(
        candidate.x - point.x,
        candidate.y - point.y,
        candidate.z - point.z,
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
    let carrier = ir
        .model
        .curves
        .iter()
        .find(|candidate| &candidate.id == curve)?;
    unit_vector(curve_tangent(&carrier.geometry, parameter)?)
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
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => {
            closest_periodic_analytic_curve_parameter(&carrier.geometry, point, seed)
        }
        CurveGeometry::Nurbs(nurbs) => closest_nurbs_curve_parameter(nurbs, point, seed),
        _ => None,
    }
}

fn closest_periodic_analytic_curve_parameter(
    geometry: &CurveGeometry,
    point: Point3,
    seed: Option<f64>,
) -> Option<f64> {
    let (center, axis, reference) = match geometry {
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            ..
        } => (*center, *axis, *ref_direction),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            ..
        } => (*center, *axis, *major_direction),
        _ => return None,
    };
    let transverse = cross_vector(axis, reference);
    let delta = Vector3::new(point.x - center.x, point.y - center.y, point.z - center.z);
    let phase = dot_vector(delta, transverse).atan2(dot_vector(delta, reference));
    phase.is_finite().then_some(())?;
    let circle_parameter = seed.map_or(phase, |seed| {
        phase + ((seed - phase) / std::f64::consts::TAU).round() * std::f64::consts::TAU
    });
    if matches!(geometry, CurveGeometry::Circle { .. }) {
        return Some(circle_parameter);
    }
    let anchor = seed.unwrap_or(phase);
    let CurveGeometry::Ellipse {
        major_radius,
        minor_radius,
        ..
    } = geometry
    else {
        unreachable!("periodic analytic curve is a circle or ellipse");
    };
    let x = dot_vector(delta, reference);
    let y = dot_vector(delta, transverse);
    let difference = minor_radius * minor_radius - major_radius * major_radius;
    let coefficients = [
        -*minor_radius * y,
        2.0 * (difference + major_radius * x),
        0.0,
        2.0 * (major_radius * x - difference),
        *minor_radius * y,
    ];
    let constant_distance = coefficients.iter().all(|coefficient| *coefficient == 0.0);
    let roots = real_polynomial_roots(&coefficients)?;
    let parameters = roots
        .into_iter()
        .map(|root| 2.0 * root.atan())
        .chain([0.0, std::f64::consts::PI])
        .chain(constant_distance.then_some(anchor))
        .map(|parameter| {
            parameter
                + ((anchor - parameter) / std::f64::consts::TAU).round() * std::f64::consts::TAU
        });
    let squared_distance = |parameter| {
        let position = curve_point(geometry, parameter)?;
        Some(
            (position.x - point.x).powi(2)
                + (position.y - point.y).powi(2)
                + (position.z - point.z).powi(2),
        )
    };
    closest_parameter_candidates(
        parameters
            .map(|parameter| Some((parameter, squared_distance(parameter)?)))
            .collect::<Option<Vec<_>>>()?,
        Some(anchor),
    )?
    .into_iter()
    .next()
}

fn real_polynomial_roots(coefficients: &[f64]) -> Option<Vec<f64>> {
    if coefficients
        .iter()
        .any(|coefficient| !coefficient.is_finite())
    {
        return None;
    }
    let mut roots = polynomial_roots_in_unit_interval(coefficients)?;
    let reversed = coefficients.iter().rev().copied().collect::<Vec<_>>();
    roots.extend(
        polynomial_roots_in_unit_interval(&reversed)?
            .into_iter()
            .filter(|root| *root != 0.0)
            .map(f64::recip),
    );
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|first, second| {
        (*first - *second).abs() <= 256.0 * f64::EPSILON * first.abs().max(second.abs()).max(1.0)
    });
    Some(roots)
}

fn polynomial_roots_in_unit_interval(coefficients: &[f64]) -> Option<Vec<f64>> {
    let mut coefficients = coefficients.to_vec();
    while coefficients
        .last()
        .is_some_and(|coefficient| *coefficient == 0.0)
    {
        coefficients.pop();
    }
    if coefficients.is_empty() {
        return Some(Vec::new());
    }
    let degree = coefficients.len().checked_sub(1)?;
    if degree == 0 {
        return Some(Vec::new());
    }
    let scale = coefficients
        .iter()
        .fold(0.0_f64, |scale, coefficient| scale.max(coefficient.abs()));
    if !scale.is_finite() || scale == 0.0 {
        return Some(Vec::new());
    }
    for coefficient in &mut coefficients {
        *coefficient /= scale;
    }
    if degree == 1 {
        let root = -coefficients[0] / coefficients[1];
        return root.is_finite().then(|| {
            if (-1.0..=1.0).contains(&root) {
                vec![root]
            } else {
                Vec::new()
            }
        });
    }
    let derivative = coefficients
        .iter()
        .enumerate()
        .skip(1)
        .map(|(degree, coefficient)| *coefficient * degree as f64)
        .collect::<Vec<_>>();
    let mut critical = polynomial_roots_in_unit_interval(&derivative)?;
    critical.sort_by(f64::total_cmp);
    critical.dedup_by(|first, second| {
        (*first - *second).abs() <= 64.0 * f64::EPSILON * first.abs().max(second.abs()).max(1.0)
    });
    let value = |parameter| polynomial_value(&coefficients, parameter);
    let tolerance = |parameter: f64| {
        256.0
            * f64::EPSILON
            * coefficients.iter().rev().fold(0.0, |bound, coefficient| {
                bound * parameter.abs() + coefficient.abs()
            })
    };
    let mut roots = critical
        .iter()
        .copied()
        .filter(|root| value(*root).abs() <= tolerance(*root))
        .collect::<Vec<_>>();
    let partitions = std::iter::once(-1.0)
        .chain(critical)
        .chain(std::iter::once(1.0))
        .collect::<Vec<_>>();
    for pair in partitions.windows(2) {
        let mut lower = pair[0];
        let mut upper = pair[1];
        let mut lower_value = value(lower);
        let upper_value = value(upper);
        if lower_value.abs() <= tolerance(lower) {
            roots.push(lower);
            continue;
        }
        if upper_value.abs() <= tolerance(upper) {
            roots.push(upper);
            continue;
        }
        if lower_value.is_sign_positive() == upper_value.is_sign_positive() {
            continue;
        }
        for _ in 0..128 {
            let middle = lower + (upper - lower) * 0.5;
            if middle == lower || middle == upper {
                break;
            }
            let middle_value = value(middle);
            if middle_value.abs() <= tolerance(middle) {
                lower = middle;
                upper = middle;
                break;
            }
            if middle_value.is_sign_positive() == lower_value.is_sign_positive() {
                lower = middle;
                lower_value = middle_value;
            } else {
                upper = middle;
            }
        }
        roots.push(lower + (upper - lower) * 0.5);
    }
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|first, second| {
        (*first - *second).abs() <= 256.0 * f64::EPSILON * first.abs().max(second.abs()).max(1.0)
    });
    Some(roots)
}

fn polynomial_value(coefficients: &[f64], parameter: f64) -> f64 {
    coefficients
        .iter()
        .rev()
        .fold(0.0, |value, coefficient| value * parameter + coefficient)
}

fn closest_nurbs_curve_parameter(
    curve: &NurbsCurve,
    point: Point3,
    seed: Option<f64>,
) -> Option<f64> {
    let degree = usize::try_from(curve.degree).ok()?;
    let count = curve.control_points.len();
    if degree == 0
        || count <= degree
        || curve.knots.len() != count.checked_add(degree)?.checked_add(1)?
        || curve.knots.iter().any(|knot| !knot.is_finite())
        || curve.knots.windows(2).any(|pair| pair[0] > pair[1])
        || curve.control_points.iter().any(|control| {
            !control.x.is_finite() || !control.y.is_finite() || !control.z.is_finite()
        })
        || !point.x.is_finite()
        || !point.y.is_finite()
        || !point.z.is_finite()
    {
        return None;
    }
    let domain = [*curve.knots.get(degree)?, *curve.knots.get(count)?];
    if !domain[0].is_finite() || !domain[1].is_finite() || domain[0] >= domain[1] {
        return None;
    }
    if seed.is_some_and(|seed| !seed.is_finite()) {
        return None;
    }
    let search_seed = seed.map(|seed| canonical_periodic_parameter(domain, curve.periodic, seed));
    let weights = match &curve.weights {
        Some(weights)
            if weights.len() == count
                && weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0) =>
        {
            weights.clone()
        }
        Some(_) => return None,
        None => vec![1.0; count],
    };
    let coordinate_scale = curve
        .control_points
        .iter()
        .flat_map(|control| [control.x, control.y, control.z])
        .chain([point.x, point.y, point.z])
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    let controls = curve
        .control_points
        .iter()
        .zip(weights)
        .map(|(control, weight)| {
            [
                weight * (control.x - point.x),
                weight * (control.y - point.y),
                weight * (control.z - point.z),
                weight,
            ]
        })
        .collect::<Vec<_>>();
    if controls.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }
    let homogeneous = HomogeneousCurveSpans {
        spans: bezier_spans(degree, &curve.knots, controls)?,
        coordinate_tolerance: 64.0 * f64::EPSILON * coordinate_scale,
    };
    let parameters = closest_parameter_candidates(
        stationary_rational_distance_candidates(&homogeneous, search_seed)?,
        search_seed,
    )?;
    lift_periodic_parameters(parameters, domain, curve.periodic, seed)
        .into_iter()
        .next()
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
        .or_else(|| initial_surface_parameters(ir, support, point, None, fit_tolerance))
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
    fit_tolerance: Option<f64>,
) -> Option<Point2> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    match &carrier.geometry {
        SurfaceGeometry::Nurbs(nurbs) => {
            nurbs_parameters_with_tolerance(nurbs, point, seed, fit_tolerance)
        }
        SurfaceGeometry::Procedural { construction } => {
            let procedural =
                ir.model.procedural_surfaces.iter().find(|candidate| {
                    &candidate.id == construction && &candidate.surface == surface
                })?;
            let ProceduralSurfaceDefinition::Offset { support, .. } = &procedural.definition else {
                return None;
            };
            initial_surface_parameters(ir, support, point, seed, fit_tolerance)
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
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    if let Some(partials) = surface_partials(&carrier.geometry, parameters.u, parameters.v) {
        return Some(if along_u { partials.du } else { partials.dv });
    }
    if let Some(partials) = model_surface_partials_by_id(ir, surface, parameters.u, parameters.v) {
        return Some(if along_u { partials.du } else { partials.dv });
    }

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
            SurfaceGeometry::Nurbs(nurbs) => {
                nurbs_parameters_with_tolerance(nurbs, point, seed, Some(fit_tolerance))
            }
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
        let rhs = residual.map(|value| -value);
        let step =
            solve_4x4(matrix, rhs).or_else(|| solve_damped_least_squares_4x4(matrix, rhs))?;
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

/// Return a finite correction for a consistent rank-deficient linearization.
///
/// Tangential surface intersections can lose one first-order direction even
/// though the chart-selected nonlinear branch remains well defined. The
/// column-scaled diagonal term selects a bounded minimum-norm correction
/// without making the result depend on unlike support parameter units. The
/// caller still requires the corrected nonlinear surfaces to agree and to
/// remain inside the source chart tolerance, so this fallback cannot qualify a
/// nearby branch.
pub(crate) fn solve_damped_least_squares_4x4(
    matrix: [[f64; 4]; 4],
    rhs: [f64; 4],
) -> Option<[f64; 4]> {
    if !matrix.iter().flatten().all(|value| value.is_finite())
        || !rhs.iter().all(|value| value.is_finite())
    {
        return None;
    }
    let normal: [[f64; 4]; 4] = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            (0..4)
                .map(|index| matrix[index][row] * matrix[index][column])
                .sum::<f64>()
        })
    });
    let normal_rhs: [f64; 4] = std::array::from_fn(|column| {
        (0..4)
            .map(|index| matrix[index][column] * rhs[index])
            .sum::<f64>()
    });
    let max_column_scale = (0..4)
        .map(|index| normal[index][index].abs())
        .fold(0.0_f64, f64::max)
        .sqrt();
    if !max_column_scale.is_finite() || max_column_scale == 0.0 {
        return None;
    }
    let column_scales: [f64; 4] = std::array::from_fn(|index| {
        normal[index][index]
            .max(0.0)
            .sqrt()
            .max(max_column_scale * 1.0e-12)
    });
    let scaled_normal: [[f64; 4]; 4] = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            normal[row][column] / (column_scales[row] * column_scales[column])
        })
    });
    let scaled_rhs: [f64; 4] =
        std::array::from_fn(|column| normal_rhs[column] / column_scales[column]);
    let initial_error = rhs.iter().map(|value| value * value).sum::<f64>();
    for exponent in -12..=-3 {
        let damping = 10_f64.powi(exponent);
        let mut regularized = scaled_normal;
        for (index, row) in regularized.iter_mut().enumerate() {
            row[index] += damping;
        }
        let Some(scaled_step) = solve_4x4(regularized, scaled_rhs) else {
            continue;
        };
        let step: [f64; 4] = std::array::from_fn(|index| scaled_step[index] / column_scales[index]);
        let linear_error = (0..4)
            .map(|row| {
                let residual = (0..4)
                    .map(|column| matrix[row][column] * step[column])
                    .sum::<f64>()
                    - rhs[row];
                residual * residual
            })
            .sum::<f64>();
        if linear_error.is_finite() && linear_error < initial_error {
            return Some(step);
        }
    }
    None
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

#[derive(Clone)]
struct RationalBezierSurfacePatch {
    u_domain: [f64; 2],
    v_domain: [f64; 2],
    u_degree: usize,
    v_degree: usize,
    controls: Vec<[f64; 4]>,
}

fn rational_surface_residual_patches(
    surface: &NurbsSurface,
    point: Point3,
) -> Option<Vec<RationalBezierSurfacePatch>> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    let control_count = u_count.checked_mul(v_count)?;
    if u_degree == 0
        || v_degree == 0
        || u_degree >= u_count
        || v_degree >= v_count
        || surface.control_points.len() != control_count
        || surface.u_knots.len() != u_count.checked_add(u_degree)?.checked_add(1)?
        || surface.v_knots.len() != v_count.checked_add(v_degree)?.checked_add(1)?
        || surface
            .u_knots
            .iter()
            .chain(&surface.v_knots)
            .any(|knot| !knot.is_finite())
        || surface.u_knots.windows(2).any(|pair| pair[0] > pair[1])
        || surface.v_knots.windows(2).any(|pair| pair[0] > pair[1])
        || surface.control_points.iter().any(|control| {
            !control.x.is_finite() || !control.y.is_finite() || !control.z.is_finite()
        })
        || !point.x.is_finite()
        || !point.y.is_finite()
        || !point.z.is_finite()
    {
        return None;
    }
    let weights = match &surface.weights {
        Some(weights)
            if weights.len() == control_count
                && weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0) =>
        {
            weights.clone()
        }
        Some(_) => return None,
        None => vec![1.0; control_count],
    };
    let residual_controls = surface
        .control_points
        .iter()
        .zip(weights)
        .map(|(control, weight)| {
            [
                weight * (control.x - point.x),
                weight * (control.y - point.y),
                weight * (control.z - point.z),
                weight,
            ]
        })
        .collect::<Vec<_>>();
    if residual_controls
        .iter()
        .flatten()
        .any(|value| !value.is_finite())
    {
        return None;
    }
    let u_spans_by_v = (0..v_count)
        .map(|v| {
            bezier_spans(
                u_degree,
                &surface.u_knots,
                (0..u_count)
                    .map(|u| residual_controls[u * v_count + v])
                    .collect(),
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let u_span_count = u_spans_by_v.first()?.len();
    if u_span_count == 0 || u_spans_by_v.iter().any(|spans| spans.len() != u_span_count) {
        return None;
    }
    let mut patches = Vec::new();
    for u_span in 0..u_span_count {
        let u_domain = u_spans_by_v[0][u_span].domain;
        if u_spans_by_v
            .iter()
            .any(|spans| spans[u_span].domain != u_domain)
        {
            return None;
        }
        let v_spans_by_u = (0..=u_degree)
            .map(|u_control| {
                bezier_spans(
                    v_degree,
                    &surface.v_knots,
                    (0..v_count)
                        .map(|v| u_spans_by_v[v][u_span].controls[u_control])
                        .collect(),
                )
            })
            .collect::<Option<Vec<_>>>()?;
        let v_span_count = v_spans_by_u.first()?.len();
        if v_span_count == 0 || v_spans_by_u.iter().any(|spans| spans.len() != v_span_count) {
            return None;
        }
        for v_span in 0..v_span_count {
            let v_domain = v_spans_by_u[0][v_span].domain;
            if v_spans_by_u
                .iter()
                .any(|spans| spans[v_span].domain != v_domain)
            {
                return None;
            }
            patches.push(RationalBezierSurfacePatch {
                u_domain,
                v_domain,
                u_degree,
                v_degree,
                controls: (0..=u_degree)
                    .flat_map(|u| v_spans_by_u[u][v_span].controls.iter().copied())
                    .collect(),
            });
        }
    }
    (!patches.is_empty()).then_some(patches)
}

fn rational_patch_distance_bounds(patch: &RationalBezierSurfacePatch) -> Option<(f64, f64)> {
    let mut minimum = [f64::INFINITY; 3];
    let mut maximum = [f64::NEG_INFINITY; 3];
    for control in &patch.controls {
        if !control[3].is_finite() || control[3] <= 0.0 {
            return None;
        }
        for axis in 0..3 {
            let coordinate = control[axis] / control[3];
            if !coordinate.is_finite() {
                return None;
            }
            minimum[axis] = minimum[axis].min(coordinate);
            maximum[axis] = maximum[axis].max(coordinate);
        }
    }
    let lower = (0..3)
        .map(|axis| {
            if minimum[axis] > 0.0 {
                minimum[axis] * minimum[axis]
            } else if maximum[axis] < 0.0 {
                maximum[axis] * maximum[axis]
            } else {
                0.0
            }
        })
        .sum::<f64>();
    let diameter = (0..3)
        .map(|axis| (maximum[axis] - minimum[axis]).powi(2))
        .sum::<f64>();
    (lower.is_finite() && diameter.is_finite()).then_some((lower, diameter))
}

fn split_rational_surface_patch(
    patch: &RationalBezierSurfacePatch,
    split_u: bool,
) -> Option<[RationalBezierSurfacePatch; 2]> {
    let (degree, line_count) = if split_u {
        (patch.u_degree, patch.v_degree + 1)
    } else {
        (patch.v_degree, patch.u_degree + 1)
    };
    let mut first_lines = Vec::with_capacity(line_count);
    let mut second_lines = Vec::with_capacity(line_count);
    for line in 0..line_count {
        let controls = if split_u {
            (0..=degree)
                .map(|index| patch.controls[index * (patch.v_degree + 1) + line])
                .collect::<Vec<_>>()
        } else {
            patch.controls[line * (patch.v_degree + 1)..(line + 1) * (patch.v_degree + 1)].to_vec()
        };
        let mut levels = vec![controls];
        while levels.last()?.len() > 1 {
            levels.push(
                levels
                    .last()?
                    .windows(2)
                    .map(|pair| std::array::from_fn(|axis| 0.5 * (pair[0][axis] + pair[1][axis])))
                    .collect(),
            );
        }
        first_lines.push(levels.iter().map(|level| level[0]).collect::<Vec<_>>());
        second_lines.push(
            levels
                .iter()
                .rev()
                .map(|level| *level.last().expect("nonempty de Casteljau level"))
                .collect::<Vec<_>>(),
        );
    }
    let assemble = |lines: Vec<Vec<[f64; 4]>>| {
        if split_u {
            (0..=patch.u_degree)
                .flat_map(|u| {
                    (0..=patch.v_degree).map({
                        let lines = &lines;
                        move |v| lines[v][u]
                    })
                })
                .collect()
        } else {
            lines.into_iter().flatten().collect()
        }
    };
    let u_middle = patch.u_domain[0] + (patch.u_domain[1] - patch.u_domain[0]) * 0.5;
    let v_middle = patch.v_domain[0] + (patch.v_domain[1] - patch.v_domain[0]) * 0.5;
    let (first_u, second_u, first_v, second_v) = if split_u {
        (
            [patch.u_domain[0], u_middle],
            [u_middle, patch.u_domain[1]],
            patch.v_domain,
            patch.v_domain,
        )
    } else {
        (
            patch.u_domain,
            patch.u_domain,
            [patch.v_domain[0], v_middle],
            [v_middle, patch.v_domain[1]],
        )
    };
    if split_u && (u_middle == patch.u_domain[0] || u_middle == patch.u_domain[1])
        || !split_u && (v_middle == patch.v_domain[0] || v_middle == patch.v_domain[1])
    {
        return None;
    }
    Some([
        RationalBezierSurfacePatch {
            u_domain: first_u,
            v_domain: first_v,
            u_degree: patch.u_degree,
            v_degree: patch.v_degree,
            controls: assemble(first_lines),
        },
        RationalBezierSurfacePatch {
            u_domain: second_u,
            v_domain: second_v,
            u_degree: patch.u_degree,
            v_degree: patch.v_degree,
            controls: assemble(second_lines),
        },
    ])
}

fn complete_nurbs_surface_starts(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
) -> Option<Vec<Point2>> {
    const MAX_PATCHES: usize = 1_000_000;

    let mut patches = rational_surface_residual_patches(surface, point)?;
    let coordinate_scale =
        patches
            .iter()
            .flat_map(|patch| &patch.controls)
            .try_fold(1.0_f64, |scale, control| {
                let weight = control[3];
                if !weight.is_finite() || weight <= 0.0 {
                    return None;
                }
                control[..3].iter().try_fold(scale, |scale, coordinate| {
                    let coordinate = (coordinate / weight).abs();
                    coordinate.is_finite().then(|| scale.max(coordinate))
                })
            })?;
    let requested_tolerance = match fit_tolerance {
        Some(tolerance) if tolerance.is_finite() && tolerance >= 0.0 => tolerance,
        Some(_) => return None,
        None => 0.0,
    };
    let distance_tolerance = requested_tolerance.max(256.0 * f64::EPSILON * coordinate_scale);
    let squared_tolerance = distance_tolerance * distance_tolerance;
    let squared_distance = |parameters: Point2| {
        let position = cadmpeg_ir::eval::nurbs_surface_point(surface, parameters.u, parameters.v)?;
        let distance = point_distance(position, point);
        distance.is_finite().then_some(distance * distance)
    };
    let position_squared_distance = |position: Point3| point_distance(position, point).powi(2);
    let center = |patch: &RationalBezierSurfacePatch| {
        Point2::new(
            patch.u_domain[0] + (patch.u_domain[1] - patch.u_domain[0]) * 0.5,
            patch.v_domain[0] + (patch.v_domain[1] - patch.v_domain[0]) * 0.5,
        )
    };
    let surface_u_domain = [
        *surface
            .u_knots
            .get(usize::try_from(surface.u_degree).ok()?)?,
        *surface
            .u_knots
            .get(usize::try_from(surface.u_count).ok()?)?,
    ];
    let surface_v_domain = [
        *surface
            .v_knots
            .get(usize::try_from(surface.v_degree).ok()?)?,
        *surface
            .v_knots
            .get(usize::try_from(surface.v_count).ok()?)?,
    ];
    let refined_upper = |start, u_domain, v_domain| {
        let parameters = refine_nurbs_surface_parameters(
            surface,
            point,
            start,
            u_domain,
            v_domain,
            &position_squared_distance,
        )
        .unwrap_or(start);
        Some((parameters, squared_distance(parameters)?))
    };
    let mut best_distance = f64::INFINITY;
    let mut best_upper_parameters = Vec::new();
    {
        let mut consider_upper = |(parameters, distance): (Point2, f64)| {
            if !best_distance.is_finite() {
                best_distance = distance;
                best_upper_parameters.push(parameters);
                return;
            }
            let tolerance = 128.0
                * f64::EPSILON
                * distance
                    .abs()
                    .max(best_distance.abs())
                    .max(squared_tolerance);
            if distance < best_distance && best_distance - distance > tolerance {
                best_distance = distance;
                best_upper_parameters.clear();
            }
            if (distance - best_distance).abs() <= tolerance {
                best_upper_parameters.push(parameters);
            }
        };
        if let Some(candidate) =
            seed.and_then(|seed| refined_upper(seed, surface_u_domain, surface_v_domain))
        {
            consider_upper(candidate);
        }
        for patch in &patches {
            consider_upper(refined_upper(
                center(patch),
                patch.u_domain,
                patch.v_domain,
            )?);
        }
    }
    best_distance.is_finite().then_some(())?;
    let mut terminal = Vec::<(Point2, f64)>::new();
    let mut examined = 0usize;
    while let Some(patch) = patches.pop() {
        examined += 1;
        if examined > MAX_PATCHES {
            return None;
        }
        let (lower_bound, diameter) = rational_patch_distance_bounds(&patch)?;
        let comparison_tolerance = 128.0
            * f64::EPSILON
            * lower_bound
                .abs()
                .max(best_distance.abs())
                .max(squared_tolerance);
        if lower_bound > best_distance + comparison_tolerance {
            continue;
        }
        let parameters = center(&patch);
        let (upper_parameters, center_distance) =
            refined_upper(parameters, patch.u_domain, patch.v_domain)?;
        let upper_tolerance = 128.0
            * f64::EPSILON
            * center_distance
                .abs()
                .max(best_distance.abs())
                .max(squared_tolerance);
        if center_distance < best_distance && best_distance - center_distance > upper_tolerance {
            best_distance = center_distance;
            best_upper_parameters.clear();
        }
        if (center_distance - best_distance).abs() <= upper_tolerance {
            best_upper_parameters.push(upper_parameters);
        }
        let indivisible = parameters.u == patch.u_domain[0]
            || parameters.u == patch.u_domain[1]
            || parameters.v == patch.v_domain[0]
            || parameters.v == patch.v_domain[1];
        if diameter <= squared_tolerance
            || center_distance - lower_bound <= squared_tolerance
            || indivisible
        {
            terminal.push((upper_parameters, lower_bound));
            continue;
        }
        let control = |u: usize, v: usize| {
            let homogeneous = patch.controls[u * (patch.v_degree + 1) + v];
            [
                homogeneous[0] / homogeneous[3],
                homogeneous[1] / homogeneous[3],
                homogeneous[2] / homogeneous[3],
            ]
        };
        let u_variation = (0..patch.u_degree)
            .flat_map(|u| (0..=patch.v_degree).map(move |v| (u, v)))
            .map(|(u, v)| {
                let first = control(u, v);
                let second = control(u + 1, v);
                (0..3)
                    .map(|axis| (second[axis] - first[axis]).powi(2))
                    .sum::<f64>()
            })
            .fold(0.0_f64, f64::max);
        let v_variation = (0..=patch.u_degree)
            .flat_map(|u| (0..patch.v_degree).map(move |v| (u, v)))
            .map(|(u, v)| {
                let first = control(u, v);
                let second = control(u, v + 1);
                (0..3)
                    .map(|axis| (second[axis] - first[axis]).powi(2))
                    .sum::<f64>()
            })
            .fold(0.0_f64, f64::max);
        let split_u = u_variation >= v_variation;
        let children = split_rational_surface_patch(&patch, split_u)?;
        patches.extend(children);
    }
    let final_tolerance = 128.0 * f64::EPSILON * best_distance.abs().max(squared_tolerance);
    let mut starts = terminal
        .into_iter()
        .filter_map(|(parameters, lower)| {
            (lower <= best_distance + final_tolerance).then_some(parameters)
        })
        .collect::<Vec<_>>();
    starts.extend(best_upper_parameters);
    (!starts.is_empty()).then_some(starts)
}

pub(crate) fn nurbs_parameters(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
) -> Option<Point2> {
    nurbs_parameters_with_tolerance(surface, point, seed, None)
}

fn nurbs_parameters_with_tolerance(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
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
    let starts = complete_nurbs_surface_starts(surface, point, seed, fit_tolerance)?;
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
    (first.x - second.x)
        .hypot(first.y - second.y)
        .hypot(first.z - second.z)
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
        if uv
            .iter()
            .flatten()
            .any(|value| missing_support_parameter(*value))
        {
            return None;
        }
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
    let mut serialized_branch_pcurves = BTreeSet::new();
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
        let pcurve_use_range = trim_ranges
            .get(&fields.curve_xmt)
            .copied()
            .and_then(ordered_parameter_range);
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
                pcurve_use_range.or(carrier.parameter_range),
                carrier.fit_tolerance,
            )
        });
        let edge_curve = ir
            .model
            .edges
            .iter()
            .find(|candidate| candidate.id == edge)
            .and_then(|edge| edge.curve.as_ref());
        if let (Some(pcurve), Some(edge_curve), Some(support)) =
            (pcurve.as_ref(), edge_curve, support.as_ref())
        {
            if curves.get(&fields.curve_xmt) == Some(edge_curve)
                && pcurve_supports.get(&fields.curve_xmt) == Some(support)
            {
                serialized_branch_pcurves.insert((
                    edge_curve.clone(),
                    support.clone(),
                    pcurve.clone(),
                ));
            }
        }
        let attached_pcurve_use_range = pcurve.as_ref().and(pcurve_use_range);
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
                    parameter_range: attached_pcurve_use_range,
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
    complete_tolerant_intersection_pcurves_from_serialized_branches(
        ir,
        &serialized_branch_pcurves,
        annotations,
    );
    complete_exact_boundary_intersection_pcurves(ir, annotations);
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

fn complete_tolerant_intersection_pcurves_from_serialized_branches(
    ir: &mut CadIr,
    serialized: &BTreeSet<(CurveId, SurfaceId, PcurveId)>,
    annotations: &mut AnnotationBuilder,
) {
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
    let mut incident = BTreeMap::<(CurveId, SurfaceId), Vec<(PcurveId, Option<[f64; 2]>)>>::new();
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
        for use_ in &coedge.pcurves {
            if !serialized.contains(&(curve.clone(), surface.clone(), use_.pcurve.clone())) {
                continue;
            }
            let candidates = incident
                .entry((curve.clone(), surface.clone()))
                .or_default();
            let candidate = (use_.pcurve.clone(), use_.parameter_range);
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }

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
    let mut replacements = Vec::new();
    for procedural in &ir.model.procedural_curves {
        let ProceduralCurveDefinition::TolerantIntersection {
            supports,
            endpoints,
            tolerance: _,
            parameterization: None,
        } = &procedural.definition
        else {
            continue;
        };
        let edges = ir
            .model
            .edges
            .iter()
            .filter(|edge| edge.curve.as_ref() == Some(&procedural.curve))
            .collect::<Vec<_>>();
        let [edge] = edges.as_slice() else {
            continue;
        };
        let Some(endpoint_tolerance) = edge
            .tolerance
            .filter(|value| value.is_finite() && *value >= 0.0)
        else {
            continue;
        };
        let edge_reversed = match (vertex_points.get(&edge.start), vertex_points.get(&edge.end)) {
            (Some(start), Some(end)) => {
                let forward = point_distance(*start, endpoints[0]) <= endpoint_tolerance
                    && point_distance(*end, endpoints[1]) <= endpoint_tolerance;
                let reversed = point_distance(*start, endpoints[1]) <= endpoint_tolerance
                    && point_distance(*end, endpoints[0]) <= endpoint_tolerance;
                match (forward, reversed) {
                    (true, false) => false,
                    (false, true) => true,
                    (true, true) if edge.start == edge.end => false,
                    _ => continue,
                }
            }
            _ => continue,
        };
        let candidates = supports.each_ref().map(|support| {
            incident
                .get(&(procedural.curve.clone(), support.clone()))
                .map(Vec::as_slice)
        });
        let [Some([(first_id, first_use_range)]), Some([(second_id, second_use_range)])] =
            candidates
        else {
            continue;
        };
        let carriers = [first_id, second_id].map(|id| {
            ir.model
                .pcurves
                .iter()
                .find(|candidate| &candidate.id == id)
        });
        let [Some(first), Some(second)] = carriers else {
            continue;
        };
        let ranges = [
            first_use_range
                .or(first.parameter_range)
                .or_else(|| pcurve_parameter_range(&first.geometry)),
            second_use_range
                .or(second.parameter_range)
                .or_else(|| pcurve_parameter_range(&second.geometry)),
        ];
        let [Some(first_range), Some(second_range)] = ranges else {
            continue;
        };
        if !first_range
            .iter()
            .zip(second_range)
            .all(|(first, second)| first.to_bits() == second.to_bits())
            || !first_range[0].is_finite()
            || !first_range[1].is_finite()
            || first_range[0] >= first_range[1]
        {
            continue;
        }
        if edge.param_range.is_some_and(|range| {
            !range
                .iter()
                .zip(first_range)
                .all(|(existing, branch)| existing.to_bits() == branch.to_bits())
        }) {
            continue;
        }
        let Some(()) = first
            .fit_tolerance
            .zip(second.fit_tolerance)
            .map(|(first, second)| first + second)
            .filter(|bound| bound.is_finite() && *bound <= endpoint_tolerance)
            .map(|_| ())
        else {
            continue;
        };
        let carriers = [first, second];
        let pcurves: [Option<PcurveGeometry>; 2] = std::array::from_fn(|side| {
            orient_tolerant_intersection_pcurve(
                ir,
                &procedural.curve,
                &supports[side],
                &carriers[side].geometry,
                first_range,
                *endpoints,
                endpoint_tolerance,
            )
        });
        if let [Some(first), Some(second)] = pcurves {
            replacements.push((
                procedural.id.clone(),
                edge.id.clone(),
                edge_reversed,
                TolerantIntersectionParameterization {
                    pcurves: [first, second],
                    parameter_range: first_range,
                },
            ));
        }
    }

    for (procedural_id, edge_id, edge_reversed, parameterization) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::TolerantIntersection {
            parameterization: slot,
            ..
        } = &mut procedural.definition
        else {
            continue;
        };
        if slot.is_some() {
            continue;
        }
        let range = parameterization.parameter_range;
        *slot = Some(parameterization);
        if let Some(edge) = ir.model.edges.iter_mut().find(|edge| edge.id == edge_id) {
            if edge_reversed {
                std::mem::swap(&mut edge.start, &mut edge.end);
            }
            edge.param_range = Some(range);
            annotations.derived(&edge.id, "param_range");
        }
    }
}

fn orient_tolerant_intersection_pcurve(
    ir: &CadIr,
    curve: &CurveId,
    support: &SurfaceId,
    pcurve: &PcurveGeometry,
    range: [f64; 2],
    endpoints: [Point3; 2],
    tolerance: f64,
) -> Option<PcurveGeometry> {
    let points = range.map(|parameter| {
        let uv = pcurve_uv(pcurve, parameter)?;
        decoded_surface_point(ir, support, uv.u, uv.v)
    });
    let [Some(first), Some(second)] = points else {
        return None;
    };
    let forward = point_distance(first, endpoints[0]) <= tolerance
        && point_distance(second, endpoints[1]) <= tolerance;
    let reversed = point_distance(first, endpoints[1]) <= tolerance
        && point_distance(second, endpoints[0]) <= tolerance;
    match (forward, reversed) {
        (true, false) => Some(pcurve.clone()),
        (false, true) => reverse_pcurve_over_range(pcurve, range),
        (true, true) => {
            let reversed = reverse_pcurve_over_range(pcurve, range)?;
            let curve_tangent = model_curve_tangent(ir, curve, range[0])?;
            let alignment = |candidate: &PcurveGeometry| {
                let uv = pcurve_uv(candidate, range[0])?;
                let uv_tangent = pcurve_tangent(candidate, range[0])?;
                let partials = model_surface_partials_by_id(ir, support, uv.u, uv.v)?;
                let tangent = unit_vector(Vector3::new(
                    uv_tangent.u * partials.du.x + uv_tangent.v * partials.dv.x,
                    uv_tangent.u * partials.du.y + uv_tangent.v * partials.dv.y,
                    uv_tangent.u * partials.du.z + uv_tangent.v * partials.dv.z,
                ))?;
                Some(dot_vector(curve_tangent, tangent))
            };
            match (alignment(pcurve)?, alignment(&reversed)?) {
                (forward_alignment, reversed_alignment)
                    if forward_alignment > 0.0 && reversed_alignment <= 0.0 =>
                {
                    Some(pcurve.clone())
                }
                (forward_alignment, reversed_alignment)
                    if reversed_alignment > 0.0 && forward_alignment <= 0.0 =>
                {
                    Some(reversed)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn reverse_pcurve_over_range(
    pcurve: &PcurveGeometry,
    [start, end]: [f64; 2],
) -> Option<PcurveGeometry> {
    let reflection = start + end;
    if !reflection.is_finite() {
        return None;
    }
    let combine = |first: Point2, first_scale: f64, second: Point2, second_scale: f64| {
        let value = Point2::new(
            first_scale * first.u + second_scale * second.u,
            first_scale * first.v + second_scale * second.v,
        );
        (value.u.is_finite() && value.v.is_finite()).then_some(value)
    };
    match pcurve {
        PcurveGeometry::Line { origin, direction } => Some(PcurveGeometry::Line {
            origin: Point2::new(
                origin.u + reflection * direction.u,
                origin.v + reflection * direction.v,
            ),
            direction: Point2::new(-direction.u, -direction.v),
        }),
        PcurveGeometry::PolarHarmonic {
            radial_center,
            radial_cos,
            radial_sin,
            axial_origin,
            axial_cos,
            axial_sin,
        } => {
            let cosine = reflection.cos();
            let sine = reflection.sin();
            Some(PcurveGeometry::PolarHarmonic {
                radial_center: *radial_center,
                radial_cos: Point2::new(
                    cosine * radial_cos.u + sine * radial_sin.u,
                    cosine * radial_cos.v + sine * radial_sin.v,
                ),
                radial_sin: Point2::new(
                    sine * radial_cos.u - cosine * radial_sin.u,
                    sine * radial_cos.v - cosine * radial_sin.v,
                ),
                axial_origin: *axial_origin,
                axial_cos: cosine * axial_cos + sine * axial_sin,
                axial_sin: sine * axial_cos - cosine * axial_sin,
            })
        }
        PcurveGeometry::Harmonic {
            center,
            cosine: source_cosine,
            sine: source_sine,
        } => {
            let cosine = reflection.cos();
            let sine = reflection.sin();
            Some(PcurveGeometry::Harmonic {
                center: *center,
                cosine: combine(*source_cosine, cosine, *source_sine, sine)?,
                sine: combine(*source_cosine, sine, *source_sine, -cosine)?,
            })
        }
        PcurveGeometry::Hyperbolic {
            center,
            cosine: source_cosine,
            sine: source_sine,
        } => {
            let cosine = reflection.cosh();
            let sine = reflection.sinh();
            Some(PcurveGeometry::Hyperbolic {
                center: *center,
                cosine: combine(*source_cosine, cosine, *source_sine, sine)?,
                sine: combine(*source_cosine, -sine, *source_sine, -cosine)?,
            })
        }
        PcurveGeometry::PolarNurbs {
            degree,
            knots,
            radial_control_points,
            axial_control_points,
            weights,
            periodic,
        } => {
            let reversed_knots = knots
                .iter()
                .rev()
                .map(|knot| reflection - knot)
                .collect::<Vec<_>>();
            let mut radial_control_points = radial_control_points.clone();
            radial_control_points.reverse();
            let mut axial_control_points = axial_control_points.clone();
            axial_control_points.reverse();
            let mut weights = weights.clone();
            if let Some(weights) = &mut weights {
                weights.reverse();
            }
            let finite = reversed_knots
                .iter()
                .chain(
                    radial_control_points
                        .iter()
                        .flat_map(|point| [&point.u, &point.v]),
                )
                .chain(&axial_control_points)
                .all(|value| value.is_finite());
            finite.then_some(PcurveGeometry::PolarNurbs {
                degree: *degree,
                knots: reversed_knots,
                radial_control_points,
                axial_control_points,
                weights,
                periodic: *periodic,
            })
        }
        PcurveGeometry::SphericalGreatCircle {
            azimuth_origin,
            azimuth_rate,
            plane_phase,
            plane_slope,
        } => {
            let reversed_origin = azimuth_origin + azimuth_rate * reflection;
            let reversed_rate = -*azimuth_rate;
            [reversed_origin, reversed_rate, *plane_phase, *plane_slope]
                .into_iter()
                .all(f64::is_finite)
                .then_some(PcurveGeometry::SphericalGreatCircle {
                    azimuth_origin: reversed_origin,
                    azimuth_rate: reversed_rate,
                    plane_phase: *plane_phase,
                    plane_slope: *plane_slope,
                })
        }
        PcurveGeometry::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => {
            let cosine = reflection.cos();
            let sine = reflection.sin();
            let reversed_x = Point2::new(
                cosine * x_axis.u + sine * y_axis.u,
                cosine * x_axis.v + sine * y_axis.v,
            );
            let reversed_y = Point2::new(
                sine * x_axis.u - cosine * y_axis.u,
                sine * x_axis.v - cosine * y_axis.v,
            );
            [reversed_x.u, reversed_x.v, reversed_y.u, reversed_y.v]
                .into_iter()
                .all(f64::is_finite)
                .then_some(PcurveGeometry::Circle {
                    center: *center,
                    x_axis: reversed_x,
                    y_axis: reversed_y,
                    radius: *radius,
                })
        }
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        } => {
            let reversed_knots = knots
                .iter()
                .rev()
                .map(|knot| reflection - knot)
                .collect::<Vec<_>>();
            let mut control_points = control_points.clone();
            control_points.reverse();
            let mut weights = weights.clone();
            if let Some(weights) = &mut weights {
                weights.reverse();
            }
            let finite = reversed_knots
                .iter()
                .chain(control_points.iter().flat_map(|point| [&point.u, &point.v]))
                .all(|value| value.is_finite());
            finite.then_some(PcurveGeometry::Nurbs {
                degree: *degree,
                knots: reversed_knots,
                control_points,
                weights,
                periodic: *periodic,
            })
        }
        PcurveGeometry::Trimmed {
            parameter_range,
            basis,
        } => Some(PcurveGeometry::Trimmed {
            parameter_range: *parameter_range,
            basis: Box::new(reverse_pcurve_over_range(basis, [start, end])?),
        }),
        PcurveGeometry::Offset { distance, basis } => Some(PcurveGeometry::Offset {
            distance: -*distance,
            basis: Box::new(reverse_pcurve_over_range(basis, [start, end])?),
        }),
        PcurveGeometry::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } if reflection == 0.0 => Some(PcurveGeometry::Ellipse {
            center: *center,
            x_axis: *x_axis,
            y_axis: Point2::new(-y_axis.u, -y_axis.v),
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        }),
        PcurveGeometry::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => {
            let cosine = reflection.cos();
            let sine = reflection.sin();
            Some(PcurveGeometry::Harmonic {
                center: *center,
                cosine: combine(*x_axis, major_radius * cosine, *y_axis, minor_radius * sine)?,
                sine: combine(
                    *x_axis,
                    major_radius * sine,
                    *y_axis,
                    -minor_radius * cosine,
                )?,
            })
        }
        PcurveGeometry::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } if reflection == 0.0 => Some(PcurveGeometry::Parabola {
            vertex: *vertex,
            x_axis: *x_axis,
            y_axis: Point2::new(-y_axis.u, -y_axis.v),
            focal_distance: *focal_distance,
        }),
        PcurveGeometry::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } if start.is_finite()
            && end.is_finite()
            && start < end
            && focal_distance.is_finite()
            && *focal_distance != 0.0 =>
        {
            let point = |parameter: f64| {
                let axial = parameter * parameter / (4.0 * focal_distance);
                Point2::new(
                    vertex.u + axial * x_axis.u + parameter * y_axis.u,
                    vertex.v + axial * x_axis.v + parameter * y_axis.v,
                )
            };
            let first = point(end);
            let last = point(start);
            let derivative = Point2::new(
                -(end / (2.0 * focal_distance) * x_axis.u + y_axis.u),
                -(end / (2.0 * focal_distance) * x_axis.v + y_axis.v),
            );
            let half_span = (end - start) * 0.5;
            let middle = Point2::new(
                first.u + half_span * derivative.u,
                first.v + half_span * derivative.v,
            );
            [first.u, first.v, middle.u, middle.v, last.u, last.v]
                .into_iter()
                .all(f64::is_finite)
                .then_some(PcurveGeometry::Nurbs {
                    degree: 2,
                    knots: vec![start, start, start, end, end, end],
                    control_points: vec![first, middle, last],
                    weights: None,
                    periodic: false,
                })
        }
        PcurveGeometry::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } if reflection == 0.0 => Some(PcurveGeometry::Hyperbola {
            center: *center,
            x_axis: *x_axis,
            y_axis: Point2::new(-y_axis.u, -y_axis.v),
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        }),
        PcurveGeometry::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => {
            let cosine = reflection.cosh();
            let sine = reflection.sinh();
            Some(PcurveGeometry::Hyperbolic {
                center: *center,
                cosine: combine(*x_axis, major_radius * cosine, *y_axis, minor_radius * sine)?,
                sine: combine(
                    *x_axis,
                    -major_radius * sine,
                    *y_axis,
                    -minor_radius * cosine,
                )?,
            })
        }
        PcurveGeometry::Parabola { .. } => None,
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
            Some((
                procedural.id.clone(),
                target,
                pcurve,
                tolerance,
                curve_is_cache_backed(ir, &procedural.curve),
            ))
        })
        .collect::<Vec<_>>();
    for (procedural_id, side, pcurve, tolerance, cache_backed) in replacements {
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
            if cache_backed {
                procedural.cache_fit_tolerance =
                    Some(procedural.cache_fit_tolerance.unwrap_or(0.0).max(tolerance));
            }
        }
    }
}

pub(crate) fn complete_exact_boundary_intersection_pcurves(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
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
            let edges = ir
                .model
                .edges
                .iter()
                .filter(|edge| edge.curve.as_ref() == Some(&procedural.curve))
                .collect::<Vec<_>>();
            let [edge] = edges.as_slice() else {
                return None;
            };
            let (supports, endpoints, range, tolerance, tolerant) = match &procedural.definition {
                ProceduralCurveDefinition::Intersection { context, .. } => {
                    if !context
                        .sides
                        .iter()
                        .all(|side| pcurve_requires_completion(side.pcurve.as_ref()))
                    {
                        return None;
                    }
                    (
                        [
                            context.sides[0].surface.as_ref()?,
                            context.sides[1].surface.as_ref()?,
                        ],
                        [
                            *vertex_points.get(&edge.start)?,
                            *vertex_points.get(&edge.end)?,
                        ],
                        context.parameter_range,
                        edge.tolerance
                            .filter(|value| value.is_finite() && *value >= 0.0)?,
                        false,
                    )
                }
                ProceduralCurveDefinition::TolerantIntersection {
                    supports,
                    endpoints,
                    tolerance,
                    parameterization: None,
                } => {
                    let range = if edge.start == edge.end
                        && ir
                            .model
                            .curves
                            .iter()
                            .find(|candidate| candidate.id == procedural.curve)
                            .is_some_and(|curve| {
                                matches!(
                                    curve.geometry,
                                    CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. }
                                )
                            }) {
                        [0.0, std::f64::consts::TAU]
                    } else {
                        [0.0, 1.0]
                    };
                    (supports.each_ref(), *endpoints, range, *tolerance, true)
                }
                _ => return None,
            };
            let [first_surface, second_surface] = supports;
            let candidates = [first_surface, second_surface].map(|surface| {
                exact_boundary_pcurve(ir, &procedural.curve, surface, endpoints, range, tolerance)
            });
            let pcurves = match candidates {
                [Some(first), Some(second)] => {
                    if coincident_pcurve_pair(
                        ir,
                        [first_surface, second_surface],
                        [&first, &second],
                        range,
                        tolerance,
                    ) {
                        [first, second]
                    } else {
                        let transferred = [
                            transfer_intersection_pcurve(
                                ir,
                                &procedural.curve,
                                first_surface,
                                &first,
                                second_surface,
                                range,
                                tolerance,
                            )
                            .map(|transferred| [first.clone(), transferred]),
                            transfer_intersection_pcurve(
                                ir,
                                &procedural.curve,
                                second_surface,
                                &second,
                                first_surface,
                                range,
                                tolerance,
                            )
                            .map(|transferred| [transferred, second.clone()]),
                        ];
                        match transferred {
                            [Some(pair), None] | [None, Some(pair)] => pair,
                            _ => return None,
                        }
                    }
                }
                [Some(first), None] => [
                    first.clone(),
                    transfer_intersection_pcurve(
                        ir,
                        &procedural.curve,
                        first_surface,
                        &first,
                        second_surface,
                        range,
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
                        range,
                        tolerance,
                    )?,
                    second,
                ],
                [None, None] => return None,
            };
            Some((
                procedural.id.clone(),
                pcurves,
                tolerance,
                curve_is_cache_backed(ir, &procedural.curve),
                procedural.curve.clone(),
                range,
                tolerant,
            ))
        })
        .collect::<Vec<_>>();
    let mut bounded_tolerant_curves = Vec::new();
    for (procedural_id, pcurves, tolerance, cache_backed, curve, range, tolerant) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        match &mut procedural.definition {
            ProceduralCurveDefinition::Intersection { context, .. }
                if context
                    .sides
                    .iter()
                    .all(|side| pcurve_requires_completion(side.pcurve.as_ref())) =>
            {
                for (side, pcurve) in context.sides.iter_mut().zip(pcurves) {
                    side.pcurve = Some(pcurve);
                }
            }
            ProceduralCurveDefinition::TolerantIntersection {
                parameterization, ..
            } if parameterization.is_none() => {
                *parameterization = Some(TolerantIntersectionParameterization {
                    pcurves,
                    parameter_range: range,
                });
            }
            _ => continue,
        }
        if cache_backed {
            procedural.cache_fit_tolerance = Some(tolerance);
        }
        if tolerant {
            bounded_tolerant_curves.push((curve, range));
        }
    }
    for (curve, range) in bounded_tolerant_curves {
        if let Some(edge) = ir
            .model
            .edges
            .iter_mut()
            .find(|edge| edge.curve.as_ref() == Some(&curve))
        {
            edge.param_range = Some(range);
            annotations.derived(&edge.id, "param_range");
        }
    }
}

fn curve_is_cache_backed(ir: &CadIr, curve: &CurveId) -> bool {
    ir.model
        .curves
        .iter()
        .find(|candidate| &candidate.id == curve)
        .is_some_and(|carrier| !matches!(&carrier.geometry, CurveGeometry::Procedural { .. }))
}

fn exact_boundary_pcurve(
    ir: &CadIr,
    curve: &CurveId,
    surface: &SurfaceId,
    endpoints: [Point3; 2],
    range: [f64; 2],
    tolerance: f64,
) -> Option<PcurveGeometry> {
    (range[0].is_finite()
        && range[1].is_finite()
        && range[0] < range[1]
        && tolerance.is_finite()
        && tolerance >= 0.0)
        .then_some(())?;
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    if let Some(candidate) = exact_analytic_isocurve_pcurve(ir, curve, surface, range, tolerance) {
        return Some(candidate);
    }
    if matches!(&carrier.geometry, SurfaceGeometry::Plane { .. }) {
        let [first, second] =
            endpoints.map(|endpoint| analytic_surface_parameters(&carrier.geometry, endpoint));
        let [first, second] = [first?, second?];
        for (endpoint, parameter) in endpoints.into_iter().zip([first, second]) {
            if !parameter.u.is_finite() || !parameter.v.is_finite() {
                return None;
            }
            let mapped = decoded_surface_point(ir, surface, parameter.u, parameter.v)?;
            let error = point_distance(mapped, endpoint);
            if !error.is_finite() || error > tolerance {
                return None;
            }
        }
        let parameter_span = range[1] - range[0];
        let direction = Point2::new(
            (second.u - first.u) / parameter_span,
            (second.v - first.v) / parameter_span,
        );
        (direction.u.is_finite()
            && direction.v.is_finite()
            && (direction.u != 0.0 || direction.v != 0.0))
            .then_some(())?;
        return Some(PcurveGeometry::Line {
            origin: Point2::new(
                first.u - direction.u * range[0],
                first.v - direction.v * range[0],
            ),
            direction,
        });
    }
    if matches!(
        &carrier.geometry,
        SurfaceGeometry::Cylinder { .. }
            | SurfaceGeometry::Cone { .. }
            | SurfaceGeometry::Sphere { .. }
            | SurfaceGeometry::Torus { .. }
    ) {
        let [first, second] =
            endpoints.map(|endpoint| analytic_surface_parameters(&carrier.geometry, endpoint));
        let [first, second] = [first?, second?];
        if [first.u, first.v, second.u, second.v]
            .into_iter()
            .any(|value| !value.is_finite())
        {
            return None;
        }
        let parameter_span = range[1] - range[0];
        let varying_scale = (second.v - first.v) / parameter_span;
        (varying_scale.is_finite() && varying_scale != 0.0).then_some(())?;
        let candidate = PcurveGeometry::Line {
            origin: Point2::new(first.u, first.v - varying_scale * range[0]),
            direction: Point2::new(0.0, varying_scale),
        };
        for (endpoint, parameter) in endpoints.into_iter().zip(range) {
            let uv = pcurve_uv(&candidate, parameter)?;
            let mapped = decoded_surface_point(ir, surface, uv.u, uv.v)?;
            let error = point_distance(mapped, endpoint);
            if !error.is_finite() || error > tolerance {
                return None;
            }
        }
        return Some(candidate);
    }
    let SurfaceGeometry::Nurbs(nurbs) = &carrier.geometry else {
        return None;
    };
    let domain = surface_parameter_domain(ir, surface)?;
    let parameters = [
        nurbs_parameters_with_tolerance(nurbs, endpoints[0], None, Some(tolerance))?,
        nurbs_parameters_with_tolerance(nurbs, endpoints[1], None, Some(tolerance))?,
    ];
    for index in 0..2 {
        if !parameters[index].u.is_finite() || !parameters[index].v.is_finite() {
            return None;
        }
        let point =
            cadmpeg_ir::eval::nurbs_surface_point(nurbs, parameters[index].u, parameters[index].v)?;
        let error = point_distance(point, endpoints[index]);
        if !error.is_finite() || error > tolerance {
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

fn exact_analytic_isocurve_pcurve(
    ir: &CadIr,
    curve: &CurveId,
    surface: &SurfaceId,
    range: [f64; 2],
    tolerance: f64,
) -> Option<PcurveGeometry> {
    const SAMPLE_INTERVALS: usize = 8;

    let curve = ir
        .model
        .curves
        .iter()
        .find(|candidate| &candidate.id == curve)?;
    let curve_speed = match &curve.geometry {
        CurveGeometry::Circle { radius, .. } => radius.abs(),
        CurveGeometry::Ellipse {
            major_radius,
            minor_radius,
            ..
        } => major_radius.abs().max(minor_radius.abs()),
        _ => return None,
    };
    let surface_carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    matches!(
        surface_carrier.geometry,
        SurfaceGeometry::Cylinder { .. }
            | SurfaceGeometry::Cone { .. }
            | SurfaceGeometry::Sphere { .. }
            | SurfaceGeometry::Torus { .. }
    )
    .then_some(())?;
    let periods = surface_parameter_periods(ir, surface);
    let mut samples = Vec::with_capacity(SAMPLE_INTERVALS + 1);
    for index in 0..=SAMPLE_INTERVALS {
        let parameter = range[0] + (range[1] - range[0]) * index as f64 / SAMPLE_INTERVALS as f64;
        let point = curve_point(&curve.geometry, parameter)?;
        let mut uv = analytic_surface_parameters(&surface_carrier.geometry, point)?;
        if let Some(previous) = samples.last().map(|(_, uv): &(f64, Point2)| *uv) {
            if let Some(period) = periods[0] {
                uv.u = lift_periodic_parameter(uv.u, previous.u, period);
            }
            if let Some(period) = periods[1] {
                uv.v = lift_periodic_parameter(uv.v, previous.v, period);
            }
        }
        samples.push((parameter, uv));
    }
    let parameter_span = range[1] - range[0];
    let first = samples.first()?.1;
    let last = samples.last()?.1;
    let mut direction = Point2::new(
        (last.u - first.u) / parameter_span,
        (last.v - first.v) / parameter_span,
    );
    let angular_tolerance = (tolerance / curve_speed.max(tolerance)).max(1.0e-10);
    let u_constant = samples
        .iter()
        .all(|(_, uv)| (uv.u - first.u).abs() <= angular_tolerance);
    let v_constant = samples
        .iter()
        .all(|(_, uv)| (uv.v - first.v).abs() <= angular_tolerance);
    match (u_constant, v_constant) {
        (true, false) => direction.u = 0.0,
        (false, true) => direction.v = 0.0,
        _ => return None,
    }
    let varying_scale = if direction.u == 0.0 {
        &mut direction.v
    } else {
        &mut direction.u
    };
    (((*varying_scale).abs() - 1.0).abs() <= angular_tolerance).then_some(())?;
    *varying_scale = varying_scale.signum();
    let candidate = PcurveGeometry::Line {
        origin: Point2::new(
            first.u - direction.u * range[0],
            first.v - direction.v * range[0],
        ),
        direction,
    };
    let parameter = range[0];
    let uv = pcurve_uv(&candidate, parameter)?;
    let surface_jet = surface_second_partials(&surface_carrier.geometry, uv.u, uv.v)?;
    let curve_position = curve_point(&curve.geometry, parameter)?;
    let curve_tangent = curve_tangent(&curve.geometry, parameter)?;
    let curve_acceleration = curve_second_derivative(&curve.geometry, parameter)?;
    let surface_tangent = Vector3::new(
        direction.u * surface_jet.du.x + direction.v * surface_jet.dv.x,
        direction.u * surface_jet.du.y + direction.v * surface_jet.dv.y,
        direction.u * surface_jet.du.z + direction.v * surface_jet.dv.z,
    );
    let surface_acceleration = Vector3::new(
        direction.u * direction.u * surface_jet.duu.x
            + 2.0 * direction.u * direction.v * surface_jet.duv.x
            + direction.v * direction.v * surface_jet.dvv.x,
        direction.u * direction.u * surface_jet.duu.y
            + 2.0 * direction.u * direction.v * surface_jet.duv.y
            + direction.v * direction.v * surface_jet.dvv.y,
        direction.u * direction.u * surface_jet.duu.z
            + 2.0 * direction.u * direction.v * surface_jet.duv.z
            + direction.v * direction.v * surface_jet.dvv.z,
    );
    let vector_error = |first: Vector3, second: Vector3| {
        ((first.x - second.x).powi(2) + (first.y - second.y).powi(2) + (first.z - second.z).powi(2))
            .sqrt()
    };
    (point_distance(curve_position, surface_jet.point) <= tolerance
        && vector_error(curve_tangent, surface_tangent) <= tolerance
        && vector_error(curve_acceleration, surface_acceleration) <= tolerance)
        .then_some(())?;
    Some(candidate)
}

fn coincident_pcurve_pair(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    pcurves: [&PcurveGeometry; 2],
    range: [f64; 2],
    tolerance: f64,
) -> bool {
    const MAX_INTERVALS: usize = 100_000;

    if !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] >= range[1]
        || !tolerance.is_finite()
        || tolerance < 0.0
    {
        return false;
    }
    let separation = |parameter| {
        let points = [0usize, 1usize].map(|side| {
            let uv = pcurve_uv(pcurves[side], parameter)?;
            decoded_surface_point(ir, surfaces[side], uv.u, uv.v)
        });
        let [Some(first), Some(second)] = points else {
            return None;
        };
        let distance = point_distance(first, second);
        distance.is_finite().then_some(distance)
    };
    let affine_breaks = [0usize, 1usize]
        .map(|side| boundary_curve_affine_breaks(ir, surfaces[side], pcurves[side], range));
    if let [Some(first), Some(second)] = affine_breaks {
        let mut breaks = first;
        breaks.extend(second);
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        return breaks
            .into_iter()
            .all(|parameter| separation(parameter).is_some_and(|value| value <= tolerance));
    }
    let Some(speed_bound) = [0usize, 1usize]
        .into_iter()
        .map(|side| boundary_curve_speed_bound(ir, surfaces[side], pcurves[side]))
        .sum::<Option<f64>>()
    else {
        return false;
    };
    if range
        .into_iter()
        .any(|parameter| !separation(parameter).is_some_and(|value| value <= tolerance))
    {
        return false;
    }
    let mut intervals = vec![range];
    let mut examined = 0usize;
    while let Some([start, end]) = intervals.pop() {
        examined += 1;
        if examined > MAX_INTERVALS {
            return false;
        }
        let middle = start + (end - start) * 0.5;
        let Some(middle_separation) = separation(middle) else {
            return false;
        };
        if middle_separation > tolerance {
            return false;
        }
        let maximum_separation = middle_separation + speed_bound * (end - start) * 0.5;
        if maximum_separation <= tolerance {
            continue;
        }
        if middle == start || middle == end {
            return false;
        }
        intervals.push([middle, end]);
        intervals.push([start, middle]);
    }
    true
}

fn boundary_curve_affine_breaks(
    ir: &CadIr,
    surface: &SurfaceId,
    pcurve: &PcurveGeometry,
    range: [f64; 2],
) -> Option<Vec<f64>> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let PcurveGeometry::Line { origin, direction } = pcurve else {
        return None;
    };
    match &carrier.geometry {
        SurfaceGeometry::Plane { .. } => Some(range.to_vec()),
        SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. }
            if direction.u == 0.0 && direction.v != 0.0 =>
        {
            Some(range.to_vec())
        }
        SurfaceGeometry::Nurbs(nurbs) => {
            let (fixed_axis, fixed_parameter, varying_origin, varying_scale) =
                if direction.u == 0.0 && direction.v != 0.0 {
                    (SurfaceParameterAxis::U, origin.u, origin.v, direction.v)
                } else if direction.v == 0.0 && direction.u != 0.0 {
                    (SurfaceParameterAxis::V, origin.v, origin.u, direction.u)
                } else {
                    return None;
                };
            let isocurve = nurbs_surface_isocurve(nurbs, fixed_axis, fixed_parameter)?;
            if isocurve.degree != 1
                || isocurve.weights.as_ref().is_some_and(|weights| {
                    weights
                        .windows(2)
                        .any(|pair| pair[0].to_bits() != pair[1].to_bits())
                })
            {
                return None;
            }
            let degree = usize::try_from(isocurve.degree).ok()?;
            let count = isocurve.control_points.len();
            let mut breaks = isocurve.knots.get(degree..=count)?.to_vec();
            for parameter in &mut breaks {
                *parameter = (*parameter - varying_origin) / varying_scale;
            }
            breaks.retain(|parameter| {
                parameter.is_finite() && *parameter >= range[0] && *parameter <= range[1]
            });
            breaks.extend(range);
            Some(breaks)
        }
        _ => None,
    }
}

fn boundary_curve_speed_bound(
    ir: &CadIr,
    surface: &SurfaceId,
    pcurve: &PcurveGeometry,
) -> Option<f64> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let PcurveGeometry::Line { origin, direction } = pcurve else {
        return None;
    };
    let affine_speed = || {
        let first = decoded_surface_point(ir, surface, origin.u, origin.v)?;
        let second =
            decoded_surface_point(ir, surface, origin.u + direction.u, origin.v + direction.v)?;
        let speed = point_distance(first, second);
        speed.is_finite().then_some(speed)
    };
    match &carrier.geometry {
        SurfaceGeometry::Plane { .. } => affine_speed(),
        SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. }
            if direction.u == 0.0 && direction.v != 0.0 =>
        {
            affine_speed()
        }
        SurfaceGeometry::Cylinder { radius, .. } if direction.v == 0.0 && direction.u != 0.0 => {
            let speed = radius.abs() * direction.u.abs();
            speed.is_finite().then_some(speed)
        }
        SurfaceGeometry::Cone {
            radius,
            ratio,
            half_angle,
            ..
        } if direction.v == 0.0 && direction.u != 0.0 => {
            let local_radius = radius + origin.v * half_angle.tan();
            let speed = local_radius.abs() * ratio.abs().max(1.0) * direction.u.abs();
            speed.is_finite().then_some(speed)
        }
        SurfaceGeometry::Sphere { radius, .. } if direction.v == 0.0 && direction.u != 0.0 => {
            let speed = radius.abs() * origin.v.cos().abs() * direction.u.abs();
            speed.is_finite().then_some(speed)
        }
        SurfaceGeometry::Sphere { radius, .. } if direction.u == 0.0 && direction.v != 0.0 => {
            let speed = radius.abs() * direction.v.abs();
            speed.is_finite().then_some(speed)
        }
        SurfaceGeometry::Torus {
            major_radius,
            minor_radius,
            ..
        } if direction.v == 0.0 && direction.u != 0.0 => {
            let ring_radius = major_radius + minor_radius * origin.v.cos();
            let speed = ring_radius.abs() * direction.u.abs();
            speed.is_finite().then_some(speed)
        }
        SurfaceGeometry::Torus { minor_radius, .. } if direction.u == 0.0 && direction.v != 0.0 => {
            let speed = minor_radius.abs() * direction.v.abs();
            speed.is_finite().then_some(speed)
        }
        SurfaceGeometry::Nurbs(nurbs) => {
            let (fixed_axis, fixed_parameter, varying_scale) =
                if direction.u == 0.0 && direction.v != 0.0 {
                    (SurfaceParameterAxis::U, origin.u, direction.v)
                } else if direction.v == 0.0 && direction.u != 0.0 {
                    (SurfaceParameterAxis::V, origin.v, direction.u)
                } else {
                    return None;
                };
            let isocurve = nurbs_surface_isocurve(nurbs, fixed_axis, fixed_parameter)?;
            let bound = nurbs_curve_speed_bound(&isocurve)? * varying_scale.abs();
            bound.is_finite().then_some(bound)
        }
        _ => None,
    }
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
    const CONTINUATION_STEPS: usize = 16;

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
    let mut coarse = Vec::with_capacity(CONTINUATION_STEPS + 1);
    coarse.push(first);
    for index in 1..=CONTINUATION_STEPS {
        let parameter = parameter_range[0]
            + (parameter_range[1] - parameter_range[0]) * index as f64 / CONTINUATION_STEPS as f64;
        let sample = transferred_pcurve_sample(
            ir,
            curve,
            source_surface,
            source_pcurve,
            target_surface,
            parameter,
            coarse.last().map(|sample| sample.1),
            tolerance,
        )?;
        coarse.push(sample);
    }
    let mut samples = vec![first];
    for pair in coarse.windows(2) {
        append_transferred_pcurve_segment(
            ir,
            curve,
            source_surface,
            source_pcurve,
            target_surface,
            pair[0],
            pair[1],
            tolerance,
            0,
            &mut samples,
        )?;
    }
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
        BoundaryInverseTarget {
            point,
            seed,
            tolerance,
        },
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
        SurfaceGeometry::Nurbs(nurbs) => {
            nurbs_parameters_with_tolerance(nurbs, point, seed, Some(tolerance))
        }
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
        let Some(edge_fields) = graph.get(16, xmt).and_then(Node::edge_fields) else {
            continue;
        };
        let Some(first_fin) = graph.get(17, edge_fields.fin).and_then(Node::fin_fields) else {
            continue;
        };
        if edge_fields.curve != 1 || first_fin.curve_xmt != 1 || first_fin.other <= 1 {
            continue;
        }
        let Some(second_fin) = graph.get(17, first_fin.other).and_then(Node::fin_fields) else {
            continue;
        };
        if second_fin.other != edge_fields.fin || second_fin.edge != xmt {
            continue;
        }
        let Some(edge) = ir
            .model
            .edges
            .iter()
            .find(|candidate| &candidate.id == edge_id)
        else {
            continue;
        };
        let Some(tolerance) = edge.tolerance else {
            continue;
        };
        if edge.curve.is_some() {
            continue;
        }
        let support = |fin_xmt| {
            let coedge_id = CoedgeId(format!("{prefix}:fin#{fin_xmt}"));
            ir.model
                .coedges
                .iter()
                .find(|coedge| coedge.id == coedge_id && &coedge.edge == edge_id)
                .and_then(|coedge| {
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
        };
        let Some(first_support) = support(edge_fields.fin) else {
            continue;
        };
        let Some(second_support) = support(first_fin.other) else {
            continue;
        };
        if first_support == second_support {
            continue;
        }
        let endpoint = |vertex_id: &VertexId| {
            let point_id = &ir
                .model
                .vertices
                .iter()
                .find(|vertex| &vertex.id == vertex_id)?
                .point;
            ir.model
                .points
                .iter()
                .find(|point| &point.id == point_id)
                .map(|point| point.position)
        };
        let (Some(start), Some(end)) = (endpoint(&edge.start), endpoint(&edge.end)) else {
            continue;
        };
        let endpoints = [start, end];
        let supports = [first_support, second_support];
        let endpoints_bound_supports = supports.iter().all(|surface| {
            endpoints.iter().all(|point| {
                surface_parameters_for_fit(ir, surface, *point, None, tolerance)
                    .and_then(|uv| decoded_surface_point(ir, surface, uv.u, uv.v))
                    .is_some_and(|support_point| point_distance(*point, support_point) <= tolerance)
            })
        });
        if !endpoints_bound_supports {
            continue;
        }
        candidates.push((xmt, edge_id.clone(), supports, endpoints, tolerance));
    }

    for (xmt, edge_id, supports, endpoints, tolerance) in candidates {
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
        annotations.derived(&edge_id, "curve");
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
            definition: ProceduralCurveDefinition::TolerantIntersection {
                supports,
                endpoints,
                tolerance,
                parameterization: None,
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
    let Some([t0, t1]) = parameter_range.or_else(|| pcurve_parameter_range(geometry)) else {
        return false;
    };
    let (Some(first_uv), Some(second_uv)) = (pcurve_uv(geometry, t0), pcurve_uv(geometry, t1))
    else {
        return false;
    };
    let (Some(first), Some(second)) = (
        decoded_surface_point(ir, surface_id, first_uv.u, first_uv.v),
        decoded_surface_point(ir, surface_id, second_uv.u, second_uv.v),
    ) else {
        return false;
    };
    let coincident_surface = [first, second];
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
    .fold(0.0_f64, f64::max);
    (point_distance(coincident_surface[0], start) <= allowance
        && point_distance(coincident_surface[1], end) <= allowance)
        || (point_distance(coincident_surface[0], end) <= allowance
            && point_distance(coincident_surface[1], start) <= allowance)
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
    let allowance = [edge_tolerance, start_tolerance, end_tolerance]
        .into_iter()
        .flatten()
        .fold(0.0_f64, f64::max);
    if point_distance(at[0], start_position) <= allowance
        && point_distance(at[1], end_position) <= allowance
    {
        Some((range, false))
    } else if point_distance(at[1], start_position) <= allowance
        && point_distance(at[0], end_position) <= allowance
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
        "header_entry_count".to_string(),
        scan.container.header_entry_count.to_string(),
    );
    attributes.insert(
        "footer_entry_count".to_string(),
        scan.container.footer_entry_count.to_string(),
    );
    attributes.insert(
        "footer_fingerprint".to_string(),
        format!(
            "{:08x}",
            u32::from_be_bytes(scan.container.footer_fingerprint)
        ),
    );
    let (control_count, classified_control_count) = offset_store_control_counts(&scan.container);
    if control_count != 0 {
        attributes.insert(
            "offset_store_control_count".to_string(),
            control_count.to_string(),
        );
        attributes.insert(
            "classified_offset_store_control_count".to_string(),
            classified_control_count.to_string(),
        );
        attributes.insert(
            "unclassified_offset_store_control_count".to_string(),
            (control_count - classified_control_count).to_string(),
        );
    }
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
        if census.transmit_header.is_some() {
            attributes.insert(format!("deltas.{index}.transmit_headers"), "1".to_string());
        }
        attributes.insert(
            format!("deltas.{index}.grammar"),
            "typed_status_framed_records".to_string(),
        );
        attributes.insert(
            format!("deltas.{index}.bytes_decoded"),
            census.bytes_decoded.to_string(),
        );
        if !census.body_revisions.is_empty() {
            attributes.insert(
                format!("deltas.{index}.body_revisions"),
                census.body_revisions.len().to_string(),
            );
        }
        if !census.term_use_numeric_tails.is_empty() {
            attributes.insert(
                format!("deltas.{index}.term_use_numeric_tails"),
                census.term_use_numeric_tails.len().to_string(),
            );
        }
        if !census.tagged_reference_lanes.is_empty() {
            attributes.insert(
                format!("deltas.{index}.tagged_reference_lanes"),
                census.tagged_reference_lanes.len().to_string(),
            );
        }
        if !census.reference_type_maps.is_empty() {
            attributes.insert(
                format!("deltas.{index}.reference_type_maps"),
                census.reference_type_maps.len().to_string(),
            );
        }
        if !census.reference_state_packets.is_empty() {
            attributes.insert(
                format!("deltas.{index}.reference_state_packets"),
                census.reference_state_packets.len().to_string(),
            );
        }
        if !census.reference_marker_packets.is_empty() {
            attributes.insert(
                format!("deltas.{index}.reference_marker_packets"),
                census.reference_marker_packets.len().to_string(),
            );
        }
        if !census.inline_schema_declarations.is_empty() {
            attributes.insert(
                format!("deltas.{index}.inline_schema_declarations"),
                census.inline_schema_declarations.len().to_string(),
            );
        }
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

    losses.push(LossNote {
        code: LossCode::CarrierSummary,
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

    if tessellation_count != 0 {
        losses.push(LossNote {
            code: LossCode::CarrierSummary,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Decoded {tessellation_count} embedded JT display tessellation(s) with scene-node ownership, model-space coordinates, topological triangle connectivity, and corner normals when bound."
            ),
            provenance: None,
        });
    }

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

    if counts.intersection_rejections.total() > 0 {
        losses.push(LossNote {
            code: LossCode::ObjectRecordsUntransferred,
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
        let unmatched_tombstone_counts = unmatched_delta_tombstone_counts(scan);
        let unmatched_tombstones = unmatched_tombstone_counts.values().sum::<usize>();
        let unmatched_tombstone_detail = unmatched_tombstone_counts
            .iter()
            .map(|(family, count)| format!("{family} {count}"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(LossNote {
            code: LossCode::DecodeDiagnostic,
            category: LossCategory::Topology,
            severity: if unmatched_tombstones == 0 {
                Severity::Info
            } else {
                Severity::Warning
            },
            message: if unmatched_tombstones == 0 {
                format!(
                    "{} Parasolid deltas stream(s) were processed in validated UG_PART segment order. \
                 Equal-schema deltas were paired with the preceding partition. Exact-key \
                 BODY, SHELL, FACE, LOOP, FIN, EDGE, VERTEX, REGION, POINT, LINE, CIRCLE, ELLIPSE, PLANE, CYLINDER, CONE, SPHERE, TORUS, BLEND_SURF, OFFSET_SURF, B_SURFACE, TRIMMED_CURVE, B_CURVE, and SP_CURVE full records and compact \
                 non-topology replacements and tombstones were applied using the last event for \
                 each key. Validated partition topology remained authoritative, including any \
                 point, curve, or surface carrier still referenced by surviving topology. Complete \
                 ENTITY_51, ENTITY_52, ENTITY_53, and ENTITY_54 records were retained for native \
                 attribute extraction. Every completely bounded full record, compact tombstone, \
                 and BODY revision envelope was retained as an individually identified native event \
                 with its source bounds and decoded identities; BODY state tails retain exact \
                 bounded bytes and digests. Complete transmit headers retain their description, \
                 schema, consecutive identities, and exact bytes. Terminal two- and \
                 four-null-reference trailers retain their exact stream boundary and bytes. \
                 Count-selected numeric tails after \
                 term-use endpoints were retained with their ordered finite binary64 values. Maximal \
                 event gaps containing only typed stream-local references, framed reference/type \
                 maps, and complete four-reference state packets, reference-marker packets, and inline schema \
                 declarations were retained in order. \
                 Spans outside those events were retained with exact inflated-stream bounds and \
                 digests. Semantic intersection and NURBS records were retained in the semantic \
                 lane. Every \
                 terminal tombstone resolved to an exact current or earlier-added key.",
                    scan.count(StreamKind::Deltas)
                )
            } else {
                format!(
                    "{} Parasolid deltas stream(s) were processed in validated UG_PART segment order. \
                 Equal-schema deltas were paired with the preceding partition. Exact-key revisions were applied using the last \
                 event for each key, but {unmatched_tombstones} terminal tombstone(s) have no exact \
                 current or earlier-added key and remain unresolved: {unmatched_tombstone_detail}.",
                    scan.count(StreamKind::Deltas)
                )
            },
            provenance: None,
        });
    }

    if has_unresolved_sub_bodies {
        losses.push(LossNote {
            code: LossCode::FeatureHistoryRetained,
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

    append_design_intent_losses(ir, &mut losses);

    losses.push(LossNote {
        code: LossCode::AttributesNotTransferred,
        category: LossCategory::Attribute,
        severity: Severity::Warning,
        message: "Material and appearance assignment, class-specific entity attribute fields, and \
                  assembly occurrence placements were not transferred: their remaining NX \
                  object-model and Parasolid field serialization is not decoded."
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

pub(crate) fn append_design_intent_losses(ir: &CadIr, losses: &mut Vec<LossNote>) {
    let unresolved_suppression_count = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.suppressed.is_none())
        .count();
    if unresolved_suppression_count != 0 {
        losses.push(LossNote {
            code: LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Warning,
            message: format!(
                "Suppression state remains unresolved for {unresolved_suppression_count} NX \
                 feature history operation(s)."
            ),
            provenance: None,
        });
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
                || (configuration.active
                    && active_configuration_state_is_incomplete(ir, configuration))
        })
        .count();
    if incomplete_configuration_count != 0 {
        losses.push(LossNote {
            code: LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Warning,
            message: format!(
                "Activation, complete body membership, evaluated feature state, or evaluated \
                 parameter state remains unresolved for {incomplete_configuration_count} NX \
                 design configuration(s)."
            ),
            provenance: None,
        });
    }

    let incomplete_expression_count = incomplete_expression_parameters(ir).len();
    if incomplete_expression_count != 0 {
        losses.push(LossNote {
            code: LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Warning,
            message: format!(
                "Neutral evaluation or dependency semantics remain incomplete for \
                 {incomplete_expression_count} NX expression parameter(s)."
            ),
            provenance: None,
        });
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
        losses.push(LossNote {
            code: LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Warning,
            message: format!(
                "NX feature-history operation(s) remain native-only because their complete neutral \
                 operation semantics are not decoded: {kinds}."
            ),
            provenance: None,
        });
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
        losses.push(LossNote {
            code: LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Warning,
            message: format!(
                "NX feature family identities were transferred, but their neutral construction \
                 semantics remain unresolved: {families}."
            ),
            provenance: None,
        });
    }

    let mut incomplete_feature_families = BTreeMap::<&str, usize>::new();
    for feature in &ir.model.features {
        if feature.suppressed != Some(true) {
            if let Some(family) = feature.definition.body_output_family().filter(|_| {
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
            FeatureDefinition::BaseFeature { bodies } if body_selection_is_incomplete(bodies) => {
                "base feature"
            }
            FeatureDefinition::Block {
                dimensions,
                placement,
            } if dimensions.is_none_or(|dimensions| {
                dimensions
                    .into_iter()
                    .any(|dimension| !positive_feature_length(dimension))
            }) || placement.is_none_or(|placement| !placement.is_proper_rigid()) =>
            {
                "block"
            }
            FeatureDefinition::DatumOffsetPlane {
                reference,
                distance,
            } if !distance.0.is_finite()
                || reference.as_ref().is_none_or(|reference| match reference {
                    DatumPlaneReference::Feature(reference) => {
                        ir.model
                            .features
                            .iter()
                            .find(|candidate| candidate.id == *reference)
                            .is_none_or(|source| source.ordinal >= feature.ordinal)
                            || !feature.dependencies.contains(reference)
                    }
                    DatumPlaneReference::Face { face, .. } => face_selection_is_incomplete(face),
                }) =>
            {
                "datum plane"
            }
            FeatureDefinition::DatumPlane {
                origin,
                normal,
                u_axis,
            } if datum_plane_is_incomplete(*origin, *normal, *u_axis) => "datum plane",
            FeatureDefinition::DatumAxis { origin, direction }
                if !finite_feature_point(*origin) || !valid_feature_direction(*direction) =>
            {
                "datum axis"
            }
            FeatureDefinition::DatumPoint { position } if !finite_feature_point(*position) => {
                "datum point"
            }
            FeatureDefinition::DatumCoordinateSystem {
                origin,
                x_axis,
                y_axis,
                z_axis,
            } if datum_coordinate_system_is_incomplete(*origin, *x_axis, *y_axis, *z_axis) => {
                "datum coordinate system"
            }
            FeatureDefinition::ExtractBody { source } if body_selection_is_incomplete(source) => {
                "extract body"
            }
            FeatureDefinition::Sketch { space, sketch }
                if !matches!(space, SketchSpace::Planar)
                    || sketch.as_ref().is_none_or(|sketch| {
                        ir.model
                            .sketches
                            .iter()
                            .find(|candidate| candidate.id == *sketch)
                            .is_none_or(|sketch| {
                                matches!(
                                    sketch.placement,
                                    cadmpeg_ir::sketches::SketchPlacement::Unresolved
                                )
                            })
                    }) =>
            {
                "sketch"
            }
            FeatureDefinition::Loft {
                sections,
                centerline,
                guides,
                op,
                max_degree,
                ..
            } if sections.len() < 2
                || sections.iter().any(loft_section_is_incomplete)
                || sections.iter().any(|section| {
                    matches!(
                        section,
                        LoftSection::Profile(profile)
                            if profile_dependency_is_incomplete(profile, &feature.dependencies)
                    )
                })
                || centerline.as_ref().is_some_and(path_ref_is_incomplete)
                || guides.iter().any(path_ref_is_incomplete)
                || (centerline.is_some() && !guides.is_empty())
                || max_degree.is_some_and(|degree| degree == 0)
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
                || projected_curve_direction_is_incomplete(*direction)
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
                || distance.is_none_or(|distance| !positive_feature_length(distance))
                || matches!(method, cadmpeg_ir::features::SurfaceExtension::Unresolved) =>
            {
                "extend surface"
            }
            FeatureDefinition::Hole {
                profile,
                profile_filter,
                face,
                position,
                direction,
                placements,
                kind,
                exit_kind,
                diameter,
                extent,
                bottom,
                taper_angle,
                specification,
                ..
            } if hole_feature_is_incomplete(
                profile.as_ref(),
                face.as_ref(),
                (*position, *direction),
                placements,
                (kind, exit_kind.as_ref()),
                *diameter,
                extent.as_ref(),
            ) || hole_auxiliary_semantics_are_incomplete(
                profile_filter.as_ref(),
                bottom.as_ref(),
                *taper_angle,
                specification.as_deref(),
            ) || extent.as_ref().is_some_and(|extent| {
                termination_dependency_is_incomplete(extent, &feature.dependencies)
            }) || profile.as_ref().is_some_and(|profile| {
                profile_dependency_is_incomplete(profile, &feature.dependencies)
            }) =>
            {
                "hole"
            }
            FeatureDefinition::Rib { construction, op }
                if rib_feature_is_incomplete(construction, *op)
                    || construction.profile.as_ref().is_some_and(|profile| {
                        profile_dependency_is_incomplete(profile, &feature.dependencies)
                    }) =>
            {
                "rib"
            }
            FeatureDefinition::Chamfer { groups, .. }
                if groups.is_empty()
                    || groups.iter().any(|group| {
                        edge_selection_is_incomplete(&group.edges)
                            || chamfer_spec_is_incomplete(&group.spec)
                    }) =>
            {
                "chamfer"
            }
            FeatureDefinition::Fillet { groups }
                if groups.is_empty()
                    || groups.iter().any(|group| {
                        edge_selection_is_incomplete(&group.edges)
                            || radius_spec_is_incomplete(&group.radius)
                            || group
                                .tangency_weight
                                .is_some_and(|weight| !weight.is_finite())
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
            FeatureDefinition::SewBodies {
                bodies,
                gap_tolerance,
            } if body_selection_is_incomplete(bodies)
                || resolved_body_selection_len(bodies).is_some_and(|count| count < 2)
                || gap_tolerance.is_some_and(|tolerance| !positive_feature_length(tolerance)) =>
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
                direction,
                start,
                extent,
                op,
                solid,
                direction_source,
                face_maker,
                ..
            } if profile_ref_is_incomplete(profile)
                || profile_dependency_is_incomplete(profile, &feature.dependencies)
                || matches!(
                    direction,
                    cadmpeg_ir::features::ExtrudeDirection::Unresolved
                )
                || matches!(
                    direction,
                    cadmpeg_ir::features::ExtrudeDirection::Explicit(direction)
                        if !valid_feature_direction(*direction)
                )
                || extrude_start_is_incomplete(start)
                || extrude_extent_is_incomplete(extent, &feature.dependencies)
                || matches!(op, BooleanOp::Unresolved)
                || solid.is_none()
                || direction_source.as_ref().is_some_and(|source| {
                    matches!(
                        source,
                        cadmpeg_ir::features::ExtrusionDirectionSource::Edge { reference }
                            if path_ref_is_incomplete(reference)
                    )
                })
                || face_maker
                    .as_ref()
                    .is_some_and(|maker| maker.class.trim().is_empty()) =>
            {
                "extrude"
            }
            FeatureDefinition::Revolve { construction, op }
                if revolve_feature_is_incomplete(construction, *op, &feature.dependencies) =>
            {
                "revolve"
            }
            FeatureDefinition::Sweep {
                profile,
                sections,
                path,
                mode,
                orientation,
                transition,
                transformation,
                twist,
                scale,
                ..
            } if profile.as_ref().is_none_or(profile_ref_is_incomplete)
                || profile.as_ref().is_some_and(|profile| {
                    profile_dependency_is_incomplete(profile, &feature.dependencies)
                })
                || sections.iter().any(profile_ref_is_incomplete)
                || sections.iter().any(|profile| {
                    profile_dependency_is_incomplete(profile, &feature.dependencies)
                })
                || path.as_ref().is_none_or(path_ref_is_incomplete)
                || sweep_mode_is_incomplete(*mode)
                || orientation
                    .as_ref()
                    .is_none_or(sweep_orientation_is_incomplete)
                || transition.is_none()
                || transformation.is_none()
                || twist.is_some_and(|twist| !twist.0.is_finite())
                || scale.is_some_and(|scale| !scale.is_finite() || scale <= 0.0) =>
            {
                "sweep"
            }
            FeatureDefinition::OffsetSurface { faces, distance }
                if face_selection_is_incomplete(faces)
                    || distance.is_none_or(|distance| !distance.0.is_finite()) =>
            {
                "offset surface"
            }
            FeatureDefinition::Thicken {
                faces,
                thickness,
                side,
            } if face_selection_is_incomplete(faces)
                || thickness.is_none_or(|thickness| !positive_feature_length(thickness))
                || side.is_none() =>
            {
                "thicken"
            }
            FeatureDefinition::Draft {
                faces,
                neutral_plane,
                pull_direction,
                angle,
                outward,
            } if face_selection_is_incomplete(faces)
                || face_selection_is_incomplete(neutral_plane)
                || pull_direction.is_none_or(|direction| !valid_feature_direction(direction))
                || angle.is_none_or(|angle| !valid_draft_angle(angle))
                || outward.is_none() =>
            {
                "draft"
            }
            FeatureDefinition::Pattern { seeds, pattern }
                if pattern_feature_is_incomplete(seeds, pattern, &feature.dependencies) =>
            {
                "pattern"
            }
            FeatureDefinition::SectionShape {
                first,
                second,
                approximate,
            } if body_selection_is_incomplete(first)
                || body_selection_is_incomplete(second)
                || body_selections_overlap(first, second)
                || approximate.is_none() =>
            {
                "section"
            }
            FeatureDefinition::Combine {
                target, tools, op, ..
            } if body_selection_is_incomplete(target)
                || body_selection_is_incomplete(tools)
                || resolved_body_selection_len(target) != Some(1)
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
            FeatureDefinition::ReplaceFace {
                targets,
                replacements,
            } if face_selection_is_incomplete(targets)
                || face_selection_is_incomplete(replacements)
                || face_selections_overlap(targets, replacements) =>
            {
                "replace face"
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
        losses.push(LossNote {
            code: LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Warning,
            message: format!(
                "NX feature families were transferred as typed neutral operations, but \
                 construction fields or output lineage remain unresolved or native-only: \
                 {families}."
            ),
            provenance: None,
        });
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
        losses.push(LossNote {
            code: LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Warning,
            message: format!(
                "Decoded {sketch_feature_count} NX sketch history feature(s), of which \
                 {unresolved_sketch_feature_count} have no neutral sketch graph because complete \
                 sketch placement and entity semantics are unresolved."
            ),
            provenance: None,
        });
    } else if sketch_feature_count != 0 && ir.model.sketch_constraints.is_empty() {
        losses.push(LossNote {
            code: LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Warning,
            message: format!(
                "Decoded {} NX sketch record(s), but no sketch constraints were transferred because \
                 their object-model field serialization and operand roles are unresolved.",
                ir.model.sketches.len()
            ),
            provenance: None,
        });
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
        losses.push(LossNote {
            code: LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Warning,
            message: format!(
                "Neutral semantics remain unresolved for {native_sketch_entity_count} NX sketch \
                 geometry record(s) and {native_sketch_constraint_count} sketch constraint \
                 record(s)."
            ),
            provenance: None,
        });
    }
}

fn active_configuration_state_is_incomplete(
    ir: &CadIr,
    configuration: &cadmpeg_ir::features::DesignConfiguration,
) -> bool {
    let suppressed_features = configuration
        .suppressed_features
        .iter()
        .collect::<BTreeSet<_>>();
    if suppressed_features.len() != configuration.suppressed_features.len()
        || ir.model.features.iter().any(|feature| {
            feature
                .suppressed
                .is_none_or(|suppressed| suppressed_features.contains(&feature.id) != suppressed)
        })
    {
        return true;
    }
    let Some(bodies) = configuration.bodies.resolved() else {
        return true;
    };
    let active_features = if ir.model.features.is_empty() {
        BTreeSet::new()
    } else {
        let Some(active_features) = crate::native::history::active_feature_closure(ir, bodies)
        else {
            return true;
        };
        active_features
    };
    if configuration.feature_states.len() != active_features.len() {
        return true;
    }
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<BTreeMap<_, _>>();
    if active_features.iter().any(|id| {
        let (Some(feature), Some(state)) = (features.get(id), configuration.feature_states.get(id))
        else {
            return true;
        };
        state.suppressed
            || state.dependencies != feature.dependencies
            || state.outputs != feature.outputs
            || state.definition != feature.definition
    }) {
        return true;
    }

    configuration.parameter_values.len() != ir.model.parameters.len()
        || ir.model.parameters.iter().any(|parameter| {
            parameter.value.as_ref().is_none_or(|value| {
                configuration.parameter_values.get(&parameter.id) != Some(value)
            })
        })
}

pub(crate) fn datum_plane_is_incomplete(origin: Point3, normal: Vector3, u_axis: Vector3) -> bool {
    !finite_feature_point(origin)
        || !valid_feature_direction(normal)
        || !valid_feature_direction(u_axis)
        || !directions_are_perpendicular(normal, u_axis)
}

pub(crate) fn datum_coordinate_system_is_incomplete(
    origin: Point3,
    x_axis: Vector3,
    y_axis: Vector3,
    z_axis: Vector3,
) -> bool {
    if !finite_feature_point(origin)
        || !unit_feature_direction(x_axis)
        || !unit_feature_direction(y_axis)
        || !unit_feature_direction(z_axis)
        || !directions_are_perpendicular(x_axis, y_axis)
        || !directions_are_perpendicular(y_axis, z_axis)
        || !directions_are_perpendicular(z_axis, x_axis)
    {
        return true;
    }
    let handedness = x_axis.cross(y_axis).dot(z_axis);
    !handedness.is_finite() || (handedness - 1.0).abs() > 1e-9
}

pub(crate) fn projected_curve_direction_is_incomplete(direction: CurveProjectionDirection) -> bool {
    match direction {
        CurveProjectionDirection::Vector(direction) => !valid_feature_direction(direction),
        CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved) => true,
        CurveProjectionDirection::State(CurveProjectionDirectionState::TargetNormal) => false,
    }
}

fn unit_feature_direction(direction: Vector3) -> bool {
    valid_feature_direction(direction) && (direction.norm() - 1.0).abs() <= 1e-9
}

fn directions_are_perpendicular(first: Vector3, second: Vector3) -> bool {
    let scale = first.norm() * second.norm();
    scale.is_finite() && first.dot(second).abs() <= 1e-9 * scale
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
        let mut ids_by_name = BTreeMap::<(&str, Option<&str>), Vec<&ParameterId>>::new();
        for parameter in &parameters {
            ids_by_name
                .entry((
                    parameter.name.as_str(),
                    parameter.properties.get("unit").map(String::as_str),
                ))
                .or_default()
                .push(&parameter.id);
        }
        let expected = parameters
            .iter()
            .map(|parameter| {
                let unit = match parameter.properties.get("unit").map(String::as_str) {
                    None => None,
                    Some(unit @ ("millimeter" | "degree")) => Some(unit),
                    Some(_) => return None,
                };
                let [_] = ids_by_name
                    .get(&(parameter.name.as_str(), unit))?
                    .as_slice()
                else {
                    return None;
                };
                let mut seen = BTreeSet::new();
                let dependencies = crate::native::expression_parameter_names(&parameter.expression)
                    .into_iter()
                    .map(|name| {
                        let [dependency] = ids_by_name.get(&(name, unit))?.as_slice() else {
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
        let mut evaluated = BTreeMap::<ParameterId, f64>::new();
        while let Some(index) = (0..parameters.len()).find(|index| {
            !emitted.contains(index)
                && expected[*index].as_ref().is_some_and(|dependencies| {
                    dependencies.iter().all(|dependency| {
                        evaluated.contains_key(dependency)
                            && indices
                                .get(dependency)
                                .is_some_and(|index| emitted.contains(index))
                    })
                })
        }) {
            let parameter = parameters[index];
            let unit = parameter.properties.get("unit").map(String::as_str);
            let value =
                crate::native::evaluate_parameterized_expression(&parameter.expression, |name| {
                    let [dependency] = ids_by_name.get(&(name, unit))?.as_slice() else {
                        return None;
                    };
                    evaluated.get(*dependency).copied()
                });
            let stored = match (unit, parameter.value.as_ref()) {
                (Some("millimeter"), Some(cadmpeg_ir::features::ParameterValue::Length(value))) => {
                    Some(value.0)
                }
                (Some("degree"), Some(cadmpeg_ir::features::ParameterValue::Angle(value))) => {
                    Some(value.0.to_degrees())
                }
                (None, Some(cadmpeg_ir::features::ParameterValue::Real(value))) => Some(*value),
                (None, Some(cadmpeg_ir::features::ParameterValue::Integer(value))) => {
                    Some(*value as f64)
                }
                _ => None,
            };
            if let (Some(value), Some(stored)) = (value, stored) {
                let tolerance = 64.0 * f64::EPSILON * value.abs().max(stored.abs()).max(1.0);
                if value.is_finite() && stored.is_finite() && (value - stored).abs() <= tolerance {
                    evaluated.insert(parameter.id.clone(), value);
                }
            }
            emitted.insert(index);
        }
        for (index, parameter) in parameters.into_iter().enumerate() {
            if expected[index].as_ref() != Some(&parameter.dependencies)
                || !emitted.contains(&index)
                || !evaluated.contains_key(&parameter.id)
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
    authored_axis: (Option<Point3>, Option<Vector3>),
    placements: &[cadmpeg_ir::features::HolePlacement],
    treatments: (&HoleKind, Option<&HoleKind>),
    diameter: Option<Length>,
    extent: Option<&Termination>,
) -> bool {
    let (position, direction) = authored_axis;
    let (kind, exit_kind) = treatments;
    let profile_incomplete = profile.is_some_and(profile_ref_is_incomplete);
    let face_incomplete = face.is_some_and(face_selection_is_incomplete);
    let finite_point =
        |point: Point3| point.x.is_finite() && point.y.is_finite() && point.z.is_finite();
    let finite_direction = |vector: Vector3| {
        vector.x.is_finite()
            && vector.y.is_finite()
            && vector.z.is_finite()
            && vector.norm() > 1e-12
    };
    let axis_is_direction_invariant = matches!(extent, Some(Termination::ThroughAll))
        && exit_kind.is_none_or(|exit| exit == kind);
    let placements_complete = !placements.is_empty()
        && !placements
            .iter()
            .enumerate()
            .any(|(index, placement)| placements[index + 1..].contains(placement))
        && placements.iter().all(|placement| match placement {
            cadmpeg_ir::features::HolePlacement::Directed {
                position,
                direction,
            } => finite_point(*position) && finite_direction(*direction),
            cadmpeg_ir::features::HolePlacement::Axis { origin, axis } => {
                axis_is_direction_invariant && finite_point(*origin) && finite_direction(*axis)
            }
        });
    let placements_incomplete = !placements.is_empty() && !placements_complete;
    let authored_axis_incomplete = position.is_some_and(|point| !finite_point(point))
        || direction.is_some_and(|vector| !finite_direction(vector));
    let location_unresolved =
        !placements_complete && position.is_none() && profile.is_none_or(profile_ref_is_incomplete);
    let orientation_unresolved = !placements_complete
        && direction.is_none()
        && face.is_none_or(face_selection_is_incomplete);
    profile_incomplete
        || face_incomplete
        || authored_axis_incomplete
        || placements_incomplete
        || location_unresolved
        || orientation_unresolved
        || hole_kind_is_incomplete(kind, diameter)
        || exit_kind.is_some_and(|kind| hole_kind_is_incomplete(kind, diameter))
        || diameter.is_none_or(|diameter| !positive_feature_length(diameter))
        || extent.is_none_or(termination_is_incomplete)
}

fn hole_kind_is_incomplete(kind: &HoleKind, bore_diameter: Option<Length>) -> bool {
    let valid_angle = |angle: cadmpeg_ir::features::Angle| {
        angle.0.is_finite() && angle.0 > 0.0 && angle.0 < std::f64::consts::PI
    };
    let treatment_diameter_is_incomplete = |diameter: Length| {
        !positive_feature_length(diameter) || bore_diameter.is_none_or(|bore| diameter.0 <= bore.0)
    };
    match kind {
        HoleKind::Unresolved { .. } => true,
        HoleKind::Simple => false,
        HoleKind::Chamfer { diameter, angle } | HoleKind::Countersink { diameter, angle } => {
            treatment_diameter_is_incomplete(*diameter) || !valid_angle(*angle)
        }
        HoleKind::SimpleDrilled { drill_point_angle } => !valid_angle(*drill_point_angle),
        HoleKind::Counterbore { diameter, depth } => {
            treatment_diameter_is_incomplete(*diameter) || !positive_feature_length(*depth)
        }
        HoleKind::CounterboreDrilled {
            diameter,
            depth,
            drill_point_angle,
        } => {
            treatment_diameter_is_incomplete(*diameter)
                || !positive_feature_length(*depth)
                || !valid_angle(*drill_point_angle)
        }
        HoleKind::Threaded {
            major_diameter,
            thread_depth,
            pitch,
            drill_point_angle,
        } => {
            !positive_feature_length(*major_diameter)
                || !positive_feature_length(*thread_depth)
                || pitch.is_some_and(|pitch| !positive_feature_length(pitch))
                || !valid_angle(*drill_point_angle)
                || bore_diameter.is_none_or(|diameter| major_diameter.0 <= diameter.0)
        }
        HoleKind::Counterdrill {
            diameter,
            entry_diameter,
            depth,
            angle,
        } => {
            treatment_diameter_is_incomplete(*diameter)
                || entry_diameter
                    .is_some_and(|entry| !positive_feature_length(entry) || entry.0 <= diameter.0)
                || !positive_feature_length(*depth)
                || !valid_angle(*angle)
        }
    }
}

pub(crate) fn hole_auxiliary_semantics_are_incomplete(
    profile_filter: Option<&cadmpeg_ir::features::HoleProfileFilter>,
    bottom: Option<&cadmpeg_ir::features::HoleBottom>,
    taper_angle: Option<cadmpeg_ir::features::Angle>,
    specification: Option<&cadmpeg_ir::features::HoleSpecification>,
) -> bool {
    let valid_angle = |angle: cadmpeg_ir::features::Angle| {
        angle.0.is_finite() && angle.0 > 0.0 && angle.0 < std::f64::consts::PI
    };
    profile_filter.is_some_and(|filter| !filter.points && !filter.circles && !filter.arcs)
        || bottom.is_some_and(|bottom| {
            matches!(
                bottom,
                cadmpeg_ir::features::HoleBottom::Angled { included_angle, .. }
                    if !valid_angle(*included_angle)
            )
        })
        || taper_angle.is_some_and(|angle| !valid_angle(angle))
        || specification.is_some_and(|specification| {
            specification.standard.trim().is_empty()
                || specification
                    .pitch
                    .is_some_and(|pitch| !positive_feature_length(pitch))
                || specification
                    .major_diameter
                    .is_some_and(|diameter| !positive_feature_length(diameter))
                || specification
                    .clearance
                    .is_some_and(|clearance| !clearance.0.is_finite())
                || matches!(
                    specification.depth,
                    cadmpeg_ir::features::HoleThreadDepth::Blind { depth }
                        if !positive_feature_length(depth)
                )
        })
}

fn chamfer_spec_is_incomplete(spec: &ChamferSpec) -> bool {
    match spec {
        ChamferSpec::Unresolved { .. } => true,
        ChamferSpec::Distance { distance } => !positive_feature_length(*distance),
        ChamferSpec::TwoDistances { first, second } => {
            !positive_feature_length(*first) || !positive_feature_length(*second)
        }
        ChamferSpec::DistanceAngle { distance, angle } => {
            !positive_feature_length(*distance)
                || !angle.0.is_finite()
                || angle.0 <= 0.0
                || angle.0 >= std::f64::consts::PI
        }
    }
}

pub(crate) fn extrude_extent_is_incomplete(
    extent: &ExtrudeExtent,
    dependencies: &[FeatureId],
) -> bool {
    let side_is_incomplete = |side: &cadmpeg_ir::features::ExtrudeSide| {
        termination_is_incomplete(&side.termination)
            || termination_dependency_is_incomplete(&side.termination, dependencies)
            || side.draft.is_some_and(|angle| {
                !angle.0.is_finite() || angle.0.abs() >= std::f64::consts::FRAC_PI_2
            })
            || side.offset.is_some_and(|offset| !offset.0.is_finite())
    };
    match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
            side_is_incomplete(side)
        }
        ExtrudeExtent::TwoSided { first, second } => {
            side_is_incomplete(first) || side_is_incomplete(second)
        }
    }
}

pub(crate) fn extrude_start_is_incomplete(start: &ExtrudeStart) -> bool {
    match start {
        ExtrudeStart::Unresolved => true,
        ExtrudeStart::FromFace { face, offset } => {
            face_selection_is_incomplete(face) || offset.is_some_and(|offset| !offset.0.is_finite())
        }
        ExtrudeStart::OffsetProfilePlane { offset } => !offset.0.is_finite(),
        ExtrudeStart::ProfilePlane => false,
    }
}

pub(crate) fn revolve_feature_is_incomplete(
    construction: &RevolutionConstruction,
    op: BooleanOp,
    dependencies: &[FeatureId],
) -> bool {
    construction
        .profile
        .as_ref()
        .is_none_or(profile_ref_is_incomplete)
        || construction
            .profile
            .as_ref()
            .is_some_and(|profile| profile_dependency_is_incomplete(profile, dependencies))
        || construction.axis.is_none_or(|axis| {
            !finite_feature_point(axis.origin) || !unit_feature_direction(axis.direction)
        })
        || construction.extent.as_ref().is_none_or(|extent| {
            let side_is_incomplete = |termination: &Termination| {
                termination_is_incomplete(termination)
                    || termination_dependency_is_incomplete(termination, dependencies)
            };
            match extent {
                RevolveExtent::OneSided { termination }
                | RevolveExtent::Symmetric { termination } => side_is_incomplete(termination),
                RevolveExtent::TwoSided { first, second } => {
                    side_is_incomplete(first) || side_is_incomplete(second)
                }
            }
        })
        || construction
            .axis_reference
            .as_ref()
            .is_some_and(path_ref_is_incomplete)
        || construction.solid.is_none()
        || construction
            .face_maker_class
            .as_ref()
            .is_some_and(|class| class.trim().is_empty())
        || matches!(op, BooleanOp::Unresolved)
}

pub(crate) fn termination_is_incomplete(termination: &Termination) -> bool {
    match termination {
        Termination::Unresolved => true,
        Termination::ToFace { face, offset } => {
            face_selection_is_incomplete(face) || offset.is_some_and(|offset| !offset.0.is_finite())
        }
        Termination::ToVertex { vertex } => match vertex {
            VertexSelection::Generated { vertex, native } => {
                native.trim().is_empty() || vertex.local_id.trim().is_empty()
            }
            VertexSelection::Unresolved | VertexSelection::Native(_) => true,
        },
        Termination::OffsetFromFace { face, offset } => {
            face_selection_is_incomplete(face) || !positive_feature_length(*offset)
        }
        Termination::ToShape { target } => face_selection_is_incomplete(target),
        Termination::Blind { length } => !length.0.is_finite() || length.0 == 0.0,
        Termination::Angle { angle } => !angle.0.is_finite() || angle.0 <= 0.0,
        Termination::ThroughAll
        | Termination::ThroughNext
        | Termination::ToFirst
        | Termination::ToLast => false,
    }
}

pub(crate) fn termination_dependency_is_incomplete(
    termination: &Termination,
    dependencies: &[FeatureId],
) -> bool {
    matches!(
        termination,
        Termination::ToVertex {
            vertex: VertexSelection::Generated { vertex, .. },
        } if !dependencies.contains(&vertex.feature)
    )
}

pub(crate) fn rib_feature_is_incomplete(construction: &RibConstruction, op: BooleanOp) -> bool {
    construction
        .profile
        .as_ref()
        .is_none_or(profile_ref_is_incomplete)
        || construction
            .direction
            .is_none_or(|direction| !valid_feature_direction(direction))
        || construction
            .thickness
            .is_none_or(|thickness| !positive_feature_length(thickness))
        || construction.side.is_none()
        || matches!(construction.draft, RibDraft::Unresolved)
        || matches!(construction.draft, RibDraft::Angle(angle) if !valid_draft_angle(angle))
        || matches!(op, BooleanOp::Unresolved)
}

pub(crate) fn sweep_mode_is_incomplete(mode: SweepMode) -> bool {
    match mode {
        SweepMode::Unresolved
        | SweepMode::Solid {
            op: BooleanOp::Unresolved,
        } => true,
        SweepMode::Solid { .. } | SweepMode::Surface => false,
    }
}

pub(crate) fn sweep_orientation_is_incomplete(orientation: &SweepOrientation) -> bool {
    match orientation {
        SweepOrientation::Auxiliary { path, .. } => path_ref_is_incomplete(path),
        SweepOrientation::Binormal { direction } => !valid_feature_direction(*direction),
        SweepOrientation::CorrectedFrenet | SweepOrientation::Fixed | SweepOrientation::Frenet => {
            false
        }
    }
}

pub(crate) fn pattern_is_incomplete(pattern: &PatternKind) -> bool {
    match pattern {
        PatternKind::Unresolved { .. } => true,
        PatternKind::Linear {
            direction,
            spacing,
            count,
            second,
        } => {
            direction.is_none_or(|direction| !valid_feature_direction(direction))
                || !positive_feature_length(*spacing)
                || *count == 0
                || second.as_ref().is_some_and(|second| {
                    !valid_feature_direction(second.direction)
                        || !positive_feature_length(second.spacing)
                        || second.count == 0
                })
        }
        PatternKind::LinearOffsets { direction, offsets } => {
            direction.is_none_or(|direction| !valid_feature_direction(direction))
                || !valid_increasing_locations(offsets.iter().map(|offset| offset.0))
        }
        PatternKind::Circular {
            axis_origin,
            axis_dir,
            angle,
            count,
        } => {
            !finite_feature_point(*axis_origin)
                || !valid_feature_direction(*axis_dir)
                || !angle.0.is_finite()
                || angle.0 <= 0.0
                || *count == 0
        }
        PatternKind::CircularAngles {
            axis_origin,
            axis_dir,
            angles,
        } => {
            !finite_feature_point(*axis_origin)
                || !valid_feature_direction(*axis_dir)
                || !valid_increasing_locations(angles.iter().map(|angle| angle.0))
        }
        PatternKind::Mirror {
            plane_origin,
            plane_normal,
        } => !finite_feature_point(*plane_origin) || !valid_feature_direction(*plane_normal),
        PatternKind::CurveDriven {
            path,
            spacing,
            count,
        } => {
            path.as_ref().is_none_or(path_ref_is_incomplete)
                || !positive_feature_length(*spacing)
                || *count == 0
        }
        PatternKind::Scale {
            center,
            final_factor,
            count,
        } => {
            matches!(center, cadmpeg_ir::features::PatternScaleCenter::Native(_))
                || matches!(
                    center,
                    cadmpeg_ir::features::PatternScaleCenter::Point(point)
                        if !finite_feature_point(*point)
                )
                || !final_factor.is_finite()
                || *final_factor <= 0.0
                || *count < 2
        }
        PatternKind::Composite { stages } => {
            stages.is_empty()
                || stages.iter().enumerate().any(|(index, stage)| {
                    stage.combination
                        != if index == 0 {
                            cadmpeg_ir::features::PatternStageCombination::Initialize
                        } else if matches!(*stage.pattern, PatternKind::Scale { .. }) {
                            cadmpeg_ir::features::PatternStageCombination::AlignedSlices
                        } else {
                            cadmpeg_ir::features::PatternStageCombination::CartesianProduct
                        }
                        || matches!(*stage.pattern, PatternKind::Composite { .. })
                        || pattern_is_incomplete(&stage.pattern)
                })
                || pattern_composition_is_incomplete(stages)
        }
    }
}

pub(crate) fn pattern_feature_is_incomplete(
    seeds: &[cadmpeg_ir::features::PatternSeed],
    pattern: &PatternKind,
    dependencies: &[cadmpeg_ir::features::FeatureId],
) -> bool {
    seeds.is_empty()
        || seeds.iter().any(|seed| match seed {
            cadmpeg_ir::features::PatternSeed::Feature(feature) => !dependencies.contains(feature),
            cadmpeg_ir::features::PatternSeed::Faces(faces) => face_selection_is_incomplete(faces),
            cadmpeg_ir::features::PatternSeed::Bodies(bodies) => {
                body_selection_is_incomplete(bodies)
            }
            cadmpeg_ir::features::PatternSeed::Occurrences(occurrences) => occurrences.is_empty(),
        })
        || seeds
            .iter()
            .enumerate()
            .any(|(index, seed)| seeds[..index].contains(seed))
        || pattern_is_incomplete(pattern)
}

pub(crate) fn radius_spec_is_incomplete(radius: &RadiusSpec) -> bool {
    match radius {
        RadiusSpec::Unresolved { .. } => true,
        RadiusSpec::Constant { radius } => !positive_feature_length(*radius),
        RadiusSpec::Chordal { chord_length } => !positive_feature_length(*chord_length),
        RadiusSpec::Variable { points } => {
            points.len() < 2
                || points.iter().any(|point| {
                    !point.parameter.is_finite()
                        || !(0.0..=1.0).contains(&point.parameter)
                        || !point.radius.0.is_finite()
                        || point.radius.0 < 0.0
                })
                || !points.iter().any(|point| point.radius.0 > 0.0)
                || points
                    .windows(2)
                    .any(|pair| pair[0].parameter >= pair[1].parameter)
        }
    }
}

fn positive_feature_length(length: Length) -> bool {
    length.0.is_finite() && length.0 > 0.0
}

fn valid_draft_angle(angle: cadmpeg_ir::features::Angle) -> bool {
    angle.0.is_finite() && angle.0.abs() < std::f64::consts::FRAC_PI_2
}

fn valid_feature_direction(direction: Vector3) -> bool {
    direction.norm().is_finite() && direction.norm() > 0.0
}

fn finite_feature_point(point: Point3) -> bool {
    [point.x, point.y, point.z].into_iter().all(f64::is_finite)
}

fn valid_increasing_locations(locations: impl Iterator<Item = f64>) -> bool {
    let mut locations = locations;
    let Some(first) = locations.next() else {
        return false;
    };
    first == 0.0
        && locations
            .try_fold(first, |previous, location| {
                (location.is_finite() && location > previous).then_some(location)
            })
            .is_some()
}

fn pattern_composition_is_incomplete(stages: &[cadmpeg_ir::features::PatternStage]) -> bool {
    let mut occurrences = None;
    stages.iter().enumerate().any(|(index, stage)| {
        let Some(stage_count) = pattern_occurrence_count(&stage.pattern) else {
            return false;
        };
        if stage_count == 0 {
            return true;
        }
        if index == 0 {
            occurrences = Some(stage_count);
            return false;
        }
        match stage.combination {
            cadmpeg_ir::features::PatternStageCombination::CartesianProduct => {
                if let Some(count) = occurrences {
                    occurrences = count.checked_mul(stage_count);
                    occurrences.is_none()
                } else {
                    false
                }
            }
            cadmpeg_ir::features::PatternStageCombination::AlignedSlices => {
                occurrences.is_some_and(|count| count % stage_count != 0)
            }
            cadmpeg_ir::features::PatternStageCombination::Initialize => true,
        }
    })
}

fn pattern_occurrence_count(pattern: &PatternKind) -> Option<usize> {
    match pattern {
        PatternKind::Linear { count, .. }
        | PatternKind::Circular { count, .. }
        | PatternKind::CurveDriven { count, .. }
        | PatternKind::Scale { count, .. } => usize::try_from(*count).ok(),
        PatternKind::LinearOffsets { offsets, .. } => Some(offsets.len()),
        PatternKind::CircularAngles { angles, .. } => Some(angles.len()),
        PatternKind::Mirror { .. } => Some(2),
        PatternKind::Unresolved { .. } | PatternKind::Composite { .. } => None,
    }
}

pub(crate) fn body_selection_is_incomplete(selection: &BodySelection) -> bool {
    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
            selection_ids_are_incomplete(bodies)
        }
        BodySelection::Local { bodies, native } => {
            native.trim().is_empty()
                || selection_ids_are_incomplete(bodies)
                || bodies.iter().any(|body| body.trim().is_empty())
        }
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Native(_)
        | BodySelection::NativeSet(_) => true,
    }
}

pub(crate) fn body_selections_overlap(first: &BodySelection, second: &BodySelection) -> bool {
    match (first, second) {
        (
            BodySelection::Local { bodies: first, .. },
            BodySelection::Local { bodies: second, .. },
        ) => first.iter().any(|body| second.contains(body)),
        _ => explicit_body_ids(first).is_some_and(|first| {
            explicit_body_ids(second)
                .is_some_and(|second| first.iter().any(|body| second.contains(body)))
        }),
    }
}

fn explicit_body_ids(selection: &BodySelection) -> Option<&[BodyId]> {
    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => Some(bodies),
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Local { .. }
        | BodySelection::Native(_)
        | BodySelection::NativeSet(_) => None,
    }
}

fn resolved_body_selection_len(selection: &BodySelection) -> Option<usize> {
    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
            Some(bodies.len())
        }
        BodySelection::Local { bodies, .. } => Some(bodies.len()),
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Native(_)
        | BodySelection::NativeSet(_) => None,
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
        ProfileRef::Unresolved(_)
        | ProfileRef::Native(_)
        | ProfileRef::SketchSelection { .. }
        | ProfileRef::SpatialSketchSelection { .. } => true,
        ProfileRef::Sketch(_) => false,
        ProfileRef::SketchEntities { entities, .. } => selection_ids_are_incomplete(entities),
        ProfileRef::SketchProfiles { profiles, .. }
        | ProfileRef::SpatialSketchProfiles { profiles, .. } => {
            selection_ids_are_incomplete(profiles)
        }
        ProfileRef::SketchRegions { regions, .. } => {
            regions.is_empty()
                || regions
                    .iter()
                    .enumerate()
                    .any(|(index, region)| regions[..index].contains(region))
        }
        ProfileRef::HistoricalFaces { faces, .. } => selection_ids_are_incomplete(faces),
        ProfileRef::Generated { curves, native } => {
            native.trim().is_empty()
                || curves.is_empty()
                || curves.iter().enumerate().any(|(index, curve)| {
                    curve.local_id.trim().is_empty() || curves[..index].contains(curve)
                })
        }
        ProfileRef::Feature(_) => false,
        ProfileRef::Faces(faces) => selection_ids_are_incomplete(faces),
    }
}

pub(crate) fn profile_dependency_is_incomplete(
    profile: &ProfileRef,
    dependencies: &[FeatureId],
) -> bool {
    match profile {
        ProfileRef::Feature(feature) => !dependencies.contains(feature),
        ProfileRef::Generated { curves, .. } => curves
            .iter()
            .any(|curve| !dependencies.contains(&curve.feature)),
        _ => false,
    }
}

pub(crate) fn loft_section_is_incomplete(section: &LoftSection) -> bool {
    match section {
        LoftSection::Profile(profile) => profile_ref_is_incomplete(profile),
        LoftSection::Point(LoftPointSection::Native(_)) => true,
        LoftSection::Point(LoftPointSection::Point(point)) => !finite_feature_point(*point),
        LoftSection::Point(LoftPointSection::Vertex(vertex)) => vertex.0.trim().is_empty(),
    }
}

fn selection_ids_are_incomplete<T: Ord>(ids: &[T]) -> bool {
    ids.is_empty() || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
}

pub(crate) fn path_ref_is_incomplete(path: &PathRef) -> bool {
    match path {
        PathRef::Unresolved(_) | PathRef::Native(_) | PathRef::SpatialSketchSelection { .. } => {
            true
        }
        PathRef::HistoricalEdges { edges, .. } => selection_ids_are_incomplete(edges),
        PathRef::Sketch(_) => false,
        PathRef::SketchCurves { curves, .. } => selection_ids_are_incomplete(curves),
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
    let (control_count, classified_control_count) = offset_store_control_counts(c);
    let mut notes = vec![format!(
        "SPLMSSTR container: version {:#04x}, file tag {}, footer offset {}, {} HEADER and {} FOOTER directory entry/ies, fingerprint {:08x}",
        c.version,
        c.file_tag,
        c.footer_offset,
        c.header_entry_count,
        c.footer_entry_count,
        u32::from_be_bytes(c.footer_fingerprint),
    )];
    notes.push(format!(
        "embedded streams: {} partition, {} deltas, {} plain (cached body), {} preview/non-Parasolid",
        scan.count(StreamKind::Partition),
        scan.count(StreamKind::Deltas),
        scan.count(StreamKind::Plain),
        scan.count(StreamKind::Preview),
    ));
    if control_count != 0 {
        notes.push(format!(
            "NX object model: {classified_control_count} of {control_count} bounded offset-store control block(s) have an admitted complete grammar"
        ));
    }
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

#[cfg(test)]
mod tests {
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, NurbsCurve,
        NurbsSurface, Pcurve, PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition,
        ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
        ProceduralSurfaceId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, PcurveUse, Point, Sense, Vertex};
    use cadmpeg_ir::AnnotationBuilder;

    #[test]
    fn analytic_closed_isocurves_retain_the_native_full_turn() {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let cone = SurfaceId("nx:test:cone".into());
        let sphere = SurfaceId("nx:test:sphere".into());
        let torus = SurfaceId("nx:test:torus".into());
        ir.model.surfaces.extend([
            Surface {
                id: cone.clone(),
                geometry: SurfaceGeometry::Cone {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    radius: 2.0,
                    ratio: 0.5,
                    half_angle: 0.25_f64.atan(),
                },
                source_object: None,
            },
            Surface {
                id: sphere.clone(),
                geometry: SurfaceGeometry::Sphere {
                    center: Point3::new(0.0, 0.0, 0.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    radius: 2.0,
                },
                source_object: None,
            },
            Surface {
                id: torus.clone(),
                geometry: SurfaceGeometry::Torus {
                    center: Point3::new(0.0, 0.0, 0.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    major_radius: 3.0,
                    minor_radius: 1.0,
                },
                source_object: None,
            },
        ]);
        let plane = SurfaceId("nx:test:plane".into());
        ir.model.surfaces.push(Surface {
            id: plane.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 1.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        let cone_ellipse = CurveId("nx:test:cone-ellipse".into());
        let sphere_circle = CurveId("nx:test:sphere-circle".into());
        let torus_circle = CurveId("nx:test:torus-circle".into());
        ir.model.curves.extend([
            Curve {
                id: cone_ellipse.clone(),
                geometry: CurveGeometry::Ellipse {
                    center: Point3::new(0.0, 0.0, 1.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                    major_direction: Vector3::new(1.0, 0.0, 0.0),
                    major_radius: 2.25,
                    minor_radius: 1.125,
                },
                source_object: None,
            },
            Curve {
                id: sphere_circle.clone(),
                geometry: CurveGeometry::Circle {
                    center: Point3::new(0.0, 0.0, 1.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    radius: 3.0_f64.sqrt(),
                },
                source_object: None,
            },
            Curve {
                id: torus_circle.clone(),
                geometry: CurveGeometry::Circle {
                    center: Point3::new(3.0, 0.0, 0.0),
                    axis: Vector3::new(0.0, -1.0, 0.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    radius: 1.0,
                },
                source_object: None,
            },
        ]);

        let range = [0.0, std::f64::consts::TAU];
        let cone_pcurve =
            super::exact_analytic_isocurve_pcurve(&ir, &cone_ellipse, &cone, range, 1.0e-12)
                .expect("cone ellipse");
        let sphere_pcurve =
            super::exact_analytic_isocurve_pcurve(&ir, &sphere_circle, &sphere, range, 1.0e-12)
                .expect("sphere parallel");
        let torus_pcurve =
            super::exact_analytic_isocurve_pcurve(&ir, &torus_circle, &torus, range, 1.0e-12)
                .expect("torus meridian");
        assert!(matches!(
            sphere_pcurve,
            PcurveGeometry::Line { origin, direction }
                if (origin.v - std::f64::consts::FRAC_PI_6).abs() < 1.0e-12
                    && direction.u == 1.0
                    && direction.v == 0.0
        ));
        assert!(matches!(
            torus_pcurve,
            PcurveGeometry::Line { origin, direction }
                if origin.u.abs() < 1.0e-12
                    && direction.u == 0.0
                    && direction.v == 1.0
        ));
        assert!(matches!(
            cone_pcurve,
            PcurveGeometry::Line { origin, direction }
                if (origin.v - 1.0).abs() < 1.0e-12
                    && direction.u == 1.0
                    && direction.v == 0.0
        ));
        for parameter in [0.0, 1.0, 3.0, 5.0, std::f64::consts::TAU] {
            for (curve, surface, pcurve) in [
                (&cone_ellipse, &cone, &cone_pcurve),
                (&sphere_circle, &sphere, &sphere_pcurve),
                (&torus_circle, &torus, &torus_pcurve),
            ] {
                let curve = ir
                    .model
                    .curves
                    .iter()
                    .find(|candidate| &candidate.id == curve)
                    .unwrap();
                let expected = cadmpeg_ir::eval::curve_point(&curve.geometry, parameter).unwrap();
                let uv = cadmpeg_ir::eval::pcurve_uv(pcurve, parameter).unwrap();
                let actual =
                    cadmpeg_ir::eval::model_surface_point_by_id(&ir, surface, uv.u, uv.v).unwrap();
                assert!(super::point_distance(expected, actual) < 1.0e-12);
            }
        }

        let construction = ProceduralCurveId("nx:test:closed-intersection".into());
        ir.model.procedural_curves.push(ProceduralCurve {
            id: construction,
            curve: sphere_circle.clone(),
            definition: ProceduralCurveDefinition::TolerantIntersection {
                supports: [sphere, plane],
                endpoints: [
                    Point3::new(3.0_f64.sqrt(), 0.0, 1.0),
                    Point3::new(3.0_f64.sqrt(), 0.0, 1.0),
                ],
                tolerance: 1.0e-8,
                parameterization: None,
            },
            cache_fit_tolerance: None,
        });
        let point = PointId("nx:test:closed-point".into());
        let vertex = VertexId("nx:test:closed-vertex".into());
        ir.model.points.push(Point {
            id: point.clone(),
            position: Point3::new(3.0_f64.sqrt(), 0.0, 1.0),
            source_object: None,
        });
        ir.model.vertices.push(Vertex {
            id: vertex.clone(),
            point,
            tolerance: Some(1.0e-8),
        });
        ir.model.edges.push(Edge {
            id: EdgeId("nx:test:closed-edge".into()),
            curve: Some(sphere_circle),
            start: vertex.clone(),
            end: vertex,
            param_range: None,
            tolerance: Some(1.0e-8),
        });

        super::complete_exact_boundary_intersection_pcurves(&mut ir, &mut AnnotationBuilder::new());
        let ProceduralCurveDefinition::TolerantIntersection {
            supports,
            parameterization: Some(parameterization),
            ..
        } = &ir.model.procedural_curves[0].definition
        else {
            panic!("closed intersection parameterization");
        };
        assert_eq!(parameterization.parameter_range, range);
        assert_eq!(ir.model.edges[0].param_range, Some(range));
        assert!(parameterization
            .pcurves
            .iter()
            .enumerate()
            .all(|(side, pcurve)| {
                for parameter in [0.0, 1.0, 3.0, 5.0, std::f64::consts::TAU] {
                    let Some(uv) = cadmpeg_ir::eval::pcurve_uv(pcurve, parameter) else {
                        return false;
                    };
                    let Some(point) = cadmpeg_ir::eval::model_surface_point_by_id(
                        &ir,
                        &supports[side],
                        uv.u,
                        uv.v,
                    ) else {
                        return false;
                    };
                    if (point.z - 1.0).abs() > 1.0e-8 {
                        return false;
                    }
                }
                true
            }));
        for parameter in [0.0, 1.0, 3.0, 5.0, std::f64::consts::TAU] {
            let curve = &ir.model.procedural_curves[0].curve;
            let point = cadmpeg_ir::eval::model_curve_point_by_id(&ir, curve, parameter)
                .expect("closed intersection evaluates");
            let inverse =
                cadmpeg_ir::eval::model_curve_parameter_near_point(&ir, curve, point, parameter)
                    .unwrap_or_else(|| {
                        panic!("closed intersection inverts at parameter {parameter}")
                    });
            assert!((inverse - parameter).abs() < 1.0e-10);
        }
    }

    fn affine_nurbs_surface(z: f64) -> SurfaceGeometry {
        SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                Point3::new(0.0, 0.0, z),
                Point3::new(0.0, 2.0, z),
                Point3::new(3.0, 0.0, z),
                Point3::new(3.0, 2.0, z),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        })
    }

    fn quadratic_translation_surface(z: f64) -> SurfaceGeometry {
        SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree: 2,
            v_degree: 2,
            u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            u_count: 3,
            v_count: 3,
            control_points: [0.0, 1.0, 3.0]
                .into_iter()
                .flat_map(|x| {
                    [0.0, 2.0, 5.0]
                        .into_iter()
                        .map(move |y| Point3::new(x, y, z))
                })
                .collect(),
            weights: Some(vec![2.0; 9]),
            u_periodic: false,
            v_periodic: false,
        })
    }

    fn degree_elevated_affine_surface(z: f64) -> SurfaceGeometry {
        SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree: 2,
            v_degree: 2,
            u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            u_count: 3,
            v_count: 3,
            control_points: [0.0, 1.5, 3.0]
                .into_iter()
                .flat_map(|x| {
                    [0.0, 1.0, 2.0]
                        .into_iter()
                        .map(move |y| Point3::new(x, y, z))
                })
                .collect(),
            weights: None,
            u_periodic: false,
            v_periodic: false,
        })
    }

    fn quadratic_paraboloid_surface() -> SurfaceGeometry {
        let coordinates = [0.0, 0.5, 1.0];
        let square_controls = [0.0, 0.0, 1.0];
        SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree: 2,
            v_degree: 2,
            u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            u_count: 3,
            v_count: 3,
            control_points: (0..3)
                .flat_map(|u| {
                    (0..3).map(move |v| {
                        Point3::new(
                            coordinates[u],
                            coordinates[v],
                            square_controls[u] + square_controls[v],
                        )
                    })
                })
                .collect(),
            weights: None,
            u_periodic: false,
            v_periodic: false,
        })
    }

    #[test]
    fn planar_offset_cache_fit_is_certified_over_the_control_net() {
        let support = affine_nurbs_surface(0.0);
        let mut candidate = affine_nurbs_surface(4.0);
        let SurfaceGeometry::Nurbs(candidate) = &mut candidate else {
            unreachable!();
        };
        candidate.control_points[3].z += 0.000_5;

        let fit = super::certified_offset_cache_fit(
            &support,
            &SurfaceGeometry::Nurbs(candidate.clone()),
            4.0,
            0.001,
        )
        .expect("whole-patch fit");
        assert!((fit - 0.000_5).abs() < 1.0e-12);
        assert!(super::certified_offset_cache_fit(
            &support,
            &SurfaceGeometry::Nurbs(candidate.clone()),
            4.0,
            0.000_4
        )
        .is_none());
    }

    #[test]
    fn offset_cache_fit_accepts_higher_degree_translation_nets() {
        assert_eq!(
            super::certified_offset_cache_fit(
                &quadratic_translation_surface(0.0),
                &quadratic_translation_surface(4.0),
                4.0,
                0.0
            ),
            Some(0.0)
        );
    }

    #[test]
    fn periodic_offset_cache_fit_covers_the_complete_active_domain() {
        let mut support = quadratic_paraboloid_surface();
        let mut candidate = support.clone();
        let SurfaceGeometry::Nurbs(support_surface) = &mut support else {
            unreachable!();
        };
        let SurfaceGeometry::Nurbs(candidate_surface) = &mut candidate else {
            unreachable!();
        };
        support_surface.u_periodic = true;
        candidate_surface.u_periodic = true;

        assert_eq!(
            super::certified_offset_cache_fit(&support, &candidate, 0.0, 0.0),
            Some(0.0)
        );
    }

    #[test]
    fn offset_cache_fit_certifies_differing_bases_on_one_parameter_domain() {
        let bound = super::certified_offset_cache_fit(
            &affine_nurbs_surface(0.0),
            &degree_elevated_affine_surface(4.0),
            4.0,
            0.1,
        )
        .expect("degree-elevated cache fit");
        assert!(bound <= 0.1);
    }

    #[test]
    fn curved_offset_cache_fit_uses_span_local_derivative_bounds() {
        let support = quadratic_paraboloid_surface();
        assert_eq!(
            super::certified_offset_cache_fit(&support, &support, 0.0, 0.0),
            Some(0.0)
        );
        let bound = super::certified_offset_cache_fit(&support, &support, 0.01, 0.02)
            .expect("nonzero curved offset certified");
        assert!((0.01..=0.02).contains(&bound));
    }

    #[test]
    fn offset_cache_fit_decouples_distant_knot_span_scale() {
        let x = [0.0, 0.25, 0.5, 1.0e9 + 0.5];
        let z = [0.0, 0.0, 0.1, 0.2];
        let support = SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree: 2,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 4,
            v_count: 2,
            control_points: (0..4)
                .flat_map(|u| (0..2).map(move |v| Point3::new(x[u], v as f64, z[u])))
                .collect(),
            weights: None,
            u_periodic: false,
            v_periodic: false,
        });

        let bound = super::certified_offset_cache_fit(&support, &support, 0.01, 0.02)
            .expect("each regular knot span certifies independently");
        assert!((0.01..=0.02).contains(&bound));
    }

    #[test]
    fn offset_cache_fit_certifies_regular_c0_knot_spans() {
        let x = [0.0, 0.25, 0.5, 1.0, 1.5];
        let z = [0.0, 0.0, 0.1, 0.1, 0.2];
        let support = SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree: 2,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 5,
            v_count: 2,
            control_points: (0..5)
                .flat_map(|u| (0..2).map(move |v| Point3::new(x[u], v as f64, z[u])))
                .collect(),
            weights: None,
            u_periodic: false,
            v_periodic: false,
        });

        let bound = super::certified_offset_cache_fit(&support, &support, 0.01, 0.02)
            .expect("regular spans certify across the C0 knot break");
        assert!((0.01..=0.02).contains(&bound));
    }

    #[test]
    fn curved_offset_cache_fit_rejects_an_uncertified_fold() {
        let mut support = quadratic_paraboloid_surface();
        let SurfaceGeometry::Nurbs(surface) = &mut support else {
            unreachable!();
        };
        for v in 0..3 {
            surface.control_points[2 * 3 + v] = surface.control_points[3 + v];
        }
        assert!(super::certified_offset_cache_fit(&support, &support, 0.0, 1.0).is_none());
    }

    #[test]
    fn curved_offset_cache_fit_accepts_a_regular_turning_control_net() {
        let mut support = quadratic_paraboloid_surface();
        let SurfaceGeometry::Nurbs(surface) = &mut support else {
            unreachable!();
        };
        for v in 0..3 {
            surface.control_points[2 * 3 + v].x = 0.0;
        }
        assert_eq!(
            super::certified_offset_cache_fit(&support, &support, 0.0, 0.0),
            Some(0.0)
        );
    }

    #[test]
    fn curved_offset_cache_fit_certifies_deeply_localized_regularity() {
        let epsilon = 2.0_f64.powi(-100);
        let x = [0.0, epsilon / 3.0, 2.0 * epsilon / 3.0, 1.0 + epsilon];
        let z = [0.0, 0.0, 1.0 / 3.0, 1.0];
        let support = SurfaceGeometry::Nurbs(NurbsSurface {
            u_degree: 3,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 4,
            v_count: 2,
            control_points: (0..4)
                .flat_map(|u| (0..2).map(move |v| Point3::new(x[u], v as f64, z[u])))
                .collect(),
            weights: None,
            u_periodic: false,
            v_periodic: false,
        });
        let SurfaceGeometry::Nurbs(surface) = &support else {
            unreachable!();
        };

        assert!(super::translation_net_normal(surface).is_none());
        assert_eq!(
            super::certified_offset_cache_fit(&support, &support, 0.0, 0.0),
            Some(0.0)
        );
    }

    #[test]
    fn offset_cache_subdivision_uses_the_remaining_divisible_axis() {
        let u0 = 1.0_f64;
        let u1 = f64::from_bits(u0.to_bits() + 1);
        let u = u0 + (u1 - u0) * 0.5;
        let mut rectangles = Vec::new();

        assert!(super::subdivide_offset_rectangle(
            &mut rectangles,
            [u0, u1, 0.0, 1.0],
            [u, 0.5],
            true,
        ));
        assert_eq!(rectangles, vec![[u0, u1, 0.0, 0.5], [u0, u1, 0.5, 1.0]]);
    }

    #[test]
    fn curved_offset_cache_fit_certifies_varying_positive_weights() {
        let mut support = quadratic_paraboloid_surface();
        let SurfaceGeometry::Nurbs(surface) = &mut support else {
            unreachable!();
        };
        let axis_weights = [1.0, 1.01, 1.02];
        surface.weights = Some(
            (0..3)
                .flat_map(|u| (0..3).map(move |v| axis_weights[u] * axis_weights[v]))
                .collect(),
        );

        assert_eq!(
            super::certified_offset_cache_fit(&support, &support, 0.0, 0.0),
            Some(0.0)
        );
        assert!(super::certified_offset_cache_fit(&support, &support, 0.01, 0.02).is_some());
    }

    #[test]
    fn rational_offset_cache_bounds_are_translation_invariant() {
        let mut support = quadratic_paraboloid_surface();
        let SurfaceGeometry::Nurbs(surface) = &mut support else {
            unreachable!();
        };
        for point in &mut surface.control_points {
            point.x += 1.0e12;
            point.y -= 2.0e12;
            point.z += 3.0e12;
        }
        let axis_weights = [1.0, 1.01, 1.02];
        surface.weights = Some(
            (0..3)
                .flat_map(|u| (0..3).map(move |v| axis_weights[u] * axis_weights[v]))
                .collect(),
        );

        let bound = super::certified_offset_cache_fit(&support, &support, 0.01, 0.02)
            .expect("absolute placement does not widen rational derivative bounds");
        assert!(bound <= 0.02);
    }

    #[test]
    fn nurbs_surface_fit_uses_the_declared_geometric_tolerance() {
        let SurfaceGeometry::Nurbs(surface) = quadratic_paraboloid_surface() else {
            unreachable!();
        };
        let mut point = cadmpeg_ir::eval::nurbs_surface_point(&surface, 0.4, 0.6).unwrap();
        point.z += 0.001;

        let parameters =
            super::nurbs_parameters_with_tolerance(&surface, point, None, Some(0.01)).unwrap();
        let mapped =
            cadmpeg_ir::eval::nurbs_surface_point(&surface, parameters.u, parameters.v).unwrap();

        assert!(super::point_distance(mapped, point) <= 0.01);
    }

    #[test]
    fn saved_offset_cache_retains_its_procedural_lineage() {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let support = SurfaceId("nx:test:support".into());
        let cache = SurfaceId("nx:test:cache".into());
        ir.model.surfaces.extend([
            Surface {
                id: support.clone(),
                geometry: affine_nurbs_surface(0.0),
                source_object: None,
            },
            Surface {
                id: cache.clone(),
                geometry: affine_nurbs_surface(4.0),
                source_object: None,
            },
        ]);
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId("nx:test:offset".into()),
            surface: cache.clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: support.clone(),
                distance: 4.0,
                u_sense: Some(0),
                v_sense: Some(0),
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: Some(0.0),
            record_bounds: None,
        });

        assert_eq!(
            super::surface_offset_lineage(&ir, &cache, 0),
            Some((support, 4.0))
        );
    }

    #[test]
    fn serialized_surface_curves_select_a_terminal_intersection_branch() {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let surfaces = [
            SurfaceId("nx:test:surface#0".into()),
            SurfaceId("nx:test:surface#1".into()),
        ];
        for surface in &surfaces {
            ir.model.surfaces.push(Surface {
                id: surface.clone(),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            });
        }
        let curve = CurveId("nx:test:curve".into());
        let procedural = ProceduralCurveId("nx:test:intersection".into());
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Procedural {
                construction: procedural.clone(),
            },
            source_object: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: procedural,
            curve: curve.clone(),
            definition: ProceduralCurveDefinition::TolerantIntersection {
                supports: surfaces.clone(),
                endpoints: [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
                tolerance: 0.01,
                parameterization: None,
            },
            cache_fit_tolerance: None,
        });
        let points = [
            PointId("nx:test:point#0".into()),
            PointId("nx:test:point#1".into()),
        ];
        let vertices = [
            VertexId("nx:test:vertex#0".into()),
            VertexId("nx:test:vertex#1".into()),
        ];
        for index in 0..2 {
            ir.model.points.push(Point {
                id: points[index].clone(),
                position: Point3::new(0.005 + 9.99 * index as f64, 0.0, 0.0),
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: vertices[index].clone(),
                point: points[index].clone(),
                tolerance: None,
            });
        }
        let edge = EdgeId("nx:test:edge".into());
        ir.model.edges.push(Edge {
            id: edge.clone(),
            curve: Some(curve),
            start: vertices[0].clone(),
            end: vertices[1].clone(),
            param_range: None,
            tolerance: Some(0.03),
        });
        let pcurves = [
            PcurveId("nx:test:pcurve#0".into()),
            PcurveId("nx:test:pcurve#1".into()),
        ];
        let faces = [
            FaceId("nx:test:face#0".into()),
            FaceId("nx:test:face#1".into()),
        ];
        let loops = [
            LoopId("nx:test:loop#0".into()),
            LoopId("nx:test:loop#1".into()),
        ];
        let coedges = [
            CoedgeId("nx:test:coedge#0".into()),
            CoedgeId("nx:test:coedge#1".into()),
        ];
        for index in 0..2 {
            ir.model.pcurves.push(Pcurve {
                id: pcurves[index].clone(),
                geometry: PcurveGeometry::Line {
                    origin: Point2::new(0.0, 0.0),
                    direction: Point2::new(1.0, 0.0),
                },
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range: Some([0.0, 10.0]),
                fit_tolerance: Some(0.02),
            });
            ir.model.faces.push(Face {
                id: faces[index].clone(),
                shell: ShellId("nx:test:shell".into()),
                surface: surfaces[index].clone(),
                sense: Sense::Forward,
                loops: vec![loops[index].clone()],
                name: None,
                color: None,
                tolerance: Some(0.03),
            });
            ir.model.loops.push(Loop {
                id: loops[index].clone(),
                face: faces[index].clone(),
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                coedges: vec![coedges[index].clone()],
                vertex_uses: Vec::new(),
            });
            ir.model.coedges.push(Coedge {
                id: coedges[index].clone(),
                owner_loop: loops[index].clone(),
                edge: edge.clone(),
                next: coedges[index].clone(),
                previous: coedges[index].clone(),
                radial_next: coedges[1 - index].clone(),
                sense: Sense::Forward,
                pcurves: vec![PcurveUse {
                    pcurve: pcurves[index].clone(),
                    isoparametric: None,
                    parameter_range: Some([0.0, 10.0]),
                }],
                use_curve: None,
                use_curve_parameter_range: None,
            });
        }
        let serialized = [0, 1]
            .map(|index| {
                (
                    ir.model.procedural_curves[0].curve.clone(),
                    surfaces[index].clone(),
                    pcurves[index].clone(),
                )
            })
            .into_iter()
            .collect();
        super::complete_tolerant_intersection_pcurves_from_serialized_branches(
            &mut ir,
            &serialized,
            &mut AnnotationBuilder::new(),
        );
        assert!(matches!(
            ir.model.procedural_curves[0].definition,
            ProceduralCurveDefinition::TolerantIntersection {
                parameterization: None,
                ..
            }
        ));
        for pcurve in &mut ir.model.pcurves {
            pcurve.fit_tolerance = Some(0.01);
        }
        super::complete_tolerant_intersection_pcurves_from_serialized_branches(
            &mut ir,
            &serialized,
            &mut AnnotationBuilder::new(),
        );

        let ProceduralCurveDefinition::TolerantIntersection {
            parameterization: Some(parameterization),
            ..
        } = &ir.model.procedural_curves[0].definition
        else {
            panic!("serialized branch transferred");
        };
        assert_eq!(parameterization.parameter_range, [0.0, 10.0]);
        assert_eq!(ir.model.edges[0].param_range, Some([0.0, 10.0]));
        assert_eq!(
            cadmpeg_ir::eval::model_surface_point_by_id(&ir, &surfaces[0], 5.0, 0.0),
            cadmpeg_ir::eval::model_surface_point_by_id(&ir, &surfaces[1], 5.0, 0.0)
        );

        let ProceduralCurveDefinition::TolerantIntersection {
            parameterization, ..
        } = &mut ir.model.procedural_curves[0].definition
        else {
            unreachable!();
        };
        *parameterization = None;
        let edge = &mut ir.model.edges[0];
        edge.param_range = None;
        std::mem::swap(&mut edge.start, &mut edge.end);
        for pcurve in &mut ir.model.pcurves {
            pcurve.geometry = PcurveGeometry::Line {
                origin: Point2::new(10.0, 0.0),
                direction: Point2::new(-1.0, 0.0),
            };
        }
        super::complete_tolerant_intersection_pcurves_from_serialized_branches(
            &mut ir,
            &serialized,
            &mut AnnotationBuilder::new(),
        );
        let ProceduralCurveDefinition::TolerantIntersection {
            parameterization: Some(parameterization),
            ..
        } = &ir.model.procedural_curves[0].definition
        else {
            panic!("reversed serialized branch transferred");
        };
        assert!(parameterization.pcurves.iter().all(|pcurve| matches!(
            pcurve,
            PcurveGeometry::Line { origin, direction }
                if origin.u == 0.0 && direction.u == 1.0
        )));
        assert_eq!(ir.model.edges[0].start, vertices[0]);
        assert_eq!(ir.model.edges[0].end, vertices[1]);

        let range = [-1.5, 1.5];
        let canonical = PcurveGeometry::Ellipse {
            center: Point2::new(5.0, 0.0),
            x_axis: Point2::new(1.0, 0.0),
            y_axis: Point2::new(0.0, 1.0),
            major_radius: 4.0,
            minor_radius: 2.0,
        };
        let endpoints = range.map(|parameter| {
            let uv = cadmpeg_ir::eval::pcurve_uv(&canonical, parameter).unwrap();
            Point3::new(uv.u, uv.v, 0.0)
        });
        for (point, position) in ir.model.points.iter_mut().zip(endpoints) {
            point.position = position;
        }
        let ProceduralCurveDefinition::TolerantIntersection {
            endpoints: stored_endpoints,
            parameterization,
            ..
        } = &mut ir.model.procedural_curves[0].definition
        else {
            unreachable!();
        };
        *stored_endpoints = endpoints;
        *parameterization = None;
        ir.model.edges[0].param_range = None;
        for coedge in &mut ir.model.coedges {
            coedge.pcurves[0].parameter_range = Some(range);
        }
        for pcurve in &mut ir.model.pcurves {
            pcurve.parameter_range = Some(range);
            pcurve.geometry = PcurveGeometry::Ellipse {
                center: Point2::new(5.0, 0.0),
                x_axis: Point2::new(1.0, 0.0),
                y_axis: Point2::new(0.0, -1.0),
                major_radius: 4.0,
                minor_radius: 2.0,
            };
        }
        super::complete_tolerant_intersection_pcurves_from_serialized_branches(
            &mut ir,
            &serialized,
            &mut AnnotationBuilder::new(),
        );
        let ProceduralCurveDefinition::TolerantIntersection {
            parameterization: Some(parameterization),
            ..
        } = &ir.model.procedural_curves[0].definition
        else {
            panic!("reversed symmetric conic branches transferred");
        };
        assert_eq!(parameterization.parameter_range, range);
        assert!(parameterization.pcurves.iter().all(|pcurve| matches!(
            pcurve,
            PcurveGeometry::Ellipse { y_axis, .. } if y_axis.v == 1.0
        )));

        let ProceduralCurveDefinition::TolerantIntersection {
            tolerance,
            parameterization,
            ..
        } = &mut ir.model.procedural_curves[0].definition
        else {
            unreachable!();
        };
        *tolerance = 10.0;
        *parameterization = None;
        ir.model.edges[0].param_range = None;
        super::complete_tolerant_intersection_pcurves_from_serialized_branches(
            &mut ir,
            &serialized,
            &mut AnnotationBuilder::new(),
        );
        assert!(matches!(
            ir.model.procedural_curves[0].definition,
            ProceduralCurveDefinition::TolerantIntersection {
                parameterization: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn reversed_nurbs_pcurve_preserves_the_selected_interval() {
        let pcurve = PcurveGeometry::Nurbs {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 2.0, 2.0, 2.0],
            control_points: vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 2.0),
                Point2::new(3.0, 1.0),
            ],
            weights: Some(vec![1.0, 2.0, 1.5]),
            periodic: false,
        };
        let range = [0.25, 1.75];
        let reversed =
            super::reverse_pcurve_over_range(&pcurve, range).expect("reversible NURBS pcurve");
        for parameter in [range[0], 0.5, 1.0, 1.5, range[1]] {
            let expected =
                cadmpeg_ir::eval::pcurve_uv(&pcurve, range[0] + range[1] - parameter).unwrap();
            let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
            assert!((actual.u - expected.u).abs() < 1.0e-12);
            assert!((actual.v - expected.v).abs() < 1.0e-12);
        }
    }

    #[test]
    fn reversed_symmetric_analytic_pcurves_preserve_the_selected_interval() {
        let carriers = [
            PcurveGeometry::Ellipse {
                center: Point2::new(2.0, 3.0),
                x_axis: Point2::new(1.0, 0.0),
                y_axis: Point2::new(0.0, 1.0),
                major_radius: 4.0,
                minor_radius: 2.0,
            },
            PcurveGeometry::Parabola {
                vertex: Point2::new(2.0, 3.0),
                x_axis: Point2::new(1.0, 0.0),
                y_axis: Point2::new(0.0, 1.0),
                focal_distance: 0.75,
            },
            PcurveGeometry::Hyperbola {
                center: Point2::new(2.0, 3.0),
                x_axis: Point2::new(1.0, 0.0),
                y_axis: Point2::new(0.0, 1.0),
                major_radius: 4.0,
                minor_radius: 2.0,
            },
        ];
        let range = [-1.5, 1.5];
        for carrier in carriers {
            let reversed = super::reverse_pcurve_over_range(&carrier, range)
                .expect("symmetric analytic pcurve is exactly reversible");
            for parameter in [-1.5, -0.75, 0.0, 0.75, 1.5] {
                let expected = cadmpeg_ir::eval::pcurve_uv(&carrier, -parameter).unwrap();
                let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
                assert!((actual.u - expected.u).abs() < 1e-12);
                assert!((actual.v - expected.v).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn reversed_analytic_conics_preserve_arbitrary_selected_intervals() {
        let carriers = [
            PcurveGeometry::Ellipse {
                center: Point2::new(2.0, 3.0),
                x_axis: Point2::new(0.6, 0.8),
                y_axis: Point2::new(-0.8, 0.6),
                major_radius: 4.0,
                minor_radius: 2.0,
            },
            PcurveGeometry::Hyperbola {
                center: Point2::new(-3.0, 5.0),
                x_axis: Point2::new(0.8, -0.6),
                y_axis: Point2::new(0.6, 0.8),
                major_radius: 2.5,
                minor_radius: 1.25,
            },
        ];
        let range = [0.25, 1.75];
        for carrier in carriers {
            let reversed = super::reverse_pcurve_over_range(&carrier, range)
                .expect("a finite conic interval has an exact coefficient reflection");
            assert!(matches!(
                (&carrier, &reversed),
                (
                    PcurveGeometry::Ellipse { .. },
                    PcurveGeometry::Harmonic { .. }
                ) | (
                    PcurveGeometry::Hyperbola { .. },
                    PcurveGeometry::Hyperbolic { .. }
                )
            ));
            for parameter in [0.25, 0.5, 1.0, 1.5, 1.75] {
                let expected =
                    cadmpeg_ir::eval::pcurve_uv(&carrier, range[0] + range[1] - parameter).unwrap();
                let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
                assert!((actual.u - expected.u).abs() < 1e-12);
                assert!((actual.v - expected.v).abs() < 1e-12);
            }

            let reflected_twice = super::reverse_pcurve_over_range(&reversed, range)
                .expect("general conic coefficients remain exactly reversible");
            for parameter in [0.25, 0.75, 1.25, 1.75] {
                let expected = cadmpeg_ir::eval::pcurve_uv(&carrier, parameter).unwrap();
                let actual = cadmpeg_ir::eval::pcurve_uv(&reflected_twice, parameter).unwrap();
                assert!((actual.u - expected.u).abs() < 1e-12);
                assert!((actual.v - expected.v).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn reversed_parabola_preserves_an_arbitrary_selected_interval() {
        let pcurve = PcurveGeometry::Parabola {
            vertex: Point2::new(2.0, 3.0),
            x_axis: Point2::new(0.6, 0.8),
            y_axis: Point2::new(-0.8, 0.6),
            focal_distance: 0.75,
        };
        let range = [0.25, 2.75];
        let reversed = super::reverse_pcurve_over_range(&pcurve, range)
            .expect("a finite parabola interval has an exact quadratic reflection");
        assert!(matches!(
            &reversed,
            PcurveGeometry::Nurbs {
                degree: 2,
                weights: None,
                periodic: false,
                ..
            }
        ));
        for parameter in [0.25, 0.5, 1.0, 1.75, 2.5, 2.75] {
            let expected =
                cadmpeg_ir::eval::pcurve_uv(&pcurve, range[0] + range[1] - parameter).unwrap();
            let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
            assert!((actual.u - expected.u).abs() < 1e-12);
            assert!((actual.v - expected.v).abs() < 1e-12);
        }

        let offset = PcurveGeometry::Offset {
            distance: 1.25,
            basis: Box::new(pcurve.clone()),
        };
        let PcurveGeometry::Offset { distance, basis } =
            super::reverse_pcurve_over_range(&offset, range)
                .expect("offset parabola reflection closes recursively")
        else {
            panic!("reversed offset parabola");
        };
        assert_eq!(distance, -1.25);
        for parameter in [0.25, 1.0, 2.0, 2.75] {
            let expected =
                cadmpeg_ir::eval::pcurve_uv(&pcurve, range[0] + range[1] - parameter).unwrap();
            let actual = cadmpeg_ir::eval::pcurve_uv(&basis, parameter).unwrap();
            assert!((actual.u - expected.u).abs() < 1e-12);
            assert!((actual.v - expected.v).abs() < 1e-12);
        }
    }

    #[test]
    fn reversed_offset_pcurve_reverses_its_basis_and_signed_side() {
        let pcurve = PcurveGeometry::Offset {
            distance: 2.5,
            basis: Box::new(PcurveGeometry::Line {
                origin: Point2::new(1.0, 3.0),
                direction: Point2::new(2.0, -1.0),
            }),
        };
        let reversed = super::reverse_pcurve_over_range(&pcurve, [2.0, 6.0])
            .expect("offset construction is exactly reversible");
        let PcurveGeometry::Offset { distance, basis } = &reversed else {
            panic!("reversed offset");
        };
        assert_eq!(*distance, -2.5);
        for parameter in [2.0, 3.0, 5.0, 6.0] {
            let expected_basis = cadmpeg_ir::eval::pcurve_uv(
                match &pcurve {
                    PcurveGeometry::Offset { basis, .. } => basis,
                    _ => unreachable!(),
                },
                8.0 - parameter,
            )
            .unwrap();
            let actual = cadmpeg_ir::eval::pcurve_uv(&basis, parameter).unwrap();
            assert_eq!(actual, expected_basis);
            let expected = cadmpeg_ir::eval::pcurve_uv(&pcurve, 8.0 - parameter).unwrap();
            let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
            assert!((actual.u - expected.u).abs() < 1e-12);
            assert!((actual.v - expected.v).abs() < 1e-12);
        }

        let support = SurfaceId("nx:test:offset-orientation-support".into());
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        ir.model.surfaces.push(Surface {
            id: support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        let first = cadmpeg_ir::eval::pcurve_uv(&pcurve, 2.0).unwrap();
        let second = cadmpeg_ir::eval::pcurve_uv(&pcurve, 6.0).unwrap();
        let oriented = super::orient_tolerant_intersection_pcurve(
            &ir,
            &CurveId("nx:test:unused-orientation-curve".into()),
            &support,
            &pcurve,
            [2.0, 6.0],
            [
                Point3::new(second.u, second.v, 0.0),
                Point3::new(first.u, first.v, 0.0),
            ],
            1e-12,
        )
        .expect("offset endpoints select the reversed terminal branch");
        for parameter in [2.0, 3.0, 5.0, 6.0] {
            let expected = cadmpeg_ir::eval::pcurve_uv(&pcurve, 8.0 - parameter).unwrap();
            let actual = cadmpeg_ir::eval::pcurve_uv(&oriented, parameter).unwrap();
            assert!((actual.u - expected.u).abs() < 1e-12);
            assert!((actual.v - expected.v).abs() < 1e-12);
        }
    }

    #[test]
    fn closed_serialized_pcurve_uses_carrier_tangent_for_orientation() {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let curve = CurveId("nx:test:closed-orientation-curve".into());
        let support = SurfaceId("nx:test:closed-orientation-support".into());
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        });
        ir.model.surfaces.push(Surface {
            id: support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        let pcurve = PcurveGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            x_axis: Point2::new(1.0, 0.0),
            y_axis: Point2::new(0.0, 1.0),
            radius: 2.0,
        };
        let endpoint = Point3::new(2.0, 0.0, 0.0);

        let oriented = super::orient_tolerant_intersection_pcurve(
            &ir,
            &curve,
            &support,
            &pcurve,
            [0.0, std::f64::consts::TAU],
            [endpoint, endpoint],
            1.0e-12,
        )
        .expect("carrier tangent selects one closed-branch orientation");
        let uv = cadmpeg_ir::eval::pcurve_uv(&oriented, std::f64::consts::FRAC_PI_2).unwrap();
        assert!((uv.u - 0.0).abs() < 1.0e-12);
        assert!((uv.v - 2.0).abs() < 1.0e-12);
    }

    #[test]
    fn edge_incidence_uses_only_declared_tolerances_at_large_scale() {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let curve_id = CurveId("nx:test:curve#0".into());
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Nurbs(NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                weights: None,
                periodic: false,
            }),
            source_object: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: ProceduralCurveId("nx:test:intersection#0".into()),
            curve: curve_id.clone(),
            definition: ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext {
                    sides: [
                        IntcurveSupportSide {
                            surface: None,
                            pcurve: None,
                            pcurve_parameter_range: None,
                        },
                        IntcurveSupportSide {
                            surface: None,
                            pcurve: None,
                            pcurve_parameter_range: None,
                        },
                    ],
                    parameter_range: [0.0, 1.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                },
                discontinuity_flag: false,
            },
            cache_fit_tolerance: Some(2.0),
        });

        let start_point = PointId("nx:test:point#0".into());
        let end_point = PointId("nx:test:point#1".into());
        ir.model.points.extend([
            Point {
                id: start_point.clone(),
                position: Point3::new(0.0, 0.0, 1.0),
                source_object: None,
            },
            Point {
                id: end_point.clone(),
                position: Point3::new(1.0, 0.005, 1.0),
                source_object: None,
            },
        ]);
        let start = VertexId("nx:test:vertex#0".into());
        let end = VertexId("nx:test:vertex#1".into());
        ir.model.vertices.extend([
            Vertex {
                id: start.clone(),
                point: start_point,
                tolerance: None,
            },
            Vertex {
                id: end.clone(),
                point: end_point,
                tolerance: None,
            },
        ]);
        let edge = EdgeId("nx:test:edge#0".into());
        ir.model.edges.push(Edge {
            id: edge.clone(),
            curve: Some(curve_id.clone()),
            start: start.clone(),
            end: end.clone(),
            param_range: None,
            tolerance: None,
        });
        let support = SurfaceId("nx:test:surface-support#0".into());
        let surface = SurfaceId("nx:test:surface#0".into());
        let construction = ProceduralSurfaceId("nx:test:surface-offset#0".into());
        ir.model.surfaces.extend([
            Surface {
                id: support.clone(),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            },
            Surface {
                id: surface.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: construction.clone(),
                },
                source_object: None,
            },
        ]);
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: construction,
            surface: surface.clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support,
                distance: 1.0,
                u_sense: Some(0),
                v_sense: Some(0),
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        let pcurve = PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            weights: None,
            periodic: false,
        };

        assert!(super::orient_edge_range(&ir, &curve_id, [0.0, 1.0], &start, &end, None).is_none());
        assert!(!super::pcurve_matches_edge(
            &ir, &edge, &surface, &pcurve, None,
        ));
        assert!(super::pcurve_matches_edge(
            &ir,
            &edge,
            &surface,
            &pcurve,
            Some(0.01),
        ));
        let large_distance = super::point_distance(
            Point3::new(1.0e200, 1.0e200, 1.0e200),
            Point3::new(0.0, 0.0, 0.0),
        );
        assert!(large_distance.is_finite());
        assert!((large_distance / 1.0e200 - 3.0_f64.sqrt()).abs() < 1.0e-15);
    }

    #[test]
    fn boundary_coincidence_is_certified_between_uniform_samples() {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let surfaces = [
            SurfaceId("nx:test:surface#0".into()),
            SurfaceId("nx:test:surface#1".into()),
        ];
        let surface = || NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 0.01, 0.02, 1.0, 1.0],
            u_count: 2,
            v_count: 4,
            control_points: [0.0, 1.0]
                .into_iter()
                .flat_map(|y| {
                    [0.0, 0.1, 0.2, 10.0]
                        .into_iter()
                        .map(move |x| Point3::new(x, y, 0.0))
                })
                .collect(),
            weights: None,
            u_periodic: false,
            v_periodic: false,
        };
        ir.model.surfaces.extend([
            Surface {
                id: surfaces[0].clone(),
                geometry: SurfaceGeometry::Nurbs(surface()),
                source_object: None,
            },
            Surface {
                id: surfaces[1].clone(),
                geometry: SurfaceGeometry::Nurbs(surface()),
                source_object: None,
            },
        ]);
        let pcurve = PcurveGeometry::Line {
            origin: Point2::new(0.0, 0.0),
            direction: Point2::new(0.0, 1.0),
        };
        assert!(super::coincident_pcurve_pair(
            &ir,
            [&surfaces[0], &surfaces[1]],
            [&pcurve, &pcurve],
            [0.0, 1.0],
            0.1,
        ));

        let SurfaceGeometry::Nurbs(second) = &mut ir.model.surfaces[1].geometry else {
            unreachable!()
        };
        second.control_points[1].z = 1.0;
        assert!(!super::coincident_pcurve_pair(
            &ir,
            [&surfaces[0], &surfaces[1]],
            [&pcurve, &pcurve],
            [0.0, 1.0],
            0.1,
        ));
    }

    #[test]
    fn rational_pcurve_incidence_isolates_close_branches() {
        let weights = [1.0, 1.1, 0.9, 1.2, 1.0];
        let controls = [
            0.006_306_3,
            -0.029_213_45,
            0.095_295_133_333_333_34,
            -0.070_192_95,
            0.024_297_3,
        ]
        .into_iter()
        .zip(weights)
        .map(|(numerator, weight)| Point2::new(numerator / weight, 0.0))
        .collect::<Vec<_>>();
        let pcurve = PcurveGeometry::Nurbs {
            degree: 4,
            knots: vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            control_points: controls,
            weights: Some(weights.to_vec()),
            periodic: false,
        };
        let roots = super::closest_pcurve_parameters(&pcurve, Point2::new(0.0, 0.0), Some(0.11))
            .expect("complete homogeneous root isolation");

        assert_eq!(roots.len(), 4);
        for (actual, expected) in roots.iter().zip([0.1001, 0.1, 0.7, 0.9]) {
            assert!((actual - expected).abs() < 1.0e-8);
        }
    }

    #[test]
    fn rational_pcurve_closest_search_retains_close_global_branches() {
        let weights = [1.0, 1.1, 0.9, 1.2, 1.0];
        let control_points = [
            0.006_306_3,
            -0.029_213_45,
            0.095_295_133_333_333_34,
            -0.070_192_95,
            0.024_297_3,
        ]
        .into_iter()
        .zip(weights)
        .map(|(numerator, weight)| Point2::new(numerator / weight, 0.0))
        .collect();
        let pcurve = PcurveGeometry::Nurbs {
            degree: 4,
            knots: vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            control_points,
            weights: Some(weights.to_vec()),
            periodic: false,
        };
        let parameters =
            super::closest_pcurve_parameters(&pcurve, Point2::new(0.0, 1.0e-4), Some(0.11))
                .expect("complete global closest-point search");

        assert_eq!(parameters.len(), 4, "{parameters:?}");
        for (actual, expected) in parameters.iter().zip([0.1001, 0.1, 0.7, 0.9]) {
            assert!((actual - expected).abs() < 1.0e-8);
        }
    }

    #[test]
    fn rational_spine_closest_search_resolves_close_global_branches() {
        let weights = [1.0, 1.1, 0.9, 1.2, 1.0];
        let control_points = [
            0.006_306_3,
            -0.029_213_45,
            0.095_295_133_333_333_34,
            -0.070_192_95,
            0.024_297_3,
        ]
        .into_iter()
        .zip(weights)
        .map(|(numerator, weight)| Point3::new(numerator / weight, 0.0, 0.0))
        .collect();
        let curve = NurbsCurve {
            degree: 4,
            knots: vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            control_points,
            weights: Some(weights.to_vec()),
            periodic: false,
        };
        let point = Point3::new(0.0, 1.0e-4, 0.0);

        let first = super::closest_nurbs_curve_parameter(&curve, point, Some(0.099))
            .expect("first close branch");
        let second = super::closest_nurbs_curve_parameter(&curve, point, Some(0.101))
            .expect("second close branch");
        let remote = super::closest_nurbs_curve_parameter(&curve, point, Some(0.69))
            .expect("remote global branch");

        assert!((first - 0.1).abs() < 1.0e-8);
        assert!((second - 0.1001).abs() < 1.0e-8);
        assert!((remote - 0.7).abs() < 1.0e-8);
    }

    #[test]
    fn periodic_nurbs_inversion_lifts_the_continuation_phase() {
        let knots = vec![0.0, 0.0, 1.0, 2.0, 2.0];
        let pcurve = PcurveGeometry::Nurbs {
            degree: 1,
            knots: knots.clone(),
            control_points: vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 0.0),
            ],
            weights: None,
            periodic: true,
        };
        let curve = NurbsCurve {
            degree: 1,
            knots,
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 0.0, 0.0),
            ],
            weights: None,
            periodic: true,
        };

        assert_eq!(
            super::closest_pcurve_parameters(&pcurve, Point2::new(0.0, 0.0), Some(4.1))
                .expect("periodic pcurve phase"),
            [4.0]
        );
        assert_eq!(
            super::closest_nurbs_curve_parameter(&curve, Point3::new(0.0, 0.0, 0.0), Some(4.1),)
                .expect("periodic curve phase"),
            4.0
        );
    }

    #[test]
    fn polynomial_root_isolation_retains_repeated_real_roots() {
        let roots = super::real_polynomial_roots(&[-1.0, 3.5, -3.0, -0.5, 1.0])
            .expect("finite quartic roots");

        assert_eq!(roots.len(), 3);
        for (actual, expected) in roots.iter().zip([-2.0, 0.5, 1.0]) {
            assert!((actual - expected).abs() < 1.0e-10, "{actual}");
        }
    }

    #[test]
    fn coincident_pcurve_interval_retains_seed_and_boundaries() {
        let pcurve = PcurveGeometry::Nurbs {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![Point2::new(2.0, -3.0); 3],
            weights: None,
            periodic: false,
        };
        let roots = super::closest_pcurve_parameters(&pcurve, Point2::new(2.0, -3.0), Some(0.3))
            .expect("coincident interval");

        assert_eq!(roots, [0.3, 0.0, 1.0]);
    }

    #[test]
    fn pcurve_bezier_extraction_preserves_rational_knot_spans() {
        let knots = [0.0, 0.0, 0.0, 0.25, 0.75, 1.0, 1.0, 1.0];
        let points = [
            Point2::new(-1.0, 0.0),
            Point2::new(0.0, 2.0),
            Point2::new(1.0, -1.0),
            Point2::new(2.0, 3.0),
            Point2::new(4.0, 0.0),
        ];
        let weights = [1.0, 1.5, 0.75, 2.0, 1.25];
        let controls = points
            .iter()
            .zip(weights)
            .map(|(point, weight)| [point.u * weight, point.v * weight, weight])
            .collect();
        let spans = super::bezier_spans(2, &knots, controls).expect("valid Bézier extraction");

        assert_eq!(spans.len(), 3);
        for span in spans {
            for fraction in [0.0, 0.5, 1.0] {
                let parameter = span.domain[0] + fraction * (span.domain[1] - span.domain[0]);
                let expected = cadmpeg_ir::eval::nurbs_pcurve_uv(
                    2,
                    &knots,
                    &points,
                    Some(&weights),
                    parameter,
                )
                .expect("source NURBS evaluation");
                let actual =
                    super::homogeneous_residual_distance(&span.controls, parameter, span.domain);
                assert!((actual - expected.u.hypot(expected.v)).abs() < 1.0e-12);
            }
        }
    }
}
