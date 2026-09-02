// SPDX-License-Identifier: Apache-2.0
//! V5 hatch compatibility-userdata admission and retention contracts.

use super::{assert_valid, decode};
use crate::chunks::{ArchiveVersion, TCODE_CRC};
use crate::test_support as support;
use crate::wire::Uuid;

const HATCH_OBJECT_TYPE: i64 = 0x0001_0000;
const OPENNURBS5_APPLICATION: Uuid = Uuid::from_canonical([
    0xc8, 0xcd, 0xa5, 0x97, 0xd9, 0x57, 0x46, 0x25, 0xa4, 0xb3, 0xa0, 0xb5, 0x10, 0xfc, 0x30, 0xd4,
]);

fn legacy_v5_hatch_payload() -> Vec<u8> {
    let mut payload = crate::hatch::tests::version_two_hatch_payload();
    payload[0] = 0x11;
    payload.truncate(payload.len() - 16);
    payload
}

fn anonymous_major(archive: ArchiveVersion, major: i32, body: &[u8]) -> Vec<u8> {
    let payload = [
        major.to_le_bytes().as_slice(),
        0_i32.to_le_bytes().as_slice(),
        body,
    ]
    .concat();
    support::test_dump::crc_chunk(archive, 0x4000_8000, &payload)
}

fn hatch_extra_payload(archive: ArchiveVersion, major: i32, malformed: bool) -> Vec<u8> {
    let mut body = vec![0; 16];
    body.extend(2.5_f64.to_le_bytes());
    if !malformed {
        body.extend(3.5_f64.to_le_bytes());
    }
    let mut payload = anonymous_major(archive, major, &body);
    payload.extend([0xde, 0xad]);
    payload
}

fn userdata(archive: ArchiveVersion, major: i32, malformed: bool) -> Vec<u8> {
    support::test_dump::class_userdata_v2_with_class_and_item_direct_payload(
        archive,
        crate::hatch::V5_HATCH_EXTRA.to_wire(),
        crate::hatch::V5_HATCH_EXTRA.to_wire(),
        OPENNURBS5_APPLICATION.to_wire(),
        50,
        202_608_010,
        &hatch_extra_payload(archive, major, malformed),
    )
}

fn hatch_record(archive: ArchiveVersion, userdata: &[u8]) -> Vec<u8> {
    let object_type = support::test_dump::short_chunk(archive, 0x8200_0071, HATCH_OBJECT_TYPE);
    let class_uuid_wire = crate::hatch::CLASS.to_wire();
    let mut uuid_body = class_uuid_wire.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid_wire).to_le_bytes());
    let class_uuid = support::test_dump::long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data =
        support::test_dump::crc_chunk(archive, 0x0002_fffc, &legacy_v5_hatch_payload());
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

fn hatch_parameters(
    result: &cadmpeg_ir::codec::DecodeResult,
) -> &std::collections::BTreeMap<String, String> {
    result
        .ir()
        .model
        .features
        .iter()
        .find_map(|feature| match &feature.definition {
            cadmpeg_ir::features::FeatureDefinition::Native {
                kind, parameters, ..
            } if kind == "hatch" => Some(parameters),
            _ => None,
        })
        .expect("typed hatch feature")
}

fn assert_object_record(result: &cadmpeg_ir::codec::DecodeResult, record: &[u8]) {
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|value| value.id() == "rhino:object:record#000000")
        .expect("hatch object record is retained");
    assert_eq!(retained.data(), Some(record));
}

#[test]
fn current_v5_hatch_extra_reaches_the_hatch_basepoint() {
    let archive = ArchiveVersion::V5;
    let record = hatch_record(archive, &userdata(archive, 1, false));
    let result = decode(support::archive_writer(
        "50",
        202_608_010,
        std::slice::from_ref(&record),
    ));

    let parameters = hatch_parameters(&result);
    assert_eq!(parameters["basepoint"], "2.5,3.5");
    assert_eq!(parameters["pattern_index"], "7");
    assert_eq!(result.ir().model.curves.len(), 1);
    assert_object_record(&result, &record);
    assert_valid(&result);
}

#[test]
fn future_v5_hatch_extra_retains_typed_hatch_and_complete_object_record() {
    let archive = ArchiveVersion::V5;
    let record = hatch_record(archive, &userdata(archive, 2, false));
    let result = decode(support::archive_writer(
        "50",
        202_608_010,
        std::slice::from_ref(&record),
    ));

    let parameters = hatch_parameters(&result);
    assert_eq!(parameters["basepoint"], "0,0");
    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("hatch userdata extension failed")
            && loss.message.contains("unsupported V5 hatch-extra version")
    }));
    assert_object_record(&result, &record);
    assert_valid(&result);
}

#[test]
fn malformed_v5_hatch_extra_retains_typed_hatch_and_complete_object_record() {
    let archive = ArchiveVersion::V5;
    let record = hatch_record(archive, &userdata(archive, 1, true));
    let result = decode(support::archive_writer(
        "50",
        202_608_010,
        std::slice::from_ref(&record),
    ));

    let parameters = hatch_parameters(&result);
    assert_eq!(parameters["basepoint"], "0,0");
    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("hatch userdata extension failed")
            && loss.message.contains("exceeds bound")
    }));
    assert_object_record(&result, &record);
    assert_valid(&result);
}
