// SPDX-License-Identifier: Apache-2.0
//! Section constraint reconciliation, incidence, and dimension emission.

use crate::decode::sketch::axis::SectionAxis;
use crate::feature::segment_rows::SegmentRow;

use crate::decode::sketch::equations_scalar::SectionScalarVariable;

use super::super::feature_history::dimensions::{
    resolved_feature_dimension_parameter_admitted,
};
use super::super::sketch::coordinates::{
    resolved_section_coordinates, saved_section_coordinate_witnesses,
    section_linear_distance_coordinate,
};
use super::super::sketch::equations_coordinate::{
    approximately_equal, section_equation_function_six_distance_rows,
    section_equation_point_on_line_constraint_rows, section_equation_radius_dimensions,
    section_equation_unsigned_coordinate_distance_rows,
};
use super::super::sketch::equations_scalar::{
    section_equation_function_five_scalar_equality_rows,
    section_equation_function_forty_three_axis_distance_rows,
    section_equation_function_forty_two_midpoint_coordinate_rows,
    section_equation_function_sixteen_angle_difference_rows,
    section_equation_function_thirty_one_point_coordinate_rows,
    section_equation_radial_constraint_rows,
};
use super::super::sketch::radii::section_radius_relation_arc;
use super::super::sketch::skamp::{section_segment_rows, unique_decoded_section_segment};
use super::super::sketch_ids::{
    sketch_constraint_id_admitted, sketch_entity_id_admitted, sketch_native_ref_admitted,
};
use crate::decode::sketch_transfer::identity::{
    opaque_section_segment_identity_suffix_admitted, section_entity_external_ids,
    section_segment_identity_suffix_admitted, unique_section_segment_external_ids,
};
use crate::decode::sketch_transfer::loci::{
    section_point_locus, section_skamp_active, section_skamp_locus,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::sketches::{
    NativeOperandField, SketchConstraint, SketchConstraintDefinitionInput, SketchCoordinateAxis,
    SketchDistancePair, SketchEntityId, SketchId, SketchLocus, SketchNativeOperand,
};
use cadmpeg_ir::{
    features::ParameterId,
    scalar::{Angle, Length},
};
use super::solver_links::{EquationIncidences, RelationIncidences};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const EPS_POLAR_ZERO: f64 = 1.0e-12;

fn point_pair([first, second]: [u32; 2]) -> [u32; 2] {
    if first <= second { [first, second] } else { [second, first] }
}

fn collect_constraint_candidates<S, T>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[S],
    operation: &'static str,
    mut convert: impl FnMut(&S) -> Result<Option<T>, cadmpeg_core::CodecError>,
) -> Result<Vec<T>, cadmpeg_core::CodecError> {
    let mut collected = Vec::new();
    for row in ctx.admit_iter(candidates, operation)? {
        if let Some(candidate) = convert(row)? {
            ctx.reserve_vec(&mut collected, 1, operation)?;
            collected.push(candidate);
        }
    }
    Ok(collected)
}

fn equation_constraint(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &SketchId,
    equation_id: u32,
    definition: SketchConstraintDefinitionInput,
    active: bool,
    offset: usize,
) -> Result<Option<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let Some(id) =
        sketch_constraint_id_admitted(ctx, sketch, format_args!("equation:{equation_id}"))?
    else {
        return Ok(None);
    };
    let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
    else {
        return Ok(None);
    };
    Ok(Some((
        SketchConstraint {
            id,
            sketch: sketch.try_clone_for_decode(ctx, "creo equation sketch identity")?,
            definition,
            name: None,
            driving: None,
            active: Some(active),
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: Some(sketch_native_ref_admitted(ctx, sketch)?),
        },
        offset,
    )))
}

pub(in super::super) fn section_segment_verhor_definition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    segment: &crate::feature::definitions::FeatureSegment,
    sketch: &SketchId,
    entity: SketchEntityId,
) -> Result<Option<SketchConstraintDefinitionInput>, cadmpeg_core::CodecError> {
    let Some(verhor) = segment.vertical_horizontal else {
        return Ok(None);
    };
    Ok(Some(match (segment.kind, verhor) {
        (crate::feature::definitions::FeatureSegmentKind::Line(_), 0) => {
            SketchConstraintDefinitionInput::Vertical { entity }
        }
        (crate::feature::definitions::FeatureSegmentKind::Line(_), 1) => {
            SketchConstraintDefinitionInput::Horizontal { entity }
        }
        _ => native_section_segment_verhor_definition(
            ctx,
            sketch,
            entity,
            segment.external_id,
            verhor,
        )?,
    }))
}

pub(super) fn native_section_segment_verhor_definition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &SketchId,
    entity: SketchEntityId,
    external_id: u32,
    verhor: u32,
) -> Result<SketchConstraintDefinitionInput, cadmpeg_core::CodecError> {
    let native_kind = ctx.copy_retained_text("creo:segtab:verhor", "creo verhor native kind")?;
    let native_kind =
        cadmpeg_core::text::NonBlankString::for_decode(ctx, native_kind, "validate nonblank text")?
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("native kind must not be empty"))?;
    let key = ctx.copy_retained_text("verhor", "creo verhor property key")?;
    let value = ctx.format_retained(format_args!("{verhor}"), "creo verhor property value")?;
    let mut native_properties = BTreeMap::new();
    ctx.insert_btree_map(
        &mut native_properties,
        key,
        value,
        "creo verhor property nodes",
    )?;
    let mut entities = Vec::new();
    ctx.reserve_vec(&mut entities, 1, "creo verhor entity references")?;
    entities.push(entity);
    let operand_kind = ctx.copy_retained_text("segtab_ptr", "creo verhor operand kind")?;
    let field = ctx.copy_retained_text("ext_id", "creo verhor operand field")?;
    let native_ref = sketch_native_ref_admitted(ctx, sketch)?;
    let mut operands = Vec::new();
    ctx.reserve_vec(&mut operands, 1, "creo verhor operands")?;
    operands.push(SketchNativeOperand {
        native_kind: cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            operand_kind,
            "validate nonblank text",
        )?
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("operand kind must not be empty"))?,
        field: Some(NativeOperandField {
            name: cadmpeg_core::text::NonBlankString::for_decode(
                ctx,
                field,
                "validate nonblank text",
            )?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("operand field must not be empty")
            })?,
            role: None,
        }),
        object_index: Some(external_id),
        native_ref: Some(native_ref),
    });
    Ok(SketchConstraintDefinitionInput::Native {
        native_kind,
        native_state: None,
        native_flags: None,
        native_properties,
        entities,
        parameter: None,
        operands,
    })
}

pub(in super::super) fn reconcile_constraint_entity_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &mut SketchConstraintDefinitionInput,
    emitted: &BTreeSet<SketchEntityId>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let locus_emitted = |locus: &SketchLocus| -> Result<bool, cadmpeg_core::CodecError> {
        match locus {
            SketchLocus::Entity(entity)
            | SketchLocus::Start(entity)
            | SketchLocus::End(entity)
            | SketchLocus::Center(entity) => {
                ctx.contains_btree_set(emitted, entity, "creo constraint emitted entity membership")
            }
        }
    };
    Ok(match definition {
        SketchConstraintDefinitionInput::Native { entities, .. } => {
            ctx.retain_vec(
                entities,
                |entity| {
                    ctx.contains_btree_set(
                        emitted,
                        entity,
                        "creo constraint emitted entity membership",
                    )
                },
                "creo constraint emitted entity retention",
            )?;
            true
        }
        SketchConstraintDefinitionInput::Coincident { entities }
        | SketchConstraintDefinitionInput::Distance { entities, .. } => {
            ctx.all_by(entities, |entity| ctx.contains_btree_set(emitted, entity, "creo constraint emitted entity membership"), "creo constraint entity references")?
        }
        SketchConstraintDefinitionInput::CoincidentLoci { loci } => {
            ctx.all_by(loci.iter(), &locus_emitted, "creo coincident constraint loci")?
        }
        SketchConstraintDefinitionInput::SameCoordinate { relation } => {
            locus_emitted(relation.first())? && locus_emitted(relation.second())?
        }
        SketchConstraintDefinitionInput::TangentLoci { first, second }
        | SketchConstraintDefinitionInput::DistanceLoci { first, second, .. }
        | SketchConstraintDefinitionInput::DistanceLociValue { first, second, .. }
        | SketchConstraintDefinitionInput::MidpointCoordinate { first, second, .. }
        | SketchConstraintDefinitionInput::PolarDistance { first, second, .. }
        | SketchConstraintDefinitionInput::HorizontalDistance { first, second, .. }
        | SketchConstraintDefinitionInput::VerticalDistance { first, second, .. } => {
            locus_emitted(first)? && locus_emitted(second)?
        }
        SketchConstraintDefinitionInput::EqualDistance { first, second } => {
            locus_emitted(&first.first)?
                && locus_emitted(&first.second)?
                && locus_emitted(&second.first)?
                && locus_emitted(&second.second)?
        }
        SketchConstraintDefinitionInput::Midpoint { point, entity } => {
            locus_emitted(point)?
                && ctx.contains_btree_set(
                    emitted,
                    entity,
                    "creo constraint emitted entity membership",
                )?
        }
        SketchConstraintDefinitionInput::PointCoordinateValues { point, .. } => {
            locus_emitted(point)?
        }
        SketchConstraintDefinitionInput::AtIntersection {
            point,
            first,
            second,
        } => {
            locus_emitted(point)?
                && ctx.contains_btree_set(
                    emitted,
                    first,
                    "creo constraint emitted entity membership",
                )?
                && ctx.contains_btree_set(
                    emitted,
                    second,
                    "creo constraint emitted entity membership",
                )?
        }
        SketchConstraintDefinitionInput::PointOnObject { point, entity } => {
            locus_emitted(point)?
                && ctx.contains_btree_set(
                    emitted,
                    entity,
                    "creo constraint emitted entity membership",
                )?
        }
        SketchConstraintDefinitionInput::Symmetric {
            first,
            second,
            axis,
        } => {
            locus_emitted(first)?
                && locus_emitted(second)?
                && ctx.contains_btree_set(
                    emitted,
                    axis,
                    "creo constraint emitted entity membership",
                )?
        }
        SketchConstraintDefinitionInput::PointSymmetric {
            first,
            second,
            center,
        } => locus_emitted(first)? && locus_emitted(second)? && locus_emitted(center)?,
        SketchConstraintDefinitionInput::Concentric { first, second }
        | SketchConstraintDefinitionInput::Coradial { first, second }
        | SketchConstraintDefinitionInput::Collinear { first, second }
        | SketchConstraintDefinitionInput::ProjectedCopy {
            source: first,
            result: second,
        }
        | SketchConstraintDefinitionInput::Parallel { first, second }
        | SketchConstraintDefinitionInput::Perpendicular { first, second }
        | SketchConstraintDefinitionInput::Tangent { first, second }
        | SketchConstraintDefinitionInput::Equal { first, second }
        | SketchConstraintDefinitionInput::Angle { first, second, .. } => {
            ctx.contains_btree_set(emitted, first, "creo constraint emitted entity membership")?
                && ctx.contains_btree_set(
                    emitted,
                    second,
                    "creo constraint emitted entity membership",
                )?
        }
        SketchConstraintDefinitionInput::Horizontal { entity }
        | SketchConstraintDefinitionInput::Vertical { entity }
        | SketchConstraintDefinitionInput::Fixed { entity }
        | SketchConstraintDefinitionInput::Radius { entity, .. }
        | SketchConstraintDefinitionInput::Diameter { entity, .. } => {
            ctx.contains_btree_set(emitted, entity, "creo constraint emitted entity membership")?
        }
        SketchConstraintDefinitionInput::ArcAngle { entity, .. }
        | SketchConstraintDefinitionInput::EllipseAngle { entity, .. } => {
            ctx.contains_btree_set(emitted, entity, "creo constraint emitted entity membership")?
        }
        SketchConstraintDefinitionInput::SnellsLaw {
            incident,
            refracted,
            interface,
            ..
        } => {
            locus_emitted(incident)?
                && locus_emitted(refracted)?
                && ctx.contains_btree_set(
                    emitted,
                    interface,
                    "creo constraint emitted entity membership",
                )?
        }
        SketchConstraintDefinitionInput::Weight { entity, .. } => {
            ctx.contains_btree_set(emitted, entity, "creo constraint emitted entity membership")?
        }
        SketchConstraintDefinitionInput::InternalAlignment { helper, parent, .. } => {
            ctx.contains_btree_set(emitted, helper, "creo constraint emitted entity membership")?
                && ctx.contains_btree_set(
                    emitted,
                    parent,
                    "creo constraint emitted entity membership",
                )?
        }
        SketchConstraintDefinitionInput::Group { elements }
        | SketchConstraintDefinitionInput::Text { elements, .. } => {
            ctx.all_by(elements.iter(), &locus_emitted, "creo grouped constraint loci")?
        }
        SketchConstraintDefinitionInput::Disabled {} => true,
        _ => true,
    })
}

