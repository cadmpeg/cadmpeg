// SPDX-License-Identifier: Apache-2.0
//! Admission of the JT document, segment, and element graph.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;

use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::{Deserialize, Serialize};

use super::{
    DisplayJtCompressedElement, DisplayJtCompressedElementSequence, DisplayJtDocument,
    DisplayJtSegment, DisplayJtShapeLodElement,
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
        let count = |length: usize| {
            u64::try_from(length)
                .map_err(|_| ctx.refuse_codec_limit("index DisplayJT graph records", 0, u64::MAX))
        };
        let _documents = ctx.reserve_scoped(
            count(wire.documents.len())?
                .checked_mul(128)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("index DisplayJT graph records", 0, u64::MAX)
                })?,
            "index DisplayJT graph records",
        )?;
        let _segments = ctx.reserve_scoped(
            count(wire.segments.len())?
                .checked_mul(128)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("index DisplayJT graph records", 0, u64::MAX)
                })?,
            "index DisplayJT graph records",
        )?;
        let _elements = ctx.reserve_scoped(
            count(wire.compressed_elements.len())?
                .checked_mul(128)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("index DisplayJT graph records", 0, u64::MAX)
                })?,
            "index DisplayJT graph records",
        )?;
        let _shape_lods = ctx.reserve_scoped(
            count(wire.shape_lod_elements.len())?
                .checked_mul(128)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("index DisplayJT graph records", 0, u64::MAX)
                })?,
            "index DisplayJT graph records",
        )?;
        let _sequences = ctx.reserve_scoped(
            count(wire.compressed_element_sequences.len())?
                .checked_mul(128)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("index DisplayJT graph records", 0, u64::MAX)
                })?,
            "index DisplayJT graph records",
        )?;
        let toc_count = wire.documents.iter().try_fold(0_u64, |sum, document| {
            count(document.toc_entries.len())?
                .checked_add(sum)
                .ok_or_else(|| ctx.refuse_codec_limit("index DisplayJT TOC entries", 0, u64::MAX))
        })?;
        let _toc = ctx.reserve_scoped(
            toc_count.checked_mul(128).ok_or_else(|| {
                ctx.refuse_codec_limit("index DisplayJT TOC entries", 0, u64::MAX)
            })?,
            "index DisplayJT TOC entries",
        )?;
        Self::from_wire(ctx, wire)
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
                compressed_element_sequences: namespace
                    .arena_as_for_decode(ctx, "display_jt_compressed_element_sequences")?,
            },
        )
    }

    #[cfg(test)]
    fn from_namespace(namespace: &NativeNamespace) -> Result<Self, NativeConvertError> {
        crate::test_support::with_decode_context(|ctx| {
            Self::from_namespace_with_context(ctx, namespace)
        })
    }

    fn from_wire(
        ctx: &DecodeContext<'_>,
        wire: DisplayJtGraphWire,
    ) -> Result<Self, NativeConvertError> {
        let documents = by_id(ctx, &wire.documents, |item| item.id.as_str(), "documents")?;
        let segments = by_id(ctx, &wire.segments, |item| item.id.as_str(), "segments")?;
        let elements = by_id(
            ctx,
            &wire.compressed_elements,
            |item| item.id.as_str(),
            "compressed_elements",
        )?;
        by_id(
            ctx,
            &wire.shape_lod_elements,
            |item| item.id.as_str(),
            "shape_lod_elements",
        )?;
        by_id(
            ctx,
            &wire.compressed_element_sequences,
            |item| item.id.as_str(),
            "compressed_element_sequences",
        )?;
        wire.documents.iter().try_fold(0_u64, |sum, document| {
            u64::try_from(document.toc_entries.len())
                .ok()
                .and_then(|count| sum.checked_add(count))
                .ok_or_else(|| invalid(ctx, &document.id, "TOC count exceeds u64"))
        })?;
        let mut toc_entries = BTreeMap::new();
        for document in &wire.documents {
            for entry in &document.toc_entries {
                ctx.charge_work(1, "index DisplayJT TOC entries")?;
                if ctx
                    .insert_btree_map(
                        &mut toc_entries,
                        (document.id.as_str(), entry.id.as_str()),
                        entry,
                        "index DisplayJT TOC entries",
                    )?
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
        for segment in &wire.segments {
            let document = documents
                .get(segment.document.as_str())
                .ok_or_else(|| invalid(ctx, &segment.id, "document does not resolve"))?;
            let entry = toc_entries
                .get(&(segment.document.as_str(), segment.toc_entry.as_str()))
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
            if segment.segment_type != cadmpeg_core::bytes::assemble_u32_be(*entry.attributes) {
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
        for element in &wire.shape_lod_elements {
            let segment = segments
                .get(element.segment.as_str())
                .ok_or_else(|| invalid(ctx, &element.id, "segment does not resolve"))?;
            if segment.segment_type != 7 {
                return Err(invalid(
                    ctx,
                    &element.id,
                    "segment is not a type-7 shape LOD",
                ));
            }
        }
        for element in &wire.compressed_elements {
            admit_compressed_owner(
                ctx,
                &segments,
                &element.id,
                &element.segment,
                element.segment_type,
                element.source_offset,
            )?;
        }
        for sequence in &wire.compressed_element_sequences {
            admit_compressed_owner(
                ctx,
                &segments,
                &sequence.id,
                &sequence.segment,
                sequence.segment_type,
                sequence.source_offset,
            )?;
            let mut next_offset = 0u64;
            for (ordinal, id) in sequence.elements().iter().enumerate() {
                let element = elements.get(id.as_str()).ok_or_else(|| {
                    invalid(
                        ctx,
                        &sequence.id,
                        "elements contains an unresolved identity",
                    )
                })?;
                if element.segment != sequence.segment
                    || usize::try_from(element.ordinal).ok() != Some(ordinal)
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

fn admit_compressed_owner(
    ctx: &DecodeContext<'_>,
    segments: &BTreeMap<&str, &DisplayJtSegment>,
    id: &str,
    owner: &str,
    segment_type: u32,
    source_offset: u64,
) -> Result<(), NativeConvertError> {
    let segment = segments
        .get(owner)
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
    records: &'a [T],
    id: impl Fn(&'a T) -> &'a str,
    arena: &str,
) -> Result<BTreeMap<&'a str, &'a T>, NativeConvertError> {
    let mut index = BTreeMap::new();
    for record in records {
        ctx.charge_work(1, "index DisplayJT graph records")?;
        let id = id(record);
        if ctx
            .insert_btree_map(&mut index, id, record, "index DisplayJT graph records")?
            .is_some()
        {
            return Err(invalid(ctx, id, &format!("duplicate identity in {arena}")));
        }
    }
    Ok(index)
}

fn invalid(ctx: &DecodeContext<'_>, id: &str, field: &str) -> NativeConvertError {
    use std::fmt::Write as _;

    let code = crate::loss::NxLossCode::DisplayJtGraphRejected.code();
    let prefix = ": display_jt ";
    let separator = ": ";
    let Some(length) = code
        .len()
        .checked_add(prefix.len())
        .and_then(|len| len.checked_add(id.len()))
        .and_then(|len| len.checked_add(separator.len()))
        .and_then(|len| len.checked_add(field.len()))
    else {
        return NativeConvertError::Resource(ctx.refuse_codec_limit(
            "retain DisplayJT graph rejection",
            0,
            1,
        ));
    };
    let mut message = match ctx.retained_string(length, "retain DisplayJT graph rejection") {
        Ok(message) => message,
        Err(error) => return NativeConvertError::Resource(error),
    };
    if write!(&mut message, "{code}{prefix}{id}{separator}{field}").is_err() {
        return NativeConvertError::Resource(ctx.refuse_codec_limit(
            "format DisplayJT graph rejection",
            0,
            1,
        ));
    }
    NativeConvertError::InvalidCollection(message)
}

#[cfg(test)]
mod tests;
