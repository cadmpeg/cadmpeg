// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::writer::tests::extrusion_authoring::source_less_extrusion;
use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, FeatureOperation};

#[test]
fn inactive_extrusion_operation_reaches_the_output() {
    use cadmpeg_ir::features::{ConfigurationFeatureState, ConfigurationId, DesignConfiguration};

    let mut ir = source_less_extrusion();
    let feature = &ir.model.features[0];
    let feature_id = feature.id.clone();
    let mut alternative = feature.evaluation.definition().clone();
    let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = &mut alternative
    else {
        panic!("extrusion");
    };
    *op = BooleanOp::Cut;
    let bodies = cadmpeg_ir::features::DistinctMembers::try_from(
        ir.model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    ir.model.configurations = (0..2)
        .map(|index| DesignConfiguration {
            id: ConfigurationId::mint(format!("synthetic:test:configuration#{index}")).unwrap(),
            ordinal: index,
            active: index == 0,
            source_index: Some(index),
            name: Some(format!("Config {index}")),
            material: None,
            properties: std::collections::BTreeMap::default(),
            parameter_overrides: std::collections::BTreeMap::default(),
            bodies: Some(bodies.clone()),
            parameter_values: std::collections::BTreeMap::default(),
            feature_states: std::collections::BTreeMap::default(),
            native_ref: None,
        })
        .collect();
    ir.model.configurations[1].feature_states.insert(
        feature_id.clone(),
        ConfigurationFeatureState::Definition {
            definition: Box::new(alternative),
        },
    );
    ir.model.finalize();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(
        !plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains(feature_id.as_str())
                && loss.message.contains("structural override")),
        "{:#?}",
        plan.report().losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let result = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(result.ir());
    let lane = native
        .feature_input_lanes
        .iter()
        .find(|lane| lane.configuration.as_deref() == Some("1"))
        .unwrap();
    let name = lane.names.iter().find(|name| name.value == "Boss").unwrap();
    let trailer = usize::try_from(name.offset).unwrap() + 6 + name.value.encode_utf16().count() * 2;
    assert_eq!(
        cadmpeg_core::decode::View::u16_le_at(
            &lane.native_payload,
            trailer + crate::layout::extrusion_sparse_operation_trailer::FAMILY
        ),
        Some(0x01ca)
    );
    let owner = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    let config = result
        .ir()
        .model
        .configurations
        .iter()
        .find(|config| config.name.as_deref() == Some("Config 1"))
        .unwrap();
    assert!(matches!(
        owner.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            op: BooleanOp::Join,
            ..
        })
    ));
    assert!(
        matches!(
            config
                .feature_states
                .get(&owner.id)
                .and_then(ConfigurationFeatureState::definition),
            Some(FeatureDefinition::Operation(FeatureOperation::Extrude {
                op: BooleanOp::Cut,
                ..
            }))
        ),
        "configuration states: {:#?}; owner: {:#?}; lanes: {:#?}; histories: {:#?}",
        config.feature_states,
        owner.evaluation,
        native
            .feature_input_lanes
            .iter()
            .map(|lane| (&lane.configuration, &lane.names, &lane.classes))
            .collect::<Vec<_>>(),
        native.feature_histories
    );
}
