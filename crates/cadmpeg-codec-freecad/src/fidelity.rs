// SPDX-License-Identifier: Apache-2.0
//! Source-only archive and XML images. No native record serializes these bytes.

use std::collections::BTreeMap;
use std::ops::Range;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::{ids::UnknownId, RetainedBytes, RetainedSourceRecord, SourceFidelity};

pub(crate) mod b1;
pub(crate) mod owners;

pub(crate) const SOURCE_IMAGE: &str = "fcstd:source:image#archive";

pub(crate) fn retain(
    fidelity: &mut SourceFidelity,
    id: &str,
    stream: &str,
    offset: u64,
    bytes: &[u8],
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
            )?,
        )
        .map_err(CodecError::malformed)
}

#[derive(Clone)]
pub(crate) struct ValueSourceRanges {
    id: String,
    record: Range<usize>,
    values: Vec<(usize, Range<usize>)>,
}

impl ValueSourceRanges {
    #[cfg(test)]
    pub(crate) fn new(
        id: String,
        record: Range<usize>,
        values: Vec<Range<usize>>,
    ) -> Result<Self, CodecError> {
        Self::new_slots(id, record, values.into_iter().enumerate().collect())
    }

    pub(crate) fn new_slots(
        id: String,
        record: Range<usize>,
        values: Vec<(usize, Range<usize>)>,
    ) -> Result<Self, CodecError> {
        if record.start > record.end
            || values.iter().any(|(_, value)| {
                value.start > value.end || value.start < record.start || value.end > record.end
            })
            || values.windows(2).any(|pair| pair[0].0 >= pair[1].0)
        {
            return Err(CodecError::malformed(
                "XML value slots are unordered or escape their source record",
            ));
        }
        Ok(Self { id, record, values })
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }
    pub(crate) fn record_range(&self) -> Range<usize> {
        self.record.clone()
    }
    pub(crate) fn value_ranges(&self) -> impl Iterator<Item = (usize, Range<usize>)> + '_ {
        self.values
            .iter()
            .map(|(order, range)| (*order, range.clone()))
    }
    pub(crate) fn has_values(&self) -> bool {
        !self.values.is_empty()
    }
}

