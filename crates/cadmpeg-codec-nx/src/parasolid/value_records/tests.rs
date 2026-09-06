// SPDX-License-Identifier: Apache-2.0

#[test]
fn parasolid_entity_54_strings_require_exact_length_and_terminator() {
    let mut bytes = vec![0xaa, 0x00, 0x54];
    bytes.extend_from_slice(&8u32.to_be_bytes());
    bytes.extend_from_slice(&17u16.to_be_bytes());
    bytes.extend_from_slice(b"deadbeef\0");
    bytes.extend_from_slice(&[0xbb, 0x00, 0x54, 0, 0, 0, 3, 0, 18, b'a', b'b', b'c', 1]);

    let records = crate::parasolid::value_records::entity_value_records(&bytes).strings;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].byte_len, 17);
    assert_eq!(records[0].xmt, 17);
    assert_eq!(records[0].value.as_str(), "deadbeef");
    assert_eq!(
        crate::parasolid::value_records::entity_54_string_record_at(&bytes, 1),
        Some(records[0].clone())
    );
    assert!(crate::parasolid::value_records::entity_54_string_record_at(&bytes, bytes.len() - 12).is_none());

    let minimum = [0, 0x54, 0, 0, 0, 1, 0, 2, b'a', 0];
    assert_eq!(
        crate::parasolid::value_records::entity_value_records(&minimum).strings[0].value.as_str(),
        "a"
    );
}

#[test]
fn parasolid_entity_52_integers_require_complete_counted_values() {
    let mut bytes = vec![0xaa, 0x00, 0x52];
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&17u16.to_be_bytes());
    bytes.extend_from_slice(&3u32.to_be_bytes());
    bytes.extend_from_slice(&u32::MAX.to_be_bytes());

    let records = crate::parasolid::value_records::entity_value_records(&bytes).integers;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].xmt, 17);
    assert_eq!(records[0].value.as_slice(), [3, u32::MAX]);
    assert_eq!(records[0].byte_len, 16);
    assert_eq!(
        crate::parasolid::value_records::entity_52_integer_record_at(&bytes, 1),
        Some(records[0].clone())
    );
    assert!(
        crate::parasolid::value_records::entity_value_records(&bytes[..bytes.len() - 1])
            .integers
            .is_empty()
    );
    assert!(crate::parasolid::value_records::entity_52_integer_record_at(&bytes[..bytes.len() - 1], 1).is_none());
}

#[test]
fn parasolid_entity_53_doubles_require_complete_finite_values() {
    let mut bytes = vec![0xaa, 0x00, 0x53, 0xff];
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&18u16.to_be_bytes());
    bytes.extend_from_slice(&0.001f64.to_be_bytes());
    bytes.extend_from_slice(&0.25f64.to_be_bytes());

    let records = crate::parasolid::value_records::entity_value_records(&bytes).doubles;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].xmt, 18);
    assert_eq!(records[0].value.as_slice(), [0.001, 0.25]);
    assert_eq!(records[0].byte_len, 25);
    assert_eq!(
        crate::parasolid::value_records::entity_53_double_record_at(&bytes, 1),
        Some(records[0].clone())
    );

    let last = bytes.len() - 8;
    bytes[last..].copy_from_slice(&f64::NAN.to_be_bytes());
    assert!(crate::parasolid::value_records::entity_value_records(&bytes)
        .doubles
        .is_empty());
    assert!(crate::parasolid::value_records::entity_53_double_record_at(&bytes, 1).is_none());
}

