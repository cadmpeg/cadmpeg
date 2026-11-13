// SPDX-License-Identifier: Apache-2.0
//! Extrude extents carried only by fixed scalar lanes.

use crate::design::feature_project::extrude::project_extrude;
use crate::ids::neutral_sketch_id;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::fixed_parameters::{
    DesignFixedExtrudeDistance, DesignFixedExtrudeParameters, DesignFixedExtrudeScalar,
};
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use cadmpeg_ir::features::{
    ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureOperation, LinearTermination,
    PlanarProfileRef, ProfileRef,
};

fn distance(
    value: f64,
    record_index: u32,
) -> DesignFixedExtrudeScalar<cadmpeg_ir::scalar::NonZeroReal> {
    DesignFixedExtrudeScalar {
        value: cadmpeg_ir::scalar::NonZeroReal::new(value).unwrap(),
        record_index,
        value_offset: u64::from(record_index) * 100,
    }
}

fn placement() -> DesignSketchPlacement {
    DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            600,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeExplicit(
                crate::records::sketch_placement::SketchPlacementMatrix::try_from([
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ])
                .unwrap(),
            ),
        )
        .unwrap(),
        id: "f3d:Design/BulkStream.dat:placement#200".into(),
        scope_record_index: Some(11),
        entity_id: crate::records::identity::DesignEntityId::try_from("0_172".to_owned())
            .expect("valid entity ID"),
        visibility: None,
        display_name: None,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        record_index: 200,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("260".to_owned())
            .unwrap(),
    }
}

/// A new-body Extrude of the whole Sketch `0_172` with the given extent and
/// fixed lanes and no indexed parameters.
fn fixed_extrude(
    extent: DesignExtrudeExtent,
    fixed: DesignFixedExtrudeParameters,
) -> DesignParameterScope {
    let direction = match extent {
        DesignExtrudeExtent::TwoSidedDistance | DesignExtrudeExtent::TwoSidedDistanceThroughAll => {
            2
        }
        DesignExtrudeExtent::SymmetricDistance => 3,
        _ => 1,
    };
    DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:scope#12".into(),
            byte_offset: 100,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 210,
            payload: DesignScopePayload::Extrude(Some(
                crate::records::feature::scope::DesignExtrudeScope {
                    extrude_prologue: Some(DesignExtrudePrologue::ReferenceAware {
                        reference: None,
                        operation: DesignExtrudeOperation::NewBody,
                        operation_offset: 128,
                        direction_face_extend_values: [direction, 0],
                        side_extent_discriminators: [1, 1],
                        side_extent_discriminator_offsets: [177, 190],
                        first_side_target_ordinal: None,
                        distance_tail: None,
                        extent,
                        direction_face_extend_offsets: [132, 136],
                        direction_reversed: false,
                        direction_reversed_offset: 140,
                        solid_operation: true,
                        solid_operation_offset: 141,
                        start: DesignExtrudeStart::ProfilePlane,
                        start_offset: 142,
                    }),
                    fixed_extrude_parameters: Some(fixed),
                    extrude_profile: Some(
                        DesignSketchProfileOperand::try_new(
                            crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
                                scope_reference_ordinal: 0,
                                record_index: 100,
                                byte_offset: 300,
                                class_tag: crate::records::references::DesignClassTag::try_from(
                                    "308".to_owned(),
                                )
                                .unwrap(),
                                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                                    "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b".to_owned(),
                                )
                                .unwrap(),
                                asset_id_offset: 330,
                                entity_id: crate::records::identity::DesignEntityId::try_from(
                                    "0_172".to_owned(),
                                )
                                .expect("valid entity identity"),
                                entity_reference_offset: 420,
                                compact_tail_flags: None,
            region_selection: None,
                                paired_class_tag:
                                    crate::records::references::DesignClassTag::try_from(
                                        "259".to_owned(),
                                    )
                                    .unwrap(),
                                paired_byte_offset: 520,
                            },
                        )
                        .unwrap(),
                    ),
                    ..Default::default()
                },
            )),
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            label: None,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 180,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![100],
                vec![185],
                "reference_members",
            )
            .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            deleted_members: None,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 300,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}

fn projected(scope: &DesignParameterScope) -> Option<FeatureDefinition> {
    let placement = placement();
    crate::test_support::with_decode_context(|decode_ctx| {
        project_extrude(
            decode_ctx,
            scope,
            &[],
            &[],
            &[],
            std::slice::from_ref(&placement),
            &[],
        )
    })
    .expect("project the extrude feature")
}

