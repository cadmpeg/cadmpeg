// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::needless_range_loop))]
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

const EPS_CONSTRAINTS_EXACT_RECTANGULAR_PATTERN_E9: f64 = 1.0e-9;
const EPS_CONSTRAINTS_SCALAR_CLOSE_E9: f64 = 1.0e-9;

/// Project each native relation as an exact atomic constraint or an explicitly
/// native aggregate when its semantic members do not prove neutral loci.
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
        NativeOperandField, SketchConstraint, SketchConstraintDefinitionInput as Definition,
        SketchNativeOperand,
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
                    u32::try_from(placement.entity_id.suffix()).ok()?,
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
    let native_operand = |scope: &str, field: &'static str, record_index: u32| {
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
            native_kind: crate::design::literals::nonempty(family),
            field: Some(NativeOperandField {
                name: crate::design::literals::nonempty(field),
                role: None,
            }),
            object_index: Some(record_index),
            native_ref: native_ref
                .filter(|_| !projected.contains_key(&(scope, record_index)))
                .map(str::to_owned),
        }
    };

    let projected_constraints = relations.iter().filter_map(|relation| {
        let scope = native_stream(&relation.id)?;
        let sketch = sketches.get(&(scope, relation.owner_reference))?.clone();
        let input_entities = relation
            .members()
            .iter()
            .filter_map(|member| {
                projected
                    .get(&(scope, member.reference.record_index()))
                    .copied()
            })
            .collect::<Vec<_>>();
        // The second reference run is the relation's semantic member order.
        // The interleaved first run is retained separately because
        // circular-pattern decoding verifies both reference sets before using
        // the semantic order.
        let semantic_entities = relation
            .return_members()
            .iter()
            .filter_map(|member| {
                projected
                    .get(&(scope, member.reference.record_index()))
                    .copied()
            })
            .collect::<Vec<_>>();
        let sole_kind = relation
            .sole_constraint_kind()
            .filter(|_| semantic_entities.len() == relation.return_members().len());
        let native_entities = || {
            relation
                .member_indices()
                .into_iter()
                .chain(relation.auxiliary_references().values().copied())
                .chain(relation.return_member_indices())
                .filter_map(|record_index| {
                    projected
                        .get(&(scope, record_index))
                        .map(|entity| entity.id().clone())
                })
                .collect()
        };
        let definition = (if let Some(kind) = sole_kind {
            let loci = if kind == SketchConstraintKind::Coincident {
                exact_coincident_loci(&semantic_entities)
            } else {
                None
            };
            loci.or_else(|| exact_atomic_constraint(kind, &semantic_entities))
        } else {
            None
        })
        .or_else(|| exact_rectangular_pattern(relation, scope, parameters, &semantic_entities))
        .or_else(|| {
            exact_circular_pattern(
                relation,
                scope,
                parameters,
                &input_entities,
                &semantic_entities,
            )
        })
        .or_else(|| exact_offset_constraint(relation, scope, &projected))
        .or_else(|| exact_text_relation(relation, scope, &projected))
        .or_else(|| {
            Some(Definition::Native {
                native_kind: cadmpeg_ir::products::NonEmptyString::new(relation_kind_name(
                    relation,
                ))?,
                native_state: Some(relation.definition.state()),
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
                entities: native_entities(),
                parameter: None,
                operands: relation
                    .member_indices()
                    .into_iter()
                    .map(|record_index| native_operand(scope, "member", record_index))
                    .chain(
                        relation
                            .auxiliary_references()
                            .values()
                            .map(|record_index| native_operand(scope, "auxiliary", *record_index)),
                    )
                    .chain(
                        relation
                            .return_member_indices()
                            .into_iter()
                            .map(|record_index| native_operand(scope, "return", record_index)),
                    )
                    .collect(),
            })
        })?;
        Some(SketchConstraint {
            id: neutral_sketch_constraint_id(&relation.id, relation.record_index)?,
            sketch,
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
            native_ref: Some(relation.id.clone()),
        })
    });
    let mut constraints = projected_constraints.collect::<Vec<_>>();
    constraints.sort_by(|a, b| a.id.cmp(&b.id));
    constraints
}

struct RectangularPatternSourceDirection {
    direction: [f64; 2],
    count: u32,
    distance: f64,
    distance_parameter: Option<cadmpeg_ir::features::ParameterId>,
    count_parameter: Option<cadmpeg_ir::features::ParameterId>,
}

#[derive(Clone, Copy)]
enum RectangularPatternDistanceForm {
    AdjacentSpacing,
    SeedToFinalSpan,
}

