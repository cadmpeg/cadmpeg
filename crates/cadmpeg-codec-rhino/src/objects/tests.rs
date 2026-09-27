// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::disallowed_methods)]

use cadmpeg_test_support::wire;

use crate::loss::Diagnostics;
use cadmpeg_ir::report::Severity;

use crate::chunks::{ArchiveVersion, BoundedReader};
use crate::objects::{AttributeState, ObjectRecord, IDEF_OBJECT_MODE};
use crate::settings;
use crate::test_support::test_dump::{
    anonymous_chunk, class_userdata_v1_with_direct_payload, class_userdata_v2_with_direct_payload,
    crc_chunk, descriptor, fixed_attributes, long_chunk, mesh_parameters, minimal_document,
    object_record, object_record_with_attribute_userdata, object_record_with_payload,
    object_record_with_unknown_trailer, object_record_without_end, point_payload, set_test_units,
    short_chunk, table, tagged_attributes, utf16_bytes, uuid_bytes, POINT_CLASS,
};
use crate::wire::Uuid;

fn parse_attributes(
    bytes: &[u8],
    body_range: std::ops::Range<usize>,
    source_range: std::ops::Range<usize>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    warnings: &mut Diagnostics,
) -> Result<crate::objects::ObjectAttributes, crate::chunks::FramingError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test attributes fit the service profile");
    crate::objects::parse_attributes(
        &ctx,
        bytes,
        body_range,
        source_range,
        archive,
        writer_version,
        warnings,
    )
}

fn attribute_resource_refusal(
    bytes: &[u8],
    collection_limit: u64,
    retained_limit: u64,
) -> crate::chunks::FramingError {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("root bytes admitted");
    crate::objects::parse_attributes(
        &ctx,
        bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V5,
        None,
        &mut Diagnostics::new(),
    )
    .expect_err("attribute value exceeds configured resource limit")
}

fn assert_attribute_resource(error: &crate::chunks::FramingError, operation: &str) {
    assert!(
        matches!(error, crate::chunks::FramingError::Resource(refusal) if refusal.operation == operation),
        "expected resource operation {operation}"
    );
}

fn attribute_userdata_refusal_at(bytes: &[u8], limit: u64) -> crate::chunks::FramingError {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("root bytes admitted");
    crate::objects::parse_attribute_userdata(
        &ctx,
        bytes,
        0..bytes.len(),
        ArchiveVersion::V4,
        &mut Diagnostics::new(),
    )
    .expect_err("attribute userdata descriptor exceeds collection limit")
}

fn attribute_userdata_refusal(bytes: &[u8]) -> crate::chunks::FramingError {
    attribute_userdata_refusal_at(bytes, 0)
}

#[test]
fn unknown_attribute_userdata_refuses_collection_limit() {
    let bytes = long_chunk(ArchiveVersion::V4, 0x0002_0001, &[]);
    assert_attribute_resource(&attribute_userdata_refusal(&bytes), "Rhino diagnostics");
    assert_attribute_resource(
        &attribute_userdata_refusal_at(&bytes, 1),
        "Rhino attribute userdata descriptors",
    );
}

#[test]
fn known_attribute_userdata_refuses_collection_limit() {
    let bytes = class_userdata_v1_with_direct_payload(ArchiveVersion::V4, [1; 16], &[]);
    assert_attribute_resource(
        &attribute_userdata_refusal(&bytes),
        "Rhino attribute userdata descriptors",
    );
}

#[test]
fn future_attribute_userdata_refuses_collection_limit() {
    let bytes = long_chunk(ArchiveVersion::V4, 0x0002_7ffd, &[0x30]);
    assert_attribute_resource(
        &attribute_userdata_refusal(&bytes),
        "Rhino attribute userdata descriptors",
    );
}

#[test]
fn fixed_object_name_and_url_refuse_retained_limit() {
    let bytes = fixed_attributes(0, 0, None);
    assert_attribute_resource(
        &attribute_resource_refusal(&bytes, 100, 3),
        "Rhino object name",
    );
    assert_attribute_resource(
        &attribute_resource_refusal(&bytes, 100, 4),
        "Rhino object URL",
    );
}

#[test]
fn tagged_object_name_and_url_refuse_retained_limit() {
    let bytes = tagged_attributes(&[(1, utf16_bytes("name"))], 0);
    assert_attribute_resource(
        &attribute_resource_refusal(&bytes, 100, 3),
        "Rhino object name",
    );
    let bytes = tagged_attributes(&[(2, utf16_bytes("url"))], 0);
    assert_attribute_resource(
        &attribute_resource_refusal(&bytes, 100, 2),
        "Rhino object URL",
    );
}

#[test]
fn fixed_object_groups_refuse_collection_limit() {
    let mut bytes = fixed_attributes(1, 0, None);
    let count_offset = fixed_attributes(0, 0, None).len();
    bytes.splice(
        count_offset..count_offset + 4,
        [1_i32.to_le_bytes(), 7_i32.to_le_bytes()].concat(),
    );
    assert_attribute_resource(
        &attribute_resource_refusal(&bytes, 0, 100),
        "Rhino object groups",
    );
}

#[test]
fn fixed_object_display_materials_refuse_collection_limit() {
    let mut bytes = fixed_attributes(3, 0, None);
    let count_offset = fixed_attributes(2, 0, None).len();
    let mut payload = 1_i32.to_le_bytes().to_vec();
    payload.extend([1; 32]);
    bytes.splice(count_offset..count_offset + 4, payload);
    assert_attribute_resource(
        &attribute_resource_refusal(&bytes, 0, 100),
        "Rhino object display materials",
    );
}

#[test]
fn fixed_object_explicit_display_materials_refuse_collection_limit() {
    let mut bytes = fixed_attributes(6, 0, None);
    let count_offset = fixed_attributes(5, 0, None).len() + 1;
    let mut payload = 1_i32.to_le_bytes().to_vec();
    payload.extend([1; 32]);
    bytes.splice(count_offset..count_offset + 4, payload);
    assert_attribute_resource(
        &attribute_resource_refusal(&bytes, 0, 100),
        "Rhino object explicit display materials",
    );
}

#[test]
fn tagged_object_groups_and_display_materials_refuse_collection_limit() {
    let mut groups = 1_i32.to_le_bytes().to_vec();
    groups.extend(7_i32.to_le_bytes());
    let bytes = tagged_attributes(&[(18, groups)], 0);
    assert_attribute_resource(
        &attribute_resource_refusal(&bytes, 0, 100),
        "Rhino object groups",
    );

    let mut materials = 1_i32.to_le_bytes().to_vec();
    materials.extend([1; 32]);
    let bytes = tagged_attributes(&[(21, materials)], 0);
    assert_attribute_resource(
        &attribute_resource_refusal(&bytes, 0, 100),
        "Rhino object display materials",
    );
}

