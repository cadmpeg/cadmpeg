// SPDX-License-Identifier: Apache-2.0
//! External document and source dependency decoding.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text_charged;
use super::StageOutcome;
use super::{RecordExt, ValueExt};

pub(super) fn decode(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    let mut losses = Vec::new();
    let mut documents = BTreeMap::new();
    let mut sources = BTreeMap::new();
    for (&id, record) in exchange.records() {
        if let Some(parameters) = document_parameters(record) {
            let identifier = parameters
                .first()
                .map(|value| {
                    decode_text_charged(
                        exchange,
                        value,
                        &mut losses,
                        id,
                        "document identifier",
                        StepLossCode::MetadataStringInvalid,
                        Some(ctx),
                    )
                })
                .transpose()?
                .flatten()
                .unwrap_or_default();
            let name = parameters
                .get(1)
                .map(|value| {
                    decode_text_charged(
                        exchange,
                        value,
                        &mut losses,
                        id,
                        "document name",
                        StepLossCode::MetadataStringInvalid,
                        Some(ctx),
                    )
                })
                .transpose()?
                .flatten()
                .unwrap_or_default();
            ctx.charge_collection_items(1, "step_dependency_documents")?;
            documents.insert(id, (identifier, name, parameters.get(3).and_then(ValueExt::reference)));
        }
        if let Some(partial) = record.partial("EXTERNAL_SOURCE") {
            let parameters = partial.parameters.as_slice();
            if let Some(source) = parameters
                .first()
                .map(|value| source_text(exchange, value, &mut losses, id, "external source", ctx))
                .transpose()?
                .flatten()
            {
                ctx.charge_collection_items(1, "step_dependency_sources")?;
                sources.insert(id, source);
            }
        }
    }
    let mut typed = HashSet::new();
    let mut notes = BTreeSet::new();

    for (&id, record) in exchange.records() {
        if let Some(parameters) = document_reference_parameters(record) {
            let Some(document_id) = parameters.first().and_then(ValueExt::reference) else {
                continue;
            };
            let Some((identifier, name, kind)) = documents.get(&document_id) else {
                continue;
            };
            let source = parameters
                .get(1)
                .map(|value| {
                    decode_text_charged(
                        exchange,
                        value,
                        &mut losses,
                        id,
                        "document reference source",
                        StepLossCode::MetadataStringInvalid,
                        Some(ctx),
                    )
                })
                .transpose()?
                .flatten()
                .unwrap_or_default();
            insert_note(&mut notes, document_note(identifier, name, &source, ctx)?, ctx)?;
            insert_claim(&mut typed, id, ctx)?;
            insert_claim(&mut typed, document_id, ctx)?;
            if let Some(kind) = kind {
                insert_claim(&mut typed, *kind, ctx)?;
            }
        }
        if let Some(partial) = record.partial("EXTERNALLY_DEFINED_ITEM") {
            let Some(source_id) = partial.parameters.get(1).and_then(ValueExt::reference) else {
                continue;
            };
            let Some(source) = sources.get(&source_id) else {
                continue;
            };
            let item = partial
                .parameters
                .first()
                .map(|value| source_text(exchange, value, &mut losses, id, "external item", ctx))
                .transpose()?
                .flatten()
                .unwrap_or_default();
            insert_note(
                &mut notes,
                charged_note(&["external source ", source, " item ", &item], ctx)?,
                ctx,
            )?;
            insert_claim(&mut typed, id, ctx)?;
            insert_claim(&mut typed, source_id, ctx)?;
        }
    }

    ctx.charge_collection_items(u64_from_index(notes.len()), "step_dependency_note_vector")?;
    let mut ordered_notes = Vec::new();
    ordered_notes.try_reserve_exact(notes.len()).map_err(|_| {
        ctx.refuse_codec_limit("step_dependency_note_vector", 0, u64_from_index(notes.len()))
    })?;
    ordered_notes.extend(notes);
    Ok(StageOutcome {
        value: (),
        claims: typed,
        notes: ordered_notes,
        losses,
    })
}

fn insert_claim(
    claims: &mut HashSet<u64>,
    id: u64,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if !claims.contains(&id) {
        ctx.charge_collection_items(1, "step_dependency_claims")?;
        claims
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("step_dependency_claims", 0, 1))?;
        claims.insert(id);
    }
    Ok(())
}

fn insert_note(
    notes: &mut BTreeSet<String>,
    note: String,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if !notes.contains(&note) {
        ctx.charge_collection_items(1, "step_dependency_note_set")?;
        notes.insert(note);
    }
    Ok(())
}

fn document_parameters(record: &RawRecord) -> Option<&[Value]> {
    record
        .partial("DOCUMENT")
        .or_else(|| record.partial("DOCUMENT_FILE"))
        .map(|partial| partial.parameters.as_slice())
}

fn document_reference_parameters(record: &RawRecord) -> Option<&[Value]> {
    record
        .partial("DOCUMENT_REFERENCE")
        .or_else(|| record.partial("APPLIED_DOCUMENT_REFERENCE"))
        .map(|partial| partial.parameters.as_slice())
}

fn source_text(
    exchange: &Exchange,
    value: &Value,
    losses: &mut Vec<LossNote>,
    record_id: u64,
    field: &str,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    match value {
        Value::String(_) => decode_text_charged(
            exchange,
            value,
            losses,
            record_id,
            field,
            StepLossCode::MetadataStringInvalid,
            Some(ctx),
        ),
        Value::Typed(_, value) => source_text(exchange, value, losses, record_id, field, ctx),
        _ => Ok(None),
    }
}

fn document_note(
    identifier: &str,
    name: &str,
    source: &str,
    ctx: &DecodeContext<'_>,
) -> Result<String, CodecError> {
    let identity: &[&str] = match (identifier.is_empty(), name.is_empty()) {
        (false, false) => &[identifier, " (", name, ")"],
        (false, true) => &[identifier],
        (true, false) => &[name],
        (true, true) => &["unnamed"],
    };
    let mut parts = Vec::with_capacity(identity.len() + 3);
    parts.push("external document ");
    parts.extend_from_slice(identity);
    if !source.is_empty() {
        parts.extend([" from ", source]);
    }
    charged_note(&parts, ctx)
}

fn charged_note(parts: &[&str], ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
    let operation = "step_dependency_note_text";
    let len = parts.iter().try_fold(0usize, |sum, part| sum.checked_add(part.len()));
    let len = len.ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    ctx.charge_retained(u64_from_index(len), operation)?;
    let mut note = String::new();
    note.try_reserve_exact(len)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, u64_from_index(len)))?;
    for part in parts {
        note.push_str(part);
    }
    Ok(note)
}

#[cfg(test)]
pub(crate) mod tests;
