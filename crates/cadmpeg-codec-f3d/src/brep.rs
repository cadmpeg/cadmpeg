// SPDX-License-Identifier: Apache-2.0
//! Wrap the format-independent ASM B-rep graph with Fusion-derived links,
//! blob-scoped id qualification, and Design body-map selector resolution.

use crate::records::{
    recipes::CreationTimestamp,
    sketch_links::{PersistentDesignLink, PersistentSubentityTag, SketchCurveLink},
};
use cadmpeg_asm::brep::attributes::attribute_key;
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_asm::brep::{decode_with_header, decode_with_purpose, retain_root_entities, AsmBrep, DecodePurpose};
use cadmpeg_asm::ids::IdFormat;
use cadmpeg_asm::sab::Record;
use cadmpeg_core::decode::{bounded_len, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::BodyId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

fn copy_brep_text(
    ctx: &DecodeContext<'_>,
    value: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let length = u64::try_from(value.len())
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    ctx.charge_retained(length, operation)?;
    let mut copy = String::new();
    copy.try_reserve(value.len())
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, length))?;
    copy.push_str(value);
    Ok(copy)
}

fn push_brep_item<T>(
    ctx: &DecodeContext<'_>,
    items: &mut Vec<T>,
    value: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    items.try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    items.push(value);
    Ok(())
}

fn append_brep_items<T>(
    ctx: &DecodeContext<'_>,
    target: &mut Vec<T>,
    source: &mut Vec<T>,
    operation: &'static str,
) -> Result<(), CodecError> {
    let count = u64::try_from(source.len())
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    ctx.charge_collection_items(count, operation)?;
    target.try_reserve(source.len())
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, count))?;
    target.append(source);
    Ok(())
}

fn merge_brep_counts(
    ctx: &DecodeContext<'_>,
    target: &mut BTreeMap<String, usize>,
    source: BTreeMap<String, usize>,
) -> Result<(), CodecError> {
    use std::collections::btree_map::Entry;
    for (kind, count) in source {
        match target.entry(kind) {
            Entry::Occupied(mut entry) => *entry.get_mut() += count,
            Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "merge F3D BREP statistic kinds")?;
                entry.insert(count);
            }
        }
    }
    Ok(())
}

fn collect_owned_ids_charged(
    ctx: &DecodeContext<'_>,
    value: &serde_value::Value,
    owned: &mut HashSet<String>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("walk F3D BREP owned IDs")?;
    if let Some(id) = cadmpeg_asm::brep::entity_id(value) {
        if !owned.contains(id) {
            let key = copy_brep_text(ctx, id, "copy F3D BREP owned ID")?;
            ctx.charge_collection_items(1, "index F3D BREP owned IDs")?;
            owned.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index F3D BREP owned IDs", 0, 1))?;
            owned.insert(key);
        }
    }
    match value {
        serde_value::Value::Map(fields) => {
            for (key, item) in fields {
                collect_owned_ids_charged(ctx, key, owned)?;
                collect_owned_ids_charged(ctx, item, owned)?;
            }
        }
        serde_value::Value::Seq(items) => {
            for item in items { collect_owned_ids_charged(ctx, item, owned)?; }
        }
        serde_value::Value::Option(Some(item)) | serde_value::Value::Newtype(item) => {
            collect_owned_ids_charged(ctx, item, owned)?;
        }
        _ => {}
    }
    Ok(())
}

fn remap_owned_ids_charged(
    ctx: &DecodeContext<'_>,
    value: &mut serde_value::Value,
    replacements: &HashMap<String, String>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("remap F3D BREP owned IDs")?;
    match value {
        serde_value::Value::String(id) => {
            if let Some(replacement) = replacements.get(id) {
                *id = copy_brep_text(ctx, replacement, "copy F3D BREP remapped ID")?;
            }
        }
        serde_value::Value::Seq(items) => {
            for item in items { remap_owned_ids_charged(ctx, item, replacements)?; }
        }
        serde_value::Value::Map(fields) => {
            let entries = std::mem::take(fields);
            for (mut key, mut item) in entries {
                remap_owned_ids_charged(ctx, &mut key, replacements)?;
                remap_owned_ids_charged(ctx, &mut item, replacements)?;
                ctx.charge_collection_items(1, "rebuild F3D BREP value map")?;
                fields.insert(key, item);
            }
        }
        serde_value::Value::Option(Some(item)) | serde_value::Value::Newtype(item) => {
            remap_owned_ids_charged(ctx, item, replacements)?;
        }
        _ => {}
    }
    Ok(())
}

fn insert_brep_string(
    ctx: &DecodeContext<'_>,
    values: &mut HashSet<String>,
    value: String,
    operation: &'static str,
) -> Result<(), CodecError> {
    if values.contains(&value) { return Ok(()); }
    ctx.charge_collection_items(1, operation)?;
    values.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    values.insert(value);
    Ok(())
}

fn collect_brep_references(
    ctx: &DecodeContext<'_>,
    value: &serde_value::Value,
    owned: &HashSet<String>,
    references: &mut HashSet<String>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("walk F3D BREP references")?;
    match value {
        serde_value::Value::String(id) if owned.contains(id) && !references.contains(id) => {
            let id = copy_brep_text(ctx, id, "copy F3D BREP adjacency reference")?;
            insert_brep_string(ctx, references, id, "collect F3D BREP adjacency references")?;
        }
        serde_value::Value::Seq(items) => {
            for item in items { collect_brep_references(ctx, item, owned, references)?; }
        }
        serde_value::Value::Map(fields) => {
            for (key, item) in fields {
                collect_brep_references(ctx, key, owned, references)?;
                collect_brep_references(ctx, item, owned, references)?;
            }
        }
        serde_value::Value::Option(Some(item)) | serde_value::Value::Newtype(item) => {
            collect_brep_references(ctx, item, owned, references)?;
        }
        _ => {}
    }
    Ok(())
}

fn insert_brep_adjacency(
    ctx: &DecodeContext<'_>,
    adjacency: &mut HashMap<String, HashSet<String>>,
    source: &str,
    target: &str,
) -> Result<(), CodecError> {
    if !adjacency.contains_key(source) {
        let source = copy_brep_text(ctx, source, "copy F3D BREP adjacency source")?;
        ctx.charge_collection_items(1, "index F3D BREP adjacency")?;
        adjacency.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index F3D BREP adjacency", 0, 1))?;
        adjacency.insert(source, HashSet::new());
    }
    if let Some(targets) = adjacency.get_mut(source) {
        if !targets.contains(target) {
            let target = copy_brep_text(ctx, target, "copy F3D BREP adjacency target")?;
            insert_brep_string(ctx, targets, target, "collect F3D BREP adjacent IDs")?;
        }
    }
    Ok(())
}

fn collect_brep_adjacency(
    ctx: &DecodeContext<'_>,
    value: &serde_value::Value,
    owned: &HashSet<String>,
) -> Result<HashMap<String, HashSet<String>>, CodecError> {
    let mut adjacency = HashMap::new();
    let serde_value::Value::Map(fields) = value else { return Ok(adjacency); };
    for value in fields.values() {
        let serde_value::Value::Seq(items) = value else { continue; };
        for item in items {
            let Some(id) = cadmpeg_asm::brep::entity_id(item) else { continue; };
            let mut references = HashSet::new();
            collect_brep_references(ctx, item, owned, &mut references)?;
            references.remove(id);
            for reference in references {
                insert_brep_adjacency(ctx, &mut adjacency, id, &reference)?;
                insert_brep_adjacency(ctx, &mut adjacency, &reference, id)?;
            }
        }
    }
    Ok(adjacency)
}

