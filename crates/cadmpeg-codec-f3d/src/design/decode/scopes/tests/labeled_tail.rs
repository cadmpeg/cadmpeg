// SPDX-License-Identifier: Apache-2.0
//! Labeled (renamed) parameter-scope tails in the plain 78- and 77-byte forms
//! and the typed form, and the state bytes and lane flags of each form.

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::serde_erase::json;

use crate::test_support::lp_utf16;
use crate::test_support::push_reference_u64;
use crate::test_support::typed_reference_scopes::{parse, typed_scope_frame};

const LABEL: &str = "Renamed";

struct PlainTail {
    label: &'static str,
    long_form: bool,
    references: [u32; 3],
    epochs: (u32, u32),
    state: [u8; 3],
    flags: (u8, u8),
    previous: u32,
}

impl Default for PlainTail {
    fn default() -> Self {
        Self {
            label: LABEL,
            long_form: true,
            references: [334, 333, 332],
            epochs: (252, 252),
            state: [0; 3],
            flags: (0, 0),
            previous: 9,
        }
    }
}

/// A Draft scope with current state 7 and a plain tail, labeled unless the
/// label is empty; returns the frame and the end of the feature-family name.
fn plain_labeled_frame(tail: &PlainTail) -> (Vec<u8>, usize) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"378");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    // Read as LP-UTF16 from the count, the reference's first code unit is a
    // control character, so the reference table reads as no family name.
    push_reference_u64(&mut bytes, 0x100);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Draft");
    let kind_end = bytes.len();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    lp_utf16(&mut bytes, tail.label);
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&tail.state);
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
    let wire = json::to_value(&scope).unwrap();
    assert_eq!(
        json::from_value::<crate::records::feature::scope::DesignParameterScope>(wire).unwrap(),
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
            ..PlainTail::default()
        });
        let (out_of_range, _) = plain_labeled_frame(&PlainTail {
            long_form,
            flags: (2, 0),
            ..PlainTail::default()
        });
        assert!(parse_plain(&out_of_range).is_none());
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

/// The tail flags of state bytes `state` at `state_at` and lane flags
/// `flags` at `flags_at`.
fn tail_flags(
    state: [u8; 3],
    flags: [u8; 2],
    state_at: usize,
    flags_at: usize,
) -> crate::records::feature::scope::DesignScopeTailFlags {
    crate::records::feature::scope::DesignScopeTailFlags {
        suppression: [state[0] != 0, state[1] != 0],
        unevaluated: state[2] != 0,
        wide_state: state
            .iter()
            .any(|byte| *byte > 1)
            .then_some(cadmpeg_ir::native::bytes::NativeBytes::new(state)),
        state_offset: u64_from_index(state_at),
        no_result: flags[0] == 1,
        rolled_back: flags[1] == 1,
        no_result_offset: u64_from_index(flags_at),
        envelope_offset: None,
    }
}

#[test]
fn plain_tails_read_their_state_bytes_and_lane_flags() {
    for label in [LABEL, ""] {
        for long_form in [true, false] {
            for (state, no_result, rolled_back) in [
                ([0, 0, 0], 0, 0),
                ([0, 0, 0], 1, 0),
                ([0, 0, 0], 0, 1),
                ([0, 0, 0], 1, 1),
                ([1, 0, 0], 1, 0),
                ([0, 1, 0], 1, 1),
                ([0, 0, 1], 1, 0),
            ] {
                let tail = PlainTail {
                    label,
                    long_form,
                    state,
                    flags: (no_result, rolled_back),
                    ..PlainTail::default()
                };
                let (bytes, kind_end) = plain_labeled_frame(&tail);
                let scope = parse_plain(&bytes).unwrap_or_else(|| {
                    panic!("plain scope: label {label:?}, long form {long_form}, state {state:?}, flags {no_result} {rolled_back}")
                });
                let state_at = kind_end + 16 + 2 * label.len();
                let at = kind_end + 62 + usize::from(long_form) + 2 * label.len();
                assert_eq!(bytes[state_at..state_at + 3], state);
                assert_eq!(bytes[at..at + 2], [no_result, rolled_back]);
                assert_eq!(
                    scope.tail_flags,
                    Some(tail_flags(state, [no_result, rolled_back], state_at, at)),
                    "label {label:?}, long form {long_form}"
                );
                let wire = json::to_value(&scope).unwrap();
                assert_eq!(
                    json::from_value::<crate::records::feature::scope::DesignParameterScope>(wire)
                        .unwrap(),
                    scope
                );
            }
        }
    }
}

