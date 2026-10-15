// SPDX-License-Identifier: Apache-2.0
//! Section constraint reconciliation, incidence, and dimension emission.

use crate::decode::sketch::axis::SectionAxis;

use crate::decode::sketch::equations_scalar::SectionScalarVariable;
use crate::feature::definitions::SolverSubtable;

use super::super::feature_history::dimensions::{
    feature_relation_table_complete, resolved_feature_dimension_parameter,
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
    opaque_section_segment_identity_suffix, section_entity_external_ids,
    section_segment_identity_suffix, unique_section_segment_external_ids,
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
use std::collections::{BTreeMap, BTreeSet};

const EPS_POLAR_ZERO: f64 = 1.0e-12;

fn collect_constraint_candidates<I, T>(candidates: I) -> Result<Vec<T>, cadmpeg_core::CodecError>
where
    I: IntoIterator<Item = Result<Option<T>, cadmpeg_core::CodecError>>,
{
    let mut collected = Vec::new();
    for candidate in candidates {
        if let Some(candidate) = candidate? {
            collected.push(candidate);
        }
    }
    Ok(collected)
}

fn equation_constraint(
    sketch: &SketchId,
    equation_id: u32,
    definition: SketchConstraintDefinitionInput,
    active: bool,
    offset: usize,
) -> Option<(SketchConstraint, usize)> {
    let id = sketch_constraint_id_admitted(sketch, format_args!("equation:{equation_id}"))?;
    let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
    else {
        return None;
    };
    Some((
        SketchConstraint {
            id,
            sketch: sketch.clone(),
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
            native_ref: Some(sketch_native_ref_admitted(sketch)),
        },
        offset,
    ))
}

pub(in super::super) fn section_segment_verhor_definition(
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
        _ => native_section_segment_verhor_definition(sketch, entity, segment.external_id, verhor)?,
    }))
}

