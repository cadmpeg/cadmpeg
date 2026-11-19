// SPDX-License-Identifier: Apache-2.0
//! The layer table of a written archive: the `rhino` native layers the write
//! re-emits (§3.4, Rule W1), then the neutral presentation layers, and the
//! layer each written object is placed on.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::presentation::PresentationItem;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{NativeNamespace, NativeRecord};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::loss::RhinoLossCode;

/// The fields of a native layer record. A record that states another field is
/// not re-emitted.
const LAYER_FIELDS: &[&str] = &[
    "archive_index",
    "clipping_planes_enabled",
    "color",
    "description",
    "display_material_uuid",
    "expanded",
    "id",
    "iges_level",
    "linetype_index",
    "locked",
    "material_index",
    "name",
    "parent_uuid",
    "per_viewport_settings",
    "plot_color",
    "plot_weight_mm",
    "rendering_materials",
    "source_offset",
    "source_uuid",
    "visible",
    "visible_in_new_details",
];

/// One layer record of the written table.
struct Layer {
    name: String,
    color: [u8; 4],
    visible: bool,
    locked: bool,
    iges_level: i32,
    plot_color: [u8; 4],
    plot_weight_mm: f64,
    /// The layer UUID in archive byte order.
    id: [u8; 16],
    /// The parent UUID in archive byte order (nil for a root layer) and the
    /// expanded state, written in a version 1.6 record.
    hierarchy: Option<([u8; 16], bool)>,
}

impl Layer {
    /// The layer every object of a document without a written layer is on.
    fn default_layer() -> Self {
        Self {
            name: "Default".into(),
            color: [0; 4],
            visible: true,
            locked: false,
            iges_level: -1,
            plot_color: [0; 4],
            plot_weight_mm: 0.0,
            id: minted_id("cadmpeg:default-layer"),
            hierarchy: None,
        }
    }

    /// The `ON_Layer` class data at archive index `index`.
    fn payload(&self, index: i32) -> Result<Vec<u8>, CodecError> {
        let mut payload = vec![if self.hierarchy.is_some() { 0x16 } else { 0x15 }];
        let mode = if self.visible {
            0_i32
        } else if self.locked {
            2
        } else {
            1
        };
        payload.extend(mode.to_le_bytes());
        payload.extend(index.to_le_bytes());
        payload.extend(self.iges_level.to_le_bytes());
        payload.extend((-1_i32).to_le_bytes());
        payload.extend((-1_i32).to_le_bytes());
        payload.extend(self.color);
        payload.extend(0_i16.to_le_bytes());
        payload.extend(0_i16.to_le_bytes());
        payload.extend(0.0_f64.to_le_bytes());
        payload.extend(1.0_f64.to_le_bytes());
        payload.extend(super::utf16(&self.name)?);
        payload.push(u8::from(self.visible));
        payload.extend((-1_i32).to_le_bytes());
        payload.extend(self.plot_color);
        payload.extend(self.plot_weight_mm.to_le_bytes());
        payload.push(u8::from(self.locked));
        payload.extend(self.id);
        if let Some((parent, expanded)) = self.hierarchy {
            payload.extend(parent);
            payload.push(u8::from(expanded));
        }
        Ok(payload)
    }
}

/// A native layer the write can re-emit, before its parent is checked.
struct Candidate<'a> {
    record: &'a NativeRecord,
    archive_index: i64,
    /// The source layer reference objects name it by: its UUID text, or its
    /// name when it has no UUID.
    key: String,
    /// The parent UUID text.
    parent: Option<String>,
    layer: Layer,
}

/// The layer table and the layer of each object.
pub(super) struct LayerTable {
    layers: Vec<Layer>,
    /// The written index of each written native layer, by source layer
    /// reference.
    by_source: BTreeMap<String, i32>,
    /// The layer each object is placed on, by object id; `None` where that
    /// layer is not written.
    by_object: BTreeMap<String, Option<i32>>,
    /// The native layer records the table re-emits.
    written: BTreeSet<String>,
    /// The native object presentation records whose content is the written
    /// object attributes.
    regenerated_presentations: BTreeSet<String>,
    losses: Vec<LossNote>,
}

impl LayerTable {
    /// Builds the table of `ir`.
    ///
    /// # Errors
    ///
    /// [`CodecError::ResourceLimit`] when the table holds more layers than
    /// an archive index addresses.
    pub(super) fn new(ir: &CadIr) -> Result<Self, CodecError> {
        let namespace = ir.native.namespace("rhino");
        let candidates = native_candidates(arena(namespace, "layers"));
        let mut table = Self {
            layers: Vec::new(),
            by_source: BTreeMap::new(),
            by_object: BTreeMap::new(),
            written: BTreeSet::new(),
            regenerated_presentations: BTreeSet::new(),
            losses: Vec::new(),
        };
        if candidates.first().is_none_or(|first| first.archive_index != 0) {
            table.layers.push(Layer::default_layer());
        }
        let mut by_archive_index = BTreeMap::new();
        for candidate in candidates {
            let index = table.next_index()?;
            by_archive_index.insert(candidate.archive_index, index);
            table.by_source.entry(candidate.key).or_insert(index);
            table.written.insert(candidate.record.id().to_owned());
            table.layers.push(candidate.layer);
        }
        table.place_native_objects(namespace, &by_archive_index);
        table.add_presentation_layers(ir)?;
        Ok(table)
    }

