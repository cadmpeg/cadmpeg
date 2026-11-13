// SPDX-License-Identifier: Apache-2.0
//! Resolve native occurrence paths to expanded local instances.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{Occurrence, OccurrenceParent};

use crate::records::feature::assembly_features::DesignComponentOccurrence;
use crate::records::feature::scope::DesignParameterScope;

#[derive(Default)]
pub(crate) struct LocalOccurrencePaths {
    paths: BTreeMap<(String, Vec<String>), Option<OccurrenceId>>,
    unplaced_external: Vec<String>,
    external_occurrences: BTreeSet<OccurrenceId>,
}

impl LocalOccurrencePaths {
    /// Paths over local and external occurrences, with each `Component
    /// Insert` occurrence keyed by the source occurrence key of its carrier.
    pub(crate) fn new(
        native: &[DesignComponentOccurrence],
        external: &[crate::records::xref::XrefReference],
        occurrences: &[Occurrence],
        scopes: &[DesignParameterScope],
    ) -> Self {
        let inserts = scopes
            .iter()
            .filter_map(|scope| {
                let key = scope
                    .component_insert_construction()?
                    .occurrence_key
                    .as_ref()?;
                Some((
                    scope.id.as_str(),
                    (crate::ids::native_stream(&scope.id)?, key.value.as_str()),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let unplaced_external = external
            .iter()
            .filter(|reference| !crate::xref::has_projected_placement(reference, occurrences))
            .map(|reference| reference.id.clone())
            .collect();
        let external_ids = external
            .iter()
            .map(|reference| reference.id.as_str())
            .collect::<BTreeSet<_>>();
        let external_occurrences = occurrences
            .iter()
            .filter(|occurrence| {
                occurrence
                    .native_ref
                    .as_deref()
                    .is_some_and(|id| external_ids.contains(id))
            })
            .map(|occurrence| occurrence.id.clone())
            .collect();
        let carriers = native
            .iter()
            .map(|carrier| (carrier.id.as_str(), carrier))
            .collect::<BTreeMap<_, _>>();
        let external = external
            .iter()
            .filter_map(|reference| {
                let source = reference.placement_source.as_ref()?;
                let guid = source.occurrence_guid.as_ref()?;
                Some((reference.id.as_str(), (source.stream.as_str(), guid)))
            })
            .collect::<BTreeMap<_, _>>();
        let instances = occurrences
            .iter()
            .map(|occurrence| (&occurrence.id, occurrence))
            .collect::<BTreeMap<_, _>>();
        let mut result = Self {
            unplaced_external,
            external_occurrences,
            ..Self::default()
        };
        for leaf in occurrences {
            let mut keys = Vec::new();
            let mut current = leaf;
            let mut stream = None;
            for _ in 0..=occurrences.len() {
                let identity = current.native_ref.as_deref().and_then(|id| {
                    carriers
                        .get(id)
                        .and_then(|carrier| {
                            crate::ids::native_stream(&carrier.id)
                                .map(|stream| (stream, carrier.occurrence_guid.as_str()))
                        })
                        .or_else(|| {
                            external
                                .get(id)
                                .map(|(stream, guid)| (*stream, guid.as_str()))
                        })
                        .or_else(|| inserts.get(id).copied())
                });
                if let Some((source_stream, key)) = identity {
                    if stream.is_some_and(|stream| stream != source_stream) {
                        break;
                    }
                    stream = Some(source_stream);
                    keys.push(key.to_ascii_lowercase());
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
            // An operand path starts at an occurrence of the component that
            // stores it, so every chain suffix ending at this leaf selects it.
            for start in 1..keys.len() {
                result.insert(stream, keys[start..].to_vec(), &leaf.id);
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

    pub(crate) fn requires_external_binding(&self, stream: &str) -> bool {
        self.unplaced_external
            .iter()
            .any(|reference| crate::ids::same_native_occurrence(reference, stream))
    }

    /// Keys left after an external occurrence continue inside its member, which
    /// the archive merge binds. Keys left after a local occurrence can name a
    /// reference that this document omitted.
    pub(crate) fn refuses_residual_keys(&self, stream: &str, occurrence: &OccurrenceId) -> bool {
        self.requires_external_binding(stream) && !self.external_occurrences.contains(occurrence)
    }

    pub(crate) fn ambiguous(&self, stream: &str, keys: &[String]) -> bool {
        (1..=keys.len()).any(|count| {
            self.paths
                .get(&(stream.to_owned(), keys[..count].to_vec()))
                .is_some_and(Option::is_none)
        })
    }

    pub(crate) fn resolve_id(&self, stream: &str, id: &OccurrenceId) -> Option<OccurrenceId> {
        let key = cadmpeg_ir::ids::Identity::from(id.clone()).key();
        self.resolve(stream, &[key.as_str().to_owned()])
            .map(|(id, _)| id)
    }

    pub(crate) fn resolve(&self, stream: &str, keys: &[String]) -> Option<(OccurrenceId, usize)> {
        for count in (1..=keys.len()).rev() {
            if let Some(id) = self.paths.get(&(stream.to_owned(), keys[..count].to_vec())) {
                return id.clone().map(|id| (id, count));
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
            FeatureDefinition::Operation(FeatureOperation::InsertComponents { components }) => {
                let mut seen = BTreeSet::new();
                let mapped = components
                    .occurrences()
                    .iter()
                    .filter_map(resolve)
                    .filter(|id| seen.insert(id.clone()))
                    .collect::<Vec<_>>();
                let mapping_complete = mapped.len() == components.occurrences().len();
                let complete = components.is_resolved() && mapping_complete;
                let native = components.native_reference().clone();
                *components = match cadmpeg_ir::features::SelectionMembers::try_from(mapped) {
                    Ok(occurrences) if complete => {
                        cadmpeg_ir::features::InsertedComponents::Resolved {
                            occurrences,
                            native,
                        }
                    }
                    Ok(occurrences) => cadmpeg_ir::features::InsertedComponents::Partial {
                        occurrences,
                        native,
                    },
                    Err(_) => cadmpeg_ir::features::InsertedComponents::Native { native },
                };
                if !mapping_complete {
                    losses.push(crate::loss::F3dLossCode::FeatureOccurrencePathUnresolved.note(
                        "a component insertion retains native membership for occurrence paths without a unique expanded target",
                    ));
                }
                true
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
    fn a_component_insert_occurrence_resolves_by_its_carrier_occurrence_key() {
        use crate::records::feature::assembly_features::DesignComponentInsertConstruction;
        use crate::records::feature::scope::{DesignFeatureKind, DesignScopePayloadMut};
        let stream = "f3d:Design/BulkStream.dat";
        let key = "bc7c305e-42cc-4c2d-a213-063506349b08";
        let nested = "d7dd44c5-9574-4a6d-9dd2-dc82af142924";
        let scope_id = format!("{stream}:design-parameter-scope#100");
        let mut scope =
            DesignParameterScope::empty(&scope_id, DesignFeatureKind::ComponentInsert, 9);
        if let DesignScopePayloadMut::ComponentInsert(slot) = scope.payload_mut() {
            *slot = Some(DesignComponentInsertConstruction {
                relation_record_index: 8,
                carrier_record_index: 7,
                occurrence_identity: Some(17),
                neutron_role: "cccccccc-dddd-eeee-ffff-000000000000".into(),
                neutron_role_offset: 259,
                occurrence_key: Some(crate::records::identity::Located {
                    value: key.to_ascii_uppercase().try_into().unwrap(),
                    offset: 40,
                }),
                placement: None,
            });
        }
        let id = OccurrenceId::mint("f3d:model:occurrence#insert").unwrap();
        let occurrence = Occurrence {
            id: id.clone(),
            prototype: cadmpeg_ir::products::PrototypeReference::Unresolved {},
            parent: OccurrenceParent::Root {},
            ordinal: 0,
            transform: cadmpeg_ir::transform::Transform::identity(),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: Some(scope_id),
        };
        let keys = [key.to_owned(), nested.to_owned()];
        let paths = LocalOccurrencePaths::new(
            &[],
            &[],
            std::slice::from_ref(&occurrence),
            std::slice::from_ref(&scope),
        );
        assert_eq!(paths.resolve(stream, &keys), Some((id, 1)));
        assert_eq!(
            LocalOccurrencePaths::new(&[], &[], std::slice::from_ref(&occurrence), &[])
                .resolve(stream, &keys),
            None
        );
    }

    #[test]
    fn residual_keys_are_refused_after_a_local_occurrence_only() {
        let local = OccurrenceId::mint("f3d:model:occurrence#local").unwrap();
        let external = OccurrenceId::mint("f3d:model:occurrence#external").unwrap();
        let stream = "f3d:Design/BulkStream.dat";
        let paths = LocalOccurrencePaths {
            unplaced_external: vec!["f3d:xref:reference#1".to_owned()],
            external_occurrences: BTreeSet::from([external.clone()]),
            paths: BTreeMap::new(),
        };
        assert!(paths.refuses_residual_keys(stream, &local));
        assert!(!paths.refuses_residual_keys(stream, &external));
        assert!(!LocalOccurrencePaths::default().refuses_residual_keys(stream, &local));
    }

    #[test]
    fn component_set_paths_keep_known_members_when_another_path_is_ambiguous() {
        use cadmpeg_ir::features::InsertedComponents;
        let stream = "f3d:Design/BulkStream.dat";
        let first = OccurrenceId::mint("f3d:model:occurrence#34").unwrap();
        let second = OccurrenceId::mint("f3d:model:occurrence#35").unwrap();
        let mapped = OccurrenceId::mint("f3d:model:occurrence#nested").unwrap();
        let paths = LocalOccurrencePaths {
            paths: BTreeMap::from([
                (
                    (stream.to_owned(), vec!["34".to_owned()]),
                    Some(mapped.clone()),
                ),
                ((stream.to_owned(), vec!["35".to_owned()]), None),
            ]),
            ..Default::default()
        };
        let mut feature = Feature {
            product_definition: None,
            id: cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#derive").unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: Some("DerivedInstance".into()),
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::InsertComponents {
                    components: InsertedComponents::resolved(
                        vec![first, second],
                        "fixture:scope#10".into(),
                    )
                    .unwrap(),
                }),
            ),
            native_ref: Some(format!("{stream}:design-parameter-scope#1")),
        };
        let losses = bind_feature_paths(std::slice::from_mut(&mut feature), &paths);
        assert!(
            matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::InsertComponents {
            components: InsertedComponents::Partial { occurrences, native }
        }) if occurrences.as_slice() == [mapped] && native.as_str() == "fixture:scope#10")
        );
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::FeatureOccurrencePathUnresolved.kind()
        );
    }

    #[test]
    fn an_ambiguous_feature_occurrence_retains_the_native_operation_with_a_loss() {
        let id = OccurrenceId::mint("f3d:model:occurrence#34").unwrap();
        let paths = LocalOccurrencePaths {
            unplaced_external: Vec::new(),
            external_occurrences: BTreeSet::new(),
            paths: BTreeMap::from([(
                (
                    "f3d:Design/BulkStream.dat".to_owned(),
                    vec!["34".to_owned()],
                ),
                None,
            )]),
        };
        let mut feature = Feature {
            product_definition: None,
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
    #[test]
    fn external_occurrence_keys_select_repeated_targets_in_their_source_stream() {
        let key_a = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        let key_b = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
        let sources = [
            ("f3d:Design/BulkStream.dat", key_a),
            ("f3d:Design/BulkStream.dat", key_b),
            ("f3d:OtherDesign/BulkStream.dat", key_a),
        ];
        let table = crate::xref::XrefTable {
            references: sources
                .iter()
                .enumerate()
                .map(|(index, (stream, key))| {
                    serde_json::from_value(serde_json::json!({
                        "id":format!("f3d:xref:reference#{index}"), "ordinal":index,
                        "from":"root.f3d", "relative_path":"part.f3d",
                        "neutron_role":"same-role", "neutron_data":"",
                        "placement_source": {
                            "stream":stream, "carrier_record_index":100 + index,
                            "occurrence_guid":key.to_ascii_uppercase()
                        }
                    }))
                    .unwrap()
                })
                .collect(),
            ..Default::default()
        };
        let occurrences = table.references.iter().map(|reference| {
            serde_json::from_value::<Occurrence>(serde_json::json!({
                "id": crate::ids::neutral_xref_occurrence_id(reference.ordinal, 0),
                "prototype": {"scope":"unresolved"}, "parent": {"kind":"root"},
                "ordinal": reference.ordinal, "transform": cadmpeg_ir::transform::Transform::identity(),
                "scale": [1.0, 1.0, 1.0], "native_ref": reference.id
            })).unwrap()
        }).collect::<Vec<_>>();
        let paths = LocalOccurrencePaths::new(&[], &table.references, &occurrences, &[]);
        for ((stream, key), occurrence) in sources.iter().zip(&occurrences) {
            assert_eq!(
                paths.resolve(stream, &[(*key).to_owned()]),
                Some((occurrence.id.clone(), 1))
            );
        }
        assert!(paths
            .resolve("f3d:OtherDesign/BulkStream.dat", &[key_b.to_owned()])
            .is_none());
        assert!(paths
            .resolve("f3d:Design/BulkStream.dat", &["same-role".to_owned()])
            .is_none());
        let mut nested = occurrences.clone();
        nested[1].parent = OccurrenceParent::Occurrence {
            occurrence: nested[2].id.clone(),
        };
        let nested_paths = LocalOccurrencePaths::new(&[], &table.references, &nested, &[]);
        assert_eq!(
            nested_paths.resolve(sources[1].0, &[key_b.to_owned()]),
            Some((nested[1].id.clone(), 1))
        );
        assert!(nested_paths
            .resolve(sources[2].0, &[key_b.to_owned()])
            .is_none());
        let key_c = "cccccccc-cccc-cccc-cccc-cccccccccccc";
        let mut table = table;
        table.references[2] = serde_json::from_value(serde_json::json!({
            "id":"f3d:xref:reference#2", "ordinal":2,
            "from":"root.f3d", "relative_path":"part.f3d",
            "neutron_role":"same-role", "neutron_data":"",
            "placement_source": {
                "stream":sources[0].0, "carrier_record_index":102,
                "occurrence_guid":key_c.to_ascii_uppercase()
            }
        }))
        .unwrap();
        let mut chain = occurrences.clone();
        chain[1].parent = OccurrenceParent::Occurrence {
            occurrence: chain[0].id.clone(),
        };
        chain[2].parent = OccurrenceParent::Occurrence {
            occurrence: chain[1].id.clone(),
        };
        let chain_paths = LocalOccurrencePaths::new(&[], &table.references, &chain, &[]);
        let stream = sources[0].0;
        let keys = |keys: &[&str]| keys.iter().map(|key| (*key).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            chain_paths.resolve(stream, &keys(&[key_b, key_c, "dddd"])),
            Some((chain[2].id.clone(), 2))
        );
        assert_eq!(
            chain_paths.resolve(stream, &keys(&[key_a, key_b, key_c])),
            Some((chain[2].id.clone(), 3))
        );
        assert_eq!(
            chain_paths.resolve(stream, &keys(&[key_b])),
            Some((chain[1].id.clone(), 1))
        );
        let mut repeated = chain.clone();
        let mut second = chain[2].clone();
        second.id = OccurrenceId::mint("f3d:model:occurrence#second-c").unwrap();
        second.parent = OccurrenceParent::Root {};
        repeated.push(second);
        let repeated_paths = LocalOccurrencePaths::new(&[], &table.references, &repeated, &[]);
        assert!(repeated_paths.ambiguous(stream, &keys(&[key_c])));
        assert_eq!(
            repeated_paths.resolve(stream, &keys(&[key_b, key_c])),
            Some((chain[2].id.clone(), 2))
        );
        let encoded = serde_json::to_value(&table.references[0]).unwrap();
        let retained: crate::records::xref::XrefReference =
            serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(retained, table.references[0]);
        let mut invalid = encoded;
        invalid["placement_source"]["occurrence_guid"] = serde_json::json!("part.f3d");
        assert!(serde_json::from_value::<crate::records::xref::XrefReference>(invalid).is_err());
    }
}
