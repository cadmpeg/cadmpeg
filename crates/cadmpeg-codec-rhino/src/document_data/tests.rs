use super::{
    annotation_settings, grid_defaults, install, render_settings, render_userdata,
    ANNOTATION_SETTINGS, ANONYMOUS, CLASS_END, CLASS_USERDATA, GRID_DEFAULTS, RENDER_SETTINGS,
    SETTINGS_TABLE,
};
use crate::chunks::ArchiveVersion;
use crate::objects::{ClassUserdata, UserdataDescriptor};
use crate::test_support::test_dump::{
    anonymous_chunk, crc_chunk, long_chunk, metadata_record, push_f64, push_i32, short_chunk,
    utf16_bytes,
};
use crate::wire::Uuid;

fn metadata_scan() -> crate::container::Scan<'static> {
    let archive = ArchiveVersion::V5;
    let bytes = crate::test_support::test_dump::minimal_document(
        "50",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, SETTINGS_TABLE, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    );
    crate::container::scan_owned(bytes).expect("complete metadata fixture")
}

fn metadata_refusal(
    scan: &crate::container::Scan<'_>,
    collection_limit: u64,
    retained_limit: u64,
) -> cadmpeg_core::CodecError {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
        .expect("root bytes admitted");
    install(&ctx, scan, &mut cadmpeg_ir::document::CadIr::empty())
        .expect_err("metadata projection exceeds configured limit")
}

fn assert_metadata_refusal(error: &cadmpeg_core::CodecError, operation: &str) {
    assert!(
        matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == operation
        ),
        "expected {operation}, got {error:?}"
    );
}

fn setting_retained_refusal<T: std::fmt::Debug>(
    bytes: &[u8],
    limit: usize,
    parse: impl FnOnce(
        &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<T, crate::chunks::FramingError>,
) -> crate::chunks::FramingError {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(limit).expect("bounded setting fixture");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("root bytes admitted");
    parse(&ctx).expect_err("setting exceeds retained-byte limit")
}

fn assert_setting_retained_refusal(error: &crate::chunks::FramingError, operation: &str) {
    assert!(
        matches!(error, crate::chunks::FramingError::Resource(refusal) if refusal.operation == operation),
        "expected {operation}, got {error:?}"
    );
}

fn revision() -> crate::settings::RevisionHistory {
    crate::settings::RevisionHistory {
        source: crate::settings::SourceRange { range: 0..0 },
        created_by: "creator".to_string(),
        created: crate::settings::UtcTime { fields: [0; 8] },
        last_edited_by: "editor".to_string(),
        last_edited: crate::settings::UtcTime { fields: [0; 8] },
        revision_count: 1,
    }
}

fn notes() -> crate::settings::Notes {
    crate::settings::Notes {
        source: crate::settings::SourceRange { range: 0..0 },
        html: false,
        text: "note".to_string(),
        visible: true,
        rectangle: [0; 4],
        locked: false,
    }
}

fn application() -> crate::settings::Application {
    crate::settings::Application {
        source: crate::settings::SourceRange { range: 0..0 },
        name: "app".to_string(),
        url: "url".to_string(),
        details: "details".to_string(),
    }
}

fn scan_with_revision() -> crate::container::Scan<'static> {
    let mut scan = metadata_scan();
    scan.metadata.properties.revision_history = Some(revision());
    scan
}

fn scan_with_notes() -> crate::container::Scan<'static> {
    let mut scan = metadata_scan();
    scan.metadata.properties.notes = Some(notes());
    scan
}

fn scan_with_application() -> crate::container::Scan<'static> {
    let mut scan = metadata_scan();
    scan.metadata.properties.application = Some(application());
    scan
}

macro_rules! retained_metadata_test {
    ($name:ident, $fixture:ident, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            let scan = $fixture();
            let limit = u64::try_from($limit).expect("bounded metadata fixture");
            assert_metadata_refusal(&metadata_refusal(&scan, 100, limit), $operation);
        }
    };
}

