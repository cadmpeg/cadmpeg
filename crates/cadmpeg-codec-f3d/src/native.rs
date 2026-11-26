// SPDX-License-Identifier: Apache-2.0
//! Autodesk Fusion native design and construction-history records.

#![deny(clippy::disallowed_methods)]

use std::collections::HashMap;

use cadmpeg_core::decode::DecodeContext;

#[cfg(test)]
thread_local! {
    static LOAD_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
/// Reset the per-thread typed-native load counter used by writer tests.
pub(crate) fn reset_load_count() {
    LOAD_COUNT.set(0);
}

#[cfg(test)]
/// Number of typed-native loads performed on this test thread since reset.
pub(crate) fn load_count() -> usize {
    LOAD_COUNT.get()
}

use serde::{Deserialize, Serialize};

use cadmpeg_ir::native::catalogue::{Catalogue, FamilyRow, Phase};

use crate::decode::design_intent::DesignIntentFailure;
use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmHistoricalTransition, AsmHistory,
};
use crate::records::{
    act::{ActComponentLink, ActEntity, ActGuid, ActRegistryChannel, ActTableReference},
    bodies::{BodyVisibility, DesignBodyBinding, DesignBodyBounds, DesignBodyMember},
    canvas::DesignCanvasImage,
    configuration::DesignConfiguration,
    decal::{DesignDecalImage, DesignRecordHeader},
    dimension_locus_arenas::{DesignDimensionLocusPairs, DesignDimensionNullLocusPairs},
    dimensions::{
        DesignDimensionAnnotationFrame, DesignDimensionLocusGroup,
        DesignDimensionPresentationFrame, DesignDimensionRecipeRecord,
    },
    entity_header::{DesignEntityHeader, DesignFeatureTimeline, SegmentType},
    feature::{
        assembly_features::DesignComponentOccurrence, scope::DesignParameterScope,
        surface_ops::DesignSurfaceTrimOperation, work_geometry::DesignEdgeTreatmentVertexOperand,
    },
    mesh::DesignMeshFeature,
    parameters::{DesignParameter, DesignParameterCompanion, DesignParameterOwner},
    placement_construction::DesignPlacementConstruction,
    recipes::{ConstructionRecipe, CreationTimestamp, DesignComponentNamingSpace},
    references::{DesignMaterialAssignment, LostEdgeReference, PersistentReference},
    sketch_geometry::{
        SketchCurveIdentity, SketchGeometryGroup, SketchPoint, SketchSurface, SketchText,
    },
    sketch_links::{PersistentDesignLink, PersistentSubentityTag, SketchCurveLink},
    sketch_placement::DesignSketchPlacement,
    sketch_relations::{SketchRelation, SketchRelationSerde},
    topology::{
        body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
        construction::DesignConstructionOperandIdentity, edge_identity::DesignEdgeIdentityOperand,
        edge_identity::DesignEdgeOperand, entity_selection::DesignEntitySelectionOperand,
        entity_selection::DesignLoftLegacyBodyCarrier,
        extrude_selection::DesignExtrudeSelectionGroup,
        extrude_selection::DesignExtrudeSelectionMember, face::DesignFaceOperand,
        face::DesignFaceSourceGroup, fillet::DesignFilletRadiusGroup,
    },
    xref::{DerivedReference, XrefDesign, XrefReference},
};
use cadmpeg_asm::brep::records::{
    BodyNativeKey, EdgeContinuity, EdgeOwnership, FaceNativeKey, FaceSidedness,
    MeshSurfaceSentinel, TolerantCoedgeParameters, TolerantEdgeTail, TolerantVertexTail,
    TransformHints, VertexOwnership, WireTopology,
};

/// Fast non-cryptographic hasher for internal bounded ID lookup tables.
#[derive(Default)]
struct FastStrHasher(u64);

impl std::hash::Hasher for FastStrHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut h = self.0;
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            let word = u64::from_ne_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            // wrapping-exception: 64-bit multiplicative hash mixing operates modulo 2^64
            h = (h.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
        let mut tail = 0_u64;
        for (i, &b) in chunks.remainder().iter().enumerate() {
            tail |= u64::from(b) << (i * 8);
        }
        if !chunks.remainder().is_empty() {
            // wrapping-exception: 64-bit multiplicative hash mixing operates modulo 2^64
            h = (h.rotate_left(5) ^ tail).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
        let mixed = h ^ cadmpeg_core::decode::u64_from_index(bytes.len());
        // wrapping-exception: 64-bit multiplicative hash mixing operates modulo 2^64
        self.0 = mixed.wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

type FastStrMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<FastStrHasher>>;

fn owner_indices<'a>(ids: impl ExactSizeIterator<Item = &'a str>) -> FastStrMap<&'a str, usize> {
    let mut indexed =
        FastStrMap::with_capacity_and_hasher(ids.len(), std::hash::BuildHasherDefault::default());
    for (ordinal, id) in ids.enumerate() {
        indexed.insert(id, ordinal);
    }
    indexed
}

#[cfg(test)]
fn group_by_owner<T>(
    records: Vec<T>,
    owners: &HashMap<String, usize>,
    owner_count: usize,
    id: impl Fn(&T) -> &str,
    owner: impl Fn(&T) -> &str,
    skipped: Option<&mut Vec<String>>,
) -> Result<Vec<Vec<T>>, cadmpeg_ir::NativeConvertError> {
    group_by_owner_map(records, owners, owner_count, id, owner, skipped)
}

fn group_by_owner_map<T, K, S>(
    records: Vec<T>,
    owners: &HashMap<K, usize, S>,
    owner_count: usize,
    id: impl Fn(&T) -> &str,
    owner: impl Fn(&T) -> &str,
    mut skipped: Option<&mut Vec<String>>,
) -> Result<Vec<Vec<T>>, cadmpeg_ir::NativeConvertError>
where
    K: std::borrow::Borrow<str> + Eq + std::hash::Hash,
    S: std::hash::BuildHasher,
{
    let mut grouped = (0..owner_count).map(|_| Vec::new()).collect::<Vec<_>>();
    for record in records {
        let parent = owner(&record);
        let Some(ordinal) = owners.get(parent) else {
            let reason = format!("orphaned or ambiguously parented records: child {} refers to missing parent {parent}", id(&record));
            if let Some(skipped) = skipped.as_deref_mut() {
                skipped.push(reason);
                continue;
            }
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(reason));
        };
        grouped[*ordinal].push(record);
    }
    Ok(grouped)
}

pub(crate) const F3D_ARENA_NAMES: &[&str] = &[
    "act_component_links",
    "act_entities",
    "act_guids",
    "act_registry_channels",
    "act_table_references",
    "asm_bulletin_boards",
    "asm_delta_states",
    "asm_entity_changes",
    "asm_face_boundaries",
    "asm_histories",
    "asm_history_records",
    "body_native_keys",
    "body_visibilities",
    "construction_recipes",
    "creation_timestamps",
    "derived_references",
    "design_body_bindings",
    "design_body_bounds",
    "design_body_members",
    "design_body_recipe_operands",
    "design_body_reference_tails",
    "design_canvas_images",
    "design_component_naming_spaces",
    "design_component_occurrences",
    "design_component_paths",
    "design_configurations",
    "design_construction_operand_groups",
    "design_construction_operand_identities",
    "design_decal_images",
    "design_dimension_annotation_frames",
    "design_dimension_locus_groups",
    "design_dimension_locus_pairs",
    "design_dimension_null_locus_pairs",
    "design_dimension_presentation_frames",
    "design_dimension_recipe_records",
    "design_edge_identity_operands",
    "design_edge_operands",
    "design_edge_treatment_vertex_operands",
    "design_entity_headers",
    "design_entity_selection_operands",
    "design_extrude_selection_groups",
    "design_extrude_selection_members",
    "design_face_operands",
    "design_face_source_groups",
    "design_feature_timelines",
    "design_fillet_radius_groups",
    "design_intent_failures",
    "design_joint_motions",
    "design_loft_legacy_body_carriers",
    "design_material_assignments",
    "design_mesh_features",
    "design_parameter_companions",
    "design_parameter_owners",
    "design_parameter_scopes",
    "design_parameters",
    "design_placement_constructions",
    "design_profile_curve_selections",
    "design_record_headers",
    "design_sheet_metal_settings",
    "design_sketch_placements",
    "design_surface_trim_operations",
    "design_types",
    "display_poses",
    "edge_continuities",
    "edge_ownerships",
    "face_native_keys",
    "face_sidedness",
    "lost_edge_references",
    "mesh_surface_sentinels",
    "persistent_design_links",
    "persistent_references",
    "persistent_subentity_tags",
    "sketch_curve_identities",
    "sketch_curve_links",
    "sketch_geometry_groups",
    "sketch_points",
    "sketch_relations",
    "sketch_surfaces",
    "sketch_texts",
    "tolerant_coedge_parameters",
    "tolerant_edge_tails",
    "tolerant_vertex_tails",
    "transform_hints",
    "vertex_ownerships",
    "wire_topologies",
    "xref_designs",
    "xref_references",
];

