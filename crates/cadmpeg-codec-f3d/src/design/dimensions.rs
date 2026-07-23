// SPDX-License-Identifier: Apache-2.0
//! Project dimension constraint relations.

use crate::design::constraints::scalar_close;
use crate::design::feature_project::{design_dimension_unit, design_length};
use crate::design::geometry::{angle_in_sweep, sketch_entity_endpoints};
use crate::ids::{
    native_stream, neutral_dimension_constraint_id, neutral_parameter_id,
    neutral_sketch_constraint_id, neutral_sketch_id, neutral_spatial_sketch_id,
};
use crate::records::{
    DesignDimensionAnnotationFrame, DesignDimensionLocusGroup, DesignDimensionLocusPair,
    DesignDimensionNullLocusPair, DesignDimensionRecipeRecord, DesignParameter,
    DesignParameterCompanion, DesignParameterKind, DesignParameterOwner, DesignSketchPlacement,
    SketchConstraintKind, SketchCurveIdentity, SketchPoint, SketchRelation, SketchRelationOperand,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Record slices shared by every dimension-constraint projection: the sketch
/// placements, parameter and companion tables, the locus/group/annotation
/// dimension records, and the sketch geometry the loci reference.
pub struct DimensionConstraintInputs<'a> {
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) parameters: &'a [DesignParameter],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) pairs: &'a [DesignDimensionLocusPair],
    pub(crate) groups: &'a [DesignDimensionLocusGroup],
    pub(crate) annotation_frames: &'a [DesignDimensionAnnotationFrame],
    pub(crate) null_pairs: &'a [DesignDimensionNullLocusPair],
    pub(crate) companions: &'a [DesignParameterCompanion],
    pub(crate) recipe_records: &'a [DesignDimensionRecipeRecord],
    pub(crate) points: &'a [SketchPoint],
    pub(crate) curves: &'a [SketchCurveIdentity],
    pub(crate) entities: &'a [cadmpeg_ir::sketches::SketchEntity],
}

/// Project dimensional parameter companions into parameter-backed sketch
/// constraints. Two-locus dimensions have neutral semantics; aggregate and
/// role-dependent forms remain explicit native constraints.
pub fn project_dimension_constraints(
    inputs: &DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
) -> Vec<cadmpeg_ir::sketches::SketchConstraint> {
    let spatial_sketch_ids = spatial_sketches
        .iter()
        .map(|sketch| sketch.id.clone())
        .collect::<HashSet<_>>();
    let placements = inputs.placements;
    project_all_dimension_constraints(inputs)
        .into_iter()
        .filter(|constraint| {
            placements
                .iter()
                .find(|placement| neutral_sketch_id(placement) == constraint.sketch)
                .is_none_or(|placement| {
                    !spatial_sketch_ids.contains(&neutral_spatial_sketch_id(placement))
                })
        })
        .collect()
}