#[test]
fn parasolid_transformable_attribute_values_preserve_vector_and_axis_grouping() {
    let vector_record = |tag: u8, xmt: u16, vectors: &[[f64; 3]]| {
        let mut bytes = vec![0x00, tag];
        bytes.extend_from_slice(
            &u32::try_from(vectors.len())
                .expect("test vector count fits u32")
                .to_be_bytes(),
        );
        bytes.extend_from_slice(&xmt.to_be_bytes());
        for vector in vectors {
            for component in vector {
                bytes.extend_from_slice(&component.to_be_bytes());
            }
        }
        bytes
    };
    let vectors = [[1.0, 2.0, 3.0], [-4.0, 5.0, 6.0]];

    let points = crate::parasolid::value_records::entity_value_records(&vector_record(0x55, 20, &vectors)).points;
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].value.as_slice(), vectors);
    let vector_values =
        crate::parasolid::value_records::entity_value_records(&vector_record(0x56, 21, &vectors)).vectors;
    assert_eq!(vector_values.len(), 1);
    assert_eq!(vector_values[0].value.as_slice(), vectors);
    let directions =
        crate::parasolid::value_records::entity_value_records(&vector_record(0x59, 22, &vectors)).directions;
    assert_eq!(directions.len(), 1);
    assert_eq!(directions[0].value.as_slice(), vectors);

    let four_vectors = [vectors[0], vectors[1], [7.0, 8.0, 9.0], [0.0, 1.0, 0.0]];
    let axes = crate::parasolid::value_records::entity_value_records(&vector_record(0x57, 23, &four_vectors)).axes;
    assert_eq!(axes.len(), 1);
    assert_eq!(
        axes[0].value.as_slice(),
        [
            [four_vectors[0], four_vectors[1]],
            [four_vectors[2], four_vectors[3]],
        ]
    );
    assert!(
        crate::parasolid::value_records::entity_value_records(&vector_record(0x57, 23, &four_vectors[..3]))
            .axes
            .is_empty()
    );

    let mut nonfinite = vectors;
    nonfinite[1][2] = f64::INFINITY;
    assert!(
        crate::parasolid::value_records::entity_value_records(&vector_record(0x55, 20, &nonfinite))
            .points
            .is_empty()
    );
}

#[test]
fn parasolid_entity_value_records_dispatches_all_value_families() {
    let vector_record = |tag: u8, xmt: u16, vectors: &[[f64; 3]]| {
        let mut bytes = vec![0x00, tag];
        bytes.extend_from_slice(
            &u32::try_from(vectors.len())
                .expect("test vector count fits u32")
                .to_be_bytes(),
        );
        bytes.extend_from_slice(&xmt.to_be_bytes());
        for vector in vectors {
            for component in vector {
                bytes.extend_from_slice(&component.to_be_bytes());
            }
        }
        bytes
    };
    let vectors = [[1.0, 2.0, 3.0]];
    let mut bytes = Vec::new();

    let mut integers = vec![0x00, 0x52];
    integers.extend_from_slice(&2u32.to_be_bytes());
    integers.extend_from_slice(&10u16.to_be_bytes());
    integers.extend_from_slice(&3u32.to_be_bytes());
    integers.extend_from_slice(&u32::MAX.to_be_bytes());
    bytes.extend(integers);

    let mut doubles = vec![0x00, 0x53];
    doubles.extend_from_slice(&1u32.to_be_bytes());
    doubles.extend_from_slice(&11u16.to_be_bytes());
    doubles.extend_from_slice(&0.25f64.to_be_bytes());
    bytes.extend(doubles);

    let mut string = vec![0x00, 0x54];
    string.extend_from_slice(&5u32.to_be_bytes());
    string.extend_from_slice(&12u16.to_be_bytes());
    string.extend_from_slice(b"label");
    string.push(0);
    bytes.extend(string);
    bytes.extend(vector_record(0x55, 13, &vectors));
    bytes.extend(vector_record(0x56, 14, &vectors));
    bytes.extend(vector_record(0x57, 15, &[vectors[0], [4.0, 5.0, 6.0]]));

    let mut tags = vec![0x00, 0x58];
    tags.extend_from_slice(&1u32.to_be_bytes());
    tags.extend_from_slice(&16u16.to_be_bytes());
    tags.extend_from_slice(&17u32.to_be_bytes());
    bytes.extend(tags);
    bytes.extend(vector_record(0x59, 17, &vectors));

    let mut unicode = vec![0x00, 0x62];
    unicode.extend_from_slice(&2u32.to_be_bytes());
    unicode.extend_from_slice(&18u16.to_be_bytes());
    unicode.extend_from_slice(&(b'N' as u16).to_be_bytes());
    unicode.extend_from_slice(&(b'X' as u16).to_be_bytes());
    bytes.extend(unicode);

    let records = crate::parasolid::value_records::entity_value_records(&bytes);
    assert_eq!(records.integers.len(), 1);
    assert_eq!(records.doubles.len(), 1);
    assert_eq!(records.strings.len(), 1);
    assert_eq!(records.points.len(), 1);
    assert_eq!(records.vectors.len(), 1);
    assert_eq!(records.axes.len(), 1);
    assert_eq!(records.tags.len(), 1);
    assert_eq!(records.directions.len(), 1);
    assert_eq!(records.unicode.len(), 1);
    assert_eq!(records.integers[0].value.as_slice(), [3, u32::MAX]);
    assert_eq!(records.doubles[0].value.as_slice(), [0.25]);
    assert_eq!(records.strings[0].value.as_str(), "label");
    assert_eq!(records.axes[0].value.as_slice().len(), 1);
    assert_eq!(records.tags[0].value.as_slice(), [17]);
    assert_eq!(records.unicode[0].value.as_str(), "NX");
}