type F3dFamilyRow = FamilyRow<F3dNative, (), cadmpeg_ir::NativeNamespace, ()>;

#[derive(Serialize)]
struct FlatAsmHistory<'a> {
    id: &'a str,
    byte_offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    history_entry_count: Option<i64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    binding_budget_exceeded: bool,
    #[serde(skip_serializing_if = "crate::history_records::is_default")]
    topology_scope: crate::history_records::AsmTopologyScope,
    states: EmptyList,
}

impl<'a> From<&'a AsmHistory> for FlatAsmHistory<'a> {
    fn from(history: &'a AsmHistory) -> Self {
        Self {
            id: &history.id,
            byte_offset: history.byte_offset,
            stream_size: history.stream_size(),
            history_entry_count: history.history_entry_count(),
            binding_budget_exceeded: history.binding_budget_exceeded,
            topology_scope: history.topology_scope,
            states: EmptyList,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct FlatAsmDeltaState<'a> {
    id: &'a str,
    bulletin_boards: EmptyList,
    byte_offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_ref: Option<i64>,
    node_index: i64,
    owner_ref: i64,
    parent: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    partner_ref: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_ref: Option<i64>,
    records: EmptyList,
    state_flag: i64,
    state_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    transition: Option<&'a AsmHistoricalTransition>,
    version_flag: i64,
}

impl<'a> From<&'a AsmDeltaState> for FlatAsmDeltaState<'a> {
    fn from(state: &'a AsmDeltaState) -> Self {
        Self {
            id: &state.id,
            parent: &state.parent,
            byte_offset: state.byte_offset,
            state_id: state.state_id,
            version_flag: state.version_flag,
            state_flag: state.state_flag,
            previous_ref: state.previous_ref,
            next_ref: state.next_ref,
            node_index: state.node_index,
            partner_ref: state.partner_ref,
            owner_ref: state.owner_ref,
            bulletin_boards: EmptyList,
            records: EmptyList,
            transition: state.transition.as_ref(),
        }
    }
}

#[derive(Serialize)]
struct FlatAsmBulletinBoard<'a> {
    id: &'a str,
    byte_offset: u64,
    changes: EmptyList,
    number: i64,
    owner_ref: i64,
    parent: &'a str,
}

impl<'a> From<&'a AsmBulletinBoard> for FlatAsmBulletinBoard<'a> {
    fn from(board: &'a AsmBulletinBoard) -> Self {
        Self {
            id: &board.id,
            parent: &board.parent,
            byte_offset: board.byte_offset,
            owner_ref: board.owner_ref,
            number: board.number,
            changes: EmptyList,
        }
    }
}

/// Read an unescaped JSON string literal starting after its opening `"` and
/// return `(slice, rest_after_closing_quote)`. Returns `None` on any backslash
/// escape or control character so the caller falls back to `serde_json`.
#[inline]
fn take_unescaped_json_str(input: &str) -> Option<(&str, &str)> {
    let bytes = input.as_bytes();
    let end = memchr::memchr2(b'"', b'\\', bytes)?;
    if bytes[end] == b'\\' || bytes[..end].iter().any(|&b| b < 0x20) {
        return None;
    }
    Some((&input[..end], &input[end + 1..]))
}

#[inline]
fn push_u64(out: &mut String, mut value: u64) {
    let mut buf = [0_u8; 20];
    let mut pos = buf.len();
    loop {
        pos -= 1;
        buf[pos] = b'0' + u8::try_from(value % 10).unwrap_or(0);
        value /= 10;
        if value == 0 {
            break;
        }
    }
    if let Ok(text) = std::str::from_utf8(&buf[pos..]) {
        out.push_str(text);
    }
}

#[inline]
fn push_i64(out: &mut String, value: i64) {
    if value < 0 {
        out.push('-');
    }
    push_u64(out, value.unsigned_abs());
}

#[inline]
fn push_hex_bytes(out: &mut String, raw: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut buf = [0_u8; 256];
    for chunk in raw.chunks(128) {
        for (i, &byte) in chunk.iter().enumerate() {
            buf[i * 2] = HEX[usize::from(byte >> 4)];
            buf[i * 2 + 1] = HEX[usize::from(byte & 0x0f)];
        }
        if let Ok(text) = std::str::from_utf8(&buf[..chunk.len() * 2]) {
            out.push_str(text);
        }
    }
}

#[inline]
fn take_json_u64(input: &str) -> Option<(u64, &str)> {
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || (i > 1 && bytes[0] == b'0') {
        return None;
    }
    let val = input[..i].parse::<u64>().ok()?;
    Some((val, &input[i..]))
}

#[inline]
fn take_json_i64(input: &str) -> Option<(i64, &str)> {
    let bytes = input.as_bytes();
    let mut i = 0;
    if bytes.first() == Some(&b'-') {
        i = 1;
    }
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let digit_len = i - digits_start;
    if digit_len == 0 || (digit_len > 1 && bytes[digits_start] == b'0') {
        return None;
    }
    let val = input[..i].parse::<i64>().ok()?;
    Some((val, &input[i..]))
}

/// Fast canonical parser for `asm_entity_changes` records emitted by
/// `emit_asm_entity_changes`. Returns `None` on any non-canonical form so
/// `NativeRecord::to_typed_fast` falls back to `serde_json`.
fn try_parse_canonical_asm_entity_change(
    json: &str,
) -> Option<crate::history_records::AsmEntityChange> {
    let rest = json.strip_prefix("{\"id\":\"")?;
    let (id, rest) = take_unescaped_json_str(rest)?;
    let rest = rest.strip_prefix(",\"byte_offset\":")?;
    let (byte_offset, rest) = take_json_u64(rest)?;
    let rest = rest.strip_prefix(",\"kind\":\"")?;
    let (kind, rest) = if let Some(rest) = rest.strip_prefix("insert\",\"new_ref\":") {
        let (new, rest) = take_json_i64(rest)?;
        (
            crate::history_records::AsmEntityChangeKind::Insert { new },
            rest,
        )
    } else if let Some(rest) = rest.strip_prefix("delete\",\"old_ref\":") {
        let (old, rest) = take_json_i64(rest)?;
        (
            crate::history_records::AsmEntityChangeKind::Delete { old },
            rest,
        )
    } else {
        let rest = rest.strip_prefix("update\",\"new_ref\":")?;
        let (new, rest) = take_json_i64(rest)?;
        let rest = rest.strip_prefix(",\"old_ref\":")?;
        let (old, rest) = take_json_i64(rest)?;
        (
            crate::history_records::AsmEntityChangeKind::Update { old, new },
            rest,
        )
    };
    let rest = rest.strip_prefix(",\"parent\":\"")?;
    let (parent, rest) = take_unescaped_json_str(rest)?;
    if rest != "}" {
        return None;
    }
    Some(crate::history_records::AsmEntityChange {
        id: id.to_owned(),
        parent: parent.to_owned(),
        byte_offset,
        kind,
    })
}

