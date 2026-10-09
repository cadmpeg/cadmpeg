// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::geometry::surface_payloads::ParallelOffsetSurfaceConstruction;
use cadmpeg_core::decode::WorkBudget;

fn publish(ir: &mut CadIr, name: &str, definition: ProceduralSurfaceDefinition) -> SurfaceId {
    let id = SurfaceId::mint(format!("test:model:zero-surface#{name}")).unwrap();
    let construction = ProceduralSurfaceId::mint(format!("test:model:zero-construction#{name}")).unwrap();
    ir.model.surfaces.push(Surface { id: id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }), source_object: None });
    ir.model.add_procedural_surface(&StandardAdmission, &id,
        ProceduralSurface::new(construction, definition, None)).unwrap().unwrap();
    id
}

fn zero(ir: &mut CadIr, support: SurfaceId, kind: usize, distance: f64) -> SurfaceId {
    let definition = if kind == 0 {
        ProceduralSurfaceDefinition::ParallelOffset(ParallelOffsetSurfaceConstruction::try_new(
            support, distance, Some(true)).unwrap())
    } else { ProceduralSurfaceDefinition::Offset(OffsetSurfaceConstruction::try_new(
        support, distance, None, None, kind == 2,
        OffsetExtension::Legacy { flags: LegacyExtensionFlags::Absent {}, cache: None }).unwrap()) };
    publish(ir, "offset", definition)
}

fn stored(ir: &mut CadIr, geometry: SolvedSurfaceGeometry) -> SurfaceId {
    let id = SurfaceId::mint("test:model:zero-surface#stored").unwrap();
    ir.model.surfaces.push(Surface { id: id.clone(), geometry: SurfaceGeometry::Solved(geometry), source_object: None });
    id
}

fn bilinear(overflowing: bool, rational: bool, reversed: bool) -> SolvedSurfaceGeometry {
    let h = if overflowing { f64::from_bits(1) } else { 1.0 };
    let poles = if overflowing {
        vec![vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)]]
    } else { vec![vec![Point3::new(-0.0, 2.0, -0.0); 2]; 2] };
    SolvedSurfaceGeometry::Nurbs(NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, h, h], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(poles, rational.then(|| vec![vec![1.0; 2]; 2])), reversed).unwrap().unwrap())
}

#[test]
fn zero_offset_point_is_exact_without_a_degenerate_or_overflowing_normal() {
    for kind in 0..3 {
        for distance in [0.0, -0.0] {
            for overflowing in [false, true] {
                for rational in [false, true] {
                    for reversed in [false, true] {
                        let mut ir = CadIr::empty();
                        let base = stored(&mut ir, bilinear(overflowing, rational, reversed));
                        let offset = zero(&mut ir, base.clone(), kind, distance);
                        let index = ModelIndex::build(&ir, StandardIndex);
                        let expected = crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
                            &index, &base, 0.0, 0.5).unwrap();
                        let actual = crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
                            &index, &offset, 0.0, 0.5).unwrap();
                        let bits = |point: FinitePoint3| { let p = point.get(); [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()] };
                        assert_eq!(bits(actual), bits(expected));
                        let mut policy = DecodePolicy::service();
                        policy.limits.max_retained_bytes = 0;
                        let arena = DecodeArena::new();
                        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                        assert_eq!(bits(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Decode(&ctx),
                            &index, &offset, 0.0, 0.5).unwrap()), bits(expected));
                        ctx.finish_session().unwrap();
                    }
                }
            }
        }
    }
}

#[test]
fn zero_offset_point_keeps_the_original_stored_cost_and_actual_model_steps() {
    for kind in 0..2 {
        let mut ir = CadIr::empty();
        let base = stored(&mut ir, bilinear(false, false, false));
        let offset = zero(&mut ir, base, kind, 0.0);
        let index = ModelIndex::build(&ir, StandardIndex);
        // The original degree1x1 stored point cost is 2*2 + 2^2 + 2^2=12.
        // Two actual model carrier steps precede it; no First cost is added.
        for cap in [13, 14] {
            let budget = WorkBudget::new(cap);
            let result = admission::EvaluationAdmission::Standard.within_work_slice(&budget, |admission| {
                crate::eval::model_surface_point_by_id(admission, &index, &offset, 0.0, 0.5)
            });
            if cap == 13 { assert_eq!(result, Err(EvaluationFailure::NoValue)); }
            else { assert_eq!(result.unwrap().get(), Point3::new(-0.0, 2.0, -0.0)); }
            assert_eq!(budget.consumed(), cap);
        }
    }
}

