// SPDX-License-Identifier: Apache-2.0
//! Construction values of another format's features: each one the written
//! record reads back, or a note that names it.

use cadmpeg_ir::features::{
    direct_edit::{DraftAnchor, DraftPull, FaceMotion},
    extents::{ExtrudeExtent, ExtrudeSide, LinearTermination},
    geometry::{FeatureDirection3, FinitePoint3},
    parameters::ParameterValue,
    selections::FaceSelection,
    Feature, FeatureDefinition, FeatureId, FeatureOperation,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length, NonZeroLength, PositiveAngle, SlopeAngle};

use super::foreign_carriers::{
    cube_with_foreign_fillet, cube_with_foreign_history, loss, plan_losses, push_foreign_feature,
    push_length_parameter, push_parameter,
};

const BOSS: &str = "other:model:feature#boss";

fn boss() -> FeatureId {
    FeatureId::mint(BOSS).unwrap()
}

fn decoded_feature<'a>(ir: &'a cadmpeg_ir::CadIr, name: &str) -> &'a Feature {
    ir.model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("{name} decodes back"))
}

fn owned_names<'a>(ir: &'a cadmpeg_ir::CadIr, feature: &Feature) -> Vec<&'a str> {
    let mut names = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.as_ref() == Some(&feature.id))
        .map(|parameter| parameter.name.as_str())
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
}

fn set_boss_extent(ir: &mut cadmpeg_ir::CadIr, new_extent: ExtrudeExtent) {
    let boss = ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id.as_str() == BOSS)
        .unwrap();
    boss.evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { extent, .. }) = definition
        else {
            panic!("extrude");
        };
        *extent = new_extent;
    });
}

fn blind(length: f64) -> ExtrudeSide {
    ExtrudeSide {
        termination: LinearTermination::Blind {
            length: NonZeroLength::new(length).unwrap(),
        },
        draft: None,
    }
}

fn decoded_extent(ir: &cadmpeg_ir::CadIr) -> (&Feature, &ExtrudeExtent) {
    let boss = decoded_feature(ir, "Boss");
    let FeatureDefinition::Operation(FeatureOperation::Extrude { extent, .. }) =
        boss.evaluation.definition()
    else {
        panic!("{:?}", boss.evaluation.definition());
    };
    (boss, extent)
}

#[test]
fn an_extrude_depth_a_float_off_its_expression_decodes_back_under_d1() {
    let depth = 6.000_000_000_000_001;
    let mut ir = cube_with_foreign_history();
    set_boss_extent(&mut ir, ExtrudeExtent::OneSided { side: blind(depth) });
    push_parameter(
        &mut ir,
        &boss(),
        "d3",
        "6 mm".into(),
        ParameterValue::Length(Length::new(depth).unwrap()),
    );
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.features-omitted").is_none(),
        "{losses:#?}"
    );

    let decoded = crate::test_support::ir::encode_decode(&ir);
    let (boss, extent) = decoded_extent(&decoded);
    assert!(
        matches!(
            extent,
            ExtrudeExtent::OneSided { side: ExtrudeSide {
                termination: LinearTermination::Blind { length }, ..
            } } if (length.get() - 6.0).abs() < 1.0e-9
        ),
        "{extent:?}"
    );
    assert_eq!(owned_names(&decoded, boss), ["D1"]);
}

