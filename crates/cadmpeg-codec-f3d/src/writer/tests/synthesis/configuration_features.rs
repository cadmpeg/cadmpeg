// SPDX-License-Identifier: Apache-2.0
//! Configurations referencing synthesized feature scopes.

use cadmpeg_ir::features::{
    BooleanOp, ConfigurationFeatureState, DesignConfiguration, Dimension, Feature, FeatureContent,
    FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation, FeatureRigidPlacement,
};
use cadmpeg_ir::scalar::PositiveLength;

#[test]
fn inactive_configuration_suppresses_authored_box() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let feature_id = FeatureId::mint("authored:model:feature#0").unwrap();
    ir.model.features.push(Feature {
        product_definition: None,
        id: feature_id.clone(),
        ordinal: 0,
        name: Some("BoxPrimitive 1".into()),
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
                    [2.0, 3.0, 4.0].map(|v| Dimension::Literal(PositiveLength::new(v).unwrap())),
                ),
                placement: Some(FeatureRigidPlacement::identity()),
                op: BooleanOp::NewBody,
            },
        )),
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: "authored:model:configuration#0".try_into().unwrap(),
        ordinal: 0,
        active: false,
        source_index: None,
        name: Some("Alternate".into()),
        material: None,
        properties: Default::default(),
        parameter_overrides: Default::default(),
        parameter_values: Default::default(),
        feature_states: [(feature_id, ConfigurationFeatureState::Suppressed {})].into(),
        bodies: None,
        native_ref: None,
    });
    let (output, report) = super::super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.features.len(), 1);
    assert_eq!(output.ir().model.configurations.len(), 1);
    let feature = &output.ir().model.features[0];
    let configuration = &output.ir().model.configurations[0];
    assert!(matches!(
        configuration.feature_states.get(&feature.id),
        Some(ConfigurationFeatureState::Suppressed {})
    ));
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("suppression")));
    ir.model.features[0].source_tag = Some("unsupported".into());
    let (output, report) = super::super::source_less_analytic::synthesize_document(&ir, None);
    assert!(output.ir().model.features.is_empty());
    assert!(report.losses.iter().any(|loss| {
        loss.message
            .contains(ir.model.configurations[0].id.as_str())
            && loss.message.contains(ir.model.features[0].id.as_str())
    }));
}
