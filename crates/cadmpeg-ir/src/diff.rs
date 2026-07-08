// SPDX-License-Identifier: Apache-2.0
//! Structural comparison of IR documents.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::CadIr;

/// A modified entity and its differing top-level fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ModifiedEntity {
    pub id: String,
    pub fields: Vec<String>,
}

/// Changes within one entity arena.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ArenaDiff {
    pub kind: &'static str,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<ModifiedEntity>,
}

/// Structural changes between two IR documents.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct IrDiff {
    pub unit_change: Option<(crate::units::Units, crate::units::Units)>,
    pub tolerance_change: Option<(crate::units::Tolerances, crate::units::Tolerances)>,
    pub per_arena: Vec<ArenaDiff>,
}

impl IrDiff {
    pub fn is_empty(&self) -> bool {
        self.unit_change.is_none()
            && self.tolerance_change.is_none()
            && self.per_arena.iter().all(|arena| {
                arena.added.is_empty() && arena.removed.is_empty() && arena.modified.is_empty()
            })
    }
}

fn differing_fields<T: Serialize>(left: &T, right: &T) -> Vec<String> {
    let (Ok(Value::Object(left)), Ok(Value::Object(right))) =
        (serde_json::to_value(left), serde_json::to_value(right))
    else {
        return vec!["value".to_string()];
    };
    left.keys()
        .chain(right.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|key| left.get(*key) != right.get(*key))
        .cloned()
        .collect()
}