#[test]
fn a_circular_pattern_decodes_back_with_its_count_and_angle() {
    use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed, PatternTransform};

    let mut ir = cube_with_foreign_history();
    let id = push_foreign_feature(&mut ir, "ring", "Ring", |_, _, _| {
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: vec![PatternSeed::Feature(boss())],
            pattern: PatternKind::new(PatternTransform::Circular {
                axis_origin: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                axis_dir: FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
                angle: PositiveAngle::new(std::f64::consts::TAU).unwrap(),
                count: 9,
            })
            .unwrap(),
        })
    });
    ir.model.features.last_mut().unwrap().dependencies =
        cadmpeg_ir::features::selections::DistinctMembers::try_from(vec![boss()]).unwrap();
    push_parameter(
        &mut ir,
        &id,
        "d29",
        "9".into(),
        ParameterValue::Real(FiniteReal::new(9.0).unwrap()),
    );
    push_parameter(
        &mut ir,
        &id,
        "d30",
        "360 deg".into(),
        ParameterValue::Angle(Angle::new(std::f64::consts::TAU).unwrap()),
    );
    let losses = plan_losses(&ir);
    let note = loss(&losses, "writer.parameters-renamed").expect("named rename");
    assert!(
        note.message
            .contains("other:model:parameter#d29 (d29 as Count)")
            && note
                .message
                .contains("other:model:parameter#d30 (d30 as Angle)"),
        "{}",
        note.message
    );

    let decoded = crate::test_support::ir::encode_decode(&ir);
    let ring = decoded_feature(&decoded, "Ring");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
        ring.evaluation.definition()
    else {
        panic!("{:?}", ring.evaluation.definition());
    };
    assert!(
        matches!(
            pattern.definition(),
            PatternTransform::Circular { angle, count: 9, .. }
                if (angle.get() - std::f64::consts::TAU).abs() < 1.0e-9
        ),
        "{pattern:?}"
    );
    assert_eq!(owned_names(&decoded, ring), ["Angle", "Count"]);
}

#[test]
fn a_draft_decodes_back_with_its_angle_under_d1() {
    let angle = 2.0_f64.to_radians();
    let mut ir = cube_with_foreign_history();
    let id = push_foreign_feature(&mut ir, "taper", "Taper", |state, face, _| {
        FeatureDefinition::Operation(FeatureOperation::Draft {
            faces: FaceSelection::historical(
                state,
                vec![face],
                "other:design:face-operand#taper".into(),
            )
            .unwrap(),
            anchor: DraftAnchor::NeutralPlane {
                plane: FaceSelection::Native("other:design:plane#taper".into()),
                pull: Some(DraftPull::new(
                    FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
                    None,
                )),
            },
            angle: Some(SlopeAngle::new(angle).unwrap()),
            outward: Some(false),
            second_side: None,
        })
    });
    push_parameter(
        &mut ir,
        &id,
        "d8",
        "2 deg".into(),
        ParameterValue::Angle(Angle::new(angle).unwrap()),
    );

    let decoded = crate::test_support::ir::encode_decode(&ir);
    let taper = decoded_feature(&decoded, "Taper");
    assert!(
        matches!(
            taper.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Draft { angle: Some(read), .. })
                if (read.get() - angle).abs() < 1.0e-9
        ),
        "{:?}",
        taper.evaluation.definition()
    );
    assert_eq!(owned_names(&decoded, taper), ["D1"]);
}

#[test]
fn a_move_face_decodes_back_with_its_distance_under_d1() {
    let mut ir = cube_with_foreign_history();
    let id = push_foreign_feature(&mut ir, "push", "Push", |state, face, _| {
        FeatureDefinition::Operation(FeatureOperation::MoveFace {
            faces: FaceSelection::historical(
                state,
                vec![face],
                "other:design:face-operand#push".into(),
            )
            .unwrap(),
            motion: FaceMotion::Offset {
                distance: Length::new(1.5).unwrap(),
            },
        })
    });
    push_length_parameter(&mut ir, &id, "d9", 1.5);

    let decoded = crate::test_support::ir::encode_decode(&ir);
    let push = decoded_feature(&decoded, "Push");
    assert!(
        matches!(
            push.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::MoveFace {
                motion: FaceMotion::Offset { distance },
                ..
            }) if distance.get() == 1.5
        ),
        "{:?}",
        push.evaluation.definition()
    );
    assert_eq!(owned_names(&decoded, push), ["D1"]);
}

