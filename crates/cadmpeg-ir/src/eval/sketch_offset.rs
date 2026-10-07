// SPDX-License-Identifier: Apache-2.0
//! Endpoint frames and fitted offset distance for sketch NURBS curves.

use super::nurbs_pcurve_parameter_domain;
use crate::features::FiniteVector3;
use crate::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};
use crate::math::{Point2, Vector3};
use crate::scalar::FiniteReal;
use cadmpeg_core::decode::{DecodeContext, ResourceLimit};

const EPS_EVAL_CLAMPED_NURBS_PCURVE_ENDPOINT_FRAMES_E12: f64 = 1.0e-12;
const EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E12: f64 = 1.0e-12;
const EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9: f64 = 1.0e-9;

pub(super) fn clamped_nurbs_pcurve_endpoint_frames(
    ctx: &DecodeContext<'_>,
    curve: &PcurveNurbs,
) -> Result<Option<[(Point2, Point2); 2]>, ResourceLimit> {
    let knots = curve.knots();
    let points = curve.pole_rows();
    let Some(domain) = nurbs_pcurve_parameter_domain(curve.degree(), knots, points.count()) else {
        return Ok(None);
    };
    let [lower, upper] = domain.endpoints();
    let degree = cadmpeg_core::decode::index_from_u32(curve.degree());
    if !ctx.all_by_limit(
        &knots[..degree + 1],
        |knot| Ok(*knot == lower),
        "sketch NURBS endpoint knot scan",
    )? || !ctx.all_by_limit(
        &knots[points.count()..points.count() + degree + 1],
        |knot| Ok(*knot == upper),
        "sketch NURBS endpoint knot scan",
    )? {
        return Ok(None);
    }
    if let PcurveNurbsPoles::Rational { points } = points {
        if !ctx.all_by_limit(
            points,
            |pole| Ok(pole.weight.get() > 0.0),
            "sketch NURBS endpoint weight scan",
        )? {
            return Ok(None);
        }
    }
    let Some(start) = points.point_at(0).map(crate::units::FinitePoint2::get) else {
        return Ok(None);
    };
    let Some(last) = points.count().checked_sub(1) else {
        return Ok(None);
    };
    let Some(end) = points.point_at(last).map(crate::units::FinitePoint2::get) else {
        return Ok(None);
    };
    let mut start_tangent = None;
    for index in 1..points.count() {
        ctx.charge_work_limit(1, "sketch NURBS endpoint tangent scan")?;
        let Some(point) = points.point_at(index) else {
            return Ok(None);
        };
        let tangent = Point2::new(point.u - start.u, point.v - start.v);
        if tangent.u.hypot(tangent.v) > EPS_EVAL_CLAMPED_NURBS_PCURVE_ENDPOINT_FRAMES_E12 {
            start_tangent = Some(tangent);
            break;
        }
    }
    let Some(start_tangent) = start_tangent else {
        return Ok(None);
    };
    let mut end_tangent = None;
    for index in (0..last).rev() {
        ctx.charge_work_limit(1, "sketch NURBS endpoint tangent scan")?;
        let Some(point) = points.point_at(index) else {
            return Ok(None);
        };
        let tangent = Point2::new(end.u - point.u, end.v - point.v);
        if tangent.u.hypot(tangent.v) > EPS_EVAL_CLAMPED_NURBS_PCURVE_ENDPOINT_FRAMES_E12 {
            end_tangent = Some(tangent);
            break;
        }
    }
    Ok(end_tangent.map(|end_tangent| [(start, start_tangent), (end, end_tangent)]))
}

