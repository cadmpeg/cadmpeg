// SPDX-License-Identifier: Apache-2.0
//! Typed presentation tables authored from current document state.

use super::{
    attributed_object_record, class_wrapper, class_wrapper_with_direct, crc_chunk,
    crc_chunk_with_direct, default_layer_payload, framed_object_record, native_count_limit,
    neutral_object_uuid, object_attributes_payload, short_chunk, unit_color_channel, utf16,
    wire_index, CadIr, CodecError, LossNote, RhinoLossCode, LAYER_CLASS, TCODE_ENDOFTABLE,
    TCODE_LAYER_RECORD,
};
use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
use serde_json::{Map, Value};

pub(super) mod annotations;
pub(super) mod document;
pub(super) mod fonts;
pub(super) mod hatches;
pub(super) mod images;
pub(super) mod lights;
pub(super) mod materials;
pub(super) mod native;
pub(super) mod products;
pub(super) mod views;

fn records<'a>(ir: &'a CadIr, arena: &str) -> &'a [cadmpeg_ir::NativeRecord] {
    ir.native
        .namespace("rhino")
        .and_then(|namespace| namespace.arenas().get(arena))
        .map_or(&[], Vec::as_slice)
}

fn object_record(ir: &CadIr, identity: &str) -> Option<String> {
    let mut native = None;
    cadmpeg_ir::schema::native_links::visit_native_references(
        &ir.model,
        &mut |_, _, id, reference| {
            if id == identity {
                native = reference.map(str::to_owned);
            }
        },
    );
    native
}

fn source_record_resolves(ir: &CadIr, source: &str) -> bool {
    records(ir, "unknowns")
        .iter()
        .any(|record| record.id() == source)
}

pub(super) fn own_object(ir: &CadIr, identity: &str) -> Option<Map<String, Value>> {
    let source = object_record(ir, identity)?;
    records(ir, "object_presentation")
        .iter()
        .find_map(|record| {
            let fields = record.fields().ok()?;
            fields
                .get("links")?
                .as_array()?
                .iter()
                .any(|link| link.as_str() == Some(source.as_str()))
                .then_some(fields)
        })
}

fn uuid(value: Option<&Value>, identity: &str) -> [u8; 16] {
    value
        .and_then(Value::as_str)
        .and_then(crate::presentation::parse_uuid_text)
        .map_or_else(|| neutral_object_uuid(identity), crate::wire::Uuid::to_wire)
}

fn anonymous(major: i32, minor: i32, fields: &[u8]) -> Result<Vec<u8>, CodecError> {
    anonymous_with_children(major, minor, fields, &[])
}

fn anonymous_with_children(
    major: i32,
    minor: i32,
    fields: &[u8],
    children: &[std::ops::Range<usize>],
) -> Result<Vec<u8>, CodecError> {
    let mut body = major.to_le_bytes().to_vec();
    body.extend(minor.to_le_bytes());
    body.extend(fields);
    let children = children
        .iter()
        .map(|range| range.start + 8..range.end + 8)
        .collect::<Vec<_>>();
    let ranges = crate::chunks::direct_checksum_ranges(&(0..body.len()), &children)
        .map_err(CodecError::malformed)?;
    let mut direct = Vec::new();
    for range in &ranges {
        direct.extend_from_slice(&body[range]);
    }
    crc_chunk_with_direct(0x4000_8000, &body, &direct)
}

fn component(
    identity: &str,
    index: i32,
    name: &str,
    source_uuid: Option<&Value>,
) -> Result<Vec<u8>, CodecError> {
    let mut fields = 13u32.to_le_bytes().to_vec();
    fields.extend(uuid(source_uuid, identity));
    fields.extend(index.to_le_bytes());
    fields.extend(utf16(name)?);
    anonymous(1, 0, &fields)
}

fn own_layer(ir: &CadIr, layer: &PresentationLayer) -> Option<Map<String, Value>> {
    let id = layer.native_ref.as_deref()?;
    records(ir, "layers")
        .iter()
        .find(|record| record.id() == id)?
        .fields()
        .ok()
}

fn integer(fields: Option<&Map<String, Value>>, name: &str, default: i32) -> i32 {
    fields
        .and_then(|fields| fields.get(name))
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(default)
}

fn color(fields: Option<&Map<String, Value>>, name: &str) -> Result<[u8; 4], CodecError> {
    let Some(value) = fields
        .and_then(|fields| fields.get(name))
        .filter(|value| !value.is_null())
    else {
        return Ok([0, 0, 0, 0]);
    };
    let color: cadmpeg_ir::topology::Color =
        crate::wire::native_color(value).map_err(CodecError::malformed)?;
    Ok([
        unit_color_channel(color.r())?,
        unit_color_channel(color.g())?,
        unit_color_channel(color.b())?,
        unit_color_channel(1.0 - color.a())?,
    ])
}

