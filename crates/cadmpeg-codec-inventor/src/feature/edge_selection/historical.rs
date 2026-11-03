// SPDX-License-Identifier: Apache-2.0
//! Direct edge operands in the stored input revision namespace.

use std::collections::{HashMap, HashSet};

use cadmpeg_asm::sab::Record;
use cadmpeg_core::{
    decode::{unique_index, DecodeContext, View},
    CodecError,
};
use cadmpeg_ir::{
    features::{
        DistinctMembers, EdgeSelection, Feature, FeatureDefinition, FeatureInputTopology,
        FeatureOperation, HistoricalEdgeGeometry,
    },
    ids::{FeatureInputTopologyId, HistoricalEdgeId},
};

use super::{geometry::EdgeGeometry, PmDcFeaturePropertyKind};
use crate::pmdc::{state_content_header, u32_list_with_context, Cursor, PmDcReference};

pub(super) struct StoredSelector {
    pub(super) source: u32,
    pub(super) generators: Vec<u32>,
    pub(super) key: u32,
    body: PmDcReference,
    item: PmDcReference,
    context: PmDcReference,
}

pub(super) fn parse(
    ctx: &DecodeContext<'_>,
    payload: View<'_>,
    version: u8,
) -> Result<Option<StoredSelector>, CodecError> {
    let mut cursor = Cursor::new(payload);
    let header = state_content_header(&mut cursor, version)?;
    if cursor.i32("edge selector state")? != -1
        || cursor.u8("edge selector enabled")? != 1
        || cursor.u8("edge selector form")? != 5
    {
        return Ok(None);
    }
    let body = cursor.reference("edge selector body")?;
    if cursor.u32("edge selector topology")? != 2
        || (super::selector_has_qualifier(version) && cursor.u32("edge selector qualifier")? != 0)
        || cursor.u32("edge selector naming form")? != 2
    {
        return Ok(None);
    }
    let generators = u32_list_with_context(ctx, &mut cursor, 8, "edge selector generators")?;
    if !matches!(generators.values().len(), 1 | 2)
        || generators.values().contains(&0)
        || generators.values().iter().collect::<HashSet<_>>().len() != generators.values().len()
        || !u32_list_with_context(ctx, &mut cursor, 8, "edge selector secondary names")?
            .values()
            .is_empty()
        || cursor.u8("edge selector suffix form")? != 0
    {
        return Ok(None);
    }
    let item = cursor.reference("edge selector item")?;
    let context = cursor.reference("edge selector context")?;
    let key = cursor.u32("edge selector key")?;
    cursor.finish("edge selector")?;
    let mut names = ctx.alloc_filled(
        generators.values().len(),
        0,
        "retain Inventor edge selector generators",
    )?;
    names.copy_from_slice(generators.values());
    Ok(Some(StoredSelector {
        source: header.source_index,
        generators: names,
        key,
        body,
        item,
        context,
    }))
}

pub(super) fn source(payload: View<'_>, version: u8) -> Result<u32, CodecError> {
    Ok(state_content_header(&mut Cursor::new(payload), version)?.source_index)
}

pub(super) fn links(selector: &StoredSelector, type_at: impl Fn(u32) -> Option<[u8; 16]>) -> bool {
    let target = |reference: PmDcReference, expected| {
        reference.qualified() && reference.record_ordinal().and_then(&type_at) == Some(expected)
    };
    target(selector.body, crate::pmdc::inventor_id(0x9087_4d47))
        && target(selector.item, super::super::EDGE_ITEM_TYPE)
        && target(
            selector.context,
            [
                0x42, 0x20, 0xe9, 0xcc, 0xd1, 0x11, 0xc5, 0x71, 0, 8, 0xa7, 0xba, 0x32, 0xa3, 0xdc,
                9,
            ],
        )
}

