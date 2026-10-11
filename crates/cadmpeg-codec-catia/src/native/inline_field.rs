// SPDX-License-Identifier: Apache-2.0
//! Exact inline field incidences and source-closed wide schema paths.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    CatiaCatalog, CatiaEntityRecord, CatiaEntityReference, CatiaObjectGraph, CatiaObjectRecord,
};
use crate::object_graph::inline::{self, FieldReference, SchemaTuple};

/// An entity incidence at its exact inline-body position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InlineIncidence {
    /// Byte offset relative to the complete inline body.
    body_offset: u64,
    /// Same-graph entity identity and its independent resolution.
    reference: CatiaEntityReference,
}

/// Complete catalog-entry path established by the paired entity production.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InlineSchemaPath {
    /// Selected outer object-schema entry.
    outer: String,
    /// Selected field/storage-schema entry.
    field: String,
    /// Independently selected data-member entry.
    member: String,
}

/// Schema dialect with no inferred mapping for the compact tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum InlineSchema {
    /// Exact `82 F2 F0 82` production; token meanings remain unassigned.
    Compact,
    /// Two complete wide selectors with an optional source-closed path.
    Words {
        /// Raw outer selector word.
        outer: u32,
        /// Raw field selector word.
        field: u32,
        /// Catalog path only when every paired byte and selection closes.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_path"
        )]
        path: Option<InlineSchemaPath>,
    },
}

/// One strict inline record; ordinary owner, class, and storage roles stay separate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatiaInlineField {
    /// Source-offset native identity.
    pub(crate) id: String,
    /// Absolute `7C09` marker offset.
    byte_offset: u64,
    /// Exact source object record.
    source_record: String,
    /// Incidence selecting the current paired entity.
    self_field: InlineIncidence,
    /// Incidence selecting the anchor field.
    anchor_field: InlineIncidence,
    /// Optional value-bearing field incidence, distinct from the short count atom.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_value_field"
    )]
    value_field: Option<InlineIncidence>,
    /// Complete schema tuple and independently verified catalog path.
    schema: InlineSchema,
}

cadmpeg_core::named_optional_field!(deserialize_path, InlineSchemaPath, "path");
cadmpeg_core::named_optional_field!(deserialize_value_field, InlineIncidence, "value_field");

fn word(bytes: &[u8]) -> Option<(u32, &[u8])> {
    let bytes = bytes.strip_prefix(&[0x32])?;
    Some((
        View::u32_le_at(bytes, 0)?,
        bytes.get(std::mem::size_of::<u32>()..)?,
    ))
}

fn schema_path(
    ctx: &DecodeContext<'_>,
    entity: &CatiaEntityRecord,
    catalog: Option<&CatiaCatalog>,
    outer: u32,
    field: u32,
) -> Result<Option<InlineSchemaPath>, CodecError> {
    let Some(body) = entity.inline_body().and_then(|b| b.strip_prefix(&[3])) else {
        return Ok(None);
    };
    let Some((prefix_field, body)) = word(body) else {
        return Ok(None);
    };
    let Some((member, body)) = word(body) else {
        return Ok(None);
    };
    let Some(body) = body.strip_prefix(&[0xea]) else {
        return Ok(None);
    };
    if View::u32_le_at(body, 0) != Some(entity.entity_id) {
        return Ok(None);
    }
    let Some(body) = body
        .get(std::mem::size_of::<u32>()..)
        .and_then(|b| b.strip_prefix(&[0x82, 0x81, 0, 0x82]))
    else {
        return Ok(None);
    };
    let Some((suffix_outer, body)) = word(body) else {
        return Ok(None);
    };
    let Some((suffix_field, _)) = word(body) else {
        return Ok(None);
    };
    if prefix_field != field || suffix_outer != outer || suffix_field != field {
        return Ok(None);
    }
    let Some(catalog) = catalog else {
        return Ok(None);
    };
    let selected = [outer, field, member].map(|ordinal| {
        usize::try_from(ordinal)
            .ok()
            .and_then(|i| catalog.entries.get(i))
    });
    let [Some(outer), Some(field), Some(member)] = selected else {
        return Ok(None);
    };
    if outer.value != "Projected3DGeometrySet"
        || field.value != "CATEarlySpecBrpLocation"
        || member.value != "data"
    {
        return Ok(None);
    }
    Ok(Some(InlineSchemaPath {
        outer: ctx.copy_retained_text(&outer.id, "catia_inline_outer_schema")?,
        field: ctx.copy_retained_text(&field.id, "catia_inline_field_schema")?,
        member: ctx.copy_retained_text(&member.id, "catia_inline_member_schema")?,
    }))
}

