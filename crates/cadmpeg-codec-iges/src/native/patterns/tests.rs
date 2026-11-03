// SPDX-License-Identifier: Apache-2.0
//! Rectangular and circular arrays project to typed neutral patterns.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

fn decode(bytes: Vec<u8>) -> cadmpeg_test_support::EditableDecodeResult {
    cadmpeg_test_support::EditableDecodeResult::from(
        crate::IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("array fixture decodes"),
    )
}

fn assert_valid(result: &cadmpeg_test_support::EditableDecodeResult) {
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn rectangular_do_lists_that_omit_the_origin_stay_native_with_a_loss() {
    let result = decode(crate::test_support::test_solids_and_structure::patterned_instance_file());
    assert!(
        result
            .ir()
            .model
            .features
            .iter()
            .all(|feature| feature.native_ref.as_deref()
                != Some("iges:product:rectangular-array#D3")),
        "a DO list that omits the grid origin has no linear pattern home"
    );
    assert!(
        result.report().losses.iter().any(|loss| loss.code
            == crate::loss::IgesLossCode::NeutralProjectionUnavailable.kind()
            && loss.message
                == "iges:product:rectangular-array#D3: pattern omitted_instances must contain distinct nonseed grid positions"),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn rectangular_dont_lists_project_linear_patterns_with_both_directions() {
    use cadmpeg_ir::features::patterns::{
        LinearPatternDirection, PatternDistance, PatternTransform,
    };
    use cadmpeg_ir::features::{Dimension, FeatureDirection3, FeatureOperation};
    use cadmpeg_ir::scalar::Length;

    let result = decode(owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "BASE".into(),
            status: "00000000",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 412,
            form: 0,
            label: "RECT".into(),
            status: "00000000",
            parameters: "412,1,1,0,0,0,2,1,4,2,0.0,1,1,2;".into(),
        },
    ]));
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("iges:product:rectangular-array#D3"))
        .expect("rectangular array projects a pattern");
    let cadmpeg_ir::features::FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds,
        pattern,
    }) = &feature.evaluation.definition()
    else {
        panic!("rectangular array projects a pattern operation");
    };
    assert!(seeds.is_empty());
    let PatternTransform::Linear {
        direction,
        distance,
        count,
        second,
        omitted_instances,
    } = pattern.definition()
    else {
        panic!("rectangular array projects a linear pattern");
    };
    assert_eq!(
        *direction.as_ref().expect("column direction"),
        FeatureDirection3::from(cadmpeg_ir::units::UnitVector3::X_AXIS)
    );
    assert_eq!(
        *distance,
        PatternDistance::Spacing {
            distance: Dimension::Literal(Length::new(4.0).expect("column spacing is finite")),
        }
    );
    assert_eq!(*count, Dimension::Literal(2_u32));
    let LinearPatternDirection {
        direction: row_direction,
        distance: row_distance,
        count: row_count,
    } = second.as_ref().expect("row direction");
    assert_eq!(
        *row_direction,
        FeatureDirection3::from(cadmpeg_ir::units::UnitVector3::Y_AXIS)
    );
    assert_eq!(
        *row_distance,
        PatternDistance::Spacing {
            distance: Dimension::Literal(Length::new(2.0).expect("row spacing is finite")),
        }
    );
    assert_eq!(*row_count, Dimension::Literal(1_u32));
    assert_eq!(*omitted_instances, vec![[1_u32, 0_u32]]);
    assert_valid(&result);
}

#[test]
fn circular_dont_lists_that_omit_the_origin_stay_native_with_a_loss() {
    let result = decode(crate::test_support::test_solids_and_structure::patterned_instance_file());
    assert!(
        result
            .ir()
            .model
            .features
            .iter()
            .all(|feature| feature.native_ref.as_deref() != Some("iges:product:circular-array#D5")),
        "a DON'T list that omits the grid origin has no circular pattern home"
    );
    assert!(
        result.report().losses.iter().any(|loss| loss.code
            == crate::loss::IgesLossCode::NeutralProjectionUnavailable.kind()
            && loss.message
                == "iges:product:circular-array#D5: pattern angles must start at zero and strictly increase"),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn fully_included_circular_arrays_project_spans_and_rectangular_omissions() {
    use cadmpeg_ir::features::patterns::PatternTransform;
    use cadmpeg_ir::features::patterns::{CircularPatternDistribution, PatternAngle};
    use cadmpeg_ir::features::FeatureOperation;

    let result = decode(owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "BASE".into(),
            status: "00000000",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 412,
            form: 0,
            label: "RECT".into(),
            status: "00000000",
            parameters: "412,1,1,0,0,0,2,1,4,2,0.0,1,1,2;".into(),
        },
        OwnedTestEntity {
            entity_type: 414,
            form: 0,
            label: "CIRCLE".into(),
            status: "00000000",
            parameters: "414,1,4,0,0,0,5,0,1.0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 414,
            form: 0,
            label: "PARTIAL".into(),
            status: "00000000",
            parameters: "414,1,4,0,0,0,5,0,1.0,2,1,2,4;".into(),
        },
    ]));
    let rectangular = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("iges:product:rectangular-array#D3"))
        .expect("rectangular array projects a pattern");
    let cadmpeg_ir::features::FeatureDefinition::Operation(FeatureOperation::Pattern {
        pattern,
        ..
    }) = &rectangular.evaluation.definition()
    else {
        panic!("rectangular array projects a pattern operation");
    };
    let PatternTransform::Linear {
        omitted_instances, ..
    } = pattern.definition()
    else {
        panic!("rectangular array projects a linear pattern");
    };
    assert_eq!(*omitted_instances, vec![[1_u32, 0_u32]]);
    let circular = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("iges:product:circular-array#D5"))
        .expect("circular array projects a pattern");
    let cadmpeg_ir::features::FeatureDefinition::Operation(FeatureOperation::Pattern {
        pattern,
        ..
    }) = &circular.evaluation.definition()
    else {
        panic!("circular array projects a pattern operation");
    };
    let PatternTransform::Circular { angle, count, .. } = pattern.definition() else {
        panic!("included circular instances project a span");
    };
    let PatternAngle::Span {
        angle,
        distribution,
    } = angle
    else {
        panic!("included circular instances span their total angle");
    };
    assert!((angle.literal().unwrap().get() - 3.0).abs() < 1.0e-12);
    assert!(matches!(
        distribution,
        CircularPatternDistribution::Inclusive
    ));
    assert_eq!(*count, cadmpeg_ir::features::Dimension::Literal(4_u32));
    let partial = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("iges:product:circular-array#D7"))
        .expect("listed circular instances project explicit angles");
    let cadmpeg_ir::features::FeatureDefinition::Operation(FeatureOperation::Pattern {
        pattern,
        ..
    }) = &partial.evaluation.definition()
    else {
        panic!("listed circular instances project a pattern operation");
    };
    let PatternTransform::CircularAngles { angles, .. } = pattern.definition() else {
        panic!("listed circular instances project explicit angles");
    };
    assert!(
        angles
            .iter()
            .map(|angle| angle.literal().unwrap().get())
            .collect::<Vec<_>>()
            == vec![0.0, 2.0],
        "{angles:?}"
    );
    assert_valid(&result);
}
