// SPDX-License-Identifier: Apache-2.0
//! Agreement checks for explicit planar trim carriers.

use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::Sense;

use super::{
    close_point, WritableEdge, WritableEdgeCurve, WritablePcurve, WriteError, EPS_WRITE_DEGENERATE,
};

pub(super) fn validate_plane_trim(
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
    tolerance: f64,
    edge: &WritableEdge<'_>,
    sense: Sense,
    explicit: &WritablePcurve<'_>,
) -> Result<(), WriteError> {
    // A reversed coedge's explicit trim follows either the traversal
    // convention Rhino authors (the edge parameter reflected across its
    // domain, as `singular_seam.3dm` slot-3 shows) or the edge convention
    // other kernels write (the shared edge parametrization, as FreeCAD
    // solids show). Both describe the same directed 3D curve, and the
    // writer keeps the authored carrier either way, so either agreement
    // admits the trim.
    let reflected = sense == Sense::Reversed;
    match check_plane_trim(origin, u_axis, v_axis, tolerance, edge, reflected, explicit) {
        Ok(()) => Ok(()),
        Err(first) if reflected => {
            check_plane_trim(origin, u_axis, v_axis, tolerance, edge, false, explicit)
                .map_err(|_| first)
        }
        Err(error) => Err(error),
    }
}

/// The agreement half of [`validate_plane_trim`]: the explicit trim tracks
/// the edge curve with (`reflect`) or without the traversal reflection.
fn check_plane_trim(
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
    tolerance: f64,
    edge: &WritableEdge<'_>,
    reflect: bool,
    explicit: &WritablePcurve<'_>,
) -> Result<(), WriteError> {
    let domain = edge.domain;
    let mut breaks = vec![domain[0].get(), domain[1].get()];
    if let WritableEdgeCurve::Nurbs(curve) = edge.curve {
        for knot in curve.knots().finite_knots() {
            if knot > domain[0] && knot < domain[1] {
                let knot = if reflect {
                    cadmpeg_ir::math::reflect_parameter(knot, domain[0], domain[1]).ok_or_else(
                        || CodecError::malformed("trim knot reflection is non-finite"),
                    )?
                } else {
                    knot
                };
                breaks.push(knot.get());
            }
        }
    }
    if let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = &explicit.source.geometry
    {
        breaks.extend(
            nurbs
                .knots()
                .iter()
                .copied()
                .filter(|knot| *knot > domain[0].get() && *knot < domain[1].get()),
        );
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    let tolerance = tolerance
        .max(
            edge.source
                .tolerance
                .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
        )
        .max(
            explicit
                .source
                .fit_tolerance()
                .map_or(0.0, cadmpeg_ir::geometry::FitTolerance::get),
        );
    for sample in samples(&breaks, domain)? {
        let uv = match cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &explicit.source.geometry,
            sample.get(),
        ) {
            Ok(point) => point.get(),
            Err(failure) => failure
                .non_finite()?
                .ok_or_else(|| CodecError::malformed("planar trim cannot be evaluated"))?,
        };
        let mapped = Point3::new(
            origin.x + uv.u * u_axis.x + uv.v * v_axis.x,
            origin.y + uv.u * u_axis.y + uv.v * v_axis.y,
            origin.z + uv.u * u_axis.z + uv.v * v_axis.z,
        );
        let parameter = if reflect {
            cadmpeg_ir::math::reflect_parameter(sample, domain[0], domain[1])
                .ok_or_else(|| CodecError::malformed("trim sample reflection is non-finite"))?
        } else {
            sample
        };
        let actual = match edge.curve.point(parameter) {
            Ok(point) => point.get(),
            Err(failure) => failure
                .non_finite()?
                .ok_or_else(|| CodecError::malformed("planar trim edge cannot be evaluated"))?,
        };
        if !close_point(mapped, actual, tolerance) {
            let reason = format!(
                "pcurve {} disagrees with its directed planar C3 projection at {}",
                explicit.source.id,
                FiniteReal::get(sample)
            );
            if mapped.x.is_finite()
                && mapped.y.is_finite()
                && mapped.z.is_finite()
                && actual.x.is_finite()
                && actual.y.is_finite()
                && actual.z.is_finite()
                && sample != domain[0]
                && sample != domain[1]
            {
                // (S-entity) Equal interior parameters are not a geometric
                // correspondence between independently parameterized curves.
                return Err(WriteError::Omit(reason));
            }
            // (C) Endpoint images disagree, or a held carrier is non-finite.
            return Err(WriteError::Omit(reason));
        }
    }

    Ok(())
}

/// Check both endpoint images before making any interior correspondence claim.
pub(super) fn samples(
    breaks: &[f64],
    domain: [FiniteReal; 2],
) -> Result<Vec<FiniteReal>, CodecError> {
    let mut samples = domain.to_vec();
    for span in breaks.windows(2) {
        for step in 0..=16 {
            samples.push(
                cadmpeg_ir::math::interpolate(span[0], span[1], f64::from(step) / 16.0)
                    .ok_or_else(|| CodecError::malformed("trim sample is non-finite"))?,
            );
        }
    }
    Ok(samples)
}

