// SPDX-License-Identifier: Apache-2.0
//! Mirror-history unit tests.

use super::super::*;

#[test]
fn mirror_face_recipe_accepts_coincident_preceding_plane_faces() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalPlane, AsmHistoricalTopology,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let operand: crate::records::DesignFaceOperand = serde_json::from_value(serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:design-face-operand#40",
        "scope_record_index": 42,
        "scope_reference_ordinal": 5,
        "group_record_index": 30,
        "group_member_ordinal": 0,
        "record_index": 40,
        "byte_offset": 0,
        "class_tag": "276",
        "paired_byte_offset": 0,
        "paired_class_tag": "262",
        "recipe_record_index": 43,
        "recipe_record_byte_offset": 0,
        "recipe_id": "f3d:Design/BulkStream.dat:construction-recipe#43",
        "recipe_prefix_offset": 0,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_kind": "face",
        "recipe_program_offset": 0,
        "recipe_program": [0, -1],
        "recipe_node_offsets": [],
        "recipe_nodes": [],
        "preceding_candidate_faces": [
            "f3d:brep:entity#10",
            "f3d:brep:entity#11"
        ],
        "next_record_index": 44,
        "next_byte_offset": 0
    }))
    .expect("face-recipe operand");
    let topology = AsmHistoricalTopology {
        faces: vec![10, 11],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 20,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 21,
            },
        ],
        surface_planes: vec![
            AsmHistoricalPlane {
                surface: 20,
                origin: Point3::new(1.0, 2.0, 3.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
            AsmHistoricalPlane {
                surface: 21,
                origin: Point3::new(1.0, 2.0, 3.0),
                normal: Vector3::new(0.0, 0.0, -1.0),
            },
        ],
        ..Default::default()
    };
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        stream_size: None,
        history_entry_count: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![AsmDeltaState {
            id: "history:state#1".into(),
            parent: "history".into(),
            byte_offset: 0,
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 1,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            record_table_complete: true,
            topology: Some(topology),
            transition: None,
        }],
    };

    let plane = historical_mirror_face_operand_plane(&operand, &history, 1)
        .expect("coincident preceding faces share one mirror plane");
    assert_eq!(plane.origin, Point3::new(1.0, 2.0, 3.0));
    assert_eq!(plane.normal, Vector3::new(0.0, 0.0, 1.0));

    let mut noncoincident = history;
    noncoincident.states[0]
        .topology
        .as_mut()
        .expect("topology")
        .surface_planes[1]
        .origin
        .z = 4.0;
    assert!(historical_mirror_face_operand_plane(&operand, &noncoincident, 1).is_none());
}
