// SPDX-License-Identifier: Apache-2.0
//! Geometric consistency checks: evaluated carrier geometry must land on the
//! topology it supports.

use crate::document::CadIr;
use crate::eval::curve_parameter_near_point;
use crate::eval::model_curve_point_by_id;
use crate::eval::model_surface_partials_by_id;
use crate::eval::model_surface_point_by_id;
use crate::eval::nurbs_pcurve_parameter_domain;
use crate::eval::pcurve_tangent;
use crate::eval::EvaluationFailure;
use crate::features::FinitePoint3;
use crate::geometry::{
    pcurve::PcurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use crate::math::{Point3, Vector3};
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::scalar::{ExtendedReal, FiniteReal};
use crate::topology::{ParameterInterval, Sense};

use crate::units::COINCIDENCE_TOLERANCE;
use cadmpeg_core::decode::{DecodeContext, ResourceLimit};
use cadmpeg_core::CodecError;

use super::pcurve_parameter_domain;
use crate::index::identities::BorrowedIdentities;
use super::scratch::Scratch;

/// A curve point as the checks measure it: the finite point, or the point an
/// evaluation outside the finite range reached, whose mismatch is then the
/// finding's measure. An evaluation with no value has no point.
fn measured_point(
    evaluation: Result<FinitePoint3, EvaluationFailure<Point3>>,
) -> Result<Option<Point3>, ResourceLimit> {
    match evaluation {
        Ok(point) => Ok(Some(point.get())),
        Err(failure) => failure.non_finite(),
    }
}

/// The worse of two endpoint mismatches. A mismatch that is NaN states no
/// comparison, and is the measure: `f64::max` would drop it.
fn worse_mismatch(first: f64, second: f64) -> f64 {
    if first.is_nan() || second.is_nan() {
        f64::NAN
    } else {
        first.max(second)
    }
}

/// The coincidence allowance combines the document-wide uncertainty with any
/// stored edge, vertex, face, or carrier tolerances.
fn allowance(document_tolerance: crate::scalar::PositiveLength, tolerances: &[Option<f64>]) -> f64 {
    tolerances.iter().flatten().copied().fold(
        COINCIDENCE_TOLERANCE.max(document_tolerance.get()),
        f64::max,
    )
}

/// Two independently evaluated procedural carriers can each consume the
/// baseline coincidence allowance. The solved cache's explicit fit tolerance
/// widens that allowance when it is larger.
fn procedural_support_allowance(
    document_tolerance: crate::scalar::PositiveLength,
    cache_fit_tolerance: Option<f64>,
) -> f64 {
    COINCIDENCE_TOLERANCE + allowance(document_tolerance, &[cache_fit_tolerance])
}

/// Embedded support pcurves must map through their surfaces onto the curve
/// they constrain at both ends of the construction interval.
pub(super) fn check_procedural_support_consistency(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let index = crate::index::ModelIndex::new(ir);
    let curves = BorrowedIdentities::build(ctx, |add| {
        for curve in &ir.model.curves { add(curve.id.as_str(), &curve.geometry)?; }
        Ok(())
    })?;
    let owners = curve_owners(ctx, ir)?;
    for procedural in &ir.model.procedural_curves {
        ctx.charge_work(1, "geometric procedural curve scan")?;
        let Some(owner) = owners.get_unique(ctx, procedural.id.as_str())?.copied() else {
            continue;
        };
        if let crate::geometry::ProceduralCurveDefinition::TolerantIntersection {
            construction: intersection,
            parameterization: Some(parameterization),
            ..
        } = procedural.definition()
        {
            let endpoints = intersection.endpoints();
            let tolerance = intersection.tolerance().get();

            let evaluated = parameterization
                .parameter_range()
                .endpoints()
                .map(|parameter| measured_point(model_curve_point_by_id(&index, owner, parameter)));
            let [Some(start), Some(end)] = [evaluated[0]?, evaluated[1]?] else {
                super::record_finding(ctx, findings, Check::GeometricConsistency, Severity::Error, Some(procedural.id.as_str()), format_args!("charted tolerant intersection does not evaluate at both endpoints"))?;
                continue;
            };
            let mismatch = worse_mismatch(
                Point3::distance(start, endpoints[0].get()),
                Point3::distance(end, endpoints[1].get()),
            );
            if !mismatch.is_finite() || mismatch > tolerance {
                super::record_finding(ctx, findings, Check::GeometricConsistency, Severity::Error, Some(procedural.id.as_str()), format_args!(
                        "charted tolerant intersection misses its endpoint witnesses by \
                         {mismatch:.6}"
                    ))?;
            }
            continue;
        }
        if let crate::geometry::ProceduralCurveDefinition::SurfaceOffset(definition_payload) =
            procedural.definition()
        {
            let context = definition_payload.context();
            let base = definition_payload.base();
            let base_endpoints = definition_payload.base_endpoints();
            let offset = definition_payload.distance().get();
            let Some(solved) = curves.get(ctx, owner.as_str())? else {
                continue;
            };
            let solved = context
                .parameter_range()
                .endpoints()
                .map(|parameter| measured_point(crate::eval::decode::curve_point(ctx, solved, parameter)));
            let [Some(solved_start), Some(solved_end)] = [solved[0]?, solved[1]?] else {
                continue;
            };
            let bound = procedural_support_allowance(
                ir.tolerances.linear,
                procedural
                    .cache_fit_tolerance()
                    .map(crate::geometry::FitTolerance::get),
            );
            let Some(base) = curves.get(ctx, base.as_str())? else {
                continue;
            };
            let base = base_endpoints.map(|parameter| match parameter {
                Some(parameter) => measured_point(crate::eval::decode::curve_point(ctx, base, parameter.get())),
                None => Ok(None),
            });
            let [Some(base_start), Some(base_end)] = [base[0]?, base[1]?] else {
                check_support_sides(
                    ctx,
                    context,
                    None,
                    SupportEndpointContract::Offset {
                        endpoints: [solved_start, solved_end],
                        distance: offset.abs(),
                    },
                    &index,
                    bound,
                    procedural.id.as_str(),
                    findings,
                )?;
                continue;
            };
            let offset_mismatch = worse_mismatch(
                (Point3::distance(solved_start, base_start) - offset.abs()).abs(),
                (Point3::distance(solved_end, base_end) - offset.abs()).abs(),
            );
            if !offset_mismatch.is_finite() || offset_mismatch > bound {
                super::record_finding(ctx, findings, Check::GeometricConsistency, Severity::Error, Some(procedural.id.as_str()), format_args!(
                        "surface-offset solved curve misses its base offset distance by \
                         {offset_mismatch:.6}"
                    ))?;
            }
            check_support_sides(
                ctx,
                context,
                None,
                SupportEndpointContract::Coincident([base_start, base_end]),
                &index,
                bound,
                procedural.id.as_str(),
                findings,
            )?;
            continue;
        }
        let (context, third) = match procedural.definition() {
            crate::geometry::ProceduralCurveDefinition::Law { context, .. } => {
                (std::borrow::Cow::Borrowed(context), None)
            }
            crate::geometry::ProceduralCurveDefinition::Intersection { context, .. } => {
                (std::borrow::Cow::Borrowed(context), None)
            }
            crate::geometry::ProceduralCurveDefinition::Silhouette(definition_payload) => (
                std::borrow::Cow::Borrowed(definition_payload.context()),
                None,
            ),
            crate::geometry::ProceduralCurveDefinition::Projection(definition_payload) => {
                let context = definition_payload.context();

                (std::borrow::Cow::Borrowed(context), None)
            }
            crate::geometry::ProceduralCurveDefinition::TwoSidedOffset(definition_payload) => {
                let context = definition_payload.context();
                {
                    (std::borrow::Cow::Borrowed(context), None)
                }
            }
            crate::geometry::ProceduralCurveDefinition::SurfaceCurve { family } => {
                (std::borrow::Cow::Borrowed(family.context()), None)
            }
            crate::geometry::ProceduralCurveDefinition::Spring(definition_payload) => (
                std::borrow::Cow::Borrowed(definition_payload.support_context()),
                None,
            ),
            crate::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection(
                definition_payload,
            ) => {
                let context = definition_payload.context();
                let third = definition_payload.third();
                (std::borrow::Cow::Borrowed(context), Some(third))
            }
            _ => continue,
        };
        let Some(curve) = curves.get(ctx, owner.as_str())? else {
            continue;
        };
        let solved = context
            .parameter_range()
            .endpoints()
            .map(|parameter| measured_point(crate::eval::decode::curve_point(ctx, curve, parameter)));
        let [Some(solved_start), Some(solved_end)] = [solved[0]?, solved[1]?] else {
            continue;
        };
        let bound = procedural_support_allowance(
            ir.tolerances.linear,
            procedural
                .cache_fit_tolerance()
                .map(crate::geometry::FitTolerance::get),
        );
        check_support_sides(
            ctx,
            &context,
            third,
            SupportEndpointContract::Coincident([solved_start, solved_end]),
            &index,
            bound,
            procedural.id.as_str(),
            findings,
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum SupportEndpointContract {
    Coincident([Point3; 2]),
    Offset {
        endpoints: [Point3; 2],
        distance: f64,
    },
}

fn check_support_sides(
    ctx: &DecodeContext<'_>,
    context: &crate::geometry::IntcurveSupportContext,
    third: Option<&crate::geometry::IntcurveSupportSide>,
    contract: SupportEndpointContract,
    index: &crate::index::ModelIndex<'_>,
    bound: f64,
    entity: &str,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let (constrained, expected_distance) = match contract {
        SupportEndpointContract::Coincident(endpoints) => (endpoints, None),
        SupportEndpointContract::Offset {
            endpoints,
            distance,
        } => (endpoints, Some(distance)),
    };
    for (side_index, side) in context.sides().iter().chain(third).enumerate() {
        ctx.charge_work(1, "geometric support side scan")?;
        let (Some(surface_id), Some(pcurve)) = (&side.surface, &side.pcurve) else {
            continue;
        };
        // A non-finite pcurve or support point is measured as a finite one
        // is: the mismatch it produces is the finding's measure.
        let support = context.parameter_range().endpoints().map(
            |parameter| -> Result<Option<Point3>, ResourceLimit> {
                let Some(parameter) = side.pcurve_parameter(context.parameter_range(), parameter)
                else {
                    return Ok(None);
                };
                let uv = match crate::eval::decode::pcurve_uv(ctx, &pcurve.geometry, parameter.get()) {
                    Ok(uv) => uv.get(),
                    Err(failure) => {
                        let Some(uv) = failure.non_finite()? else {
                            return Ok(None);
                        };
                        uv
                    }
                };
                match model_surface_point_by_id(index, surface_id, uv.u, uv.v) {
                    Ok(point) => Ok(Some(point.get())),
                    Err(failure) => failure.non_finite(),
                }
            },
        );
        let [Some(support_start), Some(support_end)] = [support[0]?, support[1]?] else {
            continue;
        };
        let endpoint_mismatch = |constrained, support| {
            let distance = Point3::distance(constrained, support);
            expected_distance.map_or(distance, |expected| (distance - expected).abs())
        };
        let mismatch = worse_mismatch(
            endpoint_mismatch(constrained[0], support_start),
            endpoint_mismatch(constrained[1], support_end),
        );
        if !mismatch.is_finite() || mismatch > bound {
            super::record_finding(ctx, findings, Check::GeometricConsistency, Severity::Error, Some(entity), format_args!(
                    "procedural support side {side_index} misses its endpoint distance contract by \
                     {mismatch:.6}"
                ))?;
        }
    }
    Ok(())
}

fn curve_owners<'ctx, 'ir>(ctx: &'ctx DecodeContext<'_>, ir: &'ir CadIr) -> Result<BorrowedIdentities<'ctx, 'ir, &'ir crate::ids::CurveId>, CodecError> {
    BorrowedIdentities::build(ctx, |add| {
        for curve in &ir.model.curves {
            ctx.charge_work(1, "geometric curve owner scan")?;
            if let Some(construction) = curve.geometry.procedural_construction() {
                add(construction.as_str(), &curve.id)?;
            }
        }
        Ok(())
    })
}

fn vertex_positions<'ctx, 'ir>(ctx: &'ctx DecodeContext<'_>, ir: &'ir CadIr) -> Result<BorrowedIdentities<'ctx, 'ir, (Point3, Option<f64>)>, CodecError> {
    let points = BorrowedIdentities::build(ctx, |add| {
        for point in &ir.model.points { add(point.id.as_str(), point.position().get())?; }
        Ok(())
    })?;
    BorrowedIdentities::build(ctx, |add| {
        for vertex in &ir.model.vertices {
            ctx.charge_work(1, "geometric vertex position scan")?;
            if let Some(position) = points.get(ctx, vertex.point.as_str())? {
                add(vertex.id.as_str(), (*position, vertex.tolerance.map(crate::scalar::PositiveReal::get)))?;
            }
        }
        Ok(())
    })
}

/// An edge's curve evaluated at its parameter range must land on the edge's
/// start and end vertex positions within the topology tolerances or the
/// evaluated curve cache's fit tolerance.
pub(super) fn check_edge_endpoint_consistency(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let curves = BorrowedIdentities::build(ctx, |add| {
        for curve in &ir.model.curves { add(curve.id.as_str(), &curve.geometry)?; }
        Ok(())
    })?;
    let owners = curve_owners(ctx, ir)?;
    let curve_cache_tolerances = BorrowedIdentities::build(ctx, |add| {
        for curve in &ir.model.procedural_curves {
            ctx.charge_work(1, "geometric curve cache scan")?;
            if let Some(owner) = owners.get_unique(ctx, curve.id.as_str())? {
                add(owner.as_str(), curve.cache_fit_tolerance())?;
            }
        }
        Ok(())
    })?;
    let vertices = vertex_positions(ctx, ir)?;
    for edge in &ir.model.edges {
        ctx.charge_work(1, "geometric edge scan")?;
        let Some([start_t, end_t]) = edge.param_range().map(crate::units::FiniteVector::get) else {
            continue;
        };
        let Some(curve_id) = edge.curve() else { continue; };
        let Some(geometry) = curves.get(ctx, curve_id.as_str())? else { continue; };
        let (Some((start, start_tol)), Some((end, end_tol))) = (
            vertices.get(ctx, edge.start.as_str())?,
            vertices.get(ctx, edge.end.as_str())?,
        ) else {
            continue;
        };
        let (Some(at_start), Some(at_end)) = (
            measured_point(crate::eval::decode::curve_point(ctx, geometry, start_t))?,
            measured_point(crate::eval::decode::curve_point(ctx, geometry, end_t))?,
        ) else {
            continue;
        };
        let bound = allowance(
            ir.tolerances.linear,
            &[
                edge.tolerance.map(crate::scalar::PositiveReal::get),
                *start_tol,
                *end_tol,
                curve_cache_tolerances
                    .get(ctx, curve_id.as_str())?
                    .copied()
                    .flatten()
                    .map(crate::geometry::FitTolerance::get),
            ],
        );
        let mismatch = worse_mismatch(
            Point3::distance(at_start, *start),
            Point3::distance(at_end, *end),
        );
        if !mismatch.is_finite() || mismatch > bound {
            super::record_finding(ctx, findings, Check::GeometricConsistency, Severity::Error, Some(edge.id.as_str()), format_args!(
                    "edge curve endpoints miss the edge's vertex positions by {mismatch:.6}"
                ))?;
        }
    }
    let edges = BorrowedIdentities::build(ctx, |add| {
        for edge in &ir.model.edges { add(edge.id.as_str(), edge)?; }
        Ok(())
    })?;
    for coedge in &ir.model.coedges {
        ctx.charge_work(1, "geometric coedge scan")?;
        let Some(use_curve) = &coedge.use_curve else {
            continue;
        };
        let [start_t, end_t] = use_curve.parameter_range.endpoints();
        let curve_id = &use_curve.curve;
        let Some(geometry) = curves.get(ctx, curve_id.as_str())? else {
            continue;
        };
        let Some(edge) = edges.get(ctx, coedge.edge.as_str())? else {
            continue;
        };
        let (first_vertex, last_vertex) = match coedge.sense {
            Sense::Forward => (&edge.start, &edge.end),
            Sense::Reversed => (&edge.end, &edge.start),
        };
        let (Some((start, start_tol)), Some((end, end_tol))) = (
            vertices.get(ctx, first_vertex.as_str())?,
            vertices.get(ctx, last_vertex.as_str())?,
        ) else {
            continue;
        };
        let (Some(at_start), Some(at_end)) = (
            measured_point(crate::eval::decode::curve_point(ctx, geometry, start_t))?,
            measured_point(crate::eval::decode::curve_point(ctx, geometry, end_t))?,
        ) else {
            continue;
        };
        let bound = allowance(
            ir.tolerances.linear,
            &[
                edge.tolerance.map(crate::scalar::PositiveReal::get),
                *start_tol,
                *end_tol,
                curve_cache_tolerances
                    .get(ctx, curve_id.as_str())?
                    .copied()
                    .flatten()
                    .map(crate::geometry::FitTolerance::get),
            ],
        );
        let mismatch = worse_mismatch(
            Point3::distance(at_start, *start),
            Point3::distance(at_end, *end),
        );
        if !mismatch.is_finite() || mismatch > bound {
            super::record_finding(ctx, findings, Check::GeometricConsistency, Severity::Error, Some(coedge.id.as_str()), format_args!(
                    "coedge use-curve endpoints miss the traversal vertices by {mismatch:.6}"
                ))?;
        }
    }
    Ok(())
}

/// A coedge's pcurve, mapped through its face's surface, must land on the
/// owning edge's vertex positions over the edge's parameter interval within
/// the topology tolerances or the evaluated pcurve carriers' fit tolerances.
/// Pcurve parameter sign and direction are independent of edge sense, so
/// either sign and either endpoint assignment satisfy the check.
pub(super) fn check_pcurve_surface_consistency(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let index = crate::index::ModelIndex::new(ir);
    let curves = BorrowedIdentities::build(ctx, |add| {
        for curve in &ir.model.curves { add(curve.id.as_str(), &curve.geometry)?; }
        Ok(())
    })?;
    let surfaces = BorrowedIdentities::build(ctx, |add| {
        for surface in &ir.model.surfaces { add(surface.id.as_str(), &surface.geometry)?; }
        Ok(())
    })?;
    let surface_owners = BorrowedIdentities::build(ctx, |add| {
        for surface in &ir.model.surfaces {
            ctx.charge_work(1, "geometric surface owner scan")?;
            if let Some(construction) = surface.geometry.procedural_construction() {
                add(construction.as_str(), &surface.id)?;
            }
        }
        Ok(())
    })?;
    let procedurally_parameterized_surfaces = BorrowedIdentities::build(ctx, |add| {
        for surface in &ir.model.procedural_surfaces {
            ctx.charge_work(1, "geometric procedural surface scan")?;
            if !matches!(surface.definition(), crate::geometry::ProceduralSurfaceDefinition::Subset(_)) {
                if let Some(owner) = surface_owners.get_unique(ctx, surface.id.as_str())? {
                    add(owner.as_str(), ())?;
                }
            }
        }
        Ok(())
    })?;
    let pcurves = BorrowedIdentities::build(ctx, |add| {
        for pcurve in &ir.model.pcurves { add(pcurve.id.as_str(), pcurve)?; }
        Ok(())
    })?;
    let edges = BorrowedIdentities::build(ctx, |add| {
        for edge in &ir.model.edges { add(edge.id.as_str(), edge)?; }
        Ok(())
    })?;
    let faces = BorrowedIdentities::build(ctx, |add| {
        for face in &ir.model.faces { add(face.id.as_str(), face)?; }
        Ok(())
    })?;
    let loops = BorrowedIdentities::build(ctx, |add| {
        for lp in &ir.model.loops { add(lp.id.as_str(), lp)?; }
        Ok(())
    })?;
    let vertices = vertex_positions(ctx, ir)?;

    for coedge in &ir.model.coedges {
        ctx.charge_work(1, "geometric coedge scan")?;
        let Some((first_use, last_use)) = coedge.pcurves.first().zip(coedge.pcurves.last()) else {
            continue;
        };
        let (Some(first), Some(last)) = (
            pcurves.get(ctx, first_use.pcurve.as_str())?,
            pcurves.get(ctx, last_use.pcurve.as_str())?,
        ) else {
            continue;
        };
        let face = match loops.get(ctx, coedge.owner_loop.as_str())? {
            Some(lp) => faces.get(ctx, lp.face.as_str())?,
            None => None,
        };
        let Some(face) = face else { continue; };
        let Some(geometry) = surfaces.get(ctx, face.surface.as_str())? else {
            continue;
        };
        // A procedural construction defines its own UV space. Its solved
        // surface is a model-space cache, not the carrier of that UV
        // parameterization, so mapping the pcurve through the cache is not a
        // valid consistency test.
        if procedurally_parameterized_surfaces.contains(ctx, face.surface.as_str())? {
            continue;
        }
        let Some(edge) = edges.get(ctx, coedge.edge.as_str())? else {
            continue;
        };
        let (Some((start, start_tol)), Some((end, end_tol))) = (
            vertices.get(ctx, edge.start.as_str())?,
            vertices.get(ctx, edge.end.as_str())?,
        ) else {
            continue;
        };
        // A single parameter-space image is checked over its candidate
        // intervals, honoring an opposite-sign parameterization and a stored
        // range. Multiple images are checked from the first image's start
        // extreme to the last image's end extreme.
        let curve_geometry = match edge.curve() {
            Some(curve) => curves.get(ctx, curve.as_str())?.copied(),
            None => None,
        };
        let bound = allowance(
            ir.tolerances.linear,
            &[
                edge.tolerance.map(crate::scalar::PositiveReal::get),
                *start_tol,
                *end_tol,
                face.tolerance.map(crate::scalar::PositiveReal::get),
                first
                    .fit_tolerance()
                    .map(crate::geometry::FitTolerance::get),
                last.fit_tolerance().map(crate::geometry::FitTolerance::get),
            ],
        );
        // Recovering an occurrence interval is a topological operation. A
        // carrier fit tolerance may qualify the final image, but must not let
        // inverse recovery move an explicitly ranged occurrence onto a
        // different, merely nearby part of the carrier.
        let recovery_bound = allowance(
            ir.tolerances.linear,
            &[
                edge.tolerance.map(crate::scalar::PositiveReal::get),
                *start_tol,
                *end_tol,
                face.tolerance.map(crate::scalar::PositiveReal::get),
            ],
        );
        // A malformed STEP export can retain a stale TRIMMED_CURVE interval
        // even though its carrier still reaches the edge vertices on another
        // interval. Keep the declared interval as a candidate, but also solve
        // the mapped carrier against the topology endpoints whenever the
        // carrier can provide such a witness.
        let surface_context = SurfacePcurveContext {
            index: &index,
            surface_id: &face.surface,
            geometry,
        };
        let recovered = edge_pcurve_parameter_ranges(ctx,
            &surface_context,
            curve_geometry,
            *start,
            *end,
            first,
            last,
            recovery_bound,
        )?;
        let declared = if coedge.pcurves.len() == 1 {
            pcurve_parameter_ranges(
                ctx,
                first,
                first_use
                    .parameter_range
                    .map(crate::geometry::DirectedParameterRange::endpoints),
                edge.param_range().map(crate::units::FiniteVector::get),
            )?
        } else {
            let first_range = first_use.parameter_range
                .map(crate::geometry::DirectedParameterRange::endpoints)
                .or(first.parameter_range().map(crate::units::FiniteVector::get));
            let first_range = match first_range {
                Some(range) => Some(range),
                None => pcurve_parameter_extremes(ctx, first)?,
            };
            let last_range = last_use.parameter_range
                .map(crate::geometry::DirectedParameterRange::endpoints)
                .or(last.parameter_range().map(crate::units::FiniteVector::get));
            let last_range = match last_range {
                Some(range) => Some(range),
                None => pcurve_parameter_extremes(ctx, last)?,
            };
            match (first_range, last_range) {
                (Some([t0, _]), Some([_, t1])) => {
                    let mut range = Scratch::new(ctx)?;
                    range.push([t0, t1])?;
                    Some(range)
                },
                _ => None,
            }
        };
        let mut minimum_mismatch: Option<f64> = None;
        for [t0, t1] in declared.iter().flat_map(|ranges| ranges.iter())
            .chain(recovered.iter().flat_map(|ranges| ranges.iter())).copied() {
            ctx.charge_work(1, "pcurve interval candidate")?;
            // A non-finite pcurve or surface point is measured as a finite
            // one is: the distance it produces is the finding's measure.
            let pcurve_point = |geometry, parameter| match crate::eval::decode::pcurve_uv(ctx, geometry, parameter) {
                Ok(uv) => Ok(Some(uv.get())),
                Err(failure) => failure.non_finite(),
            };
            let surface_point = |uv: crate::math::Point2| match model_surface_point_by_id(
                &index,
                &face.surface,
                uv.u,
                uv.v,
            ) {
                Ok(point) => Ok(Some(point.get())),
                Err(failure) => failure.non_finite(),
            };
            let (Some(uv0), Some(uv1)) = (
                pcurve_point(&first.geometry, t0)?,
                pcurve_point(&last.geometry, t1)?,
            ) else {
                continue;
            };
            let (Some(p0), Some(p1)) = (surface_point(uv0)?, surface_point(uv1)?) else {
                continue;
            };
            let forward = worse_mismatch(Point3::distance(p0, *start), Point3::distance(p1, *end));
            let reversed = worse_mismatch(Point3::distance(p0, *end), Point3::distance(p1, *start));
            let mismatch = forward.min(reversed);
            minimum_mismatch =
                Some(minimum_mismatch.map_or(mismatch, |minimum| minimum.min(mismatch)));
        }
        let Some(mismatch) = minimum_mismatch else {
            continue;
        };
        if !mismatch.is_finite() || mismatch > bound {
            super::record_finding(ctx, findings, Check::GeometricConsistency, Severity::Error, Some(coedge.id.as_str()), format_args!(
                    "pcurve mapped through the face surface misses the edge's vertex positions \
                     by {mismatch:.6}"
                ))?;
        }
    }
    Ok(())
}

/// Candidate pcurve intervals for an edge. Native pcurves can parameterize the
/// same edge with the opposite sign, and a stored use interval can wrap a
/// periodic pcurve's seam, so no single interval is authoritative. The stored
/// range and the edge interval (in either sign) are candidates; the check takes
/// the closest image. An untrimmed carrier domain is not an edge interval: the
/// STEP edge may select any sub-interval of that carrier through its vertices.
/// Such an interval is recovered independently from the shared 3D curve by
/// `edge_pcurve_parameter_ranges`.
fn pcurve_parameter_ranges<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    pcurve: &crate::geometry::pcurve::Pcurve,
    pcurve_range: Option<[f64; 2]>,
    edge_range: Option<[f64; 2]>,
) -> Result<Option<Scratch<'ctx, [f64; 2]>>, CodecError> {
    let mut ranges = Scratch::new(ctx)?;
    if let Some(range) = pcurve_range.or(pcurve
        .parameter_range()
        .map(crate::units::FiniteVector::get))
    {
        ranges.push(range)?;
    }
    if let Some([start, end]) = edge_range {
        ranges.extend([[start, end], [-start, -end]])?;
    }
    ranges.extend(pcurve_parameter_extremes(ctx, pcurve)?)?;
    if !ranges.is_empty() {
        if let Some(domain) = pcurve_parameter_domain(ctx, &pcurve.geometry)? {
            ranges.push(domain.endpoints())?;
        }
    }
    Ok((!ranges.is_empty()).then_some(ranges))
}

struct SurfacePcurveContext<'index, 'model> {
    index: &'index crate::index::ModelIndex<'model>,
    surface_id: &'index crate::ids::SurfaceId,
    geometry: &'index SurfaceGeometry,
}

/// Recover an edge interval from its mapped pcurve when the STEP topology does
/// not carry a usable parameter range. A declared range remains a candidate,
/// but malformed exports can retain a stale range while the carrier still
/// reaches the edge vertices on another interval. The 3D and surface-space
/// carriers can use different native parameterizations, so solve the mapped
/// pcurve at each vertex instead of copying a parameter from one carrier to
/// the other. `tolerance` is the topology allowance used for this inverse;
/// carrier fit tolerances are applied only after recovery. A direct conic
/// solve remains as a fallback for surfaces without a usable mapped inverse.
/// Several seeds preserve the correct branch for periodic carriers.
fn edge_pcurve_parameter_ranges<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    context: &SurfacePcurveContext<'_, '_>,
    curve_geometry: Option<&crate::geometry::CurveGeometry>,
    start: Point3,
    end: Point3,
    first: &crate::geometry::pcurve::Pcurve,
    last: &crate::geometry::pcurve::Pcurve,
    tolerance: f64,
) -> Result<Option<Scratch<'ctx, [f64; 2]>>, CodecError> {
    let mut start_parameters = Scratch::new(ctx)?;
    for seed in pcurve_parameter_seeds_on_surface(ctx, context, first)? {
        ctx.charge_work(1, "mapped pcurve start seed")?;
        if let Some(parameter) = mapped_pcurve_parameter_near_point(ctx, context, &first.geometry, start, seed, tolerance)? {
            start_parameters.push(parameter)?;
        }
    }
    let start_parameters = unique(ctx, start_parameters, Some)?;
    let mut end_parameters = Scratch::new(ctx)?;
    for seed in pcurve_parameter_seeds_on_surface(ctx, context, last)? {
        ctx.charge_work(1, "mapped pcurve end seed")?;
        if let Some(parameter) = mapped_pcurve_parameter_near_point(ctx, context, &last.geometry, end, seed, tolerance)? {
            end_parameters.push(parameter)?;
        }
    }
    let end_parameters = unique(ctx, end_parameters, Some)?;
    let ranges = parameter_pairs(ctx, &start_parameters, &end_parameters)?;
    if !ranges.is_empty() { return Ok(Some(ranges)); }
    drop(ranges);
    drop(start_parameters);
    drop(end_parameters);
    let Some(curve_geometry) = curve_geometry else { return Ok(None); };
    if !matches!(curve_geometry, crate::geometry::CurveGeometry::Solved(
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_)
        | SolvedCurveGeometry::Parabola(_) | SolvedCurveGeometry::Hyperbola(_))) {
        return Ok(None);
    }
    let mut seeds = pcurve_parameter_seeds_on_surface(ctx, context, first)?;
    seeds.extend(pcurve_parameter_seeds_on_surface(ctx, context, last)?)?;
    let mut start_parameters = Scratch::new(ctx)?;
    for seed in seeds.iter() {
        ctx.charge_work(1, "conic pcurve start seed")?;
        if let Some(parameter) = curve_parameter_near_point(ctx, curve_geometry, start, seed.get(), tolerance)? {
            start_parameters.push(parameter)?;
        }
    }
    let start_parameters = unique(ctx, start_parameters, Some)?;
    let mut end_parameters = Scratch::new(ctx)?;
    for seed in seeds.iter() {
        ctx.charge_work(1, "conic pcurve end seed")?;
        if let Some(parameter) = curve_parameter_near_point(ctx, curve_geometry, end, seed.get(), tolerance)? {
            end_parameters.push(parameter)?;
        }
    }
    let end_parameters = unique(ctx, end_parameters, Some)?;
    let ranges = parameter_pairs(ctx, &start_parameters, &end_parameters)?;
    Ok((!ranges.is_empty()).then_some(ranges))
}

