// SPDX-License-Identifier: Apache-2.0
//! Exact assembly alignment scopes.

use super::assembly_operand_frames::exact_as_built_operand_frames;
use super::assembly_operand_frames::exact_assembly_operand_frames;
use super::assembly_operand_paths::exact_assembly_operand_paths;
use super::legacy_operand_paths::exact_legacy_class_383_operand_paths;
use super::legacy_operand_paths::exact_legacy_class_388_operand_paths;
use super::legacy_operand_paths::exact_legacy_class_388_scope;
use super::legacy_operand_paths::CLASS_383_OWNER_REFERENCE_ORDINALS;
use super::legacy_operand_paths::CLASS_388_OWNER_REFERENCE_ORDINALS;
use crate::design::assembly::{AssemblyOperandFrameVariant, AssemblyScopeGeneration};
use crate::design::decode::assembly::exact_legacy_as_built_421_alignment;
use crate::design::decode::assembly::exact_legacy_as_built_421_solved_frame;
use crate::design::decode::operands::reference_at;
use crate::design::decode::record_streams::{in_stream, record_stream};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::records::feature::assembly;
use crate::records::feature::assembly::DesignAssemblyAlignment;
use crate::records::feature::assembly::DesignAssemblyOperandFrame;
use crate::records::feature::assembly::DesignAssemblyOperandPath;
use crate::records::feature::assembly::DesignAssemblyOperandQualifier;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::{Located, ReferenceRun};
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

struct AsBuiltAlignmentDraft {
    paths: [DesignAssemblyOperandPath; 2],
    frames: Option<[DesignAssemblyOperandFrame; 2]>,
}

/// Whether the scope reference at each ordinal of the fixed table names the
/// lane owner in the same position.
fn scope_references_match_owner_ordinals<const N: usize>(
    scope: &DesignParameterScope,
    ordinals: &[usize; N],
    lane_owners: &[&DesignParameterOwner],
) -> bool {
    ordinals.iter().zip(lane_owners).all(|(ordinal, owner)| {
        reference_at(scope.reference_members(), *ordinal) == Some(owner.record_index())
    })
}

/// The number of positions at which `references` repeats the owner run.
fn owner_run_count(
    ctx: &DecodeContext<'_>,
    references: &ReferenceRun<u32>,
    owners: &[Located<u32>],
) -> Result<usize, CodecError> {
    const OPERATION: &str = "scan F3D alignment reference positions";
    let Some(width) = std::num::NonZeroUsize::new(owners.len()) else {
        return Ok(0);
    };
    let owner_values = || owners.iter().map(|owner| &owner.value);
    Ok(
        match (references.unlocated_values(), references.located_rows()) {
            (Some(values), _) => ctx
                .admit_iter(values, OPERATION)?
                .windows(width)
                .filter(|window| window.iter().eq(owner_values()))
                .count(),
            (None, Some(rows)) => ctx
                .admit_iter(rows, OPERATION)?
                .windows(width)
                .filter(|window| window.iter().map(|row| &row.value).eq(owner_values()))
                .count(),
            (None, None) => 0,
        },
    )
}

impl TryFrom<AsBuiltAlignmentDraft> for assembly::DesignAssemblyAlignmentForm {
    type Error = &'static str;

    fn try_from(draft: AsBuiltAlignmentDraft) -> Result<Self, Self::Error> {
        Ok(Self::qualified(
            draft.frames.ok_or("as-built operand_frames are required")?,
            draft
                .paths
                .map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path }),
        ))
    }
}

fn occurrence_paths(paths: [DesignAssemblyOperandPath; 2]) -> [DesignAssemblyOperandQualifier; 2] {
    paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path })
}

