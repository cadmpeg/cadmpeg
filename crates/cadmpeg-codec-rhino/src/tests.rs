// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecEntry, CodecError, Confidence, DecodeOptions};
use cadmpeg_ir::decode::InspectOptions;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::LossCode;
use cadmpeg_ir::IR_VERSION;

use super::chunks::{
    anonymous_version, checked_count_bytes, chunk_at, crc16, packed_version, parse_eof,
    parse_header, verify_checksum, ArchiveVersion, BoundedReader, ChecksumStatus, FramingError,
    TCODE_CLASS_UUID, TCODE_CRC, TCODE_ENDOFFILE, TCODE_SHORT,
};
use super::settings;
use super::wire::Uuid;
use super::{RhinoCodec, MAGIC};

fn header(version: &str) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    let mut field = [b' '; 8];
    let start = 8 - version.len();
    field[start..].copy_from_slice(version.as_bytes());
    bytes.extend(field);
    bytes
}

fn long_chunk(archive: ArchiveVersion, typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    if archive.uses_eight_byte_values() {
        bytes.extend((body.len() as i64).to_le_bytes());
    } else {
        bytes.extend((body.len() as i32).to_le_bytes());
    }
    bytes.extend(body);
    bytes
}

fn crc_chunk(archive: ArchiveVersion, typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut payload = body.to_vec();
    payload.extend(crc32fast::hash(body).to_le_bytes());
    long_chunk(archive, typecode, &payload)
}

fn eof(archive: ArchiveVersion, file_size: usize) -> Vec<u8> {
    long_chunk(
        archive,
        TCODE_ENDOFFILE,
        &if archive.uses_eight_byte_values() {
            (file_size as u64).to_le_bytes().to_vec()
        } else {
            (file_size as u32).to_le_bytes().to_vec()
        },
    )
}

fn uuid_bytes() -> Vec<u8> {
    vec![0; 16]
}

fn utf16_bytes(value: &str) -> Vec<u8> {
    let mut units: Vec<u16> = value.encode_utf16().collect();
    units.push(0);
    let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
    for unit in units {
        bytes.extend(unit.to_le_bytes());
    }
    bytes
}

fn fixed_attributes(minor: u8, mode: u8, visible: Option<bool>) -> Vec<u8> {
    let mut bytes = vec![(0x10 | minor)];
    bytes.extend(uuid_bytes());
    bytes.extend((-1_i32).to_le_bytes());
    bytes.extend((-1_i32).to_le_bytes());
    bytes.extend([1, 2, 3, 4]);
    bytes.extend(0_i16.to_le_bytes());
    bytes.extend(0_i16.to_le_bytes());
    bytes.extend(0.0_f64.to_le_bytes());
    bytes.extend(1.0_f64.to_le_bytes());
    bytes.extend(1_i32.to_le_bytes());
    bytes.extend([mode, 0, 0, 0]);
    bytes.extend(utf16_bytes("name"));
    bytes.extend(utf16_bytes("https://example.test"));
    if minor >= 1 {
        bytes.extend(0_i32.to_le_bytes());
    }
    if minor >= 2 {
        bytes.push(u8::from(visible.unwrap_or(true)));
    }
    if minor >= 3 {
        bytes.extend(0_i32.to_le_bytes());
    }
    if minor >= 4 {
        bytes.extend(7_i32.to_le_bytes());
        bytes.push(0);
        bytes.extend([9, 8, 7, 6]);
        bytes.push(0);
        bytes.extend(0.25_f64.to_le_bytes());
    }
    if minor >= 5 {
        bytes.extend(4_i32.to_le_bytes());
    }
    if minor >= 6 {
        bytes.push(1);
        bytes.extend(0_i32.to_le_bytes());
    }
    if minor >= 7 {
        let rendering = crc_chunk(
            ArchiveVersion::V4,
            0x4000_8000,
            &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        bytes.extend(rendering);
    }
    bytes
}

#[test]
fn detects_existing_magic_forms() {
    assert_eq!(RhinoCodec.detect(MAGIC), Confidence::High);
    assert_eq!(RhinoCodec.detect(&MAGIC[..MAGIC.len() - 1]), Confidence::No);
    let mut incorrect = MAGIC.to_vec();
    incorrect[3] = b'X';
    assert_eq!(RhinoCodec.detect(&incorrect), Confidence::No);
    let mut prefix = vec![0x00, 0x01, 0x02, 0x03];
    prefix.extend_from_slice(MAGIC);
    prefix.extend_from_slice(&[0x04, 0x05]);
    assert_eq!(RhinoCodec.detect(&prefix), Confidence::High);
}

#[test]
fn parses_exact_header_and_scope() {
    for (text, expected) in [
        ("1", ArchiveVersion::V1),
        ("2", ArchiveVersion::V2),
        ("3", ArchiveVersion::V3),
        ("4", ArchiveVersion::V4),
        ("5", ArchiveVersion::LegacyV5),
        ("50", ArchiveVersion::V5),
        ("60", ArchiveVersion::V6),
        ("70", ArchiveVersion::V7),
        ("80", ArchiveVersion::V8),
    ] {
        let parsed = parse_header(&header(text)).expect("valid header");
        assert_eq!(parsed.archive_version, expected);
    }
    assert!(parse_header(&header("0")).is_err());
    let mut invalid = header("50");
    invalid[24] = b'0';
    assert!(matches!(
        parse_header(&invalid),
        Err(FramingError::InvalidHeader)
    ));
    invalid = header("50");
    invalid[31] = b' ';
    assert!(matches!(
        parse_header(&invalid),
        Err(FramingError::InvalidHeader)
    ));
    assert!(parse_header(&header("1234567")).is_ok());
    assert!(parse_header(&header("12345678")).is_ok());
}

#[test]
fn parses_widths_short_long_and_bounds() {
    let short = (TCODE_SHORT | 7).to_le_bytes();
    let mut bytes = short.to_vec();
    bytes.extend(42_i32.to_le_bytes());
    let parsed =
        chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V4, false).expect("required invariant");
    assert!(parsed.short);
    assert_eq!(parsed.value, 42);
    assert_eq!(parsed.next_offset, 8);

    let bytes = long_chunk(ArchiveVersion::V4, 9, &[1, 2, 3]);
    let parsed =
        chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V4, false).expect("required invariant");
    assert_eq!(parsed.body, 8..11);
    assert_eq!(parsed.header_start, 0);
    assert_eq!(parsed.range(), 0..11);
    assert_eq!(parsed.next_offset, 11);

    let bytes = long_chunk(ArchiveVersion::V5, 9, &[1, 2, 3]);
    let parsed =
        chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V5, false).expect("required invariant");
    assert_eq!(parsed.body, 12..15);
    assert_eq!(parsed.header_start, 0);
    assert_eq!(parsed.range(), 0..15);
    assert_eq!(parsed.next_offset, 15);

    let mut bad = 9_u32.to_le_bytes().to_vec();
    bad.extend((-1_i32).to_le_bytes());
    assert!(matches!(
        chunk_at(&bad, 0, bad.len(), ArchiveVersion::V4, false),
        Err(FramingError::InvalidLength { .. })
    ));
    let mut overflow = 9_u32.to_le_bytes().to_vec();
    overflow.extend(i32::MAX.to_le_bytes());
    assert!(matches!(
        chunk_at(&overflow, 0, overflow.len(), ArchiveVersion::V4, false),
        Err(FramingError::OutOfBounds { .. })
    ));
    assert!(chunk_at(&[9, 0, 0], 0, 3, ArchiveVersion::V4, false).is_err());
}

#[test]
fn verifies_crc_vectors_and_recoverable_mismatch() {
    assert_eq!(crc16(0, b""), 0);
    assert_eq!(crc16(1, b""), 1);
    assert_eq!(crc16(0, b"123456789"), 0x31c3);
    assert_eq!(crc32fast::hash(b""), 0);
    assert_eq!(crc32fast::hash(b"123456789"), 0xcbf4_3926);

    let body = b"body";
    let mut bytes = (TCODE_CRC | 9).to_le_bytes().to_vec();
    bytes.extend(((body.len() + 4) as i32).to_le_bytes());
    bytes.extend(body);
    bytes.extend(crc32fast::hash(body).to_le_bytes());
    let chunk =
        chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V2, false).expect("required invariant");
    assert_eq!(verify_checksum(&bytes, &chunk), Ok(ChecksumStatus::Valid));
    *bytes.last_mut().expect("required invariant") ^= 1;
    assert!(matches!(
        verify_checksum(&bytes, &chunk),
        Ok(ChecksumStatus::Mismatch { .. })
    ));

    assert_eq!(
        super::chunks::checksum_kind(ArchiveVersion::V1, 0x0001_0000, false),
        super::chunks::ChecksumKind::Crc16
    );
    assert_eq!(
        super::chunks::checksum_kind(ArchiveVersion::V1, TCODE_CLASS_UUID, true),
        super::chunks::ChecksumKind::Crc16
    );
}

#[test]
fn parses_mixed_endian_uuid_and_nil_uuid() {
    let uuid = Uuid::from_wire([
        0xdd, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ]);
    assert_eq!(uuid.to_string(), "4ed7d4dd-e947-11d3-bfe5-0010830122f0");
    assert!(!uuid.is_nil());
    assert!(Uuid::nil().is_nil());
    assert_eq!(
        Uuid::nil().to_string(),
        "00000000-0000-0000-0000-000000000000"
    );
}

#[test]
fn parses_fixed_attributes_through_every_minor_gate() {
    for minor in 0..=8 {
        let bytes = fixed_attributes(minor, 0, Some(true));
        let parsed = super::objects::parse_attributes(
            &bytes,
            0..bytes.len(),
            100..100 + bytes.len(),
            ArchiveVersion::V4,
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
    let hidden = fixed_attributes(1, 0x12, None);
    let hidden = super::objects::parse_attributes(
        &hidden,
        0..hidden.len(),
        0..hidden.len(),
        ArchiveVersion::V4,
        &mut Vec::new(),
    )
    .expect("required invariant");
    assert!(!hidden.visible);

    let definition = fixed_attributes(1, 0xf3, None);
    let definition = super::objects::parse_attributes(
        &definition,
        0..definition.len(),
        0..definition.len(),
        ArchiveVersion::V4,
        &mut Vec::new(),
    )
    .expect("required invariant");
    assert_eq!(definition.object_mode & 0x0f, 3);
}

#[test]
fn fixed_explicit_visibility_overrides_hidden_mode_default() {
    let bytes = fixed_attributes(2, 0x02, Some(true));
    let parsed = super::objects::parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V4,
        &mut Vec::new(),
    )
    .expect("required invariant");
    assert!(parsed.visible);
}

fn tagged_attributes(items: &[(u8, Vec<u8>)], minor: u8) -> Vec<u8> {
    let mut bytes = vec![0x20 | minor];
    bytes.extend(uuid_bytes());
    bytes.extend((-1_i32).to_le_bytes());
    for (item, payload) in items {
        bytes.push(*item);
        bytes.extend(payload);
    }
    bytes.push(0);
    bytes
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
        (28, vec![0, 0, 0, 0, 0]),
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
        let mut decoded_at_gate = super::objects::parse_attributes(
            &minimum,
            0..minimum.len(),
            0..minimum.len(),
            ArchiveVersion::V8,
            &mut Vec::new(),
        )
        .unwrap_or_else(|error| panic!("item {item} failed at minor {gate}: {error}"));
        let latest = tagged_attributes(&[(*item, payload.clone())], 13);
        let decoded_at_latest = super::objects::parse_attributes(
            &latest,
            0..latest.len(),
            0..latest.len(),
            ArchiveVersion::V8,
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
                super::objects::parse_attributes(
                    &preceding,
                    0..preceding.len(),
                    0..preceding.len(),
                    ArchiveVersion::V8,
                    &mut Vec::new(),
                )
                .is_err(),
                "item {item} was accepted before minor {gate}"
            );
        }
    }
    let bytes = tagged_attributes(&items, 13);
    let parsed = super::objects::parse_attributes(
        &bytes,
        0..bytes.len(),
        10..10 + bytes.len(),
        ArchiveVersion::V8,
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
            super::objects::parse_attributes(
                &bytes,
                0..bytes.len(),
                0..bytes.len(),
                ArchiveVersion::V8,
                &mut Vec::new()
            )
            .is_err(),
            "minor {minor} item {item}"
        );
    }
    let mut bytes = tagged_attributes(&[(1, utf16_bytes("N"))], 0);
    bytes.pop();
    assert!(super::objects::parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        &mut Vec::new()
    )
    .is_err());
    let bytes = tagged_attributes(&[(42, vec![])], 13);
    assert!(super::objects::parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        &mut Vec::new()
    )
    .is_err());
}

