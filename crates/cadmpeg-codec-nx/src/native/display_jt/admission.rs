// SPDX-License-Identifier: Apache-2.0
//! Admission of the JT document, segment, and element graph.

use std::collections::BTreeMap;

use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::{Deserialize, Serialize};

use super::{
    DisplayJtCompressedElement, DisplayJtCompressedElementSequence, DisplayJtDocument,
    DisplayJtSegment, DisplayJtShapeLodElement,
};

/// A JT graph with resolved owners and consistent repeated segment fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DisplayJtGraphWire", into = "DisplayJtGraphWire")]
pub(crate) struct DisplayJtGraph(DisplayJtGraphWire);

/// Raw JT arenas before aggregate admission.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct DisplayJtGraphWire {
    #[serde(rename = "display_jt_documents")]
    pub(crate) documents: Vec<DisplayJtDocument>,
    #[serde(rename = "display_jt_segments")]
    pub(crate) segments: Vec<DisplayJtSegment>,
    #[serde(rename = "display_jt_shape_lod_elements")]
    pub(crate) shape_lod_elements: Vec<DisplayJtShapeLodElement>,
    #[serde(rename = "display_jt_compressed_elements")]
    pub(crate) compressed_elements: Vec<DisplayJtCompressedElement>,
    #[serde(rename = "display_jt_compressed_element_sequences")]
    pub(crate) compressed_element_sequences: Vec<DisplayJtCompressedElementSequence>,
}

impl DisplayJtGraph {
    pub(crate) fn documents(&self) -> &[DisplayJtDocument] {
        &self.0.documents
    }

    pub(crate) fn segments(&self) -> &[DisplayJtSegment] {
        &self.0.segments
    }

    pub(crate) fn shape_lod_elements(&self) -> &[DisplayJtShapeLodElement] {
        &self.0.shape_lod_elements
    }

    pub(crate) fn compressed_elements(&self) -> &[DisplayJtCompressedElement] {
        &self.0.compressed_elements
    }

    pub(crate) fn compressed_element_sequences(&self) -> &[DisplayJtCompressedElementSequence] {
        &self.0.compressed_element_sequences
    }
}

impl TryFrom<DisplayJtGraphWire> for DisplayJtGraph {
    type Error = NativeConvertError;

    fn try_from(wire: DisplayJtGraphWire) -> Result<Self, Self::Error> {
        let documents = by_id(&wire.documents, |item| item.id.as_str(), "documents")?;
        let segments = by_id(&wire.segments, |item| item.id.as_str(), "segments")?;
        let elements = by_id(
            &wire.compressed_elements,
            |item| item.id.as_str(),
            "compressed_elements",
        )?;
        by_id(
            &wire.shape_lod_elements,
            |item| item.id.as_str(),
            "shape_lod_elements",
        )?;
        by_id(
            &wire.compressed_element_sequences,
            |item| item.id.as_str(),
            "compressed_element_sequences",
        )?;
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
            for (ordinal, id) in sequence.elements.iter().enumerate() {
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
            }
        }
        Ok(Self(wire))
    }
}

impl From<DisplayJtGraph> for DisplayJtGraphWire {
    fn from(value: DisplayJtGraph) -> Self {
        value.0
    }
}

impl TryFrom<&NativeNamespace> for DisplayJtGraph {
    type Error = NativeConvertError;

    fn try_from(namespace: &NativeNamespace) -> Result<Self, Self::Error> {
        DisplayJtGraphWire {
            documents: namespace.arena_as("display_jt_documents")?,
            segments: namespace.arena_as("display_jt_segments")?,
            shape_lod_elements: namespace.arena_as("display_jt_shape_lod_elements")?,
            compressed_elements: namespace.arena_as("display_jt_compressed_elements")?,
            compressed_element_sequences: namespace
                .arena_as("display_jt_compressed_element_sequences")?,
        }
        .try_into()
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

fn by_id<'a, T>(
    records: &'a [T],
    id: impl Fn(&'a T) -> &'a str,
    arena: &str,
) -> Result<BTreeMap<&'a str, &'a T>, NativeConvertError> {
    let mut index = BTreeMap::new();
    for record in records {
        let id = id(record);
        if index.insert(id, record).is_some() {
            return Err(invalid(id, &format!("duplicate identity in {arena}")));
        }
    }
    Ok(index)
}

fn invalid(id: &str, field: &str) -> NativeConvertError {
    NativeConvertError::InvalidCollection(format!("display_jt {id}: {field}"))
}

#[cfg(test)]
mod tests;
