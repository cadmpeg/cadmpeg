// SPDX-License-Identifier: Apache-2.0
//! SLDPRT helpers for appending sparse IR annotations.
#![deny(clippy::disallowed_methods)]

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationKey, Annotations};
use cadmpeg_ir::{AnnotationBuilder, Exactness, StreamName};

/// The annotation key of the native record `id`.
pub(crate) fn native_key(id: &str) -> Result<AnnotationKey, CodecError> {
    cadmpeg_ir::ids::Identity::new(id)
        .map(AnnotationKey::from)
        .map_err(CodecError::malformed)
}

/// Note the provenance of the retained record of `member`: its stream, its
/// source offset and its payload family.
pub(crate) fn note_member_record(
    annotations: &mut Annotations,
    member: crate::container::Section<'_>,
) -> Result<(), CodecError> {
    let (offset, family) = match member {
        crate::container::Section::Block(block) => (u64_from_index(block.offset), block.family),
        crate::container::Section::Compound(stream) => {
            (0, crate::container::payload_family(&stream.payload))
        }
    };
    note(
        annotations,
        member.native_id(),
        member.source_stream(),
        offset,
        family.label(),
        Exactness::ByteExact,
    )
}

pub(crate) fn note(
    annotations: &mut Annotations,
    key: impl Into<AnnotationKey>,
    stream: &StreamName,
    offset: u64,
    tag: &str,
    exactness: Exactness,
) -> Result<(), CodecError> {
    let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
    let result = builder.annotate(key, stream.as_str(), offset, tag, exactness);
    *annotations = builder.build();
    result
}
