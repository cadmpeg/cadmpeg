// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::disallowed_methods)]

use crate::chunks::{ArchiveVersion, BoundedReader};
use crate::loss::Diagnostics;
use crate::objects::ClassUserdata;
use crate::settings;
use crate::test_support::test_dump::{
    anonymous_chunk, class_userdata_with_payload, crc_chunk, crc_chunk_excluding, long_chunk,
    metadata_record, short_chunk, utf16_bytes, uuid_bytes,
};
use crate::wire::Uuid;
use std::sync::OnceLock;

fn parse_test_metadata(
    data: &[u8],
    archive: ArchiveVersion,
    tables: &[crate::container::Table],
    warnings: &mut Diagnostics,
) -> settings::DocumentMetadata {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        data,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test input fits the service profile");
    settings::parse_metadata(&ctx, data, archive, tables, warnings)
        .expect("test metadata fits the service profile")
}

fn parse_test_extensions(
    payload: &[u8],
    descriptor: &ClassUserdata,
    archive: ArchiveVersion,
    parent_id: Option<Uuid>,
) -> Result<Vec<settings::LayerPerViewportSettings>, crate::chunks::FramingError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test input fits the service profile");
    settings::parse_layer_extensions(&ctx, payload, descriptor, archive, parent_id)
}

/// Header and checksum bytes surrounding a fixture table body.
const TABLE_FRAMING: usize = 8;

/// A table whose body is the first `len` bytes of a chunk framed by
/// [`TABLE_FRAMING`] bytes.
fn metadata_table(
    typecode: u32,
    len: usize,
    records: Vec<crate::container::Record>,
) -> crate::container::Table {
    let record_count = records.len();
    crate::container::Table::new(
        typecode,
        0..len + TABLE_FRAMING,
        0..len,
        records,
        record_count,
        std::collections::BTreeMap::new(),
    )
    .expect("a fixture table body lies inside its framing")
}

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
fn retained_utf16_charges_exact_utf8_length_and_preserves_surrogates() {
    let bytes = utf16_bytes("é😀");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut limited = cadmpeg_core::decode::DecodePolicy::service();
    limited.limits.max_retained_bytes = 5;
    let (limited_ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &limited)
            .expect("root bytes admitted");
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded UTF-16 reader");
    let error = settings::utf16_retained(&limited_ctx, &mut reader, "Rhino test UTF-16 text")
        .expect_err("six UTF-8 bytes exceed five retained bytes");
    assert!(matches!(
        error,
        crate::chunks::FramingError::Resource(refusal)
            if refusal.operation == "Rhino test UTF-16 text"
    ));

    let mut admitted = cadmpeg_core::decode::DecodePolicy::service();
    admitted.limits.max_retained_bytes = 6;
    let (admitted_ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &admitted)
            .expect("root bytes admitted");
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded UTF-16 reader");
    assert_eq!(
        settings::utf16_retained(&admitted_ctx, &mut reader, "Rhino test UTF-16 text")
            .expect("exact UTF-8 length admitted"),
        "é😀"
    );
}

