// SPDX-License-Identifier: Apache-2.0
//! Admission of the JT document, segment, and element graph.

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};

use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::{Deserialize, Serialize};

use cadmpeg_ir::hash::digest::Sha256Digest;

use super::{
    DisplayJtCompressedElement, DisplayJtCompressedElementSequence,
    DisplayJtCompressedElementSequenceWire, DisplayJtDocument, DisplayJtSegment,
    DisplayJtShapeLodElement,
};

/// A JT graph with resolved owners and consistent repeated segment fields.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(Deserialize))]
#[cfg_attr(test, serde(try_from = "DisplayJtGraphWire"))]
pub(crate) struct DisplayJtGraph(DisplayJtGraphWire);

/// Raw JT arenas before aggregate admission.
#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::native) struct DisplayJtGraphWire {
    #[serde(rename = "display_jt_documents")]
    pub(in crate::native) documents: Vec<DisplayJtDocument>,
    #[serde(rename = "display_jt_segments")]
    pub(in crate::native) segments: Vec<DisplayJtSegment>,
    #[serde(rename = "display_jt_shape_lod_elements")]
    pub(in crate::native) shape_lod_elements: Vec<DisplayJtShapeLodElement>,
    #[serde(rename = "display_jt_compressed_elements")]
    pub(in crate::native) compressed_elements: Vec<DisplayJtCompressedElement>,
    #[serde(rename = "display_jt_compressed_element_sequences")]
    pub(in crate::native) compressed_element_sequences: Vec<DisplayJtCompressedElementSequence>,
}

impl DisplayJtGraph {
    pub(in crate::native) fn documents(&self) -> &[DisplayJtDocument] {
        &self.0.documents
    }

    pub(in crate::native) fn segments(&self) -> &[DisplayJtSegment] {
        &self.0.segments
    }

    pub(in crate::native) fn shape_lod_elements(&self) -> &[DisplayJtShapeLodElement] {
        &self.0.shape_lod_elements
    }

    pub(in crate::native) fn compressed_elements(&self) -> &[DisplayJtCompressedElement] {
        &self.0.compressed_elements
    }

    pub(in crate::native) fn compressed_element_sequences(
        &self,
    ) -> &[DisplayJtCompressedElementSequence] {
        &self.0.compressed_element_sequences
    }

