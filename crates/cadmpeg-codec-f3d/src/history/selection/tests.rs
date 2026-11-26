// SPDX-License-Identifier: Apache-2.0
use super::bind_face_selection;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_ir::features::selections::FaceSelection;

fn group(face_role: &str) -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:test:construction-group#20", "scope_record_index": 10,
        "scope_reference_ordinal": 2, "record_index": 20, "byte_offset": 0, "class_tag": "338",
        "members": [21], "member_offsets": [0],
        "frame": {
            "member_count_offset": 0, "auxiliary_record_indices": [],
            "auxiliary_record_offsets": [], "auxiliary_paths": [],
            "trailing_record_indices": [], "trailing_record_offsets": [],
            "trailing_transforms": [], "trailing_dual_transforms": [], "trailing_flags": [],
            "opaque_index": 1, "opaque_index_offset": 18, "opaque_scalar": 0.0,
            "opaque_scalar_offset": 22, "variant": false
        },
        "role": crate::records::topology::extrude_selection::DesignOperandRole::FACES.raw(),
        "extrude_role": "faces", "extrude_face_role": face_role, "role_offset": 0,
        "paired_class_tag": "261", "paired_byte_offset": 0
    }))
    .unwrap()
}

fn operand() -> DesignFaceOperand {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:test:face-operand#21", "scope_record_index": 10, "scope_reference_ordinal": 2,
        "group_record_index": 20, "group_member_ordinal": 0, "record_index": 21, "byte_offset": 0,
        "class_tag": "282", "paired_byte_offset": 12, "paired_class_tag": "259",
        "recipe_record_index": 24, "recipe_record_byte_offset": 24, "recipe_id": "f3d:test:recipe#22",
        "recipe_prefix_offset": 35, "recipe_prefix_bytes": "", "recipe_references": [],
        "recipe_kind": "bounded_face", "recipe_program_offset": 0, "recipe_program": [0, -1, 1],
        "recipe_node_offsets": [], "recipe_nodes": [], "next_record_index": 23, "next_byte_offset": 36
    }))
    .unwrap()
}

fn bind(group: &DesignConstructionOperandGroup, operand: &DesignFaceOperand) -> FaceSelection {
    let scope = DesignParameterScope::empty("f3d:test:scope#10", DesignFeatureKind::Extrude, 10);
    let mut selection = FaceSelection::Native(group.id.clone());
    bind_face_selection(
        &mut selection,
        &scope,
        std::slice::from_ref(group),
        std::slice::from_ref(operand),
        &[],
    );
    selection
}

fn faces(selection: &FaceSelection) -> Option<Vec<String>> {
    match selection {
        FaceSelection::Resolved { faces, .. } => {
            Some(faces.iter().map(|face| face.as_str().to_owned()).collect())
        }
        _ => None,
    }
}

#[test]
fn a_start_face_without_a_candidate_binds_the_resolved_slot() {
    let group = group("start");
    let mut operand = operand();
    // The fallback face was withdrawn, so no candidate lane names a face.
    assert_eq!(
        bind(&group, &operand),
        FaceSelection::Native(group.id.clone())
    );
    operand.resolved_face_slots = vec![107];
    assert_eq!(
        faces(&bind(&group, &operand)),
        Some(vec![crate::ids::brep_face_id(107).as_str().to_owned()])
    );
}

#[test]
fn a_start_face_candidate_lane_wins_over_the_resolved_slot() {
    let group = group("start");
    let mut operand = operand();
    operand.candidate_faces = vec![crate::ids::brep_face_id(5)];
    operand.preceding_candidate_faces = vec![crate::ids::brep_face_id(5)];
    operand.resolved_face_slots = vec![107];
    assert_eq!(
        faces(&bind(&group, &operand)),
        Some(vec![crate::ids::brep_face_id(5).as_str().to_owned()])
    );
}