#[inline]
fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Fast canonical parser for framed `asm_history_records` records emitted by
/// `emit_asm_history_records`. Returns `None` on opaque records or any
/// non-canonical form so `NativeRecord::to_typed_fast` falls back to `serde_json`.
fn try_parse_canonical_asm_history_record(
    json: &str,
) -> Option<crate::history_records::AsmHistoryRecord> {
    let rest = json.strip_prefix("{\"id\":\"")?;
    let (id, rest) = take_unescaped_json_str(rest)?;
    let rest = rest.strip_prefix(",\"byte_offset\":")?;
    let (byte_offset, mut rest) = take_json_u64(rest)?;
    let mut entity_references = Vec::new();
    if let Some(mut arr) = rest.strip_prefix(",\"entity_references\":[") {
        if let Some(after) = arr.strip_prefix(']') {
            rest = after;
        } else {
            loop {
                let (val, after) = take_json_i64(arr)?;
                entity_references.push(val);
                if let Some(next) = after.strip_prefix(',') {
                    arr = next;
                } else {
                    rest = after.strip_prefix(']')?;
                    break;
                }
            }
        }
    }
    let rest = rest.strip_prefix(",\"index\":")?;
    let (index, rest) = take_json_u64(rest)?;
    let rest = rest.strip_prefix(",\"name\":\"")?;
    let (name, rest) = take_unescaped_json_str(rest)?;
    let rest = rest.strip_prefix(",\"parent\":\"")?;
    let (parent, rest) = take_unescaped_json_str(rest)?;
    let rest = rest.strip_prefix(",\"raw_bytes\":\"")?;
    let (hex_str, mut rest) = take_unescaped_json_str(rest)?;
    let hex_bytes = hex_str.as_bytes();
    if !hex_bytes.len().is_multiple_of(2) {
        return None;
    }
    let mut raw_bytes = Vec::new();
    for pair in hex_bytes.chunks_exact(2) {
        raw_bytes.push((hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?);
    }
    let revision_id = if let Some(after) = rest.strip_prefix(",\"revision_id\":") {
        let (rev, after) = take_json_i64(after)?;
        rest = after;
        Some(rev)
    } else {
        None
    };
    if rest != "}" {
        return None;
    }
    Some(crate::history_records::AsmHistoryRecord {
        id: id.to_owned(),
        parent: parent.to_owned(),
        revision_id,
        byte_offset,
        framing: crate::history_records::AsmHistoryRecordFraming::Framed {
            index,
            name: name.to_owned(),
            entity_references,
        },
        raw_bytes: raw_bytes.into(),
    })
}

/// A nested collection the flat projection always emits empty.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct EmptyList;

impl Serialize for EmptyList {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(std::iter::empty::<()>())
    }
}

fn emit_asm_histories(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    model: &F3dNative,
    row: &F3dFamilyRow,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    namespace.set_arena_from(
        ctx,
        row.arena,
        model.asm_histories.iter().map(FlatAsmHistory::from),
    )
}

fn emit_asm_delta_states(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    model: &F3dNative,
    row: &F3dFamilyRow,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    namespace.set_arena_from(
        ctx,
        row.arena,
        model
            .asm_histories
            .iter()
            .flat_map(crate::history_records::AsmHistory::states)
            .map(FlatAsmDeltaState::from),
    )
}

fn emit_asm_bulletin_boards(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    model: &F3dNative,
    row: &F3dFamilyRow,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    namespace.set_arena_from(
        ctx,
        row.arena,
        model
            .asm_histories
            .iter()
            .flat_map(crate::history_records::AsmHistory::states)
            .flat_map(|state| &state.bulletin_boards)
            .map(FlatAsmBulletinBoard::from),
    )
}

fn emit_asm_entity_changes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    model: &F3dNative,
    row: &F3dFamilyRow,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let mut out = Vec::new();
    for (ordinal, change) in model
        .asm_histories
        .iter()
        .flat_map(crate::history_records::AsmHistory::states)
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
        .enumerate()
    {
        let mut json = String::with_capacity(change.id.len() + change.parent.len() + 96);
        json.push_str("{\"id\":");
        cadmpeg_ir::native::push_json_str(&mut json, &change.id);
        json.push_str(",\"byte_offset\":");
        push_u64(&mut json, change.byte_offset);
        json.push_str(",\"kind\":");
        match change.kind {
            crate::history_records::AsmEntityChangeKind::Insert { new } => {
                json.push_str("\"insert\",\"new_ref\":");
                push_i64(&mut json, new);
            }
            crate::history_records::AsmEntityChangeKind::Delete { old } => {
                json.push_str("\"delete\",\"old_ref\":");
                push_i64(&mut json, old);
            }
            crate::history_records::AsmEntityChangeKind::Update { old, new } => {
                json.push_str("\"update\",\"new_ref\":");
                push_i64(&mut json, new);
                json.push_str(",\"old_ref\":");
                push_i64(&mut json, old);
            }
        }
        json.push_str(",\"parent\":");
        cadmpeg_ir::native::push_json_str(&mut json, &change.parent);
        json.push('}');
        let record = match cadmpeg_ir::NativeRecord::from_canonical_flat(ctx, &change.id, json, 1) {
            Ok(record) => record,
            Err(source)
                if matches!(
                    source,
                    cadmpeg_ir::NativeConvertError::Resource(
                        cadmpeg_core::CodecError::ResourceLimit(_)
                    )
                ) =>
            {
                return Err(source);
            }
            Err(source) => {
                return Err(cadmpeg_ir::NativeConvertError::Arena {
                    arena: row.arena.to_owned(),
                    source: Box::new(cadmpeg_ir::NativeConvertError::WriteRecord {
                        ordinal,
                        source: Box::new(source),
                    }),
                });
            }
        };
        out.push(record);
    }
    namespace.set_arena_records(row.arena, out)?;
    Ok(())
}

fn emit_asm_history_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    model: &F3dNative,
    row: &F3dFamilyRow,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let mut out = Vec::new();
    for (ordinal, record) in model
        .asm_histories
        .iter()
        .flat_map(crate::history_records::AsmHistory::states)
        .flat_map(|state| &state.records)
        .enumerate()
    {
        let (index, name, framing_error, entity_references) = match &record.framing {
            crate::history_records::AsmHistoryRecordFraming::Framed {
                index,
                name,
                entity_references,
            } => (*index, name.as_str(), None, entity_references.as_slice()),
            crate::history_records::AsmHistoryRecordFraming::Opaque { error } => {
                (0, "opaque_history_payload", Some(error.as_str()), &[][..])
            }
        };
        let raw = record.raw_bytes.as_slice();
        let mut json = String::with_capacity(
            record.id.len()
                + record.parent.len()
                + name.len()
                + raw.len() * 2
                + entity_references.len() * 6
                + 128,
        );
        json.push_str("{\"id\":");
        cadmpeg_ir::native::push_json_str(&mut json, &record.id);
        json.push_str(",\"byte_offset\":");
        push_u64(&mut json, record.byte_offset);
        let depth = if entity_references.is_empty() {
            1
        } else {
            json.push_str(",\"entity_references\":[");
            for (i, &r) in entity_references.iter().enumerate() {
                if i > 0 {
                    json.push(',');
                }
                push_i64(&mut json, r);
            }
            json.push(']');
            2
        };
        if let Some(error) = framing_error {
            json.push_str(",\"framing_error\":");
            cadmpeg_ir::native::push_json_str(&mut json, error);
        }
        json.push_str(",\"index\":");
        push_u64(&mut json, index);
        json.push_str(",\"name\":");
        cadmpeg_ir::native::push_json_str(&mut json, name);
        json.push_str(",\"parent\":");
        cadmpeg_ir::native::push_json_str(&mut json, &record.parent);
        json.push_str(",\"raw_bytes\":\"");
        push_hex_bytes(&mut json, raw);
        json.push('"');
        if let Some(revision_id) = record.revision_id {
            json.push_str(",\"revision_id\":");
            push_i64(&mut json, revision_id);
        }
        json.push('}');
        let native_record =
            match cadmpeg_ir::NativeRecord::from_canonical_flat(ctx, &record.id, json, depth) {
                Ok(native_record) => native_record,
                Err(source)
                    if matches!(
                        source,
                        cadmpeg_ir::NativeConvertError::Resource(
                            cadmpeg_core::CodecError::ResourceLimit(_)
                        )
                    ) =>
                {
                    return Err(source);
                }
                Err(source) => {
                    return Err(cadmpeg_ir::NativeConvertError::Arena {
                        arena: row.arena.to_owned(),
                        source: Box::new(cadmpeg_ir::NativeConvertError::WriteRecord {
                            ordinal,
                            source: Box::new(source),
                        }),
                    });
                }
            };
        out.push(native_record);
    }
    namespace.set_arena_records(row.arena, out)?;
    Ok(())
}

