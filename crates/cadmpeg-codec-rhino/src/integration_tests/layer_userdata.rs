// SPDX-License-Identifier: Apache-2.0
//! Layer class-userdata retention contracts.

use super::{assert_valid, decode};
use crate::chunks::ArchiveVersion;
use crate::settings::LAYER_EXTENSIONS;
use crate::test_support as support;
use crate::wire::Uuid;

const LAYER_CLASS: [u8; 16] = [
    0x13, 0x98, 0x80, 0x95, 0x85, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const OPENNURBS5_APPLICATION: Uuid = Uuid::from_canonical([
    0xc8, 0xcd, 0xa5, 0x97, 0xd9, 0x57, 0x46, 0x25, 0xa4, 0xb3, 0xa0, 0xb5, 0x10, 0xfc, 0x30, 0xd4,
]);
const OBSOLETE_LAYER_SETTINGS: Uuid = Uuid::from_canonical([
    0xbf, 0xb6, 0x3c, 0x09, 0x4b, 0xc7, 0x47, 0x27, 0x89, 0xbb, 0x7c, 0xc7, 0x54, 0x11, 0x82, 0x00,
]);

fn layer_payload(archive: ArchiveVersion) -> Vec<u8> {
    let mut payload = vec![0x1f];
    payload.extend(0_i32.to_le_bytes());
    payload.extend(7_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend([10, 20, 30, 255]);
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(1.0_f64.to_le_bytes());
    payload.extend(support::test_dump::utf16_bytes("layer-witness"));
    payload.push(1);
    payload.extend((-1_i32).to_le_bytes());
    payload.extend([40, 50, 60, 255]);
    payload.extend(0.25_f64.to_le_bytes());
    payload.push(0);
    payload.extend([0x11; 16]);
    payload.extend([0; 16]);
    payload.push(1);
    payload.extend(support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ));
    payload.extend([0; 16]);
    payload.push(0);
    payload
}

fn layer_userdata(archive: ArchiveVersion, payload: &[u8]) -> Vec<u8> {
    let application = Uuid::from_canonical([
        0xc8, 0xcd, 0xa5, 0x97, 0xd9, 0x57, 0x46, 0x25, 0xa4, 0xb3, 0xa0, 0xb5, 0x10, 0xfc, 0x30,
        0xd4,
    ])
    .to_wire();
    support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        LAYER_EXTENSIONS.to_wire(),
        application,
        60,
        202_608_010,
        payload,
    )
}

fn obsolete_layer_userdata(archive: ArchiveVersion, class_uuid: Uuid, major: i32) -> Vec<u8> {
    let payload = support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &[
            major.to_le_bytes().as_slice(),
            0_i32.to_le_bytes().as_slice(),
            [0xde, 0xad].as_slice(),
        ]
        .concat(),
    );
    support::test_dump::class_userdata_v2_with_class_and_item_direct_payload(
        archive,
        class_uuid.to_wire(),
        class_uuid.to_wire(),
        OPENNURBS5_APPLICATION.to_wire(),
        50,
        202_608_010,
        &payload,
    )
}

fn malformed_obsolete_layer_userdata(archive: ArchiveVersion, class_uuid: Uuid) -> Vec<u8> {
    support::test_dump::class_userdata_v2_with_class_and_item_direct_payload(
        archive,
        class_uuid.to_wire(),
        class_uuid.to_wire(),
        OPENNURBS5_APPLICATION.to_wire(),
        50,
        202_608_010,
        &[0xde, 0xad],
    )
}

fn layer_record(archive: ArchiveVersion, userdata_payload: &[u8]) -> Vec<u8> {
    layer_record_with_userdata(archive, &layer_userdata(archive, userdata_payload))
}

fn layer_record_with_userdata(archive: ArchiveVersion, userdata: &[u8]) -> Vec<u8> {
    let class = support::test_dump::class_wrapper_with_userdata(
        archive,
        LAYER_CLASS,
        &layer_payload(archive),
        userdata,
    );
    #[allow(clippy::single_range_in_vec_init)] // The class wrapper is one checksum child.
    support::test_dump::crc_chunk_excluding(archive, 0x2000_8050, &class, &[0..class.len()])
}

fn document(archive: ArchiveVersion, layer: Vec<u8>) -> Vec<u8> {
    document_with_stamp(archive, layer, Some(202_608_010))
}

