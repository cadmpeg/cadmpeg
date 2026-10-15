// SPDX-License-Identifier: Apache-2.0
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::scope::{
    DesignExtrudeScope, DesignParameterScope, DesignParameterScopeDraft, DesignScopePayload,
};
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};

pub(in crate::design::feature_project) fn profile_group(
    index: u32,
    ordinal: u32,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("f3d:Design/BulkStream.dat:operand-group#{index}"),
        scope_record_index: 12,
        scope_reference_ordinal: ordinal,
        record_index: index,
        byte_offset: 1000,
        class_tag: "332".to_owned().try_into().unwrap(),
        members: vec![crate::records::identity::Located {
            value: index + 100,
            offset: 1026,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 1021,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 1072,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 1076,
                variant: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::ExtrudeProfile,
        role_offset: 1054,
        paired_class_tag: "259".to_owned().try_into().unwrap(),
        paired_byte_offset: 1125,
    })
    .unwrap()
}

pub(in crate::design::feature_project) fn scope() -> DesignParameterScope {
    scope_with_extent(DesignExtrudeExtent::OneSidedDistance)
}

pub(in crate::design::feature_project) fn scope_with_extent(
    extent: DesignExtrudeExtent,
) -> DesignParameterScope {
    DesignParameterScope::try_new(
        DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:scope#12".into(),
            byte_offset: 100,
            class_tag: "301".to_owned().try_into().unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 210,
            payload: DesignScopePayload::Extrude(Some(DesignExtrudeScope {
                extrude_prologue: Some(DesignExtrudePrologue::ReferenceAware {
                    reference: None,
                    operation: DesignExtrudeOperation::NewBody,
                    operation_offset: 128,
                    direction_face_extend_values: [1, 2],
                    side_extent_discriminators: [1, 0],
                    side_extent_discriminator_offsets: [177, 190],
                    first_side_target_ordinal: None,
                    extent,
                    direction_face_extend_offsets: [132, 136],
                    direction_reversed: false,
                    direction_reversed_offset: 140,
                    solid_operation: true,
                    solid_operation_offset: 141,
                    start: DesignExtrudeStart::ProfilePlane,
                    start_offset: 142,
                }),
                ..Default::default()
            })),
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 180,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![100, 101],
                vec![185, 196],
                "reference_members",
            )
            .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: "261".to_owned().try_into().unwrap(),
            paired_byte_offset: 300,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}
