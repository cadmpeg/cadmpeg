// SPDX-License-Identifier: Apache-2.0

use super::{attach_standalone_wires, CadIr, Curve, CurveGeometry, CurveId, Point3};
use cadmpeg_core::decode::{refusal_probe::RefusalProbe, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{analytic::LineCurve, SolvedCurveGeometry};
use cadmpeg_ir::ids::{PointId, VertexId};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::topology::{Point, Vertex};
use cadmpeg_ir::AnnotationBuilder;

fn wire_fixture(existing_endpoints: bool) -> (CadIr, [(CurveId, [f64; 2], usize); 1]) {
    let mut ir = CadIr::empty();
    let curve_id = CurveId::mint("catia:test:curve#candidate").expect("fixture identity");
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            LineCurve::try_new(Point3::new(2.0, 3.0, 5.0), Vector3::new(1.0, 0.0, 0.0))
                .expect("finite nonzero line"),
        )),
        source_object: None,
    });
    if existing_endpoints {
        ir.model.points = Vec::with_capacity(4);
        ir.model.vertices = Vec::with_capacity(4);
        for index in 0..3 {
            let point_id = PointId::mint(format!("catia:test:point#existing-{index}"))
                .expect("fixture identity");
            ir.model.points.push(Point::new(point_id.clone(), cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("finite fixture point"), None));
            ir.model.vertices.push(Vertex {
                id: VertexId::mint(format!("catia:test:vertex#existing-{index}"))
                    .expect("fixture identity"),
                point: point_id,
                tolerance: None,
            });
        }
        assert_eq!(ir.model.points.capacity(), 4);
        assert_eq!(ir.model.vertices.capacity(), 4);
    }
    (ir, [(curve_id, [0.0, 1.0], 17)])
}

fn assert_sticky(ctx: &DecodeContext<'_>, error: CodecError) -> cadmpeg_core::decode::ResourceLimit {
    let CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal required");
    };
    assert_eq!(ctx.resource_refusal(), Some(limit));
    let Err(CodecError::ResourceLimit(sticky)) = ctx.charge_work(1, "after wire refusal") else {
        panic!("original wire refusal must stay sticky");
    };
    assert_eq!(sticky, limit);
    limit
}

#[test]
fn wire_endpoint_pair_admits_growth_for_both_points_and_vertices() {
    for operation in ["catia_family_emit_points", "catia_family_emit_vertices"] {
        let (mut ir, wires) = wire_fixture(true);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admits endpoint boundary");
        let _probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
        let mut admission = super::super::FamilyEntityAdmission::new(&ctx);
        let error = attach_standalone_wires(
            &mut ir, &mut AnnotationBuilder::new(), &wires, &mut admission,
        ).expect_err("the second endpoint requires admitted vector growth");
        let limit = assert_sticky(&ctx, error);
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(limit.operation, operation);
        let item_size = if operation == "catia_family_emit_points" {
            std::mem::size_of::<Point>()
        } else {
            std::mem::size_of::<Vertex>()
        };
        assert_eq!(limit.additional, 4 * u64::try_from(item_size).expect("fixture slot bytes"));
        assert_eq!(ir.model.points.len(), if operation == "catia_family_emit_points" { 3 } else { 5 });
        assert_eq!(ir.model.vertices.len(), 3);
        assert!(ir.model.edges.is_empty());
    }
    let (mut ir, wires) = wire_fixture(true);
    crate::test_support::with_service_context(|ctx| {
        let mut admission = super::super::FamilyEntityAdmission::new(ctx);
        assert!(attach_standalone_wires(
            &mut ir, &mut AnnotationBuilder::new(), &wires, &mut admission,
        ).expect("service admits both endpoint pairs"));
        assert_eq!(admission.admitted(), 8);
    });
    assert_eq!(ir.model.points.len(), 5);
    assert_eq!(ir.model.vertices.len(), 5);
    assert_eq!(ir.model.points[3].position().get(), Point3::new(3.0, 3.0, 5.0));
    assert_eq!(ir.model.points[4].position().get(), Point3::new(2.0, 3.0, 5.0));
    assert_eq!(ir.model.vertices[3].point, ir.model.points[3].id);
    assert_eq!(ir.model.vertices[4].point, ir.model.points[4].id);
}

#[test]
fn selected_wire_carrier_stays_temporary_through_endpoint_identity_refusal() {
    let cap = {
        let (mut ir, wires) = wire_fixture(false);
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "catia_freeform_wire_point_id", None);
        crate::test_support::with_work_limit(u64::MAX, |ctx| {
            let mut admission = super::super::FamilyEntityAdmission::new(ctx);
            let error = attach_standalone_wires(
                &mut ir, &mut AnnotationBuilder::new(), &wires, &mut admission,
            ).expect_err("actual endpoint identity work is reached");
            let limit = assert_sticky(ctx, error);
            assert_eq!(limit.operation, "catia_freeform_wire_point_id");
            limit.used
        })
    };
    let (mut ir, wires) = wire_fixture(false);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cap;
    policy.limits.max_retained_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admits destination boundary");
    let _probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, "catia_freeform_wire_plan_carriers", None);
    let mut admission = super::super::FamilyEntityAdmission::new(&ctx);
    let error = attach_standalone_wires(
        &mut ir, &mut AnnotationBuilder::new(), &wires, &mut admission,
    ).expect_err("endpoint identity work refuses before carrier promotion");
    let limit = assert_sticky(&ctx, error);
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "catia_freeform_wire_point_id");
    assert!(ir.model.points.is_empty());
    assert!(ir.model.edges.is_empty());
}

#[test]
fn selected_wire_carrier_preserves_retained_refusal_at_edge_insertion() {
    let (mut ir, wires) = wire_fixture(false);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admits selected carrier boundary");
    let _probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, "catia_freeform_wire_plan_carriers", None);
    let mut admission = super::super::FamilyEntityAdmission::new(&ctx);
    let error = attach_standalone_wires(
        &mut ir, &mut AnnotationBuilder::new(), &wires, &mut admission,
    ).expect_err("selected actual carrier needs retained admission");
    let limit = assert_sticky(&ctx, error);
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "catia_freeform_wire_plan_carriers");
    assert_eq!(ir.model.points.len(), 2);
    assert_eq!(ir.model.vertices.len(), 2);
    assert!(ir.model.edges.is_empty());
}