/// Fixed first-side and second-side lanes in centimetres define a two-sided
/// blind Extrude in millimetres.
#[test]
fn fixed_two_sided_distance_lanes_project_two_blind_sides() {
    let scope = fixed_extrude(
        DesignExtrudeExtent::TwoSidedDistance,
        DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance(4.3, 11))),
            taper_angle: None,
            against_distance: Some(distance(0.2, 17)),
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        },
    );
    let definition = projected(&scope).expect("typed two-sided Extrude");
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile: ProfileRef::Planar(PlanarProfileRef::Sketch(profile)),
        extent:
            ExtrudeExtent::TwoSided {
                first:
                    ExtrudeSide {
                        termination: LinearTermination::Blind { length: first },
                        draft: None,
                    },
                second:
                    ExtrudeSide {
                        termination: LinearTermination::Blind { length: second },
                        draft: None,
                    },
            },
        ..
    }) = definition
    else {
        panic!("two-sided blind Extrude: {definition:?}");
    };
    assert_eq!(profile, neutral_sketch_id(&placement()));
    assert!((first.get() - 43.0).abs() < 1.0e-12);
    assert!((second.get() - 2.0).abs() < 1.0e-12);
}

#[test]
fn fixed_symmetric_distance_projects_equal_side_distances() {
    let scope = fixed_extrude(
        DesignExtrudeExtent::SymmetricDistance,
        DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance(1.2, 11))),
            taper_angle: None,
            against_distance: None,
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        },
    );
    let Some(FeatureDefinition::Operation(FeatureOperation::Extrude {
        extent: ExtrudeExtent::Symmetric { side },
        ..
    })) = projected(&scope)
    else {
        panic!("fixed symmetric distance");
    };
    assert!(
        matches!(side.termination, LinearTermination::Blind { length } if length.get() == 24.0)
    );
}

#[test]
fn two_sided_distance_with_a_zero_side_travels_the_other_side() {
    let scope = fixed_extrude(
        DesignExtrudeExtent::TwoSidedDistance,
        DesignFixedExtrudeParameters::default(),
    );
    let parameter = |kind: &str, unit: &str, value| {
        crate::design::decode::parameters::parse_design_parameter_record(
            &crate::design::test_support::parameter_record(
                Some(44),
                "value",
                kind,
                Some(unit),
                "d1",
                value,
            ),
        )
        .unwrap()
    };
    let along = parameter("AlongDistance", "mm", 0.0);
    let against = parameter("AgainstDistance", "mm", 1.2);
    let first_taper = parameter("TaperAngle", "deg", 0.1);
    let second_taper = parameter("Side2TaperAngle", "deg", 0.05);
    let placement = placement();
    let project = |second_taper: &crate::records::parameters::DesignParameter| {
        crate::test_support::with_decode_context(|ctx| {
            project_extrude(
                ctx,
                &scope,
                &[
                    (0, &along),
                    (1, &against),
                    (2, &first_taper),
                    (3, second_taper),
                ],
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .unwrap()
        .expect("two-sided Extrude with zero first side")
    };
    let definition = project(&second_taper);
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        direction,
        extent: ExtrudeExtent::OneSided { side },
        ..
    }) = definition
    else {
        panic!("one-sided Extrude definition");
    };
    assert_eq!(
        direction,
        cadmpeg_ir::features::ExtrudeDirection::ReversedProfileNormal {}
    );
    assert!(
        matches!(side.termination, LinearTermination::Blind { length } if length.get() == 12.0)
    );
    assert_eq!(
        side.draft.map(cadmpeg_ir::scalar::SlopeAngle::get),
        Some(0.05)
    );

    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        extent: ExtrudeExtent::OneSided { side },
        ..
    }) = project(&parameter("Side2TaperAngle", "deg", 0.0))
    else {
        panic!("one-sided Extrude definition");
    };
    assert_eq!(side.draft, None);
}

#[test]
fn fixed_distance_and_through_all_project_on_opposite_sides() {
    let scope = fixed_extrude(
        DesignExtrudeExtent::TwoSidedDistanceThroughAll,
        DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance(1.5, 11))),
            ..Default::default()
        },
    );
    let Some(FeatureDefinition::Operation(FeatureOperation::Extrude {
        extent: ExtrudeExtent::TwoSided { first, second },
        ..
    })) = projected(&scope)
    else {
        panic!("distance and through-all Extrude");
    };
    assert!(
        matches!(first.termination, LinearTermination::Blind { length } if length.get() == 15.0)
    );
    assert!(matches!(
        second.termination,
        LinearTermination::ThroughAll {}
    ));
    let wire = serde_json::to_string(&DesignExtrudeExtent::TwoSidedDistanceThroughAll).unwrap();
    assert_eq!(wire, "\"two_sided_distance_through_all\"");
    assert_eq!(
        serde_json::from_str::<DesignExtrudeExtent>(&wire).unwrap(),
        DesignExtrudeExtent::TwoSidedDistanceThroughAll
    );
}

#[test]
fn unresolved_sketch_placement_retains_the_native_profile() {
    let scope = fixed_extrude(
        DesignExtrudeExtent::OneSidedDistance,
        DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance(1.5, 11))),
            ..Default::default()
        },
    );
    let definition = crate::test_support::with_decode_context(|ctx| {
        project_extrude(ctx, &scope, &[], &[], &[], &[], &[])
    })
    .unwrap()
    .expect("Extrude with unresolved sketch placement");
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile: ProfileRef::Planar(PlanarProfileRef::Native(id)),
        extent: ExtrudeExtent::OneSided { side },
        ..
    }) = definition
    else {
        panic!("Extrude with native profile");
    };
    assert_eq!(id, scope.id);
    assert!(
        matches!(side.termination, LinearTermination::Blind { length } if length.get() == 15.0)
    );
}

