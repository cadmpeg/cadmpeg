use super::super as form_cages;
use super::*;

#[test]
fn review1368_compatible_partial_preserves_complete_membership() {
    let complete =
        FormConstruction::complete(vec![
            cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#known").unwrap()
        ])
        .unwrap();
    let partial = FormConstruction::try_new(
        FormInputs::default(),
        complete.observations().to_vec(),
        FormMembership::Partial {
            cardinality: Some(1),
            order: FormOrder::Source,
            members: vec![FormMember {
                observation: FormObservationIndex(0),
                ordinal: Some(0),
            }],
        },
    )
    .unwrap();
    let result = retain_ungrouped(&complete, &partial).unwrap();
    assert!(result.membership_is_complete());
    assert_eq!(
        result
            .committed_cages()
            .map(cadmpeg_ir::ids::SubdId::as_str)
            .collect::<Vec<_>>(),
        ["f3d:model:subd#known"]
    );
}

#[test]
fn review1368_unknown_to_inactive_preserves_valid_partial_evidence() {
    let make = |availability, selected| {
        FormConstruction::try_new(
            FormInputs::default(),
            vec![FormCageObservation {
                paths: Vec::new(),
                binding: FormCageBinding::Undecoded {
                    resource: FormResource {
                        source: cadmpeg_ir::ids::NativeId::mint("f3d:scope:record#299").unwrap(),
                        entry: cadmpeg_core::nonblank_literal!("asset/TSplines.BlobParts/cage.tsm"),
                    },
                    availability,
                },
            }],
            FormMembership::Partial {
                cardinality: None,
                order: FormOrder::Unknown,
                members: if selected {
                    vec![FormMember {
                        observation: FormObservationIndex(0),
                        ordinal: None,
                    }]
                } else {
                    Vec::new()
                },
            },
        )
        .unwrap()
    };
    let old = make(FormResourceAvailability::Unknown, true);
    let new = make(FormResourceAvailability::Inactive, false);
    let result = retain_ungrouped(&old, &new);
    assert!(result.is_ok());
}

#[test]
fn form_enrichment_keeps_existing_complete_and_partial_knowledge() {
    let complete =
        FormConstruction::complete(vec![
            cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#known").unwrap()
        ])
        .unwrap();
    assert_eq!(
        retain_ungrouped(&complete, &FormConstruction::default()).unwrap(),
        complete
    );
    let partial = FormConstruction::try_new(
        FormInputs::default(),
        complete.observations().to_vec(),
        FormMembership::Partial {
            cardinality: Some(2),
            order: FormOrder::Unknown,
            members: vec![FormMember {
                observation: FormObservationIndex(0),
                ordinal: Some(1),
            }],
        },
    )
    .unwrap();
    assert_eq!(
        retain_ungrouped(&partial, &FormConstruction::default()).unwrap(),
        partial
    );
}

#[test]
fn form_review_reproduces_independent_partial_members() {
    let old = FormConstruction::complete(vec![
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap()
    ])
    .unwrap();
    let new = FormConstruction::complete(vec![
        cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#b").unwrap()
    ])
    .unwrap();
    let partial = |value: FormConstruction, ordinal| {
        FormConstruction::try_new(
            FormInputs::default(),
            value.observations().to_vec(),
            FormMembership::Partial {
                cardinality: Some(3),
                order: FormOrder::Source,
                members: vec![FormMember {
                    observation: FormObservationIndex(0),
                    ordinal: Some(ordinal),
                }],
            },
        )
        .unwrap()
    };
    let result = retain_ungrouped(&partial(old, 0), &partial(new, 1)).unwrap();
    assert_eq!(
        result
            .committed_cages()
            .map(cadmpeg_ir::ids::SubdId::as_str)
            .collect::<Vec<_>>(),
        ["f3d:model:subd#a", "f3d:model:subd#b"]
    );
    assert_eq!(
        result
            .inputs()
            .position_evidence
            .iter()
            .map(|claim| claim.ordinal)
            .collect::<Vec<_>>(),
        [0, 1]
    );
}

