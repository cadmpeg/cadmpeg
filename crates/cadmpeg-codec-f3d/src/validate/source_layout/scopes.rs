// SPDX-License-Identifier: Apache-2.0
//! Source layout of the parameter scopes: each scope frame and the payload
//! offsets that its capture states.
//!
//! A scope's frame length is the extent of its capture and its offsets are
//! the pointers of that capture. A scope is checked with those offsets
//! restored; a scope with no capture, or an offset with no pointer, fails.

use std::collections::HashMap;

use cadmpeg_ir::report::check::{Check, Finding};

use super::super::{
    component_insert_frame_length, component_insert_matrix_delta, design_stream,
    joint_origin_frame_length, push_constant_finding, Ctx,
};
use super::Layouts;
use crate::records;
use crate::records::feature::scope::{DesignParameterScope, LocatedScopeFrame};

/// The scopes of a namespace with the source offsets of their captures.
pub(super) struct LocatedScopes<'a> {
    scopes: Vec<Option<DesignParameterScope>>,
    by_index: HashMap<(&'a str, u32), Vec<usize>>,
}

impl<'a> LocatedScopes<'a> {
    /// Restore every scope of `native` from its capture in `layouts`.
    pub(super) fn new(native: &'a crate::native::F3dNative, layouts: &Layouts<'_>) -> Self {
        let mut by_index: HashMap<(&'a str, u32), Vec<usize>> = HashMap::new();
        let scopes = native
            .design_parameter_scopes
            .iter()
            .enumerate()
            .map(|(position, scope)| {
                by_index
                    .entry((design_stream(&scope.id), scope.record_index))
                    .or_default()
                    .push(position);
                located_scope(scope, layouts)
            })
            .collect();
        Self { scopes, by_index }
    }

    /// The located scope at `position` in the native arena.
    pub(super) fn get(&self, position: usize) -> Option<&DesignParameterScope> {
        self.scopes.get(position)?.as_ref()
    }

    /// The located scopes with `record_index` in the stream of `scope`.
    fn indexed(
        &self,
        scope: &DesignParameterScope,
        record_index: u32,
    ) -> Vec<&DesignParameterScope> {
        let stream = design_stream(&scope.id);
        self.by_index
            .iter()
            .filter(|((candidate, index), _)| *candidate == stream && *index == record_index)
            .flat_map(|(_, positions)| positions.iter())
            .filter_map(|position| self.get(*position))
            .collect()
    }

    /// The located `JointOrigin` scopes with `record_index` in the stream of
    /// `scope`.
    pub(super) fn joint_origins(
        &self,
        scope: &DesignParameterScope,
        record_index: u32,
    ) -> impl Iterator<Item = &DesignParameterScope> {
        self.indexed(scope, record_index)
            .into_iter()
            .filter(|target| {
                target.kind() == crate::records::feature::scope::DesignFeatureKind::JointOrigin
            })
    }

    /// The located scopes in the stream of `scope`.
    fn in_stream<'s>(
        &'s self,
        scope: &'s DesignParameterScope,
    ) -> impl Iterator<Item = &'s DesignParameterScope> {
        let stream = design_stream(&scope.id);
        self.scopes
            .iter()
            .flatten()
            .filter(move |candidate| design_stream(&candidate.id) == stream)
    }
}

/// A scope with the source offsets of its capture: its pointers, and its
/// extent as its frame length. A capture that lacks a pointer of the
/// located scope, or names a pointer the scope does not hold, locates no
/// scope.
pub(super) fn located_scope(
    scope: &DesignParameterScope,
    layouts: &Layouts<'_>,
) -> Option<DesignParameterScope> {
    let layout = layouts.layout(&scope.id)?;
    let mut located = located_record(scope, layout.pointers())?;
    located.restore_frame_length(layout.byte_len());
    Some(located)
}

