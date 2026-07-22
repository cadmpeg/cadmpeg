// SPDX-License-Identifier: Apache-2.0
//! Record patchers that apply validated edit sets to archive bytes.

use std::collections::{BTreeMap, BTreeSet};

use crate::records::{
    ActEntity, ActRootComponent, DesignMaterialAssignment, LostEdgeReference, SketchCurveGeometry,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::math::Point3;

use super::edits::{
    ActGuidEdit, BodyMemberEdit, ConstructionRecipeEdit, DesignObjectEdit, EntityHeaderEdit,
    HistoryEdits, PersistentReferenceEdit, SketchCurveEdit, SketchPointEdit, SketchRelationEdit,
};
use super::geometry::{active_ref_width, patch_integer_field, required_payload_field};
use crate::nurbs::reader::LEN_TO_MM;
use crate::writer::primitives::native_bool;
use crate::{asm_header, sab};

pub(crate) fn patch_material_assignments(
    bytes: &mut [u8],
    edits: &[DesignMaterialAssignment],
) -> Result<(), CodecError> {
    for assignment in edits {
        let suffix_start = usize::try_from(assignment.entity_suffix_offset).map_err(|_| {
            CodecError::Malformed("material-assignment suffix offset exceeds address space".into())
        })?;
        bytes
            .get_mut(suffix_start..suffix_start + 8)
            .ok_or_else(|| CodecError::Malformed("material-assignment suffix is truncated".into()))?
            .copy_from_slice(&assignment.entity_suffix.to_le_bytes());
        patch_utf16_if_changed(
            bytes,
            assignment.entity_id_offset,
            &assignment.entity_id,
            "material-assignment entity id",
        )?;
        patch_utf16_if_changed(
            bytes,
            assignment.visual_guid_offset,
            &assignment.visual_guid,
            "material-assignment visual GUID",
        )?;
        if let (Some(offset), Some(value)) = (
            assignment.physical_token_offset,
            assignment.physical_token.as_deref(),
        ) {
            patch_utf16_if_changed(bytes, offset, value, "material-assignment physical token")?;
        }
        if let (Some(offset), Some(value)) = (
            assignment.visual_preset_offset,
            assignment.visual_preset.as_deref(),
        ) {
            patch_utf16_if_changed(bytes, offset, value, "material-assignment visual preset")?;
        }
    }
    Ok(())
}

pub(crate) fn patch_lost_edge_references(
    bytes: &mut [u8],
    edits: &[LostEdgeReference],
) -> Result<(), CodecError> {
    for reference in edits {
        patch_bytes_at(
            bytes,
            reference.class_tag_offset,
            reference.class_tag.as_bytes(),
            "lost-edge class tag",
        )?;
        patch_u32_at(
            bytes,
            reference.record_index_offset,
            reference.record_index,
            "lost-edge record index",
        )?;
    }
    Ok(())
}

pub(crate) fn patch_act_entities(bytes: &mut [u8], edits: &[ActEntity]) -> Result<(), CodecError> {
    for entity in edits {
        let encoded_id = entity
            .entity_id
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        for offset in [
            entity.table_entity_id_offset,
            entity.channel_entity_id_offset,
        ]
        .into_iter()
        .flatten()
        {
            patch_bytes_at(bytes, offset, &encoded_id, "ACT entity id")?;
        }
        for (name, guid) in &entity.channels {
            let encoded = guid
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            patch_bytes_at(
                bytes,
                entity.channel_guid_offsets[name],
                &encoded,
                "ACT channel GUID",
            )?;
        }
    }
    Ok(())
}

pub(crate) fn patch_act_guids(bytes: &mut [u8], edits: &[ActGuidEdit]) -> Result<(), CodecError> {
    for (offset, encoded) in edits {
        patch_bytes_at(bytes, *offset, encoded, "ACT GUID")?;
    }
    Ok(())
}

pub(crate) fn patch_act_roots(
    bytes: &mut [u8],
    edits: &[ActRootComponent],
) -> Result<(), CodecError> {
    for root in edits {
        for (offset, value, field) in [
            (
                root.record_index_offset,
                root.record_index,
                "ACT root record index",
            ),
            (
                root.instance_root_record_offset,
                root.instance_root_record,
                "ACT instance-root reference",
            ),
            (
                root.components_root_record_offset,
                root.components_root_record,
                "ACT components-root reference",
            ),
            (
                root.registry_flag_offset,
                root.registry_flag,
                "ACT registry flag",
            ),
        ] {
            patch_u32_at(bytes, offset, value, field)?;
        }
        patch_utf16_if_changed(
            bytes,
            root.entity_id_offset,
            &root.entity_id,
            "ACT root entity id",
        )?;
        patch_utf16_if_changed(
            bytes,
            root.display_name_offset,
            &root.display_name,
            "ACT root display name",
        )?;
    }
    Ok(())
}

fn patch_utf16_if_changed(
    bytes: &mut [u8],
    offset: u64,
    value: &str,
    field: &str,
) -> Result<(), CodecError> {
    let encoded = value
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    patch_bytes_at(bytes, offset, &encoded, field)
}

pub(crate) fn canonical_guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

pub(crate) fn native_stream(id: &str, delimiter: &str) -> Result<String, CodecError> {
    id.strip_prefix(crate::ids::SCHEME_PREFIX)
        .and_then(|id| id.rsplit_once(delimiter))
        .map(|(stream, _)| stream.to_owned())
        .ok_or_else(|| CodecError::Malformed(format!("invalid native record id {id}")))
}

fn patch_bytes_at(
    bytes: &mut [u8],
    offset: u64,
    encoded: &[u8],
    field: &str,
) -> Result<(), CodecError> {
    let start = usize::try_from(offset)
        .map_err(|_| CodecError::Malformed(format!("{field} offset exceeds address space")))?;
    bytes
        .get_mut(start..start + encoded.len())
        .ok_or_else(|| CodecError::Malformed(format!("{field} is truncated")))?
        .copy_from_slice(encoded);
    Ok(())
}

pub(crate) fn patch_design_objects(
    bytes: &mut [u8],
    edits: &[DesignObjectEdit],
) -> Result<(), CodecError> {
    for edit in edits {
        for (offset, encoded) in edit.integers.iter().chain(&edit.strings) {
            let start = usize::try_from(*offset).map_err(|_| {
                CodecError::Malformed("design-object offset exceeds address space".into())
            })?;
            bytes
                .get_mut(start..start + encoded.len())
                .ok_or_else(|| CodecError::Malformed("design-object field is truncated".into()))?
                .copy_from_slice(encoded);
        }
    }
    Ok(())
}

pub(crate) fn patch_entity_headers(
    bytes: &mut [u8],
    edits: &[EntityHeaderEdit],
) -> Result<(), CodecError> {
    for edit in edits {
        if let Some((offset, value)) = edit.record_reference {
            patch_u32_at(bytes, offset, value, "entity-header record reference")?;
        }
        for &(offset, value) in &edit.references {
            patch_u32_at(bytes, offset, value, "entity-header child reference")?;
        }
    }
    Ok(())
}

fn patch_u32_at(bytes: &mut [u8], offset: u64, value: u32, field: &str) -> Result<(), CodecError> {
    let start = usize::try_from(offset)
        .map_err(|_| CodecError::Malformed(format!("{field} offset exceeds address space")))?;
    bytes
        .get_mut(start..start + 4)
        .ok_or_else(|| CodecError::Malformed(format!("{field} is truncated")))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

pub(crate) fn patch_body_members(
    bytes: &mut [u8],
    edits: &[BodyMemberEdit],
) -> Result<(), CodecError> {
    for &(offset, entity_suffix, flags) in edits {
        let start = usize::try_from(offset).map_err(|_| {
            CodecError::Malformed("design-body-member offset exceeds address space".into())
        })?;
        if bytes.get(start) != Some(&1) {
            return Err(CodecError::Malformed(format!(
                "design-body-member at byte {start} has no presence marker"
            )));
        }
        bytes
            .get_mut(start + 1..start + 9)
            .ok_or_else(|| CodecError::Malformed("design-body-member id is truncated".into()))?
            .copy_from_slice(&entity_suffix.to_le_bytes());
        bytes
            .get_mut(start + 9..start + 11)
            .ok_or_else(|| CodecError::Malformed("design-body-member flags are truncated".into()))?
            .copy_from_slice(&flags.to_le_bytes());
    }
    Ok(())
}

pub(crate) fn patch_body_visibilities(
    bytes: &mut [u8],
    edits: &[(u64, bool)],
) -> Result<(), CodecError> {
    for &(offset, visible) in edits {
        let at = usize::try_from(offset).map_err(|_| {
            CodecError::Malformed("body-visibility offset exceeds address space".into())
        })?;
        let flag = bytes
            .get_mut(at)
            .filter(|flag| **flag <= 1)
            .ok_or_else(|| {
                CodecError::Malformed("body-visibility flag is missing or invalid".into())
            })?;
        *flag = u8::from(!visible);
    }
    Ok(())
}

pub(crate) fn patch_design_body_keys(
    bytes: &mut [u8],
    edits: &BTreeSet<(u64, u64)>,
) -> Result<(), CodecError> {
    for &(offset, key) in edits {
        let at = usize::try_from(offset).map_err(|_| {
            CodecError::Malformed("Design body-key offset exceeds address space".into())
        })?;
        bytes
            .get_mut(at..at + 8)
            .ok_or_else(|| CodecError::Malformed("Design body-map key is truncated".into()))?
            .copy_from_slice(&key.to_le_bytes());
    }
    Ok(())
}

pub(crate) fn patch_body_native_keys(
    bytes: &mut [u8],
    edits: &BTreeMap<usize, i64>,
) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    let start = asm_header::record_stream_start(bytes)
        .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
    let limit = asm_header::first_delta_state_offset(bytes).unwrap_or(bytes.len());
    let ref_width = asm_header::parse(bytes).map_or(8, |header| usize::from(header.width));
    let records = sab::frame(bytes, start, limit, ref_width)
        .map_err(|error| CodecError::Malformed(format!("cannot frame active BREP: {error}")))?;
    for (record_index, key) in edits {
        let record = records
            .iter()
            .find(|record| record.index == *record_index)
            .ok_or_else(|| {
                CodecError::Malformed(format!("F3D body-key record {record_index} is missing"))
            })?;
        if record.head != "body" {
            return Err(CodecError::Malformed(format!(
                "F3D body-key record {record_index} is not a body"
            )));
        }
        patch_integer_field(bytes, record, ref_width, 1, 0x04, *key)?;
    }
    Ok(())
}

pub(crate) fn patch_transform_hints(
    bytes: &mut [u8],
    edits: &BTreeMap<usize, [bool; 3]>,
) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    let start = asm_header::record_stream_start(bytes)
        .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
    let limit = asm_header::first_delta_state_offset(bytes).unwrap_or(bytes.len());
    let ref_width = active_ref_width(bytes);
    let records = sab::frame(bytes, start, limit, ref_width)
        .map_err(|error| CodecError::Malformed(format!("cannot frame active BREP: {error}")))?;
    for (record_index, flags) in edits {
        let record = records
            .iter()
            .find(|record| record.index == *record_index)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "F3D transform-hint record {record_index} is missing"
                ))
            })?;
        if !record.name.ends_with("transform") {
            return Err(CodecError::Malformed(format!(
                "F3D transform-hint record {record_index} is {}, not a transform",
                record.head
            )));
        }
        for (index, flag) in (5usize..=7).zip(flags) {
            let offset =
                sab::payload_token_offset(bytes, record, ref_width, index).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "F3D transform record {record_index} lacks hint field {index}"
                    ))
                })?;
            if !matches!(bytes.get(offset), Some(0x0a | 0x0b)) {
                return Err(CodecError::Malformed(format!(
                    "F3D transform record {record_index} field {index} is not a hint flag"
                )));
            }
            bytes[offset] = native_bool(*flag);
        }
    }
    Ok(())
}

