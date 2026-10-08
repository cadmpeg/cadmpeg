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

fn insert_dimension_property(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    node_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    properties: &mut BTreeMap<String, String>,
    key: &'static str,
    value: impl std::fmt::Display,
) -> Result<(), cadmpeg_core::CodecError> {
    let key = ctx.copy_retained_text(key, "creo dimension property key")?;
    let value = ctx.format_retained(format_args!("{value}"), "creo dimension property value")?;
    node_storage.with_storage(|| {
        ctx.insert_btree_map(properties, key, value, "creo dimension property nodes")
    })?;
    Ok(())
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

fn dimension_expression(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: Option<f64>,
) -> Result<String, cadmpeg_core::CodecError> {
    value
        .map(|value| ctx.format_retained(format_args!("{value}"), "creo dimension expression"))
        .transpose()
        .map(std::option::Option::unwrap_or_default)
}

fn push_feature_source_parameter(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    content: &mut cadmpeg_ir::features::FeatureContent,
    id: ParameterId,
) -> Result<(), cadmpeg_core::CodecError> {
    content
        .push(
            FeatureSourceContent::Parameter(id),
            ctx,
            "creo feature source content",
        )
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &SketchId,
    external_id: u32,
    occurrence: Option<usize>,
) -> Result<Option<ParameterId>, cadmpeg_core::CodecError> {
    let text = if let Some(occurrence) = occurrence {
        let ordinal = occurrence.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("creo dimension parameter identity", u64::MAX, u64::MAX)
        })?;
        ctx.format_retained(
            format_args!(
                "creo:featdefs:parameter#{}:{external_id}:{ordinal}",
                sketch_identity_scope(sketch),
            ),
            "creo dimension parameter identity",
        )?
    } else {
        ctx.format_retained(
            format_args!(
                "creo:featdefs:parameter#{}:{external_id}",
                sketch_identity_scope(sketch),
            ),
            "creo dimension parameter identity",
        )?
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(text.len()),
        "creo dimension parameter identity validation",
    )?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
    if crate::decode::uniqueness::exactly_one_by(
        ctx,
        &table.rows,
        |candidate| Ok(candidate.external_id == dimension.external_id),
        "creo admitted dimension uniqueness",
    )?
    .is_none()
    {
        return Ok(None);
    }
    Ok(
        feature_dimension_parameter_row_id_admitted(ctx, sketch, dimension.external_id, None)?
            .map(|parameter| (dimension, parameter)),
    )
}

pub(in super::super) fn planned_feature_dimension_parameter_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<BTreeSet<ParameterId>, cadmpeg_core::CodecError> {
    let mut ids = BTreeSet::new();
    for definition in ctx.admit_iter(
        &scan.features.definitions,
        "creo planned dimension definitions",
    )? {
        let Some(table) = &definition.dimensions else {
            continue;
        };
        let Some(sketch) = model_sketch_id(ctx, scan, definition)? else {
            continue;
        };
        if !feature_dimension_table_complete(table) {
            continue;
        }
        let mut scratch = ctx.reserve_scoped(0, "creo planned dimension identity index")?;
        let mut counts = std::collections::HashMap::<u32, usize>::new();
        for dimension in ctx.admit_iter(&table.rows, "creo planned dimension ID uniqueness")? {
            *scratch
                .with_storage(|| {
                    ctx.entry_hash_map(
                        &mut counts,
                        dimension.external_id,
                        "creo planned dimension identity count nodes",
                    )
                })?
                .or_default() += 1;
        }
        for dimension in ctx.admit_iter(&table.rows, "creo planned dimension rows")? {
            if counts.get(&dimension.external_id) != Some(&1) {
                continue;
            }
            let text = ctx.format_retained(
                format_args!(
                    "creo:featdefs:parameter#{}:{}",
                    crate::decode::sketch_ids::sketch_identity_scope(&sketch),
                    dimension.external_id,
                ),
                "creo planned dimension parameter identity",
            )?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(text.len()),
                "creo planned dimension parameter identity validation",
            )?;
            let Ok(parameter) = ParameterId::try_from(text) else {
                continue;
            };
            ctx.insert_btree_set(
                &mut ids,
                parameter,
                "creo planned dimension parameter ID nodes",
            )?;
        }
    }
    Ok(ids)
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

