// SPDX-License-Identifier: Apache-2.0
//! Circular pattern seeds, suppressed instances, distribution and axis controls.

use super::*;

#[test]
fn circular_pattern_seed_role_selects_body_or_face() {
    for (role, expected_seed) in [
        (
            DesignOperandRole::BODIES_B,
            PatternSeed::Bodies(BodySelection::Native(
                "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
            )),
        ),
        (
            DesignOperandRole::BODIES_A,
            PatternSeed::Faces(FaceSelection::Native(
                "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
            )),
        ),
    ] {
        for symmetric in [None, Some(false)] {
            let mut scope = circular_scope();
            if let crate::records::feature::scope::DesignScopePayloadMut::CPattern(Some(
                construction,
            )) = scope.payload_mut()
            {
                construction.symmetric =
                    symmetric.map(|value| crate::records::identity::Located { value, offset: 21 });
            }
            let mut losses = Vec::new();
            let definition = project_circular_pattern(
                &scope,
                &[group(10, 20, role)],
                &[],
                &[],
                &[],
                &listed(&[5]),
                &mut losses,
            )
            .expect("circular pattern");
            assert_unread_circular_controls(&losses);
            let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
                definition
            else {
                panic!("circular pattern definition");
            };
            assert_eq!(seeds, vec![expected_seed.clone()]);
            assert!(matches!(
                pattern.definition(),
                PatternTransform::Circular { .. }
            ));
        }
    }
}

#[test]
fn suppressed_circular_instances_leave_the_remaining_angles_or_are_charged() {
    // `keys` suppressed in a pattern of `count` over `angle`; `symmetric` is
    // the payload byte, and `separator` marks a 130-byte axis record.
    let with =
        |count: u32, angle: f64, symmetric: Option<bool>, separator: bool, keys: Vec<u32>| {
            let mut scope = circular_scope();
            if let crate::records::feature::scope::DesignScopePayloadMut::CPattern(Some(
                construction,
            )) = scope.payload_mut()
            {
                construction.count = std::num::NonZeroU32::new(count).unwrap();
                construction.angle = cadmpeg_ir::scalar::PositiveAngle::new(angle).unwrap();
                construction.symmetric =
                    symmetric.map(|value| crate::records::identity::Located { value, offset: 21 });
                construction.axis_separator_flag_offset = separator.then_some(40);
                construction.suppressed = Some(crate::records::identity::Located {
                    value: keys,
                    offset: 170,
                });
            }
            let mut losses = Vec::new();
            let pattern = project_circular_pattern(
                &scope,
                &[group(10, 20, DesignOperandRole::BODIES_B)],
                &[],
                &[],
                &[],
                &listed(&[5]),
                &mut losses,
            )
            .map(|definition| {
                let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
                    definition
                else {
                    panic!("circular pattern definition");
                };
                match pattern.definition() {
                    PatternTransform::CircularAngles { angles, .. } => Ok(angles
                        .iter()
                        .map(|angle| angle.get().to_degrees() * 10.0)
                        .map(f64::round)
                        .collect::<Vec<_>>()),
                    PatternTransform::Circular { count, .. } => Err(*count),
                    other => panic!("circular pattern transform: {other:?}"),
                }
            });
            (
                pattern,
                losses.into_iter().map(|loss| loss.code).collect::<Vec<_>>(),
            )
        };
    let suppression =
        crate::loss::F3dLossCode::FeatureCircularPatternSuppressionNotTransferred.kind();
    let controls = crate::loss::F3dLossCode::FeatureCircularPatternControlsAssumed.kind();
    // A full turn of six steps 60 degrees; instances 3 and 5 have keys 4 and 6.
    let (pattern, losses) = with(6, std::f64::consts::TAU, Some(false), false, vec![4, 6]);
    assert_eq!(pattern, Some(Ok(vec![0.0, 600.0, 1200.0, 2400.0])));
    assert_eq!(losses, [controls.clone()]);
    // A partial turn of five over 90 degrees steps 22.5 degrees.
    let (pattern, _) = with(5, std::f64::consts::FRAC_PI_2, Some(false), false, vec![2]);
    assert_eq!(pattern, Some(Ok(vec![0.0, 450.0, 675.0, 900.0])));
    // A symmetric pattern, or one about a 130-byte axis record, keeps every
    // instance and charges the suppression.
    for (symmetric, separator) in [(Some(true), false), (None, false), (Some(false), true)] {
        let (pattern, losses) = with(6, std::f64::consts::TAU, symmetric, separator, vec![4]);
        assert_eq!(pattern, Some(Err(6)), "{symmetric:?} {separator}");
        assert_eq!(
            losses,
            [suppression.clone(), controls.clone()],
            "{symmetric:?} {separator}"
        );
    }
}

