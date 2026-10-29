// SPDX-License-Identifier: Apache-2.0
//! Corner operands with explicit native construction controls.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::{decode::unique_index, text::NonBlankString, CodecError};
use cadmpeg_ir::{
    features::{
        EdgeSelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation,
        SheetMetalCornerEffect,
    },
    scalar::Length,
};

use super::{operands, projection, SheetMetalProperty};
use crate::{
    feature::{
        length_reference, resolve_property, FeatureInventory, HistoryItem, PmDcFeature,
        PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, ProjectionIndex,
    },
    pmdc::PmDcReference,
};

#[derive(Debug, PartialEq)]
pub(crate) struct Operands {
    pub(crate) gap: Length,
    pub(crate) size: Length,
    pub(crate) jacobi_radius: Length,
    pub(crate) control: bool,
    pub(crate) option: bool,
    pub(crate) relief_form: (i16, u16),
    pub(crate) secondary_form: (i16, u16),
    pub(crate) corner_form: (i16, u16),
    pub(crate) raw_value: u32,
    pub(crate) selection_sources: Vec<u32>,
}

fn boolean(property: &PmDcFeatureProperty) -> Option<bool> {
    match property.kind {
        PmDcFeaturePropertyKind::Boolean { value, .. } => Some(value),
        _ => None,
    }
}

fn enumeration(property: &PmDcFeatureProperty) -> Option<(i16, u16)> {
    match operands::sheet(property)? {
        SheetMetalProperty::Enumeration { type_value, value } => Some((*type_value, *value)),
        _ => None,
    }
}

pub(crate) fn read<'a>(
    source: &PmDcFeature,
    plate: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    length: impl Fn(PmDcReference) -> Option<Length>,
) -> Option<Operands> {
    let slots = source.properties.references();
    let plate_slots = plate.properties.references();
    if !matches!(slots.len(), 6 | 7)
        || plate_slots.len() < 13
        || source.identity.segment_token != plate.identity.segment_token
        || slots[5].index() != 0
        || (slots.len() == 7 && slots[1].index() != 0)
        || (slots.len() == 6
            && (!operands::same(slots[1], plate_slots[6])
                || !matches!(
                    property(slots[1])?.kind,
                    PmDcFeaturePropertyKind::SurfaceBody { .. }
                )))
        || ![(0, 9), (3, 12), (4, 0)]
            .into_iter()
            .all(|(a, b)| operands::same(slots[a], plate_slots[b]))
        || (slots.len() == 7
            && !matches!(
                property(slots[6])?.kind,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::ObjectCollection,
                    ..
                }
            ))
        || !matches!(
            property(slots[2])?.kind,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FeatureDimensions,
                ..
            }
        )
        || !matches!(
            property(slots[4])?.kind,
            PmDcFeaturePropertyKind::ProfileSelection { .. }
        )
    {
        return None;
    }
    let SheetMetalProperty::Corner {
        control,
        gap,
        size,
        relief_form,
        corner_form,
        secondary_form,
        jacobi_radius,
        value,
        option,
    } = operands::sheet(property(slots[0])?)?
    else {
        return None;
    };
    let relief_form = enumeration(property(*relief_form)?)?;
    let secondary_form = enumeration(property(*secondary_form)?)?;
    let corner_form = enumeration(property(*corner_form)?)?;
    if !matches!(relief_form.0, 304 | 501)
        || !matches!(secondary_form.0, 304 | 501)
        || corner_form.0 != 8
    {
        return None;
    }
    Some(Operands {
        gap: length(*gap)?,
        size: length(*size)?,
        jacobi_radius: length(*jacobi_radius)?,
        control: boolean(property(*control)?)?,
        option: boolean(property(*option)?)?,
        relief_form,
        secondary_form,
        corner_form,
        raw_value: *value,
        selection_sources: operands::selection_sources(plate, property)?,
    })
}