#[derive(Debug)]
pub(in crate::feature) struct SelectedEdge {
    pub(super) owner: i64,
    pub(super) item: u32,
    pub(super) geometry: EdgeGeometry,
}

pub(crate) const fn supported_schema(schema: u32) -> bool {
    matches!(schema, 38 | 40 | 42 | 44)
}

pub(super) fn selected(
    records: &[Record],
    selectors: &[StoredSelector],
    schema: u32,
) -> HashMap<u32, SelectedEdge> {
    if !supported_schema(schema) {
        return HashMap::new();
    }
    let by_index = unique_index(
        records
            .iter()
            .filter_map(|record| Some((i64::try_from(record.index).ok()?, record))),
    );
    let by_index = by_index
        .into_iter()
        .filter_map(|(index, record)| Some((index, record?)))
        .collect::<HashMap<_, _>>();
    let mut matches = HashMap::<(u32, u32), HashSet<i64>>::new();
    for record in records {
        if let Some((source, key, owner)) = super::matched(record, schema) {
            matches.entry((source, key)).or_default().insert(owner);
        }
    }
    let selectors = unique_index(selectors.iter().map(|selector| (selector.source, selector)));
    unique_index(selectors.values().filter_map(|selector| {
        let selector = selector.as_ref()?;
        let owners = matches.get(&(selector.source, selector.key))?;
        if owners.len() != 1 {
            return None;
        }
        let owner = *owners.iter().next()?;
        let geometry = super::geometry::line(by_index.get(&owner)?, &by_index)?;
        Some((
            selector.source,
            SelectedEdge {
                owner,
                item: selector.item.record_ordinal()?,
                geometry,
            },
        ))
    }))
    .into_iter()
    .filter_map(|(source, edge)| Some((source, edge?)))
    .collect()
}

fn controls<'a>(
    feature: &Feature,
    source: &crate::feature::PmDcFeature,
    corner: bool,
    property: impl Fn(PmDcReference) -> Option<&'a crate::feature::PmDcFeatureProperty>,
) -> bool {
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Fillet { .. }) => {
            crate::feature::fillet::explicit_selection_controls(source, corner, property)
        }
        FeatureDefinition::Operation(FeatureOperation::Chamfer { .. }) => {
            crate::feature::chamfer::explicit_selection_controls(source, property)
        }
        _ => false,
    }
}

fn state_id(source: &crate::feature::PmDcFeature) -> Result<FeatureInputTopologyId, CodecError> {
    Ok(FeatureInputTopologyId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-input"),
        source.identity.key()?,
    ))
}

fn edge_id(token: &str, source: u32) -> Result<HistoricalEdgeId, CodecError> {
    HistoricalEdgeId::mint(format!("inventor:input-edge#{token}-{source}"))
        .map_err(CodecError::malformed)
}

