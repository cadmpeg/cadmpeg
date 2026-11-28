use super::super as form_cages;
use super::*;
use cadmpeg_ir::ids::{NativeId, SubdId};
use std::collections::BTreeSet;

fn resource(source: &str, entry: &str) -> FormResource {
    FormResource {
        source: NativeId::mint(source).unwrap(),
        entry: cadmpeg_core::text::NonBlankString::new(entry.to_owned()).unwrap(),
    }
}

fn decoded(cage: &str, resource: Option<FormResource>) -> FormCageObservation {
    FormCageObservation {
        paths: Vec::new(),
        binding: FormCageBinding::Decoded {
            cage: SubdId::mint(cage).unwrap(),
            resource,
        },
    }
}

fn candidate(
    resource: FormResource,
    availability: FormResourceAvailability,
) -> FormCageObservation {
    FormCageObservation {
        paths: Vec::new(),
        binding: FormCageBinding::Undecoded {
            resource,
            availability,
        },
    }
}

fn complete(observations: Vec<FormCageObservation>, members: Vec<usize>) -> FormConstruction {
    FormConstruction::try_new(
        FormInputs::default(),
        observations,
        FormMembership::Complete {
            members: members.into_iter().map(FormObservationIndex).collect(),
            order: FormKnownOrder::Source,
            certainty: FormKnownOrder::Source,
        },
    )
    .unwrap()
}

fn unselected(observations: Vec<FormCageObservation>) -> FormConstruction {
    FormConstruction::try_new(
        FormInputs::default(),
        observations,
        FormMembership::Partial {
            cardinality: None,
            order: FormOrder::Unknown,
            members: Vec::new(),
        },
    )
    .unwrap()
}

fn associations(form: &FormConstruction) -> BTreeSet<(String, String, String)> {
    let bindings =
        form.observations()
            .iter()
            .filter_map(|observation| match &observation.binding {
                FormCageBinding::Decoded {
                    cage,
                    resource: Some(resource),
                } => Some((cage, resource)),
                _ => None,
            });
    bindings
        .chain(
            form.inputs()
                .resource_associations
                .iter()
                .map(|association| (&association.cage, &association.resource)),
        )
        .map(|(cage, resource)| {
            (
                cage.as_str().to_owned(),
                resource.source.as_str().to_owned(),
                resource.entry.as_str().to_owned(),
            )
        })
        .collect()
}

fn assert_selected_a_with_closure(form: &FormConstruction) {
    assert!(!form.membership_is_complete());
    assert_eq!(
        form.committed_cages()
            .map(SubdId::as_str)
            .collect::<Vec<_>>(),
        ["f3d:model:subd#a"]
    );
    assert!(form.inputs().closure_evidence.iter().any(|claim| {
        claim.members.len() == 1
            && matches!(
                &form.observations()[claim.members[0].0].binding,
                FormCageBinding::Decoded { cage, .. } if cage.as_str() == "f3d:model:subd#a"
            )
            && claim.order == FormKnownOrder::Source
            && claim.certainty == FormKnownOrder::Source
    }));
}

#[test]
fn ambiguous_resource_keeps_cage_association_and_prior_closure_in_both_orders() {
    let r = resource("f3d:scope:record#r", "asset/r.tsm");
    let known = complete(vec![decoded("f3d:model:subd#a", Some(r.clone()))], vec![0]);
    let ambiguous = unselected(vec![candidate(r, FormResourceAvailability::Ambiguous)]);
    for (old, new) in [(&known, &ambiguous), (&ambiguous, &known)] {
        let result = retain_ungrouped(old, new).unwrap();
        assert_selected_a_with_closure(&result);
        assert_eq!(
            associations(&result),
            BTreeSet::from([(
                "f3d:model:subd#a".into(),
                "f3d:scope:record#r".into(),
                "asset/r.tsm".into(),
            )])
        );
        assert_eq!(retain_ungrouped(&result, new).unwrap(), result);
    }
}

