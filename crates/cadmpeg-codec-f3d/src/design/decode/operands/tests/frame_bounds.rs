// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::operands::next_scope_group_offset;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Located;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::DesignOperandRole;

fn group(
    stream: &str,
    scope_record_index: u32,
    byte_offset: u64,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("f3d:{stream}:design-construction-operand-group#{byte_offset}"),
        scope_record_index,
        scope_reference_ordinal: 0,
        record_index: 900,
        byte_offset,
        class_tag: crate::records::references::DesignClassTag::try_from("275".to_owned()).unwrap(),
        members: vec![Located {
            value: 901,
            offset: 0,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 0,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 99,
                opaque_index_offset: 18,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 22,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X10),
        role_offset: 0,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("258".to_owned())
            .unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap()
}

#[test]
fn a_member_frame_ends_at_the_next_group_of_its_own_scope() {
    let stream = "Design/BulkStream.dat";
    let scope = DesignParameterScope::empty(
        &format!("f3d:{stream}:design-parameter-scope#100"),
        DesignFeatureKind::SplitFace,
        304,
    );
    let groups = [
        group(stream, 304, 100),
        group(stream, 304, 500),
        group(stream, 304, 300),
        group(stream, 305, 200),
        group("Other/BulkStream.dat", 304, 250),
    ];
    let header = |byte_offset| DesignRecordHeader {
        id: format!("f3d:{stream}:design-record-header#{byte_offset}"),
        byte_offset,
        class_tag: crate::records::references::DesignClassTag::try_from("338".to_owned()).unwrap(),
        record_index: 311,
    };
    let stream = crate::ids::native_stream(&scope.id).unwrap();
    assert_eq!(
        next_scope_group_offset(&scope, stream, &header(150), &groups),
        Some(300)
    );
    assert_eq!(
        next_scope_group_offset(&scope, stream, &header(300), &groups),
        Some(500)
    );
    assert_eq!(
        next_scope_group_offset(&scope, stream, &header(600), &groups),
        None
    );
}
