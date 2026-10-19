// SPDX-License-Identifier: Apache-2.0
//! Agreement checks for explicit planar trim carriers.

use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::Sense;

use super::{close_point, WritableEdge, WritableEdgeCurve, WritablePcurve};

pub(super) fn validate_plane_trim(
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
    tolerance: f64,
    edge: &WritableEdge<'_>,
    sense: Sense,
    explicit: &WritablePcurve<'_>,
) -> Result<(), CodecError> {
    let domain = edge.domain;
    let mut breaks = vec![domain[0].get(), domain[1].get()];
    if let WritableEdgeCurve::Nurbs(curve) = edge.curve {
        for knot in curve.knots().finite_knots() {
            if knot > domain[0] && knot < domain[1] {
                let knot = if sense == Sense::Forward {
                    knot
                } else {
                    cadmpeg_ir::math::reflect_parameter(knot, domain[0], domain[1]).ok_or_else(
                        || CodecError::malformed("trim knot reflection is non-finite"),
                    )?
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
        let parameter = if sense == Sense::Forward {
            sample
        } else {
            cadmpeg_ir::math::reflect_parameter(sample, domain[0], domain[1])
                .ok_or_else(|| CodecError::malformed("trim sample reflection is non-finite"))?
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
                return Err(CodecError::UnsupportedContent(reason));
            }
            // (C) Endpoint images disagree, or a held carrier is non-finite.
            return Err(CodecError::malformed(reason));
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
