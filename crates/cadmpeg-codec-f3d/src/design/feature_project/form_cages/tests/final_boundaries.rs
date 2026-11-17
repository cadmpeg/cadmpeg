use super::super as form_cages;
use super::*;

#[test]
fn tracking_owner_survives_missing_or_impossible_member_count() {
    let mut local = Vec::new();
    push_marked(&mut local, 220);
    let mut typed = vec![1];
    typed.extend_from_slice(&220u64.to_le_bytes());
    typed.extend_from_slice(&36u32.to_le_bytes());
    typed.extend_from_slice(b"803DEC43-AEA6-42CC-B8D0-F593B5124BDD");
    typed.extend_from_slice(&[0, 0]);
    for (owner, expected) in [
        (
            local,
            FormNativeReferenceTarget::Local {
                target: cadmpeg_ir::ids::NativeId::mint(
                    "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-record#220",
                )
                .unwrap(),
            },
        ),
        (
            typed,
            FormNativeReferenceTarget::Local {
                target: cadmpeg_ir::ids::NativeId::mint(
                    "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-record#220",
                )
                .unwrap(),
            },
        ),
        (vec![0], FormNativeReferenceTarget::Null {}),
        (vec![2], FormNativeReferenceTarget::Unreadable {}),
    ] {
        for count in [None, Some(u32::MAX), Some(0)] {
            let mut bytes = indexed_frame(b"429", 226, 21);
            bytes.extend_from_slice(&owner);
            if let Some(count) = count {
                bytes.extend_from_slice(&count.to_le_bytes());
                bytes.extend_from_slice(&[0; 52]);
            }
            bytes.extend_from_slice(&indexed_frame(b"264", 226, 15));
            let scope = form_scope(220, vec![226]);
            let meta = form_meta(&[("E079B079-016A-43C2-ACC7-9627B32A8960", 0, &[226])]);
            let (inputs, losses) = crate::design::test_support::with_test_decode_context(|ctx| {
                let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
                let mut facts = form_cages::FormFacts {
                    inputs: FormInputs::default(),
                    observations: Vec::new(),
                };
                let mut losses = Vec::new();
                facts.tracking(
                    ctx,
                    form_cages::FormSource {
                        bytes: &bytes,
                        records: &records,
                        scope: &scope,
                        cages: &[],
                        stream: "FusionAssetName[Active]/Design1/BulkStream.dat",
                    },
                    &FormRecordTypes::of(&meta),
                    &mut losses,
                )?;
                Ok::<_, cadmpeg_core::CodecError>((facts.inputs, losses))
            })
            .unwrap();
            let reference = inputs
                .reference_evidence
                .iter()
                .find(|reference| {
                    reference.role == FormReferenceRole::TrackingOwner && reference.ordinal == 0
                })
                .unwrap();
            assert_eq!(reference.target, expected);
            assert_eq!(
                reference.source.as_str(),
                "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-record#226"
            );
            if !matches!(expected, FormNativeReferenceTarget::Unreadable {}) {
                let complete = count == Some(0);
                assert_eq!(inputs.lists.len(), usize::from(complete));
                assert_eq!(
                    losses.iter().any(
                        |loss| loss.code.local_code() == "feature.form-tracking-run-unresolved"
                    ),
                    !complete
                );
            }
        }
    }
}

#[test]
fn complete_assumed_membership_accepts_full_relative_source_order() {
    let full = FormConstruction::complete(vec![
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap(),
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#b").unwrap(),
    ])
    .unwrap();
    let assumed = FormConstruction::try_new(
        FormInputs::default(),
        full.observations().to_vec(),
        FormMembership::Complete {
            members: vec![FormObservationIndex(0), FormObservationIndex(1)],
            order: FormKnownOrder::Assumed,
            certainty: FormKnownOrder::Assumed,
        },
    )
    .unwrap();
    for cardinality in [None, Some(2)] {
        for positions in [[None, None], [Some(0), None], [Some(0), Some(1)]] {
            let ordered = FormConstruction::try_new(
                FormInputs::default(),
                full.observations().to_vec(),
                FormMembership::Partial {
                    cardinality,
                    order: FormOrder::Source,
                    members: vec![
                        FormMember {
                            observation: FormObservationIndex(0),
                            ordinal: positions[0],
                        },
                        FormMember {
                            observation: FormObservationIndex(1),
                            ordinal: positions[1],
                        },
                    ],
                },
            )
            .unwrap();
            for (left, right) in [(&assumed, &ordered), (&ordered, &assumed)] {
                let result = retain_ungrouped(left, right).unwrap();
                assert!(matches!(
                    result.membership(),
                    FormMembership::Complete {
                        order: FormKnownOrder::Source,
                        certainty: FormKnownOrder::Assumed,
                        ..
                    }
                ));
                assert_eq!(retain_ungrouped(&result, right).unwrap(), result);
            }
        }
        let subset = FormConstruction::try_new(
            FormInputs::default(),
            full.observations().to_vec(),
            FormMembership::Partial {
                cardinality,
                order: FormOrder::Source,
                members: vec![FormMember {
                    observation: FormObservationIndex(0),
                    ordinal: None,
                }],
            },
        )
        .unwrap();
        let result = retain_ungrouped(&assumed, &subset).unwrap();
        assert!(matches!(
            result.membership(),
            FormMembership::Complete {
                order: FormKnownOrder::Assumed,
                certainty: FormKnownOrder::Assumed,
                ..
            }
        ));
    }
}