/// `record` with the offsets that `pointers` state, when `pointers` holds
/// exactly the pointers of the located record.
pub(super) fn located_record<T>(
    record: &T,
    pointers: &std::collections::BTreeMap<String, u64>,
) -> Option<T>
where
    T: Clone + crate::records::source_offsets::SourceOffsets,
{
    let mut located = record.clone();
    let missing = crate::records::source_offsets::restore(&mut located, pointers);
    let held = crate::records::source_offsets::collect(&mut located.clone());
    (missing.is_empty() && held.keys().eq(pointers.keys())).then_some(located)
}

/// Validate the frame and the payload offsets of every scope.
pub(super) fn validate_scope_layouts(
    ctx: &Ctx<'_, '_>,
    scopes: &LocatedScopes<'_>,
    findings: &mut Vec<Finding>,
) {
    if !ctx.holds_design_intent() {
        return;
    }
    for (position, scope) in ctx.native.design_parameter_scopes.iter().enumerate() {
        let valid = scopes
            .get(position)
            .is_some_and(|located| scope_layout_holds(ctx, scopes, located));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter scope has an invalid paired frame",
                Some(scope.id.clone()),
            );
        }
    }
}

/// The frame arithmetic and every payload offset of one located scope.
fn scope_layout_holds(
    ctx: &Ctx<'_, '_>,
    scopes: &LocatedScopes<'_>,
    scope: &DesignParameterScope,
) -> bool {
    let Some(frame) = scope.located_frame() else {
        return false;
    };
    let frame_valid = scope
        .located_draft()
        .is_some_and(|draft| draft.check_frame_layout().is_ok())
        && scope.located_generation() == Some(scope.generation())
        && scope.located_history_state_offset_holds();
    let inside = |offset: Option<u64>| {
        offset.is_some_and(|offset| {
            offset > frame.byte_offset() && offset < frame.paired_byte_offset()
        })
    };
    let entity = scope
        .sketch_entity()
        .is_none_or(|binding| inside(binding.entity_reference_offset));
    let base_flange = scope.base_flange_operation().is_none_or(|operation| {
        operation.thickness_offset == frame.byte_offset().checked_add(123)
            && operation
                .thickness_offset
                .is_some_and(|offset| offset < frame.paired_byte_offset())
    });
    let edge_flange = scope
        .edge_flange_operation()
        .is_none_or(|operation| inside(operation.bend_radius_offset));
    let hem = scope
        .hem_operation()
        .is_none_or(|operation| inside(operation.bend_radius_offset));
    let alignment = scope.assembly_alignment().is_none_or(|alignment| {
        super::assembly::alignment_layout_holds(scopes, scope, frame, alignment)
    });
    let component_insert = scope
        .component_insert_construction()
        .is_none_or(|construction| component_insert_layout_holds(scope, frame, construction));
    let copy_paste_component = scope
        .copy_paste_component_operation()
        .is_none_or(|operation| {
            let source_at = match scope.generation() {
                records::feature::scope::generation::DesignScopeGeneration::CopyPaste(
                    copy_paste,
                ) => match copy_paste.frame_length() {
                    529 => 38,
                    525 => 34,
                    _ => return false,
                },
                _ => return false,
            };
            operation.source_transform_offset == frame.byte_offset().checked_add(source_at)
                && operation.copied_transform_offset
                    == frame.byte_offset().checked_add(source_at + 156)
        });
    let draft = scope.draft_operation().is_none_or(|operation| {
        match (operation.angle_offset, operation.opposite_angle_offset) {
            (Some(angle), Some(opposite)) => angle > frame.paired_byte_offset() && opposite > angle,
            _ => false,
        }
    });
    let combine = scope
        .combine_operation()
        .is_none_or(|operation| combine_layout_holds(ctx, scope, frame, operation));
    let thread = scope.thread_construction().is_none_or(|construction| {
        let Some(designation_offset) = construction.designation_offset else {
            return false;
        };
        matches!(
            designation_offset.checked_sub(frame.byte_offset()),
            Some(38 | 42)
        ) && match construction.form {
            records::feature::thread::DesignThreadForm::Compact(Some(reference)) => {
                reference.offset.is_some_and(|offset| {
                    offset > designation_offset && offset < frame.paired_byte_offset()
                })
            }
            records::feature::thread::DesignThreadForm::Compact(None)
            | records::feature::thread::DesignThreadForm::Standard
            | records::feature::thread::DesignThreadForm::StandardLegacy
            | records::feature::thread::DesignThreadForm::CompactLegacy => true,
        }
    });
    let joint_origin = scope
        .joint_origin_frame()
        .is_none_or(|origin| joint_origin_layout_holds(scopes, scope, frame, origin));
    let work_point = scope
        .work_point_construction()
        .is_none_or(|construction| work_point_layout_holds(ctx, construction));
    let work_plane = scope
        .work_plane_frame()
        .and_then(|frame| frame.work_plane_construction.as_ref())
        .is_none_or(|construction| {
            construction
                .inputs()
                .iter()
                .all(|input| vertex_recipe_layout_holds(ctx, input))
        });
    let extrude = scope.extrude_prologue().is_none_or(|prologue| {
        super::extrude::extrude_prologue_layout_holds(ctx.native, scope, frame, prologue)
    });
    let surfaces = match scope.payload() {
        records::feature::scope::DesignScopePayload::SurfaceStitch(operation) => operation
            .gap_tolerance_offset
            .is_some_and(|offset| offset > frame.paired_byte_offset()),
        records::feature::scope::DesignScopePayload::SurfaceRuled(operation) => {
            operation.method_offset == frame.byte_offset().checked_add(20)
                && operation.alternate_face_offset == frame.byte_offset().checked_add(27)
                && operation.corner_offset == frame.byte_offset().checked_add(50)
        }
        _ => true,
    };
    let copy_paste_bodies = scope.copy_paste_bodies_operation().is_none_or(
        records::feature::body_ops::DesignCopyPasteBodiesOperation::source_layout_valid,
    );
    let copy_paste_headers = scope
        .copy_paste_bodies_operation()
        .is_none_or(|operation| copy_paste_headers_hold(ctx, scope, operation));
    let pattern = scope
        .rectangular_pattern_construction()
        .and_then(|construction| construction.instances())
        .is_none_or(|instances| pattern_instances_layout_holds(ctx, scope, instances));
    let coil = coil_operation_layout_holds(scope, frame);
    frame_valid
        && entity
        && base_flange
        && edge_flange
        && hem
        && alignment
        && component_insert
        && copy_paste_component
        && draft
        && combine
        && thread
        && joint_origin
        && work_point
        && work_plane
        && extrude
        && surfaces
        && copy_paste_bodies
        && copy_paste_headers
        && pattern
        && coil
}

