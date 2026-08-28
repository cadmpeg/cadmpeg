// SPDX-License-Identifier: Apache-2.0
#![allow(dead_code, clippy::disallowed_methods)]

use crate::chunks::{ArchiveVersion, BoundedReader};
use crate::settings;
use crate::test_support::test_dump::*;
use crate::wire::Uuid;

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
fn accepts_future_units_version_with_source_prefix_and_bounded_suffix() {
    let mut body = Vec::new();
    body.extend(103_i32.to_le_bytes());
    body.extend(8_i32.to_le_bytes());
    body.extend(0.5_f64.to_le_bytes());
    body.extend(0.01_f64.to_le_bytes());
    body.extend(0.001_f64.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(6_i32.to_le_bytes());
    body.extend(0.0254_f64.to_le_bytes());
    body.extend(0_u32.to_le_bytes());
    body.extend([0xde, 0xad]);
    let (data, record) = metadata_record(0x2000_8031, body);
    let units = settings::parse_units(&data, &record).expect("future units version");
    assert_eq!(units.version, 103);
    assert_eq!(units.unit, settings::UnitSystem::Standard(8));
    assert_eq!(units.distance_display_precision, Some(6));
    assert_eq!(units.millimeters_per_unit, Some(25.4));
}

#[test]
fn property_readers_follow_source_version_gates_and_boundaries() {
    let mut revision_body = vec![0x1f];
    revision_body.extend(utf16_bytes("creator"));
    revision_body.extend((0..8).flat_map(i32::to_le_bytes));
    revision_body.extend(utf16_bytes("editor"));
    revision_body.extend((8..16).flat_map(i32::to_le_bytes));
    revision_body.extend(7_i32.to_le_bytes());
    revision_body.extend([0xde, 0xad]);
    let (revision_data, revision_record) = metadata_record(0x2000_8021, revision_body);
    let revision = settings::parse_revision(&revision_data, &revision_record)
        .expect("revision-history future minor");
    assert_eq!(revision.created_by, "creator");
    assert_eq!(revision.last_edited_by, "editor");
    assert_eq!(revision.revision_count, 7);

    let mut notes_body = vec![0x1f];
    notes_body.extend(1_i32.to_le_bytes());
    notes_body.extend(utf16_bytes("notes"));
    notes_body.extend(1_i32.to_le_bytes());
    notes_body.extend([10_i32, 20, 30, 40].into_iter().flat_map(i32::to_le_bytes));
    notes_body.push(1);
    notes_body.extend([0xbe, 0xef]);
    let (notes_data, notes_record) = metadata_record(0x2000_8022, notes_body);
    let notes = settings::parse_notes(&notes_data, &notes_record).expect("notes future minor");
    assert_eq!(notes.text, "notes");
    assert!(notes.locked);

    let mut application_body = vec![0x2f];
    application_body.extend(utf16_bytes("app"));
    application_body.extend(utf16_bytes("https://example.test"));
    application_body.extend(utf16_bytes("details"));
    application_body.extend([0xaa, 0xbb]);
    let (application_data, application_record) = metadata_record(0x2000_8024, application_body);
    let application = settings::parse_application(&application_data, &application_record)
        .expect("application future major");
    assert_eq!(application.name, "app");
    assert_eq!(application.url, "https://example.test");
    assert_eq!(application.details, "details");
}

#[test]
fn parses_plugin_list_entries_and_bounded_future_minors() {
    let archive = ArchiveVersion::V8;
    let plugin_payload = |minor: i32, detailed: bool| {
        let mut body = (1_u8..=16).collect::<Vec<_>>();
        body.extend(7_i32.to_le_bytes());
        body.extend(utf16_bytes("WitnessPlugin"));
        body.extend(utf16_bytes("4.5.6"));
        body.extend(utf16_bytes("witness-plugin.rhp"));
        if detailed {
            for value in [
                "Witness Org",
                "1 Test Street",
                "NO",
                "+47 12345678",
                "dev@example.test",
                "https://example.test/plugin",
                "https://example.test/update",
                "+47 87654321",
            ] {
                body.extend(utf16_bytes(value));
            }
            body.extend(2_i32.to_le_bytes());
            body.extend(202_400_i32.to_le_bytes());
            body.extend(3_i32.to_le_bytes());
        }
        if minor > 2 {
            body.extend([0xbe, 0xef]);
        }
        anonymous_chunk(archive, minor, &body)
    };

    let mut body = vec![0x1f];
    body.extend(2_i32.to_le_bytes());
    body.extend(plugin_payload(15, true));
    body.extend(plugin_payload(0, false));
    body.extend([0xde, 0xad]);

    let (data, record) = metadata_record(0x2000_8135, body);
    let list = settings::parse_plugin_list(&data, &record, archive).expect("plugin list");
    assert_eq!(list.version, (1, 15));
    assert_eq!(list.plugins.len(), 2);
    let plugin = &list.plugins[0];
    assert_eq!(plugin.version, (1, 15));
    assert_eq!(
        plugin.plugin_id,
        Uuid::from_wire((1_u8..=16).collect::<Vec<_>>().try_into().expect("UUID"))
    );
    assert_eq!(plugin.plugin_type, 7);
    assert_eq!(plugin.name, "WitnessPlugin");
    assert_eq!(plugin.version_string, "4.5.6");
    assert_eq!(plugin.filename, "witness-plugin.rhp");
    assert_eq!(plugin.developer_email.as_deref(), Some("dev@example.test"));
    assert_eq!(plugin.platform, Some(2));
    assert_eq!(plugin.sdk_version, Some(202_400));
    assert_eq!(plugin.sdk_service_release, Some(3));
    assert_eq!(list.plugins[1].version, (1, 0));
    assert!(list.plugins[1].developer_email.is_none());
    assert!(list.plugins[1].platform.is_none());
}

#[test]
fn parses_settings_attributes_prefix_nested_records_and_future_minor_suffix() {
    let archive = ArchiveVersion::V8;
    let mut body = vec![0x1f];
    body.extend(2.5_f64.to_le_bytes());
    body.extend([10, 20, 30, 40]);
    body.extend(1_i32.to_le_bytes());
    body.extend((-1_i32).to_le_bytes());
    body.extend(1_i32.to_le_bytes());

    let mut page = Vec::new();
    page.extend(102_i32.to_le_bytes());
    page.extend(8_i32.to_le_bytes());
    page.extend(0.5_f64.to_le_bytes());
    page.extend(0.01_f64.to_le_bytes());
    page.extend(0.001_f64.to_le_bytes());
    page.extend(0_i32.to_le_bytes());
    page.extend(6_i32.to_le_bytes());
    page.extend(0.0254_f64.to_le_bytes());
    page.extend(utf16_bytes(""));
    body.extend(anonymous_chunk(archive, 0, &page));

    body.extend(uuid_bytes());
    for value in [1.0_f64, 2.0, 3.0] {
        body.extend(value.to_le_bytes());
    }

    let mut earth = Vec::new();
    for value in [
        10.0_f64, 20.0, 30.0, 4.0, 5.0, 6.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0,
    ] {
        earth.extend(value.to_le_bytes());
    }
    earth.extend(1_i32.to_le_bytes());
    earth.extend(uuid_bytes());
    earth.extend(utf16_bytes("Earth"));
    earth.extend(utf16_bytes("Description"));
    earth.extend(utf16_bytes("https://example.test"));
    earth.extend(utf16_bytes("tag"));
    earth.extend(2_i32.to_le_bytes());
    body.extend(anonymous_chunk(archive, 2, &earth));

    body.push(1);
    body.extend(anonymous_chunk(archive, 0, &[1, 0, 0, 0, 0]));

    body.push(0x15);
    body.extend(1_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    for value in [0.5_f64, 0.1, 10.0, 6.0] {
        body.extend(value.to_le_bytes());
    }
    body.extend(2_i32.to_le_bytes());
    body.extend(8_i32.to_le_bytes());
    for value in [0.3_f64, 1.2, 0.4, 0.5] {
        body.extend(value.to_le_bytes());
    }
    body.extend(2_i32.to_le_bytes());
    body.extend(2_i32.to_le_bytes());
    body.push(1);
    body.extend(0.25_f64.to_le_bytes());
    body.push(1);
    body.push(1);
    body.extend(anonymous_chunk(archive, 3, &[4, 0, 0, 0, 2, 0, 0, 0, 1, 0]));

    for value in 0..6 {
        body.extend((value as u8 + 1..=value as u8 + 16).collect::<Vec<_>>());
    }
    body.extend([0xde, 0xad]);

    let (data, record) = metadata_record(0x2000_8134, body);
    let attributes =
        settings::parse_settings_attributes(&data, &record, archive).expect("attributes");
    assert_eq!(attributes.version, (1, 15));
    assert_eq!(attributes.linetype_display_scale, 2.5);
    assert_eq!(attributes.current_plot_color, [10, 20, 30, 40]);
    assert_eq!(attributes.current_line_pattern_index, -1);
    assert_eq!(
        attributes
            .page_units
            .as_ref()
            .and_then(|value| value.distance_display_precision),
        Some(6)
    );
    assert_eq!(
        attributes.model_basepoint,
        Some(settings::Point3([1.0, 2.0, 3.0]))
    );
    let earth = attributes.earth_anchor.expect("earth anchor");
    assert_eq!(earth.version, (1, 2));
    assert_eq!(earth.name.as_deref(), Some("Earth"));
    assert_eq!(earth.coordinate_system, Some(2));
    assert_eq!(
        attributes
            .io_settings
            .as_ref()
            .map(|value| value.idef_link_update),
        Some(1)
    );
    let mesh = attributes.custom_render_mesh.expect("custom mesh");
    assert_eq!(mesh.version, (1, 5));
    assert_eq!(mesh.face_type, 2);
    assert_eq!(mesh.subd.as_ref().map(|value| value.version), Some(3));
    assert_eq!(
        attributes.current_hatch_pattern_id,
        Some(Uuid::from_wire(
            (6_u8..=21).collect::<Vec<_>>().try_into().expect("UUID"),
        ))
    );
}

#[test]
fn top_level_mesh_settings_use_outer_boundary_for_future_minor_suffix() {
    let archive = ArchiveVersion::V8;
    let mut body = vec![0x1f];
    body.extend(1_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    for value in [0.5_f64, 0.1, 10.0, 6.0] {
        body.extend(value.to_le_bytes());
    }
    body.extend(2_i32.to_le_bytes());
    body.extend(8_i32.to_le_bytes());
    for value in [0.3_f64, 1.2, 0.4, 0.5] {
        body.extend(value.to_le_bytes());
    }
    body.extend(2_i32.to_le_bytes());
    body.extend(2_i32.to_le_bytes());
    body.push(1);
    body.extend(0.25_f64.to_le_bytes());
    body.push(1);
    body.push(1);
    body.extend(anonymous_chunk(archive, 3, &[4, 0, 0, 0, 2, 0, 0, 0, 1, 0]));
    body.extend([0xde, 0xad]);

    let (data, render_record) = metadata_record(0x2000_8032, body.clone());
    let (analysis_data, analysis_record) = metadata_record(0x2000_8033, body);
    let mut settings_value = settings::DocumentSettings::default();
    settings::parse_setting(&data, &render_record, &mut settings_value, archive)
        .expect("render mesh settings");
    settings::parse_setting(
        &analysis_data,
        &analysis_record,
        &mut settings_value,
        archive,
    )
    .expect("analysis mesh settings");
    assert_eq!(
        settings_value
            .render_mesh_settings
            .as_ref()
            .map(|value| value.version),
        Some((1, 15))
    );
    assert_eq!(
        settings_value
            .analysis_mesh_settings
            .as_ref()
            .and_then(|value| value.subd.as_ref())
            .map(|value| value.version),
        Some(3)
    );
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
    payload.extend([36, 0, 37]);
    payload.extend(12_u32.to_le_bytes());
    payload.extend(
        "layer notes"
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes),
    );
    payload.push(0);
    let obsolete_idef_layer_settings = Uuid::from_canonical([
        0x11, 0xee, 0x2c, 0x1f, 0xf9, 0x0d, 0x4c, 0x6a, 0xa7, 0xcd, 0xec, 0x85, 0x32, 0xe1, 0xe3,
        0x2d,
    ])
    .to_wire();
    let obsolete_layer_settings = Uuid::from_canonical([
        0xbf, 0xb6, 0x3c, 0x09, 0x4b, 0xc7, 0x47, 0x27, 0x89, 0xbb, 0x7c, 0xc7, 0x54, 0x11, 0x82,
        0x00,
    ])
    .to_wire();
    let opennurbs5_application = Uuid::from_canonical([
        0xc8, 0xcd, 0xa5, 0x97, 0xd9, 0x57, 0x46, 0x25, 0xa4, 0xb3, 0xa0, 0xb5, 0x10, 0xfc, 0x30,
        0xd4,
    ])
    .to_wire();
    let obsolete_userdata = [
        class_userdata_with_payload(
            archive,
            obsolete_idef_layer_settings,
            opennurbs5_application,
            &[0xde, 0xad, 0xbe, 0xef],
        ),
        class_userdata_with_payload(
            archive,
            obsolete_layer_settings,
            opennurbs5_application,
            &[0xca, 0xfe, 0xba, 0xbe],
        ),
    ]
    .concat();
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
            obsolete_userdata,
            short_chunk(archive, 0x8002_7fff, 0),
        ]
        .concat(),
    );
    let (data, record) = metadata_record(0x2000_8050, class);
    let mut wrapper_warnings = Vec::new();
    let (class_descriptor, userdata) = crate::objects::parse_class_wrapper_with_userdata(
        &data,
        record.body.clone(),
        archive,
        &mut wrapper_warnings,
    )
    .expect("required invariant");
    assert_eq!(class_descriptor.class_data_range.len(), payload.len());
    assert_eq!(userdata.len(), 2);
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
    assert_eq!(metadata.layers[0].index, 7);
    assert_eq!(metadata.layers[0].iges_level, -1);
    assert_eq!(metadata.layers[0].render_material_index, -1);
    assert_eq!(metadata.layers[0].color, [10, 20, 30, 255]);
    assert_eq!(metadata.layers[0].name, "L");
    assert_eq!(
        metadata.layers[0].description.as_deref(),
        Some("layer notes")
    );
    assert!(metadata.layers[0].visible);
    assert!(!metadata.layers[0].locked);
    assert_eq!(metadata.layers[0].visible_in_new_details, Some(true));
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
    assert!(
        warnings
            .iter()
            .all(|warning| warning.contains(LAYER_PARENT_DIALECT)),
        "{warnings:?}"
    );

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
    assert_eq!(future.layers[0].extension_items, vec![33, 34, 35, 36, 37]);
    assert!(future.opaque_records.is_empty());
}

