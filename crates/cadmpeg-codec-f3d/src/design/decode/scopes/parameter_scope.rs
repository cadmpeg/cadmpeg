// SPDX-License-Identifier: Apache-2.0
//! Decode parameter scopes and parse one scope payload.

use super::assembly_alignment::exact_assembly_alignment;
use super::axial_assembly::bind_axial_assembly_operand_targets;
use super::axial_assembly::bind_joint_origin_frames_from_assemblies;
use super::base_feature::exact_base_feature_construction;
use super::coil::bind_coil_extent_from_parameters;
use super::coil::exact_coil_discriminators;
use super::coil::exact_coil_placement;
use super::coil::exact_long_coil_transform;
use super::combine::exact_combine_operation;
use super::component_constructions::bind_component_pattern_occurrences;
use super::component_constructions::exact_component_insert_construction;
use super::component_constructions::exact_copy_paste_component_operation;
use super::component_constructions::exact_derived_instance_construction;
use super::copy_paste_bodies::exact_copy_paste_bodies_operation;
use super::direct_face::exact_direct_face_operation;
use super::direct_face::exact_move_operation;
use super::direct_face::exact_scale_operation;
use super::draft::exact_draft_operation_with_owners;
use super::extrude::exact_extrude_prologue;
use super::fixed_parameters::exact_fixed_chamfer_parameters;
use super::fixed_parameters::exact_fixed_extrude_parameters;
use super::fixed_parameters::exact_fixed_fillet_parameters;
use super::hole::exact_hole_construction;
use super::path_feature::exact_path_feature_construction;
use super::pattern::exact_circular_pattern_construction_with_owners;
use super::pattern::exact_rectangular_pattern_construction;
use super::point_data::exact_work_point_construction;
use super::sheet_metal::bind_hem_operation_from_parameters;
use super::sheet_metal::exact_base_flange_operation;
use super::sheet_metal::exact_edge_flange_operation;
use super::solid_primitive::exact_solid_primitive;
use super::surfaces::exact_ruled_surface_operation;
use super::surfaces::exact_surface_extend_operation;
use super::surfaces::exact_surface_offset_operation;
use super::surfaces::exact_surface_stitch_operation;
use super::thread::exact_thread_construction;
use super::work_geometry::exact_joint_origin_frame;
use super::work_geometry::exact_work_axis_construction;
use super::work_geometry::exact_work_plane_frame;
use crate::bytes::lp_ascii_filtered;
use crate::bytes::lp_utf16_bounded;
use crate::container::ContainerScan;
use crate::design::decode::assembly::exact_legacy_as_built_421_operands;
use crate::design::decode::operands::RecordFrame;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids;
use crate::ids::native_stream;
use crate::records::entity_header::DesignEntityHeader;
use crate::records::feature::assembly;
use crate::records::feature::assembly_features::DesignComponentOccurrence;
use crate::records::feature::coil;
use crate::records::feature::direct_face;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;
use crate::records::recipes::ConstructionRecipe;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

