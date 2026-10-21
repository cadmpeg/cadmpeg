// SPDX-License-Identifier: Apache-2.0
//! Consume current carrier names exactly once, using short Directory labels.

use super::{
    AttributeExport, AttributeTarget, AttributeValue, BTreeSet, CadIr, Entity, Targets, target_id,
};

fn carrier(target: &AttributeTarget) -> bool {
    matches!(
        target,
        AttributeTarget::Body(_)
            | AttributeTarget::Point(_)
            | AttributeTarget::Curve(_)
            | AttributeTarget::Surface(_)
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
        .filter_map(|attribute| target_id(&attribute.target))
        .collect();
    for attribute in &ir.model.attributes {
        if !carrier(&attribute.target)
            || (matches!(attribute.target, AttributeTarget::Body(_))
                && attribute.name != "iges.directory_label")
            || !matches!(attribute.name.as_str(), "name" | "iges.directory_label")
        {
            continue;
        }
        let Some(id) = target_id(&attribute.target) else {
            continue;
        };
        if attribute.name == "name" && explicit_labels.contains(id) {
            continue;
        }
        let [AttributeValue::String(value)] = attribute.values.as_slice() else {
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
        entity.label.clone_from(value);
        result.preserved.insert(attribute.id.to_string());
    }
}
