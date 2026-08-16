// SPDX-License-Identifier: Apache-2.0
//! Section constraint reconciliation, incidence, and dimension emission.

use super::super::feature_history::{
    feature_relation_table_complete, feature_solver_table_complete,
    resolved_feature_dimension_parameter,
};
use super::super::sketch::{
    resolved_section_coordinates, section_line_fixed_coordinate,
    section_linear_distance_coordinate, section_segment_rows, section_type5_radius_arc,
    unique_section_skamp_segment,
};
use super::super::sketch_ids::{sketch_constraint_id, sketch_entity_id, sketch_native_ref};
use super::{
    opaque_section_segment_identity_suffix, section_entity_external_ids, section_point_locus,
    section_segment_identity_suffix, section_skamp_active, section_skamp_locus,
    unique_section_segment_external_ids,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinition, SketchCoordinateAxis, SketchDistancePair,
    SketchEntityId, SketchId, SketchLocus, SketchNativeOperand,
};
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn section_segment_verhor_definition(
    segment: &crate::feature::FeatureSegment,
    sketch: &SketchId,
    entity: SketchEntityId,
) -> Option<SketchConstraintDefinition> {
    let verhor = segment.vertical_horizontal?;
    match (segment.kind, verhor) {
        (crate::feature::FeatureSegmentKind::Line, 0) => {
            Some(SketchConstraintDefinition::Vertical { entity })
        }
        (crate::feature::FeatureSegmentKind::Line, 1) => {
            Some(SketchConstraintDefinition::Horizontal { entity })
        }
        _ => Some(native_section_segment_verhor_definition(
            sketch,
            entity,
            segment.external_id,
            verhor,
        )),
    }
}

pub(in super::super) fn native_section_segment_verhor_definition(
    sketch: &SketchId,
    entity: SketchEntityId,
    external_id: u32,
    verhor: u32,
) -> SketchConstraintDefinition {
    SketchConstraintDefinition::Native {
        native_kind: "creo:segtab:verhor".to_string(),
        native_state: None,
        native_flags: None,
        native_properties: BTreeMap::from([("verhor".to_string(), verhor.to_string())]),
        entities: vec![entity],
        parameter: None,
        operands: vec![SketchNativeOperand {
            native_kind: "segtab_ptr".to_string(),
            native_field: Some("ext_id".to_string()),
            native_role: None,
            object_index: external_id,
            native_ref: Some(sketch_native_ref(sketch)),
        }],
    }
}

