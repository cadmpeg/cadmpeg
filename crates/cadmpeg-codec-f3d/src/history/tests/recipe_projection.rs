// SPDX-License-Identifier: Apache-2.0
//! History-module unit tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use crate::history::{discard_projection_caches, side_one_recipe_edge};
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};

#[test]
fn projection_caches_end_after_history_consumers() {
    let transition = crate::history_records::AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: Default::default(),
        topology: Default::default(),
    };
    let state = AsmDeltaState {
        id: "f3d:test:history-state#2".into(),
        parent: "f3d:test:history#0".into(),
        byte_offset: 0,
        state_id: 2,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 2,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: vec![crate::history_records::AsmEntityVersion {
            entity_ref: 3,
            record_ref: 4,
        }],
        topology_cache: crate::history_records::AsmTopologyCache::Complete(
            AsmHistoricalTopology::default(),
        ),
        transition: Some(transition.clone()),
    };
    let mut histories = [AsmHistory {
        id: "f3d:test:history#0".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![state],
    }];

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("decode context");
    discard_projection_caches(&ctx, &mut histories).expect("projection cache budget");

    let state = &histories[0].states[0];
    assert!(histories[0]
        .projection_finalized(&ctx)
        .expect("history finalization budget"));
    assert!(state.entity_versions.is_empty());
    assert!(!state.record_table_complete());
    assert!(state.topology().is_none());
    assert_eq!(state.transition, Some(transition));

    let mut native = crate::native::F3dNative {
        asm_histories: histories.to_vec(),
        ..Default::default()
    };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .expect("store native history");
    native = crate::native::F3dNative::load(&namespace).expect("load native history");
    assert!(native.asm_histories[0]
        .projection_finalized(&ctx)
        .expect("history finalization budget"));
}

#[test]
fn side_one_edge_uses_nonzero_references_and_ignores_second_side() {
    let side = |header_value, scalars: Vec<i32>| {
        crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
            header_value,
            scalars,
            payload_prefix: vec![0],

            entries: Vec::new(),
        }
    };
    let structure = crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
        root: 2,
        sides: vec![side(1, vec![0, 2]), side(3, vec![0, 0])],
    };
    let context = |reference_ordinal, shared_edge_slots| {
        crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
            reference_ordinal,
            result_faces: Vec::new(),
            result_face_boundaries: Vec::new(),
            result_shared_edge_slots: Vec::new(),
            preceding_faces: Vec::new(),
            preceding_face_boundaries: Vec::new(),
            preceding_support_face_slots: Vec::new(),
            preceding_support_face_boundaries: Vec::new(),
            shared_edge_slots,
            changed_shared_edge_slots: Vec::new(),
            changed_reference_edge_slots: Vec::new(),
        }
    };
    let contexts = vec![
        context(0, vec![40, 41]),
        context(1, vec![41, 42]),
        context(2, vec![99]),
    ];

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| side_one_recipe_edge(
            decode_ctx,
            Some(&structure),
            &contexts,
            &[],
            &[40, 41, 42]
        ))
        .unwrap(),
        Some(41)
    );

    let ambiguous_contexts = vec![
        context(0, vec![40, 41]),
        context(1, vec![40, 41]),
        context(2, vec![99]),
    ];
    let selector = crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
        selector: 0,
        clauses: vec![None, None],
        incidence_matching_edge_slots: vec![41],

        boundary_count_matching_edge_slots: Vec::new(),
    };
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| side_one_recipe_edge(
            decode_ctx,
            Some(&structure),
            &ambiguous_contexts,
            &[selector],
            &[40, 41, 42]
        ))
        .unwrap(),
        Some(41)
    );
}

#[test]
fn recipe_side_ordinals_refuse_collection_limit() {
    let structure = crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
        root: 2,
        sides: vec![
            crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
                header_value: 1,
                scalars: Vec::new(),
                payload_prefix: Vec::new(),
                entries: Vec::new(),
            },
        ],
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = side_one_recipe_edge(&ctx, Some(&structure), &[], &[], &[]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D recipe side ordinals")
    );
}

#[test]
fn a_history_with_no_states_has_released_nothing_and_is_not_finalized() {
    let document = serde_json::json!({
        "id": "history",
        "byte_offset": 0,
        "states": []
    });
    let history: AsmHistory = serde_json::from_value(document).unwrap();
    assert!(!crate::test_support::with_decode_context(|decode_ctx| {
        history.projection_finalized(decode_ctx)
    })
    .unwrap());
    assert!(!crate::test_support::with_decode_context(|decode_ctx| {
        crate::history::projection_was_finalized(decode_ctx, std::slice::from_ref(&history))
    })
    .unwrap());
}