#[test]
fn form_enrichment_retains_conflicting_positions_and_cardinalities() {
    let cage = cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap();
    let make = |count, ordinal| {
        FormConstruction::try_new(
            FormInputs::default(),
            vec![FormCageObservation {
                paths: Vec::new(),
                binding: FormCageBinding::Decoded {
                    cage: cage.clone(),
                    resource: None,
                },
            }],
            FormMembership::Partial {
                cardinality: Some(count),
                order: FormOrder::Source,
                members: vec![FormMember {
                    observation: FormObservationIndex(0),
                    ordinal: Some(ordinal),
                }],
            },
        )
        .unwrap()
    };
    let result = retain_ungrouped(&make(2, 0), &make(3, 1)).unwrap();
    assert!(
        matches!(result.membership(), FormMembership::Partial { cardinality: None, order: FormOrder::Unknown, members } if members[0].ordinal.is_none())
    );
    assert_eq!(result.committed_cages().collect::<Vec<_>>(), [&cage]);
    assert_eq!(result.inputs().cardinality_evidence, [2, 3]);
    assert_eq!(
        result
            .inputs()
            .position_evidence
            .iter()
            .map(|claim| claim.ordinal)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(retain_ungrouped(&result, &make(3, 1)).unwrap(), result);
}

#[test]
fn form_enrichment_upgrades_resource_evidence_in_both_directions() {
    let resource = |record| FormResource {
        source: cadmpeg_ir::ids::NativeId::mint(format!("f3d:scope:record#{record}")).unwrap(),
        entry: cadmpeg_core::nonblank_literal!("asset/TSplines.BlobParts/cage.tsm"),
    };
    let path = |record| FormInputPath {
        references: cadmpeg_ir::features::selections::NonEmptyMembers::one(
            cadmpeg_ir::ids::NativeId::mint(format!("f3d:scope:record#{record}")).unwrap(),
        ),
    };
    let make = |decoded| {
        FormConstruction::try_new(
            FormInputs::default(),
            vec![FormCageObservation {
                paths: vec![path(if decoded { 2 } else { 1 })],
                binding: if decoded {
                    FormCageBinding::Decoded {
                        cage: cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#a").unwrap(),
                        resource: Some(resource(2)),
                    }
                } else {
                    FormCageBinding::Undecoded {
                        resource: resource(1),
                        availability: FormResourceAvailability::ActiveUndecoded,
                    }
                },
            }],
            FormMembership::Partial {
                cardinality: Some(2),
                order: FormOrder::Source,
                members: vec![FormMember {
                    observation: FormObservationIndex(0),
                    ordinal: Some(0),
                }],
            },
        )
        .unwrap()
    };
    let a = make(false);
    let b = make(true);
    let upgraded = retain_ungrouped(&a, &b).unwrap();
    assert_eq!(retain_ungrouped(&b, &a).unwrap(), upgraded);
    assert_eq!(retain_ungrouped(&upgraded, &b).unwrap(), upgraded);
    assert_eq!(upgraded.observations().len(), 1);
    assert_eq!(upgraded.observations()[0].paths.len(), 2);
    assert_eq!(upgraded.inputs().resource_evidence.len(), 2);
    assert_eq!(
        upgraded
            .committed_cages()
            .map(cadmpeg_ir::ids::SubdId::as_str)
            .collect::<Vec<_>>(),
        ["f3d:model:subd#a"]
    );
}

#[test]
fn form_enrichment_records_conflicting_cages_for_one_resource() {
    let resource = FormResource {
        source: cadmpeg_ir::ids::NativeId::mint("f3d:scope:record#1").unwrap(),
        entry: cadmpeg_core::nonblank_literal!("asset/TSplines.BlobParts/cage.tsm"),
    };
    let make = |id: &str| {
        FormConstruction::try_new(
            FormInputs::default(),
            vec![FormCageObservation {
                paths: Vec::new(),
                binding: FormCageBinding::Decoded {
                    cage: cadmpeg_ir::ids::SubdId::mint(id).unwrap(),
                    resource: Some(resource.clone()),
                },
            }],
            FormMembership::Complete {
                members: vec![FormObservationIndex(0)],
                order: FormKnownOrder::Source,
                certainty: FormKnownOrder::Source,
            },
        )
        .unwrap()
    };
    let newer = make("f3d:model:subd#b");
    let result = retain_ungrouped(&make("f3d:model:subd#a"), &newer).unwrap();
    assert!(!result.membership_is_complete());
    assert_eq!(
        result
            .committed_cages()
            .map(cadmpeg_ir::ids::SubdId::as_str)
            .collect::<Vec<_>>(),
        ["f3d:model:subd#a", "f3d:model:subd#b"]
    );
    assert_eq!(
        result
            .observations()
            .iter()
            .filter(|observation| matches!(
                &observation.binding,
                FormCageBinding::Undecoded {
                    availability: FormResourceAvailability::Ambiguous,
                    ..
                }
            ))
            .count(),
        1
    );
    assert_eq!(
        result
            .observations()
            .iter()
            .filter(|observation| matches!(
                &observation.binding,
                FormCageBinding::Decoded { resource: None, .. }
            ))
            .count(),
        2
    );
    assert_eq!(result.inputs().resource_evidence, [resource]);
    assert_eq!(retain_ungrouped(&result, &newer).unwrap(), result);
}

#[test]
fn form_review_reproduces_duplicate_typed_serializer_identity() {
    let bytes = [
        form_serializer_frame(299, 296, FIRST_CAGE_UUID),
        form_serializer_frame(299, 296, SECOND_CAGE_UUID),
        indexed_frame(b"258", 300, 15),
    ]
    .concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let types = FormRecordTypes::of(&form_meta(&[(
        "830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571",
        0,
        &[299],
    )]));
    let serializers = crate::design::test_support::with_test_decode_context(|ctx| {
        form_typed_serializers(ctx, &bytes, &records, &types)
    })
    .unwrap();
    assert_eq!(serializers.entries.get(&296).unwrap().len(), 2);
}

#[test]
fn form_review_reproduces_repeated_owned_list_identity() {
    let mut list = indexed_frame(b"402", 210, 99);
    list[21] = 1;
    list[22..30].copy_from_slice(&201u64.to_le_bytes());
    list[32..36].copy_from_slice(&1u32.to_le_bytes());
    list[36] = 1;
    list[37..45].copy_from_slice(&211u64.to_le_bytes());
    let bytes = [list, indexed_frame(b"264", 210, 15)].concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let lists = crate::design::test_support::with_test_decode_context(|ctx| {
        form_cages::form_cage_lists(ctx, &bytes, &records, [210, 210].into_iter(), 201)
    })
    .unwrap();
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].objects, [Some(211)]);
}