#[test]
fn retained_utf16_preserves_invalid_surrogate_error() {
    let mut bytes = Vec::new();
    bytes.extend(2_u32.to_le_bytes());
    bytes.extend(0xd83d_u16.to_le_bytes());
    bytes.extend(0_u16.to_le_bytes());
    let mut original = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded UTF-16 reader");
    let expected = settings::utf16(&mut original).expect_err("unpaired surrogate is invalid");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("root bytes admitted");
    let mut retained = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded UTF-16 reader");
    assert_eq!(
        settings::utf16_retained(&ctx, &mut retained, "Rhino test UTF-16 text")
            .expect_err("unpaired surrogate is invalid"),
        expected
    );
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
fn unit_binding_keeps_native_and_unavailable_distinct_from_physical_scale() {
    let physical = settings::UnitsAndTolerances {
        unit: settings::UnitSystem::Standard(settings::StandardUnit::Inches),
        absolute_tolerance: crate::test_support::positive(0.01),
        absolute_tolerance_millimeters: cadmpeg_ir::scalar::PositiveLength::new(0.01 * 25.4),
        angular_tolerance: crate::test_support::positive_angle(0.1),
        relative_tolerance: crate::test_support::positive(0.01),
        distance_display: None,
    };
    let native = settings::UnitsAndTolerances {
        unit: settings::UnitSystem::None,
        ..physical.clone()
    };
    let unavailable = settings::UnitsAndTolerances {
        unit: settings::UnitSystem::Unset,
        ..physical
    };

    assert_eq!(
        settings::UnitBinding::from_units(Some(&physical)),
        settings::UnitBinding::Millimeters(settings::StandardUnit::Inches.into())
    );
    assert_eq!(
        settings::UnitBinding::from_units(Some(&native)),
        settings::UnitBinding::Native
    );
    assert_eq!(
        settings::UnitBinding::from_units(Some(&unavailable)),
        settings::UnitBinding::Unavailable
    );
    assert_eq!(
        settings::UnitBinding::from_units(None),
        settings::UnitBinding::Unavailable
    );
    assert_eq!(
        settings::UnitBinding::from_units(Some(&physical)).neutral_scale(),
        Some(settings::MillimeterScale::from(
            settings::StandardUnit::Inches
        ))
    );
    assert_eq!(settings::UnitBinding::Native.neutral_scale(), None);
    assert_eq!(settings::UnitBinding::Unavailable.neutral_scale(), None);
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
    let units = settings::parse_units(
        &cadmpeg_test_support::service_decode_context(),
        &data,
        &record,
    )
    .expect("required invariant");
    assert_eq!(units.millimeters_per_unit(), Some(25.4));
    assert_eq!(units.absolute_tolerance, crate::test_support::positive(0.5));
    assert_eq!(
        units
            .absolute_tolerance_millimeters()
            .map(cadmpeg_ir::scalar::PositiveLength::get),
        Some(12.7)
    );
    assert_eq!(
        units.angular_tolerance,
        crate::test_support::positive_angle(0.01)
    );
    assert_eq!(
        units.relative_tolerance,
        crate::test_support::positive(0.001)
    );

    let mut legacy = Vec::new();
    legacy.extend(1_i32.to_le_bytes());
    legacy.extend(2_i32.to_le_bytes());
    legacy.extend(0.5_f64.to_le_bytes());
    legacy.extend(0.002_f64.to_le_bytes());
    legacy.extend(0.01_f64.to_le_bytes());
    let (data, record) = metadata_record(0x2000_8031, legacy);
    let units = settings::parse_units(
        &cadmpeg_test_support::service_decode_context(),
        &data,
        &record,
    )
    .expect("required invariant");
    assert_eq!(
        units.relative_tolerance,
        crate::test_support::positive(0.002)
    );
    assert_eq!(
        units.angular_tolerance,
        crate::test_support::positive_angle(0.01)
    );
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
    let units = settings::parse_units(
        &cadmpeg_test_support::service_decode_context(),
        &data,
        &record,
    )
    .expect("future units version");
    assert_eq!(
        units.unit,
        settings::UnitSystem::Standard(settings::StandardUnit::Inches)
    );
    assert_eq!(
        units.distance_display.map(|display| display.precision),
        Some(6)
    );
    assert_eq!(units.millimeters_per_unit(), Some(25.4));
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
    let revision = settings::parse_revision(
        &cadmpeg_test_support::service_decode_context(),
        &revision_data,
        &revision_record,
    )
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
    let notes = settings::parse_notes(
        &cadmpeg_test_support::service_decode_context(),
        &notes_data,
        &notes_record,
    )
    .expect("notes future minor");
    assert_eq!(notes.text, "notes");
    assert!(notes.locked);

    let mut application_body = vec![0x2f];
    application_body.extend(utf16_bytes("app"));
    application_body.extend(utf16_bytes("https://example.test"));
    application_body.extend(utf16_bytes("details"));
    application_body.extend([0xaa, 0xbb]);
    let (application_data, application_record) = metadata_record(0x2000_8024, application_body);
    let application = settings::parse_application(
        &cadmpeg_test_support::service_decode_context(),
        &application_data,
        &application_record,
    )
    .expect("application future major");
    assert_eq!(application.name, "app");
    assert_eq!(application.url, "https://example.test");
    assert_eq!(application.details, "details");
}

fn retained_limit_context<'a>(
    bytes: &'a [u8],
    arena: &'a cadmpeg_core::decode::DecodeArena,
    policy: &'a cadmpeg_core::decode::DecodePolicy,
) -> cadmpeg_core::decode::DecodeContext<'a> {
    cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, arena, policy)
        .expect("metadata root admitted")
        .0
}

fn property_text_refusal(kind: &str, limit: u64) -> crate::chunks::FramingError {
    let (typecode, body) = match kind {
        "revision" => {
            let mut body = vec![0x10];
            body.extend(utf16_bytes("creator"));
            body.extend([0; 32]);
            body.extend(utf16_bytes("editor"));
            body.extend([0; 32]);
            body.extend(1_i32.to_le_bytes());
            (0x2000_8021, body)
        }
        "notes" => {
            let mut body = vec![0x10];
            body.extend(0_i32.to_le_bytes());
            body.extend(utf16_bytes("notes"));
            body.extend(0_i32.to_le_bytes());
            body.extend([0; 16]);
            (0x2000_8022, body)
        }
        "application" => {
            let mut body = vec![0x10];
            body.extend(utf16_bytes("app"));
            body.extend(utf16_bytes("url"));
            body.extend(utf16_bytes("details"));
            (0x2000_8024, body)
        }
        _ => panic!("unknown property fixture: {kind}"),
    };
    let (data, record) = metadata_record(typecode, body);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let ctx = retained_limit_context(&data, &arena, &policy);
    match kind {
        "revision" => {
            settings::parse_revision(&ctx, &data, &record).expect_err("revision text exceeds limit")
        }
        "notes" => {
            settings::parse_notes(&ctx, &data, &record).expect_err("notes text exceeds limit")
        }
        "application" => settings::parse_application(&ctx, &data, &record)
            .expect_err("application text exceeds limit"),
        _ => panic!("unknown property fixture: {kind}"),
    }
}

