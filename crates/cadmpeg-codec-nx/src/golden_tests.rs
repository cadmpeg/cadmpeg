// SPDX-License-Identifier: Apache-2.0
//! Golden serialized decode/inspect snapshots. NX snapshots are code-built and
//! regenerated with `UPDATE_GOLDEN=1 cargo test-fast golden` (workspace build).
//! See `docs/golden-coverage-floors.toml` for the zlib/flate2 backend note.
#![allow(unused_imports)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::{snapshot_text, Branch, Harness};

use crate::test_support::*;
use crate::NxCodec;

/// Arena names the native catalogue can emit.
const KNOWN_ARENAS: &[&str] = &[
    "class_definitions",
    "configuration_attribute_uses",
    "configurations",
    "data_block_abr_reference_lanes",
    "data_block_column_index_tables",
    "data_block_control_class_references",
    "data_block_control_forms",
    "data_block_control_handle_pairs",
    "data_block_control_index_values",
    "data_block_control_references",
    "data_block_control_values",
    "data_block_counted_index_lanes",
    "data_block_index_rows",
    "data_block_linked_index_rows",
    "data_block_object_frames",
    "data_block_references",
    "data_block_target_index_rows",
    "data_blocks",
    "display_jt_base_node_data",
    "display_jt_compressed_element_sequences",
    "display_jt_compressed_elements",
    "display_jt_coordinate_array_headers",
    "display_jt_documents",
    "display_jt_geometric_transform_attributes",
    "display_jt_material_attributes",
    "display_jt_group_node_data",
    "display_jt_indices",
    "display_jt_initial_face_degree_symbols",
    "display_jt_instance_nodes",
    "display_jt_partition_nodes",
    "display_jt_polygon_meshes",
    "display_jt_range_lod_nodes",
    "display_jt_segments",
    "display_jt_shape_lod_bindings",
    "display_jt_shape_lod_elements",
    "display_jt_string_property_atoms",
    "display_jt_topology_packet_sequences",
    "display_jt_tri_strip_lod_headers",
    "display_jt_tri_strip_shape_nodes",
    "display_jt_vertex_colors",
    "display_jt_vertex_coordinates",
    "display_jt_vertex_flags",
    "display_jt_vertex_normals",
    "display_jt_vertex_records_headers",
    "display_jt_vertex_texture_coordinates",
    "expression_declarations",
    "expressions",
    "external_reference_empty_records",
    "external_reference_indexed_records",
    "external_reference_record_children",
    "external_reference_record_string_uses",
    "external_reference_records",
    "external_reference_tail_reference_pairs",
    "external_references",
    "fast_load_component_occurrences",
    "fast_load_component_object_groups",
    "fast_load_component_prototypes",
    "fast_load_component_uuids",
    "feature_block_construction_payloads",
    "feature_block_construction_references",
    "feature_block_constructions",
    "feature_block_dimensions",
    "feature_block_payload_named_records",
    "feature_block_payload_names",
    "feature_block_payload_point_groups",
    "feature_block_payload_points",
    "feature_block_payload_scalars",
    "feature_body_reference_occurrences",
    "feature_body_references",
    "feature_body_data_block_uses",
    "feature_body_segment_uses",
    "feature_boolean_operations",
    "feature_datum_csys_block_uses",
    "feature_datum_csys_column_row_uses",
    "feature_datum_csys_constructions",
    "feature_datum_csys_descriptors",
    "feature_datum_csys_payload_fixed_pairs",
    "feature_datum_csys_payload_scalar_pairs",
    "feature_datum_csys_payload_scalars",
    "feature_datum_csys_payloads",
    "feature_datum_plane_block_uses",
    "feature_datum_plane_csys_identity_uses",
    "feature_datum_plane_descriptors",
    "feature_datum_plane_headers",
    "feature_datum_plane_payload_scalar_pairs",
    "feature_datum_plane_payloads",
    "feature_draft_construction_binary32_lanes",
    "feature_draft_construction_fixed_lanes",
    "feature_draft_construction_graph_payloads",
    "feature_draft_construction_graph_strings",
    "feature_draft_construction_identity_frames",
    "feature_draft_construction_index_lanes",
    "feature_draft_construction_payloads",
    "feature_draft_construction_references",
    "feature_draft_construction_terminal_lanes",
    "feature_delete_construction_payloads",
    "feature_delete_reference_fields",
    "feature_extrude_32_constructions",
    "feature_extrude_construction_profiles",
    "feature_extrude_payload_32_branches",
    "feature_extrude_payload_headers",
    "feature_extrude_profile_references",
    "feature_fset_construction_payloads",
    "feature_fset_reference_graphs",
    "feature_input_block_identity_groups",
    "feature_input_blocks",
    "feature_input_column_row_uses",
    "feature_input_column_targets",
    "feature_identical_instance_output_lanes",
    "feature_hole_package_construction_group_lanes",
    "feature_hole_package_construction_group_uses",
    "feature_multi_instance_output_lanes",
    "feature_operation_body_11_continuations",
    "feature_operation_body_members",
    "feature_operation_body_operands",
    "feature_operation_body_reference_lanes",
    "feature_operation_body_scalar_triples",
    "feature_operation_data_block_references",
    "feature_operation_labels",
    "feature_operation_object_relations",
    "feature_operation_tagged_references",
    "feature_operation_records",
    "feature_operation_common_frames",
    "feature_operation_terminal_discriminators",
    "feature_operation_terminal_frames",
    "feature_operation_state_journal_uses",
    "feature_parameter_bindings",
    "feature_parameter_uses",
    "feature_pattern_construction_fixed_lanes",
    "feature_pattern_construction_payloads",
    "feature_pattern_construction_strings",
    "feature_pattern_counted_reference_lanes",
    "feature_pattern_references",
    "feature_pattern_transform_lanes",
    "feature_payload_strings",
    "feature_point_construction_headers",
    "feature_point_construction_scalar_lanes",
    "feature_projected_curve_construction_payloads",
    "feature_projected_curve_construction_strings",
    "feature_projected_curve_references",
    "feature_simple_hole_construction_groups",
    "feature_simple_hole_repeated_scalar_lane_block_references",
    "feature_simple_hole_repeated_scalar_lanes",
    "feature_simple_hole_templates",
    "feature_symbolic_threads",
    "feature_sketch_construction_inputs",
    "feature_sketch_construction_payloads",
    "feature_sketch_datum_csys_dependencies",
    "feature_sketch_fixed_points",
    "feature_sketch_named_point_block_uses",
    "feature_sketch_payload_coordinate_pairs",
    "feature_sketch_payload_fixed_pairs",
    "feature_sketch_payload_mixed_pairs",
    "feature_sketch_payload_named_records",
    "feature_sketch_payload_names",
    "feature_sketch_payload_scalar_lanes",
    "feature_sketch_payload_scalars",
    "feature_sketch_point_groups",
    "feature_sketch_point_uses",
    "feature_sketch_points",
    "feature_sketch_preceding_named_point_uses",
    "feature_sketch_records",
    "feature_sketch_references",
    "feature_surface_construction_branches",
    "feature_surface_construction_payloads",
    "feature_surface_construction_references",
    "feature_surface_construction_scalar_pairs",
    "feature_surface_construction_strings",
    "field_definitions",
    "material_texture_assets",
    "material_texture_catalog_entries",
    "object_records",
    "object_record_handle_pairs",
    "object_references",
    "object_uuid_values",
    "offset_store_named_points",
    "om_operation_state_counters",
    "om_operation_state_journal_groups",
    "om_operation_state_messages",
    "om_operation_state_slot_lanes",
    "om_operation_state_statuses",
    "om_roll_forward_state_groups",
    "om_record_areas",
    "parasolid_attribute_class_uses",
    "parasolid_attribute_field_uses",
    "parasolid_attribute_field_names",
    "parasolid_attribute_definitions",
    "parasolid_blend_bound_records",
    "parasolid_blend_surface_records",
    "parasolid_chart_records",
    "parasolid_deltas_body_revisions",
    "parasolid_deltas_transmit_headers",
    "parasolid_deltas_terminal_null_references",
    "parasolid_deltas_records",
    "parasolid_deltas_residual_spans",
    "parasolid_deltas_tagged_reference_lanes",
    "parasolid_deltas_reference_type_maps",
    "parasolid_deltas_reference_state_packets",
    "parasolid_deltas_schema_reference_preambles",
    "parasolid_deltas_reference_marker_packets",
    "parasolid_deltas_type_150_state_packets",
    "parasolid_deltas_inline_schema_declarations",
    "parasolid_deltas_inline_body_states",
    "parasolid_deltas_term_use_numeric_tails",
    "parasolid_deltas_tombstones",
    "parasolid_entity_51_numeric_uses",
    "parasolid_entity_51_records",
    "parasolid_entity_51_structured_uses",
    "parasolid_entity_51_string_uses",
    "parasolid_entity_52_integer_records",
    "parasolid_entity_53_double_records",
    "parasolid_entity_54_string_records",
    "parasolid_entity_57_axis_records",
    "parasolid_entity_58_tag_records",
    "parasolid_entity_62_unicode_records",
    "parasolid_entity_vector_records",
    "parasolid_field_names_records",
    "parasolid_intersection_records",
    "parasolid_offset_surface_records",
    "parasolid_support_uv_records",
    "parasolid_surface_curve_records",
    "parasolid_term_use_records",
    "parasolid_topology_attribute_class_uses",
    "parasolid_topology_attribute_list_references",
    "parasolid_trimmed_curve_records",
    "part_attributes",
    "part_color_definitions",
    "part_color_tables",
    "rm_display_color_assignments",
    "persistent_handles",
    "rm_creation_display_data_relations",
    "rmfastload_object_id_tables",
    "rmfastload_object_ids",
    "saved_toggle_entries",
    "saved_toggle_streams",
    "segment_body_bindings",
    "segment_body_lineage_statuses",
    "segment_index_rows",
    "segment_om_links",
    "segment_stream_links",
    "store_headers",
    "string_values",
];

