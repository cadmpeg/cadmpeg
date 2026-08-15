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
fn parses_fixed_attributes_through_every_minor_gate() {
    for minor in 0..=8 {
        let bytes = fixed_attributes(minor, 0, Some(true));
        let parsed = crate::objects::parse_attributes(
            &bytes,
            0..bytes.len(),
            100..100 + bytes.len(),
            ArchiveVersion::V4,
            None,
            &mut Vec::new(),
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
    let hidden = crate::objects::parse_attributes(
        &hidden,
        0..hidden.len(),
        0..hidden.len(),
        ArchiveVersion::V4,
        None,
        &mut Vec::new(),
    )
    .expect("required invariant");
    assert!(!hidden.visible);

    let locked = fixed_attributes(1, 0x12, None);
    let locked = crate::objects::parse_attributes(
        &locked,
        0..locked.len(),
        0..locked.len(),
        ArchiveVersion::V4,
        None,
        &mut Vec::new(),
    )
    .expect("required invariant");
    assert!(locked.visible);

    let definition = fixed_attributes(1, 0xf3, None);
    let definition = crate::objects::parse_attributes(
        &definition,
        0..definition.len(),
        0..definition.len(),
        ArchiveVersion::V4,
        None,
        &mut Vec::new(),
    )
    .expect("required invariant");
    assert_eq!(definition.object_mode & 0x0f, 3);
}

#[test]
fn fixed_explicit_visibility_overrides_hidden_mode_default() {
    let bytes = fixed_attributes(2, 0x02, Some(true));
    let parsed = crate::objects::parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V4,
        None,
        &mut Vec::new(),
    )
    .expect("required invariant");
    assert!(parsed.visible);
}

#[test]
fn object_attribute_booleans_use_writer_version_strictness() {
    let bytes = tagged_attributes(&[(11, vec![2])], 0);
    let legacy = crate::objects::parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        Some(201_708_239),
        &mut Vec::new(),
    )
    .expect("legacy Boolean remains permissive");
    assert!(legacy.visible);

    for writer_version in [201_708_240_i64, 2_348_836_140_i64] {
        let error = crate::objects::parse_attributes(
            &bytes,
            0..bytes.len(),
            0..bytes.len(),
            ArchiveVersion::V8,
            Some(writer_version),
            &mut Vec::new(),
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
        &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
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
            41 => 13,
            _ => unreachable!("items are limited to 1 through 41"),
        };
        let minimum = tagged_attributes(&[(*item, payload.clone())], gate);
        let mut decoded_at_gate = crate::objects::parse_attributes(
            &minimum,
            0..minimum.len(),
            0..minimum.len(),
            ArchiveVersion::V8,
            None,
            &mut Vec::new(),
        )
        .unwrap_or_else(|error| panic!("item {item} failed at minor {gate}: {error}"));
        let latest = tagged_attributes(&[(*item, payload.clone())], 13);
        let decoded_at_latest = crate::objects::parse_attributes(
            &latest,
            0..latest.len(),
            0..latest.len(),
            ArchiveVersion::V8,
            None,
            &mut Vec::new(),
        )
        .unwrap_or_else(|error| panic!("item {item} failed at minor 13: {error}"));
        decoded_at_gate.version = decoded_at_latest.version;
        assert_eq!(
            decoded_at_gate, decoded_at_latest,
            "item {item} changed semantics after its minimum minor {gate}"
        );
        if gate > 0 {
            let preceding = tagged_attributes(&[(*item, payload.clone())], gate - 1);
            assert!(
                crate::objects::parse_attributes(
                    &preceding,
                    0..preceding.len(),
                    0..preceding.len(),
                    ArchiveVersion::V8,
                    None,
                    &mut Vec::new(),
                )
                .is_err(),
                "item {item} was accepted before minor {gate}"
            );
        }
    }
    let bytes = tagged_attributes(&items, 13);
    let parsed = crate::objects::parse_attributes(
        &bytes,
        0..bytes.len(),
        10..10 + bytes.len(),
        ArchiveVersion::V8,
        None,
        &mut Vec::new(),
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
}

#[test]
fn tagged_attributes_reject_unknown_items_gates_and_missing_terminator() {
    for (minor, item) in [(0, 22), (1, 23), (2, 27), (8, 36), (12, 41)] {
        let bytes = tagged_attributes(&[(item, vec![0])], minor);
        assert!(
            crate::objects::parse_attributes(
                &bytes,
                0..bytes.len(),
                0..bytes.len(),
                ArchiveVersion::V8,
                None,
                &mut Vec::new()
            )
            .is_err(),
            "minor {minor} item {item}"
        );
    }
    let mut bytes = tagged_attributes(&[(1, utf16_bytes("N"))], 0);
    bytes.pop();
    assert!(crate::objects::parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        None,
        &mut Vec::new()
    )
    .is_err());
    let bytes = tagged_attributes(&[(42, vec![])], 13);
    assert!(crate::objects::parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        None,
        &mut Vec::new()
    )
    .is_err());
}

#[test]
fn tagged_attributes_reject_nonfinite_numeric_items() {
    let bytes = tagged_attributes(&[(8, f64::NAN.to_le_bytes().to_vec())], 0);
    assert!(crate::objects::parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        None,
        &mut Vec::new()
    )
    .is_err());
}

