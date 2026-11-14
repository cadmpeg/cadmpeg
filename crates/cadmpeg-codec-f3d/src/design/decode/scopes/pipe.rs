// SPDX-License-Identifier: Apache-2.0
//! Exact source framing and complete native Pipe binding.

use super::point_data::exact_point_data_construction;
use super::shared_frames::{exact_fixed_scalar, extrude_operation_at, marked_record_reference};
use crate::container::ContainerScan;
use crate::design::decode::meta::{
    design_primary_frames, metadata_for_bulk_stream, DesignPrimaryFrame,
};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::ids::design_segment;
use crate::layout::fixed_pipe_operation_prefix as compact;
use crate::layout::legacy_pipe_operation_prefix as legacy;
use crate::layout::pipe_section_frame as section_frame;
use crate::records::decal::DesignRecordHeader;
use crate::records::entity_header::SegmentType;
use crate::records::feature::path_features::pipe::{self, PipeIndex};
use crate::records::feature::path_features::{
    DesignPipeConstruction, DesignPipePointSelection, DesignPipeSection,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::feature::work_geometry::DesignWorkPointConstruction;
use crate::records::parameters::DesignParameterOwner;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

/// Native records a Pipe binding joins.
pub(crate) struct PipeSources<'a> {
    pub(crate) types: &'a [SegmentType],
    pub(crate) headers: &'a [DesignRecordHeader],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct PipeObservation {
    pub(crate) prefix: Option<PipePrefix>,
    pub(crate) owners: [Vec<u32>; 4],
    pub(crate) points: Vec<DesignWorkPointConstruction>,
    pub(crate) point_records: Vec<u32>,
    pub(crate) prefix_references: Vec<(u32, u64)>,
    pub(crate) section: Option<DesignPipeSection>,
}

pub(crate) fn observe_sources(
    scan: &ContainerScan,
    sources: &PipeSources<'_>,
    scopes: &[&DesignParameterScope],
) -> Result<HashMap<String, Option<PipeObservation>>, CodecError> {
    let mut observations = HashMap::new();
    if scopes.is_empty() {
        return Ok(observations);
    }
    let index = PipeIndex::new(
        sources.types,
        sources.headers,
        sources.owners,
        sources.groups,
    );
    for entry in scan.entries.iter().filter(|entry| {
        scan.is_design_stream(entry, ContainerRole::Bulkstream)
            && scopes.iter().any(|scope| {
                crate::ids::native_stream(&scope.id)
                    == Some(crate::ids::native_scope(&entry.name).as_str())
            })
    }) {
        let stream = crate::ids::native_scope(&entry.name);
        let Some(meta) = metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Ok(frames) = design_primary_frames(bytes, &meta) else {
            continue;
        };
        let mut indexed = HashMap::new();
        for frame in &frames {
            indexed
                .entry(frame.entity_id)
                .and_modify(|slot| *slot = None)
                .or_insert(Some(frame));
        }
        let records = IndexedRecordOffsets::build(bytes);
        for scope in scopes
            .iter()
            .copied()
            .filter(|scope| crate::ids::native_stream(&scope.id) == Some(stream.as_str()))
        {
            let observation = observe_scope(bytes, scope, &indexed, &records, &index);
            observations
                .entry(scope.id.clone())
                .and_modify(|slot| *slot = None)
                .or_insert(observation);
        }
    }
    Ok(observations)
}