#[test]
fn form_review_reproduces_full_and_partial_body_runs() {
    let mut frame = indexed_frame(b"274", 300, 11);
    push_material_envelope(&mut frame, "PrismMaterial-001");
    for target in [201, 22, 296] {
        push_marked(&mut frame, target);
    }
    frame.push(0);
    push_material_envelope(&mut frame, "PrismMaterial-002");
    for target in [201, 22, 496, 501] {
        push_marked(&mut frame, target);
    }
    frame.extend_from_slice(&indexed_frame(b"258", 300, 15));
    let records = crate::design::test_support::indexed_record_offsets_for_test(&frame);
    assert_eq!(
        crate::design::test_support::with_test_decode_context(|ctx| form_body_references(
            ctx, &frame, &records, 300
        ))
        .unwrap()
        .into_iter()
        .map(|run| run.references)
        .collect::<Vec<_>>(),
        vec![
            {
                let mut run = expected_local_run([Some(201), Some(22), Some(296), None], None);
                run[3] = Some(crate::bytes::Reference::Null);
                run
            },
            expected_local_run([Some(201), Some(22), Some(496), Some(501)], None),
        ]
    );
}

#[test]
fn form_mixed_body_runs_remain_partial_across_supported_versions() {
    for version in [9, 10, 11] {
        let mut body = indexed_frame(b"274", 300, 11);
        push_material_envelope(&mut body, "PrismMaterial-001");
        for target in [201, 22, 296] {
            push_marked(&mut body, target);
        }
        body.push(0);
        push_material_envelope(&mut body, "PrismMaterial-002");
        for target in [201, 22, 496, 501] {
            push_marked(&mut body, target);
        }
        body.extend_from_slice(&indexed_frame(b"258", 300, 15));
        let bytes = [
            form_serializer_frame(299, 296, FIRST_CAGE_UUID),
            form_serializer_frame(499, 496, SECOND_CAGE_UUID),
            body,
        ]
        .concat();
        let meta = form_meta(&[
            ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", version, &[300]),
            ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299, 499]),
            ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &[296, 496]),
            ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &[22]),
            ("702b9cd2-537c-429e-8cc4-22beeeb98c37", 1, &[501]),
        ]);
        let cages = [
            neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID),
            neutral_form_cage("f3d:model:subd#2", SECOND_CAGE_UUID),
        ];
        let result = body_construction(&bytes, &meta, &form_scope(201, vec![]), &cages);
        assert!(!result.membership_is_complete());
        assert_eq!(result.committed_cages().count(), 0);
        assert_eq!(
            result
                .referenced_cages()
                .map(cadmpeg_ir::ids::SubdId::as_str)
                .collect::<Vec<_>>(),
            ["f3d:model:subd#1", "f3d:model:subd#2"]
        );
        assert!(result.inputs().reference_evidence.iter().any(|reference| reference.source.as_str().ends_with("#300") && reference.ordinal == 2 && matches!(&reference.target, FormNativeReferenceTarget::Local { target } if target.as_str().ends_with("#296"))));
    }
}

#[test]
fn form_duplicate_serializer_identity_retains_candidates_without_certainty() {
    let cages = [
        neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID),
        neutral_form_cage("f3d:model:subd#2", SECOND_CAGE_UUID),
    ];
    for mode in 0..4 {
        let mut first = if mode == 3 {
            indexed_frame(b"377", 299, 15)
        } else {
            form_serializer_frame(299, 296, FIRST_CAGE_UUID)
        };
        if mode == 2 {
            *first.last_mut().unwrap() = 2;
        }
        let second =
            form_serializer_frame(299, if mode == 1 { 496 } else { 296 }, SECOND_CAGE_UUID);
        let body = form_body_frame(300, 0, [201, 22, 296, 301], None);
        let bytes = [first, second, body].concat();
        let meta = form_meta(&[
            ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300]),
            ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299]),
            ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &[296, 496]),
            ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &[22]),
            ("702b9cd2-537c-429e-8cc4-22beeeb98c37", 1, &[301]),
        ]);
        let result = body_construction(&bytes, &meta, &form_scope(201, vec![]), &cages);
        assert!(!result.membership_is_complete());
        assert_eq!(result.committed_cages().count(), 0);
        let expected = if mode == 3 { 1 } else { 2 };
        assert_eq!(result.referenced_cages().count(), expected);
        assert_eq!(result.inputs().resource_evidence.len(), expected);
    }
}