fn project_all_dimension_constraints(
    inputs: &DimensionConstraintInputs<'_>,
) -> Vec<cadmpeg_ir::sketches::SketchConstraint> {
    use cadmpeg_ir::sketches::{
        SketchConstraint, SketchConstraintDefinition as Definition, SketchGeometry,
        SketchNativeOperand,
    };

    let &DimensionConstraintInputs {
        placements,
        parameters,
        owners,
        pairs,
        groups,
        annotation_frames,
        null_pairs,
        companions,
        recipe_records,
        points,
        curves,
        entities,
    } = inputs;

    let sketches = placements
        .iter()
        .filter_map(|placement| {
            let scope = native_stream(&placement.id)?;
            u32::try_from(placement.entity_suffix)
                .ok()
                .map(|suffix| ((scope, suffix), neutral_sketch_id(placement)))
        })
        .collect::<HashMap<_, _>>();
    let sketches_by_scope = placements
        .iter()
        .filter_map(|placement| {
            Some((
                (native_stream(&placement.id)?, placement.scope_record_index?),
                neutral_sketch_id(placement),
            ))
        })
        .collect::<HashMap<_, _>>();
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (native_stream(&parameter.id)?, parameter.record_index),
                parameter,
            ))
        })
        .collect::<HashMap<_, _>>();
    let parameter_by_companion = owners
        .iter()
        .filter_map(|owner| {
            Some((
                (native_stream(&owner.id)?, owner.companion_record_index),
                owner.parameter_record_index,
            ))
        })
        .collect::<HashMap<_, _>>();
    let native_geometry = points
        .iter()
        .filter_map(|point| {
            Some((
                (native_stream(&point.id)?, point.record_index),
                ("point", point.owner_reference, point.id.as_str()),
            ))
        })
        .chain(curves.iter().filter_map(|curve| {
            Some((
                (native_stream(&curve.id)?, curve.record_index),
                ("curve", curve.owner_reference, curve.id.as_str()),
            ))
        }))
        .collect::<HashMap<_, _>>();
    let record_indices_by_native_ref = native_geometry
        .iter()
        .map(|(key, (_, _, native_ref))| (*native_ref, *key))
        .collect::<HashMap<_, _>>();
    let projected = entities
        .iter()
        .filter_map(|entity| {
            let native_ref = entity.native_ref.as_deref()?;
            record_indices_by_native_ref
                .get(native_ref)
                .map(|key| (*key, entity))
        })
        .collect::<HashMap<_, _>>();

    let parameter_for = |scope: &str, companion_record_index: u32| {
        let record_index = *parameter_by_companion.get(&(scope, companion_record_index))?;
        let parameter = *parameters.get(&(scope, record_index))?;
        Some((parameter, neutral_parameter_id(parameter)))
    };
    let sketch_for_geometry = |scope: &str, indices: &[u32]| {
        let mut owners = indices
            .iter()
            .filter_map(|record_index| native_geometry.get(&(scope, *record_index))?.1)
            .collect::<HashSet<_>>();
        (owners.len() == 1)
            .then(|| owners.drain().next())
            .flatten()
            .and_then(|owner| sketches.get(&(scope, owner)).cloned())
    };
    let native_operand = |scope: &str, field: &str, role: Option<u32>, record_index: u32| {
        let (native_kind, _, native_ref) = native_geometry
            .get(&(scope, record_index))
            .copied()
            .unwrap_or(("record", None, ""));
        SketchNativeOperand {
            native_kind: native_kind.into(),
            native_field: Some(field.into()),
            native_role: role,
            object_index: record_index,
            native_ref: (!native_ref.is_empty() && !projected.contains_key(&(scope, record_index)))
                .then(|| native_ref.to_owned()),
        }
    };
    let native_definition = |scope: &str,
                             source_kind: &str,
                             state: Option<u64>,
                             operands: &[(&str, Option<u32>, u32)],
                             parameter| Definition::Native {
        native_kind: source_kind.to_owned(),
        native_state: state,
        native_flags: None,
        entities: operands
            .iter()
            .filter_map(|(_, _, record_index)| {
                projected
                    .get(&(scope, *record_index))
                    .map(|entity| entity.id.clone())
            })
            .collect(),
        parameter: Some(parameter),
        operands: operands
            .iter()
            .map(|(field, role, record_index)| native_operand(scope, field, *role, *record_index))
            .collect(),
    };
    let exact_definition = |scope: &str,
                            source_parameter: &DesignParameter,
                            indices: &[u32],
                            parameter: cadmpeg_ir::features::ParameterId|
     -> Option<Definition> {
        if !design_dimension_unit(source_parameter) {
            return None;
        }
        let source_kind = source_parameter.source_kind.as_str();
        let evaluated_value = source_parameter.evaluated_value;
        let entities = indices
            .iter()
            .map(|record_index| projected.get(&(scope, *record_index)).copied())
            .collect::<Option<Vec<_>>>()?;
        if let [entity] = entities.as_slice() {
            if let Some(definition) =
                radial_dimension_definition(entity, source_kind, evaluated_value, parameter.clone())
            {
                return Some(definition);
            }
        }
        if let [first, second] = entities.as_slice() {
            if first.id == second.id {
                return None;
            }
        }
        if source_kind.starts_with("Linear Dimension") && entities.len() == 2 {
            let evaluated_mm = evaluated_value * 10.0;
            if let Some(definition) =
                directional_point_dimension(&entities, evaluated_mm, parameter.clone())
            {
                return Some(definition);
            }
            if point_line_separation(entities[0], entities[1], evaluated_mm)
                || parallel_line_separation(entities[0], entities[1], evaluated_mm)
                || concentric_circle_separation(entities[0], entities[1], evaluated_mm)
            {
                return Some(Definition::Distance {
                    entities: entities.iter().map(|entity| entity.id.clone()).collect(),
                    parameter,
                });
            }
            let (
                SketchGeometry::Point {
                    position: first_position,
                },
                SketchGeometry::Point {
                    position: second_position,
                },
            ) = (&entities[0].geometry, &entities[1].geometry)
            else {
                return None;
            };
            let measured =
                (first_position.u - second_position.u).hypot(first_position.v - second_position.v);
            let scale = 1.0 + measured.abs().max(evaluated_mm.abs());
            if evaluated_mm.is_finite() && (measured - evaluated_mm.abs()).abs() <= 1.0e-9 * scale {
                return Some(Definition::DistanceLoci {
                    first: cadmpeg_ir::sketches::SketchLocus::Entity(entities[0].id.clone()),
                    second: cadmpeg_ir::sketches::SketchLocus::Entity(entities[1].id.clone()),
                    parameter,
                });
            }
            return None;
        }
        if source_kind.starts_with("Angular Dimension")
            && entities.len() == 2
            && entities
                .iter()
                .all(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
            && line_angle_matches(
                &entities[0].geometry,
                &entities[1].geometry,
                evaluated_value,
            )
        {
            return Some(Definition::Angle {
                first: entities[0].id.clone(),
                second: entities[1].id.clone(),
                parameter,
            });
        }
        if source_kind.starts_with("Angular Dimension") && entities.len() == 2 {
            let (first, second) =
                indirect_angular_lines(scope, &entities, evaluated_value, &projected)?;
            return Some(Definition::Angle {
                first,
                second,
                parameter,
            });
        }
        None
    };
    let exact_group_definition = |scope: &str,
                                  group: &DesignDimensionLocusGroup,
                                  parameter: &DesignParameter,
                                  parameter_id: cadmpeg_ir::features::ParameterId|
     -> Option<Definition> {
        if !design_dimension_unit(parameter) {
            return None;
        }
        let entities = group
            .loci
            .iter()
            .map(|locus| {
                projected
                    .get(&(scope, locus.geometry_record_index))
                    .copied()
            })
            .collect::<Option<Vec<_>>>()?;
        if let [entity] = entities.as_slice() {
            if let Some(definition) = radial_dimension_definition(
                entity,
                &parameter.source_kind,
                parameter.evaluated_value,
                parameter_id.clone(),
            ) {
                return Some(definition);
            }
        }
        if parameter.source_kind.starts_with("Angular Dimension") {
            let indices = group
                .loci
                .iter()
                .map(|locus| locus.geometry_record_index)
                .collect::<Vec<_>>();
            return exact_definition(scope, parameter, &indices, parameter_id);
        }
        if parameter.source_kind.starts_with("Linear Dimension") {
            if group.state == 0x20 && group.unknown_constraint_bits == 0 {
                let loci = group
                    .loci
                    .iter()
                    .map(|locus| (locus.geometry_record_index, locus.role))
                    .collect::<Vec<_>>();
                let entities_by_record = group
                    .loci
                    .iter()
                    .zip(&entities)
                    .map(|(locus, entity)| (locus.geometry_record_index, *entity))
                    .collect::<HashMap<_, _>>();
                let mut definition =
                    exact_counted_offset(&loci, &group.return_members, &entities_by_record)?;
                let Definition::Offset {
                    distance,
                    parameter: driving_parameter,
                    parameter_factor,
                    ..
                } = &mut definition
                else {
                    unreachable!("exact_counted_offset always returns an offset")
                };
                if let Some(factor) =
                    offset_parameter_factor(distance.0, parameter.evaluated_value * 10.0)
                {
                    *driving_parameter = Some(parameter_id);
                    *parameter_factor = Some(factor);
                }
                return Some(definition);
            }
            if let Some(definition) = directional_point_dimension(
                &entities,
                parameter.evaluated_value * 10.0,
                parameter_id.clone(),
            ) {
                return Some(definition);
            }
            if group.state == 0 && group.unknown_constraint_bits == 0 {
                if let Some(definition) = exact_counted_dimension_relation(&entities) {
                    return Some(definition);
                }
                return two_locus_distance_dimension(&entities, parameter_id);
            }
        }
        None
    };
    let exact_pair_companions = pairs
        .iter()
        .filter_map(|pair| {
            let scope = native_stream(&pair.id)?;
            let (parameter, parameter_id) =
                parameter_for(scope, pair.governing_companion_record_index)?;
            let indices = [
                pair.first_geometry_record_index,
                pair.second_geometry_record_index,
            ];
            exact_definition(scope, parameter, &indices, parameter_id)
                .map(|_| (scope.to_owned(), pair.companion_record_index))
        })
        .collect::<HashSet<_>>();
    let parameterized_offset_companions = groups
        .iter()
        .filter_map(|group| {
            let scope = native_stream(&group.id)?;
            let (parameter, parameter_id) = parameter_for(scope, group.companion_record_index)?;
            matches!(
                exact_group_definition(scope, group, parameter, parameter_id),
                Some(Definition::Offset {
                    parameter: Some(_),
                    ..
                })
            )
            .then(|| (scope.to_owned(), group.companion_record_index))
        })
        .collect::<HashSet<_>>();
    let locus_companions = pairs
        .iter()
        .filter_map(|pair| {
            Some((
                native_stream(&pair.id)?.to_owned(),
                pair.companion_record_index,
            ))
        })
        .chain(groups.iter().filter_map(|group| {
            Some((
                native_stream(&group.id)?.to_owned(),
                group.companion_record_index,
            ))
        }))
        .chain(annotation_frames.iter().filter_map(|frame| {
            Some((
                native_stream(&frame.id)?.to_owned(),
                frame.companion_record_index?,
            ))
        }))
        .chain(null_pairs.iter().filter_map(|pair| {
            Some((
                native_stream(&pair.id)?.to_owned(),
                pair.companion_record_index,
            ))
        }))
        .collect::<HashSet<_>>();

    let mut constraints = pairs
        .iter()
        .filter_map(|pair| {
            let scope = native_stream(&pair.id)?;
            let (parameter, parameter_id) =
                parameter_for(scope, pair.governing_companion_record_index)?;
            let indices = [
                pair.first_geometry_record_index,
                pair.second_geometry_record_index,
            ];
            let sketch = sketch_for_geometry(scope, &indices)?;
            let constraint_id = neutral_dimension_constraint_id(&parameter_id, "pair");
            let definition = exact_definition(scope, parameter, &indices, parameter_id.clone())
                .unwrap_or_else(|| {
                    native_definition(
                        scope,
                        &parameter.source_kind,
                        None,
                        &[
                            ("first_locus", Some(pair.first_role), indices[0]),
                            ("second_locus", Some(pair.second_role), indices[1]),
                        ],
                        parameter_id,
                    )
                });
            Some(SketchConstraint {
                id: constraint_id,
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
                native_ref: Some(pair.id.clone()),
            })
        })
        .chain(groups.iter().filter_map(|group| {
            let scope = native_stream(&group.id)?;
            if exact_pair_companions.contains(&(scope.to_owned(), group.companion_record_index)) {
                return None;
            }
            let (parameter, parameter_id) = parameter_for(scope, group.companion_record_index)?;
            let sketch = sketches.get(&(scope, group.owner_reference))?.clone();
            let definition = exact_group_definition(scope, group, parameter, parameter_id.clone())
                .unwrap_or_else(|| {
                    let mut operands = group
                        .loci
                        .iter()
                        .map(|locus| ("locus", Some(locus.role), locus.geometry_record_index))
                        .collect::<Vec<_>>();
                    operands.push(("owner", Some(group.owner_role), group.owner_reference));
                    operands.extend(
                        group
                            .return_members
                            .iter()
                            .map(|record_index| ("return", None, *record_index)),
                    );
                    native_definition(
                        scope,
                        &parameter.source_kind,
                        Some(u64::from(group.state)),
                        &operands,
                        parameter_id,
                    )
                });
            Some(SketchConstraint {
                id: neutral_sketch_constraint_id(&group.id, group.record_index),
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
                native_ref: Some(group.id.clone()),
            })
        }))
        .chain(annotation_frames.iter().filter_map(|frame| {
            let scope = native_stream(&frame.id)?;
            let (parameter, parameter_id) =
                parameter_for(scope, frame.governing_companion_record_index)?;
            let indices = frame
                .operands
                .iter()
                .filter_map(|operand| {
                    (operand.geometry_record_index != 0).then_some(operand.geometry_record_index)
                })
                .collect::<Vec<_>>();
            let sketch = sketches.get(&(scope, frame.owner_reference))?.clone();
            let constraint_id = neutral_dimension_constraint_id(&parameter_id, "annotation");
            let definition = exact_definition(scope, parameter, &indices, parameter_id.clone())
                .unwrap_or_else(|| {
                    let operands = frame
                        .operands
                        .iter()
                        .map(|operand| {
                            if operand.geometry_record_index == 0 {
                                SketchNativeOperand {
                                    native_kind: "null_locus".into(),
                                    native_field: Some("locus".into()),
                                    native_role: Some(operand.role),
                                    object_index: 0,
                                    native_ref: None,
                                }
                            } else {
                                native_operand(
                                    scope,
                                    "locus",
                                    Some(operand.role),
                                    operand.geometry_record_index,
                                )
                            }
                        })
                        .collect();
                    Definition::Native {
                        native_kind: parameter.source_kind.clone(),
                        native_state: None,
                        native_flags: None,
                        entities: indices
                            .iter()
                            .filter_map(|record_index| {
                                projected
                                    .get(&(scope, *record_index))
                                    .map(|entity| entity.id.clone())
                            })
                            .collect(),
                        parameter: Some(parameter_id),
                        operands,
                    }
                });
            Some(SketchConstraint {
                id: constraint_id,
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
                native_ref: Some(frame.id.clone()),
            })
        }))
        .chain(null_pairs.iter().filter_map(|pair| {
            let scope = native_stream(&pair.id)?;
            if parameterized_offset_companions
                .contains(&(scope.to_owned(), pair.governing_companion_record_index))
            {
                return None;
            }
            let (parameter, parameter_id) =
                parameter_for(scope, pair.governing_companion_record_index)?;
            let indices = [pair.geometry_record_index];
            let sketch = sketch_for_geometry(scope, &indices)?;
            let constraint_id = neutral_dimension_constraint_id(&parameter_id, "null-pair");
            if design_dimension_unit(parameter) {
                if let Some(entity) = projected.get(&(scope, pair.geometry_record_index)) {
                    if let Some(definition) = null_locus_dimension_definition(
                        pair,
                        entity,
                        &parameter.source_kind,
                        parameter.evaluated_value,
                        parameter_id.clone(),
                    ) {
                        return Some(SketchConstraint {
                            id: constraint_id,
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
                            native_ref: Some(pair.id.clone()),
                        });
                    }
                }
            }
            let operands = vec![
                SketchNativeOperand {
                    native_kind: "null_locus".into(),
                    native_field: Some("locus".into()),
                    native_role: Some(pair.null_role),
                    object_index: 0,
                    native_ref: None,
                },
                native_operand(
                    scope,
                    "locus",
                    Some(pair.geometry_role),
                    pair.geometry_record_index,
                ),
            ];
            Some(SketchConstraint {
                id: constraint_id,
                sketch,
                definition: Definition::Native {
                    native_kind: parameter.source_kind.clone(),
                    native_state: None,
                    native_flags: None,
                    entities: indices
                        .iter()
                        .filter_map(|record_index| {
                            projected
                                .get(&(scope, *record_index))
                                .map(|entity| entity.id.clone())
                        })
                        .collect(),
                    parameter: Some(parameter_id),
                    operands,
                },
                name: None,
                driving: None,
                active: None,
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(pair.id.clone()),
            })
        }))
        .collect::<Vec<_>>();
    let companions_by_key = companions
        .iter()
        .filter_map(|companion| {
            Some((
                (
                    native_stream(&companion.id)?.to_owned(),
                    companion.record_index,
                ),
                companion,
            ))
        })
        .collect::<HashMap<_, _>>();
    let owners_by_companion = owners
        .iter()
        .filter_map(|owner| {
            Some((
                (
                    native_stream(&owner.id)?.to_owned(),
                    owner.companion_record_index,
                ),
                owner,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut recipes_by_companion = BTreeMap::<(String, u32), Vec<_>>::new();
    for record in recipe_records {
        let Some(scope) = native_stream(&record.id) else {
            continue;
        };
        let key = (scope.to_owned(), record.companion_record_index);
        if !locus_companions.contains(&key) {
            recipes_by_companion.entry(key).or_default().push(record);
        }
    }
    for records in recipes_by_companion.values_mut() {
        records.sort_by_key(|record| record.recipe_ordinal);
    }
    constraints.extend(recipes_by_companion.into_iter().filter_map(
        |((scope, companion_record_index), records)| {
            let companion = companions_by_key.get(&(scope.clone(), companion_record_index))?;
            let owner = owners_by_companion.get(&(scope.clone(), companion_record_index))?;
            let (parameter, parameter_id) = parameter_for(&scope, companion_record_index)?;
            let constraint_id = neutral_dimension_constraint_id(&parameter_id, "recipe-group");
            let sketch = sketches_by_scope
                .get(&(scope.as_str(), owner.scope_record_index))?
                .clone();
            let linear_candidates = if parameter.source_kind.starts_with("Linear Dimension")
                && design_dimension_unit(parameter)
            {
                recipe_linear_dimension_candidates(
                    entities,
                    &sketch,
                    parameter.evaluated_value * 10.0,
                    &parameter_id,
                )
            } else {
                Vec::default()
            };
            let repeated = repeated_linear_dimension(&linear_candidates, parameter_id.clone());
            let definition = match (linear_candidates.as_slice(), repeated) {
                ([definition], _) => definition.clone(),
                (_, Some(definition)) => definition,
                _ => Definition::Native {
                    native_kind: parameter.source_kind.clone(),
                    native_state: None,
                    native_flags: None,
                    entities: recipe_dimension_candidate_entities(&linear_candidates),
                    parameter: Some(parameter_id),
                    operands: records
                        .into_iter()
                        .map(|record| SketchNativeOperand {
                            native_kind: "construction_recipe".into(),
                            native_field: Some("recipe".into()),
                            native_role: None,
                            object_index: record.record_index,
                            native_ref: Some(record.id.clone()),
                        })
                        .collect(),
                },
            };
            Some(SketchConstraint {
                id: constraint_id,
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
                native_ref: Some(companion.id.clone()),
            })
        },
    ));
    let governed_companions = pairs
        .iter()
        .filter_map(|pair| {
            Some((
                native_stream(&pair.id)?.to_owned(),
                pair.governing_companion_record_index,
            ))
        })
        .chain(groups.iter().filter_map(|group| {
            Some((
                native_stream(&group.id)?.to_owned(),
                group.companion_record_index,
            ))
        }))
        .chain(annotation_frames.iter().filter_map(|frame| {
            Some((
                native_stream(&frame.id)?.to_owned(),
                frame.governing_companion_record_index,
            ))
        }))
        .chain(null_pairs.iter().filter_map(|pair| {
            Some((
                native_stream(&pair.id)?.to_owned(),
                pair.governing_companion_record_index,
            ))
        }))
        .chain(recipe_records.iter().filter_map(|record| {
            Some((
                native_stream(&record.id)?.to_owned(),
                record.companion_record_index,
            ))
        }))
        .collect::<HashSet<_>>();
    constraints.extend(companions.iter().filter_map(|companion| {
        let scope = native_stream(&companion.id)?;
        let key = (scope.to_owned(), companion.record_index);
        if governed_companions.contains(&key) {
            return None;
        }
        let owner = owners_by_companion.get(&key)?;
        let (parameter, parameter_id) = parameter_for(scope, companion.record_index)?;
        if parameter.kind != DesignParameterKind::Dimension {
            return None;
        }
        let sketch = sketches_by_scope
            .get(&(scope, owner.scope_record_index))?
            .clone();
        Some(SketchConstraint {
            id: neutral_dimension_constraint_id(&parameter_id, "companion-payload"),
            sketch,
            definition: Definition::Native {
                native_kind: parameter.source_kind.clone(),
                native_state: None,
                native_flags: None,
                entities: Vec::new(),
                parameter: Some(parameter_id),
                operands: vec![SketchNativeOperand {
                    native_kind: "dimension_companion".into(),
                    native_field: Some(
                        if companion.payload_byte_length == 0 {
                            "companion"
                        } else {
                            "companion_payload"
                        }
                        .into(),
                    ),
                    native_role: None,
                    object_index: companion.record_index,
                    native_ref: Some(companion.id.clone()),
                }],
            },
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: Some(companion.id.clone()),
        })
    }));
    constraints.sort_by_key(|constraint| constraint.id.clone());
    constraints
}

/// Attach single-locus offset dimensions to uniquely matching typed offset
/// relations and remove their redundant native annotation constraints.
pub(crate) fn bind_offset_dimension_parameters(
    constraints: &mut Vec<cadmpeg_ir::sketches::SketchConstraint>,
    parameters: &[DesignParameter],
) {
    use cadmpeg_ir::sketches::SketchConstraintDefinition as Definition;

    let parameter_values = parameters
        .iter()
        .filter_map(|parameter| {
            Some((neutral_parameter_id(parameter), design_length(parameter)?.0))
        })
        .collect::<HashMap<_, _>>();
    let mut bindings = Vec::new();
    for (dimension_index, dimension) in constraints.iter().enumerate() {
        let Definition::Native {
            native_kind,
            entities,
            parameter: Some(parameter),
            operands,
            ..
        } = &dimension.definition
        else {
            continue;
        };
        let [entity] = entities.as_slice() else {
            continue;
        };
        if !native_kind.starts_with("Linear Dimension")
            || operands.len() != 2
            || operands[0].native_kind != "null_locus"
            || operands[1].native_kind != "curve"
        {
            continue;
        }
        let Some(parameter_value) = parameter_values.get(parameter).copied() else {
            continue;
        };
        let candidates = constraints
            .iter()
            .enumerate()
            .filter_map(|(offset_index, constraint)| {
                if constraint.sketch != dimension.sketch {
                    return None;
                }
                let Definition::Offset {
                    pairs,
                    distance,
                    parameter: None,
                    parameter_factor: None,
                } = &constraint.definition
                else {
                    return None;
                };
                (pairs.iter().any(|pair| &pair.source == entity)
                    && scalar_close(distance.0, parameter_value.abs()))
                .then_some(offset_index)
            })
            .collect::<Vec<_>>();
        if let [offset_index] = candidates.as_slice() {
            bindings.push((
                dimension_index,
                *offset_index,
                parameter.clone(),
                parameter_value,
            ));
        }
    }
    let offset_counts = bindings.iter().fold(HashMap::new(), |mut counts, binding| {
        *counts.entry(binding.1).or_insert(0usize) += 1;
        counts
    });
    bindings.retain(|binding| offset_counts.get(&binding.1) == Some(&1));
    for (_, offset_index, parameter, parameter_value) in &bindings {
        let Definition::Offset {
            parameter: driving_parameter,
            parameter_factor,
            ..
        } = &mut constraints[*offset_index].definition
        else {
            unreachable!("offset binding index was selected from typed offsets")
        };
        *driving_parameter = Some(parameter.clone());
        *parameter_factor = Some(if parameter_value.is_sign_positive() {
            1.0
        } else {
            -1.0
        });
    }
    let removed = bindings
        .into_iter()
        .map(|(dimension, _, _, _)| dimension)
        .collect::<HashSet<_>>();
    let mut index = 0usize;
    constraints.retain(|_| {
        let keep = !removed.contains(&index);
        index += 1;
        keep
    });
}

/// Project dimensions owned by model-space sketches without assigning them
/// planar relation semantics.
pub fn project_spatial_dimension_constraints(
    inputs: &DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    spatial_entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Vec<cadmpeg_ir::sketches::SpatialSketchConstraint> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition, SpatialSketchConstraint, SpatialSketchConstraintDefinition,
    };

    let &DimensionConstraintInputs {
        placements,
        parameters,
        points,
        curves,
        ..
    } = inputs;

    let spatial_by_planar_id = placements
        .iter()
        .filter_map(|placement| {
            let spatial_id = neutral_spatial_sketch_id(placement);
            spatial_sketches
                .iter()
                .any(|sketch| sketch.id == spatial_id)
                .then(|| (neutral_sketch_id(placement), spatial_id))
        })
        .collect::<HashMap<_, _>>();
    let native_record_indices = points
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
        .collect::<HashMap<_, _>>();
    let spatial_by_record = spatial_entities
        .iter()
        .filter_map(|entity| {
            let native_ref = entity.native_ref.as_deref()?;
            native_record_indices
                .get(native_ref)
                .map(|key| (*key, entity))
        })
        .collect::<HashMap<_, _>>();
    let parameter_lengths = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                neutral_parameter_id(parameter),
                design_length(parameter)?.0.abs(),
            ))
        })
        .collect::<HashMap<_, _>>();
    project_all_dimension_constraints(inputs)
        .into_iter()
        .filter_map(|constraint| {
            let sketch = spatial_by_planar_id.get(&constraint.sketch)?.clone();
            let definition = match constraint.definition {
                SketchConstraintDefinition::Native {
                    native_kind,
                    native_state,
                    parameter,
                    operands,
                    ..
                } => {
                    let distance = parameter.as_ref().and_then(|parameter| {
                        let expected = *parameter_lengths.get(parameter)?;
                        let scope = native_stream(constraint.native_ref.as_deref()?)?;
                        let measured = operands
                            .iter()
                            .filter(|operand| operand.object_index != 0)
                            .map(|operand| {
                                spatial_by_record
                                    .get(&(scope, operand.object_index))
                                    .copied()
                            })
                            .collect::<Option<Vec<_>>>()?;
                        let [first, second] = measured.as_slice() else {
                            return None;
                        };
                        (native_kind.starts_with("Linear Dimension")
                            && first.sketch == sketch
                            && second.sketch == sketch
                            && spatial_parallel_line_distance_matches(
                                &first.geometry,
                                &second.geometry,
                                expected,
                            ))
                        .then(|| {
                            SpatialSketchConstraintDefinition::ParallelLineDistance {
                                first: first.id.clone(),
                                second: second.id.clone(),
                                parameter: parameter.clone(),
                            }
                        })
                    });
                    distance.unwrap_or(SpatialSketchConstraintDefinition::Native {
                        native_kind,
                        native_state,
                        parameter,
                        operands,
                    })
                }
                _ => return None,
            };
            Some(SpatialSketchConstraint {
                id: constraint.id,
                sketch,
                definition,
                native_ref: constraint.native_ref,
            })
        })
        .collect()
}

