// SPDX-License-Identifier: Apache-2.0
//! Fixtures shared by history and its archive or topology tests.

use crate::history_records::AsmHistoricalTopology;

pub(super) fn one_delta_state() -> Vec<u8> {
    let mut bytes = crate::history::DELTA.to_vec();
    for (tag, value) in [
        (0x04, 1_i32),
        (0x04, 1),
        (0x04, 0),
        (0x0c, -1),
        (0x0c, -1),
        (0x0c, 0),
        (0x0c, -1),
        (0x0c, 0),
    ] {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0x0b, 0x11]);
    bytes
}

pub(super) fn one_state_history() -> crate::history_records::AsmHistory {
    use crate::history_records::{AsmDeltaState, AsmHistory, AsmTopologyCache};

    AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "state".into(),
            parent: "history".into(),
            byte_offset: 0,
            state_id: 0,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 0,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: AsmTopologyCache::Absent,
            transition: None,
        }],
    }
}

pub(super) fn edge_context_topology() -> AsmHistoricalTopology {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalRelation,
        AsmHistoricalSurfaceAxis,
    };
    AsmHistoricalTopology {
        coedge_topology: vec![AsmHistoricalCoedge {
            coedge: 6,
            owner_loop: 5,
            edge: 7,
            next: 6,
            previous: 6,
            radial_next: 6,
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 5,
            member_refs: vec![6],
        }],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 4,
            member_refs: vec![5],
        }],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 4,
            carrier: 8,
        }],
        surface_axes: vec![AsmHistoricalSurfaceAxis {
            surface: 8,
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        }],
        ..Default::default()
    }
}

pub(super) fn one_face_reference() -> crate::records::dimensions::DesignRecipeReference {
    crate::records::dimensions::DesignRecipeReference {
        selector: 0,
        selector_offset: 0,
        token: "face".into(),
        token_offset: 0,
        design_reference: 1,
        design_reference_offset: 0,
        candidate_faces: vec![crate::ids::brep_face_id(1)],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    }
}

pub(super) fn with_history_decode_context<T>(
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    f(&ctx)
}

pub(super) fn active_body() -> cadmpeg_ir::topology::Body {
    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .model
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#1").unwrap();
    body
}

pub(super) fn base_feature() -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        BodySelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    };
    use cadmpeg_ir::ids::BodyId;
    Feature {
        id: FeatureId::mint("test:model:feature#feature").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Base Feature".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Native("native:scope".into()),
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(
                vec![
                    BodyId::mint("test:model:body#2").unwrap(),
                    BodyId::mint("test:model:body#1").unwrap(),
                ],
                &cadmpeg_test_support::service_decode_context(),
            )
            .unwrap(),
        ),
        native_ref: Some("native:scope".into()),
    }
}

pub(super) fn output_binding_inputs() -> (
    cadmpeg_ir::features::Feature,
    crate::records::feature::scope::DesignParameterScope,
    crate::history_records::AsmHistory,
    cadmpeg_ir::topology::Body,
) {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalEntityDelta, AsmHistoricalRelation, AsmHistoricalTopology,
        AsmHistoricalTopologyDelta, AsmHistoricalTransition, AsmHistory, AsmTopologyCache,
    };
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut feature = base_feature();
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1",
        DesignFeatureKind::BaseFeature,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(1);
            draft.previous_history_state_id = Some(0);
            draft.layout_fixture_tail();
        })
        .unwrap();
    feature.native_ref = Some(scope.id.clone());
    let mut current_topology = AsmHistoricalTopology::default();
    current_topology.bodies.push(1);
    current_topology.body_regions.push(AsmHistoricalRelation {
        owner_ref: 1,
        member_refs: Vec::new(),
    });
    let mut delta = AsmHistoricalTopologyDelta::default();
    delta.bodies.inserted.push(1);
    let state = |state_id, node_index, next_ref, topology, transition| AsmDeltaState {
        id: format!("f3d:asm-delta-state#{state_id}"),
        parent: "f3d:asm-history#1".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref,
        node_index,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: AsmTopologyCache::Complete(topology),
        transition,
    };
    let history = AsmHistory {
        id: "f3d:asm-history#1".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![
            state(
                1,
                1,
                Some(0),
                current_topology,
                Some(AsmHistoricalTransition {
                    previous_state_id: Some(0),
                    records: AsmHistoricalEntityDelta::default(),
                    topology: delta,
                }),
            ),
            state(0, 0, None, AsmHistoricalTopology::default(), None),
        ],
    };
    (feature, scope, history, active_body())
}

pub(super) fn decode_with_limits(
    bytes: &[u8],
    policy: &cadmpeg_core::decode::DecodePolicy,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, policy).unwrap();
    crate::history::decode(
        &ctx,
        bytes,
        "history",
        cadmpeg_asm::kernel_header::RefWidth::Four,
        &policy.limits,
    )
    .unwrap_err()
}
