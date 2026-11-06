// SPDX-License-Identifier: Apache-2.0

use crate::design::feature_project::reverse_normal::project_reverse_normal;
use crate::records::feature::body_ops::DesignReverseNormalOperation;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::identity::{Located, ReferenceRun};
use crate::records::references::DesignClassTag;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

const STREAM: &str = "f3d:native/BulkStream.dat";
const SCOPE: u32 = 700;
const GROUP: u32 = 704;
const BODIES: [u32; 2] = [707, 711];

fn scope(body_group: Option<u32>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#{SCOPE}"),
        DesignFeatureKind::ReverseNormal,
        SCOPE,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![GROUP, BODIES[0], BODIES[1]]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let DesignScopePayloadMut::ReverseNormal(slot) = scope.payload_mut() {
        *slot = body_group.map(|value| DesignReverseNormalOperation {
            body_group: Located { value, offset: 26 },
        });
    }
    scope
}

fn group(scope_reference_ordinal: u32, members: &[u32]) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{STREAM}:design-construction-operand-group#{GROUP}"),
        scope_record_index: SCOPE,
        scope_reference_ordinal,
        record_index: GROUP,
        byte_offset: 0,
        class_tag: DesignClassTag::try_from("329".to_owned()).unwrap(),
        members: members
            .iter()
            .copied()
            .enumerate()
            .map(|(index, value)| Located {
                value,
                offset: u64_from_index(index) * 11,
            })
            .collect(),
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
                opaque_index: 1,
                opaque_index_offset: 18,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 22,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A),
        role_offset: 0,
        paired_class_tag: DesignClassTag::try_from("268".to_owned()).unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap()
}

#[test]
fn body_group_at_the_first_reference_projects_a_shape_reversal() {
    let group = group(0, &BODIES);
    assert_eq!(
        project_reverse_normal(&scope(Some(GROUP)), std::slice::from_ref(&group)),
        Some(FeatureDefinition::Operation(
            FeatureOperation::ReverseShape {
                source: BodySelection::Native(group.id.clone()),
            }
        ))
    );
}

#[test]
fn body_group_outside_the_prefix_and_reference_table_stays_native() {
    assert_eq!(
        project_reverse_normal(&scope(None), &[group(0, &BODIES)]),
        None
    );
    assert_eq!(
        project_reverse_normal(&scope(Some(BODIES[0])), &[group(0, &BODIES)]),
        None
    );
    assert_eq!(
        project_reverse_normal(&scope(Some(GROUP)), &[group(1, &BODIES)]),
        None
    );
    assert_eq!(
        project_reverse_normal(&scope(Some(GROUP)), &[group(0, &[BODIES[1], BODIES[0]])]),
        None
    );
    assert_eq!(
        project_reverse_normal(&scope(Some(GROUP)), &[group(0, &BODIES[..1])]),
        None
    );
}