#[test]
fn recipe_side_scans_and_deduplication_propagate_work_refusal() {
    let structure = crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
        root: 2,
        sides: vec![
            crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
                header_value: 1,
                scalars: vec![0, 2],
                payload_prefix: vec![0],
                entries: Vec::new(),
            },
        ],
    };
    let context = |reference_ordinal, shared_edge_slots| {
        crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
            reference_ordinal,
            result_faces: Vec::new(),
            result_face_boundaries: Vec::new(),
            result_shared_edge_slots: Vec::new(),
            preceding_faces: Vec::new(),
            preceding_face_boundaries: Vec::new(),
            preceding_support_face_slots: Vec::new(),
            preceding_support_face_boundaries: Vec::new(),
            shared_edge_slots,
            changed_shared_edge_slots: Vec::new(),
            changed_reference_edge_slots: Vec::new(),
        }
    };
    let contexts = [context(0, vec![40, 41]), context(1, vec![41, 42])];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        side_one_recipe_edge(ctx, Some(&structure), &contexts, &[], &[40, 41, 42])
    };
    assert_eq!(
        crate::test_support::with_decode_context(run).unwrap(),
        Some(41)
    );
    for operation in [
        "scan F3D recipe side scalars",
        "deduplicate F3D recipe side ordinals",
        "scan F3D recipe side ordinals",
        "scan F3D recipe side edge sets",
        "scan F3D recipe side edge intersections",
        "find F3D recipe side edge candidate",
        "find F3D terminal edge candidate",
        "deduplicate F3D recipe side edges",
    ] {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| run(ctx).map(|_| ()),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == operation)
        );
    }
}

#[test]
fn edge_recipe_reference_scans_and_membership_propagate_work_refusal() {
    let topology = AsmHistoricalTopology {
        faces: vec![4],
        face_loops: vec![crate::history_records::AsmHistoricalRelation {
            owner_ref: 4,
            member_refs: vec![5],
        }],
        loop_coedges: vec![crate::history_records::AsmHistoricalRelation {
            owner_ref: 5,
            member_refs: vec![6],
        }],
        coedge_topology: vec![crate::history_records::AsmHistoricalCoedge {
            coedge: 6,
            owner_loop: 5,
            edge: 7,
            next: 6,
            previous: 6,
            radial_next: 6,
        }],
        face_surfaces: vec![crate::history_records::AsmHistoricalCarrierBinding {
            entity: 4,
            carrier: 20,
        }],
        ..Default::default()
    };
    let reference = crate::records::dimensions::DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: "1".into(),
        token_offset: 0,
        design_reference: 1,
        design_reference_offset: 1,
        candidate_faces: vec![cadmpeg_ir::ids::FaceId::mint("f3d:asm-history:entity#4").unwrap()],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    };
    let changed = std::collections::HashSet::from([7]);
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::history::edge_recipe_reference_context(
            ctx,
            2,
            &reference,
            crate::history::EdgeBoundaryContext {
                topology: &topology,
                boundary_edges: &[7, 99],
            },
            crate::history::EdgeBoundaryContext {
                topology: &topology,
                boundary_edges: &[7, 98],
            },
            &changed,
        )
    };
    let output = crate::test_support::with_decode_context(run).unwrap();
    assert_eq!(output.result_shared_edge_slots, [7]);
    assert_eq!(output.shared_edge_slots, [7]);
    assert_eq!(output.changed_shared_edge_slots, [7]);
    assert_eq!(output.changed_reference_edge_slots, [7]);
    for operation in [
        "scan F3D result boundary edges",
        "find F3D result boundary edge",
        "scan F3D preceding boundary edges",
        "find F3D preceding boundary edge",
        "scan F3D shared edge slots",
        "find F3D changed shared edge",
        "scan F3D support face boundaries",
        "scan F3D support face loops",
        "scan F3D support loop members",
        "scan F3D preceding reference edges",
        "scan F3D support reference edges",
        "find F3D changed reference edge",
        "deduplicate F3D changed reference edges",
    ] {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| run(ctx).map(|_| ()),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == operation)
        );
    }
}
