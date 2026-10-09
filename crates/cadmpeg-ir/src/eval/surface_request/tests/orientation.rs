// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::geometry::surface_payloads::{ParallelOffsetSurfaceConstruction, SubsetSurfaceConstruction};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

const EPS_ORIENTED_CUBIC: f64 = 1.0e-12;

fn cubic(normal_reversed: bool, rational: bool) -> SolvedSurfaceGeometry {
    // S(u,v)=(u,v,u^3). The cubic Bernstein z controls are0,0,0,1.
    // Fixture construction precedes the actual evaluation session.
    let ctx = cadmpeg_test_support::service_decode_context();
    let poles = (0..4_u32).map(|i| (0..2_u32).map(|j| {
        Point3::new(f64::from(i) / 3.0, f64::from(j), f64::from(u8::from(i == 3)))
    }).collect()).collect();
    SolvedSurfaceGeometry::Nurbs(NurbsSurface::from_lanes(&ctx,
        NurbsSurfaceAxis::new(3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(poles, rational.then(|| vec![vec![1.0; 2]; 4])),
        normal_reversed).unwrap().unwrap())
}

fn close(actual: Vector3, expected: Vector3) {
    for (actual, expected) in [actual.x, actual.y, actual.z].into_iter().zip([expected.x, expected.y, expected.z]) {
        assert!((actual - expected).abs() <= EPS_ORIENTED_CUBIC, "{actual} vs {expected}");
    }
}

#[test]
fn reversed_stored_nurbs_offsets_keep_point_first_and_true_second_orientation() {
    for reversed in [false, true] {
        for rational in [false, true] {
            for parallel in [false, true] {
                let mut ir = CadIr::empty();
                let base = stored(&mut ir, "cubic", cubic(reversed, rational));
                let shifted = if parallel {
                    procedural(&mut ir, "offset", ProceduralSurfaceDefinition::ParallelOffset(
                        ParallelOffsetSurfaceConstruction::try_new(base, 1.0, None).unwrap()))
                } else { offset(&mut ir, "offset", base, 1.0) };
                let index = ModelIndex::build(&ir, StandardIndex);
                let sign = if reversed { -1.0 } else { 1.0 };
                // At zero n=(0,0,1), n_u=0, n_uu=(-6,0,0).
                for request in [SurfaceRequest::First, SurfaceRequest::Second] {
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_retained_bytes = 0;
                    let arena = DecodeArena::new();
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    for admission in [EvaluationAdmission::Decode(&ctx), EvaluationAdmission::Standard] {
                        let result = model_jet(admission, &index, &shifted, 0.0, 0.0, request).unwrap();
                        assert_eq!(result.point.get(), Point3::new(0.0, 0.0, sign));
                        close(result.first.unwrap()[0].get(), Vector3::new(1.0, 0.0, 0.0));
                        close(result.first.unwrap()[1].get(), Vector3::new(0.0, 1.0, 0.0));
                        if request == SurfaceRequest::Second {
                            let second = result.second.unwrap();
                            close(second[0].get(), Vector3::new(-6.0 * sign, 0.0, 0.0));
                            close(second[1].get(), Vector3::new(0.0, 0.0, 0.0));
                            close(second[2].get(), Vector3::new(0.0, 0.0, 0.0));
                        }
                        assert_eq!(crate::eval::model_surface_point_by_id(admission, &index, &shifted, 0.0, 0.0).unwrap(), result.point);
                    }
                    ctx.finish_session().unwrap();
                }
            }
        }
    }
}

#[test]
fn reversed_stored_nurbs_subset_offset_composes_flag_and_chart_senses() {
    for reversed in [false, true] {
        for senses in [[true, true], [false, true], [true, false], [false, false]] {
            let mut ir = CadIr::empty();
            let base = stored(&mut ir, "cubic", cubic(reversed, false));
            let subset = procedural(&mut ir, "subset", ProceduralSurfaceDefinition::Subset(
                SubsetSurfaceConstruction::try_new(base, [[0.0, 1.0], [0.0, 1.0]], Some(senses[0]), Some(senses[1]), None).unwrap()));
            let shifted = offset(&mut ir, "offset", subset, 1.0);
            let result = evaluate(&ir, &shifted, 0.0, 0.0, SurfaceRequest::Second);
            let chart_sign = if senses[0] == senses[1] { 1.0 } else { -1.0 };
            let distance = if reversed { -chart_sign } else { chart_sign };
            assert_eq!(result.point.get(), Point3::new(0.0, 0.0, distance));
            close(result.first.unwrap()[0].get(), Vector3::new(if senses[0] { 1.0 } else { -1.0 }, 0.0, 0.0));
            close(result.first.unwrap()[1].get(), Vector3::new(0.0, if senses[1] { 1.0 } else { -1.0 }, 0.0));
            close(result.second.unwrap()[0].get(), Vector3::new(-6.0 * distance, 0.0, 0.0));
        }
    }
}

#[test]
fn reversed_stored_nurbs_zero_offset_identity_and_original_fuse_remain_exact() {
    for reversed in [false, true] {
        let mut ir = CadIr::empty();
        let base = stored(&mut ir, "cubic", cubic(reversed, true));
        let shifted = offset(&mut ir, "zero", base.clone(), 0.0);
        for request in [SurfaceRequest::First, SurfaceRequest::Second] {
            let expected = evaluate(&ir, &base, 0.25, 0.5, request);
            let actual = evaluate(&ir, &shifted, 0.25, 0.5, request);
            assert_eq!(actual.point, expected.point);
            assert_eq!(actual.first, expected.first);
            assert_eq!(actual.second, expected.second);
        }
        let index = ModelIndex::build(&ir, StandardIndex);
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let original = ctx.charge_work_limit(1, "original oriented NURBS refusal").unwrap_err();
        for request in [SurfaceRequest::First, SurfaceRequest::Second] {
            assert!(matches!(model_jet(EvaluationAdmission::Decode(&ctx), &index, &shifted, f64::NAN, 0.0, request), Err(EvaluationFailure::ResourceLimit(limit)) if limit == original));
        }
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
    }
}

mod replica;