#[test]
fn a_depth_with_two_holders_is_named_as_a_value_the_record_reads_back_without() {
    let mut ir = cube_with_foreign_history();
    push_length_parameter(&mut ir, &boss(), "d1", 12.0);
    push_length_parameter(&mut ir, &boss(), "d2", 12.0);
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.parameters-renamed").is_none(),
        "{losses:#?}"
    );
    let note = loss(&losses, "writer.feature-fields-omitted").expect("named value");
    assert!(
        note.message
            .contains(&format!("{BOSS} (extent.side.termination: blind read as")),
        "{}",
        note.message
    );
}

#[test]
fn a_two_sided_extrude_decodes_back_with_its_depths_under_d1_and_d2() {
    let mut ir = cube_with_foreign_history();
    set_boss_extent(
        &mut ir,
        ExtrudeExtent::TwoSided {
            first: blind(12.0),
            second: blind(5.0),
        },
    );
    push_length_parameter(&mut ir, &boss(), "d1", 12.0);
    push_length_parameter(&mut ir, &boss(), "d2", 5.0);
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.feature-fields-omitted").is_none(),
        "{losses:#?}"
    );
    let note = loss(&losses, "writer.history-keywords-only").expect("named summary");
    assert!(note.message.contains(BOSS), "{}", note.message);

    let decoded = crate::test_support::ir::encode_decode(&ir);
    let (boss, extent) = decoded_extent(&decoded);
    assert_eq!(
        extent,
        &ExtrudeExtent::TwoSided {
            first: blind(12.0),
            second: blind(5.0),
        }
    );
    assert_eq!(owned_names(&decoded, boss), ["D1", "D2"]);
}

#[test]
fn equal_two_sided_depths_with_one_holder_are_not_renamed_twice() {
    let mut ir = cube_with_foreign_history();
    set_boss_extent(
        &mut ir,
        ExtrudeExtent::TwoSided {
            first: blind(12.0),
            second: blind(12.0),
        },
    );
    push_length_parameter(&mut ir, &boss(), "d1", 12.0);
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.parameters-renamed").is_none(),
        "{losses:#?}"
    );
    let note = loss(&losses, "writer.feature-fields-omitted").expect("named value");
    assert!(note.message.contains(BOSS), "{}", note.message);
}

#[test]
fn property_names_a_keywords_attribute_cannot_hold_are_left_out_and_named() {
    let mut ir = cube_with_foreign_fillet();
    let fillet = ir.model.features.last_mut().unwrap();
    for name in ["Name", "xmlns"] {
        fillet.source_properties.insert(
            cadmpeg_core::text::NonBlankString::new(name.to_owned()).unwrap(),
            "urn:other".into(),
        );
    }
    let losses = plan_losses(&ir);
    assert!(
        losses.iter().any(
            |note| note.code.local_code() == "writer.feature-fields-omitted"
                && note
                    .message
                    .contains("other:model:feature#round (Name, other:sets, xmlns)")
        ),
        "{losses:#?}"
    );

    let decoded = crate::test_support::ir::encode_decode(&ir);
    let round = decoded_feature(&decoded, "Round");
    assert!(
        !round.source_properties.contains_key("xmlns"),
        "{:?}",
        round.source_properties
    );
}

#[test]
fn characters_xml_does_not_allow_are_replaced_and_named_and_a_tab_is_kept() {
    let mut ir = cube_with_foreign_history();
    let boss_feature = ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id.as_str() == BOSS)
        .unwrap();
    boss_feature.name = Some("Bo\u{1}ss".into());
    for (name, value) in [("Comment", "a\u{1}b"), ("Tabbed", "a\tb")] {
        boss_feature.source_properties.insert(
            cadmpeg_core::text::NonBlankString::new(name.to_owned()).unwrap(),
            value.into(),
        );
    }
    push_length_parameter(&mut ir, &boss(), "d1", 12.0);
    ir.model
        .parameters
        .last_mut()
        .unwrap()
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Note"), "x\u{b}y".into());
    let losses = plan_losses(&ir);
    let note = loss(&losses, "writer.keywords-characters-replaced").expect("named text");
    assert!(
        note.message.contains(BOSS) && note.message.contains("other:model:parameter#d1"),
        "{}",
        note.message
    );

    let decoded = crate::test_support::ir::encode_decode(&ir);
    let boss = decoded_feature(&decoded, "Bo\u{fffd}ss");
    assert_eq!(
        boss.source_properties.get("Comment").map(String::as_str),
        Some("a\u{fffd}b")
    );
    assert_eq!(
        boss.source_properties.get("Tabbed").map(String::as_str),
        Some("a\tb")
    );
    assert!(matches!(
        boss.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude { .. })
    ));
}

