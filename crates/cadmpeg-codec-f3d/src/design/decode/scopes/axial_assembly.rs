// SPDX-License-Identifier: Apache-2.0
//! Bind axial assembly operand targets and joint-origin frames from assemblies.

use super::combine::take_external_reference_identity;
use super::shared_frames::exact_indexed_header_at;
use super::shared_frames::exact_same_segment_record_reference;
use super::shared_frames::marked_record_reference;
use super::shared_frames::rigid_transform_at;
use super::shared_frames::unique_match;
use super::work_geometry::ScopePlacementFrame;
use crate::bytes::take_reference;
use crate::design::decode::byte_fields::zeros_at;
use crate::design::decode::reference_runs::admit_reference_values;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::fixed_relaxed_guid_text;
use crate::design::decode::text::retain_class_tag;
use crate::layout::assembly_axial_construction_carrier as axial_carrier;
use crate::layout::assembly_axial_role_prefix as axial_role;
use crate::layout::assembly_axial_selector_prefix as axial_selector;
use crate::records::feature::assembly;
use crate::records::feature::assembly::DesignAssemblyAxialOperandTarget;
use crate::records::feature::assembly::DesignAssemblyAxialSelectorIdentity;
use crate::records::feature::assembly::DesignAssemblyOperandFrame;
use crate::records::feature::assembly::DesignAssemblyOperandQualifier;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

/// A joint-origin frame an assembly scope states for the joint-origin record
/// it names, with the frame's transform offset and optional reference.
type JointOriginCandidate = (u32, SketchPlacementMatrix, u64, Option<(u32, u64)>);

/// A single-joint-origin assembly envelope: the assembly scope's position and
/// record index, the joint-origin record it names and the frame it states.
type JointOriginEnvelope = (usize, u32, u32, SketchPlacementMatrix);

fn is_assemble(scope: &DesignParameterScope) -> bool {
    matches!(scope.payload(), scope::DesignScopePayload::Assemble(_))
}

fn is_joint_origin(scope: &DesignParameterScope) -> bool {
    matches!(scope.payload(), scope::DesignScopePayload::JointOrigin(_))
}

/// The run of `values`, sorted by `key_of`, whose key is `key`. Both ends are
/// bisected.
fn sorted_run<'values, T>(
    ctx: &DecodeContext<'_>,
    values: &'values [T],
    key: u32,
    key_of: impl Fn(&T) -> u32,
    operation: &'static str,
) -> Result<&'values [T], CodecError> {
    let first = ctx.partition_point(values, |value| Ok(key_of(value) < key), operation)?;
    let rest = values.get(first..).unwrap_or(&[]);
    let length = ctx.partition_point(rest, |value| Ok(key_of(value) == key), operation)?;
    Ok(rest.get(..length).unwrap_or(&[]))
}