#[test]
fn parses_fixed_attributes_through_every_minor_gate() {
    for minor in 0..=8 {
        let bytes = fixed_attributes(minor, 0, Some(true));
        let parsed = parse_attributes(
            &bytes,
            0..bytes.len(),
            100..100 + bytes.len(),
            ArchiveVersion::V4,
            None,
            &mut Diagnostics::new(),
        )
        .unwrap_or_else(|error| panic!("minor {minor}: {error}"));
        assert_eq!(parsed.version, (1, minor));
        assert_eq!(parsed.source.range, 100..100 + bytes.len());
        assert_eq!(parsed.name, "name");
        assert_eq!(parsed.url, "https://example.test");
        assert_eq!(parsed.plot_color_source, 0);
        assert!(parsed.groups.is_empty());
        assert_eq!(parsed.linetype_index, if minor >= 5 { 4 } else { -1 });
        assert_eq!(parsed.rendering_range.is_some(), minor >= 7);
    }
}

#[test]
fn fixed_visibility_and_definition_membership_use_mode_low_nibble() {
    let hidden = fixed_attributes(1, 0x11, None);
    let hidden = parse_attributes(
        &hidden,
        0..hidden.len(),
        0..hidden.len(),
        ArchiveVersion::V4,
        None,
        &mut Diagnostics::new(),
    )
    .expect("required invariant");
    assert!(!hidden.visible);

    let locked = fixed_attributes(1, 0x12, None);
    let locked = parse_attributes(
        &locked,
        0..locked.len(),
        0..locked.len(),
        ArchiveVersion::V4,
        None,
        &mut Diagnostics::new(),
    )
    .expect("required invariant");
    assert!(locked.visible);

    let definition = fixed_attributes(1, 0xf3, None);
    let definition = parse_attributes(
        &definition,
        0..definition.len(),
        0..definition.len(),
        ArchiveVersion::V4,
        None,
        &mut Diagnostics::new(),
    )
    .expect("required invariant");
    assert_eq!(definition.object_mode & 0x0f, 3);
}

#[test]
fn fixed_explicit_visibility_overrides_hidden_mode_default() {
    let bytes = fixed_attributes(2, 0x02, Some(true));
    let parsed = parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V4,
        None,
        &mut Diagnostics::new(),
    )
    .expect("required invariant");
    assert!(parsed.visible);
}

#[test]
fn legacy_v5_fixed_attributes_follow_writer_cutoff() {
    let bytes = fixed_attributes(1, 0, Some(true));
    let parsed = parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V5,
        Some(200_712_189),
        &mut Diagnostics::new(),
    )
    .expect("pre-cutoff V5 writer uses the fixed attributes reader");
    assert_eq!(parsed.version, (1, 1));
    assert_eq!(parsed.name, "name");

    let error = parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V5,
        Some(200_712_190),
        &mut Diagnostics::new(),
    )
    .expect_err("the cutoff selects the tagged V5 reader");
    assert!(matches!(
        error,
        crate::chunks::FramingError::Structural { .. }
    ));
}

#[test]
fn object_attribute_booleans_use_writer_version_strictness() {
    let bytes = tagged_attributes(&[(11, vec![2])], 0);
    let legacy = parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        Some(201_708_239),
        &mut Diagnostics::new(),
    )
    .expect("legacy Boolean remains permissive");
    assert!(legacy.visible);

    for writer_version in [201_708_240_i64, 2_348_836_140_i64] {
        let error = parse_attributes(
            &bytes,
            0..bytes.len(),
            0..bytes.len(),
            ArchiveVersion::V8,
            Some(writer_version),
            &mut Diagnostics::new(),
        )
        .expect_err("modern Boolean must be canonical");
        assert!(matches!(
            error,
            crate::chunks::FramingError::Structural { .. }
        ));
    }
}

#[test]
fn parses_tagged_attribute_items_in_source_shaped_groups() {
    let mut items = Vec::new();
    let rendering = crc_chunk(
        ArchiveVersion::V8,
        0x4000_8000,
        &[
            1, 0, 0, 0, 3, 0, 0, 0, // object rendering version 1.3
            0, 0, 0, 0, // material-reference count
            0, 0, 0, 0, // mapping-reference count
            1, 1, 0, // casts shadows, receives shadows, advanced preview
        ],
    );
    let model_attributes = crc_chunk(ArchiveVersion::V8, 0x4000_8002, &[]);
    let mut direct_linetype = vec![2, 0, 0, 0, 1, 0, 0, 0];
    direct_linetype.extend(model_attributes.clone());
    direct_linetype.extend(0_i32.to_le_bytes());
    direct_linetype.push(0);
    let direct_linetype = crc_chunk(ArchiveVersion::V8, 0x4000_8000, &direct_linetype);
    let mut direct_section_style = vec![1, 0, 0, 0, 0, 0, 0, 0];
    direct_section_style.extend(model_attributes);
    direct_section_style.push(0);
    let direct_section_style = crc_chunk(ArchiveVersion::V8, 0x4000_8000, &direct_section_style);
    let mut item_28 = vec![0];
    item_28.extend(anonymous_chunk(ArchiveVersion::V8, 0, &0_i32.to_le_bytes()));
    items.extend([
        (1, utf16_bytes("N")),
        (2, utf16_bytes("U")),
        (3, 4_i32.to_le_bytes().to_vec()),
        (4, 5_i32.to_le_bytes().to_vec()),
        (5, rendering),
        (6, vec![1, 2, 3, 4]),
        (7, vec![5, 6, 7, 8]),
        (8, 0.5_f64.to_le_bytes().to_vec()),
        (9, vec![7]),
        (10, 3_i32.to_le_bytes().to_vec()),
        (11, vec![1]),
        (12, vec![0xf3]),
        (13, vec![1]),
        (14, vec![0]),
        (15, vec![0]),
        (16, vec![0]),
        (17, vec![0]),
        (18, 0_i32.to_le_bytes().to_vec()),
        (19, vec![1]),
        (20, uuid_bytes()),
        (21, 0_i32.to_le_bytes().to_vec()),
        (22, 2_i32.to_le_bytes().to_vec()),
        (23, vec![1]),
        (24, vec![2]),
        (25, vec![1]),
        (26, vec![2]),
        (27, vec![1]),
        (28, item_28),
        (29, vec![1]),
        (30, (-1_i32).to_le_bytes().to_vec()),
        (31, 1.0_f64.to_le_bytes().to_vec()),
        (32, 0.0_f64.to_le_bytes().to_vec()),
        (33, 1.0_f64.to_le_bytes().to_vec()),
        (34, vec![9, 9, 9, 9]),
        (35, vec![1]),
        (36, vec![1; 128]),
        (37, vec![1]),
        (38, direct_linetype),
        (39, direct_section_style),
        (40, vec![2]),
        (41, vec![1]),
        (42, vec![1]),
    ]);
    for (item, payload) in &items {
        let gate = match item {
            1..=21 => 0,
            22 => 1,
            23..=26 => 2,
            27..=28 => 3,
            29..=32 => 4,
            33 => 5,
            34..=35 => 6,
            36 => 8,
            37 => 9,
            38 => 10,
            39 => 11,
            40 => 12,
            41 | 42 => 13,
            _ => unreachable!("items are limited to 1 through 42"),
        };
        let minimum = tagged_attributes(&[(*item, payload.clone())], gate);
        let mut decoded_at_gate = parse_attributes(
            &minimum,
            0..minimum.len(),
            0..minimum.len(),
            ArchiveVersion::V8,
            None,
            &mut Diagnostics::new(),
        )
        .unwrap_or_else(|error| panic!("item {item} failed at minor {gate}: {error}"));
        let latest = tagged_attributes(&[(*item, payload.clone())], 13);
        let decoded_at_latest = parse_attributes(
            &latest,
            0..latest.len(),
            0..latest.len(),
            ArchiveVersion::V8,
            None,
            &mut Diagnostics::new(),
        )
        .unwrap_or_else(|error| panic!("item {item} failed at minor 13: {error}"));
        decoded_at_gate.version = decoded_at_latest.version;
        assert_eq!(
            decoded_at_gate, decoded_at_latest,
            "item {item} changed semantics after its minimum minor {gate}"
        );
        if gate > 0 {
            let preceding = tagged_attributes(&[(*item, payload.clone())], gate - 1);
            let decoded = parse_attributes(
                &preceding,
                0..preceding.len(),
                0..preceding.len(),
                ArchiveVersion::V8,
                None,
                &mut Diagnostics::new(),
            )
            .unwrap_or_else(|error| panic!("item {item} failed before minor {gate}: {error}"));
            let empty = tagged_attributes(&[], gate - 1);
            let expected = parse_attributes(
                &empty,
                0..empty.len(),
                0..preceding.len(),
                ArchiveVersion::V8,
                None,
                &mut Diagnostics::new(),
            )
            .expect("empty tagged attributes have source defaults");
            assert_eq!(
                decoded, expected,
                "item {item} crossed its gate instead of stopping at the boundary"
            );
        }
    }
    let bytes = tagged_attributes(&items, 13);
    let parsed = parse_attributes(
        &bytes,
        0..bytes.len(),
        10..10 + bytes.len(),
        ArchiveVersion::V8,
        None,
        &mut Diagnostics::new(),
    )
    .expect("required invariant");
    assert_eq!(parsed.name, "N");
    assert_eq!(parsed.url, "U");
    assert_eq!(parsed.object_mode & 0x0f, 3);
    assert_eq!(parsed.groups.len(), 0);
    assert_eq!(parsed.display_order, 2);
    assert_eq!(parsed.section_fill_rule, 1);
    assert!(parsed.embedded_linetype.is_some());
    assert!(parsed.embedded_section_style.is_some());
    assert_eq!(parsed.clipping_plane_label_style, 2);
    assert!(parsed.selective_clipping_list);
    assert!(parsed.detail_background_visible);
}

