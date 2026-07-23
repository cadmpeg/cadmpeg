// SPDX-License-Identifier: Apache-2.0
//! Eager, best-effort extraction of the native NX object model.
//!
//! [`NativeModel::extract`] runs the full extraction dependency DAG in the same
//! hand-ordered topological order the decode tier previously inlined, grouping
//! the resulting record vectors into five domain sub-structs. Extraction is
//! infallible: malformed data is omitted, never surfaced as an error.

use crate::container::Container;
use crate::parasolid::Stream;

#[allow(clippy::wildcard_imports)]
use super::{display_jt::*, features::*, om::*, parasolid::*, segments::*, substrate::*};

/// Records extracted from the `display_jt` domain.
#[allow(clippy::struct_field_names)]
pub(crate) struct DisplayJtRecords {
    pub(crate) display_jt_indices: Vec<DisplayJtIndex>,
    pub(crate) display_jt_documents: Vec<DisplayJtDocument>,
    pub(crate) display_jt_segments: Vec<DisplayJtSegment>,
    pub(crate) display_jt_shape_lod_elements: Vec<DisplayJtShapeLodElement>,
    pub(crate) display_jt_tri_strip_lod_headers: Vec<DisplayJtTriStripLodHeader>,
    pub(crate) display_jt_initial_face_degree_symbols: Vec<DisplayJtInitialFaceDegreeSymbols>,
    pub(crate) display_jt_topology_packet_sequences: Vec<DisplayJtTopologyPacketSequence>,
    pub(crate) display_jt_vertex_records_headers: Vec<DisplayJtCompressedVertexRecordsHeader>,
    pub(crate) display_jt_coordinate_array_headers: Vec<DisplayJtVertexCoordinateArrayHeader>,
    pub(crate) display_jt_vertex_coordinates: Vec<DisplayJtVertexCoordinates>,
    pub(crate) display_jt_vertex_normals: Vec<DisplayJtVertexNormals>,
    pub(crate) display_jt_vertex_colors: Vec<DisplayJtVertexColors>,
    pub(crate) display_jt_vertex_texture_coordinates: Vec<DisplayJtVertexTextureCoordinates>,
    pub(crate) display_jt_vertex_flags: Vec<DisplayJtVertexFlags>,
    pub(crate) display_jt_polygon_meshes: Vec<DisplayJtPolygonMesh>,
    pub(crate) display_jt_compressed_elements: Vec<DisplayJtCompressedElement>,
    pub(crate) display_jt_compressed_element_sequences: Vec<DisplayJtCompressedElementSequence>,
    pub(crate) display_jt_string_property_atoms: Vec<DisplayJtStringPropertyAtom>,
    pub(crate) display_jt_shape_lod_bindings: Vec<DisplayJtShapeLodBinding>,
    pub(crate) display_jt_base_node_data: Vec<DisplayJtBaseNodeData>,
    pub(crate) display_jt_group_node_data: Vec<DisplayJtGroupNodeData>,
    pub(crate) display_jt_instance_nodes: Vec<DisplayJtInstanceNode>,
    pub(crate) display_jt_geometric_transform_attributes: Vec<DisplayJtGeometricTransformAttribute>,
    pub(crate) display_jt_partition_nodes: Vec<DisplayJtPartitionNode>,
    pub(crate) display_jt_range_lod_nodes: Vec<DisplayJtRangeLodNode>,
    pub(crate) display_jt_tri_strip_shape_nodes: Vec<DisplayJtTriStripShapeNode>,
}

/// Records extracted from the `parasolid` domain.
#[allow(clippy::struct_field_names)]
pub(crate) struct ParasolidRecords {
    pub(crate) parasolid_blend_surface_records: Vec<ParasolidBlendSurfaceRecord>,
    pub(crate) parasolid_blend_bound_records: Vec<ParasolidBlendBoundRecord>,
    pub(crate) parasolid_offset_surface_records: Vec<ParasolidOffsetSurfaceRecord>,
    pub(crate) parasolid_trimmed_curve_records: Vec<ParasolidTrimmedCurveRecord>,
    pub(crate) parasolid_surface_curve_records: Vec<ParasolidSurfaceCurveRecord>,
    pub(crate) parasolid_intersection_records: Vec<ParasolidIntersectionRecord>,
    pub(crate) parasolid_term_use_records: Vec<ParasolidTermUseRecord>,
    pub(crate) parasolid_support_uv_records: Vec<ParasolidSupportUvRecord>,
    pub(crate) parasolid_chart_records: Vec<ParasolidChartRecord>,
    pub(crate) parasolid_attribute_definitions: Vec<ParasolidAttributeDefinition>,
    pub(crate) parasolid_entity_51_records: Vec<ParasolidEntity51Record>,
    pub(crate) parasolid_entity_52_integer_records: Vec<ParasolidEntity52IntegerRecord>,
    pub(crate) parasolid_entity_53_double_records: Vec<ParasolidEntity53DoubleRecord>,
    pub(crate) parasolid_entity_54_string_records: Vec<ParasolidEntity54StringRecord>,
    pub(crate) parasolid_entity_51_numeric_uses: Vec<ParasolidEntity51NumericUse>,
    pub(crate) parasolid_entity_51_string_uses: Vec<ParasolidEntity51StringUse>,
    pub(crate) parasolid_attribute_class_uses: Vec<ParasolidAttributeClassUse>,
    pub(crate) parasolid_topology_attribute_list_references:
        Vec<ParasolidTopologyAttributeListReference>,
    pub(crate) parasolid_topology_attribute_class_uses: Vec<ParasolidTopologyAttributeClassUse>,
}

/// Records extracted from the `segments` domain.
#[allow(clippy::struct_field_names)]
pub(crate) struct SegmentRecords {
    pub(crate) segment_index_rows: Vec<SegmentIndexRow>,
    pub(crate) segment_om_links: Vec<SegmentOmLink>,
    pub(crate) segment_stream_links: Vec<SegmentStreamLink>,
    pub(crate) segment_body_bindings: Vec<SegmentBodyBinding>,
    pub(crate) segment_body_lineage_statuses: Vec<SegmentBodyLineageStatus>,
}