#[test]
fn plain_tail_state_bytes_are_zero_unless_the_no_result_flag_is_set() {
    for (no_result, admitted) in [(0, false), (1, true)] {
        let (mut bytes, kind_end) = plain_labeled_frame(&PlainTail {
            flags: (no_result, 0),
            ..PlainTail::default()
        });
        bytes[kind_end + 17 + 2 * LABEL.len()] = 1;
        assert_eq!(parse_plain(&bytes).is_some(), admitted, "s0 {no_result}");
    }
}

/// A state byte above `1` beside a set no-result flag is read as set; the
/// tail keeps its stored state bytes and its lane flags, and the scope
/// reloads unchanged.
#[test]
fn a_plain_tail_state_byte_above_one_is_read_as_set() {
    for long_form in [true, false] {
        for byte in 0..3 {
            let mut state = [0; 3];
            state[byte] = 2;
            let (bytes, kind_end) = plain_labeled_frame(&PlainTail {
                long_form,
                state,
                flags: (1, 1),
                ..PlainTail::default()
            });
            let scope = parse_plain(&bytes).expect("plain scope");
            let state_at = kind_end + 16 + 2 * LABEL.len();
            let flags_at = kind_end + 62 + usize::from(long_form) + 2 * LABEL.len();
            let flags = tail_flags(state, [1, 1], state_at, flags_at);
            assert_eq!(scope.tail_flags, Some(flags), "state {state:?}");
            assert_eq!(flags.stored_state(), state);
            assert_eq!(flags.suppressed(), byte < 2, "state {state:?}");
            let wire = json::to_value(&scope).unwrap();
            assert_eq!(
                json::from_value::<crate::records::feature::scope::DesignParameterScope>(wire)
                    .unwrap(),
                scope
            );
        }
    }
}

/// A typed Fillet frame with the flags at tail offset 141 and the state
/// bytes, labeled when `labeled`; returns the frame and the end of the
/// feature-family name.
fn typed_flagged_frame(labeled: bool, flags: [u8; 2], state: [u8; 3]) -> (Vec<u8>, usize) {
    let kind = "Fillet";
    let mut bytes = typed_scope_frame(41, kind);
    let kind_end = 11 + 10 + 4 + 2 * 51 + 4 + 4 + 2 * kind.len();
    bytes[kind_end + 141..kind_end + 143].copy_from_slice(&flags);
    bytes[kind_end + 16..kind_end + 19].copy_from_slice(&state);
    if labeled {
        let mut label = Vec::new();
        lp_utf16(&mut label, LABEL);
        bytes.splice(kind_end + 8..kind_end + 12, label);
    }
    (bytes, kind_end)
}

#[test]
fn typed_tails_read_their_state_bytes_and_lane_flags() {
    for labeled in [false, true] {
        for (flags, state) in [
            ([1, 0], [0; 3]),
            ([0, 1], [0; 3]),
            ([1, 1], [0; 3]),
            ([1, 0], [1, 0, 0]),
            ([1, 1], [0, 1, 0]),
            ([1, 0], [0, 0, 1]),
        ] {
            let (bytes, kind_end) = typed_flagged_frame(labeled, flags, state);
            let scope = parse(&bytes).expect("flagged typed scope");
            let shift = if labeled { 2 * LABEL.len() } else { 0 };
            assert_eq!(
                scope.tail_flags,
                Some(tail_flags(
                    state,
                    flags,
                    kind_end + 16 + shift,
                    kind_end + 141 + shift
                )),
                "labeled {labeled}, flags {flags:?}, state {state:?}"
            );
            assert_eq!(scope.previous_history_state_id(), Some(41));
        }
    }
}