retained_metadata_test!(
    revision_id_refuses_retained_limit,
    scan_with_revision,
    "rhino:document:revision#current".len() - 1,
    "Rhino revision ID"
);
retained_metadata_test!(
    revision_creator_refuses_retained_limit,
    scan_with_revision,
    "rhino:document:revision#current".len() + "creator".len() - 1,
    "Rhino revision creator"
);
retained_metadata_test!(
    revision_editor_refuses_retained_limit,
    scan_with_revision,
    "rhino:document:revision#current".len() + "creator".len() + "editor".len() - 1,
    "Rhino revision editor"
);
retained_metadata_test!(
    notes_id_refuses_retained_limit,
    scan_with_notes,
    "rhino:document:notes#current".len() - 1,
    "Rhino notes ID"
);
retained_metadata_test!(
    notes_text_refuses_retained_limit,
    scan_with_notes,
    "rhino:document:notes#current".len() + "note".len() - 1,
    "Rhino notes text"
);
retained_metadata_test!(
    application_id_refuses_retained_limit,
    scan_with_application,
    "rhino:document:application#writer".len() - 1,
    "Rhino application ID"
);
retained_metadata_test!(
    application_name_refuses_retained_limit,
    scan_with_application,
    "rhino:document:application#writer".len() + "app".len() - 1,
    "Rhino application name"
);
retained_metadata_test!(
    application_url_refuses_retained_limit,
    scan_with_application,
    "rhino:document:application#writer".len() + "app".len() + "url".len() - 1,
    "Rhino application URL"
);
retained_metadata_test!(
    application_details_refuse_retained_limit,
    scan_with_application,
    "rhino:document:application#writer".len() + "app".len() + "url".len() + "details".len() - 1,
    "Rhino application details"
);

fn retained_setting_scan() -> crate::container::Scan<'static> {
    let mut scan = metadata_scan();
    let record = crate::container::Record::long(ANNOTATION_SETTINGS, 0..0, 0..0);
    scan.tables.push(
        crate::container::Table::new(
            SETTINGS_TABLE,
            0..1,
            0..0,
            vec![record],
            1,
            std::collections::BTreeMap::new(),
        )
        .expect("table framing"),
    );
    scan
}

fn projected_setting_scan(typecode: u32, body: &[u8]) -> crate::container::Scan<'static> {
    let archive = ArchiveVersion::V5;
    let record = crc_chunk(archive, typecode, body);
    let bytes = crate::test_support::test_dump::minimal_document(
        "50",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, SETTINGS_TABLE, &[record]),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    );
    let mut scan = crate::container::scan_owned(bytes).expect("complete setting fixture");
    crate::test_support::test_dump::set_test_units(&mut scan, 1.0);
    scan.metadata.settings.unsupported.clear();
    scan
}

#[test]
fn annotation_settings_refuse_collection_limit() {
    let scan = projected_setting_scan(ANNOTATION_SETTINGS, &annotation_body(0));
    assert_metadata_refusal(
        &metadata_refusal(&scan, 0, u64::MAX),
        "Rhino annotation settings",
    );
}

#[test]
fn grid_defaults_refuse_collection_limit() {
    let scan = projected_setting_scan(GRID_DEFAULTS, &grid_body());
    assert_metadata_refusal(&metadata_refusal(&scan, 0, u64::MAX), "Rhino grid defaults");
}

#[test]
fn render_settings_refuse_collection_limit() {
    let body = crc_chunk(ArchiveVersion::V5, ANONYMOUS, &modern_body(0));
    let scan = projected_setting_scan(RENDER_SETTINGS, &body);
    assert_metadata_refusal(
        &metadata_refusal(&scan, 0, u64::MAX),
        "Rhino render settings",
    );
}

#[test]
fn document_revisions_refuse_collection_limit() {
    let mut scan = metadata_scan();
    scan.metadata.properties.revision_history = Some(revision());
    assert_metadata_refusal(
        &metadata_refusal(&scan, 0, u64::MAX),
        "Rhino document revisions",
    );
}

#[test]
fn document_notes_refuse_collection_limit() {
    let mut scan = metadata_scan();
    scan.metadata.properties.notes = Some(notes());
    assert_metadata_refusal(
        &metadata_refusal(&scan, 0, u64::MAX),
        "Rhino document notes",
    );
}

