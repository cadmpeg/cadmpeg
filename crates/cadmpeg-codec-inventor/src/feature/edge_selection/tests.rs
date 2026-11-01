// SPDX-License-Identifier: Apache-2.0
use super::*;

mod native;

fn attribute(index: usize, owner: i64, family: &str, suffix: &[i64]) -> Record {
    let mut tokens = vec![
        Token::Ref(-1),
        Token::Long(-1),
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Ref(owner),
        Token::Str(family.into()),
        Token::Long(1),
        Token::Long(44),
    ];
    tokens.extend(suffix.iter().copied().map(Token::Long));
    Record {
        index,
        name: "ATTRIB_CUSTOM-attrib".into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    }
}

fn records() -> Vec<Record> {
    vec![
        Record {
            index: 140,
            name: "edge".into(),
            tokens: vec![Token::Ref(133)].into(),
            offset: 0,
            len: 0,
        },
        attribute(133, 140, "INV_NMX_MATCHED_ATTRIB", &[1, 150, -1, 7, 0]),
        attribute(
            192,
            140,
            "INV_NMX_SWEEPGENERATED_TAG",
            &[1, 115, -1, 3, 0, 18, 0, 0, 0, 0, 0, 0, 0],
        ),
    ]
}

#[test]
fn matched_source_and_key_select_an_archived_generated_edge() {
    let names = names(
        &records(),
        &[Selector {
            source: 150,
            generator: 115,
            key: 7,
        }],
    );
    assert_eq!(
        names.get(&150),
        Some(&Some(EdgeName {
            generator: 115,
            family: "INV_NMX_SWEEPGENERATED_TAG".into(),
            associative_id: 3,
            role: 18
        }))
    );
}

#[test]
fn matching_requires_both_source_and_key_and_the_declared_generator() {
    for selector in [
        Selector {
            source: 151,
            generator: 115,
            key: 7,
        },
        Selector {
            source: 150,
            generator: 115,
            key: 8,
        },
        Selector {
            source: 150,
            generator: 116,
            key: 7,
        },
    ] {
        assert!(names(&records(), &[selector]).is_empty());
    }
}

#[test]
fn face_and_vertex_matches_do_not_select_edges() {
    let mut records = records();
    records[0].name = "vertex".into();
    assert!(names(
        &records,
        &[Selector {
            source: 150,
            generator: 115,
            key: 7
        }]
    )
    .is_empty());
}

#[test]
fn conflicting_generated_names_keep_the_selection_native() {
    let mut records = records();
    records.push(attribute(
        200,
        140,
        "INV_NMX_GEN_TAG",
        &[1, 115, -1, 8, 0, 2],
    ));
    assert!(names(
        &records,
        &[Selector {
            source: 150,
            generator: 115,
            key: 7
        }]
    )
    .is_empty());
}

#[test]
fn generic_generated_names_keep_distinct_edge_roles() {
    for role in [2, 10, 18] {
        let record = attribute(192, 140, "INV_NMX_GEN_TAG", &[1, 115, -1, 3, 0, role]);
        assert_eq!(
            generated(&record).expect("known generated role").1.role,
            role
        );
    }
}

#[test]
fn untyped_records_and_unknown_families_do_not_supply_a_join() {
    let mut record = attribute(192, 140, "INV_NMX_GEN_TAG", &[1, 115, -1, 3, 0, 2]);
    record.name = "untyped".into();
    assert!(generated(&record).is_none());
    assert!(generated(&attribute(192, 140, "other", &[1, 115, -1, 3, 0, 2])).is_none());
}

fn selector_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    for value in [0_u32, 0, 0, 0x8000_0003, 150, u32::MAX] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[1, 5]);
    for value in [0x8000_000a_u32, 2, 0, 2] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [8_u16, 0x3000] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    for value in [8_u16, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&115_u32.to_le_bytes());
    for value in [8_u16, 0x3000] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(0);
    for value in [0x8000_0060_u32, 0x8000_0004, 7] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn selector_names_its_source_index_generator_and_matching_key() {
    let bytes = selector_bytes();
    assert_eq!(
        selector(View::over_retained(&bytes), 29).expect("direct selector fixture"),
        Some(Selector {
            source: 150,
            generator: 115,
            key: 7
        })
    );
}

#[test]
fn truncated_selector_cannot_supply_a_persistent_name() {
    let bytes = selector_bytes();
    assert!(selector(View::over_retained(&bytes[..bytes.len() - 1]), 29).is_err());
}