/// The source offset of the record header with `record_index` in the stream
/// of `scope`: the offset that the header identifier names.
fn record_header_offset(
    ctx: &Ctx<'_, '_>,
    scope: &DesignParameterScope,
    record_index: u32,
) -> Option<u64> {
    let header = ctx
        .records_by_index
        .get(&(design_stream(&scope.id), record_index))?;
    header
        .id
        .rsplit_once(":design-record-header#")?
        .1
        .parse()
        .ok()
}

/// The `CopyPasteBodies` body group and relation offsets are the offsets of
/// the record headers they name.
fn copy_paste_headers_hold(
    ctx: &Ctx<'_, '_>,
    scope: &DesignParameterScope,
    operation: &records::feature::body_ops::DesignCopyPasteBodiesOperation,
) -> bool {
    operation.body_group_byte_offset().is_some_and(|offset| {
        record_header_offset(ctx, scope, operation.body_group_record_index) == Some(offset)
    }) && operation.relation_byte_offset().is_some_and(|offset| {
        record_header_offset(ctx, scope, operation.relation_record_index) == Some(offset)
    })
}

/// The ten-reference `CoilPrimitive` form is the long form exactly when its
/// operation is at the long-form prologue offset.
fn coil_operation_layout_holds(scope: &DesignParameterScope, frame: LocatedScopeFrame) -> bool {
    if scope.kind() != records::feature::scope::DesignFeatureKind::CoilPrimitive
        || scope.reference_members().len() != 10
    {
        return true;
    }
    let long_form_offset = scope.coil_operation_offset().is_some()
        && scope.coil_operation_offset()
            == frame
                .byte_offset()
                .checked_add(cadmpeg_core::decode::u64_from_index(
                    crate::layout::coil_long_scope_fixed_prologue::OPERATION,
                ));
    long_form_offset == scope.coil_long_form()
}

