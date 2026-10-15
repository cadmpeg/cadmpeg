// SPDX-License-Identifier: Apache-2.0
//! Feature dimension parameters, relation tables, and transfer.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::sketches::SketchId;
use cadmpeg_ir::{
    features::{
        DesignParameter, DimensionDisplay, FeatureSourceContent, ParameterId, ParameterValue,
    },
    scalar::{Angle, Length},
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};
use std::collections::{BTreeMap, BTreeSet};

use crate::container::ContainerScan;
use crate::feature::definitions::SolverSubtable;

use super::super::native::annotate;
#[cfg(test)]
use super::super::sketch_ids::sketch_identity_key;
use super::super::sketch_ids::{
    feature_sketch_record_id_in_scan, model_sketch_id, section_owner_feature_id,
    sketch_identity_scope,
};
use super::super::uniqueness::exactly_one;

fn insert_dimension_property(
    properties: &mut BTreeMap<String, String>,
    key: &'static str,
    value: impl std::fmt::Display,
) {
    let key = key.to_owned();
    let value = format!("{value}");
    properties.insert(key, value);
}

struct HexToken<'a>(&'a [u8]);

impl std::fmt::Display for HexToken<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

fn dimension_expression(value: Option<f64>) -> Result<String, cadmpeg_core::CodecError> {
    value
        .map(|value| Ok(format!("{value}")))
        .transpose()
        .map(std::option::Option::unwrap_or_default)
}

fn push_feature_source_parameter(
    content: &mut cadmpeg_ir::features::FeatureContent,
    id: ParameterId,
) -> Result<(), cadmpeg_core::CodecError> {
    if content
        .iter()
        .any(|entry| matches!(entry, FeatureSourceContent::Parameter(existing) if existing == &id))
    {
        return Err(cadmpeg_core::CodecError::Malformed(
            "source_content repeats a parameter or child-feature reference".into(),
        ));
    }
    content
        .push(FeatureSourceContent::Parameter(id))
        .map_err(cadmpeg_core::CodecError::from)
}

#[cfg(test)]
pub(in super::super) fn feature_dimension_parameter_id(
    sketch: &SketchId,
    external_id: u32,
) -> Option<ParameterId> {
    Some(ParameterId::compose(
        &crate::identity::FEATDEFS_PARAMETER,
        sketch_identity_key(sketch)?.colon(external_id),
    ))
}

#[cfg(test)]
pub(in super::super) fn feature_dimension_parameter_row_id(
    sketch: &SketchId,
    external_id: u32,
    occurrence: Option<usize>,
) -> Option<ParameterId> {
    occurrence.map_or_else(
        || feature_dimension_parameter_id(sketch, external_id),
        |occurrence| {
            Some(ParameterId::compose(
                &crate::identity::FEATDEFS_PARAMETER,
                sketch_identity_key(sketch)?
                    .colon(external_id)
                    .colon(occurrence + 1),
            ))
        },
    )
}

fn feature_dimension_parameter_row_id_admitted(
    sketch: &SketchId,
    external_id: u32,
    occurrence: Option<usize>,
) -> Result<Option<ParameterId>, cadmpeg_core::CodecError> {
    let text = if let Some(occurrence) = occurrence {
        let ordinal = occurrence.checked_add(1).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("Creo dimension parameter identity overflows")
        })?;
        format!(
            "creo:featdefs:parameter#{}:{external_id}:{ordinal}",
            sketch_identity_scope(sketch),
        )
    } else {
        format!(
            "creo:featdefs:parameter#{}:{external_id}",
            sketch_identity_scope(sketch),
        )
    };
    Ok(ParameterId::try_from(text).ok())
}

#[cfg(test)]
pub(in super::super) fn resolved_feature_dimension_parameter<'a>(
    sketch: &SketchId,
    table: &'a crate::feature::definitions::FeatureDimensionTable,
    ordinal: usize,
) -> Option<(
    &'a crate::feature::definitions::FeatureDimension,
    ParameterId,
)> {
    feature_dimension_table_complete(table).then_some(())?;
    let dimension = table.rows.get(ordinal)?;
    (table
        .rows
        .iter()
        .filter(|candidate| candidate.external_id == dimension.external_id)
        .count()
        == 1)
        .then_some(())?;
    Some((
        dimension,
        feature_dimension_parameter_id(sketch, dimension.external_id)?,
    ))
}

