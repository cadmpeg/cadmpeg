// SPDX-License-Identifier: Apache-2.0
//! Separate attached-plate and bend history operations.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::{
    decode::{unique_index, DecodeContext},
    CodecError,
};
use cadmpeg_ir::features::{
    DistinctMembers, EdgeSelection, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation,
    FeatureOperation, FeatureResultTopology,
};

use super::{flange::Flange, operands, style::Style};
use crate::feature::{
    feature_result, length_reference, resolve_property, FeatureInventory, HistoryItem, PmDcFeature,
    PmDcFeatureLabel, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, ProjectionIndex,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Members {
    pub(crate) sketch: u32,
    pub(crate) plate: u32,
    pub(crate) bend: u32,
    pub(crate) corner: u32,
}

pub(crate) fn members(
    flange: &Flange,
    features: &[PmDcFeature],
    labels: &[PmDcFeatureLabel],
    is_sketch: impl Fn(u32) -> bool,
) -> Option<Members> {
    let token = flange.identity.segment_token.as_str();
    let label_of = |record| {
        let mut found = labels.iter().filter(|label| {
            label.identity.segment_token.as_str() == token
                && label.header.owner.record_ordinal() == Some(record)
        });
        let first = found.next()?;
        found.next().is_none().then_some(first)
    };
    let [sketch, plate, bend, corner] = label_of(flange.identity.record_ordinal)?
        .participants
        .references()
    else {
        return None;
    };
    let children = flange.children.references();
    if children.len() != 3
        || !children
            .iter()
            .zip([plate, bend, corner])
            .all(|(a, b)| operands::same(*a, *b))
    {
        return None;
    }
    let (sketch, plate, bend, corner) = (
        sketch.record_ordinal()?,
        plate.record_ordinal()?,
        bend.record_ordinal()?,
        corner.record_ordinal()?,
    );
    if !is_sketch(sketch)
        || ![
            (plate, crate::feature::PLATE_CLASS_ID),
            (bend, super::BEND_CLASS),
            (corner, super::CORNER_CLASS),
        ]
        .into_iter()
        .all(|(record, class)| {
            features
                .iter()
                .filter(|feature| {
                    feature.identity.segment_token.as_str() == token
                        && feature.identity.record_ordinal == record
                })
                .count()
                == 1
                && label_of(record).is_some_and(|label| label.class_id() == class)
        })
    {
        return None;
    }
    Some(Members {
        sketch,
        plate,
        bend,
        corner,
    })
}

pub(in crate::feature) fn profiles<'a>(
    inventory: &'a FeatureInventory,
    sketches: &HashSet<(&str, u32)>,
) -> HashMap<(&'a str, u32), u32> {
    unique_index(inventory.sheet_metal_flanges.iter().filter_map(|flange| {
        let token = flange.identity.segment_token.as_str();
        let member = members(flange, &inventory.features, &inventory.labels, |ordinal| {
            sketches.contains(&(token, ordinal))
        })?;
        Some(((token, member.plate), member.sketch))
    }))
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key, value)))
    .collect()
}

pub(crate) fn style<'a>(styles: &'a [Style], token: &str) -> Option<&'a Style> {
    let mut candidates = styles
        .iter()
        .filter(|style| style.identity.segment_token.as_str() == token);
    let first = candidates.next()?;
    candidates.next().is_none().then_some(first)
}

fn attachment(source: &PmDcFeature, index: &ProjectionIndex<'_>) -> Option<EdgeSelection> {
    let token = source.identity.segment_token.as_str();
    let selection = crate::feature::slot_property(source, 12, index)?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::SheetMetalAttachmentSelections,
        items,
    } = &selection.kind
    else {
        return None;
    };
    let mut edges = Vec::new();
    for reference in items.references() {
        let wrapper = resolve_property(token, reference.index(), index)?;
        let super::SheetMetalProperty::EdgeSelection { edges: collection } =
            operands::sheet(wrapper)?
        else {
            return None;
        };
        let collection = resolve_property(token, collection.index(), index)?;
        if !matches!(
            collection.kind,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                ..
            }
        ) {
            return None;
        }
        let EdgeSelection::Generated {
            edges: resolved, ..
        } = crate::feature::edge_selection::resolve(collection, index)
        else {
            return None;
        };
        for edge in resolved {
            if edges.contains(&edge) {
                return None;
            }
            edges.push(edge);
        }
    }
    EdgeSelection::generated(edges, selection.id()).ok()
}

