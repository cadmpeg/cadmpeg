// SPDX-License-Identifier: Apache-2.0

use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::test_support::indexed_header;

const STREAM: &str = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat";

fn group(scope_record_index: u32, record_index: u32) -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": format!("{STREAM}:group#{scope_record_index}-{record_index}"),
        "scope_record_index": scope_record_index,
        "scope_reference_ordinal": 0,
        "record_index": record_index,
        "byte_offset": 1000,
        "class_tag": "332",
        "members": [],
        "member_offsets": [],
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": DesignOperandRole::ROLE_0X10.raw(),
        "role_offset": 1054,
        "paired_class_tag": "259",
        "paired_byte_offset": 1125
    }))
    .unwrap()
}

#[test]
fn a_scope_group_record_is_not_a_face_operand() {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"306", 100);
    indexed_header(&mut bytes, *b"259", 100);
    indexed_header(&mut bytes, *b"408", 101);
    indexed_header(&mut bytes, *b"414", 102);
    indexed_header(&mut bytes, *b"423", 103);
    let recipe_at = bytes.len() + 4;
    bytes.extend_from_slice(&24u32.to_le_bytes());
    bytes.extend_from_slice(b"bounded_face_recipe_data");
    for value in [0i32, -1, 4, -1, -1, 2, 7, -1, -1, 2, 8, -1, -1, 2, 9] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    indexed_header(&mut bytes, *b"306", 104);
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#50"),
        DesignFeatureKind::Shell,
        50,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![100]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let header = DesignRecordHeader {
        id: format!("{STREAM}:design-record-header#100"),
        record_index: 100,
        class_tag: "306".to_owned().try_into().unwrap(),
        byte_offset: 0,
    };
    let recipe = ConstructionRecipe {
        id: format!("{STREAM}:construction-recipe#{recipe_at}"),
        byte_offset: u64::try_from(recipe_at).unwrap(),
        kind: ConstructionRecipeKind::BoundedFace,
        design: None,
        recipe_index: 0,
        record_index: None,
    };
    let archive = crate::test_support::body_reference_tail_test::archive(&bytes, &[]);
    crate::test_support::with_decode_context(|ctx| {
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let operands = |groups: &[DesignConstructionOperandGroup]| {
                crate::design::decode::operands::decode_face_operands(
                    ctx,
                    scan,
                    std::slice::from_ref(&scope),
                    groups,
                    std::slice::from_ref(&header),
                    std::slice::from_ref(&recipe),
                )
                .unwrap()
                .iter()
                .map(crate::records::topology::face::DesignFaceOperand::record_index)
                .collect::<Vec<_>>()
            };
            assert_eq!(operands(&[]), [100]);
            assert_eq!(operands(&[group(51, 100)]), [100]);
            assert_eq!(operands(&[group(50, 200)]), [100]);
            assert!(operands(&[group(50, 100)]).is_empty());
        });
    });
}

#[test]
fn terminal_profile_member_cannot_borrow_the_next_group_recipe() {
    use crate::records::identity::{Located, ReferenceRun};
    use crate::records::topology::construction::DesignConstructionOperandRole;

    let mut bytes = Vec::new();
    for index in [100, 100, 101, 102, 103] {
        indexed_header(&mut bytes, *b"306", index);
    }
    let next_group_at = bytes.len();
    indexed_header(&mut bytes, *b"332", 190);
    let face_at = bytes.len();
    for index in [200, 200, 201, 202, 203] {
        indexed_header(&mut bytes, *b"306", index);
    }
    bytes.extend_from_slice(&16u32.to_le_bytes());
    let recipe_at = bytes.len();
    bytes.extend_from_slice(b"face_recipe_data");
    for value in [0i32, -1] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    indexed_header(&mut bytes, *b"306", 204);
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#50"),
        DesignFeatureKind::Extrude,
        50,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 192;
            draft.paired_byte_offset = 192;
            draft.reference_members = ReferenceRun::unlocated(vec![90, 100, 190, 200]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let groups: Vec<_> = [(90, 100), (190, 200)]
        .into_iter()
        .map(|(record_index, member)| {
            let mut group = group(50, record_index);
            group.operand_role = DesignConstructionOperandRole::ExtrudeProfile;
            group
                .try_set_members(vec![Located {
                    value: member,
                    offset: 1026,
                }])
                .unwrap();
            group
        })
        .collect();
    let headers: Vec<_> = [(100, 0), (190, next_group_at), (200, face_at)]
        .into_iter()
        .map(|(record_index, offset)| DesignRecordHeader {
            id: format!("{STREAM}:design-record-header#{offset}"),
            record_index,
            class_tag: "306".to_owned().try_into().unwrap(),
            byte_offset: u64::try_from(offset).unwrap(),
        })
        .collect();
    let recipe = ConstructionRecipe {
        id: format!("{STREAM}:construction-recipe#{recipe_at}"),
        byte_offset: u64::try_from(recipe_at).unwrap(),
        kind: ConstructionRecipeKind::Face,
        design: None,
        recipe_index: 0,
        record_index: None,
    };
    let archive = crate::test_support::body_reference_tail_test::archive(&bytes, &[]);
    crate::test_support::with_decode_context(|ctx| {
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let operands = crate::design::decode::operands::decode_face_operands(
                ctx,
                scan,
                &[scope],
                &groups,
                &headers,
                &[recipe],
            )
            .unwrap();
            assert_eq!(
                operands
                    .iter()
                    .map(crate::records::topology::face::DesignFaceOperand::record_index)
                    .collect::<Vec<_>>(),
                [200]
            );
        });
    });
}
