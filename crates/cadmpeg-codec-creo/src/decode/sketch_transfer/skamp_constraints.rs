// SPDX-License-Identifier: Apache-2.0
//! SKAMP solver constraint emission and locus compatibility.

use super::super::sketch_ids::{sketch_constraint_id, sketch_entity_id, sketch_native_ref};
use crate::decode::sketch_transfer::identity::section_entity_external_ids;
use crate::decode::sketch_transfer::loci::{
    section_skamp_active, section_skamp_center_entity, section_skamp_circular_entity,
    section_skamp_curve_entity, section_skamp_incidence_locus, section_skamp_is_arc,
    section_skamp_is_line, section_skamp_is_point, section_skamp_line_pair, section_skamp_locus,
    section_skamp_midpoint, section_skamp_oriented_line, section_skamp_point_locus,
    section_skamp_same_coordinate, section_skamp_same_coordinate_axis, section_skamp_tangent_loci,
    unique_bounded_curve_segment,
};
use crate::feature::definitions::SolverSubtable;
use cadmpeg_ir::scalar::PositiveAngle;
use cadmpeg_ir::sketches::{
    NativeOperandField, SketchConstraint, SketchConstraintDefinitionInput, SketchCoordinateAxis,
    SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus,
    SketchNativeOperand,
};
use std::collections::BTreeMap;

