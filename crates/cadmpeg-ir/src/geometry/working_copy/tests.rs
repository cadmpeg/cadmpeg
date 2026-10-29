// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::features::FinitePoint3;
use crate::geometry::nurbs::{NurbsCurve, NurbsPoles3, WeightedPole3};
use crate::math::Point3;
use crate::scalar::NonZeroReal;
use crate::transform::Transform;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

fn rational_curve() -> crate::geometry::nurbs::NurbsCurve {
    NurbsCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        NurbsPoles3::Rational {
            points: vec![
                WeightedPole3 {
                    point: FinitePoint3::new(Point3::new(0.0, 1.0, 2.0)).unwrap(),
                    weight: NonZeroReal::new(-2.0).unwrap(),
                },
                WeightedPole3 {
                    point: FinitePoint3::new(Point3::new(3.0, 4.0, 5.0)).unwrap(),
                    weight: NonZeroReal::new(3.0).unwrap(),
                },
            ],
        },
        true,
    )
    .unwrap()
}

#[test]
fn admitted_working_copy_keeps_exact_nurbs_and_procedural_identity() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let geometry = CurveGeometry::Procedural {
        construction: crate::ids::ProceduralCurveId::mint("fcstd:model:curve#construction")
            .unwrap(),
        cache: Some(SolvedCurveGeometry::Nurbs(rational_curve())),
    };
    let copied = geometry.try_clone_working(&ctx).unwrap();
    assert_eq!(copied, geometry);
    assert_ne!(
        copied.procedural_construction().unwrap().as_str().as_ptr(),
        geometry
            .procedural_construction()
            .unwrap()
            .as_str()
            .as_ptr()
    );
    let SolvedCurveGeometry::Nurbs(original) = geometry.solved().unwrap() else {
        panic!("source NURBS")
    };
    let SolvedCurveGeometry::Nurbs(target) = copied.solved().unwrap() else {
        panic!("copied NURBS")
    };
    assert_eq!(target.pole_rows(), original.pole_rows());
    assert_eq!(target.knots(), original.knots());
    assert_ne!(
        target.knots().as_slice().as_ptr(),
        original.knots().as_slice().as_ptr()
    );
}

#[test]
fn admitted_placement_copy_preserves_depth_without_encode_parse_depth_charge() {
    let mut source = SolvedCurveGeometry::Nurbs(rational_curve());
    for _ in 0..super::super::MAX_GEOMETRY_NESTING {
        source = SolvedCurveGeometry::Transformed(
            PlacedCurve::try_new(Box::new(source), Transform::identity()).unwrap(),
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let copied = source.try_clone_working(&ctx).unwrap();
    assert_eq!(copied, source);
    let SolvedCurveGeometry::Transformed(placed) = &copied else {
        panic!("placed copy")
    };
    assert_eq!(placed.depth, super::super::MAX_GEOMETRY_NESTING);
    assert!(PlacedCurve::try_new(Box::new(copied), Transform::identity()).is_err());
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn admitted_working_copy_propagates_resource_refusal_without_revalidation() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let source = rational_curve();
    let mut impossible = Vec::<u64>::new();
    let CodecError::ResourceLimit(refusal) = ctx
        .reserve_working(&mut impossible, usize::MAX, "admitted copy refusal")
        .unwrap_err()
    else {
        panic!("actual working overflow must return its typed resource refusal")
    };
    assert_eq!(
        refusal.reason,
        cadmpeg_core::decode::ResourceFailure::AllocationFailed
    );
    assert!(
        matches!(source.try_clone_working(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
    );
    let absent = SolvedCurveGeometry::Unknown { record: None };
    assert!(
        matches!(absent.try_clone_working(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
    );
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}
