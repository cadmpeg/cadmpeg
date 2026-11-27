use super::*;
use crate::features::FeatureOperation;
use cadmpeg_core::serde_erase::json;

fn partial() -> FormConstruction {
    let native = |key| NativeId::mint(format!("test:stream:record#{key}")).unwrap();
    FormConstruction::try_new(
        FormInputs {
            parameters: BTreeMap::from([(
                cadmpeg_core::nonblank_literal!("width"),
                "width / 2".into(),
            )]),
            references: vec![native(1), native(1)],
            lists: vec![FormNativeList {
                source: FormInputPath {
                    references: NonEmptyMembers::try_from(vec![native(1)]).unwrap(),
                },
                role: FormListRole::CageObjects,
                declared_count: 3,
                members: vec![FormNativeMember {
                    ordinal: 2,
                    reference: native(99),
                }],
            }],
            ..FormInputs::default()
        },
        vec![
            FormCageObservation {
                paths: vec![FormInputPath {
                    references: NonEmptyMembers::try_from(vec![native(1), native(99)]).unwrap(),
                }],
                binding: FormCageBinding::Decoded {
                    cage: SubdId::mint("test:model:subd#1").unwrap(),
                    resource: Some(FormResource {
                        source: native(99),
                        entry: cadmpeg_core::nonblank_literal!("asset/TSplines.BlobParts/cage.tsm"),
                    }),
                },
            },
            FormCageObservation {
                paths: Vec::new(),
                binding: FormCageBinding::Decoded {
                    cage: SubdId::mint("test:model:subd#candidate").unwrap(),
                    resource: None,
                },
            },
        ],
        FormMembership::Partial {
            cardinality: Some(3),
            order: FormOrder::Unknown,
            members: vec![FormMember {
                observation: FormObservationIndex(0),
                ordinal: Some(0),
            }],
        },
    )
    .unwrap()
}

fn no_inputs() -> serde_json::Value {
    json::to_value(FormInputs::default()).unwrap()
}

fn form_operation_wire(
    kind: &str,
    cages: serde_json::Value,
    inputs: serde_json::Value,
) -> serde_json::Value {
    let mut form = cadmpeg_core::json!({"cages": {"kind": kind}});
    form["cages"]["value"] = cages;
    form["inputs"] = inputs;
    let mut operation = cadmpeg_core::json!({"kind": "form"});
    operation["value"] = form;
    operation
}

#[test]
fn complete_form_arrays_retain_their_v10_values() {
    for cages in [
        cadmpeg_core::json!([]),
        cadmpeg_core::json!(["test:model:subd#1", "test:model:subd#2"]),
    ] {
        let value = form_operation_wire("complete", cages, no_inputs());
        let operation: FeatureOperation = json::from_value(value.clone()).unwrap();
        assert_eq!(json::to_value(operation).unwrap(), value);
    }
    let unknown = json::to_value(FeatureOperation::Form(FormConstruction::default())).unwrap();
    let cages = &unknown["value"]["cages"];
    assert_eq!(cages["kind"], "retained");
    assert_eq!(cages["value"]["membership"]["kind"], "partial");
    assert!(cages["value"]["membership"].get("cardinality").is_none());
}

#[test]
fn partial_and_complete_forms_retain_inputs_candidates_and_certainty() {
    let construction = partial();
    let value = json::to_value(FeatureOperation::Form(construction.clone())).unwrap();
    assert_eq!(
        json::from_value::<FeatureOperation>(value).unwrap(),
        FeatureOperation::Form(construction.clone())
    );
    let complete = FormConstruction::try_new(
        construction.inputs.clone(),
        construction.observations.clone(),
        FormMembership::Complete {
            members: vec![FormObservationIndex(0)],
            order: FormKnownOrder::Assumed,
            certainty: FormKnownOrder::Assumed,
        },
    )
    .unwrap();
    let value = json::to_value(FeatureOperation::Form(complete.clone())).unwrap();
    assert_eq!(
        value["value"]["cages"],
        cadmpeg_core::json!({"kind": "complete", "value": ["test:model:subd#1"]})
    );
    assert!(value["value"]["cage_inputs"].is_object());
    assert_eq!(
        json::from_value::<FeatureOperation>(value.clone()).unwrap(),
        FeatureOperation::Form(complete)
    );
    let mut conflicting = value;
    conflicting["value"]["cages"]["value"] = cadmpeg_core::json!([]);
    assert!(json::from_value::<FeatureOperation>(conflicting).is_err());
    assert_eq!(construction.referenced_cages().count(), 2);
    assert_eq!(construction.committed_cages().count(), 1);
}

