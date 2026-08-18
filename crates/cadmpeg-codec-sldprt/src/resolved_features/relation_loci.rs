//! Relation definition and profile locus resolution.

use super::markers::marker_is_geometry_locus;
use super::relation_geometry::{relation_operand_geometry_ref, solver_line_geometry_ref};
use super::transforms::{
    compatible_marker_transform_candidates, locus_entity, locus_key, marker_entities,
    marker_transforms_with_frame_fallback, quantize, sketch_entity_loci, MarkerTransform,
};
use super::typed_relations::{
    line_endpoint_markers, relation_link_identifies_owner, relation_link_is_geometric_operand,
    relation_owner_markers, sketch_entity_contains_point,
};
use super::SKETCH_POINT_TOLERANCE;
use crate::records::{
    FeatureInputLane, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchLocus,
};
use std::collections::{HashMap, HashSet};

pub(super) fn linked_single_arc_entity(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchEntityId> {
    let links = marker
        .links
        .iter()
        .filter(|link| !relation_link_identifies_owner(marker, link))
        .collect::<Vec<_>>();
    if links.is_empty()
        || links.iter().any(|link| {
            !matches!(
                markers_by_id
                    .get(link.entity_ref.as_str())
                    .map(|marker| marker.kind),
                Some(SketchInputKind::Arc)
            )
        })
    {
        return None;
    }
    let entities = linked_single_entities(marker, markers_by_id, loci_by_marker)?;
    let [entity] = entities.as_slice() else {
        return None;
    };
    Some(entity.clone())
}

pub(super) fn linked_single_ellipse_entity(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let entities = linked_single_entities(marker, markers_by_id, loci_by_marker)?;
    let [entity] = entities.as_slice() else {
        return None;
    };
    sketch_entities
        .iter()
        .find(|candidate| candidate.id == *entity)
        .filter(|candidate| matches!(candidate.geometry, SketchGeometry::Ellipse { .. }))?;
    Some(entity.clone())
}

pub(super) fn linked_midpoint_operands(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<(SketchLocus, SketchEntityId)> {
    let links = marker
        .links
        .iter()
        .filter(|link| !relation_link_identifies_owner(marker, link))
        .collect::<Vec<_>>();
    let [first, second] = links.as_slice() else {
        return None;
    };
    let mut point = None;
    let mut entity = None;
    for link in [*first, *second] {
        let linked_marker = markers_by_id.get(link.entity_ref.as_str())?;
        let locus = unique_locus(loci_by_marker.get(&link.entity_ref)?)?;
        match linked_marker.kind {
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint if point.is_none() => {
                point = Some(locus);
            }
            SketchInputKind::LineOrCircle | SketchInputKind::Arc if entity.is_none() => {
                entity = Some(locus_entity(&locus));
            }
            _ => return None,
        }
    }
    Some((point?, entity?))
}

pub(super) fn relation_operand_loci(
    relation: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<Vec<SketchLocus>> {
    let owners = relation_owner_markers(relation, markers_by_id);
    let loci = relation
        .links
        .iter()
        .filter(|link| relation_link_is_geometric_operand(relation, link, markers_by_id))
        .map(|link| link.entity_ref.as_str())
        .chain(owners.iter().map(|owner| owner.id.as_str()))
        .map(|marker| marker_point_locus(marker, markers_by_id, loci_by_marker))
        .collect::<Option<Vec<_>>>()?;
    let loci = loci.into_iter().fold(Vec::new(), |mut unique, locus| {
        if !unique.contains(&locus) {
            unique.push(locus);
        }
        unique
    });
    (!loci.is_empty()).then_some(loci)
}

pub(super) fn linked_single_entities(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<Vec<SketchEntityId>> {
    let mut result = Vec::new();
    for link in marker
        .links
        .iter()
        .filter(|link| !relation_link_identifies_owner(marker, link))
    {
        let entities = marker_entities(&link.entity_ref, markers_by_id, loci_by_marker);
        let [entity] = entities.as_slice() else {
            return None;
        };
        if !result.contains(entity) {
            result.push(entity.clone());
        }
    }
    Some(result)
}

pub(super) fn relation_constraint_is_inactive(
    parameter: Option<&cadmpeg_ir::features::DesignParameter>,
    definition: &SketchConstraintDefinition,
    sketch_entities: &[SketchEntity],
) -> bool {
    let Some(parameter) = parameter else {
        return false;
    };
    let entity = |id: &SketchEntityId| sketch_entities.iter().find(|entity| entity.id == *id);
    match definition {
        SketchConstraintDefinition::DistanceLoci { first, second, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return true;
            };
            let measured = if let (Some(first), Some(second)) = (
                profile_locus_point(first, sketch_entities),
                profile_locus_point(second, sketch_entities),
            ) {
                Some((second.u - first.u).hypot(second.v - first.v))
            } else {
                let point_line = |point: &SketchLocus, line: &SketchLocus| {
                    let point = profile_locus_point(point, sketch_entities)?;
                    let SketchLocus::Entity(line) = line else {
                        return None;
                    };
                    point_line_distance_value(point, entity(line)?)
                };
                point_line(first, second).or_else(|| point_line(second, first))
            };
            measured.is_some_and(|measured| !same_dimension_length(measured, expected.0))
        }
        SketchConstraintDefinition::HorizontalDistance { first, second, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return true;
            };
            let (Some(first), Some(second)) = (
                profile_locus_point(first, sketch_entities),
                profile_locus_point(second, sketch_entities),
            ) else {
                return false;
            };
            !same_dimension_length((second.u - first.u).abs(), expected.0)
        }
        SketchConstraintDefinition::VerticalDistance { first, second, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return true;
            };
            let (Some(first), Some(second)) = (
                profile_locus_point(first, sketch_entities),
                profile_locus_point(second, sketch_entities),
            ) else {
                return false;
            };
            !same_dimension_length((second.v - first.v).abs(), expected.0)
        }
        SketchConstraintDefinition::Distance { entities, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return true;
            };
            let [first, second] = entities.as_slice() else {
                return true;
            };
            line_line_distance(
                match entity(first) {
                    Some(entity) => entity,
                    None => return false,
                },
                match entity(second) {
                    Some(entity) => entity,
                    None => return false,
                },
            )
            .is_some_and(|measured| !same_dimension_length(measured, expected.0))
        }
        SketchConstraintDefinition::Angle { first, second, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Angle(expected)) =
                parameter.value.as_ref()
            else {
                return true;
            };
            line_line_angle(
                match entity(first) {
                    Some(entity) => entity,
                    None => return false,
                },
                match entity(second) {
                    Some(entity) => entity,
                    None => return false,
                },
            )
            .is_some_and(|measured| !same_dimension_angle(measured, expected.0))
        }
        SketchConstraintDefinition::Radius { entity: id, .. }
        | SketchConstraintDefinition::Diameter { entity: id, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return true;
            };
            let Some(entity) = entity(id) else {
                return false;
            };
            let radius = match &entity.geometry {
                SketchGeometry::Circle { radius, .. } | SketchGeometry::Arc { radius, .. } => {
                    radius.0
                }
                _ => return true,
            };
            let measured = if matches!(definition, SketchConstraintDefinition::Diameter { .. }) {
                radius * 2.0
            } else {
                radius
            };
            !same_dimension_length(measured, expected.0)
        }
        SketchConstraintDefinition::RepeatedRadius { entities, .. }
        | SketchConstraintDefinition::RepeatedDiameter { entities, .. } => {
            let Some(cadmpeg_ir::features::ParameterValue::Length(expected)) =
                parameter.value.as_ref()
            else {
                return true;
            };
            let diameter = matches!(
                definition,
                SketchConstraintDefinition::RepeatedDiameter { .. }
            );
            let Some(radii) = entities
                .iter()
                .map(entity)
                .map(|entity| {
                    let entity = entity?;
                    match entity.geometry {
                        SketchGeometry::Circle { radius, .. }
                        | SketchGeometry::Arc { radius, .. } => Some(radius.0),
                        _ => None,
                    }
                })
                .collect::<Option<Vec<_>>>()
            else {
                return false;
            };
            !radii.into_iter().all(|radius| {
                let measured = if diameter { radius * 2.0 } else { radius };
                same_dimension_length(measured, expected.0)
            })
        }
        _ => false,
    }
}