#[test]
pub(crate) fn identity_resolution_defers_material_and_parent_colors() {
    let layer = settings::LayerRecord {
        source: settings::SourceRange { range: 0..1 },
        version: (1, 15),
        obsolete_mode: 0,
        index: -1,
        iges_level: 0,
        render_material_index: -1,
        color: [10, 20, 30, 255],
        name: "Layer".to_string(),
        visible: true,
        locked: false,
        id: Some(Uuid::from_wire([1; 16])),
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
    let mut duplicate_layer = layer.clone();
    duplicate_layer.name = "Later layer".to_string();
    duplicate_layer.color = [90, 80, 70, 255];
    let mut metadata = settings::DocumentMetadata::default();
    metadata.layers.extend([layer, duplicate_layer]);
    let mut attributes = crate::objects::parse_attributes(
        &fixed_attributes(1, 0, None),
        0..fixed_attributes(1, 0, None).len(),
        0..fixed_attributes(1, 0, None).len(),
        ArchiveVersion::V4,
        None,
        &mut Vec::new(),
    )
    .expect("required invariant");
    attributes.layer_index = -1;
    attributes.color_source = 2;
    let mut material = vec![descriptor(attributes.clone(), 10)];
    let mut warnings = Vec::new();
    crate::objects::resolve_identities(&mut material, &metadata, &mut warnings);
    assert_eq!(
        material[0]
            .identity
            .as_ref()
            .expect("required invariant")
            .effective_color,
        None
    );
    assert_eq!(
        material[0]
            .identity
            .as_ref()
            .expect("required invariant")
            .layer_name
            .as_deref(),
        Some("Layer")
    );

    attributes.color_source = 3;
    attributes.object_mode = 0xf3;
    let mut parent = vec![descriptor(attributes, 20)];
    crate::objects::resolve_identities(&mut parent, &metadata, &mut warnings);
    assert_eq!(
        parent[0]
            .identity
            .as_ref()
            .expect("required invariant")
            .effective_color,
        None
    );
    assert!(
        parent[0]
            .identity
            .as_ref()
            .expect("required invariant")
            .definition_member
    );
}

#[test]
fn identity_resolution_warns_and_keys_nil_and_duplicate_uuids_by_record() {
    let bytes = fixed_attributes(1, 0, None);
    let attributes = crate::objects::parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V4,
        None,
        &mut Vec::new(),
    )
    .expect("required invariant");
    let mut duplicate = attributes.clone();
    duplicate.object_id = Uuid::from_wire([1; 16]);
    let mut duplicate_again = duplicate.clone();
    duplicate_again.object_id = duplicate.object_id;
    let mut objects = vec![
        descriptor(attributes, 10),
        descriptor(duplicate, 20),
        descriptor(duplicate_again, 30),
    ];
    objects[0].class_uuid = Uuid::from_wire([9; 16]);
    let mut warnings = Vec::new();
    crate::objects::resolve_identities(
        &mut objects,
        &settings::DocumentMetadata::default(),
        &mut warnings,
    );
    assert_ne!(
        objects[0]
            .identity
            .as_ref()
            .expect("required invariant")
            .source_id,
        objects[2]
            .identity
            .as_ref()
            .expect("required invariant")
            .source_id
    );
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("nil object UUID")));
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("duplicate object UUID")));
    assert_eq!(
        objects[0]
            .identity
            .as_ref()
            .expect("required invariant")
            .class_uuid,
        Uuid::from_wire([9; 16])
    );
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
    let valid = long_chunk(ArchiveVersion::V4, 0x0002_7ffd, &valid_body);
    malformed.extend(valid);
    let mut warnings = Vec::new();
    let descriptors = crate::objects::parse_attribute_userdata(
        &malformed,
        0..malformed.len(),
        ArchiveVersion::V4,
        &mut warnings,
    );
    assert_eq!(descriptors.len(), 1);
    assert!(descriptors[0].known);
    assert!(descriptors[0].range.start > 0);
    assert!(!warnings.is_empty());
}

#[test]
fn uuid_list_uses_an_anonymous_versioned_chunk() {
    let archive = ArchiveVersion::V5;
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend([0x11; 16]);
    let bytes = anonymous_chunk(archive, 0, &body);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded UUID-list reader");
    let values = crate::objects::read_uuid_list(&mut reader, archive).expect("UUID list");
    assert_eq!(values.len(), 1);
    assert_eq!(reader.remaining(), 0);
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
    assert_eq!(scan.objects[0].unknown_trailer.len(), 1);
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
        assert!(scan.objects[0].framing_degraded);
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
    assert!(scan.objects[0].checksum_warnings.is_empty());
    assert!(scan.objects[1].checksum_warnings.is_empty());
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
    scan.objects[0].attributes_degraded = true;
    crate::decode::with_expand(&scan, |expand| {
        let mut context = crate::decode::DecodeContext::new(&scan, expand);
        assert!(context.mark_decoded(0));
        let result = context.commit();
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
    let offset = scan.objects[0].range.start as u64;
    let class = scan.objects[0].class_uuid.to_string();
    let result = crate::decode::decode_for_test(&scan);

    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code.category() == cadmpeg_ir::report::LossCategory::Geometry
                && loss.provenance.is_some()
        })
        .and_then(|loss| loss.provenance.as_ref())
        .expect("retained geometry loss has provenance");
    let expected_tag = format!("OBJECT_RECORD/class={class}/type=0x00000001");
    assert_eq!(loss.format, "rhino");
    assert_eq!(loss.stream, "");
    assert_eq!(loss.offset, offset);
    assert_eq!(loss.tag.as_deref(), Some(expected_tag.as_str()));
    assert!(!result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code != crate::loss::RhinoLossCode::IntegrityFailure.kind())
        .any(|loss| { loss.message.contains("OBJECT_RECORD") || loss.message.contains("offset") }));
}