#[test]
fn parasolid_value_scan_does_not_admit_nested_counted_candidates() {
    let mut outer = vec![0x00, 0x52];
    outer.extend_from_slice(&4u32.to_be_bytes());
    outer.extend_from_slice(&10u16.to_be_bytes());
    outer.extend_from_slice(&[0x00, 0x53]);
    outer.extend_from_slice(&1u32.to_be_bytes());
    outer.extend_from_slice(&20u16.to_be_bytes());
    outer.extend_from_slice(&0.25f64.to_be_bytes());

    let records = crate::parasolid::value_records::entity_value_records(&outer);
    assert_eq!(records.integers.len(), 1);
    assert_eq!(records.integers[0].xmt, 10);
    assert_eq!(records.integers[0].value.as_slice().len(), 4);
    assert!(records.doubles.is_empty());
}

#[test]
fn parasolid_tag_and_unicode_attribute_values_require_complete_counted_lanes() {
    let tags = [
        0x00, 0x58, 0, 0, 0, 2, 0, 24, 0, 0, 0, 7, 0xff, 0xff, 0xff, 0xff,
    ];
    let records = crate::parasolid::value_records::entity_value_records(&tags).tags;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].xmt, 24);
    assert_eq!(records[0].value.as_slice(), [7, u32::MAX]);
    assert!(
        crate::parasolid::value_records::entity_value_records(&tags[..tags.len() - 1])
            .tags
            .is_empty()
    );

    let code_units = [b'N' as u16, b'X' as u16, 0xd83d, 0xde80];
    let mut unicode = vec![0x00, 0x62, 0xff];
    unicode.extend_from_slice(&4u32.to_be_bytes());
    unicode.extend_from_slice(&[0xff, 0xff, 0x00, 0x01]);
    for code_unit in code_units {
        unicode.extend_from_slice(&code_unit.to_be_bytes());
    }
    let records = crate::parasolid::value_records::entity_value_records(&unicode).unicode;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].xmt, 32_768);
    assert_eq!(
        records[0].value.as_str().encode_utf16().collect::<Vec<_>>(),
        code_units
    );
    assert_eq!(records[0].value.as_str(), "NX🚀");
    assert!(
        crate::parasolid::value_records::entity_value_records(&unicode[..unicode.len() - 1])
            .unicode
            .is_empty()
    );

    let mut invalid = unicode;
    invalid[15..17].copy_from_slice(&0xd800u16.to_be_bytes());
    invalid[17..19].copy_from_slice(&0x0041u16.to_be_bytes());
    assert!(crate::parasolid::value_records::entity_value_records(&invalid)
        .unicode
        .is_empty());
}
