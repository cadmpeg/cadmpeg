// SPDX-License-Identifier: Apache-2.0
//! Source-declared field roles for configuration-row intervals.

use super::entity_record::CatiaEntityRecord;
use super::paired_records::PairedRecords;
use super::schema_configuration_chain::CatiaSchemaConfigurationRowChain;
use super::{CatiaEntityReference, CatiaEntityValueSchemaSelection, CatiaObjectRecord};
use crate::object_graph::PayloadField;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::Entry, HashMap, HashSet};

const RETAIN_ROLE: &str = "retain CATIA schema-configuration field roles";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaSchemaFieldIncidence {
    pub(crate) source_entity: String,
    pub(crate) source_object: String,
    pub(crate) schema_ordinal: u32,
    pub(crate) schema_entry: String,
    pub(crate) schema_name: String,
    pub(crate) declaration_offsets: Vec<u64>,
    pub(crate) selector_payload_offset: u64,
    pub(crate) target_payload_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaSchemaConfigurationFieldRole {
    pub(crate) target: CatiaEntityReference,
    pub(crate) target_object: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_role_name"
    )]
    pub(crate) name: Option<String>,
    pub(crate) incidences: Vec<CatiaSchemaFieldIncidence>,
}

struct Declaration<'a> {
    selection: &'a CatiaEntityValueSchemaSelection,
    offsets: Vec<u64>,
}

fn declarations(entity: &CatiaEntityRecord) -> HashMap<u32, Option<Declaration<'_>>> {
    let mut index: HashMap<u32, Option<Declaration<'_>>> = HashMap::new();
    for selection in &entity.value_schema_selections {
        match index.entry(selection.ordinal) {
            Entry::Vacant(slot) => {
                slot.insert(Some(Declaration {
                    selection,
                    offsets: vec![selection.offset],
                }));
            }
            Entry::Occupied(mut slot) => match slot.get_mut() {
                Some(first)
                    if first.selection.entry == selection.entry
                        && first.selection.name == selection.name =>
                {
                    first.offsets.push(selection.offset);
                }
                value => *value = None,
            },
        }
    }
    index
}

