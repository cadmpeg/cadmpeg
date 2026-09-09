// SPDX-License-Identifier: Apache-2.0
//! `SurfaceStitch` history-selection tests.
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

use super::super::*;
use crate::records::topology::DesignOperandRole;

#[test]
fn surface_stitch_binds_all_unique_entity_face_candidates() {
    use crate::records::feature::DesignParameterScope;
    use crate::records::topology::{
        AsmHistoricalEntityKind, DesignConstructionOperandGroup,
        DesignConstructionOperandGroupFrame, DesignEntitySelectionFaceCandidate,
        DesignEntitySelectionOperand,
    };
    use cadmpeg_ir::features::{
        FaceSelection, Feature, FeatureDefinition, FeatureId, FeatureInputTopology,
    };

    let stream = "f3d:Design/BulkStream.dat";
    let scope_id = format!("{stream}:design-parameter-scope#42");
    let history_id = format!("{stream}/BREP.surface:asm-1");
    let mut scope = DesignParameterScope::empty(
        &scope_id,
        crate::records::feature::DesignFeatureKind::SurfaceStitch,
        42,
    );
    scope.history_state_id = Some(2);
    scope.previous_history_state_id = Some(1);
    scope.reference_members =
        crate::records::ReferenceRun::unlocated(vec![100, 200, 110, 210, 300, 301]);
    let group = |record_index, scope_reference_ordinal, member| DesignConstructionOperandGroup {
        id: format!("{stream}:design-construction-operand-group#{record_index}"),
        scope_record_index: 42,
        scope_reference_ordinal,
        record_index,
        byte_offset: 0,
        class_tag: crate::records::DesignClassTag::try_from("282".to_owned()).unwrap(),
        members: vec![crate::records::Located {
            value: member,
            offset: 0,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_records: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_records: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 0,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        operand_role: crate::records::topology::DesignConstructionOperandRole::Other(
            DesignOperandRole::ROLE_0X5,
        ),
        role_offset: 0,
        paired_class_tag: crate::records::DesignClassTag::try_from("261".to_owned()).unwrap(),
        paired_byte_offset: 0,
    };
    let groups = vec![group(100, 0, 200), group(110, 2, 210)];
    let operand = |group_record_index, record_index, face_slot| DesignEntitySelectionOperand {
        id: format!("{stream}:design-entity-selection-operand#{record_index}"),
        scope_record_index: 42,
        group_record_index,
        group_member_ordinal: 0,
        record_index,
        byte_offset: 0,
        class_tag: crate::records::DesignClassTag::try_from("377".to_owned()).unwrap(),
        asset_id: crate::records::DesignRelaxedGuidText::try_from(
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
        )
        .unwrap(),
        asset_id_offset: 0,
        context_id: crate::records::DesignRelaxedGuidText::try_from(
            "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
        )
        .unwrap(),
        context_id_offset: 0,
        identity_record_index: record_index + 1,
        identity_record_offset: 0,
        primary_identity: face_slot as u64,
        primary_identity_offset: 0,
        secondary: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: vec![DesignEntitySelectionFaceCandidate {
            history_id: history_id.clone(),
            historical: crate::records::topology::HistoricalBinding {
                kind: AsmHistoricalEntityKind::Coedge,
                entity_ref: face_slot,
                state_ids: vec![1],
            },
            face_slot,
        }],
        resolved_edge_slot: None,
        next_record_index: record_index + 2,
        next_byte_offset: 0,
    };
    let operands = vec![operand(100, 200, 30), operand(110, 210, 31)];
    let state = |state_id, transition| AsmDeltaState {
        id: format!("{history_id}:state#{state_id}"),
        parent: history_id.clone(),
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
        topology_cache: crate::history_records::AsmTopologyCache::Complete(
            AsmHistoricalTopology::default(),
        ),
        transition,
    };
    let history = AsmHistory {
        id: history_id.clone(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![
            state(
                2,
                Some(AsmHistoricalTransition {
                    previous_state_id: Some(1),
                    records: Default::default(),
                    topology: Default::default(),
                }),
            ),
            state(1, None),
        ],
    };
    let feature_id = FeatureId::mint("f3d:model:feature#42").expect("identity grammar");
    let mut feature = Feature::new(
        feature_id.clone(),
        0,
        FeatureDefinition::KnitSurface {
            faces: FaceSelection::Native(scope_id.clone()),
            merge_entities: Some(true),
            create_solid: Some(true),
            gap_tolerance: Some(cadmpeg_ir::features::NonNegativeLength::new(0.1).unwrap()),
        },
    );
    feature.native_ref = Some(scope_id.clone());
    let mut input_topologies = vec![FeatureInputTopology {
        id: crate::design::edge_resolve::feature_input_topology_id(&feature_id, 1),
        input_of: feature_id.clone(),
        bodies: (Vec::new()).try_into().unwrap(),
        faces: (Vec::new()).try_into().unwrap(),
        edges: (Vec::new()).try_into().unwrap(),
        vertices: (Vec::new()).try_into().unwrap(),
        native_ref: None,
    }];
    let mut ambiguous_feature = feature.clone();
    let mut ambiguous_operands = operands.clone();
    ambiguous_operands[1]
        .historical_face_candidates
        .push(DesignEntitySelectionFaceCandidate {
            history_id: "other-history/BREP.other:asm-1".into(),
            historical: crate::records::topology::HistoricalBinding {
                kind: AsmHistoricalEntityKind::Coedge,
                entity_ref: 99,
                state_ids: vec![1],
            },
            face_slot: 99,
        });
    let mut ambiguous_topologies = input_topologies.clone();

    bind_feature_face_selections(
        std::slice::from_mut(&mut feature),
        &mut input_topologies,
        std::slice::from_ref(&scope),
        &groups,
        &[],
        &operands,
        &[],
        std::slice::from_ref(&history),
    )
    .unwrap();

    let FeatureDefinition::KnitSurface {
        faces:
            FaceSelection::Historical {
                state,
                faces,
                native,
            },
        ..
    } = feature.evaluation.definition()
    else {
        panic!("SurfaceStitch face selection remains unresolved");
    };
    let prefix = crate::ids::history_input_prefix("42", 1);
    assert_eq!(
        state,
        &crate::design::edge_resolve::feature_input_topology_id(&feature_id, 1)
    );
    assert_eq!(
        faces.as_slice(),
        &vec![
            crate::ids::history_input_face_id(&prefix, 30),
            crate::ids::history_input_face_id(&prefix, 31),
        ]
    );
    assert_eq!(native.as_str(), &scope_id);
    assert_eq!(input_topologies[0].faces.as_slice(), faces.as_slice());

    bind_feature_face_selections(
        std::slice::from_mut(&mut ambiguous_feature),
        &mut ambiguous_topologies,
        std::slice::from_ref(&scope),
        &groups,
        &[],
        &ambiguous_operands,
        &[],
        std::slice::from_ref(&history),
    )
    .unwrap();
    assert!(matches!(
        ambiguous_feature.evaluation.definition(),
        FeatureDefinition::KnitSurface {
            faces: FaceSelection::Native(native),
            ..
        } if native == &scope_id
    ));
    assert!(ambiguous_topologies[0].faces.is_empty());
}