/// Minimum distinct arenas the golden fixtures must collectively populate.
const ARENA_COVERAGE_FLOOR: usize = 122;

const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test-fast golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), "prt", REGENERATE)
}

fn inputs() -> Vec<(String, Vec<u8>)> {
    fixtures()
        .into_iter()
        .map(|(name, bytes)| (name.to_string(), bytes))
        .collect()
}

/// Covering fixture set as `(golden name, full `.prt` bytes)`.
fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    let mut f: Vec<(&'static str, Vec<u8>)> = Vec::new();

    // Self-contained `.prt` images.
    f.push(("single_part_prt", single_part_prt()));
    f.push(("topology_part_prt", topology_part_prt()));
    f.push(("prt_with_arrangements", prt_with_arrangements()));
    f.push((
        "prt_with_arrangement_attribute_none",
        prt_with_arrangement_attribute(None),
    ));
    f.push(("prt_with_indexed_om_section", prt_with_indexed_om_section()));
    f.push((
        "prt_with_size_framed_om_section",
        prt_with_size_framed_om_section(),
    ));
    f.push(("assembly_prt", assembly_prt()));
    f.push((
        "assembly_with_external_paths",
        assembly_with_external_paths(),
    ));
    f.push(("rmfastload_prt", rmfastload_prt()));
    f.push((
        "prt_with_two_bodies_and_rmfastload",
        prt_with_two_bodies_and_rmfastload(),
    ));
    f.push((
        "prt_with_two_active_bodies_and_rmfastload",
        prt_with_two_active_bodies_and_rmfastload(),
    ));
    f.push((
        "prt_with_missing_active_body_record",
        prt_with_missing_active_body_record(),
    ));
    f.push((
        "prt_with_weak_rmfastload_overlap",
        prt_with_weak_rmfastload_overlap(),
    ));

    // Parasolid neutral-binary attribute/entity records in a partition stream.
    f.push((
        "parasolid_entity_records",
        prt_with_partition(&parasolid_entity_records_stream()),
    ));

    // Embedded DisplayJT stream: outer index, one JT document, one segment.
    f.push((
        "display_jt_basic",
        prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/UG_PART/DisplayJT", display_jt_basic_stream()),
        ]),
    ));
    f.push((
        "display_jt_scene_graph",
        prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/UG_PART/DisplayJT", display_jt_scene_graph_stream()),
        ]),
    ));
    f.push((
        "display_jt_shape_lod",
        prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/UG_PART/DisplayJT", display_jt_shape_lod_stream()),
        ]),
    ));
    f.push((
        "display_jt_string_property",
        prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            (
                "/Root/UG_PART/DisplayJT",
                display_jt_string_property_stream(),
            ),
        ]),
    ));

    // Offset-store control blocks: the plain form resolves class-registry
    // ordinals; the handle form carries two adjacent persistent handles.
    f.push((
        "data_block_control_class_references",
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]),
    ));
    f.push((
        "offset_store_named_point",
        prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            offset_only_indexed_om_section_with_named_point(),
        )]),
    ));
    f.push((
        "data_block_control_index_values",
        prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            offset_only_indexed_om_section_with_index_values(),
        )]),
    ));
    // EXTREFSTREAM index, string table, and handle-set records.
    f.push((
        "external_reference_stream",
        prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/ExternalReferences", external_reference_stream()),
        ]),
    ));

    f.push(("data_block_control_handles", {
        let mut control = Vec::new();
        control.extend_from_slice(&[0xe0, 0, 0, 0, 1]);
        control.extend_from_slice(&[0xe0, 0, 0, 0, 2]);
        prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            offset_only_indexed_om_section_with_control(&control),
        )])
    }));

    // OM record areas / feature history, wrapped as a named UG_PART payload.
    f.push((
        "om_record_area",
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_om_record_area_payload())]),
    ));
    f.push((
        "om_record_area_input_store",
        prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            segment_om_record_area_with_input_store_payload(),
        )]),
    ));
    f.push((
        "multi_section_feature_history",
        prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            multi_section_feature_history_payload(),
        )]),
    ));
    f.push(("composed_feature_history", composed_feature_history_prt()));
    f.push((
        "segment_index_rows",
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]),
    ));
    f.push((
        "segment_stream_links",
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_stream_payload())]),
    ));
    f.push((
        "segment_body_bindings",
        prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            segment_body_binding_payload("partition"),
        )]),
    ));
    f.push((
        "material_texture_assets",
        prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            (
                "/Root/materialsTif/AISI Steel 4340",
                vec![b'I', b'I', 42, 0, 8, 0, 0, 0, 0, 0],
            ),
            (
                "/Root/materialsTif/Truncated",
                vec![b'I', b'I', 42, 0, 40, 0, 0, 0, 0, 0],
            ),
        ]),
    ));
    f.push(("material_texture_catalog", prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/materialsTif/unmap$1", vec![b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0]),
        ("/Root/qafmetadata", br#"<?xml version="1.0" encoding="UTF-8"?>
<folderContents>
<folderProperties location="images/preview" unmappedLocation="images/preview"><createTime>2026-07-15T08:00:00</createTime><modifyTime>2026-07-15T08:00:01</modifyTime></folderProperties>
<folderProperties location="materialsTif/unmap$1" unmappedLocation="materialsTif/Carbon Fiber Harness Satin Coated"><createTime>2026-07-15T08:01:00</createTime><modifyTime>2026-07-15T08:02:00</modifyTime></folderProperties>
</folderContents>"#.to_vec()),
    ])));
    f.push(("om_repeated_operations", {
        let section = size_framed_om_section_with_repeated_operations(12);
        let mut payload = Vec::new();
        for word in [24_u32, 9, 11, 1, 1, 24] {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        payload.extend_from_slice(&section);
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
    }));

    // Lone partition streams, each wrapped with `prt_with_partition`.
    let partitions: Vec<(&'static str, Vec<u8>)> = vec![
        (
            "topology_with_missing_tolerances",
            topology_with_missing_tolerances(),
        ),
        ("partition_stream", partition_stream()),
        (
            "offset_surface_topology_partition_stream",
            offset_surface_topology_partition_stream(),
        ),
        (
            "offset_surface_with_fully_extended_common_header",
            offset_surface_with_fully_extended_common_header(),
        ),
        (
            "surface_curve_topology_partition_stream",
            surface_curve_topology_partition_stream(),
        ),
        (
            "pcurve_topology_partition_stream",
            pcurve_topology_partition_stream(),
        ),
        (
            "shared_region_shells_partition_stream",
            shared_region_shells_partition_stream(),
        ),
        (
            "blend_surface_topology_partition_stream",
            blend_surface_topology_partition_stream(),
        ),
        (
            "blend_surface_with_extended_support_reference",
            blend_surface_with_extended_support_reference(),
        ),
        (
            "blend_surface_with_intersection_spine",
            blend_surface_with_intersection_spine(),
        ),
        (
            "blend_surface_with_forward_blend_support",
            blend_surface_with_forward_blend_support(),
        ),
        (
            "intersection_curve_topology_partition_stream",
            intersection_curve_topology_partition_stream(),
        ),
        (
            "charted_intersection_curve_topology_partition_stream",
            charted_intersection_curve_topology_partition_stream(),
        ),
        (
            "charted_intersection_with_edge_endpoint_witnesses_stream",
            charted_intersection_with_edge_endpoint_witnesses_stream(),
        ),
        (
            "charted_intersection_without_uv_stream",
            charted_intersection_without_uv_stream(),
        ),
        (
            "charted_intersection_with_approximated_term_stream",
            charted_intersection_with_approximated_term_stream(),
        ),
        (
            "two_support_charted_intersection_curve_stream",
            two_support_charted_intersection_curve_stream(),
        ),
        (
            "blend_bound_charted_intersection_curve_stream",
            blend_bound_charted_intersection_curve_stream(),
        ),
        (
            "inline_descriptor_intersection_curve_stream",
            inline_descriptor_intersection_curve_stream(),
        ),
        (
            "circle_topology_partition_stream",
            circle_topology_partition_stream(),
        ),
        (
            "ellipse_topology_partition_stream",
            ellipse_topology_partition_stream(),
        ),
        (
            "cylinder_topology_partition_stream",
            cylinder_topology_partition_stream(),
        ),
        (
            "cone_topology_partition_stream",
            cone_topology_partition_stream(),
        ),
        (
            "sphere_topology_partition_stream",
            sphere_topology_partition_stream(),
        ),
        (
            "torus_topology_partition_stream",
            torus_topology_partition_stream(),
        ),
        ("bspline_partition_stream", bspline_partition_stream()),
        (
            "extended_bspline_surface_stream",
            extended_bspline_surface_stream(),
        ),
        (
            "bspline_surface_replacement_partition_stream",
            bspline_surface_replacement_partition_stream(),
        ),
        (
            "bspline_curve_replacement_partition_stream",
            bspline_curve_replacement_partition_stream(),
        ),
        (
            "trimmed_topology_partition_stream",
            trimmed_topology_partition_stream(),
        ),
        (
            "mismatched_trimmed_topology_partition_stream",
            mismatched_trimmed_topology_partition_stream(),
        ),
        (
            "partnered_trimmed_topology_partition_stream",
            partnered_trimmed_topology_partition_stream(),
        ),
        (
            "forward_trimmed_curve_chain_stream",
            forward_trimmed_curve_chain_stream(),
        ),
        (
            "topology_with_extended_edge_curve_reference",
            topology_with_extended_edge_curve_reference(),
        ),
        (
            "topology_with_extended_face_attribute_reference",
            topology_with_extended_face_attribute_reference(),
        ),
        (
            "topology_with_extended_edge_attribute_reference",
            topology_with_extended_edge_attribute_reference(),
        ),
        (
            "topology_with_extended_internal_topology_references",
            topology_with_extended_internal_topology_references(),
        ),
        (
            "topology_with_fully_extended_geometry_headers",
            topology_with_fully_extended_geometry_headers(),
        ),
        (
            "topology_with_escaped_geometry_envelopes",
            topology_with_escaped_geometry_envelopes(),
        ),
        (
            "deltas_intersection_curve_stream",
            deltas_intersection_curve_stream(),
        ),
        ("status_framed_deltas_stream", status_framed_deltas_stream()),
        (
            "variable_status_framed_deltas_stream",
            variable_status_framed_deltas_stream(),
        ),
        (
            "status_framed_deltas_point_stream",
            status_framed_deltas_point_stream(),
        ),
        (
            "deltas_point_partition_stream",
            deltas_point_partition_stream(),
        ),
        ("many_face_partition_stream", many_face_partition_stream(1)),
        (
            "large_xmt_headers_topology",
            large_xmt_headers(&topology_partition_stream()),
        ),
    ];
    for (name, stream) in partitions {
        f.push((name, prt_with_partition(&stream)));
    }

    // Deltas streams paired with an equal-schema partition via `prt_with_streams`.
    let deltas_pairs: Vec<(&'static str, Vec<u8>, Vec<u8>)> = vec![
        (
            "deltas_edge",
            topology_partition_stream(),
            deltas_edge_partition_stream(),
        ),
        (
            "deltas_face_vertex",
            topology_partition_stream(),
            deltas_face_vertex_partition_stream(),
        ),
        (
            "deltas_loop",
            topology_partition_stream(),
            deltas_loop_partition_stream(),
        ),
        (
            "deltas_shell",
            topology_partition_stream(),
            deltas_shell_partition_stream(),
        ),
        (
            "deltas_fin",
            topology_partition_stream(),
            deltas_fin_partition_stream(),
        ),
        (
            "deltas_line",
            topology_partition_stream(),
            deltas_line_partition_stream(),
        ),
        (
            "deltas_plane",
            topology_partition_stream(),
            deltas_plane_partition_stream(),
        ),
        (
            "deltas_offset_surface",
            offset_surface_topology_partition_stream(),
            deltas_offset_surface_partition_stream(),
        ),
        (
            "deltas_blend_surface",
            blend_surface_topology_partition_stream(),
            deltas_blend_surface_partition_stream(),
        ),
        (
            "deltas_trimmed_curve",
            trimmed_topology_partition_stream(),
            deltas_trimmed_curve_partition_stream(),
        ),
        (
            "deltas_surface_curve",
            surface_curve_topology_partition_stream(),
            deltas_surface_curve_partition_stream(),
        ),
        (
            "deltas_circle",
            circle_topology_partition_stream(),
            deltas_circle_partition_stream(),
        ),
        (
            "deltas_ellipse",
            ellipse_topology_partition_stream(),
            deltas_ellipse_partition_stream(),
        ),
        (
            "deltas_cylinder",
            cylinder_topology_partition_stream(),
            deltas_cylinder_partition_stream(),
        ),
        (
            "deltas_cone",
            cone_topology_partition_stream(),
            deltas_cone_partition_stream(),
        ),
        (
            "deltas_sphere",
            sphere_topology_partition_stream(),
            deltas_sphere_partition_stream(),
        ),
        (
            "deltas_torus",
            torus_topology_partition_stream(),
            deltas_torus_partition_stream(),
        ),
        (
            "deltas_bspline_surface",
            bspline_surface_replacement_partition_stream(),
            deltas_bspline_surface_wrapper_stream(),
        ),
        (
            "deltas_bspline_curve",
            bspline_curve_replacement_partition_stream(),
            deltas_bspline_curve_wrapper_stream(),
        ),
    ];
    for (name, partition, delta) in deltas_pairs {
        f.push((name, prt_with_streams(&[&partition, &delta])));
    }

    let ext11_pairs = [
        (
            "ext11_charted_intersection_curve_stream",
            charted_intersection_curve_topology_partition_stream(),
            ext11_charted_intersection_curve_stream(),
        ),
        (
            "two_support_ext11_charted_intersection_curve_stream",
            two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]),
            two_support_ext11_charted_intersection_curve_stream(false),
        ),
        (
            "two_support_ext11_charted_intersection_curve_stream_ambiguous",
            two_support_charted_intersection_curve_stream(),
            two_support_ext11_charted_intersection_curve_stream(true),
        ),
        (
            "partial_ext11_charted_intersection_curve_stream",
            two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]),
            partial_ext11_charted_intersection_curve_stream(),
        ),
    ];
    for (name, partition, ext11) in ext11_pairs {
        f.push((name, prt_with_ext11_intersection(&partition, &ext11)));
    }

    f
}

