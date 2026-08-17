// SPDX-License-Identifier: Apache-2.0
//! Feature dimension parameters, relation tables, and transfer.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Angle, DesignParameter, DimensionDisplay, FeatureSourceContent, Length, ParameterId,
    ParameterValue,
};
use cadmpeg_ir::sketches::SketchId;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::ContainerScan;

use super::super::native::annotate;
use super::super::sketch_ids::{
    feature_sketch_record_id_in_scan, model_sketch_id, section_owner_feature_id,
    sketch_identity_scope,
};
use super::super::uniqueness::exactly_one;

pub(in super::super) fn feature_dimension_parameter_id(
    sketch: &SketchId,
    external_id: u32,
) -> ParameterId {
    ParameterId(format!(
        "creo:featdefs:parameter#{}:{external_id}",
        sketch_identity_scope(sketch),
    ))
}

pub(in super::super) fn feature_dimension_parameter_row_id(
    sketch: &SketchId,
    external_id: u32,
    occurrence: Option<usize>,
) -> ParameterId {
    occurrence.map_or_else(
        || feature_dimension_parameter_id(sketch, external_id),
        |occurrence| {
            ParameterId(format!(
                "creo:featdefs:parameter#{}:{external_id}:{}",
                sketch_identity_scope(sketch),
                occurrence + 1
            ))
        },
    )
}

pub(in super::super) fn resolved_feature_dimension_parameter<'a>(
    sketch: &SketchId,
    table: &'a crate::feature::FeatureDimensionTable,
    ordinal: usize,
) -> Option<(&'a crate::feature::FeatureDimension, ParameterId)> {
    feature_dimension_table_complete(table).then_some(())?;
    let dimension = table.rows.get(ordinal)?;
    (table
        .rows
        .iter()
        .filter(|candidate| candidate.external_id == dimension.external_id)
        .count()
        == 1)
        .then(|| {
            (
                dimension,
                feature_dimension_parameter_id(sketch, dimension.external_id),
            )
        })
}

pub(in super::super) fn feature_dimension_table_complete(
    table: &crate::feature::FeatureDimensionTable,
) -> bool {
    usize::try_from(table.declared_count).ok() == Some(table.rows.len())
}

pub(in super::super) fn feature_dimension_display(dimension_type: u32) -> Option<DimensionDisplay> {
    match dimension_type {
        0x03 => Some(DimensionDisplay::Radius),
        0x04 => Some(DimensionDisplay::Diameter),
        _ => None,
    }
}

pub(in super::super) fn feature_relation_table_complete(
    table: &crate::feature::FeatureRelationTable,
) -> bool {
    feature_relation_table_expected_rows(table) == Some(table.rows.len())
}

pub(in super::super) fn feature_relation_table_expected_rows(
    table: &crate::feature::FeatureRelationTable,
) -> Option<usize> {
    match table.declared_count {
        0 => None,
        1 => Some(0),
        count => usize::try_from(count - 2).ok(),
    }
}

pub(in super::super) fn feature_relation_table_missing_rows(
    table: &crate::feature::FeatureRelationTable,
) -> usize {
    feature_relation_table_expected_rows(table)
        .map_or(0, |expected| expected.saturating_sub(table.rows.len()))
}

pub(in super::super) fn feature_solver_table_complete(
    header: Option<&crate::feature::FeatureSolverTableHeader>,
    row_count: usize,
) -> bool {
    header.map_or(row_count == 0, |header| {
        usize::try_from(header.declared_count).ok() == Some(row_count)
    })
}

pub(in super::super) fn feature_solver_table_missing_rows(
    header: Option<&crate::feature::FeatureSolverTableHeader>,
    row_count: usize,
) -> usize {
    header.map_or(0, |header| {
        usize::try_from(header.declared_count)
            .unwrap_or(usize::MAX)
            .saturating_sub(row_count)
    })
}

pub(in super::super) fn feature_skamp_table_complete(
    table: &crate::feature::FeatureRelationTable,
) -> bool {
    feature_solver_table_complete(table.skamp_header.as_ref(), table.skamps.len())
}

pub(in super::super) fn feature_dimension_parameter_layout(
    keys: &[(SketchId, u32)],
) -> Option<Vec<(u32, String, Option<usize>)>> {
    let mut name_counts = BTreeMap::new();
    let mut local_counts = BTreeMap::new();
    for (sketch, external_id) in keys {
        *name_counts
            .entry((sketch.clone(), *external_id))
            .or_insert(0usize) += 1;
    }
    for key in keys {
        *local_counts.entry(key.clone()).or_insert(0usize) += 1;
    }
    let mut next_ordinals = BTreeMap::<SketchId, u32>::new();
    let mut local_occurrences = BTreeMap::new();
    keys.iter()
        .map(|key @ (sketch, external_id)| {
            let ordinal = next_ordinals.entry(sketch.clone()).or_default();
            let assigned = *ordinal;
            *ordinal = ordinal.checked_add(1)?;
            let occurrence = (local_counts[key] > 1).then(|| {
                let occurrence = local_occurrences.entry(key.clone()).or_insert(0usize);
                let assigned = *occurrence;
                *occurrence += 1;
                assigned
            });
            let name = if name_counts[&(sketch.clone(), *external_id)] == 1 {
                format!("d{external_id}")
            } else if let Some(occurrence) = occurrence {
                format!(
                    "d{}_{}_{}",
                    sketch_identity_scope(sketch),
                    external_id,
                    occurrence + 1
                )
            } else {
                format!("d{}_{}", sketch_identity_scope(sketch), external_id)
            };
            Some((assigned, name, occurrence))
        })
        .collect()
}