pub(super) fn fitted_nurbs_offset_candidate(
    source: [(Point2, Point2); 2],
    result: [(Point2, Point2); 2],
    linear_tolerance: f64,
) -> Option<FiniteReal> {
    let mut distances = [0.0; 2];
    for ordinal in 0..2 {
        let (source_point, source_tangent) = source[ordinal];
        let (result_point, result_tangent) = result[ordinal];
        let source_length = source_tangent.u.hypot(source_tangent.v);
        let result_length = result_tangent.u.hypot(result_tangent.v);
        if source_length <= EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E12
            || result_length <= EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E12
        {
            return None;
        }
        let source_unit =
            FiniteVector3::new(Vector3::new(source_tangent.u, source_tangent.v, 0.0))?
                .unit_nonzero()?;
        let result_unit =
            FiniteVector3::new(Vector3::new(result_tangent.u, result_tangent.v, 0.0))?
                .unit_nonzero()?;
        if source_unit.cross(result_unit).norm() > EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9 {
            return None;
        }
        let offset_projection = |x, y| {
            crate::math::sum::finite_dot(
                [
                    result_point.u,
                    -source_point.u,
                    result_point.v,
                    -source_point.v,
                ],
                [x, x, y, y],
            )
            .ok()
        };
        let tangential = offset_projection(source_unit.x, source_unit.y)?.get();
        let coordinate_scale = 1.0
            + source_point
                .u
                .abs()
                .max(source_point.v.abs())
                .max(result_point.u.abs())
                .max(result_point.v.abs());
        if tangential.abs()
            > linear_tolerance.max(EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9 * coordinate_scale)
        {
            return None;
        }
        distances[ordinal] = offset_projection(-source_unit.y, source_unit.x)?.get();
    }
    let scale = 1.0 + distances[0].abs().max(distances[1].abs());
    // Both thresholds are comparison thresholds of this predicate, not the
    // document tolerance: the constant is the double-precision noise floor of
    // the distance arithmetic above, it is never read from the source, and no
    // stated tolerance is stored or reported at the floored magnitude.
    ((distances[0] - distances[1]).abs()
        <= linear_tolerance.max(EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9 * scale)
        && distances[0].abs() > linear_tolerance.max(EPS_EVAL_FITTED_NURBS_OFFSET_CANDIDATE_E9))
    .then(|| crate::math::interpolate(distances[0], distances[1], 0.5))
    .flatten()
}

#[cfg(test)]
mod tests {
    use crate::geometry::pcurve::PcurveNurbs;
    use crate::math::Point2;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    fn curve(v: f64, weights: Option<Vec<f64>>) -> PcurveNurbs {
        PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 0.5, 1.0, 1.0],
            vec![
                Point2::new(0.0, v),
                Point2::new(1.0, v),
                Point2::new(2.0, v),
            ],
            weights,
            false,
        )
        .expect("fixture pcurve construction admission")
        .unwrap()
    }

    #[test]
    fn sketch_nurbs_endpoint_scans_preserve_first_and_later_original_refusals() {
        let curve = curve(0.0, Some(vec![1.0, 2.0, 1.0]));
        for (cap, operation) in [
            (0, "sketch NURBS endpoint knot scan"),
            (2, "sketch NURBS endpoint knot scan"),
            (4, "sketch NURBS endpoint weight scan"),
            (6, "sketch NURBS endpoint weight scan"),
            (7, "sketch NURBS endpoint tangent scan"),
            (8, "sketch NURBS endpoint tangent scan"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let limit = super::clamped_nurbs_pcurve_endpoint_frames(&ctx, &curve).unwrap_err();
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, operation);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
            );
        }
    }

    #[test]
    fn fitted_sketch_nurbs_offset_borrows_all_rows_without_storage() {
        let source = crate::sketches::SketchGeometry::nurbs(curve(0.0, Some(vec![1.0, 2.0, 1.0])));
        let result = crate::sketches::SketchGeometry::nurbs(curve(1.0, None));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let distance =
            super::super::fitted_nurbs_offset_frame_distance(&ctx, &source, &result, 0.0)
                .unwrap()
                .unwrap();
        assert_eq!(distance.get(), 1.0);
        ctx.finish_session().unwrap();
    }

    #[test]
    fn fitted_sketch_nurbs_offset_preserves_result_curve_scan_refusal() {
        let source = crate::sketches::SketchGeometry::nurbs(curve(0.0, None));
        let result = crate::sketches::SketchGeometry::nurbs(curve(1.0, None));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The source reads four endpoint knots and the first tangent at each end.
        policy.limits.max_work_units = 6;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let limit = super::super::fitted_nurbs_offset_frame_distance(&ctx, &source, &result, 0.0)
            .unwrap_err();
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "sketch NURBS endpoint knot scan");
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }

    #[test]
    fn sketch_nurbs_endpoint_frames_keep_weight_and_degenerate_absence() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let negative_weight = curve(0.0, Some(vec![1.0, -1.0, 1.0]));
        assert_eq!(
            super::clamped_nurbs_pcurve_endpoint_frames(&ctx, &negative_weight).unwrap(),
            None
        );
        let degenerate = PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(1.0, 2.0); 2],
            None,
            false,
        )
        .expect("fixture pcurve construction admission")
        .unwrap();
        assert_eq!(
            super::clamped_nurbs_pcurve_endpoint_frames(&ctx, &degenerate).unwrap(),
            None
        );
        let offset = super::clamped_nurbs_pcurve_endpoint_frames(&ctx, &curve(0.0, None))
            .unwrap()
            .unwrap();
        assert_eq!(
            offset,
            [
                (Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)),
                (Point2::new(2.0, 0.0), Point2::new(1.0, 0.0))
            ]
        );
    }
}