#[test]
fn form_admission_rejects_invalid_members_and_native_positions() {
    let construction = partial();
    for members in [
        vec![FormMember {
            observation: FormObservationIndex(9),
            ordinal: Some(0),
        }],
        vec![FormMember {
            observation: FormObservationIndex(0),
            ordinal: Some(3),
        }],
        vec![
            FormMember {
                observation: FormObservationIndex(0),
                ordinal: Some(1),
            },
            FormMember {
                observation: FormObservationIndex(1),
                ordinal: Some(1),
            },
        ],
        vec![
            FormMember {
                observation: FormObservationIndex(0),
                ordinal: None
            };
            2
        ],
    ] {
        assert!(FormConstruction::try_new(
            construction.inputs.clone(),
            construction.observations.clone(),
            FormMembership::Partial {
                cardinality: Some(3),
                order: FormOrder::Source,
                members
            }
        )
        .is_err());
    }
    let mut inputs = construction.inputs.clone();
    let repeated = inputs.lists[0].members[0].clone();
    inputs.lists[0].members.push(repeated);
    assert!(FormConstruction::try_new(
        inputs,
        construction.observations.clone(),
        construction.membership.clone()
    )
    .is_err());
    let mut observations = construction.observations.clone();
    observations.push(observations[0].clone());
    assert!(FormConstruction::try_new(
        construction.inputs.clone(),
        observations,
        construction.membership.clone()
    )
    .is_err());
    let FormCageBinding::Decoded {
        resource: Some(resource),
        ..
    } = &construction.observations[0].binding
    else {
        panic!("resource");
    };
    let mut observations = construction.observations.clone();
    if let FormCageBinding::Decoded {
        resource: other, ..
    } = &mut observations[1].binding
    {
        *other = Some(resource.clone());
    }
    assert!(FormConstruction::try_new(
        construction.inputs.clone(),
        observations,
        construction.membership.clone()
    )
    .is_err());
    let mut observations = construction.observations.clone();
    observations.push(FormCageObservation {
        paths: Vec::new(),
        binding: FormCageBinding::Undecoded {
            resource: resource.clone(),
            availability: FormResourceAvailability::ActiveUndecoded,
        },
    });
    assert!(FormConstruction::try_new(
        construction.inputs.clone(),
        observations,
        construction.membership.clone()
    )
    .is_err());
    let mut unknown = form_operation_wire("complete", cadmpeg_core::json!([]), no_inputs());
    unknown["value"]["unknown"] = 1.into();
    let mut null_inputs = form_operation_wire("complete", cadmpeg_core::json!([]), no_inputs());
    null_inputs["value"]["cage_inputs"] = serde_json::Value::Null;
    let valid = form_operation_wire("complete", cadmpeg_core::json!([]), no_inputs()).to_string();
    let duplicate_parameters = valid.replace(
        r#""parameters":{}"#,
        r#""parameters":{"width":"a","width":"b"}"#,
    );
    assert_ne!(duplicate_parameters, valid);
    for value in [
        unknown.to_string(),
        null_inputs.to_string(),
        duplicate_parameters,
    ] {
        assert!(serde_json::from_str::<FeatureOperation>(&value).is_err());
    }
}

#[test]
fn form_clone_and_identity_rewrite_visit_candidates_and_absent_targets() {
    let construction = partial();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .unwrap();
    let operation = FeatureOperation::Form(construction.clone());
    let rewritten = crate::schema::rewrite::identities(&ctx, operation.clone(), |id| {
        Ok(id.replacen("test:", "copy:", 1))
    })
    .unwrap();
    let FeatureOperation::Form(rewritten) = rewritten else {
        panic!("Form");
    };
    assert_eq!(rewritten.inputs.parameters["width"], "width / 2");
    assert_eq!(
        rewritten.observations[0].paths[0].references[1].as_str(),
        "copy:stream:record#99"
    );
    assert_eq!(
        rewritten
            .referenced_cages()
            .map(SubdId::as_str)
            .collect::<Vec<_>>(),
        ["copy:model:subd#1", "copy:model:subd#candidate"]
    );
    let FormCageBinding::Decoded {
        resource: Some(resource),
        ..
    } = &rewritten.observations[0].binding
    else {
        panic!("resource");
    };
    assert_eq!(resource.entry.as_str(), "asset/TSplines.BlobParts/cage.tsm");
    assert_eq!(resource.source.as_str(), "copy:stream:record#99");
    assert_eq!(operation, FeatureOperation::Form(construction));
}

