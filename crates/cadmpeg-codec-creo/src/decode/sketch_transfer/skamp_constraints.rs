// SPDX-License-Identifier: Apache-2.0
//! SKAMP solver constraint emission and locus compatibility.

use super::super::sketch_ids::{
    sketch_constraint_id_admitted, sketch_entity_id_admitted, sketch_native_ref_admitted,
};
use crate::decode::sketch::coordinates::resolved_section_points;
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
use std::cell::Cell;
use std::collections::BTreeMap;

fn defer_resource<T>(
    result: Result<T, cadmpeg_core::CodecError>,
    resource_error: &Cell<Option<cadmpeg_core::CodecError>>,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            resource_error.set(Some(resource_error.take().unwrap_or(error)));
            None
        }
    }
}

fn admitted_entity(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &SketchId,
    external_id: u32,
    resource_error: &Cell<Option<cadmpeg_core::CodecError>>,
) -> Option<SketchEntityId> {
    defer_resource(
        sketch_entity_id_admitted(ctx, sketch, external_id),
        resource_error,
    )
    .flatten()
}

fn native_skamp_nonblank(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: impl std::fmt::Display,
    operation: &'static str,
) -> Result<cadmpeg_core::text::NonBlankString, cadmpeg_core::CodecError> {
    let text = ctx.format_retained(format_args!("{value}"), operation)?;
    cadmpeg_core::text::NonBlankString::for_decode(ctx, text, "validate nonblank text")?
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("blank native SKAMP field"))
}

