// SPDX-License-Identifier: Apache-2.0
//! Typed construction-operation inventory from the outer ownership graph.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

use crate::object_graph::{ListItem, ObjectGraph, PayloadField};

/// Return the neutral feature arena entries carried by outer object graphs.
#[must_use]
pub fn features(graphs: &[ObjectGraph]) -> Vec<Feature> {
    let mut features = Vec::new();
    let mut ordinal = 0u64;
    for graph in graphs {
        let operation_ids: HashMap<u32, FeatureId> = graph
            .records
            .iter()
            .filter_map(|record| {
                operation_kind(record.class_name.as_deref()?).map(|_| {
                    Some((
                        u32::try_from(record.index + 1).ok()?,
                        feature_id(graph.pos, record.pos),
                    ))
                })?
            })
            .collect();
        for record in &graph.records {
            let Some(kind) = record.class_name.as_deref().and_then(operation_kind) else {
                continue;
            };
            let parent = record
                .owner_ref
                .and_then(|owner| operation_ids.get(&owner))
                .cloned();
            let mut seen = HashSet::new();
            let dependencies = payload_references(&record.payload.fields)
                .into_iter()
                .filter(|reference| Some(*reference) != record.owner_ref)
                .filter_map(|reference| operation_ids.get(&reference).cloned())
                .filter(|feature| seen.insert(feature.clone()))
                .collect();
            let mut properties = BTreeMap::new();
            if let Some(storage) = record.storage_ref {
                properties.insert("storage_ref".to_string(), storage.to_string());
            }
            properties.insert(
                "payload_subtype".to_string(),
                format!("{:?}", record.subtype),
            );
            features.push(Feature {
                id: feature_id(graph.pos, record.pos),
                ordinal,
                name: None,
                suppressed: false,
                parent,
                dependencies,
                source_properties: BTreeMap::new(),
                source_tag: record.class_name.clone(),
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
                definition: FeatureDefinition::Native {
                    kind: kind.to_string(),
                    parameters: BTreeMap::new(),
                    properties,
                },
                native_ref: Some(format!("catia:outer:object-record#{:010}", record.pos)),
            });
            ordinal += 1;
        }
    }
    features
}

fn feature_id(graph_pos: usize, record_pos: usize) -> FeatureId {
    FeatureId(format!(
        "catia:design:feature#{graph_pos:010}:{record_pos:010}"
    ))
}

fn operation_kind(class_name: &str) -> Option<&'static str> {
    Some(match class_name {
        "PRTSketch" | "Sketch" => "sketch",
        "Pad" => "pad",
        "Pocket" => "pocket",
        "Shaft" => "shaft",
        "Groove" => "groove",
        "Hole" => "hole",
        "EdgeFillet" => "edge_fillet",
        "Chamfer" => "chamfer",
        "Draft" => "draft",
        "Shell" => "shell",
        "Rib" => "rib",
        "CircPattern" => "circular_pattern",
        "RectPattern" => "rectangular_pattern",
        "UserPattern" => "user_pattern",
        "Mirror" => "mirror",
        _ => return None,
    })
}

fn payload_references(fields: &[PayloadField]) -> Vec<u32> {
    fields
        .iter()
        .flat_map(|field| match field {
            PayloadField::Reference { value, .. } => vec![*value],
            PayloadField::List { items, .. } => items
                .iter()
                .filter_map(|item| match item {
                    ListItem::Reference(value) => Some(*value),
                    ListItem::Atom(_) => None,
                })
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object_graph::{ObjectPayload, ObjectRecord, PayloadSubtype};

    fn record(
        index: usize,
        class_name: &str,
        owner_ref: Option<u32>,
        fields: Vec<PayloadField>,
    ) -> ObjectRecord {
        ObjectRecord {
            index,
            pos: 100 + index,
            total_len: 1,
            lead: 0,
            head: Vec::new(),
            owner_ref,
            class_ref: None,
            class_name: Some(class_name.to_string()),
            storage_ref: None,
            payload: ObjectPayload { size: 0, fields },
            subtype: PayloadSubtype::Empty,
        }
    }

    #[test]
    fn construction_inventory_preserves_order_parent_and_operands() {
        let graph = ObjectGraph {
            pos: 50,
            total_len: 3,
            catalog_pos: None,
            records: vec![
                record(0, "Pad", None, Vec::new()),
                record(
                    1,
                    "Pocket",
                    Some(1),
                    vec![PayloadField::Reference {
                        value: 1,
                        offset: 0,
                    }],
                ),
                record(2, "Pocket_Depth", Some(2), Vec::new()),
            ],
        };

        let features = features(&[graph]);

        assert_eq!(features.len(), 2);
        assert_eq!(features[1].parent, Some(features[0].id.clone()));
        assert!(features[1].dependencies.is_empty());
        assert_eq!(features[1].source_tag.as_deref(), Some("Pocket"));
    }
}