pub(crate) fn spatial_parallel_line_distance_matches(
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    expected: f64,
) -> bool {
    use cadmpeg_ir::sketches::SpatialSketchGeometry;

    let (
        SpatialSketchGeometry::Line {
            start: first_start,
            end: first_end,
        },
        SpatialSketchGeometry::Line {
            start: second_start,
            end: second_end,
        },
    ) = (first, second)
    else {
        return false;
    };
    let first_direction = Vector3::new(
        first_end.x - first_start.x,
        first_end.y - first_start.y,
        first_end.z - first_start.z,
    );
    let second_direction = Vector3::new(
        second_end.x - second_start.x,
        second_end.y - second_start.y,
        second_end.z - second_start.z,
    );
    let first_length = first_direction.norm();
    let second_length = second_direction.norm();
    let cross = Vector3::new(
        first_direction.y * second_direction.z - first_direction.z * second_direction.y,
        first_direction.z * second_direction.x - first_direction.x * second_direction.z,
        first_direction.x * second_direction.y - first_direction.y * second_direction.x,
    );
    if first_length <= 1.0e-12
        || second_length <= 1.0e-12
        || cross.norm() > 1.0e-9 * first_length * second_length
    {
        return false;
    }
    let offset = Vector3::new(
        second_start.x - first_start.x,
        second_start.y - first_start.y,
        second_start.z - first_start.z,
    );
    let area = Vector3::new(
        offset.y * first_direction.z - offset.z * first_direction.y,
        offset.z * first_direction.x - offset.x * first_direction.z,
        offset.x * first_direction.y - offset.y * first_direction.x,
    )
    .norm();
    let measured = area / first_length;
    let scale = 1.0 + measured.max(expected.abs());
    expected.is_finite() && (measured - expected.abs()).abs() <= 1.0e-9 * scale
}