#[test]
fn foreign_form_reference_round_trips_and_rewrites_only_typed_local_identities() {
    let original = partial();
    let mut inputs = original.inputs().clone();
    let source = NativeId::mint("test:stream:record#10").unwrap();
    let label = FormReferenceGroup::from_label(12);
    let mut local = FormNativeReference::new(
        source.clone(),
        FormReferenceRole::Body,
        1,
        FormNativeReferenceTarget::Local {
            target: NativeId::mint("test:stream:record#999").unwrap(),
        },
    )
    .with_group(label);
    local.inline_type_guid = Some("11111111-1111-4111-8111-111111111111".into());
    let segment = FormNativeReference::new(
        source.clone(),
        FormReferenceRole::Body,
        2,
        FormNativeReferenceTarget::Segment {
            target: 22,
            segment: 7,
            asset_guid: None,
        },
    )
    .with_group(label);
    let mut document = FormNativeReference::new(
        source,
        FormReferenceRole::Body,
        3,
        FormNativeReferenceTarget::Document {
            target: 23,
            segment: 8,
            asset_guid: "literal asset".into(),
            target_type_guid: Some("22222222-2222-4222-8222-222222222222".into()),
            link_name: "test:stream:record#999".into(),
            version: Some(FormExternalVersion {
                key: "literal-key".into(),
                urn: "literal:urn".into(),
            }),
        },
    )
    .with_group(label);
    document.resource = Some(FormResource {
        source: NativeId::mint("test:stream:record#77").unwrap(),
        entry: cadmpeg_core::nonblank_literal!("literal/resource.tsm"),
    });
    inputs.reference_evidence = vec![local, segment, document];
    inputs.position_evidence = vec![FormPositionEvidence {
        source: Some(inputs.lists[0].source.clone()),
        observation: FormObservationIndex(0),
        ordinal: 2,
    }];
    inputs.cardinality_evidence = vec![2, 3];
    let membership = FormMembership::Partial {
        cardinality: None,
        order: FormOrder::Unknown,
        members: vec![FormMember {
            observation: FormObservationIndex(0),
            ordinal: None,
        }],
    };
    let construction = FormConstruction::try_new(
        inputs.clone(),
        original.observations().to_vec(),
        membership.clone(),
    )
    .unwrap();
    let encoded = json::to_value(&construction).unwrap();
    assert_eq!(
        json::from_value::<FormConstruction>(encoded).unwrap(),
        construction
    );
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let rewritten = crate::schema::rewrite::identities(&ctx, construction, |id| {
        Ok(id.replace("test:stream:", "test:instance:"))
    })
    .unwrap();
    let evidence = &rewritten.inputs().reference_evidence;
    assert!(evidence
        .iter()
        .all(|fact| fact.group == Some(FormReferenceGroup::from_label(12))));
    assert_eq!(evidence[0].source.as_str(), "test:instance:record#10");
    assert!(
        matches!(&evidence[0].target, FormNativeReferenceTarget::Local { target } if target.as_str() == "test:instance:record#999")
    );
    assert!(matches!(
        &evidence[1].target,
        FormNativeReferenceTarget::Segment {
            target: 22,
            segment: 7,
            asset_guid: None
        }
    ));
    assert!(
        matches!(&evidence[2].target, FormNativeReferenceTarget::Document { link_name, version: Some(version), .. } if link_name == "test:stream:record#999" && version.urn == "literal:urn")
    );
    assert_eq!(
        evidence[2].resource.as_ref().unwrap().source.as_str(),
        "test:instance:record#77"
    );
    assert_eq!(
        evidence[2].resource.as_ref().unwrap().entry.as_str(),
        "literal/resource.tsm"
    );
    assert_eq!(
        rewritten.inputs().position_evidence[0]
            .source
            .as_ref()
            .unwrap()
            .references[0]
            .as_str(),
        "test:instance:record#1"
    );
    let mut invalid = inputs.clone();
    invalid.reference_evidence[0].inline_type_guid = Some("not-a-guid".into());
    assert!(FormConstruction::try_new(
        invalid,
        original.observations().to_vec(),
        membership.clone()
    )
    .is_err());
    let mut invalid = inputs;
    invalid.position_evidence[0].observation = FormObservationIndex(999);
    assert!(
        FormConstruction::try_new(invalid, original.observations().to_vec(), membership).is_err()
    );
}

#[test]
fn literal_form_text_survives_native_text_replacement_and_resource_refusal() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities, TextReplacements};
    let mut construction = partial();
    let source = "test:stream:record#99";
    construction
        .inputs
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("literal"), source.into());
    if let FormCageBinding::Decoded {
        resource: Some(resource),
        ..
    } = &mut construction.observations[0].binding
    {
        resource.entry = cadmpeg_core::text::NonBlankString::new(source.to_owned()).unwrap();
    }
    let targets = BTreeMap::from([(source.to_owned(), "copy:stream:record#99".to_owned())]);
    let replacements = TextReplacements::build(targets.iter()).unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .unwrap();
    let mut map = IdentityMap::new(|id: &str| Ok(id.replacen("test:", "copy:", 1)))
        .with_text_replacements(&replacements);
    let rewritten = construction
        .clone()
        .rewrite_identities(&ctx, &mut map)
        .unwrap();
    map.finish().unwrap();
    assert_eq!(rewritten.inputs.parameters["literal"], source);
    let FormCageBinding::Decoded {
        resource: Some(resource),
        ..
    } = &rewritten.observations[0].binding
    else {
        panic!("resource");
    };
    assert_eq!(resource.entry.as_str(), source);
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::schema::rewrite::identities(&ctx, construction, |id| Ok(id.to_owned())),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}

