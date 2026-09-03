// SPDX-License-Identifier: Apache-2.0
//! SKAMP solver constraint emission and locus compatibility.

use super::super::feature_history::feature_solver_table_complete;
use super::super::sketch_ids::{sketch_constraint_id, sketch_entity_id, sketch_native_ref};
use super::{
    section_entity_external_ids, section_skamp_active, section_skamp_center_entity,
    section_skamp_circular_entity, section_skamp_curve_entity, section_skamp_incidence_locus,
    section_skamp_is_arc, section_skamp_is_line, section_skamp_is_point, section_skamp_line_pair,
    section_skamp_locus, section_skamp_midpoint, section_skamp_oriented_line,
    section_skamp_point_locus, section_skamp_same_coordinate, section_skamp_same_coordinate_axis,
    section_skamp_tangent_loci, unique_bounded_curve_segment,
};
use cadmpeg_ir::features::Angle;
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinition, SketchCoordinateAxis, SketchEntityId,
    SketchGeometry, SketchId, SketchLocus, SketchNativeOperand,
};
use std::collections::BTreeMap;

pub(in super::super) fn section_skamp_constraints_for_geometry(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Vec<(SketchConstraint, usize)> {
    let Some(relations) = &definition.relations else {
        return Vec::new();
    };
    let complete_skamps =
        feature_solver_table_complete(relations.skamp_header.as_ref(), relations.skamps.len());
    let skamp_id_counts =
        relations
            .skamps
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
                .skamps
                .iter()
                .flat_map(|skamp| &skamp.items)
                .map(|item| item.entity_id)
                .filter(|entity_id| geometry.contains_key(&sketch_entity_id(sketch, *entity_id)))
                .collect()
        },
    );
    relations
        .skamps
        .iter()
        .filter_map(|skamp| {
            let unique_skamp_id = complete_skamps && skamp_id_counts.get(&skamp.id) == Some(&1);
            let joined_equation_id = if unique_skamp_id
                && feature_solver_table_complete(
                    relations.triples_header.as_ref(),
                    relations.triples.len(),
                ) {
                let mut equation_ids = relations
                    .triples
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
                let entities = skamp
                    .items
                    .iter()
                    .filter(|item| available_entities.contains(&item.entity_id))
                    .map(|item| sketch_entity_id(sketch, item.entity_id))
                    .collect::<Vec<_>>();
                let mut operands = skamp
                    .items
                    .iter()
                    .map(|item| SketchNativeOperand {
                        native_kind: "skamp_ptr".to_string(),
                        native_field: Some("items.entity_id".to_string()),
                        native_role: Some(item.sense),
                        object_index: item.entity_id,
                        native_ref: Some(native_ref.clone()),
                    })
                    .collect::<Vec<_>>();
                if let Some(equation_id) = joined_equation_id {
                    operands.push(SketchNativeOperand {
                        native_kind: "triples_ptr".to_string(),
                        native_field: Some("equation_id".to_string()),
                        native_role: None,
                        object_index: equation_id,
                        native_ref: Some(native_ref),
                    });
                }
                Some(SketchConstraintDefinition::Native {
                    native_kind: format!("creo:skamp:{}", skamp.kind),
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
            let item_geometry = |item: &crate::feature::FeatureSkampItem| {
                let entity = sketch_entity_id(sketch, item.entity_id);
                geometry?.get(&entity)
            };
            let inactive_curve_entity = |item: &crate::feature::FeatureSkampItem| {
                (!active && item.sense == 0 && item_geometry(item).is_some_and(|geometry| {
                    matches!(
                        geometry,
                        SketchGeometry::Line { .. }
                            | SketchGeometry::ReferenceLine { .. }
                            | SketchGeometry::Circle { .. }
                            | SketchGeometry::Arc { .. }
                            | SketchGeometry::Nurbs { .. }
                    ) || matches!(
                        geometry,
                        SketchGeometry::Native { native_kind }
                            if matches!(native_kind.as_str(), "line" | "arc" | "circle" | "spline")
                    )
                }))
                .then(|| sketch_entity_id(sketch, item.entity_id))
            };
            let inactive_incidence_locus = |item: &crate::feature::FeatureSkampItem| {
                section_skamp_incidence_locus(definition, sketch, item, geometry).or_else(|| {
                    (!active
                        && item.sense == 4
                        && item_geometry(item).is_some_and(|geometry| {
                            matches!(
                                geometry,
                                SketchGeometry::Circle { .. } | SketchGeometry::Arc { .. }
                            ) || matches!(
                                geometry,
                                SketchGeometry::Native { native_kind }
                                    if matches!(native_kind.as_str(), "arc" | "circle")
                            )
                        }))
                    .then(|| SketchLocus::Center(sketch_entity_id(sketch, item.entity_id)))
                })
            };
            let point_entity = |item: &crate::feature::FeatureSkampItem| {
                (item.sense == 0).then_some(())?;
                if section_skamp_is_point(definition, item) {
                    return Some(sketch_entity_id(sketch, item.entity_id));
                }
                (!active
                    && item_geometry(item).is_some_and(|geometry| {
                        matches!(geometry, SketchGeometry::Point { .. })
                            || matches!(
                                geometry,
                                SketchGeometry::Native { native_kind } if native_kind == "point"
                            )
                    }))
                .then(|| sketch_entity_id(sketch, item.entity_id))
            };
            let inactive_point_locus = |item: &crate::feature::FeatureSkampItem| {
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
                        SketchConstraintDefinition::Concentric {
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
                        SketchConstraintDefinition::CoincidentLoci {
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
                            SketchConstraintDefinition::CoincidentLoci {
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
                                SketchConstraintDefinition::PointOnObject {
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
                                    SketchConstraintDefinition::CoincidentLoci {
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
                                SketchConstraintDefinition::Horizontal { entity }
                            }
                            Some(entity) => SketchConstraintDefinition::Vertical { entity },
                            None => native_constraint()?,
                        }
                    }
                    (4, [first, second]) => {
                        if let Some([first, second]) = section_skamp_tangent_loci(
                            definition, sketch, first, second, active, geometry,
                        ) {
                            SketchConstraintDefinition::TangentLoci { first, second }
                        } else if section_skamp_curve_entity(definition, sketch, first).is_some()
                            && section_skamp_curve_entity(definition, sketch, second).is_some()
                        {
                            SketchConstraintDefinition::Tangent {
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
                                SketchConstraintDefinition::Perpendicular { first, second }
                            }
                            _ => native_constraint()?,
                        }
                    }
                    (6, [first, second])
                        if section_skamp_circular_entity(definition, sketch, first).is_some()
                            && section_skamp_circular_entity(definition, sketch, second)
                                .is_some() =>
                    {
                        SketchConstraintDefinition::Equal {
                            first: section_skamp_circular_entity(definition, sketch, first)?,
                            second: section_skamp_circular_entity(definition, sketch, second)?,
                        }
                    }
                    (7, [first, second])
                        if section_skamp_line_pair(definition, sketch, first, second).is_some() =>
                    {
                        let [first, second] =
                            section_skamp_line_pair(definition, sketch, first, second)?;
                        SketchConstraintDefinition::Parallel { first, second }
                    }
                    (8, [first, second])
                        if section_skamp_line_pair(definition, sketch, first, second).is_some() =>
                    {
                        let [first, second] =
                            section_skamp_line_pair(definition, sketch, first, second)?;
                        SketchConstraintDefinition::Equal { first, second }
                    }
                    (9, [first, second])
                        if section_skamp_line_pair(definition, sketch, first, second).is_some() =>
                    {
                        let [first, second] =
                            section_skamp_line_pair(definition, sketch, first, second)?;
                        SketchConstraintDefinition::Collinear { first, second }
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
                        SketchConstraintDefinition::PointOnObject {
                            point: section_skamp_locus(definition, sketch, point)?,
                            entity: sketch_entity_id(sketch, line.entity_id),
                        }
                    }
                    (kind @ (10 | 11), [item])
                        if item.sense == 0 && section_skamp_is_arc(definition, item) =>
                    {
                        SketchConstraintDefinition::ArcAngle {
                            entity: sketch_entity_id(sketch, item.entity_id),
                            angle: Angle(if kind == 10 {
                                std::f64::consts::FRAC_PI_2
                            } else {
                                std::f64::consts::PI
                            }),
                        }
                    }
                    (kind @ (12 | 13), [item])
                        if item.sense == 0 && section_skamp_is_arc(definition, item) =>
                    {
                        let entity = sketch_entity_id(sketch, item.entity_id);
                        let first = SketchLocus::Start(entity.clone());
                        let second = SketchLocus::End(entity);
                        let axis = if kind == 12 {
                            SketchCoordinateAxis::V
                        } else {
                            SketchCoordinateAxis::U
                        };
                        SketchConstraintDefinition::SameCoordinate {
                            first,
                            second,
                            axis,
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
                        let source = sketch_entity_id(sketch, source.entity_id);
                        let result = sketch_entity_id(sketch, result.entity_id);
                        let geometry_agrees = geometry.is_none_or(|geometry| {
                            geometry
                                .get(&source)
                                .zip(geometry.get(&result))
                                .is_none_or(|(source, result)| source == result)
                        });
                        if geometry_agrees {
                            SketchConstraintDefinition::ProjectedCopy { source, result }
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
                        SketchConstraintDefinition::Fixed {
                            entity: sketch_entity_id(sketch, item.entity_id),
                        }
                    }
                    (14, [axis, first, second])
                        if axis.sense == 0
                            && section_skamp_is_line(definition, axis)
                            && section_skamp_point_locus(definition, sketch, first).is_some()
                            && section_skamp_point_locus(definition, sketch, second).is_some() =>
                    {
                        SketchConstraintDefinition::Symmetric {
                            first: section_skamp_point_locus(definition, sketch, first)?,
                            second: section_skamp_point_locus(definition, sketch, second)?,
                            axis: sketch_entity_id(sketch, axis.entity_id),
                        }
                    }
                    (14, [center, first, second])
                        if point_entity(center).is_some()
                            && inactive_point_locus(first).is_some()
                            && inactive_point_locus(second).is_some() =>
                    {
                        SketchConstraintDefinition::PointSymmetric {
                            first: inactive_point_locus(first)?,
                            second: inactive_point_locus(second)?,
                            center: SketchLocus::Entity(point_entity(center)?),
                        }
                    }
                    (15 | 17 | 30 | 31, [_, _]) => {
                        if let Some((first, second, axis)) =
                            section_skamp_same_coordinate(definition, sketch, skamp, active)
                        {
                            SketchConstraintDefinition::SameCoordinate {
                                first,
                                second,
                                axis,
                            }
                        } else if !active {
                            let [first, second] = skamp.items.as_slice() else {
                                unreachable!();
                            };
                            match (
                                inactive_point_locus(first),
                                inactive_point_locus(second),
                                section_skamp_same_coordinate_axis(skamp),
                            ) {
                                (Some(first), Some(second), Some(axis)) => {
                                    SketchConstraintDefinition::SameCoordinate {
                                        first,
                                        second,
                                        axis: [SketchCoordinateAxis::U, SketchCoordinateAxis::V]
                                            [axis],
                                    }
                                }
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
                            SketchConstraintDefinition::Midpoint { point, entity }
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
                        sketch_constraint_id(sketch, format_args!("skamp:{}", skamp.id))
                    } else {
                        sketch_constraint_id(sketch, format_args!("skamp:offset:{}", skamp.offset))
                    },
                    sketch: sketch.clone(),
                    definition: constraint_definition,
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
    definition: &SketchConstraintDefinition,
    geometry: &BTreeMap<SketchEntityId, SketchGeometry>,
) -> bool {
    sketch_constraint_loci_compatible_with_policy(definition, geometry, false)
}

pub(in super::super) fn sketch_constraint_loci_compatible_with_policy(
    definition: &SketchConstraintDefinition,
    geometry: &BTreeMap<SketchEntityId, SketchGeometry>,
    allow_unknown_native_endpoints: bool,
) -> bool {
    let native_line_center_allowed = matches!(
        definition,
        SketchConstraintDefinition::Midpoint {
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
                    geometry,
                    SketchGeometry::Point { .. } | SketchGeometry::Circle { .. }
                ) && !matches!(
                        geometry,
                        SketchGeometry::Native { native_kind }
                            if !(matches!(
                                native_kind.as_str(),
                                "bounded_curve" | "line" | "arc" | "spline"
                            ) || allow_unknown_native_endpoints
                                && native_kind == "solver_only_section_entity")
                )
            }
            SketchLocus::Center(_) => {
                matches!(
                    geometry,
                    SketchGeometry::Circle { .. }
                        | SketchGeometry::Arc { .. }
                        | SketchGeometry::Ellipse { .. }
                ) || matches!(
                    geometry,
                    SketchGeometry::Native { native_kind }
                        if matches!(native_kind.as_str(), "circle" | "arc")
                            // A centered type-47 row retains its center on a native line.
                            || native_line_center_allowed && native_kind == "line"
                )
            }
        })
    };
    match definition {
        SketchConstraintDefinition::CoincidentLoci { loci }
        | SketchConstraintDefinition::Group { elements: loci }
        | SketchConstraintDefinition::Text { elements: loci, .. } => {
            loci.iter().all(locus_compatible)
        }
        SketchConstraintDefinition::SameCoordinate { first, second, .. }
        | SketchConstraintDefinition::TangentLoci { first, second }
        | SketchConstraintDefinition::DistanceLoci { first, second, .. }
        | SketchConstraintDefinition::DistanceLociValue { first, second, .. }
        | SketchConstraintDefinition::MidpointCoordinate { first, second, .. }
        | SketchConstraintDefinition::HorizontalDistance { first, second, .. }
        | SketchConstraintDefinition::VerticalDistance { first, second, .. } => {
            locus_compatible(first) && locus_compatible(second)
        }
        SketchConstraintDefinition::Midpoint { point, entity }
        | SketchConstraintDefinition::PointOnObject { point, entity } => {
            locus_compatible(point) && geometry.contains_key(entity)
        }
        SketchConstraintDefinition::PointCoordinateValues { point, .. } => locus_compatible(point),
        SketchConstraintDefinition::Symmetric {
            first,
            second,
            axis,
        } => locus_compatible(first) && locus_compatible(second) && geometry.contains_key(axis),
        SketchConstraintDefinition::PointSymmetric {
            first,
            second,
            center,
        } => locus_compatible(first) && locus_compatible(second) && locus_compatible(center),
        SketchConstraintDefinition::SnellsLaw {
            incident,
            refracted,
            interface,
            ..
        } => {
            locus_compatible(incident)
                && locus_compatible(refracted)
                && geometry.contains_key(interface)
        }
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
            geometry.contains_key(first) && geometry.contains_key(second)
        }
        SketchConstraintDefinition::Horizontal { entity }
        | SketchConstraintDefinition::Vertical { entity }
        | SketchConstraintDefinition::Fixed { entity }
        | SketchConstraintDefinition::Radius { entity, .. }
        | SketchConstraintDefinition::Diameter { entity, .. }
        | SketchConstraintDefinition::ArcAngle { entity, .. }
        | SketchConstraintDefinition::EllipseAngle { entity, .. } => geometry.contains_key(entity),
        SketchConstraintDefinition::AtIntersection {
            point,
            first,
            second,
        } => {
            locus_compatible(point) && geometry.contains_key(first) && geometry.contains_key(second)
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::sketch_constraint_loci_compatible_with_policy;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition, SketchEntityId, SketchGeometry, SketchLocus,
    };
    use std::collections::BTreeMap;

    #[test]
    fn typed_entity_relations_require_every_entity_in_the_emitted_geometry() {
        let first = SketchEntityId("synthetic:test:relation#first".into());
        let second = SketchEntityId("synthetic:test:relation#second".into());
        let axis = SketchEntityId("synthetic:test:relation#axis".into());
        let geometry = BTreeMap::from([
            (
                first.clone(),
                SketchGeometry::Point {
                    position: Point2::new(0.0, 0.0),
                },
            ),
            (
                second.clone(),
                SketchGeometry::Point {
                    position: Point2::new(1.0, 0.0),
                },
            ),
        ]);
        let symmetry = SketchConstraintDefinition::Symmetric {
            first: SketchLocus::Entity(first.clone()),
            second: SketchLocus::Entity(second.clone()),
            axis: axis.clone(),
        };
        assert!(!sketch_constraint_loci_compatible_with_policy(
            &symmetry, &geometry, false,
        ));

        let projected = SketchConstraintDefinition::ProjectedCopy {
            source: first.clone(),
            result: axis.clone(),
        };
        assert!(!sketch_constraint_loci_compatible_with_policy(
            &projected, &geometry, false,
        ));

        let mut complete = geometry;
        complete.insert(
            axis.clone(),
            SketchGeometry::ReferenceLine {
                origin: Point2::new(0.0, 0.0),
                direction: Point2::new(0.0, 1.0),
            },
        );
        assert!(sketch_constraint_loci_compatible_with_policy(
            &symmetry, &complete, false,
        ));
        assert!(sketch_constraint_loci_compatible_with_policy(
            &projected, &complete, false,
        ));
    }
}
