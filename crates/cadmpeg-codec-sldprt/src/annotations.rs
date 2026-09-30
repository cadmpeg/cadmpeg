// SPDX-License-Identifier: Apache-2.0
//! SLDPRT helpers for appending sparse IR annotations.
#![deny(clippy::disallowed_methods)]

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{Annotations, StreamHandle};
use cadmpeg_ir::{AnnotationBuilder, Exactness, StreamName};

pub(crate) fn note(
    ctx: &DecodeContext<'_>,
    annotations: &mut Annotations,
    id: impl AsRef<str>,
    stream: &StreamName,
    offset: u64,
    tag: &str,
    exactness: Exactness,
) -> Result<(), CodecError> {
    const OPERATION: &str = "retain SLDPRT annotation";
    let id = id.as_ref();
    let entries = annotations
        .provenance
        .len()
        .checked_add(annotations.exactness().len())
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let levels = u64::from(usize::BITS - entries.leading_zeros()) + 1;
    let comparison_work = levels
        .checked_mul(16)
        .and_then(|work| work.checked_add(4))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let bytes = [id.len(), stream.as_str().len(), tag.len()]
        .into_iter()
        .try_fold(0u64, |sum, bytes| {
            sum.checked_add(cadmpeg_core::decode::u64_from_index(bytes))
        })
        .and_then(|bytes| bytes.checked_mul(comparison_work))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(bytes, OPERATION)?;
    let provenance_id =
        crate::text_admission::format_retained(ctx, format_args!("{id}"), OPERATION)?;
    let exactness_id =
        crate::text_admission::format_retained(ctx, format_args!("{id}"), OPERATION)?;
    let stream_name = crate::text_admission::format_retained(
        ctx,
        format_args!("{}", stream.as_str()),
        OPERATION,
    )?;
    let stream_name = StreamName::try_from(stream_name)
        .map_err(|_| CodecError::malformed("empty SLDPRT annotation stream"))?;
    let tag = crate::text_admission::format_retained(ctx, format_args!("{tag}"), OPERATION)?;
    ctx.charge_collection_items(1, OPERATION)?;
    if !annotations.provenance.contains_key(id) {
        ctx.charge_collection_items(1, OPERATION)?;
    }
    if exactness != Exactness::ByteExact && !annotations.exactness().contains_key(id) {
        ctx.charge_collection_items(1, OPERATION)?;
    }
    let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
    let stream = StreamHandle::new(stream_name);
    builder.note_owned(provenance_id, &stream, offset).tag(tag);
    builder.exactness_owned(exactness_id, exactness);
    *annotations = builder.build();
    Ok(())
}

fn admit_builder_entry(
    ctx: &DecodeContext<'_>,
    builder: &AnnotationBuilder,
    text_bytes: usize,
    items: u64,
    operation: &'static str,
) -> Result<(), CodecError> {
    let annotations = builder.annotations();
    let entries = annotations
        .provenance
        .len()
        .checked_add(annotations.exactness().len())
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    let levels = u64::from(usize::BITS - entries.leading_zeros()) + 1;
    let work = levels
        .checked_mul(16)
        .and_then(|work| work.checked_add(4))
        .and_then(|work| work.checked_mul(cadmpeg_core::decode::u64_from_index(text_bytes)))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, operation)?;
    ctx.charge_collection_items(items, operation)?;
    Ok(())
}

pub(crate) fn builder_note(
    ctx: &DecodeContext<'_>,
    builder: &mut AnnotationBuilder,
    id: &str,
    stream: &StreamHandle,
    offset: u64,
    tag: &str,
) -> Result<(), CodecError> {
    const OPERATION: &str = "retain SLDPRT BRep provenance";
    let bytes = id
        .len()
        .checked_add(tag.len())
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    admit_builder_entry(ctx, builder, bytes, 1, OPERATION)?;
    let id = crate::text_admission::format_retained(ctx, format_args!("{id}"), OPERATION)?;
    let tag = crate::text_admission::format_retained(ctx, format_args!("{tag}"), OPERATION)?;
    builder.note_owned(id, stream, offset).tag(tag);
    Ok(())
}

pub(crate) fn builder_exactness(
    ctx: &DecodeContext<'_>,
    builder: &mut AnnotationBuilder,
    id: &str,
    exactness: Exactness,
) -> Result<(), CodecError> {
    const OPERATION: &str = "retain SLDPRT BRep exactness";
    admit_builder_entry(ctx, builder, id.len(), 1, OPERATION)?;
    let fields = builder
        .annotations()
        .exactness()
        .get(id)
        .map_or(0, |note| note.fields().len());
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(fields), OPERATION)?;
    let id = crate::text_admission::format_retained(ctx, format_args!("{id}"), OPERATION)?;
    builder.exactness_owned(id, exactness);
    Ok(())
}

pub(crate) fn builder_field(
    ctx: &DecodeContext<'_>,
    builder: &mut AnnotationBuilder,
    id: &str,
    field: &str,
    exactness: Exactness,
) -> Result<(), CodecError> {
    const OPERATION: &str = "retain SLDPRT BRep field exactness";
    let bytes = id
        .len()
        .checked_add(field.len())
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    admit_builder_entry(ctx, builder, bytes, 2, OPERATION)?;
    let fields = builder
        .annotations()
        .exactness()
        .get(id)
        .map_or(0, |note| note.fields().len());
    let field_work = cadmpeg_core::decode::u64_from_index(fields)
        .checked_mul(
            cadmpeg_core::decode::u64_from_index(field.len())
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
        )
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(field_work, OPERATION)?;
    let id = crate::text_admission::format_retained(ctx, format_args!("{id}"), OPERATION)?;
    let field = crate::text_admission::format_retained(ctx, format_args!("{field}"), OPERATION)?;
    builder
        .field_exactness_owned(id, field, exactness)
        .map_err(CodecError::malformed)?;
    Ok(())
}