#[test]
fn tagged_attributes_reject_nonfinite_numeric_items() {
    let bytes = tagged_attributes(&[(8, f64::NAN.to_le_bytes().to_vec())], 0);
    assert!(super::objects::parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V8,
        &mut Vec::new()
    )
    .is_err());
}

fn descriptor(
    attributes: super::objects::ObjectAttributes,
    offset: usize,
) -> super::objects::ObjectDescriptor {
    super::objects::ObjectDescriptor {
        range: offset..offset + 10,
        object_type: 0,
        class_uuid: Uuid::nil(),
        class_data_range: offset..offset,
        framing_degraded: false,
        attributes: Some(attributes),
        attributes_degraded: false,
        attributes_userdata: Vec::new(),
        identity: None,
        userdata: Vec::new(),
        attributes_range: None,
        attributes_body_range: None,
        attributes_userdata_range: None,
        attributes_userdata_body_range: None,
        history: None,
        unknown_trailer: Vec::new(),
        checksum_warnings: Vec::new(),
        warnings: Vec::new(),
    }
}

#[test]
fn identity_resolution_defers_material_and_parent_colors() {
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
    let mut metadata = settings::DocumentMetadata::default();
    metadata.layers.push(layer);
    let mut attributes = super::objects::parse_attributes(
        &fixed_attributes(1, 0, None),
        0..fixed_attributes(1, 0, None).len(),
        0..fixed_attributes(1, 0, None).len(),
        ArchiveVersion::V4,
        &mut Vec::new(),
    )
    .expect("required invariant");
    attributes.layer_index = -1;
    attributes.color_source = 2;
    let mut material = vec![descriptor(attributes.clone(), 10)];
    let mut warnings = Vec::new();
    super::objects::resolve_identities(&mut material, &metadata, &mut warnings);
    assert_eq!(
        material[0]
            .identity
            .as_ref()
            .expect("required invariant")
            .effective_color,
        None
    );

    attributes.color_source = 3;
    attributes.object_mode = 0xf3;
    let mut parent = vec![descriptor(attributes, 20)];
    super::objects::resolve_identities(&mut parent, &metadata, &mut warnings);
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
    let attributes = super::objects::parse_attributes(
        &bytes,
        0..bytes.len(),
        0..bytes.len(),
        ArchiveVersion::V4,
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
    super::objects::resolve_identities(
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
fn attribute_userdata_recovers_after_malformed_bounded_record() {
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
    let descriptors = super::objects::parse_attribute_userdata(
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
fn keeps_packed_and_anonymous_versions_distinct() {
    assert_eq!(packed_version(0x21), (2, 1));
    let bytes = [2, 0, 0, 0, 1, 0, 0, 0];
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    assert_eq!(
        anonymous_version(&mut reader).expect("required invariant"),
        (2, 1)
    );
}

#[test]
fn validates_eof_width_size_and_truncation() {
    for archive in [ArchiveVersion::V4, ArchiveVersion::V5] {
        let mut bytes = vec![0; 32];
        let marker = eof(
            archive,
            32 + 12
                + if archive.uses_eight_byte_values() {
                    16
                } else {
                    8
                },
        );
        bytes.extend(marker);
        let size = bytes.len();
        let marker_start = 32;
        let replacement = eof(archive, size);
        bytes[marker_start..].copy_from_slice(&replacement);
        assert_eq!(
            parse_eof(&bytes, marker_start, archive)
                .expect("required invariant")
                .expect("required invariant")
                .file_size,
            size as u64
        );
        let mut mismatch = bytes.clone();
        let size_offset = marker_start
            + if archive.uses_eight_byte_values() {
                12
            } else {
                8
            };
        mismatch[size_offset] ^= 1;
        assert!(matches!(
            parse_eof(&mismatch, marker_start, archive),
            Err(FramingError::FileSizeMismatch { .. })
        ));
        assert!(parse_eof(&bytes[..bytes.len() - 1], marker_start, archive).is_err());
    }
    let bytes = vec![0; 32];
    assert_eq!(
        parse_eof(&bytes, 32, ArchiveVersion::V1).expect("required invariant"),
        None
    );
    assert!(matches!(
        parse_eof(&bytes, 32, ArchiveVersion::V2),
        Err(FramingError::MissingEof)
    ));
}

#[test]
fn nested_bounds_and_unknown_skip_are_exact() {
    let child = long_chunk(ArchiveVersion::V5, 0x1234, &[9, 8, 7]);
    let sibling = long_chunk(ArchiveVersion::V5, 0x2345, &[1]);
    let mut parent = long_chunk(ArchiveVersion::V5, 0x1000, &child);
    parent.extend(sibling);
    let first =
        chunk_at(&parent, 0, parent.len(), ArchiveVersion::V5, false).expect("required invariant");
    let nested = chunk_at(
        &parent,
        first.body.start,
        first.body.end,
        ArchiveVersion::V5,
        false,
    )
    .expect("required invariant");
    assert_eq!(nested.next_offset, first.body.start + child.len());
    let next = chunk_at(
        &parent,
        first.next_offset,
        parent.len(),
        ArchiveVersion::V5,
        false,
    )
    .expect("required invariant");
    assert_eq!(next.typecode, 0x2345);
    assert!(matches!(
        chunk_at(
            &parent,
            first.body.start,
            first.body.start + child.len() - 1,
            ArchiveVersion::V5,
            false
        ),
        Err(FramingError::OutOfBounds { .. })
    ));
}

#[test]
fn checked_counts_never_allocate_from_invalid_values() {
    assert_eq!(
        checked_count_bytes(3, 4, 12, 100, 0).expect("required invariant"),
        12
    );
    assert!(checked_count_bytes(-1, 4, 12, 100, 0).is_err());
    assert!(checked_count_bytes(4, 4, 12, 100, 0).is_err());
    assert!(checked_count_bytes(3, 4, 12, 2, 0).is_err());
}

#[test]
fn bounded_reader_fixed_arrays_preserve_absolute_cursor_bounds() {
    let bytes = [9, 8, 7, 6, 5];
    let mut reader = BoundedReader::new(&bytes, 1, 5).expect("required invariant");
    assert_eq!(reader.array::<3>().expect("required invariant"), [8, 7, 6]);
    assert_eq!(reader.position(), 4);
    assert!(reader.array::<2>().is_err());
    assert_eq!(reader.position(), 4);
}

fn short_chunk(archive: ArchiveVersion, typecode: u32, value: i64) -> Vec<u8> {
    let mut bytes = (typecode | TCODE_SHORT).to_le_bytes().to_vec();
    if archive.uses_eight_byte_values() {
        bytes.extend(value.to_le_bytes());
    } else {
        bytes.extend((value as i32).to_le_bytes());
    }
    bytes
}

fn table(archive: ArchiveVersion, typecode: u32, records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = records.concat();
    body.extend(short_chunk(archive, super::chunks::TCODE_ENDOFTABLE, 0));
    long_chunk(archive, typecode, &body)
}

const INSTANCE_DEFINITION_CLASS: [u8; 16] = [
    0xf6, 0xbf, 0xf8, 0x26, 0x18, 0x26, 0x7f, 0x41, 0xa1, 0x58, 0x15, 0x3d, 0x64, 0xa9, 0x49, 0x89,
];
const INSTANCE_REFERENCE_CLASS: [u8; 16] = [
    0x38, 0xb6, 0xcf, 0xf9, 0xd4, 0xb9, 0x40, 0x43, 0x87, 0xe3, 0xc5, 0x6e, 0x78, 0x65, 0xd9, 0x6a,
];
const POINT_CLASS: [u8; 16] = [
    0x1d, 0x1a, 0x10, 0xc3, 0x57, 0xf1, 0xd3, 0x11, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const NURBS_CURVE_CLASS: [u8; 16] = [
    0xdd, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const ARC_CURVE_CLASS: [u8; 16] = [
    0x2a, 0xbe, 0x33, 0xcf, 0xb4, 0x09, 0xd4, 0x11, 0xbf, 0xfb, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const MESH_CLASS: [u8; 16] = [
    0xe4, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const SUBD_CLASS: [u8; 16] = [
    0xd9, 0xa4, 0x9b, 0xf0, 0x5b, 0x45, 0xc3, 0x42, 0xba, 0x3b, 0xe6, 0xcc, 0xac, 0xef, 0x85, 0x3b,
];
const REV_SURFACE_CLASS: [u8; 16] = [
    0xd3, 0x20, 0x62, 0xa1, 0x3b, 0x16, 0xd4, 0x11, 0x80, 0x00, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];

fn anonymous_chunk(archive: ArchiveVersion, minor: i32, body: &[u8]) -> Vec<u8> {
    let mut payload = 1_i32.to_le_bytes().to_vec();
    payload.extend(minor.to_le_bytes());
    payload.extend(body);
    crc_chunk(archive, 0x4000_8000, &payload)
}

fn unit_detail(archive: ArchiveVersion, unit: u32, meters_per_unit: f64) -> Vec<u8> {
    let mut body = unit.to_le_bytes().to_vec();
    body.extend(meters_per_unit.to_le_bytes());
    body.extend(utf16_bytes(""));
    anonymous_chunk(archive, 0, &body)
}

fn content_hash(archive: ArchiveVersion) -> Vec<u8> {
    let mut body = 123_u64.to_le_bytes().to_vec();
    body.extend(456_u64.to_le_bytes());
    body.extend(789_u64.to_le_bytes());
    body.extend(anonymous_chunk(archive, 0, &[0x11; 20]));
    body.extend(anonymous_chunk(archive, 0, &[0x22; 20]));
    anonymous_chunk(archive, 0, &body)
}

fn file_reference(archive: ArchiveVersion, full: &str, relative: &str) -> Vec<u8> {
    let mut body = utf16_bytes(full);
    body.extend(utf16_bytes(relative));
    body.extend(content_hash(archive));
    body.extend(7_u32.to_le_bytes());
    body.extend([0x44; 16]);
    anonymous_chunk(archive, 1, &body)
}

fn model_component_attributes(
    archive: ArchiveVersion,
    id: [u8; 16],
    index: i32,
    name: &str,
) -> Vec<u8> {
    let mut body = vec![1];
    body.extend(11_u32.to_le_bytes());
    body.extend(12_u32.to_le_bytes());
    body.extend(13_u32.to_le_bytes());
    body.push(1);
    body.extend(id);
    body.push(2);
    body.push(1);
    body.extend(index.to_le_bytes());
    body.push(1);
    body.extend(utf16_bytes(name));
    let mut payload = 1_i32.to_le_bytes().to_vec();
    payload.extend(0_i32.to_le_bytes());
    payload.extend(body);
    crc_chunk(archive, 0x4000_8002, &payload)
}

fn reference_settings(archive: ArchiveVersion) -> Vec<u8> {
    let mut body = 0_i32.to_le_bytes().to_vec();
    body.extend(0_i32.to_le_bytes());
    body.push(0);
    anonymous_chunk(archive, 0, &body)
}

fn definition_record(archive: ArchiveVersion, payload: &[u8]) -> Vec<u8> {
    let mut uuid_body = INSTANCE_DEFINITION_CLASS.to_vec();
    uuid_body.extend(crc32fast::hash(&INSTANCE_DEFINITION_CLASS).to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, payload);
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    let class = long_chunk(
        archive,
        0x0002_7ffa,
        &[uuid, class_data, class_end].concat(),
    );
    crc_chunk(archive, 0x2000_8076, &class)
}

fn v5_definition_payload(
    archive: ArchiveVersion,
    minor: u8,
    id: [u8; 16],
    members: &[[u8; 16]],
    linked: bool,
) -> Vec<u8> {
    let mut payload = vec![0x10 | minor];
    payload.extend(id);
    payload.extend((members.len() as i32).to_le_bytes());
    for member in members {
        payload.extend(member);
    }
    payload.extend(utf16_bytes("v5 definition"));
    payload.extend(utf16_bytes("description"));
    payload.extend(utf16_bytes("https://example.test"));
    payload.extend(utf16_bytes("tag"));
    for value in [0.0_f64, 0.0, 0.0, 1.0, 2.0, 3.0] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(if linked { 3_u32 } else { 0_u32 }.to_le_bytes());
    payload.extend(utf16_bytes(if linked { "/full/source.3dm" } else { "" }));
    payload.extend(123_u64.to_le_bytes());
    payload.extend(456_u64.to_le_bytes());
    for value in 0_u32..8 {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(2_u32.to_le_bytes());
    payload.extend(0.001_f64.to_le_bytes());
    payload.push(0);
    payload.extend(unit_detail(archive, 2, 0.001));
    payload.extend(1_i32.to_le_bytes());
    payload.extend(0_u32.to_le_bytes());
    if minor >= 7 {
        payload.push(u8::from(linked));
        if linked {
            payload.extend(file_reference(archive, "/full/source.3dm", "source.3dm"));
        }
        payload.push(0);
    }
    payload
}

fn v6_definition_payload(
    archive: ArchiveVersion,
    id: [u8; 16],
    members: &[[u8; 16]],
    kind: u32,
    linked: bool,
    settings: bool,
) -> Vec<u8> {
    let mut body = model_component_attributes(archive, id, 17, "modern definition");
    body.extend(kind.to_le_bytes());
    body.extend(unit_detail(archive, 8, 0.0254));
    body.extend(utf16_bytes("description"));
    body.extend(utf16_bytes("https://example.test"));
    body.extend(utf16_bytes("tag"));
    for value in [0.0_f64, 0.0, 0.0, 4.0, 5.0, 6.0] {
        body.extend(value.to_le_bytes());
    }
    let members_present = kind != 3;
    body.push(u8::from(members_present));
    if members_present {
        body.extend((members.len() as i32).to_le_bytes());
        for member in members {
            body.extend(member);
        }
    }
    body.push(u8::from(linked));
    if linked {
        let mut linked_body = file_reference(archive, "/full/source.3dm", "source.3dm");
        linked_body.extend(2_i32.to_le_bytes());
        linked_body.extend(2_u32.to_le_bytes());
        linked_body.push(u8::from(settings));
        if settings {
            linked_body.extend(reference_settings(archive));
        }
        body.extend(anonymous_chunk(archive, 0, &linked_body));
    }
    anonymous_chunk(archive, 0, &body)
}

fn document_with_definitions(
    version: &str,
    archive: ArchiveVersion,
    definitions: &[Vec<u8>],
    objects: &[Vec<u8>],
) -> Vec<u8> {
    minimal_document(
        version,
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0021, definitions),
            table(archive, 0x1000_0013, objects),
        ],
    )
}

#[test]
fn parses_source_shaped_v5_minor_6_and_7_definition_records() {
    let definition_id = [0x10; 16];
    let member_id = [0x20; 16];
    let v5 = definition_record(
        ArchiveVersion::V5,
        &v5_definition_payload(ArchiveVersion::V5, 6, definition_id, &[member_id], true),
    );
    let scan = super::container::scan_owned(document_with_definitions(
        "50",
        ArchiveVersion::V5,
        &[v5],
        &[],
    ))
    .expect("required invariant");
    let parsed = &scan.definitions.definitions[0];
    assert_eq!(parsed.kind, super::instances::DefinitionKind::Linked);
    assert_eq!(parsed.members, vec![Uuid::from_wire(member_id)]);
    assert_eq!(parsed.units.unit, 2);
    assert_eq!(parsed.units.meters_per_unit, 0.001);
    assert_eq!(parsed.linked_appearance, 2);
    assert_eq!(
        parsed
            .legacy_checksum_range
            .as_ref()
            .expect("required invariant")
            .len(),
        48
    );
    assert!(parsed.file_reference_range.is_none());

    let v6 = definition_record(
        ArchiveVersion::V6,
        &v5_definition_payload(ArchiveVersion::V6, 7, definition_id, &[member_id], true),
    );
    let scan = super::container::scan_owned(document_with_definitions(
        "60",
        ArchiveVersion::V6,
        &[v6],
        &[],
    ))
    .expect("required invariant");
    let parsed = &scan.definitions.definitions[0];
    assert!(parsed.file_reference_range.is_some());
}

#[test]
fn parses_source_shaped_v6_v7_v8_static_and_linked_definitions() {
    for (version, archive) in [
        ("60", ArchiveVersion::V6),
        ("70", ArchiveVersion::V7),
        ("80", ArchiveVersion::V8),
    ] {
        let definition_id = [archive.value() as u8; 16];
        let member_id = [archive.value() as u8 + 1; 16];
        let static_record = definition_record(
            archive,
            &v6_definition_payload(archive, definition_id, &[member_id], 1, false, false),
        );
        let linked_record = definition_record(
            archive,
            &v6_definition_payload(archive, [0x70; 16], &[], 3, true, true),
        );
        let embedded_record = definition_record(
            archive,
            &v6_definition_payload(archive, [0x71; 16], &[], 2, true, false),
        );
        let unset_record = definition_record(
            archive,
            &v6_definition_payload(archive, [0x72; 16], &[], 0, false, false),
        );
        let scan = super::container::scan_owned(document_with_definitions(
            version,
            archive,
            &[static_record, linked_record, embedded_record, unset_record],
            &[],
        ))
        .expect("required invariant");
        assert_eq!(scan.definitions.definitions.len(), 4);
        let static_definition = &scan.definitions.definitions[0];
        assert_eq!(
            static_definition.kind,
            super::instances::DefinitionKind::Static
        );
        assert_eq!(static_definition.index, Some(17));
        assert_eq!(static_definition.name, "modern definition");
        assert_eq!(static_definition.members, vec![Uuid::from_wire(member_id)]);
        assert_eq!(static_definition.units.unit, 8);
        assert_eq!(static_definition.units.meters_per_unit, 0.0254);
        let linked = &scan.definitions.definitions[1];
        assert_eq!(linked.kind, super::instances::DefinitionKind::Linked);
        assert!(linked.members.is_empty());
        assert_eq!(linked.linked_depth, 2);
        assert_eq!(linked.linked_appearance, 2);
        assert!(linked.reference_settings_range.is_some());
        assert!(linked.file_reference_range.is_some());
        assert_eq!(
            scan.definitions.definitions[2].kind,
            super::instances::DefinitionKind::LinkedAndEmbedded
        );
        assert_eq!(
            scan.definitions.definitions[3].kind,
            super::instances::DefinitionKind::Unset
        );
    }
}

#[test]
fn definition_scan_recovers_after_malformed_record_and_preserves_membership_union() {
    let archive = ArchiveVersion::V7;
    let duplicate_id = [0x31; 16];
    let first_member = [0x41; 16];
    let second_member = [0x42; 16];
    let malformed_member = [0x43; 16];
    let ordinary_member = [0x44; 16];
    let first = definition_record(
        archive,
        &v6_definition_payload(archive, duplicate_id, &[first_member], 1, false, false),
    );
    let second = definition_record(
        archive,
        &v6_definition_payload(archive, duplicate_id, &[second_member], 1, false, false),
    );
    let mut malformed_payload =
        v6_definition_payload(archive, [0x32; 16], &[malformed_member], 2, true, false);
    let invalid_settings_flag = malformed_payload.len() - 9;
    malformed_payload[invalid_settings_flag] = 2;
    let malformed = definition_record(archive, &malformed_payload);
    let later = definition_record(
        archive,
        &v6_definition_payload(archive, [0x33; 16], &[], 1, false, false),
    );
    let objects = [
        first_member,
        second_member,
        malformed_member,
        ordinary_member,
    ]
    .map(|_| object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 0.0, 0.0])));
    let mut scan = super::container::scan_owned(document_with_definitions(
        "70",
        archive,
        &[first, second, malformed, later],
        &objects,
    ))
    .expect("required invariant");
    assert!(scan
        .definitions
        .ambiguous_ids
        .contains(&Uuid::from_wire(duplicate_id)));
    for member in [first_member, second_member, malformed_member] {
        assert!(scan
            .definitions
            .member_object_ids
            .contains(&Uuid::from_wire(member)));
    }
    assert_eq!(scan.definitions.definitions.len(), 1);
    assert!(scan.definitions.diagnostics.len() >= 2);
    assert!(scan.definitions.diagnostics.iter().all(|diagnostic| {
        diagnostic.source_range.start < diagnostic.source_range.end
            && !diagnostic.message.contains("unsupported class")
    }));
    let container_only = super::container::container_only_result(&scan);
    assert!(container_only.report.losses.iter().any(|loss| {
        loss.severity == Severity::Warning
            && loss
                .provenance
                .as_ref()
                .and_then(|value| value.tag.as_deref())
                == Some("INSTANCE_DEFINITION_TABLE")
    }));
    for (source_order, id) in [
        first_member,
        second_member,
        malformed_member,
        ordinary_member,
    ]
    .into_iter()
    .enumerate()
    {
        set_identity(
            &mut scan,
            source_order,
            id,
            &format!("definition-member-{source_order}"),
            None,
            true,
        );
    }
    set_test_units(&mut scan, 1.0);
    let result = super::decode::decode_for_test(&scan);
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert!(result.ir.model.bodies[0]
        .id
        .to_string()
        .contains("definition-member-3"));
    let definition_loss = result
        .report
        .losses
        .iter()
        .find(|loss| loss.message.contains("instance-definition record"))
        .expect("aggregated definition diagnostic");
    assert_eq!(
        definition_loss
            .provenance
            .as_ref()
            .and_then(|value| value.tag.as_deref()),
        Some("INSTANCE_DEFINITION_TABLE")
    );
}

fn crc_table(archive: ArchiveVersion, typecode: u32, records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = records.concat();
    body.extend(short_chunk(archive, super::chunks::TCODE_ENDOFTABLE, 0));
    crc_chunk(archive, typecode | TCODE_CRC, &body)
}

fn object_record(archive: ArchiveVersion, object_type: i64, class_uuid: [u8; 16]) -> Vec<u8> {
    object_record_with_payload(archive, object_type, class_uuid, &[])
}

fn object_record_with_payload(
    archive: ArchiveVersion,
    object_type: i64,
    class_uuid: [u8; 16],
    payload: &[u8],
) -> Vec<u8> {
    let object_type = short_chunk(archive, 0x8200_0071, object_type);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, payload);
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    let class = long_chunk(
        archive,
        0x0002_7ffa,
        &[uuid, class_data, class_end].concat(),
    );
    let object_end = short_chunk(archive, 0x8200_007f, 0);
    crc_chunk(
        archive,
        0x2000_8070 | TCODE_CRC,
        &[object_type, class, object_end].concat(),
    )
}

fn class_wrapper(archive: ArchiveVersion, class_uuid: [u8; 16], payload: &[u8]) -> Vec<u8> {
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, payload);
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    long_chunk(
        archive,
        0x0002_7ffa,
        &[uuid, class_data, class_end].concat(),
    )
}

fn object_record_without_end(
    archive: ArchiveVersion,
    object_type: i64,
    class_uuid: [u8; 16],
) -> Vec<u8> {
    let object_type = short_chunk(archive, 0x8200_0071, object_type);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, &[]);
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    let class = long_chunk(
        archive,
        0x0002_7ffa,
        &[uuid, class_data, class_end].concat(),
    );
    crc_chunk(
        archive,
        0x2000_8070 | TCODE_CRC,
        &[object_type, class].concat(),
    )
}

fn object_record_with_unknown_trailer(archive: ArchiveVersion, class_uuid: [u8; 16]) -> Vec<u8> {
    let object_type = short_chunk(archive, 0x8200_0071, 1);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, &[]);
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    let class = long_chunk(
        archive,
        0x0002_7ffa,
        &[uuid, class_data, class_end].concat(),
    );
    let unknown = long_chunk(archive, 0x0200_1000, &[1, 2, 3]);
    let object_end = short_chunk(archive, 0x8200_007f, 0);
    crc_chunk(
        archive,
        0x2000_8070 | TCODE_CRC,
        &[object_type, class, unknown, object_end].concat(),
    )
}

fn minimal_document(version: &str, tables: &[Vec<u8>]) -> Vec<u8> {
    let archive = parse_header(&header(version))
        .expect("required invariant")
        .archive_version;
    let mut bytes = header(version);
    bytes.extend(long_chunk(archive, 1, b"comment"));
    for table in tables {
        bytes.extend(table);
    }
    let eof_offset = bytes.len();
    bytes.extend(eof(archive, 0));
    let marker = eof(archive, bytes.len());
    bytes[eof_offset..].copy_from_slice(&marker);
    bytes
}

#[test]
fn document_table_record_budget_rejects_compact_record_amplification() {
    let archive = ArchiveVersion::V5;
    let records = [
        long_chunk(archive, 0x7000_0001, &[]),
        long_chunk(archive, 0x7000_0002, &[]),
    ];
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
            table(archive, 0x1000_0017, &records),
        ],
    );
    let error = super::container::scan_with_test_record_limit(bytes, 1)
        .expect_err("record budget must fail before descriptor amplification");
    assert!(error.to_string().contains("table record budget"));
}

