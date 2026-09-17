// SPDX-License-Identifier: Apache-2.0
use super::exact_work_point_construction;
use super::POINT_DATA_TYPE_GUID;
use crate::design::test_support::dump::{
    lp_utf16, parse_parameter_scope, DesignParameterScope, DesignRecordHeader, HashMap,
    IndexedRecordOffsets,
};

/// A `WorkPoint` scope record, its paired header, and one point-data record
/// frame: the indexed header, the payload prologue with an optional property
/// block, the class-level members of `version`, a base-level run of `inputs`
/// references, and the second header that closes the frame.
fn work_point_stream(
    class_tag: &str,
    version: u32,
    property: bool,
    pick_point: Option<u64>,
    position: [f64; 3],
    reference_type: u32,
    inputs: u32,
) -> (Vec<u8>, DesignParameterScope, usize) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"427");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "WorkPoint");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);

    bytes.extend_from_slice(&(class_tag.len() as u32).to_le_bytes());
    bytes.extend_from_slice(class_tag.as_bytes());
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    if property {
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&6u32.to_le_bytes());
        bytes.extend_from_slice(b"pt_tag");
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&9u64.to_le_bytes());
    } else {
        bytes.push(0);
    }
    if version >= 2 {
        bytes.extend_from_slice(&0i32.to_le_bytes());
    }
    for _ in 0..2 {
        bytes.extend_from_slice(&f64::to_le_bytes(0.0));
    }
    if version >= 1 {
        match pick_point {
            Some(target) => {
                bytes.push(1);
                bytes.extend_from_slice(&target.to_le_bytes());
                bytes.extend_from_slice(&[0, 0]);
            }
            None => bytes.push(0),
        }
    }
    let position_at = bytes.len();
    for value in position {
        bytes.extend_from_slice(&f64::to_le_bytes(value));
    }
    bytes.extend_from_slice(&reference_type.to_le_bytes());
    if version >= 3 {
        for _ in 0..3 {
            bytes.extend_from_slice(&f64::to_le_bytes(-1.0));
        }
    }
    bytes.extend_from_slice(&inputs.to_le_bytes());
    for input in 0..inputs {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(70 + input).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&55u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: crate::records::references::DesignClassTag::try_from("427".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .expect("WorkPoint scope");
    (bytes, scope, position_at)
}

#[test]
fn work_point_reads_the_class_version_its_type_table_stores() {
    let (bytes, scope, position_at) =
        work_point_stream("282", 2, false, None, [4.0, 5.0, 6.0], 5, 1);
    let records = IndexedRecordOffsets::build(&bytes);
    let frame = exact_work_point_construction(
        &bytes,
        &records,
        &scope,
        &HashMap::from([(55, (POINT_DATA_TYPE_GUID, 2))]),
    )
    .expect("work point frame");
    assert_eq!(frame.position, [4.0, 5.0, 6.0]);
    assert_eq!(frame.position_offset, position_at as u64);
    // The stored version drives the read: a version that describes a
    // different member sequence does not yield this frame's coordinate.
    assert_ne!(
        exact_work_point_construction(
            &bytes,
            &records,
            &scope,
            &HashMap::from([(55, (POINT_DATA_TYPE_GUID, 0))])
        )
        .map(|frame| frame.position_offset),
        Some(position_at as u64)
    );
    // An unregistered entity falls back to the agreement sweep.
    assert_eq!(
        exact_work_point_construction(
            &bytes,
            &records,
            &scope,
            &HashMap::from([(9, (POINT_DATA_TYPE_GUID, 0))])
        ),
        exact_work_point_construction(&bytes, &records, &scope, &HashMap::new())
    );
}

#[test]
fn work_point_position_does_not_depend_on_the_segment_local_class_tag() {
    // A class tag is `256` plus an index into the segment's own type table,
    // so the point-data class wears a different tag in every segment. The
    // coordinate is the same wherever the type table names the class.
    for class_tag in ["282", "316", "364", "409", "424", "460", "468"] {
        let (bytes, scope, position_at) =
            work_point_stream(class_tag, 2, false, None, [7.5, 8.5, 9.5], 5, 1);
        let records = IndexedRecordOffsets::build(&bytes);

        let frame = exact_work_point_construction(
            &bytes,
            &records,
            &scope,
            &HashMap::from([(55, (POINT_DATA_TYPE_GUID, 2))]),
        )
        .unwrap_or_else(|| panic!("class tag {class_tag}"));
        assert_eq!(frame.position, [7.5, 8.5, 9.5], "class tag {class_tag}");
        assert_eq!(
            frame.position_offset, position_at as u64,
            "class tag {class_tag}"
        );
    }
}