#[derive(Debug)]
pub(in super::super) struct DimensionParameterLayout<'ctx> {
    pub(in super::super) rows: Vec<(u32, String, Option<usize>)>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(in super::super) fn feature_dimension_parameter_layout<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    keys: &[(SketchId, u32)],
) -> Result<Option<DimensionParameterLayout<'ctx>>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo dimension layout scratch")?;
    let mut local_counts = BTreeMap::<(&SketchId, u32), usize>::new();
    for (sketch, external_id) in ctx.admit_iter(keys, "creo dimension layout keys")? {
        let key = (sketch, *external_id);
        *scratch
            .with_storage(|| {
                ctx.entry_btree_map(&mut local_counts, key, "creo dimension layout count nodes")
            })?
            .or_insert(0) += 1;
    }
    let mut next_ordinals = BTreeMap::<&SketchId, u32>::new();
    let mut local_occurrences = BTreeMap::<(&SketchId, u32), usize>::new();
    let mut layout = Vec::new();
    let mut layout_storage = ctx.reserve_scoped(0, "creo dimension layout rows")?;
    layout_storage.with_storage(|| {
        ctx.reserve_vec(&mut layout, keys.len(), "creo dimension parameter layout")
    })?;
    let mut items = keys.iter();
    while let Some((sketch, external_id)) =
        ctx.next_charged(&mut items, "creo dimension layout entries")?
    {
        let ordinal = scratch
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut next_ordinals,
                    sketch,
                    "creo dimension layout ordinal nodes",
                )
            })?
            .or_default();
        let assigned = *ordinal;
        let Some(next) = ordinal.checked_add(1) else {
            return Ok(None);
        };
        *ordinal = next;
        let key = (sketch, *external_id);
        let count = ctx
            .get_btree_map(&local_counts, &key, "creo dimension layout count lookup")?
            .copied()
            .unwrap_or_default();
        let occurrence = if count > 1 {
            let next = scratch
                .with_storage(|| {
                    ctx.entry_btree_map(
                        &mut local_occurrences,
                        key,
                        "creo dimension layout occurrence nodes",
                    )
                })?
                .or_insert(0);
            let assigned = *next;
            *next += 1;
            Some(assigned)
        } else {
            None
        };
        let name = if count == 1 {
            ctx.format_retained(
                format_args!("d{external_id}"),
                "creo dimension parameter name",
            )?
        } else if let Some(occurrence) = occurrence {
            ctx.format_retained(
                format_args!(
                    "d{}_{}_{}",
                    sketch_identity_scope(sketch),
                    external_id,
                    occurrence + 1
                ),
                "creo dimension parameter name",
            )?
        } else {
            ctx.format_retained(
                format_args!("d{}_{}", sketch_identity_scope(sketch), external_id),
                "creo dimension parameter name",
            )?
        };
        layout.push((assigned, name, occurrence));
    }
    Ok(Some(DimensionParameterLayout {
        rows: layout,
        storage: layout_storage,
    }))
}

