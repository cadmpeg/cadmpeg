// SPDX-License-Identifier: Apache-2.0
//! Section constraint reconciliation, incidence, and dimension emission.

use crate::decode::sketch::axis::SectionAxis;

use crate::decode::sketch::equations_scalar::SectionScalarVariable;
use crate::feature::definitions::SolverSubtable;

use super::super::feature_history::{
    feature_relation_table_complete, resolved_feature_dimension_parameter,
};
use super::super::sketch::{
    approximately_equal, resolved_section_coordinates, saved_section_coordinate_witnesses,
    section_equation_function_five_scalar_equality_rows,
    section_equation_function_forty_three_axis_distance_rows,
    section_equation_function_forty_two_midpoint_coordinate_rows,
    section_equation_function_six_distance_rows,
    section_equation_function_sixteen_angle_difference_rows,
    section_equation_function_thirty_one_point_coordinate_rows,
    section_equation_point_on_line_constraint_rows, section_equation_radial_constraint_rows,
    section_equation_radius_dimensions, section_equation_unsigned_coordinate_distance_rows,
    section_linear_distance_coordinate, section_radius_relation_arc, section_segment_rows,
    unique_decoded_section_segment,
};
use super::super::sketch_ids::{sketch_constraint_id, sketch_entity_id, sketch_native_ref};
use crate::decode::sketch_transfer::identity::{
    opaque_section_segment_identity_suffix, section_entity_external_ids,
    section_segment_identity_suffix, unique_section_segment_external_ids,
};
use crate::decode::sketch_transfer::loci::{
    section_point_locus, section_skamp_active, section_skamp_locus,
};
use cadmpeg_ir::document::CadIr;
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

pub(in super::super) fn section_segment_verhor_definition(
    segment: &crate::feature::FeatureSegment,
    sketch: &SketchId,
    entity: SketchEntityId,
) -> Option<SketchConstraintDefinitionInput> {
    let verhor = segment.vertical_horizontal?;
    match (segment.kind, verhor) {
        (crate::feature::FeatureSegmentKind::Line(_), 0) => {
            Some(SketchConstraintDefinitionInput::Vertical { entity })
        }
        (crate::feature::FeatureSegmentKind::Line(_), 1) => {
            Some(SketchConstraintDefinitionInput::Horizontal { entity })
        }
        _ => native_section_segment_verhor_definition(sketch, entity, segment.external_id, verhor),
    }
}

pub(in super::super) fn native_section_segment_verhor_definition(
    sketch: &SketchId,
    entity: SketchEntityId,
    external_id: u32,
    verhor: u32,
) -> Option<SketchConstraintDefinitionInput> {
    Some(SketchConstraintDefinitionInput::Native {
        native_kind: cadmpeg_ir::products::NonEmptyString::new("creo:segtab:verhor")?,
        native_state: None,
        native_flags: None,
        native_properties: BTreeMap::from([("verhor".to_string(), verhor.to_string())]),
        entities: vec![entity],
        parameter: None,
        operands: vec![SketchNativeOperand {
            native_kind: cadmpeg_ir::products::NonEmptyString::new("segtab_ptr")
                .expect("source operand kind is nonempty"),
            field: Some(NativeOperandField {
                name: cadmpeg_ir::products::NonEmptyString::new("ext_id")
                    .expect("source field name is nonempty"),
                role: None,
            }),
            object_index: external_id,
            native_ref: Some(sketch_native_ref(sketch)),
        }],
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
        SketchConstraintDefinitionInput::Disabled => true,
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
        SketchConstraintDefinitionInput::Disabled
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
    let emitted = ir
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.id.clone())
        .collect::<BTreeSet<_>>();
    ir.model.sketch_constraints.retain_mut(|constraint| {
        constraint
            .definition
            .edit(|kind| reconcile_constraint_parameter_reference(kind, &emitted))
            .unwrap_or(false)
    });
}

pub(in super::super) fn joined_relation_incidence(
    definition: &crate::feature::FeatureDefinition,
    relation_id: u32,
) -> Option<&crate::feature::FeatureSkamp> {
    joined_relation_incidence_link(definition, relation_id).map(|(_, incidence)| incidence)
}

pub(in super::super) fn joined_relation_incidence_link(
    definition: &crate::feature::FeatureDefinition,
    relation_id: u32,
) -> Option<(
    &crate::feature::FeatureRelationTriple,
    &crate::feature::FeatureSkamp,
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
    let joins = relations
        .triples()
        .iter()
        .filter(|triple| triple.relation_id == Some(relation_id))
        .filter_map(|triple| triple.skamp_id.map(|incidence_id| (triple, incidence_id)))
        .collect::<Vec<_>>();
    let [(join, incidence_id)] = joins.as_slice() else {
        return None;
    };
    let incidences = relations
        .skamps()
        .iter()
        .filter(|skamp| skamp.id == *incidence_id)
        .collect::<Vec<_>>();
    let [incidence] = incidences.as_slice() else {
        return None;
    };
    Some((*join, *incidence))
}

pub(in super::super) fn section_solver_relation_is_disabled(
    definition: &crate::feature::FeatureDefinition,
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
    definition: &crate::feature::FeatureDefinition,
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
    let incidence_ids = relations
        .triples()
        .iter()
        .filter(|triple| triple.equation_id == Some(equation_id))
        .filter_map(|triple| triple.skamp_id)
        .collect::<Vec<_>>();
    let [incidence_id] = incidence_ids.as_slice() else {
        return false;
    };
    let incidences = relations
        .skamps()
        .iter()
        .filter(|skamp| skamp.id == *incidence_id)
        .collect::<Vec<_>>();
    let [incidence] = incidences.as_slice() else {
        return false;
    };
    !section_skamp_active(incidence.status)
}

pub(in super::super) fn relation_incidence(
    definition: &crate::feature::FeatureDefinition,
    relation_id: u32,
) -> Option<&crate::feature::FeatureSkamp> {
    let incidence = joined_relation_incidence(definition, relation_id)?;
    section_skamp_active(incidence.status).then_some(incidence)
}

pub(in super::super) fn relation_incidence_entities(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    relation_id: u32,
) -> Vec<SketchEntityId> {
    let Some(incidence) = relation_incidence(definition, relation_id) else {
        return Vec::new();
    };
    incidence
        .items
        .iter()
        .filter_map(|item| sketch_entity_id(sketch, item.entity_id))
        .collect()
}

pub(in super::super) fn joined_relation_incidence_entities(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    relation_id: u32,
) -> Vec<SketchEntityId> {
    let Some(incidence) = joined_relation_incidence(definition, relation_id) else {
        return Vec::new();
    };
    incidence
        .items
        .iter()
        .filter_map(|item| sketch_entity_id(sketch, item.entity_id))
        .collect()
}

pub(in super::super) fn relation_incidence_loci(
    definition: &crate::feature::FeatureDefinition,
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

pub(in super::super) fn section_angular_entities(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    segments: &[&crate::feature::FeatureSegment],
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
                    && matches!(segment.kind, crate::feature::FeatureSegmentKind::Line(_))
            })
            .collect::<Vec<_>>();
        (known_entities.contains(&external_id) && matching_segments.len() == 1)
            .then_some(external_id)
    };
    let [first, second] = [first_internal, second_internal].map(external_id);
    let [Some(first), Some(second)] = [first, second] else {
        return None;
    };
    (first != second).then_some(())?;
    Some([
        sketch_entity_id(sketch, first)?,
        sketch_entity_id(sketch, second)?,
    ])
}

