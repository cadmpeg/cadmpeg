// SPDX-License-Identifier: Apache-2.0
//! Project planar and spatial sketch geometry.

use crate::design::dimensions::{planar_point, sketch_normal_sign};
use crate::design::face_resolve::{
    placement_origin_scale, sketch_curve_is_spatial, sketch_point_depth,
};
use crate::design::feature_project::closed_spatial_sketch_profiles;
use crate::design::geometry::closed_sketch_profiles;
use crate::ids::{
    native_stream, neutral_sketch_constraint_id, neutral_sketch_curve_id, neutral_sketch_id,
    neutral_sketch_point_id, neutral_sketch_text_id, neutral_spatial_sketch_curve_id,
    neutral_spatial_sketch_id, neutral_spatial_sketch_point_id, neutral_spatial_sketch_surface_id,
};
use crate::records::{
    DesignSketchPlacement, SketchConstraintKind, SketchCurveGeometry, SketchCurveIdentity,
    SketchPoint, SketchRelation, SketchSurface, SketchText,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::{HashMap, HashSet};

/// Project placed Design sketches and their exact planar point/curve records.
pub fn project_sketch_design(
    placements: &[DesignSketchPlacement],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    texts: &[SketchText],
    linear_tolerance: f64,
) -> (
    Vec<cadmpeg_ir::sketches::Sketch>,
    Vec<cadmpeg_ir::sketches::SketchEntity>,
) {
    use cadmpeg_ir::features::{Angle, Length};
    use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchGeometry};

    let placements_by_suffix = placements
        .iter()
        .filter_map(|placement| {
            Some((
                (
                    native_stream(&placement.id)?,
                    u32::try_from(placement.entity_suffix).ok()?,
                ),
                placement,
            ))
        })
        .collect::<HashMap<_, _>>();
    let spatial_owners = curves
        .iter()
        .filter(|curve| sketch_curve_is_spatial(curve))
        .filter_map(|curve| Some((native_stream(&curve.id)?.to_owned(), curve.owner_reference?)))
        .chain(points.iter().filter_map(|point| {
            (sketch_point_depth(point)?.abs() > 1.0e-9)
                .then(|| Some((native_stream(&point.id)?.to_owned(), point.owner_reference?)))?
        }))
        .collect::<HashSet<_>>();
    let mut sketches = placements
        .iter()
        .filter(|placement| {
            !u32::try_from(placement.entity_suffix).is_ok_and(|owner| {
                native_stream(&placement.id)
                    .is_some_and(|scope| spatial_owners.contains(&(scope.to_owned(), owner)))
            })
        })
        .map(|placement| Sketch {
            id: neutral_sketch_id(placement),
            name: Some(placement.entity_id.clone()),
            configuration: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
                origin: Point3::new(
                    placement.transform[0][3] * placement_origin_scale(placement),
                    placement.transform[1][3] * placement_origin_scale(placement),
                    placement.transform[2][3] * placement_origin_scale(placement),
                ),
                normal: Vector3::new(
                    placement.transform[0][2],
                    placement.transform[1][2],
                    placement.transform[2][2],
                ),
                u_axis: Vector3::new(
                    placement.transform[0][0],
                    placement.transform[1][0],
                    placement.transform[2][0],
                ),
            },
            profiles: Vec::new(),
            native_ref: Some(placement.id.clone()),
        })
        .collect::<Vec<_>>();
    sketches.sort_by_key(|sketch| sketch.id.clone());

    let mut entities = points
        .iter()
        .filter_map(|point| {
            let owner = point.owner_reference?;
            let scope = native_stream(&point.id)?;
            if spatial_owners.contains(&(scope.to_owned(), owner)) {
                return None;
            }
            let placement = placements_by_suffix.get(&(scope, owner))?;
            let sketch = neutral_sketch_id(placement);
            Some(SketchEntity {
                id: neutral_sketch_point_id(&sketch, point.persistent_id),
                sketch,
                construction: false,
                native_ref: Some(point.id.clone()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: point.coordinates,
                },
            })
        })
        .collect::<Vec<_>>();
    entities.extend(curves.iter().filter_map(|curve| {
        let owner = curve.owner_reference?;
        let scope = native_stream(&curve.id)?;
        if spatial_owners.contains(&(scope.to_owned(), owner)) {
            return None;
        }
        let placement = placements_by_suffix.get(&(scope, owner))?;
        let geometry = match curve.geometry.as_ref()? {
            SketchCurveGeometry::Line {
                start, end, normal, ..
            } if planar_point(start)
                && planar_point(end)
                && normal.z.is_finite()
                && normal.z != 0.0 =>
            {
                SketchGeometry::Line {
                    start: Point2::new(start.x, start.y),
                    end: Point2::new(end.x, end.y),
                }
            }
            SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } if planar_point(center) && reference_direction.z.abs() <= 1.0e-9 && *radius > 0.0 => {
                let orientation = sketch_normal_sign(normal)?;
                let phase = reference_direction.y.atan2(reference_direction.x);
                let start_angle = phase + orientation * start_angle;
                let end_angle = phase + orientation * end_angle;
                if (end_angle - start_angle).abs() >= std::f64::consts::TAU - 1.0e-9 {
                    SketchGeometry::Circle {
                        center: Point2::new(center.x, center.y),
                        radius: Length(*radius),
                    }
                } else {
                    SketchGeometry::Arc {
                        center: Point2::new(center.x, center.y),
                        radius: Length(*radius),
                        start_angle: Angle(start_angle),
                        end_angle: Angle(end_angle),
                    }
                }
            }
            SketchCurveGeometry::Nurbs {
                degree,
                knots,
                weights,
                control_points,
                ..
            } if *degree != 0
                && usize::try_from(*degree).is_ok_and(|degree| control_points.len() > degree)
                && control_points.iter().all(planar_point) =>
            {
                SketchGeometry::Nurbs {
                    degree: *degree,
                    knots: knots.clone(),
                    control_points: control_points
                        .iter()
                        .map(|point| Point2::new(point.x, point.y))
                        .collect(),
                    weights: (!weights.is_empty()).then(|| weights.clone()),
                    periodic: false,
                }
            }
            _ => return None,
        };
        let sketch = neutral_sketch_id(placement);
        Some(SketchEntity {
            id: neutral_sketch_curve_id(&sketch, curve.primary_id, curve.secondary_id),
            sketch,
            construction: false,
            native_ref: Some(curve.id.clone()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        })
    }));
    entities.extend(texts.iter().filter_map(|text| {
        let scope = native_stream(&text.id)?;
        let placement = placements_by_suffix.get(&(scope, text.owner_reference))?;
        let sketch = neutral_sketch_id(placement);
        Some(SketchEntity {
            id: neutral_sketch_text_id(&sketch, text.persistent_id),
            sketch,
            construction: false,
            native_ref: Some(text.id.clone()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Text {
                text: text.text.clone(),
                font_family: text.font_family.clone(),
                height: Length(text.height),
                width_factor: text.width_factor,
            },
        })
    }));
    entities.sort_by_key(|entity| entity.id.clone());
    for sketch in &mut sketches {
        sketch.profiles = closed_sketch_profiles(&sketch.id, &entities, linear_tolerance);
    }
    (sketches, entities)
}

