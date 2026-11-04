// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::feature_projection::{
    non_boolean_feature_definition_with_parameters, HoleProjection,
};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use std::collections::BTreeMap;

#[test]
fn sweep_preserves_native_parameters_without_defaulting_unproved_policies() {
    let parameters = BTreeMap::from([(cadmpeg_core::nonblank_literal!("p7"), "23".to_string())]);
    let definition = non_boolean_feature_definition_with_parameters(
        "SWP104",
        &[],
        None,
        None,
        HoleProjection::default(),
        parameters.clone(),
    )
    .unwrap();
    assert_eq!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: "SWP104".into(),
            parameters,
        })
    );
}