fn copy_attribute_target(ctx: &DecodeContext<'_>, target: &AttributeTarget) -> Result<AttributeTarget, CodecError> {
    macro_rules! copy_id {
        ($id:ident, $type:ident, $variant:ident) => {
            AttributeTarget::$variant(cadmpeg_ir::ids::$type::mint(copy_brep_text(
                ctx,
                $id.as_str(),
                "copy F3D BREP attribute target",
            )?).map_err(CodecError::malformed)?)
        };
    }
    Ok(match target {
        AttributeTarget::Document => AttributeTarget::Document,
        AttributeTarget::Body(id) => copy_id!(id, BodyId, Body),
        AttributeTarget::Face(id) => copy_id!(id, FaceId, Face),
        AttributeTarget::Shell(id) => copy_id!(id, ShellId, Shell),
        AttributeTarget::Loop(id) => copy_id!(id, LoopId, Loop),
        AttributeTarget::Coedge(id) => copy_id!(id, CoedgeId, Coedge),
        AttributeTarget::Edge(id) => copy_id!(id, EdgeId, Edge),
        AttributeTarget::Vertex(id) => copy_id!(id, VertexId, Vertex),
    })
}

pub(crate) fn copy_body_id(ctx: &DecodeContext<'_>, body: &BodyId) -> Result<BodyId, CodecError> {
    BodyId::mint(copy_brep_text(ctx, body.as_str(), "copy F3D BREP body ID" )?)
        .map_err(CodecError::malformed)
}

struct BrepFormatLength(usize);

impl std::fmt::Write for BrepFormatLength {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.0 = self.0.checked_add(text.len()).ok_or(std::fmt::Error)?;
        Ok(())
    }
}

fn format_brep_text(
    ctx: &DecodeContext<'_>,
    args: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut length = BrepFormatLength(0);
    std::fmt::write(&mut length, args.clone())
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    let count = u64::try_from(length.0)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    ctx.charge_retained(count, operation)?;
    let mut text = String::new();
    text.try_reserve(length.0)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, count))?;
    std::fmt::write(&mut text, args)
        .map_err(|_| CodecError::Malformed("F3D BREP identity formatting failed".into()))?;
    Ok(text)
}

/// The ASM B-rep graph plus links derived from Fusion attribute records.
///
/// Kernel arenas are flattened at the top level for [`retain_root_entities`].
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Brep {
    /// The format-independent ASM graph.
    #[serde(flatten)]
    pub(crate) asm: AsmBrep,
    /// Typed sketch-curve provenance links.
    pub(crate) sketch_curve_links: Vec<SketchCurveLink>,
    /// Persistent design identifiers attached to solved entities.
    pub(crate) persistent_design_links: Vec<PersistentDesignLink>,
    /// Variable-width persistent tag groups attached to solved faces and edges.
    pub(crate) persistent_subentity_tags: Vec<PersistentSubentityTag>,
    /// Original authoring times attached to solved entities.
    pub(crate) creation_timestamps: Vec<CreationTimestamp>,
}

impl Brep {
    /// Wrap a decoded ASM graph and derive the Fusion attribute links.
    fn from_asm(ctx: &DecodeContext<'_>, asm: AsmBrep) -> Result<Self, CodecError> {
        let mut sketch_curve_links = Vec::new();
        let mut persistent_design_links = Vec::new();
        let mut persistent_subentity_tags = Vec::new();
        let mut creation_timestamps = Vec::new();
        for attribute in &asm.attributes {
            if let Some(link) = sketch_curve_link(ctx, attribute)? {
                push_brep_item(ctx, &mut sketch_curve_links, link, "collect F3D sketch curve links")?;
            }
            for link in self::persistent_design_links(ctx, attribute)? {
                push_brep_item(ctx, &mut persistent_design_links, link, "collect F3D persistent design links")?;
            }
            for tag in self::persistent_subentity_tags(ctx, attribute)? {
                push_brep_item(ctx, &mut persistent_subentity_tags, tag, "collect F3D persistent subentity tags")?;
            }
            if let Some(timestamp) = creation_timestamp(ctx, attribute)? {
                push_brep_item(ctx, &mut creation_timestamps, timestamp, "collect F3D creation timestamps")?;
            }
        }
        Ok(Self {
            asm,
            sketch_curve_links,
            persistent_design_links,
            persistent_subentity_tags,
            creation_timestamps,
        })
    }

    /// Map solved bodies to the selector used by this blob's Design body map.
    pub(crate) fn body_selectors(&self, ctx: &DecodeContext<'_>) -> Result<HashMap<BodyId, u64>, CodecError> {
        let ordinal_mode = self
            .asm
            .body_native_keys
            .iter()
            .all(|body| body.asm_body_key.is_none());
        let mut selectors = HashMap::new();
        for body in &self.asm.body_native_keys {
                let selector = if ordinal_mode {
                    Some(u64::from(body.body_ordinal))
                } else {
                    body.asm_body_key
                };
                let Some(selector) = selector else { continue; };
                let id = copy_body_id(ctx, &body.body)?;
                ctx.charge_collection_items(1, "index F3D BREP body selectors")?;
                selectors.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index F3D BREP body selectors", 0, 1))?;
                selectors.insert(id, selector);
        }
        Ok(selectors)
    }

