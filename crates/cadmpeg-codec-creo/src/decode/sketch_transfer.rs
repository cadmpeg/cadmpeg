// SPDX-License-Identifier: Apache-2.0
//! Sketch constraint emission and sketch arena transfer.

#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn feature_recipe(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipeKind> {
    current_feature_recipe(&scan.features.operations, feature_id)
        .map(crate::feature::FeatureRecipe::kind)
}

pub(super) fn feature_recipe_effect(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipeEffect> {
    current_feature_recipe(&scan.features.operations, feature_id)
        .map(crate::feature::FeatureRecipe::effect)
}

pub(super) fn feature_section_sweep_semantics_conflict(
    scan: &ContainerScan,
    feature_id: u32,
) -> bool {
    current_feature_operation(&scan.features.operations, feature_id).is_some_and(|operation| {
        operation.recipe_conflict
            || (operation.display_state_conflict
                && operation.recipe.is_none()
                && operation.kind == "Native Feature")
    })
}

pub(super) fn current_additive_feature_recipe(
    operations: &[crate::feature::FeatureOperation],
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipeKind> {
    let recipe = current_feature_recipe(operations, feature_id)?;
    (recipe.effect() == crate::feature::FeatureRecipeEffect::Protrude).then(|| recipe.kind())
}

pub(super) fn first_material_feature_by_definition_order(
    target_feature_id: u32,
    material_definition_offsets: &[(u32, usize)],
) -> bool {
    let mut offsets = BTreeMap::new();
    for &(feature_id, offset) in material_definition_offsets {
        if offsets.insert(feature_id, offset).is_some() {
            return false;
        }
    }
    let Some(target_offset) = offsets.get(&target_feature_id).copied() else {
        return false;
    };
    offsets
        .into_iter()
        .filter(|(feature_id, _)| *feature_id != target_feature_id)
        .all(|(_, offset)| offset > target_offset)
}

pub(super) fn feature_is_first_material_operation(scan: &ContainerScan, feature_id: u32) -> bool {
    let candidate_feature_ids = scan
        .features
        .operations
        .iter()
        .map(|operation| operation.feature_id)
        .collect::<BTreeSet<_>>()
        .into_iter();
    let mut material_definition_offsets = Vec::new();
    for candidate in candidate_feature_ids {
        let Some(operation) = current_feature_operation(&scan.features.operations, candidate)
        else {
            continue;
        };
        let recipe_is_material = operation.recipe.is_some_and(|recipe| {
            matches!(
                recipe.effect(),
                crate::feature::FeatureRecipeEffect::Protrude
                    | crate::feature::FeatureRecipeEffect::Cut
            )
        });
        if !recipe_is_material && !matches!(feature_schema_class(scan, candidate), Some(916 | 917))
        {
            continue;
        }
        let transforms = scan
            .features
            .section_transforms
            .iter()
            .filter(|transform| transform.feature_id == Some(candidate))
            .collect::<Vec<_>>();
        let [transform] = transforms.as_slice() else {
            return false;
        };
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            return false;
        };
        material_definition_offsets.push((candidate, definition.offset));
    }
    first_material_feature_by_definition_order(feature_id, &material_definition_offsets)
}

pub(super) fn current_feature_recipe(
    operations: &[crate::feature::FeatureOperation],
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipe> {
    current_feature_operation(operations, feature_id)?.recipe
}

pub(super) fn current_feature_recipe_parent(
    operations: &[crate::feature::FeatureOperation],
    feature_id: u32,
) -> Option<u32> {
    let operation = current_feature_operation(operations, feature_id)?;
    operation.recipe?;
    operation.parent_feature_id
}

pub(super) fn current_feature_operation(
    operations: &[crate::feature::FeatureOperation],
    feature_id: u32,
) -> Option<&crate::feature::FeatureOperation> {
    let mut matches = operations
        .iter()
        .filter(|operation| operation.feature_id == feature_id);
    let operation = matches.next()?;
    matches.next().is_none().then_some(operation)
}

pub(super) fn feature_schema_class(scan: &ContainerScan, feature_id: u32) -> Option<u32> {
    resolved_feature_schema_class_from_classes(
        &scan.features.operations,
        feature_row_schema_classes(scan, feature_id),
        feature_id,
    )
}

pub(super) fn resolved_feature_schema_class_from_classes(
    operations: &[crate::feature::FeatureOperation],
    classes: BTreeSet<u32>,
    feature_id: u32,
) -> Option<u32> {
    if let Some(schema_class) = current_feature_operation(operations, feature_id)
        .and_then(|operation| operation.root_schema_class)
    {
        return Some(schema_class);
    }
    if !classes.is_empty() {
        let mut classes = classes.into_iter();
        let schema_class = classes.next()?;
        return classes.next().is_none().then_some(schema_class);
    }
    None
}

pub(super) fn feature_row_schema_classes(scan: &ContainerScan, feature_id: u32) -> BTreeSet<u32> {
    row_feature_schema_classes(&scan.features.rows, feature_id)
        .into_iter()
        .chain(row_feature_schema_classes(
            &scan.features.depdb_recipe_rows,
            feature_id,
        ))
        .collect()
}

pub(super) fn row_feature_schema_classes(
    rows: &[crate::feature::FeatureRow],
    feature_id: u32,
) -> BTreeSet<u32> {
    rows.iter()
        .filter(|row| row.feature_id == feature_id)
        .filter_map(|row| row.root_schema_class)
        .collect()
}

pub(super) fn feature_revolution_extent(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<RevolveExtent> {
    unique_feature_revolution_extent_kind(&scan.features.revolution_extents, feature_id).map(
        |kind| match kind {
            crate::feature::FeatureRevolutionExtentKind::FullTurn => RevolveExtent::OneSided {
                termination: Termination::Angle {
                    angle: Angle(std::f64::consts::TAU),
                },
            },
        },
    )
}

pub(super) fn unique_feature_revolution_extent_kind(
    records: &[crate::feature::FeatureRevolutionExtent],
    feature_id: u32,
) -> Option<crate::feature::FeatureRevolutionExtentKind> {
    let mut kinds = records
        .iter()
        .filter(|record| record.feature_id == feature_id)
        .map(|record| record.kind);
    let kind = kinds.next()?;
    kinds.all(|candidate| candidate == kind).then_some(kind)
}

pub(super) fn section_segment_verhor_definition(
    segment: &crate::feature::FeatureSegment,
    sketch: &SketchId,
    entity: SketchEntityId,
) -> Option<SketchConstraintDefinition> {
    let verhor = segment.vertical_horizontal?;
    match (segment.kind, verhor) {
        (crate::feature::FeatureSegmentKind::Line, 0) => {
            Some(SketchConstraintDefinition::Vertical { entity })
        }
        (crate::feature::FeatureSegmentKind::Line, 1) => {
            Some(SketchConstraintDefinition::Horizontal { entity })
        }
        _ => Some(native_section_segment_verhor_definition(
            sketch,
            entity,
            segment.external_id,
            verhor,
        )),
    }
}

pub(super) fn native_section_segment_verhor_definition(
    sketch: &SketchId,
    entity: SketchEntityId,
    external_id: u32,
    verhor: u32,
) -> SketchConstraintDefinition {
    SketchConstraintDefinition::Native {
        native_kind: "creo:segtab:verhor".to_string(),
        native_state: None,
        native_flags: None,
        native_properties: BTreeMap::from([("verhor".to_string(), verhor.to_string())]),
        entities: vec![entity],
        parameter: None,
        operands: vec![SketchNativeOperand {
            native_kind: "segtab_ptr".to_string(),
            native_field: Some("ext_id".to_string()),
            native_role: None,
            object_index: external_id,
            native_ref: Some(sketch_native_ref(sketch)),
        }],
    }
}

pub(super) fn reconcile_constraint_entity_references(
    definition: &mut SketchConstraintDefinition,
    emitted: &BTreeSet<SketchEntityId>,
) -> bool {
    let locus_emitted = |locus: &SketchLocus| match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => emitted.contains(entity),
    };
    match definition {
        SketchConstraintDefinition::Native { entities, .. } => {
            entities.retain(|entity| emitted.contains(entity));
            true
        }
        SketchConstraintDefinition::Coincident { entities }
        | SketchConstraintDefinition::Distance { entities, .. } => {
            entities.iter().all(|entity| emitted.contains(entity))
        }
        SketchConstraintDefinition::CoincidentLoci { loci } => loci.iter().all(locus_emitted),
        SketchConstraintDefinition::SameCoordinate { first, second, .. }
        | SketchConstraintDefinition::TangentLoci { first, second }
        | SketchConstraintDefinition::DistanceLoci { first, second, .. }
        | SketchConstraintDefinition::HorizontalDistance { first, second, .. }
        | SketchConstraintDefinition::VerticalDistance { first, second, .. } => {
            locus_emitted(first) && locus_emitted(second)
        }
        SketchConstraintDefinition::Midpoint { point, entity } => {
            locus_emitted(point) && emitted.contains(entity)
        }
        SketchConstraintDefinition::AtIntersection {
            point,
            first,
            second,
        } => locus_emitted(point) && emitted.contains(first) && emitted.contains(second),
        SketchConstraintDefinition::PointOnObject { point, entity } => {
            locus_emitted(point) && emitted.contains(entity)
        }
        SketchConstraintDefinition::Symmetric {
            first,
            second,
            axis,
        } => locus_emitted(first) && locus_emitted(second) && emitted.contains(axis),
        SketchConstraintDefinition::PointSymmetric {
            first,
            second,
            center,
        } => locus_emitted(first) && locus_emitted(second) && locus_emitted(center),
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
            emitted.contains(first) && emitted.contains(second)
        }
        SketchConstraintDefinition::Horizontal { entity }
        | SketchConstraintDefinition::Vertical { entity }
        | SketchConstraintDefinition::Fixed { entity }
        | SketchConstraintDefinition::Radius { entity, .. }
        | SketchConstraintDefinition::Diameter { entity, .. } => emitted.contains(entity),
        SketchConstraintDefinition::HorizontalPoints { first, second }
        | SketchConstraintDefinition::VerticalPoints { first, second }
        | SketchConstraintDefinition::HorizontalLoci { first, second }
        | SketchConstraintDefinition::VerticalLoci { first, second } => {
            locus_emitted(first) && locus_emitted(second)
        }
        SketchConstraintDefinition::ArcAngle { entity, .. }
        | SketchConstraintDefinition::EllipseAngle { entity, .. } => emitted.contains(entity),
        SketchConstraintDefinition::SnellsLaw {
            incident,
            refracted,
            interface,
            ..
        } => locus_emitted(incident) && locus_emitted(refracted) && emitted.contains(interface),
        SketchConstraintDefinition::Weight { entity, .. } => emitted.contains(entity),
        SketchConstraintDefinition::InternalAlignment { helper, parent, .. } => {
            emitted.contains(helper) && emitted.contains(parent)
        }
        SketchConstraintDefinition::Group { elements }
        | SketchConstraintDefinition::Text { elements, .. } => elements.iter().all(locus_emitted),
        SketchConstraintDefinition::Disabled => true,
        _ => true,
    }
}

pub(super) fn reconcile_constraint_parameter_reference(
    definition: &mut SketchConstraintDefinition,
    emitted: &BTreeSet<ParameterId>,
) -> bool {
    match definition {
        SketchConstraintDefinition::Native { parameter, .. } => {
            if parameter
                .as_ref()
                .is_some_and(|parameter| !emitted.contains(parameter))
            {
                *parameter = None;
            }
            true
        }
        SketchConstraintDefinition::Distance { parameter, .. }
        | SketchConstraintDefinition::DistanceLoci { parameter, .. }
        | SketchConstraintDefinition::HorizontalDistance { parameter, .. }
        | SketchConstraintDefinition::VerticalDistance { parameter, .. }
        | SketchConstraintDefinition::Angle { parameter, .. }
        | SketchConstraintDefinition::Radius { parameter, .. }
        | SketchConstraintDefinition::Diameter { parameter, .. } => emitted.contains(parameter),
        SketchConstraintDefinition::SnellsLaw { parameter, .. }
        | SketchConstraintDefinition::Weight { parameter, .. } => emitted.contains(parameter),
        SketchConstraintDefinition::Coincident { .. }
        | SketchConstraintDefinition::CoincidentLoci { .. }
        | SketchConstraintDefinition::SameCoordinate { .. }
        | SketchConstraintDefinition::Midpoint { .. }
        | SketchConstraintDefinition::Concentric { .. }
        | SketchConstraintDefinition::Coradial { .. }
        | SketchConstraintDefinition::Collinear { .. }
        | SketchConstraintDefinition::Symmetric { .. }
        | SketchConstraintDefinition::PointSymmetric { .. }
        | SketchConstraintDefinition::Horizontal { .. }
        | SketchConstraintDefinition::Vertical { .. }
        | SketchConstraintDefinition::Parallel { .. }
        | SketchConstraintDefinition::Perpendicular { .. }
        | SketchConstraintDefinition::Tangent { .. }
        | SketchConstraintDefinition::TangentLoci { .. }
        | SketchConstraintDefinition::Equal { .. }
        | SketchConstraintDefinition::Fixed { .. } => true,
        SketchConstraintDefinition::Disabled
        | SketchConstraintDefinition::PointOnObject { .. }
        | SketchConstraintDefinition::AtIntersection { .. }
        | SketchConstraintDefinition::HorizontalPoints { .. }
        | SketchConstraintDefinition::VerticalPoints { .. }
        | SketchConstraintDefinition::ArcAngle { .. }
        | SketchConstraintDefinition::EllipseAngle { .. }
        | SketchConstraintDefinition::InternalAlignment { .. }
        | SketchConstraintDefinition::Group { .. }
        | SketchConstraintDefinition::Text { .. } => true,
        _ => true,
    }
}

pub(super) fn close_sketch_constraint_parameter_references(ir: &mut CadIr) {
    let emitted = ir
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.id.clone())
        .collect::<BTreeSet<_>>();
    ir.model.sketch_constraints.retain_mut(|constraint| {
        reconcile_constraint_parameter_reference(&mut constraint.definition, &emitted)
    });
}

pub(super) fn joined_relation_incidence(
    definition: &crate::feature::FeatureDefinition,
    relation_id: u32,
) -> Option<&crate::feature::FeatureSkamp> {
    joined_relation_incidence_link(definition, relation_id).map(|(_, incidence)| incidence)
}

pub(super) fn joined_relation_incidence_link(
    definition: &crate::feature::FeatureDefinition,
    relation_id: u32,
) -> Option<(
    &crate::feature::FeatureRelationTriple,
    &crate::feature::FeatureSkamp,
)> {
    let Some(relations) = &definition.relations else {
        return None;
    };
    if !feature_solver_table_complete(relations.triples_header.as_ref(), relations.triples.len())
        || !feature_solver_table_complete(relations.skamp_header.as_ref(), relations.skamps.len())
    {
        return None;
    }
    let joins = relations
        .triples
        .iter()
        .filter(|triple| triple.relation_id == Some(relation_id))
        .filter_map(|triple| triple.skamp_id.map(|incidence_id| (triple, incidence_id)))
        .collect::<Vec<_>>();
    let [(join, incidence_id)] = joins.as_slice() else {
        return None;
    };
    let incidences = relations
        .skamps
        .iter()
        .filter(|skamp| skamp.id == *incidence_id)
        .collect::<Vec<_>>();
    let [incidence] = incidences.as_slice() else {
        return None;
    };
    Some((*join, *incidence))
}

pub(super) fn section_solver_relation_is_disabled(
    definition: &crate::feature::FeatureDefinition,
    relation_id: u32,
) -> bool {
    let Some(relations) = definition
        .relations
        .as_ref()
        .filter(|relations| feature_relation_table_complete(relations))
    else {
        return false;
    };
    if relations
        .rows
        .iter()
        .filter(|relation| relation.relation_id == relation_id)
        .count()
        != 1
    {
        return false;
    }
    joined_relation_incidence(definition, relation_id)
        .is_some_and(|incidence| !section_skamp_active(incidence.status))
}

pub(super) fn section_solver_equation_is_disabled(
    definition: &crate::feature::FeatureDefinition,
    equation_id: u32,
) -> bool {
    let Some(relations) = &definition.relations else {
        return false;
    };
    if !feature_solver_table_complete(relations.triples_header.as_ref(), relations.triples.len())
        || !feature_solver_table_complete(relations.skamp_header.as_ref(), relations.skamps.len())
    {
        return false;
    }
    let incidence_ids = relations
        .triples
        .iter()
        .filter(|triple| triple.equation_id == Some(equation_id))
        .filter_map(|triple| triple.skamp_id)
        .collect::<Vec<_>>();
    let [incidence_id] = incidence_ids.as_slice() else {
        return false;
    };
    let incidences = relations
        .skamps
        .iter()
        .filter(|skamp| skamp.id == *incidence_id)
        .collect::<Vec<_>>();
    let [incidence] = incidences.as_slice() else {
        return false;
    };
    !section_skamp_active(incidence.status)
}