#[test]
fn form_partial_owned_list_and_complete_body_keep_both_members_and_conflicts() {
    use std::io::{Cursor, Write};
    let scope = form_scope(201, vec![210]);
    let bytes = [
        form_serializer_frame(499, 496, SECOND_CAGE_UUID),
        form_body_frame(300, 0, [201, 22, 496, 501], None),
    ]
    .concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let meta = form_meta(&[
        ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300]),
        ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[499]),
        ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &[496]),
        ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &[22]),
        ("702b9cd2-537c-429e-8cc4-22beeeb98c37", 1, &[501]),
    ]);
    let cages = [neutral_form_cage("f3d:model:subd#2", SECOND_CAGE_UUID)];
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, options);
    archive
        .start_file(
            format!("FusionAssetName[Active]/TSplines.BlobParts/TSpline.{SECOND_CAGE_UUID}.tsm"),
            options,
        )
        .unwrap();
    archive.write_all(b"synthetic").unwrap();
    let archive = archive.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::design::test_support::with_test_decode_context(|ctx| {
            let mut facts = form_cages::FormFacts {
                inputs: form_cages::form_inputs(&scope)?,
                observations: Vec::new(),
            };
            facts.list(&scope, 210, FormListRole::CageObjects, &[Some(211), None])?;
            facts.observe(FormCageObservation {
                paths: vec![form_cages::input_path(&scope, &[210, 211])?],
                binding: FormCageBinding::Decoded {
                    cage: cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#1").unwrap(),
                    resource: None,
                },
            });
            let ordinary = FormMembership::Partial {
                cardinality: Some(2),
                order: FormOrder::Source,
                members: vec![FormMember {
                    observation: FormObservationIndex(0),
                    ordinal: Some(0),
                }],
            };
            let source = form_cages::FormSource {
                bytes: &bytes,
                records: &records,
                scope: &scope,
                cages: &cages,
                stream: "FusionAssetName[Active]/Design1/BulkStream.dat",
            };
            let types = FormRecordTypes::of(&meta);
            let mut serializers =
                form_cages::form_serializers(ctx, &bytes, &records, Some(&types))?;
            let mut groups = form_cages::ReferenceGroups::default();
            form_cages::bind_serializers(ctx, source, &mut serializers, &mut groups)?;
            form_cages::retain_unbound_serializers(
                ctx,
                scan,
                source,
                &serializers,
                &mut facts,
                &mut Vec::new(),
            )?;
            let membership = form_cages::body_membership(
                ctx,
                scan,
                source,
                (&types, &serializers),
                (&mut facts, &mut groups),
                Some(ordinary),
                &mut Vec::new(),
            )?;
            let result =
                FormConstruction::try_new(facts.inputs, facts.observations, membership).unwrap();
            assert!(!result.membership_is_complete());
            assert_eq!(
                result
                    .committed_cages()
                    .map(cadmpeg_ir::ids::SubdId::as_str)
                    .collect::<Vec<_>>(),
                ["f3d:model:subd#1", "f3d:model:subd#2"]
            );
            assert_eq!(result.inputs().cardinality_evidence, [1, 2]);
            assert!(result
                .inputs()
                .position_evidence
                .iter()
                .any(|position| position.ordinal == 0
                    && position.source.as_ref().is_some_and(|source| source
                        .references
                        .last()
                        .unwrap()
                        .as_str()
                        .ends_with("#210"))));
            Ok::<(), cadmpeg_core::CodecError>(())
        })
    })
    .unwrap();
}

#[test]
fn form_repeated_and_distinct_lists_keep_direct_inputs_and_each_source_slot() {
    use std::io::{Cursor, Write};
    let make_list = |record: u32, object: u64| {
        let mut list = indexed_frame(b"402", record, 99);
        list[21] = 1;
        list[22..30].copy_from_slice(&201u64.to_le_bytes());
        list[32..36].copy_from_slice(&1u32.to_le_bytes());
        list[36] = 1;
        list[37..45].copy_from_slice(&object.to_le_bytes());
        [list, indexed_frame(b"264", record, 15)].concat()
    };
    let bytes = [make_list(210, 211), make_list(220, 221)].concat();
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, options);
    archive
        .start_file("FusionAssetName[Active]/Design1/BulkStream.dat", options)
        .unwrap();
    archive.write_all(&bytes).unwrap();
    let archive = archive.finish().unwrap().into_inner();
    let scope = form_scope(201, vec![210, 210, 220]);
    let mut rows: Vec<cadmpeg_ir::features::Feature> = vec![cadmpeg_core::serde_erase::json::from_value(serde_json::json!({"id":"f3d:model:feature#form","ordinal":0,"native_ref":scope.id,"definition":{"kind":"operation","value":{"kind":"native","value":{"kind":"Form","parameters":{}}}},"dependencies":[],"source_properties":{},"source_content":[],"outputs":[]})).unwrap()];
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::design::test_support::with_test_decode_context(|ctx| {
            form_cages::FormProjection::new(scan).project(
                ctx,
                std::slice::from_ref(&scope),
                &mut rows,
                &[],
                &mut Vec::new(),
            )
        })
    })
    .unwrap();
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Form(result),
    ) = rows[0].evaluation.definition()
    else {
        panic!("Form");
    };
    assert_eq!(
        result
            .inputs()
            .references
            .iter()
            .map(|id| id.as_str().rsplit('#').next().unwrap())
            .collect::<Vec<_>>(),
        ["210", "210", "220"]
    );
    assert_eq!(result.inputs().lists.len(), 2);
    assert!(
        matches!(result.membership(), FormMembership::Partial { cardinality: None, members, .. } if members.len() == 2 && members.iter().all(|member| member.ordinal.is_none()))
    );
    assert_eq!(
        result
            .inputs()
            .position_evidence
            .iter()
            .filter_map(|position| position.source.as_ref())
            .map(|source| source
                .references
                .last()
                .unwrap()
                .as_str()
                .rsplit('#')
                .next()
                .unwrap())
            .collect::<Vec<_>>(),
        ["210", "220"]
    );
}

