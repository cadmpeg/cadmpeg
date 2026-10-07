// SPDX-License-Identifier: Apache-2.0
//! Dimension frames and their cross-record links.

use super::{design_stream, Ctx, ScopedRecordSet};
use crate::{design, records};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::check::{Check, Finding};
use std::collections::HashSet;

/// Validate dimension recipe records; returns the owned recipe ids.
pub(super) fn validate_dimension_recipe_records<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<ScopedRecordSet<'a, (&'a str, &'a str)>, CodecError> {
    if ctx.native.design_dimension_recipe_records.is_empty() {
        return Ok((
            HashSet::new(),
            ctx.decode
                .reserve_scoped(0, "hold F3D dimension recipe result")?,
        ));
    }

    let mut set_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D dimension recipe records result")?;
    let native = ctx.native;
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D dimension recipe scratch")?;
    let first_recipes_by_id = scratch_storage.with_storage(|| {
        ctx.decode.collect_hash_map(
            native
                .construction_recipes
                .iter()
                .rev()
                .map(|recipe| (recipe.id.as_str(), recipe)),
            "index F3D first dimension recipes",
        )
    })?;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let mut dimension_recipe_ids = HashSet::new();
    for record in ctx.decode.admit_iter(
        &native.design_dimension_recipe_records,
        "scan F3D design dimension recipe records",
    )? {
        let native_stream = design_stream(&record.id);
        let companion = ctx.decode.get_hash_map(
            companions_by_index,
            &(native_stream, record.companion_record_index),
            "find F3D validation record index",
        )?;
        let dimension_companion = match companion {
            Some(companion) => match ctx.decode.get_hash_map(
                owners_by_index,
                &(native_stream, companion.owner_record_index()),
                "find F3D dimension recipe owner",
            )? {
                Some(owner) => ctx
                    .decode
                    .get_hash_map(
                        parameters_by_index,
                        &(native_stream, owner.parameter_record_index()),
                        "find F3D dimension recipe parameter",
                    )?
                    .is_some_and(|parameter| {
                        parameter.kind() == records::parameters::DesignParameterKind::Dimension
                    }),
                None => false,
            },
            None => false,
        };
        let recipe = ctx
            .decode
            .get_hash_map(
                &first_recipes_by_id,
                record.recipe_id.as_str(),
                "find F3D dimension construction recipe",
            )?
            .copied();
        let companion_order_matches = match companion {
            Some(companion) => {
                let expected = usize::try_from(record.recipe_ordinal)
                    .ok()
                    .and_then(|ordinal| {
                        companion
                            .payload()
                            .and_then(|payload| payload.owned_recipe_ids().get(ordinal))
                    });
                match expected {
                    Some(expected) => ctx.decode.equal(
                        expected,
                        &record.recipe_id,
                        "compare F3D dimension companion recipe order",
                    )?,
                    None => false,
                }
            }
            None => false,
        };
        let frame_end = record.byte_offset.checked_add(record.frame_length);
        let prefix_end = record
            .prefix_offset
            .checked_add(u64_from_index(record.prefix_bytes.len()));
        let program_end = u64_from_index(record.program.len())
            .checked_mul(4)
            .and_then(|length| record.program_offset.checked_add(length));
        let (mut decoded_references, mut decoded_references_storage) = ctx
            .decode
            .with_scoped_storage("hold F3D validation recipe references", || {
                design::decode::dimension_frames::decode_recipe_references_charged(
                    ctx.decode,
                    &record.prefix_bytes,
                    record.prefix_offset,
                )
            })?;
        for reference in ctx.decode.admit_iter(
            &mut decoded_references,
            "scan F3D decoded dimension recipe references",
        )? {
            decoded_references_storage.with_storage(|| {
                design::decode::dimension_frames::bind_recipe_reference_candidates_charged(
                    ctx.decode,
                    reference,
                    &native.persistent_subentity_tags,
                    Some(&record.id),
                )
            })?;
        }
        let references_match = ctx.decode.equal(
            &decoded_references,
            &record.references,
            "compare F3D dimension recipe references",
        )?;
        let (matching_edge_operands, _matching_edge_operands_storage) = ctx
            .decode
            .with_scoped_storage("hold F3D dimension matching edge operands", || {
                design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
                    ctx.decode,
                    record,
                    &native.design_edge_operands,
                )
            })?;
        let edge_operands_match = ctx.decode.equal(
            &matching_edge_operands,
            &record.matching_edge_operand_ids,
            "compare F3D dimension matching edge operands",
        )?;
        let recipe_frame_matches = match recipe {
            Some(recipe) => {
                ctx.decode.equal(
                    &design_stream(&recipe.id),
                    &native_stream,
                    "compare F3D dimension recipe stream",
                )? && record
                    .byte_offset
                    .checked_add(11)
                    .is_some_and(|expected_offset| recipe.byte_offset >= expected_offset)
                    && frame_end.is_some_and(|end| recipe.byte_offset < end)
                    && prefix_end == recipe.byte_offset.checked_sub(4)
                    && recipe
                        .byte_offset
                        .checked_add(u64_from_index(design::construction_recipe_family_name_len(
                            recipe.kind,
                        )))
                        .is_some_and(|expected_offset| record.program_offset == expected_offset)
            }
            None => false,
        };
        let valid = record.frame_length >= 11
            && !record.prefix_bytes.is_empty()
            && references_match
            && edge_operands_match
            && record
                .byte_offset
                .checked_add(11)
                .is_some_and(|expected_offset| record.prefix_offset == expected_offset)
            && !record.program.is_empty()
            && record
                .byte_offset
                .checked_add(11)
                .is_some_and(|expected_offset| record.program_offset >= expected_offset)
            && program_end == frame_end
            && dimension_companion
            && companion_order_matches
            && recipe_frame_matches
            && set_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut dimension_recipe_ids,
                    (native_stream, record.recipe_id.as_str()),
                    "index F3D dimension recipe IDs",
                )
            })?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension recipe has an invalid indexed-record owner",
                Some(
                    ctx.decode
                        .copy_retained_text(&record.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok((dimension_recipe_ids, set_storage))
}

/// Report dimension companions owning an unresolved construction recipe.
pub(super) fn validate_dimension_companion_recipes<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    dimension_recipe_ids: &HashSet<(&'a str, &'a str)>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    for companion in ctx.decode.admit_iter(
        &native.design_parameter_companions,
        "scan F3D design parameter companions",
    )? {
        let native_stream = design_stream(companion.id());
        let dimension_companion = match ctx.decode.get_hash_map(
            owners_by_index,
            &(native_stream, companion.owner_record_index()),
            "find F3D dimension companion owner",
        )? {
            Some(owner) => ctx
                .decode
                .get_hash_map(
                    parameters_by_index,
                    &(native_stream, owner.parameter_record_index()),
                    "find F3D dimension companion parameter",
                )?
                .is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                }),
            None => false,
        };
        if dimension_companion
            && match companion.payload() {
                Some(payload) => ctx.decode.any_by(
                    payload.owned_recipe_ids(),
                    |recipe_id| {
                        Ok(!ctx.decode.contains_hash_set(
                            dimension_recipe_ids,
                            &(native_stream, recipe_id.as_str()),
                            "find F3D owned dimension recipe identity",
                        )?)
                    },
                    "scan F3D dimension companion recipe identities",
                )?,
                None => false,
            }
        {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension companion has an unowned construction recipe",
                Some(
                    ctx.decode
                        .copy_retained_text(companion.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate dimension locus pairs; returns their companion set.
pub(super) fn validate_dimension_locus_pairs<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<ScopedRecordSet<'a, (&'a str, u32)>, CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D dimension locus pairs scratch")?;
    let mut set_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D dimension locus pairs result")?;
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut locus_pair_indices = HashSet::new();
    let mut locus_pair_companions = HashSet::new();
    let governing = design::decode::dimension_frames::GoverningCompanions::build(
        ctx.decode,
        &native.design_parameter_owners,
        &native.design_parameters,
    )?;
    for pair in ctx.decode.admit_iter(
        &*native.design_dimension_locus_pairs,
        "scan F3D design dimension locus pairs",
    )? {
        let native_stream = design_stream(&pair.id);
        let unique_index = scratch_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut locus_pair_indices,
                (native_stream, pair.record_index),
                "index F3D dimension locus pairs",
            )
        })?;
        let unique_companion = set_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut locus_pair_companions,
                (native_stream, pair.companion_record_index),
                "index F3D dimension locus pair companions",
            )
        })?;
        let companion = ctx.decode.get_hash_map(
            companions_by_index,
            &(native_stream, pair.companion_record_index),
            "find F3D validation record index",
        )?;
        let companion_contains_frame = match companion {
            Some(companion) => {
                companion
                    .byte_offset()
                    .checked_add(58)
                    .is_some_and(|expected_offset| pair.byte_offset() >= expected_offset)
                    && !ctx.decode.any_by(
                        &native.design_parameter_owners,
                        |owner| {
                            Ok(ctx.decode.equal(
                                &design_stream(owner.id()),
                                &native_stream,
                                "compare F3D dimension locus owner stream",
                            )? && owner.byte_offset() > companion.byte_offset()
                                && owner.byte_offset() <= pair.byte_offset())
                        },
                        "scan F3D dimension locus owner intervals",
                    )?
            }
            None => false,
        };
        let dimension_companion = match companion {
            Some(companion) => match ctx.decode.get_hash_map(
                owners_by_index,
                &(native_stream, companion.owner_record_index()),
                "find F3D locus companion owner",
            )? {
                Some(owner) => ctx
                    .decode
                    .get_hash_map(
                        parameters_by_index,
                        &(native_stream, owner.parameter_record_index()),
                        "find F3D locus companion parameter",
                    )?
                    .is_some_and(|parameter| {
                        parameter.kind() == records::parameters::DesignParameterKind::Dimension
                    }),
                None => false,
            },
            None => false,
        };
        let governs_following_dimension =
            governing.governing(ctx.decode, &pair.id, pair.paired_byte_offset())?
                == Some(pair.governing_companion_record_index);
        let valid = companion_contains_frame
            && dimension_companion
            && governs_following_dimension
            && ctx.decode.contains_hash_set(
                sketch_geometry_indices,
                &(native_stream, pair.loci()[0].geometry_index()),
                "find F3D dimension locus geometry",
            )?
            && ctx.decode.contains_hash_set(
                sketch_geometry_indices,
                &(native_stream, pair.loci()[1].geometry_index()),
                "find F3D dimension locus geometry",
            )?
            && unique_index
            && unique_companion;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension locus pair has an invalid frame or geometry link",
                Some(
                    ctx.decode
                        .copy_retained_text(&pair.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok((locus_pair_companions, set_storage))
}

/// Validate dimension annotation frames and their operand runs.
pub(super) fn validate_dimension_annotation_frames(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D dimension annotation frames scratch")?;
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut annotation_frame_indices = HashSet::new();
    for frame in ctx.decode.admit_iter(
        &native.design_dimension_annotation_frames,
        "scan F3D design dimension annotation frames",
    )? {
        let native_stream = design_stream(&frame.id);
        let unique_index = scratch_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut annotation_frame_indices,
                (native_stream, frame.record_index),
                "index F3D dimension annotation frames",
            )
        })?;
        let governing_owner = ctx
            .decode
            .get_hash_map(
                owners_by_index,
                &(native_stream, frame.governing_owner_record_index),
                "find F3D dimension annotation owner",
            )?
            .copied();
        let physical_interval_valid = match frame.companion_record_index {
            Some(record_index) => ctx
                .decode
                .get_hash_map(
                    companions_by_index,
                    &(native_stream, record_index),
                    "find F3D annotation companion interval",
                )?
                .is_some_and(|companion| {
                    companion
                        .byte_offset()
                        .checked_add(58)
                        .is_some_and(|expected_offset| frame.byte_offset() >= expected_offset)
                        && companion.payload().is_some_and(|payload| {
                            companion
                                .byte_offset()
                                .checked_add(58)
                                .and_then(|offset| offset.checked_add(payload.byte_length()))
                                .is_some_and(|expected_offset| {
                                    frame.paired_byte_offset() < expected_offset
                                })
                        })
                }),
            None => match governing_owner {
                Some(owner) => {
                    let scope_contains_frame = ctx
                        .decode
                        .get_hash_map(
                            scopes_by_index,
                            &(native_stream, owner.scope_record_index()),
                            "find F3D annotation interval scope",
                        )?
                        .is_some_and(|scope| frame.byte_offset() >= scope.byte_offset());
                    if scope_contains_frame {
                        let end = ctx.decode.fold(
                            &native.design_parameter_owners,
                            None::<u64>,
                            |end, candidate| {
                                if !ctx.decode.equal(
                                    &design_stream(candidate.id()),
                                    &native_stream,
                                    "compare F3D annotation interval stream",
                                )? || candidate.scope_record_index()
                                    != owner.scope_record_index()
                                {
                                    return Ok(end);
                                }
                                let Some(companion) = ctx.decode.get_hash_map(
                                    companions_by_index,
                                    &(native_stream, candidate.companion_record_index()),
                                    "find F3D annotation interval companion",
                                )?
                                else {
                                    return Ok(end);
                                };
                                let offset = companion.byte_offset();
                                Ok(Some(match end {
                                    Some(end) => end.min(offset),
                                    None => offset,
                                }))
                            },
                            "find F3D annotation interval end",
                        )?;
                        end.is_some_and(|end| frame.paired_byte_offset() < end)
                    } else {
                        false
                    }
                }
                None => false,
            },
        };
        let governing_link_valid = match governing_owner {
            Some(owner) => {
                owner.companion_record_index() == frame.governing_companion_record_index
                    && ctx
                        .decode
                        .get_hash_map(
                            parameters_by_index,
                            &(native_stream, owner.parameter_record_index()),
                            "find F3D annotation governing parameter",
                        )?
                        .is_some_and(|parameter| {
                            parameter.kind() == records::parameters::DesignParameterKind::Dimension
                        })
            }
            None => false,
        };
        let operands_valid = ctx.decode.all_by(
            frame.operands(),
            |operand| match operand.geometry_record_index {
                Some(index) => ctx.decode.contains_hash_set(
                    sketch_geometry_indices,
                    &(native_stream, index.get()),
                    "find F3D annotation operand geometry",
                ),
                None => Ok(true),
            },
            "scan F3D annotation operands",
        )?;
        let owner_is_sketch = ctx
            .decode
            .get_hash_map(
                entities_by_suffix,
                &(native_stream, u64::from(frame.owner_reference)),
                "find F3D annotation sketch owner",
            )?
            .is_some_and(|entity| entity.in_sketch_module());
        let valid = unique_index
            && physical_interval_valid
            && governing_link_valid
            && operands_valid
            && owner_is_sketch;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension annotation frame has invalid links or offsets",
                Some(
                    ctx.decode
                        .copy_retained_text(&frame.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate direct dimension presentation frames and their sketch-owner joins.
pub(super) fn validate_dimension_presentation_frames(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D dimension presentation frames scratch")?;
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let sketch_scope_by_entity = scratch_storage.with_storage(|| {
        ctx.decode.collect_hash_map(
            ctx.decode
                .admit_iter(
                    &native.design_sketch_placements,
                    "admit source for index F3D dimension presentation sketch scopes",
                )?
                .filter_map(|placement| {
                    Some((
                        (design_stream(&placement.id), placement.entity_id.suffix()),
                        placement.scope_record_index?,
                    ))
                }),
            "index F3D dimension presentation sketch scopes",
        )
    })?;
    let mut presentation_frame_indices = HashSet::new();
    for frame in ctx.decode.admit_iter(
        &native.design_dimension_presentation_frames,
        "scan F3D design dimension presentation frames",
    )? {
        let native_stream = design_stream(&frame.id);
        let unique_index = scratch_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut presentation_frame_indices,
                (native_stream, frame.record_index),
                "index F3D dimension presentation frames",
            )
        })?;
        let owner = ctx.decode.get_hash_map(
            owners_by_index,
            &(native_stream, frame.governing_owner_record_index),
            "find F3D validation record index",
        )?;
        let parameter = ctx.decode.get_hash_map(
            parameters_by_index,
            &(native_stream, frame.governing_parameter_record_index),
            "find F3D presentation governing parameter",
        )?;
        let companion = ctx.decode.get_hash_map(
            companions_by_index,
            &(native_stream, frame.governing_companion_record_index),
            "find F3D presentation governing companion",
        )?;
        let owner_link_valid = owner.is_some_and(|owner| {
            owner.parameter_record_index() == frame.governing_parameter_record_index
                && owner.companion_record_index() == frame.governing_companion_record_index
                && parameter.is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                })
                && companion
                    .is_some_and(|companion| companion.owner_record_index() == owner.record_index())
        });
        let nearest_owner = match ctx.decode.get_hash_map(
            &sketch_scope_by_entity,
            &(native_stream, u64::from(frame.owner_reference)),
            "find F3D presentation sketch scope",
        )? {
            Some(scope_record_index) => ctx.decode.fold(
                ctx.scope_owners(
                    native_stream,
                    *scope_record_index,
                    "find F3D presentation scope owners",
                )?,
                None::<&records::parameters::DesignParameterOwner>,
                |nearest, candidate| {
                    if candidate.byte_offset() <= frame.paired_byte_offset
                        || !ctx
                            .decode
                            .get_hash_map(
                                parameters_by_index,
                                &(native_stream, candidate.parameter_record_index()),
                                "find F3D presentation owner parameter",
                            )?
                            .is_some_and(|parameter| {
                                parameter.kind()
                                    == records::parameters::DesignParameterKind::Dimension
                            })
                    {
                        return Ok(nearest);
                    }
                    Ok(Some(match nearest {
                        Some(previous) if previous.byte_offset() <= candidate.byte_offset() => {
                            previous
                        }
                        _ => *candidate,
                    }))
                },
                "find F3D nearest presentation owner",
            )?,
            None => None,
        };
        let governing_owner_is_nearest = nearest_owner.is_some_and(|candidate| {
            candidate.record_index() == frame.governing_owner_record_index
        });
        let operand_start = frame.byte_offset.checked_add(24);
        let operands_valid = if frame.operands.is_empty() {
            false
        } else {
            ctx.decode.all_by(
                frame.operands.iter().enumerate(),
                |(ordinal, operand)| {
                    let Some(start) = u64_from_index(ordinal).checked_mul(15).and_then(|delta| {
                        operand_start.and_then(|offset| offset.checked_add(delta))
                    }) else {
                        return Ok(false);
                    };
                    if !(start.checked_add(1).is_some_and(|expected_offset| {
                        operand.geometry_reference_offset == expected_offset
                    }) && start
                        .checked_add(11)
                        .is_some_and(|expected_offset| operand.role_offset == expected_offset)
                        && ctx.decode.contains_hash_set(
                            sketch_geometry_indices,
                            &(native_stream, operand.geometry_record_index.get()),
                            "find F3D presentation operand geometry",
                        )?)
                    {
                        return Ok(false);
                    }

                    Ok(true)
                },
                "scan F3D presentation operands",
            )?
        };
        let owner_is_sketch = ctx
            .decode
            .get_hash_map(
                entities_by_suffix,
                &(native_stream, u64::from(frame.owner_reference)),
                "find F3D presentation sketch owner",
            )?
            .is_some_and(|entity| entity.in_sketch_module());
        let valid = unique_index
            && frame.paired_byte_offset > frame.byte_offset
            && frame
                .paired_byte_offset
                .checked_sub(frame.byte_offset)
                .is_some_and(|expected_offset| frame.frame_length == expected_offset)
            && (u64_from_index(frame.operands.len()))
                .checked_mul(15)
                .and_then(|delta| operand_start.and_then(|offset| offset.checked_add(delta)))
                .is_some_and(|expected_offset| frame.presentation_byte_offset == expected_offset)
            && frame
                .presentation_byte_offset
                .checked_add(u64_from_index(frame.presentation_bytes.len()))
                .is_some_and(|expected_offset| expected_offset == frame.paired_byte_offset)
            && frame
                .paired_byte_offset
                .checked_add(20)
                .is_some_and(|expected_offset| frame.owner_reference_offset == expected_offset)
            && owner_link_valid
            && governing_owner_is_nearest
            && operands_valid
            && owner_is_sketch;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension presentation frame has invalid links or offsets",
                Some(
                    ctx.decode
                        .copy_retained_text(&frame.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate dimension locus groups; returns their companion set.
pub(super) fn validate_dimension_locus_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<ScopedRecordSet<'a, (&'a str, u32)>, CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D dimension locus groups scratch")?;
    let mut set_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D dimension locus groups result")?;
    let decode: &DecodeContext<'_> = ctx.decode;
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut locus_group_indices = HashSet::new();
    let mut locus_group_companions = HashSet::new();
    for group in ctx.decode.admit_iter(
        &native.design_dimension_locus_groups,
        "scan F3D design dimension locus groups",
    )? {
        let native_stream = design_stream(&group.id);
        let unique_index = scratch_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut locus_group_indices,
                (native_stream, group.record_index),
                "index F3D dimension locus groups",
            )
        })?;
        set_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut locus_group_companions,
                (native_stream, group.companion_record_index),
                "index F3D dimension locus group companions",
            )
        })?;
        let companion = ctx.decode.get_hash_map(
            companions_by_index,
            &(native_stream, group.companion_record_index),
            "find F3D validation record index",
        )?;
        let companion_contains_frame = match companion {
            Some(companion) => {
                companion
                    .byte_offset()
                    .checked_add(58)
                    .is_some_and(|expected_offset| group.byte_offset >= expected_offset)
                    && !ctx.decode.any_by(
                        &native.design_parameter_owners,
                        |owner| {
                            Ok(ctx.decode.equal(
                                &design_stream(owner.id()),
                                &native_stream,
                                "compare F3D dimension locus owner stream",
                            )? && owner.byte_offset() > companion.byte_offset()
                                && owner.byte_offset() <= group.byte_offset)
                        },
                        "scan F3D dimension locus owner intervals",
                    )?
            }
            None => false,
        };
        let dimension_companion = match companion {
            Some(companion) => match ctx.decode.get_hash_map(
                owners_by_index,
                &(native_stream, companion.owner_record_index()),
                "find F3D locus companion owner",
            )? {
                Some(owner) => ctx
                    .decode
                    .get_hash_map(
                        parameters_by_index,
                        &(native_stream, owner.parameter_record_index()),
                        "find F3D locus companion parameter",
                    )?
                    .is_some_and(|parameter| {
                        parameter.kind() == records::parameters::DesignParameterKind::Dimension
                    }),
                None => false,
            },
            None => false,
        };
        let count = group.loci.len();
        let loci_start = group.byte_offset.checked_add(24);
        let loci_offsets_valid = {
            ctx.decode.all_by(
                group.loci.iter().enumerate(),
                |(ordinal, locus)| {
                    let Some(start) = u64_from_index(ordinal)
                        .checked_mul(15)
                        .and_then(|delta| loci_start.and_then(|offset| offset.checked_add(delta)))
                    else {
                        return Ok(false);
                    };
                    if !(start.checked_add(1).is_some_and(|expected_offset| {
                        locus.geometry_reference_offset == expected_offset
                    }) && start
                        .checked_add(11)
                        .is_some_and(|expected_offset| locus.role_offset == expected_offset)
                        && ctx.decode.contains_hash_set(
                            sketch_geometry_indices,
                            &(native_stream, locus.geometry_record_index),
                            "find F3D counted dimension locus geometry",
                        )?)
                    {
                        return Ok(false);
                    }

                    Ok(true)
                },
                "scan F3D dimension locus offsets",
            )?
        };
        let owner_start = u64_from_index(count)
            .checked_mul(15)
            .and_then(|delta| loci_start.and_then(|offset| offset.checked_add(delta)));
        let returns_start = owner_start.and_then(|offset| offset.checked_add(24));
        let returns_valid = {
            ctx.decode.all_by(
                group.loci.iter().enumerate(),
                |(ordinal, locus)| {
                    if !(u64_from_index(ordinal)
                        .checked_mul(11)
                        .and_then(|delta| {
                            returns_start.and_then(|offset| offset.checked_add(delta))
                        })
                        .and_then(|offset| offset.checked_add(1))
                        .is_some_and(|expected_offset| locus.returned.offset == expected_offset)
                        && ctx.decode.contains_hash_set(
                            sketch_geometry_indices,
                            &(native_stream, locus.returned.value),
                            "find F3D returned dimension locus geometry",
                        )?)
                    {
                        return Ok(false);
                    }

                    Ok(true)
                },
                "scan F3D dimension locus returns",
            )?
        };
        let (mut locus_members, _locus_members_storage) =
            ctx.decode
                .with_scoped_storage("hold F3D sorted locus members", || {
                    ctx.decode.collect_vec(
                        group.loci.iter().map(|locus| locus.geometry_record_index),
                        "collect F3D dimension locus members",
                    )
                })?;
        let (mut return_members, _return_members_storage) =
            ctx.decode
                .with_scoped_storage("hold F3D sorted locus return members", || {
                    ctx.decode.collect_vec(
                        group.loci.iter().map(|locus| locus.returned.value),
                        "collect F3D dimension return members",
                    )
                })?;
        decode.sort_unstable_by(
            &mut locus_members,
            |value| value,
            Ord::cmp,
            "f3d dimension locus members sort",
        )?;
        decode.sort_unstable_by(
            &mut return_members,
            |value| value,
            Ord::cmp,
            "f3d dimension return members sort",
        )?;
        let owner_is_sketch = ctx
            .decode
            .get_hash_map(
                entities_by_suffix,
                &(native_stream, u64::from(group.owner_reference)),
                "find F3D counted locus sketch owner",
            )?
            .is_some_and(|entity| entity.in_sketch_module());
        let frame_does_not_overlap = ctx.decode.all_by(
            &native.design_dimension_locus_groups,
            |other| {
                Ok(!ctx.decode.equal(
                    &design_stream(&other.id),
                    &native_stream,
                    "compare F3D counted locus frame stream",
                )? || other.companion_record_index != group.companion_record_index
                    || other.record_index == group.record_index
                    || group.next_byte_offset <= other.byte_offset
                    || other.next_byte_offset <= group.byte_offset)
            },
            "scan F3D counted locus frame overlap",
        )?;
        let valid = companion_contains_frame
            && dimension_companion
            && (1..=64).contains(&count)
            && loci_offsets_valid
            && owner_start
                .and_then(|offset| offset.checked_add(2))
                .is_some_and(|expected_offset| group.owner_reference_offset == expected_offset)
            && owner_start
                .and_then(|offset| offset.checked_add(12))
                .is_some_and(|expected_offset| group.owner_role_offset == expected_offset)
            && owner_start
                .and_then(|offset| offset.checked_add(16))
                .is_some_and(|expected_offset| group.state_offset == expected_offset)
            && owner_is_sketch
            && returns_valid
            && ctx.decode.equal(
                &locus_members,
                &return_members,
                "compare F3D dimension locus return members",
            )?
            && (u64_from_index(count))
                .checked_mul(11)
                .and_then(|delta| returns_start.and_then(|offset| offset.checked_add(delta)))
                .and_then(|offset| offset.checked_add(1))
                .is_some_and(|expected_offset| group.next_byte_offset == expected_offset)
            && group
                .next_byte_offset
                .checked_sub(group.byte_offset)
                .is_some_and(|expected_offset| group.frame_length == expected_offset)
            && unique_index
            && frame_does_not_overlap;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension locus group has an invalid counted frame or geometry link",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok((locus_group_companions, set_storage))
}