#[test]
fn known_bindings_can_have_unknown_membership_and_inactive_resources_cannot_be_members() {
    let construction = partial();
    let candidates = FormConstruction::try_new(
        construction.inputs.clone(),
        construction.observations.clone(),
        FormMembership::Partial {
            cardinality: None,
            order: FormOrder::Unknown,
            members: Vec::new(),
        },
    )
    .unwrap();
    assert_eq!(candidates.referenced_cages().count(), 2);
    assert_eq!(candidates.committed_cages().count(), 0);
    let resource = FormResource {
        source: NativeId::mint("test:stream:record#unavailable").unwrap(),
        entry: cadmpeg_core::nonblank_literal!("asset/cage.tsm"),
    };
    for complete in [false, true] {
        let membership = if complete {
            FormMembership::Complete {
                members: vec![FormObservationIndex(0)],
                order: FormKnownOrder::Source,
                certainty: FormKnownOrder::Source,
            }
        } else {
            FormMembership::Partial {
                cardinality: Some(1),
                order: FormOrder::Source,
                members: vec![FormMember {
                    observation: FormObservationIndex(0),
                    ordinal: Some(0),
                }],
            }
        };
        assert!(FormConstruction::try_new(
            FormInputs::default(),
            vec![FormCageObservation {
                paths: Vec::new(),
                binding: FormCageBinding::Undecoded {
                    resource: resource.clone(),
                    availability: FormResourceAvailability::Inactive
                }
            }],
            membership
        )
        .is_err());
    }
    for cages in [
        cadmpeg_core::json!(["invalid"]),
        cadmpeg_core::json!(["test:model:subd#1", "test:model:subd#1"]),
    ] {
        assert!(json::from_value::<FeatureOperation>(form_operation_wire(
            "complete",
            cages,
            no_inputs()
        ))
        .is_err());
    }
}

#[test]
fn form_candidate_and_native_references_require_checked_identities() {
    let mut invalid_reference = no_inputs();
    invalid_reference["references"] = cadmpeg_core::json!(["invalid"]);
    for value in [
        form_operation_wire(
            "retained",
            cadmpeg_core::json!({"observations":[{"paths":[],"binding":{"kind":"decoded","cage":"invalid"}}],"membership":{"kind":"partial","order":"unknown","members":[]}}),
            no_inputs(),
        ),
        form_operation_wire("complete", cadmpeg_core::json!([]), invalid_reference),
    ] {
        assert!(json::from_value::<FeatureOperation>(value).is_err());
    }
}

#[test]
fn complete_form_rejects_contradictory_supplemental_membership_claims() {
    let mut valid = json::to_value(
        FormConstruction::complete(vec![SubdId::mint("test:model:subd#1").unwrap()]).unwrap(),
    )
    .unwrap();
    valid["inputs"]["cardinality_evidence"] = cadmpeg_core::json!([1]);
    valid["inputs"]["position_evidence"] = cadmpeg_core::json!([{"observation": 0, "ordinal": 0}]);
    assert!(json::from_value::<FormConstruction>(valid.clone()).is_ok());
    let mut wrong_count = valid.clone();
    wrong_count["inputs"]["cardinality_evidence"] = cadmpeg_core::json!([2]);
    assert!(json::from_value::<FormConstruction>(wrong_count).is_err());
    let mut wrong_position = valid;
    wrong_position["inputs"]["position_evidence"][0]["ordinal"] = cadmpeg_core::json!(1);
    assert!(json::from_value::<FormConstruction>(wrong_position).is_err());

    let construction = partial();
    let mut inputs = construction.inputs().clone();
    inputs.cardinality_evidence = vec![2, 3];
    inputs.position_evidence = vec![
        FormPositionEvidence {
            source: None,
            observation: FormObservationIndex(0),
            ordinal: 0,
        },
        FormPositionEvidence {
            source: None,
            observation: FormObservationIndex(0),
            ordinal: 1,
        },
    ];
    assert!(FormConstruction::try_new(
        inputs.clone(),
        construction.observations().to_vec(),
        construction.membership().clone()
    )
    .is_err());
    assert!(FormConstruction::try_new(
        inputs,
        construction.observations().to_vec(),
        FormMembership::Partial {
            cardinality: None,
            order: FormOrder::Unknown,
            members: vec![FormMember {
                observation: FormObservationIndex(0),
                ordinal: None
            }]
        }
    )
    .is_ok());
}

