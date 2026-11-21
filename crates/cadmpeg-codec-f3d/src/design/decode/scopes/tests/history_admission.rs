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

/// The suppressed and rolled-back flags of one envelope's tail, `None` where
/// the tail stores none.
type CopyFlags = Option<(bool, bool)>;

/// Equal Thicken envelopes as `thicken_variants` builds them, one per entry of
/// `flags`, whose tails store that entry's flags, each at its own byte
/// location.
fn flagged_thicken_variants(flags: &[CopyFlags]) -> Vec<DesignParameterScope> {
    let mut scopes = thicken_variants(&vec![-1.0; flags.len()]);
    for (scope, flags) in scopes.iter_mut().zip(flags.iter().copied()) {
        let suppressed_offset = scope.feature_ordinal_offset() + 1;
        scope.tail_flags = flags.map(|(suppressed, rolled_back)| {
            crate::records::feature::scope::DesignScopeTailFlags {
                suppressed,
                rolled_back,
                suppressed_offset,
            }
        });
    }
    scopes
}

/// Envelopes whose flags differ only in location are equivalent. Envelopes
/// that differ in their suppressed or rolled-back flags, or of which only some
/// store flags, with no history binding and no primary-indexed envelope, keep
/// the later envelope and charge each envelope whose flags differ from it.
#[test]
fn scope_variants_that_differ_in_their_flags_keep_the_later_and_charge_a_loss() {
    let cases: [(&[CopyFlags], usize); 5] = [
        (&[Some((true, true)), Some((true, true))], 0),
        (&[Some((true, true)), Some((false, true))], 1),
        (&[Some((false, true)), Some((false, false))], 1),
        (
            &[
                Some((true, false)),
                Some((false, false)),
                Some((false, false)),
            ],
            1,
        ),
        (&[None, Some((false, false))], 1),
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
                .map(|flags| (flags.suppressed, flags.rolled_back)),
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
            tail_flags: 0,
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
    assert_eq!(
        retained.tail_flags.map(|flags| flags.suppressed),
        Some(true)
    );
    assert_eq!(
        superseded,
        SupersededScopeEnvelopes {
            primary_indexed: 1,
            tail_flags: 0,
        }
    );
}

/// Envelopes that differ in suppression and in their payload stay unresolved
/// without a history binding or a primary-indexed envelope.
#[test]
fn scope_variants_that_differ_in_suppression_and_payload_are_unresolved() {
    let mut variants = thicken_variants(&[-1.0, -2.0]);
    for (scope, suppressed) in variants.iter_mut().zip([true, false]) {
        scope.tail_flags = Some(crate::records::feature::scope::DesignScopeTailFlags {
            suppressed,
            rolled_back: true,
            suppressed_offset: scope.feature_ordinal_offset() + 1,
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
        tail_flags: 1,
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
            "1 duplicate feature scope envelope(s) differ from the later envelope of their record only in their suppressed or rolled-back flags; the later envelope's flags were decoded and theirs were not.",
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

    let mut both = thicken_variants(&[-1.0, -2.0]);
    let primary = std::collections::HashSet::from([(stream.clone(), 100), (stream, 200)]);
    assert!(
        with_test_ctx(|ctx| admit_history_bound_scope_variants(ctx, &mut both, &[], &primary))
            .is_err()
    );
}
