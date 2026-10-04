// SPDX-License-Identifier: Apache-2.0
//! SLDPRT helpers for appending sparse IR annotations.
#![deny(clippy::disallowed_methods)]

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
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
    let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
    let result = builder.annotate(ctx, id.as_ref(), stream.as_str(), offset, tag, exactness);
    *annotations = builder.build();
    result
}