#[test]
fn ordinary_form_list_keeps_valid_positions_around_a_malformed_slot_and_full_native_indices() {
    use std::io::{Cursor, Write};
    for malformed in [false, true] {
        let mut list = indexed_frame(b"402", 2196, 121);
        list[21] = 1;
        list[22..30].copy_from_slice(&2190u64.to_le_bytes());
        list[32..36].copy_from_slice(&3u32.to_le_bytes());
        for (ordinal, target) in [8300u64, 4_294_967_297, 8302].into_iter().enumerate() {
            let at = 36 + 11 * ordinal;
            list[at] = if malformed && ordinal == 1 { 2 } else { 1 };
            list[at + 1..at + 9].copy_from_slice(&target.to_le_bytes());
        }
        let bytes = [list, indexed_frame(b"264", 2196, 15)].concat();
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, stored);
        archive
            .start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        archive.write_all(&bytes).unwrap();
        let archive = archive.finish().unwrap().into_inner();
        let scope = form_scope(2190, vec![2196]);
        let mut rows: Vec<cadmpeg_ir::features::Feature> = vec![cadmpeg_core::serde_erase::json::from_value(serde_json::json!({"id":"f3d:model:feature#form","ordinal":0,"native_ref":scope.id,"definition":{"kind":"operation","value":{"kind":"native","value":{"kind":"Form","parameters":{}}}},"dependencies":[],"source_properties":{},"source_content":[],"outputs":[]})).unwrap()];
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            crate::design::test_support::with_test_decode_context(|ctx| {
                form_cages::FormProjection::new(scan).project(
                    ctx,
                    std::slice::from_ref(&scope),
                    &mut rows,
                    &[],
                    &mut Vec::new(),
                )
            })
        })
        .unwrap();
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Form(construction),
        ) = rows[0].evaluation.definition()
        else {
            panic!("Form");
        };
        assert!(!construction.membership_is_complete());
        let list = &construction.inputs().lists[0];
        assert_eq!(list.declared_count, 3);
        assert_eq!(
            list.members
                .iter()
                .map(|member| member.ordinal)
                .collect::<Vec<_>>(),
            if malformed { vec![0, 2] } else { vec![0, 1, 2] }
        );
        if !malformed {
            assert_eq!(
                list.members[1].reference.as_str(),
                "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-record#4294967297"
            );
        }
        let FormMembership::Partial {
            cardinality,
            members,
            ..
        } = construction.membership()
        else {
            panic!("partial");
        };
        assert_eq!(*cardinality, Some(3));
        assert_eq!(
            members
                .iter()
                .map(|member| member.ordinal)
                .collect::<Vec<_>>(),
            if malformed {
                vec![Some(0), Some(2)]
            } else {
                vec![Some(0), Some(1), Some(2)]
            }
        );
    }
}

#[test]
fn body_reference_partial_reads_retain_later_surface_and_scene_candidates() {
    let meta = form_meta(&[
        ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300]),
        ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299]),
        ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &[296]),
        ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &[22]),
        ("702b9cd2-537c-429e-8cc4-22beeeb98c37", 1, &[301]),
    ]);
    for sparse in [false, true] {
        let mut frame = indexed_frame(b"274", 300, 11);
        push_material_envelope(&mut frame, "PrismMaterial-001");
        push_marked(&mut frame, 201);
        if sparse {
            frame.push(0);
        } else {
            push_marked(&mut frame, 22);
        }
        push_marked(&mut frame, 296);
        if sparse {
            push_marked(&mut frame, 301);
        } else {
            frame.push(2);
        }
        frame.extend_from_slice(&indexed_frame(b"258", 300, 15));
        let bytes = [form_serializer_frame(299, 296, FIRST_CAGE_UUID), frame].concat();
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        assert_eq!(
            crate::design::test_support::with_test_decode_context(|ctx| form_body_references(
                ctx, &bytes, &records, 300
            ))
            .unwrap()
            .into_iter()
            .map(|run| run.references)
            .collect::<Vec<_>>(),
            vec![{
                let mut run = expected_local_run(
                    if sparse {
                        [Some(201), None, Some(296), Some(301)]
                    } else {
                        [Some(201), Some(22), Some(296), None]
                    },
                    None,
                );
                if sparse {
                    run[1] = Some(crate::bytes::Reference::Null);
                }
                run
            }]
        );
        let construction = body_construction(
            &bytes,
            &meta,
            &form_scope(201, vec![]),
            &[neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID)],
        );
        assert!(!construction.membership_is_complete());
        assert_eq!(construction.committed_cages().count(), 1);
        assert_eq!(
            construction
                .referenced_cages()
                .map(cadmpeg_ir::ids::SubdId::as_str)
                .collect::<Vec<_>>(),
            ["f3d:model:subd#1"]
        );
        let endpoint = if sparse {
            "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-record#301"
        } else {
            "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-record#22"
        };
        assert!(construction.observations()[0].paths.iter().any(|path| path
            .references
            .last()
            .unwrap()
            .as_str()
            == endpoint));
    }
}

