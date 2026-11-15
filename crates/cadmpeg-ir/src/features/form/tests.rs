use super::*;
use crate::features::FeatureOperation;

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

#[test]
fn complete_form_arrays_retain_their_v10_values() {
    for cages in [
        serde_json::json!([]),
        serde_json::json!(["test:model:subd#1", "test:model:subd#2"]),
    ] {
        let value = serde_json::json!({"definition":"form","cages":cages});
        let operation: FeatureOperation = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(operation).unwrap(), value);
    }
    let unknown =
        serde_json::to_value(FeatureOperation::Form(FormConstruction::default())).unwrap();
    assert!(unknown["cages"].is_object());
    assert_eq!(unknown["cages"]["membership"]["state"], "partial");
    assert!(unknown["cages"]["membership"].get("cardinality").is_none());
}

#[test]
fn partial_and_complete_forms_retain_inputs_candidates_and_certainty() {
    let construction = partial();
    let value = serde_json::to_value(FeatureOperation::Form(construction.clone())).unwrap();
    assert_eq!(
        serde_json::from_value::<FeatureOperation>(value).unwrap(),
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
    let value = serde_json::to_value(FeatureOperation::Form(complete.clone())).unwrap();
    assert_eq!(value["cages"], serde_json::json!(["test:model:subd#1"]));
    assert!(value["cage_inputs"].is_object());
    assert_eq!(
        serde_json::from_value::<FeatureOperation>(value.clone()).unwrap(),
        FeatureOperation::Form(complete)
    );
    let mut conflicting = value;
    conflicting["cages"] = serde_json::json!([]);
    assert!(serde_json::from_value::<FeatureOperation>(conflicting).is_err());
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
    for value in [
        r#"{"definition":"form","cages":[],"unknown":1}"#,
        r#"{"definition":"form","cages":[],"cage_inputs":null}"#,
        r#"{"definition":"form","cages":[],"inputs":{"parameters":{"width":"a","width":"b"}}}"#,
    ] {
        assert!(serde_json::from_str::<FeatureOperation>(value).is_err());
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

#[cfg(feature = "schema")]
#[test]
fn form_schema_covers_legacy_arrays_and_retained_construction() {
    let schema = serde_json::to_value(schemars::schema_for!(FormConstruction)).unwrap();
    let text = schema.to_string();
    assert!(text.contains("FormCagesWire"));
    assert!(text.contains("FormCageObservation"));
    assert!(text.contains("cage_inputs"));
    fn has_null_type(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(fields) => {
                fields.get("type").is_some_and(|kind| {
                    kind == "null"
                        || kind
                            .as_array()
                            .is_some_and(|types| types.iter().any(|kind| kind == "null"))
                }) || fields.values().any(has_null_type)
            }
            serde_json::Value::Array(values) => values.iter().any(has_null_type),
            _ => false,
        }
    }
    assert!(!has_null_type(&schema));
    assert!(!schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|field| field == "cage_inputs"));
    let condition = &schema["allOf"][0];
    assert_eq!(
        condition["then"]["properties"]["cages"]["uniqueItems"],
        true
    );
    assert_eq!(
        condition["then"]["properties"]["cage_inputs"]["properties"]["membership"]["properties"]
            ["state"]["const"],
        "complete"
    );
    assert_eq!(
        condition["else"]["properties"]["cages"]["properties"]["membership"]["properties"]["state"]
            ["const"],
        "partial"
    );
}

#[test]
fn foreign_form_reference_round_trips_and_rewrites_only_typed_local_identities() {
    let original = partial();
    let mut inputs = original.inputs().clone();
    inputs.reference_evidence = vec![
        FormNativeReference {
            source: NativeId::mint("test:stream:record#10").unwrap(),
            role: FormReferenceRole::Body,
            ordinal: 1,
            run: Some(12),
            inline_type_guid: Some("11111111-1111-4111-8111-111111111111".into()),
            target: FormNativeReferenceTarget::Local {
                target: NativeId::mint("test:stream:record#999").unwrap(),
            },
            resource: None,
        },
        FormNativeReference {
            source: NativeId::mint("test:stream:record#10").unwrap(),
            role: FormReferenceRole::Body,
            ordinal: 2,
            run: Some(12),
            inline_type_guid: None,
            target: FormNativeReferenceTarget::Segment {
                target: 22,
                segment: 7,
                asset_guid: None,
            },
            resource: None,
        },
        FormNativeReference {
            source: NativeId::mint("test:stream:record#10").unwrap(),
            role: FormReferenceRole::Body,
            ordinal: 3,
            run: Some(12),
            inline_type_guid: None,
            target: FormNativeReferenceTarget::Document {
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
            resource: Some(FormResource {
                source: NativeId::mint("test:stream:record#77").unwrap(),
                entry: cadmpeg_core::nonblank_literal!("literal/resource.tsm"),
            }),
        },
    ];
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
    let encoded = serde_json::to_value(&construction).unwrap();
    assert_eq!(
        serde_json::from_value::<FormConstruction>(encoded).unwrap(),
        construction
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    let rewritten = crate::schema::rewrite::identities(&ctx, construction, |id| {
        Ok(id.replace("test:stream:", "test:instance:"))
    })
    .unwrap();
    let evidence = &rewritten.inputs().reference_evidence;
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
        serde_json::json!(["invalid"]),
        serde_json::json!(["test:model:subd#1", "test:model:subd#1"]),
    ] {
        assert!(serde_json::from_value::<FeatureOperation>(
            serde_json::json!({"definition":"form","cages":cages})
        )
        .is_err());
    }
}

#[test]
fn form_candidate_and_native_references_require_checked_identities() {
    for value in [
        serde_json::json!({"definition":"form","cages":{"observations":[{"paths":[],"binding":{"state":"decoded","cage":"invalid"}}],"membership":{"state":"partial","order":"unknown","members":[]}}}),
        serde_json::json!({"definition":"form","cages":[],"inputs":{"references":["invalid"]}}),
    ] {
        assert!(serde_json::from_value::<FeatureOperation>(value).is_err());
    }
}

#[test]
fn complete_form_rejects_contradictory_supplemental_membership_claims() {
    let valid = serde_json::json!({
        "cages": ["test:model:subd#1"],
        "inputs": {
            "cardinality_evidence": [1],
            "position_evidence": [{"observation": 0, "ordinal": 0}]
        }
    });
    assert!(serde_json::from_value::<FormConstruction>(valid.clone()).is_ok());
    let mut wrong_count = valid.clone();
    wrong_count["inputs"]["cardinality_evidence"] = serde_json::json!([2]);
    assert!(serde_json::from_value::<FormConstruction>(wrong_count).is_err());
    let mut wrong_position = valid;
    wrong_position["inputs"]["position_evidence"][0]["ordinal"] = serde_json::json!(1);
    assert!(serde_json::from_value::<FormConstruction>(wrong_position).is_err());

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
        serde_json::json!({"cages":[],"inputs":{"cardinality_evidence":[0,0]}}),
        serde_json::json!({"cages":[],"inputs":{"resource_evidence":[{"source":"test:stream:record#1","entry":"literal"},{"source":"test:stream:record#1","entry":"literal"}]}}),
        serde_json::json!({"cages":["test:model:subd#1"],"inputs":{"position_evidence":[{"observation":0,"ordinal":0},{"observation":0,"ordinal":0}]}}),
        serde_json::json!({"cages":[],"inputs":{"reference_evidence":[{"source":"test:stream:record#1","role":"body","ordinal":0,"inline_type_guid":"not-a-guid","target":{"namespace":"local","target":"test:stream:record#2"}}]}}),
        serde_json::json!({"cages":[],"inputs":{"reference_runs":[["test:stream:record#1"],["test:stream:record#1","test:stream:record#2"]]}}),
        serde_json::json!({"cages":[],"inputs":{"reference_evidence":[{"source":"test:stream:record#1","role":"body","ordinal":0,"inline_type_guid":"11111111-1111-4111-8111-111111111111","target":{"namespace":"null"}}]}}),
    ];
    for value in bad {
        assert!(serde_json::from_value::<FormConstruction>(value).is_err());
    }
    for namespace in ["null", "unreadable", "unvisited"] {
        let value = serde_json::json!({"cages":[],"inputs":{"reference_evidence":[{"source":"test:stream:record#1","role":"body","ordinal":0,"target":{"namespace":namespace}}]}});
        let parsed: FormConstruction = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }
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