#[test]
fn document_applications_refuse_collection_limit() {
    let mut scan = metadata_scan();
    scan.metadata.properties.application = Some(application());
    assert_metadata_refusal(
        &metadata_refusal(&scan, 0, u64::MAX),
        "Rhino document applications",
    );
}

#[test]
fn document_previews_refuse_collection_limit() {
    let mut scan = metadata_scan();
    scan.metadata
        .properties
        .previews
        .push(crate::settings::PreviewDescriptor {
            source: crate::settings::SourceRange { range: 0..0 },
            compressed: false,
        });
    assert_metadata_refusal(
        &metadata_refusal(&scan, 0, u64::MAX),
        "Rhino document previews",
    );
}

#[test]
fn unsupported_setting_records_refuse_collection_limit() {
    let mut scan = metadata_scan();
    scan.metadata
        .settings
        .unsupported
        .push(crate::settings::SettingDescriptor {
            typecode: 0x2000_803f,
            source: crate::settings::SourceRange { range: 0..0 },
        });
    assert_metadata_refusal(
        &metadata_refusal(&scan, 0, u64::MAX),
        "Rhino unsupported setting records",
    );
}

#[test]
fn document_setting_losses_refuse_collection_limit() {
    let scan = retained_setting_scan();
    assert_metadata_refusal(
        &metadata_refusal(&scan, 0, u64::MAX),
        "Rhino document setting losses",
    );
}

#[test]
fn opaque_setting_records_refuse_collection_limit() {
    let scan = retained_setting_scan();
    assert_metadata_refusal(
        &metadata_refusal(&scan, 1, u64::MAX),
        "Rhino opaque setting records",
    );
}

#[test]
fn retained_setting_records_refuse_collection_limit() {
    let scan = retained_setting_scan();
    assert_metadata_refusal(
        &metadata_refusal(&scan, 2, u64::MAX),
        "Rhino retained setting records",
    );
}

#[test]
fn document_metadata_projection_succeeds_under_service_profile() {
    let mut scan = metadata_scan();
    scan.metadata.properties.revision_history = Some(revision());
    scan.metadata.properties.notes = Some(notes());
    scan.metadata.properties.application = Some(application());
    scan.metadata
        .properties
        .previews
        .push(crate::settings::PreviewDescriptor {
            source: crate::settings::SourceRange { range: 0..0 },
            compressed: false,
        });
    scan.metadata
        .settings
        .unsupported
        .push(crate::settings::SettingDescriptor {
            typecode: 0x2000_803f,
            source: crate::settings::SourceRange { range: 0..0 },
        });
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        scan.data,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("root bytes admitted");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    install(&ctx, &scan, &mut ir).expect("service profile admits metadata projection");
    let rhino = ir
        .native
        .namespace("rhino")
        .expect("native metadata namespace");
    for arena in [
        "revisions",
        "document_notes",
        "applications",
        "previews",
        "setting_records",
    ] {
        assert_eq!(rhino.arenas()[arena].len(), 1, "{arena}");
    }
}

fn push_color(bytes: &mut Vec<u8>, value: [u8; 4]) {
    bytes.extend(value);
}

fn legacy_body(version: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_i32(&mut bytes, version);
    push_i32(&mut bytes, 1);
    push_i32(&mut bytes, 1234);
    push_i32(&mut bytes, 567);
    push_color(&mut bytes, [1, 2, 3, 4]);
    push_i32(&mut bytes, 2);
    push_color(&mut bytes, [5, 6, 7, 8]);
    bytes.extend(utf16_bytes("background.png"));
    for value in [1, 0, 1, 0, 1, 0, 1, 0, 1] {
        push_i32(&mut bytes, value);
    }
    for value in [3, 2, 2048, 1024] {
        push_i32(&mut bytes, value);
    }
    push_f64(&mut bytes, 1.25);
    if version >= 101 {
        push_f64(&mut bytes, 144.5);
        push_i32(&mut bytes, 2);
    }
    if version >= 102 {
        push_color(&mut bytes, [9, 10, 11, 12]);
    }
    if version >= 103 {
        bytes.push(1);
    }
    bytes.extend([0xaa, 0xbb]);
    bytes
}