pub(in super::super) fn section_skamp_constraints_for_geometry(
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Vec<(SketchConstraint, usize)> {
    let Some(relations) = &definition.relations else {
        return Vec::new();
    };
    let complete_skamps = relations
        .skamps
        .as_ref()
        .is_none_or(SolverSubtable::is_complete);
    let skamp_id_counts =
        relations
            .skamps()
            .iter()
            .fold(BTreeMap::<u32, usize>::new(), |mut counts, skamp| {
                *counts.entry(skamp.id).or_default() += 1;
                counts
            });
    let section_entities = section_entity_external_ids(definition);
    let available_entities = geometry.map_or_else(
        || section_entities.clone(),
        |geometry| {
            relations
                .skamps()
                .iter()
                .flat_map(|skamp| &skamp.items)
                .map(|item| item.entity_id)
                .filter(|entity_id| {
                    sketch_entity_id(sketch, *entity_id)
                        .is_some_and(|id| geometry.contains_key(&id))
                })
                .collect()
        },
    );
    relations
        .skamps()
        .iter()
        .filter_map(|skamp| {
            let unique_skamp_id = complete_skamps && skamp_id_counts.get(&skamp.id) == Some(&1);
            let joined_equation_id = if unique_skamp_id
                && relations
                    .triples
                    .as_ref()
                    .is_none_or(SolverSubtable::is_complete)
            {
                let mut equation_ids = relations
                    .triples()
                    .iter()
                    .filter(|triple| triple.skamp_id == Some(skamp.id))
                    .filter_map(|triple| triple.equation_id);
                let equation_id = equation_ids.next();
                equation_id.filter(|_| equation_ids.next().is_none())
            } else {
                None
            };
            let active = section_skamp_active(skamp.status);
            let native_constraint = || {
                let native_ref = sketch_native_ref(sketch);
                let skamp_kind = cadmpeg_core::text::NonBlankString::new("skamp_ptr")?;
                let item_field = cadmpeg_core::text::NonBlankString::new("items.entity_id")?;
                let entities = skamp
                    .items
                    .iter()
                    .filter(|item| available_entities.contains(&item.entity_id))
                    .filter_map(|item| sketch_entity_id(sketch, item.entity_id))
                    .collect::<Vec<_>>();
                let mut operands = skamp
                    .items
                    .iter()
                    .map(|item| SketchNativeOperand {
                        native_kind: skamp_kind.clone(),
                        field: Some(NativeOperandField {
                            name: item_field.clone(),
                            role: Some(item.sense),
                        }),
                        object_index: Some(item.entity_id),
                        native_ref: Some(native_ref.clone()),
                    })
                    .collect::<Vec<_>>();
                if let Some(equation_id) = joined_equation_id {
                    let triples_kind = cadmpeg_core::text::NonBlankString::new("triples_ptr")?;
                    let equation_field = cadmpeg_core::text::NonBlankString::new("equation_id")?;
                    operands.push(SketchNativeOperand {
                        native_kind: triples_kind,
                        field: Some(NativeOperandField {
                            name: equation_field,
                            role: None,
                        }),
                        object_index: Some(equation_id),
                        native_ref: Some(native_ref),
                    });
                }
                Some(SketchConstraintDefinitionInput::Native {
                    native_kind: cadmpeg_core::text::NonBlankString::new(format!(
                        "creo:skamp:{}",
                        skamp.kind
                    ))?,
                    native_state: Some(u64::from(skamp.status)),
                    native_flags: Some(u64::from(skamp.flags)),
                    native_properties: if unique_skamp_id {
                        BTreeMap::new()
                    } else {
                        BTreeMap::from([("id".to_string(), skamp.id.to_string())])
                    },
                    entities,
                    parameter: None,
                    operands,
                })
            };
            let item_geometry = |item: &crate::feature::definitions::FeatureSkampItem| {
                let entity = sketch_entity_id(sketch, item.entity_id)?;
                geometry?.get(&entity)
            };
            let inactive_curve_entity = |item: &crate::feature::definitions::FeatureSkampItem| {
                (!active
                    && item.sense == 0
                    && item_geometry(item).is_some_and(|geometry| {
                        matches!(
                            geometry.definition(),
                            SketchGeometryDefinition::Line { .. }
                                | SketchGeometryDefinition::ReferenceLine { .. }
                                | SketchGeometryDefinition::Circle { .. }
                                | SketchGeometryDefinition::Arc { .. }
                                | SketchGeometryDefinition::Nurbs { .. }
                        ) || matches!((
                            geometry).definition(),
                            SketchGeometryDefinition::Native { native_kind }
                                if matches!(
                                    native_kind.as_str(),
                                    "line_or_arc" | "line" | "arc" | "circle" | "spline"
                                )
                        )
                    }))
                .then(|| sketch_entity_id(sketch, item.entity_id))
                .flatten()
            };
            let inactive_incidence_locus =
                |item: &crate::feature::definitions::FeatureSkampItem| {
                    section_skamp_incidence_locus(definition, sketch, item, geometry).or_else(
                        || {
                            (!active
                                && item.sense == 4
                                && item_geometry(item).is_some_and(|geometry| {
                                    matches!(
                                        geometry.definition(),
                                        SketchGeometryDefinition::Circle { .. }
                                            | SketchGeometryDefinition::Arc { .. }
                                    ) || matches!((
                                        geometry).definition(),
                                        SketchGeometryDefinition::Native { native_kind }
                                            if matches!(native_kind.as_str(), "arc" | "circle")
                                    )
                                }))
                            .then(|| {
                                sketch_entity_id(sketch, item.entity_id).map(SketchLocus::Center)
                            })
                            .flatten()
                        },
                    )
                };
            let point_entity = |item: &crate::feature::definitions::FeatureSkampItem| {
                (item.sense == 0).then_some(())?;
                if section_skamp_is_point(definition, item) {
                    return sketch_entity_id(sketch, item.entity_id);
                }
                (!active && item_geometry(item).is_some_and(|geometry| {
                    matches!(
                        geometry.definition(),
                        SketchGeometryDefinition::Point { .. }
                    ) || matches!((
                        geometry).definition(),
                        SketchGeometryDefinition::Native { native_kind } if native_kind == "point"
                    )
                }))
                .then(|| sketch_entity_id(sketch, item.entity_id))
                .flatten()
            };
            let inactive_point_locus = |item: &crate::feature::definitions::FeatureSkampItem| {
                section_skamp_point_locus(definition, sketch, item)
                    .or_else(|| point_entity(item).map(SketchLocus::Entity))
                    .or_else(|| inactive_incidence_locus(item))
            };
            let mut constraint_definition = if unique_skamp_id {
                match (skamp.kind, skamp.items.as_slice()) {
                    (0, [first, second])
                        if section_skamp_center_entity(definition, sketch, first).is_some()
                            && section_skamp_center_entity(definition, sketch, second)
                                .is_some() =>
                    {
                        SketchConstraintDefinitionInput::Concentric {
                            first: section_skamp_center_entity(definition, sketch, first)?,
                            second: section_skamp_center_entity(definition, sketch, second)?,
                        }
                    }
                    (0, [first, second])
                        if section_skamp_incidence_locus(definition, sketch, first, geometry)
                            .is_some()
                            && section_skamp_incidence_locus(
                                definition, sketch, second, geometry,
                            )
                            .is_some() =>
                    {
                        SketchConstraintDefinitionInput::CoincidentLoci {
                            loci: vec![
                                section_skamp_incidence_locus(definition, sketch, first, geometry)?,
                                section_skamp_incidence_locus(
                                    definition, sketch, second, geometry,
                                )?,
                            ],
                        }
                    }
                    (3, [first, second]) => {
                        if let (Some(first), Some(second)) =
                            (point_entity(first), point_entity(second))
                        {
                            SketchConstraintDefinitionInput::CoincidentLoci {
                                loci: vec![SketchLocus::Entity(first), SketchLocus::Entity(second)],
                            }
                        } else {
                            let directed = [(first, second), (second, first)];
                            let point_on_curve = directed
                                .into_iter()
                                .filter_map(|(curve, point)| {
                                    Some((
                                        section_skamp_curve_entity(definition, sketch, curve)
                                            .or_else(|| inactive_curve_entity(curve))?,
                                        inactive_incidence_locus(point)?,
                                    ))
                                })
                                .collect::<Vec<_>>();
                            if let [(entity, point)] = point_on_curve.as_slice() {
                                SketchConstraintDefinitionInput::PointOnObject {
                                    point: point.clone(),
                                    entity: entity.clone(),
                                }
                            } else {
                                let point_coincidence = directed
                                    .into_iter()
                                    .filter_map(|(point, locus)| {
                                        Some([
                                            SketchLocus::Entity(point_entity(point)?),
                                            inactive_incidence_locus(locus)?,
                                        ])
                                    })
                                    .collect::<Vec<_>>();
                                if let [loci] = point_coincidence.as_slice() {
                                    SketchConstraintDefinitionInput::CoincidentLoci {
                                        loci: loci.to_vec(),
                                    }
                                } else {
                                    native_constraint()?
                                }
                            }
                        }
                    }
                    (kind @ (1 | 2), [item]) => {
                        match section_skamp_oriented_line(definition, sketch, item, geometry) {
                            Some(entity) if kind == 1 => {
                                SketchConstraintDefinitionInput::Horizontal { entity }
                            }
                            Some(entity) => SketchConstraintDefinitionInput::Vertical { entity },
                            None => native_constraint()?,
                        }
                    }
                    (4, [first, second]) => {
                        if let Some([first, second]) = section_skamp_tangent_loci(
                            definition, sketch, first, second, active, geometry,
                        ) {
                            SketchConstraintDefinitionInput::TangentLoci { first, second }
                        } else if section_skamp_curve_entity(definition, sketch, first).is_some()
                            && section_skamp_curve_entity(definition, sketch, second).is_some()
                        {
                            SketchConstraintDefinitionInput::Tangent {
                                first: section_skamp_curve_entity(definition, sketch, first)?,
                                second: section_skamp_curve_entity(definition, sketch, second)?,
                            }
                        } else {
                            native_constraint()?
                        }
                    }
                    (5, [first, second]) => {
                        match (
                            section_skamp_curve_entity(definition, sketch, first),
                            section_skamp_curve_entity(definition, sketch, second),
                        ) {
                            (Some(first), Some(second)) => {
                                SketchConstraintDefinitionInput::Perpendicular { first, second }
                            }
                            _ => native_constraint()?,
                        }
                    }
                    (6, [first, second])
                        if section_skamp_circular_entity(definition, sketch, first).is_some()
                            && section_skamp_circular_entity(definition, sketch, second)
                                .is_some() =>
                    {
                        SketchConstraintDefinitionInput::Equal {
                            first: section_skamp_circular_entity(definition, sketch, first)?,
                            second: section_skamp_circular_entity(definition, sketch, second)?,
                        }
                    }
                    (7, [first, second])
                        if section_skamp_line_pair(definition, sketch, first, second).is_some() =>
                    {
                        let [first, second] =
                            section_skamp_line_pair(definition, sketch, first, second)?;
                        SketchConstraintDefinitionInput::Parallel { first, second }
                    }
                    (8, [first, second])
                        if section_skamp_line_pair(definition, sketch, first, second).is_some() =>
                    {
                        let [first, second] =
                            section_skamp_line_pair(definition, sketch, first, second)?;
                        SketchConstraintDefinitionInput::Equal { first, second }
                    }
                    (9, [first, second])
                        if section_skamp_line_pair(definition, sketch, first, second).is_some() =>
                    {
                        let [first, second] =
                            section_skamp_line_pair(definition, sketch, first, second)?;
                        SketchConstraintDefinitionInput::Collinear { first, second }
                    }
                    (9, [first, second])
                        if first.sense == 0
                            && second.sense == 0
                            && ((section_skamp_is_line(definition, first)
                                && section_skamp_is_point(definition, second))
                                || (section_skamp_is_point(definition, first)
                                    && section_skamp_is_line(definition, second))) =>
                    {
                        let (line, point) = if section_skamp_is_line(definition, first) {
                            (first, second)
                        } else {
                            (second, first)
                        };
                        SketchConstraintDefinitionInput::PointOnObject {
                            point: section_skamp_locus(definition, sketch, point)?,
                            entity: sketch_entity_id(sketch, line.entity_id)?,
                        }
                    }
                    (kind @ (10 | 11), [item])
                        if item.sense == 0 && section_skamp_is_arc(definition, item) =>
                    {
                        SketchConstraintDefinitionInput::ArcAngle {
                            entity: sketch_entity_id(sketch, item.entity_id)?,
                            angle: if kind == 10 {
                                PositiveAngle::QUARTER_TURN
                            } else {
                                PositiveAngle::HALF_TURN
                            },
                        }
                    }
                    (kind @ (12 | 13), [item])
                        if item.sense == 0 && section_skamp_is_arc(definition, item) =>
                    {
                        let entity = sketch_entity_id(sketch, item.entity_id)?;
                        let first = SketchLocus::Start(entity.clone());
                        let second = SketchLocus::End(entity);
                        let axis = if kind == 12 {
                            SketchCoordinateAxis::V
                        } else {
                            SketchCoordinateAxis::U
                        };
                        SketchConstraintDefinitionInput::SameCoordinate {
                            relation: cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                                first, second, axis,
                            )
                            .ok()?,
                        }
                    }
                    (37, [source, result])
                        if source.sense == 0
                            && result.sense == 0
                            && source
                                .entity_id
                                .checked_add(1)
                                .is_some_and(|expected| expected == result.entity_id)
                            && definition
                                .trim_entities
                                .iter()
                                .filter(|table| table.has_unique_external_ids())
                                .flat_map(|table| &table.rows)
                                .any(|row| row.external_id == result.entity_id) =>
                    {
                        let source = sketch_entity_id(sketch, source.entity_id)?;
                        let result = sketch_entity_id(sketch, result.entity_id)?;
                        let geometry_agrees = geometry.is_none_or(|geometry| {
                            geometry
                                .get(&source)
                                .zip(geometry.get(&result))
                                .is_none_or(|(source, result)| source == result)
                        });
                        if geometry_agrees {
                            SketchConstraintDefinitionInput::ProjectedCopy { source, result }
                        } else {
                            native_constraint()?
                        }
                    }
                    (33, [item])
                        if skamp.flags == 34
                            && item.sense == 10
                            && unique_bounded_curve_segment(definition, item.entity_id)
                                .is_some() =>
                    {
                        SketchConstraintDefinitionInput::Fixed {
                            entity: sketch_entity_id(sketch, item.entity_id)?,
                        }
                    }
                    (14, [axis, first, second])
                        if axis.sense == 0
                            && section_skamp_is_line(definition, axis)
                            && section_skamp_point_locus(definition, sketch, first).is_some()
                            && section_skamp_point_locus(definition, sketch, second).is_some() =>
                    {
                        SketchConstraintDefinitionInput::Symmetric {
                            first: section_skamp_point_locus(definition, sketch, first)?,
                            second: section_skamp_point_locus(definition, sketch, second)?,
                            axis: sketch_entity_id(sketch, axis.entity_id)?,
                        }
                    }
                    (14, [center, first, second])
                        if point_entity(center).is_some()
                            && inactive_point_locus(first).is_some()
                            && inactive_point_locus(second).is_some() =>
                    {
                        SketchConstraintDefinitionInput::PointSymmetric {
                            first: inactive_point_locus(first)?,
                            second: inactive_point_locus(second)?,
                            center: SketchLocus::Entity(point_entity(center)?),
                        }
                    }
                    (15 | 17 | 30 | 31, [_, _]) => {
                        if let Some((first, second, axis)) =
                            section_skamp_same_coordinate(definition, sketch, skamp, active)
                        {
                            SketchConstraintDefinitionInput::SameCoordinate {
                                relation: cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                                    first, second, axis,
                                )
                                .ok()?,
                            }
                        } else if !active {
                            match skamp.items.as_slice() {
                                [first, second] => match (
                                    inactive_point_locus(first),
                                    inactive_point_locus(second),
                                    section_skamp_same_coordinate_axis(skamp),
                                ) {
                                    (Some(first), Some(second), Some(axis)) => {
                                        match cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                                            first,
                                            second,
                                            [SketchCoordinateAxis::U, SketchCoordinateAxis::V]
                                                [axis.index()],
                                        ) {
                                            Ok(relation) => {
                                                SketchConstraintDefinitionInput::SameCoordinate {
                                                    relation,
                                                }
                                            }
                                            Err(_) => native_constraint()?,
                                        }
                                    }
                                    _ => native_constraint()?,
                                },
                                _ => native_constraint()?,
                            }
                        } else {
                            native_constraint()?
                        }
                    }
                    (35, [first, second]) => {
                        if let Some((point, entity)) =
                            section_skamp_midpoint(definition, sketch, first, second, geometry)
                        {
                            SketchConstraintDefinitionInput::Midpoint { point, entity }
                        } else {
                            native_constraint()?
                        }
                    }
                    _ => native_constraint()?,
                }
            } else {
                native_constraint()?
            };
            if geometry.is_some_and(|geometry| {
                !sketch_constraint_loci_compatible_with_policy(
                    &constraint_definition,
                    geometry,
                    !active,
                )
            }) {
                constraint_definition = native_constraint()?;
            }
            Some((
                SketchConstraint {
                    id: if unique_skamp_id {
                        sketch_constraint_id(sketch, format_args!("skamp:{}", skamp.id))?
                    } else {
                        sketch_constraint_id(sketch, format_args!("skamp:offset:{}", skamp.offset))?
                    },
                    sketch: sketch.clone(),
                    definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                        constraint_definition,
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
                    native_ref: Some(sketch_native_ref(sketch)),
                },
                skamp.offset,
            ))
        })
        .collect()
}

