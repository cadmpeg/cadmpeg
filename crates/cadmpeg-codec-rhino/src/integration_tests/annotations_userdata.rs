// SPDX-License-Identifier: Apache-2.0
//! V5 text-extra class-userdata admission and retention contracts.

use cadmpeg_test_support::EditableDecodeResult;

use super::{assert_valid, decode};
use crate::chunks::ArchiveVersion;
use crate::test_support::e2_structure::text_record;
use crate::test_support::test_archive as support;
use crate::wire::Uuid;

const V5_TEXT_EXTRA: [u8; 16] = [
    0xd9, 0x04, 0x90, 0xa5, 0xdb, 0x86, 0x49, 0xf8, 0xbd, 0xa1, 0x90, 0x80, 0xb1, 0xf4, 0xe9, 0x76,
];
const OPENNURBS5_APPLICATION: [u8; 16] = [
    0xc8, 0xcd, 0xa5, 0x97, 0xd9, 0x57, 0x46, 0x25, 0xa4, 0xb3, 0xa0, 0xb5, 0x10, 0xfc, 0x30, 0xd4,
];

fn versioned_text_extra_payload(
    archive: ArchiveVersion,
    major: i32,
    include_fields: bool,
) -> Vec<u8> {
    let mut fields = Uuid::nil().to_wire().to_vec();
    fields.push(1);
    fields.extend(1_i32.to_le_bytes());
    fields.extend([0x11, 0x22, 0x33, 0x44]);
    if include_fields {
        fields.extend(0.375_f64.to_le_bytes());
    }
    fields.extend([0xaa, 0xbb]);

    let mut body = major.to_le_bytes().to_vec();
    body.extend(0_i32.to_le_bytes());
    body.extend(fields);
    crate::test_support::test_dump::crc_chunk(archive, 0x4000_8000, &body)
}

fn text_userdata(archive: ArchiveVersion, payload: &[u8]) -> Vec<u8> {
    crate::test_support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        Uuid::from_canonical(V5_TEXT_EXTRA).to_wire(),
        Uuid::from_canonical(OPENNURBS5_APPLICATION).to_wire(),
        50,
        202_608_010,
        payload,
    )
}

fn annotation(result: &EditableDecodeResult) -> &cadmpeg_ir::native::NativeRecord {
    let arena = &result.ir().native.namespace("rhino").unwrap().arenas()["annotations"];
    assert_eq!(arena.len(), 1);
    &arena[0]
}

fn assert_text_and_retention<'a>(
    result: &'a EditableDecodeResult,
    record: &[u8],
) -> &'a cadmpeg_ir::native::NativeRecord {
    let annotation = annotation(result);
    assert_eq!(annotation.field("kind"), Some(serde_json::json!("text")));
    let retained = result
        .source_fidelity()
        .retained_record("rhino:object:record#000000")
        .expect("text object record is retained");
    assert_eq!(retained.data(), Some(record));
    annotation
}

#[test]
fn current_v5_text_extra_reaches_annotation_native_fields() {
    let archive = ArchiveVersion::V5;
    let userdata = text_userdata(archive, &versioned_text_extra_payload(archive, 1, true));
    let record = text_record(archive, &userdata);
    let result = decode(support::archive_writer(
        "50",
        202_608_010,
        std::slice::from_ref(&record),
    ));

    let annotation = assert_text_and_retention(&result, &record);
    let extra = annotation
        .field("v5_text_extra")
        .expect("current V5 text extra");
    assert_eq!(extra["parent_text_uuid"], serde_json::Value::Null);
    assert_eq!(extra["draw_mask"], serde_json::json!(true));
    assert_eq!(extra["mask_color_source"], serde_json::json!(1));
    assert_eq!(
        extra["mask_color"],
        serde_json::json!(cadmpeg_ir::native::bytes::NativeBytes::from([
            17, 34, 51, 68
        ]))
    );
    assert_eq!(extra["border_offset_factor"], serde_json::json!(0.375));
    assert_valid(&result);
}

#[test]
fn future_v5_text_extra_keeps_annotation_and_drops_carrier() {
    let archive = ArchiveVersion::V5;
    let userdata = text_userdata(archive, &versioned_text_extra_payload(archive, 2, false));
    let record = text_record(archive, &userdata);
    let result = decode(support::archive_writer(
        "50",
        202_608_010,
        std::slice::from_ref(&record),
    ));

    let annotation = assert_text_and_retention(&result, &record);
    assert!(annotation.field("v5_text_extra").is_none());
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("V5 text-extra userdata") && loss.message.contains("unsupported")
    }));
    assert_valid(&result);
}

#[test]
fn malformed_v5_text_extra_keeps_annotation_and_drops_carrier() {
    let archive = ArchiveVersion::V5;
    let userdata = text_userdata(archive, &versioned_text_extra_payload(archive, 1, false));
    let record = text_record(archive, &userdata);
    let result = decode(support::archive_writer(
        "50",
        202_608_010,
        std::slice::from_ref(&record),
    ));

    let annotation = assert_text_and_retention(&result, &record);
    assert!(annotation.field("v5_text_extra").is_none());
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("V5 text-extra userdata")
            && loss.message.contains("could not be transferred")
    }));
    assert_valid(&result);
}