fn observe_scope(
    bytes: &[u8],
    scope: &DesignParameterScope,
    frames: &HashMap<u64, Option<&DesignPrimaryFrame<'_>>>,
    records: &IndexedRecordOffsets,
    index: &PipeIndex<'_>,
) -> Option<PipeObservation> {
    let frame = |id: u32| frames.get(&u64::from(id)).copied().flatten();
    let own = frame(scope.record_index)?;
    if !pipe::is_pipe(own.design_type)
        || u64_from_index(own.start) != scope.byte_offset()
        || u64_from_index(own.member_end) != scope.paired_byte_offset()
    {
        return None;
    }
    let table = usize::try_from(scope.reference_count_offset()).ok()?;
    if table < own.start || table > own.member_end {
        return None;
    }
    let segment = design_segment(&scope.id)?;
    let mut observation = PipeObservation {
        prefix: pipe_prefix(bytes.get(..table)?, own.start),
        owners: std::array::from_fn(|_| Vec::new()),
        points: Vec::new(),
        point_records: Vec::new(),
        prefix_references: Vec::new(),
        section: None,
    };
    for id in scope.reference_members().values().copied() {
        let Some(selected) = frame(id) else {
            continue;
        };
        if pipe::is_scalar(selected.design_type) {
            let Some(scalar) = exact_fixed_scalar(bytes, records, id) else {
                continue;
            };
            let Some(owner) = index.owner(segment, id) else {
                continue;
            };
            let ordinal = usize::from(scalar.ordinal);
            if ordinal < 4
                && scalar.owner_record_index == Some(scope.record_index)
                && scalar.start == u64_from_index(selected.start)
                && scalar.end == u64_from_index(selected.member_end)
                && scalar.value.get().to_bits() == owner.evaluated_value().get().to_bits()
                && scalar.value_offset == owner.evaluated_value_offset()
                && owner.byte_offset() == scalar.start
                && owner.byte_offset().checked_add(owner.frame_length()) == Some(scalar.end)
                && owner.local_ordinal() == u32::from(scalar.ordinal)
                && owner.scope_record_index() == scope.record_index
            {
                observation.owners[ordinal].push(id);
            }
        } else if pipe::is_point(selected.design_type) {
            observation.point_records.push(id);
            let types = HashMap::from([(
                u64::from(id),
                (
                    selected.design_type.type_guid.as_str(),
                    selected.design_type.version,
                ),
            )]);
            if let Some(point) =
                exact_point_data_construction(bytes, records, std::iter::once(&id), &types).filter(
                    |point| {
                        point.point_record_byte_offset == u64_from_index(selected.start)
                            && records.frames(id).any(|(start, end)| {
                                start == selected.start && end == selected.member_end
                            })
                    },
                )
            {
                observation.points.push(point);
            }
        }
    }
    if let Some(start) = observation.prefix.as_ref().and_then(|prefix| {
        usize::try_from(prefix.opaque_post_operation_offset)
            .ok()?
            .checked_add(2)
    }) {
        for at in start..table {
            if at.checked_add(11).is_some_and(|end| end <= table) {
                if let Some(id) = marked_record_reference(bytes, at) {
                    observation
                        .prefix_references
                        .push((id, u64_from_index(at + 1)));
                }
            }
        }
    }
    observation.section = observe_section(bytes, table, scope, frames, index, &observation);
    Some(observation)
}

fn observe_section(
    bytes: &[u8],
    table: usize,
    scope: &DesignParameterScope,
    frames: &HashMap<u64, Option<&DesignPrimaryFrame<'_>>>,
    index: &PipeIndex<'_>,
    observation: &PipeObservation,
) -> Option<DesignPipeSection> {
    if !observation.point_records.is_empty() {
        return None;
    }
    let [Some(forward), Some(reverse), Some(size), Some(thickness)] =
        std::array::from_fn(|ordinal| match observation.owners[ordinal].as_slice() {
            [id] => Some(*id),
            _ => None,
        })
    else {
        return None;
    };
    let owners = [forward, reverse, size, thickness];
    let settings = scope
        .reference_members()
        .values()
        .copied()
        .filter(|id| {
            frames
                .get(&u64::from(*id))
                .copied()
                .flatten()
                .is_some_and(|frame| pipe::is_settings(frame.design_type))
        })
        .collect::<Vec<_>>();
    let [settings] = settings.as_slice() else {
        return None;
    };
    let groups = index
        .groups(design_segment(&scope.id)?, scope.record_index)
        .collect::<Vec<_>>();
    let paths = groups
        .iter()
        .filter(|group| group.role() == DesignOperandRole::ROLE_0X5)
        .collect::<Vec<_>>();
    let [path] = paths.as_slice() else {
        return None;
    };
    let mut claimed = HashSet::from(owners);
    claimed.insert(*settings);
    for group in &groups {
        claimed.insert(group.record_index);
        claimed.extend(group.members().iter().map(|member| member.value));
    }
    let actual = scope
        .reference_members()
        .values()
        .copied()
        .collect::<HashSet<_>>();
    if actual.len() != scope.reference_members().len() || actual != claimed {
        return None;
    }
    section_frame_shape(
        bytes,
        table,
        observation.prefix.as_ref()?.opaque_post_operation_offset,
        owners,
        *settings,
        path.record_index,
    )
}

