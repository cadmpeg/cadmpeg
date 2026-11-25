// SPDX-License-Identifier: Apache-2.0

use super::super::parameter_scope::{admit_history_bound_scope_variants, SupersededScopeEnvelopes};
use crate::history_records::AsmHistory;
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
use crate::records::feature::patterns::DesignPathPatternSettings;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::feature::sheet_metal::fold::DesignUnfoldOperation;
use crate::records::identity::Located;
use crate::test_support::real;

fn with_test_ctx<T>(run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    run(&ctx)
}

fn history_state(
    state_id: i64,
    previous_state_id: Option<i64>,
) -> crate::test_support::history::State {
    (
        AsmDeltaState {
            id: format!("history:state#{state_id}"),
            parent: "history".into(),
            byte_offset: 0,
            state_id,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: previous_state_id,
            node_index: state_id,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: None,
        },
        Some(AsmHistoricalTopology::default()),
        Vec::new(),
    )
}

fn history(states: Vec<crate::test_support::history::State>) -> AsmHistory {
    crate::test_support::history::history("history".into(), 0, None, false, states)
}

fn unbound_history_state(
    state_id: i64,
    previous_state_id: Option<i64>,
) -> crate::test_support::history::State {
    let (state, _, versions) = history_state(state_id, previous_state_id);
    (state, None, versions)
}

fn unbound_history(states: Vec<crate::test_support::history::State>) -> AsmHistory {
    crate::test_support::history::history("history".into(), 0, None, true, states)
}

fn scope(
    record_index: u32,
    byte_offset: u64,
    state_id: i64,
    previous_state_id: i64,
) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:stream:design-parameter-scope#{byte_offset}"),
        crate::records::feature::scope::DesignFeatureKind::Chamfer,
        record_index,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = byte_offset;
            draft.history_state_id = Some(state_id);
            draft.previous_history_state_id = Some(previous_state_id);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn equivalent_scope_json_refuses_nested_depth_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let dimension = ResourceDimension::RecursionDepth;
    let operation = "f3d configuration JSON depth";
    let arena = DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
        let mut scopes = vec![scope(42, 100, 7, 6), scope(42, 200, 9, 8)];
        let mut policy = DecodePolicy::default();
        policy.limits.max_recursion_depth = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
        let result =
            admit_history_bound_scope_variants(&ctx, &mut scopes, &[], &Default::default());
        if result.is_err() {
            assert_eq!(scopes.len(), 2);
        }
        result
    });
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.dimension == dimension && failure.operation == operation));
}

#[test]
fn retains_the_unique_history_bound_scope_envelope() {
    let mut scopes = vec![scope(42, 200, 9, 8), scope(42, 100, 7, 6)];
    let histories = [history(vec![
        history_state(7, Some(6)),
        history_state(6, None),
    ])];

    with_test_ctx(|ctx| {
        admit_history_bound_scope_variants(ctx, &mut scopes, &histories, &Default::default())
    })
    .expect("unique envelope");

    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].byte_offset(), 100);
}

#[test]
fn refuses_duplicate_scope_envelopes_without_one_history_binding() {
    let mut scopes = vec![scope(42, 100, 7, 6), scope(42, 200, 9, 8)];
    scopes[1].feature_ordinal = std::num::NonZeroU32::new(2).expect("nonzero ordinal");
    let histories = [history(vec![
        history_state(9, Some(8)),
        history_state(8, Some(7)),
        history_state(7, Some(6)),
        history_state(6, None),
    ])];

    assert!(with_test_ctx(|ctx| admit_history_bound_scope_variants(
        ctx,
        &mut scopes.clone(),
        &histories,
        &Default::default()
    ))
    .is_err());

    for byte_offset in [100, 200] {
        let primary = std::collections::HashSet::from([("f3d:stream".to_owned(), byte_offset)]);
        let mut selected = scopes.clone();
        let superseded = with_test_ctx(|ctx| {
            admit_history_bound_scope_variants(ctx, &mut selected, &histories, &primary)
        })
        .expect("primary envelope");
        assert_eq!(superseded.primary_indexed, 1);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].byte_offset(), byte_offset);
    }

    let primary = std::collections::HashSet::from([
        ("f3d:stream".to_owned(), 100),
        ("f3d:stream".to_owned(), 200),
    ]);
    assert!(with_test_ctx(|ctx| {
        admit_history_bound_scope_variants(ctx, &mut scopes, &histories, &primary)
    })
    .is_err());
}