/// A `Component Insert` matrix sits at the delta its frame names, and a
/// carrier matrix sits on the side of the occurrence role its frame names.
fn component_insert_layout_holds(
    scope: &DesignParameterScope,
    frame: LocatedScopeFrame,
    construction: &records::feature::assembly_features::DesignComponentInsertConstruction,
) -> bool {
    let Some(matrix) = construction.placement.as_ref() else {
        return true;
    };
    let Some(frame_length) = component_insert_frame_length(scope) else {
        return false;
    };
    let Some(delta) = component_insert_matrix_delta(
        frame_length,
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
    ) else {
        return false;
    };
    let carrier = match (frame_length, &matrix.carrier) {
        (385, None) => true,
        (404, Some(carrier)) => carrier
            .offset
            .zip(construction.neutron_role_offset)
            .is_some_and(|(carrier, role)| carrier < role),
        (_, Some(carrier)) => carrier
            .offset
            .zip(construction.neutron_role_offset)
            .is_some_and(|(carrier, role)| role < carrier),
        (_, None) => false,
    };
    matrix.scope.offset == frame.byte_offset().checked_add(delta) && carrier
}

/// A `Combine` form has the frame its prologue form names and the operation
/// and keep-tools fields at that form's offsets.
fn combine_layout_holds(
    ctx: &Ctx<'_, '_>,
    scope: &DesignParameterScope,
    frame: LocatedScopeFrame,
    operation: &records::feature::combine::DesignCombineOperation,
) -> bool {
    let kind_bytes = u64::try_from(scope.kind_name().encode_utf16().count())
        .ok()
        .and_then(|count| count.checked_mul(2));
    let compact_scope = scope.class_tag.as_str() == "387"
        && scope.paired_class_tag.as_str() == "258"
        && kind_bytes.and_then(|bytes| frame.frame_length().checked_sub(bytes)) == Some(314);
    let extended_reference_scope = scope.class_tag.as_str() == "329"
        && scope.paired_class_tag.as_str() == "261"
        && frame.frame_length() == 363;
    let native_stream = design_stream(&scope.id);
    let external = operation.tools.iter().all(|selection| {
        selection.external_identity.as_ref().is_none_or(|identity| {
            ctx.records_by_index
                .get(&(native_stream, selection.record_index))
                .is_some_and(|header| {
                    header.byte_offset.is_none_or(|offset| {
                        offset.checked_add(44) == identity.selector_asset_id_offset()
                    })
                })
        })
    });
    let (form_valid, operation_at, keep_tools_at) = match operation.form {
        records::feature::combine::DesignCombineForm::Standard => {
            (!compact_scope && !extended_reference_scope, 20, 25)
        }
        records::feature::combine::DesignCombineForm::Compact => (compact_scope, 21, 25),
        records::feature::combine::DesignCombineForm::ExtendedReference => {
            (extended_reference_scope, 31, 30)
        }
    };
    external
        && form_valid
        && operation.operation_offset == frame.byte_offset().checked_add(operation_at)
        && operation.keep_tools_offset == frame.byte_offset().checked_add(keep_tools_at)
}