#[test]
fn multiple_native_sketch_profiles_project_with_a_native_profile() {
    let mut scope = fixed_extrude(
        DesignExtrudeExtent::TwoSidedDistance,
        DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance(1.0, 11))),
            against_distance: Some(distance(2.0, 17)),
            ..Default::default()
        },
    );
    assert!(projected(&scope).is_some());
    let first = scope.extrude_profile().unwrap().clone();
    let mut second = first.clone().into_draft();
    second.record_index = 200;
    second.entity_id = "0_173".to_owned().try_into().unwrap();
    let extrude = scope.extrude_mut().unwrap();
    extrude.extrude_profile = None;
    extrude.extrude_native_profiles = Some(vec![
        first,
        DesignSketchProfileOperand::try_new(second).unwrap(),
    ]);
    let Some(FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile: ProfileRef::Planar(PlanarProfileRef::Native(profile)),
        extent: ExtrudeExtent::TwoSided { first, second },
        ..
    })) = projected(&scope)
    else {
        panic!("projected Extrude with native profile");
    };
    assert_eq!(profile, scope.id);
    assert!(
        matches!(first.termination, LinearTermination::Blind { length } if length.get() == 10.0)
    );
    assert!(
        matches!(second.termination, LinearTermination::Blind { length } if length.get() == 20.0)
    );
}

/// The measurement word, when present, states whether each side travels the
/// stored distance, for a fixed lane and an indexed parameter alike.
#[test]
fn the_measurement_word_selects_each_side_or_whole_symmetric_travel() {
    use crate::records::feature::extrude::{DesignExtrudeDistanceTail, DesignExtrudeMeasurement};
    let whole_travel = |scope: &DesignParameterScope, along: Option<f64>| {
        let along = along.map(|value| {
            crate::design::decode::parameters::parse_design_parameter_record(
                &crate::design::test_support::parameter_record(
                    Some(44),
                    "value",
                    "AlongDistance",
                    Some("mm"),
                    "d1",
                    value,
                ),
            )
            .expect("generated feature parameter is canonical")
        });
        let parameters = along.iter().map(|along| (0, along)).collect::<Vec<_>>();
        let placement = placement();
        let definition = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                scope,
                &parameters,
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")?;
        match definition {
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                extent:
                    ExtrudeExtent::Symmetric {
                        side:
                            ExtrudeSide {
                                termination: LinearTermination::Blind { length },
                                ..
                            },
                    },
                ..
            }) => Some(length.get()),
            _ => panic!("symmetric blind Extrude: {definition:?}"),
        }
    };
    let scope =
        |fixed: Option<f64>, sides: [u32; 2], measurement: Option<DesignExtrudeMeasurement>| {
            let mut scope = fixed_extrude(
                DesignExtrudeExtent::SymmetricDistance,
                DesignFixedExtrudeParameters {
                    along_distance: fixed
                        .map(|value| DesignFixedExtrudeDistance::FixedScalar(distance(value, 11))),
                    ..Default::default()
                },
            );
            let Some(DesignExtrudePrologue::ReferenceAware {
                side_extent_discriminators,
                distance_tail,
                ..
            }) = scope.extrude_prologue_mut()
            else {
                panic!("reference-aware prologue");
            };
            *side_extent_discriminators = sides;
            *distance_tail = measurement.map(|measurement| DesignExtrudeDistanceTail {
                target_option: 1,
                target_option_offset: 186,
                measurement,
                measurement_offset: 199,
            });
            scope
        };
    for sides in [[1, 0], [1, 1]] {
        for (measurement, factor) in [
            (DesignExtrudeMeasurement::EachSide, 2.0),
            (DesignExtrudeMeasurement::WholeTravel, 1.0),
        ] {
            let fixed = whole_travel(&scope(Some(1.2), sides, Some(measurement)), None)
                .expect("a fixed lane with a measurement word");
            assert!(
                (fixed - 12.0 * factor).abs() < 1.0e-12,
                "{sides:?} {measurement:?}"
            );
            let parameter = whole_travel(&scope(None, sides, Some(measurement)), Some(0.55))
                .expect("a parameter with a measurement word");
            assert!(
                (parameter - 5.5 * factor).abs() < 1.0e-12,
                "{sides:?} {measurement:?}"
            );
        }
    }
    assert_eq!(
        whole_travel(&scope(Some(1.2), [1, 0], None), None),
        Some(24.0)
    );
    let untailed = whole_travel(&scope(None, [1, 0], None), Some(0.55)).unwrap();
    assert!((untailed - 5.5).abs() < 1.0e-12);
}

mod through_all;