pub(super) fn native_section_segment_verhor_definition(
    sketch: &SketchId,
    entity: SketchEntityId,
    external_id: u32,
    verhor: u32,
) -> Result<SketchConstraintDefinitionInput, cadmpeg_core::CodecError> {
    let native_kind = "creo:segtab:verhor".to_owned();
    let native_kind = cadmpeg_core::text::NonBlankString::new(native_kind)
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("native kind must not be empty"))?;
    let key = "verhor".to_owned();
    let value = format!("{verhor}");
    let mut native_properties = BTreeMap::new();
    native_properties.insert(key, value);
    let entities = vec![entity];
    let operand_kind = "segtab_ptr".to_owned();
    let field = "ext_id".to_owned();
    let native_ref = sketch_native_ref_admitted(sketch);
    let mut operands = Vec::new();
    operands.push(SketchNativeOperand {
        native_kind: cadmpeg_core::text::NonBlankString::new(operand_kind)
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("operand kind must not be empty"))?,
        field: Some(NativeOperandField {
            name: cadmpeg_core::text::NonBlankString::new(field).ok_or_else(|| {
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
    definition: &mut SketchConstraintDefinitionInput,
    emitted: &BTreeSet<SketchEntityId>,
) -> bool {
    let locus_emitted = |locus: &SketchLocus| match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => emitted.contains(entity),
    };
    match definition {
        SketchConstraintDefinitionInput::Native { entities, .. } => {
            entities.retain(|entity| emitted.contains(entity));
            true
        }
        SketchConstraintDefinitionInput::Coincident { entities }
        | SketchConstraintDefinitionInput::Distance { entities, .. } => {
            entities.iter().all(|entity| emitted.contains(entity))
        }
        SketchConstraintDefinitionInput::CoincidentLoci { loci } => loci.iter().all(locus_emitted),
        SketchConstraintDefinitionInput::SameCoordinate { relation } => {
            locus_emitted(relation.first()) && locus_emitted(relation.second())
        }
        SketchConstraintDefinitionInput::TangentLoci { first, second }
        | SketchConstraintDefinitionInput::DistanceLoci { first, second, .. }
        | SketchConstraintDefinitionInput::DistanceLociValue { first, second, .. }
        | SketchConstraintDefinitionInput::MidpointCoordinate { first, second, .. }
        | SketchConstraintDefinitionInput::PolarDistance { first, second, .. }
        | SketchConstraintDefinitionInput::HorizontalDistance { first, second, .. }
        | SketchConstraintDefinitionInput::VerticalDistance { first, second, .. } => {
            locus_emitted(first) && locus_emitted(second)
        }
        SketchConstraintDefinitionInput::EqualDistance { first, second } => {
            locus_emitted(&first.first)
                && locus_emitted(&first.second)
                && locus_emitted(&second.first)
                && locus_emitted(&second.second)
        }
        SketchConstraintDefinitionInput::Midpoint { point, entity } => {
            locus_emitted(point) && emitted.contains(entity)
        }
        SketchConstraintDefinitionInput::PointCoordinateValues { point, .. } => {
            locus_emitted(point)
        }
        SketchConstraintDefinitionInput::AtIntersection {
            point,
            first,
            second,
        } => locus_emitted(point) && emitted.contains(first) && emitted.contains(second),
        SketchConstraintDefinitionInput::PointOnObject { point, entity } => {
            locus_emitted(point) && emitted.contains(entity)
        }
        SketchConstraintDefinitionInput::Symmetric {
            first,
            second,
            axis,
        } => locus_emitted(first) && locus_emitted(second) && emitted.contains(axis),
        SketchConstraintDefinitionInput::PointSymmetric {
            first,
            second,
            center,
        } => locus_emitted(first) && locus_emitted(second) && locus_emitted(center),
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
            emitted.contains(first) && emitted.contains(second)
        }
        SketchConstraintDefinitionInput::Horizontal { entity }
        | SketchConstraintDefinitionInput::Vertical { entity }
        | SketchConstraintDefinitionInput::Fixed { entity }
        | SketchConstraintDefinitionInput::Radius { entity, .. }
        | SketchConstraintDefinitionInput::Diameter { entity, .. } => emitted.contains(entity),
        SketchConstraintDefinitionInput::ArcAngle { entity, .. }
        | SketchConstraintDefinitionInput::EllipseAngle { entity, .. } => emitted.contains(entity),
        SketchConstraintDefinitionInput::SnellsLaw {
            incident,
            refracted,
            interface,
            ..
        } => locus_emitted(incident) && locus_emitted(refracted) && emitted.contains(interface),
        SketchConstraintDefinitionInput::Weight { entity, .. } => emitted.contains(entity),
        SketchConstraintDefinitionInput::InternalAlignment { helper, parent, .. } => {
            emitted.contains(helper) && emitted.contains(parent)
        }
        SketchConstraintDefinitionInput::Group { elements }
        | SketchConstraintDefinitionInput::Text { elements, .. } => {
            elements.iter().all(locus_emitted)
        }
        SketchConstraintDefinitionInput::Disabled {} => true,
        _ => true,
    }
}

pub(in super::super) fn reconcile_constraint_parameter_reference(
    definition: &mut SketchConstraintDefinitionInput,
    emitted: &BTreeSet<ParameterId>,
) -> bool {
    match definition {
        SketchConstraintDefinitionInput::Native { parameter, .. } => {
            if parameter
                .as_ref()
                .is_some_and(|parameter| !emitted.contains(parameter))
            {
                *parameter = None;
            }
            true
        }
        SketchConstraintDefinitionInput::PolarDistance {
            distance_parameter, ..
        } => {
            if distance_parameter
                .as_ref()
                .is_some_and(|parameter| !emitted.contains(parameter))
            {
                *distance_parameter = None;
            }
            true
        }
        SketchConstraintDefinitionInput::DistanceLociValue { parameter, .. } => {
            if parameter
                .as_ref()
                .is_some_and(|parameter| !emitted.contains(parameter))
            {
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
            emitted.contains(parameter)
        }
        SketchConstraintDefinitionInput::SnellsLaw { parameter, .. }
        | SketchConstraintDefinitionInput::Weight { parameter, .. } => emitted.contains(parameter),
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
    }
}

pub(in super::super) fn close_sketch_constraint_parameter_references(ir: &mut CadIr) {
    let mut emitted = BTreeSet::new();
    for parameter in &ir.model.parameters {
        if !emitted.contains(&parameter.id) {
            emitted.insert(parameter.id.clone());
        }
    }
    ir.model.sketch_constraints.retain_mut(|constraint| {
        constraint
            .definition
            .edit(|kind| reconcile_constraint_parameter_reference(kind, &emitted))
            .unwrap_or(false)
    });
}

pub(in super::super) fn joined_relation_incidence(
    definition: &crate::feature::definitions::FeatureDefinition,
    relation_id: u32,
) -> Option<&crate::feature::definitions::FeatureSkamp> {
    joined_relation_incidence_link(definition, relation_id).map(|(_, incidence)| incidence)
}

fn joined_relation_incidence_link(
    definition: &crate::feature::definitions::FeatureDefinition,
    relation_id: u32,
) -> Option<(
    &crate::feature::definitions::FeatureRelationTriple,
    &crate::feature::definitions::FeatureSkamp,
)> {
    let Some(relations) = &definition.relations else {
        return None;
    };
    if !relations
        .triples
        .as_ref()
        .is_none_or(SolverSubtable::is_complete)
        || !relations
            .skamps
            .as_ref()
            .is_none_or(SolverSubtable::is_complete)
    {
        return None;
    }
    let (join, incidence_id) = crate::decode::uniqueness::exactly_one(
        relations
            .triples()
            .iter()
            .filter(|triple| triple.relation_id == Some(relation_id))
            .filter_map(|triple| triple.skamp_id.map(|incidence_id| (triple, incidence_id))),
    )?;
    let incidence = crate::decode::uniqueness::exactly_one(
        relations
            .skamps()
            .iter()
            .filter(|skamp| skamp.id == incidence_id),
    )?;
    Some((join, incidence))
}

pub(in super::super) fn section_solver_relation_is_disabled(
    definition: &crate::feature::definitions::FeatureDefinition,
    relation_id: u32,
) -> bool {
    let Some(relations) = definition
        .relations
        .as_ref()
        .filter(|relations| feature_relation_table_complete(relations))
    else {
        return false;
    };
    if relations
        .rows
        .iter()
        .filter(|relation| relation.relation_id == relation_id)
        .count()
        != 1
    {
        return false;
    }
    joined_relation_incidence(definition, relation_id)
        .is_some_and(|incidence| !section_skamp_active(incidence.status))
}

pub(in super::super) fn section_solver_equation_is_disabled(
    definition: &crate::feature::definitions::FeatureDefinition,
    equation_id: u32,
) -> bool {
    let Some(relations) = &definition.relations else {
        return false;
    };
    if !relations
        .triples
        .as_ref()
        .is_none_or(SolverSubtable::is_complete)
        || !relations
            .skamps
            .as_ref()
            .is_none_or(SolverSubtable::is_complete)
    {
        return false;
    }
    let incidence_id = crate::decode::uniqueness::exactly_one(
        relations
            .triples()
            .iter()
            .filter(|triple| triple.equation_id == Some(equation_id))
            .filter_map(|triple| triple.skamp_id),
    );
    let Some(incidence_id) = incidence_id else {
        return false;
    };
    let incidence = crate::decode::uniqueness::exactly_one(
        relations
            .skamps()
            .iter()
            .filter(|skamp| skamp.id == incidence_id),
    );
    let Some(incidence) = incidence else {
        return false;
    };
    !section_skamp_active(incidence.status)
}

pub(in super::super) fn relation_incidence(
    definition: &crate::feature::definitions::FeatureDefinition,
    relation_id: u32,
) -> Option<&crate::feature::definitions::FeatureSkamp> {
    let incidence = joined_relation_incidence(definition, relation_id)?;
    section_skamp_active(incidence.status).then_some(incidence)
}

pub(in super::super) fn relation_incidence_entities(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    relation_id: u32,
) -> Vec<SketchEntityId> {
    let Some(incidence) = relation_incidence(definition, relation_id) else {
        return Vec::new();
    };
    let mut entities = Vec::new();
    for item in &incidence.items {
        if let Some(entity) = sketch_entity_id_admitted(sketch, item.entity_id) {
            entities.push(entity);
        }
    }
    entities
}

pub(in super::super) fn joined_relation_incidence_entities(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    relation_id: u32,
) -> Vec<SketchEntityId> {
    let Some(incidence) = joined_relation_incidence(definition, relation_id) else {
        return Vec::new();
    };
    let mut entities = Vec::new();
    for item in &incidence.items {
        if let Some(entity) = sketch_entity_id_admitted(sketch, item.entity_id) {
            entities.push(entity);
        }
    }
    entities
}

fn relation_incidence_loci(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    relation_id: u32,
) -> Option<[SketchLocus; 2]> {
    let incidence = relation_incidence(definition, relation_id)?;
    let [first, second] = incidence.items.as_slice() else {
        return None;
    };
    Some([
        section_skamp_locus(definition, sketch, first)?,
        section_skamp_locus(definition, sketch, second)?,
    ])
}

fn section_angular_entities(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    segments: &[&crate::feature::definitions::FeatureSegment],
    vectors: [[Option<u32>; 4]; 3],
    known_entities: &BTreeSet<u32>,
) -> Option<[SketchEntityId; 2]> {
    let [Some(first_internal), Some(second_internal), None, Some(1)] = vectors[0] else {
        return None;
    };
    let order_table = definition.order_table.as_ref()?;
    let external_id = |internal_id| {
        let external_id = order_table.external_id(internal_id)?;
        let matching_segments = segments
            .iter()
            .filter(|segment| {
                segment.external_id == external_id
                    && matches!(
                        segment.kind,
                        crate::feature::definitions::FeatureSegmentKind::Line(_)
                    )
            })
            .count();
        (known_entities.contains(&external_id) && matching_segments == 1).then_some(external_id)
    };
    let [first, second] = [first_internal, second_internal].map(external_id);
    let [Some(first), Some(second)] = [first, second] else {
        return None;
    };
    if first == second {
        return None;
    }
    let first = sketch_entity_id_admitted(sketch, first)?;
    let second = sketch_entity_id_admitted(sketch, second)?;
    Some([first, second])
}

fn segment_radius_operand(
    sketch: &SketchId,
    kind: &str,
    field: &str,
    object_index: u32,
) -> Result<SketchNativeOperand, cadmpeg_core::CodecError> {
    let kind = kind.to_owned();
    let field = field.to_owned();
    Ok(SketchNativeOperand {
        native_kind: cadmpeg_core::text::NonBlankString::new(kind).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("radius operand kind must not be empty")
        })?,
        field: Some(NativeOperandField {
            name: cadmpeg_core::text::NonBlankString::new(field).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("radius operand field must not be empty")
            })?,
            role: None,
        }),
        object_index: Some(object_index),
        native_ref: Some(sketch_native_ref_admitted(sketch)),
    })
}

