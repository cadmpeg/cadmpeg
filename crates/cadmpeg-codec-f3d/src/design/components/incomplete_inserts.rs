use std::collections::BTreeSet;

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation, InsertedComponents};
use cadmpeg_ir::products::Occurrence;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;

pub(crate) fn project_features(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    occurrences: &[Occurrence],
) -> Result<Vec<LossNote>, cadmpeg_core::CodecError> {
    let projected = occurrences
        .iter()
        .map(|occurrence| &occurrence.id)
        .collect::<BTreeSet<_>>();
    let mut losses = Vec::new();
    for scope in scopes {
        if scope.component_insert_construction().is_none()
            && scope.component_insert_placement().is_none()
        {
            continue;
        }
        let Some(feature) = features
            .iter_mut()
            .find(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        else {
            continue;
        };
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            continue;
        }
        let occurrence = crate::ids::neutral_component_insert_occurrence_id(scope);
        let definition = if projected.contains(&occurrence) {
            FeatureOperation::InsertComponent { occurrence }
        } else {
            FeatureOperation::InsertComponents {
                components: InsertedComponents::native(scope.id.clone())
                    .map_err(cadmpeg_core::CodecError::malformed)?,
            }
        };
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(definition));
        losses.push(F3dLossCode::FeatureComponentInsertMembershipAssumed.note(format!(
            "{} is represented as a component insertion with unresolved source membership; each placed scope supplies one unresolved occurrence, with per-scope membership assumed. Its native scope and decoded placement are retained (DR-487).", scope.id
        )));
    }
    Ok(losses)
}

#[cfg(test)]
mod tests;
