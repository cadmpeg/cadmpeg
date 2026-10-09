// SPDX-License-Identifier: Apache-2.0
//! External document and source dependency decoding.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text_scoped;
use super::StageOutcome;
use super::{RecordExt, ValueExt};

pub(super) fn decode<'ctx>(
    exchange: &Exchange,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<
    StageOutcome<(
        cadmpeg_core::decode::ScopedReservation<'ctx>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    )>,
    CodecError,
> {
    let slot_storage = std::cell::RefCell::new(ctx.reserve_scoped(0, "STEP stage report buffers")?);
    let mut claim_storage = ctx.reserve_scoped(0, "STEP stage claim storage")?;
    let mut scratch_storage = ctx.reserve_scoped(0, "STEP decode scratch")?;
    let mut losses = Vec::new();
    let mut documents = BTreeMap::new();
    let mut sources = BTreeMap::new();
    for (&id, record) in ctx.admit_iter(exchange.records(), "STEP decode traversal")? {
        if let Some(parameters) = document_parameters(ctx, record)? {
            let identifier = parameters
                .first()
                .map(|value| {
                    decode_text_scoped(
                        exchange,
                        value,
                        (&mut losses, &slot_storage),
                        id,
                        ("document identifier", StepLossCode::MetadataStringInvalid),
                        ctx,
                        &mut scratch_storage,
                    )
                })
                .transpose()?
                .flatten()
                .unwrap_or_default();
            let name = parameters
                .get(1)
                .map(|value| {
                    decode_text_scoped(
                        exchange,
                        value,
                        (&mut losses, &slot_storage),
                        id,
                        ("document name", StepLossCode::MetadataStringInvalid),
                        ctx,
                        &mut scratch_storage,
                    )
                })
                .transpose()?
                .flatten()
                .unwrap_or_default();
            scratch_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut documents,
                    id,
                    (
                        identifier,
                        name,
                        parameters.get(3).and_then(ValueExt::reference),
                    ),
                    "step_dependency_documents",
                )
            })?;
        }
        if let Some(partial) = record.partial(ctx, "EXTERNAL_SOURCE")? {
            let parameters = partial.parameters.as_slice();
            if let Some(source) = parameters
                .first()
                .map(|value| {
                    source_text(
                        exchange,
                        value,
                        (&mut losses, &slot_storage),
                        id,
                        "external source",
                        ctx,
                        &mut scratch_storage,
                    )
                })
                .transpose()?
                .flatten()
            {
                scratch_storage.with_storage(|| {
                    ctx.insert_btree_map(&mut sources, id, source, "step_dependency_sources")
                })?;
            }
        }
    }
    let mut typed = BTreeSet::new();
    let mut notes = BTreeSet::new();

    for (&id, record) in ctx.admit_iter(exchange.records(), "STEP decode traversal")? {
        if let Some(parameters) = document_reference_parameters(ctx, record)? {
            let Some(document_id) = parameters.first().and_then(ValueExt::reference) else {
                continue;
            };
            let Some((identifier, name, kind)) =
                ctx.get_btree_map(&documents, &document_id, "STEP dependencies documents get")?
            else {
                continue;
            };
            let mut source_storage = ctx.reserve_scoped(0, "STEP dependency source text")?;
            let source = parameters
                .get(1)
                .map(|value| {
                    decode_text_scoped(
                        exchange,
                        value,
                        (&mut losses, &slot_storage),
                        id,
                        (
                            "document reference source",
                            StepLossCode::MetadataStringInvalid,
                        ),
                        ctx,
                        &mut source_storage,
                    )
                })
                .transpose()?
                .flatten()
                .unwrap_or_default();
            let (note, note_storage) = ctx
                .with_scoped_storage("step_dependency_note_text", || {
                    document_note(identifier, name, &source, ctx)
                })?;
            if scratch_storage.with_storage(|| {
                ctx.insert_btree_set(&mut notes, note, "step_dependency_note_set")
            })? {
                notes = note_storage.commit_value(notes)?;
            }
            claim_storage
                .with_storage(|| ctx.insert_btree_set(&mut typed, id, "step_dependency_claims"))?;
            claim_storage.with_storage(|| {
                ctx.insert_btree_set(&mut typed, document_id, "step_dependency_claims")
            })?;
            if let Some(kind) = kind {
                claim_storage.with_storage(|| {
                    ctx.insert_btree_set(&mut typed, *kind, "step_dependency_claims")
                })?;
            }
        }
        if let Some(partial) = record.partial(ctx, "EXTERNALLY_DEFINED_ITEM")? {
            let Some(source_id) = partial.parameters.get(1).and_then(ValueExt::reference) else {
                continue;
            };
            let Some(source) =
                ctx.get_btree_map(&sources, &source_id, "STEP dependencies sources get")?
            else {
                continue;
            };
            let mut item_storage = ctx.reserve_scoped(0, "STEP dependency item text")?;
            let item = partial
                .parameters
                .first()
                .map(|value| {
                    source_text(
                        exchange,
                        value,
                        (&mut losses, &slot_storage),
                        id,
                        "external item",
                        ctx,
                        &mut item_storage,
                    )
                })
                .transpose()?
                .flatten()
                .unwrap_or_default();
            let (note, note_storage) =
                ctx.with_scoped_storage("step_dependency_note_text", || {
                    ctx.join_retained(
                        &["external source ", source, " item ", &item],
                        "",
                        "step_dependency_note_text",
                    )
                })?;
            if scratch_storage.with_storage(|| {
                ctx.insert_btree_set(&mut notes, note, "step_dependency_note_set")
            })? {
                notes = note_storage.commit_value(notes)?;
            }
            claim_storage
                .with_storage(|| ctx.insert_btree_set(&mut typed, id, "step_dependency_claims"))?;
            claim_storage.with_storage(|| {
                ctx.insert_btree_set(&mut typed, source_id, "step_dependency_claims")
            })?;
        }
    }

    let mut ordered_notes = slot_storage
        .borrow_mut()
        .with_storage(|| ctx.collection_vec(notes.len(), "step_dependency_note_vector"))?;
    ordered_notes.extend(ctx.admit_iter(notes, "STEP dependency note transfer")?);
    Ok(StageOutcome {
        value: (claim_storage, slot_storage.into_inner()),
        claims: typed,
        notes: ordered_notes,
        losses,
    })
}