/// Marker of the diagnostic raised for an unstamped layer record.
const LAYER_PARENT_DIALECT: &str = "layer parent link and expanded state were not read";

fn layer_metadata_with_extension(extension: &[u8]) -> settings::DocumentMetadata {
    let (metadata, warnings) = layer_metadata(extension, None);
    assert!(
        warnings
            .iter()
            .all(|warning| warning.contains(LAYER_PARENT_DIALECT)),
        "{warnings:?}"
    );
    metadata
}

/// Parses one layer record, with the writer-version stamp under test control.
///
/// A `Some` stamp is delivered the way an archive delivers it: a short
/// writer-version record in a properties table ahead of the layer table. The
/// payload follows the stamp: a stamped archive carries the parent link and the
/// expanded flag that the stamped reading consumes, an unstamped one does not,
/// so each arm parses a record its own reading admits.
fn layer_metadata(
    extension: &[u8],
    writer_version: Option<i64>,
) -> (settings::DocumentMetadata, Vec<String>) {
    let archive = ArchiveVersion::V8;
    let mut payload = vec![0x1f];
    payload.extend(0_i32.to_le_bytes());
    payload.extend(7_i32.to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0, 0, 0, 255]);
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(1.0_f64.to_le_bytes());
    payload.extend(utf16_bytes("L"));
    payload.push(1);
    payload.extend((-1_i32).to_le_bytes());
    payload.extend([0, 0, 0, 255]);
    payload.extend(0.0_f64.to_le_bytes());
    payload.push(0);
    payload.extend([0; 16]);
    if writer_version.is_some() {
        payload.extend([0x44; 16]);
        payload.push(1);
    }
    payload.extend(crc_chunk(
        archive,
        0x4000_8000,
        &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ));
    payload.extend([0; 16]);
    payload.extend_from_slice(extension);

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
    let table = crate::container::Table {
        typecode: 0x1000_0011,
        range: 0..data.len(),
        body: 0..data.len(),
        records: vec![record],
        record_count: 1,
        object_typecodes: std::collections::BTreeMap::new(),
    };
    let mut tables = Vec::new();
    if let Some(value) = writer_version {
        tables.push(crate::container::Table {
            typecode: 0x1000_0014,
            range: 0..0,
            body: 0..0,
            records: vec![crate::container::Record {
                typecode: 0xa000_0026,
                range: 0..0,
                body: 0..0,
                short: true,
                value,
            }],
            record_count: 1,
            object_typecodes: std::collections::BTreeMap::new(),
        });
    }
    tables.push(table);
    let mut warnings = Vec::new();
    let metadata = settings::parse_metadata(&data, archive, &tables, &mut warnings);
    (metadata, warnings)
}

