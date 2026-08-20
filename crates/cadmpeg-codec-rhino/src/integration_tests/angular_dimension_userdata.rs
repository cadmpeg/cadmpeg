// SPDX-License-Identifier: Apache-2.0
//! V5 angular-dimension extension admission and retention contracts.

use super::{assert_valid, decode};
use crate::chunks::{ArchiveVersion, TCODE_CRC};
use crate::test_support as support;
use crate::wire::Uuid;

const OPENNURBS5_APPLICATION: Uuid = Uuid::from_canonical([
    0xc8, 0xcd, 0xa5, 0x97, 0xd9, 0x57, 0x46, 0x25, 0xa4, 0xb3, 0xa0, 0xb5, 0x10, 0xfc, 0x30, 0xd4,
]);
const EXPECTED_ANGLE: f64 = std::f64::consts::PI / 3.0;

fn plane() -> Vec<u8> {
    [
        0.0, 0.0, 0.0, // origin
        1.0, 0.0, 0.0, // x axis
        0.0, 1.0, 0.0, // y axis
        0.0, 0.0, 1.0, // z axis
        0.0, 0.0, 1.0, 0.0, // equation
    ]
    .into_iter()
    .flat_map(f64::to_le_bytes)
    .collect()
}

fn legacy_angular_payload(archive: ArchiveVersion) -> Vec<u8> {
    let mut annotation = 3_i32.to_le_bytes().to_vec();
    annotation.extend(0_i32.to_le_bytes());
    annotation.extend(plane());
    let points: [[f64; 2]; 4] = [
        [0.0, 0.0],
        [2.5, 0.0],
        [2.0, 3.464_101_615_137_754_4],
        [5.196_152_422_706_632, 3.0],
    ];
    annotation.extend((points.len() as i32).to_le_bytes());
    for point in points {
        annotation.extend(point[0].to_le_bytes());
        annotation.extend(point[1].to_le_bytes());
    }
    annotation.extend(support::test_dump::utf16_bytes("<>"));
    annotation.extend(1_i32.to_le_bytes());
    annotation.extend(4_i32.to_le_bytes());
    annotation.extend(1.5_f64.to_le_bytes());
    annotation.extend(0_i32.to_le_bytes());
    annotation.push(1);
    annotation.extend(support::test_dump::utf16_bytes("formula"));
    annotation.extend((-1_i32).to_le_bytes());
    annotation.extend(17_i32.to_le_bytes());
    let annotation = support::test_dump::anonymous_chunk(archive, 3, &annotation);
    let mut outer = annotation;
    outer.extend(EXPECTED_ANGLE.to_le_bytes());
    outer.extend(6.0_f64.to_le_bytes());
    support::test_dump::anonymous_chunk(archive, 0, &outer)
}

fn anonymous_major(archive: ArchiveVersion, major: i32, minor: i32, body: &[u8]) -> Vec<u8> {
    let mut payload = major.to_le_bytes().to_vec();
    payload.extend(minor.to_le_bytes());
    payload.extend(body);
    support::test_dump::crc_chunk(archive, 0x4000_8000, &payload)
}

fn dimension_extension_payload(archive: ArchiveVersion, major: i32, malformed: bool) -> Vec<u8> {
    let mut body = 2.5_f64.to_le_bytes().to_vec();
    if !malformed {
        body.extend(4.0_f64.to_le_bytes());
    }
    let child = anonymous_major(archive, major, 0, &body);
    let mut payload = child;
    payload.extend([0xde, 0xad]);
    payload
}

fn userdata(archive: ArchiveVersion, major: i32, malformed: bool) -> Vec<u8> {
    support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        crate::dimensions::V5_ANGULAR_EXTRA.to_wire(),
        OPENNURBS5_APPLICATION.to_wire(),
        50,
        202_608_010,
        &dimension_extension_payload(archive, major, malformed),
    )
}

fn dimension_record(archive: ArchiveVersion, userdata: &[u8]) -> Vec<u8> {
    let object_type = support::test_dump::short_chunk(archive, 0x8200_0071, 0x200);
    let class_uuid_wire = crate::dimensions::V5_ANGULAR.to_wire();
    let mut uuid_body = class_uuid_wire.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid_wire).to_le_bytes());
    let class_uuid = support::test_dump::long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data =
        support::test_dump::crc_chunk(archive, 0x0002_fffc, &legacy_angular_payload(archive));
    let class_end = support::test_dump::short_chunk(archive, 0x8002_7fff, 0);
    let class = support::test_dump::long_chunk(
        archive,
        0x0002_7ffa,
        &[class_uuid, class_data, userdata.to_vec(), class_end].concat(),
    );
    let object_end = support::test_dump::short_chunk(archive, 0x8200_007f, 0);
    support::test_dump::nested_crc_chunk(
        archive,
        0x2000_8070 | TCODE_CRC,
        &[object_type, class, object_end].concat(),
    )
}

fn assert_object_record(result: &cadmpeg_ir::codec::DecodeResult, record: &[u8]) {
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|value| value.id == "rhino:object:record#000000")
        .expect("angular dimension object record is retained");
    assert_eq!(retained.data.as_deref(), Some(record));
}

#[test]
fn current_v5_angular_dimension_extension_reaches_dimension_semantics() {
    let archive = ArchiveVersion::V5;
    let userdata = userdata(archive, 1, false);
    let record = dimension_record(archive, &userdata);
    let result = decode(support::archive_writer(
        "50",
        202_608_010,
        std::slice::from_ref(&record),
    ));

    assert_eq!(result.ir().model.semantic_annotations.len(), 1);
    let annotation = &result.ir().model.semantic_annotations[0];
    assert_eq!(annotation.runtime_type, "angular_dimension");
    assert_eq!(annotation.parameters["first_extension_offset"], "2.5");
    assert_eq!(annotation.parameters["second_extension_offset"], "4");
    assert_eq!(annotation.value, Some(EXPECTED_ANGLE));
    assert_object_record(&result, &record);
    assert_valid(&result);
}

#[test]
fn future_v5_angular_dimension_extension_keeps_record_and_drops_dimension() {
    let archive = ArchiveVersion::V5;
    let userdata = userdata(archive, 2, false);
    let record = dimension_record(archive, &userdata);
    let result = decode(support::archive_writer(
        "50",
        202_608_010,
        std::slice::from_ref(&record),
    ));

    assert!(result.ir().model.semantic_annotations.is_empty());
    assert_object_record(&result, &record);
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("dimension extension retained")
            && loss.message.contains("unsupported dimension chunk major")
    }));
    assert_valid(&result);
}

#[test]
fn malformed_v5_angular_dimension_extension_keeps_record_and_drops_dimension() {
    let archive = ArchiveVersion::V5;
    let userdata = userdata(archive, 1, true);
    let record = dimension_record(archive, &userdata);
    let result = decode(support::archive_writer(
        "50",
        202_608_010,
        std::slice::from_ref(&record),
    ));

    assert!(result.ir().model.semantic_annotations.is_empty());
    assert_object_record(&result, &record);
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("dimension extension retained")
            && loss.message.contains("exceeds bound")
    }));
    assert_valid(&result);
}