/// A `JointOrigin` frame is inline in its own scope, or is an operand frame
/// of an `Assemble` scope, at the offsets each form places.
fn joint_origin_layout_holds(
    scopes: &LocatedScopes<'_>,
    scope: &DesignParameterScope,
    frame: LocatedScopeFrame,
    origin: &records::feature::scope::DesignJointOriginTransform,
) -> bool {
    let transform = origin.joint_origin_transform;
    let Some(transform_offset) = origin.joint_origin_transform_offset else {
        return false;
    };
    let inline = match (joint_origin_frame_length(scope), &origin.reference) {
        (Some(385), None) => frame.byte_offset().checked_add(49) == Some(transform_offset),
        (Some(336 | 347), Some(reference)) => {
            frame.byte_offset().checked_add(60) == Some(transform_offset)
                && reference.joint_origin_reference_offset == frame.byte_offset().checked_add(46)
                && scope
                    .reference_members()
                    .values()
                    .any(|value| value == &reference.joint_origin_reference)
        }
        _ => false,
    };
    let assembly_operand = origin.reference.is_none()
        && scopes.in_stream(scope).any(|assembly| {
            assembly.kind() == crate::records::feature::scope::DesignFeatureKind::Assemble
                && assembly.assembly_alignment().is_some_and(|alignment| {
                    alignment.operand_frames().is_some_and(|frames| {
                        frames.iter().any(|operand| {
                            operand.reference_record_index == scope.record_index
                                && operand.transform == transform
                                && operand.transform_offset == Some(transform_offset)
                        })
                    })
                })
        });
    let single_operand_assembly = origin.reference.as_ref().is_some_and(|reference| {
        scopes.in_stream(scope).any(|assembly| {
            assembly.kind() == crate::records::feature::scope::DesignFeatureKind::Assemble
                && assembly.class_tag.as_str() == "276"
                && assembly.paired_class_tag.as_str() == "258"
                && assembly.assembly_frame()
                    == Some(records::feature::scope::generation::DesignAssemblyFrame::Frame604)
                && assembly
                    .byte_offset()
                    .and_then(|offset| offset.checked_add(36))
                    == Some(transform_offset)
                && assembly
                    .reference_members()
                    .values()
                    .any(|value| value == &reference.joint_origin_reference)
                && assembly
                    .byte_offset()
                    .and_then(|offset| offset.checked_add(25))
                    == reference.joint_origin_reference_offset
        })
    });
    inline || assembly_operand || single_operand_assembly
}

/// A `WorkPoint` construction keeps its point record, position and reference
/// type in order, with each input reference after the reference type.
fn work_point_layout_holds(
    ctx: &Ctx<'_, '_>,
    construction: &records::feature::work_geometry::DesignWorkPointConstruction,
) -> bool {
    let (Some(point), Some(position), Some(reference_type)) = (
        construction.point_record_byte_offset,
        construction.position_offset,
        construction.reference_type_offset,
    ) else {
        return false;
    };
    point < position
        && position < reference_type
        && construction.rule.inputs().iter().all(|input| {
            input
                .reference_offset
                .is_some_and(|offset| offset > reference_type)
                && match input.carrier() {
                    None
                    | Some(
                        records::feature::work_geometry::DesignWorkPointInputCarrier::EdgeRecipe {
                            ..
                        },
                    ) => true,
                    Some(
                        records::feature::work_geometry::DesignWorkPointInputCarrier::VertexRecipe {
                            recipe,
                        },
                    ) => vertex_recipe_layout_holds(ctx, recipe),
                    Some(
                        records::feature::work_geometry::DesignWorkPointInputCarrier::WorkPlane {
                            selection,
                        },
                    ) => selection.source_layout_valid(),
                    Some(
                        records::feature::work_geometry::DesignWorkPointInputCarrier::SketchPoint {
                            selection,
                        },
                    ) => selection.source_layout_valid(),
                }
        })
}