#[test]
fn form_wire_rejects_duplicate_evidence_and_invalid_type_claims() {
    let bad = [
        cadmpeg_core::json!({"cages":[],"inputs":{"cardinality_evidence":[0,0]}}),
        cadmpeg_core::json!({"cages":[],"inputs":{"resource_evidence":[{"source":"test:stream:record#1","entry":"literal"},{"source":"test:stream:record#1","entry":"literal"}]}}),
        cadmpeg_core::json!({"cages":["test:model:subd#1"],"inputs":{"position_evidence":[{"observation":0,"ordinal":0},{"observation":0,"ordinal":0}]}}),
        cadmpeg_core::json!({"cages":[],"inputs":{"reference_evidence":[{"source":"test:stream:record#1","role":"body","ordinal":0,"inline_type_guid":"not-a-guid","target":{"kind":"local","target":"test:stream:record#2"}}]}}),
        cadmpeg_core::json!({"cages":[],"inputs":{"reference_runs":[["test:stream:record#1"],["test:stream:record#1","test:stream:record#2"]]}}),
        cadmpeg_core::json!({"cages":[],"inputs":{"reference_evidence":[{"source":"test:stream:record#1","role":"body","ordinal":0,"inline_type_guid":"11111111-1111-4111-8111-111111111111","target":{"kind":"null"}}]}}),
    ];
    let wire = |mut value: serde_json::Value| {
        let cages = value["cages"].take();
        let mut inputs = no_inputs();
        inputs
            .as_object_mut()
            .unwrap()
            .extend(value["inputs"].as_object().unwrap().clone());
        cadmpeg_core::json!({"cages":{"kind":"complete","value":cages},"inputs":inputs})
    };
    for value in bad.into_iter().map(wire) {
        assert!(json::from_value::<FormConstruction>(value).is_err());
    }
    for namespace in ["null", "unreadable", "unvisited"] {
        let value = wire(
            cadmpeg_core::json!({"cages":[],"inputs":{"reference_evidence":[{"source":"test:stream:record#1","role":"body","ordinal":0,"target":{"kind":namespace}}]}}),
        );
        let parsed: FormConstruction = json::from_value(value.clone()).unwrap();
        assert_eq!(json::to_value(parsed).unwrap(), value);
        let mut unexpected = value;
        unexpected["inputs"]["reference_evidence"][0]["target"]["unexpected"] =
            cadmpeg_core::json!(true);
        assert!(json::from_value::<FormConstruction>(unexpected).is_err());
    }
}

#[test]
fn form_reference_group_wire_rejects_null_old_names_and_out_of_range_values() {
    let plain = cadmpeg_core::json!({
        "source": "test:stream:record#1", "role": "body", "ordinal": 0,
        "target": {"kind": "unreadable"}
    });
    let absent: FormNativeReference = json::from_value(plain.clone()).unwrap();
    assert_eq!(absent.group, None);
    assert_eq!(json::to_value(absent.clone()).unwrap(), plain);
    for value in [
        serde_json::Value::Null,
        cadmpeg_core::json!(-1),
        cadmpeg_core::json!(1.5),
    ] {
        let mut bad = plain.clone();
        bad["group"] = value;
        assert!(json::from_value::<FormNativeReference>(bad).is_err());
    }
    let too_large = r#"{"source":"test:stream:record#1","role":"body","ordinal":0,"group":18446744073709551616,"target":{"kind":"unreadable"}}"#;
    assert!(serde_json::from_str::<FormNativeReference>(too_large).is_err());
    let mut old = plain.clone();
    old["run"] = cadmpeg_core::json!(12);
    assert!(json::from_value::<FormNativeReference>(old).is_err());
    let mut maximum = plain;
    maximum["group"] = cadmpeg_core::json!(u64::MAX);
    let maximum: FormNativeReference = json::from_value(maximum).unwrap();
    assert_eq!(
        maximum.group,
        Some(FormReferenceGroup::from_label(u64::MAX))
    );

    let mut inputs = FormInputs {
        reference_evidence: vec![absent.clone(), absent.clone()],
        ..FormInputs::default()
    };
    assert!(FormConstruction::try_new(
        inputs.clone(),
        vec![],
        FormConstruction::default().membership().clone()
    )
    .is_err());
    inputs.reference_evidence[0] = absent.clone().with_group(FormReferenceGroup::from_label(1));
    inputs.reference_evidence[1] = absent.with_group(FormReferenceGroup::from_label(2));
    assert!(FormConstruction::try_new(
        inputs,
        vec![],
        FormConstruction::default().membership().clone()
    )
    .is_ok());
}
#[test]
fn complete_form_rejects_ambiguous_resource_state_evidence() {
    let resource = FormResource {
        source: NativeId::mint("test:stream:record#1").unwrap(),
        entry: cadmpeg_core::nonblank_literal!("literal/cage.tsm"),
    };
    let inputs = FormInputs {
        resource_state_evidence: vec![FormResourceStateEvidence {
            resource: resource.clone(),
            availability: FormResourceAvailability::Ambiguous,
        }],
        ..FormInputs::default()
    };
    let observations = vec![FormCageObservation {
        paths: Vec::new(),
        binding: FormCageBinding::Decoded {
            cage: SubdId::mint("test:model:subd#1").unwrap(),
            resource: Some(resource),
        },
    }];
    assert!(FormConstruction::try_new(
        inputs.clone(),
        observations.clone(),
        FormMembership::Complete {
            members: vec![FormObservationIndex(0)],
            order: FormKnownOrder::Source,
            certainty: FormKnownOrder::Source
        }
    )
    .is_err());
    assert!(FormConstruction::try_new(
        inputs,
        observations,
        FormMembership::Partial {
            cardinality: Some(1),
            order: FormOrder::Source,
            members: vec![FormMember {
                observation: FormObservationIndex(0),
                ordinal: Some(0)
            }]
        }
    )
    .is_ok());
}