fn native_section_segment_radius_definition(
    sketch: &SketchId,
    entity: SketchEntityId,
    external_id: u32,
    field: &str,
    dimension_ordinal: u32,
) -> Result<SketchConstraintDefinitionInput, cadmpeg_core::CodecError> {
    let native_kind = format!("creo:segtab:{field}");
    let native_kind = cadmpeg_core::text::NonBlankString::new(native_kind).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("radius native kind must not be empty")
    })?;
    let key = "dimension_ordinal".to_owned();
    let value = format!("{dimension_ordinal}");
    let mut native_properties = BTreeMap::new();
    native_properties.insert(key, value);
    let entities = vec![entity];
    let first = segment_radius_operand(sketch, "segtab_ptr", "ext_id", external_id)?;
    let second = segment_radius_operand(sketch, "dimension_ordinal", field, dimension_ordinal)?;
    let operands = vec![first, second];
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
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<SectionSegmentRadiusBinding>, cadmpeg_core::CodecError> {
    let unique_segment_ids = unique_section_segment_external_ids(definition);
    let mut bindings = Vec::new();
    let Some(segments) = definition.segments.as_ref() else {
        return Ok(bindings);
    };
    for segment in segments.rows.ordinary() {
        if segment.radius_ref.is_none() && segment.radius2_ref.is_none() {
            continue;
        }
        let suffix = section_segment_identity_suffix(&unique_segment_ids, segment);
        for (field, ordinal) in [
            (SegmentRadiusField::Primary, segment.radius_ref),
            (SegmentRadiusField::Secondary, segment.radius2_ref),
        ] {
            let Some(ordinal) = ordinal else {
                continue;
            };
            bindings.push(SectionSegmentRadiusBinding {
                suffix: suffix.clone(),
                external_id: segment.external_id,
                field,
                ordinal,
                offset: segment.offset,
                typed_circle: None,
            });
        }
    }
    for segment in segments.rows.circles() {
        let suffix = if unique_segment_ids.contains(&segment.external_id) {
            format!("{}", segment.external_id)
        } else {
            format!("circle:offset:{}", segment.offset)
        };
        let typed_circle = if unique_segment_ids.contains(&segment.external_id) {
            match (
                usize::try_from(segment.radius_ref).ok(),
                definition.dimensions.as_ref(),
            ) {
                (Some(ordinal), Some(dimensions)) => {
                    resolved_feature_dimension_parameter(sketch, dimensions, ordinal)?
                        .map(|(dimension, parameter)| (dimension.dimension_type, parameter))
                }
                _ => None,
            }
        } else {
            None
        };
        bindings.push(SectionSegmentRadiusBinding {
            suffix,
            external_id: segment.external_id,
            field: SegmentRadiusField::Primary,
            ordinal: segment.radius_ref,
            offset: segment.offset,
            typed_circle,
        });
    }
    for segment in segments.rows.opaque() {
        if segment.radius_ref.is_none() && segment.radius2_ref.is_none() {
            continue;
        }
        let suffix = opaque_section_segment_identity_suffix(&unique_segment_ids, segment);
        for (field, ordinal) in [
            (SegmentRadiusField::Primary, segment.radius_ref),
            (SegmentRadiusField::Secondary, segment.radius2_ref),
        ] {
            let Some(ordinal) = ordinal else {
                continue;
            };
            bindings.push(SectionSegmentRadiusBinding {
                suffix: suffix.clone(),
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
    binding: SectionSegmentRadiusBinding,
    sketch: &SketchId,
) -> Result<Option<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let Some(entity) = sketch_entity_id_admitted(sketch, &binding.suffix) else {
        return Ok(None);
    };
    let (definition, kind) = match binding.typed_circle {
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
    let Some(id) = sketch_constraint_id_admitted(sketch, format_args!("{kind}:{}", binding.suffix))
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
            sketch: sketch.clone(),
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
            native_ref: Some(sketch_native_ref_admitted(sketch)),
        },
        binding.offset,
    )))
}

pub(in super::super) fn section_segment_radius_constraints(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let mut constraints = Vec::new();
    for binding in section_segment_radius_bindings(definition, sketch)? {
        if let Some(constraint) = section_segment_radius_constraint(binding, sketch)? {
            constraints.push(constraint);
        }
    }
    Ok(constraints)
}

pub(in super::super) fn section_segment_radius_constraints_for_emitted(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    emitted: &BTreeSet<SketchEntityId>,
    available_parameters: &BTreeSet<ParameterId>,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let bindings = section_segment_radius_bindings(definition, sketch)?;
    let mut constraints = Vec::new();
    for ((mut constraint, offset), binding) in
        section_segment_radius_constraints(definition, sketch)?
            .into_iter()
            .zip(bindings)
    {
        let reconciled = match constraint.definition.edit(|kind| {
            reconcile_section_segment_radius_constraint(
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
            constraints.push((constraint, offset));
        }
    }
    Ok(constraints)
}

fn reconcile_section_segment_radius_constraint(
    constraint_definition: &mut SketchConstraintDefinitionInput,
    sketch: &SketchId,
    binding: &SectionSegmentRadiusBinding,
    emitted: &BTreeSet<SketchEntityId>,
    available_parameters: &BTreeSet<ParameterId>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let entity_reconciled = reconcile_constraint_entity_references(constraint_definition, emitted);
    let parameter_reconciled =
        reconcile_constraint_parameter_reference(constraint_definition, available_parameters);
    if entity_reconciled && parameter_reconciled {
        return Ok(true);
    }
    let Some(entity) = sketch_entity_id_admitted(sketch, &binding.suffix) else {
        return Ok(false);
    };
    let native_definition = native_section_segment_radius_definition(
        sketch,
        entity,
        binding.external_id,
        binding.field.key(),
        binding.ordinal,
    )?;
    *constraint_definition = native_definition;
    Ok(
        reconcile_constraint_entity_references(constraint_definition, emitted)
            && reconcile_constraint_parameter_reference(
                constraint_definition,
                available_parameters,
            ),
    )
}

pub(in super::super) fn section_equation_radius_dimension_constraints(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let Some(segments) = definition.segments.as_ref() else {
        return Ok(Vec::new());
    };
    let Some(dimensions) = definition.dimensions.as_ref() else {
        return Ok(Vec::new());
    };
    let unique_segment_ids = unique_section_segment_external_ids(definition);
    let mut entities_by_radius = BTreeMap::<u32, Vec<u32>>::new();
    for segment in segments.rows.ordinary().filter(|segment| {
        matches!(
            segment.kind,
            crate::feature::definitions::FeatureSegmentKind::Arc(_)
        ) && unique_segment_ids.contains(&segment.external_id)
    }) {
        if let Some(radius) = segment.radius_ref {
            let entities = entities_by_radius.entry(radius).or_default();
            entities.push(segment.external_id);
        }
    }
    for segment in segments
        .rows
        .circles()
        .filter(|segment| unique_segment_ids.contains(&segment.external_id))
    {
        let entities = entities_by_radius.entry(segment.radius_ref).or_default();
        entities.push(segment.external_id);
    }

    let mut constraints = Vec::new();
    for equation in section_equation_radius_dimensions(definition) {
        let Ok(ordinal) = usize::try_from(equation.scalar.1) else {
            continue;
        };
        let Some((dimension, parameter)) =
            resolved_feature_dimension_parameter(sketch, dimensions, ordinal)?
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
        let Some(entities) = entities_by_radius.get(&equation.radius) else {
            continue;
        };
        for &external_id in entities {
            let Some(entity) = sketch_entity_id_admitted(sketch, external_id) else {
                continue;
            };
            let Some(id) = sketch_constraint_id_admitted(
                sketch,
                format_args!("equation:{}:radius:{}", equation.equation_id, external_id),
            ) else {
                continue;
            };
            let parameter = parameter.clone();
            let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                SketchConstraintDefinitionInput::Radius { entity, parameter },
            ) else {
                continue;
            };
            constraints.push((
                SketchConstraint {
                    id,
                    sketch: sketch.clone(),
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
                    native_ref: Some(sketch_native_ref_admitted(sketch)),
                },
                equation.offset,
            ));
        }
    }
    Ok(constraints)
}

pub(in super::super) fn section_equation_equal_distance_constraints(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().ambiguous)
        .unwrap_or_default();
    collect_constraint_candidates(
        super::super::sketch::equations_coordinate::section_equation_equal_length_constraint_rows(
            definition,
            &ambiguous_point_ids,
        )
        .into_iter()
        .map(|equation| {
            let Some(first_start) = section_point_locus(definition, sketch, equation.first[0])
            else {
                return Ok(None);
            };
            let Some(first_end) = section_point_locus(definition, sketch, equation.first[1]) else {
                return Ok(None);
            };
            let Some(second_start) = section_point_locus(definition, sketch, equation.second[0])
            else {
                return Ok(None);
            };
            let Some(second_end) = section_point_locus(definition, sketch, equation.second[1])
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
            Ok(equation_constraint(
                sketch,
                equation.equation_id,
                SketchConstraintDefinitionInput::EqualDistance { first, second },
                equation.active,
                equation.offset,
            ))
        }),
    )
}

