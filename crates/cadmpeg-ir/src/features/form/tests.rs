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
    assert!(!text.contains("\"null\""));
    assert!(!schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|field| field == "cage_inputs"));
    let condition = &schema["allOf"][0];
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
