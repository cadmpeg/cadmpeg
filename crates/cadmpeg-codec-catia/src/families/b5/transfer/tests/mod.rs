mod closure;
mod decode;
mod pcurves;

mod parameter_ranges;

#[test]
fn b5_transfer_propagates_ownership_collection_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("closed B5 triangle graph");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fixture fits the input limit");
    let mut admission = crate::families::FamilyEntityAdmission::new(&ctx);
    let mut ir = cadmpeg_ir::CadIr::empty();
    let error = super::transfer(
        &mut ir,
        &mut cadmpeg_ir::AnnotationBuilder::new(),
        graph,
        &cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
            .expect("identity grammar"),
        &mut crate::nurbs::LaneRefusals::new(),
        &mut admission,
    )
    .expect_err("B5 transfer carries the ownership collection refusal");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia b5 face ownership ids"));
    assert_eq!(ir.model.entity_count(), 0);
}

#[test]
fn b5_ownership_refuses_face_id_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("closed B5 triangle graph");
    crate::test_support::with_service_context(|ctx| {
        assert!(super::faces::ownership_plan(ctx, &graph)
            .expect("service decode")
            .is_some());
    });

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fixture fits the input limit");
    let Err(error) = super::faces::ownership_plan(&ctx, &graph) else {
        panic!("one face ownership id exceeds the collection limit");
    };
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia b5 face ownership ids"));
}

#[test]
fn b5_loop_orientation_refuses_loop_id_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeMap;

    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("closed B5 triangle graph");
    let reversed = graph
        .loops
        .iter()
        .map(|(id, loop_)| (*id, loop_.edge_senses()))
        .collect::<BTreeMap<_, _>>();
    crate::test_support::with_service_context(|ctx| {
        assert!(
            super::faces::orient_loop_members(ctx, &graph, reversed.clone())
                .expect("service decode")
                .is_some()
        );
    });

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fixture fits the input limit");
    let Err(error) = super::faces::orient_loop_members(&ctx, &graph, reversed) else {
        panic!("loop ids exceed the collection limit");
    };
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia b5 orientation loop ids"));
}

#[test]
fn b5_topology_entity_limit_refuses_before_first_model_append() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("closed B5 triangle graph");
    crate::test_support::with_entity_limit(0, |ctx| {
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let error = super::transfer(
            &mut ir,
            &mut annotations,
            graph,
            &cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
                .expect("identity grammar"),
            &mut crate::nurbs::LaneRefusals::new(),
            &mut admission,
        )
        .expect_err("a B5 model record exceeds the zero-entity allowance");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
                    && limit.operation == "admit CATIA family model entity"
        ));
        assert_eq!(ir.model.entity_count(), 0);
    });
}
