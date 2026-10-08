// SPDX-License-Identifier: Apache-2.0
//! Source metadata and structural decode-coverage census.

use std::collections::BTreeMap;

use crate::container::ContainerScan;
use crate::feature::definitions::{ScalarLane, VariableType};

use super::super::expanded::feature_surface_replay_association_count;
use super::super::sketch::coordinates::resolved_section_coordinates;
use super::super::sketch::equations_scalar::resolved_section_scalar_values;
use super::super::sketch::radii::resolved_section_radii;
use super::coverage::{legacy_numeric_coverage, surface_parameter_coverage, LegacyNumericCoverage};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::SourceMeta;

fn insert_source_attribute(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    node_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    attributes: &mut BTreeMap<String, String>,
    key: impl std::fmt::Display,
    value: impl std::fmt::Display,
) -> Result<(), cadmpeg_core::CodecError> {
    let key = ctx.format_retained(format_args!("{key}"), "Creo source attribute key")?;
    let value = ctx.format_retained(format_args!("{value}"), "Creo source attribute value")?;
    node_storage.with_storage(|| {
        ctx.insert_btree_map(attributes, key, value, "Creo source attribute map nodes")
    })?;
    Ok(())
}

pub(super) fn source_meta(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    classification: &crate::dialect::DialectClassification,
) -> Result<(SourceMeta, cadmpeg_ir::report::decode::Coverage), cadmpeg_core::CodecError> {
    let mut attribute_nodes = ctx.reserve_scoped(0, "Creo source attribute map nodes")?;
    let mut attributes = BTreeMap::new();
    let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
    insert_source_attribute(
        ctx,
        &mut attribute_nodes,
        &mut attributes,
        "version_line",
        &scan.framing.version_line,
    )?;
    if let Some(name) = &scan.framing.model_name {
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            "model_name",
            &name.name,
        )?;
    }
    if let Some(legacy) = scan.framing.layout.legacy_ascii() {
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            "legacy_ascii_schema",
            &legacy.schema,
        )?;
        if let Some(release) = &legacy.product_release {
            insert_source_attribute(
                ctx,
                &mut attribute_nodes,
                &mut attributes,
                "legacy_ascii_product_release",
                release,
            )?;
        }
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            "legacy_ascii_declaration_count",
            legacy.persistence.declaration_count(),
        )?;
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            "legacy_ascii_scope_count",
            legacy.persistence.scopes.len(),
        )?;
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            "legacy_ascii_value_count",
            legacy.persistence.value_count(),
        )?;
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            "legacy_ascii_continuation_count",
            legacy.persistence.continuation_count(),
        )?;
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            "legacy_ascii_unresolved_value_count",
            legacy.persistence.unresolved_value_count(),
        )?;
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            "legacy_ascii_conflicting_declaration_count",
            legacy.persistence.conflicting_declaration_count(),
        )?;
    }
    insert_source_attribute(
        ctx,
        &mut attribute_nodes,
        &mut attributes,
        "file_size",
        scan.framing.data.len(),
    )?;
    insert_source_attribute(
        ctx,
        &mut attribute_nodes,
        &mut attributes,
        "section_count",
        scan.framing.sections.len(),
    )?;
    for (index, section) in ctx
        .admit_iter(
            &scan.framing.sections,
            "creo source section attribute traversal",
        )?
        .enumerate()
    {
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            format_args!("section.{index}.name"),
            section.name(),
        )?;
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            format_args!("section.{index}.raw_name"),
            section.raw_name(),
        )?;
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            format_args!("section.{index}.role"),
            cadmpeg_core::container::ContainerRole::from(section.role()),
        )?;
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            format_args!("section.{index}.offset"),
            section.offset(),
        )?;
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            format_args!("section.{index}.length"),
            section.length(),
        )?;
    }
    if let Some(c) = scan.framing.census.srf_array_count {
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            "srf_array_count",
            c,
        )?;
    }
    if let Some(c) = scan.framing.census.crv_array_count {
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            "crv_array_count",
            c,
        )?;
    }
    if let Some(unit) = &scan.framing.principal_unit {
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            "principal_unit",
            unit,
        )?;
        if let Some(scale) = unit.length_scale_mm().filter(|scale| scale.get() != 1.0) {
            insert_source_attribute(
                ctx,
                &mut attribute_nodes,
                &mut attributes,
                "source_length_scale_mm",
                scale.get(),
            )?;
        }
    }
    if let Some(legacy) = scan.framing.layout.legacy_ascii() {
        coverage.record(
            ctx,
            crate::coverage::DECODED_LEGACY_PRINCIPAL_UNIT_COUNT,
            usize::from(scan.framing.principal_unit.is_some()),
        )?;
        let mut object_arrows = 0usize;
        let mut object_inlines = 0usize;
        let mut object_nulls = 0usize;
        let mut object_arrays = 0usize;
        for record in ctx.admit_iter(
            &legacy.persistence.objects,
            "creo legacy object coverage traversal",
        )? {
            match record.payload {
                crate::legacy::ObjectPayload::Arrow => object_arrows += 1,
                crate::legacy::ObjectPayload::Inline => object_inlines += 1,
                crate::legacy::ObjectPayload::Null => object_nulls += 1,
                crate::legacy::ObjectPayload::Array { .. } => object_arrays += 1,
                crate::legacy::ObjectPayload::Opaque { .. } => {}
            }
        }
        coverage.record(
            ctx,
            crate::coverage::DECODED_LEGACY_OBJECT_ARROW_COUNT,
            object_arrows,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_LEGACY_OBJECT_INLINE_COUNT,
            object_inlines,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_LEGACY_OBJECT_NULL_COUNT,
            object_nulls,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_LEGACY_OBJECT_ARRAY_COUNT,
            object_arrays,
        )?;
        coverage.record(
            ctx,
            crate::coverage::INCOMPLETE_LEGACY_OBJECT_ARRAY_COUNT,
            legacy.persistence.incomplete_object_array_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::UNRESOLVED_LEGACY_OBJECT_VALUE_COUNT,
            legacy.persistence.unresolved_object_value_count,
        )?;
        let integer_counts = legacy_numeric_coverage(ctx, &legacy.persistence.integer_values.rows)?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_LEGACY_INTEGER_SCALAR_COUNT,
            integer_counts.scalars,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_LEGACY_INTEGER_ARRAY_COUNT,
            integer_counts.arrays,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_LEGACY_INTEGER_ELEMENT_COUNT,
            integer_counts.elements,
        )?;
        coverage.record(
            ctx,
            crate::coverage::UNRESOLVED_LEGACY_INTEGER_VALUE_COUNT,
            legacy.persistence.integer_values.unresolved_count,
        )?;
        let real_counts = legacy_numeric_coverage(ctx, &legacy.persistence.real_values.rows)?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_LEGACY_REAL_SCALAR_COUNT,
            real_counts.scalars,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_LEGACY_REAL_ARRAY_COUNT,
            real_counts.arrays,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_LEGACY_REAL_ELEMENT_COUNT,
            real_counts.elements,
        )?;
        coverage.record(
            ctx,
            crate::coverage::UNRESOLVED_LEGACY_REAL_VALUE_COUNT,
            legacy.persistence.real_values.unresolved_count,
        )?;
        let (string_scalars, string_arrays, string_elements, undecoded_encodings) = ctx
            .admit_iter(
                &legacy.persistence.string_values,
                "creo legacy string coverage traversal",
            )?
            .try_fold(
                (0usize, 0usize, 0usize, 0usize),
                |(scalars, arrays, elements, undecoded_encodings),
                 record|
                 -> Result<_, CodecError> {
                    let (element_count, encoding_count) = match &record.payload {
                        crate::legacy::StringPayload::Scalar { value } => (1usize, value.undecoded_encoding_count()),
                        crate::legacy::StringPayload::Array { values, .. } => {
                            let mut elements = 0usize;
                            let mut encodings = 0usize;
                            for value in ctx.admit_iter(values, "creo legacy string element count")?.filter_map(|value| value.as_ref().ok()) {
                                elements += 1;
                                encodings += value.undecoded_encoding_count();
                            }
                            (elements, encodings)
                        }
                    };
                    Ok((
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
                        elements
                            .checked_add(element_count)
                            .ok_or_else(|| {
                                ctx.refuse_codec_limit(
                                    "creo legacy string element count",
                                    u64::MAX,
                                    u64::MAX,
                                )
                            })?,
                        undecoded_encodings
                            .checked_add(encoding_count)
                            .ok_or_else(|| {
                                ctx.refuse_codec_limit(
                                    "creo legacy string encoding count",
                                    u64::MAX,
                                    u64::MAX,
                                )
                            })?,
                    ))
                },
            )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_LEGACY_STRING_SCALAR_COUNT,
            string_scalars,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_LEGACY_STRING_ARRAY_COUNT,
            string_arrays,
        )?;
        coverage.record(
            ctx,
            crate::coverage::DECODED_LEGACY_STRING_ELEMENT_COUNT,
            string_elements,
        )?;
        coverage.record(
            ctx,
            crate::coverage::INCOMPLETE_LEGACY_STRING_ARRAY_COUNT,
            legacy.persistence.incomplete_string_array_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::UNRESOLVED_LEGACY_STRING_VALUE_COUNT,
            legacy.persistence.unresolved_string_value_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::UNDECODED_LEGACY_STRING_ENCODING_COUNT,
            undecoded_encodings,
        )?;
        record_scalar_string_coverage(
            ctx,
            &mut coverage,
            crate::coverage::DECODED_LEGACY_TYPE_3_SCALAR_COUNT,
            crate::coverage::UNRESOLVED_LEGACY_TYPE_3_VALUE_COUNT,
            crate::coverage::UNDECODED_LEGACY_TYPE_3_ENCODING_COUNT,
            &legacy.persistence.type_3_values,
        )?;
        record_scalar_string_coverage(
            ctx,
            &mut coverage,
            crate::coverage::DECODED_LEGACY_TYPE_4_SCALAR_COUNT,
            crate::coverage::UNRESOLVED_LEGACY_TYPE_4_VALUE_COUNT,
            crate::coverage::UNDECODED_LEGACY_TYPE_4_ENCODING_COUNT,
            &legacy.persistence.type_4_values,
        )?;
        let mut insert_numbered_numeric_coverage =
            |scalar_key,
             array_key,
             element_key,
             unresolved_key,
             counts: LegacyNumericCoverage,
             unresolved|
             -> Result<(), cadmpeg_core::CodecError> {
                coverage.record(ctx, scalar_key, counts.scalars)?;
                coverage.record(ctx, array_key, counts.arrays)?;
                coverage.record(ctx, element_key, counts.elements)?;
                coverage.record(ctx, unresolved_key, unresolved)?;
                Ok(())
            };
        insert_numbered_numeric_coverage(
            crate::coverage::DECODED_LEGACY_TYPE_5_SCALAR_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_5_ARRAY_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_5_ELEMENT_COUNT,
            crate::coverage::UNRESOLVED_LEGACY_TYPE_5_VALUE_COUNT,
            legacy_numeric_coverage(ctx, &legacy.persistence.type_5_values.rows)?,
            legacy.persistence.type_5_values.unresolved_count,
        )?;
        insert_numbered_numeric_coverage(
            crate::coverage::DECODED_LEGACY_TYPE_6_SCALAR_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_6_ARRAY_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_6_ELEMENT_COUNT,
            crate::coverage::UNRESOLVED_LEGACY_TYPE_6_VALUE_COUNT,
            legacy_numeric_coverage(ctx, &legacy.persistence.type_6_values.rows)?,
            legacy.persistence.type_6_values.unresolved_count,
        )?;
        insert_numbered_numeric_coverage(
            crate::coverage::DECODED_LEGACY_TYPE_7_SCALAR_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_7_ARRAY_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_7_ELEMENT_COUNT,
            crate::coverage::UNRESOLVED_LEGACY_TYPE_7_VALUE_COUNT,
            legacy_numeric_coverage(ctx, &legacy.persistence.type_7_values.rows)?,
            legacy.persistence.type_7_values.unresolved_count,
        )?;
        insert_numbered_numeric_coverage(
            crate::coverage::DECODED_LEGACY_TYPE_9_SCALAR_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_9_ARRAY_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_9_ELEMENT_COUNT,
            crate::coverage::UNRESOLVED_LEGACY_TYPE_9_VALUE_COUNT,
            legacy_numeric_coverage(ctx, &legacy.persistence.type_9_values.rows)?,
            legacy.persistence.type_9_values.unresolved_count,
        )?;
        insert_numbered_numeric_coverage(
            crate::coverage::DECODED_LEGACY_TYPE_11_SCALAR_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_11_ARRAY_COUNT,
            crate::coverage::DECODED_LEGACY_TYPE_11_ELEMENT_COUNT,
            crate::coverage::UNRESOLVED_LEGACY_TYPE_11_VALUE_COUNT,
            legacy_numeric_coverage(ctx, &legacy.persistence.type_11_values.rows)?,
            legacy.persistence.type_11_values.unresolved_count,
        )?;
    }
    coverage.record(
        ctx,
        crate::coverage::DECODED_PRIMITIVE_TRIANGLE_STRIP_COUNT,
        scan.primitives.triangle_strips.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::CONFLICTING_PRIMITIVE_TRIANGLE_STRIP_REPRESENTATION_COUNT,
        scan.primitives
            .conflicting_triangle_strip_representation_count,
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_SURFACE_ROW_COUNT,
        scan.surfaces.rows.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_CROSS_SECTION_SURFACE_ROW_COUNT,
        scan.surfaces.cross_section_rows.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_SURFACE_PARAMETER_RECORD_COUNT,
        scan.surfaces.parameters.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_CROSS_SECTION_SURFACE_PARAMETER_RECORD_COUNT,
        scan.surfaces.cross_section_parameters.len(),
    )?;
    let parameter_coverage = surface_parameter_coverage(ctx, scan)?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_POSITIONAL_EXTRUSION_DIRECTION_COUNT,
        parameter_coverage.extrusion_directions,
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_TORUS_RADIUS_OVERRIDE_COUNT,
        parameter_coverage.radius_overrides,
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_TYPE26_REPLAYED_MINOR_RADIUS_COUNT,
        parameter_coverage.replayed_minor_radii,
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_TORUS_OUTLINE_EXTENT_COUNT,
        parameter_coverage.outline_extents,
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_TYPE26_FIVE_COORDINATE_ENVELOPE_COUNT,
        parameter_coverage.five_coordinate_envelopes,
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_TYPE26_SPLIT_COORDINATE_ENVELOPE_COUNT,
        parameter_coverage.split_coordinate_envelopes,
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_PLANE_LOCAL_SYSTEM_COUNT,
        scan.planes.local_systems.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_CROSS_SECTION_PLANE_LOCAL_SYSTEM_COUNT,
        scan.planes.cross_section_local_systems.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_PLANE_ENVELOPE_COUNT,
        scan.planes.envelopes.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_CROSS_SECTION_PLANE_ENVELOPE_COUNT,
        scan.planes.cross_section_envelopes.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_OUTLINE_PLANE_COUNT,
        scan.planes.outlines.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_POSITIONAL_FRAME_PLANE_COUNT,
        scan.planes.positional_frames.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_CROSS_SECTION_OUTLINE_PLANE_COUNT,
        scan.planes.cross_section_outlines.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_SURFACE_PROTOTYPE_COUNT,
        scan.surfaces.prototype_count,
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_NAMED_SURFACE_PROTOTYPE_COUNT,
        scan.surfaces.prototype_records.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_REFERENCE_LINE_COUNT,
        scan.references.lines.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_REFERENCE_CIRCLE_COUNT,
        scan.references.circles.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_REFERENCE_CONIC_COUNT,
        scan.references.conics.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::TRANSFERRED_REFERENCE_ELLIPSE_COUNT,
        scan.references.ellipses.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_TABULATED_CYLINDER_CURVE_REPLAY_COUNT,
        scan.curves.tabulated_cylinder_replays.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_TABULATED_CYLINDER_CONTROL_POINT_SET_COUNT,
        ctx.admit_iter(
            &scan.curves.tabulated_cylinder_replays,
            "creo tabulated_cylinder_replays coverage traversal",
        )?
        .filter(|replay| replay.control_points.iter().all(Option::is_some))
        .count(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_CURVE_PROTOTYPE_COUNT,
        scan.curves.prototypes.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_CURVE_PARAMETER_RECORD_COUNT,
        scan.curves.parameters.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_CURVE_EXPRESSION_RECORD_COUNT,
        scan.curves.expressions.len(),
    )?;
    insert_source_attribute(
        ctx,
        &mut attribute_nodes,
        &mut attributes,
        "expanded_section_count",
        scan.framing.expanded_sections.len(),
    )?;
    insert_source_attribute(
        ctx,
        &mut attribute_nodes,
        &mut attributes,
        "expanded_section_byte_count",
        ctx.admit_iter(
            &scan.framing.expanded_sections,
            "creo expanded_sections coverage traversal",
        )?
        .map(|section| section.data.len())
        .sum::<usize>(),
    )?;
    if let Some(family_table) = scan.framing.family_table {
        match family_table.pointer {
            crate::container::FamilyTablePointer::Null => {
                insert_source_attribute(
                    ctx,
                    &mut attribute_nodes,
                    &mut attributes,
                    "family_table_pointer",
                    "null",
                )?;
                insert_source_attribute(
                    ctx,
                    &mut attribute_nodes,
                    &mut attributes,
                    "configuration_state",
                    "none",
                )?;
            }
            crate::container::FamilyTablePointer::Entity(id) => {
                insert_source_attribute(
                    ctx,
                    &mut attribute_nodes,
                    &mut attributes,
                    "family_table_pointer",
                    format_args!("entity:{id}"),
                )?;
                insert_source_attribute(
                    ctx,
                    &mut attribute_nodes,
                    &mut attributes,
                    "configuration_state",
                    "driver_table_unresolved",
                )?;
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
        ctx,
        crate::coverage::DECODED_CONFIGURATION_DRIVER_TABLE_REFERENCE_COUNT,
        configuration_driver_table_reference_count,
    )?;
    let legacy_family_table = scan.framing.legacy_family_table.as_ref();
    coverage.record(
        ctx,
        crate::coverage::DECODED_LEGACY_CONFIGURATION_DRIVER_TABLE_COUNT,
        usize::from(legacy_family_table.is_some()),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_LEGACY_CONFIGURATION_ITEM_COUNT,
        legacy_family_table.map_or(0, |table| table.items.len()),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_LEGACY_CONFIGURATION_INSTANCE_COUNT,
        legacy_family_table.map_or(0, |table| table.instances.len()),
    )?;
    coverage.record(
        ctx,
        crate::coverage::TRANSFERRED_CONFIGURATION_DRIVER_TABLE_COUNT,
        0,
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_PCURVE_COUNT,
        scan.curves.pcurves.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_TWO_CHART_PCURVE_COUNT,
        scan.curves.two_chart_pcurves.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FC_CURVE_COORDINATE_RECORD_COUNT,
        scan.curves.fc_coordinates.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FC05_CIRCLE_COUNT,
        scan.curves.fc05_circles.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FC05_CYLINDER_CAP_PAIR_COUNT,
        scan.curves.fc05_cylinder_cap_pairs.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_PROTOTYPE_PCURVE_COUNT,
        scan.curves.prototype_pcurves.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_CURVE_PROTOTYPE_TOPOLOGY_COUNT,
        scan.curves.prototype_topology.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_BOUND_PROTOTYPE_PCURVE_COUNT,
        scan.curves.bound_prototype_pcurves.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_CURVE_TOPOLOGY_ROW_COUNT,
        scan.curves.topology_rows.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_CROSS_SECTION_CURVE_ROW_COUNT,
        scan.curves.cross_section_rows.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_CROSS_SECTION_CURVE_PROTOTYPE_COUNT,
        scan.curves.cross_section_prototypes.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_HALF_EDGE_COUNT,
        scan.topology.half_edges.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_TOPOLOGICAL_VERTEX_COUNT,
        scan.topology.vertices.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_LOOP_COUNT,
        scan.topology.loops.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FACE_COMPONENT_COUNT,
        scan.topology.face_components.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_DATUM_PLANE_COUNT,
        scan.planes.datums.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_COUNT,
        scan.features.ids.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_ROW_COUNT,
        scan.features.rows.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_CHOICE_COUNT,
        scan.features.choices.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_CHOICE_FIELD_COUNT,
        scan.features.choice_fields.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_GEOMETRY_TABLE_COUNT,
        scan.features.geometry_tables.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_LOOP_HISTORY_ENTRY_COUNT,
        scan.features.loop_history_entries.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_AFFECTED_ID_ARRAY_COUNT,
        scan.features.affected_ids.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_REPLAY_AFFECTED_ID_COUNT,
        scan.features.replay_affected_ids.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_SURFACE_MERGE_REPLAY_AFFECTED_ID_COUNT,
        scan.features.surface_merge_replay_affected_ids.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_LOOP_RESTORE_DIRECTION_COUNT,
        scan.features.loop_restore_directions.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_REVOLUTION_EXTENT_COUNT,
        scan.features.revolution_extents.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_DEFINITION_COUNT,
        scan.features.definitions.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_SECTION_TRANSFORM_COUNT,
        scan.features.section_transforms.len(),
    )?;
    let mut placement_instruction_count = 0usize;
    let mut outline_count = 0usize;
    let mut section_point_count = 0usize;
    let mut solver_variable_count = 0usize;
    let mut missing_feature_solver_variable_count = 0usize;
    let mut decoded_dimension_driven_variable_count = 0usize;
    let mut decoded_dimension_driven_coordinate_variable_count = 0usize;
    let mut decoded_dimension_driven_guess_count = 0usize;
    let mut resolved_dimension_driven_variable_count = 0usize;
    let mut resolved_dimension_driven_coordinate_variable_count = 0usize;
    let mut resolved_dimension_driven_other_variable_count = 0usize;
    let mut trim_entity_count = 0usize;
    let mut trim_vertex_count = 0usize;
    let mut order_entry_count = 0usize;
    let mut dimension_count = 0usize;
    let mut relation_count = 0usize;
    let mut equation_table_count = 0usize;
    let mut equation_count = 0usize;
    let mut saved_entity_count = 0usize;
    let mut saved_conic_count = 0usize;
    let mut segment_counts = [0usize; 7];
    for definition in ctx.admit_iter(&scan.features.definitions, "creo feature definition coverage traversal")? {
        let mut resolver_storage = ctx.reserve_scoped(0, "creo definition coverage resolver storage")?;
        let placement_count = crate::feature::definitions::placement_instructions(ctx, definition)?.count();
        placement_instruction_count = placement_instruction_count.checked_add(placement_count).ok_or_else(|| CodecError::malformed("Creo placement instruction count overflow"))?;
        outline_count += definition.outlines.len();
        if let Some(variables) = &definition.variables {
            let crate::feature::definitions::ReconciledPoints { points, ambiguous } = resolver_storage.with_storage(|| variables.reconciled_points(ctx))?;
            section_point_count += points.len() + ambiguous.len();
            solver_variable_count += variables.rows.len();
            let declared_count = usize::try_from(variables.declared_count).map_err(|_| CodecError::malformed("feature solver variable count does not fit the host index type"))?;
            missing_feature_solver_variable_count += declared_count.checked_sub(variables.rows.len()).ok_or_else(|| CodecError::malformed("feature solver variable rows exceed the declared variable count"))?;
            let resolved_coordinates = resolver_storage.with_storage(|| resolved_section_coordinates(ctx, definition))?;
            let resolved_radii = resolver_storage.with_storage(|| resolved_section_radii(ctx, definition))?;
            let resolved_scalars = resolver_storage.with_storage(|| resolved_section_scalar_values(ctx, definition))?;
            for row in ctx.admit_iter(&variables.rows, "creo dimension-driven variable coverage traversal")? {
                decoded_dimension_driven_guess_count = decoded_dimension_driven_guess_count.checked_add(usize::from(row.guess == ScalarLane::DimensionDriven)).ok_or_else(|| cadmpeg_core::decode::refuse_local_limit("creo dimension-driven guess count", u64::MAX, u64::MAX))?;
                if row.value != ScalarLane::DimensionDriven { continue; }
                let coordinate = matches!(row.variable_type, VariableType::U | VariableType::V);
                decoded_dimension_driven_variable_count = decoded_dimension_driven_variable_count.checked_add(1).ok_or_else(|| cadmpeg_core::decode::refuse_local_limit("creo dimension-driven coverage count", u64::MAX, u64::MAX))?;
                decoded_dimension_driven_coordinate_variable_count = decoded_dimension_driven_coordinate_variable_count.checked_add(usize::from(coordinate)).ok_or_else(|| cadmpeg_core::decode::refuse_local_limit("creo dimension-driven coverage count", u64::MAX, u64::MAX))?;
                let resolved = match row.variable_type {
                    VariableType::U | VariableType::V => ctx.get_btree_map(&resolved_coordinates, &row.key, "creo resolved coordinate coverage lookup")?.and_then(|point| point[usize::from(row.variable_type == VariableType::V)]),
                    VariableType::Radius => ctx.get_btree_map(&resolved_radii, &row.key, "creo resolved radius coverage lookup")?.copied(),
                    _ => ctx.get_btree_map(&resolved_scalars, &(row.variable_type, row.key), "creo resolved scalar coverage lookup")?.copied(),
                };
                for (total, counted) in [
                    (&mut resolved_dimension_driven_variable_count, resolved.is_some()),
                    (&mut resolved_dimension_driven_coordinate_variable_count, coordinate && resolved.is_some()),
                    (&mut resolved_dimension_driven_other_variable_count, !coordinate && resolved.is_some()),
                ] {
                    *total = total.checked_add(usize::from(counted)).ok_or_else(|| cadmpeg_core::decode::refuse_local_limit("creo resolved dimension-driven count", u64::MAX, u64::MAX))?;
                }
            }
        }
        if let Some(segments) = &definition.segments {
            for row in ctx.admit_iter(segments.rows.as_slice(), "creo segment coverage rows")? {
                use crate::feature::segment_rows::SegmentRow;
                let family = match row {
                    SegmentRow::Circle(_) => Some(0),
                    SegmentRow::Point(_) => Some(1),
                    SegmentRow::CenteredLine(_) => Some(2),
                    SegmentRow::ReferenceLine(_) => Some(3),
                    SegmentRow::BoundedCurve(_) => Some(4),
                    SegmentRow::Conic(_) => Some(5),
                    SegmentRow::Opaque(_) => Some(6),
                    _ => None,
                };
                if let Some(index) = family {
                    segment_counts[index] = segment_counts[index].checked_add(1).ok_or_else(|| ctx.refuse_codec_limit("creo segment coverage count", u64::MAX, u64::MAX))?;
                }
            }
        }
        trim_entity_count += definition.trim_entities.as_ref().map_or(0, |table| table.rows.len());
        trim_vertex_count += definition.trim_vertices.as_ref().map_or(0, |table| table.rows.len());
        order_entry_count += definition.order_table.as_ref().map_or(0, |table| table.rows.len());
        dimension_count += definition.dimensions.as_ref().map_or(0, |table| table.rows.len());
        relation_count += definition.relations.as_ref().map_or(0, |table| table.rows.len());
        let mut equation_storage = ctx.reserve_scoped(0, "creo equation coverage table storage")?;
        if let Some(equations) = equation_storage.with_storage(|| crate::feature::definitions::equation_table(ctx, &definition.body, 0, definition.body.len()))? {
            equation_table_count += 1;
            equation_count += equations.rows.len();
        }
        if let Some(saved) = &definition.saved_section {
            saved_entity_count += saved.entities.len();
            let count = ctx.admit_iter(&saved.entities, "creo saved conic coverage traversal")?.filter(|entity| matches!(entity, crate::feature::definitions::FeatureSavedEntity::Conic(_))).count();
            saved_conic_count = saved_conic_count.checked_add(count).ok_or_else(|| cadmpeg_core::decode::refuse_local_limit("creo saved conic coverage count", u64::MAX, u64::MAX))?;
        }
    }
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_PLACEMENT_INSTRUCTION_COUNT, placement_instruction_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_OPERATION_STATE_COUNT, scan.features.operation_states.len())?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_OPERATION_COUNT, scan.features.operations.len())?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_OUTLINE_COUNT, outline_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_SECTION_POINT_COUNT, section_point_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_SOLVER_VARIABLE_COUNT, solver_variable_count)?;
    coverage.record(ctx, crate::coverage::MISSING_FEATURE_SOLVER_VARIABLE_COUNT, missing_feature_solver_variable_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT, decoded_dimension_driven_variable_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT, decoded_dimension_driven_coordinate_variable_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT, decoded_dimension_driven_variable_count.checked_sub(decoded_dimension_driven_coordinate_variable_count).ok_or_else(|| CodecError::malformed("resolved dimension count exceeds decoded count"))?)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_DIMENSION_DRIVEN_GUESS_COUNT, decoded_dimension_driven_guess_count)?;
    coverage.record(ctx, crate::coverage::RESOLVED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT, resolved_dimension_driven_variable_count)?;
    coverage.record(ctx, crate::coverage::RESOLVED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT, resolved_dimension_driven_coordinate_variable_count)?;
    coverage.record(ctx, crate::coverage::RESOLVED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT, resolved_dimension_driven_other_variable_count)?;
    coverage.record(ctx, crate::coverage::UNRESOLVED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT, decoded_dimension_driven_variable_count.checked_sub(resolved_dimension_driven_variable_count).ok_or_else(|| CodecError::malformed("resolved dimension count exceeds decoded count"))?)?;
    coverage.record(ctx, crate::coverage::UNRESOLVED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT, decoded_dimension_driven_coordinate_variable_count.checked_sub(resolved_dimension_driven_coordinate_variable_count).ok_or_else(|| CodecError::malformed("resolved dimension count exceeds decoded count"))?)?;
    coverage.record(ctx, crate::coverage::UNRESOLVED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT, decoded_dimension_driven_variable_count.checked_sub(decoded_dimension_driven_coordinate_variable_count).and_then(|count| count.checked_sub(resolved_dimension_driven_other_variable_count)).ok_or_else(|| CodecError::malformed("resolved dimension count exceeds decoded count"))?)?;
    coverage.record(ctx, crate::coverage::UNRESOLVED_FEATURE_DIMENSION_DRIVEN_GUESS_COUNT, decoded_dimension_driven_guess_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_CIRCLE_SEGMENT_COUNT, segment_counts[0])?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_POINT_SEGMENT_COUNT, segment_counts[1])?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_CENTERED_LINE_SEGMENT_COUNT, segment_counts[2])?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_REFERENCE_LINE_SEGMENT_COUNT, segment_counts[3])?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_BOUNDED_CURVE_SEGMENT_COUNT, segment_counts[4])?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_CONIC_SEGMENT_COUNT, segment_counts[5])?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_OPAQUE_SEGMENT_COUNT, segment_counts[6])?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_TRIM_ENTITY_COUNT, trim_entity_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_TRIM_VERTEX_COUNT, trim_vertex_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_ORDER_ENTRY_COUNT, order_entry_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_DIMENSION_COUNT, dimension_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_RELATION_COUNT, relation_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_EQUATION_TABLE_COUNT, equation_table_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_EQUATION_COUNT, equation_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_SAVED_ENTITY_COUNT, saved_entity_count)?;
    coverage.record(ctx, crate::coverage::DECODED_FEATURE_SAVED_CONIC_COUNT, saved_conic_count)?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_ENTITY_COUNT,
        scan.features.entities.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_ENTITY_REFERENCE_COUNT,
        scan.features.entity_references.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_ENTITY_TABLE_COUNT,
        scan.features.entity_tables.len(),
    )?;
    coverage.record(
        ctx,
        crate::coverage::DECODED_FEATURE_SURFACE_REPLAY_ASSOCIATION_COUNT,
        feature_surface_replay_association_count(ctx, scan)?,
    )?;
    if let Some(count) = scan.framing.declared_body_count {
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            "declared_body_count",
            count,
        )?;
    }
    if let Some(value) = scan.framing.first_quilt_ptr {
        insert_source_attribute(
            ctx,
            &mut attribute_nodes,
            &mut attributes,
            "first_quilt_ptr",
            value,
        )?;
    }
    Ok((
        SourceMeta::classified(
            DialectLayers::of(
                classification
                    .matched()
                    .try_clone_for_decode(ctx, "creo source dialect copy")?,
            ),
            {
                let attributes = cadmpeg_core::text::named_entries_for_decode(
                    ctx,
                    "the creo container",
                    attributes,
                )?;
                drop(attribute_nodes);
                attributes
            },
        ),
        coverage,
    ))
}

