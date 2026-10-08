mod closure;
mod decode;
mod pcurves;

mod parameter_ranges;

#[test]
fn b5_annotation_admits_retained_strings_and_map_entries() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let operations = b5_collection_refusals(|ctx| {
        super::annotate(
            ctx,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            "catia:b5:point#0",
            "object_stream_b5_03",
            "05_08_01_vertex",
            cadmpeg_ir::Exactness::Derived,
        )
    });
    assert!(operations.contains("collect source provenance"));
    assert!(operations.contains("collect source exactness entities"));

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fixture fits the input limit");
    let error = super::annotate(
        &ctx,
        &mut cadmpeg_ir::AnnotationBuilder::new(),
        "catia:b5:point#0",
        "object_stream_b5_03",
        "05_08_01_vertex",
        cadmpeg_ir::Exactness::Derived,
    )
    .expect_err("an annotation identity must fit the retained byte limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "annotation stream name"));
}

#[test]
fn b5_derived_field_admits_both_exactness_entries() {
    let operations = b5_collection_refusals(|ctx| {
        crate::resource::derived_annotation(
            ctx,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            "catia:b5:vertex#0",
            "point",
        )
    });
    assert!(operations.contains("collect source exactness entities"));

    let annotations = crate::test_support::with_service_context(|ctx| {
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        crate::resource::derived_annotation(ctx, &mut annotations, "catia:b5:vertex#0", "point")?;
        Ok::<_, cadmpeg_core::CodecError>(annotations.build())
    })
    .expect("service budget admits the field note");
    assert!(annotations.exactness().contains_key("catia:b5:vertex#0"));
}

fn b5_collection_refusals(
    mut run: impl FnMut(
        &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<(), cadmpeg_core::CodecError>,
) -> std::collections::BTreeSet<&'static str> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let mut operations = std::collections::BTreeSet::new();
    let mut completed = false;
    for limit in 0..=256 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("B5 fixture fits the input limit");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                operations.insert(error.operation);
            }
            Ok(()) => {
                completed = true;
                break;
            }
            Err(error) => panic!("unexpected B5 refusal: {error}"),
        }
    }
    assert!(completed, "collection limit 256 must admit the B5 fixture");
    operations
}

#[test]
fn b5_ownership_charges_component_state_arrays() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("closed B5 triangle graph");
    let operations = b5_collection_refusals(|ctx| {
        assert!(super::faces::ownership_plan(ctx, &graph)?.is_some());
        Ok(())
    });
    assert!(operations.contains("catia b5 closed components"));
    assert!(operations.contains("catia b5 component edge marks"));
}