/// Project non-planar Design sketch curves into model-space spatial sketches.
pub fn project_spatial_sketch_design(
    placements: &[DesignSketchPlacement],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    surfaces: &[SketchSurface],
    relations: &[SketchRelation],
    linear_tolerance: f64,
) -> (
    Vec<cadmpeg_ir::sketches::SpatialSketch>,
    Vec<cadmpeg_ir::sketches::SpatialSketchEntity>,
) {
    use cadmpeg_ir::features::{Angle, Length};
    use cadmpeg_ir::sketches::{SpatialSketch, SpatialSketchEntity, SpatialSketchGeometry};

    let placements_by_suffix = placements
        .iter()
        .filter_map(|placement| {
            Some((
                (
                    native_stream(&placement.id)?,
                    u32::try_from(placement.entity_suffix).ok()?,
                ),
                placement,
            ))
        })
        .collect::<HashMap<_, _>>();
    let spatial_owners = curves
        .iter()
        .filter(|curve| sketch_curve_is_spatial(curve))
        .filter_map(|curve| Some((native_stream(&curve.id)?.to_owned(), curve.owner_reference?)))
        .chain(points.iter().filter_map(|point| {
            (sketch_point_depth(point)?.abs() > 1.0e-9)
                .then(|| Some((native_stream(&point.id)?.to_owned(), point.owner_reference?)))?
        }))
        .chain(surfaces.iter().filter_map(|surface| {
            Some((
                native_stream(&surface.id)?.to_owned(),
                surface.owner_reference?,
            ))
        }))
        .collect::<HashSet<_>>();
    let curves_by_record = curves
        .iter()
        .filter_map(|curve| Some(((native_stream(&curve.id)?, curve.record_index), curve)))
        .collect::<HashMap<_, _>>();
    let mut spline_segments = HashMap::new();
    for relation in relations {
        if relation.unknown_constraint_bits != 0
            || relation.constraint_kinds != [SketchConstraintKind::SplineGroup]
            || relation.members.len() < 2
            || relation.members.iter().collect::<HashSet<_>>().len() != relation.members.len()
        {
            continue;
        }
        let Some(scope) = native_stream(&relation.id) else {
            continue;
        };
        let Some(curve) = relation
            .members
            .last()
            .and_then(|record| curves_by_record.get(&(scope, *record)))
        else {
            continue;
        };
        let Some(SketchCurveGeometry::Nurbs { control_points, .. }) = curve.geometry.as_ref()
        else {
            continue;
        };
        if curve.owner_reference != Some(relation.owner_reference)
            || control_points.len() != relation.members.len()
        {
            continue;
        }
        let segments = relation.members[..relation.members.len() - 1]
            .iter()
            .zip(control_points.windows(2))
            .map(|(record, points)| {
                let member = curves_by_record.get(&(scope, *record))?;
                if member.owner_reference != Some(relation.owner_reference) {
                    return None;
                }
                match member.geometry.as_ref() {
                    None => Some((*record, [points[0], points[1]])),
                    Some(SketchCurveGeometry::Line { start, end, .. })
                        if start == &points[0] && end == &points[1] =>
                    {
                        Some((*record, [points[0], points[1]]))
                    }
                    _ => None,
                }
            })
            .collect::<Option<Vec<_>>>();
        let Some(segments) = segments else { continue };
        for (record, points) in segments {
            spline_segments
                .entry((scope, record))
                .and_modify(|existing| {
                    if *existing != Some(points) {
                        *existing = None;
                    }
                })
                .or_insert(Some(points));
        }
    }
    let transform_point = |placement: &DesignSketchPlacement, point: &Point3| {
        let origin_scale = placement_origin_scale(placement);
        Point3::new(
            placement.transform[0][0] * point.x
                + placement.transform[0][1] * point.y
                + placement.transform[0][2] * point.z
                + placement.transform[0][3] * origin_scale,
            placement.transform[1][0] * point.x
                + placement.transform[1][1] * point.y
                + placement.transform[1][2] * point.z
                + placement.transform[1][3] * origin_scale,
            placement.transform[2][0] * point.x
                + placement.transform[2][1] * point.y
                + placement.transform[2][2] * point.z
                + placement.transform[2][3] * origin_scale,
        )
    };
    let transform_vector = |placement: &DesignSketchPlacement, vector: &Vector3| {
        Vector3::new(
            placement.transform[0][0] * vector.x
                + placement.transform[0][1] * vector.y
                + placement.transform[0][2] * vector.z,
            placement.transform[1][0] * vector.x
                + placement.transform[1][1] * vector.y
                + placement.transform[1][2] * vector.z,
            placement.transform[2][0] * vector.x
                + placement.transform[2][1] * vector.y
                + placement.transform[2][2] * vector.z,
        )
    };

    let mut entities = curves
        .iter()
        .filter_map(|curve| {
            let scope = native_stream(&curve.id)?;
            let owner = curve.owner_reference?;
            if !spatial_owners.contains(&(scope.to_owned(), owner)) {
                return None;
            }
            let placement = placements_by_suffix.get(&(scope, owner))?;
            let geometry = if let Some([start, end]) = spline_segments
                .get(&(scope, curve.record_index))
                .copied()
                .flatten()
            {
                SpatialSketchGeometry::Line {
                    start: transform_point(placement, &start),
                    end: transform_point(placement, &end),
                }
            } else {
                match curve.geometry.as_ref()? {
                    SketchCurveGeometry::Line { start, end, .. } => SpatialSketchGeometry::Line {
                        start: transform_point(placement, start),
                        end: transform_point(placement, end),
                    },
                    SketchCurveGeometry::Arc {
                        center,
                        normal,
                        reference_direction,
                        radius,
                        start_angle,
                        end_angle,
                    } if *radius > 0.0 => {
                        let center = transform_point(placement, center);
                        let normal = transform_vector(placement, normal);
                        let reference_direction = transform_vector(placement, reference_direction);
                        if (end_angle - start_angle).abs() >= std::f64::consts::TAU - 1.0e-9 {
                            SpatialSketchGeometry::Circle {
                                center,
                                normal,
                                reference_direction,
                                radius: Length(*radius),
                            }
                        } else {
                            SpatialSketchGeometry::Arc {
                                center,
                                normal,
                                reference_direction,
                                radius: Length(*radius),
                                start_angle: Angle(*start_angle),
                                end_angle: Angle(*end_angle),
                            }
                        }
                    }
                    SketchCurveGeometry::Nurbs {
                        degree,
                        knots,
                        weights,
                        control_points,
                        ..
                    } if *degree != 0
                        && usize::try_from(*degree)
                            .is_ok_and(|degree| control_points.len() > degree) =>
                    {
                        SpatialSketchGeometry::Nurbs {
                            degree: *degree,
                            knots: knots.clone(),
                            control_points: control_points
                                .iter()
                                .map(|point| transform_point(placement, point))
                                .collect(),
                            weights: (!weights.is_empty()).then(|| weights.clone()),
                            periodic: false,
                        }
                    }
                    _ => return None,
                }
            };
            let sketch = neutral_spatial_sketch_id(placement);
            Some(SpatialSketchEntity {
                id: neutral_spatial_sketch_curve_id(&sketch, curve.primary_id, curve.secondary_id),
                sketch,
                construction: false,
                native_ref: Some(curve.id.clone()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry,
            })
        })
        .collect::<Vec<_>>();
    entities.extend(points.iter().filter_map(|point| {
        let scope = native_stream(&point.id)?;
        let owner = point.owner_reference?;
        if !spatial_owners.contains(&(scope.to_owned(), owner)) {
            return None;
        }
        let placement = placements_by_suffix.get(&(scope, owner))?;
        let sketch = neutral_spatial_sketch_id(placement);
        let depth = sketch_point_depth(point)?;
        Some(SpatialSketchEntity {
            id: neutral_spatial_sketch_point_id(&sketch, point.persistent_id),
            sketch,
            construction: false,
            native_ref: Some(point.id.clone()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SpatialSketchGeometry::Point {
                position: transform_point(
                    placement,
                    &Point3::new(point.coordinates.u, point.coordinates.v, depth),
                ),
            },
        })
    }));
    entities.extend(surfaces.iter().filter_map(|surface| {
        let scope = native_stream(&surface.id)?;
        let owner = surface.owner_reference?;
        let placement = placements_by_suffix.get(&(scope, owner))?;
        let sketch = neutral_spatial_sketch_id(placement);
        Some(SpatialSketchEntity {
            id: neutral_spatial_sketch_surface_id(&sketch, surface.persistent_id),
            sketch,
            construction: false,
            native_ref: Some(surface.id.clone()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SpatialSketchGeometry::NurbsSurface {
                u_degree: surface.u_degree,
                v_degree: surface.v_degree,
                u_knots: surface.u_knots.clone(),
                v_knots: surface.v_knots.clone(),
                control_points: surface
                    .control_points
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|point| transform_point(placement, point))
                            .collect()
                    })
                    .collect(),
            },
        })
    }));
    entities.sort_by_key(|entity| entity.id.clone());
    let spatial_ids = entities
        .iter()
        .map(|entity| entity.sketch.clone())
        .collect::<HashSet<_>>();
    let mut sketches = placements
        .iter()
        .filter(|placement| spatial_ids.contains(&neutral_spatial_sketch_id(placement)))
        .map(|placement| {
            let id = neutral_spatial_sketch_id(placement);
            SpatialSketch {
                profiles: closed_spatial_sketch_profiles(&id, &entities, linear_tolerance),
                id,
                name: Some(placement.entity_id.clone()),
                configuration: None,
                native_ref: Some(placement.id.clone()),
            }
        })
        .collect::<Vec<_>>();
    sketches.sort_by_key(|sketch| sketch.id.clone());
    (sketches, entities)
}