pub(crate) fn exact_rectangular_pattern(
    relation: &SketchRelation,
    scope: &str,
    parameters: &[DesignParameter],
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use crate::records::SketchPatternDefinition;
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    if relation.sole_constraint_kind().is_none()
        || entities.len() != relation.return_members().len()
    {
        return None;
    }
    let distance_form = match relation.rectangular_counted_reference_count? {
        0 => RectangularPatternDistanceForm::SeedToFinalSpan,
        _ => RectangularPatternDistanceForm::AdjacentSpacing,
    };
    let pattern = relation.definition.pattern();
    let Some(SketchPatternDefinition::Rectangular { directions }) = pattern else {
        return None;
    };
    let source = directions
        .iter()
        .map(|direction| {
            if direction.direction[2].abs() > EPS_CONSTRAINTS_EXACT_RECTANGULAR_PATTERN_E9 {
                return None;
            }
            let count_parameter = parameters.iter().find(|parameter| {
                native_stream(&parameter.id) == Some(scope)
                    && parameter.owner_record_index() == Some(direction.count_parameter)
            });
            let distance_parameter = parameters.iter().find(|parameter| {
                native_stream(&parameter.id) == Some(scope)
                    && parameter.owner_record_index() == Some(direction.distance_parameter)
            });
            let count = direction.evaluated_count.get();
            if count_parameter.is_some_and(|parameter| {
                !scalar_close(parameter.evaluated_value(), f64::from(count))
            }) {
                return None;
            }
            let distance = direction.evaluated_distance * 10.0;
            if !distance.is_finite()
                || distance < 0.0
                || (count == 1 && !scalar_close(distance, 0.0))
                || distance_parameter.is_some_and(|parameter| {
                    design_length(parameter)
                        .is_none_or(|value| !scalar_close(value.get(), distance))
                })
            {
                return None;
            }
            Some(RectangularPatternSourceDirection {
                direction: [direction.direction[0], direction.direction[1]],
                count,
                distance,
                distance_parameter: distance_parameter.map(neutral_parameter_id),
                count_parameter: count_parameter.map(neutral_parameter_id),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let source: [RectangularPatternSourceDirection; 2] = source.try_into().ok()?;
    if source
        .iter()
        .any(|direction| !scalar_close(direction.direction[0].hypot(direction.direction[1]), 1.0))
    {
        return None;
    }
    let dot = source[0].direction[0] * source[1].direction[0]
        + source[0].direction[1] * source[1].direction[1];
    if dot.abs() > EPS_CONSTRAINTS_EXACT_RECTANGULAR_PATTERN_E9 {
        return None;
    }
    let directions = rectangular_pattern_directions(&source, distance_form)?;
    let counts = source.each_ref().map(|direction| direction.count);
    let rows = exact_rectangular_pattern_instances(&directions, counts, entities)?;
    let pattern = cadmpeg_ir::sketches::SketchRectangularPattern::new(directions, rows)?;
    Some(Definition::RectangularPattern { pattern })
}

fn rectangular_pattern_directions(
    source: &[RectangularPatternSourceDirection; 2],
    distance_form: RectangularPatternDistanceForm,
) -> Option<[cadmpeg_ir::sketches::SketchPatternDirection; 2]> {
    source
        .iter()
        .map(|source| {
            let spacing = match distance_form {
                RectangularPatternDistanceForm::AdjacentSpacing => source.distance,
                RectangularPatternDistanceForm::SeedToFinalSpan => {
                    if source.count > 1 {
                        source.distance / f64::from(source.count - 1)
                    } else {
                        0.0
                    }
                }
            };
            if !spacing.is_finite() || spacing < 0.0 {
                return None;
            }
            let distance = source
                .distance_parameter
                .clone()
                .map(|parameter| match distance_form {
                    RectangularPatternDistanceForm::AdjacentSpacing => {
                        cadmpeg_ir::sketches::SketchPatternDistance::Spacing { parameter }
                    }
                    RectangularPatternDistanceForm::SeedToFinalSpan => {
                        cadmpeg_ir::sketches::SketchPatternDistance::Span { parameter }
                    }
                });
            cadmpeg_ir::sketches::SketchPatternDirection::new(
                source.direction,
                cadmpeg_ir::scalar::Length::new(spacing)?,
                distance,
                source.count_parameter.clone(),
            )
        })
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn exact_rectangular_pattern_instances(
    directions: &[cadmpeg_ir::sketches::SketchPatternDirection; 2],
    counts: [u32; 2],
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<Vec<Vec<cadmpeg_ir::sketches::SketchPatternInstance>>> {
    let instance_count = usize::try_from(counts[0])
        .ok()?
        .checked_mul(usize::try_from(counts[1]).ok()?)?;
    if instance_count == 0 || !entities.len().is_multiple_of(instance_count) {
        return None;
    }
    let entity_count = entities.len() / instance_count;
    let seed = entities.get(..entity_count)?;
    if seed.is_empty() {
        return None;
    }
    let column_count = usize::try_from(counts[1]).ok()?;
    let instances = entities
        .chunks_exact(entity_count)
        .enumerate()
        .map(|(position, instance)| {
            let candidates = (0..counts[0])
                .flat_map(|first| (0..counts[1]).map(move |second| [first, second]))
                .filter(|indices| {
                    let translation = Point2::new(
                        f64::from(indices[0])
                            * directions[0].spacing().get()
                            * directions[0].direction()[0]
                            + f64::from(indices[1])
                                * directions[1].spacing().get()
                                * directions[1].direction()[0],
                        f64::from(indices[0])
                            * directions[0].spacing().get()
                            * directions[0].direction()[1]
                            + f64::from(indices[1])
                                * directions[1].spacing().get()
                                * directions[1].direction()[1],
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
            let expected = [
                u32::try_from(position / column_count).ok()?,
                u32::try_from(position % column_count).ok()?,
            ];
            if candidates.as_slice() != [expected] {
                return None;
            }
            Some(cadmpeg_ir::sketches::SketchPatternInstance {
                entities: instance.iter().map(|entity| entity.id().clone()).collect(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let mut instances = instances.into_iter();
    Some(
        (0..counts[0])
            .map(|_| instances.by_ref().take(column_count).collect())
            .collect(),
    )
}

pub(crate) fn exact_text_relation(
    relation: &SketchRelation,
    scope: &str,
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use crate::records::SketchPatternDefinition;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };
    use cadmpeg_ir::transform::Transform;

    relation.sole_constraint_kind()?;
    let pattern = relation.definition.pattern();
    match pattern {
        Some(SketchPatternDefinition::TextFrame { text_reference })
            if relation
                .members()
                .first()
                .map(|member| member.reference.record_index())
                == Some(*text_reference)
                && relation
                    .auxiliary_references()
                    .values()
                    .copied()
                    .eq([*text_reference])
                && relation.return_member_indices() == relation.member_indices()[1..] =>
        {
            let text = projected.get(&(scope, *text_reference))?;
            if !matches!(
                *text.geometry.definition(),
                SketchGeometryDefinition::Text { .. }
            ) {
                return None;
            }
            let frame = relation
                .return_members()
                .iter()
                .map(|member| {
                    projected
                        .get(&(scope, member.reference.record_index()))
                        .copied()
                })
                .collect::<Option<Vec<_>>>()?;
            (!frame.is_empty()
                && frame.iter().all(|entity| {
                    entity.id() != text.id()
                        && !matches!(
                            *entity.geometry.definition(),
                            SketchGeometryDefinition::Text { .. }
                        )
                }))
            .then(|| Definition::TextFrame {
                text: text.id().clone(),
                frame: frame
                    .into_iter()
                    .map(|entity| entity.id().clone())
                    .collect(),
            })
        }
        Some(SketchPatternDefinition::TextPath {
            text_reference,
            glyph_transforms,
        }) if relation.members().len() == 2
            && relation.members()[1].reference.record_index() == *text_reference
            && relation
                .auxiliary_references()
                .values()
                .copied()
                .eq([*text_reference])
            && relation.return_member_indices()
                == [relation.members()[0].reference.record_index()] =>
        {
            let path = projected.get(&(scope, relation.members()[0].reference.record_index()))?;
            let text = projected.get(&(scope, *text_reference))?;
            if path.id() == text.id()
                || matches!(
                    *path.geometry.definition(),
                    SketchGeometryDefinition::Point { .. } | SketchGeometryDefinition::Text { .. }
                )
                || !matches!(
                    *text.geometry.definition(),
                    SketchGeometryDefinition::Text { .. }
                )
                || glyph_transforms.is_empty()
            {
                return None;
            }
            let glyph_transforms = glyph_transforms
                .iter()
                .map(|source| {
                    let source = source.rows();
                    if source[3] != [0.0, 0.0, 0.0, 1.0] {
                        return None;
                    }
                    let mut rows = [source[0], source[1], source[2]];
                    for row in &mut rows {
                        row[3] *= 10.0;
                    }
                    Transform::affine(rows)
                })
                .collect::<Option<Vec<_>>>()?;
            Some(Definition::TextPath {
                text: text.id().clone(),
                path: path.id().clone(),
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
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use crate::records::SketchPatternDefinition;
    use cadmpeg_ir::sketches::{
        SketchCircularPattern, SketchCircularPatternInstance,
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    if relation.sole_constraint_kind().is_none()
        || members.len() != relation.members().len()
        || returned.len() != relation.return_members().len()
    {
        return None;
    }
    let pattern = relation.definition.pattern();
    let Some(SketchPatternDefinition::Circular {
        angle_parameter,
        count_parameter,
        evaluated_angle,
        evaluated_count,
    }) = pattern
    else {
        return None;
    };
    let angle_parameter = parameters.iter().find(|parameter| {
        native_stream(&parameter.id) == Some(scope)
            && parameter.owner_record_index() == Some(*angle_parameter)
    });
    let count_parameter = parameters.iter().find(|parameter| {
        native_stream(&parameter.id) == Some(scope)
            && parameter.owner_record_index() == Some(*count_parameter)
    });
    let angle = cadmpeg_ir::scalar::Angle::new(*evaluated_angle)?;
    if !evaluated_angle.is_finite()
        || angle_parameter.is_some_and(|parameter| {
            design_angle(parameter).is_none_or(|value| !scalar_close(value.get(), angle.get()))
        })
        || count_parameter.is_some_and(|parameter| {
            !scalar_close(
                parameter.evaluated_value(),
                f64::from(evaluated_count.get()),
            )
        })
    {
        return None;
    }
    // Relation ordinals do not classify center/seed/generated; partition by
    // geometry when the member and returned id sets match.
    let member_ids = members
        .iter()
        .map(|entity| entity.id())
        .collect::<HashSet<_>>();
    let returned_ids = returned
        .iter()
        .map(|entity| entity.id())
        .collect::<HashSet<_>>();
    if member_ids.len() != members.len()
        || returned_ids.len() != returned.len()
        || member_ids != returned_ids
    {
        return None;
    }
    let mut candidates = Vec::new();
    for center in members.iter().copied() {
        let SketchGeometryDefinition::Point {
            position: center_position,
        } = *center.geometry.definition()
        else {
            continue;
        };
        let patterned = returned
            .iter()
            .copied()
            .filter(|entity| entity.id() != center.id())
            .collect::<Vec<_>>();
        let count = usize::try_from(evaluated_count.get()).ok()?;
        if patterned.is_empty() || !patterned.len().is_multiple_of(count) {
            continue;
        }
        let arity = patterned.len() / count;
        let seed = &patterned[..arity];
        if seed.is_empty() {
            continue;
        }
        let mut divisors = vec![f64::from(evaluated_count.get())];
        if evaluated_count.get() > 1 {
            divisors.push(f64::from(evaluated_count.get() - 1));
        }
        divisors.dedup_by(|left, right| scalar_close(*left, *right));
        for divisor in divisors {
            // The seed is the first chunk; it is not an instance, and the
            // instances carry only their nonzero rotations.
            let instances = patterned
                .chunks_exact(arity)
                .enumerate()
                .skip(1)
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
                        .then(|| {
                            Some(SketchCircularPatternInstance {
                                angle: cadmpeg_ir::scalar::NonZeroAngle::new(rotation)?,
                                entities: instance
                                    .iter()
                                    .map(|entity| entity.id().clone())
                                    .collect(),
                            })
                        })
                        .flatten()
                })
                .collect::<Option<Vec<_>>>();
            if let Some(instances) = instances {
                let seed_entities = seed
                    .iter()
                    .map(|entity| entity.id().clone())
                    .collect::<Vec<_>>();
                candidates.push((center.id().clone(), seed_entities, instances));
            }
        }
    }
    candidates.dedup();
    let [(center, seed_entities, instances)] = candidates.as_slice() else {
        return None;
    };
    let pattern = SketchCircularPattern::new(
        center.clone(),
        angle,
        angle_parameter.map(neutral_parameter_id),
        count_parameter.map(neutral_parameter_id),
        seed_entities.clone(),
        instances.clone(),
    )?;
    Some(Definition::CircularPattern { pattern })
}

fn rotated_sketch_geometry_matches(
    source: &cadmpeg_ir::sketches::SketchGeometry,
    result: &cadmpeg_ir::sketches::SketchGeometry,
    center: Point2,
    angle: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

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
    match (source.definition(), result.definition()) {
        (
            SketchGeometryDefinition::Point { position: first },
            SketchGeometryDefinition::Point { position: second },
        ) => point_matches(*first, *second),
        (
            SketchGeometryDefinition::Line { start: a, end: b },
            SketchGeometryDefinition::Line { start: c, end: d },
        ) => point_matches(*a, *c) && point_matches(*b, *d),
        (
            SketchGeometryDefinition::Circle {
                center: a,
                radius: ar,
            },
            SketchGeometryDefinition::Circle {
                center: b,
                radius: br,
            },
        ) => point_matches(*a, *b) && scalar_close(ar.get(), br.get()),
        (
            SketchGeometryDefinition::Arc {
                center: a,
                radius: ar,
                start_angle: as_,
                end_angle: ae,
            },
            SketchGeometryDefinition::Arc {
                center: b,
                radius: br,
                start_angle: bs,
                end_angle: be,
            },
        ) => {
            point_matches(*a, *b)
                && scalar_close(ar.get(), br.get())
                && angle_matches(as_.get(), bs.get())
                && angle_matches(ae.get(), be.get())
        }
        (
            SketchGeometryDefinition::Ellipse {
                center: a,
                major_angle: aa,
                major_radius: ar,
                minor_radius: ai,
                bounds: ab,
            },
            SketchGeometryDefinition::Ellipse {
                center: b,
                major_angle: ba,
                major_radius: br,
                minor_radius: bi,
                bounds: bb,
            },
        ) => {
            point_matches(*a, *b)
                && angle_matches(aa.get(), ba.get())
                && scalar_close(ar.get(), br.get())
                && scalar_close(ai.get(), bi.get())
                && optional_angle_bounds_match(ab.as_ref(), bb.as_ref())
        }
        (
            SketchGeometryDefinition::Nurbs { curve: first },
            SketchGeometryDefinition::Nurbs { curve: second },
        ) => {
            let first_points = first.control_points();
            let second_points = second.control_points();
            let first_weights = first.weights();
            let second_weights = second.weights();
            first.degree() == second.degree()
                && first.periodic() == second.periodic()
                && equal_scalars(first.knots(), second.knots())
                && first_points.len() == second_points.len()
                && first_points
                    .iter()
                    .zip(&second_points)
                    .all(|(a, b)| point_matches(*a, *b))
                && match (&first_weights, &second_weights) {
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
        && (first - second).abs()
            <= EPS_CONSTRAINTS_SCALAR_CLOSE_E9 * (1.0 + first.abs().max(second.abs()))
}

pub(crate) fn translated_sketch_geometry_matches(
    source: &cadmpeg_ir::sketches::SketchGeometry,
    result: &cadmpeg_ir::sketches::SketchGeometry,
    translation: Point2,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let point_matches = |first: Point2, second: Point2| {
        scalar_close(first.u + translation.u, second.u)
            && scalar_close(first.v + translation.v, second.v)
    };
    match (source.definition(), result.definition()) {
        (
            SketchGeometryDefinition::Point { position: first },
            SketchGeometryDefinition::Point { position: second },
        ) => point_matches(*first, *second),
        (
            SketchGeometryDefinition::Line {
                start: first_start,
                end: first_end,
            },
            SketchGeometryDefinition::Line {
                start: second_start,
                end: second_end,
            },
        ) => point_matches(*first_start, *second_start) && point_matches(*first_end, *second_end),
        (
            SketchGeometryDefinition::Circle {
                center: first_center,
                radius: first_radius,
            },
            SketchGeometryDefinition::Circle {
                center: second_center,
                radius: second_radius,
            },
        ) => {
            point_matches(*first_center, *second_center)
                && scalar_close(first_radius.get(), second_radius.get())
        }
        (
            SketchGeometryDefinition::Arc {
                center: first_center,
                radius: first_radius,
                start_angle: first_start,
                end_angle: first_end,
            },
            SketchGeometryDefinition::Arc {
                center: second_center,
                radius: second_radius,
                start_angle: second_start,
                end_angle: second_end,
            },
        ) => {
            point_matches(*first_center, *second_center)
                && scalar_close(first_radius.get(), second_radius.get())
                && scalar_close(first_start.get(), second_start.get())
                && scalar_close(first_end.get(), second_end.get())
        }
        (
            SketchGeometryDefinition::Ellipse {
                center: first_center,
                major_angle: first_major_angle,
                major_radius: first_major_radius,
                minor_radius: first_minor_radius,
                bounds: first_bounds,
            },
            SketchGeometryDefinition::Ellipse {
                center: second_center,
                major_angle: second_major_angle,
                major_radius: second_major_radius,
                minor_radius: second_minor_radius,
                bounds: second_bounds,
            },
        ) => {
            point_matches(*first_center, *second_center)
                && scalar_close(first_major_angle.get(), second_major_angle.get())
                && scalar_close(first_major_radius.get(), second_major_radius.get())
                && scalar_close(first_minor_radius.get(), second_minor_radius.get())
                && optional_angle_bounds_match(first_bounds.as_ref(), second_bounds.as_ref())
        }
        (
            SketchGeometryDefinition::Nurbs { curve: first },
            SketchGeometryDefinition::Nurbs { curve: second },
        ) => {
            let first_points = first.control_points();
            let second_points = second.control_points();
            let first_weights = first.weights();
            let second_weights = second.weights();
            first.degree() == second.degree()
                && first.periodic() == second.periodic()
                && equal_scalars(first.knots(), second.knots())
                && first_points.len() == second_points.len()
                && first_points
                    .iter()
                    .zip(&second_points)
                    .all(|(a, b)| point_matches(*a, *b))
                && match (&first_weights, &second_weights) {
                    (None, None) => true,
                    (Some(a), Some(b)) => equal_scalars(a, b),
                    _ => false,
                }
        }
        _ => false,
    }
}

fn optional_angle_bounds_match(
    first: Option<&[cadmpeg_ir::scalar::Angle; 2]>,
    second: Option<&[cadmpeg_ir::scalar::Angle; 2]>,
) -> bool {
    match (first, second) {
        (None, None) => true,
        (Some(first), Some(second)) => {
            scalar_close(first[0].get(), second[0].get())
                && scalar_close(first[1].get(), second[1].get())
        }
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

#[cfg(test)]
mod tests {
    use super::{
        exact_circular_pattern, exact_rectangular_pattern, exact_text_relation, scalar_close,
        translated_sketch_geometry_matches, RectangularPatternDistanceForm,
    };
    use crate::records::{
        DesignParameter, SketchPatternDefinition, SketchRelation, SketchRelationMember,
        SketchRelationReturnMember,
    };
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput, SketchEntityId, SketchGeometry, SketchGeometryDefinition,
        SketchId,
    };
    #[test]
    fn rectangular_pattern_instances_require_exact_translated_geometry() {
        let source = SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(1.0, 2.0),
            end: Point2::new(4.0, 6.0),
        })
        .unwrap();
        let translated = SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(11.0, -1.0),
            end: Point2::new(14.0, 3.0),
        })
        .unwrap();
        assert!(translated_sketch_geometry_matches(
            &source,
            &translated,
            Point2::new(10.0, -3.0),
        ));
        let reversed = SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(14.0, 3.0),
            end: Point2::new(11.0, -1.0),
        })
        .unwrap();
        assert!(!translated_sketch_geometry_matches(
            &source,
            &reversed,
            Point2::new(10.0, -3.0),
        ));
        let resized = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(12.0, 0.0),
            radius: cadmpeg_ir::scalar::Length::new(3.1).unwrap(),
        })
        .unwrap();
        assert!(!translated_sketch_geometry_matches(
            &SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(2.0, 3.0),
                radius: cadmpeg_ir::scalar::Length::new(3.0).unwrap(),
            })
            .unwrap(),
            &resized,
            Point2::new(10.0, -3.0),
        ));
    }

    fn point_entity(id: &str, u: f64) -> cadmpeg_ir::sketches::SketchEntity {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, 4.0),
            })
            .unwrap(),
        )
    }

    fn rectangular_point_relation(
        evaluated_count: u32,
        evaluated_distance: f64,
        distance_form: RectangularPatternDistanceForm,
    ) -> SketchRelation {
        let (rectangular_counted_reference_count, mut auxiliary_references) = match distance_form {
            RectangularPatternDistanceForm::AdjacentSpacing => (2, vec![100, 101]),
            RectangularPatternDistanceForm::SeedToFinalSpan => (0, Vec::new()),
        };
        auxiliary_references.extend([20, 21, 22, 23]);
        let members = (1..=evaluated_count).collect::<Vec<_>>();
        SketchRelation::try_new(crate::records::SketchRelationDraft {
            id: "f3d:native:sketch-relation#rectangular".into(),
            record_index: 10,
            class_tag: crate::records::DesignClassTag::try_from("300".to_owned()).unwrap(),
            byte_offset: 0,
            state_offset: 0,
            owner_reference: 1,
            owner_entity_id: Some(cadmpeg_ir::NonEmptyString::new("0_1").unwrap()),
            auxiliary_references: crate::records::ReferenceRun::located(
                auxiliary_references
                    .into_iter()
                    .map(|value| crate::records::Located { value, offset: 0 })
                    .collect(),
            ),
            rectangular_counted_reference_count: Some(rectangular_counted_reference_count),
            members: (members
                .clone()
                .into_iter()
                .map(SketchRelationMember::from_index)
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution"),
            owner_reference_offset: 0,
            definition: crate::records::SketchRelationDefinition::new(
                0x2000_0000,
                Some(crate::records::SketchPatternDefinition::Rectangular {
                    directions: [
                        crate::records::SketchPatternDirection {
                            count_parameter: 20,
                            distance_parameter: 21,
                            evaluated_count: crate::records::SketchPatternCount::try_from(
                                evaluated_count,
                            )
                            .unwrap(),
                            direction: [1.0, 0.0, 0.0],
                            evaluated_distance,
                        },
                        crate::records::SketchPatternDirection {
                            count_parameter: 22,
                            distance_parameter: 23,
                            evaluated_count: crate::records::SketchPatternCount::try_from(1)
                                .unwrap(),
                            direction: [0.0, 1.0, 0.0],
                            evaluated_distance: 0.0,
                        },
                    ],
                }),
            )
            .expect("valid relation definition"),
            entity_genesis: None,
            return_members: (members
                .iter()
                .copied()
                .map(SketchRelationReturnMember::from_index)
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution"),
            raw_bytes: vec![0; 160],
        })
        .unwrap()
    }

    fn rectangular_parameter(record_index: u32, value: f64) -> DesignParameter {
        crate::records::DesignParameter::try_from(crate::records::DesignParameterDraft {
            id: format!("native:design-parameter#{record_index}"),
            byte_offset: 0,
            class_tag: crate::records::DesignClassTag::try_from("373".to_owned()).unwrap(),
            record_index,
            source_ordinal: 0,
            source: crate::records::DesignParameterSource::new(
                "R-Pattern1-distance".into(),
                Some(record_index),
                Some(crate::records::Located {
                    value: crate::records::DesignParameterDiscriminator::Code6,
                    offset: 22,
                }),
            )
            .unwrap(),
            expression: value.to_string(),
            expression_offset: 40,
            source_kind_offset: 60,

            unit: Some(crate::records::RecordedValue {
                value: "mm".into(),
                offset: 70,
            }),
            name: format!("d{record_index}"),
            name_offset: 80,
            evaluated_value: value,
            evaluated_value_offset: 90,
        })
        .unwrap()
    }

    fn rectangular_parameters(count: u32, distance: f64) -> [DesignParameter; 4] {
        [
            rectangular_parameter(20, f64::from(count)),
            rectangular_parameter(21, distance),
            rectangular_parameter(22, 1.0),
            rectangular_parameter(23, 0.0),
        ]
    }

    #[test]
    fn rectangular_pattern_projects_adjacent_spacing_and_parameter() {
        let seed = point_entity("generated:test:point#seed", 2.0);
        let second = point_entity("generated:test:point#second", 17.0);
        let third = point_entity("generated:test:point#third", 32.0);
        let relation =
            rectangular_point_relation(3, 1.5, RectangularPatternDistanceForm::AdjacentSpacing);
        let parameters = rectangular_parameters(3, 1.5);
        let Some(SketchConstraintDefinitionInput::RectangularPattern { pattern }) =
            exact_rectangular_pattern(&relation, "native", &parameters, &[&seed, &second, &third])
        else {
            panic!("rectangular pattern did not resolve");
        };
        let directions = pattern.directions();
        assert_eq!(directions[0].spacing().get(), 15.0);
        assert_eq!(directions[1].spacing().get(), 0.0);
        assert!(matches!(
            directions[0].distance,
            Some(cadmpeg_ir::sketches::SketchPatternDistance::Spacing { .. })
        ));
        assert!(directions[0].count_parameter.is_some());
        assert_eq!(pattern.counts(), [3, 1]);
    }

    #[test]
    fn rectangular_pattern_projects_total_span_and_keeps_span_parameter() {
        let seed = point_entity("generated:test:point#seed", 2.0);
        let second = point_entity("generated:test:point#second", 17.0);
        let third = point_entity("generated:test:point#third", 32.0);
        let relation =
            rectangular_point_relation(3, 3.0, RectangularPatternDistanceForm::SeedToFinalSpan);
        let parameters = rectangular_parameters(3, 3.0);
        let Some(SketchConstraintDefinitionInput::RectangularPattern { pattern }) =
            exact_rectangular_pattern(&relation, "native", &parameters, &[&seed, &second, &third])
        else {
            panic!("total-span rectangular pattern did not resolve");
        };
        let directions = pattern.directions();
        assert_eq!(directions[0].spacing().get(), 15.0);
        assert!(matches!(
            directions[0].distance,
            Some(cadmpeg_ir::sketches::SketchPatternDistance::Span { .. })
        ));
        assert!(directions[0].count_parameter.is_some());
    }

    #[test]
    fn rectangular_pattern_does_not_change_distance_form_to_match_geometry() {
        let seed = point_entity("generated:test:point#seed", 2.0);
        let second = point_entity("generated:test:point#second", 17.0);
        let third = point_entity("generated:test:point#third", 32.0);
        for (distance_form, distance) in [
            (RectangularPatternDistanceForm::AdjacentSpacing, 3.0),
            (RectangularPatternDistanceForm::SeedToFinalSpan, 1.5),
        ] {
            let relation = rectangular_point_relation(3, distance, distance_form);
            let parameters = rectangular_parameters(3, distance);
            assert_eq!(
                exact_rectangular_pattern(
                    &relation,
                    "native",
                    &parameters,
                    &[&seed, &second, &third],
                ),
                None
            );
        }
    }

    #[test]
    fn rectangular_pattern_requires_the_retained_counted_reference_count() {
        let seed = point_entity("generated:test:point#seed", 2.0);
        let second = point_entity("generated:test:point#second", 17.0);
        let mut relation =
            rectangular_point_relation(2, 1.5, RectangularPatternDistanceForm::AdjacentSpacing);
        relation.rectangular_counted_reference_count = None;
        let parameters = rectangular_parameters(2, 1.5);

        assert_eq!(
            exact_rectangular_pattern(&relation, "native", &parameters, &[&seed, &second]),
            None
        );
    }

    #[test]
    fn rectangular_pattern_transfers_two_instances_in_both_distance_forms() {
        let seed = point_entity("generated:test:point#seed", 2.0);
        let second = point_entity("generated:test:point#second", 17.0);
        for distance_form in [
            RectangularPatternDistanceForm::AdjacentSpacing,
            RectangularPatternDistanceForm::SeedToFinalSpan,
        ] {
            let relation = rectangular_point_relation(2, 1.5, distance_form);
            let parameters = rectangular_parameters(2, 1.5);
            let Some(SketchConstraintDefinitionInput::RectangularPattern { pattern }) =
                exact_rectangular_pattern(&relation, "native", &parameters, &[&seed, &second])
            else {
                panic!("two-instance rectangular pattern did not resolve");
            };
            let directions = pattern.directions();
            assert_eq!(directions[0].spacing().get(), 15.0);
            match distance_form {
                RectangularPatternDistanceForm::AdjacentSpacing => {
                    assert!(matches!(
                        directions[0].distance,
                        Some(cadmpeg_ir::sketches::SketchPatternDistance::Spacing { .. })
                    ));
                }
                RectangularPatternDistanceForm::SeedToFinalSpan => {
                    assert!(matches!(
                        directions[0].distance,
                        Some(cadmpeg_ir::sketches::SketchPatternDistance::Span { .. })
                    ));
                }
            }
        }
    }

    #[test]
    fn circular_pattern_resolves_full_and_partial_instance_distributions() {
        let entity = |id: &str, geometry| {
            cadmpeg_ir::sketches::SketchEntity::new(
                SketchEntityId::mint(id).unwrap(),
                SketchId::mint("generated:test:sketch#0").unwrap(),
                geometry,
            )
        };
        let center = entity(
            "generated:test:point#center",
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(2.0, -3.0),
            })
            .unwrap(),
        );
        let circle = |id: &str, angle: f64| {
            entity(
                id,
                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                    center: Point2::new(2.0 + 5.0 * angle.cos(), -3.0 + 5.0 * angle.sin()),
                    radius: cadmpeg_ir::scalar::Length::new(0.75).unwrap(),
                })
                .unwrap(),
            )
        };
        let seed = circle("generated:test:circle#seed", 0.0);
        let middle = circle("generated:test:circle#middle", std::f64::consts::FRAC_PI_2);
        let last = circle("generated:test:circle#last", std::f64::consts::PI);
        let relation = |angle| {
            SketchRelation::try_new(crate::records::SketchRelationDraft {
                id: "f3d:native:sketch-relation#circular".into(),
                record_index: 10,
                class_tag: crate::records::DesignClassTag::try_from("300".to_owned()).unwrap(),
                byte_offset: 0,
                state_offset: 0,
                owner_reference: 1,
                owner_entity_id: Some(cadmpeg_ir::NonEmptyString::new("0_1").unwrap()),
                auxiliary_references: crate::records::ReferenceRun::located(
                    vec![20, 21]
                        .into_iter()
                        .map(|value| crate::records::Located { value, offset: 0 })
                        .collect(),
                ),
                rectangular_counted_reference_count: None,
                members: (vec![
                    SketchRelationMember::from_index(1),
                    SketchRelationMember::from_index(2),
                    SketchRelationMember::from_index(3),
                    SketchRelationMember::from_index(4),
                ])
                .try_into()
                .expect("uniform member resolution"),
                owner_reference_offset: 0,
                definition: crate::records::SketchRelationDefinition::new(
                    0x1000_0000,
                    Some(crate::records::SketchPatternDefinition::Circular {
                        angle_parameter: 20,
                        count_parameter: 21,
                        evaluated_angle: angle,
                        evaluated_count: crate::records::SketchPatternCount::try_from(3).unwrap(),
                    }),
                )
                .expect("valid relation definition"),
                entity_genesis: None,
                return_members: (vec![
                    SketchRelationReturnMember::from_index(2),
                    SketchRelationReturnMember::from_index(3),
                    SketchRelationReturnMember::from_index(4),
                    SketchRelationReturnMember::from_index(1),
                ])
                .try_into()
                .expect("uniform member resolution"),
                raw_bytes: vec![0; 160],
            })
            .unwrap()
        };
        let members = [&center, &seed, &middle, &last];
        let returned = [&seed, &middle, &last, &center];
        let Some(SketchConstraintDefinitionInput::CircularPattern { pattern }) =
            exact_circular_pattern(
                &relation(std::f64::consts::PI),
                "native",
                &[],
                &members,
                &returned,
            )
        else {
            panic!("partial circular pattern did not resolve");
        };
        assert_eq!(pattern.center(), center.id());
        assert_eq!(pattern.angle().get(), std::f64::consts::PI);
        assert_eq!(pattern.count(), 3);
        // The seed is not an instance, so the instance list carries only the
        // rotations after it.
        assert_eq!(pattern.seed(), std::slice::from_ref(seed.id()));
        assert_eq!(
            pattern
                .instances()
                .iter()
                .map(|instance| instance.angle.get())
                .collect::<Vec<_>>(),
            [std::f64::consts::FRAC_PI_2, std::f64::consts::PI]
        );

        let full_middle = circle(
            "generated:test:circle#full-middle",
            std::f64::consts::TAU / 3.0,
        );
        let full_last = circle(
            "generated:test:circle#full-last",
            2.0 * std::f64::consts::TAU / 3.0,
        );
        let full_members = [&center, &seed, &full_middle, &full_last];
        let full_returned = [&seed, &full_middle, &full_last, &center];
        assert!(matches!(
            exact_circular_pattern(
                &relation(std::f64::consts::TAU),
                "native",
                &[],
                &full_members,
                &full_returned,
            ),
            Some(SketchConstraintDefinitionInput::CircularPattern { ref pattern })
                if scalar_close(pattern.instances()[0].angle.get(), std::f64::consts::TAU / 3.0)
        ));
    }

    #[test]
    fn circular_pattern_resolves_independently_of_relation_ordinals() {
        let entity = |id: &str, geometry| {
            cadmpeg_ir::sketches::SketchEntity::new(
                SketchEntityId::mint(id).unwrap(),
                SketchId::mint("generated:test:sketch#0").unwrap(),
                geometry,
            )
        };
        let center = entity(
            "generated:test:point#center",
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(2.0, -3.0),
            })
            .unwrap(),
        );
        let circle = |id: &str, angle: f64| {
            entity(
                id,
                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                    center: Point2::new(2.0 + 5.0 * angle.cos(), -3.0 + 5.0 * angle.sin()),
                    radius: cadmpeg_ir::scalar::Length::new(0.75).unwrap(),
                })
                .unwrap(),
            )
        };
        let seed = circle("generated:test:circle#seed", 0.0);
        let middle = circle("generated:test:circle#middle", std::f64::consts::TAU / 3.0);
        let last = circle(
            "generated:test:circle#last",
            2.0 * std::f64::consts::TAU / 3.0,
        );
        // Ordinals are all zero; geometry must still partition the members.
        let relation = SketchRelation::try_new(crate::records::SketchRelationDraft {
            id: "f3d:native:sketch-relation#circular".into(),
            record_index: 10,
            class_tag: crate::records::DesignClassTag::try_from("300".to_owned()).unwrap(),
            byte_offset: 0,
            state_offset: 0,
            owner_reference: 1,
            owner_entity_id: Some(cadmpeg_ir::NonEmptyString::new("0_1").unwrap()),
            auxiliary_references: crate::records::ReferenceRun::located(
                vec![20, 21]
                    .into_iter()
                    .map(|value| crate::records::Located { value, offset: 0 })
                    .collect(),
            ),
            rectangular_counted_reference_count: None,
            members: (vec![
                SketchRelationMember::from_index(1),
                SketchRelationMember::from_index(2),
                SketchRelationMember::from_index(3),
                SketchRelationMember::from_index(4),
            ])
            .try_into()
            .expect("uniform member resolution"),
            owner_reference_offset: 0,
            definition: crate::records::SketchRelationDefinition::new(
                0x1000_0000,
                Some(crate::records::SketchPatternDefinition::Circular {
                    angle_parameter: 20,
                    count_parameter: 21,
                    evaluated_angle: std::f64::consts::TAU,
                    evaluated_count: crate::records::SketchPatternCount::try_from(3).unwrap(),
                }),
            )
            .expect("valid relation definition"),
            entity_genesis: None,
            return_members: (vec![
                SketchRelationReturnMember::from_index(2),
                SketchRelationReturnMember::from_index(3),
                SketchRelationReturnMember::from_index(4),
                SketchRelationReturnMember::from_index(1),
            ])
            .try_into()
            .expect("uniform member resolution"),
            raw_bytes: vec![0; 160],
        })
        .unwrap();
        let members = [&center, &seed, &middle, &last];
        let returned = [&seed, &middle, &last, &center];
        let Some(SketchConstraintDefinitionInput::CircularPattern { pattern }) =
            exact_circular_pattern(&relation, "native", &[], &members, &returned)
        else {
            panic!("role-agnostic circular pattern did not resolve");
        };
        assert_eq!(pattern.center(), center.id());
        assert_eq!(pattern.count(), 3);
    }

    #[test]
    fn text_path_relation_projects_typed_entities_and_scaled_glyph_placements() {
        use cadmpeg_ir::math::Point2;
        use cadmpeg_ir::scalar::Length;
        use cadmpeg_ir::sketches::{
            SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
            SketchGeometryDefinition, SketchId,
        };

        let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
        let path = SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#path").unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(10.0, 0.0),
            })
            .unwrap(),
        );
        let text = SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#text").unwrap(),
            sketch,
            SketchGeometry::try_from(SketchGeometryDefinition::Text {
                text: cadmpeg_ir::products::NonEmptyString::new("A").unwrap(),
                font_family: cadmpeg_ir::products::NonEmptyString::new("Arial").unwrap(),
                font_weight: cadmpeg_ir::sketches::SketchFontWeight::Regular,
                height: Length::new(10.0).unwrap(),
                width_factor: Some(0.8),
                placement: None,
                horizontal_alignment: None,
                vertical_alignment: None,
            })
            .unwrap(),
        );
        let mut glyph = [[0.0; 4]; 4];
        for ordinal in 0..4 {
            glyph[ordinal][ordinal] = 1.0;
        }
        glyph[0][3] = 0.5;
        let relation = SketchRelation::try_new(crate::records::SketchRelationDraft {
            id: "f3d:Design/BulkStream.dat:sketch-relation#3".into(),
            record_index: 3,
            class_tag: crate::records::DesignClassTag::try_from("413".to_owned()).unwrap(),
            byte_offset: 0,
            state_offset: 0,
            owner_reference: 1,
            owner_entity_id: None,
            auxiliary_references: crate::records::ReferenceRun::located(
                vec![2]
                    .into_iter()
                    .map(|value| crate::records::Located { value, offset: 0 })
                    .collect(),
            ),
            rectangular_counted_reference_count: None,
            members: (vec![
                SketchRelationMember::from_index(1),
                SketchRelationMember::from_index(2),
            ])
            .try_into()
            .expect("uniform member resolution"),
            owner_reference_offset: 0,
            definition: crate::records::SketchRelationDefinition::new(
                0x200_0000_0000,
                Some(crate::records::SketchPatternDefinition::TextPath {
                    text_reference: 2,
                    glyph_transforms: vec![crate::records::SketchGlyphTransform::try_from(glyph)
                        .expect("finite native glyph")],
                }),
            )
            .expect("valid relation definition"),
            entity_genesis: Some(2),
            return_members: (vec![SketchRelationReturnMember::from_index(1)])
                .try_into()
                .expect("uniform member resolution"),
            raw_bytes: vec![0; 160],
        })
        .unwrap();
        let projected =
            std::collections::HashMap::from([(("scope", 1), &path), (("scope", 2), &text)]);
        let definition =
            exact_text_relation(&relation, "scope", &projected).expect("typed text path");
        assert!(matches!(
            definition,
            SketchConstraintDefinitionInput::TextPath {
                text: ref text_id,
                path: ref path_id,
                ref glyph_transforms,
            } if text_id == text.id()
                && path_id == path.id()
                && glyph_transforms[0].rows()[0][3] == 5.0
        ));
        let mut relation = relation;
        let mut overflow = glyph;
        overflow[0][3] = f64::MAX;
        let mut non_affine = glyph;
        non_affine[3][3] = 2.0;
        for rows in [overflow, non_affine] {
            relation.definition = crate::records::SketchRelationDefinition::new(
                0x200_0000_0000,
                Some(SketchPatternDefinition::TextPath {
                    text_reference: 2,
                    glyph_transforms: vec![
                        crate::records::SketchGlyphTransform::try_from(glyph).unwrap(),
                        crate::records::SketchGlyphTransform::try_from(rows).unwrap(),
                    ],
                }),
            )
            .unwrap();
            assert!(exact_text_relation(&relation, "scope", &projected).is_none());
        }
    }
}
