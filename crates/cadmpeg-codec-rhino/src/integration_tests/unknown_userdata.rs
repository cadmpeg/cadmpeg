// SPDX-License-Identifier: Apache-2.0
//! Unregistered class-userdata retention contracts.

use super::{assert_valid, decode};
use crate::chunks::{ArchiveVersion, TCODE_CRC};
use crate::test_support as support;
use crate::wire::Uuid;

const POINT_OBJECT_TYPE: i64 = 1;
const UNKNOWN_CLASS: Uuid = Uuid::from_canonical([
    0xaa, 0xaa, 0xaa, 0xaa, 0xbb, 0xbb, 0xcc, 0xcc, 0xdd, 0xdd, 0xee, 0xee, 0x11, 0x11, 0x22, 0x22,
]);
const UNKNOWN_ITEM: Uuid = Uuid::from_canonical([
    0x33, 0x33, 0x44, 0x44, 0x55, 0x55, 0x66, 0x66, 0x77, 0x77, 0x88, 0x88, 0x99, 0x99, 0xaa, 0xaa,
]);
const UNKNOWN_APPLICATION: Uuid = Uuid::from_canonical([
    0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe,
]);

fn unknown_userdata(archive: ArchiveVersion) -> Vec<u8> {
    let payload = support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &[
            1_i32.to_le_bytes().as_slice(),
            0_i32.to_le_bytes().as_slice(),
            [0xde, 0xad, 0xbe, 0xef].as_slice(),
        ]
        .concat(),
    );
    support::test_dump::class_userdata_v2_with_class_and_item_direct_payload(
        archive,
        UNKNOWN_CLASS.to_wire(),
        UNKNOWN_ITEM.to_wire(),
        UNKNOWN_APPLICATION.to_wire(),
        50,
        202_608_010,
        &payload,
    )
}

fn future_userdata(archive: ArchiveVersion) -> Vec<u8> {
    support::test_dump::crc_chunk(archive, 0x0002_7ffd, &[0x30, 0, 0xde, 0xad])
}

fn point_record(archive: ArchiveVersion, userdata: &[u8]) -> Vec<u8> {
    let object_type = support::test_dump::short_chunk(archive, 0x8200_0071, POINT_OBJECT_TYPE);
    let class_uuid = support::test_dump::long_chunk(
        archive,
        0x0002_fffb,
        &[
            support::test_dump::POINT_CLASS.as_slice(),
            &crc32fast::hash(&support::test_dump::POINT_CLASS).to_le_bytes(),
        ]
        .concat(),
    );
    let class_data = support::test_dump::crc_chunk(
        archive,
        0x0002_fffc,
        &support::point_payload([1.0, 2.0, 3.0]),
    );
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

fn assert_point_record(result: &cadmpeg_ir::codec::DecodeResult, record: &[u8]) {
    assert_eq!(result.ir().model.points.len(), 1);
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|value| value.id == "rhino:object:record#000000")
        .expect("unregistered userdata object record is retained");
    assert_eq!(retained.data.as_deref(), Some(record));
    assert_valid(result);
}

#[test]
fn unregistered_class_userdata_retains_typed_point_and_complete_record() {
    let archive = ArchiveVersion::V5;
    let record = point_record(archive, &unknown_userdata(archive));
    let result = decode(support::archive_writer(
        "50",
        202_608_010,
        std::slice::from_ref(&record),
    ));
    assert_point_record(&result, &record);
}

#[test]
fn future_generic_userdata_header_retains_typed_point_and_complete_record() {
    let archive = ArchiveVersion::V5;
    let record = point_record(archive, &future_userdata(archive));
    let result = decode(support::archive_writer(
        "50",
        202_608_010,
        std::slice::from_ref(&record),
    ));
    assert_point_record(&result, &record);
}
