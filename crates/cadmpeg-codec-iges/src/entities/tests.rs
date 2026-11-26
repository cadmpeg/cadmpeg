// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

#[test]
fn affine_parameter_map_retains_finite_ratio_of_overflowing_span() {
    let (scale, offset) = crate::entities::affine_parameter_map([-f64::MAX, f64::MAX], [0.0, 1.0])
        .expect("finite affine map");
    assert!((scale * f64::MAX - 0.5).abs() < 8.0 * f64::EPSILON);
    assert!((offset - 0.5).abs() < 8.0 * f64::EPSILON);
}

#[test]
fn affine_parameter_map_retains_identity_between_overflowing_spans() {
    assert_eq!(
        crate::entities::affine_parameter_map([-f64::MAX, f64::MAX], [-f64::MAX, f64::MAX]),
        Some((1.0, 0.0))
    );
}

#[test]
fn directed_cycle_detection_handles_long_branching_graphs_iteratively() {
    let mut graph = (1..=100_000_u32)
        .map(|sequence| (sequence, vec![sequence + 1]))
        .collect::<BTreeMap<_, _>>();
    graph.entry(50_000).or_default().push(100_001);
    let mut visited = std::collections::BTreeSet::new();

    assert!(!crate::entities::directed_cycle(
        1,
        &mut visited,
        |sequence| graph.get(&sequence).into_iter().flatten().copied()
    ));
    assert_eq!(visited.len(), 100_001);

    graph.insert(100_001, vec![50_000]);
    assert!(crate::entities::directed_cycle(
        1,
        &mut std::collections::BTreeSet::new(),
        |sequence| graph.get(&sequence).into_iter().flatten().copied()
    ));
}

#[test]
fn diagnostic_error_text_keeps_a_non_resource_error() {
    assert_eq!(
        super::non_resource_error(cadmpeg_core::CodecError::Malformed("invalid source".into()))
            .unwrap(),
        "malformed container: invalid source"
    );
}

#[test]
fn invalid_entity_parameters_report_projection_losses() {
    use crate::loss::IgesLossCode;
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    let cases = [
        (110, 0, "110,0;", "endpoint coordinate"),
        (150, 0, "150,0;", "primitive dimensions"),
        (502, 1, "502,0;", "vertex-list count"),
        (308, 0, "308,0;", "subfigure"),
        (130, 0, "130,0;", "offset distance flag"),
        (108, 0, "108,0;", "plane coefficients"),
        (142, 0, "142,0;", "curve-on-surface"),
        (112, 0, "112,0;", "spline header"),
        (102, 0, "102,0;", "child count"),
        (212, 0, "212,0;", "text count"),
    ];
    for (entity_type, form, parameters, reason) in cases {
        let bytes = owned_test_file(&[OwnedTestEntity {
            entity_type,
            form,
            label: "INVALID".into(),
            status: "00000000",
            parameters: parameters.into(),
        }]);
        let result = crate::IgesCodec
            .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        let expected = if matches!(entity_type, 308 | 212) {
            IgesLossCode::EntityNotProjected.kind()
        } else {
            IgesLossCode::GeometryNotProjected.kind()
        };
        assert!(
            result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == expected && loss.message.contains(reason)),
            "{entity_type}: {:#?}",
            result.report().losses
        );
    }
}

fn curve_entry(sequence: u32) -> crate::directory::DirectoryEntry {
    crate::directory::DirectoryEntry {
        source_offset: u64::from(sequence) * 80,
        source_length: Some(160),
        sequence,
        entity_type: 100,
        parameter_start: 1,
        structure: 0,
        line_font: Some(0),
        level: Some(0),
        view: Some(0),
        transform: 0,
        label_display: Some(0),
        status_padding_recovered: false,
        status: crate::directory::SourceStatus::from_codes([0, 0, 0, 0]),
        line_weight: Some(0),
        color: Some(0),
        parameter_line_count: 1,
        form: 0,
        reserved: [[b' '; 8]; 2],
        label: *b"CURVE   ",
        subscript: Some(0),
    }
}

#[test]
fn an_entity_that_needs_a_given_up_entity_is_dropped_with_it_and_others_keep_their_charge() {
    use cadmpeg_ir::unknown::part::{dropped_with, record_unreadable_kind};

    let image = super::CardImage::new(&[], crate::representation::Representation::FixedAscii);
    let ids = super::GivenUpIds::new(&image, &[], &[]).unwrap();
    let entries = (1..=15).step_by(2).map(curve_entry).collect::<Vec<_>>();
    let entry = |sequence: u32| &entries[usize::try_from(sequence / 2).unwrap()];
    let note = |sequence| super::geometry_note(entry(sequence), format_args!("D{sequence}"));
    let mut losses = Vec::new();
    // D1 is unreadable. D3 needs D1, and D5 needs D3.
    super::give_up(&mut losses, entry(1), note(1));
    super::give_up_needing(&mut losses, entry(3), Some(1), note(3));
    super::give_up_needing(&mut losses, entry(5), Some(3), note(5));
    // D7 needs D17, which has no give-up.
    super::give_up_needing(&mut losses, entry(7), Some(17), note(7));
    // D9 and D11 need each other.
    super::give_up_needing(&mut losses, entry(9), Some(11), note(9));
    super::give_up_needing(&mut losses, entry(11), Some(9), note(11));
    // D13 is unreadable and also needs D1.
    super::give_up_needing(&mut losses, entry(13), Some(1), note(13));
    super::give_up(&mut losses, entry(13), note(13));
    let by_sequence = entries
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();

    super::settle_needs(&mut losses, &by_sequence, &ids).unwrap();

    let charged = losses
        .iter()
        .filter(|loss| loss.code == record_unreadable_kind())
        .map(|loss| super::entity_tag(loss).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(charged, [1, 7, 9, 13]);
    let part = |sequence| ids.entity(entry(sequence)).unwrap();
    let notes = losses
        .iter()
        .filter(|loss| cadmpeg_ir::unknown::part::needs(loss).is_some())
        .map(|loss| loss.message.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        notes,
        [
            dropped_with(super::entity_record(3), part(1)).message,
            dropped_with(super::entity_record(5), super::entity_record(3)).message,
            dropped_with(super::entity_record(11), part(9)).message,
        ]
    );
}
