// SPDX-License-Identifier: Apache-2.0
//! Resolve native occurrence paths to expanded local instances.

use std::collections::BTreeMap;

use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{Occurrence, OccurrenceParent};

use crate::records::feature::assembly_features::DesignComponentOccurrence;

#[derive(Default)]
pub(crate) struct LocalOccurrencePaths {
    paths: BTreeMap<(String, Vec<String>), Option<OccurrenceId>>,
}

impl LocalOccurrencePaths {
    pub(crate) fn new(native: &[DesignComponentOccurrence], occurrences: &[Occurrence]) -> Self {
        let carriers = native
            .iter()
            .map(|carrier| (carrier.id.as_str(), carrier))
            .collect::<BTreeMap<_, _>>();
        let instances = occurrences
            .iter()
            .map(|occurrence| (&occurrence.id, occurrence))
            .collect::<BTreeMap<_, _>>();
        let mut result = Self::default();
        for leaf in occurrences {
            let mut keys = Vec::new();
            let mut current = leaf;
            let mut stream = None;
            for _ in 0..=occurrences.len() {
                if let Some(carrier) = current
                    .native_ref
                    .as_deref()
                    .and_then(|id| carriers.get(id))
                {
                    stream = crate::ids::native_stream(&carrier.id);
                    keys.push(carrier.occurrence_guid.as_str().to_ascii_lowercase());
                }
                let OccurrenceParent::Occurrence { occurrence: parent } = &current.parent else {
                    break;
                };
                let Some(parent) = instances.get(parent) else {
                    break;
                };
                current = parent;
            }
            let Some(stream) = stream else {
                continue;
            };
            keys.reverse();
            if keys.is_empty() {
                continue;
            }
            if keys.len() > 1 {
                result.insert(
                    stream,
                    vec![keys.last().cloned().unwrap_or_default()],
                    &leaf.id,
                );
            }
            result.insert(stream, keys, &leaf.id);
        }
        result
    }

    fn insert(&mut self, stream: &str, keys: Vec<String>, id: &OccurrenceId) {
        self.paths
            .entry((stream.to_owned(), keys))
            .and_modify(|value| {
                if value.as_ref() != Some(id) {
                    *value = None;
                }
            })
            .or_insert_with(|| Some(id.clone()));
    }

    pub(crate) fn ambiguous(&self, stream: &str, keys: &[String]) -> bool {
        self.paths
            .get(&(stream.to_owned(), keys.to_vec()))
            .is_some_and(Option::is_none)
    }

    pub(crate) fn resolve_id(&self, stream: &str, id: &OccurrenceId) -> Option<OccurrenceId> {
        let key = cadmpeg_ir::ids::Identity::from(id.clone()).key();
        self.resolve(stream, &[key.as_str().to_owned()])
            .map(|(id, _)| id)
    }

    pub(crate) fn resolve(&self, stream: &str, keys: &[String]) -> Option<(OccurrenceId, usize)> {
        for count in (1..=keys.len()).rev() {
            if let Some(Some(id)) = self.paths.get(&(stream.to_owned(), keys[..count].to_vec())) {
                return Some((id.clone(), count));
            }
        }
        None
    }
}

/// Bind feature occurrence references after local instance expansion.
pub(crate) fn bind_feature_paths(
    features: &mut [cadmpeg_ir::features::Feature],
    paths: &LocalOccurrencePaths,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let mut losses = Vec::new();
    for feature in features {
        let Some(stream) = feature
            .native_ref
            .as_deref()
            .and_then(crate::ids::native_stream)
        else {
            continue;
        };
        let mut definition = feature.evaluation.definition().clone();
        let resolve = |id: &OccurrenceId| {
            let key = cadmpeg_ir::ids::Identity::from(id.clone()).key();
            if paths
                .paths
                .contains_key(&(stream.to_owned(), vec![key.as_str().to_owned()]))
            {
                paths.resolve_id(stream, id)
            } else {
                Some(id.clone())
            }
        };
        let resolved = match &mut definition {
            FeatureDefinition::Operation(FeatureOperation::InsertComponent { occurrence }) => {
                resolve(occurrence)
                    .map(|id| {
                        *occurrence = id;
                    })
                    .is_some()
            }
            FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) => {
                seeds.iter_mut().all(|seed| {
                    let PatternSeed::Occurrences(ids) = seed else {
                        return true;
                    };
                    let Some(mapped) = ids.iter().map(resolve).collect::<Option<Vec<_>>>() else {
                        return false;
                    };
                    let Ok(mapped) = mapped.try_into() else {
                        return false;
                    };
                    *ids = mapped;
                    true
                })
            }
            _ => continue,
        };
        if !resolved {
            definition = FeatureDefinition::Operation(FeatureOperation::Native {
                kind: feature
                    .source_tag
                    .clone()
                    .unwrap_or_else(|| "ComponentOperation".to_owned())
                    .into(),
                parameters: BTreeMap::new(),
            });
            losses.push(
                crate::loss::F3dLossCode::FeatureOccurrencePathUnresolved
                    .note("a component feature has no unique expanded occurrence path"),
            );
        }
        feature.evaluation.set_definition(definition);
    }
    losses
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};

    #[test]
    fn an_ambiguous_feature_occurrence_retains_the_native_operation_with_a_loss() {
        let id = OccurrenceId::mint("f3d:model:occurrence#34").unwrap();
        let paths = LocalOccurrencePaths {
            paths: BTreeMap::from([(
                (
                    "f3d:Design/BulkStream.dat".to_owned(),
                    vec!["34".to_owned()],
                ),
                None,
            )]),
        };
        let mut feature = Feature {
            id: cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#copy").unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: Some("CopyPaste".to_owned()),
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::InsertComponent { occurrence: id }),
            ),
            native_ref: Some("f3d:Design/BulkStream.dat:design-parameter-scope#1".to_owned()),
        };
        let losses = bind_feature_paths(std::slice::from_mut(&mut feature), &paths);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::FeatureOccurrencePathUnresolved.kind()
        );
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
    }
}
