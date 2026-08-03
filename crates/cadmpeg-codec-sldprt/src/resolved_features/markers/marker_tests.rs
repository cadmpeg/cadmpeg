//! Tests for the `markers` module.

use super::{
    additional_linked_profile_point_coordinates, alternate_current_indexed_curve_endpoint_indices,
    alternate_current_selected_axis_endpoint_indices, angled_reference_plane_frame,
    append_spatial_vertex, arc_angle_relation_kind, auxiliary_profile_record,
    bind_resolved_curve_vertices, bounded_profile_axis_endpoints, classed_offset_plane_sources,
    common_generated_surface_axis, compact_body_component_path_at, compact_body_path_at,
    compact_body_retention_mode, compact_body_selection_at, compact_body_selection_vector,
    compact_body_state_ids, compact_bounded_curve_tangent, compact_combine_operation_at,
    compact_component_plane_frame, compact_curve_endpoint_indices, compact_edge_component_path_at,
    compact_edge_path_value, compact_edge_selection_at, compact_edge_selection_set_value,
    compact_edge_selections, compact_extrusion_blind_at,
    compact_extrusion_blind_through_all_second_at, compact_extrusion_mid_plane_at,
    compact_extrusion_offset_from_face_at, compact_extrusion_through_all_at,
    compact_extrusion_through_all_both_at, compact_extrusion_through_next_at,
    compact_extrusion_to_face_at, compact_extrusion_to_vertex_at, compact_general_curve_ref_at,
    compact_geometry_locus_point_coordinates, compact_indexed_curve_endpoint_indices,
    compact_legacy_code_one_line_endpoint_indices, compact_legacy_curve_endpoint_indices,
    compact_legacy_linked_profile_point_coordinates, compact_legacy_object_line_endpoints,
    compact_legacy_profile_vertex, compact_legacy_rectangle_line_endpoints,
    compact_legacy_selected_axis_endpoint_indices,
    compact_legacy_short_role_one_curve_endpoint_indices,
    compact_legacy_short_role_two_curve_endpoint_indices, compact_line_chain_addresses,
    compact_line_region_addresses, compact_offset_plane_source,
    compact_profile_reference_plane_source, compact_radial_circle_index,
    compact_reference_plane_frame, compact_reference_plane_source,
    compact_single_face_reference_path_at, compact_single_face_reference_record_at,
    compact_sketch_surface_component_path_at, compact_surface_selection_at,
    complete_ordered_compact_line_profile, component_face_reference_at,
    component_face_reference_in_record, component_path_feature, component_path_features,
    component_path_input_features, component_path_terminal_feature, component_profile_source_at,
    component_reference_curve_path_at, consecutive_legacy_profile_line_endpoints,
    constraint_midplane_frame, constraint_reference_plane_frame,
    coordinate_centered_line_endpoints, coordinate_circle_radius, coordinate_marker_local_links,
    coordinate_roster_arc_center, coordinate_roster_curve_endpoint_markers,
    coordinate_roster_endpoint_offset, coordinate_roster_full_circle,
    cosmetic_thread_cylinder_marker_reference, cosmetic_thread_cylinder_reference_at,
    cosmetic_thread_cylinder_references, cosmetic_thread_diameter_child_tail,
    current_compact_104_indexed_line_endpoint_indices, current_compact_104_profile_line,
    current_coordinate_linked_line_endpoints, current_direct_92_profile_line_endpoint_indices,
    current_geometry_locus_profile_vertex, current_indexed_arc_reverses_center_sweep,
    current_identity_linked_wide_curve_uses_one_based_roster, current_linked_semicircle_record,
    current_long_full_circle_radial_index, current_referenced_compact_curve_uses_marker_roster,
    current_reverse_incidence_endpoint_offsets, current_wide_arc_direct_markers,
    current_undetailed_bounded_curve_is_line, direct_indexed_curve_endpoint_indices,
    enrich_history_revolution_inputs, equal_index_coordinate_roster_full_circle,
    explicit_reference_axis_frame, explicit_reference_plane_frame,
    extended_compact_84_construction_line_endpoint_indices,
    extended_compact_96_selected_axis_endpoint_indices, extended_compact_endpoint_markers,
    extended_declared_inline_line_endpoints, extended_direct_object_line_endpoint_ids,
    extended_direct_object_line_endpoints, extended_identity_inline_line_endpoints,
    extended_linked_inline_line_endpoints, extended_profile_point_coordinates,
    extended_profile_roster_construction_line_endpoint_indices, extended_radial_circle_index,
    extended_tagged_indexed_curve_endpoint_indices, extended_terminal_profile_line,
    extended_terminal_repeated_radial_circle_index, extended_wide_construction_line_roster_indices,
    extended_wide_selected_axis_endpoints, fixed_reference_plane_frame,
    generated_surface_identities, geometry_locus_profile_vertex,
    history_features_with_object_sources, indexed_arc_uses_coordinate_center,
    indexed_profile_vertex, indexed_rectangle_from_line_cycle, inline_arc_coordinates,
    inline_surface_reference_at, legacy_compact_104_profile_line_endpoint_indices,
    legacy_compact_diameter_arc_center, legacy_compact_direct_endpoint_markers,
    legacy_compact_profile_line, legacy_coordinate_circle_radius,
    legacy_coordinate_roster_selected_axis_endpoint_indices,
    legacy_declared_handle_coordinates, legacy_direct_compact_selected_axis_endpoint_indices,
    legacy_extended_linked_profile_point_coordinates, legacy_extended_profile_curve_kind,
    legacy_extended_rectangle_diagonal_endpoint, legacy_feature_input_section,
    legacy_linked_coordinates, legacy_long_profile_line_endpoint_indices,
    legacy_offset_plane_face_alias, legacy_point_roster_line_endpoint_markers,
    legacy_reference_axis_triads, legacy_referenced_wide_arc_endpoint_indices,
    legacy_single_face_reference_path_at, legacy_single_incidence_profile_point_coordinates,
    legacy_state_five_curve_endpoint_indices, legacy_terminal_indexed_profile_line,
    legacy_terminal_profile_endpoint_offset, legacy_undetailed_profile_line,
    legacy_unlocated_geometry_handle, linked_profile_point, marker_coordinates,
    marker_curve_endpoint_markers, marker_is_geometry_locus, marker_is_selected_construction_line,
    marker_local_id, marker_local_links, marker_object_index, marker_spatial_coordinates,
    matrix_reference_plane_frame, minimal_reference_plane_frame, mirror_pattern_component_path_at,
    mirror_surface_component_path_at, named_scalars, native_scalar_matches_discrete_parameter,
    normalize_indexed_curve_entities, object_names, offset_plane_reference_frame_matches,
    offset_plane_reference_source, offset_reference_plane_frame_pair,
    one_based_point_roster_line_endpoint_markers, ordered_compact_line_profile,
    ordered_rectangle_corners, packed_legacy_curve_endpoint_indices,
    packed_legacy_linked_profile_point_coordinates, patch_spatial_vertex,
    plane_intersection_axis_frame, plane_intersection_axis_sources, principal_sketch_frame,
    profile_roster_construction_axis, profile_roster_origin_axis_endpoints,
    profile_roster_principal_axis_endpoints, project_unbound_cosmetic_thread_faces,
    radial_dimension_radius, reconcile_reference_plane_frame, reference_plane_frame_key,
    resolve_operand_marker, resolve_operand_marker_excluding, resolve_scalar_operand_markers,
    resolve_two_center_semicircle_profile, revolution_line_reference_inputs, revolution_operation,
    revolution_temporary_axis, roster_curve_endpoint_markers, select_reference_plane_frame_source,
    sketch_block_identity_normalization_origin, sketch_block_record_origin, sketch_input_entities,
    sketch_plane_frames, solved_tangent, spatial_vertex_coordinates,
    structured_offset_plane_sources, surface_reference_matches_at,
    surface_selection_producer_features, tangent_bounded_curve,
    terminal_extended_profile_point_coordinates, terminal_repeated_radial_circle_pairs,
    unique_arc_center_marker, unique_cylindrical_face, unique_dimensioned_rectangle_markers,
    unique_locus, unique_marker_candidate, unique_planar_face, unique_topological_cylindrical_face,
    wide_direct_line_endpoint_markers, wide_indexed_curve_endpoint_indices, Angle, BooleanOp,
    CompactPointReferenceKind, CompactReferencePlaneIndex, ComponentPathEnd, Length, CLASS_MARKER,
    COMPACT_EDGE_VECTOR_MARKER, FIXED_REFERENCE_PLANE_FRAME_LEN, LEGACY_EXTENDED_SKETCH_MARKER,
    LEGACY_SKETCH_MARKER, MINIMAL_REFERENCE_PLANE_FRAME_LEN, NAME_MARKER, SCALAR_HEADER,
    SKETCH_MARKER,
};
use crate::records::{
    Feature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
    FeatureInputComponentPathEntry, FeatureInputEdgeSelection, FeatureInputLane, FeatureInputName,
    FeatureInputOperand, FeatureInputOperandKind, FeatureInputScalar, FeatureInputScalarRole,
    FeatureInputSurfaceSelection, SketchInputEntity, SketchInputKind, SketchInputLink,
    SketchRelationKind,
};
use cadmpeg_ir::features::{
    DesignParameter, DimensionDisplay, FeatureId, ParameterId, ParameterValue,
};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchLocus,
};
use cadmpeg_ir::topology::{Face, Sense};
use std::collections::{BTreeMap, HashMap, HashSet};

#[test]
fn spatial_vertex_patch_preserves_record_shape_and_order() {
    let first = Point3::new(1.0, 2.0, 3.0);
    let second = Point3::new(4.0, 5.0, 6.0);
    let mut payload = Vec::new();
    append_spatial_vertex(&mut payload, first);
    append_spatial_vertex(&mut payload, second);

    let replacement = Point3::new(-7.5, 8.25, 9.0);
    patch_spatial_vertex(&mut payload, 0, replacement).expect("required invariant");

    assert_eq!(
        spatial_vertex_coordinates(&payload),
        vec![replacement, second]
    );
    assert_eq!(payload.len(), 138);
}


#[test]
fn current_spatial_point_marker_decodes_model_coordinates() {
    let mut payload = vec![0; 90];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [0.125_f64, -0.25, 0.375].into_iter().enumerate() {
        let start = 66 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, 0),
        Some(Point3::new(125.0, -250.0, 375.0))
    );
    payload[64] = 0x1e;
    assert_eq!(marker_spatial_coordinates(&payload, 0), None);
}


#[test]
fn legacy_spatial_point_marker_decodes_model_coordinates() {
    let offset = 4;
    let mut payload = vec![0; 94];
    payload[..offset].copy_from_slice(&1u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 64..offset + 66].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [0.125_f64, -0.25, 0.375].into_iter().enumerate() {
        let start = offset + 66 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, offset),
        Some(Point3::new(125.0, -250.0, 375.0))
    );
    payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        marker_spatial_coordinates(&payload, offset),
        Some(Point3::new(125.0, -250.0, 375.0))
    );
    payload[offset + 4] = 3;
    assert_eq!(marker_spatial_coordinates(&payload, offset), None);
}


#[test]
fn relation_backed_spatial_point_markers_decode_model_coordinates() {
    for (marker, sentinel, coordinates) in [
        (LEGACY_SKETCH_MARKER, 64, 66),
        (LEGACY_EXTENDED_SKETCH_MARKER, 56, 58),
    ] {
        let offset = 4;
        let mut payload = vec![0; offset + coordinates + 24];
        payload[..offset].copy_from_slice(&1u32.to_le_bytes());
        payload[offset..offset + marker.len()].copy_from_slice(marker);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 17..offset + 21].copy_from_slice(&3u32.to_le_bytes());
        payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + sentinel..offset + sentinel + 2].copy_from_slice(&[0x0e, 0x00]);
        for (index, value) in [-0.08_f64, 0.075, 0.0055].into_iter().enumerate() {
            let start = offset + coordinates + index * 8;
            payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }

        assert_eq!(
            marker_spatial_coordinates(&payload, offset),
            Some(Point3::new(-80.0, 75.0, 5.5))
        );
    }
}


#[test]
fn packed_legacy_spatial_point_uses_compact_coordinate_offset() {
    let mut payload = vec![0; 74];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&0u32.to_le_bytes());
    payload[19..25].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[29] = 0x05;
    payload[40..48].copy_from_slice(&1.0f64.to_le_bytes());
    payload[48..50].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [0.125_f64, -0.25, 0.375].into_iter().enumerate() {
        let start = 50 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, 0),
        Some(Point3::new(125.0, -250.0, 375.0))
    );
    payload[48] = 0x1e;
    assert_eq!(marker_spatial_coordinates(&payload, 0), None);
}


#[test]
fn current_spatial_point_variants_decode_model_coordinates() {
    for (kind, marker, coordinates) in [(0_u32, 56, 58), (1_u32, 64, 66)] {
        let mut payload = vec![0; 90];
        payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&kind.to_le_bytes());
        payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        payload[27..29].copy_from_slice(&1u16.to_le_bytes());
        payload[marker..marker + 2].copy_from_slice(&[0x0e, 0x00]);
        for (index, value) in [0.125_f64, -0.25, 0.375].into_iter().enumerate() {
            let start = coordinates + index * 8;
            payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }

        assert_eq!(
            marker_spatial_coordinates(&payload, 0),
            Some(Point3::new(125.0, -250.0, 375.0))
        );
    }
}


#[test]
fn object_indexed_spatial_point_uses_compact_coordinates() {
    let offset = 4;
    let mut payload = vec![0; offset + 82];
    payload[..offset].copy_from_slice(&1u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&5u32.to_le_bytes());
    payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [0.035_f64, 0.0, 0.1415].into_iter().enumerate() {
        let start = offset + 58 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, offset),
        Some(Point3::new(35.0, 0.0, 141.5))
    );
    payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        marker_spatial_coordinates(&payload, offset),
        Some(Point3::new(35.0, 0.0, 141.5))
    );
    payload[..offset].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(marker_spatial_coordinates(&payload, offset), None);
    payload[..offset].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 58..offset + 66].copy_from_slice(&f64::from_bits(1).to_le_bytes());
    assert_eq!(marker_spatial_coordinates(&payload, offset), None);
}


#[test]
fn extended_spatial_point_marker_uses_compact_coordinate_offset() {
    let mut payload = vec![0; 82];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [-0.125_f64, 0.25, -0.375].into_iter().enumerate() {
        let start = 58 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, 0),
        Some(Point3::new(-125.0, 250.0, -375.0))
    );
}


#[test]
fn extended_kind_one_spatial_point_uses_wide_coordinate_offset() {
    let mut payload = vec![0; 90];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [-0.125_f64, 0.25, -0.375].into_iter().enumerate() {
        let start = 66 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, 0),
        Some(Point3::new(-125.0, 250.0, -375.0))
    );
}


#[test]
fn extended_object_indexed_spatial_point_uses_wide_coordinate_offset() {
    let offset = 4;
    let mut payload = vec![0; offset + 90];
    payload[..offset].copy_from_slice(&1u32.to_le_bytes());
    payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 64..offset + 66].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [-0.125_f64, 0.25, -0.375].into_iter().enumerate() {
        let start = offset + 66 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, offset),
        Some(Point3::new(-125.0, 250.0, -375.0))
    );
    payload[..offset].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(marker_spatial_coordinates(&payload, offset), None);
}


#[test]
fn sketch_block_terminal_identity_carries_its_origin() {
    let mut payload = vec![0; 100];
    payload[8..12].copy_from_slice(&[0xff; 4]);
    payload[20..26].copy_from_slice(&[0x02, 0, 0, 0, 0, 0]);
    payload[26..28].copy_from_slice(&17_u16.to_le_bytes());
    payload[48..52].copy_from_slice(&[0, 0, 1, 0]);
    payload[52..54].copy_from_slice(&[0x73, 0x81]);
    for (index, value) in [0.125_f64, -0.25, 0.0].into_iter().enumerate() {
        let start = 54 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        sketch_block_record_origin(&payload, 0, payload.len()),
        Some(Point3::new(125.0, -250.0, 0.0))
    );

    payload[52..].fill(0);
    payload[52..56].copy_from_slice(CLASS_MARKER);
    payload[56..58].copy_from_slice(&17_u16.to_le_bytes());
    payload[58..75].copy_from_slice(b"moAbsolutePoint_c");
    assert_eq!(
        sketch_block_record_origin(&payload, 0, payload.len()),
        Some(Point3::new(0.0, 0.0, 0.0))
    );
}


#[test]
fn sketch_block_identity_normalization_is_inverted_for_placement() {
    let mut payload = vec![0; 300];
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&7_u16.to_le_bytes());
    payload.extend_from_slice(b"sgBlock");
    let body = payload.len();
    payload.resize(body + 184, 0);
    for (index, value) in [1.0_f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
        .into_iter()
        .enumerate()
    {
        let start = body + 72 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[body + 144..body + 152].copy_from_slice(&1_u64.to_le_bytes());
    for (index, value) in [-0.21_f64, 0.661, 0.0].into_iter().enumerate() {
        let start = body + 152 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[body + 176..body + 184].copy_from_slice(&1.0_f64.to_le_bytes());

    assert_eq!(
        sketch_block_identity_normalization_origin(&payload, 200, payload.len()),
        Some(Point3::new(210.0, -661.0, 0.0))
    );
}


#[test]
fn relation_binding_requires_family_operand_signature() {
    use super::relation_bindings;
    use crate::records::{
        FeatureInputClass, FeatureInputClassRole, FeatureInputScalar, FeatureInputScalarRole,
    };

    let class = FeatureInputClass {
        id: "class".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 10,
        name: "sgLLDist".into(),
        role: FeatureInputClassRole::SketchConstraint,
    };
    let operand = |kind, entity_index| FeatureInputOperand {
        offset: 0,
        reference_ref: String::new(),
        kind,
        entity_index,
        entity_ref: None,
    };
    let scalar = |kind| FeatureInputScalar {
        id: "scalar".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset: 20,
        object_id: 1,
        name: "name".into(),
        value: 1.0,
        role: FeatureInputScalarRole::Driving,
        entity_indices: vec![0, 1],
        operands: vec![operand(kind, 0), operand(kind, 1)],
    };

    assert_eq!(
        relation_bindings(
            "lane",
            std::slice::from_ref(&class),
            &[scalar(FeatureInputOperandKind::E1)],
        )
        .len(),
        1
    );
    assert!(relation_bindings(
        "lane",
        &[class],
        &[scalar(FeatureInputOperandKind::Native(0x8dda))],
    )
    .is_empty());
}


#[test]
fn plane_intersection_axis_requires_two_complete_known_references() {
    let record = |source: u32, object: u8, selector: u8| {
        let mut bytes = vec![0; 46];
        bytes[..4].copy_from_slice(&source.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x6255_5715u32.to_le_bytes());
        bytes[14..16].copy_from_slice(&[1, 0]);
        bytes[22] = object;
        bytes[30] = selector;
        bytes[38..46].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
        bytes
    };
    let mut payload = record(17, 0xb6, 3);
    payload.extend_from_slice(&record(23, 0x98, 0));
    let known = [17, 23].into_iter().collect();
    assert_eq!(
        plane_intersection_axis_sources(&payload, &known),
        Some([17, 23])
    );

    payload.pop();
    assert_eq!(plane_intersection_axis_sources(&payload, &known), None);
    let incomplete = record(17, 0xb6, 3);
    assert_eq!(plane_intersection_axis_sources(&incomplete, &known), None);
}


#[test]
fn legacy_reference_axis_triad_requires_consecutive_native_records() {
    let feature = |ordinal: u32, source: u32, class: &str| Feature {
        id: format!("feature-{ordinal}"),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source.to_string()),
        parent_source_id: None,
        ordinal,
        name: String::new(),
        kind: String::new(),
        input_class: Some(class.into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    let mut features = (0..3)
        .map(|index| feature(10 + index, 40 + index, "moRefPlane_c"))
        .chain((0..3).map(|index| feature(13 + index, 43 + index, "moRefAxis_c")))
        .collect::<Vec<_>>();
    assert_eq!(
        legacy_reference_axis_triads(&features),
        vec![([3, 4, 5], [[40, 41], [40, 42], [42, 41]])]
    );

    features.insert(3, feature(99, 4, "moRefPlane_c"));
    assert_eq!(
        legacy_reference_axis_triads(&features),
        vec![([4, 5, 6], [[40, 41], [40, 42], [42, 41]])]
    );

    features[5].source_id = Some("99".into());
    assert!(legacy_reference_axis_triads(&features).is_empty());
}


#[test]
fn legacy_feature_input_section_is_an_exact_numeric_config_stream() {
    assert!(legacy_feature_input_section("Contents/Config-0"));
    assert!(legacy_feature_input_section("Contents\\Config-37"));
    assert!(!legacy_feature_input_section("Contents/Config-0-Partition"));
    assert!(!legacy_feature_input_section("Contents/Config-name"));
    assert!(!legacy_feature_input_section("Other/Config-0"));
}


#[test]
fn legacy_sketch_object_stream_requires_a_sketch_and_entity_declaration() {
    let declaration = |name: &str| {
        let mut bytes = CLASS_MARKER.to_vec();
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes
    };
    let mut payload = declaration("sgSketch");
    assert!(!super::legacy_sketch_object_stream(&payload));

    payload.extend_from_slice(&declaration("sgPointHandle"));
    assert!(super::legacy_sketch_object_stream(&payload));

    assert!(!super::legacy_sketch_object_stream(&declaration(
        "sgPointHandle"
    )));
}


#[test]
fn plane_intersection_axis_uses_the_closest_point_to_the_origin() {
    let first = (
        Point3::new(2.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    );
    let second = (
        Point3::new(0.0, -3.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    );
    assert_eq!(
        plane_intersection_axis_frame(first, second),
        Some((Point3::new(2.0, -3.0, 0.0), Vector3::new(0.0, 0.0, 1.0),))
    );

    let parallel = (
        Point3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    );
    assert_eq!(plane_intersection_axis_frame(first, parallel), None);
}


#[test]
fn explicit_reference_axis_requires_redundant_collinear_witnesses() {
    let mut record = vec![0; 88];
    for (offset, value) in [
        (0, 0.25_f64),
        (8, -0.4),
        (16, 0.1),
        (24, 0.25),
        (32, 0.6),
        (40, 0.1),
        (48, 0.0),
        (56, -0.5),
        (64, 0.0),
        (72, 1.0),
        (80, 0.0),
    ] {
        record[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut payload = vec![0xaa; 17];
    payload.extend_from_slice(&record);
    payload.extend_from_slice(&[0xbb; 11]);
    assert_eq!(
        explicit_reference_axis_frame(&payload),
        Some((Point3::new(250.0, 0.0, 100.0), Vector3::new(0.0, 1.0, 0.0),))
    );

    record[24..32].copy_from_slice(&0.5_f64.to_le_bytes());
    assert_eq!(explicit_reference_axis_frame(&record), None);
}


#[test]
fn fixed_reference_plane_uses_all_three_stored_basis_vectors() {
    let mut frame = [0; FIXED_REFERENCE_PLANE_FRAME_LEN];
    for (offset, value) in [
        (0, 0.374_f64),
        (8, -0.25),
        (16, 0.125),
        (24, 1.0),
        (32, 0.0),
        (40, 0.0),
        (49, 0.0),
        (57, 0.0),
        (65, 1.0),
        (73, 0.0),
        (81, 1.0),
        (89, 0.0),
    ] {
        frame[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    frame[48] = 1;
    assert_eq!(
        fixed_reference_plane_frame(&frame),
        Some((
            Point3::new(374.0, -250.0, 125.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );

    frame[73..81].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(fixed_reference_plane_frame(&frame), None);
    assert_eq!(fixed_reference_plane_frame(&frame[..96]), None);
}


#[test]
fn reference_plane_frame_identity_canonicalizes_signed_zero() {
    let positive = (
        Point3::new(0.0, 1.0, 2.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    );
    let negative = (
        Point3::new(-0.0, 1.0, 2.0),
        Vector3::new(1.0, -0.0, 0.0),
        Vector3::new(0.0, -0.0, 1.0),
    );

    assert_eq!(
        reference_plane_frame_key(&positive),
        reference_plane_frame_key(&negative)
    );
}


#[test]
fn offset_plane_frame_pair_stores_result_before_reference() {
    let frame = |origin_x: f64| {
        let mut bytes = [0; FIXED_REFERENCE_PLANE_FRAME_LEN];
        for (offset, value) in [
            (0, origin_x / 1000.0),
            (8, 0.0),
            (16, 0.0),
            (24, 1.0),
            (32, 0.0),
            (40, 0.0),
            (49, 0.0),
            (57, 0.0),
            (65, 1.0),
            (73, 0.0),
            (81, 1.0),
            (89, 0.0),
        ] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes[48] = 1;
        bytes
    };
    let mut payload = frame(-37.0).to_vec();
    payload.extend([0; 13]);
    payload.extend(frame(0.0));

    assert_eq!(
        offset_reference_plane_frame_pair(&payload, 37.0),
        Some((
            (
                Point3::new(-37.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            ),
            (
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            ),
        ))
    );
    payload[65..73].copy_from_slice(&(-1.0_f64).to_le_bytes());
    assert!(offset_reference_plane_frame_pair(&payload, 37.0).is_some());
    assert_eq!(offset_reference_plane_frame_pair(&payload, 38.0), None);

    let mut antiparallel = frame(-37.0).to_vec();
    antiparallel[24..32].copy_from_slice(&(-1.0_f64).to_le_bytes());
    antiparallel.extend([0; 13]);
    antiparallel.extend(frame(0.0));
    assert!(offset_reference_plane_frame_pair(&antiparallel, 37.0).is_some());
}


#[test]
fn offset_plane_frame_pair_accepts_complete_matrix_frames() {
    let sine = 0.390_731_128_489_273_27_f64;
    let cosine = 0.920_504_853_452_440_5_f64;
    let frame = |distance: f64| {
        let mut bytes = [0; 121];
        for (offset, value) in [
            (0, -sine * distance / 1000.0),
            (8, 0.0),
            (16, cosine * distance / 1000.0),
            (24, -sine),
            (32, 0.0),
            (40, cosine),
            (49, cosine),
            (57, 0.0),
            (65, -sine),
            (73, 0.0),
            (81, 1.0),
            (89, 0.0),
            (97, sine),
            (105, 0.0),
            (113, cosine),
        ] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes[48] = 1;
        bytes
    };
    let mut payload = frame(27.25).to_vec();
    payload.extend([0; 13]);
    payload.extend(frame(0.0));

    let (offset, reference) = offset_reference_plane_frame_pair(&payload, 27.25).unwrap();
    assert_eq!(offset.0, Point3::new(-sine * 27.25, 0.0, cosine * 27.25));
    assert_eq!(reference.0, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(offset.1, reference.1);
    assert_eq!(offset.2, reference.2);
}


#[test]
fn offset_plane_frame_pair_accepts_ordered_mixed_frame_layouts() {
    let mut result = [0; MINIMAL_REFERENCE_PLANE_FRAME_LEN];
    for (offset, value) in [
        (0, 0.0_f64),
        (8, 0.0),
        (16, 0.210),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (57, -0.0),
        (65, -0.210),
        (73, 1.0),
    ] {
        result[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    result[56] = 0x80;
    let mut reference = [0; 82];
    for (offset, value) in [
        (0, 0.0_f64),
        (8, 0.0),
        (16, 0.235),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (48, 0.0),
        (56, 0.0),
        (65, 0.0),
        (73, 1.0),
    ] {
        reference[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut payload = result.to_vec();
    payload.extend([0xff; 19]);
    payload.extend(reference);

    assert_eq!(
        offset_reference_plane_frame_pair(&payload, 25.0),
        Some((
            (
                Point3::new(0.0, 0.0, 210.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            ),
            (
                Point3::new(0.0, 0.0, 235.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            ),
        ))
    );
}


#[test]
fn tangent_plane_frame_is_anchored_to_its_constraint_class() {
    const CLASS: &str = "moConstraintPerpPlnTanOneCylinderRefplaneData_c";
    let root = 7;
    let mut payload = vec![0xaa; root];
    payload.extend(CLASS_MARKER);
    payload.extend((CLASS.len() as u16).to_le_bytes());
    payload.extend(CLASS.as_bytes());
    let body = payload.len();
    payload.resize(body + FIXED_REFERENCE_PLANE_FRAME_LEN, 0);
    for (relative, value) in [
        (0, 0.0125_f64),
        (24, 1.0),
        (49, 0.0),
        (57, 0.0),
        (65, 1.0),
        (73, 0.0),
        (81, 1.0),
        (89, 0.0),
    ] {
        payload[body + relative..body + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[body + 48] = 1;

    assert_eq!(
        constraint_reference_plane_frame(&payload, root, CLASS),
        Some((
            Point3::new(12.5, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );
    assert_eq!(
        constraint_reference_plane_frame(&payload, root, "moRefPlane_c"),
        None
    );
}


#[test]
fn offset_plane_face_reference_owns_a_fixed_plane_frame() {
    const CLASS: &str = "moFaceRefPlnData_c";
    let root = 11;
    let mut payload = vec![0xaa; root];
    payload.extend(CLASS_MARKER);
    payload.extend((CLASS.len() as u16).to_le_bytes());
    payload.extend(CLASS.as_bytes());
    let body = payload.len();
    payload.resize(body + FIXED_REFERENCE_PLANE_FRAME_LEN, 0);
    for (relative, value) in [(0, 0.0025_f64), (24, 1.0), (57, 1.0), (89, 1.0)] {
        payload[body + relative..body + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[body + 48] = 1;

    assert_eq!(
        constraint_reference_plane_frame(&payload, root, CLASS),
        Some((
            Point3::new(2.5, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ))
    );
}


#[test]
fn offset_plane_reference_matches_parallel_frame_at_declared_distance() {
    let reference = (
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    );
    let offset = (
        Point3::new(0.0, 0.0, 6.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    );
    assert!(offset_plane_reference_frame_matches(reference, offset, 6.0));
    assert!(!offset_plane_reference_frame_matches(
        reference, offset, 5.0
    ));
    assert!(!offset_plane_reference_frame_matches(
        reference,
        (Point3::new(1.0, 0.0, 6.0), offset.1, offset.2,),
        6.0,
    ));
}


#[test]
fn constraint_midplane_uses_its_normal_form_equation() {
    const CLASS: &str = "moConstraintMidPlaneRefplaneData_c";
    let mut payload = vec![0xaa; 19];
    payload.extend(CLASS_MARKER);
    payload.extend((CLASS.len() as u16).to_le_bytes());
    payload.extend(CLASS.as_bytes());
    payload.extend([0; 8]);
    payload.extend(1.0e-16f64.to_le_bytes());
    payload.extend(0.145f64.to_le_bytes());
    payload.extend(0.0f64.to_le_bytes());
    payload.extend(0.0f64.to_le_bytes());
    payload.extend(1.0f64.to_le_bytes());
    assert_eq!(
        constraint_midplane_frame(&payload),
        Some((
            Point3::new(0.0, 0.0, 145.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ))
    );

    let normal = payload.len() - 24;
    payload[normal..normal + 8].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(constraint_midplane_frame(&payload), None);
}


#[test]
fn explicit_plane_basis_precedes_equivalent_constraint_orientation() {
    let explicit = (
        Point3::new(12.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    );
    let equivalent_constraint = (
        Point3::new(12.0, 4.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    );
    assert_eq!(
        reconcile_reference_plane_frame(Some(explicit), Some(equivalent_constraint)),
        Some(explicit)
    );

    let conflicting_constraint = (
        Point3::new(13.0, 0.0, 0.0),
        equivalent_constraint.1,
        equivalent_constraint.2,
    );
    assert_eq!(
        reconcile_reference_plane_frame(Some(explicit), Some(conflicting_constraint)),
        Some(conflicting_constraint)
    );
}


#[test]
fn angled_reference_plane_requires_its_redundant_normal_and_basis() {
    let root = 11;
    let mut payload = vec![0; root + 121];
    let inverse_sqrt_two = std::f64::consts::FRAC_1_SQRT_2;
    for (relative, value) in [
        (0, inverse_sqrt_two),
        (8, inverse_sqrt_two),
        (17, 1.0),
        (25, 0.0),
        (33, 0.0),
        (41, 0.0),
        (49, inverse_sqrt_two),
        (57, inverse_sqrt_two),
        (65, 0.0),
        (73, -inverse_sqrt_two),
        (81, inverse_sqrt_two),
        (113, 1.0),
    ] {
        payload[root + relative..root + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[root + 16] = 1;
    assert_eq!(
        angled_reference_plane_frame(&payload),
        Some((
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, inverse_sqrt_two, inverse_sqrt_two),
            Vector3::new(1.0, 0.0, 0.0),
        ))
    );

    payload[root + 8..root + 16].copy_from_slice(&(-inverse_sqrt_two).to_le_bytes());
    assert_eq!(angled_reference_plane_frame(&payload), None);
}


#[test]
fn angled_reference_plane_does_not_reinterpret_a_complete_fixed_frame() {
    let mut payload = vec![0; 153];
    for (offset, value) in [
        (24, 0.0_f64),
        (32, -1.0),
        (40, 0.0),
        (49, -1.0),
        (57, 0.0),
        (65, 0.0),
        (73, 0.0),
        (81, 0.0),
        (89, -1.0),
        (97, 0.0),
        (105, -1.0),
        (113, 0.0),
        (145, 1.0),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[48] = 1;
    assert!(fixed_reference_plane_frame(&payload[..97]).is_some());
    assert_eq!(angled_reference_plane_frame(&payload), None);
}


#[test]
fn matrix_reference_plane_uses_basis_columns() {
    let root = 9;
    let mut payload = vec![0; root + 121];
    let sine = 0.390_731_128_489_273_27_f64;
    let cosine = 0.920_504_853_452_440_5_f64;
    for (relative, value) in [
        (0, 0.008_400_719_262_519_38),
        (8, 0.019_790_854_349_227_484),
        (16, 0.0),
        (24, sine),
        (32, cosine),
        (40, 0.0),
        (49, cosine),
        (57, 0.0),
        (65, sine),
        (73, -sine),
        (81, 0.0),
        (89, cosine),
        (97, 0.0),
        (105, -1.0),
        (113, 0.0),
    ] {
        payload[root + relative..root + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[root + 48] = 1;
    assert_eq!(
        matrix_reference_plane_frame(&payload),
        Some((
            Point3::new(
                0.008_400_719_262_519_38 * 1000.0,
                0.019_790_854_349_227_484 * 1000.0,
                0.0,
            ),
            Vector3::new(sine, cosine, 0.0),
            Vector3::new(cosine, -sine, 0.0),
        ))
    );

    payload[root + 113..root + 121].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(matrix_reference_plane_frame(&payload), None);
}


#[test]
fn complete_reference_plane_frames_precede_compact_byte_patterns() {
    let mut payload = vec![0; 260];
    let matrix = 3;
    for (relative, value) in [
        (0, 0.035_f64),
        (8, 0.0),
        (16, 0.0),
        (24, 1.0),
        (32, 0.0),
        (40, 0.0),
        (49, 0.0),
        (57, 0.0),
        (65, 1.0),
        (73, 0.0),
        (81, 1.0),
        (89, 0.0),
        (97, -1.0),
        (105, 0.0),
        (113, 0.0),
    ] {
        payload[matrix + relative..matrix + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[matrix + 48] = 1;

    let compact = 165;
    for (relative, value) in [
        (0, 0.0_f64),
        (8, 0.0),
        (16, 0.0),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (48, 0.0),
        (56, 0.0),
        (65, 0.0),
        (73, 1.0),
    ] {
        payload[compact + relative..compact + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[compact + 64] = 0;
    payload[compact + 81] = 0;

    assert!(compact_reference_plane_frame(&payload).is_some());
    assert_eq!(
        explicit_reference_plane_frame(&payload),
        Ok(Some((
            Point3::new(35.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        )))
    );
}


#[test]
fn minimal_reference_plane_validates_its_redundant_offset_tail() {
    let root = 13;
    let mut payload = vec![0; root + 81];
    let distance = -0.052_f64;
    for (relative, value) in [
        (0, 0.0_f64),
        (8, 0.0),
        (16, distance),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (57, -0.0),
        (65, -distance),
        (73, 1.0),
    ] {
        payload[root + relative..root + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[root + 56] = 0x80;
    assert_eq!(
        minimal_reference_plane_frame(&payload),
        Some((
            Point3::new(0.0, 0.0, -52.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ))
    );

    payload[root + 65..root + 73].copy_from_slice(&0.051f64.to_le_bytes());
    assert_eq!(minimal_reference_plane_frame(&payload), None);
}


#[test]
fn compact_reference_plane_solves_omitted_basis_components() {
    let root = 7;
    let mut payload = vec![0xaa; root + 82];
    for (relative, value) in [
        (0, 0.001_f64),
        (8, -0.002),
        (16, 0.003),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (48, 0.0),
        (56, 0.0),
        (65, 0.0),
        (73, 1.0),
    ] {
        payload[root + relative..root + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[root + 64] = 0;
    payload[root + 81] = 0;
    assert_eq!(
        compact_reference_plane_frame(&payload),
        Some((
            Point3::new(1.0, -2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ))
    );

    payload[root + 73..root + 81].copy_from_slice(&0.5f64.to_le_bytes());
    assert_eq!(compact_reference_plane_frame(&payload), None);
}


#[test]
fn marker_local_id_is_the_trailing_u32() {
    let mut payload = vec![0; 92];
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[88..92].copy_from_slice(&37u32.to_le_bytes());
    assert_eq!(marker_local_id(&payload, 0), Some(37));
    payload[88..92].fill(0xff);
    assert_eq!(marker_local_id(&payload, 0), None);
}


#[test]
fn marker_object_index_precedes_the_marker() {
    let mut payload = 37u32.to_le_bytes().to_vec();
    payload.extend(super::SKETCH_MARKER);
    assert_eq!(marker_object_index(&payload, 4), Some(37));
    assert_eq!(marker_object_index(&payload, 3), None);
    payload[0..4].fill(0xff);
    assert_eq!(marker_object_index(&payload, 4), None);
}


#[test]
fn compact_body_states_require_a_duplicated_local_identity() {
    let token = 0x89a4u16;
    let mut payload = vec![0; 180];
    let header = &mut payload[12..95];
    header[0..2].copy_from_slice(&token.to_le_bytes());
    header[2..11].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]);
    header[11..15].copy_from_slice(&205u32.to_le_bytes());
    header[15..19].copy_from_slice(&205u32.to_le_bytes());
    header[47..63].fill(0xff);

    assert_eq!(compact_body_state_ids(&payload, 0, 180, token), [205]);

    payload[12 + 15..12 + 19].copy_from_slice(&206u32.to_le_bytes());
    assert!(compact_body_state_ids(&payload, 0, 180, token).is_empty());
}


#[test]
fn compact_body_retention_mode_follows_the_state_roster() {
    use cadmpeg_ir::features::BodyRetentionMode::{DeleteSelected, KeepSelected};

    let token = 0x89a4u16;
    let mut payload = vec![0; 112];
    let header = &mut payload[12..95];
    header[0..2].copy_from_slice(&token.to_le_bytes());
    header[2..11].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]);
    header[11..15].copy_from_slice(&205u32.to_le_bytes());
    header[15..19].copy_from_slice(&205u32.to_le_bytes());
    header[47..63].fill(0xff);
    payload[95..97].copy_from_slice(&[0x30, 0x80]);

    assert_eq!(
        compact_body_retention_mode(&payload, 0, payload.len(), token),
        Some(KeepSelected)
    );
    payload[97..101].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        compact_body_retention_mode(&payload, 0, payload.len(), token),
        Some(DeleteSelected)
    );
    payload[101] = 1;
    assert_eq!(
        compact_body_retention_mode(&payload, 0, payload.len(), token),
        None
    );
}


#[test]
fn compact_line_region_is_an_ordered_one_based_curve_roster() {
    let mut payload = b"moSketchRegion_c".to_vec();
    payload.extend(0x8060u16.to_le_bytes());
    payload.extend(4u16.to_le_bytes());
    for address in [2u16, 1, 4, 3] {
        payload.extend(0x80e1u16.to_le_bytes());
        payload.extend(address.to_le_bytes());
        payload.extend([0xff; 4]);
        payload.extend([0; 4]);
    }
    assert_eq!(
        compact_line_region_addresses(&payload),
        Some(vec![2, 1, 4, 3])
    );
    payload[22] = 1;
    assert_eq!(compact_line_region_addresses(&payload), None);
}


#[test]
fn compact_line_chain_is_an_ordered_one_based_vertex_roster() {
    let mut payload = Vec::new();
    payload.extend(4u16.to_le_bytes());
    for address in [3u32, 2, 1, 4] {
        payload.extend(address.to_le_bytes());
    }
    payload.extend(1u32.to_le_bytes());
    payload.extend(0u16.to_le_bytes());
    payload.extend(6u32.to_le_bytes());
    payload.extend([0xff; 4]);
    payload.extend([0; 8]);
    payload.extend(5u32.to_le_bytes());
    payload.extend(5u32.to_le_bytes());
    payload.extend([0xff, 0xfe, 0xff, 0, 0, 0]);
    payload.extend([0xff; 4]);
    assert_eq!(
        compact_line_chain_addresses(&payload),
        Some(vec![3, 2, 1, 4])
    );
    payload[24] = 4;
    assert_eq!(compact_line_chain_addresses(&payload), None);
}


#[test]
fn compact_rectangle_requires_each_axis_corner_exactly_once() {
    let corners = [
        Point2::new(25.75, 14.15),
        Point2::new(-25.75, -14.15),
        Point2::new(-25.75, 14.15),
        Point2::new(25.75, -14.15),
    ];
    assert_eq!(
        ordered_rectangle_corners(&corners),
        Some([
            Point2::new(-25.75, -14.15),
            Point2::new(25.75, -14.15),
            Point2::new(25.75, 14.15),
            Point2::new(-25.75, 14.15),
        ])
    );

    let duplicate = [corners[0], corners[0], corners[2], corners[3]];
    assert_eq!(ordered_rectangle_corners(&duplicate), None);
    let non_rectangular = [
        corners[0],
        corners[1],
        corners[2],
        Point2::new(24.0, -14.15),
    ];
    assert_eq!(ordered_rectangle_corners(&non_rectangular), None);
}


#[test]
fn indexed_line_cycle_carries_rectangle_from_known_vertices() {
    const CURVE_START: usize = 400;
    let mut payload = vec![0; CURVE_START + 4 * 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    let edges = [[0u16, 2u16], [0, 3], [3, 1], [2, 1]];
    for (index, edge) in edges.into_iter().enumerate() {
        let offset = CURVE_START + index * 84;
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 29..offset + 31].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 31..offset + 35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&edge[0].to_le_bytes());
        payload[offset + 58..offset + 60].copy_from_slice(&edge[1].to_le_bytes());
        payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    }
    payload[CURVE_START + 3 * 84 + 74..CURVE_START + 3 * 84 + 76]
        .copy_from_slice(&2u16.to_le_bytes());
    payload[CURVE_START + 4 * 84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let marker = |id: &str,
                  offset: u64,
                  object_index: Option<u32>,
                  coordinates_m: Option<[f64; 2]>,
                  kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [
        marker(
            "first",
            0,
            None,
            Some([-0.025, -0.011]),
            SketchInputKind::Point,
        ),
        marker(
            "opposite",
            100,
            None,
            Some([0.025, 0.011]),
            SketchInputKind::Point,
        ),
        marker("third", 200, None, None, SketchInputKind::Point),
        marker("fourth", 300, None, None, SketchInputKind::Point),
        marker(
            "line-1",
            CURVE_START as u64,
            None,
            None,
            SketchInputKind::LineOrCircle,
        ),
        marker(
            "line-2",
            (CURVE_START + 84) as u64,
            None,
            None,
            SketchInputKind::LineOrCircle,
        ),
        marker(
            "line-3",
            (CURVE_START + 168) as u64,
            None,
            None,
            SketchInputKind::LineOrCircle,
        ),
        marker(
            "line-4",
            (CURVE_START + 252) as u64,
            None,
            None,
            SketchInputKind::LineOrCircle,
        ),
    ];
    let marker_refs = markers.iter().collect::<Vec<_>>();

    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &marker_refs),
        Some([
            Point2::new(-0.025, -0.011),
            Point2::new(0.025, -0.011),
            Point2::new(0.025, 0.011),
            Point2::new(-0.025, 0.011),
        ])
    );
    for index in 0..4 {
        let offset = CURVE_START + index * 84;
        payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
    }
    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &marker_refs),
        Some([
            Point2::new(-0.025, -0.011),
            Point2::new(0.025, -0.011),
            Point2::new(0.025, 0.011),
            Point2::new(-0.025, 0.011),
        ])
    );
    for index in 0..4 {
        let offset = CURVE_START + index * 84;
        payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    }
    let mut adjacent = markers.clone();
    adjacent[1].coordinates_m = None;
    adjacent[2].coordinates_m = Some([0.025, 0.011]);
    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &adjacent.iter().collect::<Vec<_>>(),),
        None
    );

    let mut three_corners = markers;
    three_corners[2].coordinates_m = Some([0.025, -0.011]);
    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &three_corners.iter().collect::<Vec<_>>(),),
        Some([
            Point2::new(-0.025, -0.011),
            Point2::new(0.025, -0.011),
            Point2::new(0.025, 0.011),
            Point2::new(-0.025, 0.011),
        ])
    );
    let mut current_payload = payload.clone();
    for index in 0..=4 {
        let offset = CURVE_START + index * 84;
        current_payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        if index < 4 {
            current_payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
        }
    }
    let mut current_corners = three_corners.clone();
    for (index, marker) in current_corners.iter_mut().take(4).enumerate() {
        marker.object_index = Some(index as u32 + 1);
    }
    for marker in current_corners.iter_mut().skip(4) {
        marker.kind = SketchInputKind::Arc;
    }
    assert_eq!(
        indexed_rectangle_from_line_cycle(
            &current_payload,
            &current_corners.iter().collect::<Vec<_>>(),
        ),
        Some([
            Point2::new(-0.025, -0.011),
            Point2::new(0.025, -0.011),
            Point2::new(0.025, 0.011),
            Point2::new(-0.025, 0.011),
        ])
    );
    three_corners[2].coordinates_m = Some([0.024, -0.010]);
    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &three_corners.iter().collect::<Vec<_>>(),),
        None
    );
    three_corners[0].coordinates_m = Some([0.013, -0.025]);
    three_corners[1].coordinates_m = Some([0.0, -0.03]);
    three_corners[2].coordinates_m = Some([0.01, 0.0]);
    three_corners[3].coordinates_m = Some([0.0, 0.0]);
    for (index, edge) in [[2u16, 4u16], [2, 3], [3, 1], [4, 1]]
        .into_iter()
        .enumerate()
    {
        let offset = CURVE_START + index * 84;
        payload[offset + 56..offset + 58].copy_from_slice(&edge[0].to_le_bytes());
        payload[offset + 58..offset + 60].copy_from_slice(&edge[1].to_le_bytes());
    }
    payload[CURVE_START + 3 * 84 + 72..CURVE_START + 4 * 84].fill(0);
    payload.truncate(CURVE_START + 4 * 84);
    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &three_corners.iter().collect::<Vec<_>>(),),
        Some([
            Point2::new(0.0, -0.03),
            Point2::new(0.01, -0.03),
            Point2::new(0.01, 0.0),
            Point2::new(0.0, 0.0),
        ])
    );

    let mut wide = vec![0; CURVE_START + 4 * 92 + SKETCH_MARKER.len()];
    for (index, edge) in [[2u16, 4u16], [2, 3], [3, 1], [4, 1]]
        .into_iter()
        .enumerate()
    {
        let offset = CURVE_START + index * 92;
        wide[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        wide[offset + 5..offset + 13].fill(0xff);
        wide[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        wide[offset + 17..offset + 21]
            .copy_from_slice(&(if index == 3 { 2u32 } else { 1 }).to_le_bytes());
        wide[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        wide[offset + 29..offset + 31].copy_from_slice(&1u16.to_le_bytes());
        wide[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        wide[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        wide[offset + 64..offset + 66].copy_from_slice(&edge[0].to_le_bytes());
        wide[offset + 66..offset + 68].copy_from_slice(&edge[1].to_le_bytes());
        wide[offset + 68..offset + 72].copy_from_slice(&1u32.to_le_bytes());
        wide[offset + 72..offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
        wide[offset + 84..offset + 88]
            .copy_from_slice(&u32::try_from(index + 1).unwrap().to_le_bytes());
    }
    wide[CURVE_START + 4 * 92..].copy_from_slice(SKETCH_MARKER);
    let mut wide_markers = three_corners.to_vec();
    wide_markers.insert(
        0,
        marker("header", 0, None, None, SketchInputKind::LineOrCircle),
    );
    for (index, (marker, coordinates)) in wide_markers[1..5]
        .iter_mut()
        .zip([[0.01, -0.03], [0.0, -0.03], [0.01, 0.0], [0.0, 0.0]])
        .enumerate()
    {
        marker.offset = u64::try_from(index + 1).unwrap();
        marker.coordinates_m = Some(coordinates);
        marker.kind = SketchInputKind::Point;
    }
    for (index, marker) in wide_markers[5..].iter_mut().enumerate() {
        marker.offset = (CURVE_START + index * 92) as u64;
        marker.kind = if index == 3 {
            SketchInputKind::Arc
        } else {
            SketchInputKind::LineOrCircle
        };
    }
    assert_eq!(
        indexed_rectangle_from_line_cycle(&wide, &wide_markers.iter().collect::<Vec<_>>(),),
        Some([
            Point2::new(0.0, -0.03),
            Point2::new(0.01, -0.03),
            Point2::new(0.01, 0.0),
            Point2::new(0.0, 0.0),
        ])
    );
    let mut three_sides = wide[..CURVE_START + 3 * 92 + SKETCH_MARKER.len()].to_vec();
    for (index, edge) in [[1u16, 2u16], [2, 4], [4, 3]].into_iter().enumerate() {
        let offset = CURVE_START + index * 92;
        three_sides[offset + 64..offset + 66].copy_from_slice(&edge[0].to_le_bytes());
        three_sides[offset + 66..offset + 68].copy_from_slice(&edge[1].to_le_bytes());
    }
    three_sides[CURVE_START + 92 + 23..CURVE_START + 92 + 27]
        .copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    three_sides[CURVE_START + 2 * 92 + 17..CURVE_START + 2 * 92 + 21]
        .copy_from_slice(&2u32.to_le_bytes());
    let mut three_side_markers = wide_markers[..8].to_vec();
    three_side_markers[4].coordinates_m = Some([1.0e-17, 0.0]);
    three_side_markers[7].kind = SketchInputKind::Arc;
    assert_eq!(
        indexed_rectangle_from_line_cycle(
            &three_sides,
            &three_side_markers.iter().collect::<Vec<_>>(),
        ),
        Some([
            Point2::new(0.0, -0.03),
            Point2::new(0.01, -0.03),
            Point2::new(0.01, 0.0),
            Point2::new(0.0, 0.0),
        ])
    );
    three_sides[CURVE_START + 92 + 64..CURVE_START + 92 + 68].copy_from_slice(&[1, 0, 4, 0]);
    assert_eq!(
        indexed_rectangle_from_line_cycle(
            &three_sides,
            &three_side_markers.iter().collect::<Vec<_>>(),
        ),
        None
    );
    wide[CURVE_START + 3 * 92 + 17..CURVE_START + 3 * 92 + 21]
        .copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        indexed_rectangle_from_line_cycle(&wide, &wide_markers.iter().collect::<Vec<_>>(),),
        None
    );
}


#[test]
fn compact_legacy_object_index_cycle_carries_rectangle() {
    const CURVE_START: usize = 400;
    let mut payload = vec![0; CURVE_START + 4 * 68 + LEGACY_SKETCH_MARKER.len()];
    for (index, edge) in [[0u16, 3u16], [0, 2], [2, 1], [3, 1]]
        .into_iter()
        .enumerate()
    {
        let offset = CURVE_START + index * 68;
        payload[offset..offset + LEGACY_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 19..offset + 25]
            .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 25..offset + 27].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 31] = 4;
        payload[offset + 42..offset + 44].copy_from_slice(&edge[0].to_le_bytes());
        payload[offset + 44..offset + 46].copy_from_slice(&edge[1].to_le_bytes());
        payload[offset + 46..offset + 50].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 50..offset + 58].copy_from_slice(&(-1.0f64).to_le_bytes());
    }
    let terminal = CURVE_START + 3 * 68;
    payload.resize(terminal + 116, 0);
    payload[terminal + 58..terminal + 104].fill(0);
    payload[terminal + 104..terminal + 106].copy_from_slice(&4u16.to_le_bytes());
    payload[terminal + 106..terminal + 110].copy_from_slice(CLASS_MARKER);
    payload[terminal + 110..terminal + 112].copy_from_slice(&4u16.to_le_bytes());
    payload[terminal + 112..terminal + 116].copy_from_slice(b"line");
    let marker =
        |id: &str, offset: u64, object_index: u32, coordinates_m: Option<[f64; 2]>, kind| {
            SketchInputEntity {
                id: id.into(),
                parent: "lane".into(),
                feature_ref: Some("feature".into()),
                ordinal: 0,
                offset,
                object_index: Some(object_index),
                local_id: None,
                kind,
                state_value: Some(1.0),
                coordinates_m,
                links: Vec::new(),
                link_selector: None,
            }
        };
    let markers = [
        marker("missing", 0, 1, None, SketchInputKind::Point),
        marker("top-left", 100, 2, Some([0.0, 1.0]), SketchInputKind::Point),
        marker(
            "top-right",
            200,
            3,
            Some([2.0, 1.0]),
            SketchInputKind::Point,
        ),
        marker(
            "bottom-left",
            300,
            4,
            Some([0.0, 0.0]),
            SketchInputKind::Point,
        ),
        marker(
            "line-1",
            CURVE_START as u64,
            1,
            None,
            SketchInputKind::LineOrCircle,
        ),
        marker(
            "line-2",
            (CURVE_START + 68) as u64,
            2,
            None,
            SketchInputKind::LineOrCircle,
        ),
        marker(
            "line-3",
            (CURVE_START + 136) as u64,
            3,
            None,
            SketchInputKind::LineOrCircle,
        ),
        marker(
            "line-4",
            (CURVE_START + 204) as u64,
            4,
            None,
            SketchInputKind::LineOrCircle,
        ),
    ];

    assert_eq!(
        (0..4)
            .map(|index| {
                compact_legacy_code_one_line_endpoint_indices(
                    &payload,
                    CURVE_START + index * 68,
                )
            })
            .collect::<Vec<_>>(),
        [Some([1, 4]), Some([1, 3]), Some([3, 2]), Some([4, 2])]
    );
    assert_eq!(
        compact_legacy_object_line_endpoints(
            &payload,
            &markers[6],
            &markers.iter().collect::<Vec<_>>(),
        )
        .map(|endpoints| [endpoints[0].id.as_str(), endpoints[1].id.as_str()]),
        Some(["top-right", "top-left"])
    );
    payload[terminal + 106] = 0;
    assert_eq!(
        compact_legacy_code_one_line_endpoint_indices(&payload, terminal),
        None
    );
    assert_eq!(
        compact_legacy_rectangle_line_endpoints(&payload, terminal),
        Some([4, 2])
    );
    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &markers.iter().collect::<Vec<_>>()),
        Some([
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(2.0, 1.0),
            Point2::new(0.0, 1.0),
        ])
    );

    let mut geometry_locus = payload;
    for (index, endpoints) in [[0u16, 3u16], [0, 2], [2, 1], [3, 1]]
        .into_iter()
        .enumerate()
    {
        let offset = index * 84;
        geometry_locus[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        geometry_locus[offset + 56..offset + 58].copy_from_slice(&endpoints[0].to_le_bytes());
        geometry_locus[offset + 58..offset + 60].copy_from_slice(&endpoints[1].to_le_bytes());
        geometry_locus[offset + 76..offset + 80]
            .copy_from_slice(&u32::try_from(index + 1).unwrap().to_le_bytes());
        geometry_locus[offset + 80..offset + 84]
            .copy_from_slice(&u32::try_from((index + 1) % 4 + 1).unwrap().to_le_bytes());
    }
    let mut diagonal = markers;
    diagonal[0].kind = SketchInputKind::Point;
    diagonal[0].coordinates_m = Some([0.0, 0.0]);
    diagonal[1].coordinates_m = Some([2.0, 1.0]);
    diagonal[2].coordinates_m = None;
    diagonal[3].coordinates_m = None;
    assert_eq!(
        indexed_rectangle_from_line_cycle(
            &geometry_locus,
            &diagonal.iter().collect::<Vec<_>>(),
        ),
        Some([
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(2.0, 1.0),
            Point2::new(0.0, 1.0),
        ])
    );
}


#[test]
fn current_compact_line_cycle_infers_its_missing_rectangle_corner() {
    let mut payload = vec![0; 4 * 84 + SKETCH_MARKER.len()];
    for (index, endpoints) in [[2u16, 4u16], [2, 3], [3, 1], [4, 1]]
        .into_iter()
        .enumerate()
    {
        let offset = index * 84;
        payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 23..offset + 31]
            .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&endpoints[0].to_le_bytes());
        payload[offset + 58..offset + 60].copy_from_slice(&endpoints[1].to_le_bytes());
        payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    }
    payload[3 * 84 + 74..3 * 84 + 76].copy_from_slice(&2u16.to_le_bytes());
    payload[4 * 84..].copy_from_slice(SKETCH_MARKER);
    let marker =
        |id: &str, offset, object_index, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset,
            object_index,
            local_id: None,
            kind: if coordinates_m.is_some() {
                SketchInputKind::Point
            } else {
                SketchInputKind::LineOrCircle
            },
            state_value: Some(1.0),
            coordinates_m,
            links: Vec::new(),
            link_selector: None,
        };
    let markers = [
        marker("missing", 500, Some(1), None),
        marker("top-right", 510, Some(2), Some([2.0, 1.0])),
        marker("bottom-left", 520, Some(3), Some([0.0, 0.0])),
        marker("bottom-right", 530, Some(4), Some([2.0, 0.0])),
        marker("line-1", 0, Some(1), None),
        marker("line-2", 84, Some(2), None),
        marker("line-3", 168, Some(3), None),
        marker("line-4", 252, Some(4), None),
    ];

    assert_eq!(
        indexed_rectangle_from_line_cycle(&payload, &markers.iter().collect::<Vec<_>>()),
        Some([
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(2.0, 1.0),
            Point2::new(0.0, 1.0),
        ])
    );
}


#[test]
fn legacy_rectangle_diagonal_carries_one_endpoint_and_two_distinct_corner_links() {
    let mut payload = vec![0; 146 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&(-0.025f64).to_le_bytes());
    payload[66..74].copy_from_slice(&(-0.011f64).to_le_bytes());
    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x03, 0x00]);
    payload[78..80].copy_from_slice(&0x80ecu16.to_le_bytes());
    payload[80..82].copy_from_slice(&1u16.to_le_bytes());
    payload[82..86].fill(0xff);
    payload[86..88].copy_from_slice(&0x80ecu16.to_le_bytes());
    payload[88..90].copy_from_slice(&4u16.to_le_bytes());
    payload[90..94].fill(0xff);
    payload[94..100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[136..140].copy_from_slice(&1u32.to_le_bytes());
    payload[142..146].copy_from_slice(&6u32.to_le_bytes());
    payload[146..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let marker = SketchInputEntity {
        id: "diagonal".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };

    assert_eq!(
        legacy_extended_rectangle_diagonal_endpoint(&payload, &marker),
        Some([-0.025, -0.011])
    );
    let mut terminal = payload.clone();
    terminal[136..142].fill(0);
    terminal[142..146].fill(0xff);
    assert_eq!(
        legacy_extended_rectangle_diagonal_endpoint(&terminal, &marker),
        Some([-0.025, -0.011])
    );
    payload[88..90].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_extended_rectangle_diagonal_endpoint(&payload, &marker),
        None
    );
}


#[test]
fn dimensioned_rectangle_selects_one_complete_marker_product() {
    let marker = |id: &str, u, v| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: Some([u, v]),
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [
        marker("center", -0.023, 0.0),
        marker("lower-left", -0.02575, -0.00425),
        marker("upper-right", -0.02025, 0.00425),
        marker("lower-right", -0.02025, -0.00425),
        marker("upper-left", -0.02575, 0.00425),
        marker("axis-top", -0.02575, 0.01415),
        marker("axis-bottom", -0.02575, -0.01415),
        marker("origin", 0.0, 0.0),
    ];
    let marker_refs = markers.iter().collect::<Vec<_>>();
    assert_eq!(
        unique_dimensioned_rectangle_markers(&marker_refs, &[8.5, 5.5])
            .map(|markers| markers.map(|marker| marker.id.as_str())),
        Some(["lower-left", "lower-right", "upper-right", "upper-left"])
    );
    assert_eq!(
        unique_dimensioned_rectangle_markers(&marker_refs, &[8.5]),
        None
    );
    assert_eq!(
        unique_dimensioned_rectangle_markers(&marker_refs, &[28.3, 5.5]),
        None
    );

    let second_rectangle = [
        marker("second-lower-left", 0.010, 0.020),
        marker("second-lower-right", 0.0155, 0.020),
        marker("second-upper-right", 0.0155, 0.0285),
        marker("second-upper-left", 0.010, 0.0285),
    ];
    let ambiguous = marker_refs
        .iter()
        .copied()
        .chain(second_rectangle.iter())
        .collect::<Vec<_>>();
    assert_eq!(
        unique_dimensioned_rectangle_markers(&ambiguous, &[8.5, 5.5]),
        None
    );
}


#[test]
fn compact_line_endpoint_pairs_form_one_oriented_cycle() {
    let marker = SketchInputEntity {
        id: "marker".into(),
        parent: "lane".into(),
        feature_ref: None,
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let point = |u, v| Point2::new(u, v);
    let lines = vec![
        (
            SketchEntityId("top".into()),
            &marker,
            &marker,
            point(0.0, 1.0),
            point(1.0, 1.0),
        ),
        (
            SketchEntityId("bottom".into()),
            &marker,
            &marker,
            point(0.0, 0.0),
            point(1.0, 0.0),
        ),
        (
            SketchEntityId("right".into()),
            &marker,
            &marker,
            point(1.0, 0.0),
            point(1.0, 1.0),
        ),
        (
            SketchEntityId("left".into()),
            &marker,
            &marker,
            point(0.0, 1.0),
            point(0.0, 0.0),
        ),
    ];

    let profile = ordered_compact_line_profile(&lines).expect("closed line cycle");
    assert_eq!(
        profile
            .iter()
            .map(|use_| (use_.entity.0.as_str(), use_.reversed))
            .collect::<Vec<_>>(),
        [
            ("top", false),
            ("right", true),
            ("bottom", true),
            ("left", true)
        ]
    );
    assert_eq!(complete_ordered_compact_line_profile(&lines, 5), None);
}


#[test]
fn compact_reference_plane_source_requires_the_complete_trailer() {
    let mut payload = b"moCompRefPlane_c".to_vec();
    payload.extend([0; 12]);
    let start = payload.len();
    payload.extend(2u32.to_le_bytes());
    payload.extend(0x6554_f1b8_u32.to_le_bytes());
    payload.extend([0, 0, 3, 0]);
    payload.extend([0; 27]);
    payload.extend(1.0f64.to_le_bytes());
    payload.extend([
        0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xf9, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x65,
    ]);
    payload.extend([0; 4]);
    assert_eq!(compact_reference_plane_source(&payload), Some(2));
    payload[start + 50] = 3;
    payload[start + 54] = 0xff;
    assert_eq!(compact_reference_plane_source(&payload), Some(2));
    payload[start + 50] = 1;
    assert_eq!(compact_reference_plane_source(&payload), None);
    payload[start + 50] = 3;
    payload[start + 59] ^= 1;
    assert_eq!(compact_reference_plane_source(&payload), None);
}


#[test]
fn compact_legacy_reference_plane_source_uses_the_embedded_u16_id() {
    let mut payload = b"moCompRefPlane_c".to_vec();
    payload.extend([0; 12]);
    let start = payload.len();
    payload.extend(0x4f96_6817u32.to_le_bytes());
    payload.extend([0; 6]);
    payload.extend(3u16.to_le_bytes());
    payload.extend([0; 27]);
    payload.extend(1.0f64.to_le_bytes());
    payload.extend([
        0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xf9, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x65,
    ]);
    payload.extend([0; 4]);

    assert_eq!(compact_reference_plane_source(&payload), Some(3));
    payload[start + 10..start + 12].fill(0);
    assert_eq!(compact_reference_plane_source(&payload), None);
}


#[test]
fn compact_offset_plane_source_requires_the_reference_record() {
    let mut payload = Vec::new();
    payload.extend(3u32.to_le_bytes());
    payload.extend([
        0x02, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x2d, 0x80, 0x2b, 0x80,
    ]);
    assert_eq!(compact_offset_plane_source(&payload), Some(3));
    payload[19] ^= 1;
    assert_eq!(compact_offset_plane_source(&payload), None);
}


#[test]
fn legacy_offset_plane_face_alias_requires_the_complete_nested_record() {
    let mut body = vec![0; 115];
    body[..2].copy_from_slice(&0x802d_u16.to_le_bytes());
    body[2..6].copy_from_slice(&2u32.to_le_bytes());
    body[45..61].fill(0xff);
    body[69..73].copy_from_slice(&2u32.to_le_bytes());
    body[73..77].copy_from_slice(&0x4c41_ac95_u32.to_le_bytes());
    body[77..83].copy_from_slice(&[0, 0, 3, 0, 0, 0]);
    body[83..87].copy_from_slice(&1u32.to_le_bytes());
    body[91..95].copy_from_slice(&175u32.to_le_bytes());
    body[99..103].copy_from_slice(&3u32.to_le_bytes());
    body[107..115].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);

    assert_eq!(legacy_offset_plane_face_alias(&body), Some((0, 175)));
    body[91..95].fill(0);
    assert_eq!(legacy_offset_plane_face_alias(&body), None);
    body[91..95].copy_from_slice(&175u32.to_le_bytes());
    body[83] = 2;
    assert_eq!(legacy_offset_plane_face_alias(&body), None);
}


#[test]
fn structured_offset_plane_source_requires_repeated_identities_and_terminator() {
    let mut payload = vec![0; 140];
    let header = 0x8323u32.to_le_bytes();
    let identity = [
        0xd7, 0x81, 0x26, 0x03, 0x1d, 0x00, 0x00, 0x00, 0x5e, 0x2c, 0xdb, 0x54,
    ];
    let link = 0x81dcu32.to_le_bytes();
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&header);
    for offset in [8, 32, 52, 76] {
        payload[offset..offset + 12].copy_from_slice(&identity);
    }
    payload[28..32].copy_from_slice(&link);
    payload[44..48].copy_from_slice(&3u32.to_le_bytes());
    payload[48..52].copy_from_slice(&header);
    for offset in [64, 88, 108] {
        payload[offset..offset + 4].copy_from_slice(&1u32.to_le_bytes());
    }
    payload[72..76].copy_from_slice(&link);
    payload[116..120].copy_from_slice(&2600u32.to_le_bytes());
    payload[132..140].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);

    assert_eq!(structured_offset_plane_sources(&payload), [3]);
    payload[80] ^= 1;
    assert!(structured_offset_plane_sources(&payload).is_empty());
}


#[test]
fn classed_offset_plane_source_requires_exact_length_delimited_type() {
    let mut payload = 4u32.to_le_bytes().to_vec();
    payload.extend(b"\xff\xff\x01\x00\x1b\x00moFromSktEnt3IntSurfIdRep_c\x00\x00");

    assert_eq!(classed_offset_plane_sources(&payload), [4]);
    payload[8] = 0;
    assert!(classed_offset_plane_sources(&payload).is_empty());
}


#[test]
fn typed_offset_plane_reference_uses_the_last_known_plane_target() {
    let record = |source: u32, signature: [u8; 4], selector: u32| {
        let mut bytes = Vec::new();
        bytes.extend(source.to_le_bytes());
        bytes.extend(signature);
        bytes.extend([0; 2]);
        bytes.extend(selector.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend([0; 4]);
        bytes.extend(247u32.to_le_bytes());
        bytes.extend([0; 12]);
        bytes.extend([0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
        bytes
    };
    let known = HashSet::from([3, 225]);
    let principal = record(3, [0x43, 0xf6, 0x8a, 0x4d], 3);
    assert_eq!(
        offset_plane_reference_source(&principal, &known, &known, None),
        Some(3)
    );
    let feature = record(225, [0x30, 0x92, 0xab, 0x53], 0);
    assert_eq!(
        offset_plane_reference_source(&feature, &known, &known, None),
        Some(225)
    );
    assert_eq!(
        offset_plane_reference_source(&feature, &known, &known, Some(225)),
        None
    );

    let mut ambiguous = principal;
    ambiguous.extend_from_slice(&feature);
    assert_eq!(
        offset_plane_reference_source(&ambiguous, &known, &known, None),
        Some(225)
    );
    ambiguous[38] ^= 1;
    assert_eq!(
        offset_plane_reference_source(&ambiguous, &known, &known, None),
        Some(225)
    );
    let mut malformed = record(3, [0; 4], 2);
    assert_eq!(
        offset_plane_reference_source(&malformed, &known, &known, None),
        None
    );
    malformed[4..8].copy_from_slice(&[1, 2, 3, 4]);
    malformed[10..14].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        offset_plane_reference_source(&malformed, &known, &known, None),
        Some(3)
    );
    let principal_only = HashSet::from([3]);
    assert_eq!(
        offset_plane_reference_source(&feature, &known, &principal_only, None),
        None
    );
}


#[test]
fn frame_only_offset_plane_reference_prefers_a_unique_principal() {
    assert_eq!(
        select_reference_plane_frame_source(
            [
                ("derived", 20, false),
                ("principal", 80, true),
                ("older", 10, false),
            ]
            .into_iter(),
        ),
        Some("principal".into())
    );
    assert_eq!(
        select_reference_plane_frame_source(
            [("first", 80, true), ("second", 90, true)].into_iter(),
        ),
        None
    );
}


#[test]
fn frame_only_offset_plane_reference_uses_the_latest_matching_feature() {
    assert_eq!(
        select_reference_plane_frame_source(
            [
                ("older", 10, false),
                ("latest", 20, false),
                ("latest", 20, false),
            ]
            .into_iter(),
        ),
        Some("latest".into())
    );
    assert_eq!(
        select_reference_plane_frame_source(
            [("first", 20, false), ("second", 20, false)].into_iter(),
        ),
        None
    );
}


#[test]
fn compact_profile_uses_a_unique_lane_scoped_reference_plane() {
    let mut payload = b"moCompRefPlane_c".to_vec();
    payload.extend([0; 11]);
    payload.extend(2u32.to_le_bytes());
    payload.extend(19u32.to_le_bytes());
    payload.extend([0, 0, 3, 0]);
    payload.extend([0; 27]);
    payload.extend(1.0f64.to_le_bytes());
    payload.extend([
        0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xf9, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x65,
    ]);
    payload.extend([0; 80]);
    let component_start = payload.len();
    let mut component = [0u8; 138];
    component[..4].copy_from_slice(&549u32.to_le_bytes());
    component[14] = 1;
    for (offset, value) in [
        (15, 1.0),
        (23, 0.0),
        (31, 0.0),
        (39, 0.0),
        (47, 1.0),
        (55, 0.0),
        (63, 0.0),
        (71, 0.0),
        (79, 1.0),
    ] {
        component[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    component[122..126].copy_from_slice(&4u32.to_le_bytes());
    component[126..130].fill(0xff);
    payload.extend(component);
    let profile_start = payload.len();
    payload.extend([0xaa; 64]);
    let plane_index = CompactReferencePlaneIndex::new(&payload);

    assert_eq!(
        compact_profile_reference_plane_source(
            &plane_index,
            profile_start,
            profile_start,
            payload.len(),
        ),
        Some(2)
    );
    assert_eq!(
        compact_profile_reference_plane_source(
            &plane_index,
            component_start,
            component_start,
            payload.len(),
        ),
        Some(549)
    );
}


#[test]
fn qualified_operand_falls_back_to_marker_family_ordinal() {
    let markers = [4, 8, 11]
        .into_iter()
        .enumerate()
        .map(|(ordinal, local_id)| SketchInputEntity {
            id: format!("marker-{local_id}"),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: ordinal as u32,
            offset: ordinal as u64,
            object_index: None,
            local_id: Some(local_id),
            kind: SketchInputKind::LineOrCircle,
            state_value: None,
            coordinates_m: None,
            links: Vec::new(),
            link_selector: None,
        })
        .collect::<Vec<_>>();
    let kind = FeatureInputOperandKind::Native(0x8386);
    assert_eq!(
        resolve_operand_marker(&markers, kind, 4).map(|marker| marker.id.as_str()),
        Some("marker-4")
    );
    assert_eq!(
        resolve_operand_marker(&markers, kind, 2).map(|marker| marker.id.as_str()),
        Some("marker-11")
    );
}


#[test]
fn line_distance_operand_selects_a_point_coded_linked_line_handle() {
    let endpoint = |id: &str, local_id, u| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: local_id,
        offset: u64::from(local_id),
        object_index: None,
        local_id: Some(local_id),
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: Some([u, 0.0]),
        links: Vec::new(),
        link_selector: None,
    };
    let endpoints = [endpoint("first", 2, 1.0), endpoint("second", 3, 2.0)];
    let handle = SketchInputEntity {
        id: "line-handle".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 16,
        offset: 16,
        object_index: None,
        local_id: Some(16),
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: Some([9.0, 9.0]),
        links: endpoints
            .iter()
            .map(|endpoint| SketchInputLink {
                local_id: u16::try_from(endpoint.local_id.expect("local identity"))
                    .expect("u16 local identity"),
                entity_ref: endpoint.id.clone(),
            })
            .collect(),
        link_selector: Some(0x8386),
    };
    let markers = [&endpoints[0], &endpoints[1], &handle];

    assert_eq!(
        resolve_operand_marker(markers, FeatureInputOperandKind::Native(0x8386), 16,)
            .map(|marker| marker.id.as_str()),
        Some("line-handle")
    );
    assert!(
        resolve_operand_marker(markers, FeatureInputOperandKind::Native(0x8dda), 16,).is_none()
    );
}


#[test]
fn qualified_operand_selects_one_coordinate_marker_in_a_reused_local_id() {
    let marker = |id: &str, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: Some(7),
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [
        marker("reference", None),
        marker("geometry", Some([1.0, 2.0])),
    ];
    assert_eq!(
        resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0x837b), 7,)
            .map(|marker| marker.id.as_str()),
        Some("geometry")
    );
}


#[test]
fn qualified_point_operand_selects_a_curve_marker_locus() {
    let marker = SketchInputEntity {
        id: "line-locus".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: Some(16),
        kind: SketchInputKind::LineOrCircle,
        state_value: None,
        coordinates_m: Some([1.0, 2.0]),
        links: Vec::new(),
        link_selector: None,
    };
    for tag in [0x837b, 0xbc7c] {
        assert_eq!(
            resolve_operand_marker(
                std::slice::from_ref(&marker),
                FeatureInputOperandKind::Native(tag),
                16,
            )
            .map(|resolved| resolved.id.as_str()),
            Some("line-locus")
        );
    }
    let mut markers = vec![marker];
    markers.extend((0..3).map(|index| SketchInputEntity {
        id: format!("point-{index}"),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: index,
        offset: u64::from(index + 1),
        object_index: None,
        local_id: Some(10 + index),
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: Some([f64::from(index), 0.0]),
        links: Vec::new(),
        link_selector: None,
    }));
    markers[0].local_id = Some(1);
    assert_eq!(
        resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0xbc7c), 1)
            .map(|resolved| resolved.id.as_str()),
        Some("point-1")
    );
}


#[test]
fn object_indexed_bc_operands_precede_local_and_ordinal_fallbacks() {
    let marker = |id: &str, offset, object_index, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: offset as u32,
        offset,
        object_index,
        local_id: Some(100 + offset as u32),
        kind,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [
        marker(
            "unrelated-point",
            0,
            Some(3),
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        marker(
            "indexed-curve-locus",
            1,
            Some(0),
            SketchInputKind::LineOrCircle,
            Some([1.0, 0.0]),
        ),
        marker(
            "indexed-relation",
            2,
            Some(0),
            SketchInputKind::Relation(SketchRelationKind::Distance),
            None,
        ),
        SketchInputEntity {
            local_id: Some(0),
            ..marker(
                "local-id-curve",
                3,
                Some(2),
                SketchInputKind::LineOrCircle,
                Some([2.0, 0.0]),
            )
        },
    ];

    assert_eq!(
        resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0xbc7c), 0)
            .map(|marker| marker.id.as_str()),
        Some("indexed-curve-locus")
    );
    assert_eq!(
        resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0xbc87), 0)
            .map(|marker| marker.id.as_str()),
        Some("indexed-curve-locus")
    );
}


#[test]
fn point_operand_follows_relation_handle_graph_and_excludes_its_sibling() {
    let marker = |id: &str, local_id, kind, links: &[&str]| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id,
        kind,
        state_value: None,
        coordinates_m: None,
        links: links
            .iter()
            .map(|target| SketchInputLink {
                local_id: 0,
                entity_ref: (*target).into(),
            })
            .collect(),
        link_selector: None,
    };
    let markers = [
        marker("first", Some(5), SketchInputKind::Point, &[]),
        marker("second", Some(1), SketchInputKind::Point, &[]),
        marker(
            "relation-2",
            Some(2),
            SketchInputKind::Relation(SketchRelationKind::Distance),
            &["relation-0"],
        ),
        marker(
            "relation-0",
            Some(0),
            SketchInputKind::Relation(SketchRelationKind::Distance),
            &["second"],
        ),
    ];
    let operands = [
        FeatureInputOperand {
            kind: FeatureInputOperandKind::D6,
            entity_index: 0,
            offset: 0,
            reference_ref: "first-ref".into(),
            entity_ref: None,
        },
        FeatureInputOperand {
            kind: FeatureInputOperandKind::D6,
            entity_index: 2,
            offset: 0,
            reference_ref: "second-ref".into(),
            entity_ref: None,
        },
    ];
    let resolved = resolve_scalar_operand_markers(&markers, &operands);
    assert_eq!(resolved[0].map(|marker| marker.id.as_str()), Some("first"));
    assert_eq!(resolved[1].map(|marker| marker.id.as_str()), Some("second"));

    let duplicate = [
        operands[1].clone(),
        FeatureInputOperand {
            kind: FeatureInputOperandKind::D6,
            entity_index: 1,
            offset: 0,
            reference_ref: "known-second-ref".into(),
            entity_ref: None,
        },
    ];
    let resolved = resolve_scalar_operand_markers(&markers, &duplicate);
    assert_eq!(resolved[0].map(|marker| marker.id.as_str()), Some("first"));
    assert_eq!(resolved[1].map(|marker| marker.id.as_str()), Some("second"));
}


#[test]
fn curve_operand_selects_an_arc_by_local_identifier() {
    let markers = [
        SketchInputEntity {
            id: "line-11".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: 0,
            object_index: None,
            local_id: Some(11),
            kind: SketchInputKind::LineOrCircle,
            state_value: None,
            coordinates_m: Some([0.0, 0.0]),
            links: Vec::new(),
            link_selector: None,
        },
        SketchInputEntity {
            id: "arc-3".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 1,
            offset: 1,
            object_index: None,
            local_id: Some(3),
            kind: SketchInputKind::Arc,
            state_value: None,
            coordinates_m: Some([1.0, 1.0]),
            links: Vec::new(),
            link_selector: None,
        },
    ];
    assert_eq!(
        resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0x8dda), 3,)
            .map(|marker| marker.id.as_str()),
        Some("arc-3")
    );
}


#[test]
fn curve_operand_follows_a_unique_local_reference_handle() {
    let markers = [
        SketchInputEntity {
            id: "line-11".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: 0,
            object_index: None,
            local_id: Some(11),
            kind: SketchInputKind::LineOrCircle,
            state_value: None,
            coordinates_m: Some([0.0, 0.0]),
            links: Vec::new(),
            link_selector: None,
        },
        SketchInputEntity {
            id: "arc-8".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 1,
            offset: 1,
            object_index: None,
            local_id: Some(8),
            kind: SketchInputKind::Arc,
            state_value: None,
            coordinates_m: Some([1.0, 1.0]),
            links: Vec::new(),
            link_selector: None,
        },
        SketchInputEntity {
            id: "reference-3".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 2,
            offset: 2,
            object_index: None,
            local_id: Some(3),
            kind: SketchInputKind::Relation(SketchRelationKind::Angle),
            state_value: None,
            coordinates_m: None,
            links: vec![crate::records::SketchInputLink {
                local_id: 8,
                entity_ref: "arc-8".into(),
            }],
            link_selector: Some(0),
        },
    ];
    assert_eq!(
        resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0x8dda), 3,)
            .map(|marker| marker.id.as_str()),
        Some("arc-8")
    );
}


#[test]
fn curve_operand_excludes_an_already_resolved_sibling_from_a_reference_handle() {
    let curve = |id: &str, local_id, offset| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: offset as u32,
        offset,
        object_index: None,
        local_id: Some(local_id),
        kind: SketchInputKind::LineOrCircle,
        state_value: None,
        coordinates_m: Some([offset as f64, 0.0]),
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [
        curve("curve-7", 7, 0),
        curve("curve-5", 5, 1),
        SketchInputEntity {
            id: "reference-10".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 2,
            offset: 2,
            object_index: None,
            local_id: Some(10),
            kind: SketchInputKind::Relation(SketchRelationKind::Distance),
            state_value: None,
            coordinates_m: None,
            links: vec![
                crate::records::SketchInputLink {
                    local_id: 7,
                    entity_ref: "curve-7".into(),
                },
                crate::records::SketchInputLink {
                    local_id: 5,
                    entity_ref: "curve-5".into(),
                },
            ],
            link_selector: Some(0),
        },
    ];
    assert!(
        resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0x8386), 10).is_none()
    );
    assert_eq!(
        resolve_operand_marker_excluding(
            &markers,
            FeatureInputOperandKind::Native(0x8386),
            10,
            &HashSet::from(["curve-7".into()]),
        )
        .map(|marker| marker.id.as_str()),
        Some("curve-5")
    );
}


#[test]
fn exact_local_operand_excludes_an_already_resolved_sibling() {
    let point = |id: &str, offset| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: offset as u32,
        offset,
        object_index: None,
        local_id: Some(3),
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m: Some([offset as f64, 0.0]),
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [point("first", 0), point("second", 1)];
    assert_eq!(
        resolve_operand_marker_excluding(
            &markers,
            FeatureInputOperandKind::Native(0xbc7c),
            3,
            &HashSet::from(["first".into()]),
        )
        .map(|marker| marker.id.as_str()),
        Some("second")
    );
}


#[test]
fn generated_arc_angles_use_only_exact_native_quadrants() {
    assert_eq!(
        arc_angle_relation_kind(std::f64::consts::FRAC_PI_2),
        Some(SketchRelationKind::ArcAngle90)
    );
    assert_eq!(
        arc_angle_relation_kind(std::f64::consts::PI),
        Some(SketchRelationKind::ArcAngle180)
    );
    assert_eq!(
        arc_angle_relation_kind(3.0 * std::f64::consts::FRAC_PI_2),
        Some(SketchRelationKind::ArcAngle270)
    );
    assert_eq!(arc_angle_relation_kind(std::f64::consts::FRAC_PI_3), None);
}


#[test]
fn compact_extrusion_through_all_requires_the_complete_end_spec() {
    let mut payload = vec![0; 104];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 1;
    payload[30..34].copy_from_slice(&[1, 0, 0, 1]);
    payload[92] = 1;
    assert!(compact_extrusion_through_all_at(&payload, 0));
    payload[8] = 1;
    assert!(compact_extrusion_through_all_at(&payload, 0));
    payload[8] = 0;

    payload[18] = 0;
    assert!(!compact_extrusion_through_all_at(&payload, 0));
    payload[18] = 1;
    payload[103] = 1;
    assert!(!compact_extrusion_through_all_at(&payload, 0));

    let declaration = b"\xff\xff\x01\x00\x0b\x00moEndSpec_c";
    let mut direct = vec![0; declaration.len() + 102];
    direct[..declaration.len()].copy_from_slice(declaration);
    let body = declaration.len();
    direct[body + 2..body + 6].copy_from_slice(&1u32.to_le_bytes());
    direct[body + 16..body + 20].copy_from_slice(&1u32.to_le_bytes());
    direct[body + 28..body + 32].copy_from_slice(&[1, 0, 0, 1]);
    direct[body + 88..body + 92].copy_from_slice(&[0, 0, 1, 0]);
    direct[body + 98..body + 102].copy_from_slice(&[0xff, 0xff, 1, 0]);
    assert!(compact_extrusion_through_all_at(&direct, body - 2));

    direct[body + 6..body + 10].copy_from_slice(&1u32.to_le_bytes());
    assert!(compact_extrusion_through_all_at(&direct, body - 2));
}


#[test]
fn compact_extrusion_to_face_requires_a_single_face_reference_child() {
    let mut payload = vec![0; 200];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 4;
    payload[30..33].copy_from_slice(&[1, 1, 0]);
    payload[33..35].copy_from_slice(&[0x7f, 0x9d]);
    payload[35..46].copy_from_slice(&[0x2d, 0x80, 0x2b, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
    payload[88..92].copy_from_slice(&1u32.to_le_bytes());
    payload[92..96].copy_from_slice(&[0, 2, 0, 0]);
    payload[100..116].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload[118..122].copy_from_slice(&[0x32, 0x80, 0, 0]);
    payload[122..134].fill(1);
    payload[134..138].copy_from_slice(&7u32.to_le_bytes());
    assert_eq!(compact_extrusion_to_face_at(&payload, 0), Some(100));
    let path = compact_single_face_reference_path_at(&payload, 100).expect("required invariant");
    assert_eq!(path.len(), 1);
    assert_eq!(path[0].instance, Some(0x8032));
    assert_eq!(path[0].type_signature, [1; 12]);
    assert_eq!(path[0].local_id, Some(7));

    payload[35..39].copy_from_slice(&[0xe4, 0x82, 0x07, 0x81]);
    assert_eq!(compact_extrusion_to_face_at(&payload, 0), Some(100));
    payload[37..39].copy_from_slice(&[0xff, 0xff]);
    assert_eq!(compact_extrusion_to_face_at(&payload, 0), None);
    payload[37..39].copy_from_slice(&[0x07, 0x81]);

    payload[88..92].copy_from_slice(&2u32.to_le_bytes());
    payload[138..158].fill(0);
    payload[158..162].copy_from_slice(&101u32.to_le_bytes());
    let (path, terminal_source) =
        compact_single_face_reference_record_at(&payload, 100).expect("required invariant");
    assert_eq!(path.len(), 1);
    assert_eq!(terminal_source, Some(101));

    payload[88..92].copy_from_slice(&3u32.to_le_bytes());
    payload[138..142].fill(0);
    payload[142..146].copy_from_slice(&[0xf5, 0x81, 0, 0]);
    payload[146..158]
        .copy_from_slice(&[0xf0, 0x81, 0x4d, 2, 0xd6, 0, 0, 0, 0x4d, 0xb8, 0xb0, 0x59]);
    payload[158..162].copy_from_slice(&9u32.to_le_bytes());
    payload[162..186].fill(0);
    payload[186..190].copy_from_slice(&101u32.to_le_bytes());
    let (path, terminal_source) =
        compact_single_face_reference_record_at(&payload, 100).expect("required invariant");
    assert_eq!(path.len(), 2);
    assert_eq!(path[1].local_id, Some(9));
    assert_eq!(terminal_source, Some(101));
    payload[88..92].copy_from_slice(&1u32.to_le_bytes());

    payload[12] = 1;
    payload[22] = 1;
    assert_eq!(compact_extrusion_to_face_at(&payload, 0), Some(100));

    payload[88..92].fill(0);
    assert_eq!(compact_extrusion_to_face_at(&payload, 0), None);
}


#[test]
fn compact_extrusion_to_face_accepts_root_adjusted_component_paths() {
    fn payload(flag: u8, count: u32) -> Vec<u8> {
        let mut payload = vec![0; 260];
        payload[..2].copy_from_slice(&[0x0c, 0x8e]);
        payload[4] = 1;
        payload[18] = 4;
        payload[30..33].copy_from_slice(&[1, 1, 0]);
        payload[33..35].copy_from_slice(&[0x7f, 0x9d]);
        payload[35..46].copy_from_slice(&[0x2d, 0x80, 0x2b, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
        payload[88..92].copy_from_slice(&count.to_le_bytes());
        payload[92..96].copy_from_slice(&[0, flag, 0, 0]);
        payload[100..116].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        payload
    }
    fn entry(payload: &mut [u8], offset: usize, token: u16, signature: u8, local_id: u32) {
        payload[offset..offset + 2].copy_from_slice(&token.to_le_bytes());
        payload[offset + 4..offset + 16].fill(signature);
        payload[offset + 16..offset + 20].copy_from_slice(&local_id.to_le_bytes());
    }

    let mut slotted = payload(3, 3);
    entry(&mut slotted, 118, 0x8049, 1, 0);
    slotted[138..142].copy_from_slice(&34u32.to_le_bytes());
    entry(&mut slotted, 142, 0x8034, 2, 24);
    slotted[162..182].fill(0);
    slotted[182..186].copy_from_slice(&101u32.to_le_bytes());
    let path = compact_single_face_reference_path_at(&slotted, 100).expect("required invariant");
    assert_eq!(path.len(), 2);
    assert_eq!(path[1].instance, Some(0x8034));
    assert_eq!(compact_extrusion_to_face_at(&slotted, 0), Some(100));

    let mut aligned = payload(2, 5);
    entry(&mut aligned, 118, 0x8633, 1, 1);
    entry(&mut aligned, 146, 0x830d, 2, 1);
    aligned[166..176].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0]);
    entry(&mut aligned, 176, 0x830d, 3, 1);
    aligned[196..204].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    let path = compact_single_face_reference_path_at(&aligned, 100).expect("required invariant");
    assert_eq!(path.len(), 3);
    assert_eq!(path[2].type_signature, [3; 12]);
    assert_eq!(compact_extrusion_to_face_at(&aligned, 0), Some(100));
}


#[test]
fn compact_extrusion_to_face_accepts_the_legacy_end_spec_token() {
    let mut payload = vec![0; 200];
    payload[..2].copy_from_slice(&[3, 0]);
    payload[4] = 1;
    payload[18] = 4;
    payload[30..33].copy_from_slice(&[1, 1, 0]);
    payload[33..35].copy_from_slice(&[0x7f, 0x9d]);
    payload[35..46].copy_from_slice(&[0x2d, 0x80, 0x2b, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
    payload[88..92].copy_from_slice(&1u32.to_le_bytes());
    payload[92..96].copy_from_slice(&[0, 2, 0, 0]);
    payload[100..116].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload[118..122].copy_from_slice(&[0x32, 0x80, 0, 0]);
    payload[122..134].fill(1);
    payload[134..138].copy_from_slice(&7u32.to_le_bytes());

    assert_eq!(compact_extrusion_to_face_at(&payload, 0), Some(100));
    payload[0] = 2;
    assert_eq!(compact_extrusion_to_face_at(&payload, 0), None);
}


#[test]
fn compact_extrusion_to_face_accepts_a_declared_width_two_child() {
    let mut payload = vec![0; 240];
    payload[..2].copy_from_slice(&[0x09, 0x81]);
    payload[4] = 1;
    payload[18] = 4;
    payload[30..33].copy_from_slice(&[1, 1, 0]);
    let declaration = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    payload[33..33 + declaration.len()].copy_from_slice(declaration);
    let body = 33 + declaration.len();
    payload[body..body + 14]
        .copy_from_slice(&[0x7e, 0x81, 0x1f, 0x82, 2, 0, 0x22, 2, 0x4a, 2, 0, 0, 4, 0]);
    let marker = 160;
    payload[marker - 12..marker - 8].copy_from_slice(&1u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload[marker + 18..marker + 22].copy_from_slice(&[0x32, 0x80, 0, 0]);
    payload[marker + 22..marker + 34].fill(1);
    payload[marker + 34..marker + 38].copy_from_slice(&7u32.to_le_bytes());

    assert_eq!(compact_extrusion_to_face_at(&payload, 0), Some(marker));
    payload[33] = 0;
    assert_eq!(compact_extrusion_to_face_at(&payload, 0), None);
}


#[test]
fn compact_extrusion_to_face_preserves_an_unparsed_declared_face_child() {
    let end_spec = b"\xff\xff\x01\x00\x0b\x00moEndSpec_c";
    let face_ref = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    let mut payload = vec![0; 180];
    payload[..end_spec.len()].copy_from_slice(end_spec);
    let anchor = end_spec.len() - 2;
    payload[anchor + 4..anchor + 8].copy_from_slice(&1u32.to_le_bytes());
    payload[anchor + 18..anchor + 22].copy_from_slice(&4u32.to_le_bytes());
    payload[anchor + 30..anchor + 33].copy_from_slice(&[1, 1, 0]);
    let child = anchor + 33;
    payload[child..child + face_ref.len()].copy_from_slice(face_ref);
    let body = child + face_ref.len();
    payload[body..body + 18].copy_from_slice(&[
        0x18, 0x81, 0xca, 0x80, 2, 0, 0xcc, 0x80, 0, 0, 0xce, 0x80, 1, 0, 0, 0, 0xd0, 0x80,
    ]);

    assert_eq!(compact_extrusion_to_face_at(&payload, anchor), Some(body));

    let mut lane_token = payload[anchor..].to_vec();
    lane_token[..2].copy_from_slice(&[0x0c, 0x8e]);
    assert_eq!(
        compact_extrusion_to_face_at(&lane_token, 0),
        Some(body - anchor)
    );

    let comp_face = b"\xff\xff\x01\x00\x0c\x00moCompFace_c";
    payload[body..body + comp_face.len()].copy_from_slice(comp_face);
    let nested = body + comp_face.len();
    payload[nested..nested + 16].copy_from_slice(&[
        0x86, 0x81, 2, 0, 0x88, 0x81, 0, 0, 0x8a, 0x81, 1, 0, 0, 0, 0x8c, 0x81,
    ]);
    assert_eq!(compact_extrusion_to_face_at(&payload, anchor), Some(body));

    payload[nested + 2] = 3;
    assert_eq!(compact_extrusion_to_face_at(&payload, anchor), None);
    payload[nested + 2] = 2;

    payload[body + 4] = 3;
    assert_eq!(compact_extrusion_to_face_at(&payload, anchor), None);
}


#[test]
fn compact_extrusion_to_face_preserves_an_unparsed_framed_face_path() {
    let mut payload = vec![0; 240];
    payload[..2].copy_from_slice(&[0x95, 0x81]);
    payload[4] = 1;
    payload[18] = 4;
    payload[30..33].copy_from_slice(&[1, 1, 0]);
    payload[33..46].copy_from_slice(&[0x54, 0x89, 0x30, 0x80, 0x2e, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
    let marker = 140;
    payload[marker - 12..marker - 8].copy_from_slice(&6u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);

    assert_eq!(compact_extrusion_to_face_at(&payload, 0), Some(marker));
    payload[marker - 12..marker - 8].fill(0);
    assert_eq!(compact_extrusion_to_face_at(&payload, 0), None);
}


#[test]
fn termination_consensus_uses_stable_reference_identity_across_lanes() {
    let vote = |reference: &str, identity: &str| super::TerminationVote {
        condition: "ToFace".into(),
        reference: Some(reference.into()),
        second_condition: None,
        reference_identity: Some(identity.into()),
        canonical_reference: Some("components:1,2,3".into()),
        depth_m: None,
    };
    let first = vote("lane-0:100", "components:1,2,3");
    let second = vote("lane-1:200", "components:1,2,3");
    let consensus =
        super::consensus_termination_vote(&[Some(first.clone()), Some(second)]).unwrap();
    assert_eq!(consensus.reference.as_deref(), Some("components:1,2,3"));

    let exact = super::consensus_termination_vote(&[Some(first.clone())]).unwrap();
    assert_eq!(exact.reference, first.reference);
    assert!(super::consensus_termination_vote(&[
        Some(first),
        Some(vote("lane-1:200", "components:1,2,4")),
    ])
    .is_none());

    let mut first_depth = vote("lane-0:100", "components:1,2,3");
    first_depth.depth_m = Some(0.01);
    let mut second_depth = vote("lane-1:200", "components:1,2,3");
    second_depth.depth_m = Some(0.02);
    assert!(super::consensus_termination_vote(&[Some(first_depth), Some(second_depth),]).is_none());
}


#[test]
fn compact_extrusion_to_face_accepts_the_long_declared_face_path() {
    let end_spec = b"\xff\xff\x01\x00\x0b\x00moEndSpec_c";
    let face_ref = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    let mut payload = vec![0; 360];
    payload[..end_spec.len()].copy_from_slice(end_spec);
    let anchor = end_spec.len() - 2;
    payload[anchor + 4..anchor + 8].copy_from_slice(&1u32.to_le_bytes());
    payload[anchor + 18..anchor + 22].copy_from_slice(&4u32.to_le_bytes());
    payload[anchor + 30..anchor + 33].copy_from_slice(&[1, 1, 0]);
    let child = anchor + 33;
    payload[child..child + face_ref.len()].copy_from_slice(face_ref);
    let body = child + face_ref.len();
    let marker = body + 209;
    payload.truncate(marker - 12);
    assert_eq!(selection_vector_tail(&mut payload, &[8, 5, 4]), marker);

    assert_eq!(compact_extrusion_to_face_at(&payload, anchor), Some(marker));

    payload[marker - 8] = 1;
    assert_eq!(compact_extrusion_to_face_at(&payload, anchor), None);
}


#[test]
fn compact_extrusion_to_face_accepts_extended_legacy_face_path_padding() {
    let mut payload = vec![0; 300];
    payload[..2].copy_from_slice(&[0x34, 0x80]);
    payload[4] = 1;
    payload[18] = 4;
    payload[30..33].copy_from_slice(&[1, 1, 0]);
    let declaration = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    payload[33..33 + declaration.len()].copy_from_slice(declaration);
    let body = 33 + declaration.len();
    payload[body..body + 19].copy_from_slice(&[
        0x30, 0x80, 0x2e, 0x80, 2, 0, 0, 0, 0x40, 0, 0, 108, 0, 0, 0, 108, 0, 0, 0,
    ]);
    payload[body + 47..body + 63].fill(0xff);
    let control = body + 84;
    payload[control..control + 2].copy_from_slice(&[0x33, 0x80]);
    payload[control + 2..control + 6].copy_from_slice(&1u32.to_le_bytes());
    payload[control + 10..control + 14].copy_from_slice(&5u32.to_le_bytes());
    payload[control + 14..control + 18].copy_from_slice(&[0, 3, 0, 0]);
    payload[control + 22..control + 30].copy_from_slice(&[1; 8]);
    payload[control + 30..control + 38].copy_from_slice(&[1; 8]);
    let entries = [control + 40, control + 64, control + 90];
    for (entry, local_id) in entries.into_iter().zip([3u32, 2, 4]) {
        payload[entry..entry + 4].copy_from_slice(&[0x4c, 0x80, 0, 0]);
        payload[entry + 4..entry + 16].copy_from_slice(&[2; 12]);
        payload[entry + 16..entry + 20].copy_from_slice(&local_id.to_le_bytes());
        payload[entry + 20..entry + 24].copy_from_slice(&33u32.to_le_bytes());
    }
    let terminal = entries[2] + 24;
    payload[terminal..terminal + 24].fill(0);
    payload[terminal + 24..terminal + 28].copy_from_slice(&101u32.to_le_bytes());

    assert_eq!(compact_extrusion_to_face_at(&payload, 0), Some(body));
    let path = legacy_single_face_reference_path_at(&payload, body).expect("required invariant");
    assert_eq!(
        path.iter().map(|entry| entry.local_id).collect::<Vec<_>>(),
        [Some(3), Some(2), Some(4)]
    );
    payload[body + 47] = 0xfe;
    assert_eq!(compact_extrusion_to_face_at(&payload, 0), None);
}


#[test]
fn compact_extrusion_through_next_shares_the_traversal_tail() {
    let mut payload = vec![0; 104];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 2;
    payload[30..34].copy_from_slice(&[1, 0, 0, 1]);
    payload[92] = 1;
    assert!(compact_extrusion_through_next_at(&payload, 0));
    assert!(!compact_extrusion_through_all_at(&payload, 0));

    payload[18] = 1;
    assert!(compact_extrusion_through_all_at(&payload, 0));
    assert!(!compact_extrusion_through_next_at(&payload, 0));
    payload[18] = 2;
    payload[103] = 1;
    assert!(!compact_extrusion_through_next_at(&payload, 0));

    payload[103] = 0;
    payload[92] = 0;
    payload[90] = 1;
    assert!(compact_extrusion_through_next_at(&payload, 0));

    payload.resize(108, 0);
    payload[100..102].copy_from_slice(&[0x83, 0x81]);
    payload[102..106].copy_from_slice(&5u32.to_le_bytes());
    payload[106..108].copy_from_slice(&[0x74, 0x81]);
    payload.resize(108 + 16, 0);
    payload.extend_from_slice(&[0xff, 0xff, 0, 0, 1]);
    payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
    assert!(compact_extrusion_through_next_at(&payload, 0));
}


#[test]
fn compact_extrusion_through_all_accepts_a_retained_dimension_child() {
    let mut payload = vec![0; 26];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 1;
    payload.extend_from_slice(b"\xff\xff\x01\x00\x16\x00moDisplayDistanceDim_c");
    let block = payload.len();
    payload.resize(block + 16, 0);
    payload[block + 9] = 0x20;
    payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
    payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);

    assert!(compact_extrusion_through_all_at(&payload, 0));
    payload[22] = 1;
    assert!(!compact_extrusion_through_all_at(&payload, 0));
}


#[test]
fn compact_extrusion_through_all_accepts_a_dimensioned_traversal_body() {
    let mut payload = vec![0; 68];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 1;
    payload[30..34].copy_from_slice(&[1, 0, 0, 1]);
    payload[44..48].copy_from_slice(&1u32.to_le_bytes());
    payload.extend_from_slice(&[0x77, 0x83]);
    let block = payload.len();
    payload.resize(block + 16, 0);
    payload[block + 8] = 0x40;
    payload[block + 9] = 0x28;
    payload.extend_from_slice(&[0xff, 0xff, 0, 0, 1]);
    payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);

    assert!(compact_extrusion_through_all_at(&payload, 0));
    payload[44] = 0;
    assert!(!compact_extrusion_through_all_at(&payload, 0));
}


#[test]
fn compact_extrusion_mid_plane_requires_the_dimension_child() {
    let dimension_tail = |payload: &mut Vec<u8>| {
        let block = payload.len();
        payload.resize(block + 16, 0);
        payload[block + 9] = 0x20;
        payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
        payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
    };

    let mut payload = vec![0; 26];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 6;
    payload.extend_from_slice(&[0x6a, 0x81]);
    dimension_tail(&mut payload);
    assert!(compact_extrusion_mid_plane_at(&payload, 0));

    payload[18] = 5;
    assert!(!compact_extrusion_mid_plane_at(&payload, 0));
    payload[18] = 6;
    let last = payload.len() - 1;
    payload[last] = 0;
    assert!(!compact_extrusion_mid_plane_at(&payload, 0));

    let mut payload = vec![0; 26];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 6;
    payload.extend_from_slice(b"\xff\xff\x01\x00\x16\x00moDisplayDistanceDim_c");
    dimension_tail(&mut payload);
    assert!(compact_extrusion_mid_plane_at(&payload, 0));
}


#[test]
fn compact_extrusion_blind_requires_code_zero_and_the_dimension_child() {
    let mut payload = vec![0; 26];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload.extend_from_slice(b"\xff\xff\x01\x00\x16\x00moDisplayDistanceDim_c");
    let block = payload.len();
    payload.resize(block + 16, 0);
    payload[block + 9] = 0x20;
    payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
    payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);

    assert!(compact_extrusion_blind_at(&payload, 0));
    payload[block + 8] = 0x40;
    assert!(compact_extrusion_blind_at(&payload, 0));
    payload[18] = 1;
    assert!(!compact_extrusion_blind_at(&payload, 0));
    payload[18] = 0;
    payload[22] = 1;
    assert!(!compact_extrusion_blind_at(&payload, 0));

    let mut compact = payload[..22].to_vec();
    compact.extend_from_slice(&payload[26..]);
    assert!(compact_extrusion_blind_at(&compact, 0));
}


#[test]
fn inline_operation_binds_join_and_cut_to_their_family_words() {
    use super::{
        feature_inline_operation, feature_inline_operation_fields, feature_operation_code,
    };
    use crate::records::{FeatureInputLane, FeatureInputName};
    use cadmpeg_ir::features::BooleanOp;

    let value = "F";
    let name_offset = 10usize;
    let mut payload = vec![0; 40];
    let trailer = name_offset + 6 + 2;
    payload[trailer + 4] = 0x40;
    payload[trailer + 5] = 1;
    payload[trailer + 7] = 0xc0;
    payload[trailer + 8..trailer + 12].copy_from_slice(&7u32.to_le_bytes());
    payload[trailer + 16..trailer + 19].copy_from_slice(&[0xff, 0xfe, 0xff]);
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let name = FeatureInputName {
        id: "name".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: name_offset as u64,
        value: value.into(),
        object_id: Some(7),
    };
    let mut lane = lane;
    lane.native_payload[name_offset - 6..name_offset - 2].copy_from_slice(&1u32.to_le_bytes());
    lane.native_payload[name_offset - 2..name_offset].copy_from_slice(&0x8d9au16.to_le_bytes());
    assert_eq!(
        feature_operation_code(&lane, &name, Some("moICE_c")),
        Some(1)
    );
    assert_eq!(
        feature_operation_code(&lane, &name, Some("moExtrusion_c")),
        None
    );
    assert_eq!(
        feature_inline_operation(&lane, &name),
        Some(BooleanOp::Join)
    );
    // A zero operation byte on an moICE_c object carries no operation.
    lane.native_payload[trailer + 4] = 0xca;
    assert_eq!(feature_inline_operation(&lane, &name), None);
    assert!(feature_inline_operation_fields(&lane, &name).is_some());
    lane.native_payload[trailer + 6] = 2;
    assert_eq!(feature_inline_operation(&lane, &name), Some(BooleanOp::Cut));
    lane.native_payload[trailer + 4] = 0x40;
    assert_eq!(feature_inline_operation(&lane, &name), None);
    lane.native_payload[trailer + 6] = 3;
    assert_eq!(feature_inline_operation_fields(&lane, &name), None);

    lane.native_payload[trailer + 6] = 0;
    lane.native_payload[trailer + 16..trailer + 19].fill(0);
    lane.native_payload.resize(trailer + 40, 0);
    lane.native_payload[trailer + 22..trailer + 24].copy_from_slice(&[1, 0]);
    lane.native_payload[trailer + 24..trailer + 26].copy_from_slice(&0x0185u16.to_le_bytes());
    lane.native_payload[trailer + 38..trailer + 40].copy_from_slice(&0x019fu16.to_le_bytes());
    assert_eq!(
        feature_inline_operation(&lane, &name),
        Some(BooleanOp::Join)
    );
    lane.native_payload[trailer + 38..trailer + 40].fill(0);
    assert_eq!(feature_inline_operation_fields(&lane, &name), None);

    lane.native_payload[trailer + 4] = 0xca;
    lane.native_payload[trailer + 16..trailer + 40].fill(0);
    lane.native_payload[trailer + 18..trailer + 20].copy_from_slice(&[1, 0]);
    lane.native_payload[trailer + 20..trailer + 24].copy_from_slice(&360u32.to_le_bytes());
    lane.native_payload[trailer + 34..trailer + 36].copy_from_slice(&435u16.to_le_bytes());
    assert_eq!(
        feature_inline_operation_fields(&lane, &name),
        Some((0xca, 0))
    );
    assert_eq!(feature_inline_operation(&lane, &name), None);
}


#[test]
fn declared_ice_object_uses_a_unanimous_repeated_class_form() {
    use super::class_scoped_extrusion_operation;

    let native_feature = |id: &str, source: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source.into()),
        parent_source_id: None,
        ordinal: source.parse().expect("required invariant"),
        name: id.into(),
        kind: "Extrusion".into(),
        input_class: Some("moICE_c".into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let features = [
        native_feature("first", "67"),
        native_feature("second", "79"),
        native_feature("third", "90"),
    ];
    let names = [
        FeatureInputName {
            id: "first-name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 33,
            value: "F".into(),
            object_id: Some(67),
        },
        FeatureInputName {
            id: "second-name".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 100,
            value: "S".into(),
            object_id: Some(79),
        },
        FeatureInputName {
            id: "third-name".into(),
            parent: "lane".into(),
            ordinal: 2,
            offset: 150,
            value: "T".into(),
            object_id: Some(90),
        },
    ];
    let mut payload = vec![0; 200];
    let trailer = 33 + 6 + 2;
    payload[trailer + 4] = 0xca;
    payload[trailer + 5] = 1;
    payload[trailer + 8..trailer + 12].copy_from_slice(&67u32.to_le_bytes());
    payload[trailer + 16..trailer + 19].copy_from_slice(&[0xff, 0xfe, 0xff]);
    for name_offset in [100_usize, 150] {
        let code_offset = name_offset - 14;
        payload[code_offset..code_offset + 4].copy_from_slice(&11u32.to_le_bytes());
        payload[name_offset - 2..name_offset].copy_from_slice(&0x8000u16.to_le_bytes());
    }
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "ice".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 20,
            name: "moICE_c".into(),
            role: FeatureInputClassRole::Feature,
        }],
        names: names.to_vec(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let feature_refs = features.iter().collect::<Vec<_>>();

    assert_eq!(
        class_scoped_extrusion_operation(&features[0], &feature_refs, &lane, &names[0],),
        Some(BooleanOp::Cut)
    );
    lane.native_payload[136..140].copy_from_slice(&6u32.to_le_bytes());
    assert_eq!(
        class_scoped_extrusion_operation(&features[0], &feature_refs, &lane, &names[0],),
        None
    );
}


#[test]
fn extrusion_form_codes_are_scoped_to_their_native_classes() {
    use super::extrusion_operation;
    use cadmpeg_ir::features::BooleanOp;

    assert_eq!(
        extrusion_operation(Some("moExtrusion_c"), 82),
        Some(BooleanOp::Join)
    );
    assert_eq!(
        extrusion_operation(Some("moExtrusion_c"), 4),
        Some(BooleanOp::Join)
    );
    assert_eq!(extrusion_operation(Some("moICE_c"), 82), None);
    for code in [6, 21, 0x3ee4_f8b5] {
        assert_eq!(
            extrusion_operation(Some("moICE_c"), code),
            Some(BooleanOp::Join)
        );
    }
    for code in [0, 1, 2, 5, 7, 10, 14, 15, 22_993, u32::MAX] {
        assert_eq!(
            extrusion_operation(Some("moICE_c"), code),
            Some(BooleanOp::Cut)
        );
    }
    assert_eq!(extrusion_operation(Some("moExtrusion_c"), u32::MAX), None);
}


#[test]
fn compact_extrusion_through_all_both_accepts_both_carriers() {
    let dimension_tail = |payload: &mut Vec<u8>| {
        let block = payload.len();
        payload.resize(block + 16, 0);
        payload[block + 9] = 0x20;
        payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
        payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
    };

    // Traversal carrier: first-direction code 1 with second-direction 1.
    let mut payload = vec![0; 104];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[18] = 1;
    payload[22] = 1;
    payload[30..34].copy_from_slice(&[1, 0, 0, 1]);
    payload[92] = 1;
    assert!(compact_extrusion_through_all_both_at(&payload, 0));
    assert!(!compact_extrusion_through_all_at(&payload, 0));
    payload[8] = 1;
    assert!(compact_extrusion_through_all_both_at(&payload, 0));
    payload[8] = 2;
    assert!(!compact_extrusion_through_all_both_at(&payload, 0));
    payload[8] = 0;
    payload[22] = 0;
    assert!(!compact_extrusion_through_all_both_at(&payload, 0));
    payload[22] = 1;
    payload[18] = 2;
    assert!(!compact_extrusion_through_all_both_at(&payload, 0));

    // Dedicated code 9 carrier with the retained dimension child.
    let mut payload = vec![0; 26];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[18] = 9;
    payload[22] = 1;
    payload.extend_from_slice(&[0x6a, 0x81]);
    dimension_tail(&mut payload);
    assert!(compact_extrusion_through_all_both_at(&payload, 0));
    payload[22] = 0;
    assert!(!compact_extrusion_through_all_both_at(&payload, 0));
    payload[22] = 1;
    payload[4] = 2;
    assert!(!compact_extrusion_through_all_both_at(&payload, 0));
}


#[test]
fn compact_extrusion_blind_second_direction_requires_the_dimension_child() {
    let mut payload = vec![0; 26];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[22] = 1;
    payload.extend_from_slice(&[0x6a, 0x81]);
    let block = payload.len();
    payload.resize(block + 16, 0);
    payload[block + 9] = 0x20;
    payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
    payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
    assert!(compact_extrusion_blind_through_all_second_at(&payload, 0));
    assert!(!compact_extrusion_through_all_both_at(&payload, 0));
    payload[22] = 0;
    assert!(!compact_extrusion_blind_through_all_second_at(&payload, 0));
    payload[22] = 1;
    payload[4] = 0;
    assert!(!compact_extrusion_blind_through_all_second_at(&payload, 0));
    payload[4] = 1;
    let last = payload.len() - 1;
    payload[last] = 0;
    assert!(!compact_extrusion_blind_through_all_second_at(&payload, 0));
}


#[test]
fn end_spec_headers_require_the_anchor_class_identity() {
    let mut payload = vec![0; 104];
    payload[4] = 1;
    payload[18] = 1;
    payload[30..34].copy_from_slice(&[1, 0, 0, 1]);
    payload[92] = 1;
    // Header-shaped run without a class token or declaration at the anchor
    // is a fillet edge-set impostor, not an end spec.
    assert!(!compact_extrusion_through_all_at(&payload, 0));
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    assert!(compact_extrusion_through_all_at(&payload, 0));
    payload[..2].copy_from_slice(&[0xff, 0xff]);
    assert!(!compact_extrusion_through_all_at(&payload, 0));

    let mut payload = vec![0; 15];
    payload.extend_from_slice(&[0; 104]);
    payload[15 + 4] = 1;
    payload[15 + 18] = 1;
    payload[15 + 30..15 + 34].copy_from_slice(&[1, 0, 0, 1]);
    payload[15 + 92] = 1;
    assert!(!compact_extrusion_through_all_at(&payload, 15));
    payload[..17].copy_from_slice(b"\xff\xff\x01\x00\x0b\x00moEndSpec_c");
    assert!(compact_extrusion_through_all_at(&payload, 15));
}


#[test]
fn legacy_single_face_reference_requires_a_unique_counted_path() {
    let mut payload = vec![0; 128];
    payload[0..4].copy_from_slice(&[0x53, 0x81, 0x80, 0x80]);
    payload[4..8].copy_from_slice(&2u32.to_le_bytes());
    payload[11..15].copy_from_slice(&101u32.to_le_bytes());
    payload[15..19].copy_from_slice(&101u32.to_le_bytes());

    let control = 44;
    payload[control..control + 2].copy_from_slice(&[0x1e, 0x81]);
    payload[control + 2..control + 6].copy_from_slice(&1u32.to_le_bytes());
    payload[control + 10..control + 14].copy_from_slice(&1u32.to_le_bytes());
    payload[control + 14..control + 18].copy_from_slice(&[0, 2, 0, 0]);
    payload[control + 22..control + 30].copy_from_slice(&[1; 8]);
    payload[control + 30..control + 38].copy_from_slice(&[1; 8]);

    let entry = control + 40;
    payload[entry..entry + 4].copy_from_slice(&[0x32, 0x80, 0, 0]);
    payload[entry + 4..entry + 16].copy_from_slice(&[1; 12]);
    payload[entry + 16..entry + 20].copy_from_slice(&7u32.to_le_bytes());
    payload[entry + 20..entry + 28].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);

    let path = legacy_single_face_reference_path_at(&payload, 0).expect("required invariant");
    assert_eq!(path.len(), 1);
    assert_eq!(path[0].instance, Some(0x8032));
    assert_eq!(path[0].type_signature, [1; 12]);
    assert_eq!(path[0].local_id, Some(7));

    payload[entry + 1] = 0;
    assert_eq!(legacy_single_face_reference_path_at(&payload, 0), None);
    payload[entry + 1] = 0x80;
    payload[control + 30] = 2;
    assert_eq!(legacy_single_face_reference_path_at(&payload, 0), None);
}

fn selection_vector_tail(payload: &mut Vec<u8>, entries: &[u32]) -> usize {
    payload.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    payload.extend_from_slice(&[0, 2, 0, 0]);
    payload.extend_from_slice(&[0, 0, 0, 0]);
    let marker = payload.len();
    payload.extend_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload.extend_from_slice(&[0, 0]);
    for local_id in entries {
        payload.extend_from_slice(&[0x32, 0x80, 0, 0]);
        payload.extend_from_slice(&[1; 12]);
        payload.extend_from_slice(&local_id.to_le_bytes());
    }
    marker
}


#[test]
fn compact_extrusion_to_vertex_accepts_both_point_reference_forms() {
    // Variant A, repeated-token form.
    let mut payload = vec![0; 30];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 3;
    payload.extend_from_slice(&[0x82, 0x92, 0x2b, 0x80, 2, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0; 12]);
    let marker = selection_vector_tail(&mut payload, &[4, 7]);
    let (found, kind) = compact_extrusion_to_vertex_at(&payload, 0).expect("required invariant");
    assert_eq!(found, marker);
    assert_eq!(kind, CompactPointReferenceKind::Point);
    let path = compact_single_face_reference_path_at(&payload, marker).expect("required invariant");
    assert_eq!(path.last().expect("required invariant").local_id, Some(7));

    // A to-face selector byte is not a point reference.
    payload[38] = 0x40;
    assert_eq!(compact_extrusion_to_vertex_at(&payload, 0), None);
    payload[38] = 0;
    payload[18] = 4;
    assert_eq!(compact_extrusion_to_vertex_at(&payload, 0), None);
    payload[18] = 3;

    // Variant B, edge endpoint reference.
    let mut payload = vec![0; 30];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 3;
    payload.extend_from_slice(b"\xff\xff\x01\x00\x0f\x00moEndPointRef_w");
    payload.extend_from_slice(b"\xff\xff\x01\x00\x0c\x00moCompEdge_c");
    payload.extend_from_slice(&[0xcb, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
    payload.extend_from_slice(&[0; 12]);
    let marker = selection_vector_tail(&mut payload, &[2]);
    let (found, kind) = compact_extrusion_to_vertex_at(&payload, 0).expect("required invariant");
    assert_eq!(found, marker);
    assert_eq!(kind, CompactPointReferenceKind::EdgeEndpoint);
}


#[test]
fn compact_extrusion_offset_from_face_requires_the_late_face_reference() {
    let mut payload = vec![0; 26];
    payload[..2].copy_from_slice(&[0x0c, 0x8e]);
    payload[4] = 1;
    payload[18] = 5;
    payload.extend_from_slice(&[0x6a, 0x81]);
    let block = payload.len();
    payload.resize(block + 16, 0);
    payload[block + 9] = 0x20;
    payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
    payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
    payload.extend_from_slice(&[0; 40]);
    payload.extend_from_slice(&[1, 1, 0]);
    payload.extend_from_slice(b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w");
    payload.extend_from_slice(&[0xf2, 0x82, 0xe6, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
    payload.extend_from_slice(&[0; 8]);
    let marker = selection_vector_tail(&mut payload, &[9]);
    let end = payload.len();
    assert_eq!(
        compact_extrusion_offset_from_face_at(&payload, 0, end),
        Some(marker)
    );

    // Wrong code or a missing face-reference anchor yields no detection.
    payload[18] = 6;
    assert_eq!(
        compact_extrusion_offset_from_face_at(&payload, 0, end),
        None
    );
    payload[18] = 5;
    let anchor = payload
        .windows(3)
        .position(|window| window == [1, 1, 0])
        .expect("required invariant");
    payload[anchor] = 0;
    assert_eq!(
        compact_extrusion_offset_from_face_at(&payload, 0, end),
        None
    );
}


#[test]
fn object_names_follow_the_lane_name_class_token() {
    let mut payload = vec![0x42, 0, 0, 0, 0x13, 0];
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&18u16.to_le_bytes());
    payload.extend_from_slice(b"moFavoriteFolder_c");
    payload.extend_from_slice(&[0x87, 0x80, 0xff, 0xfe, 0xff]);
    payload.push(9);
    for unit in "Favorites".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.resize(payload.len() + 12, 0);
    payload.extend_from_slice(&[0x87, 0x80, 0xff, 0xfe, 0xff]);
    payload.push(4);
    for unit in "Boss".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.resize(payload.len() + 12, 0);

    let names = object_names(&payload, "lane");
    assert_eq!(
        names
            .iter()
            .map(|name| name.value.as_str())
            .collect::<Vec<_>>(),
        ["Favorites", "Boss"]
    );
}


#[test]
fn compact_general_curve_reference_requires_the_nested_profile_prefix() {
    let mut payload = vec![0; 24];
    payload[2..4].copy_from_slice(&0xe1u16.to_le_bytes());
    payload[6..8].copy_from_slice(&0x802du16.to_le_bytes());
    payload[8..18].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]);
    assert!(compact_general_curve_ref_at(&payload, 2));
    payload[12] = 1;
    assert!(!compact_general_curve_ref_at(&payload, 2));
}


#[test]
fn general_curve_component_profile_requires_a_complete_reference_record() {
    let mut payload = vec![0; 192];
    let prefix = 24;
    payload[prefix..prefix + 10].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]);
    payload[prefix + 45..prefix + 61].fill(0xff);
    let source = prefix + 81;
    payload[source..source + 4].copy_from_slice(&134u32.to_le_bytes());
    payload[source + 4..source + 8].copy_from_slice(&0x5edf_5674u32.to_le_bytes());
    payload[source + 16..source + 20].copy_from_slice(&0x65u32.to_le_bytes());
    payload[source + 24..source + 28].fill(0xff);
    for at in [source + 32, source + 36, source + 40] {
        payload[at..at + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    payload[source + 48..source + 52].copy_from_slice(&[0xf8, 0x2a, 0, 0]);

    assert_eq!(component_profile_source_at(&payload, prefix), Some(134));
    payload[source + 40] ^= 1;
    assert_eq!(component_profile_source_at(&payload, prefix), None);
}


#[test]
fn component_reference_curve_accepts_count_minus_one_with_instance_separator() {
    let marker = 24;
    let mut payload = vec![0; 180];
    payload[marker - 12..marker - 8].copy_from_slice(&5u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[4, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let mut cursor = marker + 18;
    let mut signature = [0u8; 12];
    signature[4..8].copy_from_slice(&137u32.to_le_bytes());
    for (index, instance) in [0x8c20u16, 0x8c25, 0x8c1a, 0x8c15].into_iter().enumerate() {
        if index == 1 {
            payload[cursor..cursor + 6].copy_from_slice(&[1, 0, 0, 0, 0, 0]);
            cursor += 6;
        }
        payload[cursor..cursor + 2].copy_from_slice(&instance.to_le_bytes());
        payload[cursor + 4..cursor + 16].copy_from_slice(&signature);
        payload[cursor + 16..cursor + 20].copy_from_slice(&1u32.to_le_bytes());
        cursor += 20;
    }
    payload[cursor + 8..cursor + 12].copy_from_slice(&[0xf8, 0x2a, 0, 0]);

    let components =
        component_reference_curve_path_at(&payload, marker).expect("required invariant");
    assert_eq!(components.len(), 4);
    assert_eq!(components[0].instance, Some(0x8c20));
    assert!(components
        .iter()
        .all(|component| component.local_id == Some(1)));

    payload[cursor + 8] ^= 1;
    assert_eq!(component_reference_curve_path_at(&payload, marker), None);
}


#[test]
fn scalar_trailer_is_relative_to_variable_length_name() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(3);
    for unit in "D10".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(SCALAR_HEADER);
    payload.extend_from_slice(&0.025f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 59, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&42u32.to_le_bytes());
    payload[trailer + 24..trailer + 29].copy_from_slice(&[0, 0, 0, 2, 0]);
    for (relative, index) in [(35usize, 7u16), (47, 9)] {
        payload[trailer + relative..trailer + relative + 2].copy_from_slice(&[0xd6, 0x80]);
        payload[trailer + relative + 2..trailer + relative + 4]
            .copy_from_slice(&index.to_le_bytes());
        payload[trailer + relative + 4..trailer + relative + 8].fill(0xff);
    }
    let names = object_names(&payload, "lane");
    let scalars = named_scalars(&payload, "lane", &names);
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(scalar.object_id, 42);
    assert_eq!(scalar.role, crate::records::FeatureInputScalarRole::Driving);
    assert_eq!(scalar.entity_indices, [7, 9]);
}


#[test]
fn compact_scalar_header_ends_at_the_value() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(2);
    for unit in "D1".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(super::COMPACT_SCALAR_HEADER);
    payload.extend_from_slice(&0.025f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 51, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&115u32.to_le_bytes());
    payload[trailer + 21..trailer + 27].copy_from_slice(&[1, 0, 0, 0, 2, 0]);
    payload[trailer + 27] = 0;
    payload[trailer + 35..trailer + 37].copy_from_slice(&0x8152u16.to_le_bytes());
    payload[trailer + 37..trailer + 39].copy_from_slice(&7u16.to_le_bytes());
    payload[trailer + 39..trailer + 43].fill(0xff);
    payload[trailer + 43..trailer + 45].copy_from_slice(&0x8152u16.to_le_bytes());
    payload[trailer + 45..trailer + 47].copy_from_slice(&9u16.to_le_bytes());
    payload[trailer + 47..trailer + 51].fill(0xff);

    let names = object_names(&payload, "lane");
    let scalars = named_scalars(&payload, "lane", &names);
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(scalar.value, 0.025);
    assert_eq!(scalar.object_id, 115);
    assert_eq!(scalar.role, super::FeatureInputScalarRole::Driving);
    assert!(scalar.entity_indices.is_empty());
    assert_eq!(
        scalar
            .operands
            .iter()
            .map(|operand| (operand.kind, operand.entity_index))
            .collect::<Vec<_>>(),
        [
            (FeatureInputOperandKind::Native(0x8152), 7),
            (FeatureInputOperandKind::Native(0x8152), 9),
        ]
    );
}


#[test]
fn padded_compact_scalar_header_ends_after_its_padding() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(2);
    for unit in "D1".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(super::PADDED_COMPACT_SCALAR_HEADER);
    payload.extend_from_slice(&0.2f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 12, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&16u32.to_le_bytes());

    let names = object_names(&payload, "lane");
    let scalars = named_scalars(&payload, "lane", &names);
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(scalar.value, 0.2);
    assert_eq!(scalar.object_id, 16);
    assert_eq!(usize::try_from(scalar.offset).ok(), Some(trailer - 8));
}


#[test]
fn value_only_scalar_header_ends_at_the_value() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(2);
    for unit in "D1".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(super::VALUE_ONLY_SCALAR_HEADER);
    payload.extend_from_slice(&0.01f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 24, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&132u32.to_le_bytes());

    let names = object_names(&payload, "lane");
    let scalars = named_scalars(&payload, "lane", &names);
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(scalar.value, 0.01);
    assert_eq!(scalar.object_id, 132);
    assert_eq!(scalar.role, super::FeatureInputScalarRole::Native);
    assert!(scalar.operands.is_empty());
}


#[test]
fn legacy_scalar_layout_carries_shifted_role_and_operand() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(2);
    for unit in "D1".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(SCALAR_HEADER);
    payload.extend_from_slice(&0.004f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 48, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&28u32.to_le_bytes());
    payload[trailer + 24..trailer + 30].copy_from_slice(&[0x0f, 0, 0, 0, 2, 0]);
    payload[trailer + 30] = 0;
    payload[trailer + 36..trailer + 38].copy_from_slice(&[0xcc, 0x80]);
    payload[trailer + 38..trailer + 40].copy_from_slice(&0u16.to_le_bytes());
    payload[trailer + 40..trailer + 44].fill(0xff);

    let names = object_names(&payload, "lane");
    let scalars = named_scalars(&payload, "lane", &names);
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(scalar.role, crate::records::FeatureInputScalarRole::Driving);
    assert_eq!(scalar.operands.len(), 1);
    assert_eq!(scalar.operands[0].offset, (trailer + 36) as u64);
    assert_eq!(
        scalar.operands[0].kind,
        crate::records::FeatureInputOperandKind::Native(0x80cc)
    );
    assert_eq!(scalar.operands[0].entity_index, 0);
}


#[test]
fn coordinate_marker_local_id_uses_the_variant_footer() {
    let mut payload = vec![0; 142 + 5];
    payload[..5].copy_from_slice(super::SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[138..142].copy_from_slice(&41u32.to_le_bytes());
    payload[142..147].copy_from_slice(super::SKETCH_MARKER);
    assert_eq!(marker_local_id(&payload, 0), Some(41));
}


#[test]
fn coordinate_less_geometry_locus_uses_the_variant_footer() {
    let mut payload = vec![0; 142 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[138..142].copy_from_slice(&41u32.to_le_bytes());
    payload[142..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(marker_local_id(&payload, 0), Some(41));
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(marker_local_id(&payload, 0), None);
}


#[test]
fn legacy_sketch_prefix_uses_the_shared_entity_body() {
    let mut payload = vec![0; 142 + LEGACY_SKETCH_MARKER.len()];
    payload[..5].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[66..74].copy_from_slice(&1.25f64.to_le_bytes());
    payload[74..82].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[138..142].copy_from_slice(&41u32.to_le_bytes());
    payload[142..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let entities = sketch_input_entities(&payload, "lane");

    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].coordinates_m, Some([1.25, -2.5]));
    assert_eq!(entities[0].local_id, Some(41));
}


#[test]
fn terminal_wide_geometry_locus_coordinate_record_is_a_point() {
    for (prefix, code) in [
        (SKETCH_MARKER, 2u32),
        (LEGACY_SKETCH_MARKER, 1),
        (LEGACY_EXTENDED_SKETCH_MARKER, 2),
    ] {
        let mut payload = vec![0; 142 + prefix.len()];
        payload[..prefix.len()].copy_from_slice(prefix);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&code.to_le_bytes());
        payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
        payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[64..66].copy_from_slice(&[0x1e, 0x00]);
        payload[66..74].copy_from_slice(&0.025f64.to_le_bytes());
        payload[74..82].copy_from_slice(&(-0.004f64).to_le_bytes());
        payload[92..96].copy_from_slice(&(-2i32).to_le_bytes());
        payload[138..142].copy_from_slice(&7u32.to_le_bytes());
        payload[142..].copy_from_slice(prefix);

        let entities = sketch_input_entities(&payload, "lane");
        let [entity] = entities.as_slice() else {
            panic!("expected one marker entity");
        };
        assert_eq!(entity.kind, SketchInputKind::Point);
        assert_eq!(entity.coordinates_m, Some([0.025, -0.004]));

        payload[134..138].copy_from_slice(&6u32.to_le_bytes());
        assert_eq!(
            sketch_input_entities(&payload, "lane")[0].kind,
            SketchInputKind::Point
        );
        payload[133] = 1;
        assert!(!super::terminal_wide_geometry_locus_profile_vertex(
            &payload, 0
        ));
    }
}


#[test]
fn compact_legacy_generation_carries_points_curves_and_selected_axes() {
    let mut payload = vec![0; 280 + LEGACY_SKETCH_MARKER.len()];
    let header = |payload: &mut [u8], offset: usize, code: u32, role: u16, flag: u8| {
        payload[offset..offset + 5].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&code.to_le_bytes());
        payload[offset + 17..offset + 23].copy_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x02, 0x00]);
        payload[offset + 23..offset + 25].copy_from_slice(&role.to_le_bytes());
        payload[offset + 31] = flag;
    };

    header(&mut payload, 0, 1, 1, 4);
    payload[42..44].copy_from_slice(&[0x1e, 0x00]);
    payload[44..52].copy_from_slice(&0.029f64.to_le_bytes());
    payload[52..60].copy_from_slice(&0.0f64.to_le_bytes());

    header(&mut payload, 132, 0, 1, 4);
    payload[157..159].copy_from_slice(&1u16.to_le_bytes());
    payload[174..176].copy_from_slice(&0u16.to_le_bytes());
    payload[176..178].copy_from_slice(&1u16.to_le_bytes());
    payload[178..182].copy_from_slice(&1u32.to_le_bytes());
    payload[182..190].copy_from_slice(&(-1.0f64).to_le_bytes());

    header(&mut payload, 200, 0, 2, 12);
    payload[205..209].fill(0xff);
    payload[209..213].copy_from_slice(&[0x04, 0x00, 0xff, 0xff]);
    payload[242..244].copy_from_slice(&15u16.to_le_bytes());
    payload[244..246].copy_from_slice(&0u16.to_le_bytes());
    payload[250..258].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[280..285].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(marker_coordinates(&payload, 0), Some([0.029, 0.0]));
    assert_eq!(
        compact_legacy_curve_endpoint_indices(&payload, 132),
        Some([1, 2])
    );
    assert_eq!(
        compact_legacy_selected_axis_endpoint_indices(&payload, 200),
        Some([16, 1])
    );
}


#[test]
fn compact_legacy_geometry_locus_carries_curve_endpoint_indices() {
    let mut payload = vec![0; 68 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&0u32.to_le_bytes());
    payload[17..23].copy_from_slice(&[0x00, 0x00, 0x05, 0x00, 0x01, 0x00]);
    payload[23..25].copy_from_slice(&1u16.to_le_bytes());
    payload[25..27].copy_from_slice(&1u16.to_le_bytes());
    payload[31] = 0x04;
    payload[42..44].copy_from_slice(&29u16.to_le_bytes());
    payload[44..46].copy_from_slice(&30u16.to_le_bytes());
    payload[46..50].copy_from_slice(&1u32.to_le_bytes());
    payload[50..58].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[68..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        compact_legacy_curve_endpoint_indices(&payload, 0),
        Some([30, 31])
    );
    let entities = sketch_input_entities(&payload, "lane");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].kind, SketchInputKind::LineOrCircle);

    payload[68..].fill(0);
    payload.resize(90 + LEGACY_SKETCH_MARKER.len(), 0);
    payload[58..62].copy_from_slice(&1u32.to_le_bytes());
    payload[62..64].copy_from_slice(&41u16.to_le_bytes());
    for cell in payload[64..80].chunks_exact_mut(4) {
        cell.copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[82..86].copy_from_slice(&76u32.to_le_bytes());
    payload[90..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        compact_legacy_curve_endpoint_indices(&payload, 0),
        Some([30, 31])
    );
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(42));

    payload[82..].fill(0);
    payload.resize(138, 0);
    payload[136..138].copy_from_slice(&[0x08, 0x80]);
    assert_eq!(
        compact_legacy_curve_endpoint_indices(&payload, 0),
        Some([30, 31])
    );
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(42));
}


#[test]
fn compact_legacy_short_role_two_curve_carries_endpoint_indices() {
    let mut payload = vec![0; 68 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&1u32.to_le_bytes());
    payload[17..23].copy_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x02, 0x00]);
    payload[23..25].copy_from_slice(&2u16.to_le_bytes());
    payload[31] = 0x0c;
    payload[42..44].copy_from_slice(&1u16.to_le_bytes());
    payload[44..46].copy_from_slice(&3u16.to_le_bytes());
    payload[50..58].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[58..60].copy_from_slice(&1u16.to_le_bytes());
    payload[64..68].copy_from_slice(&2u32.to_le_bytes());
    payload[68..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        compact_legacy_short_role_two_curve_endpoint_indices(&payload, 0),
        Some([2, 4])
    );
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(42));
    payload[23..25].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        compact_legacy_short_role_two_curve_endpoint_indices(&payload, 0),
        None
    );
    payload[23..25].copy_from_slice(&2u16.to_le_bytes());
    payload[25..27].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        compact_legacy_short_role_two_curve_endpoint_indices(&payload, 0),
        None
    );
    payload[25..27].fill(0);
    payload[64..68].fill(0xff);
    assert_eq!(
        compact_legacy_short_role_two_curve_endpoint_indices(&payload, 0),
        None
    );
}


#[test]
fn compact_legacy_short_role_one_curve_indexes_the_coordinate_roster() {
    let mut payload = vec![0; 68 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&1u32.to_le_bytes());
    payload[17..23].copy_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x02, 0x00]);
    payload[23..27].copy_from_slice(&[0x01, 0x00, 0x01, 0x00]);
    payload[31] = 0x04;
    payload[42..44].copy_from_slice(&0u16.to_le_bytes());
    payload[44..46].copy_from_slice(&2u16.to_le_bytes());
    payload[46..50].copy_from_slice(&1u32.to_le_bytes());
    payload[50..58].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[60..62].copy_from_slice(&3u16.to_le_bytes());
    payload[64..68].copy_from_slice(&2u32.to_le_bytes());
    payload[68..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        compact_legacy_short_role_one_curve_endpoint_indices(&payload, 0),
        Some([1, 3])
    );
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(42));
    payload[25..27].fill(0);
    payload[46..50].fill(0);
    assert_eq!(
        compact_legacy_short_role_one_curve_endpoint_indices(&payload, 0),
        Some([1, 3])
    );
    payload[23..25].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        compact_legacy_short_role_one_curve_endpoint_indices(&payload, 0),
        None
    );
    payload[23..25].copy_from_slice(&1u16.to_le_bytes());
    payload[46..50].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        compact_legacy_short_role_one_curve_endpoint_indices(&payload, 0),
        None
    );
}


#[test]
fn compact_legacy_profile_coordinate_pairings_carry_points() {
    let mut payload = vec![0; 120 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&0u32.to_le_bytes());
    payload[17..23].copy_from_slice(&[0x00, 0x00, 0x05, 0x00, 0x01, 0x00]);
    payload[23..25].copy_from_slice(&1u16.to_le_bytes());
    payload[31] = 0x04;
    payload[42..44].copy_from_slice(&[0x1e, 0x00]);
    payload[44..52].copy_from_slice(&0.025f64.to_le_bytes());
    payload[52..60].copy_from_slice(&(-0.004f64).to_le_bytes());
    payload[120..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(marker_coordinates(&payload, 0), Some([0.025, -0.004]));
    let entities = sketch_input_entities(&payload, "lane");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].kind, SketchInputKind::Point);

    payload[19..23].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(marker_coordinates(&payload, 0), Some([0.025, -0.004]));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    payload[13..17].copy_from_slice(&1u32.to_le_bytes());
    payload[19..23].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(marker_coordinates(&payload, 0), None);
}


#[test]
fn packed_legacy_geometry_locus_carries_profile_coordinates() {
    let mut payload = vec![0; 126 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&0u32.to_le_bytes());
    payload[17..23].copy_from_slice(&[0x00, 0x00, 0x05, 0x00, 0x01, 0x00]);
    payload[23..25].copy_from_slice(&1u16.to_le_bytes());
    payload[29] = 0x04;
    payload[40..48].copy_from_slice(&1.0f64.to_le_bytes());
    payload[48..50].copy_from_slice(&[0x1e, 0x00]);
    payload[50..58].copy_from_slice(&0.025f64.to_le_bytes());
    payload[58..66].copy_from_slice(&(-0.004f64).to_le_bytes());
    payload[126..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(marker_coordinates(&payload, 0), Some([0.025, -0.004]));
    let entities = sketch_input_entities(&payload, "lane");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].kind, SketchInputKind::Point);
    assert_eq!(entities[0].coordinates_m, Some([0.025, -0.004]));
    assert_eq!(entities[0].state_value, Some(1.0));
}


#[test]
fn packed_legacy_curve_codes_carry_coordinate_roster_indices() {
    let mut payload = vec![0; 76 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[17..23].copy_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x02, 0x00]);
    payload[23..25].copy_from_slice(&1u16.to_le_bytes());
    payload[25..27].copy_from_slice(&1u16.to_le_bytes());
    payload[29] = 0x04;
    payload[40..48].copy_from_slice(&1.0f64.to_le_bytes());
    payload[48..50].copy_from_slice(&3u16.to_le_bytes());
    payload[50..52].copy_from_slice(&4u16.to_le_bytes());
    payload[52..56].copy_from_slice(&1u32.to_le_bytes());
    payload[56..64].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[76..].copy_from_slice(LEGACY_SKETCH_MARKER);

    for code in 0u32..=2 {
        payload[13..17].copy_from_slice(&code.to_le_bytes());
        assert_eq!(
            packed_legacy_curve_endpoint_indices(&payload, 0),
            Some([3, 4])
        );
    }
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(48));

    payload[13..17].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(packed_legacy_curve_endpoint_indices(&payload, 0), None);
}


#[test]
fn compact_curve_uses_one_based_endpoint_indices() {
    for prefix in [
        LEGACY_SKETCH_MARKER,
        LEGACY_EXTENDED_SKETCH_MARKER,
        SKETCH_MARKER,
    ] {
        let mut payload = vec![0; 84 + prefix.len()];
        payload[..prefix.len()].copy_from_slice(prefix);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        payload[27..29].copy_from_slice(&2u16.to_le_bytes());
        payload[35..39].copy_from_slice(&[0x00, 0x00, 0x0d, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[56..58].copy_from_slice(&6u16.to_le_bytes());
        payload[58..60].copy_from_slice(&11u16.to_le_bytes());
        payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[84..].copy_from_slice(prefix);

        assert_eq!(compact_curve_endpoint_indices(&payload, 0), Some([7, 12]));
        assert_eq!(
            sketch_input_entities(&payload, "lane")[0].kind,
            SketchInputKind::LineOrCircle
        );
    }
}


#[test]
fn alternate_current_curve_roster_distinguishes_the_selected_axis() {
    let mut payload = vec![0; 168 + SKETCH_MARKER.len()];
    let record = |payload: &mut [u8], offset: usize, role: u16, state: u8| {
        payload[offset..offset + 5].copy_from_slice(SKETCH_MARKER);
        payload[offset + 5..offset + 9].fill(0xff);
        payload[offset + 9..offset + 13].copy_from_slice(&if role == 1 {
            [0x00, 0x00, 0xff, 0xff]
        } else {
            [0x04, 0x00, 0xff, 0xff]
        });
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        payload[offset + 27..offset + 29].copy_from_slice(&role.to_le_bytes());
        payload[offset + 29..offset + 31].copy_from_slice(&u16::from(role == 1).to_le_bytes());
        payload[offset + 31..offset + 35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 35..offset + 39].copy_from_slice(&[0x00, 0x00, state, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&56u16.to_le_bytes());
        payload[offset + 58..offset + 60].copy_from_slice(&57u16.to_le_bytes());
        payload[offset + 60..offset + 64].copy_from_slice(&u32::from(role == 1).to_le_bytes());
        payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    };

    record(&mut payload, 0, 1, 5);
    payload[76..80].copy_from_slice(&8u32.to_le_bytes());
    payload[80..84].copy_from_slice(&5u32.to_le_bytes());
    record(&mut payload, 84, 2, 13);
    payload[160..164].copy_from_slice(&43u32.to_le_bytes());
    payload[164..168].copy_from_slice(&47u32.to_le_bytes());
    payload[168..173].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        alternate_current_indexed_curve_endpoint_indices(&payload, 0),
        Some([57, 58])
    );
    assert_eq!(
        alternate_current_selected_axis_endpoint_indices(&payload, 84),
        Some([57, 58])
    );
    payload[160..168].fill(0);
    assert_eq!(
        alternate_current_selected_axis_endpoint_indices(&payload, 84),
        None
    );
    payload[119..123].copy_from_slice(&[0x00, 0x00, 0x0c, 0x00]);
    assert_eq!(
        alternate_current_selected_axis_endpoint_indices(&payload, 84),
        None
    );
}


#[test]
fn current_compact_selected_axis_indexes_the_zero_based_coordinate_roster() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0d, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&10u16.to_le_bytes());
    payload[58..60].copy_from_slice(&11u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[76..80].copy_from_slice(&6u32.to_le_bytes());
    payload[80..84].copy_from_slice(&6u32.to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);

    assert!(super::current_compact_roster_selected_axis(&payload, 0));
    assert_eq!(super::coordinate_roster_endpoint_offset(&payload, 0), None);
    assert!(!super::marker_is_selected_construction_line(&payload, 0));

    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    assert!(super::current_compact_roster_selected_axis(&payload, 0));

    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    assert!(!super::current_compact_roster_selected_axis(&payload, 0));
}


#[test]
fn compact_profile_curve_role_distinguishes_non_coordinate_lines() {
    let mut payload = vec![0; 92 + LEGACY_SKETCH_MARKER.len()];
    payload[..5].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    payload[64..66].copy_from_slice(&0u16.to_le_bytes());
    payload[66..68].copy_from_slice(&1u16.to_le_bytes());
    payload[92..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let entities = sketch_input_entities(&payload, "lane");

    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].coordinates_m, None);
    assert_eq!(entities[0].kind, SketchInputKind::LineOrCircle);
}


#[test]
fn unrecognized_role_two_records_are_auxiliary() {
    let mut payload = vec![0; 112 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&2u16.to_le_bytes());
    payload[66..68].copy_from_slice(&3u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[112..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert!(auxiliary_profile_record(&payload, 0));
    payload[17..21].fill(0);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x0d, 0x00]);
    assert!(auxiliary_profile_record(&payload, 0));

    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x0c, 0x00]);
    payload[56..58].copy_from_slice(&2u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..80].copy_from_slice(&[0x00, 0x00, 0x02, 0x00, 0, 0, 0, 0]);
    payload[84..84 + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert!(marker_is_selected_construction_line(&payload, 0));
    assert!(!auxiliary_profile_record(&payload, 0));
}


#[test]
fn embedded_class_header_is_not_a_sketch_entity() {
    let mut payload = vec![0; 64];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[35..39].copy_from_slice(CLASS_MARKER);

    assert!(!super::sketch_marker_at(&payload, 0));
    assert!(sketch_input_entities(&payload, "lane").is_empty());
}


#[test]
fn geometry_marker_coordinates_are_selected_by_layout() {
    let mut payload = vec![0; 82];
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&10u32.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[66..74].copy_from_slice(&1.25f64.to_le_bytes());
    payload[74..82].copy_from_slice(&(-2.5f64).to_le_bytes());
    assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));
    payload[64..66].copy_from_slice(&[0x14, 0x00]);
    assert_eq!(marker_coordinates(&payload, 0), None);
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[5] = 0;
    assert_eq!(marker_coordinates(&payload, 0), None);
}


#[test]
fn legacy_geometry_marker_coordinates_use_the_compact_body_offsets() {
    let mut payload = vec![0; 74];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&1.25f64.to_le_bytes());
    payload[66..74].copy_from_slice(&(-2.5f64).to_le_bytes());

    assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));
    let entities = sketch_input_entities(&payload, "lane");
    assert_eq!(entities[0].kind, SketchInputKind::LineOrCircle);

    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());

    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(marker_coordinates(&payload, 0), None);
    let entities = sketch_input_entities(&payload, "lane");
    assert_eq!(
        entities[0].kind,
        SketchInputKind::Relation(SketchRelationKind::Distance)
    );

    payload[17..21].copy_from_slice(&4u32.to_le_bytes());
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    let entities = sketch_input_entities(&payload, "lane");
    assert_eq!(
        entities[0].kind,
        SketchInputKind::Relation(SketchRelationKind::Horizontal)
    );

    payload.resize(154 + LEGACY_SKETCH_MARKER.len(), 0);
    payload[154..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));

    for size in [161, 162] {
        payload.resize(size + LEGACY_SKETCH_MARKER.len(), 0);
        payload[size..].copy_from_slice(LEGACY_SKETCH_MARKER);
        assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));
    }
}


#[test]
fn compact_legacy_coordinate_value_one_is_a_profile_vertex() {
    let mut payload = vec![0; 68];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&1u32.to_le_bytes());
    payload[17..25].copy_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[25..27].copy_from_slice(&1u16.to_le_bytes());
    payload[31] = 0x04;
    payload[42..44].copy_from_slice(&[0x1e, 0x00]);
    payload[44..52].copy_from_slice(&1.25f64.to_le_bytes());
    payload[52..60].copy_from_slice(&(-2.5f64).to_le_bytes());

    assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));
    assert!(compact_legacy_profile_vertex(&payload, 0));
    let entities = sketch_input_entities(&payload, "lane");
    let [entity] = entities.as_slice() else {
        panic!("expected one compact marker");
    };
    assert_eq!(entity.kind, SketchInputKind::Point);
}


#[test]
fn extended_geometry_values_share_the_coordinate_record_layout() {
    let offset = 4;
    for size in [134, 138, 140, 144] {
        let mut payload = vec![0; offset + size + LEGACY_EXTENDED_SKETCH_MARKER.len()];
        payload[..offset].copy_from_slice(&7u32.to_le_bytes());
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
        payload[offset + size..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

        for native_code in 0u32..=2 {
            payload[offset + 17..offset + 21].copy_from_slice(&native_code.to_le_bytes());
            assert_eq!(marker_coordinates(&payload, offset), Some([1.25, -2.5]));
        }
    }
}


#[test]
fn linked_profile_point_carries_coordinates_for_compact_and_long_tails() {
    let offset = 4;
    let mut payload = vec![0; offset + 154 + SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&7u32.to_le_bytes());
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
    payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
    payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[offset + 76..offset + 78].copy_from_slice(&2u16.to_le_bytes());
    for (start, id) in [(78, 2u16), (90, 3u16)] {
        payload[offset + start..offset + start + 2].copy_from_slice(&0x8178u16.to_le_bytes());
        payload[offset + start + 2..offset + start + 4].copy_from_slice(&id.to_le_bytes());
        payload[offset + start + 4..offset + start + 8].fill(0xff);
    }
    payload[offset + 102..offset + 108].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    for prefix in [SKETCH_MARKER, LEGACY_EXTENDED_SKETCH_MARKER] {
        payload[offset..offset + prefix.len()].copy_from_slice(prefix);
        payload[offset + 154..offset + 154 + prefix.len()].copy_from_slice(prefix);

        assert_eq!(
            linked_profile_point(&payload, offset),
            Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 3)]))
        );
        assert_eq!(marker_coordinates(&payload, offset), Some([1.25, -2.5]));
        let entities = super::sketch_input_entities(&payload, "lane");
        let point = entities
            .iter()
            .find(|entity| entity.offset == offset as u64)
            .expect("linked profile point");
        assert_eq!(point.kind, SketchInputKind::Point);
        assert_eq!(point.coordinates_m, Some([1.25, -2.5]));
    }
    payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[offset + 154..offset + 154 + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        linked_profile_point(&payload, offset),
        Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 3)]))
    );
    assert_eq!(
        super::sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[offset + 154..offset + 154 + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 92..offset + 94].copy_from_slice(&4u16.to_le_bytes());
    assert_eq!(
        linked_profile_point(&payload, offset),
        Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 4)]))
    );
    assert_eq!(
        super::sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[offset + 17..offset + 21].fill(0);
    payload[offset + 92..offset + 94].copy_from_slice(&3u16.to_le_bytes());

    payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 74..offset + 78].copy_from_slice(&[0x01, 0x00, 0x03, 0x00]);
    assert_eq!(
        additional_linked_profile_point_coordinates(&payload, offset),
        Some([1.25, -2.5])
    );
    assert_eq!(linked_profile_point(&payload, offset), None);
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[offset + 74..offset + 78].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    assert_eq!(
        additional_linked_profile_point_coordinates(&payload, offset),
        Some([1.25, -2.5])
    );
    assert_eq!(linked_profile_point(&payload, offset), None);
    payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);

    let mut extended = vec![0; offset + 158 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    extended[..offset + 108].copy_from_slice(&payload[..offset + 108]);
    extended[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    extended[offset + 144..offset + 148].copy_from_slice(&3u32.to_le_bytes());
    extended[offset + 148..offset + 152].copy_from_slice(&2u32.to_le_bytes());
    extended[offset + 154..offset + 158].copy_from_slice(&1u32.to_le_bytes());
    extended[offset + 158..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        linked_profile_point(&extended, offset),
        Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 3)]))
    );
    assert_eq!(marker_coordinates(&extended, offset), Some([1.25, -2.5]));
    let entities = super::sketch_input_entities(&extended, "lane");
    let point = entities
        .iter()
        .find(|entity| entity.offset == offset as u64)
        .expect("extended-tail linked profile point");
    assert_eq!(point.kind, SketchInputKind::Point);
    assert_eq!(point.coordinates_m, Some([1.25, -2.5]));

    extended[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    extended[offset + 158..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        linked_profile_point(&extended, offset),
        Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 3)]))
    );
    assert_eq!(
        super::sketch_input_entities(&extended, "lane")[0].kind,
        SketchInputKind::Point
    );

    extended[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    extended[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    extended[offset + 154..offset + 158].fill(0xff);
    extended[offset + 158..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        linked_profile_point(&extended, offset),
        Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 3)]))
    );
    assert_eq!(
        super::sketch_input_entities(&extended, "lane")[0].kind,
        SketchInputKind::Point
    );
    extended[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        linked_profile_point(&extended, offset),
        Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 3)]))
    );
    extended[offset + 17..offset + 21].fill(0);
    assert_eq!(linked_profile_point(&extended, offset), None);
    extended[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    extended[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    extended[offset + 76..offset + 78].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        linked_profile_point(&extended, offset),
        Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 3)]))
    );
    extended[offset + 144..offset + 148].fill(0);
    assert_eq!(linked_profile_point(&extended, offset), None);
}


#[test]
fn current_indexed_line_uses_its_unique_reverse_incidence_pair() {
    let first = 84;
    let second = first + 154;
    let end = second + 154;
    let mut payload = vec![0; end + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[2, 0, 5, 0]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());

    for (offset, coordinates, other) in [
        (first, [1.0f64, 2.0], 11u16),
        (second, [3.0f64, 4.0], 12u16),
    ] {
        payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&coordinates[0].to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&coordinates[1].to_le_bytes());
        payload[offset + 76..offset + 78].copy_from_slice(&2u16.to_le_bytes());
        for (start, selector, id) in [(78, 0x8178u16, 7u16), (90, 0x8132u16, other)] {
            payload[offset + start..offset + start + 2].copy_from_slice(&selector.to_le_bytes());
            payload[offset + start + 2..offset + start + 4].copy_from_slice(&id.to_le_bytes());
            payload[offset + start + 4..offset + start + 8].fill(0xff);
        }
        payload[offset + 102..offset + 108].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    }
    payload[end..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, offset, object_index| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        entity("curve", 0, Some(7)),
        entity("first", first as u64, Some(20)),
        entity("second", second as u64, Some(21)),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        current_reverse_incidence_endpoint_offsets(&payload, &entities[0], &markers),
        Some([first as u64, second as u64])
    );
}


#[test]
fn extended_indexed_profile_point_decodes_compact_coordinates() {
    let offset = 4;
    for size in [134, 138, 140] {
        let mut payload = vec![0; offset + size + LEGACY_EXTENDED_SKETCH_MARKER.len()];
        payload[..offset].copy_from_slice(&5u32.to_le_bytes());
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
        payload[offset + size..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

        assert_eq!(marker_coordinates(&payload, offset), Some([1.25, -2.5]));
    }
}


#[test]
fn extended_linked_profile_vertex_decodes_as_a_point() {
    let offset = 4;
    let mut payload = vec![0; offset + 154 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&5u32.to_le_bytes());
    payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
    payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
    payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[offset + 76..offset + 78].copy_from_slice(&3u16.to_le_bytes());
    for (cell, link) in [(78, 1u16), (90, 2u16)] {
        payload[offset + cell..offset + cell + 2].copy_from_slice(&[0xe7, 0x81]);
        payload[offset + cell + 2..offset + cell + 4].copy_from_slice(&link.to_le_bytes());
        payload[offset + cell + 4..offset + cell + 8].fill(0xff);
    }
    payload[offset + 102..offset + 108].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[offset + 150..offset + 154].copy_from_slice(&9u32.to_le_bytes());
    payload[offset + 154..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert!(super::linked_profile_vertex(&payload, offset));
    assert_eq!(marker_coordinates(&payload, offset), Some([1.25, -2.5]));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    payload[offset + 102] = 1;
    assert!(!super::linked_profile_vertex(&payload, offset));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );
}


#[test]
fn compact_linked_profile_vertex_decodes_legacy_and_extended_markers() {
    let offset = 4;
    let mut payload = vec![0; offset + 146 + LEGACY_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&5u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 23..offset + 29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
    payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
    payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[offset + 74..offset + 78].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    for (cell, link) in [(78, 4u16), (86, 10u16)] {
        payload[offset + cell..offset + cell + 2].copy_from_slice(&[0x8b, 0x81]);
        payload[offset + cell + 2..offset + cell + 4].copy_from_slice(&link.to_le_bytes());
        payload[offset + cell + 4..offset + cell + 8].fill(0xff);
    }
    payload[offset + 94..offset + 100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[offset + 142..offset + 146].copy_from_slice(&11u32.to_le_bytes());
    payload[offset + 146..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert!(super::compact_linked_profile_vertex(&payload, offset));
    assert_eq!(marker_coordinates(&payload, offset), Some([1.25, -2.5]));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    payload[offset + 88..offset + 90].copy_from_slice(&4u16.to_le_bytes());
    assert!(!super::compact_linked_profile_vertex(&payload, offset));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );

    payload[offset + 78..offset + 82].copy_from_slice(&[0x06, 0x81, 0x00, 0x00]);
    payload[offset + 86..offset + 90].copy_from_slice(&[0x01, 0x83, 0x00, 0x00]);
    assert!(super::compact_linked_profile_vertex(&payload, offset));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 146..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert!(super::compact_linked_profile_vertex(&payload, offset));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
}


#[test]
fn current_indexed_profile_point_decodes_compact_coordinates() {
    let offset = 4;
    let mut payload = vec![0; offset + 134 + SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&5u32.to_le_bytes());
    payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
    payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
    payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[offset + 134..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(marker_coordinates(&payload, offset), Some([1.25, -2.5]));
}


#[test]
fn compact_legacy_coordinate_line_ends_at_the_following_marker_coordinate() {
    let mut payload = vec![0; 268 + LEGACY_SKETCH_MARKER.len()];
    for (offset, code, coordinate) in [(0, 1u32, [1.25_f64, -2.5]), (134, 0, [3.0_f64, 4.0])] {
        payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 17..offset + 21].copy_from_slice(&code.to_le_bytes());
        payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&coordinate[0].to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&coordinate[1].to_le_bytes());
    }
    payload[268..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let mut entities = sketch_input_entities(&payload, "lane");
    entities.truncate(2);
    for entity in &mut entities {
        entity.feature_ref = Some("sketch".into());
    }
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        consecutive_legacy_profile_line_endpoints(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([1.25, -2.5]), Some([3.0, 4.0])]
    );
    assert!(consecutive_legacy_profile_line_endpoints(&payload, &entities[1], &markers).is_empty());
}


#[test]
fn linked_profile_curve_uses_its_two_typed_endpoint_cells() {
    let offset = 4;
    let mut payload = vec![0; offset + 146 + SKETCH_MARKER.len()];
    payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
    payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
    payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[offset + 76..offset + 78].copy_from_slice(&3u16.to_le_bytes());
    for (relative, endpoint) in [(78, 2u16), (86, 3u16)] {
        payload[offset + relative..offset + relative + 2].copy_from_slice(&0x8137u16.to_le_bytes());
        payload[offset + relative + 2..offset + relative + 4]
            .copy_from_slice(&endpoint.to_le_bytes());
        payload[offset + relative + 4..offset + relative + 8].fill(0xff);
    }
    payload[offset + 94..offset + 100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[offset + 142..offset + 146].copy_from_slice(&5u32.to_le_bytes());
    for prefix in [SKETCH_MARKER, LEGACY_EXTENDED_SKETCH_MARKER] {
        payload[offset..offset + prefix.len()].copy_from_slice(prefix);
        payload[offset + 146..offset + 146 + prefix.len()].copy_from_slice(prefix);
        assert_eq!(
            super::linked_profile_curve_endpoint_indices(&payload, offset),
            Some([2, 3])
        );
    }
}


#[test]
fn extended_linked_line_uses_inline_self_endpoint() {
    let mut payload = vec![0; 146 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.007f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.0075f64.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    for (relative, endpoint) in [(78, 2u16), (86, 5u16)] {
        payload[relative..relative + 2].copy_from_slice(&0x810cu16.to_le_bytes());
        payload[relative + 2..relative + 4].copy_from_slice(&endpoint.to_le_bytes());
        payload[relative + 4..relative + 8].fill(0xff);
    }
    payload[94..100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[142..146].fill(0xff);
    payload[146..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let mut external = SketchInputEntity {
        id: "external".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 1,
        offset: 0,
        object_index: Some(3),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some([0.0, 0.0075]),
        links: Vec::new(),
        link_selector: None,
    };
    let mut curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 2,
        offset: 0,
        object_index: Some(6),
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };

    assert_eq!(
        extended_linked_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        Some([[0.0, 0.0075], [0.007, 0.0075]])
    );
    payload[80..82].copy_from_slice(&1u16.to_le_bytes());
    payload[88..90].copy_from_slice(&4u16.to_le_bytes());
    payload[136..140].copy_from_slice(&1u32.to_le_bytes());
    external.object_index = Some(1);
    curve.object_index = Some(4);
    assert_eq!(
        extended_linked_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        Some([[0.0, 0.0075], [0.007, 0.0075]])
    );
    payload[140] = 1;
    assert_eq!(
        extended_linked_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        None
    );
}


#[test]
fn extended_identity_line_uses_inline_and_identified_point_endpoints() {
    let mut payload = vec![0; 134 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.007f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.0075f64.to_le_bytes());
    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[82..84].copy_from_slice(&1u16.to_le_bytes());
    payload[84..88].copy_from_slice(&(-2i32).to_le_bytes());
    payload[130..134].copy_from_slice(&5u32.to_le_bytes());
    payload[134..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let point = SketchInputEntity {
        id: "point".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 1,
        offset: 200,
        object_index: Some(5),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some([0.01, 0.012]),
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 2,
        offset: 0,
        object_index: Some(6),
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: Some([0.007, 0.0075]),
        links: Vec::new(),
        link_selector: None,
    };

    assert_eq!(
        extended_identity_inline_line_endpoints(&payload, &curve, &[&point, &curve]),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    let chained_curve = SketchInputEntity {
        id: "chained-curve".into(),
        kind: SketchInputKind::Arc,
        ..point.clone()
    };
    assert_eq!(
        extended_identity_inline_line_endpoints(&payload, &curve, &[&chained_curve, &curve],),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        extended_identity_inline_line_endpoints(&payload, &curve, &[&point, &curve]),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );
    payload[74..84]
        .copy_from_slice(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    payload[126..130].copy_from_slice(&4u32.to_le_bytes());
    let direct_curve = SketchInputEntity {
        kind: SketchInputKind::Arc,
        ..curve.clone()
    };
    assert_eq!(
        extended_identity_inline_line_endpoints(
            &payload,
            &direct_curve,
            &[&chained_curve, &direct_curve],
        ),
        Some([[0.007, 0.0075], [0.01, 0.012]])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );
    payload[126..130].fill(0);
    assert_eq!(
        extended_identity_inline_line_endpoints(
            &payload,
            &direct_curve,
            &[&chained_curve, &direct_curve],
        ),
        None
    );
    payload[126..130].copy_from_slice(&4u32.to_le_bytes());
    let duplicate = SketchInputEntity {
        id: "duplicate".into(),
        ..point.clone()
    };
    assert_eq!(
        extended_identity_inline_line_endpoints(
            &payload,
            &curve,
            &[&point, &duplicate, &curve],
        ),
        None
    );
    payload[130..134].fill(0);
    assert_eq!(
        extended_identity_inline_line_endpoints(&payload, &curve, &[&point, &curve]),
        None
    );
}


#[test]
fn extended_declared_line_uses_its_typed_point_selector() {
    let mut payload = vec![0; 170 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.0165f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.029f64.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[78..84].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]);
    payload[84..96].copy_from_slice(b"sgLineHandle");
    payload[96..106].copy_from_slice(&[0x08, 0x00, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    payload[106..108].copy_from_slice(&0x8155u16.to_le_bytes());
    payload[108..110].copy_from_slice(&7u16.to_le_bytes());
    payload[110..114].fill(0xff);
    payload[118..124].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[166..170].copy_from_slice(&4u32.to_le_bytes());
    payload[170..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let external = SketchInputEntity {
        id: "external".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 7,
        offset: 0,
        object_index: Some(7),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some([0.014, 0.016]),
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 3,
        offset: 0,
        object_index: Some(3),
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };

    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        Some([[0.014, 0.016], [0.0165, 0.029]])
    );
    payload[96..98].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        Some([[0.014, 0.016], [0.0165, 0.029]])
    );
    payload[96..98].fill(0);
    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        None
    );
    payload[96..98].fill(0xff);
    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        None
    );
    payload[96..98].copy_from_slice(&8u16.to_le_bytes());
    payload[110] = 0;
    assert_eq!(
        extended_declared_inline_line_endpoints(&payload, &curve, &[&external, &curve]),
        None
    );
}


#[test]
fn compact_indexed_curve_stores_endpoints_in_both_generations() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&6u16.to_le_bytes());
    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    payload[80..84].copy_from_slice(&19u32.to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[84..84 + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Arc
    );

    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    assert!(!marker_is_selected_construction_line(&payload, 0));
    payload[17..21].fill(0);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x45, 0x00]);
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    assert!(current_undetailed_bounded_curve_is_line(&payload, 0));
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[60..64].fill(0);

    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[56..58].copy_from_slice(&30u16.to_le_bytes());
    payload[58..60].copy_from_slice(&31u16.to_le_bytes());
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([31, 32])
    );
    assert_eq!(marker_coordinates(&payload, 0), None);
    payload[56..58].copy_from_slice(&6u16.to_le_bytes());
    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(compact_indexed_curve_endpoint_indices(&payload, 0), None);
}


#[test]
fn direct_indexed_curve_stores_feature_local_point_ids() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&6u16.to_le_bytes());
    payload[58..60].copy_from_slice(&15u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        direct_indexed_curve_endpoint_indices(&payload, 0),
        Some([6, 15])
    );
    assert_eq!(compact_indexed_curve_endpoint_indices(&payload, 0), None);
    payload[58..60].copy_from_slice(&6u16.to_le_bytes());
    assert_eq!(direct_indexed_curve_endpoint_indices(&payload, 0), None);
    payload[58..60].copy_from_slice(&15u16.to_le_bytes());
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    assert_eq!(direct_indexed_curve_endpoint_indices(&payload, 0), None);
}


#[test]
fn extended_direct_object_line_uses_exact_point_identities() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x44, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&0u16.to_le_bytes());
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[76..84].copy_from_slice(&3u64.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_direct_object_line_endpoint_ids(&payload, 0),
        Some([0, 4])
    );
    payload[17..21].fill(0);
    assert_eq!(
        extended_direct_object_line_endpoint_ids(&payload, 0),
        Some([0, 4])
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[37] = 0x04;
    assert_eq!(extended_direct_object_line_endpoint_ids(&payload, 0), None);
    payload[37] = 0x44;

    let entity = |id: &str, object_index, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset: 0,
        object_index,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        kind: SketchInputKind::LineOrCircle,
        ..entity("curve", Some(2), None)
    };
    let implicit = entity("implicit", None, Some([1.0, 2.0]));
    let explicit = entity("explicit", Some(4), Some([3.0, 4.0]));
    let markers = [&curve, &implicit, &explicit];
    assert_eq!(
        extended_direct_object_line_endpoints(&payload, &curve, &markers)
            .map(|endpoints| endpoints.map(|endpoint| endpoint.id.as_str())),
        Some(["implicit", "explicit"])
    );
    let arc = SketchInputEntity {
        kind: SketchInputKind::Arc,
        ..curve.clone()
    };
    assert_eq!(
        extended_direct_object_line_endpoints(&payload, &arc, &markers),
        None
    );
    let wrong_first = entity("wrong-first", Some(5), Some([5.0, 6.0]));
    let wrong_second = entity("wrong-second", Some(6), Some([7.0, 8.0]));
    let mut linked_curve = curve.clone();
    linked_curve.links = vec![
        SketchInputLink {
            local_id: 5,
            entity_ref: wrong_first.id.clone(),
        },
        SketchInputLink {
            local_id: 6,
            entity_ref: wrong_second.id.clone(),
        },
    ];
    let markers = [
        &linked_curve,
        &implicit,
        &explicit,
        &wrong_first,
        &wrong_second,
    ];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), *marker))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        marker_curve_endpoint_markers(&payload, &linked_curve, &markers_by_id, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["implicit", "explicit"]
    );

    payload[58..60].fill(0);
    assert_eq!(extended_direct_object_line_endpoint_ids(&payload, 0), None);
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[37] = 0x0c;
    assert_eq!(extended_direct_object_line_endpoint_ids(&payload, 0), None);
    payload[37] = 0x44;
    payload[74] = 2;
    assert_eq!(extended_direct_object_line_endpoint_ids(&payload, 0), None);
}


#[test]
fn legacy_state_five_identity_curve_uses_coordinate_roster_indices() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&6u16.to_le_bytes());
    payload[58..60].copy_from_slice(&9u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[76..80].copy_from_slice(&11u32.to_le_bytes());
    payload[80..84].copy_from_slice(&25u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&payload, 0),
        Some([7, 10])
    );
    assert_eq!(
        super::coordinate_roster_endpoint_offset(&payload, 0),
        Some(56)
    );

    payload[80..84].copy_from_slice(&11u32.to_le_bytes());
    assert_eq!(legacy_state_five_curve_endpoint_indices(&payload, 0), None);
    payload[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(legacy_state_five_curve_endpoint_indices(&payload, 0), None);
}


#[test]
fn extended_tagged_indexed_curve_uses_direct_point_ids() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..60].copy_from_slice(&31u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&24u16.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_tagged_indexed_curve_endpoint_indices(&payload, 0),
        Some([31, 24])
    );
    assert_eq!(marker_coordinates(&payload, 0), None);
    payload[76..78].copy_from_slice(&31u16.to_le_bytes());
    assert_eq!(
        extended_tagged_indexed_curve_endpoint_indices(&payload, 0),
        None
    );

    payload[76..78].copy_from_slice(&24u16.to_le_bytes());
    payload.resize(370, 0);
    payload[94..150].fill(0);
    payload[150..152].copy_from_slice(&[0x08, 0x80]);
    payload[152..162].fill(0);
    payload[162..166].copy_from_slice(&[0x01, 0x00, 0x01, 0x00]);
    for (relative, count) in [(166, 65u32), (170, 57), (174, 33), (178, 13)] {
        payload[relative..relative + 4].copy_from_slice(&count.to_le_bytes());
    }
    for relative in (182..230).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&1u32.to_le_bytes());
    }
    payload[230..258].copy_from_slice(&[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xfe, 0xff, 0x00, 0xff, 0xff, 0x00, 0x00, 0x80,
        0xbf, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
    ]);
    payload[258..282].fill(0);
    payload[282..286].copy_from_slice(&49u32.to_le_bytes());
    payload[286..338].fill(0);
    payload[338..342].copy_from_slice(&3u32.to_le_bytes());
    payload[342..346].copy_from_slice(&1u32.to_le_bytes());
    payload[346..353].fill(0);
    payload[353..357].copy_from_slice(&0x0001_86a5u32.to_le_bytes());
    payload[357..359].copy_from_slice(&5u16.to_le_bytes());
    payload[359..363].copy_from_slice(CLASS_MARKER);
    payload[363..365].copy_from_slice(&5u16.to_le_bytes());
    payload[365..370].copy_from_slice(b"class");
    assert_eq!(
        extended_tagged_indexed_curve_endpoint_indices(&payload, 0),
        Some([31, 24])
    );
    payload[338..342].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        extended_tagged_indexed_curve_endpoint_indices(&payload, 0),
        None
    );
}


#[test]
fn extended_compact_curve_resolves_zero_based_point_object_ids() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&16u16.to_le_bytes());
    payload[58..60].copy_from_slice(&0u16.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, object_index, coordinates_m, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset: 0,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        entity("curve", Some(8), None, SketchInputKind::LineOrCircle),
        entity(
            "explicit",
            Some(16),
            Some([0.0, 0.006]),
            SketchInputKind::Point,
        ),
        entity(
            "implicit-zero",
            None,
            Some([0.0, 0.0]),
            SketchInputKind::Point,
        ),
        entity(
            "explicit-fourteen",
            Some(14),
            Some([0.022, 0.0075]),
            SketchInputKind::Point,
        ),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit", "implicit-zero"]
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit", "implicit-zero"]
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    let duplicate = entity(
        "duplicate-zero",
        None,
        Some([1.0, 0.0]),
        SketchInputKind::Point,
    );
    let ambiguous = [&entities[0], &entities[1], &entities[2], &duplicate];
    assert!(extended_compact_endpoint_markers(&payload, &entities[0], &ambiguous).is_empty());

    payload.resize(96 + LEGACY_EXTENDED_SKETCH_MARKER.len(), 0);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..96].fill(0);
    payload[82..84].copy_from_slice(&2u16.to_le_bytes());
    payload[88..92].copy_from_slice(&2u32.to_le_bytes());
    payload[92..96].copy_from_slice(&1u32.to_le_bytes());
    payload[96..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit", "implicit-zero"]
    );
    payload[82..84].fill(0);
    assert!(extended_compact_endpoint_markers(&payload, &entities[0], &markers).is_empty());

    payload.resize(102, 0);
    payload[56..58].copy_from_slice(&14u16.to_le_bytes());
    payload[58..60].copy_from_slice(&16u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..102].fill(0);
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit-fourteen", "explicit"]
    );
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit-fourteen", "explicit"]
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());

    let mut roster_indexed = entities.clone();
    roster_indexed[1].object_index = None;
    roster_indexed[1].ordinal = 16;
    roster_indexed[3].object_index = None;
    roster_indexed[3].ordinal = 14;
    let markers = roster_indexed.iter().collect::<Vec<_>>();
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &roster_indexed[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit-fourteen", "explicit"]
    );

    payload.resize(116, 0);
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    payload[60..64].fill(0);
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..116].fill(0);
    assert_eq!(
        extended_compact_endpoint_markers(&payload, &roster_indexed[0], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["explicit", "implicit-zero"]
    );
}


#[test]
fn wide_profile_curves_index_the_coordinate_roster() {
    let curve_offset = 402;
    let mut payload = vec![0; curve_offset + 92 + LEGACY_SKETCH_MARKER.len()];
    for (offset, coordinate) in [
        (0, [1.0_f64, 2.0]),
        (134, [3.0_f64, 4.0]),
        (268, [5.0_f64, 6.0]),
    ] {
        payload[offset..offset + LEGACY_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&coordinate[0].to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&coordinate[1].to_le_bytes());
    }
    payload[curve_offset..curve_offset + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[curve_offset + 5..curve_offset + 13].fill(0xff);
    payload[curve_offset + 13..curve_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[curve_offset + 23..curve_offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[curve_offset + 27..curve_offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 31..curve_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[curve_offset + 48..curve_offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&0u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset + 68..curve_offset + 72].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 72..curve_offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 92..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let mut entities = sketch_input_entities(&payload, "lane");
    entities.truncate(4);
    for entity in &mut entities {
        entity.feature_ref = Some("sketch".into());
    }
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([1.0, 2.0]), Some([5.0, 6.0])]
    );

    payload[curve_offset..curve_offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[curve_offset + 92..curve_offset + 92 + SKETCH_MARKER.len()]
        .copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([1.0, 2.0]), Some([5.0, 6.0])]
    );

    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&3u16.to_le_bytes());
    payload[curve_offset + 84..curve_offset + 88].copy_from_slice(&4u32.to_le_bytes());
    payload[curve_offset + 88..curve_offset + 92].copy_from_slice(&7u32.to_le_bytes());
    assert!(current_identity_linked_wide_curve_uses_one_based_roster(
        &payload,
        curve_offset
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([1.0, 2.0]), Some([5.0, 6.0])]
    );

    payload[curve_offset + 84..curve_offset + 88].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 29..curve_offset + 31].copy_from_slice(&1u16.to_le_bytes());
    assert!(current_direct_92_profile_line_endpoint_indices(&payload, curve_offset).is_some());
    assert!(!current_identity_linked_wide_curve_uses_one_based_roster(
        &payload,
        curve_offset
    ));

    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset + 29..curve_offset + 31].fill(0);
    payload[curve_offset + 84..curve_offset + 92].fill(0);
    let mut centered_entities = entities.clone();
    centered_entities[0].coordinates_m = Some([0.0, 0.0]);
    centered_entities[0].kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    centered_entities[1].coordinates_m = Some([1.0, 0.0]);
    centered_entities[2].coordinates_m = Some([0.0, 1.0]);
    let centered_markers = centered_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        coordinate_roster_arc_center(
            &payload,
            &centered_entities[3],
            &centered_markers,
            [&centered_entities[1], &centered_entities[2]],
        ),
        Some([0.0, 0.0])
    );
    let mut hybrid_entities = centered_entities.clone();
    let mut additional_endpoint = hybrid_entities[2].clone();
    additional_endpoint.id.push_str(":additional");
    additional_endpoint.offset += 1;
    additional_endpoint.coordinates_m = Some([-1.0, 0.0]);
    hybrid_entities.insert(3, additional_endpoint);
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&1u16.to_le_bytes());
    let hybrid_markers = hybrid_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        coordinate_roster_arc_center(
            &payload,
            &hybrid_entities[4],
            &hybrid_markers,
            [&hybrid_entities[3], &hybrid_entities[2]],
        ),
        Some([0.0, 0.0])
    );
    hybrid_entities[0].coordinates_m = Some([4.0, 4.0]);
    hybrid_entities[1].coordinates_m = Some([0.0, 0.0]);
    hybrid_entities[1].object_index = Some(0);
    let hybrid_markers = hybrid_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        coordinate_roster_arc_center(
            &payload,
            &hybrid_entities[4],
            &hybrid_markers,
            [&hybrid_entities[3], &hybrid_entities[2]],
        ),
        Some([0.0, 0.0])
    );
    payload[curve_offset + 64..curve_offset + 66].copy_from_slice(&0u16.to_le_bytes());
    payload[curve_offset + 66..curve_offset + 68].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset..curve_offset + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[curve_offset + 92..curve_offset + 92 + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);

    payload[curve_offset + 23..curve_offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[curve_offset + 35..curve_offset + 39].copy_from_slice(&[0x00, 0x00, 0x05, 0x00]);
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([1.0, 2.0]), Some([5.0, 6.0])]
    );
    payload[curve_offset + 23..curve_offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[curve_offset + 35..curve_offset + 39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);

    payload[curve_offset + 56..curve_offset + 58].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 58..curve_offset + 60].copy_from_slice(&2u16.to_le_bytes());
    payload[curve_offset + 84..curve_offset + 84 + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([3.0, 4.0]), Some([5.0, 6.0])]
    );
    assert!(legacy_undetailed_profile_line(&payload, curve_offset));

    payload[curve_offset..curve_offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[curve_offset + 84..curve_offset + 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&payload, curve_offset),
        Some([2, 3])
    );
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.coordinates_m)
            .collect::<Vec<_>>(),
        vec![Some([3.0, 4.0]), Some([5.0, 6.0])]
    );

    payload.resize(curve_offset + 104 + LEGACY_EXTENDED_SKETCH_MARKER.len(), 0);
    payload[curve_offset + 84..].fill(0);
    payload[curve_offset + 60..curve_offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 64..curve_offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 72..curve_offset + 76].copy_from_slice(&1i32.to_le_bytes());
    for at in (curve_offset + 78..curve_offset + 94).step_by(4) {
        payload[at..at + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[curve_offset + 104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let mut complete_roster_entities = entities.clone();
    complete_roster_entities[0].coordinates_m = None;
    complete_roster_entities[0].kind =
        SketchInputKind::Relation(SketchRelationKind::Horizontal);
    let complete_roster_markers = complete_roster_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        roster_curve_endpoint_markers(
            &payload,
            &complete_roster_entities[3],
            &complete_roster_markers,
        )
        .iter()
        .map(|marker| marker.coordinates_m)
        .collect::<Vec<_>>(),
        vec![Some([3.0, 4.0]), Some([5.0, 6.0])]
    );
    payload[curve_offset + 56..curve_offset + 58].fill(0);
    assert!(roster_curve_endpoint_markers(
        &payload,
        &complete_roster_entities[3],
        &complete_roster_markers,
    )
    .is_empty());
}


#[test]
fn current_coordinate_circle_uses_its_complete_square_handle_grid() {
    let mut payload = vec![0; 284];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].copy_from_slice(&[0xff; 8]);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[82..86].copy_from_slice(&1u32.to_le_bytes());
    payload[92..96].copy_from_slice(&(-2i32).to_le_bytes());
    payload[142..142 + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, ordinal, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let center = entity("center", 0, 0, SketchInputKind::Arc, Some([2.0, 3.0]));
    let points = [
        [1.0, 2.0],
        [2.0, 2.0],
        [3.0, 2.0],
        [1.0, 4.0],
        [2.0, 4.0],
        [3.0, 4.0],
    ]
    .into_iter()
    .enumerate()
    .map(|(index, point)| {
        entity(
            &format!("point-{index}"),
            index as u32 + 1,
            index as u64 + 143,
            SketchInputKind::Point,
            Some(point),
        )
    })
    .collect::<Vec<_>>();
    let mut entities = vec![center.clone()];
    entities.extend(points);
    let markers = entities.iter().collect::<Vec<_>>();
    assert_eq!(
        coordinate_circle_radius(&payload, &center, &markers),
        Some(1.0)
    );
    entities[6].coordinates_m = Some([3.0, 5.0]);
    let markers = entities.iter().collect::<Vec<_>>();
    assert_eq!(coordinate_circle_radius(&payload, &center, &markers), None);
}


#[test]
fn legacy_coordinate_circle_uses_its_trailing_radial_point() {
    let mut payload = vec![0; 162 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[66..74].copy_from_slice(&0.037f64.to_le_bytes());
    payload[74..82].copy_from_slice(&0.012f64.to_le_bytes());
    payload[84..86].copy_from_slice(&2u16.to_le_bytes());
    payload[86..90].copy_from_slice(&[0x19, 0x82, 0x02, 0x00]);
    payload[90..94].fill(0xff);
    payload[98..102].copy_from_slice(&[0x19, 0x82, 0x01, 0x00]);
    payload[102..106].fill(0xff);
    payload[110..116].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[158..162].copy_from_slice(&21u32.to_le_bytes());
    payload[162..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let entity = |id: &str, ordinal, offset, object_index, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal,
        offset,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let circle = entity(
        "circle",
        10,
        0,
        Some(20),
        SketchInputKind::Arc,
        Some([0.037, 0.012]),
    );
    let radial = entity(
        "radial",
        11,
        162,
        Some(21),
        SketchInputKind::Point,
        Some([0.049, 0.012]),
    );

    assert!(
        legacy_coordinate_circle_radius(&payload, &circle, &[&circle, &radial])
            .is_some_and(|radius| super::same_dimension_length(radius, 0.012))
    );
    payload[158..162].copy_from_slice(&22u32.to_le_bytes());
    assert_eq!(
        legacy_coordinate_circle_radius(&payload, &circle, &[&circle, &radial]),
        None
    );
}


#[test]
fn extended_full_circle_uses_center_and_radial_point_roster() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[0x02, 0x00, 0x02, 0x00]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[78..94].copy_from_slice(&[
        0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff,
        0xff,
    ]);
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        entity("center", 1, SketchInputKind::Point, Some([0.0, 0.0])),
        entity("inner", 2, SketchInputKind::Point, Some([3.0, 0.0])),
        entity("radial", 3, SketchInputKind::Point, Some([0.0, 4.0])),
        entity("circle", 0, SketchInputKind::Arc, None),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        coordinate_roster_full_circle(&payload, &entities[3], &markers),
        Some(([0.0, 0.0], 4.0))
    );
    payload[58..60].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        coordinate_roster_full_circle(&payload, &entities[3], &markers),
        None
    );
}


#[test]
fn extended_profile_circle_accepts_one_unambiguous_radial_interpretation() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[0x03, 0x00, 0x03, 0x00]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[78..94].copy_from_slice(&[
        0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff,
        0xff,
    ]);
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, offset, object_index, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        entity(
            "center",
            1,
            Some(2),
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        entity(
            "direct",
            2,
            Some(3),
            SketchInputKind::Point,
            Some([3.0, 0.0]),
        ),
        entity(
            "roster",
            3,
            Some(4),
            SketchInputKind::Point,
            Some([3.0, 0.0]),
        ),
        entity("circle", 0, Some(1), SketchInputKind::LineOrCircle, None),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        super::compact_profile_full_circle(&payload, &entities[3], &markers),
        Some(([0.0, 0.0], 3.0))
    );
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[104..].copy_from_slice(SKETCH_MARKER);
    let mut current_circle = entities[3].clone();
    current_circle.kind = SketchInputKind::Arc;
    assert_eq!(
        super::compact_profile_full_circle(&payload, &current_circle, &markers),
        Some(([0.0, 0.0], 3.0))
    );
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[56..60].copy_from_slice(&[0x01, 0x00, 0x01, 0x00]);
    payload[104..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        super::equal_index_coordinate_roster_full_circle(&payload, &current_circle, &markers,),
        Some(([0.0, 0.0], 3.0))
    );
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[56..60].copy_from_slice(&[0x03, 0x00, 0x03, 0x00]);
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    let mut conflicting = entities.clone();
    conflicting[1].coordinates_m = Some([4.0, 0.0]);
    let markers = conflicting.iter().collect::<Vec<_>>();
    assert_eq!(
        super::compact_profile_full_circle(&payload, &conflicting[3], &markers),
        None
    );
}


#[test]
fn compact_legacy_repeated_radial_records_define_full_circles() {
    let mut record = vec![0; 90 + LEGACY_SKETCH_MARKER.len()];
    record[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    record[5..13].fill(0xff);
    record[13..17].copy_from_slice(&1u32.to_le_bytes());
    record[19..25].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    record[25..27].copy_from_slice(&1u16.to_le_bytes());
    record[31] = 4;
    record[42..46].copy_from_slice(&[1, 0, 1, 0]);
    record[46..50].copy_from_slice(&1u32.to_le_bytes());
    record[50..58].copy_from_slice(&(-1.0f64).to_le_bytes());
    record[58..62].copy_from_slice(&1u32.to_le_bytes());
    for cell in record[64..80].chunks_exact_mut(4) {
        cell.copy_from_slice(&(-2i32).to_le_bytes());
    }
    record[82..86].copy_from_slice(&2u32.to_le_bytes());
    record[86..90].copy_from_slice(&3u32.to_le_bytes());
    record[90..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let circle_offset = 500;
    let mut payload = vec![0; circle_offset + record.len()];
    for marker_offset in [0, 100, 200, 300] {
        payload[marker_offset..marker_offset + LEGACY_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[marker_offset + 5..marker_offset + 13].fill(0xff);
        payload[marker_offset + 13..marker_offset + 17].copy_from_slice(&1u32.to_le_bytes());
        payload[marker_offset + 19..marker_offset + 25]
            .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[marker_offset + 31] = 4;
    }
    payload[205..213].fill(0);
    payload[circle_offset..].copy_from_slice(&record);
    let entity = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        entity("center", 0, SketchInputKind::Point, Some([0.0, 0.0])),
        entity("radial", 100, SketchInputKind::Point, Some([0.0, 12.0])),
        entity("handle", 200, SketchInputKind::Native(1), None),
        entity(
            "terminal-radial",
            300,
            SketchInputKind::Point,
            Some([0.0, 5.5]),
        ),
        entity(
            "circle",
            circle_offset as u64,
            SketchInputKind::LineOrCircle,
            None,
        ),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        super::compact_legacy_profile_full_circle(&payload, &entities[4], &markers),
        Some(([0.0, 0.0], 12.0))
    );

    payload.resize(circle_offset + 131, 0);
    payload[circle_offset + 42..circle_offset + 46].copy_from_slice(&[3, 0, 3, 0]);
    payload[circle_offset + 82..circle_offset + 112].fill(0);
    payload[circle_offset + 112..circle_offset + 114].copy_from_slice(&4u16.to_le_bytes());
    payload[circle_offset + 114..circle_offset + 118].copy_from_slice(CLASS_MARKER);
    payload[circle_offset + 118..circle_offset + 120].copy_from_slice(&11u16.to_le_bytes());
    payload[circle_offset + 120..circle_offset + 131].copy_from_slice(b"sgCircleDim");
    assert_eq!(
        super::compact_legacy_profile_full_circle(&payload, &entities[4], &markers),
        Some(([0.0, 0.0], 5.5))
    );
    payload[circle_offset + 120] = b'x';
    assert_eq!(
        super::compact_legacy_profile_full_circle(&payload, &entities[4], &markers),
        None
    );
}


#[test]
fn packed_compact_legacy_curves_use_the_coordinate_roster() {
    let mut payload = vec![0; 76 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[19..25].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[29] = 5;
    payload[40..48].copy_from_slice(&1.0f64.to_le_bytes());
    payload[48..52].copy_from_slice(&[1, 0, 2, 0]);
    payload[56..64].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[66..68].copy_from_slice(&1u16.to_le_bytes());
    payload[72..76].copy_from_slice(&3u32.to_le_bytes());
    payload[76..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        super::packed_compact_legacy_curve_endpoint_indices(&payload, 0),
        Some([1, 2])
    );
    assert_eq!(
        super::coordinate_roster_endpoint_offset(&payload, 0),
        Some(48)
    );
    assert!(super::legacy_undetailed_profile_line(&payload, 0));
    assert!(!super::marker_is_selected_construction_line(&payload, 0));

    payload[13..17].copy_from_slice(&1u32.to_le_bytes());
    payload[23..25].copy_from_slice(&2u16.to_le_bytes());
    payload[29] = 12;
    payload[66..68].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        super::packed_compact_legacy_curve_endpoint_indices(&payload, 0),
        Some([1, 2])
    );
    assert!(!super::legacy_undetailed_profile_line(&payload, 0));
    assert!(super::marker_is_selected_construction_line(&payload, 0));

    payload[68] = 1;
    assert_eq!(
        super::packed_compact_legacy_curve_endpoint_indices(&payload, 0),
        None
    );
}


#[test]
fn sole_out_of_roster_packed_curve_closes_one_open_profile_chain() {
    let mut payload = vec![0; 328 + LEGACY_SKETCH_MARKER.len()];
    for point_offset in [0, 10, 20] {
        payload[point_offset..point_offset + LEGACY_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_SKETCH_MARKER);
    }
    for (curve_offset, endpoints, identity) in [
        (100, [0u16, 1], 1u32),
        (176, [1u16, 2], 2),
        (252, [3u16, 4], 3),
    ] {
        payload[curve_offset..curve_offset + LEGACY_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[curve_offset + 5..curve_offset + 13].fill(0xff);
        payload[curve_offset + 19..curve_offset + 25]
            .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[curve_offset + 29] = 5;
        payload[curve_offset + 40..curve_offset + 48].copy_from_slice(&1.0f64.to_le_bytes());
        payload[curve_offset + 48..curve_offset + 50].copy_from_slice(&endpoints[0].to_le_bytes());
        payload[curve_offset + 50..curve_offset + 52].copy_from_slice(&endpoints[1].to_le_bytes());
        payload[curve_offset + 56..curve_offset + 64].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[curve_offset + 72..curve_offset + 76].copy_from_slice(&identity.to_le_bytes());
    }
    payload[328..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let entity = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        entity("point-0", 0, SketchInputKind::Point, Some([0.0, 0.0])),
        entity("point-1", 10, SketchInputKind::Point, Some([1.0, 0.0])),
        entity("point-2", 20, SketchInputKind::Point, Some([1.0, 1.0])),
        entity("line-0", 100, SketchInputKind::LineOrCircle, None),
        entity("line-1", 176, SketchInputKind::LineOrCircle, None),
        entity("closure", 252, SketchInputKind::LineOrCircle, None),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        super::implicit_profile_chain_closure_endpoints(&payload, &entities[5], &markers),
        Some([[0.0, 0.0], [1.0, 1.0]])
    );

    payload[176 + 48..176 + 52].copy_from_slice(&[0, 0, 1, 0]);
    assert_eq!(
        super::implicit_profile_chain_closure_endpoints(&payload, &entities[5], &markers),
        None
    );
}


#[test]
fn equal_index_coordinate_roster_carries_center_and_following_radial_point() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x01, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[2, 0, 2, 0]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1u32.to_le_bytes());
    for cell in payload[78..94].chunks_exact_mut(4) {
        cell.copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let marker = |id: &str, offset, coordinates_m, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let circle = marker("circle", 0, None, SketchInputKind::Arc);
    let points = [
        marker("first", 10, Some([0.0, 0.0]), SketchInputKind::Point),
        marker("center", 20, Some([1.0, 1.0]), SketchInputKind::Point),
        marker("radial", 30, Some([1.0, 3.0]), SketchInputKind::Point),
    ];
    let markers = std::iter::once(&circle)
        .chain(points.iter())
        .collect::<Vec<_>>();

    assert_eq!(
        equal_index_coordinate_roster_full_circle(&payload, &circle, &markers),
        Some(([1.0, 1.0], 2.0))
    );
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    assert_eq!(
        equal_index_coordinate_roster_full_circle(&payload, &circle, &markers),
        Some(([1.0, 1.0], 2.0))
    );
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[104..104 + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        equal_index_coordinate_roster_full_circle(&payload, &circle, &markers),
        Some(([1.0, 1.0], 2.0))
    );
}


#[test]
fn wide_legacy_full_circle_uses_adjacent_center_and_radial_markers() {
    let mut payload = vec![0; 112 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..68].copy_from_slice(&[0x02, 0x00, 0x02, 0x00]);
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&1i32.to_le_bytes());
    payload[84..86].copy_from_slice(&4u16.to_le_bytes());
    payload[86..102].copy_from_slice(&[
        0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
        0xff, 0xff,
    ]);
    payload[104..108].copy_from_slice(&6u32.to_le_bytes());
    payload[108..112].copy_from_slice(&3u32.to_le_bytes());
    payload[112..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let entity = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        entity("unrelated", 1, SketchInputKind::Point, Some([9.0, 9.0])),
        entity("center", 2, SketchInputKind::Arc, Some([2.0, 3.0])),
        entity("radial", 3, SketchInputKind::Point, Some([5.0, 7.0])),
        entity("circle", 0, SketchInputKind::LineOrCircle, None),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        super::wide_coordinate_roster_full_circle(&payload, &entities[3], &markers),
        Some(([2.0, 3.0], 5.0))
    );
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[112..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let mut extended_circle = entities[3].clone();
    extended_circle.kind = SketchInputKind::LineOrCircle;
    assert_eq!(
        super::wide_coordinate_roster_full_circle(&payload, &extended_circle, &markers),
        Some(([2.0, 3.0], 5.0))
    );
    payload[104..108].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        super::wide_coordinate_roster_full_circle(&payload, &extended_circle, &markers),
        Some(([2.0, 3.0], 5.0))
    );
    payload[104..108].copy_from_slice(&6u32.to_le_bytes());
    let mut terminal = payload[..102].to_vec();
    terminal.resize(153, 0);
    terminal[134..136].copy_from_slice(&[0x04, 0x00]);
    terminal[136..140].copy_from_slice(CLASS_MARKER);
    terminal[140..142].copy_from_slice(&11u16.to_le_bytes());
    terminal[142..153].copy_from_slice(b"sgCircleDim");
    terminal[64..68].copy_from_slice(&[0x03, 0x00, 0x03, 0x00]);
    let mut terminal_entities = entities.clone();
    terminal_entities[0].kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    let terminal_markers = terminal_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        super::wide_coordinate_roster_full_circle(
            &terminal,
            &extended_circle,
            &terminal_markers,
        ),
        Some(([2.0, 3.0], 5.0))
    );
    terminal[133] = 1;
    assert_eq!(
        super::wide_coordinate_roster_full_circle(
            &terminal,
            &extended_circle,
            &terminal_markers,
        ),
        None
    );
    payload[66..68].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        super::wide_coordinate_roster_full_circle(&payload, &entities[3], &markers),
        None
    );
}


#[test]
fn legacy_profile_radial_circle_requires_one_selected_radial_locus() {
    let mut payload = vec![0; 112 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..68].copy_from_slice(&[0x01, 0x00, 0x01, 0x00]);
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&1i32.to_le_bytes());
    payload[86..102].copy_from_slice(&[
        0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff,
        0xff,
    ]);
    payload[104..108].copy_from_slice(&2u32.to_le_bytes());
    payload[108..112].copy_from_slice(&2u32.to_le_bytes());
    payload[112..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let entity = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        entity("center", 1, SketchInputKind::LineOrCircle, Some([0.0, 0.0])),
        entity("radial", 2, SketchInputKind::Point, Some([3.0, 0.0])),
        entity("other", 3, SketchInputKind::Point, Some([0.0, 4.0])),
        entity("circle", 0, SketchInputKind::LineOrCircle, None),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert_eq!(
        super::legacy_profile_radial_circle(&payload, &entities[3], &markers),
        Some(([0.0, 0.0], 3.0))
    );
    payload[64..68].copy_from_slice(&[0x02, 0x00, 0x02, 0x00]);
    assert_eq!(
        super::legacy_profile_radial_circle(&payload, &entities[3], &markers),
        None
    );

    payload.resize(128, 0);
    payload[64..68].copy_from_slice(&[0x03, 0x00, 0x03, 0x00]);
    payload[104..128].fill(0);
    assert_eq!(
        super::legacy_profile_radial_circle(&payload, &entities[3], &markers),
        Some(([0.0, 0.0], 4.0))
    );
}


#[test]
fn compact_legacy_linked_coordinate_uses_the_1a_pair() {
    let mut payload = vec![0; 154 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1a, 0x00]);
    payload[58..66].copy_from_slice(&0.0025f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.01f64.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    for (start, local_id) in [(78, 2u16), (90, 5u16)] {
        payload[start..start + 2].copy_from_slice(&[0x2b, 0x82]);
        payload[start + 2..start + 4].copy_from_slice(&local_id.to_le_bytes());
        payload[start + 4..start + 8].fill(0xff);
    }
    payload[102..108].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[150..154].copy_from_slice(&29u32.to_le_bytes());
    payload[154..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(legacy_linked_coordinates(&payload, 0), Some([0.0025, 0.01]));
    assert_eq!(marker_coordinates(&payload, 0), Some([0.0025, 0.01]));
    payload[90] ^= 1;
    assert_eq!(legacy_linked_coordinates(&payload, 0), None);
    assert_eq!(marker_coordinates(&payload, 0), None);

    let mut compact = vec![0; 146 + LEGACY_SKETCH_MARKER.len()];
    compact[..78].copy_from_slice(&payload[..78]);
    for (start, local_id) in [(78, 2u16), (86, 5u16)] {
        compact[start..start + 2].copy_from_slice(&[0x2b, 0x82]);
        compact[start + 2..start + 4].copy_from_slice(&local_id.to_le_bytes());
        compact[start + 4..start + 8].fill(0xff);
    }
    compact[94..100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    compact[138..142].copy_from_slice(&17u32.to_le_bytes());
    compact[142..146].copy_from_slice(&29u32.to_le_bytes());
    compact[146..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(legacy_linked_coordinates(&compact, 0), Some([0.0025, 0.01]));
    assert_eq!(marker_coordinates(&compact, 0), Some([0.0025, 0.01]));

    let mut shifted = vec![0; 162 + LEGACY_SKETCH_MARKER.len()];
    shifted[..56].copy_from_slice(&payload[..56]);
    shifted[64..66].copy_from_slice(&[0x1a, 0x00]);
    shifted[66..74].copy_from_slice(&0.0025f64.to_le_bytes());
    shifted[74..82].copy_from_slice(&0.01f64.to_le_bytes());
    shifted[84..86].copy_from_slice(&2u16.to_le_bytes());
    for (start, local_id) in [(86, 2u16), (98, 5u16)] {
        shifted[start..start + 2].copy_from_slice(&[0x2b, 0x82]);
        shifted[start + 2..start + 4].copy_from_slice(&local_id.to_le_bytes());
        shifted[start + 4..start + 8].fill(0xff);
    }
    shifted[110..116].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    shifted[158..162].copy_from_slice(&29u32.to_le_bytes());
    shifted[162..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(legacy_linked_coordinates(&shifted, 0), Some([0.0025, 0.01]));
    assert_eq!(marker_coordinates(&shifted, 0), Some([0.0025, 0.01]));
    shifted[158..162].fill(0xff);
    assert_eq!(legacy_linked_coordinates(&shifted, 0), None);
}


#[test]
fn legacy_inline_arc_decodes_center_and_endpoints() {
    let mut payload = vec![0; 146 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1a, 0x00]);
    payload[66..74].copy_from_slice(&2.0f64.to_le_bytes());
    payload[74..82].copy_from_slice(&3.0f64.to_le_bytes());
    payload[92..94].copy_from_slice(&1u16.to_le_bytes());
    payload[96..104].copy_from_slice(&1.0f64.to_le_bytes());
    payload[104..112].copy_from_slice(&3.0f64.to_le_bytes());
    payload[112..120].copy_from_slice(&2.0f64.to_le_bytes());
    payload[120..128].copy_from_slice(&4.0f64.to_le_bytes());
    payload[132..136].copy_from_slice(&8u32.to_le_bytes());
    payload[142..146].copy_from_slice(&5u32.to_le_bytes());
    payload[146..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        inline_arc_coordinates(&payload, 0),
        Some([[2.0, 3.0], [1.0, 3.0], [2.0, 4.0]])
    );
    assert_eq!(marker_coordinates(&payload, 0), Some([2.0, 3.0]));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Arc
    );

    payload[120..128].copy_from_slice(&5.0f64.to_le_bytes());
    assert_eq!(inline_arc_coordinates(&payload, 0), None);

    let mut corner = vec![0; 138 + LEGACY_SKETCH_MARKER.len()];
    corner[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    corner[5..13].fill(0xff);
    corner[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    corner[17..21].copy_from_slice(&2u32.to_le_bytes());
    corner[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    corner[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    corner[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    corner[56..58].copy_from_slice(&0x16u16.to_le_bytes());
    corner[58..66].copy_from_slice(&20.0f64.to_le_bytes());
    corner[66..74].copy_from_slice(&20.0f64.to_le_bytes());
    corner[74..76].copy_from_slice(&11u16.to_le_bytes());
    corner[84..88].copy_from_slice(&9u32.to_le_bytes());
    corner[88..96].copy_from_slice(&20.0f64.to_le_bytes());
    corner[96..104].copy_from_slice(&17.0f64.to_le_bytes());
    corner[104..112].copy_from_slice(&17.0f64.to_le_bytes());
    corner[112..120].copy_from_slice(&20.0f64.to_le_bytes());
    corner[124..128].copy_from_slice(&54u32.to_le_bytes());
    corner[128..132].copy_from_slice(&2u32.to_le_bytes());
    corner[134..138].copy_from_slice(&73u32.to_le_bytes());
    corner[138..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        inline_arc_coordinates(&corner, 0),
        Some([[17.0, 17.0], [20.0, 17.0], [17.0, 20.0]])
    );
    assert_eq!(marker_coordinates(&corner, 0), Some([17.0, 17.0]));
    assert_eq!(
        sketch_input_entities(&corner, "lane")[0].kind,
        SketchInputKind::Arc
    );

    let mut packed = vec![0; 126 + LEGACY_SKETCH_MARKER.len()];
    packed[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    packed[5..13].fill(0xff);
    packed[13..17].copy_from_slice(&2u32.to_le_bytes());
    packed[19..25].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    packed[29..33].copy_from_slice(&[0x04, 0x00, 0x00, 0x00]);
    packed[40..48].copy_from_slice(&1.0f64.to_le_bytes());
    packed[48..50].copy_from_slice(&[0x16, 0x00]);
    packed[50..58].copy_from_slice(&2.0f64.to_le_bytes());
    packed[58..66].copy_from_slice(&3.0f64.to_le_bytes());
    packed[66..68].copy_from_slice(&11u16.to_le_bytes());
    packed[76..80].copy_from_slice(&6u32.to_le_bytes());
    packed[80..88].copy_from_slice(&1.0f64.to_le_bytes());
    packed[88..96].copy_from_slice(&3.0f64.to_le_bytes());
    packed[96..104].copy_from_slice(&2.0f64.to_le_bytes());
    packed[104..112].copy_from_slice(&4.0f64.to_le_bytes());
    packed[122..126].copy_from_slice(&9u32.to_le_bytes());
    packed[126..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        inline_arc_coordinates(&packed, 0),
        Some([[2.0, 3.0], [1.0, 3.0], [2.0, 4.0]])
    );
    assert_eq!(
        sketch_input_entities(&packed, "lane")[0].kind,
        SketchInputKind::Arc
    );
    packed[48] = 0x12;
    assert_eq!(
        inline_arc_coordinates(&packed, 0),
        Some([[2.0, 3.0], [1.0, 3.0], [2.0, 4.0]])
    );
    packed[104..112].copy_from_slice(&5.0f64.to_le_bytes());
    assert_eq!(inline_arc_coordinates(&packed, 0), None);
}


#[test]
fn geometry_locus_inline_arcs_decode_direct_and_opposite_corner_centers() {
    for (prefix, code, tag, stored, tail, center) in [
        (
            LEGACY_EXTENDED_SKETCH_MARKER,
            2u32,
            [0x12, 0x00],
            [2.0f64, 3.0],
            [0x00, 0x00, 0x02, 0x00, 0x00, 0x00],
            [2.0f64, 3.0],
        ),
        (
            LEGACY_SKETCH_MARKER,
            1u32,
            [0x1a, 0x00],
            [2.0f64, 3.0],
            [0x01, 0x00, 0x00, 0x00, 0x00, 0x00],
            [1.0f64, 4.0],
        ),
    ] {
        let mut payload = vec![0; 138 + prefix.len()];
        payload[..prefix.len()].copy_from_slice(prefix);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&code.to_le_bytes());
        payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
        payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[56..58].copy_from_slice(&tag);
        payload[58..66].copy_from_slice(&stored[0].to_le_bytes());
        payload[66..74].copy_from_slice(&stored[1].to_le_bytes());
        payload[74..76].copy_from_slice(&11u16.to_le_bytes());
        payload[84..88].copy_from_slice(&1u32.to_le_bytes());
        payload[88..96].copy_from_slice(&1.0f64.to_le_bytes());
        payload[96..104].copy_from_slice(&3.0f64.to_le_bytes());
        payload[104..112].copy_from_slice(&2.0f64.to_le_bytes());
        payload[112..120].copy_from_slice(&4.0f64.to_le_bytes());
        payload[124..128].copy_from_slice(&7u32.to_le_bytes());
        payload[128..134].copy_from_slice(&tail);
        payload[134..138].copy_from_slice(&5u32.to_le_bytes());
        payload[138..].copy_from_slice(prefix);

        assert_eq!(
            inline_arc_coordinates(&payload, 0),
            Some([center, [1.0, 3.0], [2.0, 4.0]])
        );
        assert_eq!(marker_coordinates(&payload, 0), Some(center));
        assert_eq!(
            sketch_input_entities(&payload, "lane")[0].kind,
            SketchInputKind::Arc
        );

        payload[112..120].copy_from_slice(&5.0f64.to_le_bytes());
        assert_eq!(inline_arc_coordinates(&payload, 0), None);
        payload[112..120].copy_from_slice(&4.0f64.to_le_bytes());
        payload[128] ^= 1;
        assert_eq!(inline_arc_coordinates(&payload, 0), None);
    }

    let mut compact = vec![0; 134 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    compact[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    compact[5..13].fill(0xff);
    compact[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    compact[17..21].copy_from_slice(&2u32.to_le_bytes());
    compact[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    compact[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    compact[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    compact[56..58].copy_from_slice(&[0x1a, 0x00]);
    compact[58..66].copy_from_slice(&2.0f64.to_le_bytes());
    compact[66..74].copy_from_slice(&3.0f64.to_le_bytes());
    compact[74..76].copy_from_slice(&11u16.to_le_bytes());
    compact[88..96].copy_from_slice(&1.0f64.to_le_bytes());
    compact[96..104].copy_from_slice(&3.0f64.to_le_bytes());
    compact[104..112].copy_from_slice(&2.0f64.to_le_bytes());
    compact[112..120].copy_from_slice(&4.0f64.to_le_bytes());
    compact[130..134].copy_from_slice(&5u32.to_le_bytes());
    compact[134..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        inline_arc_coordinates(&compact, 0),
        Some([[2.0, 3.0], [1.0, 3.0], [2.0, 4.0]])
    );
    assert_eq!(
        sketch_input_entities(&compact, "lane")[0].kind,
        SketchInputKind::Arc
    );
    compact[130..134].fill(0);
    assert_eq!(inline_arc_coordinates(&compact, 0), None);
}


#[test]
fn legacy_declared_handle_markers_decode_their_planar_coordinates() {
    let mut payload = vec![0; 170 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.045f64.to_le_bytes());
    payload[66..74].copy_from_slice(&(-0.0225f64).to_le_bytes());
    payload[74..84]
        .copy_from_slice(&[0x00, 0x00, 0x03, 0x00, 0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]);
    payload[84..96].copy_from_slice(b"sgLineHandle");
    payload[96..106]
        .copy_from_slice(&[0x03, 0x00, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00]);
    payload[106..108].copy_from_slice(&[0x2d, 0x82]);
    payload[108..110].copy_from_slice(&4u16.to_le_bytes());
    payload[110..114].fill(0xff);
    payload[118..124].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[170..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    assert_eq!(marker_coordinates(&payload, 0), Some([0.045, -0.0225]));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    let mut linked = payload.clone();
    linked[78..170].fill(0);
    linked[78..82].copy_from_slice(&[0x15, 0x84, 0x00, 0x00]);
    linked[82..86].fill(0xff);
    linked[90..96].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]);
    linked[96..108].copy_from_slice(b"sgLineHandle");
    linked[110..114].fill(0xff);
    linked[118..124].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    linked[166..170].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&linked, 0),
        Some([0.045, -0.0225])
    );
    linked[90] = 0;
    assert_eq!(legacy_declared_handle_coordinates(&linked, 0), None);

    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[170..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[96..98].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[17..21].fill(0);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    payload[96..98].copy_from_slice(&3u16.to_le_bytes());
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[170..].copy_from_slice(LEGACY_SKETCH_MARKER);

    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    payload[96..98].fill(0);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[17..21].fill(0);
    payload[96..98].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[96..98].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    payload[96..98].fill(0);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[96..98].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[96..98].fill(0);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[162..166].copy_from_slice(&3u32.to_le_bytes());
    payload[166..170].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[166..170].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[162..170].fill(0);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload.resize(177 + LEGACY_SKETCH_MARKER.len(), 0);
    payload[96..177].fill(0);
    payload[96..108].copy_from_slice(&[
        0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x0b, 0x00,
    ]);
    payload[108..119].copy_from_slice(b"sgArcHandle");
    payload[119..121].copy_from_slice(&3u16.to_le_bytes());
    payload[121..125].fill(0xff);
    payload[125..131].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[173..177].copy_from_slice(&2u32.to_le_bytes());
    payload[177..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    let mut padded_handle = payload.clone();
    padded_handle.resize(185 + LEGACY_SKETCH_MARKER.len(), 0);
    padded_handle[96..98].copy_from_slice(&1u16.to_le_bytes());
    padded_handle[98..185].fill(0);
    padded_handle[98..106].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00]);
    padded_handle[106..112].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0b, 0x00]);
    padded_handle[112..123].copy_from_slice(b"sgArcHandle");
    padded_handle[125..129].fill(0xff);
    padded_handle[133..139].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    padded_handle[181..185].copy_from_slice(&5u32.to_le_bytes());
    padded_handle[185..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        legacy_declared_handle_coordinates(&padded_handle, 0),
        Some([0.045, -0.0225])
    );
    padded_handle[181..185].fill(0);
    assert_eq!(legacy_declared_handle_coordinates(&padded_handle, 0), None);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[17..21].fill(0);
    payload[96..98].copy_from_slice(&3u16.to_le_bytes());
    payload[119..121].fill(0);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.045, -0.0225])
    );
    payload[96..98].fill(0);
    assert_eq!(legacy_declared_handle_coordinates(&payload, 0), None);
    payload[119..121].fill(0xff);
    assert_eq!(legacy_declared_handle_coordinates(&payload, 0), None);
    payload[96..98].copy_from_slice(&3u16.to_le_bytes());
    payload[119..121].fill(0);
    payload[84] = b'x';
    assert_eq!(legacy_declared_handle_coordinates(&payload, 0), None);
}


#[test]
fn legacy_arc_handle_marker_decodes_its_planar_coordinate() {
    let mut payload = vec![0; 169 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.352f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.005f64.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[78..82].copy_from_slice(&[0x5e, 0x82, 0x03, 0x00]);
    payload[82..86].fill(0xff);
    payload[90..96].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0b, 0x00]);
    payload[96..107].copy_from_slice(b"sgArcHandle");
    payload[109..113].fill(0xff);
    payload[117..123].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[165..169].copy_from_slice(&7u32.to_le_bytes());
    payload[169..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.352, 0.005])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.352, 0.005])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());

    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[17..21].fill(0);
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    payload[169..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        legacy_declared_handle_coordinates(&payload, 0),
        Some([0.352, 0.005])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[169..].copy_from_slice(LEGACY_SKETCH_MARKER);

    payload[80..82].fill(0);
    assert_eq!(legacy_declared_handle_coordinates(&payload, 0), None);
    payload[80..82].copy_from_slice(&3u16.to_le_bytes());
    payload[165..169].fill(0xff);
    assert_eq!(legacy_declared_handle_coordinates(&payload, 0), None);
}


#[test]
fn linked_profile_point_146_decodes_prefix_specific_coordinate_tags() {
    let mut payload = vec![0; 146 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.8f64.to_le_bytes());
    payload[66..74].copy_from_slice(&0.0125f64.to_le_bytes());
    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[78..82].copy_from_slice(&[0x16, 0x81, 0x06, 0x00]);
    payload[82..86].fill(0xff);
    payload[86..90].copy_from_slice(&[0x16, 0x81, 0x07, 0x00]);
    payload[90..94].fill(0xff);
    payload[94..100].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[142..146].copy_from_slice(&6u32.to_le_bytes());
    payload[146..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[86..88].copy_from_slice(&0x8121u16.to_le_bytes());
    payload[88..90].copy_from_slice(&6u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[86..88].copy_from_slice(&0x8116u16.to_le_bytes());
    payload[88..90].copy_from_slice(&7u16.to_le_bytes());
    payload[100..146].fill(0);
    payload[136..140].copy_from_slice(&1u32.to_le_bytes());
    payload[142..146].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[136..140].fill(0);
    payload[142..146].copy_from_slice(&6u32.to_le_bytes());
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[56..58].copy_from_slice(&[0x1a, 0x00]);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[142..146].fill(0xff);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[56..58].copy_from_slice(&[0x1a, 0x00]);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[142..146].copy_from_slice(&6u32.to_le_bytes());
    payload[134..136].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[134..136].fill(0);
    let mut continuation = vec![0; 150 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    continuation[..146].copy_from_slice(&payload[..146]);
    continuation[136..140].copy_from_slice(&6u32.to_le_bytes());
    continuation[140..142].fill(0);
    continuation[142..146].copy_from_slice(&1u32.to_le_bytes());
    continuation[146..150].copy_from_slice(&1u32.to_le_bytes());
    continuation[150..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&continuation, 0),
        Some([0.8, 0.0125])
    );
    assert_eq!(
        sketch_input_entities(&continuation, "lane")[0].kind,
        SketchInputKind::Point
    );
    continuation[142..146].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&continuation, 0),
        None
    );
    continuation[142..146].copy_from_slice(&1u32.to_le_bytes());
    continuation[76..78].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&continuation, 0),
        None
    );
    continuation[76..78].copy_from_slice(&2u16.to_le_bytes());
    continuation[136..140].fill(0);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&continuation, 0),
        None
    );
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[80..82].fill(0);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[142..146].fill(0xff);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[88..90].fill(0);
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[88..90].copy_from_slice(&7u16.to_le_bytes());
    payload[74..76].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[74..76].fill(0);
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[141] = 1;
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[141] = 0;
    payload[80..82].copy_from_slice(&6u16.to_le_bytes());
    payload[142..146].copy_from_slice(&6u32.to_le_bytes());
    payload[74..76].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[74..76].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[74..76].copy_from_slice(&1u16.to_le_bytes());
    payload[76..78].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[138..142].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );

    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );

    payload[88..90].copy_from_slice(&8u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        Some([0.8, 0.0125])
    );
    payload[80..82].copy_from_slice(&8u16.to_le_bytes());
    assert_eq!(
        legacy_extended_linked_profile_point_coordinates(&payload, 0),
        None
    );
}


#[test]
fn compact_legacy_linked_profile_point_decodes_inline_coordinates() {
    let mut payload = vec![0; 132 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[19..25].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31] = 0x04;
    payload[42..44].copy_from_slice(&[0x1e, 0x00]);
    payload[44..52].copy_from_slice(&0.004f64.to_le_bytes());
    payload[52..60].copy_from_slice(&0.006f64.to_le_bytes());
    payload[62..64].copy_from_slice(&2u16.to_le_bytes());
    for (relative, id) in [(64, 0u16), (72, 3u16)] {
        payload[relative..relative + 2].copy_from_slice(&0x811au16.to_le_bytes());
        payload[relative + 2..relative + 4].copy_from_slice(&id.to_le_bytes());
        payload[relative + 4..relative + 8].fill(0xff);
    }
    payload[80..86].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[128..132].copy_from_slice(&7u32.to_le_bytes());
    payload[132..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        compact_legacy_linked_profile_point_coordinates(&payload, 0),
        Some([0.004, 0.006])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[42] = 0x1a;
    payload[62..64].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        compact_legacy_linked_profile_point_coordinates(&payload, 0),
        Some([0.004, 0.006])
    );
    payload[74..76].fill(0);
    assert_eq!(
        compact_legacy_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[74..76].copy_from_slice(&3u16.to_le_bytes());
    payload[128..132].fill(0xff);
    assert_eq!(
        compact_legacy_linked_profile_point_coordinates(&payload, 0),
        None
    );
}


#[test]
fn legacy_single_incidence_profile_point_decodes_both_identity_trailers() {
    let mut payload = vec![0; 140 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&0.052f64.to_le_bytes());
    payload[66..74].copy_from_slice(&(-0.01f64).to_le_bytes());
    payload[76..78].copy_from_slice(&1u16.to_le_bytes());
    payload[78..82].copy_from_slice(&[0x29, 0x81, 0x0e, 0x00]);
    payload[82..86].fill(0xff);
    payload[90..96].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff, 0x00, 0x00]);
    payload[136..140].copy_from_slice(&24u32.to_le_bytes());
    payload[140..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_single_incidence_profile_point_coordinates(&payload, 0),
        Some([0.052, -0.01])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[17..21].fill(0);
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    payload[96..140].fill(0);
    payload[128..132].copy_from_slice(&1u32.to_le_bytes());
    payload[136..140].copy_from_slice(&10u32.to_le_bytes());
    assert_eq!(
        legacy_single_incidence_profile_point_coordinates(&payload, 0),
        Some([0.052, -0.01])
    );
    payload[136..140].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        legacy_single_incidence_profile_point_coordinates(&payload, 0),
        None
    );
}


#[test]
fn packed_legacy_linked_profile_point_decodes_inline_coordinates() {
    let mut payload = vec![0; 138 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[19..25].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[27..31].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[40..48].copy_from_slice(&1.0f64.to_le_bytes());
    payload[48..50].copy_from_slice(&[0x1a, 0x00]);
    payload[50..58].copy_from_slice(&0.0021f64.to_le_bytes());
    payload[58..66].copy_from_slice(&0.0f64.to_le_bytes());
    payload[68..70].copy_from_slice(&2u16.to_le_bytes());
    for (relative, id) in [(70, 2u16), (78, 5u16)] {
        payload[relative..relative + 2].copy_from_slice(&0x8181u16.to_le_bytes());
        payload[relative + 2..relative + 4].copy_from_slice(&id.to_le_bytes());
        payload[relative + 4..relative + 8].fill(0xff);
    }
    payload[86..92].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    payload[134..138].copy_from_slice(&29u32.to_le_bytes());
    payload[138..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        packed_legacy_linked_profile_point_coordinates(&payload, 0),
        Some([0.0021, 0.0])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[80..82].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        packed_legacy_linked_profile_point_coordinates(&payload, 0),
        None
    );
    payload[80..82].copy_from_slice(&5u16.to_le_bytes());
    payload[72..74].fill(0);
    assert_eq!(
        packed_legacy_linked_profile_point_coordinates(&payload, 0),
        None
    );
}


#[test]
fn extended_profile_point_forms_decode_as_points() {
    let common = |length: usize| {
        let mut payload = vec![0; length + LEGACY_EXTENDED_SKETCH_MARKER.len()];
        payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&2u32.to_le_bytes());
        payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[56..58].copy_from_slice(&[0x1e, 0x00]);
        payload[58..66].copy_from_slice(&0.435f64.to_le_bytes());
        payload[66..74].copy_from_slice(&0.0075f64.to_le_bytes());
        payload[74..78].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
        payload[length..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload
    };
    let mut declaration = common(170);
    declaration[78..84].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]);
    declaration[84..96].copy_from_slice(b"sgLineHandle");
    declaration[96..106].copy_from_slice(&[0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    declaration[106..110].copy_from_slice(&[0x56, 0x81, 0x07, 0x00]);
    declaration[110..114].fill(0xff);
    declaration[118..124].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    assert_eq!(
        extended_profile_point_coordinates(&declaration, 0),
        Some([0.435, 0.0075])
    );
    assert_eq!(
        sketch_input_entities(&declaration, "lane")[0].kind,
        SketchInputKind::Point
    );
    declaration[76..78].copy_from_slice(&3u16.to_le_bytes());
    declaration[96..98].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&declaration, 0),
        Some([0.435, 0.0075])
    );
    assert_eq!(
        sketch_input_entities(&declaration, "lane")[0].kind,
        SketchInputKind::Point
    );
    declaration[96..98].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&declaration, 0),
        Some([0.435, 0.0075])
    );
    declaration[96..98].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(extended_profile_point_coordinates(&declaration, 0), None);

    let mut compact_declaration = common(162);
    compact_declaration[78..84].copy_from_slice(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00]);
    compact_declaration[84..96].copy_from_slice(b"sgLineHandle");
    compact_declaration[96..98].copy_from_slice(&1u16.to_le_bytes());
    compact_declaration[98..102].fill(0xff);
    compact_declaration[102..104].copy_from_slice(&0x8156u16.to_le_bytes());
    compact_declaration[104..106].copy_from_slice(&2u16.to_le_bytes());
    compact_declaration[106..110].fill(0xff);
    compact_declaration[110..116].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    compact_declaration[158..162].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        Some([0.435, 0.0075])
    );
    assert_eq!(
        sketch_input_entities(&compact_declaration, "lane")[0].kind,
        SketchInputKind::Point
    );
    compact_declaration[96..98].fill(0);
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        Some([0.435, 0.0075])
    );
    compact_declaration[96..98].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        None
    );
    compact_declaration[96..98].copy_from_slice(&1u16.to_le_bytes());
    compact_declaration[154..158].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        Some([0.435, 0.0075])
    );
    compact_declaration[154..158].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        None
    );
    compact_declaration[154..158].fill(0);
    compact_declaration[74..78].copy_from_slice(&[0x00, 0x00, 0x03, 0x00]);
    compact_declaration[96..98].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        Some([0.435, 0.0075])
    );
    compact_declaration[102..104].fill(0);
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        None
    );
    compact_declaration[102..104].copy_from_slice(&0x8156u16.to_le_bytes());
    compact_declaration[104..106].fill(0);
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        None
    );
    compact_declaration[104..106].copy_from_slice(&2u16.to_le_bytes());
    compact_declaration[74..78].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    compact_declaration[96..98].fill(0);
    compact_declaration[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    compact_declaration[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        Some([0.435, 0.0075])
    );
    compact_declaration[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    compact_declaration[17..21].copy_from_slice(&1u32.to_le_bytes());
    compact_declaration[96..98].copy_from_slice(&12u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        Some([0.435, 0.0075])
    );
    assert_eq!(
        sketch_input_entities(&compact_declaration, "lane")[0].kind,
        SketchInputKind::Point
    );
    compact_declaration[96..98].copy_from_slice(&11u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&compact_declaration, 0),
        None
    );

    let mut linked = common(154);
    for (relative, id) in [(78, 1u16), (90, 2u16)] {
        linked[relative..relative + 2].copy_from_slice(&[0x56, 0x81]);
        linked[relative + 2..relative + 4].copy_from_slice(&id.to_le_bytes());
        linked[relative + 4..relative + 8].fill(0xff);
    }
    linked[102..108].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    linked[150..154].copy_from_slice(&10u32.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&linked, 0),
        Some([0.435, 0.0075])
    );
    linked[17..21].copy_from_slice(&0u32.to_le_bytes());
    linked[76..78].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&linked, 0),
        Some([0.435, 0.0075])
    );
    assert_eq!(
        sketch_input_entities(&linked, "lane")[0].kind,
        SketchInputKind::Point
    );
    linked[92..94].copy_from_slice(&4u16.to_le_bytes());
    assert_eq!(
        extended_profile_point_coordinates(&linked, 0),
        Some([0.435, 0.0075])
    );
    linked[92..94].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(extended_profile_point_coordinates(&linked, 0), None);
    linked[92..94].fill(0);
    assert_eq!(
        extended_profile_point_coordinates(&linked, 0),
        Some([0.435, 0.0075])
    );
}


#[test]
fn terminal_extended_profile_point_decodes_inline_coordinates() {
    let mut payload = vec![0; 180];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&(-0.19f64).to_le_bytes());
    payload[66..74].copy_from_slice(&0.0f64.to_le_bytes());
    payload[74..78].copy_from_slice(&1u32.to_le_bytes());
    payload[84..88].copy_from_slice(&(-2i32).to_le_bytes());
    payload[174..176].copy_from_slice(&6u16.to_le_bytes());
    payload[176..178].copy_from_slice(&0x81a3u16.to_le_bytes());

    assert_eq!(
        terminal_extended_profile_point_coordinates(&payload, 0),
        Some([-0.19, 0.0])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    payload[174..176].fill(0);
    assert_eq!(
        terminal_extended_profile_point_coordinates(&payload, 0),
        None
    );
    payload[174..176].copy_from_slice(&6u16.to_le_bytes());
    payload[176..178].fill(0);
    assert_eq!(
        terminal_extended_profile_point_coordinates(&payload, 0),
        None
    );
}


#[test]
fn current_geometry_locus_profile_vertex_decodes_as_a_point() {
    let mut payload = vec![0; 146 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[66..74].copy_from_slice(&(-1.125f64).to_le_bytes());
    payload[74..82].copy_from_slice(&0.542f64.to_le_bytes());
    payload[82..86].copy_from_slice(&1u32.to_le_bytes());
    payload[92..98].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff, 0x00, 0x00]);
    payload[132..136].copy_from_slice(&7u32.to_le_bytes());
    payload[142..146].fill(0xff);
    payload[146..].copy_from_slice(SKETCH_MARKER);

    assert!(current_geometry_locus_profile_vertex(&payload, 0));
    let entity = &sketch_input_entities(&payload, "lane")[0];
    assert_eq!(entity.kind, SketchInputKind::Point);
    assert_eq!(entity.coordinates_m, Some([-1.125, 0.542]));
    payload[132..136].fill(0);
    assert!(!current_geometry_locus_profile_vertex(&payload, 0));
}


#[test]
fn current_compact_geometry_locus_profile_point_decodes_inline_coordinates() {
    let mut payload = vec![0; 134 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&(-1.125f64).to_le_bytes());
    payload[66..74].copy_from_slice(&0.542f64.to_le_bytes());
    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[82..84].copy_from_slice(&1u16.to_le_bytes());
    payload[84..88].copy_from_slice(&(-2i32).to_le_bytes());
    payload[130..134].copy_from_slice(&7u32.to_le_bytes());
    payload[134..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        compact_geometry_locus_point_coordinates(&payload, 0),
        Some([-1.125, 0.542])
    );
    let entity = &sketch_input_entities(&payload, "lane")[0];
    assert_eq!(entity.kind, SketchInputKind::Point);
    assert_eq!(entity.coordinates_m, Some([-1.125, 0.542]));
    payload[82..84].fill(0);
    assert_eq!(compact_geometry_locus_point_coordinates(&payload, 0), None);
}


#[test]
fn legacy_compact_geometry_locus_point_decodes_inline_coordinates() {
    let mut payload = vec![0; 134 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&(-1.125f64).to_le_bytes());
    payload[66..74].copy_from_slice(&0.542f64.to_le_bytes());
    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[82..84].copy_from_slice(&1u16.to_le_bytes());
    payload[84..88].copy_from_slice(&(-2i32).to_le_bytes());
    payload[130..134].copy_from_slice(&7u32.to_le_bytes());
    payload[134..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        compact_geometry_locus_point_coordinates(&payload, 0),
        Some([-1.125, 0.542])
    );
    let entity = &sketch_input_entities(&payload, "lane")[0];
    assert_eq!(entity.kind, SketchInputKind::Point);
    assert_eq!(entity.coordinates_m, Some([-1.125, 0.542]));
    payload[130..134].fill(0);
    assert_eq!(compact_geometry_locus_point_coordinates(&payload, 0), None);
}


#[test]
fn geometry_locus_profile_vertex_decodes_compact_marker_bands() {
    let mut payload = vec![0; 134 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&(-0.04f64).to_le_bytes());
    payload[66..74].copy_from_slice(&0.0045f64.to_le_bytes());
    payload[74..78].copy_from_slice(&1u32.to_le_bytes());
    payload[84..88].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    payload[130..134].fill(0xff);
    payload[134..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert!(geometry_locus_profile_vertex(&payload, 0));
    let entity = &sketch_input_entities(&payload, "lane")[0];
    assert_eq!(entity.kind, SketchInputKind::Point);
    assert_eq!(entity.coordinates_m, Some([-0.04, 0.0045]));
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    assert!(geometry_locus_profile_vertex(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[134..].copy_from_slice(SKETCH_MARKER);
    assert!(geometry_locus_profile_vertex(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[134..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[37] = 0x05;
    assert!(geometry_locus_profile_vertex(&payload, 0));
    payload[37] = 0x06;
    assert!(!geometry_locus_profile_vertex(&payload, 0));
    payload[37] = 0x04;
    payload[74..78].fill(0);
    assert!(geometry_locus_profile_vertex(&payload, 0));
    payload[74..78].copy_from_slice(&2u32.to_le_bytes());
    assert!(!geometry_locus_profile_vertex(&payload, 0));

    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[82..84].copy_from_slice(&2u16.to_le_bytes());
    payload[130..134].copy_from_slice(&13u32.to_le_bytes());
    assert!(geometry_locus_profile_vertex(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[130..134].fill(0);
    assert!(!geometry_locus_profile_vertex(&payload, 0));

    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[74..134].fill(0);
    payload[74..78].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[84..88].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    payload[126..130].copy_from_slice(&3u32.to_le_bytes());
    payload[130..134].copy_from_slice(&10u32.to_le_bytes());
    payload[134..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert!(geometry_locus_profile_vertex(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[130..134].copy_from_slice(&3u32.to_le_bytes());
    assert!(!geometry_locus_profile_vertex(&payload, 0));

    payload.resize(138 + LEGACY_EXTENDED_SKETCH_MARKER.len(), 0);
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[74..138].fill(0);
    payload[74..78].copy_from_slice(&1u32.to_le_bytes());
    payload[84..88].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    payload[124..128].copy_from_slice(&2u32.to_le_bytes());
    payload[128..132].copy_from_slice(&2u32.to_le_bytes());
    payload[132..138].copy_from_slice(&[0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);
    payload[138..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert!(geometry_locus_profile_vertex(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[128..132].copy_from_slice(&3u32.to_le_bytes());
    assert!(!geometry_locus_profile_vertex(&payload, 0));
    payload[128..132].copy_from_slice(&2u32.to_le_bytes());
    payload[17..21].copy_from_slice(&3u32.to_le_bytes());
    assert!(!geometry_locus_profile_vertex(&payload, 0));
}


#[test]
fn extended_coordinate_ellipse_uses_its_complete_corner_grid() {
    let mut payload = vec![0; 134 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[134..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, ordinal, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let ellipse = entity("ellipse", 0, 0, SketchInputKind::Arc, Some([2.0, 3.0]));
    let points = [
        [-2.0, 2.0],
        [-2.0, 4.0],
        [6.0, 2.0],
        [6.0 + f64::EPSILON * 4.0, 4.0],
    ]
    .into_iter()
    .enumerate()
    .map(|(index, point)| {
        entity(
            &format!("point-{index}"),
            index as u32 + 1,
            index as u64 + 134,
            SketchInputKind::Point,
            Some(point),
        )
    })
    .collect::<Vec<_>>();
    let mut entities = vec![ellipse.clone()];
    entities.extend(points);
    let markers = entities.iter().collect::<Vec<_>>();
    assert!(
        super::coordinate_ellipse_axes(&payload, &ellipse, &markers).is_some_and(
            |(axis, major, minor)| {
                axis == [1.0, 0.0]
                    && super::same_dimension_length(major, 4.0)
                    && super::same_dimension_length(minor, 1.0)
            }
        )
    );

    entities[4].coordinates_m = Some([6.0, 5.0]);
    let markers = entities.iter().collect::<Vec<_>>();
    assert_eq!(
        super::coordinate_ellipse_axes(&payload, &ellipse, &markers),
        None
    );
}


#[test]
fn coordinate_lines_use_their_centered_endpoint_pairs() {
    let mut payload = vec![0; 147];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].copy_from_slice(&[0xff; 8]);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[66..74].copy_from_slice(&2.0f64.to_le_bytes());
    payload[74..82].copy_from_slice(&3.0f64.to_le_bytes());
    payload[82..86].copy_from_slice(&1u32.to_le_bytes());
    payload[92..96].copy_from_slice(&(-2i32).to_le_bytes());
    payload[142..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, offset, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: None,
        coordinates_m: Some(coordinates_m),
        links: Vec::new(),
        link_selector: None,
    };
    let line = entity("line", 0, [2.0, 3.0]);
    let first = entity("first", 143, [1.0, 2.0]);
    let second = entity("second", 144, [3.0, 4.0]);
    let markers = [&line, &first, &second];
    assert_eq!(
        coordinate_centered_line_endpoints(&payload, &line, &markers),
        Some([&first, &second])
    );

    let mut extended = vec![0; 139];
    extended[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    extended[5..13].fill(0xff);
    extended[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    extended[17..21].copy_from_slice(&2u32.to_le_bytes());
    extended[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    extended[27..29].copy_from_slice(&1u16.to_le_bytes());
    extended[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    extended[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    extended[56..58].copy_from_slice(&[0x1e, 0x00]);
    extended[58..66].copy_from_slice(&2.0f64.to_le_bytes());
    extended[66..74].copy_from_slice(&3.0f64.to_le_bytes());
    extended[76..78].copy_from_slice(&1u16.to_le_bytes());
    extended[82..84].copy_from_slice(&1u16.to_le_bytes());
    extended[84..88].copy_from_slice(&(-2i32).to_le_bytes());
    extended[130..134].copy_from_slice(&7u32.to_le_bytes());
    extended[134..].copy_from_slice(SKETCH_MARKER);
    let mut extended_line = line.clone();
    extended_line.coordinates_m = None;
    let markers = [&extended_line, &first, &second];
    assert_eq!(
        coordinate_centered_line_endpoints(&extended, &extended_line, &markers),
        Some([&first, &second])
    );
    extended[84] ^= 1;
    assert_eq!(
        coordinate_centered_line_endpoints(&extended, &extended_line, &markers),
        None
    );
}


#[test]
fn current_coordinate_line_uses_its_single_local_link() {
    let mut payload = vec![0; 157];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].copy_from_slice(&[0xff; 8]);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[82..86].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[86..88].copy_from_slice(&0xbc87u16.to_le_bytes());
    payload[88..90].copy_from_slice(&22u16.to_le_bytes());
    payload[90..94].fill(0xff);
    payload[102..106].copy_from_slice(&(-2i32).to_le_bytes());
    payload[152..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, offset, local_id, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id,
        kind,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let line = entity(
        "line",
        0,
        Some(1),
        SketchInputKind::LineOrCircle,
        Some([2.0, 3.0]),
    );
    let endpoint = entity(
        "endpoint",
        153,
        Some(22),
        SketchInputKind::Point,
        Some([4.0, 5.0]),
    );
    assert_eq!(
        current_coordinate_linked_line_endpoints(&payload, &line, &[&line, &endpoint]),
        Some([&line, &endpoint])
    );
}


#[test]
fn compact_legacy_wide_selected_axis_indexes_the_coordinate_roster() {
    let mut payload = vec![0; 92 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0d, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&8u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[92..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_coordinate_roster_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x0c, 0x00]);
    assert_eq!(
        legacy_coordinate_roster_selected_axis_endpoint_indices(&payload, 0),
        None
    );
}


#[test]
fn extended_profile_roster_construction_line_indexes_coordinate_markers() {
    let mut payload = vec![0; 92 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&3u16.to_le_bytes());
    payload[66..68].copy_from_slice(&4u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..88].copy_from_slice(&7u32.to_le_bytes());
    payload[88..92].copy_from_slice(&7u32.to_le_bytes());
    payload[92..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_profile_roster_construction_line_endpoint_indices(&payload, 0),
        Some([4, 5])
    );
    assert_eq!(
        super::coordinate_roster_endpoint_offset(&payload, 0),
        Some(64)
    );
    assert!(marker_is_selected_construction_line(&payload, 0));

    payload[88..92].copy_from_slice(&8u32.to_le_bytes());
    assert_eq!(
        extended_profile_roster_construction_line_endpoint_indices(&payload, 0),
        None
    );
}


#[test]
fn extended_wide_selected_axis_uses_object_ids_then_one_based_point_roster() {
    let mut payload = vec![0; 92 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[66..68].copy_from_slice(&2u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[88..92].fill(0xff);
    payload[92..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str,
                  offset: u64,
                  object_index: Option<u32>,
                  coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind: if coordinates_m.is_some() {
            SketchInputKind::Point
        } else {
            SketchInputKind::LineOrCircle
        },
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, Some(8), None);
    let first = entity("first", 10, Some(1), Some([0.0, 0.0]));
    let second = entity("second", 20, Some(3), Some([1.0, 0.0]));
    let third = entity("third", 30, Some(20), Some([2.0, 0.0]));
    let markers = [&curve, &first, &second, &third];

    assert_eq!(
        extended_wide_selected_axis_endpoints(&payload, &curve, &markers)
            .expect("object-index endpoints")
            .map(|endpoint| endpoint.id.as_str()),
        ["first", "second"]
    );

    payload[64..66].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        extended_wide_selected_axis_endpoints(&payload, &curve, &markers)
            .expect("one-based roster endpoints")
            .map(|endpoint| endpoint.id.as_str()),
        ["third", "second"]
    );
}


#[test]
fn extended_compact_construction_line_distinguishes_direct_ids_from_roster_indices() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..64].copy_from_slice(&[0x00, 0x00, 0x01, 0x00, 0, 0, 0, 0]);
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[76..80].copy_from_slice(&8u32.to_le_bytes());
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_compact_84_construction_line_endpoint_indices(&payload, 0),
        Some([8, 2])
    );
    payload[72..76].fill(0);
    assert_eq!(
        extended_compact_84_construction_line_endpoint_indices(&payload, 0),
        Some([8, 2])
    );
    payload[56..60].copy_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    let entity = |id: &str, offset, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: if coordinates_m.is_some() {
            SketchInputKind::Point
        } else {
            SketchInputKind::LineOrCircle
        },
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, None);
    let first = entity("first", 10, Some([1.0, 2.0]));
    let second = entity("second", 20, Some([3.0, 4.0]));
    let markers = [&curve, &first, &second];
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "first"]
    );

    payload[56..64].copy_from_slice(&[0x00, 0x00, 0x01, 0x00, 0, 0, 0, 0]);
    payload[80..84].copy_from_slice(&8u32.to_le_bytes());
    assert_eq!(
        extended_compact_84_construction_line_endpoint_indices(&payload, 0),
        None
    );
}


#[test]
fn extended_compact_96_selected_axis_uses_one_based_object_indices() {
    let mut payload = vec![0; 96 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&3u16.to_le_bytes());
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[82..84].copy_from_slice(&5u16.to_le_bytes());
    payload[88..92].copy_from_slice(&5u32.to_le_bytes());
    payload[92..96].copy_from_slice(&1u32.to_le_bytes());
    payload[96..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_compact_96_selected_axis_endpoint_indices(&payload, 0),
        Some([4, 5])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));

    payload[88..92].copy_from_slice(&6u32.to_le_bytes());
    assert_eq!(
        extended_compact_96_selected_axis_endpoint_indices(&payload, 0),
        None
    );
}


#[test]
fn extended_marker84_line_uses_state_selected_point_roster_base() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, offset, coordinates_m, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, None, SketchInputKind::LineOrCircle);
    let points = [
        entity("first", 10, Some([0.0, 0.0]), SketchInputKind::Point),
        entity("second", 20, Some([1.0, 0.0]), SketchInputKind::Point),
        entity("third", 30, Some([1.0, 1.0]), SketchInputKind::Point),
        entity("fourth", 40, Some([0.0, 1.0]), SketchInputKind::Point),
    ];
    let markers = std::iter::once(&curve)
        .chain(points.iter())
        .collect::<Vec<_>>();

    assert!(super::extended_marker84_line_uses_point_roster(&payload, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "third"]
    );
    payload[56..58].copy_from_slice(&2u16.to_le_bytes());
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[80..84].fill(0xff);
    assert!(!super::extended_marker84_line_uses_point_roster(
        &payload, 0
    ));
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "third"]
    );
    payload[80..84].fill(0);
    assert!(!super::extended_marker84_line_uses_point_roster(
        &payload, 0
    ));
    assert!(roster_curve_endpoint_markers(&payload, &curve, &markers).is_empty());
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[72..76].fill(0);
    assert!(super::extended_marker84_line_uses_point_roster(&payload, 0));
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "fourth"]
    );
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    assert!(super::extended_marker84_line_uses_point_roster(&payload, 0));
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[56..58].copy_from_slice(&4u16.to_le_bytes());
    payload[58..60].copy_from_slice(&1u16.to_le_bytes());
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["fourth", "first"]
    );
    payload[72..76].fill(0);
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "fourth"]
    );
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    assert!(!super::extended_marker84_line_uses_point_roster(
        &payload, 0
    ));

    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[56..58].fill(0);
    assert!(super::extended_marker84_line_uses_point_roster(&payload, 0));
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "fourth"]
    );
    payload[56..58].fill(0xff);
    assert!(!super::extended_marker84_line_uses_point_roster(
        &payload, 0
    ));
}


#[test]
fn legacy_compact_marker84_profile_line_uses_zero_based_point_roster() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..41].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x08, 0x00, 0x58, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&0u16.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[80..84].copy_from_slice(&7u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let entity = |id: &str, offset, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: if coordinates_m.is_some() {
            SketchInputKind::Point
        } else {
            SketchInputKind::LineOrCircle
        },
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, None);
    let first = entity("first", 10, Some([0.0, 0.0]));
    let second = entity("second", 20, Some([1.0, 0.0]));
    let third = entity("third", 30, Some([1.0, 1.0]));
    let markers = [&curve, &first, &second, &third];

    assert!(super::legacy_compact_84_profile_line_uses_point_roster(
        &payload, 0
    ));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "third"]
    );

    payload[74..76].fill(0);
    assert!(super::legacy_compact_84_profile_line_uses_point_roster(
        &payload, 0
    ));
    payload[80..84].fill(0);
    assert!(!super::legacy_compact_84_profile_line_uses_point_roster(
        &payload, 0
    ));
    assert!(roster_curve_endpoint_markers(&payload, &curve, &markers).is_empty());
}


#[test]
fn extended_compact_marker84_profile_line_uses_zero_based_geometry_roster() {
    let mut payload = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..41]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x08, 0x00, 0x58, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&0u16.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[80..84].copy_from_slice(&7u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, offset, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: if coordinates_m.is_some() {
            SketchInputKind::Point
        } else {
            SketchInputKind::LineOrCircle
        },
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, None);
    let first = entity("first", 10, Some([0.0, 0.0]));
    let second = entity("second", 20, Some([1.0, 0.0]));
    let third = entity("third", 30, Some([1.0, 1.0]));
    let markers = [&curve, &first, &second, &third];

    assert!(super::extended_compact_84_profile_line_uses_point_roster(
        &payload, 0
    ));
    assert!(!marker_is_selected_construction_line(&payload, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "third"]
    );

    payload[39] = 0x40;
    assert!(super::extended_compact_84_profile_line_uses_point_roster(
        &payload, 0
    ));
    payload[39] = 0x58;
    payload[80..84].fill(0);
    assert!(!super::extended_compact_84_profile_line_uses_point_roster(
        &payload, 0
    ));
    assert!(roster_curve_endpoint_markers(&payload, &curve, &markers).is_empty());
}


#[test]
fn legacy_referenced_wide_arc_indexes_center_and_endpoints() {
    let mut payload = vec![0; 112 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&1u16.to_le_bytes());
    payload[66..68].copy_from_slice(&2u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&1i32.to_le_bytes());
    for relative in [86, 90, 94, 98] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..108].copy_from_slice(&2u32.to_le_bytes());
    payload[108..112].copy_from_slice(&2u32.to_le_bytes());
    payload[112..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_referenced_wide_arc_endpoint_indices(&payload, 0),
        Some([2, 3])
    );
    assert_eq!(
        super::coordinate_roster_endpoint_offset(&payload, 0),
        Some(64)
    );
    assert!(super::indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(
        super::sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Arc
    );

    payload[108..112].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        legacy_referenced_wide_arc_endpoint_indices(&payload, 0),
        None
    );
}


#[test]
fn current_compact_104_line_indexes_coordinate_markers() {
    let mut payload = vec![0; 104 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&2u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[88..92].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[100..104].copy_from_slice(&1u32.to_le_bytes());
    payload[104..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        current_compact_104_indexed_line_endpoint_indices(&payload, 0),
        Some([8, 3])
    );
    assert_eq!(
        super::coordinate_roster_endpoint_offset(&payload, 0),
        Some(64)
    );

    payload[100..104].fill(0);
    assert_eq!(
        current_compact_104_indexed_line_endpoint_indices(&payload, 0),
        None
    );
}


#[test]
fn current_compact_84_line_falls_back_to_zero_based_point_roster() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[58..60].copy_from_slice(&1u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);

    let entity =
        |id: &str, offset, object_index, coordinates_m: Option<[f64; 2]>| SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("sketch".into()),
            ordinal: 0,
            offset,
            object_index,
            local_id: None,
            kind: if coordinates_m.is_some() {
                SketchInputKind::Point
            } else {
                SketchInputKind::LineOrCircle
            },
            state_value: Some(1.0),
            coordinates_m,
            links: Vec::new(),
            link_selector: None,
        };
    let curve = entity("curve", 0, Some(1), None);
    let first = entity("first", 10, Some(10), Some([0.0, 0.0]));
    let second = entity("second", 20, Some(11), Some([1.0, 0.0]));
    let markers = [&curve, &first, &second];

    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload.resize(96 + SKETCH_MARKER.len(), 0);
    payload[72..82].fill(0);
    payload[82..84].copy_from_slice(&2u16.to_le_bytes());
    payload[84..88].fill(0);
    payload[88..92].copy_from_slice(&2u32.to_le_bytes());
    payload[92..96].copy_from_slice(&1u32.to_le_bytes());
    payload[96..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload.resize(104 + SKETCH_MARKER.len(), 0);
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[94..96].fill(0);
    payload[96..104].copy_from_slice(&[2, 0, 0, 0, 3, 0, 0, 0]);
    payload[104..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &curve, &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
}


#[test]
fn current_compact_104_profile_record_is_a_line() {
    let mut payload = vec![0; 104 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&4u16.to_le_bytes());
    payload[58..60].copy_from_slice(&6u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1u32.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[96..100].copy_from_slice(&2u32.to_le_bytes());
    payload[100..104].copy_from_slice(&2u32.to_le_bytes());
    payload[104..].copy_from_slice(SKETCH_MARKER);

    assert!(current_compact_104_profile_line(&payload, 0));

    payload[100..104].copy_from_slice(&3u32.to_le_bytes());
    assert!(!current_compact_104_profile_line(&payload, 0));
}


#[test]
fn legacy_compact_104_profile_line_uses_one_based_point_indices() {
    let offset = 4;
    let mut payload = vec![0; offset + 104 + LEGACY_SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&2u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 23..offset + 31]
        .copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 60].copy_from_slice(&[6, 0, 8, 0]);
    payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[offset + 72..offset + 76].copy_from_slice(&1u32.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[offset + relative..offset + relative + 4]
            .copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[offset + 96..offset + 100].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 100..offset + 104].copy_from_slice(&3u32.to_le_bytes());
    payload[offset + 104..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_compact_104_profile_line_endpoint_indices(&payload, offset),
        Some([7, 9])
    );
    payload[offset + 96..offset + 100].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(
        legacy_compact_104_profile_line_endpoint_indices(&payload, offset),
        None
    );
}


#[test]
fn current_direct_92_profile_line_uses_point_object_ids() {
    let mut payload = vec![0; 92 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&6u16.to_le_bytes());
    payload[66..68].copy_from_slice(&9u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..88].copy_from_slice(&1u32.to_le_bytes());
    payload[88..92].copy_from_slice(&6u32.to_le_bytes());
    payload[92..].copy_from_slice(SKETCH_MARKER);

    assert_eq!(
        current_direct_92_profile_line_endpoint_indices(&payload, 0),
        Some([6, 9])
    );

    payload[88..92].fill(0);
    assert_eq!(
        current_direct_92_profile_line_endpoint_indices(&payload, 0),
        None
    );
}


#[test]
fn current_referenced_compact_line_uses_complete_one_based_marker_roster() {
    let curve_offset = 100;
    let mut payload = vec![0; curve_offset + 104 + SKETCH_MARKER.len()];
    payload[curve_offset..curve_offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[curve_offset + 5..curve_offset + 13].fill(0xff);
    payload[curve_offset + 13..curve_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[curve_offset + 23..curve_offset + 31]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[curve_offset + 31..curve_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[curve_offset + 48..curve_offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[curve_offset + 56..curve_offset + 58].copy_from_slice(&1u16.to_le_bytes());
    payload[curve_offset + 58..curve_offset + 60].copy_from_slice(&3u16.to_le_bytes());
    payload[curve_offset + 60..curve_offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 64..curve_offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[curve_offset + 72..curve_offset + 76].copy_from_slice(&1i32.to_le_bytes());
    payload[curve_offset + 76..curve_offset + 78].copy_from_slice(&22u16.to_le_bytes());
    for relative in [78, 82, 86, 90] {
        payload[curve_offset + relative..curve_offset + relative + 4]
            .copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[curve_offset + 96..curve_offset + 100].copy_from_slice(&13u32.to_le_bytes());
    payload[curve_offset + 100..curve_offset + 104].copy_from_slice(&7u32.to_le_bytes());
    payload[curve_offset + 104..].copy_from_slice(SKETCH_MARKER);

    let marker = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        marker("first", 0, SketchInputKind::Point, Some([1.0, 2.0])),
        marker(
            "relation",
            10,
            SketchInputKind::Relation(SketchRelationKind::Horizontal),
            None,
        ),
        marker("second", 20, SketchInputKind::Point, Some([3.0, 4.0])),
        marker("curve", 100, SketchInputKind::Arc, None),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    assert!(current_referenced_compact_curve_uses_marker_roster(
        &payload,
        curve_offset
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    let compact_104 = payload.clone();
    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 72..curve_offset + 76].copy_from_slice(&(-1i32).to_le_bytes());
    assert!(current_referenced_compact_curve_uses_marker_roster(
        &payload,
        curve_offset
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload = compact_104.clone();
    payload[curve_offset + 72..curve_offset + 104].fill(0);
    payload[curve_offset + 76..curve_offset + 80].copy_from_slice(&8u32.to_le_bytes());
    payload[curve_offset + 80..curve_offset + 84].copy_from_slice(&7u32.to_le_bytes());
    payload[curve_offset + 84..curve_offset + 84 + SKETCH_MARKER.len()]
        .copy_from_slice(SKETCH_MARKER);
    assert!(current_referenced_compact_curve_uses_marker_roster(
        &payload,
        curve_offset
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 56..curve_offset + 58].copy_from_slice(&0u16.to_le_bytes());
    payload[curve_offset + 58..curve_offset + 60].copy_from_slice(&1u16.to_le_bytes());
    assert!(current_referenced_compact_curve_uses_marker_roster(
        &payload,
        curve_offset
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload[curve_offset + 17..curve_offset + 21].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    payload = compact_104.clone();
    payload[curve_offset + 72..curve_offset + 104].fill(0);
    payload[curve_offset + 82..curve_offset + 84].copy_from_slice(&12u16.to_le_bytes());
    payload[curve_offset + 88..curve_offset + 92].copy_from_slice(&19u32.to_le_bytes());
    payload[curve_offset + 92..curve_offset + 96].copy_from_slice(&1u32.to_le_bytes());
    payload[curve_offset + 96..curve_offset + 96 + SKETCH_MARKER.len()]
        .copy_from_slice(SKETCH_MARKER);
    assert!(current_referenced_compact_curve_uses_marker_roster(
        &payload,
        curve_offset
    ));
    assert_eq!(
        coordinate_roster_curve_endpoint_markers(&payload, &entities[3], &markers)
            .iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    payload = compact_104;
    payload[curve_offset + 100..curve_offset + 104].copy_from_slice(&13u32.to_le_bytes());
    assert!(!current_referenced_compact_curve_uses_marker_roster(
        &payload,
        curve_offset
    ));
}


#[test]
fn extended_terminal_profile_record_is_a_line() {
    let mut payload = vec![0; 170];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[58..60].copy_from_slice(&1u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[142..144].copy_from_slice(&[0x08, 0x80]);
    payload[154..170].copy_from_slice(&[
        0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
        0x00,
    ]);

    assert!(extended_terminal_profile_line(&payload, 0));

    payload[142..144].fill(0);
    assert!(!extended_terminal_profile_line(&payload, 0));
}


#[test]
fn legacy_long_profile_line_uses_point_object_ids() {
    let mut payload = vec![0; 124];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[19..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..33].copy_from_slice(&4u16.to_le_bytes());
    payload[42..44].copy_from_slice(&6u16.to_le_bytes());
    payload[44..46].copy_from_slice(&8u16.to_le_bytes());
    payload[46..50].copy_from_slice(&1u32.to_le_bytes());
    payload[50..58].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[58..62].copy_from_slice(&1u32.to_le_bytes());
    payload[62..64].copy_from_slice(&7u16.to_le_bytes());
    for relative in [64, 68, 72, 76] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[120..124].copy_from_slice(&16u32.to_le_bytes());

    assert_eq!(
        legacy_long_profile_line_endpoint_indices(&payload, 0),
        Some([6, 8])
    );

    payload[120..124].fill(0);
    assert_eq!(legacy_long_profile_line_endpoint_indices(&payload, 0), None);
}


#[test]
fn current_long_full_circle_indexes_its_radial_point() {
    let mut payload = vec![0; 154];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..68].copy_from_slice(&[1, 0, 1, 0]);
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&1u32.to_le_bytes());
    for relative in [86, 90, 94, 98] {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[134..136].copy_from_slice(&4u16.to_le_bytes());
    payload[136..154].copy_from_slice(&[
        0xf1, 0x80, 0x00, 0x00, 0x00, 0x00, 0xf3, 0x80, 0x04, 0x80, 0xff, 0xfe, 0xff, 0x02, 0x44,
        0x00, 0x31, 0x00,
    ]);

    assert_eq!(current_long_full_circle_radial_index(&payload, 0), Some(1));

    payload[66..68].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(current_long_full_circle_radial_index(&payload, 0), None);
}


#[test]
fn extended_wide_construction_line_indexes_the_complete_marker_roster() {
    let mut payload = vec![0; 92 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&14u16.to_le_bytes());
    payload[66..68].copy_from_slice(&15u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..88].copy_from_slice(&1u32.to_le_bytes());
    payload[88..92].copy_from_slice(&5u32.to_le_bytes());
    payload[92..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        extended_wide_construction_line_roster_indices(&payload, 0),
        Some([14, 15])
    );
    payload[66..68].copy_from_slice(&14u16.to_le_bytes());
    assert_eq!(
        extended_wide_construction_line_roster_indices(&payload, 0),
        None
    );
    payload[66..68].copy_from_slice(&15u16.to_le_bytes());
    payload[82] = 1;
    payload[84..88].fill(0);
    assert_eq!(
        extended_wide_construction_line_roster_indices(&payload, 0),
        Some([14, 15])
    );
    payload[82] = 2;
    assert_eq!(
        extended_wide_construction_line_roster_indices(&payload, 0),
        None
    );
    payload[82] = 0;
    assert_eq!(
        extended_wide_construction_line_roster_indices(&payload, 0),
        None
    );
}


#[test]
fn terminal_legacy_wide_curve_indexes_the_coordinate_roster() {
    let mut payload = vec![0; 128];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&12u16.to_le_bytes());
    payload[66..68].copy_from_slice(&13u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());

    assert_eq!(
        super::wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([13, 14])
    );
    assert_eq!(
        super::coordinate_roster_endpoint_offset(&payload, 0),
        Some(64)
    );

    payload[127] = 1;
    assert_eq!(
        super::wide_indexed_curve_endpoint_indices(&payload, 0),
        None
    );
}


#[test]
fn terminal_legacy_profile_curve_addresses_consecutive_point_identities() {
    let mut wide = vec![0; 92 + LEGACY_SKETCH_MARKER.len()];
    wide[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    wide[5..13].fill(0xff);
    wide[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    wide[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    wide[27..29].copy_from_slice(&1u16.to_le_bytes());
    wide[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    wide[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    wide[64..66].copy_from_slice(&15u16.to_le_bytes());
    wide[66..68].copy_from_slice(&16u16.to_le_bytes());
    wide[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    wide[80..84].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    wide[84..88].copy_from_slice(&9u32.to_le_bytes());
    wide[88..92].copy_from_slice(&12u32.to_le_bytes());
    wide[92..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(legacy_terminal_profile_endpoint_offset(&wide, 0), Some(64));
    assert_eq!(legacy_state_five_curve_endpoint_indices(&wide, 0), None);
    assert!(legacy_undetailed_profile_line(&wide, 0));
    let mut compact = wide;
    compact.copy_within(64..84, 56);
    compact.truncate(84 + LEGACY_SKETCH_MARKER.len());
    compact[76..80].copy_from_slice(&9u32.to_le_bytes());
    compact[80..84].copy_from_slice(&12u32.to_le_bytes());
    compact[84..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        legacy_terminal_profile_endpoint_offset(&compact, 0),
        Some(56)
    );
    assert_eq!(legacy_state_five_curve_endpoint_indices(&compact, 0), None);
    assert!(legacy_undetailed_profile_line(&compact, 0));

    compact[72..76].fill(0);
    assert_eq!(legacy_terminal_profile_endpoint_offset(&compact, 0), None);
}


#[test]
fn unlocated_legacy_geometry_handle_has_no_neutral_geometry() {
    let mut payload = vec![0; 142 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x12, 0x00]);
    payload[92..96].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    payload[142..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert!(legacy_unlocated_geometry_handle(&payload, 0));
    payload[92] = 0;
    assert!(!legacy_unlocated_geometry_handle(&payload, 0));
}


#[test]
fn compact_legacy_profile_selected_axis_indexes_the_coordinate_roster() {
    let mut payload = vec![0; 92 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&8u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[92..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        super::legacy_profile_roster_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[80..84].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    assert_eq!(
        super::legacy_profile_roster_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[80..84].fill(0);
    assert_eq!(
        super::legacy_profile_roster_selected_axis_endpoint_indices(&payload, 0),
        None
    );
    payload[84..88].copy_from_slice(&1u32.to_le_bytes());
    payload[88..92].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        super::legacy_profile_roster_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[88..92].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        super::legacy_profile_roster_selected_axis_endpoint_indices(&payload, 0),
        None
    );

    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[80..84].fill(0);
    payload[84..88].fill(0);
    payload[88..92].copy_from_slice(&29u32.to_le_bytes());
    assert_eq!(
        super::legacy_profile_roster_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[88..92].fill(0);
    assert_eq!(
        super::legacy_profile_roster_selected_axis_endpoint_indices(&payload, 0),
        None
    );
}


#[test]
fn standard_legacy_compact_selected_axis_indexes_the_coordinate_roster() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&7u16.to_le_bytes());
    payload[58..60].copy_from_slice(&8u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[74..76].copy_from_slice(&2u16.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        super::standard_legacy_compact_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
}


#[test]
fn compact_legacy_selected_axis_distinguishes_direct_and_roster_ids() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&2u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[74] = 1;
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_direct_compact_selected_axis_endpoint_indices(&payload, 0),
        Some([2, 3])
    );
    payload[72..76].fill(0);
    payload[76..80].copy_from_slice(&1u32.to_le_bytes());
    payload[80..84].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        legacy_direct_compact_selected_axis_endpoint_indices(&payload, 0),
        None
    );
    assert_eq!(
        super::legacy_compact_roster_selected_axis_endpoint_indices(&payload, 0),
        Some([3, 4])
    );
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[80..84].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        super::legacy_compact_roster_selected_axis_endpoint_indices(&payload, 0),
        None
    );
    payload[80..84].copy_from_slice(&1u32.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        super::legacy_compact_roster_selected_axis_endpoint_indices(&payload, 0),
        None
    );
}


#[test]
fn legacy_code_six_axis_excludes_role_two_code_three_chords() {
    let mut payload = vec![0; 92 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&6u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&8u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    payload[88..92].copy_from_slice(&2u32.to_le_bytes());
    payload[92..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        super::legacy_code_five_or_six_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[17..21].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        super::legacy_code_five_or_six_selected_axis_endpoint_indices(&payload, 0),
        None
    );
    assert!(!marker_is_selected_construction_line(&payload, 0));
}


#[test]
fn legacy_code_five_axis_requires_distinct_trailing_identities() {
    let mut payload = vec![0; 92 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&5u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&8u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..88].copy_from_slice(&1u32.to_le_bytes());
    payload[88..92].copy_from_slice(&3u32.to_le_bytes());
    payload[92..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        super::legacy_code_five_or_six_selected_axis_endpoint_indices(&payload, 0),
        Some([8, 9])
    );
    assert!(marker_is_selected_construction_line(&payload, 0));
    payload[88..92].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        super::legacy_code_five_or_six_selected_axis_endpoint_indices(&payload, 0),
        None
    );
    assert!(!marker_is_selected_construction_line(&payload, 0));
}


#[test]
fn compact_legacy_state_five_line_indexes_the_coordinate_roster() {
    let mut payload = vec![0; 92 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&6u16.to_le_bytes());
    payload[66..68].copy_from_slice(&9u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[92..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&payload, 0),
        Some([7, 10])
    );
    assert!(legacy_undetailed_profile_line(&payload, 0));
    payload[68..70].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&payload, 0),
        Some([7, 10])
    );
    payload[70..72].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(legacy_state_five_curve_endpoint_indices(&payload, 0), None);
    payload[68..72].fill(0);

    let mut compact = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    compact[..56].copy_from_slice(&payload[..56]);
    compact[56..58].copy_from_slice(&6u16.to_le_bytes());
    compact[58..60].copy_from_slice(&9u16.to_le_bytes());
    compact[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    compact[84..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&compact, 0),
        Some([7, 10])
    );
    assert!(legacy_undetailed_profile_line(&compact, 0));
    compact[74..76].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&compact, 0),
        Some([7, 10])
    );
    compact[74..76].fill(0);
    compact[60..64].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&compact, 0),
        Some([7, 10])
    );
    compact[74..76].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        legacy_state_five_curve_endpoint_indices(&compact, 0),
        Some([7, 10])
    );
}


#[test]
fn terminal_compact_indexed_curve_owns_its_endpoint_trailer() {
    let mut payload = vec![0; 102];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&7u16.to_le_bytes());
    payload[58..60].copy_from_slice(&9u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&u32::MAX.to_le_bytes());
    payload[76..78].copy_from_slice(&8u16.to_le_bytes());
    for at in (78..94).step_by(4) {
        payload[at..at + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }

    assert_eq!(
        compact_indexed_curve_endpoint_indices(&payload, 0),
        Some([8, 10])
    );

    payload[90] = 0;
    assert_eq!(compact_indexed_curve_endpoint_indices(&payload, 0), None);
}


#[test]
fn extended_compact_indexed_curves_own_their_endpoint_trailers() {
    let marker = |size: usize| {
        let mut payload = vec![0; size + LEGACY_SKETCH_MARKER.len()];
        payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[27..29].copy_from_slice(&1u16.to_le_bytes());
        payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[56..58].copy_from_slice(&4u16.to_le_bytes());
        payload[58..60].copy_from_slice(&8u16.to_le_bytes());
        payload[60..64].copy_from_slice(&1u32.to_le_bytes());
        payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[size..].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload
    };

    let mut compact_96 = marker(96);
    compact_96[82..84].copy_from_slice(&3u16.to_le_bytes());
    compact_96[88..92].copy_from_slice(&4u32.to_le_bytes());
    compact_96[92..96].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&compact_96, 0),
        Some([5, 9])
    );
    let valid_compact_96 = compact_96.clone();
    compact_96[84] = 1;
    assert_eq!(compact_indexed_curve_endpoint_indices(&compact_96, 0), None);

    let mut compact_104 = marker(104);
    compact_104[72..76].copy_from_slice(&(-1i32).to_le_bytes());
    compact_104[76..78].copy_from_slice(&5u16.to_le_bytes());
    for at in (78..94).step_by(4) {
        compact_104[at..at + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    compact_104[96..100].copy_from_slice(&6u32.to_le_bytes());
    compact_104[100..104].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&compact_104, 0),
        Some([5, 9])
    );
    let valid_compact_104 = compact_104.clone();
    compact_104[94] = 1;
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&compact_104, 0),
        None
    );
    let mut current_compact_104 = valid_compact_104.clone();
    current_compact_104[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    current_compact_104[17..21].copy_from_slice(&2u32.to_le_bytes());
    current_compact_104[104..].copy_from_slice(SKETCH_MARKER);
    assert!(current_undetailed_bounded_curve_is_line(
        &current_compact_104,
        0
    ));
    current_compact_104[58..60].copy_from_slice(&4u16.to_le_bytes());
    assert!(!current_undetailed_bounded_curve_is_line(
        &current_compact_104,
        0
    ));

    let extended = |mut payload: Vec<u8>| {
        payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        let size = payload.len() - LEGACY_SKETCH_MARKER.len();
        payload[size..size + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload
    };
    let normalized_payload = extended(valid_compact_96);
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&normalized_payload, 0),
        Some([5, 9])
    );
    assert!(current_undetailed_bounded_curve_is_line(
        &normalized_payload,
        0
    ));
    let entity = |id: &str, offset, object_index, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: normalized_payload,
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![
            entity("curve", 0, None, None),
            entity("start", 1, Some(5), Some([0.0, 0.0])),
            entity("end", 2, Some(9), Some([1.0, 0.0])),
        ],
    };
    normalize_indexed_curve_entities(&mut lane);
    assert_eq!(lane.sketch_entities[1].kind, SketchInputKind::Point);
    assert_eq!(lane.sketch_entities[2].kind, SketchInputKind::Point);
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&extended(valid_compact_104), 0,),
        None
    );

    let mut continuation_120 = vec![0; 140];
    continuation_120[..80].copy_from_slice(&marker(84)[..80]);
    continuation_120[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    continuation_120[120..122].copy_from_slice(&32u16.to_le_bytes());
    continuation_120[122..126].copy_from_slice(CLASS_MARKER);
    continuation_120[126..128].copy_from_slice(&12u16.to_le_bytes());
    continuation_120[128..].copy_from_slice(b"sgPntPntDist");
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&continuation_120, 0),
        Some([5, 9])
    );
    continuation_120[122..140].copy_from_slice(&[
        0xf7, 0x81, 0x00, 0x00, 0x00, 0x00, 0xe6, 0x81, 0x1c, 0x81, 0xff, 0xfe, 0xff, 0x02,
        0x44, 0x00, 0x31, 0x00,
    ]);
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&continuation_120, 0),
        Some([5, 9])
    );
    continuation_120[130..132].copy_from_slice(&[0xe6, 0x81]);
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&continuation_120, 0),
        None
    );
    continuation_120[130..132].copy_from_slice(&[0x1c, 0x81]);
    continuation_120[119] = 1;
    assert_eq!(
        super::extended_compact_indexed_curve_endpoint_indices(&continuation_120, 0),
        None
    );

    let mut reference_table_126 = vec![0; 206];
    reference_table_126[..80].copy_from_slice(&marker(84)[..80]);
    reference_table_126[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    reference_table_126[126..128].copy_from_slice(&12u16.to_le_bytes());
    reference_table_126[136..140].fill(0xff);
    reference_table_126[154..158].copy_from_slice(&5u32.to_le_bytes());
    reference_table_126[158..162].copy_from_slice(&2u32.to_le_bytes());
    reference_table_126[166..170].copy_from_slice(&[0xfe, 0xff, 0x00, 0x00]);
    reference_table_126[170..172].copy_from_slice(&0x88c5u16.to_le_bytes());
    reference_table_126[174..178].fill(0xff);
    reference_table_126[190..194].fill(0xff);
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&reference_table_126, 0),
        Some([5, 9])
    );
    reference_table_126[126..128].fill(0);
    assert_eq!(
        compact_indexed_curve_endpoint_indices(&reference_table_126, 0),
        None
    );
}


#[test]
fn duplicated_compact_curve_address_identifies_a_radial_circle_witness() {
    let marker = |construction: bool| {
        let mut payload = vec![0; 104 + LEGACY_SKETCH_MARKER.len()];
        payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&(if construction { 7u32 } else { 2 }).to_le_bytes());
        payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[27..29].copy_from_slice(&(if construction { 2u16 } else { 1 }).to_le_bytes());
        payload[31..39].copy_from_slice(&[
            0x00,
            0x00,
            0x80,
            0xbf,
            0x00,
            0x00,
            if construction { 0x0c } else { 0x04 },
            0x00,
        ]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[56..58].copy_from_slice(&9u16.to_le_bytes());
        payload[58..60].copy_from_slice(&9u16.to_le_bytes());
        payload[60..64].copy_from_slice(&u32::from(!construction).to_le_bytes());
        payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[72..76].copy_from_slice(&1i32.to_le_bytes());
        payload[76..78].copy_from_slice(&(if construction { 8u16 } else { 4 }).to_le_bytes());
        for at in (78..94).step_by(4) {
            payload[at..at + 4].copy_from_slice(&(-2i32).to_le_bytes());
        }
        payload[104..].copy_from_slice(LEGACY_SKETCH_MARKER);
        if construction {
            payload[9..13].copy_from_slice(&[0x04, 0x00, 0xff, 0xff]);
        } else {
            payload[29..31].copy_from_slice(&1u16.to_le_bytes());
        }
        payload
    };

    for construction in [false, true] {
        let mut payload = marker(construction);
        assert_eq!(compact_radial_circle_index(&payload, 0), Some(9));
        payload[58..60].copy_from_slice(&10u16.to_le_bytes());
        assert_eq!(compact_radial_circle_index(&payload, 0), None);
    }
    let mut extended_envelope = marker(false);
    extended_envelope[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(compact_radial_circle_index(&extended_envelope, 0), Some(9));
    let mut terminal = marker(false);
    terminal.truncate(102);
    assert_eq!(compact_radial_circle_index(&terminal, 0), Some(9));
}


#[test]
fn radial_dimensions_normalize_radius_and_diameter_displays() {
    let parameter = |display, value| DesignParameter {
        id: ParameterId("radial".into()),
        owner: Some(FeatureId("sketch".into())),
        ordinal: 0,
        name: "radial".into(),
        expression: String::new(),
        display,
        value: Some(ParameterValue::Length(Length(value))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };

    assert_eq!(
        radial_dimension_radius(&parameter(Some(DimensionDisplay::Radius), 2.0)),
        Some(2.0)
    );
    assert_eq!(
        radial_dimension_radius(&parameter(Some(DimensionDisplay::Diameter), 4.0)),
        Some(2.0)
    );
    assert_eq!(radial_dimension_radius(&parameter(None, 2.0)), None);
    assert_eq!(
        radial_dimension_radius(&parameter(Some(DimensionDisplay::Radius), -2.0)),
        None
    );
}


#[test]
fn terminal_radial_address_resolves_every_consecutive_equal_radius_pair() {
    let marker = |ordinal: u32, object_index: u32, coordinates_m: [f64; 2]| SketchInputEntity {
        id: format!("marker-{ordinal}"),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal,
        offset: u64::from(ordinal) * 100,
        object_index: Some(object_index),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some(coordinates_m),
        links: Vec::new(),
        link_selector: None,
    };
    let markers = [
        marker(0, 2, [0.0, 0.0]),
        marker(1, 1, [0.021, 0.0]),
        marker(2, 7, [-0.012, 0.012]),
        marker(3, 6, [-0.0095, 0.012]),
        marker(4, 9, [-0.012, -0.012]),
        marker(5, 8, [-0.0095, -0.012]),
        marker(6, 11, [0.012, -0.012]),
        marker(7, 10, [0.0145, -0.012]),
        marker(8, 13, [0.012, 0.012]),
        marker(9, 12, [0.0145, 0.012]),
    ];
    let roster = markers.iter().collect::<Vec<_>>();

    let pairs = terminal_repeated_radial_circle_pairs(roster.len(), &roster, 0.0025)
        .expect("terminal one-based address and repeated radius");
    assert_eq!(pairs.len(), 4);
    assert_eq!(
        pairs
            .iter()
            .map(|(center, radial)| (center.object_index, radial.object_index))
            .collect::<Vec<_>>(),
        vec![
            (Some(7), Some(6)),
            (Some(9), Some(8)),
            (Some(11), Some(10)),
            (Some(13), Some(12)),
        ]
    );
    assert!(terminal_repeated_radial_circle_pairs(roster.len() - 1, &roster, 0.0025).is_none());
    assert!(terminal_repeated_radial_circle_pairs(roster.len(), &roster, 0.003).is_none());
}


#[test]
fn extended_terminal_radial_record_carries_a_one_based_roster_address() {
    let mut payload = vec![0; 112];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&12u16.to_le_bytes());
    payload[58..60].copy_from_slice(&12u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&(-1i32).to_le_bytes());
    payload[76..78].copy_from_slice(&11u16.to_le_bytes());
    for at in (78..94).step_by(4) {
        payload[at..at + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }

    assert_eq!(
        extended_terminal_repeated_radial_circle_index(&payload, 0),
        Some(12)
    );
    payload[58..60].copy_from_slice(&13u16.to_le_bytes());
    assert_eq!(
        extended_terminal_repeated_radial_circle_index(&payload, 0),
        None
    );
}


#[test]
fn duplicated_extended_curve_address_identifies_a_radial_circle_roster() {
    let mut payload = vec![0; 112 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&7u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[80..84].copy_from_slice(&1u32.to_le_bytes());
    payload[112..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(extended_radial_circle_index(&payload, 0), Some(7));
    payload[66..68].copy_from_slice(&8u16.to_le_bytes());
    assert_eq!(extended_radial_circle_index(&payload, 0), None);
}


#[test]
fn wide_indexed_curve_owns_its_endpoint_trailer_in_all_generations() {
    let detail = 92;
    let mut payload = vec![0; detail + 80];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&6u16.to_le_bytes());
    payload[66..68].copy_from_slice(&10u16.to_le_bytes());
    payload[68..72].copy_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[detail..detail + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[detail + 5..detail + 13]
        .copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]);
    payload[detail + 13..detail + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[detail + 23..detail + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[detail + 27..detail + 29].copy_from_slice(&2u16.to_le_bytes());
    payload[detail + 31..detail + 35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[detail + 35..detail + 39].copy_from_slice(&[0x00, 0x00, 0x0c, 0x00]);
    payload[detail + 48..detail + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[detail + 64..detail + 72].copy_from_slice(&(-1.0f64).to_le_bytes());

    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    assert_eq!(marker_local_links(&payload, 0), None);
    assert!(!marker_is_selected_construction_line(&payload, 0));
    assert_eq!(
        compact_bounded_curve_tangent(&payload, 0),
        Some([-1.0, 0.0])
    );

    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[detail + 23..detail + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );

    let entity = |id: &str,
                  offset: u64,
                  object_index: Option<u32>,
                  coordinates_m: Option<[f64; 2]>,
                  kind: SketchInputKind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload.clone(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![
            entity("curve", 0, None, None, SketchInputKind::Arc),
            entity(
                "start",
                1,
                Some(7),
                Some([0.0, 0.0]),
                SketchInputKind::LineOrCircle,
            ),
            entity(
                "end",
                2,
                Some(11),
                Some([1.0, 0.0]),
                SketchInputKind::LineOrCircle,
            ),
        ],
    };
    normalize_indexed_curve_entities(&mut lane);
    assert_eq!(lane.sketch_entities[1].kind, SketchInputKind::Point);
    assert_eq!(lane.sketch_entities[2].kind, SketchInputKind::Point);

    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x44, 0x00]);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x84, 0x00]);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x04, 0x00]);

    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[detail + 23..detail + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    assert!(!marker_is_selected_construction_line(&payload, 0));
    assert_eq!(
        compact_bounded_curve_tangent(&payload, 0),
        Some([-1.0, 0.0])
    );

    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[detail + 23..detail + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&payload, 0),
        Some([7, 11])
    );
    assert_eq!(marker_local_links(&payload, 0), None);
    assert_eq!(
        compact_bounded_curve_tangent(&payload, 0),
        Some([-1.0, 0.0])
    );
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );

    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Arc
    );

    let mut coordinate_line = vec![0; 134 + SKETCH_MARKER.len()];
    coordinate_line[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    coordinate_line[5..13].fill(0xff);
    coordinate_line[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    coordinate_line[17..21].copy_from_slice(&2u32.to_le_bytes());
    coordinate_line[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    coordinate_line[27..29].copy_from_slice(&1u16.to_le_bytes());
    coordinate_line[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    coordinate_line[64..66].copy_from_slice(&[0x1e, 0x00]);
    coordinate_line[66..74].copy_from_slice(&0.015f64.to_le_bytes());
    coordinate_line[74..82].copy_from_slice(&0.0f64.to_le_bytes());
    coordinate_line[134..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        sketch_input_entities(&coordinate_line, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );

    let mut legacy_112 = vec![0; 112 + LEGACY_SKETCH_MARKER.len()];
    legacy_112[..80].copy_from_slice(&payload[..80]);
    legacy_112[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    legacy_112[80..84].copy_from_slice(&1i32.to_le_bytes());
    legacy_112[84..86].copy_from_slice(&4u16.to_le_bytes());
    for offset in (86..102).step_by(4) {
        legacy_112[offset..offset + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    legacy_112[104..108].copy_from_slice(&583u32.to_le_bytes());
    legacy_112[108..112].copy_from_slice(&450u32.to_le_bytes());
    legacy_112[112..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&legacy_112, 0),
        Some([7, 11])
    );
    legacy_112[98] = 0;
    assert_eq!(wide_indexed_curve_endpoint_indices(&legacy_112, 0), None);

    let mut current_112 = legacy_112;
    current_112[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    current_112[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    current_112[29..31].copy_from_slice(&1u16.to_le_bytes());
    current_112[35..39].copy_from_slice(&[0x00, 0x00, 0x44, 0x00]);
    current_112[80..84].copy_from_slice(&(-1i32).to_le_bytes());
    current_112[98..102].copy_from_slice(&(-2i32).to_le_bytes());
    current_112[112..112 + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&current_112, 0),
        Some([7, 11])
    );
    let mut current_112_with_detail = current_112.clone();
    current_112_with_detail.resize(112 + 80, 0);
    current_112_with_detail[112..112 + 80].copy_from_slice(&payload[detail..detail + 80]);
    assert_eq!(
        compact_bounded_curve_tangent(&current_112_with_detail, 0),
        Some([-1.0, 0.0])
    );
    current_112_with_detail[84..86].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        compact_bounded_curve_tangent(&current_112_with_detail, 0),
        None
    );
    current_112[17..21].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(wide_indexed_curve_endpoint_indices(&current_112, 0), None);
    current_112[17..21].copy_from_slice(&2u32.to_le_bytes());
    current_112[84..86].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(wide_indexed_curve_endpoint_indices(&current_112, 0), None);

    let mut legacy_terminal = vec![0; 156];
    legacy_terminal[..80].copy_from_slice(&payload[..80]);
    legacy_terminal[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    legacy_terminal[17..21].copy_from_slice(&1u32.to_le_bytes());
    legacy_terminal[80..84].copy_from_slice(&1i32.to_le_bytes());
    legacy_terminal[84..86].copy_from_slice(&12u16.to_le_bytes());
    for offset in (86..102).step_by(4) {
        legacy_terminal[offset..offset + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    legacy_terminal[136..138].copy_from_slice(&[0x05, 0x00]);
    legacy_terminal[138..142].copy_from_slice(CLASS_MARKER);
    legacy_terminal[142..144].copy_from_slice(&12u16.to_le_bytes());
    legacy_terminal[144..].copy_from_slice(b"sgPntPntDist");
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&legacy_terminal, 0),
        Some([7, 11])
    );
    assert_eq!(
        coordinate_roster_endpoint_offset(&legacy_terminal, 0),
        Some(64)
    );
    legacy_terminal[135] = 1;
    assert_eq!(
        wide_indexed_curve_endpoint_indices(&legacy_terminal, 0),
        None
    );
}


#[test]
fn current_wide_arc_uses_direct_point_ids_with_an_arc_center_carrier() {
    let mut payload = vec![0; 92 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&6u16.to_le_bytes());
    payload[66..68].copy_from_slice(&5u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..88].copy_from_slice(&4u32.to_le_bytes());
    payload[88..92].copy_from_slice(&3u32.to_le_bytes());
    payload[92..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, object_index, coordinates_m, kind: SketchInputKind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset: 0,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        entity("curve", Some(2), None, SketchInputKind::Arc),
        entity("center", Some(4), Some([0.0, 0.0]), SketchInputKind::Arc),
        entity("start", Some(6), Some([0.0, 1.0]), SketchInputKind::Point),
        entity("end", Some(5), Some([0.0, -1.0]), SketchInputKind::Point),
        entity("shifted", Some(7), Some([2.0, 0.0]), SketchInputKind::Point),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    let (endpoints, center) = current_wide_arc_direct_markers(&payload, &entities[0], &markers)
        .expect("direct endpoint IDs");
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["start", "end"]
    );
    assert_eq!(
        coordinate_roster_arc_center(
            &payload,
            &entities[0],
            &markers,
            [endpoints[0], endpoints[1]],
        ),
        Some(center)
    );
}


#[test]
fn linked_semicircle_records_close_a_two_center_profile() {
    let mut payload = vec![0; 224];
    for (offset, addresses) in [(0, [1u16, 2]), (112, [3, 5])] {
        payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
        payload[offset + 23..offset + 31]
            .copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 64..offset + 66].copy_from_slice(&addresses[0].to_le_bytes());
        payload[offset + 66..offset + 68].copy_from_slice(&addresses[1].to_le_bytes());
        payload[offset + 68..offset + 72].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 72..offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[offset + 80..offset + 84].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 86..offset + 102].copy_from_slice(&[
            0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
            0xff, 0xff,
        ]);
    }
    assert!(current_linked_semicircle_record(&payload, 0));
    assert!(current_linked_semicircle_record(&payload, 112));
    let marker = |id: &str, offset, center: &str| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: vec![SketchInputLink {
            entity_ref: center.into(),
            local_id: 1,
        }],
        link_selector: Some(1),
    };
    let records = [
        marker("curve-a", 0, "center-a"),
        marker("curve-b", 112, "center-b"),
    ];
    let markers = records.iter().collect::<Vec<_>>();
    let sketch = SketchId("sketch".into());
    let point = |id: &str, position| SketchEntity {
        id: SketchEntityId(format!("entity-{id}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let curve = |id: &str| SketchEntity {
        id: SketchEntityId(format!("entity-{id}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Native {
            native_kind: "sldprt:marker-geometry:1".into(),
        },
    };
    let mut entities = vec![
        point("center-a", Point2::new(0.0, 0.0)),
        point("a-plus", Point2::new(0.0, 2.0)),
        point("a-minus", Point2::new(0.0, -2.0)),
        point("center-b", Point2::new(3.0, 0.0)),
        point("b-plus", Point2::new(3.0, 2.0)),
        point("b-minus", Point2::new(3.0, -2.0)),
        curve("curve-a"),
        curve("curve-b"),
    ];

    resolve_two_center_semicircle_profile(&payload, &markers, &mut entities, 1.0e-9);

    assert_eq!(
        entities
            .iter()
            .filter(|entity| matches!(entity.geometry, SketchGeometry::Arc { .. }))
            .count(),
        2
    );
    assert_eq!(
        entities
            .iter()
            .filter(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
            .count(),
        2
    );
    assert!(entities
        .iter()
        .filter_map(|entity| match entity.geometry {
            SketchGeometry::Arc {
                radius: Length(radius),
                ..
            } => Some(radius),
            _ => None,
        })
        .all(|radius| (radius - 2.0).abs() < 1.0e-9));
}


#[test]
fn wide_line_uses_direct_point_ids_after_one_based_resolution_fails() {
    let mut payload = vec![0; 92 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&7u16.to_le_bytes());
    payload[66..68].copy_from_slice(&8u16.to_le_bytes());
    payload[68..72].copy_from_slice(&1u32.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[92..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, object_index, coordinates_m, kind: SketchInputKind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset: 0,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        entity("curve", Some(6), None, SketchInputKind::LineOrCircle),
        entity("start", Some(7), Some([-1.0, 0.0]), SketchInputKind::Point),
        entity("end", Some(8), Some([1.0, 0.0]), SketchInputKind::Point),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    let endpoints = wide_direct_line_endpoint_markers(&payload, &entities[0], &markers)
        .expect("direct point IDs");
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["start", "end"]
    );

    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[64..66].fill(0);
    let zero = entity("zero", None, Some([0.0, 0.0]), SketchInputKind::Point);
    let extended = [&entities[0], &zero, &entities[2]];
    let endpoints = wide_direct_line_endpoint_markers(&payload, &entities[0], &extended)
        .expect("unique zero-identity point");
    assert_eq!(endpoints[0].id, "zero");

    let other_zero = entity("other-zero", None, Some([2.0, 0.0]), SketchInputKind::Point);
    let ambiguous = [&entities[0], &zero, &other_zero, &entities[2]];
    assert_eq!(
        wide_direct_line_endpoint_markers(&payload, &entities[0], &ambiguous),
        None
    );
    payload[92] = 0;
    assert_eq!(
        wide_direct_line_endpoint_markers(&payload, &entities[0], &extended),
        None
    );
}


#[test]
fn current_line_resolves_one_based_point_roster_endpoints() {
    let mut payload = vec![0; 84 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&2u16.to_le_bytes());
    payload[58..60].copy_from_slice(&4u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, offset, coordinates_m, kind: SketchInputKind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, None, SketchInputKind::LineOrCircle);
    let points = [
        entity("first", 10, Some([0.0, 0.0]), SketchInputKind::Point),
        entity("second", 20, Some([1.0, 0.0]), SketchInputKind::Point),
        entity("third", 30, Some([1.0, 1.0]), SketchInputKind::Point),
        entity("fourth", 40, Some([0.0, 1.0]), SketchInputKind::Point),
    ];
    let markers = std::iter::once(&curve)
        .chain(points.iter())
        .collect::<Vec<_>>();

    let endpoints = one_based_point_roster_line_endpoint_markers(&payload, &curve, &markers)
        .expect("one-based point roster");
    assert_eq!(
        endpoints.map(|endpoint| endpoint.id.as_str()),
        ["second", "fourth"]
    );

    let arc = entity("arc", 50, None, SketchInputKind::Arc);
    let mixed = markers
        .iter()
        .copied()
        .chain(std::iter::once(&arc))
        .collect::<Vec<_>>();
    assert_eq!(
        one_based_point_roster_line_endpoint_markers(&payload, &curve, &mixed),
        None
    );

    payload[56..58].fill(0);
    assert_eq!(
        one_based_point_roster_line_endpoint_markers(&payload, &curve, &markers),
        None
    );
}


#[test]
fn legacy_geometry_locus_line_resolves_zero_based_point_roster_endpoints() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[76..80].copy_from_slice(&5u32.to_le_bytes());
    payload[80..84].copy_from_slice(&4u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let entity = |id: &str, offset, coordinates_m, kind: SketchInputKind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, None, SketchInputKind::LineOrCircle);
    let points = [
        entity("first", 10, Some([0.0, 0.0]), SketchInputKind::Point),
        entity("second", 20, Some([1.0, 0.0]), SketchInputKind::Point),
        entity("third", 30, Some([1.0, 1.0]), SketchInputKind::Point),
        entity("fourth", 40, Some([0.0, 1.0]), SketchInputKind::Point),
    ];
    let markers = std::iter::once(&curve)
        .chain(points.iter())
        .collect::<Vec<_>>();

    let endpoints = legacy_point_roster_line_endpoint_markers(&payload, &curve, &markers)
        .expect("zero-based point roster");
    assert_eq!(
        endpoints.map(|endpoint| endpoint.id.as_str()),
        ["second", "fourth"]
    );

    payload[80..84].fill(0xff);
    assert_eq!(
        legacy_point_roster_line_endpoint_markers(&payload, &curve, &markers),
        None
    );
}


#[test]
fn extended_marker104_arc_prefers_point_roster_endpoints() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&4u16.to_le_bytes());
    payload[58..60].copy_from_slice(&6u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    for start in (78..94).step_by(4) {
        payload[start..start + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[96..100].copy_from_slice(&2u32.to_le_bytes());
    payload[100..104].copy_from_slice(&2u32.to_le_bytes());
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    let entity = |id: &str, offset, object_index, coordinates_m, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = entity("curve", 0, None, None, SketchInputKind::Arc);
    let object_indices = [1, 2, 4, 5, 6, 7, 8];
    let points = object_indices.map(|object_index| {
        entity(
            &format!("point-{object_index}"),
            u64::from(object_index) * 10,
            Some(object_index),
            Some([f64::from(object_index), 0.0]),
            SketchInputKind::Point,
        )
    });
    let markers = std::iter::once(&curve)
        .chain(points.iter())
        .collect::<Vec<_>>();

    assert!(indexed_arc_uses_coordinate_center(&payload, 0));
    assert_eq!(coordinate_roster_endpoint_offset(&payload, 0), Some(56));
    let endpoints = roster_curve_endpoint_markers(&payload, &curve, &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["point-6", "point-8"]
    );
}


#[test]
fn indexed_curve_vertex_binding_follows_the_resolved_coordinate_roster() {
    let mut payload = vec![0; 104 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    for start in (78..94).step_by(4) {
        payload[start..start + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let entity = |id: &str, offset, object_index, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![
            entity("curve", 0, Some(1), SketchInputKind::Arc, None),
            entity("handle", 1, None, SketchInputKind::Point, Some([-1.0, 0.0])),
            entity(
                "start",
                2,
                Some(2),
                SketchInputKind::Point,
                Some([0.0, 0.0]),
            ),
            entity("center", 3, None, SketchInputKind::Point, Some([0.5, 0.5])),
            entity(
                "end",
                4,
                Some(3),
                SketchInputKind::LineOrCircle,
                Some([1.0, 0.0]),
            ),
        ],
    };

    normalize_indexed_curve_entities(&mut lane);
    bind_resolved_curve_vertices(&mut lane);

    assert_eq!(lane.sketch_entities[4].kind, SketchInputKind::Point);
}


#[test]
fn legacy_compact_geometry_locus_code_two_is_a_profile_line() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&2u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[74..76].copy_from_slice(&2u16.to_le_bytes());
    payload[76..80].copy_from_slice(&4u32.to_le_bytes());
    payload[80..84].copy_from_slice(&3u32.to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert!(legacy_compact_profile_line(&payload, 0));
    let entities = sketch_input_entities(&payload, "lane");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].kind, SketchInputKind::LineOrCircle);

    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert!(!legacy_compact_profile_line(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Arc
    );

    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload.resize(96 + LEGACY_SKETCH_MARKER.len(), 0);
    payload[72..96].fill(0);
    payload[82..84].copy_from_slice(&2u16.to_le_bytes());
    payload[88..92].copy_from_slice(&3u32.to_le_bytes());
    payload[92..96].copy_from_slice(&1u32.to_le_bytes());
    payload[96..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert!(legacy_compact_profile_line(&payload, 0));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::LineOrCircle
    );

    payload[82..84].copy_from_slice(&10u16.to_le_bytes());
    assert!(legacy_compact_profile_line(&payload, 0));
    payload[82..84].copy_from_slice(&0u16.to_le_bytes());
    assert!(!legacy_compact_profile_line(&payload, 0));
    payload[82..84].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(!legacy_compact_profile_line(&payload, 0));
}


#[test]
fn compact_legacy_bounded_curve_can_use_direct_point_ids() {
    let mut payload = vec![0; 84 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[29..31].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&7u16.to_le_bytes());
    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[84..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let entity = |id: &str, object_index, coordinates_m, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset: 0,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let entities = [
        entity("curve", Some(1), None, SketchInputKind::Arc),
        entity("start", Some(7), Some([-1.0, 0.0]), SketchInputKind::Point),
        entity("end", Some(10), Some([1.0, 0.0]), SketchInputKind::Point),
        entity(
            "one-based-start",
            Some(8),
            Some([0.0, 1.0]),
            SketchInputKind::Point,
        ),
    ];
    let markers = entities.iter().collect::<Vec<_>>();

    let endpoints = legacy_compact_direct_endpoint_markers(&payload, 0, &entities[0], &markers);
    assert_eq!(
        endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["start", "end"]
    );
    assert_eq!(
        roster_curve_endpoint_markers(&payload, &entities[0], &markers)
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<Vec<_>>(),
        ["start", "end"]
    );

    payload.resize(104 + LEGACY_SKETCH_MARKER.len(), 0);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..104].fill(0);
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[96..100].copy_from_slice(&3u32.to_le_bytes());
    payload[100..104].copy_from_slice(&2u32.to_le_bytes());
    payload[104..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let endpoints =
        super::legacy_marker104_arc_endpoints(&payload, &entities[0], &markers).expect("endpoints");
    assert_eq!(
        endpoints.map(|endpoint| endpoint.id.as_str()),
        ["start", "end"]
    );
    assert_eq!(
        super::legacy_marker104_arc_center(&payload, &entities[0], &markers, endpoints,),
        Some([0.0, 1.0])
    );
}


#[test]
fn indexed_profile_framing_distinguishes_vertices_lines_and_arcs() {
    let mut vertex = vec![0; 74];
    vertex[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    vertex[5..13].fill(0xff);
    vertex[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    vertex[17..21].copy_from_slice(&1u32.to_le_bytes());
    vertex[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    vertex[27..29].copy_from_slice(&1u16.to_le_bytes());
    vertex[56..58].copy_from_slice(&[0x1e, 0x00]);
    vertex[58..66].copy_from_slice(&0.025f64.to_le_bytes());
    vertex[66..74].copy_from_slice(&0.01f64.to_le_bytes());
    assert!(indexed_profile_vertex(&vertex, 0));
    assert_eq!(marker_coordinates(&vertex, 0), Some([0.025, 0.01]));
    vertex[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    assert!(indexed_profile_vertex(&vertex, 0));
    assert_eq!(marker_coordinates(&vertex, 0), Some([0.025, 0.01]));
    vertex.resize(112 + LEGACY_EXTENDED_SKETCH_MARKER.len(), 0);
    vertex[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    vertex[17..21].copy_from_slice(&4u32.to_le_bytes());
    vertex[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    vertex[27..31].copy_from_slice(&[1, 0, 1, 0]);
    vertex[64..66].copy_from_slice(&[0x1e, 0x00]);
    vertex[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    vertex[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    vertex[68..72].copy_from_slice(&1u32.to_le_bytes());
    vertex[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    vertex[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    vertex[84..86].copy_from_slice(&1u16.to_le_bytes());
    for offset in (86..102).step_by(4) {
        vertex[offset..offset + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    vertex[112..112 + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert_eq!(marker_coordinates(&vertex, 0), None);

    let mut curve = vec![0; 84 + 39];
    curve[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    curve[5..13].fill(0xff);
    curve[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    curve[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    curve[27..29].copy_from_slice(&1u16.to_le_bytes());
    curve[60..64].copy_from_slice(&1u32.to_le_bytes());
    curve[84..84 + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    curve[89..97].fill(0xff);
    curve[97..101].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    assert_eq!(
        legacy_extended_profile_curve_kind(&curve, 0),
        Some(SketchInputKind::LineOrCircle)
    );
    curve[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        legacy_extended_profile_curve_kind(&curve, 0),
        Some(SketchInputKind::LineOrCircle)
    );
    curve[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    curve[89..93].copy_from_slice(&[0xff, 0xff, 0x04, 0x00]);
    assert_eq!(
        legacy_extended_profile_curve_kind(&curve, 0),
        Some(SketchInputKind::Arc)
    );
}


#[test]
fn terminal_legacy_indexed_curve_retains_its_sibling_line_kind() {
    let detail = 84;
    let mut payload = vec![0; detail * 2];
    for offset in [0, detail] {
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    }
    let entity = |id: &str, offset, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let sibling = entity("sibling", 0, SketchInputKind::LineOrCircle);
    let terminal = entity("terminal", detail as u64, SketchInputKind::Arc);

    assert!(legacy_terminal_indexed_profile_line(
        &payload,
        &terminal,
        &[&sibling, &terminal],
    ));
    assert!(!legacy_terminal_indexed_profile_line(
        &payload,
        &terminal,
        &[&terminal],
    ));
}


#[test]
fn compact_curve_detail_tangent_distinguishes_lines_and_arcs() {
    let detail = 84;
    let mut payload = vec![0; detail + 80];
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[detail..detail + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[detail + 5..detail + 13]
        .copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]);
    payload[detail + 13..detail + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[detail + 23..detail + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[detail + 27..detail + 29].copy_from_slice(&2u16.to_le_bytes());
    payload[detail + 31..detail + 35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[detail + 35..detail + 39].copy_from_slice(&[0x00, 0x00, 0x0c, 0x00]);
    payload[detail + 48..detail + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[detail + 64..detail + 72].copy_from_slice(&(-1.0f64).to_le_bytes());

    assert_eq!(
        compact_bounded_curve_tangent(&payload, 0),
        Some([-1.0, 0.0])
    );
    assert_eq!(
        tangent_bounded_curve(
            Point2::new(0.0, 2.0),
            Point2::new(0.0, 0.0),
            [-1.0, 0.0],
            1.0e-9,
        ),
        Some(SketchGeometry::Arc {
            center: Point2::new(0.0, 1.0),
            radius: Length(1.0),
            start_angle: Angle(std::f64::consts::FRAC_PI_2),
            end_angle: Angle(-std::f64::consts::FRAC_PI_2),
        })
    );
    assert_eq!(
        tangent_bounded_curve(
            Point2::new(0.0, 2.0),
            Point2::new(0.0, 0.0),
            [0.0, -1.0],
            1.0e-9,
        ),
        Some(SketchGeometry::Line {
            start: Point2::new(0.0, 2.0),
            end: Point2::new(0.0, 0.0),
        })
    );
}


#[test]
fn indexed_arcs_use_one_equidistant_center_marker() {
    let mut payload = vec![0; 104 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    for offset in (78..94).step_by(4) {
        payload[offset..offset + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert!(indexed_arc_uses_coordinate_center(&payload, 0));

    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[35..39].copy_from_slice(&[0x00, 0x00, 0x05, 0x00]);
    payload[56..58].copy_from_slice(&8u16.to_le_bytes());
    payload[58..60].copy_from_slice(&10u16.to_le_bytes());
    let entity = |id: String, offset, object_index, coordinates_m| SketchInputEntity {
        id,
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let mut coordinates = (0..11)
        .map(|index| {
            entity(
                format!("point-{index}"),
                u64::from(index),
                Some(100 + index),
                Some([f64::from(index), f64::from(index)]),
            )
        })
        .collect::<Vec<_>>();
    coordinates[4].object_index = Some(7);
    coordinates[4].coordinates_m = Some([0.0, -0.02]);
    coordinates[8].coordinates_m = Some([-0.015, 0.02]);
    coordinates[10].coordinates_m = Some([0.015, 0.02]);
    let mut curve = entity("curve".into(), 0, Some(3), None);
    curve.kind = SketchInputKind::Arc;
    let markers = coordinates
        .iter()
        .chain(std::iter::once(&curve))
        .collect::<Vec<_>>();
    assert_eq!(
        coordinate_roster_arc_center(
            &payload,
            &curve,
            &markers,
            [&coordinates[8], &coordinates[10]],
        ),
        Some([0.0, -0.02])
    );

    let mut compact_84 = vec![0; 84 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    compact_84[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    compact_84[23..29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    compact_84[29..31].copy_from_slice(&1u16.to_le_bytes());
    compact_84[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    compact_84[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    compact_84[56..60].copy_from_slice(&[15, 0, 16, 0]);
    compact_84[60..64].copy_from_slice(&1u32.to_le_bytes());
    compact_84[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    compact_84[72..76].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    compact_84[80..84].copy_from_slice(&8u32.to_le_bytes());
    compact_84[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert!(indexed_arc_uses_coordinate_center(&compact_84, 0));
    compact_84[58..60].copy_from_slice(&15u16.to_le_bytes());
    assert!(!indexed_arc_uses_coordinate_center(&compact_84, 0));

    let mut current = vec![0; 92 + SKETCH_MARKER.len()];
    current[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    current[5..13].fill(0xff);
    current[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    current[17..21].copy_from_slice(&2u32.to_le_bytes());
    current[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    current[27..29].copy_from_slice(&1u16.to_le_bytes());
    current[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    current[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    current[64..66].copy_from_slice(&1u16.to_le_bytes());
    current[66..68].copy_from_slice(&2u16.to_le_bytes());
    current[68..72].copy_from_slice(&1u32.to_le_bytes());
    current[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    current[92..].copy_from_slice(SKETCH_MARKER);
    assert!(indexed_arc_uses_coordinate_center(&current, 0));
    assert!(current_undetailed_bounded_curve_is_line(&current, 0));
    let mut extended = current.clone();
    extended[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert!(indexed_arc_uses_coordinate_center(&extended, 0));
    assert!(current_undetailed_bounded_curve_is_line(&extended, 0));
    extended[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert!(indexed_arc_uses_coordinate_center(&extended, 0));
    assert!(current_undetailed_bounded_curve_is_line(&extended, 0));
    extended[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert!(!current_undetailed_bounded_curve_is_line(&extended, 0));
    let mut current_compact = current[..84].to_vec();
    current_compact[29..31].copy_from_slice(&1u16.to_le_bytes());
    current_compact[56..58].copy_from_slice(&1u16.to_le_bytes());
    current_compact[58..60].copy_from_slice(&2u16.to_le_bytes());
    current_compact[60..64].copy_from_slice(&1u32.to_le_bytes());
    current_compact[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    current_compact[72..84].fill(0);
    current_compact.extend_from_slice(SKETCH_MARKER);
    assert!(indexed_arc_uses_coordinate_center(&current_compact, 0));
    assert!(current_undetailed_bounded_curve_is_line(
        &current_compact,
        0
    ));
    let mut extended_compact = current_compact.clone();
    extended_compact[..LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    extended_compact[84..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    assert!(current_undetailed_bounded_curve_is_line(
        &extended_compact,
        0
    ));
    extended_compact[17..21].copy_from_slice(&0u32.to_le_bytes());
    extended_compact[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert!(current_undetailed_bounded_curve_is_line(
        &extended_compact,
        0
    ));
    current_compact[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert!(!indexed_arc_uses_coordinate_center(&current_compact, 0));
    let mut detailed = current.clone();
    detailed.resize(172, 0);
    detailed[97..105].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]);
    detailed[105..109].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    detailed[115..119].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    detailed[119..121].copy_from_slice(&2u16.to_le_bytes());
    detailed[123..131].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    detailed[140..148].copy_from_slice(&1.0f64.to_le_bytes());
    detailed[156..164].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert!(!current_undetailed_bounded_curve_is_line(&detailed, 0));
    assert!(!current_indexed_arc_reverses_center_sweep(&current, 0));
    current[80..84].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    assert!(current_indexed_arc_reverses_center_sweep(&current, 0));
    current[17..21].copy_from_slice(&1u32.to_le_bytes());
    assert!(!indexed_arc_uses_coordinate_center(&current, 0));

    let start = Point2::new(1.0, 0.0);
    let end = Point2::new(0.0, 1.0);
    assert_eq!(
        unique_arc_center_marker(
            start,
            end,
            &[Point2::new(0.0, 0.0), Point2::new(4.0, 3.0)],
            1.0e-8,
        ),
        Some(Point2::new(0.0, 0.0))
    );
    assert_eq!(
        unique_arc_center_marker(
            start,
            end,
            &[Point2::new(0.0, 0.0), Point2::new(0.5, 0.5)],
            1.0e-8,
        ),
        None
    );
}


#[test]
fn compact_legacy_bounded_arc_uses_its_diameter_center_marker() {
    let mut payload = vec![0; 102];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..60].copy_from_slice(&[4, 0, 0, 0]);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&5u16.to_le_bytes());
    for relative in (78..94).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    let marker = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = marker("arc", 0, SketchInputKind::Arc, None);
    let start = marker("start", 1, SketchInputKind::Point, Some([1.0, 0.0]));
    let center = marker("center", 2, SketchInputKind::Point, Some([0.0, 0.0]));
    let end = marker("end", 3, SketchInputKind::Point, Some([-1.0, 0.0]));
    let off_axis = marker("handle", 4, SketchInputKind::Point, Some([0.0, 2.0]));
    let markers = [&start, &center, &end, &off_axis];

    assert_eq!(
        legacy_compact_diameter_arc_center(&payload, &curve, &markers, [&start, &end]),
        Some([0.0, 0.0])
    );
}


#[test]
fn geometry_locus_role_excludes_display_handles() {
    let mut payload = vec![0; 27];
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert!(marker_is_geometry_locus(&payload, 0));
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert!(!marker_is_geometry_locus(&payload, 0));
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x02, 0x00]);
    assert!(!marker_is_geometry_locus(&payload, 0));
}


#[test]
fn local_links_require_the_reference_trailer() {
    let mut payload = vec![0; 80];
    payload[64..66].copy_from_slice(&37u16.to_le_bytes());
    payload[66..68].copy_from_slice(&39u16.to_le_bytes());
    payload[68..70].copy_from_slice(&1u16.to_le_bytes());
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert_eq!(marker_local_links(&payload, 0), Some(([37, 39], 1)));
    payload[70] = 1;
    assert_eq!(marker_local_links(&payload, 0), None);
    payload[70] = 0;
    payload[72..80].copy_from_slice(&0.0f64.to_le_bytes());
    assert_eq!(marker_local_links(&payload, 0), None);
    payload[5..17].copy_from_slice(&[
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x80, 0xbf,
    ]);
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert_eq!(marker_local_links(&payload, 0), Some(([30, 39], 1)));
}


#[test]
fn coordinate_marker_links_are_sentinel_terminated_reference_cells() {
    let mut payload = vec![0; 118];
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[66..74].copy_from_slice(&1.25f64.to_le_bytes());
    payload[74..82].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[84..86].copy_from_slice(&3u16.to_le_bytes());
    for (index, local_id) in [7u16, 11].into_iter().enumerate() {
        let start = 86 + index * 12;
        payload[start..start + 2].copy_from_slice(&0x8386u16.to_le_bytes());
        payload[start + 2..start + 4].copy_from_slice(&local_id.to_le_bytes());
        payload[start + 4..start + 8].fill(0xff);
    }
    payload[112..116].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    assert_eq!(
        coordinate_marker_local_links(&payload, 0),
        Some((vec![7, 11], 0x8386))
    );
    for start in [86, 98] {
        payload[start..start + 2].copy_from_slice(&0xbc87u16.to_le_bytes());
    }
    assert_eq!(
        coordinate_marker_local_links(&payload, 0),
        Some((vec![7, 11], 0xbc87))
    );
    payload[98] ^= 1;
    assert_eq!(coordinate_marker_local_links(&payload, 0), None);
}


#[test]
fn non_coordinate_legacy_profile_line_carries_counted_endpoint_links() {
    let mut payload = vec![0; 162];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[84..86].copy_from_slice(&2u16.to_le_bytes());
    for (index, local_id) in [2u16, 5].into_iter().enumerate() {
        let start = 86 + index * 12;
        payload[start..start + 2].copy_from_slice(&0x83a9u16.to_le_bytes());
        payload[start + 2..start + 4].copy_from_slice(&local_id.to_le_bytes());
        payload[start + 4..start + 8].fill(0xff);
    }
    payload[112..116].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);

    assert_eq!(
        coordinate_marker_local_links(&payload, 0),
        Some((vec![2, 5], 0x83a9))
    );
}


#[test]
fn coordinate_namespace_disambiguates_reused_local_id() {
    let candidates = vec![("relation".into(), false), ("geometry".into(), true)];
    assert_eq!(unique_marker_candidate(&candidates), Some("geometry"));
    let ambiguous = vec![("first".into(), true), ("second".into(), true)];
    assert_eq!(unique_marker_candidate(&ambiguous), None);
}


#[test]
fn point_operand_requires_one_profile_locus() {
    let entity = SketchEntityId("entity".into());
    let locus = SketchLocus::Start(entity.clone());
    assert_eq!(unique_locus(std::slice::from_ref(&locus)), Some(locus));
    assert_eq!(unique_locus(&[]), None);
    assert_eq!(
        unique_locus(&[SketchLocus::Start(entity.clone()), SketchLocus::End(entity)]),
        None
    );
}


#[test]
fn compact_body_selection_requires_the_complete_trailer() {
    let mut payload = vec![0xaa; 9];
    payload.extend(11000u32.to_le_bytes());
    payload.extend([0; 8]);
    payload.extend(2u32.to_le_bytes());
    payload.extend(287u32.to_le_bytes());
    payload.extend(115u32.to_le_bytes());
    payload.extend(u32::MAX.to_le_bytes());
    payload.extend([0; 12]);
    payload.extend([0x6a, 0xcb]);
    assert_eq!(
        compact_body_selection_vector(&payload, 100, Some(0xcb6a)),
        Some((109, vec![287, 115]))
    );
    assert_eq!(compact_body_selection_at(&payload, 9), Some(vec![287, 115]));
    let mut embedded_false_header = vec![0xaa; 9];
    embedded_false_header.extend(11000u32.to_le_bytes());
    embedded_false_header.extend([0; 8]);
    embedded_false_header.extend(5u32.to_le_bytes());
    for id in [287, 11000, 0, 0, u32::MAX] {
        embedded_false_header.extend(id.to_le_bytes());
    }
    embedded_false_header.extend(u32::MAX.to_le_bytes());
    embedded_false_header.extend([0; 12]);
    assert_eq!(
        compact_body_selection_vector(&embedded_false_header, 100, None),
        Some((109, vec![287, 11000, 0, 0, u32::MAX]))
    );
    let zero_trailer = payload.len() - 3;
    payload[zero_trailer] = 1;
    assert_eq!(
        compact_body_selection_vector(&payload, 100, Some(0xcb6a)),
        None
    );
}


#[test]
fn compact_edge_selection_is_count_delimited_and_signature_typed() {
    let mut payload = Vec::new();
    payload.extend(3u32.to_le_bytes());
    payload.extend([0x00, 0x02, 0x00, 0x00, 0, 0, 0, 0]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    let signature = [
        0x00, 0x81, 0x03, 0x01, 0x2c, 0, 0, 0, 0x63, 0x18, 0x58, 0x69,
    ];
    for (index, edge_id) in [4u32, 0, 5].into_iter().enumerate() {
        payload.extend((0x818bu32 + index as u32).to_le_bytes());
        payload.extend(signature);
        payload.extend(edge_id.to_le_bytes());
        if index == 0 {
            payload.extend([0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
        } else if index == 1 {
            payload.extend([0; 8]);
        }
    }
    assert_eq!(compact_edge_selection_at(&payload, 12), Some(vec![4, 0, 5]));
    payload[12 + 18 + 28 + 4] ^= 1;
    assert_eq!(compact_edge_selection_at(&payload, 12), Some(vec![4, 0, 5]));
}


#[test]
fn compact_edge_selection_accepts_object_terminated_u16_paths() {
    let marker = 12;
    let mut payload = vec![0; marker + 18];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0x0e, 0x02, 0x13, 0x02, 0x13, 0x02, 0x13, 0x02]);
    payload.extend([0; 8]);
    payload.extend([0xe2, 0x80, 0, 0]);

    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![526, 531, 531, 531])
    );

    payload[marker + 18 + 8 + 7] = 1;
    assert_eq!(compact_edge_selection_at(&payload, marker), None);
    payload[marker + 18 + 8 + 7] = 0;
    payload[marker + 18 + 8 + 8] = 0xff;
    payload[marker + 18 + 8 + 9] = 0xff;
    assert_eq!(compact_edge_selection_at(&payload, marker), None);
}


#[test]
fn compact_edge_selection_rejects_unbounded_counts_and_short_headers() {
    let mut payload = vec![0; 40];
    payload[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[12..28].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    assert_eq!(compact_edge_selection_at(&payload, 12), None);
    assert_eq!(compact_edge_component_path_at(&payload, 12), None);

    payload[..16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    assert_eq!(compact_edge_selection_at(&payload, 0), None);
    assert_eq!(compact_edge_component_path_at(&payload, 0), None);
    assert_eq!(compact_surface_selection_at(&payload, 0), None);
}


#[test]
fn solved_tangent_treats_arcs_as_bounded_circles() {
    use cadmpeg_ir::features::{Angle, Length};

    let line = SketchGeometry::Line {
        start: Point2::new(-2.0, 1.0),
        end: Point2::new(2.0, 1.0),
    };
    let arc = SketchGeometry::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length(1.0),
        start_angle: Angle(0.0),
        end_angle: Angle(std::f64::consts::PI),
    };
    let circle = SketchGeometry::Circle {
        center: Point2::new(2.0, 0.0),
        radius: Length(1.0),
    };
    assert_eq!(solved_tangent(&line, &arc), Some(true));
    assert_eq!(solved_tangent(&arc, &circle), Some(true));
}


#[test]
fn tangent_bridge_arc_requires_one_equidistant_radial_intersection() {
    let geometry = super::tangent_bridge_arc_geometry(
        Point2::new(1.0, 0.0),
        Point2::new(0.0, 1.0),
        Point2::new(2.0, 0.0),
        Point2::new(0.0, 2.0),
        1.0e-9,
    );
    assert_eq!(
        geometry,
        Some(SketchGeometry::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length(1.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::FRAC_PI_2),
        })
    );
    assert_eq!(
        super::tangent_bridge_arc_geometry(
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            Point2::new(2.0, 0.0),
            Point2::new(1.0, 2.0),
            1.0e-9,
        ),
        None
    );
}


#[test]
fn unresolved_fillet_requires_matching_endpoint_tangent_circles() {
    let sketch = SketchId("sketch".into());
    let entity = |id: &str, geometry, endpoint_refs: &[&str]| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: endpoint_refs.iter().map(|id| (*id).into()).collect(),
        geometry,
    };
    let mut entities = vec![
        entity(
            "start",
            SketchGeometry::Point {
                position: Point2::new(1.0, 0.0),
            },
            &[],
        ),
        entity(
            "end",
            SketchGeometry::Point {
                position: Point2::new(0.0, 1.0),
            },
            &[],
        ),
        entity(
            "start-line",
            SketchGeometry::Line {
                start: Point2::new(1.0, -1.0),
                end: Point2::new(1.0, 0.0),
            },
            &["start-line-other", "start"],
        ),
        entity(
            "end-line",
            SketchGeometry::Line {
                start: Point2::new(0.0, 1.0),
                end: Point2::new(-1.0, 1.0),
            },
            &["end", "end-line-other"],
        ),
        entity(
            "fillet",
            SketchGeometry::Native {
                native_kind: "sldprt:marker-geometry:2".into(),
            },
            &["start", "end"],
        ),
    ];

    super::resolve_tangent_bridge_marker_arcs(&mut entities, 1.0e-9);

    assert!(matches!(
        entities[4].geometry,
        SketchGeometry::Arc {
            center,
            radius: Length(radius),
            ..
        } if center == Point2::new(0.0, 0.0) && radius == 1.0
    ));
}


#[test]
fn indexed_arc_uses_its_consecutive_middle_point_as_center() {
    let sketch = SketchId("sketch".into());
    let point = |id: &str, offset: u64, position| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(format!("native:{offset}")),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let mut entities = vec![
        point("start", 100, Point2::new(1.0, 0.0)),
        point("center", 200, Point2::new(0.0, 0.0)),
        point("end", 300, Point2::new(0.0, 1.0)),
        cadmpeg_ir::sketches::SketchEntity {
            id: SketchEntityId("arc".into()),
            sketch,
            construction: false,
            native_ref: Some("native:400".into()),
            geometry_ref: None,
            endpoint_refs: vec!["native:100".into(), "native:300".into()],
            geometry: SketchGeometry::Native {
                native_kind: "sldprt:marker-geometry:2".into(),
            },
        },
    ];

    super::resolve_connected_marker_arcs(&mut entities, 1.0e-9);

    assert_eq!(
        entities[3].geometry,
        SketchGeometry::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length(1.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::FRAC_PI_2),
        }
    );
}


#[test]
fn slot_cycle_supplies_the_missing_cap_endpoints_and_center() {
    let slot_offset = 500;
    let mut payload = vec![0; slot_offset + 140];
    let declaration = b"\xff\xff\x01\x00\x08\x00sgSlot_c\0\0\0\0\x01\0\0\0";
    payload[slot_offset - declaration.len()..slot_offset].copy_from_slice(declaration);
    payload[slot_offset..slot_offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[slot_offset + 5..slot_offset + 13].fill(0xff);
    payload[slot_offset + 13..slot_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[slot_offset + 23..slot_offset + 29]
        .copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[slot_offset + 31..slot_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[slot_offset + 48..slot_offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    for (index, (tag, id)) in [
        (0x8156_u16, 0_u16),
        (0x814c, 3),
        (0x8156, 1),
        (0x8156, 2),
        (0x8294, 0),
        (0x8294, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let start = slot_offset + 64 + index * 12;
        payload[start..start + 2].copy_from_slice(&tag.to_le_bytes());
        payload[start + 2..start + 4].copy_from_slice(&id.to_le_bytes());
        payload[start + 4..start + 8].fill(0xff);
    }

    let input = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let inputs = [
        input("center-left", 100, SketchInputKind::Point, Some([0.0, 0.0])),
        input(
            "center-right",
            110,
            SketchInputKind::Point,
            Some([2.0, 0.0]),
        ),
        input("left-top", 120, SketchInputKind::Point, Some([0.0, 1.0])),
        input("right-top", 130, SketchInputKind::Point, Some([2.0, 1.0])),
        input(
            "left-bottom",
            140,
            SketchInputKind::Point,
            Some([0.0, -1.0]),
        ),
        input(
            "right-bottom",
            150,
            SketchInputKind::Point,
            Some([2.0, -1.0]),
        ),
        input("top", 200, SketchInputKind::LineOrCircle, None),
        input("bottom", 210, SketchInputKind::LineOrCircle, None),
        input("right", 220, SketchInputKind::Arc, None),
        input("left", 230, SketchInputKind::Arc, None),
        input("slot", slot_offset as u64, SketchInputKind::Point, None),
    ];
    let markers = inputs.iter().collect::<Vec<_>>();
    let sketch = SketchId("sketch".into());
    let point = |id: &str, position| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(format!("model:{id}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point { position },
    };
    let curve = |id: &str, geometry, endpoint_refs: &[&str]| cadmpeg_ir::sketches::SketchEntity {
        id: SketchEntityId(format!("model:{id}")),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: endpoint_refs.iter().map(|id| (*id).into()).collect(),
        geometry,
    };
    let mut entities = vec![
        point("center-left", Point2::new(0.0, 0.0)),
        point("center-right", Point2::new(2.0, 0.0)),
        point("left-top", Point2::new(0.0, 1.0)),
        point("right-top", Point2::new(2.0, 1.0)),
        point("left-bottom", Point2::new(0.0, -1.0)),
        point("right-bottom", Point2::new(2.0, -1.0)),
        curve(
            "top",
            SketchGeometry::Line {
                start: Point2::new(0.0, 1.0),
                end: Point2::new(2.0, 1.0),
            },
            &["left-top", "right-top"],
        ),
        curve(
            "bottom",
            SketchGeometry::Line {
                start: Point2::new(0.0, -1.0),
                end: Point2::new(2.0, -1.0),
            },
            &["left-bottom", "right-bottom"],
        ),
        curve(
            "right",
            SketchGeometry::Arc {
                center: Point2::new(2.0, 0.0),
                radius: Length(1.0),
                start_angle: Angle(std::f64::consts::FRAC_PI_2),
                end_angle: Angle(-std::f64::consts::FRAC_PI_2),
            },
            &["right-top", "right-bottom"],
        ),
        curve(
            "left",
            SketchGeometry::Native {
                native_kind: "sldprt:marker-geometry:2".into(),
            },
            &[],
        ),
    ];

    super::resolve_slot_marker_arcs(&payload, &markers, &mut entities, 1.0e-9);

    assert_eq!(
        entities[9].endpoint_refs,
        ["left-top".to_string(), "left-bottom".to_string()]
    );
    assert!(matches!(
        entities[9].geometry,
        SketchGeometry::Arc {
            center,
            radius: Length(radius),
            ..
        } if center == Point2::new(0.0, 0.0) && radius == 1.0
    ));
}


#[test]
fn packed_slot_descriptor_run_is_not_independent_geometry() {
    let slot_offset = 22;
    let mut payload = vec![0; slot_offset + 252];
    let declaration = b"\xff\xff\x01\x00\x08\x00sgSlot_c\0\0\0\0\x01\0\0\0";
    payload[..slot_offset].copy_from_slice(declaration);
    payload[slot_offset..slot_offset + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[slot_offset + 5..slot_offset + 13].fill(0xff);
    payload[slot_offset + 13..slot_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[slot_offset + 17..slot_offset + 21].copy_from_slice(&0_u32.to_le_bytes());
    payload[slot_offset + 23..slot_offset + 29]
        .copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[slot_offset + 31..slot_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[slot_offset + 48..slot_offset + 56].copy_from_slice(&1.0_f64.to_le_bytes());
    for (index, (tag, id)) in [
        (0x8156_u16, 0_u16),
        (0x814c, 3),
        (0x8156, 1),
        (0x8156, 2),
        (0x8294, 0),
        (0x8294, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let start = slot_offset + 64 + index * 8;
        payload[start..start + 2].copy_from_slice(&tag.to_le_bytes());
        payload[start + 2..start + 4].copy_from_slice(&id.to_le_bytes());
        payload[start + 4..start + 8].fill(0xff);
    }
    payload.copy_within(slot_offset..slot_offset + 126, slot_offset + 126);

    assert_eq!(
        super::slot_curve_and_center_indices(&payload, slot_offset),
        Some(([0, 3, 1, 2], [0, 1]))
    );
    assert_eq!(
        super::slot_curve_and_center_indices(&payload, slot_offset + 126),
        Some(([0, 3, 1, 2], [0, 1]))
    );

    let entities = sketch_input_entities(&payload, "lane");

    assert_eq!(entities.len(), 2);
    assert!(entities
        .iter()
        .all(|entity| entity.kind == SketchInputKind::Native(0)));
}


#[test]
fn every_principal_plane_has_a_sketch_frame() {
    use cadmpeg_ir::features::PrincipalPlane;

    for plane in [
        PrincipalPlane::Front,
        PrincipalPlane::Top,
        PrincipalPlane::Right,
    ] {
        let (_, normal, u_axis) = principal_sketch_frame(plane);
        assert!((super::dot(normal, normal) - 1.0).abs() <= 1.0e-12);
        assert!((super::dot(u_axis, u_axis) - 1.0).abs() <= 1.0e-12);
        assert!(super::dot(normal, u_axis).abs() <= 1.0e-12);
    }
}


#[test]
fn offset_plane_frame_translates_its_reference_frame() {
    use cadmpeg_ir::features::{
        Feature as NeutralFeature, FeatureDefinition, FeatureId, Length, PrincipalPlane,
    };

    let native = |id: &str, source: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source.into()),
        parent_source_id: None,
        ordinal: source.parse().expect("required invariant"),
        name: id.into(),
        kind: String::new(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let neutral = |id: &str, native_ref: &str, definition| NeutralFeature {
        id: FeatureId(id.into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: Some(native_ref.into()),
    };
    let features = vec![
        neutral(
            "plane",
            "plane-native",
            FeatureDefinition::DatumPrincipalPlane {
                plane: PrincipalPlane::Top,
            },
        ),
        neutral(
            "offset",
            "offset-native",
            FeatureDefinition::DatumOffsetPlane {
                reference: Some(cadmpeg_ir::features::DatumPlaneReference::Feature(
                    FeatureId("plane".into()),
                )),
                distance: Length(3.0),
            },
        ),
    ];
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![native("plane-native", "3"), native("offset-native", "549")],
    };

    assert_eq!(
        sketch_plane_frames(&features, &[history]).get(&549),
        Some(&(
            Point3::new(0.0, 0.0, 3.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        ))
    );
}


#[test]
fn compact_edge_selection_accepts_heterogeneous_component_paths() {
    let marker = 12;
    let mut payload = vec![0; 120];
    payload[..4].copy_from_slice(&2u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[8..12].copy_from_slice(&37u32.to_le_bytes());
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let first = marker + 18;
    payload[first..first + 4].copy_from_slice(&[0x3d, 0x80, 0, 0]);
    payload[first + 4..first + 16].copy_from_slice(&[1; 12]);
    payload[first + 16..first + 20].copy_from_slice(&2u32.to_le_bytes());
    let second = first + 28;
    payload[second..second + 4].copy_from_slice(&[0x4a, 0x80, 0, 0]);
    payload[second + 4..second + 16].copy_from_slice(&[2; 12]);
    payload[second + 16..second + 20].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![2, 3])
    );
    assert_eq!(
        compact_edge_component_path_at(&payload, marker),
        Some(vec![
            FeatureInputComponentPathEntry {
                instance: Some(0x803d),
                type_signature: [1; 12],
                local_id: Some(2),
            },
            FeatureInputComponentPathEntry {
                instance: Some(0x804a),
                type_signature: [2; 12],
                local_id: Some(3),
            },
        ])
    );

    payload[..4].copy_from_slice(&3u32.to_le_bytes());
    let third = second + 24;
    payload[second + 20..third].fill(0xff);
    payload[third..third + 4].copy_from_slice(&[0x53, 0x80, 0, 0]);
    payload[third + 4..third + 16].copy_from_slice(&[3; 12]);
    payload[third + 16..third + 20].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![2, 3, 4])
    );
}


#[test]
fn compact_edge_selection_accepts_root_and_zero_run_separators() {
    let marker = 12;
    let mut payload = vec![0; 180];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let signature = [0xff, 0x80, 1, 1, 20, 0, 0, 0, 1, 0x42, 0x3e, 0x4f];
    let entry = |payload: &mut [u8], offset: usize, instance: u16, local_id: u32| {
        payload[offset..offset + 2].copy_from_slice(&instance.to_le_bytes());
        payload[offset + 4..offset + 16].copy_from_slice(&signature);
        payload[offset + 16..offset + 20].copy_from_slice(&local_id.to_le_bytes());
    };
    let first = marker + 18;
    entry(&mut payload, first, 0x862a, 1);
    let second = first + 20 + 12;
    entry(&mut payload, second, 0x8631, 10);
    payload[second + 20..second + 28].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 1, 0, 0, 0]);
    let third = second + 28;
    entry(&mut payload, third, 0x8102, 1);
    payload[third + 20..third + 24].copy_from_slice(&[0xa3, 0x86, 1, 0]);
    let fourth = third + 24;
    entry(&mut payload, fourth, 0x8102, 0);

    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![1, 10, 1, 0])
    );
}


#[test]
fn compact_edge_selection_accepts_wide_component_entries() {
    let marker = 12;
    let mut payload = vec![0; 160];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let signature = [0x2a, 0x81, 0x2c, 1, 28, 0, 0, 0, 0x24, 1, 0xd3, 0x48];
    let entry = |payload: &mut [u8], offset: usize, instance: u16, local_id: u32| {
        payload[offset..offset + 2].copy_from_slice(&instance.to_le_bytes());
        payload[offset + 4..offset + 16].copy_from_slice(&signature);
        payload[offset + 20..offset + 24].copy_from_slice(&local_id.to_le_bytes());
    };
    let first = marker + 18;
    entry(&mut payload, first, 0x8130, 0);
    let second = first + 24;
    entry(&mut payload, second, 0x8130, 2);
    let third = second + 24;
    entry(&mut payload, third, 0x8141, 1);
    let fourth = third + 28;
    entry(&mut payload, fourth, 0x8141, 0);

    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![0, 2, 1, 0])
    );
    let path = compact_edge_component_path_at(&payload, marker).unwrap();
    assert_eq!(
        path.iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(0), Some(2), Some(1), Some(0)]
    );
    assert!(path
        .iter()
        .all(|component| component.type_signature == signature));
}


#[test]
fn compact_edge_selection_accepts_ordinal_and_zero_separator() {
    let marker = 12;
    let mut payload = vec![0; 160];
    payload[..4].copy_from_slice(&3u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let signature = [0x35, 0x80, 0x38, 0, 13, 1, 0, 0, 0x8a, 0xd8, 0x3f, 0x58];
    let entry = |payload: &mut [u8], offset: usize, instance: u16, local_id: u32| {
        payload[offset..offset + 2].copy_from_slice(&instance.to_le_bytes());
        payload[offset + 4..offset + 16].copy_from_slice(&signature);
        payload[offset + 16..offset + 20].copy_from_slice(&local_id.to_le_bytes());
    };
    let first = marker + 18;
    entry(&mut payload, first, 0x803e, 1);
    payload[first + 20..first + 24].copy_from_slice(&3u32.to_le_bytes());
    let second = first + 28;
    entry(&mut payload, second, 0x8385, 12);
    payload[second + 20..second + 24].copy_from_slice(&[0xff; 4]);
    let third = second + 28;
    entry(&mut payload, third, 0x8385, 12);

    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![1, 12, 12])
    );
}


#[test]
fn compact_edge_selection_accepts_zero_and_state_separator() {
    let marker = 12;
    let mut payload = vec![0; 128];
    payload[..4].copy_from_slice(&2u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[8..12].copy_from_slice(&375_491u32.to_le_bytes());
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let signature = [0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x3e, 0x77, 0x0e, 0x60];
    let entry = |payload: &mut [u8], offset: usize, local_id: u32| {
        payload[offset..offset + 2].copy_from_slice(&0x8158u16.to_le_bytes());
        payload[offset + 4..offset + 16].copy_from_slice(&signature);
        payload[offset + 16..offset + 20].copy_from_slice(&local_id.to_le_bytes());
    };
    let first = marker + 18;
    entry(&mut payload, first, 3);
    payload[first + 24..first + 28].copy_from_slice(&1u32.to_le_bytes());
    let second = first + 28;
    entry(&mut payload, second, 2);

    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![3, 2])
    );
}


#[test]
fn compact_edge_selection_preserves_an_idless_path_entry() {
    let marker = 12;
    let mut payload = vec![0; 160];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[8..12].copy_from_slice(&2_366_854u32.to_le_bytes());
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let entry =
        |payload: &mut [u8], offset: usize, instance: u16, source: u32, local_id: Option<u32>| {
            payload[offset..offset + 2].copy_from_slice(&instance.to_le_bytes());
            payload[offset + 4..offset + 8].copy_from_slice(&[0xe8, 0x80, 0xea, 0]);
            payload[offset + 8..offset + 12].copy_from_slice(&source.to_le_bytes());
            payload[offset + 12..offset + 16].copy_from_slice(&[0x1e, 0x0a, 0xca, 0x5a]);
            if let Some(local_id) = local_id {
                payload[offset + 16..offset + 20].copy_from_slice(&local_id.to_le_bytes());
            }
        };
    let first = marker + 18;
    entry(&mut payload, first, 0x80eb, 130, Some(4));
    let second = first + 20;
    entry(&mut payload, second, 0x86e9, 172, None);
    let third = second + 16;
    entry(&mut payload, third, 0x80ee, 152, Some(4));
    payload[third + 20..third + 24].copy_from_slice(&[0xff; 4]);
    let fourth = third + 28;
    entry(&mut payload, fourth, 0x80f8, 130, Some(0));

    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![4, 4, 0])
    );
    let components = compact_edge_component_path_at(&payload, marker).unwrap();
    assert_eq!(
        components
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(4), None, Some(4), Some(0)]
    );
    let selection = FeatureInputEdgeSelection {
        id: "selection".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: marker as u64,
        object_name_ref: "name".into(),
        feature_ref: "consumer".into(),
        local_edge_ids: vec![4, 4, 0],
        components,
        producer_feature_refs: vec!["producer".into()],
        terminal_feature_ref: Some("producer".into()),
    };
    assert_eq!(compact_edge_path_value(&selection), "4,_,4,0");
    assert_eq!(
        compact_edge_selection_set_value(&[&selection]),
        "sldprt:feature-input:edge-ids:4,_,4,0"
    );
}


#[test]
fn compact_edge_selection_marker_does_not_require_a_class_declaration() {
    let native_feature =
        |id: &str, name: &str, source_id: Option<u32>, ordinal: u32, input_class: &str| Feature {
            id: id.into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: source_id.map(|source_id| source_id.to_string()),
            parent_source_id: None,
            ordinal,
            name: name.into(),
            kind: "Feature".into(),
            input_class: Some(input_class.into()),
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native_feature("producer", "Producer", None, 0, "moExtrusion_c"),
            native_feature("consumer", "Consumer", Some(2), 1, "Chamfer_c"),
        ],
    };
    let marker = 52;
    let mut payload = vec![0; 96];
    payload[marker - 12..marker - 8].copy_from_slice(&1u32.to_le_bytes());
    payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let entry = marker + 18;
    payload[entry..entry + 2].copy_from_slice(&0x8130u16.to_le_bytes());
    payload[entry + 4..entry + 8].copy_from_slice(&[0x2a, 0x81, 0x2c, 1]);
    payload[entry + 8..entry + 12].copy_from_slice(&1u32.to_le_bytes());
    payload[entry + 12..entry + 16].copy_from_slice(&[0x24, 1, 0xd3, 0x48]);
    payload[entry + 16..entry + 20].copy_from_slice(&7u32.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "producer-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: Some(1),
                value: "Producer".into(),
            },
            FeatureInputName {
                id: "consumer-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 24,
                object_id: Some(2),
                value: "Consumer".into(),
            },
        ],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    let selections = compact_edge_selections(&[history], &lane);

    assert_eq!(selections.len(), 1);
    assert_eq!(selections[0].feature_ref, "consumer");
    assert_eq!(selections[0].local_edge_ids, [7]);
    assert_eq!(
        selections[0].terminal_feature_ref.as_deref(),
        Some("producer")
    );
}


#[test]
fn compact_edge_selection_excludes_terminal_feature_reference_cell() {
    let marker = 12;
    let mut payload = vec![0; 160];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let signature = [0x34, 0x80, 0x37, 0, 121, 0, 0, 0, 0x9b, 0x95, 0x90, 0x5f];
    let mut cursor = marker + 18;
    for (index, local_id) in [32u32, 34, 1].into_iter().enumerate() {
        payload[cursor..cursor + 4].copy_from_slice(&[0x3d, 0x80, 0, 0]);
        payload[cursor + 4..cursor + 16].copy_from_slice(&signature);
        payload[cursor + 16..cursor + 20].copy_from_slice(&local_id.to_le_bytes());
        cursor += 20;
        if index != 2 {
            payload[cursor..cursor + 8].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
            cursor += 8;
        }
    }
    payload[cursor..cursor + 36].copy_from_slice(&[
        1, 0, 0, 0, 0, 0, 0, 0, 0x4a, 0x80, 0, 0, 0x34, 0x80, 0x37, 0, 35, 0, 0, 0, 0x89, 0x6b,
        0x90, 0x5f, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![32, 34, 1])
    );
    assert_eq!(
        compact_edge_component_path_at(&payload, marker).map(|components| components.len()),
        Some(3)
    );
}


#[test]
fn compact_body_path_requires_type_three_vector() {
    let marker = 12;
    let mut payload = vec![0; 100];
    payload[..4].copy_from_slice(&2u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 3, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let first = marker + 18;
    payload[first..first + 4].copy_from_slice(&[0x32, 0x80, 0, 0]);
    payload[first + 4..first + 16].copy_from_slice(&[1; 12]);
    payload[first + 16..first + 20].copy_from_slice(&6u32.to_le_bytes());
    let second = first + 28;
    payload[second..second + 4].copy_from_slice(&[0x3b, 0x80, 0, 0]);
    payload[second + 4..second + 16].copy_from_slice(&[2; 12]);
    payload[second + 16..second + 20].copy_from_slice(&7u32.to_le_bytes());
    assert_eq!(compact_body_path_at(&payload, marker), Some(vec![6, 7]));
    assert_eq!(
        compact_body_component_path_at(&payload, marker).map(|components| components.len()),
        Some(2)
    );

    payload[..4].copy_from_slice(&3u32.to_le_bytes());
    payload[second + 20..second + 28].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    assert_eq!(compact_body_path_at(&payload, marker), Some(vec![6, 7]));

    payload[4] = 2;
    assert_eq!(compact_body_path_at(&payload, marker), None);
}


#[test]
fn compact_combine_operation_is_name_length_relative() {
    let offset = 7;
    let mut payload = vec![0; 180];
    payload[offset..offset + 5].copy_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff]);
    payload[offset + 5] = 8;
    let operation = offset + 117 + 16;
    payload[operation..operation + 4].copy_from_slice(&2u32.to_le_bytes());
    payload[operation + 10..operation + 14].copy_from_slice(&[0xff; 4]);
    assert_eq!(
        compact_combine_operation_at(&payload, offset),
        Some("Intersect")
    );
    payload[operation - 1] = 1;
    assert_eq!(compact_combine_operation_at(&payload, offset), None);
}


#[test]
fn compact_edge_selection_accepts_counted_u16_ids() {
    let marker = 12;
    let mut payload = vec![0; 80];
    payload[..4].copy_from_slice(&3u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let ids = marker + 18;
    payload[ids..ids + 6].copy_from_slice(&[4, 0, 8, 0, 12, 0]);
    payload[ids + 22..ids + 25].copy_from_slice(&[0xff, 0xfe, 0xff]);
    assert_eq!(
        compact_edge_selection_at(&payload, marker),
        Some(vec![4, 8, 12])
    );
    assert_eq!(compact_edge_component_path_at(&payload, marker), None);
}


#[test]
fn native_scalar_must_match_an_existing_discrete_parameter() {
    let feature = crate::records::Feature {
        id: "feature".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 0,
        name: "Pattern".into(),
        kind: "Pattern".into(),
        input_class: Some("moLPattern_c".into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    assert!(native_scalar_matches_discrete_parameter(
        &feature, "D1", "15", 15.0
    ));
    assert!(!native_scalar_matches_discrete_parameter(
        &feature,
        "D1",
        "15",
        8.371_160_993_642_741e298
    ));
}


#[test]
fn compact_surface_selection_ends_with_its_entry_signature() {
    let mut payload = Vec::new();
    payload.extend(6u32.to_le_bytes());
    payload.extend([0x04, 0x02, 0, 0]);
    payload.extend(0x1234u32.to_le_bytes());
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    let signature = [0x34, 0x80, 0x37, 0, 0x89, 0, 0, 0, 0xe2, 0x56, 0xdf, 0x5e];
    for (index, id) in [2u32, 1, 11].into_iter().enumerate() {
        payload.extend((0x8c20u32 + index as u32).to_le_bytes());
        payload.extend(signature);
        payload.extend(id.to_le_bytes());
        if index == 0 {
            payload.extend(1u32.to_le_bytes());
        }
    }
    payload.extend([0; 24]);
    let components = compact_surface_selection_at(&payload, 12).expect("required invariant");
    assert_eq!(
        components
            .iter()
            .map(|component| (
                component.instance,
                component.type_signature,
                component.local_id
            ))
            .collect::<Vec<_>>(),
        vec![
            (Some(0x8c20), signature, Some(2)),
            (Some(0x8c21), signature, Some(1)),
            (Some(0x8c22), signature, Some(11))
        ]
    );
    payload[12 + 18 + 24 + 4] ^= 1;
    assert_eq!(
        compact_surface_selection_at(&payload, 12)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        vec![Some(2)]
    );
}


#[test]
fn cosmetic_thread_cylinder_reference_uses_the_typed_child_layout() {
    let body_offset = 30;
    let marker = body_offset + 94;
    let mut payload = vec![0; marker - 12];
    payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    let actual_marker = selection_vector_tail(&mut payload, &[3]);
    assert_eq!(actual_marker, marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&payload, body_offset).expect("required invariant");
    assert_eq!(actual_marker, marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(3)
    );

    let compact_marker = body_offset + 66;
    let mut compact = vec![0; compact_marker - 12];
    compact[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    compact[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    compact[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut compact, &[5]), compact_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&compact, body_offset).expect("required invariant");
    assert_eq!(actual_marker, compact_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(5)
    );

    let selected_marker = body_offset + 70;
    let mut selected = vec![0; selected_marker - 12];
    selected[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    selected[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    selected[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    selected[body_offset + 8] = 0x40;
    assert_eq!(selection_vector_tail(&mut selected, &[7]), selected_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&selected, body_offset).expect("required invariant");
    assert_eq!(actual_marker, selected_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(7)
    );

    let extended_marker = body_offset + 106;
    let mut extended = vec![0; extended_marker - 12];
    extended[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    extended[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    extended[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut extended, &[9]), extended_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&extended, body_offset).expect("required invariant");
    assert_eq!(actual_marker, extended_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(9)
    );

    let compact_legacy_marker = body_offset + 46;
    let mut compact_legacy = vec![0; compact_legacy_marker - 12];
    compact_legacy[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    compact_legacy[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    compact_legacy[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        selection_vector_tail(&mut compact_legacy, &[10]),
        compact_legacy_marker
    );
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&compact_legacy, body_offset)
            .expect("required invariant");
    assert_eq!(actual_marker, compact_legacy_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(10)
    );

    let legacy_marker = body_offset + 102;
    let mut legacy = vec![0; legacy_marker - 12];
    legacy[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    legacy[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    legacy[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut legacy, &[11]), legacy_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&legacy, body_offset).expect("required invariant");
    assert_eq!(actual_marker, legacy_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(11)
    );

    let extended_marker = body_offset + 110;
    let mut extended = vec![0; extended_marker - 12];
    extended[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    extended[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    extended[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut extended, &[12]), extended_marker);
    let (actual_marker, components) =
        cosmetic_thread_cylinder_reference_at(&extended, body_offset).expect("required invariant");
    assert_eq!(actual_marker, extended_marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(12)
    );

    for (relative, local_id) in [(62, 13), (90, 14)] {
        let marker = body_offset + relative;
        let mut payload = vec![0; marker - 12];
        payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
        payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
        payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(selection_vector_tail(&mut payload, &[local_id]), marker);
        let (actual_marker, components) =
            cosmetic_thread_cylinder_reference_at(&payload, body_offset)
                .expect("required invariant");
        assert_eq!(actual_marker, marker);
        assert_eq!(
            components.last().expect("required invariant").local_id,
            Some(local_id)
        );
    }

    assert_eq!(
        cosmetic_thread_cylinder_reference_at(&payload, body_offset + 1),
        None
    );

    let mut payload = vec![0; marker - 12];
    payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    payload.extend(3u32.to_le_bytes());
    payload.extend([0, 2, 0, 0]);
    payload.extend([0; 4]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    for (instance, signature, local_id, gap) in [
        (0x8032_u16, [1; 12], 3_u32, Some(6_u32)),
        (0x803e, [2; 12], 7, None),
    ] {
        payload.extend(instance.to_le_bytes());
        payload.extend([0; 2]);
        payload.extend(signature);
        payload.extend(local_id.to_le_bytes());
        if let Some(gap) = gap {
            payload.extend(gap.to_le_bytes());
        }
    }
    let (_, components) =
        cosmetic_thread_cylinder_reference_at(&payload, body_offset).expect("required invariant");
    assert_eq!(
        components
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(3), Some(7)]
    );
}


#[test]
fn cosmetic_thread_retains_unique_cylinder_marker_without_component_path() {
    let body_offset = 30;
    let marker = body_offset + 94;
    let mut payload = vec![0; marker - 12];
    payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802b_u16.to_le_bytes());
    payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut payload, &[3]), marker);
    payload.truncate(marker + 18);
    let feature = Feature {
        id: "thread".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some("20".into()),
        parent_source_id: None,
        ordinal: 0,
        name: "thread".into(),
        kind: "Feature".into(),
        input_class: Some("moCosmeticThread_c".into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    assert_eq!(
        cosmetic_thread_cylinder_marker_reference(
            &feature,
            &lane,
            0,
            lane.native_payload.len(),
            &HashSet::from([0x802f]),
        ),
        vec![(marker, None)]
    );
}


#[test]
fn cosmetic_thread_cylinder_reference_follows_its_owned_diameter_child() {
    let body_offset = 220;
    let marker = body_offset + 94;
    let mut payload = vec![0; marker - 12];
    payload[body_offset..body_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    payload[body_offset + 2..body_offset + 4].copy_from_slice(&0x802d_u16.to_le_bytes());
    payload[body_offset + 4..body_offset + 8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(selection_vector_tail(&mut payload, &[3]), marker);
    payload.resize(500, 0);

    let feature = Feature {
        id: "thread".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some("53".into()),
        parent_source_id: None,
        ordinal: 0,
        name: "Thread".into(),
        kind: "Feature".into(),
        input_class: Some("moCosmeticThread_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([("D2".into(), "<MOD-DIAM>8".into())]),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let diameter = FeatureInputScalar {
        id: "diameter".into(),
        parent: "lane".into(),
        feature_ref: Some("other-feature".into()),
        ordinal: 0,
        offset: 150,
        object_id: 52,
        name: "diameter-name".into(),
        value: 0.008,
        role: FeatureInputScalarRole::Native,
        entity_indices: Vec::new(),
        operands: Vec::new(),
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "diameter-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 120,
                object_id: Some(u32::MAX),
                value: "D2".into(),
            },
            FeatureInputName {
                id: "next-feature".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 400,
                object_id: Some(54),
                value: "Next".into(),
            },
        ],
        scalars: vec![diameter],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    assert_eq!(
        cosmetic_thread_diameter_child_tail(&feature, &lane),
        Some(158..400)
    );
    let references =
        cosmetic_thread_cylinder_references(&feature, &lane, 20, 100, &HashSet::from([0x802f]));
    assert_eq!(
        references
            .iter()
            .map(|(offset, components)| (*offset, components[0].local_id))
            .collect::<Vec<_>>(),
        [(marker, Some(3))]
    );

    lane.scalars.push(FeatureInputScalar {
        id: "next-scalar".into(),
        parent: "lane".into(),
        feature_ref: None,
        ordinal: 1,
        offset: 200,
        object_id: 54,
        name: "next-feature".into(),
        value: 1.0,
        role: FeatureInputScalarRole::Native,
        entity_indices: Vec::new(),
        operands: Vec::new(),
    });
    assert!(cosmetic_thread_cylinder_references(
        &feature,
        &lane,
        20,
        100,
        &HashSet::from([0x802f]),
    )
    .is_empty());
}


#[test]
fn cosmetic_thread_radius_requires_one_topological_cylinder_face() {
    let surface = Surface {
        id: SurfaceId("cylinder".into()),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 4.0,
        },
        source_object: None,
    };
    let face = Face {
        id: FaceId("face".into()),
        shell: ShellId("shell".into()),
        surface: surface.id.clone(),
        sense: Sense::Forward,
        loops: Vec::new(),
        name: None,
        color: None,
        tolerance: None,
    };
    assert_eq!(
        unique_cylindrical_face(
            4.0,
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface)
        ),
        Some(face.id.clone())
    );
    assert_eq!(
        unique_cylindrical_face(
            3.0,
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface)
        ),
        None
    );
    assert_eq!(
        unique_topological_cylindrical_face(
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface)
        ),
        Some(face.id.clone())
    );
    let mut duplicate = face.clone();
    duplicate.id = FaceId("other-face".into());
    assert_eq!(
        unique_cylindrical_face(
            4.0,
            &[face.clone(), duplicate.clone()],
            std::slice::from_ref(&surface),
        ),
        None
    );
    assert_eq!(
        unique_topological_cylindrical_face(&[face, duplicate], &[surface]),
        None
    );
}


#[test]
fn frame_only_plane_support_requires_one_coincident_face() {
    let surface = Surface {
        id: SurfaceId("plane".into()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 5.0),
            normal: Vector3::new(0.0, 0.0, -1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    let face = Face {
        id: FaceId("face".into()),
        shell: ShellId("shell".into()),
        surface: surface.id.clone(),
        sense: Sense::Forward,
        loops: Vec::new(),
        name: None,
        color: None,
        tolerance: None,
    };

    assert_eq!(
        unique_planar_face(
            Point3::new(4.0, -2.0, 5.0),
            Vector3::new(0.0, 0.0, 1.0),
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface),
        ),
        Some(face.id.clone())
    );
    assert_eq!(
        unique_planar_face(
            Point3::new(0.0, 0.0, 6.0),
            Vector3::new(0.0, 0.0, 1.0),
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface),
        ),
        None
    );
    let mut duplicate = face.clone();
    duplicate.id = FaceId("other-face".into());
    assert_eq!(
        unique_planar_face(
            Point3::new(0.0, 0.0, 5.0),
            Vector3::new(0.0, 0.0, 1.0),
            &[face, duplicate],
            &[surface],
        ),
        None
    );
}


#[test]
fn cosmetic_thread_uses_consensus_persistent_face_path_before_radius() {
    let native_feature = |id: &str, source_id: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source_id.into()),
        parent_source_id: None,
        ordinal: 0,
        name: id.into(),
        kind: "Feature".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native_feature("producer-native", "10"),
            native_feature("thread-native", "20"),
        ],
    };
    let neutral_feature = |id: &str, native_ref: &str, definition| cadmpeg_ir::features::Feature {
        id: FeatureId(id.into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: Some(native_ref.into()),
    };
    let mut features = vec![
        neutral_feature(
            "producer",
            "producer-native",
            cadmpeg_ir::features::FeatureDefinition::BaseFeature {
                bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            },
        ),
        neutral_feature(
            "thread",
            "thread-native",
            cadmpeg_ir::features::FeatureDefinition::CosmeticThread {
                face: cadmpeg_ir::features::FaceSelection::Unresolved,
                diameter: None,
                extent: None,
            },
        ),
    ];
    let mut signature = [0; 12];
    signature[4..8].copy_from_slice(&10_u32.to_le_bytes());
    let selection = |parent: &str, offset| FeatureInputSurfaceSelection {
        id: format!("selection-{parent}"),
        parent: parent.into(),
        ordinal: 0,
        offset,
        object_name_ref: "name".into(),
        feature_ref: "thread-native".into(),
        producer_feature_refs: vec!["producer-native".into()],
        terminal_feature_ref: Some("producer-native".into()),
        components: vec![
            FeatureInputComponentPathEntry {
                instance: Some(0x8020),
                type_signature: signature,
                local_id: Some(7),
            },
            FeatureInputComponentPathEntry {
                instance: Some(0x8021),
                type_signature: signature,
                local_id: Some(u32::try_from(offset / 20).expect("test offset fits u32")),
            },
        ],
    };
    let lane = |id: &str, offset| FeatureInputLane {
        id: id.into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: vec![selection(id, offset)],
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    project_unbound_cosmetic_thread_faces(
        &mut features,
        std::slice::from_ref(&history),
        &[lane("lane-a", 40), lane("lane-b", 60)],
        &[],
        &[],
    );

    let cadmpeg_ir::features::FeatureDefinition::CosmeticThread { face, .. } =
        &features[1].definition
    else {
        panic!("expected cosmetic thread");
    };
    assert!(matches!(
        face,
        cadmpeg_ir::features::FaceSelection::Generated { faces, native }
            if faces.as_slice() == [cadmpeg_ir::features::GeneratedFaceRef {
                feature: FeatureId("producer".into()),
                local_id: "7".into(),
            }]
                && native == "sldprt:feature-input:cylinder-reference:lane-a:40,lane-b:60"
    ));
    assert_eq!(features[1].dependencies, [FeatureId("producer".into())]);

    let surface = Surface {
        id: SurfaceId("cylinder".into()),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 4.0,
        },
        source_object: None,
    };
    let topology_face = Face {
        id: FaceId("cylinder-face".into()),
        shell: ShellId("shell".into()),
        surface: surface.id.clone(),
        sense: Sense::Forward,
        loops: Vec::new(),
        name: None,
        color: None,
        tolerance: None,
    };
    let cadmpeg_ir::features::FeatureDefinition::CosmeticThread { face, diameter, .. } =
        &mut features[1].definition
    else {
        panic!("expected cosmetic thread");
    };
    *face = cadmpeg_ir::features::FaceSelection::Unresolved;
    *diameter = Some(Length(8.0));
    project_unbound_cosmetic_thread_faces(
        &mut features,
        std::slice::from_ref(&history),
        &[],
        std::slice::from_ref(&topology_face),
        std::slice::from_ref(&surface),
    );
    assert!(matches!(
        &features[1].definition,
        cadmpeg_ir::features::FeatureDefinition::CosmeticThread {
            face: cadmpeg_ir::features::FaceSelection::Faces(faces),
            ..
        } if faces == std::slice::from_ref(&topology_face.id)
    ));
}


#[test]
fn component_face_reference_accepts_both_nested_body_flags() {
    let body_offset = 30;
    let build_payload = |flag: u8, marker: usize| {
        let mut payload = vec![0; marker - 12];
        payload[body_offset..body_offset + 2].copy_from_slice(&0x802b_u16.to_le_bytes());
        payload[body_offset + 2..body_offset + 6].copy_from_slice(&2u32.to_le_bytes());
        payload[body_offset + 6] = flag;
        assert_eq!(selection_vector_tail(&mut payload, &[6]), marker);
        payload
    };
    let marker = body_offset + 92;
    let mut payload = build_payload(0, marker);

    let (actual_marker, components) =
        component_face_reference_at(&payload, body_offset).expect("required invariant");
    assert_eq!(actual_marker, marker);
    assert_eq!(
        components.last().expect("required invariant").local_id,
        Some(6)
    );

    let compact = build_payload(0, body_offset + 68);
    assert!(component_face_reference_at(&compact, body_offset).is_some());

    let flagged = build_payload(0x40, body_offset + 100);
    assert!(component_face_reference_at(&flagged, body_offset).is_some());
    let mut record = CLASS_MARKER.to_vec();
    record.extend((b"moCompFace_c".len() as u16).to_le_bytes());
    record.extend(b"moCompFace_c");
    record.extend_from_slice(&flagged[body_offset..]);
    assert!(component_face_reference_in_record(&record).is_some());

    payload[body_offset + 6] = 1;
    assert_eq!(component_face_reference_at(&payload, body_offset), None);
}


#[test]
fn sketch_surface_component_path_has_two_implicit_root_slots() {
    let marker = 12;
    let mut payload = Vec::new();
    payload.extend(5u32.to_le_bytes());
    payload.extend([0, 3, 0, 0]);
    payload.extend([0; 4]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    for (index, local_id) in [4u32, 3, 5].into_iter().enumerate() {
        if index == 2 {
            payload.extend([0; 2]);
        }
        payload.extend((0x8094 + index as u16).to_le_bytes());
        payload.extend([0; 2]);
        payload.extend([index as u8 + 1; 12]);
        payload.extend(local_id.to_le_bytes());
    }

    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(4), Some(3), Some(5)]
    );
}


#[test]
fn sketch_surface_component_path_accepts_a_slot_cell_between_entries() {
    let marker = 12;
    let mut payload = Vec::new();
    payload.extend(5u32.to_le_bytes());
    payload.extend([0, 3, 0, 0]);
    payload.extend([0; 4]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    for (index, local_id) in [2u32, 0, 1].into_iter().enumerate() {
        if index == 1 {
            payload.extend([0; 4]);
        } else if index == 2 {
            payload.extend([1, 0, 0, 0, 0, 0]);
        }
        payload.extend((0x8034 + index as u16).to_le_bytes());
        payload.extend([0; 2]);
        payload.extend([index as u8 + 1; 12]);
        payload.extend(local_id.to_le_bytes());
    }

    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(2), Some(0), Some(1)]
    );

    let slot = marker + 18 + 20 + 4 + 20;
    payload[slot..slot + 6].fill(0);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(2), Some(0), Some(1)]
    );

    payload[slot..slot + 2].fill(0xff);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker),
        None
    );
}


#[test]
fn legacy_sketch_surface_component_path_requires_its_ownership_trailer() {
    let marker = 12;
    let mut payload = Vec::new();
    payload.extend(5u32.to_le_bytes());
    payload.extend([0, 2, 0, 0]);
    payload.extend(7u32.to_le_bytes());
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    for (index, local_id) in [2u32, 1, 0].into_iter().enumerate() {
        if index == 1 {
            payload.extend(3u32.to_le_bytes());
        } else if index == 2 {
            payload.extend(12u16.to_le_bytes());
            payload.extend([0; 4]);
        }
        payload.extend((0x8032 + index as u16).to_le_bytes());
        payload.extend([0; 2]);
        payload.extend([index as u8 + 1; 12]);
        payload.extend(local_id.to_le_bytes());
    }
    let trailer = payload.len();
    payload.extend([0; 20]);
    payload.extend(1u32.to_le_bytes());
    payload.extend(0u32.to_le_bytes());
    payload.extend(175u32.to_le_bytes());
    payload.extend([0; 12]);

    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker)
            .expect("required invariant")
            .iter()
            .map(|component| component.local_id)
            .collect::<Vec<_>>(),
        [Some(2), Some(1), Some(0)]
    );

    payload[trailer + 28..trailer + 32].fill(0);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker),
        None
    );

    payload[trailer + 28..trailer + 32].copy_from_slice(&175u32.to_le_bytes());
    payload.truncate(trailer);
    payload.extend(14u32.to_le_bytes());
    payload.extend([0; 8]);
    payload.extend(1u32.to_le_bytes());
    payload.extend(0u32.to_le_bytes());
    payload.extend(135u32.to_le_bytes());
    payload.extend([0; 12]);
    assert!(compact_sketch_surface_component_path_at(&payload, marker).is_some());

    payload[trailer..trailer + 4].fill(0);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker),
        None
    );

    payload.truncate(trailer);
    payload.extend([0; 8]);
    payload.extend(1u32.to_le_bytes());
    payload.extend(0u32.to_le_bytes());
    payload.extend(135u32.to_le_bytes());
    payload.extend([0; 12]);
    assert!(compact_sketch_surface_component_path_at(&payload, marker).is_some());

    payload[trailer + 16..trailer + 20].fill(0);
    assert_eq!(
        compact_sketch_surface_component_path_at(&payload, marker),
        None
    );
}


#[test]
fn mirror_pattern_path_count_includes_the_unserialized_root_cell() {
    let marker = 12;
    let mut payload = vec![0; marker];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    for (index, (instance, signature)) in [
        (
            0x803e_u16,
            [0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x7a, 0x83, 0xd9, 0x4a],
        ),
        (
            0x8263,
            [0x34, 0x80, 0x37, 0, 50, 0, 0, 0, 0xf9, 0x83, 0xd9, 0x4a],
        ),
        (
            0x803e,
            [0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x7a, 0x83, 0xd9, 0x4a],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 2 {
            payload.extend([0; 8]);
        }
        payload.extend(instance.to_le_bytes());
        payload.extend([0, 0]);
        payload.extend(signature);
        payload.extend([2u32, 1, 3][index].to_le_bytes());
    }
    payload.extend([0; 32]);

    let path = mirror_pattern_component_path_at(&payload, marker).expect("required invariant");
    assert_eq!(path.len(), 3);
    assert_eq!(path.last().expect("required invariant").local_id, Some(3));
    assert_eq!(
        &path.last().expect("required invariant").type_signature[4..8],
        &37u32.to_le_bytes()
    );

    payload[..4].copy_from_slice(&5u32.to_le_bytes());
    assert_eq!(
        mirror_pattern_component_path_at(&payload, marker)
            .expect("two root slots")
            .len(),
        3
    );
    payload[4] = 1;
    assert!(mirror_pattern_component_path_at(&payload, marker).is_none());

    for (count, separator) in [
        (3u32, &[][..]),
        (4, &[1, 0, 0, 0, 0, 0, 0, 0][..]),
        (5, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0][..]),
    ] {
        let mut mixed = vec![0; marker];
        mixed[..4].copy_from_slice(&count.to_le_bytes());
        mixed.extend(COMPACT_EDGE_VECTOR_MARKER);
        mixed.extend([0, 0]);
        mixed.extend(0x803e_u16.to_le_bytes());
        mixed.extend([0, 0]);
        mixed.extend([0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x7a, 0x83, 0xd9, 0x4a]);
        mixed.extend(2u32.to_le_bytes());
        mixed.extend(separator);
        mixed.extend([0x34, 0x80, 0x37, 0, 50, 0, 0, 0, 0xf9, 0x83, 0xd9, 0x4a]);
        mixed.extend(1u32.to_le_bytes());
        mixed.extend(0x8263_u16.to_le_bytes());
        mixed.extend([0, 0]);
        mixed.extend([0x34, 0x80, 0x37, 0, 37, 0, 0, 0, 0x7a, 0x83, 0xd9, 0x4a]);
        mixed.extend(3u32.to_le_bytes());
        assert_eq!(
            mirror_pattern_component_path_at(&mixed, marker)
                .expect("mixed mirror path")
                .len(),
            3
        );
    }
}


#[test]
fn mirror_surface_path_preserves_tagged_and_anonymous_nodes() {
    let marker = 12;
    let mut payload = vec![0; marker];
    payload[..4].copy_from_slice(&2u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload.extend(COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0, 0]);
    payload.extend(0x803e_u16.to_le_bytes());
    payload.extend([0, 0]);
    payload.extend([0x34, 0x80, 1, 0, 57, 0, 0, 0, 1, 0, 0, 0]);
    payload.extend(9u32.to_le_bytes());
    payload.extend([0; 4]);
    payload.extend([0x34, 0x80, 1, 0, 56, 0, 0, 0, 2, 0, 0, 0]);
    payload.extend(4u32.to_le_bytes());

    let path = mirror_surface_component_path_at(&payload, marker).expect("required invariant");
    assert_eq!(path.len(), 2);
    assert_eq!(path[0].instance, Some(0x803e));
    assert_eq!(path[0].local_id, Some(9));
    assert_eq!(path[1].instance, None);
    assert_eq!(path[1].local_id, Some(4));
    assert_eq!(&path[1].type_signature[4..8], &56u32.to_le_bytes());
    assert!(surface_reference_matches_at(&payload, marker, &path));
}


#[test]
fn inline_surface_path_distinguishes_branch_and_selection_nodes() {
    let prefix = [0x54, 0x81, 0x56, 0x01];
    let signature = |source: u32, identity: u32| {
        let mut signature = [0; 12];
        signature[..4].copy_from_slice(&prefix);
        signature[4..8].copy_from_slice(&source.to_le_bytes());
        signature[8..].copy_from_slice(&identity.to_le_bytes());
        signature
    };
    let mut payload = 0x8157_u16.to_le_bytes().to_vec();
    payload.extend([0, 0]);
    payload.extend(signature(20, 1));
    payload.extend(0x8200_u16.to_le_bytes());
    payload.extend([0, 0]);
    payload.extend(signature(10, 2));
    payload.extend(7u32.to_le_bytes());

    let path = inline_surface_reference_at(&payload, 4).expect("required invariant");
    assert_eq!(path.len(), 2);
    assert_eq!(path[0].instance, Some(0x8157));
    assert_eq!(path[0].local_id, None);
    assert_eq!(path[1].instance, Some(0x8200));
    assert_eq!(path[1].local_id, Some(7));
}


#[test]
fn generated_surface_identities_are_producer_outputs() {
    let class_name = "moWzdHoleSurfIdRep_c";
    let prefix = [0xc3, 0x80, 0xc5, 0x00];
    let mut payload = CLASS_MARKER.to_vec();
    payload.extend((class_name.len() as u16).to_le_bytes());
    payload.extend(class_name.as_bytes());
    payload.extend([0, 0]);
    payload.extend(prefix);
    payload.extend(89u32.to_le_bytes());
    payload.extend(0x52e4_6185u32.to_le_bytes());
    payload.extend(2u32.to_le_bytes());
    payload.extend(0x85b5u16.to_le_bytes());
    payload.extend([0, 0]);
    payload.extend(prefix);
    payload.extend(89u32.to_le_bytes());
    payload.extend(0x52e4_6185u32.to_le_bytes());
    payload.extend(2u32.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            name: class_name.into(),
            role: FeatureInputClassRole::Auxiliary,
        }],
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    let identities = generated_surface_identities(&lane);

    assert_eq!(identities.len(), 2, "{identities:#?}");
    assert!(identities.iter().all(|identity| {
        identity.type_prefix == prefix
            && identity.feature_source_id == 89
            && identity.local_identity == 2
    }));
    assert_eq!(identities[0].components[0].instance, None);
    assert_eq!(identities[1].components[0].instance, Some(0x85b5));
}


#[test]
fn component_path_type_identities_name_ordered_features() {
    let feature = |id: &str, source_id: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source_id.into()),
        parent_source_id: None,
        ordinal: 0,
        name: String::new(),
        kind: String::new(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    let mut signature = [0u8; 12];
    signature[4..8].copy_from_slice(&42u32.to_le_bytes());
    let components = vec![
        FeatureInputComponentPathEntry {
            instance: Some(0x8032),
            type_signature: signature,
            local_id: Some(7),
        },
        FeatureInputComponentPathEntry {
            instance: Some(0x803b),
            type_signature: signature,
            local_id: Some(1),
        },
    ];
    assert_eq!(
        component_path_features(&components, &[feature("producer", "42")]),
        vec!["producer"]
    );
    assert_eq!(
        component_path_features(
            &components,
            &[feature("first", "42"), feature("second", "42")]
        ),
        Vec::<String>::new()
    );
    let mut mixed = components;
    mixed[1].type_signature[4..8].copy_from_slice(&43u32.to_le_bytes());
    assert_eq!(
        component_path_features(&mixed, &[feature("producer", "42"), feature("other", "43")]),
        vec!["producer", "other"]
    );
    assert_eq!(
        component_path_terminal_feature(
            &mixed,
            &[feature("producer", "42"), feature("other", "43")]
        ),
        Some("other".into())
    );
    assert_eq!(
        surface_selection_producer_features(
            &mixed,
            Some("explicit"),
            &[feature("producer", "42"), feature("other", "43")]
        ),
        ["producer", "other", "explicit"]
    );
    mixed.push(FeatureInputComponentPathEntry {
        instance: Some(0x8040),
        type_signature: {
            let mut signature = [0; 12];
            signature[4..8].copy_from_slice(&99u32.to_le_bytes());
            signature
        },
        local_id: Some(5),
    });
    assert_eq!(
        component_path_terminal_feature(
            &mixed,
            &[feature("producer", "42"), feature("other", "43")]
        ),
        Some("other".into())
    );

    let owner = feature("mirror", "44");
    mixed.push(FeatureInputComponentPathEntry {
        instance: None,
        type_signature: {
            let mut signature = [0; 12];
            signature[4..8].copy_from_slice(&44u32.to_le_bytes());
            signature
        },
        local_id: Some(9),
    });
    let producer = feature("producer", "42");
    let other = feature("other", "43");
    let history = [&producer, &other, &owner];
    let (component, preceding) =
        component_path_feature(&mixed, &history, "mirror", ComponentPathEnd::Trailing)
            .expect("required invariant");
    assert_eq!(preceding.id, "other");
    assert_eq!(component.local_id, Some(1));

    let mut prior = feature("prior", "42");
    prior.ordinal = 3;
    let mut consumer = feature("consumer", "88");
    consumer.ordinal = 2;
    let mut future = feature("future", "99");
    future.ordinal = 1;
    let path = [88_u32, 42, 99, 88]
        .into_iter()
        .map(|source| FeatureInputComponentPathEntry {
            instance: Some(0x8180),
            type_signature: {
                let mut signature = [0; 12];
                signature[4..8].copy_from_slice(&source.to_le_bytes());
                signature
            },
            local_id: Some(1),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        component_path_input_features(&path, &[prior, consumer, future], "consumer"),
        ["prior"]
    );
}


#[test]
fn idless_history_features_use_unique_feature_input_object_sources() {
    let feature = Feature {
        id: "producer".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 0,
        name: "Producer".into(),
        kind: "Feature".into(),
        input_class: Some("ProducerClass".into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature],
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: Some(233),
            value: "Producer".into(),
        }],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    let ambiguous_history = history.clone();
    let resolved = history_features_with_object_sources(&[history], &lane);

    assert_eq!(resolved[0].source_id.as_deref(), Some("233"));

    lane.names.push(FeatureInputName {
        id: "ambiguous-name".into(),
        parent: "lane".into(),
        ordinal: 1,
        offset: 1,
        object_id: Some(234),
        value: "Producer".into(),
    });
    let ambiguous = history_features_with_object_sources(&[ambiguous_history], &lane);
    assert_eq!(ambiguous[0].source_id, None);
}


#[test]
fn revolution_line_reference_inputs_decode_profile_owner_and_placed_axis() {
    let mut payload = vec![0; 240];
    let handles = 96;
    payload[64..68].copy_from_slice(&42u32.to_le_bytes());
    payload[68..72].copy_from_slice(&0x5919_4a35u32.to_le_bytes());
    payload[72..74].copy_from_slice(&0x81dbu16.to_le_bytes());
    payload[76..80].copy_from_slice(&[0xff; 4]);
    payload[handles..handles + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 4..handles + 8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 12..handles + 16].copy_from_slice(&7000u32.to_le_bytes());
    for (index, value) in [0.012, -0.034, 0.056, 0.0, 1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 16 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[handles + 64..handles + 68].copy_from_slice(CLASS_MARKER);
    assert_eq!(
        revolution_line_reference_inputs(&payload, 32, payload.len(), &HashSet::from([42])),
        Some((
            42,
            Point3::new(12.0, -34.0, 56.0),
            Vector3::new(0.0, 1.0, 0.0)
        ))
    );

    payload[handles + 8..handles + 12].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 12..handles + 16].copy_from_slice(&[0; 4]);
    payload[handles + 16..handles + 20].copy_from_slice(&7000u32.to_le_bytes());
    payload[handles + 64..handles + 68].copy_from_slice(&[0; 4]);
    for (index, value) in [0.012, -0.034, 0.056, 0.1, 0.2, 0.3, 0.0, 0.0, -1.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 20 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[handles + 92..handles + 96].copy_from_slice(CLASS_MARKER);
    assert_eq!(
        revolution_line_reference_inputs(&payload, 32, payload.len(), &HashSet::from([42])),
        Some((
            42,
            Point3::new(12.0, -34.0, 56.0),
            Vector3::new(0.0, 0.0, -1.0)
        ))
    );

    payload.fill(0);
    payload[64..68].copy_from_slice(&42u32.to_le_bytes());
    payload[68..72].copy_from_slice(&0x5919_4a35u32.to_le_bytes());
    payload[72..74].copy_from_slice(&0x81dbu16.to_le_bytes());
    payload[76..80].copy_from_slice(&[0xff; 4]);
    payload[handles..handles + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 4..handles + 8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 12..handles + 16].copy_from_slice(&7000u32.to_le_bytes());
    for (index, value) in [0.012, -0.034, 0.056, 0.1, 0.2, 0.0, 1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 16 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[handles + 80..handles + 84].copy_from_slice(CLASS_MARKER);
    assert_eq!(
        revolution_line_reference_inputs(&payload, 32, payload.len(), &HashSet::from([42])),
        Some((
            42,
            Point3::new(12.0, -34.0, 56.0),
            Vector3::new(0.0, 1.0, 0.0)
        ))
    );
}


#[test]
fn revolution_line_reference_inputs_decode_repeated_instance_frame() {
    let mut payload = vec![0; 240];
    let handles = 96;
    payload[64..68].copy_from_slice(&42u32.to_le_bytes());
    payload[68..72].copy_from_slice(&0x536b_2f76u32.to_le_bytes());
    payload[72..74].copy_from_slice(&0x8127u16.to_le_bytes());
    payload[76..80].copy_from_slice(&[0xff; 4]);
    payload[handles..handles + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 4..handles + 8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 12..handles + 16].copy_from_slice(&7000u32.to_le_bytes());
    for (index, value) in [0.012, -0.034, 0.056, 0.0, 0.0, 1.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 24 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[handles + 75..handles + 77].copy_from_slice(&0x81bau16.to_le_bytes());

    assert_eq!(
        revolution_line_reference_inputs(&payload, 32, payload.len(), &HashSet::from([42])),
        Some((
            42,
            Point3::new(12.0, -34.0, 56.0),
            Vector3::new(0.0, 0.0, 1.0)
        ))
    );
}


#[test]
fn revolution_line_reference_inputs_decode_declared_pre_handle_address() {
    let mut payload = vec![0; 240];
    let handles = 108;
    let source = handles - 44;
    payload[source..source + 4].copy_from_slice(&42u32.to_le_bytes());
    payload[source + 4..source + 8].copy_from_slice(&0x4901_2c88u32.to_le_bytes());
    payload[source + 8..source + 10].copy_from_slice(&0x810fu16.to_le_bytes());
    payload[source + 12..source + 16].copy_from_slice(&[0xff; 4]);
    payload[source + 16..source + 20].copy_from_slice(&1u32.to_le_bytes());
    payload[source + 20..source + 24].copy_from_slice(&1u32.to_le_bytes());
    payload[source + 28..source + 32].copy_from_slice(&122u32.to_le_bytes());
    payload[handles..handles + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 4..handles + 8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    for (index, value) in [1.0, 0.0, 0.060_285_851_239_7, 0.0, 0.0, -1.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 24 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[handles + 72..handles + 76].copy_from_slice(CLASS_MARKER);

    assert_eq!(
        revolution_line_reference_inputs(&payload, 32, payload.len(), &HashSet::from([42])),
        Some((
            42,
            Point3::new(1000.0, 0.0, 0.060_285_851_239_7 * 1000.0),
            Vector3::new(0.0, 0.0, -1.0)
        ))
    );
}


#[test]
fn revolution_line_reference_inputs_decode_declared_three_handle_layouts() {
    let make_payload = |addressed: bool| {
        let mut payload = vec![0; 320];
        let handles = 128;
        let source = handles - 48;
        payload[source..source + 4].copy_from_slice(&42u32.to_le_bytes());
        payload[source + 4..source + 8].copy_from_slice(&0x3e34_ce43u32.to_le_bytes());
        payload[source + 8..source + 10].copy_from_slice(&0x8101u16.to_le_bytes());
        payload[source + 12..source + 16].copy_from_slice(&[0xff; 4]);
        payload[source + 20..source + 24]
            .copy_from_slice(&(if addressed { 4u32 } else { 10 }).to_le_bytes());
        payload[source + 24..source + 28].copy_from_slice(&1u32.to_le_bytes());
        payload[source + 32..source + 36].copy_from_slice(&274u32.to_le_bytes());
        for offset in [handles, handles + 4, handles + 8] {
            payload[offset..offset + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
        }
        let handles_end = handles + 12;
        let (frame, values, marker) = if addressed {
            payload[handles_end + 4..handles_end + 8].copy_from_slice(&9000u32.to_le_bytes());
            payload[handles_end + 20..handles_end + 24].copy_from_slice(&[0xff; 4]);
            (
                handles_end + 24,
                vec![0.0, 0.015, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                handles_end + 89,
            )
        } else {
            (
                handles_end + 4,
                vec![0.0, 0.0, 0.0, 0.052, 0.0, 0.0, 0.0, 0.0, 1.0],
                handles_end + 85,
            )
        };
        for (index, value) in values.into_iter().enumerate() {
            let offset = frame + index * 8;
            payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
        }
        payload[marker..marker + 4].copy_from_slice(CLASS_MARKER);
        payload
    };

    assert_eq!(
        revolution_line_reference_inputs(&make_payload(true), 32, 320, &HashSet::from([42])),
        Some((42, Point3::new(0.0, 15.0, 0.0), Vector3::new(0.0, 0.0, 1.0)))
    );
    assert_eq!(
        revolution_line_reference_inputs(&make_payload(false), 32, 320, &HashSet::from([42])),
        Some((42, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)))
    );
}


#[test]
fn revolution_line_reference_inputs_decode_extended_two_handle_layouts() {
    let mut declared = vec![0; 280];
    let handles = 112;
    let source = handles - 48;
    declared[source..source + 4].copy_from_slice(&42u32.to_le_bytes());
    declared[source + 4..source + 8].copy_from_slice(&0x49ab_4bc9u32.to_le_bytes());
    declared[source + 8..source + 10].copy_from_slice(&0x8120u16.to_le_bytes());
    declared[source + 12..source + 16].copy_from_slice(&[0xff; 4]);
    declared[source + 20..source + 24].copy_from_slice(&2u32.to_le_bytes());
    declared[source + 24..source + 28].copy_from_slice(&1u32.to_le_bytes());
    declared[source + 32..source + 36].copy_from_slice(&308u32.to_le_bytes());
    for offset in [handles, handles + 4] {
        declared[offset..offset + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    for (index, value) in [0.0, 0.0, 0.0, 0.064, 0.0, 0.0, 0.0, 1.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 8 + index * 8;
        declared[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    declared[handles + 89..handles + 93].copy_from_slice(CLASS_MARKER);
    assert_eq!(
        revolution_line_reference_inputs(&declared, 32, declared.len(), &HashSet::from([42])),
        Some((42, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)))
    );

    let mut nested = vec![0; 300];
    let handles = 132;
    let source = handles - 44;
    nested[source..source + 4].copy_from_slice(&42u32.to_le_bytes());
    nested[source + 4..source + 8].copy_from_slice(&0x4890_6465u32.to_le_bytes());
    nested[source + 8..source + 10].copy_from_slice(&0x80b6u16.to_le_bytes());
    nested[source + 12..source + 16].copy_from_slice(&[0xff; 4]);
    nested[source + 16..source + 20].copy_from_slice(&7u32.to_le_bytes());
    nested[source + 20..source + 24].copy_from_slice(&1u32.to_le_bytes());
    nested[source + 28..source + 32].copy_from_slice(&126u32.to_le_bytes());
    for offset in [handles, handles + 4] {
        nested[offset..offset + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    nested[handles + 12..handles + 16].copy_from_slice(&3800u32.to_le_bytes());
    for (index, value) in [0.056, -0.051, -0.008, 0.0, 1.0, 0.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 24 + index * 8;
        nested[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    nested[handles + 85..handles + 89].copy_from_slice(&103u32.to_le_bytes());
    assert_eq!(
        revolution_line_reference_inputs(&nested, 32, nested.len(), &HashSet::from([42])),
        Some((
            42,
            Point3::new(56.0, -51.0, -8.0),
            Vector3::new(1.0, 0.0, 0.0)
        ))
    );
}


#[test]
fn revolution_line_reference_inputs_decode_declared_post_handle_address() {
    let mut payload = vec![0; 300];
    let handles = 128;
    let source = handles - 48;
    payload[source..source + 4].copy_from_slice(&42u32.to_le_bytes());
    payload[source + 4..source + 8].copy_from_slice(&0x5976_e99cu32.to_le_bytes());
    payload[source + 8..source + 10].copy_from_slice(&0x81e4u16.to_le_bytes());
    payload[source + 12..source + 16].copy_from_slice(&[0xff; 4]);
    payload[source + 20..source + 24].copy_from_slice(&1u32.to_le_bytes());
    payload[source + 24..source + 28].copy_from_slice(&1u32.to_le_bytes());
    payload[source + 32..source + 36].copy_from_slice(&151u32.to_le_bytes());
    for offset in [handles, handles + 4, handles + 8] {
        payload[offset..offset + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    payload[handles + 16..handles + 20].copy_from_slice(&8000u32.to_le_bytes());
    for (index, value) in [0.0, 0.0, 0.006, 0.006, 0.0, 0.0, -1.0, 0.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 24 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[handles + 97..handles + 101].copy_from_slice(CLASS_MARKER);

    assert_eq!(
        revolution_line_reference_inputs(&payload, 32, payload.len(), &HashSet::from([42])),
        Some((42, Point3::new(0.0, 0.0, 6.0), Vector3::new(-1.0, 0.0, 0.0)))
    );
}


#[test]
fn revolution_temporary_axis_decodes_placed_axis_record() {
    let mut payload = vec![0; 400];
    let declaration = 40;
    payload[declaration..declaration + 4].copy_from_slice(CLASS_MARKER);
    payload[declaration + 4..declaration + 6].copy_from_slice(&15u16.to_le_bytes());
    payload[declaration + 6..declaration + 21].copy_from_slice(b"moTempAxisRef_w");
    for offset in [declaration + 223, declaration + 227] {
        payload[offset..offset + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    payload[declaration + 235..declaration + 239].copy_from_slice(&5000u32.to_le_bytes());
    for (index, value) in [0.0, 0.0, 0.03, 0.0, 0.0, 0.072, 0.0, 0.0, -1.0]
        .into_iter()
        .enumerate()
    {
        let offset = declaration + 239 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[declaration + 312..declaration + 316].copy_from_slice(CLASS_MARKER);

    assert_eq!(
        revolution_temporary_axis(&payload, 32, payload.len()),
        Some((Point3::new(0.0, 0.0, 30.0), Vector3::new(0.0, 0.0, -1.0)))
    );
}


#[test]
fn compact_component_matrix_places_a_sketch_plane() {
    let mut payload = vec![0; 138];
    payload[..4].copy_from_slice(&89u32.to_le_bytes());
    payload[14] = 1;
    for (index, value) in [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, -0.031, 1.0,
    ]
    .into_iter()
    .enumerate()
    {
        let offset = 15 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[122..126].copy_from_slice(&4u32.to_le_bytes());
    payload[126..130].copy_from_slice(&[0xff; 4]);

    assert_eq!(
        compact_component_plane_frame(&payload),
        Some((
            Point3::new(0.0, 0.0, -31.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0)
        ))
    );
}


#[test]
fn indexed_profile_construction_line_places_a_revolution_axis() {
    let mut payload = vec![0; 300];
    for offset in [0, 100, 200] {
        payload[offset..offset + 5].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    }
    payload[217..221].copy_from_slice(&2u32.to_le_bytes());
    payload[256..258].copy_from_slice(&0u16.to_le_bytes());
    payload[258..260].copy_from_slice(&2u16.to_le_bytes());
    payload[260..264].copy_from_slice(&[1, 0, 0, 0]);
    let marker = |id: &str,
                  offset: u64,
                  object_index: Option<u32>,
                  coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile-native".into()),
        ordinal: object_index.unwrap_or(3),
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![
            marker("first", 0, Some(1), Some([0.0, 0.0195])),
            marker("relation", 50, None, None),
            marker("second", 100, Some(2), Some([0.008, 0.0195])),
            marker("axis", 200, None, None),
        ],
    };
    lane.sketch_entities[1].kind = SketchInputKind::Relation(SketchRelationKind::Distance);
    lane.sketch_entities[3].kind = SketchInputKind::LineOrCircle;
    let sketch = Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, -1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, -1.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };

    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 19.5),
            direction: Vector3::new(1.0, 0.0, 0.0),
        })
    );
    let markers = lane.sketch_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        roster_curve_endpoint_markers(&lane.native_payload, &lane.sketch_entities[3], &markers,)
            .into_iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    lane.native_payload.resize(400, 0);
    lane.native_payload[200..292].fill(0);
    lane.native_payload[200..205].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.native_payload[205..213].fill(0xff);
    lane.native_payload[213..217].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    lane.native_payload[217..221].copy_from_slice(&4u32.to_le_bytes());
    lane.native_payload[223..227].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    lane.native_payload[227..229].copy_from_slice(&2u16.to_le_bytes());
    lane.native_payload[231..239]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    lane.native_payload[248..256].copy_from_slice(&1.0f64.to_le_bytes());
    lane.native_payload[264..266].copy_from_slice(&0u16.to_le_bytes());
    lane.native_payload[266..268].copy_from_slice(&1u16.to_le_bytes());
    lane.native_payload[272..280].copy_from_slice(&(-1.0f64).to_le_bytes());
    lane.native_payload[292..297].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.sketch_entities[3].kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 19.5),
            direction: Vector3::new(1.0, 0.0, 0.0),
        })
    );

    lane.native_payload[200..292].fill(0);
    lane.native_payload[200..205].copy_from_slice(SKETCH_MARKER);
    lane.native_payload[205..213].fill(0xff);
    lane.native_payload[213..217].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    lane.native_payload[217..221].copy_from_slice(&5u32.to_le_bytes());
    lane.native_payload[223..227].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    lane.native_payload[227..229].copy_from_slice(&2u16.to_le_bytes());
    lane.native_payload[231..239]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    lane.native_payload[248..256].copy_from_slice(&1.0f64.to_le_bytes());
    lane.native_payload[256..258].copy_from_slice(&0u16.to_le_bytes());
    lane.native_payload[258..260].copy_from_slice(&1u16.to_le_bytes());
    lane.native_payload[264..272].copy_from_slice(&(-1.0f64).to_le_bytes());
    lane.native_payload[284..289].copy_from_slice(SKETCH_MARKER);
    lane.sketch_entities[3].kind = SketchInputKind::Relation(SketchRelationKind::Vertical);
    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 19.5),
            direction: Vector3::new(1.0, 0.0, 0.0),
        })
    );

    lane.native_payload[200..312].fill(0);
    lane.native_payload[200..205].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.native_payload[205..213].fill(0xff);
    lane.native_payload[213..217].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    lane.native_payload[217..221].copy_from_slice(&4u32.to_le_bytes());
    lane.native_payload[223..227].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    lane.native_payload[227..231].copy_from_slice(&[1, 0, 1, 0]);
    lane.native_payload[231..239]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    lane.native_payload[248..256].copy_from_slice(&1.0f64.to_le_bytes());
    lane.native_payload[264..266].copy_from_slice(&0u16.to_le_bytes());
    lane.native_payload[266..268].copy_from_slice(&1u16.to_le_bytes());
    lane.native_payload[268..272].copy_from_slice(&1u32.to_le_bytes());
    lane.native_payload[272..280].copy_from_slice(&(-1.0f64).to_le_bytes());
    lane.native_payload[280..284].copy_from_slice(&u32::MAX.to_le_bytes());
    lane.native_payload[284..286].copy_from_slice(&1u16.to_le_bytes());
    for offset in (286..302).step_by(4) {
        lane.native_payload[offset..offset + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    lane.native_payload[312..317].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.sketch_entities[3].kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 19.5),
            direction: Vector3::new(1.0, 0.0, 0.0),
        })
    );
}


#[test]
fn compact_profile_construction_role_places_a_revolution_axis() {
    let mut payload = vec![0; 300];
    for offset in [0, 100, 200] {
        payload[offset..offset + 5].copy_from_slice(LEGACY_SKETCH_MARKER);
    }
    payload[205..213].fill(0xff);
    payload[213..217].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[217..221].copy_from_slice(&0u32.to_le_bytes());
    payload[223..227].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[227..229].copy_from_slice(&2u16.to_le_bytes());
    payload[231..239].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0d, 0x00]);
    payload[248..256].copy_from_slice(&1.0f64.to_le_bytes());
    payload[264..266].copy_from_slice(&0u16.to_le_bytes());
    payload[266..268].copy_from_slice(&1u16.to_le_bytes());
    payload[272..280].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[292..297].copy_from_slice(LEGACY_SKETCH_MARKER);
    let marker = |id: &str,
                  offset: u64,
                  object_index: Option<u32>,
                  coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile-native".into()),
        ordinal: object_index.unwrap_or(3),
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![
            marker("first", 0, Some(1), Some([0.0, 0.0195])),
            marker("second", 100, Some(2), Some([0.008, 0.0195])),
            marker("axis", 200, None, None),
        ],
    };
    lane.sketch_entities[2].kind = SketchInputKind::LineOrCircle;
    let sketch = Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, -1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, -1.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };

    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 19.5),
            direction: Vector3::new(1.0, 0.0, 0.0),
        })
    );
    lane.sketch_entities[0].kind = SketchInputKind::Arc;
    assert!(profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]).is_some());
}


#[test]
fn bounded_profile_chords_place_implicit_revolution_axes() {
    let curve = 300;
    let mut payload = vec![0; curve + 180];
    payload[curve..curve + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[curve + 5..curve + 13].fill(0xff);
    payload[curve + 13..curve + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[curve + 17..curve + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[curve + 23..curve + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[curve + 27..curve + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[curve + 31..curve + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[curve + 48..curve + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[curve + 56..curve + 58].copy_from_slice(&0u16.to_le_bytes());
    payload[curve + 58..curve + 60].copy_from_slice(&1u16.to_le_bytes());
    payload[curve + 84..curve + 84 + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    let marker = |id: &str,
                  offset: u64,
                  object_index: Option<u32>,
                  coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile-native".into()),
        ordinal: object_index.unwrap_or(4),
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![
            marker("first", 0, Some(1), Some([0.0, 0.0])),
            marker("second", 100, Some(2), Some([0.0, 0.02])),
            marker("profile-point", 200, Some(3), Some([-0.01, 0.01])),
            marker("axis-chord", curve as u64, None, None),
        ],
    };
    lane.sketch_entities[3].kind = SketchInputKind::Arc;
    let sketch = Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, -1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, -1.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };

    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        })
    );

    lane.sketch_entities.push(marker(
        "opposite-profile-point",
        250,
        Some(4),
        Some([0.01, 0.01]),
    ));
    let markers = lane.sketch_entities.iter().collect::<Vec<_>>();
    assert!(!bounded_profile_axis_endpoints(
        "profile-native",
        &markers,
        &HashSet::from(["profile-point", "opposite-profile-point"]),
        [&lane.sketch_entities[0], &lane.sketch_entities[1]],
    ));
    lane.sketch_entities[4].object_index = None;
    let markers = lane.sketch_entities.iter().collect::<Vec<_>>();
    assert!(bounded_profile_axis_endpoints(
        "profile-native",
        &markers,
        &HashSet::from(["profile-point", "opposite-profile-point"]),
        [&lane.sketch_entities[0], &lane.sketch_entities[1]],
    ));
    lane.sketch_entities.pop();

    lane.sketch_entities[2].kind = SketchInputKind::LineOrCircle;
    assert!(profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]).is_some());

    lane.sketch_entities[2].kind = SketchInputKind::Point;
    lane.sketch_entities[2].coordinates_m = Some([-0.01, 0.01]);

    lane.native_payload[curve + 56..curve + 60].fill(0);
    lane.native_payload[curve + 64..curve + 66].copy_from_slice(&0u16.to_le_bytes());
    lane.native_payload[curve + 66..curve + 68].copy_from_slice(&1u16.to_le_bytes());
    lane.native_payload[curve + 68..curve + 72].copy_from_slice(&[1, 0, 0, 0]);
    lane.native_payload[curve + 72..curve + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    lane.native_payload[curve + 84..curve + 92].fill(0);
    lane.native_payload[curve + 92..curve + 92 + SKETCH_MARKER.len()]
        .copy_from_slice(SKETCH_MARKER);
    assert!(profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]).is_some());

    lane.native_payload[curve..curve + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.native_payload[curve + 17..curve + 21].copy_from_slice(&0u32.to_le_bytes());
    lane.native_payload[curve + 56..curve + 58].copy_from_slice(&0u16.to_le_bytes());
    lane.native_payload[curve + 58..curve + 60].copy_from_slice(&1u16.to_le_bytes());
    lane.native_payload[curve + 60..curve + 64].copy_from_slice(&1u32.to_le_bytes());
    lane.native_payload[curve + 64..curve + 80].fill(0);
    let detail = curve + 84;
    lane.native_payload[detail..detail + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.native_payload[detail + 5..detail + 13]
        .copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]);
    lane.native_payload[detail + 13..detail + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    lane.native_payload[detail + 23..detail + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    lane.native_payload[detail + 27..detail + 29].copy_from_slice(&2u16.to_le_bytes());
    lane.native_payload[detail + 31..detail + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    lane.native_payload[detail + 48..detail + 56].copy_from_slice(&1.0f64.to_le_bytes());
    lane.native_payload[detail + 64..detail + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    lane.native_payload[detail + 72..detail + 80].copy_from_slice(&0.0f64.to_le_bytes());
    assert_eq!(
        compact_bounded_curve_tangent(&lane.native_payload, curve),
        Some([-1.0, 0.0])
    );
    assert!(profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]).is_some());

    lane.native_payload[curve..curve + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    lane.native_payload[detail..detail + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    assert!(profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]).is_some());

    lane.native_payload[curve..curve + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.native_payload[detail..detail + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.native_payload[curve + 17..curve + 21].copy_from_slice(&2u32.to_le_bytes());
    assert!(profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]).is_some());

    lane.native_payload[curve + 17..curve + 21].copy_from_slice(&1u32.to_le_bytes());
    lane.sketch_entities[0].coordinates_m = Some([-0.01, 0.0]);
    lane.sketch_entities[1].coordinates_m = Some([-0.01, 0.02]);
    lane.sketch_entities[2].kind = SketchInputKind::LineOrCircle;
    lane.sketch_entities
        .push(marker("axis-start", 450, None, Some([0.0, 0.0])));
    lane.sketch_entities
        .push(marker("axis-end", 460, None, Some([0.0, 0.02])));
    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        })
    );

    lane.sketch_entities.truncate(4);
    lane.sketch_entities[0].coordinates_m = Some([0.0, 0.0]);
    lane.sketch_entities[1].coordinates_m = Some([-0.01, 0.01]);
    lane.sketch_entities
        .push(marker("selected-axis-end", 50, None, Some([0.0, 0.02])));
    lane.native_payload[126..130].copy_from_slice(&1u32.to_le_bytes());
    lane.native_payload[curve + 58..curve + 60].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        })
    );

    lane.native_payload[76..80].copy_from_slice(&1u32.to_le_bytes());
    lane.native_payload[curve + 56..curve + 58].copy_from_slice(&2u16.to_le_bytes());
    lane.native_payload[curve + 58..curve + 60].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        })
    );
}


#[test]
fn generated_revolution_axis_requires_multiple_coaxial_surfaces() {
    let cylinder = |id: &str, origin: Point3| Surface {
        id: SurfaceId(id.into()),
        geometry: SurfaceGeometry::Cylinder {
            origin,
            axis: Vector3::new(1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: 5.0,
        },
        source_object: None,
    };
    let first = cylinder("first", Point3::new(0.0, 0.0, 0.0));
    let second = cylinder("second", Point3::new(10.0, 0.0, 0.0));

    assert_eq!(
        common_generated_surface_axis(std::slice::from_ref(&first)),
        None
    );
    assert_eq!(
        common_generated_surface_axis(&[first.clone(), second]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        })
    );
    assert_eq!(
        common_generated_surface_axis(&[first, cylinder("offset", Point3::new(0.0, 1.0, 0.0)),]),
        None
    );
}


#[test]
fn omitted_origin_and_principal_axes_use_unique_maximum_incidence_support_lines() {
    let mut payload = vec![0; 700];
    let curve = |payload: &mut [u8], offset: usize, start: u16, end: u16| {
        payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0, 0, 0x80, 0xbf]);
        payload[offset + 23..offset + 27].copy_from_slice(&[4, 0, 2, 0]);
        payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 31..offset + 39].copy_from_slice(&[0, 0, 0x80, 0xbf, 0, 0, 4, 0]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&start.to_le_bytes());
        payload[offset + 58..offset + 60].copy_from_slice(&end.to_le_bytes());
        payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
    };
    curve(&mut payload, 400, 0, 1);
    curve(&mut payload, 484, 1, 2);
    curve(&mut payload, 568, 2, 0);
    let marker = |id: &str, offset, object_index, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile-native".into()),
        ordinal: offset as u32,
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let mut entities = vec![
        marker("vertical-near", 0, Some(1), Some([0.0, 0.01])),
        marker("vertical-far", 100, Some(2), Some([0.0, 0.02])),
        marker("tangent", 200, Some(3), Some([-0.01, 0.01])),
        marker("origin", 300, Some(4), Some([0.0, 0.0])),
        marker("curve-a", 400, None, None),
        marker("curve-b", 484, None, None),
        marker("curve-c", 568, None, None),
    ];
    for entity in &mut entities[4..] {
        entity.kind = SketchInputKind::LineOrCircle;
    }
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: entities,
    };
    let markers = lane.sketch_entities.iter().collect::<Vec<_>>();

    assert_eq!(
        profile_roster_origin_axis_endpoints(&lane, "profile-native", &markers),
        Some([[0.0, 0.0], [0.0, 0.01]])
    );
    assert_eq!(
        profile_roster_principal_axis_endpoints(&lane, "profile-native", &markers),
        Some([[0.0, 0.0], [0.0, 1.0]])
    );
}


#[test]
fn revolution_form_words_distinguish_new_body_and_join() {
    for code in [5, 6, 11, 60, 20_322, 22_016] {
        assert_eq!(
            revolution_operation(Some("moRevolution_c"), code),
            Some(BooleanOp::NewBody)
        );
    }
    assert_eq!(
        revolution_operation(Some("moRevolution_c"), 8),
        Some(BooleanOp::Join)
    );
    assert_eq!(revolution_operation(Some("moRevolution_c"), 7), None);
    assert_eq!(
        revolution_operation(Some("moRevCut_c"), 13),
        Some(BooleanOp::Cut)
    );
}


#[test]
fn revolution_consumes_the_preceding_profile_object() {
    let feature = |id: &str, source: &str, class: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source.into()),
        parent_source_id: None,
        ordinal: 0,
        name: id.into(),
        kind: String::new(),
        input_class: Some(class.into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    let mut histories = [FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            feature("profile", "23", "moProfileFeature_c"),
            feature("revolution", "28", "moRevolution_c"),
            feature("cut-profile", "29", "moProfileFeature_c"),
            feature("cut", "30", "moRevCut_c"),
        ],
    }];
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0; 256],
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "profile-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 100,
                object_id: Some(23),
                value: "profile".into(),
            },
            FeatureInputName {
                id: "revolution-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 200,
                object_id: Some(28),
                value: "revolution".into(),
            },
            FeatureInputName {
                id: "cut-profile-name".into(),
                parent: "lane".into(),
                ordinal: 2,
                offset: 220,
                object_id: Some(29),
                value: "cut-profile".into(),
            },
            FeatureInputName {
                id: "cut-name".into(),
                parent: "lane".into(),
                ordinal: 3,
                offset: 240,
                object_id: Some(30),
                value: "cut".into(),
            },
        ],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    enrich_history_revolution_inputs(&mut histories, std::slice::from_ref(&lane));

    assert_eq!(
        histories[0].features[1].properties.get("Profile"),
        Some(&"23".into())
    );
    assert_eq!(
        histories[0].features[3].properties.get("Profile"),
        Some(&"29".into())
    );

    for feature in &mut histories[0].features {
        feature.source_id = None;
        feature.properties.clear();
    }
    enrich_history_revolution_inputs(&mut histories, &[lane]);
    assert_eq!(
        histories[0].features[1].properties.get("Profile"),
        Some(&"23".into())
    );
    assert_eq!(
        histories[0].features[3].properties.get("Profile"),
        Some(&"29".into())
    );
}