#[test]
fn object_rendering_attributes_require_minor_one() {
    let rendering = crc_chunk(
        ArchiveVersion::V8,
        0x4000_8000,
        &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    );
    let bytes = tagged_attributes(&[(5, rendering)], 0);
    assert!(parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        None,
        &mut Diagnostics::new(),
    )
    .is_err());
}

#[test]
fn object_rendering_attributes_consume_mapping_reference_and_channel() {
    let mut channel_body = 7_i32.to_le_bytes().to_vec();
    channel_body.extend(uuid_bytes());
    channel_body.extend(
        [
            1.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.25, -2.5, 3.75, 1.0,
        ]
        .into_iter()
        .flat_map(f64::to_le_bytes),
    );
    let channel = anonymous_chunk(ArchiveVersion::V8, 1, &channel_body);
    let mut mapping_body = uuid_bytes();
    mapping_body.extend(1_i32.to_le_bytes());
    mapping_body.extend(channel);
    let mapping = anonymous_chunk(ArchiveVersion::V8, 0, &mapping_body);
    let mut rendering = vec![1, 0, 0, 0, 3, 0, 0, 0];
    rendering.extend(0_i32.to_le_bytes());
    rendering.extend(1_i32.to_le_bytes());
    rendering.extend(mapping);
    rendering.extend([0, 0, 0]);
    let rendering = crc_chunk(ArchiveVersion::V8, 0x4000_8000, &rendering);
    let bytes = tagged_attributes(&[(5, rendering)], 0);

    let parsed = parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        None,
        &mut Diagnostics::new(),
    )
    .expect("mapping reference and channel have source-shaped framing");
    assert!(parsed.rendering_range.is_some());
}

#[test]
fn tagged_attributes_follow_source_cascade_boundaries() {
    for (minor, item) in [(0, 22), (1, 23), (2, 27), (12, 41), (12, 42)] {
        let bytes = tagged_attributes(&[(item, vec![0xaa, 0xbb])], minor);
        let parsed = parse_attributes(
            &bytes,
            0..bytes.len(),
            0..bytes.len(),
            ArchiveVersion::V8,
            None,
            &mut Diagnostics::new(),
        )
        .unwrap_or_else(|error| panic!("minor {minor} item {item}: {error}"));
        assert_eq!(parsed.version, (2, minor));
    }

    let mut out_of_order = tagged_attributes(&[(2, utf16_bytes("U")), (1, utf16_bytes("N"))], 0);
    out_of_order.extend([0xde, 0xad]);
    let parsed = parse_attributes(
        &out_of_order,
        0..out_of_order.len(),
        0..out_of_order.len(),
        ArchiveVersion::V8,
        None,
        &mut Diagnostics::new(),
    )
    .expect("out-of-order item stops at the containing attributes boundary");
    assert_eq!(parsed.url, "U");
    assert!(parsed.name.is_empty());
}

#[test]
fn tagged_attributes_reject_malformed_values_and_missing_terminator() {
    let bytes = tagged_attributes(&[(36, vec![0])], 8);
    assert!(
        parse_attributes(
            &bytes,
            0..bytes.len(),
            0..bytes.len(),
            ArchiveVersion::V8,
            None,
            &mut Diagnostics::new()
        )
        .is_err(),
        "an admitted object-frame item still requires its value grammar"
    );
    let bytes = tagged_attributes(&[(42, vec![])], 13);
    assert!(parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        None,
        &mut Diagnostics::new()
    )
    .is_err());

    let mut bytes = tagged_attributes(&[(1, utf16_bytes("N"))], 0);
    bytes.pop();
    assert!(parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        None,
        &mut Diagnostics::new()
    )
    .is_err());
}

