// SPDX-License-Identifier: Apache-2.0
//! Scope history binding through the body entity suffixes of a Base Feature.
#![allow(clippy::unwrap_used)]

use crate::history::scope_histories::bind_scope_histories;
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};

#[test]
fn ambiguous_base_feature_histories_use_the_blob_of_its_body_entity_suffix() {
    use crate::records::bodies::DesignBodyBinding;
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
    };

    let state = |history: &str, state_id: i64, previous_state_id: Option<i64>| {
        (
            AsmDeltaState {
                id: format!("{history}:asm-delta-state#{state_id}"),
                parent: history.into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: previous_state_id,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(AsmHistoricalTopology::default()),
            Vec::new(),
        )
    };
    let history = |source: &str| {
        let id = format!("f3d:asset/Breps.BlobParts/BREP.{source}.smbh:asm-history#1");
        crate::test_support::history::history(
            id.clone(),
            0,
            None,
            false,
            vec![state(&id, 4, Some(3)), state(&id, 3, None)],
        )
    };
    let histories = [history("first"), history("second")];
    let stream = "f3d:Design/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        DesignFeatureKind::BaseFeature,
        100,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(4);
            draft.previous_history_state_id = Some(3);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let DesignScopePayloadMut::BaseFeature(slot) = scope.payload_mut() else {
        panic!("Base Feature payload");
    };
    *slot = Some(
        serde_json::from_value(serde_json::json!({
            "body_entity_fields": ["000000000000"],
            "body_entity_suffix_offsets": [24],
            "body_entity_suffixes": [80],
            "body_reference_fields": ["000000000000"],
            "body_reference_record_offsets": [39],
            "body_reference_records": [90],
            "metadata_field": "000000000000",
            "metadata_record": 104,
            "metadata_record_offset": 81,
            "repeated_reference_fields": ["000000000000"],
            "result_fields": ["000000000000"],
            "result_record_offsets": [100],
            "result_records": [108]
        }))
        .unwrap(),
    );
    let binding = |entity_suffix, source: &str| {
        DesignBodyBinding::try_from(crate::records::bodies::DesignBodyBindingWire {
            component_owner: None,
            component_owner_unresolved: false,
            history_only: true,
            id: format!("{stream}:design-body-binding#0"),
            stream: "Design/BulkStream.dat".into(),
            pair_count: 1,
            pair_ordinal: 0,
            asm_body_key: 1,
            asm_body_key_offset: 0,
            entity_suffix,
            entity_suffix_offset: 8,
            blob_name: format!("BREP.{source}.smbh"),
            blob_name_offset: 16,
            body: None,
        })
        .unwrap()
    };
    let bind = |bindings: &[DesignBodyBinding]| {
        bind_scope_histories(
            std::slice::from_ref(&scope),
            bindings,
            &[],
            Default::default(),
            &histories,
            &mut Vec::new(),
        )
    };
    assert_eq!(bind(&[binding(80, "second")])[&scope.id], histories[1].id);
    assert!(!bind(&[binding(90, "second")]).contains_key(&scope.id));

    // A partial suffix match (suffix 80 resolves, suffix 81 does not) must not bind.
    let DesignScopePayloadMut::BaseFeature(slot) = scope.payload_mut() else {
        panic!("Base Feature payload");
    };
    *slot = Some(
        serde_json::from_value(serde_json::json!({
            "body_entity_fields": ["000000000000", "000000000000"],
            "body_entity_suffix_offsets": [24, 32],
            "body_entity_suffixes": [80, 81],
            "body_reference_fields": ["000000000000", "000000000000"],
            "body_reference_record_offsets": [39, 47],
            "body_reference_records": [90, 91],
            "metadata_field": "000000000000",
            "metadata_record": 104,
            "metadata_record_offset": 81,
            "repeated_reference_fields": ["000000000000", "000000000000"],
            "result_fields": ["000000000000", "000000000000"],
            "result_record_offsets": [100, 108],
            "result_records": [108, 109]
        }))
        .unwrap(),
    );
    let partial_bound = bind_scope_histories(
        std::slice::from_ref(&scope),
        &[binding(80, "second")],
        &[],
        Default::default(),
        &histories,
        &mut Vec::new(),
    );
    assert!(!partial_bound.contains_key(&scope.id));
    let both_bound = bind_scope_histories(
        std::slice::from_ref(&scope),
        &[binding(80, "second"), binding(81, "second")],
        &[],
        Default::default(),
        &histories,
        &mut Vec::new(),
    );
    assert_eq!(both_bound[&scope.id], histories[1].id);
}