pub(in super::super) fn native_section_segment_radius_definition(
    sketch: &SketchId,
    entity: SketchEntityId,
    external_id: u32,
    field: &str,
    dimension_ordinal: u32,
) -> Option<SketchConstraintDefinitionInput> {
    Some(SketchConstraintDefinitionInput::Native {
        native_kind: cadmpeg_ir::products::NonEmptyString::new(format!("creo:segtab:{field}"))?,
        native_state: None,
        native_flags: None,
        native_properties: BTreeMap::from([(
            "dimension_ordinal".to_string(),
            dimension_ordinal.to_string(),
        )]),
        entities: vec![entity],
        parameter: None,
        operands: vec![
            SketchNativeOperand {
                native_kind: cadmpeg_ir::products::NonEmptyString::new("segtab_ptr")
                    .expect("source operand kind is nonempty"),
                field: Some(NativeOperandField {
                    name: cadmpeg_ir::products::NonEmptyString::new("ext_id")
                        .expect("source field name is nonempty"),
                    role: None,
                }),
                object_index: external_id,
                native_ref: Some(sketch_native_ref(sketch)),
            },
            SketchNativeOperand {
                native_kind: cadmpeg_ir::products::NonEmptyString::new("dimension_ordinal")
                    .expect("source operand kind is nonempty"),
                field: Some(NativeOperandField {
                    name: cadmpeg_ir::products::NonEmptyString::new(field)
                        .expect("source field name is nonempty"),
                    role: None,
                }),
                object_index: dimension_ordinal,
                native_ref: Some(sketch_native_ref(sketch)),
            },
        ],
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
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<SectionSegmentRadiusBinding> {
    let unique_segment_ids = unique_section_segment_external_ids(definition);
    let mut bindings = Vec::new();
    let Some(segments) = definition.segments.as_ref() else {
        return bindings;
    };
    for segment in segments.rows.ordinary() {
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
            segment.external_id.to_string()
        } else {
            format!("circle:offset:{}", segment.offset)
        };
        let typed_circle = if unique_segment_ids.contains(&segment.external_id) {
            usize::try_from(segment.radius_ref)
                .ok()
                .and_then(|ordinal| {
                    resolved_feature_dimension_parameter(
                        sketch,
                        definition.dimensions.as_ref()?,
                        ordinal,
                    )
                })
                .map(|(dimension, parameter)| (dimension.dimension_type, parameter))
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
    bindings
}

fn section_segment_radius_constraint(
    binding: SectionSegmentRadiusBinding,
    sketch: &SketchId,
) -> Option<(SketchConstraint, usize)> {
    let entity = sketch_entity_id(sketch, &binding.suffix)?;
    let (definition, kind) = match binding.typed_circle {
        Some((dimension_type, parameter)) if matches!(dimension_type, 3 | 4) => (
            circular_dimension_constraint(entity.clone(), parameter, dimension_type),
            if dimension_type == 4 {
                "diameter"
            } else {
                "radius"
            },
        ),
        _ => (
            native_section_segment_radius_definition(
                sketch,
                entity.clone(),
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
    Some((
        SketchConstraint {
            id: sketch_constraint_id(sketch, format_args!("{kind}:{}", binding.suffix))?,
            sketch: sketch.clone(),
            definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
                .ok()?,
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: Some(sketch_native_ref(sketch)),
        },
        binding.offset,
    ))
}

pub(in super::super) fn section_segment_radius_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    section_segment_radius_bindings(definition, sketch)
        .into_iter()
        .filter_map(|binding| section_segment_radius_constraint(binding, sketch))
        .collect()
}

pub(in super::super) fn section_segment_radius_constraints_for_emitted(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    emitted: &BTreeSet<SketchEntityId>,
    available_parameters: &BTreeSet<ParameterId>,
) -> Vec<(SketchConstraint, usize)> {
    let bindings = section_segment_radius_bindings(definition, sketch);
    section_segment_radius_constraints(definition, sketch)
        .into_iter()
        .zip(bindings)
        .filter_map(|((mut constraint, offset), binding)| {
            constraint
                .definition
                .edit(|kind| {
                    reconcile_section_segment_radius_constraint(
                        kind,
                        sketch,
                        &binding,
                        emitted,
                        available_parameters,
                    )
                })
                .unwrap_or(false)
                .then_some((constraint, offset))
        })
        .collect()
}

fn reconcile_section_segment_radius_constraint(
    constraint_definition: &mut SketchConstraintDefinitionInput,
    sketch: &SketchId,
    binding: &SectionSegmentRadiusBinding,
    emitted: &BTreeSet<SketchEntityId>,
    available_parameters: &BTreeSet<ParameterId>,
) -> bool {
    let entity_reconciled = reconcile_constraint_entity_references(constraint_definition, emitted);
    let parameter_reconciled =
        reconcile_constraint_parameter_reference(constraint_definition, available_parameters);
    if entity_reconciled && parameter_reconciled {
        return true;
    }
    let Some(entity) = sketch_entity_id(sketch, &binding.suffix) else {
        return false;
    };
    let Some(native_definition) = native_section_segment_radius_definition(
        sketch,
        entity,
        binding.external_id,
        binding.field.key(),
        binding.ordinal,
    ) else {
        return false;
    };
    *constraint_definition = native_definition;
    reconcile_constraint_entity_references(constraint_definition, emitted)
        && reconcile_constraint_parameter_reference(constraint_definition, available_parameters)
}

pub(in super::super) fn section_equation_radius_dimension_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let Some(segments) = definition.segments.as_ref() else {
        return Vec::new();
    };
    let Some(dimensions) = definition.dimensions.as_ref() else {
        return Vec::new();
    };
    let unique_segment_ids = unique_section_segment_external_ids(definition);
    let mut entities_by_radius = BTreeMap::<u32, Vec<u32>>::new();
    for segment in segments.rows.ordinary().filter(|segment| {
        matches!(segment.kind, crate::feature::FeatureSegmentKind::Arc(_))
            && unique_segment_ids.contains(&segment.external_id)
    }) {
        if let Some(radius) = segment.radius_ref {
            entities_by_radius
                .entry(radius)
                .or_default()
                .push(segment.external_id);
        }
    }
    for segment in segments
        .rows
        .circles()
        .filter(|segment| unique_segment_ids.contains(&segment.external_id))
    {
        entities_by_radius
            .entry(segment.radius_ref)
            .or_default()
            .push(segment.external_id);
    }

    section_equation_radius_dimensions(definition)
        .into_iter()
        .flat_map(|equation| {
            let Some((dimension, parameter)) =
                usize::try_from(equation.scalar.1).ok().and_then(|ordinal| {
                    resolved_feature_dimension_parameter(sketch, dimensions, ordinal)
                })
            else {
                return Vec::new();
            };
            let Some(dimension_value) = dimension
                .value
                .resolved()
                .filter(|value| value.is_finite() && *value > 0.0)
            else {
                return Vec::new();
            };
            if dimension.dimension_type != 3
                || !approximately_equal(dimension_value, equation.value)
            {
                return Vec::new();
            }
            entities_by_radius
                .get(&equation.radius)
                .into_iter()
                .flatten()
                .copied()
                .filter_map(|external_id| {
                    Some({
                        let entity = sketch_entity_id(sketch, external_id)?;
                        (
                            SketchConstraint {
                                id: sketch_constraint_id(
                                    sketch,
                                    format_args!(
                                        "equation:{}:radius:{}",
                                        equation.equation_id, external_id
                                    ),
                                )?,
                                sketch: sketch.clone(),
                                definition:
                                    cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                                        SketchConstraintDefinitionInput::Radius {
                                            entity,
                                            parameter: parameter.clone(),
                                        },
                                    )
                                    .ok()?,
                                name: None,
                                driving: None,
                                active: Some(equation.active),
                                virtual_space: None,
                                visible: None,
                                orientation: None,
                                label_distance: None,
                                label_position: None,
                                metadata: None,
                                native_ref: Some(sketch_native_ref(sketch)),
                            },
                            equation.offset,
                        )
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(in super::super) fn section_equation_equal_distance_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().1)
        .unwrap_or_default();
    super::super::sketch::section_equation_equal_length_constraint_rows(
        definition,
        &ambiguous_point_ids,
    )
    .into_iter()
    .filter_map(|equation| {
        let first = SketchDistancePair {
            first: section_point_locus(definition, sketch, equation.first[0])?,
            second: section_point_locus(definition, sketch, equation.first[1])?,
        };
        let second = SketchDistancePair {
            first: section_point_locus(definition, sketch, equation.second[0])?,
            second: section_point_locus(definition, sketch, equation.second[1])?,
        };
        Some((
            SketchConstraint {
                id: sketch_constraint_id(
                    sketch,
                    format_args!("equation:{}", equation.equation_id),
                )?,
                sketch: sketch.clone(),
                definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                    SketchConstraintDefinitionInput::EqualDistance { first, second },
                )
                .ok()?,
                name: None,
                driving: None,
                active: Some(equation.active),
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(sketch_native_ref(sketch)),
            },
            equation.offset,
        ))
    })
    .collect()
}

fn section_equation_radius_dimension_parameters(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> BTreeMap<SectionScalarVariable, Option<(ParameterId, f64)>> {
    let mut dimension_parameters =
        BTreeMap::<SectionScalarVariable, Option<(ParameterId, f64)>>::new();
    let Some(dimensions) = definition.dimensions.as_ref() else {
        return dimension_parameters;
    };
    for equation in section_equation_radius_dimensions(definition) {
        let Some(ordinal) = usize::try_from(equation.scalar.1).ok() else {
            continue;
        };
        let Some((dimension, parameter)) =
            resolved_feature_dimension_parameter(sketch, dimensions, ordinal)
        else {
            continue;
        };
        let Some(dimension_value) = dimension.value.resolved() else {
            continue;
        };
        if dimension.dimension_type != 3
            || !dimension_value.is_finite()
            || dimension_value <= 0.0
            || !approximately_equal(dimension_value, equation.value)
        {
            continue;
        }
        let candidate = (parameter, dimension_value);
        for variable in [equation.radius_variable, equation.scalar] {
            let slot = dimension_parameters
                .entry(variable)
                .or_insert_with(|| Some(candidate.clone()));
            if slot.as_ref() != Some(&candidate) {
                *slot = None;
            }
        }
    }
    dimension_parameters
}

fn section_equation_dimension_parameter(
    parameters: &BTreeMap<SectionScalarVariable, Option<(ParameterId, f64)>>,
    variable: SectionScalarVariable,
    value: f64,
) -> Option<ParameterId> {
    let Some(Some((parameter, dimension_value))) = parameters.get(&variable) else {
        return None;
    };
    approximately_equal(*dimension_value, value).then(|| parameter.clone())
}

pub(in super::super) fn section_equation_function_six_distance_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let coordinates = resolved_section_coordinates(definition);
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().1)
        .unwrap_or_default();
    let dimension_parameters = section_equation_radius_dimension_parameters(definition, sketch);
    section_equation_function_six_distance_rows(definition, &coordinates, &ambiguous_point_ids)
        .into_iter()
        .filter_map(|equation| {
            let distance = equation.constraint_distance()?;
            let first = section_point_locus(definition, sketch, equation.first)?;
            let second = section_point_locus(definition, sketch, equation.second)?;
            let parameter = section_equation_dimension_parameter(
                &dimension_parameters,
                equation.radius,
                distance,
            );
            Some((
                SketchConstraint {
                    id: sketch_constraint_id(
                        sketch,
                        format_args!("equation:{}", equation.equation_id),
                    )?,
                    sketch: sketch.clone(),
                    definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                        SketchConstraintDefinitionInput::DistanceLociValue {
                            first,
                            second,
                            distance: Length::new(distance)?,
                            parameter,
                        },
                    )
                    .ok()?,
                    name: None,
                    driving: None,
                    active: Some(equation.active()),
                    virtual_space: None,
                    visible: None,
                    orientation: None,
                    label_distance: None,
                    label_position: None,
                    metadata: None,
                    native_ref: Some(sketch_native_ref(sketch)),
                },
                equation.offset,
            ))
        })
        .collect()
}

pub(in super::super) fn section_equation_function_forty_two_midpoint_coordinate_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let coordinates = resolved_section_coordinates(definition);
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().1)
        .unwrap_or_default();
    section_equation_function_forty_two_midpoint_coordinate_rows(
        definition,
        &coordinates,
        &ambiguous_point_ids,
    )
    .into_iter()
    .filter_map(|equation| {
        let value = equation.value?;
        if !value.is_finite() {
            return None;
        }
        let first = section_point_locus(definition, sketch, equation.first)?;
        let second = section_point_locus(definition, sketch, equation.second)?;
        let axis = match equation.coordinate {
            SectionAxis::U => SketchCoordinateAxis::U,
            SectionAxis::V => SketchCoordinateAxis::V,
        };
        Some((
            SketchConstraint {
                id: sketch_constraint_id(
                    sketch,
                    format_args!("equation:{}", equation.equation_id),
                )?,
                sketch: sketch.clone(),
                definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                    SketchConstraintDefinitionInput::MidpointCoordinate {
                        first,
                        second,
                        axis,
                        value: Length::new(value)?,
                    },
                )
                .ok()?,
                name: None,
                driving: None,
                active: Some(equation.active),
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(sketch_native_ref(sketch)),
            },
            equation.offset,
        ))
    })
    .collect()
}

pub(in super::super) fn section_equation_function_thirty_one_point_coordinate_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let coordinates = resolved_section_coordinates(definition);
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().1)
        .unwrap_or_default();
    section_equation_function_thirty_one_point_coordinate_rows(
        definition,
        &coordinates,
        &ambiguous_point_ids,
    )
    .into_iter()
    .filter_map(|equation| {
        let [u, v] = equation.values;
        let (Some(u), Some(v)) = (u, v) else {
            return None;
        };
        if !u.is_finite() || !v.is_finite() {
            return None;
        }
        let point = section_point_locus(definition, sketch, equation.point)?;
        Some((
            SketchConstraint {
                id: sketch_constraint_id(
                    sketch,
                    format_args!("equation:{}", equation.equation_id),
                )?,
                sketch: sketch.clone(),
                definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                    SketchConstraintDefinitionInput::PointCoordinateValues {
                        point,
                        values: [Length::new(u)?, Length::new(v)?],
                    },
                )
                .ok()?,
                name: None,
                driving: None,
                active: Some(equation.active),
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(sketch_native_ref(sketch)),
            },
            equation.offset,
        ))
    })
    .collect()
}

