// SPDX-License-Identifier: Apache-2.0
//! Box placement conversion during source-less synthesis.

use cadmpeg_ir::features::{
    BooleanOp, Dimension, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation, FeatureRigidPlacement,
};
use cadmpeg_ir::scalar::PositiveLength;

#[test]
fn underflowed_box_placement_reports_loss_and_keeps_other_features() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let placement = |translation| {
        FeatureRigidPlacement::new(
            cadmpeg_ir::transform::Transform::affine([
                [1.0, 0.0, 0.0, translation],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .unwrap(),
        )
        .unwrap()
    };
    for (ordinal, translation) in [(0, 0.0), (1, f64::from_bits(1))] {
        ir.model.features.push(Feature {
            product_definition: None,
            id: FeatureId::mint(format!("authored:model:feature#{ordinal}")).unwrap(),
            ordinal,
            name: Some(format!("BoxPrimitive {}", ordinal + 1)),
            suppressed: Some(false),
            needs_recompute: false,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: Some("BoxPrimitive".into()),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Block {
                    dimensions: Some(
                        [2.0, 3.0, 4.0]
                            .map(|v| Dimension::Literal(PositiveLength::new(v).unwrap())),
                    ),
                    placement: Some(placement(translation)),
                    op: BooleanOp::NewBody,
                },
            )),
            native_ref: None,
        });
    }
    let (output, report) = super::super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.features.len(), 1);
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(ir.model.features[1].id.as_str())));
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains(ir.model.features[0].id.as_str())));
}
