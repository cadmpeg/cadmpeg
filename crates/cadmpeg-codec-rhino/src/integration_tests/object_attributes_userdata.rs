// SPDX-License-Identifier: Apache-2.0
//! Object-attributes class-userdata admission and retention contracts.

use super::{assert_valid, decode};
use crate::chunks::{ArchiveVersion, TCODE_CRC};
use crate::test_support as support;
use crate::wire::Uuid;

const PER_OBJECT_APPLICATION: [u8; 16] = [
    0xc8, 0xcd, 0xa5, 0x97, 0xd9, 0x57, 0x46, 0x25, 0xa4, 0xb3, 0xa0, 0xb5, 0x10, 0xfc, 0x30, 0xd4,
];

fn object_record(archive: ArchiveVersion, userdata: &[u8], attributes: &[u8]) -> Vec<u8> {
    let object_type = support::test_dump::short_chunk(archive, 0x8200_0071, 1);
    let mut uuid_body = support::test_dump::POINT_CLASS.to_vec();
    uuid_body.extend(crc32fast::hash(&support::test_dump::POINT_CLASS).to_le_bytes());
    let class_uuid = support::test_dump::long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = support::test_dump::crc_chunk(
        archive,
        0x0002_fffc,
        &support::point_payload([1.25, -2.5, 3.75]),
    );
    let class_end = support::test_dump::short_chunk(archive, 0x8002_7fff, 0);
    let class = support::test_dump::long_chunk(
        archive,
        0x0002_7ffa,
        &[class_uuid, class_data, class_end].concat(),
    );
    let attributes = support::test_dump::crc_chunk(archive, 0x0200_8072, attributes);
    let attribute_userdata = support::test_dump::long_chunk(
        archive,
        0x0200_0073,
        &[
            userdata,
            &support::test_dump::short_chunk(archive, 0x8002_7fff, 0),
        ]
        .concat(),
    );
    let object_end = support::test_dump::short_chunk(archive, 0x8200_007f, 0);
    support::test_dump::nested_crc_chunk(
        archive,
        0x2000_8070 | TCODE_CRC,
        &[
            object_type,
            class,
            attributes,
            attribute_userdata,
            object_end,
        ]
        .concat(),
    )
}

fn userdata(archive: ArchiveVersion, payload: &[u8]) -> Vec<u8> {
    support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        crate::objects::PER_OBJECT_MESH_PARAMETERS_USERDATA.to_wire(),
        Uuid::from_canonical(PER_OBJECT_APPLICATION).to_wire(),
        50,
        202_608_010,
        payload,
    )
}

fn obsolete_custom_mesh_userdata(archive: ArchiveVersion, payload: &[u8]) -> Vec<u8> {
    support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        crate::objects::OBSOLETE_CUSTOM_MESH_USERDATA.to_wire(),
        [0; 16],
        50,
        2_348_836_140,
        payload,
    )
}

fn obsolete_custom_mesh_payload(archive: ArchiveVersion) -> Vec<u8> {
    [
        37_i32.to_le_bytes().as_slice(),
        [1].as_slice(),
        support::test_dump::mesh_parameters(archive).as_slice(),
        [0xde, 0xad].as_slice(),
    ]
    .concat()
}

fn current_payload(archive: ArchiveVersion) -> Vec<u8> {
    let inner = support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &support::test_dump::mesh_parameters(archive),
    );
    let mut body = [
        1_i32.to_le_bytes().as_slice(),
        0_i32.to_le_bytes().as_slice(),
    ]
    .concat();
    body.extend(inner);
    body.extend([0xde, 0xad]);
    support::test_dump::crc_chunk(archive, 0x4000_8000, &body)
}

fn future_payload(archive: ArchiveVersion) -> Vec<u8> {
    support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &[
            2_i32.to_le_bytes().as_slice(),
            0_i32.to_le_bytes().as_slice(),
            [0xde, 0xad].as_slice(),
        ]
        .concat(),
    )
}

fn malformed_payload(archive: ArchiveVersion) -> Vec<u8> {
    let inner = support::test_dump::short_chunk(archive, 0x4000_8000, 0);
    support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &[
            1_i32.to_le_bytes().as_slice(),
            0_i32.to_le_bytes().as_slice(),
            inner.as_slice(),
        ]
        .concat(),
    )
}

fn object_presentation(
    result: &cadmpeg_ir::codec::DecodeResult,
) -> &cadmpeg_ir::native::NativeRecord {
    let arena = &result.ir().native.namespace("rhino").unwrap().arenas["object_presentation"];
    assert_eq!(arena.len(), 1);
    &arena[0]
}