pub(super) fn typed_relation_definition(
    relation: &FeatureInputRelationInstance,
    parameter: Option<&cadmpeg_ir::features::DesignParameter>,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchConstraintDefinition> {
    use FeatureInputRelationFamily::{
        Angle, CircleDiameter, LineLineDistance, PointLineDistance, PointPointDistance,
        PointPointHorizontalDistance, PointPointVerticalDistance,
    };
    let parameter = parameter?;
    let parameter_id = parameter.id.clone();
    let marker = |index: usize| relation_operand_marker(relation, index, sketch, markers_by_id);
    let point = |index: usize| {
        let scoped_ref = relation_operand_geometry_ref(relation, index);
        sketch_entities
            .iter()
            .find(|entity| entity.geometry_ref.as_deref() == Some(scoped_ref.as_str()))
            .filter(|entity| matches!(entity.geometry, SketchGeometry::Point { .. }))
            .map(|entity| SketchLocus::Entity(entity.id.clone()))
            .or_else(|| {
                let marker = marker(index)?;
                if let Some(entity) = sketch_entities.iter().find(|entity| {
                    entity.native_ref.as_deref() == Some(marker)
                        && matches!(entity.geometry, SketchGeometry::Point { .. })
                }) {
                    return Some(SketchLocus::Entity(entity.id.clone()));
                }
                if matches!(
                    relation.operands.get(index).map(|operand| operand.kind),
                    Some(FeatureInputOperandKind::Native(0x837b | 0xbc7c))
                ) {
                    loci_by_marker
                        .get(&qualified_point_marker_key(marker))
                        .and_then(|loci| unique_locus(loci))
                } else {
                    marker_point_locus(marker, markers_by_id, loci_by_marker)
                }
            })
    };
    match relation.family {
        PointPointDistance => {
            let first = point(0);
            let second = point(1);
            let authoritative = first.is_some() && second.is_some();
            let (mut first, mut second) = match (first, second) {
                (Some(first), Some(second)) => (first, second),
                (Some(known), None) => doubled_profile_distance_loci(
                    relation,
                    0,
                    1,
                    sketch,
                    parameter,
                    sketch_entities,
                    markers_by_id,
                )
                .or_else(|| {
                    Some((
                        known.clone(),
                        unique_profile_distance_locus(sketch, &known, parameter, sketch_entities)?,
                    ))
                })?,
                (None, Some(known)) => doubled_profile_distance_loci(
                    relation,
                    1,
                    0,
                    sketch,
                    parameter,
                    sketch_entities,
                    markers_by_id,
                )
                .or_else(|| {
                    Some((
                        unique_profile_distance_locus(sketch, &known, parameter, sketch_entities)?,
                        known,
                    ))
                })?,
                (None, None) => {
                    unique_profile_distance_loci_pair(sketch, parameter, sketch_entities)?
                }
            };
            if first == second {
                return None;
            }
            if !sketch_entities.is_empty() {
                let cadmpeg_ir::features::ParameterValue::Length(expected) =
                    parameter.value.as_ref()?
                else {
                    return None;
                };
                let first_point = profile_locus_point(&first, sketch_entities)?;
                let second_point = profile_locus_point(&second, sketch_entities)?;
                if !same_dimension_length(
                    (second_point.u - first_point.u).hypot(second_point.v - first_point.v),
                    expected.0,
                ) {
                    let horizontal =
                        same_dimension_length((second_point.u - first_point.u).abs(), expected.0);
                    let vertical =
                        same_dimension_length((second_point.v - first_point.v).abs(), expected.0);
                    let projected_distance_operands = relation
                        .operands
                        .iter()
                        .all(|operand| operand.kind == FeatureInputOperandKind::Native(0xbc7c));
                    if projected_distance_operands && horizontal != vertical {
                        return Some(if horizontal {
                            SketchConstraintDefinition::HorizontalDistance {
                                first,
                                second,
                                parameter: parameter_id,
                            }
                        } else {
                            SketchConstraintDefinition::VerticalDistance {
                                first,
                                second,
                                parameter: parameter_id,
                            }
                        });
                    }
                    if authoritative {
                        return Some(SketchConstraintDefinition::DistanceLoci {
                            first,
                            second,
                            parameter: parameter_id,
                        });
                    }
                    (first, second) = unique_repaired_profile_distance_loci_pair(
                        sketch,
                        &first,
                        &second,
                        parameter,
                        sketch_entities,
                    )?;
                }
            }
            Some(SketchConstraintDefinition::DistanceLoci {
                first,
                second,
                parameter: parameter_id,
            })
        }
        PointPointHorizontalDistance | PointPointVerticalDistance => {
            let horizontal = relation.family == PointPointHorizontalDistance;
            let first = point(0);
            let second = point(1);
            let authoritative = first.is_some() && second.is_some();
            let (mut first, mut second) = match (first, second) {
                (Some(first), Some(second)) => (first, second),
                (Some(known), None) => (
                    known.clone(),
                    unique_profile_axis_distance_locus(
                        sketch,
                        &known,
                        parameter,
                        sketch_entities,
                        horizontal,
                    )?,
                ),
                (None, Some(known)) => (
                    unique_profile_axis_distance_locus(
                        sketch,
                        &known,
                        parameter,
                        sketch_entities,
                        horizontal,
                    )?,
                    known,
                ),
                (None, None) => unique_profile_axis_distance_pair(
                    sketch,
                    parameter,
                    sketch_entities,
                    horizontal,
                )?,
            };
            if first == second {
                return None;
            }
            if !sketch_entities.is_empty() {
                let cadmpeg_ir::features::ParameterValue::Length(expected) =
                    parameter.value.as_ref()?
                else {
                    return None;
                };
                let first_point = profile_locus_point(&first, sketch_entities)?;
                let second_point = profile_locus_point(&second, sketch_entities)?;
                let measured = if horizontal {
                    (second_point.u - first_point.u).abs()
                } else {
                    (second_point.v - first_point.v).abs()
                };
                if !same_dimension_length(measured, expected.0) && !authoritative {
                    (first, second) = unique_repaired_profile_axis_distance_pair(
                        sketch,
                        &first,
                        &second,
                        parameter,
                        sketch_entities,
                        horizontal,
                    )?;
                }
            }
            Some(match relation.family {
                PointPointHorizontalDistance => SketchConstraintDefinition::HorizontalDistance {
                    first,
                    second,
                    parameter: parameter_id,
                },
                PointPointVerticalDistance => SketchConstraintDefinition::VerticalDistance {
                    first,
                    second,
                    parameter: parameter_id,
                },
                _ => unreachable!("relation family was filtered above"),
            })
        }
        PointLineDistance => {
            let point = marker(0)
                .and_then(|marker| marker_point_locus(marker, markers_by_id, loci_by_marker));
            let line = marker(1).and_then(|marker| {
                single_marker_line_entity(marker, markers_by_id, loci_by_marker, sketch_entities)
            });
            let authoritative = point.is_some() && line.is_some();
            let (mut point, mut line) = match (point, line) {
                (Some(point), Some(line)) => (point, line),
                (Some(point), None) => (
                    point.clone(),
                    unique_profile_point_line_entity(sketch, &point, parameter, sketch_entities)?,
                ),
                (None, Some(line)) => (
                    unique_profile_line_point_locus(sketch, &line, parameter, sketch_entities)?,
                    line,
                ),
                (None, None) => unique_profile_point_line_pair(sketch, parameter, sketch_entities)?,
            };
            let cadmpeg_ir::features::ParameterValue::Length(expected) =
                parameter.value.as_ref()?
            else {
                return None;
            };
            let point_position = profile_locus_point(&point, sketch_entities)?;
            let line_entity = sketch_entities.iter().find(|entity| entity.id == line)?;
            if !point_line_distance_value(point_position, line_entity)
                .is_some_and(|measured| same_dimension_length(measured, expected.0))
                && !authoritative
            {
                (point, line) = unique_repaired_profile_point_line_pair(
                    sketch,
                    &point,
                    &line,
                    parameter,
                    sketch_entities,
                )?;
            }
            Some(SketchConstraintDefinition::DistanceLoci {
                first: point,
                second: SketchLocus::Entity(line),
                parameter: parameter_id,
            })
        }
        LineLineDistance => {
            let operand_marker = |index: usize| {
                relation_operand_marker(relation, index, sketch, markers_by_id).or_else(|| {
                    relation_line_point_marker(relation, index, markers_by_id)
                        .map(|marker| marker.id.as_str())
                })
            };
            let curve = |index: usize| {
                let operand = relation.operands.get(index)?;
                (operand.kind == FeatureInputOperandKind::E1 && operand.entity_ref.is_none())
                    .then(|| solver_line_geometry_ref(&relation.feature_ref, operand.entity_index))
                    .and_then(|geometry_ref| {
                        sketch_entities
                            .iter()
                            .find(|entity| {
                                entity.sketch == *sketch
                                    && entity.geometry_ref.as_deref() == Some(geometry_ref.as_str())
                                    && matches!(entity.geometry, SketchGeometry::Line { .. })
                            })
                            .map(|entity| entity.id.clone())
                    })
                    .or_else(|| {
                        operand_marker(index).and_then(|marker| {
                            single_marker_line_entity(
                                marker,
                                markers_by_id,
                                loci_by_marker,
                                sketch_entities,
                            )
                        })
                    })
            };
            let first = curve(0);
            let second = curve(1);
            let authoritative =
                matches!((&first, &second), (Some(first), Some(second)) if first != second);
            let (mut first, mut second) = match (first, second) {
                (Some(first), Some(second)) => (first, second),
                (Some(known), None) => (
                    known.clone(),
                    if let Some(marker) = relation_line_point_marker(relation, 1, markers_by_id) {
                        unique_marker_line_distance_entity(
                            &marker.id,
                            sketch,
                            &known,
                            parameter,
                            sketch_entities,
                            markers_by_id,
                            loci_by_marker,
                        )?
                    } else {
                        unique_profile_line_distance_entity(
                            sketch,
                            &known,
                            parameter,
                            sketch_entities,
                        )?
                    },
                ),
                (None, Some(known)) => (
                    if let Some(marker) = relation_line_point_marker(relation, 0, markers_by_id) {
                        unique_marker_line_distance_entity(
                            &marker.id,
                            sketch,
                            &known,
                            parameter,
                            sketch_entities,
                            markers_by_id,
                            loci_by_marker,
                        )?
                    } else {
                        unique_profile_line_distance_entity(
                            sketch,
                            &known,
                            parameter,
                            sketch_entities,
                        )?
                    },
                    known,
                ),
                (None, None) => {
                    unique_profile_line_distance_pair(sketch, parameter, sketch_entities)?
                }
            };
            if first == second {
                let [first_operand, second_operand] = relation.operands.as_slice() else {
                    return None;
                };
                if first_operand.entity_index == second_operand.entity_index {
                    return None;
                }
                second = unique_profile_line_distance_entity(
                    sketch,
                    &first,
                    parameter,
                    sketch_entities,
                )?;
            }
            let cadmpeg_ir::features::ParameterValue::Length(expected) =
                parameter.value.as_ref()?
            else {
                return None;
            };
            let first_line = sketch_entities.iter().find(|entity| entity.id == first)?;
            let second_line = sketch_entities.iter().find(|entity| entity.id == second)?;
            if !line_line_distance(first_line, second_line)
                .is_some_and(|measured| same_dimension_length(measured, expected.0))
                && !authoritative
            {
                (first, second) = unique_repaired_profile_line_distance_pair(
                    sketch,
                    &first,
                    &second,
                    parameter,
                    sketch_entities,
                )?;
            }
            Some(SketchConstraintDefinition::Distance {
                entities: vec![first, second],
                parameter: parameter_id,
            })
        }
        Angle => {
            let curve = |index| {
                marker(index).and_then(|marker| {
                    single_marker_line_entity(
                        marker,
                        markers_by_id,
                        loci_by_marker,
                        sketch_entities,
                    )
                })
            };
            let first = curve(0);
            let second = curve(1);
            let authoritative = first.is_some() && second.is_some();
            let (mut first, mut second) = match (first, second) {
                (Some(first), Some(second)) => (first, second),
                (Some(known), None) => (
                    known.clone(),
                    unique_profile_line_angle_entity(sketch, &known, parameter, sketch_entities)?,
                ),
                (None, Some(known)) => (
                    unique_profile_line_angle_entity(sketch, &known, parameter, sketch_entities)?,
                    known,
                ),
                (None, None) => unique_profile_line_angle_pair(sketch, parameter, sketch_entities)?,
            };
            if first == second {
                return None;
            }
            let cadmpeg_ir::features::ParameterValue::Angle(expected) = parameter.value.as_ref()?
            else {
                return None;
            };
            let first_line = sketch_entities.iter().find(|entity| entity.id == first)?;
            let second_line = sketch_entities.iter().find(|entity| entity.id == second)?;
            if !line_line_angle(first_line, second_line)
                .is_some_and(|measured| same_dimension_angle(measured, expected.0))
                && !authoritative
            {
                (first, second) = unique_repaired_profile_line_angle_pair(
                    sketch,
                    &first,
                    &second,
                    parameter,
                    sketch_entities,
                )?;
            }
            Some(SketchConstraintDefinition::Angle {
                first,
                second,
                parameter: parameter_id,
            })
        }
        CircleDiameter => {
            if let Some(entities) =
                repeated_dimensioned_circular_entities(relation, parameter, sketch, sketch_entities)
            {
                return Some(match parameter.display {
                    Some(cadmpeg_ir::features::DimensionDisplay::Radius) => {
                        SketchConstraintDefinition::RepeatedRadius {
                            entities,
                            parameter: parameter_id,
                        }
                    }
                    Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => {
                        SketchConstraintDefinition::RepeatedDiameter {
                            entities,
                            parameter: parameter_id,
                        }
                    }
                    None => return None,
                });
            }
            let resolved_entity = sketch_entities
                .iter()
                .find(|entity| {
                    entity.sketch == *sketch
                        && entity.geometry_ref.as_deref() == Some(relation.id.as_str())
                        && matches!(
                            entity.geometry,
                            SketchGeometry::Circle { .. } | SketchGeometry::Arc { .. }
                        )
                })
                .map(|entity| entity.id.clone())
                .or_else(|| {
                    marker(0).and_then(|marker| {
                        marker_center_dimensioned_entity(marker, sketch, sketch_entities, parameter)
                            .or_else(|| {
                                if sketch_entities.is_empty() {
                                    single_marker_entity(marker, markers_by_id, loci_by_marker)
                                } else {
                                    single_marker_circular_entity(
                                        marker,
                                        markers_by_id,
                                        loci_by_marker,
                                        sketch_entities,
                                    )
                                }
                            })
                    })
                });
            let authoritative = resolved_entity.is_some();
            let entity = resolved_entity
                .or_else(|| unique_dimensioned_circle_entity(sketch, sketch_entities, parameter))?;
            if !sketch_entities.is_empty() {
                let cadmpeg_ir::features::ParameterValue::Length(expected) =
                    parameter.value.as_ref()?
                else {
                    return None;
                };
                let geometry = &sketch_entities
                    .iter()
                    .find(|candidate| candidate.id == entity)?
                    .geometry;
                let radius = match geometry {
                    SketchGeometry::Circle { radius, .. } | SketchGeometry::Arc { radius, .. } => {
                        radius.0
                    }
                    _ => return None,
                };
                let expected_radius = match parameter.display {
                    Some(cadmpeg_ir::features::DimensionDisplay::Radius) => expected.0,
                    Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => expected.0 * 0.5,
                    None => return None,
                };
                if !same_dimension_length(radius, expected_radius) && !authoritative {
                    return None;
                }
            }
            match parameter.display {
                Some(cadmpeg_ir::features::DimensionDisplay::Radius) => {
                    Some(SketchConstraintDefinition::Radius {
                        entity,
                        parameter: parameter_id,
                    })
                }
                Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => {
                    Some(SketchConstraintDefinition::Diameter {
                        entity,
                        parameter: parameter_id,
                    })
                }
                None => None,
            }
        }
    }
}

fn repeated_dimensioned_circular_entities(
    relation: &FeatureInputRelationInstance,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
) -> Option<Vec<SketchEntityId>> {
    let parameter_native_ref = parameter.native_ref.as_deref()?;
    if relation.parameter_scalar_ref.as_deref() != Some(parameter_native_ref) {
        return None;
    }
    let cadmpeg_ir::features::ParameterValue::Length(value) = parameter.value.as_ref()? else {
        return None;
    };
    let expected_radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
        None => return None,
    };
    if !(expected_radius.is_finite() && expected_radius > 0.0) {
        return None;
    }
    let entities = sketch_entities
        .iter()
        .filter(|entity| {
            entity.sketch == *sketch && entity.geometry_ref.as_deref() == Some(parameter_native_ref)
        })
        .filter_map(|entity| {
            let radius = match entity.geometry {
                SketchGeometry::Circle { radius, .. } | SketchGeometry::Arc { radius, .. } => {
                    radius.0
                }
                _ => return None,
            };
            same_dimension_length(radius, expected_radius).then(|| entity.id.clone())
        })
        .collect::<Vec<_>>();
    (entities.len() >= 2).then_some(entities)
}

