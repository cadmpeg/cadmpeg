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
                draft.reference_count_offset = draft.byte_offset + frame_length - 104;
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(Vec::new());
                draft.layout_fixture_references();
                draft.feature_ordinal_offset = draft.paired_byte_offset - 76;
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
    assert!(rigid_group_members_link(
        &scope(257),
        &read,
        true,
        &Default::default()
    ));
    assert!(!rigid_group_members_link(
        &scope(268),
        &read,
        true,
        &Default::default()
    ));
    assert!(rigid_group_members_link(
        &scope(268),
        &unread,
        true,
        &Default::default()
    ));
    assert!(rigid_group_members_link(
        &scope(268),
        &read,
        false,
        &Default::default()
    ));
}

#[test]
fn rigid_group_locator_version_mismatch_reports_a_scope_finding() {
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
    use crate::records::feature::assembly::{
        DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyOperandFrame,
        DesignAssemblyOperandPath, DesignAssemblyOperandPathLink, DesignAssemblyOperandQualifier,
        DesignQualifiedAssemblyOperand, TYPED_OCCURRENCE_PATH_TYPE_GUID,
    };
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignScopePayload,
    };
    use crate::records::identity::{Located, ReferenceRun};

    for (length, version, wrong_version, backlink, wrapper, matrix) in
        [(309, 2, 3, 201, 252, 73), (180, 3, 2, 152, 163, 23)]
    {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#3",
            DesignFeatureKind::AsBuilt,
            3,
        );
        let operands = [64, 67].map(|index| {
            let locator = 1000 * u64::from(index);
            let path_at = locator + length;
            let wrapper_at = path_at + 425;
            let path = DesignAssemblyOperandPath::try_new_typed(
                DesignAssemblyOperandPathLink {
                    locator_reference_offset: if index == 64 { 42 } else { 53 },
                    locator_record_index: index,
                    locator_class_tag: "283".to_owned().try_into().unwrap(),
                    locator_byte_offset: locator,
                    locator_scope_reference_offset: locator + backlink + 1,
                    wrapper_record_index: index + 2,
                    wrapper_reference_offset: locator + wrapper + 1,
                    wrapper_class_tag: "327".to_owned().try_into().unwrap(),
                    wrapper_byte_offset: wrapper_at,
                    path_reference_offset: wrapper_at + 27,
                },
                index + 1,
                "366".to_owned().try_into().unwrap(),
                Some(
                    TYPED_OCCURRENCE_PATH_TYPE_GUID
                        .to_owned()
                        .try_into()
                        .unwrap(),
                ),
                path_at,
                vec![Located {
                    value: "11111111-1111-1111-1111-111111111111"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    offset: path_at + 29,
                }],
                [105, 181, 265, 341]
                    .map(|offset| Located {
                        value: "22222222-2222-2222-2222-222222222222"
                            .to_owned()
                            .try_into()
                            .unwrap(),
                        offset: path_at + offset,
                    })
                    .to_vec(),
            )
            .unwrap();
            DesignQualifiedAssemblyOperand {
                frame: DesignAssemblyOperandFrame {
                    reference_record_index: if version == 3 { index } else { index + 900 },
                    reference_offset: locator + if version == 3 { 7 } else { 22 },
                    transform: cadmpeg_ir::transform::Transform::identity()
                        .rows()
                        .try_into()
                        .unwrap(),
                    transform_offset: locator + matrix,
                },
                qualifier: DesignAssemblyOperandQualifier::OccurrencePath { path },
            }
        });
        scope
            .try_edit(|draft| {
                crate::design::test_support::lengthen_scope_frame(draft, 257);
                draft.reference_count_offset = 153;
                draft.reference_members = ReferenceRun::unlocated(Vec::new());
                draft.layout_fixture_references();
                draft.feature_ordinal_offset = draft.paired_byte_offset - 76;
                draft.payload = DesignScopePayload::AsBuilt(Some(
                    DesignAssemblyAlignment::try_new(
                        0.0,
                        [0.0; 3],
                        Vec::new(),
                        Some(DesignAssemblyAlignmentForm::Qualified(operands)),
                    )
                    .unwrap(),
                ));
            })
            .unwrap();
        let mut native = crate::native::F3dNative {
            design_record_headers: vec![crate::records::decal::DesignRecordHeader {
                id: "f3d:Design/BulkStream.dat:design-record-header#3".into(),
                record_index: 3,
                class_tag: scope.class_tag.clone(),
                byte_offset: scope.byte_offset(),
            }],
            design_types: vec![SegmentType::try_new(
                "f3d:Design/MetaStream.dat:design-type#0".into(),
                SegmentTypeData {
                    byte_offset: 0,
                    type_guid: "2E3AC990-483D-4A55-8662-C064B976A957"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    type_guid_offset: 4,
                    base_type_guid: BaseTypeGuid::Absent,
                    version,
                    version_offset: 44,
                    module: "Fusion".into(),
                    entities: ReferenceRun::unlocated(vec![64, 67]),
                },
            )
            .unwrap()],
            design_parameter_scopes: vec![scope],
            ..Default::default()
        };
        crate::test_support::with_decode_context(|decode| {
            let ir = cadmpeg_ir::examples::unit_cube().unwrap();
            let findings = |native: &crate::native::F3dNative| {
                let ctx = crate::validate::Ctx::new(&ir, native, decode);
                let mut findings = Vec::new();
                crate::validate::validate_parameter_scopes(&ctx, &mut findings).unwrap();
                findings
            };
            assert!(findings(&native).is_empty());
            native.design_types[0].set_version(wrong_version);
            let findings = findings(&native);
            assert_eq!(findings.len(), 1);
            assert_eq!(
                findings[0].check,
                cadmpeg_ir::report::check::Check::NativeLinks
            );
            assert_eq!(
                findings[0].entity.as_deref(),
                Some(native.design_parameter_scopes[0].id.as_str())
            );
            assert_eq!(
                findings[0].message,
                "Fusion Design parameter scope has an invalid paired frame"
            );
        });
    }
}