pub(in super::super) fn reconcile_constraint_parameter_reference(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &mut SketchConstraintDefinitionInput,
    emitted: &BTreeSet<ParameterId>,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(match definition {
        SketchConstraintDefinitionInput::Native { parameter, .. } => {
            let should_remove = match parameter.as_ref() {
                Some(parameter) => !ctx.contains_btree_set(
                    emitted,
                    parameter,
                    "creo constraint parameter membership",
                )?,
                None => false,
            };
            if should_remove {
                *parameter = None;
            }
            true
        }
        SketchConstraintDefinitionInput::PolarDistance {
            distance_parameter, ..
        } => {
            let should_remove = match distance_parameter.as_ref() {
                Some(parameter) => !ctx.contains_btree_set(
                    emitted,
                    parameter,
                    "creo constraint parameter membership",
                )?,
                None => false,
            };
            if should_remove {
                *distance_parameter = None;
            }
            true
        }
        SketchConstraintDefinitionInput::DistanceLociValue { parameter, .. } => {
            let should_remove = match parameter.as_ref() {
                Some(parameter) => !ctx.contains_btree_set(
                    emitted,
                    parameter,
                    "creo constraint parameter membership",
                )?,
                None => false,
            };
            if should_remove {
                *parameter = None;
            }
            true
        }
        SketchConstraintDefinitionInput::Distance { parameter, .. }
        | SketchConstraintDefinitionInput::DistanceLoci { parameter, .. }
        | SketchConstraintDefinitionInput::HorizontalDistance { parameter, .. }
        | SketchConstraintDefinitionInput::VerticalDistance { parameter, .. }
        | SketchConstraintDefinitionInput::Angle { parameter, .. }
        | SketchConstraintDefinitionInput::Radius { parameter, .. }
        | SketchConstraintDefinitionInput::Diameter { parameter, .. } => {
            ctx.contains_btree_set(emitted, parameter, "creo constraint parameter membership")?
        }
        SketchConstraintDefinitionInput::SnellsLaw { parameter, .. }
        | SketchConstraintDefinitionInput::Weight { parameter, .. } => {
            ctx.contains_btree_set(emitted, parameter, "creo constraint parameter membership")?
        }
        SketchConstraintDefinitionInput::Coincident { .. }
        | SketchConstraintDefinitionInput::CoincidentLoci { .. }
        | SketchConstraintDefinitionInput::SameCoordinate { .. }
        | SketchConstraintDefinitionInput::Midpoint { .. }
        | SketchConstraintDefinitionInput::PointCoordinateValues { .. }
        | SketchConstraintDefinitionInput::MidpointCoordinate { .. }
        | SketchConstraintDefinitionInput::Concentric { .. }
        | SketchConstraintDefinitionInput::Coradial { .. }
        | SketchConstraintDefinitionInput::Collinear { .. }
        | SketchConstraintDefinitionInput::Symmetric { .. }
        | SketchConstraintDefinitionInput::PointSymmetric { .. }
        | SketchConstraintDefinitionInput::Horizontal { .. }
        | SketchConstraintDefinitionInput::Vertical { .. }
        | SketchConstraintDefinitionInput::Parallel { .. }
        | SketchConstraintDefinitionInput::Perpendicular { .. }
        | SketchConstraintDefinitionInput::Tangent { .. }
        | SketchConstraintDefinitionInput::TangentLoci { .. }
        | SketchConstraintDefinitionInput::Equal { .. }
        | SketchConstraintDefinitionInput::EqualDistance { .. }
        | SketchConstraintDefinitionInput::Fixed { .. } => true,
        SketchConstraintDefinitionInput::Disabled {}
        | SketchConstraintDefinitionInput::PointOnObject { .. }
        | SketchConstraintDefinitionInput::AtIntersection { .. }
        | SketchConstraintDefinitionInput::ArcAngle { .. }
        | SketchConstraintDefinitionInput::EllipseAngle { .. }
        | SketchConstraintDefinitionInput::InternalAlignment { .. }
        | SketchConstraintDefinitionInput::Group { .. }
        | SketchConstraintDefinitionInput::Text { .. } => true,
        _ => true,
    })
}

pub(in super::super) fn close_sketch_constraint_parameter_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo emitted parameter scratch storage")?;
    let mut emitted = BTreeSet::new();
    for parameter in ctx.admit_iter(&ir.model.parameters, "creo emitted parameter rows")? {
        if !ctx.contains_btree_set(
            &emitted,
            &parameter.id,
            "creo emitted parameter identity membership",
        )? {
            storage.with_storage(|| ctx.insert_btree_set(
                &mut emitted,
                parameter
                    .id
                    .try_clone_for_decode(ctx, "creo emitted parameter identity")?,
                "creo emitted parameter ID nodes",
            ))?;
        }
    }
    ctx.retain_mut(
        &mut ir.model.sketch_constraints,
        |constraint| match constraint
            .definition
            .edit(|kind| reconcile_constraint_parameter_reference(ctx, kind, &emitted))
        {
            Ok(result) => result,
            Err(_) => Ok(false),
        },
        "creo sketch constraint parameter reconciliation",
    )?;
    Ok(())
}

#[cfg(test)]
pub(in super::super) fn joined_relation_incidence<'definition>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &'definition crate::feature::definitions::FeatureDefinition,
    relation_id: u32,
) -> Result<Option<&'definition crate::feature::definitions::FeatureSkamp>, cadmpeg_core::CodecError> {
    Ok(RelationIncidences::new(ctx, definition)?.joined(relation_id).map(|(_, incidence)| incidence))
}

#[cfg(test)]
pub(in super::super) fn relation_incidence<'definition>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &'definition crate::feature::definitions::FeatureDefinition,
    relation_id: u32,
) -> Result<Option<&'definition crate::feature::definitions::FeatureSkamp>, cadmpeg_core::CodecError> {
    Ok(joined_relation_incidence(ctx, definition, relation_id)?.filter(|incidence| section_skamp_active(incidence.status)))
}

fn incidence_entities(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &SketchId,
    incidence: Option<&crate::feature::definitions::FeatureSkamp>,
    operation: &'static str,
) -> Result<Vec<SketchEntityId>, cadmpeg_core::CodecError> {
    let Some(incidence) = incidence else { return Ok(Vec::new()); };
    let mut entities = Vec::new();
    for item in ctx.admit_iter(&incidence.items, operation)? {
        if let Some(entity) = sketch_entity_id_admitted(ctx, sketch, item.entity_id)? {
            ctx.push_vec(&mut entities, entity, operation)?;
        }
    }
    Ok(entities)
}

#[cfg(test)]
pub(in super::super) fn relation_incidence_entities(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    relation_id: u32,
) -> Result<Vec<SketchEntityId>, cadmpeg_core::CodecError> {
    incidence_entities(ctx, sketch, relation_incidence(ctx, definition, relation_id)?, "creo relation incidence items")
}

#[cfg(test)]
pub(in super::super) fn joined_relation_incidence_entities(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    relation_id: u32,
) -> Result<Vec<SketchEntityId>, cadmpeg_core::CodecError> {
    incidence_entities(ctx, sketch, joined_relation_incidence(ctx, definition, relation_id)?, "creo joined relation incidence items")
}

fn relation_incidence_loci(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refusal: &Cell<Option<cadmpeg_core::CodecError>>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    incidence: Option<&crate::feature::definitions::FeatureSkamp>,
) -> Result<Option<[SketchLocus; 2]>, cadmpeg_core::CodecError> {
    let Some(incidence) = incidence.filter(|incidence| section_skamp_active(incidence.status)) else { return Ok(None); };
    let [first, second] = incidence.items.as_slice() else { return Ok(None); };
    let Some(first) = section_skamp_locus(ctx, refusal, definition, sketch, first)? else { return Ok(None); };
    let Some(second) = section_skamp_locus(ctx, refusal, definition, sketch, second)? else { return Ok(None); };
    Ok(Some([first, second]))
}

fn section_angular_entities(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    vectors: [[Option<u32>; 4]; 3],
    known_entities: &BTreeSet<u32>,
) -> Result<Option<[SketchEntityId; 2]>, cadmpeg_core::CodecError> {
    let [Some(first_internal), Some(second_internal), None, Some(1)] = vectors[0] else {
        return Ok(None);
    };
    let Some(order_table) = definition.order_table.as_ref() else {
        return Ok(None);
    };
    let external_id = |internal_id| -> Result<Option<u32>, cadmpeg_core::CodecError> {
        let Some(external_id) = order_table.external_id(internal_id) else {
            return Ok(None);
        };
        let is_line = unique_decoded_section_segment(definition, external_id)
            .is_some_and(|segment| matches!(segment.kind, crate::feature::definitions::FeatureSegmentKind::Line(_)));
        Ok((ctx.contains_btree_set(known_entities, &external_id, "creo angular entity membership")? && is_line).then_some(external_id))
    };
    let first = external_id(first_internal)?;
    let second = external_id(second_internal)?;
    let [Some(first), Some(second)] = [first, second] else {
        return Ok(None);
    };
    if first == second {
        return Ok(None);
    }
    let Some(first) = sketch_entity_id_admitted(ctx, sketch, first)? else {
        return Ok(None);
    };
    let Some(second) = sketch_entity_id_admitted(ctx, sketch, second)? else {
        return Ok(None);
    };
    Ok(Some([first, second]))
}

fn segment_radius_operand(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &SketchId,
    kind: &str,
    field: &str,
    object_index: u32,
) -> Result<SketchNativeOperand, cadmpeg_core::CodecError> {
    let kind = ctx.copy_retained_text(kind, "creo radius operand kind")?;
    let field = ctx.copy_retained_text(field, "creo radius operand field")?;
    Ok(SketchNativeOperand {
        native_kind: cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            kind,
            "validate nonblank text",
        )?
        .ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("radius operand kind must not be empty")
        })?,
        field: Some(NativeOperandField {
            name: cadmpeg_core::text::NonBlankString::for_decode(
                ctx,
                field,
                "validate nonblank text",
            )?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("radius operand field must not be empty")
            })?,
            role: None,
        }),
        object_index: Some(object_index),
        native_ref: Some(sketch_native_ref_admitted(ctx, sketch)?),
    })
}

fn native_section_segment_radius_definition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &SketchId,
    entity: SketchEntityId,
    external_id: u32,
    field: &str,
    dimension_ordinal: u32,
) -> Result<SketchConstraintDefinitionInput, cadmpeg_core::CodecError> {
    let native_kind = ctx.format_retained(
        format_args!("creo:segtab:{field}"),
        "creo radius native kind",
    )?;
    let native_kind =
        cadmpeg_core::text::NonBlankString::for_decode(ctx, native_kind, "validate nonblank text")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("radius native kind must not be empty")
            })?;
    let key = ctx.copy_retained_text("dimension_ordinal", "creo radius property key")?;
    let value = ctx.format_retained(
        format_args!("{dimension_ordinal}"),
        "creo radius property value",
    )?;
    let mut native_properties = BTreeMap::new();
    ctx.insert_btree_map(
        &mut native_properties,
        key,
        value,
        "creo radius property nodes",
    )?;
    let mut entities = Vec::new();
    ctx.reserve_vec(&mut entities, 1, "creo radius entity references")?;
    entities.push(entity);
    let first = segment_radius_operand(ctx, sketch, "segtab_ptr", "ext_id", external_id)?;
    let second =
        segment_radius_operand(ctx, sketch, "dimension_ordinal", field, dimension_ordinal)?;
    let mut operands = Vec::new();
    ctx.reserve_vec(&mut operands, 2, "creo radius operands")?;
    operands.push(first);
    operands.push(second);
    Ok(SketchConstraintDefinitionInput::Native {
        native_kind,
        native_state: None,
        native_flags: None,
        native_properties,
        entities,
        parameter: None,
        operands,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SegmentRadiusField {
    Primary,
    Secondary,
}

impl SegmentRadiusField {
    const fn key(self) -> &'static str {
        match self {
            Self::Primary => "radius",
            Self::Secondary => "radius2",
        }
    }
}

struct SectionSegmentRadiusBinding {
    suffix: String,
    external_id: u32,
    field: SegmentRadiusField,
    ordinal: u32,
    offset: usize,
    typed_circle: Option<(u32, ParameterId)>,
}

fn section_segment_radius_bindings(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<Vec<SectionSegmentRadiusBinding>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let unique_segment_ids = scratch_storage.with_storage(|| unique_section_segment_external_ids(ctx, definition))?;
    let mut bindings = Vec::new();
    let Some(segments) = definition.segments.as_ref() else {
        return Ok(bindings);
    };
    for segment in ctx
        .admit_iter(
            segments.rows.as_slice(),
            "creo segment radius ordinary rows",
        )?
        .filter_map(|row| match row {
            SegmentRow::Ordinary(segment) => Some(segment),
            _ => None,
        })
    {
        if segment.radius_ref.is_none() && segment.radius2_ref.is_none() {
            continue;
        }
        let suffix = storage.with_storage(|| section_segment_identity_suffix_admitted(ctx, &unique_segment_ids, segment))?;
        for (field, ordinal) in [
            (SegmentRadiusField::Primary, segment.radius_ref),
            (SegmentRadiusField::Secondary, segment.radius2_ref),
        ] {
            let Some(ordinal) = ordinal else {
                continue;
            };
            storage.with_storage(|| ctx.reserve_vec(&mut bindings, 1, "creo segment radius bindings"))?;
            bindings.push(SectionSegmentRadiusBinding {
                suffix: storage.with_storage(|| ctx.copy_retained_text(&suffix, "creo radius binding suffix copy"))?,
                external_id: segment.external_id,
                field,
                ordinal,
                offset: segment.offset,
                typed_circle: None,
            });
        }
    }
    for segment in ctx
        .admit_iter(segments.rows.as_slice(), "creo segment radius circle rows")?
        .filter_map(|row| match row {
            SegmentRow::Circle(segment) => Some(segment),
            _ => None,
        })
    {
        let unique_id = ctx.contains_btree_set(&unique_segment_ids, &segment.external_id, "creo radius binding identity membership")?;
        let suffix = if unique_id {
            storage.with_storage(|| ctx.format_retained(
                format_args!("{}", segment.external_id),
                "creo radius circle suffix",
            ))?
        } else {
            storage.with_storage(|| ctx.format_retained(
                format_args!("circle:offset:{}", segment.offset),
                "creo radius circle suffix",
            ))?
        };
        let typed_circle = if unique_id {
            match (
                usize::try_from(segment.radius_ref).ok(),
                definition.dimensions.as_ref(),
            ) {
                (Some(ordinal), Some(dimensions)) => {
                    resolved_feature_dimension_parameter_admitted(ctx, sketch, dimensions, ordinal)?
                        .map(|(dimension, parameter)| (dimension.dimension_type, parameter))
                }
                _ => None,
            }
        } else {
            None
        };
        storage.with_storage(|| ctx.reserve_vec(&mut bindings, 1, "creo segment radius bindings"))?;
        bindings.push(SectionSegmentRadiusBinding {
            suffix,
            external_id: segment.external_id,
            field: SegmentRadiusField::Primary,
            ordinal: segment.radius_ref,
            offset: segment.offset,
            typed_circle,
        });
    }
    for segment in ctx
        .admit_iter(segments.rows.as_slice(), "creo segment radius opaque rows")?
        .filter_map(|row| match row {
            SegmentRow::Opaque(segment) => Some(segment),
            _ => None,
        })
    {
        if segment.radius_ref.is_none() && segment.radius2_ref.is_none() {
            continue;
        }
        let suffix =
            storage.with_storage(|| opaque_section_segment_identity_suffix_admitted(ctx, &unique_segment_ids, segment))?;
        for (field, ordinal) in [
            (SegmentRadiusField::Primary, segment.radius_ref),
            (SegmentRadiusField::Secondary, segment.radius2_ref),
        ] {
            let Some(ordinal) = ordinal else {
                continue;
            };
            storage.with_storage(|| ctx.reserve_vec(&mut bindings, 1, "creo segment radius bindings"))?;
            bindings.push(SectionSegmentRadiusBinding {
                suffix: storage.with_storage(|| ctx.copy_retained_text(&suffix, "creo radius binding suffix copy"))?,
                external_id: segment.external_id,
                field,
                ordinal,
                offset: segment.offset,
                typed_circle: None,
            });
        }
    }
    Ok(bindings)
}

fn section_segment_radius_constraint(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    binding: &mut SectionSegmentRadiusBinding,
    sketch: &SketchId,
) -> Result<Option<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let Some(entity) = sketch_entity_id_admitted(ctx, sketch, &binding.suffix)? else {
        return Ok(None);
    };
    let (definition, kind) = match binding.typed_circle.take() {
        Some((dimension_type, parameter)) if matches!(dimension_type, 3 | 4) => (
            circular_dimension_constraint(entity, parameter, dimension_type),
            if dimension_type == 4 {
                "diameter"
            } else {
                "radius"
            },
        ),
        _ => (
            native_section_segment_radius_definition(
                ctx,
                sketch,
                entity,
                binding.external_id,
                binding.field.key(),
                binding.ordinal,
            )?,
            if binding.field == SegmentRadiusField::Secondary {
                "segtab-radius2"
            } else {
                "segtab-radius"
            },
        ),
    };
    let Some(id) =
        sketch_constraint_id_admitted(ctx, sketch, format_args!("{kind}:{}", binding.suffix))?
    else {
        return Ok(None);
    };
    let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
    else {
        return Ok(None);
    };
    Ok(Some((
        SketchConstraint {
            id,
            sketch: sketch.try_clone_for_decode(ctx, "creo radius constraint sketch identity")?,
            definition,
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: Some(sketch_native_ref_admitted(ctx, sketch)?),
        },
        binding.offset,
    )))
}