    pub(in crate::native) fn from_wire_with_context(
        ctx: &DecodeContext<'_>,
        wire: DisplayJtGraphWire,
    ) -> Result<Self, NativeConvertError> {
        let mut storage = ctx.reserve_scoped(0, "index DisplayJT graph records")?;
        let documents = by_id(
            ctx,
            &mut storage,
            &wire.documents,
            |item| item.id.as_str(),
            "duplicate identity in documents",
        )?;
        let segments = by_id(
            ctx,
            &mut storage,
            &wire.segments,
            |item| item.id.as_str(),
            "duplicate identity in segments",
        )?;
        let elements = by_id(
            ctx,
            &mut storage,
            &wire.compressed_elements,
            |item| item.id.as_str(),
            "duplicate identity in compressed_elements",
        )?;
        by_id(
            ctx,
            &mut storage,
            &wire.shape_lod_elements,
            |item| item.id.as_str(),
            "duplicate identity in shape_lod_elements",
        )?;
        by_id(
            ctx,
            &mut storage,
            &wire.compressed_element_sequences,
            |item| item.id.as_str(),
            "duplicate identity in compressed_element_sequences",
        )?;
        let mut toc_entries = HashMap::new();
        let mut document_records = wire.documents.iter();
        while document_records.len() != 0 {
            let Some(document) =
                ctx.next_charged(&mut document_records, "index DisplayJT TOC entries")?
            else {
                break;
            };
            let mut entries = document.toc_entries.iter();
            while entries.len() != 0 {
                let Some(entry) =
                    ctx.next_charged(&mut entries, "index DisplayJT TOC entries")?
                else {
                    break;
                };
                if storage
                    .with_storage(|| {
                        ctx.insert_hash_map(
                            &mut toc_entries,
                            (document.id.as_str(), entry.id.as_str()),
                            entry,
                            "index DisplayJT TOC entries",
                        )
                    })?
                    .is_some()
                {
                    return Err(invalid(
                        ctx,
                        &entry.id,
                        "duplicate toc_entry identity in document",
                    ));
                }
            }
        }
        let mut segment_records = wire.segments.iter();
        while segment_records.len() != 0 {
            let Some(segment) =
                ctx.next_charged(&mut segment_records, "admit DisplayJT segments")?
            else {
                break;
            };
            let document = ctx
                .get_hash_map(
                    &documents,
                    segment.document.as_str(),
                    "match DisplayJT segment documents",
                )?
                .ok_or_else(|| invalid(ctx, &segment.id, "document does not resolve"))?;
            let entry = ctx
                .get_hash_map(
                    &toc_entries,
                    &(segment.document.as_str(), segment.toc_entry.as_str()),
                    "match DisplayJT segment TOC entries",
                )?
                .ok_or_else(|| {
                    invalid(ctx, &segment.id, "toc_entry does not resolve in document")
                })?;
            if segment.segment_id != entry.segment_id {
                return Err(invalid(
                    ctx,
                    &segment.id,
                    "segment_id disagrees with toc_entry",
                ));
            }
            if segment.segment_type != cadmpeg_core::bytes::assemble_u32_be(entry.attributes) {
                return Err(invalid(
                    ctx,
                    &segment.id,
                    "segment_type disagrees with toc_entry.attributes",
                ));
            }
            if segment.segment_byte_len != entry.segment_byte_len {
                return Err(invalid(
                    ctx,
                    &segment.id,
                    "segment_byte_len disagrees with toc_entry",
                ));
            }
            if document
                .source_offset
                .checked_add(u64::from(entry.segment_offset))
                != Some(segment.source_offset)
            {
                return Err(invalid(
                    ctx,
                    &segment.id,
                    "source_offset disagrees with document and toc_entry",
                ));
            }
        }
        let mut shape_elements = wire.shape_lod_elements.iter();
        while shape_elements.len() != 0 {
            let Some(element) =
                ctx.next_charged(&mut shape_elements, "admit DisplayJT shape elements")?
            else {
                break;
            };
            let segment = ctx
                .get_hash_map(
                    &segments,
                    element.segment.as_str(),
                    "match DisplayJT element segments",
                )?
                .ok_or_else(|| invalid(ctx, &element.id, "segment does not resolve"))?;
            if segment.segment_type != 7 {
                return Err(invalid(
                    ctx,
                    &element.id,
                    "segment is not a type-7 shape LOD",
                ));
            }
        }
        let mut compressed_elements = wire.compressed_elements.iter();
        while compressed_elements.len() != 0 {
            let Some(element) = ctx
                .next_charged(
                    &mut compressed_elements,
                    "admit DisplayJT compressed elements",
                )?
            else {
                break;
            };
            admit_compressed_owner(
                ctx,
                &segments,
                &element.id,
                &element.segment,
                element.segment_type,
                element.source_offset,
            )?;
        }
        let mut sequences = wire.compressed_element_sequences.iter();
        while sequences.len() != 0 {
            let Some(sequence) = ctx
                .next_charged(
                    &mut sequences,
                    "admit DisplayJT compressed element sequences",
                )?
            else {
                break;
            };
            admit_compressed_owner(
                ctx,
                &segments,
                &sequence.id,
                &sequence.segment,
                sequence.segment_type,
                sequence.source_offset,
            )?;
            let mut next_offset = 0u64;
            let mut ids = sequence.elements().iter().enumerate();
            while ids.len() != 0 {
                let Some((ordinal, id)) =
                    ctx.next_charged(&mut ids, "admit DisplayJT sequence elements")?
                else {
                    break;
                };
                let element = ctx
                    .get_hash_map(&elements, id.as_str(), "match DisplayJT sequence elements")?
                    .ok_or_else(|| {
                        invalid(
                            ctx,
                            &sequence.id,
                            "elements contains an unresolved identity",
                        )
                    })?;
                if !ctx.equal(
                    element.segment.as_str(),
                    sequence.segment.as_str(),
                    "match DisplayJT sequence elements",
                )? || usize::try_from(element.ordinal).ok() != Some(ordinal)
                {
                    return Err(invalid(
                        ctx,
                        &sequence.id,
                        "elements disagrees with element.segment or element.ordinal",
                    ));
                }
                if u64::from(element.inflated_offset) != next_offset {
                    return Err(invalid(
                        ctx,
                        &element.id,
                        "inflated_offset disagrees with the preceding element extent",
                    ));
                }
                next_offset = next_offset
                    .checked_add(25 + u64::from(element.body_byte_len()))
                    .ok_or_else(|| invalid(ctx, &sequence.id, "framed_byte_len overflows"))?;
            }
            if next_offset.checked_add(20) != Some(u64::from(sequence.framed_byte_len())) {
                return Err(invalid(
                    ctx,
                    &sequence.id,
                    "framed_byte_len disagrees with element body_byte_len values and end marker",
                ));
            }
        }
        Ok(Self(wire))
    }

    pub(crate) fn from_namespace_with_context(
        ctx: &DecodeContext<'_>,
        namespace: &NativeNamespace,
    ) -> Result<Self, NativeConvertError> {
        Self::from_wire_with_context(
            ctx,
            DisplayJtGraphWire {
                documents: namespace.arena_as_for_decode(ctx, "display_jt_documents")?,
                segments: namespace.arena_as_for_decode(ctx, "display_jt_segments")?,
                shape_lod_elements: namespace
                    .arena_as_for_decode(ctx, "display_jt_shape_lod_elements")?,
                compressed_elements: namespace
                    .arena_as_for_decode(ctx, "display_jt_compressed_elements")?,
                compressed_element_sequences: sequences_for_decode(ctx, namespace)?,
            },
        )
    }

    #[cfg(test)]
    fn from_namespace(namespace: &NativeNamespace) -> Result<Self, NativeConvertError> {
        crate::test_support::with_decode_context(|ctx| {
            Self::from_namespace_with_context(ctx, namespace)
        })
    }
}