fn parameter_pairs<'ctx>(ctx: &'ctx DecodeContext<'_>, starts: &[FiniteReal], ends: &[FiniteReal]) -> Result<Scratch<'ctx, [f64; 2]>, CodecError> {
    let mut pairs = Scratch::new(ctx)?;
    for start in starts {
        ctx.charge_work(1, "pcurve parameter pair row")?;
        for end in ends {
            ctx.charge_work(1, "pcurve parameter pair visit")?;
            pairs.push([start.get(), end.get()])?;
        }
    }
    Ok(pairs)
}

/// Find a pcurve parameter whose mapped surface point is near a topology
/// vertex. Newton steps use the pcurve tangent pushed through the surface
/// partials; a short backtracking search keeps the iteration on the selected
/// branch of a periodic or rational carrier.
fn mapped_pcurve_parameter_near_point(
    ctx: &DecodeContext<'_>,
    context: &SurfacePcurveContext<'_, '_>,
    pcurve_geometry: &PcurveGeometry,
    target: Point3,
    seed: FiniteReal,
    tolerance: f64,
) -> Result<Option<FiniteReal>, ResourceLimit> {
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Ok(None);
    }
    let domain = pcurve_parameter_domain(ctx, pcurve_geometry)?.map(ParameterInterval::from);
    // A step projects onto the pcurve domain. Without a domain, a step past
    // the finite range reaches no pcurve point at a finite distance, so the
    // search ends there.
    let stepped = |parameter: FiniteReal, step: FiniteReal| match domain {
        Some(domain) => {
            Some(domain.project(ExtendedReal::stepped(parameter, FiniteReal::ONE, step)))
        }
        None => FiniteReal::new(parameter.get() - step.get()),
    };
    // A non-finite pcurve or surface point is evaluated as a finite one is;
    // the search reads its non-finite distance.
    let uv_at = |parameter: FiniteReal| match crate::eval::decode::pcurve_uv(ctx, pcurve_geometry, parameter.get()) {
        Ok(uv) => Ok(Some(uv.get())),
        Err(failure) => failure.non_finite(),
    };
    let point_at = |parameter: FiniteReal| -> Result<Option<Point3>, ResourceLimit> {
        let Some(uv) = uv_at(parameter)? else {
            return Ok(None);
        };
        match model_surface_point_by_id(context.index, context.surface_id, uv.u, uv.v) {
            Ok(point) => Ok(Some(point.get())),
            Err(failure) => failure.non_finite(),
        }
    };
    // Only a Newton step reads the tangent pushed through the partials.
    let tangent_at = |parameter: FiniteReal| -> Result<Option<Vector3>, ResourceLimit> {
        let Some(uv) = uv_at(parameter)? else {
            return Ok(None);
        };
        let tangent_uv = match pcurve_tangent(ctx, pcurve_geometry, parameter.get()) {
            Ok(tangent) => tangent,
            Err(EvaluationFailure::ResourceLimit(limit)) => return Err(limit),
            Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(_)) => return Ok(None),
        };
        let partials =
            match model_surface_partials_by_id(context.index, context.surface_id, uv.u, uv.v) {
                Ok(partials) => partials,
                Err(EvaluationFailure::ResourceLimit(limit)) => return Err(limit),
                Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(_)) => {
                    return Ok(None)
                }
            };
        Ok(Some(Vector3::new(
            partials.du.x * tangent_uv.u + partials.dv.x * tangent_uv.v,
            partials.du.y * tangent_uv.u + partials.dv.y * tangent_uv.v,
            partials.du.z * tangent_uv.u + partials.dv.z * tangent_uv.v,
        )))
    };
    let mismatch = |point: Point3| Point3::distance(point, target);
    let mut parameter = domain.map_or(seed, |domain| {
        domain.project(ExtendedReal::from_finite(seed))
    });
    for _ in 0..32 {
        ctx.charge_work_limit(1, "mapped pcurve Newton iteration")?;
        let Some(point) = point_at(parameter)? else {
            return Ok(None);
        };
        let error = mismatch(point);
        if error.is_finite() && error <= tolerance {
            return Ok(Some(parameter));
        }
        let Some(tangent) = tangent_at(parameter)? else {
            return Ok(None);
        };
        let Some(step) = crate::math::solve::projection_step(tangent, point.vector_from(target))
        else {
            return Ok(None);
        };
        let Some(mut candidate) = stepped(parameter, step) else {
            return Ok(None);
        };
        let Some(candidate_point) = point_at(candidate)? else {
            return Ok(None);
        };
        let mut candidate_error = mismatch(candidate_point);
        for _ in 0..12 {
            ctx.charge_work_limit(1, "mapped pcurve backtracking comparison")?;
            if candidate_error <= error {
                break;
            }
            // Both parameters lie in the domain, so their midpoint does too.
            candidate = candidate.midpoint(parameter);
            let Some(candidate_point) = point_at(candidate)? else {
                return Ok(None);
            };
            candidate_error = mismatch(candidate_point);
        }
        if candidate == parameter || !candidate_error.is_finite() || candidate_error >= error {
            return Ok(None);
        }
        parameter = candidate;
    }
    Ok(None)
}

