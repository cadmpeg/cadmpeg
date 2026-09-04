// SPDX-License-Identifier: Apache-2.0
//! Sketch arena transfer from feature section tables.

use super::super::coverage::SketchSegmentTransferCoverage;
use super::super::feature_history::{
    owned_section_feature_id, planned_feature_dimension_parameter_ids,
    section_entity_is_generated_profile, section_generated_profile_surface_kinds,
};
use super::super::native::annotate;
use super::super::sketch::{
    resolved_section_coordinates, resolved_section_radii, resolved_section_reference_line_geometry,
    resolved_section_segment_geometry_with_missing_line, resolved_trim_vertex_coordinates,
    saved_profile_chains, saved_section_missing_line_geometry,
    section_axis_reference_line_geometry, section_centered_line_geometry, section_circle_geometry,
    section_point_row_geometry, section_segment_rows, trim_segment_id,
    trimmed_section_segment_geometry_with_missing_line,
};
use super::super::sketch_ids::{
    feature_definition_has_sketch_design, model_sketch_id, sketch_constraint_id, sketch_entity_id,
    sketch_feature_id, sketch_native_ref,
};
use super::super::uniqueness::unique_feature_section_transform;
use super::entities::transfer_section_entities;
use super::{
    ambiguous_section_segment_external_ids, materialized_saved_section_external_ids,
    native_section_segment_verhor_definition, opaque_section_segment_identity_suffix,
    reconcile_constraint_entity_references, reconcile_constraint_parameter_reference,
    reconcile_section_dimension_constraint, resolved_profile_chains, section_degenerate_axis_line,
    section_dimension_constraints, section_equation_axis_distance_constraints,
    section_equation_equal_distance_constraints,
    section_equation_function_five_scalar_equality_constraints,
    section_equation_function_forty_two_midpoint_coordinate_constraints,
    section_equation_function_six_distance_constraints,
    section_equation_function_sixteen_angle_difference_constraints,
    section_equation_function_thirty_one_point_coordinate_constraints,
    section_equation_native_constraints, section_equation_point_on_line_constraints,
    section_equation_polar_distance_constraints, section_equation_radius_dimension_constraints,
    section_equation_same_coordinate_constraints, section_equation_unsigned_distance_constraints,
    section_segment_identity_suffix, section_segment_radius_constraints_for_emitted,
    section_segment_verhor_definition, section_skamp_constraints_for_geometry,
    solver_only_section_entities, solver_only_section_entity_family,
    unique_saved_section_internal_ids, unique_section_segment_external_ids,
    SectionEntityIncidenceFamily,
};
use crate::container::ContainerScan;
use crate::coverage::SketchSegmentFamily;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::Feature;
use cadmpeg_ir::features::FeatureDefinition as IrFeatureDefinition;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchConstraint, SketchEntity, SketchGeometry};
use cadmpeg_ir::{AnnotationBuilder, Exactness};
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn transfer_sketches(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> SketchSegmentTransferCoverage {
    let mut coverage = SketchSegmentTransferCoverage::default();
    let mut available_parameter_ids = ir
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.id.clone())
        .collect::<BTreeSet<_>>();
    available_parameter_ids.extend(planned_feature_dimension_parameter_ids(scan));
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
                    crate::feature::FeatureSegmentKind::Line => SketchSegmentFamily::Line,
                    crate::feature::FeatureSegmentKind::Arc => SketchSegmentFamily::Arc,
                    crate::feature::FeatureSegmentKind::Point => SketchSegmentFamily::Point,
                };
                coverage.family_mut(family).0 += 1;
            }
            for (family, count) in [
                (SketchSegmentFamily::Circle, table.circle_rows.len()),
                (SketchSegmentFamily::Point, table.point_rows.len()),
                (
                    SketchSegmentFamily::CenteredLine,
                    table.centered_line_rows.len(),
                ),
                (
                    SketchSegmentFamily::ReferenceLine,
                    table.reference_line_rows.len(),
                ),
                (
                    SketchSegmentFamily::BoundedCurve,
                    table.bounded_curve_rows.len(),
                ),
                (SketchSegmentFamily::Conic, table.conic_rows.len()),
                (SketchSegmentFamily::Opaque, table.opaque_rows.len()),
            ] {
                coverage.family_mut(family).0 += count;
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
                crate::feature::FeatureSegmentKind::Line => SketchSegmentFamily::Line,
                crate::feature::FeatureSegmentKind::Arc => SketchSegmentFamily::Arc,
                crate::feature::FeatureSegmentKind::Point => SketchSegmentFamily::Point,
            };
            coverage.family_mut(family).1 += 1;
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
        coverage.family_mut(SketchSegmentFamily::Circle).1 += resolved_circles;
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
        coverage.family_mut(SketchSegmentFamily::Point).1 += resolved_points;
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
        coverage.family_mut(SketchSegmentFamily::CenteredLine).1 += resolved_centered_lines;
        let resolved_reference_lines = definition
            .segments
            .iter()
            .flat_map(|table| &table.reference_line_rows)
            .filter(|segment| reference_line_geometries.contains_key(&segment.offset))
            .count();
        coverage.resolved_geometry += resolved_reference_lines;
        coverage.family_mut(SketchSegmentFamily::ReferenceLine).1 += resolved_reference_lines;
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
        coverage.family_mut(SketchSegmentFamily::BoundedCurve).1 += resolved_bounded_curves;
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
        coverage.family_mut(SketchSegmentFamily::Conic).1 += resolved_conics;
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
        coverage.family_mut(SketchSegmentFamily::Opaque).1 += resolved_opaque;
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
        let (mut entities, profiles) = transfer_section_entities(
            scan,
            ir,
            annotations,
            definition,
            transform,
            &sketch_id,
            segments,
            &unique_segment_ids,
            &unique_saved_ids,
            &ambiguous_segment_ids,
            complete_segment_table,
            &solved,
            &segment_geometries,
            &resolved_segment_geometries,
            &circle_geometries,
            &point_geometries,
            &centered_line_geometries,
            &reference_line_geometries,
            &materialized_saved_section_external_ids,
            profiles,
            &profile_entities,
        );
        for (external_id, offset) in solver_only_section_entities(definition) {
            let id = sketch_entity_id(&sketch_id, external_id);
            if entities.iter().any(|entity| entity.id() == &id) {
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
            entities.push(
                SketchEntity::new(
                    id,
                    sketch_id.clone(),
                    SketchGeometry::Native {
                        native_kind: match solver_only_section_entity_family(
                            definition,
                            external_id,
                        ) {
                            Some(SectionEntityIncidenceFamily::Point) => "point",
                            Some(SectionEntityIncidenceFamily::BoundedCurve) => "bounded_curve",
                            Some(SectionEntityIncidenceFamily::Line) => "line",
                            Some(SectionEntityIncidenceFamily::Arc) => "arc",
                            Some(SectionEntityIncidenceFamily::Circular) => "circle",
                            None => "solver_only_section_entity",
                        }
                        .to_string(),
                    },
                )
                .with_construction(true)
                .with_native_ref(Some(sketch_native_ref(&sketch_id))),
            );
        }
        let emitted_entity_ids = entities
            .iter()
            .map(|entity| entity.id().clone())
            .collect::<BTreeSet<_>>();
        let emitted_entity_geometry = entities
            .iter()
            .map(|entity| (entity.id().clone(), entity.geometry.clone()))
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
        for (relation_index, (mut constraint, offset)) in
            section_dimension_constraints(definition, &sketch_id)
                .into_iter()
                .enumerate()
        {
            let Some(relation) = definition
                .relations
                .as_ref()
                .and_then(|relations| relations.rows.get(relation_index))
            else {
                continue;
            };
            if !reconcile_section_dimension_constraint(
                &mut constraint.definition,
                definition,
                &sketch_id,
                relation,
                &emitted_entity_ids,
                &available_parameter_ids,
            ) {
                continue;
            }
            annotate(
                annotations,
                &constraint.id.as_str(),
                "FeatDefs",
                offset as u64,
                "section_dimension_constraint",
                Exactness::ByteExact,
            );
            constraints.push(constraint);
        }
        for (constraint, offset) in section_segment_radius_constraints_for_emitted(
            definition,
            &sketch_id,
            &emitted_entity_ids,
            &available_parameter_ids,
        ) {
            annotate(
                annotations,
                &constraint.id.as_str(),
                "FeatDefs",
                offset as u64,
                "section_segment_radius_constraint",
                Exactness::ByteExact,
            );
            constraints.push(constraint);
        }
        let equation_constraints =
            section_equation_axis_distance_constraints(definition, &sketch_id)
                .into_iter()
                .chain(section_equation_unsigned_distance_constraints(
                    definition, &sketch_id,
                ))
                .chain(section_equation_point_on_line_constraints(
                    definition, &sketch_id,
                ))
                .chain(section_equation_same_coordinate_constraints(
                    definition, &sketch_id,
                ))
                .chain(
                    section_equation_function_thirty_one_point_coordinate_constraints(
                        definition, &sketch_id,
                    ),
                )
                .chain(
                    section_equation_function_forty_two_midpoint_coordinate_constraints(
                        definition, &sketch_id,
                    ),
                )
                .chain(section_equation_function_five_scalar_equality_constraints(
                    definition, &sketch_id,
                ))
                .chain(
                    section_equation_function_sixteen_angle_difference_constraints(
                        definition, &sketch_id,
                    ),
                )
                .chain(section_equation_radius_dimension_constraints(
                    definition, &sketch_id,
                ))
                .chain(section_equation_polar_distance_constraints(
                    definition, &sketch_id,
                ))
                .chain(section_equation_function_six_distance_constraints(
                    definition, &sketch_id,
                ))
                .chain(section_equation_equal_distance_constraints(
                    definition, &sketch_id,
                ))
                .collect::<Vec<_>>();
        let equation_offsets = equation_constraints
            .iter()
            .map(|(_, offset)| *offset)
            .collect::<BTreeSet<_>>();
        let mut rejected_equation_offsets = BTreeSet::new();
        let mut reconciled_equation_constraints = Vec::new();
        for (mut constraint, offset) in equation_constraints {
            let entity_reconciled = reconcile_constraint_entity_references(
                &mut constraint.definition,
                &emitted_entity_ids,
            );
            let parameter_reconciled = reconcile_constraint_parameter_reference(
                &mut constraint.definition,
                &available_parameter_ids,
            );
            if !entity_reconciled || !parameter_reconciled {
                rejected_equation_offsets.insert(offset);
                continue;
            }
            reconciled_equation_constraints.push((constraint, offset));
        }
        let mut typed_equation_offsets = BTreeSet::new();
        for (constraint, offset) in reconciled_equation_constraints {
            if rejected_equation_offsets.contains(&offset) {
                continue;
            }
            annotate(
                annotations,
                &constraint.id.as_str(),
                "FeatDefs",
                offset as u64,
                "section_equation_constraint",
                Exactness::ByteExact,
            );
            constraints.push(constraint);
        }
        typed_equation_offsets.extend(
            equation_offsets
                .into_iter()
                .filter(|offset| !rejected_equation_offsets.contains(offset)),
        );
        for (constraint, offset) in
            section_equation_native_constraints(definition, &sketch_id, &typed_equation_offsets)
        {
            annotate(
                annotations,
                &constraint.id.as_str(),
                "FeatDefs",
                offset as u64,
                "section_native_equation_constraint",
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
                &constraint.id.as_str(),
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
                dependencies: Vec::new(),
                source_properties: BTreeMap::new(),
                source_tag: Some("section".to_string()),
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
                definition: IrFeatureDefinition::Sketch {
                    sketch: Some(sketch_id.clone()),
                },
                native_ref: Some(sketch_native_ref(&sketch_id)),
            });
        }
    }
    coverage
}