pub(crate) fn patch_tolerant_coedge_parameters(
    bytes: &mut [u8],
    edits: &BTreeMap<usize, [f64; 2]>,
) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    let start = asm_header::record_stream_start(bytes)
        .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
    let limit = asm_header::first_delta_state_offset(bytes).unwrap_or(bytes.len());
    let ref_width = active_ref_width(bytes);
    let records = sab::frame(bytes, start, limit, ref_width)
        .map_err(|error| CodecError::Malformed(format!("cannot frame active BREP: {error}")))?;
    for (record_index, range) in edits {
        let record = records
            .iter()
            .find(|record| record.index == *record_index)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "F3D tolerant-coedge record {record_index} is missing"
                ))
            })?;
        if record.head != "tcoedge" {
            return Err(CodecError::Malformed(format!(
                "F3D tolerant-coedge record {record_index} is {}",
                record.head
            )));
        }
        for (index, value) in [(11usize, range[0]), (12, range[1])] {
            let offset = required_payload_field(bytes, record, ref_width, index, 0x06)?;
            bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}

pub(crate) fn patch_wire_topologies(
    bytes: &mut [u8],
    edits: &BTreeMap<usize, crate::records::WireSide>,
) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    let start = asm_header::record_stream_start(bytes)
        .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
    let limit = asm_header::first_delta_state_offset(bytes).unwrap_or(bytes.len());
    let ref_width = active_ref_width(bytes);
    let records = sab::frame(bytes, start, limit, ref_width)
        .map_err(|error| CodecError::Malformed(format!("cannot frame active BREP: {error}")))?;
    for (record_index, side) in edits {
        let record = records
            .iter()
            .find(|record| record.index == *record_index)
            .ok_or_else(|| {
                CodecError::Malformed(format!("F3D wire record {record_index} is missing"))
            })?;
        if record.head != "wire" {
            return Err(CodecError::Malformed(format!(
                "F3D wire record {record_index} is {}",
                record.head
            )));
        }
        let offset = sab::payload_token_offset(bytes, record, ref_width, 7).ok_or_else(|| {
            CodecError::Malformed(format!("F3D wire record {record_index} lacks side field 7"))
        })?;
        if !matches!(bytes.get(offset), Some(0x0a | 0x0b)) {
            return Err(CodecError::Malformed(format!(
                "F3D wire record {record_index} field 7 is not a side token"
            )));
        }
        bytes[offset] = match side {
            crate::records::WireSide::In => 0x0a,
            crate::records::WireSide::Out => 0x0b,
        };
    }
    Ok(())
}

