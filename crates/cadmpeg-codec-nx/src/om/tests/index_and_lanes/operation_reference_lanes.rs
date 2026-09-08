use crate::om::compact::LocatedCompactIndex;
use crate::om::pattern::{PatternRows, PatternScalarEncoding};
use crate::om::pattern_references::{PatternPayloadReferenceLayout, PatternReferences};
use crate::om::projected_references::ProjectedCurveReferences;
use crate::om::scalar::ShiftedScalar;
use crate::om::PatternPayloadTransformLane;

struct ObservedPatternScalar {
    encoding: PatternScalarEncoding,
    value: f64,
    offset: usize,
}

impl PatternPayloadTransformLane {
    fn rows(&self) -> impl Iterator<Item = (Vec<ObservedPatternScalar>, &LocatedCompactIndex)> {
        match &self.rows {
            PatternRows::Scalar(rows) => rows
                .as_slice()
                .iter()
                .map(|row| {
                    let encoding = match row.values.scalar {
                        ShiftedScalar::Binary32(_) => PatternScalarEncoding::Binary32,
                        ShiftedScalar::Binary64(_) => PatternScalarEncoding::Binary64,
                    };
                    (
                        vec![ObservedPatternScalar {
                            encoding,
                            value: row.values.scalar.value(),
                            offset: row.values.offset,
                        }],
                        &row.selector,
                    )
                })
                .collect::<Vec<_>>(),
            PatternRows::Wide(rows) => rows
                .as_slice()
                .iter()
                .map(|row| {
                    let mut values = row
                        .values
                        .first
                        .iter()
                        .map(|value| ObservedPatternScalar {
                            encoding: PatternScalarEncoding::Binary64,
                            value: value.scalar.value(),
                            offset: value.offset,
                        })
                        .collect::<Vec<_>>();
                    values.push(ObservedPatternScalar {
                        encoding: row.values.terminal.scalar.encoding(),
                        value: row.values.terminal.scalar.value(),
                        offset: row.values.terminal.offset,
                    });
                    (values, &row.selector)
                })
                .collect(),
        }
        .into_iter()
    }
}

#[test]
fn om_surface_payload_strings_require_exact_length_utf8_and_terminator() {
    let bytes = b"\x66\x1b\x03\x05Steel\0\xaa\x66\x1b\x03\x02\xc3\x97\0";
    let strings = super::super::surface_payload_strings(bytes);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[0].offset, 0);
    assert_eq!(strings[0].value.as_str(), "Steel");
    assert_eq!(strings[1].offset, 11);
    assert_eq!(strings[1].value.as_str(), "×");

    let truncated = b"\x66\x1b\x03\x05Steel";
    assert!(super::super::surface_payload_strings(truncated).is_empty());
    let invalid_utf8 = b"\x66\x1b\x03\x01\xff\0";
    assert!(super::super::surface_payload_strings(invalid_utf8).is_empty());
    let control = b"\x66\x1b\x03\x01\n\0";
    assert!(super::super::surface_payload_strings(control).is_empty());
}

