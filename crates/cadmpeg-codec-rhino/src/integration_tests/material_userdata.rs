// SPDX-License-Identifier: Apache-2.0
//! Material class-userdata retention contracts.

use super::{assert_valid, decode};
use crate::chunks::ArchiveVersion;
use crate::test_support::test_archive as support;
use crate::test_support::test_dump::{
    anonymous_chunk, class_userdata_v2_with_direct_payload, crc_chunk, long_chunk,
    minimal_document, nested_crc_chunk, object_record_with_payload, short_chunk, table,
    utf16_bytes, POINT_CLASS,
};

/// A V5 document with one material record of packed class-data `version`
/// whose physically based userdata has a future payload, and one point.
fn material_with_future_pbr_userdata(version: u8) -> (Vec<u8>, Vec<u8>) {
    let archive = ArchiveVersion::V5;
    let material_class = crate::wire::Uuid::from_canonical([
        0x60, 0xb5, 0xdb, 0xbc, 0xe6, 0x60, 0x11, 0xd3, 0xbf, 0xe4, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ])
    .to_wire();
    let mut material_payload = [[0x11; 16].as_slice(), 2_i32.to_le_bytes().as_slice()].concat();
    material_payload.extend(utf16_bytes("steel"));
    material_payload.extend([0x22; 16]);
    for color in [
        [1, 2, 3, 4],
        [5, 6, 7, 8],
        [9, 10, 11, 12],
        [13, 14, 15, 16],
        [17, 18, 19, 20],
        [128, 128, 128, 24],
    ] {
        material_payload.extend(color);
    }
    for value in [1.5_f64, 0.25, 64.0, 0.1] {
        material_payload.extend(value.to_le_bytes());
    }
    material_payload.extend(anonymous_chunk(archive, 0, &0_i32.to_le_bytes()));
    material_payload.extend(utf16_bytes(""));
    material_payload.extend(0_i32.to_le_bytes());
    material_payload.extend([1, 0]);
    material_payload.push(1);
    for value in [0.9_f64, 0.8, 1.4] {
        material_payload.extend(value.to_le_bytes());
    }
    material_payload.extend([0x33; 16]);
    material_payload.push(1);
    material_payload.extend([0xaa, 0xbb]);
    let mut material_class_data = vec![version];
    material_class_data.extend(anonymous_chunk(archive, 7, &material_payload));

    let future_pbr_payload = crc_chunk(
        archive,
        0x4000_8000,
        &[
            2_i32.to_le_bytes().as_slice(),
            0_i32.to_le_bytes().as_slice(),
            &[0xde, 0xad],
        ]
        .concat(),
    );
    let userdata = class_userdata_v2_with_direct_payload(
        archive,
        crate::wire::Uuid::from_canonical([
            0x56, 0x94, 0xe1, 0xac, 0x40, 0xe6, 0x44, 0xf4, 0x9c, 0xa9, 0x3b, 0x6d, 0x0e, 0x8c,
            0x44, 0x40,
        ])
        .to_wire(),
        crate::wire::Uuid::from_canonical([
            0x7b, 0x0b, 0x58, 0x5d, 0x7a, 0x31, 0x45, 0xd0, 0x92, 0x5e, 0xbd, 0xd7, 0xdd, 0xf3,
            0xe4, 0xe3,
        ])
        .to_wire(),
        50,
        0,
        &future_pbr_payload,
    );
    let mut uuid_body = material_class.to_vec();
    uuid_body.extend(crc32fast::hash(&material_class).to_le_bytes());
    let class_uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, &material_class_data);
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    let material_class_wrapper = long_chunk(
        archive,
        0x0002_7ffa,
        &[class_uuid, class_data, userdata, class_end].concat(),
    );
    let material_record = nested_crc_chunk(archive, 0x2000_8040, &material_class_wrapper);
    let valid_point = object_record_with_payload(
        archive,
        1,
        POINT_CLASS,
        &support::point_payload([4.0, 5.0, 6.0]),
    );
    let mut units_body = 100_i32.to_le_bytes().to_vec();
    units_body.extend(2_i32.to_le_bytes());
    units_body.extend(0.01_f64.to_le_bytes());
    units_body.extend(0.1_f64.to_le_bytes());
    units_body.extend(0.001_f64.to_le_bytes());
    let units = crc_chunk(archive, 0x2000_8031, &units_body);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[units]),
            table(archive, 0x1000_0010, std::slice::from_ref(&material_record)),
            table(archive, 0x1000_0013, &[valid_point]),
        ],
    );
    (bytes, material_record)
}

#[test]
fn registered_material_userdata_future_payload_is_retained_by_table_owner() {
    let (bytes, material_record) = material_with_future_pbr_userdata(0x20);
    let result = decode(bytes);

    assert_eq!(result.ir().geometry.points.len(), 1);
    let materials = &result.ir().native.namespace("rhino").unwrap().arenas()["materials"];
    assert_eq!(materials.len(), 1);
    assert_eq!(
        materials[0]
            .field("name")
            .and_then(|value| value.as_str().map(str::to_owned)),
        Some("steel".to_owned())
    );
    assert!(materials[0].field("physically_based").is_none());
    let retained = result
        .source_fidelity()
        .retained_records()
        .iter()
        .find(|(id, _)| {
            id.as_str()
                .starts_with("rhino:opaque:record#10000010-20008040-")
        })
        .map(|(_, record)| record)
        .expect("future material userdata record is retained");
    assert_eq!(retained.data(), Some(material_record.as_slice()));
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("physically based material userdata")
            && loss.message.contains("could not be transferred")
    }));
    assert_valid(&result);
}

/// A material record that does not parse is given up; the userdata loss it
/// read before the failure does not stay beside the charge (§21).
#[test]
fn a_given_up_material_keeps_no_userdata_loss_beside_its_charge() {
    let (bytes, material_record) = material_with_future_pbr_userdata(0x30);
    let result = decode(bytes);
    let losses = &result.report().losses;
    assert!(
        result.ir().native.namespace("rhino").unwrap().arenas()["materials"].is_empty(),
        "{losses:#?}"
    );
    let charges = losses
        .iter()
        .filter(|loss| {
            cadmpeg_ir::unknown::part::gives_up(&loss.code)
                && loss.message.contains("table 0x10000010")
        })
        .collect::<Vec<_>>();
    assert_eq!(charges.len(), 1, "{losses:#?}");
    assert!(
        !losses
            .iter()
            .any(|loss| loss.message.contains("physically based material userdata")),
        "{losses:#?}"
    );
    assert!(result
        .source_fidelity()
        .retained_records()
        .iter()
        .any(|(_, record)| record.data() == Some(material_record.as_slice())));
    assert_valid(&result);
}