#[test]
fn aliases_survive_both_orders_and_later_inactive_or_ambiguous_evidence() {
    let r = resource("f3d:scope:record#r", "asset/r.tsm");
    for (source, entry) in [
        ("f3d:scope:record#t", "asset/t.tsm"),
        ("f3d:scope:record#a", "asset/a.tsm"),
        ("f3d:scope:record#other", "asset/r.tsm"),
    ] {
        let t = resource(source, entry);
        let first = complete(vec![decoded("f3d:model:subd#a", Some(r.clone()))], vec![0]);
        let second = complete(vec![decoded("f3d:model:subd#a", Some(t.clone()))], vec![0]);
        for (old, new) in [(&first, &second), (&second, &first)] {
            let joined = retain_ungrouped(old, new).unwrap();
            assert!(joined.membership_is_complete());
            assert_eq!(
                associations(&joined),
                BTreeSet::from([
                    (
                        "f3d:model:subd#a".into(),
                        "f3d:scope:record#r".into(),
                        "asset/r.tsm".into()
                    ),
                    ("f3d:model:subd#a".into(), source.into(), entry.into()),
                ])
            );
            for availability in [
                FormResourceAvailability::Inactive,
                FormResourceAvailability::Ambiguous,
            ] {
                let contradicts = unselected(vec![candidate(t.clone(), availability)]);
                let result = retain_ungrouped(&joined, &contradicts).unwrap();
                assert_selected_a_with_closure(&result);
                assert_eq!(associations(&result), associations(&joined));
                assert_eq!(retain_ungrouped(&result, &contradicts).unwrap(), result);
            }
        }
    }
}

#[test]
fn only_selected_associations_control_closure() {
    let r = resource("f3d:scope:record#r", "asset/r.tsm");
    let t = resource("f3d:scope:record#t", "asset/t.tsm");
    let base = complete(
        vec![
            decoded("f3d:model:subd#a", Some(r.clone())),
            decoded("f3d:model:subd#b", Some(t.clone())),
        ],
        vec![0],
    );
    for availability in [
        FormResourceAvailability::Inactive,
        FormResourceAvailability::Ambiguous,
    ] {
        let other = unselected(vec![candidate(t.clone(), availability)]);
        let result = retain_ungrouped(&base, &other).unwrap();
        assert!(result.membership_is_complete());
        assert_eq!(
            result
                .committed_cages()
                .map(SubdId::as_str)
                .collect::<Vec<_>>(),
            ["f3d:model:subd#a"]
        );
        assert_eq!(retain_ungrouped(&result, &other).unwrap(), result);
    }
    for availability in [
        FormResourceAvailability::Unknown,
        FormResourceAvailability::ActiveUndecoded,
    ] {
        let compatible = unselected(vec![candidate(r.clone(), availability)]);
        assert!(retain_ungrouped(&base, &compatible)
            .unwrap()
            .membership_is_complete());
    }
    let without_resource = complete(vec![decoded("f3d:model:subd#a", None)], vec![0]);
    let unrelated = unselected(vec![candidate(t, FormResourceAvailability::Ambiguous)]);
    assert!(retain_ungrouped(&without_resource, &unrelated)
        .unwrap()
        .membership_is_complete());
    assert!(
        retain_ungrouped(&complete(Vec::new(), Vec::new()), &unrelated)
            .unwrap()
            .membership_is_complete()
    );
}

#[test]
fn shared_entry_remains_associated_with_each_cage_and_demotes_selected_cage() {
    let r1 = resource("f3d:scope:record#r1", "asset/r.tsm");
    let r2 = resource("f3d:scope:record#r2", "asset/r.tsm");
    let selected = complete(vec![decoded("f3d:model:subd#a", Some(r1))], vec![0]);
    let nonselected = unselected(vec![decoded("f3d:model:subd#b", Some(r2))]);
    for (old, new) in [(&selected, &nonselected), (&nonselected, &selected)] {
        let result = retain_ungrouped(old, new).unwrap();
        assert_selected_a_with_closure(&result);
        assert_eq!(
            associations(&result),
            BTreeSet::from([
                (
                    "f3d:model:subd#a".into(),
                    "f3d:scope:record#r1".into(),
                    "asset/r.tsm".into()
                ),
                (
                    "f3d:model:subd#b".into(),
                    "f3d:scope:record#r2".into(),
                    "asset/r.tsm".into()
                ),
            ])
        );
        assert_eq!(retain_ungrouped(&result, new).unwrap(), result);
    }
}

