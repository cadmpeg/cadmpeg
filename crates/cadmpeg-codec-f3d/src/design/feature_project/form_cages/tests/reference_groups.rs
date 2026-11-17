use super::super as form_cages;
use super::*;
use std::collections::HashMap;

#[test]
fn group_json_keys_match_serde_and_return_allocation_refusals() {
    use cadmpeg_core::decode::ResourceFailure;
    let scope = form_scope(201, vec![]);
    crate::design::test_support::with_test_decode_context(|ctx| {
        let reference = crate::bytes::Reference::CrossDocument {
            target: 42,
            inline_type_guid: Some("11111111-1111-4111-8111-111111111111".to_owned()),
            segment: 3,
            asset_guid: "asset\"\\é".to_owned(),
            target_type_guid: None,
            link_name: "link\nname".to_owned(),
            version: Some(("key".to_owned(), "urn".to_owned())),
        };
        let slot = form_cages::ReferenceSlot::ungrouped(300, FormReferenceRole::Body, 0);
        let fact = form_cages::profile_reference(ctx, &scope, slot, Some(&reference), false)?;
        let value = (&fact, "serializer\"name");
        assert_eq!(form_cages::group_json_key(ctx, &value)?, serde_json::to_vec(&value).unwrap());
        let mut writer = form_cages::GroupKeyWriter::default();
        writer.reserve(usize::MAX);
        serde_json::to_writer(&mut writer, &value).unwrap();
        assert!(matches!(writer.finish(ctx), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.reason == ResourceFailure::AllocationFailed));
        Ok::<(), cadmpeg_core::CodecError>(())
    }).unwrap();
}