pub(in super::super) fn section_equation_function_sixteen_angle_difference_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    section_equation_function_sixteen_angle_difference_rows(definition)
        .into_iter()
        .filter_map(|equation| {
            Some({
                (
                    SketchConstraint {
                        id: sketch_constraint_id(
                            sketch,
                            format_args!("equation:{}", equation.equation_id),
                        )?,
                        sketch: sketch.clone(),
                        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                            SketchConstraintDefinitionInput::AngleDifference {
                                first: equation.first.1,
                                second: equation.second.1,
                                difference: equation.difference.1,
                                value: Angle::new(equation.value)?,
                            },
                        )
                        .ok()?,
                        name: None,
                        driving: None,
                        active: Some(equation.active),
                        virtual_space: None,
                        visible: None,
                        orientation: None,
                        label_distance: None,
                        label_position: None,
                        metadata: None,
                        native_ref: Some(sketch_native_ref(sketch)),
                    },
                    equation.offset,
                )
            })
        })
        .collect()
}

pub(in super::super) fn section_equation_function_five_scalar_equality_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    section_equation_function_five_scalar_equality_rows(definition)
        .into_iter()
        .filter_map(|equation| {
            Some({
                (
                    SketchConstraint {
                        id: sketch_constraint_id(
                            sketch,
                            format_args!("equation:{}", equation.equation_id),
                        )?,
                        sketch: sketch.clone(),
                        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                            SketchConstraintDefinitionInput::ScalarEquality {
                                first: equation.first.1,
                                second: equation.second.1,
                            },
                        )
                        .ok()?,
                        name: None,
                        driving: None,
                        active: Some(true),
                        virtual_space: None,
                        visible: None,
                        orientation: None,
                        label_distance: None,
                        label_position: None,
                        metadata: None,
                        native_ref: Some(sketch_native_ref(sketch)),
                    },
                    equation.offset,
                )
            })
        })
        .collect()
}