/// Where several envelopes of a record are history-bound, the primary-indexed
/// envelope is kept and the loss counts the envelopes that differ from it in
/// payload or in their current or effective preceding history state.
#[test]
fn several_history_bound_envelopes_count_only_those_that_differ_from_the_primary() {
    let histories = [history(vec![
        history_state(9, Some(8)),
        history_state(8, Some(7)),
        history_state(7, Some(6)),
        history_state(6, None),
    ])];
    let mut renumbered = scope(43, 400, 7, 6);
    renumbered.feature_ordinal = std::num::NonZeroU32::new(2).expect("nonzero ordinal");
    let mut scopes = vec![
        scope(42, 100, 7, 6),
        scope(42, 200, 7, 6),
        scope(43, 300, 7, 6),
        renumbered,
        scope(43, 500, 7, 6),
        scope(44, 600, 9, 8),
        scope(44, 700, 7, 6),
    ];
    let primary = std::collections::HashSet::from(
        [100, 300, 600].map(|byte_offset| ("f3d:stream".to_owned(), byte_offset)),
    );

    let superseded = with_test_ctx(|ctx| {
        admit_history_bound_scope_variants(ctx, &mut scopes, &histories, &primary)
    })
    .expect("primary-indexed envelopes");

    assert_eq!(
        superseded,
        SupersededScopeEnvelopes {
            primary_indexed: 2,
            tail_flags: Vec::new(),
        }
    );
    assert_eq!(
        scopes
            .iter()
            .map(DesignParameterScope::byte_offset)
            .collect::<Vec<_>>(),
        [100, 300, 600]
    );
}