#[test]
fn duplicate_material_envelopes_keep_separate_null_and_unreadable_groups() {
    let meta = form_meta(&[("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300])]);
    for marker in [0u8, 2] {
        let mut body = indexed_frame(b"274", 300, 11);
        for _ in 0..2 {
            push_material_envelope(&mut body, "PrismMaterial-identical");
            if marker == 0 {
                body.extend_from_slice(&[0; 4]);
            } else {
                body.push(marker);
            }
        }
        body.extend_from_slice(&indexed_frame(b"258", 300, 15));
        let result = body_construction(&body, &meta, &form_scope(201, vec![]), &[]);
        let facts = &result.inputs().reference_evidence;
        assert_eq!(facts.len(), 8);
        let mut groups = std::collections::HashMap::new();
        for fact in facts {
            groups
                .entry(fact.group.unwrap())
                .or_insert_with(Vec::new)
                .push(fact);
        }
        assert_eq!(groups.len(), 2);
        for slots in groups.values() {
            assert_eq!(slots.len(), 4);
            for fact in slots {
                let expected = if marker == 0 {
                    FormNativeReferenceTarget::Null {}
                } else if fact.ordinal == 0 {
                    FormNativeReferenceTarget::Unreadable {}
                } else {
                    FormNativeReferenceTarget::Unvisited {}
                };
                assert_eq!(fact.target, expected);
            }
        }
        let wire = serde_json::to_string(&result).unwrap();
        let round_trip: FormConstruction = serde_json::from_str(&wire).unwrap();
        assert_eq!(round_trip, result);
    }
}

#[test]
fn body_groups_are_independent_of_padding_and_alternative_order() {
    fn body(prefix: usize, reversed: bool) -> Vec<u8> {
        let mut bytes = indexed_frame(b"274", 300, 11);
        bytes.extend(std::iter::repeat_n(0xa5, prefix));
        let mut alternatives = [[201, 22, 296, 301], [201, 22, 496, 501]];
        if reversed {
            alternatives.reverse();
        }
        for references in alternatives {
            push_material_envelope(&mut bytes, "PrismMaterial-layout");
            for target in references {
                push_marked(&mut bytes, target);
            }
        }
        bytes.extend_from_slice(&indexed_frame(b"258", 300, 15));
        bytes
    }
    let meta = form_meta(&[("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300])]);
    let scope = form_scope(201, vec![]);
    let facts = |bytes: &[u8]| {
        let result = body_construction(bytes, &meta, &scope, &[]);
        let mut inputs = result.inputs().clone();
        crate::design::test_support::with_test_decode_context(|ctx| {
            form_cages::sort_reference_evidence(ctx, &mut inputs)
        })
        .unwrap();
        inputs.reference_evidence
    };
    assert_eq!(facts(&body(0, false)), facts(&body(37, true)));
}

#[test]
fn body_group_registry_retains_origin_when_candidates_and_slot_facts_change() {
    let mut bytes = indexed_frame(b"274", 300, 11);
    for _ in 0..2 {
        push_material_envelope(&mut bytes, "PrismMaterial-identical");
        bytes.push(2);
    }
    bytes.extend_from_slice(&indexed_frame(b"258", 300, 15));
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let scope = form_scope(201, vec![]);
    let source = form_cages::FormSource {
        bytes: &bytes,
        records: &records,
        scope: &scope,
        cages: &[],
        stream: "FusionAssetName[Active]/Design1/BulkStream.dat",
    };
    crate::design::test_support::with_test_decode_context(|ctx| {
        let mut groups = form_cages::ReferenceGroups::default();
        let mut runs = form_body_references(ctx, &bytes, &records, 300)?;
        assert_eq!(runs.len(), 2);
        form_cages::bind_body_runs(ctx, source, 300, &mut runs[1..], &mut groups)?;
        let later = runs[1].group.unwrap();
        form_cages::bind_body_runs(ctx, source, 300, &mut runs, &mut groups)?;
        let earlier = runs[0].group.unwrap();
        assert_eq!(runs[1].group.unwrap().label, later.label);
        assert_ne!(earlier.label, later.label);
        let make = |facts: Vec<FormNativeReference>| {
            let inputs = FormInputs {
                reference_evidence: facts,
                ..FormInputs::default()
            };
            FormConstruction::try_new(
                inputs,
                vec![],
                FormConstruction::default().membership().clone(),
            )
            .unwrap()
        };
        let mut old_facts = form_cages::FormFacts {
            inputs: FormInputs::default(),
            observations: vec![],
        };
        for run in &runs {
            for ordinal in 0..4 {
                old_facts.missing_reference(
                    &scope,
                    run.group.unwrap().slot(ordinal),
                    ordinal != 0,
                )?;
            }
        }
        let old = make(old_facts.inputs.reference_evidence);
        let witness = form_cages::published_facts(ctx, &old)?;
        groups.active.clear();
        let target = crate::bytes::Reference::Local {
            target: 201,
            inline_type_guid: None,
        };
        runs[1].references[0] = Some(target.clone());
        form_cages::bind_body_runs(ctx, source, 300, &mut runs[1..], &mut groups)?;
        assert_eq!(runs[1].group.unwrap().label, later.label);
        let mut fresh_facts = form_cages::FormFacts {
            inputs: FormInputs::default(),
            observations: vec![],
        };
        fresh_facts.reference(
            ctx,
            &scope,
            runs[1].group.unwrap().slot(0),
            runs[1].references[0].as_ref().unwrap(),
            None,
        )?;
        let fresh = make(fresh_facts.inputs.reference_evidence);
        let retained = form_cages::retain_knowledge(ctx, &old, &fresh, &groups, Some(&witness))?;
        let facts = &retained.inputs().reference_evidence;
        assert_eq!(facts.len(), 9);
        for old_fact in &old.inputs().reference_evidence {
            assert!(facts.contains(old_fact));
        }
        assert!(facts.iter().any(|fact| fact.group == Some(later.label)
            && fact.ordinal == 0
            && matches!(fact.target, FormNativeReferenceTarget::Local { .. })));
        assert!(!facts.iter().any(|fact| fact.group == Some(earlier.label)
            && matches!(fact.target, FormNativeReferenceTarget::Local { .. })));
        let retained_witness = form_cages::published_facts(ctx, &retained)?;
        assert_eq!(
            form_cages::retain_knowledge(ctx, &retained, &fresh, &groups, Some(&retained_witness))?,
            retained
        );
        assert!(form_cages::retain_knowledge(ctx, &old, &fresh, &groups, None).is_err());
        groups.active.clear();
        assert!(form_cages::retain_knowledge(ctx, &old, &fresh, &groups, Some(&witness)).is_err());
        Ok::<(), cadmpeg_core::CodecError>(())
    })
    .unwrap();
}

#[test]
fn serializer_inventory_reuses_existing_origins_when_an_earlier_candidate_appears() {
    let earlier = form_serializer_frame(299, 296, FIRST_CAGE_UUID);
    let mut later = form_serializer_frame(299, 496, SECOND_CAGE_UUID);
    later[4..7].copy_from_slice(b"315");
    let bytes = [earlier, later, indexed_frame(b"258", 400, 15)].concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let scope = form_scope(201, vec![]);
    let source = form_cages::FormSource {
        bytes: &bytes,
        records: &records,
        scope: &scope,
        cages: &[],
        stream: "FusionAssetName[Active]/Design1/BulkStream.dat",
    };
    let mut types = FormRecordTypes::default();
    types.serializers.push(299);
    types.type_guids = vec![String::new(); 122];
    types.type_guids[121] = "830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571".into();
    crate::design::test_support::with_test_decode_context(|ctx| {
        let mut groups = form_cages::ReferenceGroups::default();
        let mut generic = form_cage_serializers(ctx, &bytes, &records)?;
        form_cages::bind_serializers(ctx, source, &mut generic, &mut groups)?;
        let later = generic.entries[&496][0].group.unwrap().label;
        let mut merged = form_cages::form_serializers(ctx, &bytes, &records, Some(&types))?;
        form_cages::bind_serializers(ctx, source, &mut merged, &mut groups)?;
        assert_eq!(merged.entries[&496].len(), 1);
        assert_eq!(merged.entries[&496][0].group.unwrap().label, later);
        assert_ne!(merged.entries[&296][0].group.unwrap().label, later);
        form_cages::bind_serializers(ctx, source, &mut merged, &mut groups)?;
        assert_eq!(groups.origins.values().map(HashMap::len).sum::<usize>(), 2);
        Ok::<(), cadmpeg_core::CodecError>(())
    })
    .unwrap();
}

#[test]
fn identical_serializer_frames_keep_distinct_groups() {
    let mut serializer = form_serializer_frame(299, 296, FIRST_CAGE_UUID);
    serializer[4..7].copy_from_slice(b"315");
    let bytes = [
        serializer.clone(),
        serializer,
        indexed_frame(b"258", 400, 15),
    ]
    .concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let scope = form_scope(201, vec![]);
    let source = form_cages::FormSource {
        bytes: &bytes,
        records: &records,
        scope: &scope,
        cages: &[],
        stream: "FusionAssetName[Active]/Design1/BulkStream.dat",
    };
    crate::design::test_support::with_test_decode_context(|ctx| {
        let mut serializers = form_cage_serializers(ctx, &bytes, &records)?;
        let mut groups = form_cages::ReferenceGroups::default();
        form_cages::bind_serializers(ctx, source, &mut serializers, &mut groups)?;
        let entries = &serializers.entries[&296];
        assert_eq!(entries.len(), 2);
        assert_ne!(
            entries[0].group.unwrap().label,
            entries[1].group.unwrap().label
        );
        Ok::<(), cadmpeg_core::CodecError>(())
    })
    .unwrap();
}

#[test]
fn serializer_groups_are_independent_of_frame_order_and_padding() {
    fn assignments(reversed: bool) -> Vec<(String, FormReferenceGroup)> {
        let mut first = form_serializer_frame(299, 296, FIRST_CAGE_UUID);
        let mut second = form_serializer_frame(299, 496, SECOND_CAGE_UUID);
        first[4..7].copy_from_slice(b"315");
        second[4..7].copy_from_slice(b"315");
        let mut frames = [first, second];
        if reversed {
            frames.reverse();
        }
        let padding = std::iter::repeat_n(0xa5, if reversed { 29 } else { 0 }).collect::<Vec<_>>();
        let bytes = [padding, frames.concat(), indexed_frame(b"258", 400, 15)].concat();
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let scope = form_scope(201, vec![]);
        let source = form_cages::FormSource {
            bytes: &bytes,
            records: &records,
            scope: &scope,
            cages: &[],
            stream: "FusionAssetName[Active]/Design1/BulkStream.dat",
        };
        crate::design::test_support::with_test_decode_context(|ctx| {
            let mut inventory = form_cage_serializers(ctx, &bytes, &records)?;
            form_cages::bind_serializers(
                ctx,
                source,
                &mut inventory,
                &mut form_cages::ReferenceGroups::default(),
            )?;
            let mut result = inventory
                .entries
                .values()
                .flatten()
                .map(|entry| (entry.name.clone(), entry.group.unwrap().label))
                .collect::<Vec<_>>();
            result.sort_by(|a, b| a.0.cmp(&b.0));
            Ok::<_, cadmpeg_core::CodecError>(result)
        })
        .unwrap()
    }
    assert_eq!(assignments(false), assignments(true));
}

#[test]
fn forms_sharing_a_body_use_the_same_group_bindings() {
    use std::io::Cursor;
    let bytes = form_body_frame(300, 0, [201, 22, 296, 301], Some([202, 22, 496, 501]));
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let types = FormRecordTypes::of(&form_meta(&[(
        "FA1F0081-C9A1-4E14-8F25-D12DD868ED90",
        10,
        &[300],
    )]));
    let scopes = [form_scope(201, vec![]), form_scope(202, vec![])];
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(
        &mut archive,
        crate::zip_write::file_options(zip::CompressionMethod::Stored),
    );
    let archive = archive.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::design::test_support::with_test_decode_context(|ctx| {
            let mut groups = form_cages::ReferenceGroups::default();
            let mut observed = Vec::new();
            for scope in &scopes {
                let source = form_cages::FormSource {
                    bytes: &bytes,
                    records: &records,
                    scope,
                    cages: &[],
                    stream: "FusionAssetName[Active]/Design1/BulkStream.dat",
                };
                let mut serializers =
                    form_cages::form_serializers(ctx, &bytes, &records, Some(&types))?;
                form_cages::bind_serializers(ctx, source, &mut serializers, &mut groups)?;
                let mut facts = form_cages::FormFacts {
                    inputs: FormInputs::default(),
                    observations: vec![],
                };
                form_cages::body_membership(
                    ctx,
                    scan,
                    source,
                    (&types, &serializers),
                    (&mut facts, &mut groups),
                    None,
                    &mut Vec::new(),
                )?;
                observed.push(facts.inputs.reference_evidence);
            }
            assert_eq!(observed[0], observed[1]);
            assert_eq!(observed[0].len(), 8);
            assert_ne!(observed[0][0].group, observed[0][4].group);
            Ok::<(), cadmpeg_core::CodecError>(())
        })
    })
    .unwrap();
}

#[test]
fn projector_rejects_unwitnessed_groups_and_does_not_publish_failed_attempts() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
    use std::io::{Cursor, Write};

    let mut frame = form_serializer_frame(299, 296, FIRST_CAGE_UUID);
    frame[4..7].copy_from_slice(b"315");
    frame.truncate(frame.len() - 11);
    frame.push(1);
    frame.extend_from_slice(&22u64.to_le_bytes());
    frame.push(1);
    frame.extend_from_slice(&7u32.to_le_bytes());
    push_lp_utf16(&mut frame, "11111111-1111-4111-8111-111111111111");
    frame.push(1);
    let bytes = [frame, indexed_frame(b"258", 400, 15)].concat();
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, options);
    archive
        .start_file("FusionAssetName[Active]/Design1/BulkStream.dat", options)
        .unwrap();
    archive.write_all(&bytes).unwrap();
    archive
        .start_file(
            format!("FusionAssetName[Active]/TSplines.BlobParts/TSpline.{FIRST_CAGE_UUID}.tsm"),
            options,
        )
        .unwrap();
    archive.write_all(b"synthetic resource").unwrap();
    let archive = archive.finish().unwrap().into_inner();
    let scope = form_scope(201, vec![]);
    let feature: Feature = serde_json::from_value(serde_json::json!({
        "id":"f3d:model:feature#form", "ordinal":0, "native_ref":scope.id,
        "definition":{"kind":"operation","value":{"kind":"native","value":{"kind":"Form","parameters":{}}}},
        "dependencies":[], "source_properties":{}, "source_content":[], "outputs":[]
    })).unwrap();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let mut projection = form_cages::FormProjection::new(scan);
        let mut rows = vec![feature.clone()];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let (refused, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            projection.project(
                &refused,
                std::slice::from_ref(&scope),
                &mut rows,
                &[],
                &mut Vec::new()
            ),
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        assert_eq!(rows, [feature.clone()]);
        assert!(projection.published.is_empty());
        assert_eq!(projection.groups.origins.values().map(HashMap::len).sum::<usize>(), 1);
        crate::design::test_support::with_test_decode_context(|ctx| {
            projection.project(
                ctx,
                std::slice::from_ref(&scope),
                &mut rows,
                &[],
                &mut Vec::new(),
            )?;
            let once = rows.clone();
            let FeatureDefinition::Operation(FeatureOperation::Form(form)) =
                rows[0].evaluation.definition()
            else {
                panic!("Form")
            };
            assert!(form
                .inputs()
                .reference_evidence
                .iter()
                .any(|fact| fact.group.is_some()));
            let mut seeded = once.clone();
            seeded[0].id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#seed").unwrap();
            let before = seeded.clone();
            assert!(projection
                .project(
                    ctx,
                    std::slice::from_ref(&scope),
                    &mut seeded,
                    &[],
                    &mut Vec::new()
                )
                .is_err());
            assert_eq!(seeded, before);
            let mut altered_inputs = form.inputs().clone();
            let fact = altered_inputs
                .reference_evidence
                .iter_mut()
                .find(|fact| fact.group.is_some())
                .unwrap();
            let FormNativeReferenceTarget::Segment { target, .. } = &mut fact.target else {
                panic!("segment")
            };
            *target = 23;
            let altered = FormConstruction::try_new(
                altered_inputs,
                form.observations().to_vec(),
                form.membership().clone(),
            )
            .unwrap();
            let mut tampered = once.clone();
            tampered[0]
                .evaluation
                .set_definition(FeatureDefinition::Operation(FeatureOperation::Form(
                    altered,
                )));
            let before = tampered.clone();
            assert!(projection
                .project(
                    ctx,
                    std::slice::from_ref(&scope),
                    &mut tampered,
                    &[],
                    &mut Vec::new()
                )
                .is_err());
            assert_eq!(tampered, before);
            let mut foreign_owner = form_cages::FormProjection::new(scan);
            assert!(foreign_owner
                .project(
                    ctx,
                    std::slice::from_ref(&scope),
                    &mut rows,
                    &[],
                    &mut Vec::new()
                )
                .is_err());
            assert_eq!(rows, once);
            projection.project(
                ctx,
                std::slice::from_ref(&scope),
                &mut rows,
                &[],
                &mut Vec::new(),
            )?;
            assert_eq!(rows, once);
            let old_witness = projection.published.get(rows[0].id.as_str()).unwrap().clone();
            let old_facts = match rows[0].evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Form(form)) => {
                    form.inputs().reference_evidence.clone()
                }
                _ => panic!("Form"),
            };
            projection.project(ctx, &[], &mut rows, &[], &mut Vec::new())?;
            assert_eq!(rows, once);
            assert_eq!(projection.published.get(rows[0].id.as_str()), Some(&old_witness));
            projection.project(ctx, std::slice::from_ref(&scope), &mut rows, &[], &mut Vec::new())?;
            assert_eq!(rows, once);
            assert_eq!(projection.published.get(rows[0].id.as_str()), Some(&old_witness));

            let failure_arena = DecodeArena::new();
            let (failure_ctx, _) = DecodeContext::from_root_bytes(&[], &failure_arena, &policy).unwrap();
            assert!(matches!(projection.project(&failure_ctx, std::slice::from_ref(&scope), &mut rows, &[], &mut Vec::new()), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.reason == cadmpeg_core::decode::ResourceFailure::BudgetExceeded));
            assert_eq!(rows, once);
            assert_eq!(projection.published.get(rows[0].id.as_str()), Some(&old_witness));
            let cage = neutral_form_cage("f3d:model:subd#first", FIRST_CAGE_UUID);
            projection.project(ctx, std::slice::from_ref(&scope), &mut rows, std::slice::from_ref(&cage), &mut Vec::new())?;
            let FeatureDefinition::Operation(FeatureOperation::Form(enriched)) = rows[0].evaluation.definition() else {
                panic!("Form")
            };
            for fact in &old_facts {
                assert!(enriched.inputs().reference_evidence.contains(fact));
            }
            assert!(enriched.observations().iter().any(|observation| matches!(&observation.binding, FormCageBinding::Decoded { cage: id, .. } if id == &cage.id)));
            assert_eq!(projection.published.get(rows[0].id.as_str()), Some(&old_witness));

            let mut first = feature.clone();
            first.id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#first").unwrap();
            projection.project(ctx, std::slice::from_ref(&scope), std::slice::from_mut(&mut first), std::slice::from_ref(&cage), &mut Vec::new())?;
            let first_witness = projection.published.get(first.id.as_str()).unwrap().clone();
            let mut missing_scope = form_scope(999, vec![205]);
            missing_scope.id = "f3d:FusionAssetName[Active]/Design2/BulkStream.dat:scope#999".to_owned();
            first.native_ref = Some(missing_scope.id.clone());
            let scopes = [scope.clone(), missing_scope];
            let previous_second = rows[0].clone();
            let mut pair = vec![first.clone(), previous_second.clone()];
            let pair_arena = DecodeArena::new();
            let (pair_ctx, _) = DecodeContext::from_root_bytes(&[], &pair_arena, &policy).unwrap();
            assert!(matches!(projection.project(&pair_ctx, &scopes, &mut pair, std::slice::from_ref(&cage), &mut Vec::new()), Err(cadmpeg_core::CodecError::ResourceLimit(_))));
            assert_ne!(pair[0], first);
            let FeatureDefinition::Operation(FeatureOperation::Form(first_form)) = pair[0].evaluation.definition() else { panic!("Form") };
            assert!(first_form.inputs().references.iter().chain(first_form.inputs().reference_runs.iter().flatten()).any(|id| id.as_str() == "f3d:FusionAssetName[Active]/Design2/BulkStream.dat:design-record#205"));
            assert_eq!(projection.published.get(pair[0].id.as_str()), Some(&first_witness));
            assert_eq!(pair[1], previous_second);
            assert_eq!(projection.published.get(pair[1].id.as_str()), Some(&old_witness));
            projection.project(ctx, &scopes, &mut pair, std::slice::from_ref(&cage), &mut Vec::new())?;
            assert_eq!(pair[1], previous_second);

            let mut second = feature.clone();
            second.id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#second").unwrap();
            rows.push(second);
            projection.project(
                ctx,
                std::slice::from_ref(&scope),
                &mut rows,
                &[],
                &mut Vec::new(),
            )?;
            let forms = rows
                .iter()
                .map(|row| match row.evaluation.definition() {
                    FeatureDefinition::Operation(FeatureOperation::Form(form)) => form,
                    _ => panic!("Form"),
                })
                .collect::<Vec<_>>();
            assert_eq!(
                forms[0].inputs().reference_evidence,
                forms[1].inputs().reference_evidence
            );
            assert!(forms[0]
                .inputs()
                .reference_evidence
                .iter()
                .any(|fact| fact.group.is_some()));
            let both = rows.clone();
            projection.project(
                ctx,
                std::slice::from_ref(&scope),
                &mut rows,
                &[],
                &mut Vec::new(),
            )?;
            assert_eq!(rows, both);
            Ok::<(), cadmpeg_core::CodecError>(())
        })
    })
    .unwrap();
}