fn section_equation_radius_dimension_parameters(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<BTreeMap<SectionScalarVariable, Option<(ParameterId, f64)>>, cadmpeg_core::CodecError> {
    let mut dimension_parameters =
        BTreeMap::<SectionScalarVariable, Option<(ParameterId, f64)>>::new();
    let Some(dimensions) = definition.dimensions.as_ref() else {
        return Ok(dimension_parameters);
    };
    for equation in section_equation_radius_dimensions(definition) {
        let Some(ordinal) = usize::try_from(equation.scalar.1).ok() else {
            continue;
        };
        let Some((dimension, parameter)) =
            resolved_feature_dimension_parameter(sketch, dimensions, ordinal)?
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
            if let Some(slot) = dimension_parameters.get_mut(&variable) {
                if slot.as_ref() != Some(&candidate) {
                    *slot = None;
                }
            } else {
                let copied_parameter = candidate.0.clone();
                dimension_parameters.insert(variable, Some((copied_parameter, candidate.1)));
            }
        }
    }
    Ok(dimension_parameters)
}

fn section_equation_dimension_parameter(
    parameters: &BTreeMap<SectionScalarVariable, Option<(ParameterId, f64)>>,
    variable: SectionScalarVariable,
    value: f64,
) -> Option<ParameterId> {
    let Some(Some((parameter, dimension_value))) = parameters.get(&variable) else {
        return None;
    };
    if (FiniteReal::new(*dimension_value))
        .zip(FiniteReal::new(value))
        .is_some_and(|(first, second)| approximately_equal(first, second))
    {
        Some(parameter.clone())
    } else {
        None
    }
}

pub(in super::super) fn section_equation_function_six_distance_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let coordinates = resolved_section_coordinates(ctx, definition)?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().ambiguous)
        .unwrap_or_default();
    let dimension_parameters = section_equation_radius_dimension_parameters(definition, sketch)?;
    collect_constraint_candidates(
        section_equation_function_six_distance_rows(definition, &coordinates, &ambiguous_point_ids)
            .into_iter()
            .map(|equation| {
                let Some(distance) = equation.constraint_distance() else {
                    return Ok(None);
                };
                let Some(first) = section_point_locus(definition, sketch, equation.first) else {
                    return Ok(None);
                };
                let Some(second) = section_point_locus(definition, sketch, equation.second) else {
                    return Ok(None);
                };
                let parameter = section_equation_dimension_parameter(
                    &dimension_parameters,
                    equation.radius,
                    distance.get(),
                );
                Ok(equation_constraint(
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
                ))
            }),
    )
}

pub(in super::super) fn section_equation_function_forty_two_midpoint_coordinate_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let coordinates = resolved_section_coordinates(ctx, definition)?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().ambiguous)
        .unwrap_or_default();
    collect_constraint_candidates(
        section_equation_function_forty_two_midpoint_coordinate_rows(
            definition,
            &coordinates,
            &ambiguous_point_ids,
        )
        .into_iter()
        .map(|equation| {
            let Some(value) = equation.value else {
                return Ok(None);
            };
            if !value.is_finite() {
                return Ok(None);
            }
            let Some(first) = section_point_locus(definition, sketch, equation.first) else {
                return Ok(None);
            };
            let Some(second) = section_point_locus(definition, sketch, equation.second) else {
                return Ok(None);
            };
            let axis = match equation.coordinate {
                SectionAxis::U => SketchCoordinateAxis::U,
                SectionAxis::V => SketchCoordinateAxis::V,
            };
            let Some(value) = Length::new(value) else {
                return Ok(None);
            };
            Ok(equation_constraint(
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
            ))
        }),
    )
}

pub(in super::super) fn section_equation_function_thirty_one_point_coordinate_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let coordinates = resolved_section_coordinates(ctx, definition)?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().ambiguous)
        .unwrap_or_default();
    collect_constraint_candidates(
        section_equation_function_thirty_one_point_coordinate_rows(
            definition,
            &coordinates,
            &ambiguous_point_ids,
        )
        .into_iter()
        .map(|equation| {
            let [u, v] = equation.values;
            let (Some(u), Some(v)) = (u, v) else {
                return Ok(None);
            };
            if !u.is_finite() || !v.is_finite() {
                return Ok(None);
            }
            let Some(point) = section_point_locus(definition, sketch, equation.point) else {
                return Ok(None);
            };
            let (Some(u), Some(v)) = (Length::new(u), Length::new(v)) else {
                return Ok(None);
            };
            Ok(equation_constraint(
                sketch,
                equation.equation_id,
                SketchConstraintDefinitionInput::PointCoordinateValues {
                    point,
                    values: [u, v],
                },
                equation.active,
                equation.offset,
            ))
        }),
    )
}