#[test]
fn retains_later_equivalent_scope_envelope_without_history_binding() {
    let mut older = scope(42, 100, 7, 6);
    older.class_tag =
        crate::records::references::DesignClassTag::try_from("392".to_owned()).unwrap();
    older
        .try_edit(|draft| {
            draft.frame_length = 260;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    older.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
    older
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                vec![101, 102, 103],
                vec![110, 120, 130],
                "reference_members",
            )
            .unwrap();
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    older.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
    let mut newer = older.clone();
    newer.id = "f3d:stream:design-parameter-scope#200".into();
    newer
        .try_edit(|draft| {
            draft.byte_offset = 200;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    newer.class_tag =
        crate::records::references::DesignClassTag::try_from("404".to_owned()).unwrap();
    newer
        .try_edit(|draft| {
            draft.frame_length = 340;
            draft.history_state_id = Some(9);
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                draft.reference_members.values().copied().collect(),
                vec![210, 220, 230],
                "reference_members",
            )
            .unwrap();
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    newer.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();

    let mut scopes = vec![older, newer];
    with_test_ctx(|ctx| {
        admit_history_bound_scope_variants(ctx, &mut scopes, &[], &Default::default())
    })
    .expect("equivalent envelope");

    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].byte_offset(), 200);
}

/// `copies` same-index `kind` envelopes without a history binding, at byte
/// offsets 100, 200 and on. `fill` completes the envelope with its copy number.
fn relocated_variants(
    kind: &DesignFeatureKind,
    copies: usize,
    fill: impl Fn(&mut DesignParameterScope, usize),
) -> Vec<DesignParameterScope> {
    (100_u64..)
        .step_by(100)
        .take(copies)
        .enumerate()
        .map(|(copy, byte_offset)| {
            let mut scope = DesignParameterScope::empty(
                &format!("f3d:stream:design-parameter-scope#{byte_offset}"),
                kind.clone(),
                42,
            );
            scope
                .try_edit(|draft| {
                    draft.byte_offset = byte_offset;
                    draft.reference_count_offset = draft.byte_offset + 9;
                    draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                    draft.layout_fixture_references();
                    draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                    draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                    draft.layout_fixture_tail();
                })
                .unwrap();
            fill(&mut scope, copy);
            scope
        })
        .collect()
}

/// Same-index Thicken envelopes as `relocated_variants` places them, one per
/// signed thickness of `thicknesses`.
fn thicken_variants(thicknesses: &[f64]) -> Vec<DesignParameterScope> {
    relocated_variants(
        &DesignFeatureKind::Thicken,
        thicknesses.len(),
        |scope, copy| {
            let at = scope.byte_offset();
            let DesignScopePayloadMut::Thicken(slot) = scope.payload_mut() else {
                panic!("a Thicken scope holds a Thicken payload");
            };
            *slot = Some(
                crate::records::feature::direct_face::DesignThickenOperation {
                    signed_thickness: Some(real(thicknesses[copy])),
                    thickness_record_index: 74,
                    thickness_offset: Some(at + 40),
                    symmetric: false,
                    tangent_chain: true,
                    ..Default::default()
                },
            );
        },
    )
}

/// Two envelopes as `relocated_variants` builds them, labeled `labels`. Each
/// label and each located payload value lies inside its own envelope, so the
/// copies store every location at a different byte offset.
fn labeled_variants(kind: &DesignFeatureKind, labels: [&str; 2]) -> Vec<DesignParameterScope> {
    relocated_variants(kind, labels.len(), |scope, copy| {
        let at = scope.byte_offset();
        let located = |value: u32, delta| Located {
            value,
            offset: at + delta,
        };
        scope.label = Some(Located {
            value: labels[copy].to_owned(),
            offset: scope.feature_ordinal_offset() + 8,
        });
        match scope.payload_mut() {
            DesignScopePayloadMut::Unfold(slot) => {
                *slot = Some(DesignUnfoldOperation {
                    control: Located {
                        value: 1,
                        offset: at + 20,
                    },
                    prefix_end: at + 40,
                    bend_group: located(7, 24),
                    settings: located(8, 28),
                    wrapper: located(9, 32),
                    stationary_group: located(10, 36),
                    rule_record_index: None,
                    rule_reference_assumed: false,
                    rule_material_scalar: Some(Located {
                        value: real(0.5),
                        offset: at + 44,
                    }),
                });
            }
            DesignScopePayloadMut::PathPattern(slot) => {
                *slot = Some(DesignPathPatternSettings {
                    settings_record_index: Some(5),
                    settings_byte_offset: Some(at + 300),
                    total_extent: Some(Located {
                        value: true,
                        offset: at + 310,
                    }),
                    start_record_index: Some(6),
                    start_frame_end: Some(at + 400),
                    start_parameter: Some(Located {
                        value: real(0.25),
                        offset: at + 390,
                    }),
                    ..Default::default()
                });
            }
            _ => {}
        }
    })
}

/// A renamed feature can be stored twice. Copies that differ only in where
/// they store the label and the located payload values are one logical scope:
/// the later copy is retained. Copies whose label texts differ stay
/// unresolved.
#[test]
fn relocated_identical_scope_copies_are_equivalent() {
    for kind in [
        DesignFeatureKind::Thicken,
        DesignFeatureKind::Unfold,
        DesignFeatureKind::PathPattern,
    ] {
        let mut copies = labeled_variants(&kind, ["Renamed", "Renamed"]);
        with_test_ctx(|ctx| {
            admit_history_bound_scope_variants(ctx, &mut copies, &[], &Default::default())
        })
        .unwrap_or_else(|error| panic!("{kind:?} copies are equivalent: {error}"));
        let [retained] = copies.as_slice() else {
            panic!("one retained {kind:?} envelope")
        };
        assert_eq!(retained.byte_offset(), 200);
        assert_eq!(
            retained.label.as_ref().map(|label| label.value.as_str()),
            Some("Renamed")
        );

        let mut different = labeled_variants(&kind, ["Renamed", "Renamed 2"]);
        assert!(
            with_test_ctx(|ctx| admit_history_bound_scope_variants(
                ctx,
                &mut different,
                &[],
                &Default::default()
            ))
            .is_err(),
            "{kind:?} copies with different labels"
        );
    }
}

/// The payload comparison states every difference of a checked float: the
/// later of two envelopes equal but for provenance is retained, and two
/// envelopes whose thicknesses differ in the last place stay unresolved.
#[test]
fn scope_variants_that_differ_in_a_payload_float_are_not_equivalent() {
    let mut equivalent = thicken_variants(&[-1.0, -1.0]);
    with_test_ctx(|ctx| {
        admit_history_bound_scope_variants(ctx, &mut equivalent, &[], &Default::default())
    })
    .expect("equivalent envelopes");
    assert_eq!(equivalent.len(), 1);
    assert_eq!(equivalent[0].byte_offset(), 200);

    let mut different = thicken_variants(&[-1.0, -1.0 - f64::EPSILON]);
    assert!(with_test_ctx(|ctx| admit_history_bound_scope_variants(
        ctx,
        &mut different,
        &[],
        &Default::default()
    ))
    .is_err());
}

/// The no-result and rolled-back flags of one envelope's tail, whose state
/// bytes are `00 00 00`, `None` where the tail stores none.
type CopyFlags = Option<(bool, bool)>;

/// Equal Thicken envelopes as `thicken_variants` builds them, one per entry of
/// `flags`, whose tails store that entry's flags, each at its own byte
/// location.
fn flagged_thicken_variants(flags: &[CopyFlags]) -> Vec<DesignParameterScope> {
    let mut scopes = thicken_variants(&vec![-1.0; flags.len()]);
    for (scope, flags) in scopes.iter_mut().zip(flags.iter().copied()) {
        let state_offset = scope.feature_ordinal_offset() + 1;
        scope.tail_flags = flags.map(|(no_result, rolled_back)| {
            crate::records::feature::scope::DesignScopeTailFlags {
                suppression: [false; 2],
                unevaluated: false,
                wide_state: None,
                state_offset,
                no_result,
                rolled_back,
                no_result_offset: state_offset + 3,
                envelope_offset: None,
            }
        });
    }
    scopes
}

/// Envelopes whose flags differ only in location are equivalent. Envelopes
/// that differ in their no-result or rolled-back flags, or of which only some
/// store flags, with no history binding and no primary-indexed envelope, keep
/// the later envelope with its flags. One note names the record, the kept
/// flags and each envelope whose flags differ from them.
#[test]
fn scope_variants_that_differ_in_their_flags_keep_the_later_and_charge_a_loss() {
    let id = "f3d:stream:design-parameter-scope";
    let kept = |at: u64, flags: &str, other: &str| {
        format!("Feature scope {id}#{at} (record 42): the decoder kept {flags} from its later envelope at byte {at} (open item DR-1005). 1 other envelope(s) store different flags, which were not decoded: {other}.")
    };
    let cases: [(&[CopyFlags], Vec<String>); 5] = [
        (&[Some((true, true)), Some((true, true))], Vec::new()),
        (
            &[Some((true, true)), Some((false, true))],
            vec![kept(
                200,
                "state=000, no-result=false, rolled-back=true",
                "at byte 100, state=000, no-result=true, rolled-back=true",
            )],
        ),
        (
            &[Some((false, true)), Some((false, false))],
            vec![kept(
                200,
                "state=000, no-result=false, rolled-back=false",
                "at byte 100, state=000, no-result=false, rolled-back=true",
            )],
        ),
        (
            &[
                Some((true, false)),
                Some((false, false)),
                Some((false, false)),
            ],
            vec![kept(
                300,
                "state=000, no-result=false, rolled-back=false",
                "at byte 100, state=000, no-result=true, rolled-back=false",
            )],
        ),
        (
            &[None, Some((false, false))],
            vec![kept(
                200,
                "state=000, no-result=false, rolled-back=false",
                "at byte 100, none",
            )],
        ),
    ];
    for (flags, tail_flags) in cases {
        let mut variants = flagged_thicken_variants(flags);
        let later = variants.last().map(DesignParameterScope::byte_offset);
        let superseded = with_test_ctx(|ctx| {
            admit_history_bound_scope_variants(ctx, &mut variants, &[], &Default::default())
        })
        .expect("the document decodes");
        let [retained] = variants.as_slice() else {
            panic!("one retained envelope")
        };
        assert_eq!(Some(retained.byte_offset()), later, "flags {flags:?}");
        assert_eq!(
            retained
                .tail_flags
                .map(|flags| (flags.no_result, flags.rolled_back)),
            flags.last().copied().flatten(),
            "flags {flags:?}"
        );
        assert_eq!(
            superseded,
            SupersededScopeEnvelopes {
                primary_indexed: 0,
                tail_flags,
            },
            "flags {flags:?}"
        );
    }

    let mut primary_last = flagged_thicken_variants(&[
        Some((true, false)),
        Some((false, false)),
        Some((false, false)),
    ]);
    let stream = crate::ids::native_stream(&primary_last[0].id)
        .expect("fixture stream")
        .to_owned();
    let primary = std::collections::HashSet::from([(stream, 300)]);
    let superseded = with_test_ctx(|ctx| {
        admit_history_bound_scope_variants(ctx, &mut primary_last, &[], &primary)
    })
    .expect("primary-indexed envelope");
    assert_eq!(primary_last.len(), 1);
    assert_eq!(primary_last[0].byte_offset(), 300);
    assert_eq!(
        superseded,
        SupersededScopeEnvelopes {
            primary_indexed: 1,
            tail_flags: Vec::new(),
        }
    );

    let mut primary_earlier = flagged_thicken_variants(&[Some((true, true)), Some((false, true))]);
    let stream = crate::ids::native_stream(&primary_earlier[0].id)
        .expect("fixture stream")
        .to_owned();
    let primary = std::collections::HashSet::from([(stream, 100)]);
    let superseded = with_test_ctx(|ctx| {
        admit_history_bound_scope_variants(ctx, &mut primary_earlier, &[], &primary)
    })
    .expect("primary-indexed envelope");
    let [retained] = primary_earlier.as_slice() else {
        panic!("one retained envelope")
    };
    assert_eq!(retained.byte_offset(), 100);
    assert_eq!(retained.tail_flags.map(|flags| flags.no_result), Some(true));
    assert_eq!(
        superseded,
        SupersededScopeEnvelopes {
            primary_indexed: 1,
            tail_flags: Vec::new(),
        }
    );
}

/// Envelopes that differ only in their tail state bytes keep the later
/// envelope, with or without a primary-indexed envelope, and one note names
/// the record, the kept state and each envelope whose state differs.
#[test]
fn scope_variants_that_differ_only_in_their_state_bytes_keep_the_later_and_charge_a_loss() {
    let id = "f3d:stream:design-parameter-scope";
    let note = format!("Feature scope {id}#200 (record 42): the decoder kept state=000, no-result=true, rolled-back=false from its later envelope at byte 200 (open item DR-1005). 1 other envelope(s) store different flags, which were not decoded: at byte 100, state=010, no-result=true, rolled-back=false.");
    for primary_at in [None, Some(100), Some(200)] {
        let mut variants = flagged_thicken_variants(&[Some((true, false)), Some((true, false))]);
        if let Some(flags) = variants[0].tail_flags.as_mut() {
            flags.suppression = [false, true];
        }
        let stream = crate::ids::native_stream(&variants[0].id)
            .expect("fixture stream")
            .to_owned();
        let primary = primary_at
            .map(|at| (stream, at))
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let superseded = with_test_ctx(|ctx| {
            admit_history_bound_scope_variants(ctx, &mut variants, &[], &primary)
        })
        .expect("the document decodes");
        let [retained] = variants.as_slice() else {
            panic!("one retained envelope")
        };
        assert_eq!(retained.byte_offset(), 200, "primary {primary_at:?}");
        assert_eq!(
            retained.tail_flags.map(|flags| flags.suppressed()),
            Some(false),
            "primary {primary_at:?}"
        );
        assert_eq!(
            superseded,
            SupersededScopeEnvelopes {
                primary_indexed: 0,
                tail_flags: vec![note.clone()],
            },
            "primary {primary_at:?}"
        );
    }
}

/// The primary-indexed envelope is kept where envelopes also differ in their
/// lane flags, and the loss counts every other envelope whose payload or tail
/// state differs from it, also one that differs only in its state bytes.
#[test]
fn the_primary_indexed_count_includes_envelopes_that_differ_only_in_their_state_bytes() {
    let mut variants = flagged_thicken_variants(&[
        Some((true, false)),
        Some((true, false)),
        Some((false, false)),
        Some((true, false)),
    ]);
    if let Some(flags) = variants[1].tail_flags.as_mut() {
        flags.suppression = [false, true];
    }
    let stream = crate::ids::native_stream(&variants[0].id)
        .expect("fixture stream")
        .to_owned();
    let primary = std::collections::HashSet::from([(stream, 100)]);
    let superseded =
        with_test_ctx(|ctx| admit_history_bound_scope_variants(ctx, &mut variants, &[], &primary))
            .expect("primary-indexed envelope");
    let [retained] = variants.as_slice() else {
        panic!("one retained envelope")
    };
    assert_eq!(retained.byte_offset(), 100);
    assert_eq!(
        superseded,
        SupersededScopeEnvelopes {
            primary_indexed: 2,
            tail_flags: Vec::new(),
        }
    );
}

/// Where the later envelope stores no flags, the decoder keeps the flags of
/// the latest envelope that stores them, with their location, and one note
/// names the record, the envelope the flags came from and the flags it did not
/// decode. The kept scope reloads with those flags.
#[test]
fn a_later_envelope_without_flags_keeps_the_latest_stored_flags() {
    let id = "f3d:stream:design-parameter-scope";
    // The copies' flags, the copy whose flags are kept, and the note.
    let cases: [(&[CopyFlags], usize, String); 4] = [
        (
            &[Some((true, false)), None],
            0,
            format!("Feature scope {id}#200 (record 42): its later envelope at byte 200 stores no tail state, so the decoder kept state=000, no-result=true, rolled-back=false from the earlier envelope at byte 100 (open item DR-1005)."),
        ),
        (
            &[Some((false, true)), None],
            0,
            format!("Feature scope {id}#200 (record 42): its later envelope at byte 200 stores no tail state, so the decoder kept state=000, no-result=false, rolled-back=true from the earlier envelope at byte 100 (open item DR-1005)."),
        ),
        (
            &[Some((true, true)), None, None],
            0,
            format!("Feature scope {id}#300 (record 42): its later envelope at byte 300 stores no tail state, so the decoder kept state=000, no-result=true, rolled-back=true from the earlier envelope at byte 100 (open item DR-1005). 1 other envelope(s) store different flags, which were not decoded: at byte 200, none."),
        ),
        (
            &[Some((true, false)), Some((false, true)), None],
            1,
            format!("Feature scope {id}#300 (record 42): its later envelope at byte 300 stores no tail state, so the decoder kept state=000, no-result=false, rolled-back=true from the earlier envelope at byte 200 (open item DR-1005). 1 other envelope(s) store different flags, which were not decoded: at byte 100, state=000, no-result=true, rolled-back=false."),
        ),
    ];
    for (flags, source, note) in cases {
        let mut variants = flagged_thicken_variants(flags);
        let source_at = variants[source].byte_offset();
        let source = variants[source]
            .tail_flags
            .expect("the source copy stores flags");
        let later = variants.last().map(DesignParameterScope::byte_offset);
        let superseded = with_test_ctx(|ctx| {
            admit_history_bound_scope_variants(ctx, &mut variants, &[], &Default::default())
        })
        .expect("the document decodes");
        let [retained] = variants.as_slice() else {
            panic!("one retained envelope")
        };
        assert_eq!(Some(retained.byte_offset()), later, "flags {flags:?}");
        assert_eq!(
            retained.tail_flags,
            Some(crate::records::feature::scope::DesignScopeTailFlags {
                envelope_offset: Some(source_at),
                ..source
            }),
            "flags {flags:?}"
        );
        assert_eq!(
            superseded
                .losses()
                .map(|loss| loss.message)
                .collect::<Vec<_>>(),
            [note],
            "flags {flags:?}"
        );
        let reloaded = cadmpeg_core::serde_erase::json::from_value::<DesignParameterScope>(
            cadmpeg_core::serde_erase::json::to_value(retained).unwrap(),
        )
        .expect("the kept scope reloads");
        assert_eq!(reloaded.tail_flags, retained.tail_flags, "flags {flags:?}");
    }
}

/// Flags of an earlier envelope that do not lie before the later envelope's
/// start are not carried into it, and the note says so.
#[test]
fn flags_that_reach_the_later_envelope_are_not_carried() {
    let id = "f3d:stream:design-parameter-scope";
    let mut variants = flagged_thicken_variants(&[Some((true, false)), None]);
    let later = variants[1].byte_offset();
    if let Some(flags) = variants[0].tail_flags.as_mut() {
        flags.no_result_offset = later - 1;
    }
    let superseded = with_test_ctx(|ctx| {
        admit_history_bound_scope_variants(ctx, &mut variants, &[], &Default::default())
    })
    .expect("the document decodes");
    let [retained] = variants.as_slice() else {
        panic!("one retained envelope")
    };
    assert_eq!(retained.byte_offset(), later);
    assert_eq!(retained.tail_flags, None);
    assert_eq!(
        superseded
            .losses()
            .map(|loss| loss.message)
            .collect::<Vec<_>>(),
        [format!("Feature scope {id}#200 (record 42): its later envelope at byte 200 stores no tail state, and no earlier envelope stores it before that byte (open item DR-1005). 1 other envelope(s) store different flags, which were not decoded: at byte 100, state=000, no-result=true, rolled-back=false.")]
    );
}

/// Each record whose envelopes differ only in their flags has one note, and
/// the notes follow record order on every decode, whatever order the scopes
/// arrive in.
#[test]
fn flags_only_notes_follow_record_order() {
    let id = "f3d:stream:design-parameter-scope";
    let notes = [42, 43].map(|record| format!("Feature scope {id}#200 (record {record}): its later envelope at byte 200 stores no tail state, so the decoder kept state=000, no-result=true, rolled-back=false from the earlier envelope at byte 100 (open item DR-1005)."));
    for _ in 0..32 {
        let mut variants = Vec::new();
        for record in [43, 42] {
            let mut copies = flagged_thicken_variants(&[Some((true, false)), None]);
            for scope in &mut copies {
                scope.record_index = record;
            }
            variants.extend(copies);
        }
        let superseded = with_test_ctx(|ctx| {
            admit_history_bound_scope_variants(ctx, &mut variants, &[], &Default::default())
        })
        .expect("the document decodes");
        assert_eq!(
            superseded
                .losses()
                .map(|loss| loss.message)
                .collect::<Vec<_>>(),
            notes
        );
    }
}

/// Envelopes that differ in suppression and in their payload stay unresolved
/// without a history binding or a primary-indexed envelope.
#[test]
fn scope_variants_that_differ_in_suppression_and_payload_are_unresolved() {
    let mut variants = thicken_variants(&[-1.0, -2.0]);
    for (scope, suppressed) in variants.iter_mut().zip([true, false]) {
        scope.tail_flags = Some(crate::records::feature::scope::DesignScopeTailFlags {
            suppression: [suppressed, false],
            unevaluated: false,
            wide_state: None,
            state_offset: scope.feature_ordinal_offset() + 1,
            no_result: true,
            rolled_back: true,
            no_result_offset: scope.feature_ordinal_offset() + 4,
            envelope_offset: None,
        });
    }
    assert!(with_test_ctx(|ctx| admit_history_bound_scope_variants(
        ctx,
        &mut variants,
        &[],
        &Default::default()
    ))
    .is_err());
}

#[test]
fn superseded_scope_envelopes_name_their_difference_in_the_loss() {
    assert_eq!(SupersededScopeEnvelopes::default().losses().count(), 0);
    let losses = SupersededScopeEnvelopes {
        primary_indexed: 2,
        tail_flags: vec!["Feature scope f3d:stream:design-parameter-scope#200 (record 42): the decoder kept state=000, no-result=false, rolled-back=false from its later envelope at byte 200 (open item DR-1005).".to_owned()],
    }
    .losses()
    .collect::<Vec<_>>();
    let code = crate::loss::F3dLossCode::FeatureScopeEnvelopeSuperseded.kind();
    assert!(losses.iter().all(|loss| loss.code == code));
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.message.as_str())
            .collect::<Vec<_>>(),
        [
            "2 duplicate feature scope envelope(s) differ from the primary-indexed envelope of their record and were not decoded.",
            "Feature scope f3d:stream:design-parameter-scope#200 (record 42): the decoder kept state=000, no-result=false, rolled-back=false from its later envelope at byte 200 (open item DR-1005).",
        ]
    );
}