/// Records extracted from the `features` domain.
pub(crate) struct FeatureRecords {
    pub(crate) feature_operation_labels: Vec<FeatureOperationLabel>,
    pub(crate) feature_operation_records: Vec<FeatureOperationRecord>,
    pub(crate) feature_payload_strings: Vec<FeaturePayloadString>,
    pub(crate) feature_simple_hole_templates: Vec<FeatureSimpleHoleTemplate>,
    pub(crate) feature_simple_hole_repeated_scalar_lanes: Vec<FeatureSimpleHoleRepeatedScalarLane>,
    pub(crate) feature_simple_hole_repeated_scalar_lane_block_references:
        Vec<FeatureSimpleHoleRepeatedScalarLaneBlockReferences>,
    pub(crate) feature_simple_hole_construction_groups: Vec<FeatureSimpleHoleConstructionGroup>,
    pub(crate) feature_body_references: Vec<FeatureBodyReference>,
    pub(crate) feature_body_segment_uses: Vec<FeatureBodySegmentUse>,
    pub(crate) feature_body_reference_occurrences: Vec<FeatureBodyReferenceOccurrence>,
    pub(crate) feature_input_blocks: Vec<FeatureInputBlock>,
    pub(crate) feature_input_block_identity_groups: Vec<FeatureInputBlockIdentityGroup>,
    pub(crate) feature_datum_csys_constructions: Vec<FeatureDatumCsysConstruction>,
    pub(crate) feature_datum_csys_payloads: Vec<FeatureDatumCsysPayload>,
    pub(crate) feature_datum_csys_payload_scalar_pairs: Vec<FeatureDatumCsysPayloadScalarPair>,
    pub(crate) feature_datum_csys_payload_fixed_pairs: Vec<FeatureDatumCsysPayloadFixedPair>,
    pub(crate) feature_datum_csys_payload_scalars: Vec<FeatureDatumCsysPayloadScalar>,
    pub(crate) feature_datum_csys_descriptors: Vec<FeatureDatumCsysDescriptor>,
    pub(crate) feature_datum_plane_headers: Vec<FeatureDatumPlaneHeader>,
    pub(crate) feature_datum_plane_block_uses: Vec<FeatureDatumPlaneBlockUse>,
    pub(crate) feature_datum_plane_payloads: Vec<FeatureDatumPlanePayload>,
    pub(crate) feature_datum_plane_payload_scalar_pairs: Vec<FeatureDatumPlanePayloadScalarPair>,
    pub(crate) feature_datum_plane_descriptors: Vec<FeatureDatumPlaneDescriptor>,
    pub(crate) feature_datum_plane_csys_identity_uses: Vec<FeatureDatumPlaneCsysIdentityUse>,
    pub(crate) feature_datum_csys_block_uses: Vec<FeatureDatumCsysBlockUse>,
    pub(crate) feature_sketch_references: Vec<FeatureSketchReference>,
    pub(crate) feature_projected_curve_references: Vec<FeatureProjectedCurveReference>,
    pub(crate) feature_projected_curve_construction_payloads:
        Vec<FeatureProjectedCurveConstructionPayload>,
    pub(crate) feature_projected_curve_construction_strings:
        Vec<FeatureProjectedCurveConstructionString>,
    pub(crate) feature_pattern_references: Vec<FeaturePatternReference>,
    pub(crate) feature_pattern_construction_payloads: Vec<FeaturePatternConstructionPayload>,
    pub(crate) feature_pattern_construction_strings: Vec<FeaturePatternConstructionString>,
    pub(crate) feature_pattern_construction_fixed_lanes: Vec<FeaturePatternConstructionFixedLane>,
    pub(crate) feature_pattern_transform_lanes: Vec<FeaturePatternTransformLane>,
    pub(crate) feature_point_construction_headers: Vec<FeaturePointConstructionHeader>,
    pub(crate) feature_point_construction_scalar_lanes: Vec<FeaturePointConstructionScalarLane>,
    pub(crate) feature_draft_construction_references: Vec<FeatureDraftConstructionReference>,
    pub(crate) feature_draft_construction_index_lanes: Vec<FeatureDraftConstructionIndexLane>,
    pub(crate) feature_draft_construction_payloads: Vec<FeatureDraftConstructionPayload>,
    pub(crate) feature_draft_construction_graph_payloads: Vec<FeatureDraftConstructionGraphPayload>,
    pub(crate) feature_draft_construction_fixed_lanes: Vec<FeatureDraftConstructionFixedLane>,
    pub(crate) feature_draft_construction_binary32_lanes: Vec<FeatureDraftConstructionBinary32Lane>,
    pub(crate) feature_draft_construction_graph_strings: Vec<FeatureDraftConstructionGraphString>,
    pub(crate) feature_draft_construction_identity_frames:
        Vec<FeatureDraftConstructionIdentityFrame>,
    pub(crate) feature_draft_construction_terminal_lanes: Vec<FeatureDraftConstructionTerminalLane>,
    pub(crate) feature_surface_construction_references: Vec<FeatureSurfaceConstructionReference>,
    pub(crate) feature_surface_construction_payloads: Vec<FeatureSurfaceConstructionPayload>,
    pub(crate) feature_surface_construction_scalar_pairs: Vec<FeatureSurfaceConstructionScalarPair>,
    pub(crate) feature_surface_construction_strings: Vec<FeatureSurfaceConstructionString>,
    pub(crate) feature_surface_construction_branches: Vec<FeatureSurfaceConstructionBranch>,
    pub(crate) feature_extrude_profile_references: Vec<FeatureExtrudeProfileReference>,
    pub(crate) feature_extrude_payload_headers: Vec<FeatureExtrudePayloadHeader>,
    pub(crate) feature_extrude_payload_footers: Vec<FeatureExtrudePayloadFooter>,
    pub(crate) feature_operation_body_scalar_triples: Vec<FeatureOperationBodyScalarTriple>,
    pub(crate) feature_operation_body_members: Vec<FeatureOperationBodyMember>,
    pub(crate) feature_operation_body_operands: Vec<FeatureOperationBodyOperand>,
    pub(crate) feature_operation_body_11_continuations: Vec<FeatureOperationBody11Continuation>,
    pub(crate) feature_operation_body_reference_lanes: Vec<FeatureOperationBodyReferenceLane>,
    pub(crate) feature_extrude_construction_profiles: Vec<FeatureExtrudeConstructionProfile>,
    pub(crate) feature_extrude_payload_32_branches: Vec<FeatureExtrudePayload32Branch>,
    pub(crate) feature_extrude_32_constructions: Vec<FeatureExtrude32Construction>,
    pub(crate) feature_block_construction_references: Vec<FeatureBlockConstructionReference>,
    pub(crate) feature_block_constructions: Vec<FeatureBlockConstruction>,
    pub(crate) feature_block_construction_payloads: Vec<FeatureBlockConstructionPayload>,
    pub(crate) feature_block_payload_scalars: Vec<FeatureBlockPayloadScalar>,
    pub(crate) feature_block_payload_names: Vec<FeatureBlockPayloadName>,
    pub(crate) feature_block_payload_named_records: Vec<FeatureBlockPayloadNamedRecord>,
    pub(crate) feature_block_payload_points: Vec<FeatureBlockPayloadPoint>,
    pub(crate) feature_block_payload_point_groups: Vec<FeatureBlockPayloadPointGroup>,
    pub(crate) feature_sketch_records: Vec<FeatureSketchRecord>,
    pub(crate) feature_sketch_construction_inputs: Vec<FeatureSketchConstructionInputs>,
    pub(crate) feature_sketch_construction_payloads: Vec<FeatureSketchConstructionPayload>,
    pub(crate) feature_sketch_payload_coordinate_pairs: Vec<FeatureSketchPayloadCoordinatePair>,
    pub(crate) feature_sketch_payload_fixed_pairs: Vec<FeatureSketchPayloadFixedPair>,
    pub(crate) feature_sketch_payload_scalars: Vec<FeatureSketchPayloadScalar>,
    pub(crate) feature_sketch_payload_names: Vec<FeatureSketchPayloadName>,
    pub(crate) feature_sketch_payload_named_records: Vec<FeatureSketchPayloadNamedRecord>,
    pub(crate) feature_sketch_fixed_points: Vec<FeatureSketchFixedPoint>,
    pub(crate) feature_sketch_points: Vec<FeatureSketchPoint>,
    pub(crate) feature_sketch_point_groups: Vec<FeatureSketchPointGroup>,
    pub(crate) offset_store_named_points: Vec<OffsetStoreNamedPoint>,
    pub(crate) feature_sketch_named_point_block_uses: Vec<FeatureSketchNamedPointBlockUse>,
    pub(crate) feature_sketch_preceding_named_point_uses: Vec<FeatureSketchPrecedingNamedPointUse>,
    pub(crate) feature_sketch_point_uses: Vec<FeatureSketchPointUse>,
    pub(crate) feature_sketch_datum_csys_dependencies: Vec<FeatureSketchDatumCsysDependency>,
    pub(crate) feature_boolean_operations: Vec<FeatureBooleanOperation>,
    pub(crate) data_block_object_frames: Vec<DataBlockObjectFrame>,
    pub(crate) feature_input_column_row_uses: Vec<FeatureInputColumnRowUse>,
    pub(crate) feature_input_column_targets: Vec<FeatureInputColumnTarget>,
    pub(crate) feature_parameter_bindings: Vec<FeatureParameterBinding>,
    pub(crate) feature_parameter_uses: Vec<FeatureParameterUse>,
    pub(crate) feature_block_dimensions: Vec<FeatureBlockDimensions>,
}

