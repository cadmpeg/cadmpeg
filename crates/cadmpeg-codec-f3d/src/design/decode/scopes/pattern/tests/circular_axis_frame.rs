// SPDX-License-Identifier: Apache-2.0
//! The 221-byte circular-pattern axis frame and the circular-pattern payload.

use super::{rectangular_pattern_direction_frame, spacing_direction_frame};
use crate::design::decode::scopes::pattern::select_circular_pattern_axis;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::indexed_header;
use crate::test_support::push_marked_reference;

#[test]
fn a_solved_axis_record_takes_precedence_over_an_extended_frame() {
    use crate::design::decode::scopes::pattern::CircularPatternAxisCandidate;
    use crate::records::feature::patterns::DesignCircularPatternAxis;

    let candidate = |axis_record_index| CircularPatternAxisCandidate {
        separator_flag_offset: None,
        axis: DesignCircularPatternAxis::Inline {
            origin: crate::test_support::reals([1.0, 2.0, 3.0]),
            origin_offset: 29,
            direction: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            direction_offset: 53,
        },
        axis_record_index,
        selection_record_index: axis_record_index + 3,
    };
    let selected = |solved: Vec<u32>, extended: Vec<u32>| {
        let candidates = super::super::inline_axis_candidates(
            solved.into_iter().map(candidate).collect(),
            extended.into_iter().map(candidate).collect(),
        );
        select_circular_pattern_axis(&candidates).map(|index| candidates[index].axis_record_index)
    };
    assert_eq!(selected(vec![10], vec![20]), Some(10));
    assert_eq!(selected(vec![10], vec![20, 30]), Some(10));
    assert_eq!(selected(Vec::new(), vec![20]), Some(20));
    assert_eq!(selected(Vec::new(), vec![20, 30]), None);
    assert_eq!(selected(vec![10, 11], vec![20]), None);
}

#[test]
fn circular_pattern_axis_reads_the_spacing_direction_frame() {
    let bytes = spacing_direction_frame();
    let parse = |bytes: &[u8], paired_at: usize| {
        super::super::exact_extended_circular_pattern_axis(bytes, 0, paired_at, 20, 23, 10)
    };
    assert_eq!(
        parse(&bytes, 221),
        Some(
            crate::records::feature::patterns::DesignCircularPatternAxis::Inline {
                origin: crate::test_support::reals([-2.0, 1.0, 0.5]),
                origin_offset: 25,
                direction: cadmpeg_ir::units::UnitVector3::normalized(
                    cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
                )
                .unwrap(),
                direction_offset: 49,
            }
        )
    );
    let mut face_kind = bytes.clone();
    face_kind[21..25].copy_from_slice(&16u32.to_le_bytes());
    face_kind[49..73].fill(0);
    face_kind[57..65].copy_from_slice(&1.0f64.to_le_bytes());
    assert!(
        super::super::exact_rectangular_pattern_direction(&face_kind, 0, 221, 20, 23, 10).is_some()
    );
    assert!(parse(&face_kind, 221).is_none());
    let extent = rectangular_pattern_direction_frame(8, [0.0, 2.0, 0.0]);
    assert!(parse(&extent, 220).is_none());
}

/// A circular-pattern payload of a pattern with three instances: the
/// symmetric byte, `references` marked references, two entries, and three
/// instance references.
fn circular_payload(symmetric: u8, references: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"355", 77);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(symmetric);
    push_marked_reference(&mut bytes, 52);
    push_marked_reference(&mut bytes, 20);
    assert_eq!(bytes.len(), 44);
    bytes.extend_from_slice(&references.to_le_bytes());
    for index in 0..references {
        push_marked_reference(&mut bytes, 300 + index);
    }
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for (key, identity) in [(2u64, 0x130u64), (3, 0x131)] {
        bytes.extend_from_slice(&key.to_le_bytes());
        bytes.extend_from_slice(&identity.to_le_bytes());
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    for index in 200..203 {
        push_marked_reference(&mut bytes, index);
    }
    for index in [50, 10, 100] {
        push_marked_reference(&mut bytes, index);
    }
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"307", 99);
    bytes
}

#[test]
fn circular_pattern_payload_supplies_the_symmetric_byte() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::CPattern,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 20, 23]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let parse = |bytes: &[u8], owners: [u32; 3], count: u32| {
        super::super::exact_circular_pattern_symmetric(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            &scope,
            owners,
            count,
        )
    };
    let one_sided = parse(&circular_payload(0, 0), [50, 52, 20], 3).expect("one-sided payload");
    assert_eq!((one_sided.value, one_sided.offset), (false, 21));
    let symmetric = parse(&circular_payload(1, 1), [50, 52, 20], 3).expect("symmetric payload");
    assert_eq!((symmetric.value, symmetric.offset), (true, 21));

    let bytes = circular_payload(0, 0);
    assert!(parse(&bytes, [51, 52, 20], 3).is_none());
    assert!(parse(&bytes, [50, 53, 20], 3).is_none());
    assert!(parse(&bytes, [50, 52, 21], 3).is_none());
    assert!(parse(&bytes, [50, 52, 20], 4).is_none());
    // The symmetric byte, the reference count, the zero run, the fixed word,
    // the sentinel, the fixed byte, the entry count, and the scope reference.
    for (offset, value) in [
        (21, 2),
        (44, 1),
        (48, 1),
        (56, 3),
        (64, 0),
        (68, 0),
        (69, 3),
        (154, 11),
    ] {
        let mut corrupt = bytes.clone();
        corrupt[offset] = value;
        assert!(
            parse(&corrupt, [50, 52, 20], 3).is_none(),
            "offset {offset}"
        );
    }
    let mut ambiguous = bytes.clone();
    ambiguous.extend_from_slice(&bytes);
    assert!(parse(&ambiguous, [50, 52, 20], 3).is_none());
}