pub(in super::super) fn section_equation_polar_distance_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let coordinates = resolved_section_coordinates(definition);
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().1)
        .unwrap_or_default();
    let dimension_parameters = section_equation_radius_dimension_parameters(definition, sketch);
    section_equation_radial_constraint_rows(definition, &coordinates, &ambiguous_point_ids)
        .into_iter()
        .filter_map(|equation| {
            let distance = equation.radius_value?;
            if !distance.is_finite() || distance < 0.0 {
                return None;
            }
            let angle = if distance <= EPS_POLAR_ZERO {
                None
            } else {
                Some(Angle::new(equation.angle_value?)?)
            };
            let first = section_point_locus(definition, sketch, equation.first)?;
            let second = section_point_locus(definition, sketch, equation.second)?;
            let distance_parameter = section_equation_dimension_parameter(
                &dimension_parameters,
                equation.radius,
                distance,
            );
            Some((
                SketchConstraint {
                    id: sketch_constraint_id(
                        sketch,
                        format_args!("equation:{}", equation.equation_id),
                    )?,
                    sketch: sketch.clone(),
                    definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                        SketchConstraintDefinitionInput::PolarDistance {
                            first,
                            second,
                            distance: Length::new(distance)?,
                            angle,
                            distance_parameter,
                        },
                    )
                    .ok()?,
                    name: None,
                    driving: None,
                    active: Some(equation.active),
                    virtual_space: None,
                    visible: None,
                    orientation: None,
                    label_distance: None,
                    label_position: None,
                    metadata: None,
                    native_ref: Some(sketch_native_ref(sketch)),
                },
                equation.offset,
            ))
        })
        .collect()
}