#[test]
fn a_selection_written_by_a_source_token_is_named_as_naming_no_written_record() {
    let mut ir = cube_with_foreign_history();
    let face = ir.geometry.faces[0].id.clone();
    let id = push_foreign_feature(&mut ir, "cap", "Cap", |_, _, _| {
        FeatureDefinition::Operation(FeatureOperation::DeleteFace {
            faces: FaceSelection::Resolved {
                faces: vec![face],
                native: "other:design:face#cap".into(),
            },
            heal: true,
        })
    });
    let losses = plan_losses(&ir);
    let note = loss(&losses, "writer.selection-identities-omitted").expect("named selection");
    assert!(
        note.message.contains(id.as_str())
            && note.message.contains("names no record of the written part"),
        "{}",
        note.message
    );
}

#[test]
fn a_profile_selection_of_every_profile_of_its_sketch_is_not_named() {
    use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};

    let mut ir = cube_with_foreign_history();
    let sketch = ir.model.sketches[0].id.clone();
    assert_eq!(ir.model.sketches[0].profiles.len(), 1);
    let boss = ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id.as_str() == BOSS)
        .unwrap();
    boss.evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) = definition
        else {
            panic!("extrude");
        };
        *profile = ProfileRef::Planar(PlanarProfileRef::SketchProfiles {
            sketch,
            profiles: cadmpeg_ir::features::selections::SelectionMembers::try_from(vec![0])
                .unwrap(),
        });
    });
    let losses = plan_losses(&ir);
    assert!(
        loss(&losses, "writer.feature-fields-omitted").is_none(),
        "{losses:#?}"
    );
}

/// `cube_with_foreign_history` plus a pattern `Grid` of the boss.
fn cube_with_foreign_pattern(
    transform: cadmpeg_ir::features::patterns::PatternTransform,
) -> (cadmpeg_ir::CadIr, FeatureId) {
    use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed};

    let mut ir = cube_with_foreign_history();
    let id = push_foreign_feature(&mut ir, "grid", "Grid", |_, _, _| {
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: vec![PatternSeed::Feature(boss())],
            pattern: PatternKind::new(transform).unwrap(),
        })
    });
    ir.model.features.last_mut().unwrap().dependencies =
        cadmpeg_ir::features::selections::DistinctMembers::try_from(vec![boss()]).unwrap();
    (ir, id)
}

fn decoded_pattern(
    ir: &cadmpeg_ir::CadIr,
) -> (&Feature, &cadmpeg_ir::features::patterns::PatternTransform) {
    let grid = decoded_feature(ir, "Grid");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
        grid.evaluation.definition()
    else {
        panic!("{:?}", grid.evaluation.definition());
    };
    (grid, pattern.definition())
}

fn assert_renamed(losses: &[cadmpeg_ir::report::loss::LossNote], renames: &[(&str, &str)]) {
    let note = loss(losses, "writer.parameters-renamed").expect("named rename");
    for (from, to) in renames {
        assert!(
            note.message
                .contains(&format!("other:model:parameter#{from} ({from} as {to})")),
            "{}",
            note.message
        );
    }
}