fn assert_point_and_retention(result: &cadmpeg_ir::codec::DecodeResult, record: &[u8]) {
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(
        result.ir().model.points[0].position,
        cadmpeg_ir::math::Point3::new(1.25, -2.5, 3.75)
    );
    let presentation = object_presentation(result);
    assert!(presentation.field("layer_index").is_some());
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|value| value.id == "rhino:object:record#000000")
        .expect("object record is retained");
    assert_eq!(retained.data.as_deref(), Some(record));
}

#[test]
fn current_per_object_mesh_userdata_reaches_object_presentation() {
    let archive = ArchiveVersion::V5;
    let attributes = support::test_dump::tagged_attributes(&[], 0);
    let userdata = userdata(archive, &current_payload(archive));
    let record = object_record(archive, &userdata, &attributes);
    let result = decode(support::archive_writer(
        "50",
        202_608_010,
        std::slice::from_ref(&record),
    ));

    assert_point_and_retention(&result, &record);
    let mesh = object_presentation(&result)
        .field("custom_render_mesh")
        .expect("current per-object mesh settings");
    assert_eq!(mesh["version"], serde_json::json!([1, 5]));
    assert_eq!(mesh["compute_curvature"], serde_json::json!(false));
    assert_eq!(mesh["custom_settings"], serde_json::json!(true));
    assert_valid(&result);
}

#[test]
fn future_per_object_mesh_userdata_keeps_point_and_attributes() {
    let archive = ArchiveVersion::V5;
    let attributes = support::test_dump::tagged_attributes(&[], 0);
    let userdata = userdata(archive, &future_payload(archive));
    let record = object_record(archive, &userdata, &attributes);
    let result = decode(support::archive_writer(
        "50",
        202_608_010,
        std::slice::from_ref(&record),
    ));

    assert_point_and_retention(&result, &record);
    assert!(object_presentation(&result)
        .field("custom_render_mesh")
        .is_none());
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("per-object mesh userdata") && loss.message.contains("unsupported")
    }));
    assert_valid(&result);
}

#[test]
fn malformed_per_object_mesh_userdata_keeps_point_and_attributes() {
    let archive = ArchiveVersion::V5;
    let attributes = support::test_dump::tagged_attributes(&[], 0);
    let userdata = userdata(archive, &malformed_payload(archive));
    let record = object_record(archive, &userdata, &attributes);
    let result = decode(support::archive_writer(
        "50",
        202_608_010,
        std::slice::from_ref(&record),
    ));

    assert_point_and_retention(&result, &record);
    assert!(object_presentation(&result)
        .field("custom_render_mesh")
        .is_none());
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("per-object mesh userdata") && loss.message.contains("dropped")
    }));
    assert_valid(&result);
}

#[test]
fn obsolete_custom_mesh_userdata_reaches_object_presentation() {
    let archive = ArchiveVersion::V5;
    let attributes = support::test_dump::tagged_attributes(&[], 0);
    let userdata = obsolete_custom_mesh_userdata(archive, &obsolete_custom_mesh_payload(archive));
    let record = object_record(archive, &userdata, &attributes);
    let result = decode(support::archive_writer(
        "50",
        2_348_836_140,
        std::slice::from_ref(&record),
    ));

    assert_point_and_retention(&result, &record);
    let mesh = object_presentation(&result)
        .field("custom_render_mesh")
        .expect("obsolete custom mesh settings");
    assert_eq!(mesh["version"], serde_json::json!([1, 5]));
    assert_eq!(mesh["custom_settings"], serde_json::json!(true));
    assert_eq!(mesh["custom_settings_enabled"], serde_json::json!(true));
    assert_eq!(mesh["compute_curvature"], serde_json::json!(false));
    assert_valid(&result);
}

#[test]
fn malformed_obsolete_custom_mesh_userdata_keeps_point_and_attributes() {
    let archive = ArchiveVersion::V5;
    let attributes = support::test_dump::tagged_attributes(&[], 0);
    let userdata = obsolete_custom_mesh_userdata(archive, &[7, 2]);
    let record = object_record(archive, &userdata, &attributes);
    let result = decode(support::archive_writer(
        "50",
        2_348_836_140,
        std::slice::from_ref(&record),
    ));

    assert_point_and_retention(&result, &record);
    assert!(object_presentation(&result)
        .field("custom_render_mesh")
        .is_none());
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("obsolete custom mesh userdata") && loss.message.contains("dropped")
    }));
    assert_valid(&result);
}