#[cfg(test)]
pub(in super::super) fn sketch_constraint_loci_compatible(
    definition: &SketchConstraintDefinitionInput,
    geometry: &BTreeMap<SketchEntityId, SketchGeometry>,
) -> bool {
    sketch_constraint_loci_compatible_with_policy(definition, geometry, false)
}

fn sketch_constraint_loci_compatible_with_policy(
    definition: &SketchConstraintDefinitionInput,
    geometry: &BTreeMap<SketchEntityId, SketchGeometry>,
    allow_unknown_native_endpoints: bool,
) -> bool {
    let native_line_center_allowed = matches!(
        definition,
        SketchConstraintDefinitionInput::Midpoint {
            point: SketchLocus::Center(_),
            ..
        }
    );
    let locus_compatible = |locus: &SketchLocus| {
        let entity = match locus {
            SketchLocus::Entity(entity)
            | SketchLocus::Start(entity)
            | SketchLocus::End(entity)
            | SketchLocus::Center(entity) => entity,
        };
        geometry.get(entity).is_some_and(|geometry| match locus {
            SketchLocus::Entity(_) => true,
            SketchLocus::Start(_) | SketchLocus::End(_) => {
                !matches!(
                    geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                        | SketchGeometryDefinition::Circle { .. }
                ) && !matches!((
                        geometry).definition(),
                        SketchGeometryDefinition::Native { native_kind }
                            if !(matches!(
                                native_kind.as_str(),
                                "bounded_curve" | "line_or_arc" | "line" | "arc" | "spline"
                            ) || allow_unknown_native_endpoints
                                && native_kind == "solver_only_section_entity")
                )
            }
            SketchLocus::Center(_) => {
                matches!(
                    geometry.definition(),
                    SketchGeometryDefinition::Circle { .. }
                        | SketchGeometryDefinition::Arc { .. }
                        | SketchGeometryDefinition::Ellipse { .. }
                ) || matches!((
                    geometry).definition(),
                    SketchGeometryDefinition::Native { native_kind }
                        if matches!(native_kind.as_str(), "circle" | "arc")
                            // A centered type-47 row retains its center on a native line.
                            || native_line_center_allowed && native_kind == "line"
                )
            }
        })
    };
    let loci_compatible = match definition {
        SketchConstraintDefinitionInput::CoincidentLoci { loci }
        | SketchConstraintDefinitionInput::Group { elements: loci }
        | SketchConstraintDefinitionInput::Text { elements: loci, .. } => {
            loci.iter().all(locus_compatible)
        }
        SketchConstraintDefinitionInput::SameCoordinate { relation } => {
            locus_compatible(relation.first()) && locus_compatible(relation.second())
        }
        SketchConstraintDefinitionInput::TangentLoci { first, second }
        | SketchConstraintDefinitionInput::DistanceLoci { first, second, .. }
        | SketchConstraintDefinitionInput::DistanceLociValue { first, second, .. }
        | SketchConstraintDefinitionInput::MidpointCoordinate { first, second, .. }
        | SketchConstraintDefinitionInput::HorizontalDistance { first, second, .. }
        | SketchConstraintDefinitionInput::VerticalDistance { first, second, .. } => {
            locus_compatible(first) && locus_compatible(second)
        }
        SketchConstraintDefinitionInput::Midpoint { point, entity }
        | SketchConstraintDefinitionInput::PointOnObject { point, entity } => {
            locus_compatible(point) && geometry.contains_key(entity)
        }
        SketchConstraintDefinitionInput::PointCoordinateValues { point, .. } => {
            locus_compatible(point)
        }
        SketchConstraintDefinitionInput::Symmetric {
            first,
            second,
            axis,
        } => locus_compatible(first) && locus_compatible(second) && geometry.contains_key(axis),
        SketchConstraintDefinitionInput::PointSymmetric {
            first,
            second,
            center,
        } => locus_compatible(first) && locus_compatible(second) && locus_compatible(center),
        SketchConstraintDefinitionInput::SnellsLaw {
            incident,
            refracted,
            interface,
            ..
        } => {
            locus_compatible(incident)
                && locus_compatible(refracted)
                && geometry.contains_key(interface)
        }
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
            geometry.contains_key(first) && geometry.contains_key(second)
        }
        SketchConstraintDefinitionInput::Horizontal { entity }
        | SketchConstraintDefinitionInput::Vertical { entity }
        | SketchConstraintDefinitionInput::Fixed { entity }
        | SketchConstraintDefinitionInput::Radius { entity, .. }
        | SketchConstraintDefinitionInput::Diameter { entity, .. }
        | SketchConstraintDefinitionInput::ArcAngle { entity, .. }
        | SketchConstraintDefinitionInput::EllipseAngle { entity, .. } => {
            geometry.contains_key(entity)
        }
        SketchConstraintDefinitionInput::AtIntersection {
            point,
            first,
            second,
        } => {
            locus_compatible(point) && geometry.contains_key(first) && geometry.contains_key(second)
        }
        _ => true,
    };
    // A relation whose entity kind the IR refuses retains its native form.
    loci_compatible
        && definition
            .entity_kind_restriction()
            .is_none_or(|(entity, restriction)| {
                geometry
                    .get(entity)
                    .is_some_and(|geometry| restriction.admits(geometry.definition()))
            })
}

