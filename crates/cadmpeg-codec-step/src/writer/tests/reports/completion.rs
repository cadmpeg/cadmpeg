// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn a_stale_feature_charges_evaluation_stale_on_step_export() {
    use cadmpeg_ir::features::{
        BooleanOp, Dimension, Feature, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureOperation, PrimitiveSolid, PrimitiveSolidKind,
    };
    use cadmpeg_ir::scalar::Length;

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#stale").expect("valid feature identity"),
        ordinal: 0,
        name: None,
        suppressed: None,
        needs_recompute: true,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        native_ref: None,
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Primitive {
                placement: None,
                solid: PrimitiveSolid::new(PrimitiveSolidKind::Box {
                    length: Dimension::Literal(Length::new(1.0).unwrap()),
                    width: Dimension::Literal(Length::new(2.0).unwrap()),
                    height: Dimension::Literal(Length::new(3.0).unwrap()),
                })
                .unwrap(),
                op: BooleanOp::NewBody,
            },
        )),
    });
    ir.finalize();
    let plan = StepCodec::default()
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("a stale evaluation still plans");
    assert!(
        plan.report().losses.iter().any(|loss| {
            loss.code.local_code() == "evaluation.stale"
                && loss.severity >= cadmpeg_ir::report::Severity::Warning
        }),
        "a stale feature must charge export:evaluation.stale: {:?}",
        plan.report().losses
    );
}

#[test]
fn native_arena_declarations_cover_the_decoded_input() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21")),
            &DecodeOptions::default(),
        )
        .expect("sheet fixture decodes");
    let plan = StepCodec::default()
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .expect("sheet plans an export");
    cadmpeg_test_support::contract::assert_declarations_cover_input(
        "step",
        decoded.ir(),
        plan.report(),
    );
}
