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
    if opaque
        || matches!(
            property.body,
            Some(crate::native::PropertyBody::Unreadable(_))
        )
    {
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

/// Charge missing content at the admission that retained only its evidence.
pub(crate) fn mark_unreadable(
    ir: &cadmpeg_ir::CadIr,
    fidelity: &mut SourceFidelity,
) -> Result<(), CodecError> {
    use cadmpeg_ir::ArenaClass;
    let Some(native) = ir.native.namespace("fcstd") else {
        return Ok(());
    };
    let objects = native.arena_as::<crate::native::ObjectRecord>("objects")?;
    let properties = native.arena_as::<crate::native::PropertyRecord>("properties")?;
    let class = |property: &crate::native::PropertyRecord| {
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
    };
    for property in &properties {
        if matches!(
            property.body,
            Some(crate::native::PropertyBody::Unreadable(_))
        ) {
            mark_untyped(fidelity, &property.id, class(property))?;
        }
    }
    let entries = native.arena_as::<crate::native::EntryRecord>("entries")?;
    let maps = native.arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")?;
    for property in &properties {
        if matches!(
            property.type_name.as_str(),
            "App::PropertyXLink"
                | "App::PropertyXLinkSub"
                | "App::PropertyXLinkList"
                | "App::PropertyXLinkSubList"
        ) {
            continue;
        }
        let missing = property
            .values()
            .iter()
            .flat_map(|value| &value.attributes)
            .any(|(key, name)| {
                matches!(key.as_str(), "file" | "File")
                    && !name.is_empty()
                    && !entries.iter().any(|entry| entry.name() == name)
            });
        let map_missing = property.values().iter().any(|value| {
            matches!(value.tag.as_str(), "ElementMap" | "ElementMap2")
                && (value
                    .text
                    .as_deref()
                    .is_some_and(|text| !text.trim().is_empty())
                    || value
                        .attributes
                        .get("count")
                        .is_some_and(|count| count != "0"))
        }) && !maps.iter().any(|map| map.property == property.id);
        if missing || map_missing {
            mark_untyped(fidelity, &property.id, class(property))?;
        }
    }

    if objects.is_empty()
        && native
            .arena_as::<crate::native::DocumentFacts>("document")?
            .iter()
            .any(|document| document.object_count != 0)
    {
        fidelity
            .mark_untyped(SOURCE_IMAGE, [ArenaClass::Model, ArenaClass::Design])
            .map_err(CodecError::malformed)?;
    }
    Ok(())
}

/// The original XML extent of one property element.
#[derive(Clone)]
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