    /// Resolve the Design selectors present for this blob. An exact native
    /// body key has precedence. A selector absent from the native-key domain
    /// selects the body with the same zero-based ordinal.
    pub(crate) fn body_selectors_for(
        &self,
        ctx: &DecodeContext<'_>,
        selectors: &HashSet<u64>,
    ) -> Result<HashMap<BodyId, u64>, cadmpeg_core::CodecError> {
        let mut resolved = HashMap::new();
        for selector in selectors {
            let Some(body) = resolve_body_selector(self.asm.body_native_keys.iter(), *selector)? else {
                continue;
            };
            let id = copy_body_id(ctx, body)?;
            ctx.charge_collection_items(1, "index F3D selected BREP bodies")?;
            resolved.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index F3D selected BREP bodies", 0, 1))?;
            if let Some(previous) = resolved.insert(id, *selector) {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "F3D body {} is selected by both {previous} and {selector}",
                    body.as_str()
                )));
            }
        }
        Ok(resolved)
    }

    /// Retain the connected entity graph rooted at the body-map keys selected
    /// for one BREP blob.
    pub(crate) fn retain_body_keys(
        &mut self,
        ctx: &DecodeContext<'_>,
        selected_keys: &HashSet<u64>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let annotations = std::mem::take(&mut self.asm.annotation_records);
        let sketch_curve_links = std::mem::take(&mut self.sketch_curve_links);
        let persistent_design_links = std::mem::take(&mut self.persistent_design_links);
        let persistent_subentity_tags = std::mem::take(&mut self.persistent_subentity_tags);
        let creation_timestamps = std::mem::take(&mut self.creation_timestamps);
        let mut value = serde_value::to_value(&*self).map_err(|error| {
            cadmpeg_core::CodecError::malformed(format_args!("BREP serialization failed: {error}"))
        })?;
        let mut owned = HashSet::new();
        collect_owned_ids_charged(ctx, &value, &mut owned)?;
        let mut native_body_ids = HashSet::new();
        for native in &self.asm.body_native_keys {
            if !native_body_ids.contains(native.body.as_str()) {
                ctx.charge_collection_items(1, "index F3D native BREP bodies")?;
                native_body_ids.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index F3D native BREP bodies", 0, 1))?;
                native_body_ids.insert(native.body.as_str());
            }
        }
        let mut reachable = HashSet::new();
        for body in self.body_selectors_for(ctx, selected_keys)?.into_keys() {
            insert_brep_string(ctx, &mut reachable, body.into_string(), "collect F3D selected BREP roots")?;
        }
        // A Design body map selects native ASM body records. Neutral roots
        // projected from other saved top-level entities have no ASM body key
        // and remain part of the selected BREP blob.
        for body in &self.asm.bodies {
            if !native_body_ids.contains(body.id.as_str()) {
                let id = copy_brep_text(ctx, body.id.as_str(), "copy F3D neutral BREP root")?;
                insert_brep_string(ctx, &mut reachable, id, "collect F3D neutral BREP roots")?;
            }
        }
        let adjacency = collect_brep_adjacency(ctx, &value, &owned)?;
        let mut pending = Vec::new();
        for id in &reachable {
            let id = copy_brep_text(ctx, id, "copy F3D BREP pending root")?;
            push_brep_item(ctx, &mut pending, id, "collect F3D BREP pending roots")?;
        }
        while let Some(id) = pending.pop() {
            for adjacent in adjacency.get(&id).into_iter().flatten() {
                if !reachable.contains(adjacent) {
                    let reached = copy_brep_text(ctx, adjacent, "copy F3D reachable BREP ID")?;
                    insert_brep_string(ctx, &mut reachable, reached, "collect F3D reachable BREP IDs")?;
                    let queued = copy_brep_text(ctx, adjacent, "copy F3D pending BREP ID")?;
                    push_brep_item(ctx, &mut pending, queued, "collect F3D pending BREP IDs")?;
                }
            }
        }
        retain_root_entities(&mut value, &reachable);
        let mut retained: Self = crate::value_tree::from_value(value).map_err(|error| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "retained BREP graph is invalid: {error}"
            ))
        })?;
        for annotation in annotations {
            if reachable.contains(&annotation.id) {
                push_brep_item(ctx, &mut retained.asm.annotation_records, annotation, "collect F3D retained annotations")?;
            }
        }
        for link in sketch_curve_links {
            if retained_attribute_target(&link.target, &reachable) {
                push_brep_item(ctx, &mut retained.sketch_curve_links, link, "collect F3D retained sketch links")?;
            }
        }
        for link in persistent_design_links {
            if retained_attribute_target(&link.target, &reachable) {
                push_brep_item(ctx, &mut retained.persistent_design_links, link, "collect F3D retained design links")?;
            }
        }
        for tag in persistent_subentity_tags {
            if retained_attribute_target(&tag.target, &reachable) {
                push_brep_item(ctx, &mut retained.persistent_subentity_tags, tag, "collect F3D retained subentity tags")?;
            }
        }
        for timestamp in creation_timestamps {
            if retained_attribute_target(&timestamp.target, &reachable) {
                push_brep_item(ctx, &mut retained.creation_timestamps, timestamp, "collect F3D retained timestamps")?;
            }
        }
        *self = retained;
        Ok(())
    }

    /// Qualify every entity owned by this graph so several BREP blobs can
    /// coexist in one document model without record-index collisions.
    pub(crate) fn qualify_ids(
        &mut self,
        ctx: &DecodeContext<'_>,
        format: IdFormat,
        namespace: &str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let annotations = std::mem::take(&mut self.asm.annotation_records);
        let mut value = serde_value::to_value(&*self).map_err(|error| {
            cadmpeg_core::CodecError::malformed(format_args!("BREP serialization failed: {error}"))
        })?;
        let mut owned = HashSet::new();
        collect_owned_ids_charged(ctx, &value, &mut owned)?;
        let scheme_prefix = format_brep_text(ctx, format_args!("{format}:"), "retain F3D BREP scheme prefix")?;
        let mut replacements = HashMap::new();
        for id in owned {
            let replacement = format_brep_text(ctx, format_args!("{format}:brep/{namespace}/{}", id.strip_prefix(&scheme_prefix).unwrap_or(&id)), "retain F3D qualified BREP ID")?;
            ctx.charge_collection_items(1, "index F3D BREP replacements")?;
            replacements.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index F3D BREP replacements", 0, 1))?;
            replacements.insert(id, replacement);
        }
        remap_owned_ids_charged(ctx, &mut value, &replacements)?;
        let mut qualified: Self = crate::value_tree::from_value(value).map_err(|error| {
            cadmpeg_core::CodecError::malformed(format_args!("qualified BREP is invalid: {error}"))
        })?;
        let mut qualified_annotations = Vec::new();
        for mut annotation in annotations {
            if let Some(id) = replacements.get(&annotation.id) {
                annotation.id = copy_brep_text(ctx, id, "copy F3D qualified annotation ID")?;
            }
            push_brep_item(ctx, &mut qualified_annotations, annotation, "collect F3D qualified annotations")?;
        }
        qualified.asm.annotation_records = qualified_annotations;
        *self = qualified;
        Ok(())
    }

    /// Append a disjoint, already-qualified BREP graph.
    pub(crate) fn append(&mut self, ctx: &DecodeContext<'_>, other: Self) -> Result<(), CodecError> {
        let mut asm = other.asm;
        macro_rules! append_vecs {
            ($($field:ident),+ $(,)?) => {
                $(append_brep_items(ctx, &mut self.asm.$field, &mut asm.$field,
                    concat!("merge F3D BREP ", stringify!($field)))?;)+
            };
        }
        append_vecs!(
            bodies, regions, shells, faces, loops, coedges, edges, vertices, points,
            surfaces, curves, pcurves, procedural_surfaces, procedural_curves,
            edge_continuities, edge_ownerships, vertex_ownerships, face_sidedness,
            face_native_keys, tolerant_coedge_parameters, tolerant_edge_tails,
            tolerant_vertex_tails, mesh_surface_sentinels, transform_hints,
            body_native_keys, wire_topologies, attributes, unknowns, annotation_records,
        );
        self.asm.stats.mesh_surface_faces += asm.stats.mesh_surface_faces;
        self.asm.stats.nurbs_surfaces += asm.stats.nurbs_surfaces;
        self.asm.stats.nurbs_curves += asm.stats.nurbs_curves;
        self.asm.stats.partial_procedural_supports += asm.stats.partial_procedural_supports;
        macro_rules! merge_count_maps {
            ($($field:ident),+ $(,)?) => {
                $(merge_brep_counts(ctx, &mut self.asm.stats.$field, std::mem::take(&mut asm.stats.$field))?;)+
            };
        }
        merge_count_maps!(
            missing_face_surface_kinds, unknown_surface_kinds,
            procedural_curve_kinds, undecoded_pcurve_kinds, other_record_kinds,
        );
        let mut other = Self { asm, ..other };
        macro_rules! append_derived {
            ($($field:ident),+ $(,)?) => {
                $(append_brep_items(ctx, &mut self.$field, &mut other.$field,
                    concat!("merge F3D BREP ", stringify!($field)))?;)+
            };
        }
        append_derived!(
            sketch_curve_links,
            persistent_design_links,
            persistent_subentity_tags,
            creation_timestamps,
        );
        Ok(())
    }
}

/// Decode a framed active slice into the IR B-rep graph.
///
/// `stream` names the source ZIP entry for provenance. Ids are minted as
/// `<format>:brep:entity#<record-index>`, unique across the `RecordTable`.
pub(crate) fn decode(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[Record],
    bytes: &[u8],
    stream: &str,
    format: IdFormat,
) -> Result<Brep, cadmpeg_core::CodecError> {
    Brep::from_asm(ctx, decode_with_purpose(
        ctx,
        records,
        bytes,
        stream,
        format,
        DecodePurpose::Model,
    )?)
}

/// Decode a parsed text stream ([`cadmpeg_asm::sat`]) into the IR B-rep graph.
///
/// The text parser already typed the records and converted lengths into the
/// binary centimetre convention, so the shared decode path runs unchanged.
/// The header comes from the stream's ASCII header lines rather than a binary
/// header parse of `bytes`.
pub(crate) fn decode_text(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    stream: &cadmpeg_asm::sat::TextStream,
    bytes: &[u8],
    entry: &str,
    format: IdFormat,
) -> Result<Brep, cadmpeg_core::CodecError> {
    Brep::from_asm(ctx, decode_with_header(
        ctx,
        &stream.records,
        bytes,
        Some(stream.header.as_kernel_header()),
        entry,
        format,
        DecodePurpose::Model,
    )?)
}