#[test]
fn b5_loop_orientation_charges_constraint_arrays() {
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
        .map(|(id, loop_)| {
            (
                *id,
                crate::test_support::with_service_context(|ctx| loop_.edge_senses(ctx))
                    .expect("service budget"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let operations = b5_collection_refusals(|ctx| {
        assert!(super::faces::orient_loop_members(ctx, &graph, reversed.clone())?.is_some());
        Ok(())
    });
    assert!(operations.contains("catia b5 loop orientation constraints"));
    assert!(operations.contains("catia b5 loop orientation assignments"));
}

#[test]
fn b5_plan_charges_loop_senses_and_index() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service budget")
    .expect("closed graph");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:test:unknown#b5-plan-senses".to_string())
        .expect("valid test identity");
    let operations = b5_collection_refusals(|ctx| {
        let _plan = build_plan(
            ctx,
            &graph,
            &payload,
            &mut crate::nurbs::LaneRefusals::new(),
        )?;
        Ok(())
    });
    assert!(operations.contains("catia_b5_loop_edge_senses"));
    assert!(operations.contains("catia_b5_transfer_loop_senses"));
}

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
fn b5_emit_points_refuses_collection_limit_before_model_arena_growth() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("closed B5 triangle graph");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
        .expect("identity grammar");
    let plan = crate::test_support::with_service_context(|ctx| {
        build_plan(
            ctx,
            &graph,
            &payload,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service resource budget")
    .expect("complete B5 plan");
    let mut cap = 0;
    let mut reached = false;
    for _ in 0..128 {
        match crate::test_support::with_collection_limit(cap, |ctx| {
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            let mut ir = cadmpeg_ir::CadIr::empty();
            super::vertices::emit_vertices(
                &mut ir,
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &graph,
                &plan,
                &mut admission,
            )
        }) {
            Err(cadmpeg_core::CodecError::ResourceLimit(error))
                if error.operation == "catia_b5_emit_points" =>
            {
                reached = true;
                break;
            }
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) => {
                cap = error
                    .used
                    .checked_add(error.additional)
                    .expect("bounded fixture");
            }
            other => panic!("point arena limit not reached: {other:?}"),
        }
    }
    assert!(reached, "point arena limit was not reached");
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
    .expect("service resource budget");
    assert!(admitted > 0);
}

#[test]
fn b5_pcurve_occurrence_groups_refuse_collection_limit_before_growth() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("closed B5 triangle graph");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
        .expect("identity grammar");
    let plan = crate::test_support::with_service_context(|ctx| {
        build_plan(
            ctx,
            &graph,
            &payload,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service resource budget")
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
    let refused = crate::test_support::with_collection_limit(0, run);
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_pcurve_occurrence_objects")
    );
    let admitted = crate::test_support::with_service_context(run).expect("service resource budget");
    assert!(!admitted.is_empty());
}

#[test]
fn b5_surface_id_map_refuses_collection_limit_before_growth() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("closed B5 triangle graph");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
        .expect("identity grammar");
    let make_plan = || {
        crate::test_support::with_service_context(|ctx| {
            build_plan(
                ctx,
                &graph,
                &payload,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("service resource budget")
        .expect("complete B5 plan")
    };
    let mut limited_plan = make_plan();
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let mut storage = ctx.reserve_scoped(0, "test_surface_ids")?;
        super::surfaces::emit_surfaces(
            &mut cadmpeg_ir::CadIr::empty(),
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &graph,
            &mut limited_plan,
            &mut admission,
            &mut storage,
        )
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_emitted_surface_ids")
    );
    let mut service_plan = make_plan();
    let admitted = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let mut storage = ctx.reserve_scoped(0, "test_surface_ids")?;
        let ids = super::surfaces::emit_surfaces(
            &mut cadmpeg_ir::CadIr::empty(),
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &graph,
            &mut service_plan,
            &mut admission,
            &mut storage,
        )?;
        storage.commit()?;
        Ok::<_, cadmpeg_core::CodecError>(ids)
    })
    .expect("service resource budget");
    assert!(!admitted.is_empty());
}

#[test]
fn b5_edge_id_map_refuses_collection_limit_before_growth() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("closed B5 triangle graph");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
        .expect("identity grammar");
    let make_plan = || {
        let mut plan = crate::test_support::with_service_context(|ctx| {
            build_plan(
                ctx,
                &graph,
                &payload,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("service resource budget")
        .expect("complete B5 plan");
        plan.exact_support_edges.clear();
        plan.exact_support_curves.clear();
        plan.edge_helix_plan.clear();
        plan
    };
    let surfaces = std::collections::BTreeMap::new();
    let mut cap = 1;
    let mut reached = false;
    for _ in 0..128 {
        let mut limited_plan = make_plan();
        match crate::test_support::with_collection_limit(cap, |ctx| {
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            super::edges::emit_edges(
                &mut cadmpeg_ir::CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &graph,
                &payload,
                &mut limited_plan,
                &surfaces,
                &mut admission,
            )
        }) {
            Err(cadmpeg_core::CodecError::ResourceLimit(error))
                if error.operation == "catia_b5_emitted_edge_ids" =>
            {
                reached = true;
                break;
            }
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) => {
                cap = error
                    .used
                    .checked_add(error.additional)
                    .expect("bounded fixture");
            }
            other => panic!("edge identity map limit not reached: {other:?}"),
        }
    }
    assert!(reached, "edge identity map limit was not reached");
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
    .expect("service resource budget");
    assert!(!emitted.is_empty());
}

#[test]
fn b5_region_id_map_refuses_collection_limit_before_growth() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("closed B5 triangle graph");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
        .expect("identity grammar");
    let plan = crate::test_support::with_service_context(|ctx| {
        build_plan(
            ctx,
            &graph,
            &payload,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service resource budget")
    .expect("complete B5 plan");
    let surfaces = std::collections::BTreeMap::new();
    let pcurves = std::collections::HashMap::new();
    let edges = std::collections::HashMap::new();
    let emitted = super::faces::EmittedFaceInputs {
        surface_ids: &surfaces,
        pcurve_uses: &pcurves,
        edge_ids: &edges,
    };
    let refused = crate::test_support::with_collection_limit(2, |ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        super::faces::emit_faces(
            &mut cadmpeg_ir::CadIr::empty(),
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &graph,
            &plan,
            &emitted,
            &mut admission,
        )
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_region_ids")
    );
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
    .expect("service resource budget");
    assert!(admitted);
}

#[test]
fn b5_face_loop_and_coedge_emission_refuse_each_collection_limit() {
    use cadmpeg_ir::ids::{EdgeId, SurfaceId};

    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("closed B5 triangle graph");
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
        .expect("identity grammar");
    let plan = crate::test_support::with_service_context(|ctx| {
        build_plan(
            ctx,
            &graph,
            &payload,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service resource budget")
    .expect("complete B5 plan");
    let surfaces = graph
        .surfaces
        .keys()
        .map(|&object_id| {
            (
                object_id,
                SurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("catia", "b5", "surface"),
                    object_id,
                ),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let edges = graph
        .vertices
        .edges()
        .keys()
        .map(|&object_id| {
            (
                object_id,
                EdgeId::compose(
                    &cadmpeg_ir::identity_namespace!("catia", "b5", "edge"),
                    object_id,
                ),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let pcurves = std::collections::HashMap::new();
    let emitted = super::faces::EmittedFaceInputs {
        surface_ids: &surfaces,
        pcurve_uses: &pcurves,
        edge_ids: &edges,
    };
    let operations = b5_collection_refusals(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let emitted = super::faces::emit_faces(
            &mut cadmpeg_ir::CadIr::empty(),
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &graph,
            &plan,
            &emitted,
            &mut admission,
        )?;
        assert!(emitted);
        Ok(())
    });
    for operation in [
        "catia_b5_coedge_ids_by_member",
        "catia_b5_oriented_coedge_ids",
        "catia_b5_loop_vertex_uses",
        "catia_b5_coedge_radial_occurrences",
        "catia_b5_emit_coedges",
    ] {
        assert!(
            operations.contains(operation),
            "missing collection charge: {operation}"
        );
    }
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
fn b5_loop_orientation_refuses_edge_group_collection_limit() {
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
        .map(|(id, loop_)| {
            (
                *id,
                crate::test_support::with_service_context(|ctx| loop_.edge_senses(ctx))
                    .expect("service budget"),
            )
        })
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
        panic!("edge groups exceed the collection limit");
    };
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia b5 orientation edge keys"));
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

fn assert_b5_incomplete_face_refusal(operation: &'static str) {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service fixture admission")
    .expect("closed triangle graph");
    graph.complete = false;
    assert!(!graph.faces.is_empty());
    let payload = cadmpeg_ir::ids::UnknownId::mint("catia:payload:unknown#test".to_string())
        .expect("identity grammar");
    let refusal = crate::test_support::with_work_refusal(operation, |ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let result = super::transfer(
            &mut cadmpeg_ir::CadIr::empty(),
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            graph.clone(),
            &payload,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut admission,
        );
        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    });
    assert!(
        matches!(refusal, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == operation)
    );
}

#[test]
fn b5_incomplete_face_retain_preserves_saved_refusal() {
    assert_b5_incomplete_face_refusal("catia_b5_incomplete_face_retain");
}

#[test]
fn b5_incomplete_face_loop_visits_preserves_saved_refusal() {
    assert_b5_incomplete_face_refusal("catia_b5_incomplete_face_loop_visits");
}

#[test]
fn b5_unique_face_loop_owner_retain_preserves_saved_refusal() {
    assert_b5_incomplete_face_refusal("catia_b5_unique_face_loop_owner_retain");
}

#[test]
fn b5_unique_face_loop_owner_visits_preserves_saved_refusal() {
    assert_b5_incomplete_face_refusal("catia_b5_unique_face_loop_owner_visits");
}

/// These fixtures survive their construction context and belong to test output.
fn build_plan(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    graph: &super::B5Graph,
    payload: &cadmpeg_ir::ids::UnknownId,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<super::TransferPlan>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "test_b5_plan_storage")?;
    let plan = super::build_plan(ctx, graph, payload, refusal, &mut storage)?;
    if plan.is_some() {
        storage.commit()?;
    }
    Ok(plan)
}

mod budget_tests;
