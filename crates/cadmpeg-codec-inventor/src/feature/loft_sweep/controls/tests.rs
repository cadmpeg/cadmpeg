// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::tests::{
    raw_parameter, reference, reference_list, test_feature, test_property,
};
use crate::record_identity::{Located, RecordTypeId};
use cadmpeg_ir::scalar::{Angle, FiniteReal};

struct Fixture {
    source: PmDcFeature,
    properties: Vec<PmDcFeatureProperty>,
    parameters: [PmDcParameter; 2],
    values: [ParameterValue; 2],
}

fn edited<T: Clone>(record: &Located<T>, edit: impl FnOnce(&mut T)) -> Located<T> {
    let mut payload = (**record).clone();
    edit(&mut payload);
    Located::new(
        payload,
        record.identity.type_id.clone(),
        record.identity.segment_token.clone(),
        record.identity.record_ordinal,
    )
}

impl Fixture {
    fn new() -> Self {
        let enumeration = |family, type_value, value| PmDcFeaturePropertyKind::Enumeration {
            family,
            type_value,
            value,
        };
        let boolean = PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: false,
        };
        let section = |entity| PmDcFeaturePropertyKind::LoftSection {
            entity_link: reference(entity),
            value: 0,
            condition: reference(9),
            parameters: [reference(10), reference(11)],
            map_value: 0,
            source_value: 1,
            option: reference(0),
        };
        let mut properties = vec![
            test_property(
                0,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::LoftSections,
                    items: reference_list(&[7, 8]),
                },
            ),
            test_property(1, enumeration(PmDcFeatureEnumFamily::PartOperation, 5, 1)),
            test_property(2, boolean.clone()),
            test_property(3, boolean),
            test_property(4, enumeration(PmDcFeatureEnumFamily::Auxiliary, 2, 0)),
            test_property(
                5,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::ObjectCollection,
                    items: reference_list(&[100]),
                },
            ),
            test_property(6, section(100)),
            test_property(7, section(101)),
            test_property(8, enumeration(PmDcFeatureEnumFamily::Auxiliary, 2, 0)),
        ];
        for (ordinal, type_id) in [
            (4, LOFT_MODE_TYPE),
            (6, SECTION_TYPE),
            (7, SECTION_TYPE),
            (8, SECTION_CONDITION_TYPE),
        ] {
            properties[ordinal].identity.type_id = RecordTypeId::from_bytes(type_id);
        }
        let first = raw_parameter(9);
        let second = edited(&raw_parameter(10), |p| {
            p.nominal_value = FiniteReal::new(std::f64::consts::FRAC_PI_2).expect("right angle");
            p.model_value = p.nominal_value;
        });
        Self {
            source: test_feature(20, 14, &[(0, 0), (1, 1), (2, 2), (9, 3), (10, 4), (13, 5)]),
            properties,
            parameters: [first, second],
            values: [
                ParameterValue::Real(FiniteReal::ZERO),
                ParameterValue::Angle(Angle::from_assigned_real(
                    FiniteReal::new(std::f64::consts::FRAC_PI_2).expect("right angle"),
                )),
            ],
        }
    }

    fn admitted(&self) -> Result<[PmDcReference; 2], NativeReason> {
        sections(
            &self.source,
            |r| {
                self.properties
                    .iter()
                    .find(|p| Some(p.identity.record_ordinal) == r.record_ordinal())
            },
            |r| {
                let ordinal = usize::try_from(r.record_ordinal()?).ok()?.checked_sub(9)?;
                Some((self.parameters.get(ordinal)?, self.values.get(ordinal)?))
            },
        )
    }
}

#[test]
fn positional_sections_check_both_scalar_operands_and_their_units() {
    assert_eq!(
        Fixture::new().admitted(),
        Ok([reference(100), reference(101)])
    );
    for parameter in 0..2 {
        for field in 0..4 {
            let mut f = Fixture::new();
            if field == 2 {
                f.values[parameter] = ParameterValue::Integer(0);
            } else if field == 3 {
                let value = FiniteReal::new(2.0).expect("scalar");
                f.values[parameter] = if parameter == 0 {
                    ParameterValue::Real(value)
                } else {
                    ParameterValue::Angle(Angle::from_assigned_real(value))
                };
            } else {
                f.parameters[parameter] = edited(&f.parameters[parameter], |p| {
                    if field == 0 {
                        p.nominal_value = FiniteReal::new(2.0).expect("scalar");
                    } else {
                        p.model_value = FiniteReal::new(2.0).expect("scalar");
                    }
                });
            }
            assert_eq!(f.admitted(), Err(NativeReason::SectionParameters));
        }
    }
}