/// The layer parent link rests on the stamp, so the loss follows the stamp.
#[test]
fn unstamped_layer_charges_the_parent_link_stamp_loss() {
    // A single zero closes the extension-item chain, so both arms read a whole
    // record and the difference between them is only the stamp.
    let (unstamped_metadata, unstamped) = layer_metadata(&[0], None);
    assert_eq!(unstamped_metadata.layers.len(), 1, "{unstamped:?}");
    assert_eq!(unstamped_metadata.layers[0].parent_id, None);
    assert_eq!(unstamped_metadata.layers[0].expanded, None);
    assert!(
        unstamped
            .iter()
            .any(|warning| warning.contains(LAYER_PARENT_DIALECT)),
        "{unstamped:?}"
    );

    // The stamped arm must read a layer, or its silence proves nothing.
    let (stamped_metadata, stamped) = layer_metadata(&[0], Some(200_912_010));
    assert_eq!(stamped_metadata.layers.len(), 1, "{stamped:?}");
    assert_eq!(
        stamped_metadata.layers[0].parent_id,
        Some(Uuid::from_canonical([0x44; 16]))
    );
    assert_eq!(stamped_metadata.layers[0].expanded, Some(true));
    assert!(
        !stamped
            .iter()
            .any(|warning| warning.contains(LAYER_PARENT_DIALECT)),
        "{stamped:?}"
    );
}

