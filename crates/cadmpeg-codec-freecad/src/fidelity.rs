// SPDX-License-Identifier: Apache-2.0
//! Source-only archive and XML images. No native record serializes these bytes.

use std::collections::BTreeMap;
use std::ops::Range;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::source_fidelity::RecordLayout;
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

/// Record a subrange without retaining its image twice. The complete logical
/// entry image remains available in fidelity for layout reuse.
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

pub(crate) fn layout<T: serde::Serialize>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    fidelity: &mut SourceFidelity,
    name: &str,
    value: &T,
) -> Result<(), CodecError> {
    let bytes = serde_json::to_vec(value).map_err(CodecError::malformed)?;
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "retain FCStd source layout",
    )?;
    retain(
        fidelity,
        &crate::native::native_id("source-layout", name),
        "fcstd:source:layout#archive",
        0,
        &bytes,
        ArenaClasses::new(),
    )
}

pub(crate) fn property(
    fidelity: &mut SourceFidelity,
    property: &crate::native::PropertyRecord,
    objects: &[crate::native::ObjectRecord],
) -> Result<Option<cadmpeg_ir::ArenaClass>, CodecError> {
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
    if opaque || matches!(property.body, crate::native::PropertyBody::Unreadable(_)) {
        classes.insert(class);
    }
    evidence(
        fidelity,
        &property.id,
        "Document.xml",
        property.xml.start(),
        property.xml.text().as_bytes(),
        classes,
    )?;
    Ok(opaque.then_some(class))
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
        if matches!(property.body, crate::native::PropertyBody::Unreadable(_)) {
            fidelity
                .mark_untyped(&property.id, [class(property)])
                .map_err(CodecError::malformed)?;
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
            fidelity
                .mark_untyped(&property.id, [class(property)])
                .map_err(CodecError::malformed)?;
        }
    }
    for entry in native.arena_as::<crate::container::UnreadableEntry>("unreadable_entries")? {
        let mut classes = ArenaClasses::new();
        for property in properties.iter().filter(|property| {
            property.side_entries().contains(&entry.name)
                || property.values().iter().any(|value| {
                    value.attributes.iter().any(|(key, name)| {
                        matches!(key.as_str(), "file" | "File") && *name == entry.name
                    })
                })
        }) {
            classes.insert(class(property));
        }
        if entry.name == "GuiDocument.xml"
            || std::path::Path::new(&entry.name)
                .extension()
                .is_some_and(|extension| extension.as_encoded_bytes().eq_ignore_ascii_case(b"brp"))
        {
            classes.insert(ArenaClass::Model);
        }
        if classes.is_empty() {
            classes.extend([ArenaClass::Model, ArenaClass::Design]);
        }
        fidelity
            .mark_untyped(&entry.id, classes)
            .map_err(CodecError::malformed)?;
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

#[derive(Clone)]
pub(crate) struct ValueSourceRanges {
    id: String,
    record: Range<usize>,
    values: Vec<Range<usize>>,
}

impl ValueSourceRanges {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }
    pub(crate) fn record_range(&self) -> Range<usize> {
        self.record.clone()
    }
    pub(crate) fn has_values(&self) -> bool {
        !self.values.is_empty()
    }
}

pub(crate) fn state_source(
    node: roxmltree::Node<'_, '_>,
    state: &crate::native::GuiStateRecord,
) -> Result<ValueSourceRanges, CodecError> {
    value_source_from_node(&state.id, node, &state.values)
}

pub(crate) fn gui_property_source(
    node: roxmltree::Node<'_, '_>,
    property: &crate::native::GuiPropertyRecord,
) -> Result<ValueSourceRanges, CodecError> {
    value_source_from_node(&property.id, node, &property.values)
}

