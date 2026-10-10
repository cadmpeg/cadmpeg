// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::{Point3, Vector3};

fn curve(periodic: bool) -> NurbsCurve {
    let knots = if periodic { vec![-1.0, 0.0, 1.0, 2.0] } else { vec![0.0, 0.0, 1.0, 1.0] };
    NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 1, knots,
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)], None, periodic)
        .expect("fixture constructor admission").expect("valid degree-one spline")
}

fn unavailable<T>(result: Option<Result<T, CodecError>>, original: Option<ResourceLimit>) {
    match original {
        Some(first) => assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first)),
        None => assert!(result.is_none()),
    }
}

#[test]
fn asm_polynomial_circle_recognition_preserves_original_refusal() {
    let curve = curve(false);
    crate::test_support::with_entry_context(|ctx, original| {
        unavailable(super::super::rational_four_arc_circle(ctx, &curve), original);
    });
}

#[test]
fn asm_periodic_spine_recognition_preserves_original_refusal() {
    let curve = curve(true);
    crate::test_support::with_entry_context(|ctx, original| {
        unavailable(super::super::linear_nurbs_spine(ctx, &curve), original);
    });
}

#[test]
fn asm_empty_spine_points_preserve_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        unavailable(super::super::linear_spine_points(ctx, &[] as &[cadmpeg_ir::features::FinitePoint3],
            |_| panic!("empty spine must execute no point callback")), original);
    });
}

#[test]
fn asm_polynomial_extrusion_recognition_preserves_original_refusal() {
    let definition = crate::nurbs::proc_surface::DecodedProceduralSurfaceDefinition::Extrusion {
        directrix: curve(false), parameter_interval: [0.0, 1.0], direction: Vector3::new(0.0, 0.0, 1.0),
        native_position: Point3::new(0.0, 0.0, 0.0), revision_form: None,
    };
    crate::test_support::with_entry_context(|ctx, original| {
        unavailable(super::super::analytic_procedural_surface(ctx, &definition), original);
    });
}

fn rolling_ball(radius: f64) {
    let spine = curve(false);
    crate::test_support::with_entry_context(|ctx, original| {
        unavailable(super::super::analytic_rolling_ball_surface(ctx, &[None, None], None, &spine, radius), original);
    });
}

#[test]
fn asm_zero_radius_rolling_ball_preserves_original_refusal() { rolling_ball(0.0); }

#[test]
fn asm_absent_rolling_ball_supports_preserve_original_refusal() { rolling_ball(1.0); }

#[test]
fn asm_fixed_curve_reversal_preserves_original_refusal() {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::geometry::analytic::LineCurve;
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::try_new(
        Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).expect("line fixture")));
    let reversed = CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::try_new(
        Point3::new(0.0, 0.0, 0.0), Vector3::new(-1.0, 0.0, 0.0)).expect("reversed line fixture")));
    crate::test_support::with_entry_context(|ctx, original| {
        let mut geometry = line.clone();
        let result = super::super::reverse_curve_geometry(ctx, &mut geometry);
        match original {
            Some(first) => {
                assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
                assert_eq!(geometry, line);
            }
            None => {
                result.expect("fixed line reversal is free");
                assert_eq!(geometry, reversed);
            }
        }
    });
}