fn modern_body(minor: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_i32(&mut bytes, 1);
    push_i32(&mut bytes, minor);
    bytes.push(1);
    push_i32(&mut bytes, 1234);
    push_i32(&mut bytes, 567);
    push_f64(&mut bytes, 144.5);
    push_i32(&mut bytes, 2);
    push_color(&mut bytes, [1, 2, 3, 4]);
    push_i32(&mut bytes, 2);
    push_color(&mut bytes, [5, 6, 7, 8]);
    push_color(&mut bytes, [9, 10, 11, 12]);
    bytes.extend(utf16_bytes("background.png"));
    bytes.extend([1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0]);
    for value in [3, 2, 2048, 1024] {
        push_i32(&mut bytes, value);
    }
    push_f64(&mut bytes, 1.25);
    if minor >= 1 {
        push_i32(&mut bytes, 2);
        push_f64(&mut bytes, 100.0);
        push_f64(&mut bytes, 64.0);
        push_f64(&mut bytes, 0.1);
        push_i32(&mut bytes, 10);
    }
    if minor >= 2 {
        push_i32(&mut bytes, 2);
        bytes.extend(utf16_bytes("specific-viewport"));
        bytes.extend(utf16_bytes("named-view"));
        bytes.extend(utf16_bytes("snapshot"));
    }
    if minor >= 3 {
        bytes.push(1);
    }
    bytes.extend([0xaa, 0xbb]);
    bytes
}

fn annotation_body(minor: u8) -> Vec<u8> {
    let mut bytes = vec![0x10 | minor];
    for value in [1.0, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5] {
        push_f64(&mut bytes, value);
    }
    push_i32(&mut bytes, 2);
    for value in [4, 1, 2, 3, 2, 6] {
        push_i32(&mut bytes, value);
    }
    bytes.extend(utf16_bytes("WitnessFace"));
    if minor >= 1 {
        push_f64(&mut bytes, 1.25);
        bytes.push(0);
    }
    if minor >= 2 {
        push_f64(&mut bytes, 2.5);
        bytes.push(0);
    }
    if minor >= 3 {
        bytes.extend([1, 0]);
    }
    if minor >= 4 {
        bytes.push(1);
        bytes.extend([0; 16]);
    }
    bytes.extend([0xde, 0xad]);
    bytes
}

fn grid_body() -> Vec<u8> {
    let mut bytes = vec![0x1f];
    push_f64(&mut bytes, 2.5);
    push_f64(&mut bytes, 0.75);
    for value in [42, 3, 0, 1, 0] {
        push_i32(&mut bytes, value);
    }
    bytes.extend([0xde, 0xad]);
    bytes
}

fn annotation_retained_refusal(
    minor: u8,
    limit: usize,
    dimension_id: bool,
) -> crate::chunks::FramingError {
    let mut bytes = annotation_body(minor);
    if dimension_id {
        let uuid_start = bytes.len() - 18;
        bytes[uuid_start] = 1;
    }
    setting_retained_refusal(&bytes, limit, |ctx| {
        annotation_settings(
            ctx,
            &bytes,
            0..bytes.len(),
            0,
            crate::settings::MillimeterScale::IDENTITY,
        )
    })
}

fn render_retained_refusal(modern: bool, limit: usize) -> crate::chunks::FramingError {
    let (bytes, archive) = if modern {
        (
            crc_chunk(ArchiveVersion::V8, ANONYMOUS, &modern_body(2)),
            ArchiveVersion::V8,
        )
    } else {
        (legacy_body(100), ArchiveVersion::V5)
    };
    setting_retained_refusal(&bytes, limit, |ctx| {
        render_settings(
            ctx,
            &bytes,
            0..bytes.len(),
            0,
            archive,
            crate::settings::MillimeterScale::IDENTITY,
        )
    })
}