/// Decode every canonical sketch or construction-operation scope, including
/// scopes that own no parameters and therefore have no owner-frame backlink.
pub(crate) fn decode_parameter_scopes(
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
    types: &[crate::records::entity_header::SegmentType],
    parameters: &[DesignParameter],
    parameter_owners: &[crate::records::parameters::DesignParameterOwner],
    component_occurrences: &[DesignComponentOccurrence],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignParameterScope>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = ids::native_scope(&entry.name);
        let records = IndexedRecordOffsets::build(bytes);
        let stream_types = crate::design::decode::meta::stream_types_by_entity(types, &entry.name);
        let stream_scope_start = out.len();
        for header in parameter_scope_candidate_headers(bytes, &records) {
            let Some(mut scope) = parse_parameter_scope(
                bytes,
                &records,
                header.record_index,
                &header.class_tag,
                header.byte_offset,
            ) else {
                continue;
            };
            scope.id = ids::native_design_parameter_scope_id(&entry.name, scope.byte_offset());
            bind_coil_extent_from_parameters(&mut scope, parameters, parameter_owners);
            bind_hem_operation_from_parameters(bytes, &mut scope, parameters, parameter_owners);
            if design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sketch) {
                let start = usize::try_from(scope.byte_offset()).ok();
                let end = usize::try_from(scope.paired_byte_offset()).ok();
                let frame = start
                    .zip(end)
                    .and_then(|(start, end)| bytes.get(start..end));
                let mut matches = Vec::new();
                if let Some(frame) = frame {
                    // One pass over the frame: the first offset of every
                    // marked reference (a one byte, a u32 suffix, six zero
                    // bytes), then each eligible entity looks its suffix up.
                    let mut first_at: HashMap<u32, usize> = HashMap::new();
                    for at in memchr::memchr_iter(1, frame) {
                        if at + 11 <= frame.len() && frame[at + 5..at + 11] == [0; 6] {
                            if let Some(suffix) = View::u32_le_at(frame, at + 1) {
                                first_at.entry(suffix).or_insert(at);
                            }
                        }
                    }
                    for entity in entities {
                        if native_stream(&entity.id) != Some(stream.as_str())
                            || !entity.in_sketch_module()
                            || entity.entity_id.suffix() > u64::from(u32::MAX)
                        {
                            continue;
                        }
                        if let Some(at) = first_at.get(&(entity.entity_id.suffix() as u32)) {
                            matches.push((entity, at + 1));
                        }
                    }
                }
                if let [(entity, relative_offset)] = matches.as_slice() {
                    let entity_reference_offset =
                        scope.byte_offset().saturating_add(*relative_offset as u64);
                    if let scope::DesignScopePayloadMut::Sketch(slot)
                    | scope::DesignScopePayloadMut::Esquisse(slot)
                    | scope::DesignScopePayloadMut::Skizze(slot)
                    | scope::DesignScopePayloadMut::Esboco(slot) = scope.payload_mut()
                    {
                        *slot = Some(scope::DesignSketchEntityBinding {
                            entity_id: entity.entity_id.clone(),
                            entity_reference_offset,
                        });
                    }
                }
            }
            if scope.kind() == scope::DesignFeatureKind::WorkPlane {
                if let Some(frame) = exact_work_plane_frame(bytes, &records, &scope) {
                    if let scope::DesignScopePayloadMut::WorkPlane(slot) = scope.payload_mut() {
                        *slot = Some(scope::DesignWorkPlaneTransform {
                            work_plane_transform: frame.transform,
                            work_plane_transform_offset: frame.transform_offset,
                            reference: frame.reference.map(|(record_index, offset)| {
                                scope::DesignWorkPlaneReference {
                                    work_plane_reference: record_index,
                                    work_plane_reference_offset: offset,
                                }
                            }),
                            work_plane_construction: None,
                        });
                    }
                }
            }
            if let Some(construction) = exact_work_axis_construction(bytes, &records, &scope) {
                if let scope::DesignScopePayloadMut::WorkAxis(slot) = scope.payload_mut() {
                    *slot = Some(construction);
                }
            }
            if scope.kind() == scope::DesignFeatureKind::JointOrigin {
                if let Some(frame) = exact_joint_origin_frame(bytes, &records, &scope) {
                    if let scope::DesignScopePayloadMut::JointOrigin(slot) = scope.payload_mut() {
                        *slot = Some(scope::DesignJointOriginTransform {
                            joint_origin_transform: frame.transform,
                            joint_origin_transform_offset: frame.transform_offset,
                            reference: frame.reference.map(|(record_index, offset)| {
                                scope::DesignJointOriginReference {
                                    joint_origin_reference: record_index,
                                    joint_origin_reference_offset: offset,
                                }
                            }),
                        });
                    }
                }
            }
            {
                let construction =
                    exact_work_point_construction(bytes, &records, &scope, &stream_types);
                if let scope::DesignScopePayloadMut::WorkPoint(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_hole_construction(bytes, &records, &scope, &stream_types);
                if let scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            if let Some(placement) = exact_coil_placement(bytes, &records, &scope, recipes) {
                if let scope::DesignScopePayloadMut::SpirePrimitive(slot)
                | scope::DesignScopePayloadMut::CoilPrimitive(slot) = scope.payload_mut()
                {
                    slot.get_or_insert_with(Default::default).coil_placement = Some(placement);
                }
            }
            if let Some(construction) =
                exact_solid_primitive(bytes, &records, &scope, parameter_owners)
            {
                scope
                    .try_edit(|draft| {
                        draft.payload = construction.into();
                    })
                    .map_err(|error| CodecError::NotImplemented(error.to_string()))?;
            }
            {
                let construction = exact_direct_face_operation(bytes, &records, &scope);
                match (scope.payload_mut(), construction) {
                    (
                        scope::DesignScopePayloadMut::OffsetFaces(slot)
                        | scope::DesignScopePayloadMut::DecalerLesFaces(slot),
                        Some(direct_face::DesignDirectFaceOperation::OffsetFaces(value)),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::Shell(slot)
                        | scope::DesignScopePayloadMut::Schale(slot),
                        Some(direct_face::DesignDirectFaceOperation::Shell(value)),
                    ) => *slot = Some(value),
                    (
                        scope::DesignScopePayloadMut::Thicken(slot),
                        Some(direct_face::DesignDirectFaceOperation::Thicken(value)),
                    ) => *slot = Some(value),
                    _ => {}
                }
            }
            {
                let construction = exact_move_operation(bytes, &records, &scope);
                if let scope::DesignScopePayloadMut::Move(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_scale_operation(bytes, &records, &scope, &stream_types);
                if let scope::DesignScopePayloadMut::Scale(slot)
                | scope::DesignScopePayloadMut::Massstab(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            {
                let construction = exact_surface_extend_operation(bytes, &records, &scope);
                if let scope::DesignScopePayloadMut::SurfaceExtend(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_surface_offset_operation(bytes, &records, &scope);
                if let scope::DesignScopePayloadMut::SurfaceOffset(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            if let Some(parameters) = exact_fixed_extrude_parameters(
                bytes,
                &records,
                &scope,
                parameters,
                parameter_owners,
            ) {
                {
                    let value = Some(parameters);
                    if let scope::DesignScopePayloadMut::Extrude(slot)
                    | scope::DesignScopePayloadMut::Extrusion(slot)
                    | scope::DesignScopePayloadMut::Extrusao(slot) = scope.payload_mut()
                    {
                        slot.get_or_insert_with(Default::default)
                            .fixed_extrude_parameters = value;
                    }
                }
            }
            {
                let construction = exact_fixed_fillet_parameters(bytes, &records, &scope);
                if let scope::DesignScopePayloadMut::Fillet(slot)
                | scope::DesignScopePayloadMut::Conge(slot)
                | scope::DesignScopePayloadMut::Abrundung(slot)
                | scope::DesignScopePayloadMut::Arredondamento(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            {
                let construction =
                    exact_fixed_chamfer_parameters(bytes, &records, &scope, parameter_owners);
                if let scope::DesignScopePayloadMut::Chamfer(slot)
                | scope::DesignScopePayloadMut::Chanfrein(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            if let Some(construction) =
                exact_path_feature_construction(bytes, &records, &scope, parameter_owners)
            {
                scope
                    .try_edit(|draft| {
                        draft.payload = construction.into();
                    })
                    .map_err(|error| CodecError::NotImplemented(error.to_string()))?;
            }
            {
                let construction = exact_combine_operation(bytes, &records, &scope);
                if let scope::DesignScopePayloadMut::Combine(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_thread_construction(bytes, &scope);
                if let scope::DesignScopePayloadMut::Thread(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction =
                    exact_draft_operation_with_owners(bytes, &records, &scope, parameter_owners);
                if let scope::DesignScopePayloadMut::Draft(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_circular_pattern_construction_with_owners(
                    bytes,
                    &records,
                    &scope,
                    parameter_owners,
                );
                if let scope::DesignScopePayloadMut::CPattern(slot)
                | scope::DesignScopePayloadMut::CircularPattern(slot)
                | scope::DesignScopePayloadMut::ReseauC(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            {
                let construction = exact_rectangular_pattern_construction(
                    bytes,
                    &records,
                    &scope,
                    parameter_owners,
                );
                if let scope::DesignScopePayloadMut::RPattern(slot)
                | scope::DesignScopePayloadMut::RectangularPattern(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            {
                let construction =
                    exact_assembly_alignment(bytes, &records, &scope, parameter_owners);
                if let scope::DesignScopePayloadMut::Assemble(slot)
                | scope::DesignScopePayloadMut::AsBuilt(slot) = scope.payload_mut()
                {
                    *slot = construction;
                }
            }
            let legacy_form = scope.assembly_alignment().and_then(|alignment| {
                let assembly::DesignAssemblyAlignmentForm::SolvedOnly {
                    solved_frame,
                    limits,
                } = alignment.form.as_ref()?
                else {
                    return None;
                };
                let carriers = exact_legacy_as_built_421_operands(
                    bytes,
                    &records,
                    &scope,
                    &stream_types,
                    recipes,
                    solved_frame,
                )?;
                Some(assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                    carriers,
                    solved_frame: solved_frame.clone(),
                    limits: limits.clone(),
                    frames_field_present: true,
                })
            });
            if let (Some(alignment), Some(form)) = (scope.assembly_alignment_mut(), legacy_form) {
                alignment.form = Some(form);
            }
            {
                let construction = exact_component_insert_construction(bytes, &records, &scope);
                if let scope::DesignScopePayloadMut::ComponentInsert(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_derived_instance_construction(
                    bytes,
                    &records,
                    &scope,
                    component_occurrences,
                );
                if let scope::DesignScopePayloadMut::DerivedInstance(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_copy_paste_component_operation(
                    bytes,
                    &records,
                    &scope,
                    component_occurrences,
                );
                if let scope::DesignScopePayloadMut::CopyPaste(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            bind_component_pattern_occurrences(&mut scope, component_occurrences);
            {
                let construction = exact_copy_paste_bodies_operation(bytes, &records, &scope);
                if let scope::DesignScopePayloadMut::CopyPasteBodies(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            {
                let construction = exact_base_feature_construction(bytes, &scope);
                if let scope::DesignScopePayloadMut::BaseFeature(slot) = scope.payload_mut() {
                    *slot = construction;
                }
            }
            out.push(scope);
        }
        bind_joint_origin_frames_from_assemblies(bytes, &mut out[stream_scope_start..]);
        bind_axial_assembly_operand_targets(bytes, &records, &mut out[stream_scope_start..]);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    Ok(out)
}

/// Admit one envelope for every logical scope identity.
///
/// Some Design streams retain more than one complete envelope for one record
/// index. A history-bound envelope is authoritative when exactly one
/// candidate resolves to a unique ASM state transition; an unresolved group
/// remains an error so a duplicate cannot be selected by byte order.
pub(crate) fn admit_history_bound_scope_variants(
    scopes: &mut Vec<DesignParameterScope>,
    histories: &[crate::history_records::AsmHistory],
) -> Result<(), CodecError> {
    let mut groups = HashMap::<(String, u32), Vec<usize>>::new();
    for (index, scope) in scopes.iter().enumerate() {
        let stream = native_stream(&scope.id)
            .unwrap_or(ids::DEFAULT_STREAM)
            .to_owned();
        groups
            .entry((stream, scope.record_index))
            .or_default()
            .push(index);
    }

    let mut admitted =
        cadmpeg_core::decode::alloc_filled(scopes.len(), true, "f3d scope admission")?;
    for indices in groups.values() {
        let [first, following @ ..] = indices.as_slice() else {
            continue;
        };
        if following.is_empty() {
            continue;
        }
        let history_bound = indices
            .iter()
            .copied()
            .filter(|index| {
                let scope = &scopes[*index];
                let Some(state_id) = scope.history_state_id() else {
                    return false;
                };
                let Some(previous_state_id) =
                    crate::history::effective_scope_previous_history_state_id(scope, histories)
                else {
                    return false;
                };
                crate::history::unique_history_state_pair(histories, state_id, previous_state_id)
                    .is_some()
            })
            .collect::<Vec<_>>();
        let equivalent_payload = following
            .iter()
            .all(|index| equivalent_scope_variant_payload(&scopes[*first], &scopes[*index]));
        let keep = match history_bound.as_slice() {
            [keep] => *keep,
            [] if equivalent_payload => following.iter().copied().fold(*first, |keep, index| {
                if scopes[index].byte_offset() >= scopes[keep].byte_offset() {
                    index
                } else {
                    keep
                }
            }),
            _ => {
                return Err(CodecError::Malformed(
                    "Design scope record identity has unresolved duplicate envelopes".into(),
                ));
            }
        };
        for index in indices {
            admitted[*index] = *index == keep;
        }
    }

    let retained = std::mem::take(scopes)
        .into_iter()
        .enumerate()
        .filter_map(|(index, scope)| admitted[index].then_some(scope))
        .collect();
    *scopes = retained;
    Ok(())
}

/// Compare two same-index scope envelopes after removing source-location and
/// dynamic-class fields. An equivalent envelope is one serialization of the
/// same logical scope; the later envelope supersedes the earlier one when no
/// decoded ASM state pair can select a revision.
fn equivalent_scope_variant_payload(
    left: &DesignParameterScope,
    right: &DesignParameterScope,
) -> bool {
    let (Ok(mut left), Ok(mut right)) = (serde_json::to_value(left), serde_json::to_value(right))
    else {
        return false;
    };
    strip_scope_variant_provenance(&mut left, true);
    strip_scope_variant_provenance(&mut right, true);
    left == right
}

fn strip_scope_variant_provenance(value: &mut serde_json::Value, top_level: bool) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                strip_scope_variant_provenance(item, false);
            }
        }
        serde_json::Value::Object(fields) => {
            fields.retain(|key, _| {
                if key.ends_with("_offset") || key.ends_with("_offsets") {
                    return false;
                }
                if top_level
                    && matches!(
                        key.as_str(),
                        "id" | "class_tag"
                            | "frame_length"
                            | "history_state_id"
                            | "previous_history_state_id"
                            | "paired_class_tag"
                    )
                {
                    return false;
                }
                true
            });
            for field in fields.values_mut() {
                strip_scope_variant_provenance(field, false);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

/// Skip the payload prologue at `at`: a leading-block presence byte, a property
/// presence byte, and the property block that byte gates. The leading-block
/// byte belongs to classes that write one, so this reader only steps over it.
pub(in crate::design::decode) fn payload_prologue(
    bytes: &[u8],
    at: usize,
    end: usize,
) -> Option<usize> {
    let mut cursor = at.checked_add(1)?;
    let present = *bytes.get(cursor)?;
    cursor += 1;
    match present {
        0 => Some(cursor),
        1 => {
            let count = View::u32_le_at(bytes, cursor)?;
            if count > 16 {
                return None;
            }
            cursor += 4;
            for _ in 0..count {
                let (_key, after_key) =
                    lp_ascii_filtered(bytes, cursor, 1..=64, u8::is_ascii_graphic)?;
                let (type_name, after_type) =
                    lp_ascii_filtered(bytes, after_key, 1..=64, u8::is_ascii_graphic)?;
                if type_name != "IntrinsicMetaTypeuint64" {
                    return None;
                }
                cursor = after_type.checked_add(8)?;
            }
            (cursor <= end).then_some(cursor)
        }
        _ => None,
    }
}

/// Every indexed-record header that can open a parameter scope: a scope is
/// delimited by two headers carrying its record index, so the last header of an
/// index opens nothing.
pub(super) fn parameter_scope_candidate_headers(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
) -> Vec<RecordFrame> {
    records
        .records()
        .flat_map(|(record_index, offsets)| {
            offsets[..offsets.len().saturating_sub(1)]
                .iter()
                .filter_map(move |at| {
                    let (class_tag, _) =
                        lp_ascii_filtered(bytes, *at, 0..=2000, u8::is_ascii_graphic)?;
                    Some(RecordFrame {
                        record_index,
                        class_tag: class_tag.try_into().ok()?,
                        byte_offset: *at as u64,
                    })
                })
        })
        .collect()
}

pub(crate) fn parameter_scope_tail_length_is_valid(
    kind: impl AsRef<str>,
    tail_length: usize,
) -> bool {
    let kind = kind.as_ref();
    if (80..=590).contains(&tail_length) && tail_length.is_multiple_of(2) {
        return true;
    }
    match kind {
        "CopyPasteBodies" => tail_length == 110,
        "CoilPrimitive" => matches!(tail_length, 72 | 76 | 77 | 78 | 87 | 88),
        _ => matches!(tail_length, 72 | 76 | 77 | 78 | 87),
    }
}

pub(crate) fn parameter_scope_previous_history_offset(
    kind: impl AsRef<str>,
    tail_length: usize,
) -> Option<usize> {
    parameter_scope_previous_history_offset_for_form(
        kind.as_ref(),
        tail_length,
        ScopeTailForm::Fixed,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeTailForm {
    Fixed,
    Named,
}

fn parameter_scope_previous_history_offset_for_form(
    kind: &str,
    tail_length: usize,
    tail_form: ScopeTailForm,
) -> Option<usize> {
    if tail_form == ScopeTailForm::Named {
        return None;
    }
    match (kind, tail_length) {
        ("CopyPasteBodies", 110) => Some(53),
        ("CoilPrimitive", 88) => None,
        (_, 72 | 76) => Some(30),
        (_, 77 | 78) => Some(31),
        (_, 87) => Some(41),
        _ => None,
    }
}

pub(in crate::design::decode) fn parse_parameter_scope(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class_tag: &crate::records::references::DesignClassTag,
    byte_offset: u64,
) -> Option<DesignParameterScope> {
    let start = usize::try_from(byte_offset).ok()?;
    let paired_at = records.first_at_or_after(start.checked_add(11)?, record_index)?;
    let (paired_class_tag, _) =
        lp_ascii_filtered(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    let mut candidates = Vec::new();
    let kind_scan_start = paired_at
        .saturating_sub(590 + 4 + 2 * 256)
        .max(start.checked_add(11)?);
    let kind_scan_end = paired_at.checked_sub(72)?;
    for at in kind_scan_start..kind_scan_end {
        let Some((kind, kind_end)) = lp_utf16_bounded(bytes, at, 1..=256) else {
            continue;
        };
        if !kind.chars().all(|character| !character.is_control()) {
            continue;
        }
        let Some(tail_length) = paired_at.checked_sub(kind_end) else {
            continue;
        };
        let fixed_tail = matches!(tail_length, 72 | 76 | 77 | 78 | 82 | 87 | 88 | 104 | 110);
        if fixed_tail && parameter_scope_tail_length_is_valid(&kind, tail_length) {
            candidates.push((
                at,
                kind_end,
                tail_length,
                kind.clone(),
                ScopeTailForm::Fixed,
            ));
        }
        let named_tail = (78..=590).contains(&tail_length)
            && tail_length.is_multiple_of(2)
            && (parameter_scope_tail_length_is_valid(&kind, tail_length) || tail_length == 78)
            && named_parameter_scope_tail_is_valid(bytes, kind_end, paired_at, tail_length)
                .is_some_and(|valid| valid);
        if named_tail {
            candidates.push((at, kind_end, tail_length, kind, ScopeTailForm::Named));
        }
    }
    if candidates
        .iter()
        .filter(|candidate| candidate.4 == ScopeTailForm::Named)
        .count()
        == 1
    {
        candidates.retain(|candidate| candidate.4 == ScopeTailForm::Named);
    }
    let [(kind_at, kind_end, tail_length, kind, tail_form)] = candidates.as_slice() else {
        return None;
    };
    let kind_text = kind.clone();
    let kind = scope::DesignFeatureKind::try_from(kind_text.clone()).ok()?;
    let kind_end = *kind_end;
    let reference_table_end = kind_at.checked_sub(4)?;
    let feature_ordinal = std::num::NonZeroU32::new(View::u32_le_at(bytes, kind_end)?)?;
    let history_state_id_offset = reference_table_end;
    let history_state_id = match View::u32_le_at(bytes, history_state_id_offset)? {
        u32::MAX => None,
        state_id => Some(i64::from(state_id)),
    };
    let previous_history_state_id_offset = match parameter_scope_previous_history_offset_for_form(
        &kind_text,
        *tail_length,
        *tail_form,
    ) {
        Some(offset) => Some(kind_end.checked_add(offset)?),
        None => None,
    };
    let previous_history_state_id =
        previous_history_state_id_offset.and_then(|offset| match View::u32_le_at(bytes, offset)? {
            u32::MAX => None,
            state_id => Some(i64::from(state_id)),
        });
    let mut reference_tables = Vec::new();
    for count_at in start + 11..reference_table_end {
        let count = usize::try_from(View::u32_le_at(bytes, count_at)?).ok()?;
        if count == 0
            || count_at
                .checked_add(4)?
                .checked_add(count.checked_mul(11)?)?
                != reference_table_end
        {
            continue;
        }
        let first = count_at.checked_add(4)?;
        let mut members = Vec::with_capacity(count);
        let mut offsets = Vec::with_capacity(count);
        for ordinal in 0..count {
            let marker = first.checked_add(ordinal.checked_mul(11)?)?;
            if bytes.get(marker) != Some(&1) || bytes.get(marker + 5..marker + 11)? != [0; 6] {
                members.clear();
                break;
            }
            members.push(View::u32_le_at(bytes, marker + 1)?);
            offsets.push(u64::try_from(marker + 1).ok()?);
        }
        if members.len() == count {
            reference_tables.push((count_at, members, offsets));
        }
    }
    let [(reference_count_at, reference_members, reference_member_offsets)] =
        reference_tables.as_slice()
    else {
        return None;
    };
    let surface_stitch_operation = if kind == scope::DesignFeatureKind::SurfaceStitch {
        exact_surface_stitch_operation(bytes, records, record_index, reference_members)
    } else {
        None
    };
    let surface_patch_boundaries = if kind == scope::DesignFeatureKind::SurfacePatch {
        crate::design::decode::patch::surface_patch_boundaries(bytes, records, reference_members)
    } else {
        Vec::new()
    };
    let base_flange_operation = if kind == scope::DesignFeatureKind::BaseFlange {
        exact_base_flange_operation(bytes, start, paired_at, reference_members)
    } else {
        None
    };
    let edge_flange_operation = if kind == scope::DesignFeatureKind::EdgeFlange {
        exact_edge_flange_operation(
            bytes,
            start,
            paired_at,
            class_tag.as_str(),
            &paired_class_tag,
            reference_members,
        )
    } else {
        None
    };
    let ruled_surface_operation = if kind == scope::DesignFeatureKind::SurfaceRuled {
        exact_ruled_surface_operation(
            bytes,
            start,
            paired_at,
            *reference_count_at,
            reference_members,
        )
    } else {
        None
    };
    let family = design_feature_family(&kind);
    // A `Sketch` scope carries either the single entity-suffix reference form
    // or, when the stream's sketch entity headers use the `EntityGenesis`
    // form, the generic ordered reference table. Both parse here; the entity
    // binding in `decode_parameter_scopes` requires a unique suffix match.
    let extrude_prologue = if family == Some(DesignFeatureFamily::Extrude) {
        // The generic scope envelope is independently self-delimiting. An
        // unrecognized Extrude prologue therefore withholds only the typed
        // fields, not the scope and its ordered reference table.
        exact_extrude_prologue(
            bytes,
            start,
            paired_at,
            class_tag.as_str(),
            &paired_class_tag,
            *reference_count_at,
            reference_members,
        )
    } else {
        None
    };
    let coil_discriminators = if family == Some(DesignFeatureFamily::Coil) {
        exact_coil_discriminators(bytes, start, paired_at, &kind, reference_members)
    } else {
        None
    };
    let coil_transform = if family == Some(DesignFeatureFamily::Coil) {
        exact_long_coil_transform(bytes, start, paired_at, &kind, reference_members)
    } else {
        None
    };
    let coil = if family == Some(DesignFeatureFamily::Coil) {
        Some(coil::DesignCoilScope {
            coil_operation: coil_discriminators.as_ref().map(|fields| {
                crate::records::identity::RecordedValue {
                    value: fields.operation,
                    offset: fields.operation_offset,
                }
            }),
            coil_extent: coil_discriminators
                .as_ref()
                .and_then(|fields| fields.extent),
            coil_section: coil_discriminators
                .as_ref()
                .map(|fields| match fields.section_offset {
                    Some(offset) => crate::records::identity::MaybeRecordedValue::Located(
                        crate::records::identity::RecordedValue {
                            value: fields.section,
                            offset,
                        },
                    ),
                    None => crate::records::identity::MaybeRecordedValue::Unlocated(fields.section),
                }),
            coil_section_placement: coil_discriminators.as_ref().map(|fields| {
                match fields.section_placement_offset {
                    Some(offset) => crate::records::identity::MaybeRecordedValue::Located(
                        crate::records::identity::RecordedValue {
                            value: fields.section_placement,
                            offset,
                        },
                    ),
                    None => crate::records::identity::MaybeRecordedValue::Unlocated(
                        fields.section_placement,
                    ),
                }
            }),
            coil_clockwise: coil_discriminators.as_ref().map(|fields| {
                match fields.clockwise_offset {
                    Some(offset) => crate::records::identity::MaybeRecordedValue::Located(
                        crate::records::identity::RecordedValue {
                            value: fields.clockwise,
                            offset,
                        },
                    ),
                    None => {
                        crate::records::identity::MaybeRecordedValue::Unlocated(fields.clockwise)
                    }
                }
            }),
            coil_placement: None,
            coil_transform,
        })
    } else {
        None
    };
    let mut scope = DesignParameterScope::try_new(scope::DesignParameterScopeDraft {
        id: String::new(),
        byte_offset,
        class_tag: class_tag.clone(),
        record_index,
        frame_length: u64::try_from(paired_at.checked_sub(start)?).ok()?,
        kind_offset: u64::try_from(kind_at.checked_add(4)?).ok()?,
        feature_ordinal,
        feature_ordinal_offset: u64::try_from(kind_end).ok()?,
        history_state_id,

        previous_history_state_id,
        previous_history_state_id_offset: previous_history_state_id_offset
            .and_then(|offset| u64::try_from(offset).ok())
            .filter(|&offset| offset != 0),
        reference_count_offset: u64::try_from(*reference_count_at).ok()?,
        reference_members: crate::records::identity::ReferenceRun::located(
            reference_members
                .iter()
                .copied()
                .zip(reference_member_offsets.iter().copied())
                .map(|(value, offset)| crate::records::identity::Located { value, offset })
                .collect(),
        ),
        payload: match kind {
            scope::DesignFeatureKind::SurfaceStitch => {
                scope::DesignScopePayload::SurfaceStitch(surface_stitch_operation?)
            }
            scope::DesignFeatureKind::SurfaceRuled => {
                scope::DesignScopePayload::SurfaceRuled(ruled_surface_operation?)
            }
            kind => kind.try_into().ok()?,
        },
        unclosed_construction_operand_groups: Vec::new(),
        paired_class_tag: paired_class_tag.try_into().ok()?,
        paired_byte_offset: paired_at as u64,
    })
    .ok()?;
    if let Some(prologue) = extrude_prologue {
        {
            let construction = Some(scope::DesignExtrudeScope {
                extrude_prologue: Some(prologue),
                ..scope::DesignExtrudeScope::default()
            });
            if let scope::DesignScopePayloadMut::Extrude(slot)
            | scope::DesignScopePayloadMut::Extrusion(slot)
            | scope::DesignScopePayloadMut::Extrusao(slot) = scope.payload_mut()
            {
                *slot = construction;
            }
        }
    }
    if let Some(coil) = coil {
        {
            let construction = Some(coil);
            if let scope::DesignScopePayloadMut::SpirePrimitive(slot)
            | scope::DesignScopePayloadMut::CoilPrimitive(slot) = scope.payload_mut()
            {
                *slot = construction;
            }
        }
    }
    if !surface_patch_boundaries.is_empty() {
        {
            let construction = surface_patch_boundaries;
            if let scope::DesignScopePayloadMut::SurfacePatch(slot) = scope.payload_mut() {
                *slot = construction;
            }
        }
    }
    if let Some(operation) = base_flange_operation {
        {
            let construction = Some(scope::DesignBaseFlangeScope {
                base_flange_operation: Some(operation),
                ..scope::DesignBaseFlangeScope::default()
            });
            if let scope::DesignScopePayloadMut::BaseFlange(slot) = scope.payload_mut() {
                *slot = construction;
            }
        }
    }
    if let Some(operation) = edge_flange_operation {
        {
            let construction = Some(operation);
            if let scope::DesignScopePayloadMut::EdgeFlange(slot) = scope.payload_mut() {
                *slot = construction;
            }
        }
    }
    Some(scope)
}

fn named_parameter_scope_tail_is_valid(
    bytes: &[u8],
    kind_end: usize,
    paired_at: usize,
    tail_length: usize,
) -> Option<bool> {
    let label_at = kind_end.checked_add(8)?;
    let (label, label_end) = lp_utf16_bounded(bytes, label_at, 0..=256)?;
    let label_code_units = label.encode_utf16().count();
    if tail_length != 78usize.checked_add(label_code_units.checked_mul(2)?)?
        || label_end.checked_add(7)? != kind_end.checked_add(19 + label_code_units * 2)?
        || label.chars().any(char::is_control)
    {
        return Some(false);
    }
    let marker = kind_end.checked_add(19 + label_code_units.checked_mul(2)?)?;
    if marker.checked_add(59)? != paired_at || bytes.get(label_end..marker)? != [0; 7] {
        return Some(false);
    }
    let first_lane_value = View::u64_le_at(bytes, marker + 2)?;
    let second_lane_value = View::u64_le_at(bytes, marker + 34)?;
    let third_lane_value = View::u64_le_at(bytes, marker + 48)?;
    Some(
        bytes.get(kind_end + 4..kind_end + 8)? == [0; 4]
            && bytes.get(marker) == Some(&1)
            && bytes.get(marker + 1).is_some_and(|field_id| *field_id != 0)
            && matches!(first_lane_value, 0 | 1)
            && second_lane_value == first_lane_value
            && third_lane_value == first_lane_value
            && bytes.get(marker + 10..marker + 12)? == [0; 2]
            && View::u32_le_at(bytes, marker + 12)? > 0
            && View::u32_le_at(bytes, marker + 16)? == 0xfc
            && View::f64_le_at(bytes, marker + 20)?.is_finite()
            && View::u32_le_at(bytes, marker + 28)? == 0xfc
            && bytes.get(marker + 32) == Some(&1)
            && bytes
                .get(marker + 33)
                .is_some_and(|field_id| *field_id != 0)
            && bytes.get(marker + 42..marker + 46)? == [0, 1, 0, 0]
            && bytes.get(marker + 46) == Some(&1)
            && bytes
                .get(marker + 47)
                .is_some_and(|field_id| *field_id != 0)
            && bytes.get(marker + 56..marker + 59)? == [0; 3],
    )
}

pub(crate) fn parameter_scope_payload_length(scope: &DesignParameterScope) -> Option<u64> {
    let kind_bytes = u64::try_from(scope.kind_name().encode_utf16().count())
        .ok()?
        .checked_mul(2)?;
    scope.frame_length().checked_sub(kind_bytes)
}

#[cfg(test)]
mod tests;