#[test]
fn candidate_before_same_cage_remaps_positions_order_and_closure() {
    let r = resource("f3d:scope:record#r", "asset/r.tsm");
    let t = resource("f3d:scope:record#t", "asset/t.tsm");
    let u = resource("f3d:scope:record#u", "asset/u.tsm");
    let old = FormConstruction::try_new(
        FormInputs {
            position_evidence: vec![
                FormPositionEvidence {
                    source: None,
                    observation: FormObservationIndex(0),
                    ordinal: 0,
                },
                FormPositionEvidence {
                    source: None,
                    observation: FormObservationIndex(2),
                    ordinal: 1,
                },
            ],
            order_evidence: vec![FormOrderEvidence {
                order: FormKnownOrder::Source,
                members: vec![FormObservationIndex(0), FormObservationIndex(2)],
            }],
            closure_evidence: vec![FormClosureEvidence {
                members: vec![FormObservationIndex(0), FormObservationIndex(2)],
                order: FormKnownOrder::Source,
                certainty: FormKnownOrder::Source,
            }],
            ..FormInputs::default()
        },
        vec![
            candidate(t.clone(), FormResourceAvailability::Unknown),
            decoded("f3d:model:subd#a", Some(r)),
            decoded("f3d:model:subd#b", Some(u.clone())),
        ],
        FormMembership::Partial {
            cardinality: None,
            order: FormOrder::Unknown,
            members: Vec::new(),
        },
    )
    .unwrap();
    let new = complete(
        vec![
            decoded("f3d:model:subd#a", Some(t)),
            decoded("f3d:model:subd#b", Some(u)),
        ],
        vec![0, 1],
    );
    let result = retain_ungrouped(&old, &new).unwrap();
    assert!(result.membership_is_complete());
    assert_eq!(
        result
            .committed_cages()
            .map(SubdId::as_str)
            .collect::<Vec<_>>(),
        ["f3d:model:subd#a", "f3d:model:subd#b"]
    );
    assert_eq!(result.observations().len(), 2);
    assert_eq!(
        result
            .inputs()
            .position_evidence
            .iter()
            .map(|e| (e.observation.0, e.ordinal))
            .collect::<Vec<_>>(),
        [(0, 0), (1, 1)]
    );
    assert_eq!(
        result.inputs().order_evidence[0].members,
        [FormObservationIndex(0), FormObservationIndex(1)]
    );
    assert_eq!(
        result.inputs().closure_evidence[0].members,
        [FormObservationIndex(0), FormObservationIndex(1)]
    );
    assert_eq!(retain_ungrouped(&result, &new).unwrap(), result);
}

#[test]
fn multiple_cages_from_each_serializer_route_keep_all_associations() {
    let uuid = "00000000-0000-4000-8000-000000000099";
    let mut serializer = form_serializer_frame(299, 296, uuid);
    serializer[4..7].copy_from_slice(b"315");
    let bytes = [serializer, indexed_frame(b"258", 300, 15)].concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let mut serializers = parsed_serializers(&bytes, &records);
    let scope = form_scope(201, Vec::new());
    let cages = [
        neutral_form_cage("f3d:model:subd#a", uuid),
        neutral_form_cage("f3d:model:subd#b", uuid),
    ];
    let archive = crate::test_support::zip_fixture::f3d_with_form_body_resource(b"synthetic");
    crate::test_support::zip_fixture::with_scan(&archive, |scan| {
        crate::design::test_support::with_test_decode_context(|ctx| {
            let source = form_cages::FormSource { bytes: &bytes, records: &records, scope: &scope, cages: &cages, stream: "FusionAssetName[Active]/Design1/BulkStream.dat" };
            form_cages::bind_serializers(ctx, source, &mut serializers, &mut form_cages::ReferenceGroups::default())?;
            for surface_route in [false, true] {
                let mut facts = form_cages::FormFacts { inputs: FormInputs::default(), observations: Vec::new() };
                if surface_route {
                    facts.surface(ctx, scan, source, form_cages::FormSurface { path: form_cages::input_path(&scope, &[296])?, record: Some(296) }, &serializers, &mut Vec::new())?;
                } else {
                    form_cages::retain_serializer_candidate(ctx, scan, source, &serializers.entries[&296][0], &mut facts)?;
                }
                facts.prepare_resources();
                let form = FormConstruction::try_new(facts.inputs, facts.observations, FormConstruction::default().membership().clone()).unwrap();
                assert_eq!(form.referenced_cages().map(SubdId::as_str).collect::<BTreeSet<_>>(), BTreeSet::from(["f3d:model:subd#a", "f3d:model:subd#b"]));
                assert_eq!(associations(&form), BTreeSet::from([
                    ("f3d:model:subd#a".into(), "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-record#299".into(), "FusionAssetName[Active]/TSplines.BlobParts/TSpline.00000000-0000-4000-8000-000000000099.tsm".into()),
                    ("f3d:model:subd#b".into(), "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-record#299".into(), "FusionAssetName[Active]/TSplines.BlobParts/TSpline.00000000-0000-4000-8000-000000000099.tsm".into()),
                ]));
            }
            Ok::<_, cadmpeg_core::CodecError>(())
        })
    }).unwrap();
}