macro_rules! property_text_limit {
    ($name:ident, $kind:literal, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(property_text_refusal($kind, $limit), crate::chunks::FramingError::Resource(refusal) if refusal.operation == $operation));
        }
    };
}

property_text_limit!(
    revision_creator_refuses_retained_limit,
    "revision",
    0,
    "Rhino revision creator"
);
property_text_limit!(
    revision_editor_refuses_retained_limit,
    "revision",
    7,
    "Rhino revision editor"
);
property_text_limit!(
    document_notes_refuse_retained_limit,
    "notes",
    0,
    "Rhino document notes"
);
property_text_limit!(
    application_name_refuses_retained_limit,
    "application",
    0,
    "Rhino application name"
);
property_text_limit!(
    application_url_refuses_retained_limit,
    "application",
    3,
    "Rhino application URL"
);
property_text_limit!(
    application_details_refuse_retained_limit,
    "application",
    6,
    "Rhino application details"
);

fn custom_units_body() -> Vec<u8> {
    let mut body = 102_i32.to_le_bytes().to_vec();
    body.extend(11_i32.to_le_bytes());
    body.extend(0.5_f64.to_le_bytes());
    body.extend(0.01_f64.to_le_bytes());
    body.extend(0.001_f64.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(3_i32.to_le_bytes());
    body.extend(0.001_f64.to_le_bytes());
    body.extend(utf16_bytes("custom"));
    body
}

#[test]
fn custom_unit_name_refuses_retained_limit() {
    let (data, record) = metadata_record(0x2000_8031, custom_units_body());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 5;
    let ctx = retained_limit_context(&data, &arena, &policy);
    let error = settings::parse_units(&ctx, &data, &record)
        .expect_err("custom unit name exceeds retained limit");
    assert!(
        matches!(error, crate::chunks::FramingError::Resource(refusal) if refusal.operation == "Rhino custom unit name")
    );
    let admitted = settings::parse_units(
        &cadmpeg_test_support::service_decode_context(),
        &data,
        &record,
    )
    .expect("custom unit name fits service profile");
    assert_eq!(admitted.millimeters_per_unit(), Some(1.0));
}

#[test]
fn discarded_page_units_custom_name_uses_no_retained_budget() {
    let data = custom_units_body();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let ctx = retained_limit_context(&data, &arena, &policy);
    let mut reader = BoundedReader::new(&data, 0, data.len()).expect("unit bounds");
    let units = settings::parse_units_reader(&ctx, &mut reader, false)
        .expect("discarded custom name needs no retained budget");
    assert_eq!(units.millimeters_per_unit(), Some(1.0));
}

#[test]
fn as_file_name_refuses_retained_limit() {
    let (data, record) = metadata_record(0x2000_8027, utf16_bytes("file.3dm"));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 7;
    let ctx = retained_limit_context(&data, &arena, &policy);
    let error = settings::utf16_record(&ctx, &data, &record, "Rhino as-file name")
        .expect_err("as-file name exceeds retained limit");
    assert!(
        matches!(error, crate::chunks::FramingError::Resource(refusal) if refusal.operation == "Rhino as-file name")
    );
    assert_eq!(
        settings::utf16_record(
            &cadmpeg_test_support::service_decode_context(),
            &data,
            &record,
            "Rhino as-file name",
        )
        .expect("as-file name fits service profile"),
        "file.3dm"
    );
}

#[test]
fn model_url_refuses_retained_limit() {
    let (data, record) = metadata_record(0x2000_8131, utf16_bytes("model URL"));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 8;
    let ctx = retained_limit_context(&data, &arena, &policy);
    let mut settings_value = settings::DocumentSettings::default();
    let error = settings::parse_setting(
        &ctx,
        &data,
        &record,
        &mut settings_value,
        ArchiveVersion::V8,
    )
    .expect_err("model URL exceeds retained limit");
    assert!(
        matches!(error, crate::chunks::FramingError::Resource(refusal) if refusal.operation == "Rhino model URL")
    );
    settings::parse_setting(
        &cadmpeg_test_support::service_decode_context(),
        &data,
        &record,
        &mut settings_value,
        ArchiveVersion::V8,
    )
    .expect("model URL fits service profile");
    assert_eq!(settings_value.model_url.as_deref(), Some("model URL"));
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
    settings::parse_plugin_list(&data, &record, archive).expect("plugin list");
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
    settings::parse_settings_attributes(
        &cadmpeg_test_support::service_decode_context(),
        &data,
        &record,
        archive,
    )
    .expect("attributes");
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
    settings::parse_setting(
        &cadmpeg_test_support::service_decode_context(),
        &data,
        &render_record,
        &mut settings_value,
        archive,
    )
    .expect("render mesh settings");
    settings::parse_setting(
        &cadmpeg_test_support::service_decode_context(),
        &analysis_data,
        &analysis_record,
        &mut settings_value,
        archive,
    )
    .expect("analysis mesh settings");
}

/// Each refused tolerance names the first byte of the value it refuses, which
/// is the field's own offset and not the one that follows it.
#[test]
fn nonfinite_unit_tolerances_are_refused_at_the_value_first_byte() {
    for (index, label) in [
        (0, "absolute tolerance"),
        (1, "angular tolerance"),
        (2, "relative tolerance"),
    ] {
        let mut values = [0.5_f64, 0.01, 0.001];
        values[index] = f64::NAN;
        let mut body = Vec::new();
        body.extend(100_i32.to_le_bytes());
        body.extend(8_i32.to_le_bytes());
        for value in values {
            body.extend(value.to_le_bytes());
        }
        let (data, record) = metadata_record(0x2000_8031, body);
        let error = settings::parse_units(
            &cadmpeg_test_support::service_decode_context(),
            &data,
            &record,
        )
        .expect_err("nonfinite tolerance");
        assert_eq!(
            error,
            crate::chunks::FramingError::structural(
                8 + index * 8,
                format!("{label} is not finite")
            )
        );
    }
}

/// A group refusal names the first byte of the group, not the byte after it.
#[test]
fn a_nonfinite_point_component_is_refused_at_the_point_first_byte() {
    let mut bytes = vec![0xa5, 0xa5, 0xa5];
    let point_offset = bytes.len();
    for value in [1.0_f64, f64::NAN, 3.0] {
        bytes.extend(value.to_le_bytes());
    }
    let mut reader = BoundedReader::new(&bytes, point_offset, bytes.len()).expect("point reader");
    let error = settings::point(&mut reader).expect_err("nonfinite point");
    assert_eq!(
        error,
        crate::chunks::FramingError::structural(point_offset, "point contains a nonfinite value")
    );
}

/// The interval reader hands back the finite endpoints it admitted and refuses a
/// nonfinite endpoint at the interval's first byte.
#[test]
fn an_interval_holds_its_admitted_finite_endpoints() {
    let interval_bytes = |values: [f64; 2]| {
        let mut bytes = vec![0xa5];
        for value in values {
            bytes.extend(value.to_le_bytes());
        }
        bytes
    };
    let bytes = interval_bytes([-2.5, 7.0]);
    let mut reader = BoundedReader::new(&bytes, 1, bytes.len()).expect("interval reader");
    let interval = settings::interval(&mut reader).expect("finite interval");
    assert_eq!(interval.0.get(), [-2.5, 7.0]);
    let bytes = interval_bytes([0.0, f64::INFINITY]);
    let mut reader = BoundedReader::new(&bytes, 1, bytes.len()).expect("interval reader");
    let error = settings::interval(&mut reader).expect_err("nonfinite interval");
    assert_eq!(
        error,
        crate::chunks::FramingError::structural(1, "interval contains a nonfinite value")
    );
}

/// The point, vector and transform readers hand back the finite values they
/// admitted, and the plane reader keeps the admitted coordinates of its parts.
#[test]
fn point_vector_plane_and_transform_readers_hold_their_admitted_values() {
    let mut bytes = vec![0xa5];
    for value in [1.0_f64, -2.0, 3.0, 0.0, 1.0, 0.0] {
        bytes.extend(value.to_le_bytes());
    }
    let mut reader = BoundedReader::new(&bytes, 1, bytes.len()).expect("point reader");
    let point = settings::point(&mut reader).expect("finite point");
    let vector = settings::vector(&mut reader).expect("finite vector");
    assert_eq!(point.0.get(), [1.0, -2.0, 3.0]);
    assert_eq!(vector.0.get(), [0.0, 1.0, 0.0]);

    let mut bytes = vec![0xa5];
    for value in [
        1.0_f64, 2.0, 3.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, -3.0,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    let mut reader = BoundedReader::new(&bytes, 1, bytes.len()).expect("plane reader");
    let plane = settings::plane(&mut reader).expect("finite plane");
    assert!(matches!(
        plane.origin,
        settings::CoordinateLane::Admitted(_)
    ));
    assert_eq!(plane.xaxis.get(), [1.0, 0.0, 0.0]);
    assert!(matches!(
        plane.equation,
        settings::CoordinateLane::Admitted(_)
    ));
    assert_eq!(plane.origin.get(), [1.0, 2.0, 3.0]);
    assert_eq!(plane.zaxis.get(), [0.0, 0.0, 1.0]);
    assert_eq!(plane.equation.get(), [0.0, 0.0, 1.0, -3.0]);
    let mut reader = BoundedReader::new(&bytes, 1, bytes.len()).expect("transform reader");
    let transform = settings::xform(&mut reader).expect("finite transform");
    assert_eq!(transform.0.get()[15], -3.0);
    let mut refused = bytes.clone();
    refused[1 + 8 * 15..].copy_from_slice(&f64::NAN.to_le_bytes());
    let mut reader = BoundedReader::new(&refused, 1, refused.len()).expect("transform reader");
    assert_eq!(
        settings::xform(&mut reader).expect_err("nonfinite transform"),
        crate::chunks::FramingError::structural(1, "transform contains a nonfinite value")
    );
}

/// The mesh-parameters route reads each value through the shared reader, so its
/// refusal names the value's first byte.
#[test]
fn nonfinite_mesh_tolerance_is_refused_at_the_value_first_byte() {
    let archive = ArchiveVersion::V8;
    let mut body = vec![0x1f];
    for value in [1_i32, 0, 1, 0, 0] {
        body.extend(value.to_le_bytes());
    }
    let tolerance_offset = body.len();
    for value in [f64::NAN, 0.1, 10.0, 6.0] {
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

    let (data, record) = metadata_record(0x2000_8032, body);
    let mut settings_value = settings::DocumentSettings::default();
    let error = settings::parse_setting(
        &cadmpeg_test_support::service_decode_context(),
        &data,
        &record,
        &mut settings_value,
        archive,
    )
    .expect_err("nonfinite mesh tolerance");
    assert_eq!(
        error,
        crate::chunks::FramingError::structural(tolerance_offset, "mesh tolerance is not finite")
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
    assert!(settings::parse_units(
        &cadmpeg_test_support::service_decode_context(),
        &data,
        &record
    )
    .is_err());
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
    assert!(settings::parse_units(
        &cadmpeg_test_support::service_decode_context(),
        &data,
        &record
    )
    .is_err());

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
    assert!(settings::parse_units(
        &cadmpeg_test_support::service_decode_context(),
        &data,
        &record
    )
    .is_err());
}

#[test]
fn decodes_as_file_name_as_utf16_and_skips_fixed_trailing_bytes() {
    let mut name = Vec::new();
    name.extend(2_u32.to_le_bytes());
    name.extend([b'X', 0, 0, 0]);
    let (data, record) = metadata_record(0x2000_8027, name);
    let table = metadata_table(0x1000_0014, data.len(), vec![record]);
    let mut warnings = Diagnostics::new();
    let metadata = parse_test_metadata(&data, ArchiveVersion::V5, &[table], &mut warnings);
    assert_eq!(metadata.properties.as_file_name.as_deref(), Some("X"));
    assert!(warnings.is_empty());

    let mut trailing = data;
    trailing.push(1);
    let (trailing, record) = metadata_record(0x2000_8027, trailing);
    let table = metadata_table(0x1000_0014, trailing.len(), vec![record]);
    let mut warnings = Diagnostics::new();
    let metadata = parse_test_metadata(&trailing, ArchiveVersion::V5, &[table], &mut warnings);
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
        std::slice::from_ref(&(8..8 + model_attributes.len())),
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
    let mut wrapper_warnings = Diagnostics::new();
    let (class_descriptor, userdata) = crate::objects::parse_class_wrapper_with_userdata(
        &cadmpeg_test_support::service_decode_context(),
        &data,
        record.body(),
        archive,
        &mut wrapper_warnings,
    )
    .expect("required invariant");
    assert_eq!(class_descriptor.class_data_range.len(), payload.len());
    assert_eq!(userdata.len(), 2);
    let table = metadata_table(0x1000_0011, data.len(), vec![record]);
    let mut warnings = Diagnostics::new();
    let metadata = parse_test_metadata(&data, archive, &[table], &mut warnings);
    assert_eq!(metadata.layers.len(), 1, "{warnings:?}");
    assert_eq!(metadata.layers[0].index, 7);
    assert_eq!(metadata.layers[0].iges_level, None);
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
    assert_eq!(metadata.opaque_records.len(), 1);
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
    let future_table = metadata_table(0x1000_0011, future_data.len(), vec![future_record.clone()]);
    let mut future_warnings = Diagnostics::new();
    let future = parse_test_metadata(&future_data, archive, &[future_table], &mut future_warnings);
    assert_eq!(future.layers.len(), 1, "{future_warnings:?}");
    assert_eq!(future.layers[0].extension_items, vec![33, 34, 35, 36, 37]);
    assert_eq!(future.opaque_records.len(), 1);
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
) -> (settings::DocumentMetadata, Diagnostics) {
    layer_metadata_with_record_count(extension, writer_version, 1)
}

fn layer_metadata_with_record_count(
    extension: &[u8],
    writer_version: Option<i64>,
    record_count: usize,
) -> (settings::DocumentMetadata, Diagnostics) {
    layer_metadata_with_record_count_and_id(extension, writer_version, record_count, [0; 16])
}

fn layer_metadata_with_record_count_and_id(
    extension: &[u8],
    writer_version: Option<i64>,
    record_count: usize,
    id: [u8; 16],
) -> (settings::DocumentMetadata, Diagnostics) {
    let (data, tables) = layer_fixture(extension, writer_version, record_count, id, &[]);
    let mut warnings = Diagnostics::new();
    let metadata = parse_test_metadata(&data, ArchiveVersion::V8, &tables, &mut warnings);
    (metadata, warnings)
}

fn layer_fixture(
    extension: &[u8],
    writer_version: Option<i64>,
    record_count: usize,
    id: [u8; 16],
    userdata: &[u8],
) -> (Vec<u8>, Vec<crate::container::Table>) {
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
    payload.extend(id);
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
            userdata.to_vec(),
            short_chunk(archive, 0x8002_7fff, 0),
        ]
        .concat(),
    );
    let (data, record) = metadata_record(0x2000_8050, class);
    let table = metadata_table(0x1000_0011, data.len(), vec![record; record_count]);
    let mut tables = Vec::new();
    if let Some(value) = writer_version {
        tables.push(metadata_table(
            0x1000_0014,
            0,
            vec![crate::container::Record::short(0xa000_0026, 0..0, value)],
        ));
    }
    tables.push(table);
    (data, tables)
}

fn metadata_limit_operations(
    dimension: cadmpeg_core::decode::ResourceDimension,
    source_id: [u8; 16],
    extension: &[u8],
    userdata: &[u8],
) -> Vec<&'static str> {
    let (data, mut layer_tables) =
        layer_fixture(extension, Some(200_912_010), 1, source_id, userdata);
    layer_tables.remove(0);
    let mut tables = vec![
        metadata_table(
            super::PROPERTIES,
            0,
            vec![
                crate::container::Record::short(super::WRITER_VERSION, 0..0, 200_912_010),
                crate::container::Record::long(super::PREVIEW, 0..0, 0..0),
            ],
        ),
        metadata_table(
            super::SETTINGS,
            0,
            vec![
                crate::container::Record::short(super::CURRENT_LAYER, 0..0, 3),
                crate::container::Record::long(0x2000_ffff, 0..0, 0..0),
            ],
        ),
    ];
    tables.append(&mut layer_tables);
    let mut limit = 0_u64;
    let mut operations = Vec::new();
    for _ in 0..128 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        match dimension {
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = limit;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = limit;
            }
            other => panic!("unsupported metadata test dimension: {other:?}"),
        }
        let ctx = retained_limit_context(&data, &arena, &policy);
        match settings::parse_metadata(
            &ctx,
            &data,
            ArchiveVersion::V8,
            &tables,
            &mut Diagnostics::new(),
        ) {
            Ok(metadata) => {
                assert_eq!(metadata.layers.len(), 1);
                return operations;
            }
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == dimension =>
            {
                operations.push(refusal.operation);
                let next = refusal.used + refusal.additional;
                limit = next.max(limit + 1);
            }
            Err(error) => panic!("unexpected metadata error: {error}"),
        }
    }
    panic!("metadata limit ladder did not terminate");
}

