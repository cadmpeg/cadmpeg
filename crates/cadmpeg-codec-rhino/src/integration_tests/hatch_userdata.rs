// SPDX-License-Identifier: Apache-2.0
//! Hatch-owned class-userdata admission and retention contracts.

use super::{assert_valid, decode};
use crate::chunks::{ArchiveVersion, TCODE_CRC};
use crate::test_support as support;

const HATCH_OBJECT_TYPE: i64 = 0x0001_0000;
const GRADIENT_APPLICATION: [u8; 16] = [
    0x5d, 0x58, 0x0b, 0x7b, 0x31, 0x7a, 0xd0, 0x45, 0x92, 0x5e, 0xbd, 0xd7, 0xdd, 0xf3, 0xe4, 0xe3,
];

fn gradient_payload(archive: ArchiveVersion, major: i32) -> Vec<u8> {
    if major != 1 {
        return support::test_dump::crc_chunk(
            archive,
            0x4000_8000,
            &[
                major.to_le_bytes().as_slice(),
                0_i32.to_le_bytes().as_slice(),
                [0xde, 0xad].as_slice(),
            ]
            .concat(),
        );
    }

    let first_stop = support::test_dump::anonymous_chunk(
        archive,
        0,
        &[
            [255, 0, 0, 255].as_slice(),
            0.0_f64.to_le_bytes().as_slice(),
        ]
        .concat(),
    );
    let second_stop = support::test_dump::anonymous_chunk(
        archive,
        0,
        &[
            [0, 0, 255, 255].as_slice(),
            1.0_f64.to_le_bytes().as_slice(),
        ]
        .concat(),
    );
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(
        [
            1.0_f64, 2.0, 3.0, // gradient start
            4.0, 5.0, 6.0, // gradient end
        ]
        .into_iter()
        .flat_map(f64::to_le_bytes),
    );
    body.extend(1.5_f64.to_le_bytes());
    body.extend(2_i32.to_le_bytes());
    body.extend(first_stop);
    body.extend(second_stop);
    support::test_dump::anonymous_chunk(archive, 0, &body)
}

fn hatch_record(archive: ArchiveVersion, userdata: &[u8]) -> Vec<u8> {
    let object_type = support::test_dump::short_chunk(archive, 0x8200_0071, HATCH_OBJECT_TYPE);
    let class_uuid = crate::hatch::CLASS.to_wire();
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let class_uuid = support::test_dump::long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = support::test_dump::crc_chunk(
        archive,
        0x0002_fffc,
        &crate::hatch::tests::version_two_hatch_payload(),
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

fn gradient_userdata(archive: ArchiveVersion, major: i32) -> Vec<u8> {
    let payload = gradient_payload(archive, major);
    gradient_userdata_with_payload(archive, &payload)
}

fn gradient_userdata_with_payload(archive: ArchiveVersion, payload: &[u8]) -> Vec<u8> {
    support::test_dump::class_userdata_v2_with_class_and_item_direct_payload(
        archive,
        crate::hatch::GRADIENT_COLOR_DATA.to_wire(),
        crate::hatch::GRADIENT_COLOR_DATA.to_wire(),
        GRADIENT_APPLICATION,
        60,
        202_608_010,
        payload,
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

#[test]
fn current_gradient_userdata_reaches_the_hatch_native_parameter() {
    let archive = ArchiveVersion::V8;
    let hatch = hatch_record(archive, &gradient_userdata(archive, 1));
    let result = decode(support::archive_writer("80", 202_608_010, &[hatch]));

    let parameters = hatch_parameters(&result);
    assert!(parameters
        .get("gradient")
        .is_some_and(|value| value.contains("linear")));
    assert_eq!(result.ir().model.curves.len(), 1);
    assert_valid(&result);
}

#[test]
fn future_gradient_userdata_retains_typed_hatch_and_complete_object_record() {
    let archive = ArchiveVersion::V8;
    let hatch = hatch_record(archive, &gradient_userdata(archive, 2));
    let result = decode(support::archive_writer(
        "80",
        202_608_010,
        std::slice::from_ref(&hatch),
    ));

    let parameters = hatch_parameters(&result);
    assert!(!parameters.contains_key("gradient"));
    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("hatch userdata extension failed")
            && loss
                .message
                .contains("unsupported gradient userdata version 2")
    }));
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| record.id() == "rhino:object:record#000000")
        .expect("future gradient userdata object record is retained");
    assert_eq!(retained.data(), Some(hatch.as_slice()));
    assert_valid(&result);
}

#[test]
fn malformed_gradient_userdata_retains_typed_hatch_and_complete_object_record() {
    let archive = ArchiveVersion::V8;
    let malformed = support::test_dump::anonymous_chunk(archive, 0, &5_i32.to_le_bytes());
    let hatch = hatch_record(
        archive,
        &gradient_userdata_with_payload(archive, &malformed),
    );
    let result = decode(support::archive_writer(
        "80",
        202_608_010,
        std::slice::from_ref(&hatch),
    ));

    let parameters = hatch_parameters(&result);
    assert!(!parameters.contains_key("gradient"));
    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("hatch userdata extension failed")
            && loss.message.contains("invalid gradient type")
    }));
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| record.id() == "rhino:object:record#000000")
        .expect("malformed gradient userdata object record is retained");
    assert_eq!(retained.data(), Some(hatch.as_slice()));
    assert_valid(&result);
}