/// Project exact aggregate relations owned by model-space spatial sketches.
pub fn project_spatial_sketch_constraints(
    placements: &[DesignSketchPlacement],
    relations: &[SketchRelation],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    surfaces: &[SketchSurface],
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Vec<cadmpeg_ir::sketches::SpatialSketchConstraint> {
    use cadmpeg_ir::sketches::{
        SpatialSketchConstraint, SpatialSketchConstraintDefinition as Definition,
        SpatialSketchGeometry,
    };

    let spatial_sketches = entities
        .iter()
        .map(|entity| entity.sketch.clone())
        .collect::<HashSet<_>>();
    let sketches = placements
        .iter()
        .filter_map(|placement| {
            let id = neutral_spatial_sketch_id(placement);
            spatial_sketches.contains(&id).then_some((
                (
                    native_stream(&placement.id)?,
                    u32::try_from(placement.entity_suffix).ok()?,
                ),
                (id, placement),
            ))
        })
        .collect::<HashMap<_, _>>();
    let record_indices = curves
        .iter()
        .map(|curve| (curve.id.as_str(), curve.record_index))
        .chain(
            points
                .iter()
                .map(|point| (point.id.as_str(), point.record_index)),
        )
        .chain(
            surfaces
                .iter()
                .map(|surface| (surface.id.as_str(), surface.record_index)),
        )
        .collect::<HashMap<_, _>>();
    let projected = entities
        .iter()
        .filter_map(|entity| {
            let native_ref = entity.native_ref.as_deref()?;
            Some((
                (native_stream(native_ref)?, *record_indices.get(native_ref)?),
                entity,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut constraints = relations
        .iter()
        .filter_map(|relation| {
            if relation.unknown_constraint_bits != 0 || relation.constraint_kinds.len() != 1 {
                return None;
            }
            let scope = native_stream(&relation.id)?;
            let (sketch, placement) = sketches.get(&(scope, relation.owner_reference))?;
            let member_entities = relation
                .members
                .iter()
                .map(|record_index| projected.get(&(scope, *record_index)).copied())
                .collect::<Option<Vec<_>>>()?;
            let members = member_entities
                .iter()
                .map(|entity| entity.id.clone())
                .collect::<Vec<_>>();
            let distinct = members.iter().collect::<HashSet<_>>();
            if distinct.len() != members.len() {
                return None;
            }
            let definition = match relation.constraint_kinds[0] {
                SketchConstraintKind::Coincident => {
                    let [first, second] = member_entities.as_slice() else {
                        return None;
                    };
                    let point_on_surface = match (&first.geometry, &second.geometry) {
                        (
                            SpatialSketchGeometry::Point { .. },
                            SpatialSketchGeometry::NurbsSurface { .. },
                        ) => Some((first, second)),
                        (
                            SpatialSketchGeometry::NurbsSurface { .. },
                            SpatialSketchGeometry::Point { .. },
                        ) => Some((second, first)),
                        _ => None,
                    };
                    if let Some((point, surface)) = point_on_surface {
                        Definition::PointOnSurface {
                            point: point.id.clone(),
                            surface: surface.id.clone(),
                        }
                    } else {
                        let (
                            SpatialSketchGeometry::Point {
                                position: first_position,
                            },
                            SpatialSketchGeometry::Point {
                                position: second_position,
                            },
                        ) = (&first.geometry, &second.geometry)
                        else {
                            return None;
                        };
                        let scale = 1.0
                            + first_position
                                .x
                                .abs()
                                .max(first_position.y.abs())
                                .max(first_position.z.abs())
                                .max(second_position.x.abs())
                                .max(second_position.y.abs())
                                .max(second_position.z.abs());
                        if (first_position.x - second_position.x).abs() > scale * 1.0e-9
                            || (first_position.y - second_position.y).abs() > scale * 1.0e-9
                            || (first_position.z - second_position.z).abs() > scale * 1.0e-9
                        {
                            return None;
                        }
                        Definition::Coincident {
                            first: first.id.clone(),
                            second: second.id.clone(),
                        }
                    }
                }
                SketchConstraintKind::SplineGroup if members.len() >= 2 => {
                    Definition::SplineGroup { entities: members }
                }
                SketchConstraintKind::Tangent => {
                    let [first, second] = member_entities.as_slice() else {
                        return None;
                    };
                    let curve = |geometry: &SpatialSketchGeometry| {
                        matches!(
                            geometry,
                            SpatialSketchGeometry::Line { .. }
                                | SpatialSketchGeometry::Circle { .. }
                                | SpatialSketchGeometry::Arc { .. }
                                | SpatialSketchGeometry::Nurbs { .. }
                        )
                    };
                    if !curve(&first.geometry) || !curve(&second.geometry) {
                        return None;
                    }
                    Definition::Tangent {
                        first: first.id.clone(),
                        second: second.id.clone(),
                    }
                }
                SketchConstraintKind::Midpoint => {
                    let [first, second] = member_entities.as_slice() else {
                        return None;
                    };
                    let (point, line, position, start, end) =
                        match (&first.geometry, &second.geometry) {
                            (
                                SpatialSketchGeometry::Point { position },
                                SpatialSketchGeometry::Line { start, end },
                            ) => (first, second, position, start, end),
                            (
                                SpatialSketchGeometry::Line { start, end },
                                SpatialSketchGeometry::Point { position },
                            ) => (second, first, position, start, end),
                            _ => return None,
                        };
                    let midpoint = Point3::new(
                        (start.x + end.x) * 0.5,
                        (start.y + end.y) * 0.5,
                        (start.z + end.z) * 0.5,
                    );
                    let scale = 1.0 + midpoint.x.abs().max(midpoint.y.abs()).max(midpoint.z.abs());
                    if (position.x - midpoint.x).abs() > scale * 1.0e-9
                        || (position.y - midpoint.y).abs() > scale * 1.0e-9
                        || (position.z - midpoint.z).abs() > scale * 1.0e-9
                    {
                        return None;
                    }
                    Definition::Midpoint {
                        point: point.id.clone(),
                        entity: line.id.clone(),
                    }
                }
                SketchConstraintKind::Horizontal | SketchConstraintKind::Vertical => {
                    let [entity] = member_entities.as_slice() else {
                        return None;
                    };
                    let SpatialSketchGeometry::Line { start, end } = entity.geometry else {
                        return None;
                    };
                    let direction = match relation.constraint_kinds[0] {
                        SketchConstraintKind::Horizontal => Vector3::new(
                            placement.transform[0][0],
                            placement.transform[1][0],
                            placement.transform[2][0],
                        ),
                        SketchConstraintKind::Vertical => Vector3::new(
                            placement.transform[0][1],
                            placement.transform[1][1],
                            placement.transform[2][1],
                        ),
                        _ => unreachable!(),
                    };
                    let line = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
                    let cross = Vector3::new(
                        line.y * direction.z - line.z * direction.y,
                        line.z * direction.x - line.x * direction.z,
                        line.x * direction.y - line.y * direction.x,
                    );
                    if line.norm() <= 1.0e-12 || cross.norm() > 1.0e-9 * line.norm() {
                        return None;
                    }
                    Definition::ParallelToDirection {
                        entity: entity.id.clone(),
                        direction,
                    }
                }
                _ => return None,
            };
            Some(SpatialSketchConstraint {
                id: neutral_sketch_constraint_id(&relation.id, relation.record_index),
                sketch: sketch.clone(),
                definition,
                native_ref: Some(relation.id.clone()),
            })
        })
        .collect::<Vec<_>>();
    constraints.sort_by_key(|constraint| constraint.id.clone());
    constraints
}