pub(super) fn bind_joint_origin_frames_from_assemblies(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scopes: &mut [DesignParameterScope],
) -> Result<(), CodecError> {
    let mut candidate_storage = ctx.reserve_scoped(0, "f3d joint-origin frame candidates")?;
    let mut candidates: Vec<JointOriginCandidate> = Vec::new();
    let mut envelope_storage = ctx.reserve_scoped(0, "f3d joint-origin assembly envelopes")?;
    let mut envelopes: Vec<JointOriginEnvelope> = Vec::new();
    for (ordinal, scope) in ctx
        .admit_iter(&*scopes, "scan F3D joint-origin source scopes")?
        .enumerate()
    {
        if !is_assemble(scope) {
            continue;
        }
        if let Some(frames) = scope
            .assembly_alignment()
            .and_then(assembly::DesignAssemblyAlignment::operand_frames)
        {
            for frame in frames {
                ctx.push_scoped_vec(
                    &mut candidate_storage,
                    &mut candidates,
                    (
                        frame.reference_record_index,
                        frame.transform,
                        frame.transform_offset,
                        None,
                    ),
                    "f3d joint-origin frame candidates",
                )?;
            }
        }
        if let Some((joint_origin, frame)) = exact_single_joint_origin_frame(bytes, scope) {
            ctx.push_scoped_vec(
                &mut envelope_storage,
                &mut envelopes,
                (ordinal, scope.record_index, joint_origin, frame.transform),
                "f3d joint-origin assembly envelopes",
            )?;
            ctx.push_scoped_vec(
                &mut candidate_storage,
                &mut candidates,
                (
                    joint_origin,
                    frame.transform,
                    frame.transform_offset,
                    frame.reference,
                ),
                "f3d joint-origin frame candidates",
            )?;
        }
    }
    // A stable sort keeps the candidates of one joint origin in scope order.
    ctx.stable_sort_by_key(
        &mut candidates[..],
        |candidate| candidate.0,
        Ord::cmp,
        "sort F3D joint-origin frame candidates",
    )?;
    for ordinal in ctx.admit_iter(&(0..scopes.len()), "scan F3D joint-origin scopes")? {
        let scope = &mut scopes[ordinal];
        if !is_joint_origin(scope) || scope.joint_origin_frame().is_some() {
            continue;
        }
        let run = sorted_run(
            ctx,
            &candidates,
            scope.record_index,
            |candidate| candidate.0,
            "find F3D joint-origin frame candidates",
        )?;
        let Some(((_, transform, transform_offset, reference), others)) = run.split_first() else {
            continue;
        };
        let reference_record_index = reference.map(|(record_index, _)| record_index);
        if ctx.any_by(
            others,
            |(_, other_transform, _, other_reference)| {
                Ok(other_transform != transform
                    || other_reference.map(|(record_index, _)| record_index)
                        != reference_record_index)
            },
            "compare F3D joint-origin frame candidates",
        )? {
            continue;
        }
        let construction = Some(scope::DesignJointOriginTransform {
            joint_origin_transform: *transform,
            joint_origin_transform_offset: *transform_offset,
            reference: reference.map(|(record_index, offset)| scope::DesignJointOriginReference {
                joint_origin_reference: record_index,
                joint_origin_reference_offset: offset,
            }),
        });
        if let scope::DesignScopePayloadMut::JointOrigin(slot) = scope.payload_mut() {
            *slot = construction;
        }
    }
    if envelopes.is_empty() {
        return Ok(());
    }
    // Resolved joint origins in scope order; for a repeated record index the
    // last scope's frame stands.
    let mut origin_storage = ctx.reserve_scoped(0, "f3d resolved joint origins")?;
    let mut resolved_origins = Vec::new();
    // Record indexes of every Assemble scope. An envelope binds only an
    // assembly whose record index no other Assemble scope repeats.
    let mut assembly_storage = ctx.reserve_scoped(0, "f3d joint-origin assembly records")?;
    let mut assembly_indexes = Vec::new();
    for scope in ctx.admit_iter(&*scopes, "scan F3D resolved joint-origin scopes")? {
        if is_assemble(scope) {
            ctx.push_scoped_vec(
                &mut assembly_storage,
                &mut assembly_indexes,
                scope.record_index,
                "f3d joint-origin assembly records",
            )?;
        } else if let Some(transform) = scope
            .joint_origin_transform()
            .filter(|_| is_joint_origin(scope))
        {
            ctx.push_scoped_vec(
                &mut origin_storage,
                &mut resolved_origins,
                (scope.record_index, transform),
                "f3d resolved joint origins",
            )?;
        }
    }
    ctx.stable_sort_by_key(
        &mut resolved_origins[..],
        |origin| origin.0,
        Ord::cmp,
        "sort F3D resolved joint origins",
    )?;
    ctx.stable_sort_by_key(
        &mut assembly_indexes[..],
        |record_index| *record_index,
        Ord::cmp,
        "sort F3D joint-origin assembly records",
    )?;
    for &(assembly_ordinal, assembly_record_index, joint_origin_record_index, transform) in
        ctx.admit_iter(&envelopes, "scan F3D joint-origin assembly envelopes")?
    {
        let origins = sorted_run(
            ctx,
            &resolved_origins,
            joint_origin_record_index,
            |origin| origin.0,
            "find F3D resolved joint origin",
        )?;
        if origins.last().map(|origin| origin.1) != Some(transform)
            || sorted_run(
                ctx,
                &assembly_indexes,
                assembly_record_index,
                |record_index| *record_index,
                "find F3D joint-origin assembly",
            )?
            .len()
                != 1
        {
            continue;
        }
        if let Some(alignment) = scopes[assembly_ordinal].assembly_alignment_mut() {
            alignment.form = Some(assembly::DesignAssemblyAlignmentForm::DatumEnvelope {
                joint_origin_scope_record_index: joint_origin_record_index,
            });
        }
    }
    Ok(())
}

