// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::needless_range_loop))]
//! Project sketch constraint relations.

use cadmpeg_core::convert::f64_from_index;

use crate::design::dimensions::{
    exact_atomic_constraint, exact_coincident_loci, exact_offset_constraint, relation_kind_name,
};
use crate::design::face_resolve::design_angle;
use crate::design::feature_project::design_length;
use crate::ids::native_stream_charged;
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
use std::collections::{BTreeSet, HashMap, HashSet};

const EPS_CONSTRAINTS_EXACT_RECTANGULAR_PATTERN_E9: f64 = 1.0e-9;
const EPS_CONSTRAINTS_SCALAR_CLOSE_E9: f64 = 1.0e-9;

/// Project each native relation as an exact atomic constraint or an explicitly
/// native aggregate when its semantic members do not prove neutral loci.
pub(crate) fn project_sketch_constraints(
    ctx: &DecodeContext<'_>,
    placements: &[DesignSketchPlacement],
    parameters: &[DesignParameter],
    (points, curves, texts): (&[SketchPoint], &[SketchCurveIdentity], &[SketchText]),
    relations: &[SketchRelation],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Result<Vec<cadmpeg_ir::sketches::SketchConstraint>, CodecError> {
    use cadmpeg_ir::sketches::{
        NativeOperandField, SketchConstraint, SketchConstraintDefinitionInput as Definition,
        SketchNativeOperand,
    };

    let mut lookup_storage = ctx.reserve_scoped(0, "f3d sketch constraint lookup storage")?;
    let (sketches, projected, point_native_refs, curve_native_refs, text_native_refs) =
        lookup_storage.with_storage(|| -> Result<_, CodecError> {
            let sketches = {
                let mut membership_storage =
                    ctx.reserve_scoped(0, "f3d planar sketch membership storage")?;
                let admitted_sketches =
                    membership_storage.with_storage(|| -> Result<_, CodecError> {
                        let mut admitted = HashSet::new();
                        for entity in
                            ctx.admit_iter(entities, "f3d planar sketch admission scan")?
                        {
                            ctx.insert_hash_set(
                                &mut admitted,
                                &entity.sketch,
                                "f3d planar sketch admission index",
                            )?;
                        }
                        Ok(admitted)
                    })?;
                let mut sketches = HashMap::new();
                for placement in
                    ctx.admit_iter(placements, "f3d sketch constraint placement scan")?
                {
                    let mut identity_storage =
                        ctx.reserve_scoped(0, "f3d planar admission identity storage")?;
                    let id = identity_storage.with_storage(|| {
                        crate::design::identity::neutral_sketch_id(ctx, placement)
                    })?;
                    let Some(admitted) = ctx
                        .get_hash_set(
                            &admitted_sketches,
                            &id,
                            "f3d planar sketch admission lookup",
                        )?
                        .copied()
                    else {
                        continue;
                    };
                    let (Some(scope), Ok(entity_id)) = (
                        native_stream_charged(ctx, &placement.id, "f3d constraint native stream")?,
                        u32::try_from(placement.entity_id.suffix()),
                    ) else {
                        continue;
                    };
                    ctx.insert_hash_map(
                        &mut sketches,
                        (scope, entity_id),
                        admitted,
                        "f3d sketch constraint placement index",
                    )?;
                }
                sketches
            };
            let mut record_storage = ctx.reserve_scoped(0, "f3d planar native record storage")?;
            let record_keys_by_native_ref =
                record_storage.with_storage(|| -> Result<_, CodecError> {
                    let mut record_keys_by_native_ref = HashMap::new();
                    for point in ctx.admit_iter(points, "f3d sketch constraint points scan")? {
                        if let Some(scope) =
                            native_stream_charged(ctx, &point.id, "f3d constraint native stream")?
                        {
                            ctx.insert_hash_map(
                                &mut record_keys_by_native_ref,
                                point.id.as_str(),
                                (scope, point.record_index),
                                "f3d sketch constraint native record key",
                            )
                            .map(|_| ())?;
                        }
                    }
                    for curve in ctx.admit_iter(curves, "f3d sketch constraint curves scan")? {
                        if let Some(scope) =
                            native_stream_charged(ctx, &curve.id, "f3d constraint native stream")?
                        {
                            ctx.insert_hash_map(
                                &mut record_keys_by_native_ref,
                                curve.id.as_str(),
                                (scope, curve.record_index),
                                "f3d sketch constraint native record key",
                            )
                            .map(|_| ())?;
                        }
                    }
                    for text in ctx.admit_iter(texts, "f3d sketch constraint texts scan")? {
                        if let Some(scope) =
                            native_stream_charged(ctx, &text.id, "f3d constraint native stream")?
                        {
                            ctx.insert_hash_map(
                                &mut record_keys_by_native_ref,
                                text.id.as_str(),
                                (scope, text.record_index),
                                "f3d sketch constraint native record key",
                            )
                            .map(|_| ())?;
                        }
                    }
                    Ok(record_keys_by_native_ref)
                })?;
            let mut projected = HashMap::new();
            for entity in ctx.admit_iter(entities, "f3d sketch constraint entities scan")? {
                let key = match entity.native_ref.as_deref() {
                    Some(native_ref) => ctx
                        .get_hash_map(
                            &record_keys_by_native_ref,
                            native_ref,
                            "f3d sketch constraint native record lookup",
                        )?
                        .copied(),
                    None => None,
                };
                if let Some(key) = key {
                    ctx.insert_hash_map(
                        &mut projected,
                        key,
                        entity,
                        "f3d sketch constraint projected entity index",
                    )
                    .map(|_| ())?;
                }
            }
            drop(record_keys_by_native_ref);
            drop(record_storage);
            let mut point_native_refs = HashMap::new();
            for point in ctx.admit_iter(points, "f3d sketch constraint points scan")? {
                if let Some(scope) =
                    native_stream_charged(ctx, &point.id, "f3d constraint native stream")?
                {
                    ctx.insert_hash_map(
                        &mut point_native_refs,
                        (scope, point.record_index),
                        point.id.as_str(),
                        "f3d sketch constraint point reference index",
                    )
                    .map(|_| ())?;
                }
            }
            let mut curve_native_refs = HashMap::new();
            for curve in ctx.admit_iter(curves, "f3d sketch constraint curves scan")? {
                if let Some(scope) =
                    native_stream_charged(ctx, &curve.id, "f3d constraint native stream")?
                {
                    ctx.insert_hash_map(
                        &mut curve_native_refs,
                        (scope, curve.record_index),
                        curve.id.as_str(),
                        "f3d sketch constraint curve reference index",
                    )
                    .map(|_| ())?;
                }
            }
            let mut text_native_refs = HashMap::new();
            for text in ctx.admit_iter(texts, "f3d sketch constraint texts scan")? {
                if let Some(scope) =
                    native_stream_charged(ctx, &text.id, "f3d constraint native stream")?
                {
                    ctx.insert_hash_map(
                        &mut text_native_refs,
                        (scope, text.record_index),
                        text.id.as_str(),
                        "f3d sketch constraint text reference index",
                    )
                    .map(|_| ())?;
                }
            }
            Ok((
                sketches,
                projected,
                point_native_refs,
                curve_native_refs,
                text_native_refs,
            ))
        })?;
    let native_operand = |scope: &str,
                          field: cadmpeg_core::text::NonBlankString,
                          record_index: u32|
     -> Result<SketchNativeOperand, CodecError> {
        let (family, native_ref) = if let Some(native_ref) = ctx
            .get_hash_map(
                &point_native_refs,
                &(scope, record_index),
                "f3d sketch constraint operand reference lookup",
            )?
            .copied()
        {
            (cadmpeg_core::nonblank_literal!("point"), Some(native_ref))
        } else if let Some(native_ref) = ctx
            .get_hash_map(
                &curve_native_refs,
                &(scope, record_index),
                "f3d sketch constraint operand reference lookup",
            )?
            .copied()
        {
            (cadmpeg_core::nonblank_literal!("curve"), Some(native_ref))
        } else if let Some(native_ref) = ctx
            .get_hash_map(
                &text_native_refs,
                &(scope, record_index),
                "f3d sketch constraint operand reference lookup",
            )?
            .copied()
        {
            (cadmpeg_core::nonblank_literal!("text"), Some(native_ref))
        } else {
            (cadmpeg_core::nonblank_literal!("record"), None)
        };
        let native_ref = if native_ref.is_some()
            && ctx.contains_key_hash_map(
                &projected,
                &(scope, record_index),
                "f3d sketch constraint operand entity lookup",
            )? {
            None
        } else {
            native_ref
        };
        Ok(SketchNativeOperand {
            native_kind: family,
            field: Some(NativeOperandField {
                name: field,
                role: None,
            }),
            object_index: Some(record_index),
            native_ref: native_ref
                .map(|value| {
                    ctx.copy_retained_text(value, "f3d sketch constraint operand native reference")
                })
                .transpose()?,
        })
    };

    let mut pattern_parameters = PatternParameters::new(ctx, parameters);
    let mut project_relation =
        |relation: &SketchRelation| -> Result<Option<SketchConstraint>, CodecError> {
            let Some(scope) =
                native_stream_charged(ctx, &relation.id, "f3d constraint native stream")?
            else {
                return Ok(None);
            };
            let Some(sketch) = ctx.get_hash_map(
                &sketches,
                &(scope, relation.owner_reference),
                "f3d sketch constraint owner lookup",
            )?
            else {
                return Ok(None);
            };
            let sketch = (sketch).try_clone_for_decode(ctx, "f3d sketch constraint sketch id")?;
            let mut entity_storage =
                ctx.reserve_scoped(0, "f3d sketch constraint member storage")?;
            let (input_entities, semantic_entities) =
                entity_storage.with_storage(|| -> Result<_, CodecError> {
                    let mut input_entities = Vec::new();
                    for member in ctx
                        .admit_iter(&relation.members()[..], "f3d sketch constraint member scan")?
                    {
                        if let Some(entity) = ctx.get_hash_map(
                            &projected,
                            &(scope, member.reference.record_index()),
                            "f3d sketch constraint member lookup",
                        )? {
                            ctx.push_vec(
                                &mut input_entities,
                                *entity,
                                "f3d sketch constraint input entity",
                            )?;
                        }
                    }
                    // The second reference run is the relation's semantic member order.
                    // The interleaved first run is retained separately because
                    // circular-pattern decoding verifies both reference sets before using
                    // the semantic order.
                    let mut semantic_entities = Vec::new();
                    for member in ctx.admit_iter(
                        &relation.return_members()[..],
                        "f3d sketch constraint return scan",
                    )? {
                        if let Some(entity) = ctx.get_hash_map(
                            &projected,
                            &(scope, member.reference.record_index()),
                            "f3d sketch constraint member lookup",
                        )? {
                            ctx.push_vec(
                                &mut semantic_entities,
                                *entity,
                                "f3d sketch constraint semantic entity",
                            )?;
                        }
                    }
                    Ok((input_entities, semantic_entities))
                })?;
            let sole_kind = crate::design::relation_kinds::sole_constraint_kind(relation)
                .filter(|_| semantic_entities.len() == relation.return_members().len());
            let mut definition = if let Some(kind) = sole_kind {
                let loci = if kind == SketchConstraintKind::Coincident {
                    exact_coincident_loci(&semantic_entities, ctx)?
                } else {
                    None
                };
                match loci {
                    Some(loci) => Some(loci),
                    None => exact_atomic_constraint(kind, &semantic_entities, ctx)?,
                }
            } else {
                None
            };
            if definition.is_none() {
                definition = exact_rectangular_pattern(
                    relation,
                    scope,
                    &mut pattern_parameters,
                    &semantic_entities,
                    ctx,
                )?;
            }
            if definition.is_none() {
                definition = exact_circular_pattern(
                    relation,
                    scope,
                    &mut pattern_parameters,
                    &input_entities,
                    &semantic_entities,
                    ctx,
                )?;
            }
            let definition = if definition.is_some() {
                definition
            } else {
                exact_offset_constraint(ctx, relation, scope, &projected).transpose()?
            };
            let definition = match definition {
                Some(definition) => Some(definition),
                None => exact_text_relation(relation, scope, &projected, ctx)?,
            };
            let definition = if let Some(definition) = definition {
                definition
            } else {
                let Some(native_kind) = cadmpeg_core::text::NonBlankString::for_decode(
                    ctx,
                    relation_kind_name(relation, ctx)?,
                    "validate nonblank text",
                )?
                else {
                    return Ok(None);
                };
                let member_indices = relation
                    .members()
                    .iter()
                    .map(|member| member.reference.record_index());
                let auxiliary_indices = relation.auxiliary_references().values().copied();
                let return_indices = relation
                    .return_members()
                    .iter()
                    .map(|member| member.reference.record_index());
                let mut native_entities = Vec::new();
                let mut record_indices = member_indices
                    .chain(auxiliary_indices)
                    .chain(return_indices);
                while let Some(record_index) = ctx.next_charged(
                    &mut record_indices,
                    "f3d sketch constraint native entity scan",
                )? {
                    if let Some(entity) = ctx.get_hash_map(
                        &projected,
                        &(scope, record_index),
                        "f3d sketch constraint native entity lookup",
                    )? {
                        let id = (entity.id())
                            .try_clone_for_decode(ctx, "f3d sketch constraint native entity id")?;
                        ctx.push_vec(
                            &mut native_entities,
                            id,
                            "f3d sketch constraint native entity",
                        )?;
                    }
                }
                let mut operands = Vec::new();
                for member in
                    ctx.admit_iter(&relation.members()[..], "f3d sketch constraint member scan")?
                {
                    let operand = native_operand(
                        scope,
                        cadmpeg_core::nonblank_literal!("member"),
                        member.reference.record_index(),
                    )?;
                    ctx.push_vec(
                        &mut operands,
                        operand,
                        "f3d sketch constraint native operand",
                    )?;
                }
                let mut auxiliary = relation.auxiliary_references().values();
                while let Some(record_index) =
                    ctx.next_charged(&mut auxiliary, "f3d sketch constraint auxiliary scan")?
                {
                    let operand = native_operand(
                        scope,
                        cadmpeg_core::nonblank_literal!("auxiliary"),
                        *record_index,
                    )?;
                    ctx.push_vec(
                        &mut operands,
                        operand,
                        "f3d sketch constraint native operand",
                    )?;
                }
                for member in ctx.admit_iter(
                    &relation.return_members()[..],
                    "f3d sketch constraint return scan",
                )? {
                    let operand = native_operand(
                        scope,
                        cadmpeg_core::nonblank_literal!("return"),
                        member.reference.record_index(),
                    )?;
                    ctx.push_vec(
                        &mut operands,
                        operand,
                        "f3d sketch constraint native operand",
                    )?;
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
            let Some(definition) =
                cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition).ok()
            else {
                return Ok(None);
            };
            Ok(Some(SketchConstraint {
                id: crate::design::identity::neutral_sketch_constraint_id(
                    ctx,
                    &relation.id,
                    relation.record_index,
                )?,
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
                native_ref: Some(
                    ctx.copy_retained_text(&relation.id, "f3d sketch constraint native reference")?,
                ),
            }))
        };
    let mut constraints = Vec::new();
    for relation in ctx.admit_iter(relations, "f3d sketch constraint relations scan")? {
        if let Some(constraint) = project_relation(relation)? {
            ctx.push_vec(
                &mut constraints,
                constraint,
                "f3d projected sketch constraint",
            )?;
        }
    }
    ctx.stable_sort_by(
        &mut constraints[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design constraints 1",
    )?;
    Ok(constraints)
}

struct PatternParameters<'a, 'ctx> {
    source: &'a [DesignParameter],
    ctx: &'ctx DecodeContext<'ctx>,
    index: Option<IndexedPatternParameters<'a, 'ctx>>,
}

struct IndexedPatternParameters<'a, 'ctx> {
    values: HashMap<(&'a str, u32), &'a DesignParameter>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'a, 'ctx> PatternParameters<'a, 'ctx> {
    fn new(ctx: &'ctx DecodeContext<'ctx>, source: &'a [DesignParameter]) -> Self {
        Self {
            source,
            ctx,
            index: None,
        }
    }

    fn get(&mut self, scope: &str, record: u32) -> Result<Option<&'a DesignParameter>, CodecError> {
        let ctx = self.ctx;
        let index = match &mut self.index {
            Some(index) => index,
            vacant @ None => {
                let mut storage = ctx.reserve_scoped(0, "f3d pattern parameter storage")?;
                let values = storage.with_storage(|| -> Result<_, CodecError> {
                    let mut index = HashMap::new();
                    for parameter in ctx.admit_iter(self.source, "f3d pattern parameter scan")? {
                        let (Some(stream), Some(owner)) = (
                            native_stream_charged(
                                ctx,
                                &parameter.id,
                                "f3d constraint native stream",
                            )?,
                            parameter.owner_record_index(),
                        ) else {
                            continue;
                        };
                        ctx.entry_hash_map(
                            &mut index,
                            (stream, owner),
                            "f3d pattern parameter index",
                        )?
                        .or_insert(parameter);
                    }
                    Ok(index)
                })?;
                vacant.insert(IndexedPatternParameters {
                    values,
                    _storage: storage,
                })
            }
        };
        Ok(ctx
            .get_hash_map(
                &index.values,
                &(scope, record),
                "f3d pattern parameter lookup",
            )?
            .copied())
    }
}

struct RectangularPatternSourceDirection<'a> {
    direction: [f64; 2],
    count: u32,
    distance: cadmpeg_ir::scalar::NonNegativeLength,
    distance_parameter: Option<&'a DesignParameter>,
    count_parameter: Option<&'a DesignParameter>,
}

#[derive(Clone, Copy)]
enum RectangularPatternDistanceForm {
    AdjacentSpacing,
    SeedToFinalSpan,
}

fn exact_rectangular_pattern(
    relation: &SketchRelation,
    scope: &str,
    parameters: &mut PatternParameters<'_, '_>,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput>, CodecError> {
    use crate::records::sketch_relations::SketchPatternDefinition;
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    if crate::design::relation_kinds::sole_constraint_kind(relation).is_none()
        || entities.len() != relation.return_members().len()
    {
        return Ok(None);
    }
    let Some(counted_references) = relation.rectangular_counted_reference_count else {
        return Ok(None);
    };
    let distance_form = match counted_references {
        0 => RectangularPatternDistanceForm::SeedToFinalSpan,
        _ => RectangularPatternDistanceForm::AdjacentSpacing,
    };
    let pattern = relation.definition.pattern();
    let Some(SketchPatternDefinition::Rectangular { directions }) = pattern else {
        return Ok(None);
    };
    let mut project_direction = |direction: &crate::records::sketch_relations::SketchPatternDirection| -> Result<Option<_>, CodecError> {
            let source_direction = direction.direction.get();
            if source_direction[2].abs() > EPS_CONSTRAINTS_EXACT_RECTANGULAR_PATTERN_E9 {
                return Ok(None);
            }
            let count_parameter = parameters.get(scope, direction.count_parameter)?;
            let distance_parameter = parameters.get(scope, direction.distance_parameter)?;
            let count = direction.evaluated_count.get();
            if count_parameter.is_some_and(|parameter| {
                !scalar_close(parameter.evaluated_value().get(), f64::from(count))
            }) {
                return Ok(None);
            }
            let distance = cadmpeg_ir::scalar::NonNegativeLength::new(
                direction.evaluated_distance.get() * 10.0,
            );
            let Some(distance) = distance else { return Ok(None); };
            if (count == 1 && !scalar_close(distance.get(), 0.0))
                || distance_parameter.is_some_and(|parameter| {
                    design_length(parameter)
                        .is_none_or(|value| !scalar_close(value.get(), distance.get()))
                })
            {
                return Ok(None);
            }
            Ok(Some(RectangularPatternSourceDirection {
                direction: [source_direction[0], source_direction[1]],
                count,
                distance,
                distance_parameter,
                count_parameter,
            }))
    };
    let Some(first) = project_direction(&directions[0])? else {
        return Ok(None);
    };
    let Some(second) = project_direction(&directions[1])? else {
        return Ok(None);
    };
    let source = [first, second];
    if source
        .iter()
        .any(|direction| !scalar_close(direction.direction[0].hypot(direction.direction[1]), 1.0))
    {
        return Ok(None);
    }
    let dot = source[0].direction[0] * source[1].direction[0]
        + source[0].direction[1] * source[1].direction[1];
    if dot.abs() > EPS_CONSTRAINTS_EXACT_RECTANGULAR_PATTERN_E9 {
        return Ok(None);
    }
    let make = |source: &RectangularPatternSourceDirection<'_>| {
        let spacing = match distance_form {
            RectangularPatternDistanceForm::AdjacentSpacing => source.distance.get(),
            RectangularPatternDistanceForm::SeedToFinalSpan if source.count > 1 => {
                source.distance.get() / f64::from(source.count - 1)
            }
            RectangularPatternDistanceForm::SeedToFinalSpan => 0.0,
        };
        cadmpeg_ir::sketches::SketchPatternDirection::new(
            source.direction,
            cadmpeg_ir::scalar::Length::new(spacing)?,
            None,
            None,
        )
    };
    let (Some(first), Some(second)) = (make(&source[0]), make(&source[1])) else {
        return Ok(None);
    };
    let mut directions = [first, second];
    let counts = source.each_ref().map(|direction| direction.count);
    let Some(rows) = exact_rectangular_pattern_instances(&directions, counts, entities, ctx)?
    else {
        return Ok(None);
    };
    for (direction, source) in directions.iter_mut().zip(&source) {
        direction.distance = source
            .distance_parameter
            .map(|parameter| {
                crate::design::identity::neutral_parameter_id_with_operation(
                    ctx,
                    parameter,
                    "f3d rectangular pattern distance parameter id",
                )
            })
            .transpose()?
            .map(|parameter| match distance_form {
                RectangularPatternDistanceForm::AdjacentSpacing => {
                    cadmpeg_ir::sketches::SketchPatternDistance::Spacing { parameter }
                }
                RectangularPatternDistanceForm::SeedToFinalSpan => {
                    cadmpeg_ir::sketches::SketchPatternDistance::Span { parameter }
                }
            });
        direction.count_parameter = source
            .count_parameter
            .map(|parameter| {
                crate::design::identity::neutral_parameter_id_with_operation(
                    ctx,
                    parameter,
                    "f3d rectangular pattern count parameter id",
                )
            })
            .transpose()?;
    }
    Ok(
        cadmpeg_ir::sketches::SketchRectangularPattern::new(directions, rows)
            .map(|pattern| Definition::RectangularPattern { pattern }),
    )
}

fn exact_rectangular_pattern_instances(
    directions: &[cadmpeg_ir::sketches::SketchPatternDirection; 2],
    counts: [u32; 2],
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Vec<cadmpeg_ir::sketches::SketchPatternInstance>>>, CodecError> {
    let instance_count = usize::try_from(counts[0]).ok().and_then(|first| {
        usize::try_from(counts[1])
            .ok()
            .and_then(|second| first.checked_mul(second))
    });
    let Some(instance_count) = instance_count else {
        return Ok(None);
    };
    if instance_count == 0 || !entities.len().is_multiple_of(instance_count) {
        return Ok(None);
    }
    let entity_count = entities.len() / instance_count;
    let Some(seed) = entities.get(..entity_count) else {
        return Ok(None);
    };
    if seed.is_empty() {
        return Ok(None);
    }
    let Ok(column_count) = usize::try_from(counts[1]) else {
        return Ok(None);
    };
    let mut instances = entities.chunks_exact(entity_count).enumerate();
    while let Some((position, instance)) =
        ctx.next_charged(&mut instances, "f3d rectangular pattern instance scan")?
    {
        let expected = [
            match u32::try_from(position / column_count) {
                Ok(value) => value,
                Err(_) => return Ok(None),
            },
            match u32::try_from(position % column_count) {
                Ok(value) => value,
                Err(_) => return Ok(None),
            },
        ];
        let mut matched = false;
        let mut candidates =
            (0..counts[0]).flat_map(|first| (0..counts[1]).map(move |second| (first, second)));
        while let Some((first, second)) =
            ctx.next_charged(&mut candidates, "f3d rectangular pattern candidate match")?
        {
            let indices = [first, second];
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
            if ctx.all_by(
                seed.iter().zip(instance),
                |(source, result)| {
                    translated_sketch_geometry_matches(
                        ctx,
                        &source.geometry,
                        &result.geometry,
                        translation,
                    )
                },
                "f3d rectangular pattern geometry scan",
            )? {
                if matched || indices != expected {
                    return Ok(None);
                }
                matched = true;
            }
        }
        if !matched {
            return Ok(None);
        }
    }
    let mut rows = Vec::new();
    let mut instances = entities.chunks_exact(entity_count).enumerate();
    while let Some((position, instance)) =
        ctx.next_charged(&mut instances, "f3d rectangular selected instance scan")?
    {
        let mut entity_ids = Vec::new();
        for entity in ctx.admit_iter(instance, "f3d pattern instance entity scan")? {
            let id = (entity.id())
                .try_clone_for_decode(ctx, "f3d rectangular pattern instance entity id")?;
            ctx.push_vec(
                &mut entity_ids,
                id,
                "f3d rectangular pattern instance entity",
            )?;
        }
        if position % column_count == 0 {
            ctx.push_vec(&mut rows, Vec::new(), "f3d rectangular pattern row")?;
        }
        let Some(row) = rows.last_mut() else {
            return Ok(None);
        };
        ctx.push_vec(
            row,
            cadmpeg_ir::sketches::SketchPatternInstance {
                entities: entity_ids,
            },
            "f3d rectangular pattern instance",
        )?;
    }
    Ok(Some(rows))
}

fn exact_text_relation(
    relation: &SketchRelation,
    scope: &str,
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput>, CodecError> {
    use crate::records::sketch_relations::SketchPatternDefinition;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };
    use cadmpeg_ir::transform::Transform;

    if crate::design::relation_kinds::sole_constraint_kind(relation).is_none() {
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
                && relation.auxiliary_references().values_array::<1>()
                    == Some([text_reference])
                && relation.return_members().len() == relation.members().len() - 1
                && ctx.all_by(
                    relation
                        .return_members()
                        .iter()
                        .zip(&relation.members()[1..]),
                    |(returned, member)| {
                        Ok(returned.reference.record_index() == member.reference.record_index())
                    },
                    "f3d text frame member comparison",
                )? =>
        {
            let Some(text) = ctx.get_hash_map(
                projected,
                &(scope, *text_reference),
                "f3d sketch text relation entity lookup",
            )?
            else {
                return Ok(None);
            };
            if !matches!(
                *text.geometry.definition(),
                SketchGeometryDefinition::Text { .. }
            ) {
                return Ok(None);
            }
            let mut frame_storage = ctx.reserve_scoped(0, "f3d text frame reference storage")?;
            let mut frame = Vec::new();
            let mut returned = relation.return_members().iter();
            while let Some(member) =
                ctx.next_charged(&mut returned, "f3d sketch text frame scan")?
            {
                let Some(entity) = ctx.get_hash_map(
                    projected,
                    &(scope, member.reference.record_index()),
                    "f3d sketch text relation entity lookup",
                )?
                else {
                    return Ok(None);
                };
                if ctx.equal(
                    entity.id(),
                    text.id(),
                    "f3d sketch text frame id comparison",
                )? || matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Text { .. }
                ) {
                    return Ok(None);
                }
                frame_storage.with_storage(|| {
                    ctx.push_vec(&mut frame, entity.id(), "f3d text frame admitted entity")
                })?;
            }
            if frame.is_empty() {
                return Ok(None);
            }
            let mut retained = Vec::new();
            for entity in ctx.admit_iter(frame, "f3d text frame selected scan")? {
                let id = entity
                    .try_clone_for_decode(ctx, "f3d sketch constraint text frame entity id")?;
                ctx.push_vec(&mut retained, id, "f3d sketch constraint text frame entity")?;
            }
            Some(Definition::TextFrame {
                text: (text.id())
                    .try_clone_for_decode(ctx, "f3d sketch constraint text frame text id")?,
                frame: retained,
            })
        }
        Some(SketchPatternDefinition::TextPath {
            text_reference,
            glyph_transforms,
        }) if relation.members().len() == 2
            && relation.members()[1].reference.record_index() == *text_reference
            && relation.auxiliary_references().values_array::<1>() == Some([text_reference])
            && relation.return_members().len() == 1
            && relation.return_members()[0].reference.record_index()
                == relation.members()[0].reference.record_index() =>
        {
            let Some(path) = ctx.get_hash_map(
                projected,
                &(scope, relation.members()[0].reference.record_index()),
                "f3d sketch text relation entity lookup",
            )?
            else {
                return Ok(None);
            };
            let Some(text) = ctx.get_hash_map(
                projected,
                &(scope, *text_reference),
                "f3d sketch text relation entity lookup",
            )?
            else {
                return Ok(None);
            };
            if ctx.equal(path.id(), text.id(), "f3d sketch text path id comparison")?
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
            let mut glyph_storage = ctx.reserve_scoped(0, "f3d text glyph storage")?;
            let mut glyphs = Vec::new();
            let mut transforms = glyph_transforms.iter();
            while let Some(source) =
                ctx.next_charged(&mut transforms, "f3d sketch text glyph scan")?
            {
                let source = source.rows();
                if source[3] != [0.0, 0.0, 0.0, 1.0] {
                    return Ok(None);
                }
                let mut rows = [source[0], source[1], source[2]];
                for row in &mut rows {
                    row[3] *= 10.0;
                }
                let Some(glyph) = Transform::affine(rows) else {
                    return Ok(None);
                };
                glyph_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut glyphs,
                        glyph,
                        "f3d sketch constraint text glyph transform",
                    )
                })?;
            }
            glyph_storage.commit()?;
            Some(Definition::TextPath {
                text: (text.id())
                    .try_clone_for_decode(ctx, "f3d sketch constraint text path text id")?,
                path: (path.id())
                    .try_clone_for_decode(ctx, "f3d sketch constraint text path curve id")?,
                glyph_transforms: glyphs,
            })
        }
        _ => None,
    })
}

