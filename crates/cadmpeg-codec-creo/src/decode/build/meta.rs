// SPDX-License-Identifier: Apache-2.0
//! Source metadata and structural decode-coverage census.

use std::collections::BTreeMap;

use crate::container::ContainerScan;

use super::super::expanded::feature_surface_replay_associations;
use super::super::sketch::{
    resolved_section_coordinates, resolved_section_radii, resolved_section_scalar_values,
};
use super::coverage::{legacy_numeric_coverage, torus_parameter_coverage};
use cadmpeg_ir::document::SourceMeta;

pub(in super::super) fn source_meta(scan: &ContainerScan) -> (SourceMeta, BTreeMap<String, usize>) {
    let mut attributes = BTreeMap::new();
    let mut coverage = BTreeMap::new();
    attributes.insert(
        "version_line".to_string(),
        scan.framing.version_line.clone(),
    );
    if let Some(name) = &scan.framing.model_name {
        attributes.insert("model_name".to_string(), name.clone());
    }
    attributes.insert(
        "layout".to_string(),
        scan.framing.layout.token().to_string(),
    );
    if let Some(legacy) = &scan.framing.legacy_ascii {
        attributes.insert("legacy_ascii_schema".to_string(), legacy.schema.clone());
        if let Some(release) = &legacy.product_release {
            attributes.insert("legacy_ascii_product_release".to_string(), release.clone());
        }
        attributes.insert(
            "legacy_ascii_declaration_count".to_string(),
            legacy.persistence.declaration_count().to_string(),
        );
        attributes.insert(
            "legacy_ascii_scope_count".to_string(),
            legacy.persistence.scopes.len().to_string(),
        );
        attributes.insert(
            "legacy_ascii_value_count".to_string(),
            legacy.persistence.value_count().to_string(),
        );
        attributes.insert(
            "legacy_ascii_continuation_count".to_string(),
            legacy.persistence.continuation_count().to_string(),
        );
        attributes.insert(
            "legacy_ascii_unresolved_value_count".to_string(),
            legacy.persistence.unresolved_value_count().to_string(),
        );
        attributes.insert(
            "legacy_ascii_conflicting_declaration_count".to_string(),
            legacy
                .persistence
                .conflicting_declaration_count()
                .to_string(),
        );
    }
    attributes.insert("file_size".to_string(), scan.framing.data.len().to_string());
    attributes.insert(
        "section_count".to_string(),
        scan.framing.sections.len().to_string(),
    );
    for (index, section) in scan.framing.sections.iter().enumerate() {
        let prefix = format!("section.{index}");
        attributes.insert(format!("{prefix}.name"), section.name.clone());
        attributes.insert(format!("{prefix}.raw_name"), section.raw_name.clone());
        attributes.insert(format!("{prefix}.role"), section.role.to_string());
        attributes.insert(format!("{prefix}.offset"), section.offset.to_string());
        attributes.insert(format!("{prefix}.length"), section.length.to_string());
    }
    if let Some(c) = scan.framing.census.srf_array_count {
        attributes.insert("srf_array_count".to_string(), c.to_string());
    }
    if let Some(c) = scan.framing.census.crv_array_count {
        attributes.insert("crv_array_count".to_string(), c.to_string());
    }
    if let Some(unit) = &scan.framing.principal_unit {
        attributes.insert("principal_unit".to_string(), unit.token());
        if let Some(scale) = unit.length_scale_mm().filter(|scale| *scale != 1.0) {
            attributes.insert("source_length_scale_mm".to_string(), scale.to_string());
        }
    }
    if scan.framing.layout == crate::container::Layout::LegacyAscii {
        coverage.insert(
            "decoded_legacy_principal_unit_count".to_string(),
            usize::from(scan.framing.principal_unit.is_some()),
        );
        if let Some(legacy) = &scan.framing.legacy_ascii {
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
            coverage.insert(
                "decoded_legacy_object_arrow_count".to_string(),
                object_arrows,
            );
            coverage.insert(
                "decoded_legacy_object_inline_count".to_string(),
                object_inlines,
            );
            coverage.insert("decoded_legacy_object_null_count".to_string(), object_nulls);
            coverage.insert(
                "decoded_legacy_object_array_count".to_string(),
                object_arrays,
            );
            coverage.insert(
                "incomplete_legacy_object_array_count".to_string(),
                legacy.persistence.incomplete_object_array_count,
            );
            coverage.insert(
                "unresolved_legacy_object_value_count".to_string(),
                legacy.persistence.unresolved_object_value_count,
            );
            let (integer_scalars, integer_arrays, integer_elements) =
                legacy_numeric_coverage(&legacy.persistence.integer_values);
            coverage.insert(
                "decoded_legacy_integer_scalar_count".to_string(),
                integer_scalars,
            );
            coverage.insert(
                "decoded_legacy_integer_array_count".to_string(),
                integer_arrays,
            );
            coverage.insert(
                "decoded_legacy_integer_element_count".to_string(),
                integer_elements,
            );
            coverage.insert(
                "unresolved_legacy_integer_value_count".to_string(),
                legacy.persistence.unresolved_integer_value_count,
            );
            let (real_scalars, real_arrays, real_elements) =
                legacy_numeric_coverage(&legacy.persistence.real_values);
            coverage.insert("decoded_legacy_real_scalar_count".to_string(), real_scalars);
            coverage.insert("decoded_legacy_real_array_count".to_string(), real_arrays);
            coverage.insert(
                "decoded_legacy_real_element_count".to_string(),
                real_elements,
            );
            coverage.insert(
                "unresolved_legacy_real_value_count".to_string(),
                legacy.persistence.unresolved_real_value_count,
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
            coverage.insert(
                "decoded_legacy_string_scalar_count".to_string(),
                string_scalars,
            );
            coverage.insert(
                "decoded_legacy_string_array_count".to_string(),
                string_arrays,
            );
            coverage.insert(
                "decoded_legacy_string_element_count".to_string(),
                string_elements,
            );
            coverage.insert(
                "incomplete_legacy_string_array_count".to_string(),
                legacy.persistence.incomplete_string_array_count,
            );
            coverage.insert(
                "unresolved_legacy_string_value_count".to_string(),
                legacy.persistence.unresolved_string_value_count,
            );
            coverage.insert(
                "undecoded_legacy_string_encoding_count".to_string(),
                undecoded_encodings,
            );
            for (type_code, records, unresolved) in [
                (
                    3u8,
                    legacy.persistence.type_3_values.as_slice(),
                    legacy.persistence.unresolved_type_3_value_count,
                ),
                (
                    4u8,
                    legacy.persistence.type_4_values.as_slice(),
                    legacy.persistence.unresolved_type_4_value_count,
                ),
            ] {
                let scalars = records.len();
                let undecoded_encodings = records
                    .iter()
                    .map(|record| record.payload.undecoded_encoding_count())
                    .sum();
                coverage.insert(
                    format!("decoded_legacy_type_{type_code}_scalar_count"),
                    scalars,
                );
                coverage.insert(
                    format!("unresolved_legacy_type_{type_code}_value_count"),
                    unresolved,
                );
                coverage.insert(
                    format!("undecoded_legacy_type_{type_code}_encoding_count"),
                    undecoded_encodings,
                );
            }
            let mut insert_numbered_numeric_coverage =
                |type_code: u8, (scalars, arrays, elements), unresolved| {
                    coverage.insert(
                        format!("decoded_legacy_type_{type_code}_scalar_count"),
                        scalars,
                    );
                    coverage.insert(
                        format!("decoded_legacy_type_{type_code}_array_count"),
                        arrays,
                    );
                    coverage.insert(
                        format!("decoded_legacy_type_{type_code}_element_count"),
                        elements,
                    );
                    coverage.insert(
                        format!("unresolved_legacy_type_{type_code}_value_count"),
                        unresolved,
                    );
                };
            insert_numbered_numeric_coverage(
                5,
                legacy_numeric_coverage(&legacy.persistence.type_5_values),
                legacy.persistence.unresolved_type_5_value_count,
            );
            insert_numbered_numeric_coverage(
                6,
                legacy_numeric_coverage(&legacy.persistence.type_6_values),
                legacy.persistence.unresolved_type_6_value_count,
            );
            insert_numbered_numeric_coverage(
                7,
                legacy_numeric_coverage(&legacy.persistence.type_7_values),
                legacy.persistence.unresolved_type_7_value_count,
            );
            insert_numbered_numeric_coverage(
                9,
                legacy_numeric_coverage(&legacy.persistence.type_9_values),
                legacy.persistence.unresolved_type_9_value_count,
            );
            insert_numbered_numeric_coverage(
                11,
                legacy_numeric_coverage(&legacy.persistence.type_11_values),
                legacy.persistence.unresolved_type_11_value_count,
            );
        }
    }
    coverage.insert(
        "decoded_primitive_triangle_strip_count".to_string(),
        scan.primitives.triangle_strips.len(),
    );
    coverage.insert(
        "conflicting_primitive_triangle_strip_representation_count".to_string(),
        scan.primitives
            .conflicting_triangle_strip_representation_count,
    );
    coverage.insert(
        "decoded_surface_row_count".to_string(),
        scan.surfaces.rows.len(),
    );
    coverage.insert(
        "decoded_cross_section_surface_row_count".to_string(),
        scan.surfaces.cross_section_rows.len(),
    );
    coverage.insert(
        "decoded_surface_parameter_record_count".to_string(),
        scan.surfaces.parameters.len(),
    );
    coverage.insert(
        "decoded_cross_section_surface_parameter_record_count".to_string(),
        scan.surfaces.cross_section_parameters.len(),
    );
    coverage.insert(
        "decoded_positional_extrusion_direction_count".to_string(),
        scan.surfaces
            .parameters
            .iter()
            .filter(|record| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
                    .is_some_and(|row| {
                        row.kind == crate::surface::SurfaceKind::Extrusion
                            && record.extrusion_direction(row.type_byte).is_some()
                    })
            })
            .count(),
    );
    let torus_coverage = torus_parameter_coverage(scan);
    coverage.insert(
        "decoded_torus_radius_override_count".to_string(),
        torus_coverage.radius_overrides,
    );
    coverage.insert(
        "decoded_type26_replayed_minor_radius_count".to_string(),
        torus_coverage.replayed_minor_radii,
    );
    coverage.insert(
        "decoded_torus_outline_extent_count".to_string(),
        torus_coverage.outline_extents,
    );
    coverage.insert(
        "decoded_type26_five_coordinate_envelope_count".to_string(),
        torus_coverage.five_coordinate_envelopes,
    );
    coverage.insert(
        "decoded_type26_split_coordinate_envelope_count".to_string(),
        torus_coverage.split_coordinate_envelopes,
    );
    coverage.insert(
        "decoded_plane_local_system_count".to_string(),
        scan.planes.local_systems.len(),
    );
    coverage.insert(
        "decoded_cross_section_plane_local_system_count".to_string(),
        scan.planes.cross_section_local_systems.len(),
    );
    coverage.insert(
        "decoded_plane_envelope_count".to_string(),
        scan.planes.envelopes.len(),
    );
    coverage.insert(
        "decoded_cross_section_plane_envelope_count".to_string(),
        scan.planes.cross_section_envelopes.len(),
    );
    coverage.insert(
        "decoded_outline_plane_count".to_string(),
        scan.planes.outlines.len(),
    );
    coverage.insert(
        "decoded_positional_frame_plane_count".to_string(),
        scan.planes.positional_frames.len(),
    );
    coverage.insert(
        "decoded_cross_section_outline_plane_count".to_string(),
        scan.planes.cross_section_outlines.len(),
    );
    coverage.insert(
        "decoded_surface_prototype_count".to_string(),
        scan.surfaces.prototypes.len(),
    );
    coverage.insert(
        "decoded_named_surface_prototype_count".to_string(),
        scan.surfaces.prototype_records.len(),
    );
    coverage.insert(
        "decoded_reference_line_count".to_string(),
        scan.references.lines.len(),
    );
    coverage.insert(
        "decoded_reference_circle_count".to_string(),
        scan.references.circles.len(),
    );
    coverage.insert(
        "decoded_reference_conic_count".to_string(),
        scan.references.conics.len(),
    );
    coverage.insert(
        "transferred_reference_ellipse_count".to_string(),
        scan.references.ellipses.len(),
    );
    coverage.insert(
        "decoded_tabulated_cylinder_curve_replay_count".to_string(),
        scan.curves.tabulated_cylinder_replays.len(),
    );
    coverage.insert(
        "decoded_tabulated_cylinder_control_point_set_count".to_string(),
        scan.curves
            .tabulated_cylinder_replays
            .iter()
            .filter(|replay| replay.control_points.iter().all(Option::is_some))
            .count(),
    );
    coverage.insert(
        "decoded_curve_prototype_count".to_string(),
        scan.curves.prototypes.len(),
    );
    coverage.insert(
        "decoded_curve_parameter_record_count".to_string(),
        scan.curves.parameters.len(),
    );
    coverage.insert(
        "decoded_curve_expression_record_count".to_string(),
        scan.curves.expressions.len(),
    );
    attributes.insert(
        "expanded_section_count".to_string(),
        scan.framing.expanded_sections.len().to_string(),
    );
    attributes.insert(
        "expanded_section_byte_count".to_string(),
        scan.framing
            .expanded_sections
            .iter()
            .map(|section| section.data.len())
            .sum::<usize>()
            .to_string(),
    );
    if let Some(family_table) = scan.framing.family_table {
        attributes.insert(
            "family_table_pointer".to_string(),
            match family_table.pointer {
                crate::container::FamilyTablePointer::Null => "null".to_string(),
                crate::container::FamilyTablePointer::Entity(id) => format!("entity:{id}"),
            },
        );
        attributes.insert(
            "configuration_state".to_string(),
            match family_table.pointer {
                crate::container::FamilyTablePointer::Null => "none".to_string(),
                crate::container::FamilyTablePointer::Entity(_) => {
                    "driver_table_unresolved".to_string()
                }
            },
        );
    }
    let configuration_driver_table_reference_count =
        usize::from(scan.framing.family_table.is_some_and(|table| {
            matches!(
                table.pointer,
                crate::container::FamilyTablePointer::Entity(_)
            )
        }));
    coverage.insert(
        "decoded_configuration_driver_table_reference_count".to_string(),
        configuration_driver_table_reference_count,
    );
    let legacy_family_table = scan.framing.legacy_family_table.as_ref();
    coverage.insert(
        "decoded_legacy_configuration_driver_table_count".to_string(),
        usize::from(legacy_family_table.is_some()),
    );
    coverage.insert(
        "decoded_legacy_configuration_item_count".to_string(),
        legacy_family_table.map_or(0, |table| table.items.len()),
    );
    coverage.insert(
        "decoded_legacy_configuration_instance_count".to_string(),
        legacy_family_table.map_or(0, |table| table.instances.len()),
    );
    coverage.insert(
        "transferred_configuration_driver_table_count".to_string(),
        0,
    );
    coverage.insert(
        "decoded_pcurve_count".to_string(),
        scan.curves.pcurves.len(),
    );
    coverage.insert(
        "decoded_two_chart_pcurve_count".to_string(),
        scan.curves.two_chart_pcurves.len(),
    );
    coverage.insert(
        "decoded_fc_curve_coordinate_record_count".to_string(),
        scan.curves.fc_coordinates.len(),
    );
    coverage.insert(
        "decoded_fc05_circle_count".to_string(),
        scan.curves.fc05_circles.len(),
    );
    coverage.insert(
        "decoded_fc05_cylinder_cap_pair_count".to_string(),
        scan.curves.fc05_cylinder_cap_pairs.len(),
    );
    coverage.insert(
        "decoded_prototype_pcurve_count".to_string(),
        scan.curves.prototype_pcurves.len(),
    );
    coverage.insert(
        "decoded_curve_prototype_topology_count".to_string(),
        scan.curves.prototype_topology.len(),
    );
    coverage.insert(
        "decoded_bound_prototype_pcurve_count".to_string(),
        scan.curves.bound_prototype_pcurves.len(),
    );
    coverage.insert(
        "decoded_curve_topology_row_count".to_string(),
        scan.curves.topology_rows.len(),
    );
    coverage.insert(
        "decoded_cross_section_curve_row_count".to_string(),
        scan.curves.cross_section_rows.len(),
    );
    coverage.insert(
        "decoded_cross_section_curve_prototype_count".to_string(),
        scan.curves.cross_section_prototypes.len(),
    );
    coverage.insert(
        "decoded_half_edge_count".to_string(),
        scan.topology.half_edges.len(),
    );
    coverage.insert(
        "decoded_topological_vertex_count".to_string(),
        scan.topology.vertices.len(),
    );
    coverage.insert("decoded_loop_count".to_string(), scan.topology.loops.len());
    coverage.insert(
        "decoded_face_component_count".to_string(),
        scan.topology.face_components.len(),
    );
    coverage.insert(
        "decoded_datum_plane_count".to_string(),
        scan.planes.datums.len(),
    );
    coverage.insert("decoded_feature_count".to_string(), scan.features.ids.len());
    coverage.insert(
        "decoded_feature_row_count".to_string(),
        scan.features.rows.len(),
    );
    coverage.insert(
        "decoded_feature_choice_count".to_string(),
        scan.features.choices.len(),
    );
    coverage.insert(
        "decoded_feature_choice_field_count".to_string(),
        scan.features.choice_fields.len(),
    );
    coverage.insert(
        "decoded_feature_geometry_table_count".to_string(),
        scan.features.geometry_tables.len(),
    );
    coverage.insert(
        "decoded_feature_loop_history_entry_count".to_string(),
        scan.features.loop_history_entries.len(),
    );
    coverage.insert(
        "decoded_feature_affected_id_array_count".to_string(),
        scan.features.affected_ids.len(),
    );
    coverage.insert(
        "decoded_feature_replay_affected_id_count".to_string(),
        scan.features.replay_affected_ids.len(),
    );
    coverage.insert(
        "decoded_surface_merge_replay_affected_id_count".to_string(),
        scan.features.surface_merge_replay_affected_ids.len(),
    );
    coverage.insert(
        "decoded_feature_loop_restore_direction_count".to_string(),
        scan.features.loop_restore_directions.len(),
    );
    coverage.insert(
        "decoded_feature_revolution_extent_count".to_string(),
        scan.features.revolution_extents.len(),
    );
    coverage.insert(
        "decoded_feature_definition_count".to_string(),
        scan.features.definitions.len(),
    );
    coverage.insert(
        "decoded_feature_section_transform_count".to_string(),
        scan.features.section_transforms.len(),
    );
    coverage.insert(
        "decoded_feature_placement_instruction_count".to_string(),
        scan.features
            .definitions
            .iter()
            .map(|definition| crate::feature::placement_instructions(definition).len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_operation_state_count".to_string(),
        scan.features.operation_states.len(),
    );
    coverage.insert(
        "decoded_feature_operation_count".to_string(),
        scan.features.operations.len(),
    );
    coverage.insert(
        "decoded_feature_outline_count".to_string(),
        scan.features
            .definitions
            .iter()
            .map(|definition| definition.outlines.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_section_point_count".to_string(),
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
    coverage.insert(
        "decoded_feature_solver_variable_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.variables.as_ref())
            .map(|variables| variables.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "missing_feature_solver_variable_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.variables.as_ref())
            .map(|variables| {
                usize::try_from(variables.declared_count)
                    .expect("u32 variable count fits usize")
                    .saturating_sub(variables.rows.len())
            })
            .sum::<usize>(),
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
        .filter(|row| row.dimension_driven)
        .fold((0usize, 0usize), |(all, coordinates), row| {
            (
                all + 1,
                coordinates + usize::from(matches!(row.variable_type, 1 | 2)),
            )
        });
    let decoded_dimension_driven_guess_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.variables.as_ref())
        .flat_map(|variables| &variables.rows)
        .filter(|row| row.guess_dimension_driven)
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
            let resolved_coordinates = resolved_section_coordinates(definition);
            let resolved_radii = resolved_section_radii(definition);
            let resolved_scalars = resolved_section_scalar_values(definition);
            definition
                .variables
                .iter()
                .flat_map(|variables| &variables.rows)
                .filter(|row| row.dimension_driven)
                .fold(
                    (0usize, 0usize, 0usize),
                    |(all, coordinates, other), row| {
                        let resolved = match row.variable_type {
                            1 | 2 => resolved_coordinates
                                .get(&row.key)
                                .and_then(|point| point[usize::from(row.variable_type == 2)]),
                            3 => resolved_radii.get(&row.key).copied(),
                            _ => resolved_scalars.get(&(row.variable_type, row.key)).copied(),
                        };
                        (
                            all + usize::from(resolved.is_some()),
                            coordinates
                                + usize::from(
                                    matches!(row.variable_type, 1 | 2) && resolved.is_some(),
                                ),
                            other
                                + usize::from(
                                    !matches!(row.variable_type, 1 | 2) && resolved.is_some(),
                                ),
                        )
                    },
                )
        })
        .fold((0usize, 0usize, 0usize), |total, counts| {
            (total.0 + counts.0, total.1 + counts.1, total.2 + counts.2)
        });
    coverage.insert(
        "decoded_feature_dimension_driven_variable_count".to_string(),
        decoded_dimension_driven_variable_count,
    );
    coverage.insert(
        "decoded_feature_dimension_driven_coordinate_variable_count".to_string(),
        decoded_dimension_driven_coordinate_variable_count,
    );
    coverage.insert(
        "decoded_feature_dimension_driven_other_variable_count".to_string(),
        decoded_dimension_driven_variable_count
            .saturating_sub(decoded_dimension_driven_coordinate_variable_count),
    );
    coverage.insert(
        "decoded_feature_dimension_driven_guess_count".to_string(),
        decoded_dimension_driven_guess_count,
    );
    coverage.insert(
        "resolved_feature_dimension_driven_variable_count".to_string(),
        resolved_dimension_driven_variable_count,
    );
    coverage.insert(
        "resolved_feature_dimension_driven_coordinate_variable_count".to_string(),
        resolved_dimension_driven_coordinate_variable_count,
    );
    coverage.insert(
        "resolved_feature_dimension_driven_other_variable_count".to_string(),
        resolved_dimension_driven_other_variable_count,
    );
    coverage.insert(
        "unresolved_feature_dimension_driven_variable_count".to_string(),
        decoded_dimension_driven_variable_count
            .saturating_sub(resolved_dimension_driven_variable_count),
    );
    coverage.insert(
        "unresolved_feature_dimension_driven_coordinate_variable_count".to_string(),
        decoded_dimension_driven_coordinate_variable_count
            .saturating_sub(resolved_dimension_driven_coordinate_variable_count),
    );
    coverage.insert(
        "unresolved_feature_dimension_driven_other_variable_count".to_string(),
        decoded_dimension_driven_variable_count
            .saturating_sub(decoded_dimension_driven_coordinate_variable_count)
            .saturating_sub(resolved_dimension_driven_other_variable_count),
    );
    coverage.insert(
        "unresolved_feature_dimension_driven_guess_count".to_string(),
        decoded_dimension_driven_guess_count,
    );
    coverage.insert(
        "decoded_feature_circle_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.circle_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_point_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.point_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_centered_line_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.centered_line_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_reference_line_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.reference_line_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_bounded_curve_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.bounded_curve_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_conic_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.conic_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_opaque_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.opaque_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_trim_entity_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.trim_entities.as_ref())
            .map(|entities| entities.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_trim_vertex_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.trim_vertices.as_ref())
            .map(|vertices| vertices.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_order_entry_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.order_table.as_ref())
            .map(|order| order.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_dimension_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.dimensions.as_ref())
            .map(|dimensions| dimensions.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_relation_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.relations.as_ref())
            .map(|relations| relations.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_equation_table_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter(|definition| {
                crate::feature::equation_table(&definition.body, 0, definition.body.len()).is_some()
            })
            .count(),
    );
    coverage.insert(
        "decoded_feature_equation_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| {
                crate::feature::equation_table(&definition.body, 0, definition.body.len())
            })
            .map(|equations| equations.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_saved_entity_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.saved_section.as_ref())
            .map(|saved| saved.entities.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_saved_conic_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.saved_section.as_ref())
            .flat_map(|saved| &saved.entities)
            .filter(|entity| matches!(entity, crate::feature::FeatureSavedEntity::Conic(_)))
            .count(),
    );
    coverage.insert(
        "decoded_feature_entity_count".to_string(),
        scan.features.entities.len(),
    );
    coverage.insert(
        "decoded_feature_entity_reference_count".to_string(),
        scan.features.entity_references.len(),
    );
    coverage.insert(
        "decoded_feature_entity_table_count".to_string(),
        scan.features.entity_tables.len(),
    );
    coverage.insert(
        "decoded_feature_surface_replay_association_count".to_string(),
        feature_surface_replay_associations(scan).len(),
    );
    if let Some(count) = scan.framing.declared_body_count {
        attributes.insert("declared_body_count".to_string(), count.to_string());
    }
    if let Some(value) = scan.framing.first_quilt_ptr {
        attributes.insert("first_quilt_ptr".to_string(), value.to_string());
    }
    // The primary-layer match is the source of `dialect` and `declared`. The
    // `layout` attribute above stays exactly as it was: the duplication is
    // deliberate for this phase, and retiring the ad-hoc attribute keys is a
    // later one.
    let primary = crate::dialect::classify(scan);
    (
        SourceMeta {
            declared: primary.declared,
            dialect: primary.dialect,
            format: crate::dialect::FORMAT.to_string(),
            attributes,
        },
        coverage,
    )
}
