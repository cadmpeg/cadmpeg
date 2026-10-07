// SPDX-License-Identifier: Apache-2.0
//! Record-area and operation-frame fixtures.

use super::{
    operation_labels, operation_payload_strings, operation_payload_text_frames,
    operation_records_with_labels_and_ordinals, sections, size_framed_om_section,
    OperationPayloadTextFrame, OperationTextMarker,
};

fn legacy_feature_om_section_with_record_area() -> Vec<u8> {
    let mut bytes = vec![0xff; 16];
    bytes[12..14].copy_from_slice(b"OM");
    bytes.extend_from_slice(&[0, 1, 2]);
    let class_name = b"UGS::FEATURE_RECORD";
    bytes.push(u8::try_from(class_name.len() + 1).expect("fixture value fits u8"));
    bytes.extend_from_slice(class_name);
    bytes.push(0xa0);
    bytes.extend_from_slice(&[0x81, 0x21, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0x06]);
    let pointer_offset = bytes.len();
    let record_area_offset = pointer_offset + 20;
    bytes.push(0x01);
    bytes.extend_from_slice(
        &(u32::try_from(record_area_offset - 1).expect("fixture value fits u32")).to_le_bytes(),
    );
    bytes.resize(record_area_offset, 0);
    bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    bytes.extend_from_slice(b"\x01\x0eNX 1980.1700\0");
    bytes.extend_from_slice(
        b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x07UNITE\0",
    );
    let payload_len = u32::try_from(bytes.len() - 16).expect("fixture value fits u32");
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());
    bytes
}

#[test]
fn om_feature_section_accepts_the_legacy_record_area_pointer_and_product_frame() {
    let bytes = legacy_feature_om_section_with_record_area();
    let section = crate::test_support::with_decode_context(|ctx| sections(ctx, &bytes))
        .unwrap()
        .remove(0);
    let record_area_offset = section.record_area.expect("record area").offset;
    assert_eq!(
        record_area_offset,
        16 + 3 + 1 + b"UGS::FEATURE_RECORD".len() + 1 + 12 + 20
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| section.record_area_header(ctx))
            .unwrap()
            .expect("record header")
            .product
            .value
            .as_str(),
        "NX 1980.1700"
    );
    assert_eq!(section.operation_labels().len(), 1);
    assert_eq!(section.operation_labels()[0].value, "UNITE");

    let mut invalid = bytes;
    invalid[record_area_offset + 13] = 0x02;
    assert!(
        crate::test_support::with_decode_context(|ctx| sections(ctx, &invalid)).unwrap()[0]
            .record_area
            .is_none()
    );
}

#[test]
fn om_registry_uses_the_bounded_record_area_as_its_registry_end() {
    let mut bytes = size_framed_om_section();
    bytes.extend(std::iter::repeat_n(0xa5, 4097));
    bytes.extend_from_slice(&[
        u8::try_from(b"m_lateField".len() + 1).expect("fixture value fits u8"),
        b'm',
        b'_',
        b'l',
        b'a',
        b't',
        b'e',
        b'F',
        b'i',
        b'e',
        b'l',
        b'd',
        0x82,
    ]);
    let pointer_offset = bytes.len();
    let record_area_offset = pointer_offset + 20;
    bytes.extend_from_slice(
        &(u32::try_from(record_area_offset).expect("fixture value fits u32")).to_le_bytes(),
    );
    bytes.resize(record_area_offset, 0);
    bytes.extend_from_slice(&[13, 0, 0, 0, 14, 0, 0, 0, 44, 0, 0, 0]);
    bytes.extend_from_slice(b"\x05\x01\x0eNX 2027.3102\0");
    let payload_len = u32::try_from(bytes.len() - 16).expect("synthetic section fits");
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());

    let section = crate::test_support::with_decode_context(|ctx| sections(ctx, &bytes))
        .unwrap()
        .remove(0);
    assert_eq!(
        section.fields.last().expect("late field").name,
        "m_lateField"
    );
    assert_eq!(
        section.record_area.map(|area| area.offset),
        Some(record_area_offset)
    );
}

#[test]
fn om_operation_labels_require_the_complete_frame() {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x01\x82\x40\x90\x17\xd3\xff\x03\x07UNITE\0\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x02\x03\xff\xff\x03\x08SKETCH\0";
    let labels = operation_labels(bytes, 100);
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0].header.end_offset(), 122);
    assert_eq!(labels[0].header.offset(), 100);
    assert_eq!(labels[0].value, "UNITE");
    assert_eq!(
        labels[0].header.objects().values(),
        [Some(1), Some(576), Some(6099), None]
    );
    assert_eq!(labels[1].value, "SKETCH");
    assert_eq!(
        labels[1].header.objects().values(),
        [Some(2), Some(3), None, None]
    );

    assert!(operation_labels(b"\xff\xff\x03\x07UNITE\0", 0).is_empty());
    let mut invalid = bytes.to_vec();
    invalid[15] = 0x91;
    assert_eq!(operation_labels(&invalid, 0).len(), 1);
}

#[test]
fn om_operation_records_use_consecutive_validated_headers() {
    let bytes = b"prefix\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x07UNITE\0payload\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x08SKETCH\0tail";
    let labels = operation_labels(bytes, 10);
    let records_with_ordinals = operation_records_with_labels_and_ordinals(bytes, 10, &labels);
    let records = records_with_ordinals
        .iter()
        .map(|(_, record)| record)
        .collect::<Vec<_>>();
    assert_eq!(records_with_ordinals[0].0, 0);
    assert_eq!(records_with_ordinals[1].0, 1);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].offset(), 16);
    assert_eq!(records[0].label().value, "UNITE");
    assert!(records[0].bytes().ends_with(b"payload"));
    assert_eq!(records[0].payload(), b"payload");
    assert_eq!(records[0].payload_offset(), 43);
    assert_eq!(records[1].label().value, "SKETCH");
    assert!(records[1].bytes().ends_with(b"tail"));
    assert_eq!(records[1].payload(), b"tail");
}

#[test]
fn om_operation_payload_strings_require_complete_utf8_frames() {
    let label = "SIMPLE HOLE";
    let payload = b"\x00\x04\x07BLOCK\0\x04\x04\xc3\x97\0\x04\x07BROKEN";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let strings = operation_payload_strings(record);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[0].offset, 201);
    assert_eq!(strings[0].value.as_str(), "BLOCK");
    assert_eq!(strings[1].value.as_str(), "×");
}

#[test]
fn om_operation_payload_text_frames_retain_marker_and_order() {
    let label = "SYMBOLIC_THREAD";
    let payload = b"\x03\x05CUT\0\x04\x06DONE\0\x03\x0bM Profile\0";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let frames = operation_payload_text_frames(record);
    assert_eq!(
        frames,
        vec![
            OperationPayloadTextFrame {
                marker: OperationTextMarker::Text,
                offset: 200,
                value: crate::payload_text::PayloadText::new("CUT").unwrap(),
            },
            OperationPayloadTextFrame {
                marker: OperationTextMarker::String,
                offset: 206,
                value: crate::payload_text::PayloadText::new("DONE").unwrap(),
            },
            OperationPayloadTextFrame {
                marker: OperationTextMarker::Text,
                offset: 213,
                value: crate::payload_text::PayloadText::new("M Profile").unwrap(),
            },
        ]
    );
}
