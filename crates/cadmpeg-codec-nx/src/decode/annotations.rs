// SPDX-License-Identifier: Apache-2.0
//! Decode-budget admission for source annotations.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

pub(super) fn note(
    ctx: &DecodeContext<'_>,
    annotations: &mut AnnotationBuilder,
    id: &str,
    stream: &StreamHandle,
    offset: u64,
    tag: &str,
) -> Result<(), CodecError> {
    let bytes = id.len().checked_add(tag.len()).ok_or_else(|| {
        ctx.refuse_codec_limit("nx annotation provenance text", 0, u64::MAX)
    })?;
    ctx.charge_retained(u64_from_index(bytes), "nx annotation provenance text")?;
    ctx.charge_collection_items(1, "nx annotation provenance node")?;
    annotations.note(id, stream, offset).tag(tag);
    Ok(())
}

pub(super) fn derived(
    ctx: &DecodeContext<'_>,
    annotations: &mut AnnotationBuilder,
    id: &str,
    field: &str,
) -> Result<(), CodecError> {
    let bytes = id.len().checked_add(field.len()).ok_or_else(|| {
        ctx.refuse_codec_limit("nx annotation exactness text", 0, u64::MAX)
    })?;
    ctx.charge_retained(u64_from_index(bytes), "nx annotation exactness text")?;
    ctx.charge_collection_items(2, "nx annotation exactness nodes")?;
    annotations.derived(id, field).map_err(CodecError::malformed)?;
    Ok(())
}

pub(super) fn exactness(
    ctx: &DecodeContext<'_>,
    annotations: &mut AnnotationBuilder,
    id: &str,
    value: Exactness,
) -> Result<(), CodecError> {
    ctx.charge_retained(u64_from_index(id.len()), "nx annotation exactness identity")?;
    ctx.charge_collection_items(1, "nx annotation exactness node")?;
    annotations.exactness(id, value);
    Ok(())
}