#[test]
fn zero_offset_keeps_a_later_offset_actual_normal_demand() {
    for case in [0, 4, 5] {
        for kind in 0..3 {
            let (mut ir, base, _, u, v, _, expected) = fixture(case);
            let identity = zero(&mut ir, base, kind, 0.0);
            let outer = publish(&mut ir, "outer", ProceduralSurfaceDefinition::Offset(
                OffsetSurfaceConstruction::try_new(identity, 1.0, None, None, false,
                    OffsetExtension::Legacy { flags: LegacyExtensionFlags::Absent {}, cache: None }).unwrap()));
            let index = ModelIndex::build(&ir, StandardIndex);
            assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
                &index, &outer, u, v).unwrap().get(), expected);
        }
    }
}

#[test]
fn zero_extended_support_point_needs_the_extrapolated_partial_without_a_normal() {
    let mut ir = CadIr::empty();
    // S(u,v)=(u,0,0) has no regular chart normal. The selected outside
    // strip point still follows its actual boundary u tangent.
    let geometry = SolvedSurfaceGeometry::Nurbs(NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(vec![vec![Point3::new(0.0, 0.0, 0.0); 2],
            vec![Point3::new(1.0, 0.0, 0.0); 2]], None), false).unwrap().unwrap());
    let base = stored(&mut ir, geometry);
    let offset = zero(&mut ir, base.clone(), 2, 0.0);
    let index = ModelIndex::build(&ir, StandardIndex);
    assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
        &index, &base, 1.5, 0.5).unwrap().get(), Point3::new(1.5, 0.0, 0.0));
    assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
        &index, &offset, 1.5, 0.5).unwrap().get(), Point3::new(1.5, 0.0, 0.0));
}

#[test]
fn zero_offset_preserves_the_selected_linear_extension_point() {
    let mut ir = CadIr::empty();
    // The quadratic Bezier poles 0,0,1 give S(u,v)=(u^2,0,0).
    // At u=1.5 the stored polynomial is 2.25; the selected boundary
    // extension is S(1,v)+(1.5-1)*S_u(1,v)=1+0.5*2=2.
    let geometry = SolvedSurfaceGeometry::Nurbs(NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(vec![vec![Point3::new(0.0, 0.0, 0.0); 2],
            vec![Point3::new(0.0, 0.0, 0.0); 2],
            vec![Point3::new(1.0, 0.0, 0.0); 2]], None), false).unwrap().unwrap());
    let base = stored(&mut ir, geometry);
    let offset = zero(&mut ir, base.clone(), 2, -0.0);
    let index = ModelIndex::build(&ir, StandardIndex);
    assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
        &index, &base, 1.5, 0.5).unwrap().get(), Point3::new(2.25, 0.0, 0.0));
    assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
        &index, &offset, 1.5, 0.5).unwrap().get(), Point3::new(2.0, 0.0, 0.0));
}

#[test]
fn zero_offset_preserves_the_original_prefused_refusal_and_finish() {
    for kind in 0..3 {
        let mut ir = CadIr::empty();
        let base = stored(&mut ir, bilinear(false, false, false));
        let offset = zero(&mut ir, base, kind, 0.0);
        let index = ModelIndex::build(&ir, StandardIndex);
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let original = ctx.charge_work_limit(1, "actual prior zero-offset refusal").unwrap_err();
        for (u, v) in [(0.0, 0.5), (f64::NAN, f64::NAN)] {
            assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Decode(&ctx),
                &index, &offset, u, v), Err(EvaluationFailure::ResourceLimit(original)));
        }
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
    }
}
