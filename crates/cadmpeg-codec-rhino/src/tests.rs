// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecError, Confidence, DecodeOptions};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::IR_VERSION;

use super::chunks::{
    anonymous_version, checked_count_bytes, chunk_at, crc16, packed_version, parse_eof,
    parse_header, verify_checksum, ArchiveVersion, BoundedReader, ChecksumStatus, FramingError,
    TCODE_CLASS_UUID, TCODE_CRC, TCODE_ENDOFFILE, TCODE_SHORT,
};
use super::objects::Uuid;
use super::settings;
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
    assert!(matches!(parse_header(&header("12345678")), Ok(_)));
}

#[test]
fn parses_widths_short_long_and_bounds() {
    let short = (TCODE_SHORT | 7).to_le_bytes();
    let mut bytes = short.to_vec();
    bytes.extend(42_i32.to_le_bytes());
    let parsed = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V4, false).unwrap();
    assert!(parsed.short);
    assert_eq!(parsed.value, 42);
    assert_eq!(parsed.next_offset, 8);

    let bytes = long_chunk(ArchiveVersion::V4, 9, &[1, 2, 3]);
    let parsed = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V4, false).unwrap();
    assert_eq!(parsed.body, 8..11);
    assert_eq!(parsed.header_start, 0);
    assert_eq!(parsed.range(), 0..11);
    assert_eq!(parsed.next_offset, 11);

    let bytes = long_chunk(ArchiveVersion::V5, 9, &[1, 2, 3]);
    let parsed = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V5, false).unwrap();
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
    let chunk = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V2, false).unwrap();
    assert_eq!(verify_checksum(&bytes, &chunk), Ok(ChecksumStatus::Valid));
    *bytes.last_mut().unwrap() ^= 1;
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
    .unwrap();
    assert!(!hidden.visible);

    let definition = fixed_attributes(1, 0xf3, None);
    let definition = super::objects::parse_attributes(
        &definition,
        0..definition.len(),
        0..definition.len(),
        ArchiveVersion::V4,
        &mut Vec::new(),
    )
    .unwrap();
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
    .unwrap();
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
    let bytes = tagged_attributes(&items, 13);
    let parsed = super::objects::parse_attributes(
        &bytes,
        0..bytes.len(),
        10..10 + bytes.len(),
        ArchiveVersion::V8,
        &mut Vec::new(),
    )
    .unwrap();
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
    .unwrap();
    attributes.layer_index = -1;
    attributes.color_source = 2;
    let mut material = vec![descriptor(attributes.clone(), 10)];
    let mut warnings = Vec::new();
    super::objects::resolve_identities(&mut material, &metadata, &mut warnings);
    assert_eq!(material[0].identity.as_ref().unwrap().effective_color, None);

    attributes.color_source = 3;
    attributes.object_mode = 0xf3;
    let mut parent = vec![descriptor(attributes, 20)];
    super::objects::resolve_identities(&mut parent, &metadata, &mut warnings);
    assert_eq!(parent[0].identity.as_ref().unwrap().effective_color, None);
    assert!(parent[0].identity.as_ref().unwrap().definition_member);
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
    .unwrap();
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
        objects[0].identity.as_ref().unwrap().source_id,
        objects[2].identity.as_ref().unwrap().source_id
    );
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("nil object UUID")));
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("duplicate object UUID")));
    assert_eq!(
        objects[0].identity.as_ref().unwrap().class_uuid,
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
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    assert_eq!(anonymous_version(&mut reader).unwrap(), (2, 1));
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
                .unwrap()
                .unwrap()
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
    assert_eq!(parse_eof(&bytes, 32, ArchiveVersion::V1).unwrap(), None);
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
    let first = chunk_at(&parent, 0, parent.len(), ArchiveVersion::V5, false).unwrap();
    let nested = chunk_at(
        &parent,
        first.body.start,
        first.body.end,
        ArchiveVersion::V5,
        false,
    )
    .unwrap();
    assert_eq!(nested.next_offset, first.body.start + child.len());
    let next = chunk_at(
        &parent,
        first.next_offset,
        parent.len(),
        ArchiveVersion::V5,
        false,
    )
    .unwrap();
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
    assert_eq!(checked_count_bytes(3, 4, 12, 100, 0).unwrap(), 12);
    assert!(checked_count_bytes(-1, 4, 12, 100, 0).is_err());
    assert!(checked_count_bytes(4, 4, 12, 100, 0).is_err());
    assert!(checked_count_bytes(3, 4, 12, 2, 0).is_err());
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

fn crc_table(archive: ArchiveVersion, typecode: u32, records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = records.concat();
    body.extend(short_chunk(archive, super::chunks::TCODE_ENDOFTABLE, 0));
    crc_chunk(archive, typecode | TCODE_CRC, &body)
}

fn object_record(archive: ArchiveVersion, object_type: i64, class_uuid: [u8; 16]) -> Vec<u8> {
    let object_type = short_chunk(archive, 0x82a0_0071, object_type);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, &[]);
    let class_end = short_chunk(archive, 0x8202_7fff, 0);
    let class = long_chunk(
        archive,
        0x0002_7ffa,
        &[uuid, class_data, class_end].concat(),
    );
    let object_end = short_chunk(archive, 0x82a0_007f, 0);
    crc_chunk(
        archive,
        0x2000_8070 | TCODE_CRC,
        &[object_type, class, object_end].concat(),
    )
}

