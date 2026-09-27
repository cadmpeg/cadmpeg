// SPDX-License-Identifier: Apache-2.0
//! Admission of the JT document, segment, and element graph.

use std::collections::BTreeMap;
use std::io::Write;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};

use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::{
    DisplayJtCompressedElement, DisplayJtCompressedElementSequence, DisplayJtDocument,
    DisplayJtSegment, DisplayJtShapeLodElement,
};

/// A JT graph with resolved owners and consistent repeated segment fields.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DisplayJtGraphWire")]
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
        Self::from_wire(Some(ctx), wire)
    }

    pub(crate) fn from_namespace_with_context(
        ctx: &DecodeContext<'_>,
        namespace: &NativeNamespace,
    ) -> Result<Self, NativeConvertError> {
        Self::from_namespace(Some(ctx), namespace)
    }

    fn from_namespace(
        ctx: Option<&DecodeContext<'_>>,
        namespace: &NativeNamespace,
    ) -> Result<Self, NativeConvertError> {
        Self::from_wire(
            ctx,
            DisplayJtGraphWire {
                documents: arena_as_charged(ctx, namespace, "display_jt_documents")?,
                segments: arena_as_charged(ctx, namespace, "display_jt_segments")?,
                shape_lod_elements: arena_as_charged(
                    ctx,
                    namespace,
                    "display_jt_shape_lod_elements",
                )?,
                compressed_elements: arena_as_charged(
                    ctx,
                    namespace,
                    "display_jt_compressed_elements",
                )?,
                compressed_element_sequences: arena_as_charged(
                    ctx,
                    namespace,
                    "display_jt_compressed_element_sequences",
                )?,
            },
        )
    }

    fn from_wire(
        ctx: Option<&DecodeContext<'_>>,
        wire: DisplayJtGraphWire,
    ) -> Result<Self, NativeConvertError> {
        let (documents, _document_index) =
            by_id(ctx, &wire.documents, |item| item.id.as_str(), "documents")?;
        let (segments, _segment_index) =
            by_id(ctx, &wire.segments, |item| item.id.as_str(), "segments")?;
        let (elements, _element_index) = by_id(
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
        let toc_count = wire.documents.iter().try_fold(0_u64, |sum, document| {
            u64::try_from(document.toc_entries.len())
                .ok()
                .and_then(|count| sum.checked_add(count))
                .ok_or_else(|| invalid(&document.id, "TOC count exceeds u64"))
        })?;
        let _toc_index = reserve_graph_index(ctx, toc_count, "index DisplayJT TOC entries")?;
        let mut toc_entries = BTreeMap::new();
        for document in &wire.documents {
            for entry in &document.toc_entries {
                if toc_entries
                    .insert((document.id.as_str(), entry.id.as_str()), entry)
                    .is_some()
                {
                    return Err(invalid(
                        &entry.id,
                        "duplicate toc_entry identity in document",
                    ));
                }
            }
        }
        for segment in &wire.segments {
            let document = documents
                .get(segment.document.as_str())
                .ok_or_else(|| invalid(&segment.id, "document does not resolve"))?;
            let entry = toc_entries
                .get(&(segment.document.as_str(), segment.toc_entry.as_str()))
                .ok_or_else(|| invalid(&segment.id, "toc_entry does not resolve in document"))?;
            if segment.segment_id != entry.segment_id {
                return Err(invalid(&segment.id, "segment_id disagrees with toc_entry"));
            }
            if segment.segment_type != cadmpeg_core::bytes::assemble_u32_be(entry.attributes) {
                return Err(invalid(
                    &segment.id,
                    "segment_type disagrees with toc_entry.attributes",
                ));
            }
            if segment.segment_byte_len != entry.segment_byte_len {
                return Err(invalid(
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
                    &segment.id,
                    "source_offset disagrees with document and toc_entry",
                ));
            }
        }
        for element in &wire.shape_lod_elements {
            let segment = segments
                .get(element.segment.as_str())
                .ok_or_else(|| invalid(&element.id, "segment does not resolve"))?;
            if segment.segment_type != 7 {
                return Err(invalid(&element.id, "segment is not a type-7 shape LOD"));
            }
        }
        for element in &wire.compressed_elements {
            admit_compressed_owner(
                &segments,
                &element.id,
                &element.segment,
                element.segment_type,
                element.source_offset,
            )?;
        }
        for sequence in &wire.compressed_element_sequences {
            admit_compressed_owner(
                &segments,
                &sequence.id,
                &sequence.segment,
                sequence.segment_type,
                sequence.source_offset,
            )?;
            let mut next_offset = 0u64;
            for (ordinal, id) in sequence.elements().iter().enumerate() {
                let element = elements.get(id.as_str()).ok_or_else(|| {
                    invalid(&sequence.id, "elements contains an unresolved identity")
                })?;
                if element.segment != sequence.segment
                    || usize::try_from(element.ordinal).ok() != Some(ordinal)
                {
                    return Err(invalid(
                        &sequence.id,
                        "elements disagrees with element.segment or element.ordinal",
                    ));
                }
                if u64::from(element.inflated_offset) != next_offset {
                    return Err(invalid(
                        &element.id,
                        "inflated_offset disagrees with the preceding element extent",
                    ));
                }
                next_offset = next_offset
                    .checked_add(25 + u64::from(element.body_byte_len()))
                    .ok_or_else(|| invalid(&sequence.id, "framed_byte_len overflows"))?;
            }
            if next_offset.checked_add(20) != Some(u64::from(sequence.framed_byte_len())) {
                return Err(invalid(
                    &sequence.id,
                    "framed_byte_len disagrees with element body_byte_len values and end marker",
                ));
            }
        }
        Ok(Self(wire))
    }
}

