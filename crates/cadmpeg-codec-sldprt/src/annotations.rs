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
    let id = ctx.format_retained(format_args!("{}", id.as_ref()), OPERATION)?;
    let stream_name = ctx.format_retained(format_args!("{}", stream.as_str()), OPERATION)?;
    let stream_name = StreamName::try_from(stream_name)
        .map_err(|_| CodecError::malformed("empty SLDPRT annotation stream"))?;
    let tag = ctx.format_retained(format_args!("{tag}"), OPERATION)?;
    let id_bytes = u64::try_from(id.len())
        .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_retained(id_bytes, OPERATION)?;
    ctx.charge_retained(id_bytes, OPERATION)?;
    ctx.charge_collection_items(1, OPERATION)?;
    if !annotations.provenance.contains_key(&id) {
        ctx.charge_collection_items(1, OPERATION)?;
    }
    if exactness != Exactness::ByteExact && !annotations.exactness().contains_key(&id) {
        ctx.charge_collection_items(1, OPERATION)?;
    }
    let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
    let stream = StreamHandle::new(stream_name);
    builder.note(&id, &stream, offset).tag(tag);
    builder.exactness(id, exactness);
    *annotations = builder.build();
    Ok(())
}
