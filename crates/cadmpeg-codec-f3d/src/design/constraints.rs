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
    parameters::DesignParameter,
    sketch_geometry::{SketchCurveIdentity, SketchPoint, SketchText},
    sketch_placement::DesignSketchPlacement,
    sketch_relations::{SketchConstraintKind, SketchRelation},
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};
use cadmpeg_ir::math::Point2;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

const EPS_CONSTRAINTS_EXACT_RECTANGULAR_PATTERN_E9: f64 = 1.0e-9;
const EPS_CONSTRAINTS_SCALAR_CLOSE_E9: f64 = 1.0e-9;

fn insert_constraint_index<K: Eq + Hash, V>(
    ctx: Option<&DecodeContext<'_>>,
    index: &mut HashMap<K, V>,
    key: K,
    value: V,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !index.contains_key(&key) {
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, operation)?;
            index.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
        }
    }
    // discarded-value: duplicate native keys keep the last record.
    let _ = index.insert(key, value);
    Ok(())
}

fn push_constraint_item<T>(
    ctx: Option<&DecodeContext<'_>>,
    items: &mut Vec<T>,
    item: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(1, operation)?;
        items.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    }
    items.push(item);
    Ok(())
}

fn copy_constraint_text(
    ctx: Option<&DecodeContext<'_>>,
    value: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    match ctx {
        Some(ctx) => String::from_utf8(ctx.copy_retained(value.as_bytes(), operation)?)
            .map_err(|_| CodecError::malformed("validated constraint text is not UTF-8")),
        None => Ok(value.to_owned()),
    }
}

fn copy_constraint_id<T>(
    ctx: Option<&DecodeContext<'_>>,
    value: &str,
    operation: &'static str,
) -> Result<T, CodecError>
where
    T: TryFrom<String>,
    T::Error: std::fmt::Display,
{
    T::try_from(copy_constraint_text(ctx, value, operation)?).map_err(CodecError::malformed)
}