fn unique<'ctx, T>(
    ctx: &'ctx DecodeContext<'_>,
    values: impl IntoIterator<Item = T>,
    mut project: impl FnMut(T) -> Option<FiniteReal>,
) -> Result<Scratch<'ctx, FiniteReal>, CodecError> {
    let mut unique = Scratch::new(ctx)?;
    for value in values {
        ctx.charge_work(1, "parameter uniqueness source scan")?;
        let Some(value) = project(value) else { continue; };
        let mut present = false;
        for candidate in unique.iter() {
            ctx.charge_work(1, "parameter uniqueness comparison")?;
            if *candidate == value { present = true; break; }
        }
        if !present { unique.push(value)?; }
    }
    Ok(unique)
}

fn pcurve_parameter_seeds<'ctx>(ctx: &'ctx DecodeContext<'_>, pcurve: &crate::geometry::pcurve::Pcurve) -> Result<Scratch<'ctx, f64>, CodecError> {
    let mut seeds = Scratch::new(ctx)?;
    seeds.push(0.0)?;
    if let Some(range) = pcurve.parameter_range() {
        seeds.extend(range.get())?;
    }
    if let Some(domain) = pcurve_parameter_domain(ctx, &pcurve.geometry)? {
        let [start, end] = domain.endpoints();
        seeds.extend([start, start.midpoint(end), end])?;
    }
    Ok(seeds)
}

