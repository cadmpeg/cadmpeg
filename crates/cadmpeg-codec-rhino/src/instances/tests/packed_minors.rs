// SPDX-License-Identifier: Apache-2.0
use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump as bytes;

/// A packed instance-definition record whose fields stop at `minor`.
fn packed_record(archive: ArchiveVersion, minor: u8, unit: u32, meters_per_unit: f64) -> Vec<u8> {
    let mut direct = vec![0x10 | minor];
    direct.extend([0x61; 16]);
    direct.extend(1_i32.to_le_bytes());
    direct.extend([0x62; 16]);
    direct.extend(bytes::utf16_bytes("packed"));
    for _ in 0..3 {
        direct.extend(bytes::utf16_bytes(""));
    }
    for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 1.0] {
        direct.extend(value.to_le_bytes());
    }
    direct.extend(0_u32.to_le_bytes()); // static definition
    direct.extend(bytes::utf16_bytes("")); // no linked path
    if minor >= 1 {
        direct.extend([0_u8; 48]); // linked-file checksum
    }
    if minor >= 2 {
        direct.extend(unit.to_le_bytes());
    }
    if minor >= 3 {
        direct.extend(meters_per_unit.to_le_bytes());
        direct.push(0); // legacy relative-path flag
    }
    let mut payload = direct.clone();
    if minor >= 4 {
        payload.extend(bytes::unit_detail(archive, 2, 0.001));
    }
    let mut tail = Vec::new();
    if minor >= 5 {
        tail.extend(1_i32.to_le_bytes()); // nested linked-definition depth
    }
    direct.extend(&tail);
    payload.extend(tail);
    payload.extend(crc32fast::hash(&direct).to_le_bytes());
    let class = [
        bytes::crc_chunk(archive, 0x0002_fffb, &bytes::INSTANCE_DEFINITION_CLASS),
        bytes::long_chunk(archive, 0x0002_fffc, &payload),
        bytes::short_chunk(archive, 0x8002_7fff, 0),
    ]
    .concat();
    let mut record = bytes::long_chunk(archive, 0x0002_7ffa, &class);
    record.extend(0_u32.to_le_bytes());
    bytes::long_chunk(archive, 0x2000_8076, &record)
}

#[test]
fn archives_below_50_read_every_packed_minor() {
    for (version, archive) in [
        ("2", ArchiveVersion::V2),
        ("3", ArchiveVersion::V3),
        ("4", ArchiveVersion::V4),
        ("50", ArchiveVersion::V5),
    ] {
        for minor in 0..=5_u8 {
            let record = packed_record(archive, minor, 8, 0.0254);
            let document = bytes::document_with_definitions(version, archive, &[record], &[]);
            let scan = crate::container::scan_owned(document).expect("packed definition document");
            let definitions = scan.definitions.definitions();
            assert_eq!(definitions.len(), 1, "archive={archive:?} minor={minor}");
            let definition = &definitions[0];
            assert_eq!(
                definition.members,
                [crate::wire::Uuid::from_wire([0x62; 16])]
            );
            assert_eq!(definition.name, "packed");
            let expected_unit = match minor {
                0 | 1 => 0,
                2 | 3 => 8,
                _ => 2,
            };
            assert_eq!(definition.units.unit, expected_unit, "minor={minor}");
            assert_eq!(definition.linked_depth, i32::from(minor >= 5));
        }
    }
}

#[test]
fn packed_custom_units_take_the_minor_3_scale_when_positive() {
    for (minor, meters_per_unit, unit, bits) in [
        (2_u8, 0.5_f64, 4_u32, 1.0_f64.to_bits()),
        (3, 0.5, 11, 0.5_f64.to_bits()),
        (3, 0.0, 4, 1.0_f64.to_bits()),
    ] {
        let record = packed_record(ArchiveVersion::V4, minor, 11, meters_per_unit);
        let document = bytes::document_with_definitions("4", ArchiveVersion::V4, &[record], &[]);
        let scan = crate::container::scan_owned(document).expect("packed definition document");
        let units = &scan.definitions.definitions()[0].units;
        assert_eq!(units.unit, unit, "minor={minor} scale={meters_per_unit}");
        assert_eq!(units.meters_per_unit_bits(), bits);
    }
}
