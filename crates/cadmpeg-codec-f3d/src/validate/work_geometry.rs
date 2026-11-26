// SPDX-License-Identifier: Apache-2.0
//! Validation of work point, work plane, and vertex recipe constructions.

use super::{design_stream, Ctx};
use crate::{design, history, records};
use cadmpeg_core::CodecError;

pub(super) fn valid_work_point_construction(
    ctx: &Ctx,
    scope: &records::feature::scope::DesignParameterScope,
    native_stream: &str,
) -> Result<bool, CodecError> {
    let Some(construction) = scope.work_point_construction() else {
        return Ok(true);
    };
    let native = ctx.native;
    if construction.point_record_byte_offset >= construction.position_offset
        || construction.position_offset >= construction.reference_type_offset
        || !scope
            .reference_members()
            .values()
            .any(|value| value == &construction.point_record_index)
        || ctx
            .records_by_index
            .get(&(native_stream, construction.point_record_index))
            .is_none_or(|header| header.byte_offset != construction.point_record_byte_offset)
    {
        return Ok(false);
    }

    construction
        .rule
        .inputs()
        .iter()
        .try_fold(true, |valid, input| {
            if !valid {
                return Ok(false);
            }
            let header = ctx
                .records_by_index
                .get(&(native_stream, input.record_index()));
            Ok(scope
                .reference_members()
                .values()
                .any(|value| value == &input.record_index())
                && input.reference_offset > construction.reference_type_offset
                && header.is_some() && match input.carrier() {
                None => true,
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::EdgeRecipe {
                        operand_id,
                    },
                ) => native.design_edge_operands.iter().any(|operand| {
                    operand.id == *operand_id
                        && design_stream(&operand.id) == native_stream
                        && operand.scope_record_index == scope.record_index
                        && operand.record_index() == input.record_index()
                }),
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::VertexRecipe {
                        recipe: vertex,
                    },
                ) => valid_vertex_recipe(ctx, scope, native_stream, input.record_index(), vertex),
                Some(records::feature::work_geometry::DesignWorkPointInputCarrier::WorkPlane {
                    selection,
                }) => {
                    header.is_some_and(|header| {
                        header.class_tag == selection.class_tag
                            && selection.asset_id_offset() > header.byte_offset
                    }) && u32::try_from(selection.primary_identity)
                        .ok()
                        .and_then(|identity| identity.checked_add(1))
                        == Some(selection.work_plane_scope_record_index)
                        && native.design_parameter_scopes.iter().any(|plane| {
                            design_stream(&plane.id) == native_stream
                                && plane.kind()
                                    == crate::records::feature::scope::DesignFeatureKind::WorkPlane
                                && plane.record_index == selection.work_plane_scope_record_index
                        })
                }
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::SketchPoint {
                        selection,
                    },
                ) => {
                    header.is_some_and(|header| {
                        header.class_tag == selection.class_tag
                            && selection.asset_id_offset() > header.byte_offset
                    }) && u32::try_from(selection.point_persistent_id).is_ok()
                        && !selection.point_native_id.trim().is_empty()
                        && native.sketch_points.iter().any(|point| {
                            point.id == selection.point_native_id
                                && design_stream(&point.id) == native_stream
                                && point.owner_reference == Some(selection.sketch_record_index)
                                && point.persistent_id() == Some(selection.point_persistent_id)
                        })
                }
            })
        })
}

