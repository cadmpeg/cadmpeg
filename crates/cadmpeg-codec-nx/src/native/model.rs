// SPDX-License-Identifier: Apache-2.0
//! Eager, best-effort extraction of the native NX object model.
//!
//! [`NativeModel::extract`] runs the extraction dependency DAG and groups
//! record vectors into domain sub-structs. JT graph admission rejects
//! inconsistent owners before attachment.

use crate::native::features::draft::feature_draft_construction_binary32_lanes;
use crate::native::features::draft::feature_draft_construction_fixed_lanes;
use crate::native::features::draft::feature_draft_construction_graph_payloads;
use crate::native::features::draft::feature_draft_construction_graph_strings;
use crate::native::features::draft::feature_draft_construction_identity_frames;
use crate::native::features::draft::feature_draft_construction_index_lanes;
use crate::native::features::draft::feature_draft_construction_payloads;
use crate::native::features::draft::feature_draft_construction_references;
use crate::native::features::draft::feature_draft_construction_terminal_lanes;
use crate::native::features::draft::FeatureDraftConstructionBinary32Lane;
use crate::native::features::draft::FeatureDraftConstructionFixedLane;
use crate::native::features::draft::FeatureDraftConstructionGraphPayload;
use crate::native::features::draft::FeatureDraftConstructionGraphString;
use crate::native::features::draft::FeatureDraftConstructionIdentityFrame;
use crate::native::features::draft::FeatureDraftConstructionIndexLane;
use crate::native::features::draft::FeatureDraftConstructionReference;
use crate::native::features::draft::FeatureDraftConstructionTerminalLane;
use crate::native::features::pattern::feature_identical_instance_output_lanes;
use crate::native::features::pattern::feature_multi_instance_output_lanes;
use crate::native::features::pattern::feature_pattern_construction_fixed_lanes;
use crate::native::features::pattern::feature_pattern_construction_payloads;
use crate::native::features::pattern::feature_pattern_construction_strings;
use crate::native::features::pattern::feature_pattern_counted_reference_lanes;
use crate::native::features::pattern::feature_pattern_references;
use crate::native::features::pattern::feature_pattern_transform_lanes;
use crate::native::features::pattern::FeatureIdenticalInstanceOutputLane;
use crate::native::features::pattern::FeatureMultiInstanceOutputLane;
use crate::native::features::pattern::FeaturePatternConstructionFixedLane;
use crate::native::features::pattern::FeaturePatternConstructionString;
use crate::native::features::pattern::FeaturePatternCountedReferenceLane;
use crate::native::features::pattern::FeaturePatternReference;
use crate::native::features::pattern::FeaturePatternTransformLane;

use super::display_jt::admission::{DisplayJtGraph, DisplayJtGraphWire};
use super::display_jt::{
    display_jt_base_node_data, display_jt_compressed_element_sequences, display_jt_documents,
    display_jt_geometric_transform_attributes, display_jt_group_node_data, display_jt_indices,
    display_jt_initial_face_degree_symbols, display_jt_instance_nodes,
    display_jt_material_attributes, display_jt_partition_nodes, display_jt_polygon_meshes,
    display_jt_range_lod_nodes, display_jt_segments, display_jt_shape_lod_bindings,
    display_jt_shape_lod_elements, display_jt_string_property_atoms,
    display_jt_topology_packet_sequences, display_jt_tri_strip_lod_headers,
    display_jt_tri_strip_shape_nodes, display_jt_vertex_colors, display_jt_vertex_coordinates,
    display_jt_vertex_flags, display_jt_vertex_normals, display_jt_vertex_texture_coordinates,
    DisplayJtBaseNodeData, DisplayJtCompressedVertexRecordsHeader,
    DisplayJtGeometricTransformAttribute, DisplayJtGroupNodeData, DisplayJtIndex,
    DisplayJtInitialFaceDegreeSymbols, DisplayJtInstanceNode, DisplayJtMaterialAttribute,
    DisplayJtPartitionNode, DisplayJtPolygonMesh, DisplayJtRangeLodNode, DisplayJtShapeLodBinding,
    DisplayJtStringPropertyAtom, DisplayJtTopologyArrays, DisplayJtTopologyPacketSequence,
    DisplayJtTriStripLodHeader, DisplayJtTriStripShapeNode, DisplayJtVertexColors,
    DisplayJtVertexCoordinateArrayHeader, DisplayJtVertexCoordinates, DisplayJtVertexFlagInputs,
    DisplayJtVertexFlags, DisplayJtVertexNormals, DisplayJtVertexTextureCoordinates,
};
use super::features::construction_records::{
    data_block_object_frames, feature_block_construction_payloads,
    feature_block_construction_references, feature_block_constructions, feature_block_dimensions,
    feature_block_payload_named_records, feature_block_payload_names,
    feature_block_payload_point_groups, feature_block_payload_points,
    feature_block_payload_scalars, feature_extrude_32_constructions,
    feature_extrude_construction_profiles, feature_extrude_payload_32_branches,
    feature_extrude_payload_headers, feature_extrude_profile_references,
    feature_operation_body_11_continuations, feature_operation_body_members,
    feature_operation_body_operands, feature_operation_body_reference_lanes,
    feature_operation_body_scalar_triples, feature_operation_terminal_discriminators,
    feature_point_construction_headers, feature_point_construction_scalar_lanes,
    feature_projected_curve_construction_payloads, feature_projected_curve_construction_strings,
    feature_projected_curve_references, feature_surface_construction_payloads,
    feature_surface_construction_references, feature_surface_construction_scalar_pairs,
    feature_surface_construction_strings, feature_swp104_leading_branches,
    feature_thru_curve_construction_envelopes,
};
use super::features::operation_record::FeatureOperationRecord;
use super::features::unlabeled_record::FeatureUnlabeledOperationRecord;
use super::features::{
    feature_body_data_block_uses, feature_body_reference_occurrences, feature_body_references,
    feature_body_segment_uses, feature_body_write_group_partition_uses, feature_boolean_operations,
    feature_datum_csys_block_uses, feature_datum_csys_column_row_uses,
    feature_datum_csys_constructions, feature_datum_csys_descriptors,
    feature_datum_csys_payload_fixed_pairs, feature_datum_csys_payload_scalar_pairs,
    feature_datum_csys_payload_scalars, feature_datum_csys_payloads,
    feature_datum_plane_block_uses, feature_datum_plane_csys_identity_uses,
    feature_datum_plane_descriptors, feature_datum_plane_payload_scalar_pairs,
    feature_datum_plane_payloads, feature_input_block_identity_groups, feature_input_blocks,
    feature_input_column_row_uses, feature_input_column_targets,
    feature_operation_body_identity_segment_uses, feature_operation_body_image_segment_uses,
    feature_operation_body_partition_uses, feature_operation_body_writes,
    feature_operation_common_frames, feature_operation_labels, feature_operation_object_references,
    feature_operation_records, feature_operation_state_journal_uses,
    feature_operation_terminal_frames, feature_parameter_bindings, feature_parameter_uses,
    feature_payload_strings, feature_sketch_construction_inputs,
    feature_sketch_construction_payloads, feature_sketch_datum_csys_dependencies,
    feature_sketch_fixed_points, feature_sketch_named_point_block_uses,
    feature_sketch_payload_coordinate_pairs, feature_sketch_payload_fixed_pairs,
    feature_sketch_payload_mixed_pairs, feature_sketch_payload_named_records,
    feature_sketch_payload_names, feature_sketch_payload_scalar_lanes,
    feature_sketch_payload_scalars, feature_sketch_point_groups, feature_sketch_point_uses,
    feature_sketch_points, feature_sketch_preceding_named_point_uses, feature_sketch_records,
    feature_sketch_references, feature_unlabeled_operation_body_writes,
    feature_unlabeled_operation_records, offset_store_named_points, FeatureBlockConstruction,
    FeatureBlockDimensions, FeatureBlockPayloadNamedRecord, FeatureBlockPayloadPoint,
    FeatureBlockPayloadPointGroup, FeatureBodyDataBlockUse, FeatureBodyReference,
    FeatureBodySegmentUse, FeatureBodyWriteGroupPartitionUse, FeatureBooleanOperation,
    FeatureConstructionPayload, FeatureDatumCsysBlockUse, FeatureDatumCsysColumnRowUse,
    FeatureDatumCsysConstruction, FeatureDatumCsysDescriptor, FeatureDatumCsysPayload,
    FeatureDatumCsysPayloadFixedPair, FeatureDatumPlaneBlockUse, FeatureDatumPlaneCsysIdentityUse,
    FeatureDatumPlaneDescriptor, FeatureDatumPlanePayload, FeatureExtrudeConstructionProfile,
    FeatureExtrudePayloadHeader, FeatureExtrudeProfileReference, FeatureInputBlock,
    FeatureInputBlockIdentityGroup, FeatureInputColumnRowUse, FeatureInputColumnTarget,
    FeatureOperationBody11Continuation, FeatureOperationBodyIdentitySegmentUse,
    FeatureOperationBodyImageSegmentUse, FeatureOperationBodyMember, FeatureOperationBodyOperand,
    FeatureOperationBodyPartitionUse, FeatureOperationBodyReferenceLane, FeatureOperationBodyWrite,
    FeatureOperationCommonFrame, FeatureOperationLabel, FeatureOperationObjectReference,
    FeatureOperationStateJournalUse, FeatureOperationTerminalFrame, FeatureParameterBinding,
    FeatureParameterUse, FeaturePayloadScalar, FeaturePayloadScalarPair, FeaturePayloadString,
    FeaturePointConstructionHeader, FeatureProjectedCurveConstructionString,
    FeatureProjectedCurveReference, FeatureSketchConstructionInputs,
    FeatureSketchDatumCsysDependency, FeatureSketchFixedPoint, FeatureSketchNamedPointBlockUse,
    FeatureSketchPayloadFixedPair, FeatureSketchPayloadMixedPair, FeatureSketchPayloadNamedRecord,
    FeatureSketchPayloadScalarLane, FeatureSketchPoint, FeatureSketchPointGroup,
    FeatureSketchPointUse, FeatureSketchPrecedingNamedPointUse, FeatureSketchRecord,
    FeatureSketchReference, FeatureSurfaceConstructionPayload, FeatureSurfaceConstructionReference,
    FeatureSurfaceConstructionString, FeatureThruCurveConstructionEnvelope, OffsetStoreNamedPoint,
};
use super::om::{
    audit_trail_rows, class_definitions, configuration_attribute_uses, configurations,
    data_block_column_index_tables, data_block_control_class_references, data_block_control_forms,
    data_block_control_handle_pairs, data_block_control_index_values,
    data_block_control_references, data_block_control_values, data_block_references, data_blocks,
    expression_declarations, expressions, external_reference_empty_records,
    external_reference_indexed_records, external_reference_record_children,
    external_reference_record_string_uses, external_reference_records,
    external_reference_tail_reference_pairs, external_references, field_definitions,
    material_texture_catalog_entries, object_record_handle_pairs, object_records,
    object_references, om_record_areas, operation_state_counters, operation_state_groups,
    operation_state_journal_groups, operation_state_messages, operation_state_slot_lanes,
    operation_state_statuses, part_attributes, part_color_tables, persistent_handles,
    rmfastload_object_id_table, store_headers, string_values, ClassDefinition, Configuration,
    ConfigurationAttributeUse, DataBlock, DataBlockColumnIndexTable,
    DataBlockControlClassReference, DataBlockControlForm, DataBlockControlHandlePair,
    DataBlockControlIndexValue, DataBlockControlReference, DataBlockControlValue,
    DataBlockReference, ExpressionDeclaration, ExternalReference, ExternalReferenceEmptyRecord,
    ExternalReferenceIndexedRecord, ExternalReferenceRecord, ExternalReferenceRecordChild,
    ExternalReferenceRecordStringUse, ExternalReferenceTailReferencePair, FieldDefinition,
    MaterialTextureCatalogEntry, ObjectRecord, ObjectRecordHandlePair, ObjectReference,
    OmAuditTrailRow, OmOperationStateCounter, OmOperationStateMessage, OmRecordArea,
    ParameterFormula, PartAttribute, PartColorDefinition, PartColorTable, PersistentHandle,
    RmFastLoadObjectId, RmFastLoadObjectIdTable, StoreHeader, StringValue,
};
use super::parasolid::{
    parasolid_attribute_class_uses, parasolid_attribute_definitions,
    parasolid_attribute_field_names, parasolid_attribute_field_uses, parasolid_blend_bound_records,
    parasolid_blend_surface_records, parasolid_chart_records,
    parasolid_deltas_events_with_censuses, parasolid_entity_51_numeric_uses,
    parasolid_entity_51_records, parasolid_entity_51_string_uses,
    parasolid_entity_51_structured_uses, parasolid_entity_value_records,
    parasolid_field_names_records, parasolid_group_members, parasolid_group_records,
    parasolid_intersection_records, parasolid_offset_surface_records, parasolid_support_uv_records,
    parasolid_surface_curve_records, parasolid_term_use_records,
    parasolid_topology_attribute_class_uses,
    parasolid_topology_attribute_fields_have_untransferred_values,
    parasolid_topology_attribute_list_references, parasolid_trimmed_curve_records,
    ParasolidAttributeClassUse, ParasolidAttributeDefinition, ParasolidAttributeFieldNames,
    ParasolidAttributeFieldUse, ParasolidBlendBoundRecord, ParasolidBlendSurfaceRecord,
    ParasolidChartRecord, ParasolidDeltasBodyRevision, ParasolidDeltasInlineBodyState,
    ParasolidDeltasInlineSchemaDeclaration, ParasolidDeltasRecord,
    ParasolidDeltasReferenceMarkerPacket, ParasolidDeltasReferenceStatePacket,
    ParasolidDeltasReferenceTypeMap, ParasolidDeltasResidualSpan,
    ParasolidDeltasSchemaReferencePreamble, ParasolidDeltasTaggedReferenceLane,
    ParasolidDeltasTermUseNumericTail, ParasolidDeltasTerminalNullReferences,
    ParasolidDeltasTombstone, ParasolidDeltasTransmitHeader, ParasolidDeltasType150StatePacket,
    ParasolidEntity51NumericUse, ParasolidEntity51Record, ParasolidEntity51StringUse,
    ParasolidEntity51StructuredUse, ParasolidEntity52IntegerRecord, ParasolidEntity53DoubleRecord,
    ParasolidEntity54StringRecord, ParasolidEntity57AxisRecord, ParasolidEntity58TagRecord,
    ParasolidEntity62UnicodeRecord, ParasolidEntityVectorRecord, ParasolidFieldNamesRecord,
    ParasolidGroupMember, ParasolidGroupRecord, ParasolidIntersectionRecord,
    ParasolidOffsetSurfaceRecord, ParasolidSupportUvRecord, ParasolidSurfaceCurveRecord,
    ParasolidTermUseRecord, ParasolidTopologyAttributeClassUse,
    ParasolidTopologyAttributeListReference, ParasolidTrimmedCurveRecord,
};
use super::segments::{
    segment_body_bindings, segment_body_lineage_statuses, segment_index_rows, segment_om_links,
    segment_stream_links, SegmentBodyBinding, SegmentBodyLineageStatus, SegmentIndexRow,
    SegmentOmLink, SegmentStreamLink,
};
use super::structure::{
    fast_load_component_object_groups, fast_load_component_roster, FastLoadComponentObjectGroup,
    FastLoadComponentPrototype, FastLoadComponentUuid,
};
use super::substrate::{pair_stream_indices, ParsedStreams};
use super::toggle::{saved_toggle_records, SavedToggleEntry, SavedToggleStream};
use crate::container::Container;
use crate::native::features::block_reference::FeatureBlockConstructionReference;
use crate::native::features::body_scalar_triple::FeatureOperationBodyScalarTriple;
use crate::native::features::datum_plane_header::{
    feature_datum_plane_headers, FeatureDatumPlaneHeader,
};
use crate::native::features::delete::{
    feature_delete_construction_payloads, feature_delete_reference_fields,
    FeatureDeleteConstructionPayload, FeatureDeleteReferenceField,
};
use crate::native::features::extrude_32::{
    FeatureExtrude32Construction, FeatureExtrudePayload32Branch,
};
use crate::native::features::fset::{
    feature_fset_construction_payloads, feature_fset_reference_graphs, FeatureFsetReferenceGraph,
};
use crate::native::features::holes::feature_hole_package_construction_group_lanes;
use crate::native::features::holes::feature_hole_package_construction_group_uses;
use crate::native::features::holes::feature_simple_hole_construction_groups;
use crate::native::features::holes::feature_simple_hole_repeated_scalar_lane_block_references;
use crate::native::features::holes::feature_simple_hole_repeated_scalar_lanes;
use crate::native::features::holes::feature_simple_hole_templates;
use crate::native::features::holes::feature_symbolic_threads;
use crate::native::features::holes::feature_threaded_hole_templates;
use crate::native::features::holes::FeatureHolePackageConstructionGroupLane;
use crate::native::features::holes::FeatureHolePackageConstructionGroupUse;
use crate::native::features::holes::FeatureSimpleHoleConstructionGroup;
use crate::native::features::holes::FeatureSimpleHoleRepeatedScalarLane;
use crate::native::features::holes::FeatureSimpleHoleRepeatedScalarLaneBlockReferences;
use crate::native::features::holes::FeatureSimpleHoleTemplate;
use crate::native::features::holes::FeatureSymbolicThread;
use crate::native::features::holes::FeatureThreadedHoleTemplate;
use crate::native::features::object_frame::DataBlockObjectFrame;
use crate::native::features::payload_name::FeaturePayloadName;
use crate::native::features::point_scalar_lane::FeaturePointConstructionScalarLane;
use crate::native::features::surface_branches::{
    feature_surface_construction_branches, FeatureSurfaceConstructionBranch,
};
use crate::native::features::swp104_branch::FeatureSwp104LeadingBranch;
use crate::native::features::terminal_discriminator::FeatureOperationTerminalDiscriminator;
use crate::native::features::thru_curve_branches::{
    feature_thru_curve_construction_branch_groups, FeatureThruCurveConstructionBranchGroup,
};
use crate::native::om::column_row::{
    data_block_index_rows, data_block_linked_index_rows, data_block_target_index_rows,
    DataBlockIndexRow, DataBlockLinkedIndexRow, DataBlockTargetIndexRow,
};
use crate::native::om::compact_lane::{
    data_block_abr_reference_lanes, data_block_counted_index_lanes, DataBlockAbrReferenceLane,
    DataBlockCountedIndexLane,
};
use crate::native::om::creation_display::{
    rm_creation_display_data_relations, RmCreationDisplayDataRelation,
};
use crate::native::om::display_color::{rm_display_color_assignments, RmDisplayColorAssignment};
use crate::native::om::journal_group::OmOperationStateJournalGroup;
use crate::native::om::material_texture::{material_texture_assets, MaterialTextureAsset};
use crate::native::om::object_uuid::{object_uuid_values, ObjectUuidValue};
use crate::native::om::roll_forward::OmRollForwardStateTable;
use crate::native::om::state_slot_lane::OmOperationStateSlotLane;
use crate::native::om::state_status::OmOperationStateStatus;
use crate::parasolid::Stream;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::BodyId;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