// Reduce a set of candidate locus pairs to the sole survivor: order the pairs
// by their component locus keys, drop exact duplicates, and yield the pair only
// when exactly one remains. Shared by every profile-pair resolver so the
// tie-breaking order is identical across measurements.
fn sole_locus_pair(
    mut candidates: Vec<(SketchLocus, SketchLocus)>,
) -> Option<(SketchLocus, SketchLocus)> {
    candidates.sort_by(|(first_left, second_left), (first_right, second_right)| {
        locus_key(first_left)
            .cmp(&locus_key(first_right))
            .then_with(|| locus_key(second_left).cmp(&locus_key(second_right)))
    });
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

// Find the unique profile locus pair whose spanned dimension, measured by
// `measure` over the two loci points, equals the parameter length. Every
// unordered pair of canonical profile loci is a candidate; `measure` is the
// only axis of variation between the distance and axis-distance resolvers.
fn unique_profile_measured_loci_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    measure: impl Fn(&Point2, &Point2) -> f64,
) -> Option<(SketchLocus, SketchLocus)> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let loci = canonical_profile_loci(sketch, sketch_entities);
    let mut candidates = Vec::new();
    for (first_index, (first_point, first)) in loci.iter().enumerate() {
        for (second_point, second) in &loci[first_index + 1..] {
            if same_dimension_length(measure(first_point, second_point), distance.0) {
                candidates.push((first.clone(), second.clone()));
            }
        }
    }
    sole_locus_pair(candidates)
}

// Repair a candidate pair by resolving each supplied locus to its unique
// partner via `partner`, forming the sorted pair, and keeping it only when the
// two starting loci agree on exactly one pair.
fn unique_repaired_profile_pair(
    first: &SketchLocus,
    second: &SketchLocus,
    partner: impl Fn(&SketchLocus) -> Option<SketchLocus>,
) -> Option<(SketchLocus, SketchLocus)> {
    let candidates = [first, second]
        .into_iter()
        .filter_map(|known| {
            let mut pair = [known.clone(), partner(known)?];
            pair.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
            Some((pair[0].clone(), pair[1].clone()))
        })
        .collect::<Vec<_>>();
    sole_locus_pair(candidates)
}