pub(in super::super) fn section_skamp_constraints_for_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Result<Vec<(SketchConstraint, usize)>, cadmpeg_core::CodecError> {
    let Some(relations) = &definition.relations else {
        return Ok(Vec::new());
    };
    let resolved_points = if ctx.any_by(
        relations.skamps(),
        |skamp| Ok(section_skamp_active(skamp.status) && matches!(skamp.kind, 15 | 17 | 30 | 31)),
        "creo SKAMP coordinate requirement search",
    )? {
        Some(resolved_section_points(ctx, definition)?)
    } else {
        None
    };
    let complete_skamps = relations
        .skamps
        .as_ref()
        .is_none_or(SolverSubtable::is_complete);
    let mut skamp_id_counts = BTreeMap::<u32, usize>::new();
    for skamp in ctx.admit_iter(relations.skamps(), "creo SKAMP constraint row traversal")? {
        *ctx.entry_btree_map(&mut skamp_id_counts, skamp.id, "creo skamp ID count nodes")?
            .or_default() += 1;
    }
    let available_entities = if let Some(geometry) = geometry {
        let mut ids = std::collections::BTreeSet::new();
        for skamp in ctx.admit_iter(
            relations.skamps(),
            "creo SKAMP available entity row traversal",
        )? {
            for item in
                ctx.admit_iter(&skamp.items, "creo SKAMP available entity item traversal")?
            {
                let entity_id = item.entity_id;
                if sketch_entity_id_admitted(ctx, sketch, entity_id)?
                    .is_some_and(|id| geometry.contains_key(&id))
                {
                    ctx.insert_btree_set(&mut ids, entity_id, "creo skamp available entity nodes")?;
                }
            }
        }
        ids
    } else {
        section_entity_external_ids(ctx, definition)?
    };
    let mut constraints = Vec::new();
    for skamp in ctx.admit_iter(relations.skamps(), "creo SKAMP constraint row traversal")? {
        let resource_error = Cell::new(None);
        let candidate = (|| {
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
            let native_constraint = |resource_error: &Cell<Option<cadmpeg_core::CodecError>>| {
                let native_ref =
                    defer_resource(sketch_native_ref_admitted(ctx, sketch), resource_error)?;
                let mut entities = Vec::new();
                for item in defer_resource(
                    ctx.admit_iter(&skamp.items, "creo SKAMP native entity item traversal")
                        .map_err(cadmpeg_core::CodecError::from),
                    resource_error,
                )?
                .filter(|item| available_entities.contains(&item.entity_id))
                {
                    let Some(id) = defer_resource(
                        sketch_entity_id_admitted(ctx, sketch, item.entity_id),
                        resource_error,
                    )?
                    else {
                        continue;
                    };
                    defer_resource(
                        ctx.reserve_vec(&mut entities, 1, "creo skamp native entities"),
                        resource_error,
                    )?;
                    entities.push(id);
                }
                let mut operands = Vec::new();
                for item in defer_resource(
                    ctx.admit_iter(&skamp.items, "creo SKAMP native operand item traversal")
                        .map_err(cadmpeg_core::CodecError::from),
                    resource_error,
                )? {
                    defer_resource(
                        ctx.reserve_vec(&mut operands, 1, "creo skamp native operands"),
                        resource_error,
                    )?;
                    operands.push(SketchNativeOperand {
                        native_kind: defer_resource(
                            native_skamp_nonblank(
                                ctx,
                                format_args!("skamp_ptr"),
                                "creo skamp operand kind",
                            ),
                            resource_error,
                        )?,
                        field: Some(NativeOperandField {
                            name: defer_resource(
                                native_skamp_nonblank(
                                    ctx,
                                    format_args!("items.entity_id"),
                                    "creo skamp operand field",
                                ),
                                resource_error,
                            )?,
                            role: Some(item.sense),
                        }),
                        object_index: Some(item.entity_id),
                        native_ref: Some(defer_resource(
                            ctx.copy_retained_text(&native_ref, "creo skamp operand reference"),
                            resource_error,
                        )?),
                    });
                }
                if let Some(equation_id) = joined_equation_id {
                    defer_resource(
                        ctx.reserve_vec(&mut operands, 1, "creo skamp native operands"),
                        resource_error,
                    )?;
                    operands.push(SketchNativeOperand {
                        native_kind: defer_resource(
                            native_skamp_nonblank(
                                ctx,
                                format_args!("triples_ptr"),
                                "creo skamp operand kind",
                            ),
                            resource_error,
                        )?,
                        field: Some(NativeOperandField {
                            name: defer_resource(
                                native_skamp_nonblank(
                                    ctx,
                                    format_args!("equation_id"),
                                    "creo skamp operand field",
                                ),
                                resource_error,
                            )?,
                            role: None,
                        }),
                        object_index: Some(equation_id),
                        native_ref: Some(native_ref),
                    });
                }
                let mut native_properties = BTreeMap::new();
                if !unique_skamp_id {
                    let key = defer_resource(
                        ctx.copy_retained_text("id", "creo skamp property key"),
                        resource_error,
                    )?;
                    let value = defer_resource(
                        ctx.format_retained(
                            format_args!("{}", skamp.id),
                            "creo skamp property value",
                        ),
                        resource_error,
                    )?;
                    defer_resource(
                        ctx.insert_btree_map(
                            &mut native_properties,
                            key,
                            value,
                            "creo skamp native property nodes",
                        ),
                        resource_error,
                    )?;
                }
                Some(SketchConstraintDefinitionInput::Native {
                    native_kind: defer_resource(
                        native_skamp_nonblank(
                            ctx,
                            format_args!("creo:skamp:{}", skamp.kind),
                            "creo skamp native kind",
                        ),
                        resource_error,
                    )?,
                    native_state: Some(u64::from(skamp.status)),
                    native_flags: Some(u64::from(skamp.flags)),
                    native_properties,
                    entities,
                    parameter: None,
                    operands,
                })
            };
            let item_geometry = |item: &crate::feature::definitions::FeatureSkampItem| {
                let entity = admitted_entity(ctx, sketch, item.entity_id, &resource_error)?;
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
                .then(|| admitted_entity(ctx, sketch, item.entity_id, &resource_error))
                .flatten()
            };
            let inactive_incidence_locus =
                |item: &crate::feature::definitions::FeatureSkampItem| {
                    defer_resource(
                        section_skamp_incidence_locus(
                            ctx,
                            &resource_error,
                            definition,
                            sketch,
                            item,
                            geometry,
                        ),
                        &resource_error,
                    )?
                    .or_else(|| {
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
                            admitted_entity(ctx, sketch, item.entity_id, &resource_error)
                                .map(SketchLocus::Center)
                        })
                        .flatten()
                    })
                };
            let point_entity = |item: &crate::feature::definitions::FeatureSkampItem| {
                (item.sense == 0).then_some(())?;
                if defer_resource(
                    section_skamp_is_point(ctx, definition, item),
                    &resource_error,
                )? {
                    return admitted_entity(ctx, sketch, item.entity_id, &resource_error);
                }
                (!active && item_geometry(item).is_some_and(|geometry| {
                    matches!(
                        geometry.definition(),
                        SketchGeometryDefinition::Point { .. }
                    ) || matches!((
                        geometry).definition(),
                        SketchGeometryDefinition::Native { native_kind } if native_kind.as_str() == "point"
                    )
                }))
                .then(|| admitted_entity(ctx, sketch, item.entity_id, &resource_error))
                .flatten()
            };
            let inactive_point_locus = |item: &crate::feature::definitions::FeatureSkampItem| {
                defer_resource(
                    section_skamp_point_locus(ctx, &resource_error, definition, sketch, item),
                    &resource_error,
                )?
                .or_else(|| point_entity(item).map(SketchLocus::Entity))
                .or_else(|| inactive_incidence_locus(item))
            };
            let mut constraint_definition = if unique_skamp_id {
                match (skamp.kind, skamp.items.as_slice()) {
                    (0, [first, second])
                        if defer_resource(
                            section_skamp_center_entity(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                first,
                            ),
                            &resource_error,
                        )?
                        .is_some()
                            && defer_resource(
                                section_skamp_center_entity(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    second,
                                ),
                                &resource_error,
                            )?
                            .is_some() =>
                    {
                        SketchConstraintDefinitionInput::Concentric {
                            first: defer_resource(
                                section_skamp_center_entity(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    first,
                                ),
                                &resource_error,
                            )??,
                            second: defer_resource(
                                section_skamp_center_entity(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    second,
                                ),
                                &resource_error,
                            )??,
                        }
                    }
                    (0, [first, second])
                        if defer_resource(
                            section_skamp_incidence_locus(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                first,
                                geometry,
                            ),
                            &resource_error,
                        )?
                        .is_some()
                            && defer_resource(
                                section_skamp_incidence_locus(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    second,
                                    geometry,
                                ),
                                &resource_error,
                            )?
                            .is_some() =>
                    {
                        let mut loci = Vec::new();
                        defer_resource(
                            ctx.reserve_vec(&mut loci, 2, "creo skamp coincident loci"),
                            &resource_error,
                        )?;
                        loci.push(defer_resource(
                            section_skamp_incidence_locus(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                first,
                                geometry,
                            ),
                            &resource_error,
                        )??);
                        loci.push(defer_resource(
                            section_skamp_incidence_locus(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                second,
                                geometry,
                            ),
                            &resource_error,
                        )??);
                        SketchConstraintDefinitionInput::CoincidentLoci { loci }
                    }
                    (3, [first, second]) => {
                        if let (Some(first), Some(second)) =
                            (point_entity(first), point_entity(second))
                        {
                            let mut loci = Vec::new();
                            defer_resource(
                                ctx.reserve_vec(&mut loci, 2, "creo skamp coincident loci"),
                                &resource_error,
                            )?;
                            loci.push(SketchLocus::Entity(first));
                            loci.push(SketchLocus::Entity(second));
                            SketchConstraintDefinitionInput::CoincidentLoci { loci }
                        } else {
                            let directed = [(first, second), (second, first)];
                            let point_on_curve = crate::decode::uniqueness::exactly_one(
                                directed.into_iter().filter_map(|(curve, point)| {
                                    Some((
                                        defer_resource(
                                            section_skamp_curve_entity(
                                                ctx,
                                                &resource_error,
                                                definition,
                                                sketch,
                                                curve,
                                            ),
                                            &resource_error,
                                        )?
                                        .or_else(|| inactive_curve_entity(curve))?,
                                        inactive_incidence_locus(point)?,
                                    ))
                                }),
                            );
                            if let Some((entity, point)) = point_on_curve {
                                SketchConstraintDefinitionInput::PointOnObject { point, entity }
                            } else {
                                let point_coincidence = crate::decode::uniqueness::exactly_one(
                                    directed.into_iter().filter_map(|(point, locus)| {
                                        Some([
                                            SketchLocus::Entity(point_entity(point)?),
                                            inactive_incidence_locus(locus)?,
                                        ])
                                    }),
                                );
                                if let Some(loci) = point_coincidence {
                                    let mut admitted_loci = Vec::new();
                                    defer_resource(
                                        ctx.reserve_vec(
                                            &mut admitted_loci,
                                            2,
                                            "creo skamp coincident loci",
                                        ),
                                        &resource_error,
                                    )?;
                                    admitted_loci.extend(loci);
                                    SketchConstraintDefinitionInput::CoincidentLoci {
                                        loci: admitted_loci,
                                    }
                                } else {
                                    native_constraint(&resource_error)?
                                }
                            }
                        }
                    }
                    (kind @ (1 | 2), [item]) => {
                        match defer_resource(
                            section_skamp_oriented_line(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                item,
                                geometry,
                            ),
                            &resource_error,
                        )? {
                            Some(entity) if kind == 1 => {
                                SketchConstraintDefinitionInput::Horizontal { entity }
                            }
                            Some(entity) => SketchConstraintDefinitionInput::Vertical { entity },
                            None => native_constraint(&resource_error)?,
                        }
                    }
                    (4, [first, second]) => {
                        if let Some([first, second]) = defer_resource(
                            section_skamp_tangent_loci(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                (first, second),
                                active,
                                geometry,
                            ),
                            &resource_error,
                        )? {
                            SketchConstraintDefinitionInput::TangentLoci { first, second }
                        } else if defer_resource(
                            section_skamp_curve_entity(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                first,
                            ),
                            &resource_error,
                        )?
                        .is_some()
                            && defer_resource(
                                section_skamp_curve_entity(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    second,
                                ),
                                &resource_error,
                            )?
                            .is_some()
                        {
                            SketchConstraintDefinitionInput::Tangent {
                                first: defer_resource(
                                    section_skamp_curve_entity(
                                        ctx,
                                        &resource_error,
                                        definition,
                                        sketch,
                                        first,
                                    ),
                                    &resource_error,
                                )??,
                                second: defer_resource(
                                    section_skamp_curve_entity(
                                        ctx,
                                        &resource_error,
                                        definition,
                                        sketch,
                                        second,
                                    ),
                                    &resource_error,
                                )??,
                            }
                        } else {
                            native_constraint(&resource_error)?
                        }
                    }
                    (5, [first, second]) => {
                        match (
                            defer_resource(
                                section_skamp_curve_entity(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    first,
                                ),
                                &resource_error,
                            )?,
                            defer_resource(
                                section_skamp_curve_entity(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    second,
                                ),
                                &resource_error,
                            )?,
                        ) {
                            (Some(first), Some(second)) => {
                                SketchConstraintDefinitionInput::Perpendicular { first, second }
                            }
                            _ => native_constraint(&resource_error)?,
                        }
                    }
                    (6, [first, second])
                        if defer_resource(
                            section_skamp_circular_entity(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                first,
                            ),
                            &resource_error,
                        )?
                        .is_some()
                            && defer_resource(
                                section_skamp_circular_entity(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    second,
                                ),
                                &resource_error,
                            )?
                            .is_some() =>
                    {
                        SketchConstraintDefinitionInput::Equal {
                            first: defer_resource(
                                section_skamp_circular_entity(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    first,
                                ),
                                &resource_error,
                            )??,
                            second: defer_resource(
                                section_skamp_circular_entity(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    second,
                                ),
                                &resource_error,
                            )??,
                        }
                    }
                    (7, [first, second])
                        if defer_resource(
                            section_skamp_line_pair(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                first,
                                second,
                            ),
                            &resource_error,
                        )?
                        .is_some() =>
                    {
                        let [first, second] = defer_resource(
                            section_skamp_line_pair(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                first,
                                second,
                            ),
                            &resource_error,
                        )??;
                        SketchConstraintDefinitionInput::Parallel { first, second }
                    }
                    (8, [first, second])
                        if defer_resource(
                            section_skamp_line_pair(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                first,
                                second,
                            ),
                            &resource_error,
                        )?
                        .is_some() =>
                    {
                        let [first, second] = defer_resource(
                            section_skamp_line_pair(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                first,
                                second,
                            ),
                            &resource_error,
                        )??;
                        SketchConstraintDefinitionInput::Equal { first, second }
                    }
                    (9, [first, second])
                        if defer_resource(
                            section_skamp_line_pair(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                first,
                                second,
                            ),
                            &resource_error,
                        )?
                        .is_some() =>
                    {
                        let [first, second] = defer_resource(
                            section_skamp_line_pair(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                first,
                                second,
                            ),
                            &resource_error,
                        )??;
                        SketchConstraintDefinitionInput::Collinear { first, second }
                    }
                    (9, [first, second])
                        if first.sense == 0
                            && second.sense == 0
                            && ((defer_resource(
                                section_skamp_is_line(ctx, definition, first),
                                &resource_error,
                            )? && defer_resource(
                                section_skamp_is_point(ctx, definition, second),
                                &resource_error,
                            )?) || (defer_resource(
                                section_skamp_is_point(ctx, definition, first),
                                &resource_error,
                            )? && defer_resource(
                                section_skamp_is_line(ctx, definition, second),
                                &resource_error,
                            )?)) =>
                    {
                        let (line, point) = if defer_resource(
                            section_skamp_is_line(ctx, definition, first),
                            &resource_error,
                        )? {
                            (first, second)
                        } else {
                            (second, first)
                        };
                        SketchConstraintDefinitionInput::PointOnObject {
                            point: defer_resource(
                                section_skamp_locus(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    point,
                                ),
                                &resource_error,
                            )??,
                            entity: admitted_entity(ctx, sketch, line.entity_id, &resource_error)?,
                        }
                    }
                    (kind @ (10 | 11), [item])
                        if item.sense == 0
                            && defer_resource(
                                section_skamp_is_arc(ctx, definition, item),
                                &resource_error,
                            )? =>
                    {
                        SketchConstraintDefinitionInput::ArcAngle {
                            entity: admitted_entity(ctx, sketch, item.entity_id, &resource_error)?,
                            angle: if kind == 10 {
                                PositiveAngle::QUARTER_TURN
                            } else {
                                PositiveAngle::HALF_TURN
                            },
                        }
                    }
                    (kind @ (12 | 13), [item])
                        if item.sense == 0
                            && defer_resource(
                                section_skamp_is_arc(ctx, definition, item),
                                &resource_error,
                            )? =>
                    {
                        let entity = admitted_entity(ctx, sketch, item.entity_id, &resource_error)?;
                        let first = SketchLocus::Start(defer_resource(
                            entity
                                .try_clone_for_decode(ctx, "creo skamp arc endpoint identity copy"),
                            &resource_error,
                        )?);
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
                            && match definition.trim_entities.as_ref() {
                                Some(table) if table.has_unique_external_ids() => defer_resource(
                                    ctx.admit_iter(
                                        &table.rows,
                                        "creo SKAMP projected trim row search",
                                    )
                                    .map_err(cadmpeg_core::CodecError::from),
                                    &resource_error,
                                )?
                                .any(|row| row.external_id == result.entity_id),
                                _ => false,
                            } =>
                    {
                        let source =
                            admitted_entity(ctx, sketch, source.entity_id, &resource_error)?;
                        let result =
                            admitted_entity(ctx, sketch, result.entity_id, &resource_error)?;
                        let geometry_agrees = match geometry {
                            Some(geometry) => {
                                match geometry.get(&source).zip(geometry.get(&result)) {
                                    Some((source, result)) => defer_resource(
                                        ctx.equal(
                                            source,
                                            result,
                                            "creo SKAMP projected geometry agreement",
                                        ),
                                        &resource_error,
                                    )?,
                                    None => true,
                                }
                            }
                            None => true,
                        };
                        if geometry_agrees {
                            SketchConstraintDefinitionInput::ProjectedCopy { source, result }
                        } else {
                            native_constraint(&resource_error)?
                        }
                    }
                    (33, [item])
                        if skamp.flags == 34
                            && item.sense == 10
                            && unique_bounded_curve_segment(definition, item.entity_id)
                                .is_some() =>
                    {
                        SketchConstraintDefinitionInput::Fixed {
                            entity: admitted_entity(ctx, sketch, item.entity_id, &resource_error)?,
                        }
                    }
                    (14, [axis, first, second])
                        if axis.sense == 0
                            && defer_resource(
                                section_skamp_is_line(ctx, definition, axis),
                                &resource_error,
                            )?
                            && defer_resource(
                                section_skamp_point_locus(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    first,
                                ),
                                &resource_error,
                            )?
                            .is_some()
                            && defer_resource(
                                section_skamp_point_locus(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    second,
                                ),
                                &resource_error,
                            )?
                            .is_some() =>
                    {
                        SketchConstraintDefinitionInput::Symmetric {
                            first: defer_resource(
                                section_skamp_point_locus(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    first,
                                ),
                                &resource_error,
                            )??,
                            second: defer_resource(
                                section_skamp_point_locus(
                                    ctx,
                                    &resource_error,
                                    definition,
                                    sketch,
                                    second,
                                ),
                                &resource_error,
                            )??,
                            axis: admitted_entity(ctx, sketch, axis.entity_id, &resource_error)?,
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
                        if let Some((first, second, axis)) = defer_resource(
                            section_skamp_same_coordinate(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                skamp,
                                active,
                                resolved_points.as_ref(),
                            ),
                            &resource_error,
                        )? {
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
                                            Err(_) => native_constraint(&resource_error)?,
                                        }
                                    }
                                    _ => native_constraint(&resource_error)?,
                                },
                                _ => native_constraint(&resource_error)?,
                            }
                        } else {
                            native_constraint(&resource_error)?
                        }
                    }
                    (35, [first, second]) => {
                        if let Some((point, entity)) = defer_resource(
                            section_skamp_midpoint(
                                ctx,
                                &resource_error,
                                definition,
                                sketch,
                                first,
                                second,
                                geometry,
                            ),
                            &resource_error,
                        )? {
                            SketchConstraintDefinitionInput::Midpoint { point, entity }
                        } else {
                            native_constraint(&resource_error)?
                        }
                    }
                    _ => native_constraint(&resource_error)?,
                }
            } else {
                native_constraint(&resource_error)?
            };
            let incompatible = match geometry {
                Some(geometry) => !defer_resource(
                    sketch_constraint_loci_compatible_with_policy(
                        ctx,
                        &constraint_definition,
                        geometry,
                        !active,
                    ),
                    &resource_error,
                )?,
                None => false,
            };
            if incompatible {
                constraint_definition = native_constraint(&resource_error)?;
            }
            Some((
                SketchConstraint {
                    id: if unique_skamp_id {
                        defer_resource(
                            sketch_constraint_id_admitted(
                                ctx,
                                sketch,
                                format_args!("skamp:{}", skamp.id),
                            ),
                            &resource_error,
                        )??
                    } else {
                        defer_resource(
                            sketch_constraint_id_admitted(
                                ctx,
                                sketch,
                                format_args!("skamp:offset:{}", skamp.offset),
                            ),
                            &resource_error,
                        )??
                    },
                    sketch: defer_resource(
                        sketch.try_clone_for_decode(ctx, "creo skamp constraint sketch identity"),
                        &resource_error,
                    )?,
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
                    native_ref: Some(defer_resource(
                        sketch_native_ref_admitted(ctx, sketch),
                        &resource_error,
                    )?),
                },
                skamp.offset,
            ))
        })();
        if let Some(error) = resource_error.into_inner() {
            return Err(error);
        }
        if let Some(candidate) = candidate {
            ctx.reserve_vec(&mut constraints, 1, "creo skamp constraints")?;
            constraints.push(candidate);
        }
    }
    Ok(constraints)
}

