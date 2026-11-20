// SPDX-License-Identifier: Apache-2.0
//! Labeled (renamed) parameter-scope tails in the plain 78- and 77-byte forms
//! and the typed form.

use cadmpeg_core::decode::u64_from_index;

use crate::test_support::lp_utf16;
use crate::test_support::push_reference_u64;
use crate::test_support::typed_reference_scopes::{parse, typed_scope_frame};

const LABEL: &str = "Renamed";

struct PlainTail {
    long_form: bool,
    references: [u32; 3],
    epochs: (u32, u32),
    flags: (u8, u8),
    previous: u32,
}

impl Default for PlainTail {
    fn default() -> Self {
        Self {
            long_form: true,
            references: [334, 333, 332],
            epochs: (252, 252),
            flags: (0, 0),
            previous: 9,
        }
    }
}

/// A Draft scope with current state 7 and a labeled plain tail; returns the
/// frame and the end of the feature-family name.
fn plain_labeled_frame(tail: &PlainTail) -> (Vec<u8>, usize) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"378");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    push_reference_u64(&mut bytes, 55);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Draft");
    let kind_end = bytes.len();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    lp_utf16(&mut bytes, LABEL);
    bytes.extend_from_slice(&[0; 7]);
    push_reference_u64(&mut bytes, u64::from(tail.references[0]));
    bytes.push(0);
    bytes.extend_from_slice(&tail.previous.to_le_bytes());
    bytes.extend_from_slice(&tail.epochs.0.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&tail.epochs.1.to_le_bytes());
    push_reference_u64(&mut bytes, u64::from(tail.references[1]));
    if tail.long_form {
        bytes.push(1);
    }
    bytes.extend_from_slice(&[tail.flags.0, tail.flags.1]);
    push_reference_u64(&mut bytes, u64::from(tail.references[2]));
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    (bytes, kind_end)
}

fn parse_plain(bytes: &[u8]) -> Option<crate::records::feature::scope::DesignParameterScope> {
    crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        12,
        &crate::records::references::DesignClassTag::try_from("378".to_owned()).unwrap(),
        0,
    )
    .unwrap()
}

fn assert_labeled_with_previous(tail: &PlainTail) {
    let (bytes, kind_end) = plain_labeled_frame(tail);
    let scope = parse_plain(&bytes).expect("labeled scope");
    assert_eq!(
        scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::Draft
    );
    assert_eq!(scope.label.as_ref().unwrap().value, LABEL);
    assert_eq!(
        scope.label.as_ref().unwrap().offset,
        u64_from_index(kind_end + 12)
    );
    assert_eq!(scope.history_state_id(), Some(7));
    assert_eq!(
        scope.previous_history_state_id(),
        Some(i64::from(tail.previous))
    );
    assert_eq!(
        scope.previous_history_state_id_offset(),
        Some(u64_from_index(kind_end + 31 + 2 * LABEL.len()))
    );
    let wire = serde_json::to_value(&scope).unwrap();
    assert_eq!(
        serde_json::from_value::<crate::records::feature::scope::DesignParameterScope>(wire)
            .unwrap(),
        scope
    );
}

#[test]
fn labeled_78_byte_tail_reads_its_preceding_state() {
    assert_labeled_with_previous(&PlainTail::default());
}

#[test]
fn labeled_77_byte_tail_reads_its_preceding_state() {
    assert_labeled_with_previous(&PlainTail {
        long_form: false,
        ..PlainTail::default()
    });
}

#[test]
fn labeled_tail_admits_any_record_index_epoch_and_flag_state() {
    for long_form in [true, false] {
        assert_labeled_with_previous(&PlainTail {
            long_form,
            references: [0x0001_0100, 0x0002_00ff, 0x0001_0000],
            epochs: (64, 89),
            flags: (1, 1),
            previous: 70_000,
        });
    }
}

#[test]
fn labeled_tail_refuses_a_zero_reference_and_an_infinite_duration() {
    let (bytes, kind_end) = plain_labeled_frame(&PlainTail::default());
    let first_reference = kind_end + 19 + 2 * LABEL.len();
    let mut zero_reference = bytes.clone();
    zero_reference[first_reference + 1..first_reference + 5].copy_from_slice(&[0; 4]);
    assert!(parse_plain(&zero_reference).is_none());
    let mut infinite = bytes;
    infinite[first_reference + 20..first_reference + 28]
        .copy_from_slice(&f64::INFINITY.to_le_bytes());
    assert!(parse_plain(&infinite).is_none());
}

#[test]
fn labeled_typed_tail_reads_its_preceding_state_after_the_label() {
    let kind = "Fillet";
    let unlabeled = typed_scope_frame(41, kind);
    let kind_end = 11 + 10 + 4 + 2 * 51 + 4 + 4 + 2 * kind.len();
    assert_eq!(unlabeled[kind_end..kind_end + 4], 1u32.to_le_bytes());
    assert_eq!(unlabeled[kind_end + 4..kind_end + 19], [0; 15]);
    let mut label = Vec::new();
    lp_utf16(&mut label, LABEL);
    let mut bytes = unlabeled.clone();
    bytes.splice(kind_end + 8..kind_end + 12, label);
    let scope = parse(&bytes).expect("labeled typed scope");
    assert_eq!(scope.label.as_ref().unwrap().value, LABEL);
    assert_eq!(scope.history_state_id(), Some(7));
    assert_eq!(scope.previous_history_state_id(), Some(41));
    assert_eq!(
        scope.previous_history_state_id_offset(),
        Some(u64_from_index(kind_end + 70 + 2 * LABEL.len()))
    );
    assert_eq!(
        scope.reference_members().values().collect::<Vec<_>>(),
        [&222, &234]
    );

    let mut broken = bytes;
    broken[kind_end + 12 + 2 * LABEL.len()] = 1;
    assert!(parse(&broken).is_none());
}

#[test]
fn labeled_scope_admits_its_shifted_preceding_state_offset_or_none() {
    let (bytes, _) = plain_labeled_frame(&PlainTail::default());
    let scope = parse_plain(&bytes).expect("labeled scope");
    let offset = scope.previous_history_state_id_offset().unwrap();
    let mut unshifted = scope.clone();
    assert!(unshifted
        .try_edit(|draft| draft.previous_history_state_id_offset = Some(offset - 1))
        .is_err());
    let mut absent = scope;
    absent
        .try_edit(|draft| {
            draft.previous_history_state_id_offset = None;
            draft.previous_history_state_id = None;
        })
        .unwrap();
    assert_eq!(absent.previous_history_state_id_offset(), None);
}
