// SPDX-License-Identifier: Apache-2.0
//! Record-decoder tests for the `b2` family over synthetic byte fixtures.

#![allow(clippy::unwrap_used)]

use crate::tests::{
    b2_circle_stream, b2_cone_face_stream, b2_cone_stream, b2_construction_use_stream,
    b2_counted_61_stream, b2_cylinder_stream, b2_edge_node_stream, b2_edge_parameter_stream,
    b2_embedded_cylinder_stream, b2_group_stream, b2_implicit_axis_cylinder_stream,
    b2_link_5f_stream, b2_linked_counted_owner_stream, b2_linked_owner_stream, b2_long_61_stream,
    b2_offset_support_stream, b2_owner_packet_stream, b2_parameter_point_stream, b2_pcurve_stream,
    b2_phase_tailed_cylinder_stream, b2_reference_list_stream, b2_revolution_stream,
    b2_topology_metadata_stream, b2_width_coded_owner_packet_stream, b3_cylinder_stream,
    b3_offset_support_stream,
};
use cadmpeg_ir::geometry::SurfaceGeometry;

#[test]
fn b_family_pcurve_parser_reads_six_channel_uv_jet() {
    let pcurves = crate::families::b2::records::b2_pcurves(&b2_pcurve_stream());
    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].support_id, 0x1234);
    assert_eq!(pcurves[0].degree, 5);
    assert_eq!(pcurves[0].second_derivatives, vec![[0.0, 0.0]; 2]);
}

#[test]
fn b2_parameter_point_parser_reads_uv_station_and_unsplit_layouts() {
    use crate::families::b2::records::B2ParameterPoint;

    let points = crate::families::b2::records::b2_parameter_points(&b2_parameter_point_stream());
    assert_eq!(points.len(), 3);
    assert!(matches!(
        points[0],
        B2ParameterPoint::Uv { uv: [2.0, 3.0], .. }
    ));
    assert!(matches!(
        points[1],
        B2ParameterPoint::StationUv {
            station: 11.0,
            uv: [4.0, 5.0],
            ..
        }
    ));
    assert!(matches!(points[2], B2ParameterPoint::FiveScalars { .. }));
}

#[test]
fn b2_reference_list_parser_reads_compact_refs_and_unit_tail() {
    let records = crate::families::b2::records::b2_reference_lists(&b2_reference_list_stream());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].references, (0u32..26).collect::<Vec<_>>());
}

#[test]
fn b2_owner_packet_parser_closes_nine_references_and_numeric_tail() {
    use crate::families::b2::records::B2OwnerReferenceEncoding;

    let packets = crate::families::b2::records::b2_owner_packets(&b2_owner_packet_stream());
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].header_token, 5);
    assert_eq!(
        packets[0].reference_encoding,
        B2OwnerReferenceEncoding::TaggedU16Strong
    );
    assert_eq!(
        packets[0].references,
        [1000, 1, 1001, 2, 1002, 3, 1003, 4, 1004]
    );
    assert_eq!(
        packets[0].numeric_tail.header,
        [0x84, 0x41, 0xbb, 0x05, 0x0d]
    );
    assert_eq!(packets[0].numeric_tail.scalar64, [-0.0, 4.5, 12.25, 7.0]);
    assert_eq!(
        packets[0].numeric_tail.scalar32,
        [1.0, -2.0, 3.5, 4.0, 5.25, 6.0]
    );

    let packets =
        crate::families::b2::records::b2_owner_packets(&b2_width_coded_owner_packet_stream());
    assert_eq!(packets.len(), 1);
    assert_eq!(
        packets[0].reference_encoding,
        B2OwnerReferenceEncoding::WidthCodedStrong
    );
    assert_eq!(
        packets[0].references,
        [216, 3, 540, 7, 223, 19, 545, 31, 606]
    );
}

#[test]
fn b2_owner_packet_parser_rejects_invalid_numeric_tail_framing() {
    let valid = b2_owner_packet_stream();
    let tail = valid.len() - 62;
    for (offset, replacement) in [
        (0, vec![0x85]),
        (1, vec![0x40]),
        (4, vec![0x0c]),
        (37, vec![0x00]),
        (5, f64::NAN.to_le_bytes().to_vec()),
        (38, f32::INFINITY.to_le_bytes().to_vec()),
    ] {
        let mut invalid = valid.clone();
        invalid[tail + offset..tail + offset + replacement.len()].copy_from_slice(&replacement);
        assert!(crate::families::b2::records::b2_owner_packets(&invalid).is_empty());
    }
}