pub(super) fn layers(ir: &CadIr) -> Result<Vec<Vec<u8>>, CodecError> {
    if ir.model.presentation_layers.is_empty() {
        return Ok(vec![crc_chunk_with_direct(
            TCODE_LAYER_RECORD,
            &class_wrapper(LAYER_CLASS, &default_layer_payload()?)?,
            &[],
        )?]);
    }
    let mut records = Vec::new();
    for (index, layer) in ir.model.presentation_layers.iter().enumerate() {
        let native = own_layer(ir, layer);
        let fields = native.as_ref();
        let mut payload = vec![0x16];
        payload.extend(0_i32.to_le_bytes());
        payload.extend(
            i32::try_from(index)
                .map_err(|_| {
                    native_count_limit(2_147_483_647, cadmpeg_core::decode::u64_from_index(index))
                })?
                .to_le_bytes(),
        );
        payload.extend(integer(fields, "iges_level", -1).to_le_bytes());
        payload.extend(integer(fields, "material_index", -1).to_le_bytes());
        payload.extend((-1_i32).to_le_bytes());
        payload.extend(color(fields, "color")?);
        payload.extend(0_i16.to_le_bytes());
        payload.extend(0_i16.to_le_bytes());
        payload.extend(0.0_f64.to_le_bytes());
        payload.extend(1.0_f64.to_le_bytes());
        payload.extend(utf16(&layer.name)?);
        payload.push(u8::from(layer.visible.unwrap_or(true)));
        payload.extend(integer(fields, "linetype_index", -1).to_le_bytes());
        payload.extend(color(fields, "plot_color")?);
        payload.extend(
            fields
                .and_then(|fields| fields.get("plot_weight_mm"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .to_le_bytes(),
        );
        payload.push(u8::from(
            fields
                .and_then(|fields| fields.get("locked"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ));
        payload.extend(neutral_object_uuid(layer.id.as_str()));
        payload.extend(
            layer
                .parent
                .as_ref()
                .map_or([0; 16], |parent| neutral_object_uuid(parent.as_str())),
        );
        payload.push(u8::from(
            fields
                .and_then(|fields| fields.get("expanded"))
                .and_then(Value::as_bool)
                .unwrap_or(true),
        ));
        records.push(crc_chunk_with_direct(
            TCODE_LAYER_RECORD,
            &class_wrapper(LAYER_CLASS, &payload)?,
            &[],
        )?);
    }
    Ok(records)
}

fn item_identity(item: &PresentationItem) -> &str {
    match item {
        PresentationItem::Body { body } => body.as_str(),
        PresentationItem::Face { face } => face.as_str(),
        PresentationItem::Edge { edge } => edge.as_str(),
        PresentationItem::Vertex { vertex } => vertex.as_str(),
        PresentationItem::Point { point } => point.as_str(),
        PresentationItem::Curve { curve } => curve.as_str(),
        PresentationItem::Surface { surface } => surface.as_str(),
        PresentationItem::Product { product } => product.as_str(),
        PresentationItem::Occurrence { occurrence } => occurrence.as_str(),
        PresentationItem::Pmi { annotation } => annotation.as_str(),
        PresentationItem::Tessellation { tessellation } => tessellation,
        PresentationItem::Source { source_id } => source_id.as_str(),
    }
}

pub(super) fn layer_index(ir: &CadIr, identity: &str) -> Result<i32, CodecError> {
    ir.model
        .presentation_layers
        .iter()
        .position(|layer| {
            layer
                .items
                .iter()
                .any(|item| item_identity(item) == identity)
        })
        .map_or(Ok(0), |index| {
            i32::try_from(index).map_err(|_| {
                native_count_limit(2_147_483_647, cadmpeg_core::decode::u64_from_index(index))
            })
        })
}

pub(super) fn native_layer_complete(record: &cadmpeg_ir::NativeRecord) -> bool {
    let Ok(fields) = record.fields() else {
        return false;
    };
    fields
        .get("display_material_uuid")
        .is_none_or(Value::is_null)
        && fields
            .get("clipping_planes_enabled")
            .is_none_or(|value| value.is_null() || value.as_bool() == Some(true))
        && fields
            .get("visible_in_new_details")
            .is_none_or(|value| value.is_null() || value.as_bool() == Some(true))
        && ["rendering_materials", "per_viewport_settings"]
            .iter()
            .all(|key| {
                fields
                    .get(*key)
                    .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty))
            })
}

#[cfg(test)]
mod tests;