#[test]
fn future_tagged_attributes_stop_at_unknown_item_and_preserve_suffix() {
    let mut bytes = tagged_attributes(&[(43, vec![0xaa, 0xbb])], 14);
    bytes.extend([0xde, 0xad]);
    let parsed = parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        None,
        &mut Diagnostics::new(),
    )
    .expect("future unknown item is bounded by the containing chunk");
    assert_eq!(parsed.version, (2, 14));
    assert_eq!(parsed.object_id, Uuid::nil());
    assert_eq!(parsed.layer_index, -1);
    assert!(parsed.name.is_empty());
}

#[test]
fn future_tagged_attributes_accept_known_prefix_and_suffix() {
    let mut bytes = tagged_attributes(&[(1, utf16_bytes("future"))], 14);
    bytes.extend([0xde, 0xad]);
    let parsed = parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        None,
        &mut Diagnostics::new(),
    )
    .expect("future minor with a known prefix");
    assert_eq!(parsed.version, (2, 14));
    assert_eq!(parsed.name, "future");
}

/// A tagged numeric item is refused at the value's first byte, which follows
/// the one-byte item tag.
#[test]
fn tagged_attributes_refuse_a_nonfinite_plot_weight_at_its_first_byte() {
    let bytes = tagged_attributes(&[(8, f64::NAN.to_le_bytes().to_vec())], 0);
    let value_offset = 22;
    assert_eq!(
        &bytes[value_offset..value_offset + 8],
        f64::NAN.to_le_bytes()
    );
    let error = parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        None,
        &mut Diagnostics::new(),
    )
    .expect_err("nonfinite plot weight");
    assert_eq!(
        error,
        crate::chunks::FramingError::structural(value_offset, "plot weight is not finite")
    );
}

/// The obsolete thickness is refused at its own first byte, not at the start of
/// the attribute body that contains it.
#[test]
fn fixed_attributes_refuse_a_nonfinite_obsolete_thickness_at_its_first_byte() {
    let mut bytes = fixed_attributes(4, 0, Some(true));
    let value_offset = 33;
    bytes[value_offset..value_offset + 8].copy_from_slice(&f64::NAN.to_le_bytes());
    let error = parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V4,
        None,
        &mut Diagnostics::new(),
    )
    .expect_err("nonfinite obsolete thickness");
    assert_eq!(
        error,
        crate::chunks::FramingError::structural(value_offset, "obsolete thickness is not finite")
    );
}

#[test]
fn tagged_attributes_reject_nonfinite_numeric_items() {
    let bytes = tagged_attributes(&[(8, f64::NAN.to_le_bytes().to_vec())], 0);
    assert!(parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        None,
        &mut Diagnostics::new()
    )
    .is_err());
}

#[test]
pub(crate) fn identity_resolution_defers_material_and_parent_colors() {
    let layer = settings::LayerRecord {
        source: settings::SourceRange { range: 0..1 },
        index: -1,
        iges_level: Some(0),
        render_material_index: -1,
        color: [10, 20, 30, 255],
        name: "Layer".to_string(),
        description: None,
        visible: true,
        locked: false,
        id: Some(Uuid::from_wire([1; 16])),
        hierarchy: None,
        linetype_index: None,
        plot: None,
        display_material_id: None,
        no_clipping_planes: None,
        visible_in_new_details: None,
        rendering_range: None,
        extension_items: Vec::new(),
        embedded_linetype: None,
        embedded_section_style: None,
        per_viewport_settings: Vec::new(),
    };
    let mut duplicate_layer = layer.clone();
    duplicate_layer.name = "Later layer".to_string();
    duplicate_layer.color = [90, 80, 70, 255];
    let mut metadata = settings::DocumentMetadata::default();
    metadata.layers.extend([layer, duplicate_layer]);
    let mut attributes = parse_attributes(
        &fixed_attributes(1, 0, None),
        0..fixed_attributes(1, 0, None).len(),
        0..fixed_attributes(1, 0, None).len(),
        ArchiveVersion::V4,
        None,
        &mut Diagnostics::new(),
    )
    .expect("required invariant");
    attributes.layer_index = -1;
    attributes.color_source = crate::objects::ColorSource::Material;
    let material = vec![ObjectRecord::Framed(descriptor(attributes.clone(), 10))];
    let mut warnings = Diagnostics::new();
    let material = crate::objects::resolve_identities(
        &cadmpeg_test_support::service_decode_context(),
        material,
        &metadata,
        &mut warnings,
    )
    .expect("service profile admits material identity");
    assert_eq!(
        material[0]
            .identity()
            .expect("required invariant")
            .effective_color,
        None
    );
    assert_eq!(
        material[0]
            .identity()
            .expect("required invariant")
            .layer
            .as_ref()
            .map(|layer| layer.name.as_str()),
        None
    );
    assert!(warnings.iter().any(|warning| {
        warning.code == Some(crate::loss::RhinoLossCode::DuplicateRecordResolved)
            && warning.contains("ambiguous layer index -1")
    }));

    attributes.color_source = crate::objects::ColorSource::Parent;
    attributes.object_mode = 0xf3;
    let parent = vec![ObjectRecord::Framed(descriptor(attributes, 20))];
    let parent = crate::objects::resolve_identities(
        &cadmpeg_test_support::service_decode_context(),
        parent,
        &metadata,
        &mut warnings,
    )
    .expect("service profile admits parent identity");
    assert_eq!(
        parent[0]
            .identity()
            .expect("required invariant")
            .effective_color,
        None
    );
    assert_eq!(
        parent[0]
            .identity()
            .expect("required invariant")
            .object_mode
            & 0x0f,
        IDEF_OBJECT_MODE
    );
}

#[test]
fn identity_resolution_warns_and_keys_nil_and_duplicate_uuids_by_record() {
    let bytes = fixed_attributes(1, 0, None);
    let attributes = parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V4,
        None,
        &mut Diagnostics::new(),
    )
    .expect("required invariant");
    let mut duplicate = attributes.clone();
    duplicate.object_id = Uuid::from_wire([1; 16]);
    let mut duplicate_again = duplicate.clone();
    duplicate_again.object_id = duplicate.object_id;
    let mut objects = vec![
        ObjectRecord::Framed(descriptor(attributes, 10)),
        ObjectRecord::Framed(descriptor(duplicate, 20)),
        ObjectRecord::Framed(descriptor(duplicate_again, 30)),
    ];
    let ObjectRecord::Framed(object) = &mut objects[0] else {
        panic!("test object is framed");
    };
    object.class_uuid = Uuid::from_wire([9; 16]);
    let mut warnings = Diagnostics::new();
    let objects = crate::objects::resolve_identities(
        &cadmpeg_test_support::service_decode_context(),
        objects,
        &settings::DocumentMetadata::default(),
        &mut warnings,
    )
    .expect("service profile admits object identities");
    assert_ne!(
        objects[0].identity().expect("required invariant").source_id,
        objects[2].identity().expect("required invariant").source_id
    );
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("nil object UUID")));
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("duplicate object UUID")));
    assert_eq!(
        objects[0]
            .identity()
            .expect("required invariant")
            .class_uuid,
        Uuid::from_wire([9; 16])
    );
}