pub(in super::super) fn section_equation_native_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    typed_offsets: &BTreeSet<usize>,
) -> Vec<(SketchConstraint, usize)> {
    let Some(table) = crate::feature::equation_table(&definition.body, 0, definition.body.len())
    else {
        return Vec::new();
    };
    table
        .rows
        .into_iter()
        .filter(|equation| !typed_offsets.contains(&equation.offset))
        .filter_map(|equation| {
            Some({
                let active = !section_solver_equation_is_disabled(definition, equation.equation_id);
                let native_ref = sketch_native_ref(sketch);
                let argument_slots = equation
                    .arguments
                    .iter()
                    .enumerate()
                    .map(|(slot, argument)| match argument {
                        Some(argument) => format!("{slot}:{argument}"),
                        None => format!("{slot}:null"),
                    })
                    .collect::<Vec<_>>();
                let null_argument_ordinals = equation
                    .arguments
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, argument)| argument.is_none().then_some(slot.to_string()))
                    .collect::<Vec<_>>();
                let mut native_properties = BTreeMap::from([
                    ("equation_id".to_string(), equation.equation_id.to_string()),
                    ("function_id".to_string(), equation.function_id.to_string()),
                    ("offset".to_string(), equation.offset.to_string()),
                    ("table_offset".to_string(), table.offset.to_string()),
                    (
                        "table_declared_count".to_string(),
                        table.declared_count.to_string(),
                    ),
                    ("active".to_string(), active.to_string()),
                    ("argument_slots".to_string(), argument_slots.join(",")),
                ]);
                if let Some(explicit_argument_count) = equation.explicit_argument_count {
                    native_properties.insert(
                        "explicit_argument_count".to_string(),
                        explicit_argument_count.to_string(),
                    );
                }
                if let Some(entity_ref) = table.entity_ref {
                    native_properties
                        .insert("table_entity_ref".to_string(), entity_ref.to_string());
                }
                if !null_argument_ordinals.is_empty() {
                    native_properties.insert(
                        "null_argument_ordinals".to_string(),
                        null_argument_ordinals.join(","),
                    );
                }
                let mut operands = vec![SketchNativeOperand {
                    native_kind: cadmpeg_ir::products::NonEmptyString::new("eqtn_arr")
                        .expect("source operand kind is nonempty"),
                    field: Some(NativeOperandField {
                        name: cadmpeg_ir::products::NonEmptyString::new("equation_id")
                            .expect("source field name is nonempty"),
                        role: None,
                    }),
                    object_index: equation.equation_id,
                    native_ref: Some(native_ref.clone()),
                }];
                operands.extend(equation.arguments.iter().enumerate().filter_map(
                    |(slot, argument)| {
                        argument.map(|object_index| SketchNativeOperand {
                            native_kind: cadmpeg_ir::products::NonEmptyString::new("var_arr")
                                .expect("source operand kind is nonempty"),
                            field: Some(NativeOperandField {
                                name: cadmpeg_ir::products::NonEmptyString::new(format!(
                                    "arguments[{slot}]"
                                ))
                                .expect("source field name is nonempty"),
                                role: None,
                            }),
                            object_index,
                            native_ref: Some(native_ref.clone()),
                        })
                    },
                ));
                (
                    SketchConstraint {
                        id: sketch_constraint_id(
                            sketch,
                            format_args!("equation:offset:{}", equation.offset),
                        )?,
                        sketch: sketch.clone(),
                        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                            SketchConstraintDefinitionInput::Native {
                                native_kind: cadmpeg_ir::products::NonEmptyString::new(format!(
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
                        )
                        .ok()?,
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
                )
            })
        })
        .collect()
}

pub(in super::super) fn section_equation_same_coordinate_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().1)
        .unwrap_or_default();
    let rows = super::super::sketch::section_equation_coordinate_equality_rows(
        definition,
        &ambiguous_point_ids,
    );
    rows.into_iter()
        .filter(|equation| matches!(equation.function_id, 2 | 10 | 13))
        .filter_map(|equation| {
            let first = section_point_locus(definition, sketch, equation.first)?;
            let second = section_point_locus(definition, sketch, equation.second)?;
            let axis = match equation.axis {
                SectionAxis::U => SketchCoordinateAxis::U,
                SectionAxis::V => SketchCoordinateAxis::V,
            };
            Some((
                SketchConstraint {
                    id: sketch_constraint_id(
                        sketch,
                        format_args!("equation:{}", equation.equation_id),
                    )?,
                    sketch: sketch.clone(),
                    definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                        SketchConstraintDefinitionInput::SameCoordinate {
                            relation: cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                                first, second, axis,
                            )
                            .ok()?,
                        },
                    )
                    .ok()?,
                    name: None,
                    driving: None,
                    active: Some(equation.active),
                    virtual_space: None,
                    visible: None,
                    orientation: None,
                    label_distance: None,
                    label_position: None,
                    metadata: None,
                    native_ref: Some(sketch_native_ref(sketch)),
                },
                equation.offset,
            ))
        })
        .collect()
}