pub(in super::super) fn resolved_feature_dimension_parameter_admitted<'a>(
    sketch: &SketchId,
    table: &'a crate::feature::definitions::FeatureDimensionTable,
    ordinal: usize,
) -> Result<
    Option<(
        &'a crate::feature::definitions::FeatureDimension,
        ParameterId,
    )>,
    cadmpeg_core::CodecError,
> {
    if !feature_dimension_table_complete(table) {
        return Ok(None);
    }
    let Some(dimension) = table.rows.get(ordinal) else {
        return Ok(None);
    };
    let unique = table
        .rows
        .iter()
        .filter(|candidate| candidate.external_id == dimension.external_id)
        .count()
        == 1;
    if !unique {
        return Ok(None);
    }
    Ok(
        feature_dimension_parameter_row_id_admitted(sketch, dimension.external_id, None)?
            .map(|parameter| (dimension, parameter)),
    )
}

pub(in super::super) fn planned_feature_dimension_parameter_ids(
    scan: &ContainerScan,
) -> BTreeSet<ParameterId> {
    let mut ids = BTreeSet::new();
    for definition in &scan.features.definitions {
        let Some(table) = &definition.dimensions else {
            continue;
        };
        let Some(sketch) = model_sketch_id(scan, definition) else {
            continue;
        };
        if !feature_dimension_table_complete(table) {
            continue;
        }
        for dimension in &table.rows {
            if table
                .rows
                .iter()
                .filter(|candidate| candidate.external_id == dimension.external_id)
                .count()
                != 1
            {
                continue;
            }
            let text = format!(
                "creo:featdefs:parameter#{}:{}",
                crate::decode::sketch_ids::sketch_identity_scope(&sketch),
                dimension.external_id,
            );
            let Ok(parameter) = ParameterId::try_from(text) else {
                continue;
            };
            ids.insert(parameter);
        }
    }
    ids
}

pub(in super::super) fn feature_dimension_table_complete(
    table: &crate::feature::definitions::FeatureDimensionTable,
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
    table: &crate::feature::definitions::FeatureRelationTable,
) -> bool {
    feature_relation_table_expected_rows(table) == Some(table.rows.len())
}

pub(in super::super) fn feature_relation_table_expected_rows(
    table: &crate::feature::definitions::FeatureRelationTable,
) -> Option<usize> {
    match table.declared_count {
        0 => None,
        1 => Some(0),
        count => usize::try_from(count - 2).ok(),
    }
}

pub(in super::super) fn feature_relation_table_missing_rows(
    table: &crate::feature::definitions::FeatureRelationTable,
) -> Result<usize, cadmpeg_core::CodecError> {
    let Some(expected) = feature_relation_table_expected_rows(table) else {
        return Ok(0);
    };
    expected.checked_sub(table.rows.len()).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("relation row count exceeds the declared count")
    })
}

pub(in super::super) fn feature_skamp_table_complete(
    table: &crate::feature::definitions::FeatureRelationTable,
) -> bool {
    table
        .skamps
        .as_ref()
        .is_none_or(SolverSubtable::is_complete)
}

