// SPDX-License-Identifier: Apache-2.0
//! Source metadata and structural decode-coverage census.

use std::collections::BTreeMap;

use crate::container::ContainerScan;
use crate::feature::definitions::{ScalarLane, VariableType};

use super::super::expanded::feature_surface_replay_associations;
use super::super::sketch::coordinates::resolved_section_coordinates;
use super::super::sketch::equations_scalar::resolved_section_scalar_values;
use super::super::sketch::radii::resolved_section_radii;
use super::coverage::{legacy_numeric_coverage, torus_parameter_coverage, LegacyNumericCoverage};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_ir::document::SourceMeta;

fn insert_source_attribute(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    attributes: &mut BTreeMap<String, String>,
    key: impl std::fmt::Display,
    value: impl std::fmt::Display,
) -> Result<(), cadmpeg_core::CodecError> {
    let key = ctx.format_retained(key, "Creo source attribute key")?;
    let value = ctx.format_retained(value, "Creo source attribute value")?;
    if !attributes.contains_key(&key) {
        ctx.charge_collection_items(1, "Creo source attribute map nodes")?;
    }
    attributes.insert(key, value);
    Ok(())
}

pub(super) fn source_meta(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    classification: &crate::dialect::DialectClassification,
) -> Result<(SourceMeta, cadmpeg_ir::report::decode::Coverage), cadmpeg_core::CodecError> {
    let mut attributes = BTreeMap::new();
    let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
    insert_source_attribute(ctx, &mut attributes, "version_line", &scan.framing.version_line)?;
    if let Some(name) = &scan.framing.model_name {
        insert_source_attribute(ctx, &mut attributes, "model_name", &name.name)?;
    }
    if let Some(legacy) = scan.framing.layout.legacy_ascii() {
        insert_source_attribute(ctx, &mut attributes, "legacy_ascii_schema", &legacy.schema)?;
        if let Some(release) = &legacy.product_release {
            insert_source_attribute(ctx, &mut attributes, "legacy_ascii_product_release", release)?;
        }
        insert_source_attribute(ctx, &mut attributes, "legacy_ascii_declaration_count", legacy.persistence.declaration_count())?;
        insert_source_attribute(ctx, &mut attributes, "legacy_ascii_scope_count", legacy.persistence.scopes.len())?;
        insert_source_attribute(ctx, &mut attributes, "legacy_ascii_value_count", legacy.persistence.value_count())?;
        insert_source_attribute(ctx, &mut attributes, "legacy_ascii_continuation_count", legacy.persistence.continuation_count())?;
        insert_source_attribute(ctx, &mut attributes, "legacy_ascii_unresolved_value_count", legacy.persistence.unresolved_value_count())?;
        insert_source_attribute(ctx, &mut attributes, "legacy_ascii_conflicting_declaration_count", legacy.persistence.conflicting_declaration_count())?;
    }
    insert_source_attribute(ctx, &mut attributes, "file_size", scan.framing.data.len())?;
    insert_source_attribute(ctx, &mut attributes, "section_count", scan.framing.sections.len())?;
    for (index, section) in scan.framing.sections.iter().enumerate() {
        insert_source_attribute(ctx, &mut attributes, format_args!("section.{index}.name"), section.name())?;
        insert_source_attribute(ctx, &mut attributes, format_args!("section.{index}.raw_name"), &section.raw_name)?;
        insert_source_attribute(ctx, &mut attributes, format_args!("section.{index}.role"), cadmpeg_core::container::ContainerRole::from(section.role()))?;
        insert_source_attribute(ctx, &mut attributes, format_args!("section.{index}.offset"), section.offset())?;
        insert_source_attribute(ctx, &mut attributes, format_args!("section.{index}.length"), section.length())?;
    }
    if let Some(c) = scan.framing.census.srf_array_count {
        insert_source_attribute(ctx, &mut attributes, "srf_array_count", c)?;
    }
    if let Some(c) = scan.framing.census.crv_array_count {
        insert_source_attribute(ctx, &mut attributes, "crv_array_count", c)?;
    }
    if let Some(unit) = &scan.framing.principal_unit {
        insert_source_attribute(ctx, &mut attributes, "principal_unit", unit)?;
        if let Some(scale) = unit.length_scale_mm().filter(|scale| scale.get() != 1.0) {
            insert_source_attribute(ctx, &mut attributes, "source_length_scale_mm", scale.get())?;
        }
    }
    if let Some(legacy) = scan.framing.layout.legacy_ascii() {
        coverage.record(
            crate::coverage::DECODED_LEGACY_PRINCIPAL_UNIT_COUNT,
            usize::from(scan.framing.principal_unit.is_some()),
        );
        let mut object_arrows = 0usize;
        let mut object_inlines = 0usize;
        let mut object_nulls = 0usize;
        let mut object_arrays = 0usize;
        for record in &legacy.persistence.objects {
            match record.payload {
                crate::legacy::ObjectPayload::Arrow => object_arrows += 1,
                crate::legacy::ObjectPayload::Inline => object_inlines += 1,
                crate::legacy::ObjectPayload::Null => object_nulls += 1,
                crate::legacy::ObjectPayload::Array { .. } => object_arrays += 1,
                crate::legacy::ObjectPayload::Opaque { .. } => {}
            }
        }
        coverage.record(
            crate::coverage::DECODED_LEGACY_OBJECT_ARROW_COUNT,
            object_arrows,
        );
        coverage.record(
            crate::coverage::DECODED_LEGACY_OBJECT_INLINE_COUNT,
            object_inlines,
        );
        coverage.record(
            crate::coverage::DECODED_LEGACY_OBJECT_NULL_COUNT,
            object_nulls,
        );
        coverage.record(
            crate::coverage::DECODED_LEGACY_OBJECT_ARRAY_COUNT,
            object_arrays,
        );
        coverage.record(
            crate::coverage::INCOMPLETE_LEGACY_OBJECT_ARRAY_COUNT,
            legacy.persistence.incomplete_object_array_count,
        );
        coverage.record(
            crate::coverage::UNRESOLVED_LEGACY_OBJECT_VALUE_COUNT,
            legacy.persistence.unresolved_object_value_count,
        );
        let integer_counts = legacy_numeric_coverage(&legacy.persistence.integer_values.rows);
        coverage.record(
            crate::coverage::DECODED_LEGACY_INTEGER_SCALAR_COUNT,
            integer_counts.scalars,
        );
        coverage.record(
            crate::coverage::DECODED_LEGACY_INTEGER_ARRAY_COUNT,
            integer_counts.arrays,
        );
        coverage.record(
            crate::coverage::DECODED_LEGACY_INTEGER_ELEMENT_COUNT,
            integer_counts.elements,
        );
        coverage.record(
            crate::coverage::UNRESOLVED_LEGACY_INTEGER_VALUE_COUNT,
            legacy.persistence.integer_values.unresolved_count,
        );
        let real_counts = legacy_numeric_coverage(&legacy.persistence.real_values.rows);
        coverage.record(
            crate::coverage::DECODED_LEGACY_REAL_SCALAR_COUNT,
            real_counts.scalars,
        );
        coverage.record(
            crate::coverage::DECODED_LEGACY_REAL_ARRAY_COUNT,
            real_counts.arrays,
        );
        coverage.record(
            crate::coverage::DECODED_LEGACY_REAL_ELEMENT_COUNT,
            real_counts.elements,
        );
        coverage.record(
            crate::coverage::UNRESOLVED_LEGACY_REAL_VALUE_COUNT,
            legacy.persistence.real_values.unresolved_count,
        );
        let (string_scalars, string_arrays, string_elements, undecoded_encodings) =
            legacy.persistence.string_values.iter().fold(
                (0usize, 0usize, 0usize, 0usize),
                |(scalars, arrays, elements, undecoded_encodings), record| {
                    (
                        scalars
                            + usize::from(matches!(
                                record.payload,
                                crate::legacy::StringPayload::Scalar { .. }
                            )),
                        arrays
                            + usize::from(matches!(
                                record.payload,
                                crate::legacy::StringPayload::Array { .. }
                            )),
                        elements.saturating_add(record.payload.element_count()),
                        undecoded_encodings
                            .saturating_add(record.payload.undecoded_encoding_count()),
                    )
                },
            );
        coverage.record(
            crate::coverage::DECODED_LEGACY_STRING_SCALAR_COUNT,
            string_scalars,
        );
        coverage.record(
            crate::coverage::DECODED_LEGACY_STRING_ARRAY_COUNT,
            string_arrays,
        );
        coverage.record(
            crate::coverage::DECODED_LEGACY_STRING_ELEMENT_COUNT,
            string_elements,
        );
        coverage.record(
            crate::coverage::INCOMPLETE_LEGACY_STRING_ARRAY_COUNT,
            legacy.persistence.incomplete_string_array_count,
        );
        coverage.record(
            crate::coverage::UNRESOLVED_LEGACY_STRING_VALUE_COUNT,
            legacy.persistence.unresolved_string_value_count,
        );
        coverage.record(
            crate::coverage::UNDECODED_LEGACY_STRING_ENCODING_COUNT,
            undecoded_encodings,
        );
        record_scalar_string_coverage(
            &mut coverage,
            crate::coverage::DECODED_LEGACY_TYPE_3_SCALAR_COUNT,
            crate::coverage::UNRESOLVED_LEGACY_TYPE_3_VALUE_COUNT,
            crate::coverage::UNDECODED_LEGACY_TYPE_3_ENCODING_COUNT,
            &legacy.persistence.type_3_values,
        );
        record_scalar_string_coverage(
            &mut coverage,
            crate::coverage::DECODED_LEGACY_TYPE_4_SCALAR_COUNT,
            crate::coverage::UNRESOLVED_LEGACY_TYPE_4_VALUE_COUNT,
            crate::coverage::UNDECODED_LEGACY_TYPE_4_ENCODING_COUNT,
            &legacy.persistence.type_4_values,
        );
        let mut insert_numbered_numeric_coverage =
            |scalar_key,
             array_key,
             element_key,
             unresolved_key,
             counts: LegacyNumericCoverage,
             unresolved| {
                coverage.record(scalar_key, counts.scalars);
                coverage.record(array_key, counts.arrays);
                coverage.record(element_key, counts.elements);
                coverage.record(unresolved_key, unresolved);
            };
        insert_numbered_numeric_coverage(
            crate::coverage::DECODED_LEGACY_TYPE_5_SCALAR_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_5_ARRAY_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_5_ELEMENT_COUNT,
            crate::coverage::UNRESOLVED_LEGACY_TYPE_5_VALUE_COUNT,
            legacy_numeric_coverage(&legacy.persistence.type_5_values.rows),
            legacy.persistence.type_5_values.unresolved_count,
        );
        insert_numbered_numeric_coverage(
            crate::coverage::DECODED_LEGACY_TYPE_6_SCALAR_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_6_ARRAY_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_6_ELEMENT_COUNT,
            crate::coverage::UNRESOLVED_LEGACY_TYPE_6_VALUE_COUNT,
            legacy_numeric_coverage(&legacy.persistence.type_6_values.rows),
            legacy.persistence.type_6_values.unresolved_count,
        );
        insert_numbered_numeric_coverage(
            crate::coverage::DECODED_LEGACY_TYPE_7_SCALAR_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_7_ARRAY_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_7_ELEMENT_COUNT,
            crate::coverage::UNRESOLVED_LEGACY_TYPE_7_VALUE_COUNT,
            legacy_numeric_coverage(&legacy.persistence.type_7_values.rows),
            legacy.persistence.type_7_values.unresolved_count,
        );
        insert_numbered_numeric_coverage(
            crate::coverage::DECODED_LEGACY_TYPE_9_SCALAR_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_9_ARRAY_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_9_ELEMENT_COUNT,
            crate::coverage::UNRESOLVED_LEGACY_TYPE_9_VALUE_COUNT,
            legacy_numeric_coverage(&legacy.persistence.type_9_values.rows),
            legacy.persistence.type_9_values.unresolved_count,
        );
        insert_numbered_numeric_coverage(
            crate::coverage::DECODED_LEGACY_TYPE_11_SCALAR_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_11_ARRAY_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_11_ELEMENT_COUNT,
            crate::coverage::UNRESOLVED_LEGACY_TYPE_11_VALUE_COUNT,
            legacy_numeric_coverage(&legacy.persistence.type_11_values.rows),
            legacy.persistence.type_11_values.unresolved_count,
        );
    }
    coverage.record(
        crate::coverage::DECODED_PRIMITIVE_TRIANGLE_STRIP_COUNT,
        scan.primitives.triangle_strips.len(),
    );
    coverage.record(
        crate::coverage::CONFLICTING_PRIMITIVE_TRIANGLE_STRIP_REPRESENTATION_COUNT,
        scan.primitives
            .conflicting_triangle_strip_representation_count,
    );
    coverage.record(
        crate::coverage::DECODED_SURFACE_ROW_COUNT,
        scan.surfaces.rows.len(),
    );
    coverage.record(
        crate::coverage::DECODED_CROSS_SECTION_SURFACE_ROW_COUNT,
        scan.surfaces.cross_section_rows.len(),
    );
    coverage.record(
        crate::coverage::DECODED_SURFACE_PARAMETER_RECORD_COUNT,
        scan.surfaces.parameters.len(),
    );
    coverage.record(
        crate::coverage::DECODED_CROSS_SECTION_SURFACE_PARAMETER_RECORD_COUNT,
        scan.surfaces.cross_section_parameters.len(),
    );
    coverage.record(
        crate::coverage::DECODED_POSITIONAL_EXTRUSION_DIRECTION_COUNT,
        scan.surfaces
            .parameters
            .iter()
            .filter(|record| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id).is_some()
                    && record.extrusion_direction().is_some()
            })
            .count(),
    );
    let torus_coverage = torus_parameter_coverage(scan);
    coverage.record(
        crate::coverage::DECODED_TORUS_RADIUS_OVERRIDE_COUNT,
        torus_coverage.radius_overrides,
    );
    coverage.record(
        crate::coverage::DECODED_TYPE26_REPLAYED_MINOR_RADIUS_COUNT,
        torus_coverage.replayed_minor_radii,
    );
    coverage.record(
        crate::coverage::DECODED_TORUS_OUTLINE_EXTENT_COUNT,
        torus_coverage.outline_extents,
    );
    coverage.record(
        crate::coverage::DECODED_TYPE26_FIVE_COORDINATE_ENVELOPE_COUNT,
        torus_coverage.five_coordinate_envelopes,
    );
    coverage.record(
        crate::coverage::DECODED_TYPE26_SPLIT_COORDINATE_ENVELOPE_COUNT,
        torus_coverage.split_coordinate_envelopes,
    );
    coverage.record(
        crate::coverage::DECODED_PLANE_LOCAL_SYSTEM_COUNT,
        scan.planes.local_systems.len(),
    );
    coverage.record(
        crate::coverage::DECODED_CROSS_SECTION_PLANE_LOCAL_SYSTEM_COUNT,
        scan.planes.cross_section_local_systems.len(),
    );
    coverage.record(
        crate::coverage::DECODED_PLANE_ENVELOPE_COUNT,
        scan.planes.envelopes.len(),
    );
    coverage.record(
        crate::coverage::DECODED_CROSS_SECTION_PLANE_ENVELOPE_COUNT,
        scan.planes.cross_section_envelopes.len(),
    );
    coverage.record(
        crate::coverage::DECODED_OUTLINE_PLANE_COUNT,
        scan.planes.outlines.len(),
    );
    coverage.record(
        crate::coverage::DECODED_POSITIONAL_FRAME_PLANE_COUNT,
        scan.planes.positional_frames.len(),
    );
    coverage.record(
        crate::coverage::DECODED_CROSS_SECTION_OUTLINE_PLANE_COUNT,
        scan.planes.cross_section_outlines.len(),
    );
    coverage.record(
        crate::coverage::DECODED_SURFACE_PROTOTYPE_COUNT,
        scan.surfaces.prototype_count,
    );
    coverage.record(
        crate::coverage::DECODED_NAMED_SURFACE_PROTOTYPE_COUNT,
        scan.surfaces.prototype_records.len(),
    );
    coverage.record(
        crate::coverage::DECODED_REFERENCE_LINE_COUNT,
        scan.references.lines.len(),
    );
    coverage.record(
        crate::coverage::DECODED_REFERENCE_CIRCLE_COUNT,
        scan.references.circles.len(),
    );
    coverage.record(
        crate::coverage::DECODED_REFERENCE_CONIC_COUNT,
        scan.references.conics.len(),
    );
    coverage.record(
        crate::coverage::TRANSFERRED_REFERENCE_ELLIPSE_COUNT,
        scan.references.ellipses.len(),
    );
    coverage.record(
        crate::coverage::DECODED_TABULATED_CYLINDER_CURVE_REPLAY_COUNT,
        scan.curves.tabulated_cylinder_replays.len(),
    );
    coverage.record(
        crate::coverage::DECODED_TABULATED_CYLINDER_CONTROL_POINT_SET_COUNT,
        scan.curves
            .tabulated_cylinder_replays
            .iter()
            .filter(|replay| replay.control_points.iter().all(Option::is_some))
            .count(),
    );
    coverage.record(
        crate::coverage::DECODED_CURVE_PROTOTYPE_COUNT,
        scan.curves.prototypes.len(),
    );
    coverage.record(
        crate::coverage::DECODED_CURVE_PARAMETER_RECORD_COUNT,
        scan.curves.parameters.len(),
    );
    coverage.record(
        crate::coverage::DECODED_CURVE_EXPRESSION_RECORD_COUNT,
        scan.curves.expressions.len(),
    );
    insert_source_attribute(ctx, &mut attributes, "expanded_section_count", scan.framing.expanded_sections.len())?;
    insert_source_attribute(
        ctx,
        &mut attributes,
        "expanded_section_byte_count",
        scan.framing.expanded_sections.iter().map(|section| section.data.len()).sum::<usize>(),
    )?;
    if let Some(family_table) = scan.framing.family_table {
        match family_table.pointer {
            crate::container::FamilyTablePointer::Null => {
                insert_source_attribute(ctx, &mut attributes, "family_table_pointer", "null")?;
                insert_source_attribute(ctx, &mut attributes, "configuration_state", "none")?;
            }
            crate::container::FamilyTablePointer::Entity(id) => {
                insert_source_attribute(ctx, &mut attributes, "family_table_pointer", format_args!("entity:{id}"))?;
                insert_source_attribute(ctx, &mut attributes, "configuration_state", "driver_table_unresolved")?;
            }
        }
    }
    let configuration_driver_table_reference_count =
        usize::from(scan.framing.family_table.is_some_and(|table| {
            matches!(
                table.pointer,
                crate::container::FamilyTablePointer::Entity(_)
            )
        }));
    coverage.record(
        crate::coverage::DECODED_CONFIGURATION_DRIVER_TABLE_REFERENCE_COUNT,
        configuration_driver_table_reference_count,
    );
    let legacy_family_table = scan.framing.legacy_family_table.as_ref();
    coverage.record(
        crate::coverage::DECODED_LEGACY_CONFIGURATION_DRIVER_TABLE_COUNT,
        usize::from(legacy_family_table.is_some()),
    );
    coverage.record(
        crate::coverage::DECODED_LEGACY_CONFIGURATION_ITEM_COUNT,
        legacy_family_table.map_or(0, |table| table.items.len()),
    );
    coverage.record(
        crate::coverage::DECODED_LEGACY_CONFIGURATION_INSTANCE_COUNT,
        legacy_family_table.map_or(0, |table| table.instances.len()),
    );
    coverage.record(
        crate::coverage::TRANSFERRED_CONFIGURATION_DRIVER_TABLE_COUNT,
        0,
    );
    coverage.record(
        crate::coverage::DECODED_PCURVE_COUNT,
        scan.curves.pcurves.len(),
    );
    coverage.record(
        crate::coverage::DECODED_TWO_CHART_PCURVE_COUNT,
        scan.curves.two_chart_pcurves.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FC_CURVE_COORDINATE_RECORD_COUNT,
        scan.curves.fc_coordinates.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FC05_CIRCLE_COUNT,
        scan.curves.fc05_circles.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FC05_CYLINDER_CAP_PAIR_COUNT,
        scan.curves.fc05_cylinder_cap_pairs.len(),
    );
    coverage.record(
        crate::coverage::DECODED_PROTOTYPE_PCURVE_COUNT,
        scan.curves.prototype_pcurves.len(),
    );
    coverage.record(
        crate::coverage::DECODED_CURVE_PROTOTYPE_TOPOLOGY_COUNT,
        scan.curves.prototype_topology.len(),
    );
    coverage.record(
        crate::coverage::DECODED_BOUND_PROTOTYPE_PCURVE_COUNT,
        scan.curves.bound_prototype_pcurves.len(),
    );
    coverage.record(
        crate::coverage::DECODED_CURVE_TOPOLOGY_ROW_COUNT,
        scan.curves.topology_rows.len(),
    );
    coverage.record(
        crate::coverage::DECODED_CROSS_SECTION_CURVE_ROW_COUNT,
        scan.curves.cross_section_rows.len(),
    );
    coverage.record(
        crate::coverage::DECODED_CROSS_SECTION_CURVE_PROTOTYPE_COUNT,
        scan.curves.cross_section_prototypes.len(),
    );
    coverage.record(
        crate::coverage::DECODED_HALF_EDGE_COUNT,
        scan.topology.half_edges.len(),
    );
    coverage.record(
        crate::coverage::DECODED_TOPOLOGICAL_VERTEX_COUNT,
        scan.topology.vertices.len(),
    );
    coverage.record(
        crate::coverage::DECODED_LOOP_COUNT,
        scan.topology.loops.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FACE_COMPONENT_COUNT,
        scan.topology.face_components.len(),
    );
    coverage.record(
        crate::coverage::DECODED_DATUM_PLANE_COUNT,
        scan.planes.datums.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_COUNT,
        scan.features.ids.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_ROW_COUNT,
        scan.features.rows.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_CHOICE_COUNT,
        scan.features.choices.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_CHOICE_FIELD_COUNT,
        scan.features.choice_fields.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_GEOMETRY_TABLE_COUNT,
        scan.features.geometry_tables.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_LOOP_HISTORY_ENTRY_COUNT,
        scan.features.loop_history_entries.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_AFFECTED_ID_ARRAY_COUNT,
        scan.features.affected_ids.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_REPLAY_AFFECTED_ID_COUNT,
        scan.features.replay_affected_ids.len(),
    );
    coverage.record(
        crate::coverage::DECODED_SURFACE_MERGE_REPLAY_AFFECTED_ID_COUNT,
        scan.features.surface_merge_replay_affected_ids.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_LOOP_RESTORE_DIRECTION_COUNT,
        scan.features.loop_restore_directions.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_REVOLUTION_EXTENT_COUNT,
        scan.features.revolution_extents.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_DEFINITION_COUNT,
        scan.features.definitions.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_SECTION_TRANSFORM_COUNT,
        scan.features.section_transforms.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_PLACEMENT_INSTRUCTION_COUNT,
        scan.features
            .definitions
            .iter()
            .map(|definition| crate::feature::definitions::placement_instructions(definition).count())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_OPERATION_STATE_COUNT,
        scan.features.operation_states.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_OPERATION_COUNT,
        scan.features.operations.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_OUTLINE_COUNT,
        scan.features
            .definitions
            .iter()
            .map(|definition| definition.outlines.len())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_SECTION_POINT_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.variables.as_ref())
            .map(|variables| {
                let (points, ambiguous) = variables.reconciled_points();
                points.len() + ambiguous.len()
            })
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_SOLVER_VARIABLE_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.variables.as_ref())
            .map(|variables| variables.rows.len())
            .sum::<usize>(),
    );
    let missing_feature_solver_variable_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.variables.as_ref())
        .try_fold(0usize, |total, variables| {
            let declared_count = usize::try_from(variables.declared_count).map_err(|_| {
                cadmpeg_core::CodecError::malformed(
                    "feature solver variable count does not fit the host index type",
                )
            })?;
            let missing = declared_count
                .checked_sub(variables.rows.len())
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "feature solver variable rows exceed the declared variable count",
                    )
                })?;
            Ok::<usize, cadmpeg_core::CodecError>(total + missing)
        })?;
    coverage.record(
        crate::coverage::MISSING_FEATURE_SOLVER_VARIABLE_COUNT,
        missing_feature_solver_variable_count,
    );
    let (
        decoded_dimension_driven_variable_count,
        decoded_dimension_driven_coordinate_variable_count,
    ) = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.variables.as_ref())
        .flat_map(|variables| &variables.rows)
        .filter(|row| row.value == ScalarLane::DimensionDriven)
        .fold((0usize, 0usize), |(all, coordinates), row| {
            (
                all + 1,
                coordinates
                    + usize::from(matches!(
                        row.variable_type,
                        VariableType::U | VariableType::V
                    )),
            )
        });
    let decoded_dimension_driven_guess_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.variables.as_ref())
        .flat_map(|variables| &variables.rows)
        .filter(|row| row.guess == ScalarLane::DimensionDriven)
        .count();
    let (
        resolved_dimension_driven_variable_count,
        resolved_dimension_driven_coordinate_variable_count,
        resolved_dimension_driven_other_variable_count,
    ) = scan
        .features
        .definitions
        .iter()
        .map(|definition| {
            let resolved_coordinates = resolved_section_coordinates(ctx, definition)?;
            let resolved_radii = resolved_section_radii(ctx, definition)?;
            let resolved_scalars = resolved_section_scalar_values(ctx, definition)?;
            Ok::<_, cadmpeg_core::CodecError>(
                definition
                    .variables
                    .iter()
                    .flat_map(|variables| &variables.rows)
                    .filter(|row| row.value == ScalarLane::DimensionDriven)
                    .fold(
                        (0usize, 0usize, 0usize),
                        |(all, coordinates, other), row| {
                            let resolved = match row.variable_type {
                                VariableType::U | VariableType::V => {
                                    resolved_coordinates.get(&row.key).and_then(|point| {
                                        point[usize::from(row.variable_type == VariableType::V)]
                                    })
                                }
                                VariableType::Radius => resolved_radii.get(&row.key).copied(),
                                _ => resolved_scalars.get(&(row.variable_type, row.key)).copied(),
                            };
                            (
                                all + usize::from(resolved.is_some()),
                                coordinates
                                    + usize::from(
                                        matches!(
                                            row.variable_type,
                                            VariableType::U | VariableType::V
                                        ) && resolved.is_some(),
                                    ),
                                other
                                    + usize::from(
                                        !matches!(
                                            row.variable_type,
                                            VariableType::U | VariableType::V
                                        ) && resolved.is_some(),
                                    ),
                            )
                        },
                    ),
            )
        })
        .try_fold((0usize, 0usize, 0usize), |total, counts| {
            let counts = counts?;
            Ok::<_, cadmpeg_core::CodecError>((
                total.0 + counts.0,
                total.1 + counts.1,
                total.2 + counts.2,
            ))
        })?;
    coverage.record(
        crate::coverage::DECODED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT,
        decoded_dimension_driven_variable_count,
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT,
        decoded_dimension_driven_coordinate_variable_count,
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT,
        decoded_dimension_driven_variable_count
            .saturating_sub(decoded_dimension_driven_coordinate_variable_count),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_DIMENSION_DRIVEN_GUESS_COUNT,
        decoded_dimension_driven_guess_count,
    );
    coverage.record(
        crate::coverage::RESOLVED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT,
        resolved_dimension_driven_variable_count,
    );
    coverage.record(
        crate::coverage::RESOLVED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT,
        resolved_dimension_driven_coordinate_variable_count,
    );
    coverage.record(
        crate::coverage::RESOLVED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT,
        resolved_dimension_driven_other_variable_count,
    );
    coverage.record(
        crate::coverage::UNRESOLVED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT,
        decoded_dimension_driven_variable_count
            .saturating_sub(resolved_dimension_driven_variable_count),
    );
    coverage.record(
        crate::coverage::UNRESOLVED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT,
        decoded_dimension_driven_coordinate_variable_count
            .saturating_sub(resolved_dimension_driven_coordinate_variable_count),
    );
    coverage.record(
        crate::coverage::UNRESOLVED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT,
        decoded_dimension_driven_variable_count
            .saturating_sub(decoded_dimension_driven_coordinate_variable_count)
            .saturating_sub(resolved_dimension_driven_other_variable_count),
    );
    coverage.record(
        crate::coverage::UNRESOLVED_FEATURE_DIMENSION_DRIVEN_GUESS_COUNT,
        decoded_dimension_driven_guess_count,
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_CIRCLE_SEGMENT_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.rows.circles().count())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_POINT_SEGMENT_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.rows.points().count())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_CENTERED_LINE_SEGMENT_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.rows.centered_lines().count())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_REFERENCE_LINE_SEGMENT_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.rows.reference_lines().count())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_BOUNDED_CURVE_SEGMENT_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.rows.bounded_curves().count())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_CONIC_SEGMENT_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.rows.conics().count())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_OPAQUE_SEGMENT_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.rows.opaque().count())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_TRIM_ENTITY_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.trim_entities.as_ref())
            .map(|entities| entities.rows.len())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_TRIM_VERTEX_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.trim_vertices.as_ref())
            .map(|vertices| vertices.rows.len())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_ORDER_ENTRY_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.order_table.as_ref())
            .map(|order| order.rows.len())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_DIMENSION_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.dimensions.as_ref())
            .map(|dimensions| dimensions.rows.len())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_RELATION_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.relations.as_ref())
            .map(|relations| relations.rows.len())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_EQUATION_TABLE_COUNT,
        scan.features
            .definitions
            .iter()
            .filter(|definition| {
                crate::feature::definitions::equation_table(
                    &definition.body,
                    0,
                    definition.body.len(),
                )
                .is_some()
            })
            .count(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_EQUATION_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| {
                crate::feature::definitions::equation_table(
                    &definition.body,
                    0,
                    definition.body.len(),
                )
            })
            .map(|equations| equations.rows.len())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_SAVED_ENTITY_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.saved_section.as_ref())
            .map(|saved| saved.entities.len())
            .sum::<usize>(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_SAVED_CONIC_COUNT,
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.saved_section.as_ref())
            .flat_map(|saved| &saved.entities)
            .filter(|entity| {
                matches!(
                    entity,
                    crate::feature::definitions::FeatureSavedEntity::Conic(_)
                )
            })
            .count(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_ENTITY_COUNT,
        scan.features.entities.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_ENTITY_REFERENCE_COUNT,
        scan.features.entity_references.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_ENTITY_TABLE_COUNT,
        scan.features.entity_tables.len(),
    );
    coverage.record(
        crate::coverage::DECODED_FEATURE_SURFACE_REPLAY_ASSOCIATION_COUNT,
        feature_surface_replay_associations(scan).len(),
    );
    if let Some(count) = scan.framing.declared_body_count {
        insert_source_attribute(ctx, &mut attributes, "declared_body_count", count)?;
    }
    if let Some(value) = scan.framing.first_quilt_ptr {
        insert_source_attribute(ctx, &mut attributes, "first_quilt_ptr", value)?;
    }
    Ok((
        SourceMeta::classified(
            DialectLayers::of(classification.matched().clone()),
            cadmpeg_core::text::named_entries_checked(ctx, "the creo container", attributes)?,
        ),
        coverage,
    ))
}

