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

use crate::history::discard_projection_caches;
use crate::history::face_boundaries::{side_one_common_edge, side_one_recipe_edge};
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};

#[test]
fn projection_caches_end_after_history_consumers() {
    let state = (
        AsmDeltaState {
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
            transition: None,
        },
        Some(AsmHistoricalTopology {
            edges: vec![3],
            ..Default::default()
        }),
        vec![crate::history_records::AsmEntityVersion {
            entity_ref: 3,
            record_ref: 4,
        }],
    );
    let mut histories = [crate::test_support::history::history(
        "f3d:test:history#0".into(),
        0,
        None,
        false,
        vec![state],
    )];

    let transition = histories[0].states()[0].transition.clone();
    assert_eq!(
        histories[0].entity_versions(&histories[0].states()[0]),
        vec![crate::history_records::AsmEntityVersion {
            entity_ref: 3,
            record_ref: 4
        }]
    );
    discard_projection_caches(&mut histories);

    let state = &histories[0].states()[0];
    assert!(crate::history::projection_was_finalized(&histories));
    assert!(histories[0].entity_versions(state).is_empty());
    assert_ne!(
        histories[0].topology_scope,
        crate::history_records::AsmTopologyScope::Complete
    );
    assert!(histories[0].topology(state).is_none());
    assert_eq!(state.transition, transition);

    let mut native = crate::native::F3dNative {
        asm_histories: histories.to_vec(),
        ..Default::default()
    };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &mut namespace,
        )
        .expect("store native history");
    native = crate::native::F3dNative::load(&namespace).expect("load native history");
    assert!(crate::history::projection_was_finalized(
        &native.asm_histories
    ));
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
            result_face_boundary_ids: Vec::new(),
            result_shared_edge_slots: Vec::new(),
            preceding_faces: Vec::new(),
            preceding_face_boundary_ids: Vec::new(),
            preceding_support_face_slots: Vec::new(),
            preceding_support_face_boundary_ids: Vec::new(),
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
        side_one_recipe_edge(Some(&structure), &contexts, &[], &[40, 41, 42]),
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
        side_one_recipe_edge(
            Some(&structure),
            &ambiguous_contexts,
            &[selector],
            &[40, 41, 42]
        ),
        Some(41)
    );
}

#[test]
fn side_one_single_face_boundary_resolves_without_incidence_candidates() {
    let program = [
        -1, -1, 2, 0, -1, 1, -1, 1, -1, 2, 1, -1, 0, -1, 0, 1, 0, 2, 1, 1, 1, 1, 1, 1, -1,
    ];
    let structure = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::recipe_structure::edge_recipe_structure(ctx, &program)
    })
    .unwrap()
    .unwrap();
    let context = |reference_ordinal, shared_edge_slots| {
        crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
            reference_ordinal,
            result_faces: Vec::new(),
            result_face_boundary_ids: Vec::new(),
            result_shared_edge_slots: Vec::new(),
            preceding_faces: Vec::new(),
            preceding_face_boundary_ids: Vec::new(),
            preceding_support_face_slots: Vec::new(),
            preceding_support_face_boundary_ids: Vec::new(),
            shared_edge_slots,
            changed_shared_edge_slots: Vec::new(),
            changed_reference_edge_slots: Vec::new(),
        }
    };
    let mut contexts = [context(0, vec![31]), context(1, vec![31, 32])];
    let selectors = [
        crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
            selector: 0,
            clauses: vec![None],
            incidence_matching_edge_slots: Vec::new(),
            boundary_count_matching_edge_slots: Vec::new(),
        },
    ];
    assert_eq!(
        side_one_recipe_edge(Some(&structure), &contexts, &selectors, &[31, 32]),
        Some(31)
    );
    assert_eq!(
        side_one_recipe_edge(Some(&structure), &contexts, &selectors, &[32]),
        None
    );
    contexts[0].shared_edge_slots.push(32);
    assert_eq!(
        side_one_recipe_edge(Some(&structure), &contexts, &selectors, &[31, 32]),
        None
    );
}

#[test]
fn side_one_common_edge_requires_one_shared_candidate_without_incidence_fallback() {
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
        sides: vec![side(0, vec![2, 1]), side(0, vec![1, 3])],
    };
    let context = |reference_ordinal, shared_edge_slots| {
        crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
            reference_ordinal,
            result_faces: Vec::new(),
            result_face_boundary_ids: Vec::new(),
            result_shared_edge_slots: Vec::new(),
            preceding_faces: Vec::new(),
            preceding_face_boundary_ids: Vec::new(),
            preceding_support_face_slots: Vec::new(),
            preceding_support_face_boundary_ids: Vec::new(),
            shared_edge_slots,
            changed_shared_edge_slots: Vec::new(),
            changed_reference_edge_slots: Vec::new(),
        }
    };
    // The first side names references 2 and 1; reference 3 is in a later side.
    let contexts = vec![
        context(0, vec![139, 255, 737, 757]),
        context(1, vec![737, 739, 741]),
        context(2, vec![737]),
    ];
    assert_eq!(
        side_one_common_edge(Some(&structure), &contexts, &[737, 739, 741, 757]),
        Some(737)
    );
    // The shared edge must be a candidate edge.
    assert_eq!(
        side_one_common_edge(Some(&structure), &contexts, &[739, 741]),
        None
    );
    // Two shared candidates stay unresolved.
    let ambiguous = vec![
        context(0, vec![139, 737]),
        context(1, vec![139, 737]),
        context(2, vec![]),
    ];
    assert_eq!(
        side_one_common_edge(Some(&structure), &ambiguous, &[139, 737]),
        None
    );
    // Disjoint reference sets select nothing.
    let disjoint = vec![context(0, vec![5423]), context(1, vec![5396])];
    assert_eq!(
        side_one_common_edge(Some(&structure), &disjoint, &[5396, 5423]),
        None
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
    assert!(history.states().is_empty());
    assert!(!crate::history::projection_was_finalized(
        std::slice::from_ref(&history)
    ));
}