#[test]
fn scan_retains_history_record_source_boundaries() {
    let archive = ArchiveVersion::V5;
    let history_record = crc_chunk(archive, 0x2000_807b, &[1, 2, 3, 4]);
    let bytes = minimal_document(
        "50",
        &[
            crc_table(archive, 0x1000_0014, &[]),
            crc_table(archive, 0x1000_0015, &[]),
            crc_table(archive, 0x1000_0013, &[]),
            crc_table(archive, 0x1000_0026, &[history_record]),
        ],
    );

    let scan = super::container::scan_owned(bytes).expect("history table");
    let history = scan
        .tables
        .iter()
        .find(|table| table.typecode & !TCODE_CRC == 0x1000_0026)
        .expect("history table descriptor");
    assert_eq!(history.records.len(), 1);
    assert_eq!(history.records[0].typecode, 0x2000_807b);
    assert_eq!(&scan.data[history.records[0].body.clone()], &[1, 2, 3, 4]);
}

#[test]
fn scan_decodes_history_identity_dependencies_and_typed_values() {
    let archive = ArchiveVersion::V5;
    let record_id = [1, 0, 0, 0, 2, 0, 3, 0, 4, 5, 6, 7, 8, 9, 10, 11];
    let command_id = [12, 0, 0, 0, 13, 0, 14, 0, 15, 16, 17, 18, 19, 20, 21, 22];
    let descendant = [23, 0, 0, 0, 24, 0, 25, 0, 26, 27, 28, 29, 30, 31, 32, 33];
    let antecedent = [34, 0, 0, 0, 35, 0, 36, 0, 37, 38, 39, 40, 41, 42, 43, 44];
    let uuid_list = |uuid: [u8; 16]| {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(uuid);
        anonymous_chunk(archive, 0, &body)
    };
    let value = |kind: i32, id: i32, payload: &[u8]| {
        let mut body = kind.to_le_bytes().to_vec();
        body.extend(id.to_le_bytes());
        body.extend(payload);
        anonymous_chunk(archive, 0, &body)
    };
    let mut integers = 2_i32.to_le_bytes().to_vec();
    integers.extend(7_i32.to_le_bytes());
    integers.extend((-9_i32).to_le_bytes());
    let mut text = 1_i32.to_le_bytes().to_vec();
    text.extend(utf16_bytes("distance"));
    let referenced_object = [45, 0, 0, 0, 46, 0, 47, 0, 48, 49, 50, 51, 52, 53, 54, 55];
    let mut object_reference = referenced_object.to_vec();
    object_reference.extend(7_i32.to_le_bytes());
    object_reference.extend(8_i32.to_le_bytes());
    object_reference.extend(4_i32.to_le_bytes());
    for coordinate in [1.0_f64, 2.0, 3.0] {
        object_reference.extend(coordinate.to_le_bytes());
    }
    object_reference.extend(9_i32.to_le_bytes());
    object_reference.extend(10_i32.to_le_bytes());
    object_reference.extend(11_i32.to_le_bytes());
    for parameter in [0.1_f64, 0.2, 0.3, 0.4] {
        object_reference.extend(parameter.to_le_bytes());
    }
    object_reference.extend(0_i32.to_le_bytes());
    for bound in [0.0_f64, 1.0, 2.0, 3.0, 4.0, 5.0] {
        object_reference.extend(bound.to_le_bytes());
    }
    object_reference.extend(12_i32.to_le_bytes());
    let object_reference = anonymous_chunk(archive, 3, &object_reference);
    let mut object_references = 1_i32.to_le_bytes().to_vec();
    object_references.extend(object_reference);
    let values = [
        value(2, 10, &integers),
        value(8, 20, &text),
        value(9, 25, &object_references),
        value(99, 30, &[0xaa, 0xbb]),
    ];
    let mut values_body = 4_i32.to_le_bytes().to_vec();
    values_body.extend(values.concat());
    let mut body = record_id.to_vec();
    body.extend(202_607_130_i32.to_le_bytes());
    body.extend(command_id);
    body.extend(uuid_list(descendant));
    body.extend(uuid_list(antecedent));
    body.extend(anonymous_chunk(archive, 0, &values_body));
    body.extend(1_i32.to_le_bytes());
    body.push(1);
    let payload = anonymous_chunk(archive, 2, &body);
    let history_class = [
        0x2f, 0xfd, 0xd0, 0xec, 0x88, 0x20, 0xdc, 0x49, 0x96, 0x41, 0x9c, 0xf7, 0xa2, 0x8f, 0xfa,
        0x6b,
    ];
    let record = crc_chunk(
        archive,
        0x2000_807b,
        &class_wrapper(archive, history_class, &payload),
    );
    let bytes = minimal_document(
        "50",
        &[
            crc_table(archive, 0x1000_0014, &[]),
            crc_table(archive, 0x1000_0015, &[]),
            crc_table(archive, 0x1000_0013, &[]),
            crc_table(archive, 0x1000_0026, &[record]),
        ],
    );

    let scan = super::container::scan_owned(bytes).expect("typed history record");
    let history = &scan.history[0];
    assert_eq!(
        history.id.to_string(),
        "00000001-0002-0003-0405-060708090a0b"
    );
    assert_eq!(history.version, 202_607_130);
    assert_eq!(
        history.command_id.to_string(),
        "0000000c-000d-000e-0f10-111213141516"
    );
    assert_eq!(history.descendants.len(), 1);
    assert_eq!(history.antecedents.len(), 1);
    assert_eq!(history.values.len(), 4);
    assert!(matches!(
        &history.values[0].value,
        super::history::Value::Integers(values) if values == &[7, -9]
    ));
    assert!(matches!(
        &history.values[1].value,
        super::history::Value::Strings(values) if values == &["distance"]
    ));
    assert!(matches!(
        &history.values[2].value,
        super::history::Value::ObjectReferences(values)
            if values.len() == 1
                && values[0].object_id.to_string() == "0000002d-002e-002f-3031-323334353637"
                && values[0].component == [7, 8]
                && values[0].geometry_type == 4
                && values[0].point.0 == [1.0, 2.0, 3.0]
                && values[0].evaluation.parameter_type == 9
                && values[0].evaluation.component == [10, 11]
                && values[0].evaluation.parameters == [0.1, 0.2, 0.3, 0.4]
                && values[0].evaluation.intervals == [[0.0, 1.0], [2.0, 3.0], [4.0, 5.0]]
                && values[0].instance_path.is_empty()
                && values[0].osnap_mode == 12
    ));
    assert!(matches!(
        history.values[3].value,
        super::history::Value::Opaque { type_code: 99, .. }
    ));
    assert_eq!(
        history.record_type,
        super::history::RecordType::FeatureParameters
    );
    assert!(history.copy_on_replace);

    let decoded = super::decode::decode_for_test(&scan);
    assert_eq!(decoded.ir.model.features.len(), 1);
    assert_eq!(
        decoded.ir.model.features[0].native_ref.as_deref(),
        Some("rhino:history:record#00000001-0002-0003-0405-060708090a0b")
    );
}

