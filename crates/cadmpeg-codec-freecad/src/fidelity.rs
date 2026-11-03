// SPDX-License-Identifier: Apache-2.0
//! Source-only archive and XML images. No native record serializes these bytes.

use std::ops::Range;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    ids::UnknownId, ArenaClasses, RetainedBytes, RetainedSourceRecord, SourceFidelity,
};

pub(crate) const SOURCE_IMAGE: &str = "fcstd:source:image#archive";

pub(crate) fn retain(
    fidelity: &mut SourceFidelity,
    id: &str,
    stream: &str,
    offset: u64,
    bytes: &[u8],
    untyped: ArenaClasses,
) -> Result<(), CodecError> {
    fidelity
        .insert_retained_record(
            UnknownId::mint(id).map_err(CodecError::malformed)?,
            RetainedSourceRecord::from_bytes(
                stream,
                offset,
                RetainedBytes::Inline {
                    data: bytes.to_vec(),
                },
            )?
            .with_untyped(untyped),
        )
        .map_err(CodecError::malformed)
}

/// Record an unknown subrange without another image. Source processing reopens
/// its genuine member from the one retained original archive when necessary.
pub(crate) fn evidence(
    fidelity: &mut SourceFidelity,
    id: &str,
    stream: &str,
    offset: u64,
    bytes: &[u8],
    untyped: ArenaClasses,
) -> Result<(), CodecError> {
    let evidence = cadmpeg_ir::source_fidelity::SourceEvidence::of(bytes);
    fidelity
        .insert_retained_record(
            UnknownId::mint(id).map_err(CodecError::malformed)?,
            RetainedSourceRecord::from_bytes(
                stream,
                offset,
                RetainedBytes::Digest {
                    byte_len: evidence.length,
                    sha256: evidence.sha256,
                },
            )?
            .with_untyped(untyped),
        )
        .map_err(CodecError::malformed)
}

pub(crate) fn property(
    ctx: &crate::property_reader::PropertyReader<'_, '_>,
    fidelity: &mut SourceFidelity,
    property: &crate::native::PropertyRecord,
    objects: &[crate::native::ObjectRecord],
) -> Result<Option<cadmpeg_ir::ArenaClass>, CodecError> {
    if property.body.is_none() {
        return Ok(None);
    }
    let opaque = crate::persistence::opaque_python_payload(property);
    let mut classes = ArenaClasses::new();
    let class = if property.type_name == "Part::PropertyPartShape" {
        cadmpeg_ir::ArenaClass::Model
    } else if property.owner == crate::native::native_id("document", "0")
        || objects.iter().any(|object| {
            object.id().as_str() == property.owner
                && crate::design::is_design_kind(&object.type_name)
        })
    {
        cadmpeg_ir::ArenaClass::Design
    } else {
        cadmpeg_ir::ArenaClass::Model
    };
    if opaque {
        classes.insert(class);
    }
    if !classes.is_empty() {
        let range = ctx.source_range(property)?.ok_or_else(|| {
            CodecError::malformed("source classification requires an original property range")
        })?;
        evidence(
            fidelity,
            &property.id,
            "Document.xml",
            cadmpeg_core::decode::u64_from_index(range.start),
            ctx.property_xml(property)?.as_bytes(),
            classes,
        )?;
    }
    Ok(opaque.then_some(class))
}

/// Mark `class` untyped on the record `id` retained, or on the archive image
/// that holds its bytes when it retained no record of its own.
pub(crate) fn mark_untyped(
    fidelity: &mut SourceFidelity,
    id: &str,
    class: cadmpeg_ir::ArenaClass,
) -> Result<(), CodecError> {
    let owner = if fidelity.retained_record(id).is_some() {
        id
    } else {
        SOURCE_IMAGE
    };
    fidelity
        .mark_untyped(owner, [class])
        .map_err(CodecError::malformed)
}

/// The class a property value part's bytes carry, when its owner is known:
/// shape payloads are model; document-level and design-owned properties are
/// design. Unknown owners (extension records) stay broad: the part helper
/// then marks the requested scope. Narrowing keeps a failed design part from
/// holding model in scope (#209 §1.1).
pub(crate) fn property_value_class(
    objects: &[crate::native::ObjectRecord],
    owner: &str,
    type_name: &str,
) -> Option<cadmpeg_ir::ArenaClass> {
    use cadmpeg_ir::ArenaClass;
    if type_name == "Part::PropertyPartShape" {
        return Some(ArenaClass::Model);
    }
    if owner == crate::native::native_id("document", "0") {
        return Some(ArenaClass::Design);
    }
    objects
        .iter()
        .find(|object| object.id().as_str() == owner)
        .map(|object| {
            if crate::design::is_design_kind(&object.type_name) {
                ArenaClass::Design
            } else {
                ArenaClass::Model
            }
        })
}

/// The class whose content a property's bytes carry: shape payloads are
/// model; document-level and design-owned properties are design.
pub(crate) fn property_class(
    objects: &[crate::native::ObjectRecord],
    property: &crate::native::PropertyRecord,
) -> cadmpeg_ir::ArenaClass {
    use cadmpeg_ir::ArenaClass;
    if property.type_name == "Part::PropertyPartShape" {
        ArenaClass::Model
    } else if property.owner == crate::native::native_id("document", "0")
        || objects.iter().any(|object| {
            object.id().as_str() == property.owner
                && crate::design::is_design_kind(&object.type_name)
        })
    {
        ArenaClass::Design
    } else {
        ArenaClass::Model
    }
}

/// The original XML extent of one property element.
#[derive(Debug, Clone)]
pub(crate) struct ValueSourceRanges {
    record: Range<usize>,
}

impl ValueSourceRanges {
    pub(crate) fn record_range(&self) -> Range<usize> {
        self.record.clone()
    }
}

pub(crate) fn property_source(node: roxmltree::Node<'_, '_>) -> ValueSourceRanges {
    ValueSourceRanges {
        record: node.range(),
    }
}

pub(crate) fn relative_xml_extent(
    source: View<'_>,
    range: Range<usize>,
) -> Result<View<'_>, CodecError> {
    if range.start >= range.end || range.end > source.window().len() {
        return Err(CodecError::malformed(
            "XML record range escapes its source entry",
        ));
    }
    let start = source
        .start()
        .checked_add(range.start)
        .ok_or_else(|| CodecError::malformed("XML record start overflows"))?;
    let end = source
        .start()
        .checked_add(range.end)
        .ok_or_else(|| CodecError::malformed("XML record end overflows"))?;
    source
        .child(start, end)
        .ok_or_else(|| CodecError::malformed("XML record range escapes its source entry"))
}

#[cfg(test)]
mod tests;