pub(in super::super) fn section_segment_radius_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut constraints = Vec::new();
    let mut binding_storage = ctx.reserve_scoped(0, "creo radius binding scratch storage")?;
    let bindings = section_segment_radius_bindings(ctx, definition, sketch, &mut binding_storage)?;
    for mut binding in ctx.admit_iter(bindings, "creo segment radius binding rows")? {
        if let Some(constraint) = section_segment_radius_constraint(ctx, &mut binding, sketch)? {
            ctx.reserve_vec(&mut constraints, 1, "creo segment radius constraints")?;
            constraints.push(constraint);
        }
    }
    Ok(constraints)
}

pub(in super::super) fn section_segment_radius_constraints_for_emitted(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    emitted: &BTreeSet<SketchEntityId>,
    available_parameters: &BTreeSet<ParameterId>,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut binding_storage = ctx.reserve_scoped(0, "creo radius binding scratch storage")?;
    let mut bindings = section_segment_radius_bindings(ctx, definition, sketch, &mut binding_storage)?;
    let mut candidate_storage = ctx.reserve_scoped(0, "creo emitted radius candidate storage")?;
    let mut candidates = Vec::new();
    for binding in ctx.admit_iter(&mut bindings, "creo segment radius binding rows")? {
        if let Some(constraint) = section_segment_radius_constraint(ctx, binding, sketch)? {
            ctx.push_scoped_vec(&mut candidate_storage, &mut candidates, constraint, "creo segment radius constraints")?;
        }
    }
    let mut constraints = Vec::new();
    let mut candidates = candidates.into_iter().zip(bindings);
    while let Some(((mut constraint, offset), binding)) = ctx.next_charged(&mut candidates, "creo emitted radius binding rows")? {
        let reconciled = match constraint.definition.edit(|kind| {
            reconcile_section_segment_radius_constraint(
                ctx,
                kind,
                sketch,
                &binding,
                emitted,
                available_parameters,
            )
        }) {
            Ok(result) => result?,
            Err(_) => false,
        };
        if reconciled {
            ctx.reserve_vec(
                &mut constraints,
                1,
                "creo emitted segment radius constraints",
            )?;
            constraints.push((constraint, offset));
        }
    }
    Ok(constraints)
}

fn reconcile_section_segment_radius_constraint(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    constraint_definition: &mut SketchConstraintDefinitionInput,
    sketch: &SketchId,
    binding: &SectionSegmentRadiusBinding,
    emitted: &BTreeSet<SketchEntityId>,
    available_parameters: &BTreeSet<ParameterId>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let entity_reconciled =
        reconcile_constraint_entity_references(ctx, constraint_definition, emitted)?;
    let parameter_reconciled =
        reconcile_constraint_parameter_reference(ctx, constraint_definition, available_parameters)?;
    if entity_reconciled && parameter_reconciled {
        return Ok(true);
    }
    let Some(entity) = sketch_entity_id_admitted(ctx, sketch, &binding.suffix)? else {
        return Ok(false);
    };
    let native_definition = native_section_segment_radius_definition(
        ctx,
        sketch,
        entity,
        binding.external_id,
        binding.field.key(),
        binding.ordinal,
    )?;
    *constraint_definition = native_definition;
    Ok(
        reconcile_constraint_entity_references(ctx, constraint_definition, emitted)?
            && reconcile_constraint_parameter_reference(
                ctx,
                constraint_definition,
                available_parameters,
            )?,
    )
}

pub(in super::super) fn section_equation_radius_dimension_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let Some(segments) = definition.segments.as_ref() else {
        return Ok(Vec::new());
    };
    let Some(dimensions) = definition.dimensions.as_ref() else {
        return Ok(Vec::new());
    };
    let unique_segment_ids = scratch_storage.with_storage(|| unique_section_segment_external_ids(ctx, definition))?;
    let mut entities_by_radius = BTreeMap::<u32, Vec<u32>>::new();
    for segment in ctx
        .admit_iter(
            segments.rows.as_slice(),
            "creo equation radius ordinary rows",
        )?
        .filter_map(|row| match row {
            SegmentRow::Ordinary(segment) => Some(segment),
            _ => None,
        })
    {
        if !matches!(segment.kind, crate::feature::definitions::FeatureSegmentKind::Arc(_))
            || !ctx.contains_btree_set(&unique_segment_ids, &segment.external_id, "creo radius segment identity membership")? { continue; }
        if let Some(radius) = segment.radius_ref {
            let entities = scratch_storage.with_storage(|| ctx
                .entry_btree_map(
                    &mut entities_by_radius,
                    radius,
                    "creo equation radius group nodes",
                ))?
                .or_default();
            scratch_storage.with_storage(|| ctx.reserve_vec(entities, 1, "creo equation radius group entities"))?;
            entities.push(segment.external_id);
        }
    }
    for segment in ctx
        .admit_iter(segments.rows.as_slice(), "creo equation radius circle rows")?
        .filter_map(|row| match row {
            SegmentRow::Circle(segment) => Some(segment),
            _ => None,
        })
    {
        if !ctx.contains_btree_set(&unique_segment_ids, &segment.external_id, "creo radius segment identity membership")? { continue; }
        let entities = scratch_storage.with_storage(|| ctx
            .entry_btree_map(
                &mut entities_by_radius,
                segment.radius_ref,
                "creo equation radius group nodes",
            ))?
            .or_default();
        scratch_storage.with_storage(|| ctx.reserve_vec(entities, 1, "creo equation radius group entities"))?;
        entities.push(segment.external_id);
    }

    let mut constraints = Vec::new();
    let equations = scratch_storage.with_storage(|| section_equation_radius_dimensions(ctx, definition))?;
    for equation in ctx.admit_iter(&equations, "creo radius dimension equations")? {
        let Ok(ordinal) = usize::try_from(equation.scalar.1) else {
            continue;
        };
        let Some((dimension, parameter)) =
            scratch_storage.with_storage(|| resolved_feature_dimension_parameter_admitted(ctx, sketch, dimensions, ordinal))?
        else {
            continue;
        };
        let Some(dimension_value) = dimension
            .value
            .resolved()
            .filter(|value| value.is_finite() && *value > 0.0)
        else {
            continue;
        };
        if dimension.dimension_type != 3
            || !(FiniteReal::new(dimension_value))
                .zip(FiniteReal::new(equation.value.get()))
                .is_some_and(|(first, second)| approximately_equal(first, second))
        {
            continue;
        }
        let Some(entities) = ctx.get_btree_map(&entities_by_radius, &equation.radius, "creo equation radius group lookup")? else {
            continue;
        };
        for &external_id in ctx.admit_iter(entities, "creo radius equation entities")? {
            let Some(entity) = sketch_entity_id_admitted(ctx, sketch, external_id)? else {
                continue;
            };
            let Some(id) = sketch_constraint_id_admitted(
                ctx,
                sketch,
                format_args!("equation:{}:radius:{}", equation.equation_id, external_id),
            )?
            else {
                continue;
            };
            let parameter =
                parameter.try_clone_for_decode(ctx, "creo equation radius parameter copy")?;
            let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                SketchConstraintDefinitionInput::Radius { entity, parameter },
            ) else {
                continue;
            };
            ctx.reserve_vec(
                &mut constraints,
                1,
                "creo equation radius dimension constraints",
            )?;
            constraints.push((
                SketchConstraint {
                    id,
                    sketch: sketch.try_clone_for_decode(ctx, "creo equation sketch identity")?,
                    definition,
                    name: None,
                    driving: None,
                    active: Some(equation.active),
                    virtual_space: None,
                    visible: None,
                    orientation: None,
                    label_distance: None,
                    label_position: None,
                    metadata: None,
                    native_ref: Some(sketch_native_ref_admitted(ctx, sketch)?),
                },
                equation.offset,
            ));
        }
    }
    Ok(constraints)
}

pub(in super::super) fn section_equation_equal_distance_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| {
            scratch_storage.with_storage(|| variables.reconciled_points(ctx))
                .map(|points| points.ambiguous)
        })
        .transpose()?
        .unwrap_or_default();
    let equations =
        scratch_storage.with_storage(|| super::super::sketch::equations_coordinate::section_equation_equal_length_constraint_rows(
            ctx,
            definition,
            &ambiguous_point_ids,
        ))?;
    collect_constraint_candidates(
        ctx,
        &equations,
        "creo section equation equal distance constraints",
        |equation| {
            let Some(first_start) =
                section_point_locus(ctx, definition, sketch, equation.first[0])?
            else {
                return Ok(None);
            };
            let Some(first_end) = section_point_locus(ctx, definition, sketch, equation.first[1])?
            else {
                return Ok(None);
            };
            let Some(second_start) =
                section_point_locus(ctx, definition, sketch, equation.second[0])?
            else {
                return Ok(None);
            };
            let Some(second_end) =
                section_point_locus(ctx, definition, sketch, equation.second[1])?
            else {
                return Ok(None);
            };
            let first = SketchDistancePair {
                first: first_start,
                second: first_end,
            };
            let second = SketchDistancePair {
                first: second_start,
                second: second_end,
            };
            equation_constraint(
                ctx,
                sketch,
                equation.equation_id,
                SketchConstraintDefinitionInput::EqualDistance { first, second },
                equation.active,
                equation.offset,
            )
        },
    )
}

fn section_equation_radius_dimension_parameters(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<BTreeMap<SectionScalarVariable, Option<(ParameterId, f64)>>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let mut dimension_parameters =
        BTreeMap::<SectionScalarVariable, Option<(ParameterId, f64)>>::new();
    let Some(dimensions) = definition.dimensions.as_ref() else {
        return Ok(dimension_parameters);
    };
    let equations = scratch_storage.with_storage(|| section_equation_radius_dimensions(ctx, definition))?;
    for equation in ctx.admit_iter(&equations, "creo equation radius dimensions")? {
        let Some(ordinal) = usize::try_from(equation.scalar.1).ok() else {
            continue;
        };
        let Some((dimension, parameter)) =
            resolved_feature_dimension_parameter_admitted(ctx, sketch, dimensions, ordinal)?
        else {
            continue;
        };
        let Some(dimension_value) = dimension.value.resolved() else {
            continue;
        };
        if dimension.dimension_type != 3
            || !dimension_value.is_finite()
            || dimension_value <= 0.0
            || !(FiniteReal::new(dimension_value))
                .zip(FiniteReal::new(equation.value.get()))
                .is_some_and(|(first, second)| approximately_equal(first, second))
        {
            continue;
        }
        let candidate = (parameter, dimension_value);
        for variable in [equation.radius_variable, equation.scalar] {
            match ctx.entry_btree_map(&mut dimension_parameters, variable, "creo equation dimension parameter nodes")? {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let slot = entry.get_mut();
                    if !ctx.equal(&slot.as_ref(), &Some(&candidate), "creo equation dimension parameter agreement")? {
                        *slot = None;
                    }
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let copied_parameter = candidate.0.try_clone_for_decode(ctx, "creo equation dimension parameter copy")?;
                    entry.insert(Some((copied_parameter, candidate.1)));
                }
            }
        }
    }
    Ok(dimension_parameters)
}

fn section_equation_dimension_parameter(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parameters: &BTreeMap<SectionScalarVariable, Option<(ParameterId, f64)>>,
    variable: SectionScalarVariable,
    value: f64,
) -> Result<Option<ParameterId>, cadmpeg_core::CodecError> {
    let Some(Some((parameter, dimension_value))) = ctx.get_btree_map(parameters, &variable, "creo equation dimension parameter lookup")? else {
        return Ok(None);
    };
    if (FiniteReal::new(*dimension_value))
        .zip(FiniteReal::new(value))
        .is_some_and(|(first, second)| approximately_equal(first, second))
    {
        Ok(Some(parameter.try_clone_for_decode(
            ctx,
            "creo equation distance parameter copy",
        )?))
    } else {
        Ok(None)
    }
}

pub(in super::super) fn section_equation_function_six_distance_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let coordinates = scratch_storage.with_storage(|| resolved_section_coordinates(ctx, definition))?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| {
            scratch_storage.with_storage(|| variables.reconciled_points(ctx))
                .map(|points| points.ambiguous)
        })
        .transpose()?
        .unwrap_or_default();
    let dimension_parameters =
        scratch_storage.with_storage(|| section_equation_radius_dimension_parameters(ctx, definition, sketch))?;
    let equations = scratch_storage.with_storage(|| section_equation_function_six_distance_rows(
        ctx,
        definition,
        &coordinates,
        &ambiguous_point_ids,
    ))?;
    collect_constraint_candidates(
        ctx,
        &equations,
        "creo function six distance constraints",
        |equation| {
            let Some(distance) = equation.constraint_distance() else {
                return Ok(None);
            };
            let Some(first) = section_point_locus(ctx, definition, sketch, equation.first)? else {
                return Ok(None);
            };
            let Some(second) = section_point_locus(ctx, definition, sketch, equation.second)?
            else {
                return Ok(None);
            };
            let parameter = section_equation_dimension_parameter(
                ctx,
                &dimension_parameters,
                equation.radius,
                distance.get(),
            )?;
            equation_constraint(
                ctx,
                sketch,
                equation.equation_id,
                SketchConstraintDefinitionInput::DistanceLociValue {
                    first,
                    second,
                    distance: Length::from(distance),
                    parameter,
                },
                equation.active(),
                equation.offset,
            )
        },
    )
}