#[test]
fn partial_form_rejects_contradictory_current_resource_states() {
    let resource = FormResource {
        source: NativeId::mint("test:stream:record#1").unwrap(),
        entry: cadmpeg_core::nonblank_literal!("literal/cage.tsm"),
    };
    let inputs = FormInputs {
        resource_state_evidence: vec![FormResourceStateEvidence {
            resource: resource.clone(),
            availability: FormResourceAvailability::Inactive,
        }],
        ..FormInputs::default()
    };
    let observation = |availability| FormCageObservation {
        paths: Vec::new(),
        binding: FormCageBinding::Undecoded {
            resource: resource.clone(),
            availability,
        },
    };
    let membership = FormMembership::Partial {
        cardinality: None,
        order: FormOrder::Unknown,
        members: vec![FormMember {
            observation: FormObservationIndex(0),
            ordinal: None,
        }],
    };
    for availability in [
        FormResourceAvailability::Unknown,
        FormResourceAvailability::ActiveUndecoded,
    ] {
        assert!(FormConstruction::try_new(
            inputs.clone(),
            vec![observation(availability)],
            membership.clone()
        )
        .is_err());
    }
    assert!(FormConstruction::try_new(
        inputs,
        vec![observation(FormResourceAvailability::Ambiguous)],
        membership
    )
    .is_ok());
    assert!(FormConstruction::try_new(
        FormInputs::default(),
        vec![
            observation(FormResourceAvailability::Unknown),
            observation(FormResourceAvailability::ActiveUndecoded)
        ],
        FormMembership::Partial {
            cardinality: None,
            order: FormOrder::Unknown,
            members: Vec::new()
        }
    )
    .is_err());
}

#[test]
fn membership_checks_order_and_complete_closure_evidence() {
    let complete = FormConstruction::complete(vec![
        SubdId::mint("test:model:subd#1").unwrap(),
        SubdId::mint("test:model:subd#2").unwrap(),
    ])
    .unwrap();
    let observations = complete.observations().to_vec();
    let partial = |order| FormMembership::Partial {
        cardinality: Some(2),
        order,
        members: vec![
            FormMember {
                observation: FormObservationIndex(0),
                ordinal: Some(1),
            },
            FormMember {
                observation: FormObservationIndex(1),
                ordinal: Some(0),
            },
        ],
    };
    assert!(FormConstruction::try_new(
        FormInputs::default(),
        observations.clone(),
        partial(FormOrder::Source)
    )
    .is_err());
    assert!(FormConstruction::try_new(
        FormInputs::default(),
        observations.clone(),
        partial(FormOrder::Assumed)
    )
    .is_err());
    assert!(FormConstruction::try_new(
        FormInputs::default(),
        observations.clone(),
        partial(FormOrder::Unknown)
    )
    .is_ok());
    let conflicting_positions = FormInputs {
        position_evidence: vec![
            FormPositionEvidence {
                source: None,
                observation: FormObservationIndex(0),
                ordinal: 1,
            },
            FormPositionEvidence {
                source: None,
                observation: FormObservationIndex(1),
                ordinal: 0,
            },
        ],
        ..FormInputs::default()
    };
    let no_current_positions = |order| FormMembership::Partial {
        cardinality: Some(2),
        order,
        members: vec![
            FormMember {
                observation: FormObservationIndex(0),
                ordinal: None,
            },
            FormMember {
                observation: FormObservationIndex(1),
                ordinal: None,
            },
        ],
    };
    assert!(FormConstruction::try_new(
        conflicting_positions.clone(),
        observations.clone(),
        no_current_positions(FormOrder::Source)
    )
    .is_err());
    assert!(FormConstruction::try_new(
        conflicting_positions.clone(),
        observations.clone(),
        no_current_positions(FormOrder::Unknown)
    )
    .is_ok());
    let asserted_position = FormMembership::Partial {
        cardinality: Some(2),
        order: FormOrder::Unknown,
        members: vec![FormMember {
            observation: FormObservationIndex(0),
            ordinal: Some(0),
        }],
    };
    assert!(FormConstruction::try_new(
        conflicting_positions,
        observations.clone(),
        asserted_position
    )
    .is_err());
    for (members, certainty, selected_certainty, accepted) in [
        (
            vec![0, 1],
            FormKnownOrder::Source,
            FormKnownOrder::Source,
            true,
        ),
        (
            vec![0, 1],
            FormKnownOrder::Assumed,
            FormKnownOrder::Source,
            true,
        ),
        (
            vec![0, 1],
            FormKnownOrder::Source,
            FormKnownOrder::Assumed,
            false,
        ),
        (
            vec![0, 1],
            FormKnownOrder::Assumed,
            FormKnownOrder::Assumed,
            true,
        ),
        (
            vec![0],
            FormKnownOrder::Source,
            FormKnownOrder::Source,
            false,
        ),
        (
            vec![1, 0],
            FormKnownOrder::Source,
            FormKnownOrder::Source,
            false,
        ),
    ] {
        let inputs = FormInputs {
            closure_evidence: vec![FormClosureEvidence {
                members: members.into_iter().map(FormObservationIndex).collect(),
                order: FormKnownOrder::Source,
                certainty,
            }],
            ..FormInputs::default()
        };
        let membership = FormMembership::Complete {
            members: vec![FormObservationIndex(0), FormObservationIndex(1)],
            order: FormKnownOrder::Source,
            certainty: selected_certainty,
        };
        assert_eq!(
            FormConstruction::try_new(inputs, observations.clone(), membership).is_ok(),
            accepted
        );
    }
}

