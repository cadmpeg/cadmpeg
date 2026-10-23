// SPDX-License-Identifier: Apache-2.0
//! Rigid alignment scalar and member validation.

use crate::validate::{rigid_alignment_scalars_hold, rigid_group_members_link};

/// An owner-less rigid alignment holds only a zero angle and offset.
#[test]
fn rigid_alignment_without_owners_requires_zero_scalars() {
    use crate::records::feature::assembly::DesignAssemblyAlignment;
    let zero = DesignAssemblyAlignment::try_new(0.0, [0.0; 3], Vec::new(), None).unwrap();
    let turned = DesignAssemblyAlignment::try_new(0.5, [0.0; 3], Vec::new(), None).unwrap();
    let moved = DesignAssemblyAlignment::try_new(0.0, [0.0, 1.0, 0.0], Vec::new(), None).unwrap();
    assert!(rigid_alignment_scalars_hold(&zero, true));
    assert!(!rigid_alignment_scalars_hold(&turned, true));
    assert!(!rigid_alignment_scalars_hold(&moved, true));
    assert!(rigid_alignment_scalars_hold(&turned, false));
}

/// A rigid group's frame fixes its member count, so a group whose operands
/// were read needs the members after its first two. A group whose operands
/// were not read has no members to keep.
#[test]
fn rigid_group_frame_requires_its_further_members() {
    use crate::records::feature::assembly::{
        DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyOperandFrame,
    };
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let scope = |frame_length: u64| {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#3",
            DesignFeatureKind::AsBuilt,
            3,
        );
        scope
            .try_edit(|draft| {
                crate::design::test_support::lengthen_scope_frame(draft, frame_length);
            })
            .unwrap();
        scope
    };
    let frames = [10, 20].map(|index| DesignAssemblyOperandFrame {
        reference_record_index: index,
        reference_offset: 0,
        transform: cadmpeg_ir::transform::Transform::identity()
            .rows()
            .try_into()
            .unwrap(),
        transform_offset: 0,
    });
    let read = DesignAssemblyAlignment::try_new(
        0.0,
        [0.0; 3],
        Vec::new(),
        Some(DesignAssemblyAlignmentForm::Frames { frames }),
    )
    .unwrap();
    let unread = DesignAssemblyAlignment::try_new(0.0, [0.0; 3], Vec::new(), None).unwrap();
    assert!(rigid_group_members_link(&scope(269), &read, true));
    assert!(!rigid_group_members_link(&scope(280), &read, true));
    assert!(rigid_group_members_link(&scope(280), &unread, true));
    assert!(rigid_group_members_link(&scope(280), &read, false));
}