/// Serialize decode + inspect output as stable pretty JSON. Errors are frozen.
fn snapshot(bytes: &[u8]) -> String {
    let decode = match NxCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default()) {
        Ok(result) => serde_json::json!({
            "ir": serde_json::to_value(result.ir()).expect("serialize ir"),
            "report": serde_json::to_value(result.report()).expect("serialize report"),
            "source_fidelity": serde_json::to_value(result.source_fidelity())
                .expect("serialize source_fidelity"),
        }),
        Err(err) => serde_json::json!({ "decode_error": err.to_string() }),
    };
    let inspect =
        match NxCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default()) {
            Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect"),
            Err(err) => serde_json::json!({ "inspect_error": err.to_string() }),
        };
    snapshot_text(&serde_json::json!({ "decode": decode, "inspect": inspect }))
}

#[test]
fn golden_snapshots_are_byte_identical() {
    harness().check_inputs(&inputs(), &[Branch::new("", snapshot)]);
}

/// Guards against nondeterministic codec output (`HashMap` iteration order,
/// timestamps): decoding the same bytes twice must produce identical JSON.
#[test]
fn golden_output_is_deterministic() {
    harness().check_determinism_inputs(&inputs(), &[Branch::new("", snapshot)]);
}

/// Union of `nx`-namespace arenas the fixture set populates.
fn covered_arenas() -> BTreeSet<String> {
    let mut covered = BTreeSet::new();
    for (_, bytes) in fixtures() {
        let Ok(result) = NxCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()) else {
            continue;
        };
        if let Some(namespace) = result.ir().native.namespace("nx") {
            for (arena, records) in &namespace.arenas {
                if !records.is_empty() {
                    covered.insert(arena.clone());
                }
            }
        }
    }
    covered
}

