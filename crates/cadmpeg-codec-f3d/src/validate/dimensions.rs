// SPDX-License-Identifier: Apache-2.0
//! Validate Design dimension recipes, locus pairs and groups, and their frames.

use super::{design_stream, push_constant_finding, Ctx};
use crate::{design, records};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::report::check::{Check, Finding};
use std::collections::HashSet;

/// Validate dimension recipe records; returns the owned recipe ids.
pub(super) fn validate_dimension_recipe_records<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, &'a str)> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let mut dimension_recipe_ids = HashSet::new();
    for record in &native.design_dimension_recipe_records {
        let native_stream = design_stream(&record.id);
        let companion = companions_by_index.get(&(native_stream, record.companion_record_index));
        let dimension_companion = companion.is_some_and(|companion| {
            owners_by_index
                .get(&(native_stream, companion.owner_record_index()))
                .and_then(|owner| {
                    parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                })
                .is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                })
        });
        let recipe = native
            .construction_recipes
            .iter()
            .find(|recipe| recipe.id == record.recipe_id);
        let companion_order_matches = companion.is_some_and(|companion| {
            usize::try_from(record.recipe_ordinal)
                .ok()
                .and_then(|ordinal| {
                    companion
                        .payload()
                        .and_then(|payload| payload.owned_recipe_ids().get(ordinal))
                })
                == Some(&record.recipe_id)
        });
        let frame_end = record.byte_offset.checked_add(record.frame_length);
        let prefix_end = record
            .prefix_offset
            .checked_add(u64_from_index(record.prefix_bytes.len()));
        let program_end = u64_from_index(record.program.len())
            .checked_mul(4)
            .and_then(|length| record.program_offset.checked_add(length));
        let mut decoded_references = design::decode::dimension_frames::decode_recipe_references(
            &record.prefix_bytes,
            record.prefix_offset,
        );
        for reference in &mut decoded_references {
            design::decode::dimension_frames::bind_recipe_reference_candidates(
                reference,
                &native.persistent_subentity_tags,
                Some(&record.id),
            );
        }
        let references_match = decoded_references == record.references;
        let edge_operands_match =
            design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
                record,
                &native.design_edge_operands,
            ) == record.matching_edge_operand_ids;
        let recipe_frame_matches = recipe.is_some_and(|recipe| {
            design_stream(&recipe.id) == native_stream
                && record
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
        });
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
            && dimension_recipe_ids.insert((native_stream, record.recipe_id.as_str()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension recipe has an invalid indexed-record owner",
                Some(record.id.clone()),
            );
        }
    }
    dimension_recipe_ids
}

/// Report dimension companions owning an unresolved construction recipe.
pub(super) fn validate_dimension_companion_recipes<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    dimension_recipe_ids: &HashSet<(&'a str, &'a str)>,
) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    for companion in &native.design_parameter_companions {
        let native_stream = design_stream(companion.id());
        let dimension_companion = owners_by_index
            .get(&(native_stream, companion.owner_record_index()))
            .and_then(|owner| {
                parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
            })
            .is_some_and(|parameter| {
                parameter.kind() == records::parameters::DesignParameterKind::Dimension
            });
        if dimension_companion
            && companion.payload().is_some_and(|payload| {
                payload.owned_recipe_ids().iter().any(|recipe_id| {
                    !dimension_recipe_ids.contains(&(native_stream, recipe_id.as_str()))
                })
            })
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension companion has an unowned construction recipe",
                Some(companion.id().to_owned()),
            );
        }
    }
}