/// Decode only the topology and analytic measurements used to bind ASM
/// history. Free-form carrier shapes are not materialized because historical
/// binding consumes their stable record identities, not their control data.
pub(crate) fn decode_history_topology(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[Record],
    bytes: &[u8],
    format: IdFormat,
) -> Result<Brep, cadmpeg_core::CodecError> {
    Brep::from_asm(ctx, decode_with_purpose(
        ctx,
        records,
        bytes,
        "history",
        format,
        DecodePurpose::History,
    )?)
}

/// Resolve one Design body selector within one BREP blob. Exact native keys
/// take precedence; an absent key falls back to the zero-based body ordinal.
pub(crate) fn resolve_body_selector<'a>(
    body_keys: impl Iterator<Item = &'a BodyNativeKey> + Clone,
    selector: u64,
) -> Result<Option<&'a BodyId>, cadmpeg_core::CodecError> {
    let mut direct = body_keys.clone().filter(|body| body.asm_body_key == Some(selector));
    if let Some(body) = direct.next() {
        if direct.next().is_some() {
            return Err(cadmpeg_core::CodecError::malformed(format_args!("F3D body selector {selector} matches multiple native body keys")));
        }
        return Ok(Some(&body.body));
    }
    let Some(ordinal) = u32::try_from(selector).ok() else {
        return Ok(None);
    };
    let mut matches = body_keys.filter(|body| body.body_ordinal == ordinal);
    match matches.next() {
        Some(body) if matches.next().is_none() => Ok(Some(&body.body)),
        None => Ok(None),
        _ => Err(cadmpeg_core::CodecError::malformed(format_args!("F3D body selector {selector} matches multiple body ordinals"))),
    }
}

/// The five members every `sketch_attrib_def` payload form writes.
struct SketchLinkPayload {
    sketch_curve_id: i64,
    ref_b: u64,
    sense: i64,
    role: i64,
    closure: i64,
}

/// Read the payload following a `sketch_attrib_def` family name.
///
/// The three header integers are `1`, `1`, and a form selector. Form `3` writes
/// the members as one tagged ASCII field with a `0` between the sense and the
/// role, form `2` as six integers with a trailing `0`, and form `0` as the five
/// members alone. All three write the same five members in the same order, so
/// each yields one link.
fn sketch_link_payload(values: &[AttributeValue]) -> Option<SketchLinkPayload> {
    let [AttributeValue::Integer(1), AttributeValue::Integer(1), AttributeValue::Integer(form), payload @ ..] =
        values
    else {
        return None;
    };
    match (*form, payload) {
        (3, [AttributeValue::String(field)]) => {
            let mut fields = field.split_ascii_whitespace();
            let (Some(sketch_curve_id), Some(ref_b), Some(sense), Some("0"), Some(role), Some(closure), None) =
                (fields.next(), fields.next(), fields.next(), fields.next(), fields.next(), fields.next(), fields.next()) else {
                return None;
            };
            // `ref_b` reaches the full unsigned 64-bit range, so it is read
            // unsigned; every other member is signed.
            Some(SketchLinkPayload {
                sketch_curve_id: sketch_curve_id.parse().ok()?,
                ref_b: ref_b.parse().ok()?,
                sense: sense.parse().ok()?,
                role: role.parse().ok()?,
                closure: closure.parse().ok()?,
            })
        }
        (
            2,
            [AttributeValue::Integer(sketch_curve_id), AttributeValue::Integer(ref_b), AttributeValue::Integer(sense), AttributeValue::Integer(role), AttributeValue::Integer(closure), AttributeValue::Integer(0)],
        )
        | (
            0,
            [AttributeValue::Integer(sketch_curve_id), AttributeValue::Integer(ref_b), AttributeValue::Integer(sense), AttributeValue::Integer(role), AttributeValue::Integer(closure)],
        ) => Some(SketchLinkPayload {
            sketch_curve_id: *sketch_curve_id,
            ref_b: u64::try_from(*ref_b).ok()?,
            sense: *sense,
            role: *role,
            closure: *closure,
        }),
        _ => None,
    }
}

fn sketch_curve_link(ctx: &DecodeContext<'_>, attribute: &SourceAttribute) -> Result<Option<SketchCurveLink>, CodecError> {
    let Some(family) = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "sketch_attrib_def"),
    ) else {
        return Ok(None);
    };
    let Some(payload) = sketch_link_payload(&attribute.values[family + 1..]) else {
        return Ok(None);
    };
    Ok(Some(SketchCurveLink {
        id: format_brep_text(ctx, format_args!("f3d:design:sketch-curve-link#{}", attribute_key(attribute)), "retain F3D sketch curve link ID")?,
        target: copy_attribute_target(ctx, &attribute.target)?,
        sketch_curve_id: payload.sketch_curve_id,
        ref_b: payload.ref_b,
        sense: crate::records::sketch_links::SketchLinkSense::try_from(payload.sense).ok(),
        role: payload.role,
        closure: payload.closure,
    }))
}

fn persistent_design_links(ctx: &DecodeContext<'_>, attribute: &SourceAttribute) -> Result<Vec<PersistentDesignLink>, CodecError> {
    let AttributeTarget::Body(_) = &attribute.target else {
        return Ok(Vec::new());
    };
    let Some((version, group_count, rest)) = generic_tag_payload(attribute) else {
        return Ok(Vec::new());
    };
    let group_width = match version {
        GenericTagVersion::V2 => 4,
        GenericTagVersion::V3 => 5,
    };
    if group_count.checked_mul(group_width) != Some(rest.len()) {
        return Ok(Vec::new());
    }
    let mut links = Vec::new();
    for values in rest.chunks_exact(group_width) {
        let (entity_kind, design_id, design_reference) = match values {
            [AttributeValue::Integer(entity_kind), AttributeValue::String(design_id), AttributeValue::Integer(design_reference), AttributeValue::Integer(0)]
            | [AttributeValue::Integer(entity_kind), AttributeValue::String(design_id), AttributeValue::Integer(design_reference), AttributeValue::Integer(0), AttributeValue::Integer(0)] => {
                (*entity_kind, design_id, *design_reference)
            }
            _ => return Ok(Vec::new()),
        };
        let design_id_text = copy_brep_text(ctx, design_id, "copy F3D persistent design ID")?;
        let Ok(design_id) = crate::records::sketch_links::DesignPersistentIdText::try_from(design_id_text) else {
            return Ok(Vec::new());
        };
        if entity_kind != 3 {
            continue;
        }
        let ordinal = u32::try_from(links.len())
            .map_err(|_| CodecError::Malformed("F3D persistent design link ordinal overflows".into()))?;
        let link = PersistentDesignLink {
            id: format_brep_text(ctx, format_args!("f3d:design:persistent-design-link#{}:{ordinal}", attribute_key(attribute)), "retain F3D persistent design link ID")?,
            target: copy_attribute_target(ctx, &attribute.target)?,
            design_id,
            design_reference,
            ordinal,
        };
        push_brep_item(ctx, &mut links, link, "collect F3D attribute design links")?;
    }
    Ok(links)
}