#[test]
fn form_scope_membership_stays_separate_from_other_scopes_and_serializer_alternatives() {
    let cages = [
        neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID),
        neutral_form_cage("f3d:model:subd#2", SECOND_CAGE_UUID),
    ];
    let bytes = [
        form_serializer_frame(299, 296, FIRST_CAGE_UUID),
        form_body_frame(300, 0, [201, 22, 296, 301], None),
        form_serializer_frame(499, 496, SECOND_CAGE_UUID),
        form_body_frame(500, 0, [999, 22, 496, 501], None),
    ]
    .concat();
    for (owner, expected) in [(201, "f3d:model:subd#1"), (999, "f3d:model:subd#2")] {
        let construction = body_construction(
            &bytes,
            &two_body_form_meta(),
            &form_scope(owner, vec![]),
            &cages,
        );
        assert!(construction.membership_is_complete());
        assert_eq!(
            construction
                .committed_cages()
                .map(cadmpeg_ir::ids::SubdId::as_str)
                .collect::<Vec<_>>(),
            [expected]
        );
    }
    let bytes = [
        form_serializer_frame(299, 296, FIRST_CAGE_UUID),
        form_serializer_frame(499, 296, SECOND_CAGE_UUID),
        form_body_frame(300, 0, [201, 22, 296, 301], None),
    ]
    .concat();
    let meta = form_meta(&[
        ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300]),
        ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299, 499]),
        ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &[296]),
        ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &[22]),
        ("702b9cd2-537c-429e-8cc4-22beeeb98c37", 1, &[301]),
    ]);
    let construction = body_construction(&bytes, &meta, &form_scope(201, vec![]), &cages);
    assert!(!construction.membership_is_complete());
    assert_eq!(construction.committed_cages().count(), 0);
    assert_eq!(
        construction
            .referenced_cages()
            .map(cadmpeg_ir::ids::SubdId::as_str)
            .collect::<Vec<_>>(),
        ["f3d:model:subd#1", "f3d:model:subd#2"]
    );
}

#[test]
fn form_competing_body_runs_retain_each_real_owner_without_cross_owner_paths() {
    let bytes = [
        form_serializer_frame(299, 296, FIRST_CAGE_UUID),
        form_serializer_frame(499, 496, SECOND_CAGE_UUID),
        form_body_frame(300, 0, [201, 22, 296, 301], Some([202, 22, 496, 501])),
    ]
    .concat();
    let meta = form_meta(&[
        ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300]),
        ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299, 499]),
        ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &[296, 496]),
        ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &[22]),
        ("702b9cd2-537c-429e-8cc4-22beeeb98c37", 1, &[301, 501]),
    ]);
    let cages = [
        neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID),
        neutral_form_cage("f3d:model:subd#2", SECOND_CAGE_UUID),
    ];
    for (owner, expected_cage, surface, forbidden) in [
        (201, "f3d:model:subd#1", 296, 496),
        (202, "f3d:model:subd#2", 496, 296),
    ] {
        let scope = form_scope(owner, vec![]);
        let result = body_construction(&bytes, &meta, &scope, &cages);
        assert!(!result.membership_is_complete());
        assert_eq!(
            result
                .referenced_cages()
                .map(cadmpeg_ir::ids::SubdId::as_str)
                .collect::<Vec<_>>(),
            [expected_cage]
        );
        let expected = form_cages::input_path(
            &scope,
            &[300u64, surface, if owner == 201 { 299 } else { 499 }],
        )
        .unwrap();
        assert!(result.observations()[0].paths.contains(&expected));
        assert!(!result
            .observations()
            .iter()
            .flat_map(|observation| &observation.paths)
            .any(|path| path
                .references
                .iter()
                .any(|id| id.as_str().ends_with(&format!("#{forbidden}")))));
        let owner_targets = result
            .inputs()
            .reference_evidence
            .iter()
            .filter(|evidence| evidence.source.as_str().ends_with("#300") && evidence.ordinal == 0)
            .collect::<Vec<_>>();
        assert_eq!(owner_targets.len(), 2);
        assert!(owner_targets
            .iter()
            .all(|reference| reference.group.is_some()));
        assert_ne!(owner_targets[0].group, owner_targets[1].group);
        for (owner_ref, surface_id) in [("#201", "#296"), ("#202", "#496")] {
            let owner_group = owner_targets.iter().find(|reference| matches!(&reference.target, FormNativeReferenceTarget::Local { target } if target.as_str().ends_with(owner_ref))).unwrap().group;
            assert!(result.inputs().reference_evidence.iter().any(|reference| reference.source.as_str().ends_with("#300") && reference.ordinal == 2 && reference.group == owner_group && matches!(&reference.target, FormNativeReferenceTarget::Local { target } if target.as_str().ends_with(surface_id))));
        }
    }
}

