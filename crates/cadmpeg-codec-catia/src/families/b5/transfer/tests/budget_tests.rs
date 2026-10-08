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