#[test]
fn resource_associations_are_checked_and_round_trip() {
    let empty = FormConstruction::complete(Vec::new()).unwrap();
    let explicit_empty = json::to_value(&empty).unwrap();
    assert_eq!(
        explicit_empty["inputs"]["resource_associations"],
        cadmpeg_core::json!([])
    );
    assert_eq!(
        json::from_value::<FormConstruction>(explicit_empty.clone()).unwrap(),
        empty
    );
    let mut omitted = explicit_empty.clone();
    omitted["inputs"]
        .as_object_mut()
        .unwrap()
        .remove("resource_associations");
    assert!(json::from_value::<FormConstruction>(omitted).is_err());
    let mut null = explicit_empty;
    null["inputs"]["resource_associations"] = serde_json::Value::Null;
    assert!(json::from_value::<FormConstruction>(null).is_err());
    let cage = SubdId::mint("test:model:subd#a").unwrap();
    let other = SubdId::mint("test:model:subd#b").unwrap();
    let primary = FormResource {
        source: NativeId::mint("test:stream:record#r").unwrap(),
        entry: cadmpeg_core::nonblank_literal!("asset/r.tsm"),
    };
    let alias = FormResource {
        source: NativeId::mint("test:stream:record#t").unwrap(),
        entry: cadmpeg_core::nonblank_literal!("asset/t.tsm"),
    };
    let association = FormResourceAssociation {
        cage: cage.clone(),
        resource: alias.clone(),
    };
    let inputs = FormInputs {
        resource_associations: vec![association.clone()],
        ..FormInputs::default()
    };
    let observations = vec![FormCageObservation {
        paths: Vec::new(),
        binding: FormCageBinding::Decoded {
            cage: cage.clone(),
            resource: Some(primary.clone()),
        },
    }];
    let complete = FormMembership::Complete {
        members: vec![FormObservationIndex(0)],
        order: FormKnownOrder::Source,
        certainty: FormKnownOrder::Source,
    };
    let valid =
        FormConstruction::try_new(inputs.clone(), observations.clone(), complete.clone()).unwrap();
    let wire = json::to_value(&valid).unwrap();
    assert_eq!(
        json::from_value::<FormConstruction>(wire.clone()).unwrap(),
        valid
    );
    let mut duplicate = wire.clone();
    duplicate["inputs"]["resource_associations"]
        .as_array_mut()
        .unwrap()
        .push(json::to_value(&association).unwrap());
    assert!(json::from_value::<FormConstruction>(duplicate).is_err());
    let mut unknown_cage = wire.clone();
    unknown_cage["inputs"]["resource_associations"][0]["cage"] =
        cadmpeg_core::json!(other.as_str());
    assert!(json::from_value::<FormConstruction>(unknown_cage).is_err());
    let mut duplicate_binding_wire = wire.clone();
    duplicate_binding_wire["inputs"]["resource_associations"][0]["resource"] =
        json::to_value(&primary).unwrap();
    assert!(json::from_value::<FormConstruction>(duplicate_binding_wire).is_err());
    let mut conflicting_wire = wire;
    conflicting_wire["inputs"]["resource_state_evidence"] = cadmpeg_core::json!([{
        "resource": {"source": "test:stream:record#t", "entry": "asset/t.tsm"},
        "availability": "inactive"
    }]);
    assert!(json::from_value::<FormConstruction>(conflicting_wire).is_err());
    let mut duplicate_binding = inputs.clone();
    duplicate_binding.resource_associations[0].resource = primary;
    assert!(
        FormConstruction::try_new(duplicate_binding, observations.clone(), complete.clone())
            .is_err()
    );
    let mut conflict = inputs.clone();
    conflict
        .resource_state_evidence
        .push(FormResourceStateEvidence {
            resource: alias.clone(),
            availability: FormResourceAvailability::Inactive,
        });
    assert!(
        FormConstruction::try_new(conflict.clone(), observations.clone(), complete.clone())
            .is_err()
    );
    let partial = FormMembership::Partial {
        cardinality: Some(1),
        order: FormOrder::Source,
        members: vec![FormMember {
            observation: FormObservationIndex(0),
            ordinal: Some(0),
        }],
    };
    assert!(FormConstruction::try_new(conflict, observations.clone(), partial).is_ok());
    let mut unrelated = inputs.clone();
    unrelated
        .resource_state_evidence
        .push(FormResourceStateEvidence {
            resource: FormResource {
                source: NativeId::mint("test:stream:record#x").unwrap(),
                entry: cadmpeg_core::nonblank_literal!("asset/x.tsm"),
            },
            availability: FormResourceAvailability::Ambiguous,
        });
    assert!(FormConstruction::try_new(unrelated, observations.clone(), complete.clone()).is_ok());
    let mut shared = inputs;
    shared.resource_associations.push(FormResourceAssociation {
        cage: other.clone(),
        resource: alias,
    });
    let mut both = observations;
    both.push(FormCageObservation {
        paths: Vec::new(),
        binding: FormCageBinding::Decoded {
            cage: other,
            resource: None,
        },
    });
    assert!(FormConstruction::try_new(shared, both, complete).is_err());
}