#[test]
fn resolved_object_identities_refuse_collection_limit() {
    let objects = vec![ObjectRecord::Degraded {
        range: 0..1,
        warning: "fixture".to_owned(),
    }];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let refusal = crate::objects::resolve_identities(
        &ctx,
        objects.clone(),
        &settings::DocumentMetadata::default(),
        &mut Diagnostics::new(),
    )
    .expect_err("one resolved record exceeds zero collection items");
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino resolved object identities"
    ));
    let resolved = crate::objects::resolve_identities(
        &cadmpeg_test_support::service_decode_context(),
        objects,
        &settings::DocumentMetadata::default(),
        &mut Diagnostics::new(),
    )
    .expect("service profile admits resolved records");
    assert_eq!(resolved.len(), 1);
}

#[test]
fn malformed_attribute_userdata_diagnostic_refuses_collection_limit() {
    let bytes = [0_u8];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("root bytes admitted");
    let refusal = crate::objects::parse_attribute_userdata(
        &ctx,
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        &mut Diagnostics::new(),
    )
    .expect_err("one degradation diagnostic exceeds zero collection items");
    assert!(matches!(
        refusal,
        crate::chunks::FramingError::Resource(limit)
            if limit.operation == "Rhino diagnostics"
    ));
    let mut warnings = Diagnostics::new();
    crate::objects::parse_attribute_userdata(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        &mut warnings,
    )
    .expect("service profile retains malformed userdata warning");
    assert_eq!(warnings.iter().count(), 1);
}

#[test]
pub(crate) fn attribute_userdata_recovers_after_malformed_bounded_record() {
    let mut malformed = long_chunk(ArchiveVersion::V4, 0x0002_7ffd, &[0x10]);
    let mut valid_body = vec![0x10];
    valid_body.extend(uuid_bytes());
    valid_body.extend(uuid_bytes());
    valid_body.extend(1_i32.to_le_bytes());
    valid_body.extend([0; 128]);
    valid_body.extend(crc_chunk(ArchiveVersion::V4, 0x4000_8000, &[9, 8, 7]));
    valid_body.extend(short_chunk(ArchiveVersion::V4, 0x8002_7fff, 0));
    let valid = long_chunk(ArchiveVersion::V4, 0x0002_7ffd, &valid_body);
    malformed.extend(valid);
    let mut warnings = Diagnostics::new();
    let descriptors = crate::objects::parse_attribute_userdata(
        &cadmpeg_test_support::service_decode_context(),
        &malformed,
        0..malformed.len(),
        ArchiveVersion::V4,
        &mut warnings,
    )
    .expect("valid userdata descriptor remains after malformed child");
    assert_eq!(descriptors.len(), 1);
    assert!(descriptors[0].known().is_some());
    assert!(descriptors[0].known().unwrap().range.start > 0);
    assert!(!warnings.is_empty());
}

#[test]
fn obsolete_custom_mesh_userdata_transfers_to_object_attributes() {
    for (version, archive) in [("40", ArchiveVersion::V4), ("50", ArchiveVersion::V5)] {
        let mut userdata_body = 37_i32.to_le_bytes().to_vec();
        userdata_body.push(1);
        userdata_body.extend(mesh_parameters(archive));
        userdata_body.extend([0xde, 0xad]);
        let userdata = if archive == ArchiveVersion::V4 {
            class_userdata_v1_with_direct_payload(
                archive,
                crate::objects::OBSOLETE_CUSTOM_MESH_USERDATA.to_wire(),
                &userdata_body,
            )
        } else {
            class_userdata_v2_with_direct_payload(
                archive,
                crate::objects::OBSOLETE_CUSTOM_MESH_USERDATA.to_wire(),
                [0; 16],
                50,
                2_348_836_140_u32,
                &userdata_body,
            )
        };
        let attributes = if archive == ArchiveVersion::V4 {
            fixed_attributes(8, 0, Some(true))
        } else {
            tagged_attributes(&[], 0)
        };
        let object =
            object_record_with_attribute_userdata(archive, 1, POINT_CLASS, &attributes, &userdata);
        let bytes = minimal_document(
            version,
            &[
                table(archive, 0x1000_0014, &[]),
                table(archive, 0x1000_0015, &[]),
                table(archive, 0x1000_0013, &[object]),
            ],
        );
        let scan = crate::container::scan_owned(bytes).expect("custom mesh object record");
        let object = scan.objects[0].framed().expect("test object is framed");
        assert_eq!(object.attributes_userdata.len(), 1);
        assert_eq!(
            object.attributes_userdata[0]
                .known()
                .map(|value| value.class_uuid),
            Some(crate::objects::OBSOLETE_CUSTOM_MESH_USERDATA)
        );
        let mesh = object
            .attributes
            .parsed()
            .and_then(|attributes| attributes.custom_render_mesh.as_ref())
            .expect("converted custom mesh settings");
        assert_eq!(mesh.version, (1, 5));
        assert!(!mesh.compute_curvature);
        assert!(mesh.simple_planes);
        assert_eq!(mesh.obsolete_weld, -17);
        assert_eq!(mesh.tolerance, crate::test_support::finite(0.125));
        assert_eq!(mesh.custom_settings, Some(true));
        assert_eq!(mesh.custom_settings_enabled, Some(true));
        assert_eq!(
            mesh.subd.as_ref().map(|value| value.display_density),
            Some(5)
        );
        assert_eq!(mesh.subd.as_ref().map(|value| value.mesh_location), Some(2));
        assert!(object.checksum_warnings.iter().all(|warning| {
            !warning.contains("obsolete custom mesh userdata") || !warning.contains("dropped")
        }));
    }
}

#[test]
fn malformed_obsolete_custom_mesh_userdata_keeps_object_attributes() {
    let archive = ArchiveVersion::V5;
    let userdata_body = [7_i32.to_le_bytes().as_slice(), [2].as_slice()].concat();
    let userdata = class_userdata_v2_with_direct_payload(
        archive,
        crate::objects::OBSOLETE_CUSTOM_MESH_USERDATA.to_wire(),
        [0; 16],
        50,
        2_348_836_140_u32,
        &userdata_body,
    );
    let attributes = tagged_attributes(&[], 0);
    let object =
        object_record_with_attribute_userdata(archive, 1, POINT_CLASS, &attributes, &userdata);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[object]),
        ],
    );
    let scan = crate::container::scan_owned(bytes).expect("malformed custom mesh record");
    let object = scan.objects[0].framed().expect("test object is framed");
    assert!(object.attributes.parsed().is_some());
    assert!(object
        .attributes
        .parsed()
        .expect("object attributes")
        .custom_render_mesh
        .is_none());
    assert!(object
        .checksum_warnings
        .iter()
        .any(|warning| warning.contains("obsolete custom mesh userdata")
            && warning.contains("dropped")));
}

