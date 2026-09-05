// SPDX-License-Identifier: Apache-2.0
//! Body-selection history ownership tests.

#![allow(clippy::default_trait_access)]

use super::super::*;

#[test]
fn move_body_selection_uses_unique_owning_history() {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalEntityDelta, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
        AsmHistoricalTransition, AsmHistory,
    };
    use cadmpeg_ir::features::{BodySelection, Feature, FeatureDefinition, FeatureId};

    let mut scope = crate::records::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::DesignFeatureKind::Move,
        10,
    );
    scope.history_state_id = Some(42);
    scope.previous_history_state_id = Some(41);
    let group_id = "f3d:Design/BulkStream.dat:design-construction-operand-group#20";
    let group = crate::records::DesignConstructionOperandGroup {
        id: group_id.into(),
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        record_index: 20,
        byte_offset: 0,
        class_tag: "280".into(),
        members: vec![crate::records::Located { value: 21, offset: 0 }],
        lost_edge_references: Vec::new(),
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_records: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_records: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role: 0x0000_0004_0000_0000,
        extrude_role: None,
        role_offset: 0,
        paired_class_tag: "259".into(),
        paired_byte_offset: 0,
    };
    let topology = || AsmHistoricalTopology {
        bodies: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let state = |state_id: i64,
                 parent: &str,
                 transition: Option<AsmHistoricalTransition>|
     -> AsmDeltaState {
        AsmDeltaState {
            id: format!("f3d:{parent}:asm-delta-state#{state_id}"),
            parent: parent.into(),
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
            topology_cache: crate::history_records::AsmTopologyCache::Complete(topology()),
            transition,
        }
    };
    let mut transition = AsmHistoricalTransition {
        previous_state_id: Some(41),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    transition.topology.bodies.updated.push(1);
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: true,
        states: vec![
            state(42, "history", Some(transition)),
            state(41, "history", None),
        ],
    };
    let unrelated_history = AsmHistory {
        id: "unrelated-history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: true,
        states: vec![state(41, "unrelated-history", None)],
    };
    let histories = [history, unrelated_history];
    let mut feature = Feature::new(
        FeatureId("f3d:feature#move".into()),
        0,
        FeatureDefinition::MoveBody {
            bodies: BodySelection::Native(group_id.into()),
            translation: cadmpeg_ir::math::Vector3::new(1.0, 2.0, 3.0),
            rotation: None,
            copies: 0,
        },
    );
    feature.native_ref = Some(scope.id.clone());
    let inputs = FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: std::slice::from_ref(&group),
        body_recipe_operands: &[],
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &histories,
        bodies: &[],
        regions: &[],
        shells: &[],
    };
    bind_feature_body_selections(std::slice::from_mut(&mut feature), &inputs);

    let expected_body =
        crate::ids::history_input_body_id(&crate::ids::history_input_prefix("move", 41), 1);
    assert!(matches!(
        feature.definition,
        FeatureDefinition::MoveBody {
            bodies: BodySelection::Historical { ref bodies, ref native, .. },
            ..
        } if bodies == &[expected_body] && native == group_id
    ));
}
