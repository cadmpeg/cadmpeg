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
    sketch_feature_id, sketch_native_ref,
};
use super::super::uniqueness::unique_feature_section_transform;
use super::entities::{transfer_section_entities, SectionEntityTransfer};
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
use cadmpeg_ir::sketches::SketchEntityId;
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinitionInput, SketchEntity, SketchGeometry,
    SketchId,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};
use std::collections::{BTreeMap, BTreeSet};

fn expected_segment_rows(
    definition_id: u32,
    table: &crate::feature::definitions::FeatureSegmentTable,
) -> Result<usize, cadmpeg_core::CodecError> {
    let declared_count = usize::try_from(table.declared_count).map_err(|_| cadmpeg_core::CodecError::malformed(format!("feature {definition_id} states segment table count {}, which exceeds the addressable row range {}",
        table.declared_count,
        usize::MAX)))?;
    let elided_prototype_rows = usize::from(table.has_elided_prototype);
    declared_count
        .checked_sub(elided_prototype_rows)
        .map_or_else(
            || {
                Err(cadmpeg_core::CodecError::malformed(format!(
                    "feature {definition_id} states segment table count {declared_count} and \
         {elided_prototype_rows} elided prototype row(s), so the ordinary row count is below zero"
                )))
            },
            Ok,
        )
}

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
        ir.model.parameters.iter().map(|parameter| &parameter.id),
        planned_feature_dimension_parameter_ids(scan),
    );
    for definition in &scan.features.definitions {
        if !feature_definition_has_sketch_design(definition) {
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
        let Some(sketch_id) = model_sketch_id(scan, definition) else {
            continue;
        };
        let segments = section_segment_rows(definition);
        let unique_segment_ids = unique_section_segment_external_ids(definition);
        let ambiguous_segment_ids = ambiguous_section_segment_external_ids(definition);
        let unique_saved_ids = unique_saved_section_internal_ids(definition);
        let complete_segment_table = definition
            .segments
            .as_ref()
            .is_some_and(crate::feature::definitions::FeatureSegmentTable::is_complete);
        if let Some(table) = &definition.segments {
            let decoded_rows = table.rows.len();
            let expected_rows = expected_segment_rows(definition.identity.id(), table)?;
            coverage.record_table_rows(decoded_rows, expected_rows)?;
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
                points.insert(*point, [*u, *v]);
            }
        }
        let radii = resolved_section_radii(ctx, definition)?;
        let missing_line_geometry = saved_section_missing_line_geometry(definition);
        let solved = collect_numeric_set(
            definition
                .trim_entities
                .iter()
                .flat_map(|table| &table.rows)
                .filter_map(|row| trim_segment_id(definition, row)),
        );
        let trim_vertex_coordinates = resolved_trim_vertex_coordinates(definition, &points, &radii);
        let mut resolved_segment_geometries = BTreeMap::new();
        for segment in &segments {
            resolved_segment_geometries.insert(
                segment.offset,
                resolved_section_segment_geometry_with_missing_line(
                    definition,
                    &points,
                    segment,
                    missing_line_geometry.as_ref(),
                ),
            );
        }
        let mut segment_geometries = BTreeMap::new();
        for segment in &segments {
            let geometry = if unique_segment_ids.contains(&segment.external_id)
                && solved.contains(&segment.external_id)
            {
                trimmed_section_segment_geometry_with_missing_line(
                    definition,
                    &points,
                    &radii,
                    &trim_vertex_coordinates,
                    segment,
                    missing_line_geometry.as_ref(),
                )
            } else {
                resolved_segment_geometries
                    .get(&segment.offset)
                    .and_then(Option::as_ref)
                    .cloned()
            };
            let geometry = match geometry {
                Some(geometry) => Some(geometry),
                None => section_axis_reference_line_geometry(definition, &variable_points, segment),
            };
            segment_geometries.insert(segment.offset, geometry);
        }
        let mut circle_geometries = BTreeMap::new();
        let mut point_geometries = BTreeMap::new();
        let mut centered_line_geometries = BTreeMap::new();
        let mut reference_line_geometries = BTreeMap::new();
        if let Some(table) = &definition.segments {
            for segment in table.rows.circles() {
                if let Some(geometry) = section_circle_geometry(&points, &radii, segment) {
                    circle_geometries.insert(segment.offset, geometry);
                }
            }
            for segment in table.rows.points() {
                if let Some(geometry) = section_point_row_geometry(&points, segment) {
                    point_geometries.insert(segment.offset, geometry);
                }
            }
            for segment in table.rows.centered_lines() {
                if let Some(geometry) = section_centered_line_geometry(&points, segment) {
                    centered_line_geometries.insert(segment.offset, geometry);
                }
            }
            for segment in table.rows.reference_lines() {
                if let Some(geometry) = resolved_section_reference_line_geometry(
                    definition,
                    &variable_points,
                    &points,
                    segment,
                ) {
                    reference_line_geometries.insert(segment.offset, geometry);
                }
            }
        }
        let emitted = collect_numeric_set(
            segments
                .iter()
                .filter(|segment| {
                    unique_segment_ids.contains(&segment.external_id)
                        && (section_degenerate_axis_line(definition, segment)
                            || segment_geometries
                                .get(&segment.offset)
                                .is_some_and(Option::is_some))
                })
                .map(|segment| segment.external_id)
                .chain(
                    definition
                        .segments
                        .iter()
                        .flat_map(|table| table.rows.circles())
                        .filter(|segment| {
                            unique_segment_ids.contains(&segment.external_id)
                                && circle_geometries.contains_key(&segment.offset)
                        })
                        .map(|segment| segment.external_id),
                ),
        );
        let resolved_segment_offsets = collect_numeric_set(
            segments
                .iter()
                .filter(|segment| {
                    segment_geometries
                        .get(&segment.offset)
                        .is_some_and(Option::is_some)
                })
                .map(|segment| segment.offset),
        );
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let materialized_saved_section_external_ids =
            materialized_saved_section_external_ids(ctx, definition, &mut refusal)?;
        for record in refusal.take_records() {
            let message = format!(
                "Feature {} states a saved section entity that materializes no sketch \
             geometry: {record}",
                definition.identity.id()
            );
            losses.push(crate::loss::CreoLossCode::SectionSplineUnresolved.note(message));
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
            let Some(geometry) = segment_geometries
                .get(&segment.offset)
                .and_then(Option::as_ref)
            else {
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
                generated_profile_geometries.push((segment.external_id, geometry.clone()));
            }
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| table.rows.circles())
        {
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
                generated_profile_geometries.push((segment.external_id, geometry.clone()));
            }
        }
        let mut profile_entities = BTreeSet::new();
        for entity_use in profiles.iter().flatten() {
            if !profile_entities.contains(&entity_use.entity) {
                profile_entities.insert(entity_use.entity.clone());
            }
        }
        for profile in saved_profile_chains(ctx, &sketch_id, &generated_profile_geometries)? {
            if profile
                .iter()
                .all(|entity_use| !profile_entities.contains(&entity_use.entity))
            {
                for entity_use in &profile {
                    if !profile_entities.contains(&entity_use.entity) {
                        profile_entities.insert(entity_use.entity.clone());
                    }
                }
                profiles.push(profile);
            }
        }
        let (mut entities, profiles) = transfer_section_entities(
            ctx,
            SectionEntityTransfer {
                scan,
                ir,
                annotations,
                definition,
                transform,
                sketch_id: &sketch_id,
                segments: &segments,
                unique_segment_ids: &unique_segment_ids,
                unique_saved_ids: &unique_saved_ids,
                ambiguous_segment_ids: &ambiguous_segment_ids,
                complete_segment_table,
                solved: &solved,
                segment_geometries: &segment_geometries,
                resolved_segment_geometries: &resolved_segment_geometries,
                circle_geometries: &circle_geometries,
                point_geometries: &point_geometries,
                centered_line_geometries: &centered_line_geometries,
                reference_line_geometries: &reference_line_geometries,
                materialized_saved_section_external_ids: &materialized_saved_section_external_ids,
                profiles,
                profile_entities: &profile_entities,
                losses,
                source_carriers,
            },
        )?;
        let profiles = cadmpeg_ir::sketches::SketchProfiles::try_from(profiles)
            .map_err(cadmpeg_core::CodecError::malformed)?;
        for (external_id, offset) in solver_only_section_entities(definition) {
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
                cadmpeg_core::decode::u64_from_index(offset),
                "solver_only_section_entity",
                Exactness::ByteExact,
            )?;
            ctx.charge_entities(1, "admit Creo model sketch_entities")?;
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
                    sketch_id.clone(),
                    SketchGeometry::native(
                        cadmpeg_core::text::NonBlankString::new(native_kind.to_owned())
                            .ok_or_else(|| {
                                cadmpeg_core::CodecError::malformed("native_kind must not be empty")
                            })?,
                    ),
                )
                .with_construction(true)
                .with_native_ref(Some(sketch_native_ref(&sketch_id))),
            );
        }
        let (emitted_entity_ids, emitted_entity_geometry) = emitted_entity_views(&entities);
        let mut constraints = Vec::new();
        for segment in &segments {
            if segment.vertical_horizontal.is_none() {
                continue;
            }
            let suffix = section_segment_identity_suffix(&unique_segment_ids, segment);
            let Some(entity) = sketch_entity_id(&sketch_id, &suffix) else {
                continue;
            };
            if let Some(definition) =
                section_segment_verhor_definition(segment, &sketch_id, entity)?
            {
                emit_verhor_constraint(
                    ctx,
                    (annotations, &mut constraints),
                    &emitted_entity_ids,
                    &sketch_id,
                    &suffix,
                    definition,
                    segment.offset,
                )?;
            }
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| table.rows.centered_lines())
        {
            let suffix = if unique_segment_ids.contains(&segment.external_id) {
                format!("{}", segment.external_id)
            } else {
                format!("centered_line:offset:{}", segment.offset)
            };
            let Some(entity) = sketch_entity_id(&sketch_id, &suffix) else {
                continue;
            };
            let definition = native_section_segment_verhor_definition(
                &sketch_id,
                entity,
                segment.external_id,
                0,
            )?;
            emit_verhor_constraint(
                ctx,
                (annotations, &mut constraints),
                &emitted_entity_ids,
                &sketch_id,
                &suffix,
                definition,
                segment.offset,
            )?;
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| table.rows.bounded_curves())
        {
            let Some(verhor) = segment.vertical_horizontal else {
                continue;
            };
            let suffix = if unique_segment_ids.contains(&segment.external_id) {
                format!("{}", segment.external_id)
            } else {
                format!("bounded_curve:offset:{}", segment.offset)
            };
            let Some(entity) = sketch_entity_id(&sketch_id, &suffix) else {
                continue;
            };
            let definition = native_section_segment_verhor_definition(
                &sketch_id,
                entity,
                segment.external_id,
                verhor,
            )?;
            emit_verhor_constraint(
                ctx,
                (annotations, &mut constraints),
                &emitted_entity_ids,
                &sketch_id,
                &suffix,
                definition,
                segment.offset,
            )?;
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| table.rows.reference_lines())
        {
            let Some(verhor) = segment.vertical_horizontal else {
                continue;
            };
            let suffix = if unique_segment_ids.contains(&segment.external_id) {
                format!("{}", segment.external_id)
            } else {
                format!("reference_line:offset:{}", segment.offset)
            };
            let Some(entity) = sketch_entity_id(&sketch_id, &suffix) else {
                continue;
            };
            let definition = native_section_segment_verhor_definition(
                &sketch_id,
                entity,
                segment.external_id,
                verhor,
            )?;
            emit_verhor_constraint(
                ctx,
                (annotations, &mut constraints),
                &emitted_entity_ids,
                &sketch_id,
                &suffix,
                definition,
                segment.offset,
            )?;
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| table.rows.opaque())
        {
            let Some(verhor) = segment.vertical_horizontal else {
                continue;
            };
            let suffix = opaque_section_segment_identity_suffix(&unique_segment_ids, segment);
            let Some(entity) = sketch_entity_id(&sketch_id, &suffix) else {
                continue;
            };
            let definition = native_section_segment_verhor_definition(
                &sketch_id,
                entity,
                segment.external_id,
                verhor,
            )?;
            emit_verhor_constraint(
                ctx,
                (annotations, &mut constraints),
                &emitted_entity_ids,
                &sketch_id,
                &suffix,
                definition,
                segment.offset,
            )?;
        }
        for (mut constraint, offset, relation_index) in
            section_dimension_constraints(ctx, definition, &sketch_id)?
        {
            let Some(relation) = definition
                .relations
                .as_ref()
                .and_then(|relations| relations.rows.get(relation_index))
            else {
                continue;
            };
            let reconciled = match constraint.definition.edit(|kind| {
                reconcile_section_dimension_constraint(
                    kind,
                    definition,
                    &sketch_id,
                    relation,
                    &emitted_entity_ids,
                    &available_parameter_ids,
                )
            }) {
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
                cadmpeg_core::decode::u64_from_index(offset),
                "section_dimension_constraint",
                Exactness::ByteExact,
            )?;
            admit_constraint_row(ctx, &mut constraints, constraint)?;
        }
        for (constraint, offset) in section_segment_radius_constraints_for_emitted(
            definition,
            &sketch_id,
            &emitted_entity_ids,
            &available_parameter_ids,
        )? {
            annotate(
                annotations,
                constraint.id.as_str(),
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(offset),
                "section_segment_radius_constraint",
                Exactness::ByteExact,
            )?;
            admit_constraint_row(ctx, &mut constraints, constraint)?;
        }
        let equation_constraints =
            section_equation_axis_distance_constraints(ctx, definition, &sketch_id)?
                .into_iter()
                .chain(section_equation_unsigned_distance_constraints(
                    definition, &sketch_id,
                )?)
                .chain(section_equation_point_on_line_constraints(
                    definition, &sketch_id,
                )?)
                .chain(section_equation_same_coordinate_constraints(
                    definition, &sketch_id,
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
                    definition, &sketch_id,
                )?)
                .chain(
                    section_equation_function_sixteen_angle_difference_constraints(
                        definition, &sketch_id,
                    )?,
                )
                .chain(section_equation_radius_dimension_constraints(
                    definition, &sketch_id,
                )?)
                .chain(section_equation_polar_distance_constraints(
                    ctx, definition, &sketch_id,
                )?)
                .chain(section_equation_function_six_distance_constraints(
                    ctx, definition, &sketch_id,
                )?)
                .chain(section_equation_equal_distance_constraints(
                    definition, &sketch_id,
                )?)
                .collect::<Vec<_>>();
        let equation_offsets =
            collect_numeric_set(equation_constraints.iter().map(|(_, offset)| *offset));
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
                rejected_equation_offsets.insert(offset);
                continue;
            }
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
                cadmpeg_core::decode::u64_from_index(
                    definition.body_position(offset)?.source()?.get(),
                ),
                "section_equation_constraint",
                Exactness::ByteExact,
            )?;
            admit_constraint_row(ctx, &mut constraints, constraint)?;
        }
        let typed_equation_offsets = collect_numeric_set(
            equation_offsets
                .into_iter()
                .filter(|offset| !rejected_equation_offsets.contains(offset)),
        );
        for (constraint, offset) in
            section_equation_native_constraints(definition, &sketch_id, &typed_equation_offsets)?
        {
            annotate(
                annotations,
                constraint.id.as_str(),
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(
                    definition.body_position(offset)?.source()?.get(),
                ),
                "section_native_equation_constraint",
                Exactness::ByteExact,
            )?;
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
                cadmpeg_core::decode::u64_from_index(offset),
                "section_solver_constraint",
                Exactness::ByteExact,
            )?;
            admit_constraint_row(ctx, &mut constraints, constraint)?;
        }
        source_carriers.admit_sketch_entities(ir, entities)?;
        source_carriers.admit_sketch_constraints(ir, constraints)?;
        let source_offset = transform.map_or(definition.offset, |transform| transform.offset);
        annotate(
            annotations,
            sketch_id.as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(source_offset),
            if transform.is_some() {
                "datum_placed_section"
            } else {
                "unplaced_section"
            },
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model sketches")?;
        source_carriers.admit_sketch(
            ir,
            Sketch {
                id: sketch_id.clone(),
                name: None,
                configuration: None,
                visible: None,
                placement,
                profiles,
                native_ref: Some(sketch_native_ref(&sketch_id)),
            },
        )?;
        if owned_section_feature_id(scan, definition.identity.id()).is_none() {
            let Some(feature_id) = sketch_feature_id(&sketch_id) else {
                continue;
            };
            annotate(
                annotations,
                feature_id.as_str(),
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(source_offset),
                "section_sketch_feature",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model features")?;
            let feature = Feature {
                id: feature_id,
                ordinal: cadmpeg_core::decode::u64_from_index(ir.model.features.len()),
                name: None,
                suppressed: Some(false),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: BTreeMap::new(),
                source_tag: Some("section".to_owned()),
                source_text: None,
                source_content: cadmpeg_ir::features::FeatureContent::default(),

                evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                    IrFeatureDefinition::Operation(IrFeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                            sketch_id.clone(),
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
    entities: &[SketchEntity],
) -> (
    std::collections::BTreeSet<cadmpeg_ir::SketchEntityId>,
    std::collections::BTreeMap<cadmpeg_ir::SketchEntityId, cadmpeg_ir::SketchGeometry>,
) {
    let mut ids = BTreeSet::new();
    let mut geometry = BTreeMap::new();
    for entity in entities {
        ids.insert(entity.id().clone());
        geometry.insert(entity.id().clone(), entity.geometry.clone());
    }
    (ids, geometry)
}

fn admit_constraint_row(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &mut Vec<SketchConstraint>,
    constraint: SketchConstraint,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_entities(1, "admit Creo model sketch_constraints")?;
    rows.push(constraint);
    Ok(())
}

fn emit_verhor_constraint(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    output: (&mut AnnotationBuilder, &mut Vec<SketchConstraint>),
    emitted_entity_ids: &BTreeSet<SketchEntityId>,
    sketch: &SketchId,
    suffix: &str,
    mut constraint_definition: SketchConstraintDefinitionInput,
    offset: usize,
) -> Result<(), cadmpeg_core::CodecError> {
    let (annotations, constraints) = output;

    if !reconcile_constraint_entity_references(&mut constraint_definition, emitted_entity_ids) {
        return Ok(());
    }
    let Some(id) = sketch_constraint_id(sketch, format_args!("verhor:{suffix}")) else {
        return Ok(());
    };
    let Ok(definition) =
        cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(constraint_definition)
    else {
        return Ok(());
    };
    annotate(
        annotations,
        id.as_str(),
        "FeatDefs",
        cadmpeg_core::decode::u64_from_index(offset),
        "section_verhor_constraint",
        Exactness::ByteExact,
    )?;
    let constraint = SketchConstraint {
        id,
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
    };
    admit_constraint_row(ctx, constraints, constraint)
}

fn available_parameter_ids<'a>(
    existing: impl IntoIterator<Item = &'a ParameterId>,
    planned: BTreeSet<ParameterId>,
) -> BTreeSet<ParameterId> {
    let mut ids = BTreeSet::new();
    for id in existing {
        if !ids.contains(id) {
            ids.insert(id.clone());
        }
    }
    for id in planned {
        ids.insert(id);
    }
    ids
}

fn collect_numeric_set<T: Ord>(values: impl IntoIterator<Item = T>) -> BTreeSet<T> {
    let mut result = BTreeSet::new();
    for value in values {
        result.insert(value);
    }
    result
}

#[cfg(test)]
mod tests;
