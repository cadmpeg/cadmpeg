// SPDX-License-Identifier: Apache-2.0
//! Consume current carrier names exactly once, using short Directory labels.

use super::{
    target_id, AttributeExport, AttributeTarget, AttributeValue, BTreeSet, CadIr, Entity, Targets,
};

fn carrier(target: &AttributeTarget) -> bool {
    matches!(
        target,
        AttributeTarget::Point(_) | AttributeTarget::Curve(_) | AttributeTarget::Surface(_)
    )
}

fn short_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 8
        && value.trim_end_matches(' ') == value
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' ')
}

pub(super) fn apply(
    ir: &CadIr,
    entities: &mut [Entity],
    targets: &Targets,
    result: &mut AttributeExport,
) {
    apply_sheet_boundary_labels(ir, entities, targets, result);
    let explicit_labels: BTreeSet<_> = ir
        .model
        .attributes
        .iter()
        .filter(|attribute| carrier(&attribute.target) && attribute.name == "iges.directory_label")
        .filter_map(|attribute| target_id(ir, &attribute.target))
        .collect();
    for attribute in &ir.model.attributes {
        if !carrier(&attribute.target)
            || !matches!(attribute.name.as_str(), "name" | "iges.directory_label")
        {
            continue;
        }
        let Some(id) = target_id(ir, &attribute.target) else {
            continue;
        };
        if attribute.name == "name" && explicit_labels.contains(id) {
            continue;
        }
        let [Some(AttributeValue::String(value))] = attribute.values.as_slice() else {
            continue;
        };
        let Some(indices) = targets.get(id) else {
            continue;
        };
        let [index] = indices.as_slice() else {
            continue;
        };
        if !short_label(value) {
            continue;
        }
        let Some(entity) = entities.get_mut(*index) else {
            continue;
        };
        if entity.type_code == 502 {
            let all_equal = entity.source_targets.iter().filter(|id| {
                ir.model.points.iter().any(|point| point.id.as_str() == id.as_str())
            }).all(|id| {
                if attribute.name == "name" && explicit_labels.contains(id.as_str()) {
                    return false;
                }
                let mut candidates = ir.model.attributes.iter().filter(|candidate| {
                    matches!(&candidate.target, AttributeTarget::Point(point) if point.as_str() == id)
                        && candidate.name == attribute.name
                });
                let Some(candidate) = candidates.next() else { return false; };
                candidates.next().is_none()
                    && matches!(candidate.values.as_slice(), [Some(AttributeValue::String(other))] if other == value)
            });
            if !all_equal {
                continue;
            }
        }
        entity.label.clone_from(value);
        result.preserved.insert(attribute.id.to_string());
    }
}

fn apply_sheet_boundary_labels(
    ir: &CadIr,
    entities: &mut [Entity],
    targets: &Targets,
    result: &mut AttributeExport,
) {
    'entity: for entity in entities.iter_mut().filter(|entity| entity.type_code == 143) {
        let Some(face) = ir.model.faces.iter().find(|face| {
            entity
                .source_targets
                .iter()
                .any(|id| id == face.id.as_str())
        }) else {
            continue;
        };
        let mut points = BTreeSet::new();
        for loop_id in &face.loops {
            let Some(boundary) = ir
                .model
                .loops
                .iter()
                .find(|boundary| boundary.id == *loop_id)
            else {
                continue 'entity;
            };
            for vertex in boundary.vertices() {
                let Some(vertex) = ir.model.vertices.iter().find(|row| row.id == *vertex) else {
                    continue 'entity;
                };
                points.insert(vertex.point.as_str());
            }
            for coedge_id in boundary.coedges() {
                let Some(edge) = ir
                    .model
                    .coedges
                    .iter()
                    .find(|coedge| coedge.id == *coedge_id)
                    .and_then(|coedge| ir.model.edges.iter().find(|edge| edge.id == coedge.edge))
                else {
                    continue 'entity;
                };
                for vertex in [&edge.start, &edge.end] {
                    let Some(vertex) = ir.model.vertices.iter().find(|row| row.id == *vertex)
                    else {
                        continue 'entity;
                    };
                    points.insert(vertex.point.as_str());
                }
            }
        }
        if points.is_empty() || points.iter().any(|point| targets.contains_key(*point)) {
            continue;
        }
        let mut selected = Vec::new();
        let mut label: Option<&str> = None;
        for point in points {
            let mut attrs = ir.model.attributes.iter().filter(|attribute| {
                matches!(&attribute.target, AttributeTarget::Point(id) if id.as_str() == point)
                    && matches!(attribute.name.as_str(), "name" | "iges.directory_label")
            });
            let Some(attribute) = attrs.next() else {
                selected.clear();
                break;
            };
            let [Some(AttributeValue::String(value))] = attribute.values.as_slice() else {
                selected.clear();
                break;
            };
            if attrs.next().is_some()
                || attribute.name != "name"
                || !short_label(value)
                || label.is_some_and(|label| label != value)
            {
                selected.clear();
                break;
            }
            label = Some(value);
            selected.push(attribute.id.to_string());
        }
        let Some(label) = label.filter(|_| !selected.is_empty()) else {
            continue;
        };
        if ir.model.bodies.iter().any(|body| {
            entity
                .source_targets
                .iter()
                .any(|id| id == body.id.as_str())
                && body
                    .name
                    .as_deref()
                    .is_some_and(|name| !name.is_empty() && name != label)
        }) {
            continue;
        }
        entity.label = label.to_owned();
        result.preserved.extend(selected);
    }
}