pub(in crate::feature) fn project_plate(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
    inventory: &FeatureInventory,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let token = source.identity.segment_token.as_str();
    let attachment = attachment(source, index)?;
    let inputs = operands::selection_sources(source, |reference| {
        resolve_property(token, reference.index(), index)
    })?;
    if inputs.iter().any(|source| {
        !inventory
            .edge_inputs
            .geometry
            .contains_key(&(token.to_owned(), *source))
    }) {
        return None;
    }
    let sketch_ordinal = *index
        .plate_sketches
        .get(&(token, source.identity.record_ordinal))?;
    let sketch = index.sketches.get(&(token, sketch_ordinal))?.as_ref()?;
    let sketch_id = index.sketch_ids.get(sketch.id().as_str())?;
    let sketch = index.sketches_by_id.get(sketch_id.as_str())?;
    let profile = index.profiles.whole_sketch(token, sketch_ordinal, sketch)?;
    let (_, normal, u_axis) = sketch.resolved_placement()?;
    let values = plate_values(source, normal, u_axis, index, inventory)?;
    let (id, result) = match feature_result(ctx, source, 15, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = ctx.charge_entities(1, "project Inventor attached plate") {
        return Some(Err(error));
    }
    Some(Ok((
        Feature {
            id,
            ordinal: u64::from(label.index),
            name: label.name().map(str::to_owned),
            suppressed: None,
            needs_recompute: false,
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::default(),
            source_tag: Some("sheet_metal_attached_plate".into()),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::SheetMetalAttachedPlate {
                    profile,
                    thickness: values.thickness,
                    side: values.side,
                    attachment,
                },
            )),
            native_ref: Some(source.id()),
        },
        result,
    )))
}

fn plate_values(
    source: &PmDcFeature,
    normal: cadmpeg_ir::features::FiniteVector3,
    u_axis: cadmpeg_ir::features::FiniteVector3,
    index: &ProjectionIndex<'_>,
    inventory: &FeatureInventory,
) -> Option<operands::PlateOperands> {
    let token = source.identity.segment_token.as_str();
    operands::plate(
        source,
        style(&inventory.sheet_metal_styles, token)?,
        |reference| resolve_property(token, reference.index(), index),
        |reference| length_reference(token, reference.index(), index),
        |reference| {
            let record = index
                .directions
                .get(&(token, reference.record_ordinal()?))?
                .as_ref()?;
            cadmpeg_ir::units::UnitVector3::normalized(cadmpeg_ir::math::Vector3::new(
                record.direction[0].get(),
                record.direction[1].get(),
                record.direction[2].get(),
            ))
        },
        cadmpeg_ir::units::UnitVector3::normalized(*normal.as_raw())?,
        cadmpeg_ir::units::UnitVector3::normalized(*u_axis.as_raw())?,
    )
}

pub(crate) fn bend_definition<'a>(
    plate: &Feature,
    plate_source: &PmDcFeature,
    bend_source: &PmDcFeature,
    values: &operands::PlateOperands,
    property: impl Fn(crate::pmdc::PmDcReference) -> Option<&'a crate::feature::PmDcFeatureProperty>,
) -> Option<FeatureOperation> {
    if plate.native_ref.as_deref() != Some(plate_source.id().as_str())
        || !operands::bend(plate_source, bend_source, property)
    {
        return None;
    }
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalAttachedPlate {
        attachment:
            attachment @ (EdgeSelection::Generated { .. } | EdgeSelection::Historical { .. }),
        ..
    }) = plate.evaluation.definition()
    else {
        return None;
    };
    Some(FeatureOperation::SheetMetalBend {
        plate: plate.id.clone(),
        attachment: attachment.clone(),
        inside_radius: values.inside_radius,
        relief: values.relief.clone(),
        transition: values.transition.clone(),
    })
}