fn document_with_stamp(
    archive: ArchiveVersion,
    layer: Vec<u8>,
    writer_version: Option<i64>,
) -> Vec<u8> {
    let properties: Vec<Vec<u8>> = writer_version
        .map(|value| vec![support::test_dump::short_chunk(archive, 0xa000_0026, value)])
        .unwrap_or_default();
    support::test_dump::minimal_document(
        "80",
        &[
            support::test_dump::table(archive, 0x1000_0014, &properties),
            support::test_dump::table(archive, 0x1000_0015, &[]),
            support::test_dump::table(archive, 0x1000_0011, &[layer]),
            support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    )
}

fn assert_layer_record_retained(
    result: &cadmpeg_ir::codec::DecodeResult,
    layer: &[u8],
    message: &str,
) {
    let layers = &result.ir().native.namespace("rhino").unwrap().arenas["layers"];
    assert_eq!(layers.len(), 1);
    let fields = layers[0].fields();
    assert_eq!(
        fields
            .get("archive_index")
            .and_then(serde_json::Value::as_i64),
        Some(7)
    );
    assert_eq!(
        fields.get("name").and_then(serde_json::Value::as_str),
        Some("layer-witness")
    );
    assert!(fields
        .get("per_viewport_settings")
        .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty)));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(message)));
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| {
            record
                .id
                .starts_with("rhino:opaque:record#10000011-20008050-")
        })
        .expect("layer record is retained");
    assert_eq!(retained.data.as_deref(), Some(layer));
    assert_valid(result);
}

#[test]
fn layer_userdata_future_payload_retains_complete_layer_record() {
    let archive = ArchiveVersion::V8;
    let outer_payload = support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &[
            2_i32.to_le_bytes().as_slice(),
            0_i32.to_le_bytes().as_slice(),
            [0xde, 0xad].as_slice(),
        ]
        .concat(),
    );
    let layer = layer_record(archive, &outer_payload);
    let result = decode(document(archive, layer.clone()));
    assert_layer_record_retained(&result, &layer, "layer per-viewport userdata at offset");
}

/// The layer parent-link charge reaches the report as a typed loss code.
///
/// `parse_layer` pushes a bare warning string that `dialect_unverified_diagnostic`
/// promotes in the scan-warning loop. Asserting the warning text alone would
/// leave that promotion untested, so this asserts the code the report carries.
#[test]
fn unstamped_layer_promotes_the_parent_link_warning_to_a_typed_loss_code() {
    let archive = ArchiveVersion::V8;
    let layer = layer_record(archive, &[0xde, 0xad]);

    let unstamped = decode(document_with_stamp(archive, layer.clone(), None));
    assert!(
        unstamped.report().losses.iter().any(|loss| {
            loss.code == crate::loss::RhinoLossCode::SourceDialectUnverified.kind()
                && loss.message.contains("layer parent link")
        }),
        "{:?}",
        unstamped.report().losses
    );

    // The same record under a stamp: the parent link is read, so nothing is
    // charged and the layer still reaches the native arena.
    let stamped = decode(document(archive, layer));
    let layers = &stamped.ir().native.namespace("rhino").unwrap().arenas["layers"];
    assert_eq!(layers.len(), 1);
    assert!(
        !stamped
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::RhinoLossCode::SourceDialectUnverified.kind()),
        "{:?}",
        stamped.report().losses
    );
}

#[test]
fn layer_userdata_malformed_payload_retains_complete_layer_record() {
    let archive = ArchiveVersion::V8;
    let layer = layer_record(archive, &[0xde, 0xad]);
    let result = decode(document(archive, layer.clone()));
    assert_layer_record_retained(&result, &layer, "layer per-viewport userdata at offset");
}

#[test]
fn obsolete_layer_settings_are_consumed_without_typed_layer_fields() {
    let archive = ArchiveVersion::V8;
    for major in [1, 2] {
        let userdata = obsolete_layer_userdata(archive, OBSOLETE_LAYER_SETTINGS, major);
        let layer = layer_record_with_userdata(archive, &userdata);
        let result = decode(document(archive, layer));
        let layers = &result.ir().native.namespace("rhino").unwrap().arenas["layers"];
        assert_eq!(layers.len(), 1);
        let fields = layers[0].fields();
        assert_eq!(
            fields.get("name").and_then(serde_json::Value::as_str),
            Some("layer-witness")
        );
        assert!(fields
            .get("per_viewport_settings")
            .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty)));
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("layer per-viewport userdata")));
        assert_valid(&result);
    }
}

#[test]
fn malformed_obsolete_layer_settings_are_discarded_without_altering_the_layer() {
    let archive = ArchiveVersion::V8;
    let userdata = malformed_obsolete_layer_userdata(archive, OBSOLETE_LAYER_SETTINGS);
    let layer = layer_record_with_userdata(archive, &userdata);
    let result = decode(document(archive, layer));
    let layers = &result.ir().native.namespace("rhino").unwrap().arenas["layers"];
    assert_eq!(layers.len(), 1);
    let fields = layers[0].fields();
    assert_eq!(
        fields.get("name").and_then(serde_json::Value::as_str),
        Some("layer-witness")
    );
    assert!(fields
        .get("per_viewport_settings")
        .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty)));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("layer per-viewport userdata")));
    assert_valid(&result);
}