pub(crate) fn repeated_linear_dimension(
    candidates: &[cadmpeg_ir::sketches::SketchConstraintDefinition],
    parameter: cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchDistanceMeasurement as Measurement,
        SketchLocus,
    };

    if candidates.len() < 2 {
        return None;
    }
    let mut entities = HashSet::new();
    let mut measurements = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let (first, second, measurement) = match candidate {
            Definition::Distance { entities: pair, .. } => {
                let [first, second] = pair.as_slice() else {
                    return None;
                };
                (
                    first,
                    second,
                    Measurement::Distance {
                        first: SketchLocus::Entity(first.clone()),
                        second: SketchLocus::Entity(second.clone()),
                    },
                )
            }
            Definition::HorizontalDistance { first, second, .. } => (
                locus_entity_id(first),
                locus_entity_id(second),
                Measurement::Horizontal {
                    first: first.clone(),
                    second: second.clone(),
                },
            ),
            Definition::VerticalDistance { first, second, .. } => (
                locus_entity_id(first),
                locus_entity_id(second),
                Measurement::Vertical {
                    first: first.clone(),
                    second: second.clone(),
                },
            ),
            _ => return None,
        };
        if first == second || !entities.insert(first.clone()) || !entities.insert(second.clone()) {
            return None;
        }
        measurements.push(measurement);
    }
    Some(Definition::RepeatedDistance {
        measurements,
        parameter,
    })
}

fn locus_entity_id(
    locus: &cadmpeg_ir::sketches::SketchLocus,
) -> &cadmpeg_ir::sketches::SketchEntityId {
    use cadmpeg_ir::sketches::SketchLocus;
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity,
    }
}

pub(crate) fn null_locus_dimension_definition(
    pair: &DesignDimensionNullLocusPair,
    entity: &cadmpeg_ir::sketches::SketchEntity,
    source_kind: &str,
    evaluated_value: f64,
    parameter: cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchAxis, SketchConstraintDefinition as Definition, SketchGeometry,
    };

    if let Some(definition) =
        radial_dimension_definition(entity, source_kind, evaluated_value, parameter.clone())
    {
        return Some(definition);
    }
    if source_kind != "Angular Dimension-2"
        || pair.null_role != 14
        || pair.geometry_role != 3
        || !matches!(entity.geometry, SketchGeometry::Line { .. })
    {
        return None;
    }
    let horizontal_axis = SketchGeometry::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    };
    line_angle_matches(&entity.geometry, &horizontal_axis, evaluated_value).then(|| {
        Definition::AngleToAxis {
            entity: entity.id.clone(),
            axis: SketchAxis::Horizontal,
            parameter,
        }
    })
}

pub(crate) fn radial_dimension_definition(
    entity: &cadmpeg_ir::sketches::SketchEntity,
    source_kind: &str,
    evaluated_value: f64,
    parameter: cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchGeometry as Geometry,
    };

    let radius = match &entity.geometry {
        Geometry::Circle { radius, .. } | Geometry::Arc { radius, .. } => radius.0,
        _ => return None,
    };
    let measured = if source_kind.starts_with("Radius Dimension") {
        radius
    } else if source_kind.starts_with("Diameter Dimension") {
        2.0 * radius
    } else {
        return None;
    };
    let evaluated = evaluated_value * 10.0;
    let scale = 1.0 + measured.abs().max(evaluated.abs());
    if !evaluated.is_finite() || (measured - evaluated).abs() > 1.0e-9 * scale {
        return None;
    }
    Some(if source_kind.starts_with("Radius Dimension") {
        Definition::Radius {
            entity: entity.id.clone(),
            parameter,
        }
    } else {
        Definition::Diameter {
            entity: entity.id.clone(),
            parameter,
        }
    })
}

/// Remove generic relation parses whose exact stream position is owned by a
/// typed dimension frame.
pub fn remove_dimension_frame_relations(
    relations: &mut Vec<SketchRelation>,
    pairs: &[DesignDimensionLocusPair],
    groups: &[DesignDimensionLocusGroup],
    null_pairs: &[DesignDimensionNullLocusPair],
) {
    let dimension_frames =
        pairs
            .iter()
            .filter_map(|pair| Some((native_stream(&pair.id)?.to_owned(), pair.byte_offset)))
            .chain(groups.iter().filter_map(|group| {
                Some((native_stream(&group.id)?.to_owned(), group.byte_offset))
            }))
            .chain(
                null_pairs.iter().filter_map(|pair| {
                    Some((native_stream(&pair.id)?.to_owned(), pair.byte_offset))
                }),
            )
            .collect::<HashSet<_>>();
    relations.retain(|relation| {
        native_stream(&relation.id).is_none_or(|scope| {
            !dimension_frames.contains(&(scope.to_owned(), relation.byte_offset))
        })
    });
}

