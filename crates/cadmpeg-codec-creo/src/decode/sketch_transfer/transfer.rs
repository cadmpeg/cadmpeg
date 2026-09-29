// SPDX-License-Identifier: Apache-2.0
//! Sketch arena transfer from feature section tables.

use super::super::coverage::SketchSegmentTransferCoverage;
use super::super::feature_history::dimensions::planned_feature_dimension_parameter_ids;
use super::super::feature_history::link::{
    section_entity_is_generated_profile, section_generated_profile_surface_kinds,
};
use super::super::feature_history::outputs::owned_section_feature_id;
use super::super::native::annotate;
use super::super::sketch::coordinates::resolved_section_coordinates;
use super::super::sketch::geometry::{
    resolved_section_reference_line_geometry, resolved_section_segment_geometry_with_missing_line,
    saved_profile_chains, saved_section_missing_line_geometry, section_centered_line_geometry,
    section_circle_geometry, section_point_row_geometry,
};
use super::super::sketch::intersect::{
    resolved_trim_vertex_coordinates, trimmed_section_segment_geometry_with_missing_line,
};
use super::super::sketch::radii::{
    resolved_section_radii, section_axis_reference_line_geometry, trim_segment_id,
};
use super::super::sketch::skamp::section_segment_rows;
use super::super::sketch_ids::{
    feature_definition_has_sketch_design, model_sketch_id, sketch_constraint_id, sketch_entity_id,
    sketch_feature_id_admitted, sketch_native_ref,
};
use super::super::uniqueness::unique_feature_section_transform;
use super::entities::transfer_section_entities;
use crate::container::ContainerScan;
use crate::coverage::SketchSegmentFamily;
use crate::decode::sketch_transfer::constraints::{
    native_section_segment_verhor_definition, reconcile_constraint_entity_references,
    reconcile_constraint_parameter_reference, reconcile_section_dimension_constraint,
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
    section_segment_radius_constraints_for_emitted, section_segment_verhor_definition,
};
use crate::decode::sketch_transfer::identity::{
    ambiguous_section_segment_external_ids, materialized_saved_section_external_ids,
    opaque_section_segment_identity_suffix, section_segment_identity_suffix,
    unique_saved_section_internal_ids, unique_section_segment_external_ids,
};
use crate::decode::sketch_transfer::loci::section_degenerate_axis_line;
use crate::decode::sketch_transfer::profiles::{
    resolved_profile_chains, solver_only_section_entities, solver_only_section_entity_family,
    SectionEntityIncidenceFamily,
};
use crate::decode::sketch_transfer::skamp_constraints::section_skamp_constraints_for_geometry;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::Feature;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::features::{
    FeatureDefinition as IrFeatureDefinition, FeatureOperation as IrFeatureOperation,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::sketches::{Sketch, SketchConstraint, SketchEntity, SketchGeometry};
use cadmpeg_ir::sketches::SketchEntityId;
use cadmpeg_ir::{AnnotationBuilder, Exactness};
use std::collections::{BTreeMap, BTreeSet};

/// Transfer every sketch-design feature definition into model sketches.
///
/// A saved section entity whose lanes the IR carrier refuses is omitted from
/// the sketch, which the model carries, so the refusal is a loss note naming
/// the feature definition and the entity offset.
pub(in super::super) fn transfer_sketches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<SketchSegmentTransferCoverage, cadmpeg_core::CodecError> {
    let mut coverage = SketchSegmentTransferCoverage::default();
    let available_parameter_ids = available_parameter_ids(
        ctx,
        ir.model.parameters.iter().map(|parameter| &parameter.id),
        planned_feature_dimension_parameter_ids(ctx, scan)?,
    )?;
    for definition in &scan.features.definitions {
        if !feature_definition_has_sketch_design(ctx, definition)? {
            continue;
        }
        let transform = definition.section_3d.as_ref().and_then(|section| {
            unique_feature_section_transform(
                &scan.features.section_transforms,
                definition.identity.id(),
                section.offset,
            )
        });
        let placement = match transform {
            Some(transform) => cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                Point3::from(transform.origin()),
                transform.normal_vector(),
                transform.u_axis_vector(),
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,
            None => cadmpeg_ir::sketches::SketchPlacement::Unresolved {},
        };
        let Some(sketch_id) = model_sketch_id(ctx, scan, definition)? else {
            continue;
        };
        let segments = section_segment_rows(ctx, definition)?;
        let unique_segment_ids = unique_section_segment_external_ids(ctx, definition)?;
        let ambiguous_segment_ids = ambiguous_section_segment_external_ids(ctx, definition)?;
        let unique_saved_ids = unique_saved_section_internal_ids(ctx, definition)?;
        let complete_segment_table = definition
            .segments
            .as_ref()
            .is_some_and(crate::feature::definitions::FeatureSegmentTable::is_complete);
        if let Some(table) = &definition.segments {
            let decoded_rows = table.rows.len();
            let declared_count = usize::try_from(table.declared_count).map_err(|_| {
                cadmpeg_core::CodecError::malformed(format!(
                    "feature {} states segment table count {}, which exceeds the addressable row \
                     range {}",
                    definition.identity.id(),
                    table.declared_count,
                    usize::MAX
                ))
            })?;
            let elided_prototype_rows = usize::from(table.has_elided_prototype);
            let expected_rows = declared_count
                .checked_sub(elided_prototype_rows)
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(format!(
                        "feature {} states segment table count {declared_count} and \
                         {elided_prototype_rows} elided prototype row(s), so the ordinary row \
                         count is below zero",
                        definition.identity.id()
                    ))
                })?;
            coverage.record_table_rows(decoded_rows, expected_rows);
            for segment in table.rows.ordinary() {
                let family = match segment.kind {
                    crate::feature::definitions::FeatureSegmentKind::Line(_) => {
                        SketchSegmentFamily::Line
                    }
                    crate::feature::definitions::FeatureSegmentKind::Arc(_) => {
                        SketchSegmentFamily::Arc
                    }
                    crate::feature::definitions::FeatureSegmentKind::Point(_) => {
                        SketchSegmentFamily::Point
                    }
                };
                coverage.record_family_rows(family, 1);
            }
            for (family, count) in [
                (SketchSegmentFamily::Circle, table.rows.circles().count()),
                (SketchSegmentFamily::Point, table.rows.points().count()),
                (
                    SketchSegmentFamily::CenteredLine,
                    table.rows.centered_lines().count(),
                ),
                (
                    SketchSegmentFamily::ReferenceLine,
                    table.rows.reference_lines().count(),
                ),
                (
                    SketchSegmentFamily::BoundedCurve,
                    table.rows.bounded_curves().count(),
                ),
                (SketchSegmentFamily::Conic, table.rows.conics().count()),
                (SketchSegmentFamily::Opaque, table.rows.opaque().count()),
            ] {
                coverage.record_family_rows(family, count);
            }
        }
        let variable_points = resolved_section_coordinates(ctx, definition)?;
        let mut points = BTreeMap::new();
        for (point, [u, v]) in &variable_points {
            if let (Some(u), Some(v)) = (u, v) {
                insert_tree(
                    ctx,
                    &mut points,
                    *point,
                    [*u, *v],
                    "creo resolved sketch point nodes",
                )?;
            }
        }
        let radii = resolved_section_radii(ctx, definition)?;
        let missing_line_geometry = saved_section_missing_line_geometry(ctx, definition)?;
        let solved = collect_numeric_set(
            ctx,
            definition
                .trim_entities
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| trim_segment_id(definition, row)),
            "creo solved section segment ID nodes",
        )?;
        let trim_vertex_coordinates = resolved_trim_vertex_coordinates(ctx, definition, &points, &radii)?;
        let mut resolved_segment_geometries = BTreeMap::new();
        for segment in &segments {
            insert_tree(
                ctx,
                &mut resolved_segment_geometries,
                segment.offset,
                resolved_section_segment_geometry_with_missing_line(
                    definition,
                    &points,
                    segment,
                    missing_line_geometry.as_ref(),
                ),
                "creo resolved section geometry nodes",
            )?;
        }
        let mut segment_geometries = BTreeMap::new();
        for segment in &segments {
                let geometry = if unique_segment_ids.contains(&segment.external_id)
                    && solved.contains(&segment.external_id)
                {
                    trimmed_section_segment_geometry_with_missing_line(
                        ctx,
                        definition,
                        &points,
                        &radii,
                        &trim_vertex_coordinates,
                        segment,
                        missing_line_geometry.as_ref(),
                    )?
                } else {
                    resolved_segment_geometries
                        .get(&segment.offset)
                        .and_then(Option::as_ref)
                        .map(|geometry| geometry.copy_admitted(ctx, "creo resolved section geometry copy"))
                        .transpose()?
                };
                let geometry = match geometry {
                    Some(geometry) => Some(geometry),
                    None => section_axis_reference_line_geometry(ctx, definition, &variable_points, segment)?,
                };
                insert_tree(ctx, &mut segment_geometries, segment.offset, geometry,
                    "creo section geometry nodes")?;
        }
        let mut circle_geometries = BTreeMap::new();
        let mut point_geometries = BTreeMap::new();
        let mut centered_line_geometries = BTreeMap::new();
        let mut reference_line_geometries = BTreeMap::new();
        if let Some(table) = &definition.segments {
            for segment in table.rows.circles() {
                if let Some(geometry) = section_circle_geometry(&points, &radii, segment) {
                    insert_tree(ctx, &mut circle_geometries, segment.offset, geometry,
                        "creo section circle geometry nodes")?;
                }
            }
            for segment in table.rows.points() {
                if let Some(geometry) = section_point_row_geometry(&points, segment) {
                    insert_tree(ctx, &mut point_geometries, segment.offset, geometry,
                        "creo section point geometry nodes")?;
                }
            }
            for segment in table.rows.centered_lines() {
                if let Some(geometry) = section_centered_line_geometry(&points, segment) {
                    insert_tree(ctx, &mut centered_line_geometries, segment.offset, geometry,
                        "creo section centered-line geometry nodes")?;
                }
            }
            for segment in table.rows.reference_lines() {
                if let Some(geometry) = resolved_section_reference_line_geometry(
                    ctx, definition, &variable_points, &points, segment,
                )? {
                    insert_tree(ctx, &mut reference_line_geometries, segment.offset, geometry,
                        "creo section reference-line geometry nodes")?;
                }
            }
        }
        let emitted = collect_numeric_set(
            ctx,
            segments.iter().filter(|segment| {
                unique_segment_ids.contains(&segment.external_id)
                    && (section_degenerate_axis_line(definition, segment)
                        || segment_geometries.get(&segment.offset).is_some_and(Option::is_some))
            }).map(|segment| segment.external_id)
                .chain(definition.segments.iter().flat_map(|table| table.rows.circles())
                    .filter(|segment| unique_segment_ids.contains(&segment.external_id)
                        && circle_geometries.contains_key(&segment.offset))
                    .map(|segment| segment.external_id)),
            "creo emitted section segment ID nodes",
        )?;
        let resolved_segment_offsets = collect_numeric_set(
            ctx,
            segments.iter().filter(|segment| {
                segment_geometries.get(&segment.offset).is_some_and(Option::is_some)
            }).map(|segment| segment.offset),
            "creo resolved section offset nodes",
        )?;
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let materialized_saved_section_external_ids =
            materialized_saved_section_external_ids(ctx, definition, &mut refusal)?;
        for record in refusal.take_records_checked()? {
            let message = ctx.format_retained(
                format_args!(
                    "Feature {} states a saved section entity that materializes no sketch \
                     geometry: {record}",
                    definition.identity.id()
                ),
                "creo unresolved saved section loss text",
            )?;
            ctx.try_reserve_items(losses, 1, "creo unresolved saved section losses")?;
            losses.push(
                crate::loss::CreoLossCode::SectionSplineUnresolved.note(message),
            );
        }
        coverage.record_resolved_geometry(resolved_segment_offsets.len());
        for segment in segments
            .iter()
            .filter(|segment| resolved_segment_offsets.contains(&segment.offset))
        {
            let family = match segment.kind {
                crate::feature::definitions::FeatureSegmentKind::Line(_) => {
                    SketchSegmentFamily::Line
                }
                crate::feature::definitions::FeatureSegmentKind::Arc(_) => SketchSegmentFamily::Arc,
                crate::feature::definitions::FeatureSegmentKind::Point(_) => {
                    SketchSegmentFamily::Point
                }
            };
            coverage.record_family_resolution(family, 1);
        }
        let resolved_circles = definition
            .segments
            .iter()
            .flat_map(|table| table.rows.circles())
            .filter(|segment| {
                circle_geometries.contains_key(&segment.offset)
                    || (unique_segment_ids.contains(&segment.external_id)
                        && materialized_saved_section_external_ids.contains(&segment.external_id))
            })
            .count();
        coverage.record_resolved_geometry(resolved_circles);
        coverage.record_family_resolution(SketchSegmentFamily::Circle, resolved_circles);
        let resolved_points = definition
            .segments
            .iter()
            .flat_map(|table| table.rows.points())
            .filter(|segment| {
                point_geometries.contains_key(&segment.offset)
                    || (unique_segment_ids.contains(&segment.external_id)
                        && materialized_saved_section_external_ids.contains(&segment.external_id))
            })
            .count();
        coverage.record_resolved_geometry(resolved_points);
        coverage.record_family_resolution(SketchSegmentFamily::Point, resolved_points);
        let resolved_centered_lines = definition
            .segments
            .iter()
            .flat_map(|table| table.rows.centered_lines())
            .filter(|segment| {
                centered_line_geometries.contains_key(&segment.offset)
                    || (unique_segment_ids.contains(&segment.external_id)
                        && materialized_saved_section_external_ids.contains(&segment.external_id))
            })
            .count();
        coverage.record_resolved_geometry(resolved_centered_lines);
        coverage
            .record_family_resolution(SketchSegmentFamily::CenteredLine, resolved_centered_lines);
        let resolved_reference_lines = definition
            .segments
            .iter()
            .flat_map(|table| table.rows.reference_lines())
            .filter(|segment| reference_line_geometries.contains_key(&segment.offset))
            .count();
        coverage.record_resolved_geometry(resolved_reference_lines);
        coverage
            .record_family_resolution(SketchSegmentFamily::ReferenceLine, resolved_reference_lines);
        let resolved_bounded_curves = definition
            .segments
            .iter()
            .flat_map(|table| table.rows.bounded_curves())
            .filter(|segment| {
                unique_segment_ids.contains(&segment.external_id)
                    && materialized_saved_section_external_ids.contains(&segment.external_id)
            })
            .count();
        coverage.record_resolved_geometry(resolved_bounded_curves);
        coverage
            .record_family_resolution(SketchSegmentFamily::BoundedCurve, resolved_bounded_curves);
        let resolved_conics = definition
            .segments
            .iter()
            .flat_map(|table| table.rows.conics())
            .filter(|segment| {
                unique_segment_ids.contains(&segment.external_id)
                    && materialized_saved_section_external_ids.contains(&segment.external_id)
            })
            .count();
        coverage.record_resolved_geometry(resolved_conics);
        coverage.record_family_resolution(SketchSegmentFamily::Conic, resolved_conics);
        let resolved_opaque = definition
            .segments
            .iter()
            .flat_map(|table| table.rows.opaque())
            .filter(|segment| {
                unique_segment_ids.contains(&segment.external_id)
                    && materialized_saved_section_external_ids.contains(&segment.external_id)
            })
            .count();
        coverage.record_resolved_geometry(resolved_opaque);
        coverage.record_family_resolution(SketchSegmentFamily::Opaque, resolved_opaque);
        let mut profiles = resolved_profile_chains(ctx, definition, &sketch_id, &emitted)?;
        let mut generated_profile_geometries = Vec::new();
        for segment in &segments {
            if !unique_segment_ids.contains(&segment.external_id)
                || !emitted.contains(&segment.external_id)
            {
                continue;
            }
            let Some(geometry) = segment_geometries.get(&segment.offset).and_then(Option::as_ref) else {
                continue;
            };
            let Some(expected_kinds) = section_generated_profile_surface_kinds(geometry) else {
                continue;
            };
            if section_entity_is_generated_profile(
                complete_segment_table,
                definition.identity.owner_feature_id(),
                segment.external_id,
                expected_kinds,
                &scan.features.entity_tables,
                &scan.surfaces.rows,
            ) {
                ctx.try_reserve_items(&mut generated_profile_geometries, 1,
                    "creo generated profile geometry rows")?;
                generated_profile_geometries.push((segment.external_id,
                    geometry.copy_admitted(ctx, "creo generated profile geometry copy")?));
            }
        }
        for segment in definition.segments.iter().flat_map(|table| table.rows.circles()) {
            if !unique_segment_ids.contains(&segment.external_id) {
                continue;
            }
            let Some(geometry) = circle_geometries.get(&segment.offset) else {
                continue;
            };
            let Some(expected_kinds) = section_generated_profile_surface_kinds(geometry) else {
                continue;
            };
            if section_entity_is_generated_profile(
                complete_segment_table,
                definition.identity.owner_feature_id(),
                segment.external_id,
                expected_kinds,
                &scan.features.entity_tables,
                &scan.surfaces.rows,
            ) {
                ctx.try_reserve_items(&mut generated_profile_geometries, 1,
                    "creo generated profile geometry rows")?;
                generated_profile_geometries.push((segment.external_id,
                    geometry.copy_admitted(ctx, "creo generated profile geometry copy")?));
            }
        }
        let mut profile_entities = BTreeSet::new();
        for entity_use in profiles.iter().flatten() {
            if !profile_entities.contains(&entity_use.entity) {
                ctx.charge_collection_items(1, "creo profile entity ID nodes")?;
                profile_entities.insert(entity_use.entity.copy_admitted(ctx,
                    "creo profile entity identities")?);
            }
        }
        for profile in saved_profile_chains(ctx, &sketch_id, &generated_profile_geometries)? {
            if profile
                .iter()
                .all(|entity_use| !profile_entities.contains(&entity_use.entity))
            {
                for entity_use in &profile {
                    if !profile_entities.contains(&entity_use.entity) {
                        ctx.charge_collection_items(1, "creo profile entity ID nodes")?;
                        profile_entities.insert(entity_use.entity.copy_admitted(ctx,
                            "creo profile entity identities")?);
                    }
                }
                ctx.try_reserve_items(&mut profiles, 1, "creo merged sketch profile rows")?;
                profiles.push(profile);
            }
        }
        let (mut entities, profiles) = transfer_section_entities(
            ctx,
            scan,
            ir,
            annotations,
            definition,
            transform,
            &sketch_id,
            &segments,
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
            losses,
            source_carriers,
        )?;
        let profiles = cadmpeg_ir::sketches::SketchProfiles::try_from(profiles)
            .map_err(cadmpeg_core::CodecError::malformed)?;
        for (external_id, offset) in solver_only_section_entities(ctx, definition)? {
            let Some(id) = sketch_entity_id(&sketch_id, external_id) else {
                continue;
            };
            if entities.iter().any(|entity| entity.id() == &id) {
                continue;
            }
            annotate(
                annotations,
                id.as_str(),
                "FeatDefs",
                offset as u64,
                "solver_only_section_entity",
                Exactness::ByteExact,
            );
            ctx.charge_entities(1, "admit Creo model sketch_entities")?;
            ctx.try_reserve_items(&mut entities, 1, "creo solver-only sketch entities")?;
            let native_kind = match solver_only_section_entity_family(definition, external_id) {
                Some(SectionEntityIncidenceFamily::Point) => "point",
                Some(SectionEntityIncidenceFamily::BoundedCurve) => "bounded_curve",
                Some(SectionEntityIncidenceFamily::LineOrArc) => "line_or_arc",
                Some(SectionEntityIncidenceFamily::Line) => "line",
                Some(SectionEntityIncidenceFamily::Arc) => "arc",
                Some(SectionEntityIncidenceFamily::Circular) => "circle",
                None => "solver_only_section_entity",
            };
            entities.push(
                SketchEntity::new(
                    id,
                    sketch_id.copy_admitted(ctx, "creo solver-only entity sketch identity")?,
                    SketchGeometry::native(
                        cadmpeg_core::text::NonBlankString::new(
                            ctx.copy_retained_text(native_kind, "creo solver-only native kind")?,
                        )
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed("native_kind must not be empty")
                        })?,
                    ),
                )
                .with_construction(true)
                .with_native_ref(Some(sketch_native_ref(&sketch_id))),
            );
        }
        let (emitted_entity_ids, emitted_entity_geometry) =
            emitted_entity_views(ctx, &entities)?;
        let verhor_definitions = segments
            .iter()
            .filter_map(|segment| {
                let suffix = section_segment_identity_suffix(&unique_segment_ids, segment);
                let entity = sketch_entity_id(&sketch_id, &suffix)?;
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
                    .flat_map(|table| table.rows.centered_lines())
                    .filter_map(|segment| {
                        Some({
                            let suffix = if unique_segment_ids.contains(&segment.external_id) {
                                segment.external_id.to_string()
                            } else {
                                format!("centered_line:offset:{}", segment.offset)
                            };
                            let entity = sketch_entity_id(&sketch_id, &suffix)?;
                            (
                                suffix,
                                native_section_segment_verhor_definition(
                                    &sketch_id,
                                    entity,
                                    segment.external_id,
                                    0,
                                )?,
                                segment.offset,
                            )
                        })
                    }),
            )
            .chain(
                definition
                    .segments
                    .iter()
                    .flat_map(|table| table.rows.bounded_curves())
                    .filter_map(|segment| {
                        let verhor = segment.vertical_horizontal?;
                        let suffix = if unique_segment_ids.contains(&segment.external_id) {
                            segment.external_id.to_string()
                        } else {
                            format!("bounded_curve:offset:{}", segment.offset)
                        };
                        let entity = sketch_entity_id(&sketch_id, &suffix)?;
                        Some((
                            suffix,
                            native_section_segment_verhor_definition(
                                &sketch_id,
                                entity,
                                segment.external_id,
                                verhor,
                            )?,
                            segment.offset,
                        ))
                    }),
            )
            .chain(
                definition
                    .segments
                    .iter()
                    .flat_map(|table| table.rows.reference_lines())
                    .filter_map(|segment| {
                        let verhor = segment.vertical_horizontal?;
                        let suffix = if unique_segment_ids.contains(&segment.external_id) {
                            segment.external_id.to_string()
                        } else {
                            format!("reference_line:offset:{}", segment.offset)
                        };
                        let entity = sketch_entity_id(&sketch_id, &suffix)?;
                        Some((
                            suffix,
                            native_section_segment_verhor_definition(
                                &sketch_id,
                                entity,
                                segment.external_id,
                                verhor,
                            )?,
                            segment.offset,
                        ))
                    }),
            )
            .chain(
                definition
                    .segments
                    .iter()
                    .flat_map(|table| table.rows.opaque())
                    .filter_map(|segment| {
                        let verhor = segment.vertical_horizontal?;
                        let suffix =
                            opaque_section_segment_identity_suffix(&unique_segment_ids, segment);
                        let entity = sketch_entity_id(&sketch_id, &suffix)?;
                        Some((
                            suffix,
                            native_section_segment_verhor_definition(
                                &sketch_id,
                                entity,
                                segment.external_id,
                                verhor,
                            )?,
                            segment.offset,
                        ))
                    }),
            );
        let mut constraints = Vec::new();
        for (suffix, mut constraint_definition, offset) in verhor_definitions {
            if !reconcile_constraint_entity_references(
                &mut constraint_definition,
                &emitted_entity_ids,
            ) {
                continue;
            }
            let Some(id) = sketch_constraint_id(&sketch_id, format_args!("verhor:{suffix}")) else {
                continue;
            };
            let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                constraint_definition,
            ) else {
                continue;
            };
            annotate(
                annotations,
                id.as_str(),
                "FeatDefs",
                offset as u64,
                "section_verhor_constraint",
                Exactness::ByteExact,
            );
            admit_constraint_row(ctx, &mut constraints, SketchConstraint {
                id,
                sketch: sketch_id.copy_admitted(ctx, "creo constraint sketch identity")?,
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
                native_ref: Some(sketch_native_ref(&sketch_id)),
            })?;
        }
        for (relation_index, (mut constraint, offset)) in
            section_dimension_constraints(ctx, definition, &sketch_id)?
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
            let reconciled = match constraint
                .definition
                .edit(|kind| {
                    reconcile_section_dimension_constraint(
                        ctx,
                        kind,
                        definition,
                        &sketch_id,
                        relation,
                        &emitted_entity_ids,
                        &available_parameter_ids,
                    )
                })
            {
                Ok(result) => result?,
                Err(_) => false,
            };
            if !reconciled {
                continue;
            }
            annotate(
                annotations,
                constraint.id.as_str(),
                "FeatDefs",
                offset as u64,
                "section_dimension_constraint",
                Exactness::ByteExact,
            );
            admit_constraint_row(ctx, &mut constraints, constraint)?;
        }
        for (constraint, offset) in section_segment_radius_constraints_for_emitted(
            ctx,
            definition,
            &sketch_id,
            &emitted_entity_ids,
            &available_parameter_ids,
        )? {
            annotate(
                annotations,
                constraint.id.as_str(),
                "FeatDefs",
                offset as u64,
                "section_segment_radius_constraint",
                Exactness::ByteExact,
            );
            admit_constraint_row(ctx, &mut constraints, constraint)?;
        }
        let equation_constraints = crate::decode::collect_items(ctx,
            section_equation_axis_distance_constraints(ctx, definition, &sketch_id)?
                .into_iter()
                .chain(section_equation_unsigned_distance_constraints(
                    ctx, definition, &sketch_id,
                )?)
                .chain(section_equation_point_on_line_constraints(
                    ctx, definition, &sketch_id,
                )?)
                .chain(section_equation_same_coordinate_constraints(
                    ctx, definition, &sketch_id,
                )?)
                .chain(
                    section_equation_function_thirty_one_point_coordinate_constraints(
                        ctx, definition, &sketch_id,
                    )?,
                )
                .chain(
                    section_equation_function_forty_two_midpoint_coordinate_constraints(
                        ctx, definition, &sketch_id,
                    )?,
                )
                .chain(section_equation_function_five_scalar_equality_constraints(
                    ctx, definition, &sketch_id,
                )?)
                .chain(
                    section_equation_function_sixteen_angle_difference_constraints(
                        ctx, definition, &sketch_id,
                    )?,
                )
                .chain(section_equation_radius_dimension_constraints(
                    ctx, definition, &sketch_id,
                )?)
                .chain(section_equation_polar_distance_constraints(
                    ctx, definition, &sketch_id,
                )?)
                .chain(section_equation_function_six_distance_constraints(
                    ctx, definition, &sketch_id,
                )?)
                .chain(section_equation_equal_distance_constraints(
                    ctx, definition, &sketch_id,
                )?)
                , "creo sketch equation constraints")?;
        let equation_offsets = collect_numeric_set(
            ctx,
            equation_constraints.iter().map(|(_, offset)| *offset),
            "creo equation offset nodes",
        )?;
        let mut rejected_equation_offsets = BTreeSet::new();
        let mut reconciled_equation_constraints = Vec::new();
        for (mut constraint, offset) in equation_constraints {
            let entity_reconciled = constraint
                .definition
                .edit(|kind| reconcile_constraint_entity_references(kind, &emitted_entity_ids))
                .unwrap_or(false);
            let parameter_reconciled = constraint
                .definition
                .edit(|kind| {
                    reconcile_constraint_parameter_reference(kind, &available_parameter_ids)
                })
                .unwrap_or(false);
            if !entity_reconciled || !parameter_reconciled {
                insert_set(ctx, &mut rejected_equation_offsets, offset,
                    "creo rejected equation offset nodes")?;
                continue;
            }
            ctx.try_reserve_items(&mut reconciled_equation_constraints, 1,
                "creo reconciled equation rows")?;
            reconciled_equation_constraints.push((constraint, offset));
        }
        for (constraint, offset) in reconciled_equation_constraints {
            if rejected_equation_offsets.contains(&offset) {
                continue;
            }
            annotate(
                annotations,
                constraint.id.as_str(),
                "FeatDefs",
                offset as u64,
                "section_equation_constraint",
                Exactness::ByteExact,
            );
            admit_constraint_row(ctx, &mut constraints, constraint)?;
        }
        let typed_equation_offsets = collect_numeric_set(
            ctx,
            equation_offsets.into_iter()
                .filter(|offset| !rejected_equation_offsets.contains(offset)),
            "creo typed equation offset nodes",
        )?;
        for (constraint, offset) in
            section_equation_native_constraints(ctx, definition, &sketch_id, &typed_equation_offsets)?
        {
            annotate(
                annotations,
                constraint.id.as_str(),
                "FeatDefs",
                offset as u64,
                "section_native_equation_constraint",
                Exactness::ByteExact,
            );
            admit_constraint_row(ctx, &mut constraints, constraint)?;
        }
        for (mut constraint, offset) in section_skamp_constraints_for_geometry(
            ctx,
            definition,
            &sketch_id,
            Some(&emitted_entity_geometry),
        )? {
            if !constraint
                .definition
                .edit(|kind| reconcile_constraint_entity_references(kind, &emitted_entity_ids))
                .unwrap_or(false)
            {
                continue;
            }
            annotate(
                annotations,
                constraint.id.as_str(),
                "FeatDefs",
                offset as u64,
                "section_solver_constraint",
                Exactness::ByteExact,
            );
            admit_constraint_row(ctx, &mut constraints, constraint)?;
        }
        source_carriers.admit_sketch_entities(ctx, ir, entities)?;
        source_carriers.admit_sketch_constraints(ctx, ir, constraints)?;
        let source_offset = transform.map_or(definition.offset, |transform| transform.offset);
        annotate(
            annotations,
            sketch_id.as_str(),
            "FeatDefs",
            source_offset as u64,
            if transform.is_some() {
                "datum_placed_section"
            } else {
                "unplaced_section"
            },
            Exactness::Derived,
        );
        ctx.charge_entities(1, "admit Creo model sketches")?;
        source_carriers.admit_sketch(
            ctx,
            ir,
            Sketch {
                id: sketch_id.copy_admitted(ctx, "creo model sketch identity copy")?,
                name: None,
                configuration: None,
                visible: None,
                placement,
                profiles,
                native_ref: Some(sketch_native_ref(&sketch_id)),
            },
        )?;
        if owned_section_feature_id(scan, definition.identity.id()).is_none() {
            let Some(feature_id) = sketch_feature_id_admitted(ctx, &sketch_id)? else {
                continue;
            };
            annotate(
                annotations,
                feature_id.as_str(),
                "FeatDefs",
                source_offset as u64,
                "section_sketch_feature",
                Exactness::Derived,
            );
            ctx.charge_entities(1, "admit Creo model features")?;
            let feature = Feature {
                id: feature_id,
                ordinal: ir.model.features.len() as u64,
                name: None,
                suppressed: Some(false),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: BTreeMap::new(),
                source_tag: Some(ctx.copy_retained_text("section", "creo sketch feature source tag")?),
                source_text: None,
                source_content: cadmpeg_ir::features::FeatureContent::default(),

                evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                    IrFeatureDefinition::Operation(IrFeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                            sketch_id.copy_admitted(ctx, "creo sketch feature binding identity")?,
                        )),
                    }),
                ),
                native_ref: Some(sketch_native_ref(&sketch_id)),
            };
            source_carriers.admit_feature(ctx, ir, feature)?;
        }
    }
    Ok(coverage)
}

