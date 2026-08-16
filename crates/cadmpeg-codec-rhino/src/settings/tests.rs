// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports, dead_code, clippy::disallowed_methods)]

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::IR_VERSION;

use crate::chunks::{
    anonymous_version, checked_count_bytes, chunk_at, crc16, packed_version, parse_eof,
    parse_header, verify_checksum, ArchiveVersion, BoundedReader, ChecksumStatus, FramingError,
    TCODE_CRC, TCODE_ENDOFFILE, TCODE_SHORT,
};
use crate::settings;
use crate::test_support::test_dump::*;
use crate::wire::Uuid;
use crate::{RhinoCodec, MAGIC};

#[test]
fn decodes_bounded_utf8_and_utf16_strings() {
    let mut utf8_bytes = Vec::new();
    utf8_bytes.extend(3_u32.to_le_bytes());
    utf8_bytes.extend_from_slice("é\0".as_bytes());
    let mut utf8_reader =
        BoundedReader::new(&utf8_bytes, 0, utf8_bytes.len()).expect("bounded UTF-8 reader");
    assert_eq!(
        settings::utf8(&mut utf8_reader).expect("required invariant"),
        "é"
    );

    let mut utf16_bytes = Vec::new();
    utf16_bytes.extend(3_u32.to_le_bytes());
    utf16_bytes.extend(0xd83d_u16.to_le_bytes());
    utf16_bytes.extend(0xde00_u16.to_le_bytes());
    utf16_bytes.extend(0_u16.to_le_bytes());
    let mut utf16_reader =
        BoundedReader::new(&utf16_bytes, 0, utf16_bytes.len()).expect("bounded UTF-16 reader");
    assert_eq!(
        settings::utf16(&mut utf16_reader).expect("required invariant"),
        "😀"
    );

    let mut missing_nul = Vec::new();
    missing_nul.extend(2_u32.to_le_bytes());
    missing_nul.extend_from_slice(b"ab");
    let mut reader =
        BoundedReader::new(&missing_nul, 0, missing_nul.len()).expect("bounded string reader");
    assert!(settings::utf8(&mut reader).is_err());
}

#[test]
fn maps_standard_units_to_millimeters() {
    assert_eq!(settings::standard_scale(2), Some(1.0));
    assert_eq!(settings::standard_scale(8), Some(25.4));
    assert_eq!(settings::standard_scale(12), Some(1.0e-7));
    assert_eq!(settings::standard_scale(23), Some(149_597_870_000_000.0));
    assert_eq!(settings::standard_scale(24), Some(9.460_730_472_580_8e18));
    assert_eq!(settings::standard_scale(25), Some(3.085_677_58e19));
    assert_eq!(settings::standard_scale(255), None);
}

#[test]
pub(crate) fn parses_units_with_single_scale_transfer_and_legacy_order() {
    let mut body = Vec::new();
    body.extend(100_i32.to_le_bytes());
    body.extend(8_i32.to_le_bytes());
    body.extend(0.5_f64.to_le_bytes());
    body.extend(0.01_f64.to_le_bytes());
    body.extend(0.001_f64.to_le_bytes());
    let (data, record) = metadata_record(0x2000_8031, body);
    let units = settings::parse_units(&data, &record).expect("required invariant");
    assert_eq!(units.millimeters_per_unit, Some(25.4));
    assert_eq!(units.absolute_tolerance, 0.5);
    assert_eq!(units.absolute_tolerance_millimeters, Some(12.7));
    assert_eq!(units.angular_tolerance, 0.01);
    assert_eq!(units.relative_tolerance, 0.001);

    let mut legacy = Vec::new();
    legacy.extend(1_i32.to_le_bytes());
    legacy.extend(2_i32.to_le_bytes());
    legacy.extend(0.5_f64.to_le_bytes());
    legacy.extend(0.002_f64.to_le_bytes());
    legacy.extend(0.01_f64.to_le_bytes());
    let (data, record) = metadata_record(0x2000_8031, legacy);
    let units = settings::parse_units(&data, &record).expect("required invariant");
    assert_eq!(units.relative_tolerance, 0.002);
    assert_eq!(units.angular_tolerance, 0.01);
}