const F3D_FAMILIES: &[F3dFamilyRow] = &[
    F3dFamilyRow {
        arena: "act_entities",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| namespace.set_arena(ctx, row.arena, &model.act_entities),
        len: |model| model.act_entities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "act_guids",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| namespace.set_arena(ctx, row.arena, &model.act_guids),
        len: |model| model.act_guids.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "act_registry_channels",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.act_registry_channels)
        },
        len: |model| model.act_registry_channels.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "act_component_links",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.act_component_links)
        },
        len: |model| model.act_component_links.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "act_table_references",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.act_table_references)
        },
        len: |model| model.act_table_references.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "body_native_keys",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.body_native_keys)
        },
        len: |model| model.body_native_keys.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "body_visibilities",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.body_visibilities)
        },
        len: |model| model.body_visibilities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_types",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| namespace.set_arena(ctx, row.arena, &model.design_types),
        len: |model| model.design_types.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_canvas_images",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_canvas_images)
        },
        len: |model| model.design_canvas_images.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_decal_images",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_decal_images)
        },
        len: |model| model.design_decal_images.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_mesh_features",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_mesh_features)
        },
        len: |model| model.design_mesh_features.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_component_occurrences",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_component_occurrences)
        },
        len: |model| model.design_component_occurrences.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_component_paths",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_component_paths)
        },
        len: |model| model.design_component_paths.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_component_naming_spaces",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_component_naming_spaces)
        },
        len: |model| model.design_component_naming_spaces.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_body_recipe_operands",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_body_recipe_operands)
        },
        len: |model| model.design_body_recipe_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_body_reference_tails",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_body_reference_tails)
        },
        len: |model| model.design_body_reference_tails.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_loft_legacy_body_carriers",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_loft_legacy_body_carriers)
        },
        len: |model| model.design_loft_legacy_body_carriers.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_annotation_frames",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_dimension_annotation_frames)
        },
        len: |model| model.design_dimension_annotation_frames.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_locus_groups",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_dimension_locus_groups)
        },
        len: |model| model.design_dimension_locus_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_locus_pairs",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_dimension_locus_pairs)
        },
        len: |model| model.design_dimension_locus_pairs.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_null_locus_pairs",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena_from(
                ctx,
                row.arena,
                model
                    .design_dimension_null_locus_pairs
                    .iter()
                    .map(crate::records::dimension_null_locus_wire::BorrowedWire::from),
            )
        },
        len: |model| model.design_dimension_null_locus_pairs.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_presentation_frames",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_dimension_presentation_frames)
        },
        len: |model| model.design_dimension_presentation_frames.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_recipe_records",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_dimension_recipe_records)
        },
        len: |model| model.design_dimension_recipe_records.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_edge_operands",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_edge_operands)
        },
        len: |model| model.design_edge_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_edge_treatment_vertex_operands",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_edge_treatment_vertex_operands)
        },
        len: |model| model.design_edge_treatment_vertex_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_edge_identity_operands",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_edge_identity_operands)
        },
        len: |model| model.design_edge_identity_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_entity_selection_operands",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_entity_selection_operands)
        },
        len: |model| model.design_entity_selection_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_face_operands",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_face_operands)
        },
        len: |model| model.design_face_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_face_source_groups",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_face_source_groups)
        },
        len: |model| model.design_face_source_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_construction_operand_groups",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_construction_operand_groups)
        },
        len: |model| model.design_construction_operand_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_construction_operand_identities",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(
                ctx,
                row.arena,
                &model.design_construction_operand_identities,
            )
        },
        len: |model| model.design_construction_operand_identities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_extrude_selection_groups",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_extrude_selection_groups)
        },
        len: |model| model.design_extrude_selection_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_extrude_selection_members",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_extrude_selection_members)
        },
        len: |model| model.design_extrude_selection_members.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_feature_timelines",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_feature_timelines)
        },
        len: |model| model.design_feature_timelines.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_fillet_radius_groups",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_fillet_radius_groups)
        },
        len: |model| model.design_fillet_radius_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_intent_failures",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_intent_failures)
        },
        len: |model| model.design_intent_failures.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_profile_curve_selections",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_profile_curve_selections)
        },
        len: |model| model.design_profile_curve_selections.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_parameter_companions",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_parameter_companions)
        },
        len: |model| model.design_parameter_companions.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_parameter_owners",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_parameter_owners)
        },
        len: |model| model.design_parameter_owners.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_parameter_scopes",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_parameter_scopes)
        },
        len: |model| model.design_parameter_scopes.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_joint_motions",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_joint_motions)
        },
        len: |model| model.design_joint_motions.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_sheet_metal_settings",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_sheet_metal_settings)
        },
        len: |model| model.design_sheet_metal_settings.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_surface_trim_operations",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_surface_trim_operations)
        },
        len: |model| model.design_surface_trim_operations.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_parameters",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_parameters)
        },
        len: |model| model.design_parameters.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_entity_headers",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_entity_headers)
        },
        len: |model| model.design_entity_headers.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_record_headers",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_record_headers)
        },
        len: |model| model.design_record_headers.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_placement_constructions",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_placement_constructions)
        },
        len: |model| model.design_placement_constructions.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_sketch_placements",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_sketch_placements)
        },
        len: |model| model.design_sketch_placements.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_body_bindings",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_body_bindings)
        },
        len: |model| model.design_body_bindings.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_body_bounds",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_body_bounds)
        },
        len: |model| model.design_body_bounds.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_body_members",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_body_members)
        },
        len: |model| model.design_body_members.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_configurations",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_configurations)
        },
        len: |model| model.design_configurations.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_material_assignments",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.design_material_assignments)
        },
        len: |model| model.design_material_assignments.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "edge_continuities",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.edge_continuities)
        },
        len: |model| model.edge_continuities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "edge_ownerships",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.edge_ownerships)
        },
        len: |model| model.edge_ownerships.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "face_sidedness",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.face_sidedness)
        },
        len: |model| model.face_sidedness.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "face_native_keys",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.face_native_keys)
        },
        len: |model| model.face_native_keys.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "construction_recipes",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.construction_recipes)
        },
        len: |model| model.construction_recipes.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "creation_timestamps",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.creation_timestamps)
        },
        len: |model| model.creation_timestamps.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "persistent_design_links",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.persistent_design_links)
        },
        len: |model| model.persistent_design_links.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "persistent_references",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.persistent_references)
        },
        len: |model| model.persistent_references.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "persistent_subentity_tags",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.persistent_subentity_tags)
        },
        len: |model| model.persistent_subentity_tags.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_curve_links",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.sketch_curve_links)
        },
        len: |model| model.sketch_curve_links.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_relations",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.sketch_relations)
        },
        len: |model| model.sketch_relations.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_geometry_groups",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.sketch_geometry_groups)
        },
        len: |model| model.sketch_geometry_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_points",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.sketch_points)
        },
        len: |model| model.sketch_points.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_curve_identities",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.sketch_curve_identities)
        },
        len: |model| model.sketch_curve_identities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_surfaces",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.sketch_surfaces)
        },
        len: |model| model.sketch_surfaces.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_texts",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| namespace.set_arena(ctx, row.arena, &model.sketch_texts),
        len: |model| model.sketch_texts.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "lost_edge_references",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.lost_edge_references)
        },
        len: |model| model.lost_edge_references.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "mesh_surface_sentinels",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.mesh_surface_sentinels)
        },
        len: |model| model.mesh_surface_sentinels.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "vertex_ownerships",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.vertex_ownerships)
        },
        len: |model| model.vertex_ownerships.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "tolerant_coedge_parameters",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.tolerant_coedge_parameters)
        },
        len: |model| model.tolerant_coedge_parameters.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "tolerant_edge_tails",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.tolerant_edge_tails)
        },
        len: |model| model.tolerant_edge_tails.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "tolerant_vertex_tails",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.tolerant_vertex_tails)
        },
        len: |model| model.tolerant_vertex_tails.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "transform_hints",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.transform_hints)
        },
        len: |model| model.transform_hints.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "wire_topologies",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.wire_topologies)
        },
        len: |model| model.wire_topologies.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "display_poses",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.display_poses)
        },
        len: |model| model.display_poses.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "derived_references",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.derived_references)
        },
        len: |model| model.derived_references.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "xref_designs",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| namespace.set_arena(ctx, row.arena, &model.xref_designs),
        len: |model| model.xref_designs.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "xref_references",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.xref_references)
        },
        len: |model| model.xref_references.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "asm_histories",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: emit_asm_histories,
        len: |model| model.asm_histories.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "asm_delta_states",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: emit_asm_delta_states,
        len: |model| {
            model
                .asm_histories
                .iter()
                .map(|history| history.states().len())
                .sum()
        },
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "asm_bulletin_boards",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: emit_asm_bulletin_boards,
        len: |model| {
            model
                .asm_histories
                .iter()
                .flat_map(crate::history_records::AsmHistory::states)
                .map(|state| state.bulletin_boards.len())
                .sum()
        },
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "asm_entity_changes",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: emit_asm_entity_changes,
        len: |model| {
            model
                .asm_histories
                .iter()
                .flat_map(crate::history_records::AsmHistory::states)
                .flat_map(|state| &state.bulletin_boards)
                .map(|board| board.changes.len())
                .sum()
        },
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "asm_history_records",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: emit_asm_history_records,
        len: |model| {
            model
                .asm_histories
                .iter()
                .flat_map(crate::history_records::AsmHistory::states)
                .map(|state| state.records.len())
                .sum()
        },
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "asm_face_boundaries",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |ctx, model, row, namespace| {
            namespace.set_arena(ctx, row.arena, &model.asm_face_boundaries)
        },
        len: |model| model.asm_face_boundaries.len(),
        counts_toward_emptiness: true,
    },
];