pub(crate) fn patch_edge_ownerships(
    bytes: &mut [u8],
    edits: &BTreeMap<usize, i64>,
) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    let start = asm_header::record_stream_start(bytes)
        .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
    let limit = asm_header::first_delta_state_offset(bytes).unwrap_or(bytes.len());
    let ref_width = active_ref_width(bytes);
    let records = sab::frame(bytes, start, limit, ref_width)
        .map_err(|error| CodecError::Malformed(format!("cannot frame active BREP: {error}")))?;
    for (record_index, owner) in edits {
        let record = records
            .iter()
            .find(|record| record.index == *record_index)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "F3D edge-ownership record {record_index} is missing"
                ))
            })?;
        if !matches!(record.head.as_str(), "edge" | "tedge") {
            return Err(CodecError::Malformed(format!(
                "F3D edge-ownership record {record_index} is {}",
                record.head
            )));
        }
        patch_integer_field(bytes, record, ref_width, 7, 0x0c, *owner)?;
    }
    Ok(())
}

pub(crate) fn patch_construction_recipes(
    bytes: &mut [u8],
    edits: &[ConstructionRecipeEdit],
) -> Result<(), CodecError> {
    for edit in edits {
        if let Some((offset, record_index)) = edit.record_index {
            let start = usize::try_from(offset).map_err(|_| {
                CodecError::Malformed("construction-recipe offset exceeds address space".into())
            })?;
            bytes
                .get_mut(start..start + 4)
                .ok_or_else(|| {
                    CodecError::Malformed("construction-recipe record index is truncated".into())
                })?
                .copy_from_slice(&record_index.to_le_bytes());
        }
        if let Some((offset, encoded)) = &edit.design_id {
            let start = usize::try_from(*offset).map_err(|_| {
                CodecError::Malformed(
                    "construction-recipe design-id offset exceeds address space".into(),
                )
            })?;
            bytes
                .get_mut(start..start + encoded.len())
                .ok_or_else(|| {
                    CodecError::Malformed("construction-recipe design id is truncated".into())
                })?
                .copy_from_slice(encoded);
        }
    }
    Ok(())
}