#[test]
fn near_budget_user_table_keeps_count_without_record_descriptors() {
    let archive = ArchiveVersion::V5;
    let records = (0..127)
        .map(|index| long_chunk(archive, 0x7000_0000 + index, &[]))
        .collect::<Vec<_>>();
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
            table(archive, 0x1000_0017, &records),
        ],
    );
    let scan =
        super::container::scan_with_test_record_limit(bytes, 128).expect("near-budget user table");
    let user = scan.tables.last().expect("user table");
    assert_eq!(user.record_count, 127);
    assert!(user.records.is_empty());
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
    let scan = super::container::scan_owned(bytes).expect("bounded unknown trailer");
    assert_eq!(scan.objects[0].unknown_trailer.len(), 1);
}

#[test]
fn scans_metadata_tables_and_reports_offsets() {
    let archive = ArchiveVersion::V5;
    let object = object_record(archive, 0x20, [0; 16]);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[object]),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("required invariant");
    assert_eq!(summary.container_kind, "3dm-chunks");
    assert_eq!(summary.entries.len(), 4);
    assert!(summary
        .notes
        .iter()
        .any(|note| note == "archive version 50"));
    assert_eq!(
        summary.entries[2].attributes.get("record_count"),
        Some(&"1".to_string())
    );
    assert_eq!(
        summary.entries[2].attributes.get("object_typecode_0x20"),
        Some(&"1".to_string())
    );
}

