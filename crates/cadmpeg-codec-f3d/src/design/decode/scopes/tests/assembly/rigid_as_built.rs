// SPDX-License-Identifier: Apache-2.0
//! The 269-byte rigid `As-built` scope: no reference table entries, no
//! parameter owners, and two wide operand-path locators.

use super::compact_wide::{push_header, push_lp_utf16, push_marked, push_wide_operand_path};

/// A 269-byte rigid as-built scope (record 10) whose operand
/// locators are records 64 and 67, then its paired header, the two wide
/// operand paths and the next header.
fn rigid_as_built_stream() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_header(&mut bytes, b"405", 10);
    bytes.resize(22, 0);
    push_marked(&mut bytes, 70);
    bytes.resize(37, 0);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    push_marked(&mut bytes, 64);
    push_marked(&mut bytes, 67);
    push_marked(&mut bytes, 9);
    push_lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&[0; 3]);
    // The empty reference table, then the history state and the kind.
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    push_lp_utf16(&mut bytes, "As-built");
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.resize(269, 0);
    push_header(&mut bytes, b"262", 10);
    bytes.resize(269 + 30, 0);
    push_wide_operand_path(&mut bytes, 64, 10, "11111111-1111-1111-1111-111111111111");
    // The second operand names a different construction record.
    let second = bytes.len();
    push_wide_operand_path(&mut bytes, 67, 10, "66666666-6666-6666-6666-666666666666");
    bytes[second + 57..second + 61].copy_from_slice(&901_u32.to_le_bytes());
    push_header(&mut bytes, b"300", 71);
    bytes
}

#[test]
fn rigid_as_built_scope_reads_its_two_wide_operand_paths_without_owners() {
    use crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment_with_types;
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID;
    let bytes = rigid_as_built_stream();
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let class_tag = crate::records::references::DesignClassTag::try_from("405".to_owned()).unwrap();
    let mut scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        &bytes, &records, 10, &class_tag, 0,
    )
    .unwrap()
    .expect("a scope with an empty reference table");
    assert_eq!(
        scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::AsBuilt
    );
    assert_eq!(scope.frame_length(), 269);
    assert_eq!(scope.reference_members().len(), 0);
    // Class tags are per-file dynamic values: the same scope under other tags
    // is the same rigid form, and the kind and frame length identify it.
    let mut retagged = bytes.clone();
    retagged[4..7].copy_from_slice(b"328");
    retagged[269 + 4..269 + 7].copy_from_slice(b"264");
    let retagged_records = crate::design::decode::sketch::IndexedRecordOffsets::build(&retagged);
    let retagged_class =
        crate::records::references::DesignClassTag::try_from("328".to_owned()).unwrap();
    let retagged_scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        &retagged,
        &retagged_records,
        10,
        &retagged_class,
        0,
    )
    .unwrap()
    .expect("the rigid form under other class tags");
    assert_eq!(retagged_scope.frame_length(), 269);
    assert_eq!(retagged_scope.reference_members().len(), 0);
    // The empty table is read only for the `As-built` kind.
    let mut other = bytes.clone();
    let kind_at = other
        .windows(4)
        .position(|window| window == [b'A', 0, b's', 0])
        .expect("the kind name");
    other[kind_at] = b'B';
    let other_records = crate::design::decode::sketch::IndexedRecordOffsets::build(&other);
    assert!(
        crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
            &other,
            &other_records,
            10,
            &class_tag,
            0,
        )
        .unwrap()
        .is_none()
    );
    scope.id = "f3d:Design/BulkStream.dat:design-parameter-scope#0".into();

    let locator = "2E3AC990-483D-4A55-8662-C064B976A957";
    let types = std::collections::HashMap::from([
        (64, (locator, 5)),
        (65, (TYPED_OCCURRENCE_PATH_TYPE_GUID, 4)),
        (67, (locator, 5)),
        (68, (TYPED_OCCURRENCE_PATH_TYPE_GUID, 4)),
    ]);
    let alignment = exact_assembly_alignment_with_types(&bytes, &records, &scope, &[], &types)
        .unwrap()
        .expect("rigid as-built alignment");
    assert_eq!(alignment.angle(), 0.0);
    assert_eq!(alignment.offset(), [0.0; 3]);
    let frames = alignment
        .operand_frames()
        .expect("identity frames at the flags");
    assert!(
        frames
            .iter()
            .all(|frame| frame.transform.rows()
                == cadmpeg_ir::transform::Transform::identity().rows())
    );
    let paths = alignment.operand_path_refs().expect("both operand paths");
    assert_eq!(
        paths.map(|path| path.occurrence_guids()[0].value.as_str().to_owned()),
        [
            "11111111-1111-1111-1111-111111111111".to_owned(),
            "66666666-6666-6666-6666-666666666666".to_owned()
        ]
    );
}