pub(in super::super) fn reconcile_constraint_entity_references(
    definition: &mut SketchConstraintDefinition,
    emitted: &BTreeSet<SketchEntityId>,
) -> bool {
    let locus_emitted = |locus: &SketchLocus| match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => emitted.contains(entity),
    };
    match definition {
        SketchConstraintDefinition::Native { entities, .. } => {
            entities.retain(|entity| emitted.contains(entity));
            true
        }
        SketchConstraintDefinition::Coincident { entities }
        | SketchConstraintDefinition::Distance { entities, .. } => {
            entities.iter().all(|entity| emitted.contains(entity))
        }
        SketchConstraintDefinition::CoincidentLoci { loci } => loci.iter().all(locus_emitted),
        SketchConstraintDefinition::SameCoordinate { first, second, .. }
        | SketchConstraintDefinition::TangentLoci { first, second }
        | SketchConstraintDefinition::DistanceLoci { first, second, .. }
        | SketchConstraintDefinition::HorizontalDistance { first, second, .. }
        | SketchConstraintDefinition::VerticalDistance { first, second, .. } => {
            locus_emitted(first) && locus_emitted(second)
        }
        SketchConstraintDefinition::EqualDistance { first, second } => {
            locus_emitted(&first.first)
                && locus_emitted(&first.second)
                && locus_emitted(&second.first)
                && locus_emitted(&second.second)
        }
        SketchConstraintDefinition::Midpoint { point, entity } => {
            locus_emitted(point) && emitted.contains(entity)
        }
        SketchConstraintDefinition::AtIntersection {
            point,
            first,
            second,
        } => locus_emitted(point) && emitted.contains(first) && emitted.contains(second),
        SketchConstraintDefinition::PointOnObject { point, entity } => {
            locus_emitted(point) && emitted.contains(entity)
        }
        SketchConstraintDefinition::Symmetric {
            first,
            second,
            axis,
        } => locus_emitted(first) && locus_emitted(second) && emitted.contains(axis),
        SketchConstraintDefinition::PointSymmetric {
            first,
            second,
            center,
        } => locus_emitted(first) && locus_emitted(second) && locus_emitted(center),
        SketchConstraintDefinition::Concentric { first, second }
        | SketchConstraintDefinition::Coradial { first, second }
        | SketchConstraintDefinition::Collinear { first, second }
        | SketchConstraintDefinition::ProjectedCopy {
            source: first,
            result: second,
        }
        | SketchConstraintDefinition::Parallel { first, second }
        | SketchConstraintDefinition::Perpendicular { first, second }
        | SketchConstraintDefinition::Tangent { first, second }
        | SketchConstraintDefinition::Equal { first, second }
        | SketchConstraintDefinition::Angle { first, second, .. } => {
            emitted.contains(first) && emitted.contains(second)
        }
        SketchConstraintDefinition::Horizontal { entity }
        | SketchConstraintDefinition::Vertical { entity }
        | SketchConstraintDefinition::Fixed { entity }
        | SketchConstraintDefinition::Radius { entity, .. }
        | SketchConstraintDefinition::Diameter { entity, .. } => emitted.contains(entity),
        SketchConstraintDefinition::HorizontalPoints { first, second }
        | SketchConstraintDefinition::VerticalPoints { first, second }
        | SketchConstraintDefinition::HorizontalLoci { first, second }
        | SketchConstraintDefinition::VerticalLoci { first, second } => {
            locus_emitted(first) && locus_emitted(second)
        }
        SketchConstraintDefinition::ArcAngle { entity, .. }
        | SketchConstraintDefinition::EllipseAngle { entity, .. } => emitted.contains(entity),
        SketchConstraintDefinition::SnellsLaw {
            incident,
            refracted,
            interface,
            ..
        } => locus_emitted(incident) && locus_emitted(refracted) && emitted.contains(interface),
        SketchConstraintDefinition::Weight { entity, .. } => emitted.contains(entity),
        SketchConstraintDefinition::InternalAlignment { helper, parent, .. } => {
            emitted.contains(helper) && emitted.contains(parent)
        }
        SketchConstraintDefinition::Group { elements }
        | SketchConstraintDefinition::Text { elements, .. } => elements.iter().all(locus_emitted),
        SketchConstraintDefinition::Disabled => true,
        _ => true,
    }
}