#[test]
fn b2_counted_61_parser_separates_references_from_tail() {
    let records = crate::families::b2::records::b2_counted_61(&b2_counted_61_stream());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].header_token, 5);
    assert_eq!(records[0].references, [1300, 1294, 30, 74]);
    assert_eq!(records[0].tail, [0x41, 0x03]);
}

#[test]
fn b2_long_61_parser_derives_monotone_member_boundary_from_suffix() {
    let records = crate::families::b2::records::b2_long_61(&b2_long_61_stream());
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].prefix,
        [0xb5, 0x03, 0x2b, 0x47, 0x8f, 0xb3, 0xd7, 0xfb]
    );
    assert_eq!(records[0].members, [0x064a, 0x0650, 0x0656]);
    assert_eq!(
        records[0].references,
        [0x0100, 0x0103, 0x0106, 0x0109, 0x010c]
    );
    assert_eq!(records[0].scalar, 42.5);

    let mut short = vec![0xb2, 0x03, 0x61, 27, 0x05];
    short.extend_from_slice(&[0; 27]);
    short[13] = 0x06;
    assert!(crate::families::b2::records::b2_long_61(&short).is_empty());
}

#[test]
fn b2_link_5f_parser_accepts_each_compact_target_width_and_fixed_tail() {
    let mut bytes = Vec::new();
    for payload in [
        &[0x82, 0x04, 0x5d, 0x03, 0x05][..],
        &[0x82, 0x08, 0x5d, 0x02, 0x03, 0x05],
        &[0x82, 0x0c, 0x5d, 0x02, 0x01, 0x03, 0x05],
        &[0x82, 0x10, 0x5d, 0x02, 0x01, 0x01, 0x03, 0x05],
    ] {
        bytes.extend_from_slice(&[0xb2, 0x03, 0x5f, u8::try_from(payload.len()).unwrap(), 0x05]);
        bytes.extend_from_slice(payload);
    }
    let links = crate::families::b2::records::b2_links_5f(&bytes);
    assert_eq!(links.len(), 4);
    assert!(links.iter().all(|link| link.header_token == 5));
    assert_eq!(
        links.iter().map(|link| link.target).collect::<Vec<_>>(),
        [0x5d, 0x025d, 0x0001_025d, 0x0101_025d]
    );

    let malformed = [
        0xb2, 0x03, 0x5f, 0x06, 0x05, 0x82, 0x04, 0x5d, 0x00, 0x03, 0x05,
    ];
    assert!(crate::families::b2::records::b2_links_5f(&malformed).is_empty());
}

#[test]
fn b2_linked_owner_requires_adjacency_and_successor_identity() {
    let pairs = crate::families::b2::records::b2_linked_owners(&b2_linked_owner_stream());
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].link.target, 1003);
    assert_eq!(pairs[0].owner.references[8], 1004);

    let mut separated = b2_link_5f_stream();
    separated.extend_from_slice(&[0xb2, 0x03, 0x2e, 0x01, 0x05, 0x05]);
    separated.extend_from_slice(&b2_owner_packet_stream());
    assert!(crate::families::b2::records::b2_linked_owners(&separated).is_empty());
}

#[test]
fn b2_counted_owner_closes_variable_reference_lane_and_successor_link() {
    let bytes = b2_linked_counted_owner_stream();
    let owners = crate::families::b2::records::b2_counted_owners(&bytes);
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].references, [911, 7, 263, 258, 281, 276, 917]);
    assert_eq!(owners[0].tail, [0x83, 0x41, 0x92, 0x00, 0x01]);

    let linked = crate::families::b2::records::b2_linked_counted_owners(&bytes);
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].link.target, 916);
    assert_eq!(linked[0].owner.references.last(), Some(&917));

    let mut wrong_successor = bytes;
    wrong_successor[35] = 0x99;
    assert!(crate::families::b2::records::b2_linked_counted_owners(&wrong_successor).is_empty());
}

#[test]
fn b2_cone_face_parser_reads_refs_scale_and_half_angle() {
    let records = crate::families::b2::records::b2_cone_faces(&b2_cone_face_stream());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].references.len(), 16);
    assert_eq!(records[0].angular_scale, 1.5);
    assert_eq!(records[0].half_angle, std::f64::consts::FRAC_PI_4);
}

#[test]
fn b2_topology_metadata_parser_preserves_refs_and_sense_code() {
    use crate::families::b2::records::B2UseSense;

    let bytes = b2_topology_metadata_stream();
    let edges = crate::families::b2::records::b2_edge_metadata(&bytes);
    let uses = crate::families::b2::records::b2_use_metadata(&bytes);
    assert_eq!(edges[0].references, vec![0x1234, 0x5678]);
    assert_eq!(edges[0].payload, [0x0a, 0x34, 0x12, 0x0a, 0x78, 0x56, 0]);
    assert_eq!(uses[0].sense, Some(B2UseSense::Sense88));
    assert!(uses[0].references.is_none());
    assert_eq!(uses[0].payload, [1, 2, 3, 0x88]);
}

