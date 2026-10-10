// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::tests::law_sweep::law_sweep_model;
use crate::geometry::LawExpression;
use cadmpeg_core::decode::{ResourceDimension, WorkBudget};

fn shifted(ir: &mut CadIr, support: SurfaceId, distance: f64) -> SurfaceId {
    let id = SurfaceId::mint("test:model:sweep-normal#offset").unwrap();
    let construction = ProceduralSurfaceId::mint("test:model:sweep-normal#construction").unwrap();
    ir.model.surfaces.push(Surface { id: id.clone(), geometry: SurfaceGeometry::Solved(
        SolvedSurfaceGeometry::Unknown { record: None }), source_object: None });
    ir.model.add_procedural_surface(&StandardAdmission, &id, ProceduralSurface::new(construction,
        ProceduralSurfaceDefinition::Offset(OffsetSurfaceConstruction::try_new(support, distance,
            None, None, false, OffsetExtension::Legacy { flags: LegacyExtensionFlags::Absent {}, cache: None }).unwrap()),
        None)).unwrap().unwrap();
    id
}

#[test]
fn cacheless_law_sweep_point_supplies_its_actual_requested_oriented_normal() {
    for (law, expected) in [
        (LawExpression::Double { value: 1.0 }, Point3::new(0.25, -2.0, 0.5)),
        (LawExpression::Text { value: cadmpeg_core::nonblank_literal!("2.0*X") },
            Point3::new(0.25, -1.0 - 1.0 / 5.0_f64.sqrt(), 0.5 - 2.0 / 5.0_f64.sqrt())),
    ] {
        let (mut ir, base) = law_sweep_model(law);
        let offset = shifted(&mut ir, base.clone(), 1.0);
        let index = ModelIndex::build(&ir, StandardIndex);
        let plain = crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
            &index, &base, 0.25, 0.5).unwrap();
        let actual = crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
            &index, &offset, 0.25, 0.5).unwrap();
        assert!(actual.get().distance(expected) <= EPS_DIRECT_NORMAL_POINT);
        let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Decode(&ctx),
            &index, &offset, 0.25, 0.5), Ok(actual));
        assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Decode(&ctx),
            &index, &base, 0.25, 0.5), Ok(plain));
        ctx.finish_session().unwrap();
    }
}

#[test]
fn cacheless_sweep_point_keeps_unavailable_normal_and_zero_offset_identity() {
    let law = LawExpression::Algebraic { operator: "ABS".into(), operands: vec![
        LawExpression::Text { value: cadmpeg_core::nonblank_literal!("X") }] };
    for distance in [0.0, 1.0] {
        let (mut ir, base) = law_sweep_model(law.clone());
        let offset = shifted(&mut ir, base.clone(), distance);
        let index = ModelIndex::build(&ir, StandardIndex);
        let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for mode in [admission::EvaluationAdmission::Standard, admission::EvaluationAdmission::Decode(&ctx)] {
            let plain = crate::eval::model_surface_point_by_id(mode, &index, &base, 0.25, 0.0).unwrap();
            assert_eq!(plain.get(), Point3::new(0.25, 0.0, 0.0));
            let actual = crate::eval::model_surface_point_by_id(mode, &index, &offset, 0.25, 0.0);
            if distance == 0.0 {
                let bits = |p: FinitePoint3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
                assert_eq!(bits(actual.unwrap()), bits(plain));
            } else { assert_eq!(actual, Err(EvaluationFailure::NoValue)); }
        }
        ctx.finish_session().unwrap();
    }
}

#[test]
fn requested_sweep_normal_keeps_the_original_refusal_and_point_only_work() {
    let (mut ir, base) = law_sweep_model(LawExpression::Double { value: 1.0 });
    let offset = shifted(&mut ir, base.clone(), 1.0);
    let index = ModelIndex::build(&ir, StandardIndex);
    // One surface visit, two actual curve visits and one scalar law visit.
    // Straight-path lookup, the null frame and fixed scale grammar are free.
    let work = WorkBudget::new(4);
    assert_eq!(admission::EvaluationAdmission::Standard.within_work_slice(&work, |mode|
        crate::eval::model_surface_point_by_id(mode, &index, &base, 0.25, 0.5)).unwrap().get(),
        Point3::new(0.25, -1.0, 0.5));
    assert_eq!(work.consumed(), 4);
    let mut policy = DecodePolicy::service(); policy.limits.max_work_units = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = ctx.charge_work_limit(1, "actual prior sweep-normal refusal").unwrap_err();
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    for id in [&base, &offset] {
        for (u, v) in [(0.25, 0.5), (f64::NAN, f64::NAN)] {
            assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Decode(&ctx),
                &index, id, u, v), Err(EvaluationFailure::ResourceLimit(original)));
        }
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}