#[test]
fn duplicate_scope_selection_follows_header_link_when_transition_disagrees() {
    let mut bound = history(vec![
        history_state(10, Some(6)),
        history_state(6, Some(4)),
        history_state(4, None),
    ]);
    bound.states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .previous_state_id = Some(4);
    let histories = [bound];
    let mut omitted_predecessor = scope(43, 300, 10, 6);
    omitted_predecessor
        .try_edit(|draft| {
            draft.previous_history_state_id = None;
            draft.previous_history_state_id_offset = None;
        })
        .unwrap();
    let mut scopes = vec![
        scope(42, 100, 10, 6),
        scope(42, 200, 12, 11),
        omitted_predecessor,
        scope(43, 400, 12, 11),
    ];
    for index in [1, 3] {
        scopes[index].feature_ordinal = std::num::NonZeroU32::new(2).unwrap();
    }

    with_test_ctx(|ctx| {
        admit_history_bound_scope_variants(ctx, &mut scopes, &histories, &Default::default())
    })
    .expect("unique header-linked envelopes");

    assert_eq!(scopes.len(), 2);
    assert_eq!(scopes[0].record_index, 42);
    assert_eq!(scopes[0].byte_offset(), 100);
    assert_eq!(scopes[1].record_index, 43);
    assert_eq!(scopes[1].byte_offset(), 300);
}

