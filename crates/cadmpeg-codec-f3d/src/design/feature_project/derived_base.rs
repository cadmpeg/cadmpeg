// SPDX-License-Identifier: Apache-2.0

use crate::records::bodies::DesignBodyBinding;
use crate::records::feature::derived_base::feature_derived_base_scopes;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};

pub(super) fn project(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    bindings: &[DesignBodyBinding],
) -> Result<(), CodecError> {
    for feature in features {
        let Some(group) = feature_derived_base_scopes(feature, scopes) else {
            continue;
        };
        let [scope] = group.as_slice() else { continue };
        let Some(source) = scope.derived_base_source.as_ref() else {
            continue;
        };
        let (Some(snapshot), Some(current)) = (source.snapshot.as_ref(), scope.history_state_id())
        else {
            continue;
        };
        let prefix = crate::ids::feature_input_prefix(&feature.id, current);
        let bodies = BodySelection::Historical {
            state: crate::ids::history_input_state_id(&prefix),
            bodies: vec![crate::ids::history_input_body_id(
                &prefix,
                snapshot.body_slot,
            )]
            .try_into()
            .map_err(CodecError::malformed)?,
            native: scope.id.clone().try_into().map_err(CodecError::malformed)?,
        };
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::BaseFeature { bodies },
            ));
        let stream = crate::ids::native_stream(&scope.id);
        for (suffix, key) in [
            (
                source.source_body.value,
                cadmpeg_core::nonblank_literal!("derived_base:source_binding"),
            ),
            (
                source.result_body.value,
                cadmpeg_core::nonblank_literal!("derived_base:result_binding"),
            ),
        ] {
            if let Some(binding) = bindings.iter().find(|binding| {
                crate::ids::native_stream(binding.id()) == stream && binding.entity_suffix == suffix
            }) {
                feature.source_properties.insert(key, binding.id().clone());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
