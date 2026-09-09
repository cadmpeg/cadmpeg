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
    neutral_sketch_point_id, neutral_sketch_record_id, neutral_sketch_text_id,
    neutral_spatial_sketch_curve_id, neutral_spatial_sketch_id, neutral_spatial_sketch_point_id,
    neutral_spatial_sketch_record_id, neutral_spatial_sketch_surface_id,
};
use crate::records::{
    DesignSketchPlacement, SketchConstraintKind, SketchCurveGeometry, SketchCurveIdentity,
    SketchPoint, SketchRelation, SketchSurface, SketchText,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::{HashMap, HashSet};

const EPS_SPATIAL_OWNER_DEPTH: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SKETCH_DESIGN_E9: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_DESIGN_E9: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E12: f64 = 1.0e-12;

fn spatial_geometry_owners(
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
) -> HashSet<(String, u32)> {
    curves
        .iter()
        .filter(|curve| sketch_curve_is_spatial(curve))
        .filter_map(|curve| Some((native_stream(&curve.id)?.to_owned(), curve.owner_reference?)))
        .chain(points.iter().filter_map(|point| {
            (sketch_point_depth(point)?.abs() > EPS_SPATIAL_OWNER_DEPTH)
                .then(|| Some((native_stream(&point.id)?.to_owned(), point.owner_reference?)))?
        }))
        .collect()
}

fn sketch_text_horizontal_alignment(
    code: Option<u32>,
) -> Option<cadmpeg_ir::sketches::SketchTextHorizontalAlignment> {
    code.map(|code| match code {
        1 => cadmpeg_ir::sketches::SketchTextHorizontalAlignment::Left,
        2 => cadmpeg_ir::sketches::SketchTextHorizontalAlignment::Right,
        3 => cadmpeg_ir::sketches::SketchTextHorizontalAlignment::Center,
        code => cadmpeg_ir::sketches::SketchTextHorizontalAlignment::Native(code),
    })
}

fn sketch_text_vertical_alignment(
    code: Option<u32>,
) -> Option<cadmpeg_ir::sketches::SketchTextVerticalAlignment> {
    code.map(|code| match code {
        1 => cadmpeg_ir::sketches::SketchTextVerticalAlignment::Top,
        2 => cadmpeg_ir::sketches::SketchTextVerticalAlignment::Bottom,
        3 => cadmpeg_ir::sketches::SketchTextVerticalAlignment::Middle,
        code => cadmpeg_ir::sketches::SketchTextVerticalAlignment::Native(code),
    })
}

fn text_frame_curve_records(
    relations: &[SketchRelation],
    curves: &[SketchCurveIdentity],
    texts: &[SketchText],
) -> HashSet<(String, u32)> {
    let curve_owners = curves
        .iter()
        .filter_map(|curve| {
            Some((
                (native_stream(&curve.id)?.to_owned(), curve.record_index),
                curve.owner_reference?,
            ))
        })
        .collect::<HashMap<_, _>>();
    let text_owners = texts
        .iter()
        .filter_map(|text| {
            Some((
                (native_stream(&text.id)?.to_owned(), text.record_index),
                text.owner_reference,
            ))
        })
        .collect::<HashMap<_, _>>();
    relations
        .iter()
        .filter_map(|relation| {
            let pattern = relation.definition.pattern();
            let Some(crate::records::SketchPatternDefinition::TextFrame { text_reference }) =
                pattern
            else {
                return None;
            };
            let scope = native_stream(&relation.id)?.to_owned();
            if relation.unknown_constraint_bits() != 0
                || relation.constraint_kinds().len() != 1
                || relation
                    .members()
                    .first()
                    .map(|member| member.reference.record_index())
                    != Some(*text_reference)
                || !relation
                    .auxiliary_references()
                    .values()
                    .copied()
                    .eq([*text_reference])
                || relation.members().len() < 2
                || relation.return_member_indices() != relation.member_indices()[1..]
                || text_owners.get(&(scope.clone(), *text_reference))
                    != Some(&relation.owner_reference)
            {
                return None;
            }
            if !relation.return_members().iter().all(|member| {
                curve_owners.get(&(scope.clone(), member.reference.record_index()))
                    == Some(&relation.owner_reference)
            }) {
                return None;
            }
            Some(
                relation
                    .return_member_indices()
                    .into_iter()
                    .map(move |record_index| (scope.clone(), record_index)),
            )
        })
        .flatten()
        .collect()
}

/// Project placed Design sketches and their exact planar point/curve records.
pub fn project_sketch_design(
    placements: &[DesignSketchPlacement],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    relations: &[SketchRelation],
    texts: &[SketchText],
    linear_tolerance: f64,
) -> (
    Vec<cadmpeg_ir::sketches::Sketch>,
    Vec<cadmpeg_ir::sketches::SketchEntity>,
) {
    use cadmpeg_ir::scalar::{Angle, Length};
    use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchGeometry, SketchGeometryDefinition};

    let text_frame_curves = text_frame_curve_records(relations, curves, texts);
    let placements_by_suffix = placements
        .iter()
        .filter_map(|placement| {
            Some((
                (
                    native_stream(&placement.id)?,
                    u32::try_from(placement.entity_id.suffix()).ok()?,
                ),
                placement,
            ))
        })
        .collect::<HashMap<_, _>>();
    let spatial_owners = spatial_geometry_owners(points, curves);
    let mut sketches = placements
        .iter()
        .filter(|placement| {
            !u32::try_from(placement.entity_id.suffix()).is_ok_and(|owner| {
                native_stream(&placement.id)
                    .is_some_and(|scope| spatial_owners.contains(&(scope.to_owned(), owner)))
            })
        })
        .filter_map(|placement| {
            Some(Sketch {
                id: neutral_sketch_id(placement)?,
                name: Some(placement.entity_id.as_str().to_owned()),
                configuration: None,
                visible: placement
                    .visibility
                    .as_ref()
                    .map(|visibility| visibility.visible),
                placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                    Point3::new(
                        placement.transform()[0][3] * placement_origin_scale(placement),
                        placement.transform()[1][3] * placement_origin_scale(placement),
                        placement.transform()[2][3] * placement_origin_scale(placement),
                    ),
                    Vector3::new(
                        placement.transform()[0][2],
                        placement.transform()[1][2],
                        placement.transform()[2][2],
                    ),
                    Vector3::new(
                        placement.transform()[0][0],
                        placement.transform()[1][0],
                        placement.transform()[2][0],
                    ),
                )
                .ok()?,
                profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
                native_ref: Some(placement.id.clone()),
            })
        })
        .collect::<Vec<_>>();
    sketches.sort_by(|a, b| a.id.cmp(&b.id));

    let mut entities = points
        .iter()
        .filter_map(|point| {
            let owner = point.owner_reference?;
            let scope = native_stream(&point.id)?;
            if spatial_owners.contains(&(scope.to_owned(), owner)) {
                return None;
            }
            let placement = placements_by_suffix.get(&(scope, owner))?;
            let sketch = neutral_sketch_id(placement)?;
            Some(
                SketchEntity::new(
                    point.persistent_id().map_or_else(
                        || neutral_sketch_record_id(&sketch, point.record_index),
                        |persistent_id| neutral_sketch_point_id(&sketch, persistent_id),
                    )?,
                    sketch,
                    SketchGeometry::try_from(SketchGeometryDefinition::Point {
                        position: point.coordinates(),
                    })
                    .ok()?,
                )
                .with_native_ref(Some(point.id.clone())),
            )
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
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(start.x, start.y),
                    end: Point2::new(end.x, end.y),
                })
                .ok()?
            }
            SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } if planar_point(center)
                && reference_direction.z.abs() <= EPS_SKETCH_PROJECT_PROJECT_SKETCH_DESIGN_E9
                && *radius > 0.0 =>
            {
                let orientation = sketch_normal_sign(normal)?;
                let phase = reference_direction.y.atan2(reference_direction.x);
                let start_angle = phase + orientation * start_angle;
                let end_angle = phase + orientation * end_angle;
                if (end_angle - start_angle).abs()
                    >= std::f64::consts::TAU - EPS_SKETCH_PROJECT_PROJECT_SKETCH_DESIGN_E9
                {
                    SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                        center: Point2::new(center.x, center.y),
                        radius: Length::new(*radius)?,
                    })
                    .ok()?
                } else {
                    SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                        center: Point2::new(center.x, center.y),
                        radius: Length::new(*radius)?,
                        start_angle: Angle::new(start_angle)?,
                        end_angle: Angle::new(end_angle)?,
                    })
                    .ok()?
                }
            }
            SketchCurveGeometry::Nurbs {
                degree,
                knots,
                poles,
                ..
            } if *degree != 0
                && usize::try_from(*degree).is_ok_and(|degree| poles.point_count() > degree)
                && poles.points().all(planar_point) =>
            {
                SketchGeometry::nurbs(
                    cadmpeg_ir::geometry::PcurveNurbs::new(
                        *degree,
                        knots.clone(),
                        poles
                            .points()
                            .map(|point| Point2::new(point.x, point.y))
                            .collect(),
                        poles
                            .weights()
                            .next()
                            .is_some()
                            .then(|| poles.weights().copied().collect()),
                        false,
                    )
                    .ok()?,
                )
            }
            _ => return None,
        };
        let sketch = neutral_sketch_id(placement)?;
        Some(
            SketchEntity::new(
                neutral_sketch_curve_id(&sketch, curve.primary_id.get(), curve.secondary_id)?,
                sketch,
                geometry,
            )
            .with_construction(text_frame_curves.contains(&(scope.to_owned(), curve.record_index)))
            .with_native_ref(Some(curve.id.clone())),
        )
    }));
    entities.extend(texts.iter().filter_map(|text| {
        let scope = native_stream(&text.id)?;
        let placement = placements_by_suffix.get(&(scope, text.owner_reference))?;
        let sketch = neutral_sketch_id(placement)?;
        Some(
            SketchEntity::new(
                text.persistent_id.map_or_else(
                    || neutral_sketch_record_id(&sketch, text.record_index),
                    |persistent_id| neutral_sketch_text_id(&sketch, persistent_id),
                )?,
                sketch,
                SketchGeometry::try_from(SketchGeometryDefinition::Text {
                    text: cadmpeg_ir::products::NonEmptyString::new(text.text.clone())?,
                    font_family: cadmpeg_ir::products::NonEmptyString::new(
                        text.font_family.clone(),
                    )?,
                    font_weight: text.font_weight.try_into().ok()?,
                    height: Length::new(text.height)?,
                    // The record's `0` does not scale glyph advance to zero, so it
                    // is not a neutral horizontal scale of zero; only a positive
                    // factor carries one.
                    width_factor: text.width_factor().filter(|factor| *factor > 0.0),
                    placement: text.placement(),
                    horizontal_alignment: sketch_text_horizontal_alignment(
                        text.alignment().map(|alignment| alignment.horizontal),
                    ),
                    vertical_alignment: sketch_text_vertical_alignment(
                        text.alignment().map(|alignment| alignment.vertical),
                    ),
                })
                .ok()?,
            )
            .with_native_ref(Some(text.id.clone())),
        )
    }));
    entities.sort_by(|a, b| a.id().cmp(b.id()));
    for sketch in &mut sketches {
        let Ok(profiles) = cadmpeg_ir::sketches::SketchProfiles::try_from(closed_sketch_profiles(
            &sketch.id,
            &entities,
            linear_tolerance,
        )) else {
            continue;
        };
        sketch.profiles = profiles;
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
) -> Result<
    (
        Vec<cadmpeg_ir::sketches::SpatialSketch>,
        Vec<cadmpeg_ir::sketches::SpatialSketchEntity>,
    ),
    cadmpeg_core::CodecError,