pub(crate) fn selection_sources<'a>(
    collection: &crate::feature::PmDcFeatureProperty,
    property: impl Fn(PmDcReference) -> Option<&'a crate::feature::PmDcFeatureProperty>,
) -> Option<Vec<(u32, u32)>> {
    let PmDcFeaturePropertyKind::References {
        family: crate::feature::PmDcFeatureReferenceFamily::EdgeCollection,
        items,
    } = &collection.kind
    else {
        return None;
    };
    if items.references().is_empty() {
        return None;
    }
    let mut sources = Vec::new();
    for reference in items.references() {
        let item = property(*reference)?;
        let PmDcFeaturePropertyKind::EdgeItem {
            index_references,
            index_reference_value: -1,
            value: 0,
        } = &item.kind
        else {
            return None;
        };
        if index_references.values().is_empty() {
            return None;
        }
        for source in index_references.values() {
            if sources.iter().any(|(_, previous)| previous == source) {
                return None;
            }
            sources.push((item.identity.record_ordinal, *source));
        }
    }
    Some(sources)
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    features: &mut [Feature],
    inventory: &crate::feature::FeatureInventory,
) -> Result<Vec<FeatureInputTopology>, CodecError> {
    if inventory.edge_inputs.historical.is_empty() {
        return Ok(Vec::new());
    }
    let sources = unique_index(
        inventory
            .features
            .iter()
            .map(|source| (source.id(), source)),
    );
    let properties = unique_index(
        inventory
            .properties
            .iter()
            .map(|property| (property.id(), property)),
    );
    let by_ordinal = unique_index(inventory.properties.iter().map(|property| {
        (
            (
                property.identity.segment_token.as_str(),
                property.identity.record_ordinal,
            ),
            property,
        )
    }));
    let mut states = Vec::new();
    for feature in features {
        let Some(Some(source)) = feature
            .native_ref
            .as_deref()
            .and_then(|native| sources.get(native))
        else {
            continue;
        };
        let token = source.identity.segment_token.as_str();
        let mut labels = inventory.labels.iter().filter(|label| {
            label.identity.segment_token.as_str() == token
                && label.header.owner.record_ordinal() == Some(source.identity.record_ordinal)
        });
        let Some(label) = labels.next() else {
            continue;
        };
        if labels.next().is_some() {
            continue;
        }
        let corner = label.class_id() == crate::feature::CORNER_ROUND_CLASS_ID;
        if !controls(feature, source, corner, |r| {
            by_ordinal
                .get(&(token, r.record_ordinal()?))?
                .as_ref()
                .copied()
        }) {
            continue;
        }
        let state = state_id(source)?;
        let mut geometry = Vec::new();
        let mut owners = HashSet::new();
        let mut replacements = Vec::new();
        let mut revision_sources = HashMap::new();
        let selections = selections(feature);
        for selection in selections {
            let EdgeSelection::Native(native) = selection else {
                continue;
            };
            let Some(Some(collection)) = properties.get(native.as_str()) else {
                continue;
            };
            let Some(selected) = selection_sources(collection, |r| {
                by_ordinal
                    .get(&(token, r.record_ordinal()?))?
                    .as_ref()
                    .copied()
            }) else {
                continue;
            };
            let samples = selected
                .iter()
                .map(|(item, source)| {
                    let edge = inventory
                        .edge_inputs
                        .historical
                        .get(&(token.to_owned(), *source))?;
                    (edge.item == *item).then_some((*source, edge))
                })
                .collect::<Option<Vec<_>>>();
            let Some(samples) = samples else {
                continue;
            };
            let mut group_owners = HashSet::new();
            if samples.iter().any(|(source, edge)| {
                !group_owners.insert(edge.owner)
                    || revision_sources
                        .get(&edge.owner)
                        .is_some_and(|previous| previous != source)
            }) {
                continue;
            }
            let mut edges = Vec::new();
            for (selected_source, edge) in samples {
                revision_sources.insert(edge.owner, selected_source);
                let id = edge_id(token, selected_source)?;
                if owners.insert(id.clone()) {
                    geometry.push(HistoricalEdgeGeometry {
                        edge: id.clone(),
                        start: Some(edge.geometry.start),
                        end: Some(edge.geometry.end),
                        interior: edge.geometry.interior,
                        tangent: Some(edge.geometry.tangent),
                        faces: DistinctMembers::default(),
                    });
                }
                edges.push(id);
            }
            replacements.push((
                native.clone(),
                EdgeSelection::historical(state.clone(), edges, native.clone())
                    .map_err(CodecError::malformed)?,
            ));
        }
        if replacements.is_empty() {
            continue;
        }
        feature.evaluation.edit(|definition, _| {
            let replace = |selection: &mut EdgeSelection| {
                if let EdgeSelection::Native(native) = selection {
                    if let Some((_, replacement)) = replacements.iter().find(|(id, _)| id == native)
                    {
                        *selection = replacement.clone();
                    }
                }
            };
            match definition {
                FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
                    for group in groups.iter_mut() {
                        replace(&mut group.edges);
                    }
                }
                FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) => {
                    for group in groups.iter_mut() {
                        replace(&mut group.edges);
                    }
                }
                _ => {}
            }
        });
        ctx.charge_entities(
            1 + cadmpeg_core::decode::u64_from_index(geometry.len()),
            "project Inventor historical treatment edges",
        )?;
        states.push(FeatureInputTopology {
            id: state,
            input_of: feature.id.clone(),
            bodies: DistinctMembers::default(),
            faces: DistinctMembers::default(),
            edges: geometry
                .iter()
                .map(|sample| sample.edge.clone())
                .collect::<Vec<_>>()
                .try_into()
                .map_err(CodecError::malformed)?,
            vertices: DistinctMembers::default(),
            edge_geometry: geometry,
            native_ref: Some(source.id()),
        });
    }
    Ok(states)
}

