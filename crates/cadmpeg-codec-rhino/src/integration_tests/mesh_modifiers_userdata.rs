// SPDX-License-Identifier: Apache-2.0
//! Mesh-modifier XML userdata admission and retention contracts.

use super::{assert_valid, decode};
use crate::chunks::{ArchiveVersion, TCODE_CRC};
use crate::mesh_modifiers;
use crate::test_support as support;
use crate::wire::Uuid;

const MODIFIER_XML: [(&str, &str); 5] = [
    ("displacement", "<xml><new-displacement-object-data/></xml>"),
    ("edge_softening", "<xml><edge-softening-object-data/></xml>"),
    ("thickening", "<xml><thickening-object-data/></xml>"),
    ("curve_piping", "<xml><curve-piping-object-data/></xml>"),
    ("shut_lining", "<xml><shut-lining-object-data/></xml>"),
];

fn modifier_ids(label: &str) -> (Uuid, Uuid) {
    match label {
        "displacement" => (
            mesh_modifiers::DISPLACEMENT_CLASS,
            mesh_modifiers::DISPLACEMENT_ITEM,
        ),
        "edge_softening" => (
            mesh_modifiers::EDGE_SOFTENING_CLASS,
            mesh_modifiers::EDGE_SOFTENING_ITEM,
        ),
        "thickening" => (
            mesh_modifiers::THICKENING_CLASS,
            mesh_modifiers::THICKENING_ITEM,
        ),
        "curve_piping" => (
            mesh_modifiers::CURVE_PIPING_CLASS,
            mesh_modifiers::CURVE_PIPING_ITEM,
        ),
        "shut_lining" => (
            mesh_modifiers::SHUT_LINING_CLASS,
            mesh_modifiers::SHUT_LINING_ITEM,
        ),
        _ => panic!("unknown mesh modifier {label}"),
    }
}

fn warning_label(label: &str) -> &str {
    match label {
        "edge_softening" => "edge-softening",
        "curve_piping" => "curve-piping",
        "shut_lining" => "shut-lining",
        other => other,
    }
}

fn object_record(archive: ArchiveVersion, userdata: &[u8]) -> Vec<u8> {
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
    let attributes = support::test_dump::crc_chunk(
        archive,
        0x0200_8072,
        &support::test_dump::tagged_attributes(&[], 0),
    );
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

fn xml_payload(version: i32, xml: &str) -> Vec<u8> {
    let mut payload = version.to_le_bytes().to_vec();
    if version == 2 {
        payload.extend((xml.len() as i32).to_le_bytes());
        payload.extend(xml.as_bytes());
    }
    payload.extend([0xde, 0xad]);
    payload
}

fn modifier_userdata(archive: ArchiveVersion, label: &str, version: i32, xml: &str) -> Vec<u8> {
    let (class, item) = modifier_ids(label);
    support::test_dump::class_userdata_v2_with_class_and_item_direct_payload(
        archive,
        class.to_wire(),
        item.to_wire(),
        mesh_modifiers::MESH_MODIFIER_PLUGIN.to_wire(),
        50,
        202_608_010,
        &xml_payload(version, xml),
    )
}

fn assert_point_and_retention(result: &cadmpeg_ir::codec::DecodeResult, record: &[u8]) {
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(
        result.ir().model.points[0].position,
        cadmpeg_ir::math::Point3::new(1.25, -2.5, 3.75)
    );
    let presentation =
        &result.ir().native.namespace("rhino").unwrap().arenas["object_presentation"];
    assert_eq!(presentation.len(), 1);
    assert!(presentation[0].field("layer_index").is_some());
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|value| value.id == "rhino:object:record#000000")
        .expect("object record is retained");
    assert_eq!(retained.data.as_deref(), Some(record));
}

#[test]
fn current_mesh_modifier_xml_reaches_each_native_field() {
    let archive = ArchiveVersion::V5;
    for (label, xml) in MODIFIER_XML {
        let userdata = modifier_userdata(archive, label, 2, xml);
        let record = object_record(archive, &userdata);
        let result = decode(support::archive_writer(
            "50",
            202_608_010,
            std::slice::from_ref(&record),
        ));

        assert_point_and_retention(&result, &record);
        let modifiers = result.ir().native.namespace("rhino").unwrap().arenas
            ["object_presentation"][0]
            .field("mesh_modifiers")
            .expect("current mesh modifier");
        assert!(modifiers.get(label).is_some(), "missing {label} modifier");
        assert_valid(&result);
    }
}

#[test]
fn future_mesh_modifier_xml_keeps_object_and_drops_only_modifier() {
    let archive = ArchiveVersion::V5;
    for (label, xml) in MODIFIER_XML {
        let userdata = modifier_userdata(archive, label, 3, xml);
        let record = object_record(archive, &userdata);
        let result = decode(support::archive_writer(
            "50",
            202_608_010,
            std::slice::from_ref(&record),
        ));

        assert_point_and_retention(&result, &record);
        assert!(
            result.ir().native.namespace("rhino").unwrap().arenas["object_presentation"][0]
                .field("mesh_modifiers")
                .is_none()
        );
        assert!(result.report().losses.iter().any(|loss| {
            loss.message.contains(warning_label(label))
                && loss.message.contains("userdata")
                && loss.message.contains("unsupported")
        }));
        assert_valid(&result);
    }
}

#[test]
fn malformed_mesh_modifier_xml_keeps_object_and_drops_only_modifier() {
    let archive = ArchiveVersion::V5;
    for (label, _) in MODIFIER_XML {
        let userdata = modifier_userdata(archive, label, 2, "<xml><broken>");
        let record = object_record(archive, &userdata);
        let result = decode(support::archive_writer(
            "50",
            202_608_010,
            std::slice::from_ref(&record),
        ));

        assert_point_and_retention(&result, &record);
        assert!(
            result.ir().native.namespace("rhino").unwrap().arenas["object_presentation"][0]
                .field("mesh_modifiers")
                .is_none()
        );
        assert!(result.report().losses.iter().any(|loss| {
            loss.message.contains(warning_label(label))
                && loss.message.contains("userdata")
                && loss.message.contains("dropped")
        }));
        assert_valid(&result);
    }
}
