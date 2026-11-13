// SPDX-License-Identifier: Apache-2.0
//! Exact surface extend, offset, boundary, stitch and ruled-surface operation scopes.

use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::take_reference;
use crate::design::decode::operands::parse_construction_operand_group;
use crate::design::decode::operands::ConstructionOperandGroupParse;
use crate::design::decode::operands::RecordFrame;
use crate::design::decode::parameters::parse_parameter_owner;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::fixed_guid_end;
use crate::design::decode::text::fixed_relaxed_guid_text;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::layout::compact_flag_record as flag_record;
use crate::layout::surface_boundary_group_tail_87 as tail_87;
use crate::layout::surface_boundary_group_tail_88 as tail_88;
use crate::layout::surface_untrim_prologue as untrim;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::DesignRuledSurfaceCorner;
use crate::records::feature::surface_ops::DesignRuledSurfaceMethod;
use crate::records::feature::surface_ops::DesignRuledSurfaceOperation;
use crate::records::feature::surface_ops::DesignSurfaceExtendOperation;
use crate::records::feature::surface_ops::DesignSurfaceOffsetOperation;
use crate::records::feature::surface_ops::DesignSurfaceOffsetSupport;
use crate::records::feature::surface_ops::DesignSurfaceStitchOperation;
use crate::records::feature::surface_ops::DesignSurfaceUntrimLoops;
use crate::records::feature::surface_ops::DesignSurfaceUntrimOperation;
use crate::records::recipes::ConstructionRecipe;
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::HashSet;

pub(super) fn exact_surface_extend_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<DesignSurfaceExtendOperation> {
    let operation = exact_surface_boundary_operation(
        bytes,
        records,
        scope,
        DesignFeatureFamily::SurfaceExtend,
        DesignOperandRole::BODIES_B,
    )?;
    surface_boundary_flag_loss(
        bytes,
        records,
        scope,
        operation.boundary_reference_record_index,
        losses,
    );
    Some(DesignSurfaceExtendOperation {
        distance: operation.distance,
        distance_offset: operation.distance_offset,
        distance_record_index: operation.distance_record_index,
        boundary_record_index: operation.boundary_record_index,
        boundary_reference_record_index: operation.boundary_reference_record_index,
        boundary_reference_offset: operation.boundary_reference_offset,
        edge_record_indices: operation.edge_record_indices,
        tolerance: operation.tolerance,
        tolerance_offset: operation.tolerance_offset,
    })
}

const MARKED_REFERENCE_LEN: usize = 11;
const U32_LEN: usize = 4;

pub(super) fn exact_surface_untrim_operation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<DesignSurfaceUntrimOperation>, CodecError> {
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SurfaceUntrim {
        return Ok(None);
    }
    let Some(frame) = surface_untrim_frame(bytes, scope) else {
        return Ok(None);
    };
    let Some(count) = View::over_retained(frame.groups).counted(
        u64_from_index(frame.groups.len() / MARKED_REFERENCE_LEN),
        MARKED_REFERENCE_LEN,
    ) else {
        return Ok(None);
    };
    let mut operation = DesignSurfaceUntrimOperation::new(
        frame.loops,
        u64_from_index(frame.loops_offset),
        frame.extension_record_index,
        frame.face_group_record_index,
    );
    operation.padded = frame.padded;
    operation.loop_group_record_indices =
        ctx.alloc_filled(count.get(), 0u32, "F3D untrim loop references")?;
    for (target, chunk) in operation
        .loop_group_record_indices
        .iter_mut()
        .zip(frame.groups.chunks_exact(MARKED_REFERENCE_LEN))
    {
        let Some(reference) = marked_record_reference(chunk, 0) else {
            return Ok(None);
        };
        *target = reference;
    }
    let mut owners =
        records
            .frames(frame.extension_record_index)
            .filter_map(|(frame_start, frame_end)| {
                let owner = parse_parameter_owner(bytes.get(frame_start..frame_end)?)?;
                (owner.record_index == frame.extension_record_index
                    && owner.scope_record_index == scope.record_index
                    && owner.local_ordinal == 0)
                    .then_some(())?;
                let offset = owner
                    .evaluated_value_offset
                    .absolute(u64_from_index(frame_start))?;
                Some((owner.evaluated_value, offset))
            });
    if let (Some((extension, offset)), None) = (owners.next(), owners.next()) {
        operation.extension = Some(extension);
        operation.extension_offset = Some(offset);
    } else {
        losses.push(crate::loss::F3dLossCode::SurfaceUntrimExtensionUnresolved.note(format!(
            "SurfaceUntrim {} retains extension reference {} because its numeric owner is missing, ambiguous or inconsistent (DR-491).",
            scope.id, frame.extension_record_index
        )));
    }
    if !frame.padded {
        losses.push(crate::loss::F3dLossCode::SurfaceUntrimPaddingAssumed.note(format!(
            "SurfaceUntrim {} reads the uniquely closed unpadded reference runs as the same loop-removal frame (DR-490).", scope.id
        )));
    }
    Ok(Some(operation))
}