/// Bind the pathless axial assembly selectors after every scope in the Design
/// stream has decoded its own construction. A binding rewrites only its own
/// Assemble scope, and target resolution reads only component-insert and
/// joint-origin scopes, so each binding is written as soon as it is found.
pub(super) fn bind_axial_assembly_operand_targets(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scopes: &mut [DesignParameterScope],
) -> Result<(), CodecError> {
    for ordinal in ctx.admit_iter(&(0..scopes.len()), "scan F3D axial assembly scopes")? {
        let scope = &scopes[ordinal];
        if !matches!(scope.frame_length(), 705 | 772) {
            continue;
        }
        let Some(assembly::DesignAssemblyAlignmentForm::Frames { frames }) = scope
            .assembly_alignment()
            .and_then(|alignment| alignment.form.as_ref())
        else {
            continue;
        };
        let Some(first) =
            exact_assembly_axial_operand_target(ctx, bytes, records, scope, &frames[0], scopes)?
        else {
            continue;
        };
        let Some(second) =
            exact_assembly_axial_operand_target(ctx, bytes, records, scope, &frames[1], scopes)?
        else {
            continue;
        };
        let form = assembly::DesignAssemblyAlignmentForm::qualified(
            frames.clone(),
            [first, second].map(|target| DesignAssemblyOperandQualifier::AxialTarget { target }),
        );
        if let Some(alignment) = scopes[ordinal].assembly_alignment_mut() {
            alignment.form = Some(form);
        }
    }
    Ok(())
}

struct AxialComponentOperand<'bytes> {
    construction_record_index: u32,
    construction_class_tag: &'bytes [u8; 3],
    construction_byte_offset: u64,
    construction_transform_offset: u64,
    axis_record_index_offsets: [u64; 2],
    construction_paired_class_tag: &'bytes [u8; 3],
    construction_paired_byte_offset: u64,
    selectors: Box<[DesignAssemblyAxialSelectorIdentity; 2]>,
}

#[derive(Clone, Copy)]
struct ExactIndexedRecordPair<'bytes> {
    record_index: u32,
    class_tag: &'bytes [u8; 3],
    byte_offset: usize,
    paired_class_tag: &'bytes [u8; 3],
    paired_byte_offset: usize,
}