/// Records extracted from the `display_jt` domain.
pub(in crate::native) struct DisplayJtRecords {
    pub(super) graph: DisplayJtGraph,
    pub(super) indices: Vec<DisplayJtIndex>,
    pub(super) tri_strip_lod_headers: Vec<DisplayJtTriStripLodHeader>,
    pub(super) initial_face_degree_symbols: Vec<DisplayJtInitialFaceDegreeSymbols>,
    pub(super) topology_packet_sequences: Vec<DisplayJtTopologyPacketSequence>,
    pub(super) vertex_records_headers: Vec<DisplayJtCompressedVertexRecordsHeader>,
    pub(super) coordinate_array_headers: Vec<DisplayJtVertexCoordinateArrayHeader>,
    pub(super) vertex_coordinates: Vec<DisplayJtVertexCoordinates>,
    pub(super) vertex_normals: Vec<DisplayJtVertexNormals>,
    pub(super) vertex_colors: Vec<DisplayJtVertexColors>,
    pub(super) vertex_texture_coordinates: Vec<DisplayJtVertexTextureCoordinates>,
    pub(super) vertex_flags: Vec<DisplayJtVertexFlags>,
    pub(super) polygon_meshes: Vec<DisplayJtPolygonMesh>,
    pub(super) string_property_atoms: Vec<DisplayJtStringPropertyAtom>,
    pub(super) shape_lod_bindings: Vec<DisplayJtShapeLodBinding>,
    pub(super) base_node_data: Vec<DisplayJtBaseNodeData>,
    pub(super) group_node_data: Vec<DisplayJtGroupNodeData>,
    pub(super) instance_nodes: Vec<DisplayJtInstanceNode>,
    pub(super) geometric_transform_attributes: Vec<DisplayJtGeometricTransformAttribute>,
    pub(super) material_attributes: Vec<DisplayJtMaterialAttribute>,
    pub(super) partition_nodes: Vec<DisplayJtPartitionNode>,
    pub(super) range_lod_nodes: Vec<DisplayJtRangeLodNode>,
    pub(super) tri_strip_shape_nodes: Vec<DisplayJtTriStripShapeNode>,
}