/// A typed tail state byte above `1` is read as set, with its lane flags.
#[test]
fn a_typed_tail_state_byte_above_one_is_read_as_set() {
    for labeled in [false, true] {
        let shift = if labeled { 2 * LABEL.len() } else { 0 };
        for byte in 0..3 {
            let mut state = [0; 3];
            state[byte] = 2;
            let (bytes, kind_end) = typed_flagged_frame(labeled, [1, 1], state);
            assert_eq!(
                parse(&bytes).and_then(|scope| scope.tail_flags),
                Some(tail_flags(
                    state,
                    [1, 1],
                    kind_end + 16 + shift,
                    kind_end + 141 + shift
                )),
                "labeled {labeled}, state {state:?}"
            );
        }
    }
}

/// Stored state bytes reload only where one is above `1` and they agree with
/// the set state bytes.
#[test]
fn scope_wire_refuses_stored_state_bytes_that_disagree_with_the_set_bytes() {
    let (bytes, _) = plain_labeled_frame(&PlainTail {
        state: [0, 0, 2],
        flags: (1, 0),
        ..PlainTail::default()
    });
    let scope = parse_plain(&bytes).expect("plain scope");
    let wire = json::to_value(&scope).unwrap();
    for stored in ["000001", "000200", "020002"] {
        let mut changed = wire.clone();
        changed["tail_flags"]["wide_state"] = cadmpeg_core::json!(stored);
        assert!(
            json::from_value::<crate::records::feature::scope::DesignParameterScope>(changed)
                .is_err(),
            "stored {stored:?}"
        );
    }
}

#[test]
fn typed_tail_state_bytes_are_zero_unless_the_no_result_flag_is_set() {
    for labeled in [false, true] {
        let shift = if labeled { 2 * LABEL.len() } else { 0 };
        assert!(parse(&typed_flagged_frame(labeled, [0, 0], [0, 1, 0]).0).is_none());
        assert!(parse(&typed_flagged_frame(labeled, [0, 1], [1, 0, 0]).0).is_none());
        assert!(parse(&typed_flagged_frame(labeled, [2, 0], [0; 3]).0).is_none());
        let (bytes, kind_end) = typed_flagged_frame(labeled, [1, 0], [0, 1, 0]);
        assert_eq!(
            parse(&bytes).and_then(|scope| scope.tail_flags),
            Some(tail_flags(
                [0, 1, 0],
                [1, 0],
                kind_end + 16 + shift,
                kind_end + 141 + shift
            ))
        );
    }
}

#[test]
fn scope_wire_refuses_tail_flags_outside_the_tail_or_set_without_the_no_result_flag() {
    let (bytes, _) = plain_labeled_frame(&PlainTail {
        flags: (1, 1),
        ..PlainTail::default()
    });
    let scope = parse_plain(&bytes).expect("flagged scope");
    let wire = json::to_value(&scope).unwrap();
    for offset in [
        scope.feature_ordinal_offset() - 1,
        scope.paired_byte_offset() - 1,
        u64::MAX - 1,
        u64::MAX,
    ] {
        let mut moved = wire.clone();
        moved["tail_flags"]["no_result_offset"] = offset.into();
        assert!(
            json::from_value::<crate::records::feature::scope::DesignParameterScope>(moved)
                .is_err(),
            "offset {offset}"
        );
    }
    let flags = scope.tail_flags.expect("flags");
    for offset in [
        scope.feature_ordinal_offset() - 1,
        flags.no_result_offset - 2,
    ] {
        let mut moved = wire.clone();
        moved["tail_flags"]["state_offset"] = offset.into();
        assert!(
            json::from_value::<crate::records::feature::scope::DesignParameterScope>(moved)
                .is_err(),
            "state offset {offset}"
        );
    }
    let mut current = wire;
    current["tail_flags"]["no_result"] = false.into();
    current["tail_flags"]["suppression"] = cadmpeg_core::json!([false, true]);
    assert!(
        json::from_value::<crate::records::feature::scope::DesignParameterScope>(current).is_err()
    );
}