pub(in super::super) fn section_equation_point_on_line_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().1)
        .unwrap_or_default();
    let unique_segment_ids = unique_section_segment_external_ids(definition);
    let segments = section_segment_rows(definition);
    section_equation_point_on_line_constraint_rows(definition, &ambiguous_point_ids)
        .into_iter()
        .filter_map(|equation| {
            let point = section_point_locus(definition, sketch, equation.target)?;
            let matching_line_ids = segments
                .iter()
                .filter(|segment| {
                    matches!(segment.kind, crate::feature::FeatureSegmentKind::Line(_))
                        && unique_segment_ids.contains(&segment.external_id)
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
                                && matches!([equation.first, equation.second], [0, 1] | [1, 0])
                        })
                        .map(|segment| segment.external_id),
                )
                .collect::<Vec<_>>();
            let [line_external_id] = matching_line_ids.as_slice() else {
                return None;
            };
            let entity = sketch_entity_id(sketch, *line_external_id)?;
            Some((
                SketchConstraint {
                    id: sketch_constraint_id(
                        sketch,
                        format_args!("equation:{}", equation.equation_id),
                    )?,
                    sketch: sketch.clone(),
                    definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                        SketchConstraintDefinitionInput::PointOnObject { point, entity },
                    )
                    .ok()?,
                    name: None,
                    driving: None,
                    active: Some(equation.active),
                    virtual_space: None,
                    visible: None,
                    orientation: None,
                    label_distance: None,
                    label_position: None,
                    metadata: None,
                    native_ref: Some(sketch_native_ref(sketch)),
                },
                equation.offset,
            ))
        })
        .collect()
}

pub(in super::super) fn section_equation_axis_distance_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let Some(dimensions) = definition.dimensions.as_ref() else {
        return Vec::new();
    };
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().1)
        .unwrap_or_default();
    section_equation_function_forty_three_axis_distance_rows(
        definition,
        &resolved_section_coordinates(definition),
        &ambiguous_point_ids,
    )
    .into_iter()
    .filter_map(|equation| {
        let first = section_point_locus(definition, sketch, equation.first)?;
        let second = section_point_locus(definition, sketch, equation.second)?;
        let (dimension, parameter) = resolved_feature_dimension_parameter(
            sketch,
            dimensions,
            usize::try_from(equation.scalar.1).ok()?,
        )?;
        let dimension_value = dimension.value.resolved()?;
        if !matches!(dimension.dimension_type, 1..=5)
            || !dimension_value.is_finite()
            || dimension_value < 0.0
            || !approximately_equal(dimension_value, equation.value)
        {
            return None;
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
        Some((
            SketchConstraint {
                id: sketch_constraint_id(
                    sketch,
                    format_args!("equation:{}", equation.equation_id),
                )?,
                sketch: sketch.clone(),
                definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
                    .ok()?,
                name: None,
                driving: None,
                active: Some(equation.active),
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(sketch_native_ref(sketch)),
            },
            equation.offset,
        ))
    })
    .collect()
}

pub(in super::super) fn section_equation_unsigned_distance_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let Some(dimensions) = definition.dimensions.as_ref() else {
        return Vec::new();
    };
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().1)
        .unwrap_or_default();
    section_equation_unsigned_coordinate_distance_rows(definition, &ambiguous_point_ids)
        .into_iter()
        .filter_map(|equation| {
            let first = section_point_locus(definition, sketch, equation.first)?;
            let second = section_point_locus(definition, sketch, equation.second)?;
            let parameter = resolved_feature_dimension_parameter(
                sketch,
                dimensions,
                usize::try_from(equation.scalar.1).ok()?,
            )?
            .1;
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
            Some((
                SketchConstraint {
                    id: sketch_constraint_id(
                        sketch,
                        format_args!("equation:{}", equation.equation_id),
                    )?,
                    sketch: sketch.clone(),
                    definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                        definition,
                    )
                    .ok()?,
                    name: None,
                    driving: None,
                    active: Some(equation.active),
                    virtual_space: None,
                    visible: None,
                    orientation: None,
                    label_distance: None,
                    label_position: None,
                    metadata: None,
                    native_ref: Some(sketch_native_ref(sketch)),
                },
                equation.offset,
            ))
        })
        .collect()
}