/// Records extracted from the `parasolid` domain.
pub(in crate::native) struct ParasolidRecords {
    pub(super) group_records: Vec<ParasolidGroupRecord>,
    pub(super) group_members: Vec<ParasolidGroupMember>,
    pub(super) deltas_transmit_headers: Vec<ParasolidDeltasTransmitHeader>,
    pub(super) deltas_terminal_null_references: Vec<ParasolidDeltasTerminalNullReferences>,
    pub(super) deltas_records: Vec<ParasolidDeltasRecord>,
    pub(super) deltas_tombstones: Vec<ParasolidDeltasTombstone>,
    pub(super) deltas_body_revisions: Vec<ParasolidDeltasBodyRevision>,
    pub(super) deltas_term_use_numeric_tails: Vec<ParasolidDeltasTermUseNumericTail>,
    pub(super) deltas_tagged_reference_lanes: Vec<ParasolidDeltasTaggedReferenceLane>,
    pub(super) deltas_reference_type_maps: Vec<ParasolidDeltasReferenceTypeMap>,
    pub(super) deltas_reference_state_packets: Vec<ParasolidDeltasReferenceStatePacket>,
    pub(super) deltas_schema_reference_preambles: Vec<ParasolidDeltasSchemaReferencePreamble>,
    pub(super) deltas_reference_marker_packets: Vec<ParasolidDeltasReferenceMarkerPacket>,
    pub(super) deltas_type_150_state_packets: Vec<ParasolidDeltasType150StatePacket>,
    pub(super) deltas_inline_schema_declarations: Vec<ParasolidDeltasInlineSchemaDeclaration>,
    pub(super) deltas_inline_body_states: Vec<ParasolidDeltasInlineBodyState>,
    pub(super) deltas_residual_spans: Vec<ParasolidDeltasResidualSpan>,
    pub(super) blend_surface_records: Vec<ParasolidBlendSurfaceRecord>,
    pub(super) blend_bound_records: Vec<ParasolidBlendBoundRecord>,
    pub(super) offset_surface_records: Vec<ParasolidOffsetSurfaceRecord>,
    pub(super) trimmed_curve_records: Vec<ParasolidTrimmedCurveRecord>,
    pub(super) surface_curve_records: Vec<ParasolidSurfaceCurveRecord>,
    pub(super) intersection_records: Vec<ParasolidIntersectionRecord>,
    pub(super) term_use_records: Vec<ParasolidTermUseRecord>,
    pub(super) support_uv_records: Vec<ParasolidSupportUvRecord>,
    pub(super) chart_records: Vec<ParasolidChartRecord>,
    pub(super) attribute_definitions: Vec<ParasolidAttributeDefinition>,
    pub(super) field_names_records: Vec<ParasolidFieldNamesRecord>,
    pub(super) attribute_field_names: Vec<ParasolidAttributeFieldNames>,
    pub(super) entity_51_records: Vec<ParasolidEntity51Record>,
    pub(super) entity_52_integer_records: Vec<ParasolidEntity52IntegerRecord>,
    pub(super) entity_53_double_records: Vec<ParasolidEntity53DoubleRecord>,
    pub(super) entity_54_string_records: Vec<ParasolidEntity54StringRecord>,
    pub(super) entity_vector_records: Vec<ParasolidEntityVectorRecord>,
    pub(super) entity_57_axis_records: Vec<ParasolidEntity57AxisRecord>,
    pub(super) entity_58_tag_records: Vec<ParasolidEntity58TagRecord>,
    pub(super) entity_62_unicode_records: Vec<ParasolidEntity62UnicodeRecord>,
    pub(super) entity_51_numeric_uses: Vec<ParasolidEntity51NumericUse>,
    pub(super) entity_51_string_uses: Vec<ParasolidEntity51StringUse>,
    pub(super) entity_51_structured_uses: Vec<ParasolidEntity51StructuredUse>,
    pub(super) attribute_class_uses: Vec<ParasolidAttributeClassUse>,
    pub(super) attribute_field_uses: Vec<ParasolidAttributeFieldUse>,
    pub(super) topology_attribute_list_references: Vec<ParasolidTopologyAttributeListReference>,
    pub(super) topology_attribute_class_uses: Vec<ParasolidTopologyAttributeClassUse>,
}

/// Records extracted from the `segments` domain.
pub(crate) struct SegmentRecords {
    pub(super) index_rows: Vec<SegmentIndexRow>,
    pub(super) om_links: Vec<SegmentOmLink>,
    pub(super) stream_links: Vec<SegmentStreamLink>,
    pub(crate) body_bindings: Vec<SegmentBodyBinding>,
    pub(crate) body_lineage_statuses: Vec<SegmentBodyLineageStatus>,
}

/// Records extracted from the fast-load `structure` domain.
pub(in crate::native) struct StructureRecords {
    pub(super) prototypes: Vec<FastLoadComponentPrototype>,
    pub(super) uuids: Vec<FastLoadComponentUuid>,
    pub(super) occurrences: super::structure::occurrences::FastLoadOccurrences,
    pub(super) object_groups: Vec<FastLoadComponentObjectGroup>,
}

/// Records extracted from the saved toggle-information stream.
pub(in crate::native) struct ToggleRecords {
    pub(super) streams: Vec<SavedToggleStream>,
    pub(super) entries: Vec<SavedToggleEntry>,
}

/// Records extracted from the `features` domain.
pub(in crate::native) struct FeatureRecords {
    pub(super) feature_operation_labels: Vec<FeatureOperationLabel>,
    pub(super) feature_operation_records: Vec<FeatureOperationRecord>,
    pub(super) feature_unlabeled_operation_records: Vec<FeatureUnlabeledOperationRecord>,
    pub(super) feature_unlabeled_operation_body_writes: Vec<FeatureOperationBodyWrite>,
    pub(super) feature_operation_body_writes: Vec<FeatureOperationBodyWrite>,
    pub(super) feature_operation_body_image_segment_uses: Vec<FeatureOperationBodyImageSegmentUse>,
    pub(super) feature_operation_body_identity_segment_uses:
        Vec<FeatureOperationBodyIdentitySegmentUse>,
    pub(super) feature_operation_body_partition_uses: Vec<FeatureOperationBodyPartitionUse>,
    pub(super) feature_body_write_group_partition_uses: Vec<FeatureBodyWriteGroupPartitionUse>,
    pub(super) feature_operation_tagged_references: Vec<FeatureOperationObjectReference>,
    pub(super) feature_operation_data_block_references: Vec<FeatureOperationObjectReference>,
    pub(super) feature_operation_common_frames: Vec<FeatureOperationCommonFrame>,
    pub(super) feature_operation_terminal_discriminators:
        Vec<FeatureOperationTerminalDiscriminator>,
    pub(super) feature_operation_terminal_frames: Vec<FeatureOperationTerminalFrame>,
    pub(super) feature_operation_state_journal_uses: Vec<FeatureOperationStateJournalUse>,
    pub(super) feature_payload_strings: Vec<FeaturePayloadString>,
    pub(super) feature_symbolic_threads: Vec<FeatureSymbolicThread>,
    pub(super) feature_threaded_hole_templates: Vec<FeatureThreadedHoleTemplate>,
    pub(super) feature_simple_hole_templates: Vec<FeatureSimpleHoleTemplate>,
    pub(super) feature_simple_hole_repeated_scalar_lanes: Vec<FeatureSimpleHoleRepeatedScalarLane>,
    pub(super) feature_simple_hole_repeated_scalar_lane_block_references:
        Vec<FeatureSimpleHoleRepeatedScalarLaneBlockReferences>,
    pub(super) feature_simple_hole_construction_groups: Vec<FeatureSimpleHoleConstructionGroup>,
    pub(super) feature_hole_package_construction_group_lanes:
        Vec<FeatureHolePackageConstructionGroupLane>,
    pub(super) feature_hole_package_construction_group_uses:
        Vec<FeatureHolePackageConstructionGroupUse>,
    pub(super) feature_body_references: Vec<FeatureBodyReference>,
    pub(super) feature_body_segment_uses: Vec<FeatureBodySegmentUse>,
    pub(super) feature_body_data_block_uses: Vec<FeatureBodyDataBlockUse>,
    pub(super) feature_body_reference_occurrences: Vec<FeatureBodyReference>,
    pub(super) feature_input_blocks: Vec<FeatureInputBlock>,
    pub(super) feature_input_block_identity_groups: Vec<FeatureInputBlockIdentityGroup>,
    pub(super) feature_datum_csys_constructions: Vec<FeatureDatumCsysConstruction>,
    pub(super) feature_datum_csys_column_row_uses: Vec<FeatureDatumCsysColumnRowUse>,
    pub(super) feature_datum_csys_payloads: Vec<FeatureDatumCsysPayload>,
    pub(super) feature_datum_csys_payload_scalar_pairs: Vec<FeaturePayloadScalarPair>,
    pub(super) feature_datum_csys_payload_fixed_pairs: Vec<FeatureDatumCsysPayloadFixedPair>,
    pub(super) feature_datum_csys_payload_scalars: Vec<FeaturePayloadScalar>,
    pub(super) feature_datum_csys_descriptors: Vec<FeatureDatumCsysDescriptor>,
    pub(super) feature_datum_plane_headers: Vec<FeatureDatumPlaneHeader>,
    pub(super) feature_datum_plane_block_uses: Vec<FeatureDatumPlaneBlockUse>,
    pub(super) feature_datum_plane_payloads: Vec<FeatureDatumPlanePayload>,
    pub(super) feature_datum_plane_payload_scalar_pairs: Vec<FeaturePayloadScalarPair>,
    pub(super) feature_datum_plane_descriptors: Vec<FeatureDatumPlaneDescriptor>,
    pub(super) feature_datum_plane_csys_identity_uses: Vec<FeatureDatumPlaneCsysIdentityUse>,
    pub(super) feature_datum_csys_block_uses: Vec<FeatureDatumCsysBlockUse>,
    pub(super) feature_sketch_references: Vec<FeatureSketchReference>,
    pub(super) feature_projected_curve_references: Vec<FeatureProjectedCurveReference>,
    pub(super) feature_projected_curve_construction_payloads: Vec<FeatureConstructionPayload>,
    pub(super) feature_projected_curve_construction_strings:
        Vec<FeatureProjectedCurveConstructionString>,
    pub(super) feature_fset_reference_graphs: Vec<FeatureFsetReferenceGraph>,
    pub(super) feature_fset_construction_payloads: Vec<FeatureConstructionPayload>,
    pub(super) feature_delete_reference_fields: Vec<FeatureDeleteReferenceField>,
    pub(super) feature_delete_construction_payloads: Vec<FeatureDeleteConstructionPayload>,
    pub(super) feature_pattern_references: Vec<FeaturePatternReference>,
    pub(super) feature_pattern_counted_reference_lanes: Vec<FeaturePatternCountedReferenceLane>,
    pub(super) feature_pattern_construction_payloads: Vec<FeatureConstructionPayload>,
    pub(super) feature_pattern_construction_strings: Vec<FeaturePatternConstructionString>,
    pub(super) feature_pattern_construction_fixed_lanes: Vec<FeaturePatternConstructionFixedLane>,
    pub(super) feature_pattern_transform_lanes: Vec<FeaturePatternTransformLane>,
    pub(super) feature_multi_instance_output_lanes: Vec<FeatureMultiInstanceOutputLane>,
    pub(super) feature_identical_instance_output_lanes: Vec<FeatureIdenticalInstanceOutputLane>,
    pub(super) feature_point_construction_headers: Vec<FeaturePointConstructionHeader>,
    pub(super) feature_point_construction_scalar_lanes: Vec<FeaturePointConstructionScalarLane>,
    pub(super) feature_draft_construction_references: Vec<FeatureDraftConstructionReference>,
    pub(super) feature_draft_construction_index_lanes: Vec<FeatureDraftConstructionIndexLane>,
    pub(super) feature_draft_construction_payloads: Vec<FeatureConstructionPayload>,
    pub(super) feature_draft_construction_graph_payloads: Vec<FeatureDraftConstructionGraphPayload>,
    pub(super) feature_draft_construction_fixed_lanes: Vec<FeatureDraftConstructionFixedLane>,
    pub(super) feature_draft_construction_binary32_lanes: Vec<FeatureDraftConstructionBinary32Lane>,
    pub(super) feature_draft_construction_graph_strings: Vec<FeatureDraftConstructionGraphString>,
    pub(super) feature_draft_construction_identity_frames:
        Vec<FeatureDraftConstructionIdentityFrame>,
    pub(super) feature_draft_construction_terminal_lanes: Vec<FeatureDraftConstructionTerminalLane>,
    pub(super) feature_surface_construction_references: Vec<FeatureSurfaceConstructionReference>,
    pub(super) feature_surface_construction_payloads: Vec<FeatureSurfaceConstructionPayload>,
    pub(super) feature_surface_construction_scalar_pairs: Vec<FeaturePayloadScalarPair>,
    pub(super) feature_surface_construction_strings: Vec<FeatureSurfaceConstructionString>,
    pub(super) feature_surface_construction_branches: Vec<FeatureSurfaceConstructionBranch>,
    pub(super) feature_swp104_leading_branches: Vec<FeatureSwp104LeadingBranch>,
    pub(super) feature_thru_curve_construction_branch_groups:
        Vec<FeatureThruCurveConstructionBranchGroup>,
    pub(super) feature_thru_curve_construction_envelopes: Vec<FeatureThruCurveConstructionEnvelope>,
    pub(super) feature_extrude_profile_references: Vec<FeatureExtrudeProfileReference>,
    pub(super) feature_extrude_payload_headers: Vec<FeatureExtrudePayloadHeader>,
    pub(super) feature_operation_body_scalar_triples: Vec<FeatureOperationBodyScalarTriple>,
    pub(super) feature_operation_body_members: Vec<FeatureOperationBodyMember>,
    pub(super) feature_operation_body_operands: Vec<FeatureOperationBodyOperand>,
    pub(super) feature_operation_body_11_continuations: Vec<FeatureOperationBody11Continuation>,
    pub(super) feature_operation_body_reference_lanes: Vec<FeatureOperationBodyReferenceLane>,
    pub(super) feature_extrude_construction_profiles: Vec<FeatureExtrudeConstructionProfile>,
    pub(super) feature_extrude_payload_32_branches: Vec<FeatureExtrudePayload32Branch>,
    pub(super) feature_extrude_32_constructions: Vec<FeatureExtrude32Construction>,
    pub(super) feature_block_construction_references: Vec<FeatureBlockConstructionReference>,
    pub(super) feature_block_constructions: Vec<FeatureBlockConstruction>,
    pub(super) feature_block_construction_payloads: Vec<FeatureConstructionPayload>,
    pub(super) feature_block_payload_scalars: Vec<FeaturePayloadScalar>,
    pub(super) feature_block_payload_names: Vec<FeaturePayloadName>,
    pub(super) feature_block_payload_named_records: Vec<FeatureBlockPayloadNamedRecord>,
    pub(super) feature_block_payload_points: Vec<FeatureBlockPayloadPoint>,
    pub(super) feature_block_payload_point_groups: Vec<FeatureBlockPayloadPointGroup>,
    pub(super) feature_sketch_records: Vec<FeatureSketchRecord>,
    pub(super) feature_sketch_construction_inputs: Vec<FeatureSketchConstructionInputs>,
    pub(super) feature_sketch_construction_payloads: Vec<FeatureConstructionPayload>,
    pub(super) feature_sketch_payload_coordinate_pairs: Vec<FeaturePayloadScalarPair>,
    pub(super) feature_sketch_payload_fixed_pairs: Vec<FeatureSketchPayloadFixedPair>,
    pub(super) feature_sketch_payload_mixed_pairs: Vec<FeatureSketchPayloadMixedPair>,
    pub(super) feature_sketch_payload_scalars: Vec<FeaturePayloadScalar>,
    pub(super) feature_sketch_payload_scalar_lanes: Vec<FeatureSketchPayloadScalarLane>,
    pub(super) feature_sketch_payload_names: Vec<FeaturePayloadName>,
    pub(super) feature_sketch_payload_named_records: Vec<FeatureSketchPayloadNamedRecord>,
    pub(super) feature_sketch_fixed_points: Vec<FeatureSketchFixedPoint>,
    pub(super) feature_sketch_points: Vec<FeatureSketchPoint>,
    pub(super) feature_sketch_point_groups: Vec<FeatureSketchPointGroup>,
    pub(super) offset_store_named_points: Vec<OffsetStoreNamedPoint>,
    pub(super) feature_sketch_named_point_block_uses: Vec<FeatureSketchNamedPointBlockUse>,
    pub(super) feature_sketch_preceding_named_point_uses: Vec<FeatureSketchPrecedingNamedPointUse>,
    pub(super) feature_sketch_point_uses: Vec<FeatureSketchPointUse>,
    pub(super) feature_sketch_datum_csys_dependencies: Vec<FeatureSketchDatumCsysDependency>,
    pub(super) feature_boolean_operations: Vec<FeatureBooleanOperation>,
    pub(super) data_block_object_frames: Vec<DataBlockObjectFrame>,
    pub(super) feature_input_column_row_uses: Vec<FeatureInputColumnRowUse>,
    pub(super) feature_input_column_targets: Vec<FeatureInputColumnTarget>,
    pub(super) feature_parameter_bindings: Vec<FeatureParameterBinding>,
    pub(super) feature_parameter_uses: Vec<FeatureParameterUse>,
    pub(super) feature_block_dimensions: Vec<FeatureBlockDimensions>,
}