fn object_record_without_end(
    archive: ArchiveVersion,
    object_type: i64,
    class_uuid: [u8; 16],
) -> Vec<u8> {
    let object_type = short_chunk(archive, 0x82a0_0071, object_type);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(archive, 0x0002_fffc, &[]);
    let class_end = short_chunk(archive, 0x8202_7fff, 0);
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

fn minimal_document(version: &str, tables: &[Vec<u8>]) -> Vec<u8> {
    let archive = parse_header(&header(version)).unwrap().archive_version;
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
    let summary = RhinoCodec.inspect(&mut Cursor::new(bytes)).unwrap();
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
    let summary = RhinoCodec.inspect(&mut Cursor::new(bytes)).unwrap();
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
        let archive = parse_header(&header(version)).unwrap().archive_version;
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
                },
            )
            .unwrap();
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
        let archive = parse_header(&header(version)).unwrap().archive_version;
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
                },
            )
            .unwrap();
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
        let summary = RhinoCodec.inspect(&mut Cursor::new(bytes.clone())).unwrap();
        assert!(summary.entries.is_empty());
        assert_eq!(summary.container_kind, "3dm-chunks");
        let result = RhinoCodec.decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
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
        RhinoCodec.inspect(&mut Cursor::new(missing)),
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
        RhinoCodec.inspect(&mut Cursor::new(bytes)),
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
fn object_marker_errors_report_the_structural_rule() {
    let archive = ArchiveVersion::V5;
    for object in [object_record_without_end(archive, 1, [0; 16]), {
        let mut bytes = object_record(archive, 1, [0; 16]);
        bytes[12..16].copy_from_slice(&0x82a0_0072_u32.to_le_bytes());
        bytes
    }] {
        let bytes = minimal_document(
            "50",
            &[
                table(archive, 0x1000_0014, &[]),
                table(archive, 0x1000_0015, &[]),
                table(archive, 0x1000_0013, &[object]),
            ],
        );
        let error = RhinoCodec.inspect(&mut Cursor::new(bytes)).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("missing object end") || message.contains("first short child"),
            "unexpected diagnostic: {message}"
        );
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
            RhinoCodec.inspect(&mut Cursor::new(bytes)),
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
    let summary = RhinoCodec.inspect(&mut Cursor::new(bytes)).unwrap();
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
    let scan = super::container::scan(bytes).unwrap();
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
    let summary = RhinoCodec.inspect(&mut Cursor::new(bytes)).unwrap();
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
    assert!(RhinoCodec.inspect(&mut Cursor::new(valid)).is_ok());

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
        RhinoCodec.inspect(&mut Cursor::new(invalid)),
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
    let summary = RhinoCodec.inspect(&mut Cursor::new(bytes)).unwrap();
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
        RhinoCodec.inspect(&mut Cursor::new(short_object)),
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
    let summary = RhinoCodec.inspect(&mut Cursor::new(unknown)).unwrap();
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
    assert_eq!(settings::utf8(&mut utf8_reader).unwrap(), "é");

    let mut utf16_bytes = Vec::new();
    utf16_bytes.extend(3_u32.to_le_bytes());
    utf16_bytes.extend(0xd83d_u16.to_le_bytes());
    utf16_bytes.extend(0xde00_u16.to_le_bytes());
    utf16_bytes.extend(0_u16.to_le_bytes());
    let mut utf16_reader =
        BoundedReader::new(&utf16_bytes, 0, utf16_bytes.len()).expect("bounded UTF-16 reader");
    assert_eq!(settings::utf16(&mut utf16_reader).unwrap(), "😀");

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
    let units = settings::parse_units(&data, &record).unwrap();
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
    let units = settings::parse_units(&data, &record).unwrap();
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
        object_typecodes: std::collections::BTreeMap::new(),
        objects: Vec::new(),
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
        object_typecodes: std::collections::BTreeMap::new(),
        objects: Vec::new(),
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
            short_chunk(archive, 0x8202_7fff, 0),
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
    .unwrap();
    assert_eq!(class_descriptor.class_data_range.len(), payload.len());
    let table = super::container::Table {
        typecode: 0x1000_0011,
        range: 0..data.len(),
        body: 0..data.len(),
        records: vec![record],
        object_typecodes: std::collections::BTreeMap::new(),
        objects: Vec::new(),
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
    settings::parse_setting(&material_data, &material_record, &mut settings_value).unwrap();
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
    settings::parse_setting(&color_data, &color_record, &mut settings_value).unwrap();
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
        settings::parse_setting(&[], &record, &mut settings_value).unwrap();
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
    let scan = super::container::scan(bytes).unwrap();
    let mut context = super::decode::DecodeContext::new(&scan);
    assert!(context.object(0).is_some());
    assert!(context.unknown(0).is_some());
    assert_eq!(context.unit_scale(), None);
    assert_eq!(context.archive(), archive);
    assert!(context.append_link(0, "rhino:curve#1".to_string()));
    assert_eq!(
        context.unknown(0).unwrap().links,
        vec!["rhino:curve#1".to_string()]
    );
    assert!(!context.mark_retained(0));
    assert!(context.mark_decoded(0));
    assert!(!context.mark_decoded(0));
    assert!(!context.mark_failed(0));
    assert_eq!(context.ir_mut().model.bodies.len(), 0);
    context.ir_mut().unknowns[0].links.clear();
    let result = context.commit();
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.severity == Severity::Info));
    assert_eq!(result.ir.unknowns.len(), 1);
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert_eq!(validation.error_count(), 0);
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
    let scan = super::container::scan(bytes).unwrap();
    let offset = scan.objects[0].range.start as u64;
    let class = scan.objects[0].class_uuid.to_string();
    let result = super::decode::decode(&scan);

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