fn document_parameters<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a [Value]>, CodecError> {
    let mut partial = record.partial(ctx, "DOCUMENT")?;
    if partial.is_none() {
        partial = record.partial(ctx, "DOCUMENT_FILE")?;
    }
    Ok(partial.map(|partial| partial.parameters.as_slice()))
}

fn document_reference_parameters<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a [Value]>, CodecError> {
    let mut partial = record.partial(ctx, "DOCUMENT_REFERENCE")?;
    if partial.is_none() {
        partial = record.partial(ctx, "APPLIED_DOCUMENT_REFERENCE")?;
    }
    Ok(partial.map(|partial| partial.parameters.as_slice()))
}

fn source_text(
    exchange: &Exchange,
    value: &Value,
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    record_id: u64,
    field: &str,
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<Option<String>, CodecError> {
    let mut value = value;
    while let Value::Typed(_, inner) = value {
        ctx.charge_work(1, "STEP dependency source text step")?;
        value = inner;
    }
    decode_text_scoped(
        exchange,
        value,
        (losses, slot_storage),
        record_id,
        (field, StepLossCode::MetadataStringInvalid),
        ctx,
        storage,
    )
}

fn document_note(
    identifier: &str,
    name: &str,
    source: &str,
    ctx: &DecodeContext<'_>,
) -> Result<String, CodecError> {
    let (first, open, second, close) = match (identifier.is_empty(), name.is_empty()) {
        (false, false) => (identifier, " (", name, ")"),
        (false, true) => (identifier, "", "", ""),
        (true, false) => (name, "", "", ""),
        (true, true) => ("unnamed", "", "", ""),
    };
    ctx.join_retained(
        &[
            "external document ",
            first,
            open,
            second,
            close,
            if source.is_empty() { "" } else { " from " },
            source,
        ],
        "",
        "step_dependency_note_text",
    )
}

#[cfg(test)]
pub(crate) mod tests;