struct CircularPatternCandidate<'a, 'ctx> {
    center: &'a cadmpeg_ir::sketches::SketchEntityId,
    seed: Vec<&'a cadmpeg_ir::sketches::SketchEntityId>,
    instances: Vec<(
        cadmpeg_ir::scalar::NonZeroAngle,
        Vec<&'a cadmpeg_ir::sketches::SketchEntityId>,
    )>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn exact_circular_pattern(
    relation: &SketchRelation,
    scope: &str,
    parameters: &mut PatternParameters<'_, '_>,
    members: &[&cadmpeg_ir::sketches::SketchEntity],
    returned: &[&cadmpeg_ir::sketches::SketchEntity],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput>, CodecError> {
    use crate::records::sketch_relations::SketchPatternDefinition;
    use cadmpeg_ir::sketches::{
        SketchCircularPattern, SketchCircularPatternInstance,
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    if crate::design::relation_kinds::sole_constraint_kind(relation).is_none()
        || members.len() != relation.members().len()
        || returned.len() != relation.return_members().len()
    {
        return Ok(None);
    }
    let pattern = relation.definition.pattern();
    let Some(SketchPatternDefinition::Circular {
        angle_parameter,
        count_parameter,
        evaluated_angle,
        evaluated_count,
    }) = pattern
    else {
        return Ok(None);
    };
    let angle_parameter = parameters.get(scope, *angle_parameter)?;
    let count_parameter = parameters.get(scope, *count_parameter)?;
    let angle = cadmpeg_ir::scalar::Angle::from_assigned_real(*evaluated_angle);
    if angle_parameter.is_some_and(|parameter| {
        design_angle(parameter).is_none_or(|value| !scalar_close(value.get(), angle.get()))
    }) || count_parameter.is_some_and(|parameter| {
        !scalar_close(
            parameter.evaluated_value().get(),
            f64::from(evaluated_count.get()),
        )
    }) {
        return Ok(None);
    }
    // Relation ordinals do not classify center/seed/generated; partition by
    // geometry when the member and returned id sets match.
    let mut reference_storage = ctx.reserve_scoped(0, "f3d circular pattern reference storage")?;
    let matching_references = reference_storage.with_storage(|| -> Result<_, CodecError> {
        let mut member_ids = BTreeSet::new();
        for entity in ctx.admit_iter(members, "f3d circular pattern member scan")? {
            ctx.insert_btree_set(
                &mut member_ids,
                entity.id(),
                "f3d circular pattern member id",
            )
            .map(|_| ())?;
        }
        let mut returned_ids = BTreeSet::new();
        for entity in ctx.admit_iter(returned, "f3d circular pattern return scan")? {
            ctx.insert_btree_set(
                &mut returned_ids,
                entity.id(),
                "f3d circular pattern returned id",
            )
            .map(|_| ())?;
        }
        if member_ids.len() != members.len()
            || returned_ids.len() != returned.len()
            || !ctx.all_by(
                member_ids.iter().zip(&returned_ids),
                |(first, second)| {
                    ctx.equal(
                        *first,
                        *second,
                        "f3d circular pattern reference id comparison",
                    )
                },
                "f3d circular pattern reference set comparison",
            )?
        {
            return Ok(false);
        }
        Ok(true)
    })?;
    if !matching_references {
        return Ok(None);
    }
    drop(reference_storage);
    let mut candidate_storage = ctx.reserve_scoped(0, "f3d circular pattern candidate storage")?;
    let mut candidates = Vec::new();
    let mut centers = members.iter().copied();
    while let Some(center) = ctx.next_charged(&mut centers, "f3d circular pattern center scan")? {
        let SketchGeometryDefinition::Point {
            position: center_position,
        } = *center.geometry.definition()
        else {
            continue;
        };
        let center_position = center_position.get();
        let mut entity_storage = ctx.reserve_scoped(0, "f3d circular pattern entity storage")?;
        let patterned = entity_storage.with_storage(|| -> Result<_, CodecError> {
            let mut patterned = Vec::new();
            for entity in ctx.admit_iter(returned, "f3d circular pattern returned entity scan")? {
                if !ctx.equal(
                    entity.id(),
                    center.id(),
                    "f3d circular pattern center comparison",
                )? {
                    ctx.push_vec(&mut patterned, *entity, "f3d circular pattern entity")?;
                }
            }
            Ok(patterned)
        })?;
        let Ok(count) = usize::try_from(evaluated_count.get()) else {
            return Ok(None);
        };
        if patterned.is_empty() || !patterned.len().is_multiple_of(count) {
            continue;
        }
        let arity = patterned.len() / count;
        let seed = &patterned[..arity];
        if seed.is_empty() {
            continue;
        }
        let first_divisor = f64::from(evaluated_count.get());
        let second_divisor = (evaluated_count.get() > 1)
            .then(|| f64::from(evaluated_count.get() - 1))
            .filter(|second| !scalar_close(first_divisor, *second));
        for divisor in [Some(first_divisor), second_divisor].into_iter().flatten() {
            // The seed is the first chunk; it is not an instance, and the
            // instances carry only their nonzero rotations.
            let mut instance_storage =
                ctx.reserve_scoped(0, "f3d circular pattern instance storage")?;
            let mut instances = Vec::new();
            let mut valid = true;
            let mut chunks = patterned.chunks_exact(arity).enumerate().skip(1);
            while let Some((index, instance)) =
                ctx.next_charged(&mut chunks, "f3d circular pattern instance scan")?
            {
                let Some(index) = f64_from_index(index) else {
                    return Ok(None);
                };
                let rotation = evaluated_angle.get() * index / divisor;
                if !ctx.all_by(
                    seed.iter().zip(instance),
                    |(source, result)| {
                        rotated_sketch_geometry_matches(
                            ctx,
                            &source.geometry,
                            &result.geometry,
                            center_position,
                            rotation,
                        )
                    },
                    "f3d circular pattern geometry scan",
                )? {
                    valid = false;
                    break;
                }
                let Some(angle) = cadmpeg_ir::scalar::NonZeroAngle::new(rotation) else {
                    valid = false;
                    break;
                };
                let entity_ids = instance_storage.with_storage(|| {
                    ctx.collect_vec(
                        instance.iter().map(|entity| entity.id()),
                        "f3d circular pattern instance entity",
                    )
                })?;
                instance_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut instances,
                        (angle, entity_ids),
                        "f3d circular pattern instance",
                    )
                })?;
            }
            if valid {
                let seed_entities = instance_storage.with_storage(|| {
                    ctx.collect_vec(
                        seed.iter().map(|entity| entity.id()),
                        "f3d circular pattern seed entity",
                    )
                })?;
                candidate_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut candidates,
                        CircularPatternCandidate {
                            center: center.id(),
                            seed: seed_entities,
                            instances,
                            _storage: instance_storage,
                        },
                        "f3d circular pattern candidate",
                    )
                })?;
            }
        }
    }
    ctx.dedup_by(
        &mut candidates,
        |first, second| {
            Ok(ctx.equal(
                first.center,
                second.center,
                "f3d circular candidate center comparison",
            )? && ctx.equal(
                &first.seed,
                &second.seed,
                "f3d circular candidate seed comparison",
            )? && ctx.equal(
                &first.instances,
                &second.instances,
                "f3d circular candidate instance comparison",
            )?)
        },
        "f3d constraints candidates dedup",
    )?;
    if candidates.len() != 1 {
        return Ok(None);
    }
    let Some(selected) = candidates.pop() else {
        return Ok(None);
    };
    if selected.instances.is_empty() {
        return Ok(None);
    }
    let mut instances = Vec::new();
    for (angle, entities) in
        ctx.admit_iter(&selected.instances, "f3d circular selected instance scan")?
    {
        let mut entity_ids = Vec::new();
        for entity in ctx.admit_iter(entities, "f3d circular selected member scan")? {
            let id = entity.try_clone_for_decode(ctx, "f3d circular pattern instance entity id")?;
            ctx.push_vec(&mut entity_ids, id, "f3d circular pattern instance entity")?;
        }
        ctx.push_vec(
            &mut instances,
            SketchCircularPatternInstance {
                angle: *angle,
                entities: entity_ids,
            },
            "f3d circular pattern instance",
        )?;
    }
    let mut seed_entities = Vec::new();
    for entity in ctx.admit_iter(&selected.seed, "f3d circular selected seed scan")? {
        let id = entity.try_clone_for_decode(ctx, "f3d circular pattern seed entity id")?;
        ctx.push_vec(&mut seed_entities, id, "f3d circular pattern seed entity")?;
    }
    let center = selected
        .center
        .try_clone_for_decode(ctx, "f3d circular pattern center id")?;
    let pattern = SketchCircularPattern::new(
        center,
        angle,
        angle_parameter
            .map(|parameter| {
                crate::design::identity::neutral_parameter_id_with_operation(
                    ctx,
                    parameter,
                    "f3d parameter identifier",
                )
            })
            .transpose()?,
        count_parameter
            .map(|parameter| {
                crate::design::identity::neutral_parameter_id_with_operation(
                    ctx,
                    parameter,
                    "f3d parameter identifier",
                )
            })
            .transpose()?,
        seed_entities,
        instances,
    );
    Ok(pattern.map(|pattern| Definition::CircularPattern { pattern }))
}