#[test]
fn aggregates_object_classes_after_table_entries() {
    let archive = ArchiveVersion::V5;
    let first = object_record(archive, 1, [0; 16]);
    let second = object_record(archive, 2, [1; 16]);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[first, second]),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("required invariant");
    assert_eq!(summary.entries.len(), 5);
    assert_eq!(summary.entries[3].role, "object-class");
    assert_eq!(
        summary.entries[3].attributes.get("count"),
        Some(&"1".to_string())
    );
    assert_eq!(
        summary.entries[4].attributes.get("count"),
        Some(&"1".to_string())
    );
}

#[test]
fn container_only_returns_empty_current_ir_for_full_bands() {
    for version in ["50", "60", "70", "80"] {
        let archive = parse_header(&header(version))
            .expect("required invariant")
            .archive_version;
        let bytes = minimal_document(
            version,
            &[
                table(archive, 0x1000_0014, &[]),
                table(archive, 0x1000_0015, &[]),
                table(archive, 0x1000_0013, &[]),
            ],
        );
        let result = RhinoCodec
            .decode(
                &mut Cursor::new(bytes),
                &DecodeOptions {
                    container_only: true,
                    ..Default::default()
                },
            )
            .expect("required invariant");
        assert_eq!(result.ir.ir_version, IR_VERSION);
        assert!(result.ir.model.bodies.is_empty());
        assert!(result.ir.model.subds.is_empty());
        assert!(result.report.container_only);
        assert_eq!(result.report.format, "rhino");
    }
}

#[test]
fn container_only_returns_empty_current_ir_for_v3_and_v4() {
    for version in ["3", "4"] {
        let archive = parse_header(&header(version))
            .expect("required invariant")
            .archive_version;
        let bytes = minimal_document(
            version,
            &[
                table(archive, 0x1000_0014, &[]),
                table(archive, 0x1000_0015, &[]),
                table(archive, 0x1000_0013, &[]),
            ],
        );
        let result = RhinoCodec
            .decode(
                &mut Cursor::new(bytes),
                &DecodeOptions {
                    container_only: true,
                    ..Default::default()
                },
            )
            .expect("required invariant");
        assert_eq!(result.ir.ir_version, IR_VERSION);
        assert!(result.ir.model.bodies.is_empty());
        assert!(result.ir.model.subds.is_empty());
        assert!(result.report.container_only);
    }
}

#[test]
fn header_only_bands_inspect_without_scanning_and_do_not_decode() {
    for version in ["1", "2", "5", "999"] {
        let bytes = header(version);
        let summary = RhinoCodec
            .inspect(&mut Cursor::new(bytes.clone()), &InspectOptions::default())
            .expect("required invariant");
        assert!(summary.entries.is_empty());
        assert_eq!(summary.container_kind, "3dm-chunks");
        let result = RhinoCodec.decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
                ..Default::default()
            },
        );
        assert!(matches!(result, Err(CodecError::NotImplemented(_))));
    }
}

#[test]
fn requires_end_of_table_and_rejects_wrong_order() {
    let archive = ArchiveVersion::V5;
    let mut missing = header("50");
    missing.extend(long_chunk(archive, 1, b"comment"));
    missing.extend(long_chunk(archive, 0x1000_0014, &[]));
    assert!(matches!(
        RhinoCodec.inspect(&mut Cursor::new(missing), &InspectOptions::default()),
        Err(CodecError::Malformed(_))
    ));

    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0014, &[]),
        ],
    );
    assert!(matches!(
        RhinoCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default()),
        Err(CodecError::Malformed(_))
    ));
}

#[test]
fn structural_framing_errors_keep_diagnostics() {
    let error = FramingError::Structural {
        offset: 42,
        message: "object record is missing object end".to_string(),
    };
    assert_eq!(
        error.to_string(),
        "framing error at 42: object record is missing object end"
    );
}

#[test]
fn malformed_bounded_object_is_retained_and_later_point_decodes() {
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
        let mut scan = super::container::scan_owned(bytes).expect("bounded object recovery");
        assert!(scan.objects[0].framing_degraded);
        set_test_units(&mut scan, 1.0);
        let result = super::decode::decode_for_test(&scan);
        assert_eq!(
            result
                .ir
                .native_unknowns("rhino")
                .expect("required invariant")
                .len(),
            2
        );
        assert_eq!(result.ir.model.points.len(), 1);
        assert!(result
            .report
            .losses
            .iter()
            .any(|loss| loss.severity == Severity::Error));
    }
}

#[test]
fn requires_properties_settings_and_object_tables() {
    let archive = ArchiveVersion::V5;
    for tables in [
        vec![
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
        vec![
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
        vec![
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
        ],
    ] {
        let bytes = minimal_document("50", &tables);
        assert!(matches!(
            RhinoCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default()),
            Err(CodecError::Malformed(message))
                if message.contains("properties, settings, and object tables")
        ));
    }
}

#[test]
fn crc_mismatch_is_a_summary_warning_and_later_record_survives() {
    let archive = ArchiveVersion::V5;
    let mut bad_object = object_record(archive, 0x08, [0; 16]);
    let crc_offset = bad_object.len() - 1;
    bad_object[crc_offset] ^= 1;
    let good_object = object_record(archive, 0x08, [0; 16]);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[bad_object, good_object]),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("required invariant");
    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("CRC mismatch")));
    assert_eq!(
        summary.entries[2].attributes.get("record_count"),
        Some(&"2".to_string())
    );
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
    let scan = super::container::scan_owned(bytes).expect("required invariant");
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
fn repeated_consecutive_user_tables_are_allowed() {
    let archive = ArchiveVersion::V5;
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
            table(archive, 0x1000_0017, &[]),
            table(archive, 0x1000_0017, &[]),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("required invariant");
    assert_eq!(summary.entries.len(), 5);
}

#[test]
fn obsolete_layerset_occupies_the_layer_group_compatibility_slot() {
    let archive = ArchiveVersion::V5;
    let valid = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0011, &[]),
            table(archive, 0x1000_0024, &[]),
            table(archive, 0x1000_0018, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
    );
    assert!(RhinoCodec
        .inspect(&mut Cursor::new(valid), &InspectOptions::default())
        .is_ok());

    let invalid = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0024, &[]),
            table(archive, 0x1000_0011, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
    );
    assert!(matches!(
        RhinoCodec.inspect(&mut Cursor::new(invalid), &InspectOptions::default()),
        Err(CodecError::Malformed(_))
    ));
}

#[test]
fn accepts_table_crc_with_its_declared_bound() {
    let archive = ArchiveVersion::V5;
    let bytes = minimal_document(
        "50",
        &[
            crc_table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("required invariant");
    assert_eq!(summary.entries.len(), 3);
}

#[test]
fn rejects_short_object_and_unknown_table_records() {
    let archive = ArchiveVersion::V5;
    let short_object = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(
                archive,
                0x1000_0013,
                &[short_chunk(archive, 0x2000_8070, 0)],
            ),
        ],
    );
    assert!(matches!(
        RhinoCodec.inspect(&mut Cursor::new(short_object), &InspectOptions::default()),
        Err(CodecError::Malformed(_))
    ));

    let unknown = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[long_chunk(archive, 0x1234, &[1])]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(unknown), &InspectOptions::default())
        .expect("required invariant");
    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("unknown bounded record")));
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
fn maps_standard_units_to_millimeters() {
    assert_eq!(settings::standard_scale(2), Some(1.0));
    assert_eq!(settings::standard_scale(8), Some(25.4));
    assert_eq!(settings::standard_scale(12), Some(1.0e-7));
    assert_eq!(settings::standard_scale(255), None);
}

fn metadata_record(typecode: u32, data: Vec<u8>) -> (Vec<u8>, super::container::Record) {
    let length = data.len();
    (
        data,
        super::container::Record {
            typecode,
            range: 0..length,
            body: 0..length,
            short: false,
            value: 0,
        },
    )
}