#[test]
fn nonpositional_conditions_options_and_scalar_patterns_stay_native() {
    for property in [2, 3, 4, 8] {
        let mut f = Fixture::new();
        f.properties[property] = edited(&f.properties[property], |p| match &mut p.kind {
            PmDcFeaturePropertyKind::Boolean { value, .. } => *value = true,
            PmDcFeaturePropertyKind::Enumeration { value, .. } => *value = 1,
            _ => panic!("control"),
        });
        assert!(f.admitted().is_err());
    }
    for field in 0..6 {
        let mut f = Fixture::new();
        f.properties[6] = edited(&f.properties[6], |p| {
            let PmDcFeaturePropertyKind::LoftSection {
                entity_link,
                value,
                parameters,
                map_value,
                source_value,
                condition,
                ..
            } = &mut p.kind
            else {
                panic!("section");
            };
            match field {
                0 => *entity_link = reference(0),
                1 => *value = 1,
                2 => parameters[0] = reference(0),
                3 => *map_value = 1,
                4 => *source_value = 2,
                _ => *condition = reference(0),
            }
        });
        assert!(f.admitted().is_err());
    }
    let mut f = Fixture::new();
    f.properties[6] = edited(&f.properties[6], |p| {
        let PmDcFeaturePropertyKind::LoftSection { option, .. } = &mut p.kind else {
            panic!("section");
        };
        *option = reference(3);
    });
    assert!(f.admitted().is_ok());
    f.properties.push(test_property(
        11,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: true,
        },
    ));
    f.properties[6] = edited(&f.properties[6], |p| {
        let PmDcFeaturePropertyKind::LoftSection { option, .. } = &mut p.kind else {
            panic!("section");
        };
        *option = reference(12);
    });
    assert_eq!(f.admitted(), Err(NativeReason::SectionOption));
}

fn positional_definition() -> FeatureDefinition {
    use cadmpeg_ir::features::{
        BooleanOp, LoftGuidance, LoftSection, PlanarProfileRef, ProfileRef, SketchProfileRegion,
    };
    let profile = PlanarProfileRef::sketch_regions(
        cadmpeg_ir::sketches::SketchId::mint("test:model:sketch#section").expect("sketch"),
        vec![SketchProfileRegion::loops(0, Vec::new()).expect("region")],
    )
    .expect("profile");
    FeatureDefinition::Operation(FeatureOperation::Loft {
        sections: vec![
            LoftSection::Profile(ProfileRef::Planar(profile.clone())),
            LoftSection::Profile(ProfileRef::Planar(profile)),
        ],
        guidance: LoftGuidance::default(),
        op: BooleanOp::NewBody,
        closed: false,
        solid: true,
        ruled: false,
        linearize: false,
        max_degree: None,
        allow_multi_profile_faces: None,
    })
}

#[test]
fn validator_refuses_each_construction_value_outside_the_positional_form() {
    let f = Fixture::new();
    let definition = positional_definition();
    assert!(matches_definition(&definition));
    let base = serde_json::to_value(&definition).expect("definition");
    for (field, value) in [
        ("closed", serde_json::json!(true)),
        ("solid", serde_json::json!(false)),
        ("ruled", serde_json::json!(true)),
        ("linearize", serde_json::json!(true)),
        ("op", serde_json::json!("join")),
        ("max_degree", serde_json::json!(3)),
        ("allow_multi_profile_faces", serde_json::json!(true)),
    ] {
        let mut changed = base.clone();
        changed[field] = value;
        let changed = serde_json::from_value(changed).expect("changed loft");
        assert!(!matches_definition(&changed));
    }
    assert!(validates(
        &f.source,
        &definition,
        |r| f.properties.get(usize::try_from(r.record_ordinal()?).ok()?),
        |r| {
            let i = usize::try_from(r.record_ordinal()?).ok()?.checked_sub(9)?;
            Some((f.parameters.get(i)?, f.values.get(i)?))
        },
    ));
    assert!(!validates(
        &f.source,
        &definition,
        |r| f.properties.get(usize::try_from(r.record_ordinal()?).ok()?),
        |_| None,
    ));
}