/// Records extracted from the `om` domain.
pub(in crate::native) struct OmRecords {
    pub(super) om_record_areas: Vec<OmRecordArea>,
    pub(super) audit_trail_rows: Vec<OmAuditTrailRow>,
    pub(super) operation_state_journal_groups: Vec<OmOperationStateJournalGroup>,
    pub(super) operation_state_counters: Vec<OmOperationStateCounter>,
    pub(super) operation_state_groups: Vec<OmRollForwardStateTable>,
    pub(super) operation_state_messages: Vec<OmOperationStateMessage>,
    pub(super) operation_state_statuses: Vec<OmOperationStateStatus>,
    pub(super) operation_state_slot_lanes: Vec<OmOperationStateSlotLane>,
    pub(super) expression_declarations: Vec<ExpressionDeclaration>,
    pub(super) expressions: Vec<ParameterFormula>,
    pub(super) classes: Vec<ClassDefinition>,
    pub(super) fields: Vec<FieldDefinition>,
    pub(super) object_records: Vec<ObjectRecord>,
    pub(super) rmfastload_object_id_tables: Vec<RmFastLoadObjectIdTable>,
    pub(super) rmfastload_object_ids: Vec<RmFastLoadObjectId>,
    pub(super) data_blocks: Vec<DataBlock>,
    pub(super) data_block_control_forms: Vec<DataBlockControlForm>,
    pub(super) data_block_control_values: Vec<DataBlockControlValue>,
    pub(super) data_block_control_class_references: Vec<DataBlockControlClassReference>,
    pub(super) data_block_control_index_values: Vec<DataBlockControlIndexValue>,
    pub(super) data_block_control_references: Vec<DataBlockControlReference>,
    pub(super) data_block_control_handle_pairs: Vec<DataBlockControlHandlePair>,
    pub(super) data_block_references: Vec<DataBlockReference>,
    pub(super) data_block_counted_index_lanes: Vec<DataBlockCountedIndexLane>,
    pub(super) data_block_abr_reference_lanes: Vec<DataBlockAbrReferenceLane>,
    pub(super) data_block_index_rows: Vec<DataBlockIndexRow>,
    pub(super) data_block_linked_index_rows: Vec<DataBlockLinkedIndexRow>,
    pub(super) data_block_target_index_rows: Vec<DataBlockTargetIndexRow>,
    pub(super) rm_creation_display_data_relations: Vec<RmCreationDisplayDataRelation>,
    pub(super) part_color_tables: Vec<PartColorTable>,
    pub(super) part_color_definitions: Vec<PartColorDefinition>,
    pub(super) rm_display_color_assignments: Vec<RmDisplayColorAssignment>,
    pub(super) data_block_column_index_tables: Vec<DataBlockColumnIndexTable>,
    pub(super) store_headers: Vec<StoreHeader>,
    pub(super) string_values: Vec<StringValue>,
    pub(super) object_uuid_values: Vec<ObjectUuidValue>,
    pub(super) object_references: Vec<ObjectReference>,
    pub(super) object_record_handle_pairs: Vec<ObjectRecordHandlePair>,
    pub(super) configurations: Vec<Configuration>,
    pub(super) part_attributes: Vec<PartAttribute>,
    pub(super) configuration_attribute_uses: Vec<ConfigurationAttributeUse>,
    pub(super) external_references: Vec<ExternalReference>,
    pub(super) external_reference_records: Vec<ExternalReferenceRecord>,
    pub(super) external_reference_indexed_records: Vec<ExternalReferenceIndexedRecord>,
    pub(super) external_reference_empty_records: Vec<ExternalReferenceEmptyRecord>,
    pub(super) external_reference_tail_reference_pairs: Vec<ExternalReferenceTailReferencePair>,
    pub(super) external_reference_record_string_uses: Vec<ExternalReferenceRecordStringUse>,
    pub(super) external_reference_record_children: Vec<ExternalReferenceRecordChild>,
    pub(super) material_texture_assets: Vec<MaterialTextureAsset>,
    pub(super) material_texture_catalog_entries: Vec<MaterialTextureCatalogEntry>,
    pub(super) persistent_handles: Vec<PersistentHandle>,
}

/// The complete set of native records extracted from one scanned part, grouped
/// by domain. A wide struct by necessity: the attachment tier needs per-family
/// access to every record vector.
pub(crate) struct NativeModel {
    pub(super) display_jt: DisplayJtRecords,
    pub(super) parasolid: ParasolidRecords,
    pub(crate) segments: SegmentRecords,
    pub(super) structure: StructureRecords,
    pub(super) toggle: ToggleRecords,
    pub(super) features: FeatureRecords,
    pub(super) om: OmRecords,
}

/// The segment-history subset needed before geometry construction can decide
/// which body images are current. The full native extraction reuses these
/// inputs so history selection does not require a second container scan.
pub(crate) struct SegmentLineage {
    pub(crate) bindings: Vec<SegmentBodyBinding>,
    labels: Vec<FeatureOperationLabel>,
    references: Vec<FeatureBodyReference>,
    data_blocks: Vec<DataBlock>,
    inputs: Vec<FeatureInputBlock>,
    body_data_block_uses: Vec<FeatureBodyDataBlockUse>,
    body_reference_occurrences: Vec<FeatureBodyReference>,
    members: Vec<FeatureOperationBodyMember>,
    operands: Vec<FeatureOperationBodyOperand>,
    booleans: Vec<FeatureBooleanOperation>,
    pub(crate) statuses: Vec<SegmentBodyLineageStatus>,
}