/// Bind geometry referenced only by dimensional companions to the sketch
/// reached through the parameter scope or the counted frame's explicit owner.
#[allow(clippy::too_many_arguments)]
pub fn bind_dimension_loci(
    placements: &[DesignSketchPlacement],
    owners: &[DesignParameterOwner],
    pairs: &[DesignDimensionLocusPair],
    groups: &[DesignDimensionLocusGroup],
    annotation_frames: &[DesignDimensionAnnotationFrame],
    null_pairs: &[DesignDimensionNullLocusPair],
    points: &mut [SketchPoint],
    curves: &mut [SketchCurveIdentity],
) -> Result<(), CodecError> {
    let placements_by_scope = placements
        .iter()
        .filter_map(|placement| {
            Some((
                (native_stream(&placement.id)?, placement.scope_record_index?),
                u32::try_from(placement.entity_suffix).ok()?,
            ))
        })
        .collect::<HashMap<_, _>>();
    let scopes_by_companion = owners
        .iter()
        .filter_map(|owner| {
            Some((
                (native_stream(&owner.id)?, owner.companion_record_index),
                owner.scope_record_index,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut bindings = HashMap::<(String, u32), u32>::new();
    for pair in pairs {
        let Some(scope) = native_stream(&pair.id) else {
            continue;
        };
        let Some(parameter_scope) = scopes_by_companion
            .get(&(scope, pair.governing_companion_record_index))
            .copied()
        else {
            continue;
        };
        let Some(owner) = placements_by_scope.get(&(scope, parameter_scope)).copied() else {
            continue;
        };
        insert_dimension_binding(
            &mut bindings,
            scope,
            pair.first_geometry_record_index,
            owner,
        )?;
        insert_dimension_binding(
            &mut bindings,
            scope,
            pair.second_geometry_record_index,
            owner,
        )?;
    }
    for group in groups {
        let Some(scope) = native_stream(&group.id) else {
            continue;
        };
        for locus in &group.loci {
            insert_dimension_binding(
                &mut bindings,
                scope,
                locus.geometry_record_index,
                group.owner_reference,
            )?;
        }
    }
    for frame in annotation_frames {
        let Some(scope) = native_stream(&frame.id) else {
            continue;
        };
        for record_index in frame.operands.iter().filter_map(|operand| {
            (operand.geometry_record_index != 0).then_some(operand.geometry_record_index)
        }) {
            insert_dimension_binding(&mut bindings, scope, record_index, frame.owner_reference)?;
        }
    }
    for pair in null_pairs {
        let Some(scope) = native_stream(&pair.id) else {
            continue;
        };
        let Some(parameter_scope) = scopes_by_companion
            .get(&(scope, pair.governing_companion_record_index))
            .copied()
        else {
            continue;
        };
        let Some(owner) = placements_by_scope.get(&(scope, parameter_scope)).copied() else {
            continue;
        };
        insert_dimension_binding(&mut bindings, scope, pair.geometry_record_index, owner)?;
    }
    for point in points {
        let Some(scope) = native_stream(&point.id) else {
            continue;
        };
        let Some(owner) = bindings
            .get(&(scope.to_owned(), point.record_index))
            .copied()
        else {
            continue;
        };
        if point
            .owner_reference
            .replace(owner)
            .is_some_and(|existing| existing != owner)
        {
            return Err(CodecError::Malformed(format!(
                "Fusion sketch point {} has conflicting relation and dimension owners",
                point.record_index
            )));
        }
    }
    for curve in curves {
        let Some(scope) = native_stream(&curve.id) else {
            continue;
        };
        let Some(owner) = bindings
            .get(&(scope.to_owned(), curve.record_index))
            .copied()
        else {
            continue;
        };
        if curve
            .owner_reference
            .replace(owner)
            .is_some_and(|existing| existing != owner)
        {
            return Err(CodecError::Malformed(format!(
                "Fusion sketch curve {} has conflicting relation and dimension owners",
                curve.record_index
            )));
        }
    }
    Ok(())
}

fn insert_dimension_binding(
    bindings: &mut HashMap<(String, u32), u32>,
    scope: &str,
    record_index: u32,
    owner: u32,
) -> Result<(), CodecError> {
    if bindings
        .insert((scope.to_owned(), record_index), owner)
        .is_some_and(|existing| existing != owner)
    {
        return Err(CodecError::Malformed(format!(
            "Fusion dimensional geometry record {record_index} belongs to multiple sketches"
        )));
    }
    Ok(())
}

pub(crate) fn exact_atomic_constraint(
    kind: SketchConstraintKind,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchGeometry as Geometry, SketchLocus,
    };

    let lines = || {
        (entities.len() == 2
            && entities[0].id != entities[1].id
            && entities
                .iter()
                .all(|entity| matches!(entity.geometry, Geometry::Line { .. })))
        .then(|| (entities[0].id.clone(), entities[1].id.clone()))
    };
    let curves = || {
        (entities.len() == 2
            && entities[0].id != entities[1].id
            && entities.iter().all(|entity| {
                matches!(
                    entity.geometry,
                    Geometry::Line { .. }
                        | Geometry::Circle { .. }
                        | Geometry::Arc { .. }
                        | Geometry::Ellipse { .. }
                        | Geometry::Nurbs { .. }
                )
            }))
        .then(|| (entities[0].id.clone(), entities[1].id.clone()))
    };
    let equal_size_entities = || {
        let [first, second] = entities else {
            return None;
        };
        (first.id != second.id
            && matches!(
                (&first.geometry, &second.geometry),
                (Geometry::Line { .. }, Geometry::Line { .. })
                    | (
                        Geometry::Circle { .. } | Geometry::Arc { .. },
                        Geometry::Circle { .. } | Geometry::Arc { .. }
                    )
                    | (Geometry::Ellipse { .. }, Geometry::Ellipse { .. })
            ))
        .then(|| (first.id.clone(), second.id.clone()))
    };
    match kind {
        SketchConstraintKind::Coincident
            if entities.len() >= 2
                && entities
                    .iter()
                    .map(|entity| &entity.id)
                    .collect::<HashSet<_>>()
                    .len()
                    == entities.len() =>
        {
            Some(Definition::Coincident {
                entities: entities.iter().map(|entity| entity.id.clone()).collect(),
            })
        }
        SketchConstraintKind::Colinear => {
            lines().map(|(first, second)| Definition::Collinear { first, second })
        }
        SketchConstraintKind::Concentric => {
            if entities.len() == 2
                && entities[0].id != entities[1].id
                && entities.iter().all(|entity| {
                    matches!(
                        entity.geometry,
                        Geometry::Circle { .. } | Geometry::Arc { .. } | Geometry::Ellipse { .. }
                    )
                })
            {
                return Some(Definition::Concentric {
                    first: entities[0].id.clone(),
                    second: entities[1].id.clone(),
                });
            }
            let (first, second, axis) = reflected_symmetry(entities)?;
            Some(Definition::Symmetric {
                first: cadmpeg_ir::sketches::SketchLocus::Entity(first.id.clone()),
                second: cadmpeg_ir::sketches::SketchLocus::Entity(second.id.clone()),
                axis: axis.id.clone(),
            })
        }
        SketchConstraintKind::Symmetry => {
            let (first, second, axis) = reflected_symmetry(entities)?;
            Some(Definition::Symmetric {
                first: cadmpeg_ir::sketches::SketchLocus::Entity(first.id.clone()),
                second: cadmpeg_ir::sketches::SketchLocus::Entity(second.id.clone()),
                axis: axis.id.clone(),
            })
        }
        SketchConstraintKind::EqualLength => {
            lines().map(|(first, second)| Definition::Equal { first, second })
        }
        SketchConstraintKind::Parallel => lines()
            .map(|(first, second)| Definition::Parallel { first, second })
            .or_else(|| midpoint_constraint(entities)),
        SketchConstraintKind::Perpendicular => {
            lines().map(|(first, second)| Definition::Perpendicular { first, second })
        }
        SketchConstraintKind::Horizontal
            if entities.len() == 1 && matches!(entities[0].geometry, Geometry::Line { .. }) =>
        {
            Some(Definition::Horizontal {
                entity: entities[0].id.clone(),
            })
        }
        SketchConstraintKind::Horizontal
            if entities.len() == 2
                && entities[0].id != entities[1].id
                && entities
                    .iter()
                    .all(|entity| matches!(entity.geometry, Geometry::Point { .. })) =>
        {
            Some(Definition::HorizontalLoci {
                first: SketchLocus::Entity(entities[0].id.clone()),
                second: SketchLocus::Entity(entities[1].id.clone()),
            })
        }
        SketchConstraintKind::Vertical
            if entities.len() == 1 && matches!(entities[0].geometry, Geometry::Line { .. }) =>
        {
            Some(Definition::Vertical {
                entity: entities[0].id.clone(),
            })
        }
        SketchConstraintKind::Vertical
            if entities.len() == 2
                && entities[0].id != entities[1].id
                && entities
                    .iter()
                    .all(|entity| matches!(entity.geometry, Geometry::Point { .. })) =>
        {
            Some(Definition::VerticalLoci {
                first: SketchLocus::Entity(entities[0].id.clone()),
                second: SketchLocus::Entity(entities[1].id.clone()),
            })
        }
        SketchConstraintKind::Tangent => {
            curves().map(|(first, second)| Definition::Tangent { first, second })
        }
        SketchConstraintKind::Curvature => {
            curves().map(|(first, second)| Definition::Curvature { first, second })
        }
        SketchConstraintKind::Midpoint => midpoint_constraint(entities),
        SketchConstraintKind::Equal => {
            equal_size_entities().map(|(first, second)| Definition::Equal { first, second })
        }
        SketchConstraintKind::Polygon
            if entities.len() >= 3
                && entities
                    .iter()
                    .map(|entity| &entity.id)
                    .collect::<HashSet<_>>()
                    .len()
                    == entities.len() =>
        {
            Some(Definition::Polygon {
                entities: entities.iter().map(|entity| entity.id.clone()).collect(),
            })
        }
        SketchConstraintKind::SplineGroup
            if entities.len() >= 2
                && entities
                    .iter()
                    .map(|entity| &entity.id)
                    .collect::<HashSet<_>>()
                    .len()
                    == entities.len() =>
        {
            Some(Definition::SplineGroup {
                entities: entities.iter().map(|entity| entity.id.clone()).collect(),
            })
        }
        _ => None,
    }
}

pub(crate) fn exact_coincident_loci(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchGeometry as Geometry, SketchLocus,
    };

    let loci = |entity: &cadmpeg_ir::sketches::SketchEntity| {
        let mut loci = Vec::new();
        if let Some([start, end]) = sketch_entity_endpoints(entity) {
            loci.push((SketchLocus::Start(entity.id.clone()), start));
            loci.push((SketchLocus::End(entity.id.clone()), end));
        }
        match &entity.geometry {
            Geometry::Point { position } => {
                loci.push((SketchLocus::Entity(entity.id.clone()), *position));
            }
            Geometry::Circle { center, .. }
            | Geometry::Arc { center, .. }
            | Geometry::Ellipse { center, .. }
            | Geometry::Hyperbola { center, .. } => {
                loci.push((SketchLocus::Center(entity.id.clone()), *center));
            }
            Geometry::Line { .. }
            | Geometry::ReferenceLine { .. }
            | Geometry::Parabola { .. }
            | Geometry::Nurbs { .. }
            | Geometry::Text { .. }
            | Geometry::Native { .. } => {}
        }
        loci
    };

    if entities.len() < 2
        || entities
            .iter()
            .map(|entity| &entity.id)
            .collect::<HashSet<_>>()
            .len()
            != entities.len()
    {
        return None;
    }
    let loci = entities
        .iter()
        .map(|entity| loci(entity))
        .collect::<Vec<_>>();
    let mut solutions = Vec::new();
    for (first_locus, position) in &loci[0] {
        let mut solution = vec![first_locus.clone()];
        for member_loci in loci.iter().skip(1) {
            let matches = member_loci
                .iter()
                .filter(|(_, candidate)| {
                    (candidate.u - position.u).hypot(candidate.v - position.v) <= 1.0e-9
                })
                .collect::<Vec<_>>();
            let [matched] = matches.as_slice() else {
                solution.clear();
                break;
            };
            solution.push(matched.0.clone());
        }
        if solution.len() == entities.len() && !solutions.contains(&solution) {
            solutions.push(solution);
        }
    }
    let [loci] = solutions.as_slice() else {
        return None;
    };
    Some(Definition::CoincidentLoci { loci: loci.clone() })
}

fn midpoint_constraint(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchGeometry as Geometry, SketchLocus,
    };

    let (line, point) = match entities {
        [line, point]
            if matches!(line.geometry, Geometry::Line { .. })
                && matches!(point.geometry, Geometry::Point { .. }) =>
        {
            (*line, *point)
        }
        [point, line]
            if matches!(line.geometry, Geometry::Line { .. })
                && matches!(point.geometry, Geometry::Point { .. }) =>
        {
            (*line, *point)
        }
        _ => return None,
    };
    let Geometry::Line { start, end } = &line.geometry else {
        unreachable!("line operand matched above")
    };
    let Geometry::Point { position } = &point.geometry else {
        unreachable!("point operand matched above")
    };
    let midpoint = Point2::new((start.u + end.u) * 0.5, (start.v + end.v) * 0.5);
    ((position.u - midpoint.u).abs() <= 1.0e-9 && (position.v - midpoint.v).abs() <= 1.0e-9).then(
        || Definition::Midpoint {
            point: SketchLocus::Entity(point.id.clone()),
            entity: line.id.clone(),
        },
    )
}

pub(crate) fn indirect_angular_lines(
    scope: &str,
    operands: &[&cadmpeg_ir::sketches::SketchEntity],
    evaluated_value: f64,
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
) -> Option<(
    cadmpeg_ir::sketches::SketchEntityId,
    cadmpeg_ir::sketches::SketchEntityId,
)> {
    use cadmpeg_ir::sketches::SketchGeometry;

    let (point_ordinal, point, explicit_line) = match operands {
        [point, line]
            if matches!(point.geometry, SketchGeometry::Point { .. })
                && matches!(line.geometry, SketchGeometry::Line { .. }) =>
        {
            (0, *point, *line)
        }
        [line, point]
            if matches!(line.geometry, SketchGeometry::Line { .. })
                && matches!(point.geometry, SketchGeometry::Point { .. }) =>
        {
            (1, *point, *line)
        }
        _ => return None,
    };
    let SketchGeometry::Point { position } = &point.geometry else {
        unreachable!("point operand matched above")
    };
    if !evaluated_value.is_finite() || !(0.0..=std::f64::consts::PI).contains(&evaluated_value) {
        return None;
    }
    let mut candidates = projected
        .iter()
        .filter(|((candidate_scope, _), candidate)| {
            *candidate_scope == scope
                && candidate.sketch == explicit_line.sketch
                && candidate.id != explicit_line.id
        })
        .filter_map(|(_, candidate)| {
            let SketchGeometry::Line { start, end } = &candidate.geometry else {
                return None;
            };
            (sketch_points_close(*position, *start) || sketch_points_close(*position, *end))
                .then_some(*candidate)
        })
        .filter(|candidate| {
            line_angle_matches(
                &explicit_line.geometry,
                &candidate.geometry,
                evaluated_value,
            )
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    candidates.dedup_by(|left, right| left.id == right.id);
    let candidate = (candidates.len() == 1).then(|| candidates.remove(0))?;
    Some(if point_ordinal == 0 {
        (candidate.id.clone(), explicit_line.id.clone())
    } else {
        (explicit_line.id.clone(), candidate.id.clone())
    })
}

pub(crate) fn directional_point_dimension(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    evaluated_mm: f64,
    parameter: cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchGeometry, SketchLocus,
    };

    let [first, second] = entities else {
        return None;
    };
    let SketchGeometry::Point {
        position: first_position,
    } = &first.geometry
    else {
        return None;
    };
    let SketchGeometry::Point {
        position: second_position,
    } = &second.geometry
    else {
        return None;
    };
    let expected = evaluated_mm.abs();
    let scale = 1.0 + expected;
    let first_locus = SketchLocus::Entity(first.id.clone());
    let second_locus = SketchLocus::Entity(second.id.clone());
    let horizontal =
        ((first_position.u - second_position.u).abs() - expected).abs() <= scale * 1.0e-9;
    let vertical =
        ((first_position.v - second_position.v).abs() - expected).abs() <= scale * 1.0e-9;
    match (horizontal, vertical) {
        (true, false) => Some(Definition::HorizontalDistance {
            first: first_locus,
            second: second_locus,
            parameter,
        }),
        (false, true) => Some(Definition::VerticalDistance {
            first: first_locus,
            second: second_locus,
            parameter,
        }),
        (false, false) | (true, true) => None,
    }
}

pub(crate) fn recipe_linear_dimension_candidates(
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    evaluated_mm: f64,
    parameter: &cadmpeg_ir::features::ParameterId,
) -> Vec<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    let sketch_entities = entities
        .iter()
        .filter(|entity| &entity.sketch == sketch)
        .collect::<Vec<_>>();
    let points = sketch_entities
        .iter()
        .copied()
        .filter(|entity| {
            matches!(
                entity.geometry,
                cadmpeg_ir::sketches::SketchGeometry::Point { .. }
            )
        })
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for first in 0..points.len() {
        for second in first + 1..points.len() {
            if let Some(definition) = directional_point_dimension(
                &[points[first], points[second]],
                evaluated_mm,
                parameter.clone(),
            ) {
                candidates.push(definition);
            }
        }
    }
    let lines = sketch_entities
        .iter()
        .copied()
        .filter(|entity| {
            matches!(
                entity.geometry,
                cadmpeg_ir::sketches::SketchGeometry::Line { .. }
            )
        })
        .collect::<Vec<_>>();
    for first in 0..lines.len() {
        for second in first + 1..lines.len() {
            if parallel_line_separation(lines[first], lines[second], evaluated_mm) {
                candidates.push(cadmpeg_ir::sketches::SketchConstraintDefinition::Distance {
                    entities: vec![lines[first].id.clone(), lines[second].id.clone()],
                    parameter: parameter.clone(),
                });
            }
        }
    }
    candidates
}

pub(crate) fn recipe_dimension_candidate_entities(
    candidates: &[cadmpeg_ir::sketches::SketchConstraintDefinition],
) -> Vec<cadmpeg_ir::sketches::SketchEntityId> {
    use cadmpeg_ir::sketches::SketchConstraintDefinition as Definition;

    let mut entities = Vec::new();
    for candidate in candidates {
        let candidate_entities = match candidate {
            Definition::Distance {
                entities: candidate_entities,
                ..
            } => candidate_entities.clone(),
            Definition::HorizontalDistance { first, second, .. }
            | Definition::VerticalDistance { first, second, .. } => {
                vec![
                    locus_entity_id(first).clone(),
                    locus_entity_id(second).clone(),
                ]
            }
            _ => Vec::new(),
        };
        for entity in candidate_entities {
            if !entities.contains(&entity) {
                entities.push(entity);
            }
        }
    }
    entities
}

pub(crate) fn parallel_line_separation(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    evaluated_mm: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    let SketchGeometry::Line {
        start: first_start,
        end: first_end,
    } = &first.geometry
    else {
        return false;
    };
    let SketchGeometry::Line {
        start: second_start,
        end: second_end,
    } = &second.geometry
    else {
        return false;
    };
    let first_direction = Point2::new(first_end.u - first_start.u, first_end.v - first_start.v);
    let second_direction =
        Point2::new(second_end.u - second_start.u, second_end.v - second_start.v);
    let first_length = first_direction.u.hypot(first_direction.v);
    let second_length = second_direction.u.hypot(second_direction.v);
    if first_length <= 1.0e-12 || second_length <= 1.0e-12 {
        return false;
    }
    let cross = first_direction.u * second_direction.v - first_direction.v * second_direction.u;
    if cross.abs() > 1.0e-9 * first_length * second_length {
        return false;
    }
    let offset = Point2::new(
        second_start.u - first_start.u,
        second_start.v - first_start.v,
    );
    let separation =
        (offset.u * first_direction.v - offset.v * first_direction.u).abs() / first_length;
    let expected = evaluated_mm.abs();
    (separation - expected).abs() <= 1.0e-9 * (1.0 + expected)
}

pub(crate) fn concentric_circle_separation(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    evaluated_mm: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    let (
        SketchGeometry::Circle {
            center: first_center,
            radius: first_radius,
        },
        SketchGeometry::Circle {
            center: second_center,
            radius: second_radius,
        },
    ) = (&first.geometry, &second.geometry)
    else {
        return false;
    };
    if !evaluated_mm.is_finite() {
        return false;
    }
    let coordinate_scale = 1.0
        + first_center
            .u
            .abs()
            .max(first_center.v.abs())
            .max(second_center.u.abs())
            .max(second_center.v.abs());
    let center_separation =
        (first_center.u - second_center.u).hypot(first_center.v - second_center.v);
    if center_separation > 1.0e-9 * coordinate_scale {
        return false;
    }
    let measured = (first_radius.0 - second_radius.0).abs();
    let expected = evaluated_mm.abs();
    measured > 0.0 && (measured - expected).abs() <= 1.0e-9 * (1.0 + measured.max(expected))
}

pub(crate) fn point_line_separation(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    evaluated_mm: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    let (point, line) = match (&first.geometry, &second.geometry) {
        (SketchGeometry::Point { position }, SketchGeometry::Line { start, end })
        | (SketchGeometry::Line { start, end }, SketchGeometry::Point { position }) => {
            (*position, (*start, *end))
        }
        _ => return false,
    };
    let direction = Point2::new(line.1.u - line.0.u, line.1.v - line.0.v);
    let length = direction.u.hypot(direction.v);
    if length <= 1.0e-12 || !evaluated_mm.is_finite() {
        return false;
    }
    let offset = Point2::new(point.u - line.0.u, point.v - line.0.v);
    let measured = (offset.u * direction.v - offset.v * direction.u).abs() / length;
    let expected = evaluated_mm.abs();
    (measured - expected).abs() <= 1.0e-9 * (1.0 + measured.max(expected))
}

pub(crate) fn two_locus_distance_dimension(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    parameter: cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::SketchConstraintDefinition as Definition;

    (entities.len() == 2 && entities[0].id != entities[1].id).then(|| Definition::Distance {
        entities: entities.iter().map(|entity| entity.id.clone()).collect(),
        parameter,
    })
}

pub(crate) fn exact_counted_dimension_relation(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition as Definition, SketchGeometry, SketchLocus,
    };

    if let Some((first, second, axis)) = reflected_symmetry(entities) {
        return Some(Definition::Symmetric {
            first: SketchLocus::Entity(first.id.clone()),
            second: SketchLocus::Entity(second.id.clone()),
            axis: axis.id.clone(),
        });
    }
    let [first, second] = entities else {
        return None;
    };
    if first.id == second.id {
        return None;
    }
    let point_on_geometry =
        |point: &cadmpeg_ir::sketches::SketchEntity,
         geometry: &cadmpeg_ir::sketches::SketchEntity| {
            let SketchGeometry::Point { position } = point.geometry else {
                return false;
            };
            point_lies_on_sketch_geometry(position, &geometry.geometry)
        };
    if point_on_geometry(first, second) || point_on_geometry(second, first) {
        return Some(Definition::Coincident {
            entities: vec![first.id.clone(), second.id.clone()],
        });
    }
    let (
        SketchGeometry::Line {
            start: first_start,
            end: first_end,
        },
        SketchGeometry::Line {
            start: second_start,
            end: second_end,
        },
    ) = (&first.geometry, &second.geometry)
    else {
        return None;
    };
    let first_direction = Point2::new(first_end.u - first_start.u, first_end.v - first_start.v);
    let second_direction =
        Point2::new(second_end.u - second_start.u, second_end.v - second_start.v);
    let first_length = first_direction
        .u
        .mul_add(first_direction.u, first_direction.v * first_direction.v)
        .sqrt();
    let second_length = second_direction
        .u
        .mul_add(second_direction.u, second_direction.v * second_direction.v)
        .sqrt();
    if first_length <= 1.0e-9 || second_length <= 1.0e-9 {
        return None;
    }
    let scale = first_length * second_length;
    let cross = first_direction
        .u
        .mul_add(second_direction.v, -first_direction.v * second_direction.u);
    if cross.abs() <= scale * 1.0e-9 {
        let signed_offset = parallel_line_offset(&first.geometry, &second.geometry)?;
        return Some(if signed_offset.abs() <= 1.0e-9 * (1.0 + first_length) {
            Definition::Collinear {
                first: first.id.clone(),
                second: second.id.clone(),
            }
        } else {
            Definition::Parallel {
                first: first.id.clone(),
                second: second.id.clone(),
            }
        });
    }
    let dot = first_direction
        .u
        .mul_add(second_direction.u, first_direction.v * second_direction.v);
    (dot.abs() <= scale * 1.0e-9).then(|| Definition::Perpendicular {
        first: first.id.clone(),
        second: second.id.clone(),
    })
}