fn layer_metadata_with_description(description: &str) -> settings::DocumentMetadata {
    let mut extension = vec![37];
    extension.extend(utf16_bytes(description));
    extension.push(0);
    layer_metadata_with_extension(&extension)
}

#[test]
fn layer_description_uses_opennurbs_trim_set() {
    for (description, expected) in [
        ("\u{1680}description\u{1680}", "\u{1680}description\u{1680}"),
        ("\u{205f}description\u{205f}", "\u{205f}description\u{205f}"),
        ("\u{3000}description\u{3000}", "\u{3000}description\u{3000}"),
        (" description ", "description"),
    ] {
        let metadata = layer_metadata_with_description(description);
        assert_eq!(metadata.layers.len(), 1);
        assert_eq!(metadata.layers[0].description.as_deref(), Some(expected));
        assert_eq!(metadata.layers[0].extension_items, vec![37]);
    }
}

#[test]
fn layer_out_of_order_id_leaves_value_at_boundary() {
    // Item 33 follows item 34. The source cascade consumes the ID and closes
    // the class-data scan; the following byte is not a linetype payload.
    let metadata = layer_metadata_with_extension(&[34, 1, 33, 0xaa]);
    assert_eq!(metadata.layers.len(), 1);
    assert_eq!(metadata.layers[0].extension_items, vec![34]);
    assert!(metadata.layers[0].embedded_linetype.is_none());
}