pub(super) fn relation_incidence(
    definition: &crate::feature::FeatureDefinition,
    relation_id: u32,
) -> Option<&crate::feature::FeatureSkamp> {
    let incidence = joined_relation_incidence(definition, relation_id)?;
    section_skamp_active(incidence.status).then_some(incidence)
}

pub(super) fn relation_incidence_entities(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    relation_id: u32,
) -> Vec<SketchEntityId> {
    let Some(incidence) = relation_incidence(definition, relation_id) else {
        return Vec::new();
    };
    incidence
        .items
        .iter()
        .map(|item| sketch_entity_id(sketch, item.entity_id))
        .collect()
}

pub(super) fn joined_relation_incidence_entities(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    relation_id: u32,
) -> Vec<SketchEntityId> {
    let Some(incidence) = joined_relation_incidence(definition, relation_id) else {
        return Vec::new();
    };
    incidence
        .items
        .iter()
        .map(|item| sketch_entity_id(sketch, item.entity_id))
        .collect()
}

pub(super) fn relation_incidence_loci(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    relation_id: u32,
) -> Option<[SketchLocus; 2]> {
    let incidence = relation_incidence(definition, relation_id)?;
    let [first, second] = incidence.items.as_slice() else {
        return None;
    };
    Some([
        section_skamp_locus(definition, sketch, first)?,
        section_skamp_locus(definition, sketch, second)?,
    ])
}

pub(super) fn section_angular_entities(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    segments: &[crate::feature::FeatureSegment],
    vectors: [[Option<u32>; 4]; 3],
    known_entities: &BTreeSet<u32>,
) -> Option<[SketchEntityId; 2]> {
    let [Some(first_internal), Some(second_internal), None, Some(1)] = vectors[0] else {
        return None;
    };
    let order_table = definition.order_table.as_ref()?;
    let external_id = |internal_id| {
        let external_id = order_table.external_id(internal_id)?;
        let matching_segments = segments
            .iter()
            .filter(|segment| {
                segment.external_id == external_id
                    && segment.kind == crate::feature::FeatureSegmentKind::Line
            })
            .collect::<Vec<_>>();
        (known_entities.contains(&external_id) && matching_segments.len() == 1)
            .then_some(external_id)
    };
    let [first, second] = [first_internal, second_internal].map(external_id);
    let [Some(first), Some(second)] = [first, second] else {
        return None;
    };
    (first != second)
        .then(|| [first, second].map(|external_id| sketch_entity_id(sketch, external_id)))
}

pub(super) fn native_section_segment_radius_definition(
    sketch: &SketchId,
    entity: SketchEntityId,
    external_id: u32,
    field: &str,
    dimension_ordinal: u32,
) -> SketchConstraintDefinition {
    SketchConstraintDefinition::Native {
        native_kind: format!("creo:segtab:{field}"),
        native_state: None,
        native_flags: None,
        native_properties: BTreeMap::from([(
            "dimension_ordinal".to_string(),
            dimension_ordinal.to_string(),
        )]),
        entities: vec![entity],
        parameter: None,
        operands: vec![
            SketchNativeOperand {
                native_kind: "segtab_ptr".to_string(),
                native_field: Some("ext_id".to_string()),
                native_role: None,
                object_index: external_id,
                native_ref: Some(sketch_native_ref(sketch)),
            },
            SketchNativeOperand {
                native_kind: "dimension_ordinal".to_string(),
                native_field: Some(field.to_string()),
                native_role: None,
                object_index: dimension_ordinal,
                native_ref: Some(sketch_native_ref(sketch)),
            },
        ],
    }
}

pub(super) fn section_segment_radius_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let unique_segment_ids = unique_section_segment_external_ids(definition);
    let typed = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .flat_map(|segment| {
            let suffix = section_segment_identity_suffix(&unique_segment_ids, segment);
            [
                ("radius", segment.radius_ref),
                ("radius2", segment.radius2_ref),
            ]
            .into_iter()
            .filter_map(move |(field, ordinal)| {
                Some((
                    suffix.clone(),
                    segment.external_id,
                    field,
                    ordinal?,
                    segment.offset,
                    None,
                ))
            })
        });
    let opaque = definition
        .segments
        .iter()
        .flat_map(|table| &table.opaque_rows)
        .flat_map(|segment| {
            let suffix = opaque_section_segment_identity_suffix(&unique_segment_ids, segment);
            [
                ("radius", segment.radius_ref),
                ("radius2", segment.radius2_ref),
            ]
            .into_iter()
            .filter_map(move |(field, ordinal)| {
                Some((
                    suffix.clone(),
                    segment.external_id,
                    field,
                    ordinal?,
                    segment.offset,
                    None,
                ))
            })
        });
    let circles = definition
        .segments
        .iter()
        .flat_map(|table| &table.circle_rows)
        .map(|segment| {
            let suffix = if unique_segment_ids.contains(&segment.external_id) {
                segment.external_id.to_string()
            } else {
                format!("circle:offset:{}", segment.offset)
            };
            let parameter = usize::try_from(segment.radius_ref)
                .ok()
                .and_then(|ordinal| {
                    resolved_feature_dimension_parameter(
                        sketch,
                        definition.dimensions.as_ref()?,
                        ordinal,
                    )
                });
            (
                suffix,
                segment.external_id,
                "radius",
                segment.radius_ref,
                segment.offset,
                parameter,
            )
        });
    typed
        .chain(circles)
        .chain(opaque)
        .map(
            |(suffix, external_id, field, ordinal, offset, typed_circle)| {
                let entity = sketch_entity_id(sketch, &suffix);
                let (definition, kind) = match typed_circle {
                    Some((dimension, parameter)) if matches!(dimension.dimension_type, 3 | 4) => (
                        circular_dimension_constraint(entity, parameter, dimension.dimension_type),
                        if dimension.dimension_type == 4 {
                            "diameter"
                        } else {
                            "radius"
                        },
                    ),
                    _ => (
                        native_section_segment_radius_definition(
                            sketch,
                            entity,
                            external_id,
                            field,
                            ordinal,
                        ),
                        if field == "radius2" {
                            "segtab-radius2"
                        } else {
                            "segtab-radius"
                        },
                    ),
                };
                (
                    SketchConstraint {
                        id: sketch_constraint_id(sketch, format_args!("{kind}:{suffix}")),
                        sketch: sketch.clone(),
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
                        native_ref: Some(sketch_native_ref(sketch)),
                    },
                    offset,
                )
            },
        )
        .collect()
}

pub(super) fn circular_dimension_constraint(
    entity: SketchEntityId,
    parameter: ParameterId,
    dimension_type: u32,
) -> SketchConstraintDefinition {
    if dimension_type == 4 {
        SketchConstraintDefinition::Diameter { entity, parameter }
    } else {
        SketchConstraintDefinition::Radius { entity, parameter }
    }
}