#[test]
fn parses_units_with_single_scale_transfer_and_legacy_order() {
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
fn decodes_as_file_name_as_utf16_and_rejects_fixed_trailing_bytes() {
    let mut name = Vec::new();
    name.extend(2_u32.to_le_bytes());
    name.extend([b'X', 0, 0, 0]);
    let (data, record) = metadata_record(0x2000_8027, name);
    let table = super::container::Table {
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
    let table = super::container::Table {
        typecode: 0x1000_0014,
        range: 0..trailing.len(),
        body: 0..trailing.len(),
        records: vec![record],
        record_count: 1,
        object_typecodes: std::collections::BTreeMap::new(),
    };
    let mut warnings = Vec::new();
    let metadata = settings::parse_metadata(&trailing, ArchiveVersion::V5, &[table], &mut warnings);
    assert!(metadata.properties.as_file_name.is_none());
    assert_eq!(warnings.len(), 1);
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
    section_style.extend(crc_chunk(archive, 0x4000_8002, &[]));
    section_style.push(0);
    payload.push(35);
    payload.extend(crc_chunk(archive, 0x4000_8000, &section_style));
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
    let class_descriptor = super::objects::parse_class_wrapper(
        &data,
        record.body.clone(),
        archive,
        &mut wrapper_warnings,
    )
    .expect("required invariant");
    assert_eq!(class_descriptor.class_data_range.len(), payload.len());
    let table = super::container::Table {
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
}

#[test]
fn parses_selector_widths_from_their_serialized_forms() {
    let mut settings_value = settings::DocumentSettings::default();
    let mut material_data = 42_i32.to_le_bytes().to_vec();
    material_data.extend(3_i32.to_le_bytes());
    let material_record = super::container::Record {
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
    let color_record = super::container::Record {
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
        let record = super::container::Record {
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
fn decode_context_transitions_object_status_once_and_links_unknowns() {
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
    let scan = super::container::scan_owned(bytes).expect("required invariant");
    super::decode::with_expand(&scan, |expand| {
        let mut context = super::decode::DecodeContext::new(&scan, expand);
        assert!(context.object(0).is_some());
        assert!(context.unknown(0).is_some());
        assert_eq!(context.unit_scale(), None);
        assert_eq!(context.archive(), archive);
        assert!(context.append_link(0, "rhino:curve#2".to_string()));
        assert!(context.append_link(0, "rhino:curve#1".to_string()));
        assert!(context.append_link(0, "rhino:curve#2".to_string()));
        assert_eq!(
            context.unknown(0).expect("required invariant").links,
            vec!["rhino:curve#1".to_string(), "rhino:curve#2".to_string()]
        );
        assert!(context.mark_decoded(0));
        assert!(!context.mark_decoded(0));
        assert!(!context.mark_failed(0));
        assert_eq!(context.ir_mut().model.bodies.len(), 0);
        context
            .unknown_mut(0)
            .expect("required invariant")
            .links
            .clear();
        let result = context.commit();
        assert!(result
            .report
            .losses
            .iter()
            .any(|loss| loss.severity == Severity::Info));
        assert_eq!(
            result
                .ir
                .native_unknowns("rhino")
                .expect("required invariant")
                .len(),
            1
        );
        let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
        assert_eq!(validation.error_count(), 0);
    });
}

#[test]
fn rejected_candidate_detaches_payload_clone_and_preserves_live_bytes() {
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
    let scan = super::container::scan_owned(bytes).expect("required invariant");
    super::decode::with_expand(&scan, |expand| {
        let mut context = super::decode::DecodeContext::new(&scan, expand);
        let original = context
            .unknown(0)
            .expect("required invariant")
            .data
            .clone()
            .expect("required invariant");
        let (payloads_detached, findings) = context.reject_duplicate_unknown_candidate();
        assert!(payloads_detached);
        assert!(findings.contains("identity"));
        assert_eq!(
            context
                .unknown(0)
                .expect("required invariant")
                .data
                .as_deref(),
            Some(original.as_slice())
        );
        assert_eq!(context.unknown_count(), 1);
    });
}

fn set_test_units(scan: &mut super::container::Scan<'_>, scale: f64) {
    scan.metadata.settings.units = Some(settings::UnitsAndTolerances {
        version: 1,
        unit_value: 2,
        unit: settings::UnitSystem::Standard(2),
        millimeters_per_unit: Some(scale),
        absolute_tolerance: 0.01,
        absolute_tolerance_millimeters: Some(0.01 * scale),
        angular_tolerance: 0.1,
        relative_tolerance: 0.01,
        distance_display_mode: None,
        distance_display_precision: None,
        source: settings::SourceRange { range: 0..0 },
    });
}

fn point_payload(point: [f64; 3]) -> Vec<u8> {
    let mut payload = vec![0x10];
    for value in point {
        payload.extend(value.to_le_bytes());
    }
    payload
}

fn nurbs_curve_payload(points: [[f64; 3]; 2]) -> Vec<u8> {
    let mut payload = vec![0x10];
    payload.extend(3_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend(2_i32.to_le_bytes());
    payload.extend(2_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 1.0] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(2_i32.to_le_bytes());
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(1.0_f64.to_le_bytes());
    payload.extend(2_i32.to_le_bytes());
    for point in points {
        for value in point {
            payload.extend(value.to_le_bytes());
        }
    }
    payload
}

fn circle_payload() -> Vec<u8> {
    let mut payload = vec![0x10];
    for value in [
        0.0_f64,
        0.0,
        0.0, // origin
        1.0,
        0.0,
        0.0, // x
        0.0,
        1.0,
        0.0, // y
        0.0,
        0.0,
        1.0, // z
        0.0,
        0.0,
        1.0,
        0.0, // equation
        1.0, // radius
        1.0,
        0.0,
        0.0, // zero
        0.0,
        1.0,
        0.0, // half pi
        -1.0,
        0.0,
        0.0, // pi
        0.0,
        std::f64::consts::TAU, // angle
        0.0,
        std::f64::consts::TAU, // domain
    ] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(3_i32.to_le_bytes());
    payload
}

fn mesh_payload() -> Vec<u8> {
    let mut payload = vec![0x10];
    payload.extend(3_i32.to_le_bytes());
    payload.extend(1_i32.to_le_bytes());
    for _ in 0..4 {
        payload.extend(0.0_f64.to_le_bytes());
        payload.extend(1.0_f64.to_le_bytes());
    }
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(0.0_f64.to_le_bytes());
    for _ in 0..16 {
        payload.extend(0.0_f32.to_le_bytes());
    }
    payload.extend(0_i32.to_le_bytes());
    payload.push(0);
    payload.extend([0; 4]);
    payload.extend(1_i32.to_le_bytes());
    payload.extend([0, 1, 2, 2]);
    payload.extend(3_i32.to_le_bytes());
    for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(3_i32.to_le_bytes());
    for _ in 0..3 {
        for value in [1.0_f32, 0.0, 1.0] {
            payload.extend(value.to_le_bytes());
        }
    }
    payload.extend(0_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload
}

fn instance_reference_payload(definition_id: [u8; 16], rows: [[f64; 4]; 4]) -> Vec<u8> {
    let mut payload = vec![0x10];
    payload.extend(definition_id);
    for value in rows.into_iter().flatten() {
        payload.extend(value.to_le_bytes());
    }
    for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 1.0] {
        payload.extend(value.to_le_bytes());
    }
    payload
}

fn transform(scale_x: f64, translation: [f64; 3]) -> [[f64; 4]; 4] {
    [
        [scale_x, 0.0, 0.0, translation[0]],
        [0.0, 1.0, 0.0, translation[1]],
        [0.0, 0.0, 1.0, translation[2]],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn static_definition(id: [u8; 16], members: &[[u8; 16]]) -> super::instances::InstanceDefinition {
    super::instances::InstanceDefinition {
        source_range: 0..0,
        id: Uuid::from_wire(id),
        members: members.iter().copied().map(Uuid::from_wire).collect(),
        index: None,
        name: String::new(),
        description: String::new(),
        url: String::new(),
        url_tag: String::new(),
        kind: super::instances::DefinitionKind::Static,
        units: super::instances::UnitDetail {
            unit: 2,
            meters_per_unit: 0.001,
            custom_name: String::new(),
        },
        legacy_linked_path: String::new(),
        legacy_checksum_range: None,
        legacy_relative_path: false,
        linked_depth: 0,
        linked_appearance: 0,
        file_reference_range: None,
        file_reference: None,
        reference_settings_range: None,
    }
}

fn set_identity(
    scan: &mut super::container::Scan<'_>,
    source_order: usize,
    object_id: [u8; 16],
    source_key: &str,
    color: Option<[u8; 4]>,
    visible: bool,
) {
    let object = &mut scan.objects[source_order];
    object.identity = Some(super::objects::SourceIdentity {
        source_id: format!("rhino:object:record#{source_key}"),
        object_id: Uuid::from_wire(object_id),
        class_uuid: object.class_uuid,
        name: String::new(),
        layer_index: -1,
        layer_id: None,
        layer_name: None,
        effective_color: color,
        effective_visible: visible,
        object_mode: 0,
        definition_member: false,
        object_frame: None,
        source: settings::SourceRange {
            range: object.range.clone(),
        },
    });
}

fn scan_with_objects(objects: &[Vec<u8>]) -> super::container::Scan<'static> {
    let archive = ArchiveVersion::V5;
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, objects),
        ],
    );
    let mut scan = super::container::scan_owned(bytes).expect("required invariant");
    set_test_units(&mut scan, 1.0);
    scan
}

fn install_definitions(
    scan: &mut super::container::Scan<'_>,
    definitions: Vec<super::instances::InstanceDefinition>,
) {
    scan.definitions.definitions = definitions;
    scan.definitions.member_object_ids = scan
        .definitions
        .definitions
        .iter()
        .flat_map(|definition| definition.members.iter().copied())
        .collect();
}

#[test]
fn static_instance_suppresses_member_and_two_references_expand_with_distinct_ids() {
    let archive = ArchiveVersion::V5;
    let member_id = [0x51; 16];
    let definition_id = [0x61; 16];
    let first_reference_id = [0x71; 16];
    let second_reference_id = [0x72; 16];
    let member =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0]));
    let first = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, transform(1.0, [10.0, 0.0, 0.0])),
    );
    let second = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, transform(1.0, [20.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[member, first, second]);
    set_identity(&mut scan, 0, member_id, "member", None, true);
    set_identity(&mut scan, 1, first_reference_id, "first", None, true);
    set_identity(&mut scan, 2, second_reference_id, "second", None, true);
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[member_id])],
    );

    let result = super::decode::decode_for_test(&scan);
    assert_eq!(result.ir.model.bodies.len(), 2);
    assert_eq!(result.ir.model.points.len(), 2);
    assert_eq!(
        result
            .ir
            .model
            .bodies
            .iter()
            .map(|body| body.transform.expect("required invariant").rows[0][3])
            .collect::<Vec<_>>(),
        vec![10.0, 20.0]
    );
    let body_ids = result
        .ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.to_string())
        .collect::<Vec<_>>();
    assert_ne!(body_ids[0], body_ids[1]);
    assert_eq!(
        result
            .ir
            .native_unknowns("rhino")
            .expect("required invariant")[0]
            .links,
        body_ids
    );
    assert_eq!(
        result
            .ir
            .native_unknowns("rhino")
            .expect("required invariant")[1]
            .links
            .len(),
        1
    );
    assert_eq!(
        result
            .ir
            .native_unknowns("rhino")
            .expect("required invariant")[2]
            .links
            .len(),
        1
    );
    let native = result
        .ir
        .native
        .namespace("rhino")
        .expect("required invariant");
    assert_eq!(native.arenas["product_definitions"].len(), 1);
    assert_eq!(native.arenas["product_occurrences"].len(), 2);
    assert_eq!(
        native.arenas["product_occurrences"][0].fields["definition_uuid"],
        Uuid::from_wire(definition_id).to_string()
    );
    assert_eq!(
        native.arenas["product_occurrences"][0].fields["transform_units"],
        "millimeter"
    );
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message == "decoded 3/3 Rhino object records"));
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn nested_instance_composes_parent_child_and_records_outer_to_inner_path() {
    let archive = ArchiveVersion::V5;
    let member_id = [0x52; 16];
    let nested_reference_id = [0x73; 16];
    let world_reference_id = [0x74; 16];
    let inner_definition_id = [0x62; 16];
    let outer_definition_id = [0x63; 16];
    let curve = object_record_with_payload(
        archive,
        4,
        NURBS_CURVE_CLASS,
        &nurbs_curve_payload([[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]),
    );
    let nested = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(inner_definition_id, transform(2.0, [0.0, 0.0, 0.0])),
    );
    let world = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(outer_definition_id, transform(1.0, [10.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[curve, nested, world]);
    set_identity(&mut scan, 0, member_id, "curve", None, true);
    set_identity(
        &mut scan,
        1,
        nested_reference_id,
        "nested-reference",
        None,
        true,
    );
    set_identity(
        &mut scan,
        2,
        world_reference_id,
        "world-reference",
        Some([255, 0, 0, 0]),
        false,
    );
    install_definitions(
        &mut scan,
        vec![
            static_definition(inner_definition_id, &[member_id]),
            static_definition(outer_definition_id, &[nested_reference_id]),
        ],
    );

    let result = super::decode::decode_for_test(&scan);
    assert_eq!(result.ir.model.curves.len(), 1);
    let curve = &result.ir.model.curves[0];
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        panic!("expected transformed NURBS");
    };
    assert_eq!(nurbs.control_points[0].x, 12.0);
    assert_eq!(nurbs.control_points[1].x, 14.0);
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .expect("required invariant")
            .instance_path,
        vec![
            Uuid::from_wire(world_reference_id).to_string(),
            Uuid::from_wire(nested_reference_id).to_string()
        ]
    );
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .expect("required invariant")
            .color,
        Some(cadmpeg_ir::topology::Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        })
    );
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .expect("required invariant")
            .visible,
        Some(false)
    );
    assert!(curve.id.to_string().contains(&format!(
        "{}.{}",
        Uuid::from_wire(world_reference_id),
        Uuid::from_wire(nested_reference_id)
    )));
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn nil_and_duplicate_reference_ids_use_distinct_record_path_segments() {
    let archive = ArchiveVersion::V5;
    let member_id = [0x53; 16];
    let definition_id = [0x64; 16];
    let duplicate_reference_id = [0x75; 16];
    let curve = object_record_with_payload(
        archive,
        4,
        NURBS_CURVE_CLASS,
        &nurbs_curve_payload([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]),
    );
    let reference = || {
        object_record_with_payload(
            archive,
            0x1000,
            INSTANCE_REFERENCE_CLASS,
            &instance_reference_payload(definition_id, transform(1.0, [0.0, 0.0, 0.0])),
        )
    };
    let mut scan = scan_with_objects(&[curve, reference(), reference(), reference(), reference()]);
    set_identity(&mut scan, 0, member_id, "member", None, true);
    set_identity(&mut scan, 1, [0; 16], "nil-first", None, true);
    set_identity(&mut scan, 2, [0; 16], "nil-second", None, true);
    set_identity(
        &mut scan,
        3,
        duplicate_reference_id,
        "duplicate-first",
        None,
        true,
    );
    set_identity(
        &mut scan,
        4,
        duplicate_reference_id,
        "duplicate-second",
        None,
        true,
    );
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[member_id])],
    );

    let result = super::decode::decode_for_test(&scan);
    assert_eq!(result.ir.model.curves.len(), 4);
    let ids = result
        .ir
        .model
        .curves
        .iter()
        .map(|curve| curve.id.to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 4);
    let paths = result
        .ir
        .model
        .curves
        .iter()
        .map(|curve| {
            curve
                .source_object
                .as_ref()
                .expect("required invariant")
                .instance_path
                .clone()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(paths.len(), 4);
    assert!(paths
        .iter()
        .flatten()
        .all(|segment| segment.starts_with("record-")));
    assert_eq!(
        result
            .ir
            .native_unknowns("rhino")
            .expect("required invariant")[0]
            .links
            .len(),
        4
    );
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn instance_bakes_mesh_subd_and_normals_without_changing_subd_metadata() {
    let archive = ArchiveVersion::V5;
    let mesh_id = [0x54; 16];
    let subd_id = [0x55; 16];
    let definition_id = [0x65; 16];
    let reference_id = [0x76; 16];
    let mesh = object_record_with_payload(archive, 0x20, MESH_CLASS, &mesh_payload());
    let subd = object_record_with_payload(
        archive,
        0x0004_0000,
        SUBD_CLASS,
        &super::subd::tests::quad_payload(archive),
    );
    let rows = [
        [2.0, 0.0, 0.0, 5.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 0.5, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, rows),
    );
    let mut scan = scan_with_objects(&[mesh, subd, reference]);
    set_identity(&mut scan, 0, mesh_id, "mesh", None, true);
    set_identity(&mut scan, 1, subd_id, "subd", None, true);
    set_identity(&mut scan, 2, reference_id, "reference", None, true);
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[mesh_id, subd_id])],
    );

    let result = super::decode::decode_for_test(&scan);
    let mesh = &result.ir.model.tessellations[0];
    assert_eq!(mesh.vertices[0].x, 5.0);
    assert_eq!(mesh.vertices[1].x, 7.0);
    assert_eq!(
        mesh.normals[0],
        cadmpeg_ir::math::Vector3::new(0.242_535_625_036_332_97, 0.0, 0.970_142_500_145_331_9)
    );
    let subd = &result.ir.model.subds[0];
    assert_eq!(subd.vertices[2].point.x, 7.0);
    assert_eq!(subd.edges[0].sharpness, [0.25, 0.25]);
    assert_eq!(subd.edges[0].sector_coefficients, [0.125, 0.875]);
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn failed_instance_expansion_retains_inflated_member_mesh_budget() {
    // Failed expansions cannot reclaim bytes retained in the shared arena. Their
    // mesh-budget charge must survive to prevent repeated failures bypassing the cap.
    let archive = ArchiveVersion::V5;
    let mesh_id = [0x54; 16];
    let missing_id = [0x99; 16];
    let definition_id = [0x65; 16];
    let reference_id = [0x76; 16];
    let mesh = object_record_with_payload(
        archive,
        0x20,
        MESH_CLASS,
        &super::archive_test_support::mesh_payload(3, 0, false, false),
    );
    let reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, transform(1.0, [0.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[mesh, reference]);
    set_identity(&mut scan, 0, mesh_id, "mesh", None, true);
    set_identity(&mut scan, 1, reference_id, "reference", None, true);
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[mesh_id, missing_id])],
    );

    super::decode::with_expand(&scan, |expand| {
        let mut context = super::decode::DecodeContext::new(&scan, expand);
        context.decode_geometry();
        assert!(context.mesh_budget_used() > 0);
        let result = context.commit();
        assert!(result.ir.model.tessellations.is_empty());
        assert!(result.ir.model.bodies.is_empty());
    });
}