fn rotated_sketch_geometry_matches(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::sketches::SketchGeometry,
    result: &cadmpeg_ir::sketches::SketchGeometry,
    center: Point2,
    angle: f64,
) -> Result<bool, CodecError> {
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
    Ok(match (source.definition(), result.definition()) {
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
                && equal_scalars(ctx, first.knots(), second.knots())?
                && nurbs_poles_match(ctx, first, second, point_matches)?
        }
        _ => false,
    })
}

pub(super) fn scalar_close(first: f64, second: f64) -> bool {
    first.is_finite()
        && second.is_finite()
        && (first - second).abs()
            <= EPS_CONSTRAINTS_SCALAR_CLOSE_E9 * (1.0 + first.abs().max(second.abs()))
}

fn translated_sketch_geometry_matches(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::sketches::SketchGeometry,
    result: &cadmpeg_ir::sketches::SketchGeometry,
    translation: Point2,
) -> Result<bool, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let point_matches = |first: Point2, second: Point2| {
        scalar_close(first.u + translation.u, second.u)
            && scalar_close(first.v + translation.v, second.v)
    };
    Ok(match (source.definition(), result.definition()) {
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
                && equal_scalars(ctx, first.knots(), second.knots())?
                && nurbs_poles_match(ctx, first, second, point_matches)?
        }
        _ => false,
    })
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