pub(super) fn section_dimension_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let Some(relations) = &definition.relations else {
        return Vec::new();
    };
    let segments = section_segment_rows(definition);
    let known_entities = section_entity_external_ids(definition);
    relations
        .rows
        .iter()
        .map(|relation| {
            let unique_relation_id = feature_relation_table_complete(relations)
                && relations
                    .rows
                    .iter()
                    .filter(|candidate| candidate.relation_id == relation.relation_id)
                    .count()
                    == 1;
            let dimension = definition.dimensions.as_ref().and_then(|dimensions| {
                resolved_feature_dimension_parameter(
                    sketch,
                    dimensions,
                    usize::try_from(relation.dimension_id).ok()?,
                )
            });
            let parameter = dimension.as_ref().map(|(_, parameter)| parameter.clone());
            let joined_incidence_link = unique_relation_id
                .then(|| joined_relation_incidence_link(definition, relation.relation_id))
                .flatten();
            let joined_incidence = joined_incidence_link.map(|(_, incidence)| incidence);
            let typed = (|| {
                unique_relation_id.then_some(())?;
                let (dimension, _) = dimension.as_ref()?;
                let parameter = parameter.clone()?;
                if relation.relation_type == 1
                    && dimension.value_unit == crate::feature::DimensionUnit::Radians
                {
                    let [first, second] = section_angular_entities(
                        definition,
                        sketch,
                        segments,
                        relation.operand_vectors?,
                        &known_entities,
                    )?;
                    return Some(SketchConstraintDefinition::Angle {
                        first,
                        second,
                        parameter,
                    });
                }
                if relation.relation_type == 0
                    && matches!(relation.sign, 0 | 1 | 0xf6)
                    && dimension.value_unit == crate::feature::DimensionUnit::SchemaDefined
                    && dimension.value == Some(0.0)
                {
                    let vectors = relation.operand_vectors?;
                    if section_linear_distance_vectors(vectors) {
                        let [Some(first_id), Some(second_id), _, _] = vectors[0] else {
                            return None;
                        };
                        let incidence = joined_incidence?;
                        let [item] = incidence.items.as_slice() else {
                            return None;
                        };
                        if !section_skamp_active(incidence.status) {
                            return None;
                        }
                        let expected_coordinate = match incidence.kind {
                            1 => 1,
                            2 => 0,
                            _ => return None,
                        };
                        if item.sense != 0 {
                            return None;
                        }
                        let measured = unique_section_skamp_segment(definition, item.entity_id)?;
                        if measured.kind == crate::feature::FeatureSegmentKind::Line
                            && (measured.point_ids == [first_id, second_id]
                                || measured.point_ids == [second_id, first_id])
                            && measured.vertical_horizontal == Some(expected_coordinate)
                            && known_entities.contains(&measured.external_id)
                        {
                            let entity = sketch_entity_id(sketch, measured.external_id);
                            return Some(if incidence.kind == 1 {
                                SketchConstraintDefinition::Horizontal { entity }
                            } else {
                                SketchConstraintDefinition::Vertical { entity }
                            });
                        }
                    }
                }
                if dimension.value_unit != crate::feature::DimensionUnit::Millimeters {
                    return None;
                }
                if relation.relation_type == 5 && relation.sign == 1 {
                    let segment = section_type5_radius_arc(definition, relation)?;
                    return Some(circular_dimension_constraint(
                        sketch_entity_id(sketch, segment.external_id),
                        parameter,
                        dimension.dimension_type,
                    ));
                }
                if relation.relation_type == 14
                    && relation.sign == 1
                    && matches!(dimension.dimension_type, 1..=5)
                    && relation.operand_vectors?[1] == [Some(0); 4]
                    && relation.operand_vectors?[2] == [Some(15), Some(0), Some(0), Some(0)]
                {
                    let vectors = relation.operand_vectors?;
                    let [Some(radius_id), Some(0), Some(0), Some(0)] = vectors[0] else {
                        return None;
                    };
                    let matching = segments
                        .iter()
                        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
                        .map(|segment| (segment.external_id, segment.radius_ref))
                        .chain(
                            definition
                                .segments
                                .iter()
                                .flat_map(|table| &table.circle_rows)
                                .map(|segment| (segment.external_id, Some(segment.radius_ref))),
                        )
                        .filter(|(_, radius_ref)| *radius_ref == Some(radius_id))
                        .collect::<Vec<_>>();
                    let [(external_id, _)] = matching.as_slice() else {
                        return None;
                    };
                    known_entities.contains(external_id).then_some(())?;
                    return Some(circular_dimension_constraint(
                        sketch_entity_id(sketch, *external_id),
                        parameter,
                        dimension.dimension_type,
                    ));
                }
                if relation.relation_type != 0 || !matches!(relation.sign, 0 | 1 | 0xf6) {
                    return None;
                }
                if let Some(vectors) = relation.operand_vectors {
                    if section_linear_distance_vectors(vectors) {
                        if let [Some(first_id), Some(second_id), _, _] = vectors[0] {
                            let matching = segments
                                .iter()
                                .filter(|segment| {
                                    segment.point_ids == [first_id, second_id]
                                        || segment.point_ids == [second_id, first_id]
                                })
                                .collect::<Vec<_>>();
                            if let [measured] = matching.as_slice() {
                                if known_entities.contains(&measured.external_id) {
                                    let entity = sketch_entity_id(sketch, measured.external_id);
                                    let [first, second] =
                                        if measured.point_ids == [first_id, second_id] {
                                            [
                                                SketchLocus::Start(entity.clone()),
                                                SketchLocus::End(entity),
                                            ]
                                        } else {
                                            [
                                                SketchLocus::End(entity.clone()),
                                                SketchLocus::Start(entity),
                                            ]
                                        };
                                    match section_line_fixed_coordinate(definition, measured) {
                                        Some(0) => {
                                            return Some(
                                                SketchConstraintDefinition::VerticalDistance {
                                                    first,
                                                    second,
                                                    parameter,
                                                },
                                            );
                                        }
                                        Some(1) => {
                                            return Some(
                                                SketchConstraintDefinition::HorizontalDistance {
                                                    first,
                                                    second,
                                                    parameter,
                                                },
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            let points = resolved_section_points(definition);
                            if let (Some(first_point), Some(second_point)) =
                                (points.get(&first_id), points.get(&second_id))
                            {
                                let scale = first_point
                                    .iter()
                                    .chain(second_point)
                                    .map(|coordinate| coordinate.abs())
                                    .fold(1.0, f64::max);
                                let same_u =
                                    (first_point[0] - second_point[0]).abs() <= 1e-9 * scale;
                                let same_v =
                                    (first_point[1] - second_point[1]).abs() <= 1e-9 * scale;
                                if same_u != same_v {
                                    if let (Some(first), Some(second)) = (
                                        section_point_locus(definition, sketch, first_id),
                                        section_point_locus(definition, sketch, second_id),
                                    ) {
                                        return Some(if same_u {
                                            SketchConstraintDefinition::VerticalDistance {
                                                first,
                                                second,
                                                parameter,
                                            }
                                        } else {
                                            SketchConstraintDefinition::HorizontalDistance {
                                                first,
                                                second,
                                                parameter,
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some([first, second]) =
                    relation_incidence_loci(definition, sketch, relation.relation_id)
                {
                    return Some(SketchConstraintDefinition::DistanceLoci {
                        first,
                        second,
                        parameter,
                    });
                }
                if let Some(incidence) =
                    joined_incidence.filter(|incidence| !section_skamp_active(incidence.status))
                {
                    if let [first, second] = incidence.items.as_slice() {
                        if let (Some(first), Some(second)) = (
                            section_skamp_locus(definition, sketch, first),
                            section_skamp_locus(definition, sketch, second),
                        ) {
                            return Some(SketchConstraintDefinition::DistanceLoci {
                                first,
                                second,
                                parameter,
                            });
                        }
                    }
                    if !incidence.items.is_empty() {
                        return Some(SketchConstraintDefinition::Distance {
                            entities: incidence
                                .items
                                .iter()
                                .map(|item| sketch_entity_id(sketch, item.entity_id))
                                .collect(),
                            parameter,
                        });
                    }
                }
                let entities =
                    relation_incidence_entities(definition, sketch, relation.relation_id);
                (!entities.is_empty()).then_some(SketchConstraintDefinition::Distance {
                    entities,
                    parameter,
                })
            })();
            let incidence_entities = if unique_relation_id {
                joined_relation_incidence_entities(definition, sketch, relation.relation_id)
            } else {
                Vec::new()
            };
            let active = joined_incidence.map(|incidence| section_skamp_active(incidence.status));
            let constraint_definition =
                typed.unwrap_or_else(|| SketchConstraintDefinition::Native {
                    native_kind: format!("creo:relation:{}", relation.relation_type),
                    native_state: Some(u64::from(relation.used)),
                    native_flags: None,
                    native_properties: {
                        let mut properties = BTreeMap::from([
                            (
                                "dimension_id".to_string(),
                                relation.dimension_id.to_string(),
                            ),
                            ("sign".to_string(), relation.sign.to_string()),
                        ]);
                        if !unique_relation_id {
                            properties.insert(
                                "relation_id".to_string(),
                                relation.relation_id.to_string(),
                            );
                        }
                        properties
                    },
                    entities: incidence_entities,
                    parameter,
                    operands: {
                        let native_ref = sketch_native_ref(sketch);
                        let mut operands = Vec::new();
                        if unique_relation_id {
                            operands.push(SketchNativeOperand {
                                native_kind: "relat_ptr".to_string(),
                                native_field: None,
                                native_role: None,
                                object_index: relation.relation_id,
                                native_ref: Some(native_ref.clone()),
                            });
                        }
                        if let Some(incidence) = joined_incidence {
                            operands.push(SketchNativeOperand {
                                native_kind: "skamp_ptr".to_string(),
                                native_field: Some("triples_ptr.skamp_id".to_string()),
                                native_role: None,
                                object_index: incidence.id,
                                native_ref: Some(native_ref.clone()),
                            });
                        }
                        if let Some(equation_id) =
                            joined_incidence_link.and_then(|(join, _)| join.equation_id)
                        {
                            operands.push(SketchNativeOperand {
                                native_kind: "triples_ptr".to_string(),
                                native_field: Some("equation_id".to_string()),
                                native_role: None,
                                object_index: equation_id,
                                native_ref: Some(native_ref.clone()),
                            });
                        }
                        if let Some(vectors) = relation.operand_vectors {
                            for (vector, values) in ["a", "b", "c"].into_iter().zip(vectors) {
                                operands.extend(values.into_iter().enumerate().filter_map(
                                    |(slot, value)| {
                                        value.map(|object_index| SketchNativeOperand {
                                            native_kind: "relat_ptr".to_string(),
                                            native_field: Some(format!("{vector}[{slot}]")),
                                            native_role: None,
                                            object_index,
                                            native_ref: Some(native_ref.clone()),
                                        })
                                    },
                                ));
                            }
                        }
                        operands
                    },
                });
            (
                SketchConstraint {
                    id: if unique_relation_id {
                        sketch_constraint_id(
                            sketch,
                            format_args!("relation:{}", relation.relation_id),
                        )
                    } else {
                        sketch_constraint_id(
                            sketch,
                            format_args!("relation:offset:{}", relation.offset),
                        )
                    },
                    sketch: sketch.clone(),
                    definition: constraint_definition,
                    name: None,
                    driving: None,
                    active,
                    virtual_space: None,
                    visible: None,
                    orientation: None,
                    label_distance: None,
                    label_position: None,
                    metadata: None,
                    native_ref: Some(sketch_native_ref(sketch)),
                },
                relation.offset,
            )
        })
        .collect()
}

pub(super) fn section_linear_distance_vectors(vectors: [[Option<u32>; 4]; 3]) -> bool {
    vectors[0][2..] == [None, Some(1)]
        && matches!(
            vectors[1],
            [Some(0), Some(0), Some(0), Some(0)] | [Some(1), Some(1), Some(0), Some(1)]
        )
        && vectors[2] == [Some(15), Some(16), Some(15), Some(1)]
}

pub(super) fn section_point_locus(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    point_id: u32,
) -> Option<SketchLocus> {
    let unique_entities = unique_section_segment_external_ids(definition);
    definition
        .segments
        .as_ref()?
        .rows
        .iter()
        .filter(|segment| unique_entities.contains(&segment.external_id))
        .filter_map(|segment| {
            let entity = sketch_entity_id(sketch, segment.external_id);
            let locus = match segment.kind {
                crate::feature::FeatureSegmentKind::Point
                    if segment.point_ids[0] == point_id && segment.point_ids[1] == point_id =>
                {
                    SketchLocus::Entity(entity)
                }
                crate::feature::FeatureSegmentKind::Line if segment.point_ids[0] == point_id => {
                    SketchLocus::Start(entity)
                }
                crate::feature::FeatureSegmentKind::Line if segment.point_ids[1] == point_id => {
                    SketchLocus::End(entity)
                }
                crate::feature::FeatureSegmentKind::Arc if segment.point_ids[0] == point_id => {
                    SketchLocus::End(entity)
                }
                crate::feature::FeatureSegmentKind::Arc if segment.point_ids[1] == point_id => {
                    SketchLocus::Start(entity)
                }
                _ => return None,
            };
            Some((segment.offset, locus))
        })
        .min_by_key(|(offset, _)| *offset)
        .map(|(_, locus)| locus)
}

pub(super) fn unique_circle_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureCircleSegment> {
    let segments = definition.segments.as_ref()?;
    let segment = segments
        .circle_rows
        .iter()
        .find(|segment| segment.external_id == external_id)?;
    (segments.external_id_count(external_id) == 1).then_some(segment)
}

pub(super) fn unique_point_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeaturePointSegment> {
    let segments = definition.segments.as_ref()?;
    let segment = segments
        .point_rows
        .iter()
        .find(|segment| segment.external_id == external_id)?;
    (segments.external_id_count(external_id) == 1).then_some(segment)
}

pub(super) fn unique_centered_line_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureCenteredLineSegment> {
    let segments = definition.segments.as_ref()?;
    let segment = segments
        .centered_line_rows
        .iter()
        .find(|segment| segment.external_id == external_id)?;
    (segments.external_id_count(external_id) == 1).then_some(segment)
}

pub(super) fn unique_reference_line_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureReferenceLineSegment> {
    let segments = definition.segments.as_ref()?;
    let segment = segments
        .reference_line_rows
        .iter()
        .find(|segment| segment.external_id == external_id)?;
    (segments.external_id_count(external_id) == 1).then_some(segment)
}

pub(super) fn unique_bounded_curve_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureBoundedCurveSegment> {
    let segments = definition.segments.as_ref()?;
    let segment = segments
        .bounded_curve_rows
        .iter()
        .find(|segment| segment.external_id == external_id)?;
    (segments.external_id_count(external_id) == 1).then_some(segment)
}

pub(super) fn section_skamp_locus(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    let entity = sketch_entity_id(sketch, item.entity_id);
    if let Some(family) = solver_only_section_entity_family(definition, item.entity_id) {
        return section_entity_family_locus(family, entity, item.sense);
    }
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        return match (segment.kind, item.sense) {
            (_, 0) => Some(SketchLocus::Entity(entity)),
            (crate::feature::FeatureSegmentKind::Arc, 2) => Some(SketchLocus::End(entity)),
            (crate::feature::FeatureSegmentKind::Arc, 3) => Some(SketchLocus::Start(entity)),
            (crate::feature::FeatureSegmentKind::Arc, 4) => Some(SketchLocus::Center(entity)),
            (_, 2) => Some(SketchLocus::Start(entity)),
            (_, 3) => Some(SketchLocus::End(entity)),
            _ => None,
        };
    }
    if let Some(segment) = unique_reference_line_segment(definition, item.entity_id) {
        return match item.sense {
            0 => Some(SketchLocus::Entity(entity)),
            2 if segment.point_ids[0].is_some() => Some(SketchLocus::Start(entity)),
            3 if segment.point_ids[1].is_some() => Some(SketchLocus::End(entity)),
            _ => None,
        };
    }
    if unique_bounded_curve_segment(definition, item.entity_id).is_some() {
        return match item.sense {
            0 => Some(SketchLocus::Entity(entity)),
            2 => Some(SketchLocus::Start(entity)),
            3 => Some(SketchLocus::End(entity)),
            _ => None,
        };
    }
    if unique_point_segment(definition, item.entity_id).is_some() {
        return matches!(item.sense, 0 | 4).then_some(SketchLocus::Entity(entity));
    }
    if unique_centered_line_segment(definition, item.entity_id).is_some() {
        return match item.sense {
            0 => Some(SketchLocus::Entity(entity)),
            2 => Some(SketchLocus::Start(entity)),
            3 => Some(SketchLocus::End(entity)),
            4 => Some(SketchLocus::Center(entity)),
            _ => None,
        };
    }
    if unique_circle_segment(definition, item.entity_id).is_some() {
        return match item.sense {
            0 => Some(SketchLocus::Entity(entity)),
            4 => Some(SketchLocus::Center(entity)),
            _ => None,
        };
    }
    if definition
        .segments
        .iter()
        .flat_map(|segments| {
            segments
                .rows
                .iter()
                .map(|segment| segment.external_id)
                .chain(
                    segments
                        .circle_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    segments
                        .point_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    segments
                        .centered_line_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    segments
                        .reference_line_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    segments
                        .bounded_curve_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    segments
                        .conic_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    segments
                        .opaque_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
        })
        .any(|external_id| external_id == item.entity_id)
    {
        return section_incidence_curve_locus(definition, entity, item);
    }
    let saved = section_saved_entity(definition, item.entity_id)?;
    match (saved, item.sense) {
        (_, 0) => Some(SketchLocus::Entity(entity)),
        (crate::feature::FeatureSavedEntity::Line(_), 2) => Some(SketchLocus::Start(entity)),
        (crate::feature::FeatureSavedEntity::Line(_), 3) => Some(SketchLocus::End(entity)),
        (crate::feature::FeatureSavedEntity::Arc(_), 2) => Some(SketchLocus::End(entity)),
        (crate::feature::FeatureSavedEntity::Arc(_), 3) => Some(SketchLocus::Start(entity)),
        (
            crate::feature::FeatureSavedEntity::Arc(_)
            | crate::feature::FeatureSavedEntity::Circle(_)
            | crate::feature::FeatureSavedEntity::Conic(_),
            4,
        ) => Some(SketchLocus::Center(entity)),
        _ => None,
    }
}

pub(super) fn section_incidence_curve_locus(
    definition: &crate::feature::FeatureDefinition,
    entity: SketchEntityId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    section_entity_family_locus(
        unique_section_incidence_curve_family(definition, item.entity_id)?,
        entity,
        item.sense,
    )
}

pub(super) fn section_entity_family_locus(
    family: SectionEntityIncidenceFamily,
    entity: SketchEntityId,
    sense: u32,
) -> Option<SketchLocus> {
    match (family, sense) {
        (SectionEntityIncidenceFamily::Point, 0) => Some(SketchLocus::Entity(entity)),
        (
            SectionEntityIncidenceFamily::BoundedCurve
            | SectionEntityIncidenceFamily::Line
            | SectionEntityIncidenceFamily::Arc,
            0,
        ) => Some(SketchLocus::Entity(entity)),
        (
            SectionEntityIncidenceFamily::BoundedCurve
            | SectionEntityIncidenceFamily::Line
            | SectionEntityIncidenceFamily::Arc,
            2,
        ) => Some(SketchLocus::Start(entity)),
        (
            SectionEntityIncidenceFamily::BoundedCurve
            | SectionEntityIncidenceFamily::Line
            | SectionEntityIncidenceFamily::Arc,
            3,
        ) => Some(SketchLocus::End(entity)),
        (SectionEntityIncidenceFamily::Arc | SectionEntityIncidenceFamily::Circular, 4) => {
            Some(SketchLocus::Center(entity))
        }
        (SectionEntityIncidenceFamily::Circular, 0) => Some(SketchLocus::Entity(entity)),
        _ => None,
    }
}

pub(super) fn section_skamp_endpoint(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    matches!(item.sense, 2 | 3)
        .then(|| section_skamp_locus(definition, sketch, item))
        .flatten()
}

pub(super) fn section_skamp_shared_endpoint(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    entity: &crate::feature::FeatureSkampItem,
    selected: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    (entity.sense == 0).then_some(())?;
    let segment = unique_section_skamp_segment(definition, entity.entity_id)?;
    matches!(
        segment.kind,
        crate::feature::FeatureSegmentKind::Line | crate::feature::FeatureSegmentKind::Arc
    )
    .then_some(())?;
    let selected_point = section_skamp_selected_point_id(definition, selected)?;
    let mut endpoints = segment
        .point_ids
        .iter()
        .enumerate()
        .filter(|(_, point_id)| **point_id == selected_point);
    let (endpoint, _) = endpoints.next()?;
    endpoints.next().is_none().then_some(())?;
    section_skamp_locus(
        definition,
        sketch,
        &crate::feature::FeatureSkampItem {
            entity_id: entity.entity_id,
            sense: if endpoint == 0 { 2 } else { 3 },
        },
    )
}

pub(super) fn section_skamp_tangent_loci(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    first: &crate::feature::FeatureSkampItem,
    second: &crate::feature::FeatureSkampItem,
    active: bool,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Option<[SketchLocus; 2]> {
    let selected_locus = |item| {
        if active {
            section_skamp_endpoint(definition, sketch, item)
        } else if matches!(item.sense, 2 | 3) {
            section_skamp_incidence_locus(definition, sketch, item, geometry)
        } else {
            None
        }
    };
    if let (Some(first), Some(second)) = (selected_locus(first), selected_locus(second)) {
        return Some([first, second]);
    }
    [(first, second), (second, first)]
        .into_iter()
        .find_map(|(entity, selected)| {
            Some([
                section_skamp_shared_endpoint(definition, sketch, entity, selected)?,
                selected_locus(selected)?,
            ])
        })
        .map(|loci| {
            if first.sense == 0 {
                loci
            } else {
                [loci[1].clone(), loci[0].clone()]
            }
        })
}

pub(super) fn section_skamp_point_locus(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    if item.sense == 0 && section_skamp_is_point(definition, item) {
        return section_skamp_locus(definition, sketch, item);
    }
    matches!(item.sense, 2..=4)
        .then(|| section_skamp_locus(definition, sketch, item))
        .flatten()
}

pub(super) fn section_skamp_incidence_locus(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Option<SketchLocus> {
    section_skamp_point_locus(definition, sketch, item).or_else(|| {
        let entity = sketch_entity_id(sketch, item.entity_id);
        let locus = match item.sense {
            2 => SketchLocus::Start(entity.clone()),
            3 => SketchLocus::End(entity.clone()),
            _ => return None,
        };
        geometry?
            .get(&entity)
            .is_some_and(|geometry| matches!(geometry, SketchGeometry::Native { .. }))
            .then_some(locus)
    })
}

pub(super) fn section_skamp_line_pair(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    first: &crate::feature::FeatureSkampItem,
    second: &crate::feature::FeatureSkampItem,
) -> Option<[SketchEntityId; 2]> {
    if first.sense != 0
        || second.sense != 0
        || !section_skamp_is_line(definition, first)
        || !section_skamp_is_line(definition, second)
    {
        return None;
    }
    Some([first, second].map(|item| sketch_entity_id(sketch, item.entity_id)))
}

pub(super) fn section_skamp_oriented_line(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Option<SketchEntityId> {
    (item.sense == 0).then_some(())?;
    let entity = sketch_entity_id(sketch, item.entity_id);
    if section_skamp_is_line(definition, item) {
        return Some(entity);
    }
    if section_skamp_is_point(definition, item)
        || section_skamp_is_circular(definition, item)
        || section_saved_entity(definition, item.entity_id)
            .is_some_and(|entity| matches!(entity, crate::feature::FeatureSavedEntity::Spline(_)))
    {
        return None;
    }
    if solver_only_section_entities(definition).contains_key(&item.entity_id) {
        return Some(entity);
    }
    let line_role_evidence = complete_section_skamps(definition).any(|skamp| {
        skamp.items.iter().any(|candidate| {
            candidate.entity_id == item.entity_id && matches!(candidate.sense, 2 | 3)
        }) || match (skamp.kind, skamp.items.as_slice()) {
            (35, [first, second]) => {
                (first.entity_id == item.entity_id
                    && first.sense == 0
                    && section_skamp_point_locus(definition, sketch, second).is_some())
                    || (second.entity_id == item.entity_id
                        && second.sense == 0
                        && section_skamp_point_locus(definition, sketch, first).is_some())
            }
            _ => false,
        }
    });
    (line_role_evidence
        && geometry?
            .get(&entity)
            .is_some_and(|geometry| matches!(geometry, SketchGeometry::Native { .. })))
    .then_some(entity)
}

pub(super) fn section_skamp_same_coordinate(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    skamp: &crate::feature::FeatureSkamp,
    require_satisfied: bool,
) -> Option<(SketchLocus, SketchLocus, SketchCoordinateAxis)> {
    let [first, second] = skamp.items.as_slice() else {
        return None;
    };
    let first_locus = section_skamp_point_locus(definition, sketch, first)?;
    let second_locus = section_skamp_point_locus(definition, sketch, second)?;
    let coordinate = section_skamp_same_coordinate_axis(skamp)?;
    let axis = [SketchCoordinateAxis::U, SketchCoordinateAxis::V][coordinate];
    if require_satisfied {
        let ([first_source, second_source], _) =
            section_skamp_same_coordinate_sources(definition, skamp)?;
        let points = resolved_section_points(definition);
        let point = |source| {
            Some(match source {
                SectionPointSource::Point(point_id) => *points.get(&point_id)?,
                SectionPointSource::Value(point) => point,
            })
        };
        if let (Some(first_point), Some(second_point)) = (point(first_source), point(second_source))
        {
            let scale = first_point
                .iter()
                .chain(&second_point)
                .map(|coordinate| coordinate.abs())
                .fold(1.0, f64::max);
            ((first_point[coordinate] - second_point[coordinate]).abs() <= 1e-9 * scale)
                .then_some(())?;
        }
    }
    Some((first_locus, second_locus, axis))
}

pub(super) fn section_skamp_same_coordinate_sources(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<([SectionPointSource; 2], usize)> {
    if matches!(skamp.kind, 12 | 13) {
        let [item] = skamp.items.as_slice() else {
            return None;
        };
        (item.sense == 0 && section_skamp_is_arc(definition, item)).then_some(())?;
        let endpoint = |sense| {
            section_skamp_selected_point(
                definition,
                &crate::feature::FeatureSkampItem {
                    entity_id: item.entity_id,
                    sense,
                },
            )
        };
        return Some((
            [endpoint(2)?, endpoint(3)?],
            section_skamp_same_coordinate_axis(skamp)?,
        ));
    }
    let [first, second] = skamp.items.as_slice() else {
        return None;
    };
    let coordinate = section_skamp_same_coordinate_axis(skamp)?;
    Some((
        [
            section_skamp_selected_point(definition, first)?,
            section_skamp_selected_point(definition, second)?,
        ],
        coordinate,
    ))
}

pub(super) fn section_skamp_same_coordinate_axis(
    skamp: &crate::feature::FeatureSkamp,
) -> Option<usize> {
    Some(match (skamp.kind, skamp.flags) {
        (12, _) => 1,
        (13, _) => 0,
        (15 | 17, 1) => 0,
        (15 | 17, 2) => 1,
        (30, _) => 1,
        (31, _) => 0,
        _ => return None,
    })
}

pub(super) fn section_skamp_is_line(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    if solver_only_section_entity_family(definition, item.entity_id)
        == Some(SectionEntityIncidenceFamily::Line)
    {
        return true;
    }
    if unique_centered_line_segment(definition, item.entity_id).is_some() {
        return true;
    }
    if unique_reference_line_segment(definition, item.entity_id).is_some() {
        return true;
    }
    let has_segment = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .any(|segment| segment.external_id == item.entity_id);
    if has_segment {
        return unique_decoded_section_segment(definition, item.entity_id).is_some_and(|segment| {
            segment.kind == crate::feature::FeatureSegmentKind::Line
                || section_degenerate_axis_line(definition, segment)
        });
    }
    section_saved_entity(definition, item.entity_id)
        .is_some_and(|entity| matches!(entity, crate::feature::FeatureSavedEntity::Line(_)))
}

pub(super) fn section_degenerate_axis_line(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> bool {
    if segment.kind != crate::feature::FeatureSegmentKind::Point
        || segment.point_ids[0] != segment.point_ids[1]
    {
        return false;
    }
    let expected_kind = match segment.vertical_horizontal {
        Some(0) => 2,
        Some(1) => 1,
        _ => return false,
    };
    let unary_orientation = complete_section_skamps(definition).any(|skamp| {
        matches!(
            (skamp.kind, skamp.items.as_slice()),
            (kind, [item])
                if kind == expected_kind && item.entity_id == segment.external_id && item.sense == 0
        )
    });
    let symmetry_axis = complete_section_skamps(definition).any(|skamp| {
        matches!(
            (skamp.kind, skamp.items.as_slice()),
            (14, [axis, _, _]) if axis.entity_id == segment.external_id && axis.sense == 0
        )
    });
    unary_orientation && symmetry_axis
}

pub(super) fn section_skamp_is_point(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    solver_only_section_entity_family(definition, item.entity_id)
        == Some(SectionEntityIncidenceFamily::Point)
        || unique_point_segment(definition, item.entity_id).is_some()
        || unique_decoded_section_segment(definition, item.entity_id).is_some_and(|segment| {
            segment.kind == crate::feature::FeatureSegmentKind::Point
                && !section_degenerate_axis_line(definition, segment)
        })
}

pub(super) fn section_skamp_is_arc(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    let has_segment = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .any(|segment| segment.external_id == item.entity_id);
    if has_segment {
        return unique_decoded_section_segment(definition, item.entity_id)
            .is_some_and(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc);
    }
    section_saved_entity(definition, item.entity_id)
        .is_some_and(|entity| matches!(entity, crate::feature::FeatureSavedEntity::Arc(_)))
}

pub(super) fn section_skamp_curve_entity(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchEntityId> {
    if item.sense != 0 {
        return None;
    }
    let is_curve = section_skamp_is_line(definition, item)
        || unique_bounded_curve_segment(definition, item.entity_id).is_some()
        || section_skamp_is_circular(definition, item)
        || matches!(
            solver_only_section_entity_family(definition, item.entity_id),
            Some(
                SectionEntityIncidenceFamily::BoundedCurve
                    | SectionEntityIncidenceFamily::Arc
                    | SectionEntityIncidenceFamily::Circular
            )
        )
        || section_saved_entity(definition, item.entity_id).is_some_and(|entity| {
            matches!(
                entity,
                crate::feature::FeatureSavedEntity::Conic(_)
                    | crate::feature::FeatureSavedEntity::Spline(_)
            )
        });
    is_curve.then(|| sketch_entity_id(sketch, item.entity_id))
}

pub(super) fn section_skamp_midpoint(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    first: &crate::feature::FeatureSkampItem,
    second: &crate::feature::FeatureSkampItem,
    geometry: Option<&BTreeMap<SketchEntityId, SketchGeometry>>,
) -> Option<(SketchLocus, SketchEntityId)> {
    let target = |item: &crate::feature::FeatureSkampItem| {
        if item.sense == 4 && unique_centered_line_segment(definition, item.entity_id).is_some() {
            return Some(sketch_entity_id(sketch, item.entity_id));
        }
        (item.sense == 0).then_some(())?;
        if section_skamp_is_arc(definition, item) {
            return Some(sketch_entity_id(sketch, item.entity_id));
        }
        section_skamp_oriented_line(definition, sketch, item, geometry)
    };
    let point = |item: &crate::feature::FeatureSkampItem| {
        section_skamp_point_locus(definition, sketch, item).or_else(|| {
            (item.sense == 0 && section_skamp_is_circular(definition, item))
                .then(|| SketchLocus::Center(sketch_entity_id(sketch, item.entity_id)))
        })
    };
    let candidate = |target, point| Some((point?, target?));
    match (
        candidate(target(first), point(second)),
        candidate(target(second), point(first)),
    ) {
        (Some(candidate), None) | (None, Some(candidate)) => Some(candidate),
        _ => None,
    }
}

pub(super) fn section_saved_entity(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureSavedEntity> {
    let internal_id = definition.order_table.as_ref()?.internal_id(external_id)?;
    let mut matches = semantic_saved_section_entities(definition).filter(|entity| match entity {
        crate::feature::FeatureSavedEntity::Line(line) => line.entity_id == internal_id,
        crate::feature::FeatureSavedEntity::Arc(arc) => arc.entity_id == internal_id,
        crate::feature::FeatureSavedEntity::Circle(circle) => circle.entity_id == internal_id,
        crate::feature::FeatureSavedEntity::Conic(conic) => conic.entity_id == internal_id,
        crate::feature::FeatureSavedEntity::Spline(spline) => spline.entity_id == Some(internal_id),
        crate::feature::FeatureSavedEntity::Dummy(dummy) => dummy.entity_id == Some(internal_id),
    });
    let entity = matches.next()?;
    matches.next().is_none().then_some(entity)
}

pub(super) fn section_skamp_circular_entity(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchEntityId> {
    if item.sense != 0 {
        return None;
    }
    section_skamp_is_circular(definition, item).then(|| sketch_entity_id(sketch, item.entity_id))
}

pub(super) fn section_skamp_center_entity(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchEntityId> {
    (item.sense == 4 && section_skamp_is_circular(definition, item))
        .then(|| sketch_entity_id(sketch, item.entity_id))
}

pub(super) fn section_skamp_is_circular(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    if solver_only_section_entities(definition).contains_key(&item.entity_id) {
        return solver_only_section_entity_family(definition, item.entity_id).is_some_and(
            |family| {
                matches!(
                    family,
                    SectionEntityIncidenceFamily::Arc | SectionEntityIncidenceFamily::Circular
                )
            },
        );
    }
    if matches!(
        unique_section_incidence_curve_family(definition, item.entity_id),
        Some(SectionEntityIncidenceFamily::Arc | SectionEntityIncidenceFamily::Circular)
    ) {
        return true;
    }
    if unique_circle_segment(definition, item.entity_id).is_some() {
        return true;
    }
    let has_segment = definition
        .segments
        .iter()
        .flat_map(|segments| &segments.rows)
        .any(|segment| segment.external_id == item.entity_id);
    if has_segment {
        unique_decoded_section_segment(definition, item.entity_id)
            .is_some_and(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
    } else {
        section_saved_entity(definition, item.entity_id).is_some_and(|entity| {
            matches!(
                entity,
                crate::feature::FeatureSavedEntity::Arc(_)
                    | crate::feature::FeatureSavedEntity::Circle(_)
            )
        })
    }
}

pub(super) fn section_skamp_line_midpoint_sources(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<([u32; 2], SectionPointSource)> {
    let (35, [first, second]) = (skamp.kind, skamp.items.as_slice()) else {
        return None;
    };
    let target = |item: &crate::feature::FeatureSkampItem| {
        if item.sense == 4 {
            return unique_centered_line_segment(definition, item.entity_id)
                .map(|line| [line.center_id, line.center_id]);
        }
        if item.sense != 0 {
            return None;
        }
        let segment = unique_decoded_section_segment(definition, item.entity_id)?;
        (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(segment.point_ids)
    };
    let point =
        |item: &crate::feature::FeatureSkampItem| section_skamp_selected_point(definition, item);
    let candidates = [(first, second), (second, first)]
        .into_iter()
        .filter_map(|(target_item, point_item)| Some((target(target_item)?, point(point_item)?)))
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(super) fn section_skamp_arc_midpoint_source(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Option<(SectionPointSource, [f64; 2])> {
    let (35, [first, second]) = (skamp.kind, skamp.items.as_slice()) else {
        return None;
    };
    let candidates = [(first, second), (second, first)]
        .into_iter()
        .filter_map(|(target, point)| {
            (target.sense == 0 && section_skamp_is_arc(definition, target)).then_some(())?;
            Some((
                section_skamp_selected_point(definition, point)?,
                section_skamp_arc_midpoint(definition, target, coordinates)?,
            ))
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(super) fn section_skamp_arc_midpoint(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Option<[f64; 2]> {
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        if segment.kind != crate::feature::FeatureSegmentKind::Arc
            || segment.arc_orientation != Some(0)
        {
            return None;
        }
        let center = complete_section_coordinate(coordinates, segment.center_id?)?;
        let first = complete_section_coordinate(coordinates, segment.point_ids[0])?;
        let second = complete_section_coordinate(coordinates, segment.point_ids[1])?;
        return oriented_arc_midpoint(center, first, second, None);
    }
    let crate::feature::FeatureSavedEntity::Arc(arc) =
        section_saved_entity(definition, item.entity_id)?
    else {
        return None;
    };
    saved_arc_midpoint(arc)
}

pub(super) fn complete_section_coordinate(
    coordinates: &BTreeMap<u32, [Option<f64>; 2]>,
    point_id: u32,
) -> Option<[f64; 2]> {
    let [Some(u), Some(v)] = coordinates.get(&point_id).copied()? else {
        return None;
    };
    Some([u, v])
}

pub(super) fn saved_arc_midpoint(arc: &crate::feature::FeatureSavedArc) -> Option<[f64; 2]> {
    let [Some(center_u), Some(center_v), _] = arc.center else {
        return None;
    };
    let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] = arc.endpoints
    else {
        return None;
    };
    oriented_arc_midpoint(
        [center_u, center_v],
        [first_u, first_v],
        [second_u, second_v],
        arc.radius,
    )
}

pub(super) fn oriented_arc_midpoint(
    center: [f64; 2],
    first: [f64; 2],
    second: [f64; 2],
    stored_radius: Option<f64>,
) -> Option<[f64; 2]> {
    let first_offset = [first[0] - center[0], first[1] - center[1]];
    let second_offset = [second[0] - center[0], second[1] - center[1]];
    let first_radius = first_offset[0].hypot(first_offset[1]);
    let second_radius = second_offset[0].hypot(second_offset[1]);
    let radius = stored_radius.unwrap_or(first_radius);
    let scale = radius.max(first_radius).max(second_radius).max(1.0);
    if !center
        .into_iter()
        .chain(first)
        .chain(second)
        .chain([first_radius, second_radius, radius])
        .all(f64::is_finite)
        || radius <= 1e-12
        || (first_radius - second_radius).abs() > 1e-9 * scale
        || (radius - first_radius).abs() > 1e-9 * scale
    {
        return None;
    }
    let start = second_offset[1].atan2(second_offset[0]);
    let mut end = first_offset[1].atan2(first_offset[0]);
    while end <= start {
        end += std::f64::consts::TAU;
    }
    let angle = f64::midpoint(start, end);
    Some([
        center[0] + radius * angle.cos(),
        center[1] + radius * angle.sin(),
    ])
}

pub(super) fn section_skamp_active(status: u32) -> bool {
    status & 1 != 0
}

pub(super) fn complete_section_skamps(
    definition: &crate::feature::FeatureDefinition,
) -> impl Iterator<Item = &crate::feature::FeatureSkamp> {
    definition
        .relations
        .iter()
        .filter(|relations| feature_skamp_table_complete(relations))
        .flat_map(|relations| &relations.skamps)
}

pub(super) fn active_complete_section_skamps(
    definition: &crate::feature::FeatureDefinition,
) -> impl Iterator<Item = &crate::feature::FeatureSkamp> {
    complete_section_skamps(definition).filter(|skamp| section_skamp_active(skamp.status))
}

pub(super) fn section_skamp_constraints_for_geometry(
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
                        if kind == 12 {
                            SketchConstraintDefinition::HorizontalLoci { first, second }
                        } else {
                            SketchConstraintDefinition::VerticalLoci { first, second }
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
pub(super) fn sketch_constraint_loci_compatible(
    definition: &SketchConstraintDefinition,
    geometry: &BTreeMap<SketchEntityId, SketchGeometry>,
) -> bool {
    sketch_constraint_loci_compatible_with_policy(definition, geometry, false)
}

pub(super) fn sketch_constraint_loci_compatible_with_policy(
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
        | SketchConstraintDefinition::HorizontalDistance { first, second, .. }
        | SketchConstraintDefinition::VerticalDistance { first, second, .. }
        | SketchConstraintDefinition::HorizontalLoci { first, second }
        | SketchConstraintDefinition::VerticalLoci { first, second } => {
            locus_compatible(first) && locus_compatible(second)
        }
        SketchConstraintDefinition::Midpoint { point, entity }
        | SketchConstraintDefinition::PointOnObject { point, entity } => {
            locus_compatible(point) && geometry.contains_key(entity)
        }
        SketchConstraintDefinition::Symmetric { first, second, .. } => {
            locus_compatible(first) && locus_compatible(second)
        }
        SketchConstraintDefinition::PointSymmetric {
            first,
            second,
            center,
        } => locus_compatible(first) && locus_compatible(second) && locus_compatible(center),
        SketchConstraintDefinition::SnellsLaw {
            incident,
            refracted,
            ..
        } => locus_compatible(incident) && locus_compatible(refracted),
        _ => true,
    }
}

pub(super) fn section_entity_external_ids(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeSet<u32> {
    let mut ids = unique_section_segment_external_ids(definition);
    let Some(order) = &definition.order_table else {
        return ids;
    };
    let ambiguous_segment_ids = ambiguous_section_segment_external_ids(definition);
    let unique_saved_ids = unique_saved_section_internal_ids(definition);
    ids.extend(
        semantic_saved_section_entities(definition)
            .filter_map(|entity| saved_section_entity_identity(entity).0)
            .filter_map(|internal_id| {
                saved_section_external_id(
                    order,
                    &unique_saved_ids,
                    &ambiguous_segment_ids,
                    internal_id,
                )
            }),
    );
    ids
}

pub(super) fn section_segment_external_id_counts(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, usize> {
    definition
        .segments
        .as_ref()
        .map_or_else(BTreeMap::new, |table| {
            table
                .rows
                .iter()
                .map(|row| row.external_id)
                .chain(table.circle_rows.iter().map(|row| row.external_id))
                .chain(table.point_rows.iter().map(|row| row.external_id))
                .chain(table.centered_line_rows.iter().map(|row| row.external_id))
                .chain(table.reference_line_rows.iter().map(|row| row.external_id))
                .chain(table.bounded_curve_rows.iter().map(|row| row.external_id))
                .chain(table.conic_rows.iter().map(|row| row.external_id))
                .chain(table.opaque_rows.iter().map(|row| row.external_id))
                .fold(BTreeMap::new(), |mut counts, external_id| {
                    *counts.entry(external_id).or_insert(0) += 1;
                    counts
                })
        })
}

pub(super) fn unique_section_segment_external_ids(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeSet<u32> {
    section_segment_external_id_counts(definition)
        .into_iter()
        .filter_map(|(external_id, count)| (count == 1).then_some(external_id))
        .collect()
}

pub(super) fn ambiguous_section_segment_external_ids(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeSet<u32> {
    section_segment_external_id_counts(definition)
        .into_iter()
        .filter_map(|(external_id, count)| (count > 1).then_some(external_id))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SavedSectionEntityKind {
    Line,
    Arc,
    Circle,
    Conic,
    Spline,
    Dummy,
}

impl SavedSectionEntityKind {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Arc => "arc",
            Self::Circle => "circle",
            Self::Conic => "conic",
            Self::Spline => "spline",
            Self::Dummy => "dummy",
        }
    }
}

pub(super) fn saved_section_entity_identity(
    entity: &crate::feature::FeatureSavedEntity,
) -> (Option<u32>, usize, SavedSectionEntityKind) {
    match entity {
        crate::feature::FeatureSavedEntity::Line(line) => (
            Some(line.entity_id),
            line.offset,
            SavedSectionEntityKind::Line,
        ),
        crate::feature::FeatureSavedEntity::Arc(arc) => {
            (Some(arc.entity_id), arc.offset, SavedSectionEntityKind::Arc)
        }
        crate::feature::FeatureSavedEntity::Circle(circle) => (
            Some(circle.entity_id),
            circle.offset,
            SavedSectionEntityKind::Circle,
        ),
        crate::feature::FeatureSavedEntity::Conic(conic) => (
            Some(conic.entity_id),
            conic.offset,
            SavedSectionEntityKind::Conic,
        ),
        crate::feature::FeatureSavedEntity::Spline(spline) => (
            spline.entity_id,
            spline.offset,
            SavedSectionEntityKind::Spline,
        ),
        crate::feature::FeatureSavedEntity::Dummy(dummy) => {
            (dummy.entity_id, dummy.offset, SavedSectionEntityKind::Dummy)
        }
    }
}

pub(super) fn unresolved_saved_section_entity(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    saved: &crate::feature::FeatureSavedEntity,
    unique_saved_ids: &BTreeSet<u32>,
    ambiguous_segment_ids: &BTreeSet<u32>,
) -> (SketchEntity, usize) {
    let (internal_id, offset, kind) = saved_section_entity_identity(saved);
    let unique_internal_id = internal_id.is_some_and(|id| unique_saved_ids.contains(&id));
    let external_id = if unique_internal_id {
        definition.order_table.as_ref().and_then(|order| {
            saved_section_external_id(order, unique_saved_ids, ambiguous_segment_ids, internal_id?)
        })
    } else {
        None
    };
    let suffix = if unique_internal_id {
        external_id.map_or_else(
            || {
                let internal_id = internal_id.expect("unique saved entity has an id");
                match kind {
                    SavedSectionEntityKind::Spline | SavedSectionEntityKind::Dummy => {
                        internal_id.to_string()
                    }
                    _ => format!("saved{internal_id}"),
                }
            },
            |external_id| external_id.to_string(),
        )
    } else {
        format!("saved:offset:{offset}")
    };
    let id = external_id.map_or_else(
        || match kind {
            SavedSectionEntityKind::Spline => SketchEntityId(format!(
                "creo:featdefs:saved_spline#{}:{suffix}",
                sketch_identity_scope(sketch)
            )),
            SavedSectionEntityKind::Dummy => SketchEntityId(format!(
                "creo:featdefs:saved_dummy#{}:{suffix}",
                sketch_identity_scope(sketch)
            )),
            _ => sketch_entity_id(sketch, &suffix),
        },
        |external_id| sketch_entity_id(sketch, external_id),
    );
    (
        SketchEntity {
            id,
            sketch: sketch.clone(),
            construction: true,
            native_ref: Some(sketch_native_ref(sketch)),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Native {
                native_kind: format!("saved_{}", kind.name()),
            },
        },
        offset,
    )
}

pub(super) fn unique_saved_section_internal_ids(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeSet<u32> {
    semantic_saved_section_entities(definition)
        .filter_map(|entity| saved_section_entity_identity(entity).0)
        .fold(BTreeMap::new(), |mut counts, internal_id| {
            *counts.entry(internal_id).or_insert(0usize) += 1;
            counts
        })
        .into_iter()
        .filter_map(|(internal_id, count)| (count == 1).then_some(internal_id))
        .collect()
}

pub(super) fn saved_section_entity_is_elided_prototype(
    definition: &crate::feature::FeatureDefinition,
    entity: &crate::feature::FeatureSavedEntity,
) -> bool {
    let Some(internal_id) = saved_section_entity_identity(entity).0 else {
        return false;
    };
    definition
        .segments
        .as_ref()
        .is_some_and(|segments| segments.has_elided_prototype)
        && definition
            .order_table
            .as_ref()
            .is_some_and(|order| order.has_prototype)
        && definition.saved_section.as_ref().is_some_and(|saved| {
            crate::feature::saved_entity_offset(entity) == saved.offset
                && saved.entities.iter().any(|candidate| {
                    crate::feature::saved_entity_offset(candidate) > saved.offset
                        && saved_section_entity_identity(candidate).0 == Some(internal_id)
                })
        })
}

pub(super) fn semantic_saved_section_entities(
    definition: &crate::feature::FeatureDefinition,
) -> impl Iterator<Item = &crate::feature::FeatureSavedEntity> {
    definition
        .saved_section
        .iter()
        .flat_map(|saved| &saved.entities)
        .filter(|entity| !saved_section_entity_is_elided_prototype(definition, entity))
}

pub(super) fn materialized_saved_section_external_ids(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeSet<u32> {
    let unique_saved_ids = unique_saved_section_internal_ids(definition);
    let ambiguous_segment_ids = ambiguous_section_segment_external_ids(definition);
    semantic_saved_section_entities(definition)
        .filter_map(|entity| {
            match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => {
                    saved_spline_sketch_geometry(spline)?;
                }
                _ => {
                    saved_section_entity_geometry(entity)?;
                }
            }
            let internal_id = saved_section_entity_identity(entity).0?;
            unique_saved_ids.contains(&internal_id).then_some(())?;
            definition.order_table.as_ref().and_then(|order| {
                saved_section_external_id(
                    order,
                    &unique_saved_ids,
                    &ambiguous_segment_ids,
                    internal_id,
                )
            })
        })
        .collect()
}

pub(super) fn saved_section_external_id(
    order: &crate::feature::FeatureOrderTable,
    unique_saved_ids: &BTreeSet<u32>,
    ambiguous_segment_ids: &BTreeSet<u32>,
    internal_id: u32,
) -> Option<u32> {
    unique_saved_ids.contains(&internal_id).then_some(())?;
    let external_id = order.external_id(internal_id)?;
    (!ambiguous_segment_ids.contains(&external_id)).then_some(external_id)
}

pub(super) fn section_segment_identity_suffix(
    unique_external_ids: &BTreeSet<u32>,
    segment: &crate::feature::FeatureSegment,
) -> String {
    if unique_external_ids.contains(&segment.external_id) {
        segment.external_id.to_string()
    } else {
        format!("offset:{}", segment.offset)
    }
}

pub(super) fn opaque_section_segment_identity_suffix(
    unique_external_ids: &BTreeSet<u32>,
    segment: &crate::feature::FeatureOpaqueSegment,
) -> String {
    if unique_external_ids.contains(&segment.external_id) {
        segment.external_id.to_string()
    } else {
        format!("opaque:offset:{}", segment.offset)
    }
}

pub(super) fn resolved_profile_chains(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    emitted: &BTreeSet<u32>,
) -> Vec<Vec<SketchEntityUse>> {
    let Some(table) = &definition.trim_entities else {
        return resolved_segment_profile_chains(definition, sketch, emitted);
    };
    if !table.has_complete_bucket_frame() || !table.has_unique_external_ids() {
        return resolved_segment_profile_chains(definition, sketch, emitted);
    }
    let rows = table
        .rows
        .iter()
        .filter_map(|row| Some((row, trim_segment_id(definition, row)?)))
        .collect::<Vec<_>>();
    let trimmed_ids = rows
        .iter()
        .map(|(_, external_id)| *external_id)
        .collect::<BTreeSet<_>>();
    let trimmed_points = definition
        .segments
        .iter()
        .flat_map(|segments| &segments.rows)
        .filter(|segment| trimmed_ids.contains(&segment.external_id))
        .flat_map(|segment| segment.point_ids)
        .collect::<BTreeSet<_>>();
    if definition
        .segments
        .iter()
        .flat_map(|segments| &segments.rows)
        .filter(|segment| {
            emitted.contains(&segment.external_id)
                && !trimmed_ids.contains(&segment.external_id)
                && matches!(
                    segment.kind,
                    crate::feature::FeatureSegmentKind::Line
                        | crate::feature::FeatureSegmentKind::Arc
                )
        })
        .any(|segment| {
            segment
                .point_ids
                .into_iter()
                .any(|point| trimmed_points.contains(&point))
        })
    {
        return resolved_segment_profile_chains(definition, sketch, emitted);
    }
    let mut incident = BTreeMap::<u32, Vec<usize>>::new();
    for (index, row) in rows.iter().enumerate() {
        for vertex in row.0.vertices {
            incident.entry(vertex).or_default().push(index);
        }
    }
    let mut remaining = (0..rows.len()).collect::<BTreeSet<_>>();
    let mut profiles = Vec::new();
    while let Some(seed) = remaining.first().copied() {
        let mut component = BTreeSet::from([seed]);
        let mut frontier = vec![seed];
        while let Some(index) = frontier.pop() {
            for vertex in rows[index].0.vertices {
                for adjacent in &incident[&vertex] {
                    if component.insert(*adjacent) {
                        frontier.push(*adjacent);
                    }
                }
            }
        }
        remaining.retain(|index| !component.contains(index));
        if incident
            .values()
            .any(|rows| rows.iter().filter(|row| component.contains(row)).count() > 2)
        {
            continue;
        }
        if component
            .iter()
            .any(|index| !emitted.contains(&rows[*index].1))
        {
            continue;
        }
        let endpoints = incident
            .iter()
            .filter(|(_, rows)| rows.iter().filter(|row| component.contains(row)).count() == 1)
            .map(|(vertex, _)| *vertex)
            .collect::<Vec<_>>();
        if !matches!(endpoints.len(), 0 | 2) {
            continue;
        }
        let first_row = component
            .iter()
            .min_by_key(|index| rows[**index].1)
            .copied()
            .expect("component contains seed");
        let mut vertex = endpoints
            .iter()
            .min()
            .copied()
            .unwrap_or(rows[first_row].0.vertices[0]);
        let start_vertex = vertex;
        let mut unused = component;
        let mut profile = Vec::new();
        while !unused.is_empty() {
            let candidates = incident[&vertex]
                .iter()
                .filter(|index| unused.contains(index))
                .copied()
                .collect::<Vec<_>>();
            let index = if profile.is_empty() && endpoints.is_empty() {
                if candidates.contains(&first_row) {
                    first_row
                } else {
                    break;
                }
            } else if candidates.len() == 1 {
                candidates[0]
            } else {
                break;
            };
            let (row, external_id) = rows[index];
            let row_reversed = row.vertices[1] == vertex;
            if !row_reversed && row.vertices[0] != vertex {
                break;
            }
            let arc_orientation_reversed = definition
                .segments
                .as_ref()
                .and_then(|table| table.segment(external_id))
                .is_some_and(|segment| {
                    segment.kind == crate::feature::FeatureSegmentKind::Arc
                        && segment.arc_orientation == Some(0)
                });
            profile.push(SketchEntityUse {
                entity: sketch_entity_id(sketch, external_id),
                reversed: row_reversed ^ arc_orientation_reversed,
            });
            vertex = if row_reversed {
                row.vertices[0]
            } else {
                row.vertices[1]
            };
            unused.remove(&index);
        }
        let terminal_ok = if endpoints.is_empty() {
            vertex == start_vertex
        } else {
            endpoints.contains(&vertex) && vertex != start_vertex
        };
        if unused.is_empty() && terminal_ok {
            profiles.push(profile);
        }
    }
    profiles
}

pub(super) fn resolved_segment_profile_chains(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    emitted: &BTreeSet<u32>,
) -> Vec<Vec<SketchEntityUse>> {
    let Some(table) = definition
        .segments
        .as_ref()
        .filter(|table| table.is_complete())
    else {
        return Vec::new();
    };
    let rows = table
        .rows
        .iter()
        .filter(|segment| {
            emitted.contains(&segment.external_id)
                && matches!(
                    segment.kind,
                    crate::feature::FeatureSegmentKind::Line
                        | crate::feature::FeatureSegmentKind::Arc
                )
        })
        .collect::<Vec<_>>();
    let mut incident = BTreeMap::<u32, Vec<usize>>::new();
    for (index, segment) in rows.iter().enumerate() {
        for point in segment.point_ids {
            incident.entry(point).or_default().push(index);
        }
    }
    let mut remaining = (0..rows.len()).collect::<BTreeSet<_>>();
    let mut profiles = Vec::new();
    while let Some(seed) = remaining.first().copied() {
        let mut component = BTreeSet::from([seed]);
        let mut frontier = vec![seed];
        while let Some(index) = frontier.pop() {
            for point in rows[index].point_ids {
                for adjacent in &incident[&point] {
                    if component.insert(*adjacent) {
                        frontier.push(*adjacent);
                    }
                }
            }
        }
        remaining.retain(|index| !component.contains(index));
        if component.iter().any(|index| {
            rows[*index].point_ids.into_iter().any(|point| {
                incident[&point]
                    .iter()
                    .filter(|row| component.contains(row))
                    .count()
                    != 2
            })
        }) {
            continue;
        }
        let first = component
            .iter()
            .min_by_key(|index| rows[**index].external_id)
            .copied()
            .expect("component contains seed");
        let mut point = rows[first].point_ids[0].min(rows[first].point_ids[1]);
        let start = point;
        let mut unused = component;
        let mut profile = Vec::new();
        while !unused.is_empty() {
            let candidates = incident[&point]
                .iter()
                .filter(|index| unused.contains(index))
                .copied()
                .collect::<BTreeSet<_>>();
            let index = if profile.is_empty() && candidates.contains(&first) {
                first
            } else if candidates.len() == 1 {
                *candidates.first().expect("one candidate")
            } else {
                break;
            };
            let segment = rows[index];
            let traversal_reversed = segment.point_ids[1] == point;
            if !traversal_reversed && segment.point_ids[0] != point {
                break;
            }
            let analytic_reversed = segment.kind == crate::feature::FeatureSegmentKind::Arc
                && segment.arc_orientation == Some(0);
            profile.push(SketchEntityUse {
                entity: sketch_entity_id(sketch, segment.external_id),
                reversed: traversal_reversed ^ analytic_reversed,
            });
            point = if traversal_reversed {
                segment.point_ids[0]
            } else {
                segment.point_ids[1]
            };
            unused.remove(&index);
        }
        if unused.is_empty() && point == start {
            profiles.push(profile);
        }
    }
    profiles
}

pub(super) fn solver_only_section_entities(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, usize> {
    let declared_segment_ids = definition
        .segments
        .iter()
        .flat_map(|table| {
            table
                .rows
                .iter()
                .map(|segment| segment.external_id)
                .chain(table.circle_rows.iter().map(|segment| segment.external_id))
                .chain(table.point_rows.iter().map(|segment| segment.external_id))
                .chain(
                    table
                        .centered_line_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    table
                        .reference_line_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(
                    table
                        .bounded_curve_rows
                        .iter()
                        .map(|segment| segment.external_id),
                )
                .chain(table.conic_rows.iter().map(|segment| segment.external_id))
                .chain(table.opaque_rows.iter().map(|segment| segment.external_id))
        })
        .collect::<BTreeSet<_>>();
    definition
        .relations
        .iter()
        .flat_map(|relations| &relations.skamps)
        .flat_map(|skamp| {
            skamp
                .items
                .iter()
                .map(move |item| (item.entity_id, skamp.offset))
        })
        .filter(|(entity_id, _)| !declared_segment_ids.contains(entity_id))
        .fold(
            BTreeMap::<u32, usize>::new(),
            |mut entities, (id, offset)| {
                entities
                    .entry(id)
                    .and_modify(|first_offset| *first_offset = (*first_offset).min(offset))
                    .or_insert(offset);
                entities
            },
        )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SectionEntityIncidenceFamily {
    Point,
    BoundedCurve,
    Line,
    Arc,
    Circular,
}

pub(super) fn section_skamp_has_proven_point_locus(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> bool {
    if item.sense == 0 {
        return unique_point_segment(definition, item.entity_id).is_some()
            || unique_decoded_section_segment(definition, item.entity_id).is_some_and(|segment| {
                segment.kind == crate::feature::FeatureSegmentKind::Point
                    && !section_degenerate_axis_line(definition, segment)
            });
    }
    let solver_family = section_incidence_curve_family_evidence(definition, item.entity_id);
    if solver_family.len() == 1
        && ((solver_family.contains(&SectionEntityIncidenceFamily::BoundedCurve)
            || solver_family.contains(&SectionEntityIncidenceFamily::Line)
            || solver_family.contains(&SectionEntityIncidenceFamily::Arc))
            && matches!(item.sense, 2 | 3)
            || (solver_family.contains(&SectionEntityIncidenceFamily::Arc)
                || solver_family.contains(&SectionEntityIncidenceFamily::Circular))
                && matches!(item.sense, 2..=4))
    {
        return true;
    }
    if let Some(segment) = unique_decoded_section_segment(definition, item.entity_id) {
        return matches!(
            (segment.kind, item.sense),
            (crate::feature::FeatureSegmentKind::Line, 2 | 3)
                | (crate::feature::FeatureSegmentKind::Arc, 2..=4)
        );
    }
    if unique_centered_line_segment(definition, item.entity_id).is_some() {
        return matches!(item.sense, 2..=4);
    }
    if let Some(segment) = unique_reference_line_segment(definition, item.entity_id) {
        return match item.sense {
            2 => segment.point_ids[0].is_some(),
            3 => segment.point_ids[1].is_some(),
            _ => false,
        };
    }
    if unique_bounded_curve_segment(definition, item.entity_id).is_some() {
        return matches!(item.sense, 2 | 3);
    }
    if unique_circle_segment(definition, item.entity_id).is_some() {
        return item.sense == 4;
    }
    matches!(
        (section_saved_entity(definition, item.entity_id), item.sense),
        (Some(crate::feature::FeatureSavedEntity::Line(_)), 2 | 3)
            | (Some(crate::feature::FeatureSavedEntity::Arc(_)), 2..=4)
            | (
                Some(
                    crate::feature::FeatureSavedEntity::Circle(_)
                        | crate::feature::FeatureSavedEntity::Conic(_),
                ),
                4,
            )
    )
}

pub(super) fn section_incidence_curve_family_evidence(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> BTreeSet<SectionEntityIncidenceFamily> {
    let mut evidence = BTreeSet::new();
    if complete_section_skamps(definition).any(|skamp| {
        matches!(
            (skamp.kind, skamp.items.as_slice()),
            (1 | 2, [item]) if item.sense == 0 && item.entity_id == entity_id
        )
    }) {
        evidence.insert(SectionEntityIncidenceFamily::Line);
    }
    for skamp in definition
        .relations
        .iter()
        .filter(|relations| feature_skamp_table_complete(relations))
        .flat_map(|relations| &relations.skamps)
        .filter(|skamp| section_skamp_active(skamp.status))
    {
        for item in &skamp.items {
            if item.entity_id == entity_id && matches!(item.sense, 2 | 3) {
                evidence.insert(SectionEntityIncidenceFamily::BoundedCurve);
            }
            if item.entity_id == entity_id && item.sense == 4 {
                evidence.insert(SectionEntityIncidenceFamily::Circular);
            }
        }
        match (skamp.kind, skamp.items.as_slice()) {
            (5 | 7 | 8, [first, second])
                if first.sense == 0
                    && second.sense == 0
                    && (first.entity_id == entity_id || second.entity_id == entity_id) =>
            {
                evidence.insert(SectionEntityIncidenceFamily::Line);
            }
            (6, [first, second])
                if first.sense == 0
                    && second.sense == 0
                    && (first.entity_id == entity_id || second.entity_id == entity_id) =>
            {
                evidence.insert(SectionEntityIncidenceFamily::Circular);
            }
            _ => {}
        }
    }
    normalize_section_incidence_curve_family_evidence(&mut evidence);
    evidence
}

pub(super) fn unique_section_incidence_curve_family(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> Option<SectionEntityIncidenceFamily> {
    exactly_one(section_incidence_curve_family_evidence(definition, entity_id).into_iter())
}

pub(super) fn normalize_section_incidence_curve_family_evidence(
    evidence: &mut BTreeSet<SectionEntityIncidenceFamily>,
) {
    if evidence.contains(&SectionEntityIncidenceFamily::Line) {
        evidence.remove(&SectionEntityIncidenceFamily::BoundedCurve);
    } else if evidence.contains(&SectionEntityIncidenceFamily::Circular)
        && evidence.remove(&SectionEntityIncidenceFamily::BoundedCurve)
    {
        evidence.remove(&SectionEntityIncidenceFamily::Circular);
        evidence.insert(SectionEntityIncidenceFamily::Arc);
    }
}

pub(super) fn solver_only_section_entity_family(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> Option<SectionEntityIncidenceFamily> {
    solver_only_section_entities(definition)
        .contains_key(&entity_id)
        .then_some(())?;
    let mut evidence = section_incidence_curve_family_evidence(definition, entity_id);
    if complete_section_skamps(definition).any(|skamp| {
        skamp
            .items
            .iter()
            .any(|item| item.entity_id == entity_id && item.sense == 4)
    }) {
        evidence.insert(SectionEntityIncidenceFamily::Circular);
    }
    if !evidence.contains(&SectionEntityIncidenceFamily::Line)
        && complete_section_skamps(definition).any(|skamp| {
            let (35, [first, second]) = (skamp.kind, skamp.items.as_slice()) else {
                return false;
            };
            [(first, second), (second, first)]
                .into_iter()
                .any(|(point, target)| {
                    point.entity_id == entity_id
                        && point.sense == 0
                        && target.sense == 4
                        && unique_centered_line_segment(definition, target.entity_id).is_some()
                })
        })
    {
        evidence.insert(SectionEntityIncidenceFamily::Point);
    }
    if !evidence.contains(&SectionEntityIncidenceFamily::Point) {
        let solver_only_point_from_midpoint = complete_section_skamps(definition).any(|skamp| {
            let (35, [first, second]) = (skamp.kind, skamp.items.as_slice()) else {
                return false;
            };
            [(first, second), (second, first)]
                .into_iter()
                .filter(|(point, target)| {
                    point.sense == 0
                        && point.entity_id == entity_id
                        && target.sense == 0
                        && (unique_decoded_section_segment(definition, target.entity_id)
                            .is_some_and(|segment| {
                                matches!(
                                    segment.kind,
                                    crate::feature::FeatureSegmentKind::Line
                                        | crate::feature::FeatureSegmentKind::Arc
                                )
                            })
                            || section_saved_entity(definition, target.entity_id).is_some_and(
                                |saved| {
                                    matches!(
                                        saved,
                                        crate::feature::FeatureSavedEntity::Line(_)
                                            | crate::feature::FeatureSavedEntity::Arc(_)
                                    )
                                },
                            ))
                })
                .count()
                == 1
        });
        if solver_only_point_from_midpoint {
            evidence.insert(SectionEntityIncidenceFamily::Point);
        }
    }
    for skamp in definition
        .relations
        .iter()
        .filter(|relations| feature_skamp_table_complete(relations))
        .flat_map(|relations| &relations.skamps)
        .filter(|skamp| section_skamp_active(skamp.status))
    {
        if let (0, [first, second]) = (skamp.kind, skamp.items.as_slice()) {
            if first.entity_id == entity_id
                && first.sense == 0
                && section_skamp_has_proven_point_locus(definition, second)
            {
                evidence.insert(SectionEntityIncidenceFamily::Point);
            }
            if second.entity_id == entity_id
                && second.sense == 0
                && section_skamp_has_proven_point_locus(definition, first)
            {
                evidence.insert(SectionEntityIncidenceFamily::Point);
            }
        }
    }
    let mut evidence = evidence.into_iter();
    let family = evidence.next()?;
    evidence.next().is_none().then_some(family)
}

pub(super) fn transfer_sketches(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> SketchSegmentTransferCoverage {
    let mut coverage = SketchSegmentTransferCoverage::default();
    for definition in scan
        .features
        .definitions
        .iter()
        .filter(|definition| feature_definition_has_sketch_design(definition))
    {
        let transform = definition.section_3d.as_ref().and_then(|section| {
            unique_feature_section_transform(
                &scan.features.section_transforms,
                definition.id,
                section.offset,
            )
        });
        let sketch_id = model_sketch_id(scan, definition);
        let segments = section_segment_rows(definition);
        let unique_segment_ids = unique_section_segment_external_ids(definition);
        let ambiguous_segment_ids = ambiguous_section_segment_external_ids(definition);
        let unique_saved_ids = unique_saved_section_internal_ids(definition);
        let complete_segment_table = definition
            .segments
            .as_ref()
            .is_some_and(crate::feature::FeatureSegmentTable::is_complete);
        if let Some(table) = &definition.segments {
            let decoded_rows = table.retained_row_count();
            let expected_rows = usize::try_from(table.declared_count)
                .expect("u32 segment count fits usize")
                .saturating_sub(usize::from(table.has_elided_prototype));
            coverage.decoded_rows += decoded_rows;
            coverage.missing_rows += expected_rows.saturating_sub(decoded_rows);
            for segment in &table.rows {
                let family = match segment.kind {
                    crate::feature::FeatureSegmentKind::Line => "line",
                    crate::feature::FeatureSegmentKind::Arc => "arc",
                    crate::feature::FeatureSegmentKind::Point => "point",
                };
                coverage.by_family.entry(family).or_default().0 += 1;
            }
            for (family, count) in [
                ("circle", table.circle_rows.len()),
                ("point", table.point_rows.len()),
                ("centered_line", table.centered_line_rows.len()),
                ("reference_line", table.reference_line_rows.len()),
                ("bounded_curve", table.bounded_curve_rows.len()),
                ("conic", table.conic_rows.len()),
                ("opaque", table.opaque_rows.len()),
            ] {
                coverage.by_family.entry(family).or_default().0 += count;
            }
        }
        let variable_points = resolved_section_coordinates(definition);
        let points = variable_points
            .iter()
            .filter_map(|(point, [u, v])| {
                Some((*point, [u.as_ref().copied()?, v.as_ref().copied()?]))
            })
            .collect::<BTreeMap<_, _>>();
        let radii = resolved_section_radii(definition);
        let missing_line_geometry = saved_section_missing_line_geometry(definition);
        let solved = definition
            .trim_entities
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| trim_segment_id(definition, row))
            .collect::<BTreeSet<_>>();
        let trim_vertex_coordinates = resolved_trim_vertex_coordinates(definition, &points);
        let resolved_segment_geometries = segments
            .iter()
            .map(|segment| {
                (
                    segment.offset,
                    resolved_section_segment_geometry_with_missing_line(
                        definition,
                        &points,
                        segment,
                        missing_line_geometry.as_ref(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let segment_geometries = segments
            .iter()
            .map(|segment| {
                let geometry = if unique_segment_ids.contains(&segment.external_id)
                    && solved.contains(&segment.external_id)
                {
                    trimmed_section_segment_geometry_with_missing_line(
                        definition,
                        &points,
                        &trim_vertex_coordinates,
                        segment,
                        missing_line_geometry.as_ref(),
                    )
                } else {
                    resolved_segment_geometries
                        .get(&segment.offset)
                        .cloned()
                        .flatten()
                }
                .or_else(|| {
                    section_axis_reference_line_geometry(definition, &variable_points, segment)
                });
                (segment.offset, geometry)
            })
            .collect::<BTreeMap<_, _>>();
        let segment_geometry = |segment: &crate::feature::FeatureSegment| {
            if section_degenerate_axis_line(definition, segment) {
                return segment_geometries
                    .get(&segment.offset)
                    .cloned()
                    .flatten()
                    .or_else(|| {
                        Some(SketchGeometry::Native {
                            native_kind: "line".to_string(),
                        })
                    });
            }
            segment_geometries.get(&segment.offset).cloned().flatten()
        };
        let circle_geometries = definition
            .segments
            .iter()
            .flat_map(|table| &table.circle_rows)
            .filter_map(|segment| {
                Some((
                    segment.offset,
                    section_circle_geometry(&points, &radii, segment)?,
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let point_geometries = definition
            .segments
            .iter()
            .flat_map(|table| &table.point_rows)
            .filter_map(|segment| {
                Some((
                    segment.offset,
                    section_point_row_geometry(&points, segment)?,
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let centered_line_geometries = definition
            .segments
            .iter()
            .flat_map(|table| &table.centered_line_rows)
            .filter_map(|segment| {
                Some((
                    segment.offset,
                    section_centered_line_geometry(&points, segment)?,
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let reference_line_geometries = definition
            .segments
            .iter()
            .flat_map(|table| &table.reference_line_rows)
            .filter_map(|segment| {
                Some((
                    segment.offset,
                    resolved_section_reference_line_geometry(
                        definition,
                        &variable_points,
                        &points,
                        segment,
                    )?,
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let mut emitted = segments
            .iter()
            .filter(|segment| {
                unique_segment_ids.contains(&segment.external_id)
                    && segment_geometry(segment).is_some()
            })
            .map(|segment| segment.external_id)
            .collect::<BTreeSet<_>>();
        emitted.extend(
            definition
                .segments
                .iter()
                .flat_map(|table| &table.circle_rows)
                .filter(|segment| {
                    unique_segment_ids.contains(&segment.external_id)
                        && circle_geometries.contains_key(&segment.offset)
                })
                .map(|segment| segment.external_id),
        );
        let resolved_segment_offsets = segments
            .iter()
            .filter(|segment| {
                segment_geometries
                    .get(&segment.offset)
                    .is_some_and(Option::is_some)
            })
            .map(|segment| segment.offset)
            .collect::<BTreeSet<_>>();
        let materialized_saved_section_external_ids =
            materialized_saved_section_external_ids(definition);
        coverage.resolved_geometry += resolved_segment_offsets.len();
        for segment in segments
            .iter()
            .filter(|segment| resolved_segment_offsets.contains(&segment.offset))
        {
            let family = match segment.kind {
                crate::feature::FeatureSegmentKind::Line => "line",
                crate::feature::FeatureSegmentKind::Arc => "arc",
                crate::feature::FeatureSegmentKind::Point => "point",
            };
            coverage.by_family.entry(family).or_default().1 += 1;
        }
        let resolved_circles = definition
            .segments
            .iter()
            .flat_map(|table| &table.circle_rows)
            .filter(|segment| {
                circle_geometries.contains_key(&segment.offset)
                    || (unique_segment_ids.contains(&segment.external_id)
                        && materialized_saved_section_external_ids.contains(&segment.external_id))
            })
            .count();
        coverage.resolved_geometry += resolved_circles;
        coverage.by_family.entry("circle").or_default().1 += resolved_circles;
        let resolved_points = definition
            .segments
            .iter()
            .flat_map(|table| &table.point_rows)
            .filter(|segment| {
                point_geometries.contains_key(&segment.offset)
                    || (unique_segment_ids.contains(&segment.external_id)
                        && materialized_saved_section_external_ids.contains(&segment.external_id))
            })
            .count();
        coverage.resolved_geometry += resolved_points;
        coverage.by_family.entry("point").or_default().1 += resolved_points;
        let resolved_centered_lines = definition
            .segments
            .iter()
            .flat_map(|table| &table.centered_line_rows)
            .filter(|segment| {
                centered_line_geometries.contains_key(&segment.offset)
                    || (unique_segment_ids.contains(&segment.external_id)
                        && materialized_saved_section_external_ids.contains(&segment.external_id))
            })
            .count();
        coverage.resolved_geometry += resolved_centered_lines;
        coverage.by_family.entry("centered_line").or_default().1 += resolved_centered_lines;
        let resolved_reference_lines = definition
            .segments
            .iter()
            .flat_map(|table| &table.reference_line_rows)
            .filter(|segment| reference_line_geometries.contains_key(&segment.offset))
            .count();
        coverage.resolved_geometry += resolved_reference_lines;
        coverage.by_family.entry("reference_line").or_default().1 += resolved_reference_lines;
        let resolved_bounded_curves = definition
            .segments
            .iter()
            .flat_map(|table| &table.bounded_curve_rows)
            .filter(|segment| {
                unique_segment_ids.contains(&segment.external_id)
                    && materialized_saved_section_external_ids.contains(&segment.external_id)
            })
            .count();
        coverage.resolved_geometry += resolved_bounded_curves;
        coverage.by_family.entry("bounded_curve").or_default().1 += resolved_bounded_curves;
        let resolved_conics = definition
            .segments
            .iter()
            .flat_map(|table| &table.conic_rows)
            .filter(|segment| {
                unique_segment_ids.contains(&segment.external_id)
                    && materialized_saved_section_external_ids.contains(&segment.external_id)
            })
            .count();
        coverage.resolved_geometry += resolved_conics;
        coverage.by_family.entry("conic").or_default().1 += resolved_conics;
        let resolved_opaque = definition
            .segments
            .iter()
            .flat_map(|table| &table.opaque_rows)
            .filter(|segment| {
                unique_segment_ids.contains(&segment.external_id)
                    && materialized_saved_section_external_ids.contains(&segment.external_id)
            })
            .count();
        coverage.resolved_geometry += resolved_opaque;
        coverage.by_family.entry("opaque").or_default().1 += resolved_opaque;
        let mut profiles = resolved_profile_chains(definition, &sketch_id, &emitted);
        let generated_profile_geometries = segments
            .iter()
            .filter(|segment| {
                unique_segment_ids.contains(&segment.external_id)
                    && emitted.contains(&segment.external_id)
            })
            .filter_map(|segment| {
                let geometry = segment_geometry(segment)?;
                let expected_kinds = section_generated_profile_surface_kinds(&geometry)?;
                section_entity_is_generated_profile(
                    complete_segment_table,
                    definition.owner_feature_id,
                    segment.external_id,
                    expected_kinds,
                    &scan.features.entity_tables,
                    &scan.surfaces.rows,
                )
                .then_some((segment.external_id, geometry))
            })
            .chain(
                definition
                    .segments
                    .iter()
                    .flat_map(|table| &table.circle_rows)
                    .filter(|segment| unique_segment_ids.contains(&segment.external_id))
                    .filter_map(|segment| {
                        let geometry = circle_geometries.get(&segment.offset)?.clone();
                        let expected_kinds = section_generated_profile_surface_kinds(&geometry)?;
                        section_entity_is_generated_profile(
                            complete_segment_table,
                            definition.owner_feature_id,
                            segment.external_id,
                            expected_kinds,
                            &scan.features.entity_tables,
                            &scan.surfaces.rows,
                        )
                        .then_some((segment.external_id, geometry))
                    }),
            )
            .collect::<Vec<_>>();
        let mut profile_entities = profiles
            .iter()
            .flatten()
            .map(|entity_use| entity_use.entity.clone())
            .collect::<BTreeSet<_>>();
        for profile in saved_profile_chains(&sketch_id, &generated_profile_geometries) {
            if profile
                .iter()
                .all(|entity_use| !profile_entities.contains(&entity_use.entity))
            {
                profile_entities.extend(profile.iter().map(|entity_use| entity_use.entity.clone()));
                profiles.push(profile);
            }
        }
        let mut entities = segments
            .iter()
            .filter_map(|segment| {
                let geometry = segment_geometry(segment)?;
                let suffix = section_segment_identity_suffix(&unique_segment_ids, segment);
                let id = sketch_entity_id(&sketch_id, &suffix);
                annotate(
                    annotations,
                    &id.0,
                    "FeatDefs",
                    segment.offset as u64,
                    match (&geometry, segment.kind) {
                        (SketchGeometry::Native { native_kind }, _) if native_kind == "line" => {
                            "section_degenerate_axis_line"
                        }
                        (SketchGeometry::ReferenceLine { .. }, _) => {
                            "solved_section_axis_reference_line"
                        }
                        (_, crate::feature::FeatureSegmentKind::Line) => "solved_section_line",
                        (_, crate::feature::FeatureSegmentKind::Arc) => "solved_section_arc",
                        (_, crate::feature::FeatureSegmentKind::Point) => "solved_section_point",
                    },
                    if matches!(&geometry, SketchGeometry::Native { .. }) {
                        Exactness::ByteExact
                    } else {
                        Exactness::Derived
                    },
                );
                let construction = matches!(geometry, SketchGeometry::ReferenceLine { .. })
                    || !unique_segment_ids.contains(&segment.external_id)
                    || (!solved.contains(&segment.external_id) && !profile_entities.contains(&id));
                Some(SketchEntity {
                    id,
                    sketch: sketch_id.clone(),
                    construction,
                    native_ref: Some(sketch_native_ref(&sketch_id)),
                    geometry_ref: placed_sketch_curve_ref(transform, &sketch_id, suffix, &geometry),
                    endpoint_refs: match (&geometry, segment.kind) {
                        (SketchGeometry::Native { native_kind }, _) if native_kind == "line" => {
                            vec![segment.point_ids[0]]
                        }
                        (SketchGeometry::ReferenceLine { .. }, _)
                            if section_degenerate_axis_line(definition, segment) =>
                        {
                            vec![segment.point_ids[0]]
                        }
                        (_, crate::feature::FeatureSegmentKind::Arc) => {
                            vec![segment.point_ids[1], segment.point_ids[0]]
                        }
                        (_, crate::feature::FeatureSegmentKind::Line) => segment.point_ids.to_vec(),
                        (_, crate::feature::FeatureSegmentKind::Point) => {
                            vec![segment.point_ids[0]]
                        }
                    }
                    .into_iter()
                    .map(|point| sketch_point_ref(&sketch_id, point))
                    .collect(),
                    geometry,
                })
            })
            .collect::<Vec<_>>();
        for segment in segments
            .iter()
            .filter(|segment| segment_geometry(segment).is_none())
        {
            let id = sketch_entity_id(
                &sketch_id,
                section_segment_identity_suffix(&unique_segment_ids, segment),
            );
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                segment.offset as u64,
                "unresolved_section_segment",
                Exactness::ByteExact,
            );
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction: true,
                native_ref: Some(sketch_native_ref(&sketch_id)),
                geometry_ref: None,
                endpoint_refs: match segment.kind {
                    crate::feature::FeatureSegmentKind::Arc => {
                        vec![segment.point_ids[1], segment.point_ids[0]]
                    }
                    crate::feature::FeatureSegmentKind::Line => segment.point_ids.to_vec(),
                    crate::feature::FeatureSegmentKind::Point => vec![segment.point_ids[0]],
                }
                .into_iter()
                .map(|point| sketch_point_ref(&sketch_id, point))
                .collect(),
                geometry: SketchGeometry::Native {
                    native_kind: match segment.kind {
                        crate::feature::FeatureSegmentKind::Line => "line",
                        crate::feature::FeatureSegmentKind::Arc => "arc",
                        crate::feature::FeatureSegmentKind::Point => "point",
                    }
                    .to_string(),
                },
            });
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| &table.circle_rows)
        {
            let unique_external_id = unique_segment_ids.contains(&segment.external_id);
            if unique_external_id
                && materialized_saved_section_external_ids.contains(&segment.external_id)
            {
                continue;
            }
            let suffix = if unique_external_id {
                segment.external_id.to_string()
            } else {
                format!("circle:offset:{}", segment.offset)
            };
            let id = sketch_entity_id(&sketch_id, &suffix);
            let geometry = circle_geometries
                .get(&segment.offset)
                .cloned()
                .unwrap_or_else(|| SketchGeometry::Native {
                    native_kind: "circle".to_string(),
                });
            let solved_geometry = matches!(geometry, SketchGeometry::Circle { .. });
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                segment.offset as u64,
                if solved_geometry {
                    "solved_section_circle"
                } else {
                    "unresolved_section_circle"
                },
                if solved_geometry {
                    Exactness::Derived
                } else {
                    Exactness::ByteExact
                },
            );
            let construction = !unique_external_id || !profile_entities.contains(&id);
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction,
                native_ref: Some(sketch_native_ref(&sketch_id)),
                geometry_ref: placed_sketch_curve_ref(transform, &sketch_id, suffix, &geometry),
                endpoint_refs: Vec::new(),
                geometry,
            });
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| &table.point_rows)
        {
            let unique_external_id = unique_segment_ids.contains(&segment.external_id);
            if unique_external_id
                && materialized_saved_section_external_ids.contains(&segment.external_id)
            {
                continue;
            }
            let suffix = if unique_external_id {
                segment.external_id.to_string()
            } else {
                format!("point:offset:{}", segment.offset)
            };
            let id = sketch_entity_id(&sketch_id, &suffix);
            let geometry = point_geometries
                .get(&segment.offset)
                .cloned()
                .unwrap_or_else(|| SketchGeometry::Native {
                    native_kind: "point".to_string(),
                });
            let solved_geometry = matches!(geometry, SketchGeometry::Point { .. });
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                segment.offset as u64,
                if solved_geometry {
                    "solved_section_point"
                } else {
                    "unresolved_section_point"
                },
                if solved_geometry {
                    Exactness::Derived
                } else {
                    Exactness::ByteExact
                },
            );
            let construction = !unique_external_id || !profile_entities.contains(&id);
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction,
                native_ref: Some(sketch_native_ref(&sketch_id)),
                geometry_ref: None,
                endpoint_refs: vec![sketch_point_ref(&sketch_id, segment.point_id)],
                geometry,
            });
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| &table.centered_line_rows)
        {
            let unique_external_id = unique_segment_ids.contains(&segment.external_id);
            if unique_external_id
                && materialized_saved_section_external_ids.contains(&segment.external_id)
            {
                continue;
            }
            let suffix = if unique_external_id {
                segment.external_id.to_string()
            } else {
                format!("centered_line:offset:{}", segment.offset)
            };
            let id = sketch_entity_id(&sketch_id, &suffix);
            let geometry = centered_line_geometries
                .get(&segment.offset)
                .cloned()
                .unwrap_or_else(|| SketchGeometry::Native {
                    native_kind: "line".to_string(),
                });
            let solved_geometry = matches!(geometry, SketchGeometry::Line { .. });
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                segment.offset as u64,
                if solved_geometry {
                    "solved_section_centered_line"
                } else {
                    "unresolved_section_centered_line"
                },
                if solved_geometry {
                    Exactness::Derived
                } else {
                    Exactness::ByteExact
                },
            );
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction: true,
                native_ref: Some(sketch_native_ref(&sketch_id)),
                geometry_ref: placed_sketch_curve_ref(transform, &sketch_id, suffix, &geometry),
                endpoint_refs: [0, 1]
                    .into_iter()
                    .map(|point| sketch_point_ref(&sketch_id, point))
                    .collect(),
                geometry,
            });
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| &table.reference_line_rows)
        {
            let unique_external_id = unique_segment_ids.contains(&segment.external_id);
            if unique_external_id
                && materialized_saved_section_external_ids.contains(&segment.external_id)
            {
                continue;
            }
            let suffix = if unique_external_id {
                segment.external_id.to_string()
            } else {
                format!("reference_line:offset:{}", segment.offset)
            };
            let id = sketch_entity_id(&sketch_id, &suffix);
            let geometry = reference_line_geometries
                .get(&segment.offset)
                .cloned()
                .unwrap_or_else(|| SketchGeometry::Native {
                    native_kind: "reference_line".to_string(),
                });
            let solved_geometry = matches!(geometry, SketchGeometry::ReferenceLine { .. });
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                segment.offset as u64,
                if solved_geometry {
                    "solved_section_reference_line"
                } else {
                    "unresolved_section_reference_line"
                },
                if solved_geometry {
                    Exactness::Derived
                } else {
                    Exactness::ByteExact
                },
            );
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction: true,
                native_ref: Some(sketch_native_ref(&sketch_id)),
                geometry_ref: placed_sketch_curve_ref(transform, &sketch_id, suffix, &geometry),
                endpoint_refs: segment
                    .point_ids
                    .into_iter()
                    .flatten()
                    .map(|point| sketch_point_ref(&sketch_id, point))
                    .collect(),
                geometry,
            });
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| &table.bounded_curve_rows)
        {
            let unique_external_id = unique_segment_ids.contains(&segment.external_id);
            if unique_external_id
                && materialized_saved_section_external_ids.contains(&segment.external_id)
            {
                continue;
            }
            let suffix = if unique_external_id {
                segment.external_id.to_string()
            } else {
                format!("bounded_curve:offset:{}", segment.offset)
            };
            let id = sketch_entity_id(&sketch_id, &suffix);
            let construction = !unique_external_id || !profile_entities.contains(&id);
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                segment.offset as u64,
                "unresolved_section_bounded_curve",
                Exactness::ByteExact,
            );
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction,
                native_ref: Some(sketch_native_ref(&sketch_id)),
                geometry_ref: None,
                endpoint_refs: segment
                    .point_ids
                    .into_iter()
                    .map(|point| sketch_point_ref(&sketch_id, point))
                    .collect(),
                geometry: SketchGeometry::Native {
                    native_kind: "bounded_curve".to_string(),
                },
            });
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| &table.conic_rows)
        {
            let unique_external_id = unique_segment_ids.contains(&segment.external_id);
            if unique_external_id
                && materialized_saved_section_external_ids.contains(&segment.external_id)
            {
                continue;
            }
            let suffix = if unique_external_id {
                segment.external_id.to_string()
            } else {
                format!("conic:offset:{}", segment.offset)
            };
            let id = sketch_entity_id(&sketch_id, suffix);
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                segment.offset as u64,
                "unresolved_section_conic",
                Exactness::ByteExact,
            );
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction: true,
                native_ref: Some(sketch_native_ref(&sketch_id)),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Native {
                    native_kind: "conic".to_string(),
                },
            });
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| &table.opaque_rows)
        {
            let unique_external_id = unique_segment_ids.contains(&segment.external_id);
            if unique_external_id
                && materialized_saved_section_external_ids.contains(&segment.external_id)
            {
                continue;
            }
            let suffix = opaque_section_segment_identity_suffix(&unique_segment_ids, segment);
            let id = sketch_entity_id(&sketch_id, suffix);
            let geometry = if unique_external_id {
                let native_kind =
                    match unique_section_incidence_curve_family(definition, segment.external_id) {
                        Some(SectionEntityIncidenceFamily::BoundedCurve) => {
                            "bounded_curve".to_string()
                        }
                        Some(SectionEntityIncidenceFamily::Line) => "line".to_string(),
                        Some(SectionEntityIncidenceFamily::Arc) => "arc".to_string(),
                        Some(SectionEntityIncidenceFamily::Circular) => "circle".to_string(),
                        _ => format!("segment_type:{}", segment.kind),
                    };
                SketchGeometry::Native { native_kind }
            } else {
                SketchGeometry::Native {
                    native_kind: format!("segment_type:{}", segment.kind),
                }
            };
            let construction = !unique_external_id || !profile_entities.contains(&id);
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                segment.offset as u64,
                "opaque_section_segment",
                Exactness::ByteExact,
            );
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction,
                native_ref: Some(sketch_native_ref(&sketch_id)),
                geometry_ref: placed_sketch_curve_ref(
                    transform,
                    &sketch_id,
                    if unique_external_id {
                        segment.external_id.to_string()
                    } else {
                        format!("opaque:offset:{}", segment.offset)
                    },
                    &geometry,
                ),
                endpoint_refs: Vec::new(),
                geometry,
            });
        }
        let mut saved_section_geometries = Vec::new();
        let mut generated_saved_geometries = Vec::new();
        for (internal_id, geometry, offset) in
            semantic_saved_section_entities(definition).filter_map(saved_section_entity_geometry)
        {
            let unique_internal_id = unique_saved_ids.contains(&internal_id);
            let external_id = if unique_internal_id {
                definition.order_table.as_ref().and_then(|order| {
                    saved_section_external_id(
                        order,
                        &unique_saved_ids,
                        &ambiguous_segment_ids,
                        internal_id,
                    )
                })
            } else {
                None
            };
            let suffix = if unique_internal_id {
                external_id.map_or_else(
                    || format!("saved{internal_id}"),
                    |external_id| external_id.to_string(),
                )
            } else {
                format!("saved:offset:{offset}")
            };
            let entity_id = sketch_entity_id(&sketch_id, &suffix);
            if entities.iter().any(|entity| entity.id == entity_id) {
                continue;
            }
            let generated = external_id.is_some_and(|external_id| {
                section_generated_profile_surface_kinds(&geometry).is_some_and(|expected_kinds| {
                    section_entity_is_generated_profile(
                        complete_segment_table,
                        definition.owner_feature_id,
                        external_id,
                        expected_kinds,
                        &scan.features.entity_tables,
                        &scan.surfaces.rows,
                    )
                })
            });
            let curve_id = CurveId(sketch_section_curve_id(&sketch_id, &suffix));
            annotate(
                annotations,
                &entity_id.0,
                "FeatDefs",
                offset as u64,
                "saved_section_entity",
                Exactness::Derived,
            );
            if let Some(external_id) = external_id.filter(|_| generated) {
                generated_saved_geometries.push((external_id, geometry.clone()));
            }
            entities.push(SketchEntity {
                id: entity_id,
                sketch: sketch_id.clone(),
                construction: !generated,
                native_ref: Some(format!(
                    "{}:saved_entity#{internal_id}",
                    sketch_native_ref(&sketch_id)
                )),
                geometry_ref: placed_sketch_curve_ref(transform, &sketch_id, &suffix, &geometry),
                endpoint_refs: Vec::new(),
                geometry: geometry.clone(),
            });
            saved_section_geometries.push((internal_id, external_id, geometry, offset, curve_id));
        }
        for spline in
            semantic_saved_section_entities(definition).filter_map(|entity| match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
        {
            let Some(geometry) = saved_spline_sketch_geometry(spline) else {
                continue;
            };
            let unique_internal_id = spline
                .entity_id
                .is_some_and(|id| unique_saved_ids.contains(&id));
            let suffix = if unique_internal_id {
                spline
                    .entity_id
                    .expect("unique saved spline has an internal id")
                    .to_string()
            } else {
                format!("offset{}", spline.offset)
            };
            let external_id = if unique_internal_id {
                definition.order_table.as_ref().and_then(|order| {
                    saved_section_external_id(
                        order,
                        &unique_saved_ids,
                        &ambiguous_segment_ids,
                        spline.entity_id?,
                    )
                })
            } else {
                None
            };
            let generated = external_id.is_some_and(|external_id| {
                let Some(expected_kinds) = section_generated_profile_surface_kinds(&geometry)
                else {
                    return false;
                };
                section_entity_is_generated_profile(
                    complete_segment_table,
                    definition.owner_feature_id,
                    external_id,
                    expected_kinds,
                    &scan.features.entity_tables,
                    &scan.surfaces.rows,
                )
            });
            let entity_id = external_id.map_or_else(
                || {
                    SketchEntityId(format!(
                        "creo:featdefs:saved_spline#{}:{suffix}",
                        sketch_identity_scope(&sketch_id)
                    ))
                },
                |external_id| sketch_entity_id(&sketch_id, external_id),
            );
            let curve_id = CurveId(format!(
                "creo:featdefs:saved_spline_curve#{}:{suffix}",
                sketch_identity_scope(&sketch_id)
            ));
            if entities.iter().any(|entity| entity.id == entity_id) {
                continue;
            }
            annotate(
                annotations,
                &entity_id.0,
                "FeatDefs",
                spline.offset as u64,
                "saved_interpolation_spline",
                Exactness::Derived,
            );
            entities.push(SketchEntity {
                id: entity_id,
                sketch: sketch_id.clone(),
                construction: !generated,
                native_ref: Some(format!(
                    "{}:saved_spline#{suffix}",
                    sketch_native_ref(&sketch_id)
                )),
                geometry_ref: transform.map(|_| curve_id.0.clone()),
                endpoint_refs: Vec::new(),
                geometry: geometry.clone(),
            });
            if let Some(external_id) = external_id.filter(|_| generated) {
                generated_saved_geometries.push((external_id, geometry));
            }
        }
        for saved in semantic_saved_section_entities(definition) {
            let (entity, offset) = unresolved_saved_section_entity(
                definition,
                &sketch_id,
                saved,
                &unique_saved_ids,
                &ambiguous_segment_ids,
            );
            if entities.iter().any(|existing| existing.id == entity.id) {
                continue;
            }
            annotate(
                annotations,
                &entity.id.0,
                "FeatDefs",
                offset as u64,
                "unresolved_saved_section_entity",
                Exactness::ByteExact,
            );
            entities.push(entity);
        }
        profiles.extend(saved_profile_chains(
            &sketch_id,
            &generated_saved_geometries,
        ));
        if let Some(transform) = transform {
            for segment in segments {
                let Some(section_geometry) = resolved_segment_geometries
                    .get(&segment.offset)
                    .cloned()
                    .flatten()
                    .or_else(|| {
                        segment_geometries
                            .get(&segment.offset)
                            .cloned()
                            .flatten()
                            .filter(|geometry| {
                                matches!(geometry, SketchGeometry::ReferenceLine { .. })
                            })
                    })
                else {
                    continue;
                };
                let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry)
                else {
                    continue;
                };
                let suffix = section_segment_identity_suffix(&unique_segment_ids, segment);
                let id = CurveId(sketch_section_curve_id(&sketch_id, &suffix));
                if ir.model.curves.iter().any(|existing| existing.id == id) {
                    continue;
                }
                annotate(
                    annotations,
                    &id,
                    "FeatDefs",
                    segment.offset as u64,
                    "placed_section_curve",
                    Exactness::Derived,
                );
                ir.model.curves.push(Curve {
                    id,
                    geometry,
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: format!(
                            "FeatDefs:section#{}:{suffix}",
                            sketch_identity_scope(&sketch_id)
                        ),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
            }
            for segment in definition
                .segments
                .iter()
                .flat_map(|segments| &segments.circle_rows)
            {
                let Some(section_geometry) = circle_geometries.get(&segment.offset).cloned() else {
                    continue;
                };
                let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry)
                else {
                    continue;
                };
                let suffix = if unique_segment_ids.contains(&segment.external_id) {
                    segment.external_id.to_string()
                } else {
                    format!("circle:offset:{}", segment.offset)
                };
                let id = CurveId(sketch_section_curve_id(&sketch_id, &suffix));
                if ir.model.curves.iter().any(|existing| existing.id == id) {
                    continue;
                }
                annotate(
                    annotations,
                    &id,
                    "FeatDefs",
                    segment.offset as u64,
                    "placed_section_circle",
                    Exactness::Derived,
                );
                ir.model.curves.push(Curve {
                    id,
                    geometry,
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: format!(
                            "FeatDefs:section#{}:{suffix}",
                            sketch_identity_scope(&sketch_id)
                        ),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
            }
            for segment in definition
                .segments
                .iter()
                .flat_map(|segments| &segments.centered_line_rows)
            {
                let Some(section_geometry) = centered_line_geometries.get(&segment.offset).cloned()
                else {
                    continue;
                };
                let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry)
                else {
                    continue;
                };
                let suffix = if unique_segment_ids.contains(&segment.external_id) {
                    segment.external_id.to_string()
                } else {
                    format!("centered_line:offset:{}", segment.offset)
                };
                let id = CurveId(sketch_section_curve_id(&sketch_id, &suffix));
                if ir.model.curves.iter().any(|existing| existing.id == id) {
                    continue;
                }
                annotate(
                    annotations,
                    &id,
                    "FeatDefs",
                    segment.offset as u64,
                    "placed_section_line",
                    Exactness::Derived,
                );
                ir.model.curves.push(Curve {
                    id,
                    geometry,
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: format!(
                            "FeatDefs:section#{}:{suffix}",
                            sketch_identity_scope(&sketch_id)
                        ),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
            }
            for (internal_id, external_id, section_geometry, offset, id) in saved_section_geometries
            {
                if ir.model.curves.iter().any(|existing| existing.id == id) {
                    continue;
                }
                let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry)
                else {
                    continue;
                };
                annotate(
                    annotations,
                    &id,
                    "FeatDefs",
                    offset as u64,
                    "placed_saved_section_curve",
                    Exactness::Derived,
                );
                ir.model.curves.push(Curve {
                    id,
                    geometry,
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: external_id.map_or_else(
                            || format!("FeatDefs:saved_entity#{internal_id}"),
                            |external_id| {
                                format!(
                                    "FeatDefs:section#{}:{external_id}",
                                    sketch_identity_scope(&sketch_id)
                                )
                            },
                        ),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
            }
        }
        for (external_id, offset) in solver_only_section_entities(definition) {
            let id = sketch_entity_id(&sketch_id, external_id);
            if entities.iter().any(|entity| entity.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                offset as u64,
                "solver_only_section_entity",
                Exactness::ByteExact,
            );
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction: true,
                native_ref: Some(sketch_native_ref(&sketch_id)),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Native {
                    native_kind: match solver_only_section_entity_family(definition, external_id) {
                        Some(SectionEntityIncidenceFamily::Point) => "point",
                        Some(SectionEntityIncidenceFamily::BoundedCurve) => "bounded_curve",
                        Some(SectionEntityIncidenceFamily::Line) => "line",
                        Some(SectionEntityIncidenceFamily::Arc) => "arc",
                        Some(SectionEntityIncidenceFamily::Circular) => "circle",
                        None => "solver_only_section_entity",
                    }
                    .to_string(),
                },
            });
        }
        let emitted_entity_ids = entities
            .iter()
            .map(|entity| entity.id.clone())
            .collect::<BTreeSet<_>>();
        let emitted_entity_geometry = entities
            .iter()
            .map(|entity| (entity.id.clone(), entity.geometry.clone()))
            .collect::<BTreeMap<_, _>>();
        let verhor_definitions = segments
            .iter()
            .filter_map(|segment| {
                let suffix = section_segment_identity_suffix(&unique_segment_ids, segment);
                let entity = sketch_entity_id(&sketch_id, &suffix);
                Some((
                    suffix,
                    section_segment_verhor_definition(segment, &sketch_id, entity)?,
                    segment.offset,
                ))
            })
            .chain(
                definition
                    .segments
                    .iter()
                    .flat_map(|table| &table.centered_line_rows)
                    .map(|segment| {
                        let suffix = if unique_segment_ids.contains(&segment.external_id) {
                            segment.external_id.to_string()
                        } else {
                            format!("centered_line:offset:{}", segment.offset)
                        };
                        let entity = sketch_entity_id(&sketch_id, &suffix);
                        (
                            suffix,
                            native_section_segment_verhor_definition(
                                &sketch_id,
                                entity,
                                segment.external_id,
                                0,
                            ),
                            segment.offset,
                        )
                    }),
            )
            .chain(
                definition
                    .segments
                    .iter()
                    .flat_map(|table| &table.bounded_curve_rows)
                    .filter_map(|segment| {
                        let verhor = segment.vertical_horizontal?;
                        let suffix = if unique_segment_ids.contains(&segment.external_id) {
                            segment.external_id.to_string()
                        } else {
                            format!("bounded_curve:offset:{}", segment.offset)
                        };
                        let entity = sketch_entity_id(&sketch_id, &suffix);
                        Some((
                            suffix,
                            native_section_segment_verhor_definition(
                                &sketch_id,
                                entity,
                                segment.external_id,
                                verhor,
                            ),
                            segment.offset,
                        ))
                    }),
            )
            .chain(
                definition
                    .segments
                    .iter()
                    .flat_map(|table| &table.reference_line_rows)
                    .filter_map(|segment| {
                        let verhor = segment.vertical_horizontal?;
                        let suffix = if unique_segment_ids.contains(&segment.external_id) {
                            segment.external_id.to_string()
                        } else {
                            format!("reference_line:offset:{}", segment.offset)
                        };
                        let entity = sketch_entity_id(&sketch_id, &suffix);
                        Some((
                            suffix,
                            native_section_segment_verhor_definition(
                                &sketch_id,
                                entity,
                                segment.external_id,
                                verhor,
                            ),
                            segment.offset,
                        ))
                    }),
            )
            .chain(
                definition
                    .segments
                    .iter()
                    .flat_map(|table| &table.opaque_rows)
                    .filter_map(|segment| {
                        let verhor = segment.vertical_horizontal?;
                        let suffix =
                            opaque_section_segment_identity_suffix(&unique_segment_ids, segment);
                        let entity = sketch_entity_id(&sketch_id, &suffix);
                        Some((
                            suffix,
                            native_section_segment_verhor_definition(
                                &sketch_id,
                                entity,
                                segment.external_id,
                                verhor,
                            ),
                            segment.offset,
                        ))
                    }),
            );
        let mut constraints = verhor_definitions
            .filter_map(|(suffix, mut constraint_definition, offset)| {
                reconcile_constraint_entity_references(
                    &mut constraint_definition,
                    &emitted_entity_ids,
                )
                .then_some(())?;
                let id = sketch_constraint_id(&sketch_id, format_args!("verhor:{suffix}"));
                annotate(
                    annotations,
                    &id.0,
                    "FeatDefs",
                    offset as u64,
                    "section_verhor_constraint",
                    Exactness::ByteExact,
                );
                Some(SketchConstraint {
                    id,
                    sketch: sketch_id.clone(),
                    definition: constraint_definition,
                    name: None,
                    driving: None,
                    active: None,
                    virtual_space: None,
                    visible: None,
                    orientation: None,
                    label_distance: None,
                    label_position: None,
                    metadata: None,
                    native_ref: Some(sketch_native_ref(&sketch_id)),
                })
            })
            .collect::<Vec<_>>();
        for (mut constraint, offset) in section_dimension_constraints(definition, &sketch_id) {
            if !reconcile_constraint_entity_references(
                &mut constraint.definition,
                &emitted_entity_ids,
            ) {
                continue;
            }
            annotate(
                annotations,
                &constraint.id.0,
                "FeatDefs",
                offset as u64,
                "section_dimension_constraint",
                Exactness::ByteExact,
            );
            constraints.push(constraint);
        }
        for (mut constraint, offset) in section_segment_radius_constraints(definition, &sketch_id) {
            if !reconcile_constraint_entity_references(
                &mut constraint.definition,
                &emitted_entity_ids,
            ) {
                continue;
            }
            annotate(
                annotations,
                &constraint.id.0,
                "FeatDefs",
                offset as u64,
                "section_segment_radius_constraint",
                Exactness::ByteExact,
            );
            constraints.push(constraint);
        }
        for (mut constraint, offset) in section_skamp_constraints_for_geometry(
            definition,
            &sketch_id,
            Some(&emitted_entity_geometry),
        ) {
            if !reconcile_constraint_entity_references(
                &mut constraint.definition,
                &emitted_entity_ids,
            ) {
                continue;
            }
            annotate(
                annotations,
                &constraint.id.0,
                "FeatDefs",
                offset as u64,
                "section_solver_constraint",
                Exactness::ByteExact,
            );
            constraints.push(constraint);
        }
        ir.model.sketch_entities.extend(entities);
        ir.model.sketch_constraints.extend(constraints);
        let source_offset = transform.map_or(definition.offset, |transform| transform.offset);
        annotate(
            annotations,
            &sketch_id.0,
            "FeatDefs",
            source_offset as u64,
            if transform.is_some() {
                "datum_placed_section"
            } else {
                "unplaced_section"
            },
            Exactness::Derived,
        );
        ir.model.sketches.push(Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            visible: None,
            placement: transform.map_or(
                cadmpeg_ir::sketches::SketchPlacement::Unresolved,
                |transform| cadmpeg_ir::sketches::SketchPlacement::Resolved {
                    origin: Point3::new(
                        transform.origin[0],
                        transform.origin[1],
                        transform.origin[2],
                    ),
                    normal: Vector3::new(
                        transform.normal[0],
                        transform.normal[1],
                        transform.normal[2],
                    ),
                    u_axis: Vector3::new(
                        transform.u_axis[0],
                        transform.u_axis[1],
                        transform.u_axis[2],
                    ),
                },
            ),
            profiles,
            native_ref: Some(sketch_native_ref(&sketch_id)),
        });
        if owned_section_feature_id(scan, definition.id).is_none() {
            let feature_id = sketch_feature_id(&sketch_id);
            annotate(
                annotations,
                &feature_id.0,
                "FeatDefs",
                source_offset as u64,
                "section_sketch_feature",
                Exactness::Derived,
            );
            ir.model.features.push(Feature {
                id: feature_id,
                ordinal: ir.model.features.len() as u64,
                name: None,
                suppressed: Some(false),
                parent: None,
                dependencies: Vec::new(),
                source_properties: BTreeMap::new(),
                source_tag: Some("section".to_string()),
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
                definition: IrFeatureDefinition::Sketch {
                    space: cadmpeg_ir::features::SketchSpace::default(),
                    sketch: Some(sketch_id.clone()),
                },
                native_ref: Some(sketch_native_ref(&sketch_id)),
            });
        }
    }
    coverage
}