pub(in super::super) fn reconcile_constraint_parameter_reference(
    definition: &mut SketchConstraintDefinition,
    emitted: &BTreeSet<ParameterId>,
) -> bool {
    match definition {
        SketchConstraintDefinition::Native { parameter, .. } => {
            if parameter
                .as_ref()
                .is_some_and(|parameter| !emitted.contains(parameter))
            {
                *parameter = None;
            }
            true
        }
        SketchConstraintDefinition::Distance { parameter, .. }
        | SketchConstraintDefinition::DistanceLoci { parameter, .. }
        | SketchConstraintDefinition::HorizontalDistance { parameter, .. }
        | SketchConstraintDefinition::VerticalDistance { parameter, .. }
        | SketchConstraintDefinition::Angle { parameter, .. }
        | SketchConstraintDefinition::Radius { parameter, .. }
        | SketchConstraintDefinition::Diameter { parameter, .. } => emitted.contains(parameter),
        SketchConstraintDefinition::SnellsLaw { parameter, .. }
        | SketchConstraintDefinition::Weight { parameter, .. } => emitted.contains(parameter),
        SketchConstraintDefinition::Coincident { .. }
        | SketchConstraintDefinition::CoincidentLoci { .. }
        | SketchConstraintDefinition::SameCoordinate { .. }
        | SketchConstraintDefinition::Midpoint { .. }
        | SketchConstraintDefinition::Concentric { .. }
        | SketchConstraintDefinition::Coradial { .. }
        | SketchConstraintDefinition::Collinear { .. }
        | SketchConstraintDefinition::Symmetric { .. }
        | SketchConstraintDefinition::PointSymmetric { .. }
        | SketchConstraintDefinition::Horizontal { .. }
        | SketchConstraintDefinition::Vertical { .. }
        | SketchConstraintDefinition::Parallel { .. }
        | SketchConstraintDefinition::Perpendicular { .. }
        | SketchConstraintDefinition::Tangent { .. }
        | SketchConstraintDefinition::TangentLoci { .. }
        | SketchConstraintDefinition::Equal { .. }
        | SketchConstraintDefinition::EqualDistance { .. }
        | SketchConstraintDefinition::Fixed { .. } => true,
        SketchConstraintDefinition::Disabled
        | SketchConstraintDefinition::PointOnObject { .. }
        | SketchConstraintDefinition::AtIntersection { .. }
        | SketchConstraintDefinition::HorizontalPoints { .. }
        | SketchConstraintDefinition::VerticalPoints { .. }
        | SketchConstraintDefinition::ArcAngle { .. }
        | SketchConstraintDefinition::EllipseAngle { .. }
        | SketchConstraintDefinition::InternalAlignment { .. }
        | SketchConstraintDefinition::Group { .. }
        | SketchConstraintDefinition::Text { .. } => true,
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
        reconcile_constraint_parameter_reference(&mut constraint.definition, &emitted)
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
    if !feature_solver_table_complete(relations.triples_header.as_ref(), relations.triples.len())
        || !feature_solver_table_complete(relations.skamp_header.as_ref(), relations.skamps.len())
    {
        return None;
    }
    let joins = relations
        .triples
        .iter()
        .filter(|triple| triple.relation_id == Some(relation_id))
        .filter_map(|triple| triple.skamp_id.map(|incidence_id| (triple, incidence_id)))
        .collect::<Vec<_>>();
    let [(join, incidence_id)] = joins.as_slice() else {
        return None;
    };
    let incidences = relations
        .skamps
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
    if !feature_solver_table_complete(relations.triples_header.as_ref(), relations.triples.len())
        || !feature_solver_table_complete(relations.skamp_header.as_ref(), relations.skamps.len())
    {
        return false;
    }
    let incidence_ids = relations
        .triples
        .iter()
        .filter(|triple| triple.equation_id == Some(equation_id))
        .filter_map(|triple| triple.skamp_id)
        .collect::<Vec<_>>();
    let [incidence_id] = incidence_ids.as_slice() else {
        return false;
    };
    let incidences = relations
        .skamps
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
        .map(|item| sketch_entity_id(sketch, item.entity_id))
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
        .map(|item| sketch_entity_id(sketch, item.entity_id))
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
    segments: &[crate::feature::FeatureSegment],
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
                    && segment.kind == crate::feature::FeatureSegmentKind::Line
            })
            .collect::<Vec<_>>();
        (known_entities.contains(&external_id) && matching_segments.len() == 1)
            .then_some(external_id)
    };
    let [first, second] = [first_internal, second_internal].map(external_id);
    let [Some(first), Some(second)] = [first, second] else {
        return None;
    };
    (first != second)
        .then(|| [first, second].map(|external_id| sketch_entity_id(sketch, external_id)))
}

pub(in super::super) fn native_section_segment_radius_definition(
    sketch: &SketchId,
    entity: SketchEntityId,
    external_id: u32,
    field: &str,
    dimension_ordinal: u32,
) -> SketchConstraintDefinition {
    SketchConstraintDefinition::Native {
        native_kind: format!("creo:segtab:{field}"),
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
                native_kind: "segtab_ptr".to_string(),
                native_field: Some("ext_id".to_string()),
                native_role: None,
                object_index: external_id,
                native_ref: Some(sketch_native_ref(sketch)),
            },
            SketchNativeOperand {
                native_kind: "dimension_ordinal".to_string(),
                native_field: Some(field.to_string()),
                native_role: None,
                object_index: dimension_ordinal,
                native_ref: Some(sketch_native_ref(sketch)),
            },
        ],
    }
}