// Find the unique profile locus at a given dimension from `known`, where
// `measure` reports the dimension between the known point and a candidate
// point. The known locus is excluded and, as for the pair resolvers, `measure`
// is the sole axis of variation between the straight-distance and axis-distance
// forms.
fn unique_profile_measured_locus(
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    measure: impl Fn(&Point2, &Point2) -> f64,
) -> Option<SketchLocus> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let known_point = profile_locus_point(known, sketch_entities)?;
    let mut candidates = canonical_profile_loci(sketch, sketch_entities)
        .into_iter()
        .filter_map(|(candidate_point, candidate)| {
            (candidate != *known
                && same_dimension_length(measure(&known_point, &candidate_point), distance.0))
            .then_some(candidate)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(super) fn unique_profile_distance_locus(
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchLocus> {
    unique_profile_measured_locus(
        sketch,
        known,
        parameter,
        sketch_entities,
        |known, candidate| (candidate.u - known.u).hypot(candidate.v - known.v),
    )
}

pub(super) fn doubled_profile_distance_loci(
    relation: &FeatureInputRelationInstance,
    line_operand: usize,
    center_operand: usize,
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
) -> Option<(SketchLocus, SketchLocus)> {
    const NATIVE_TO_IR: f64 = 1000.0;

    let cadmpeg_ir::features::ParameterValue::Length(expected) = parameter.value.as_ref()? else {
        return None;
    };
    let line_marker_id = relation_operand_marker(relation, line_operand, sketch, markers_by_id)?;
    let center_marker_id =
        relation_operand_marker(relation, center_operand, sketch, markers_by_id)?;
    let line_marker = markers_by_id.get(line_marker_id)?;
    let center_marker = markers_by_id.get(center_marker_id)?;
    if line_marker.feature_ref.as_deref() != Some(relation.feature_ref.as_str())
        || center_marker.feature_ref.as_deref() != Some(relation.feature_ref.as_str())
    {
        return None;
    }
    let center_is_distance_handle = markers_by_id.values().any(|marker| {
        marker.feature_ref.as_deref() == Some(relation.feature_ref.as_str())
            && marker.kind
                == SketchInputKind::Relation(crate::records::SketchRelationKind::Distance)
            && matches!(marker.links.as_slice(), [link] if link.entity_ref == center_marker.id)
    });
    if !center_is_distance_handle {
        return None;
    }
    let [line_u, line_v] = line_marker.coordinates_m?;
    let [center_u, center_v] = center_marker.coordinates_m?;
    let has_half_dimension = [
        (center_u - line_u).abs() * NATIVE_TO_IR * 2.0,
        (center_v - line_v).abs() * NATIVE_TO_IR * 2.0,
    ]
    .into_iter()
    .any(|distance| same_dimension_length(distance, expected.0));
    if !has_half_dimension {
        return None;
    }
    let candidates = sketch_entities
        .iter()
        .filter(|entity| {
            entity.sketch == *sketch && entity.native_ref.as_deref() == Some(line_marker_id)
        })
        .filter_map(|entity| {
            let SketchGeometry::Line { start, end } = entity.geometry else {
                return None;
            };
            same_dimension_length((end.u - start.u).hypot(end.v - start.v), expected.0).then(|| {
                (
                    SketchLocus::Start(entity.id.clone()),
                    SketchLocus::End(entity.id.clone()),
                )
            })
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn unique_repaired_profile_distance_loci_pair(
    sketch: &SketchId,
    first: &SketchLocus,
    second: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchLocus)> {
    unique_repaired_profile_pair(first, second, |known| {
        unique_profile_distance_locus(sketch, known, parameter, sketch_entities)
    })
}

pub(super) fn unique_profile_axis_distance_locus(
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    horizontal: bool,
) -> Option<SketchLocus> {
    unique_profile_measured_locus(
        sketch,
        known,
        parameter,
        sketch_entities,
        |known, candidate| {
            if horizontal {
                (candidate.u - known.u).abs()
            } else {
                (candidate.v - known.v).abs()
            }
        },
    )
}

fn unique_repaired_profile_axis_distance_pair(
    sketch: &SketchId,
    first: &SketchLocus,
    second: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    horizontal: bool,
) -> Option<(SketchLocus, SketchLocus)> {
    unique_repaired_profile_pair(first, second, |known| {
        unique_profile_axis_distance_locus(sketch, known, parameter, sketch_entities, horizontal)
    })
}

pub(super) fn unique_profile_axis_distance_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    horizontal: bool,
) -> Option<(SketchLocus, SketchLocus)> {
    unique_profile_measured_loci_pair(sketch, parameter, sketch_entities, |first, second| {
        if horizontal {
            (second.u - first.u).abs()
        } else {
            (second.v - first.v).abs()
        }
    })
}

pub(super) fn unique_profile_distance_loci_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchLocus)> {
    unique_profile_measured_loci_pair(sketch, parameter, sketch_entities, |first, second| {
        (second.u - first.u).hypot(second.v - first.v)
    })
}

pub(super) fn canonical_profile_loci(
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
) -> Vec<(Point2, SketchLocus)> {
    const QUANTUM: f64 = 1.0e-8;
    let mut loci = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .flat_map(sketch_entity_loci)
        .collect::<Vec<_>>();
    loci.sort_by(|(left_point, left_locus), (right_point, right_locus)| {
        quantize(*left_point, QUANTUM)
            .cmp(&quantize(*right_point, QUANTUM))
            .then_with(|| locus_key(left_locus).cmp(&locus_key(right_locus)))
    });
    loci.dedup_by(|(left_point, _), (right_point, _)| {
        quantize(*left_point, QUANTUM) == quantize(*right_point, QUANTUM)
    });
    loci
}

// Reduce candidate entity matches to the sole survivor by natural order:
// sort, drop duplicates, and yield the value only when exactly one remains.
// Serves both single-entity and entity-pair resolvers.
fn sole_sorted<T: Ord + Clone>(mut candidates: Vec<T>) -> Option<T> {
    candidates.sort();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

// Find the unique sketch entity, other than `known`, for which `matches`
// accepts the ordered pair `(known, candidate)`. The parameter kind guard and
// the measurement both live in `matches`, so the straight-distance and angle
// resolvers differ only in the closure they pass.
fn unique_profile_matched_entity(
    sketch: &SketchId,
    known: &SketchEntityId,
    sketch_entities: &[SketchEntity],
    matches: impl Fn(&SketchEntity, &SketchEntity) -> bool,
) -> Option<SketchEntityId> {
    let known = sketch_entities.iter().find(|entity| entity.id == *known)?;
    let candidates = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch && entity.id != known.id)
        .filter(|candidate| matches(known, candidate))
        .map(|candidate| candidate.id.clone())
        .collect::<Vec<_>>();
    sole_sorted(candidates)
}

// Find the unique unordered pair of sketch lines for which `matches` accepts
// the pair. Only line entities participate; `matches` supplies both the
// measurement and its comparison.
fn unique_profile_matched_line_pair(
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    matches: impl Fn(&SketchEntity, &SketchEntity) -> bool,
) -> Option<(SketchEntityId, SketchEntityId)> {
    let lines = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .filter(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for (first_index, first) in lines.iter().enumerate() {
        for second in &lines[first_index + 1..] {
            if matches(first, second) {
                candidates.push((first.id.clone(), second.id.clone()));
            }
        }
    }
    sole_sorted(candidates)
}

// Repair an entity pair by resolving each supplied entity to its unique partner
// via `partner`, forming the sorted pair, and keeping it only when the two
// starting entities agree on exactly one pair.
fn unique_repaired_entity_pair(
    first: &SketchEntityId,
    second: &SketchEntityId,
    partner: impl Fn(&SketchEntityId) -> Option<SketchEntityId>,
) -> Option<(SketchEntityId, SketchEntityId)> {
    let candidates = [first, second]
        .into_iter()
        .filter_map(|known| {
            let mut pair = [known.clone(), partner(known)?];
            pair.sort();
            Some((pair[0].clone(), pair[1].clone()))
        })
        .collect::<Vec<_>>();
    sole_sorted(candidates)
}

pub(super) fn unique_profile_line_distance_entity(
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    unique_profile_matched_entity(sketch, known, sketch_entities, |known, candidate| {
        line_line_distance(known, candidate)
            .is_some_and(|measured| same_dimension_length(measured, distance.0))
    })
}

fn unique_marker_line_distance_entity(
    marker: &str,
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchEntityId> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let marker_locus = marker_point_locus(marker, markers_by_id, loci_by_marker)?;
    let marker_point = profile_locus_point(&marker_locus, sketch_entities)?;
    let known = sketch_entities.iter().find(|entity| entity.id == *known)?;
    sole_sorted(
        sketch_entities
            .iter()
            .filter(|candidate| candidate.sketch == *sketch && candidate.id != known.id)
            .filter(|candidate| matches!(candidate.geometry, SketchGeometry::Line { .. }))
            .filter(|candidate| sketch_entity_contains_point(candidate, marker_point))
            .filter(|candidate| {
                line_line_distance(known, candidate)
                    .is_some_and(|measured| same_dimension_length(measured, distance.0))
            })
            .map(|candidate| candidate.id.clone())
            .collect(),
    )
}

pub(super) fn unique_profile_line_distance_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchEntityId, SketchEntityId)> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    unique_profile_matched_line_pair(sketch, sketch_entities, |first, second| {
        line_line_distance(first, second)
            .is_some_and(|measured| same_dimension_length(measured, distance.0))
    })
}

pub(super) fn unique_repaired_profile_line_distance_pair(
    sketch: &SketchId,
    first: &SketchEntityId,
    second: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchEntityId, SketchEntityId)> {
    unique_repaired_entity_pair(first, second, |known| {
        unique_profile_line_distance_entity(sketch, known, parameter, sketch_entities)
    })
}

pub(super) fn line_line_distance(first: &SketchEntity, second: &SketchEntity) -> Option<f64> {
    let SketchGeometry::Line {
        start: first_start,
        end: first_end,
    } = &first.geometry
    else {
        return None;
    };
    let SketchGeometry::Line {
        start: second_start,
        end: second_end,
    } = &second.geometry
    else {
        return None;
    };
    let first_direction = [first_end.u - first_start.u, first_end.v - first_start.v];
    let second_direction = [second_end.u - second_start.u, second_end.v - second_start.v];
    let first_length = first_direction[0].hypot(first_direction[1]);
    let second_length = second_direction[0].hypot(second_direction[1]);
    if first_length <= SKETCH_POINT_TOLERANCE || second_length <= SKETCH_POINT_TOLERANCE {
        return None;
    }
    let cross = |left: [f64; 2], right: [f64; 2]| left[0] * right[1] - left[1] * right[0];
    if cross(first_direction, second_direction).abs()
        > SKETCH_POINT_TOLERANCE * first_length * second_length
    {
        return None;
    }
    Some(
        cross(
            [
                second_start.u - first_start.u,
                second_start.v - first_start.v,
            ],
            first_direction,
        )
        .abs()
            / first_length,
    )
}

pub(super) fn unique_profile_line_angle_entity(
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let cadmpeg_ir::features::ParameterValue::Angle(angle) = parameter.value.as_ref()? else {
        return None;
    };
    unique_profile_matched_entity(sketch, known, sketch_entities, |known, candidate| {
        line_line_angle(known, candidate)
            .is_some_and(|measured| same_dimension_angle(measured, angle.0))
    })
}

pub(super) fn unique_profile_line_angle_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchEntityId, SketchEntityId)> {
    let cadmpeg_ir::features::ParameterValue::Angle(angle) = parameter.value.as_ref()? else {
        return None;
    };
    unique_profile_matched_line_pair(sketch, sketch_entities, |first, second| {
        line_line_angle(first, second)
            .is_some_and(|measured| same_dimension_angle(measured, angle.0))
    })
}

pub(super) fn unique_repaired_profile_line_angle_pair(
    sketch: &SketchId,
    first: &SketchEntityId,
    second: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchEntityId, SketchEntityId)> {
    unique_repaired_entity_pair(first, second, |known| {
        unique_profile_line_angle_entity(sketch, known, parameter, sketch_entities)
    })
}

fn line_line_angle(first: &SketchEntity, second: &SketchEntity) -> Option<f64> {
    let SketchGeometry::Line {
        start: first_start,
        end: first_end,
    } = &first.geometry
    else {
        return None;
    };
    let SketchGeometry::Line {
        start: second_start,
        end: second_end,
    } = &second.geometry
    else {
        return None;
    };
    let first_direction = [first_end.u - first_start.u, first_end.v - first_start.v];
    let second_direction = [second_end.u - second_start.u, second_end.v - second_start.v];
    let first_length = first_direction[0].hypot(first_direction[1]);
    let second_length = second_direction[0].hypot(second_direction[1]);
    if first_length <= SKETCH_POINT_TOLERANCE || second_length <= SKETCH_POINT_TOLERANCE {
        return None;
    }
    Some(
        ((first_direction[0] * second_direction[0] + first_direction[1] * second_direction[1])
            / (first_length * second_length))
            .clamp(-1.0, 1.0)
            .acos(),
    )
}

pub(super) fn same_dimension_angle(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1.0e-9 * left.abs().max(right.abs()).max(1.0)
}

pub(super) fn unique_profile_point_line_entity(
    sketch: &SketchId,
    point: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let point = profile_locus_point(point, sketch_entities)?;
    let mut candidates = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .filter_map(|line| {
            point_line_distance_value(point, line)
                .filter(|measured| same_dimension_length(*measured, distance.0))
                .map(|_| line.id.clone())
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(super) fn unique_profile_line_point_locus(
    sketch: &SketchId,
    line: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchLocus> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let line = sketch_entities.iter().find(|entity| entity.id == *line)?;
    let mut candidates = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .flat_map(sketch_entity_loci)
        .filter_map(|(point, locus)| {
            point_line_distance_value(point, line)
                .filter(|measured| same_dimension_length(*measured, distance.0))
                .map(|_| locus)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(super) fn unique_profile_point_line_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchEntityId)> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let loci = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .flat_map(sketch_entity_loci)
        .collect::<Vec<_>>();
    let lines = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .filter(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for (point, locus) in loci {
        for line in &lines {
            if point_line_distance_value(point, line)
                .is_some_and(|measured| same_dimension_length(measured, distance.0))
            {
                candidates.push((locus.clone(), line.id.clone()));
            }
        }
    }
    candidates.sort_by(|(left_locus, left_line), (right_locus, right_line)| {
        locus_key(left_locus)
            .cmp(&locus_key(right_locus))
            .then_with(|| left_line.cmp(right_line))
    });
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(super) fn unique_repaired_profile_point_line_pair(
    sketch: &SketchId,
    point: &SketchLocus,
    line: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchEntityId)> {
    let mut candidates = Vec::new();
    if let Some(candidate_line) =
        unique_profile_point_line_entity(sketch, point, parameter, sketch_entities)
    {
        candidates.push((point.clone(), candidate_line));
    }
    if let Some(candidate_point) =
        unique_profile_line_point_locus(sketch, line, parameter, sketch_entities)
    {
        candidates.push((candidate_point, line.clone()));
    }
    candidates.sort_by(|(left_point, left_line), (right_point, right_line)| {
        locus_key(left_point)
            .cmp(&locus_key(right_point))
            .then_with(|| left_line.cmp(right_line))
    });
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(super) fn profile_locus_point(
    locus: &SketchLocus,
    sketch_entities: &[SketchEntity],
) -> Option<Point2> {
    let entity = sketch_entities
        .iter()
        .find(|entity| entity.id == locus_entity(locus))?;
    sketch_entity_loci(entity)
        .into_iter()
        .find_map(|(point, candidate)| (candidate == *locus).then_some(point))
}

fn canonicalize_physical_loci(
    loci: &mut Vec<SketchLocus>,
    sketch_entities: &[SketchEntity],
    quantum: f64,
) {
    if loci.len() < 2 {
        return;
    }
    let points = loci
        .iter()
        .map(|locus| {
            profile_locus_point(locus, sketch_entities).map(|point| quantize(point, quantum))
        })
        .collect::<Option<Vec<_>>>();
    let Some(points) = points else {
        return;
    };
    if points.iter().all(|point| *point == points[0]) {
        loci.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
        loci.truncate(1);
    }
}

pub(super) fn point_line_distance_value(point: Point2, line: &SketchEntity) -> Option<f64> {
    let SketchGeometry::Line { start, end } = &line.geometry else {
        return None;
    };
    let direction = [end.u - start.u, end.v - start.v];
    let length = direction[0].hypot(direction[1]);
    (length > SKETCH_POINT_TOLERANCE).then(|| {
        ((point.u - start.u) * direction[1] - (point.v - start.v) * direction[0]).abs() / length
    })
}

pub(super) fn relation_operand_marker<'a>(
    relation: &'a FeatureInputRelationInstance,
    index: usize,
    sketch: &SketchId,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Option<&'a str> {
    let operand = relation.operands.get(index)?;
    if sketch.0.contains("sketch#compact:") && operand.kind == FeatureInputOperandKind::D6 {
        let mut coordinate_handles = markers_by_id
            .values()
            .copied()
            .filter(|marker| marker.feature_ref.as_deref() == Some(&relation.feature_ref))
            .filter(|marker| marker.coordinates_m.is_some())
            .filter(|marker| {
                matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
            })
            .collect::<Vec<_>>();
        coordinate_handles.sort_unstable_by_key(|marker| marker.offset);
        return coordinate_handles
            .get(usize::from(operand.entity_index))
            .map(|marker| marker.id.as_str());
    }
    operand.entity_ref.as_deref()
}

fn relation_line_point_marker<'a>(
    relation: &FeatureInputRelationInstance,
    index: usize,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Option<&'a SketchInputEntity> {
    let operand = relation.operands.get(index)?;
    if operand.kind != FeatureInputOperandKind::Native(0x8386) || operand.entity_ref.is_some() {
        return None;
    }
    let candidates = markers_by_id
        .values()
        .copied()
        .filter(|marker| marker.feature_ref.as_deref() == Some(&relation.feature_ref))
        .filter(|marker| marker.local_id == Some(u32::from(operand.entity_index)))
        .filter(|marker| marker.coordinates_m.is_some())
        .filter(|marker| {
            matches!(
                marker.kind,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        })
        .collect::<Vec<_>>();
    let [marker] = candidates.as_slice() else {
        return None;
    };
    Some(*marker)
}

fn marker_center_dimensioned_entity(
    marker_id: &str,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Option<SketchEntityId> {
    let cadmpeg_ir::features::ParameterValue::Length(value) = parameter.value.as_ref()? else {
        return None;
    };
    let expected_radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
        None => return None,
    };
    let centers = sketch_entities
        .iter()
        .filter(|entity| {
            entity.sketch == *sketch
                && entity.native_ref.as_deref() == Some(marker_id)
                && matches!(entity.geometry, SketchGeometry::Point { .. })
        })
        .collect::<Vec<_>>();
    let [center_entity] = centers.as_slice() else {
        return None;
    };
    let SketchGeometry::Point { position: center } = center_entity.geometry else {
        return None;
    };
    let candidates = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .filter_map(|entity| {
            let (candidate_center, radius) = match entity.geometry {
                SketchGeometry::Circle { center, radius }
                | SketchGeometry::Arc { center, radius, .. } => (center, radius.0),
                _ => return None,
            };
            (quantize(candidate_center, 1.0e-8) == quantize(center, 1.0e-8)
                && same_dimension_length(radius, expected_radius))
            .then_some(entity.id.clone())
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn unique_dimensioned_circle_entity(
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Option<SketchEntityId> {
    let cadmpeg_ir::features::ParameterValue::Length(value) = parameter.value.as_ref()? else {
        return None;
    };
    let expected_radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
        None => return None,
    };
    let mut matches = sketch_entities.iter().filter_map(|entity| {
        if entity.sketch != *sketch {
            return None;
        }
        let radius = match &entity.geometry {
            SketchGeometry::Circle { radius, .. } | SketchGeometry::Arc { radius, .. } => radius.0,
            _ => return None,
        };
        same_dimension_length(radius, expected_radius).then_some(entity.id.clone())
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

pub(super) fn same_dimension_length(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1.0e-9 * left.abs().max(right.abs()).max(1.0)
}

pub(super) fn marker_point_locus(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchLocus> {
    if let Some(locus) = loci_by_marker
        .get(&qualified_point_marker_key(marker_id))
        .and_then(|loci| unique_locus(loci))
    {
        return Some(locus);
    }
    resolved_marker_locus(
        marker_id,
        markers_by_id,
        loci_by_marker,
        &mut HashSet::new(),
    )
}

pub(super) fn qualified_point_marker_key(marker_id: &str) -> String {
    format!("{marker_id}:qualified-point")
}

pub(super) fn resolved_marker_locus(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    visited: &mut HashSet<String>,
) -> Option<SketchLocus> {
    if let Some(locus) = loci_by_marker
        .get(marker_id)
        .and_then(|loci| unique_locus(loci))
    {
        return Some(locus);
    }
    if !visited.insert(marker_id.to_string()) {
        return None;
    }
    let marker = markers_by_id.get(marker_id)?;
    let mut linked = marker
        .links
        .iter()
        .filter(|link| link.entity_ref != marker_id)
        .filter(|link| {
            !matches!(
                markers_by_id
                    .get(link.entity_ref.as_str())
                    .map(|marker| marker.kind),
                Some(SketchInputKind::Relation(_))
            )
        })
        .filter_map(|link| {
            resolved_marker_locus(
                &link.entity_ref,
                markers_by_id,
                loci_by_marker,
                &mut visited.clone(),
            )
        })
        .collect::<Vec<_>>();
    linked.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    linked.dedup();
    unique_locus(&linked)
}

pub(super) fn unique_locus(loci: &[SketchLocus]) -> Option<SketchLocus> {
    let [locus] = loci else {
        return None;
    };
    Some(locus.clone())
}

fn single_marker_entity(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchEntityId> {
    let entities = marker_entities(marker_id, markers_by_id, loci_by_marker);
    let [entity] = entities.as_slice() else {
        return None;
    };
    Some(entity.clone())
}

fn single_marker_circular_entity(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let mut entities = marker_entities(marker_id, markers_by_id, loci_by_marker)
        .into_iter()
        .filter(|id| {
            sketch_entities
                .iter()
                .find(|entity| entity.id == *id)
                .is_some_and(|entity| {
                    matches!(
                        entity.geometry,
                        SketchGeometry::Circle { .. } | SketchGeometry::Arc { .. }
                    )
                })
        })
        .collect::<Vec<_>>();
    entities.sort();
    entities.dedup();
    let [entity] = entities.as_slice() else {
        return None;
    };
    Some(entity.clone())
}

pub(super) fn single_marker_line_entity(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let mut entities = marker_line_entities_inner(
        marker_id,
        markers_by_id,
        loci_by_marker,
        sketch_entities,
        &mut HashSet::new(),
    );
    entities.sort();
    entities.dedup();
    if let [entity] = entities.as_slice() {
        return Some(entity.clone());
    }
    let marker = markers_by_id.get(marker_id)?;
    let links = marker
        .links
        .iter()
        .filter(|link| {
            link.entity_ref != marker_id
                && (!matches!(marker.kind, SketchInputKind::Relation(_))
                    || !relation_link_identifies_owner(marker, link))
        })
        .collect::<Vec<_>>();
    let [first_link, second_link] = links.as_slice() else {
        return unique_line_containing_marker_point(
            marker_id,
            markers_by_id,
            loci_by_marker,
            sketch_entities,
        );
    };
    let Some(first_locus) =
        marker_point_locus(&first_link.entity_ref, markers_by_id, loci_by_marker)
    else {
        return unique_line_containing_marker_point(
            marker_id,
            markers_by_id,
            loci_by_marker,
            sketch_entities,
        );
    };
    let Some(second_locus) =
        marker_point_locus(&second_link.entity_ref, markers_by_id, loci_by_marker)
    else {
        return unique_line_containing_marker_point(
            marker_id,
            markers_by_id,
            loci_by_marker,
            sketch_entities,
        );
    };
    let first_entity = locus_entity(&first_locus);
    let second_entity = locus_entity(&second_locus);
    let Some(sketch) = sketch_entities
        .iter()
        .find(|entity| entity.id == first_entity)
        .map(|entity| entity.sketch.clone())
    else {
        return unique_line_containing_marker_point(
            marker_id,
            markers_by_id,
            loci_by_marker,
            sketch_entities,
        );
    };
    if sketch_entities
        .iter()
        .find(|entity| entity.id == second_entity)
        .is_none_or(|entity| entity.sketch != sketch)
    {
        return unique_line_containing_marker_point(
            marker_id,
            markers_by_id,
            loci_by_marker,
            sketch_entities,
        );
    }
    let first = profile_locus_point(&first_locus, sketch_entities)?;
    let second = profile_locus_point(&second_locus, sketch_entities)?;
    if same_dimension_length(first.u, second.u) && same_dimension_length(first.v, second.v) {
        return unique_line_containing_marker_point(
            marker_id,
            markers_by_id,
            loci_by_marker,
            sketch_entities,
        );
    }
    sole_sorted(
        sketch_entities
            .iter()
            .filter(|entity| {
                entity.sketch == sketch && matches!(entity.geometry, SketchGeometry::Line { .. })
            })
            .filter(|entity| {
                sketch_entity_contains_point(entity, first)
                    && sketch_entity_contains_point(entity, second)
            })
            .map(|entity| entity.id.clone())
            .collect(),
    )
    .or_else(|| {
        unique_line_containing_marker_point(
            marker_id,
            markers_by_id,
            loci_by_marker,
            sketch_entities,
        )
    })
}

fn unique_line_containing_marker_point(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let marker = markers_by_id.get(marker_id)?;
    if !matches!(
        marker.kind,
        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
    ) {
        return None;
    }
    let locus = marker_point_locus(marker_id, markers_by_id, loci_by_marker)?;
    let point = profile_locus_point(&locus, sketch_entities)?;
    let sketch = sketch_entities
        .iter()
        .find(|entity| entity.id == locus_entity(&locus))
        .map(|entity| entity.sketch.clone())?;
    sole_sorted(
        sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch)
            .filter(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
            .filter(|entity| sketch_entity_contains_point(entity, point))
            .map(|entity| entity.id.clone())
            .collect(),
    )
}

fn marker_line_entities_inner(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
    visited: &mut HashSet<String>,
) -> Vec<SketchEntityId> {
    let is_line = |id: &SketchEntityId| {
        sketch_entities
            .iter()
            .find(|entity| entity.id == *id)
            .is_some_and(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
    };
    let direct = loci_by_marker.get(marker_id).map(|loci| {
        loci.iter()
            .map(locus_entity)
            .filter(is_line)
            .collect::<HashSet<_>>()
    });
    if loci_by_marker.contains_key(marker_id) {
        return direct.into_iter().flatten().collect();
    }
    if !visited.insert(marker_id.to_string()) {
        return direct.into_iter().flatten().collect();
    }
    let Some(marker) = markers_by_id.get(marker_id) else {
        return direct.into_iter().flatten().collect();
    };
    let mut linked = marker
        .links
        .iter()
        .filter(|link| {
            link.entity_ref != marker_id
                && (!matches!(marker.kind, SketchInputKind::Relation(_))
                    || !relation_link_identifies_owner(marker, link))
        })
        .map(|link| {
            marker_line_entities_inner(
                &link.entity_ref,
                markers_by_id,
                loci_by_marker,
                sketch_entities,
                &mut visited.clone(),
            )
            .into_iter()
            .collect::<HashSet<_>>()
        })
        .filter(|entities| !entities.is_empty());
    let mut entities = direct
        .filter(|entities| !entities.is_empty())
        .or_else(|| linked.next())
        .unwrap_or_default();
    for candidates in linked {
        entities.retain(|entity| candidates.contains(entity));
    }
    entities.into_iter().collect()
}

pub(super) fn profile_loci_by_marker(
    features: &[cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    sketch_entities: &[SketchEntity],
    lanes: &[FeatureInputLane],
) -> HashMap<String, Vec<SketchLocus>> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;
    let qualified_point_markers = lanes
        .iter()
        .flat_map(|lane| &lane.relation_instances)
        .flat_map(|relation| &relation.operands)
        .filter(|operand| {
            matches!(
                operand.kind,
                FeatureInputOperandKind::D6
                    | FeatureInputOperandKind::Native(
                        0x80cc
                            | 0x8152
                            | 0x81b2
                            | 0x837b
                            | 0x8ab6
                            | 0x8dcb
                            | 0x929d
                            | 0xbc7c
                            | 0xbd69,
                    )
            )
        })
        .filter_map(|operand| operand.entity_ref.as_deref())
        .collect::<HashSet<_>>();

    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch))
        })
        .collect::<HashMap<_, _>>();
    let mut profile_loci = HashMap::<&SketchId, Vec<(Point2, SketchLocus)>>::new();
    let mut line_midpoints = HashMap::<&SketchId, Vec<(Point2, SketchLocus)>>::new();
    let geometry_by_entity = sketch_entities
        .iter()
        .map(|entity| (&entity.id, &entity.geometry))
        .collect::<HashMap<_, _>>();
    let transforms =
        marker_transform_candidates_by_feature(features, sketches, sketch_entities, lanes);
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let native_point_markers_with_nonpoint_carrier = sketch_entities
        .iter()
        .filter(|entity| !matches!(entity.geometry, SketchGeometry::Point { .. }))
        .filter_map(|entity| entity.native_ref.as_deref())
        .collect::<HashSet<_>>();
    for entity in sketch_entities {
        for (point, locus) in sketch_entity_loci(entity) {
            profile_loci
                .entry(&entity.sketch)
                .or_default()
                .push((point, locus));
        }
        if let SketchGeometry::Line { start, end } = &entity.geometry {
            line_midpoints.entry(&entity.sketch).or_default().push((
                Point2::new((start.u + end.u) * 0.5, (start.v + end.v) * 0.5),
                SketchLocus::Entity(entity.id.clone()),
            ));
        }
    }
    let mut result = sketch_entities
        .iter()
        .filter_map(|entity| {
            let (marker, qualified_point) = if let Some(marker) = entity.native_ref.as_ref() {
                (
                    marker,
                    matches!(entity.geometry, SketchGeometry::Point { .. })
                        && native_point_markers_with_nonpoint_carrier.contains(marker.as_str()),
                )
            } else {
                let reference = entity.geometry_ref.as_ref().filter(|reference| {
                    reference.starts_with("sldprt:feature-input:sketch-entity#")
                })?;
                (
                    reference,
                    matches!(entity.geometry, SketchGeometry::Point { .. }),
                )
            };
            markers_by_id.contains_key(marker.as_str()).then(|| {
                let locus = if entity.id.0.contains("sketch-entity#compact:")
                    && matches!(entity.geometry, SketchGeometry::Line { .. })
                {
                    SketchLocus::Start(entity.id.clone())
                } else if markers_by_id.get(marker.as_str()).is_some_and(|marker| {
                    matches!(
                        marker.kind,
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
                }) && matches!(
                    entity.geometry,
                    SketchGeometry::Circle { .. }
                        | SketchGeometry::Arc { .. }
                        | SketchGeometry::Ellipse { .. }
                ) {
                    SketchLocus::Center(entity.id.clone())
                } else {
                    SketchLocus::Entity(entity.id.clone())
                };
                let marker = if qualified_point {
                    qualified_point_marker_key(marker)
                } else {
                    marker.clone()
                };
                (marker, vec![locus])
            })
        })
        .collect::<HashMap<String, Vec<SketchLocus>>>();
    let mut endpoint_marker_keys = HashSet::new();
    for entity in sketch_entities {
        let [start, end] = entity.endpoint_refs.as_slice() else {
            continue;
        };
        for (marker, locus) in [
            (start, SketchLocus::Start(entity.id.clone())),
            (end, SketchLocus::End(entity.id.clone())),
        ] {
            if !markers_by_id.contains_key(marker.as_str()) {
                continue;
            }
            endpoint_marker_keys.insert(marker.clone());
            let loci = result.entry(marker.clone()).or_default();
            if !loci.contains(&locus) {
                loci.push(locus.clone());
            }
            if qualified_point_markers.contains(marker.as_str()) {
                let qualified_key = qualified_point_marker_key(marker);
                endpoint_marker_keys.insert(qualified_key.clone());
                let loci = result.entry(qualified_key).or_default();
                if !loci.contains(&locus) {
                    loci.push(locus);
                }
            }
        }
    }
    for marker in endpoint_marker_keys {
        if let Some(loci) = result.get_mut(&marker) {
            canonicalize_physical_loci(loci, sketch_entities, QUANTUM);
        }
    }
    for lane in lanes {
        let mut markers_by_feature = HashMap::<&str, Vec<&SketchInputEntity>>::new();
        for marker in &lane.sketch_entities {
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            if marker.coordinates_m.is_some() && sketches_by_feature.contains_key(feature) {
                markers_by_feature.entry(feature).or_default().push(marker);
            }
        }
        for (feature, markers) in markers_by_feature {
            let Some(sketch) = sketches_by_feature.get(feature) else {
                continue;
            };
            let Some(loci) = profile_loci.get(sketch) else {
                continue;
            };
            let transforms = transforms
                .get(feature)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let loci_by_point = loci.iter().fold(
                HashMap::<(i64, i64), Vec<SketchLocus>>::new(),
                |mut by_point, (point, locus)| {
                    by_point
                        .entry(quantize(*point, QUANTUM))
                        .or_default()
                        .push(locus.clone());
                    by_point
                },
            );
            for marker in markers {
                let qualified_point = qualified_point_markers.contains(marker.id.as_str());
                let result_key = if qualified_point {
                    qualified_point_marker_key(&marker.id)
                } else {
                    marker.id.clone()
                };
                if result.contains_key(&result_key) {
                    continue;
                }
                if qualified_point && sketch.0.contains("sketch#compact:") {
                    continue;
                }
                let Some([u, v]) = marker.coordinates_m else {
                    continue;
                };
                let primary_geometry_locus = usize::try_from(marker.offset)
                    .ok()
                    .is_some_and(|offset| marker_is_geometry_locus(&lane.native_payload, offset));
                let point = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                let translated_points = transforms
                    .iter()
                    .filter_map(|transform| transform.apply(point))
                    .collect::<HashSet<_>>();
                let marker_loci = translated_points
                    .into_iter()
                    .filter_map(|translated| {
                        let mut marker_loci = loci_by_point
                            .get(&translated)
                            .into_iter()
                            .flatten()
                            .filter(|locus| {
                                geometry_by_entity.get(&locus_entity(locus)).is_some_and(
                                    |geometry| marker_accepts_locus(marker.kind, geometry),
                                )
                            })
                            .map(|locus| {
                                if !qualified_point
                                    && matches!(
                                        marker.kind,
                                        SketchInputKind::LineOrCircle | SketchInputKind::Arc
                                    )
                                {
                                    SketchLocus::Entity(locus_entity(locus))
                                } else {
                                    locus.clone()
                                }
                            })
                            .collect::<Vec<_>>();
                        if marker_loci.is_empty() && marker.kind == SketchInputKind::LineOrCircle {
                            marker_loci.extend(
                                line_midpoints.get(sketch).into_iter().flatten().filter_map(
                                    |(point, locus)| {
                                        (quantize(*point, QUANTUM) == translated)
                                            .then_some(locus.clone())
                                    },
                                ),
                            );
                        }
                        if marker_loci.is_empty()
                            && primary_geometry_locus
                            && marker.kind == SketchInputKind::LineOrCircle
                        {
                            marker_loci.extend(sketch_entities.iter().filter_map(|entity| {
                                if entity.sketch != **sketch {
                                    return None;
                                }
                                let SketchGeometry::Line { start, end } = &entity.geometry else {
                                    return None;
                                };
                                point_on_quantized_segment(
                                    translated,
                                    quantize(*start, QUANTUM),
                                    quantize(*end, QUANTUM),
                                )
                                .then(|| SketchLocus::Entity(entity.id.clone()))
                            }));
                        }
                        marker_loci.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
                        marker_loci.dedup();
                        if qualified_point {
                            canonicalize_physical_loci(&mut marker_loci, sketch_entities, QUANTUM);
                        }
                        (!marker_loci.is_empty()).then_some(marker_loci)
                    })
                    .collect::<Vec<_>>();
                let Some(first) = marker_loci.first() else {
                    continue;
                };
                if !marker_loci.is_empty() && marker_loci.iter().all(|candidate| candidate == first)
                {
                    result.insert(result_key, first.clone());
                }
            }
        }
    }
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    for marker in markers_by_id.values().copied() {
        if marker.kind != SketchInputKind::LineOrCircle || result.contains_key(&marker.id) {
            continue;
        }
        let endpoints = line_endpoint_markers(marker, &markers_by_id);
        let (Some(feature), [first, second]) =
            (marker.feature_ref.as_deref(), endpoints.as_slice())
        else {
            continue;
        };
        let (Some(sketch_id), Some(first), Some(second)) = (
            sketches_by_feature.get(feature),
            first.coordinates_m,
            second.coordinates_m,
        ) else {
            continue;
        };
        let first_native = quantize(
            Point2::new(first[0] * NATIVE_TO_IR, first[1] * NATIVE_TO_IR),
            QUANTUM,
        );
        let second_native = quantize(
            Point2::new(second[0] * NATIVE_TO_IR, second[1] * NATIVE_TO_IR),
            QUANTUM,
        );
        let endpoint_pairs = transforms
            .get(feature)
            .into_iter()
            .flatten()
            .filter_map(|transform| {
                Some((
                    transform.apply(first_native)?,
                    transform.apply(second_native)?,
                ))
            })
            .collect::<HashSet<_>>();
        if endpoint_pairs.is_empty() {
            continue;
        }
        let mut matches = HashSet::new();
        let mut complete = true;
        for (start, end) in endpoint_pairs {
            let candidates = sketch_entities
                .iter()
                .filter(|entity| entity.sketch == **sketch_id)
                .filter_map(|entity| {
                    let SketchGeometry::Line {
                        start: candidate_start,
                        end: candidate_end,
                    } = entity.geometry
                    else {
                        return None;
                    };
                    let candidate_start = quantize(candidate_start, QUANTUM);
                    let candidate_end = quantize(candidate_end, QUANTUM);
                    ((candidate_start == start && candidate_end == end)
                        || (candidate_start == end && candidate_end == start))
                        .then_some(entity.id.clone())
                })
                .collect::<Vec<_>>();
            let [entity] = candidates.as_slice() else {
                complete = false;
                break;
            };
            matches.insert(entity.clone());
        }
        if complete {
            if let [entity] = matches.into_iter().collect::<Vec<_>>().as_slice() {
                result.insert(marker.id.clone(), vec![SketchLocus::Entity(entity.clone())]);
            }
        }
    }
    let entities_by_id = sketch_entities
        .iter()
        .map(|entity| (&entity.id, entity))
        .collect::<HashMap<_, _>>();
    loop {
        let additions = markers_by_id
            .values()
            .filter(|marker| {
                marker.coordinates_m.is_none()
                    && matches!(
                        marker.kind,
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
                    && !result.contains_key(&marker.id)
            })
            .filter_map(|marker| {
                unique_linked_endpoint_locus(
                    marker,
                    &markers_by_id,
                    &result,
                    &entities_by_id,
                    QUANTUM,
                )
                .map(|locus| (marker.id.clone(), vec![locus]))
            })
            .collect::<Vec<_>>();
        if additions.is_empty() {
            break;
        }
        result.extend(additions);
    }
    result
}

pub(super) fn unique_linked_endpoint_locus(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    entities_by_id: &HashMap<&SketchEntityId, &SketchEntity>,
    quantum: f64,
) -> Option<SketchLocus> {
    if marker.links.len() < 2 {
        return None;
    }
    let mut groups = Vec::<HashMap<(i64, i64), Vec<SketchLocus>>>::new();
    let mut sketches = HashSet::new();
    for link in &marker.links {
        let entities = marker_entities(&link.entity_ref, markers_by_id, loci_by_marker);
        if entities.is_empty() {
            return None;
        }
        let mut endpoints = HashMap::<(i64, i64), Vec<SketchLocus>>::new();
        for entity_id in entities {
            let entity = entities_by_id.get(&entity_id)?;
            sketches.insert(&entity.sketch);
            for (point, locus) in sketch_entity_loci(entity) {
                if matches!(
                    locus,
                    SketchLocus::Start(_) | SketchLocus::End(_) | SketchLocus::Entity(_)
                ) {
                    endpoints
                        .entry(quantize(point, quantum))
                        .or_default()
                        .push(locus);
                }
            }
        }
        if endpoints.is_empty() {
            return None;
        }
        groups.push(endpoints);
    }
    if sketches.len() != 1 {
        return None;
    }
    let mut shared = groups[0].keys().copied().collect::<HashSet<_>>();
    for group in &groups[1..] {
        shared.retain(|point| group.contains_key(point));
    }
    let shared = shared.into_iter().collect::<Vec<_>>();
    let [point] = shared.as_slice() else {
        return None;
    };
    let mut loci = groups
        .iter()
        .flat_map(|group| group.get(point).into_iter().flatten().cloned())
        .collect::<Vec<_>>();
    loci.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    loci.dedup();
    loci.into_iter().next()
}

fn point_on_quantized_segment(point: (i64, i64), start: (i64, i64), end: (i64, i64)) -> bool {
    let ab = (
        i128::from(end.0) - i128::from(start.0),
        i128::from(end.1) - i128::from(start.1),
    );
    let ap = (
        i128::from(point.0) - i128::from(start.0),
        i128::from(point.1) - i128::from(start.1),
    );
    let cross = ab.0 * ap.1 - ab.1 * ap.0;
    let projection = ab.0 * ap.0 + ab.1 * ap.1;
    let squared_length = ab.0 * ab.0 + ab.1 * ab.1;
    squared_length != 0 && cross == 0 && (0..=squared_length).contains(&projection)
}

pub(super) fn marker_transform_candidates_by_feature(
    features: &[cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    sketch_entities: &[SketchEntity],
    lanes: &[FeatureInputLane],
) -> HashMap<String, Vec<MarkerTransform>> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch))
        })
        .collect::<HashMap<_, _>>();
    let mut result = HashMap::new();
    for lane in lanes {
        let mut markers_by_feature = HashMap::<&str, Vec<&SketchInputEntity>>::new();
        for marker in &lane.sketch_entities {
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            if marker.coordinates_m.is_some() && sketches_by_feature.contains_key(feature) {
                markers_by_feature.entry(feature).or_default().push(marker);
            }
        }
        for (feature, markers) in markers_by_feature {
            let Some(sketch) = sketches_by_feature.get(feature) else {
                continue;
            };
            if !sketch_entities
                .iter()
                .any(|entity| entity.sketch == **sketch)
            {
                continue;
            }
            let mut directly_bound = HashMap::<(i64, i64), HashSet<(i64, i64)>>::new();
            for marker in &markers {
                let Some([u, v]) = marker.coordinates_m else {
                    continue;
                };
                let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                for entity in sketch_entities.iter().filter(|entity| {
                    entity.sketch == **sketch
                        && entity.native_ref.as_deref() == Some(marker.id.as_str())
                }) {
                    let anchors = match entity.geometry {
                        SketchGeometry::Point { position } => vec![position],
                        _ => marker_geometry_anchors(marker.kind, &entity.geometry),
                    };
                    for anchor in anchors {
                        directly_bound
                            .entry(native)
                            .or_default()
                            .insert(quantize(anchor, QUANTUM));
                    }
                }
            }
            let compatible = |primary_only: bool| {
                let mut points = HashMap::<(i64, i64), HashSet<(i64, i64)>>::new();
                for marker in &markers {
                    if !matches!(
                        marker.kind,
                        SketchInputKind::Point
                            | SketchInputKind::LineOrCircle
                            | SketchInputKind::Arc
                            | SketchInputKind::ConstrainedPoint
                    ) {
                        continue;
                    }
                    let Some([u, v]) = marker.coordinates_m else {
                        continue;
                    };
                    if primary_only
                        && usize::try_from(marker.offset).ok().is_none_or(|offset| {
                            !marker_is_geometry_locus(&lane.native_payload, offset)
                        })
                    {
                        continue;
                    }
                    let marker_point =
                        quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                    let anchors = sketch_entities
                        .iter()
                        .filter(|entity| entity.sketch == **sketch)
                        .flat_map(|entity| {
                            if primary_only {
                                sketch_entity_loci(entity)
                                    .into_iter()
                                    .filter_map(|(point, locus)| {
                                        marker_accepts_locus(marker.kind, &entity.geometry)
                                            .then_some((point, locus))
                                    })
                                    .map(|(point, _)| point)
                                    .collect::<Vec<_>>()
                            } else {
                                marker_geometry_anchors(marker.kind, &entity.geometry)
                            }
                        });
                    for point in anchors {
                        points
                            .entry(marker_point)
                            .or_default()
                            .insert(quantize(point, QUANTUM));
                    }
                }
                points
            };
            let direct = compatible_marker_transform_candidates(&directly_bound);
            let primary = compatible_marker_transform_candidates(&compatible(true));
            let fallback = compatible_marker_transform_candidates(&compatible(false));
            let candidates = if direct.len() == 1 {
                direct
            } else if primary.len() == 1 || fallback.is_empty() {
                primary
            } else {
                fallback
            };
            let candidates = sketches
                .iter()
                .find(|candidate| candidate.id == **sketch)
                .map_or(candidates.clone(), |sketch| {
                    marker_transforms_with_frame_fallback(&candidates, sketch, QUANTUM)
                });
            if !candidates.is_empty() {
                result.insert(feature.to_string(), candidates);
            }
        }
    }
    result
}

fn marker_geometry_anchors(kind: SketchInputKind, geometry: &SketchGeometry) -> Vec<Point2> {
    match (kind, geometry) {
        (
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint,
            SketchGeometry::Point { position },
        ) => vec![*position],
        (
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint,
            SketchGeometry::Line { start, end },
        ) => vec![*start, *end],
        (
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint,
            SketchGeometry::Circle { center, .. }
            | SketchGeometry::Arc { center, .. }
            | SketchGeometry::Ellipse { center, .. },
        ) => vec![*center],
        (SketchInputKind::LineOrCircle, SketchGeometry::Line { start, end }) => {
            vec![
                *start,
                *end,
                Point2::new((start.u + end.u) * 0.5, (start.v + end.v) * 0.5),
            ]
        }
        (
            SketchInputKind::LineOrCircle,
            SketchGeometry::Circle { center, .. } | SketchGeometry::Ellipse { center, .. },
        )
        | (SketchInputKind::Arc, SketchGeometry::Arc { center, .. }) => vec![*center],
        _ => Vec::new(),
    }
}

pub(super) fn marker_accepts_locus(kind: SketchInputKind, geometry: &SketchGeometry) -> bool {
    match kind {
        SketchInputKind::Arc => matches!(geometry, SketchGeometry::Arc { .. }),
        SketchInputKind::LineOrCircle => matches!(
            geometry,
            SketchGeometry::Line { .. }
                | SketchGeometry::Circle { .. }
                | SketchGeometry::Ellipse { .. }
        ),
        SketchInputKind::Point
        | SketchInputKind::ConstrainedPoint
        | SketchInputKind::Relation(_)
        | SketchInputKind::Native(_) => true,
    }
}

#[cfg(test)]
mod relation_loci_tests;