fn arena<T, F>(kind: &'static str, left: &[T], right: &[T], id: F) -> ArenaDiff
where
    T: PartialEq + Serialize,
    F: Fn(&T) -> String,
{
    let left: BTreeMap<_, _> = left.iter().map(|entity| (id(entity), entity)).collect();
    let right: BTreeMap<_, _> = right.iter().map(|entity| (id(entity), entity)).collect();
    let removed = left
        .keys()
        .filter(|id| !right.contains_key(*id))
        .cloned()
        .collect();
    let added = right
        .keys()
        .filter(|id| !left.contains_key(*id))
        .cloned()
        .collect();
    let modified = left
        .iter()
        .filter_map(|(id, before)| {
            let after = right.get(id)?;
            (*before != *after).then(|| ModifiedEntity {
                id: id.clone(),
                fields: differing_fields(*before, *after),
            })
        })
        .collect();
    ArenaDiff {
        kind,
        added,
        removed,
        modified,
    }
}

/// Compare units, tolerances, and every entity arena by stable entity ID.
pub fn diff(left: &CadIr, right: &CadIr) -> IrDiff {
    let unit_change =
        (left.units != right.units).then(|| (left.units.clone(), right.units.clone()));
    let tolerance_change =
        (left.tolerances != right.tolerances).then_some((left.tolerances, right.tolerances));
    let per_arena = vec![
        arena("bodies", &left.bodies, &right.bodies, |e| e.id.0.clone()),
        arena("lumps", &left.lumps, &right.lumps, |e| e.id.0.clone()),
        arena("shells", &left.shells, &right.shells, |e| e.id.0.clone()),
        arena("faces", &left.faces, &right.faces, |e| e.id.0.clone()),
        arena("loops", &left.loops, &right.loops, |e| e.id.0.clone()),
        arena("coedges", &left.coedges, &right.coedges, |e| e.id.0.clone()),
        arena("edges", &left.edges, &right.edges, |e| e.id.0.clone()),
        arena("vertices", &left.vertices, &right.vertices, |e| {
            e.id.0.clone()
        }),
        arena("points", &left.points, &right.points, |e| e.id.0.clone()),
        arena("surfaces", &left.surfaces, &right.surfaces, |e| {
            e.id.0.clone()
        }),
        arena("curves", &left.curves, &right.curves, |e| e.id.0.clone()),
        arena("pcurves", &left.pcurves, &right.pcurves, |e| e.id.0.clone()),
        arena(
            "surface_parameterizations",
            &left.surface_parameterizations,
            &right.surface_parameterizations,
            |e| e.surface.0.clone(),
        ),
        arena(
            "procedural_surfaces",
            &left.procedural_surfaces,
            &right.procedural_surfaces,
            |e| e.surface.0.clone(),
        ),
        arena(
            "procedural_curves",
            &left.procedural_curves,
            &right.procedural_curves,
            |e| e.curve.0.clone(),
        ),
        arena(
            "sketch_curve_links",
            &left.sketch_curve_links,
            &right.sketch_curve_links,
            |e| format!("{}:{}", e.coedge.0, e.sketch_curve_id),
        ),
        arena(
            "persistent_design_links",
            &left.persistent_design_links,
            &right.persistent_design_links,
            |e| format!("{:?}:{}:{}", e.target, e.design_id, e.ordinal),
        ),
        arena(
            "construction_recipes",
            &left.construction_recipes,
            &right.construction_recipes,
            |e| format!("{:?}:{:?}:{}", e.kind, e.design_id, e.recipe_index),
        ),
        arena(
            "persistent_references",
            &left.persistent_references,
            &right.persistent_references,
            |e| format!("{:?}:{}", e.kind, e.meta.provenance.offset),
        ),
        arena(
            "lost_edge_references",
            &left.lost_edge_references,
            &right.lost_edge_references,
            |e| format!("{}:{}", e.class_tag, e.record_index),
        ),
        arena(
            "design_objects",
            &left.design_objects,
            &right.design_objects,
            |e| e.self_guid.clone(),
        ),
        arena(
            "design_entity_headers",
            &left.design_entity_headers,
            &right.design_entity_headers,
            |e| format!("{}:{}", e.class_tag, e.entity_id),
        ),
        arena(
            "design_record_headers",
            &left.design_record_headers,
            &right.design_record_headers,
            |e| format!("{}:{}", e.record_index, e.class_tag),
        ),
        arena(
            "sketch_relations",
            &left.sketch_relations,
            &right.sketch_relations,
            |e| e.record_index.to_string(),
        ),
        arena(
            "sketch_points",
            &left.sketch_points,
            &right.sketch_points,
            |e| e.record_index.to_string(),
        ),
        arena(
            "sketch_curve_identities",
            &left.sketch_curve_identities,
            &right.sketch_curve_identities,
            |e| e.record_index.to_string(),
        ),
        arena(
            "design_body_members",
            &left.design_body_members,
            &right.design_body_members,
            |e| e.entity_suffix.to_string(),
        ),
        arena(
            "act_entities",
            &left.act_entities,
            &right.act_entities,
            |e| format!("{}:{}", e.record_index, e.entity_id),
        ),
        arena("act_guids", &left.act_guids, &right.act_guids, |e| {
            format!("{}:{}", e.ordinal, e.guid)
        }),
        arena(
            "act_root_components",
            &left.act_root_components,
            &right.act_root_components,
            |e| format!("{}:{}", e.record_index, e.entity_id),
        ),
        arena(
            "tessellations",
            &left.tessellations,
            &right.tessellations,
            |e| e.id.clone(),
        ),
        arena(
            "feature_histories",
            &left.feature_histories,
            &right.feature_histories,
            |e| format!("{}:{}", e.meta.provenance.stream, e.meta.provenance.offset),
        ),
        arena(
            "feature_input_lanes",
            &left.feature_input_lanes,
            &right.feature_input_lanes,
            |e| e.id.clone(),
        ),
        arena(
            "asm_histories",
            &left.asm_histories,
            &right.asm_histories,
            |e| format!("{}:{}", e.meta.provenance.stream, e.meta.provenance.offset),
        ),
        arena("appearances", &left.appearances, &right.appearances, |e| {
            e.id.0.clone()
        }),
        arena(
            "appearance_bindings",
            &left.appearance_bindings,
            &right.appearance_bindings,
            |e| format!("{:?}:{}", e.target, e.appearance.0),
        ),
        arena("attributes", &left.attributes, &right.attributes, |e| {
            e.id.0.clone()
        }),
        arena("unknowns", &left.unknowns, &right.unknowns, |e| {
            e.id.0.clone()
        }),
    ];
    IrDiff {
        unit_change,
        tolerance_change,
        per_arena,
    }
}

#[cfg(test)]
mod tests {
    use super::diff;
    use crate::examples::unit_cube;

    #[test]
    fn detects_changes_in_all_document_dimensions() {
        let left = unit_cube();
        let mut right = left.clone();
        right.points[0].position.x += 1.0;
        right.loops.pop();
        right.coedges.pop();

        let result = diff(&left, &right);
        assert!(!result.is_empty());
        assert_eq!(
            result
                .per_arena
                .iter()
                .find(|a| a.kind == "points")
                .unwrap()
                .modified[0]
                .fields,
            ["position"]
        );
        assert_eq!(
            result
                .per_arena
                .iter()
                .find(|a| a.kind == "loops")
                .unwrap()
                .removed
                .len(),
            1
        );
        assert_eq!(
            result
                .per_arena
                .iter()
                .find(|a| a.kind == "coedges")
                .unwrap()
                .removed
                .len(),
            1
        );
    }

    #[test]
    fn identical_documents_have_empty_diff() {
        let ir = unit_cube();
        assert!(diff(&ir, &ir).is_empty());
    }
}
