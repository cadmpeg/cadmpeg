// SPDX-License-Identifier: Apache-2.0
//! IR-writing attachment of the native object model.
//!
//! This module is the sole IR-mutation surface inside `native/`: it walks the
//! extracted [`NativeModel`], emits source annotations in the legacy note order,
//! serializes each record family into an `nx` namespace arena, and attaches the
//! semantic islands (tessellations, source attributes, feature operations). The
//! IR-free domain modules, `model.rs`, and `catalogue.rs` never write IR.

use std::collections::BTreeMap;

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    ConfigurationBodies, ConfigurationId, DesignConfiguration, Feature, FeatureId,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{AttributeId, BodyId, UnknownId};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

#[allow(clippy::wildcard_imports)]
use crate::decode::*;

pub(crate) fn attach(
    ir: &mut CadIr,
    model: &crate::native::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let object_sections = scan.container.indexed_om_sections();
    if model.is_empty() && object_sections.is_empty() {
        return Ok(());
    }
    let display_jt_tessellations = display_jt_tessellations(&DisplayJtTessellationInputs {
        meshes: &model.display_jt.display_jt_polygon_meshes,
        coordinates: &model.display_jt.display_jt_vertex_coordinates,
        normals: &model.display_jt.display_jt_vertex_normals,
        colors: &model.display_jt.display_jt_vertex_colors,
        texture_coordinates: &model.display_jt.display_jt_vertex_texture_coordinates,
        vertex_flags: &model.display_jt.display_jt_vertex_flags,
        vertex_headers: &model.display_jt.display_jt_vertex_records_headers,
        coordinate_headers: &model.display_jt.display_jt_coordinate_array_headers,
        shape_elements: &model.display_jt.display_jt_shape_lod_elements,
        bindings: &model.display_jt.display_jt_shape_lod_bindings,
        shape_nodes: &model.display_jt.display_jt_tri_strip_shape_nodes,
        base_nodes: &model.display_jt.display_jt_base_node_data,
        group_nodes: &model.display_jt.display_jt_group_node_data,
        instance_nodes: &model.display_jt.display_jt_instance_nodes,
        transforms: &model.display_jt.display_jt_geometric_transform_attributes,
        compressed_elements: &model.display_jt.display_jt_compressed_elements,
    })
    .unwrap_or_default();
    let annotation_stream = annotations.stream("nx:container");
    for (tessellation, source_offset) in display_jt_tessellations {
        annotations
            .note(&tessellation.id, annotation_stream, source_offset)
            .tag("DISPLAY_JT_TESSELLATION");
        annotations.exactness(&tessellation.id, Exactness::Derived);
        ir.model.tessellations.push(tessellation);
    }
    for index in &model.display_jt.display_jt_indices {
        annotations
            .note(&index.id, annotation_stream, index.source_offset)
            .tag("DISPLAY_JT_INDEX");
        annotations.exactness(&index.id, Exactness::ByteExact);
        for row in &index.rows {
            annotations
                .note(&row.id, annotation_stream, row.source_offset)
                .tag("DISPLAY_JT_INDEX_ROW");
            annotations.exactness(&row.id, Exactness::ByteExact);
        }
    }
    for document in &model.display_jt.display_jt_documents {
        annotations
            .note(&document.id, annotation_stream, document.source_offset)
            .tag("DISPLAY_JT_DOCUMENT");
        annotations.exactness(&document.id, Exactness::ByteExact);
        for entry in &document.toc_entries {
            annotations
                .note(&entry.id, annotation_stream, entry.source_offset)
                .tag("DISPLAY_JT_TOC_ENTRY");
            annotations.exactness(&entry.id, Exactness::ByteExact);
        }
    }
    for segment in &model.display_jt.display_jt_segments {
        annotations
            .note(&segment.id, annotation_stream, segment.source_offset)
            .tag("DISPLAY_JT_SEGMENT");
        annotations.exactness(&segment.id, Exactness::ByteExact);
    }
    for element in &model.display_jt.display_jt_shape_lod_elements {
        annotations
            .note(&element.id, annotation_stream, element.source_offset)
            .tag("DISPLAY_JT_SHAPE_LOD_ELEMENT");
        annotations.exactness(&element.id, Exactness::ByteExact);
    }
    for header in &model.display_jt.display_jt_tri_strip_lod_headers {
        annotations
            .note(&header.id, annotation_stream, header.source_offset)
            .tag("DISPLAY_JT_TRI_STRIP_LOD_HEADER");
        annotations.exactness(&header.id, Exactness::ByteExact);
    }
    for symbols in &model.display_jt.display_jt_initial_face_degree_symbols {
        annotations
            .note(&symbols.id, annotation_stream, symbols.source_offset)
            .tag("DISPLAY_JT_INITIAL_FACE_DEGREE_SYMBOLS");
        annotations.exactness(&symbols.id, Exactness::ByteExact);
    }
    for sequence in &model.display_jt.display_jt_topology_packet_sequences {
        annotations
            .note(&sequence.id, annotation_stream, sequence.source_offset)
            .tag("DISPLAY_JT_TOPOLOGY_PACKET_SEQUENCE");
        annotations.exactness(&sequence.id, Exactness::ByteExact);
    }
    for header in &model.display_jt.display_jt_vertex_records_headers {
        annotations
            .note(&header.id, annotation_stream, header.source_offset)
            .tag("DISPLAY_JT_VERTEX_RECORDS_HEADER");
        annotations.exactness(&header.id, Exactness::ByteExact);
    }
    for header in &model.display_jt.display_jt_coordinate_array_headers {
        annotations
            .note(&header.id, annotation_stream, header.source_offset)
            .tag("DISPLAY_JT_COORDINATE_ARRAY_HEADER");
        annotations.exactness(&header.id, Exactness::ByteExact);
    }
    for coordinates in &model.display_jt.display_jt_vertex_coordinates {
        annotations
            .note(
                &coordinates.id,
                annotation_stream,
                coordinates.source_offset,
            )
            .tag("DISPLAY_JT_VERTEX_COORDINATES");
        annotations.exactness(&coordinates.id, Exactness::Derived);
    }
    for normals in &model.display_jt.display_jt_vertex_normals {
        annotations
            .note(&normals.id, annotation_stream, normals.source_offset)
            .tag("DISPLAY_JT_VERTEX_NORMALS");
        annotations.exactness(&normals.id, Exactness::Derived);
    }
    for colors in &model.display_jt.display_jt_vertex_colors {
        annotations
            .note(&colors.id, annotation_stream, colors.source_offset)
            .tag("DISPLAY_JT_VERTEX_COLORS");
        annotations.exactness(&colors.id, Exactness::Derived);
    }
    for texture_coordinates in &model.display_jt.display_jt_vertex_texture_coordinates {
        annotations
            .note(
                &texture_coordinates.id,
                annotation_stream,
                texture_coordinates.source_offset,
            )
            .tag("DISPLAY_JT_VERTEX_TEXTURE_COORDINATES");
        annotations.exactness(&texture_coordinates.id, Exactness::Derived);
    }
    for flags in &model.display_jt.display_jt_vertex_flags {
        annotations
            .note(&flags.id, annotation_stream, flags.source_offset)
            .tag("DISPLAY_JT_VERTEX_FLAGS");
        annotations.exactness(&flags.id, Exactness::Derived);
    }
    for transform in &model.display_jt.display_jt_geometric_transform_attributes {
        annotations
            .note(&transform.id, annotation_stream, transform.source_offset)
            .tag("DISPLAY_JT_GEOMETRIC_TRANSFORM");
        annotations.exactness(&transform.id, Exactness::Derived);
    }
    for mesh in &model.display_jt.display_jt_polygon_meshes {
        annotations
            .note(&mesh.id, annotation_stream, mesh.source_offset)
            .tag("DISPLAY_JT_POLYGON_MESH");
        annotations.exactness(&mesh.id, Exactness::Derived);
    }
    for sequence in &model.display_jt.display_jt_compressed_element_sequences {
        annotations
            .note(&sequence.id, annotation_stream, sequence.source_offset)
            .tag("DISPLAY_JT_COMPRESSED_ELEMENT_SEQUENCE");
        annotations.exactness(&sequence.id, Exactness::ByteExact);
    }
    for element in &model.display_jt.display_jt_compressed_elements {
        annotations
            .note(&element.id, annotation_stream, element.source_offset)
            .tag("DISPLAY_JT_COMPRESSED_ELEMENT");
        annotations.exactness(&element.id, Exactness::ByteExact);
    }
    for atom in &model.display_jt.display_jt_string_property_atoms {
        annotations
            .note(&atom.id, annotation_stream, atom.source_offset)
            .tag("DISPLAY_JT_STRING_PROPERTY_ATOM");
        annotations.exactness(&atom.id, Exactness::ByteExact);
    }
    for binding in &model.display_jt.display_jt_shape_lod_bindings {
        annotations
            .note(&binding.id, annotation_stream, binding.source_offset)
            .tag("DISPLAY_JT_SHAPE_LOD_BINDING");
        annotations.exactness(&binding.id, Exactness::ByteExact);
    }
    for node in &model.display_jt.display_jt_base_node_data {
        annotations
            .note(&node.id, annotation_stream, node.source_offset)
            .tag("DISPLAY_JT_BASE_NODE_DATA");
        annotations.exactness(&node.id, Exactness::ByteExact);
    }
    for node in &model.display_jt.display_jt_group_node_data {
        annotations
            .note(&node.id, annotation_stream, node.source_offset)
            .tag("DISPLAY_JT_GROUP_NODE_DATA");
        annotations.exactness(&node.id, Exactness::ByteExact);
    }
    for node in &model.display_jt.display_jt_instance_nodes {
        annotations
            .note(&node.id, annotation_stream, node.source_offset)
            .tag("DISPLAY_JT_INSTANCE_NODE");
        annotations.exactness(&node.id, Exactness::ByteExact);
    }
    for node in &model.display_jt.display_jt_partition_nodes {
        annotations
            .note(&node.id, annotation_stream, node.source_offset)
            .tag("DISPLAY_JT_PARTITION_NODE");
        annotations.exactness(&node.id, Exactness::ByteExact);
    }
    for node in &model.display_jt.display_jt_range_lod_nodes {
        annotations
            .note(&node.id, annotation_stream, node.source_offset)
            .tag("DISPLAY_JT_RANGE_LOD_NODE");
        annotations.exactness(&node.id, Exactness::ByteExact);
    }
    for node in &model.display_jt.display_jt_tri_strip_shape_nodes {
        annotations
            .note(&node.id, annotation_stream, node.source_offset)
            .tag("DISPLAY_JT_TRI_STRIP_SHAPE_NODE");
        annotations.exactness(&node.id, Exactness::ByteExact);
    }
    for row in &model.segments.segment_index_rows {
        annotations
            .note(&row.id, annotation_stream, row.source_offset)
            .tag("UG_PART_SEGMENT_INDEX_ROW");
        annotations.exactness(&row.id, Exactness::ByteExact);
    }
    for link in &model.segments.segment_stream_links {
        annotations
            .note(&link.id, annotation_stream, link.source_offset)
            .tag("UG_PART_SEGMENT_STREAM_LINK");
        annotations.exactness(&link.id, Exactness::ByteExact);
    }
    for binding in &model.segments.segment_body_bindings {
        annotations
            .note(&binding.id, annotation_stream, binding.source_offset)
            .tag("UG_PART_SEGMENT_BODY_BINDING");
        annotations.exactness(&binding.id, Exactness::ByteExact);
    }
    for status in &model.segments.segment_body_lineage_statuses {
        annotations
            .note(&status.id, annotation_stream, status.source_offset)
            .tag("SEGMENT_BODY_LINEAGE_STATUS");
        annotations.exactness(&status.id, Exactness::Derived);
    }
    for record in &model.parasolid.parasolid_blend_surface_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("BLEND_SURF");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &model.parasolid.parasolid_blend_bound_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("BLEND_BOUND");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &model.parasolid.parasolid_offset_surface_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("OFFSET_SURF");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &model.parasolid.parasolid_trimmed_curve_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("TRIMMED_CURVE");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &model.parasolid.parasolid_surface_curve_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("SP_CURVE");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &model.parasolid.parasolid_intersection_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag(if record.delta_twin {
                "INTERSECTION_DATA"
            } else {
                "INTERSECTION"
            });
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &model.parasolid.parasolid_term_use_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("term_use");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &model.parasolid.parasolid_support_uv_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("values");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &model.parasolid.parasolid_chart_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("CHART_s");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for definition in &model.parasolid.parasolid_attribute_definitions {
        let source_stream = annotations.stream(format!("nx:s{}", definition.stream_ordinal));
        annotations
            .note(&definition.id, source_stream, definition.inflated_offset)
            .tag("ATTRIBUTE_DEFINITION");
        annotations.exactness(&definition.id, Exactness::ByteExact);
    }
    for record in &model.parasolid.parasolid_entity_51_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("ENTITY_51");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &model.parasolid.parasolid_entity_52_integer_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("ENTITY_52_INTEGERS");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &model.parasolid.parasolid_entity_53_double_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("ENTITY_53_DOUBLES");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &model.parasolid.parasolid_entity_54_string_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("ENTITY_54_STRING");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for block_use in &model.parasolid.parasolid_entity_51_string_uses {
        let source_stream = annotations.stream(format!("nx:s{}", block_use.stream_ordinal));
        annotations
            .note(&block_use.id, source_stream, block_use.inflated_offset)
            .tag("ENTITY_51_STRING_USE");
        annotations.exactness(&block_use.id, Exactness::ByteExact);
    }
    for value_use in &model.parasolid.parasolid_entity_51_numeric_uses {
        let source_stream = annotations.stream(format!("nx:s{}", value_use.stream_ordinal));
        annotations
            .note(&value_use.id, source_stream, value_use.inflated_offset)
            .tag("ENTITY_51_NUMERIC_USE");
        annotations.exactness(&value_use.id, Exactness::ByteExact);
    }
    for class_use in &model.parasolid.parasolid_attribute_class_uses {
        let entity = model
            .parasolid
            .parasolid_entity_51_records
            .iter()
            .find(|entity| entity.id == class_use.entity_51_record)
            .expect("class use owns a type-81 entity");
        let source_stream = annotations.stream(format!("nx:s{}", class_use.stream_ordinal));
        annotations
            .note(&class_use.id, source_stream, entity.inflated_offset)
            .tag("ATTRIBUTE_CLASS_USE");
        annotations.exactness(&class_use.id, Exactness::Derived);
    }
    for reference in &model.parasolid.parasolid_topology_attribute_list_references {
        let source_stream = annotations.stream(format!("nx:s{}", reference.stream_ordinal));
        annotations
            .note(&reference.id, source_stream, reference.inflated_offset)
            .tag("TOPOLOGY_ATTRIBUTE_LIST_REFERENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for class_use in &model.parasolid.parasolid_topology_attribute_class_uses {
        let reference = model
            .parasolid
            .parasolid_topology_attribute_list_references
            .iter()
            .find(|reference| reference.id == class_use.topology_attribute_reference)
            .expect("class use owns a topology attribute reference");
        let source_stream = annotations.stream(format!("nx:s{}", reference.stream_ordinal));
        let entity = model
            .parasolid
            .parasolid_entity_51_records
            .iter()
            .find(|entity| entity.id == class_use.entity_51_record)
            .expect("class use owns a type-81 entity");
        annotations
            .note(&class_use.id, source_stream, entity.inflated_offset)
            .tag("TOPOLOGY_ATTRIBUTE_CLASS_USE");
        annotations.exactness(&class_use.id, Exactness::Derived);
    }
    for frame in &model.features.data_block_object_frames {
        annotations
            .note(&frame.id, annotation_stream, frame.source_offset)
            .tag("OFFSET_STORE_OBJECT_FRAME");
        annotations.exactness(&frame.id, Exactness::ByteExact);
    }
    for point in &model.features.offset_store_named_points {
        annotations
            .note(&point.id, annotation_stream, point.source_offset)
            .tag("OFFSET_STORE_NAMED_POINT");
        annotations.exactness(&point.id, Exactness::ByteExact);
    }
    for block_use in &model.features.feature_sketch_named_point_block_uses {
        annotations
            .note(&block_use.id, annotation_stream, block_use.source_offset)
            .tag("SKETCH_NAMED_POINT_BLOCK_USE");
        annotations.exactness(&block_use.id, Exactness::ByteExact);
    }
    for point_use in &model.features.feature_sketch_preceding_named_point_uses {
        annotations
            .note(&point_use.id, annotation_stream, point_use.source_offset)
            .tag("SKETCH_PRECEDING_NAMED_POINT_USE");
        annotations.exactness(&point_use.id, Exactness::ByteExact);
    }
    for point_use in &model.features.feature_sketch_point_uses {
        annotations
            .note(
                &point_use.id,
                annotation_stream,
                point_use.source_offsets[0],
            )
            .tag("SKETCH_POINT_USE");
        annotations.exactness(&point_use.id, Exactness::Derived);
    }
    for dependency in &model.features.feature_sketch_datum_csys_dependencies {
        annotations
            .note(&dependency.id, annotation_stream, dependency.source_offset)
            .tag("SKETCH_DATUM_CSYS_DEPENDENCY");
        annotations.exactness(&dependency.id, Exactness::Derived);
    }
    for group in &model.features.feature_input_block_identity_groups {
        annotations
            .note(&group.id, annotation_stream, group.source_offsets[0])
            .tag("FEATURE_INPUT_BLOCK_IDENTITY_GROUP");
        annotations.exactness(&group.id, Exactness::ByteExact);
    }
    for lane in &model.om.data_block_abr_reference_lanes {
        annotations
            .note(&lane.id, annotation_stream, lane.source_offset)
            .tag("OFFSET_STORE_ABR_REFERENCE_LANE");
        annotations.exactness(&lane.id, Exactness::ByteExact);
    }
    for link in &model.segments.segment_om_links {
        annotations
            .note(&link.id, annotation_stream, link.source_offset)
            .tag("UG_PART_SEGMENT_OM_LINK");
        annotations.exactness(&link.id, Exactness::ByteExact);
    }
    for area in &model.om.om_record_areas {
        annotations
            .note(&area.id, annotation_stream, area.source_offset)
            .tag("OM_RECORD_AREA");
        annotations.exactness(&area.id, Exactness::ByteExact);
    }
    for label in &model.features.feature_operation_labels {
        annotations
            .note(&label.id, annotation_stream, label.source_offset)
            .tag("FEATURE_OPERATION_LABEL");
        annotations.exactness(&label.id, Exactness::ByteExact);
    }
    for sketch in &model.features.feature_sketch_records {
        annotations
            .note(&sketch.id, annotation_stream, sketch.source_offset)
            .tag("FEATURE_SKETCH_RECORD");
        annotations.exactness(&sketch.id, Exactness::Derived);
    }
    for pair in &model.features.feature_sketch_payload_fixed_pairs {
        annotations
            .note(&pair.id, annotation_stream, pair.source_offset)
            .tag("FEATURE_SKETCH_FIXED_PAIR");
        annotations.exactness(&pair.id, Exactness::ByteExact);
    }
    for point in &model.features.feature_sketch_fixed_points {
        annotations
            .note(&point.id, annotation_stream, point.source_offset)
            .tag("FEATURE_SKETCH_FIXED_POINT");
        annotations.exactness(&point.id, Exactness::Derived);
    }
    for record in &model.features.feature_operation_records {
        annotations
            .note(&record.id, annotation_stream, record.source_offset)
            .tag("FEATURE_OPERATION_RECORD");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for value in &model.features.feature_payload_strings {
        annotations
            .note(&value.id, annotation_stream, value.source_offset)
            .tag("FEATURE_PAYLOAD_STRING");
        annotations.exactness(&value.id, Exactness::ByteExact);
    }
    for reference in &model.features.feature_body_references {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("FEATURE_BODY_REFERENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for reference in &model.features.feature_body_reference_occurrences {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("FEATURE_BODY_REFERENCE_OCCURRENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for input in &model.features.feature_input_blocks {
        annotations
            .note(&input.id, annotation_stream, input.source_offset)
            .tag("FEATURE_INPUT_BLOCK");
        annotations.exactness(&input.id, Exactness::ByteExact);
    }
    for operation in &model.features.feature_boolean_operations {
        annotations
            .note(&operation.id, annotation_stream, operation.source_offset)
            .tag("FEATURE_BOOLEAN_OPERATION");
        annotations.exactness(&operation.id, Exactness::ByteExact);
    }
    for declaration in &model.om.expression_declarations {
        annotations
            .note(
                &declaration.id,
                annotation_stream,
                declaration.source_offset,
            )
            .tag("EXPRESSION_DECLARATION");
        annotations.exactness(&declaration.id, Exactness::ByteExact);
    }
    for value in &model.om.data_block_control_values {
        annotations
            .note(&value.id, annotation_stream, value.source_offset)
            .tag("OM_DATA_BLOCK_CONTROL_VALUE");
        annotations.exactness(&value.id, Exactness::ByteExact);
    }
    for reference in &model.om.data_block_control_class_references {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("OM_DATA_BLOCK_CONTROL_CLASS_REFERENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for value in &model.om.data_block_control_index_values {
        annotations
            .note(&value.id, annotation_stream, value.source_offset)
            .tag("OM_DATA_BLOCK_CONTROL_INDEX_VALUE");
        annotations.exactness(&value.id, Exactness::ByteExact);
    }
    for reference in &model.om.data_block_control_references {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("OM_DATA_BLOCK_CONTROL_REFERENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for pair in &model.om.data_block_control_handle_pairs {
        annotations
            .note(&pair.id, annotation_stream, pair.source_offset)
            .tag("OM_DATA_BLOCK_CONTROL_HANDLE_PAIR");
        annotations.exactness(&pair.id, Exactness::ByteExact);
    }
    for reference in &model.om.data_block_references {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("OM_DATA_BLOCK_REFERENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for binding in &model.features.feature_parameter_bindings {
        annotations
            .note(&binding.id, annotation_stream, binding.source_offset)
            .tag("FEATURE_PARAMETER_BINDING");
        annotations.exactness(&binding.id, Exactness::Derived);
    }
    for parameter_use in &model.features.feature_parameter_uses {
        annotations
            .note(
                &parameter_use.id,
                annotation_stream,
                parameter_use.source_offsets[0],
            )
            .tag("FEATURE_PARAMETER_USE");
        annotations.exactness(&parameter_use.id, Exactness::Derived);
    }
    for header in &model.om.store_headers {
        annotations
            .note(&header.id, annotation_stream, header.source_offset)
            .tag("OM_STORE_VERSION");
        annotations.exactness(&header.id, Exactness::ByteExact);
    }
    for reference in &model.om.external_references {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("EXTREFSTREAM_STRING");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for attribute in &model.om.part_attributes {
        annotations
            .note(&attribute.id, annotation_stream, attribute.source_offset)
            .tag("Attribute");
        annotations.exactness(&attribute.id, Exactness::ByteExact);
        let id = AttributeId(format!("{}:neutral", attribute.id));
        annotations
            .note(&id.0, annotation_stream, attribute.source_offset)
            .tag("Attribute");
        annotations.derived(&id.0, "target");
        annotations.derived(&id.0, "name");
        annotations.derived(&id.0, "values");
        ir.model.attributes.push(SourceAttribute {
            id,
            target: AttributeTarget::Document,
            name: attribute.title.clone(),
            values: vec![AttributeValue::String(attribute.value.clone())],
        });
    }
    attach_parasolid_topology_string_attributes(
        ir,
        &model.parasolid.parasolid_topology_attribute_list_references,
        &model.parasolid.parasolid_topology_attribute_class_uses,
        &model.parasolid.parasolid_attribute_definitions,
        &model.parasolid.parasolid_entity_51_string_uses,
        &model.parasolid.parasolid_entity_54_string_records,
        annotations,
    );
    attach_parasolid_topology_numeric_attributes(
        ir,
        &ParasolidNumericAttributeSources {
            topology_references: &model.parasolid.parasolid_topology_attribute_list_references,
            class_uses: &model.parasolid.parasolid_topology_attribute_class_uses,
            definitions: &model.parasolid.parasolid_attribute_definitions,
            numeric_uses: &model.parasolid.parasolid_entity_51_numeric_uses,
            integers: &model.parasolid.parasolid_entity_52_integer_records,
            doubles: &model.parasolid.parasolid_entity_53_double_records,
        },
        annotations,
    );
    for record in &model.om.external_reference_records {
        annotations
            .note(&record.id, annotation_stream, record.source_offset)
            .tag("EXTREFSTREAM_RECORD");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for asset in &model.om.material_texture_assets {
        annotations
            .note(&asset.id, annotation_stream, asset.source_offset)
            .tag("TIFF_MATERIAL_TEXTURE");
        annotations.exactness(&asset.id, Exactness::ByteExact);
    }
    for entry in &model.om.material_texture_catalog_entries {
        annotations
            .note(&entry.id, annotation_stream, entry.source_offset)
            .tag("QAF_MATERIAL_TEXTURE_CATALOG_ENTRY");
        annotations.exactness(&entry.id, Exactness::Derived);
    }
    for (section_index, (entry, section)) in object_sections.iter().enumerate() {
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (record_index, record) in section
            .control
            .iter()
            .chain(section.records.iter())
            .enumerate()
        {
            let kind = if record.object_id.is_some() {
                "record"
            } else {
                "block"
            };
            let id = UnknownId(format!(
                "nx:om-section-{section_index}:{kind}#{record_index}"
            ));
            let offset = entry_offset + record.offset as u64;
            annotations
                .note(&id, annotation_stream, offset)
                .tag(if record.object_id.is_some() {
                    "OM_ENTITY_RECORD"
                } else {
                    "OM_DATA_BLOCK"
                });
            annotations.exactness(&id, Exactness::ByteExact);
            unknowns.push(UnknownRecord {
                id,
                offset,
                byte_len: record.bytes.len() as u64,
                sha256: sha256_hex(record.bytes),
                data: Some(record.bytes.to_vec()),
                links: Vec::new(),
            });
        }
    }
    if !model.om.configurations.is_empty() {
        for (ordinal, configuration) in model.om.configurations.iter().enumerate() {
            let id = ConfigurationId(format!("nx:arrangements:configuration#{ordinal}"));
            let active_attribute_use = model
                .om
                .configuration_attribute_uses
                .iter()
                .find(|relation| relation.configuration == configuration.id);
            let bodies = if active_attribute_use.is_some() {
                ConfigurationBodies::Resolved(
                    ir.model.bodies.iter().map(|body| body.id.clone()).collect(),
                )
            } else {
                ConfigurationBodies::Unresolved
            };
            annotations
                .note(&id.0, annotation_stream, configuration.source_offset)
                .tag("Arrangement");
            annotations.derived(&id.0, "ordinal");
            annotations.derived(&id.0, "active");
            annotations.derived(&id.0, "source_index");
            annotations.derived(&id.0, "name");
            annotations.derived(&id.0, "native_ref");
            if bodies.resolved().is_some_and(|bodies| !bodies.is_empty()) {
                annotations.derived(&id.0, "bodies");
            }
            ir.model.configurations.push(DesignConfiguration {
                id,
                ordinal: ordinal as u32,
                active: active_attribute_use.is_some(),
                source_index: Some(ordinal as u32),
                name: configuration.name.clone(),
                material: None,
                properties: active_attribute_use
                    .map(|relation| {
                        BTreeMap::from([("active_attribute_use".to_string(), relation.id.clone())])
                    })
                    .unwrap_or_default(),
                bodies,
                native_ref: Some(configuration.id.clone()),
            });
        }
    }
    attach_expression_parameters(
        ir,
        &model.om.expressions,
        &model.om.expression_declarations,
        &model.features.feature_parameter_uses,
        annotations,
    );
    attach_feature_operations(
        ir,
        &model.features,
        &model.om.expressions,
        &model.segments.segment_body_bindings,
        annotations,
    );
    attach_block_dimension_parameter_consumers(
        ir,
        &model.features.feature_block_dimensions,
        annotations,
    );
    ir.model
        .features
        .sort_by(|first, second| first.id.cmp(&second.id));
    let namespace = ir.native.namespace_mut("nx");
    namespace.version = namespace.version.max(155);
    if !model.segments.segment_index_rows.is_empty() {
        namespace.set_arena("segment_index_rows", &model.segments.segment_index_rows)?;
    }
    if !model.segments.segment_stream_links.is_empty() {
        namespace.set_arena("segment_stream_links", &model.segments.segment_stream_links)?;
    }
    if !model.segments.segment_body_bindings.is_empty() {
        namespace.set_arena(
            "segment_body_bindings",
            &model.segments.segment_body_bindings,
        )?;
    }
    if !model.features.feature_body_segment_uses.is_empty() {
        namespace.set_arena(
            "feature_body_segment_uses",
            &model.features.feature_body_segment_uses,
        )?;
    }
    if !model.segments.segment_body_lineage_statuses.is_empty() {
        namespace.set_arena(
            "segment_body_lineage_statuses",
            &model.segments.segment_body_lineage_statuses,
        )?;
    }
    if !model.parasolid.parasolid_blend_surface_records.is_empty() {
        namespace.set_arena(
            "parasolid_blend_surface_records",
            &model.parasolid.parasolid_blend_surface_records,
        )?;
    }
    if !model.parasolid.parasolid_blend_bound_records.is_empty() {
        namespace.set_arena(
            "parasolid_blend_bound_records",
            &model.parasolid.parasolid_blend_bound_records,
        )?;
    }
    if !model.parasolid.parasolid_offset_surface_records.is_empty() {
        namespace.set_arena(
            "parasolid_offset_surface_records",
            &model.parasolid.parasolid_offset_surface_records,
        )?;
    }
    if !model.parasolid.parasolid_trimmed_curve_records.is_empty() {
        namespace.set_arena(
            "parasolid_trimmed_curve_records",
            &model.parasolid.parasolid_trimmed_curve_records,
        )?;
    }
    if !model.parasolid.parasolid_surface_curve_records.is_empty() {
        namespace.set_arena(
            "parasolid_surface_curve_records",
            &model.parasolid.parasolid_surface_curve_records,
        )?;
    }
    if !model.parasolid.parasolid_intersection_records.is_empty() {
        namespace.set_arena(
            "parasolid_intersection_records",
            &model.parasolid.parasolid_intersection_records,
        )?;
    }
    if !model.parasolid.parasolid_term_use_records.is_empty() {
        namespace.set_arena(
            "parasolid_term_use_records",
            &model.parasolid.parasolid_term_use_records,
        )?;
    }
    if !model.parasolid.parasolid_support_uv_records.is_empty() {
        namespace.set_arena(
            "parasolid_support_uv_records",
            &model.parasolid.parasolid_support_uv_records,
        )?;
    }
    if !model.parasolid.parasolid_chart_records.is_empty() {
        namespace.set_arena(
            "parasolid_chart_records",
            &model.parasolid.parasolid_chart_records,
        )?;
    }
    if !model.parasolid.parasolid_attribute_definitions.is_empty() {
        namespace.set_arena(
            "parasolid_attribute_definitions",
            &model.parasolid.parasolid_attribute_definitions,
        )?;
    }
    if !model.parasolid.parasolid_entity_51_records.is_empty() {
        namespace.set_arena(
            "parasolid_entity_51_records",
            &model.parasolid.parasolid_entity_51_records,
        )?;
    }
    if !model
        .parasolid
        .parasolid_entity_52_integer_records
        .is_empty()
    {
        namespace.set_arena(
            "parasolid_entity_52_integer_records",
            &model.parasolid.parasolid_entity_52_integer_records,
        )?;
    }
    if !model
        .parasolid
        .parasolid_entity_53_double_records
        .is_empty()
    {
        namespace.set_arena(
            "parasolid_entity_53_double_records",
            &model.parasolid.parasolid_entity_53_double_records,
        )?;
    }
    if !model
        .parasolid
        .parasolid_entity_54_string_records
        .is_empty()
    {
        namespace.set_arena(
            "parasolid_entity_54_string_records",
            &model.parasolid.parasolid_entity_54_string_records,
        )?;
    }
    if !model.parasolid.parasolid_entity_51_string_uses.is_empty() {
        namespace.set_arena(
            "parasolid_entity_51_string_uses",
            &model.parasolid.parasolid_entity_51_string_uses,
        )?;
    }
    if !model.parasolid.parasolid_entity_51_numeric_uses.is_empty() {
        namespace.set_arena(
            "parasolid_entity_51_numeric_uses",
            &model.parasolid.parasolid_entity_51_numeric_uses,
        )?;
    }
    if !model.parasolid.parasolid_attribute_class_uses.is_empty() {
        namespace.set_arena(
            "parasolid_attribute_class_uses",
            &model.parasolid.parasolid_attribute_class_uses,
        )?;
    }
    if !model
        .parasolid
        .parasolid_topology_attribute_list_references
        .is_empty()
    {
        namespace.set_arena(
            "parasolid_topology_attribute_list_references",
            &model.parasolid.parasolid_topology_attribute_list_references,
        )?;
    }
    if !model
        .parasolid
        .parasolid_topology_attribute_class_uses
        .is_empty()
    {
        namespace.set_arena(
            "parasolid_topology_attribute_class_uses",
            &model.parasolid.parasolid_topology_attribute_class_uses,
        )?;
    }
    if !model.segments.segment_om_links.is_empty() {
        namespace.set_arena("segment_om_links", &model.segments.segment_om_links)?;
    }
    if !model.om.om_record_areas.is_empty() {
        namespace.set_arena("om_record_areas", &model.om.om_record_areas)?;
    }
    if !model.features.feature_operation_labels.is_empty() {
        namespace.set_arena(
            "feature_operation_labels",
            &model.features.feature_operation_labels,
        )?;
    }
    if !model.features.feature_operation_records.is_empty() {
        namespace.set_arena(
            "feature_operation_records",
            &model.features.feature_operation_records,
        )?;
    }
    if !model.features.feature_payload_strings.is_empty() {
        namespace.set_arena(
            "feature_payload_strings",
            &model.features.feature_payload_strings,
        )?;
    }
    if !model.features.feature_simple_hole_templates.is_empty() {
        namespace.set_arena(
            "feature_simple_hole_templates",
            &model.features.feature_simple_hole_templates,
        )?;
    }
    if !model
        .features
        .feature_simple_hole_repeated_scalar_lanes
        .is_empty()
    {
        namespace.set_arena(
            "feature_simple_hole_repeated_scalar_lanes",
            &model.features.feature_simple_hole_repeated_scalar_lanes,
        )?;
    }
    if !model
        .features
        .feature_simple_hole_repeated_scalar_lane_block_references
        .is_empty()
    {
        namespace.set_arena(
            "feature_simple_hole_repeated_scalar_lane_block_references",
            &model
                .features
                .feature_simple_hole_repeated_scalar_lane_block_references,
        )?;
    }
    if !model
        .features
        .feature_simple_hole_construction_groups
        .is_empty()
    {
        namespace.set_arena(
            "feature_simple_hole_construction_groups",
            &model.features.feature_simple_hole_construction_groups,
        )?;
    }
    if !model.features.feature_body_references.is_empty() {
        namespace.set_arena(
            "feature_body_references",
            &model.features.feature_body_references,
        )?;
    }
    if !model.features.feature_body_reference_occurrences.is_empty() {
        namespace.set_arena(
            "feature_body_reference_occurrences",
            &model.features.feature_body_reference_occurrences,
        )?;
    }
    if !model.features.feature_input_blocks.is_empty() {
        namespace.set_arena("feature_input_blocks", &model.features.feature_input_blocks)?;
    }
    if !model
        .features
        .feature_input_block_identity_groups
        .is_empty()
    {
        namespace.set_arena(
            "feature_input_block_identity_groups",
            &model.features.feature_input_block_identity_groups,
        )?;
    }
    if !model.display_jt.display_jt_indices.is_empty() {
        namespace.set_arena("display_jt_indices", &model.display_jt.display_jt_indices)?;
    }
    if !model.display_jt.display_jt_documents.is_empty() {
        namespace.set_arena(
            "display_jt_documents",
            &model.display_jt.display_jt_documents,
        )?;
    }
    if !model.display_jt.display_jt_segments.is_empty() {
        namespace.set_arena("display_jt_segments", &model.display_jt.display_jt_segments)?;
    }
    if !model.display_jt.display_jt_shape_lod_elements.is_empty() {
        namespace.set_arena(
            "display_jt_shape_lod_elements",
            &model.display_jt.display_jt_shape_lod_elements,
        )?;
    }
    if !model.display_jt.display_jt_tri_strip_lod_headers.is_empty() {
        namespace.set_arena(
            "display_jt_tri_strip_lod_headers",
            &model.display_jt.display_jt_tri_strip_lod_headers,
        )?;
    }
    if !model
        .display_jt
        .display_jt_initial_face_degree_symbols
        .is_empty()
    {
        namespace.set_arena(
            "display_jt_initial_face_degree_symbols",
            &model.display_jt.display_jt_initial_face_degree_symbols,
        )?;
    }
    if !model
        .display_jt
        .display_jt_topology_packet_sequences
        .is_empty()
    {
        namespace.set_arena(
            "display_jt_topology_packet_sequences",
            &model.display_jt.display_jt_topology_packet_sequences,
        )?;
    }
    if !model
        .display_jt
        .display_jt_vertex_records_headers
        .is_empty()
    {
        namespace.set_arena(
            "display_jt_vertex_records_headers",
            &model.display_jt.display_jt_vertex_records_headers,
        )?;
    }
    if !model
        .display_jt
        .display_jt_coordinate_array_headers
        .is_empty()
    {
        namespace.set_arena(
            "display_jt_coordinate_array_headers",
            &model.display_jt.display_jt_coordinate_array_headers,
        )?;
    }
    if !model.display_jt.display_jt_vertex_coordinates.is_empty() {
        namespace.set_arena(
            "display_jt_vertex_coordinates",
            &model.display_jt.display_jt_vertex_coordinates,
        )?;
    }
    if !model.display_jt.display_jt_vertex_normals.is_empty() {
        namespace.set_arena(
            "display_jt_vertex_normals",
            &model.display_jt.display_jt_vertex_normals,
        )?;
    }
    if !model.display_jt.display_jt_vertex_colors.is_empty() {
        namespace.set_arena(
            "display_jt_vertex_colors",
            &model.display_jt.display_jt_vertex_colors,
        )?;
    }
    if !model
        .display_jt
        .display_jt_vertex_texture_coordinates
        .is_empty()
    {
        namespace.set_arena(
            "display_jt_vertex_texture_coordinates",
            &model.display_jt.display_jt_vertex_texture_coordinates,
        )?;
    }
    if !model.display_jt.display_jt_vertex_flags.is_empty() {
        namespace.set_arena(
            "display_jt_vertex_flags",
            &model.display_jt.display_jt_vertex_flags,
        )?;
    }
    if !model
        .display_jt
        .display_jt_geometric_transform_attributes
        .is_empty()
    {
        namespace.set_arena(
            "display_jt_geometric_transform_attributes",
            &model.display_jt.display_jt_geometric_transform_attributes,
        )?;
    }
    if !model.display_jt.display_jt_polygon_meshes.is_empty() {
        namespace.set_arena(
            "display_jt_polygon_meshes",
            &model.display_jt.display_jt_polygon_meshes,
        )?;
    }
    if !model
        .display_jt
        .display_jt_compressed_element_sequences
        .is_empty()
    {
        namespace.set_arena(
            "display_jt_compressed_element_sequences",
            &model.display_jt.display_jt_compressed_element_sequences,
        )?;
    }
    if !model.display_jt.display_jt_compressed_elements.is_empty() {
        namespace.set_arena(
            "display_jt_compressed_elements",
            &model.display_jt.display_jt_compressed_elements,
        )?;
    }
    if !model.display_jt.display_jt_string_property_atoms.is_empty() {
        namespace.set_arena(
            "display_jt_string_property_atoms",
            &model.display_jt.display_jt_string_property_atoms,
        )?;
    }
    if !model.display_jt.display_jt_shape_lod_bindings.is_empty() {
        namespace.set_arena(
            "display_jt_shape_lod_bindings",
            &model.display_jt.display_jt_shape_lod_bindings,
        )?;
    }
    if !model.display_jt.display_jt_base_node_data.is_empty() {
        namespace.set_arena(
            "display_jt_base_node_data",
            &model.display_jt.display_jt_base_node_data,
        )?;
    }
    if !model.display_jt.display_jt_group_node_data.is_empty() {
        namespace.set_arena(
            "display_jt_group_node_data",
            &model.display_jt.display_jt_group_node_data,
        )?;
    }
    if !model.display_jt.display_jt_instance_nodes.is_empty() {
        namespace.set_arena(
            "display_jt_instance_nodes",
            &model.display_jt.display_jt_instance_nodes,
        )?;
    }
    if !model.display_jt.display_jt_partition_nodes.is_empty() {
        namespace.set_arena(
            "display_jt_partition_nodes",
            &model.display_jt.display_jt_partition_nodes,
        )?;
    }
    if !model.display_jt.display_jt_range_lod_nodes.is_empty() {
        namespace.set_arena(
            "display_jt_range_lod_nodes",
            &model.display_jt.display_jt_range_lod_nodes,
        )?;
    }
    if !model.display_jt.display_jt_tri_strip_shape_nodes.is_empty() {
        namespace.set_arena(
            "display_jt_tri_strip_shape_nodes",
            &model.display_jt.display_jt_tri_strip_shape_nodes,
        )?;
    }
    if !model.features.feature_datum_csys_constructions.is_empty() {
        namespace.set_arena(
            "feature_datum_csys_constructions",
            &model.features.feature_datum_csys_constructions,
        )?;
    }
    if !model.features.feature_datum_csys_payloads.is_empty() {
        namespace.set_arena(
            "feature_datum_csys_payloads",
            &model.features.feature_datum_csys_payloads,
        )?;
    }
    if !model
        .features
        .feature_datum_csys_payload_scalar_pairs
        .is_empty()
    {
        namespace.set_arena(
            "feature_datum_csys_payload_scalar_pairs",
            &model.features.feature_datum_csys_payload_scalar_pairs,
        )?;
    }
    if !model
        .features
        .feature_datum_csys_payload_fixed_pairs
        .is_empty()
    {
        namespace.set_arena(
            "feature_datum_csys_payload_fixed_pairs",
            &model.features.feature_datum_csys_payload_fixed_pairs,
        )?;
    }
    if !model.features.feature_datum_csys_payload_scalars.is_empty() {
        namespace.set_arena(
            "feature_datum_csys_payload_scalars",
            &model.features.feature_datum_csys_payload_scalars,
        )?;
    }
    if !model.features.feature_datum_csys_descriptors.is_empty() {
        namespace.set_arena(
            "feature_datum_csys_descriptors",
            &model.features.feature_datum_csys_descriptors,
        )?;
    }
    if !model.features.feature_datum_csys_block_uses.is_empty() {
        namespace.set_arena(
            "feature_datum_csys_block_uses",
            &model.features.feature_datum_csys_block_uses,
        )?;
    }
    if !model.features.feature_datum_plane_headers.is_empty() {
        namespace.set_arena(
            "feature_datum_plane_headers",
            &model.features.feature_datum_plane_headers,
        )?;
    }
    if !model.features.feature_datum_plane_block_uses.is_empty() {
        namespace.set_arena(
            "feature_datum_plane_block_uses",
            &model.features.feature_datum_plane_block_uses,
        )?;
    }
    if !model.features.feature_datum_plane_payloads.is_empty() {
        namespace.set_arena(
            "feature_datum_plane_payloads",
            &model.features.feature_datum_plane_payloads,
        )?;
    }
    if !model
        .features
        .feature_datum_plane_payload_scalar_pairs
        .is_empty()
    {
        namespace.set_arena(
            "feature_datum_plane_payload_scalar_pairs",
            &model.features.feature_datum_plane_payload_scalar_pairs,
        )?;
    }
    if !model.features.feature_datum_plane_descriptors.is_empty() {
        namespace.set_arena(
            "feature_datum_plane_descriptors",
            &model.features.feature_datum_plane_descriptors,
        )?;
    }
    if !model
        .features
        .feature_datum_plane_csys_identity_uses
        .is_empty()
    {
        namespace.set_arena(
            "feature_datum_plane_csys_identity_uses",
            &model.features.feature_datum_plane_csys_identity_uses,
        )?;
    }
    if !model.features.feature_sketch_references.is_empty() {
        namespace.set_arena(
            "feature_sketch_references",
            &model.features.feature_sketch_references,
        )?;
    }
    if !model.features.feature_projected_curve_references.is_empty() {
        namespace.set_arena(
            "feature_projected_curve_references",
            &model.features.feature_projected_curve_references,
        )?;
    }
    if !model
        .features
        .feature_projected_curve_construction_payloads
        .is_empty()
    {
        namespace.set_arena(
            "feature_projected_curve_construction_payloads",
            &model.features.feature_projected_curve_construction_payloads,
        )?;
    }
    if !model
        .features
        .feature_projected_curve_construction_strings
        .is_empty()
    {
        namespace.set_arena(
            "feature_projected_curve_construction_strings",
            &model.features.feature_projected_curve_construction_strings,
        )?;
    }
    if !model.features.feature_pattern_references.is_empty() {
        namespace.set_arena(
            "feature_pattern_references",
            &model.features.feature_pattern_references,
        )?;
    }
    if !model
        .features
        .feature_pattern_construction_payloads
        .is_empty()
    {
        namespace.set_arena(
            "feature_pattern_construction_payloads",
            &model.features.feature_pattern_construction_payloads,
        )?;
    }
    if !model
        .features
        .feature_pattern_construction_strings
        .is_empty()
    {
        namespace.set_arena(
            "feature_pattern_construction_strings",
            &model.features.feature_pattern_construction_strings,
        )?;
    }
    if !model
        .features
        .feature_pattern_construction_fixed_lanes
        .is_empty()
    {
        namespace.set_arena(
            "feature_pattern_construction_fixed_lanes",
            &model.features.feature_pattern_construction_fixed_lanes,
        )?;
    }
    if !model.features.feature_pattern_transform_lanes.is_empty() {
        namespace.set_arena(
            "feature_pattern_transform_lanes",
            &model.features.feature_pattern_transform_lanes,
        )?;
    }
    if !model.features.feature_point_construction_headers.is_empty() {
        namespace.set_arena(
            "feature_point_construction_headers",
            &model.features.feature_point_construction_headers,
        )?;
    }
    if !model
        .features
        .feature_point_construction_scalar_lanes
        .is_empty()
    {
        namespace.set_arena(
            "feature_point_construction_scalar_lanes",
            &model.features.feature_point_construction_scalar_lanes,
        )?;
    }
    if !model
        .features
        .feature_draft_construction_references
        .is_empty()
    {
        namespace.set_arena(
            "feature_draft_construction_references",
            &model.features.feature_draft_construction_references,
        )?;
    }
    if !model
        .features
        .feature_draft_construction_index_lanes
        .is_empty()
    {
        namespace.set_arena(
            "feature_draft_construction_index_lanes",
            &model.features.feature_draft_construction_index_lanes,
        )?;
    }
    if !model
        .features
        .feature_draft_construction_payloads
        .is_empty()
    {
        namespace.set_arena(
            "feature_draft_construction_payloads",
            &model.features.feature_draft_construction_payloads,
        )?;
    }
    if !model
        .features
        .feature_draft_construction_graph_payloads
        .is_empty()
    {
        namespace.set_arena(
            "feature_draft_construction_graph_payloads",
            &model.features.feature_draft_construction_graph_payloads,
        )?;
    }
    if !model
        .features
        .feature_draft_construction_fixed_lanes
        .is_empty()
    {
        namespace.set_arena(
            "feature_draft_construction_fixed_lanes",
            &model.features.feature_draft_construction_fixed_lanes,
        )?;
    }
    if !model
        .features
        .feature_draft_construction_binary32_lanes
        .is_empty()
    {
        namespace.set_arena(
            "feature_draft_construction_binary32_lanes",
            &model.features.feature_draft_construction_binary32_lanes,
        )?;
    }
    if !model
        .features
        .feature_draft_construction_graph_strings
        .is_empty()
    {
        namespace.set_arena(
            "feature_draft_construction_graph_strings",
            &model.features.feature_draft_construction_graph_strings,
        )?;
    }
    if !model
        .features
        .feature_draft_construction_identity_frames
        .is_empty()
    {
        namespace.set_arena(
            "feature_draft_construction_identity_frames",
            &model.features.feature_draft_construction_identity_frames,
        )?;
    }
    if !model
        .features
        .feature_draft_construction_terminal_lanes
        .is_empty()
    {
        namespace.set_arena(
            "feature_draft_construction_terminal_lanes",
            &model.features.feature_draft_construction_terminal_lanes,
        )?;
    }
    if !model
        .features
        .feature_surface_construction_references
        .is_empty()
    {
        namespace.set_arena(
            "feature_surface_construction_references",
            &model.features.feature_surface_construction_references,
        )?;
    }
    if !model
        .features
        .feature_surface_construction_payloads
        .is_empty()
    {
        namespace.set_arena(
            "feature_surface_construction_payloads",
            &model.features.feature_surface_construction_payloads,
        )?;
    }
    if !model
        .features
        .feature_surface_construction_scalar_pairs
        .is_empty()
    {
        namespace.set_arena(
            "feature_surface_construction_scalar_pairs",
            &model.features.feature_surface_construction_scalar_pairs,
        )?;
    }
    if !model
        .features
        .feature_surface_construction_strings
        .is_empty()
    {
        namespace.set_arena(
            "feature_surface_construction_strings",
            &model.features.feature_surface_construction_strings,
        )?;
    }
    if !model
        .features
        .feature_surface_construction_branches
        .is_empty()
    {
        namespace.set_arena(
            "feature_surface_construction_branches",
            &model.features.feature_surface_construction_branches,
        )?;
    }
    if !model.features.feature_extrude_profile_references.is_empty() {
        namespace.set_arena(
            "feature_extrude_profile_references",
            &model.features.feature_extrude_profile_references,
        )?;
    }
    if !model.features.feature_extrude_payload_headers.is_empty() {
        namespace.set_arena(
            "feature_extrude_payload_headers",
            &model.features.feature_extrude_payload_headers,
        )?;
    }
    if !model.features.feature_extrude_payload_footers.is_empty() {
        namespace.set_arena(
            "feature_extrude_payload_footers",
            &model.features.feature_extrude_payload_footers,
        )?;
    }
    if !model
        .features
        .feature_operation_body_scalar_triples
        .is_empty()
    {
        namespace.set_arena(
            "feature_operation_body_scalar_triples",
            &model.features.feature_operation_body_scalar_triples,
        )?;
    }
    if !model.features.feature_operation_body_members.is_empty() {
        namespace.set_arena(
            "feature_operation_body_members",
            &model.features.feature_operation_body_members,
        )?;
    }
    if !model.features.feature_operation_body_operands.is_empty() {
        namespace.set_arena(
            "feature_operation_body_operands",
            &model.features.feature_operation_body_operands,
        )?;
    }
    if !model
        .features
        .feature_operation_body_11_continuations
        .is_empty()
    {
        namespace.set_arena(
            "feature_operation_body_11_continuations",
            &model.features.feature_operation_body_11_continuations,
        )?;
    }
    if !model
        .features
        .feature_operation_body_reference_lanes
        .is_empty()
    {
        namespace.set_arena(
            "feature_operation_body_reference_lanes",
            &model.features.feature_operation_body_reference_lanes,
        )?;
    }
    if !model
        .features
        .feature_extrude_construction_profiles
        .is_empty()
    {
        namespace.set_arena(
            "feature_extrude_construction_profiles",
            &model.features.feature_extrude_construction_profiles,
        )?;
    }
    if !model
        .features
        .feature_extrude_payload_32_branches
        .is_empty()
    {
        namespace.set_arena(
            "feature_extrude_payload_32_branches",
            &model.features.feature_extrude_payload_32_branches,
        )?;
    }
    if !model.features.feature_extrude_32_constructions.is_empty() {
        namespace.set_arena(
            "feature_extrude_32_constructions",
            &model.features.feature_extrude_32_constructions,
        )?;
    }
    if !model
        .features
        .feature_block_construction_references
        .is_empty()
    {
        namespace.set_arena(
            "feature_block_construction_references",
            &model.features.feature_block_construction_references,
        )?;
    }
    if !model.features.feature_block_constructions.is_empty() {
        namespace.set_arena(
            "feature_block_constructions",
            &model.features.feature_block_constructions,
        )?;
    }
    if !model
        .features
        .feature_block_construction_payloads
        .is_empty()
    {
        namespace.set_arena(
            "feature_block_construction_payloads",
            &model.features.feature_block_construction_payloads,
        )?;
    }
    if !model.features.feature_block_payload_scalars.is_empty() {
        namespace.set_arena(
            "feature_block_payload_scalars",
            &model.features.feature_block_payload_scalars,
        )?;
    }
    if !model.features.feature_block_payload_names.is_empty() {
        namespace.set_arena(
            "feature_block_payload_names",
            &model.features.feature_block_payload_names,
        )?;
    }
    if !model
        .features
        .feature_block_payload_named_records
        .is_empty()
    {
        namespace.set_arena(
            "feature_block_payload_named_records",
            &model.features.feature_block_payload_named_records,
        )?;
    }
    if !model.features.feature_block_payload_points.is_empty() {
        namespace.set_arena(
            "feature_block_payload_points",
            &model.features.feature_block_payload_points,
        )?;
    }
    if !model.features.feature_block_payload_point_groups.is_empty() {
        namespace.set_arena(
            "feature_block_payload_point_groups",
            &model.features.feature_block_payload_point_groups,
        )?;
    }
    if !model.features.feature_block_dimensions.is_empty() {
        namespace.set_arena(
            "feature_block_dimensions",
            &model.features.feature_block_dimensions,
        )?;
    }
    if !model.features.feature_sketch_records.is_empty() {
        namespace.set_arena(
            "feature_sketch_records",
            &model.features.feature_sketch_records,
        )?;
    }
    if !model.features.feature_sketch_construction_inputs.is_empty() {
        namespace.set_arena(
            "feature_sketch_construction_inputs",
            &model.features.feature_sketch_construction_inputs,
        )?;
    }
    if !model
        .features
        .feature_sketch_construction_payloads
        .is_empty()
    {
        namespace.set_arena(
            "feature_sketch_construction_payloads",
            &model.features.feature_sketch_construction_payloads,
        )?;
    }
    if !model
        .features
        .feature_sketch_payload_coordinate_pairs
        .is_empty()
    {
        namespace.set_arena(
            "feature_sketch_payload_coordinate_pairs",
            &model.features.feature_sketch_payload_coordinate_pairs,
        )?;
    }
    if !model.features.feature_sketch_payload_fixed_pairs.is_empty() {
        namespace.set_arena(
            "feature_sketch_payload_fixed_pairs",
            &model.features.feature_sketch_payload_fixed_pairs,
        )?;
    }
    if !model.features.feature_sketch_payload_scalars.is_empty() {
        namespace.set_arena(
            "feature_sketch_payload_scalars",
            &model.features.feature_sketch_payload_scalars,
        )?;
    }
    if !model.features.feature_sketch_payload_names.is_empty() {
        namespace.set_arena(
            "feature_sketch_payload_names",
            &model.features.feature_sketch_payload_names,
        )?;
    }
    if !model
        .features
        .feature_sketch_payload_named_records
        .is_empty()
    {
        namespace.set_arena(
            "feature_sketch_payload_named_records",
            &model.features.feature_sketch_payload_named_records,
        )?;
    }
    if !model.features.feature_sketch_points.is_empty() {
        namespace.set_arena(
            "feature_sketch_points",
            &model.features.feature_sketch_points,
        )?;
    }
    if !model.features.feature_sketch_fixed_points.is_empty() {
        namespace.set_arena(
            "feature_sketch_fixed_points",
            &model.features.feature_sketch_fixed_points,
        )?;
    }
    if !model.features.feature_sketch_point_groups.is_empty() {
        namespace.set_arena(
            "feature_sketch_point_groups",
            &model.features.feature_sketch_point_groups,
        )?;
    }
    if !model.features.offset_store_named_points.is_empty() {
        namespace.set_arena(
            "offset_store_named_points",
            &model.features.offset_store_named_points,
        )?;
    }
    if !model
        .features
        .feature_sketch_named_point_block_uses
        .is_empty()
    {
        namespace.set_arena(
            "feature_sketch_named_point_block_uses",
            &model.features.feature_sketch_named_point_block_uses,
        )?;
    }
    if !model
        .features
        .feature_sketch_preceding_named_point_uses
        .is_empty()
    {
        namespace.set_arena(
            "feature_sketch_preceding_named_point_uses",
            &model.features.feature_sketch_preceding_named_point_uses,
        )?;
    }
    if !model.features.feature_sketch_point_uses.is_empty() {
        namespace.set_arena(
            "feature_sketch_point_uses",
            &model.features.feature_sketch_point_uses,
        )?;
    }
    if !model
        .features
        .feature_sketch_datum_csys_dependencies
        .is_empty()
    {
        namespace.set_arena(
            "feature_sketch_datum_csys_dependencies",
            &model.features.feature_sketch_datum_csys_dependencies,
        )?;
    }
    if !model.features.feature_boolean_operations.is_empty() {
        namespace.set_arena(
            "feature_boolean_operations",
            &model.features.feature_boolean_operations,
        )?;
    }
    if !model.om.expression_declarations.is_empty() {
        namespace.set_arena("expression_declarations", &model.om.expression_declarations)?;
    }
    if !model.features.data_block_object_frames.is_empty() {
        namespace.set_arena(
            "data_block_object_frames",
            &model.features.data_block_object_frames,
        )?;
    }
    if !model.om.expressions.is_empty() {
        namespace.set_arena("expressions", &model.om.expressions)?;
    }
    if !model.om.classes.is_empty() {
        namespace.set_arena("class_definitions", &model.om.classes)?;
    }
    if !model.om.fields.is_empty() {
        namespace.set_arena("field_definitions", &model.om.fields)?;
    }
    if !model.om.object_records.is_empty() {
        namespace.set_arena("object_records", &model.om.object_records)?;
    }
    if !model.om.rmfastload_object_id_tables.is_empty() {
        namespace.set_arena(
            "rmfastload_object_id_tables",
            &model.om.rmfastload_object_id_tables,
        )?;
    }
    if !model.om.rmfastload_object_ids.is_empty() {
        namespace.set_arena("rmfastload_object_ids", &model.om.rmfastload_object_ids)?;
    }
    if !model.om.data_blocks.is_empty() {
        namespace.set_arena("data_blocks", &model.om.data_blocks)?;
    }
    if !model.om.data_block_control_values.is_empty() {
        namespace.set_arena(
            "data_block_control_values",
            &model.om.data_block_control_values,
        )?;
    }
    if !model.om.data_block_control_class_references.is_empty() {
        namespace.set_arena(
            "data_block_control_class_references",
            &model.om.data_block_control_class_references,
        )?;
    }
    if !model.om.data_block_control_index_values.is_empty() {
        namespace.set_arena(
            "data_block_control_index_values",
            &model.om.data_block_control_index_values,
        )?;
    }
    if !model.om.data_block_control_references.is_empty() {
        namespace.set_arena(
            "data_block_control_references",
            &model.om.data_block_control_references,
        )?;
    }
    if !model.om.data_block_control_handle_pairs.is_empty() {
        namespace.set_arena(
            "data_block_control_handle_pairs",
            &model.om.data_block_control_handle_pairs,
        )?;
    }
    if !model.om.data_block_references.is_empty() {
        namespace.set_arena("data_block_references", &model.om.data_block_references)?;
    }
    if !model.om.data_block_counted_index_lanes.is_empty() {
        namespace.set_arena(
            "data_block_counted_index_lanes",
            &model.om.data_block_counted_index_lanes,
        )?;
    }
    if !model.om.data_block_abr_reference_lanes.is_empty() {
        namespace.set_arena(
            "data_block_abr_reference_lanes",
            &model.om.data_block_abr_reference_lanes,
        )?;
    }
    if !model.om.data_block_index_rows.is_empty() {
        namespace.set_arena("data_block_index_rows", &model.om.data_block_index_rows)?;
    }
    if !model.om.data_block_linked_index_rows.is_empty() {
        namespace.set_arena(
            "data_block_linked_index_rows",
            &model.om.data_block_linked_index_rows,
        )?;
    }
    if !model.om.data_block_target_index_rows.is_empty() {
        namespace.set_arena(
            "data_block_target_index_rows",
            &model.om.data_block_target_index_rows,
        )?;
    }
    if !model.om.data_block_column_index_tables.is_empty() {
        namespace.set_arena(
            "data_block_column_index_tables",
            &model.om.data_block_column_index_tables,
        )?;
    }
    if !model.features.feature_input_column_row_uses.is_empty() {
        namespace.set_arena(
            "feature_input_column_row_uses",
            &model.features.feature_input_column_row_uses,
        )?;
    }
    if !model.features.feature_input_column_targets.is_empty() {
        namespace.set_arena(
            "feature_input_column_targets",
            &model.features.feature_input_column_targets,
        )?;
    }
    if !model.features.feature_parameter_bindings.is_empty() {
        namespace.set_arena(
            "feature_parameter_bindings",
            &model.features.feature_parameter_bindings,
        )?;
    }
    if !model.features.feature_parameter_uses.is_empty() {
        namespace.set_arena(
            "feature_parameter_uses",
            &model.features.feature_parameter_uses,
        )?;
    }
    if !model.om.store_headers.is_empty() {
        namespace.set_arena("store_headers", &model.om.store_headers)?;
    }
    if !model.om.string_values.is_empty() {
        namespace.set_arena("string_values", &model.om.string_values)?;
    }
    if !model.om.object_references.is_empty() {
        namespace.set_arena("object_references", &model.om.object_references)?;
    }
    if !model.om.persistent_handles.is_empty() {
        namespace.set_arena("persistent_handles", &model.om.persistent_handles)?;
    }
    if !model.om.configurations.is_empty() {
        namespace.set_arena("configurations", &model.om.configurations)?;
    }
    if !model.om.configuration_attribute_uses.is_empty() {
        namespace.set_arena(
            "configuration_attribute_uses",
            &model.om.configuration_attribute_uses,
        )?;
    }
    if !model.om.part_attributes.is_empty() {
        namespace.set_arena("part_attributes", &model.om.part_attributes)?;
    }
    if !model.om.external_references.is_empty() {
        namespace.set_arena("external_references", &model.om.external_references)?;
    }
    if !model.om.external_reference_records.is_empty() {
        namespace.set_arena(
            "external_reference_records",
            &model.om.external_reference_records,
        )?;
    }
    if !model.om.external_reference_indexed_records.is_empty() {
        namespace.set_arena(
            "external_reference_indexed_records",
            &model.om.external_reference_indexed_records,
        )?;
    }
    if !model.om.external_reference_empty_records.is_empty() {
        namespace.set_arena(
            "external_reference_empty_records",
            &model.om.external_reference_empty_records,
        )?;
    }
    if !model.om.external_reference_tail_reference_pairs.is_empty() {
        namespace.set_arena(
            "external_reference_tail_reference_pairs",
            &model.om.external_reference_tail_reference_pairs,
        )?;
    }
    if !model.om.external_reference_record_string_uses.is_empty() {
        namespace.set_arena(
            "external_reference_record_string_uses",
            &model.om.external_reference_record_string_uses,
        )?;
    }
    if !model.om.external_reference_record_children.is_empty() {
        namespace.set_arena(
            "external_reference_record_children",
            &model.om.external_reference_record_children,
        )?;
    }
    if !model.om.material_texture_assets.is_empty() {
        namespace.set_arena("material_texture_assets", &model.om.material_texture_assets)?;
    }
    if !model.om.material_texture_catalog_entries.is_empty() {
        namespace.set_arena(
            "material_texture_catalog_entries",
            &model.om.material_texture_catalog_entries,
        )?;
    }
    Ok(())
}

fn attach_feature_operations(
    ir: &mut CadIr,
    features: &crate::native::FeatureRecords,
    expressions: &[crate::native::Expression],
    body_bindings: &[crate::native::SegmentBodyBinding],
    annotations: &mut AnnotationBuilder,
) {
    let labels = features.feature_operation_labels.as_slice();
    let booleans = features.feature_boolean_operations.as_slice();
    let body_references = features.feature_body_references.as_slice();
    let body_reference_occurrences = features.feature_body_reference_occurrences.as_slice();
    let input_blocks = features.feature_input_blocks.as_slice();
    let input_block_identity_groups = features.feature_input_block_identity_groups.as_slice();
    let datum_csys_constructions = features.feature_datum_csys_constructions.as_slice();
    let datum_csys_payloads = features.feature_datum_csys_payloads.as_slice();
    let datum_csys_block_uses = features.feature_datum_csys_block_uses.as_slice();
    let datum_plane_headers = features.feature_datum_plane_headers.as_slice();
    let datum_plane_block_uses = features.feature_datum_plane_block_uses.as_slice();
    let datum_plane_payloads = features.feature_datum_plane_payloads.as_slice();
    let datum_plane_csys_identity_uses = features.feature_datum_plane_csys_identity_uses.as_slice();
    let sketch_datum_csys_dependencies = features.feature_sketch_datum_csys_dependencies.as_slice();
    let sketch_references = features.feature_sketch_references.as_slice();
    let projected_curve_references = features.feature_projected_curve_references.as_slice();
    let projected_curve_construction_payloads = features
        .feature_projected_curve_construction_payloads
        .as_slice();
    let projected_curve_construction_strings = features
        .feature_projected_curve_construction_strings
        .as_slice();
    let pattern_references = features.feature_pattern_references.as_slice();
    let pattern_construction_payloads = features.feature_pattern_construction_payloads.as_slice();
    let pattern_construction_strings = features.feature_pattern_construction_strings.as_slice();
    let pattern_construction_fixed_lanes =
        features.feature_pattern_construction_fixed_lanes.as_slice();
    let pattern_transform_lanes = features.feature_pattern_transform_lanes.as_slice();
    let point_construction_headers = features.feature_point_construction_headers.as_slice();
    let point_construction_scalar_lanes =
        features.feature_point_construction_scalar_lanes.as_slice();
    let draft_construction_references = features.feature_draft_construction_references.as_slice();
    let draft_construction_index_lanes = features.feature_draft_construction_index_lanes.as_slice();
    let draft_construction_payloads = features.feature_draft_construction_payloads.as_slice();
    let draft_construction_graph_payloads = features
        .feature_draft_construction_graph_payloads
        .as_slice();
    let draft_construction_fixed_lanes = features.feature_draft_construction_fixed_lanes.as_slice();
    let draft_construction_binary32_lanes = features
        .feature_draft_construction_binary32_lanes
        .as_slice();
    let draft_construction_graph_strings =
        features.feature_draft_construction_graph_strings.as_slice();
    let draft_construction_identity_frames = features
        .feature_draft_construction_identity_frames
        .as_slice();
    let draft_construction_terminal_lanes = features
        .feature_draft_construction_terminal_lanes
        .as_slice();
    let surface_construction_references =
        features.feature_surface_construction_references.as_slice();
    let surface_construction_payloads = features.feature_surface_construction_payloads.as_slice();
    let surface_construction_scalar_pairs = features
        .feature_surface_construction_scalar_pairs
        .as_slice();
    let surface_construction_strings = features.feature_surface_construction_strings.as_slice();
    let surface_construction_branches = features.feature_surface_construction_branches.as_slice();
    let sketch_named_point_block_uses = features.feature_sketch_named_point_block_uses.as_slice();
    let sketch_preceding_named_point_uses = features
        .feature_sketch_preceding_named_point_uses
        .as_slice();
    let sketch_point_uses = features.feature_sketch_point_uses.as_slice();
    let sketch_point_groups = features.feature_sketch_point_groups.as_slice();
    let extrude_profile_references = features.feature_extrude_profile_references.as_slice();
    let extrude_construction_profiles = features.feature_extrude_construction_profiles.as_slice();
    let operation_body_operands = features.feature_operation_body_operands.as_slice();
    let sketch_construction_inputs = features.feature_sketch_construction_inputs.as_slice();
    let sketch_records = features.feature_sketch_records.as_slice();
    let sketch_construction_payloads = features.feature_sketch_construction_payloads.as_slice();
    let sketch_coordinate_pairs = features.feature_sketch_payload_coordinate_pairs.as_slice();
    let sketch_fixed_pairs = features.feature_sketch_payload_fixed_pairs.as_slice();
    let sketch_fixed_points = features.feature_sketch_fixed_points.as_slice();
    let block_constructions = features.feature_block_constructions.as_slice();
    let block_construction_payloads = features.feature_block_construction_payloads.as_slice();
    let block_dimensions = features.feature_block_dimensions.as_slice();
    let block_payload_points = features.feature_block_payload_points.as_slice();
    let block_payload_point_groups = features.feature_block_payload_point_groups.as_slice();
    let extrude_32_constructions = features.feature_extrude_32_constructions.as_slice();
    let extrude_payload_headers = features.feature_extrude_payload_headers.as_slice();
    let extrude_payload_footers = features.feature_extrude_payload_footers.as_slice();
    let extrude_payload_32_branches = features.feature_extrude_payload_32_branches.as_slice();
    let operation_body_scalar_triples = features.feature_operation_body_scalar_triples.as_slice();
    let operation_body_members = features.feature_operation_body_members.as_slice();
    let operation_body_11_continuations =
        features.feature_operation_body_11_continuations.as_slice();
    let operation_body_reference_lanes = features.feature_operation_body_reference_lanes.as_slice();
    let parameter_bindings = features.feature_parameter_bindings.as_slice();
    let parameter_uses = features.feature_parameter_uses.as_slice();
    let operation_records = features.feature_operation_records.as_slice();
    let payload_strings = features.feature_payload_strings.as_slice();
    let simple_hole_templates = features.feature_simple_hole_templates.as_slice();
    let simple_hole_repeated_scalar_lanes = features
        .feature_simple_hole_repeated_scalar_lanes
        .as_slice();
    let simple_hole_repeated_scalar_lane_block_references = features
        .feature_simple_hole_repeated_scalar_lane_block_references
        .as_slice();
    let simple_hole_construction_groups =
        features.feature_simple_hole_construction_groups.as_slice();
    let stream = annotations.stream("nx:container");
    let base_ordinal = ir.model.features.len() as u64;
    let booleans = booleans
        .iter()
        .map(|operation| (operation.operation_label.as_str(), operation))
        .collect::<BTreeMap<_, _>>();
    let body_references = body_references
        .iter()
        .map(|reference| {
            (
                reference.operation_label.as_str(),
                reference.body_object_index,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut body_reference_occurrences_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureBodyReferenceOccurrence>>::new();
    for reference in body_reference_occurrences {
        body_reference_occurrences_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut last_body_writer = BTreeMap::<u32, FeatureId>::new();
    let body_alias_roots = crate::native::body_alias_roots(body_bindings).unwrap_or_default();
    let canonical_body =
        |identity: u32| body_alias_roots.get(&identity).copied().unwrap_or(identity);
    let mut input_blocks_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureInputBlock>>::new();
    for input in input_blocks {
        input_blocks_by_operation
            .entry(input.operation_label.as_str())
            .or_default()
            .push(input);
    }
    let input_block_identity_group_by_input = input_block_identity_groups
        .iter()
        .flat_map(|group| {
            group
                .input_blocks
                .iter()
                .map(move |input| (input.as_str(), group.id.as_str()))
        })
        .collect::<BTreeMap<_, _>>();
    let datum_csys_constructions_by_operation = datum_csys_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        .collect::<BTreeMap<_, _>>();
    let datum_csys_payloads_by_operation =
        records_by_operation(datum_csys_payloads, |payload| &payload.operation_label);
    let mut datum_csys_uses_by_input_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureDatumCsysBlockUse>>::new();
    for block_use in datum_csys_block_uses {
        datum_csys_uses_by_input_operation
            .entry(block_use.input_operation_label.as_str())
            .or_default()
            .push(block_use);
    }
    let datum_plane_headers_by_operation = datum_plane_headers
        .iter()
        .map(|header| (header.operation_label.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    let datum_plane_payloads_by_operation = datum_plane_payloads
        .iter()
        .map(|payload| (payload.operation_label.as_str(), payload))
        .collect::<BTreeMap<_, _>>();
    let mut datum_plane_uses_by_input_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureDatumPlaneBlockUse>>::new();
    for block_use in datum_plane_block_uses {
        datum_plane_uses_by_input_operation
            .entry(block_use.input_operation_label.as_str())
            .or_default()
            .push(block_use);
    }
    let operation_positions = labels
        .iter()
        .enumerate()
        .map(|(position, label)| (label.id.as_str(), position))
        .collect::<BTreeMap<_, _>>();
    let feature_ids_by_operation = labels
        .iter()
        .filter(|label| projects_neutral_feature(&label.value))
        .map(|label| {
            let key = label
                .id
                .strip_prefix("nx:feature-history:operation-label#")
                .unwrap_or(label.id.as_str());
            (
                label.id.as_str(),
                FeatureId(format!("nx:feature-history:feature#{key}")),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let sketch_datum_csys_dependencies = sketch_datum_csys_dependencies
        .iter()
        .map(|dependency| (dependency.datum_csys_operation_label.as_str(), dependency))
        .collect::<BTreeMap<_, _>>();
    let mut datum_identity_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureDatumPlaneCsysIdentityUse>>::new();
    for identity_use in datum_plane_csys_identity_uses {
        datum_identity_uses_by_operation
            .entry(identity_use.datum_plane_operation_label.as_str())
            .or_default()
            .push(identity_use);
        datum_identity_uses_by_operation
            .entry(identity_use.datum_csys_operation_label.as_str())
            .or_default()
            .push(identity_use);
    }
    let mut sketch_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchReference>>::new();
    for reference in sketch_references {
        sketch_references_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let projected_curve_references_by_operation =
        records_by_operation(projected_curve_references, |reference| {
            &reference.operation_label
        });
    let projected_curve_construction_payloads_by_operation =
        records_by_operation(projected_curve_construction_payloads, |payload| {
            &payload.operation_label
        });
    let projected_curve_construction_strings_by_operation =
        records_by_operation(projected_curve_construction_strings, |value| {
            &value.operation_label
        });
    let pattern_references_by_operation =
        records_by_operation(pattern_references, |reference| &reference.operation_label);
    let pattern_construction_payloads_by_operation =
        records_by_operation(pattern_construction_payloads, |payload| {
            &payload.operation_label
        });
    let pattern_construction_strings_by_operation =
        records_by_operation(pattern_construction_strings, |value| &value.operation_label);
    let pattern_construction_fixed_lanes_by_operation =
        records_by_operation(pattern_construction_fixed_lanes, |lane| {
            &lane.operation_label
        });
    let pattern_transform_lanes_by_operation =
        records_by_operation(pattern_transform_lanes, |lane| &lane.operation_label);
    let point_construction_headers_by_operation = point_construction_headers
        .iter()
        .map(|header| (header.operation_label.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    let point_construction_scalar_lanes_by_operation = point_construction_scalar_lanes
        .iter()
        .map(|lane| (lane.operation_label.as_str(), lane))
        .collect::<BTreeMap<_, _>>();
    let draft_construction_references_by_operation =
        records_by_operation(draft_construction_references, |reference| {
            &reference.operation_label
        });
    let draft_construction_index_lanes_by_operation =
        records_by_operation(draft_construction_index_lanes, |lane| &lane.operation_label);
    let draft_construction_payloads_by_operation =
        records_by_operation(draft_construction_payloads, |payload| {
            &payload.operation_label
        });
    let draft_construction_graph_payloads_by_operation =
        records_by_operation(draft_construction_graph_payloads, |payload| {
            &payload.operation_label
        });
    let draft_construction_fixed_lanes_by_operation =
        records_by_operation(draft_construction_fixed_lanes, |lane| &lane.operation_label);
    let draft_construction_binary32_lanes_by_operation =
        records_by_operation(draft_construction_binary32_lanes, |lane| {
            &lane.operation_label
        });
    let draft_construction_graph_strings_by_operation =
        records_by_operation(draft_construction_graph_strings, |value| {
            &value.operation_label
        });
    let draft_construction_identity_frames_by_operation =
        records_by_operation(draft_construction_identity_frames, |frame| {
            &frame.operation_label
        });
    let draft_construction_terminal_lanes_by_operation =
        records_by_operation(draft_construction_terminal_lanes, |lane| {
            &lane.operation_label
        });
    let surface_construction_references_by_operation =
        records_by_operation(surface_construction_references, |reference| {
            &reference.operation_label
        });
    let surface_construction_payloads_by_operation =
        records_by_operation(surface_construction_payloads, |payload| {
            &payload.operation_label
        });
    let surface_construction_scalar_pairs_by_operation =
        records_by_operation(surface_construction_scalar_pairs, |pair| {
            &pair.operation_label
        });
    let surface_construction_strings_by_operation =
        records_by_operation(surface_construction_strings, |value| &value.operation_label);
    let surface_construction_branches_by_operation =
        records_by_operation(surface_construction_branches, |branch| {
            &branch.operation_label
        });
    let mut sketch_named_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchNamedPointBlockUse>>::new();
    for block_use in sketch_named_point_block_uses {
        sketch_named_point_uses_by_operation
            .entry(block_use.operation_label.as_str())
            .or_default()
            .push(block_use);
    }
    let mut sketch_preceding_named_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPrecedingNamedPointUse>>::new();
    for point_use in sketch_preceding_named_point_uses {
        sketch_preceding_named_point_uses_by_operation
            .entry(point_use.operation_label.as_str())
            .or_default()
            .push(point_use);
    }
    let mut sketch_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPointUse>>::new();
    for point_use in sketch_point_uses {
        sketch_point_uses_by_operation
            .entry(point_use.operation_label.as_str())
            .or_default()
            .push(point_use);
    }
    let mut sketch_point_groups_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPointGroup>>::new();
    for group in sketch_point_groups {
        sketch_point_groups_by_operation
            .entry(group.operation_label.as_str())
            .or_default()
            .push(group);
    }
    let mut extrude_profile_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureExtrudeProfileReference>>::new();
    for reference in extrude_profile_references {
        extrude_profile_references_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let extrude_construction_profiles_by_operation = extrude_construction_profiles
        .iter()
        .map(|profile| (profile.operation_label.as_str(), profile))
        .collect::<BTreeMap<_, _>>();
    let mut operation_body_operands_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureOperationBodyOperand>>::new();
    for operand in operation_body_operands {
        operation_body_operands_by_operation
            .entry(operand.operation_label.as_str())
            .or_default()
            .push(operand);
    }
    let sketch_construction_inputs_by_operation = sketch_construction_inputs
        .iter()
        .map(|inputs| (inputs.operation_label.as_str(), inputs))
        .collect::<BTreeMap<_, _>>();
    let sketch_records_by_operation =
        records_by_operation(sketch_records, |record| &record.operation_label);
    let sketch_construction_payloads_by_operation =
        records_by_operation(sketch_construction_payloads, |payload| {
            &payload.operation_label
        });
    let mut sketch_coordinate_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPayloadCoordinatePair>>::new();
    for pair in sketch_coordinate_pairs {
        sketch_coordinate_pairs_by_operation
            .entry(pair.operation_label.as_str())
            .or_default()
            .push(pair);
    }
    let mut sketch_fixed_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPayloadFixedPair>>::new();
    for pair in sketch_fixed_pairs {
        sketch_fixed_pairs_by_operation
            .entry(pair.operation_label.as_str())
            .or_default()
            .push(pair);
    }
    let mut sketch_fixed_points_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchFixedPoint>>::new();
    for point in sketch_fixed_points {
        sketch_fixed_points_by_operation
            .entry(point.operation_label.as_str())
            .or_default()
            .push(point);
    }
    let block_constructions_by_operation = block_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        .collect::<BTreeMap<_, _>>();
    let block_construction_payloads_by_operation =
        records_by_operation(block_construction_payloads, |payload| {
            &payload.operation_label
        });
    let block_dimensions_by_operation = block_dimensions
        .iter()
        .map(|dimensions| (dimensions.operation_label.as_str(), dimensions))
        .collect::<BTreeMap<_, _>>();
    let mut block_payload_points_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureBlockPayloadPoint>>::new();
    for point in block_payload_points {
        block_payload_points_by_operation
            .entry(point.operation_label.as_str())
            .or_default()
            .push(point);
    }
    let mut block_payload_point_groups_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureBlockPayloadPointGroup>>::new();
    for group in block_payload_point_groups {
        block_payload_point_groups_by_operation
            .entry(group.operation_label.as_str())
            .or_default()
            .push(group);
    }
    let extrude_32_constructions_by_operation = extrude_32_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        .collect::<BTreeMap<_, _>>();
    let extrude_payload_headers_by_operation = extrude_payload_headers
        .iter()
        .map(|header| (header.operation_label.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    let extrude_payload_footers_by_operation = extrude_payload_footers
        .iter()
        .map(|footer| (footer.operation_label.as_str(), footer))
        .collect::<BTreeMap<_, _>>();
    let extrude_payload_32_branches_by_operation =
        records_by_operation(extrude_payload_32_branches, |branch| {
            &branch.operation_label
        });
    let mut operation_body_scalar_triples_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureOperationBodyScalarTriple>>::new();
    for triple in operation_body_scalar_triples {
        operation_body_scalar_triples_by_operation
            .entry(triple.operation_label.as_str())
            .or_default()
            .push(triple);
    }
    for triples in operation_body_scalar_triples_by_operation.values_mut() {
        triples.sort_by_key(|triple| triple.body_reference_ordinal);
    }
    let mut operation_body_members_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureOperationBodyMember>>::new();
    for member in operation_body_members {
        operation_body_members_by_operation
            .entry(member.operation_label.as_str())
            .or_default()
            .push(member);
    }
    let mut operation_body_11_continuations_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureOperationBody11Continuation>>::new();
    for continuation in operation_body_11_continuations {
        operation_body_11_continuations_by_operation
            .entry(continuation.operation_label.as_str())
            .or_default()
            .push(continuation);
    }
    let mut operation_body_reference_lanes_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureOperationBodyReferenceLane>>::new();
    for lane in operation_body_reference_lanes {
        operation_body_reference_lanes_by_operation
            .entry(lane.operation_label.as_str())
            .or_default()
            .push(lane);
    }
    let mut bodies_by_object_index = BTreeMap::<u32, Vec<BodyId>>::new();
    for binding in body_bindings {
        let prefix = format!("nx:s{}:", binding.stream_ordinal);
        let mut stream_bodies = Vec::new();
        for body in ir
            .model
            .bodies
            .iter()
            .filter(|body| body.id.0.starts_with(&prefix))
        {
            if !stream_bodies.contains(&body.id) {
                stream_bodies.push(body.id.clone());
            }
        }
        for identity in [binding.body_object_index, binding.body_alias_object_index] {
            let bodies = bodies_by_object_index.entry(identity).or_default();
            for body in &stream_bodies {
                if !bodies.contains(body) {
                    bodies.push(body.clone());
                }
            }
        }
    }
    let hole_outputs = simple_hole_templates
        .iter()
        .filter_map(|template| {
            let object_index = body_references.get(template.operation_label.as_str())?;
            Some((
                template.operation_label.clone(),
                feature_body_outputs(*object_index, &bodies_by_object_index),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let simple_hole_operations =
        simple_hole_operations(simple_hole_templates, simple_hole_construction_groups)
            .unwrap_or_default();
    let simple_hole_diameters = simple_hole_diameters(
        ir,
        simple_hole_templates,
        simple_hole_construction_groups,
        &hole_outputs,
    );
    let simple_hole_directions =
        hole_directions_for_operations(ir, &simple_hole_operations, &hole_outputs);
    let simple_hole_positions =
        hole_positions_for_operations(ir, &simple_hole_operations, &hole_outputs);
    let hole_package_operations = labels
        .iter()
        .filter(|label| label.value == "HOLE PACKAGE")
        .map(|label| label.id.clone())
        .collect::<Vec<_>>();
    let hole_package_outputs = hole_package_operations
        .iter()
        .filter_map(|operation| {
            let object_index = body_references.get(operation.as_str())?;
            Some((
                operation.clone(),
                feature_body_outputs(*object_index, &bodies_by_object_index),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let hole_package_diameters =
        hole_diameters_for_operations(ir, &hole_package_operations, &hole_package_outputs);
    let hole_package_directions =
        hole_directions_for_operations(ir, &hole_package_operations, &hole_package_outputs);
    let hole_package_positions =
        hole_positions_for_operations(ir, &hole_package_operations, &hole_package_outputs);
    let simple_hole_chamfers = simple_hole_chamfers(ir, simple_hole_templates, &hole_outputs);
    let mut parameter_bindings_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureParameterBinding>>::new();
    for binding in parameter_bindings {
        parameter_bindings_by_operation
            .entry(binding.operation_label.as_str())
            .or_default()
            .push(binding);
    }
    let mut parameter_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureParameterUse>>::new();
    for parameter_use in parameter_uses {
        parameter_uses_by_operation
            .entry(parameter_use.operation_label.as_str())
            .or_default()
            .push(parameter_use);
    }
    let operation_labels_by_record = operation_records
        .iter()
        .map(|record| (record.id.as_str(), record.operation_label.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut payload_strings_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeaturePayloadString>>::new();
    for value in payload_strings {
        let Some(operation) = operation_labels_by_record.get(value.operation_record.as_str())
        else {
            continue;
        };
        payload_strings_by_operation
            .entry(operation)
            .or_default()
            .push(value);
    }
    let parameter_owners = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect::<BTreeMap<_, _>>();
    for (ordinal, label) in labels.iter().enumerate() {
        if !projects_neutral_feature(&label.value) {
            continue;
        }
        let id = feature_ids_by_operation
            .get(label.id.as_str())
            .expect("every operation label owns one neutral feature identity")
            .clone();
        let mut dependencies = Vec::new();
        if let Some(body) = body_references.get(label.id.as_str()) {
            if let Some(writer) = last_body_writer.get(&canonical_body(*body)) {
                dependencies.push(writer.clone());
            }
        }
        if let Some(operation) = booleans.get(label.id.as_str()) {
            for body in &operation.tool_object_indices {
                if let Some(writer) = last_body_writer.get(&canonical_body(*body)) {
                    if !dependencies.contains(writer) {
                        dependencies.push(writer.clone());
                    }
                }
            }
        }
        for operand in operation_body_operands_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            if let Some(writer) =
                last_body_writer.get(&canonical_body(operand.operand_object_index))
            {
                if !dependencies.contains(writer) {
                    dependencies.push(writer.clone());
                }
            }
        }
        for block_use in datum_plane_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            let Some(dependency) = preceding_operation_dependency(
                block_use.construction_operation_label.as_str(),
                ordinal,
                &operation_positions,
                &feature_ids_by_operation,
            ) else {
                continue;
            };
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
        }
        for block_use in datum_csys_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            let Some(dependency) = preceding_operation_dependency(
                block_use.construction_operation_label.as_str(),
                ordinal,
                &operation_positions,
                &feature_ids_by_operation,
            ) else {
                continue;
            };
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
        }
        for identity_use in datum_identity_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            let other = if identity_use.datum_plane_operation_label == label.id {
                identity_use.datum_csys_operation_label.as_str()
            } else {
                identity_use.datum_plane_operation_label.as_str()
            };
            let Some(dependency) = preceding_operation_dependency(
                other,
                ordinal,
                &operation_positions,
                &feature_ids_by_operation,
            ) else {
                continue;
            };
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
        }
        if let Some(dependency) = sketch_datum_csys_dependencies.get(label.id.as_str()) {
            if let Some(feature) = feature_ids_by_operation
                .get(dependency.sketch_operation_label.as_str())
                .cloned()
            {
                if !dependencies.contains(&feature) {
                    dependencies.push(feature);
                }
            }
        }
        let mut source_properties = BTreeMap::new();
        for (use_ordinal, block_use) in datum_csys_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("datum_csys_block_use.{use_ordinal}"),
                block_use.id.clone(),
            );
        }
        if let Some(dependency) = sketch_datum_csys_dependencies.get(label.id.as_str()) {
            source_properties.insert(
                "sketch_point_dependency_use".to_string(),
                dependency.sketch_point_use.clone(),
            );
            match &dependency.block_relation {
                crate::native::FeatureSketchDatumCsysBlockRelation::Shared { data_block } => {
                    source_properties.insert(
                        "sketch_point_dependency_shared_block".to_string(),
                        data_block.clone(),
                    );
                }
                crate::native::FeatureSketchDatumCsysBlockRelation::Consecutive {
                    point_data_block,
                    construction_data_block,
                } => {
                    source_properties.insert(
                        "sketch_point_dependency_point_block".to_string(),
                        point_data_block.clone(),
                    );
                    source_properties.insert(
                        "sketch_point_dependency_construction_block".to_string(),
                        construction_data_block.clone(),
                    );
                }
            }
            source_properties.insert(
                "sketch_datum_csys_dependency".to_string(),
                dependency.id.clone(),
            );
        }
        let deletes_body = label.value == "DELETE";
        let outputs = if deletes_body {
            Vec::new()
        } else {
            body_references
                .get(label.id.as_str())
                .map_or_else(Vec::new, |body| {
                    feature_body_outputs(*body, &bodies_by_object_index)
                })
        };
        if let Some(body) = body_references.get(label.id.as_str()) {
            source_properties.insert("primary_body_object_index".to_string(), body.to_string());
        }
        for reference in body_reference_occurrences_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("body_reference.{}", reference.ordinal),
                reference.body_object_index.to_string(),
            );
        }
        if let Some(inputs) = sketch_construction_inputs_by_operation.get(label.id.as_str()) {
            source_properties.insert("sketch_construction_inputs".to_string(), inputs.id.clone());
        }
        for (ordinal, record) in sketch_records_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("sketch_record.{ordinal}"), record.id.clone());
        }
        for (ordinal, payload) in sketch_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("sketch_construction_payload.{ordinal}"),
                payload.id.clone(),
            );
        }
        for pair in sketch_coordinate_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("sketch_coordinate_pair.{}", pair.ordinal),
                pair.id.clone(),
            );
        }
        for pair in sketch_fixed_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("sketch_fixed_pair.{}", pair.ordinal),
                pair.id.clone(),
            );
        }
        for (ordinal, point) in sketch_fixed_points_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("sketch_fixed_point.{ordinal}"), point.id.clone());
        }
        if let Some(construction) = block_constructions_by_operation.get(label.id.as_str()) {
            source_properties.insert("block_construction".to_string(), construction.id.clone());
        }
        for (ordinal, payload) in block_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("block_construction_payload.{ordinal}"),
                payload.id.clone(),
            );
        }
        if let Some(dimensions) = block_dimensions_by_operation.get(label.id.as_str()) {
            source_properties.insert("block_dimensions".to_string(), dimensions.id.clone());
            for (dimension_ordinal, (declaration, expression)) in dimensions
                .declarations
                .iter()
                .zip(&dimensions.expressions)
                .enumerate()
            {
                source_properties.insert(
                    format!("block_dimension_declaration.{dimension_ordinal}"),
                    declaration.clone(),
                );
                source_properties.insert(
                    format!("block_dimension_expression.{dimension_ordinal}"),
                    expression.clone(),
                );
            }
        }
        for (ordinal, point) in block_payload_points_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("block_payload_point.{ordinal}"), point.id.clone());
        }
        for (ordinal, group) in block_payload_point_groups_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("block_payload_point_group.{ordinal}"),
                group.id.clone(),
            );
        }
        if let Some(construction) = extrude_32_constructions_by_operation.get(label.id.as_str()) {
            source_properties.insert(
                "extrude_32_construction".to_string(),
                construction.id.clone(),
            );
        }
        if let Some(header) = extrude_payload_headers_by_operation.get(label.id.as_str()) {
            source_properties.insert("extrude_payload_header".to_string(), header.id.clone());
        }
        if let Some(footer) = extrude_payload_footers_by_operation.get(label.id.as_str()) {
            source_properties.insert("extrude_payload_footer".to_string(), footer.id.clone());
        }
        for (ordinal, branch) in extrude_payload_32_branches_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("extrude_payload_32_branch.{ordinal}"),
                branch.id.clone(),
            );
        }
        for triple in operation_body_scalar_triples_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "operation_body_scalar_triple.{}",
                    triple.body_reference_ordinal
                ),
                triple.id.clone(),
            );
        }
        for member in operation_body_members_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "operation_body_member.{}.{}",
                    member.body_reference_ordinal, member.ordinal
                ),
                member.id.clone(),
            );
        }
        for continuation in operation_body_11_continuations_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "operation_body_11_continuation.{}",
                    continuation.body_reference_ordinal
                ),
                continuation.id.clone(),
            );
        }
        for lane in operation_body_reference_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "operation_body_reference_lane.{}",
                    lane.body_reference_ordinal
                ),
                lane.id.clone(),
            );
        }
        if let Some(construction) = datum_csys_constructions_by_operation.get(label.id.as_str()) {
            source_properties.insert(
                "datum_csys_construction".to_string(),
                construction.id.clone(),
            );
        }
        for (ordinal, payload) in datum_csys_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("datum_csys_payload.{ordinal}"), payload.id.clone());
        }
        if let Some(header) = datum_plane_headers_by_operation.get(label.id.as_str()) {
            source_properties.insert("datum_plane_header".to_string(), header.id.clone());
        }
        if let Some(payload) = datum_plane_payloads_by_operation.get(label.id.as_str()) {
            source_properties.insert("datum_plane_payload".to_string(), payload.id.clone());
        }
        for (ordinal, identity_use) in datum_identity_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("datum_identity_use.{ordinal}"),
                identity_use.id.clone(),
            );
        }
        for (use_ordinal, block_use) in datum_plane_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("datum_plane_block_use.{use_ordinal}"),
                block_use.id.clone(),
            );
        }
        source_properties.extend(simple_hole_native_properties(
            &label.id,
            simple_hole_templates,
            simple_hole_repeated_scalar_lanes,
            simple_hole_repeated_scalar_lane_block_references,
            simple_hole_construction_groups,
        ));
        for (slot, value) in label.object_indices.iter().enumerate() {
            source_properties.insert(
                format!("object_index.{slot}"),
                value.map_or_else(|| "null".to_string(), |value| value.to_string()),
            );
        }
        for input in input_blocks_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("input_block.{}", input.input_slot),
                input.data_block.clone(),
            );
            if let Some(group) = input_block_identity_group_by_input.get(input.id.as_str()) {
                source_properties.insert(
                    format!("input_block_identity_group.{}", input.input_slot),
                    (*group).to_string(),
                );
            }
        }
        for reference in sketch_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("sketch_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        for reference in projected_curve_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("projected_curve_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        for payload in projected_curve_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                "projected_curve_construction_payload".to_string(),
                payload.id.clone(),
            );
        }
        for value in projected_curve_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("projected_curve_construction_string.{}", value.ordinal),
                value.id.clone(),
            );
        }
        for reference in pattern_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("pattern_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        for payload in pattern_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                "pattern_construction_payload".to_string(),
                payload.id.clone(),
            );
        }
        for value in pattern_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("pattern_construction_string.{}", value.ordinal),
                value.id.clone(),
            );
        }
        for lane in pattern_construction_fixed_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("pattern_construction_fixed_lane.{}", lane.ordinal),
                lane.id.clone(),
            );
        }
        for lane in pattern_transform_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert("pattern_transform_lane".to_string(), lane.id.clone());
        }
        if let Some(header) = point_construction_headers_by_operation.get(label.id.as_str()) {
            source_properties.insert("point_construction_header".to_string(), header.id.clone());
            source_properties.insert(
                "point_construction_reference".to_string(),
                header
                    .data_block
                    .clone()
                    .unwrap_or_else(|| header.object_index.to_string()),
            );
            source_properties.insert(
                "point_construction_mode".to_string(),
                format!("{:02x}", header.mode),
            );
        }
        if let Some(lane) = point_construction_scalar_lanes_by_operation.get(label.id.as_str()) {
            source_properties.insert(
                "point_construction_scalar_lane".to_string(),
                lane.id.clone(),
            );
        }
        for reference in draft_construction_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("draft_construction_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        for lane in draft_construction_index_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert("draft_construction_index_lane".to_string(), lane.id.clone());
        }
        for payload in draft_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert("draft_construction_payload".to_string(), payload.id.clone());
        }
        for payload in draft_construction_graph_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                "draft_construction_graph_payload".to_string(),
                payload.id.clone(),
            );
        }
        for lane in draft_construction_fixed_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("draft_construction_fixed_lane.{}", lane.ordinal),
                lane.id.clone(),
            );
        }
        for lane in draft_construction_binary32_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("draft_construction_binary32_lane.{}", lane.ordinal),
                lane.id.clone(),
            );
        }
        for value in draft_construction_graph_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("draft_construction_graph_string.{}", value.ordinal),
                value.id.clone(),
            );
        }
        for frame in draft_construction_identity_frames_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("draft_construction_identity_frame.{}", frame.ordinal),
                frame.id.clone(),
            );
        }
        for lane in draft_construction_terminal_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                "draft_construction_terminal_lane".to_string(),
                lane.id.clone(),
            );
        }
        for reference in surface_construction_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("surface_construction_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        for payload in surface_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                "surface_construction_payload".to_string(),
                payload.id.clone(),
            );
        }
        for pair in surface_construction_scalar_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("surface_construction_scalar_pair.{}", pair.ordinal),
                pair.id.clone(),
            );
        }
        for value in surface_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("surface_construction_string.{}", value.ordinal),
                value.id.clone(),
            );
        }
        for branch in surface_construction_branches_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            for member in &branch.members {
                source_properties.insert(
                    format!(
                        "surface_construction_branch.{}.member.{}",
                        branch.ordinal, member.ordinal
                    ),
                    member
                        .data_block
                        .clone()
                        .unwrap_or_else(|| member.object_index.to_string()),
                );
            }
            source_properties.insert(
                format!("surface_construction_branch.{}.terminal", branch.ordinal),
                branch
                    .terminal
                    .data_block
                    .clone()
                    .unwrap_or_else(|| branch.terminal.object_index.to_string()),
            );
        }
        for (ordinal, block_use) in sketch_named_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("sketch_named_point_block_use.{ordinal}"),
                block_use.id.clone(),
            );
        }
        for (ordinal, point_use) in sketch_preceding_named_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("sketch_preceding_named_point_use.{ordinal}"),
                point_use.id.clone(),
            );
        }
        for (ordinal, point_use) in sketch_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("sketch_point_use.{ordinal}"), point_use.id.clone());
        }
        for (ordinal, group) in sketch_point_groups_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("sketch_point_group.{ordinal}"), group.id.clone());
        }
        for reference in extrude_profile_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("extrude_profile_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        if let Some(profile) = extrude_construction_profiles_by_operation.get(label.id.as_str()) {
            source_properties.insert(
                "extrude_construction_profile".to_string(),
                profile.id.clone(),
            );
        }
        for operand in operation_body_operands_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                operand.source_property_key(),
                operand.operand_object_index.to_string(),
            );
        }
        for binding in parameter_bindings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "input_parameter_declaration.{}.{}",
                    binding.input_slot, binding.reference_ordinal
                ),
                binding.expression_declaration.clone(),
            );
            if let Some(expression) = &binding.expression {
                source_properties.insert(
                    format!(
                        "input_parameter_expression.{}.{}",
                        binding.input_slot, binding.reference_ordinal
                    ),
                    expression.clone(),
                );
            }
        }
        for (ordinal, parameter_use) in parameter_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("parameter_use.{ordinal}"), parameter_use.id.clone());
        }
        let operation_payload_string_records = payload_strings_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let operation_payload_strings = operation_payload_string_records
            .iter()
            .map(|value| value.value.as_str())
            .collect::<Vec<_>>();
        let block_dimension_values = block_dimensions_by_operation
            .get(label.id.as_str())
            .map(|dimensions| dimensions.values);
        let block_placement = (label.value == "BLOCK")
            .then(|| block_placement(ir, block_dimension_values?, &outputs))
            .flatten();
        let sew_projection = (label.value == "SEW")
            .then(|| {
                sew_body_feature_definition(
                    *body_references.get(label.id.as_str())?,
                    operation_body_operands_by_operation
                        .get(label.id.as_str())?
                        .as_slice(),
                    &bodies_by_object_index,
                )
            })
            .flatten();
        let trim_body_projection = (label.value == "TRIM BODY")
            .then(|| {
                trim_body_feature_definition(
                    *body_references.get(label.id.as_str())?,
                    operation_body_operands_by_operation
                        .get(label.id.as_str())?
                        .as_slice(),
                    &bodies_by_object_index,
                )
            })
            .flatten();
        let offset_projection = (label.value == "OFFSET")
            .then(|| offset_surface_feature_definition(ir, &outputs))
            .flatten();
        if let Some((_, supports)) = &offset_projection {
            for (support_ordinal, support) in supports.iter().enumerate() {
                source_properties.insert(
                    format!("offset_support_surface.{support_ordinal}"),
                    support.0.clone(),
                );
            }
        }
        let thicken_projection = (label.value == "THICKEN_SHEET")
            .then(|| thicken_feature_definition(ir, &outputs))
            .flatten();
        if let Some((_, supports)) = &thicken_projection {
            for (support_ordinal, support) in supports.iter().enumerate() {
                source_properties.insert(
                    format!("thicken_support_surface.{support_ordinal}"),
                    support.0.clone(),
                );
            }
        }
        let blend_family = match label.value.as_str() {
            "BLEND" => Some(NxBlendFamily::Edge),
            "FACE_BLEND" => Some(NxBlendFamily::Face),
            _ => None,
        };
        let blend_projection =
            blend_family.and_then(|family| blend_feature_definition(ir, &outputs, family));
        if let Some((_, surfaces)) = &blend_projection {
            for (surface_ordinal, surface) in surfaces.iter().enumerate() {
                source_properties.insert(
                    format!("blend_result_surface.{surface_ordinal}"),
                    surface.0.clone(),
                );
            }
        }
        let extrude_projection = (label.value == "EXTRUDE")
            .then(|| {
                let body = body_references.get(label.id.as_str())?;
                let output_kinds = outputs
                    .iter()
                    .filter_map(|output| {
                        ir.model
                            .bodies
                            .iter()
                            .find(|body| body.id == *output)
                            .map(|body| body.kind)
                    })
                    .collect::<Vec<_>>();
                let op = extrude_boolean_op(
                    last_body_writer.contains_key(&canonical_body(*body)),
                    &output_kinds,
                );
                extrude_feature_definition(
                    extrude_construction_profiles_by_operation
                        .get(label.id.as_str())
                        .map(|profile| profile.id.as_str()),
                    extrude_32_constructions_by_operation
                        .get(label.id.as_str())
                        .map(|construction| construction.id.as_str()),
                    op,
                )
            })
            .flatten();
        let delete_projection = deletes_body
            .then(|| {
                delete_body_feature_definition(
                    body_references.get(label.id.as_str()).copied(),
                    &bodies_by_object_index,
                )
            })
            .flatten();
        let operation_parameter_uses = parameter_uses_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let native_parameters = native_feature_parameters(operation_parameter_uses, expressions);
        let definition = booleans.get(label.id.as_str()).map_or_else(
            || {
                trim_body_projection
                    .or(delete_projection)
                    .or(sew_projection)
                    .or(extrude_projection)
                    .or_else(|| blend_projection.map(|(definition, _)| definition))
                    .or_else(|| thicken_projection.map(|(definition, _)| definition))
                    .or_else(|| offset_projection.map(|(definition, _)| definition))
                    .unwrap_or_else(|| {
                        non_boolean_feature_definition_with_parameters(
                            &label.value,
                            &operation_payload_strings,
                            block_dimension_values,
                            block_placement,
                            HoleProjection {
                                position: simple_hole_positions
                                    .get(label.id.as_str())
                                    .or_else(|| hole_package_positions.get(label.id.as_str()))
                                    .copied(),
                                diameter: simple_hole_diameters
                                    .get(label.id.as_str())
                                    .or_else(|| hole_package_diameters.get(label.id.as_str()))
                                    .copied(),
                                direction: simple_hole_directions
                                    .get(label.id.as_str())
                                    .or_else(|| hole_package_directions.get(label.id.as_str()))
                                    .copied(),
                                chamfer: simple_hole_chamfers.get(label.id.as_str()).copied(),
                            },
                            native_parameters,
                        )
                    })
            },
            |operation| boolean_feature_definition(operation, &bodies_by_object_index),
        );
        annotations
            .note(&id, stream, label.source_offset)
            .tag("FEATURE_OPERATION");
        annotations.exactness(&id, Exactness::Derived);
        let mut source_content =
            feature_source_content(operation_payload_string_records, operation_parameter_uses);
        if let Some(dimensions) = block_dimensions_by_operation.get(label.id.as_str()) {
            append_feature_expression_content(&mut source_content, &dimensions.expressions);
        }
        for owner in parameter_owner_dependencies(&parameter_owners, &source_content) {
            if !dependencies.contains(&owner) {
                dependencies.push(owner);
            }
        }
        if !source_content.is_empty() {
            annotations.derived(&id, "source_content");
        }
        if let Some(annotation) = text_semantic_annotation(
            &label.value,
            &id,
            &label.id,
            u32::try_from(ir.model.semantic_annotations.len()).unwrap_or(u32::MAX),
            &operation_payload_strings,
        ) {
            annotations
                .note(&annotation.id.0, stream, label.source_offset)
                .tag("TEXT_SEMANTIC_ANNOTATION");
            annotations.exactness(&annotation.id.0, Exactness::Derived);
            ir.model.semantic_annotations.push(annotation);
        }
        ir.model.features.push(Feature {
            id: id.clone(),
            ordinal: base_ordinal + ordinal as u64,
            name: Some(label.value.clone()),
            suppressed: None,
            parent: None,
            dependencies,
            source_properties,
            source_tag: Some(label.value.clone()),
            source_text: None,
            source_content,
            outputs,
            definition,
            native_ref: Some(label.id.clone()),
        });
        if !deletes_body {
            if let Some(body) = body_references.get(label.id.as_str()) {
                last_body_writer.insert(canonical_body(*body), id);
            }
        }
    }
}

fn records_by_operation<'a, T>(
    records: &'a [T],
    operation_label: impl Fn(&'a T) -> &'a str,
) -> BTreeMap<&'a str, Vec<&'a T>> {
    let mut grouped = BTreeMap::new();
    for record in records {
        grouped
            .entry(operation_label(record))
            .or_insert_with(Vec::new)
            .push(record);
    }
    grouped
}