pub(super) fn section_equation_function_sixteen_angle_difference_constraints(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    collect_constraint_candidates(
        section_equation_function_sixteen_angle_difference_rows(definition)
            .into_iter()
            .map(|equation| {
                let Some(value) = Angle::new(equation.value) else {
                    return Ok(None);
                };
                Ok(equation_constraint(
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
                ))
            }),
    )
}

pub(super) fn section_equation_function_five_scalar_equality_constraints(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    collect_constraint_candidates(
        section_equation_function_five_scalar_equality_rows(definition)
            .into_iter()
            .map(|equation| {
                Ok(equation_constraint(
                    sketch,
                    equation.equation_id,
                    SketchConstraintDefinitionInput::ScalarEquality {
                        first: equation.first.1,
                        second: equation.second.1,
                    },
                    true,
                    equation.offset,
                ))
            }),
    )
}

pub(in super::super) fn section_equation_polar_distance_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let coordinates = resolved_section_coordinates(ctx, definition)?;
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().ambiguous)
        .unwrap_or_default();
    let dimension_parameters = section_equation_radius_dimension_parameters(definition, sketch)?;
    collect_constraint_candidates(
        section_equation_radial_constraint_rows(definition, &coordinates, &ambiguous_point_ids)
            .into_iter()
            .map(|equation| {
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
                let Some(first) = section_point_locus(definition, sketch, equation.first) else {
                    return Ok(None);
                };
                let Some(second) = section_point_locus(definition, sketch, equation.second) else {
                    return Ok(None);
                };
                let distance_parameter = section_equation_dimension_parameter(
                    &dimension_parameters,
                    equation.radius,
                    distance.get(),
                );
                Ok(equation_constraint(
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
                ))
            }),
    )
}

struct EquationArgumentSlots<'a>(&'a [Option<u32>]);

impl std::fmt::Display for EquationArgumentSlots<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (slot, argument) in self.0.iter().enumerate() {
            if slot != 0 {
                f.write_str(",")?;
            }
            match argument {
                Some(argument) => write!(f, "{slot}:{argument}")?,
                None => write!(f, "{slot}:null")?,
            }
        }
        Ok(())
    }
}

struct EquationNullOrdinals<'a>(&'a [Option<u32>]);

impl std::fmt::Display for EquationNullOrdinals<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for (slot, argument) in self.0.iter().enumerate() {
            if argument.is_some() {
                continue;
            }
            if !first {
                f.write_str(",")?;
            }
            write!(f, "{slot}")?;
            first = false;
        }
        Ok(())
    }
}

fn insert_native_equation_property(
    properties: &mut BTreeMap<String, String>,
    key: &'static str,
    value: impl std::fmt::Display,
) {
    let key = key.to_owned();
    let value = format!("{value}");
    properties.insert(key, value);
}

fn native_equation_nonblank(
    value: std::fmt::Arguments<'_>,
) -> Result<cadmpeg_core::text::NonBlankString, cadmpeg_core::CodecError> {
    cadmpeg_core::text::NonBlankString::new(std::fmt::format(value)).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("native equation text must not be blank")
    })
}

fn native_equation_operands(
    equation_id: u32,
    arguments: &[Option<u32>],
    native_ref: &str,
) -> Result<Vec<SketchNativeOperand>, cadmpeg_core::CodecError> {
    let mut operands = Vec::new();
    operands.push(SketchNativeOperand {
        native_kind: native_equation_nonblank(format_args!("eqtn_arr"))?,
        field: Some(NativeOperandField {
            name: native_equation_nonblank(format_args!("equation_id"))?,
            role: None,
        }),
        object_index: Some(equation_id),
        native_ref: Some(native_ref.to_owned()),
    });
    for (slot, argument) in arguments.iter().enumerate() {
        let Some(object_index) = *argument else {
            continue;
        };
        operands.push(SketchNativeOperand {
            native_kind: native_equation_nonblank(format_args!("var_arr"))?,
            field: Some(NativeOperandField {
                name: native_equation_nonblank(format_args!("arguments[{slot}]"))?,
                role: None,
            }),
            object_index: Some(object_index),
            native_ref: Some(native_ref.to_owned()),
        });
    }
    Ok(operands)
}