fn charge_texts<'a>(
    ctx: &DecodeContext<'_>,
    texts: impl IntoIterator<Item = &'a str>,
) -> Result<(), CodecError> {
    for text in texts {
        ctx.retain_bytes(u64_from_index(text.len()), RETAIN_ROLE)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

fn charge_incidence(
    ctx: &DecodeContext<'_>,
    texts: [&str; 4],
    offsets: &[u64],
) -> Result<(), CodecError> {
    ctx.retain_bytes(
        u64_from_index(std::mem::size_of::<CatiaSchemaFieldIncidence>()),
        RETAIN_ROLE,
    )?;
    charge_texts(ctx, texts)?;
    for _ in offsets {
        ctx.retain_bytes(u64_from_index(std::mem::size_of::<u64>()), RETAIN_ROLE)?;
    }
    Ok(())
}

fn incidence(
    ctx: &DecodeContext<'_>,
    entity: &CatiaEntityRecord,
    object: &CatiaObjectRecord,
    declaration: &Declaration<'_>,
    selector: usize,
    target: usize,
) -> Result<CatiaSchemaFieldIncidence, CodecError> {
    let selection = declaration.selection;
    charge_incidence(
        ctx,
        [&entity.id, &object.id, &selection.entry, &selection.name],
        &declaration.offsets,
    )?;
    Ok(CatiaSchemaFieldIncidence {
        source_entity: entity.id.clone(),
        source_object: object.id.clone(),
        schema_ordinal: selection.ordinal,
        schema_entry: selection.entry.clone(),
        schema_name: selection.name.clone(),
        declaration_offsets: declaration.offsets.clone(),
        selector_payload_offset: u64_from_index(selector),
        target_payload_offset: u64_from_index(target),
    })
}

pub(super) fn bind<'e, 'o>(
    ctx: &DecodeContext<'_>,
    chains: &mut [CatiaSchemaConfigurationRowChain],
    entities: &'e [CatiaEntityRecord],
    objects: impl IntoIterator<Item = &'o CatiaObjectRecord>,
) -> Result<(), CodecError> {
    for chain in chains.iter_mut() {
        for link in chain.links_mut() {
            link.field_roles.clear();
        }
    }
    if !chains
        .iter()
        .flat_map(super::schema_configuration_chain::CatiaSchemaConfigurationRowChain::links)
        .any(|l| {
            l.intervening_entities
                .as_ref()
                .is_some_and(|v| !v.is_empty())
        })
    {
        return Ok(());
    }
    let objects = objects.into_iter().collect::<Vec<_>>();
    let owners = PairedRecords::new(entities, &objects);
    let mut uses = HashMap::<(&str, u32), usize>::new();
    for chain in chains.iter() {
        for target in chain
            .links()
            .iter()
            .filter_map(|l| l.intervening_entities.as_ref())
            .flatten()
        {
            let Some((entity, _)) = owners.target(&chain.object_graph, target) else {
                continue;
            };
            *uses
                .entry((&entity.object_graph, entity.entity_id))
                .or_default() += 1;
        }
    }
    if uses.is_empty() {
        return Ok(());
    }
    let graphs = uses.keys().map(|(graph, _)| *graph).collect::<HashSet<_>>();
    let mut found = HashMap::<(&str, u32), Vec<(u64, CatiaSchemaFieldIncidence)>>::new();
    for entity in entities {
        if !graphs.contains(entity.object_graph.as_str())
            || entity.value_schema_selections.is_empty()
        {
            continue;
        }
        let Some((_, object)) = owners.pair(&entity.object_graph, entity.entity_id) else {
            continue;
        };
        let declared = declarations(entity);
        for pair in object.payload.fields.windows(2) {
            let [PayloadField::Reference {
                value: selector,
                offset: selector_offset,
            }, PayloadField::Atom {
                value: target,
                offset: target_offset,
            }] = pair
            else {
                continue;
            };
            let key = (entity.object_graph.as_str(), *target);
            if !uses.contains_key(&key) {
                continue;
            }
            let Some(Some(declaration)) = declared.get(selector) else {
                continue;
            };
            let value = incidence(
                ctx,
                entity,
                object,
                declaration,
                *selector_offset,
                *target_offset,
            )?;
            found
                .entry(key)
                .or_default()
                .push((object.byte_offset, value));
        }
    }
    for values in found.values_mut() {
        values.sort_by_key(|(source, value)| {
            (
                *source,
                value.selector_payload_offset,
                value.target_payload_offset,
            )
        });
    }
    for chain in chains {
        let graph = chain.object_graph.clone();
        for link in chain.links_mut() {
            let Some(targets) = &link.intervening_entities else {
                continue;
            };
            for target in targets {
                let Some((entity, object)) = owners.target(&graph, target) else {
                    continue;
                };
                let key = (entity.object_graph.as_str(), entity.entity_id);
                let Some(cached) = found.get(&key) else {
                    continue;
                };
                let Some(remaining) = uses.get_mut(&key) else {
                    return Err(CodecError::malformed(
                        "CATIA schema-field target count disappeared",
                    ));
                };
                *remaining = remaining.checked_sub(1).ok_or_else(|| {
                    CodecError::malformed("CATIA schema-field target count underflow")
                })?;
                let values = if *remaining == 0 {
                    found.remove(&key).ok_or_else(|| {
                        CodecError::malformed("CATIA schema-field incidences disappeared")
                    })?
                } else {
                    for (_, value) in cached {
                        charge_incidence(
                            ctx,
                            [
                                &value.source_entity,
                                &value.source_object,
                                &value.schema_entry,
                                &value.schema_name,
                            ],
                            &value.declaration_offsets,
                        )?;
                    }
                    cached.clone()
                };
                let incidences = values
                    .into_iter()
                    .map(|(_, value)| value)
                    .collect::<Vec<_>>();
                let name = incidences
                    .first()
                    .filter(|first| {
                        incidences
                            .iter()
                            .all(|i| i.schema_name == first.schema_name)
                    })
                    .map(|first| first.schema_name.as_str());
                ctx.retain_bytes(
                    u64_from_index(std::mem::size_of::<CatiaSchemaConfigurationFieldRole>()),
                    RETAIN_ROLE,
                )?;
                charge_texts(
                    ctx,
                    [
                        target.entity().unwrap_or_default(),
                        target.class_name().unwrap_or_default(),
                        object.id.as_str(),
                        name.unwrap_or_default(),
                    ],
                )?;
                link.field_roles.push(CatiaSchemaConfigurationFieldRole {
                    target: target.clone(),
                    target_object: object.id.clone(),
                    name: name.map(str::to_owned),
                    incidences,
                });
            }
        }
    }
    Ok(())
}

cadmpeg_core::named_optional_field!(deserialize_role_name, String, "name");
