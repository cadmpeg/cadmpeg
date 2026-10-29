// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn rectangular_grids_keep_one_based_source_suppressions_as_zero_based_omissions() {
    for version in [19, 25, 28, 29, 30, 31] {
        let feature = project_fixture(&pattern(version, true), |operands, _, _| {
            operands.records.insert(
                ("generated".into(), 81),
                Some(Operand::Suppression {
                    pairs: vec![[3, 2], [2, 1]],
                }),
            );
        })
        .expect("resolved masked grid");
        let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
            feature.evaluation.definition()
        else {
            panic!("pattern");
        };
        let PatternTransform::Linear {
            count,
            second,
            omitted_instances,
            ..
        } = pattern.definition()
        else {
            panic!("linear grid");
        };
        assert_eq!(*count, 3.into());
        assert_eq!(second.as_ref().expect("second direction").count, 2.into());
        assert_eq!(omitted_instances, &[[2, 1], [1, 0]]);
        assert_eq!(pattern.occurrence_count(), Some(4));
    }
}

#[test]
fn grids_keep_a_suppressed_final_row_and_an_active_seed_only() {
    for pairs in [
        vec![[1, 2], [2, 2], [3, 2]],
        vec![[2, 1], [3, 1], [1, 2], [2, 2], [3, 2]],
    ] {
        let expected = 6 - pairs.len();
        let feature = project_fixture(&pattern(19, true), |operands, _, _| {
            operands.records.insert(
                ("generated".into(), 81),
                Some(Operand::Suppression { pairs }),
            );
        })
        .expect("masked grid");
        let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
            feature.evaluation.definition()
        else {
            panic!("pattern");
        };
        assert_eq!(pattern.occurrence_count(), Some(expected));
    }
}

#[test]
fn projection_refuses_invalid_native_suppression_coordinates_with_a_loss() {
    for pairs in [
        vec![[0, 1]],
        vec![[1, 1]],
        vec![[4, 1]],
        vec![[1, 3]],
        vec![[2, 1], [2, 1]],
    ] {
        assert!(project_fixture(&pattern(19, true), |operands, _, _| {
            operands.records.insert(
                ("generated".into(), 81),
                Some(Operand::Suppression { pairs }),
            );
        })
        .is_none());
    }
}

#[test]
fn two_by_two_grid_omits_the_last_position_and_keeps_both_nominal_counts() {
    let feature = project_fixture(&pattern(19, true), |operands, _, properties| {
        properties[3] = test_property(
            86,
            PmDcFeaturePropertyKind::RdxVariable {
                name: String::new(),
                name_value: 0,
                nominal_value: 2,
                model_value: 2,
            },
        );
        operands.records.insert(
            ("generated".into(), 81),
            Some(Operand::Suppression {
                pairs: vec![[2, 2]],
            }),
        );
    })
    .expect("three retained positions");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
        feature.evaluation.definition()
    else {
        panic!("pattern");
    };
    assert!(matches!(
        pattern.definition(),
        PatternTransform::Linear { count: cadmpeg_ir::features::Dimension::Literal(2), second: Some(LinearPatternDirection { count: cadmpeg_ir::features::Dimension::Literal(2), .. }), omitted_instances, .. }
            if omitted_instances == &[[1, 1]]
    ));
    assert_eq!(pattern.occurrence_count(), Some(3));
}