#[test]
fn form_body_inline_types_match_registered_roles_or_remain_unresolved() {
    let types = [
        "11111111-1111-4111-8111-111111111111",
        "CD57BC48-50EC-47DC-975A-FB6DEA72F4DA",
        "22709494-9149-4A70-A829-7D748A7B5DB1",
        "702B9CD2-537C-429E-8CC4-22BEEEB98C37",
    ];
    let meta = form_meta(&[
        (types[0], 0, &[201]),
        ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300]),
        ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299]),
        (types[2], 0, &[296]),
        (types[1], 4, &[22]),
        (types[3], 1, &[301]),
    ]);
    for bad in 0..=4 {
        let mut body = indexed_frame(b"274", 300, 11);
        push_material_envelope(&mut body, "PrismMaterial-001");
        for (ordinal, target) in [201u64, 22, 296, 301].into_iter().enumerate() {
            body.push(1);
            body.extend_from_slice(&target.to_le_bytes());
            body.extend_from_slice(&36u32.to_le_bytes());
            body.extend_from_slice(if bad == ordinal {
                b"00000000-0000-4000-8000-000000000002"
            } else {
                types[ordinal].as_bytes()
            });
            body.extend_from_slice(&[0, 0]);
        }
        body.extend_from_slice(&indexed_frame(b"258", 300, 15));
        let bytes = [form_serializer_frame(299, 296, FIRST_CAGE_UUID), body].concat();
        let result = body_construction(
            &bytes,
            &meta,
            &form_scope(201, vec![]),
            &[neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID)],
        );
        assert_eq!(result.membership_is_complete(), bad == 4);
        assert_eq!(
            result
                .inputs()
                .reference_evidence
                .iter()
                .filter(|reference| reference.source.as_str().ends_with("#300"))
                .count(),
            4
        );
        if bad < 4 {
            assert_eq!(
                result.committed_cages().count(),
                usize::from(bad == 1 || bad == 3)
            );
            assert_eq!(result.referenced_cages().count(), 1);
            if bad == 0 || bad == 2 {
                assert!(result
                    .observations()
                    .iter()
                    .filter(|observation| matches!(
                        observation.binding,
                        FormCageBinding::Decoded { .. }
                    ))
                    .flat_map(|observation| &observation.paths)
                    .all(|path| !path.references[0].as_str().contains(":scope#201")));
            }
        }
    }
}

#[test]
fn form_body_foreign_references_retain_namespace_and_sibling_facts() {
    let mut segment = vec![1];
    segment.extend_from_slice(&22u64.to_le_bytes());
    segment.extend_from_slice(&[0, 1]);
    segment.extend_from_slice(&7u32.to_le_bytes());
    let mut document = vec![1];
    document.extend_from_slice(&22u64.to_le_bytes());
    document.push(1);
    document.extend_from_slice(&7u32.to_le_bytes());
    push_lp_utf16(&mut document, "11111111-2222-3333-4444-555555555555");
    document.push(0);
    crate::test_support::lp_ascii(&mut document, "CD57BC48-50EC-47DC-975A-FB6DEA72F4DA");
    push_lp_utf16(&mut document, "foreign link");
    document.push(1);
    push_lp_utf16(&mut document, "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee");
    push_lp_utf16(&mut document, "literal:version/1");
    for (is_document, foreign, owner_foreign) in [
        (false, segment.clone(), false),
        (true, document.clone(), false),
        (false, segment, true),
        (true, document, true),
    ] {
        let mut body = indexed_frame(b"274", 300, 11);
        push_material_envelope(&mut body, "PrismMaterial-001");
        if owner_foreign {
            body.extend_from_slice(&foreign);
            push_marked(&mut body, 22);
        } else {
            push_marked(&mut body, 201);
            body.extend_from_slice(&foreign);
        }
        push_marked(&mut body, 296);
        push_marked(&mut body, 301);
        body.extend_from_slice(&indexed_frame(b"258", 300, 15));
        let bytes = [form_serializer_frame(299, 296, FIRST_CAGE_UUID), body].concat();
        let result = body_construction(
            &bytes,
            &two_body_form_meta(),
            &form_scope(201, vec![]),
            &[neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID)],
        );
        assert!(!result.membership_is_complete());
        assert_eq!(result.referenced_cages().count(), 1);
        if owner_foreign {
            assert!(result
                .observations()
                .iter()
                .flat_map(|observation| &observation.paths)
                .all(|path| !path.references[0].as_str().contains(":scope#201")));
        }
        let reference = result
            .inputs()
            .reference_evidence
            .iter()
            .find(|reference| {
                reference.source.as_str().ends_with("#300")
                    && reference.ordinal == u32::from(!owner_foreign)
            })
            .unwrap();
        match &reference.target {
            FormNativeReferenceTarget::Segment {
                target, segment, ..
            } if !is_document => {
                assert_eq!((*target, *segment), (22, 7));
            }
            FormNativeReferenceTarget::Document {
                target,
                segment,
                asset_guid,
                target_type_guid,
                link_name,
                version,
            } if is_document => {
                assert_eq!((*target, *segment), (22, 7));
                assert_eq!(asset_guid, "11111111-2222-3333-4444-555555555555");
                assert_eq!(
                    target_type_guid.as_deref(),
                    Some("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA")
                );
                assert_eq!(link_name, "foreign link");
                assert_eq!(version.as_ref().unwrap().urn, "literal:version/1");
            }
            _ => panic!("foreign reference"),
        }
    }
}