pub(in super::super) fn section_equation_native_constraints(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    typed_offsets: &BTreeSet<usize>,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let Some(table) =
        crate::feature::definitions::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Ok(Vec::new());
    };
    let mut constraints = Vec::new();
    for equation in table.rows {
        if typed_offsets.contains(&equation.offset) {
            continue;
        }
        let active = !section_solver_equation_is_disabled(definition, equation.equation_id);
        let native_ref = sketch_native_ref_admitted(sketch);
        let mut native_properties = BTreeMap::new();
        insert_native_equation_property(
            &mut native_properties,
            "equation_id",
            equation.equation_id,
        );
        insert_native_equation_property(
            &mut native_properties,
            "function_id",
            equation.function_id,
        );
        insert_native_equation_property(&mut native_properties, "offset", equation.offset);
        insert_native_equation_property(&mut native_properties, "table_offset", table.offset);
        insert_native_equation_property(
            &mut native_properties,
            "table_declared_count",
            table.declared_count,
        );
        insert_native_equation_property(&mut native_properties, "active", active);
        insert_native_equation_property(
            &mut native_properties,
            "argument_slots",
            EquationArgumentSlots(&equation.arguments),
        );
        if let Some(count) = equation.explicit_argument_count {
            insert_native_equation_property(
                &mut native_properties,
                "explicit_argument_count",
                count,
            );
        }
        if let Some(entity_ref) = table.entity_ref {
            insert_native_equation_property(&mut native_properties, "table_entity_ref", entity_ref);
        }
        if equation.arguments.iter().any(Option::is_none) {
            insert_native_equation_property(
                &mut native_properties,
                "null_argument_ordinals",
                EquationNullOrdinals(&equation.arguments),
            );
        }
        let operands =
            native_equation_operands(equation.equation_id, &equation.arguments, &native_ref)?;
        let Some(id) = sketch_constraint_id_admitted(
            sketch,
            format_args!("equation:offset:{}", equation.offset),
        ) else {
            continue;
        };
        let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Native {
                native_kind: native_equation_nonblank(format_args!(
                    "creo:equation:{}",
                    equation.function_id
                ))?,
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
        constraints.push((
            SketchConstraint {
                id,
                sketch: sketch.clone(),
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
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().ambiguous)
        .unwrap_or_default();
    let rows = super::super::sketch::equations_scalar::section_equation_coordinate_equality_rows(
        definition,
        &ambiguous_point_ids,
    );
    collect_constraint_candidates(
        rows.into_iter()
            .filter(|equation| matches!(equation.function_id, 2 | 10 | 13))
            .map(|equation| {
                let Some(first) = section_point_locus(definition, sketch, equation.first) else {
                    return Ok(None);
                };
                let Some(second) = section_point_locus(definition, sketch, equation.second) else {
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
                Ok(equation_constraint(
                    sketch,
                    equation.equation_id,
                    SketchConstraintDefinitionInput::SameCoordinate { relation },
                    equation.active,
                    equation.offset,
                ))
            }),
    )
}

pub(in super::super) fn section_equation_point_on_line_constraints(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().ambiguous)
        .unwrap_or_default();
    let unique_segment_ids = unique_section_segment_external_ids(definition);
    let segments = section_segment_rows(definition);
    collect_constraint_candidates(
        section_equation_point_on_line_constraint_rows(definition, &ambiguous_point_ids)
            .into_iter()
            .map(|equation| {
                let Some(point) = section_point_locus(definition, sketch, equation.target) else {
                    return Ok(None);
                };
                let Some(line_external_id) = crate::decode::uniqueness::exactly_one(
                    segments
                        .iter()
                        .filter(|segment| {
                            matches!(
                                segment.kind,
                                crate::feature::definitions::FeatureSegmentKind::Line(_)
                            ) && unique_segment_ids.contains(&segment.external_id)
                                && (segment.point_ids() == [equation.first, equation.second]
                                    || segment.point_ids() == [equation.second, equation.first])
                        })
                        .map(|segment| segment.external_id)
                        .chain(
                            definition
                                .segments
                                .iter()
                                .flat_map(|table| table.rows.reference_lines())
                                .filter(|segment| {
                                    unique_segment_ids.contains(&segment.external_id)
                                        && (segment.point_ids
                                            == [Some(equation.first), Some(equation.second)]
                                            || segment.point_ids
                                                == [Some(equation.second), Some(equation.first)])
                                })
                                .map(|segment| segment.external_id),
                        )
                        .chain(
                            definition
                                .segments
                                .iter()
                                .flat_map(|table| table.rows.centered_lines())
                                .filter(|segment| {
                                    unique_segment_ids.contains(&segment.external_id)
                                        && matches!(
                                            [equation.first, equation.second],
                                            [0, 1] | [1, 0]
                                        )
                                })
                                .map(|segment| segment.external_id),
                        ),
                ) else {
                    return Ok(None);
                };
                let Some(entity) = sketch_entity_id_admitted(sketch, line_external_id) else {
                    return Ok(None);
                };
                Ok(equation_constraint(
                    sketch,
                    equation.equation_id,
                    SketchConstraintDefinitionInput::PointOnObject { point, entity },
                    equation.active,
                    equation.offset,
                ))
            }),
    )
}

pub(in super::super) fn section_equation_axis_distance_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let Some(dimensions) = definition.dimensions.as_ref() else {
        return Ok(Vec::new());
    };
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().ambiguous)
        .unwrap_or_default();
    collect_constraint_candidates(
        section_equation_function_forty_three_axis_distance_rows(
            definition,
            &resolved_section_coordinates(ctx, definition)?,
            &ambiguous_point_ids,
        )
        .into_iter()
        .map(|equation| {
            let Some(first) = section_point_locus(definition, sketch, equation.first) else {
                return Ok(None);
            };
            let Some(second) = section_point_locus(definition, sketch, equation.second) else {
                return Ok(None);
            };
            let Ok(ordinal) = usize::try_from(equation.scalar.1) else {
                return Ok(None);
            };
            let Some((dimension, parameter)) =
                resolved_feature_dimension_parameter(sketch, dimensions, ordinal)?
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
            Ok(equation_constraint(
                sketch,
                equation.equation_id,
                definition,
                equation.active,
                equation.offset,
            ))
        }),
    )
}

pub(in super::super) fn section_equation_unsigned_distance_constraints(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let Some(dimensions) = definition.dimensions.as_ref() else {
        return Ok(Vec::new());
    };
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().ambiguous)
        .unwrap_or_default();
    collect_constraint_candidates(
        section_equation_unsigned_coordinate_distance_rows(definition, &ambiguous_point_ids)
            .into_iter()
            .map(|equation| {
                let Some(first) = section_point_locus(definition, sketch, equation.first) else {
                    return Ok(None);
                };
                let Some(second) = section_point_locus(definition, sketch, equation.second) else {
                    return Ok(None);
                };
                let Ok(ordinal) = usize::try_from(equation.scalar.1) else {
                    return Ok(None);
                };
                let Some((_, parameter)) =
                    resolved_feature_dimension_parameter(sketch, dimensions, ordinal)?
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
                Ok(equation_constraint(
                    sketch,
                    equation.equation_id,
                    definition,
                    equation.active,
                    equation.offset,
                ))
            }),
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
    properties: &mut BTreeMap<String, String>,
    key: &'static str,
    value: impl std::fmt::Display,
) {
    let value = format!("{value}");
    let key = key.to_owned();
    properties.insert(key, value);
}

fn push_relation_operand(
    operands: &mut Vec<SketchNativeOperand>,
    native_ref: &str,
    kind: &'static str,
    field: Option<String>,
    object_index: u32,
) -> Result<(), cadmpeg_core::CodecError> {
    let kind = kind.to_owned();
    let native_ref = native_ref.to_owned();
    let field = field
        .map(|name| {
            cadmpeg_core::text::NonBlankString::new(name)
                .map(|name| NativeOperandField { name, role: None })
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("native operand field must not be empty")
                })
        })
        .transpose()?;
    operands.push(SketchNativeOperand {
        native_kind: cadmpeg_core::text::NonBlankString::new(kind).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("native operand kind must not be empty")
        })?,
        field,
        object_index: Some(object_index),
        native_ref: Some(native_ref),
    });
    Ok(())
}