/// A scope reads flags that an earlier envelope of its record stores when they
/// lie after the start of that envelope and before its own start.
#[test]
fn scope_wire_admits_the_flags_of_an_earlier_envelope() {
    use crate::records::feature::scope::{DesignParameterScope, DesignScopeTailFlags};

    let (frame, _) = plain_labeled_frame(&PlainTail {
        flags: (1, 1),
        ..PlainTail::default()
    });
    let bytes = [frame.as_slice(), frame.as_slice()].concat();
    let earlier = parse_plain(&bytes).expect("earlier envelope");
    let later = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        12,
        &crate::records::references::DesignClassTag::try_from("378".to_owned()).unwrap(),
        u64_from_index(frame.len()),
    )
    .unwrap()
    .expect("later envelope");
    let flags = earlier.tail_flags.expect("earlier flags");
    let carried = |envelope_offset: u64, no_result_offset: u64| {
        let mut wire = json::to_value(&later).unwrap();
        wire["tail_flags"] = json::to_value(DesignScopeTailFlags {
            no_result_offset,
            envelope_offset: Some(envelope_offset),
            ..flags
        })
        .unwrap();
        json::from_value::<DesignParameterScope>(wire)
    };
    assert_eq!(
        carried(earlier.byte_offset(), flags.no_result_offset)
            .expect("flags of the earlier envelope")
            .tail_flags,
        Some(DesignScopeTailFlags {
            envelope_offset: Some(earlier.byte_offset()),
            ..flags
        })
    );
    assert!(carried(earlier.byte_offset(), later.byte_offset() - 2).is_ok());
    let own = later.tail_flags.expect("later flags").no_result_offset;
    for (envelope_offset, no_result_offset) in [
        (later.byte_offset(), flags.no_result_offset),
        (flags.no_result_offset, flags.no_result_offset),
        (earlier.byte_offset(), later.byte_offset() - 1),
        (earlier.byte_offset(), own),
        (earlier.byte_offset(), later.paired_byte_offset() - 1),
        (earlier.byte_offset(), u64::MAX - 1),
        (earlier.byte_offset(), u64::MAX),
    ] {
        assert!(
            carried(envelope_offset, no_result_offset).is_err(),
            "envelope {envelope_offset}, flags {no_result_offset}"
        );
    }
}

/// The decoder gives a later envelope that stores no flags the flags of the
/// latest earlier envelope that stores them, located at those flag bytes, and
/// the kept scope reloads unchanged.
#[test]
fn carried_flags_of_an_earlier_envelope_reload() {
    use crate::design::decode::scopes::parameter_scope::{
        admit_history_bound_scope_variants, parse_parameter_scope,
    };
    use crate::records::feature::scope::{DesignParameterScope, DesignScopeTailFlags};

    let frames = [(1, 0), (0, 1), (1, 1)].map(|flags| {
        plain_labeled_frame(&PlainTail {
            flags,
            ..PlainTail::default()
        })
        .0
    });
    let bytes = frames.concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let tag = crate::records::references::DesignClassTag::try_from("378".to_owned()).unwrap();
    let scopes = [0, frames[0].len(), frames[0].len() + frames[1].len()].map(|start| {
        parse_parameter_scope(&bytes, &records, 12, &tag, u64_from_index(start))
            .unwrap()
            .expect("envelope")
    });
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    // The envelopes whose tails are left unread, and the one whose flags the
    // last envelope takes.
    for (unread, source) in [(&[2][..], 1), (&[1, 2][..], 0)] {
        let mut variants = scopes.to_vec();
        for index in unread {
            variants[*index].tail_flags = None;
        }
        admit_history_bound_scope_variants(&ctx, &mut variants, &[], &Default::default())
            .expect("the document decodes");
        let [kept] = variants.as_slice() else {
            panic!("one retained envelope")
        };
        let flags = scopes[source].tail_flags.expect("source flags");
        assert_eq!(kept.byte_offset(), scopes[2].byte_offset());
        assert_eq!(
            kept.tail_flags,
            Some(DesignScopeTailFlags {
                envelope_offset: Some(scopes[source].byte_offset()),
                ..flags
            }),
            "unread {unread:?}"
        );
        let at = usize::try_from(flags.no_result_offset).unwrap();
        assert_eq!(
            &bytes[at..at + 2],
            &[u8::from(flags.no_result), u8::from(flags.rolled_back)]
        );
        let reloaded = json::from_value::<DesignParameterScope>(json::to_value(kept).unwrap())
            .expect("the kept scope reloads");
        assert_eq!(&reloaded, kept, "unread {unread:?}");
    }
}