pub(super) fn valid_work_plane_construction(
    ctx: &Ctx,
    scope: &records::feature::scope::DesignParameterScope,
    native_stream: &str,
) -> bool {
    let Some(frame) = scope.work_plane_frame() else {
        return true;
    };
    let placements = &ctx.native.design_placement_constructions;
    let typed_operand =
        crate::design::decode::placement_construction::work_planes::has_operand(scope, placements);
    if typed_operand {
        return crate::design::decode::placement_construction::work_planes::solved_frame(
            scope, placements,
        )
        .as_ref()
            == Some(frame);
    }
    let Some(construction) = &frame.work_plane_construction else {
        return true;
    };
    let placement_record_index = &construction.placement_record_index;
    let inputs = construction.inputs();
    let Some([placement, first, second, third, extra_offset]) =
        scope.reference_members().values_array()
    else {
        return false;
    };
    let Some(placement_header) = ctx
        .records_by_index
        .get(&(native_stream, *placement_record_index))
    else {
        return false;
    };
    let transform_offset = frame.work_plane_transform_offset;
    let Some(owner) = ctx.native.design_parameter_owners.iter().find(|owner| {
        design_stream(owner.id()) == native_stream
            && owner.record_index() == *extra_offset
            && owner.scope_record_index() == scope.record_index
            && owner.evaluated_value().get() == 0.0
    }) else {
        return false;
    };

    placement == placement_record_index
        && [
            inputs[0].record_index(),
            inputs[1].record_index(),
            inputs[2].record_index(),
        ] == [*first, *second, *third]
        && scope.work_plane_reference() == Some(*extra_offset)
        && scope
            .work_plane_frame()
            .and_then(|frame| frame.reference.as_ref())
            .is_some()
        && transform_offset > placement_header.byte_offset
        && inputs.iter().all(|input| {
            valid_vertex_recipe(ctx, scope, native_stream, input.record_index(), input)
        })
        && ctx.native.design_parameters.iter().any(|parameter| {
            design_stream(&parameter.id) == native_stream
                && parameter.record_index == owner.parameter_record_index()
                && parameter.owner_record_index() == Some(owner.record_index())
                && parameter.source_kind() == "ExtraOffset"
                && parameter.evaluated_value().get() == 0.0
        })
}

pub(super) fn valid_vertex_recipe(
    ctx: &Ctx,
    scope: &records::feature::scope::DesignParameterScope,
    native_stream: &str,
    record_index: u32,
    vertex: &records::feature::work_geometry::DesignVertexRecipe,
) -> bool {
    let native = ctx.native;
    let header = ctx.records_by_index.get(&(native_stream, record_index));
    let recipe = ctx.recipes_by_id.get(vertex.recipe_id.as_str());
    let mut expected_references = design::decode::dimension_frames::decode_recipe_references(
        &vertex.recipe_prefix_bytes,
        vertex.recipe_prefix_offset(),
    );
    for reference in &mut expected_references {
        design::decode::dimension_frames::bind_recipe_reference_candidates(
            reference,
            &native.persistent_subentity_tags,
            Some(&scope.id),
        );
    }
    let prefix_length = u64::try_from(vertex.recipe_prefix_bytes.len()).ok();
    let family_name_length = u64::try_from(design::construction_recipe_family_name_len(
        records::recipes::ConstructionRecipeKind::Vertex,
    ))
    .ok();
    let program_byte_length = u64::try_from(vertex.recipe_program.len())
        .ok()
        .and_then(|length| length.checked_mul(4));
    let resolution_is_valid = match vertex.resolution {
        None => true,
        Some(resolution) => {
            let state_id = resolution.state_id;
            let vertex_slot = resolution.vertex_slot();
            let mut states = native
                .asm_histories
                .iter()
                .flat_map(|history| history.states().iter().map(move |state| (history, state)))
                .filter(|(_, state)| state.state_id == state_id);
            states.next().is_some_and(|(history, state)| {
                states.next().is_none()
                    && history.topology(state).map_or_else(
                        || history::projection_was_finalized(&native.asm_histories),
                        |topology| topology.vertices.contains(&vertex_slot),
                    )
            })
        }
    };
    vertex.record_index() == record_index
        && header.is_some_and(|header| {
            header.byte_offset == vertex.byte_offset() && header.class_tag == vertex.class_tag
        })
        && prefix_length.is_some_and(|prefix_length| {
            vertex
                .recipe_prefix_offset()
                .checked_add(prefix_length)
                .zip(recipe.and_then(|recipe| recipe.byte_offset.checked_sub(4)))
                .is_some_and(|(prefix_end, recipe_prefix_end)| prefix_end == recipe_prefix_end)
        })
        && vertex.recipe_references == expected_references
        && resolution_is_valid
        && recipe.is_some_and(|recipe| {
            design_stream(&recipe.id) == native_stream
                && recipe.kind == records::recipes::ConstructionRecipeKind::Vertex
                && recipe.byte_offset > vertex.recipe_record_byte_offset()
                && recipe.byte_offset < vertex.next_byte_offset()
                && family_name_length.is_some_and(|family_name_length| {
                    recipe
                        .byte_offset
                        .checked_add(family_name_length)
                        .is_some_and(|expected_offset| {
                            vertex.recipe_program_offset == expected_offset
                        })
                })
        })
        && program_byte_length.is_some_and(|program_byte_length| {
            program_byte_length != 0
                && vertex
                    .recipe_program_offset
                    .checked_add(program_byte_length)
                    .is_some_and(|expected_offset| expected_offset == vertex.next_byte_offset())
        })
}