fn metadata_collection_operations() -> &'static [&'static str] {
    static OPERATIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        metadata_limit_operations(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            [0x44; 16],
            &[0],
            &[],
        )
    })
}

fn metadata_materialized_operations() -> &'static [&'static str] {
    static OPERATIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        metadata_limit_operations(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            [0x44; 16],
            &[0],
            &[],
        )
    })
}

fn metadata_opaque_operations() -> &'static [&'static str] {
    static OPERATIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        metadata_limit_operations(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            [0; 16],
            &[0],
            &[],
        )
    })
}

fn metadata_extension_operations() -> &'static [&'static str] {
    static OPERATIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        let mut extension = vec![37];
        extension.extend(utf16_bytes("description"));
        extension.push(0);
        metadata_limit_operations(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            [0x44; 16],
            &extension,
            &[],
        )
    })
}

fn metadata_userdata_operations() -> &'static [&'static str] {
    static OPERATIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        let archive = ArchiveVersion::V8;
        let mut entry = super::LAYER_PER_VIEWPORT_ID.to_le_bytes().to_vec();
        entry.extend(Uuid::from_canonical([1; 16]).to_wire());
        let mut outer_body = 1_i32.to_le_bytes().to_vec();
        outer_body.extend(anonymous_chunk(archive, 2, &entry));
        let userdata = class_userdata_with_payload(
            archive,
            settings::LAYER_EXTENSIONS.to_wire(),
            [0; 16],
            &outer_body,
        );
        metadata_limit_operations(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            [0x44; 16],
            &[0],
            &userdata,
        )
    })
}