fn record_scalar_string_coverage<K>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    coverage: &mut cadmpeg_ir::report::decode::Coverage,
    scalar_key: cadmpeg_ir::report::decode::CoverageKey,
    unresolved_key: cadmpeg_ir::report::decode::CoverageKey,
    undecoded_key: cadmpeg_ir::report::decode::CoverageKey,
    values: &crate::legacy::TypedValues<crate::legacy::ValueRecord<K>>,
) -> Result<(), cadmpeg_core::CodecError>
where
    K: crate::legacy::LegacyCode<Payload = crate::legacy::StringValue>,
{
    let undecoded_encodings = ctx
        .admit_iter(&values.rows, "creo rows coverage traversal")?
        .map(|record| record.payload.undecoded_encoding_count())
        .sum();
    coverage.record(ctx, scalar_key, values.rows.len())?;
    coverage.record(ctx, unresolved_key, values.unresolved_count)?;
    coverage.record(ctx, undecoded_key, undecoded_encodings)?;
    Ok(())
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
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let mut node_storage = ctx
            .reserve_scoped(0, "Creo source attribute map nodes")
            .expect("source attribute lease");
        let mut attributes = BTreeMap::new();
        let error =
            insert_source_attribute(&ctx, &mut node_storage, &mut attributes, "file_size", 12)
                .expect_err("one source attribute needs one map node");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "Creo source attribute map nodes")
        );
        assert!(attributes.is_empty());
    }

    #[test]
    fn source_attribute_map_keeps_rendered_key_value_and_order() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let mut node_storage = ctx
            .reserve_scoped(0, "Creo source attribute map nodes")
            .expect("source attribute lease");
        let mut attributes = BTreeMap::new();
        insert_source_attribute(
            &ctx,
            &mut node_storage,
            &mut attributes,
            "version_line",
            "Creo 10",
        )
        .expect("first attribute is admitted");
        insert_source_attribute(
            &ctx,
            &mut node_storage,
            &mut attributes,
            format_args!("section.{}.name", 0),
            "MdlStatus",
        )
        .expect("section attribute is admitted");
        assert_eq!(attributes["version_line"], "Creo 10");
        assert_eq!(attributes["section.0.name"], "MdlStatus");
    }

    #[test]
    fn principal_unit_attribute_refuses_before_retaining_token() {
        let error = crate::test_support::last_refusal_at(
            &[], ResourceDimension::RetainedBytes, "Creo source attribute value",
            |ctx| {
                let mut node_storage = ctx.reserve_scoped(0, "Creo source attribute map nodes")?;
                let mut attributes = BTreeMap::new();
                let result = insert_source_attribute(ctx, &mut node_storage, &mut attributes, "principal_unit", "unknown:7");
                if result.is_err() { assert!(attributes.is_empty()); }
                result
            },
        );
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "Creo source attribute value"));
    }

    #[test]
    fn source_attribute_staging_node_refuses_materialized_storage() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let mut node_storage = ctx
            .reserve_scoped(0, "Creo source attribute map nodes")
            .expect("source attribute lease");
        let mut attributes = BTreeMap::new();
        let error =
            insert_source_attribute(&ctx, &mut node_storage, &mut attributes, "file_size", 12)
                .expect_err("one staging node exceeds materialized storage");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "Creo source attribute map nodes")
        );
    }

    #[test]
    fn source_attribute_named_output_node_remains_retained() {
        let error = crate::test_support::last_refusal_at(
            &[], ResourceDimension::RetainedBytes, "named entry map nodes",
            |ctx| {
                let mut node_storage = ctx.reserve_scoped(0, "Creo source attribute map nodes")?;
                let mut attributes = BTreeMap::new();
                insert_source_attribute(ctx, &mut node_storage, &mut attributes, "file_size", 12)?;
                cadmpeg_core::text::named_entries_for_decode(ctx, "source", attributes).map_err(cadmpeg_core::CodecError::from)
            },
        );
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes && resource.operation == "named entry map nodes"));
    }

    #[test]
    fn source_attribute_named_entry_service_preserves_order() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let mut node_storage = ctx
            .reserve_scoped(0, "Creo source attribute map nodes")
            .expect("source attribute lease");
        let mut attributes = BTreeMap::new();
        insert_source_attribute(&ctx, &mut node_storage, &mut attributes, "z", 1)
            .expect("first attribute fits");
        insert_source_attribute(&ctx, &mut node_storage, &mut attributes, "a", 2)
            .expect("second attribute fits");
        let entries = cadmpeg_core::text::named_entries_for_decode(&ctx, "source", attributes)
            .expect("valid attribute keys are admitted");
        drop(node_storage);
        assert_eq!(
            entries
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect::<Vec<_>>(),
            vec![("a", "2"), ("z", "1")]
        );
    }
    #[test]
    fn segment_coverage_counts_admit_each_original_mixed_row_source() {
        use crate::feature::definitions::{
            DefinitionIdentity, FeatureCircleSegment, FeatureDefinition, FeatureSegmentTable,
        };
        use crate::feature::segment_rows::SegmentRow;
        let mut scan = crate::test_support::empty_container_scan();
        scan.features.definitions.push(FeatureDefinition {
            identity: DefinitionIdentity::Parsed {
                schema_id: None,
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(FeatureSegmentTable {
                declared_count: 1,
                has_elided_prototype: false,
                entity_ref: None,
                rows: [SegmentRow::Circle(FeatureCircleSegment {
                    center_id: 1,
                    radius_ref: 2,
                    external_id: 3,
                    offset: 0,
                })]
                .into_iter()
                .collect(),
                offset: 0,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        });
        let classification =
            crate::decode::with_test_decode_ctx(|ctx| crate::dialect::classify(ctx, &scan))
                .expect("classified source");
        crate::test_support::assert_work_boundaries(
            &["creo segment coverage rows"],
            |ctx| super::source_meta(ctx, &scan, &classification),
        );
        let rows = &scan.features.definitions[0]
            .segments
            .as_ref()
            .expect("table")
            .rows;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.circles().count(), 1);
        assert_eq!(rows.points().count(), 0);
    }
}
