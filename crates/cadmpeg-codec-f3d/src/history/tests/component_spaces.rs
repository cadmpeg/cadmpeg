// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used, unused_imports)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::needless_pass_by_value
)]

use super::super::*;

#[test]
fn extrude_history_identity_resolves_only_in_context_component_breps() {
    fn history(blob: &str, state_id: i64, kind: AsmHistoricalEntityKind) -> AsmHistory {
        let mut topology = AsmHistoricalTopology::default();
        match kind {
            AsmHistoricalEntityKind::Edge => topology.edges.push(42),
            AsmHistoricalEntityKind::Loop => topology.loops.push(42),
            _ => panic!("test history kind"),
        }
        let id = crate::ids::native_scoped_id(
            &format!("Asset/Breps.BlobParts/{blob}"),
            "asm-history",
            0,
        );
        AsmHistory {
            states: vec![AsmDeltaState {
                id: format!("{id}:state#{state_id}"),
                parent: id.clone(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                entity_versions: Vec::new(),
                record_table_complete: true,
                topology: Some(topology),
                transition: None,
            }],
            id,
            byte_offset: 0,
            stream_size: None,
            history_entry_count: None,
            record_table_binding_budget_exceeded: false,
            projection_finalized: false,
        }
    }

    let design_stream = "Asset/Design1/BulkStream.dat";
    let naming_spaces = vec![
        crate::records::DesignComponentNamingSpace {
            id: crate::ids::native_design_component_naming_space_id(design_stream, 0),
            byte_offset: 0,
            component_record_index: 10,
            context_uuid: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into(),
            context_uuid_offset: 12,
        },
        crate::records::DesignComponentNamingSpace {
            id: crate::ids::native_design_component_naming_space_id(design_stream, 100),
            byte_offset: 100,
            component_record_index: 20,
            context_uuid: "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb".into(),
            context_uuid_offset: 112,
        },
    ];
    let binding = |at, entity_suffix, blob_name: &str| crate::records::DesignBodyBinding {
        id: crate::ids::native_design_body_binding_id(design_stream, at),
        stream: design_stream.into(),
        pair_count: 1,
        pair_ordinal: 0,
        asm_body_key: 1,
        asm_body_key_offset: at,
        entity_suffix,
        entity_suffix_offset: at + 8,
        blob_name: blob_name.into(),
        blob_name_offset: at + 16,
        body: None,
    };
    let body_bindings = vec![
        binding(200, 15, "BREP.a.smbh"),
        binding(300, 25, "BREP.b.smbh"),
    ];
    let histories = vec![
        history("BREP.a.smbh", 1, AsmHistoricalEntityKind::Edge),
        history("BREP.b.smbh", 2, AsmHistoricalEntityKind::Loop),
    ];
    let mut members = vec![crate::records::DesignExtrudeSelectionMember {
        id: crate::ids::native_scoped_id(design_stream, "extrude-selection-member", 400),
        group_record_index: 1,
        group_member_ordinal: 0,
        record_index: 2,
        byte_offset: 400,
        class_tag: "300".into(),
        local_id: 42,
        local_id_offset: 421,
        asset_id: "11111111-2222-4333-8444-555555555555".into(),
        asset_id_offset: 429,
        context_id: "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb".into(),
        context_id_offset: 505,
        tail_slot_present: false,
        tail_slot_offset: 581,
        resolved_geometry: None,
        operand_identity_ids: Vec::new(),
        historical_entity_kind: None,
        historical_entity_ref: None,
        historical_state_ids: Vec::new(),
        next_record_index: 3,
        next_byte_offset: 590,
    }];

    bind_extrude_selection_history(&mut members, &naming_spaces, &body_bindings, &histories);

    assert_eq!(
        members[0].historical_entity_kind,
        Some(AsmHistoricalEntityKind::Loop)
    );
    assert_eq!(members[0].historical_entity_ref, Some(42));
    assert_eq!(members[0].historical_state_ids, [2]);
}