#[test]
fn rendering_attributes_accept_layer_future_minor_suffix() {
    let bytes = crc_chunk(
        ArchiveVersion::V8,
        0x4000_8000,
        &[1, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0xaa, 0xbb],
    );
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded chunk reader");
    let mut warnings = Vec::new();
    let range = settings::parse_rendering_attributes(
        &bytes,
        &mut reader,
        ArchiveVersion::V8,
        settings::RenderingAttributesKind::Layer,
        &mut warnings,
    )
    .expect("layer reader preserves a later anonymous minor suffix");
    assert_eq!(range, 0..bytes.len());
    assert!(warnings.is_empty());
}

#[test]
fn layer_extensions_read_effective_fields_sort_entries_and_apply_root_rule() {
    let archive = ArchiveVersion::V8;
    let first_viewport = Uuid::from_canonical([
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f,
    ]);
    let second_viewport = Uuid::from_canonical([
        0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e,
        0x2f,
    ]);
    let mut full_entry = 63_u32.to_le_bytes().to_vec();
    full_entry.extend(second_viewport.to_wire());
    full_entry.extend([10, 20, 30, 40]);
    full_entry.extend([50, 60, 70, 80]);
    full_entry.extend(1.25_f64.to_le_bytes());
    full_entry.extend([2, 2, 2]);
    full_entry.extend([0xde, 0xad]);
    let mut color_entry = 3_u32.to_le_bytes().to_vec();
    color_entry.extend(first_viewport.to_wire());
    color_entry.extend([90, 100, 110, 120]);
    let entries = [
        anonymous_chunk(archive, 2, &full_entry),
        anonymous_chunk(archive, 2, &color_entry),
    ]
    .concat();
    let mut outer_body = 2_i32.to_le_bytes().to_vec();
    outer_body.extend(entries);
    outer_body.extend([0xbe, 0xef]);
    let payload = anonymous_chunk(archive, 0, &outer_body);
    let descriptor = crate::objects::UserdataDescriptor {
        range: 0..payload.len(),
        version: (2, 2),
        class_uuid: settings::LAYER_EXTENSIONS,
        item_uuid: settings::LAYER_EXTENSIONS,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: None,
        last_saved_as_goo: None,
        archive_version: None,
        writer_version: None,
        payload_range: 0..payload.len(),
        unknown_version: false,
    };
    let values = settings::parse_layer_extensions(
        &payload,
        &descriptor,
        archive,
        Some(Uuid::from_canonical([1; 16])),
    )
    .expect("layer extensions payload");
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].viewport_id, first_viewport);
    assert_eq!(values[0].settings_mask, 3);
    assert_eq!(values[0].color, Some([90, 100, 110, 120]));
    assert_eq!(values[1].viewport_id, second_viewport);
    assert_eq!(values[1].settings_mask, 63);
    assert_eq!(values[1].plot_weight_mm, Some(1.25));
    assert_eq!(values[1].visible, Some(2));
    assert_eq!(values[1].persistent_visibility, Some(2));

    let root_values = settings::parse_layer_extensions(&payload, &descriptor, archive, None)
        .expect("root layer extensions payload");
    assert_eq!(root_values[1].settings_mask, 31);
    assert_eq!(root_values[1].persistent_visibility, None);
}