fn persistent_subentity_tags(
    ctx: &DecodeContext<'_>,
    attribute: &SourceAttribute,
) -> Result<Vec<PersistentSubentityTag>, CodecError> {
    if !matches!(
        attribute.target,
        AttributeTarget::Face(_) | AttributeTarget::Edge(_)
    ) {
        return Ok(Vec::new());
    }
    let Some((version, group_count, rest)) = generic_tag_payload(attribute) else {
        return Ok(Vec::new());
    };
    // Each group consumes at least four leading attribute values from `rest`.
    let Ok(group_count) = u64::try_from(group_count) else {
        return Ok(Vec::new());
    };
    let Some(group_count) = bounded_len(group_count, 4, rest.len()) else {
        return Ok(Vec::new());
    };
    let mut position: usize = 0;
    let mut groups = Vec::new();
    for ordinal in 0..group_count {
        let Some(
            [AttributeValue::Integer(selector), AttributeValue::String(token), AttributeValue::Integer(0), AttributeValue::Integer(reference_count)],
        ) = rest.get(position..position + 4)
        else {
            return Ok(Vec::new());
        };
        let Some(token) = cadmpeg_core::text::NonBlankString::new(copy_brep_text(
            ctx,
            token,
            "copy F3D persistent subentity token",
        )?) else {
            return Ok(Vec::new());
        };
        if *reference_count < 0 {
            return Ok(Vec::new());
        }
        let Ok(reference_count) = usize::try_from(*reference_count) else {
            return Ok(Vec::new());
        };
        let reference_start = position + 4;
        let Some(reference_end) = reference_start.checked_add(reference_count) else {
            return Ok(Vec::new());
        };
        let Some(reference_values) = rest.get(reference_start..reference_end) else {
            return Ok(Vec::new());
        };
        let mut design_references = Vec::new();
        for value in reference_values {
            let AttributeValue::Integer(value) = value else {
                return Ok(Vec::new());
            };
            push_brep_item(ctx, &mut design_references, *value, "collect F3D subentity references")?;
        }
        if matches!(version, GenericTagVersion::V3) {
            if !matches!(rest.get(reference_end), Some(AttributeValue::Integer(0))) {
                return Ok(Vec::new());
            }
            position = reference_end + 1;
        } else {
            position = reference_end;
        }
        let ordinal = u32::try_from(ordinal)
            .map_err(|_| CodecError::Malformed("F3D persistent subentity ordinal overflows".into()))?;
        let group = PersistentSubentityTag {
            id: format_brep_text(ctx, format_args!("f3d:design:persistent-subentity-tag#{}:{ordinal}", attribute_key(attribute)), "retain F3D persistent subentity tag ID")?,
            target: copy_attribute_target(ctx, &attribute.target)?,
            selector: *selector,
            token,
            design_references,
            ordinal,
        };
        push_brep_item(ctx, &mut groups, group, "collect F3D subentity tag groups")?;
    }
    if position != rest.len() {
        return Ok(Vec::new());
    }
    Ok(groups)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GenericTagVersion {
    V2,
    V3,
}

/// Return the common generic-tag version, group count, and payload.
///
/// The two leading integers are an equal envelope version. Versions two and
/// three select distinct, bounded group envelopes; a mixed or unsupported
/// pair is not a generic-tag envelope.
fn generic_tag_payload(
    attribute: &SourceAttribute,
) -> Option<(GenericTagVersion, usize, &[AttributeValue])> {
    let family = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "generic_tag_attrib_def"),
    )?;
    let values = attribute.values.get(family + 1..)?;
    let [AttributeValue::Integer(left_version), AttributeValue::Integer(right_version), AttributeValue::Integer(-1), AttributeValue::String(marker), AttributeValue::Integer(group_count), rest @ ..] =
        values
    else {
        return None;
    };
    if left_version != right_version || marker != "generic_tag_attrib_def " || *group_count < 0 {
        return None;
    }
    let version = match *left_version {
        2 => GenericTagVersion::V2,
        3 => GenericTagVersion::V3,
        _ => return None,
    };
    Some((version, usize::try_from(*group_count).ok()?, rest))
}

fn retained_attribute_target(target: &AttributeTarget, reachable: &HashSet<String>) -> bool {
    match target {
        AttributeTarget::Document => true,
        AttributeTarget::Body(id) => reachable.contains(id.as_str()),
        AttributeTarget::Face(id) => reachable.contains(id.as_str()),
        AttributeTarget::Shell(id) => reachable.contains(id.as_str()),
        AttributeTarget::Loop(id) => reachable.contains(id.as_str()),
        AttributeTarget::Coedge(id) => reachable.contains(id.as_str()),
        AttributeTarget::Edge(id) => reachable.contains(id.as_str()),
        AttributeTarget::Vertex(id) => reachable.contains(id.as_str()),
    }
}

fn creation_timestamp(ctx: &DecodeContext<'_>, attribute: &SourceAttribute) -> Result<Option<CreationTimestamp>, CodecError> {
    let Some(family) = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "Timestamp_attrib_def"),
    ) else {
        return Ok(None);
    };
    let Some(marker) = attribute.values.get(family + 1) else {
        return Ok(None);
    };
    if !matches!(marker, AttributeValue::Integer(1)) {
        return Ok(None);
    }
    let Some(AttributeValue::Float(unix_microseconds)) = attribute.values.get(family + 2) else {
        return Ok(None);
    };
    let Some(record_index) = attribute_key(attribute).parse().ok() else {
        return Ok(None);
    };
    Ok(Some(CreationTimestamp {
        id: format_brep_text(ctx, format_args!("f3d:design:creation-timestamp#{}", attribute_key(attribute)), "retain F3D creation timestamp ID")?,
        target: copy_attribute_target(ctx, &attribute.target)?,
        record_index,
        unix_microseconds: *unix_microseconds,
    }))
}

#[cfg(test)]
mod tests {
    use super::{collect_brep_references, insert_brep_adjacency, insert_brep_string, persistent_design_links, persistent_subentity_tags, Brep};
    use crate::records::recipes::CreationTimestamp;
    use crate::records::sketch_links::{
        PersistentDesignLink, PersistentSubentityTag, SketchCurveLink,
    };
    use cadmpeg_asm::brep::annotations::AnnotationRecord;
    use cadmpeg_asm::brep::records::BodyNativeKey;
    use cadmpeg_asm::brep::AsmBrep;
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::ids::{FaceId, RegionId};
    use cadmpeg_ir::topology::{Body, BodyKind, Region};
    use std::collections::{HashMap, HashSet};

    fn with_limits<T>(
        max_items: u64,
        max_retained: u64,
        run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
    ) -> T {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        run(&ctx)
    }

    fn with_context<T>(run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
        with_limits(u64::MAX, u64::MAX, run)
    }