#[test]
fn repeated_serializer_aliases_select_one_distinct_cage() {
    let full = FormConstruction::complete(vec![
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap(),
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#b").unwrap(),
    ])
    .unwrap();
    let facts = form_cages::FormFacts {
        inputs: FormInputs::default(),
        observations: full.observations().to_vec(),
    };
    for (alternatives, selected) in [
        (vec![0], true),
        (vec![0, 0], true),
        (vec![0, 0, 0], true),
        (vec![0, 1], false),
        (Vec::new(), false),
    ] {
        let membership = facts
            .membership(
                vec![alternatives.into_iter().map(FormObservationIndex).collect()],
                Some(1),
                FormOrder::Source,
                FormKnownOrder::Source,
                true,
            )
            .unwrap();
        let result =
            FormConstruction::try_new(facts.inputs.clone(), facts.observations.clone(), membership)
                .unwrap();
        assert_eq!(result.membership_is_complete(), selected);
        assert_eq!(
            result
                .committed_cages()
                .map(cadmpeg_ir::ids::SubdId::as_str)
                .collect::<Vec<_>>(),
            if selected {
                vec!["f3d:model:subd#a"]
            } else {
                Vec::new()
            }
        );
    }
}

#[test]
fn accumulated_source_order_is_independent_of_grouping() {
    let full = FormConstruction::complete(vec![
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap(),
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#b").unwrap(),
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#c").unwrap(),
    ])
    .unwrap();
    let assumed = FormConstruction::try_new(
        FormInputs::default(),
        full.observations().to_vec(),
        FormMembership::Complete {
            members: vec![
                FormObservationIndex(0),
                FormObservationIndex(1),
                FormObservationIndex(2),
            ],
            order: FormKnownOrder::Assumed,
            certainty: FormKnownOrder::Assumed,
        },
    )
    .unwrap();
    let partial = |indices: &[usize]| {
        FormConstruction::try_new(
            FormInputs::default(),
            full.observations().to_vec(),
            FormMembership::Partial {
                cardinality: None,
                order: FormOrder::Source,
                members: indices
                    .iter()
                    .map(|&index| FormMember {
                        observation: FormObservationIndex(index),
                        ordinal: None,
                    })
                    .collect(),
            },
        )
        .unwrap()
    };
    let ab = partial(&[0, 1]);
    let bc = partial(&[1, 2]);
    let join =
        |left: &FormConstruction, right: &FormConstruction| retain_ungrouped(left, right).unwrap();
    let grouped = join(&ab, &bc);
    for result in [
        join(&join(&assumed, &ab), &bc),
        join(&join(&ab, &assumed), &bc),
        join(&join(&assumed, &bc), &ab),
        join(&assumed, &grouped),
        join(&grouped, &assumed),
    ] {
        assert!(
            matches!(
                result.membership(),
                FormMembership::Complete {
                    order: FormKnownOrder::Source,
                    certainty: FormKnownOrder::Assumed,
                    ..
                }
            ),
            "{:?}",
            result.membership()
        );
        assert_eq!(
            result
                .committed_cages()
                .map(cadmpeg_ir::ids::SubdId::as_str)
                .collect::<Vec<_>>(),
            ["f3d:model:subd#a", "f3d:model:subd#b", "f3d:model:subd#c"]
        );
        assert_eq!(join(&result, &bc), result);
    }
    let insufficient = join(&assumed, &ab);
    assert!(matches!(
        insufficient.membership(),
        FormMembership::Complete {
            order: FormKnownOrder::Assumed,
            certainty: FormKnownOrder::Assumed,
            ..
        }
    ));
    let reversed = partial(&[1, 0]);
    assert!(!join(&assumed, &reversed).membership_is_complete());
    let mut evidence = FormInputs::default();
    evidence.order_evidence.push(FormOrderEvidence {
        order: FormKnownOrder::Source,
        members: vec![
            FormObservationIndex(0),
            FormObservationIndex(1),
            FormObservationIndex(2),
        ],
    });
    let evidence_only = FormConstruction::try_new(
        evidence,
        full.observations().to_vec(),
        FormConstruction::default().membership().clone(),
    )
    .unwrap();
    assert!(matches!(
        join(&assumed, &evidence_only).membership(),
        FormMembership::Complete {
            order: FormKnownOrder::Source,
            certainty: FormKnownOrder::Assumed,
            ..
        }
    ));
}