pub(in super::super) fn transfer_feature_dimensions(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> (usize, BTreeMap<String, ParameterId>) {
    let feature_ids = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<BTreeSet<_>>();
    let mut candidates = Vec::new();
    for definition in &scan.features.definitions {
        let sketch = model_sketch_id(scan, definition);
        let owner = section_owner_feature_id(scan, definition.id, &sketch);
        if !feature_ids.contains(&owner) {
            continue;
        }
        let Some(table) = &definition.dimensions else {
            continue;
        };
        for (source_ordinal, dimension) in table.rows.iter().enumerate() {
            candidates.push((sketch.clone(), definition, source_ordinal, dimension));
        }
    }
    candidates.sort_by_key(|(_, definition, source_ordinal, _)| {
        (definition.offset, definition.id, *source_ordinal)
    });
    let keys = candidates
        .iter()
        .map(|(sketch, _, _, dimension)| (sketch.clone(), dimension.external_id))
        .collect::<Vec<_>>();
    let Some(layout) = feature_dimension_parameter_layout(&keys) else {
        return (0, BTreeMap::new());
    };
    let unique_external_ids = keys
        .iter()
        .fold(BTreeMap::new(), |mut counts, (_, external_id)| {
            *counts.entry(*external_id).or_insert(0usize) += 1;
            counts
        });
    let transferred = layout.len();
    let mut relation_parameters = BTreeMap::new();
    for ((sketch, definition, source_ordinal, dimension), (ordinal, name, occurrence)) in
        candidates.into_iter().zip(layout)
    {
        let owner_id = section_owner_feature_id(scan, definition.id, &sketch);
        let id = feature_dimension_parameter_row_id(&sketch, dimension.external_id, occurrence);
        if unique_external_ids[&dimension.external_id] == 1 {
            relation_parameters.insert(format!("d{}", dimension.external_id), id.clone());
        }
        annotate(
            annotations,
            &id.0,
            "FeatDefs",
            dimension.offset as u64,
            "section_dimension",
            Exactness::Derived,
        );
        let mut properties = BTreeMap::from([
            ("definition_id".to_string(), definition.id.to_string()),
            ("source_ordinal".to_string(), source_ordinal.to_string()),
            ("external_id".to_string(), dimension.external_id.to_string()),
            (
                "dimension_type".to_string(),
                dimension.dimension_type.to_string(),
            ),
            (
                "direction_byte".to_string(),
                dimension.direction_byte.to_string(),
            ),
        ]);
        if let Some(auxiliary) = dimension.auxiliary_value {
            properties.insert("auxiliary_value".to_string(), auxiliary.to_string());
        }
        if dimension.value.is_none() {
            properties.insert("value_state".to_string(), "unresolved".to_string());
        }
        if let Some(token) = &dimension.unresolved_value_token {
            let encoding = match token.as_slice() {
                [0x00, _, _] => Some("three_byte_placeholder"),
                [0x01, _, _, _] => Some("four_byte_placeholder"),
                _ => None,
            };
            if let Some(encoding) = encoding {
                properties.insert("value_encoding".to_string(), encoding.to_string());
                let value_token = token.iter().fold(
                    String::with_capacity(token.len() * 2),
                    |mut encoded, byte| {
                        write!(encoded, "{byte:02x}").expect("writing to a string cannot fail");
                        encoded
                    },
                );
                properties.insert("value_token".to_string(), value_token);
            }
        }
        let expression = dimension
            .value
            .map_or_else(String::new, |value| value.to_string());
        let value = dimension.value.map(|value| match dimension.value_unit {
            crate::feature::DimensionUnit::Radians => ParameterValue::Angle(Angle(value)),
            crate::feature::DimensionUnit::Millimeters => ParameterValue::Length(Length(value)),
            crate::feature::DimensionUnit::SchemaDefined => ParameterValue::Real(value),
        });
        ir.model.parameters.push(DesignParameter {
            id: id.clone(),
            owner: Some(owner_id.clone()),
            ordinal,
            name,
            expression,
            display: feature_dimension_display(dimension.dimension_type),
            value,
            dependencies: Vec::new(),
            properties,
            pmi: None,
            native_ref: Some(feature_sketch_record_id_in_scan(scan, definition)),
        });
        if let Some(feature) = exactly_one(
            ir.model
                .features
                .iter_mut()
                .filter(|feature| feature.id == owner_id),
        ) {
            feature
                .source_content
                .push(FeatureSourceContent::Parameter(id));
        }
    }
    (transferred, relation_parameters)
}

#[cfg(test)]
mod tests;