/// Validate dimension locus pairs; returns their companion set.
pub(super) fn validate_dimension_locus_pairs<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut locus_pair_indices = HashSet::new();
    let mut locus_pair_companions = HashSet::new();
    for pair in &native.design_dimension_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let unique_index = locus_pair_indices.insert((native_stream, pair.record_index));
        let unique_companion =
            locus_pair_companions.insert((native_stream, pair.companion_record_index));
        let companion = companions_by_index.get(&(native_stream, pair.companion_record_index));
        let companion_contains_frame = companion.is_some_and(|companion| {
            companion
                .payload_byte_offset()
                .is_some_and(|expected_offset| pair.byte_offset() >= expected_offset)
                && !native.design_parameter_owners.iter().any(|owner| {
                    design_stream(owner.id()) == native_stream
                        && owner.byte_offset() > companion.byte_offset()
                        && owner.byte_offset() <= pair.byte_offset()
                })
        });
        let dimension_companion = companion.is_some_and(|companion| {
            owners_by_index
                .get(&(native_stream, companion.owner_record_index()))
                .and_then(|owner| {
                    parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                })
                .is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                })
        });
        let governs_following_dimension =
            design::decode::dimension_frames::following_dimension_companion_record_index(
                &pair.id,
                pair.paired_byte_offset(),
                &native.design_parameter_owners,
                &native.design_parameters,
            ) == Some(pair.governing_companion_record_index);
        let valid = companion_contains_frame
            && dimension_companion
            && governs_following_dimension
            && sketch_geometry_indices.contains(&(native_stream, pair.loci()[0].geometry_index()))
            && sketch_geometry_indices.contains(&(native_stream, pair.loci()[1].geometry_index()))
            && unique_index
            && unique_companion;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension locus pair has an invalid frame or geometry link",
                Some(pair.id.clone()),
            );
        }
    }
    locus_pair_companions
}

/// Validate dimension annotation frames and their operand runs.
pub(super) fn validate_dimension_annotation_frames(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut annotation_frame_indices = HashSet::new();
    for frame in &native.design_dimension_annotation_frames {
        let native_stream = design_stream(&frame.id);
        let unique_index = annotation_frame_indices.insert((native_stream, frame.record_index));
        let governing_owner = owners_by_index
            .get(&(native_stream, frame.governing_owner_record_index))
            .copied();
        let physical_interval_valid = match frame.companion_record_index {
            Some(record_index) => companions_by_index
                .get(&(native_stream, record_index))
                .is_some_and(|companion| {
                    companion
                        .payload_byte_offset()
                        .is_some_and(|expected_offset| frame.byte_offset() >= expected_offset)
                        && companion.payload().is_some_and(|payload| {
                            companion
                                .payload_byte_offset()
                                .and_then(|offset| offset.checked_add(payload.byte_length()))
                                .is_some_and(|expected_offset| {
                                    frame.paired_byte_offset() < expected_offset
                                })
                        })
                }),
            None => governing_owner.is_some_and(|owner| {
                scopes_by_index
                    .get(&(native_stream, owner.scope_record_index()))
                    .is_some_and(|scope| frame.byte_offset() >= scope.byte_offset())
                    && parameters_by_index
                        .get(&(native_stream, owner.parameter_record_index()))
                        .is_some_and(|parameter| {
                            frame.paired_byte_offset() < parameter.byte_offset()
                        })
            }),
        };
        let governing_link_valid = governing_owner.is_some_and(|owner| {
            owner.companion_record_index() == frame.governing_companion_record_index
                && parameters_by_index
                    .get(&(native_stream, owner.parameter_record_index()))
                    .is_some_and(|parameter| {
                        parameter.kind() == records::parameters::DesignParameterKind::Dimension
                    })
        });
        let operands_valid = frame.operands().iter().all(|operand| {
            operand
                .geometry_record_index
                .is_none_or(|index| sketch_geometry_indices.contains(&(native_stream, index.get())))
        });
        let owner_is_sketch = entities_by_suffix
            .get(&(native_stream, u64::from(frame.owner_reference)))
            .is_some_and(|entity| entity.in_sketch_module());
        let valid = unique_index
            && physical_interval_valid
            && governing_link_valid
            && operands_valid
            && owner_is_sketch;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension annotation frame has invalid links or offsets",
                Some(frame.id.clone()),
            );
        }
    }
}

