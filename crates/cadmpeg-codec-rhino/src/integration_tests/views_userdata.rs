// SPDX-License-Identifier: Apache-2.0
//! Viewport class-userdata retention contracts.

use super::{assert_valid, decode};
use crate::chunks::ArchiveVersion;
use crate::test_support as support;

fn point(bytes: &mut Vec<u8>, value: [f64; 3]) {
    for coordinate in value {
        bytes.extend(coordinate.to_le_bytes());
    }
}

fn viewport_body() -> Vec<u8> {
    let mut bytes = vec![0x15];
    for value in [1_i32, 1, 1, 2] {
        bytes.extend(value.to_le_bytes());
    }
    point(&mut bytes, [1.0, 2.0, 3.0]);
    for vector in [
        [0.0, 0.0, -1.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ] {
        point(&mut bytes, vector);
    }
    for value in [-2.0_f64, 2.0, -1.0, 1.0, 0.1, 100.0] {
        bytes.extend(value.to_le_bytes());
    }
    for value in [0_i32, 1920, 0, 1080, 1, 100] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0x11; 16]);
    bytes.extend([1, 0, 1, 0, 1]);
    point(&mut bytes, [4.0, 5.0, 6.0]);
    bytes.push(1);
    for value in [1.0_f64, 2.0, 3.0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes
}

fn viewport_userdata(archive: ArchiveVersion) -> Vec<u8> {
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
    let class_end_start = userdata.len();
    let mut body = userdata;
    body.extend(&class_end);
    body.extend([0xca, 0xfe]);
    let class_end_range = class_end_start..class_end_start + class_end.len();
    support::test_dump::crc_chunk_excluding(
        archive,
        0x2000_8d3b,
        &body,
        &[0..class_end_start, class_end_range],
    )
}

fn named_views_record(archive: ArchiveVersion) -> Vec<u8> {
    named_views_record_with_userdata(archive, viewport_userdata(archive))
}

fn named_views_record_with_userdata(archive: ArchiveVersion, userdata: Vec<u8>) -> Vec<u8> {
    let viewport = support::test_dump::crc_chunk(archive, 0x2000_823b, &viewport_body());
    let end_marker = support::test_dump::short_chunk(archive, crate::chunks::TCODE_ENDOFTABLE, 0);
    let mut view_body = viewport;
    let viewport_range = 0..view_body.len();
    let userdata_start = view_body.len();
    view_body.extend(userdata);
    let userdata_range = userdata_start..view_body.len();
    let end_start = view_body.len();
    view_body.extend(end_marker);
    let end_range = end_start..view_body.len();
    let view = support::test_dump::crc_chunk_excluding(
        archive,
        0x2000_803b,
        &view_body,
        &[viewport_range, userdata_range, end_range],
    );
    let mut list_body = 1_i32.to_le_bytes().to_vec();
    let view_range = list_body.len()..list_body.len() + view.len();
    list_body.extend(view);
    support::test_dump::crc_chunk_excluding(
        archive,
        0x2000_8036,
        &list_body,
        std::slice::from_ref(&view_range),
    )
}

#[test]
fn viewport_userdata_future_payload_retains_typed_view_list_record() {
    let archive = ArchiveVersion::V8;
    let named_views = named_views_record(archive);
    let bytes = support::test_dump::minimal_document(
        "80",
        &[
            support::test_dump::table(archive, 0x1000_0014, &[]),
            support::test_dump::table(archive, 0x1000_0015, std::slice::from_ref(&named_views)),
            support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    );
    let result = decode(bytes);

    let views = &result.ir().native.namespace("rhino").unwrap().arenas["views"];
    assert_eq!(views.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::ViewportUserdataDropped.kind()
            && loss.message.contains("no typed CADIR owner")
    }));
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| {
            record
                .id
                .starts_with("rhino:opaque:record#10000015-20008036-")
        })
        .expect("view list record is retained");
    assert_eq!(retained.data.as_deref(), Some(named_views.as_slice()));
    assert_valid(&result);
}

#[test]
fn malformed_viewport_userdata_retains_typed_view_list_record() {
    let archive = ArchiveVersion::V8;
    let malformed_userdata = support::test_dump::crc_chunk(archive, 0x2000_8d3b, &[0xde, 0xad]);
    let named_views = named_views_record_with_userdata(archive, malformed_userdata);
    let bytes = support::test_dump::minimal_document(
        "80",
        &[
            support::test_dump::table(archive, 0x1000_0014, &[]),
            support::test_dump::table(archive, 0x1000_0015, std::slice::from_ref(&named_views)),
            support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    );
    let result = decode(bytes);

    let views = &result.ir().native.namespace("rhino").unwrap().arenas["views"];
    assert_eq!(views.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::ViewportUserdataDropped.kind()
            && loss.message.contains("could not be framed")
    }));
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| {
            record
                .id
                .starts_with("rhino:opaque:record#10000015-20008036-")
        })
        .expect("malformed view list record is retained");
    assert_eq!(retained.data.as_deref(), Some(named_views.as_slice()));
    assert_valid(&result);
}
