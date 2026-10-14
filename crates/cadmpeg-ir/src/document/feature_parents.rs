// SPDX-License-Identifier: Apache-2.0
//! Structural ownership and predecessor ordering for feature relations.

use std::collections::HashMap;

use super::Model;
use crate::features::{FeatureDefinition, FeatureId, FeatureOperation};

/// A rejected parent relation expressed through borrowed model identities.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ParentError<'a> {
    #[error("feature identity `{owner}` is repeated")]
    Repeated { owner: &'a FeatureId },
    #[error("feature `{owner}` has two tree parents `{first}` and `{second}`")]
    TreeParents {
        owner: &'a FeatureId,
        first: &'a FeatureId,
        second: &'a FeatureId,
    },
    #[error("feature `{owner}` has two regeneration parent entries `{first}` and `{second}`")]
    RegenerationParents {
        owner: &'a FeatureId,
        first: &'a FeatureId,
        second: &'a FeatureId,
    },
    #[error("regeneration relation names missing child feature `{owner}`")]
    MissingChild { owner: &'a FeatureId },
    #[error("tree child `{owner}` is owned by `{parent}` and states no regeneration parent")]
    TreeChild {
        owner: &'a FeatureId,
        parent: &'a FeatureId,
    },
    #[error("feature `{owner}` names missing regeneration parent `{parent}`")]
    MissingParent {
        owner: &'a FeatureId,
        parent: &'a FeatureId,
    },
    #[error("regeneration parent `{parent}` does not precede child `{owner}`")]
    Order {
        owner: &'a FeatureId,
        parent: &'a FeatureId,
    },
}

impl ParentError<'_> {
    pub(crate) fn owner(&self) -> &FeatureId {
        match self {
            Self::Repeated { owner }
            | Self::TreeParents { owner, .. }
            | Self::RegenerationParents { owner, .. }
            | Self::MissingChild { owner }
            | Self::TreeChild { owner, .. }
            | Self::MissingParent { owner, .. }
            | Self::Order { owner, .. } => owner,
        }
    }
}

/// Check the parent relations of the supplied models.
pub(crate) fn validate<'a>(models: &[&'a Model]) -> Result<(), ParentError<'a>> {
    let mut features = HashMap::new();
    for feature in models.iter().flat_map(|model| &model.features) {
        if insert_first(&mut features, &feature.id, feature).is_some() {
            return Err(ParentError::Repeated { owner: &feature.id });
        }
    }
    let mut tree_parents = HashMap::new();
    for parent in models.iter().flat_map(|model| &model.features) {
        let FeatureDefinition::Operation(FeatureOperation::TreeNode { children, .. }) =
            parent.evaluation.definition()
        else {
            continue;
        };
        for child in children {
            if let Some(previous) = insert_first(&mut tree_parents, child, &parent.id) {
                return Err(ParentError::TreeParents {
                    owner: child,
                    first: previous,
                    second: &parent.id,
                });
            }
        }
    }
    let mut regeneration_parents = HashMap::new();
    for (child, parent) in models
        .iter()
        .flat_map(|model| &model.feature_regeneration_parents.0)
    {
        if let Some(previous) = insert_first(&mut regeneration_parents, child, parent) {
            return Err(ParentError::RegenerationParents {
                owner: child,
                first: previous,
                second: parent,
            });
        }
        let Some(child_feature) = features.get(child) else {
            return Err(ParentError::MissingChild { owner: child });
        };
        if let Some(existing) = tree_parents.get(child) {
            return Err(ParentError::TreeChild {
                owner: child,
                parent: existing,
            });
        }
        let Some(parent_feature) = features.get(parent) else {
            return Err(ParentError::MissingParent {
                owner: child,
                parent,
            });
        };
        if parent_feature.ordinal >= child_feature.ordinal {
            return Err(ParentError::Order {
                owner: child,
                parent,
            });
        }
    }
    Ok(())
}

/// Insert `value` for an absent `id`; return the value already present otherwise.
fn insert_first<'a, T: Copy>(
    facts: &mut HashMap<&'a FeatureId, T>,
    id: &'a FeatureId,
    value: T,
) -> Option<T> {
    match facts.entry(id) {
        std::collections::hash_map::Entry::Occupied(entry) => Some(*entry.get()),
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(value);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{validate, FeatureDefinition, FeatureOperation, Model};

    fn model() -> Model {
        let mut model = Model::default();
        model.features.push(crate::features::Feature {
            id: "test:parents:feature#one".try_into().unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: crate::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: crate::features::FeatureContent::default(),
            evaluation: crate::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            ),
            native_ref: None,
        });
        model
    }

    #[test]
    fn feature_parent_validation_accepts_one_feature_and_serde_reconstruction() {
        let model = model();
        let wire = serde_json::to_value(&model).unwrap();
        validate(&[&model]).unwrap();
        assert_eq!(serde_json::from_value::<Model>(wire).unwrap(), model);
    }
}