#[test]
fn per_object_mesh_userdata_transfers_nested_parameters_to_object_attributes() {
    for (version, archive, archive_version) in [
        ("4", ArchiveVersion::V4, 4),
        ("50", ArchiveVersion::V5, 50),
        ("60", ArchiveVersion::V6, 60),
    ] {
        let inner = crc_chunk(archive, 0x4000_8000, &mesh_parameters(archive));
        let mut outer_body = 1_i32.to_le_bytes().to_vec();
        outer_body.extend(0_i32.to_le_bytes());
        outer_body.extend(inner);
        outer_body.extend([0xde, 0xad]);
        let userdata_body = crc_chunk(archive, 0x4000_8000, &outer_body);
        let userdata = class_userdata_v2_with_direct_payload(
            archive,
            crate::objects::PER_OBJECT_MESH_PARAMETERS_USERDATA.to_wire(),
            [0; 16],
            archive_version,
            0,
            &userdata_body,
        );
        let object = object_record_with_attribute_userdata(
            archive,
            1,
            POINT_CLASS,
            &(if archive == ArchiveVersion::V4 {
                fixed_attributes(8, 0, Some(true))
            } else {
                tagged_attributes(&[], 0)
            }),
            &userdata,
        );
        let bytes = minimal_document(
            version,
            &[
                table(archive, 0x1000_0014, &[]),
                table(archive, 0x1000_0015, &[]),
                table(archive, 0x1000_0013, &[object]),
            ],
        );
        let scan = crate::container::scan_owned(bytes).expect("per-object mesh record");
        let object = scan.objects[0].framed().expect("test object is framed");
        assert_eq!(object.attributes_userdata.len(), 1);
        assert_eq!(
            object.attributes_userdata[0]
                .known()
                .map(|value| value.class_uuid),
            Some(crate::objects::PER_OBJECT_MESH_PARAMETERS_USERDATA)
        );
        let mesh = object
            .attributes
            .parsed()
            .and_then(|attributes| attributes.custom_render_mesh.as_ref())
            .expect("nested custom mesh settings");
        assert_eq!(mesh.version, (1, 5));
        assert!(!mesh.compute_curvature);
        assert_eq!(mesh.custom_settings, Some(true));
        assert_eq!(mesh.custom_settings_enabled, Some(false));
        assert_eq!(mesh.obsolete_weld, -17);
        assert_eq!(mesh.tolerance, crate::test_support::finite(0.125));
        assert_eq!(
            mesh.subd.as_ref().map(|value| value.display_density),
            Some(5)
        );
        assert_eq!(mesh.subd.as_ref().map(|value| value.mesh_location), Some(2));
        assert!(object.checksum_warnings.iter().all(|warning| {
            !warning.contains("per-object mesh userdata") || !warning.contains("dropped")
        }));
    }
}

#[test]
fn malformed_per_object_mesh_userdata_keeps_object_attributes() {
    let archive = ArchiveVersion::V5;
    let malformed_outer = anonymous_chunk(archive, 0, &short_chunk(archive, 0x4000_8000, 0));
    let userdata = class_userdata_v2_with_direct_payload(
        archive,
        crate::objects::PER_OBJECT_MESH_PARAMETERS_USERDATA.to_wire(),
        [0; 16],
        50,
        0,
        &malformed_outer,
    );
    let object = object_record_with_attribute_userdata(
        archive,
        1,
        POINT_CLASS,
        &tagged_attributes(&[], 0),
        &userdata,
    );
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[object]),
        ],
    );
    let scan = crate::container::scan_owned(bytes).expect("malformed per-object mesh record");
    let object = scan.objects[0].framed().expect("test object is framed");
    assert!(object.attributes.parsed().is_some());
    assert!(object
        .attributes
        .parsed()
        .expect("object attributes")
        .custom_render_mesh
        .is_none());
    assert!(
        object
            .checksum_warnings
            .iter()
            .any(|warning| warning.contains("per-object mesh userdata")
                && warning.contains("dropped"))
    );
}

#[test]
fn custom_mesh_userdata_diagnostics_refuse_collection_limit() {
    let bytes = [0_u8; 5];
    for (class_uuid, parse) in [
        (
            crate::objects::OBSOLETE_CUSTOM_MESH_USERDATA,
            super::parse_obsolete_custom_mesh_userdata
                as fn(
                    &cadmpeg_core::decode::DecodeContext<'_>,
                    &[u8],
                    &[crate::objects::AttributeUserdataDescriptor],
                    ArchiveVersion,
                    &mut Diagnostics,
                ) -> Result<
                    Option<crate::settings::MeshParameters>,
                    crate::chunks::FramingError,
                >,
        ),
        (
            crate::objects::PER_OBJECT_MESH_PARAMETERS_USERDATA,
            super::parse_per_object_mesh_userdata,
        ),
    ] {
        let descriptors = [crate::objects::AttributeUserdataDescriptor::Known(
            crate::objects::AttributeUserdata {
                range: 0..bytes.len(),
                class_uuid,
                item_uuid: class_uuid,
                application_uuid: None,
                writer_version: None,
                payload_range: 0..bytes.len(),
            },
        )];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("root bytes admitted");
        let refused = parse(
            &ctx,
            &bytes,
            &descriptors,
            ArchiveVersion::V5,
            &mut Diagnostics::new(),
        )
        .expect_err("custom mesh diagnostic exceeds zero collection items");
        assert!(
            matches!(refused, crate::chunks::FramingError::Resource(limit) if limit.operation == "Rhino diagnostics")
        );
    }
}

#[test]
fn user_string_list_reads_ordered_entries_and_bounded_suffixes() {
    let archive = ArchiveVersion::V5;
    let mut first = utf16_bytes("CaseKey");
    first.extend(utf16_bytes("first"));
    let mut second = utf16_bytes("casekey");
    second.extend(utf16_bytes("second"));
    let mut list_body = 2_i32.to_le_bytes().to_vec();
    list_body.extend(anonymous_chunk(archive, 7, &first));
    let mut second_entry = anonymous_chunk(archive, 9, &second);
    second_entry.extend([0xaa, 0xbb]);
    list_body.extend(second_entry);
    let mut payload = anonymous_chunk(archive, 3, &list_body);
    payload.extend([0xde, 0xad]);

    let values = crate::objects::parse_user_string_list(
        &cadmpeg_test_support::service_decode_context(),
        &payload,
        0..payload.len(),
        archive,
    )
    .expect("user-string list");
    assert_eq!(
        values,
        [
            ("CaseKey".to_string(), "first".to_string()),
            ("casekey".to_string(), "second".to_string())
        ]
    );
}

