// SPDX-License-Identifier: Apache-2.0
//! Render-settings class-userdata retention contracts.

use super::{assert_valid, decode};
use crate::chunks::ArchiveVersion;
use crate::test_support as support;

fn render_settings_record(archive: ArchiveVersion) -> Vec<u8> {
    let mut body = Vec::new();
    body.push(1);
    body.extend(1234_i32.to_le_bytes());
    body.extend(567_i32.to_le_bytes());
    body.extend(144.5_f64.to_le_bytes());
    body.extend(2_u32.to_le_bytes());
    body.extend([1, 2, 3, 4]);
    body.extend(2_i32.to_le_bytes());
    body.extend([5, 6, 7, 8]);
    body.extend([9, 10, 11, 12]);
    body.extend(support::test_dump::utf16_bytes("background.png"));
    body.extend([1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0]);
    body.extend(3_i32.to_le_bytes());
    body.extend(2_i32.to_le_bytes());
    body.extend(2048_i32.to_le_bytes());
    body.extend(1024_i32.to_le_bytes());
    body.extend(1.25_f64.to_le_bytes());
    body.extend([0xaa, 0xbb]);
    let child = support::test_dump::anonymous_chunk(archive, 0, &body);
    support::test_dump::crc_chunk_excluding(
        archive,
        0x2000_803d,
        &child,
        std::slice::from_ref(&(0..child.len())),
    )
}

fn render_userdata_record(archive: ArchiveVersion) -> Vec<u8> {
    let application = crate::wire::Uuid::from_canonical([
        0x17, 0xb3, 0xec, 0xda, 0x17, 0xba, 0x4e, 0x45, 0x9e, 0x67, 0xa2, 0xb8, 0xd9, 0xbe, 0x52,
        0x0d,
    ])
    .to_wire();
    let payload = [
        2_i32.to_le_bytes().as_slice(),
        0_i32.to_le_bytes().as_slice(),
        [0xde, 0xad].as_slice(),
    ]
    .concat();
    let userdata = support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        crate::objects::USER_STRING_LIST.to_wire(),
        application,
        60,
        202_608_010,
        &payload,
    );
    let class_end = support::test_dump::short_chunk(archive, 0x8002_7fff, 0);
    let class_end_len = class_end.len();
    let body = [userdata, class_end].concat();
    let userdata_range = 0..body.len() - class_end_len;
    let class_end_range = body.len() - class_end_len..body.len();
    support::test_dump::crc_chunk_excluding(
        archive,
        0x2000_8136,
        &body,
        &[userdata_range, class_end_range],
    )
}

#[test]
fn render_settings_userdata_future_payload_retains_complete_record() {
    let archive = ArchiveVersion::V8;
    let render = render_settings_record(archive);
    let userdata = render_userdata_record(archive);
    let bytes = support::test_dump::minimal_document(
        "80",
        &[
            support::test_dump::table(archive, 0x1000_0014, &[]),
            support::test_dump::table(archive, 0x1000_0015, &[render, userdata.clone()]),
            support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    );
    let result = decode(bytes);

    let render_settings = &result.ir().native.namespace("rhino").unwrap().arenas["render_settings"];
    assert_eq!(render_settings.len(), 1);
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| {
            record
                .id()
                .starts_with("rhino:opaque:record#10000015-20008136-")
        })
        .expect("render-settings userdata record is retained");
    assert_eq!(retained.data(), Some(userdata.as_slice()));
    assert_valid(&result);
}