fn exact_assembly_axial_operand_target(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    assembly: &DesignParameterScope,
    frame: &DesignAssemblyOperandFrame,
    scopes: &[DesignParameterScope],
) -> Result<Option<DesignAssemblyAxialOperandTarget>, CodecError> {
    let component =
        match exact_assembly_axial_component_operand(ctx, bytes, records, assembly, frame)? {
            Some(component) => {
                let role = component.selectors[0].occurrence_role.as_str();
                let component_insert = unique_match(
                    ctx,
                    scopes,
                    |scope| match scope.component_insert_construction() {
                        Some(construction) => ctx.eq_ignore_ascii_case(
                            &construction.neutron_role,
                            role,
                            "match F3D axial component insert role",
                        ),
                        None => Ok(false),
                    },
                    "find F3D axial component insert",
                )?
                .one();
                match component_insert {
                    Some(component_insert) => Some(
                        DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
                            component_insert_scope_record_index: component_insert.record_index,
                            construction_record_index: component.construction_record_index,
                            construction_class_tag: retain_class_tag(
                                ctx,
                                *component.construction_class_tag,
                                "copy F3D axial assembly construction class tag",
                            )?,
                            construction_byte_offset: component.construction_byte_offset,
                            construction_transform_offset: component.construction_transform_offset,
                            axis_record_index_offsets: component.axis_record_index_offsets,
                            construction_paired_class_tag: retain_class_tag(
                                ctx,
                                *component.construction_paired_class_tag,
                                "copy F3D axial assembly construction class tag",
                            )?,
                            construction_paired_byte_offset: component
                                .construction_paired_byte_offset,
                            selectors: component.selectors,
                        },
                    ),
                    None => None,
                }
            }
            None => None,
        };
    let root = unique_match(
        ctx,
        scopes,
        |scope| {
            Ok(is_joint_origin(scope)
                && scope.record_index == frame.reference_record_index
                && scope.joint_origin_transform() == Some(frame.transform))
        },
        "find F3D axial root joint origin",
    )?
    .one()
    .map(
        |origin| DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
            scope_record_index: origin.record_index,
        },
    );
    Ok(match (component, root) {
        (Some(target), None) | (None, Some(target)) => Some(target),
        _ => None,
    })
}

fn exact_assembly_axial_component_operand<'bytes>(
    ctx: &DecodeContext<'_>,
    bytes: &'bytes [u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frame: &DesignAssemblyOperandFrame,
) -> Result<Option<AxialComponentOperand<'bytes>>, CodecError> {
    if !matches!(scope.frame_length(), 705 | 772) {
        return Ok(None);
    }
    let matching_reference_count = admit_reference_values(
        ctx,
        scope.reference_members(),
        "count F3D axial scope references",
    )?
    .filter(|record_index| **record_index == frame.reference_record_index)
    .count();
    if matching_reference_count != 1 {
        return Ok(None);
    }
    let Some(search_start) = usize::try_from(scope.paired_byte_offset()).ok() else {
        return Ok(None);
    };
    let offsets = records.offsets(frame.reference_record_index);
    let first = ctx.partition_point(
        offsets,
        |start| Ok(*start < search_start),
        "find F3D axial component candidate offsets",
    )?;
    // Candidates are tested under scoped storage; only the one that is kept
    // is read again into retained storage.
    let mut candidate = None;
    let ambiguous = ctx.position_by(
        offsets.get(first..).unwrap_or(&[]),
        |&start| {
            let (operand, _storage) =
                ctx.with_scoped_storage("f3d axial component candidate", || {
                    exact_assembly_axial_component_operand_at(
                        ctx, bytes, records, scope, frame, start,
                    )
                })?;
            if operand.is_none() {
                return Ok(false);
            }
            Ok(candidate.replace(start).is_some())
        },
        "scan F3D axial component candidate offsets",
    )?;
    match candidate {
        Some(start) if ambiguous.is_none() => {
            exact_assembly_axial_component_operand_at(ctx, bytes, records, scope, frame, start)
        }
        _ => Ok(None),
    }
}

/// The two axis references, with their offsets, of the axial construction
/// carrier at `start` when the carrier repeats the transform of `frame`.
fn axial_carrier_axes(
    bytes: &[u8],
    frame: &DesignAssemblyOperandFrame,
    start: usize,
) -> Option<[(u32, u64); 2]> {
    let construction_transform_at = start.checked_add(axial_carrier::OPERAND_TRANSFORM)?;
    if rigid_transform_at(bytes, construction_transform_at)? != frame.transform {
        return None;
    }
    let first = exact_same_segment_record_reference(
        bytes,
        start.checked_add(axial_carrier::FIRST_AXIS_RECORD_REFERENCE)?,
    )?;
    let second = exact_same_segment_record_reference(
        bytes,
        start.checked_add(axial_carrier::SECOND_AXIS_RECORD_REFERENCE)?,
    )?;
    (first.0 != second.0).then_some([first, second])
}