struct SurfaceUntrimFrame<'a> {
    groups: &'a [u8],
    extension_record_index: u32,
    loops: DesignSurfaceUntrimLoops,
    loops_offset: usize,
    face_group_record_index: u32,
    padded: bool,
}

fn surface_untrim_frame<'a>(
    bytes: &'a [u8],
    scope: &DesignParameterScope,
) -> Option<SurfaceUntrimFrame<'a>> {
    let prefix_end = usize::try_from(scope.reference_count_offset()).ok()?;
    let bytes = bytes.get(..prefix_end)?;
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let count_at = start.checked_add(untrim::LOOP_GROUP_COUNT)?;
    if bytes
        .get(start.checked_add(untrim::ZERO_RUN)?..count_at)?
        .iter()
        .any(|byte| *byte != 0)
    {
        return None;
    }
    let mut view = View::over_retained(bytes.get(count_at..)?);
    let count = view.u32_le()?;
    let count = view.counted(u64::from(count), MARKED_REFERENCE_LEN)?.get();
    let groups = view.take(count.checked_mul(MARKED_REFERENCE_LEN)?)?;
    if groups
        .chunks_exact(MARKED_REFERENCE_LEN)
        .any(|chunk| marked_record_reference(chunk, 0).is_none())
    {
        return None;
    }
    let mut cursor = count_at.checked_add(view.position())?;
    let extension_record_index = marked_record_reference(bytes, cursor)?;
    cursor = cursor.checked_add(MARKED_REFERENCE_LEN)?;
    let loops_offset = cursor;
    let loops = match View::u32_le_at(bytes, cursor)? {
        0 => DesignSurfaceUntrimLoops::All,
        1 => DesignSurfaceUntrimLoops::Internal,
        2 => DesignSurfaceUntrimLoops::External,
        3 => DesignSurfaceUntrimLoops::Manual,
        _ => return None,
    };
    cursor = cursor.checked_add(U32_LEN)?;
    let mut endings = [cursor, cursor.checked_add(U32_LEN)?]
        .into_iter()
        .filter(|at| *at == cursor || View::u32_le_at(bytes, cursor) == Some(0))
        .filter_map(|at| untrim_face_group_reference(bytes, at).map(|group| (group, at != cursor)));
    let (Some((face_group_record_index, padded)), None) = (endings.next(), endings.next()) else {
        return None;
    };
    let mut references = scope.reference_members().values().copied();
    if references.next() != Some(extension_record_index)
        || references.next() != Some(face_group_record_index)
    {
        return None;
    }
    for chunk in groups.chunks_exact(MARKED_REFERENCE_LEN) {
        let group = marked_record_reference(chunk, 0)?;
        references.by_ref().find(|reference| *reference == group)?;
    }
    Some(SurfaceUntrimFrame {
        groups,
        extension_record_index,
        loops,
        loops_offset,
        face_group_record_index,
        padded,
    })
}