    fn next_index(&self) -> Result<i32, CodecError> {
        super::native_i32_count(self.layers.len(), "layer table exceeds native counts".into())
    }

    /// Places each object an object presentation record links to on that
    /// record's layer.
    fn place_native_objects(
        &mut self,
        namespace: Option<&NativeNamespace>,
        by_archive_index: &BTreeMap<i64, i32>,
    ) {
        let links = arena(namespace, "unknowns")
            .iter()
            .map(|record| (record.id(), string_array(record.field("links"))))
            .collect::<BTreeMap<_, _>>();
        for record in arena(namespace, "object_presentation") {
            let Ok(fields) = record.fields() else {
                continue;
            };
            let Some(layer) = fields.get("layer_index").and_then(Value::as_i64) else {
                continue;
            };
            let written = by_archive_index.get(&layer).copied();
            if written.is_some() && super::default_native_presentation(&fields) {
                self.regenerated_presentations
                    .insert(record.id().to_owned());
            }
            for link in string_array(fields.get("links").cloned()) {
                for object in links.get(link.as_str()).into_iter().flatten() {
                    self.by_object.entry(object.clone()).or_insert(written);
                }
            }
        }
    }

    /// Appends each neutral presentation layer and places the objects it
    /// lists.
    fn add_presentation_layers(&mut self, ir: &CadIr) -> Result<(), CodecError> {
        let mut unplaced = Vec::new();
        for layer in &ir.model.presentation_layers {
            if layer.name.contains('\0') {
                self.losses.push(RhinoLossCode::WriterLayerReduced.note(format!(
                    "layer {} was omitted: its name holds a null character, which a Rhino \
                     string cannot state",
                    layer.id.as_str()
                )));
                continue;
            }
            if layer.description.is_some() {
                unplaced.push(format!("the description of layer {}", layer.id.as_str()));
            }
            let index = self.next_index()?;
            self.layers.push(Layer {
                name: layer.name.clone(),
                visible: layer.visible.unwrap_or(true),
                id: minted_id(layer.id.as_str()),
                ..Layer::default_layer()
            });
            for item in &layer.items {
                let object = match &item.entity {
                    PresentationItem::Body { body } => Some(body.as_str()),
                    PresentationItem::Point { point } => Some(point.as_str()),
                    PresentationItem::Curve { curve } => Some(curve.as_str()),
                    PresentationItem::Surface { surface } => Some(surface.as_str()),
                    PresentationItem::Tessellation { tessellation } => {
                        Some(tessellation.as_str())
                    }
                    _ => None,
                }
                .filter(|_| item.path.is_none() && item.state.is_none());
                match object {
                    Some(object) if !self.by_object.contains_key(object) => {
                        self.by_object.insert(object.to_owned(), Some(index));
                    }
                    Some(object) => unplaced.push(format!(
                        "object {object} on layer {}: a Rhino object is on one layer",
                        layer.id.as_str()
                    )),
                    None => unplaced.push(format!(
                        "an item of layer {}: a Rhino layer holds whole objects of the model",
                        layer.id.as_str()
                    )),
                }
            }
        }
        if !unplaced.is_empty() {
            self.losses.push(RhinoLossCode::WriterLayerReduced.note(format!(
                "{} layer assignment(s) or description(s) were omitted: {}",
                unplaced.len(),
                unplaced.join("; ")
            )));
        }
        Ok(())
    }

    /// The encoded layer table records.
    pub(super) fn records(&self) -> Result<Vec<Vec<u8>>, CodecError> {
        self.layers
            .iter()
            .enumerate()
            .map(|(index, layer)| {
                let index = super::native_i32_count(index, "layer table exceeds native counts".into())?;
                super::zero_crc_chunk(
                    super::TCODE_LAYER_RECORD,
                    &super::class_wrapper(super::LAYER_CLASS, &layer.payload(index)?)?,
                )
            })
            .collect()
    }

    /// The layer index of object `object`, whose source names `source_layer`,
    /// and whether the object is on a layer the table does not write.
    pub(super) fn object_layer(&self, object: &str, source_layer: Option<&str>) -> (i32, bool) {
        match self.by_object.get(object) {
            Some(Some(index)) => (*index, false),
            Some(None) => (0, true),
            None => source_layer.map_or((0, false), |layer| {
                self.by_source
                    .get(layer)
                    .map_or((0, true), |index| (*index, false))
            }),
        }
    }

    /// Whether the table re-emits native layer record `id`.
    pub(super) fn writes_native_layer(&self, id: &str) -> bool {
        self.written.contains(id)
    }