#[test]
fn form_native_endpoint_width_is_independent_of_indexed_frame_lookup() {
    let wide = 4_294_967_297u64;
    let mut serializer = form_serializer_frame(299, 296, FIRST_CAGE_UUID);
    let target_at = serializer.len() - 10;
    serializer[target_at..target_at + 8].copy_from_slice(&wide.to_le_bytes());
    let mut body = indexed_frame(b"274", 300, 11);
    push_material_envelope(&mut body, "PrismMaterial-001");
    for target in [201u64, 22, wide, 301] {
        body.push(1);
        body.extend_from_slice(&target.to_le_bytes());
        body.extend_from_slice(&[0, 0]);
    }
    body.extend_from_slice(&indexed_frame(b"258", 300, 15));
    let bytes = [serializer, body].concat();
    let mut meta = two_body_form_meta();
    meta.types[0].entities = crate::records::identity::ReferenceRun::unlocated(vec![300]);
    meta.types[1].entities = crate::records::identity::ReferenceRun::unlocated(vec![299]);
    meta.types[2].entities = crate::records::identity::ReferenceRun::unlocated(vec![wide]);
    let cages = [neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID)];
    let construction = body_construction(&bytes, &meta, &form_scope(201, vec![]), &cages);
    assert!(construction.membership_is_complete());
    assert_eq!(
        construction
            .committed_cages()
            .map(cadmpeg_ir::ids::SubdId::as_str)
            .collect::<Vec<_>>(),
        ["f3d:model:subd#1"]
    );
    assert!(construction.observations()[0].paths.iter().any(|path| {
        path.references.iter().any(|reference| {
            reference.as_str()
                == "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-record#4294967297"
        })
    }));
    meta.types[0].entities = crate::records::identity::ReferenceRun::unlocated(vec![300, wide]);
    let construction = body_construction(&bytes, &meta, &form_scope(201, vec![]), &cages);
    assert!(!construction.membership_is_complete());
    assert_eq!(construction.committed_cages().count(), 0);
    assert_eq!(construction.referenced_cages().count(), 1);
}

#[test]
fn complete_owned_list_does_not_hide_conflicting_body_evidence() {
    let mut list = indexed_frame(b"402", 210, 88);
    list[21] = 1;
    list[22..30].copy_from_slice(&1400u64.to_le_bytes());
    let bytes = [
        list,
        indexed_frame(b"264", 210, 15),
        form_serializer_frame(1700, 1702, "00000000-0000-4000-8000-000000000099"),
        form_body_frame(1701, 0, [1400, 1703, 1702, 1704], None),
    ]
    .concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let scope = form_scope(1400, vec![210]);
    let cages = [neutral_form_cage(
        "f3d:model:subd#1",
        "00000000-0000-4000-8000-000000000099",
    )];
    let archive = crate::test_support::zip_test::f3d_with_form_body_resource(b"synthetic");
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::design::test_support::with_test_decode_context(|ctx| {
            let source = form_cages::FormSource {
                bytes: &bytes,
                records: &records,
                scope: &scope,
                cages: &cages,
                stream: "FusionAssetName[Active]/Design1/BulkStream.dat",
            };
            let mut facts = form_cages::FormFacts {
                inputs: form_cages::form_inputs(&scope)?,
                observations: Vec::new(),
            };
            let membership = form_cages::collect_membership(
                ctx,
                scan,
                source,
                std::slice::from_ref(&scope),
                &mut facts,
                &mut form_cages::ReferenceGroups::default(),
                &mut Vec::new(),
            )?;
            let result = FormConstruction::try_new(facts.inputs, facts.observations, membership)
                .map_err(cadmpeg_core::CodecError::malformed)?;
            assert!(!result.membership_is_complete());
            assert_eq!(result.inputs().cardinality_evidence, [0, 1]);
            assert_eq!(result.committed_cages().count(), 1);
            assert!(result.inputs().reference_evidence.iter().any(|reference| {
                reference.role == FormReferenceRole::Body
                    && reference.source.as_str().ends_with("#1701")
            }));
            Ok::<(), cadmpeg_core::CodecError>(())
        })
    })
    .unwrap();
}

#[test]
fn serializer_inline_surface_type_must_match_registered_target() {
    let plain = form_serializer_frame(299, 296, FIRST_CAGE_UUID);
    let type_at = plain.len() - 2;
    let mut serializer = plain[..type_at].to_vec();
    serializer.extend_from_slice(&36u32.to_le_bytes());
    serializer.extend_from_slice(b"22709494-9149-4A70-A829-7D748A7B5DB1");
    serializer.extend_from_slice(&[0, 0]);
    serializer.extend_from_slice(&indexed_frame(b"258", 299, 15));
    let records = crate::design::test_support::indexed_record_offsets_for_test(&serializer);
    for (target_guid, valid) in [
        ("22709494-9149-4A70-A829-7D748A7B5DB1", true),
        ("702B9CD2-537C-429E-8CC4-22BEEEB98C37", false),
    ] {
        let types = FormRecordTypes::of(&form_meta(&[
            ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299]),
            (target_guid, 0, &[296]),
        ]));
        let serializers = crate::design::test_support::with_test_decode_context(|ctx| {
            form_typed_serializers(ctx, &serializer, &records, &types)
        })
        .unwrap();
        assert_eq!(serializers.entries[&296][0].valid, valid);
        assert!(!serializers.entries[&296][0].ambiguous);
    }
}