fn value_source_from_node(
    id: &str,
    node: roxmltree::Node<'_, '_>,
    values: &[crate::native::ValueRecord],
) -> Result<ValueSourceRanges, CodecError> {
    let mut ranges = Vec::new();
    for (order, value) in node
        .descendants()
        .filter(|value| value.is_element() && *value != node)
        .enumerate()
    {
        if !values
            .get(order)
            .is_some_and(|record| record.order == order && record.tag == value.tag_name().name())
        {
            return Err(CodecError::malformed(
                "XML value ranges differ from admitted typed values",
            ));
        }
        ranges.push(value.range());
    }
    if ranges.len() != values.len() {
        return Err(CodecError::malformed(
            "XML value range count differs from admitted typed values",
        ));
    }
    Ok(ValueSourceRanges {
        id: id.to_owned(),
        record: node.range(),
        values: ranges,
    })
}

pub(crate) fn value_sources<'a>(
    xml: &roxmltree::Document<'_>,
    owners: impl Iterator<
        Item = (
            &'a str,
            &'a crate::native::RetainedXml,
            &'a [crate::native::ValueRecord],
        ),
    >,
) -> Result<Vec<ValueSourceRanges>, CodecError> {
    let mut by_start = BTreeMap::new();
    for (id, source, values) in owners.filter(|(_, _, values)| !values.is_empty()) {
        let start = usize::try_from(source.start())
            .map_err(|_| CodecError::malformed("value owner start exceeds memory"))?;
        let end = usize::try_from(source.end())
            .map_err(|_| CodecError::malformed("value owner end exceeds memory"))?;
        if by_start.insert(start, (id, end, values)).is_some() {
            return Err(CodecError::malformed(
                "duplicate XML value owner source start",
            ));
        }
    }
    let mut output = Vec::new();
    for node in xml.descendants().filter(roxmltree::Node::is_element) {
        let Some((id, end, values)) = by_start.remove(&node.range().start) else {
            continue;
        };
        if end != node.range().end {
            return Err(CodecError::malformed(
                "XML value owner range differs from its parsed element",
            ));
        }
        output.push(value_source_from_node(id, node, values)?);
    }
    if !by_start.is_empty() {
        return Err(CodecError::malformed(
            "XML value owner has no original source element",
        ));
    }
    Ok(output)
}

pub(crate) fn value_layout_payloads(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    ranges: &ValueSourceRanges,
    payloads: &mut BTreeMap<String, BTreeMap<String, NativeBytes>>,
) -> Result<(), CodecError> {
    let extent = relative_xml_extent(source, ranges.record.clone())?;
    let fields = ctx.record(&ranges.id, extent, || {
        copy_value_fields(ctx, source, ranges)
    })?;
    if payloads.insert(ranges.id.clone(), fields).is_some() {
        return Err(CodecError::malformed("duplicate XML value layout identity"));
    }
    Ok(())
}

fn copy_value_fields(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    ranges: &ValueSourceRanges,
) -> Result<BTreeMap<String, NativeBytes>, CodecError> {
    let mut fields = BTreeMap::new();
    for (order, range) in ranges.values.iter().enumerate() {
        if range.start < ranges.record.start || range.end > ranges.record.end {
            return Err(CodecError::malformed("XML value escapes its owning record"));
        }
        let value = relative_xml_extent(source, range.clone())?;
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(value.window().len()),
            "retain FCStd value layout payload",
        )?;
        let mut bytes = ctx.alloc_filled(
            value.window().len(),
            0_u8,
            "retain FCStd value layout payload",
        )?;
        bytes.copy_from_slice(value.window());
        fields.insert(format!("values.{order}.raw_xml"), NativeBytes::from(bytes));
    }
    Ok(fields)
}