    /// Whether the written object attributes state all of native object
    /// presentation record `id`.
    pub(super) fn regenerates_presentation(&self, id: &str) -> bool {
        self.regenerated_presentations.contains(id)
    }

    /// The losses of the layers and assignments the table omits.
    pub(super) fn into_losses(self) -> Vec<LossNote> {
        self.losses
    }
}

/// The native layers the write can re-emit, in archive order, each with a
/// parent among them.
fn native_candidates(records: &[NativeRecord]) -> Vec<Candidate<'_>> {
    let mut candidates = records.iter().filter_map(candidate).collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| candidate.archive_index);
    candidates.dedup_by_key(|candidate| candidate.archive_index);
    loop {
        let keys = candidates
            .iter()
            .map(|candidate| candidate.key.clone())
            .collect::<BTreeSet<_>>();
        let before = candidates.len();
        candidates.retain(|candidate| {
            candidate
                .parent
                .as_ref()
                .is_none_or(|parent| keys.contains(parent))
        });
        if candidates.len() == before {
            return candidates;
        }
    }
}

/// `record` as a layer the written record holds whole: every reference it
/// states resolves in the written archive and every field is one the record
/// encodes.
fn candidate(record: &NativeRecord) -> Option<Candidate<'_>> {
    let fields = record.fields().ok()?;
    let null = |name: &str| fields.get(name).is_none_or(Value::is_null);
    let empty = |name: &str| {
        fields
            .get(name)
            .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty))
    };
    if fields.keys().any(|key| !LAYER_FIELDS.contains(&key.as_str()))
        || fields.get("material_index").and_then(Value::as_i64) != Some(-1)
        || !(null("linetype_index")
            || fields.get("linetype_index").and_then(Value::as_i64) == Some(-1))
        || !null("description")
        || !null("display_material_uuid")
        || !null("clipping_planes_enabled")
        || !null("visible_in_new_details")
        || !empty("rendering_materials")
        || !empty("per_viewport_settings")
    {
        return None;
    }
    let name = fields.get("name")?.as_str()?;
    if name.contains('\0') {
        return None;
    }
    let iges_level = match fields.get("iges_level") {
        None | Some(Value::Null) => -1,
        Some(value) => i32::try_from(value.as_i64()?).ok()?,
    };
    let plot_color = match fields.get("plot_color") {
        None | Some(Value::Null) => [0; 4],
        Some(value) => color_array(value)?,
    };
    let plot_weight_mm = match fields.get("plot_weight_mm") {
        None | Some(Value::Null) => 0.0,
        Some(value) => value.as_f64().filter(|value| value.is_finite())?,
    };
    let source_uuid = match fields.get("source_uuid") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_str()?),
    };
    let id = match source_uuid {
        Some(text) => wire_uuid(text)?,
        None => minted_id(record.id()),
    };
    let parent = match fields.get("parent_uuid") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_str()?),
    };
    let hierarchy = match (parent, fields.get("expanded").and_then(Value::as_bool)) {
        (None, None) => None,
        (parent, expanded) => Some((
            parent.map_or(Some([0; 16]), wire_uuid)?,
            expanded.unwrap_or(false),
        )),
    };
    Some(Candidate {
        record,
        archive_index: fields.get("archive_index")?.as_i64()?,
        key: source_uuid.unwrap_or(name).to_owned(),
        parent: parent.map(str::to_owned),
        layer: Layer {
            name: name.to_owned(),
            color: hex_color(fields.get("color")?.as_str()?)?,
            visible: fields.get("visible")?.as_bool()?,
            locked: fields.get("locked")?.as_bool()?,
            iges_level,
            plot_color,
            plot_weight_mm,
            id,
            hierarchy,
        },
    })
}

fn arena<'a>(namespace: Option<&'a NativeNamespace>, name: &str) -> &'a [NativeRecord] {
    namespace
        .and_then(|namespace| namespace.arenas().get(name))
        .map_or(&[], Vec::as_slice)
}

fn string_array(value: Option<Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A UUID minted from `seed`, in archive byte order.
fn minted_id(seed: &str) -> [u8; 16] {
    let digest = Sha256::digest(seed.as_bytes());
    let mut id = [0; 16];
    id.copy_from_slice(&digest[..16]);
    id
}

/// The archive byte order of UUID text.
fn wire_uuid(text: &str) -> Option<[u8; 16]> {
    crate::presentation::parse_uuid_text(text).map(crate::wire::Uuid::to_wire)
}

/// Four colour bytes from eight hexadecimal digits.
fn hex_color(text: &str) -> Option<[u8; 4]> {
    if text.len() != 8 || !text.is_ascii() {
        return None;
    }
    let mut color = [0; 4];
    for (index, byte) in color.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(color)
}

/// Four colour bytes from a JSON array of four bytes.
fn color_array(value: &Value) -> Option<[u8; 4]> {
    let items = value.as_array()?;
    if items.len() != 4 {
        return None;
    }
    let mut color = [0; 4];
    for (byte, item) in color.iter_mut().zip(items) {
        *byte = u8::try_from(item.as_u64()?).ok()?;
    }
    Some(color)
}