#[test]
fn a_linear_pattern_decodes_back_with_its_counts_and_spacings_under_d1_to_d4() {
    use cadmpeg_ir::features::patterns::{LinearPatternDirection, PatternTransform};
    use cadmpeg_ir::scalar::PositiveLength;

    let (mut ir, id) = cube_with_foreign_pattern(PatternTransform::Linear {
        direction: Some(FeatureDirection3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap()),
        spacing: PositiveLength::new(5.0).unwrap(),
        count: 4,
        second: Some(LinearPatternDirection {
            direction: FeatureDirection3::new(Vector3::new(0.0, 1.0, 0.0)).unwrap(),
            spacing: PositiveLength::new(7.0).unwrap(),
            count: 3,
            symmetric: false,
        }),
        omitted_instances: Vec::new(),
        symmetric: false,
    });
    push_parameter(&mut ir, &id, "d11", "4".into(), ParameterValue::Integer(4));
    push_length_parameter(&mut ir, &id, "d12", 5.0);
    push_parameter(&mut ir, &id, "d13", "3".into(), ParameterValue::Integer(3));
    push_length_parameter(&mut ir, &id, "d14", 7.0);
    let losses = plan_losses(&ir);
    assert_renamed(
        &losses,
        &[("d11", "D1"), ("d12", "D3"), ("d13", "D2"), ("d14", "D4")],
    );

    let decoded = crate::test_support::ir::encode_decode(&ir);
    let (grid, pattern) = decoded_pattern(&decoded);
    assert!(
        matches!(
            pattern,
            PatternTransform::Linear { spacing, count: 4, second: Some(second), .. }
                if (spacing.get() - 5.0).abs() < 1.0e-9
                    && second.count == 3
                    && (second.spacing.get() - 7.0).abs() < 1.0e-9
        ),
        "{pattern:?}"
    );
    assert_eq!(owned_names(&decoded, grid), ["D1", "D2", "D3", "D4"]);
}

#[test]
fn a_curve_driven_pattern_decodes_back_with_its_count_and_spacing_under_d1_and_d3() {
    use cadmpeg_ir::features::patterns::PatternTransform;
    use cadmpeg_ir::scalar::PositiveLength;

    let (mut ir, id) = cube_with_foreign_pattern(PatternTransform::CurveDriven {
        path: Some(cadmpeg_ir::features::profiles::PathRef::Sketch(
            cadmpeg_ir::sketches::SketchId::mint("other:model:sketch#profile").unwrap(),
        )),
        spacing: PositiveLength::new(2.5).unwrap(),
        count: 6,
    });
    push_parameter(&mut ir, &id, "d21", "6".into(), ParameterValue::Integer(6));
    push_length_parameter(&mut ir, &id, "d22", 2.5);
    let losses = plan_losses(&ir);
    assert_renamed(&losses, &[("d21", "D1"), ("d22", "D3")]);

    let decoded = crate::test_support::ir::encode_decode(&ir);
    let (grid, pattern) = decoded_pattern(&decoded);
    assert!(
        matches!(
            pattern,
            PatternTransform::CurveDriven { spacing, count: 6, .. }
                if (spacing.get() - 2.5).abs() < 1.0e-9
        ),
        "{pattern:?}"
    );
    assert_eq!(owned_names(&decoded, grid), ["D1", "D3"]);
}

#[test]
fn an_extrude_taper_decodes_back_under_draft() {
    let taper = 3.0_f64.to_radians();
    let mut ir = cube_with_foreign_history();
    set_boss_extent(
        &mut ir,
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                draft: Some(SlopeAngle::new(taper).unwrap()),
                ..blind(12.0)
            },
        },
    );
    push_length_parameter(&mut ir, &boss(), "d1", 12.0);
    push_parameter(
        &mut ir,
        &boss(),
        "d2",
        "3 deg".into(),
        ParameterValue::Angle(Angle::new(taper).unwrap()),
    );
    let losses = plan_losses(&ir);
    assert_renamed(&losses, &[("d1", "D1"), ("d2", "Draft")]);

    let decoded = crate::test_support::ir::encode_decode(&ir);
    let (boss, extent) = decoded_extent(&decoded);
    assert!(
        matches!(
            extent,
            ExtrudeExtent::OneSided { side: ExtrudeSide { draft: Some(draft), .. } }
                if (draft.get() - taper).abs() < 1.0e-9
        ),
        "{extent:?}"
    );
    assert_eq!(owned_names(&decoded, boss), ["D1", "Draft"]);
}

