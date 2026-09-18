// SPDX-License-Identifier: Apache-2.0
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

use crate::history::{
    bind_mirror_selection_planes, design_geometry_mirror_plane, historical_loop_plane,
    historical_mirror_plane,
};
use crate::history_records::{
    AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalTopology, AsmHistory,
};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

#[test]
fn mirror_plane_candidate_uses_unique_primary_when_persistent_identity_is_absent() {
    let candidate = |history_id: &str, face_slot| {
        crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate {
            history_id: history_id.into(),
            historical: crate::records::topology::fillet::HistoricalBinding {
                kind: crate::records::topology::body_recipe::AsmHistoricalEntityKind::Loop,
                entity_ref: face_slot + 100,
                state_ids: vec![2, 1],
            },
            face_slot,
        }
    };
    let primary = candidate("history-a", 10);
    assert_eq!(
        super::super::unique_mirror_plane_candidate(vec![primary.clone()], Vec::new()),
        Some(primary.clone())
    );

    let second_primary = candidate("history-b", 20);
    assert_eq!(
        super::super::unique_mirror_plane_candidate(
            vec![primary.clone(), second_primary.clone()],
            Vec::new(),
        ),
        None
    );
    assert_eq!(
        super::super::unique_mirror_plane_candidate(
            vec![primary, second_primary.clone()],
            vec![second_primary.clone()],
        ),
        Some(second_primary.clone())
    );
    assert_eq!(
        super::super::unique_mirror_plane_candidate(
            vec![candidate("history-a", 10), second_primary.clone()],
            vec![candidate("history-a", 11), second_primary],
        ),
        None
    );
}

#[test]
fn mirror_plane_binding_falls_back_when_identity_has_no_persistent_value() {
    use crate::history_records::AsmHistoricalPlane;
    use cadmpeg_ir::math::{Point3, Vector3};
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#42",
        crate::records::feature::scope::DesignFeatureKind::Mirror,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Mirror(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::SymetrieMiroir(slot) =
        scope.payload_mut()
    {
        *slot = Some(
            serde_json::from_value(serde_json::json!({
                "count": 2, "count_record_index": 11, "count_offset": 0,
                "stitch_tolerance": 0.001, "stitch_tolerance_record_index": 12,
                "stitch_tolerance_offset": 0, "seed_group_record_index": 20,
                "plane_group_record_index": 30, "plane_selection_record_index": 40
            }))
            .expect("mirror construction"),
        );
    }
    let group: crate::records::topology::construction::DesignConstructionOperandGroup =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:Design/BulkStream.dat:group#30", "scope_record_index": 42,
            "scope_reference_ordinal": 0, "record_index": 30, "byte_offset": 0,
            "class_tag": "282", "members": [40], "member_offsets": [0],
            "frame": {"member_count_offset": 0, "opaque_index": 1,
                "opaque_index_offset": 18, "opaque_scalar": 0.0,
                "opaque_scalar_offset": 22, "variant": false},
            "role": 21_474_836_480u64, "role_offset": 0,
            "paired_class_tag": "261", "paired_byte_offset": 0
        }))
        .expect("mirror plane group");
    let mut operand: crate::records::topology::entity_selection::DesignEntitySelectionOperand =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:Design/BulkStream.dat:operand#40", "scope_record_index": 42,
            "group_record_index": 30, "group_member_ordinal": 0, "record_index": 40,
            "byte_offset": 0, "class_tag": "313", "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
            "asset_id_offset": 0, "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 0,
            "identity_record_index": 43, "identity_record_offset": 0,
            "primary_identity": 10, "primary_identity_offset": 21,
            "next_record_index": 42, "next_byte_offset": 29
        }))
        .expect("mirror plane selection");
    let state = |state_id, topology, transition| crate::history_records::AsmDeltaState {
        id: format!("history:state#{state_id}"),
        parent: "history".into(),
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
        topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
        transition,
    };
    let history = crate::history_records::AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![
            state(
                2,
                crate::history_records::AsmHistoricalTopology::default(),
                Some(crate::history_records::AsmHistoricalTransition {
                    previous_state_id: Some(1),
                    records: Default::default(),
                    topology: Default::default(),
                }),
            ),
            state(
                1,
                crate::history_records::AsmHistoricalTopology {
                    faces: vec![10],
                    face_surfaces: vec![crate::history_records::AsmHistoricalCarrierBinding {
                        entity: 10,
                        carrier: 20,
                    }],
                    surface_planes: vec![AsmHistoricalPlane {
                        surface: 20,
                        origin: Point3::new(1.0, 2.0, 3.0),
                        normal: Vector3::new(0.0, 0.0, 1.0),
                    }],
                    ..Default::default()
                },
                None,
            ),
        ],
    };

    bind_mirror_selection_planes(
        std::slice::from_mut(&mut scope),
        std::slice::from_ref(&group),
        std::slice::from_ref(&operand),
        &[],
        &[],
        std::slice::from_ref(&history),
    );

    let construction = scope.mirror_construction().expect("mirror construction");
    assert_eq!(
        construction.plane,
        Some(crate::records::feature::patterns::DesignPlane {
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        })
    );

    operand.primary_identity = 44;
    bind_mirror_selection_planes(
        std::slice::from_mut(&mut scope),
        std::slice::from_ref(&group),
        std::slice::from_ref(&operand),
        &[],
        &[],
        std::slice::from_ref(&history),
    );

    let construction = scope.mirror_construction().expect("mirror construction");
    assert_eq!(
        construction.plane,
        Some(crate::records::feature::patterns::DesignPlane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
        })
    );
}