pub(super) fn exact_assembly_alignment(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<DesignAssemblyAlignment>, CodecError> {
    use assembly::DesignAssemblyAlignmentForm;
    if !matches!(
        scope.payload(),
        scope::DesignScopePayload::Assemble(_) | scope::DesignScopePayload::AsBuilt(_)
    ) {
        return Ok(None);
    }
    let Some(stream) = record_stream(ctx, &scope.id)? else {
        return Ok(None);
    };
    let mut lane_storage = ctx.reserve_scoped(0, "f3d assembly alignment lanes")?;
    let mut lanes = Vec::new();
    for owner in ctx.admit_iter(parameter_owners, "scan F3D assembly alignment owners")? {
        if owner.scope_record_index() == scope.record_index && in_stream(ctx, owner.id(), stream)? {
            ctx.push_scoped_vec(
                &mut lane_storage,
                &mut lanes,
                owner,
                "f3d assembly alignment lanes",
            )?;
        }
    }
    ctx.stable_sort_by_key(
        &mut lanes[..],
        |owner| owner.local_ordinal(),
        Ord::cmp,
        "sort F3D assembly alignment lanes",
    )?;
    let mut ordinal = 0_usize;
    if ctx.any_by(
        &lanes,
        |owner| {
            let out_of_place = u32::try_from(ordinal) != Ok(owner.local_ordinal());
            ordinal += 1;
            Ok(out_of_place)
        },
        "check F3D assembly alignment lane ordinals",
    )? {
        return Ok(None);
    }
    let class_tags = (scope.class_tag.as_str(), scope.paired_class_tag.as_str());
    let as_built_421 = crate::design::assembly::legacy_as_built_421_generation(
        scope.frame_length(),
        class_tags.0,
        class_tags.1,
    )
    .is_some();
    let legacy_class_383 = crate::design::assembly::legacy_class_383_258_scope(
        scope.frame_length(),
        class_tags.0,
        class_tags.1,
    );
    let generation = AssemblyScopeGeneration::new(scope.frame_length(), class_tags.0, class_tags.1);
    let legacy_class_388 =
        generation.operand_frame_variant() == Some(AssemblyOperandFrameVariant::LegacyClass388);
    let variable_reference =
        crate::design::assembly::variable_reference_assembly_generation(class_tags.0, class_tags.1);
    if legacy_class_388 && exact_legacy_class_388_scope(bytes, scope).is_none() {
        return Ok(None);
    }

    if as_built_421 {
        let Some(exact) = exact_legacy_as_built_421_alignment(ctx, bytes, scope, &lanes)? else {
            return Ok(None);
        };
        let form = match exact_legacy_as_built_421_solved_frame(ctx, bytes, records, scope)? {
            Some(solved_frame) => DesignAssemblyAlignmentForm::SolvedOnly {
                solved_frame,
                limits: Some(exact.limits),
            },
            None => DesignAssemblyAlignmentForm::LimitsOnly {
                limits: exact.limits,
            },
        };
        return Ok(DesignAssemblyAlignment::try_new(
            exact.angle,
            exact.offset,
            exact.owners,
            Some(form),
        )
        .ok());
    }
    if matches!(scope.frame_length(), 671 | 744 | 748)
        && generation.operand_frame_variant().is_none()
    {
        return Ok(None);
    }
    let Some(alignment_lanes) = generation
        .alignment_lane_bounds(lanes.len())
        .and_then(|(alignment_start, alignment_end)| lanes.get(alignment_start..alignment_end))
    else {
        return Ok(None);
    };
    let (angle, offset) = match alignment_lanes {
        [angle, offset_x, offset_y, offset_z] => (
            angle.evaluated_value().get(),
            [
                offset_x.evaluated_value().get(),
                offset_y.evaluated_value().get(),
                offset_z.evaluated_value().get(),
            ],
        ),
        [angle, axial_offset] => (
            angle.evaluated_value().get(),
            [0.0, 0.0, axial_offset.evaluated_value().get()],
        ),
        _ => return Ok(None),
    };
    // Both supported alignment lanes use one fixed four-owner allocation.
    let mut owners = ctx.vector_storage(4, "collect F3D assembly alignment owners")?;
    for owner in alignment_lanes {
        ctx.push_vec(
            &mut owners,
            Located {
                value: owner.record_index(),
                offset: owner.evaluated_value_offset(),
            },
            "collect F3D assembly alignment owners",
        )?;
    }
    let owner_values = || owners.iter().map(|owner| &owner.value);
    let lane_class_differs = |class_tag: &'static [u8; 3], operation| {
        ctx.any_by(
            &lanes,
            |owner| {
                Ok(owner.class_tag().as_str().as_bytes() != class_tag
                    || owner.frame_length() != 103)
            },
            operation,
        )
    };
    let owners_match = if legacy_class_388 {
        scope_references_match_owner_ordinals(scope, &CLASS_388_OWNER_REFERENCE_ORDINALS, &lanes)
            && !lane_class_differs(b"282", "scan F3D class-388 alignment lanes")?
            && scope
                .reference_members()
                .values()
                .skip(4)
                .take(4)
                .eq(owner_values())
    } else if legacy_class_383 {
        scope_references_match_owner_ordinals(scope, &CLASS_383_OWNER_REFERENCE_ORDINALS, &lanes)
            && !lane_class_differs(b"284", "scan F3D class-383 alignment lanes")?
            && scope
                .reference_members()
                .values()
                .skip(8)
                .take(4)
                .eq(owner_values())
    } else if variable_reference {
        !lane_class_differs(b"289", "scan F3D variable alignment lanes")?
            && owner_run_count(ctx, scope.reference_members(), &owners)? == 1
    } else {
        scope
            .reference_members()
            .values()
            .rev()
            .take(owners.len())
            .eq(owner_values().rev())
    };
    if !owners_match {
        return Ok(None);
    }

    let form = if matches!(scope.payload(), scope::DesignScopePayload::AsBuilt(_)) {
        match exact_assembly_operand_paths(ctx, bytes, records, scope)? {
            Some(paths) => {
                let frames = exact_as_built_operand_frames(bytes, &paths);
                let Ok(form) =
                    DesignAssemblyAlignmentForm::try_from(AsBuiltAlignmentDraft { paths, frames })
                else {
                    return Ok(None);
                };
                Some(form)
            }
            None => None,
        }
    } else if let Some(frames) = exact_assembly_operand_frames(bytes, scope) {
        let qualifiers = if legacy_class_383 {
            exact_legacy_class_383_operand_paths(ctx, bytes, records, scope, &frames)?
                .map(occurrence_paths)
        } else if legacy_class_388 {
            exact_legacy_class_388_operand_paths(ctx, bytes, records, scope)?.map(occurrence_paths)
        } else if variable_reference {
            match super::assembly_carrier_paths::exact_variable_reference_operand_qualifiers(
                ctx, bytes, records, scope, &frames,
            )? {
                Some(direct) => Some(direct),
                None => {
                    exact_assembly_operand_paths(ctx, bytes, records, scope)?.map(occurrence_paths)
                }
            }
        } else {
            exact_assembly_operand_paths(ctx, bytes, records, scope)?.map(occurrence_paths)
        };
        Some(match qualifiers {
            Some(qualifiers) => DesignAssemblyAlignmentForm::qualified(frames, qualifiers),
            None => DesignAssemblyAlignmentForm::Frames { frames },
        })
    } else {
        None
    };
    Ok(DesignAssemblyAlignment::try_new(angle, offset, owners, form).ok())
}

#[cfg(test)]
mod tests;