fn equal_scalars(
    ctx: &DecodeContext<'_>,
    first: &[f64],
    second: &[f64],
) -> Result<bool, CodecError> {
    Ok(first.len() == second.len()
        && ctx.all_by(
            first.iter().zip(second),
            |(first, second)| Ok(scalar_close(*first, *second)),
            "f3d pattern knot scan",
        )?)
}

fn nurbs_poles_match(
    ctx: &DecodeContext<'_>,
    first: &PcurveNurbs,
    second: &PcurveNurbs,
    point_matches: impl Fn(Point2, Point2) -> bool,
) -> Result<bool, CodecError> {
    Ok(match (first.pole_rows(), second.pole_rows()) {
        (
            PcurveNurbsPoles::Polynomial { points: first },
            PcurveNurbsPoles::Polynomial { points: second },
        ) => {
            first.len() == second.len()
                && ctx.all_by(
                    first.iter().zip(second),
                    |(a, b)| Ok(point_matches(a.get(), b.get())),
                    "f3d pattern polynomial pole scan",
                )?
        }
        (
            PcurveNurbsPoles::Rational { points: first },
            PcurveNurbsPoles::Rational { points: second },
        ) => {
            first.len() == second.len()
                && ctx.all_by(
                    first.iter().zip(second),
                    |(a, b)| {
                        Ok(point_matches(a.point.get(), b.point.get())
                            && scalar_close(a.weight.get(), b.weight.get()))
                    },
                    "f3d pattern rational pole scan",
                )?
        }
        _ => false,
    })
}

#[cfg(test)]
mod tests;