fn copy_value_fields(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    ranges: &ValueSourceRanges,
) -> Result<BTreeMap<String, NativeBytes>, CodecError> {
    let mut fields = BTreeMap::new();
    for (order, range) in &ranges.values {
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
    document: &str,
    order: usize,
    payloads: &mut BTreeMap<String, BTreeMap<String, NativeBytes>>,
    pointers: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<(), CodecError> {
    let prefix = format!("states.{order}.");
    if payloads
        .get(document)
        .is_some_and(|fields| fields.keys().any(|key| key.starts_with(&prefix)))
        || pointers
            .get(document)
            .is_some_and(|fields| fields.keys().any(|key| key.starts_with(&prefix)))
    {
        return Err(CodecError::malformed(
            "duplicate GUI state source layout identity",
        ));
    }
    let fields = copy_xml_value_layout(ctx, source, ranges)?;
    payloads.entry(document.to_owned()).or_default().extend(
        fields
            .into_iter()
            .map(|(name, bytes)| (format!("{prefix}{name}"), bytes)),
    );
    pointers.entry(document.to_owned()).or_default().extend([
        (
            format!("{prefix}byte_start"),
            cadmpeg_core::decode::u64_from_index(ranges.record.start),
        ),
        (
            format!("{prefix}byte_end"),
            cadmpeg_core::decode::u64_from_index(ranges.record.end),
        ),
    ]);
    Ok(())
}

#[cfg(test)]
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

pub(crate) fn captured_object_layout_payloads(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    id: &str,
    range: Range<usize>,
    payloads: &mut BTreeMap<String, BTreeMap<String, NativeBytes>>,
    pointers: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<(), CodecError> {
    let ranges = ValueSourceRanges {
        id: id.to_owned(),
        record: range,
        values: Vec::new(),
    };
    let fields = copy_xml_value_layout(ctx, source, &ranges)?;
    install_xml_value_fields(&ranges, fields, payloads, pointers)
}

#[cfg(test)]
pub(crate) fn property_layout_payloads(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    ranges: &ValueSourceRanges,
    payloads: &mut BTreeMap<String, BTreeMap<String, NativeBytes>>,
    pointers: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<(), CodecError> {
    xml_value_layout_payloads(ctx, source, ranges, payloads, pointers)
}

pub(crate) fn captured_property_layout_payloads(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    ranges: &ValueSourceRanges,
    property: &crate::native::PropertyRecord,
    payloads: &mut BTreeMap<String, BTreeMap<String, NativeBytes>>,
    pointers: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<(), CodecError> {
    let fields = if property.body.is_none() {
        BTreeMap::new()
    } else {
        copy_xml_value_layout(ctx, source, ranges)?
    };
    install_xml_value_fields(ranges, fields, payloads, pointers)
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
    let fields = copy_xml_value_layout(ctx, source, ranges)?;
    install_xml_value_fields(ranges, fields, payloads, pointers)
}

fn copy_xml_value_layout(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    ranges: &ValueSourceRanges,
) -> Result<BTreeMap<String, NativeBytes>, CodecError> {
    ctx.require_source(source)?;
    let extent = relative_xml_extent(source, ranges.record.clone())?;
    let mut fields = copy_value_fields(ctx, source, ranges)?;
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(extent.window().len()),
        "retain FCStd XML layout",
    )?;
    let mut bytes = ctx.alloc_filled(extent.window().len(), 0_u8, "retain FCStd XML layout")?;
    bytes.copy_from_slice(extent.window());
    fields.insert("raw_xml".into(), NativeBytes::from(bytes));
    Ok(fields)
}

fn install_xml_value_fields(
    ranges: &ValueSourceRanges,
    fields: BTreeMap<String, NativeBytes>,
    payloads: &mut BTreeMap<String, BTreeMap<String, NativeBytes>>,
    pointers: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<(), CodecError> {
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

#[cfg(test)]
pub(crate) fn xml_layout_payload(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    id: &str,
    range: Range<usize>,
    payloads: &mut BTreeMap<String, BTreeMap<String, NativeBytes>>,
) -> Result<(), CodecError> {
    let extent = relative_xml_extent(source, range.clone())?;
    ctx.record(id, extent, || {
        captured_xml_layout_payload(ctx, source, id, range, payloads)
    })
}

pub(crate) fn captured_xml_layout_payload(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    id: &str,
    range: Range<usize>,
    payloads: &mut BTreeMap<String, BTreeMap<String, NativeBytes>>,
) -> Result<(), CodecError> {
    ctx.require_source(source)?;
    if payloads.contains_key(id) {
        return Err(CodecError::malformed(
            "duplicate XML layout payload identity",
        ));
    }
    let extent = relative_xml_extent(source, range)?;
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
    payloads.insert(
        id.to_owned(),
        BTreeMap::from([("raw_xml".into(), NativeBytes::from(bytes))]),
    );
    Ok(())
}

pub(crate) fn install_layouts(
    ctx: &DecodeContext<'_>,
    fidelity: &mut SourceFidelity,
    mut payloads: BTreeMap<String, BTreeMap<String, NativeBytes>>,
    mut pointers: BTreeMap<String, BTreeMap<String, u64>>,
    pruning: Option<cadmpeg_ir::source_fidelity::parts::PrunedPartCaptures>,
) -> Result<(), CodecError> {
    let mut fields = |id: &str, layout: &mut cadmpeg_ir::source_fidelity::RecordLayout| {
        if let Some(fields) = payloads.remove(id) {
            for (field, bytes) in fields {
                layout.insert_payload(field, bytes)?;
            }
        }
        if let Some(fields) = pointers.remove(id) {
            for (field, pointer) in fields {
                layout.insert_pointer(field, pointer)?;
            }
        }
        Ok(())
    };
    if let Some(pruning) = pruning {
        fidelity.install_pruned_record_captures(ctx, pruning, &mut fields)?;
    } else {
        fidelity.install_record_captures(ctx, &mut fields)?;
    }
    if !payloads.is_empty() || !pointers.is_empty() {
        return Err(CodecError::malformed(format!(
            "XML payload has no completed record capture: payload owners {:?}; pointer owners {:?}",
            payloads.keys(),
            pointers.keys()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

pub(crate) fn gui_layout_payloads(
    ctx: &DecodeContext<'_>,
    entries: &BTreeMap<String, View<'_>>,
    graph: &crate::gui::Graph,
    payloads: &mut BTreeMap<String, BTreeMap<String, NativeBytes>>,
    pointers: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<(), CodecError> {
    for document in &graph.documents {
        for (order, state) in document.states.iter().enumerate() {
            let source = graph
                .state_sources
                .get(&state.id)
                .ok_or_else(|| CodecError::malformed("GUI state has no actual source range"))?;
            state_layout_payloads(
                ctx,
                *entries
                    .get("GuiDocument.xml")
                    .ok_or_else(|| CodecError::malformed("GUI state has no source entry"))?,
                source,
                &document.id,
                order,
                payloads,
                pointers,
            )?;
        }
    }
    for source in graph
        .value_sources
        .iter()
        .filter(|source| graph.property_sources.contains_key(source.id()))
        .chain(
            graph
                .property_sources
                .values()
                .filter(|source| !source.has_values()),
        )
    {
        gui_property_layout_payloads(
            ctx,
            *entries
                .get("GuiDocument.xml")
                .ok_or_else(|| CodecError::malformed("GUI property has no source entry"))?,
            source,
            payloads,
            pointers,
        )?;
    }
    for (id, range) in &graph.provider_sources {
        let view = entries
            .get("GuiDocument.xml")
            .ok_or_else(|| CodecError::malformed("GUI provider has no source entry"))?;
        captured_xml_layout_payload(ctx, *view, id, range.clone(), payloads)?;
    }
    Ok(())
}