/// Bind every Pipe scope whose source framing and complete native relation
/// are exact. Other Pipe scopes keep no construction.
pub(crate) fn bind_constructions(
    scan: &ContainerScan,
    sources: &PipeSources<'_>,
    scopes: &mut [DesignParameterScope],
) -> Result<(), CodecError> {
    if !scopes
        .iter()
        .any(|scope| scope.kind() == DesignFeatureKind::Pipe)
    {
        return Ok(());
    }
    let index = PipeIndex::new(
        sources.types,
        sources.headers,
        sources.owners,
        sources.groups,
    );
    let mut bindings = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let stream = crate::ids::native_scope(&entry.name);
        let Some(meta) = metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Ok(frames) = design_primary_frames(bytes, &meta) else {
            continue;
        };
        // A record repeated in the primary index has no unique frame.
        let mut indexed = HashMap::new();
        for frame in &frames {
            indexed
                .entry(frame.entity_id)
                .and_modify(|slot| *slot = None)
                .or_insert(Some(frame));
        }
        let records = IndexedRecordOffsets::build(bytes);
        for (ordinal, scope) in scopes.iter().enumerate().filter(|(_, scope)| {
            scope.kind() == DesignFeatureKind::Pipe
                && crate::ids::native_stream(&scope.id) == Some(stream.as_str())
        }) {
            if let Some(construction) = read_construction(bytes, scope, &indexed, &records, &index)
            {
                bindings.push((ordinal, construction));
            }
        }
    }
    for (ordinal, construction) in bindings {
        if let DesignScopePayloadMut::Pipe(slot) = scopes[ordinal].payload_mut() {
            *slot = Some(construction);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct PipePrefix {
    pub(crate) operation: Option<crate::records::feature::extrude::DesignExtrudeOperation>,
    pub(crate) operation_value: Option<u32>,
    pub(crate) operation_offset: u64,
    pub(crate) is_hollow_value: Option<u8>,
    pub(crate) is_hollow_offset: u64,
    pub(crate) opaque_post_operation: Option<cadmpeg_ir::native::bytes::NativeBytes<[u8; 2]>>,
    pub(crate) opaque_post_operation_offset: u64,
}

pub(crate) fn pipe_prefix(bytes: &[u8], start: usize) -> Option<PipePrefix> {
    let frame = bytes.get(start..)?;
    let (hollow_at, operation_at, opaque_at) =
        if frame.get(compact::ZERO_RUN_10..compact::IS_HOLLOW) == Some(&[0; 10]) {
            (
                compact::IS_HOLLOW,
                compact::OPERATION,
                compact::OPAQUE_POST_OPERATION,
            )
        } else if frame.get(legacy::ZERO_RUN_9..legacy::PREFIX_MARKER) == Some(&[0; 9])
            && frame.get(legacy::PREFIX_MARKER) == Some(&legacy::PREFIX_MARKER_VALUE)
            && frame.get(legacy::ZERO_RUN_4..legacy::IS_HOLLOW) == Some(&[0; 4])
        {
            (
                legacy::IS_HOLLOW,
                legacy::OPERATION,
                legacy::OPAQUE_POST_OPERATION,
            )
        } else {
            return None;
        };
    let at = |offset: usize| u64::try_from(start.checked_add(offset)?).ok();
    let opaque: Option<[u8; 2]> = frame
        .get(opaque_at..opaque_at.checked_add(2)?)
        .and_then(|bytes| bytes.try_into().ok());
    Some(PipePrefix {
        operation: extrude_operation_at(frame, operation_at),
        operation_value: cadmpeg_core::decode::View::u32_le_at(frame, operation_at),
        operation_offset: at(operation_at)?,
        is_hollow_value: frame.get(hollow_at).copied(),
        is_hollow_offset: at(hollow_at)?,
        opaque_post_operation: opaque.map(cadmpeg_ir::native::bytes::NativeBytes::new),
        opaque_post_operation_offset: at(opaque_at)?,
    })
}

fn exact_pipe_prefix(bytes: &[u8], start: usize) -> Option<DesignPipeConstruction> {
    let prefix = pipe_prefix(bytes, start)?;
    let is_hollow = match prefix.is_hollow_value? {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some(DesignPipeConstruction {
        operation: prefix.operation?,
        operation_offset: prefix.operation_offset,
        is_hollow,
        is_hollow_offset: prefix.is_hollow_offset,
        opaque_post_operation: prefix.opaque_post_operation?,
        opaque_post_operation_offset: prefix.opaque_post_operation_offset,
        record_indexes: [0; 4],
        point_selection: None,
        section: None,
    })
}

fn read_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
    frames: &HashMap<u64, Option<&DesignPrimaryFrame<'_>>>,
    records: &IndexedRecordOffsets,
    index: &PipeIndex<'_>,
) -> Option<DesignPipeConstruction> {
    let frame = |id: u32| frames.get(&u64::from(id)).copied().flatten();
    let own = frame(scope.record_index)?;
    if !pipe::is_pipe(own.design_type)
        || u64_from_index(own.start) != scope.byte_offset()
        || u64_from_index(own.member_end) != scope.paired_byte_offset()
        || own.class_tag != scope.class_tag
    {
        return None;
    }
    let segment = design_segment(&scope.id)?;
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let table = usize::try_from(scope.reference_count_offset()).ok()?;
    let mut construction = exact_pipe_prefix(bytes.get(..table)?, start)?;
    let mut lanes = 0_usize;
    let mut point = None;
    for id in scope.reference_members().values().copied() {
        let selected = frame(id)?;
        if pipe::is_scalar(selected.design_type) {
            let scalar = exact_fixed_scalar(bytes, records, id)?;
            let owner = index.owner(segment, id)?;
            if lanes >= 4
                || usize::from(scalar.ordinal) != lanes
                || scalar.owner_record_index != Some(scope.record_index)
                || scalar.start != u64_from_index(selected.start)
                || scalar.end != u64_from_index(selected.member_end)
                || scalar.value.get().to_bits() != owner.evaluated_value().get().to_bits()
                || scalar.value_offset != owner.evaluated_value_offset()
                || owner.byte_offset() != scalar.start
                || owner.byte_offset().checked_add(owner.frame_length()) != Some(scalar.end)
                || owner.class_tag() != &selected.class_tag
                || owner.local_ordinal() != u32::from(scalar.ordinal)
                || owner.scope_record_index() != scope.record_index
            {
                return None;
            }
            construction.record_indexes[lanes] = id;
            lanes += 1;
        } else if pipe::is_point(selected.design_type) && point.replace((id, selected)).is_some() {
            return None;
        }
    }
    if lanes != 4 {
        return None;
    }
    let mut paths = Vec::new();
    for group in index.groups(segment, scope.record_index) {
        let selected = frame(group.record_index)?;
        if u64_from_index(selected.start) != group.byte_offset
            || u64_from_index(selected.member_end) != group.paired_byte_offset
            || selected.class_tag != group.class_tag
        {
            return None;
        }
        if group.role() == DesignOperandRole::ROLE_0X5 {
            paths.push(group.record_index);
        }
    }
    let settings = scope
        .reference_members()
        .values()
        .copied()
        .filter(|id| frame(*id).is_some_and(|selected| pipe::is_settings(selected.design_type)))
        .collect::<Vec<_>>();
    if let (None, [settings], [path]) = (point, settings.as_slice(), paths.as_slice()) {
        construction.section = section_frame_shape(
            bytes,
            table,
            construction.opaque_post_operation_offset,
            construction.record_indexes,
            *settings,
            *path,
        );
    }
    if let Some((id, selected)) = point {
        let types = HashMap::from([(
            u64::from(id),
            (
                selected.design_type.type_guid.as_str(),
                selected.design_type.version,
            ),
        )]);
        let point_data =
            exact_point_data_construction(bytes, records, std::iter::once(&id), &types)?;
        if point_data.point_record_byte_offset != u64_from_index(selected.start)
            || !records
                .frames(id)
                .any(|(start, end)| start == selected.start && end == selected.member_end)
        {
            return None;
        }
        // Exactly one marked reference between the prefix and the reference
        // table names the point-data record.
        let after_prefix = usize::try_from(construction.opaque_post_operation_offset)
            .ok()?
            .checked_add(2)?;
        let mut prefix_reference_offset = None;
        for at in after_prefix..table {
            if at.checked_add(11).is_some_and(|after| after <= table)
                && marked_record_reference(bytes, at) == Some(id)
                && prefix_reference_offset
                    .replace(u64_from_index(at + 1))
                    .is_some()
            {
                return None;
            }
        }
        construction.point_selection = Some(DesignPipePointSelection {
            prefix_reference_offset: prefix_reference_offset?,
            point_data,
        });
    }
    index.check_relation(scope, &construction)?;
    Some(construction)
}

/// Read the section shape when the section frame after the prefix names
/// exactly the scalar owners and settings record, then the path group.
fn section_frame_shape(
    bytes: &[u8],
    table: usize,
    opaque_offset: u64,
    owners: [u32; 4],
    settings: u32,
    path: u32,
) -> Option<DesignPipeSection> {
    let start = usize::try_from(opaque_offset).ok()?.checked_add(2)?;
    let frame = bytes.get(start..table)?;
    if frame.len() < section_frame::LEN {
        return None;
    }
    let mut named = (0..5)
        .map(|ordinal| marked_record_reference(frame, ordinal * 11))
        .collect::<Option<Vec<_>>>()?;
    named.sort_unstable();
    let mut expected = owners.to_vec();
    expected.push(settings);
    expected.sort_unstable();
    if named != expected
        || frame.get(section_frame::ZERO_BYTE) != Some(&section_frame::ZERO_BYTE_VALUE)
        || marked_record_reference(frame, section_frame::PATH_GROUP_REFERENCE) != Some(path)
    {
        return None;
    }
    Some(DesignPipeSection {
        shape_value: cadmpeg_core::decode::View::u32_le_at(frame, section_frame::SHAPE)?,
        shape_offset: u64_from_index(start.checked_add(section_frame::SHAPE)?),
    })
}

#[cfg(test)]
mod tests;
