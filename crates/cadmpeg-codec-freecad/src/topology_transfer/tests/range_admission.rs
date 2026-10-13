// SPDX-License-Identifier: Apache-2.0

use super::super::normalize_occt_curve_range;
use super::{admitted_range, raw_range};
use cadmpeg_core::decode::admission::StandardAdmission;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{PlacedCurve, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;

fn circle() -> SolvedCurveGeometry {
    SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("finite circle"),
    )
}

fn place(basis: SolvedCurveGeometry) -> SolvedCurveGeometry {
    SolvedCurveGeometry::Transformed(
        PlacedCurve::try_new(Box::new(basis), Transform::identity())
            .expect("one admitted placement frame"),
    )
}

#[test]
fn actual_curve_range_walk_obeys_original_depth_and_sticky_refusal() {
    for range in [Some(admitted_range([-f64::MAX, f64::MAX])), None] {
        for cap in [0, 1, 2] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_recursion_depth = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
            let leaf = circle();
            if cap != 0 {
                assert_eq!(
                    normalize_occt_curve_range(&ctx, &leaf, range).unwrap(),
                    range
                );
                let released = ctx.enter_nested("completed curve range leaf").unwrap();
                drop(released);
            }
            let nested = place(circle());
            if cap == 2 {
                assert_eq!(
                    normalize_occt_curve_range(&ctx, &nested, range).unwrap(),
                    range
                );
                let root = ctx.enter_nested("completed curve range root").unwrap();
                let child = ctx.enter_nested("completed curve range child").unwrap();
                drop((child, root));
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let selected = if cap == 0 { &leaf } else { &nested };
                let CodecError::ResourceLimit(original) =
                    normalize_occt_curve_range(&ctx, selected, range)
                        .expect_err("actual curve range frame exceeds caller depth")
                else {
                    panic!("recursion refusal");
                };
                assert_eq!(original.dimension, ResourceDimension::RecursionDepth);
                assert_eq!(original.used, cap);
                assert_eq!(original.additional, 1);
                assert_eq!(original.limit, cap);
                assert_eq!(original.operation, "FreeCAD curve range nesting");
                assert_eq!(ctx.resource_refusal(), Some(original));
                assert!(matches!(normalize_occt_curve_range(&ctx, &leaf, None),
                    Err(CodecError::ResourceLimit(repeated)) if repeated == original));
            }
        }
    }
}

#[test]
fn standard_curve_range_walk_keeps_transformed_arithmetic_and_absence() {
    let parabola = SolvedCurveGeometry::Parabola(
        cadmpeg_ir::geometry::analytic::ParabolaCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
        )
        .expect("finite parabola"),
    );
    let nested = place(place(parabola));
    assert_eq!(
        normalize_occt_curve_range(
            &StandardAdmission,
            &nested,
            Some(admitted_range([-2.0, 4.0]))
        )
        .unwrap()
        .map(raw_range),
        Some([-0.25, 0.5])
    );
    assert_eq!(
        normalize_occt_curve_range(&StandardAdmission, &nested, None).unwrap(),
        None
    );
    let nested = place(place(circle()));
    assert_eq!(
        normalize_occt_curve_range(
            &StandardAdmission,
            &nested,
            Some(admitted_range([-f64::MAX, f64::MAX]))
        )
        .unwrap()
        .map(raw_range),
        Some([-f64::MAX, f64::MAX])
    );
}