/// Checks that an explicit pcurve tracks its directed edge curve through an
/// analytic surface rewritten as a native revolution surface.
///
/// `usage` is the used parameter range the revolution payload spans, so the
/// domain test runs against the written carrier, not the unbounded analytic
/// surface. Either parameter convention admits the trim; see
/// [`validate_plane_trim`].
pub(super) fn validate_analytic_trim(
    surface: &cadmpeg_ir::geometry::SurfaceGeometry,
    usage: [[f64; 2]; 2],
    face_tolerance: f64,
    edge: &WritableEdge<'_>,
    sense: Sense,
    explicit: &WritablePcurve<'_>,
) -> Result<(), WriteError> {
    let reflected = sense == Sense::Reversed;
    match check_analytic_trim(surface, usage, face_tolerance, edge, reflected, explicit) {
        Ok(()) => Ok(()),
        Err(first) if reflected => {
            check_analytic_trim(surface, usage, face_tolerance, edge, false, explicit)
                .map_err(|_| first)
        }
        Err(error) => Err(error),
    }
}

/// The agreement half of [`validate_analytic_trim`].
fn check_analytic_trim(
    surface: &cadmpeg_ir::geometry::SurfaceGeometry,
    usage: [[f64; 2]; 2],
    face_tolerance: f64,
    edge: &WritableEdge<'_>,
    reflect: bool,
    explicit: &WritablePcurve<'_>,
) -> Result<(), WriteError> {
    let [[u0, u1], [v0, v1]] = usage;
    let domain = edge.domain;
    let inside_domain = |u: f64, v: f64| {
        cadmpeg_ir::math::parameter_in_domain(u, [u0, u1], EPS_WRITE_DEGENERATE)
            && cadmpeg_ir::math::parameter_in_domain(v, [v0, v1], EPS_WRITE_DEGENERATE)
    };
    let mut breaks = vec![domain[0].get(), domain[1].get()];
    if let WritableEdgeCurve::Nurbs(nurbs) = edge.curve {
        for knot in nurbs.knots().finite_knots() {
            if knot > domain[0] && knot < domain[1] {
                let knot = if reflect {
                    cadmpeg_ir::math::reflect_parameter(knot, domain[0], domain[1]).ok_or_else(
                        || CodecError::malformed("trim knot reflection is non-finite"),
                    )?
                } else {
                    knot
                };
                breaks.push(knot.get());
            }
        }
    }
    if let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = &explicit.source.geometry
    {
        breaks.extend(
            nurbs
                .knots()
                .iter()
                .copied()
                .filter(|knot| *knot > domain[0].get() && *knot < domain[1].get()),
        );
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    let tolerance = face_tolerance
        .max(
            edge.source
                .tolerance
                .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
        )
        .max(
            explicit
                .source
                .fit_tolerance()
                .map_or(0.0, cadmpeg_ir::geometry::FitTolerance::get),
        );
    for sample in samples(&breaks, domain)? {
        let parameter = sample.get();
        let uv = match cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &explicit.source.geometry,
            parameter,
        ) {
            Ok(uv) => uv.get(),
            Err(failure) => failure.non_finite()?.ok_or_else(|| {
                WriteError::Omit(format!(
                    "pcurve {} cannot be evaluated over its edge domain",
                    explicit.source.id.as_str()
                ))
            })?,
        };
        if !inside_domain(uv.u, uv.v) {
            return Err(WriteError::Omit(format!(
                "pcurve {} leaves its revolution parameter domain",
                explicit.source.id.as_str()
            )));
        }
        let mapped = match cadmpeg_ir::eval::decode::surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            surface,
            uv.u,
            uv.v,
        ) {
            Ok(point) => point.get(),
            Err(failure) => failure.non_finite()?.ok_or_else(|| {
                WriteError::Omit(format!(
                    "pcurve {} cannot be evaluated through its analytic surface",
                    explicit.source.id.as_str()
                ))
            })?,
        };
        let curve_parameter = if reflect {
            cadmpeg_ir::math::reflect_parameter(sample, domain[0], domain[1])
                .ok_or_else(|| WriteError::Omit("reversed edge parameter is non-finite".into()))?
        } else {
            sample
        };
        let edge_point = match edge.curve.point(curve_parameter) {
            Ok(point) => point.get(),
            Err(failure) => failure.non_finite()?.ok_or_else(|| {
                WriteError::Omit(format!(
                    "edge curve {} cannot be evaluated over its edge domain",
                    edge.curve_id
                ))
            })?,
        };
        let distance = cadmpeg_ir::math::Point3::distance(mapped, edge_point);
        if !distance.is_finite() || distance > tolerance {
            let reason = format!(
                "pcurve {} misses directed edge curve {} by {distance}",
                explicit.source.id.as_str(),
                edge.curve_id
            );
            if distance.is_finite() && sample != domain[0] && sample != domain[1] {
                return Err(WriteError::Omit(reason));
            }
            return Err(WriteError::Omit(reason));
        }
    }
    Ok(())
}