macro_rules! metadata_limit_test {
    ($name:ident, $operations:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let operations = $operations();
            assert!(operations.contains(&$operation), "reached {operations:?}");
        }
    };
}

metadata_limit_test!(
    property_previews_refuse_collection_limit,
    metadata_collection_operations,
    "Rhino property previews"
);
metadata_limit_test!(
    property_singletons_refuse_collection_limit,
    metadata_collection_operations,
    "Rhino property singleton keys"
);
metadata_limit_test!(
    unsupported_settings_refuse_collection_limit,
    metadata_collection_operations,
    "Rhino unsupported settings"
);
metadata_limit_test!(
    setting_singletons_refuse_collection_limit,
    metadata_collection_operations,
    "Rhino setting singleton keys"
);
metadata_limit_test!(
    layer_uuid_keys_refuse_collection_limit,
    metadata_collection_operations,
    "Rhino layer UUID keys"
);
metadata_limit_test!(
    metadata_layers_refuse_collection_limit,
    metadata_collection_operations,
    "Rhino metadata layers"
);
metadata_limit_test!(
    layer_index_counts_refuse_collection_limit,
    metadata_collection_operations,
    "Rhino layer index counts"
);
metadata_limit_test!(
    layer_parent_counts_refuse_collection_limit,
    metadata_collection_operations,
    "Rhino layer parent counts"
);
metadata_limit_test!(
    metadata_opaque_records_refuse_collection_limit,
    metadata_opaque_operations,
    "Rhino metadata opaque records"
);
metadata_limit_test!(
    property_singleton_workspace_refuses_materialized_limit,
    metadata_materialized_operations,
    "Rhino property singleton workspace"
);
metadata_limit_test!(
    setting_singleton_workspace_refuses_materialized_limit,
    metadata_materialized_operations,
    "Rhino setting singleton workspace"
);
metadata_limit_test!(
    layer_uuid_workspace_refuses_materialized_limit,
    metadata_materialized_operations,
    "Rhino layer UUID workspace"
);
metadata_limit_test!(
    layer_index_workspace_refuses_materialized_limit,
    metadata_materialized_operations,
    "Rhino layer index workspace"
);
metadata_limit_test!(
    layer_parent_workspace_refuses_materialized_limit,
    metadata_materialized_operations,
    "Rhino layer parent workspace"
);
metadata_limit_test!(
    layer_extension_items_refuse_collection_limit,
    metadata_extension_operations,
    "Rhino layer extension items"
);
metadata_limit_test!(
    layer_userdata_resource_refusal_propagates,
    metadata_userdata_operations,
    "Rhino layer extension entries"
);