    #[test]
    fn persistent_subentity_token_refuses_retained_limit() {
        let attribute = generic_tag_attribute(
            AttributeTarget::Face(FaceId::mint("f3d:test:face#1").unwrap()),
            (2, 2),
            1,
            vec![AttributeValue::Integer(7), AttributeValue::String("97".into()),
                AttributeValue::Integer(0), AttributeValue::Integer(1), AttributeValue::Integer(302)],
        );
        let error = with_limits(u64::MAX, 1, |ctx| persistent_subentity_tags(ctx, &attribute).unwrap_err());
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "copy F3D persistent subentity token"));
    }

    #[test]
    fn persistent_subentity_references_refuse_collection_limit() {
        let attribute = generic_tag_attribute(
            AttributeTarget::Face(FaceId::mint("f3d:test:face#1").unwrap()),
            (2, 2),
            1,
            vec![AttributeValue::Integer(7), AttributeValue::String("97".into()),
                AttributeValue::Integer(0), AttributeValue::Integer(1), AttributeValue::Integer(302)],
        );
        let error = with_limits(0, u64::MAX, |ctx| persistent_subentity_tags(ctx, &attribute).unwrap_err());
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect F3D subentity references"));
    }

    #[test]
    fn persistent_design_id_refuses_retained_limit() {
        let attribute = generic_tag_attribute(
            AttributeTarget::Body(BodyId::mint("f3d:test:body#1").unwrap()),
            (2, 2),
            1,
            vec![AttributeValue::Integer(3), AttributeValue::String("301".into()),
                AttributeValue::Integer(1), AttributeValue::Integer(0)],
        );
        let error = with_limits(u64::MAX, 2, |ctx| persistent_design_links(ctx, &attribute).unwrap_err());
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "copy F3D persistent design ID"));
    }

    fn generic_tag_attribute(
        target: AttributeTarget,
        versions: (i64, i64),
        group_count: i64,
        groups: Vec<AttributeValue>,
    ) -> SourceAttribute {
        SourceAttribute {
            id: "f3d:brep:attribute#1".try_into().expect("valid identity"),
            target,
            name: "ATTRIB_CUSTOM-attrib".into(),
            values: [
                AttributeValue::String("generic_tag_attrib_def".into()),
                AttributeValue::Integer(versions.0),
                AttributeValue::Integer(versions.1),
                AttributeValue::Integer(-1),
                AttributeValue::String("generic_tag_attrib_def ".into()),
                AttributeValue::Integer(group_count),
            ]
            .into_iter()
            .chain(groups)
            .collect(),
        }
    }

    #[test]
    fn generic_tag_payload_accepts_both_equal_envelope_versions() {
        for (version, groups) in [
            (
                2,
                vec![
                    AttributeValue::Integer(7),
                    AttributeValue::String("97".into()),
                    AttributeValue::Integer(0),
                    AttributeValue::Integer(1),
                    AttributeValue::Integer(302),
                ],
            ),
            (
                3,
                vec![
                    AttributeValue::Integer(7),
                    AttributeValue::String("97".into()),
                    AttributeValue::Integer(0),
                    AttributeValue::Integer(1),
                    AttributeValue::Integer(302),
                    AttributeValue::Integer(0),
                ],
            ),
        ] {
            let attribute = generic_tag_attribute(
                AttributeTarget::Face(FaceId::mint("f3d:test:face#1").expect("identity grammar")),
                (version, version),
                1,
                groups,
            );
            assert_eq!(with_context(|ctx| persistent_subentity_tags(ctx, &attribute).unwrap()).len(), 1);
            assert_eq!(
                with_context(|ctx| persistent_subentity_tags(ctx, &attribute).unwrap())[0].design_references,
                [302]
            );
        }
    }

    #[test]
    fn generic_tag_payload_rejects_mixed_or_unsupported_envelope_versions() {
        let groups = vec![
            AttributeValue::Integer(7),
            AttributeValue::String("97".into()),
            AttributeValue::Integer(0),
            AttributeValue::Integer(0),
            AttributeValue::Integer(0),
        ];
        for versions in [(2, 3), (1, 1), (4, 4)] {
            let attribute = generic_tag_attribute(
                AttributeTarget::Face(FaceId::mint("f3d:test:face#1").expect("identity grammar")),
                versions,
                1,
                groups.clone(),
            );
            assert!(with_context(|ctx| persistent_subentity_tags(ctx, &attribute).unwrap()).is_empty());
        }
    }

    #[test]
    fn generic_tag_payload_binds_modern_body_design_links() {
        let attribute = generic_tag_attribute(
            AttributeTarget::Body(BodyId::mint("f3d:test:body#1").expect("identity grammar")),
            (2, 2),
            1,
            vec![
                AttributeValue::Integer(3),
                AttributeValue::String("301".into()),
                AttributeValue::Integer(1),
                AttributeValue::Integer(0),
            ],
        );
        let links = with_context(|ctx| persistent_design_links(ctx, &attribute).unwrap());
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].design_id.as_str(), "301");
        assert_eq!(links[0].design_reference, 1);
    }

    #[test]
    fn generic_tag_payload_binds_legacy_body_design_links() {
        let attribute = generic_tag_attribute(
            AttributeTarget::Body(BodyId::mint("f3d:test:body#1").expect("identity grammar")),
            (3, 3),
            1,
            vec![
                AttributeValue::Integer(3),
                AttributeValue::String("301".into()),
                AttributeValue::Integer(1),
                AttributeValue::Integer(0),
                AttributeValue::Integer(0),
            ],
        );
        assert_eq!(with_context(|ctx| persistent_design_links(ctx, &attribute).unwrap()).len(), 1);
    }

    #[test]
    fn brep_qualification_rewrites_owned_ids_and_cross_references() {
        let body = BodyId::mint("f3d:brep:entity#1").expect("identity grammar");
        let region = RegionId::mint("f3d:brep:entity#2").expect("identity grammar");
        let mut brep = Brep {
            asm: AsmBrep {
                bodies: vec![Body {
                    id: body.clone(),
                    kind: BodyKind::default(),
                    regions: vec![region.clone()],
                    transform: None,
                    name: None,
                    color: None,
                    visible: None,
                }],
                regions: vec![Region {
                    id: region,
                    body: body.clone(),
                    shells: Vec::new(),
                }],

                body_native_keys: vec![BodyNativeKey {
                    source_namespace:
                        cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                            crate::ids::ID_FORMAT,
                        ),
                    body,
                    record_index: 1,
                    body_ordinal: 0,
                    source_brep: Some("BREP.source.smbh".into()),
                    asm_body_key: Some(7),
                }],
                annotation_records: vec![AnnotationRecord {
                    id: "f3d:brep:entity#1".into(),
                    stream: "asset/BREP.source.smbh".into(),
                    offset: 10,
                    tag: cadmpeg_asm::brep::annotations::AnnotationTag::Record("body".into()),
                    derived_fields: Vec::new(),
                }],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };

        with_context(|ctx| brep.qualify_ids(ctx, crate::ids::ID_FORMAT, "source"))
            .expect("qualify BREP");

        let qualified = BodyId::mint("f3d:brep/source/brep:entity#1").expect("identity grammar");
        assert_eq!(brep.asm.bodies[0].id, qualified);
        assert_eq!(brep.asm.regions[0].body, qualified);
        assert_eq!(brep.asm.body_native_keys[0].body, qualified);
        assert_eq!(
            brep.asm
                .body_native_keys
                .iter()
                .find(|record| record.body == qualified)
                .and_then(|record| record.asm_body_key.as_ref()),
            Some(&7)
        );
        assert_eq!(brep.asm.annotation_records[0].id, qualified.as_str());
        assert_eq!(
            brep.asm.body_native_keys[0].source_brep.as_deref(),
            Some("BREP.source.smbh")
        );
    }

    fn one_body_brep() -> Brep {
        Brep {
            asm: AsmBrep {
                bodies: vec![Body {
                    id: BodyId::mint("f3d:brep:entity#1").unwrap(),
                    kind: BodyKind::default(),
                    regions: Vec::new(),
                    transform: None,
                    name: None,
                    color: None,
                    visible: None,
                }],
                ..AsmBrep::default()
            },
            ..Brep::default()
        }
    }

    #[test]
    fn brep_owned_id_copy_refuses_retained_limit() {
        let mut brep = one_body_brep();
        let error = with_limits(u64::MAX, 0, |ctx| {
            brep.qualify_ids(ctx, crate::ids::ID_FORMAT, "source").unwrap_err()
        });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "copy F3D BREP owned ID"));
    }

    #[test]
    fn brep_owned_id_index_refuses_collection_limit() {
        let mut brep = one_body_brep();
        let error = with_limits(0, u64::MAX, |ctx| {
            brep.qualify_ids(ctx, crate::ids::ID_FORMAT, "source").unwrap_err()
        });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D BREP owned IDs"));
    }

    #[test]
    fn brep_replacement_index_refuses_collection_limit() {
        let mut brep = one_body_brep();
        let error = with_limits(1, u64::MAX, |ctx| {
            brep.qualify_ids(ctx, crate::ids::ID_FORMAT, "source").unwrap_err()
        });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D BREP replacements"));
    }

    #[test]
    fn brep_remapped_id_refuses_retained_limit() {
        let mut brep = one_body_brep();
        let original = "f3d:brep:entity#1";
        let replacement = format!("f3d:brep/source/{}", original.strip_prefix("f3d:").unwrap());
        let before_remap = original.len() + "f3d:".len() + replacement.len();
        let error = with_limits(u64::MAX, before_remap as u64, |ctx| {
            brep.qualify_ids(ctx, crate::ids::ID_FORMAT, "source").unwrap_err()
        });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "copy F3D BREP remapped ID"));
    }

    #[test]
    fn brep_value_map_rebuild_refuses_collection_limit() {
        let mut brep = one_body_brep();
        let error = with_limits(2, u64::MAX, |ctx| {
            brep.qualify_ids(ctx, crate::ids::ID_FORMAT, "source").unwrap_err()
        });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "rebuild F3D BREP value map"));
    }

    #[test]
    fn brep_adjacency_reference_refuses_retained_limit() {
        let owned = HashSet::from(["f3d:brep:entity#1".to_owned()]);
        let value = serde_value::Value::String("f3d:brep:entity#1".to_owned());
        let error = with_limits(u64::MAX, 0, |ctx| {
            collect_brep_references(ctx, &value, &owned, &mut HashSet::new()).unwrap_err()
        });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "copy F3D BREP adjacency reference"));
    }

    #[test]
    fn brep_adjacency_index_refuses_collection_limit() {
        let error = with_limits(0, u64::MAX, |ctx| {
            insert_brep_adjacency(ctx, &mut HashMap::new(), "source", "target").unwrap_err()
        });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D BREP adjacency"));
    }

    #[test]
    fn brep_adjacent_ids_refuse_collection_limit() {
        let mut adjacency = HashMap::from([("source".to_owned(), HashSet::new())]);
        let error = with_limits(0, u64::MAX, |ctx| {
            insert_brep_adjacency(ctx, &mut adjacency, "source", "target").unwrap_err()
        });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect F3D BREP adjacent IDs"));
    }

    #[test]
    fn brep_reachable_id_set_refuses_collection_limit() {
        let error = with_limits(0, u64::MAX, |ctx| {
            insert_brep_string(ctx, &mut HashSet::new(), "f3d:brep:entity#1".to_owned(), "collect F3D reachable BREP IDs").unwrap_err()
        });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect F3D reachable BREP IDs"));
    }

    #[test]
    fn brep_retained_sketch_links_refuse_collection_limit() {
        let mut brep = Brep {
            sketch_curve_links: vec![SketchCurveLink {
                id: "f3d:design:sketch-curve-link#1".into(),
                target: AttributeTarget::Document,
                sketch_curve_id: 1,
                ref_b: 0,
                sense: None,
                role: 0,
                closure: 0,
            }],
            ..Brep::default()
        };
        let error = with_limits(0, u64::MAX, |ctx| brep.retain_body_keys(ctx, &HashSet::new()).unwrap_err());
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect F3D retained sketch links"));
    }

    #[test]
    fn brep_retained_design_links_refuse_collection_limit() {
        let mut brep = Brep {
            persistent_design_links: vec![PersistentDesignLink {
                id: "f3d:design:persistent-design-link#1".into(),
                target: AttributeTarget::Document,
                design_id: "301".to_owned().try_into().unwrap(),
                design_reference: 1,
                ordinal: 0,
            }],
            ..Brep::default()
        };
        let error = with_limits(0, u64::MAX, |ctx| brep.retain_body_keys(ctx, &HashSet::new()).unwrap_err());
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect F3D retained design links"));
    }

    #[test]
    fn brep_retained_subentity_tags_refuse_collection_limit() {
        let mut brep = Brep {
            persistent_subentity_tags: vec![PersistentSubentityTag {
                id: "f3d:design:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Document,
                selector: 1,
                token: cadmpeg_core::text::NonBlankString::new("97").unwrap(),
                design_references: vec![1],
                ordinal: 0,
            }],
            ..Brep::default()
        };
        let error = with_limits(0, u64::MAX, |ctx| brep.retain_body_keys(ctx, &HashSet::new()).unwrap_err());
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect F3D retained subentity tags"));
    }

    #[test]
    fn brep_retained_timestamps_refuse_collection_limit() {
        let mut brep = Brep {
            creation_timestamps: vec![CreationTimestamp {
                id: "f3d:design:creation-timestamp#1".into(),
                target: AttributeTarget::Document,
                record_index: 1,
                unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
            }],
            ..Brep::default()
        };
        let error = with_limits(0, u64::MAX, |ctx| brep.retain_body_keys(ctx, &HashSet::new()).unwrap_err());
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect F3D retained timestamps"));
    }

    #[test]
    fn body_key_retention_keeps_only_the_selected_connected_graph() {
        let body = |index, region| Body {
            id: BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
            kind: BodyKind::default(),
            regions: vec![
                RegionId::mint(format!("f3d:brep:entity#{region}")).expect("identity grammar")
            ],
            transform: None,
            name: None,
            color: None,
            visible: None,
        };
        let native_key = |index, key| BodyNativeKey {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                crate::ids::ID_FORMAT,
            ),
            body: BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
            record_index: index,
            body_ordinal: index - 1,
            source_brep: Some("BREP.source.smbh".into()),
            asm_body_key: Some(key),
        };
        let mut brep = Brep {
            asm: AsmBrep {
                bodies: vec![body(1, 2), body(3, 4)],
                regions: vec![
                    Region {
                        id: RegionId::mint("f3d:brep:entity#2").expect("identity grammar"),
                        body: BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
                        shells: Vec::new(),
                    },
                    Region {
                        id: RegionId::mint("f3d:brep:entity#4").expect("identity grammar"),
                        body: BodyId::mint("f3d:brep:entity#3").expect("identity grammar"),
                        shells: Vec::new(),
                    },
                ],

                body_native_keys: vec![native_key(1, 10), native_key(3, 20)],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };

        with_context(|ctx| brep.retain_body_keys(ctx, &HashSet::from([20])))
            .expect("retain body graph");

        assert_eq!(brep.asm.bodies.len(), 1);
        assert_eq!(brep.asm.bodies[0].id.as_str(), "f3d:brep:entity#3");
        assert_eq!(brep.asm.regions.len(), 1);
        assert_eq!(brep.asm.regions[0].id.as_str(), "f3d:brep:entity#4");
        assert_eq!(brep.asm.body_native_keys.len(), 1);
        assert_eq!(
            brep.asm
                .body_native_keys
                .iter()
                .filter(|record| record.asm_body_key.is_some())
                .count(),
            1
        );
    }

    #[test]
    fn body_key_retention_preserves_derived_links_for_reachable_targets() {
        let body = |index| Body {
            id: BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
            kind: BodyKind::default(),
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        };
        let native_key = |index, key| BodyNativeKey {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                crate::ids::ID_FORMAT,
            ),
            body: BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
            record_index: index,
            body_ordinal: index - 1,
            source_brep: Some("BREP.source.smbh".into()),
            asm_body_key: Some(key),
        };
        let target = |index| {
            AttributeTarget::Body(
                BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
            )
        };
        let mut brep = Brep {
            asm: AsmBrep {
                bodies: vec![body(1), body(3)],

                body_native_keys: vec![native_key(1, 10), native_key(3, 20)],
                ..AsmBrep::default()
            },
            sketch_curve_links: vec![
                SketchCurveLink {
                    id: "link-retained".into(),
                    target: target(1),
                    sketch_curve_id: 1,
                    ref_b: 0,
                    sense: None,
                    role: 0,
                    closure: 0,
                },
                SketchCurveLink {
                    id: "link-dropped".into(),
                    target: target(3),
                    sketch_curve_id: 3,
                    ref_b: 0,
                    sense: None,
                    role: 0,
                    closure: 0,
                },
            ],
            persistent_design_links: vec![
                PersistentDesignLink {
                    id: "design-retained".into(),
                    target: target(1),
                    design_id: "301".to_owned().try_into().unwrap(),

                    design_reference: 1,
                    ordinal: 0,
                },
                PersistentDesignLink {
                    id: "design-dropped".into(),
                    target: target(3),
                    design_id: "303".to_owned().try_into().unwrap(),

                    design_reference: 3,
                    ordinal: 0,
                },
            ],
            persistent_subentity_tags: vec![
                PersistentSubentityTag {
                    id: "tag-retained".into(),
                    target: target(1),
                    selector: 1,
                    token: cadmpeg_core::text::NonBlankString::new("97").unwrap(),
                    design_references: vec![1],
                    ordinal: 0,
                },
                PersistentSubentityTag {
                    id: "tag-dropped".into(),
                    target: target(3),
                    selector: 1,
                    token: cadmpeg_core::text::NonBlankString::new("97").unwrap(),
                    design_references: vec![3],
                    ordinal: 0,
                },
            ],
            creation_timestamps: vec![
                CreationTimestamp {
                    id: "time-retained".into(),
                    target: target(1),
                    record_index: 1,
                    unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
                },
                CreationTimestamp {
                    id: "time-dropped".into(),
                    target: target(3),
                    record_index: 3,
                    unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(3.0).unwrap(),
                },
            ],
        };

        with_context(|ctx| brep.retain_body_keys(ctx, &HashSet::from([10])))
            .expect("retain body graph");

        assert_eq!(brep.sketch_curve_links.len(), 1);
        assert_eq!(brep.persistent_design_links.len(), 1);
        assert_eq!(brep.persistent_subentity_tags.len(), 1);
        assert_eq!(brep.creation_timestamps.len(), 1);
        assert_eq!(brep.persistent_subentity_tags[0].id, "tag-retained");
    }

    #[test]
    fn body_key_retention_preserves_selectorless_neutral_roots() {
        let native_body = BodyId::mint("f3d:brep:entity#1").expect("identity grammar");
        let projected_body = BodyId::mint("f3d:brep:saved-edge-body#5").expect("identity grammar");
        let mut brep = Brep {
            asm: AsmBrep {
                bodies: vec![
                    Body {
                        id: native_body.clone(),
                        kind: BodyKind::Solid,
                        regions: Vec::new(),
                        transform: None,
                        name: None,
                        color: None,
                        visible: None,
                    },
                    Body {
                        id: projected_body.clone(),
                        kind: BodyKind::Wire,
                        regions: Vec::new(),
                        transform: None,
                        name: None,
                        color: None,
                        visible: None,
                    },
                ],

                body_native_keys: vec![BodyNativeKey {
                    source_namespace:
                        cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                            crate::ids::ID_FORMAT,
                        ),
                    body: native_body,
                    record_index: 1,
                    body_ordinal: 0,
                    source_brep: Some("BREP.source.smbh".into()),
                    asm_body_key: Some(10),
                }],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };

        with_context(|ctx| brep.retain_body_keys(ctx, &HashSet::from([10])))
            .expect("retain body graph");

        assert_eq!(brep.asm.bodies.len(), 2);
        assert!(brep.asm.bodies.iter().any(|body| body.id == projected_body));
    }

    #[test]
    fn body_selectors_use_ordinals_only_for_an_all_null_key_lane() {
        let native_key = |ordinal, key| BodyNativeKey {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                crate::ids::ID_FORMAT,
            ),
            body: BodyId::mint(format!("f3d:brep:entity#{ordinal}")).expect("identity grammar"),
            record_index: ordinal,
            body_ordinal: ordinal,
            source_brep: Some("BREP.source.smb".into()),
            asm_body_key: key,
        };
        let mut brep = Brep {
            asm: AsmBrep {
                body_native_keys: vec![native_key(0, None), native_key(1, None)],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };

        assert_eq!(with_context(|ctx| brep.body_selectors(ctx).unwrap()).len(), 2);
        assert_eq!(
            with_context(|ctx| brep.body_selectors(ctx).unwrap())[&BodyId::mint("f3d:brep:entity#1").expect("identity grammar")],
            1
        );

        brep.asm.body_native_keys[1].asm_body_key = Some(7);
        assert_eq!(
            with_context(|ctx| brep.body_selectors(ctx).unwrap()),
            HashMap::from([(
                BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
                7
            )])
        );
    }

    #[test]
    fn body_selector_id_copy_refuses_retained_limit() {
        let brep = Brep {
            asm: AsmBrep {
                body_native_keys: vec![BodyNativeKey {
                    source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(crate::ids::ID_FORMAT),
                    body: BodyId::mint("f3d:brep:entity#1").unwrap(),
                    record_index: 1,
                    body_ordinal: 0,
                    source_brep: None,
                    asm_body_key: Some(7),
                }],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };
        let error = with_limits(u64::MAX, 0, |ctx| brep.body_selectors(ctx).unwrap_err());
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "copy F3D BREP body ID"));
    }

    #[test]
    fn body_selector_index_refuses_collection_limit() {
        let brep = Brep {
            asm: AsmBrep {
                body_native_keys: vec![BodyNativeKey {
                    source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(crate::ids::ID_FORMAT),
                    body: BodyId::mint("f3d:brep:entity#1").unwrap(),
                    record_index: 1,
                    body_ordinal: 0,
                    source_brep: None,
                    asm_body_key: Some(7),
                }],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };
        let error = with_limits(0, u64::MAX, |ctx| brep.body_selectors(ctx).unwrap_err());
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D BREP body selectors"));
    }

    #[test]
    fn design_body_selectors_prefer_exact_keys_then_fall_back_to_ordinals() {
        let native_key = |ordinal, key| BodyNativeKey {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                crate::ids::ID_FORMAT,
            ),
            body: BodyId::mint(format!("f3d:brep:entity#{ordinal}")).expect("identity grammar"),
            record_index: ordinal,
            body_ordinal: ordinal,
            source_brep: Some("BREP.source.smb".into()),
            asm_body_key: Some(key),
        };
        let mut brep = Brep {
            asm: AsmBrep {
                body_native_keys: vec![native_key(0, 1), native_key(1, 0)],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };

        assert_eq!(
            with_context(|ctx| brep.body_selectors_for(ctx, &HashSet::from([0])).unwrap()),
            HashMap::from([(
                BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
                0
            )])
        );

        brep.asm.body_native_keys = vec![native_key(0, 436)];
        assert_eq!(
            with_context(|ctx| brep.body_selectors_for(ctx, &HashSet::from([0])).unwrap()),
            HashMap::from([(
                BodyId::mint("f3d:brep:entity#0").expect("identity grammar"),
                0
            )])
        );
    }

    #[test]
    fn selected_body_index_refuses_collection_limit() {
        let brep = Brep {
            asm: AsmBrep {
                body_native_keys: vec![BodyNativeKey {
                    source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(crate::ids::ID_FORMAT),
                    body: BodyId::mint("f3d:brep:entity#1").unwrap(),
                    record_index: 1,
                    body_ordinal: 0,
                    source_brep: None,
                    asm_body_key: Some(7),
                }],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };
        let error = with_limits(0, u64::MAX, |ctx| {
            brep.body_selectors_for(ctx, &HashSet::from([7])).unwrap_err()
        });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D selected BREP bodies"));
    }

    #[test]
    fn brep_append_refuses_body_collection_limit() {
        let mut target = Brep::default();
        let source = Brep {
            asm: AsmBrep {
                bodies: vec![Body {
                    id: BodyId::mint("f3d:brep:entity#1").unwrap(),
                    kind: BodyKind::default(),
                    regions: Vec::new(),
                    transform: None,
                    name: None,
                    color: None,
                    visible: None,
                }],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };
        let error = with_limits(0, u64::MAX, |ctx| target.append(ctx, source).unwrap_err());
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "merge F3D BREP bodies"));
    }

    #[test]
    fn brep_append_refuses_statistic_index_limit() {
        let mut target = Brep::default();
        let mut source = Brep::default();
        source.asm.stats.missing_face_surface_kinds.insert("plane".into(), 1);
        let error = with_limits(0, u64::MAX, |ctx| target.append(ctx, source).unwrap_err());
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "merge F3D BREP statistic kinds"));
    }
}
