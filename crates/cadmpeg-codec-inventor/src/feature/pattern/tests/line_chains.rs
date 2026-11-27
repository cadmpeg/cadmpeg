// SPDX-License-Identifier: Apache-2.0
use super::*;

/// Cached selection of type `LINE_DIRECTION_TYPE` with one `(origin, span)` entry per element.
fn chain(version: u8, entries: &[([f64; 3], [f64; 3])]) -> Vec<u8> {
    let form = u32::try_from(entries.len()).expect("small chain");
    let mut bytes = cached_selection_prefix(version, -1, form, 0, 23);
    for (index, (origin, span)) in entries.iter().enumerate() {
        if index != 0 {
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(&23u32.to_le_bytes());
        }
        for value in origin.iter().chain(span) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    for value in [0.0f64; 3] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend_from_slice(&93u32.to_le_bytes());
    bytes
}

fn parse_chain(version: u8, entries: &[([f64; 3], [f64; 3])]) -> Operand {
    let bytes = chain(version, entries);
    with_bytes(&bytes, |ctx, source| {
        parse(ctx, LINE_DIRECTION_TYPE, source, version)
    })
    .expect("well-formed chain")
}

#[test]
fn collinear_continuing_entries_form_one_straight_direction() {
    for version in [19, 23, 25, 28, 31] {
        for count in [2usize, 3, 5] {
            let entries: Vec<([f64; 3], [f64; 3])> = (0..count)
                .map(|index| {
                    (
                        [
                            0.5 * f64::from(u32::try_from(index).expect("small")),
                            0.0,
                            0.0,
                        ],
                        [0.5, 0.0, 0.0],
                    )
                })
                .collect();
            let operand = parse_chain(version, &entries);
            let Operand::Line { span, segments, .. } = &operand else {
                panic!("straight chain is a line");
            };
            assert_eq!(*span, Vector3::new(0.5, 0.0, 0.0));
            assert_eq!(usize::try_from(*segments).expect("small"), count);
            let bytes = chain(version, &entries);
            assert_eq!(
                operand_length(version, LINE_DIRECTION_TYPE, &operand),
                Some(cadmpeg_core::decode::u64_from_index(bytes.len()))
            );
        }
    }
}

#[test]
fn chains_that_bend_gap_or_reverse_stay_unresolved() {
    let first = ([0.0, 0.0, 0.0], [2.0, 0.0, 0.0]);
    for second in [
        ([2.0, 0.0, 0.0], [0.0, 2.0, 0.0]),
        ([2.5, 0.0, 0.0], [2.0, 0.0, 0.0]),
        ([2.0, 0.0, 0.0], [-2.0, 0.0, 0.0]),
        ([2.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        ([2.0, 0.0, 0.0], [2.0, 1.0e-3, 0.0]),
    ] {
        assert!(matches!(
            parse_chain(19, &[first, second]),
            Operand::Unresolved
        ));
    }
}

#[test]
fn chain_entry_markers_and_shared_state_are_checked() {
    let entries = [
        ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        ([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
    ];
    let header = content_at(2, 19, &[]).len();
    let mut bad_curve = chain(19, &entries);
    bad_curve[header + 16 + 54 + 2] = 24;
    let mut bad_state = chain(19, &entries);
    let state = bad_state.len() - 5;
    bad_state[state] = 0;
    for bytes in [bad_curve, bad_state] {
        assert!(matches!(
            with_bytes(&bytes, |ctx, source| parse(
                ctx,
                LINE_DIRECTION_TYPE,
                source,
                19
            )),
            Ok(Operand::Unresolved)
        ));
    }
    let mut short = chain(19, &entries);
    short.pop();
    assert!(matches!(
        with_bytes(&short, |ctx, source| parse(
            ctx,
            LINE_DIRECTION_TYPE,
            source,
            19
        )),
        Ok(Operand::Unresolved)
    ));
    let mut edge = chain(19, &entries);
    edge.extend_from_slice(&94u32.to_le_bytes());
    assert!(matches!(
        with_bytes(&edge, |ctx, source| parse(
            ctx,
            EDGE_DIRECTION_TYPE,
            source,
            19
        )),
        Ok(Operand::Unresolved)
    ));
}

#[test]
fn straight_chain_direction_projects_like_a_single_line() {
    let feature = project_fixture(&pattern(19, false), |operands, _, _| {
        let entries = [
            ([0.0, 0.0, 0.0], [0.0, -2.0, 0.0]),
            ([0.0, -2.0, 0.0], [0.0, -3.0, 0.0]),
        ];
        let operand = parse_chain(19, &entries);
        operands
            .records
            .insert(("generated".into(), 87), Some(operand));
    })
    .expect("straight chain direction");
    let FeatureDefinition::Operation(FeatureOperation::Pattern {
        pattern: construction,
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("pattern");
    };
    let PatternTransform::Linear { direction, .. } = construction.definition() else {
        panic!("linear");
    };
    assert_eq!(
        direction.expect("resolved direction"),
        Vector3::new(0.0, -1.0, 0.0)
    );
}

#[test]
fn chain_segments_survive_the_native_record_round_trip() {
    let entries = [
        ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        ([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
    ];
    let operand = parse_chain(19, &entries);
    let wire = json::to_value(&operand).expect("serializable");
    assert_eq!(wire["segments"], 2);
    let decoded: Operand = json::from_value(wire).expect("restorable");
    assert!(matches!(decoded, Operand::Line { segments: 2, .. }));
    let single = parse_chain(19, &entries[..1]);
    let wire = json::to_value(&single).expect("serializable");
    assert!(wire.get("segments").is_none());
}