fn untrim_face_group_reference(bytes: &[u8], runs_at: usize) -> Option<u32> {
    if View::u32_le_at(bytes, runs_at)? != 0
        || View::u32_le_at(bytes, runs_at.checked_add(U32_LEN)?)? != 0
        || View::u32_le_at(bytes, runs_at.checked_add(2 * U32_LEN)?)? != 1
    {
        return None;
    }
    let reference_at = runs_at.checked_add(3 * U32_LEN)?;
    let face_group = marked_record_reference(bytes, reference_at)?;
    let guid_end = fixed_guid_end(bytes, reference_at.checked_add(MARKED_REFERENCE_LEN)?)?;
    let table_at = guid_end.checked_add(3)?;
    (bytes.get(guid_end..table_at)? == [0; 3] && table_at == bytes.len()).then_some(face_group)
}

pub(super) fn exact_surface_offset_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    recipes: &[ConstructionRecipe],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<DesignSurfaceOffsetOperation>, CodecError> {
    if let Some((operation, ordinals)) =
        exact_surface_offset_face_groups(bytes, records, scope, recipes)?
    {
        if ordinals.iter().any(|ordinal| *ordinal != ordinals[0]) {
            losses.push(
                crate::loss::F3dLossCode::FeatureSurfaceOffsetGroupOrdinalsAssumed.note(format!(
                    "{} reads support groups with ordinals {} as one support-face selection.",
                    scope.id,
                    ordinals
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            );
        }
        return Ok(Some(operation));
    }
    let Some(stream) = native_stream(&scope.id) else {
        return Ok(None);
    };
    let operation = exact_surface_boundary_operation(
        bytes,
        records,
        scope,
        DesignFeatureFamily::SurfaceOffset,
        DesignOperandRole::PROFILE,
    )
    .filter(|operation| {
        operation.edge_record_indices.iter().all(|member| {
            member_recipe_kind(records, recipes, stream, *member)
                == Some(ConstructionRecipeKind::Edge)
        })
    });
    if let Some(operation) = &operation {
        surface_boundary_flag_loss(
            bytes,
            records,
            scope,
            operation.boundary_reference_record_index,
            losses,
        );
    }
    Ok(operation.map(|operation| DesignSurfaceOffsetOperation {
        distance: operation.distance,
        distance_offset: operation.distance_offset,
        distance_record_index: operation.distance_record_index,
        support: DesignSurfaceOffsetSupport::BoundaryCarrier {
            boundary_record_index: operation.boundary_record_index,
            boundary_reference_record_index: operation.boundary_reference_record_index,
            boundary_reference_offset: operation.boundary_reference_offset,
            edge_record_indices: operation.edge_record_indices,
            tolerance: operation.tolerance,
            tolerance_offset: operation.tolerance_offset,
        },
    }))
}

fn surface_boundary_flag_loss(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    reference: u32,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let matches = records.offsets(reference).iter().any(|&start| {
        let Some(end) = start.checked_add(flag_record::LEN) else {
            return false;
        };
        let Some(frame) = bytes.get(start..end) else {
            return false;
        };
        frame.get(flag_record::ZERO_RUN..flag_record::MARKER) == Some(&[0; 10])
            && frame.get(flag_record::MARKER) == Some(&1)
            && frame
                .get(flag_record::VALUE)
                .is_some_and(|value| *value <= 1)
            && frame.get(flag_record::TERMINATOR) == Some(&0)
    });
    if !matches {
        losses.push(
            crate::loss::F3dLossCode::FeatureSurfaceBoundaryFlagRecordAssumed.note(format!(
                "{} retains boundary reference {} without a matching compact flag record (DR-311).",
                scope.id, reference
            )),
        );
    }
}

/// The grouped support form and the ordinal of each of its groups.
fn exact_surface_offset_face_groups(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    recipes: &[ConstructionRecipe],
) -> Result<Option<(DesignSurfaceOffsetOperation, Vec<std::num::NonZeroU32>)>, CodecError> {
    let parsed = (|| {
        if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::SurfaceOffset) {
            return None;
        }
        let stream = native_stream(&scope.id)?;
        let distance_record_index = scope.reference_members().values().next()?;
        let support_reference_count = scope.reference_members().len() - 1;
        if support_reference_count == 0 {
            return None;
        }
        let scalar = exact_fixed_scalar(bytes, records, *distance_record_index)?;
        if scalar.owner_record_index != Some(scope.record_index) || scalar.ordinal != 0 {
            return None;
        }

        let mut group_record_indices = Vec::new();
        let mut covered_references = HashSet::new();
        let mut ordinals = Vec::new();
        for (scope_reference_ordinal, record_index) in scope
            .reference_members()
            .values()
            .copied()
            .enumerate()
            .skip(1)
        {
            let group = exact_construction_operand_group(
                bytes,
                records,
                scope,
                u32::try_from(scope_reference_ordinal).ok()?,
                record_index,
            );
            let Some(group) = group else {
                continue;
            };
            let group = match group {
                Ok(group) => group,
                Err(error) => return Some(Err(error)),
            };
            if group.role() != DesignOperandRole::PROFILE
                || group.members().is_empty()
                || covered_references.contains(&group.record_index)
            {
                return None;
            }

            covered_references.insert(group.record_index);
            for member in group.members().iter().map(|member| &member.value) {
                if *member == *distance_record_index
                    || !scope
                        .reference_members()
                        .values()
                        .skip(1)
                        .any(|value| value == member)
                    || covered_references.contains(member)
                    || member_recipe_kind(records, recipes, stream, *member)
                        != Some(ConstructionRecipeKind::BoundedFace)
                {
                    return None;
                }

                covered_references.insert(*member);
            }

            group_record_indices.push(group.record_index);
            ordinals.push(group.frame.opaque_index);
        }
        if group_record_indices.is_empty() || covered_references.len() != support_reference_count {
            return None;
        }

        Some(Ok((
            DesignSurfaceOffsetOperation {
                distance: scalar.value,
                distance_offset: scalar.value_offset,
                distance_record_index: *distance_record_index,
                support: DesignSurfaceOffsetSupport::FaceGroups {
                    group_record_indices,
                },
            },
            ordinals,
        )))
    })();
    parsed.transpose()
}

fn member_recipe_kind(
    records: &IndexedRecordOffsets,
    recipes: &[ConstructionRecipe],
    stream: &str,
    member: u32,
) -> Option<ConstructionRecipeKind> {
    let recipe_start = (0..4).try_fold(0, |position, delta| {
        records
            .first_at_or_after(position, member.checked_add(delta)?)
            .and_then(|offset| offset.checked_add(11))
    })?;
    let recipe_end = member
        .checked_add(4)
        .and_then(|next| records.first_at_or_after(recipe_start, next))
        .map(u64::try_from);
    let recipe_start = u64::try_from(recipe_start).ok()?;
    recipes
        .iter()
        .filter(|recipe| {
            native_stream(&recipe.id) == Some(stream) && recipe.byte_offset >= recipe_start
        })
        .min_by_key(|recipe| recipe.byte_offset)
        .filter(|recipe| recipe_end.is_none_or(|end| end.is_ok_and(|end| recipe.byte_offset < end)))
        .map(|recipe| recipe.kind)
}

fn exact_construction_operand_group(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    record_index: u32,
) -> Option<
    Result<crate::records::topology::construction::DesignConstructionOperandGroup, CodecError>,
> {
    let mut candidate = None;
    for (start, _) in records.frames(record_index) {
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
        if after_tag != start + 7 {
            continue;
        }
        let header = RecordFrame {
            record_index,
            class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
            byte_offset: u64::try_from(start).ok()?,
        };
        match parse_construction_operand_group(bytes, scope, scope_reference_ordinal, &header) {
            ConstructionOperandGroupParse::Complete(group) => {
                if candidate.replace(*group).is_some() {
                    return None;
                }
            }
            ConstructionOperandGroupParse::NotAGroup | ConstructionOperandGroupParse::Unclosed => {}
        }
    }
    candidate.map(Ok)
}

struct ExactSurfaceBoundaryOperation {
    distance: cadmpeg_ir::scalar::FiniteReal,
    distance_offset: u64,
    distance_record_index: u32,
    boundary_record_index: u32,
    boundary_reference_record_index: u32,
    boundary_reference_offset: u64,
    edge_record_indices: Vec<u32>,
    tolerance: cadmpeg_ir::scalar::PositiveReal,
    tolerance_offset: u64,
}

struct SurfaceBoundaryTail {
    len: usize,
    absent_optional_references: usize,
    trailing_count: usize,
    trailing_reference: usize,
    role: usize,
    zero_run_10: usize,
    ordinal: usize,
    tolerance: usize,
    ordinal_copy: usize,
    second_next_reference: usize,
    flag_prefix: Option<usize>,
    variant: usize,
    closing_flag: usize,
    next_reference: usize,
    zero_byte: usize,
    scope_reference: usize,
}

const SURFACE_BOUNDARY_TAILS: [SurfaceBoundaryTail; 2] = [
    SurfaceBoundaryTail {
        len: tail_88::LEN,
        absent_optional_references: tail_88::ABSENT_OPTIONAL_REFERENCES,
        trailing_count: tail_88::TRAILING_COUNT,
        trailing_reference: tail_88::TRAILING_REFERENCE,
        role: tail_88::ROLE,
        zero_run_10: tail_88::ZERO_RUN_10,
        ordinal: tail_88::ORDINAL,
        tolerance: tail_88::TOLERANCE,
        ordinal_copy: tail_88::ORDINAL_COPY,
        second_next_reference: tail_88::SECOND_NEXT_REFERENCE,
        flag_prefix: Some(tail_88::FLAG_PREFIX),
        variant: tail_88::VARIANT,
        closing_flag: tail_88::CLOSING_FLAG,
        next_reference: tail_88::NEXT_REFERENCE,
        zero_byte: tail_88::ZERO_BYTE,
        scope_reference: tail_88::SCOPE_REFERENCE,
    },
    SurfaceBoundaryTail {
        len: tail_87::LEN,
        absent_optional_references: tail_87::ABSENT_OPTIONAL_REFERENCES,
        trailing_count: tail_87::TRAILING_COUNT,
        trailing_reference: tail_87::TRAILING_REFERENCE,
        role: tail_87::ROLE,
        zero_run_10: tail_87::ZERO_RUN_10,
        ordinal: tail_87::ORDINAL,
        tolerance: tail_87::TOLERANCE,
        ordinal_copy: tail_87::ORDINAL_COPY,
        second_next_reference: tail_87::SECOND_NEXT_REFERENCE,
        flag_prefix: None,
        variant: tail_87::VARIANT,
        closing_flag: tail_87::CLOSING_FLAG,
        next_reference: tail_87::NEXT_REFERENCE,
        zero_byte: tail_87::ZERO_BYTE,
        scope_reference: tail_87::SCOPE_REFERENCE,
    },
];

fn exact_surface_boundary_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    family: DesignFeatureFamily,
    role: DesignOperandRole,
) -> Option<ExactSurfaceBoundaryOperation> {
    if design_feature_family(&scope.kind()) != Some(family) {
        return None;
    }
    let mut references = scope.reference_members().values().copied();
    let distance_record_index = references.next()?;
    let boundary_record_index = references.next()?;
    let edge_record_indices: Vec<u32> = references.collect();
    if edge_record_indices.is_empty() {
        return None;
    }
    let mut owners = records
        .frames(distance_record_index)
        .filter_map(|(start, end)| {
            let owner = parse_parameter_owner(bytes.get(start..end)?)?;
            (owner.record_index == distance_record_index
                && owner.scope_record_index == scope.record_index
                && owner.local_ordinal == 0)
                .then_some(())?;
            let offset = owner
                .evaluated_value_offset
                .absolute(u64::try_from(start).ok()?)?;
            Some((owner.evaluated_value, offset))
        });
    let (distance, distance_offset) = owners.next()?;
    if owners.next().is_some() {
        return None;
    }
    let mut boundaries = records
        .frames(boundary_record_index)
        .filter_map(|(start, end)| {
            exact_surface_boundary_group(
                bytes,
                start,
                end,
                boundary_record_index,
                &edge_record_indices,
                role,
                scope.record_index,
            )
        });
    let (boundary_reference_record_index, boundary_reference_offset, tolerance, tolerance_offset) =
        boundaries.next()?;
    if boundaries.next().is_some() {
        return None;
    }
    Some(ExactSurfaceBoundaryOperation {
        distance,
        distance_offset,
        distance_record_index,
        boundary_record_index,
        boundary_reference_record_index,
        boundary_reference_offset,
        edge_record_indices,
        tolerance,
        tolerance_offset,
    })
}

