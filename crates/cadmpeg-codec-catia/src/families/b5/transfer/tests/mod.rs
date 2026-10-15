mod closure;
mod decode;
mod pcurves;

mod parameter_ranges;

#[test]
fn b5_derived_field_records_exactness() {
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    annotations
        .derived("catia:b5:vertex#0", "point")
        .expect("record the derived field note");
    let annotations = annotations.build();
    assert!(annotations.exactness().contains_key("catia:b5:vertex#0"));
}

#[test]
fn b5_topology_entity_limit_refuses_before_first_model_append() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("parse the graph")
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

#[test]
fn b5_closed_triangle_emits_its_vertex_points() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("parse the graph")
    .expect("closed B5 triangle graph");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
        .expect("identity grammar");
    let plan = crate::test_support::with_service_context(|ctx| {
        super::build_plan(
            ctx,
            &graph,
            &payload,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("build plan")
    .expect("complete B5 plan");
    let admitted = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let mut ir = cadmpeg_ir::CadIr::empty();
        super::vertices::emit_vertices(
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &graph,
            &plan,
            &mut admission,
        )?;
        Ok::<_, cadmpeg_core::CodecError>(ir.model.points.len())
    })
    .expect("emit vertices");
    assert!(admitted > 0);
}

#[test]
fn b5_closed_triangle_emits_its_pcurves() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("parse the graph")
    .expect("closed B5 triangle graph");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
        .expect("identity grammar");
    let plan = crate::test_support::with_service_context(|ctx| {
        super::build_plan(
            ctx,
            &graph,
            &payload,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("build plan")
    .expect("complete B5 plan");
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let mut ir = cadmpeg_ir::CadIr::empty();
        super::pcurves::emit_pcurves(
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &graph,
            &plan,
            &mut admission,
        )
    };
    let admitted = crate::test_support::with_service_context(run).expect("emit pcurves");
    assert!(!admitted.is_empty());
}

#[test]
fn b5_closed_triangle_emits_its_surfaces() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("parse the graph")
    .expect("closed B5 triangle graph");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
        .expect("identity grammar");
    let make_plan = || {
        crate::test_support::with_service_context(|ctx| {
            super::build_plan(
                ctx,
                &graph,
                &payload,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("build plan")
        .expect("complete B5 plan")
    };

    let mut service_plan = make_plan();
    let admitted = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        super::surfaces::emit_surfaces(
            &mut cadmpeg_ir::CadIr::empty(),
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &graph,
            &mut service_plan,
            &mut admission,
        )
    })
    .expect("emit surfaces");
    assert!(!admitted.is_empty());
}

#[test]
fn b5_triangle_edges_emit_without_exact_support_plans() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("parse the graph")
    .expect("closed B5 triangle graph");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
        .expect("identity grammar");
    let make_plan = || {
        let mut plan = crate::test_support::with_service_context(|ctx| {
            super::build_plan(
                ctx,
                &graph,
                &payload,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("build plan")
        .expect("complete B5 plan");
        plan.exact_support_edges.clear();
        plan.exact_support_curves.clear();
        plan.edge_helix_plan.clear();
        plan
    };
    let surfaces = std::collections::HashMap::new();
    let mut service_plan = make_plan();
    let emitted = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        super::edges::emit_edges(
            &mut cadmpeg_ir::CadIr::empty(),
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &graph,
            &payload,
            &mut service_plan,
            &surfaces,
            &mut admission,
        )
    })
    .expect("emit edges");
    assert!(!emitted.is_empty());
}

#[test]
fn b5_closed_triangle_transfers_into_an_empty_model() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("parse the graph")
    .expect("closed B5 triangle graph");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
        .expect("identity grammar");

    let admitted = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        super::transfer(
            &mut cadmpeg_ir::CadIr::empty(),
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            graph.clone(),
            &payload,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut admission,
        )
    })
    .expect("transfer the B5 graph");
    assert!(admitted);
}

#[test]
fn b5_closed_triangle_has_a_face_ownership_plan() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("parse the graph")
    .expect("closed B5 triangle graph");
    crate::test_support::with_service_context(|ctx| {
        assert!(super::faces::ownership_plan(ctx, &graph)
            .expect("service decode")
            .is_some());
    });
}

#[test]
fn b5_closed_triangle_loops_orient_from_their_edge_senses() {
    use std::collections::BTreeMap;
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("parse the graph")
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
}