/// Whether the scope references hold each `[axis, selector]` pair adjacently
/// exactly once and each of the four records exactly once. One traversal
/// counts every adjacency and membership.
fn scope_references_name_axes_once(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    pairs: [[u32; 2]; 2],
) -> Result<bool, CodecError> {
    let mut adjacent = [0_usize; 2];
    let mut members = [[0_usize; 2]; 2];
    let mut previous = None;
    for value in admit_reference_values(
        ctx,
        scope.reference_members(),
        "count F3D paired axial scope references",
    )? {
        for ((pair, adjacent), members) in pairs.iter().zip(&mut adjacent).zip(&mut members) {
            if previous == Some(pair[0]) && *value == pair[1] {
                *adjacent += 1;
            }
            for (record_index, count) in pair.iter().zip(members.iter_mut()) {
                if *value == *record_index {
                    *count += 1;
                }
            }
        }
        previous = Some(*value);
    }
    Ok(adjacent == [1; 2] && members == [[1; 2]; 2])
}

fn exact_assembly_axial_component_operand_at<'bytes>(
    ctx: &DecodeContext<'_>,
    bytes: &'bytes [u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frame: &DesignAssemblyOperandFrame,
    start: usize,
) -> Result<Option<AxialComponentOperand<'bytes>>, CodecError> {
    let Some(construction_class_tag) =
        exact_indexed_header_at(bytes, start, frame.reference_record_index)
    else {
        return Ok(None);
    };
    let paired_at = start + axial_carrier::PAIRED_INDEXED_HEADER;
    let Some(construction_paired_class_tag) =
        exact_indexed_header_at(bytes, paired_at, frame.reference_record_index)
    else {
        return Ok(None);
    };
    let Some(
        [(first_axis_record_index, first_axis_offset), (second_axis_record_index, second_axis_offset)],
    ) = axial_carrier_axes(bytes, frame, start)
    else {
        return Ok(None);
    };
    let (Some(first_selector_record_index), Some(second_selector_record_index)) = (
        first_axis_record_index.checked_add(3),
        second_axis_record_index.checked_add(3),
    ) else {
        return Ok(None);
    };
    if !scope_references_name_axes_once(
        ctx,
        scope,
        [
            [first_axis_record_index, first_selector_record_index],
            [second_axis_record_index, second_selector_record_index],
        ],
    )? {
        return Ok(None);
    }
    let Some(search_start) = usize::try_from(scope.paired_byte_offset()).ok() else {
        return Ok(None);
    };
    let Some(first_axis) = exact_paired_indexed_record_between(
        ctx,
        bytes,
        records,
        first_axis_record_index,
        search_start,
        start,
    )?
    else {
        return Ok(None);
    };
    let Some(second_axis) = exact_paired_indexed_record_between(
        ctx,
        bytes,
        records,
        second_axis_record_index,
        search_start,
        start,
    )?
    else {
        return Ok(None);
    };
    if first_axis.byte_offset >= second_axis.byte_offset {
        return Ok(None);
    }
    let second_axis_at = second_axis.byte_offset;
    let Some(first) =
        exact_assembly_axial_selector(ctx, bytes, records, first_axis, second_axis_at)?
    else {
        return Ok(None);
    };
    let Some(second) = exact_assembly_axial_selector(ctx, bytes, records, second_axis, start)?
    else {
        return Ok(None);
    };
    if !first.selects_same_object(&second)
        || !ctx.eq_ignore_ascii_case(
            first.occurrence_role.as_str(),
            second.occurrence_role.as_str(),
            "match F3D axial selector roles",
        )?
    {
        return Ok(None);
    }
    Ok(Some(AxialComponentOperand {
        construction_record_index: frame.reference_record_index,
        construction_class_tag,
        construction_byte_offset: u64_from_index(start),
        construction_transform_offset: u64_from_index(start + axial_carrier::OPERAND_TRANSFORM),
        axis_record_index_offsets: [first_axis_offset, second_axis_offset],
        construction_paired_class_tag,
        construction_paired_byte_offset: u64_from_index(paired_at),
        selectors: Box::new([first, second]),
    }))
}