fn exact_surface_boundary_group(
    bytes: &[u8],
    start: usize,
    end: usize,
    record_index: u32,
    edge_record_indices: &[u32],
    role: DesignOperandRole,
    scope_record_index: u32,
) -> Option<(u32, u64, cadmpeg_ir::scalar::PositiveReal, u64)> {
    let tail = start
        .checked_add(25)?
        .checked_add(edge_record_indices.len().checked_mul(11)?)?;
    let layout = SURFACE_BOUNDARY_TAILS
        .iter()
        .find(|layout| tail.checked_add(layout.len) == Some(end))?;
    let (class_tag, after_tag) = lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
    let flag = |at: usize| bytes.get(tail + at).is_some_and(|value| *value <= 1);
    let ordinal = View::u32_le_at(bytes, tail + layout.ordinal)?;
    let complete = after_tag == start + 7
        && class_tag.len() == 3
        && class_tag.bytes().all(|byte| byte.is_ascii_digit())
        && bytes.get(start + 11..start + 21)? == [0; 10]
        && View::u32_le_at(bytes, start + 21)? == u32::try_from(edge_record_indices.len()).ok()?
        && edge_record_indices
            .iter()
            .enumerate()
            .all(|(position, record_index)| {
                marked_record_reference(bytes, start + 25 + position * 11) == Some(*record_index)
            })
        && bytes.get(tail + layout.absent_optional_references..tail + layout.trailing_count)?
            == [0; 2]
        && View::u32_le_at(bytes, tail + layout.trailing_count)? == 1
        && View::u64_le_at(bytes, tail + layout.role)? == role.raw()
        && bytes.get(tail + layout.zero_run_10..tail + layout.ordinal)? == [0; 10]
        && ordinal != 0
        && View::u32_le_at(bytes, tail + layout.ordinal_copy)? == ordinal
        && marked_record_reference(bytes, tail + layout.second_next_reference)
            == record_index.checked_add(2)
        && layout
            .flag_prefix
            .is_none_or(|at| bytes.get(tail + at) == Some(&1))
        && flag(layout.variant)
        && flag(layout.closing_flag)
        && marked_record_reference(bytes, tail + layout.next_reference)
            == record_index.checked_add(1)
        && bytes.get(tail + layout.zero_byte) == Some(&0)
        && marked_record_reference(bytes, tail + layout.scope_reference)
            == Some(scope_record_index);
    if !complete {
        return None;
    }
    let trailing_reference = tail + layout.trailing_reference;
    let tolerance = tail + layout.tolerance;
    Some((
        marked_record_reference(bytes, trailing_reference)?,
        u64::try_from(trailing_reference).ok()?,
        cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, tolerance)?)?,
        u64::try_from(tolerance).ok()?,
    ))
}