/// Project each native relation as an exact atomic constraint or an explicitly
/// native aggregate when its semantic members do not prove neutral loci.
pub(crate) fn project_sketch_constraints(
    ctx: Option<&DecodeContext<'_>>,
    placements: &[DesignSketchPlacement],
    parameters: &[DesignParameter],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    texts: &[SketchText],
    relations: &[SketchRelation],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Result<Vec<cadmpeg_ir::sketches::SketchConstraint>, CodecError> {
    use cadmpeg_ir::sketches::{
        NativeOperandField, SketchConstraint, SketchConstraintDefinitionInput as Definition,
        SketchNativeOperand,
    };

    let mut sketches = HashMap::new();
    for placement in placements {
        let id = neutral_sketch_id(placement);
        if let Some(ctx) = ctx {
            let work = u64::try_from(entities.len())
                .map_err(|_| ctx.refuse_codec_limit("f3d planar sketch admission scan", 0, 1))?;
            ctx.charge_work(work, "f3d planar sketch admission scan")?;
        }
        if !entities.iter().any(|entity| entity.sketch == id) {
            continue;
        }
        let (Some(scope), Ok(entity_id)) = (
            native_stream(&placement.id),
            u32::try_from(placement.entity_id.suffix()),
        ) else { continue; };
        insert_constraint_index(ctx, &mut sketches, (scope, entity_id), id,
            "f3d sketch constraint placement index")?;
    }
    let mut record_keys_by_native_ref = HashMap::new();
    for point in points {
        if let Some(scope) = native_stream(&point.id) {
            insert_constraint_index(ctx, &mut record_keys_by_native_ref,
                point.id.as_str(), (scope, point.record_index),
                "f3d sketch constraint native record key")?;
        }
    }
    for curve in curves {
        if let Some(scope) = native_stream(&curve.id) {
            insert_constraint_index(ctx, &mut record_keys_by_native_ref,
                curve.id.as_str(), (scope, curve.record_index),
                "f3d sketch constraint native record key")?;
        }
    }
    for text in texts {
        if let Some(scope) = native_stream(&text.id) {
            insert_constraint_index(ctx, &mut record_keys_by_native_ref,
                text.id.as_str(), (scope, text.record_index),
                "f3d sketch constraint native record key")?;
        }
    }
    let mut projected = HashMap::new();
    for entity in entities {
        if let Some(key) = entity.native_ref.as_deref()
            .and_then(|native_ref| record_keys_by_native_ref.get(native_ref).copied())
        {
            insert_constraint_index(ctx, &mut projected, key, entity,
                "f3d sketch constraint projected entity index")?;
        }
    }
    let mut point_native_refs = HashMap::new();
    for point in points {
        if let Some(scope) = native_stream(&point.id) {
            insert_constraint_index(ctx, &mut point_native_refs,
                (scope, point.record_index), point.id.as_str(),
                "f3d sketch constraint point reference index")?;
        }
    }
    let mut curve_native_refs = HashMap::new();
    for curve in curves {
        if let Some(scope) = native_stream(&curve.id) {
            insert_constraint_index(ctx, &mut curve_native_refs,
                (scope, curve.record_index), curve.id.as_str(),
                "f3d sketch constraint curve reference index")?;
        }
    }
    let mut text_native_refs = HashMap::new();
    for text in texts {
        if let Some(scope) = native_stream(&text.id) {
            insert_constraint_index(ctx, &mut text_native_refs,
                (scope, text.record_index), text.id.as_str(),
                "f3d sketch constraint text reference index")?;
        }
    }
    let native_operand = |scope: &str,
                          field: cadmpeg_core::text::NonBlankString,
                          record_index: u32| -> Result<SketchNativeOperand, CodecError> {
        let (family, native_ref) = if let Some(native_ref) =
            point_native_refs.get(&(scope, record_index)).copied()
        {
            (cadmpeg_core::nonblank_literal!("point"), Some(native_ref))
        } else if let Some(native_ref) = curve_native_refs.get(&(scope, record_index)).copied() {
            (cadmpeg_core::nonblank_literal!("curve"), Some(native_ref))
        } else if let Some(native_ref) = text_native_refs.get(&(scope, record_index)).copied() {
            (cadmpeg_core::nonblank_literal!("text"), Some(native_ref))
        } else {
            (cadmpeg_core::nonblank_literal!("record"), None)
        };
        Ok(SketchNativeOperand {
            native_kind: family,
            field: Some(NativeOperandField {
                name: field,
                role: None,
            }),
            object_index: Some(record_index),
            native_ref: native_ref
                .filter(|_| !projected.contains_key(&(scope, record_index)))
                .map(|value| copy_constraint_text(ctx, value,
                    "f3d sketch constraint operand native reference"))
                .transpose()?,
        })
    };

    let project_relation = |relation: &SketchRelation| -> Result<Option<SketchConstraint>, CodecError> {
        let Some(scope) = native_stream(&relation.id) else { return Ok(None); };
        let Some(sketch) = sketches.get(&(scope, relation.owner_reference)) else {
            return Ok(None);
        };
        let sketch = copy_constraint_id(ctx, sketch.as_str(),
            "f3d sketch constraint sketch id")?;
        let mut input_entities = Vec::new();
        for member in relation.members().iter() {
            if let Some(entity) = projected.get(&(scope, member.reference.record_index())) {
                push_constraint_item(ctx, &mut input_entities, *entity,
                    "f3d sketch constraint input entity")?;
            }
        }
        // The second reference run is the relation's semantic member order.
        // The interleaved first run is retained separately because
        // circular-pattern decoding verifies both reference sets before using
        // the semantic order.
        let mut semantic_entities = Vec::new();
        for member in relation.return_members().iter() {
            if let Some(entity) = projected.get(&(scope, member.reference.record_index())) {
                push_constraint_item(ctx, &mut semantic_entities, *entity,
                    "f3d sketch constraint semantic entity")?;
            }
        }
        let sole_kind = relation
            .sole_constraint_kind()
            .filter(|_| semantic_entities.len() == relation.return_members().len());
        let definition = (if let Some(kind) = sole_kind {
            let loci = if kind == SketchConstraintKind::Coincident {
                exact_coincident_loci(&semantic_entities, ctx)?
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
        .or_else(|| exact_offset_constraint(relation, scope, &projected));
        let definition = match definition {
            Some(definition) => Some(definition),
            None => exact_text_relation(relation, scope, &projected, ctx)?,
        };
        let definition = if let Some(definition) = definition {
            definition
        } else {
            let Some(native_kind) = cadmpeg_core::text::NonBlankString::new(relation_kind_name(relation)) else {
                return Ok(None);
            };
            let member_indices = relation.members().iter().map(|member| member.reference.record_index());
            let auxiliary_indices = relation.auxiliary_references().values().copied();
            let return_indices = relation.return_members().iter().map(|member| member.reference.record_index());
            let mut native_entities = Vec::new();
            for record_index in member_indices.chain(auxiliary_indices).chain(return_indices) {
                if let Some(entity) = projected.get(&(scope, record_index)) {
                    let id = copy_constraint_id(ctx, entity.id().as_str(),
                        "f3d sketch constraint native entity id")?;
                    push_constraint_item(ctx, &mut native_entities, id,
                        "f3d sketch constraint native entity")?;
                }
            }
            let mut operands = Vec::new();
            for member in relation.members().iter() {
                let operand = native_operand(scope, cadmpeg_core::nonblank_literal!("member"),
                    member.reference.record_index())?;
                push_constraint_item(ctx, &mut operands, operand,
                    "f3d sketch constraint native operand")?;
            }
            for record_index in relation.auxiliary_references().values() {
                let operand = native_operand(scope, cadmpeg_core::nonblank_literal!("auxiliary"),
                    *record_index)?;
                push_constraint_item(ctx, &mut operands, operand,
                    "f3d sketch constraint native operand")?;
            }
            for member in relation.return_members().iter() {
                let operand = native_operand(scope, cadmpeg_core::nonblank_literal!("return"),
                    member.reference.record_index())?;
                push_constraint_item(ctx, &mut operands, operand,
                    "f3d sketch constraint native operand")?;
            }
            Definition::Native {
                native_kind,
                native_state: Some(relation.definition.state()),
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
                entities: native_entities,
                parameter: None,
                operands,
            }
        };
        let Some(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition).ok() else {
            return Ok(None);
        };
        Ok(Some(SketchConstraint {
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
            native_ref: Some(copy_constraint_id(ctx, relation.id.as_str(),
                "f3d sketch constraint native reference")?),
        }))
    };
    let mut constraints = Vec::new();
    for relation in relations {
        if let Some(constraint) = project_relation(relation)? {
            push_constraint_item(ctx, &mut constraints, constraint,
                "f3d projected sketch constraint")?;
        }
    }
    constraints.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(constraints)
}

struct RectangularPatternSourceDirection {
    direction: [f64; 2],
    count: u32,
    distance: cadmpeg_ir::scalar::NonNegativeLength,
    distance_parameter: Option<cadmpeg_ir::features::ParameterId>,
    count_parameter: Option<cadmpeg_ir::features::ParameterId>,
}

#[derive(Clone, Copy)]
enum RectangularPatternDistanceForm {
    AdjacentSpacing,
    SeedToFinalSpan,
}

fn exact_rectangular_pattern(
    relation: &SketchRelation,
    scope: &str,
    parameters: &[DesignParameter],
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use crate::records::sketch_relations::SketchPatternDefinition;
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
            let source_direction = direction.direction.get();
            if source_direction[2].abs() > EPS_CONSTRAINTS_EXACT_RECTANGULAR_PATTERN_E9 {
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
                !scalar_close(parameter.evaluated_value().get(), f64::from(count))
            }) {
                return None;
            }
            let distance = cadmpeg_ir::scalar::NonNegativeLength::new(
                direction.evaluated_distance.get() * 10.0,
            )?;
            if (count == 1 && !scalar_close(distance.get(), 0.0))
                || distance_parameter.is_some_and(|parameter| {
                    design_length(parameter)
                        .is_none_or(|value| !scalar_close(value.get(), distance.get()))
                })
            {
                return None;
            }
            Some(RectangularPatternSourceDirection {
                direction: [source_direction[0], source_direction[1]],
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
                RectangularPatternDistanceForm::AdjacentSpacing => source.distance.get(),
                RectangularPatternDistanceForm::SeedToFinalSpan => {
                    if source.count > 1 {
                        source.distance.get() / f64::from(source.count - 1)
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
                            * directions[0].direction().get()[0]
                            + f64::from(indices[1])
                                * directions[1].spacing().get()
                                * directions[1].direction().get()[0],
                        f64::from(indices[0])
                            * directions[0].spacing().get()
                            * directions[0].direction().get()[1]
                            + f64::from(indices[1])
                                * directions[1].spacing().get()
                                * directions[1].direction().get()[1],
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

fn exact_text_relation(
    relation: &SketchRelation,
    scope: &str,
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput>, CodecError> {
    use crate::records::sketch_relations::SketchPatternDefinition;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };
    use cadmpeg_ir::transform::Transform;

    if relation.sole_constraint_kind().is_none() {
        return Ok(None);
    }
    let pattern = relation.definition.pattern();
    Ok(match pattern {
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
                && relation.return_members().iter().map(|member| member.reference.record_index())
                    .eq(relation.members().iter().skip(1).map(|member| member.reference.record_index())) =>
        {
            let Some(text) = projected.get(&(scope, *text_reference)) else { return Ok(None); };
            if !matches!(
                *text.geometry.definition(),
                SketchGeometryDefinition::Text { .. }
            ) {
                return Ok(None);
            }
            let mut frame = Vec::new();
            for member in relation.return_members().iter() {
                let Some(entity) = projected.get(&(scope, member.reference.record_index())) else {
                    return Ok(None);
                };
                if entity.id() == text.id()
                    || matches!(*entity.geometry.definition(), SketchGeometryDefinition::Text { .. })
                {
                    return Ok(None);
                }
                let id = copy_constraint_id(ctx, entity.id().as_str(),
                    "f3d sketch constraint text frame entity id")?;
                push_constraint_item(ctx, &mut frame, id,
                    "f3d sketch constraint text frame entity")?;
            }
            if frame.is_empty() { return Ok(None); }
            Some(Definition::TextFrame {
                text: copy_constraint_id(ctx, text.id().as_str(),
                    "f3d sketch constraint text frame text id")?,
                frame,
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
            && relation.return_members().iter().map(|member| member.reference.record_index())
                .eq([relation.members()[0].reference.record_index()]) =>
        {
            let Some(path) = projected.get(&(scope, relation.members()[0].reference.record_index())) else {
                return Ok(None);
            };
            let Some(text) = projected.get(&(scope, *text_reference)) else { return Ok(None); };
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
                return Ok(None);
            }
            let mut glyphs = Vec::new();
            for source in glyph_transforms {
                let source = source.rows();
                if source[3] != [0.0, 0.0, 0.0, 1.0] {
                    return Ok(None);
                }
                let mut rows = [source[0], source[1], source[2]];
                for row in &mut rows {
                    row[3] *= 10.0;
                }
                let Some(glyph) = Transform::affine(rows) else { return Ok(None); };
                push_constraint_item(ctx, &mut glyphs, glyph,
                    "f3d sketch constraint text glyph transform")?;
            }
            Some(Definition::TextPath {
                text: copy_constraint_id(ctx, text.id().as_str(),
                    "f3d sketch constraint text path text id")?,
                path: copy_constraint_id(ctx, path.id().as_str(),
                    "f3d sketch constraint text path curve id")?,
                glyph_transforms: glyphs,
            })
        }
        _ => None,
    })
}

fn exact_circular_pattern(
    relation: &SketchRelation,
    scope: &str,
    parameters: &[DesignParameter],
    members: &[&cadmpeg_ir::sketches::SketchEntity],
    returned: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use crate::records::sketch_relations::SketchPatternDefinition;
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
    let angle = cadmpeg_ir::scalar::Angle::from_assigned_real(*evaluated_angle);
    if angle_parameter.is_some_and(|parameter| {
        design_angle(parameter).is_none_or(|value| !scalar_close(value.get(), angle.get()))
    }) || count_parameter.is_some_and(|parameter| {
        !scalar_close(
            parameter.evaluated_value().get(),
            f64::from(evaluated_count.get()),
        )
    }) {
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
        let center_position = center_position.get();
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
                    let rotation = evaluated_angle.get() * index as f64 / divisor;
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
        ) => point_matches(first.get(), second.get()),
        (
            SketchGeometryDefinition::Line { start: a, end: b },
            SketchGeometryDefinition::Line { start: c, end: d },
        ) => point_matches(a.get(), c.get()) && point_matches(b.get(), d.get()),
        (
            SketchGeometryDefinition::Circle {
                center: a,
                radius: ar,
            },
            SketchGeometryDefinition::Circle {
                center: b,
                radius: br,
            },
        ) => point_matches(a.get(), b.get()) && scalar_close(ar.get(), br.get()),
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
            point_matches(a.get(), b.get())
                && scalar_close(ar.get(), br.get())
                && angle_matches(as_.get(), bs.get())
                && angle_matches(ae.get(), be.get())
        }
        (
            SketchGeometryDefinition::Ellipse {
                center: a,
                major_angle: aa,
                radii: ar,
                bounds: ab,
            },
            SketchGeometryDefinition::Ellipse {
                center: b,
                major_angle: ba,
                radii: br,
                bounds: bb,
            },
        ) => {
            point_matches(a.get(), b.get())
                && angle_matches(aa.get(), ba.get())
                && scalar_close(ar.major().get(), br.major().get())
                && scalar_close(ar.minor().get(), br.minor().get())
                && optional_angle_bounds_match(ab.as_ref(), bb.as_ref())
        }
        (
            SketchGeometryDefinition::Nurbs { curve: first },
            SketchGeometryDefinition::Nurbs { curve: second },
        ) => {
            first.degree() == second.degree()
                && first.periodic() == second.periodic()
                && equal_scalars(first.knots(), second.knots())
                && nurbs_poles_match(first, second, point_matches)
        }
        _ => false,
    }
}

pub(super) fn scalar_close(first: f64, second: f64) -> bool {
    first.is_finite()
        && second.is_finite()
        && (first - second).abs()
            <= EPS_CONSTRAINTS_SCALAR_CLOSE_E9 * (1.0 + first.abs().max(second.abs()))
}

fn translated_sketch_geometry_matches(
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
        ) => point_matches(first.get(), second.get()),
        (
            SketchGeometryDefinition::Line {
                start: first_start,
                end: first_end,
            },
            SketchGeometryDefinition::Line {
                start: second_start,
                end: second_end,
            },
        ) => {
            point_matches(first_start.get(), second_start.get())
                && point_matches(first_end.get(), second_end.get())
        }
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
            point_matches(first_center.get(), second_center.get())
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
            point_matches(first_center.get(), second_center.get())
                && scalar_close(first_radius.get(), second_radius.get())
                && scalar_close(first_start.get(), second_start.get())
                && scalar_close(first_end.get(), second_end.get())
        }
        (
            SketchGeometryDefinition::Ellipse {
                center: first_center,
                major_angle: first_major_angle,
                radii: first_radii,
                bounds: first_bounds,
            },
            SketchGeometryDefinition::Ellipse {
                center: second_center,
                major_angle: second_major_angle,
                radii: second_radii,
                bounds: second_bounds,
            },
        ) => {
            point_matches(first_center.get(), second_center.get())
                && scalar_close(first_major_angle.get(), second_major_angle.get())
                && scalar_close(first_radii.major().get(), second_radii.major().get())
                && scalar_close(first_radii.minor().get(), second_radii.minor().get())
                && optional_angle_bounds_match(first_bounds.as_ref(), second_bounds.as_ref())
        }
        (
            SketchGeometryDefinition::Nurbs { curve: first },
            SketchGeometryDefinition::Nurbs { curve: second },
        ) => {
            first.degree() == second.degree()
                && first.periodic() == second.periodic()
                && equal_scalars(first.knots(), second.knots())
                && nurbs_poles_match(first, second, point_matches)
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

fn nurbs_poles_match(
    first: &PcurveNurbs,
    second: &PcurveNurbs,
    point_matches: impl Fn(Point2, Point2) -> bool,
) -> bool {
    match (first.pole_rows(), second.pole_rows()) {
        (PcurveNurbsPoles::Polynomial { points: first },
         PcurveNurbsPoles::Polynomial { points: second }) => {
            first.len() == second.len()
                && first.iter().zip(second).all(|(a, b)| point_matches(a.get(), b.get()))
        }
        (PcurveNurbsPoles::Rational { points: first },
         PcurveNurbsPoles::Rational { points: second }) => {
            first.len() == second.len()
                && first.iter().zip(second).all(|(a, b)| {
                    point_matches(a.point.get(), b.point.get())
                        && scalar_close(a.weight.get(), b.weight.get())
                })
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