pub(crate) fn point_lies_on_sketch_geometry(
    point: Point2,
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    let close = |left: f64, right: f64| {
        (left - right).abs() <= 1.0e-9 * (1.0 + left.abs().max(right.abs()))
    };
    match geometry {
        SketchGeometry::Point { position } => sketch_points_close(point, *position),
        SketchGeometry::Line { start, end } => {
            let direction = Point2::new(end.u - start.u, end.v - start.v);
            let length_squared = direction.u.mul_add(direction.u, direction.v * direction.v);
            if length_squared <= 1.0e-18 {
                return false;
            }
            let relative = Point2::new(point.u - start.u, point.v - start.v);
            let parameter =
                relative.u.mul_add(direction.u, relative.v * direction.v) / length_squared;
            let cross = relative.u.mul_add(direction.v, -relative.v * direction.u);
            (-1.0e-9..=1.0 + 1.0e-9).contains(&parameter)
                && cross.abs() <= 1.0e-9 * (1.0 + length_squared.sqrt())
        }
        SketchGeometry::ReferenceLine { origin, direction } => {
            let length = direction.u.hypot(direction.v);
            if length <= 1.0e-9 {
                return false;
            }
            let relative = Point2::new(point.u - origin.u, point.v - origin.v);
            relative
                .u
                .mul_add(direction.v, -relative.v * direction.u)
                .abs()
                <= 1.0e-9 * (1.0 + length)
        }
        SketchGeometry::Circle { center, radius } => {
            close((point.u - center.u).hypot(point.v - center.v), radius.0)
        }
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            close(relative.u.hypot(relative.v), radius.0)
                && angle_in_sweep(
                    relative.v.atan2(relative.u),
                    start_angle.0,
                    end_angle.0,
                    1.0e-9,
                )
        }
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle,
            end_angle,
        } => {
            if major_radius.0 <= 0.0 || minor_radius.0 <= 0.0 {
                return false;
            }
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            let (sin, cos) = major_angle.0.sin_cos();
            let x = relative.u.mul_add(cos, relative.v * sin) / major_radius.0;
            let y = (-relative.u).mul_add(sin, relative.v * cos) / minor_radius.0;
            close(x.mul_add(x, y * y), 1.0)
                && match (start_angle, end_angle) {
                    (Some(start), Some(end)) => angle_in_sweep(y.atan2(x), start.0, end.0, 1.0e-9),
                    (None, None) => true,
                    _ => false,
                }
        }
        SketchGeometry::Hyperbola {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_parameter,
            end_parameter,
        } => {
            if major_radius.0 <= 0.0 || minor_radius.0 <= 0.0 {
                return false;
            }
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            let (sin, cos) = major_angle.0.sin_cos();
            let x = relative.u.mul_add(cos, relative.v * sin) / major_radius.0;
            let y = (-relative.u).mul_add(sin, relative.v * cos) / minor_radius.0;
            let parameter = y.asinh();
            close(x, parameter.cosh())
                && match (start_parameter, end_parameter) {
                    (Some(start), Some(end)) => {
                        parameter >= *start - 1.0e-9 && parameter <= *end + 1.0e-9
                    }
                    (None, None) => true,
                    _ => false,
                }
        }
        SketchGeometry::Parabola {
            vertex,
            axis_angle,
            focal_length,
            start_parameter,
            end_parameter,
        } => {
            if focal_length.0 <= 0.0 {
                return false;
            }
            let relative = Point2::new(point.u - vertex.u, point.v - vertex.v);
            let (sin, cos) = axis_angle.0.sin_cos();
            let x = relative.u.mul_add(cos, relative.v * sin);
            let y = (-relative.u).mul_add(sin, relative.v * cos);
            let parameter = y / (2.0 * focal_length.0);
            close(x, focal_length.0 * parameter * parameter)
                && match (start_parameter, end_parameter) {
                    (Some(start), Some(end)) => {
                        parameter >= *start - 1.0e-9 && parameter <= *end + 1.0e-9
                    }
                    (None, None) => true,
                    _ => false,
                }
        }
        SketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic: false,
        } => {
            let tolerance = 1.0e-9 * (1.0 + point.u.abs().max(point.v.abs()));
            cadmpeg_ir::eval::nurbs_pcurve_contains_point(
                *degree,
                knots,
                control_points,
                weights.as_deref(),
                point,
                tolerance,
            )
            .unwrap_or(false)
        }
        SketchGeometry::Nurbs { periodic: true, .. }
        | SketchGeometry::Text { .. }
        | SketchGeometry::Native { .. } => false,
    }
}