#[test]
fn sharing_an_alias_after_enrichment_invalidates_the_selected_cage() {
    let r = resource("f3d:scope:record#r", "asset/r.tsm");
    let t = resource("f3d:scope:record#t", "asset/t.tsm");
    let a_r = complete(vec![decoded("f3d:model:subd#a", Some(r))], vec![0]);
    let a_t = complete(vec![decoded("f3d:model:subd#a", Some(t.clone()))], vec![0]);
    let b_t = unselected(vec![decoded("f3d:model:subd#b", Some(t))]);
    for (old, new) in [(&a_r, &a_t), (&a_t, &a_r)] {
        let aliases = retain_ungrouped(old, new).unwrap();
        let result = retain_ungrouped(&aliases, &b_t).unwrap();
        assert_selected_a_with_closure(&result);
        assert_eq!(
            associations(&result),
            BTreeSet::from([
                (
                    "f3d:model:subd#a".into(),
                    "f3d:scope:record#r".into(),
                    "asset/r.tsm".into()
                ),
                (
                    "f3d:model:subd#a".into(),
                    "f3d:scope:record#t".into(),
                    "asset/t.tsm".into()
                ),
                (
                    "f3d:model:subd#b".into(),
                    "f3d:scope:record#t".into(),
                    "asset/t.tsm".into()
                ),
            ])
        );
        assert_eq!(retain_ungrouped(&result, &b_t).unwrap(), result);
    }
}
#[test]
fn active_state_survives_coalescing_before_later_inactive_state() {
    let r = resource("f3d:scope:record#r", "asset/r.tsm");
    let known = complete(vec![decoded("f3d:model:subd#a", Some(r.clone()))], vec![0]);
    let active = unselected(vec![candidate(
        r.clone(),
        FormResourceAvailability::ActiveUndecoded,
    )]);
    let inactive = unselected(vec![candidate(
        r.clone(),
        FormResourceAvailability::Inactive,
    )]);
    for (old, new) in [(&known, &active), (&active, &known)] {
        let joined = retain_ungrouped(old, new).unwrap();
        assert!(joined.membership_is_complete());
        assert!(joined
            .inputs()
            .resource_state_evidence
            .contains(&FormResourceStateEvidence {
                resource: r.clone(),
                availability: FormResourceAvailability::ActiveUndecoded,
            }));
        assert_eq!(retain_ungrouped(&joined, &active).unwrap(), joined);
        let result = retain_ungrouped(&joined, &inactive).unwrap();
        assert_selected_a_with_closure(&result);
        for availability in [
            FormResourceAvailability::ActiveUndecoded,
            FormResourceAvailability::Inactive,
        ] {
            assert!(result
                .inputs()
                .resource_state_evidence
                .contains(&FormResourceStateEvidence {
                    resource: r.clone(),
                    availability,
                }));
        }
        assert_eq!(retain_ungrouped(&result, &inactive).unwrap(), result);
    }
}

#[test]
fn shared_alias_does_not_attach_unrelated_cage_paths_to_candidate() {
    let path = |id| FormInputPath {
        references: cadmpeg_ir::features::selections::NonEmptyMembers::one(
            NativeId::mint(id).unwrap(),
        ),
    };
    let r = resource("f3d:scope:record#r", "asset/r.tsm");
    let t = resource("f3d:scope:record#t", "asset/t.tsm");
    let mut ar = decoded("f3d:model:subd#a", Some(r));
    ar.paths.push(path("f3d:scope:record#path-r"));
    let mut at = decoded("f3d:model:subd#a", Some(t.clone()));
    at.paths.push(path("f3d:scope:record#path-t"));
    let mut bt = decoded("f3d:model:subd#b", Some(t));
    bt.paths.push(path("f3d:scope:record#path-b"));
    let aliases =
        retain_ungrouped(&complete(vec![ar], vec![0]), &complete(vec![at], vec![0])).unwrap();
    let result = retain_ungrouped(&aliases, &unselected(vec![bt])).unwrap();
    let a = result.observations().iter().find(|observation| matches!(&observation.binding, FormCageBinding::Decoded { cage, .. } if cage.as_str() == "f3d:model:subd#a")).unwrap();
    assert!(a.paths.contains(&path("f3d:scope:record#path-r")));
    assert!(a.paths.contains(&path("f3d:scope:record#path-t")));
    let candidate_observation = result.observations().iter().find(|observation| matches!(&observation.binding, FormCageBinding::Undecoded { resource, availability: FormResourceAvailability::Ambiguous } if resource.entry.as_str() == "asset/t.tsm")).unwrap();
    assert!(candidate_observation.paths.is_empty());
    let mut observed = candidate(
        resource("f3d:scope:record#t", "asset/t.tsm"),
        FormResourceAvailability::Ambiguous,
    );
    observed.paths.push(path("f3d:scope:record#observed-t"));
    let enriched = retain_ungrouped(&result, &unselected(vec![observed])).unwrap();
    let candidate = enriched.observations().iter().find(|observation| matches!(&observation.binding, FormCageBinding::Undecoded { resource, availability: FormResourceAvailability::Ambiguous } if resource.entry.as_str() == "asset/t.tsm")).unwrap();
    assert_eq!(candidate.paths, [path("f3d:scope:record#observed-t")]);
}