/// Validate direct dimension presentation frames and their sketch-owner joins.
pub(super) fn validate_dimension_presentation_frames(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let sketch_scope_by_entity = native
        .design_sketch_placements
        .iter()
        .filter_map(|placement| {
            Some((
                (design_stream(&placement.id), placement.entity_id.suffix()),
                placement.scope_record_index?,
            ))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut presentation_frame_indices = HashSet::new();
    for frame in &native.design_dimension_presentation_frames {
        let native_stream = design_stream(&frame.id);
        let unique_index = presentation_frame_indices.insert((native_stream, frame.record_index));
        let owner = owners_by_index.get(&(native_stream, frame.governing_owner_record_index));
        let parameter =
            parameters_by_index.get(&(native_stream, frame.governing_parameter_record_index));
        let companion =
            companions_by_index.get(&(native_stream, frame.governing_companion_record_index));
        let owner_link_valid = owner.is_some_and(|owner| {
            owner.parameter_record_index() == frame.governing_parameter_record_index
                && owner.companion_record_index() == frame.governing_companion_record_index
                && parameter.is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                })
                && companion
                    .is_some_and(|companion| companion.owner_record_index() == owner.record_index())
        });
        let nearest_owner = native
            .design_parameter_owners
            .iter()
            .filter(|candidate| {
                design_stream(candidate.id()) == native_stream
                    && sketch_scope_by_entity
                        .get(&(native_stream, u64::from(frame.owner_reference)))
                        .is_some_and(|scope_record_index| {
                            candidate.scope_record_index() == *scope_record_index
                        })
                    && candidate.byte_offset() > frame.paired_byte_offset
                    && parameters_by_index
                        .get(&(native_stream, candidate.parameter_record_index()))
                        .is_some_and(|parameter| {
                            parameter.kind() == records::parameters::DesignParameterKind::Dimension
                        })
            })
            .min_by_key(|candidate| candidate.byte_offset());
        let governing_owner_is_nearest = nearest_owner.is_some_and(|candidate| {
            candidate.record_index() == frame.governing_owner_record_index
        });
        let operand_start = frame.byte_offset.checked_add(24);
        let operands_valid = !frame.operands.is_empty()
            && frame.operands.iter().enumerate().all(|(ordinal, operand)| {
                let Some(start) = u64_from_index(ordinal)
                    .checked_mul(15)
                    .and_then(|delta| operand_start.and_then(|offset| offset.checked_add(delta)))
                else {
                    return false;
                };
                start.checked_add(1).is_some_and(|expected_offset| {
                    operand.geometry_reference_offset == expected_offset
                }) && start
                    .checked_add(11)
                    .is_some_and(|expected_offset| operand.role_offset == expected_offset)
                    && sketch_geometry_indices
                        .contains(&(native_stream, operand.geometry_record_index.get()))
            });
        let owner_is_sketch = entities_by_suffix
            .get(&(native_stream, u64::from(frame.owner_reference)))
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
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension presentation frame has invalid links or offsets",
                Some(frame.id.clone()),
            );
        }
    }
}