/// Two envelopes of one record that differ only in a state byte of `2` keep
/// the later envelope with its own tail state, whichever envelope is
/// primary-indexed, and one note names both states.
#[test]
fn envelopes_that_differ_only_in_a_state_byte_of_two_keep_the_later() {
    use crate::design::decode::scopes::parameter_scope::{
        admit_history_bound_scope_variants, parse_parameter_scope, SupersededScopeEnvelopes,
    };

    let tag = crate::records::references::DesignClassTag::try_from("378".to_owned()).unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    for states in [[[0, 0, 0], [0, 0, 2]], [[0, 0, 2], [0, 0, 0]]] {
        let frames = states.map(|state| {
            plain_labeled_frame(&PlainTail {
                state,
                flags: (1, 0),
                ..PlainTail::default()
            })
            .0
        });
        let bytes = frames.concat();
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let scopes = [0, frames[0].len()].map(|start| {
            let mut scope =
                parse_parameter_scope(&bytes, &records, 12, &tag, u64_from_index(start))
                    .unwrap()
                    .expect("envelope");
            scope.id = format!("f3d:stream:design-parameter-scope#{start}");
            scope
        });
        let stream = "f3d:stream".to_owned();
        let note = format!(
            "Feature scope {} (record 12): the decoder kept state={}, no-result=true, rolled-back=false from its later envelope at byte {} (open item DR-1005). 1 other envelope(s) store different flags, which were not decoded: at byte 0, state={}, no-result=true, rolled-back=false.",
            scopes[1].id,
            states[1].map(|byte| byte.to_string()).concat(),
            scopes[1].byte_offset(),
            states[0].map(|byte| byte.to_string()).concat(),
        );
        for primary_at in [None, Some(0), Some(scopes[1].byte_offset())] {
            let mut variants = scopes.to_vec();
            let primary = primary_at
                .map(|at| (stream.clone(), at))
                .into_iter()
                .collect::<std::collections::HashSet<_>>();
            let superseded = admit_history_bound_scope_variants(&ctx, &mut variants, &[], &primary)
                .expect("the document decodes");
            let [kept] = variants.as_slice() else {
                panic!("one retained envelope")
            };
            let case = format!("states {states:?}, primary {primary_at:?}");
            assert_eq!(kept.byte_offset(), scopes[1].byte_offset(), "{case}");
            assert_eq!(kept.tail_flags, scopes[1].tail_flags, "{case}");
            assert_eq!(
                kept.tail_flags.map(|flags| flags.stored_state()),
                Some(states[1]),
                "{case}"
            );
            assert_eq!(
                superseded,
                SupersededScopeEnvelopes {
                    primary_indexed: 0,
                    tail_flags: vec![note.clone()],
                },
                "{case}"
            );
        }
    }
}
