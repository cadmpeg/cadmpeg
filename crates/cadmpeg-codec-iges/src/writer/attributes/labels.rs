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
