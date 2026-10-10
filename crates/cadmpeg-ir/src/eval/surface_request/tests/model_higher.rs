// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::surface_request::{model_requested_jet, RequestedJet};
use crate::features::FiniteVector3;
use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::geometry::surface_payloads::SubsetSurfaceConstruction;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

const EPS_MODEL_HIGHER_PARTIALS: f64 = 1.0e-10;

fn mixed_polynomial(rational: bool) -> SolvedSurfaceGeometry {
    // S=(u,v,(1+u)^5*(1+v)^5). The z Bernstein controls are 2^(i+j).
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let poles = (0..6_u32).map(|i| (0..6_u32).map(|j| Point3::new(
        f64::from(i) / 5.0, f64::from(j) / 5.0, f64::from(1_u32 << (i + j)),
    )).collect()).collect();
    let knots = vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    let surface = NurbsSurface::from_lanes(&ctx,
        NurbsSurfaceAxis::new(5, knots.clone(), false),
        NurbsSurfaceAxis::new(5, knots, false),
        NurbsSurfaceLanes::new(poles, rational.then(|| vec![vec![1.0; 6]; 6])), false,
    ).unwrap().unwrap();
    ctx.finish_session().unwrap();
    SolvedSurfaceGeometry::Nurbs(surface)
}

fn close<const N: usize>(actual: [FiniteVector3; N], expected: [Vector3; N]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual.get() - expected).norm() <= EPS_MODEL_HIGHER_PARTIALS,
            "{actual:?} versus {expected:?}");
    }
}

fn lower_equal(actual: &RequestedJet, expected: &RequestedJet) {
    assert_eq!(actual.jet.point, expected.jet.point);
    assert_eq!(actual.jet.first, expected.jet.first);
    assert_eq!(actual.jet.second, expected.jet.second);
}

#[test]
fn model_requested_higher_maps_every_mixed_lane_through_subset_replica_and_zero_offset() {
    for rational in [false, true] {
        for reversed in [[false, false], [true, false], [false, true], [true, true]] {
            let mut ir = CadIr::empty();
            let base = stored(&mut ir, "mixed", mixed_polynomial(rational));
            let subset = procedural(&mut ir, "mixed-subset", ProceduralSurfaceDefinition::Subset(
                SubsetSurfaceConstruction::try_new(base, [[0.0, 1.0], [0.0, 1.0]],
                    Some(!reversed[0]), Some(!reversed[1]), None).unwrap()));
            let placed = procedural(&mut ir, "mixed-placed", ProceduralSurfaceDefinition::Replica {
                source: subset, transform: Transform::affine([
                    [2.0, 0.0, 0.0, 7.0], [0.0, 4.0, 0.0, 11.0], [0.0, 0.0, 3.0, 13.0],
                ]).unwrap(),
            });
            let zero = offset(&mut ir, "mixed-zero", placed.clone(), -0.0);
            let index = ModelIndex::build(&ir, StandardIndex);
            let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            // Each lane is 3*(5)_(n-k)*(5)_k with the actual chart sign.
            let expected = |coefficients: &[f64], order: usize, lane: usize| {
                let sign = if (reversed[0] && (order - lane) % 2 != 0)
                    != (reversed[1] && lane % 2 != 0) { -1.0 } else { 1.0 };
                Vector3::new(0.0, 0.0, sign * 3.0 * coefficients[lane])
            };
            for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
                let third = model_requested_jet(admission, &index, &placed, 0.0, 0.0, SurfaceRequest::Third).unwrap();
                let fourth = model_requested_jet(admission, &index, &placed, 0.0, 0.0, SurfaceRequest::Fourth).unwrap();
                let fifth = model_requested_jet(admission, &index, &placed, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
                assert_eq!(fifth.jet.point.get(), Point3::new(7.0, 11.0, 16.0));
                lower_equal(&fifth, &third); lower_equal(&fifth, &fourth);
                assert_eq!(fifth.higher.third(), third.higher.third());
                assert_eq!(fifth.higher.fourth(), fourth.higher.fourth());
                assert_eq!(third.higher.fourth(), Err(EvaluationFailure::NoValue));
                assert_eq!(fourth.higher.fifth(), Err(EvaluationFailure::NoValue));
                close(fifth.higher.third().unwrap(), std::array::from_fn(|k| expected(&[60.0, 100.0, 100.0, 60.0], 3, k)));
                close(fifth.higher.fourth().unwrap(), std::array::from_fn(|k| expected(&[120.0, 300.0, 400.0, 300.0, 120.0], 4, k)));
                close(fifth.higher.fifth().unwrap(), std::array::from_fn(|k| expected(&[120.0, 600.0, 1200.0, 1200.0, 600.0, 120.0], 5, k)));
                let zero = model_requested_jet(admission, &index, &zero, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
                lower_equal(&zero, &fifth);
                assert_eq!(zero.higher.third(), fifth.higher.third());
                assert_eq!(zero.higher.fourth(), fifth.higher.fourth());
                assert_eq!(zero.higher.fifth(), fifth.higher.fifth());
            }
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn model_requested_higher_preserves_available_offset_orders_and_affine_capability() {
    let mut ir = CadIr::empty();
    let cylinder = stored(&mut ir, "cylinder", cylinder());
    let shifted = offset(&mut ir, "cylinder-offset", cylinder, 1.0);
    let plane = stored(&mut ir, "plane", plane());
    let plane = offset(&mut ir, "plane-offset", plane, 3.0);
    let index = ModelIndex::build(&ir, StandardIndex);
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let fourth = model_requested_jet(admission, &index, &shifted, 0.0, 0.0, SurfaceRequest::Fourth).unwrap();
        let fifth = model_requested_jet(admission, &index, &shifted, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
        lower_equal(&fifth, &fourth);
        assert_eq!(fifth.higher.third(), fourth.higher.third());
        assert_eq!(fifth.higher.fourth(), fourth.higher.fourth());
        close(fifth.higher.fourth().unwrap(), [Vector3::new(3.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 0.0)]);
        assert_eq!(fifth.higher.fifth(), Err(EvaluationFailure::NoValue));
        let plane = model_requested_jet(admission, &index, &plane, 0.25, 0.5, SurfaceRequest::Fifth).unwrap();
        assert!(matches!(plane.higher, HigherPartials::Affine));
        assert_eq!(plane.higher.third(), Ok([FiniteVector3::ZERO; 4]));
        assert_eq!(plane.higher.fourth(), Ok([FiniteVector3::ZERO; 5]));
        assert_eq!(plane.higher.fifth(), Ok([FiniteVector3::ZERO; 6]));
    }
    ctx.finish_session().unwrap();
}

#[test]
fn model_requested_higher_preserves_original_refusal_before_parameter_and_order() {
    let mut ir = CadIr::empty();
    let plane = stored(&mut ir, "plane", plane());
    let index = ModelIndex::build(&ir, StandardIndex);
    let mut policy = DecodePolicy::service(); policy.limits.max_work_units = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = ctx.charge_work_limit(1, "actual prior model higher refusal").unwrap_err();
    for request in [SurfaceRequest::First, SurfaceRequest::Second, SurfaceRequest::Third, SurfaceRequest::Fourth, SurfaceRequest::Fifth] {
        let actual = model_requested_jet(EvaluationAdmission::Decode(&ctx), &index, &plane, f64::NAN, f64::NAN, request);
        assert!(matches!(actual, Err(EvaluationFailure::ResourceLimit(limit)) if limit == original));
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}