fn pcurve_parameter_seeds_on_surface<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    context: &SurfacePcurveContext<'_, '_>,
    pcurve: &crate::geometry::pcurve::Pcurve,
) -> Result<Scratch<'ctx, FiniteReal>, CodecError> {
    let mut seeds = pcurve_parameter_seeds(ctx, pcurve)?;
    let Some((origin, direction)) = pcurve.geometry.line_parameters(ctx)? else {
        return unique(ctx, seeds, FiniteReal::new);
    };
    let domains = match context.geometry.solved() {
        Some(geometry) => solved_surface_parameter_domains(ctx, geometry)?,
        None => None,
    };
    let Some([[u_lower, u_upper], [v_lower, v_upper]]) = domains else {
        return unique(ctx, seeds, FiniteReal::new);
    };
    for boundary in [u_lower, u_lower.midpoint(u_upper), u_upper] {
        if direction.u != 0.0 {
            seeds.push((boundary - origin.u) / direction.u)?;
        }
    }
    for boundary in [v_lower, v_lower.midpoint(v_upper), v_upper] {
        if direction.v != 0.0 {
            seeds.push((boundary - origin.v) / direction.v)?;
        }
    }
    unique(ctx, seeds, FiniteReal::new)
}

fn solved_surface_parameter_domains(ctx: &DecodeContext<'_>, geometry: &SolvedSurfaceGeometry) -> Result<Option<[[f64; 2]; 2]>, ResourceLimit> {
    let _depth = ctx.enter_nested_limit("surface parameter domain nesting")?;
    ctx.charge_work_limit(1, "surface parameter domain visit")?;
    Ok(match geometry {
        SolvedSurfaceGeometry::Nurbs(surface) => {
            let u_count = surface.u_count();
            let v_count = surface.v_count();
            match (
                nurbs_pcurve_parameter_domain(surface.u_degree(), surface.u_knots(), u_count),
                nurbs_pcurve_parameter_domain(surface.v_degree(), surface.v_knots(), v_count),
            ) {
                (Some(u), Some(v)) => Some([u.endpoints(), v.endpoints()]),
                _ => None,
            }
        }
        SolvedSurfaceGeometry::Transformed(placed) => {
            solved_surface_parameter_domains(ctx, placed.basis())?
        }
        SolvedSurfaceGeometry::Plane(_)
        | SolvedSurfaceGeometry::Cylinder(_)
        | SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Sphere(_)
        | SolvedSurfaceGeometry::Torus(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Unknown { .. } => None,
    })
}

/// Explicit trim metadata, if the pcurve carrier itself supplies it. A raw
/// NURBS knot domain is deliberately excluded: it bounds the carrier, not the
/// edge occurrence.
fn pcurve_parameter_extremes(ctx: &DecodeContext<'_>, pcurve: &crate::geometry::pcurve::Pcurve) -> Result<Option<[f64; 2]>, ResourceLimit> {
    match pcurve.parameter_range().map(crate::units::FiniteVector::get) {
        Some(range) => Ok(Some(range)),
        None => pcurve_geometry_trim_range(ctx, &pcurve.geometry),
    }
}

fn pcurve_geometry_trim_range(ctx: &DecodeContext<'_>, geometry: &PcurveGeometry) -> Result<Option<[f64; 2]>, ResourceLimit> {
    let _depth = ctx.enter_nested_limit("pcurve trim range nesting")?;
    ctx.charge_work_limit(1, "pcurve trim range visit")?;
    Ok(match geometry {
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let parameter_range = trimmed_pcurve.parameter_range();
            Some(parameter_range.endpoints())
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            let basis = offset_pcurve.basis();
            pcurve_geometry_trim_range(ctx, basis)?
        }
        PcurveGeometry::Transformed(placed) => pcurve_geometry_trim_range(ctx, placed.basis())?,
        PcurveGeometry::Line(_) => None,
        PcurveGeometry::Circle(_) => None,
        PcurveGeometry::Ellipse(_) => None,
        PcurveGeometry::Harmonic(_) => None,
        PcurveGeometry::Parabola(_) => None,
        PcurveGeometry::Hyperbola(_) => None,
        PcurveGeometry::Hyperbolic(_) => None,
        PcurveGeometry::PolarHarmonic(_) => None,
        PcurveGeometry::PolarNurbs { .. } => None,
        PcurveGeometry::Nurbs { .. } => None,
        PcurveGeometry::SphericalGreatCircle(_) => None,
    })
}

#[cfg(test)]
mod tests;