/// Extract the bounded feature-history inputs used by terminal body lineage.
pub(crate) fn extract_segment_lineage(
    ctx: &DecodeContext<'_>,
    container: &Container,
    streams: &[Stream],
) -> Result<SegmentLineage, CodecError> {
    let bindings = segment_body_bindings(ctx, container, streams)?;
    let labels = feature_operation_labels(ctx, container)?;
    let references = feature_body_references(ctx, container)?;
    let blocks = data_blocks(ctx, container)?;
    let inputs = feature_input_blocks(ctx, container)?;
    let body_data_block_uses = feature_body_data_block_uses(ctx, &references, &inputs, &blocks)?;
    let body_reference_occurrences = feature_body_reference_occurrences(ctx, container)?;
    let members = feature_operation_body_members(ctx, container)?;
    let operands = feature_operation_body_operands(
        ctx,
        &members,
        &body_reference_occurrences,
        &inputs,
        &blocks,
        &bindings,
    )?;
    let booleans = feature_boolean_operations(ctx, container)?;
    let statuses = segment_body_lineage_statuses(
        ctx,
        &crate::native::segments::BodyLineageInputs {
            labels: &labels,
            references: &references,
            data_block_uses: &body_data_block_uses,
            data_blocks: &blocks,
            booleans: &booleans,
            operands: &operands,
            bindings: &bindings,
            inputs: &inputs,
        },
    )?
    .unwrap_or_default();
    Ok(SegmentLineage {
        bindings,
        labels,
        references,
        data_blocks: blocks,
        inputs,
        body_data_block_uses,
        body_reference_occurrences,
        members,
        operands,
        booleans,
        statuses,
    })
}

/// Select emitted body images whose complete segment binding has a terminal
/// status. The mapping must cover every emitted body image before selection is
/// admitted; a partial mapping is not a body-selection proof.
pub(crate) fn terminal_feature_body_ids(
    ctx: &DecodeContext<'_>,
    emitted: &BTreeSet<BodyId>,
    bindings: &[SegmentBodyBinding],
    statuses: &[SegmentBodyLineageStatus],
) -> Result<Option<BTreeSet<BodyId>>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "nx terminal body workspace")?;
    let mut statuses_by_binding = BTreeMap::new();
    for status in statuses {
        if statuses_by_binding.contains_key(status.segment_body_binding.as_str()) {
            return Ok(None);
        }
        storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut statuses_by_binding,
                status.segment_body_binding.as_str(),
                status,
                "nx terminal body status index",
            )
        })?;
    }
    let mut mapped = BTreeSet::new();
    let mut selected = BTreeSet::new();
    for binding in bindings {
        let Some(status) = statuses_by_binding.remove(binding.id.as_str()) else {
            return Ok(None);
        };
        let mut ordinal = binding.stream_ordinal;
        let mut digits = 1_u64;
        while ordinal >= 10 {
            ordinal /= 10;
            digits += 1;
        }
        let prefix_len = 5_u64 + digits;
        let mut prefix_reservation = ctx.reserve_scoped(0, "nx terminal body prefix")?;
        let mut prefix = String::new();
        prefix_reservation.with_storage(|| {
            ctx.try_reserve_retained_text(
                &mut prefix,
                cadmpeg_core::decode::index_from_u64(prefix_len).ok_or_else(|| {
                    ctx.refuse_codec_limit("nx terminal body prefix", 0, prefix_len)
                })?,
                "nx terminal body prefix",
            )
        })?;
        write!(&mut prefix, "nx:s{}:", binding.stream_ordinal)
            .map_err(|_| ctx.refuse_codec_limit("nx terminal body prefix", 0, prefix_len))?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(emitted.len()),
            "nx terminal body scan",
        )?;
        for body in emitted
            .iter()
            .filter(|body| body.as_str().starts_with(&prefix))
        {
            if !mapped.contains(body) {
                storage.with_storage(|| {
                    ctx.insert_btree_set(&mut mapped, body, "nx mapped terminal body")
                })?;
            }
            if status.terminal && !selected.contains(body) {
                let body_id =
                    body.try_clone_for_decode(ctx, "nx selected terminal body identity")?;
                ctx.insert_btree_set(&mut selected, body_id, "nx selected terminal body")?;
            }
        }
    }
    Ok(
        (statuses_by_binding.is_empty() && mapped.len() == emitted.len() && !selected.is_empty())
            .then_some(selected),
    )
}