pub(in super::super) fn section_segment_radius_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let unique_segment_ids = unique_section_segment_external_ids(definition);
    let typed = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .flat_map(|segment| {
            let suffix = section_segment_identity_suffix(&unique_segment_ids, segment);
            [
                ("radius", segment.radius_ref),
                ("radius2", segment.radius2_ref),
            ]
            .into_iter()
            .filter_map(move |(field, ordinal)| {
                Some((
                    suffix.clone(),
                    segment.external_id,
                    field,
                    ordinal?,
                    segment.offset,
                    None,
                ))
            })
        });
    let opaque = definition
        .segments
        .iter()
        .flat_map(|table| &table.opaque_rows)
        .flat_map(|segment| {
            let suffix = opaque_section_segment_identity_suffix(&unique_segment_ids, segment);
            [
                ("radius", segment.radius_ref),
                ("radius2", segment.radius2_ref),
            ]
            .into_iter()
            .filter_map(move |(field, ordinal)| {
                Some((
                    suffix.clone(),
                    segment.external_id,
                    field,
                    ordinal?,
                    segment.offset,
                    None,
                ))
            })
        });
    let circles = definition
        .segments
        .iter()
        .flat_map(|table| &table.circle_rows)
        .map(|segment| {
            let suffix = if unique_segment_ids.contains(&segment.external_id) {
                segment.external_id.to_string()
            } else {
                format!("circle:offset:{}", segment.offset)
            };
            let parameter = usize::try_from(segment.radius_ref)
                .ok()
                .and_then(|ordinal| {
                    resolved_feature_dimension_parameter(
                        sketch,
                        definition.dimensions.as_ref()?,
                        ordinal,
                    )
                });
            (
                suffix,
                segment.external_id,
                "radius",
                segment.radius_ref,
                segment.offset,
                parameter,
            )
        });
    typed
        .chain(circles)
        .chain(opaque)
        .map(
            |(suffix, external_id, field, ordinal, offset, typed_circle)| {
                let entity = sketch_entity_id(sketch, &suffix);
                let (definition, kind) = match typed_circle {
                    Some((dimension, parameter)) if matches!(dimension.dimension_type, 3 | 4) => (
                        circular_dimension_constraint(entity, parameter, dimension.dimension_type),
                        if dimension.dimension_type == 4 {
                            "diameter"
                        } else {
                            "radius"
                        },
                    ),
                    _ => (
                        native_section_segment_radius_definition(
                            sketch,
                            entity,
                            external_id,
                            field,
                            ordinal,
                        ),
                        if field == "radius2" {
                            "segtab-radius2"
                        } else {
                            "segtab-radius"
                        },
                    ),
                };
                (
                    SketchConstraint {
                        id: sketch_constraint_id(sketch, format_args!("{kind}:{suffix}")),
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
                        native_ref: Some(sketch_native_ref(sketch)),
                    },
                    offset,
                )
            },
        )
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
                id: sketch_constraint_id(sketch, format_args!("equation:{}", equation.equation_id)),
                sketch: sketch.clone(),
                definition: SketchConstraintDefinition::EqualDistance { first, second },
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
        .filter(|equation| equation.function_id == 13)
        .filter_map(|equation| {
            let first = section_point_locus(definition, sketch, equation.first)?;
            let second = section_point_locus(definition, sketch, equation.second)?;
            let axis = match equation.axis {
                0 => SketchCoordinateAxis::U,
                1 => SketchCoordinateAxis::V,
                _ => return None,
            };
            Some((
                SketchConstraint {
                    id: sketch_constraint_id(
                        sketch,
                        format_args!("equation:{}", equation.equation_id),
                    ),
                    sketch: sketch.clone(),
                    definition: SketchConstraintDefinition::SameCoordinate {
                        first,
                        second,
                        axis,
                    },
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
) -> SketchConstraintDefinition {
    if dimension_type == 4 {
        SketchConstraintDefinition::Diameter { entity, parameter }
    } else {
        SketchConstraintDefinition::Radius { entity, parameter }
    }
}

pub(in super::super) fn section_dimension_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let Some(relations) = &definition.relations else {
        return Vec::new();
    };
    let segments = section_segment_rows(definition);
    let segment_refs = segments.iter().collect::<Vec<_>>();
    let known_entities = section_entity_external_ids(definition);
    relations
        .rows
        .iter()
        .map(|relation| {
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
                    && dimension.value_unit == crate::feature::DimensionUnit::Radians
                {
                    let [first, second] = section_angular_entities(
                        definition,
                        sketch,
                        segments,
                        relation.operand_vectors?,
                        &known_entities,
                    )?;
                    return Some(SketchConstraintDefinition::Angle {
                        first,
                        second,
                        parameter,
                    });
                }
                if relation.relation_type == 0
                    && matches!(relation.sign, 0 | 1 | 0xf6)
                    && dimension.value_unit == crate::feature::DimensionUnit::SchemaDefined
                    && dimension.value == Some(0.0)
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
                        let measured = unique_section_skamp_segment(definition, item.entity_id)?;
                        if measured.kind == crate::feature::FeatureSegmentKind::Line
                            && (measured.point_ids == [first_id, second_id]
                                || measured.point_ids == [second_id, first_id])
                            && measured.vertical_horizontal == Some(expected_coordinate)
                            && known_entities.contains(&measured.external_id)
                        {
                            let entity = sketch_entity_id(sketch, measured.external_id);
                            return Some(if incidence.kind == 1 {
                                SketchConstraintDefinition::Horizontal { entity }
                            } else {
                                SketchConstraintDefinition::Vertical { entity }
                            });
                        }
                    }
                }
                if dimension.value_unit != crate::feature::DimensionUnit::Millimeters {
                    return None;
                }
                if relation.relation_type == 5 && relation.sign == 1 {
                    let segment = section_type5_radius_arc(definition, relation)?;
                    return Some(circular_dimension_constraint(
                        sketch_entity_id(sketch, segment.external_id),
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
                        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
                        .map(|segment| (segment.external_id, segment.radius_ref))
                        .chain(
                            definition
                                .segments
                                .iter()
                                .flat_map(|table| &table.circle_rows)
                                .map(|segment| (segment.external_id, Some(segment.radius_ref))),
                        )
                        .filter(|(_, radius_ref)| *radius_ref == Some(radius_id))
                        .collect::<Vec<_>>();
                    let [(external_id, _)] = matching.as_slice() else {
                        return None;
                    };
                    known_entities.contains(external_id).then_some(())?;
                    return Some(circular_dimension_constraint(
                        sketch_entity_id(sketch, *external_id),
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
                            let matching = segments
                                .iter()
                                .filter(|segment| {
                                    segment.point_ids == [first_id, second_id]
                                        || segment.point_ids == [second_id, first_id]
                                })
                                .collect::<Vec<_>>();
                            if let [measured] = matching.as_slice() {
                                if known_entities.contains(&measured.external_id) {
                                    let entity = sketch_entity_id(sketch, measured.external_id);
                                    let [first, second] =
                                        if measured.point_ids == [first_id, second_id] {
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
                                    match section_line_fixed_coordinate(definition, measured) {
                                        Some(0) => {
                                            return Some(
                                                SketchConstraintDefinition::VerticalDistance {
                                                    first,
                                                    second,
                                                    parameter,
                                                },
                                            );
                                        }
                                        Some(1) => {
                                            return Some(
                                                SketchConstraintDefinition::HorizontalDistance {
                                                    first,
                                                    second,
                                                    parameter,
                                                },
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            let coordinate = section_linear_distance_coordinate(
                                definition,
                                &segment_refs,
                                first_id,
                                second_id,
                                &resolved_section_coordinates(definition),
                                &[],
                                &BTreeSet::new(),
                            );
                            if let (Some(coordinate), Some(first), Some(second)) = (
                                coordinate,
                                section_point_locus(definition, sketch, first_id),
                                section_point_locus(definition, sketch, second_id),
                            ) {
                                return Some(match coordinate {
                                    0 => SketchConstraintDefinition::HorizontalDistance {
                                        first,
                                        second,
                                        parameter,
                                    },
                                    1 => SketchConstraintDefinition::VerticalDistance {
                                        first,
                                        second,
                                        parameter,
                                    },
                                    _ => return None,
                                });
                            }
                        }
                    }
                }
                if let Some([first, second]) =
                    relation_incidence_loci(definition, sketch, relation.relation_id)
                {
                    return Some(SketchConstraintDefinition::DistanceLoci {
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
                            return Some(SketchConstraintDefinition::DistanceLoci {
                                first,
                                second,
                                parameter,
                            });
                        }
                    }
                    if !incidence.items.is_empty() {
                        return Some(SketchConstraintDefinition::Distance {
                            entities: incidence
                                .items
                                .iter()
                                .map(|item| sketch_entity_id(sketch, item.entity_id))
                                .collect(),
                            parameter,
                        });
                    }
                }
                let entities =
                    relation_incidence_entities(definition, sketch, relation.relation_id);
                (!entities.is_empty()).then_some(SketchConstraintDefinition::Distance {
                    entities,
                    parameter,
                })
            })();
            let incidence_entities = if unique_relation_id {
                joined_relation_incidence_entities(definition, sketch, relation.relation_id)
            } else {
                Vec::new()
            };
            let active = joined_incidence.map(|incidence| section_skamp_active(incidence.status));
            let constraint_definition =
                typed.unwrap_or_else(|| SketchConstraintDefinition::Native {
                    native_kind: format!("creo:relation:{}", relation.relation_type),
                    native_state: Some(u64::from(relation.used)),
                    native_flags: None,
                    native_properties: {
                        let mut properties = BTreeMap::from([
                            (
                                "dimension_id".to_string(),
                                relation.dimension_id.to_string(),
                            ),
                            ("sign".to_string(), relation.sign.to_string()),
                        ]);
                        if !unique_relation_id {
                            properties.insert(
                                "relation_id".to_string(),
                                relation.relation_id.to_string(),
                            );
                        }
                        properties
                    },
                    entities: incidence_entities,
                    parameter,
                    operands: {
                        let native_ref = sketch_native_ref(sketch);
                        let mut operands = Vec::new();
                        if unique_relation_id {
                            operands.push(SketchNativeOperand {
                                native_kind: "relat_ptr".to_string(),
                                native_field: None,
                                native_role: None,
                                object_index: relation.relation_id,
                                native_ref: Some(native_ref.clone()),
                            });
                        }
                        if let Some(incidence) = joined_incidence {
                            operands.push(SketchNativeOperand {
                                native_kind: "skamp_ptr".to_string(),
                                native_field: Some("triples_ptr.skamp_id".to_string()),
                                native_role: None,
                                object_index: incidence.id,
                                native_ref: Some(native_ref.clone()),
                            });
                        }
                        if let Some(equation_id) =
                            joined_incidence_link.and_then(|(join, _)| join.equation_id)
                        {
                            operands.push(SketchNativeOperand {
                                native_kind: "triples_ptr".to_string(),
                                native_field: Some("equation_id".to_string()),
                                native_role: None,
                                object_index: equation_id,
                                native_ref: Some(native_ref.clone()),
                            });
                        }
                        if let Some(vectors) = relation.operand_vectors {
                            for (vector, values) in ["a", "b", "c"].into_iter().zip(vectors) {
                                operands.extend(values.into_iter().enumerate().filter_map(
                                    |(slot, value)| {
                                        value.map(|object_index| SketchNativeOperand {
                                            native_kind: "relat_ptr".to_string(),
                                            native_field: Some(format!("{vector}[{slot}]")),
                                            native_role: None,
                                            object_index,
                                            native_ref: Some(native_ref.clone()),
                                        })
                                    },
                                ));
                            }
                        }
                        operands
                    },
                });
            (
                SketchConstraint {
                    id: if unique_relation_id {
                        sketch_constraint_id(
                            sketch,
                            format_args!("relation:{}", relation.relation_id),
                        )
                    } else {
                        sketch_constraint_id(
                            sketch,
                            format_args!("relation:offset:{}", relation.offset),
                        )
                    },
                    sketch: sketch.clone(),
                    definition: constraint_definition,
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