const F3D_CATALOGUE: Catalogue<'static, F3dNative, (), cadmpeg_ir::NativeNamespace, ()> =
    Catalogue::new(F3D_FAMILIES);

/// Autodesk Fusion records retained outside the format-neutral model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct F3dNative {
    /// Fusion ACT change-tracking table entities.
    #[serde(default)]
    pub(crate) act_entities: Vec<ActEntity>,
    /// Fusion ACT stream-wide asset/change-version GUID pool.
    #[serde(default)]
    pub(crate) act_guids: Vec<ActGuid>,
    /// Fusion ACT stream-wide named channel registry.
    #[serde(default)]
    pub(crate) act_registry_channels: Vec<ActRegistryChannel>,
    /// Fusion ACT component-to-registry links.
    #[serde(default)]
    pub(crate) act_component_links: Vec<ActComponentLink>,
    /// Fusion ACT table references between the GUID pool and channel registry.
    #[serde(default)]
    pub(crate) act_table_references: Vec<ActTableReference>,
    /// Native Design-join keys stored on ASM bodies.
    #[serde(default)]
    pub(crate) body_native_keys: Vec<BodyNativeKey>,
    /// Design browser-node visibility joined to solved ASM bodies.
    #[serde(default)]
    pub(crate) body_visibilities: Vec<BodyVisibility>,
    /// Design `MetaStream` type-table entries.
    #[serde(default)]
    pub(crate) design_types: Vec<SegmentType>,
    /// Whole-body operands joined to persistent body construction recipes.
    #[serde(default)]
    pub(crate) design_body_recipe_operands: Vec<DesignBodyRecipeOperand>,
    /// Independent reference runs after whole-body operand UUIDs.
    #[serde(default)]
    pub(crate) design_body_reference_tails:
        Vec<crate::records::body_reference_tail::DesignBodyReferenceTail>,
    /// Exact role-less body carriers paired with legacy Boolean-Loft groups.
    #[serde(default)]
    pub(crate) design_loft_legacy_body_carriers: Vec<DesignLoftLegacyBodyCarrier>,
    /// Exact image-plane bindings owned by Canvas timeline objects.
    #[serde(default)]
    pub(crate) design_canvas_images: Vec<DesignCanvasImage>,
    /// Exact image and target bindings owned by Decal timeline objects.
    #[serde(default)]
    pub(crate) design_decal_images: Vec<DesignDecalImage>,
    /// Complete typed `Base Mesh Feature` Design graphs.
    #[serde(default)]
    pub(crate) design_mesh_features: Vec<DesignMeshFeature>,
    /// Exact local component-definition and placed-occurrence carriers.
    #[serde(default)]
    pub(crate) design_component_occurrences: Vec<DesignComponentOccurrence>,
    /// Component-path carriers whose elements are all local records.
    #[serde(default)]
    pub(crate) design_component_paths: Vec<crate::records::component_path::DesignComponentPath>,
    /// Component-local entity naming spaces selected by context UUID.
    #[serde(default)]
    pub(crate) design_component_naming_spaces: Vec<DesignComponentNamingSpace>,
    /// Annotated paired dimension frames governing parameter companions.
    #[serde(default)]
    pub(crate) design_dimension_annotation_frames: Vec<DesignDimensionAnnotationFrame>,
    /// Fusion presentation frames governing parameter companions.
    #[serde(default)]
    pub(crate) design_dimension_presentation_frames: Vec<DesignDimensionPresentationFrame>,
    /// Typed paired loci recovered from dimensional companion graphs.
    #[serde(default)]
    pub(crate) design_dimension_locus_pairs: DesignDimensionLocusPairs,
    /// Counted typed loci recovered from dimensional companion graphs.
    #[serde(default)]
    pub(crate) design_dimension_locus_groups: Vec<DesignDimensionLocusGroup>,
    /// Null-plus-typed loci recovered from dimensional companion graphs.
    #[serde(default)]
    pub(crate) design_dimension_null_locus_pairs: DesignDimensionNullLocusPairs,
    /// Indexed records containing dimension-owned construction recipes.
    #[serde(default)]
    pub(crate) design_dimension_recipe_records: Vec<DesignDimensionRecipeRecord>,
    /// Edge-selection operands recovered from Fillet and Chamfer scopes.
    #[serde(default)]
    pub(crate) design_edge_operands: Vec<DesignEdgeOperand>,
    /// Corner-vertex operands recovered from edge-treatment groups.
    #[serde(default)]
    pub(crate) design_edge_treatment_vertex_operands: Vec<DesignEdgeTreatmentVertexOperand>,
    /// Persistent selection identities recovered from Fillet and Chamfer groups.
    #[serde(default)]
    pub(crate) design_edge_identity_operands: Vec<DesignEdgeIdentityOperand>,
    /// Face-selection operands recovered from Extrude construction groups.
    #[serde(default)]
    pub(crate) design_face_operands: Vec<DesignFaceOperand>,
    /// Ordered persistent source identities recovered from Face operations.
    #[serde(default)]
    pub(crate) design_face_source_groups: Vec<DesignFaceSourceGroup>,
    /// Counted Design scope lists in authored feature order.
    #[serde(default)]
    pub(crate) design_feature_timelines: Vec<DesignFeatureTimeline>,
    /// Counted construction-operand groups owned by feature parameter scopes.
    #[serde(default)]
    pub(crate) design_construction_operand_groups: Vec<DesignConstructionOperandGroup>,
    /// Persistent identity frames named by construction-operand groups.
    #[serde(default)]
    pub(crate) design_construction_operand_identities: Vec<DesignConstructionOperandIdentity>,
    /// Counted selection groups owned by Extrude parameter scopes.
    #[serde(default)]
    pub(crate) design_extrude_selection_groups: Vec<DesignExtrudeSelectionGroup>,
    /// Fixed-width members named by Extrude selection groups.
    #[serde(default)]
    pub(crate) design_extrude_selection_members: Vec<DesignExtrudeSelectionMember>,
    /// Nested persistent-entity operands named by counted construction groups.
    #[serde(default)]
    pub(crate) design_entity_selection_operands: Vec<DesignEntitySelectionOperand>,
    /// Radius parameters paired with counted Fillet edge groups.
    #[serde(default)]
    pub(crate) design_fillet_radius_groups: Vec<DesignFilletRadiusGroup>,
    /// Contained design-intent decode failures. A document with one holds
    /// none of the records design-intent decoding produces.
    #[serde(default)]
    pub(crate) design_intent_failures: Vec<DesignIntentFailure>,
    /// Typed-reference profile selections that list Sketch curve loops.
    #[serde(default)]
    pub(crate) design_profile_curve_selections:
        Vec<crate::records::topology::entity_selection::DesignProfileCurveSelection>,
    /// Fixed prefixes of indexed records paired with parameter owners.
    #[serde(default)]
    pub(crate) design_parameter_companions: Vec<DesignParameterCompanion>,
    /// Same-index-delimited owner frames for indexed Design parameters.
    #[serde(default)]
    pub(crate) design_parameter_owners: Vec<DesignParameterOwner>,
    /// Sketch and construction-operation records that scope parameters.
    #[serde(default)]
    pub(crate) design_parameter_scopes: Vec<DesignParameterScope>,
    /// Freedoms and limits of assembly joints with relative motion.
    #[serde(default)]
    pub(crate) design_joint_motions: Vec<crate::records::feature::joint_motion::DesignJointMotion>,
    /// MetaStream-bounded common sheet-metal settings frames.
    #[serde(default)]
    pub(crate) design_sheet_metal_settings:
        Vec<crate::records::sheet_metal_settings::DesignSheetMetalSettings>,
    /// Exact BRep-cell carriers owned by `SurfaceTrim` operations.
    #[serde(default)]
    pub(crate) design_surface_trim_operations: Vec<DesignSurfaceTrimOperation>,
    /// Indexed Design parameter and expression records.
    #[serde(default)]
    pub(crate) design_parameters: Vec<DesignParameter>,
    /// Local-to-model placement frames for Design sketches.
    #[serde(default)]
    pub(crate) design_sketch_placements: Vec<DesignSketchPlacement>,
    /// Versioned placement members and their exact reference encodings.
    #[serde(default)]
    pub(crate) design_placement_constructions: Vec<DesignPlacementConstruction>,
    /// Self-validating per-entity headers from the Design `BulkStream`.
    #[serde(default)]
    pub(crate) design_entity_headers: Vec<DesignEntityHeader>,
    /// Indexed dynamic-class record headers from the Design `BulkStream`.
    #[serde(default)]
    pub(crate) design_record_headers: Vec<DesignRecordHeader>,
    /// `BodiesRoot` list members from the Design `BulkStream`.
    #[serde(default)]
    pub(crate) design_body_members: Vec<DesignBodyMember>,
    /// Ordered BREP body-map pairs from Design streams.
    #[serde(default)]
    pub(crate) design_body_bindings: Vec<DesignBodyBinding>,
    /// Triplicated axis-aligned bounds cached by Design body containers.
    #[serde(default)]
    pub(crate) design_body_bounds: Vec<DesignBodyBounds>,
    /// Design configuration tables and rules with complete JSON payloads.
    #[serde(default)]
    pub(crate) design_configurations: Vec<DesignConfiguration>,
    /// Design entity-to-material assignment records.
    #[serde(default)]
    pub(crate) design_material_assignments: Vec<DesignMaterialAssignment>,
    /// Kernel continuity classifications stored on solved ASM edges.
    #[serde(default)]
    pub(crate) edge_continuities: Vec<EdgeContinuity>,
    /// Native owner-coedge selectors stored on ASM edges.
    #[serde(default)]
    pub(crate) edge_ownerships: Vec<EdgeOwnership>,
    /// Native single/double-sided classifications stored on ASM faces.
    #[serde(default)]
    pub(crate) face_sidedness: Vec<FaceSidedness>,
    /// Native Design-join keys stored on solved ASM faces.
    #[serde(default)]
    pub(crate) face_native_keys: Vec<FaceNativeKey>,
    /// Parametric regeneration recipes from the Design `BulkStream`.
    #[serde(default)]
    pub(crate) construction_recipes: Vec<ConstructionRecipe>,
    /// Original authoring times attached to solved entities.
    #[serde(default)]
    pub(crate) creation_timestamps: Vec<CreationTimestamp>,
    /// Persistent Fusion design identifiers attached to solved B-rep entities.
    #[serde(default)]
    pub(crate) persistent_design_links: Vec<PersistentDesignLink>,
    /// Persistent point/curve references from Design construction records.
    #[serde(default)]
    pub(crate) persistent_references: Vec<PersistentReference>,
    /// Variable-width persistent tag groups attached to solved faces and edges.
    #[serde(default)]
    pub(crate) persistent_subentity_tags: Vec<PersistentSubentityTag>,
    /// Provenance links from sketch curves to generated B-rep coedges.
    #[serde(default)]
    pub(crate) sketch_curve_links: Vec<SketchCurveLink>,
    /// Bidirectional relations owned by sketch containers.
    #[serde(default)]
    pub(crate) sketch_relations: Vec<SketchRelation>,
    /// Persistent source sketch points.
    #[serde(default)]
    pub(crate) sketch_points: Vec<SketchPoint>,
    /// Persistent identity pairs attached to source sketch-curve records.
    #[serde(default)]
    pub(crate) sketch_curve_identities: Vec<SketchCurveIdentity>,
    /// Persistent tensor-product surfaces owned by spatial sketches.
    #[serde(default)]
    pub(crate) sketch_surfaces: Vec<SketchSurface>,
    #[serde(default)]
    pub(crate) sketch_geometry_groups: Vec<SketchGeometryGroup>,
    /// Persistent text entities owned by planar sketches.
    #[serde(default)]
    pub(crate) sketch_texts: Vec<SketchText>,
    /// Construction-history edge selections that Fusion could not re-resolve.
    #[serde(default)]
    pub(crate) lost_edge_references: Vec<LostEdgeReference>,
    /// Zero-payload ASM mesh-surface sentinels linked to unknown exact surfaces.
    #[serde(default)]
    pub(crate) mesh_surface_sentinels: Vec<MeshSurfaceSentinel>,
    /// Native owner-edge and endpoint-slot fields stored on ASM vertices.
    #[serde(default)]
    pub(crate) vertex_ownerships: Vec<VertexOwnership>,
    /// Native parameter intervals stored on tolerant ASM coedges.
    #[serde(default)]
    pub(crate) tolerant_coedge_parameters: Vec<TolerantCoedgeParameters>,
    /// Native trailing LONG slots stored on tolerant ASM edges.
    #[serde(default)]
    pub(crate) tolerant_edge_tails: Vec<TolerantEdgeTail>,
    /// Native trailing f32 slots stored on tolerant ASM vertices.
    #[serde(default)]
    pub(crate) tolerant_vertex_tails: Vec<TolerantVertexTail>,
    /// Native transform rotation/reflection/shear classifications.
    #[serde(default)]
    pub(crate) transform_hints: Vec<TransformHints>,
    /// Native wire records and their side classifications.
    #[serde(default)]
    pub(crate) wire_topologies: Vec<WireTopology>,
    /// Container external-reference design entries
    /// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
    #[serde(default)]
    pub(crate) xref_designs: Vec<XrefDesign>,
    #[serde(default)]
    pub(crate) derived_references: Vec<DerivedReference>,
    #[serde(default)]
    pub(crate) display_poses: Vec<crate::scene::DisplayPose>,
    /// Container outgoing XREF placements
    /// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
    #[serde(default)]
    pub(crate) xref_references: Vec<XrefReference>,
    /// ASM construction-history containers and their linked delta states.
    #[serde(default)]
    pub(crate) asm_histories: Vec<AsmHistory>,
    /// Historical face boundaries named by edge-recipe reference contexts.
    #[serde(default)]
    pub(crate) asm_face_boundaries:
        Vec<crate::records::topology::historical_context::AsmFaceBoundary>,
}

