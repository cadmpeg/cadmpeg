// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature as owner;

#[test]
fn parses_versioned_thicken_selection_lists_exactly() {
    for (version, stamps) in [(16, &[][..]), (23, &[-1][..]), (29, &[-1, 1][..])] {
        let mut bytes = content_at(3, version, stamps);
        bytes.extend_from_slice(&references(&[0x8000_0005, 7]));
        parse(&bytes, |ctx, source| {
            let parser = owner::feature_property_parser(owner::SELECTION_LIST_TYPE)
                .expect("thicken selection parser");
            let parsed = parser(ctx, source, version).expect("selection list");
            let PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::SelectionList,
                items,
            } = parsed.kind
            else {
                panic!("thicken selection list");
            };
            assert_eq!(items.references().len(), 2);
            assert_eq!(items.references()[0].index(), 5);
            assert!(items.references()[0].qualified());
            assert_eq!(items.references()[1].index(), 7);
            assert!(!items.references()[1].qualified());
        });
        bytes.push(0);
        parse(&bytes, |ctx, source| {
            assert!(owner::parse_selection_list(ctx, source, version).is_err());
        });
        bytes.truncate(bytes.len() - 2);
        parse(&bytes, |ctx, source| {
            assert!(owner::parse_selection_list(ctx, source, version).is_err());
        });
    }
}

struct Fixture {
    source: PmDcFeature,
    properties: Vec<PmDcFeatureProperty>,
    parameters: Vec<crate::design::PmDcParameter>,
    neutral: Vec<DesignParameter>,
    inventory: owner::FeatureInventory,
}

fn fixture() -> Fixture {
    let mut payload = (*raw_parameter(20)).clone();
    payload.model_value = cadmpeg_ir::scalar::FiniteReal::new(0.2).expect("synthetic length");
    let raw = Located::new(
        payload,
        crate::record_identity::RecordTypeId::try_from(
            "264d8790d011f8d10008cabc0663dc09".to_owned(),
        )
        .expect("parameter type"),
        segment(),
        20,
    );
    let control = raw_parameter(21);
    let value = ParameterValue::Length(Length::new(2.0).expect("synthetic thickness"));
    let mut properties = vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::SelectionList,
                items: reference_list(&[30, 31]),
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 3,
            },
        ),
    ];
    for (ordinal, flag) in [
        (3, true),
        (4, false),
        (5, false),
        (6, false),
        (7, false),
        (8, true),
        (9, false),
    ] {
        properties.push(test_property(
            ordinal,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: flag,
            },
        ));
    }
    properties.push(test_property(
        10,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::ObjectCollection,
            items: reference_list(&[12, 13]),
        },
    ));
    properties.push(test_property(
        11,
        PmDcFeaturePropertyKind::SurfaceBody {
            body: reference(40),
        },
    ));
    properties.push(test_property(
        12,
        PmDcFeaturePropertyKind::SurfaceBody {
            body: reference(41),
        },
    ));
    properties.push(test_property(
        13,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FeatureDimensions,
            items: reference_list(&[]),
        },
    ));
    let neutral = vec![
        neutral_parameter(&raw, value),
        neutral_parameter(
            &control,
            ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::ZERO),
        ),
    ];
    let parameters = vec![raw, control];
    let source = test_feature(
        100,
        18,
        &[
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 4),
            (4, 5),
            (6, 20),
            (7, 6),
            (9, 13),
            (10, 7),
            (12, 21),
            (13, 22),
            (14, 23),
            (15, 10),
            (16, 8),
            (17, 9),
        ],
    );
    let inventory = owner::FeatureInventory {
        edge_names: std::collections::HashMap::new(),
        record_types: [(
            segment(),
            [
                (22, Some("4c40b0a3ac4b9504e719b28a70e0e818".to_owned())),
                (23, Some("c11c7b63ea4d840b68b76e8c67bcbc48".to_owned())),
                (29, Some("00000000000000000000000000000001".to_owned())),
                (30, Some("00000000000000000000000000000001".to_owned())),
            ]
            .into_iter()
            .collect(),
        )]
        .into_iter()
        .collect(),
        features: Vec::new(),
        pattern_features: Vec::new(),
        terminators: Vec::new(),
        properties: Vec::new(),
        labels: Vec::new(),
        entity_style_links: Vec::new(),
        kernel_entity_links: Vec::new(),
        work_planes: Vec::new(),
        issues: Vec::new(),
    };
    Fixture {
        source,
        properties,
        parameters,
        neutral,
        inventory,
    }
}