fn incidence(
    ctx: &DecodeContext<'_>,
    source: FieldReference,
    records: &HashMap<u32, &CatiaObjectRecord>,
    terminal: Option<u32>,
) -> Result<InlineIncidence, CodecError> {
    let target = records.get(&source.identity);
    let reference = if terminal == Some(source.identity) {
        CatiaEntityReference::Null {
            entity_id: source.identity,
        }
    } else {
        CatiaEntityReference::resolved_or_unresolved(
            source.identity,
            target
                .and_then(|r| r.entity_record())
                .map(|id| ctx.copy_retained_text(id, "catia_inline_target_entity"))
                .transpose()?,
            target
                .and_then(|r| r.class.as_ref())
                .and_then(|c| c.name.as_deref())
                .map(|name| ctx.copy_retained_text(name, "catia_inline_target_class"))
                .transpose()?,
        )
    };
    Ok(InlineIncidence {
        body_offset: u64_from_index(source.offset),
        reference,
    })
}

/// Bind strict bodies in their complete paired graph; never assign ordinary head roles.
pub(crate) fn derive(
    ctx: &DecodeContext<'_>,
    graphs: &[CatiaObjectGraph],
    entities: &[CatiaEntityRecord],
    catalogs: &[CatiaCatalog],
) -> Result<Vec<CatiaInlineField>, CodecError> {
    let mut fields = Vec::new();
    for graph in graphs {
        ctx.charge_work(
            u64_from_index(graph.records.len()),
            "catia_inline_candidate_scan",
        )?;
        let mut candidates = graph
            .records
            .iter()
            .filter_map(|record| Some((record, inline::parse(record.inline_body.as_deref()?)?)))
            .peekable();
        if candidates.peek().is_none() {
            continue;
        }
        ctx.charge_work(
            u64_from_index(entities.len()),
            "catia_inline_entity_scope_scan",
        )?;
        let entity_by_identity = ctx.collect_hash_map(
            entities
                .iter()
                .filter(|e| e.object_graph == graph.id)
                .map(|e| (e.entity_id, e)),
            "catia_inline_entity_index",
        )?;
        let records = ctx.collect_hash_map(
            graph
                .records
                .iter()
                .filter_map(|r| Some((r.entity_id()?, r))),
            "catia_inline_record_index",
        )?;
        let terminal = records.keys().max().and_then(|n| n.checked_add(1));
        let catalog = graph
            .catalog
            .as_ref()
            .and_then(|id| catalogs.iter().find(|c| &c.id == id));
        for (record, parsed) in candidates {
            ctx.charge_work(1, "catia_inline_field_binding")?;
            if record.entity_id() != Some(parsed.self_field.identity) {
                continue;
            }
            let Some(entity) = entity_by_identity.get(&parsed.self_field.identity) else {
                continue;
            };
            if entity.inline_body().is_none() || record.entity_record() != Some(entity.id.as_str())
            {
                continue;
            }
            if let Some(anchor) = records.get(&parsed.anchor_field.identity) {
                if anchor.lead != 0x12 || anchor.design_object.is_none() {
                    continue;
                }
            }
            if let Some(value) = parsed.value_field.and_then(|v| records.get(&v.identity)) {
                let Some(value_entity) =
                    value.entity_id().and_then(|id| entity_by_identity.get(&id))
                else {
                    continue;
                };
                ctx.charge_work(
                    u64_from_index(value.payload.fields.len()),
                    "catia_inline_object_value_carrier",
                )?;
                ctx.charge_work(
                    u64_from_index(value_entity.value_payload().len()),
                    "catia_inline_entity_value_carrier",
                )?;
                if value
                    .payload
                    .fields
                    .iter()
                    .all(|field| matches!(field, crate::object_graph::PayloadField::Terminator))
                    || value_entity
                        .value_payload()
                        .iter()
                        .all(|byte| *byte == 0xfe)
                {
                    continue;
                }
            }
            let schema = match parsed.schema {
                SchemaTuple::Compact => InlineSchema::Compact,
                SchemaTuple::Words { outer, field } => InlineSchema::Words {
                    outer,
                    field,
                    path: schema_path(ctx, entity, catalog, outer, field)?,
                },
            };
            ctx.push_vec(
                &mut fields,
                CatiaInlineField {
                    id: ctx.format_retained(
                        format_args!("catia:outer:inline-field#{:010}", record.byte_offset),
                        "catia_inline_field_id",
                    )?,
                    byte_offset: record.byte_offset,
                    source_record: ctx
                        .copy_retained_text(&record.id, "catia_inline_source_record")?,
                    self_field: incidence(ctx, parsed.self_field, &records, terminal)?,
                    anchor_field: incidence(ctx, parsed.anchor_field, &records, terminal)?,
                    value_field: parsed
                        .value_field
                        .map(|value| incidence(ctx, value, &records, terminal))
                        .transpose()?,
                    schema,
                },
                "catia_inline_fields",
            )?;
        }
    }
    Ok(fields)
}

#[cfg(test)]
mod tests;