#[test]
fn native_loft_refusals_pin_the_operand_result_and_scalar_losses() {
    use crate::loss::InventorLossCode;
    for reason in [
        NativeReason::Controls,
        NativeReason::Slot(9),
        NativeReason::SectionList,
        NativeReason::SectionRecord,
        NativeReason::SectionCondition,
        NativeReason::SectionOption,
    ] {
        assert_eq!(
            reason.note().code,
            InventorLossCode::FeatureLoftOperandsNative.kind()
        );
    }
    for reason in [
        NativeReason::ResultCollection,
        NativeReason::Profile,
        NativeReason::ResultIdentity,
    ] {
        assert_eq!(
            reason.note().code,
            InventorLossCode::FeatureLoftControlsUnresolved.kind()
        );
    }
    assert_eq!(
        NativeReason::ResultTopology.note().code,
        InventorLossCode::FeatureLoftResultOperandsNative.kind()
    );
    assert_eq!(
        NativeReason::SectionParameters.note().code,
        InventorLossCode::FeatureLoftSectionScalarsNative.kind()
    );
}

#[test]
fn positional_vectors_require_exact_slots_distinct_sections_and_one_result() {
    for count in [13, 15] {
        let mut f = Fixture::new();
        let slots = [(0, 0), (1, 1), (2, 2), (9, 3), (10, 4), (13, 5)];
        f.source = test_feature(
            20,
            count,
            &slots
                .into_iter()
                .filter(|(slot, _)| *slot < count)
                .collect::<Vec<_>>(),
        );
        assert_eq!(f.admitted(), Err(NativeReason::Controls));
    }
    let mut f = Fixture::new();
    f.source = test_feature(
        20,
        14,
        &[(0, 0), (1, 1), (2, 2), (7, 0), (9, 3), (10, 4), (13, 5)],
    );
    assert_eq!(f.admitted(), Err(NativeReason::Controls));
    for members in [&[7][..], &[7, 8, 7], &[7, 7]] {
        let mut f = Fixture::new();
        f.properties[0] = edited(&f.properties[0], |p| {
            p.kind = PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::LoftSections,
                items: reference_list(members),
            };
        });
        assert_eq!(f.admitted(), Err(NativeReason::SectionList));
    }
    for members in [&[][..], &[100, 101]] {
        let mut f = Fixture::new();
        f.properties[5] = edited(&f.properties[5], |p| {
            p.kind = PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(members),
            };
        });
        assert_eq!(f.admitted(), Err(NativeReason::ResultCollection));
    }
}

#[test]
fn validator_checks_vector_lengths_controls_sections_and_both_scalars() {
    let definition = positional_definition();
    let accepts = |f: &Fixture| {
        validates(
            &f.source,
            &definition,
            |r| f.properties.get(usize::try_from(r.record_ordinal()?).ok()?),
            |r| {
                let i = usize::try_from(r.record_ordinal()?).ok()?.checked_sub(9)?;
                Some((f.parameters.get(i)?, f.values.get(i)?))
            },
        )
    };
    assert!(accepts(&Fixture::new()));
    for count in [13, 15] {
        let mut f = Fixture::new();
        f.source = test_feature(20, count, &[(0, 0), (1, 1), (2, 2), (9, 3), (10, 4)]);
        assert!(!accepts(&f));
    }
    for slot in [3, 4, 5, 6, 7, 8, 11, 12] {
        let mut f = Fixture::new();
        f.source = test_feature(
            20,
            14,
            &[(0, 0), (1, 1), (2, 2), (9, 3), (10, 4), (13, 5), (slot, 0)],
        );
        assert!(!accepts(&f));
    }
    for property in [1, 2, 3, 4, 8] {
        let mut f = Fixture::new();
        f.properties[property] = edited(&f.properties[property], |p| match &mut p.kind {
            PmDcFeaturePropertyKind::Enumeration { value, .. } => *value += 1,
            PmDcFeaturePropertyKind::Boolean { value, .. } => *value = true,
            _ => panic!("control"),
        });
        assert!(!accepts(&f));
    }
    for parameter in 0..2 {
        for nominal in [false, true] {
            let mut f = Fixture::new();
            f.parameters[parameter] = edited(&f.parameters[parameter], |p| {
                let value = FiniteReal::new(2.0).expect("scalar");
                if nominal {
                    p.nominal_value = value;
                } else {
                    p.model_value = value;
                }
            });
            assert!(!accepts(&f));
        }
        let mut f = Fixture::new();
        f.values[parameter] = ParameterValue::Integer(0);
        assert!(!accepts(&f));
    }
    for members in [&[7][..], &[7, 7], &[7, 8, 7]] {
        let mut f = Fixture::new();
        f.properties[0] = edited(&f.properties[0], |p| {
            p.kind = PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::LoftSections,
                items: reference_list(members),
            };
        });
        assert!(!accepts(&f));
    }
    for members in [&[][..], &[100, 101]] {
        let mut f = Fixture::new();
        f.properties[5] = edited(&f.properties[5], |p| {
            p.kind = PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(members),
            };
        });
        assert!(!accepts(&f));
    }
}