pub(in super::super) fn circular_dimension_constraint(
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

pub(in super::super) fn native_section_dimension_constraint_definition(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    relation: &crate::feature::FeatureRelation,
) -> Option<SketchConstraintDefinitionInput> {
    let Some(relations) = definition.relations.as_ref() else {
        return Some(SketchConstraintDefinitionInput::Native {
            native_kind: cadmpeg_ir::products::NonEmptyString::new(format!(
                "creo:relation:{}",
                relation.relation_type
            ))?,
            native_state: Some(u64::from(relation.used)),
            native_flags: None,
            native_properties: BTreeMap::from([
                (
                    "dimension_id".to_string(),
                    relation.dimension_id.to_string(),
                ),
                ("sign".to_string(), relation.sign.to_string()),
                ("relation_id".to_string(), relation.relation_id.to_string()),
            ]),
            entities: Vec::new(),
            parameter: None,
            operands: Vec::new(),
        });
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
    let parameter = definition.dimensions.as_ref().and_then(|dimensions| {
        resolved_feature_dimension_parameter(
            sketch,
            dimensions,
            usize::try_from(relation.dimension_id).ok()?,
        )
        .map(|(_, parameter)| parameter)
    });
    let entities = if unique_relation_id {
        joined_relation_incidence_entities(definition, sketch, relation.relation_id)
    } else {
        Vec::new()
    };
    let native_ref = sketch_native_ref(sketch);
    let mut native_properties = BTreeMap::from([
        (
            "dimension_id".to_string(),
            relation.dimension_id.to_string(),
        ),
        ("sign".to_string(), relation.sign.to_string()),
    ]);
    if !unique_relation_id {
        native_properties.insert("relation_id".to_string(), relation.relation_id.to_string());
    }
    let mut operands = Vec::new();
    if unique_relation_id {
        operands.push(SketchNativeOperand {
            native_kind: cadmpeg_ir::products::NonEmptyString::new("relat_ptr")
                .expect("source operand kind is nonempty"),
            field: None,
            object_index: relation.relation_id,
            native_ref: Some(native_ref.clone()),
        });
    }
    if let Some(incidence) = joined_incidence {
        operands.push(SketchNativeOperand {
            native_kind: cadmpeg_ir::products::NonEmptyString::new("skamp_ptr")
                .expect("source operand kind is nonempty"),
            field: Some(NativeOperandField {
                name: cadmpeg_ir::products::NonEmptyString::new("triples_ptr.skamp_id")
                    .expect("source field name is nonempty"),
                role: None,
            }),
            object_index: incidence.id,
            native_ref: Some(native_ref.clone()),
        });
    }
    if let Some(equation_id) = joined_relation_incidence_link.and_then(|(join, _)| join.equation_id)
    {
        operands.push(SketchNativeOperand {
            native_kind: cadmpeg_ir::products::NonEmptyString::new("triples_ptr")
                .expect("source operand kind is nonempty"),
            field: Some(NativeOperandField {
                name: cadmpeg_ir::products::NonEmptyString::new("equation_id")
                    .expect("source field name is nonempty"),
                role: None,
            }),
            object_index: equation_id,
            native_ref: Some(native_ref.clone()),
        });
    }
    if let Some(vectors) = relation.operand_vectors {
        for (vector, values) in ["a", "b", "c"].into_iter().zip(vectors) {
            operands.extend(values.into_iter().enumerate().filter_map(|(slot, value)| {
                value.map(|object_index| SketchNativeOperand {
                    native_kind: cadmpeg_ir::products::NonEmptyString::new("relat_ptr")
                        .expect("source operand kind is nonempty"),
                    field: Some(NativeOperandField {
                        name: cadmpeg_ir::products::NonEmptyString::new(format!(
                            "{vector}[{slot}]"
                        ))
                        .expect("source field name is nonempty"),
                        role: None,
                    }),
                    object_index,
                    native_ref: Some(native_ref.clone()),
                })
            }));
        }
    }
    Some(SketchConstraintDefinitionInput::Native {
        native_kind: cadmpeg_ir::products::NonEmptyString::new(format!(
            "creo:relation:{}",
            relation.relation_type
        ))?,
        native_state: Some(u64::from(relation.used)),
        native_flags: None,
        native_properties,
        entities,
        parameter,
        operands,
    })
}

pub(in super::super) fn reconcile_section_dimension_constraint(
    constraint_definition: &mut SketchConstraintDefinitionInput,
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    relation: &crate::feature::FeatureRelation,
    emitted: &BTreeSet<SketchEntityId>,
    available_parameters: &BTreeSet<ParameterId>,
) -> bool {
    let entity_reconciled = reconcile_constraint_entity_references(constraint_definition, emitted);
    let parameter_reconciled =
        reconcile_constraint_parameter_reference(constraint_definition, available_parameters);
    if entity_reconciled && parameter_reconciled {
        return true;
    }
    let Some(native_definition) =
        native_section_dimension_constraint_definition(definition, sketch, relation)
    else {
        return false;
    };
    *constraint_definition = native_definition;
    reconcile_constraint_entity_references(constraint_definition, emitted)
        && reconcile_constraint_parameter_reference(constraint_definition, available_parameters)
}

pub(in super::super) fn section_dimension_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let Some(relations) = &definition.relations else {
        return Vec::new();
    };
    let segments = section_segment_rows(definition);

    let known_entities = section_entity_external_ids(definition);
    let ambiguous_point_ids = definition
        .variables
        .as_ref()
        .filter(|variables| variables.is_complete())
        .map(|variables| variables.reconciled_points().1)
        .unwrap_or_default();
    let resolved_coordinates = resolved_section_coordinates(definition);
    let saved_coordinate_witnesses =
        saved_section_coordinate_witnesses(definition, &ambiguous_point_ids);
    relations
        .rows
        .iter()
        .filter_map(|relation| {
            Some({
                let unique_relation_id = feature_relation_table_complete(relations)
                    && relations
                        .rows
                        .iter()
                        .filter(|candidate| candidate.relation_id == relation.relation_id)
                        .count()
                        == 1;
                let dimension = definition.dimensions.as_ref().and_then(|dimensions| {
                    resolved_feature_dimension_parameter(
                        sketch,
                        dimensions,
                        usize::try_from(relation.dimension_id).ok()?,
                    )
                });
                let parameter = dimension.as_ref().map(|(_, parameter)| parameter.clone());
                let joined_incidence_link = unique_relation_id
                    .then(|| joined_relation_incidence_link(definition, relation.relation_id))
                    .flatten();
                let joined_incidence = joined_incidence_link.map(|(_, incidence)| incidence);
                let typed = (|| {
                    unique_relation_id.then_some(())?;
                    let (dimension, _) = dimension.as_ref()?;
                    let parameter = parameter.clone()?;
                    if relation.relation_type == 1
                        && dimension.unit() == crate::feature::DimensionUnit::Radians
                    {
                        let [first, second] = section_angular_entities(
                            definition,
                            sketch,
                            &segments,
                            relation.operand_vectors?,
                            &known_entities,
                        )?;
                        return Some(SketchConstraintDefinitionInput::Angle {
                            first,
                            second,
                            parameter,
                        });
                    }
                    if relation.relation_type == 0
                        && matches!(relation.sign, 0 | 1 | 0xf6)
                        && dimension.unit() == crate::feature::DimensionUnit::SchemaDefined
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
                            if matches!(measured.kind, crate::feature::FeatureSegmentKind::Line(_))
                                && (measured.point_ids() == [first_id, second_id]
                                    || measured.point_ids() == [second_id, first_id])
                                && measured.vertical_horizontal == Some(expected_coordinate)
                                && known_entities.contains(&measured.external_id)
                            {
                                let entity = sketch_entity_id(sketch, measured.external_id)?;
                                return Some(if incidence.kind == 1 {
                                    SketchConstraintDefinitionInput::Horizontal { entity }
                                } else {
                                    SketchConstraintDefinitionInput::Vertical { entity }
                                });
                            }
                        }
                    }
                    if dimension.unit() != crate::feature::DimensionUnit::Millimeters {
                        return None;
                    }
                    if matches!(relation.relation_type, 5 | 6) && relation.sign == 1 {
                        let segment = section_radius_relation_arc(definition, relation)?;
                        return Some(circular_dimension_constraint(
                            sketch_entity_id(sketch, segment.external_id)?,
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
                        let matching = segments
                            .iter()
                            .filter(|segment| {
                                matches!(segment.kind, crate::feature::FeatureSegmentKind::Arc(_))
                            })
                            .map(|segment| (segment.external_id, segment.radius_ref))
                            .chain(
                                definition
                                    .segments
                                    .iter()
                                    .flat_map(|table| table.rows.circles())
                                    .map(|segment| (segment.external_id, Some(segment.radius_ref))),
                            )
                            .filter(|(_, radius_ref)| *radius_ref == Some(radius_id))
                            .collect::<Vec<_>>();
                        let [(external_id, _)] = matching.as_slice() else {
                            return None;
                        };
                        known_entities.contains(external_id).then_some(())?;
                        return Some(circular_dimension_constraint(
                            sketch_entity_id(sketch, *external_id)?,
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
                                    first_id,
                                    second_id,
                                    &resolved_coordinates,
                                    &saved_coordinate_witnesses,
                                    &ambiguous_point_ids,
                                );
                                let matching = segments
                                    .iter()
                                    .filter(|segment| {
                                        segment.point_ids() == [first_id, second_id]
                                            || segment.point_ids() == [second_id, first_id]
                                    })
                                    .collect::<Vec<_>>();
                                if let [measured] = matching.as_slice() {
                                    if matches!(
                                        measured.kind,
                                        crate::feature::FeatureSegmentKind::Line(_)
                                    ) && known_entities.contains(&measured.external_id)
                                    {
                                        let entity =
                                            sketch_entity_id(sketch, measured.external_id)?;
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
                                    section_point_locus(definition, sketch, first_id),
                                    section_point_locus(definition, sketch, second_id),
                                ) {
                                    return Some(match coordinate {
                                        SectionAxis::U => {
                                            SketchConstraintDefinitionInput::HorizontalDistance {
                                                first,
                                                second,
                                                parameter,
                                            }
                                        }
                                        SectionAxis::V => SketchConstraintDefinitionInput::VerticalDistance {
                                            first,
                                            second,
                                            parameter,
                                        },
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
                                entities: incidence
                                    .items
                                    .iter()
                                    .filter_map(|item| sketch_entity_id(sketch, item.entity_id))
                                    .collect(),
                                parameter,
                            });
                        }
                    }
                    let entities =
                        relation_incidence_entities(definition, sketch, relation.relation_id);
                    (!entities.is_empty()).then_some(SketchConstraintDefinitionInput::Distance {
                        entities,
                        parameter,
                    })
                })();
                let active =
                    joined_incidence.map(|incidence| section_skamp_active(incidence.status));
                let constraint_definition = typed.or_else(|| {
                    native_section_dimension_constraint_definition(definition, sketch, relation)
                })?;
                (
                    SketchConstraint {
                        id: if unique_relation_id {
                            sketch_constraint_id(
                                sketch,
                                format_args!("relation:{}", relation.relation_id),
                            )?
                        } else {
                            sketch_constraint_id(
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
                        native_ref: Some(sketch_native_ref(sketch)),
                    },
                    relation.offset,
                )
            })
        })
        .collect()
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
        reconcile_section_dimension_constraint,
        section_equation_function_five_scalar_equality_constraints,
        section_equation_function_sixteen_angle_difference_constraints,
    };
    use cadmpeg_ir::features::ParameterId;
    use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntityId, SketchId};
    use std::collections::BTreeSet;

    #[test]
    fn direct_angle_difference_transfers_solver_scalar_operands() {
        let row = |variable_type, key, value: Option<f64>| crate::feature::FeatureVariableRow {
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
        };
        let definition = crate::feature::FeatureDefinition {
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
            variables: Some(crate::feature::FeatureVariableTable {
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
        let constraints =
            section_equation_function_sixteen_angle_difference_constraints(&definition, &sketch);
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
        let row = |variable_type, key, value: Option<f64>| crate::feature::FeatureVariableRow {
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
        };
        let definition = crate::feature::FeatureDefinition {
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
            variables: Some(crate::feature::FeatureVariableTable {
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
            section_equation_function_five_scalar_equality_constraints(&definition, &sketch);
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
        assert!(
            section_equation_function_five_scalar_equality_constraints(&conflicting, &sketch,)
                .is_empty()
        );
    }

    #[test]
    fn typed_dimension_relation_falls_back_to_native_when_references_are_not_emitted() {
        let relation = crate::feature::FeatureRelation {
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
        let definition = crate::feature::FeatureDefinition {
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
            relations: Some(crate::feature::FeatureRelationTable {
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
            &BTreeSet::new(),
        ));
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
            &BTreeSet::new(),
        ));
        assert!(matches!(
            missing_parameter,
            SketchConstraintDefinitionInput::Native {
                native_kind,
                ..
            } if native_kind == "creo:relation:0"
        ));
    }
}