impl NativeModel {
    pub(crate) fn has_untransferred_parasolid_attribute_fields(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<bool, CodecError> {
        parasolid_topology_attribute_fields_have_untransferred_values(
            ctx,
            &self.parasolid.attribute_definitions,
            &self.parasolid.entity_51_records,
            &self.parasolid.attribute_field_uses,
            &self.parasolid.topology_attribute_class_uses,
        )
    }

    /// Runs the extraction dependency DAG. Ordering is load-bearing: later
    /// families depend on earlier ones, and some record ids embed position.
    pub(crate) fn extract(
        ctx: &DecodeContext<'_>,
        root: View<'_>,
        container: &Container,
        streams: &[Stream],
        parsed: &mut ParsedStreams,
        precomputed_lineage: Option<SegmentLineage>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let SegmentLineage {
            bindings: segment_body_bindings,
            labels: feature_operation_labels,
            references: feature_body_references,
            data_blocks,
            inputs: feature_input_blocks,
            body_data_block_uses: feature_body_data_block_uses,
            body_reference_occurrences: feature_body_reference_occurrences,
            members: feature_operation_body_members,
            operands: feature_operation_body_operands,
            booleans: feature_boolean_operations,
            statuses: segment_body_lineage_statuses,
        } = match precomputed_lineage {
            Some(lineage) => lineage,
            None => extract_segment_lineage(ctx, container, streams)?,
        };
        let data_block_object_frames = data_block_object_frames(ctx, container)?;
        let segment_index_rows = segment_index_rows(ctx, container)?;
        let segment_om_links = segment_om_links(ctx, container)?;
        let segment_stream_links = segment_stream_links(ctx, container, streams)?;
        let mut linked_deltas = BTreeSet::new();
        for link in segment_stream_links
            .iter()
            .filter(|link| link.stream_kind == crate::parasolid::StreamKind::Deltas)
        {
            let ordinal = usize::try_from(link.stream_ordinal)
                .map_err(|_| ctx.refuse_codec_limit("nx linked delta ordinal", 0, u64::MAX))?;
            if !linked_deltas.contains(&ordinal) {
                ctx.insert_btree_set(&mut linked_deltas, ordinal, "nx linked delta index")?;
            }
        }
        let delta_pairs = pair_stream_indices(
            ctx,
            streams,
            (!segment_stream_links.is_empty()).then_some(&linked_deltas),
        )?;
        let deltas_events =
            parasolid_deltas_events_with_censuses(ctx, streams, parsed.take_delta_censuses(ctx)?)?;
        let parasolid_group_records =
            parasolid_group_records(ctx, streams, &delta_pairs, &deltas_events.records)?;
        let parasolid_group_members = parasolid_group_members(ctx, streams, &delta_pairs, parsed)?;
        let parasolid_blend_surface_records = parasolid_blend_surface_records(ctx, parsed)?;
        let parasolid_blend_bound_records = parasolid_blend_bound_records(ctx, streams)?;
        let parasolid_offset_surface_records = parasolid_offset_surface_records(ctx, parsed)?;
        let parasolid_trimmed_curve_records = parasolid_trimmed_curve_records(ctx, parsed)?;
        let parasolid_surface_curve_records = parasolid_surface_curve_records(ctx, parsed)?;
        let parasolid_intersection_records = parasolid_intersection_records(ctx, parsed)?;
        let parasolid_term_use_records = parasolid_term_use_records(ctx, streams)?;
        let parasolid_support_uv_records = parasolid_support_uv_records(ctx, streams)?;
        let parasolid_chart_records = parasolid_chart_records(ctx, streams)?;
        let parasolid_attribute_definitions = parasolid_attribute_definitions(ctx, streams)?;
        let parasolid_entity_51_records = parasolid_entity_51_records(ctx, streams)?;
        let value_records = parasolid_entity_value_records(ctx, streams, &deltas_events.records)?;
        let parasolid_entity_52_integer_records = value_records.integers;
        let parasolid_entity_53_double_records = value_records.doubles;
        let parasolid_entity_54_string_records = value_records.strings;
        let parasolid_entity_vector_records = value_records.vectors;
        let parasolid_entity_57_axis_records = value_records.axes;
        let parasolid_entity_58_tag_records = value_records.tags;
        let parasolid_entity_62_unicode_records = value_records.unicode;
        let parasolid_field_names_records = parasolid_field_names_records(ctx, streams)?;
        let parasolid_attribute_field_names = parasolid_attribute_field_names(
            ctx,
            &parasolid_attribute_definitions,
            &parasolid_field_names_records,
            &parasolid_entity_54_string_records,
            &parasolid_entity_62_unicode_records,
        )?;
        let parasolid_entity_51_numeric_uses = parasolid_entity_51_numeric_uses(
            ctx,
            &parasolid_entity_51_records,
            &parasolid_entity_52_integer_records,
            &parasolid_entity_53_double_records,
        )?;
        let parasolid_entity_51_string_uses = parasolid_entity_51_string_uses(
            ctx,
            &parasolid_entity_51_records,
            &parasolid_entity_54_string_records,
        )?;
        let parasolid_entity_51_structured_uses = parasolid_entity_51_structured_uses(
            ctx,
            &parasolid_entity_51_records,
            &parasolid_entity_vector_records,
            &parasolid_entity_57_axis_records,
            &parasolid_entity_58_tag_records,
            &parasolid_entity_62_unicode_records,
        )?;
        let parasolid_attribute_class_uses = parasolid_attribute_class_uses(
            ctx,
            &parasolid_entity_51_records,
            &parasolid_attribute_definitions,
        )?;
        let parasolid_attribute_field_uses = parasolid_attribute_field_uses(
            ctx,
            &parasolid_attribute_class_uses,
            &parasolid_attribute_definitions,
            &parasolid_entity_51_numeric_uses,
            &parasolid_entity_51_string_uses,
            &parasolid_entity_51_structured_uses,
        )?;
        let parasolid_topology_attribute_list_references =
            parasolid_topology_attribute_list_references(
                ctx,
                parsed,
                &parasolid_entity_51_records,
            )?;
        let parasolid_topology_attribute_class_uses = parasolid_topology_attribute_class_uses(
            ctx,
            &parasolid_topology_attribute_list_references,
            &parasolid_entity_51_records,
            &parasolid_attribute_class_uses,
        )?;
        let om_record_areas = om_record_areas(ctx, container)?;
        let audit_trail_rows = audit_trail_rows(ctx, container)?;
        let operation_state_journal_groups = operation_state_journal_groups(ctx, container)?;
        let operation_state_counters = operation_state_counters(ctx, container)?;
        let operation_state_groups = operation_state_groups(ctx, container)?;
        let operation_state_messages = operation_state_messages(ctx, container)?;
        let operation_state_statuses = operation_state_statuses(ctx, container)?;
        let operation_state_slot_lanes = operation_state_slot_lanes(ctx, container)?;
        let feature_operation_records = feature_operation_records(ctx, container)?;
        let feature_unlabeled_operation_records =
            feature_unlabeled_operation_records(ctx, container)?;
        let feature_unlabeled_operation_body_writes =
            feature_unlabeled_operation_body_writes(ctx, container)?;
        let feature_operation_body_writes = feature_operation_body_writes(ctx, container)?;
        let feature_operation_body_image_segment_uses = feature_operation_body_image_segment_uses(
            ctx,
            &feature_operation_body_writes,
            &segment_body_bindings,
        )?;
        let feature_operation_body_identity_segment_uses =
            feature_operation_body_identity_segment_uses(
                ctx,
                &feature_operation_body_writes,
                &segment_body_bindings,
            )?;
        let feature_operation_body_partition_uses = feature_operation_body_partition_uses(
            ctx,
            &feature_operation_body_writes,
            &feature_operation_body_image_segment_uses,
            &segment_body_bindings,
            streams,
            &parasolid_group_records,
            &parasolid_group_members,
        )?;
        let feature_body_write_group_partition_uses = feature_body_write_group_partition_uses(
            ctx,
            &feature_operation_body_writes,
            &feature_unlabeled_operation_body_writes,
            &parasolid_group_records,
            &parasolid_group_members,
        )?;
        let feature_operation_tagged_references = feature_operation_object_references(
            ctx,
            container,
            crate::om::direct_reference::ReferenceFieldKind::Tagged17,
        )?;
        let feature_operation_data_block_references = feature_operation_object_references(
            ctx,
            container,
            crate::om::direct_reference::ReferenceFieldKind::DataBlock03,
        )?;
        let feature_operation_common_frames = feature_operation_common_frames(ctx, container)?;
        let feature_operation_terminal_discriminators =
            feature_operation_terminal_discriminators(ctx, container)?;
        let feature_operation_terminal_frames =
            feature_operation_terminal_frames(ctx, container, &feature_operation_common_frames)?;
        let feature_operation_state_journal_uses = feature_operation_state_journal_uses(
            ctx,
            &feature_operation_labels,
            &feature_operation_records,
            &feature_operation_terminal_frames,
            &operation_state_journal_groups,
        )?;
        let feature_payload_strings = feature_payload_strings(ctx, container)?;
        let feature_symbolic_threads = feature_symbolic_threads(ctx, container)?;
        let feature_threaded_hole_templates = feature_threaded_hole_templates(
            ctx,
            &feature_operation_labels,
            &feature_operation_records,
            &feature_payload_strings,
        )?;
        let feature_simple_hole_templates = feature_simple_hole_templates(
            ctx,
            &feature_operation_labels,
            &feature_operation_records,
            &feature_payload_strings,
        )?;
        let feature_simple_hole_repeated_scalar_lanes =
            feature_simple_hole_repeated_scalar_lanes(ctx, container)?;
        let feature_simple_hole_repeated_scalar_lane_block_references =
            feature_simple_hole_repeated_scalar_lane_block_references(ctx, container)?;
        let feature_simple_hole_construction_groups = feature_simple_hole_construction_groups(
            ctx,
            &feature_operation_labels,
            &feature_simple_hole_repeated_scalar_lanes,
            &feature_simple_hole_repeated_scalar_lane_block_references,
        )?;
        let feature_hole_package_construction_group_lanes =
            feature_hole_package_construction_group_lanes(ctx, container)?;
        let feature_hole_package_construction_group_uses =
            feature_hole_package_construction_group_uses(
                ctx,
                &feature_hole_package_construction_group_lanes,
                &feature_simple_hole_construction_groups,
            )?;
        let feature_body_segment_uses = feature_body_segment_uses(
            ctx,
            &feature_body_references,
            &feature_body_data_block_uses,
            &feature_input_blocks,
            &data_blocks,
            &segment_body_bindings,
            &data_block_object_frames,
        )?;
        let feature_input_block_identity_groups =
            feature_input_block_identity_groups(ctx, &feature_input_blocks)?;
        let display_jt_indices = display_jt_indices(ctx, container)?;
        let display_jt_documents = display_jt_documents(ctx, container, &display_jt_indices)?;
        let budget = (ctx, root);
        let display_jt_segments = display_jt_segments(budget, container, &display_jt_documents)?;
        let display_jt_shape_lod_elements =
            display_jt_shape_lod_elements(budget, container, &display_jt_segments)?;
        let display_jt_tri_strip_lod_headers =
            display_jt_tri_strip_lod_headers(ctx, container, &display_jt_shape_lod_elements)?;
        let display_jt_initial_face_degree_symbols =
            display_jt_initial_face_degree_symbols(ctx, container, &display_jt_shape_lod_elements)?;
        let DisplayJtTopologyArrays {
            sequences: display_jt_topology_packet_sequences,
            vertex_headers: display_jt_vertex_records_headers,
            coordinate_headers: display_jt_coordinate_array_headers,
        } = display_jt_topology_packet_sequences(ctx, container, &display_jt_shape_lod_elements)?;
        let display_jt_vertex_coordinates =
            display_jt_vertex_coordinates(ctx, container, &display_jt_coordinate_array_headers)?;
        let display_jt_vertex_normals = display_jt_vertex_normals(
            ctx,
            container,
            &display_jt_vertex_records_headers,
            &display_jt_coordinate_array_headers,
            &display_jt_vertex_coordinates,
        )?;
        let display_jt_vertex_colors = display_jt_vertex_colors(
            ctx,
            container,
            &display_jt_vertex_records_headers,
            &display_jt_coordinate_array_headers,
            &display_jt_vertex_coordinates,
            &display_jt_vertex_normals,
        )?;
        let display_jt_vertex_texture_coordinates = display_jt_vertex_texture_coordinates(
            ctx,
            container,
            &display_jt_vertex_records_headers,
            &display_jt_coordinate_array_headers,
            &display_jt_vertex_coordinates,
            &display_jt_vertex_normals,
            &display_jt_vertex_colors,
        )?;
        let display_jt_vertex_flags = display_jt_vertex_flags(
            ctx,
            DisplayJtVertexFlagInputs {
                container,
                vertex_headers: &display_jt_vertex_records_headers,
                coordinate_headers: &display_jt_coordinate_array_headers,
                coordinates: &display_jt_vertex_coordinates,
                normals: &display_jt_vertex_normals,
                colors: &display_jt_vertex_colors,
                texture_coordinates: &display_jt_vertex_texture_coordinates,
            },
        )?;
        let display_jt_polygon_meshes = display_jt_polygon_meshes(
            ctx,
            &display_jt_topology_packet_sequences,
            &display_jt_coordinate_array_headers,
        )?;
        let (display_jt_compressed_elements, display_jt_compressed_element_sequences) =
            display_jt_compressed_element_sequences(budget, container, &display_jt_segments)?;
        let display_jt_string_property_atoms =
            display_jt_string_property_atoms(budget, container, &display_jt_segments)?;
        let display_jt_shape_lod_bindings =
            display_jt_shape_lod_bindings(budget, container, &display_jt_segments)?;
        let display_jt_base_node_data = display_jt_base_node_data(
            budget,
            container,
            &display_jt_segments,
            &display_jt_documents,
        )?;
        let display_jt_group_node_data = display_jt_group_node_data(
            budget,
            container,
            &display_jt_segments,
            &display_jt_documents,
        )?;
        let display_jt_instance_nodes = display_jt_instance_nodes(
            budget,
            container,
            &display_jt_segments,
            &display_jt_documents,
        )?;
        let display_jt_geometric_transform_attributes = display_jt_geometric_transform_attributes(
            budget,
            container,
            &display_jt_segments,
            &display_jt_documents,
        )?;
        let display_jt_material_attributes = display_jt_material_attributes(
            budget,
            container,
            &display_jt_segments,
            &display_jt_documents,
        )?;
        let display_jt_partition_nodes = display_jt_partition_nodes(
            budget,
            container,
            &display_jt_segments,
            &display_jt_documents,
        )?;
        let display_jt_range_lod_nodes = display_jt_range_lod_nodes(
            budget,
            container,
            &display_jt_segments,
            &display_jt_documents,
        )?;
        let display_jt_tri_strip_shape_nodes = display_jt_tri_strip_shape_nodes(
            budget,
            container,
            &display_jt_segments,
            &display_jt_documents,
        )?;
        let feature_datum_csys_constructions = feature_datum_csys_constructions(ctx, container)?;
        let feature_datum_csys_payloads =
            feature_datum_csys_payloads(ctx, container, &feature_datum_csys_constructions)?;
        let feature_datum_csys_payload_scalar_pairs =
            feature_datum_csys_payload_scalar_pairs(ctx, container, &feature_datum_csys_payloads)?;
        let feature_datum_csys_payload_fixed_pairs =
            feature_datum_csys_payload_fixed_pairs(ctx, container, &feature_datum_csys_payloads)?;
        let feature_datum_csys_payload_scalars =
            feature_datum_csys_payload_scalars(ctx, container, &feature_datum_csys_payloads)?;
        let feature_datum_csys_descriptors =
            feature_datum_csys_descriptors(ctx, container, &feature_datum_csys_constructions)?;
        let feature_datum_plane_headers = feature_datum_plane_headers(ctx, container)?;
        let feature_datum_plane_block_uses = feature_datum_plane_block_uses(
            ctx,
            &feature_datum_plane_headers,
            &feature_input_blocks,
        )?;
        let feature_datum_plane_payloads =
            feature_datum_plane_payloads(ctx, container, &feature_datum_plane_headers)?;
        let feature_datum_plane_payload_scalar_pairs = feature_datum_plane_payload_scalar_pairs(
            ctx,
            container,
            &feature_datum_plane_payloads,
        )?;
        let feature_datum_plane_descriptors =
            feature_datum_plane_descriptors(ctx, container, &feature_datum_plane_headers)?;
        let feature_datum_plane_csys_identity_uses = feature_datum_plane_csys_identity_uses(
            ctx,
            &feature_datum_plane_descriptors,
            &feature_datum_csys_descriptors,
        )?;
        let feature_datum_csys_block_uses = feature_datum_csys_block_uses(
            ctx,
            &feature_datum_csys_constructions,
            &feature_input_blocks,
        )?;
        let feature_sketch_references = feature_sketch_references(ctx, container)?;
        let feature_projected_curve_references =
            feature_projected_curve_references(ctx, container)?;
        let feature_projected_curve_construction_payloads =
            feature_projected_curve_construction_payloads(
                ctx,
                container,
                &feature_operation_labels,
                &feature_projected_curve_references,
            )?;
        let feature_projected_curve_construction_strings =
            feature_projected_curve_construction_strings(
                ctx,
                container,
                &feature_projected_curve_construction_payloads,
            )?;
        let feature_fset_reference_graphs = feature_fset_reference_graphs(ctx, container)?;
        let feature_fset_construction_payloads =
            feature_fset_construction_payloads(ctx, container, &feature_fset_reference_graphs)?;
        let feature_delete_reference_fields = feature_delete_reference_fields(ctx, container)?;
        let feature_delete_construction_payloads =
            feature_delete_construction_payloads(ctx, container, &feature_delete_reference_fields)?;
        let feature_pattern_references = feature_pattern_references(ctx, container)?;
        let feature_pattern_counted_reference_lanes =
            feature_pattern_counted_reference_lanes(ctx, container)?;
        let feature_pattern_construction_payloads = feature_pattern_construction_payloads(
            ctx,
            container,
            &feature_operation_labels,
            &feature_pattern_references,
        )?;
        let feature_pattern_construction_strings = feature_pattern_construction_strings(
            ctx,
            container,
            &feature_pattern_construction_payloads,
        )?;
        let feature_pattern_construction_fixed_lanes = feature_pattern_construction_fixed_lanes(
            ctx,
            container,
            &feature_pattern_construction_payloads,
        )?;
        let feature_pattern_transform_lanes = feature_pattern_transform_lanes(ctx, container)?;
        let feature_multi_instance_output_lanes =
            feature_multi_instance_output_lanes(ctx, container)?;
        let feature_identical_instance_output_lanes =
            feature_identical_instance_output_lanes(ctx, container)?;
        let feature_point_construction_headers =
            feature_point_construction_headers(ctx, container)?;
        let feature_point_construction_scalar_lanes = feature_point_construction_scalar_lanes(
            ctx,
            container,
            &feature_point_construction_headers,
        )?;
        let feature_draft_construction_references =
            feature_draft_construction_references(ctx, container)?;
        let feature_draft_construction_index_lanes =
            feature_draft_construction_index_lanes(ctx, container)?;
        let feature_draft_construction_payloads = feature_draft_construction_payloads(
            ctx,
            container,
            &feature_draft_construction_index_lanes,
        )?;
        let feature_draft_construction_graph_payloads = feature_draft_construction_graph_payloads(
            ctx,
            container,
            &feature_draft_construction_index_lanes,
            &feature_draft_construction_references,
        )?;
        let feature_draft_construction_fixed_lanes = feature_draft_construction_fixed_lanes(
            ctx,
            container,
            &feature_draft_construction_graph_payloads,
        )?;
        let feature_draft_construction_binary32_lanes = feature_draft_construction_binary32_lanes(
            ctx,
            container,
            &feature_draft_construction_graph_payloads,
        )?;
        let feature_draft_construction_graph_strings = feature_draft_construction_graph_strings(
            ctx,
            container,
            &feature_draft_construction_graph_payloads,
        )?;
        let feature_draft_construction_identity_frames =
            feature_draft_construction_identity_frames(
                ctx,
                container,
                &feature_draft_construction_payloads,
            )?;
        let feature_draft_construction_terminal_lanes =
            feature_draft_construction_terminal_lanes(ctx, container)?;
        let feature_surface_construction_references =
            feature_surface_construction_references(ctx, container)?;
        let feature_surface_construction_payloads = feature_surface_construction_payloads(
            ctx,
            container,
            &feature_surface_construction_references,
        )?;
        let feature_surface_construction_scalar_pairs = feature_surface_construction_scalar_pairs(
            ctx,
            container,
            &feature_surface_construction_payloads,
        )?;
        let feature_surface_construction_strings = feature_surface_construction_strings(
            ctx,
            container,
            &feature_surface_construction_payloads,
        )?;
        let feature_surface_construction_branches =
            feature_surface_construction_branches(ctx, container)?;
        let feature_swp104_leading_branches = feature_swp104_leading_branches(ctx, container)?;
        let feature_thru_curve_construction_branch_groups =
            feature_thru_curve_construction_branch_groups(ctx, container)?;
        let feature_thru_curve_construction_envelopes =
            feature_thru_curve_construction_envelopes(ctx, container)?;
        let feature_extrude_profile_references =
            feature_extrude_profile_references(ctx, container)?;
        let feature_extrude_payload_headers = feature_extrude_payload_headers(ctx, container)?;
        let feature_operation_body_scalar_triples =
            feature_operation_body_scalar_triples(ctx, container)?;
        let feature_operation_body_11_continuations =
            feature_operation_body_11_continuations(ctx, container)?;
        let feature_operation_body_reference_lanes =
            feature_operation_body_reference_lanes(ctx, container)?;
        let feature_extrude_construction_profiles =
            feature_extrude_construction_profiles(ctx, &feature_extrude_profile_references)?;
        let feature_extrude_payload_32_branches =
            feature_extrude_payload_32_branches(ctx, container)?;
        let feature_extrude_32_constructions = feature_extrude_32_constructions(
            ctx,
            &feature_extrude_profile_references,
            &feature_extrude_payload_32_branches,
        )?;
        let feature_block_construction_references =
            feature_block_construction_references(ctx, container)?;
        let feature_block_constructions =
            feature_block_constructions(ctx, &feature_block_construction_references)?;
        let feature_block_construction_payloads =
            feature_block_construction_payloads(ctx, container, &feature_block_constructions)?;
        let feature_block_payload_scalars =
            feature_block_payload_scalars(ctx, container, &feature_block_construction_payloads)?;
        let feature_block_payload_names =
            feature_block_payload_names(ctx, container, &feature_block_construction_payloads)?;
        let feature_block_payload_named_records = feature_block_payload_named_records(
            ctx,
            &feature_block_construction_payloads,
            &feature_block_payload_names,
            &feature_block_payload_scalars,
        )?;
        let feature_block_payload_points = feature_block_payload_points(
            ctx,
            &feature_block_payload_named_records,
            &feature_block_payload_names,
            &feature_block_payload_scalars,
        )?;
        let feature_block_payload_point_groups =
            feature_block_payload_point_groups(ctx, &feature_block_payload_points)?;
        let feature_sketch_records = feature_sketch_records(
            ctx,
            &feature_operation_labels,
            &feature_operation_records,
            &feature_input_blocks,
            &feature_sketch_references,
        )?;
        let feature_sketch_construction_inputs = feature_sketch_construction_inputs(
            ctx,
            &feature_sketch_records,
            &feature_sketch_references,
        )?;
        let feature_sketch_construction_payloads = feature_sketch_construction_payloads(
            ctx,
            container,
            &feature_sketch_construction_inputs,
        )?;
        let feature_sketch_payload_coordinate_pairs = feature_sketch_payload_coordinate_pairs(
            ctx,
            container,
            &feature_sketch_construction_payloads,
        )?;
        let feature_sketch_payload_fixed_pairs = feature_sketch_payload_fixed_pairs(
            ctx,
            container,
            &feature_sketch_construction_payloads,
        )?;
        let feature_sketch_payload_mixed_pairs = feature_sketch_payload_mixed_pairs(
            ctx,
            container,
            &feature_sketch_construction_payloads,
        )?;
        let feature_sketch_payload_scalars =
            feature_sketch_payload_scalars(ctx, container, &feature_sketch_construction_inputs)?;
        let feature_sketch_payload_scalar_lanes = feature_sketch_payload_scalar_lanes(
            ctx,
            container,
            &feature_sketch_construction_payloads,
        )?;
        let feature_sketch_payload_names =
            feature_sketch_payload_names(ctx, container, &feature_sketch_construction_inputs)?;
        let feature_sketch_payload_named_records = feature_sketch_payload_named_records(
            ctx,
            &feature_sketch_construction_payloads,
            &feature_sketch_payload_names,
            &feature_sketch_payload_scalars,
            &feature_sketch_payload_fixed_pairs,
            &feature_sketch_payload_mixed_pairs,
        )?;
        let feature_sketch_fixed_points = feature_sketch_fixed_points(
            ctx,
            &feature_sketch_payload_named_records,
            &feature_sketch_payload_names,
            &feature_sketch_payload_fixed_pairs,
        )?;
        let feature_sketch_points = feature_sketch_points(
            ctx,
            &feature_sketch_payload_named_records,
            &feature_sketch_payload_names,
            &feature_sketch_payload_scalars,
        )?;
        let feature_sketch_point_groups = feature_sketch_point_groups(ctx, &feature_sketch_points)?;
        let offset_store_named_points = offset_store_named_points(ctx, container)?;
        let feature_sketch_named_point_block_uses = feature_sketch_named_point_block_uses(
            ctx,
            &feature_sketch_references,
            &offset_store_named_points,
        )?;
        let feature_sketch_preceding_named_point_uses = feature_sketch_preceding_named_point_uses(
            ctx,
            &feature_sketch_references,
            &offset_store_named_points,
        )?;
        let feature_sketch_point_uses = feature_sketch_point_uses(
            ctx,
            &feature_sketch_point_groups,
            &offset_store_named_points,
            &feature_sketch_named_point_block_uses,
        )?;
        let feature_sketch_datum_csys_dependencies = feature_sketch_datum_csys_dependencies(
            ctx,
            &feature_operation_labels,
            &offset_store_named_points,
            &feature_sketch_point_uses,
            &feature_datum_csys_constructions,
            &feature_datum_csys_payload_scalars,
        )?;
        let expression_declarations = expression_declarations(ctx, container)?;
        let expressions = expressions(ctx, container, &expression_declarations)?;
        let classes = class_definitions(ctx, container)?;
        let fields = field_definitions(ctx, container)?;
        let object_records = object_records(ctx, container)?;
        let (rmfastload_object_id_tables, rmfastload_object_ids) =
            match rmfastload_object_id_table(ctx, container)? {
                Some((table, object_ids)) => {
                    let mut tables = ctx.collection_vec(1, "nx RMFastLoad object ID tables")?;
                    tables.push(table);
                    (tables, object_ids)
                }
                None => (Vec::new(), Vec::new()),
            };
        let data_block_control_forms = data_block_control_forms(ctx, container)?;
        let data_block_control_values = data_block_control_values(ctx, container)?;
        let data_block_control_class_references =
            data_block_control_class_references(ctx, container)?;
        let data_block_control_index_values = data_block_control_index_values(ctx, container)?;
        let data_block_control_references = data_block_control_references(ctx, container)?;
        let data_block_control_handle_pairs =
            data_block_control_handle_pairs(ctx, &data_block_control_references)?;
        let data_block_references =
            data_block_references(ctx, container, &object_records, &expression_declarations)?;
        let data_block_counted_index_lanes = data_block_counted_index_lanes(ctx, container)?;
        let data_block_abr_reference_lanes = data_block_abr_reference_lanes(ctx, container)?;
        let data_block_index_rows = data_block_index_rows(ctx, container)?;
        let data_block_linked_index_rows = data_block_linked_index_rows(ctx, container)?;
        let data_block_target_index_rows = data_block_target_index_rows(ctx, container)?;
        let rm_creation_display_data_relations =
            rm_creation_display_data_relations(ctx, container, &rmfastload_object_ids)?;
        let (part_color_tables, part_color_definitions) = part_color_tables(ctx, container)?;
        let rm_display_color_assignments = rm_display_color_assignments(
            ctx,
            container,
            &part_color_definitions,
            &rmfastload_object_ids,
        )?;
        let data_block_column_index_tables = data_block_column_index_tables(
            ctx,
            &data_block_linked_index_rows,
            &data_block_target_index_rows,
        )?;
        let feature_datum_csys_column_row_uses = feature_datum_csys_column_row_uses(
            ctx,
            &feature_datum_csys_constructions,
            &data_block_index_rows,
            &data_block_linked_index_rows,
            &data_block_target_index_rows,
            &data_block_column_index_tables,
        )?;
        let feature_input_column_row_uses = feature_input_column_row_uses(
            ctx,
            &feature_input_blocks,
            &data_block_index_rows,
            &data_block_linked_index_rows,
            &data_block_target_index_rows,
            &data_block_column_index_tables,
        )?;
        let feature_input_column_targets = feature_input_column_targets(
            ctx,
            &feature_input_blocks,
            &feature_input_column_row_uses,
            &data_block_linked_index_rows,
            &data_block_target_index_rows,
        )?;
        let feature_parameter_bindings = feature_parameter_bindings(
            ctx,
            &feature_input_blocks,
            &data_block_references,
            &expressions,
        )?;
        let feature_parameter_uses = feature_parameter_uses(ctx, &feature_parameter_bindings)?;
        let feature_block_dimensions = feature_block_dimensions(
            ctx,
            &feature_block_constructions,
            &feature_parameter_bindings,
            &expression_declarations,
            &expressions,
        )?;
        let store_headers = store_headers(ctx, container)?;
        let string_values = string_values(ctx, container)?;
        let object_uuid_values = object_uuid_values(ctx, container)?;
        let object_references = object_references(ctx, container)?;
        let object_record_handle_pairs = object_record_handle_pairs(ctx, &object_references)?;
        let configurations = configurations(ctx, container)?;
        let part_attributes = part_attributes(ctx, container)?;
        let configuration_attribute_uses =
            configuration_attribute_uses(ctx, &configurations, &part_attributes)?;
        let external_references = external_references(ctx, container)?;
        let external_reference_records = external_reference_records(ctx, container)?;
        let external_reference_indexed_records =
            external_reference_indexed_records(ctx, container, &external_reference_records)?;
        let external_reference_empty_records =
            external_reference_empty_records(ctx, container, &external_reference_indexed_records)?;
        let external_reference_tail_reference_pairs =
            external_reference_tail_reference_pairs(ctx, container, &external_reference_records)?;
        let external_reference_record_string_uses = external_reference_record_string_uses(
            ctx,
            &external_reference_records,
            &external_references,
        )?;
        let external_reference_record_children = external_reference_record_children(
            ctx,
            &external_reference_records,
            &external_references,
            &external_reference_record_string_uses,
        )?;
        let material_texture_assets = material_texture_assets(ctx, container)?;
        let material_texture_catalog_entries =
            material_texture_catalog_entries(ctx, container, &material_texture_assets)?;
        let persistent_handles = persistent_handles(
            ctx,
            &object_references,
            &data_block_control_references,
            &external_reference_records,
            &external_reference_tail_reference_pairs,
        )?;
        let (
            fast_load_component_prototypes,
            fast_load_component_uuids,
            fast_load_component_occurrences,
        ) = fast_load_component_roster(ctx, container)?;
        let fast_load_component_object_groups = fast_load_component_object_groups(
            ctx,
            &fast_load_component_uuids,
            fast_load_component_occurrences.as_slice(),
            &object_uuid_values,
        )?;
        let (saved_toggle_streams, saved_toggle_entries) = saved_toggle_records(ctx, container)?;
        Ok(NativeModel {
            display_jt: DisplayJtRecords {
                graph: DisplayJtGraph::from_wire_with_context(
                    ctx,
                    DisplayJtGraphWire {
                        documents: display_jt_documents,
                        segments: display_jt_segments,
                        shape_lod_elements: display_jt_shape_lod_elements,
                        compressed_elements: display_jt_compressed_elements,
                        compressed_element_sequences: display_jt_compressed_element_sequences,
                    },
                )?,
                indices: display_jt_indices,
                tri_strip_lod_headers: display_jt_tri_strip_lod_headers,
                initial_face_degree_symbols: display_jt_initial_face_degree_symbols,
                topology_packet_sequences: display_jt_topology_packet_sequences,
                vertex_records_headers: display_jt_vertex_records_headers,
                coordinate_array_headers: display_jt_coordinate_array_headers,
                vertex_coordinates: display_jt_vertex_coordinates,
                vertex_normals: display_jt_vertex_normals,
                vertex_colors: display_jt_vertex_colors,
                vertex_texture_coordinates: display_jt_vertex_texture_coordinates,
                vertex_flags: display_jt_vertex_flags,
                polygon_meshes: display_jt_polygon_meshes,
                string_property_atoms: display_jt_string_property_atoms,
                shape_lod_bindings: display_jt_shape_lod_bindings,
                base_node_data: display_jt_base_node_data,
                group_node_data: display_jt_group_node_data,
                instance_nodes: display_jt_instance_nodes,
                geometric_transform_attributes: display_jt_geometric_transform_attributes,
                material_attributes: display_jt_material_attributes,
                partition_nodes: display_jt_partition_nodes,
                range_lod_nodes: display_jt_range_lod_nodes,
                tri_strip_shape_nodes: display_jt_tri_strip_shape_nodes,
            },
            parasolid: ParasolidRecords {
                group_records: parasolid_group_records,
                group_members: parasolid_group_members,
                deltas_transmit_headers: deltas_events.transmit_headers,
                deltas_terminal_null_references: deltas_events.terminal_null_references,
                deltas_records: deltas_events.records,
                deltas_tombstones: deltas_events.tombstones,
                deltas_body_revisions: deltas_events.body_revisions,
                deltas_term_use_numeric_tails: deltas_events.term_use_numeric_tails,
                deltas_tagged_reference_lanes: deltas_events.tagged_reference_lanes,
                deltas_reference_type_maps: deltas_events.reference_type_maps,
                deltas_reference_state_packets: deltas_events.reference_state_packets,
                deltas_schema_reference_preambles: deltas_events.schema_reference_preambles,
                deltas_reference_marker_packets: deltas_events.reference_marker_packets,
                deltas_type_150_state_packets: deltas_events.type_150_state_packets,
                deltas_inline_schema_declarations: deltas_events.inline_schema_declarations,
                deltas_inline_body_states: deltas_events.inline_body_states,
                deltas_residual_spans: deltas_events.residual_spans,
                blend_surface_records: parasolid_blend_surface_records,
                blend_bound_records: parasolid_blend_bound_records,
                offset_surface_records: parasolid_offset_surface_records,
                trimmed_curve_records: parasolid_trimmed_curve_records,
                surface_curve_records: parasolid_surface_curve_records,
                intersection_records: parasolid_intersection_records,
                term_use_records: parasolid_term_use_records,
                support_uv_records: parasolid_support_uv_records,
                chart_records: parasolid_chart_records,
                attribute_definitions: parasolid_attribute_definitions,
                field_names_records: parasolid_field_names_records,
                attribute_field_names: parasolid_attribute_field_names,
                entity_51_records: parasolid_entity_51_records,
                entity_52_integer_records: parasolid_entity_52_integer_records,
                entity_53_double_records: parasolid_entity_53_double_records,
                entity_54_string_records: parasolid_entity_54_string_records,
                entity_vector_records: parasolid_entity_vector_records,
                entity_57_axis_records: parasolid_entity_57_axis_records,
                entity_58_tag_records: parasolid_entity_58_tag_records,
                entity_62_unicode_records: parasolid_entity_62_unicode_records,
                entity_51_numeric_uses: parasolid_entity_51_numeric_uses,
                entity_51_string_uses: parasolid_entity_51_string_uses,
                entity_51_structured_uses: parasolid_entity_51_structured_uses,
                attribute_class_uses: parasolid_attribute_class_uses,
                attribute_field_uses: parasolid_attribute_field_uses,
                topology_attribute_list_references: parasolid_topology_attribute_list_references,
                topology_attribute_class_uses: parasolid_topology_attribute_class_uses,
            },
            segments: SegmentRecords {
                index_rows: segment_index_rows,
                om_links: segment_om_links,
                stream_links: segment_stream_links,
                body_bindings: segment_body_bindings,
                body_lineage_statuses: segment_body_lineage_statuses,
            },
            structure: StructureRecords {
                prototypes: fast_load_component_prototypes,
                uuids: fast_load_component_uuids,
                occurrences: fast_load_component_occurrences,
                object_groups: fast_load_component_object_groups,
            },
            toggle: ToggleRecords {
                streams: saved_toggle_streams,
                entries: saved_toggle_entries,
            },
            features: FeatureRecords {
                feature_operation_labels,
                feature_operation_records,
                feature_unlabeled_operation_records,
                feature_unlabeled_operation_body_writes,
                feature_operation_body_writes,
                feature_operation_body_image_segment_uses,
                feature_operation_body_identity_segment_uses,
                feature_operation_body_partition_uses,
                feature_body_write_group_partition_uses,
                feature_operation_tagged_references,
                feature_operation_data_block_references,
                feature_operation_common_frames,
                feature_operation_terminal_discriminators,
                feature_operation_terminal_frames,
                feature_operation_state_journal_uses,
                feature_payload_strings,
                feature_symbolic_threads,
                feature_threaded_hole_templates,
                feature_simple_hole_templates,
                feature_simple_hole_repeated_scalar_lanes,
                feature_simple_hole_repeated_scalar_lane_block_references,
                feature_simple_hole_construction_groups,
                feature_hole_package_construction_group_lanes,
                feature_hole_package_construction_group_uses,
                feature_body_references,
                feature_body_segment_uses,
                feature_body_data_block_uses,
                feature_body_reference_occurrences,
                feature_input_blocks,
                feature_input_block_identity_groups,
                feature_datum_csys_constructions,
                feature_datum_csys_column_row_uses,
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
                feature_fset_reference_graphs,
                feature_fset_construction_payloads,
                feature_delete_reference_fields,
                feature_delete_construction_payloads,
                feature_pattern_references,
                feature_pattern_counted_reference_lanes,
                feature_pattern_construction_payloads,
                feature_pattern_construction_strings,
                feature_pattern_construction_fixed_lanes,
                feature_pattern_transform_lanes,
                feature_multi_instance_output_lanes,
                feature_identical_instance_output_lanes,
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
                feature_swp104_leading_branches,
                feature_thru_curve_construction_branch_groups,
                feature_thru_curve_construction_envelopes,
                feature_extrude_profile_references,
                feature_extrude_payload_headers,
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
                feature_sketch_payload_mixed_pairs,
                feature_sketch_payload_scalars,
                feature_sketch_payload_scalar_lanes,
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
                audit_trail_rows,
                operation_state_journal_groups,
                operation_state_counters,
                operation_state_groups,
                operation_state_messages,
                operation_state_statuses,
                operation_state_slot_lanes,
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
                rm_creation_display_data_relations,
                part_color_tables,
                part_color_definitions,
                rm_display_color_assignments,
                data_block_column_index_tables,
                store_headers,
                string_values,
                object_uuid_values,
                object_references,
                object_record_handle_pairs,
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
        })
    }

    /// Whether every emptiness-counting catalogue family is empty.
    ///
    /// Visits rows with `counts_toward_emptiness`. Does not cover
    /// `object_sections`; the caller checks that separately.
    pub(super) fn is_empty(&self) -> bool {
        super::catalogue::NATIVE_CATALOGUE.is_empty(self)
    }
}

#[cfg(test)]
mod tests;