fn record_scalar_string_coverage<K>(
    coverage: &mut cadmpeg_ir::report::decode::Coverage,
    scalar_key: cadmpeg_ir::report::decode::CoverageKey,
    unresolved_key: cadmpeg_ir::report::decode::CoverageKey,
    undecoded_key: cadmpeg_ir::report::decode::CoverageKey,
    values: &crate::legacy::TypedValues<crate::legacy::ValueRecord<K>>,
) where
    K: crate::legacy::LegacyCode<Payload = crate::legacy::StringValue>,
{
    let undecoded_encodings = values
        .rows
        .iter()
        .map(|record| record.payload.undecoded_encoding_count())
        .sum();
    coverage.record(scalar_key, values.rows.len());
    coverage.record(unresolved_key, values.unresolved_count);
    coverage.record(undecoded_key, undecoded_encodings);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    use super::insert_source_attribute;

    #[test]
    fn source_attribute_map_refuses_new_node_at_collection_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        let mut attributes = BTreeMap::new();
        let error = insert_source_attribute(&ctx, &mut attributes, "file_size", 12)
            .expect_err("one source attribute needs one map node");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "Creo source attribute map nodes"));
        assert!(attributes.is_empty());
    }

    #[test]
    fn source_attribute_map_keeps_rendered_key_value_and_order() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        let mut attributes = BTreeMap::new();
        insert_source_attribute(&ctx, &mut attributes, "version_line", "Creo 10")
            .expect("first attribute is admitted");
        insert_source_attribute(&ctx, &mut attributes, format_args!("section.{}.name", 0), "MdlStatus")
            .expect("section attribute is admitted");
        assert_eq!(attributes["version_line"], "Creo 10");
        assert_eq!(attributes["section.0.name"], "MdlStatus");
    }

    #[test]
    fn principal_unit_attribute_refuses_before_retaining_token() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = ("principal_unit".len() + "unknown:7".len() - 1) as u64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        let mut attributes = BTreeMap::new();
        let error = insert_source_attribute(
            &ctx,
            &mut attributes,
            "principal_unit",
            crate::legacy::PrincipalUnitSystem::UnknownBinarySelector(7),
        )
        .expect_err("token needs one more retained byte");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "Creo source attribute value"));
        assert!(attributes.is_empty());
    }
}