pub(crate) fn patch_persistent_references(
    bytes: &mut [u8],
    edits: &[PersistentReferenceEdit],
) -> Result<(), CodecError> {
    for &(record_offset, value_offset, value) in edits {
        let start = usize::try_from(record_offset)
            .ok()
            .and_then(|offset| offset.checked_add(value_offset as usize))
            .ok_or_else(|| {
                CodecError::Malformed("persistent-reference offset exceeds address space".into())
            })?;
        bytes
            .get_mut(start..start + 8)
            .ok_or_else(|| CodecError::Malformed("persistent-reference value is truncated".into()))?
            .copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

pub(crate) fn patch_history_states(
    bytes: &mut [u8],
    edits: &HistoryEdits,
) -> Result<(), CodecError> {
    const DELTA_HEADER_LEN: usize = b"\x11\x0d\x0bdelta_state".len();
    const PREAMBLE_LEN: usize = b"\x0d\x0ehistory_stream".len();
    if let Some(history) = &edits.preamble {
        let start = usize::try_from(history.byte_offset)
            .ok()
            .and_then(|offset| offset.checked_add(PREAMBLE_LEN))
            .ok_or_else(|| {
                CodecError::Malformed("ASM preamble offset exceeds address space".into())
            })?;
        let size = history.stream_size;
        let entry_count = history.history_entry_count;
        for (ordinal, value) in [(0, size), (1, size), (3, entry_count)] {
            let tag = start + ordinal * 9;
            if bytes.get(tag) != Some(&0x04) {
                return Err(CodecError::Malformed(format!(
                    "ASM history-preamble field {ordinal} at byte {tag} is not a long token"
                )));
            }
            bytes
                .get_mut(tag + 1..tag + 9)
                .ok_or_else(|| CodecError::Malformed("ASM history preamble is truncated".into()))?
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    for state in &edits.states {
        let first_tag = usize::try_from(state.byte_offset)
            .ok()
            .and_then(|offset| offset.checked_add(DELTA_HEADER_LEN))
            .ok_or_else(|| {
                CodecError::Malformed("ASM history offset exceeds address space".into())
            })?;
        let values = [
            (0, 0x04, state.state_id),
            (1, 0x04, state.version_flag),
            (2, 0x04, state.state_flag),
            (3, 0x0c, state.previous_ref.unwrap_or(-1)),
            (4, 0x0c, state.next_ref.unwrap_or(-1)),
            (5, 0x0c, state.node_index),
            (6, 0x0c, state.partner_ref.unwrap_or(-1)),
            (7, 0x0c, state.owner_ref),
        ];
        for (ordinal, expected_tag, value) in values {
            let tag = first_tag + ordinal * 9;
            if bytes.get(tag) != Some(&expected_tag) {
                return Err(CodecError::Malformed(format!(
                    "ASM delta-state field {ordinal} at byte {tag} has the wrong token tag"
                )));
            }
            bytes
                .get_mut(tag + 1..tag + 9)
                .ok_or_else(|| CodecError::Malformed("ASM delta-state field is truncated".into()))?
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    for board in &edits.boards {
        patch_tagged_i64(bytes, board.byte_offset, 1, 0x0c, board.owner_ref)?;
        patch_tagged_i64(bytes, board.byte_offset, 2, 0x04, board.number)?;
    }
    for change in &edits.changes {
        patch_tagged_i64(
            bytes,
            change.byte_offset,
            1,
            0x0c,
            change.old_ref.unwrap_or(-1),
        )?;
        patch_tagged_i64(
            bytes,
            change.byte_offset,
            2,
            0x0c,
            change.new_ref.unwrap_or(-1),
        )?;
    }
    Ok(())
}

fn patch_tagged_i64(
    bytes: &mut [u8],
    record_offset: u64,
    ordinal: usize,
    expected_tag: u8,
    value: i64,
) -> Result<(), CodecError> {
    let tag = usize::try_from(record_offset)
        .ok()
        .and_then(|offset| offset.checked_add(ordinal * 9))
        .ok_or_else(|| CodecError::Malformed("ASM record offset exceeds address space".into()))?;
    if bytes.get(tag) != Some(&expected_tag) {
        return Err(CodecError::Malformed(format!(
            "ASM field {ordinal} at byte {tag} has the wrong token tag"
        )));
    }
    bytes
        .get_mut(tag + 1..tag + 9)
        .ok_or_else(|| CodecError::Malformed("ASM tagged integer is truncated".into()))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

pub(crate) fn patch_sketch_points(
    bytes: &mut [u8],
    edits: &[SketchPointEdit],
) -> Result<(), CodecError> {
    for (record_offset, coordinate_offset, coordinates) in edits {
        let start = usize::try_from(*record_offset)
            .ok()
            .and_then(|record| record.checked_add(*coordinate_offset as usize))
            .ok_or_else(|| {
                CodecError::Malformed("sketch-point offset exceeds address space".into())
            })?;
        let payload = bytes.get_mut(start..start + 16).ok_or_else(|| {
            CodecError::Malformed("sketch-point coordinate payload is outside BulkStream".into())
        })?;
        payload[..8].copy_from_slice(&(coordinates.u / LEN_TO_MM).to_le_bytes());
        payload[8..].copy_from_slice(&(coordinates.v / LEN_TO_MM).to_le_bytes());
    }
    Ok(())
}

pub(crate) fn patch_sketch_curves(
    bytes: &mut [u8],
    edits: &[SketchCurveEdit],
) -> Result<(), CodecError> {
    for (record_offset, geometry_offset, geometry) in edits {
        let start = usize::try_from(*record_offset)
            .ok()
            .and_then(|record| record.checked_add(*geometry_offset as usize))
            .ok_or_else(|| {
                CodecError::Malformed("sketch-curve offset exceeds address space".into())
            })?;
        if let SketchCurveGeometry::Nurbs {
            fit_tolerance,
            knots,
            weights,
            control_points,
            ..
        } = geometry
        {
            patch_sketch_nurbs(bytes, start, *fit_tolerance, knots, weights, control_points)?;
            continue;
        }
        let payload = bytes.get_mut(start..start + 96).ok_or_else(|| {
            CodecError::Malformed("sketch-curve analytic payload is outside BulkStream".into())
        })?;
        let values = match geometry {
            SketchCurveGeometry::Line {
                start,
                end,
                direction,
                normal,
            } => [
                start.x / LEN_TO_MM,
                start.y / LEN_TO_MM,
                start.z / LEN_TO_MM,
                (end.x - start.x) / LEN_TO_MM,
                (end.y - start.y) / LEN_TO_MM,
                (end.z - start.z) / LEN_TO_MM,
                direction.x,
                direction.y,
                direction.z,
                normal.x,
                normal.y,
                normal.z,
            ],
            SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => [
                center.x / LEN_TO_MM,
                center.y / LEN_TO_MM,
                center.z / LEN_TO_MM,
                normal.x,
                normal.y,
                normal.z,
                reference_direction.x,
                reference_direction.y,
                reference_direction.z,
                radius / LEN_TO_MM,
                *start_angle,
                *end_angle,
            ],
            SketchCurveGeometry::Nurbs { .. } => unreachable!("NURBS handled before fixed payload"),
        };
        for (ordinal, value) in values.into_iter().enumerate() {
            payload[ordinal * 8..ordinal * 8 + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}

fn patch_sketch_nurbs(
    bytes: &mut [u8],
    start: usize,
    fit_tolerance: f64,
    knots: &[f64],
    weights: &[f64],
    control_points: &[Point3],
) -> Result<(), CodecError> {
    let fit_at = start + 94;
    let knots_at = start + 114;
    let weights_header = knots_at + knots.len() * 8;
    let weights_at = weights_header + 12;
    let points_header = weights_at + weights.len() * 8;
    let points_at = points_header + 12;
    let end = points_at + control_points.len() * 24;
    if end > bytes.len() {
        return Err(CodecError::Malformed(
            "sketch NURBS arrays extend beyond BulkStream".into(),
        ));
    }
    bytes[fit_at..fit_at + 8].copy_from_slice(&(fit_tolerance / LEN_TO_MM).to_le_bytes());
    for (ordinal, value) in knots.iter().enumerate() {
        let at = knots_at + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (ordinal, value) in weights.iter().enumerate() {
        let at = weights_at + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (ordinal, point) in control_points.iter().enumerate() {
        let at = points_at + ordinal * 24;
        for (component, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            let component_at = at + component * 8;
            bytes[component_at..component_at + 8]
                .copy_from_slice(&(value / LEN_TO_MM).to_le_bytes());
        }
    }
    Ok(())
}

pub(crate) fn patch_sketch_relations(
    bytes: &mut [u8],
    edits: &[SketchRelationEdit],
) -> Result<(), CodecError> {
    for edit in edits {
        for (offset, value) in edit {
            patch_bytes_at(bytes, *offset, value, "sketch-relation value")?;
        }
    }
    Ok(())
}