#[test]
fn annotation_settings_id_refuses_retained_limit() {
    assert_setting_retained_refusal(
        &annotation_retained_refusal(
            0,
            "rhino:document:annotation_settings#current".len() - 1,
            false,
        ),
        "Rhino annotation settings ID",
    );
}

#[test]
fn annotation_font_face_refuses_retained_limit() {
    assert_setting_retained_refusal(
        &annotation_retained_refusal(
            0,
            "rhino:document:annotation_settings#current".len() + "WitnessFace".len() - 1,
            false,
        ),
        "Rhino annotation font face",
    );
}

#[test]
fn annotation_dimension_layer_uuid_refuses_retained_limit() {
    assert_setting_retained_refusal(
        &annotation_retained_refusal(
            4,
            "rhino:document:annotation_settings#current".len() + "WitnessFace".len() + 36 - 1,
            true,
        ),
        "Rhino annotation dimension layer UUID",
    );
}

#[test]
fn grid_defaults_id_refuses_retained_limit() {
    let bytes = grid_body();
    let error = setting_retained_refusal(
        &bytes,
        "rhino:document:grid_defaults#current".len() - 1,
        |ctx| {
            grid_defaults(
                ctx,
                &bytes,
                0..bytes.len(),
                0,
                crate::settings::MillimeterScale::IDENTITY,
            )
        },
    );
    assert_setting_retained_refusal(&error, "Rhino grid defaults ID");
}

#[test]
fn render_background_bitmap_path_refuses_retained_limit() {
    assert_setting_retained_refusal(
        &render_retained_refusal(false, "background.png".len() - 1),
        "Rhino render background bitmap path",
    );
}

#[test]
fn render_settings_id_refuses_retained_limit() {
    assert_setting_retained_refusal(
        &render_retained_refusal(
            false,
            "background.png".len() + "rhino:document:render_settings#current".len() - 1,
        ),
        "Rhino render settings ID",
    );
}

#[test]
fn render_specific_viewport_refuses_retained_limit() {
    assert_setting_retained_refusal(
        &render_retained_refusal(true, "background.png".len() + "specific-viewport".len() - 1),
        "Rhino render specific viewport",
    );
}

#[test]
fn render_named_view_refuses_retained_limit() {
    assert_setting_retained_refusal(
        &render_retained_refusal(
            true,
            "background.png".len() + "specific-viewport".len() + "named-view".len() - 1,
        ),
        "Rhino render named view",
    );
}

#[test]
fn render_snapshot_refuses_retained_limit() {
    assert_setting_retained_refusal(
        &render_retained_refusal(
            true,
            "background.png".len()
                + "specific-viewport".len()
                + "named-view".len()
                + "snapshot".len()
                - 1,
        ),
        "Rhino render snapshot",
    );
}

#[test]
fn legacy_render_settings_gate_each_v5_suffix() {
    let value_100 = render_settings(
        &cadmpeg_test_support::service_decode_context(),
        &legacy_body(100),
        0..legacy_body(100).len(),
        7,
        ArchiveVersion::V5,
        crate::settings::MillimeterScale::IDENTITY,
    )
    .expect("legacy version 100 settings");
    assert_eq!(value_100.image_dpi, None);
    assert_eq!(value_100.image_unit_system, None);
    assert_eq!(value_100.background_bottom_color, None);
    assert!(!value_100.scale_background_to_fit);

    let value_101 = legacy_body(101);
    let value_101 = render_settings(
        &cadmpeg_test_support::service_decode_context(),
        &value_101,
        0..value_101.len(),
        7,
        ArchiveVersion::V5,
        crate::settings::MillimeterScale::IDENTITY,
    )
    .expect("legacy version 101 settings");
    assert_eq!(value_101.image_dpi, Some(144.5));
    assert_eq!(value_101.image_unit_system, Some(2));
    assert_eq!(value_101.background_bottom_color, None);

    let value_102 = legacy_body(102);
    let value_102 = render_settings(
        &cadmpeg_test_support::service_decode_context(),
        &value_102,
        0..value_102.len(),
        7,
        ArchiveVersion::V5,
        crate::settings::MillimeterScale::IDENTITY,
    )
    .expect("legacy version 102 settings");
    assert_eq!(value_102.background_bottom_color, Some([9, 10, 11, 12]));
    assert!(!value_102.scale_background_to_fit);

    let value_103 = legacy_body(103);
    let value_103 = render_settings(
        &cadmpeg_test_support::service_decode_context(),
        &value_103,
        0..value_103.len(),
        7,
        ArchiveVersion::V5,
        crate::settings::MillimeterScale::IDENTITY,
    )
    .expect("legacy version 103 settings");
    assert!(value_103.scale_background_to_fit);
    assert_eq!(value_103.shadowmap_size_pixels, [2048, 1024]);
}