pub(in super::super) fn section_equation_function_forty_two_midpoint_coordinate_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let coordinates = scratch_storage.with_storage(|| resolved_section_coordinates(ctx, definition))?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| {
            scratch_storage.with_storage(|| variables.reconciled_points(ctx))
                .map(|points| points.ambiguous)
        })
        .transpose()?
        .unwrap_or_default();
    let equations = scratch_storage.with_storage(|| section_equation_function_forty_two_midpoint_coordinate_rows(
        ctx,
        definition,
        &coordinates,
        &ambiguous_point_ids,
    ))?;
    collect_constraint_candidates(
        ctx,
        &equations,
        "creo midpoint coordinate constraints",
        |equation| {
            let Some(value) = equation.value else {
                return Ok(None);
            };
            if !value.is_finite() {
                return Ok(None);
            }
            let Some(first) = section_point_locus(ctx, definition, sketch, equation.first)? else {
                return Ok(None);
            };
            let Some(second) = section_point_locus(ctx, definition, sketch, equation.second)?
            else {
                return Ok(None);
            };
            let axis = match equation.coordinate {
                SectionAxis::U => SketchCoordinateAxis::U,
                SectionAxis::V => SketchCoordinateAxis::V,
            };
            let Some(value) = Length::new(value) else {
                return Ok(None);
            };
            equation_constraint(
                ctx,
                sketch,
                equation.equation_id,
                SketchConstraintDefinitionInput::MidpointCoordinate {
                    first,
                    second,
                    axis,
                    value,
                },
                equation.active,
                equation.offset,
            )
        },
    )
}

pub(in super::super) fn section_equation_function_thirty_one_point_coordinate_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let coordinates = scratch_storage.with_storage(|| resolved_section_coordinates(ctx, definition))?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| {
            scratch_storage.with_storage(|| variables.reconciled_points(ctx))
                .map(|points| points.ambiguous)
        })
        .transpose()?
        .unwrap_or_default();
    let equations = scratch_storage.with_storage(|| section_equation_function_thirty_one_point_coordinate_rows(
        ctx,
        definition,
        &coordinates,
        &ambiguous_point_ids,
    ))?;
    collect_constraint_candidates(
        ctx,
        &equations,
        "creo point coordinate constraints",
        |equation| {
            let [u, v] = equation.values;
            let (Some(u), Some(v)) = (u, v) else {
                return Ok(None);
            };
            if !u.is_finite() || !v.is_finite() {
                return Ok(None);
            }
            let Some(point) = section_point_locus(ctx, definition, sketch, equation.point)? else {
                return Ok(None);
            };
            let (Some(u), Some(v)) = (Length::new(u), Length::new(v)) else {
                return Ok(None);
            };
            equation_constraint(
                ctx,
                sketch,
                equation.equation_id,
                SketchConstraintDefinitionInput::PointCoordinateValues {
                    point,
                    values: [u, v],
                },
                equation.active,
                equation.offset,
            )
        },
    )
}

pub(super) fn section_equation_function_sixteen_angle_difference_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let equations = scratch_storage.with_storage(|| section_equation_function_sixteen_angle_difference_rows(ctx, definition))?;
    collect_constraint_candidates(
        ctx,
        &equations,
        "creo section equation function sixteen angle difference constraints",
        |equation| {
            let Some(value) = Angle::new(equation.value) else {
                return Ok(None);
            };
            equation_constraint(
                ctx,
                sketch,
                equation.equation_id,
                SketchConstraintDefinitionInput::AngleDifference {
                    first: equation.first.1,
                    second: equation.second.1,
                    difference: equation.difference.1,
                    value,
                },
                equation.active,
                equation.offset,
            )
        },
    )
}

pub(super) fn section_equation_function_five_scalar_equality_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let equations = scratch_storage.with_storage(|| section_equation_function_five_scalar_equality_rows(ctx, definition))?;
    collect_constraint_candidates(
        ctx,
        &equations,
        "creo section equation function five scalar equality constraints",
        |equation| {
            equation_constraint(
                ctx,
                sketch,
                equation.equation_id,
                SketchConstraintDefinitionInput::ScalarEquality {
                    first: equation.first.1,
                    second: equation.second.1,
                },
                true,
                equation.offset,
            )
        },
    )
}

pub(in super::super) fn section_equation_polar_distance_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let coordinates = scratch_storage.with_storage(|| resolved_section_coordinates(ctx, definition))?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| {
            scratch_storage.with_storage(|| variables.reconciled_points(ctx))
                .map(|points| points.ambiguous)
        })
        .transpose()?
        .unwrap_or_default();
    let dimension_parameters =
        scratch_storage.with_storage(|| section_equation_radius_dimension_parameters(ctx, definition, sketch))?;
    let equations = scratch_storage.with_storage(|| section_equation_radial_constraint_rows(
        ctx,
        definition,
        &coordinates,
        &ambiguous_point_ids,
    ))?;
    collect_constraint_candidates(
        ctx,
        &equations,
        "creo polar distance constraints",
        |equation| {
            let Some(distance) = equation.radius_value else {
                return Ok(None);
            };
            let angle = if distance.get() <= EPS_POLAR_ZERO {
                None
            } else {
                let Some(angle) = equation.angle_value else {
                    return Ok(None);
                };
                Some(angle)
            };
            let Some(first) = section_point_locus(ctx, definition, sketch, equation.first)? else {
                return Ok(None);
            };
            let Some(second) = section_point_locus(ctx, definition, sketch, equation.second)?
            else {
                return Ok(None);
            };
            let distance_parameter = section_equation_dimension_parameter(
                ctx,
                &dimension_parameters,
                equation.radius,
                distance.get(),
            )?;
            equation_constraint(
                ctx,
                sketch,
                equation.equation_id,
                SketchConstraintDefinitionInput::PolarDistance {
                    first,
                    second,
                    distance: distance.into(),
                    angle,
                    distance_parameter,
                },
                equation.active,
                equation.offset,
            )
        },
    )
}

fn insert_native_equation_property(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    key: &'static str,
    value: impl std::fmt::Display,
) -> Result<(), cadmpeg_core::CodecError> {
    let key = ctx.copy_retained_text(key, "creo native equation property keys")?;
    let value = ctx.format_retained(
        format_args!("{value}"),
        "creo native equation property values",
    )?;
    ctx.insert_btree_map(
        properties,
        key,
        value,
        "creo native equation property nodes",
    )?;
    Ok(())
}

fn native_equation_nonblank(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<cadmpeg_core::text::NonBlankString, cadmpeg_core::CodecError> {
    cadmpeg_core::text::NonBlankString::for_decode(
        ctx,
        ctx.format_retained(value, operation)?,
        "validate nonblank text",
    )?
    .ok_or_else(|| cadmpeg_core::CodecError::malformed("native equation text must not be blank"))
}

#[derive(Debug)]
struct NativeEquationData {
    operands: Vec<SketchNativeOperand>,
    argument_slots: String,
    null_argument_ordinals: Option<String>,
}

fn native_equation_data(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    equation_id: u32,
    arguments: &[Option<u32>],
    native_ref: &str,
) -> Result<NativeEquationData, cadmpeg_core::CodecError> {
    let mut argument_slots = String::new();
    let mut null_argument_ordinals = String::new();
    let mut operands = Vec::new();
    ctx.reserve_vec(&mut operands, 1, "creo native equation operands")?;
    operands.push(SketchNativeOperand {
        native_kind: native_equation_nonblank(
            ctx,
            format_args!("eqtn_arr"),
            "creo equation operand kind",
        )?,
        field: Some(NativeOperandField {
            name: native_equation_nonblank(
                ctx,
                format_args!("equation_id"),
                "creo equation operand field",
            )?,
            role: None,
        }),
        object_index: Some(equation_id),
        native_ref: Some(ctx.copy_retained_text(native_ref, "creo equation operand reference")?),
    });
    for (slot, argument) in ctx
        .admit_iter(arguments, "creo native equation argument slots")?
        .enumerate()
    {
        let separator = if slot == 0 { "" } else { "," };
        match argument {
            Some(argument) => ctx.append_formatted_retained(&mut argument_slots,
                format_args!("{separator}{slot}:{argument}"), "creo native equation property values")?,
            None => {
                ctx.append_formatted_retained(&mut argument_slots,
                    format_args!("{separator}{slot}:null"), "creo native equation property values")?;
                let separator = if null_argument_ordinals.is_empty() { "" } else { "," };
                ctx.append_formatted_retained(&mut null_argument_ordinals,
                    format_args!("{separator}{slot}"), "creo native equation property values")?;
            }
        }
        let Some(object_index) = *argument else {
            continue;
        };
        ctx.reserve_vec(&mut operands, 1, "creo native equation operands")?;
        operands.push(SketchNativeOperand {
            native_kind: native_equation_nonblank(
                ctx,
                format_args!("var_arr"),
                "creo equation operand kind",
            )?,
            field: Some(NativeOperandField {
                name: native_equation_nonblank(
                    ctx,
                    format_args!("arguments[{slot}]"),
                    "creo equation operand field",
                )?,
                role: None,
            }),
            object_index: Some(object_index),
            native_ref: Some(
                ctx.copy_retained_text(native_ref, "creo equation operand reference")?,
            ),
        });
    }
    Ok(NativeEquationData { operands, argument_slots,
        null_argument_ordinals: (!null_argument_ordinals.is_empty()).then_some(null_argument_ordinals),
    })
}

pub(in super::super) fn section_equation_native_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    typed_offsets: &BTreeSet<usize>,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let Some(table) = scratch_storage.with_storage(|| crate::feature::definitions::equation_table(
        ctx,
        &definition.body,
        0,
        definition.body.len(),
    ))?
    else {
        return Ok(Vec::new());
    };
    let solver = EquationIncidences::new(ctx, definition)?;
    let mut constraints = Vec::new();
    for equation in ctx.admit_iter(&table.rows, "creo native equation rows")? {
        if ctx.contains_btree_set(typed_offsets, &equation.offset, "creo typed equation offset membership")? {
            continue;
        }
        let active = !solver.is_disabled(equation.equation_id);
        let native_ref = sketch_native_ref_admitted(ctx, sketch)?;
        let mut native_properties = BTreeMap::new();
        insert_native_equation_property(
            ctx,
            &mut native_properties,
            "equation_id",
            equation.equation_id,
        )?;
        insert_native_equation_property(
            ctx,
            &mut native_properties,
            "function_id",
            equation.function_id,
        )?;
        insert_native_equation_property(ctx, &mut native_properties, "offset", equation.offset)?;
        insert_native_equation_property(ctx, &mut native_properties, "table_offset", table.offset)?;
        insert_native_equation_property(
            ctx,
            &mut native_properties,
            "table_declared_count",
            table.declared_count,
        )?;
        insert_native_equation_property(ctx, &mut native_properties, "active", active)?;
        let NativeEquationData { operands, argument_slots, null_argument_ordinals } =
            native_equation_data(ctx, equation.equation_id, &equation.arguments, &native_ref)?;
        let key = ctx.copy_retained_text("argument_slots", "creo native equation property keys")?;
        ctx.insert_btree_map(&mut native_properties, key, argument_slots, "creo native equation property nodes")?;
        if let Some(ordinals) = null_argument_ordinals {
            let key = ctx.copy_retained_text("null_argument_ordinals", "creo native equation property keys")?;
            ctx.insert_btree_map(&mut native_properties, key, ordinals, "creo native equation property nodes")?;
        }
        if let Some(count) = equation.explicit_argument_count {
            insert_native_equation_property(
                ctx,
                &mut native_properties,
                "explicit_argument_count",
                count,
            )?;
        }
        if let Some(entity_ref) = table.entity_ref {
            insert_native_equation_property(
                ctx,
                &mut native_properties,
                "table_entity_ref",
                entity_ref,
            )?;
        }
        let Some(id) = sketch_constraint_id_admitted(
            ctx,
            sketch,
            format_args!("equation:offset:{}", equation.offset),
        )?
        else {
            continue;
        };
        let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Native {
                native_kind: native_equation_nonblank(
                    ctx,
                    format_args!("creo:equation:{}", equation.function_id),
                    "creo equation native kind",
                )?,
                native_state: Some(u64::from(active)),
                native_flags: None,
                native_properties,
                entities: Vec::new(),
                parameter: None,
                operands,
            },
        ) else {
            continue;
        };
        ctx.reserve_vec(&mut constraints, 1, "creo native equation constraints")?;
        constraints.push((
            SketchConstraint {
                id,
                sketch: sketch.try_clone_for_decode(ctx, "creo native equation sketch identity")?,
                definition,
                name: None,
                driving: None,
                active: Some(active),
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(native_ref),
            },
            equation.offset,
        ));
    }
    Ok(constraints)
}

pub(in super::super) fn section_equation_same_coordinate_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| {
            scratch_storage.with_storage(|| variables.reconciled_points(ctx))
                .map(|points| points.ambiguous)
        })
        .transpose()?
        .unwrap_or_default();
    let rows = scratch_storage.with_storage(|| super::super::sketch::equations_scalar::section_equation_coordinate_equality_rows(
        ctx,
        definition,
        &ambiguous_point_ids,
    ))?;
    collect_constraint_candidates(
        ctx,
        &rows,
        "creo section equation same coordinate constraints",
        |equation| {
            if !matches!(equation.function_id, 2 | 10 | 13) {
                return Ok(None);
            }
            let Some(first) = section_point_locus(ctx, definition, sketch, equation.first)? else {
                return Ok(None);
            };
            let Some(second) = section_point_locus(ctx, definition, sketch, equation.second)?
            else {
                return Ok(None);
            };
            let axis = match equation.axis {
                SectionAxis::U => SketchCoordinateAxis::U,
                SectionAxis::V => SketchCoordinateAxis::V,
            };
            let Ok(relation) =
                cadmpeg_ir::sketches::SketchSameCoordinate::try_new(first, second, axis)
            else {
                return Ok(None);
            };
            equation_constraint(
                ctx,
                sketch,
                equation.equation_id,
                SketchConstraintDefinitionInput::SameCoordinate { relation },
                equation.active,
                equation.offset,
            )
        },
    )
}