fn native_section_dimension_constraint_definition(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    relation: &crate::feature::definitions::FeatureRelation,
) -> Result<Option<SketchConstraintDefinitionInput>, cadmpeg_core::CodecError> {
    let native_kind = format!("creo:relation:{}", relation.relation_type);
    let native_kind = cadmpeg_core::text::NonBlankString::new(native_kind).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("native relation kind must not be empty")
    })?;
    let mut native_properties = BTreeMap::new();
    insert_relation_property(
        &mut native_properties,
        "dimension_id",
        relation.dimension_id,
    );
    insert_relation_property(&mut native_properties, "sign", relation.sign);
    let Some(relations) = definition.relations.as_ref() else {
        insert_relation_property(&mut native_properties, "relation_id", relation.relation_id);
        return Ok(Some(SketchConstraintDefinitionInput::Native {
            native_kind,
            native_state: Some(u64::from(relation.used)),
            native_flags: None,
            native_properties,
            entities: Vec::new(),
            parameter: None,
            operands: Vec::new(),
        }));
    };
    let unique_relation_id = feature_relation_table_complete(relations)
        && relations
            .rows
            .iter()
            .filter(|candidate| candidate.relation_id == relation.relation_id)
            .count()
            == 1;
    let joined_relation_incidence_link = unique_relation_id
        .then(|| joined_relation_incidence_link(definition, relation.relation_id))
        .flatten();
    let joined_incidence = joined_relation_incidence_link.map(|(_, incidence)| incidence);
    let parameter = match definition
        .dimensions
        .as_ref()
        .zip(usize::try_from(relation.dimension_id).ok())
    {
        Some((dimensions, ordinal)) => {
            resolved_feature_dimension_parameter(sketch, dimensions, ordinal)?
                .map(|(_, parameter)| parameter)
        }
        None => None,
    };
    let entities = if unique_relation_id {
        joined_relation_incidence_entities(definition, sketch, relation.relation_id)
    } else {
        Vec::new()
    };
    if !unique_relation_id {
        insert_relation_property(&mut native_properties, "relation_id", relation.relation_id);
    }
    let native_ref = sketch_native_ref_admitted(sketch);
    let mut operands = Vec::new();
    if unique_relation_id {
        push_relation_operand(
            &mut operands,
            &native_ref,
            "relat_ptr",
            None,
            relation.relation_id,
        )?;
    }
    if let Some(incidence) = joined_incidence {
        let field = "triples_ptr.skamp_id".to_owned();
        push_relation_operand(
            &mut operands,
            &native_ref,
            "skamp_ptr",
            Some(field),
            incidence.id,
        )?;
    }
    if let Some(equation_id) = joined_relation_incidence_link.and_then(|(join, _)| join.equation_id)
    {
        let field = "equation_id".to_owned();
        push_relation_operand(
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
                    let field = format!("{vector}[{slot}]");
                    push_relation_operand(
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
    constraint_definition: &mut SketchConstraintDefinitionInput,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    relation: &crate::feature::definitions::FeatureRelation,
    emitted: &BTreeSet<SketchEntityId>,
    available_parameters: &BTreeSet<ParameterId>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let entity_reconciled = reconcile_constraint_entity_references(constraint_definition, emitted);
    let parameter_reconciled =
        reconcile_constraint_parameter_reference(constraint_definition, available_parameters);
    if entity_reconciled && parameter_reconciled {
        return Ok(true);
    }
    let Some(native_definition) =
        native_section_dimension_constraint_definition(definition, sketch, relation)?
    else {
        return Ok(false);
    };
    *constraint_definition = native_definition;
    Ok(
        reconcile_constraint_entity_references(constraint_definition, emitted)
            && reconcile_constraint_parameter_reference(
                constraint_definition,
                available_parameters,
            ),
    )
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

pub(in super::super) fn section_dimension_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
) -> Result<Vec<(SketchConstraint, usize, usize)>, cadmpeg_core::CodecError> {
    let Some(relations) = &definition.relations else {
        return Ok(Vec::new());
    };
    let segments = section_segment_rows(definition);

    let known_entities = section_entity_external_ids(definition);
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().ambiguous)
        .unwrap_or_default();
    let resolved_coordinates = resolved_section_coordinates(ctx, definition)?;
    let saved_coordinate_witnesses =
        saved_section_coordinate_witnesses(definition, &ambiguous_point_ids);
    let mut constraints = Vec::new();
    for (relation_index, relation) in relations.rows.iter().enumerate() {
        let mut coordinate_refusal = None;
        let candidate = (|| {
            Some({
                let unique_relation_id = feature_relation_table_complete(relations)
                    && relations
                        .rows
                        .iter()
                        .filter(|candidate| candidate.relation_id == relation.relation_id)
                        .count()
                        == 1;
                let dimension = match definition
                    .dimensions
                    .as_ref()
                    .zip(usize::try_from(relation.dimension_id).ok())
                {
                    Some((dimensions, ordinal)) => capture_constraint_refusal(
                        &mut coordinate_refusal,
                        resolved_feature_dimension_parameter(sketch, dimensions, ordinal),
                    )?,
                    None => None,
                };
                let parameter = dimension.as_ref().map(|(_, parameter)| parameter.clone());
                let joined_incidence_link = unique_relation_id
                    .then(|| joined_relation_incidence_link(definition, relation.relation_id))
                    .flatten();
                let joined_incidence = joined_incidence_link.map(|(_, incidence)| incidence);
                let typed = (|| {
                    unique_relation_id.then_some(())?;
                    let (dimension, _) = dimension.as_ref()?;
                    let parameter = parameter.as_ref()?.clone();
                    if relation.relation_type == 1
                        && dimension.unit() == crate::feature::definitions::DimensionUnit::Radians
                    {
                        let [first, second] = capture_constraint_refusal(
                            &mut coordinate_refusal,
                            Ok(section_angular_entities(
                                definition,
                                sketch,
                                &segments,
                                relation.operand_vectors?,
                                &known_entities,
                            )),
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
                                && known_entities.contains(&measured.external_id)
                            {
                                let entity =
                                    sketch_entity_id_admitted(sketch, measured.external_id)?;
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
                        let segment = section_radius_relation_arc(definition, relation)?;
                        return Some(circular_dimension_constraint(
                            sketch_entity_id_admitted(sketch, segment.external_id)?,
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
                        let (external_id, _) = crate::decode::uniqueness::exactly_one(
                            segments
                                .iter()
                                .filter(|segment| {
                                    matches!(
                                        segment.kind,
                                        crate::feature::definitions::FeatureSegmentKind::Arc(_)
                                    )
                                })
                                .map(|segment| (segment.external_id, segment.radius_ref))
                                .chain(
                                    definition
                                        .segments
                                        .iter()
                                        .flat_map(|table| table.rows.circles())
                                        .map(|segment| {
                                            (segment.external_id, Some(segment.radius_ref))
                                        }),
                                )
                                .filter(|(_, radius_ref)| *radius_ref == Some(radius_id)),
                        )?;
                        known_entities.contains(&external_id).then_some(())?;
                        return Some(circular_dimension_constraint(
                            sketch_entity_id_admitted(sketch, external_id)?,
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
                                let coordinate = section_linear_distance_coordinate(
                                    definition,
                                    &segments,
                                    [first_id, second_id],
                                    &resolved_coordinates,
                                    &saved_coordinate_witnesses,
                                    &ambiguous_point_ids,
                                );
                                let measured = crate::decode::uniqueness::exactly_one(
                                    segments.iter().filter(|segment| {
                                        segment.point_ids() == [first_id, second_id]
                                            || segment.point_ids() == [second_id, first_id]
                                    }),
                                );
                                if let Some(measured) = measured {
                                    if matches!(
                                        measured.kind,
                                        crate::feature::definitions::FeatureSegmentKind::Line(_)
                                    ) && known_entities.contains(&measured.external_id)
                                    {
                                        let entity = sketch_entity_id_admitted(
                                            sketch,
                                            measured.external_id,
                                        )?;
                                        let [first, second] =
                                            if measured.point_ids() == [first_id, second_id] {
                                                [
                                                    SketchLocus::Start(entity.clone()),
                                                    SketchLocus::End(entity),
                                                ]
                                            } else {
                                                [
                                                    SketchLocus::End(entity.clone()),
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
                                        Ok(section_point_locus(definition, sketch, first_id)),
                                    )?,
                                    capture_constraint_refusal(
                                        &mut coordinate_refusal,
                                        Ok(section_point_locus(definition, sketch, second_id)),
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
                    if let Some([first, second]) =
                        relation_incidence_loci(definition, sketch, relation.relation_id)
                    {
                        return Some(SketchConstraintDefinitionInput::DistanceLoci {
                            first,
                            second,
                            parameter,
                        });
                    }
                    if let Some(incidence) =
                        joined_incidence.filter(|incidence| !section_skamp_active(incidence.status))
                    {
                        if let [first, second] = incidence.items.as_slice() {
                            if let (Some(first), Some(second)) = (
                                section_skamp_locus(definition, sketch, first),
                                section_skamp_locus(definition, sketch, second),
                            ) {
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
                                    Ok(joined_relation_incidence_entities(
                                        definition,
                                        sketch,
                                        relation.relation_id,
                                    )),
                                )?,
                                parameter,
                            });
                        }
                    }
                    let entities = capture_constraint_refusal(
                        &mut coordinate_refusal,
                        Ok(relation_incidence_entities(
                            definition,
                            sketch,
                            relation.relation_id,
                        )),
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
                            definition, sketch, relation,
                        ),
                    )??,
                };
                (
                    SketchConstraint {
                        id: if unique_relation_id {
                            sketch_constraint_id_admitted(
                                sketch,
                                format_args!("relation:{}", relation.relation_id),
                            )?
                        } else {
                            sketch_constraint_id_admitted(
                                sketch,
                                format_args!("relation:offset:{}", relation.offset),
                            )?
                        },
                        sketch: sketch.clone(),
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
                        native_ref: Some(sketch_native_ref_admitted(sketch)),
                    },
                    relation.offset,
                    relation_index,
                )
            })
        })();
        if let Some(error) = coordinate_refusal {
            return Err(error);
        }
        if let Some(candidate) = candidate {
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
    use super::{
        close_sketch_constraint_parameter_references, insert_native_equation_property,
        insert_relation_property, native_equation_operands, push_relation_operand,
        reconcile_section_dimension_constraint,
        section_equation_function_five_scalar_equality_constraints,
        section_equation_function_sixteen_angle_difference_constraints,
    };
    use cadmpeg_ir::features::ParameterId;
    use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntityId, SketchId};
    use std::collections::BTreeSet;

    #[test]
    fn equation_constraint_collects_one_scalar_equality_row() {
        let sketch = SketchId::mint("creo:model:sketch#5").expect("valid sketch ID");
        let definition = SketchConstraintDefinitionInput::ScalarEquality {
            first: 10,
            second: 11,
        };
        let rows = ({
            super::collect_constraint_candidates([Ok(super::equation_constraint(
                &sketch, 1, definition, true, 7,
            ))])
        })
        .expect("one equation row");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].0.id.as_str(),
            "creo:featdefs:sketch_constraint#5:equation:1"
        );
        assert_eq!(rows[0].1, 7);
    }

    #[test]
    fn native_verhor_definition_names_property_entity_and_operand() {
        let sketch = SketchId::mint("creo:model:sketch#5").expect("valid sketch ID");
        let entity =
            SketchEntityId::mint("creo:featdefs:sketch_entity#5:42").expect("valid entity ID");
        let admitted =
            super::native_section_segment_verhor_definition(&sketch, entity.clone(), 42, 2)
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
    fn native_segment_radius_definition_names_property_entity_and_operands() {
        let sketch = SketchId::mint("creo:model:sketch#5").expect("valid sketch ID");
        let entity =
            SketchEntityId::mint("creo:featdefs:sketch_entity#5:42").expect("valid entity ID");
        let admitted = super::native_section_segment_radius_definition(
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
    fn segment_radius_rows_name_constraint_for_segment() {
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
        let rows = ({ super::section_segment_radius_constraints(&definition, &sketch) })
            .expect("radius row");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].0.id.as_str(),
            "creo:featdefs:sketch_constraint#5:segtab-radius:42"
        );
        let emitted = BTreeSet::from([
            SketchEntityId::mint("creo:featdefs:sketch_entity#5:42").expect("valid entity ID")
        ]);
        assert_eq!(
            ({
                super::section_segment_radius_constraints_for_emitted(
                    &definition,
                    &sketch,
                    &emitted,
                    &BTreeSet::new(),
                )
            })
            .expect("emitted radius row")
            .len(),
            1
        );
    }

    #[test]
    fn native_relation_property_stores_value() {
        let mut properties = std::collections::BTreeMap::new();
        insert_relation_property(&mut properties, "dimension_id", 7);
        assert_eq!(properties["dimension_id"], "7");
    }

    #[test]
    fn native_relation_operand_stores_index_and_reference() {
        let native_ref = "creo:featdefs:sketch#5";
        let mut operands = Vec::new();
        push_relation_operand(&mut operands, native_ref, "relat_ptr", None, 7).expect("operand");
        assert_eq!(operands[0].object_index, Some(7));
        assert_eq!(operands[0].native_ref.as_deref(), Some(native_ref));
    }

    #[test]
    fn native_equation_property_stores_value() {
        let mut properties = std::collections::BTreeMap::new();
        insert_native_equation_property(&mut properties, "equation_id", 7);
        assert_eq!(properties.get("equation_id").map(String::as_str), Some("7"));
    }

    #[test]
    fn native_equation_operands_name_each_argument() {
        let arguments = [None, Some(2), Some(3)];
        let operands = native_equation_operands(1, &arguments, "creo:featdefs:sketch#40")
            .expect("service operands");
        assert_eq!(operands.len(), 3);
        assert_eq!(operands[0].object_index, Some(1));
        assert_eq!(
            operands[1].field.as_ref().map(|field| field.name.as_str()),
            Some("arguments[1]")
        );
        assert_eq!(operands[2].object_index, Some(3));
    }

    #[test]
    fn closing_parameter_references_keeps_parameter_ids() {
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
        close_sketch_constraint_parameter_references(&mut document);
        assert_eq!(document.model.parameters[0].id, id);
    }

    #[test]
    fn segment_radius_constraints_emit_without_emitted_entities() {
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
        ({
            assert_eq!(
                super::section_segment_radius_constraints(&definition, &sketch)
                    .expect("service radius constraint")
                    .len(),
                1
            );
            assert_eq!(
                super::section_segment_radius_constraints_for_emitted(
                    &definition,
                    &sketch,
                    &BTreeSet::new(),
                    &BTreeSet::new()
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
        let constraints = ({
            section_equation_function_sixteen_angle_difference_constraints(&definition, &sketch)
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
        let constraints =
            ({ section_equation_function_five_scalar_equality_constraints(&definition, &sketch) })
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
        assert!(({
            section_equation_function_five_scalar_equality_constraints(&conflicting, &sketch)
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

        assert!(reconcile_section_dimension_constraint(
            &mut constraint,
            &definition,
            &sketch,
            &relation,
            &BTreeSet::new(),
            &BTreeSet::new()
        )
        .expect("service dimension fallback admission"));
        assert!(matches!(
            constraint,
            SketchConstraintDefinitionInput::Native {
                native_kind,
                entities,
                ..
            } if native_kind == "creo:relation:0" && entities.is_empty()
        ));

        let emitted_entity = SketchEntityId::mint("synthetic:test:dimension-relation#emitted")
            .expect("valid test fixture");
        let mut missing_parameter = SketchConstraintDefinitionInput::Distance {
            entities: vec![emitted_entity.clone()],
            parameter: ParameterId::mint("synthetic:test:id#synthetic:test:dimension-parameter")
                .expect("identity grammar"),
        };
        assert!(reconcile_section_dimension_constraint(
            &mut missing_parameter,
            &definition,
            &sketch,
            &relation,
            &BTreeSet::from([emitted_entity]),
            &BTreeSet::new()
        )
        .expect("service dimension fallback admission"));
        assert!(matches!(
            missing_parameter,
            SketchConstraintDefinitionInput::Native {
                native_kind,
                ..
            } if native_kind == "creo:relation:0"
        ));
    }

    #[test]
    fn section_dimension_constraint_names_relation_row() {
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
        let admitted = crate::decode::with_service_context(|ctx| {
            super::section_dimension_constraints(ctx, &definition, &sketch)
        })
        .expect("dimension row");
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
        let constraints = crate::decode::with_service_context(|ctx| {
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