/// Validate dimension locus groups; returns their companion set.
pub(super) fn validate_dimension_locus_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut locus_group_indices = HashSet::new();
    let mut locus_group_companions = HashSet::new();
    for group in &native.design_dimension_locus_groups {
        let native_stream = design_stream(&group.id);
        let unique_index = locus_group_indices.insert((native_stream, group.record_index));
        locus_group_companions.insert((native_stream, group.companion_record_index));
        let companion = companions_by_index.get(&(native_stream, group.companion_record_index));
        let companion_contains_frame = companion.is_some_and(|companion| {
            companion
                .payload_byte_offset()
                .is_some_and(|expected_offset| group.byte_offset >= expected_offset)
                && !native.design_parameter_owners.iter().any(|owner| {
                    design_stream(owner.id()) == native_stream
                        && owner.byte_offset() > companion.byte_offset()
                        && owner.byte_offset() <= group.byte_offset
                })
        });
        let dimension_companion = companion.is_some_and(|companion| {
            owners_by_index
                .get(&(native_stream, companion.owner_record_index()))
                .and_then(|owner| {
                    parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                })
                .is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                })
        });
        let count = group.loci.len();
        let loci_start = group.byte_offset.checked_add(24);
        let loci_offsets_valid = group.loci.iter().enumerate().all(|(ordinal, locus)| {
            let Some(start) = u64_from_index(ordinal)
                .checked_mul(15)
                .and_then(|delta| loci_start.and_then(|offset| offset.checked_add(delta)))
            else {
                return false;
            };
            start
                .checked_add(1)
                .is_some_and(|expected_offset| locus.geometry_reference_offset == expected_offset)
                && start
                    .checked_add(11)
                    .is_some_and(|expected_offset| locus.role_offset == expected_offset)
                && sketch_geometry_indices.contains(&(native_stream, locus.geometry_record_index))
        });
        let owner_start = u64_from_index(count)
            .checked_mul(15)
            .and_then(|delta| loci_start.and_then(|offset| offset.checked_add(delta)));
        let returns_start = owner_start.and_then(|offset| offset.checked_add(24));
        let returns_valid = group.loci.iter().enumerate().all(|(ordinal, locus)| {
            (u64_from_index(ordinal))
                .checked_mul(11)
                .and_then(|delta| returns_start.and_then(|offset| offset.checked_add(delta)))
                .and_then(|offset| offset.checked_add(1))
                .is_some_and(|expected_offset| locus.returned.offset == expected_offset)
                && sketch_geometry_indices.contains(&(native_stream, locus.returned.value))
        });
        let mut locus_members = group
            .loci
            .iter()
            .map(|locus| locus.geometry_record_index)
            .collect::<Vec<_>>();
        let mut return_members = group
            .loci
            .iter()
            .map(|locus| locus.returned.value)
            .collect::<Vec<_>>();
        locus_members.sort_unstable_by(Ord::cmp);
        return_members.sort_unstable_by(Ord::cmp);
        let owner_is_sketch = entities_by_suffix
            .get(&(native_stream, u64::from(group.owner_reference)))
            .is_some_and(|entity| entity.in_sketch_module());
        let frame_does_not_overlap = native.design_dimension_locus_groups.iter().all(|other| {
            design_stream(&other.id) != native_stream
                || other.companion_record_index != group.companion_record_index
                || other.record_index == group.record_index
                || group.next_byte_offset <= other.byte_offset
                || other.next_byte_offset <= group.byte_offset
        });
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
            && locus_members == return_members
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
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension locus group has an invalid counted frame or geometry link",
                Some(group.id.clone()),
            );
        }
    }
    locus_group_companions
}

/// Validate null-locus dimension pairs against typed companions.
pub(super) fn validate_dimension_null_locus_pairs<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    locus_pair_companions: &HashSet<(&'a str, u32)>,
    locus_group_companions: &HashSet<(&'a str, u32)>,
) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut null_locus_pair_indices = HashSet::new();
    let mut null_locus_pair_companions = HashSet::new();
    for pair in &native.design_dimension_null_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let unique_index = null_locus_pair_indices.insert((native_stream, pair.record_index));
        let unique_companion =
            null_locus_pair_companions.insert((native_stream, pair.companion_record_index));
        let companion = companions_by_index.get(&(native_stream, pair.companion_record_index));
        let companion_contains_frame = companion.is_some_and(|companion| {
            companion
                .payload_byte_offset()
                .is_some_and(|expected_offset| pair.byte_offset() >= expected_offset)
                && !native.design_parameter_owners.iter().any(|owner| {
                    design_stream(owner.id()) == native_stream
                        && owner.byte_offset() > companion.byte_offset()
                        && owner.byte_offset() <= pair.byte_offset()
                })
        });
        let dimension_companion = companion.is_some_and(|companion| {
            owners_by_index
                .get(&(native_stream, companion.owner_record_index()))
                .and_then(|owner| {
                    parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                })
                .is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                })
        });
        let governs_following_dimension =
            design::decode::dimension_frames::following_dimension_companion_record_index(
                &pair.id,
                pair.paired_byte_offset(),
                &native.design_parameter_owners,
                &native.design_parameters,
            ) == Some(pair.governing_companion_record_index);
        let companion_has_typed_frame = locus_pair_companions
            .contains(&(native_stream, pair.companion_record_index))
            || locus_group_companions.contains(&(native_stream, pair.companion_record_index));
        let valid = companion_contains_frame
            && dimension_companion
            && governs_following_dimension
            && !companion_has_typed_frame
            && sketch_geometry_indices.contains(&(native_stream, pair.loci()[1].geometry_index()))
            && unique_index
            && unique_companion;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design null-locus dimension pair has an invalid frame or geometry link",
                Some(pair.id.clone()),
            );
        }
    }
}