#[test]
fn duplicate_scope_selection_is_equal_with_and_without_history_binding() {
    let bound = history(vec![
        history_state(10, Some(6)),
        history_state(6, Some(4)),
        history_state(4, Some(2)),
        history_state(2, None),
    ]);
    let unbound = unbound_history(vec![
        unbound_history_state(10, Some(6)),
        unbound_history_state(6, Some(4)),
        unbound_history_state(4, Some(2)),
        unbound_history_state(2, None),
    ]);
    assert!(bound
        .states()
        .iter()
        .all(|state| state.transition.is_some()));
    assert!(unbound
        .states()
        .iter()
        .all(|state| state.transition.is_none()));
    let mut omitted_predecessor = scope(44, 500, 6, 4);
    omitted_predecessor
        .try_edit(|draft| {
            draft.previous_history_state_id = None;
            draft.previous_history_state_id_offset = None;
        })
        .unwrap();
    let mut scopes = vec![
        scope(42, 100, 10, 6),
        scope(42, 200, 12, 11),
        scope(43, 300, 10, 4),
        scope(43, 400, 12, 11),
        omitted_predecessor,
        scope(44, 600, 12, 11),
    ];
    for index in [1, 3, 5] {
        scopes[index].feature_ordinal = std::num::NonZeroU32::new(2).unwrap();
    }
    let expected_ids = [0, 2, 4].map(|index| scopes[index].id.clone());
    let mut bound_scopes = scopes.clone();
    let mut unbound_scopes = scopes;

    with_test_ctx(|ctx| {
        admit_history_bound_scope_variants(ctx, &mut bound_scopes, &[bound], &Default::default())
    })
    .expect("bound history envelopes");
    with_test_ctx(|ctx| {
        admit_history_bound_scope_variants(
            ctx,
            &mut unbound_scopes,
            &[unbound],
            &Default::default(),
        )
    })
    .expect("unbound history envelopes");

    let retained_ids = |scopes: &[DesignParameterScope]| {
        scopes
            .iter()
            .map(|scope| scope.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(retained_ids(&bound_scopes), retained_ids(&unbound_scopes));
    assert_eq!(retained_ids(&bound_scopes), expected_ids);
}

/// Without a history binding, the one envelope that the stream's primary
/// record index names is admitted even when the payloads differ.
#[test]
fn primary_indexed_scope_variant_is_admitted_without_a_history_binding() {
    let mut different = thicken_variants(&[-1.0, -2.0]);
    let stream = crate::ids::native_stream(&different[0].id)
        .expect("fixture stream")
        .to_owned();
    let primary = std::collections::HashSet::from([(stream.clone(), 100)]);
    let superseded =
        with_test_ctx(|ctx| admit_history_bound_scope_variants(ctx, &mut different, &[], &primary))
            .expect("primary-indexed envelope");
    assert_eq!(superseded.primary_indexed, 1);
    assert_eq!(different.len(), 1);
    assert_eq!(different[0].byte_offset(), 100);

    let mut first_of_three = thicken_variants(&[-1.0, -1.0, -2.0]);
    let superseded = with_test_ctx(|ctx| {
        admit_history_bound_scope_variants(ctx, &mut first_of_three, &[], &primary)
    })
    .expect("primary-indexed envelope");
    assert_eq!(superseded.primary_indexed, 1);
    assert_eq!(first_of_three.len(), 1);
    assert_eq!(first_of_three[0].byte_offset(), 100);

    let mut both = thicken_variants(&[-1.0, -2.0]);
    let primary = std::collections::HashSet::from([(stream.clone(), 100), (stream, 200)]);
    assert!(
        with_test_ctx(|ctx| admit_history_bound_scope_variants(ctx, &mut both, &[], &primary))
            .is_err()
    );
}