pub(in super::super) fn feature_dimension_parameter_layout(
    keys: &[(SketchId, u32)],
) -> Option<impl ExactSizeIterator<Item = (u32, String, Option<usize>)> + std::fmt::Debug> {
    let mut local_counts = BTreeMap::<(&SketchId, u32), usize>::new();
    for (sketch, external_id) in keys {
        let key = (sketch, *external_id);
        *local_counts.entry(key).or_insert(0) += 1;
    }
    let mut next_ordinals = BTreeMap::<&SketchId, u32>::new();
    let mut local_occurrences = BTreeMap::<(&SketchId, u32), usize>::new();
    let mut layout = Vec::new();
    for (sketch, external_id) in keys {
        let ordinal = next_ordinals.entry(sketch).or_default();
        let assigned = *ordinal;
        let next = ordinal.checked_add(1)?;
        *ordinal = next;
        let key = (sketch, *external_id);
        let occurrence = if local_counts[&key] > 1 {
            let next = local_occurrences.entry(key).or_insert(0);
            let assigned = *next;
            *next += 1;
            Some(assigned)
        } else {
            None
        };
        let name = if local_counts[&key] == 1 {
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
        layout.push((assigned, name, occurrence));
    }
    Some(layout.into_iter())
}

pub(in super::super) fn transfer_feature_dimensions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<(usize, BTreeMap<String, ParameterId>), cadmpeg_core::CodecError> {
    let mut feature_ids = BTreeSet::new();
    for feature in &ir.model.features {
        if !feature_ids.contains(&feature.id) {
            feature_ids.insert(feature.id.clone());
        }
    }
    let mut candidates = Vec::new();
    for definition in &scan.features.definitions {
        let Some(sketch) = model_sketch_id(scan, definition) else {
            continue;
        };
        let Some(owner) = section_owner_feature_id(scan, definition.identity.id(), &sketch) else {
            continue;
        };
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
    candidates.as_mut_slice().sort_by(
        |(_, left, left_ordinal, _), (_, right, right_ordinal, _)| {
            (left.offset, left.identity.id(), *left_ordinal).cmp(&(
                right.offset,
                right.identity.id(),
                *right_ordinal,
            ))
        },
    );
    let mut keys = Vec::new();
    for (sketch, _, _, dimension) in &candidates {
        keys.push((sketch.clone(), dimension.external_id));
    }
    let Some(layout) = feature_dimension_parameter_layout(&keys) else {
        return Ok((0, BTreeMap::new()));
    };
    let mut unique_external_ids = BTreeMap::new();
    for (_, external_id) in &keys {
        *unique_external_ids.entry(*external_id).or_insert(0usize) += 1;
    }
    let transferred = layout.len();
    let mut relation_parameters = BTreeMap::new();
    for ((sketch, definition, source_ordinal, dimension), (ordinal, name, occurrence)) in
        candidates.into_iter().zip(layout)
    {
        let Some(owner_id) = section_owner_feature_id(scan, definition.identity.id(), &sketch)
        else {
            continue;
        };
        let Some(id) = feature_dimension_parameter_row_id_admitted(
            &sketch,
            dimension.external_id,
            occurrence,
        )?
        else {
            continue;
        };
        if unique_external_ids[&dimension.external_id] == 1 {
            relation_parameters.insert(format!("d{}", dimension.external_id), id.clone());
        }
        annotate(
            annotations,
            id.as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(dimension.offset),
            "section_dimension",
            Exactness::Derived,
        )?;
        let mut properties = BTreeMap::new();
        insert_dimension_property(&mut properties, "definition_id", definition.identity.id());
        insert_dimension_property(&mut properties, "source_ordinal", source_ordinal);
        insert_dimension_property(&mut properties, "external_id", dimension.external_id);
        insert_dimension_property(&mut properties, "dimension_type", dimension.dimension_type);
        insert_dimension_property(&mut properties, "direction_byte", dimension.direction_byte);
        if let Some(auxiliary) = dimension.auxiliary_value {
            insert_dimension_property(&mut properties, "auxiliary_value", auxiliary);
        }
        if dimension.value.resolved().is_none() {
            insert_dimension_property(&mut properties, "value_state", "unresolved");
        }
        if let Some(token) = dimension.value.unresolved_token() {
            let encoding = match token {
                [0x00, _, _] => Some("three_byte_placeholder"),
                [0x01, _, _, _] => Some("four_byte_placeholder"),
                _ => None,
            };
            if let Some(encoding) = encoding {
                insert_dimension_property(&mut properties, "value_encoding", encoding);
                insert_dimension_property(&mut properties, "value_token", HexToken(token));
            }
        }
        let expression = dimension_expression(dimension.value.resolved())?;
        let value = dimension
            .value
            .resolved()
            .and_then(|value| match dimension.unit() {
                crate::feature::definitions::DimensionUnit::Radians => {
                    Angle::new(value).map(ParameterValue::Angle)
                }
                crate::feature::definitions::DimensionUnit::Millimeters => {
                    Length::new(value).map(ParameterValue::Length)
                }
                crate::feature::definitions::DimensionUnit::SchemaDefined => {
                    cadmpeg_ir::scalar::FiniteReal::new(value).map(ParameterValue::Real)
                }
            });
        ctx.charge_entities(1, "admit Creo model parameters")?;
        source_carriers.admit_parameter(
            ir,
            DesignParameter {
                id: id.clone(),
                owner: Some(owner_id.clone()),
                ordinal,
                name,
                expression,
                display: feature_dimension_display(dimension.dimension_type),
                value,
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                properties: cadmpeg_core::text::named_entries(id.as_str(), properties)?,
                pmi: None,
                native_ref: Some(feature_sketch_record_id_in_scan(scan, definition)),
            },
        )?;
        if let Some(feature) = exactly_one(
            ir.model
                .features
                .iter_mut()
                .filter(|feature| feature.id == owner_id),
        ) {
            push_feature_source_parameter(&mut feature.source_content, id)?;
        }
    }
    Ok((transferred, relation_parameters))
}

#[cfg(test)]
mod tests;