> {
    use cadmpeg_ir::scalar::{Angle, Length};
    use cadmpeg_ir::sketches::{
        SpatialSketch, SpatialSketchEntity, SpatialSketchGeometry, SpatialSketchGeometryDefinition,
    };

    let placements_by_suffix = placements
        .iter()
        .filter_map(|placement| {
            Some((
                (
                    native_stream(&placement.id)?,
                    u32::try_from(placement.entity_id.suffix()).ok()?,
                ),
                placement,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut spatial_owners = spatial_geometry_owners(points, curves);
    spatial_owners.extend(surfaces.iter().filter_map(|surface| {
        Some((
            native_stream(&surface.id)?.to_owned(),
            surface.owner_reference?,
        ))
    }));
    let curves_by_record = curves
        .iter()
        .filter_map(|curve| Some(((native_stream(&curve.id)?, curve.record_index), curve)))
        .collect::<HashMap<_, _>>();
    let mut spline_segments = HashMap::new();
    for relation in relations {
        // Only the second reference run of a relation record is in semantic
        // order: the control polygon ends with the spline there, and the
        // interleaved first run orders its members by nothing a reader can use.
        let members = relation.return_member_indices();
        if relation.unknown_constraint_bits() != 0
            || relation.constraint_kinds() != [SketchConstraintKind::SplineGroup]
            || members.len() < 2
            || members.iter().collect::<HashSet<_>>().len() != members.len()
        {
            continue;
        }
        let Some(scope) = native_stream(&relation.id) else {
            continue;
        };
        let Some(curve) = members
            .last()
            .and_then(|record| curves_by_record.get(&(scope, *record)))
        else {
            continue;
        };
        let Some(SketchCurveGeometry::Nurbs { poles, .. }) = curve.geometry.as_ref() else {
            continue;
        };
        if curve.owner_reference != Some(relation.owner_reference)
            || poles.point_count() != members.len()
        {
            continue;
        }
        let segments = members[..members.len() - 1]
            .iter()
            .zip(poles.points().zip(poles.points().skip(1)))
            .map(|(record, (first, second))| {
                let member = curves_by_record.get(&(scope, *record))?;
                if member.owner_reference != Some(relation.owner_reference) {
                    return None;
                }
                match member.geometry.as_ref() {
                    None => Some((*record, [*first, *second])),
                    Some(SketchCurveGeometry::Line { start, end, .. })
                        if start == first && end == second =>
                    {
                        Some((*record, [*first, *second]))
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
            placement.transform()[0][0] * point.x
                + placement.transform()[0][1] * point.y
                + placement.transform()[0][2] * point.z
                + placement.transform()[0][3] * origin_scale,
            placement.transform()[1][0] * point.x
                + placement.transform()[1][1] * point.y
                + placement.transform()[1][2] * point.z
                + placement.transform()[1][3] * origin_scale,
            placement.transform()[2][0] * point.x
                + placement.transform()[2][1] * point.y
                + placement.transform()[2][2] * point.z
                + placement.transform()[2][3] * origin_scale,
        )
    };
    let transform_vector = |placement: &DesignSketchPlacement, vector: &Vector3| {
        Vector3::new(
            placement.transform()[0][0] * vector.x
                + placement.transform()[0][1] * vector.y
                + placement.transform()[0][2] * vector.z,
            placement.transform()[1][0] * vector.x
                + placement.transform()[1][1] * vector.y
                + placement.transform()[1][2] * vector.z,
            placement.transform()[2][0] * vector.x
                + placement.transform()[2][1] * vector.y
                + placement.transform()[2][2] * vector.z,
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
                SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                    start: transform_point(placement, &start),
                    end: transform_point(placement, &end),
                })
                .ok()?
            } else {
                match curve.geometry.as_ref()? {
                    SketchCurveGeometry::Line { start, end, .. } => {
                        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                            start: transform_point(placement, start),
                            end: transform_point(placement, end),
                        })
                        .ok()?
                    }
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
                        if (end_angle - start_angle).abs()
                            >= std::f64::consts::TAU
                                - EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_DESIGN_E9
                        {
                            SpatialSketchGeometry::try_from(
                                SpatialSketchGeometryDefinition::Circle {
                                    center,
                                    normal,
                                    reference_direction,
                                    radius: Length::new(*radius)?,
                                },
                            )
                            .ok()?
                        } else {
                            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Arc {
                                center,
                                normal,
                                reference_direction,
                                radius: Length::new(*radius)?,
                                start_angle: Angle::new(*start_angle)?,
                                end_angle: Angle::new(*end_angle)?,
                            })
                            .ok()?
                        }
                    }
                    SketchCurveGeometry::Nurbs {
                        degree,
                        knots,
                        poles,
                        ..
                    } => SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Nurbs {
                        curve: cadmpeg_ir::geometry::NurbsCurve::new(
                            *degree,
                            knots.clone(),
                            poles
                                .points()
                                .map(|point| transform_point(placement, point))
                                .collect(),
                            poles
                                .weights()
                                .next()
                                .is_some()
                                .then(|| poles.weights().copied().collect()),
                            false,
                        )
                        .ok()?
                        .try_into()
                        .ok()?,
                    })
                    .ok()?,
                    SketchCurveGeometry::Arc { .. } => return None,
                }
            };
            let sketch = neutral_spatial_sketch_id(placement)?;
            Some(
                SpatialSketchEntity::new(
                    neutral_spatial_sketch_curve_id(
                        &sketch,
                        curve.primary_id.get(),
                        curve.secondary_id,
                    )?,
                    sketch,
                    geometry,
                )
                .with_native_ref(Some(curve.id.clone())),
            )
        })
        .collect::<Vec<_>>();
    entities.extend(points.iter().filter_map(|point| {
        let scope = native_stream(&point.id)?;
        let owner = point.owner_reference?;
        if !spatial_owners.contains(&(scope.to_owned(), owner)) {
            return None;
        }
        let placement = placements_by_suffix.get(&(scope, owner))?;
        let sketch = neutral_spatial_sketch_id(placement)?;
        let depth = sketch_point_depth(point)?;
        Some(
            SpatialSketchEntity::new(
                point.persistent_id().map_or_else(
                    || neutral_spatial_sketch_record_id(&sketch, point.record_index),
                    |persistent_id| neutral_spatial_sketch_point_id(&sketch, persistent_id),
                )?,
                sketch,
                SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
                    position: transform_point(
                        placement,
                        &Point3::new(point.coordinates().u, point.coordinates().v, depth),
                    ),
                })
                .ok()?,
            )
            .with_native_ref(Some(point.id.clone())),
        )
    }));
    for surface in surfaces {
        let Some(scope) = native_stream(&surface.id) else {
            continue;
        };
        let Some(owner) = surface.owner_reference else {
            continue;
        };
        let Some(placement) = placements_by_suffix.get(&(scope, owner)) else {
            continue;
        };
        let Some(sketch) = neutral_spatial_sketch_id(placement) else {
            continue;
        };
        let Some(entity_id) =
            neutral_spatial_sketch_surface_id(&sketch, surface.persistent_id.get())
        else {
            continue;
        };
        entities.push(
            SpatialSketchEntity::new(
                entity_id,
                sketch,
                SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::NurbsSurface {
                    surface: cadmpeg_ir::geometry::BsplineSurface::new(
                        surface.u_degree,
                        surface.v_degree,
                        surface.u_knots.clone(),
                        surface.v_knots.clone(),
                        surface
                            .control_points
                            .iter()
                            .map(|row| {
                                row.iter()
                                    .map(|point| transform_point(placement, point))
                                    .collect()
                            })
                            .collect(),
                    )
                    .map_err(|error| {
                        cadmpeg_core::CodecError::malformed(format_args!(
                            "F3D spatial sketch surface {} is invalid: {error}",
                            surface.id
                        ))
                    })?,
                })
                .map_err(cadmpeg_core::CodecError::malformed)?,
            )
            .with_native_ref(Some(surface.id.clone())),
        );
    }
    entities.sort_by(|a, b| a.id().cmp(b.id()));
    let spatial_ids = entities
        .iter()
        .map(|entity| entity.sketch.clone())
        .collect::<HashSet<_>>();
    let mut sketches = placements
        .iter()
        .filter(|placement| {
            neutral_spatial_sketch_id(placement).is_some_and(|id| spatial_ids.contains(&id))
        })
        .filter_map(|placement| {
            let id = neutral_spatial_sketch_id(placement)?;
            Some(SpatialSketch {
                profiles: closed_spatial_sketch_profiles(&id, &entities, linear_tolerance),
                id,
                name: Some(placement.entity_id.as_str().to_owned()),
                configuration: None,
                visible: placement
                    .visibility
                    .as_ref()
                    .map(|visibility| visibility.visible),
                native_ref: Some(placement.id.clone()),
            })
        })
        .collect::<Vec<_>>();
    sketches.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((sketches, entities))
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
        SpatialSketchConstraint, SpatialSketchConstraintDefinitionInput as Definition,
        SpatialSketchGeometry, SpatialSketchGeometryDefinition,
    };

    let spatial_sketches = entities
        .iter()
        .map(|entity| entity.sketch.clone())
        .collect::<HashSet<_>>();
    let sketches = placements
        .iter()
        .filter_map(|placement| {
            let id = neutral_spatial_sketch_id(placement)?;
            spatial_sketches.contains(&id).then_some((
                (
                    native_stream(&placement.id)?,
                    u32::try_from(placement.entity_id.suffix()).ok()?,
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
            if relation.unknown_constraint_bits() != 0 || relation.constraint_kinds().len() != 1 {
                return None;
            }
            let scope = native_stream(&relation.id)?;
            let (sketch, placement) = sketches.get(&(scope, relation.owner_reference))?;
            // The second relation run is the semantic member order. The first
            // run interleaves per-member relation ordinals and has no role
            // order, so it cannot define a neutral spatial constraint.
            let semantic_entities = relation
                .return_members()
                .iter()
                .map(|member| {
                    projected
                        .get(&(scope, member.reference.record_index()))
                        .copied()
                })
                .collect::<Option<Vec<_>>>()?;
            let members = semantic_entities
                .iter()
                .map(|entity| entity.id().clone())
                .collect::<Vec<_>>();
            let distinct = members.iter().collect::<HashSet<_>>();
            if distinct.len() != members.len() {
                return None;
            }
            let definition = match relation.constraint_kinds()[0] {
                SketchConstraintKind::Coincident => {
                    let [first, second] = semantic_entities.as_slice() else {
                        return None;
                    };
                    let point_on_surface =
                        match (first.geometry.definition(), second.geometry.definition()) {
                            (
                                SpatialSketchGeometryDefinition::Point { .. },
                                SpatialSketchGeometryDefinition::NurbsSurface { .. },
                            ) => Some((first, second)),
                            (
                                SpatialSketchGeometryDefinition::NurbsSurface { .. },
                                SpatialSketchGeometryDefinition::Point { .. },
                            ) => Some((second, first)),
                            _ => None,
                        };
                    if let Some((point, surface)) = point_on_surface {
                        Definition::PointOnSurface {
                            point: point.id().clone(),
                            surface: surface.id().clone(),
                        }
                    } else {
                        let (
                            SpatialSketchGeometryDefinition::Point {
                                position: first_position,
                            },
                            SpatialSketchGeometryDefinition::Point {
                                position: second_position,
                            },
                        ) = (first.geometry.definition(), second.geometry.definition())
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
                        if (first_position.x - second_position.x).abs()
                            > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                            || (first_position.y - second_position.y).abs()
                                > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                            || (first_position.z - second_position.z).abs()
                                > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                        {
                            return None;
                        }
                        Definition::Coincident {
                            first: first.id().clone(),
                            second: second.id().clone(),
                        }
                    }
                }
                SketchConstraintKind::SplineGroup if members.len() >= 2 => {
                    Definition::SplineGroup { entities: members }
                }
                SketchConstraintKind::Tangent => {
                    let [first, second] = semantic_entities.as_slice() else {
                        return None;
                    };
                    let curve = |geometry: &SpatialSketchGeometry| {
                        matches!(
                            (geometry).definition(),
                            SpatialSketchGeometryDefinition::Line { .. }
                                | SpatialSketchGeometryDefinition::Circle { .. }
                                | SpatialSketchGeometryDefinition::Arc { .. }
                                | SpatialSketchGeometryDefinition::Nurbs { .. }
                        )
                    };
                    if !curve(&first.geometry) || !curve(&second.geometry) {
                        return None;
                    }
                    Definition::Tangent {
                        first: first.id().clone(),
                        second: second.id().clone(),
                    }
                }
                SketchConstraintKind::Midpoint => {
                    let [first, second] = semantic_entities.as_slice() else {
                        return None;
                    };
                    let (point, line, position, start, end) =
                        match (first.geometry.definition(), second.geometry.definition()) {
                            (
                                SpatialSketchGeometryDefinition::Point { position },
                                SpatialSketchGeometryDefinition::Line { start, end },
                            ) => (first, second, position, start, end),
                            (
                                SpatialSketchGeometryDefinition::Line { start, end },
                                SpatialSketchGeometryDefinition::Point { position },
                            ) => (second, first, position, start, end),
                            _ => return None,
                        };
                    let midpoint = Point3::new(
                        (start.x + end.x) * 0.5,
                        (start.y + end.y) * 0.5,
                        (start.z + end.z) * 0.5,
                    );
                    let scale = 1.0 + midpoint.x.abs().max(midpoint.y.abs()).max(midpoint.z.abs());
                    if (position.x - midpoint.x).abs()
                        > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                        || (position.y - midpoint.y).abs()
                            > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                        || (position.z - midpoint.z).abs()
                            > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                    {
                        return None;
                    }
                    Definition::Midpoint {
                        point: point.id().clone(),
                        entity: line.id().clone(),
                    }
                }
                SketchConstraintKind::Horizontal | SketchConstraintKind::Vertical => {
                    let [entity] = semantic_entities.as_slice() else {
                        return None;
                    };
                    let SpatialSketchGeometryDefinition::Line { start, end } =
                        *entity.geometry.definition()
                    else {
                        return None;
                    };
                    let direction = match relation.constraint_kinds()[0] {
                        SketchConstraintKind::Horizontal => Vector3::new(
                            placement.transform()[0][0],
                            placement.transform()[1][0],
                            placement.transform()[2][0],
                        ),
                        SketchConstraintKind::Vertical => Vector3::new(
                            placement.transform()[0][1],
                            placement.transform()[1][1],
                            placement.transform()[2][1],
                        ),
                        _ => unreachable!(),
                    };
                    let line = end.vector_from(start);
                    let cross = line.cross(direction);
                    if line.norm() <= EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E12
                        || cross.norm()
                            > EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9 * line.norm()
                    {
                        return None;
                    }
                    Definition::ParallelToDirection {
                        entity: entity.id().clone(),
                        direction,
                    }
                }
                _ => return None,
            };
            Some(SpatialSketchConstraint {
                id: neutral_sketch_constraint_id(&relation.id, relation.record_index)?,
                sketch: sketch.clone(),
                definition: cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::try_from(
                    definition,
                )
                .ok()?,
                native_ref: Some(relation.id.clone()),
            })
        })
        .collect::<Vec<_>>();
    constraints.sort_by(|a, b| a.id.cmp(&b.id));
    constraints
}

#[cfg(test)]
mod tests;