impl F3dNative {
    /// Design records that declare content a design can hold without a BREP:
    /// parameters and their scopes, feature timelines, sketch carriers,
    /// component occurrences, mesh features, configurations, joints and
    /// material assignments.
    pub(crate) fn declared_record_count(&self) -> usize {
        self.design_parameters.len()
            + self.design_parameter_scopes.len()
            + self.design_feature_timelines.len()
            + self.design_sketch_placements.len()
            + self.design_placement_constructions.len()
            + self.design_component_occurrences.len()
            + self.design_component_paths.len()
            + self.design_mesh_features.len()
            + self.design_configurations.len()
            + self.design_joint_motions.len()
            + self.design_material_assignments.len()
            + self.design_decal_images.len()
            + self.design_surface_trim_operations.len()
            + self.design_sheet_metal_settings.len()
            + self.construction_recipes.len()
            + self.persistent_references.len()
            + self.sketch_points.len()
            + self.sketch_relations.len()
            + self.sketch_curve_identities.len()
            + self.sketch_surfaces.len()
            + self.sketch_geometry_groups.len()
            + self.sketch_texts.len()
    }

    pub(crate) fn load(
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        let decode_arena = cadmpeg_core::decode::DecodeArena::new();
        let (decode_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &decode_arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )
        .map_err(cadmpeg_ir::NativeConvertError::Resource)?;
        let decode_ctx = &decode_ctx;

        Self::load_inner(decode_ctx, namespace)
    }

    /// Load the typed native records from `namespace` under `ctx`.
    pub(crate) fn load_inner(
        ctx: &DecodeContext<'_>,
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        Self::load_impl(ctx, namespace, None)
    }