impl TryFrom<DisplayJtGraphWire> for DisplayJtGraph {
    type Error = NativeConvertError;

    fn try_from(wire: DisplayJtGraphWire) -> Result<Self, Self::Error> {
        Self::from_wire(None, wire)
    }
}

impl Serialize for DisplayJtGraph {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl TryFrom<&NativeNamespace> for DisplayJtGraph {
    type Error = NativeConvertError;

    fn try_from(namespace: &NativeNamespace) -> Result<Self, Self::Error> {
        Self::from_namespace(None, namespace)
    }
}

fn admit_compressed_owner(
    segments: &BTreeMap<&str, &DisplayJtSegment>,
    id: &str,
    owner: &str,
    segment_type: u32,
    source_offset: u64,
) -> Result<(), NativeConvertError> {
    let segment = segments
        .get(owner)
        .ok_or_else(|| invalid(id, "segment does not resolve"))?;
    if segment.compression.is_none() {
        return Err(invalid(id, "segment has no compression envelope"));
    }
    if segment_type != segment.segment_type {
        return Err(invalid(id, "segment_type disagrees with segment"));
    }
    if segment.source_offset.checked_add(24) != Some(source_offset) {
        return Err(invalid(id, "source_offset disagrees with segment"));
    }
    Ok(())
}

fn reserve_graph_index<'a>(
    ctx: Option<&'a DecodeContext<'_>>,
    count: u64,
    operation: &'static str,
) -> Result<Option<ScopedReservation<'a>>, NativeConvertError> {
    let Some(ctx) = ctx else {
        return Ok(None);
    };
    ctx.charge_collection_items(count, operation)?;
    let bytes = count
        .checked_mul(128)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    Ok(Some(ctx.reserve_scoped(bytes, operation)?))
}

#[derive(Default)]
struct JsonByteCount(u64);

impl Write for JsonByteCount {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let size = u64::try_from(bytes.len()).map_err(std::io::Error::other)?;
        self.0 = self
            .0
            .checked_add(size)
            .ok_or_else(|| std::io::Error::other("DisplayJT JSON byte count exceeds u64"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn arena_as_charged<T: DeserializeOwned>(
    ctx: Option<&DecodeContext<'_>>,
    namespace: &NativeNamespace,
    name: &'static str,
) -> Result<Vec<T>, NativeConvertError> {
    let Some(ctx) = ctx else {
        return namespace.arena_as(name);
    };
    let records = namespace.arenas().get(name);
    let count = records.map_or(0, Vec::len);
    let count = u64::try_from(count)
        .map_err(|_| ctx.refuse_codec_limit("decode DisplayJT native records", 0, u64::MAX))?;
    let mut json_size = JsonByteCount::default();
    if let Some(records) = records {
        for record in records {
            serde_json::to_writer(&mut json_size, record)?;
        }
    }
    ctx.charge_work(json_size.0, "decode DisplayJT native records")?;
    ctx.charge_collection_items(count, "decode DisplayJT native records")?;
    let slot_bytes = count
        .checked_mul(
            u64::try_from(std::mem::size_of::<T>()).map_err(|_| {
                ctx.refuse_codec_limit("retain DisplayJT native records", 0, u64::MAX)
            })?,
        )
        .ok_or_else(|| ctx.refuse_codec_limit("retain DisplayJT native records", 0, u64::MAX))?;
    let copied_bytes = json_size
        .0
        .checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit("retain DisplayJT native records", 0, u64::MAX))?;
    let retained = slot_bytes
        .checked_add(copied_bytes)
        .ok_or_else(|| ctx.refuse_codec_limit("retain DisplayJT native records", 0, u64::MAX))?;
    ctx.charge_retained(retained, "retain DisplayJT native records")?;
    let temporary = json_size.0.checked_mul(4).ok_or_else(|| {
        ctx.refuse_codec_limit("materialize DisplayJT native records", 0, u64::MAX)
    })?;
    let _reservation = ctx.reserve_scoped(temporary, "materialize DisplayJT native records")?;
    namespace.arena_as(name)
}

fn by_id<'a, 'ctx, T>(
    ctx: Option<&'ctx DecodeContext<'_>>,
    records: &'a [T],
    id: impl Fn(&'a T) -> &'a str,
    arena: &str,
) -> Result<(BTreeMap<&'a str, &'a T>, Option<ScopedReservation<'ctx>>), NativeConvertError> {
    let count = u64::try_from(records.len()).map_err(|_| {
        NativeConvertError::InvalidCollection("DisplayJT index count exceeds u64".into())
    })?;
    let reservation = reserve_graph_index(ctx, count, "index DisplayJT graph records")?;
    let mut index = BTreeMap::new();
    for record in records {
        let id = id(record);
        if index.insert(id, record).is_some() {
            return Err(invalid(id, &format!("duplicate identity in {arena}")));
        }
    }
    Ok((index, reservation))
}

fn invalid(id: &str, field: &str) -> NativeConvertError {
    NativeConvertError::InvalidCollection(format!(
        "{}: display_jt {id}: {field}",
        crate::loss::NxLossCode::DisplayJtGraphRejected.code()
    ))
}

#[cfg(test)]
mod tests;