#[test]
fn nonuniform_instance_converts_analytic_circle_to_exact_nurbs() {
    let archive = ArchiveVersion::V5;
    let member_id = [0x56; 16];
    let definition_id = [0x66; 16];
    let reference_id = [0x77; 16];
    let circle = object_record_with_payload(archive, 4, ARC_CURVE_CLASS, &circle_payload());
    let reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, transform(2.0, [0.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[circle, reference]);
    set_identity(&mut scan, 0, member_id, "circle", None, true);
    set_identity(&mut scan, 1, reference_id, "reference", None, true);
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[member_id])],
    );

    let result = super::decode::decode_for_test(&scan);
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &result.ir.model.curves[0].geometry
    else {
        panic!("nonuniform circle must become NURBS");
    };
    assert_eq!(nurbs.degree, 2);
    assert_eq!(nurbs.control_points[0].x, 2.0);
    assert_eq!(nurbs.control_points[2].y, 1.0);
    assert_eq!(
        nurbs.weights.as_ref().expect("required invariant")[1],
        std::f64::consts::FRAC_1_SQRT_2
    );
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn transformed_procedural_instance_keeps_solved_carriers_without_dangling_references() {
    let archive = ArchiveVersion::V5;
    let member_id = [0x57; 16];
    let definition_id = [0x67; 16];
    let reference_id = [0x78; 16];
    let revolution = object_record_with_payload(
        archive,
        8,
        REV_SURFACE_CLASS,
        &super::surfaces::tests::valid_revolution_payload(0x20),
    );
    let reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, transform(2.0, [3.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[revolution, reference]);
    set_identity(&mut scan, 0, member_id, "revolution", None, true);
    set_identity(&mut scan, 1, reference_id, "reference", None, true);
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[member_id])],
    );

    let result = super::decode::decode_for_test(&scan);
    assert!(!result.ir.model.surfaces.is_empty());
    assert!(result.ir.model.procedural_surfaces.is_empty());
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("exact solved carrier retained")));
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn branching_instance_budget_retains_current_reference_and_later_reference_recovers() {
    let archive = ArchiveVersion::V5;
    let first_member_id = [0x31; 16];
    let second_member_id = [0x32; 16];
    let wide_definition = [0x41; 16];
    let narrow_definition = [0x42; 16];
    let first_member =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 0.0, 0.0]));
    let second_member =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([2.0, 0.0, 0.0]));
    let wide_reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(wide_definition, transform(1.0, [0.0, 0.0, 0.0])),
    );
    let narrow_reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(narrow_definition, transform(1.0, [10.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[
        first_member,
        second_member,
        wide_reference,
        narrow_reference,
    ]);
    for (source_order, id) in [first_member_id, second_member_id, [0x51; 16], [0x52; 16]]
        .into_iter()
        .enumerate()
    {
        set_identity(
            &mut scan,
            source_order,
            id,
            &format!("budget-{source_order}"),
            None,
            true,
        );
    }
    install_definitions(
        &mut scan,
        vec![
            static_definition(wide_definition, &[first_member_id, second_member_id]),
            static_definition(narrow_definition, &[second_member_id]),
        ],
    );
    super::decode::with_expand(&scan, |expand| {
        let mut context = super::decode::DecodeContext::new(&scan, expand);
        context.set_expansion_limits([16, 1, 128]);
        context.decode_geometry();
        let result = context.commit();
        assert_eq!(result.ir.model.points.len(), 1);
        assert_eq!(
            result.ir.model.bodies[0]
                .transform
                .expect("instance transform")
                .rows[0][3],
            10.0
        );
        assert!(result
            .report
            .losses
            .iter()
            .any(|loss| loss.message.contains("instance member budget exceeded")));
    });
}

#[test]
fn invalid_instance_families_are_atomic_and_later_reference_recovers() {
    let archive = ArchiveVersion::V5;
    let nested_b_id = [0x81; 16];
    let nested_a_id = [0x82; 16];
    let ambiguous_member_id = [0x83; 16];
    let valid_member_id = [0x84; 16];
    let unknown_member_id = [0x85; 16];
    let definition_a = [0x91; 16];
    let definition_b = [0x92; 16];
    let missing_member_definition = [0x93; 16];
    let duplicate_member_definition = [0x94; 16];
    let ambiguous_member_definition = [0x95; 16];
    let external_definition = [0x96; 16];
    let valid_definition = [0x97; 16];
    let unknown_definition = [0x98; 16];
    let missing_definition = [0x99; 16];

    let reference_object = |definition, rows| {
        object_record_with_payload(
            archive,
            0x1000,
            INSTANCE_REFERENCE_CLASS,
            &instance_reference_payload(definition, rows),
        )
    };
    let nested_b = reference_object(definition_b, transform(1.0, [0.0, 0.0, 0.0]));
    let nested_a = reference_object(definition_a, transform(1.0, [0.0, 0.0, 0.0]));
    let ambiguous_first =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 0.0, 0.0]));
    let ambiguous_second =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([2.0, 0.0, 0.0]));
    let valid_member =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([3.0, 0.0, 0.0]));
    let unknown_member = object_record_with_payload(archive, 8, REV_SURFACE_CLASS, &[0]);
    let world_cycle = reference_object(definition_a, transform(1.0, [0.0, 0.0, 0.0]));
    let missing_definition_reference =
        reference_object(missing_definition, transform(1.0, [0.0, 0.0, 0.0]));
    let missing_member_reference =
        reference_object(missing_member_definition, transform(1.0, [0.0, 0.0, 0.0]));
    let duplicate_member_reference =
        reference_object(duplicate_member_definition, transform(1.0, [0.0, 0.0, 0.0]));
    let ambiguous_member_reference =
        reference_object(ambiguous_member_definition, transform(1.0, [0.0, 0.0, 0.0]));
    let external_reference = reference_object(external_definition, transform(1.0, [0.0, 0.0, 0.0]));
    let mut singular = transform(1.0, [0.0, 0.0, 0.0]);
    singular[2][2] = 0.0;
    let singular_reference = reference_object(valid_definition, singular);
    let mut nonfinite = transform(1.0, [0.0, 0.0, 0.0]);
    nonfinite[0][0] = f64::NAN;
    let nonfinite_reference = reference_object(valid_definition, nonfinite);
    let unknown_reference = reference_object(unknown_definition, transform(1.0, [0.0, 0.0, 0.0]));
    let valid_reference = reference_object(valid_definition, transform(1.0, [30.0, 0.0, 0.0]));

    let mut scan = scan_with_objects(&[
        nested_b,
        nested_a,
        ambiguous_first,
        ambiguous_second,
        valid_member,
        unknown_member,
        world_cycle,
        missing_definition_reference,
        missing_member_reference,
        duplicate_member_reference,
        ambiguous_member_reference,
        external_reference,
        singular_reference,
        nonfinite_reference,
        unknown_reference,
        valid_reference,
    ]);
    let identities = [
        nested_b_id,
        nested_a_id,
        ambiguous_member_id,
        ambiguous_member_id,
        valid_member_id,
        unknown_member_id,
        [0xa0; 16],
        [0xa1; 16],
        [0xa2; 16],
        [0xa3; 16],
        [0xa4; 16],
        [0xa5; 16],
        [0xa6; 16],
        [0xa7; 16],
        [0xa8; 16],
        [0xa9; 16],
    ];
    for (source_order, id) in identities.into_iter().enumerate() {
        set_identity(
            &mut scan,
            source_order,
            id,
            &format!("object-{source_order}"),
            None,
            true,
        );
    }
    let missing_member_id = [0xff; 16];
    let mut external = static_definition(external_definition, &[]);
    external.kind = super::instances::DefinitionKind::Linked;
    install_definitions(
        &mut scan,
        vec![
            static_definition(definition_a, &[nested_b_id]),
            static_definition(definition_b, &[nested_a_id]),
            static_definition(missing_member_definition, &[missing_member_id]),
            static_definition(
                duplicate_member_definition,
                &[valid_member_id, valid_member_id],
            ),
            static_definition(ambiguous_member_definition, &[ambiguous_member_id]),
            external,
            static_definition(valid_definition, &[valid_member_id]),
            static_definition(unknown_definition, &[unknown_member_id]),
        ],
    );

    let result = super::decode::decode_for_test(&scan);
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.points.len(), 1);
    assert!(result.ir.model.surfaces.is_empty());
    assert_eq!(
        result.ir.model.bodies[0]
            .transform
            .expect("required invariant")
            .rows[0][3],
        30.0
    );
    for unknown in &result
        .ir
        .native_unknowns("rhino")
        .expect("required invariant")[6..15]
    {
        assert!(unknown.links.is_empty());
    }
    assert_eq!(
        result
            .ir
            .native_unknowns("rhino")
            .expect("required invariant")[15]
            .links
            .len(),
        1
    );
    assert!(result.report.losses.iter().any(|loss| {
        loss.message
            .starts_with("f9cfb638-b9d4-4340-87e3-c56e7865d96a:")
            && loss.message.contains("decode warnings")
    }));
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn subd_decode_commits_association_link_exactness_status_and_report() {
    let archive = ArchiveVersion::V5;
    let uuid = [
        0xd9, 0xa4, 0x9b, 0xf0, 0x5b, 0x45, 0xc3, 0x42, 0xba, 0x3b, 0xe6, 0xcc, 0xac, 0xef, 0x85,
        0x3b,
    ];
    let object = object_record_with_payload(
        archive,
        0x0004_0000,
        uuid,
        &super::subd::tests::quad_payload(archive),
    );
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[object]),
        ],
    );
    let mut scan = super::container::scan_owned(bytes).expect("required invariant");
    set_test_units(&mut scan, 25.4);
    let result = super::decode::decode_for_test(&scan);
    assert_eq!(result.ir.model.subds.len(), 1);
    let subd = &result.ir.model.subds[0];
    assert!(subd.source_object.is_some());
    assert_eq!(subd.vertices[2].point.x, 25.4);
    assert_eq!(
        result
            .source_fidelity
            .annotations
            .exactness
            .get(&subd.id.to_string())
            .map(|note| note.entity),
        Some(cadmpeg_ir::Exactness::Derived)
    );
    assert_eq!(
        result
            .ir
            .native_unknowns("rhino")
            .expect("required invariant")[0]
            .links,
        vec![subd.id.to_string()]
    );
    assert!(result.report.geometry_transferred);
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message == "decoded 1/1 Rhino object records"));
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn malformed_subd_is_atomic_and_later_object_recovers() {
    let archive = ArchiveVersion::V5;
    let uuid = [
        0xd9, 0xa4, 0x9b, 0xf0, 0x5b, 0x45, 0xc3, 0x42, 0xba, 0x3b, 0xe6, 0xcc, 0xac, 0xef, 0x85,
        0x3b,
    ];
    let malformed = object_record_with_payload(archive, 0x0004_0000, uuid, &[2]);
    let empty = object_record_with_payload(archive, 0x0004_0000, uuid, &[0]);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[malformed, empty]),
        ],
    );
    let mut scan = super::container::scan_owned(bytes).expect("required invariant");
    set_test_units(&mut scan, 1.0);
    let result = super::decode::decode_for_test(&scan);
    assert!(result.ir.model.subds.is_empty());
    assert_eq!(
        result
            .ir
            .native_unknowns("rhino")
            .expect("required invariant")
            .len(),
        2
    );
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.severity == Severity::Error));
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message == "decoded 1/2 Rhino object records"));
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
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
    let mut scan = super::container::scan_owned(bytes).expect("required invariant");
    scan.objects[0].attributes_degraded = true;
    super::decode::with_expand(&scan, |expand| {
        let mut context = super::decode::DecodeContext::new(&scan, expand);
        assert!(context.mark_decoded(0));
        let result = context.commit();
        assert!(result
            .report
            .losses
            .iter()
            .any(|loss| { loss.code == LossCode::AttributesNotTransferred }));
    });
}