#[test]
fn b2_edge_node_parser_reads_compact_native_vertex_identities() {
    let nodes = crate::families::b2::records::b2_edge_nodes(&b2_edge_node_stream());
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].header_token, 5);
    assert_eq!(nodes[0].curve_ref, 216);
    assert_eq!(nodes[0].start_vertex_ref, 889);
    assert_eq!(nodes[0].end_vertex_ref, 895);
    assert_eq!(nodes[0].start_parameter_ref, 215);
    assert_eq!(nodes[0].end_parameter_ref, 214);
    assert_eq!(nodes[0].tail, 0x21);
}

#[test]
fn b2_edge_node_parser_reads_tagged_and_raw_vertex_identities() {
    let mut bytes = vec![
        0xb2, 0x03, 0x5e, 0x09, 0x05, 0x0d, 0x06, 0x8b, 0x0a, 0xc1, 0x01, 0x09, 0x05, 0x01,
    ];
    bytes.extend_from_slice(&[
        0xb2, 0x03, 0x5e, 0x06, 0x05, 0x0d, 0xcf, 0xe7, 0x09, 0x05, 0x01,
    ]);
    let nodes = crate::families::b2::records::b2_edge_nodes(&bytes);
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].curve_ref, 3);
    assert_eq!(nodes[0].start_vertex_ref, 139);
    assert_eq!(nodes[0].end_vertex_ref, 449);
    assert_eq!(nodes[0].start_parameter_ref, 2);
    assert_eq!(nodes[0].end_parameter_ref, 1);
    assert_eq!(nodes[0].tail, 0x01);
    assert_eq!(nodes[1].start_vertex_ref, 207);
    assert_eq!(nodes[1].end_vertex_ref, 231);
}

#[test]
fn b2_revolution_parser_reads_axis_profile_bounds_and_exact_scale_relations() {
    for reference_token in [0x08, 0x0a] {
        let mut stream = b2_revolution_stream();
        stream[5] = reference_token;
        let records = crate::families::b2::records::b2_revolutions(&stream);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].profile_curve_id, 0x1234);
        assert_eq!(records[0].origin, [1.0, 2.0, 3.0]);
        assert_eq!(records[0].axis, [0.0, 0.0, 1.0]);
        assert_eq!(records[0].profile_range, [-4.0, 9.0]);
    }
}

#[test]
fn b2_group_parser_reads_separator_and_typed_opener() {
    let bytes = b2_group_stream();
    let separators = crate::families::b2::records::b2_group_separators(&bytes);
    let groups = crate::families::b2::records::b2_groups(&bytes);
    assert_eq!(separators.len(), 1);
    assert_eq!(separators[0].token, 0x05);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].group_id, 32);
    assert_eq!(groups[0].group_type, 3);
}

#[test]
fn b2_offset_support_parser_reads_carrier_distance_and_domain() {
    let offsets = crate::families::b2::records::b2_offset_supports(&b2_offset_support_stream());
    assert_eq!(offsets.len(), 1);
    assert_eq!(offsets[0].support_id, 0x1234);
    assert_eq!(offsets[0].distance, 2.5);
    assert_eq!(offsets[0].domain, [0.0, -1.0, 4.0, 3.0]);
}

#[test]
fn consolidated_offset_support_parser_reads_width2_frame() {
    let offsets = crate::families::b2::records::b2_offset_supports(&b3_offset_support_stream());
    assert_eq!(offsets.len(), 1);
    assert_eq!(offsets[0].support_id, 0x1234);
    assert_eq!(offsets[0].distance, 2.5);
}

#[test]
fn b2_edge_parameter_parser_validates_repeated_range_packet() {
    let packets = crate::families::b2::records::b2_edge_parameters(&b2_edge_parameter_stream());
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].range, [2.0, 7.0]);
    assert_eq!(packets[0].tolerance, 1e-6);
}

#[test]
fn b2_circle_parser_reads_arc_length_parameterization() {
    let circles = crate::families::b2::records::b2_circles(&b2_circle_stream());
    assert_eq!(circles.len(), 1);
    assert_eq!(circles[0].record_id, 0x1234);
    assert_eq!(circles[0].center_pair, [4.0, -2.0]);
    assert_eq!(circles[0].radius, 3.0);
    assert!(circles[0].full_circle);
}

