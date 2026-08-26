#[test]
fn om_surface_payload_strings_require_exact_length_utf8_and_terminator() {
    let bytes = b"\x66\x1b\x03\x05Steel\0\xaa\x66\x1b\x03\x02\xc3\x97\0";
    let strings = super::super::surface_payload_strings(bytes);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[0].offset, 0);
    assert_eq!(strings[0].value, "Steel");
    assert_eq!(strings[1].offset, 11);
    assert_eq!(strings[1].value, "×");

    let truncated = b"\x66\x1b\x03\x05Steel";
    assert!(super::super::surface_payload_strings(truncated).is_empty());
    let invalid_utf8 = b"\x66\x1b\x03\x01\xff\0";
    assert!(super::super::surface_payload_strings(invalid_utf8).is_empty());
    let control = b"\x66\x1b\x03\x01\n\0";
    assert!(super::super::surface_payload_strings(control).is_empty());
}

#[test]
fn om_projected_curve_references_require_one_complete_field() {
    let label = super::super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "CPROJ",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload =
        b"\0\x01\x02\xf1\x02\xc8\xf1\x02\xc9\x80\x57\x00\x02\x01\xf1\x02\xca\xff\x01\x02\x02\x7d\0";
    let record = super::super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = super::super::projected_curve_payload_references(record).expect("complete field");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| (reference.object_index, reference.offset))
            .collect::<Vec<_>>(),
        [(712, 203), (713, 206), (714, 214)]
    );

    let mut malformed = payload.to_vec();
    malformed[17] = 0x00;
    assert!(
        super::super::projected_curve_payload_references(super::super::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        super::super::projected_curve_payload_references(super::super::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_combined_projected_curve_references_require_the_complete_graph() {
    let label = super::super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "CPROJ_CMB",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x3c\x32\x01\x02\x32\x01\x04\x36\x01\x33\xf1\x03\x18\x33\xf1\x03\x19\x00\xf1\x03\x1a\x00\x00\x00\x00\x00\x00\xf1\x03\x1b\x16\x01\x02\xf1\x03\x18\x01\x02\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x03\x1c\x00\x81\x5c\x16\x01\x02\xf1\x03\x19\x01\x02\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x03\x1d\x00\x81\x5c\xff\x01\xff\x01\xf1\x03\x1e\xf1\x03\x1f\x04\x02";
    let record = super::super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = super::super::projected_curve_payload_references(record).expect("complete graph");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| (reference.object_index, reference.offset))
            .collect::<Vec<_>>(),
        [
            (792, 210),
            (793, 214),
            (794, 218),
            (795, 227),
            (796, 246),
            (797, 268),
            (798, 278),
            (799, 281),
        ]
    );

    let mut inconsistent = payload.to_vec();
    inconsistent[35] = 0x19;
    assert!(
        super::super::projected_curve_payload_references(super::super::OperationRecord {
            bytes: &inconsistent,
            payload: &inconsistent,
            ..record
        })
        .is_none()
    );

    let mut malformed = payload.to_vec();
    malformed[84] = 0x00;
    assert!(
        super::super::projected_curve_payload_references(super::super::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        super::super::projected_curve_payload_references(super::super::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_pattern_reference_graph_preserves_nullable_terminal_slot() {
    let label = super::super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "Pattern Geometry",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let nullable = b"\x61\xf1\x1b\x08\xff\x00\xff\x01\xf1\x1b\x09\xf1\x1b\x0a\x61\xf1\x1b\x0b\xff\x00\xff\x01\xf1\x1b\x0c\xf1\x1b\x0d\xff\x62\xf1\x1b\x0e\xf1\x1b\x0f\xff\x00\x00\x01\xf1\x1b\x10\xff\xff\xff\x01";
    let record = super::super::OperationRecord {
        offset: 100,
        bytes: nullable,
        payload_offset: 200,
        payload: nullable,
        label,
    };
    let field = super::super::pattern_payload_references(record).expect("complete graph");
    assert_eq!(
        field.layout,
        super::super::PatternPayloadReferenceLayout::CanonicalGraph
    );
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        (6920..=6928).collect::<Vec<_>>()
    );

    let populated = [&nullable[..nullable.len() - 4], b"\xf1\x1b\x11\xff\xff\x01"].concat();
    let field = super::super::pattern_payload_references(super::super::OperationRecord {
        label: super::super::OperationLabel {
            value: "Pattern Feature",
            ..label
        },
        bytes: &populated,
        payload: &populated,
        ..record
    })
    .expect("populated terminal slot");
    assert_eq!(field.references.len(), 10);
    assert_eq!(field.references[9].object_index, 6929);

    let mut malformed = nullable.to_vec();
    malformed[18] = 0x60;
    assert!(
        super::super::pattern_payload_references(super::super::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let compact = b"\x3b\xf1\x1b\x20\xff\x00\x01\xf1\x1b\x21\xf1\x1b\x22\x3b\xf1\x1b\x23\xff\x00\x01\xf1\x1b\x24\xf1\x1b\x25\xff\x3c\xf1\x1b\x26\xf1\x1b\x27\xff\x00\x00\x01\xf1\x1b\x28\xff\xff\xff\x01";
    let field = super::super::pattern_payload_references(super::super::OperationRecord {
        bytes: compact,
        payload: compact,
        ..record
    })
    .expect("complete compact graph");
    assert_eq!(
        field.layout,
        super::super::PatternPayloadReferenceLayout::CompactGraph
    );
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        (0x1b20..=0x1b28).collect::<Vec<_>>()
    );
}

#[test]
fn om_pattern_transform_lanes_require_counted_family_rows() {
    let feature_payload = b"\xaa\x01\x03\x60\x01\x00\x00\x50\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\x60\x01\x00\x00\xd0\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x9f\xfe\x01\x02\x00\x00\xff\x00\x00\x5f\x00\x00\x01";
    let label = super::super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "Pattern Feature",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let record = super::super::OperationRecord {
        offset: 100,
        bytes: feature_payload,
        payload_offset: 200,
        payload: feature_payload,
        label,
    };
    let lane = super::super::pattern_payload_transform_lane(record).expect("feature lane");
    assert_eq!(lane.offset, 201);
    assert_eq!(lane.row_schema_index, 0x60);
    assert_eq!(
        lane.layout,
        super::super::PatternTransformLayout::ScalarRows
    );
    assert_eq!(lane.declared_count, 3);
    assert_eq!(
        lane.encodings,
        [
            super::super::PatternTransformEncoding::Binary32,
            super::super::PatternTransformEncoding::Binary32,
        ]
    );
    assert_eq!(lane.values, [3.3125, -3.3125]);
    assert_eq!(lane.value_offsets, [207, 237]);
    assert_eq!(lane.selectors, [2, 8190]);
    assert_eq!(lane.raw_selectors, [vec![0x02], vec![0x9f, 0xfe]]);
    assert_eq!(lane.selector_offsets, [225, 255]);

    let geometry_payload = b"\x01\x03\x60\x01\x00\x00\x00\x00\x01\x00\x30\x60\x80\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\x60\x01\x00\x00\x00\x00\x01\x00\x30\x70\x80\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x03\x01\x02\x00\x00\xff\x00\x00\x5f\x00\x00\x01";
    let geometry_record = super::super::OperationRecord {
        label: super::super::OperationLabel {
            value: "Pattern Geometry",
            ..label
        },
        bytes: geometry_payload,
        payload: geometry_payload,
        ..record
    };
    let lane =
        super::super::pattern_payload_transform_lane(geometry_record).expect("geometry lane");
    assert_eq!(lane.row_schema_index, 0x60);
    assert_eq!(
        lane.layout,
        super::super::PatternTransformLayout::ScalarRows
    );
    assert_eq!(
        lane.encodings,
        [
            super::super::PatternTransformEncoding::Binary64,
            super::super::PatternTransformEncoding::Binary64,
        ]
    );
    assert_eq!(lane.values, [132.0, 264.0]);
    assert_eq!(lane.selectors, [2, 3]);
    assert_eq!(lane.raw_selectors, [vec![0x02], vec![0x03]]);
    assert_eq!(lane.selector_offsets, [228, 262]);

    let schema_relative_payload = b"\x01\x04\
        \x3d\x01\x00\x00\x50\x9e\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\
        \x3d\x01\x00\x00\x50\xae\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x03\x01\x02\x00\x00\xff\x00\x00\
        \x3d\x01\x00\x00\x30\xb6\x80\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x04\x01\x03\x00\x00\xff\x00\x00\
        \x3c\x00\x00\x01";
    let relative_lane =
        super::super::pattern_payload_transform_lane(super::super::OperationRecord {
            bytes: schema_relative_payload,
            payload: schema_relative_payload,
            ..record
        })
        .expect("schema-relative feature lane");
    assert_eq!(relative_lane.row_schema_index, 0x3d);
    assert_eq!(
        relative_lane.layout,
        super::super::PatternTransformLayout::ScalarRows
    );
    assert_eq!(relative_lane.declared_count, 4);
    assert_eq!(
        relative_lane.encodings,
        [
            super::super::PatternTransformEncoding::Binary32,
            super::super::PatternTransformEncoding::Binary32,
            super::super::PatternTransformEncoding::Binary64,
        ]
    );
    assert_eq!(relative_lane.selectors, [2, 3, 4]);

    let wide_payload = b"\x01\x03\
        \x35\x2f\xf3\xc6\xef\x37\x2f\xe9\x60\xb0\x0e\x6f\x0e\x13\x44\x54\xfd\x00\x00\x30\x0e\x6f\x0e\x13\x44\x54\xfd\x2f\xf3\xc6\xef\x37\x2f\xe9\x60\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\
        \x35\xb0\x09\xe3\x77\x9b\x97\xf4\xb9\x30\x02\xcf\x23\x04\x75\x5a\x46\x00\x00\xb0\x02\xcf\x23\x04\x75\x5a\x46\xb0\x09\xe3\x77\x9b\x97\xf4\xb9\x00\x00\x00\x00\x50\x0f\xff\xff\x00\x00\x00\x00\x01\x01\x03\x03\x01\x02\x00\x00\xff\x00\x00\
        \x34\x00\x00\x02";
    let wide_lane = super::super::pattern_payload_transform_lane(super::super::OperationRecord {
        bytes: wide_payload,
        payload: wide_payload,
        ..record
    })
    .expect("wide feature lane");
    assert_eq!(wide_lane.row_schema_index, 0x35);
    assert_eq!(
        wide_lane.layout,
        super::super::PatternTransformLayout::WideRows
    );
    assert_eq!(wide_lane.declared_count, 3);
    assert_eq!(wide_lane.values.len(), 10);
    assert_eq!(
        wide_lane.encodings,
        [
            super::super::PatternTransformEncoding::Binary64,
            super::super::PatternTransformEncoding::Binary64,
            super::super::PatternTransformEncoding::Binary64,
            super::super::PatternTransformEncoding::Binary64,
            super::super::PatternTransformEncoding::ExactOne,
            super::super::PatternTransformEncoding::Binary64,
            super::super::PatternTransformEncoding::Binary64,
            super::super::PatternTransformEncoding::Binary64,
            super::super::PatternTransformEncoding::Binary64,
            super::super::PatternTransformEncoding::Binary32,
        ]
    );
    assert_eq!(wide_lane.selectors, [2, 3]);

    let mut zero_terminal_value = wide_payload.to_vec();
    let terminal_value = zero_terminal_value
        .windows(12)
        .position(|bytes| {
            bytes
                == [
                    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x03,
                ]
        })
        .expect("exact-one terminal value")
        + 4;
    zero_terminal_value[terminal_value] = 0x00;
    assert!(
        super::super::pattern_payload_transform_lane(super::super::OperationRecord {
            bytes: &zero_terminal_value,
            payload: &zero_terminal_value,
            ..record
        })
        .is_none()
    );

    let mut changed_schema = feature_payload.to_vec();
    let second_row = changed_schema
        .windows(4)
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == [0x60, 0x01, 0x00, 0x00]).then_some(offset))
        .nth(1)
        .expect("second row");
    changed_schema[second_row] = 0x61;
    assert!(
        super::super::pattern_payload_transform_lane(super::super::OperationRecord {
            bytes: &changed_schema,
            payload: &changed_schema,
            ..record
        })
        .is_none()
    );

    let mut wrong_ordinal = feature_payload.to_vec();
    wrong_ordinal[29] = 2;
    assert!(
        super::super::pattern_payload_transform_lane(super::super::OperationRecord {
            bytes: &wrong_ordinal,
            payload: &wrong_ordinal,
            ..record
        })
        .is_none()
    );
    assert!(
        super::super::pattern_payload_transform_lane(super::super::OperationRecord {
            bytes: &feature_payload[..feature_payload.len() - 1],
            payload: &feature_payload[..feature_payload.len() - 1],
            ..record
        })
        .is_none()
    );
}