#[test]
fn om_projected_curve_references_require_one_complete_field() {
    let label = "CPROJ";
    let payload =
        b"\0\x01\x02\xf1\x02\xc8\xf1\x02\xc9\x80\x57\x00\x02\x01\xf1\x02\xca\xff\x01\x02\x02\x7d\0";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let field = ProjectedCurveReferences::read(record).expect("complete field");
    assert_eq!(
        field
            .into_references()
            .iter()
            .map(|reference| (reference.token.value(), reference.offset))
            .collect::<Vec<_>>(),
        [(712, 203), (713, 206), (714, 214)]
    );

    let mut malformed = payload.to_vec();
    malformed[17] = 0x00;
    assert!(ProjectedCurveReferences::read(
        crate::om::operation_record::OperationPayload::new(
            &malformed,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(ProjectedCurveReferences::read(
        crate::om::operation_record::OperationPayload::new(
            &ambiguous,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
}

#[test]
fn om_combined_projected_curve_references_require_the_complete_graph() {
    let label = "CPROJ_CMB";
    let payload = b"\x3c\x32\x01\x02\x32\x01\x04\x36\x01\x33\xf1\x03\x18\x33\xf1\x03\x19\x00\xf1\x03\x1a\x00\x00\x00\x00\x00\x00\xf1\x03\x1b\x16\x01\x02\xf1\x03\x18\x01\x02\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x03\x1c\x00\x81\x5c\x16\x01\x02\xf1\x03\x19\x01\x02\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x03\x1d\x00\x81\x5c\xff\x01\xff\x01\xf1\x03\x1e\xf1\x03\x1f\x04\x02";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let field = ProjectedCurveReferences::read(record).expect("complete graph");
    assert_eq!(
        field
            .into_references()
            .iter()
            .map(|reference| (reference.token.value(), reference.offset))
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
    assert!(ProjectedCurveReferences::read(
        crate::om::operation_record::OperationPayload::new(
            &inconsistent,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());

    let mut malformed = payload.to_vec();
    malformed[84] = 0x00;
    assert!(ProjectedCurveReferences::read(
        crate::om::operation_record::OperationPayload::new(
            &malformed,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(ProjectedCurveReferences::read(
        crate::om::operation_record::OperationPayload::new(
            &ambiguous,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
}

#[test]
fn om_pattern_reference_graph_preserves_nullable_terminal_slot() {
    let label = "Pattern Geometry";
    let nullable = b"\x61\xf1\x1b\x08\xff\x00\xff\x01\xf1\x1b\x09\xf1\x1b\x0a\x61\xf1\x1b\x0b\xff\x00\xff\x01\xf1\x1b\x0c\xf1\x1b\x0d\xff\x62\xf1\x1b\x0e\xf1\x1b\x0f\xff\x00\x00\x01\xf1\x1b\x10\xff\xff\xff\x01";
    let record = crate::om::operation_record::OperationPayload::new(nullable, 200, label).unwrap();
    let field = PatternReferences::read(record).expect("complete graph");
    assert_eq!(
        field.layout(),
        PatternPayloadReferenceLayout::CanonicalGraph
    );
    assert_eq!(
        field
            .into_references()
            .iter()
            .map(|reference| reference.token.value())
            .collect::<Vec<_>>(),
        (6920..=6928).collect::<Vec<_>>()
    );

    let populated = [&nullable[..nullable.len() - 4], b"\xf1\x1b\x11\xff\xff\x01"].concat();
    let field = PatternReferences::read(
        crate::om::operation_record::OperationPayload::new(
            &populated,
            record.payload_offset(),
            "Pattern Feature",
        )
        .unwrap(),
    )
    .expect("populated terminal slot");
    let references = field.into_references();
    assert_eq!(references.len(), 10);
    assert_eq!(references[9].token.value(), 6929);

    let mut malformed = nullable.to_vec();
    malformed[18] = 0x60;
    assert!(PatternReferences::read(
        crate::om::operation_record::OperationPayload::new(
            &malformed,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());

    let compact = b"\x3b\xf1\x1b\x20\xff\x00\x01\xf1\x1b\x21\xf1\x1b\x22\x3b\xf1\x1b\x23\xff\x00\x01\xf1\x1b\x24\xf1\x1b\x25\xff\x3c\xf1\x1b\x26\xf1\x1b\x27\xff\x00\x00\x01\xf1\x1b\x28\xff\xff\xff\x01";
    let field = PatternReferences::read(
        crate::om::operation_record::OperationPayload::new(
            compact,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    )
    .expect("complete compact graph");
    assert_eq!(field.layout(), PatternPayloadReferenceLayout::CompactGraph);
    assert_eq!(
        field
            .into_references()
            .iter()
            .map(|reference| reference.token.value())
            .collect::<Vec<_>>(),
        (0x1b20..=0x1b28).collect::<Vec<_>>()
    );
}

#[test]
fn om_pattern_transform_lanes_require_counted_family_rows() {
    let feature_payload = b"\xaa\x01\x03\x60\x01\x00\x00\x50\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\x60\x01\x00\x00\xd0\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x9f\xfe\x01\x02\x00\x00\xff\x00\x00\x5f\x00\x00\x01";
    let label = "Pattern Feature";
    let record =
        crate::om::operation_record::OperationPayload::new(feature_payload, 200, label).unwrap();
    let lane = super::super::pattern_payload_transform_lane(record).expect("feature lane");
    assert_eq!(lane.offset, 201);
    assert_eq!(lane.row_schema_index.get(), 0x60);
    assert!(matches!(lane.rows, PatternRows::Scalar(_)));
    assert_eq!(lane.rows().count() + 1, 3);
    assert_eq!(
        lane.rows()
            .flat_map(|(values, _)| values)
            .map(|token| token.encoding)
            .collect::<Vec<_>>(),
        [
            PatternScalarEncoding::Binary32,
            PatternScalarEncoding::Binary32,
        ]
    );
    assert_eq!(
        lane.rows()
            .flat_map(|(values, _)| values)
            .map(|token| token.value)
            .collect::<Vec<_>>(),
        [3.3125, -3.3125]
    );
    assert_eq!(
        lane.rows()
            .flat_map(|(values, _)| values)
            .map(|token| token.offset)
            .collect::<Vec<_>>(),
        [207, 237]
    );
    assert_eq!(
        lane.rows()
            .map(|(_, selector)| selector)
            .map(|token| token.atom.value())
            .collect::<Vec<_>>(),
        [2, 8190]
    );
    assert_eq!(
        lane.rows()
            .map(|(_, selector)| selector)
            .map(|token| token.atom.raw().to_vec())
            .collect::<Vec<_>>(),
        [vec![0x02], vec![0x9f, 0xfe]]
    );
    assert_eq!(
        lane.rows()
            .map(|(_, selector)| selector)
            .map(|token| token.offset)
            .collect::<Vec<_>>(),
        [225, 255]
    );

    let geometry_payload = b"\x01\x03\x60\x01\x00\x00\x00\x00\x01\x00\x30\x60\x80\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\x60\x01\x00\x00\x00\x00\x01\x00\x30\x70\x80\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x03\x01\x02\x00\x00\xff\x00\x00\x5f\x00\x00\x01";
    let geometry_record = crate::om::operation_record::OperationPayload::new(
        geometry_payload,
        record.payload_offset(),
        "Pattern Geometry",
    )
    .unwrap();
    let lane =
        super::super::pattern_payload_transform_lane(geometry_record).expect("geometry lane");
    assert_eq!(lane.row_schema_index.get(), 0x60);
    assert!(matches!(lane.rows, PatternRows::Scalar(_)));
    assert_eq!(
        lane.rows()
            .flat_map(|(values, _)| values)
            .map(|token| token.encoding)
            .collect::<Vec<_>>(),
        [
            PatternScalarEncoding::Binary64,
            PatternScalarEncoding::Binary64,
        ]
    );
    assert_eq!(
        lane.rows()
            .flat_map(|(values, _)| values)
            .map(|token| token.value)
            .collect::<Vec<_>>(),
        [132.0, 264.0]
    );
    assert_eq!(
        lane.rows()
            .map(|(_, selector)| selector)
            .map(|token| token.atom.value())
            .collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(
        lane.rows()
            .map(|(_, selector)| selector)
            .map(|token| token.atom.raw().to_vec())
            .collect::<Vec<_>>(),
        [vec![0x02], vec![0x03]]
    );
    assert_eq!(
        lane.rows()
            .map(|(_, selector)| selector)
            .map(|token| token.offset)
            .collect::<Vec<_>>(),
        [228, 262]
    );

    let schema_relative_payload = b"\x01\x04\
        \x3d\x01\x00\x00\x50\x9e\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\
        \x3d\x01\x00\x00\x50\xae\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x03\x01\x02\x00\x00\xff\x00\x00\
        \x3d\x01\x00\x00\x30\xb6\x80\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x04\x01\x03\x00\x00\xff\x00\x00\
        \x3c\x00\x00\x01";
    let relative_lane = super::super::pattern_payload_transform_lane(
        crate::om::operation_record::OperationPayload::new(
            schema_relative_payload,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    )
    .expect("schema-relative feature lane");
    assert_eq!(relative_lane.row_schema_index.get(), 0x3d);
    assert!(matches!(relative_lane.rows, PatternRows::Scalar(_)));
    assert_eq!(relative_lane.rows().count() + 1, 4);
    assert_eq!(
        relative_lane
            .rows()
            .flat_map(|(values, _)| values)
            .map(|token| token.encoding)
            .collect::<Vec<_>>(),
        [
            PatternScalarEncoding::Binary32,
            PatternScalarEncoding::Binary32,
            PatternScalarEncoding::Binary64,
        ]
    );
    assert_eq!(
        relative_lane
            .rows()
            .map(|(_, selector)| selector)
            .map(|token| token.atom.value())
            .collect::<Vec<_>>(),
        [2, 3, 4]
    );

    let wide_payload = b"\x01\x03\
        \x35\x2f\xf3\xc6\xef\x37\x2f\xe9\x60\xb0\x0e\x6f\x0e\x13\x44\x54\xfd\x00\x00\x30\x0e\x6f\x0e\x13\x44\x54\xfd\x2f\xf3\xc6\xef\x37\x2f\xe9\x60\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\
        \x35\xb0\x09\xe3\x77\x9b\x97\xf4\xb9\x30\x02\xcf\x23\x04\x75\x5a\x46\x00\x00\xb0\x02\xcf\x23\x04\x75\x5a\x46\xb0\x09\xe3\x77\x9b\x97\xf4\xb9\x00\x00\x00\x00\x50\x0f\xff\xff\x00\x00\x00\x00\x01\x01\x03\x03\x01\x02\x00\x00\xff\x00\x00\
        \x34\x00\x00\x02";
    let wide_lane = super::super::pattern_payload_transform_lane(
        crate::om::operation_record::OperationPayload::new(
            wide_payload,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    )
    .expect("wide feature lane");
    assert_eq!(wide_lane.row_schema_index.get(), 0x35);
    assert!(matches!(wide_lane.rows, PatternRows::Wide(_)));
    assert_eq!(wide_lane.rows().count() + 1, 3);
    assert_eq!(
        wide_lane
            .rows()
            .map(|(values, _)| values.len())
            .sum::<usize>(),
        10
    );
    assert_eq!(
        wide_lane
            .rows()
            .flat_map(|(values, _)| values)
            .map(|token| token.encoding)
            .collect::<Vec<_>>(),
        [
            PatternScalarEncoding::Binary64,
            PatternScalarEncoding::Binary64,
            PatternScalarEncoding::Binary64,
            PatternScalarEncoding::Binary64,
            PatternScalarEncoding::ExactOne,
            PatternScalarEncoding::Binary64,
            PatternScalarEncoding::Binary64,
            PatternScalarEncoding::Binary64,
            PatternScalarEncoding::Binary64,
            PatternScalarEncoding::Binary32,
        ]
    );
    assert_eq!(
        wide_lane
            .rows()
            .map(|(_, selector)| selector)
            .map(|token| token.atom.value())
            .collect::<Vec<_>>(),
        [2, 3]
    );

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
    assert!(super::super::pattern_payload_transform_lane(
        crate::om::operation_record::OperationPayload::new(
            &zero_terminal_value,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());

    let mut changed_schema = feature_payload.to_vec();
    let second_row = changed_schema
        .windows(4)
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == [0x60, 0x01, 0x00, 0x00]).then_some(offset))
        .nth(1)
        .expect("second row");
    changed_schema[second_row] = 0x61;
    assert!(super::super::pattern_payload_transform_lane(
        crate::om::operation_record::OperationPayload::new(
            &changed_schema,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());

    let mut wrong_ordinal = feature_payload.to_vec();
    wrong_ordinal[29] = 2;
    assert!(super::super::pattern_payload_transform_lane(
        crate::om::operation_record::OperationPayload::new(
            &wrong_ordinal,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
    assert!(super::super::pattern_payload_transform_lane(
        crate::om::operation_record::OperationPayload::new(
            &feature_payload[..feature_payload.len() - 1],
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
}