/// The headers of `record_index` strictly between `after` and `before` when
/// there are exactly `N` of them. The ascending offsets are bisected to the
/// first header after `after`.
fn exact_headers_between<const N: usize>(
    ctx: &DecodeContext<'_>,
    records: &IndexedRecordOffsets,
    record_index: u32,
    after: usize,
    before: usize,
) -> Result<Option<[usize; N]>, CodecError> {
    let offsets = records.offsets(record_index);
    let first = ctx.partition_point(
        offsets,
        |offset| Ok(*offset <= after),
        "find F3D axial selector records",
    )?;
    let rest = offsets.get(first..).unwrap_or(&[]);
    let Some(headers) = rest.first_chunk::<N>() else {
        return Ok(None);
    };
    if headers.iter().any(|offset| *offset >= before)
        || rest.get(N).is_some_and(|next| *next < before)
    {
        return Ok(None);
    }
    Ok(Some(*headers))
}

/// The fixed prefix of an axial selector record from `selector_at` to its
/// paired header.
struct AxialSelectorPrefix<'bytes> {
    class_tag: &'bytes [u8; 3],
    paired_class_tag: &'bytes [u8; 3],
    nested_record_index: u32,
    nested_record_index_offset: usize,
    /// Offset of the counted selector asset GUID.
    asset_at: usize,
}

fn axial_selector_prefix(
    bytes: &[u8],
    selector_at: usize,
    selector_paired_at: usize,
    selector_record_index: u32,
) -> Option<AxialSelectorPrefix<'_>> {
    let class_tag = exact_indexed_header_at(bytes, selector_at, selector_record_index)?;
    let paired_class_tag =
        exact_indexed_header_at(bytes, selector_paired_at, selector_record_index)?;
    if !zeros_at::<{ axial_selector::NESTED_RECORD_REFERENCE - axial_selector::ZERO_RUN_11 }>(
        bytes,
        selector_at.checked_add(axial_selector::ZERO_RUN_11)?,
    ) {
        return None;
    }
    let nested_at = selector_at.checked_add(axial_selector::NESTED_RECORD_REFERENCE)?;
    let (nested_record_index, _) = exact_same_segment_record_reference(bytes, nested_at)?;
    if nested_record_index != selector_record_index.checked_add(3)?
        || View::u32_le_at(bytes, nested_at.checked_add(11)?)? != 1
    {
        return None;
    }
    Some(AxialSelectorPrefix {
        class_tag,
        paired_class_tag,
        nested_record_index,
        nested_record_index_offset: nested_at.checked_add(1)?,
        asset_at: nested_at.checked_add(15)?,
    })
}

/// The local occurrence reference that follows the selector context GUID
/// ending at `context_end`, its offset, and the cursor after the count 1 that
/// follows it.
fn axial_selector_occurrence(bytes: &[u8], context_end: usize) -> Option<(u64, usize, usize)> {
    if View::u32_le_at(bytes, context_end)? != 2
        || View::u32_le_at(bytes, context_end.checked_add(4)?)? != 0
        || View::u32_le_at(bytes, context_end.checked_add(8)?)? != 1
    {
        return None;
    }
    let mut cursor = context_end.checked_add(12)?;
    let occurrence_reference_offset = cursor.checked_add(1)?;
    let (occurrence_reference, _) = take_reference(bytes, &mut cursor)?.local()?;
    if View::u32_le_at(bytes, cursor)? != 1 {
        return None;
    }
    Some((
        occurrence_reference,
        occurrence_reference_offset,
        cursor.checked_add(4)?,
    ))
}