/// Every arena a fixture populates must be a name production actually writes.
#[test]
fn arena_coverage_is_a_subset() {
    let known: BTreeSet<&str> = KNOWN_ARENAS.iter().copied().collect();
    let unknown: Vec<String> = covered_arenas()
        .into_iter()
        .filter(|a| a != "unknowns" && !known.contains(a.as_str()))
        .collect();
    assert!(
        unknown.is_empty(),
        "fixtures populated arenas absent from KNOWN_ARENAS (update the denominator): {unknown:?}"
    );
}

/// Collective arena coverage floor across the fixture set.
#[test]
fn arena_coverage_meets_floor() {
    let covered = covered_arenas();
    let known: BTreeSet<&str> = KNOWN_ARENAS.iter().copied().collect();
    let hit = covered
        .iter()
        .filter(|a| known.contains(a.as_str()))
        .count();
    let uncovered: Vec<&str> = KNOWN_ARENAS
        .iter()
        .copied()
        .filter(|a| !covered.contains(*a))
        .collect();
    println!(
        "golden arena coverage: {hit}/{} known arenas ({:.1}%)\nuncovered: {uncovered:?}",
        KNOWN_ARENAS.len(),
        100.0 * hit as f64 / KNOWN_ARENAS.len() as f64,
    );
    assert!(
        hit >= ARENA_COVERAGE_FLOOR,
        "arena coverage regressed: {hit} < floor {ARENA_COVERAGE_FLOOR}"
    );
}