/// The layer parent link rests on the stamp, so the loss follows the stamp.
#[test]
fn unstamped_layer_charges_the_parent_link_stamp_loss() {
    // A single zero closes the extension-item chain, so both arms read a whole
    // record and the difference between them is only the stamp.
    let (unstamped_metadata, unstamped) = layer_metadata(&[0], None);
    assert_eq!(unstamped_metadata.layers.len(), 1, "{unstamped:?}");
    assert_eq!(
        unstamped_metadata.layers[0]
            .hierarchy
            .map(|hierarchy| hierarchy.parent_id),
        None
    );
    assert_eq!(
        unstamped_metadata.layers[0]
            .hierarchy
            .map(|hierarchy| hierarchy.expanded),
        None
    );
    assert!(
        unstamped_metadata
            .losses
            .iter()
            .any(|loss| loss.message.contains(LAYER_PARENT_DIALECT)),
        "{:?}",
        unstamped_metadata.losses
    );

    // The stamped arm must read a layer, or its silence proves nothing.
    let (stamped_metadata, stamped) = layer_metadata(&[0], Some(200_912_010));
    assert_eq!(stamped_metadata.layers.len(), 1, "{stamped:?}");
    assert_eq!(
        stamped_metadata.layers[0]
            .hierarchy
            .map(|hierarchy| hierarchy.parent_id),
        Some(Uuid::from_canonical([0x44; 16]))
    );
    assert_eq!(
        stamped_metadata.layers[0]
            .hierarchy
            .map(|hierarchy| hierarchy.expanded),
        Some(true)
    );
    assert!(
        !stamped
            .iter()
            .any(|warning| warning.contains(LAYER_PARENT_DIALECT)),
        "{stamped:?}"
    );
}