#[test]
fn unknown_surface_placeholder_does_not_report_geometry_transfer() {
    let archive = ArchiveVersion::V5;
    let object = object_record_with_payload(archive, 8, REV_SURFACE_CLASS, &[0]);
    let mut scan = scan_with_objects(&[object]);
    set_test_units(&mut scan, 1.0);
    let result = super::decode::decode_for_test(&scan);
    assert_eq!(result.ir.model.surfaces.len(), 1);
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Unknown { .. }
    ));
    assert!(!result.report.geometry_transferred);
}

#[test]
fn scaled_coordinate_overflow_retains_object_transactionally_and_repeats_deterministically() {
    let archive = ArchiveVersion::V5;
    let object =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([2.0, 0.0, 0.0]));
    let mut scan = scan_with_objects(&[object]);
    set_test_units(&mut scan, 1.0e308);
    let first = super::decode::decode_for_test(&scan);
    let second = super::decode::decode_for_test(&scan);
    assert!(first.ir.model.points.is_empty());
    assert_eq!(first.ir, second.ir);
    assert_eq!(first.report, second.report);
    assert!(first
        .report
        .losses
        .iter()
        .any(|loss| loss.severity == Severity::Error));
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
    let scan = super::container::scan_owned(bytes).expect("required invariant");
    let offset = scan.objects[0].range.start as u64;
    let class = scan.objects[0].class_uuid.to_string();
    let result = super::decode::decode_for_test(&scan);

    let loss = result
        .report
        .losses
        .iter()
        .find(|loss| {
            loss.category == cadmpeg_ir::report::LossCategory::Geometry && loss.provenance.is_some()
        })
        .and_then(|loss| loss.provenance.as_ref())
        .expect("retained geometry loss has provenance");
    let expected_tag = format!("OBJECT_RECORD/class={class}/type=0x00000001");
    assert_eq!(loss.format, "rhino");
    assert_eq!(loss.stream, "");
    assert_eq!(loss.offset, offset);
    assert_eq!(loss.tag.as_deref(), Some(expected_tag.as_str()));
    assert!(!result
        .report
        .losses
        .iter()
        .any(|loss| { loss.message.contains("OBJECT_RECORD") || loss.message.contains("offset") }));
}

#[test]
fn procedural_surface_dispatch_accepts_native_legacy_and_sum_uuids() {
    let native = Uuid::from_wire([
        0xd3, 0x20, 0x62, 0xa1, 0x3b, 0x16, 0xd4, 0x11, 0x80, 0x00, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ]);
    let legacy = Uuid::from_wire([
        0xb6, 0x01, 0x84, 0x0a, 0x34, 0x4d, 0x99, 0x4b, 0x86, 0x15, 0x1b, 0x4e, 0x72, 0x3d, 0xc4,
        0xe5,
    ]);
    let sum = Uuid::from_wire([
        0x59, 0x53, 0xcd, 0xc4, 0x6d, 0x44, 0x90, 0x46, 0x9f, 0xf5, 0x29, 0x05, 0x97, 0x32, 0x47,
        0x2b,
    ]);
    for uuid in [native, legacy, sum] {
        assert!(super::curves::supported_class(uuid));
        assert!(super::surfaces::is_procedural_class(uuid));
    }
    assert_ne!(native.to_string(), legacy.to_string());
}

#[test]
fn typed_class_constants_preserve_canonical_uuid_display() {
    assert_eq!(
        super::mesh::ON_MESH.to_string(),
        "4ed7d4e4-e947-11d3-bfe5-0010830122f0"
    );
    assert_eq!(
        super::brep::ON_BREP.to_string(),
        "60b5dbc5-e660-11d3-bfe4-0010830122f0"
    );
    assert_eq!(
        super::extrusion::ON_EXTRUSION.to_string(),
        "36f53175-72b8-4d47-bf1f-b4e6fc24f4b9"
    );
    assert_eq!(
        super::subd::ON_SUBD.to_string(),
        "f09ba4d9-455b-42c3-ba3b-e6ccacef853b"
    );
}