pub(in super::super) fn section_equation_point_on_line_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| {
            scratch_storage.with_storage(|| variables.reconciled_points(ctx))
                .map(|points| points.ambiguous)
        })
        .transpose()?
        .unwrap_or_default();
    let equations =
        scratch_storage.with_storage(|| section_equation_point_on_line_constraint_rows(ctx, definition, &ambiguous_point_ids))?;
    let mut storage = ctx.reserve_scoped(0, "creo point-on-line index storage")?;
    let mut lines_by_points = HashMap::new();
    if !equations.is_empty() {
        if let Some(table) = &definition.segments {
            for row in ctx.admit_iter(table.rows.as_slice(), "creo point-on-line segment rows")? {
                let (external_id, points) = match row {
                    SegmentRow::Ordinary(segment) if matches!(segment.kind, crate::feature::definitions::FeatureSegmentKind::Line(_)) => (segment.external_id, segment.point_ids()),
                    SegmentRow::ReferenceLine(segment) => {
                        let [Some(first), Some(second)] = segment.point_ids else { continue; };
                        (segment.external_id, [first, second])
                    }
                    SegmentRow::CenteredLine(segment) => (segment.external_id, [0, 1]),
                    _ => continue,
                };
                if table.rows.get(external_id).is_none() { continue; }
                storage.with_storage(|| {
                    ctx.entry_hash_map(&mut lines_by_points, point_pair(points), "creo point-on-line pair index")?
                        .and_modify(|unique| *unique = None).or_insert(Some(external_id));
                    Ok::<_, cadmpeg_core::CodecError>(())
                })?;
            }
        }
    }
    collect_constraint_candidates(
        ctx,
        &equations,
        "creo section equation point on line constraints",
        |equation| {
            let Some(point) = section_point_locus(ctx, definition, sketch, equation.target)? else {
                return Ok(None);
            };
            let Some(line_external_id) = lines_by_points.get(&point_pair([equation.first, equation.second])).copied().flatten() else { return Ok(None); };
            let Some(entity) = sketch_entity_id_admitted(ctx, sketch, line_external_id)? else {
                return Ok(None);
            };
            equation_constraint(
                ctx,
                sketch,
                equation.equation_id,
                SketchConstraintDefinitionInput::PointOnObject { point, entity },
                equation.active,
                equation.offset,
            )
        },
    )
}

pub(in super::super) fn section_equation_axis_distance_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let Some(dimensions) = definition.dimensions.as_ref() else {
        return Ok(Vec::new());
    };
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| {
            scratch_storage.with_storage(|| variables.reconciled_points(ctx))
                .map(|points| points.ambiguous)
        })
        .transpose()?
        .unwrap_or_default();
    let coordinates = scratch_storage.with_storage(|| resolved_section_coordinates(ctx, definition))?;
    let equations = scratch_storage.with_storage(|| section_equation_function_forty_three_axis_distance_rows(
        ctx,
        definition,
        &coordinates,
        &ambiguous_point_ids,
    ))?;
    collect_constraint_candidates(
        ctx,
        &equations,
        "creo axis distance constraints",
        |equation| {
            let Some(first) = section_point_locus(ctx, definition, sketch, equation.first)? else {
                return Ok(None);
            };
            let Some(second) = section_point_locus(ctx, definition, sketch, equation.second)?
            else {
                return Ok(None);
            };
            let Ok(ordinal) = usize::try_from(equation.scalar.1) else {
                return Ok(None);
            };
            let Some((dimension, parameter)) =
                resolved_feature_dimension_parameter_admitted(ctx, sketch, dimensions, ordinal)?
            else {
                return Ok(None);
            };
            let Some(dimension_value) = dimension.value.resolved() else {
                return Ok(None);
            };
            if !matches!(dimension.dimension_type, 1..=5)
                || !dimension_value.is_finite()
                || dimension_value < 0.0
                || !(FiniteReal::new(dimension_value))
                    .zip(FiniteReal::new(equation.value))
                    .is_some_and(|(first, second)| approximately_equal(first, second))
            {
                return Ok(None);
            }
            let definition = match equation.coordinate {
                SectionAxis::U => SketchConstraintDefinitionInput::HorizontalDistance {
                    first,
                    second,
                    parameter,
                },
                SectionAxis::V => SketchConstraintDefinitionInput::VerticalDistance {
                    first,
                    second,
                    parameter,
                },
            };
            equation_constraint(
                ctx,
                sketch,
                equation.equation_id,
                definition,
                equation.active,
                equation.offset,
            )
        },
    )
}

pub(in super::super) fn section_equation_unsigned_distance_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let Some(dimensions) = definition.dimensions.as_ref() else {
        return Ok(Vec::new());
    };
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| {
            scratch_storage.with_storage(|| variables.reconciled_points(ctx))
                .map(|points| points.ambiguous)
        })
        .transpose()?
        .unwrap_or_default();
    let equations =
        scratch_storage.with_storage(|| section_equation_unsigned_coordinate_distance_rows(ctx, definition, &ambiguous_point_ids))?;
    collect_constraint_candidates(
        ctx,
        &equations,
        "creo section equation unsigned distance constraints",
        |equation| {
            let Some(first) = section_point_locus(ctx, definition, sketch, equation.first)? else {
                return Ok(None);
            };
            let Some(second) = section_point_locus(ctx, definition, sketch, equation.second)?
            else {
                return Ok(None);
            };
            let Ok(ordinal) = usize::try_from(equation.scalar.1) else {
                return Ok(None);
            };
            let Some((_, parameter)) =
                resolved_feature_dimension_parameter_admitted(ctx, sketch, dimensions, ordinal)?
            else {
                return Ok(None);
            };
            let definition = match equation.coordinate {
                SectionAxis::U => SketchConstraintDefinitionInput::HorizontalDistance {
                    first,
                    second,
                    parameter,
                },
                SectionAxis::V => SketchConstraintDefinitionInput::VerticalDistance {
                    first,
                    second,
                    parameter,
                },
            };
            equation_constraint(
                ctx,
                sketch,
                equation.equation_id,
                definition,
                equation.active,
                equation.offset,
            )
        },
    )
}

fn circular_dimension_constraint(
    entity: SketchEntityId,
    parameter: ParameterId,
    dimension_type: u32,
) -> SketchConstraintDefinitionInput {
    if dimension_type == 4 {
        SketchConstraintDefinitionInput::Diameter { entity, parameter }
    } else {
        SketchConstraintDefinitionInput::Radius { entity, parameter }
    }
}

fn insert_relation_property(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    key: &'static str,
    value: impl std::fmt::Display,
) -> Result<(), cadmpeg_core::CodecError> {
    let value = ctx.format_retained(
        format_args!("{value}"),
        "creo native relation property value",
    )?;
    let key = ctx.copy_retained_text(key, "creo native relation property key")?;
    ctx.insert_btree_map(
        properties,
        key,
        value,
        "creo native relation property nodes",
    )?;
    Ok(())
}

fn push_relation_operand(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut Vec<SketchNativeOperand>,
    native_ref: &str,
    kind: &'static str,
    field: Option<String>,
    object_index: u32,
) -> Result<(), cadmpeg_core::CodecError> {
    let kind = ctx.copy_retained_text(kind, "creo native relation operand kind")?;
    let native_ref =
        ctx.copy_retained_text(native_ref, "creo native relation operand reference")?;
    let field = field
        .map(|name| {
            cadmpeg_core::text::NonBlankString::for_decode(ctx, name, "validate nonblank text")?
                .map(|name| NativeOperandField { name, role: None })
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("native operand field must not be empty")
                })
        })
        .transpose()?;
    ctx.reserve_vec(operands, 1, "creo native relation operands")?;
    operands.push(SketchNativeOperand {
        native_kind: cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            kind,
            "validate nonblank text",
        )?
        .ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("native operand kind must not be empty")
        })?,
        field,
        object_index: Some(object_index),
        native_ref: Some(native_ref),
    });
    Ok(())
}

fn native_section_dimension_constraint_definition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &SketchId,
    relation: &crate::feature::definitions::FeatureRelation,
    solver: &RelationIncidences<'_, '_>,
) -> Result<Option<SketchConstraintDefinitionInput>, cadmpeg_core::CodecError> {
    let definition = solver.definition;
    let native_kind = ctx.format_retained(
        format_args!("creo:relation:{}", relation.relation_type),
        "creo native relation kind",
    )?;
    let native_kind =
        cadmpeg_core::text::NonBlankString::for_decode(ctx, native_kind, "validate nonblank text")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("native relation kind must not be empty")
            })?;
    let mut native_properties = BTreeMap::new();
    insert_relation_property(
        ctx,
        &mut native_properties,
        "dimension_id",
        relation.dimension_id,
    )?;
    insert_relation_property(ctx, &mut native_properties, "sign", relation.sign)?;
    if definition.relations.is_none() {
        insert_relation_property(
            ctx,
            &mut native_properties,
            "relation_id",
            relation.relation_id,
        )?;
        return Ok(Some(SketchConstraintDefinitionInput::Native {
            native_kind,
            native_state: Some(u64::from(relation.used)),
            native_flags: None,
            native_properties,
            entities: Vec::new(),
            parameter: None,
            operands: Vec::new(),
        }));
    }
    let unique_relation_id = solver.is_unique(relation.relation_id);
    let joined_relation_incidence_link = if unique_relation_id { solver.joined(relation.relation_id) } else { None };
    let joined_incidence = joined_relation_incidence_link.map(|(_, incidence)| incidence);
    let parameter = match definition
        .dimensions
        .as_ref()
        .zip(usize::try_from(relation.dimension_id).ok())
    {
        Some((dimensions, ordinal)) => {
            resolved_feature_dimension_parameter_admitted(ctx, sketch, dimensions, ordinal)?
                .map(|(_, parameter)| parameter)
        }
        None => None,
    };
    let entities = if unique_relation_id {
        incidence_entities(ctx, sketch, joined_incidence, "creo joined relation incidence items")?
    } else {
        Vec::new()
    };
    if !unique_relation_id {
        insert_relation_property(
            ctx,
            &mut native_properties,
            "relation_id",
            relation.relation_id,
        )?;
    }
    let native_ref = sketch_native_ref_admitted(ctx, sketch)?;
    let mut operands = Vec::new();
    if unique_relation_id {
        push_relation_operand(
            ctx,
            &mut operands,
            &native_ref,
            "relat_ptr",
            None,
            relation.relation_id,
        )?;
    }
    if let Some(incidence) = joined_incidence {
        let field =
            ctx.copy_retained_text("triples_ptr.skamp_id", "creo native relation operand field")?;
        push_relation_operand(
            ctx,
            &mut operands,
            &native_ref,
            "skamp_ptr",
            Some(field),
            incidence.id,
        )?;
    }
    if let Some(equation_id) = joined_relation_incidence_link.and_then(|(join, _)| join.equation_id)
    {
        let field = ctx.copy_retained_text("equation_id", "creo native relation operand field")?;
        push_relation_operand(
            ctx,
            &mut operands,
            &native_ref,
            "triples_ptr",
            Some(field),
            equation_id,
        )?;
    }
    if let Some(vectors) = relation.operand_vectors {
        for (vector, values) in ["a", "b", "c"].into_iter().zip(vectors) {
            for (slot, value) in values.into_iter().enumerate() {
                if let Some(object_index) = value {
                    let field = ctx.format_retained(
                        format_args!("{vector}[{slot}]"),
                        "creo native relation operand field",
                    )?;
                    push_relation_operand(
                        ctx,
                        &mut operands,
                        &native_ref,
                        "relat_ptr",
                        Some(field),
                        object_index,
                    )?;
                }
            }
        }
    }
    Ok(Some(SketchConstraintDefinitionInput::Native {
        native_kind,
        native_state: Some(u64::from(relation.used)),
        native_flags: None,
        native_properties,
        entities,
        parameter,
        operands,
    }))
}

pub(super) fn reconcile_section_dimension_constraint(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    constraint_definition: &mut SketchConstraintDefinitionInput,
    sketch: &SketchId,
    relation: &crate::feature::definitions::FeatureRelation,
    emitted: &BTreeSet<SketchEntityId>,
    available_parameters: &BTreeSet<ParameterId>,
    solver: &RelationIncidences<'_, '_>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let entity_reconciled =
        reconcile_constraint_entity_references(ctx, constraint_definition, emitted)?;
    let parameter_reconciled =
        reconcile_constraint_parameter_reference(ctx, constraint_definition, available_parameters)?;
    if entity_reconciled && parameter_reconciled {
        return Ok(true);
    }
    let Some(native_definition) =
        native_section_dimension_constraint_definition(ctx, sketch, relation, solver)?
    else {
        return Ok(false);
    };
    *constraint_definition = native_definition;
    Ok(
        reconcile_constraint_entity_references(ctx, constraint_definition, emitted)?
            && reconcile_constraint_parameter_reference(
                ctx,
                constraint_definition,
                available_parameters,
            )?,
    )
}

fn capture_locus_refusal<T>(
    refusal: &Cell<Option<cadmpeg_core::CodecError>>,
    result: Result<T, cadmpeg_core::CodecError>,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            refusal.set(Some(refusal.take().unwrap_or(error)));
            None
        }
    }
}

fn capture_constraint_refusal<T>(
    refusal: &mut Option<cadmpeg_core::CodecError>,
    result: Result<T, cadmpeg_core::CodecError>,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            *refusal = Some(error);
            None
        }
    }
}

#[cfg(test)]
pub(in super::super) fn section_dimension_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize, usize)>, cadmpeg_core::CodecError> {
    let solver = RelationIncidences::new(ctx, definition)?;
    section_dimension_constraints_with_links(ctx, sketch, &solver)
}

