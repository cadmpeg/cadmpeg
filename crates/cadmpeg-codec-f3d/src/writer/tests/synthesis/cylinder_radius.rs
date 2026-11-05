// SPDX-License-Identifier: Apache-2.0
//! Radius-form cylinders in source-less synthesis.

use cadmpeg_ir::features::{
    BooleanOp, Dimension, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation, PrimitiveSolid, PrimitiveSolidKind, RadialDimension,
};
use cadmpeg_ir::scalar::{Angle, Length};

#[test]
fn literal_radius_keeps_cylinder_geometry_and_reports_form_change() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let id = FeatureId::mint("authored:model:feature#0").unwrap();
    ir.model.features.push(Feature {
        product_definition: None,
        id: id.clone(),
        ordinal: 0,
        name: Some("CylinderPrimitive 1".into()),
        suppressed: Some(false),
        needs_recompute: false,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("CylinderPrimitive".into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Primitive {
                solid: PrimitiveSolid::new(PrimitiveSolidKind::Cylinder {
                    radius: RadialDimension::Radius(Dimension::Literal(Length::new(3.0).unwrap())),
                    height: Dimension::Literal(Length::new(10.0).unwrap()),
                    angle: Angle::FULL_TURN.into(),
                })
                .unwrap(),
                placement: None,
                op: BooleanOp::NewBody,
            },
        )),
        native_ref: None,
    });
    let (output, report) = super::super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.features.len(), 1);
    let FeatureDefinition::Operation(FeatureOperation::Primitive { solid, .. }) =
        output.ir().model.features[0].evaluation.definition()
    else {
        panic!("expected cylinder");
    };
    let PrimitiveSolidKind::Cylinder {
        radius: RadialDimension::Diameter(diameter),
        ..
    } = solid.kind()
    else {
        panic!("expected diameter");
    };
    assert_eq!(output.ir().dimension_value(diameter).unwrap().get(), 6.0);
    assert!(report.losses.iter().any(|loss| {
        loss.message.contains(id.as_str()) && loss.message.contains("radius as a diameter")
    }));
    ir.model.features[0].evaluation = FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Primitive {
            solid: PrimitiveSolid::new(PrimitiveSolidKind::Cylinder {
                radius: RadialDimension::Radius(Dimension::Literal(Length::new(1.0e308).unwrap())),
                height: Dimension::Literal(Length::new(10.0).unwrap()),
                angle: Angle::FULL_TURN.into(),
            })
            .unwrap(),
            placement: None,
            op: BooleanOp::NewBody,
        }),
    );
    let (output, report) = super::super::source_less_analytic::synthesize_document(&ir, None);
    assert!(output.ir().model.features.is_empty());
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str())));
}