fn insert(
    map: &mut BTreeMap<NonBlankString, String>,
    key: &str,
    value: String,
) -> Result<(), CodecError> {
    map.insert(
        NonBlankString::new(key.to_owned())
            .ok_or_else(|| CodecError::malformed("Inventor corner operand key is blank"))?,
        value,
    );
    Ok(())
}

impl Operands {
    fn parameters(&self) -> Result<BTreeMap<NonBlankString, String>, CodecError> {
        let mut values = BTreeMap::new();
        for (name, value) in [
            ("gap_mm", self.gap.get()),
            ("size_mm", self.size.get()),
            ("jacobi_radius_mm", self.jacobi_radius.get()),
        ] {
            insert(&mut values, name, value.to_string())?;
        }
        for (name, (kind, value)) in [
            ("relief_form", self.relief_form),
            ("secondary_form", self.secondary_form),
            ("corner_form", self.corner_form),
        ] {
            insert(&mut values, name, format!("{kind}:{value}"))?;
        }
        insert(&mut values, "control", self.control.to_string())?;
        insert(&mut values, "option", self.option.to_string())?;
        insert(&mut values, "raw_value", self.raw_value.to_string())?;
        Ok(values)
    }
}

fn unchanged(
    values: &Operands,
    source: u32,
    inventory: &super::super::edge_selection::EdgeInventory,
) -> bool {
    values.selection_sources.len() == 1
        && values.option
        && values.raw_value == 0
        && values.corner_form == (8, 3)
        && matches!(
            (values.control, values.relief_form, values.secondary_form),
            (true, (304, 103), (304, 303))
                | (true, (501, 103), (501, 303))
                | (false, (501, 100), (501, 303))
        )
        && inventory.generated_sources_complete
        && inventory
            .generated_sources
            .as_ref()
            .is_some_and(|names| !names.contains_key(&source))
}

fn active_main(feature: &Feature) -> bool {
    feature.suppressed == Some(false) && !feature.source_properties.contains_key("history")
}

fn result_shared(source: &PmDcFeature, plate: &PmDcFeature) -> bool {
    let slots = source.properties.references();
    let pair = match slots {
        [_, body, _, _, _, _] => (*body, 6),
        [_, _, _, _, _, _, collection] => (*collection, 15),
        _ => return false,
    };
    plate
        .properties
        .references()
        .get(pair.1)
        .is_some_and(|plate_result| operands::same(pair.0, *plate_result))
}

