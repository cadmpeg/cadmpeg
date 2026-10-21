// SPDX-License-Identifier: Apache-2.0
//! Active pattern refinements respect explicit definitions and scoped ownership.

use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed, PatternTransform};
use cadmpeg_ir::features::{
    ConfigurationFeatureState, Feature, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation,
};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::BTreeMap;

fn mirror(origin: f64) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds: vec![PatternSeed::Feature(
            FeatureId::mint("test:model:feature#seed").unwrap(),
        )],
        pattern: PatternKind::new(PatternTransform::Mirror {
            plane_origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(origin, 2.0, 3.0))
                .unwrap(),
            plane_normal: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                .unwrap(),
        })
        .unwrap(),
    })
}

fn document(definition: FeatureDefinition) -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let id = FeatureId::mint("test:model:feature#mirror").unwrap();
    ir.model.features.push(Feature {
        id: id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        needs_recompute: false,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(mirror(1.0)),
        native_ref: None,
    });
    let mut configuration = super::design_configuration("active", 0, Some(0), None);
    configuration.active = true;
    configuration.feature_states.insert(
        id,
        ConfigurationFeatureState::Definition {
            definition: Box::new(definition),
        },
    );
    ir.model.configurations.push(configuration);
    ir
}

#[test]
fn active_explicit_pattern_definition_keeps_its_own_plane() {
    let definition = mirror(9.0);
    let mut ir = document(definition.clone());
    super::super::normalize_configuration_differences(
        &cadmpeg_test_support::service_decode_context(),
        &mut ir,
    )
    .unwrap();
    assert_eq!(*ir.model.features[0].evaluation.definition(), definition);
    assert!(ir.model.configurations[0].feature_states.is_empty());
}

#[test]
fn active_scoped_pattern_keeps_its_unresolved_plane() {
    let definition = FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds: vec![PatternSeed::Feature(
            FeatureId::mint("test:model:feature#seed").unwrap(),
        )],
        pattern: PatternKind::UNRESOLVED,
    });
    let mut ir = document(definition.clone());
    let ctx = cadmpeg_test_support::service_decode_context();
    let lane = super::feature_input_lane("sldprt:native:feature-input-lane#scoped", Some("0"));
    let native = crate::native::SldprtNative {
        feature_input_lanes: vec![lane],
        ..Default::default()
    };
    native
        .store(&ctx, ir.native.namespace_mut("sldprt"))
        .unwrap();
    super::super::normalize_configuration_differences(&ctx, &mut ir).unwrap();
    assert_eq!(*ir.model.features[0].evaluation.definition(), definition);
    assert!(ir.model.configurations[0].feature_states.is_empty());
}

#[test]
fn active_unscoped_declared_mirror_inherits_its_late_plane() {
    let mut ir = document(FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds: Vec::new(),
        pattern: PatternKind::UNRESOLVED_MIRROR,
    }));
    super::super::normalize_configuration_differences(
        &cadmpeg_test_support::service_decode_context(),
        &mut ir,
    )
    .unwrap();
    assert_eq!(*ir.model.features[0].evaluation.definition(), mirror(1.0));
    assert!(ir.model.configurations[0].feature_states.is_empty());
}

#[test]
fn active_unscoped_declared_linear_does_not_inherit_a_mirror_plane() {
    let definition = FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds: vec![PatternSeed::Feature(
            FeatureId::mint("test:model:feature#seed").unwrap(),
        )],
        pattern: PatternKind::UNRESOLVED_LINEAR,
    });
    let mut ir = document(definition.clone());
    super::super::normalize_configuration_differences(
        &cadmpeg_test_support::service_decode_context(),
        &mut ir,
    )
    .unwrap();
    assert_eq!(*ir.model.features[0].evaluation.definition(), definition);
    assert!(ir.model.configurations[0].feature_states.is_empty());
}