#[cfg(test)]
mod tests {
    use super::sketch_constraint_loci_compatible_with_policy;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput, SketchEntityId, SketchGeometry, SketchGeometryDefinition,
        SketchLocus,
    };
    use std::collections::BTreeMap;

    #[test]
    fn typed_entity_relations_require_every_entity_in_the_emitted_geometry() {
        let first =
            SketchEntityId::mint("synthetic:test:relation#first").expect("valid test fixture");
        let second =
            SketchEntityId::mint("synthetic:test:relation#second").expect("valid test fixture");
        let axis =
            SketchEntityId::mint("synthetic:test:relation#axis").expect("valid test fixture");
        let geometry = BTreeMap::from([
            (
                first.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(0.0, 0.0),
                })
                .expect("valid test fixture"),
            ),
            (
                second.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(1.0, 0.0),
                })
                .expect("valid test fixture"),
            ),
        ]);
        let symmetry = SketchConstraintDefinitionInput::Symmetric {
            first: SketchLocus::Entity(first.clone()),
            second: SketchLocus::Entity(second.clone()),
            axis: axis.clone(),
        };
        assert!(!sketch_constraint_loci_compatible_with_policy(
            &symmetry, &geometry, false,
        ));

        let projected = SketchConstraintDefinitionInput::ProjectedCopy {
            source: first.clone(),
            result: axis.clone(),
        };
        assert!(!sketch_constraint_loci_compatible_with_policy(
            &projected, &geometry, false,
        ));

        let mut complete = geometry;
        complete.insert(
            axis.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
                origin: Point2::new(0.0, 0.0),
                direction: Point2::new(0.0, 1.0),
            })
            .expect("valid test fixture"),
        );
        assert!(sketch_constraint_loci_compatible_with_policy(
            &symmetry, &complete, false,
        ));
        assert!(sketch_constraint_loci_compatible_with_policy(
            &projected, &complete, false,
        ));
    }

    #[test]
    fn midpoint_and_arc_angle_targets_admit_only_their_neutral_entity_kinds() {
        use cadmpeg_ir::scalar::{Angle, Length, PositiveAngle};

        let entity = |name: &str| {
            SketchEntityId::mint(format!("synthetic:test:target#{name}"))
                .expect("valid test fixture")
        };
        let target = entity("target");
        let point = entity("point");
        let with_target = |definition: SketchGeometryDefinition| {
            BTreeMap::from([
                (
                    point.clone(),
                    SketchGeometry::try_from(SketchGeometryDefinition::Point {
                        position: Point2::new(0.0, 0.0),
                    })
                    .expect("valid test fixture"),
                ),
                (
                    target.clone(),
                    SketchGeometry::try_from(definition).expect("valid test fixture"),
                ),
            ])
        };
        let ellipse = |bounds| SketchGeometryDefinition::Ellipse {
            center: Point2::new(0.0, 0.0),
            major_angle: Angle::ZERO,
            major_radius: Length::new(2.0).expect("valid test fixture"),
            minor_radius: Length::new(1.0).expect("valid test fixture"),
            bounds,
        };
        let point_target = with_target(SketchGeometryDefinition::Point {
            position: Point2::new(1.0, 0.0),
        });
        let line = with_target(SketchGeometryDefinition::Line {
            start: Point2::new(-1.0, 0.0),
            end: Point2::new(1.0, 0.0),
        });
        let reference_line = with_target(SketchGeometryDefinition::ReferenceLine {
            origin: Point2::new(-1.0, 0.0),
            direction: Point2::new(2.0, 0.0),
        });
        let native_line = with_target(SketchGeometryDefinition::Native {
            native_kind: cadmpeg_core::text::NonBlankString::new("reference_line")
                .expect("valid test fixture"),
        });
        let arc = with_target(SketchGeometryDefinition::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(1.0).expect("valid test fixture"),
            start_angle: Angle::new(0.0).expect("valid test fixture"),
            end_angle: Angle::new(1.0).expect("valid test fixture"),
        });
        let circle = with_target(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(1.0).expect("valid test fixture"),
        });
        let full_ellipse = with_target(ellipse(None));
        let bounded_ellipse = with_target(ellipse(Some([
            Angle::ZERO,
            Angle::new(1.0).expect("valid test fixture"),
        ])));

        let midpoint = SketchConstraintDefinitionInput::Midpoint {
            point: SketchLocus::Entity(point.clone()),
            entity: target.clone(),
        };
        let arc_angle = SketchConstraintDefinitionInput::ArcAngle {
            entity: target.clone(),
            angle: PositiveAngle::QUARTER_TURN,
        };
        let ellipse_angle = SketchConstraintDefinitionInput::EllipseAngle {
            entity: target.clone(),
            angle: PositiveAngle::QUARTER_TURN,
        };
        // Each row states whether the midpoint, arc-angle and ellipse-angle
        // relations admit the target geometry.
        for (geometry, admitted) in [
            (&point_target, [false, false, false]),
            (&line, [true, false, false]),
            (&reference_line, [false, false, false]),
            (&native_line, [true, true, true]),
            (&arc, [true, true, false]),
            (&circle, [false, false, false]),
            (&full_ellipse, [false, false, false]),
            (&bounded_ellipse, [true, false, true]),
        ] {
            for (relation, admitted) in [&midpoint, &arc_angle, &ellipse_angle]
                .into_iter()
                .zip(admitted)
            {
                let compatible =
                    sketch_constraint_loci_compatible_with_policy(relation, geometry, false);
                assert_eq!(compatible, admitted, "{relation:?} on {geometry:?}");
                let (restricted, restriction) = relation
                    .entity_kind_restriction()
                    .expect("a restricted relation");
                assert_eq!(restricted, &target);
                let target_geometry = geometry.get(&target).expect("target geometry");
                assert_eq!(
                    compatible,
                    restriction.admits(target_geometry.definition()),
                    "the Creo gate and the IR rule disagree on {relation:?} on {geometry:?}"
                );
            }
        }
    }

    /// Decode one `FeatDefs` section. Its skamp table starts with a type-35
    /// incidence between entity 42 and the type-5 point entity 43 at section
    /// point 9, followed by `further_skamps`, each closed by the table trailer.
    fn decode_type35_section(
        variables: &[(u8, u8, &[u8])],
        target_row: [u8; 12],
        further_skamps: &[&[u8]],
    ) -> cadmpeg_ir::codec::DecodeResult {
        use cadmpeg_ir::codec::{Codec, DecodeOptions};

        let mut payload = b"feat_defs_40\0var_arr\0\xf8".to_vec();
        payload.push(u8::try_from(variables.len()).expect("small test fixture"));
        payload.extend_from_slice(b"\xf7\x01\xfb\xe2schema\xf1\xf7\x01\xe2");
        for (uvar, (variable_type, point, value)) in (1_u8..).zip(variables) {
            payload.extend_from_slice(&[*variable_type, *point]);
            payload.extend_from_slice(value);
            payload.extend_from_slice(&[0x0f, 1, 0, uvar, 0xe2]);
        }
        payload.extend_from_slice(b"segtab_ptr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf2\xf7\x01\xe2");
        payload.extend_from_slice(&target_row);
        payload.extend_from_slice(&[0xe2, 0xe3]);
        payload.extend_from_slice(&[5, 0, 0, 0, 9, 0xf6, 0xf6, 0, 0xf6, 0xf6, 0xf6, 43, 0xe2]);
        payload.extend_from_slice(b"relat_ptr\0\xf8\x01\xf7\x6a\xfb\xe2skamp_ptr\0\xf3\xf8");
        payload.push(u8::try_from(further_skamps.len() + 1).expect("small test fixture"));
        payload.extend_from_slice(
            b"\xf7\x6b\xfb\xe2\
              \xe0\x01id\0\x05\xe0\x01type\0\x23\xe0\x01flags\0\x00\
              \xe0\x01status\0\x01\xe0\x01items\0\xf8\x02\xf7\x6c\xfb\xe2\
              \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x00\xf1\xf7\x6c\xe2\
              \x2b\x00\xf3\xf7\x6b\xe2",
        );
        for skamp in further_skamps {
            payload.extend_from_slice(skamp);
            payload.extend_from_slice(b"\xf3\xf7\x6b\xe2");
        }
        payload.extend_from_slice(b"dimtab_ptr\0");
        crate::CreoCodec
            .decode(
                &mut std::io::Cursor::new(crate::test_support::build_prt(
                    "c",
                    &[("FeatDefs", payload)],
                )),
                &DecodeOptions::default(),
            )
            .expect("decode")
    }

    const X: [u8; 8] = [0x46, 0x08, 0, 0, 0, 0, 0, 0];
    /// A type-25 section-reference row with external identifier 42 and the
    /// endpoint references 7 and 8.
    const REFERENCE_LINE_ROW: [u8; 12] = [25, 0, 0, 0, 7, 8, 0xf6, 0, 0xf6, 0xf6, 0xf6, 42];

    /// Assert that the type-35 incidence retains its native form over the
    /// target entity and the point entity, and that the document validates.
    fn assert_type35_retains_its_native_form(
        result: &cadmpeg_ir::codec::DecodeResult,
        target: &SketchEntityId,
    ) {
        let model = &result.ir().model;
        let point = model
            .sketch_entities
            .iter()
            .find(|entity| {
                matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                )
            })
            .expect("solved point");
        let [relation] = model
            .sketch_constraints
            .iter()
            .filter(|constraint| constraint.id.as_str().ends_with(":skamp:5"))
            .collect::<Vec<_>>()[..]
        else {
            panic!("one type-35 relation: {:#?}", model.sketch_constraints);
        };
        let SketchConstraintDefinitionInput::Native {
            native_kind,
            entities,
            ..
        } = relation.definition.kind()
        else {
            panic!("a native type-35 relation: {relation:#?}");
        };
        assert_eq!(native_kind, "creo:skamp:35");
        assert_eq!(entities, &vec![target.clone(), point.id().clone()]);
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
        assert!(validation.is_ok(), "{validation:#?}");
    }

    #[test]
    fn a_midpoint_incidence_on_a_solved_reference_line_retains_its_native_form() {
        let result = decode_type35_section(
            &[
                (1, 7, &[0xe4]),
                (2, 7, &[0xe4]),
                (1, 8, &[0xe4]),
                (2, 8, &X),
                (1, 9, &X),
                (2, 9, &[0xe4]),
            ],
            REFERENCE_LINE_ROW,
            &[],
        );
        let model = &result.ir().model;
        let reference_line = model
            .sketch_entities
            .iter()
            .find(|entity| {
                matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::ReferenceLine { .. }
                )
            })
            .expect("solved reference line");
        let point = model
            .sketch_entities
            .iter()
            .find(|entity| {
                matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                )
            })
            .expect("solved point");
        let [relation] = model
            .sketch_constraints
            .iter()
            .filter(|constraint| {
                !matches!(
                    constraint.definition.kind(),
                    SketchConstraintDefinitionInput::Native { native_kind, .. }
                        if native_kind == "creo:segtab:verhor"
                )
            })
            .collect::<Vec<_>>()[..]
        else {
            panic!("one solver relation: {:#?}", model.sketch_constraints);
        };
        let SketchConstraintDefinitionInput::Native {
            native_kind,
            entities,
            ..
        } = relation.definition.kind()
        else {
            panic!("type-35 relation on a reference line: {relation:#?}");
        };
        assert_eq!(native_kind, "creo:skamp:35");
        assert_eq!(
            entities,
            &vec![reference_line.id().clone(), point.id().clone()]
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
        assert!(validation.is_ok(), "{validation:#?}");
    }

    #[test]
    fn a_midpoint_incidence_on_an_unresolved_reference_line_retains_its_native_form() {
        // Endpoint 8 has no stored coordinates, so the row has no solved carrier.
        let result = decode_type35_section(
            &[
                (1, 7, &[0xe4]),
                (2, 7, &[0xe4]),
                (1, 9, &X),
                (2, 9, &[0xe4]),
            ],
            REFERENCE_LINE_ROW,
            &[],
        );
        let reference_line = result
            .ir()
            .model
            .sketch_entities
            .iter()
            .find(|entity| {
                matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Native { native_kind }
                        if native_kind == "reference_line"
                )
            })
            .expect("unresolved reference line");
        assert_type35_retains_its_native_form(&result, reference_line.id());
    }

    #[test]
    fn a_midpoint_incidence_on_an_unresolved_axis_line_retains_its_native_form() {
        // Entity 42 is a type-5 row at section point 10 with the vertical
        // selector. An inactive unary vertical incidence and an inactive
        // symmetry incidence with 42 as its axis make it an axis line. Point
        // 10 has no stored coordinates, so the axis has no solved carrier.
        let result = decode_type35_section(
            &[(1, 9, &X), (2, 9, &[0xe4])],
            [5, 0, 0, 0, 10, 0xf6, 0xf6, 0, 0, 0xf6, 0xf6, 42],
            &[
                b"\x06\x02\x00\x00\xf8\x01\xf7\x6c\xfb\xe2\x2a\x00",
                b"\x07\x0e\x00\x00\xf8\x03\xf7\x6c\xfb\xe2\x2a\x00\xe2\x2b\x00\xe2\x2b\x00",
            ],
        );
        let axis_line = result
            .ir()
            .model
            .sketch_entities
            .iter()
            .find(|entity| {
                matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Native { native_kind } if native_kind == "line"
                )
            })
            .expect("unresolved axis line");
        assert_type35_retains_its_native_form(&result, axis_line.id());
    }

    /// An opaque `segtab` row with the unknown type 99 and external identifier
    /// `external_id`.
    fn opaque_row(external_id: u8) -> [u8; 12] {
        [
            99,
            0,
            0,
            0,
            0xf6,
            0xf6,
            0xf6,
            0,
            0xf6,
            0xf6,
            0xf6,
            external_id,
        ]
    }

    /// The decoded sketch entity with external identifier 42.
    fn entity_42(result: &cadmpeg_ir::codec::DecodeResult) -> &SketchEntityId {
        result
            .ir()
            .model
            .sketch_entities
            .iter()
            .find(|entity| entity.id().as_str().ends_with(":42"))
            .expect("entity 42")
            .id()
    }

    const POINT_9: [(u8, u8, &[u8]); 2] = [(1, 9, &X), (2, 9, &[0xe4])];

    #[test]
    fn a_midpoint_target_role_does_not_make_an_opaque_row_a_midpoint_target() {
        let result = decode_type35_section(&POINT_9, opaque_row(42), &[]);
        assert_type35_retains_its_native_form(&result, entity_42(&result));
    }

    #[test]
    fn a_midpoint_target_role_does_not_make_a_solver_only_entity_a_midpoint_target() {
        // Entity 42 has no `segtab` row; the row is entity 44.
        let result = decode_type35_section(&POINT_9, opaque_row(44), &[]);
        assert_type35_retains_its_native_form(&result, entity_42(&result));
    }

    #[test]
    fn a_midpoint_target_role_does_not_make_a_bounded_curve_row_a_midpoint_target() {
        // A type-12 row between the unsolved section points 7 and 8.
        let result = decode_type35_section(
            &POINT_9,
            [12, 0, 0, 0, 7, 8, 0xf6, 0, 0xf6, 0xf6, 0xf6, 42],
            &[],
        );
        assert_type35_retains_its_native_form(&result, entity_42(&result));
    }

    #[test]
    fn an_endpoint_role_does_not_make_an_opaque_row_a_midpoint_target() {
        // An inactive type-0 incidence selects the first endpoint of entity 42.
        let result = decode_type35_section(
            &POINT_9,
            opaque_row(42),
            &[b"\x06\x00\x00\x00\xf8\x02\xf7\x6c\xfb\xe2\x2a\x02\xe2\x2b\x00"],
        );
        assert_type35_retains_its_native_form(&result, entity_42(&result));
    }

    /// The native kind of entity 42, after the document validates.
    fn native_kind_42(result: &cadmpeg_ir::codec::DecodeResult) -> &str {
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
        assert!(validation.is_ok(), "{validation:#?}");
        let entity = result
            .ir()
            .model
            .sketch_entities
            .iter()
            .find(|entity| entity.id().as_str().ends_with(":42"))
            .expect("entity 42");
        let SketchGeometryDefinition::Native { native_kind } = entity.geometry.definition() else {
            panic!("a native entity 42: {entity:#?}");
        };
        native_kind.as_str()
    }

    #[test]
    fn a_type35_target_role_labels_an_opaque_row_line_or_arc() {
        let result = decode_type35_section(&POINT_9, opaque_row(42), &[]);
        assert_eq!(native_kind_42(&result), "line_or_arc");
    }

    #[test]
    fn a_type35_target_role_labels_a_solver_only_entity_line_or_arc() {
        // Entity 42 has no `segtab` row; the row is entity 44.
        let result = decode_type35_section(&POINT_9, opaque_row(44), &[]);
        assert_eq!(native_kind_42(&result), "line_or_arc");
    }

    #[test]
    fn a_unary_line_role_makes_an_opaque_row_a_midpoint_target() {
        // An inactive unary vertical incidence on entity 42.
        let result = decode_type35_section(
            &POINT_9,
            opaque_row(42),
            &[b"\x06\x02\x00\x00\xf8\x01\xf7\x6c\xfb\xe2\x2a\x00"],
        );
        let model = &result.ir().model;
        let [relation] = model
            .sketch_constraints
            .iter()
            .filter(|constraint| constraint.id.as_str().ends_with(":skamp:5"))
            .collect::<Vec<_>>()[..]
        else {
            panic!("one type-35 relation: {:#?}", model.sketch_constraints);
        };
        let SketchConstraintDefinitionInput::Midpoint {
            point: SketchLocus::Entity(point),
            entity,
        } = relation.definition.kind()
        else {
            panic!("a midpoint relation: {relation:#?}");
        };
        assert!(point.as_str().ends_with(":43"), "{point:?}");
        assert_eq!(entity, entity_42(&result));
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
        assert!(validation.is_ok(), "{validation:#?}");
    }
}