fn exact_assembly_axial_selector(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    axis: ExactIndexedRecordPair,
    limit: usize,
) -> Result<Option<DesignAssemblyAxialSelectorIdentity>, CodecError> {
    const CLASS_TAG_OPERATION: &str = "copy F3D axial selector class tag";
    let axis_record_index = axis.record_index;
    let Some(selector_record_index) = axis_record_index.checked_add(3) else {
        return Ok(None);
    };
    let Some([selector_at, selector_paired_at]) = exact_headers_between::<2>(
        ctx,
        records,
        selector_record_index,
        axis.paired_byte_offset,
        limit,
    )?
    else {
        return Ok(None);
    };
    let Some(prefix) = axial_selector_prefix(
        bytes,
        selector_at,
        selector_paired_at,
        selector_record_index,
    ) else {
        return Ok(None);
    };
    let Some((selector_asset_id, selector_context_at)) =
        fixed_relaxed_guid_text(ctx, bytes, prefix.asset_at)?
    else {
        return Ok(None);
    };
    let Some((selector_context_id, after_selector_context_id)) =
        fixed_relaxed_guid_text(ctx, bytes, selector_context_at)?
    else {
        return Ok(None);
    };
    let Some((occurrence_reference, occurrence_reference_offset, mut cursor)) =
        axial_selector_occurrence(bytes, after_selector_context_id)
    else {
        return Ok(None);
    };
    let Some(external) = take_external_reference_identity(ctx, bytes, &mut cursor)? else {
        return Ok(None);
    };
    if cursor > selector_paired_at
        || !ctx.eq_ignore_ascii_case(
            external.asset_id.as_str(),
            selector_asset_id.as_str(),
            "match F3D axial selector asset",
        )?
    {
        return Ok(None);
    }
    let Some(role_record_index) = selector_record_index.checked_add(5) else {
        return Ok(None);
    };
    let Some([role_at]) =
        exact_headers_between::<1>(ctx, records, role_record_index, selector_paired_at, limit)?
    else {
        return Ok(None);
    };
    let Some(role_class_tag) = exact_indexed_header_at(bytes, role_at, role_record_index) else {
        return Ok(None);
    };
    if !zeros_at::<{ axial_role::CONSTANT_ONE - axial_role::ZERO_RUN_10 }>(
        bytes,
        role_at + axial_role::ZERO_RUN_10,
    ) || View::u32_le_at(bytes, role_at + axial_role::CONSTANT_ONE) != Some(1)
    {
        return Ok(None);
    }
    let occurrence_role_at = role_at + axial_role::ROLE_CODE_UNIT_COUNT;
    let Some((occurrence_role, after_occurrence_role)) =
        fixed_relaxed_guid_text(ctx, bytes, occurrence_role_at)?
    else {
        return Ok(None);
    };
    if after_occurrence_role > limit {
        return Ok(None);
    }
    Ok(Some(DesignAssemblyAxialSelectorIdentity {
        axis_record_index,
        axis_class_tag: retain_class_tag(ctx, *axis.class_tag, CLASS_TAG_OPERATION)?,
        axis_byte_offset: u64_from_index(axis.byte_offset),
        axis_paired_class_tag: retain_class_tag(ctx, *axis.paired_class_tag, CLASS_TAG_OPERATION)?,
        axis_paired_byte_offset: u64_from_index(axis.paired_byte_offset),
        selector_record_index,
        selector_class_tag: retain_class_tag(ctx, *prefix.class_tag, CLASS_TAG_OPERATION)?,
        selector_byte_offset: u64_from_index(selector_at),
        selector_paired_class_tag: retain_class_tag(
            ctx,
            *prefix.paired_class_tag,
            CLASS_TAG_OPERATION,
        )?,
        selector_paired_byte_offset: u64_from_index(selector_paired_at),
        nested_record_index: prefix.nested_record_index,
        nested_record_index_offset: u64_from_index(prefix.nested_record_index_offset),
        selector_asset_id,
        selector_asset_id_offset: u64_from_index(prefix.asset_at + 4),
        selector_context_id,
        selector_context_id_offset: u64_from_index(selector_context_at + 4),
        occurrence_reference,
        occurrence_reference_offset: u64_from_index(occurrence_reference_offset),
        external_object_reference: external.target,
        external_object_reference_offset: external.target_offset,
        external_segment: external.segment,
        external_segment_offset: external.segment_offset,
        external_asset_id: external.asset_id,
        external_asset_id_offset: external.asset_id_offset,
        external_link_name: external.link_name,
        external_link_name_offset: external.link_name_offset,
        external_version: external.version,
        role_record_index,
        role_class_tag: retain_class_tag(ctx, *role_class_tag, CLASS_TAG_OPERATION)?,
        role_byte_offset: u64_from_index(role_at),
        occurrence_role,
        occurrence_role_offset: u64_from_index(occurrence_role_at + 4),
    }))
}