fn feature_with_generated_group(ctx: &DecodeContext<'_>) -> Vec<cadmpeg_ir::features::Feature> {
    use cadmpeg_ir::features::{
        edge_treatments::{FilletGroup, RadiusSpec},
        FeatureDefinition, FeatureEvaluation, FeatureOperation, NonEmptyMembers,
    };
    let identity = crate::record_identity::RecordIdentity {
        segment_token: cadmpeg_ir::ids::IdentityKey::try_new("test")
            .expect("valid edge selection fixture"),
        record_ordinal: 1,
        type_id: crate::record_identity::RecordTypeId::from_bytes(super::super::FEATURE_TYPE),
    };
    let producer =
        super::super::native_feature(ctx, &identity, None, "inventor:pmdc:feature#test-1".into())
            .expect("valid edge selection fixture");
    let mut consumer = producer.clone();
    consumer.id = cadmpeg_ir::features::FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
        cadmpeg_ir::ids::IdentityKey::try_new("test-2").expect("valid edge selection fixture"),
    );
    consumer.ordinal = 1;
    consumer.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Fillet {
            groups: NonEmptyMembers::one(FilletGroup {
                edges: EdgeSelection::generated(
                    vec![
                        GeneratedEdgeRef::new(producer.id.clone(), "named-edge".into())
                            .expect("valid edge selection fixture"),
                    ],
                    "native-collection".into(),
                )
                .expect("valid edge selection fixture"),
                radius: RadiusSpec::Constant {
                    radius: cadmpeg_ir::scalar::PositiveLength::new(1.0)
                        .expect("positive radius fixture")
                        .into(),
                },
                tangency_weight: None,
            }),
        },
    ));
    vec![producer, consumer]
}

#[test]
fn generated_operands_require_a_producer_dependency() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("valid edge selection fixture");
    let mut features = feature_with_generated_group(&ctx);
    let mut results = Vec::new();
    bind_generated(&mut features, &mut results).expect("bind generated operands");
    assert_eq!(
        features[1].dependencies.as_slice(),
        [features[0].id.clone()]
    );
    assert!(results.is_empty());
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features = features;
    ir.model.feature_result_topologies = results;
    let report = cadmpeg_ir::validate::validate_neutral(&ir, Vec::new()).expect("validate fixture");
    assert!(!report.findings.iter().any(|finding| finding.check
        == cadmpeg_ir::report::check::Check::GeometricConsistency
        && finding.severity == cadmpeg_ir::report::Severity::Error));
}

#[test]
fn adding_named_edges_keeps_existing_result_members_and_source_reference() {
    use cadmpeg_core::{
        decode::{DecodeArena, DecodePolicy},
        text::NonBlankString,
    };
    use cadmpeg_ir::features::{FeatureResultMembers, FeatureResultTopology};
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("valid edge selection fixture");
    let mut features = feature_with_generated_group(&ctx);
    let mut results = vec![FeatureResultTopology::new(
        cadmpeg_ir::ids::FeatureResultTopologyId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
            features[0].id.key(),
        ),
        features[0].id.clone(),
        FeatureResultMembers::new(
            vec![NonBlankString::new("body").expect("nonblank body name")],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("valid edge selection fixture"),
        Some("source-result".into()),
    )];
    bind_generated(&mut features, &mut results).expect("bind generated operands");
    bind_generated(&mut features, &mut results).expect("bind generated operands");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].bodies()[0].as_str(), "body");
    assert_eq!(results[0].edges().len(), 1);
    assert_eq!(results[0].native_ref.as_deref(), Some("source-result"));
}

#[test]
fn absent_or_later_producers_keep_the_native_group() {
    let producer = cadmpeg_ir::features::FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
        cadmpeg_ir::ids::IdentityKey::try_new("test-1").expect("valid edge selection fixture"),
    );
    for producers in [
        HashMap::new(),
        HashMap::from([(producer.as_str().to_owned(), 2)]),
    ] {
        let mut selection = EdgeSelection::generated(
            vec![GeneratedEdgeRef::new(producer.clone(), "named-edge".into())
                .expect("valid edge selection fixture")],
            "native-collection".into(),
        )
        .expect("valid edge selection fixture");
        retain_producers(&mut selection, 1, &producers);
        assert_eq!(selection, EdgeSelection::Native("native-collection".into()));
    }
}

#[test]
fn base_flange_result_carries_the_selected_generated_edge() {
    use cadmpeg_core::{
        decode::{DecodeArena, DecodePolicy},
        text::NonBlankString,
    };
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureEvaluation, FeatureOperation, FeatureResultMembers,
        FeatureResultTopology, PlanarProfileRef, SheetMetalThicknessSide,
    };
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let mut features = feature_with_generated_group(&ctx);
    features[0].evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalBaseFlange {
            profile: PlanarProfileRef::Sketch(
                cadmpeg_ir::sketches::SketchId::mint("inventor:design:sketch#test-3")
                    .expect("sketch id"),
            ),
            thickness: cadmpeg_ir::scalar::PositiveLength::new(3.0)
                .expect("thickness")
                .into(),
            side: SheetMetalThicknessSide::Forward,
        },
    ));
    let mut results = vec![FeatureResultTopology::new(
        cadmpeg_ir::ids::FeatureResultTopologyId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
            features[0].id.key(),
        ),
        features[0].id.clone(),
        FeatureResultMembers::new(
            vec![NonBlankString::new("plate-body").expect("body")],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("members"),
        Some("plate-collection".into()),
    )];
    bind_generated(&mut features, &mut results).expect("bind edge");
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
        features[1].evaluation.definition()
    else {
        panic!("fillet")
    };
    let EdgeSelection::Generated { edges, .. } = &groups[0].edges else {
        panic!("generated edges")
    };
    assert_eq!(edges[0].feature, features[0].id);
    assert_eq!(results[0].output_of, edges[0].feature);
    assert_eq!(results[0].edges()[0].as_str(), edges[0].local_id.as_str());
    assert_eq!(results[0].bodies()[0].as_str(), "plate-body");
    assert_eq!(results[0].native_ref.as_deref(), Some("plate-collection"));
    assert_eq!(
        features[1].dependencies.as_slice(),
        [features[0].id.clone()]
    );
}