pub(super) fn section_dimension_constraints_with_links(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &SketchId,
    solver: &RelationIncidences<'_, '_>,
) -> Result<Vec<(SketchConstraint, usize, usize)>, cadmpeg_core::CodecError> {
    let definition = solver.definition;
    let mut scratch_storage = ctx.reserve_scoped(0, "creo constraint scratch storage")?;
    let Some(relations) = &definition.relations else {
        return Ok(Vec::new());
    };
    if relations.rows.is_empty() { return Ok(Vec::new()); }
    let segments = scratch_storage.with_storage(|| section_segment_rows(ctx, definition))?;

    let known_entities = scratch_storage.with_storage(|| section_entity_external_ids(ctx, definition))?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| {
            scratch_storage.with_storage(|| variables.reconciled_points(ctx))
                .map(|points| points.ambiguous)
        })
        .transpose()?
        .unwrap_or_default();
    let resolved_coordinates = scratch_storage.with_storage(|| resolved_section_coordinates(ctx, definition))?;
    let saved_coordinate_witnesses =
        scratch_storage.with_storage(|| saved_section_coordinate_witnesses(ctx, definition, &ambiguous_point_ids))?;
    let mut index_storage = ctx.reserve_scoped(0, "creo dimension geometry indexes")?;
    let mut measured_by_points = HashMap::new();
    let mut circles_by_radius = HashMap::new();
    for segment in ctx.admit_iter(&segments, "creo dimension geometry index rows")? {
        index_storage.with_storage(|| {
            ctx.entry_hash_map(&mut measured_by_points, point_pair(segment.point_ids()), "creo measured point pair index")?
                .and_modify(|unique| *unique = None).or_insert(Some(segment));
            if matches!(segment.kind, crate::feature::definitions::FeatureSegmentKind::Arc(_)) {
                if let Some(radius) = segment.radius_ref {
                    ctx.entry_hash_map(&mut circles_by_radius, radius, "creo circular dimension radius index")?
                        .and_modify(|unique| *unique = None).or_insert(Some(segment.external_id));
                }
            }
            Ok::<_, cadmpeg_core::CodecError>(())
        })?;
    }
    if let Some(table) = &definition.segments {
        for row in ctx.admit_iter(table.rows.as_slice(), "creo circular dimension index rows")? {
            let SegmentRow::Circle(segment) = row else { continue; };
            index_storage.with_storage(|| {
                ctx.entry_hash_map(&mut circles_by_radius, segment.radius_ref, "creo circular dimension radius index")?
                    .and_modify(|unique| *unique = None).or_insert(Some(segment.external_id));
                Ok::<_, cadmpeg_core::CodecError>(())
            })?;
        }
    }
    let mut constraints = Vec::new();
    for (relation_index, relation) in ctx
        .admit_iter(&relations.rows, "creo section dimension relation rows")?
        .enumerate()
    {
        let mut coordinate_refusal = None;
        let locus_refusal = Cell::new(None);
        let candidate = (|| {
            Some({
                let unique_relation_id = solver.is_unique(relation.relation_id);
                let dimension = match definition
                    .dimensions
                    .as_ref()
                    .zip(usize::try_from(relation.dimension_id).ok())
                {
                    Some((dimensions, ordinal)) => capture_constraint_refusal(
                        &mut coordinate_refusal,
                        resolved_feature_dimension_parameter_admitted(
                            ctx, sketch, dimensions, ordinal,
                        ),
                    )?,
                    None => None,
                };
                let parameter = capture_constraint_refusal(
                    &mut coordinate_refusal,
                    dimension
                        .as_ref()
                        .map(|(_, parameter)| {
                            parameter
                                .try_clone_for_decode(ctx, "creo section dimension parameter copy")
                        })
                        .transpose(),
                )?;
                let joined_incidence_link = if unique_relation_id { solver.joined(relation.relation_id) } else { None };
                let joined_incidence = joined_incidence_link.map(|(_, incidence)| incidence);
                let typed = (|| {
                    unique_relation_id.then_some(())?;
                    let (dimension, _) = dimension.as_ref()?;
                    let parameter = capture_constraint_refusal(
                        &mut coordinate_refusal,
                        parameter
                            .as_ref()?
                            .try_clone_for_decode(ctx, "creo typed dimension parameter copy"),
                    )?;
                    if relation.relation_type == 1
                        && dimension.unit() == crate::feature::definitions::DimensionUnit::Radians
                    {
                        let [first, second] = capture_constraint_refusal(
                            &mut coordinate_refusal,
                            section_angular_entities(
                                ctx,
                                definition,
                                sketch,
                                relation.operand_vectors?,
                                &known_entities,
                            ),
                        )??;
                        return Some(SketchConstraintDefinitionInput::Angle {
                            first,
                            second,
                            parameter,
                        });
                    }
                    if relation.relation_type == 0
                        && matches!(relation.sign, 0 | 1 | 0xf6)
                        && dimension.unit()
                            == crate::feature::definitions::DimensionUnit::SchemaDefined
                        && dimension.value.resolved() == Some(0.0)
                    {
                        let vectors = relation.operand_vectors?;
                        if section_linear_distance_vectors(vectors) {
                            let [Some(first_id), Some(second_id), _, _] = vectors[0] else {
                                return None;
                            };
                            let incidence = joined_incidence?;
                            let [item] = incidence.items.as_slice() else {
                                return None;
                            };
                            if !section_skamp_active(incidence.status) {
                                return None;
                            }
                            let expected_coordinate = match incidence.kind {
                                1 => 1,
                                2 => 0,
                                _ => return None,
                            };
                            if item.sense != 0 {
                                return None;
                            }
                            let measured =
                                unique_decoded_section_segment(definition, item.entity_id)?;
                            if matches!(
                                measured.kind,
                                crate::feature::definitions::FeatureSegmentKind::Line(_)
                            ) && (measured.point_ids() == [first_id, second_id]
                                || measured.point_ids() == [second_id, first_id])
                                && measured.vertical_horizontal == Some(expected_coordinate)
                                && capture_constraint_refusal(&mut coordinate_refusal, ctx.contains_btree_set(&known_entities, &measured.external_id, "creo dimension known entity membership"))?
                            {
                                let entity = capture_constraint_refusal(
                                    &mut coordinate_refusal,
                                    sketch_entity_id_admitted(ctx, sketch, measured.external_id),
                                )??;
                                return Some(if incidence.kind == 1 {
                                    SketchConstraintDefinitionInput::Horizontal { entity }
                                } else {
                                    SketchConstraintDefinitionInput::Vertical { entity }
                                });
                            }
                        }
                    }
                    if dimension.unit() != crate::feature::definitions::DimensionUnit::Millimeters {
                        return None;
                    }
                    if matches!(relation.relation_type, 5 | 6) && relation.sign == 1 {
                        let segment = capture_constraint_refusal(
                            &mut coordinate_refusal,
                            section_radius_relation_arc(ctx, definition, relation),
                        )??;
                        return Some(circular_dimension_constraint(
                            capture_constraint_refusal(
                                &mut coordinate_refusal,
                                sketch_entity_id_admitted(ctx, sketch, segment.external_id),
                            )??,
                            parameter,
                            dimension.dimension_type,
                        ));
                    }
                    if relation.relation_type == 14
                        && relation.sign == 1
                        && matches!(dimension.dimension_type, 1..=5)
                        && relation.operand_vectors?[1] == [Some(0); 4]
                        && relation.operand_vectors?[2] == [Some(15), Some(0), Some(0), Some(0)]
                    {
                        let vectors = relation.operand_vectors?;
                        let [Some(radius_id), Some(0), Some(0), Some(0)] = vectors[0] else {
                            return None;
                        };
                        let external_id = circles_by_radius.get(&radius_id).copied().flatten()?;
                        capture_constraint_refusal(&mut coordinate_refusal, ctx.contains_btree_set(&known_entities, &external_id, "creo dimension known entity membership"))?.then_some(())?;
                        return Some(circular_dimension_constraint(
                            capture_constraint_refusal(
                                &mut coordinate_refusal,
                                sketch_entity_id_admitted(ctx, sketch, external_id),
                            )??,
                            parameter,
                            dimension.dimension_type,
                        ));
                    }
                    if relation.relation_type != 0 || !matches!(relation.sign, 0 | 1 | 0xf6) {
                        return None;
                    }
                    if let Some(vectors) = relation.operand_vectors {
                        if section_linear_distance_vectors(vectors) {
                            if let [Some(first_id), Some(second_id), _, _] = vectors[0] {
                                let coordinate = match section_linear_distance_coordinate(
                                    ctx,
                                    definition,
                                    &segments,
                                    [first_id, second_id],
                                    &resolved_coordinates,
                                    &saved_coordinate_witnesses,
                                    &ambiguous_point_ids,
                                ) {
                                    Ok(coordinate) => coordinate,
                                    Err(error) => {
                                        coordinate_refusal = Some(error);
                                        return None;
                                    }
                                };
                                let measured = measured_by_points.get(&point_pair([first_id, second_id])).copied().flatten();
                                if let Some(measured) = measured {
                                    if matches!(
                                        measured.kind,
                                        crate::feature::definitions::FeatureSegmentKind::Line(_)
                                    ) && capture_constraint_refusal(&mut coordinate_refusal, ctx.contains_btree_set(&known_entities, &measured.external_id, "creo dimension known entity membership"))?
                                    {
                                        let entity = capture_constraint_refusal(
                                            &mut coordinate_refusal,
                                            sketch_entity_id_admitted(
                                                ctx,
                                                sketch,
                                                measured.external_id,
                                            ),
                                        )??;
                                        let [first, second] =
                                            if measured.point_ids() == [first_id, second_id] {
                                                [
                                                    SketchLocus::Start(capture_constraint_refusal(
                                                        &mut coordinate_refusal,
                                                        entity.try_clone_for_decode(
                                                            ctx,
                                                            "creo dimension locus entity copy",
                                                        ),
                                                    )?),
                                                    SketchLocus::End(entity),
                                                ]
                                            } else {
                                                [
                                                    SketchLocus::End(capture_constraint_refusal(
                                                        &mut coordinate_refusal,
                                                        entity.try_clone_for_decode(
                                                            ctx,
                                                            "creo dimension locus entity copy",
                                                        ),
                                                    )?),
                                                    SketchLocus::Start(entity),
                                                ]
                                            };
                                        if let Some(coordinate) = coordinate {
                                            return Some(match coordinate {
                                                SectionAxis::U => {
                                                    SketchConstraintDefinitionInput::HorizontalDistance {
                                                        first,
                                                        second,
                                                        parameter,
                                                    }
                                                }
                                                SectionAxis::V => {
                                                    SketchConstraintDefinitionInput::VerticalDistance {
                                                        first,
                                                        second,
                                                        parameter,
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                                if let (Some(coordinate), Some(first), Some(second)) = (
                                    coordinate,
                                    capture_constraint_refusal(
                                        &mut coordinate_refusal,
                                        section_point_locus(ctx, definition, sketch, first_id),
                                    )?,
                                    capture_constraint_refusal(
                                        &mut coordinate_refusal,
                                        section_point_locus(ctx, definition, sketch, second_id),
                                    )?,
                                ) {
                                    return Some(match coordinate {
                                        SectionAxis::U => {
                                            SketchConstraintDefinitionInput::HorizontalDistance {
                                                first,
                                                second,
                                                parameter,
                                            }
                                        }
                                        SectionAxis::V => {
                                            SketchConstraintDefinitionInput::VerticalDistance {
                                                first,
                                                second,
                                                parameter,
                                            }
                                        }
                                    });
                                }
                            }
                        }
                    }
                    if let Some(Some([first, second])) = capture_locus_refusal(
                        &locus_refusal,
                        relation_incidence_loci(
                            ctx,
                            &locus_refusal,
                            definition,
                            sketch,
                            joined_incidence,
                        ),
                    ) {
                        return Some(SketchConstraintDefinitionInput::DistanceLoci {
                            first,
                            second,
                            parameter,
                        });
                    }
                    if let Some(incidence) =
                        joined_incidence.filter(|incidence| !section_skamp_active(incidence.status))
                    {
                        if let [first_item, second_item] = incidence.items.as_slice() {
                            let first_result = section_skamp_locus(
                                ctx,
                                &locus_refusal,
                                definition,
                                sketch,
                                first_item,
                            );
                            let first = capture_locus_refusal(&locus_refusal, first_result);
                            let second_result = section_skamp_locus(
                                ctx,
                                &locus_refusal,
                                definition,
                                sketch,
                                second_item,
                            );
                            let second = capture_locus_refusal(&locus_refusal, second_result);
                            if let (Some(Some(first)), Some(Some(second))) = (first, second) {
                                return Some(SketchConstraintDefinitionInput::DistanceLoci {
                                    first,
                                    second,
                                    parameter,
                                });
                            }
                        }
                        if !incidence.items.is_empty() {
                            return Some(SketchConstraintDefinitionInput::Distance {
                                entities: capture_constraint_refusal(
                                    &mut coordinate_refusal,
                                    incidence_entities(ctx, sketch, joined_incidence, "creo joined relation incidence items"),
                                )?,
                                parameter,
                            });
                        }
                    }
                    let entities = capture_constraint_refusal(
                        &mut coordinate_refusal,
                        incidence_entities(ctx, sketch, joined_incidence.filter(|incidence| section_skamp_active(incidence.status)), "creo relation incidence items"),
                    )?;
                    (!entities.is_empty()).then_some(SketchConstraintDefinitionInput::Distance {
                        entities,
                        parameter,
                    })
                })();
                let active =
                    joined_incidence.map(|incidence| section_skamp_active(incidence.status));
                if coordinate_refusal.is_some() {
                    return None;
                }
                let constraint_definition = match typed {
                    Some(typed) => typed,
                    None => capture_constraint_refusal(
                        &mut coordinate_refusal,
                        native_section_dimension_constraint_definition(
                            ctx, sketch, relation, solver,
                        ),
                    )??,
                };
                (
                    SketchConstraint {
                        id: if unique_relation_id {
                            capture_constraint_refusal(
                                &mut coordinate_refusal,
                                sketch_constraint_id_admitted(
                                    ctx,
                                    sketch,
                                    format_args!("relation:{}", relation.relation_id),
                                ),
                            )??
                        } else {
                            capture_constraint_refusal(
                                &mut coordinate_refusal,
                                sketch_constraint_id_admitted(
                                    ctx,
                                    sketch,
                                    format_args!("relation:offset:{}", relation.offset),
                                ),
                            )??
                        },
                        sketch: capture_constraint_refusal(
                            &mut coordinate_refusal,
                            sketch.try_clone_for_decode(
                                ctx,
                                "creo section dimension sketch identity",
                            ),
                        )?,
                        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                            constraint_definition,
                        )
                        .ok()?,
                        name: None,
                        driving: None,
                        active,
                        virtual_space: None,
                        visible: None,
                        orientation: None,
                        label_distance: None,
                        label_position: None,
                        metadata: None,
                        native_ref: Some(capture_constraint_refusal(
                            &mut coordinate_refusal,
                            sketch_native_ref_admitted(ctx, sketch),
                        )?),
                    },
                    relation.offset,
                    relation_index,
                )
            })
        })();
        if let Some(error) = locus_refusal.into_inner() {
            return Err(error);
        }
        if let Some(error) = coordinate_refusal {
            return Err(error);
        }
        if let Some(candidate) = candidate {
            ctx.reserve_vec(&mut constraints, 1, "creo section dimension constraints")?;
            constraints.push(candidate);
        }
    }
    Ok(constraints)
}

pub(in super::super) fn section_linear_distance_vectors(vectors: [[Option<u32>; 4]; 3]) -> bool {
    vectors[0][2..] == [None, Some(1)]
        && matches!(
            vectors[1],
            [Some(0), Some(0), Some(0), Some(0)] | [Some(1), Some(1), Some(0), Some(1)]
        )
        && vectors[2] == [Some(15), Some(16), Some(15), Some(1)]
}

#[cfg(test)]
mod tests {
    mod retain_mut;
    mod retain_vec;
    mod set_owner_tests;

    use super::{
        close_sketch_constraint_parameter_references, insert_native_equation_property,
        insert_relation_property, native_equation_data, push_relation_operand,
        reconcile_section_dimension_constraint,
        section_equation_function_five_scalar_equality_constraints,
        section_equation_function_sixteen_angle_difference_constraints,
    };
    use cadmpeg_ir::features::ParameterId;
    use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntityId, SketchId};
    use std::collections::BTreeSet;

    #[test]
    fn native_relation_identity_scan_refuses_before_counting_rows() {
        let relation = crate::feature::definitions::FeatureRelation {
            relation_id: 42,
            used: 7,
            operands: Vec::new(),
            operand_vectors: None,
            sign: 0,
            dimension_id: 0,
            relation_type: 17,
            body: Vec::new(),
            offset: 0,
        };
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: Some(crate::feature::definitions::FeatureRelationTable {
                declared_count: 3,
                entity_ref: None,
                rows: vec![relation.clone()],
                skamps: None,
                triples: None,
                offset: 0,
            }),
            saved_section: None,
            offset: 0,
        };
        let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch identity");
        let result = crate::test_support::assert_work_boundaries(
            &["creo solver relation identity rows"],
            |ctx| {
                super::native_section_dimension_constraint_definition(
                    ctx,
                    &sketch,
                    &relation, &super::RelationIncidences::new(ctx, &definition)?)
                .map(|candidate| candidate.expect("one relation produces a native constraint"))
            },
        );
        let SketchConstraintDefinitionInput::Native {
            native_state,
            operands,
            ..
        } = result
        else {
            panic!("unique relation remains a native constraint");
        };
        assert_eq!(native_state, Some(7));
        assert_eq!(operands.len(), 1);
        assert_eq!(operands[0].object_index, Some(42));
    }

    #[test]
    fn equation_constraint_refuses_each_retained_identity_and_output_row() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let sketch = SketchId::mint("creo:model:sketch#5").expect("valid sketch ID");
        let definition = || SketchConstraintDefinitionInput::ScalarEquality {
            first: 10,
            second: 11,
        };
        let id = "creo:featdefs:sketch_constraint#5:equation:1";
        let native_ref = "creo:featdefs:sketch#5";
        let mut total = 0u64;
        for (text, operation) in [
            (id, "creo sketch constraint identity"),
            (sketch.as_str(), "creo equation sketch identity"),
            (native_ref, "creo sketch native reference"),
        ] {
            total += cadmpeg_core::decode::u64_from_index(text.len());
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                Some(operation),
                |cap| {
                    let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                    let mut trial_policy = policy;
                    trial_policy.limits.max_retained_bytes = cap;
                    let (trial_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                        &[],
                        &trial_arena,
                        &trial_policy,
                    )
                    .expect("root");
                    super::equation_constraint(&trial_ctx, &sketch, 1, definition(), true, 7)
                },
            );
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let error = super::equation_constraint(&ctx, &sketch, 1, definition(), true, 7)
                .expect_err("one retained identity exceeds cap");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::RetainedBytes
                    && resource.operation == operation),
                "{error}"
            );
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let source = [std::cell::Cell::new(Some(super::equation_constraint(
            &ctx,
            &sketch,
            1,
            definition(),
            true,
            7,
        )))];
        let error = super::collect_constraint_candidates(
            &ctx,
            &source,
            "creo scalar equality constraints",
            |constraint| constraint.take().expect("fixture constraint consumed once"),
        )
        .expect_err("one output row exceeds zero items");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo scalar equality constraints")
        );
        policy.limits.max_collection_items = 1;
        policy.limits.max_retained_bytes = total
            + cadmpeg_core::decode::u64_from_index(
                4 * std::mem::size_of::<(cadmpeg_ir::sketches::SketchConstraint, usize)>(),
            );
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let source = [std::cell::Cell::new(Some(super::equation_constraint(
            &ctx,
            &sketch,
            1,
            definition(),
            true,
            7,
        )))];
        let rows = super::collect_constraint_candidates(
            &ctx,
            &source,
            "creo scalar equality constraints",
            |constraint| constraint.take().expect("fixture constraint consumed once"),
        )
        .expect("exact caps admit one equation");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.id.as_str(), id);
        assert_eq!(rows[0].1, 7);
    }

    #[test]
    fn native_verhor_refuses_each_nested_text_and_collection() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let sketch = SketchId::mint("creo:model:sketch#5").expect("valid sketch ID");
        let entity =
            SketchEntityId::mint("creo:featdefs:sketch_entity#5:42").expect("valid entity ID");
        let fields = [
            ("creo:segtab:verhor", "creo verhor native kind"),
            ("verhor", "creo verhor property key"),
            ("2", "creo verhor property value"),
            ("segtab_ptr", "creo verhor operand kind"),
            ("ext_id", "creo verhor operand field"),
            ("creo:featdefs:sketch#5", "creo sketch native reference"),
        ];
        let mut total = 0u64;
        for (field_index, (field, operation)) in fields.into_iter().enumerate() {
            if field_index == 3 {
                total += cadmpeg_core::decode::u64_from_index(
                    11 * std::mem::size_of::<(String, String)>()
                        + 16 * std::mem::size_of::<usize>()
                        + 2 * std::mem::align_of::<(String, String)>()
                        + 4 * std::mem::size_of::<SketchEntityId>(),
                );
            }
            total += cadmpeg_core::decode::u64_from_index(field.len());
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = total - 1;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            assert!(
                matches!(super::native_section_segment_verhor_definition(&ctx, &sketch, entity.clone(), 42, 2),
                Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes
                        && refusal.operation == operation)
            );
        }
        for (limit, operation) in [
            (0, "creo verhor property nodes"),
            (1, "creo verhor entity references"),
            (2, "creo verhor operands"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            assert!(
                matches!(super::native_section_segment_verhor_definition(&ctx, &sketch, entity.clone(), 42, 2),
                Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == operation)
            );
        }
        let arena = DecodeArena::new();
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
        let admitted =
            super::native_section_segment_verhor_definition(&ctx, &sketch, entity.clone(), 42, 2)
                .expect("service verhor admission");
        let SketchConstraintDefinitionInput::Native {
            native_properties,
            entities,
            operands,
            ..
        } = admitted
        else {
            panic!("native verhor definition");
        };
        assert_eq!(native_properties["verhor"], "2");
        assert_eq!(entities, [entity]);
        assert_eq!(operands[0].object_index, Some(42));
    }

    #[test]
    fn native_segment_radius_refuses_each_nested_text_and_collection() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let sketch = SketchId::mint("creo:model:sketch#5").expect("valid sketch ID");
        let entity =
            SketchEntityId::mint("creo:featdefs:sketch_entity#5:42").expect("valid entity ID");
        let fields = [
            ("creo:segtab:radius", "creo radius native kind"),
            ("dimension_ordinal", "creo radius property key"),
            ("2", "creo radius property value"),
            ("segtab_ptr", "creo radius operand kind"),
            ("ext_id", "creo radius operand field"),
            ("creo:featdefs:sketch#5", "creo sketch native reference"),
            ("dimension_ordinal", "creo radius operand kind"),
            ("radius", "creo radius operand field"),
            ("creo:featdefs:sketch#5", "creo sketch native reference"),
        ];
        let mut total = 0u64;
        for (field_index, (field, operation)) in fields.into_iter().enumerate() {
            if field_index == 3 {
                total += cadmpeg_core::decode::u64_from_index(
                    11 * std::mem::size_of::<(String, String)>()
                        + 16 * std::mem::size_of::<usize>()
                        + 2 * std::mem::align_of::<(String, String)>()
                        + 4 * std::mem::size_of::<SketchEntityId>(),
                );
            }
            total += cadmpeg_core::decode::u64_from_index(field.len());
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = total - 1;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            assert!(
                matches!(super::native_section_segment_radius_definition(&ctx, &sketch, entity.clone(), 42, "radius", 2),
                Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes
                        && refusal.operation == operation)
            );
        }
        for (limit, operation) in [
            (0, "creo radius property nodes"),
            (1, "creo radius entity references"),
            (3, "creo radius operands"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            assert!(
                matches!(super::native_section_segment_radius_definition(&ctx, &sketch, entity.clone(), 42, "radius", 2),
                Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == operation)
            );
        }
        let arena = DecodeArena::new();
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
        let admitted = super::native_section_segment_radius_definition(
            &ctx,
            &sketch,
            entity.clone(),
            42,
            "radius",
            2,
        )
        .expect("service radius admission");
        let SketchConstraintDefinitionInput::Native {
            native_properties,
            entities,
            operands,
            ..
        } = admitted
        else {
            panic!("native radius definition");
        };
        assert_eq!(native_properties["dimension_ordinal"], "2");
        assert_eq!(entities, [entity]);
        assert_eq!(
            operands
                .iter()
                .map(|operand| operand.object_index)
                .collect::<Vec<_>>(),
            [Some(42), Some(2)]
        );
    }

    #[test]
    fn segment_radius_rows_refuse_at_each_output_vector() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let segment = crate::feature::definitions::FeatureSegment {
            kind: crate::feature::definitions::FeatureSegmentKind::Line([7, 9]),
            directions: [None; 3],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: Some(2),
            radius2_ref: None,
            external_id: 42,
            body: Vec::new(),
            offset: 40,
        };
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(5),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 1,
                has_elided_prototype: false,
                entity_ref: None,
                rows: [crate::feature::segment_rows::SegmentRow::Ordinary(segment)]
                    .into_iter()
                    .collect(),
                offset: 38,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };
        let sketch = SketchId::mint("creo:model:sketch#5").expect("valid sketch ID");
        let rows = crate::test_support::assert_refusal_order(ResourceDimension::CollectionItems,
            &["creo segment radius constraints"], |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
                super::section_segment_radius_constraints(&ctx, &definition, &sketch)
            });
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].0.id.as_str(),
            "creo:featdefs:sketch_constraint#5:segtab-radius:42"
        );
        let emitted = BTreeSet::from([
            SketchEntityId::mint("creo:featdefs:sketch_entity#5:42").expect("valid entity ID")
        ]);
        let rows = crate::test_support::assert_refusal_order(ResourceDimension::CollectionItems,
            &["creo emitted segment radius constraints"], |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
                super::section_segment_radius_constraints_for_emitted(&ctx, &definition, &sketch, &emitted, &BTreeSet::new())
            });
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn native_relation_property_refuses_value_key_and_tree_node() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let mut properties = std::collections::BTreeMap::new();
        for (limit, operation) in [
            (0, "creo native relation property value"),
            (
                cadmpeg_core::decode::u64_from_index("7".len())
                    + cadmpeg_core::decode::u64_from_index("dimension_id".len())
                    - 1,
                "creo native relation property key",
            ),
        ] {
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            assert!(
                matches!(insert_relation_property(&ctx, &mut properties, "dimension_id", 7),
                Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes && refusal.operation == operation)
            );
            assert!(properties.is_empty());
        }
        policy.limits.max_retained_bytes = DecodePolicy::service().limits.max_retained_bytes;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(
            matches!(insert_relation_property(&ctx, &mut properties, "dimension_id", 7),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "creo native relation property nodes")
        );
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        insert_relation_property(&ctx, &mut properties, "dimension_id", 7)
            .expect("exact cap admits node");
        assert_eq!(properties["dimension_id"], "7");
    }

    #[test]
    fn native_relation_operand_refuses_kind_reference_and_vector_slot() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let native_ref = "creo:featdefs:sketch#5";
        for (limit, operation) in [
            (
                cadmpeg_core::decode::u64_from_index("relat_ptr".len()) - 1,
                "creo native relation operand kind",
            ),
            (
                cadmpeg_core::decode::u64_from_index("relat_ptr".len())
                    + cadmpeg_core::decode::u64_from_index(native_ref.len())
                    - 1,
                "creo native relation operand reference",
            ),
        ] {
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            assert!(
                matches!(push_relation_operand(&ctx, &mut Vec::new(), native_ref, "relat_ptr", None, 7),
                Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes && refusal.operation == operation)
            );
        }
        policy.limits.max_retained_bytes = DecodePolicy::service().limits.max_retained_bytes;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(
            matches!(push_relation_operand(&ctx, &mut Vec::new(), native_ref, "relat_ptr", None, 7),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "creo native relation operands")
        );
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut operands = Vec::new();
        push_relation_operand(&ctx, &mut operands, native_ref, "relat_ptr", None, 7)
            .expect("exact cap admits operand");
        assert_eq!(operands[0].object_index, Some(7));
        assert_eq!(operands[0].native_ref.as_deref(), Some(native_ref));
    }

    #[test]
    fn native_equation_properties_refuse_node_key_and_value() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut properties = std::collections::BTreeMap::new();
        let error = insert_native_equation_property(&ctx, &mut properties, "equation_id", 7)
            .expect_err("one property exceeds zero nodes");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native equation property nodes")
        );
        assert!(properties.is_empty());
        policy.limits.max_collection_items = DecodePolicy::service().limits.max_collection_items;
        policy.limits.max_retained_bytes =
            cadmpeg_core::decode::u64_from_index("equation_id".len()) - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error = insert_native_equation_property(&ctx, &mut properties, "equation_id", 7)
            .expect_err("property key exceeds retained cap");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native equation property keys")
        );
        policy.limits.max_retained_bytes =
            cadmpeg_core::decode::u64_from_index("equation_id".len());
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error = insert_native_equation_property(&ctx, &mut properties, "equation_id", 7)
            .expect_err("property value exceeds remaining retained cap");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native equation property values")
        );
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
        insert_native_equation_property(&ctx, &mut properties, "equation_id", 7)
            .expect("service property");
        assert_eq!(properties.get("equation_id").map(String::as_str), Some("7"));
    }

    #[test]
    fn native_equation_operands_refuse_each_nested_boundary() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let arena = DecodeArena::new();
        let arguments = [None, Some(2), Some(3)];
        for cap in 0..3 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let error = native_equation_data(&ctx, 1, &arguments, "creo:featdefs:sketch#40")
                .expect_err("next operand exceeds collection cap");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == "creo native equation operands")
            );
        }
        for (_cap, operation) in [
            (0, "creo equation operand kind"),
            (
                cadmpeg_core::decode::u64_from_index("eqtn_arr".len()),
                "creo equation operand field",
            ),
            (
                cadmpeg_core::decode::u64_from_index("eqtn_arr".len() + "equation_id".len()),
                "creo equation operand reference",
            ),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                Some(operation),
                |cap| {
                    let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                    let mut trial_policy = policy;
                    trial_policy.limits.max_retained_bytes = cap;
                    let (trial_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                        &[],
                        &trial_arena,
                        &trial_policy,
                    )
                    .expect("root");
                    native_equation_data(&trial_ctx, 1, &arguments, "creo:featdefs:sketch#40")
                },
            );
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let error = native_equation_data(&ctx, 1, &arguments, "creo:featdefs:sketch#40")
                .expect_err("next operand string exceeds retained cap");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::RetainedBytes
                    && resource.operation == operation)
            );
        }
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
        let operands = native_equation_data(&ctx, 1, &arguments, "creo:featdefs:sketch#40")
            .expect("service operands");
        assert_eq!(operands.argument_slots, "0:null,1:2,2:3");
        assert_eq!(operands.null_argument_ordinals.as_deref(), Some("0"));
        let operands = operands.operands;
        assert_eq!(operands.len(), 3);
        assert_eq!(operands[0].object_index, Some(1));
        assert_eq!(
            operands[1].field.as_ref().map(|field| field.name.as_str()),
            Some("arguments[1]")
        );
        assert_eq!(operands[2].object_index, Some(3));
    }

    #[test]
    fn emitted_parameter_ids_refuse_node_and_identity_copy() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let id = ParameterId::mint("creo:featdefs:parameter#1").expect("parameter ID");
        let mut document = cadmpeg_ir::document::CadIr::empty();
        document
            .model
            .parameters
            .push(cadmpeg_ir::features::DesignParameter {
                id: id.clone(),
                owner: None,
                ordinal: 0,
                name: "x".into(),
                expression: "x".into(),
                display: None,
                value: None,
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                properties: std::collections::BTreeMap::new(),
                pmi: None,
                native_ref: None,
            });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error = close_sketch_constraint_parameter_references(&ctx, &mut document)
            .expect_err("one parameter node exceeds zero items");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo emitted parameter ID nodes")
        );
        let error = crate::test_support::last_refusal_at(&[], ResourceDimension::MaterializedBytes,
            "creo emitted parameter identity", |ctx| {
                close_sketch_constraint_parameter_references(ctx, &mut document.clone())
            });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo emitted parameter identity"));
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
        close_sketch_constraint_parameter_references(&ctx, &mut document)
            .expect("service parameter closure");
        assert_eq!(document.model.parameters[0].id, id);
    }

    #[test]
    fn segment_radius_bindings_and_constraints_refuse_before_vector_growth() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::definitions::FeatureSegmentTable {
                declared_count: 1,
                has_elided_prototype: false,
                entity_ref: None,
                rows: [crate::feature::segment_rows::SegmentRow::Ordinary(
                    crate::feature::definitions::FeatureSegment {
                        kind: crate::feature::definitions::FeatureSegmentKind::Line([1, 2]),
                        directions: [None; 3],
                        center_id: None,
                        arc_orientation: None,
                        vertical_horizontal: None,
                        radius_ref: Some(0),
                        radius2_ref: None,
                        external_id: 7,
                        body: Vec::new(),
                        offset: 0,
                    },
                )]
                .into_iter()
                .collect(),
                offset: 0,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };
        let sketch = SketchId::mint("creo:model:sketch#1").expect("valid sketch identity");
        crate::test_support::assert_refusal_order(cadmpeg_core::decode::ResourceDimension::CollectionItems,
            &["creo segment radius bindings", "creo segment radius constraints"], |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
                super::section_segment_radius_constraints(&ctx, &definition, &sketch)
            });
        crate::test_support::assert_refusal_order(cadmpeg_core::decode::ResourceDimension::CollectionItems,
            &["creo emitted segment radius constraints"], |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
                super::section_segment_radius_constraints_for_emitted(&ctx, &definition, &sketch, &BTreeSet::new(), &BTreeSet::new())
            });
        crate::decode::with_test_decode_ctx(|ctx| {
            assert_eq!(
                super::section_segment_radius_constraints(ctx, &definition, &sketch)
                    .expect("service radius constraint")
                    .len(),
                1
            );
            assert_eq!(
                super::section_segment_radius_constraints_for_emitted(
                    ctx,
                    &definition,
                    &sketch,
                    &BTreeSet::new(),
                    &BTreeSet::new(),
                )
                .expect("service emitted radius constraint")
                .len(),
                1
            );
        });
    }

    #[test]
    fn direct_angle_difference_transfers_solver_scalar_operands() {
        let row = |variable_type, key, value: Option<f64>| {
            crate::feature::definitions::FeatureVariableRow {
                variable_type: crate::feature::definitions::VariableType::from(variable_type),
                key,
                value: value.map_or(
                    crate::feature::definitions::ScalarLane::DimensionDriven,
                    crate::feature::definitions::ScalarLane::Value,
                ),
                value_body: Vec::new(),
                guess: value.map_or(
                    crate::feature::definitions::ScalarLane::DimensionDriven,
                    crate::feature::definitions::ScalarLane::Value,
                ),
                guess_body: Vec::new(),

                known: Some(0),
                homogeneity: Some(1),
                uvar_id: None,

                offset: 0,
            }
        };
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(40),
                owner_feature_id: None,
            },
            body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                    \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                    \x01\x10\xf8\x04\x00\x01\x02\x03\xf6\xe2"
                .to_vec(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(crate::feature::definitions::FeatureVariableTable {
                declared_count: 4,
                entity_ref: None,
                rows: vec![
                    row(4, 10, Some(2.5)),
                    row(4, 11, Some(1.0)),
                    row(0, 20, Some(1.5)),
                    row(5, 0, Some(0.0)),
                ],
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };
        let sketch =
            SketchId::mint("creo:model:sketch#angle-difference").expect("valid test fixture");
        let constraints = crate::decode::with_test_decode_ctx(|ctx| {
            section_equation_function_sixteen_angle_difference_constraints(
                ctx,
                &definition,
                &sketch,
            )
        })
        .expect("section_equation_function_sixteen_angle_difference_constraints admitted");
        assert_eq!(constraints.len(), 1);
        assert_eq!(constraints[0].1, 28);
        assert_eq!(constraints[0].0.active, Some(true));
        assert_eq!(
            *(constraints[0].0.definition).kind(),
            SketchConstraintDefinitionInput::AngleDifference {
                first: 10,
                second: 11,
                difference: 20,
                value: cadmpeg_ir::scalar::Angle::new(1.5).expect("finite angle fixture"),
            }
        );
    }

    #[test]
    fn direct_scalar_equality_transfers_solver_scalar_operands() {
        let row = |variable_type, key, value: Option<f64>| {
            crate::feature::definitions::FeatureVariableRow {
                variable_type: crate::feature::definitions::VariableType::from(variable_type),
                key,
                value: value.map_or(
                    crate::feature::definitions::ScalarLane::DimensionDriven,
                    crate::feature::definitions::ScalarLane::Value,
                ),
                value_body: Vec::new(),
                guess: value.map_or(
                    crate::feature::definitions::ScalarLane::DimensionDriven,
                    crate::feature::definitions::ScalarLane::Value,
                ),
                guess_body: Vec::new(),

                known: Some(0),
                homogeneity: Some(1),
                uvar_id: None,

                offset: 0,
            }
        };
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(40),
                owner_feature_id: None,
            },
            body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                    \xe0\x01id\0\0\xf1\xf7\x80\x9f\xe2\
                    \x01\x05\xf8\x03\x00\x01\x02\xf6\xe2"
                .to_vec(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(crate::feature::definitions::FeatureVariableTable {
                declared_count: 3,
                entity_ref: None,
                rows: vec![
                    row(6, 10, Some(2.5)),
                    row(6, 11, Some(2.5)),
                    row(5, 20, Some(0.0)),
                ],
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };
        let sketch =
            SketchId::mint("creo:model:sketch#scalar-equality").expect("valid test fixture");
        let constraints = crate::decode::with_test_decode_ctx(|ctx| {
            section_equation_function_five_scalar_equality_constraints(ctx, &definition, &sketch)
        })
        .expect("section_equation_function_five_scalar_equality_constraints admitted");
        assert_eq!(constraints.len(), 1);
        assert_eq!(constraints[0].0.active, Some(true));
        assert_eq!(
            *(constraints[0].0.definition).kind(),
            SketchConstraintDefinitionInput::ScalarEquality {
                first: 10,
                second: 11,
            }
        );

        let mut conflicting = definition.clone();
        conflicting.variables.as_mut().expect("variables").rows[1].value =
            crate::feature::definitions::ScalarLane::Value(3.5);
        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            section_equation_function_five_scalar_equality_constraints(ctx, &conflicting, &sketch)
        })
        .expect("section_equation_function_five_scalar_equality_constraints admitted")
        .is_empty());
    }

    #[test]
    fn typed_dimension_relation_falls_back_to_native_when_references_are_not_emitted() {
        let relation = crate::feature::definitions::FeatureRelation {
            relation_id: 7,
            used: 1,
            operands: Vec::new(),
            operand_vectors: None,
            sign: 1,
            dimension_id: 0,
            relation_type: 0,
            body: Vec::new(),
            offset: 11,
        };
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: Some(crate::feature::definitions::FeatureRelationTable {
                declared_count: 3,
                entity_ref: None,
                rows: vec![relation.clone()],
                skamps: None,
                triples: None,
                offset: 0,
            }),
            saved_section: None,
            offset: 0,
        };
        let sketch = SketchId::mint("synthetic:test:id#synthetic:test:dimension-relation")
            .expect("valid test fixture");
        let missing = SketchEntityId::mint("synthetic:test:dimension-relation#missing")
            .expect("valid test fixture");
        let mut constraint = SketchConstraintDefinitionInput::Distance {
            entities: vec![missing],
            parameter: ParameterId::mint("synthetic:test:id#synthetic:test:dimension-parameter")
                .expect("identity grammar"),
        };

        assert!(
            crate::decode::with_test_decode_ctx(|ctx| reconcile_section_dimension_constraint(
                ctx,
                &mut constraint,
                &sketch,
                &relation,
                &BTreeSet::new(),
                &BTreeSet::new(), &super::RelationIncidences::new(ctx, &definition)?))
            .expect("service dimension fallback admission")
        );
        assert!(matches!(
            constraint,
            SketchConstraintDefinitionInput::Native {
                native_kind,
                entities,
                ..
            } if native_kind.as_str() == "creo:relation:0" && entities.is_empty()
        ));

        let emitted_entity = SketchEntityId::mint("synthetic:test:dimension-relation#emitted")
            .expect("valid test fixture");
        let mut missing_parameter = SketchConstraintDefinitionInput::Distance {
            entities: vec![emitted_entity.clone()],
            parameter: ParameterId::mint("synthetic:test:id#synthetic:test:dimension-parameter")
                .expect("identity grammar"),
        };
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| reconcile_section_dimension_constraint(
                ctx,
                &mut missing_parameter,
                &sketch,
                &relation,
                &BTreeSet::from([emitted_entity]),
                &BTreeSet::new(), &super::RelationIncidences::new(ctx, &definition)?))
            .expect("service dimension fallback admission")
        );
        assert!(matches!(
            missing_parameter,
            SketchConstraintDefinitionInput::Native {
                native_kind,
                ..
            } if native_kind.as_str() == "creo:relation:0"
        ));
    }

    #[test]
    fn section_dimension_row_refuses_after_native_property_and_operand_nodes() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let relation = crate::feature::definitions::FeatureRelation {
            relation_id: 7,
            used: 1,
            operands: Vec::new(),
            operand_vectors: None,
            sign: 1,
            dimension_id: 0,
            relation_type: 0,
            body: Vec::new(),
            offset: 11,
        };
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: Some(crate::feature::definitions::FeatureRelationTable {
                declared_count: 3,
                entity_ref: None,
                rows: vec![relation],
                skamps: None,
                triples: None,
                offset: 0,
            }),
            saved_section: None,
            offset: 0,
        };
        let sketch = SketchId::mint("creo:model:sketch#5").expect("valid sketch ID");
        let admitted = crate::test_support::assert_refusal_order(
            ResourceDimension::CollectionItems,
            &["creo solver relation identity rows", "creo native relation operands",
              "creo section dimension constraints"],
            |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
                super::section_dimension_constraints(&ctx, &definition, &sketch)
            },
        );
        assert_eq!(admitted.len(), 1);
        assert_eq!(
            admitted[0].0.id.as_str(),
            "creo:featdefs:sketch_constraint#5:relation:7"
        );
    }
}

