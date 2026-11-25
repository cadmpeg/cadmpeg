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

/// Drops each parent relation of `model` that [`validate`] refuses, given
/// that `model` states each feature id once: each tree child of a node after
/// the first node in arena order that lists it, and each regeneration edge
/// whose child or parent is missing, whose child has a tree parent, or whose
/// parent does not precede its child. Returns the child of each dropped
/// relation with what the drop removed.
pub(crate) fn release_invalid(model: &mut Model) -> Vec<(FeatureId, String)> {
    let mut released = Vec::new();
    let mut tree_parents: HashMap<FeatureId, FeatureId> = HashMap::new();
    for node in &mut model.features {
        let node_id = &node.id;
        node.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::TreeNode { children, .. }) =
                definition
            else {
                return;
            };
            children.retain(|child| {
                if let Some(first) = tree_parents.get(child) {
                    released.push((
                        child.clone(),
                        format!(
                            "drops feature `{child}` from the children of tree node \
                             `{node_id}`: tree node `{first}` lists it first"
                        ),
                    ));
                    return false;
                }
                tree_parents.insert(child.clone(), node_id.clone());
                true
            });
        });
    }
    let mut ordinals = HashMap::new();
    for feature in &model.features {
        ordinals.entry(&feature.id).or_insert(feature.ordinal);
    }
    model
        .feature_regeneration_parents
        .0
        .retain(|child, parent| {
            let why = match (ordinals.get(child), ordinals.get(parent)) {
                (None, _) => "names a missing child".to_owned(),
                _ if tree_parents.contains_key(child) => {
                    format!("the child has tree parent `{}`", tree_parents[child])
                }
                (Some(_), None) => "names a missing parent".to_owned(),
                (Some(child_ordinal), Some(parent_ordinal)) if parent_ordinal >= child_ordinal => {
                    "the parent does not precede the child".to_owned()
                }
                (Some(_), Some(_)) => return true,
            };
            released.push((
                child.clone(),
                format!("drops regeneration parent `{parent}` of feature `{child}`: {why}"),
            ));
            false
        });
    released
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
            product_definition: None,
            id: "test:parents:feature#one".try_into().unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: crate::features::selections::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: crate::features::FeatureContent::default(),
            evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            ),
            native_ref: None,
        });
        model
    }

    #[test]
    fn feature_parent_validation_accepts_one_feature() {
        let model = model();
        validate(&[&model]).unwrap();
        assert!(serde_json::to_value(&model)
            .unwrap()
            .get("features")
            .is_none());
    }
}