impl Fixture {
    fn project(
        &self,
        policy: DecodePolicy,
    ) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
        let index = test_projection_index(
            &self.properties,
            &self.parameters,
            &self.neutral,
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        let label = test_label(100, 4, owner::THICKEN_CLASS_ID, &[]);
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        owner::project_thicken(&ctx, &self.source, &label, &index, &self.inventory)
    }

    fn set_slot(&mut self, slot: usize, value: u32) {
        let mut payload = (*self.source).clone();
        let (marker, metadata, mut references) = payload.properties.into_parts();
        references[slot] = reference(value);
        payload.properties =
            PmDcReferenceList::new(marker, metadata, references).expect("slot count unchanged");
        self.source = Located::new(
            payload,
            self.source.identity.type_id.clone(),
            segment(),
            100,
        );
    }
}

fn generated_thicken(
    modify: impl FnOnce(&mut Vec<PmDcFeatureProperty>, &mut ParameterValue),
    policy: DecodePolicy,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let mut fixture = fixture();
    modify(
        &mut fixture.properties,
        fixture.neutral[0].value.as_mut().expect("thickness"),
    );
    if let Some(ParameterValue::Length(length)) = fixture.neutral[0].value.as_ref() {
        let mut payload = (*fixture.parameters[0]).clone();
        payload.model_value = cadmpeg_ir::scalar::FiniteReal::new(length.get() / 10.0)
            .expect("synthetic model value");
        fixture.parameters[0] = Located::new(
            payload,
            fixture.parameters[0].identity.type_id.clone(),
            segment(),
            20,
        );
    }
    fixture.project(policy)
}

#[test]
fn projects_thicken_with_native_selection_thickness_and_ordered_results() {
    let (feature, result) = generated_thicken(|_, _| {}, DecodePolicy::service())
        .expect("thicken candidate")
        .expect("thicken projection");
    let FeatureDefinition::Operation(FeatureOperation::Thicken {
        faces,
        thickness,
        side,
    }) = feature.evaluation.definition()
    else {
        panic!("thicken operation")
    };
    assert_eq!(
        faces,
        &cadmpeg_ir::features::FaceSelection::Native(
            test_property(
                1,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::SelectionList,
                    items: reference_list(&[30, 31]),
                },
            )
            .id()
        )
    );
    assert_eq!(thickness.expect("positive thickness").get(), 2.0);
    assert_eq!(*side, None);
    assert_eq!(feature.source_properties.len(), 7);
    assert_eq!(result.output_of, feature.id);
    assert_eq!(result.bodies().len(), 2);
    for (body, ordinal) in result.bodies().iter().zip([11, 12]) {
        assert_eq!(
            body.as_str(),
            test_property(
                ordinal,
                PmDcFeaturePropertyKind::SurfaceBody {
                    body: reference(40)
                }
            )
            .id()
        );
    }
    let family = owner::FeatureFamily::from_definition(feature.evaluation.definition())
        .expect("thicken family");
    assert_eq!(family.class_ids(), &[owner::THICKEN_CLASS_ID]);
    assert_eq!(family.output_slot(&super::test_feature(100, 18, &[])), 15);
}