/// A vertex recipe's prefix, recipe record, program and closing record are
/// in order, and its references are the ones its prefix bytes encode at
/// their source offsets.
pub(in crate::validate) fn vertex_recipe_layout_holds(
    ctx: &Ctx<'_, '_>,
    vertex: &records::feature::work_geometry::DesignVertexRecipe,
) -> bool {
    let (Some(prefix_offset), Some(recipe_record), Some(program), Some(next)) = (
        vertex.recipe_prefix_offset(),
        vertex.recipe_record_byte_offset(),
        vertex.recipe_program_offset,
        vertex.next_byte_offset(),
    ) else {
        return false;
    };
    let recipe = ctx.recipes_by_id.get(vertex.recipe_id.as_str());
    let expected_references = crate::design::decode::dimension_frames::decode_recipe_references(
        &vertex.recipe_prefix_bytes,
        prefix_offset,
    );
    let reference_offsets = |reference: &records::dimensions::DesignRecipeReference| {
        (
            reference.clause_index,
            reference.selector_offset,
            reference.token_offset,
            reference.design_reference_offset,
        )
    };
    let prefix_length = u64::try_from(vertex.recipe_prefix_bytes.len()).ok();
    let family_name_length = u64::try_from(crate::design::construction_recipe_family_name_len(
        records::recipes::ConstructionRecipeKind::Vertex,
    ))
    .ok();
    let program_byte_length = u64::try_from(vertex.recipe_program.len())
        .ok()
        .and_then(|length| length.checked_mul(4));
    let recipe_offset = recipe.and_then(|recipe| recipe.byte_offset);
    vertex.source_layout_valid()
        && (recipe_offset.is_none()
            || prefix_length
                .and_then(|length| prefix_offset.checked_add(length))
                .zip(recipe_offset.and_then(|offset| offset.checked_sub(4)))
                .is_some_and(|(prefix_end, recipe_prefix_end)| prefix_end == recipe_prefix_end))
        && vertex
            .recipe_references
            .iter()
            .map(reference_offsets)
            .eq(expected_references.iter().map(reference_offsets))
        && recipe_offset.is_none_or(|offset| offset > recipe_record && offset < next)
        && family_name_length.is_some_and(|family_name_length| {
            recipe_offset
                .is_none_or(|offset| offset.checked_add(family_name_length) == Some(program))
        })
        && program_byte_length
            .and_then(|length| program.checked_add(length))
            .is_some_and(|end| end == next)
}

/// The generated occurrences of a component pattern are the placed
/// occurrences at the source positions of its instance transforms.
fn pattern_instances_layout_holds(
    ctx: &Ctx<'_, '_>,
    scope: &DesignParameterScope,
    instances: &records::feature::patterns::DesignRectangularPatternInstances,
) -> bool {
    let frames_follow_headers = instances.frames().all(|frame| {
        frame.transform.offset.is_some_and(|offset| {
            record_header_offset(ctx, scope, frame.record_index)
                .is_some_and(|header| header < offset)
        })
    });
    let records::feature::patterns::DesignRectangularPatternInstances::Components {
        generated, ..
    } = instances
    else {
        return frames_follow_headers;
    };
    let stream = design_stream(&scope.id);
    frames_follow_headers
        && generated.iter().all(|row| {
            row.instance.transform.offset.is_some_and(|offset| {
                ctx.native
                    .design_component_occurrences
                    .iter()
                    .any(|occurrence| {
                        design_stream(&occurrence.id) == stream
                            && occurrence
                                .occurrence_guid
                                .as_str()
                                .eq_ignore_ascii_case(row.occurrence_guid.as_str())
                            && occurrence.transform().map(|transform| transform.offset)
                                == Some(offset)
                    })
            })
        })
}

/// An edge-treatment vertex operand has the identity of its envelope offset,
/// and its recipe has the layout of a vertex recipe in its scope.
pub(super) fn validate_edge_treatment_vertex_layouts(
    ctx: &Ctx<'_, '_>,
    layouts: &Layouts<'_>,
    findings: &mut Vec<Finding>,
) {
    for operand in &ctx.native.design_edge_treatment_vertex_operands {
        let stream = design_stream(&operand.id);
        let valid = layouts.layout(&operand.id).is_some_and(|layout| {
            let Some(located) = located_record(operand, layout.pointers()) else {
                return false;
            };
            let scope = ctx
                .scopes_by_index
                .get(&(stream, operand.scope_record_index));
            located.recipe.byte_offset().is_some_and(|offset| {
                operand.id
                    == crate::ids::native_scoped_id(stream, "edge-treatment-vertex-operand", offset)
            }) && scope.is_some()
                && vertex_recipe_layout_holds(ctx, &located.recipe)
        });
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion edge-treatment vertex operand has an invalid group or recipe frame",
                Some(operand.id.clone()),
            );
        }
    }
}