#[cfg(test)]
mod binding_tests {
    use cadmpeg_ir::sketches::SketchId;
    #[test]
    fn dimension_constraints_preserve_original_row_after_skipped_candidates() {
        let relation = crate::feature::definitions::FeatureRelation {
            relation_id: 7,
            used: 1,
            operands: Vec::new(),
            operand_vectors: None,
            sign: 1,
            dimension_id: 0,
            relation_type: 0,
            body: Vec::new(),
            offset: 11,
        };
        let mut definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: Some(crate::feature::definitions::FeatureRelationTable {
                declared_count: 5,
                entity_ref: None,
                rows: vec![relation],
                skamps: None,
                triples: None,
                offset: 0,
            }),
            saved_section: None,
            offset: 0,
        };
        let table = definition.relations.as_mut().expect("relations");
        let mut skipped = table.rows[0].clone();
        skipped.offset = 12;
        let mut retained = skipped.clone();
        retained.relation_id = 8;
        retained.offset = 13;
        table.rows.extend([skipped, retained]);
        let sketch = SketchId::mint("creo:model:sketch#5").expect("valid sketch ID");
        let constraints = crate::decode::with_test_decode_ctx(|ctx| {
            super::section_dimension_constraints(ctx, &definition, &sketch)
        })
        .expect("constraints");
        assert_eq!(constraints.len(), 1);
        assert_eq!(constraints[0].1, 13);
        assert_eq!(constraints[0].2, 2);
        assert_eq!(
            constraints[0].0.id.as_str(),
            "creo:featdefs:sketch_constraint#5:relation:8"
        );
    }
}