pub(crate) fn exact_counted_offset(
    loci: &[(u32, u32)],
    return_members: &[u32],
    entities: &HashMap<u32, &cadmpeg_ir::sketches::SketchEntity>,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::features::Length;
    use cadmpeg_ir::sketches::{SketchConstraintDefinition as Definition, SketchOffsetPair};

    if loci.len() != entities.len()
        || loci.len() != return_members.len()
        || loci.len() < 2
        || !loci.len().is_multiple_of(2)
        || !return_members.len().is_multiple_of(2)
    {
        return None;
    }
    let source_count = loci.iter().position(|(_, role)| *role == 0)?;
    if source_count == 0
        || source_count * 2 != loci.len()
        || loci[..source_count].iter().any(|(_, role)| *role == 0)
        || loci[source_count..].iter().any(|(_, role)| *role != 0)
    {
        return None;
    }
    let roles = loci.iter().copied().collect::<HashMap<_, _>>();
    if roles.len() != loci.len() {
        return None;
    }
    let mut used_members = HashSet::new();
    let mut pairs = Vec::with_capacity(source_count);
    let mut canonical_distance: Option<f64> = None;
    for members in return_members.chunks_exact(2) {
        let [source_record_index, result_record_index] = members else {
            unreachable!("chunks_exact(2) always yields pairs")
        };
        if roles.get(source_record_index).copied()? == 0
            || roles.get(result_record_index).copied()? != 0
            || !used_members.insert(*source_record_index)
            || !used_members.insert(*result_record_index)
        {
            return None;
        }
        let source = entities.get(source_record_index)?;
        let result = entities.get(result_record_index)?;
        let distance = parallel_line_offset(&source.geometry, &result.geometry)?;
        if distance.abs() <= 1.0e-9 {
            return None;
        }
        let source_reversed = offset_source_reversed(distance, &mut canonical_distance)?;
        pairs.push(SketchOffsetPair {
            source: source.id.clone(),
            result: result.id.clone(),
            source_reversed,
        });
    }
    if used_members.len() != loci.len() {
        return None;
    }
    Some(Definition::Offset {
        pairs,
        distance: Length(canonical_distance?),
        parameter: None,
        parameter_factor: None,
    })
}

pub(crate) fn offset_parameter_factor(distance: f64, parameter_value: f64) -> Option<f64> {
    let scale = 1.0 + distance.abs().max(parameter_value.abs());
    (distance.is_finite()
        && parameter_value.is_finite()
        && (distance - parameter_value.abs()).abs() <= scale * 1.0e-9)
        .then(|| {
            if parameter_value.is_sign_positive() {
                1.0
            } else {
                -1.0
            }
        })
}

pub(crate) fn line_angle_matches(
    first: &cadmpeg_ir::sketches::SketchGeometry,
    second: &cadmpeg_ir::sketches::SketchGeometry,
    expected: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    let SketchGeometry::Line {
        start: first_start,
        end: first_end,
    } = first
    else {
        return false;
    };
    let SketchGeometry::Line {
        start: second_start,
        end: second_end,
    } = second
    else {
        return false;
    };
    let first_du = first_end.u - first_start.u;
    let first_dv = first_end.v - first_start.v;
    let second_du = second_end.u - second_start.u;
    let second_dv = second_end.v - second_start.v;
    let denominator = first_du.hypot(first_dv) * second_du.hypot(second_dv);
    if denominator <= 1.0e-18 {
        return false;
    }
    let cosine = ((first_du * second_du + first_dv * second_dv) / denominator).clamp(-1.0, 1.0);
    let angle = cosine.acos();
    let supplementary = std::f64::consts::PI - angle;
    let scale = 1.0 + expected.abs();
    (angle - expected).abs() <= scale * 1.0e-9 || (supplementary - expected).abs() <= scale * 1.0e-9
}