#[cfg(test)]
pub(in super::super) fn sketch_constraint_loci_compatible(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &SketchConstraintDefinitionInput,
    geometry: &BTreeMap<SketchEntityId, SketchGeometry>,
) -> Result<bool, cadmpeg_core::CodecError> {
    sketch_constraint_loci_compatible_with_policy(ctx, definition, geometry, false)
}

fn sketch_constraint_loci_compatible_with_policy(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &SketchConstraintDefinitionInput,
    geometry: &BTreeMap<SketchEntityId, SketchGeometry>,
    allow_unknown_native_endpoints: bool,
) -> Result<bool, cadmpeg_core::CodecError> {
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
                                && native_kind.as_str() == "solver_only_section_entity")
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
                            || native_line_center_allowed && native_kind.as_str() == "line"
                )
            }
        })
    };
    let loci_compatible = match definition {
        SketchConstraintDefinitionInput::CoincidentLoci { loci }
        | SketchConstraintDefinitionInput::Group { elements: loci }
        | SketchConstraintDefinitionInput::Text { elements: loci, .. } => ctx
            .admit_iter(loci, "creo SKAMP compatible locus traversal")?
            .all(locus_compatible),
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
    Ok(loci_compatible
        && definition
            .entity_kind_restriction()
            .is_none_or(|(entity, restriction)| {
                geometry
                    .get(entity)
                    .is_some_and(|geometry| restriction.admits(geometry.definition()))
            }))
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

    fn make_definition(duplicate: bool) -> crate::feature::definitions::FeatureDefinition {
        let row = crate::feature::definitions::FeatureSkamp {
            id: 3,
            kind: 99,
            flags: 0,
            status: 1,
            items: vec![crate::feature::definitions::FeatureSkampItem {
                entity_id: 7,
                sense: 0,
            }],
            offset: 0,
        };
        let rows = if duplicate {
            vec![
                row.clone(),
                crate::feature::definitions::FeatureSkamp { offset: 1, ..row },
            ]
        } else {
            vec![row]
        };
        crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: Some(crate::feature::definitions::FeatureRelationTable {
                declared_count: u32::try_from(rows.len()).expect("fixture value fits u32"),
                entity_ref: None,
                rows: Vec::new(),
                skamps: Some(crate::feature::definitions::SolverSubtable::Declared {
                    header: crate::feature::definitions::FeatureSolverTableHeader {
                        declared_count: u32::try_from(rows.len()).expect("fixture value fits u32"),
                        entity_ref: 1,
                        offset: 0,
                    },
                    rows,
                }),
                triples: None,
                offset: 0,
            }),
            saved_section: None,
            offset: 0,
        }
    }

    #[test]
    fn skamp_constraint_scan_refuses_before_coordinate_search() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let definition = make_definition(false);
        let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#1").expect("sketch");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let error = super::section_skamp_constraints_for_geometry(&ctx, &definition, &sketch, None)
            .expect_err("row search requires work");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::WorkUnits
                && resource.operation == "creo SKAMP coordinate requirement search")
        );
    }

    #[test]
    fn skamp_locus_compatibility_refuses_before_geometry_lookup() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let entity = SketchEntityId::mint("creo:test:entity#1").expect("entity");
        let definition = SketchConstraintDefinitionInput::CoincidentLoci {
            loci: vec![SketchLocus::Entity(entity.clone())],
        };
        let geometry = BTreeMap::from([(
            entity,
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(0.0, 0.0),
            })
            .expect("point geometry"),
        )]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let error =
            sketch_constraint_loci_compatible_with_policy(&ctx, &definition, &geometry, false)
                .expect_err("locus traversal requires work");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::WorkUnits
                && resource.operation == "creo SKAMP compatible locus traversal")
        );
        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            sketch_constraint_loci_compatible_with_policy(ctx, &definition, &geometry, false)
        })
        .expect("service locus traversal admitted"));
    }

    #[test]
    fn skamp_arc_endpoint_copy_refuses_retained_limit() {
        for kind in [12, 13] {
            let mut definition = make_definition(false);
            let Some(crate::feature::definitions::SolverSubtable::Declared { rows, .. }) =
                definition
                    .relations
                    .as_mut()
                    .expect("relations")
                    .skamps
                    .as_mut()
            else {
                panic!("declared SKAMP rows");
            };
            rows[0].kind = kind;
            definition.order_table = Some(crate::feature::definitions::FeatureOrderTable {
                declared_count: 1,
                has_prototype: false,
                entity_ref: None,
                rows: vec![crate::feature::definitions::FeatureOrderRow {
                    external_id: 7,
                    internal_id: 7,
                    bitmask: 0,
                    offset: 0,
                }],
                offset: 0,
            });
            definition.saved_section = Some(crate::feature::definitions::FeatureSavedSection {
                entities: vec![crate::feature::definitions::FeatureSavedEntity::Arc(
                    crate::feature::definitions::FeatureSavedArc {
                        entity_id: 7,
                        center: [Some(0.0); 3],
                        radius: Some(1.0),
                        endpoints: [
                            [Some(1.0), Some(0.0), Some(0.0)],
                            [Some(-1.0), Some(0.0), Some(0.0)],
                        ],
                        parameters: [Some(0.0), Some(std::f64::consts::PI)],
                        body: Vec::new(),
                        offset: 0,
                    },
                )],
                offset: 0,
            });
            let sketch =
                cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#1").expect("sketch");
            let constraints = crate::test_support::assert_retained_boundaries(
                &["creo skamp arc endpoint identity copy"],
                |ctx| {
                    super::section_skamp_constraints_for_geometry(ctx, &definition, &sketch, None)
                },
            );
            assert_eq!(constraints.len(), 1);
            assert!(matches!(
                constraints[0].0.definition.kind(),
                SketchConstraintDefinitionInput::SameCoordinate { .. }
            ));
        }
    }

    #[test]
    fn native_skamp_retained_fields_refuse_below_each_need() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#1")
            .expect("valid sketch identity");
        let arena = DecodeArena::new();
        let entity =
            crate::decode::sketch_ids::sketch_entity_id(&sketch, 7).expect("valid entity identity");
        let geometry = BTreeMap::from([(
            entity,
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(0.0, 0.0),
            })
            .expect("valid point geometry"),
        )]);
        for (duplicate, expected) in [
            (
                false,
                [
                    "creo sketch entity identity",
                    "creo sketch native reference",
                    "creo skamp operand kind",
                    "creo skamp operand field",
                    "creo skamp operand reference",
                    "creo skamp native kind",
                    "creo sketch constraint identity",
                    "creo skamp constraint sketch identity",
                ]
                .as_slice(),
            ),
            (
                true,
                ["creo skamp property key", "creo skamp property value"].as_slice(),
            ),
        ] {
            let definition = make_definition(duplicate);
            let mut observed = std::collections::BTreeSet::new();
            let mut exact_cap = None;
            let mut limit = 0_u64;
            for _ in 0..4096 {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root admitted");
                match super::section_skamp_constraints_for_geometry(
                    &ctx,
                    &definition,
                    &sketch,
                    Some(&geometry),
                ) {
                    Err(cadmpeg_core::CodecError::ResourceLimit(resource)) => {
                        let need = resource
                            .used
                            .checked_add(resource.additional)
                            .expect("retained need");
                        assert!(need > limit);
                        limit = need;
                        assert_eq!(resource.dimension, ResourceDimension::RetainedBytes);
                        observed.insert(resource.operation);
                    }
                    Ok(constraints) => {
                        assert_eq!(constraints.len(), if duplicate { 2 } else { 1 });
                        exact_cap = Some(limit);
                        break;
                    }
                    Err(error) => panic!("unexpected SKAMP error: {error:?}"),
                }
            }
            let exact_cap = exact_cap.expect("one SKAMP fits within scanned cap");
            assert!(exact_cap > 0);
            assert!(
                expected
                    .iter()
                    .all(|operation| observed.contains(operation)),
                "missing admission boundary: {expected:?} vs {observed:?}"
            );
        }
    }

    #[test]
    fn skamp_count_available_and_result_nodes_refuse_at_named_limits() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let mut definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: Some(crate::feature::definitions::FeatureRelationTable {
                declared_count: 1,
                entity_ref: None,
                rows: Vec::new(),
                skamps: Some(crate::feature::definitions::SolverSubtable::Declared {
                    header: crate::feature::definitions::FeatureSolverTableHeader {
                        declared_count: 1,
                        entity_ref: 1,
                        offset: 0,
                    },
                    rows: vec![crate::feature::definitions::FeatureSkamp {
                        id: 3,
                        kind: 99,
                        flags: 0,
                        status: 1,
                        items: vec![crate::feature::definitions::FeatureSkampItem {
                            entity_id: 7,
                            sense: 0,
                        }],
                        offset: 0,
                    }],
                }),
                triples: None,
                offset: 0,
            }),
            saved_section: None,
            offset: 0,
        };
        let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#1")
            .expect("valid sketch identity");
        let entity =
            crate::decode::sketch_ids::sketch_entity_id(&sketch, 7).expect("valid entity identity");
        let mut geometry = BTreeMap::from([(
            entity,
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(0.0, 0.0),
            })
            .expect("valid point geometry"),
        )]);
        let arena = DecodeArena::new();
        for (limit, operation) in [
            (0, "creo skamp ID count nodes"),
            (1, "creo skamp available entity nodes"),
            (2, "creo skamp native entities"),
            (3, "creo skamp native operands"),
            (4, "creo skamp constraints"),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
            let error = super::section_skamp_constraints_for_geometry(
                &ctx,
                &definition,
                &sketch,
                Some(&geometry),
            )
            .expect_err("skamp collection exceeds its limit");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.operation == operation),
                "{error:?}"
            );
        }
        let constraints = crate::decode::with_test_decode_ctx(|ctx| {
            super::section_skamp_constraints_for_geometry(
                ctx,
                &definition,
                &sketch,
                Some(&geometry),
            )
        })
        .expect("service skamp collection");
        assert_eq!(constraints.len(), 1);

        let mut duplicate = definition.clone();
        let Some(relations) = duplicate.relations.as_mut() else {
            panic!("fixture relation table");
        };
        let Some(crate::feature::definitions::SolverSubtable::Declared { header, rows }) =
            relations.skamps.as_mut()
        else {
            panic!("fixture skamp table");
        };
        header.declared_count = 2;
        let mut second = rows[0].clone();
        second.offset = 1;
        rows.push(second);
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 4;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let error = super::section_skamp_constraints_for_geometry(
            &ctx,
            &duplicate,
            &sketch,
            Some(&geometry),
        )
        .expect_err("native property node exceeds its limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == "creo skamp native property nodes"),
            "{error:?}"
        );

        let Some(relations) = definition.relations.as_mut() else {
            panic!("fixture relation table");
        };
        let Some(crate::feature::definitions::SolverSubtable::Declared { rows, .. }) =
            relations.skamps.as_mut()
        else {
            panic!("fixture skamp table");
        };
        rows[0].kind = 3;
        rows[0].status = 0;
        rows[0]
            .items
            .push(crate::feature::definitions::FeatureSkampItem {
                entity_id: 8,
                sense: 0,
            });
        let second_entity =
            crate::decode::sketch_ids::sketch_entity_id(&sketch, 8).expect("valid entity identity");
        geometry.insert(
            second_entity,
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(1.0, 0.0),
            })
            .expect("valid point geometry"),
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 4;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let error = super::section_skamp_constraints_for_geometry(
            &ctx,
            &definition,
            &sketch,
            Some(&geometry),
        )
        .expect_err("typed loci exceed their limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == "creo skamp coincident loci"),
            "{error:?}"
        );
        let constraints = crate::decode::with_test_decode_ctx(|ctx| {
            super::section_skamp_constraints_for_geometry(
                ctx,
                &definition,
                &sketch,
                Some(&geometry),
            )
        })
        .expect("service typed loci");
        assert_eq!(constraints.len(), 1);
    }

    #[test]
    fn typed_skamp_entity_identity_refuses_below_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let sketch = cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#1")
            .expect("valid sketch identity");
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(1),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: Some(crate::feature::definitions::FeatureRelationTable {
                declared_count: 1,
                entity_ref: None,
                rows: Vec::new(),
                skamps: Some(crate::feature::definitions::SolverSubtable::Declared {
                    header: crate::feature::definitions::FeatureSolverTableHeader {
                        declared_count: 1,
                        entity_ref: 1,
                        offset: 0,
                    },
                    rows: vec![crate::feature::definitions::FeatureSkamp {
                        id: 3,
                        kind: 3,
                        flags: 0,
                        status: 0,
                        items: vec![
                            crate::feature::definitions::FeatureSkampItem {
                                entity_id: 7,
                                sense: 0,
                            },
                            crate::feature::definitions::FeatureSkampItem {
                                entity_id: 8,
                                sense: 0,
                            },
                        ],
                        offset: 0,
                    }],
                }),
                triples: None,
                offset: 0,
            }),
            saved_section: None,
            offset: 0,
        };
        let geometry = BTreeMap::from([7, 8].map(|external_id| {
            (
                crate::decode::sketch_ids::sketch_entity_id(&sketch, external_id).expect("entity"),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(f64::from(external_id), 0.0),
                })
                .expect("point"),
            )
        }));
        let arena = DecodeArena::new();
        let mut saw_typed_identity = false;
        let mut exact_cap = None;
        let mut limit = 0_u64;
        for _ in 0..4096 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            match super::section_skamp_constraints_for_geometry(
                &ctx,
                &definition,
                &sketch,
                Some(&geometry),
            ) {
                Err(cadmpeg_core::CodecError::ResourceLimit(resource)) => {
                    let need = resource
                        .used
                        .checked_add(resource.additional)
                        .expect("retained need");
                    assert!(need > limit);
                    limit = need;
                    assert_eq!(resource.dimension, ResourceDimension::RetainedBytes);
                    saw_typed_identity |= resource.operation == "creo sketch entity identity";
                }
                Ok(constraints) => {
                    assert_eq!(constraints.len(), 1);
                    exact_cap = Some(limit);
                    break;
                }
                Err(error) => panic!("unexpected typed SKAMP error: {error:?}"),
            }
        }
        assert!(saw_typed_identity);
        assert!(exact_cap.is_some());
    }

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
        assert!(!crate::decode::with_test_decode_ctx(|ctx| {
            sketch_constraint_loci_compatible_with_policy(ctx, &symmetry, &geometry, false)
        })
        .expect("service locus compatibility admitted"));

        let projected = SketchConstraintDefinitionInput::ProjectedCopy {
            source: first.clone(),
            result: axis.clone(),
        };
        assert!(!crate::decode::with_test_decode_ctx(|ctx| {
            sketch_constraint_loci_compatible_with_policy(ctx, &projected, &geometry, false)
        })
        .expect("service locus compatibility admitted"));

        let mut complete = geometry;
        complete.insert(
            axis.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
                origin: Point2::new(0.0, 0.0),
                direction: Point2::new(0.0, 1.0),
            })
            .expect("valid test fixture"),
        );
        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            sketch_constraint_loci_compatible_with_policy(ctx, &symmetry, &complete, false)
        })
        .expect("service locus compatibility admitted"));
        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            sketch_constraint_loci_compatible_with_policy(ctx, &projected, &complete, false)
        })
        .expect("service locus compatibility admitted"));
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
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: Length::new(2.0).expect("valid test fixture"),
                minor_radius: Length::new(1.0).expect("valid test fixture"),
            },
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
            native_kind: cadmpeg_core::text::NonBlankString::try_from("reference_line")
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
                let compatible = crate::decode::with_test_decode_ctx(|ctx| {
                    sketch_constraint_loci_compatible_with_policy(ctx, relation, geometry, false)
                })
                .expect("service locus compatibility admitted");
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
        assert_eq!(native_kind.as_str(), "creo:skamp:35");
        assert_eq!(entities, &vec![target.clone(), point.id().clone()]);
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
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
                        if native_kind.as_str() == "creo:segtab:verhor"
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
        assert_eq!(native_kind.as_str(), "creo:skamp:35");
        assert_eq!(
            entities,
            &vec![reference_line.id().clone(), point.id().clone()]
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
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
                        if native_kind.as_str() == "reference_line"
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
                    SketchGeometryDefinition::Native { native_kind } if native_kind.as_str() == "line"
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
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
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
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{validation:#?}");
    }
}