#[test]
fn user_string_list_rejects_a_negative_count() {
    let archive = ArchiveVersion::V5;
    let payload = anonymous_chunk(archive, 0, &(-1_i32).to_le_bytes());
    assert!(crate::objects::parse_user_string_list(
        &cadmpeg_test_support::service_decode_context(),
        &payload,
        0..payload.len(),
        archive,
    )
    .is_err());
}

fn user_string_list_refusal(
    collection_limit: u64,
    retained_limit: u64,
) -> crate::chunks::FramingError {
    let archive = ArchiveVersion::V5;
    let mut entry = utf16_bytes("key");
    entry.extend(utf16_bytes("value"));
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(anonymous_chunk(archive, 0, &entry));
    let bytes = anonymous_chunk(archive, 0, &body);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("root bytes admitted");
    crate::objects::parse_user_string_list(&ctx, &bytes, 0..bytes.len(), archive)
        .expect_err("user strings exceed configured limit")
}

#[test]
fn user_string_entries_refuse_collection_limit() {
    assert_attribute_resource(
        &user_string_list_refusal(0, 100),
        "Rhino user-string entries",
    );
}

#[test]
fn user_string_key_refuses_retained_limit() {
    assert_attribute_resource(&user_string_list_refusal(100, 2), "Rhino user-string key");
}

#[test]
fn user_string_value_refuses_retained_limit() {
    assert_attribute_resource(&user_string_list_refusal(100, 3), "Rhino user-string value");
}

#[test]
fn null_polymorphic_wrapper_contains_only_a_nil_class_uuid() {
    let archive = ArchiveVersion::V5;
    let uuid = crc_chunk(archive, 0x0002_fffb, &[0; 16]);
    let wrapper = long_chunk(archive, 0x0002_7ffa, &uuid);
    let (class, userdata) = crate::objects::parse_class_wrapper_with_userdata(
        &cadmpeg_test_support::service_decode_context(),
        &wrapper,
        0..wrapper.len(),
        archive,
        &mut Diagnostics::new(),
    )
    .expect("null object wrapper");
    assert_eq!(class.class_uuid, Uuid::nil());
    assert!(class.class_data_range.is_empty());
    assert!(userdata.is_empty());
}

#[test]
fn class_uuid_checksum_diagnostic_refuses_collection_limit() {
    let archive = ArchiveVersion::V5;
    let mut uuid = crc_chunk(archive, 0x0002_fffb, &[0; 16]);
    let crc_offset = uuid.len() - 1;
    uuid[crc_offset] ^= 1;
    let wrapper = long_chunk(archive, 0x0002_7ffa, &uuid);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&wrapper, &arena, &policy)
        .expect("root bytes admitted");
    let refusal = crate::objects::parse_class_wrapper(
        &ctx,
        &wrapper,
        0..wrapper.len(),
        archive,
        &mut Diagnostics::new(),
    )
    .expect_err("checksum diagnostic exceeds zero collection items");
    assert!(matches!(
        refusal,
        crate::chunks::FramingError::Resource(limit)
            if limit.operation == "Rhino diagnostics"
    ));
    let mut warnings = Diagnostics::new();
    crate::objects::parse_class_wrapper(
        &cadmpeg_test_support::service_decode_context(),
        &wrapper,
        0..wrapper.len(),
        archive,
        &mut warnings,
    )
    .expect("service profile retains checksum diagnostic");
    assert_eq!(warnings.iter().count(), 1);
}

#[test]
fn retained_class_userdata_refuses_collection_limit_without_affecting_scan_only() {
    let archive = ArchiveVersion::V8;
    let userdata = crate::test_support::test_dump::class_userdata(
        archive,
        [2; 16],
        [3; 16],
        "source.3dm",
        false,
    );
    let wrapper = crate::test_support::test_dump::class_wrapper_with_userdata(
        archive,
        [1; 16],
        &[],
        &userdata,
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&wrapper, &arena, &policy)
        .expect("root bytes admitted");
    crate::objects::parse_class_wrapper(
        &ctx,
        &wrapper,
        0..wrapper.len(),
        archive,
        &mut Diagnostics::new(),
    )
    .expect("scan-only class wrapper does not retain userdata");
    let error = crate::objects::parse_class_wrapper_with_userdata(
        &ctx,
        &wrapper,
        0..wrapper.len(),
        archive,
        &mut Diagnostics::new(),
    )
    .expect_err("retained userdata exceeds collection limit");
    assert!(matches!(
        error,
        crate::chunks::FramingError::Resource(refusal)
            if refusal.operation == "Rhino class userdata"
    ));
    let (class, retained) = crate::objects::parse_class_wrapper_with_userdata(
        &cadmpeg_test_support::service_decode_context(),
        &wrapper,
        0..wrapper.len(),
        archive,
        &mut Diagnostics::new(),
    )
    .expect("service profile retains userdata");
    assert_eq!(class.class_uuid, Uuid::from_wire([1; 16]));
    assert_eq!(retained.len(), 1);
}

#[test]
fn uuid_list_uses_an_anonymous_versioned_chunk() {
    let archive = ArchiveVersion::V5;
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend([0x11; 16]);
    let bytes = anonymous_chunk(archive, 0, &body);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded UUID-list reader");
    let values = crate::objects::read_uuid_list(
        &cadmpeg_test_support::service_decode_context(),
        &mut reader,
        archive,
    )
    .expect("UUID list");
    assert_eq!(values.len(), 1);
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn object_uuid_list_refuses_collection_limit() {
    let archive = ArchiveVersion::V5;
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend([0x11; 16]);
    let bytes = anonymous_chunk(archive, 0, &body);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("root bytes admitted");
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded UUID-list reader");
    let error = crate::objects::read_uuid_list(&ctx, &mut reader, archive)
        .expect_err("UUID list exceeds collection limit");
    assert_attribute_resource(&error, "Rhino UUID list");
}

#[test]
fn object_trailer_accepts_bounded_unknown_child_without_history() {
    let archive = ArchiveVersion::V5;
    let object = object_record_with_unknown_trailer(archive, POINT_CLASS);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[object]),
        ],
    );
    let scan = crate::container::scan_owned(bytes).expect("bounded unknown trailer");
    assert_eq!(
        scan.objects[0]
            .framed()
            .expect("test object is framed")
            .unknown_trailer
            .len(),
        1
    );
}

fn object_record_collection_refusal(bytes: &[u8], limit: u64) -> crate::chunks::FramingError {
    let archive = ArchiveVersion::V5;
    let chunk = crate::chunks::chunk_at(bytes, 0, bytes.len(), archive, false)
        .expect("object record framing");
    let record = crate::container::Record::long(chunk.typecode, chunk.range(), chunk.body());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("root bytes admitted");
    crate::objects::parse_object_record(
        &ctx,
        bytes,
        &record,
        archive,
        None,
        &mut Diagnostics::new(),
    )
    .expect_err("object collection exceeds configured limit")
}