#[test]
fn layer_extensions_reject_negative_count() {
    let archive = ArchiveVersion::V8;
    let payload = anonymous_chunk(archive, 0, &(-1_i32).to_le_bytes());
    let descriptor = crate::objects::UserdataDescriptor {
        range: 0..payload.len(),
        version: (2, 2),
        class_uuid: settings::LAYER_EXTENSIONS,
        item_uuid: settings::LAYER_EXTENSIONS,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: None,
        last_saved_as_goo: None,
        archive_version: None,
        writer_version: None,
        payload_range: 0..payload.len(),
        unknown_version: false,
    };
    assert!(settings::parse_layer_extensions(&payload, &descriptor, archive, None).is_err());
}

#[test]
fn rendering_attributes_parse_object_mapping_and_future_suffix() {
    let mut channel_body = 7_i32.to_le_bytes().to_vec();
    channel_body.extend(uuid_bytes());
    for value in 0..16 {
        channel_body.extend((value as f64).to_le_bytes());
    }
    let channel = anonymous_chunk(ArchiveVersion::V8, 1, &channel_body);
    let mut mapping_body = uuid_bytes();
    mapping_body.extend(1_i32.to_le_bytes());
    mapping_body.extend(channel);
    let mut mapping_payload = 1_i32.to_le_bytes().to_vec();
    mapping_payload.extend(0_i32.to_le_bytes());
    let channel_start = mapping_payload.len() + 16 + 4;
    mapping_payload.extend(mapping_body);
    #[allow(clippy::single_range_in_vec_init)] // One nested mapping-channel range.
    let mapping = crc_chunk_excluding(
        ArchiveVersion::V8,
        0x4000_8000,
        &mapping_payload,
        &[channel_start..mapping_payload.len()],
    );

    let mut rendering_body = vec![1, 0, 0, 0, 4, 0, 0, 0];
    rendering_body.extend(0_i32.to_le_bytes());
    rendering_body.extend(1_i32.to_le_bytes());
    let mapping_start = rendering_body.len();
    rendering_body.extend(mapping);
    let mapping_end = rendering_body.len();
    rendering_body.extend([1, 0, 1]);
    rendering_body.extend([0xaa, 0xbb]);
    #[allow(clippy::single_range_in_vec_init)] // The range is one direct child.
    let bytes = crc_chunk_excluding(
        ArchiveVersion::V8,
        0x4000_8000,
        &rendering_body,
        &[mapping_start..mapping_end],
    );
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded chunk reader");
    let mut warnings = Vec::new();
    let range = settings::parse_rendering_attributes(
        &bytes,
        &mut reader,
        ArchiveVersion::V8,
        settings::RenderingAttributesKind::Object,
        &mut warnings,
    )
    .expect("object reader consumes mapping channels and leaves the suffix bounded");
    assert_eq!(range, 0..bytes.len());
    assert!(warnings.is_empty());
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
fn linetype_out_of_order_id_leaves_value_at_boundary() {
    let archive = ArchiveVersion::V8;
    let model_attributes = crc_chunk(archive, 0x4000_8002, &[]);
    let mut body = Vec::new();
    body.extend(2_i32.to_le_bytes());
    body.extend(4_i32.to_le_bytes());
    body.extend(&model_attributes);
    body.extend(0_i32.to_le_bytes());
    body.push(3);
    body.extend(2.0_f64.to_le_bytes());
    // Item 1 follows item 3. The source cascade consumes the ID and closes;
    // the one-byte value has no generic width.
    body.extend([1, 0xaa]);
    #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child.
    let chunk = crc_chunk_excluding(
        archive,
        0x4000_8000,
        &body,
        &[8..8 + model_attributes.len()],
    );
    let mut reader = BoundedReader::new(&chunk, 0, chunk.len()).expect("bounded linetype");
    let mut warnings = Vec::new();
    settings::parse_direct_linetype(&chunk, &mut reader, archive, &mut warnings)
        .expect("source ordered cascade leaves out-of-order value bounded");
    assert_eq!(reader.remaining(), 0);
    assert!(warnings.is_empty());
}

#[test]
fn future_section_style_extension_stops_at_unknown_code() {
    let archive = ArchiveVersion::V8;
    let model_attributes = crc_chunk(archive, 0x4000_8002, &[]);
    let mut body = Vec::new();
    body.extend(1_i32.to_le_bytes());
    body.extend(4_i32.to_le_bytes());
    body.extend(&model_attributes);
    body.extend([12, 0xaa, 0xbb, 0, 0xde]);
    #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child.
    let chunk = crc_chunk_excluding(
        archive,
        0x4000_8000,
        &body,
        &[8..8 + model_attributes.len()],
    );
    let mut reader = BoundedReader::new(&chunk, 0, chunk.len()).expect("bounded section style");
    let mut warnings = Vec::new();
    let descriptor =
        settings::parse_direct_section_style(&chunk, &mut reader, archive, &mut warnings)
            .expect("future section-style code is bounded by the anonymous chunk");
    assert_eq!(descriptor.version, (1, 4));
    assert_eq!(reader.remaining(), 0);
    assert!(warnings.is_empty());
}

#[test]
fn section_style_out_of_order_id_leaves_value_at_boundary() {
    let archive = ArchiveVersion::V8;
    let model_attributes = crc_chunk(archive, 0x4000_8002, &[]);
    let mut body = Vec::new();
    body.extend(1_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    body.extend(&model_attributes);
    body.push(5);
    body.extend(2.0_f64.to_le_bytes());
    // The source reader has passed item 1 after consuming item 5. It reads
    // this ID and closes the anonymous chunk; the value has no generic width.
    body.extend([1, 0xaa]);
    #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child.
    let chunk = crc_chunk_excluding(
        archive,
        0x4000_8000,
        &body,
        &[8..8 + model_attributes.len()],
    );
    let mut reader = BoundedReader::new(&chunk, 0, chunk.len()).expect("bounded section style");
    let mut warnings = Vec::new();
    settings::parse_direct_section_style(&chunk, &mut reader, archive, &mut warnings)
        .expect("source ordered cascade leaves out-of-order value bounded");
    assert_eq!(reader.remaining(), 0);
    assert!(warnings.is_empty());
}

#[test]
fn parses_selector_widths_and_skips_direct_suffix() {
    let mut settings_value = settings::DocumentSettings::default();
    let mut material_data = 42_i32.to_le_bytes().to_vec();
    material_data.extend(3_i32.to_le_bytes());
    material_data.extend([0xaa, 0xbb]);
    let material_record = crate::container::Record {
        typecode: 0x2000_8039,
        range: 0..material_data.len(),
        body: 0..material_data.len(),
        short: false,
        value: material_data.len() as i64,
    };
    settings::parse_setting(
        &material_data,
        &material_record,
        &mut settings_value,
        ArchiveVersion::V8,
    )
    .expect("required invariant");
    assert_eq!(settings_value.current_material, Some(42));
    assert_eq!(settings_value.current_material_source, Some(3));

    let mut color_data = vec![1, 2, 3, 4];
    color_data.extend(2_i32.to_le_bytes());
    color_data.extend([0xcc, 0xdd]);
    let color_record = crate::container::Record {
        typecode: 0x2000_803a,
        range: 0..color_data.len(),
        body: 0..color_data.len(),
        short: false,
        value: color_data.len() as i64,
    };
    settings::parse_setting(
        &color_data,
        &color_record,
        &mut settings_value,
        ArchiveVersion::V8,
    )
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
        settings::parse_setting(&[], &record, &mut settings_value, ArchiveVersion::V8)
            .expect("required invariant");
    }
    assert_eq!(settings_value.current_layer, Some(3));
    assert_eq!(settings_value.current_wire_density, Some(5));
    assert_eq!(settings_value.current_font, Some(7));
    assert_eq!(settings_value.current_dimstyle, Some(9));
}

#[test]
fn current_material_accepts_the_source_reader_i32_range() {
    let mut data = (-2_i32).to_le_bytes().to_vec();
    data.extend(3_i32.to_le_bytes());
    let record = crate::container::Record {
        typecode: 0x2000_8039,
        range: 0..data.len(),
        body: 0..data.len(),
        short: false,
        value: data.len() as i64,
    };
    let mut settings_value = settings::DocumentSettings::default();

    settings::parse_setting(&data, &record, &mut settings_value, ArchiveVersion::V8)
        .expect("source reader accepts every signed i32 material index");

    assert_eq!(settings_value.current_material, Some(-2));
    assert_eq!(settings_value.current_material_source, Some(3));
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
        description: None,
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
        visible_in_new_details: None,
        rendering_range: None,
        extension_items: Vec::new(),
        embedded_linetype: None,
        embedded_section_style: None,
        per_viewport_settings: Vec::new(),
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
