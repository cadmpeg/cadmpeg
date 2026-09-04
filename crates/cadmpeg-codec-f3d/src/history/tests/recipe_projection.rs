// SPDX-License-Identifier: Apache-2.0
//! History-module unit tests.
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use super::super::*;

#[test]
fn projection_caches_end_after_history_consumers() {
    let transition = crate::history_records::AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: Default::default(),
        topology: Default::default(),
    };
    let state = AsmDeltaState {
        id: "history:state#2".into(),
        parent: "history".into(),
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
        record_table_complete: true,
        topology: Some(AsmHistoricalTopology::default()),
        transition: Some(transition.clone()),
    };
    let mut histories = [AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        stream_size: None,
        history_entry_count: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![state],
    }];

    discard_projection_caches(&mut histories);

    let state = &histories[0].states[0];
    assert!(histories[0].projection_finalized);
    assert!(state.entity_versions.is_empty());
    assert!(!state.record_table_complete);
    assert!(state.topology.is_none());
    assert_eq!(state.transition, Some(transition));

    let mut native = crate::native::F3dNative {
        asm_histories: histories.to_vec(),
        ..Default::default()
    };
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native.store(&mut namespace).expect("store native history");
    native = crate::native::F3dNative::load(&namespace).expect("load native history");
    assert!(native.asm_histories[0].projection_finalized);
}

#[test]
fn side_one_edge_uses_nonzero_references_and_ignores_second_side() {
    let side = |header_value, scalars: Vec<i32>| crate::records::DesignTopologyRecipeSide {
        field_count: std::num::NonZeroU32::new(3).unwrap(),
        header_value,
        scalars,
        payload_prefix: vec![0],
        payload_entry_count: 0,
        entries: Vec::new(),
    };
    let structure = crate::records::DesignEdgeRecipeStructure {
        root: 2,
        sides: vec![side(1, vec![0, 2]), side(3, vec![0, 0])],
    };
    let context =
        |reference_ordinal, shared_edge_slots| crate::records::DesignEdgeRecipeReferenceContext {
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
    let selector = crate::records::DesignEdgeRecipeSelectorContext {
        selector: 0,
        clause_entries: vec![None, None],
        clause_triplet_edge_slots: vec![None, None],
        incidence_matching_edge_slots: vec![41],
        unique_incidence_edge_slot: Some(41),
        boundary_count_matching_edge_slots: Vec::new(),
    };
    assert_eq!(
        side_one_recipe_edge(
            Some(&structure),
            &ambiguous_contexts,
            &[selector],
            &[40, 41, 42],
        ),
        Some(41)
    );
}