#[test]
fn history_section_wrapper_retains_the_first_edge_revision_identity() {
    let mut records = records();
    records[0].name = "End-of-ASM-History-Section".into();
    let mut tokens = vec![Token::Ident("edge".into())];
    tokens.extend(records[0].tokens.iter().cloned());
    records[0].tokens = tokens.into();
    assert!(names(
        &records,
        &[Selector {
            source: 150,
            generator: 115,
            key: 7
        }]
    )
    .get(&150)
    .is_some_and(Option::is_some));
}

#[test]
fn generated_sources_count_face_and_edge_revisions_without_selector_names() {
    let entity = |index, name: &str| Record {
        index,
        name: name.into(),
        tokens: Vec::new().into(),
        offset: 0,
        len: 0,
    };
    let records = vec![
        entity(0, "face"),
        entity(1, "edge"),
        attribute(2, 0, "INV_NMX_FLANGE_TAG", &[1, 61, -1, 7]),
        attribute(3, 1, "INV_NMX_BEND_TAG", &[1, 61, -1, 9]),
        attribute(4, 1, "INV_NMX_MATCHED_ATTRIB", &[1, 62, -1, 9, 0]),
        attribute(5, 0, "other", &[1, 63, -1, 9]),
    ];
    let (sources, complete) = generated_sources(&records, 44);
    assert!(complete);
    assert_eq!(
        sources.get(&61),
        Some(&SourceNames {
            face_revisions: 1,
            edge_revisions: 1
        })
    );
    assert!(!sources.contains_key(&62));
    assert!(!sources.contains_key(&63));
}

#[test]
fn legacy_names_preserve_every_source_pair_and_require_the_full_family_tail() {
    for (family, tail_len) in [
        ("NMx_Generated_Brep_tag", 3),
        ("NMx_Sweep_Generated_Brep_tag", 10),
        ("NMx_flange_tag", 7),
        ("NMx_blend_tag", 7),
        ("NMx_bend_tag", 8),
        ("NMx_Hole_Brep_tag", 2),
    ] {
        let mut record = Record {
            index: 2,
            name: format!("{family}-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib"),
            tokens: vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(0),
                Token::Long(38),
                Token::Long(2),
                Token::Long(61),
                Token::Long(7),
                Token::Long(63),
                Token::Long(-1),
            ]
            .into(),
            offset: 0,
            len: 0,
        };
        let mut tokens = record.tokens.to_vec();
        tokens.extend(std::iter::repeat_n(Token::Long(0), tail_len));
        record.tokens = tokens.clone().into();
        assert_eq!(
            source_names(&record, &values(&record), 38),
            Some((0, HashSet::from([61, 63])))
        );
        tokens.pop();
        record.tokens = tokens.into();
        assert!(source_names(&record, &values(&record), 38).is_none());
    }
}

#[test]
fn unknown_generated_name_forms_prevent_an_absence_proof() {
    let mut record = attribute(0, -1, "INV_NMX_FLANGE_TAG", &[1, 61, -1, 7]);
    let (_, complete) = generated_sources(std::slice::from_ref(&record), 44);
    assert!(complete);
    let unknown = attribute(0, -1, "INV_NMX_UNKNOWN", &[1, 61, -1, 7]);
    let (_, complete) = generated_sources(&[unknown], 44);
    assert!(!complete);
    record.tokens = vec![
        Token::Ref(-1),
        Token::Long(-1),
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Str("INV_NMX_FLANGE_TAG".into()),
        Token::Long(99),
    ]
    .into();
    let (_, complete) = generated_sources(&[record], 44);
    assert!(!complete);
}

#[test]
fn duplicate_qualifiers_need_a_separate_generated_name_on_the_same_owner() {
    let face = Record {
        index: 0,
        name: "face".into(),
        tokens: Vec::new().into(),
        offset: 0,
        len: 0,
    };
    let generated = attribute(1, 0, "INV_NMX_GEN_TAG", &[1, 61, -1, 7, 0, 2]);
    let duplicate = attribute(2, 0, "INV_NMX_DUP", &[31, 37, 41, 43]);
    let (sources, complete) = generated_sources(&[face.clone(), generated, duplicate.clone()], 44);
    assert!(complete);
    assert_eq!(
        sources.get(&61),
        Some(&SourceNames {
            face_revisions: 1,
            edge_revisions: 0
        })
    );
    assert!(!sources.contains_key(&31));
    let (_, complete) = generated_sources(&[face, duplicate], 44);
    assert!(!complete);
}