pub(crate) fn exact_offset_constraint(
    relation: &SketchRelation,
    scope: &str,
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinition> {
    use cadmpeg_ir::features::Length;
    use cadmpeg_ir::sketches::{SketchConstraintDefinition as Definition, SketchOffsetPair};

    if relation.unknown_constraint_bits != 0
        || !matches!(
            relation.constraint_kinds.as_slice(),
            [SketchConstraintKind::Perpendicular | SketchConstraintKind::Offset]
        )
        || relation.return_members.len() < 4
        || !relation.return_members.len().is_multiple_of(2)
        || relation.return_members.len() != relation.members.len()
        || relation.resolved_return_members.len() != relation.return_members.len()
    {
        return None;
    }
    let offset_members = if relation.constraint_kinds == [SketchConstraintKind::Offset] {
        let source_count = relation.members.len() / 2;
        if !relation.members.len().is_multiple_of(2)
            || relation.member_roles.len() != relation.members.len()
            || source_count == 0
            || relation.member_roles[..source_count].contains(&1)
            || relation.member_roles[source_count..]
                .iter()
                .any(|role| *role != 1)
        {
            return None;
        }
        let sources = relation.members[..source_count]
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        let results = relation.members[source_count..]
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        if sources.len() != source_count || results.len() != source_count {
            return None;
        }
        Some((sources, results))
    } else {
        None
    };
    let mut pairs = Vec::new();
    let mut used_entities = HashSet::new();
    let mut canonical_distance: Option<f64> = None;
    for operands in relation.resolved_return_members.chunks_exact(2) {
        let (first_record_index, first_secondary_id, second_record_index, second_secondary_id) =
            match operands {
                [SketchRelationOperand::Curve {
                    record_index: first_record_index,
                    secondary_id: first_secondary_id,
                    ..
                }, SketchRelationOperand::Curve {
                    record_index: second_record_index,
                    secondary_id: second_secondary_id,
                    ..
                }] => (
                    *first_record_index,
                    *first_secondary_id,
                    *second_record_index,
                    *second_secondary_id,
                ),
                _ => return None,
            };
        let (source_record_index, result_record_index) =
            if let Some((sources, results)) = &offset_members {
                if sources.contains(&first_record_index) && results.contains(&second_record_index) {
                    (first_record_index, second_record_index)
                } else {
                    return None;
                }
            } else if first_secondary_id == 0 && second_secondary_id != 0 {
                (first_record_index, second_record_index)
            } else {
                return None;
            };
        let source = projected.get(&(scope, source_record_index))?;
        let result = projected.get(&(scope, result_record_index))?;
        if !used_entities.insert(source.id.clone()) || !used_entities.insert(result.id.clone()) {
            return None;
        }
        let distance = parallel_line_offset(&source.geometry, &result.geometry)?;
        if distance.abs() <= 1.0e-9 {
            return None;
        }
        let source_reversed = offset_source_reversed(distance, &mut canonical_distance)?;
        pairs.push(SketchOffsetPair {
            source: source.id.clone(),
            result: result.id.clone(),
            source_reversed,
        });
    }
    Some(Definition::Offset {
        pairs,
        distance: Length(canonical_distance?),
        parameter: None,
        parameter_factor: None,
    })
}

fn offset_source_reversed(distance: f64, canonical: &mut Option<f64>) -> Option<bool> {
    let magnitude = distance.abs();
    let Some(expected) = *canonical else {
        *canonical = Some(magnitude);
        return Some(distance.is_sign_negative());
    };
    let scale = 1.0 + magnitude.max(expected);
    if (magnitude - expected).abs() > scale * 1.0e-9 {
        return None;
    }
    Some(distance.is_sign_negative())
}

fn parallel_line_offset(
    source: &cadmpeg_ir::sketches::SketchGeometry,
    result: &cadmpeg_ir::sketches::SketchGeometry,
) -> Option<f64> {
    use cadmpeg_ir::sketches::SketchGeometry;

    let SketchGeometry::Line {
        start: source_start,
        end: source_end,
    } = source
    else {
        return None;
    };
    let SketchGeometry::Line {
        start: result_start,
        end: result_end,
    } = result
    else {
        return None;
    };
    let source_du = source_end.u - source_start.u;
    let source_dv = source_end.v - source_start.v;
    let result_du = result_end.u - result_start.u;
    let result_dv = result_end.v - result_start.v;
    let source_length = source_du.hypot(source_dv);
    let result_length = result_du.hypot(result_dv);
    if source_length <= 1.0e-12 || result_length <= 1.0e-12 {
        return None;
    }
    let parallel_error =
        (source_du * result_dv - source_dv * result_du).abs() / (source_length * result_length);
    if parallel_error > 1.0e-9 {
        return None;
    }
    let normal_u = -source_dv / source_length;
    let normal_v = source_du / source_length;
    let distance_at = |point: &Point2| {
        (point.u - source_start.u) * normal_u + (point.v - source_start.v) * normal_v
    };
    let start_distance = distance_at(result_start);
    let end_distance = distance_at(result_end);
    let scale = 1.0 + start_distance.abs().max(end_distance.abs());
    ((start_distance - end_distance).abs() <= scale * 1.0e-9).then_some(start_distance)
}

fn reflected_symmetry<'a>(
    entities: &[&'a cadmpeg_ir::sketches::SketchEntity],
) -> Option<(
    &'a cadmpeg_ir::sketches::SketchEntity,
    &'a cadmpeg_ir::sketches::SketchEntity,
    &'a cadmpeg_ir::sketches::SketchEntity,
)> {
    use cadmpeg_ir::sketches::SketchGeometry;

    if entities.len() != 3
        || entities
            .iter()
            .map(|entity| &entity.id)
            .collect::<HashSet<_>>()
            .len()
            != 3
    {
        return None;
    }
    let mut candidates = Vec::new();
    for axis_ordinal in 0..entities.len() {
        let axis = entities[axis_ordinal];
        let SketchGeometry::Line {
            start: axis_start,
            end: axis_end,
        } = &axis.geometry
        else {
            continue;
        };
        let others = entities
            .iter()
            .enumerate()
            .filter(|(ordinal, _)| *ordinal != axis_ordinal)
            .map(|(_, entity)| *entity)
            .collect::<Vec<_>>();
        if reflected_geometry_matches(
            &others[0].geometry,
            &others[1].geometry,
            axis_start,
            axis_end,
        ) {
            candidates.push((others[0], others[1], axis));
        }
    }
    (candidates.len() == 1).then(|| candidates.remove(0))
}

fn reflected_geometry_matches(
    first: &cadmpeg_ir::sketches::SketchGeometry,
    second: &cadmpeg_ir::sketches::SketchGeometry,
    axis_start: &Point2,
    axis_end: &Point2,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    match (first, second) {
        (
            SketchGeometry::Point {
                position: first_position,
            },
            SketchGeometry::Point {
                position: second_position,
            },
        ) => reflect_point(*first_position, *axis_start, *axis_end)
            .is_some_and(|reflected| sketch_points_close(reflected, *second_position)),
        (
            SketchGeometry::Line {
                start: first_start,
                end: first_end,
            },
            SketchGeometry::Line {
                start: second_start,
                end: second_end,
            },
        ) => {
            let Some(reflected_start) = reflect_point(*first_start, *axis_start, *axis_end) else {
                return false;
            };
            let Some(reflected_end) = reflect_point(*first_end, *axis_start, *axis_end) else {
                return false;
            };
            sketch_points_close(reflected_start, *second_start)
                && sketch_points_close(reflected_end, *second_end)
                || sketch_points_close(reflected_start, *second_end)
                    && sketch_points_close(reflected_end, *second_start)
        }
        _ => false,
    }
}

fn reflect_point(point: Point2, axis_start: Point2, axis_end: Point2) -> Option<Point2> {
    let du = axis_end.u - axis_start.u;
    let dv = axis_end.v - axis_start.v;
    let norm_squared = du * du + dv * dv;
    (norm_squared > 1.0e-18).then(|| {
        let projection =
            ((point.u - axis_start.u) * du + (point.v - axis_start.v) * dv) / norm_squared;
        Point2::new(
            2.0 * (axis_start.u + projection * du) - point.u,
            2.0 * (axis_start.v + projection * dv) - point.v,
        )
    })
}

fn sketch_points_close(first: Point2, second: Point2) -> bool {
    let scale = 1.0
        + first
            .u
            .abs()
            .max(first.v.abs())
            .max(second.u.abs())
            .max(second.v.abs());
    (first.u - second.u).abs() <= scale * 1.0e-9 && (first.v - second.v).abs() <= scale * 1.0e-9
}

pub(crate) fn relation_kind_name(relation: &SketchRelation) -> String {
    let mut names = relation
        .constraint_kinds
        .iter()
        .map(|kind| match kind {
            SketchConstraintKind::Coincident => "coincident",
            SketchConstraintKind::Colinear => "collinear",
            SketchConstraintKind::Concentric => "concentric",
            SketchConstraintKind::EqualLength => "equal_length",
            SketchConstraintKind::Parallel => "parallel",
            SketchConstraintKind::Perpendicular => "perpendicular",
            SketchConstraintKind::Horizontal => "horizontal",
            SketchConstraintKind::Vertical => "vertical",
            SketchConstraintKind::Tangent => "tangent",
            SketchConstraintKind::Curvature => "curvature",
            SketchConstraintKind::Symmetry => "symmetry",
            SketchConstraintKind::Equal => "equal",
            SketchConstraintKind::Midpoint => "midpoint",
            SketchConstraintKind::Polygon => "polygon",
            SketchConstraintKind::Offset => "offset",
            SketchConstraintKind::SplineGroup => "spline_group",
            SketchConstraintKind::CircularPattern => "circular_pattern",
            SketchConstraintKind::RectangularPattern => "rectangular_pattern",
            SketchConstraintKind::TextFrame => "text_frame",
            SketchConstraintKind::TextPath => "text_path",
        })
        .collect::<Vec<_>>();
    if relation.unknown_constraint_bits != 0 {
        names.push("unknown_bits");
    }
    names.join("+")
}

pub(crate) fn planar_point(point: &Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite() && point.z.abs() <= 1.0e-9
}

pub(crate) fn sketch_normal_sign(normal: &Vector3) -> Option<f64> {
    (normal.x.abs() <= 1.0e-9 && normal.y.abs() <= 1.0e-9 && (normal.z.abs() - 1.0).abs() <= 1.0e-9)
        .then_some(normal.z.signum())
}

pub(crate) fn expression_identifiers(expression: &str) -> impl Iterator<Item = String> {
    let identifier_character = |character: char| {
        character.is_alphanumeric() || matches!(character, '_' | '"' | '$' | '°' | 'µ')
    };
    let mut identifiers = Vec::new();
    let mut start = None;
    for (offset, character) in expression
        .char_indices()
        .chain(std::iter::once((expression.len(), '\0')))
    {
        if identifier_character(character) {
            start.get_or_insert(offset);
            continue;
        }
        let Some(token_start) = start.take() else {
            continue;
        };
        let token = &expression[token_start..offset];
        if !token
            .chars()
            .next()
            .is_some_and(|character| character.is_alphabetic() || character == '_')
        {
            continue;
        }
        let next = expression[offset..]
            .chars()
            .find(|character| !character.is_whitespace());
        if next == Some('(') {
            continue;
        }
        let previous = expression[..token_start]
            .chars()
            .rev()
            .find(|character| !character.is_whitespace());
        if matches!(token, "mm" | "cm" | "m" | "in" | "ft" | "deg" | "rad")
            && previous.is_some_and(|character| character.is_ascii_digit() || character == ')')
        {
            continue;
        }
        identifiers.push(token.to_owned());
    }
    identifiers.into_iter()
}

/// Count decoded same-stream parameter-name symbols that have no neutral
/// dependency edge.
pub(crate) fn unresolved_parameter_expression_dependency_count(
    native: &[DesignParameter],
    projected: &[cadmpeg_ir::features::DesignParameter],
) -> usize {
    let projected_by_native_ref = projected
        .iter()
        .filter_map(|parameter| Some((parameter.native_ref.as_deref()?, parameter)))
        .collect::<HashMap<_, _>>();
    let projected_by_id = projected
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let mut names_by_stream = HashMap::<&str, HashSet<&str>>::new();
    for parameter in native {
        let Some(stream) = native_stream(&parameter.id) else {
            continue;
        };
        names_by_stream
            .entry(stream)
            .or_default()
            .insert(parameter.name.as_str());
    }

    native
        .iter()
        .filter_map(|parameter| {
            let stream = native_stream(&parameter.id)?;
            let names = names_by_stream.get(stream)?;
            let projected = projected_by_native_ref.get(parameter.id.as_str())?;
            let dependency_names = projected
                .dependencies
                .iter()
                .filter_map(|dependency| projected_by_id.get(dependency))
                .map(|dependency| dependency.name.as_str())
                .collect::<HashSet<_>>();
            Some(
                expression_identifiers(&parameter.expression)
                    .filter(|identifier| names.contains(identifier.as_str()))
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .filter(|identifier| !dependency_names.contains(identifier.as_str()))
                    .count(),
            )
        })
        .sum()
}

pub(crate) fn json_scalar_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        value => value.to_string(),
    }
}
