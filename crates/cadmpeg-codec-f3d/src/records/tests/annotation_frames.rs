// SPDX-License-Identifier: Apache-2.0

use crate::records::{
    DesignDimensionAnnotationFrame as Frame, DesignDimensionAnnotationFrameDraft as Draft,
    DesignDimensionAnnotationOperand as Operand, Located,
};
use std::num::NonZeroU32;

fn draft(base: u64) -> Draft {
    Draft {
        id: "annotation".into(),
        companion_record_index: None,
        governing_companion_record_index: 2,
        byte_offset: base,
        class_tag: "256".to_owned().try_into().unwrap(),
        record_index: 3,
        frame_length: 300,
        operands: [0, 10, 11, 10]
            .into_iter()
            .enumerate()
            .map(|(ordinal, index)| Operand {
                geometry_record_index: NonZeroU32::new(index),
                geometry_reference_offset: base + 25 + ordinal as u64 * 15,
                role: 1,
                role_offset: base + 35 + ordinal as u64 * 15,
            })
            .collect(),
        entity_genesis: 0,
        annotation_bytes: vec![7, 8],
        annotation_byte_offset: base + 141,
        governing_owner_record_index: 4,
        governing_owner_reference_offset: base + 144,
        return_members: [10, 10, 11]
            .into_iter()
            .enumerate()
            .map(|(ordinal, index)| Located {
                value: NonZeroU32::new(index).unwrap(),
                offset: base + 159 + ordinal as u64 * 11,
            })
            .collect(),
        paired_class_tag: "259".to_owned().try_into().unwrap(),
        paired_byte_offset: base + 300,
        owner_reference: 5,
        owner_reference_offset: base + 320,
    }
}

#[test]
fn annotation_frame_preserves_nulls_duplicate_geometry_and_return_order() {
    let original = draft(100);
    let frame = Frame::try_new(original.clone()).unwrap();
    assert_eq!(frame.clone().into_draft(), original);
    assert_eq!(
        frame
            .clone()
            .into_draft()
            .return_members
            .iter()
            .map(|index| index.value.get())
            .collect::<Vec<_>>(),
        [10, 10, 11]
    );
    let wire = serde_json::to_value(&frame).unwrap();
    assert_eq!(serde_json::from_value::<Frame>(wire).unwrap(), frame);
    let mut all_null = original;
    for operand in &mut all_null.operands {
        operand.geometry_record_index = None;
    }
    all_null.return_members.clear();
    assert!(Frame::try_new(all_null).is_ok());
}

#[test]
fn annotation_frame_rejects_stale_offsets_and_changed_multisets() {
    let frame = Frame::try_new(draft(100)).unwrap();
    let wire = serde_json::to_value(&frame).unwrap();
    for field in [
        "annotation_byte_offset",
        "governing_owner_reference_offset",
        "paired_byte_offset",
        "owner_reference_offset",
    ] {
        let mut invalid = wire.clone();
        invalid[field] = (invalid[field].as_u64().unwrap() + 1).into();
        assert!(serde_json::from_value::<Frame>(invalid)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
    for field in ["geometry_reference_offset", "role_offset"] {
        let mut invalid = wire.clone();
        invalid["operands"][0][field] = 0.into();
        assert!(serde_json::from_value::<Frame>(invalid)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
    let mut invalid = wire.clone();
    invalid["return_member_offsets"][0] = 0.into();
    assert!(serde_json::from_value::<Frame>(invalid)
        .unwrap_err()
        .to_string()
        .contains("return_member_offsets"));
    let mut invalid = wire;
    invalid["return_members"][0] = 11.into();
    assert!(serde_json::from_value::<Frame>(invalid)
        .unwrap_err()
        .to_string()
        .contains("return_members"));
    let mut empty = draft(100);
    empty.operands.clear();
    assert!(Frame::try_new(empty).unwrap_err().contains("operands"));
}

#[test]
fn annotation_frame_rejects_unrepresentable_extents() {
    let valid = draft(u64::MAX - 320);
    assert_eq!(
        Frame::try_new(valid.clone())
            .unwrap()
            .owner_reference_offset(),
        u64::MAX
    );
    let mut overflow = valid;
    overflow.frame_length += 1;
    assert!(Frame::try_new(overflow)
        .unwrap_err()
        .contains("owner_reference_offset"));
    let mut overflow = draft(0);
    overflow.byte_offset = u64::MAX;
    assert!(Frame::try_new(overflow)
        .unwrap_err()
        .contains("annotation_byte_offset"));
}