#[cfg(test)]
impl TryFrom<DisplayJtGraphWire> for DisplayJtGraph {
    type Error = NativeConvertError;

    fn try_from(wire: DisplayJtGraphWire) -> Result<Self, Self::Error> {
        crate::test_support::with_decode_context(|ctx| Self::from_wire_with_context(ctx, wire))
    }
}

#[cfg(test)]
impl Serialize for DisplayJtGraph {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

#[cfg(test)]
impl TryFrom<&NativeNamespace> for DisplayJtGraph {
    type Error = NativeConvertError;

    fn try_from(namespace: &NativeNamespace) -> Result<Self, Self::Error> {
        Self::from_namespace(namespace)
    }
}

/// Loads stored element sequences, hashing each tail under the decode budget.
fn sequences_for_decode(
    ctx: &DecodeContext<'_>,
    namespace: &NativeNamespace,
) -> Result<Vec<DisplayJtCompressedElementSequence>, NativeConvertError> {
    const ARENA: &str = "display_jt_compressed_element_sequences";
    const OPERATION: &str = "load DisplayJT element sequences";
    let records = ctx
        .get_btree_map(namespace.arenas(), ARENA, OPERATION)?
        .map_or(&[][..], Vec::as_slice);
    let mut wires =
        namespace.arena_iter_as_for_decode::<DisplayJtCompressedElementSequenceWire>(ctx, ARENA);
    let mut sequences = ctx.vector_storage(records.len(), OPERATION)?;
    let mut record_iter = records.iter();
    while record_iter.len() != 0 {
        let Some(record) = ctx.next_charged(&mut record_iter, OPERATION)? else {
            break;
        };
        let Some(wire) = wires.next() else {
            break;
        };
        let mut digest_storage = ctx.reserve_scoped(0, "check DisplayJT sequence tail digest")?;
        let sequence = match DisplayJtCompressedElementSequence::from_wire(wire?, |tail| {
            digest_storage.with_storage(|| {
                Sha256Digest::digest_for_decode(ctx, tail, "check DisplayJT sequence tail digest")
            })
        })? {
            Ok(sequence) => sequence,
            Err(message) => {
                let source = NativeConvertError::ReadRecordMessage {
                    id: record.identity_for_decode(ctx, "retain native record error identity")?,
                    message: ctx.copy_retained_text(message, "retain DisplayJT graph rejection")?,
                };
                let arena = ctx.copy_retained_text(ARENA, "retain native arena error name")?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<NativeConvertError>()),
                    "retain native arena error",
                )?;
                return Err(NativeConvertError::Arena {
                    arena,
                    source: Box::new(source),
                });
            }
        };
        ctx.reserve_vec(&mut sequences, 1, OPERATION)?;
        sequences.push(sequence);
    }
    Ok(sequences)
}

fn admit_compressed_owner(
    ctx: &DecodeContext<'_>,
    segments: &HashMap<&str, &DisplayJtSegment>,
    id: &str,
    owner: &str,
    segment_type: u32,
    source_offset: u64,
) -> Result<(), NativeConvertError> {
    let segment = ctx
        .get_hash_map(segments, owner, "match DisplayJT compressed owners")?
        .ok_or_else(|| invalid(ctx, id, "segment does not resolve"))?;
    if segment.compression.is_none() {
        return Err(invalid(ctx, id, "segment has no compression envelope"));
    }
    if segment_type != segment.segment_type {
        return Err(invalid(ctx, id, "segment_type disagrees with segment"));
    }
    if segment.source_offset.checked_add(24) != Some(source_offset) {
        return Err(invalid(ctx, id, "source_offset disagrees with segment"));
    }
    Ok(())
}

fn by_id<'a, T>(
    ctx: &DecodeContext<'_>,
    storage: &mut ScopedReservation<'_>,
    records: &'a [T],
    id: impl Fn(&'a T) -> &'a str,
    duplicate: &'static str,
) -> Result<HashMap<&'a str, &'a T>, NativeConvertError> {
    let mut index = HashMap::new();
    let mut record_iter = records.iter();
    while record_iter.len() != 0 {
        let Some(record) = ctx.next_charged(&mut record_iter, "index DisplayJT graph records")?
        else {
            break;
        };
        let id = id(record);
        if storage
            .with_storage(|| {
                ctx.insert_hash_map(&mut index, id, record, "index DisplayJT graph records")
            })?
            .is_some()
        {
            return Err(invalid(ctx, id, duplicate));
        }
    }
    Ok(index)
}

fn invalid(ctx: &DecodeContext<'_>, id: &str, field: &str) -> NativeConvertError {
    let code = crate::loss::NxLossCode::DisplayJtGraphRejected.code();
    let prefix = ": display_jt ";
    let separator = ": ";
    match ctx.format_retained(
        format_args!("{code}{prefix}{id}{separator}{field}"),
        "retain DisplayJT graph rejection",
    ) {
        Ok(message) => NativeConvertError::InvalidCollection(message),
        Err(error) => NativeConvertError::Resource(error),
    }
}

#[cfg(test)]
mod tests;
