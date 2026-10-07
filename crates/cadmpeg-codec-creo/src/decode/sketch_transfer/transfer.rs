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
    resolved_section_radii, section_axis_reference_line_geometry, trim_segment_ids,
};
use super::super::sketch::skamp::section_segment_rows;
use super::super::sketch_ids::{
    feature_definition_has_sketch_design, model_sketch_id, sketch_constraint_id_admitted,
    sketch_entity_id_admitted, sketch_feature_id_admitted, sketch_native_ref_admitted,
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
    opaque_section_segment_identity_suffix_admitted, section_segment_identity_suffix_admitted,
    unique_saved_section_internal_ids, unique_section_segment_external_ids,
};
use crate::decode::sketch_transfer::loci::section_degenerate_axis_line;
use crate::decode::sketch_transfer::profiles::{
    resolved_profile_chains, solver_only_section_entities, solver_only_section_entity_family,
    SectionEntityIncidenceFamily,
};
use crate::decode::sketch_transfer::skamp_constraints::section_skamp_constraints_for_geometry;
use crate::feature::segment_rows::SegmentRow;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition_id: u32,
    table: &crate::feature::definitions::FeatureSegmentTable,
) -> Result<usize, cadmpeg_core::CodecError> {
    let declared_count = usize::try_from(table.declared_count).or_else(|_| {
        Err(cadmpeg_core::CodecError::Malformed(ctx.format_retained(
            format_args!(
                "feature {definition_id} states segment table count {}, which exceeds the addressable row range {}",
                table.declared_count,
                usize::MAX
            ),
            "creo segment table count error text",
        )?))
    })?;
    let elided_prototype_rows = usize::from(table.has_elided_prototype);
    declared_count.checked_sub(elided_prototype_rows).map_or_else(
        || Err(cadmpeg_core::CodecError::Malformed(ctx.format_retained(
            format_args!(
                "feature {definition_id} states segment table count {declared_count} and \
                 {elided_prototype_rows} elided prototype row(s), so the ordinary row count is below zero"
            ),
            "creo segment table underflow error text",
        )?)),
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
    let planned_parameter_ids = planned_feature_dimension_parameter_ids(ctx, scan)?;
    let existing_parameter_ids = ctx
        .admit_iter(&ir.model.parameters, "creo existing parameter rows")?
        .map(|parameter| &parameter.id);
    let available_parameter_ids =
        available_parameter_ids(ctx, existing_parameter_ids, planned_parameter_ids)?;
    for definition in ctx.admit_iter(
        &scan.features.definitions,
        "creo sketch feature definitions",
    )? {
        if !feature_definition_has_sketch_design(ctx, definition)? {
            continue;
        }
        let transform = definition
            .section_3d
            .as_ref()
            .map(|section| {
                unique_feature_section_transform(
                    ctx,
                    &scan.features.section_transforms,
                    definition.identity.id(),
                    section.offset,
                )
            })
            .transpose()?
            .flatten();
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
            let expected_rows = expected_segment_rows(ctx, definition.identity.id(), table)?;
            coverage.record_table_rows(decoded_rows, expected_rows)?;
            for segment in ctx
                .admit_iter(table.rows.as_slice(), "creo coverage ordinary segment rows")?
                .filter_map(|row| match row {
                    SegmentRow::Ordinary(segment) => Some(segment),
                    _ => None,
                })
            {
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
                (
                    SketchSegmentFamily::Circle,
                    ctx.admit_iter(table.rows.as_slice(), "creo coverage circle segment rows")?
                        .filter_map(|row| match row {
                            SegmentRow::Circle(segment) => Some(segment),
                            _ => None,
                        })
                        .count(),
                ),
                (
                    SketchSegmentFamily::Point,
                    ctx.admit_iter(table.rows.as_slice(), "creo coverage point segment rows")?
                        .filter_map(|row| match row {
                            SegmentRow::Point(segment) => Some(segment),
                            _ => None,
                        })
                        .count(),
                ),
                (
                    SketchSegmentFamily::CenteredLine,
                    ctx.admit_iter(
                        table.rows.as_slice(),
                        "creo coverage centered-line segment rows",
                    )?
                    .filter_map(|row| match row {
                        SegmentRow::CenteredLine(segment) => Some(segment),
                        _ => None,
                    })
                    .count(),
                ),
                (
                    SketchSegmentFamily::ReferenceLine,
                    ctx.admit_iter(
                        table.rows.as_slice(),
                        "creo coverage reference-line segment rows",
                    )?
                    .filter_map(|row| match row {
                        SegmentRow::ReferenceLine(segment) => Some(segment),
                        _ => None,
                    })
                    .count(),
                ),
                (
                    SketchSegmentFamily::BoundedCurve,
                    ctx.admit_iter(
                        table.rows.as_slice(),
                        "creo coverage bounded-curve segment rows",
                    )?
                    .filter_map(|row| match row {
                        SegmentRow::BoundedCurve(segment) => Some(segment),
                        _ => None,
                    })
                    .count(),
                ),
                (
                    SketchSegmentFamily::Conic,
                    ctx.admit_iter(table.rows.as_slice(), "creo coverage conic segment rows")?
                        .filter_map(|row| match row {
                            SegmentRow::Conic(segment) => Some(segment),
                            _ => None,
                        })
                        .count(),
                ),
                (
                    SketchSegmentFamily::Opaque,
                    ctx.admit_iter(table.rows.as_slice(), "creo coverage opaque segment rows")?
                        .filter_map(|row| match row {
                            SegmentRow::Opaque(segment) => Some(segment),
                            _ => None,
                        })
                        .count(),
                ),
            ] {
                coverage.record_family_rows(family, count);
            }
        }
        let variable_points = resolved_section_coordinates(ctx, definition)?;
        let mut points = BTreeMap::new();
        for (point, [u, v]) in ctx.admit_iter(&variable_points, "creo resolved sketch points")? {
            if let (Some(u), Some(v)) = (u, v) {
                ctx.insert_btree_map(
                    &mut points,
                    *point,
                    [*u, *v],
                    "creo resolved sketch point nodes",
                )?;
            }
        }
        let radii = resolved_section_radii(ctx, definition)?;
        let missing_line_geometry = saved_section_missing_line_geometry(ctx, definition)?;
        let mut solved = BTreeSet::new();
        for id in ctx
            .admit_iter(
                trim_segment_ids(ctx, definition)?,
                "creo solved section trim rows",
            )?
            .flatten()
        {
            ctx.insert_btree_set(&mut solved, id, "creo solved section segment ID nodes")?;
        }
        let trim_vertex_coordinates =
            resolved_trim_vertex_coordinates(ctx, definition, &points, &radii)?;
        let mut resolved_segment_geometries = BTreeMap::new();
        for segment in ctx.admit_iter(&segments, "creo resolved segment rows")? {
            ctx.insert_btree_map(
                &mut resolved_segment_geometries,
                segment.offset,
                resolved_section_segment_geometry_with_missing_line(
                    ctx,
                    definition,
                    &points,
                    segment,
                    missing_line_geometry.as_ref(),
                )?,
                "creo resolved section geometry nodes",
            )?;
        }
        let mut segment_geometries = BTreeMap::new();
        for segment in ctx.admit_iter(&segments, "creo transferred segment rows")? {
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
                    .map(|geometry| {
                        geometry.try_clone_for_decode(ctx, "creo resolved section geometry copy")
                    })
                    .transpose()?
            };
            let geometry = match geometry {
                Some(geometry) => Some(geometry),
                None => section_axis_reference_line_geometry(
                    ctx,
                    definition,
                    &variable_points,
                    segment,
                )?,
            };
            ctx.insert_btree_map(
                &mut segment_geometries,
                segment.offset,
                geometry,
                "creo section geometry nodes",
            )?;
        }
        let mut circle_geometries = BTreeMap::new();
        let mut point_geometries = BTreeMap::new();
        let mut centered_line_geometries = BTreeMap::new();
        let mut reference_line_geometries = BTreeMap::new();
        if let Some(table) = &definition.segments {
            for row in
                ctx.admit_iter(table.rows.as_slice(), "creo transfer segment geometry rows")?
            {
                let (geometry, offset, geometries, operation) = match row {
                    SegmentRow::Circle(segment) => (
                        section_circle_geometry(ctx, &points, &radii, segment)?,
                        segment.offset,
                        &mut circle_geometries,
                        "creo section circle geometry nodes",
                    ),
                    SegmentRow::Point(segment) => (
                        section_point_row_geometry(ctx, &points, segment)?,
                        segment.offset,
                        &mut point_geometries,
                        "creo section point geometry nodes",
                    ),
                    SegmentRow::CenteredLine(segment) => (
                        section_centered_line_geometry(ctx, &points, segment)?,
                        segment.offset,
                        &mut centered_line_geometries,
                        "creo section centered-line geometry nodes",
                    ),
                    SegmentRow::ReferenceLine(segment) => (
                        resolved_section_reference_line_geometry(
                            ctx,
                            definition,
                            &variable_points,
                            &points,
                            segment,
                        )?,
                        segment.offset,
                        &mut reference_line_geometries,
                        "creo section reference-line geometry nodes",
                    ),
                    _ => continue,
                };
                if let Some(geometry) = geometry {
                    ctx.insert_btree_map(geometries, offset, geometry, operation)?;
                }
            }
        }
        let mut emitted = BTreeSet::new();
        for segment in ctx.admit_iter(&segments, "creo emitted ordinary segment rows")? {
            if unique_segment_ids.contains(&segment.external_id)
                && (section_degenerate_axis_line(ctx, definition, segment)?
                    || segment_geometries
                        .get(&segment.offset)
                        .is_some_and(Option::is_some))
            {
                ctx.insert_btree_set(
                    &mut emitted,
                    segment.external_id,
                    "creo emitted section segment ID nodes",
                )?;
            }
        }
        if let Some(table) = definition.segments.as_ref() {
            for segment in ctx
                .admit_iter(table.rows.as_slice(), "creo emitted circle segment rows")?
                .filter_map(|row| match row {
                    SegmentRow::Circle(segment) => Some(segment),
                    _ => None,
                })
            {
                if unique_segment_ids.contains(&segment.external_id)
                    && circle_geometries.contains_key(&segment.offset)
                {
                    ctx.insert_btree_set(
                        &mut emitted,
                        segment.external_id,
                        "creo emitted section segment ID nodes",
                    )?;
                }
            }
        }
        let mut resolved_segment_offsets = BTreeSet::new();
        for segment in ctx.admit_iter(&segments, "creo resolved ordinary segment rows")? {
            if segment_geometries
                .get(&segment.offset)
                .is_some_and(Option::is_some)
            {
                ctx.insert_btree_set(
                    &mut resolved_segment_offsets,
                    segment.offset,
                    "creo resolved section offset nodes",
                )?;
            }
        }
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let materialized_saved_section_external_ids =
            materialized_saved_section_external_ids(ctx, definition, &mut refusal)?;
        let refused_records = refusal.take_records_checked()?;
        for record in ctx.admit_iter(&refused_records, "creo refused saved spline records")? {
            let message = ctx.format_retained(
                format_args!(
                    "Feature {} states a saved section entity that materializes no sketch \
                     geometry: {record}",
                    definition.identity.id()
                ),
                "creo unresolved saved section loss text",
            )?;
            ctx.reserve_vec(losses, 1, "creo unresolved saved section losses")?;
            losses.push(crate::loss::CreoLossCode::SectionSplineUnresolved.note(message));
        }
        coverage.record_resolved_geometry(resolved_segment_offsets.len());
        for segment in ctx
            .admit_iter(&segments, "creo resolved sketch segments")?
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
        let resolved_circles = if let Some(table) = definition.segments.as_ref() {
            ctx.admit_iter(table.rows.as_slice(), "creo resolved circle segment rows")?
                .filter_map(|row| match row {
                    SegmentRow::Circle(segment) => Some(segment),
                    _ => None,
                })
                .filter(|segment| {
                    circle_geometries.contains_key(&segment.offset)
                        || (unique_segment_ids.contains(&segment.external_id)
                            && materialized_saved_section_external_ids
                                .contains(&segment.external_id))
                })
                .count()
        } else {
            0
        };
        coverage.record_resolved_geometry(resolved_circles);
        coverage.record_family_resolution(SketchSegmentFamily::Circle, resolved_circles);
        let resolved_points = if let Some(table) = definition.segments.as_ref() {
            ctx.admit_iter(table.rows.as_slice(), "creo resolved point segment rows")?
                .filter_map(|row| match row {
                    SegmentRow::Point(segment) => Some(segment),
                    _ => None,
                })
                .filter(|segment| {
                    point_geometries.contains_key(&segment.offset)
                        || (unique_segment_ids.contains(&segment.external_id)
                            && materialized_saved_section_external_ids
                                .contains(&segment.external_id))
                })
                .count()
        } else {
            0
        };
        coverage.record_resolved_geometry(resolved_points);
        coverage.record_family_resolution(SketchSegmentFamily::Point, resolved_points);
        let resolved_centered_lines = if let Some(table) = definition.segments.as_ref() {
            ctx.admit_iter(
                table.rows.as_slice(),
                "creo resolved centered-line segment rows",
            )?
            .filter_map(|row| match row {
                SegmentRow::CenteredLine(segment) => Some(segment),
                _ => None,
            })
            .filter(|segment| {
                centered_line_geometries.contains_key(&segment.offset)
                    || (unique_segment_ids.contains(&segment.external_id)
                        && materialized_saved_section_external_ids.contains(&segment.external_id))
            })
            .count()
        } else {
            0
        };
        coverage.record_resolved_geometry(resolved_centered_lines);
        coverage
            .record_family_resolution(SketchSegmentFamily::CenteredLine, resolved_centered_lines);
        let resolved_reference_lines = if let Some(table) = definition.segments.as_ref() {
            ctx.admit_iter(
                table.rows.as_slice(),
                "creo resolved reference-line segment rows",
            )?
            .filter_map(|row| match row {
                SegmentRow::ReferenceLine(segment) => Some(segment),
                _ => None,
            })
            .filter(|segment| reference_line_geometries.contains_key(&segment.offset))
            .count()
        } else {
            0
        };
        coverage.record_resolved_geometry(resolved_reference_lines);
        coverage
            .record_family_resolution(SketchSegmentFamily::ReferenceLine, resolved_reference_lines);
        let resolved_bounded_curves = if let Some(table) = definition.segments.as_ref() {
            ctx.admit_iter(
                table.rows.as_slice(),
                "creo resolved bounded-curve segment rows",
            )?
            .filter_map(|row| match row {
                SegmentRow::BoundedCurve(segment) => Some(segment),
                _ => None,
            })
            .filter(|segment| {
                unique_segment_ids.contains(&segment.external_id)
                    && materialized_saved_section_external_ids.contains(&segment.external_id)
            })
            .count()
        } else {
            0
        };
        coverage.record_resolved_geometry(resolved_bounded_curves);
        coverage
            .record_family_resolution(SketchSegmentFamily::BoundedCurve, resolved_bounded_curves);
        let resolved_conics = if let Some(table) = definition.segments.as_ref() {
            ctx.admit_iter(table.rows.as_slice(), "creo resolved conic segment rows")?
                .filter_map(|row| match row {
                    SegmentRow::Conic(segment) => Some(segment),
                    _ => None,
                })
                .filter(|segment| {
                    unique_segment_ids.contains(&segment.external_id)
                        && materialized_saved_section_external_ids.contains(&segment.external_id)
                })
                .count()
        } else {
            0
        };
        coverage.record_resolved_geometry(resolved_conics);
        coverage.record_family_resolution(SketchSegmentFamily::Conic, resolved_conics);
        let resolved_opaque = if let Some(table) = definition.segments.as_ref() {
            ctx.admit_iter(table.rows.as_slice(), "creo resolved opaque segment rows")?
                .filter_map(|row| match row {
                    SegmentRow::Opaque(segment) => Some(segment),
                    _ => None,
                })
                .filter(|segment| {
                    unique_segment_ids.contains(&segment.external_id)
                        && materialized_saved_section_external_ids.contains(&segment.external_id)
                })
                .count()
        } else {
            0
        };
        coverage.record_resolved_geometry(resolved_opaque);
        coverage.record_family_resolution(SketchSegmentFamily::Opaque, resolved_opaque);
        let mut profiles = resolved_profile_chains(ctx, definition, &sketch_id, &emitted)?;
        let mut generated_profile_geometries = Vec::new();
        for segment in ctx.admit_iter(&segments, "creo generated profile segments")? {
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
                ctx,
                complete_segment_table,
                definition.identity.owner_feature_id(),
                segment.external_id,
                expected_kinds,
                &scan.features.entity_tables,
                &scan.surfaces.rows,
            )? {
                ctx.reserve_vec(
                    &mut generated_profile_geometries,
                    1,
                    "creo generated profile geometry rows",
                )?;
                generated_profile_geometries.push((
                    segment.external_id,
                    geometry.try_clone_for_decode(ctx, "creo generated profile geometry copy")?,
                ));
            }
        }
        if let Some(table) = definition.segments.as_ref() {
            for segment in ctx
                .admit_iter(table.rows.as_slice(), "creo generated profile circle rows")?
                .filter_map(|row| match row {
                    SegmentRow::Circle(segment) => Some(segment),
                    _ => None,
                })
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
                    ctx,
                    complete_segment_table,
                    definition.identity.owner_feature_id(),
                    segment.external_id,
                    expected_kinds,
                    &scan.features.entity_tables,
                    &scan.surfaces.rows,
                )? {
                    ctx.reserve_vec(
                        &mut generated_profile_geometries,
                        1,
                        "creo generated profile geometry rows",
                    )?;
                    generated_profile_geometries.push((
                        segment.external_id,
                        geometry
                            .try_clone_for_decode(ctx, "creo generated profile geometry copy")?,
                    ));
                }
            }
        }
        let mut profile_storage = ctx.reserve_scoped(0, "Creo sketch profile membership")?;
        let mut profile_entities = BTreeSet::new();
        for profile in ctx.admit_iter(&profiles, "creo sketch profiles")? {
            for entity_use in ctx.admit_iter(profile, "creo sketch profile entities")? {
                if !ctx.contains_btree_set(
                    &profile_entities,
                    &entity_use.entity,
                    "creo sketch profile entity membership",
                )? {
                    profile_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut profile_entities,
                            entity_use
                                .entity
                                .try_clone_for_decode(ctx, "creo profile entity identities")?,
                            "creo profile entity ID nodes",
                        )
                    })?;
                }
            }
        }
        for profile in saved_profile_chains(ctx, &sketch_id, &generated_profile_geometries)? {
            let mut profile_is_new = true;
            for entity_use in ctx.admit_iter(&profile, "creo saved profile entities")? {
                if ctx.contains_btree_set(
                    &profile_entities,
                    &entity_use.entity,
                    "creo sketch profile entity membership",
                )? {
                    profile_is_new = false;
                    break;
                }
            }
            if profile_is_new {
                for entity_use in ctx.admit_iter(&profile, "creo saved profile entities")? {
                    if !ctx.contains_btree_set(
                        &profile_entities,
                        &entity_use.entity,
                        "creo sketch profile entity membership",
                    )? {
                        profile_storage.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut profile_entities,
                                entity_use
                                    .entity
                                    .try_clone_for_decode(ctx, "creo profile entity identities")?,
                                "creo profile entity ID nodes",
                            )
                        })?;
                    }
                }
                ctx.reserve_vec(&mut profiles, 1, "creo merged sketch profile rows")?;
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
        let solver_only_entities = solver_only_section_entities(ctx, definition)?;
        for (external_id, offset) in
            ctx.admit_iter(&solver_only_entities, "creo solver-only section entities")?
        {
            let external_id = *external_id;
            let offset = *offset;
            let Some(id) = sketch_entity_id_admitted(ctx, &sketch_id, external_id)? else {
                continue;
            };
            let mut already_present = false;
            for entity in ctx.admit_iter(&entities, "creo transferred sketch entities")? {
                if ctx.equal(
                    entity.id(),
                    &id,
                    "creo transferred sketch entity ID comparison",
                )? {
                    already_present = true;
                    break;
                }
            }
            if already_present {
                continue;
            }
            annotate(
                ctx,
                annotations,
                id.as_str(),
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(offset),
                "solver_only_section_entity",
                Exactness::ByteExact,
            )?;
            ctx.charge_entities(1, "admit Creo model sketch_entities")?;
            ctx.reserve_vec(&mut entities, 1, "creo solver-only sketch entities")?;
            let native_kind = match solver_only_section_entity_family(ctx, definition, external_id)?
            {
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
                    sketch_id
                        .try_clone_for_decode(ctx, "creo solver-only entity sketch identity")?,
                    SketchGeometry::native(
                        cadmpeg_core::text::NonBlankString::for_decode(
                            ctx,
                            ctx.copy_retained_text(native_kind, "creo solver-only native kind")?,
                            "validate nonblank text",
                        )?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed("native_kind must not be empty")
                        })?,
                    ),
                )
                .with_construction(true)
                .with_native_ref(Some(sketch_native_ref_admitted(ctx, &sketch_id)?)),
            );
        }
        let mut emitted_storage = ctx.reserve_scoped(0, "Creo emitted sketch lookup")?;
        let (emitted_entity_ids, emitted_entity_geometry) =
            emitted_storage.with_storage(|| emitted_entity_views(ctx, &entities))?;
        let mut constraints = Vec::new();
        for segment in ctx.admit_iter(&segments, "creo segment orientation rows")? {
            if segment.vertical_horizontal.is_none() {
                continue;
            }
            let suffix =
                section_segment_identity_suffix_admitted(ctx, &unique_segment_ids, segment)?;
            let Some(entity) = sketch_entity_id_admitted(ctx, &sketch_id, &suffix)? else {
                continue;
            };
            if let Some(definition) =
                section_segment_verhor_definition(ctx, segment, &sketch_id, entity)?
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
        if let Some(table) = definition.segments.as_ref() {
            for segment in ctx
                .admit_iter(
                    table.rows.as_slice(),
                    "creo transfer centered_lines segment rows",
                )?
                .filter_map(|row| match row {
                    SegmentRow::CenteredLine(segment) => Some(segment),
                    _ => None,
                })
            {
                let suffix = if unique_segment_ids.contains(&segment.external_id) {
                    ctx.format_retained(
                        format_args!("{}", segment.external_id),
                        "creo verhor entity suffix",
                    )?
                } else {
                    ctx.format_retained(
                        format_args!("centered_line:offset:{}", segment.offset),
                        "creo verhor entity suffix",
                    )?
                };
                let Some(entity) = sketch_entity_id_admitted(ctx, &sketch_id, &suffix)? else {
                    continue;
                };
                let definition = native_section_segment_verhor_definition(
                    ctx,
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
        }
        if let Some(table) = definition.segments.as_ref() {
            for segment in ctx
                .admit_iter(
                    table.rows.as_slice(),
                    "creo transfer bounded_curves segment rows",
                )?
                .filter_map(|row| match row {
                    SegmentRow::BoundedCurve(segment) => Some(segment),
                    _ => None,
                })
            {
                let Some(verhor) = segment.vertical_horizontal else {
                    continue;
                };
                let suffix = if unique_segment_ids.contains(&segment.external_id) {
                    ctx.format_retained(
                        format_args!("{}", segment.external_id),
                        "creo verhor entity suffix",
                    )?
                } else {
                    ctx.format_retained(
                        format_args!("bounded_curve:offset:{}", segment.offset),
                        "creo verhor entity suffix",
                    )?
                };
                let Some(entity) = sketch_entity_id_admitted(ctx, &sketch_id, &suffix)? else {
                    continue;
                };
                let definition = native_section_segment_verhor_definition(
                    ctx,
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
        }
        if let Some(table) = definition.segments.as_ref() {
            for segment in ctx
                .admit_iter(
                    table.rows.as_slice(),
                    "creo transfer reference_lines segment rows",
                )?
                .filter_map(|row| match row {
                    SegmentRow::ReferenceLine(segment) => Some(segment),
                    _ => None,
                })
            {
                let Some(verhor) = segment.vertical_horizontal else {
                    continue;
                };
                let suffix = if unique_segment_ids.contains(&segment.external_id) {
                    ctx.format_retained(
                        format_args!("{}", segment.external_id),
                        "creo verhor entity suffix",
                    )?
                } else {
                    ctx.format_retained(
                        format_args!("reference_line:offset:{}", segment.offset),
                        "creo verhor entity suffix",
                    )?
                };
                let Some(entity) = sketch_entity_id_admitted(ctx, &sketch_id, &suffix)? else {
                    continue;
                };
                let definition = native_section_segment_verhor_definition(
                    ctx,
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
        }
        if let Some(table) = definition.segments.as_ref() {
            for segment in ctx
                .admit_iter(table.rows.as_slice(), "creo transfer opaque segment rows")?
                .filter_map(|row| match row {
                    SegmentRow::Opaque(segment) => Some(segment),
                    _ => None,
                })
            {
                let Some(verhor) = segment.vertical_horizontal else {
                    continue;
                };
                let suffix = opaque_section_segment_identity_suffix_admitted(
                    ctx,
                    &unique_segment_ids,
                    segment,
                )?;
                let Some(entity) = sketch_entity_id_admitted(ctx, &sketch_id, &suffix)? else {
                    continue;
                };
                let definition = native_section_segment_verhor_definition(
                    ctx,
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
                    ctx,
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
                ctx,
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
            ctx,
            definition,
            &sketch_id,
            &emitted_entity_ids,
            &available_parameter_ids,
        )? {
            annotate(
                ctx,
                annotations,
                constraint.id.as_str(),
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(offset),
                "section_segment_radius_constraint",
                Exactness::ByteExact,
            )?;
            admit_constraint_row(ctx, &mut constraints, constraint)?;
        }
        let equation_constraints = ctx.collect_vec(
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
                )?),
            "creo sketch equation constraints",
        )?;
        let equation_offsets = collect_numeric_set(
            ctx,
            ctx.admit_iter(&equation_constraints, "creo equation offset source")?
                .map(|(_, offset)| *offset),
            "creo equation offset nodes",
        )?;
        let mut rejected_equation_offsets = BTreeSet::new();
        let mut reconciled_equation_constraints = Vec::new();
        for (mut constraint, offset) in equation_constraints {
            let entity_reconciled = match constraint
                .definition
                .edit(|kind| reconcile_constraint_entity_references(ctx, kind, &emitted_entity_ids))
            {
                Ok(result) => result?,
                Err(_) => false,
            };
            let parameter_reconciled = constraint.definition.edit(|kind| {
                reconcile_constraint_parameter_reference(ctx, kind, &available_parameter_ids)
            });
            let parameter_reconciled = match parameter_reconciled {
                Ok(result) => result?,
                Err(_) => false,
            };
            if !entity_reconciled || !parameter_reconciled {
                ctx.insert_btree_set(
                    &mut rejected_equation_offsets,
                    offset,
                    "creo rejected equation offset nodes",
                )?;
                continue;
            }
            ctx.reserve_vec(
                &mut reconciled_equation_constraints,
                1,
                "creo reconciled equation rows",
            )?;
            reconciled_equation_constraints.push((constraint, offset));
        }
        for (constraint, offset) in reconciled_equation_constraints {
            if rejected_equation_offsets.contains(&offset) {
                continue;
            }
            annotate(
                ctx,
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
            ctx,
            ctx.admit_iter(&equation_offsets, "creo typed equation offset source")?
                .copied()
                .filter(|offset| !rejected_equation_offsets.contains(offset)),
            "creo typed equation offset nodes",
        )?;
        drop(equation_offsets);
        for (constraint, offset) in section_equation_native_constraints(
            ctx,
            definition,
            &sketch_id,
            &typed_equation_offsets,
        )? {
            annotate(
                ctx,
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
            let entity_reconciled = match constraint
                .definition
                .edit(|kind| reconcile_constraint_entity_references(ctx, kind, &emitted_entity_ids))
            {
                Ok(result) => result?,
                Err(_) => false,
            };
            if !entity_reconciled {
                continue;
            }
            annotate(
                ctx,
                annotations,
                constraint.id.as_str(),
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(offset),
                "section_solver_constraint",
                Exactness::ByteExact,
            )?;
            admit_constraint_row(ctx, &mut constraints, constraint)?;
        }
        source_carriers.admit_sketch_entities(ctx, ir, entities)?;
        source_carriers.admit_sketch_constraints(ctx, ir, constraints)?;
        let source_offset = transform.map_or(definition.offset, |transform| transform.offset);
        annotate(
            ctx,
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
            ctx,
            ir,
            Sketch {
                id: sketch_id.try_clone_for_decode(ctx, "creo model sketch identity copy")?,
                name: None,
                configuration: None,
                visible: None,
                placement,
                profiles,
                native_ref: Some(sketch_native_ref_admitted(ctx, &sketch_id)?),
            },
        )?;
        if owned_section_feature_id(scan, definition.identity.id()).is_none() {
            let Some(feature_id) = sketch_feature_id_admitted(ctx, &sketch_id)? else {
                continue;
            };
            annotate(
                ctx,
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
                source_tag: Some(
                    ctx.copy_retained_text("section", "creo sketch feature source tag")?,
                ),
                source_text: None,
                source_content: cadmpeg_ir::features::FeatureContent::default(),

                evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                    IrFeatureDefinition::Operation(IrFeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                            sketch_id.try_clone_for_decode(
                                ctx,
                                "creo sketch feature binding identity",
                            )?,
                        )),
                    }),
                ),
                native_ref: Some(sketch_native_ref_admitted(ctx, &sketch_id)?),
            };
            source_carriers.admit_feature(ctx, ir, feature)?;
        }
    }
    Ok(coverage)
}

fn emitted_entity_views(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entities: &[SketchEntity],
) -> Result<
    (
        BTreeSet<SketchEntityId>,
        BTreeMap<SketchEntityId, SketchGeometry>,
    ),
    cadmpeg_core::CodecError,
> {
    let mut ids = BTreeSet::new();
    let mut geometry = BTreeMap::new();
    for entity in ctx.admit_iter(entities, "creo emitted entity views")? {
        ctx.insert_btree_set(
            &mut ids,
            entity
                .id()
                .try_clone_for_decode(ctx, "creo emitted sketch entity IDs")?,
            "creo emitted sketch entity ID nodes",
        )?;
        ctx.insert_btree_map(
            &mut geometry,
            entity
                .id()
                .try_clone_for_decode(ctx, "creo emitted sketch geometry keys")?,
            entity
                .geometry
                .try_clone_for_decode(ctx, "creo emitted sketch geometry")?,
            "creo emitted sketch geometry nodes",
        )?;
    }
    Ok((ids, geometry))
}

fn admit_constraint_row(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &mut Vec<SketchConstraint>,
    constraint: SketchConstraint,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_entities(1, "admit Creo model sketch_constraints")?;
    ctx.reserve_vec(rows, 1, "creo sketch constraint rows")?;
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

    if !reconcile_constraint_entity_references(ctx, &mut constraint_definition, emitted_entity_ids)?
    {
        return Ok(());
    }
    let Some(id) = sketch_constraint_id_admitted(ctx, sketch, format_args!("verhor:{suffix}"))?
    else {
        return Ok(());
    };
    let Ok(definition) =
        cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(constraint_definition)
    else {
        return Ok(());
    };
    annotate(
        ctx,
        annotations,
        id.as_str(),
        "FeatDefs",
        cadmpeg_core::decode::u64_from_index(offset),
        "section_verhor_constraint",
        Exactness::ByteExact,
    )?;
    let constraint = SketchConstraint {
        id,
        sketch: sketch.try_clone_for_decode(ctx, "creo constraint sketch identity")?,
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
        native_ref: Some(sketch_native_ref_admitted(ctx, sketch)?),
    };
    admit_constraint_row(ctx, constraints, constraint)
}

fn available_parameter_ids<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    existing: impl IntoIterator<Item = &'a ParameterId>,
    planned: BTreeSet<ParameterId>,
) -> Result<BTreeSet<ParameterId>, cadmpeg_core::CodecError> {
    let mut ids = BTreeSet::new();
    for id in existing {
        if !ctx.contains_btree_set(&ids, id, "creo available parameter ID membership")? {
            ctx.insert_btree_set(
                &mut ids,
                id.try_clone_for_decode(ctx, "creo available parameter identities")?,
                "creo available parameter ID nodes",
            )?;
        }
    }
    for id in planned {
        ctx.insert_btree_set(&mut ids, id, "creo available planned parameter ID nodes")?;
    }
    Ok(ids)
}

fn collect_numeric_set<T: Ord + cadmpeg_core::decode::cost::DecodeCost>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    values: impl IntoIterator<Item = T>,
    operation: &'static str,
) -> Result<BTreeSet<T>, cadmpeg_core::CodecError> {
    let mut result = BTreeSet::new();
    for value in values {
        ctx.insert_btree_set(&mut result, value, operation)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