    pub(crate) fn load_fragment(
        ctx: &DecodeContext<'_>,
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<(Self, Vec<String>), cadmpeg_ir::NativeConvertError> {
        let mut skipped = Vec::new();
        let native = Self::load_impl(ctx, namespace, Some(&mut skipped))?;
        Ok((native, skipped))
    }

    fn load_impl(
        ctx: &DecodeContext<'_>,
        namespace: &cadmpeg_ir::NativeNamespace,
        mut skipped: Option<&mut Vec<String>>,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        macro_rules! read_arena {
            ($name:literal) => {{
                let mut records = Vec::new();
                for record in namespace.arena_iter_as_for_decode(ctx, $name) {
                    match record {
                        Ok(record) => records.push(record),
                        Err(error) if ctx.resource_refusal().is_some() => return Err(error),
                        Err(error) => match skipped.as_deref_mut() {
                            Some(skipped) => skipped.push(format!("{}: {error}", $name)),
                            None => return Err(error),
                        },
                    }
                }
                records
            }};
        }
        let big_history_count = namespace
            .arenas()
            .get("asm_entity_changes")
            .map_or(0, Vec::len)
            + namespace
                .arenas()
                .get("asm_history_records")
                .map_or(0, Vec::len);
        let run_parallel_load = skipped.is_none() && big_history_count >= 512;
        let (native_res, states_outcome, changes_outcome, records_outcome) = if run_parallel_load {
            let checkpoint = ctx.checkpoint();
            std::thread::scope(|scope| {
                let states_handle = {
                    let cp = checkpoint.clone();
                    scope.spawn(move || {
                        let arena = cadmpeg_core::decode::DecodeArena::new();
                        let local_ctx = cp.resume(&arena);
                        let res = (|| -> Result<_, cadmpeg_ir::NativeConvertError> {
                            let states: Vec<crate::history_records::AsmDeltaState> =
                                namespace.arena_as_for_decode(&local_ctx, "asm_delta_states")?;
                            let boards: Vec<crate::history_records::AsmBulletinBoard> =
                                namespace.arena_as_for_decode(&local_ctx, "asm_bulletin_boards")?;
                            Ok((states, boards))
                        })();
                        (res, local_ctx.take_trace())
                    })
                };
                let changes_handle = {
                    let cp = checkpoint.clone();
                    scope.spawn(move || {
                        let arena = cadmpeg_core::decode::DecodeArena::new();
                        let local_ctx = cp.resume(&arena);
                        let res: Result<Vec<crate::history_records::AsmEntityChange>, _> =
                            namespace
                                .arena_iter_as_for_decode_fast(
                                    &local_ctx,
                                    "asm_entity_changes",
                                    try_parse_canonical_asm_entity_change,
                                )
                                .collect();
                        (res, local_ctx.take_trace())
                    })
                };
                let records_handle = {
                    let cp = checkpoint.clone();
                    scope.spawn(move || {
                        let arena = cadmpeg_core::decode::DecodeArena::new();
                        let local_ctx = cp.resume(&arena);
                        let res: Result<Vec<crate::history_records::AsmHistoryRecord>, _> =
                            namespace
                                .arena_iter_as_for_decode_fast(
                                    &local_ctx,
                                    "asm_history_records",
                                    try_parse_canonical_asm_history_record,
                                )
                                .collect();
                        (res, local_ctx.take_trace())
                    })
                };
                let native_res = Self::load_non_history_arenas(ctx, namespace, None);
                (
                    native_res,
                    Some(states_handle.join().ok()),
                    Some(changes_handle.join().ok()),
                    Some(records_handle.join().ok()),
                )
            })
        } else {
            let native_res = Self::load_non_history_arenas(ctx, namespace, skipped.as_deref_mut());
            (native_res, None, None, None)
        };
        let mut native = native_res?;
        let (states, boards) = match states_outcome {
            Some(Some((Ok(pair), trace))) if ctx.accept_trace(&trace) => pair,
            _ => {
                let states: Vec<crate::history_records::AsmDeltaState> =
                    read_arena!("asm_delta_states");
                let boards: Vec<crate::history_records::AsmBulletinBoard> =
                    read_arena!("asm_bulletin_boards");
                (states, boards)
            }
        };
        let changes: Vec<crate::history_records::AsmEntityChange> = match changes_outcome {
            Some(Some((Ok(records), trace))) if ctx.accept_trace(&trace) => records,
            _ => {
                let mut records = Vec::new();
                for record in namespace.arena_iter_as_for_decode_fast(
                    ctx,
                    "asm_entity_changes",
                    try_parse_canonical_asm_entity_change,
                ) {
                    match record {
                        Ok(record) => records.push(record),
                        Err(error) if ctx.resource_refusal().is_some() => return Err(error),
                        Err(error) => match skipped.as_deref_mut() {
                            Some(skipped) => skipped.push(format!("asm_entity_changes: {error}")),
                            None => return Err(error),
                        },
                    }
                }
                records
            }
        };
        let records: Vec<crate::history_records::AsmHistoryRecord> = match records_outcome {
            Some(Some((Ok(records), trace))) if ctx.accept_trace(&trace) => records,
            _ => {
                let mut records = Vec::new();
                for record in namespace.arena_iter_as_for_decode_fast(
                    ctx,
                    "asm_history_records",
                    try_parse_canonical_asm_history_record,
                ) {
                    match record {
                        Ok(record) => records.push(record),
                        Err(error) if ctx.resource_refusal().is_some() => return Err(error),
                        Err(error) => match skipped.as_deref_mut() {
                            Some(skipped) => skipped.push(format!("asm_history_records: {error}")),
                            None => return Err(error),
                        },
                    }
                }
                records
            }
        };
        let board_indices = owner_indices(boards.iter().map(|board| board.id.as_str()));
        let changes_by_board = group_by_owner_map(
            changes,
            &board_indices,
            boards.len(),
            |change| &change.id,
            |change| &change.parent,
            skipped.as_deref_mut(),
        )?;
        let mut attached_boards = Vec::new();
        for (mut board, changes) in boards.into_iter().zip(changes_by_board) {
            board.changes = changes;
            attached_boards.push(board);
        }
        let boards = attached_boards;
        let state_indices = owner_indices(states.iter().map(|state| state.id.as_str()));
        let boards_by_state = group_by_owner_map(
            boards,
            &state_indices,
            states.len(),
            |board| &board.id,
            |board| &board.parent,
            skipped.as_deref_mut(),
        )?;
        let records_by_state = group_by_owner_map(
            records,
            &state_indices,
            states.len(),
            |record| &record.id,
            |record| &record.parent,
            skipped.as_deref_mut(),
        )?;
        let mut attached_states = Vec::new();
        for ((mut state, bulletin_boards), records) in states
            .into_iter()
            .zip(boards_by_state)
            .zip(records_by_state)
        {
            state.bulletin_boards = bulletin_boards;
            state.records = records;
            attached_states.push(state);
        }
        let states = attached_states;
        let history_indices = owner_indices(
            native
                .asm_histories
                .iter()
                .map(|history| history.id.as_str()),
        );
        let states_by_history = group_by_owner_map(
            states,
            &history_indices,
            native.asm_histories.len(),
            |state| &state.id,
            |state| &state.parent,
            skipped.as_deref_mut(),
        )?;
        for (history, states) in native.asm_histories.iter_mut().zip(states_by_history) {
            *history.states_mut() = states;
        }
        if let Err(error) = crate::records::feature::path_features::pipe::admit_native(&native) {
            match skipped {
                Some(skipped) => skipped.push(format!("pipe stream admission: {error}")),
                None => return Err(error),
            }
        }
        Ok(native)
    }

    fn load_non_history_arenas(
        ctx: &DecodeContext<'_>,
        namespace: &cadmpeg_ir::NativeNamespace,
        mut skipped: Option<&mut Vec<String>>,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        macro_rules! read_arena {
            ($name:literal) => {{
                let mut records = Vec::new();
                for record in namespace.arena_iter_as_for_decode(ctx, $name) {
                    match record {
                        Ok(record) => records.push(record),
                        Err(error) if ctx.resource_refusal().is_some() => return Err(error),
                        Err(error) => match skipped.as_deref_mut() {
                            Some(skipped) => skipped.push(format!("{}: {error}", $name)),
                            None => return Err(error),
                        },
                    }
                }
                records
            }};
        }
        let sketch_relations = {
            let wires: Vec<SketchRelationSerde> = read_arena!("sketch_relations");

            let mut relations = Vec::new();

            for wire in wires {
                relations.push(SketchRelation::try_from(wire).map_err(|error| {
                    cadmpeg_ir::NativeConvertError::InvalidCollection(
                        cadmpeg_core::CodecError::Malformed(error.to_string()).to_string(),
                    )
                })?);
            }
            relations
        };
        let null_locus_entries: Vec<crate::records::dimension_null_locus_wire::Entry> =
            read_arena!("design_dimension_null_locus_pairs");
        #[cfg(test)]
        LOAD_COUNT.set(LOAD_COUNT.get() + 1);
        Ok(Self {
            act_entities: read_arena!("act_entities"),
            act_guids: read_arena!("act_guids"),
            act_registry_channels: read_arena!("act_registry_channels"),
            act_component_links: read_arena!("act_component_links"),
            act_table_references: read_arena!("act_table_references"),
            body_native_keys: read_arena!("body_native_keys"),
            body_visibilities: read_arena!("body_visibilities"),
            design_types: read_arena!("design_types"),
            design_canvas_images: read_arena!("design_canvas_images"),
            design_decal_images: read_arena!("design_decal_images"),
            design_mesh_features: read_arena!("design_mesh_features"),
            design_component_occurrences: read_arena!("design_component_occurrences"),
            design_component_paths: read_arena!("design_component_paths"),
            design_component_naming_spaces: read_arena!("design_component_naming_spaces"),
            design_body_recipe_operands: read_arena!("design_body_recipe_operands"),
            design_body_reference_tails: read_arena!("design_body_reference_tails"),
            design_loft_legacy_body_carriers: read_arena!("design_loft_legacy_body_carriers"),
            design_dimension_annotation_frames: read_arena!("design_dimension_annotation_frames"),
            design_dimension_presentation_frames: read_arena!(
                "design_dimension_presentation_frames"
            ),
            design_dimension_locus_groups: read_arena!("design_dimension_locus_groups"),
            design_dimension_locus_pairs: DesignDimensionLocusPairs::try_from(read_arena!(
                "design_dimension_locus_pairs"
            ))
            .map_err(<serde_json::Error as serde::de::Error>::custom)?,
            design_dimension_null_locus_pairs: DesignDimensionNullLocusPairs::from_entries(
                null_locus_entries,
            )?,
            design_dimension_recipe_records: read_arena!("design_dimension_recipe_records"),
            design_edge_operands: read_arena!("design_edge_operands"),
            design_edge_treatment_vertex_operands: read_arena!(
                "design_edge_treatment_vertex_operands"
            ),
            design_edge_identity_operands: read_arena!("design_edge_identity_operands"),
            design_entity_selection_operands: read_arena!("design_entity_selection_operands"),
            design_face_operands: read_arena!("design_face_operands"),
            design_face_source_groups: read_arena!("design_face_source_groups"),
            design_feature_timelines: read_arena!("design_feature_timelines"),
            design_construction_operand_groups: read_arena!("design_construction_operand_groups"),
            design_construction_operand_identities: read_arena!(
                "design_construction_operand_identities"
            ),
            design_extrude_selection_groups: read_arena!("design_extrude_selection_groups"),
            design_extrude_selection_members: read_arena!("design_extrude_selection_members"),
            design_fillet_radius_groups: read_arena!("design_fillet_radius_groups"),
            design_intent_failures: read_arena!("design_intent_failures"),
            design_profile_curve_selections: read_arena!("design_profile_curve_selections"),
            design_parameter_companions: read_arena!("design_parameter_companions"),
            design_parameter_owners: read_arena!("design_parameter_owners"),
            design_parameter_scopes: read_arena!("design_parameter_scopes"),
            design_sheet_metal_settings: read_arena!("design_sheet_metal_settings"),
            design_joint_motions: read_arena!("design_joint_motions"),
            design_surface_trim_operations: read_arena!("design_surface_trim_operations"),
            design_parameters: read_arena!("design_parameters"),
            design_entity_headers: read_arena!("design_entity_headers"),
            design_record_headers: read_arena!("design_record_headers"),
            design_sketch_placements: read_arena!("design_sketch_placements"),
            design_placement_constructions: read_arena!("design_placement_constructions"),
            design_body_bindings: read_arena!("design_body_bindings"),
            design_body_bounds: read_arena!("design_body_bounds"),
            design_body_members: read_arena!("design_body_members"),
            design_configurations: read_arena!("design_configurations"),
            design_material_assignments: read_arena!("design_material_assignments"),
            edge_continuities: read_arena!("edge_continuities"),
            edge_ownerships: read_arena!("edge_ownerships"),
            face_sidedness: read_arena!("face_sidedness"),
            face_native_keys: read_arena!("face_native_keys"),
            construction_recipes: read_arena!("construction_recipes"),
            creation_timestamps: read_arena!("creation_timestamps"),
            persistent_design_links: read_arena!("persistent_design_links"),
            persistent_references: read_arena!("persistent_references"),
            persistent_subentity_tags: read_arena!("persistent_subentity_tags"),
            sketch_curve_links: read_arena!("sketch_curve_links"),
            sketch_relations,
            sketch_points: read_arena!("sketch_points"),
            sketch_curve_identities: read_arena!("sketch_curve_identities"),
            sketch_surfaces: read_arena!("sketch_surfaces"),
            sketch_geometry_groups: read_arena!("sketch_geometry_groups"),
            sketch_texts: read_arena!("sketch_texts"),
            lost_edge_references: read_arena!("lost_edge_references"),
            mesh_surface_sentinels: read_arena!("mesh_surface_sentinels"),
            vertex_ownerships: read_arena!("vertex_ownerships"),
            tolerant_coedge_parameters: read_arena!("tolerant_coedge_parameters"),
            tolerant_edge_tails: read_arena!("tolerant_edge_tails"),
            tolerant_vertex_tails: read_arena!("tolerant_vertex_tails"),
            transform_hints: read_arena!("transform_hints"),
            wire_topologies: read_arena!("wire_topologies"),
            derived_references: read_arena!("derived_references"),
            display_poses: read_arena!("display_poses"),
            xref_designs: read_arena!("xref_designs"),
            xref_references: read_arena!("xref_references"),
            asm_histories: read_arena!("asm_histories"),
            asm_face_boundaries: read_arena!("asm_face_boundaries"),
        })
    }

    pub(crate) fn store(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        let big_history_items: usize = self
            .asm_histories
            .iter()
            .flat_map(crate::history_records::AsmHistory::states)
            .map(|state| {
                state.records.len()
                    + state
                        .bulletin_boards
                        .iter()
                        .map(|board| board.changes.len())
                        .sum::<usize>()
            })
            .sum();
        if big_history_items < 256 {
            F3D_CATALOGUE.emit_all(ctx, self, namespace)?;
            return Ok(());
        }
        let delta_idx = F3D_FAMILIES
            .iter()
            .position(|row| row.arena == "asm_delta_states");
        let changes_idx = F3D_FAMILIES
            .iter()
            .position(|row| row.arena == "asm_entity_changes");
        let records_idx = F3D_FAMILIES
            .iter()
            .position(|row| row.arena == "asm_history_records");
        let (Some(delta_idx), Some(changes_idx), Some(records_idx)) =
            (delta_idx, changes_idx, records_idx)
        else {
            F3D_CATALOGUE.emit_all(ctx, self, namespace)?;
            return Ok(());
        };
        if !(delta_idx < changes_idx && changes_idx < records_idx) {
            F3D_CATALOGUE.emit_all(ctx, self, namespace)?;
            return Ok(());
        }
        let checkpoint = ctx.checkpoint();
        let spawn_row = |row: &'static F3dFamilyRow, cp: cadmpeg_core::decode::DecodeCheckpoint| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let local_ctx = cp.resume(&arena);
            let mut local_ns = cadmpeg_ir::NativeNamespace::default();
            let res = (row.emit)(&local_ctx, self, row, &mut local_ns).map(|()| local_ns);
            (res, local_ctx.take_trace())
        };
        let (prefix_res, delta_out, changes_out, records_out) = std::thread::scope(|scope| {
            let delta_row = &F3D_FAMILIES[delta_idx];
            let changes_row = &F3D_FAMILIES[changes_idx];
            let records_row = &F3D_FAMILIES[records_idx];
            let cp1 = checkpoint.clone();
            let h1 = scope.spawn(move || spawn_row(delta_row, cp1));
            let cp2 = checkpoint.clone();
            let h2 = scope.spawn(move || spawn_row(changes_row, cp2));
            let cp3 = checkpoint.clone();
            let h3 = scope.spawn(move || spawn_row(records_row, cp3));
            let prefix_res = (|| -> Result<(), cadmpeg_ir::NativeConvertError> {
                for row in &F3D_FAMILIES[..delta_idx] {
                    (row.emit)(ctx, self, row, namespace)?;
                }
                Ok(())
            })();
            (prefix_res, h1.join().ok(), h2.join().ok(), h3.join().ok())
        });
        prefix_res?;
        let commit_or_emit = |namespace: &mut cadmpeg_ir::NativeNamespace,
                              row: &F3dFamilyRow,
                              outcome: Option<(
            Result<cadmpeg_ir::NativeNamespace, cadmpeg_ir::NativeConvertError>,
            cadmpeg_core::decode::DecodeTrace,
        )>|
         -> Result<(), cadmpeg_ir::NativeConvertError> {
            if let Some((Ok(mut local_ns), trace)) = outcome {
                if ctx.accept_trace(&trace) {
                    if let Some((key, records)) = local_ns.arenas_mut().pop_first() {
                        namespace.arenas_mut().insert(key, records);
                    }
                    return Ok(());
                }
            }
            (row.emit)(ctx, self, row, namespace)
        };
        commit_or_emit(namespace, &F3D_FAMILIES[delta_idx], delta_out)?;
        for row in &F3D_FAMILIES[delta_idx + 1..changes_idx] {
            (row.emit)(ctx, self, row, namespace)?;
        }
        commit_or_emit(namespace, &F3D_FAMILIES[changes_idx], changes_out)?;
        for row in &F3D_FAMILIES[changes_idx + 1..records_idx] {
            (row.emit)(ctx, self, row, namespace)?;
        }
        commit_or_emit(namespace, &F3D_FAMILIES[records_idx], records_out)?;
        for row in &F3D_FAMILIES[records_idx + 1..] {
            (row.emit)(ctx, self, row, namespace)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