fn selections(feature: &Feature) -> Vec<&EdgeSelection> {
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
            groups.iter().map(|g| &g.edges).collect()
        }
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) => {
            groups.iter().map(|g| &g.edges).collect()
        }
        _ => Vec::new(),
    }
}

pub(crate) fn valid<'a>(
    feature: &Feature,
    source: &crate::feature::PmDcFeature,
    states: &[FeatureInputTopology],
    carrier: &crate::native::ActiveCarrierRecord,
    corner: bool,
    property: impl Fn(&str) -> Option<&'a crate::feature::PmDcFeatureProperty>,
    property_at: impl Fn(PmDcReference) -> Option<&'a crate::feature::PmDcFeatureProperty>,
) -> bool {
    selections(feature).iter().all(|selection| {
        let EdgeSelection::Historical {
            state,
            edges,
            native,
        } = selection
        else {
            return true;
        };
        if !matches!(carrier, crate::native::ActiveCarrierRecord::Selected { family: crate::kernel::KernelFamily::Asm, schema, .. } if supported_schema(*schema)) {
            return false;
        }
        if !controls(feature, source, corner, &property_at) { return false; }
        let Some(collection) = property(native.as_str()) else {
            return false;
        };
        if collection.identity.segment_token != source.identity.segment_token {
            return false;
        }
        let Some(sources) = selection_sources(collection, &property_at) else {
            return false;
        };
        let Ok(expected_state) = state_id(source) else {
            return false;
        };
        let expected = sources
            .iter()
            .map(|(_, index)| edge_id(source.identity.segment_token.as_str(), *index))
            .collect::<Result<Vec<_>, _>>();
        let Ok(expected) = expected else {
            return false;
        };
        let mut matches = states
            .iter()
            .filter(|row| row.id == *state && row.input_of == feature.id);
        let Some(row) = matches.next() else {
            return false;
        };
        *state == expected_state
            && edges.as_slice() == expected
            && matches.next().is_none()
            && row.native_ref.as_deref() == Some(source.id().as_str())
            && edges.iter().all(|edge| {
                row.edges.contains(edge)
                    && row
                        .edge_geometry
                        .iter()
                        .filter(|sample| {
                            sample.edge == *edge
                                && sample.start.is_some()
                                && sample.end.is_some()
                                && sample.tangent.is_some()
                        })
                        .count()
                        == 1
            })
    })
}

pub(super) fn native_losses(features: &[Feature]) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let count = features
        .iter()
        .filter(|feature| {
            selections(feature).iter().any(|selection| {
                matches!(
                    selection,
                    EdgeSelection::Native(_) | EdgeSelection::Unresolved
                )
            })
        })
        .count();
    if count == 0 {
        Vec::new()
    } else {
        vec![crate::loss::InventorLossCode::FeatureEdgeSelectionNative.note(format!("Retained {count} edge-treatment feature(s) with unresolved input-edge revision joins or construction controls."))]
    }
}

#[cfg(test)]
pub(crate) mod tests;