pub(in crate::feature) fn reject_unresolved_inputs(
    ctx: &DecodeContext<'_>,
    projected: &mut [HistoryItem<'_>],
    inventory: &FeatureInventory,
) -> Result<usize, CodecError> {
    let producers: HashMap<_, _> = projected
        .iter()
        .filter(|item| item.neutral && item.result.is_some())
        .map(|item| (item.feature.id.as_str().to_owned(), item.feature.ordinal))
        .collect();
    let mut rejected = 0;
    for item in projected {
        let FeatureDefinition::Operation(FeatureOperation::SheetMetalAttachedPlate {
            attachment,
            ..
        }) = item.feature.evaluation.definition()
        else {
            continue;
        };
        let complete = matches!(attachment,EdgeSelection::Generated { edges,.. } if edges.iter().all(|edge| producers.get(edge.feature.as_str()).is_some_and(|ordinal| *ordinal<item.feature.ordinal)));
        if complete {
            continue;
        }
        retain_native(ctx, item, inventory)?;
        rejected += 1;
    }
    Ok(rejected)
}

fn retain_native(
    ctx: &DecodeContext<'_>,
    item: &mut HistoryItem<'_>,
    inventory: &FeatureInventory,
) -> Result<(), CodecError> {
    let source = inventory
        .features
        .iter()
        .find(|source| Some(source.id().as_str()) == item.feature.native_ref.as_deref())
        .ok_or_else(|| {
            CodecError::malformed("Inventor sheet-metal feature has no source record")
        })?;
    let mut native =
        crate::feature::native_feature(ctx, &source.identity, item.label, source.id())?;
    native.ordinal = item.feature.ordinal;
    native.suppressed = item.feature.suppressed;
    native.dependencies = item.feature.dependencies.clone();
    native.source_properties = item.feature.source_properties.clone();
    item.feature = native;
    item.result = None;
    item.neutral = false;
    Ok(())
}

pub(in crate::feature) fn reject_incomplete_pairs(
    ctx: &DecodeContext<'_>,
    projected: &mut [HistoryItem<'_>],
    inventory: &FeatureInventory,
) -> Result<usize, CodecError> {
    let pairs: HashSet<_> = projected.iter().filter_map(|plate| {
        if !plate.neutral || !matches!(plate.feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::SheetMetalAttachedPlate { .. })) {
            return None;
        }
        let mut bends = projected.iter().filter(|bend| bend.neutral && bend.feature.ordinal > plate.feature.ordinal
            && bend.feature.dependencies.contains(&plate.feature.id)
            && matches!(bend.feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::SheetMetalBend { plate: target, .. }) if *target == plate.feature.id));
        let bend = bends.next()?;
        bends.next().is_none().then_some((plate.feature.id.clone(), bend.feature.id.clone()))
    }).collect();
    let mut rejected = 0;
    for item in projected {
        let complete = match item.feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::SheetMetalAttachedPlate { .. }) => {
                pairs.iter().any(|(plate, _)| *plate == item.feature.id)
            }
            FeatureDefinition::Operation(FeatureOperation::SheetMetalBend { .. }) => {
                pairs.iter().any(|(_, bend)| *bend == item.feature.id)
            }
            _ => continue,
        };
        if !complete {
            retain_native(ctx, item, inventory)?;
            rejected += 1;
        }
    }
    Ok(rejected)
}

pub(in crate::feature) fn project_bends(
    ctx: &DecodeContext<'_>,
    projected: &mut [HistoryItem<'_>],
    index: &ProjectionIndex<'_>,
    inventory: &FeatureInventory,
) -> Result<usize, CodecError> {
    let sketches: HashSet<_> = index.sketches.keys().copied().collect();
    let pairs = unique_index(inventory.sheet_metal_flanges.iter().filter_map(|flange| {
        let token = flange.identity.segment_token.as_str();
        let member = members(flange, &inventory.features, &inventory.labels, |ordinal| {
            sketches.contains(&(token, ordinal))
        })?;
        Some(((token, member.bend), member.plate))
    }));
    let mut added = 0;
    for ((token, bend), plate) in pairs {
        let Some(plate) = plate else {
            continue;
        };
        let Some(plate_source) = inventory.features.iter().find(|source| {
            source.identity.segment_token.as_str() == token
                && source.identity.record_ordinal == plate
        }) else {
            continue;
        };
        let Some(bend_source) = inventory.features.iter().find(|source| {
            source.identity.segment_token.as_str() == token
                && source.identity.record_ordinal == bend
        }) else {
            continue;
        };
        let Some(plate_position) = projected.iter().position(|item| {
            item.feature.native_ref.as_deref() == Some(plate_source.id().as_str()) && item.neutral
        }) else {
            continue;
        };
        let Some(bend_position) = projected.iter().position(|item| {
            item.feature.native_ref.as_deref() == Some(bend_source.id().as_str()) && !item.neutral
        }) else {
            continue;
        };
        if projected[plate_position].feature.ordinal >= projected[bend_position].feature.ordinal {
            continue;
        }
        let FeatureDefinition::Operation(FeatureOperation::SheetMetalAttachedPlate {
            profile: cadmpeg_ir::features::PlanarProfileRef::SketchRegions { sketch, .. },
            ..
        }) = projected[plate_position].feature.evaluation.definition()
        else {
            continue;
        };
        let Some((_, normal, u_axis)) = index
            .sketches_by_id
            .get(sketch.as_str())
            .and_then(|s| s.resolved_placement())
        else {
            continue;
        };
        let Some(values) = plate_values(plate_source, normal, u_axis, index, inventory) else {
            continue;
        };
        let Some(definition) = bend_definition(
            &projected[plate_position].feature,
            plate_source,
            bend_source,
            &values,
            |reference| resolve_property(token, reference.index(), index),
        ) else {
            continue;
        };
        let Some(result) = feature_result(ctx, bend_source, 9, index) else {
            continue;
        };
        let (id, result) = result?;
        if id != projected[bend_position].feature.id {
            return Err(CodecError::malformed(
                "Inventor bend history identity differs from its result",
            ));
        }
        let plate_id = projected[plate_position].feature.id.clone();
        let item = &mut projected[bend_position];
        item.feature.evaluation =
            FeatureEvaluation::from_definition(FeatureDefinition::Operation(definition));
        item.feature.dependencies.insert(plate_id);
        item.feature.source_tag = Some("sheet_metal_bend".into());
        item.result = Some(result);
        item.neutral = true;
        added += 1;
    }
    Ok(added)
}

#[cfg(test)]
mod tests;