#[test]
fn duplicate_layer_indexes_are_preserved_and_reported() {
    let (metadata, warnings) = layer_metadata_with_record_count(&[0], None, 2);

    assert_eq!(metadata.layers.len(), 2, "{warnings:?}");
    assert!(metadata.layers.iter().all(|layer| layer.index == 7));
    assert!(
        warnings.iter().any(|warning| {
            warning.contains(
                "duplicate layer index 7 occurs 2 times; raw indexes preserved and object bindings withheld",
            )
        }),
        "{warnings:?}"
    );
}

#[test]
fn nil_layer_uuid_is_source_absence_and_is_retained_per_record() {
    let (metadata, warnings) = layer_metadata_with_record_count_and_id(&[0], None, 2, [0; 16]);

    assert_eq!(metadata.layers.len(), 2, "{warnings:?}");
    assert!(metadata.layers.iter().all(|layer| layer.id.is_none()));
    assert_eq!(metadata.opaque_records.len(), 2);
    assert!(!warnings
        .iter()
        .any(|warning| warning.contains("duplicate layer UUID")));
}

#[test]
fn duplicate_non_nil_layer_uuids_remain_ambiguous() {
    let id = [0x44; 16];
    let (metadata, warnings) = layer_metadata_with_record_count_and_id(&[0], None, 2, id);

    assert_eq!(metadata.layers.len(), 2, "{warnings:?}");
    assert!(metadata
        .layers
        .iter()
        .all(|layer| layer.id == Some(Uuid::from_canonical(id))));
    assert!(warnings.iter().any(|warning| {
        warning.code == Some(crate::loss::RhinoLossCode::DuplicateRecordResolved)
            && warning.contains("duplicate layer UUID")
    }));
    assert!(metadata.opaque_records.is_empty());
}