#[test]
fn resource_association_identities_rewrite_and_entry_stays_literal() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities, TextReplacements};
    let source = "test:stream:record#t";
    let cage = SubdId::mint("test:model:subd#a").unwrap();
    let construction = FormConstruction::try_new(
        FormInputs {
            resource_associations: vec![FormResourceAssociation {
                cage: cage.clone(),
                resource: FormResource {
                    source: NativeId::mint(source).unwrap(),
                    entry: cadmpeg_core::text::NonBlankString::new(source.to_owned()).unwrap(),
                },
            }],
            ..FormInputs::default()
        },
        vec![FormCageObservation {
            paths: Vec::new(),
            binding: FormCageBinding::Decoded {
                cage,
                resource: None,
            },
        }],
        FormMembership::Complete {
            members: vec![FormObservationIndex(0)],
            order: FormKnownOrder::Source,
            certainty: FormKnownOrder::Source,
        },
    )
    .unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .unwrap();
    let mut visited = Vec::new();
    construction
        .visit_identity_references(&ctx, &mut |id| {
            visited.push(id.to_owned());
            Ok(())
        })
        .unwrap();
    assert!(!visited.contains(&source.to_owned()));
    assert_eq!(
        visited
            .iter()
            .filter(|id| id.as_str() == "test:model:subd#a")
            .count(),
        2
    );
    let targets = BTreeMap::from([(source.to_owned(), "copy:stream:record#t".to_owned())]);
    let replacements = TextReplacements::build(targets.iter()).unwrap();
    let mut map = IdentityMap::new(|id: &str| Ok(id.replacen("test:", "copy:", 1)))
        .with_text_replacements(&replacements);
    let rewritten = construction
        .clone()
        .rewrite_identities(&ctx, &mut map)
        .unwrap();
    map.finish().unwrap();
    assert_eq!(
        rewritten.inputs().resource_associations[0].cage.as_str(),
        "copy:model:subd#a"
    );
    assert_eq!(
        rewritten.inputs().resource_associations[0]
            .resource
            .source
            .as_str(),
        "copy:stream:record#t"
    );
    assert_eq!(
        rewritten.inputs().resource_associations[0]
            .resource
            .entry
            .as_str(),
        source
    );
    assert_eq!(
        rewritten.committed_cages().next().unwrap().as_str(),
        "copy:model:subd#a"
    );
    assert!(
        crate::schema::rewrite::identities(&ctx, construction, |id| {
            if id == source {
                Err(cadmpeg_core::CodecError::malformed(
                    "native identity failure",
                ))
            } else {
                Ok(id.to_owned())
            }
        })
        .is_err()
    );
}