#[test]
fn object_unknown_trailer_refuses_collection_limit() {
    let bytes = object_record_with_unknown_trailer(ArchiveVersion::V5, POINT_CLASS);
    assert!(matches!(
        object_record_collection_refusal(&bytes, 0),
        crate::chunks::FramingError::Resource(refusal)
            if refusal.operation == "Rhino object unknown trailer"
    ));
}

#[test]
fn object_class_userdata_refuses_collection_limit() {
    let archive = ArchiveVersion::V5;
    let object_type = short_chunk(archive, 0x8200_0071, 1);
    let uuid = crc_chunk(archive, 0x0002_fffb, &POINT_CLASS);
    let class_data = crc_chunk(archive, 0x0002_fffc, &[]);
    let userdata = class_userdata_v1_with_direct_payload(archive, [1; 16], &[]);
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    let class = long_chunk(
        archive,
        0x0002_7ffa,
        &[uuid, class_data, userdata, class_end].concat(),
    );
    let object_end = short_chunk(archive, 0x8200_007f, 0);
    let bytes = crate::test_support::test_dump::nested_crc_chunk(
        archive,
        0x2000_8070,
        &[object_type, class, object_end].concat(),
    );
    assert!(matches!(
        object_record_collection_refusal(&bytes, 0),
        crate::chunks::FramingError::Resource(refusal)
            if refusal.operation == "Rhino object userdata"
    ));
    let chunk = crate::chunks::chunk_at(&bytes, 0, bytes.len(), archive, false)
        .expect("object record framing");
    let record = crate::container::Record::long(chunk.typecode, chunk.range(), chunk.body());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("root bytes admitted");
    let parsed = crate::objects::parse_object_record(
        &ctx,
        &bytes,
        &record,
        archive,
        None,
        &mut Diagnostics::new(),
    )
    .expect("service profile admits class userdata");
    assert_eq!(parsed.framed().expect("framed object").userdata.len(), 1);
}

#[test]
pub(crate) fn malformed_bounded_object_is_retained_and_later_point_decodes() {
    let archive = ArchiveVersion::V5;
    for malformed in [object_record_without_end(archive, 1, [0; 16]), {
        let mut bytes = object_record(archive, 1, [0; 16]);
        bytes[12..16].copy_from_slice(&0x82a0_0072_u32.to_le_bytes());
        bytes
    }] {
        let point =
            object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0]));
        let bytes = minimal_document(
            "50",
            &[
                table(archive, 0x1000_0014, &[]),
                table(archive, 0x1000_0015, &[]),
                table(archive, 0x1000_0013, &[malformed, point]),
            ],
        );
        let mut scan = crate::container::scan_owned(bytes).expect("bounded object recovery");
        assert!(scan.objects[0].is_degraded());
        set_test_units(&mut scan, 1.0);
        let result = crate::decode::decode_for_test(&scan);
        assert_eq!(
            result
                .ir()
                .native_unknowns("rhino")
                .expect("required invariant")
                .len(),
            2
        );
        assert_eq!(result.ir().model.points.len(), 1);
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.severity == Severity::Error));
    }
}

#[test]
fn object_warning_lists_do_not_inherit_global_warnings() {
    let archive = ArchiveVersion::V5;
    let first = object_record(archive, 1, [0; 16]);
    let mut second = object_record(archive, 2, [1; 16]);
    let last = second.len() - 1;
    second[last] ^= 1;
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[first, second]),
        ],
    );
    let scan = crate::container::scan_owned(bytes).expect("required invariant");
    assert!(scan.objects[0]
        .framed()
        .expect("test object is framed")
        .checksum_warnings
        .is_empty());
    assert!(scan.objects[1]
        .framed()
        .expect("test object is framed")
        .checksum_warnings
        .is_empty());
    assert_eq!(
        scan.warnings
            .iter()
            .filter(|warning| warning.contains("CRC mismatch"))
            .count(),
        1
    );
}

#[test]
fn geometry_decode_does_not_clear_attribute_degradation() {
    let archive = ArchiveVersion::V5;
    let object = object_record(archive, 1, [0; 16]);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[object]),
        ],
    );
    let mut scan = crate::container::scan_owned(bytes).expect("required invariant");
    let ObjectRecord::Framed(object) = &mut scan.objects[0] else {
        panic!("test object is framed");
    };
    object.attributes = AttributeState::Degraded;
    crate::decode::with_expand(&scan, |expand| {
        let mut context =
            crate::decode::DecodeContext::new(&scan, expand).expect("test transaction");
        assert!(context.mark_decoded(0));
        let result =
            crate::decode::seal_for_test(context.commit().expect("test decode commit"), false);
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == crate::loss::RhinoLossCode::ObjectAttributesDegraded.kind()
        }));
    });
}

#[test]
fn report_attributes_aggregated_class_losses_to_first_object_record() {
    let archive = ArchiveVersion::V5;
    let class_uuid = [7; 16];
    let object = object_record(archive, 1, class_uuid);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[object]),
        ],
    );
    let scan = crate::container::scan_owned(bytes).expect("required invariant");
    let offset = scan.objects[0].range().start as u64;
    let class = scan.objects[0].class_uuid().unwrap().to_string();
    let result = crate::decode::decode_for_test(&scan);

    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code.category() == cadmpeg_ir::report::loss::LossCategory::Geometry
                && loss.provenance.is_some()
        })
        .and_then(|loss| loss.provenance.as_ref())
        .expect("retained geometry loss has provenance");
    let expected_tag = format!("OBJECT_RECORD/class={class}/type=0x00000001");
    assert_eq!(wire::field::<String>(&loss, "format"), "rhino");
    assert_eq!(
        wire::field_or_default::<Option<String>>(&loss, "stream").as_deref(),
        None
    );
    assert_eq!(loss.offset, offset);
    assert_eq!(loss.tag.as_deref(), Some(expected_tag.as_str()));
    assert!(!result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code != crate::loss::RhinoLossCode::IntegrityFailure.kind())
        .any(|loss| { loss.message.contains("OBJECT_RECORD") || loss.message.contains("offset") }));
}

#[test]
fn degraded_object_warning_refuses_retained_limit() {
    let record = crate::container::Record::short(0x2000_8070, 0..1, 0);
    let error = crate::chunks::FramingError::InvalidHeader;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let refusal = crate::objects::degraded_object_record(&ctx, &record, &error)
        .expect_err("degraded warning text exceeds zero retained bytes");
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino degraded object warning"
    ));
    let result = crate::objects::degraded_object_record(
        &cadmpeg_test_support::service_decode_context(),
        &record,
        &error,
    )
    .expect("service profile admits degraded warning");
    assert!(
        matches!(result, ObjectRecord::Degraded { warning, .. } if warning.contains("degraded"))
    );
}