#[test]
fn annotation_settings_gate_packed_minor_fields_and_skip_suffix() {
    for minor in 0..=5 {
        let bytes = annotation_body(minor);
        let value = annotation_settings(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            0..bytes.len(),
            19,
            crate::test_support::millimeter_scale(2.0),
        )
        .expect("annotation settings packed version");

        assert_eq!(value.source_offset, 19);
        assert_eq!(value.dimension_scale, crate::test_support::finite(1.0));
        assert_eq!(value.text_height_mm, crate::test_support::finite(5.0));
        assert_eq!(
            value.extension_line_extension_mm,
            crate::test_support::finite(7.0)
        );
        assert_eq!(
            value.extension_line_offset_mm,
            crate::test_support::finite(9.0)
        );
        assert_eq!(value.arrow_length_mm, crate::test_support::finite(11.0));
        assert_eq!(value.arrow_width_mm, crate::test_support::finite(13.0));
        assert_eq!(value.center_mark_mm, crate::test_support::finite(15.0));
        assert_eq!(value.dimension_units, 2);
        assert_eq!(value.font_face, "WitnessFace");
        assert_eq!(
            value.world_view_text_scale,
            (minor >= 1).then(|| crate::test_support::finite(1.25))
        );
        assert_eq!(value.annotation_scaling, (minor >= 1).then_some(false));
        assert_eq!(
            value.world_view_hatch_scale,
            (minor >= 2).then(|| crate::test_support::finite(2.5))
        );
        assert_eq!(value.hatch_scaling, (minor >= 2).then_some(false));
        assert_eq!(
            value.model_space_annotation_scaling,
            (minor >= 3).then_some(true)
        );
        assert_eq!(
            value.layout_space_annotation_scaling,
            (minor >= 3).then_some(false)
        );
        assert_eq!(value.use_dimension_layer, (minor >= 4).then_some(true));
        assert_eq!(value.dimension_layer_uuid, None);
    }

    let mut bytes = annotation_body(4);
    let uuid_offset = bytes.len() - 2 - 16;
    bytes[uuid_offset..uuid_offset + 16].copy_from_slice(&[
        0x40, 0x30, 0x20, 0x10, 0x60, 0x50, 0x80, 0x70, 0x90, 0xa0, 0xb0, 0xc0, 0xd0, 0xe0, 0xf0,
        0x01,
    ]);
    let value = annotation_settings(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0..bytes.len(),
        19,
        crate::settings::MillimeterScale::IDENTITY,
    )
    .expect("annotation settings dimension-layer UUID");
    assert_eq!(
        value.dimension_layer_uuid.as_deref(),
        Some("10203040-5060-7080-90a0-b0c0d0e0f001")
    );
}

