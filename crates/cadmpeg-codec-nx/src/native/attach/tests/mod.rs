// SPDX-License-Identifier: Apache-2.0

pub(crate) use super::*;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::Length;
use cadmpeg_ir::ids::BodyId;
use std::collections::BTreeMap;

pub(crate) fn hole_diameters_for_operations(
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> BTreeMap<String, Length> {
    hole_body_projection(ir, operations, outputs)
        .map(|projection| projection.diameters)
        .unwrap_or_default()
}

pub(crate) fn simple_hole_diameters(
    ir: &CadIr,
    templates: &[crate::native::features::FeatureSimpleHoleTemplate],
    groups: &[crate::native::features::FeatureSimpleHoleConstructionGroup],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> BTreeMap<String, Length> {
    let operation_positions = templates
        .iter()
        .enumerate()
        .map(|(position, template)| (template.operation_label.as_str(), position))
        .collect::<BTreeMap<_, _>>();
    let Some(operations) = simple_hole_operations(templates, groups, &operation_positions) else {
        return BTreeMap::new();
    };
    hole_diameters_for_operations(ir, &operations, outputs)
}

mod configuration;
mod holes_offsets_and_attributes;
mod operations_and_holes;