pub(super) fn exact_surface_stitch_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    references: &[u32],
) -> Option<DesignSurfaceStitchOperation> {
    if references.len() < 4 || !references.len().is_multiple_of(2) {
        return None;
    }
    let tolerance_record_index = references[references.len() - 2];
    let settings_record_index = references[references.len() - 1];
    let scalar = exact_fixed_scalar(bytes, records, tolerance_record_index)?;
    if scalar.owner_record_index != Some(scope_record_index) || scalar.ordinal != 0 {
        return None;
    }
    let gap_tolerance = cadmpeg_ir::scalar::PositiveReal::new(scalar.value.get())?;
    Some(DesignSurfaceStitchOperation {
        gap_tolerance,
        gap_tolerance_offset: scalar.value_offset,
        tolerance_record_index,
        settings_record_index,
    })
}

pub(super) fn exact_ruled_surface_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Result<Option<DesignRuledSurfaceOperation>, CodecError> {
    let parsed = (|| {
        if bytes.get(start.checked_add(11)?..start.checked_add(20)?)? != [0; 9] {
            return None;
        }
        let method_offset = start.checked_add(20)?;
        let method = match View::u32_le_at(bytes, method_offset)? {
            0 => DesignRuledSurfaceMethod::Tangent,
            1 => DesignRuledSurfaceMethod::Normal,
            2 => DesignRuledSurfaceMethod::Direction,
            _ => return None,
        };
        if bytes.get(start.checked_add(24)?..start.checked_add(27)?)? != [0; 3] {
            return None;
        }
        let alternate_face_offset = start.checked_add(27)?;
        let alternate_face = match bytes.get(alternate_face_offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let fixed_reference = |at: usize| {
            let mut cursor = at;
            let reference = take_reference(bytes, &mut cursor)?;
            (cursor == at.checked_add(11)?).then(|| u32::try_from(reference.local()?.0).ok())?
        };
        let angle_owner_record_index = fixed_reference(start.checked_add(28)?)?;
        let distance_owner_record_index = fixed_reference(start.checked_add(39)?)?;
        let corner_offset = start.checked_add(50)?;
        let corner = match View::u32_le_at(bytes, corner_offset)? {
            0 => DesignRuledSurfaceCorner::Rounded,
            1 => DesignRuledSurfaceCorner::Mitered,
            _ => return None,
        };
        let take_reference_list = |mut cursor: usize| {
            let count = usize::try_from(View::u32_le_at(bytes, cursor)?).ok()?;
            if count > 100_000 {
                return None;
            }
            cursor = cursor.checked_add(4)?;

            let mut records = Vec::new();
            for _ in 0..count {
                records.push(match fixed_reference(cursor) {
                    Some(record) => record,
                    None => return None,
                });
                cursor = cursor.checked_add(11)?;
            }
            Some(Ok((records, cursor)))
        };
        let (mut edge_group_record_indices, mut cursor) =
            match take_reference_list(start.checked_add(54)?)? {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            };
        if View::u32_le_at(bytes, cursor)? != 0 {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        let (auxiliary_record_indices, next) = match take_reference_list(cursor)? {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        };
        cursor = next;
        if View::u32_le_at(bytes, cursor)? != 0 {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        let (trailing_edge_groups, next) = match take_reference_list(cursor)? {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        };
        cursor = next;

        edge_group_record_indices.extend(trailing_edge_groups);
        let (direction_entity_id, direction_end) = match fixed_relaxed_guid_text(bytes, cursor) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let direction_absent =
            direction_entity_id.as_str() == "00000000-0000-0000-0000-000000000000";
        if direction_end.checked_add(3)? != reference_count_at
            || bytes.get(direction_end..reference_count_at)? != [0; 3]
            || paired_at <= reference_count_at
        {
            return None;
        }
        let direction_entity_id = if direction_absent {
            None
        } else {
            Some(direction_entity_id)
        };
        if reference_members.first() != Some(&distance_owner_record_index)
            || reference_members.get(1) != Some(&angle_owner_record_index)
            || edge_group_record_indices.is_empty()
            || edge_group_record_indices
                .iter()
                .any(|record_index| !reference_members.contains(record_index))
        {
            return None;
        }
        Some(Ok(DesignRuledSurfaceOperation {
            method,
            method_offset: u64_from_index(method_offset),
            corner,
            corner_offset: u64_from_index(corner_offset),
            alternate_face,
            alternate_face_offset: u64_from_index(alternate_face_offset),
            angle_owner_record_index,
            distance_owner_record_index,
            edge_group_record_indices,
            auxiliary_record_indices,
            direction_entity_id,
        }))
    })();
    parsed.transpose()
}