#[test]
fn annotation_settings_refuse_nonfinite_scales() {
    for (minor, original) in [(0, 1.0_f64), (1, 1.25), (2, 2.5)] {
        let mut bytes = annotation_body(minor);
        let offset = bytes
            .windows(8)
            .position(|window| window == original.to_le_bytes())
            .expect("fixture contains the selected annotation scale");
        bytes[offset..offset + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(annotation_settings(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            0..bytes.len(),
            19,
            crate::settings::MillimeterScale::IDENTITY,
        )
        .is_err());
    }
}

#[test]
fn grid_defaults_accept_future_minor_and_scale_lengths() {
    let bytes = grid_body();
    let value = grid_defaults(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0..bytes.len(),
        23,
        crate::test_support::millimeter_scale(2.0),
    )
    .expect("grid defaults");

    assert_eq!(value.source_offset, 23);
    assert_eq!(value.grid_spacing_mm, crate::test_support::finite(5.0));
    assert_eq!(value.snap_spacing_mm, crate::test_support::finite(1.5));
    assert_eq!(value.grid_line_count, 42);
    assert_eq!(value.thick_line_frequency, 3);
    assert!(!value.show_grid && value.show_grid_axes && !value.show_world_axes);
}

#[test]
fn modern_render_settings_consumes_known_prefix_and_future_suffix() {
    let body = modern_body(4);
    let bytes = crc_chunk(ArchiveVersion::V8, ANONYMOUS, &body);
    let value = render_settings(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0..bytes.len(),
        11,
        ArchiveVersion::V8,
        crate::settings::MillimeterScale::IDENTITY,
    )
    .expect("modern future-minor settings");

    assert_eq!(value.source_offset, 11);
    assert_eq!(value.image_width_pixels, 1234);
    assert_eq!(value.image_height_pixels, 567);
    assert_eq!(value.image_dpi, Some(144.5));
    assert_eq!(value.image_unit_system, Some(2));
    assert_eq!(value.background_bitmap_path, "background.png");
    assert_eq!(
        value.obsolete_focal_blur,
        Some([2.0, 100.0, 64.0, 0.1, 10.0])
    );
    assert_eq!(value.rendering_source, Some(2));
    assert_eq!(value.specific_viewport, "specific-viewport");
    assert_eq!(value.named_view, "named-view");
    assert_eq!(value.snapshot, "snapshot");
    assert_eq!(value.force_viewport_aspect_ratio, Some(true));
    assert!(value.use_hidden_lights && value.flat_shade);
    assert!(!value.depth_cue && !value.transparent_background);
}

#[test]
fn modern_render_settings_rejects_negative_minor() {
    let body = modern_body(-1);
    let bytes = crc_chunk(ArchiveVersion::V8, ANONYMOUS, &body);
    let error = render_settings(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0..bytes.len(),
        0,
        ArchiveVersion::V8,
        crate::settings::MillimeterScale::IDENTITY,
    )
    .expect_err("negative modern minor");
    assert!(error
        .to_string()
        .contains("render-settings version is unsupported"));
}

#[test]
fn render_userdata_uses_shared_header_grammar_and_outer_suffix_boundaries() {
    let archive = ArchiveVersion::V8;
    let class_uuid = Uuid::from_wire((1_u8..=16).collect::<Vec<_>>().try_into().expect("UUID"));
    let item_uuid = Uuid::from_wire((17_u8..=32).collect::<Vec<_>>().try_into().expect("UUID"));
    let application_uuid =
        Uuid::from_wire((33_u8..=48).collect::<Vec<_>>().try_into().expect("UUID"));

    let mut header_body = class_uuid.to_wire().to_vec();
    header_body.extend(item_uuid.to_wire());
    header_body.extend(1_i32.to_le_bytes());
    for value in [1.0_f64; 16] {
        header_body.extend(value.to_le_bytes());
    }
    header_body.extend(application_uuid.to_wire());
    header_body.push(0);
    header_body.extend(60_i32.to_le_bytes());
    header_body.extend(202_400_i32.to_le_bytes());
    header_body.extend([0xde, 0xad]);
    let header = crc_chunk(archive, 0x0002_fff9, &header_body);
    let payload = anonymous_chunk(archive, 4, &[0x51, 0x52, 0xbe, 0xef]);
    let mut major_two_body = vec![0x2f];
    major_two_body.extend(header);
    major_two_body.extend(payload);
    major_two_body.extend([0xca, 0xfe]);
    let major_two = long_chunk(archive, CLASS_USERDATA, &major_two_body);

    let mut major_one_body = vec![0x10];
    major_one_body.extend(class_uuid.to_wire());
    major_one_body.extend(item_uuid.to_wire());
    major_one_body.extend(2_i32.to_le_bytes());
    major_one_body.extend([0_u8; 16 * 8]);
    major_one_body.extend(anonymous_chunk(archive, 0, &[0x61, 0x62]));
    let major_one = long_chunk(archive, CLASS_USERDATA, &major_one_body);

    let mut body = major_two;
    body.extend(long_chunk(archive, 0x4000_1234, &[0xaa, 0xbb]));
    body.extend(major_one);
    body.extend(short_chunk(archive, CLASS_END, 0));
    body.extend([0xfa, 0xce]);
    let (data, record) = metadata_record(0x2000_8136, body);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)
        .expect("root bytes admitted");
    let descriptor = render_userdata(&ctx, &data, &record, archive).expect("render userdata");

    assert_eq!(descriptor.source, record.range);
    assert_eq!(descriptor.items.len(), 2);
    assert_eq!(descriptor.unknown_chunks.len(), 1);
    assert_eq!(descriptor.suffix, data.len() - 2..data.len());
    let modern = &descriptor.items[0];
    let UserdataDescriptor::Known(ClassUserdata {
        version,
        class_uuid: modern_class,
        item_uuid: modern_item,
        copy_count,
        application_uuid: modern_application,
        save_context,
        payload_range,
        ..
    }) = modern
    else {
        panic!("expected known userdata");
    };
    assert_eq!(*version, (2, 15));
    assert_eq!(*modern_class, class_uuid);
    assert_eq!(*modern_item, item_uuid);
    assert_eq!(*copy_count, 1);
    assert_eq!(*modern_application, Some(application_uuid));
    assert_eq!(
        save_context.map(|value| value.last_saved_as_goo),
        Some(false)
    );
    assert_eq!(save_context.map(|value| value.archive_version), Some(60));
    assert_eq!(
        save_context.map(|value| value.writer_version),
        Some(202_400)
    );
    assert!(!payload_range.is_empty());
    let legacy = &descriptor.items[1];
    let UserdataDescriptor::Known(ClassUserdata {
        version,
        copy_count,
        application_uuid,
        save_context,
        ..
    }) = legacy
    else {
        panic!("expected known userdata");
    };
    assert_eq!(*version, (1, 0));
    assert_eq!(*copy_count, 2);
    assert_eq!(*application_uuid, None);
    assert_eq!(save_context.map(|value| value.last_saved_as_goo), None);
    assert_eq!(save_context.map(|value| value.archive_version), None);
    assert_eq!(save_context.map(|value| value.writer_version), None);
}