#[test]
fn a_face_rotation_decodes_back_with_its_angle_under_angle() {
    let turn = 15.0_f64.to_radians();
    let mut ir = cube_with_foreign_history();
    let id = push_foreign_feature(&mut ir, "turn", "Turn", |state, face, _| {
        FeatureDefinition::Operation(FeatureOperation::MoveFace {
            faces: FaceSelection::historical(
                state,
                vec![face],
                "other:design:face-operand#turn".into(),
            )
            .unwrap(),
            motion: FaceMotion::Rotate {
                axis_origin: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                axis_dir: FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
                angle: Angle::new(turn).unwrap(),
            },
        })
    });
    push_parameter(
        &mut ir,
        &id,
        "d31",
        "15 deg".into(),
        ParameterValue::Angle(Angle::new(turn).unwrap()),
    );
    let losses = plan_losses(&ir);
    assert_renamed(&losses, &[("d31", "Angle")]);

    let decoded = crate::test_support::ir::encode_decode(&ir);
    let turned = decoded_feature(&decoded, "Turn");
    assert!(
        matches!(
            turned.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::MoveFace {
                motion: FaceMotion::Rotate { angle, .. },
                ..
            }) if (angle.get() - turn).abs() < 1.0e-9
        ),
        "{:?}",
        turned.evaluation.definition()
    );
    assert_eq!(owned_names(&decoded, turned), ["Angle"]);
}

#[test]
fn the_bodies_a_feature_produces_are_named_when_its_record_reads_back_without_them() {
    let mut ir = cube_with_foreign_history();
    let body = ir.geometry.bodies[0].id.clone();
    ir.model
        .features
        .iter_mut()
        .find(|feature| feature.id.as_str() == BOSS)
        .unwrap()
        .evaluation
        .edit(|_, outputs| {
            *outputs =
                cadmpeg_ir::features::selections::DistinctMembers::try_from(vec![body]).unwrap();
        });
    let losses = plan_losses(&ir);
    assert!(
        losses.iter().any(|note| {
            note.code.local_code() == "writer.feature-fields-omitted"
                && note.message.contains(&format!(
                    "writes 1 feature(s) whose records read back without 1 of the bodies their \
                     source features produce: `{BOSS}`"
                ))
        }),
        "{losses:#?}"
    );
}

#[test]
fn a_dimension_whose_name_collides_after_replacement_is_named_as_not_written() {
    let mut ir = cube_with_foreign_history();
    push_length_parameter(&mut ir, &boss(), "d1", 12.0);
    for (name, source_name, value) in [("w1", "w\u{1}", 3.0), ("w2", "w\u{2}", 4.0)] {
        push_length_parameter(&mut ir, &boss(), name, value);
        ir.model.parameters.last_mut().unwrap().name = source_name.into();
    }
    let losses = plan_losses(&ir);
    let dropped = losses
        .iter()
        .find(|note| {
            note.code.local_code() == "writer.features-omitted"
                && note
                    .message
                    .contains("writes no dimension for 1 parameter(s)")
        })
        .unwrap_or_else(|| panic!("{losses:#?}"));
    assert!(
        dropped.message.ends_with(": other:model:parameter#w2"),
        "{}",
        dropped.message
    );
    let replaced = loss(&losses, "writer.keywords-characters-replaced").expect("named text");
    assert!(
        replaced.message.contains("other:model:parameter#w1")
            && !replaced.message.contains("other:model:parameter#w2"),
        "{}",
        replaced.message
    );

    let decoded = crate::test_support::ir::encode_decode(&ir);
    let (boss, _) = decoded_extent(&decoded);
    assert_eq!(owned_names(&decoded, boss), ["D1", "w\u{fffd}"]);
}
