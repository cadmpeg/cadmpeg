// SPDX-License-Identifier: Apache-2.0
use super::prelude::*;

#[test]
fn class_347_thicken_frame_admits_group_before_scalar() {
    fn put_indexed_header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    fn put_marked_reference(bytes: &mut [u8], offset: usize, record_index: u32) {
        bytes[offset] = 1;
        bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
    }

    let mut frame = vec![0; 291];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"347");
    frame[7..11].copy_from_slice(&1u32.to_le_bytes());
    frame[21..25].copy_from_slice(&4u32.to_le_bytes());
    frame[25..29].copy_from_slice(&1u32.to_le_bytes());
    put_marked_reference(&mut frame, 29, 200);
    frame[40..42].copy_from_slice(&[1, 1]);
    put_marked_reference(&mut frame, 42, 74);
    frame[53..57].copy_from_slice(&1u32.to_le_bytes());
    put_marked_reference(&mut frame, 57, 300);
    frame[76..80].copy_from_slice(&36u32.to_le_bytes());
    let guid = "00000000-0000-0000-0000-000000000000";
    for (ordinal, unit) in guid.encode_utf16().enumerate() {
        frame[80 + ordinal * 2..82 + ordinal * 2].copy_from_slice(&unit.to_le_bytes());
    }
    frame[155..159].copy_from_slice(&3u32.to_le_bytes());
    put_marked_reference(&mut frame, 159, 200);
    put_marked_reference(&mut frame, 170, 201);
    put_marked_reference(&mut frame, 181, 74);
    frame[192..196].copy_from_slice(&1u32.to_le_bytes());
    frame[196..200].copy_from_slice(&7u32.to_le_bytes());
    for (ordinal, unit) in "Thicken".encode_utf16().enumerate() {
        frame[200 + ordinal * 2..202 + ordinal * 2].copy_from_slice(&unit.to_le_bytes());
    }
    frame[214..218].copy_from_slice(&1u32.to_le_bytes());
    frame[245..249].copy_from_slice(&1u32.to_le_bytes());

    let mut bytes = frame;
    put_indexed_header(&mut bytes, *b"258", 1);
    let scalar_start = bytes.len();
    put_indexed_header(&mut bytes, *b"277", 74);
    bytes.resize(scalar_start + 104, 0);
    bytes[scalar_start + 40..scalar_start + 48].copy_from_slice(&(-1.0f64).to_le_bytes());
    put_indexed_header(&mut bytes, *b"261", 74);

    let mut scope = DesignParameterScope::empty(
        "f3d:test:thicken#1",
        crate::records::DesignFeatureKind::Thicken,
        1,
    );
    scope.class_tag = "347".into();
    scope.paired_class_tag = "258".into();
    scope.frame_length = 291;
    scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![200, 201, 74]);
    assert!(matches!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &scope),
        Some(DesignDirectFaceOperation::Thicken(crate::records::DesignThickenOperation {
            signed_thickness: -1.0,
            thickness_record_index: 74,
            ..
        }))
    ));

    scope.paired_class_tag = "259".into();
    assert_eq!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &scope),
        None
    );
}
