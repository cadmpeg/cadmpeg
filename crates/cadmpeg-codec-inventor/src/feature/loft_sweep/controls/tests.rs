// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::tests::{
    raw_parameter, reference, reference_list, test_feature, test_property,
};
use crate::record_identity::{Located, RecordTypeId};
use cadmpeg_core::serde_erase::json;
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
    use cadmpeg_ir::features::operation_settings::BooleanOp;
    use cadmpeg_ir::features::profiles::{
        LoftSection, PlanarProfileRef, ProfileRef, SketchProfileRegion,
    };
    use cadmpeg_ir::features::LoftGuidance;
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
    let base = json::to_value(&definition).expect("definition");
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
        changed["value"]["value"][field] = value;
        let changed = json::from_value(changed).expect("changed loft");
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
    for reason in [
        NativeReason::ResultTopology,
        NativeReason::ResultStateFailed,
        NativeReason::ResultStateBounded,
    ] {
        assert_eq!(
            reason.note().code,
            InventorLossCode::FeatureLoftResultOperandsNative.kind()
        );
    }
    for (reason, detail) in [
        (
            NativeReason::ResultStateFailed,
            "history state of the loft does not decode",
        ),
        (
            NativeReason::ResultStateBounded,
            "history state of the loft passed the local work cap of state views",
        ),
    ] {
        assert_eq!(
            reason.note().message,
            format!("Retained Inventor solid loft natively: {detail}.")
        );
    }
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

/// A surface loft fixture: slot 6 holds the result wrapper, slot 13 is null,
/// the first section is the face loop (its `source_value` is its own source
/// index), and the second section selects a sketch wire.
fn surface_fixture() -> Fixture {
    let mut f = Fixture::new();
    f.source = test_feature(20, 14, &[(0, 0), (1, 1), (2, 2), (9, 3), (10, 4), (6, 5)]);
    f.properties[1] = edited(&f.properties[1], |p| {
        let PmDcFeaturePropertyKind::Enumeration { value, .. } = &mut p.kind else {
            panic!("part operation");
        };
        *value = 5;
    });
    f.properties[5] = edited(&f.properties[5], |p| {
        p.kind = PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) };
    });
    f.properties[6] = edited(&f.properties[6], |p| {
        p.header.source_index = 7;
        let PmDcFeaturePropertyKind::LoftSection { source_value, .. } = &mut p.kind else {
            panic!("section");
        };
        *source_value = 7;
    });
    f
}

