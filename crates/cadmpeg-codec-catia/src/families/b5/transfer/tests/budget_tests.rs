// SPDX-License-Identifier: Apache-2.0
//! Cross-pass bookkeeping ownership.

#[test]
fn b5_transfer_bookkeeping_uses_a_live_materialized_owner() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("graph construction budget")
    .expect("closed topology");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:test:unknown#budget-plan".to_owned())
        .expect("identity grammar");
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
        crate::test_support::with_materialized_limit(0, |ctx| {
            let mut storage = ctx.reserve_scoped(0, "test_plan_storage")?;
            super::super::build_plan(
                ctx,
                &graph,
                &payload,
                &mut crate::nurbs::LaneRefusals::new(),
                &mut storage,
            )
        })
    else {
        panic!("plan bookkeeping refusal")
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes
    );
    assert_eq!(limit.operation, "catia b5 face ownership ids");
}

#[test]
fn b5_transfer_plan_preserves_finite_knot_admission() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("graph admission")
    .expect("closed topology");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:test:unknown#finite-plan".to_owned())
        .expect("identity grammar");
    crate::test_support::with_work_limit(u64::MAX, |ctx| {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "IR NURBS knot finiteness",
            None,
        );
        let mut storage = ctx.reserve_scoped(0, "test_plan_storage").expect("scratch owner");
        let plan = super::super::build_plan(
            ctx,
            &graph,
            &payload,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut storage,
        )
        .expect("finite knots need no repeated finiteness scan")
        .expect("closed transfer plan");
        assert_eq!(plan.pcurve_plan.len(), 3);
        for (geometry, _, _) in plan.pcurve_plan.values() {
            let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
                panic!("native pcurve must keep its NURBS carrier")
            };
            assert_eq!(nurbs.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
        }
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn b5_extrusion_support_preserves_finite_knot_admission() {
    use crate::families::b5::graph::{B5ExtrusionDirectrix, B5ExtrusionSurface};
    use crate::test_support::test_b5::{finite_pair, increasing, increasing_bounds, unit};

    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("graph admission")
    .expect("closed topology");
    graph.extrusion_surfaces.insert(
        900,
        B5ExtrusionSurface {
            object_id: 900,
            direction: unit([0.0, 0.0, 1.0]),
            parameter_bounds: increasing_bounds([[-1.0, 1.0], [0.0, 1.0]]),
            directrix: B5ExtrusionDirectrix::SurfaceCurve {
                object_id: 901,
                support: (100, 200, finite_pair([0.0, 1.0])),
                parameter_range: increasing([0.0, 1.0]),
            },
        },
    );
    crate::test_support::with_work_limit(u64::MAX, |ctx| {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "IR NURBS knot finiteness",
            None,
        );
        let extrusion = super::super::resolved_extrusion_surface(
            ctx,
            &graph,
            900,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("finite knots need no repeated finiteness scan")
        .expect("exact extrusion support");
        let super::super::ResolvedExtrusionDirectrix::SurfaceCurve { support, .. } =
            extrusion.directrix
        else {
            panic!("one-support extrusion directrix")
        };
        let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = support.pcurve else {
            panic!("native support pcurve must keep its NURBS carrier")
        };
        assert_eq!(support.surface_object_id, 100);
        assert_eq!(support.pcurve_parameter_range, [0.0, 1.0]);
        assert_eq!(nurbs.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
        assert!(ctx.resource_refusal().is_none());
    });
}