/// A Base Mesh Feature's scope record is the scope frame it names.
pub(super) fn validate_mesh_feature_scope_layouts(
    ctx: &Ctx<'_, '_>,
    scopes: &LocatedScopes<'_>,
    findings: &mut Vec<Finding>,
) {
    for feature in &ctx.native.design_mesh_features {
        let stream = design_stream(&feature.id);
        let record = feature.scope().record();
        let base = feature.scope().base_record();
        let valid = scopes
            .by_index
            .get(&(stream, record.record_index()))
            .into_iter()
            .flatten()
            .filter_map(|position| scopes.get(*position))
            .any(|scope| {
                scope.located_frame().is_some_and(|frame| {
                    frame.byte_offset() == record.byte_offset()
                        && frame.paired_byte_offset() == base.byte_offset()
                        && record.byte_offset().checked_add(frame.frame_length())
                            == Some(base.byte_offset())
                })
            });
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design mesh feature has an invalid scope frame",
                Some(feature.id.clone()),
            );
        }
    }
}

/// A canvas or decal image locates the scope frame it names.
pub(super) fn validate_image_scope_layouts(
    ctx: &Ctx<'_, '_>,
    scopes: &LocatedScopes<'_>,
    layouts: &Layouts<'_>,
    findings: &mut Vec<Finding>,
) {
    let scope_offset = |stream: &str, record_index: u32| {
        scopes
            .by_index
            .get(&(stream, record_index))
            .into_iter()
            .flatten()
            .filter_map(|position| scopes.get(*position))
            .find_map(DesignParameterScope::byte_offset)
    };
    for image in &ctx.native.design_canvas_images {
        let stream = design_stream(&image.id);
        let image_scope_offset = layouts
            .pointer(&image.id, "asset_byte_offset")
            .and_then(|offset| image.scope_byte_offset_after_asset(offset));
        let valid = image_scope_offset
            .is_none_or(|offset| scope_offset(stream, image.scope_record_index) == Some(offset));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design canvas image has an invalid scope frame",
                Some(image.id.clone()),
            );
        }
    }
    for image in &ctx.native.design_decal_images {
        let stream = design_stream(&image.id);
        let valid = layouts
            .pointer(&image.id, "scope_byte_offset")
            .is_none_or(|offset| scope_offset(stream, image.scope_record_index()) == Some(offset));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design decal image has an invalid scope frame",
                Some(image.id.clone()),
            );
        }
    }
}

/// The scope lanes that name a parameter owner and the source position of
/// the owner's evaluated value: `(owner record index, value offset)`. A lane
/// without its offset is `None`.
pub(super) fn scope_owner_lanes(scope: &DesignParameterScope) -> Vec<(u32, Option<u64>)> {
    let mut lanes = Vec::new();
    if let Some(construction) = scope.rectangular_pattern_construction() {
        let offsets = construction
            .value_offsets
            .map_or([None; 4], |offsets| offsets.map(Some));
        lanes.extend(construction.owner_record_indices.into_iter().zip(offsets));
    }
    if let Some(alignment) = scope.assembly_alignment() {
        lanes.extend(
            alignment
                .owners
                .iter()
                .map(|lane| (lane.value, lane.offset)),
        );
        let as_built_421 =
            crate::design::assembly::scope_legacy_as_built_421_generation(scope).is_some();
        if let Some(limits) = alignment.limits().filter(|_| as_built_421) {
            let offsets = limits
                .value_offsets
                .map_or([None; 2], |offsets| offsets.map(Some));
            lanes.extend(limits.owner_record_indices.into_iter().zip(offsets));
        }
    }
    lanes
}