pub(in super::super) fn transfer_feature_dimensions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<(usize, BTreeMap<String, ParameterId>), cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo dimension transfer staging")?;
    let mut feature_ids = BTreeSet::new();
    for feature in ctx.admit_iter(&ir.model.features, "creo dimension owner features")? {
        if !ctx.contains_btree_set(
            &feature_ids,
            &feature.id,
            "creo dimension owner feature ID lookup",
        )? {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut feature_ids,
                    feature
                        .id
                        .try_clone_for_decode(ctx, "creo dimension owner feature IDs")?,
                    "creo dimension owner feature ID nodes",
                )
            })?;
        }
    }
    let mut candidates = Vec::new();
    for definition in ctx.admit_iter(
        &scan.features.definitions,
        "creo dimension source definitions",
    )? {
        let Some(sketch) = model_sketch_id(ctx, scan, definition)? else {
            continue;
        };
        let Some(owner) = section_owner_feature_id(ctx, scan, definition.identity.id(), &sketch)?
        else {
            continue;
        };
        if !ctx.contains_btree_set(&feature_ids, &owner, "creo dimension owner lookup")? {
            continue;
        }
        let Some(table) = &definition.dimensions else {
            continue;
        };
        for (source_ordinal, dimension) in ctx
            .admit_iter(&table.rows, "creo source dimension rows")?
            .enumerate()
        {
            scratch.with_storage(|| {
                ctx.reserve_vec(&mut candidates, 1, "creo dimension candidates")
            })?;
            candidates.push((
                scratch.with_storage(|| {
                    sketch.try_clone_for_decode(ctx, "creo dimension candidate sketch IDs")
                })?,
                definition,
                source_ordinal,
                dimension,
            ));
        }
    }
    ctx.stable_sort_by_key(
        candidates.as_mut_slice(),
        |value| (value.1.offset, value.1.identity.id(), value.2),
        Ord::cmp,
        "creo transfer feature dimensions candidates ordering",
    )?;
    let mut keys = Vec::new();
    scratch.with_storage(|| {
        ctx.reserve_vec(&mut keys, candidates.len(), "creo dimension layout keys")
    })?;
    for (sketch, _, _, dimension) in
        ctx.admit_iter(&candidates, "creo dimension layout candidates")?
    {
        keys.push((
            scratch.with_storage(|| {
                sketch.try_clone_for_decode(ctx, "creo dimension layout sketch IDs")
            })?,
            dimension.external_id,
        ));
    }
    let Some(layout) = feature_dimension_parameter_layout(ctx, &keys)? else {
        return Ok((0, BTreeMap::new()));
    };
    let mut unique_external_ids = BTreeMap::new();
    for (_, external_id) in ctx.admit_iter(&keys, "creo unique dimension external IDs")? {
        *scratch
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut unique_external_ids,
                    *external_id,
                    "creo unique dimension external ID nodes",
                )
            })?
            .or_insert(0usize) += 1;
    }
    let transferred = layout.rows.len();
    let _layout_storage = layout.storage;
    let mut relation_parameters = BTreeMap::new();
    for ((sketch, definition, source_ordinal, dimension), (ordinal, name, occurrence)) in ctx
        .admit_iter(candidates, "creo dimension candidate transfer")?
        .zip(ctx.admit_iter(layout.rows, "creo dimension layout transfer")?)
    {
        let Some(owner_id) =
            section_owner_feature_id(ctx, scan, definition.identity.id(), &sketch)?
        else {
            continue;
        };
        let Some(id) = feature_dimension_parameter_row_id_admitted(
            ctx,
            &sketch,
            dimension.external_id,
            occurrence,
        )?
        else {
            continue;
        };
        if ctx.get_btree_map(
            &unique_external_ids,
            &dimension.external_id,
            "creo unique dimension external ID lookup",
        )? == Some(&1)
        {
            ctx.insert_btree_map(
                &mut relation_parameters,
                ctx.format_retained(
                    format_args!("d{}", dimension.external_id),
                    "creo relation parameter names",
                )?,
                id.try_clone_for_decode(ctx, "creo relation parameter identities")?,
                "creo relation parameter nodes",
            )?;
        }
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(dimension.offset),
            "section_dimension",
            Exactness::Derived,
        )?;
        let mut property_nodes = ctx.reserve_scoped(0, "creo dimension property nodes")?;
        let mut properties = BTreeMap::new();
        insert_dimension_property(
            ctx,
            &mut property_nodes,
            &mut properties,
            "definition_id",
            definition.identity.id(),
        )?;
        insert_dimension_property(
            ctx,
            &mut property_nodes,
            &mut properties,
            "source_ordinal",
            source_ordinal,
        )?;
        insert_dimension_property(
            ctx,
            &mut property_nodes,
            &mut properties,
            "external_id",
            dimension.external_id,
        )?;
        insert_dimension_property(
            ctx,
            &mut property_nodes,
            &mut properties,
            "dimension_type",
            dimension.dimension_type,
        )?;
        insert_dimension_property(
            ctx,
            &mut property_nodes,
            &mut properties,
            "direction_byte",
            dimension.direction_byte,
        )?;
        if let Some(auxiliary) = dimension.auxiliary_value {
            insert_dimension_property(
                ctx,
                &mut property_nodes,
                &mut properties,
                "auxiliary_value",
                auxiliary,
            )?;
        }
        if dimension.value.resolved().is_none() {
            insert_dimension_property(
                ctx,
                &mut property_nodes,
                &mut properties,
                "value_state",
                "unresolved",
            )?;
        }
        if let Some(token) = dimension.value.unresolved_token() {
            let encoding = match token {
                [0x00, _, _] => Some("three_byte_placeholder"),
                [0x01, _, _, _] => Some("four_byte_placeholder"),
                _ => None,
            };
            if let Some(encoding) = encoding {
                insert_dimension_property(
                    ctx,
                    &mut property_nodes,
                    &mut properties,
                    "value_encoding",
                    encoding,
                )?;
                insert_dimension_property(
                    ctx,
                    &mut property_nodes,
                    &mut properties,
                    "value_token",
                    HexToken(token),
                )?;
            }
        }
        let expression = dimension_expression(ctx, dimension.value.resolved())?;
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
            ctx,
            ir,
            DesignParameter {
                id: id.try_clone_for_decode(ctx, "creo design parameter identity copy")?,
                owner: Some(
                    owner_id.try_clone_for_decode(ctx, "creo design parameter owner identity")?,
                ),
                ordinal,
                name,
                expression,
                display: feature_dimension_display(dimension.dimension_type),
                value,
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                properties: {
                    let properties =
                        cadmpeg_core::text::named_entries_for_decode(ctx, id.as_str(), properties)?;
                    drop(property_nodes);
                    properties
                },
                pmi: None,
                native_ref: Some(feature_sketch_record_id_in_scan(ctx, scan, definition)?),
            },
        )?;
        let mut owner_index = None;
        let mut items = ir.model.features.iter().enumerate();
        while let Some((index, feature)) =
            ctx.next_charged(&mut items, "creo dimension owner feature lookup")?
        {
            if !ctx.equal(
                &feature.id,
                &owner_id,
                "creo dimension owner feature identity comparison",
            )? {
                continue;
            }
            if owner_index.is_some() {
                owner_index = None;
                break;
            }
            owner_index = Some(index);
        }
        if let Some(index) = owner_index {
            push_feature_source_parameter(ctx, &mut ir.model.features[index].source_content, id)?;
        }
    }
    Ok((transferred, relation_parameters))
}

#[cfg(test)]
mod tests;