#[test]
fn rejects_invalid_unit_tolerances_and_trailing_bytes() {
    let mut body = Vec::new();
    body.extend(102_i32.to_le_bytes());
    body.extend(11_i32.to_le_bytes());
    body.extend(1.0_f64.to_le_bytes());
    body.extend(0.01_f64.to_le_bytes());
    body.extend(0.1_f64.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(2_i32.to_le_bytes());
    body.extend(1.0_f64.to_le_bytes());
    body.extend(2_u32.to_le_bytes());
    body.extend(b"m\0");
    body.extend(1_u8.to_le_bytes());
    let (data, record) = metadata_record(0x2000_8031, body);
    assert!(settings::parse_units(&data, &record).is_err());
}

#[test]
fn rejects_custom_scale_and_tolerance_products_that_overflow() {
    let mut scale_overflow = Vec::new();
    scale_overflow.extend(102_i32.to_le_bytes());
    scale_overflow.extend(11_i32.to_le_bytes());
    scale_overflow.extend(1.0_f64.to_le_bytes());
    scale_overflow.extend(0.01_f64.to_le_bytes());
    scale_overflow.extend(0.1_f64.to_le_bytes());
    scale_overflow.extend(0_i32.to_le_bytes());
    scale_overflow.extend(2_i32.to_le_bytes());
    scale_overflow.extend(1.0e308_f64.to_le_bytes());
    scale_overflow.extend(1_u32.to_le_bytes());
    scale_overflow.extend([0_u8, 0]);
    let (data, record) = metadata_record(0x2000_8031, scale_overflow);
    assert!(settings::parse_units(&data, &record).is_err());

    let mut tolerance_overflow = Vec::new();
    tolerance_overflow.extend(102_i32.to_le_bytes());
    tolerance_overflow.extend(11_i32.to_le_bytes());
    tolerance_overflow.extend(1.0e308_f64.to_le_bytes());
    tolerance_overflow.extend(0.01_f64.to_le_bytes());
    tolerance_overflow.extend(0.1_f64.to_le_bytes());
    tolerance_overflow.extend(0_i32.to_le_bytes());
    tolerance_overflow.extend(2_i32.to_le_bytes());
    tolerance_overflow.extend(1.0e100_f64.to_le_bytes());
    tolerance_overflow.extend(1_u32.to_le_bytes());
    tolerance_overflow.extend([0_u8, 0]);
    let (data, record) = metadata_record(0x2000_8031, tolerance_overflow);
    assert!(settings::parse_units(&data, &record).is_err());
}

#[test]
fn decodes_as_file_name_as_utf16_and_skips_fixed_trailing_bytes() {
    let mut name = Vec::new();
    name.extend(2_u32.to_le_bytes());
    name.extend([b'X', 0, 0, 0]);
    let (data, record) = metadata_record(0x2000_8027, name);
    let table = crate::container::Table {
        typecode: 0x1000_0014,
        range: 0..data.len(),
        body: 0..data.len(),
        records: vec![record],
        record_count: 1,
        object_typecodes: std::collections::BTreeMap::new(),
    };
    let mut warnings = Vec::new();
    let metadata = settings::parse_metadata(&data, ArchiveVersion::V5, &[table], &mut warnings);
    assert_eq!(metadata.properties.as_file_name.as_deref(), Some("X"));
    assert!(warnings.is_empty());

    let mut trailing = data;
    trailing.push(1);
    let (trailing, record) = metadata_record(0x2000_8027, trailing);
    let table = crate::container::Table {
        typecode: 0x1000_0014,
        range: 0..trailing.len(),
        body: 0..trailing.len(),
        records: vec![record],
        record_count: 1,
        object_typecodes: std::collections::BTreeMap::new(),
    };
    let mut warnings = Vec::new();
    let metadata = settings::parse_metadata(&trailing, ArchiveVersion::V5, &[table], &mut warnings);
    assert_eq!(metadata.properties.as_file_name.as_deref(), Some("X"));
    assert!(warnings.is_empty());
}

#[test]
fn parses_layer_class_wrapper_and_rendering_chunk() {
    let archive = ArchiveVersion::V5;
    let mut payload = vec![0x18];
    payload.extend(0_i32.to_le_bytes());
    payload.extend(7_i32.to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend([10, 20, 30, 255]);
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(1.0_f64.to_le_bytes());
    payload.extend(2_u32.to_le_bytes());
    payload.extend([b'L', 0, 0, 0]);
    payload.push(1);
    payload.extend((-1_i32).to_le_bytes());
    payload.extend([0, 0, 0, 255]);
    payload.extend(0.0_f64.to_le_bytes());
    payload.push(0);
    payload.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut rendering = Vec::new();
    rendering.extend(1_i32.to_le_bytes());
    rendering.extend(0_i32.to_le_bytes());
    rendering.extend(0_i32.to_le_bytes());
    payload.extend(crc_chunk(archive, 0x4000_8000, &rendering));
    payload.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    payload[0] = 0x1f;
    let mut linetype = Vec::new();
    linetype.extend(1_i32.to_le_bytes());
    linetype.extend(1_i32.to_le_bytes());
    linetype.extend(0_i32.to_le_bytes());
    linetype.extend(0_u32.to_le_bytes());
    linetype.extend(0_i32.to_le_bytes());
    linetype.extend([0; 16]);
    payload.push(33);
    payload.extend(crc_chunk(archive, 0x4000_8000, &linetype));
    payload.extend([34, 1]);
    let mut section_style = Vec::new();
    section_style.extend(1_i32.to_le_bytes());
    section_style.extend(1_i32.to_le_bytes());
    let model_attributes = crc_chunk(archive, 0x4000_8002, &[]);
    section_style.extend(&model_attributes);
    section_style.push(0);
    payload.push(35);
    #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child.
    payload.extend(crc_chunk_excluding(
        archive,
        0x4000_8000,
        &section_style,
        &[8..8 + model_attributes.len()],
    ));
    payload.extend([36, 0, 0]);
    let class_uuid = [
        0x13, 0x98, 0x80, 0x95, 0x85, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ];
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let class = long_chunk(
        archive,
        0x0002_7ffa,
        &[
            long_chunk(archive, 0x0002_fffb, &uuid_body),
            crc_chunk(archive, 0x0002_fffc, &payload),
            short_chunk(archive, 0x8002_7fff, 0),
        ]
        .concat(),
    );
    let (data, record) = metadata_record(0x2000_8050, class);
    let mut wrapper_warnings = Vec::new();
    let class_descriptor = crate::objects::parse_class_wrapper(
        &data,
        record.body.clone(),
        archive,
        &mut wrapper_warnings,
    )
    .expect("required invariant");
    assert_eq!(class_descriptor.class_data_range.len(), payload.len());
    let table = crate::container::Table {
        typecode: 0x1000_0011,
        range: 0..data.len(),
        body: 0..data.len(),
        records: vec![record],
        record_count: 1,
        object_typecodes: std::collections::BTreeMap::new(),
    };
    let mut warnings = Vec::new();
    let metadata = settings::parse_metadata(&data, archive, &[table], &mut warnings);
    assert_eq!(metadata.layers.len(), 1, "{warnings:?}");
    assert_eq!(metadata.layers[0].name, "L");
    assert_eq!(
        metadata.layers[0]
            .embedded_linetype
            .as_ref()
            .map(|value| value.version),
        Some((1, 1))
    );
    assert_eq!(
        metadata.layers[0]
            .embedded_section_style
            .as_ref()
            .map(|value| value.version),
        Some((1, 1))
    );
    assert!(warnings.is_empty());

    let mut future_payload = payload.clone();
    future_payload.pop();
    future_payload.extend([0xfe, 0xaa, 0xbb, 0, 0xde]);
    let future_class = long_chunk(
        archive,
        0x0002_7ffa,
        &[
            long_chunk(archive, 0x0002_fffb, &uuid_body),
            crc_chunk(archive, 0x0002_fffc, &future_payload),
            short_chunk(archive, 0x8002_7fff, 0),
        ]
        .concat(),
    );
    let (future_data, future_record) = metadata_record(0x2000_8050, future_class);
    let future_table = crate::container::Table {
        typecode: 0x1000_0011,
        range: 0..future_data.len(),
        body: 0..future_data.len(),
        records: vec![future_record.clone()],
        record_count: 1,
        object_typecodes: std::collections::BTreeMap::new(),
    };
    let mut future_warnings = Vec::new();
    let future =
        settings::parse_metadata(&future_data, archive, &[future_table], &mut future_warnings);
    assert_eq!(future.layers.len(), 1, "{future_warnings:?}");
    assert_eq!(future.layers[0].extension_items, vec![33, 34, 35, 36]);
    assert!(future.opaque_records.is_empty());
}

#[test]
fn future_linetype_extension_stops_at_unknown_code() {
    let archive = ArchiveVersion::V8;
    let model_attributes = crc_chunk(archive, 0x4000_8002, &[]);
    let mut body = Vec::new();
    body.extend(2_i32.to_le_bytes());
    body.extend(4_i32.to_le_bytes());
    body.extend(&model_attributes);
    body.extend(0_i32.to_le_bytes());
    body.extend([7, 0xaa, 0xbb, 0, 0xde]);
    #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child.
    let chunk = crc_chunk_excluding(
        archive,
        0x4000_8000,
        &body,
        &[8..8 + model_attributes.len()],
    );
    let mut reader = BoundedReader::new(&chunk, 0, chunk.len()).expect("bounded linetype");
    let mut warnings = Vec::new();
    let descriptor = settings::parse_direct_linetype(&chunk, &mut reader, archive, &mut warnings)
        .expect("future linetype code is bounded by the anonymous chunk");
    assert_eq!(descriptor.version, (2, 4));
    assert_eq!(reader.remaining(), 0);
    assert!(warnings.is_empty());
}

#[test]
fn parses_selector_widths_from_their_serialized_forms() {
    let mut settings_value = settings::DocumentSettings::default();
    let mut material_data = 42_i32.to_le_bytes().to_vec();
    material_data.extend(3_i32.to_le_bytes());
    let material_record = crate::container::Record {
        typecode: 0x2000_8039,
        range: 0..8,
        body: 0..8,
        short: false,
        value: 8,
    };
    settings::parse_setting(&material_data, &material_record, &mut settings_value)
        .expect("required invariant");
    assert_eq!(settings_value.current_material, Some(42));
    assert_eq!(settings_value.current_material_source, Some(3));

    let mut color_data = vec![1, 2, 3, 4];
    color_data.extend(2_i32.to_le_bytes());
    let color_record = crate::container::Record {
        typecode: 0x2000_803a,
        range: 0..8,
        body: 0..8,
        short: false,
        value: 8,
    };
    settings::parse_setting(&color_data, &color_record, &mut settings_value)
        .expect("required invariant");
    assert_eq!(settings_value.current_color, Some([1, 2, 3, 4]));
    assert_eq!(settings_value.current_color_source, Some(2));

    for (typecode, value) in [
        (0xa000_0038, 3),
        (0xa000_003c, 5),
        (0xa000_0132, 7),
        (0xa000_0133, 9),
    ] {
        let record = crate::container::Record {
            typecode,
            range: 0..0,
            body: 0..0,
            short: true,
            value,
        };
        settings::parse_setting(&[], &record, &mut settings_value).expect("required invariant");
    }
    assert_eq!(settings_value.current_layer, Some(3));
    assert_eq!(settings_value.current_wire_density, Some(5));
    assert_eq!(settings_value.current_font, Some(7));
    assert_eq!(settings_value.current_dimstyle, Some(9));
}

#[test]
fn duplicate_singleton_settings_use_the_later_valid_record_and_report_it() {
    let table = crate::container::Table {
        typecode: 0x1000_0015,
        range: 0..0,
        body: 0..0,
        records: vec![
            crate::container::Record {
                typecode: 0xa000_0038,
                range: 0..0,
                body: 0..0,
                short: true,
                value: 3,
            },
            crate::container::Record {
                typecode: 0xa000_0038,
                range: 0..0,
                body: 0..0,
                short: true,
                value: 7,
            },
        ],
        record_count: 2,
        object_typecodes: std::collections::BTreeMap::new(),
    };
    let mut warnings = Vec::new();
    let metadata = settings::parse_metadata(&[], ArchiveVersion::V5, &[table], &mut warnings);
    assert_eq!(metadata.settings.current_layer, Some(7));
    assert_eq!(
        warnings,
        vec!["duplicate singleton metadata record 0xa0000038; later record wins"]
    );
}

#[test]
fn duplicate_layer_indices_reassign_later_records_without_rebinding_originals() {
    let layer = |index| settings::LayerRecord {
        source: settings::SourceRange { range: 0..1 },
        version: (1, 15),
        obsolete_mode: 0,
        index,
        iges_level: 0,
        render_material_index: -1,
        color: [0, 0, 0, 255],
        name: String::new(),
        visible: true,
        locked: false,
        id: None,
        parent_id: None,
        expanded: None,
        linetype_index: None,
        plot_color: None,
        plot_weight: None,
        display_material_id: None,
        no_clipping_planes: None,
        rendering_range: None,
        extension_items: Vec::new(),
        embedded_linetype: None,
        embedded_section_style: None,
    };
    let mut layers = vec![layer(7), layer(7), layer(9), layer(9)];
    let mut warnings = Vec::new();
    super::reassign_duplicate_layer_indices(&mut layers, &mut warnings);
    assert_eq!(
        layers.iter().map(|layer| layer.index).collect::<Vec<_>>(),
        vec![7, 10, 9, 11]
    );
    assert_eq!(warnings.len(), 2);
    assert!(warnings[0].contains("duplicate layer index 7"));
    assert!(warnings[0].contains("assigned new index 10"));
    assert!(warnings[1].contains("duplicate layer index 9"));
    assert!(warnings[1].contains("assigned new index 11"));
}