fn emitted_entity_views(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entities: &[SketchEntity],
) -> Result<(BTreeSet<SketchEntityId>, BTreeMap<SketchEntityId, SketchGeometry>), cadmpeg_core::CodecError> {
    let mut ids = BTreeSet::new();
    let mut geometry = BTreeMap::new();
    for entity in entities {
        ctx.charge_collection_items(1, "creo emitted sketch entity ID nodes")?;
        ids.insert(entity.id().copy_admitted(ctx, "creo emitted sketch entity IDs")?);
        ctx.charge_collection_items(1, "creo emitted sketch geometry nodes")?;
        geometry.insert(
            entity.id().copy_admitted(ctx, "creo emitted sketch geometry keys")?,
            entity.geometry.copy_admitted(ctx, "creo emitted sketch geometry")?,
        );
    }
    Ok((ids, geometry))
}

fn admit_constraint_row(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &mut Vec<SketchConstraint>,
    constraint: SketchConstraint,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_entities(1, "admit Creo model sketch_constraints")?;
    ctx.try_reserve_items(rows, 1, "creo sketch constraint rows")?;
    rows.push(constraint);
    Ok(())
}

fn available_parameter_ids<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    existing: impl IntoIterator<Item = &'a ParameterId>,
    planned: BTreeSet<ParameterId>,
) -> Result<BTreeSet<ParameterId>, cadmpeg_core::CodecError> {
    let mut ids = BTreeSet::new();
    for id in existing {
        if !ids.contains(id) {
            ctx.charge_collection_items(1, "creo available parameter ID nodes")?;
            ids.insert(id.copy_admitted(ctx, "creo available parameter identities")?);
        }
    }
    for id in planned {
        if !ids.contains(&id) {
            ctx.charge_collection_items(1, "creo available planned parameter ID nodes")?;
            ids.insert(id);
        }
    }
    Ok(ids)
}

fn insert_tree<K: Ord, V>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    map: &mut BTreeMap<K, V>,
    key: K,
    value: V,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    if !map.contains_key(&key) {
        ctx.charge_collection_items(1, operation)?;
    }
    map.insert(key, value);
    Ok(())
}

fn collect_numeric_set<T: Ord>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    values: impl IntoIterator<Item = T>,
    operation: &'static str,
) -> Result<BTreeSet<T>, cadmpeg_core::CodecError> {
    let mut result = BTreeSet::new();
    for value in values {
        insert_set(ctx, &mut result, value, operation)?;
    }
    Ok(result)
}

fn insert_set<T: Ord>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    set: &mut BTreeSet<T>,
    value: T,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    if !set.contains(&value) {
        ctx.charge_collection_items(1, operation)?;
        set.insert(value);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