#[test]
fn design_geometry_origin_plane_ids_use_coordinate_planes() {
    use cadmpeg_ir::math::{Point3, Vector3};

    for (identity, normal) in [
        (42, Vector3::new(0.0, 0.0, 1.0)),
        (43, Vector3::new(0.0, 1.0, 0.0)),
        (44, Vector3::new(1.0, 0.0, 0.0)),
    ] {
        let plane = design_geometry_mirror_plane(identity).expect("origin plane");
        assert_eq!(plane.origin, Point3::new(0.0, 0.0, 0.0));
        assert_eq!(plane.normal, normal);
    }
    assert!(design_geometry_mirror_plane(45).is_none());
}

#[test]
fn historical_loop_plane_requires_coincident_axis_bearing_curves() {
    use crate::history_records::{
        AsmHistoricalCoedge, AsmHistoricalCurveAxis, AsmHistoricalOptionalCarrierBinding,
        AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut topology = AsmHistoricalTopology {
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 5,
            member_refs: vec![6, 7],
        }],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 6,
                owner_loop: 5,
                edge: 10,
                next: 7,
                previous: 7,
                radial_next: 6,
            },
            AsmHistoricalCoedge {
                coedge: 7,
                owner_loop: 5,
                edge: 11,
                next: 6,
                previous: 6,
                radial_next: 7,
            },
        ],
        edge_curves: vec![
            AsmHistoricalOptionalCarrierBinding {
                entity: 10,
                carrier: Some(20),
            },
            AsmHistoricalOptionalCarrierBinding {
                entity: 11,
                carrier: Some(21),
            },
        ],
        curve_axes: vec![
            AsmHistoricalCurveAxis {
                curve: 20,
                origin: Point3::new(1.0, 2.0, 3.0),
                direction: Vector3::new(0.0, 0.0, 1.0),
            },
            AsmHistoricalCurveAxis {
                curve: 21,
                origin: Point3::new(4.0, 5.0, 3.0),
                direction: Vector3::new(0.0, 0.0, -1.0),
            },
        ],
        ..Default::default()
    };
    let plane = historical_loop_plane(5, &topology).expect("coincident loop curve planes");
    assert_eq!(plane.origin, Point3::new(1.0, 2.0, 3.0));
    assert_eq!(plane.normal, Vector3::new(0.0, 0.0, 1.0));

    topology.curve_axes[1].origin.z = 4.0;
    assert!(historical_loop_plane(5, &topology).is_none());
}

#[test]
fn historical_mirror_plane_requires_one_exact_plane_in_the_selected_state() {
    use crate::history_records::AsmHistoricalPlane;
    use cadmpeg_ir::math::{Point3, Vector3};

    let topology = || AsmHistoricalTopology {
        faces: vec![27],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 27,
            carrier: 41,
        }],
        surface_planes: vec![AsmHistoricalPlane {
            surface: 41,
            origin: Point3 {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            normal: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        }],
        ..AsmHistoricalTopology::default()
    };
    let state = |state_id, topology| AsmDeltaState {
        id: format!("history:state#{state_id}"),
        parent: "history".into(),
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
        topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
        transition: None,
    };
    let candidate =
        crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate {
            history_id: "history".into(),
            historical: crate::records::topology::fillet::HistoricalBinding {
                kind: AsmHistoricalEntityKind::Face,
                entity_ref: 69,
                state_ids: vec![2, 1],
            },
            face_slot: 27,
        };
    let mut history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![state(2, topology()), state(1, topology())],
    };

    let plane = historical_mirror_plane(&candidate, 1, std::slice::from_ref(&history))
        .expect("stable selected-face plane");
    assert_eq!(
        plane.origin,
        Point3 {
            x: 1.0,
            y: 2.0,
            z: 3.0
        }
    );
    assert!(historical_mirror_plane(&candidate, 3, std::slice::from_ref(&history)).is_some());
    history.states[0].topology_mut().unwrap().surface_planes[0]
        .normal
        .z = -1.0;
    assert!(historical_mirror_plane(&candidate, 1, std::slice::from_ref(&history)).is_some());
    assert!(historical_mirror_plane(&candidate, 3, std::slice::from_ref(&history)).is_some());
    history.states[0].topology_mut().unwrap().surface_planes[0]
        .origin
        .z = 4.0;
    assert!(historical_mirror_plane(&candidate, 3, std::slice::from_ref(&history)).is_none());
    let duplicate = history.states[1].topology().unwrap().face_surfaces[0].clone();
    history.states[1]
        .topology_mut()
        .unwrap()
        .face_surfaces
        .push(duplicate);
    assert!(historical_mirror_plane(&candidate, 1, &[history]).is_none());
}
