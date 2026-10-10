// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::tests::variable_blend::variable_blend_eval_fixture;
use crate::geometry::surface_payloads::ParallelOffsetSurfaceConstruction;
use crate::geometry::VariableBlendCrossSection;
use crate::math::Point2;
use cadmpeg_core::decode::WorkBudget;

fn rounded(parallel: bool, distance: f64, moving: bool) -> (CadIr, SurfaceId, SurfaceId) {
    let (mut ir, base) = variable_blend_eval_fixture(Point3::new(0.0, 0.0, 0.0),
        [(Point2::new(3.0, 0.0), if moving { Point2::new(0.0, 1.0) } else { Point2::new(1.0, 0.0) }),
            (Point2::new(0.0, 3.0), if moving { Point2::new(1.0, 0.0) } else { Point2::new(0.0, -1.0) })],
        [3.0, 3.0], Some(VariableBlendCrossSection::RoundedChamfer { radius: None }));
    let id = SurfaceId::mint("test:model:rounded-normal#offset").unwrap();
    let construction = ProceduralSurfaceId::mint("test:model:rounded-normal#construction").unwrap();
    let definition = if parallel {
        ProceduralSurfaceDefinition::ParallelOffset(ParallelOffsetSurfaceConstruction::try_new(
            base.clone(), distance, Some(true)).unwrap())
    } else { ProceduralSurfaceDefinition::Offset(OffsetSurfaceConstruction::try_new(
        base.clone(), distance, None, None, false,
        OffsetExtension::Legacy { flags: LegacyExtensionFlags::Absent {}, cache: None }).unwrap()) };
    ir.model.surfaces.push(Surface { id: id.clone(), geometry: SurfaceGeometry::Solved(
        SolvedSurfaceGeometry::Unknown { record: None }), source_object: None });
    ir.model.add_procedural_surface(&StandardAdmission, &id,
        ProceduralSurface::new(construction, definition, None)).unwrap().unwrap();
    (ir, base, id)
}

#[test]
fn rounded_chamfer_point_supplies_its_genuine_ruled_oriented_normal() {
    // B=(3(1-u),v,3u); Bu cross Bv=(-3,0,-3).
    let expected = Point3::new(2.25 - 1.0 / 2.0_f64.sqrt(), 0.5, 0.75 - 1.0 / 2.0_f64.sqrt());
    for parallel in [false, true] {
        let (ir, base, offset) = rounded(parallel, 1.0, true);
        let index = ModelIndex::build(&ir, StandardIndex);
        let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for mode in [admission::EvaluationAdmission::Standard, admission::EvaluationAdmission::Decode(&ctx)] {
            assert_eq!(crate::eval::model_surface_point_by_id(mode, &index, &base, 0.25, 0.5).unwrap().get(),
                Point3::new(2.25, 0.5, 0.75));
            assert!(crate::eval::model_surface_point_by_id(mode, &index, &offset, 0.25, 0.5)
                .unwrap().get().distance(expected) <= EPS_DIRECT_NORMAL_POINT);
        }
        ctx.finish_session().unwrap();
    }
}

#[test]
fn rounded_chamfer_normal_reads_two_supports_once_with_real_work_boundary() {
    for parallel in [false, true] {
        let (ir, base, offset) = rounded(parallel, 1.0, true);
        let index = ModelIndex::build(&ir, StandardIndex);
        // Base: one carrier plus two support steps. Offset adds one carrier.
        for (id, needed) in [(&base, 3), (&offset, 4)] {
            for cap in [needed - 1, needed] {
                let work = WorkBudget::new(cap);
                let actual = admission::EvaluationAdmission::Standard.within_work_slice(&work, |mode|
                    crate::eval::model_surface_point_by_id(mode, &index, id, 0.25, 0.5));
                if cap == needed { assert!(actual.is_ok()); }
                else { assert_eq!(actual, Err(EvaluationFailure::NoValue)); }
                assert_eq!(work.consumed(), cap);
            }
        }
        let mut policy = DecodePolicy::service(); policy.limits.max_work_units = 0;
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let original = ctx.charge_work_limit(1, "actual prior rounded-normal refusal").unwrap_err();
        for id in [&base, &offset] {
            assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Decode(&ctx),
                &index, id, f64::NAN, f64::NAN), Err(EvaluationFailure::ResourceLimit(original)));
        }
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
    }
}

#[test]
fn rounded_chamfer_zero_offset_keeps_a_point_without_any_track_normal() {
    // At (u,v)=(1/2,0), Bu=(-3,0,3) and Bv=(1/2,0,-1/2).
    // Both line directions are admitted; the ruled normal is zero.
    for parallel in [false, true] {
        for distance in [0.0, 1.0] {
            let (ir, base, offset) = rounded(parallel, distance, false);
            let index = ModelIndex::build(&ir, StandardIndex);
            let policy = DecodePolicy::service();
            let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            for mode in [admission::EvaluationAdmission::Standard, admission::EvaluationAdmission::Decode(&ctx)] {
                let point = crate::eval::model_surface_point_by_id(mode, &index, &base, 0.5, 0.0).unwrap();
                assert_eq!(point.get(), Point3::new(1.5, 0.0, 1.5));
                let actual = crate::eval::model_surface_point_by_id(mode, &index, &offset, 0.5, 0.0);
                if distance == 0.0 { assert_eq!(actual, Ok(point)); }
                else { assert_eq!(actual, Err(EvaluationFailure::NoValue)); }
            }
            ctx.finish_session().unwrap();
        }
    }
}
