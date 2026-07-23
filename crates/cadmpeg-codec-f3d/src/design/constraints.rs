// SPDX-License-Identifier: Apache-2.0
//! Project sketch constraint relations.

use crate::design::dimensions::{
    exact_atomic_constraint, exact_coincident_loci, exact_offset_constraint, relation_kind_name,
};
use crate::design::face_resolve::design_angle;
use crate::design::feature_project::design_length;
use crate::ids::{
    native_stream, neutral_parameter_id, neutral_sketch_constraint_id, neutral_sketch_id,
};
use crate::records::{
    DesignParameter, DesignSketchPlacement, SketchConstraintKind, SketchCurveIdentity, SketchPoint,
    SketchRelation, SketchText,
};
use cadmpeg_ir::math::Point2;
use std::collections::{HashMap, HashSet};

/// Project each native relation as an exact atomic constraint or an explicitly
/// native aggregate when its member roles do not determine neutral loci.
pub fn project_sketch_constraints(
    placements: &[DesignSketchPlacement],
    parameters: &[DesignParameter],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    texts: &[SketchText],
    relations: &[SketchRelation],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Vec<cadmpeg_ir::sketches::SketchConstraint> {
    use cadmpeg_ir::sketches::{
        SketchConstraint, SketchConstraintDefinition as Definition, SketchNativeOperand,
    };

    let planar_sketches = entities
        .iter()
        .map(|entity| entity.sketch.clone())
        .collect::<HashSet<_>>();
    let sketches = placements
        .iter()
        .filter_map(|placement| {
            let id = neutral_sketch_id(placement);
            if !planar_sketches.contains(&id) {
                return None;
            }
            Some((
                (
                    native_stream(&placement.id)?,
                    u32::try_from(placement.entity_suffix).ok()?,
                ),
                id,
            ))
        })
        .collect::<HashMap<_, _>>();
    let record_keys_by_native_ref = points
        .iter()
        .filter_map(|point| {
            Some((
                point.id.as_str(),
                (native_stream(&point.id)?, point.record_index),
            ))
        })
        .chain(curves.iter().filter_map(|curve| {
            Some((
                curve.id.as_str(),
                (native_stream(&curve.id)?, curve.record_index),
            ))
        }))
        .chain(texts.iter().filter_map(|text| {
            Some((
                text.id.as_str(),
                (native_stream(&text.id)?, text.record_index),
            ))
        }))
        .collect::<HashMap<_, _>>();
    let projected = entities
        .iter()
        .filter_map(|entity| {
            entity
                .native_ref
                .as_deref()
                .and_then(|native_ref| record_keys_by_native_ref.get(native_ref).copied())
                .map(|key| (key, entity))
        })
        .collect::<HashMap<_, _>>();
    let point_native_refs = points
        .iter()
        .filter_map(|point| {
            Some((
                (native_stream(&point.id)?, point.record_index),
                point.id.as_str(),
            ))
        })
        .collect::<HashMap<_, _>>();
    let curve_native_refs = curves
        .iter()
        .filter_map(|curve| {
            Some((
                (native_stream(&curve.id)?, curve.record_index),
                curve.id.as_str(),
            ))
        })
        .collect::<HashMap<_, _>>();
    let text_native_refs = texts
        .iter()
        .filter_map(|text| {
            Some((
                (native_stream(&text.id)?, text.record_index),
                text.id.as_str(),
            ))
        })
        .collect::<HashMap<_, _>>();
    let native_operand = |scope: &str, field: &str, record_index: u32| {
        let (family, native_ref) = if let Some(native_ref) =
            point_native_refs.get(&(scope, record_index)).copied()
        {
            ("point", Some(native_ref))
        } else if let Some(native_ref) = curve_native_refs.get(&(scope, record_index)).copied() {
            ("curve", Some(native_ref))
        } else if let Some(native_ref) = text_native_refs.get(&(scope, record_index)).copied() {
            ("text", Some(native_ref))
        } else {
            ("record", None)
        };
        SketchNativeOperand {
            native_kind: family.into(),
            native_field: Some(field.into()),
            native_role: None,
            object_index: record_index,
            native_ref: native_ref
                .filter(|_| !projected.contains_key(&(scope, record_index)))
                .map(str::to_owned),
        }
    };

    let projected_constraints = relations.iter().filter_map(|relation| {
        let scope = native_stream(&relation.id)?;
        let sketch = sketches.get(&(scope, relation.owner_reference))?.clone();
        let member_entities = relation
            .members
            .iter()
            .filter_map(|record_index| projected.get(&(scope, *record_index)).copied())
            .collect::<Vec<_>>();
        if relation.constraint_kinds == [SketchConstraintKind::SplineGroup]
            && member_entities.is_empty()
        {
            return None;
        }
        let return_entities = relation
            .return_members
            .iter()
            .filter_map(|record_index| projected.get(&(scope, *record_index)).copied())
            .collect::<Vec<_>>();
        let exact = relation.unknown_constraint_bits == 0
            && relation.constraint_kinds.len() == 1
            && member_entities.len() == relation.members.len();
        let native_entities = || {
            relation
                .members
                .iter()
                .chain(&relation.auxiliary_references)
                .chain(&relation.return_members)
                .filter_map(|record_index| {
                    projected
                        .get(&(scope, *record_index))
                        .map(|entity| entity.id.clone())
                })
                .collect()
        };
        let definition = (if exact {
            let kind = relation.constraint_kinds[0];
            let loci = if kind == SketchConstraintKind::Coincident {
                exact_coincident_loci(&member_entities)
            } else {
                None
            };
            loci.or_else(|| exact_atomic_constraint(kind, &member_entities))
        } else {
            None
        })
        .or_else(|| exact_rectangular_pattern(relation, scope, parameters, &return_entities))
        .or_else(|| {
            exact_circular_pattern(
                relation,
                scope,
                parameters,
                &member_entities,
                &return_entities,
            )
        })
        .or_else(|| exact_offset_constraint(relation, scope, &projected))
        .or_else(|| exact_text_relation(relation, scope, &projected))
        .unwrap_or_else(|| Definition::Native {
            native_kind: relation_kind_name(relation),
            native_state: Some(relation.state),
            native_flags: None,
            entities: native_entities(),
            parameter: None,
            operands: relation
                .members
                .iter()
                .map(|record_index| native_operand(scope, "member", *record_index))
                .chain(
                    relation
                        .auxiliary_references
                        .iter()
                        .map(|record_index| native_operand(scope, "auxiliary", *record_index)),
                )
                .chain(
                    relation
                        .return_members
                        .iter()
                        .map(|record_index| native_operand(scope, "return", *record_index)),
                )
                .collect(),
        });
        Some(SketchConstraint {
            id: neutral_sketch_constraint_id(&relation.id, relation.record_index),
            sketch,
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
            native_ref: Some(relation.id.clone()),
        })
    });
    let mut constraints = projected_constraints.collect::<Vec<_>>();
    constraints.sort_by_key(|constraint| constraint.id.clone());
    constraints
}

pub(crate) fn exact_rectangular_pattern(
    relation: &SketchRelation,
    scope: &str,
    parameters: &[DesignParameter],
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use crate::records::SketchPatternDefinition;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchPatternDirection, SketchPatternInstance,
    };

    if relation.unknown_constraint_bits != 0
        || relation.constraint_kinds != [SketchConstraintKind::RectangularPattern]
        || entities.len() != relation.return_members.len()
    {
        return None;
    }
    let SketchPatternDefinition::Rectangular { directions } = relation.pattern.as_ref()? else {
        return None;
    };
    let directions = directions
        .iter()
        .map(|direction| {
            if direction.direction[2].abs() > 1.0e-9 {
                return None;
            }
            let count_parameter = parameters.iter().find(|parameter| {
                native_stream(&parameter.id) == Some(scope)
                    && parameter.owner_record_index == Some(direction.count_parameter)
            });
            let span_parameter = parameters.iter().find(|parameter| {
                native_stream(&parameter.id) == Some(scope)
                    && parameter.owner_record_index == Some(direction.distance_parameter)
            });
            let count = direction.evaluated_count;
            if count_parameter
                .is_some_and(|parameter| !scalar_close(parameter.evaluated_value, f64::from(count)))
            {
                return None;
            }
            let span = cadmpeg_ir::features::Length(direction.evaluated_distance * 10.0);
            if !span.0.is_finite()
                || span_parameter.is_some_and(|parameter| {
                    design_length(parameter).is_none_or(|value| !scalar_close(value.0, span.0))
                })
            {
                return None;
            }
            let spacing = cadmpeg_ir::features::Length(if count > 1 {
                span.0 / f64::from(count - 1)
            } else {
                0.0
            });
            Some(SketchPatternDirection {
                direction: [direction.direction[0], direction.direction[1]],
                spacing,
                count,
                span_parameter: span_parameter.map(neutral_parameter_id),
                count_parameter: count_parameter.map(neutral_parameter_id),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let directions: [SketchPatternDirection; 2] = directions.try_into().ok()?;
    if directions.iter().any(|direction| {
        let length = direction.direction[0].hypot(direction.direction[1]);
        !scalar_close(length, 1.0)
    }) {
        return None;
    }
    let dot = directions[0].direction[0] * directions[1].direction[0]
        + directions[0].direction[1] * directions[1].direction[1];
    if dot.abs() > 1.0e-9 {
        return None;
    }
    let instance_count = usize::try_from(directions[0].count)
        .ok()?
        .checked_mul(usize::try_from(directions[1].count).ok()?)?;
    if instance_count == 0 || !entities.len().is_multiple_of(instance_count) {
        return None;
    }
    let entity_count = entities.len() / instance_count;
    let seed = entities.get(..entity_count)?;
    if seed.is_empty() {
        return None;
    }
    let mut instances = Vec::with_capacity(instance_count);
    let mut occupied = HashSet::new();
    for instance in entities.chunks_exact(entity_count) {
        let candidates = (0..directions[0].count)
            .flat_map(|first| (0..directions[1].count).map(move |second| [first, second]))
            .filter(|indices| {
                let translation = Point2::new(
                    f64::from(indices[0]) * directions[0].spacing.0 * directions[0].direction[0]
                        + f64::from(indices[1])
                            * directions[1].spacing.0
                            * directions[1].direction[0],
                    f64::from(indices[0]) * directions[0].spacing.0 * directions[0].direction[1]
                        + f64::from(indices[1])
                            * directions[1].spacing.0
                            * directions[1].direction[1],
                );
                seed.iter().zip(instance).all(|(source, result)| {
                    translated_sketch_geometry_matches(
                        &source.geometry,
                        &result.geometry,
                        translation,
                    )
                })
            })
            .collect::<Vec<_>>();
        let [indices] = candidates.as_slice() else {
            return None;
        };
        if !occupied.insert(*indices) {
            return None;
        }
        instances.push(SketchPatternInstance {
            indices: *indices,
            entities: instance.iter().map(|entity| entity.id.clone()).collect(),
        });
    }
    if instances.first().map(|instance| instance.indices) != Some([0, 0]) {
        return None;
    }
    Some(Definition::RectangularPattern {
        directions,
        instances,
    })
}

pub(crate) fn exact_text_relation(
    relation: &SketchRelation,
    scope: &str,
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use crate::records::SketchPatternDefinition;
    use cadmpeg_ir::sketches::{SketchConstraintDefinition as Definition, SketchGeometry};
    use cadmpeg_ir::transform::Transform;

    if relation.unknown_constraint_bits != 0 || relation.constraint_kinds.len() != 1 {
        return None;
    }
    match relation.pattern.as_ref()? {
        SketchPatternDefinition::TextFrame { text_reference }
            if relation.constraint_kinds == [SketchConstraintKind::TextFrame]
                && relation.members.first() == Some(text_reference)
                && relation.auxiliary_references == [*text_reference]
                && relation.return_members == relation.members[1..] =>
        {
            let text = projected.get(&(scope, *text_reference))?;
            if !matches!(text.geometry, SketchGeometry::Text { .. }) {
                return None;
            }
            let frame = relation
                .return_members
                .iter()
                .map(|record_index| projected.get(&(scope, *record_index)).copied())
                .collect::<Option<Vec<_>>>()?;
            (!frame.is_empty()
                && frame.iter().all(|entity| {
                    entity.id != text.id && !matches!(entity.geometry, SketchGeometry::Text { .. })
                }))
            .then(|| Definition::TextFrame {
                text: text.id.clone(),
                frame: frame.into_iter().map(|entity| entity.id.clone()).collect(),
            })
        }
        SketchPatternDefinition::TextPath {
            text_reference,
            glyph_transforms,
        } if relation.constraint_kinds == [SketchConstraintKind::TextPath]
            && relation.members.len() == 2
            && relation.member_roles.len() == 2
            && relation.member_roles[0] != 0
            && relation.member_roles[1] == 0
            && relation.members[1] == *text_reference
            && relation.auxiliary_references == [*text_reference]
            && relation.return_members == [relation.members[0]] =>
        {
            let path = projected.get(&(scope, relation.members[0]))?;
            let text = projected.get(&(scope, *text_reference))?;
            if path.id == text.id
                || matches!(
                    path.geometry,
                    SketchGeometry::Point { .. } | SketchGeometry::Text { .. }
                )
                || !matches!(text.geometry, SketchGeometry::Text { .. })
                || glyph_transforms.is_empty()
                || glyph_transforms
                    .iter()
                    .flatten()
                    .flatten()
                    .any(|value| !value.is_finite())
            {
                return None;
            }
            let glyph_transforms = glyph_transforms
                .iter()
                .map(|source| {
                    let mut rows = *source;
                    for row in rows.iter_mut().take(3) {
                        row[3] *= 10.0;
                    }
                    Transform { rows }
                })
                .collect();
            Some(Definition::TextPath {
                text: text.id.clone(),
                path: path.id.clone(),
                glyph_transforms,
            })
        }
        _ => None,
    }
}

pub(crate) fn exact_circular_pattern(
    relation: &SketchRelation,
    scope: &str,
    parameters: &[DesignParameter],
    members: &[&cadmpeg_ir::sketches::SketchEntity],
    returned: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use crate::records::SketchPatternDefinition;
    use cadmpeg_ir::sketches::{
        SketchCircularPatternInstance, SketchConstraintDefinition as Definition, SketchGeometry,
    };

    if relation.unknown_constraint_bits != 0
        || relation.constraint_kinds != [SketchConstraintKind::CircularPattern]
        || members.len() != relation.members.len()
        || returned.len() != relation.return_members.len()
    {
        return None;
    }
    let SketchPatternDefinition::Circular {
        angle_parameter,
        count_parameter,
        evaluated_angle,
        evaluated_count,
    } = relation.pattern.as_ref()?
    else {
        return None;
    };
    let angle_parameter = parameters.iter().find(|parameter| {
        native_stream(&parameter.id) == Some(scope)
            && parameter.owner_record_index == Some(*angle_parameter)
    });
    let count_parameter = parameters.iter().find(|parameter| {
        native_stream(&parameter.id) == Some(scope)
            && parameter.owner_record_index == Some(*count_parameter)
    });
    let angle = cadmpeg_ir::features::Angle(*evaluated_angle);
    if !evaluated_angle.is_finite()
        || angle_parameter.is_some_and(|parameter| {
            design_angle(parameter).is_none_or(|value| !scalar_close(value.0, angle.0))
        })
        || count_parameter.is_some_and(|parameter| {
            !scalar_close(parameter.evaluated_value, f64::from(*evaluated_count))
        })
        || *evaluated_count == 0
    {
        return None;
    }
    let input = members
        .iter()
        .zip(&relation.member_roles)
        .filter_map(|(entity, role)| (*role != 0).then_some(*entity))
        .collect::<Vec<_>>();
    let input_ids = input
        .iter()
        .map(|entity| &entity.id)
        .collect::<HashSet<_>>();
    let generated_ids = members
        .iter()
        .zip(&relation.member_roles)
        .filter_map(|(entity, role)| (*role == 0).then_some(&entity.id))
        .collect::<HashSet<_>>();
    let mut candidates = Vec::new();
    for center in input.iter().copied() {
        let SketchGeometry::Point {
            position: center_position,
        } = center.geometry
        else {
            continue;
        };
        if !returned.iter().any(|entity| entity.id == center.id) {
            continue;
        }
        let patterned = returned
            .iter()
            .copied()
            .filter(|entity| entity.id != center.id)
            .collect::<Vec<_>>();
        let count = usize::try_from(*evaluated_count).ok()?;
        if patterned.is_empty() || !patterned.len().is_multiple_of(count) {
            continue;
        }
        let arity = patterned.len() / count;
        let seed = &patterned[..arity];
        if seed.is_empty()
            || !seed.iter().all(|entity| input_ids.contains(&entity.id))
            || patterned[arity..]
                .iter()
                .map(|entity| &entity.id)
                .collect::<HashSet<_>>()
                != generated_ids
        {
            continue;
        }
        let mut divisors = vec![f64::from(*evaluated_count)];
        if *evaluated_count > 1 {
            divisors.push(f64::from(*evaluated_count - 1));
        }
        divisors.dedup_by(|left, right| scalar_close(*left, *right));
        for divisor in divisors {
            let instances = patterned
                .chunks_exact(arity)
                .enumerate()
                .map(|(index, instance)| {
                    let rotation = *evaluated_angle * index as f64 / divisor;
                    seed.iter()
                        .zip(instance)
                        .all(|(source, result)| {
                            rotated_sketch_geometry_matches(
                                &source.geometry,
                                &result.geometry,
                                center_position,
                                rotation,
                            )
                        })
                        .then(|| SketchCircularPatternInstance {
                            index: index as u32,
                            angle: cadmpeg_ir::features::Angle(rotation),
                            entities: instance.iter().map(|entity| entity.id.clone()).collect(),
                        })
                })
                .collect::<Option<Vec<_>>>();
            if let Some(instances) = instances {
                candidates.push((center.id.clone(), instances));
            }
        }
    }
    candidates.dedup();
    let [(center, instances)] = candidates.as_slice() else {
        return None;
    };
    Some(Definition::CircularPattern {
        center: center.clone(),
        angle,
        count: *evaluated_count,
        angle_parameter: angle_parameter.map(neutral_parameter_id),
        count_parameter: count_parameter.map(neutral_parameter_id),
        instances: instances.clone(),
    })
}

fn rotated_sketch_geometry_matches(
    source: &cadmpeg_ir::sketches::SketchGeometry,
    result: &cadmpeg_ir::sketches::SketchGeometry,
    center: Point2,
    angle: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    let rotate = |point: Point2| {
        let (sin, cos) = angle.sin_cos();
        let u = point.u - center.u;
        let v = point.v - center.v;
        Point2::new(center.u + cos * u - sin * v, center.v + sin * u + cos * v)
    };
    let point_matches = |first: Point2, second: Point2| {
        let first = rotate(first);
        scalar_close(first.u, second.u) && scalar_close(first.v, second.v)
    };
    let angle_matches = |first: f64, second: f64| {
        let delta = (first + angle - second).rem_euclid(std::f64::consts::TAU);
        scalar_close(delta, 0.0) || scalar_close(delta, std::f64::consts::TAU)
    };
    match (source, result) {
        (SketchGeometry::Point { position: first }, SketchGeometry::Point { position: second }) => {
            point_matches(*first, *second)
        }
        (SketchGeometry::Line { start: a, end: b }, SketchGeometry::Line { start: c, end: d }) => {
            point_matches(*a, *c) && point_matches(*b, *d)
        }
        (
            SketchGeometry::Circle {
                center: a,
                radius: ar,
            },
            SketchGeometry::Circle {
                center: b,
                radius: br,
            },
        ) => point_matches(*a, *b) && scalar_close(ar.0, br.0),
        (
            SketchGeometry::Arc {
                center: a,
                radius: ar,
                start_angle: as_,
                end_angle: ae,
            },
            SketchGeometry::Arc {
                center: b,
                radius: br,
                start_angle: bs,
                end_angle: be,
            },
        ) => {
            point_matches(*a, *b)
                && scalar_close(ar.0, br.0)
                && angle_matches(as_.0, bs.0)
                && angle_matches(ae.0, be.0)
        }
        (
            SketchGeometry::Ellipse {
                center: a,
                major_angle: aa,
                major_radius: ar,
                minor_radius: ai,
                start_angle: as_,
                end_angle: ae,
            },
            SketchGeometry::Ellipse {
                center: b,
                major_angle: ba,
                major_radius: br,
                minor_radius: bi,
                start_angle: bs,
                end_angle: be,
            },
        ) => {
            point_matches(*a, *b)
                && angle_matches(aa.0, ba.0)
                && scalar_close(ar.0, br.0)
                && scalar_close(ai.0, bi.0)
                && optional_angle_matches(as_.as_ref(), bs.as_ref())
                && optional_angle_matches(ae.as_ref(), be.as_ref())
        }
        (
            SketchGeometry::Nurbs {
                degree: ad,
                knots: ak,
                control_points: ap,
                weights: aw,
                periodic: ax,
            },
            SketchGeometry::Nurbs {
                degree: bd,
                knots: bk,
                control_points: bp,
                weights: bw,
                periodic: bx,
            },
        ) => {
            ad == bd
                && ax == bx
                && equal_scalars(ak, bk)
                && ap.len() == bp.len()
                && ap.iter().zip(bp).all(|(a, b)| point_matches(*a, *b))
                && match (aw, bw) {
                    (None, None) => true,
                    (Some(a), Some(b)) => equal_scalars(a, b),
                    _ => false,
                }
        }
        _ => false,
    }
}

pub(crate) fn scalar_close(first: f64, second: f64) -> bool {
    first.is_finite()
        && second.is_finite()
        && (first - second).abs() <= 1.0e-9 * (1.0 + first.abs().max(second.abs()))
}

pub(crate) fn translated_sketch_geometry_matches(
    source: &cadmpeg_ir::sketches::SketchGeometry,
    result: &cadmpeg_ir::sketches::SketchGeometry,
    translation: Point2,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    let point_matches = |first: Point2, second: Point2| {
        scalar_close(first.u + translation.u, second.u)
            && scalar_close(first.v + translation.v, second.v)
    };
    match (source, result) {
        (SketchGeometry::Point { position: first }, SketchGeometry::Point { position: second }) => {
            point_matches(*first, *second)
        }
        (
            SketchGeometry::Line {
                start: first_start,
                end: first_end,
            },
            SketchGeometry::Line {
                start: second_start,
                end: second_end,
            },
        ) => point_matches(*first_start, *second_start) && point_matches(*first_end, *second_end),
        (
            SketchGeometry::Circle {
                center: first_center,
                radius: first_radius,
            },
            SketchGeometry::Circle {
                center: second_center,
                radius: second_radius,
            },
        ) => {
            point_matches(*first_center, *second_center)
                && scalar_close(first_radius.0, second_radius.0)
        }
        (
            SketchGeometry::Arc {
                center: first_center,
                radius: first_radius,
                start_angle: first_start,
                end_angle: first_end,
            },
            SketchGeometry::Arc {
                center: second_center,
                radius: second_radius,
                start_angle: second_start,
                end_angle: second_end,
            },
        ) => {
            point_matches(*first_center, *second_center)
                && scalar_close(first_radius.0, second_radius.0)
                && scalar_close(first_start.0, second_start.0)
                && scalar_close(first_end.0, second_end.0)
        }
        (
            SketchGeometry::Ellipse {
                center: first_center,
                major_angle: first_major_angle,
                major_radius: first_major_radius,
                minor_radius: first_minor_radius,
                start_angle: first_start,
                end_angle: first_end,
            },
            SketchGeometry::Ellipse {
                center: second_center,
                major_angle: second_major_angle,
                major_radius: second_major_radius,
                minor_radius: second_minor_radius,
                start_angle: second_start,
                end_angle: second_end,
            },
        ) => {
            point_matches(*first_center, *second_center)
                && scalar_close(first_major_angle.0, second_major_angle.0)
                && scalar_close(first_major_radius.0, second_major_radius.0)
                && scalar_close(first_minor_radius.0, second_minor_radius.0)
                && optional_angle_matches(first_start.as_ref(), second_start.as_ref())
                && optional_angle_matches(first_end.as_ref(), second_end.as_ref())
        }
        (
            SketchGeometry::Nurbs {
                degree: first_degree,
                knots: first_knots,
                control_points: first_points,
                weights: first_weights,
                periodic: first_periodic,
            },
            SketchGeometry::Nurbs {
                degree: second_degree,
                knots: second_knots,
                control_points: second_points,
                weights: second_weights,
                periodic: second_periodic,
            },
        ) => {
            *first_degree == *second_degree
                && first_periodic == second_periodic
                && equal_scalars(first_knots, second_knots)
                && first_points.len() == second_points.len()
                && first_points
                    .iter()
                    .zip(second_points)
                    .all(|(first, second)| point_matches(*first, *second))
                && match (first_weights, second_weights) {
                    (None, None) => true,
                    (Some(first), Some(second)) => equal_scalars(first, second),
                    _ => false,
                }
        }
        _ => false,
    }
}

fn optional_angle_matches(
    first: Option<&cadmpeg_ir::features::Angle>,
    second: Option<&cadmpeg_ir::features::Angle>,
) -> bool {
    match (first, second) {
        (None, None) => true,
        (Some(first), Some(second)) => scalar_close(first.0, second.0),
        _ => false,
    }
}

fn equal_scalars(first: &[f64], second: &[f64]) -> bool {
    first.len() == second.len()
        && first
            .iter()
            .zip(second)
            .all(|(first, second)| scalar_close(*first, *second))
}