/// Records extracted from the `om` domain.
pub(crate) struct OmRecords {
    pub(crate) om_record_areas: Vec<OmRecordArea>,
    pub(crate) expression_declarations: Vec<ExpressionDeclaration>,
    pub(crate) expressions: Vec<Expression>,
    pub(crate) classes: Vec<ClassDefinition>,
    pub(crate) fields: Vec<FieldDefinition>,
    pub(crate) object_records: Vec<ObjectRecord>,
    pub(crate) rmfastload_object_id_tables: Vec<RmFastLoadObjectIdTable>,
    pub(crate) rmfastload_object_ids: Vec<RmFastLoadObjectId>,
    pub(crate) data_blocks: Vec<DataBlock>,
    pub(crate) data_block_control_forms: Vec<DataBlockControlForm>,
    pub(crate) data_block_control_values: Vec<DataBlockControlValue>,
    pub(crate) data_block_control_class_references: Vec<DataBlockControlClassReference>,
    pub(crate) data_block_control_index_values: Vec<DataBlockControlIndexValue>,
    pub(crate) data_block_control_references: Vec<DataBlockControlReference>,
    pub(crate) data_block_control_handle_pairs: Vec<DataBlockControlHandlePair>,
    pub(crate) data_block_references: Vec<DataBlockReference>,
    pub(crate) data_block_counted_index_lanes: Vec<DataBlockCountedIndexLane>,
    pub(crate) data_block_abr_reference_lanes: Vec<DataBlockAbrReferenceLane>,
    pub(crate) data_block_index_rows: Vec<DataBlockIndexRow>,
    pub(crate) data_block_linked_index_rows: Vec<DataBlockLinkedIndexRow>,
    pub(crate) data_block_target_index_rows: Vec<DataBlockTargetIndexRow>,
    pub(crate) data_block_column_index_tables: Vec<DataBlockColumnIndexTable>,
    pub(crate) store_headers: Vec<StoreHeader>,
    pub(crate) string_values: Vec<StringValue>,
    pub(crate) object_references: Vec<ObjectReference>,
    pub(crate) configurations: Vec<Configuration>,
    pub(crate) part_attributes: Vec<PartAttribute>,
    pub(crate) configuration_attribute_uses: Vec<ConfigurationAttributeUse>,
    pub(crate) external_references: Vec<ExternalReference>,
    pub(crate) external_reference_records: Vec<ExternalReferenceRecord>,
    pub(crate) external_reference_indexed_records: Vec<ExternalReferenceIndexedRecord>,
    pub(crate) external_reference_empty_records: Vec<ExternalReferenceEmptyRecord>,
    pub(crate) external_reference_tail_reference_pairs: Vec<ExternalReferenceTailReferencePair>,
    pub(crate) external_reference_record_string_uses: Vec<ExternalReferenceRecordStringUse>,
    pub(crate) external_reference_record_children: Vec<ExternalReferenceRecordChild>,
    pub(crate) material_texture_assets: Vec<MaterialTextureAsset>,
    pub(crate) material_texture_catalog_entries: Vec<MaterialTextureCatalogEntry>,
    pub(crate) persistent_handles: Vec<PersistentHandle>,
}