#[test]
fn render_userdata_items_refuse_collection_limit() {
    let archive = ArchiveVersion::V8;
    let mut item = vec![0x10];
    item.extend([0_u8; 16]);
    item.extend([1_u8; 16]);
    item.extend(0_i32.to_le_bytes());
    item.extend([0_u8; 16 * 8]);
    item.extend(anonymous_chunk(archive, 0, &[0x61]));
    let mut body = long_chunk(archive, CLASS_USERDATA, &item);
    body.extend(short_chunk(archive, CLASS_END, 0));
    let (data, record) = metadata_record(0x2000_8136, body);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)
        .expect("root bytes admitted");
    let error = render_userdata(&ctx, &data, &record, archive)
        .expect_err("render userdata item exceeds collection limit");
    assert!(matches!(
        error,
        crate::chunks::FramingError::Resource(refusal)
            if refusal.operation == "Rhino render userdata items"
    ));
}

#[test]
fn render_userdata_unknown_chunks_refuse_collection_limit() {
    let archive = ArchiveVersion::V8;
    let mut body = long_chunk(archive, 0x4000_1234, &[0xaa, 0xbb]);
    body.extend(short_chunk(archive, CLASS_END, 0));
    let (data, record) = metadata_record(0x2000_8136, body);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)
        .expect("root bytes admitted");
    let error = render_userdata(&ctx, &data, &record, archive)
        .expect_err("unknown render userdata chunk exceeds collection limit");
    assert!(matches!(
        error,
        crate::chunks::FramingError::Resource(refusal)
            if refusal.operation == "Rhino render userdata unknown chunks"
    ));
}