/// Validate null-locus dimension pairs against typed companions.
pub(super) fn validate_dimension_null_locus_pairs<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    locus_pair_companions: &HashSet<(&'a str, u32)>,
    locus_group_companions: &HashSet<(&'a str, u32)>,
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D dimension null locus pairs scratch")?;
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut null_locus_pair_indices = HashSet::new();
    let mut null_locus_pair_companions = HashSet::new();
    let governing = design::decode::dimension_frames::GoverningCompanions::build(
        ctx.decode,
        &native.design_parameter_owners,
        &native.design_parameters,
    )?;
    for pair in ctx.decode.admit_iter(
        &*native.design_dimension_null_locus_pairs,
        "scan F3D design dimension null locus pairs",
    )? {
        let native_stream = design_stream(&pair.id);
        let unique_index = scratch_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut null_locus_pair_indices,
                (native_stream, pair.record_index),
                "index F3D null-locus dimension pairs",
            )
        })?;
        let unique_companion = scratch_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut null_locus_pair_companions,
                (native_stream, pair.companion_record_index),
                "index F3D null-locus dimension companions",
            )
        })?;
        let companion = ctx.decode.get_hash_map(
            companions_by_index,
            &(native_stream, pair.companion_record_index),
            "find F3D validation record index",
        )?;
        let companion_contains_frame = match companion {
            Some(companion) => {
                companion
                    .byte_offset()
                    .checked_add(58)
                    .is_some_and(|expected_offset| pair.byte_offset() >= expected_offset)
                    && !ctx.decode.any_by(
                        &native.design_parameter_owners,
                        |owner| {
                            Ok(ctx.decode.equal(
                                &design_stream(owner.id()),
                                &native_stream,
                                "compare F3D dimension locus owner stream",
                            )? && owner.byte_offset() > companion.byte_offset()
                                && owner.byte_offset() <= pair.byte_offset())
                        },
                        "scan F3D dimension locus owner intervals",
                    )?
            }
            None => false,
        };
        let dimension_companion = match companion {
            Some(companion) => match ctx.decode.get_hash_map(
                owners_by_index,
                &(native_stream, companion.owner_record_index()),
                "find F3D locus companion owner",
            )? {
                Some(owner) => ctx
                    .decode
                    .get_hash_map(
                        parameters_by_index,
                        &(native_stream, owner.parameter_record_index()),
                        "find F3D locus companion parameter",
                    )?
                    .is_some_and(|parameter| {
                        parameter.kind() == records::parameters::DesignParameterKind::Dimension
                    }),
                None => false,
            },
            None => false,
        };
        let governs_following_dimension =
            governing.governing(ctx.decode, &pair.id, pair.paired_byte_offset())?
                == Some(pair.governing_companion_record_index);
        let companion_has_typed_frame = ctx.decode.contains_hash_set(
            locus_pair_companions,
            &(native_stream, pair.companion_record_index),
            "find F3D typed locus pair companion",
        )? || ctx.decode.contains_hash_set(
            locus_group_companions,
            &(native_stream, pair.companion_record_index),
            "find F3D typed locus group companion",
        )?;
        let valid = companion_contains_frame
            && dimension_companion
            && governs_following_dimension
            && !companion_has_typed_frame
            && ctx.decode.contains_hash_set(
                sketch_geometry_indices,
                &(native_stream, pair.loci()[1].geometry_index()),
                "find F3D null dimension locus geometry",
            )?
            && unique_index
            && unique_companion;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design null-locus dimension pair has an invalid frame or geometry link",
                Some(
                    ctx.decode
                        .copy_retained_text(&pair.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