/// The complete set of native records extracted from one scanned part, grouped
/// by domain. A wide struct by necessity: the attachment tier needs per-family
/// access to every record vector.
pub(crate) struct NativeModel {
    pub(crate) display_jt: DisplayJtRecords,
    pub(crate) parasolid: ParasolidRecords,
    pub(crate) segments: SegmentRecords,
    pub(crate) features: FeatureRecords,
    pub(crate) om: OmRecords,
}

impl NativeModel {
    /// Runs the full extraction dependency DAG in the original hand-ordered
    /// sequence. The ordering is load-bearing: several families feed later ones
    /// and some record ids embed positional information, so the `let` order here
    /// is fixed to match the legacy wiring block byte-for-byte.
    pub(crate) fn extract(
        container: &Container,
        streams: &[Stream],
        parsed: &ParsedStreams,
    ) -> Self {
        let segment_index_rows = segment_index_rows(container);
        let segment_om_links = segment_om_links(container);
        let segment_stream_links = segment_stream_links(container, streams);
        let segment_body_bindings = segment_body_bindings(container, streams);
        let parasolid_blend_surface_records = parasolid_blend_surface_records(parsed);
        let parasolid_blend_bound_records = parasolid_blend_bound_records(streams);
        let parasolid_offset_surface_records = parasolid_offset_surface_records(parsed);
        let parasolid_trimmed_curve_records = parasolid_trimmed_curve_records(parsed);
        let parasolid_surface_curve_records = parasolid_surface_curve_records(parsed);
        let parasolid_intersection_records = parasolid_intersection_records(parsed);
        let parasolid_term_use_records = parasolid_term_use_records(streams);
        let parasolid_support_uv_records = parasolid_support_uv_records(streams);
        let parasolid_chart_records = parasolid_chart_records(streams);
        let parasolid_attribute_definitions = parasolid_attribute_definitions(streams);
        let parasolid_entity_51_records = parasolid_entity_51_records(streams);
        let parasolid_entity_52_integer_records = parasolid_entity_52_integer_records(streams);
        let parasolid_entity_53_double_records = parasolid_entity_53_double_records(streams);
        let parasolid_entity_54_string_records = parasolid_entity_54_string_records(streams);
        let parasolid_entity_51_numeric_uses = parasolid_entity_51_numeric_uses(
            &parasolid_entity_51_records,
            &parasolid_entity_52_integer_records,
            &parasolid_entity_53_double_records,
        );
        let parasolid_entity_51_string_uses = parasolid_entity_51_string_uses(
            &parasolid_entity_51_records,
            &parasolid_entity_54_string_records,
        );
        let parasolid_attribute_class_uses = parasolid_attribute_class_uses(
            &parasolid_entity_51_records,
            &parasolid_attribute_definitions,
        );
        let parasolid_topology_attribute_list_references =
            parasolid_topology_attribute_list_references(parsed, &parasolid_entity_51_records);
        let parasolid_topology_attribute_class_uses = parasolid_topology_attribute_class_uses(
            &parasolid_topology_attribute_list_references,
            &parasolid_attribute_class_uses,
        );
        let om_record_areas = om_record_areas(container);
        let feature_operation_labels = feature_operation_labels(container);
        let feature_operation_records = feature_operation_records(container);
        let feature_payload_strings = feature_payload_strings(container);
        let feature_simple_hole_templates = feature_simple_hole_templates(
            &feature_operation_labels,
            &feature_operation_records,
            &feature_payload_strings,
        );
        let feature_simple_hole_repeated_scalar_lanes =
            feature_simple_hole_repeated_scalar_lanes(container);
        let feature_simple_hole_repeated_scalar_lane_block_references =
            feature_simple_hole_repeated_scalar_lane_block_references(container);
        let feature_simple_hole_construction_groups = feature_simple_hole_construction_groups(
            &feature_simple_hole_repeated_scalar_lanes,
            &feature_simple_hole_repeated_scalar_lane_block_references,
        );
        let feature_body_references = feature_body_references(container);
        let feature_body_segment_uses =
            feature_body_segment_uses(&feature_body_references, &segment_body_bindings);
        let feature_body_reference_occurrences = feature_body_reference_occurrences(container);
        let feature_input_blocks = feature_input_blocks(container);
        let feature_input_block_identity_groups =
            feature_input_block_identity_groups(&feature_input_blocks);
        let display_jt_indices = display_jt_indices(container);
        let display_jt_documents = display_jt_documents(container, &display_jt_indices);
        let display_jt_segments = display_jt_segments(container, &display_jt_documents);
        let display_jt_shape_lod_elements =
            display_jt_shape_lod_elements(container, &display_jt_segments);
        let display_jt_tri_strip_lod_headers =
            display_jt_tri_strip_lod_headers(container, &display_jt_shape_lod_elements);
        let display_jt_initial_face_degree_symbols =
            display_jt_initial_face_degree_symbols(container, &display_jt_shape_lod_elements);
        let (
            display_jt_topology_packet_sequences,
            display_jt_vertex_records_headers,
            display_jt_coordinate_array_headers,
        ) = display_jt_topology_packet_sequences(container, &display_jt_shape_lod_elements);
        let display_jt_vertex_coordinates =
            display_jt_vertex_coordinates(container, &display_jt_coordinate_array_headers);
        let display_jt_vertex_normals = display_jt_vertex_normals(
            container,
            &display_jt_vertex_records_headers,
            &display_jt_coordinate_array_headers,
            &display_jt_vertex_coordinates,
        );
        let display_jt_vertex_colors = display_jt_vertex_colors(
            container,
            &display_jt_vertex_records_headers,
            &display_jt_coordinate_array_headers,
            &display_jt_vertex_coordinates,
            &display_jt_vertex_normals,
        );
        let display_jt_vertex_texture_coordinates = display_jt_vertex_texture_coordinates(
            container,
            &display_jt_vertex_records_headers,
            &display_jt_coordinate_array_headers,
            &display_jt_vertex_coordinates,
            &display_jt_vertex_normals,
            &display_jt_vertex_colors,
        );
        let display_jt_vertex_flags = display_jt_vertex_flags(
            container,
            &display_jt_vertex_records_headers,
            &display_jt_coordinate_array_headers,
            &display_jt_vertex_coordinates,
            &display_jt_vertex_normals,
            &display_jt_vertex_colors,
            &display_jt_vertex_texture_coordinates,
        );
        let display_jt_polygon_meshes = display_jt_polygon_meshes(
            &display_jt_topology_packet_sequences,
            &display_jt_coordinate_array_headers,
        );
        let (display_jt_compressed_elements, display_jt_compressed_element_sequences) =
            display_jt_compressed_element_sequences(container, &display_jt_segments);
        let display_jt_string_property_atoms =
            display_jt_string_property_atoms(container, &display_jt_segments);
        let display_jt_shape_lod_bindings =
            display_jt_shape_lod_bindings(container, &display_jt_segments);
        let display_jt_base_node_data =
            display_jt_base_node_data(container, &display_jt_segments, &display_jt_documents);
        let display_jt_group_node_data =
            display_jt_group_node_data(container, &display_jt_segments, &display_jt_documents);
        let display_jt_instance_nodes =
            display_jt_instance_nodes(container, &display_jt_segments, &display_jt_documents);
        let display_jt_geometric_transform_attributes = display_jt_geometric_transform_attributes(
            container,
            &display_jt_segments,
            &display_jt_documents,
        );
        let display_jt_partition_nodes =
            display_jt_partition_nodes(container, &display_jt_segments, &display_jt_documents);
        let display_jt_range_lod_nodes =
            display_jt_range_lod_nodes(container, &display_jt_segments, &display_jt_documents);
        let display_jt_tri_strip_shape_nodes = display_jt_tri_strip_shape_nodes(
            container,
            &display_jt_segments,
            &display_jt_documents,
        );
        let feature_datum_csys_constructions = feature_datum_csys_constructions(container);
        let feature_datum_csys_payloads =
            feature_datum_csys_payloads(container, &feature_datum_csys_constructions);
        let feature_datum_csys_payload_scalar_pairs =
            feature_datum_csys_payload_scalar_pairs(container, &feature_datum_csys_payloads);
        let feature_datum_csys_payload_fixed_pairs =
            feature_datum_csys_payload_fixed_pairs(container, &feature_datum_csys_payloads);
        let feature_datum_csys_payload_scalars =
            feature_datum_csys_payload_scalars(container, &feature_datum_csys_payloads);
        let feature_datum_csys_descriptors =
            feature_datum_csys_descriptors(container, &feature_datum_csys_constructions);
        let feature_datum_plane_headers = feature_datum_plane_headers(container);
        let feature_datum_plane_block_uses =
            feature_datum_plane_block_uses(&feature_datum_plane_headers, &feature_input_blocks);
        let feature_datum_plane_payloads =
            feature_datum_plane_payloads(container, &feature_datum_plane_headers);
        let feature_datum_plane_payload_scalar_pairs =
            feature_datum_plane_payload_scalar_pairs(container, &feature_datum_plane_payloads);
        let feature_datum_plane_descriptors =
            feature_datum_plane_descriptors(container, &feature_datum_plane_headers);
        let feature_datum_plane_csys_identity_uses = feature_datum_plane_csys_identity_uses(
            &feature_datum_plane_descriptors,
            &feature_datum_csys_descriptors,
        );
        let feature_datum_csys_block_uses =
            feature_datum_csys_block_uses(&feature_datum_csys_constructions, &feature_input_blocks);
        let feature_sketch_references = feature_sketch_references(container);
        let feature_projected_curve_references = feature_projected_curve_references(container);
        let feature_projected_curve_construction_payloads =
            feature_projected_curve_construction_payloads(
                container,
                &feature_operation_labels,
                &feature_projected_curve_references,
            );
        let feature_projected_curve_construction_strings =
            feature_projected_curve_construction_strings(
                container,
                &feature_projected_curve_construction_payloads,
            );
        let feature_pattern_references = feature_pattern_references(container);
        let feature_pattern_construction_payloads = feature_pattern_construction_payloads(
            container,
            &feature_operation_labels,
            &feature_pattern_references,
        );
        let feature_pattern_construction_strings =
            feature_pattern_construction_strings(container, &feature_pattern_construction_payloads);
        let feature_pattern_construction_fixed_lanes = feature_pattern_construction_fixed_lanes(
            container,
            &feature_pattern_construction_payloads,
        );
        let feature_pattern_transform_lanes = feature_pattern_transform_lanes(container);
        let feature_point_construction_headers = feature_point_construction_headers(container);
        let feature_point_construction_scalar_lanes =
            feature_point_construction_scalar_lanes(container, &feature_point_construction_headers);
        let feature_draft_construction_references =
            feature_draft_construction_references(container);
        let feature_draft_construction_index_lanes =
            feature_draft_construction_index_lanes(container);
        let feature_draft_construction_payloads =
            feature_draft_construction_payloads(container, &feature_draft_construction_index_lanes);
        let feature_draft_construction_graph_payloads = feature_draft_construction_graph_payloads(
            container,
            &feature_draft_construction_index_lanes,
            &feature_draft_construction_references,
        );
        let feature_draft_construction_fixed_lanes = feature_draft_construction_fixed_lanes(
            container,
            &feature_draft_construction_graph_payloads,
        );
        let feature_draft_construction_binary32_lanes = feature_draft_construction_binary32_lanes(
            container,
            &feature_draft_construction_graph_payloads,
        );
        let feature_draft_construction_graph_strings = feature_draft_construction_graph_strings(
            container,
            &feature_draft_construction_graph_payloads,
        );
        let feature_draft_construction_identity_frames = feature_draft_construction_identity_frames(
            container,
            &feature_draft_construction_payloads,
        );
        let feature_draft_construction_terminal_lanes =
            feature_draft_construction_terminal_lanes(container);
        let feature_surface_construction_references =
            feature_surface_construction_references(container);
        let feature_surface_construction_payloads = feature_surface_construction_payloads(
            container,
            &feature_surface_construction_references,
        );
        let feature_surface_construction_scalar_pairs = feature_surface_construction_scalar_pairs(
            container,
            &feature_surface_construction_payloads,
        );
        let feature_surface_construction_strings =
            feature_surface_construction_strings(container, &feature_surface_construction_payloads);
        let feature_surface_construction_branches =
            feature_surface_construction_branches(container);
        let feature_extrude_profile_references = feature_extrude_profile_references(container);
        let feature_extrude_payload_headers = feature_extrude_payload_headers(container);
        let feature_extrude_payload_footers = feature_extrude_payload_footers(container);
        let feature_operation_body_scalar_triples =
            feature_operation_body_scalar_triples(container);
        let feature_operation_body_members = feature_operation_body_members(container);
        let feature_operation_body_operands = feature_operation_body_operands(
            &feature_operation_body_members,
            &feature_body_reference_occurrences,
            &segment_body_bindings,
        );
        let feature_operation_body_11_continuations =
            feature_operation_body_11_continuations(container);
        let feature_operation_body_reference_lanes =
            feature_operation_body_reference_lanes(container);
        let feature_extrude_construction_profiles = feature_extrude_construction_profiles(
            &feature_extrude_profile_references,
            &feature_operation_body_reference_lanes,
        );
        let feature_extrude_payload_32_branches = feature_extrude_payload_32_branches(container);
        let feature_extrude_32_constructions = feature_extrude_32_constructions(
            &feature_extrude_profile_references,
            &feature_extrude_payload_32_branches,
        );
        let feature_block_construction_references =
            feature_block_construction_references(container);
        let feature_block_constructions =
            feature_block_constructions(&feature_block_construction_references);
        let feature_block_construction_payloads =
            feature_block_construction_payloads(container, &feature_block_constructions);
        let feature_block_payload_scalars =
            feature_block_payload_scalars(container, &feature_block_construction_payloads);
        let feature_block_payload_names =
            feature_block_payload_names(container, &feature_block_construction_payloads);
        let feature_block_payload_named_records = feature_block_payload_named_records(
            &feature_block_construction_payloads,
            &feature_block_payload_names,
            &feature_block_payload_scalars,
        );
        let feature_block_payload_points = feature_block_payload_points(
            &feature_block_payload_named_records,
            &feature_block_payload_names,
            &feature_block_payload_scalars,
        );
        let feature_block_payload_point_groups =
            feature_block_payload_point_groups(&feature_block_payload_points);
        let feature_sketch_records = feature_sketch_records(
            &feature_operation_labels,
            &feature_operation_records,
            &feature_input_blocks,
            &feature_sketch_references,
        );
        let feature_sketch_construction_inputs =
            feature_sketch_construction_inputs(&feature_sketch_records, &feature_sketch_references);
        let feature_sketch_construction_payloads =
            feature_sketch_construction_payloads(container, &feature_sketch_construction_inputs);
        let feature_sketch_payload_coordinate_pairs = feature_sketch_payload_coordinate_pairs(
            container,
            &feature_sketch_construction_payloads,
        );
        let feature_sketch_payload_fixed_pairs =
            feature_sketch_payload_fixed_pairs(container, &feature_sketch_construction_payloads);
        let feature_sketch_payload_scalars =
            feature_sketch_payload_scalars(container, &feature_sketch_construction_inputs);
        let feature_sketch_payload_names =
            feature_sketch_payload_names(container, &feature_sketch_construction_inputs);
        let feature_sketch_payload_named_records = feature_sketch_payload_named_records(
            &feature_sketch_construction_payloads,
            &feature_sketch_payload_names,
            &feature_sketch_payload_scalars,
            &feature_sketch_payload_fixed_pairs,
        );
        let feature_sketch_fixed_points = feature_sketch_fixed_points(
            &feature_sketch_payload_named_records,
            &feature_sketch_payload_names,
            &feature_sketch_payload_fixed_pairs,
        );
        let feature_sketch_points = feature_sketch_points(
            &feature_sketch_payload_named_records,
            &feature_sketch_payload_names,
            &feature_sketch_payload_scalars,
        );
        let feature_sketch_point_groups = feature_sketch_point_groups(&feature_sketch_points);
        let offset_store_named_points = offset_store_named_points(container);
        let feature_sketch_named_point_block_uses = feature_sketch_named_point_block_uses(
            &feature_sketch_references,
            &offset_store_named_points,
        );
        let feature_sketch_preceding_named_point_uses = feature_sketch_preceding_named_point_uses(
            &feature_sketch_references,
            &offset_store_named_points,
        );
        let feature_sketch_point_uses = feature_sketch_point_uses(
            &feature_sketch_point_groups,
            &offset_store_named_points,
            &feature_sketch_named_point_block_uses,
        );
        let feature_sketch_datum_csys_dependencies = feature_sketch_datum_csys_dependencies(
            &feature_operation_labels,
            &offset_store_named_points,
            &feature_sketch_point_uses,
            &feature_datum_csys_constructions,
        );
        let feature_boolean_operations = feature_boolean_operations(container);
        let segment_body_lineage_statuses = segment_body_lineage_statuses(
            &feature_operation_labels,
            &feature_body_references,
            &feature_boolean_operations,
            &feature_operation_body_operands,
            &segment_body_bindings,
        )
        .unwrap_or_default();
        let expression_declarations = expression_declarations(container);
        let data_block_object_frames = data_block_object_frames(container);
        let expressions = expressions(container);
        let classes = class_definitions(container);
        let fields = field_definitions(container);
        let object_records = object_records(container);
        let (rmfastload_object_id_tables, rmfastload_object_ids) =
            match rmfastload_object_id_table(container) {
                Some((table, object_ids)) => (vec![table], object_ids),
                None => (Vec::new(), Vec::new()),
            };
        let data_blocks = data_blocks(container);
        let data_block_control_forms = data_block_control_forms(container);
        let data_block_control_values = data_block_control_values(container);
        let data_block_control_class_references = data_block_control_class_references(container);
        let data_block_control_index_values = data_block_control_index_values(container);
        let data_block_control_references = data_block_control_references(container);
        let data_block_control_handle_pairs =
            data_block_control_handle_pairs(&data_block_control_references);
        let data_block_references = data_block_references(container);
        let data_block_counted_index_lanes = data_block_counted_index_lanes(container);
        let data_block_abr_reference_lanes = data_block_abr_reference_lanes(container);
        let data_block_index_rows = data_block_index_rows(container);
        let data_block_linked_index_rows = data_block_linked_index_rows(container);
        let data_block_target_index_rows = data_block_target_index_rows(container);
        let data_block_column_index_tables = data_block_column_index_tables(
            &data_block_linked_index_rows,
            &data_block_target_index_rows,
        );
        let feature_input_column_row_uses = feature_input_column_row_uses(
            &feature_input_blocks,
            &data_block_index_rows,
            &data_block_linked_index_rows,
            &data_block_target_index_rows,
            &data_block_column_index_tables,
        );
        let feature_input_column_targets = feature_input_column_targets(
            &feature_input_blocks,
            &feature_input_column_row_uses,
            &data_block_linked_index_rows,
            &data_block_target_index_rows,
        );
        let feature_parameter_bindings =
            feature_parameter_bindings(&feature_input_blocks, &data_block_references, &expressions);
        let feature_parameter_uses = feature_parameter_uses(&feature_parameter_bindings);
        let feature_block_dimensions = feature_block_dimensions(
            &feature_block_constructions,
            &feature_parameter_bindings,
            &expression_declarations,
            &expressions,
        );
        let store_headers = store_headers(container);
        let string_values = string_values(container);
        let object_references = object_references(container);
        let configurations = configurations(container);
        let part_attributes = part_attributes(container);
        let configuration_attribute_uses =
            configuration_attribute_uses(&configurations, &part_attributes);
        let external_references = external_references(container);
        let external_reference_records = external_reference_records(container);
        let external_reference_indexed_records =
            external_reference_indexed_records(container, &external_reference_records);
        let external_reference_empty_records =
            external_reference_empty_records(container, &external_reference_indexed_records);
        let external_reference_tail_reference_pairs =
            external_reference_tail_reference_pairs(container, &external_reference_records);
        let external_reference_record_string_uses = external_reference_record_string_uses(
            &external_reference_records,
            &external_references,
        );
        let external_reference_record_children = external_reference_record_children(
            &external_reference_records,
            &external_references,
            &external_reference_record_string_uses,
        );
        let material_texture_assets = material_texture_assets(container);
        let material_texture_catalog_entries =
            material_texture_catalog_entries(container, &material_texture_assets);
        let persistent_handles = persistent_handles(
            &object_references,
            &data_block_control_references,
            &external_reference_records,
            &external_reference_tail_reference_pairs,
        );

        NativeModel {
            display_jt: DisplayJtRecords {
                display_jt_indices,
                display_jt_documents,
                display_jt_segments,
                display_jt_shape_lod_elements,
                display_jt_tri_strip_lod_headers,
                display_jt_initial_face_degree_symbols,
                display_jt_topology_packet_sequences,
                display_jt_vertex_records_headers,
                display_jt_coordinate_array_headers,
                display_jt_vertex_coordinates,
                display_jt_vertex_normals,
                display_jt_vertex_colors,
                display_jt_vertex_texture_coordinates,
                display_jt_vertex_flags,
                display_jt_polygon_meshes,
                display_jt_compressed_elements,
                display_jt_compressed_element_sequences,
                display_jt_string_property_atoms,
                display_jt_shape_lod_bindings,
                display_jt_base_node_data,
                display_jt_group_node_data,
                display_jt_instance_nodes,
                display_jt_geometric_transform_attributes,
                display_jt_partition_nodes,
                display_jt_range_lod_nodes,
                display_jt_tri_strip_shape_nodes,
            },
            parasolid: ParasolidRecords {
                parasolid_blend_surface_records,
                parasolid_blend_bound_records,
                parasolid_offset_surface_records,
                parasolid_trimmed_curve_records,
                parasolid_surface_curve_records,
                parasolid_intersection_records,
                parasolid_term_use_records,
                parasolid_support_uv_records,
                parasolid_chart_records,
                parasolid_attribute_definitions,
                parasolid_entity_51_records,
                parasolid_entity_52_integer_records,
                parasolid_entity_53_double_records,
                parasolid_entity_54_string_records,
                parasolid_entity_51_numeric_uses,
                parasolid_entity_51_string_uses,
                parasolid_attribute_class_uses,
                parasolid_topology_attribute_list_references,
                parasolid_topology_attribute_class_uses,
            },
            segments: SegmentRecords {
                segment_index_rows,
                segment_om_links,
                segment_stream_links,
                segment_body_bindings,
                segment_body_lineage_statuses,
            },
            features: FeatureRecords {
                feature_operation_labels,
                feature_operation_records,
                feature_payload_strings,
                feature_simple_hole_templates,
                feature_simple_hole_repeated_scalar_lanes,
                feature_simple_hole_repeated_scalar_lane_block_references,
                feature_simple_hole_construction_groups,
                feature_body_references,
                feature_body_segment_uses,
                feature_body_reference_occurrences,
                feature_input_blocks,
                feature_input_block_identity_groups,
                feature_datum_csys_constructions,
                feature_datum_csys_payloads,
                feature_datum_csys_payload_scalar_pairs,
                feature_datum_csys_payload_fixed_pairs,
                feature_datum_csys_payload_scalars,
                feature_datum_csys_descriptors,
                feature_datum_plane_headers,
                feature_datum_plane_block_uses,
                feature_datum_plane_payloads,
                feature_datum_plane_payload_scalar_pairs,
                feature_datum_plane_descriptors,
                feature_datum_plane_csys_identity_uses,
                feature_datum_csys_block_uses,
                feature_sketch_references,
                feature_projected_curve_references,
                feature_projected_curve_construction_payloads,
                feature_projected_curve_construction_strings,
                feature_pattern_references,
                feature_pattern_construction_payloads,
                feature_pattern_construction_strings,
                feature_pattern_construction_fixed_lanes,
                feature_pattern_transform_lanes,
                feature_point_construction_headers,
                feature_point_construction_scalar_lanes,
                feature_draft_construction_references,
                feature_draft_construction_index_lanes,
                feature_draft_construction_payloads,
                feature_draft_construction_graph_payloads,
                feature_draft_construction_fixed_lanes,
                feature_draft_construction_binary32_lanes,
                feature_draft_construction_graph_strings,
                feature_draft_construction_identity_frames,
                feature_draft_construction_terminal_lanes,
                feature_surface_construction_references,
                feature_surface_construction_payloads,
                feature_surface_construction_scalar_pairs,
                feature_surface_construction_strings,
                feature_surface_construction_branches,
                feature_extrude_profile_references,
                feature_extrude_payload_headers,
                feature_extrude_payload_footers,
                feature_operation_body_scalar_triples,
                feature_operation_body_members,
                feature_operation_body_operands,
                feature_operation_body_11_continuations,
                feature_operation_body_reference_lanes,
                feature_extrude_construction_profiles,
                feature_extrude_payload_32_branches,
                feature_extrude_32_constructions,
                feature_block_construction_references,
                feature_block_constructions,
                feature_block_construction_payloads,
                feature_block_payload_scalars,
                feature_block_payload_names,
                feature_block_payload_named_records,
                feature_block_payload_points,
                feature_block_payload_point_groups,
                feature_sketch_records,
                feature_sketch_construction_inputs,
                feature_sketch_construction_payloads,
                feature_sketch_payload_coordinate_pairs,
                feature_sketch_payload_fixed_pairs,
                feature_sketch_payload_scalars,
                feature_sketch_payload_names,
                feature_sketch_payload_named_records,
                feature_sketch_fixed_points,
                feature_sketch_points,
                feature_sketch_point_groups,
                offset_store_named_points,
                feature_sketch_named_point_block_uses,
                feature_sketch_preceding_named_point_uses,
                feature_sketch_point_uses,
                feature_sketch_datum_csys_dependencies,
                feature_boolean_operations,
                data_block_object_frames,
                feature_input_column_row_uses,
                feature_input_column_targets,
                feature_parameter_bindings,
                feature_parameter_uses,
                feature_block_dimensions,
            },
            om: OmRecords {
                om_record_areas,
                expression_declarations,
                expressions,
                classes,
                fields,
                object_records,
                rmfastload_object_id_tables,
                rmfastload_object_ids,
                data_blocks,
                data_block_control_forms,
                data_block_control_values,
                data_block_control_class_references,
                data_block_control_index_values,
                data_block_control_references,
                data_block_control_handle_pairs,
                data_block_references,
                data_block_counted_index_lanes,
                data_block_abr_reference_lanes,
                data_block_index_rows,
                data_block_linked_index_rows,
                data_block_target_index_rows,
                data_block_column_index_tables,
                store_headers,
                string_values,
                object_references,
                configurations,
                part_attributes,
                configuration_attribute_uses,
                external_references,
                external_reference_records,
                external_reference_indexed_records,
                external_reference_empty_records,
                external_reference_tail_reference_pairs,
                external_reference_record_string_uses,
                external_reference_record_children,
                material_texture_assets,
                material_texture_catalog_entries,
                persistent_handles,
            },
        }
    }

    /// Whether every emptiness-counting record family is empty. Derived from
    /// [`CATALOGUE`](super::catalogue::CATALOGUE): the fold visits
    /// exactly the rows whose `counts_toward_emptiness` flag is set (133 of the
    /// 179 families), reproducing the operand set of the legacy hand-written
    /// all-empty guard. The 46 non-counting families are documented on
    /// [`CatalogueRow::counts_toward_emptiness`](super::catalogue::CatalogueRow::counts_toward_emptiness).
    /// The fold is order-insensitive — the legacy guard was a pure `&&` of
    /// `is_empty()` calls on plain `Vec`s — so it is behavior-identical to the
    /// conjunction it replaces.
    ///
    /// The caller additionally checks `object_sections`, the sole non-model
    /// operand of the legacy guard, which this method does not cover.
    pub(crate) fn is_empty(&self) -> bool {
        super::catalogue::CATALOGUE
            .iter()
            .filter(|row| row.counts_toward_emptiness)
            .all(|row| (row.len)(self) == 0)
    }
}
