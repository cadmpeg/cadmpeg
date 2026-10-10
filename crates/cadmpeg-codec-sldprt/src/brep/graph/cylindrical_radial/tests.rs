// SPDX-License-Identifier: Apache-2.0
//! Radial projection lifetime and source-backed refusal boundaries.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::scalar::PositiveLength;
use crate::brep::graph::cylindrical_radial::CylindricalRadialControls;

const SCRATCH_BYTES: u64 = 16 * 1024 * 1024;

fn curve(rational: bool) -> NurbsCurve {
    NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 2.0, 0.0), Point3::new(0.0, 2.0, 0.0)],
        rational.then(|| vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]), false,
    ).unwrap().unwrap()
}

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = 0;
    policy
}

fn project(point: Point3) -> Point2 { Point2::new(point.x, point.y) }
fn radius() -> PositiveLength { PositiveLength::new(2.0).unwrap() }
fn radial_bytes() -> u64 { u64::try_from(4 * std::mem::size_of::<Point2>()).unwrap() }
fn weight_bytes() -> u64 { u64::try_from(4 * std::mem::size_of::<f64>()).unwrap() }

#[test]
fn radius_validation_releases_weights_before_polar_projection() {
    let curve = curve(true);
    let arena = DecodeArena::new();
    let mut policy = policy();
    // collect_vec starts at minimum capacity4 for each three-pole lane.
    policy.limits.max_materialized_bytes = radial_bytes() + weight_bytes();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let radial = CylindricalRadialControls::new(&ctx, &curve, project).unwrap();
    assert!(radial.has_constant_radius(radius()).unwrap());
    assert_eq!(radial.values(), [Point2::new(2.0, 0.0), Point2::new(2.0, 2.0), Point2::new(0.0, 2.0)]);
    drop(ctx.reserve_scoped(weight_bytes(), "test expired radius weights").unwrap());
    drop(radial);
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test expired radial controls").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn rejected_polynomial_radius_releases_controls_without_retained_storage() {
    let curve = curve(false);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let radial = CylindricalRadialControls::new(&ctx, &curve, project).unwrap();
    assert!(!radial.has_constant_radius(radius()).unwrap());
    drop(radial);
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test rejected radial controls").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn radial_projection_refuses_before_the_first_backing_allocation() {
    let curve = curve(true);
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| CylindricalRadialControls::new(&ctx, &curve, project));
    let Err(CodecError::ResourceLimit(limit)) = result else { panic!("radial backing refusal"); };
    drop(result);
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect Parasolid cylinder radial poles");
    assert_eq!((limit.used, limit.additional), (0, radial_bytes()));
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn radius_weight_refusal_keeps_live_radial_backing_and_original_fuse() {
    let curve = curve(true);
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = radial_bytes();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let radial = CylindricalRadialControls::new(&ctx, &curve, project).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| radial.has_constant_radius(radius()));
    let Err(CodecError::ResourceLimit(limit)) = result else { panic!("weight backing refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect Parasolid cylinder pole weights");
    assert_eq!((limit.used, limit.additional), (radial_bytes(), weight_bytes()));
    assert_eq!(allocations, 0);
    drop(radial);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn radial_projection_preserves_prior_refusal_without_allocating() {
    let curve = curve(true);
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_collection_items(1, "test original radial refusal") else { panic!("original refusal"); };
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| CylindricalRadialControls::new(&ctx, &curve, project));
    assert!(matches!(&result, Err(CodecError::ResourceLimit(actual)) if *actual == limit));
    assert_eq!(allocations, 0);
    drop(result);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

fn polar_backing() -> (u64, u64, u64, u64, u64) {
    use cadmpeg_ir::geometry::pcurve::{PolarNurbsPole, WeightedPolarNurbsPole};
    use cadmpeg_ir::scalar::FiniteReal;
    use cadmpeg_ir::units::FinitePoint2;
    // Raw and paired lanes use minimum capacity4; IR converts exactly3 poles.
    let raw = u64::try_from(4 * std::mem::size_of::<PolarNurbsPole>()).unwrap();
    let weights = u64::try_from(4 * std::mem::size_of::<cadmpeg_ir::scalar::NonZeroReal>()).unwrap();
    let knots = u64::try_from(6 * std::mem::size_of::<f64>()).unwrap();
    let paired = u64::try_from(4 * std::mem::size_of::<WeightedPolarNurbsPole>()).unwrap();
    let admitted = u64::try_from(3 * std::mem::size_of::<WeightedPolarNurbsPole<FinitePoint2, FiniteReal>>()).unwrap();
    (raw, weights, knots, paired, admitted)
}

#[test]
fn polar_conversion_retains_only_final_lanes_and_releases_workspace() {
    let source = curve(true);
    let (raw, weights, knots, paired, admitted) = polar_backing();
    let peak = raw + weights + knots + paired + admitted;
    let output = knots + admitted;
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = peak;
    policy.limits.max_retained_bytes = output;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let radial = CylindricalRadialControls::new(&ctx, &source, project).unwrap();
    assert!(radial.has_constant_radius(radius()).unwrap());
    let polar = radial.into_polar(|point| point.z, |_| -> Result<(), CodecError> {
        panic!("valid finite poles preserve the polar carrier")
    }).unwrap().unwrap();
    assert_eq!(polar.degree(), 2);
    assert_eq!(polar.knots().as_slice(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    let cadmpeg_ir::geometry::pcurve::PolarNurbsPoles::Rational { poles } = polar.pole_rows() else {
        panic!("the source rational form is preserved");
    };
    assert_eq!(poles.len(), 3);
    for (index, pole) in poles.iter().enumerate() {
        assert_eq!(pole.radial.get(), project(source.pole_rows().point_at(index).unwrap().get()));
        assert_eq!(pole.axial.get(), 0.0);
        assert_eq!(Some(pole.weight.get()), source.pole_rows().weight_at(index));
    }
    drop(ctx.reserve_scoped(peak, "reuse consumed polar construction lanes").unwrap());
    let CodecError::ResourceLimit(limit) = ctx.copy_retained_text("x", "probe final polar backing").unwrap_err() else {
        panic!("the actual carrier consumes its exact output allowance");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((limit.used, limit.additional), (output, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn polar_conversion_preserves_exact_workspace_and_output_refusals() {
    let source = curve(true);
    let (raw, weights, knots, paired, admitted) = polar_backing();
    let peak = raw + weights + knots + paired + admitted;
    let output = knots + admitted;
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::RetainedBytes] {
        let arena = DecodeArena::new();
        let mut policy = policy();
        policy.limits.max_materialized_bytes = peak;
        policy.limits.max_retained_bytes = output;
        if dimension == ResourceDimension::MaterializedBytes {
            policy.limits.max_materialized_bytes -= 1;
        } else {
            policy.limits.max_retained_bytes -= 1;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let radial = CylindricalRadialControls::new(&ctx, &source, project).unwrap();
        let result = radial.into_polar(|point| point.z, |_| -> Result<(), CodecError> {
            panic!("a resource refusal must precede structural recovery")
        });
        let Err(CodecError::ResourceLimit(limit)) = result else {
            panic!("one byte below actual peak or output refuses");
        };
        assert_eq!(limit.dimension, dimension);
        if dimension == ResourceDimension::MaterializedBytes {
            assert_eq!(limit.operation, "IR polar admitted poles");
            assert_eq!((limit.used, limit.additional), (raw + weights + knots + paired, admitted));
        } else {
            assert_eq!(limit.operation, "hold cylindrical polar construction");
            assert_eq!((limit.used, limit.additional), (0, output));
        }
        assert_eq!(ctx.resource_refusal(), Some(limit));
        let CodecError::ResourceLimit(replay) = ctx.reserve_scoped(0, "replay original polar refusal").unwrap_err() else {
            panic!("original refusal remains sticky after conversion teardown");
        };
        assert_eq!(replay, limit);
    }
}

#[test]
fn polar_nonfinite_projection_retains_only_original_final_loss() {
    let source = curve(true);
    // Subtracting two finite axial coordinates can overflow. The original
    // dot products then yield NaN in radial terms multiplied by zero.
    let origin = Point3::new(0.0, 0.0, -f64::MAX);
    let source = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), source.degree(),
        source.knots().as_slice().to_vec(),
        vec![Point3::new(2.0, 0.0, f64::MAX), Point3::new(2.0, 2.0, f64::MAX), Point3::new(0.0, 2.0, f64::MAX)],
        Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]), false,
    ).unwrap().unwrap();
    const EXPECTED: &str = "cylindrical pcurve for edge test:model:entity#edge: poles contain a non-finite value; the carrier is not emitted and the entities that reference it fall back to an untyped support.";
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_retained_bytes = u64::try_from(EXPECTED.len()).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let radial = CylindricalRadialControls::new(&ctx, &source, |point| {
        let d = [point.x - origin.x, point.y - origin.y, point.z - origin.z];
        Point2::new(d[0] + d[1] * 0.0 + d[2] * 0.0, d[0] * 0.0 + d[1] + d[2] * 0.0)
    }).unwrap();
    assert!(radial.has_constant_radius(radius()).unwrap());
    let note = radial.into_polar(|point| point.z - origin.z, |error| {
        crate::loss::spline_lane_refusal(&ctx,
            format_args!("cylindrical pcurve for edge test:model:entity#edge: {error}"))
    }).unwrap().unwrap_err();
    assert_eq!(note.message, EXPECTED);
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "reuse rejected polar construction").unwrap());
    let CodecError::ResourceLimit(limit) = ctx.copy_retained_text("x", "probe final polar loss").unwrap_err() else {
        panic!("only the final loss message remains retained");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((limit.used, limit.additional), (u64::try_from(EXPECTED.len()).unwrap(), 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