/// Every catalogue arena appears once, and the catalogue arena set equals
/// `KNOWN_ARENAS`.
#[test]
fn catalogue_arenas_match_known_arenas() {
    use cadmpeg_ir::native::catalogue::Phase;

    use crate::native::catalogue::CATALOGUE;

    assert_eq!(CATALOGUE.len(), 242, "one catalogue row per model field");
    assert_eq!(
        CATALOGUE
            .iter()
            .filter(|row| row.phase == Phase::GroupA)
            .count(),
        117,
        "group A family count"
    );
    assert_eq!(
        CATALOGUE
            .iter()
            .filter(|row| row.phase == Phase::GroupB)
            .count(),
        9,
        "group B family count"
    );

    let mut catalogue_arenas = BTreeSet::new();
    for row in CATALOGUE {
        assert!(
            catalogue_arenas.insert(row.arena),
            "arena {:?} appears in more than one catalogue row",
            row.arena
        );
    }
    assert_eq!(
        catalogue_arenas.len(),
        CATALOGUE.len(),
        "every catalogue row owns a distinct arena"
    );

    let known: BTreeSet<&str> = KNOWN_ARENAS.iter().copied().collect();
    let catalogue_not_known: Vec<&str> = catalogue_arenas.difference(&known).copied().collect();
    let known_not_catalogue: Vec<&str> = known.difference(&catalogue_arenas).copied().collect();
    assert!(
        catalogue_not_known.is_empty(),
        "catalogue arenas absent from KNOWN_ARENAS: {catalogue_not_known:?}"
    );
    assert!(
        known_not_catalogue.is_empty(),
        "KNOWN_ARENAS entries absent from CATALOGUE: {known_not_catalogue:?}"
    );
}