/// The only two headers of `record_index` in `start..end`. The ascending
/// offsets are bisected to the first header at or after `start`.
fn exact_paired_indexed_record_between<'bytes>(
    ctx: &DecodeContext<'_>,
    bytes: &'bytes [u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    start: usize,
    end: usize,
) -> Result<Option<ExactIndexedRecordPair<'bytes>>, CodecError> {
    let offsets = records.offsets(record_index);
    let first = ctx.partition_point(
        offsets,
        |offset| Ok(*offset < start),
        "find F3D axial assembly paired record",
    )?;
    let Some(rest) = offsets.get(first..) else {
        return Ok(None);
    };
    let Some(&[primary_at, paired_at]) = rest.first_chunk::<2>() else {
        return Ok(None);
    };
    if paired_at >= end || rest.get(2).is_some_and(|next| *next < end) {
        return Ok(None);
    }
    let Some(class_tag) = exact_indexed_header_at(bytes, primary_at, record_index) else {
        return Ok(None);
    };
    let Some(paired_class_tag) = exact_indexed_header_at(bytes, paired_at, record_index) else {
        return Ok(None);
    };
    Ok(Some(ExactIndexedRecordPair {
        record_index,
        class_tag,
        byte_offset: primary_at,
        paired_class_tag,
        paired_byte_offset: paired_at,
    }))
}

fn exact_single_joint_origin_frame(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<(u32, ScopePlacementFrame)> {
    if !is_assemble(scope)
        || scope.class_tag.as_str() != "276"
        || scope.paired_class_tag.as_str() != "258"
        || scope.frame_length() != 604
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if usize::try_from(scope.paired_byte_offset()).ok()? != start.checked_add(604)?
        || !zeros_at::<13>(bytes, start + 11)
        || !zeros_at::<7>(bytes, start + 29)
        || !zeros_at::<6>(bytes, start + 169)
        || View::u32_le_at(bytes, start + 175)? != 1
    {
        return None;
    }
    let reference_record_index = marked_record_reference(bytes, start + 24)?;
    let joint_origin_record_index = marked_record_reference(bytes, start + 164)?;
    if reference_record_index == joint_origin_record_index {
        return None;
    }
    let transform = rigid_transform_at(bytes, start + 36)?;
    Some((
        joint_origin_record_index,
        ScopePlacementFrame {
            transform,
            transform_offset: u64_from_index(start + 36),
            reference: Some((reference_record_index, u64_from_index(start + 25))),
        },
    ))
}
