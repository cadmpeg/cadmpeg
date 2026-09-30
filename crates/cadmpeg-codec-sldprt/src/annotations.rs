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
    let entries = annotations.provenance.len().checked_add(annotations.exactness().len())
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let levels = u64::from(usize::BITS - entries.leading_zeros()) + 1;
    let comparison_work = levels.checked_mul(16).and_then(|work| work.checked_add(4))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let bytes = [id.len(), stream.as_str().len(), tag.len()].into_iter().try_fold(0u64, |sum, bytes| {
        sum.checked_add(cadmpeg_core::decode::u64_from_index(bytes))
    }).and_then(|bytes| bytes.checked_mul(comparison_work))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(bytes, OPERATION)?;
    let provenance_id = crate::retained_text::format_retained(ctx, format_args!("{id}"), OPERATION)?;
    let exactness_id = crate::retained_text::format_retained(ctx, format_args!("{id}"), OPERATION)?;
    let stream_name = crate::retained_text::format_retained(ctx, format_args!("{}", stream.as_str()), OPERATION)?;
    let stream_name = StreamName::try_from(stream_name)
        .map_err(|_| CodecError::malformed("empty SLDPRT annotation stream"))?;
    let tag = crate::retained_text::format_retained(ctx, format_args!("{tag}"), OPERATION)?;
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
