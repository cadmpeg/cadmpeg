// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{FeatureDefinition, FeatureEvaluation, FeatureOperation};

use super::annotation;
use crate::validate::tagging_definition_matches;

#[test]
fn tagging_validation_requires_the_native_definition_without_a_neutral_result() {
    let mut feature = annotation("inventor:pmdc:feature#generated-0".into());
    feature.evaluation =
        FeatureEvaluation::from_definition(crate::feature::tagging::definition(true));
    assert!(tagging_definition_matches(&feature, false, true));
    assert!(!tagging_definition_matches(&feature, true, true));
    for definition in [
        FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::History,
            children: cadmpeg_ir::features::TreeChildren::default(),
        }),
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other("Base".into()),
            parameters: std::collections::BTreeMap::new(),
        }),
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other("Tagging".into()),
            parameters: std::collections::BTreeMap::from([(
                cadmpeg_core::text::NonBlankString::new("count").expect("key"),
                "1".into(),
            )]),
        }),
    ] {
        feature.evaluation = FeatureEvaluation::from_definition(definition);
        for has_result in [false, true] {
            assert!(!tagging_definition_matches(&feature, has_result, true));
        }
    }
}

#[test]
fn tagging_validation_keeps_ambiguous_owners_native() {
    let mut feature = annotation("inventor:pmdc:feature#generated-0".into());
    feature.evaluation =
        FeatureEvaluation::from_definition(crate::feature::tagging::definition(false));
    assert!(tagging_definition_matches(&feature, false, false));
    assert!(!tagging_definition_matches(&feature, true, false));
    assert!(!tagging_definition_matches(&feature, false, true));
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::History,
            children: cadmpeg_ir::features::TreeChildren::default(),
        },
    ));
    assert!(!tagging_definition_matches(&feature, false, false));
}