pub(crate) fn state_layout_payloads(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    ranges: &ValueSourceRanges,
    payloads: &mut BTreeMap<String, BTreeMap<String, NativeBytes>>,
    pointers: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<(), CodecError> {
    xml_value_layout_payloads(ctx, source, ranges, payloads, pointers)
}

pub(crate) fn object_layout_payloads(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    id: &str,
    range: Range<usize>,
    payloads: &mut BTreeMap<String, BTreeMap<String, NativeBytes>>,
    pointers: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<(), CodecError> {
    // Object fields have no ValueRecord children: properties own those separately.
    let ranges = ValueSourceRanges {
        id: id.to_owned(),
        record: range,
        values: Vec::new(),
    };
    xml_value_layout_payloads(ctx, source, &ranges, payloads, pointers)
}

pub(crate) fn gui_property_layout_payloads(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    ranges: &ValueSourceRanges,
    payloads: &mut BTreeMap<String, BTreeMap<String, NativeBytes>>,
    pointers: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<(), CodecError> {
    xml_value_layout_payloads(ctx, source, ranges, payloads, pointers)
}

fn xml_value_layout_payloads(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    ranges: &ValueSourceRanges,
    payloads: &mut BTreeMap<String, BTreeMap<String, NativeBytes>>,
    pointers: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<(), CodecError> {
    let extent = relative_xml_extent(source, ranges.record.clone())?;
    let fields = ctx.record(&ranges.id, extent, || {
        let mut fields = copy_value_fields(ctx, source, ranges)?;
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(extent.window().len()),
            "retain FCStd XML layout",
        )?;
        let mut bytes = ctx.alloc_filled(extent.window().len(), 0_u8, "retain FCStd XML layout")?;
        bytes.copy_from_slice(extent.window());
        fields.insert("raw_xml".into(), NativeBytes::from(bytes));
        Ok(fields)
    })?;
    if payloads.contains_key(&ranges.id) || pointers.contains_key(&ranges.id) {
        return Err(CodecError::malformed(
            "duplicate XML source layout identity",
        ));
    }
    payloads.insert(ranges.id.clone(), fields);
    pointers.insert(
        ranges.id.clone(),
        BTreeMap::from([
            (
                "byte_start".into(),
                cadmpeg_core::decode::u64_from_index(ranges.record.start),
            ),
            (
                "byte_end".into(),
                cadmpeg_core::decode::u64_from_index(ranges.record.end),
            ),
        ]),
    );
    Ok(())
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

pub(crate) fn xml_layout_payload(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    id: &str,
    range: Range<usize>,
    payloads: &mut BTreeMap<String, BTreeMap<String, NativeBytes>>,
) -> Result<(), CodecError> {
    let extent = relative_xml_extent(source, range)?;
    let bytes = ctx.record(id, extent, || {
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(extent.window().len()),
            "retain FCStd XML layout payload",
        )?;
        let mut bytes = ctx.alloc_filled(
            extent.window().len(),
            0_u8,
            "retain FCStd XML layout payload",
        )?;
        bytes.copy_from_slice(extent.window());
        Ok(NativeBytes::from(bytes))
    })?;
    if payloads
        .insert(id.to_owned(), BTreeMap::from([("raw_xml".into(), bytes)]))
        .is_some()
    {
        return Err(CodecError::malformed(
            "duplicate XML layout payload identity",
        ));
    }
    Ok(())
}

pub(crate) fn install_layouts(
    ctx: &DecodeContext<'_>,
    fidelity: &mut SourceFidelity,
    mut payloads: BTreeMap<String, BTreeMap<String, NativeBytes>>,
    mut pointers: BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<(), CodecError> {
    for capture in ctx.take_record_captures()? {
        let (id, mut layout) = RecordLayout::from_capture(capture);
        if let Some(fields) = payloads.remove(&id) {
            for (field, bytes) in fields {
                layout.insert_payload(field, bytes)?;
            }
        }
        if let Some(fields) = pointers.remove(&id) {
            for (field, pointer) in fields {
                layout.insert_pointer(field, pointer)?;
            }
        }
        fidelity.insert_record_layout(id, layout)?;
    }
    if !payloads.is_empty() || !pointers.is_empty() {
        return Err(CodecError::malformed(
            "XML payload has no completed record capture",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