#[test]
fn a_symmetric_partial_turn_reports_its_dropped_distribution() {
    let with = |angle: f64, symmetric: bool| {
        let mut scope = circular_scope();
        if let crate::records::feature::scope::DesignScopePayloadMut::CPattern(Some(construction)) =
            scope.payload_mut()
        {
            construction.symmetric = Some(crate::records::identity::Located {
                value: symmetric,
                offset: 21,
            });
            construction.angle = cadmpeg_ir::scalar::PositiveAngle::new(angle).unwrap();
        }
        let mut losses = Vec::new();
        project_circular_pattern(
            &scope,
            &[group(10, 20, DesignOperandRole::BODIES_B)],
            &[],
            &[],
            &[],
            &listed(&[5]),
            &mut losses,
        )
        .expect("circular pattern");
        losses
    };
    let losses = with(std::f64::consts::FRAC_PI_2, true);
    assert_eq!(losses.len(), 2);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureCircularPatternControlsAssumed.kind()
    );
    assert_eq!(
        losses[1].code,
        crate::loss::F3dLossCode::FeatureCircularPatternPlacementNotTransferred.kind()
    );
    assert!(losses[1].message.contains("symmetric distribution"));
    for losses in [
        with(std::f64::consts::TAU, true),
        with(std::f64::consts::FRAC_PI_2, false),
    ] {
        let [loss] = losses.as_slice() else {
            panic!("one circular control assumption loss");
        };
        assert_eq!(
            loss.code,
            crate::loss::F3dLossCode::FeatureCircularPatternControlsAssumed.kind()
        );
    }
}

fn assert_unread_circular_controls(losses: &[cadmpeg_ir::report::loss::LossNote]) {
    let [loss] = losses else {
        panic!("one circular control assumption loss");
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::FeatureCircularPatternControlsAssumed.kind()
    );
    assert!(loss.message.contains("equal spacing"));
    assert!(loss.message.contains("spacing control remains unread"));
    assert!(!loss.message.contains("Compute"));
}

#[test]
fn circular_pattern_unread_controls_cover_partial_turn_and_feature_seeds() {
    for feature_seed in [false, true] {
        let mut scope = circular_scope();
        if let crate::records::feature::scope::DesignScopePayloadMut::CPattern(Some(construction)) =
            scope.payload_mut()
        {
            construction.count = std::num::NonZeroU32::new(2).unwrap();
            construction.angle =
                cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::FRAC_PI_2).unwrap();
        }
        let feature = seed_scope(5, Some(2));
        let selections = if feature_seed {
            vec![compact_entity_selection(30, 31, 5)]
        } else {
            Vec::new()
        };
        let mut losses = Vec::new();
        let definition = project_circular_pattern(
            &scope,
            &[group(10, 30, DesignOperandRole::BODIES_A)],
            &[],
            &selections,
            &[scope.clone(), feature.clone()],
            &listed(&[5]),
            &mut losses,
        )
        .expect("partial-turn circular pattern");
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) = definition
        else {
            panic!("circular pattern definition");
        };
        if feature_seed {
            assert_eq!(
                seeds,
                vec![PatternSeed::Feature(crate::ids::neutral_feature_id(
                    &feature
                ))]
            );
        }
        let PatternTransform::Circular { angle, count, .. } = pattern.definition() else {
            panic!("circular transform");
        };
        assert_eq!(*count, 2);
        assert!((angle.get() - std::f64::consts::FRAC_PI_2).abs() < EPS_SPACING);
        let [controls, distribution] = losses.as_slice() else {
            panic!("controls and distribution losses");
        };
        assert_unread_circular_controls(std::slice::from_ref(controls));
        assert_eq!(
            distribution.code,
            crate::loss::F3dLossCode::FeatureCircularPatternDistributionAssumed.kind()
        );
        assert!(distribution.message.contains("one-sided"));
    }
}

#[test]
fn a_separator_axis_projects_only_a_full_turn() {
    let with = |angle: f64| {
        let mut scope = circular_scope();
        if let crate::records::feature::scope::DesignScopePayloadMut::CPattern(Some(construction)) =
            scope.payload_mut()
        {
            construction.axis_separator_flag_offset = Some(104);
            construction.angle = cadmpeg_ir::scalar::PositiveAngle::new(angle).unwrap();
        }
        project_circular_pattern(
            &scope,
            &[group(10, 20, DesignOperandRole::BODIES_B)],
            &[],
            &[],
            &[],
            &listed(&[5]),
            &mut Vec::new(),
        )
    };
    assert!(with(std::f64::consts::TAU).is_some());
    assert!(with(std::f64::consts::FRAC_PI_2).is_none());
}