pub(in crate::feature) fn project(
    projected: &mut [HistoryItem<'_>],
    index: &ProjectionIndex<'_>,
    inventory: &FeatureInventory,
) -> Result<usize, CodecError> {
    let mut added = 0;
    let sketches: HashSet<_> = index.sketches.keys().copied().collect();
    let owners = unique_index(inventory.sheet_metal_flanges.iter().filter_map(|flange| {
        let token = flange.identity.segment_token.as_str();
        let members =
            projection::members(flange, &inventory.features, &inventory.labels, |ordinal| {
                sketches.contains(&(token, ordinal))
            })?;
        Some(((token, members.corner), (flange, members)))
    }));
    for ((token, corner), owner) in owners {
        let Some((flange, members)) = owner else {
            continue;
        };
        let source = |record| {
            inventory.features.iter().find(|source| {
                source.identity.segment_token.as_str() == token
                    && source.identity.record_ordinal == record
            })
        };
        let Some((source, plate)) = source(corner).zip(source(members.plate)) else {
            continue;
        };
        let Some(values) = read(
            source,
            plate,
            |reference| resolve_property(token, reference.index(), index),
            |reference| length_reference(token, reference.index(), index),
        ) else {
            continue;
        };
        let dependencies = [members.plate, members.bend]
            .into_iter()
            .filter_map(|record| {
                let item = projected.iter().find(|item| item.record == record)?;
                Some((item.feature.id.clone(), item.feature.ordinal))
            })
            .collect::<Vec<_>>();
        let plate_id = crate::feature::history_feature_id(&plate.identity)?;
        let result_shared = result_shared(source, plate);
        let plate_active = projected
            .iter()
            .any(|item| item.record == members.plate && item.neutral && active_main(&item.feature));
        let bend = projected
            .iter()
            .find(|item| {
                item.record == members.bend
                    && item.neutral
                    && plate_active
                    && active_main(&item.feature)
            })
            .filter(|item| {
                matches!(item.feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::SheetMetalBend {
                    plate, attachment: EdgeSelection::Generated { edges, .. }, ..
                }) if plate == &plate_id && edges.len() == 1)
            })
            .map(|item| (item.feature.id.clone(), item.feature.ordinal));
        let Some(item) = projected
            .iter_mut()
            .find(|item| item.record == corner && !item.neutral)
        else {
            continue;
        };
        let parameters = values
            .parameters()?
            .into_iter()
            .map(|(name, value)| {
                (
                    name,
                    cadmpeg_ir::features::Dimension::Literal(
                        cadmpeg_ir::features::Expression::text(value),
                    ),
                )
            })
            .collect();
        item.feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Native {
                parameters: target,
                ..
            }) = definition
            {
                *target = parameters;
            }
        });
        for (dependency, ordinal) in dependencies {
            if ordinal < item.feature.ordinal {
                item.feature.dependencies.insert(dependency);
            }
        }
        for (name, value) in [
            ("owning_flange", flange.id()),
            ("plate", plate.id()),
            ("bend", source_id(inventory, token, members.bend)?),
            (
                "attachment_edge_sources",
                values
                    .selection_sources
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        ] {
            insert(&mut item.feature.source_properties, name, value)?;
        }
        for (name, slot) in [
            ("corner_control", 0),
            ("result_body", 1),
            ("feature_dimensions", 2),
            ("attachment_selections", 3),
            ("profile_selection", 4),
        ] {
            if let Some(property) =
                resolve_property(token, source.properties.references()[slot].index(), index)
            {
                insert(&mut item.feature.source_properties, name, property.id())?;
            }
        }
        if let Some(reference) = source.properties.references().get(6) {
            if let Some(property) = resolve_property(token, reference.index(), index) {
                insert(
                    &mut item.feature.source_properties,
                    "result_collection",
                    property.id(),
                )?;
            }
        }
        if let Some(sources) = &inventory.edge_inputs.generated_sources {
            insert(
                &mut item.feature.source_properties,
                "generated_names_complete",
                inventory.edge_inputs.generated_sources_complete.to_string(),
            )?;
            let names = sources.get(&source.header.source_index);
            insert(
                &mut item.feature.source_properties,
                "generated_face_name_records",
                names.map_or(0, |names| names.face_revisions).to_string(),
            )?;
            insert(
                &mut item.feature.source_properties,
                "generated_edge_name_records",
                names.map_or(0, |names| names.edge_revisions).to_string(),
            )?;
        }
        if let Some((bend, _)) = bend
            .filter(|(_, ordinal)| *ordinal < item.feature.ordinal && active_main(&item.feature))
        {
            if result_shared
                && unchanged(&values, source.header.source_index, &inventory.edge_inputs)
            {
                for (name, value) in values.parameters()? {
                    item.feature.source_properties.insert(name, value);
                }
                item.feature.evaluation = FeatureEvaluation::from_definition(
                    FeatureDefinition::Operation(FeatureOperation::SheetMetalCorner {
                        bend: bend.clone(),
                        effect: SheetMetalCornerEffect::Unchanged,
                    }),
                );
                item.feature.dependencies.insert(bend);
                item.feature.source_tag = Some("sheet_metal_corner".into());
                item.neutral = true;
                added += 1;
            }
        }
    }
    Ok(added)
}

fn source_id(
    inventory: &FeatureInventory,
    token: &str,
    ordinal: u32,
) -> Result<String, CodecError> {
    inventory
        .features
        .iter()
        .find(|source| {
            source.identity.segment_token.as_str() == token
                && source.identity.record_ordinal == ordinal
        })
        .map(PmDcFeature::id)
        .ok_or_else(|| CodecError::malformed("Inventor corner bend has no source record"))
}

#[cfg(test)]
mod tests;