impl Fixture {
    fn admitted_surface(&self) -> Result<SurfaceSections, NativeReason> {
        surface_sections(
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
fn surface_loft_admits_one_sketch_section_and_one_face_loop_in_either_order() {
    let f = surface_fixture();
    let admitted = f.admitted_surface().expect("surface loft");
    assert!(!admitted.sketch_first);
    assert_eq!(admitted.face_loop.entity_link, reference(100));
    assert_eq!(admitted.sketch.entity_link, reference(101));
    assert_eq!(admitted.face_loop.record, reference(7));
    let mut swapped = surface_fixture();
    swapped.properties[0] = edited(&swapped.properties[0], |p| {
        p.kind = PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::LoftSections,
            items: reference_list(&[8, 7]),
        };
    });
    assert!(swapped.admitted_surface().expect("swapped").sketch_first);
}

#[test]
fn surface_loft_refuses_forms_outside_the_positional_surface_vector() {
    // The solid form is not a surface loft: slot 1 holds value 1 and slot 13
    // holds the result collection.
    assert!(Fixture::new().admitted_surface().is_err());
    // Two sketch sections, or two face loops, are not the stored form.
    let mut both_loops = surface_fixture();
    both_loops.properties[7] = edited(&both_loops.properties[7], |p| {
        p.header.source_index = 8;
        let PmDcFeaturePropertyKind::LoftSection { source_value, .. } = &mut p.kind else {
            panic!("section");
        };
        *source_value = 8;
    });
    assert_eq!(
        both_loops.admitted_surface(),
        Err(NativeReason::SectionRecord)
    );
    let mut both_sketch = surface_fixture();
    both_sketch.properties[6] = edited(&both_sketch.properties[6], |p| {
        let PmDcFeaturePropertyKind::LoftSection { source_value, .. } = &mut p.kind else {
            panic!("section");
        };
        *source_value = 1;
    });
    assert_eq!(
        both_sketch.admitted_surface(),
        Err(NativeReason::SectionRecord)
    );
    // The result wrapper must be a surface body, and slot 13 stays null.
    let mut no_body = surface_fixture();
    no_body.properties[5] = edited(&no_body.properties[5], |p| {
        p.kind = PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: false,
        };
    });
    assert_eq!(
        no_body.admitted_surface(),
        Err(NativeReason::ResultCollection)
    );
    let mut solid_slot = surface_fixture();
    solid_slot.source = test_feature(
        20,
        14,
        &[(0, 0), (1, 1), (2, 2), (6, 5), (9, 3), (10, 4), (13, 5)],
    );
    assert_eq!(solid_slot.admitted_surface(), Err(NativeReason::Controls));
}

#[test]
fn surface_definition_is_the_unguided_new_body_sheet_loft() {
    let solid = positional_definition();
    assert!(!matches_surface_definition(&solid));
    let mut base = serde_json::to_value(&solid).expect("definition");
    base["value"]["value"]["solid"] = serde_json::json!(false);
    let surface: FeatureDefinition = json::from_value(base.clone()).expect("surface loft");
    assert!(matches_surface_definition(&surface));
    assert!(!matches_definition(&surface));
    for (field, value) in [
        ("closed", serde_json::json!(true)),
        ("ruled", serde_json::json!(true)),
        ("linearize", serde_json::json!(true)),
        ("op", serde_json::json!("join")),
        ("max_degree", serde_json::json!(3)),
    ] {
        let mut changed = base.clone();
        changed["value"]["value"][field] = value;
        let changed = json::from_value(changed).expect("changed loft");
        assert!(!matches_surface_definition(&changed));
    }
}

#[test]
fn a_solid_loft_with_a_bad_source_value_keeps_its_section_record_reason() {
    let mut f = Fixture::new();
    f.properties[6] = edited(&f.properties[6], |p| {
        let PmDcFeaturePropertyKind::LoftSection { source_value, .. } = &mut p.kind else {
            panic!("section");
        };
        *source_value = 2;
    });
    assert_eq!(f.admitted(), Err(NativeReason::SectionRecord));
    // The reason holds whatever later section check also fails.
    f.properties[8] = edited(&f.properties[8], |p| {
        let PmDcFeaturePropertyKind::Enumeration { value, .. } = &mut p.kind else {
            panic!("condition");
        };
        *value = 1;
    });
    assert_eq!(f.admitted(), Err(NativeReason::SectionRecord));
}

#[test]
fn the_face_loop_participant_is_the_other_participant_with_its_record_type() {
    use crate::feature::loft_sweep::slots::SURFACE_LOFT_CLASS;
    use crate::feature::tests::test_label;
    let participant = type_id_string(FACE_LOOP_PARTICIPANT_TYPE);
    let other = type_id_string(SECTION_TYPE);
    // Participant values are record ordinals plus one.
    let label = test_label(20, 1, SURFACE_LOFT_CLASS, &[51, 52]);
    let typed = |ordinal: u32| match ordinal {
        51 => Some(participant.as_str()),
        50 => Some(other.as_str()),
        _ => None,
    };
    assert!(face_loop_participant(&label, 50, typed));
    // The sketch participant is the first one here: it is not the loop.
    assert!(!face_loop_participant(&label, 51, typed));
    assert!(!face_loop_participant(&label, 49, typed));
    assert!(!face_loop_participant(&label, 50, |_| None));
    let alone = test_label(20, 1, SURFACE_LOFT_CLASS, &[51]);
    assert!(!face_loop_participant(&alone, 50, typed));
    let twice = test_label(20, 1, SURFACE_LOFT_CLASS, &[51, 51]);
    assert!(!face_loop_participant(&twice, 50, typed));
}

#[test]
fn only_a_surface_loft_with_a_native_section_counts_as_unresolved() {
    use cadmpeg_ir::features::profiles::{LoftSection, PlanarProfileRef, ProfileRef};
    let mut base = serde_json::to_value(positional_definition()).expect("definition");
    base["value"]["value"]["solid"] = serde_json::json!(false);
    let surface: FeatureDefinition = json::from_value(base.clone()).expect("surface loft");
    let FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. }) = &surface else {
        panic!("loft");
    };
    // The sketch sections of the positional definition are resolved.
    assert!(!has_native_section(&surface));
    let mut native = sections.clone();
    native[0] = LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::native(
        "record".to_owned(),
    )));
    let mut with_native = base.clone();
    with_native["value"]["value"]["sections"] = serde_json::to_value(&native).expect("sections");
    let with_native: FeatureDefinition = json::from_value(with_native).expect("loft");
    assert!(has_native_section(&with_native));
    // The solid loft keeps its own accounting.
    let mut solid = serde_json::to_value(&with_native).expect("definition");
    solid["value"]["value"]["solid"] = serde_json::json!(true);
    let solid: FeatureDefinition = json::from_value(solid).expect("solid loft");
    assert!(!has_native_section(&solid));
}
