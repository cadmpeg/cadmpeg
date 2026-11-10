// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::feature::extrude::{DesignExtrudeOperation, DesignExtrudePrologue};
use crate::records::feature::scope::{DesignFeatureKind, DesignScopePayloadMut};
use crate::records::topology::construction::{
    DesignConstructionOperandGroupDraft, DesignConstructionOperandGroupFrame,
    DesignConstructionOperandGroupFrameDraft, DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::DesignExtrudeFaceEncoding;

fn scope(extent: DesignExtrudeExtent, start: DesignExtrudeStart) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#80",
        DesignFeatureKind::Extrude,
        80,
    );
    if let DesignScopePayloadMut::Extrude(slot) = scope.payload_mut() {
        slot.get_or_insert_with(Default::default).extrude_prologue =
            Some(DesignExtrudePrologue::ReferenceAware {
                reference: None,
                operation: DesignExtrudeOperation::Cut,
                operation_offset: 28,
                direction_face_extend_values: [1, 1],
                side_extent_discriminators: [2, 0],
                side_extent_discriminator_offsets: [117, 280],
                first_side_target_ordinal: None,
                distance_tail: None,
                extent,
                direction_face_extend_offsets: [32, 36],
                direction_reversed: true,
                direction_reversed_offset: 40,
                solid_operation: true,
                solid_operation_offset: 41,
                start,
                start_offset: 42,
            });
    }
    scope
}

fn group(usage: DesignExtrudeFaceRole) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
        scope_record_index: 80,
        scope_reference_ordinal: 4,
        record_index: 90,
        byte_offset: 900,
        class_tag: "317".to_owned().try_into().unwrap(),
        members: vec![crate::records::identity::Located {
            value: 201,
            offset: 926,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 921,
                opaque_index: 1,
                opaque_index_offset: 971,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 975,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::ExtrudeFaces {
            encoding: DesignExtrudeFaceEncoding::Faces,
            usage,
        },
        role_offset: 953,
        paired_class_tag: "265".to_owned().try_into().unwrap(),
        paired_byte_offset: 1024,
    })
    .unwrap()
}

#[test]
fn missing_termination_keeps_the_prologue_and_reports_a_loss() {
    let scopes = [scope(
        DesignExtrudeExtent::OneSidedToFace,
        DesignExtrudeStart::FromFace,
    )];
    let before = scopes[0].extrude_prologue();
    let groups = [group(DesignExtrudeFaceRole::Start)];
    let losses = report_missing_terminations(&scopes, &groups);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.extrude-termination-missing"
    );
    assert!(before.is_some());
    assert_eq!(scopes[0].extrude_prologue(), before);
}

#[test]
fn complete_face_targets_keep_their_prologue() {
    for (extent, required) in [
        (DesignExtrudeExtent::OneSidedToFace, 1),
        (DesignExtrudeExtent::TwoSidedDistanceToFace, 1),
        (DesignExtrudeExtent::TwoSidedToFaces, 2),
    ] {
        let scopes = [scope(extent, DesignExtrudeStart::FromFace)];
        let mut groups = vec![group(DesignExtrudeFaceRole::Start)];
        for ordinal in 0..required {
            let mut termination = group(DesignExtrudeFaceRole::Termination);
            termination.record_index += ordinal + 1;
            groups.push(termination);
        }
        let before = scopes[0].extrude_prologue();
        assert!(report_missing_terminations(&scopes, &groups).is_empty());
        assert_eq!(scopes[0].extrude_prologue(), before);
    }
}

#[test]
fn shape_targets_and_distance_extents_keep_their_prologue() {
    for extent in [
        DesignExtrudeExtent::OneSidedToFace,
        DesignExtrudeExtent::OneSidedDistance,
    ] {
        let scopes = [scope(extent, DesignExtrudeStart::FromFace)];
        let mut shape = group(DesignExtrudeFaceRole::Termination);
        shape.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5);
        let groups = [group(DesignExtrudeFaceRole::Start), shape];
        let before = scopes[0].extrude_prologue();
        assert!(report_missing_terminations(&scopes, &groups).is_empty());
        assert_eq!(scopes[0].extrude_prologue(), before);
    }
}

#[test]
fn two_sided_face_targets_charge_nothing() {
    for extent in [
        DesignExtrudeExtent::TwoSidedToFaces,
        DesignExtrudeExtent::TwoSidedDistanceToFace,
    ] {
        let scopes = [scope(extent, DesignExtrudeStart::FromFace)];
        let groups = [group(DesignExtrudeFaceRole::Start)];
        assert!(report_missing_terminations(&scopes, &groups).is_empty());
    }
}