#[test]
fn duplicate_layer_parent_uuid_is_reported_as_ambiguous() {
    let (mut metadata, _) = layer_metadata(&[0], Some(200_912_010));
    let parent = Uuid::from_canonical([0x44; 16]);
    metadata.layers[0].id = Some(parent);
    let mut duplicate = metadata.layers[0].clone();
    duplicate.index = 8;
    metadata.layers.push(duplicate);

    let mut warnings = Diagnostics::new();
    super::report_layer_parent_references(
        &cadmpeg_test_support::service_decode_context(),
        &metadata.layers,
        &mut warnings,
    )
    .expect("parent count map fits service profile");

    assert!(
        warnings.iter().any(|warning| {
            warning.code == Some(crate::loss::RhinoLossCode::DuplicateRecordResolved)
                && warning.contains("ambiguous parent UUID")
                && warning.contains("2 layer records")
        }),
        "{warnings:?}"
    );
}

fn layer_metadata_with_description(description: &str) -> settings::DocumentMetadata {
    let mut extension = vec![37];
    extension.extend(utf16_bytes(description));
    extension.push(0);
    layer_metadata_with_extension(&extension)
}

fn layer_text_refusal(extension: &[u8], limit: u64) -> cadmpeg_core::CodecError {
    let (data, tables) = layer_fixture(extension, None, 1, [0; 16], &[]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let ctx = retained_limit_context(&data, &arena, &policy);
    settings::parse_metadata(
        &ctx,
        &data,
        ArchiveVersion::V8,
        &tables,
        &mut Diagnostics::new(),
    )
    .expect_err("layer text exceeds retained limit")
}

#[test]
fn layer_name_refuses_retained_limit() {
    let error = layer_text_refusal(&[0], 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino layer name")
    );
}

#[test]
fn layer_description_refuses_retained_limit() {
    let mut extension = vec![37];
    extension.extend(utf16_bytes(" description "));
    extension.push(0);
    let error = layer_text_refusal(&extension, 1);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino layer description")
    );
    let admitted = layer_metadata_with_description(" description ");
    assert_eq!(
        admitted.layers[0].description.as_deref(),
        Some("description")
    );
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
    let mut warnings = Diagnostics::new();
    let range = settings::parse_rendering_attributes(
        &cadmpeg_test_support::service_decode_context(),
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

fn object_rendering_with_minors(
    outer_minor: i32,
    material_minor: i32,
    mapping_minor: Option<i32>,
    channel_minor: Option<i32>,
) -> Vec<u8> {
    let mut material_body = uuid_bytes();
    material_body.extend(uuid_bytes());
    material_body.extend(0_i32.to_le_bytes());
    let material = anonymous_chunk(ArchiveVersion::V8, material_minor, &material_body);

    let mut body = uuid_bytes();
    let channel_count = i32::from(channel_minor.is_some());
    body.extend(channel_count.to_le_bytes());
    if let Some(channel_minor) = channel_minor {
        let mut channel_body = 7_i32.to_le_bytes().to_vec();
        channel_body.extend(uuid_bytes());
        if channel_minor >= 1 {
            channel_body.extend((0..16).flat_map(|value| (value as f64).to_le_bytes()));
        }
        body.extend(anonymous_chunk(
            ArchiveVersion::V8,
            channel_minor,
            &channel_body,
        ));
    }
    let mapping = mapping_minor.map(|minor| anonymous_chunk(ArchiveVersion::V8, minor, &body));

    let mut rendering_body = 1_i32.to_le_bytes().to_vec();
    rendering_body.extend(material);
    rendering_body.extend(i32::from(mapping.is_some()).to_le_bytes());
    if let Some(mapping) = mapping {
        rendering_body.extend(mapping);
    }
    let mut payload = 1_i32.to_le_bytes().to_vec();
    payload.extend(outer_minor.to_le_bytes());
    payload.extend(rendering_body);
    crc_chunk(ArchiveVersion::V8, 0x4000_8000, &payload)
}

#[test]
fn rendering_attributes_reject_negative_version_minors_at_each_nested_gate() {
    for (label, bytes) in [
        ("outer", object_rendering_with_minors(-1, 0, None, None)),
        ("material", object_rendering_with_minors(1, -1, None, None)),
        (
            "mapping",
            object_rendering_with_minors(1, 0, Some(-1), None),
        ),
        (
            "channel",
            object_rendering_with_minors(1, 0, Some(0), Some(-1)),
        ),
    ] {
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("rendering chunk");
        let mut warnings = Diagnostics::new();
        let result = settings::parse_rendering_attributes(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &mut reader,
            ArchiveVersion::V8,
            settings::RenderingAttributesKind::Object,
            &mut warnings,
        );
        assert!(
            result.is_err(),
            "negative {label} minor was admitted: {result:?}"
        );
    }
}

mod embedded_records;
mod rendering_checksums;

#[test]
fn every_standard_unit_reads_its_scale_from_the_admitted_table() {
    for value in 0..=30 {
        if let Some(unit) = settings::StandardUnit::from_value(value) {
            let scale = settings::MillimeterScale::from(unit);
            assert_eq!(scale.value(), unit.millimeters_per_unit());
            assert_eq!(scale.real().get(), scale.value());
        }
    }
    assert_eq!(settings::MillimeterScale::IDENTITY.value(), 1.0);
}