#[test]
fn thicken_rejects_open_operands_other_controls_and_resource_refusal() {
    for case in 0..9 {
        let result = generated_thicken(
            |properties, value| match case {
                0..=2 => {
                    properties[0] = test_property(
                        1,
                        PmDcFeaturePropertyKind::References {
                            family: PmDcFeatureReferenceFamily::SelectionList,
                            items: reference_list(match case {
                                0 => &[],
                                1 => &[0],
                                _ => &[30, 30],
                            }),
                        },
                    );
                }
                3 => {
                    properties[1] = test_property(
                        2,
                        PmDcFeaturePropertyKind::Enumeration {
                            family: PmDcFeatureEnumFamily::PartOperation,
                            type_value: 5,
                            value: 2,
                        },
                    );
                }
                4 => *value = ParameterValue::Length(Length::new(0.0).expect("zero thickness")),
                5 => *value = ParameterValue::Angle(Angle::new(1.0).expect("wrong dimension")),
                6 => {
                    properties[2] = test_property(
                        3,
                        PmDcFeaturePropertyKind::Boolean {
                            name: String::new(),
                            name_value: 0,
                            value: false,
                        },
                    );
                }
                8 => {
                    *value = ParameterValue::Length(Length::new(-1.0).expect("negative thickness"));
                }
                _ => {
                    properties[9] = test_property(
                        10,
                        PmDcFeaturePropertyKind::References {
                            family: PmDcFeatureReferenceFamily::FeatureDimensions,
                            items: reference_list(&[12, 13]),
                        },
                    );
                }
            },
            DecodePolicy::service(),
        );
        assert!(result.is_none(), "case {case}");
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    assert!(matches!(
        generated_thicken(|_, _| {}, policy),
        Some(Err(CodecError::ResourceLimit(_)))
    ));
}

#[test]
fn thicken_requires_all_slot_roles_and_unique_parameter_values() {
    assert!(fixture()
        .project(DecodePolicy::service())
        .expect("candidate")
        .is_ok());
    for slot in [0, 1, 2, 3, 4, 6, 7, 9, 10, 12, 13, 14, 15, 16, 17] {
        let mut fixture = fixture();
        fixture.set_slot(slot, 0);
        assert!(
            fixture.project(DecodePolicy::service()).is_none(),
            "null slot {slot}"
        );
    }
    for slot in [5, 8, 11] {
        let mut fixture = fixture();
        fixture.set_slot(slot, 2);
        assert!(
            fixture.project(DecodePolicy::service()).is_none(),
            "non-null slot {slot}"
        );
    }
    for slot in [13, 14] {
        let mut fixture = fixture();
        fixture.set_slot(slot, if slot == 13 { 24 } else { 23 });
        assert!(
            fixture.project(DecodePolicy::service()).is_none(),
            "wrong control type {slot}"
        );
    }
    for index in [0, 1] {
        let mut fixture = fixture();
        fixture.neutral.push(fixture.neutral[index].clone());
        assert!(
            fixture.project(DecodePolicy::service()).is_none(),
            "duplicate value {index}"
        );
        fixture.neutral.last_mut().expect("duplicate").value = None;
        assert!(
            fixture.project(DecodePolicy::service()).is_none(),
            "duplicate without value {index}"
        );
    }
    for index in [0, 1] {
        let mut fixture = fixture();
        fixture.parameters.push(fixture.parameters[index].clone());
        assert!(
            fixture.project(DecodePolicy::service()).is_none(),
            "duplicate native parameter {index}"
        );
    }
    let mut fixture = fixture();
    fixture.neutral[1].value = Some(ParameterValue::Length(
        Length::new(1.0).expect("wrong control dimension"),
    ));
    assert!(fixture.project(DecodePolicy::service()).is_none());
}

#[test]
fn thicken_requires_resolved_selection_records_and_native_length_agreement() {
    let mut fixture = fixture();
    fixture
        .inventory
        .record_types
        .get_mut(&segment())
        .expect("segment")
        .remove(&29);
    assert!(fixture.project(DecodePolicy::service()).is_none());
    fixture
        .inventory
        .record_types
        .get_mut(&segment())
        .expect("segment")
        .insert(29, None);
    assert!(fixture.project(DecodePolicy::service()).is_none());
    let mut fixture = self::fixture();
    fixture.neutral[0].value = Some(ParameterValue::Length(
        Length::new(3.0).expect("different length"),
    ));
    assert!(fixture.project(DecodePolicy::service()).is_none());
}

#[test]
fn thicken_selection_record_round_trip_retains_its_family_and_qualifiers() {
    let mut bytes = content_at(3, 16, &[]);
    bytes.extend_from_slice(&references(&[0x8000_0005, 7]));
    let inventory =
        inventory_with_record(owner::SELECTION_LIST_TYPE, &bytes, DecodePolicy::service())
            .expect("selection inventory");
    assert!(inventory.issues.is_empty());
    let [property] = inventory.properties.as_slice() else {
        panic!("one selection list")
    };
    let wire = serde_json::to_value(property).expect("selection wire");
    let admitted: PmDcFeatureProperty = serde_json::from_value(wire).expect("selection admission");
    assert_eq!(&admitted, property);
}

#[test]
fn thicken_rejects_ambiguous_properties_and_wrong_dimension_list() {
    let mut fixture = fixture();
    fixture.properties.push(fixture.properties[0].clone());
    assert!(fixture.project(DecodePolicy::service()).is_none());
    let mut fixture = self::fixture();
    fixture.properties[12] = test_property(
        13,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::ObjectCollection,
            items: reference_list(&[]),
        },
    );
    assert!(fixture.project(DecodePolicy::service()).is_none());
}
