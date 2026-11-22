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
    circular_payload_with_keys(symmetric, references, &[])
}

/// `circular_payload` whose suffix stores the count of `keys` and `keys`.
fn circular_payload_with_keys(symmetric: u8, references: u32, keys: &[u32]) -> Vec<u8> {
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
    bytes.extend_from_slice(&u32::try_from(keys.len()).unwrap().to_le_bytes());
    for key in keys {
        bytes.extend_from_slice(&key.to_le_bytes());
    }
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
        .map(|(symmetric, _)| symmetric)
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
    // The Compute option at offset 56: Optimized, Identical, or Adjust.
    for compute in 0..=2 {
        let mut bytes = bytes.clone();
        bytes[56] = compute;
        assert!(
            parse(&bytes, [50, 52, 20], 3).is_some(),
            "Compute {compute}"
        );
    }
    // The symmetric byte, the reference count, the zero run, a Compute option
    // of 3, the sentinel, the fixed byte, the entry count, and the scope
    // reference.
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

#[test]
fn circular_pattern_payload_reads_its_suppressed_instance_keys() {
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
    let parse = |bytes: &[u8]| {
        super::super::exact_circular_pattern_symmetric(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            &scope,
            [50, 52, 20],
            3,
        )
        .map(|(_, suppressed)| suppressed)
    };
    let none = parse(&circular_payload(0, 0)).expect("no suppressed instance");
    assert!(none.value.is_empty());
    // Instance 2 of three has key 3.
    let bytes = circular_payload_with_keys(0, 0, &[3]);
    let one = parse(&bytes).expect("one suppressed instance");
    assert_eq!(one.value, [3]);
    assert_eq!(one.offset, u64::try_from(bytes.len() - 11 - 8).unwrap());
    // The seed, an instance past the count, a repeated key, and key zero.
    for keys in [&[1][..], &[4], &[2, 2], &[0]] {
        assert!(
            parse(&circular_payload_with_keys(0, 0, keys)).is_none(),
            "{keys:?}"
        );
    }
}
