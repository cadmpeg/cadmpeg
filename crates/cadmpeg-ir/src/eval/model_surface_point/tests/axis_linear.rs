// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::surface_request::{model_jet, SurfaceRequest};
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::surface_payloads::SubsetSurfaceConstruction;
use crate::transform::Transform;

fn publish(ir: &mut CadIr, name: &str, definition: ProceduralSurfaceDefinition) -> SurfaceId {
    let id = SurfaceId::mint(format!("test:model:normal-surface#{name}")).unwrap();
    let construction = ProceduralSurfaceId::mint(format!("test:model:normal-construction#{name}")).unwrap();
    ir.model.surfaces.push(Surface { id: id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }), source_object: None });
    ir.model.add_procedural_surface(&StandardAdmission, &id,
        ProceduralSurface::new(construction, definition, None)).unwrap().unwrap();
    id
}

fn shifted(ir: &mut CadIr, name: &str, base: SurfaceId) -> SurfaceId {
    publish(ir, name, ProceduralSurfaceDefinition::Offset(OffsetSurfaceConstruction::try_new(
        base, 1.0, None, None, false,
        OffsetExtension::Legacy { flags: LegacyExtensionFlags::Absent {}, cache: None }).unwrap()))
}

#[test]
fn linear_sweep_point_reads_the_actual_first_order_normal() { direct_normal(4); }

#[test]
fn axis_revolution_point_reads_the_actual_first_order_normal() { direct_normal(5); }

#[test]
fn axis_linear_point_normals_keep_subset_and_reflected_chart_orientation() {
    for case in 4..6 {
        for senses in [[true, true], [false, true], [true, false], [false, false]] {
            let (mut ir, base, _, _, _, _, _) = fixture(case);
            let subset = publish(&mut ir, "subset", ProceduralSurfaceDefinition::Subset(
                SubsetSurfaceConstruction::try_new(base, [[0.0, 1.0], [0.0, 1.0]],
                    Some(senses[0]), Some(senses[1]), None).unwrap()));
            let reflected = publish(&mut ir, "reflected", ProceduralSurfaceDefinition::Replica {
                source: subset, transform: Transform::affine([
                    [-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0],
                ]).unwrap(),
            });
            let offset = shifted(&mut ir, "reflected-offset", reflected);
            let index = ModelIndex::build(&ir, StandardIndex);
            // The source chart is (2 cos u, 2 sin u, v). Reflection changes
            // its cross-product orientation; one subset reversal changes it again.
            let expected_x = if senses[0] == senses[1] { -1.0 } else { -3.0 };
            let expected = Point3::new(expected_x, 0.0, 0.0);
            let point = crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
                &index, &offset, 0.0, 0.0).unwrap();
            assert_eq!(point.get(), expected);
            for request in [SurfaceRequest::First, SurfaceRequest::Second] {
                assert_eq!(model_jet(admission::EvaluationAdmission::Standard, &index,
                    &offset, 0.0, 0.0, request).unwrap().point, point);
            }
        }
    }
}

#[test]
fn axis_linear_point_never_reads_an_unrequested_missing_or_nonfinite_tangent() {
    for axis in [false, true] {
        for degenerate in [false, true] {
            let mut ir = CadIr::empty();
            let first = Point3::new(2.0, 0.0, 0.0);
            let last = if degenerate { first } else if axis {
                Point3::new(2.0, 0.0, 1.0)
            } else { Point3::new(3.0, 0.0, 0.0) };
            // The point at zero is first. For unequal poles the true tangent
            // has magnitude 1 / 2^-1074, which does not fit in binary64.
            let h = f64::from_bits(1);
            let nurbs = NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 1,
                vec![0.0, 0.0, h, h], vec![first, last], None, false).unwrap().unwrap();
            let curve = CurveId::mint("test:model:normal-curve#narrow").unwrap();
            ir.model.curves.push(Curve { id: curve.clone(), geometry: CurveGeometry::Solved(
                SolvedCurveGeometry::Nurbs(nurbs)), source_object: None });
            let z = Vector3::new(0.0, 0.0, 1.0);
            let definition = if axis {
                ProceduralSurfaceDefinition::AxisRevolution(AxisRevolutionSurfaceConstruction::try_new(
                    curve, Point3::new(0.0, 0.0, 0.0), z).unwrap())
            } else { ProceduralSurfaceDefinition::LinearSweep(LinearSweepSurfaceConstruction::try_new(curve, z).unwrap()) };
            let base = publish(&mut ir, "narrow", definition);
            let offset = shifted(&mut ir, "narrow-offset", base.clone());
            let index = ModelIndex::build(&ir, StandardIndex);
            let base_point = crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
                &index, &base, 0.0, 0.0).unwrap();
            assert_eq!(base_point.get(), first);
            let requested = model_jet(admission::EvaluationAdmission::Standard, &index,
                &base, 0.0, 0.0, SurfaceRequest::First).unwrap();
            assert_eq!(requested.point, base_point);
            let result = crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Standard,
                &index, &offset, 0.0, 0.0);
            if degenerate {
                assert_eq!(result, Err(EvaluationFailure::NoValue));
            } else {
                assert_eq!(requested.first, Err(EvaluationFailure::NonFinite(())));
                let Err(EvaluationFailure::NonFinite(point)) = result else {
                    panic!("the read non-finite tangent leaves no finite offset coordinate");
                };
                assert!(point.x.is_nan() && point.y.is_nan() && point.z.is_nan());
            }
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Decode(&ctx),
                &index, &base, 0.0, 0.0).unwrap(), base_point);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn axis_linear_normal_preserves_original_refusal_before_parameters_and_children() {
    for case in 4..6 {
        let (ir, _, offset, u, v, _, _) = fixture(case);
        let index = ModelIndex::build(&ir, StandardIndex);
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let original = ctx.charge_work_limit(1, "actual prior axis-linear normal refusal").unwrap_err();
        for (u, v) in [(u, v), (f64::NAN, f64::NAN)] {
            assert_eq!(crate::eval::model_surface_point_by_id(admission::EvaluationAdmission::Decode(&ctx),
                &index, &offset, u, v), Err(EvaluationFailure::ResourceLimit(original)));
        }
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
    }
}