#[test]
fn b2_cylinder_parser_reads_arc_length_carrier() {
    let cylinders = crate::families::b2::records::b2_cylinders(&b2_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].u_range, [0.0, 4.0 * std::f64::consts::PI]);
    assert_eq!(cylinders[0].v_range, [-4.0, 5.0]);
    match &cylinders[0].geometry {
        Some(SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        }) => {
            assert_eq!([origin.x, origin.y, origin.z], [1.0, 2.0, 3.0]);
            assert_eq!([axis.x, axis.y, axis.z], [1.0, 0.0, 0.0]);
            assert_eq!(*radius, 2.0);
        }
        other => panic!("expected cylinder, got {other:?}"),
    }
}

#[test]
fn consolidated_cylinder_parser_reads_width2_frame() {
    let cylinders = crate::families::b2::records::b2_cylinders(&b3_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].layout, 0x5a);
    assert!(cylinders[0].geometry.is_some());
}

#[test]
fn consolidated_frame_width_and_flag_are_independent() {
    let mut width1_flag13 = b2_cylinder_stream();
    width1_flag13[1] = 0x13;
    let mut width2_flag83 = b3_cylinder_stream();
    width2_flag83[1] = 0x83;
    assert_eq!(
        crate::families::b2::records::b2_cylinders(&width1_flag13).len(),
        1
    );
    assert_eq!(
        crate::families::b2::records::b2_cylinders(&width2_flag83).len(),
        1
    );
}

#[test]
fn b2_cylinder_parser_reads_implicit_axis_layout() {
    let cylinders = crate::families::b2::records::b2_cylinders(&b2_implicit_axis_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].layout, 0x52);
    assert!(matches!(
        cylinders[0].geometry,
        Some(SurfaceGeometry::Cylinder { axis, .. }) if [axis.x, axis.y, axis.z] == [1.0, 0.0, 0.0]
    ));
}

#[test]
fn b2_cylinder_parser_preserves_phase_tailed_layout_raw() {
    let cylinders = crate::families::b2::records::b2_cylinders(&b2_phase_tailed_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].layout, 0x62);
    assert!(cylinders[0].geometry.is_none());
    assert_eq!(cylinders[0].stored_vector, Some([0.0, 1.0]));
    assert_eq!(cylinders[0].phase, Some(0.75));

    for range in [30..38, 46..54, 95..103] {
        let mut malformed = b2_phase_tailed_cylinder_stream();
        malformed[range].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(crate::families::b2::records::b2_cylinders(&malformed).is_empty());
    }
}

#[test]
fn b2_cone_parser_reads_orthonormal_slant_chart() {
    let cones = crate::families::b2::records::b2_cones(&b2_cone_stream());
    assert_eq!(cones.len(), 1);
    assert_eq!(cones[0].apex, [1.0, 2.0, 3.0]);
    assert_eq!(cones[0].axis, [0.0, 0.0, 1.0]);
    assert_eq!(cones[0].half_angle, 0.25);
    assert_eq!(cones[0].slant_range, [2.0, 8.0]);
    assert_eq!(cones[0].angular_scale, 3.0);
}

#[test]
fn b2_construction_use_parser_reorders_offset_domain() {
    let uses = crate::families::b2::records::b2_construction_uses(&b2_construction_use_stream());
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].support_id, 0x1234);
    assert_eq!(uses[0].distance, -2.0);
    assert_eq!(uses[0].kind, 0x01);
    assert_eq!(uses[0].domain, Some([0.0, -1.0, 4.0, 3.0]));
    let offsets = crate::families::b2::records::b2_offset_supports(&b2_construction_use_stream());
    assert_eq!(offsets.len(), 1);
    assert_eq!(offsets[0].support_id, 0x1234);
    assert_eq!(offsets[0].distance, -2.0);
    assert_eq!(offsets[0].domain, [0.0, -1.0, 4.0, 3.0]);
}

#[test]
fn b2_offset_support_parser_ignores_other_construction_kinds() {
    let mut record = b2_construction_use_stream();
    record[17] = 0x19;

    let uses = crate::families::b2::records::b2_construction_uses(&record);
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].kind, 0x19);
    assert!(crate::families::b2::records::b2_offset_supports(&record).is_empty());
}

#[test]
fn b2_composite_parser_reads_embedded_cylinder_frame() {
    let cylinders =
        crate::families::b2::records::b2_embedded_cylinders(&b2_embedded_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].object_id, 0x5678);
    assert_eq!(cylinders[0].wrapper_pos, 0);
    assert_eq!(
        cylinders[0].cylinder.u_range,
        [0.0, 4.0 * std::f64::consts::PI]
    );
}
