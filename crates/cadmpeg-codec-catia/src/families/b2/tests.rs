// SPDX-License-Identifier: Apache-2.0
//! Record-decoder tests for the `b2` family over synthetic byte fixtures.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::SurfaceGeometry;

use crate::test_support::*;
use crate::variant::Variant;
use crate::CatiaCodec;

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
    use crate::families::b2::records::B2ParameterPointPayload;

    let points = crate::families::b2::records::b2_parameter_points(&b2_parameter_point_stream());
    assert_eq!(points.len(), 4);
    assert_eq!(
        points.iter().map(|point| point.prefix).collect::<Vec<_>>(),
        [0x05, 0x09, 0x0d, 0x11]
    );
    assert!(matches!(
        &points[0].payload,
        B2ParameterPointPayload::Uv { uv: [2.0, 3.0] }
    ));
    assert!(matches!(
        &points[1].payload,
        B2ParameterPointPayload::StationUv {
            station: 11.0,
            uv: [4.0, 5.0],
        }
    ));
    assert!(matches!(
        &points[2].payload,
        B2ParameterPointPayload::FiveScalars { .. }
    ));
    assert!(matches!(
        &points[3].payload,
        B2ParameterPointPayload::StationUv {
            station: 12.0,
            uv: [6.0, 7.0],
        }
    ));
}

#[test]
fn b2_plane_carrier_parser_preserves_each_selector_layout() {
    use crate::families::b2::records::B2PlaneCarrierPayload;

    let carriers = crate::families::b2::records::b2_plane_carriers(&b2_plane_carrier_stream());
    assert_eq!(carriers.len(), 3);
    assert_eq!(
        carriers
            .iter()
            .map(|carrier| carrier.selector)
            .collect::<Vec<_>>(),
        [0xe4, 0xc4, 0xec]
    );
    assert!(matches!(
        &carriers[0].payload,
        B2PlaneCarrierPayload::PointDirection2 {
            point: [10.0, 20.0],
            direction: [1.0, 0.0],
            tail: [5.0, -2.0, 3.0],
        }
    ));
    assert!(matches!(
        &carriers[1].payload,
        B2PlaneCarrierPayload::PointDirection3 {
            point: [10.0, 20.0],
            direction: [1.0, 0.0, 0.0],
            tail: [5.0, -2.0, 3.0],
        }
    ));
    assert!(matches!(
        &carriers[2].payload,
        B2PlaneCarrierPayload::PointTail {
            point: [10.0, 20.0],
            tail: [-2.0, 5.0, -2.0, 3.0],
        }
    ));
    assert_eq!(carriers[0].end - carriers[0].pos, 63);
}

#[test]
fn b2_plane_carrier_parser_retains_unclassified_scalar_lanes() {
    use crate::families::b2::records::B2PlaneCarrierPayload;

    let mut stream = b2_plane_carrier_stream();
    let values = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
    stream.extend_from_slice(&[
        0xb2,
        0x03,
        0x27,
        2 + u8::try_from(values.len() * 8).expect("scalar lane fixture"),
        0x05,
        0xb4,
        0x40,
    ]);
    for value in values {
        stream.extend_from_slice(&crate::test_support::le_f64(value));
    }

    let carriers = crate::families::b2::records::b2_plane_carriers(&stream);
    assert_eq!(carriers.len(), 4);
    assert_eq!(carriers[3].selector, 0x40);
    assert!(matches!(
        &carriers[3].payload,
        B2PlaneCarrierPayload::ScalarLane { values: lane } if lane == &values
    ));
    assert!(crate::families::b2::records::b2_plane_geometry(&carriers[3]).is_none());
}

#[test]
fn b2_plane_carrier_parser_rejects_open_or_nonfinite_layouts() {
    let valid = b2_plane_carrier_stream();
    let mut invalid_marker = valid.clone();
    invalid_marker[5] = 0xb5;
    assert_eq!(
        crate::families::b2::records::b2_plane_carriers(&invalid_marker).len(),
        2
    );

    let mut invalid_selector = valid.clone();
    invalid_selector[6] = 0xc4;
    assert_eq!(
        crate::families::b2::records::b2_plane_carriers(&invalid_selector).len(),
        2
    );

    let mut invalid_flag = valid.clone();
    invalid_flag[1] = 0x04;
    assert_eq!(
        crate::families::b2::records::b2_plane_carriers(&invalid_flag).len(),
        2
    );

    let mut invalid_scalar = valid;
    invalid_scalar[7..15].copy_from_slice(&f64::NAN.to_le_bytes());
    assert_eq!(
        crate::families::b2::records::b2_plane_carriers(&invalid_scalar).len(),
        2
    );
}

#[test]
fn b2_plane_geometry_uses_direction_bearing_layouts_only() {
    use crate::families::b2::records::B2PlaneCarrierPayload;

    let carriers = crate::families::b2::records::b2_plane_carriers(&b2_plane_carrier_stream());
    let geometry =
        crate::families::b2::records::b2_plane_geometry(&carriers[0]).expect("e4 plane geometry");
    let cadmpeg_ir::geometry::SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = geometry
    else {
        panic!("plane carrier geometry")
    };
    assert_eq!(origin, cadmpeg_ir::math::Point3::new(10.0, 20.0, 0.0));
    assert_eq!(u_axis, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(normal, cadmpeg_ir::math::Vector3::new(0.0, -1.0, 0.0));
    assert!(crate::families::b2::records::b2_plane_geometry(&carriers[1]).is_some());
    assert!(matches!(
        &carriers[2].payload,
        B2PlaneCarrierPayload::PointTail { .. }
    ));
    assert!(crate::families::b2::records::b2_plane_geometry(&carriers[2]).is_none());
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
    assert_eq!(packets[0].numeric_tail.lower, [-0.0, 4.5]);
    assert_eq!(packets[0].numeric_tail.upper, [12.25, 7.0]);
    assert_eq!(
        packets[0].numeric_tail.bounds,
        [[-2.0, 1.0], [3.5, 4.0], [5.25, 6.0]]
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

    let packets =
        crate::families::b2::records::b2_owner_packets(&b2_all_compact_owner_packet_stream());
    assert_eq!(packets.len(), 1);
    assert_eq!(
        packets[0].reference_encoding,
        B2OwnerReferenceEncoding::AllCompact
    );
    assert_eq!(
        packets[0].references,
        [278, 324, 276, 268, 277, 374, 199, 195, 279]
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
        (5, 13.0f64.to_le_bytes().to_vec()),
        (38, f32::INFINITY.to_le_bytes().to_vec()),
        (38, 2.0f32.to_le_bytes().to_vec()),
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
fn b2_face_node_5f_parser_accepts_each_compact_target_width_and_fixed_tail() {
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
    let nodes = crate::families::b2::records::b2_face_nodes_5f(&bytes);
    assert_eq!(nodes.len(), 4);
    assert!(nodes.iter().all(|node| node.header_token == 5));
    assert_eq!(
        nodes.iter().map(|node| node.target).collect::<Vec<_>>(),
        [0x5d, 0x025d, 0x0001_025d, 0x0101_025d]
    );

    let malformed = [
        0xb2, 0x03, 0x5f, 0x06, 0x05, 0x82, 0x04, 0x5d, 0x00, 0x03, 0x05,
    ];
    assert!(crate::families::b2::records::b2_face_nodes_5f(&malformed).is_empty());
}

#[test]
fn b2_face_node_5f_parser_retains_strong_targets_and_terminal_pairs() {
    use crate::families::b2::records::B2FaceNode5fTargetEncoding;

    let mut bytes = vec![
        0xb2, 0x03, 0x5f, 0x06, 0x05, 0x82, 0x0a, 0x34, 0x12, 0x03, 0x05,
    ];
    bytes.extend_from_slice(&[
        0xb2, 0x03, 0x5f, 0x06, 0x05, 0x82, 0x0a, 0x78, 0x56, 0x0f, 0x05,
    ]);

    let nodes = crate::families::b2::records::b2_face_nodes_5f(&bytes);
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].target, 0x1234);
    assert_eq!(
        nodes[0].target_encoding,
        B2FaceNode5fTargetEncoding::TaggedU16Strong
    );
    assert_eq!(nodes[0].terminal, [0x03, 0x05]);
    assert_eq!(nodes[1].target, 0x5678);
    assert_eq!(nodes[1].terminal, [0x0f, 0x05]);

    let mut owner_stream = vec![
        0xb2, 0x03, 0x5f, 0x06, 0x05, 0x82, 0x0a, 0xeb, 0x03, 0x03, 0x05,
    ];
    owner_stream.extend_from_slice(&b2_owner_packet_stream());
    let related = crate::families::b2::records::b2_adjacent_face_owners(&owner_stream);
    assert_eq!(related.len(), 1);
    assert_eq!(
        related[0].face_node.target_encoding,
        B2FaceNode5fTargetEncoding::TaggedU16Strong
    );
}

#[test]
fn b2_adjacent_face_owner_requires_adjacency_and_successor_identity() {
    let pairs =
        crate::families::b2::records::b2_adjacent_face_owners(&b2_adjacent_face_owner_stream());
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].face_node.target, 1003);
    assert_eq!(pairs[0].owner.references[8], 1004);

    let mut separated = b2_face_node_5f_stream();
    separated.extend_from_slice(&[0xb2, 0x03, 0x2e, 0x01, 0x05, 0x05]);
    separated.extend_from_slice(&b2_owner_packet_stream());
    assert!(crate::families::b2::records::b2_adjacent_face_owners(&separated).is_empty());
}

#[test]
fn b2_secondary_face_node_terminal_requires_all_compact_owner() {
    let secondary = b2_adjacent_secondary_face_owner_stream();
    let pairs = crate::families::b2::records::b2_adjacent_face_owners(&secondary);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].face_node.terminal, [0x03, 0x03]);
    assert_eq!(pairs[0].face_node.target, 278);
    assert_eq!(pairs[0].owner.references[8], 279);

    let owner_start = secondary
        .windows(3)
        .position(|window| window == [0xb2, 0x03, 0x62])
        .expect("owner frame");
    let mut tagged = b2_adjacent_face_owner_stream();
    let tagged_owner_start = tagged
        .windows(3)
        .position(|window| window == [0xb2, 0x03, 0x62])
        .expect("tagged owner frame");
    tagged[tagged_owner_start - 1] = 0x03;
    assert!(crate::families::b2::records::b2_adjacent_face_owners(&tagged).is_empty());

    let mut unknown_terminal = secondary;
    unknown_terminal[owner_start - 1] = 0x07;
    assert!(crate::families::b2::records::b2_adjacent_face_owners(&unknown_terminal).is_empty());
}

#[test]
fn b2_counted_owner_closes_variable_reference_lane_and_face_node_relation() {
    let bytes = b2_adjacent_face_counted_owner_stream();
    let owners = crate::families::b2::records::b2_counted_owners(&bytes);
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].references, [911, 7, 263, 258, 281, 276, 917]);
    assert_eq!(owners[0].tail, [0x83, 0x41, 0x92, 0x00, 0x01]);

    let related = crate::families::b2::records::b2_adjacent_face_counted_owners(&bytes);
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].face_node.target, 916);
    assert_eq!(related[0].owner.references.last(), Some(&917));

    let mut wrong_successor = bytes;
    wrong_successor[35] = 0x99;
    assert!(
        crate::families::b2::records::b2_adjacent_face_counted_owners(&wrong_successor).is_empty()
    );
}

#[test]
fn b2_cone_face_parser_reads_program_scale_and_half_angle() {
    let records = crate::families::b2::records::b2_cone_faces(&b2_cone_face_stream());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].program.len(), 16);
    assert_eq!(records[0].angular_scale, 1.5);
    assert_eq!(records[0].half_angle, std::f64::consts::FRAC_PI_4);

    let mut degenerate = b2_cone_face_stream();
    let half_angle = degenerate.len() - 8;
    degenerate[half_angle..].copy_from_slice(&0.0_f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_cone_faces(&degenerate).is_empty());
}

#[test]
fn b2_cone_face_parser_reads_a_complete_nested_frame() {
    let nested = b2_cone_face_stream();
    let mut bytes = vec![0xa5, 0x03, 0x7f];
    bytes.extend_from_slice(
        &u32::try_from(nested.len())
            .expect("bounded nested frame")
            .to_le_bytes(),
    );
    bytes.push(0x05);
    bytes.extend(nested);
    let records = crate::families::b2::records::b2_cone_faces(&bytes);
    let [record] = records.as_slice() else {
        panic!("one nested cone-face chart")
    };
    assert_eq!(record.pos, 8);
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
        assert_eq!(records[0].pos, 0);
        assert_eq!(records[0].reference_token, reference_token);
        assert_eq!(records[0].profile_allocation_id, 0x1234);
        assert_eq!(records[0].origin, [1.0, 2.0, 3.0]);
        assert_eq!(records[0].direction_x, [1.0, 0.0, 0.0]);
        assert_eq!(records[0].direction_y, [0.0, 1.0, 0.0]);
        assert_eq!(records[0].axis, [0.0, 0.0, 1.0]);
        assert_eq!(
            records[0].angular_range,
            [2.0 * 0.5, 2.0 * (0.5 + std::f64::consts::TAU)]
        );
        assert_eq!(records[0].profile_range, [-4.0, 9.0]);
        assert_eq!(records[0].angular_scale, 2.0);
    }
}

#[test]
fn b2_revolution_profile_requires_one_exact_circle_interval() {
    let stream = b2_resolved_revolution_stream();
    let resolved = crate::families::b2::records::b2_resolved_revolutions(&stream);
    let [resolved] = resolved.as_slice() else {
        panic!("one resolved revolution profile")
    };
    assert_eq!(resolved.revolution.profile_range, resolved.profile.range);
    assert_eq!(resolved.revolution_index, 0);

    let mut unmatched_prefix = b2_revolution_stream();
    unmatched_prefix[120..128].copy_from_slice(&(-5.0f64).to_le_bytes());
    unmatched_prefix.extend_from_slice(&stream);
    let resolved = crate::families::b2::records::b2_resolved_revolutions(&unmatched_prefix);
    let [resolved] = resolved.as_slice() else {
        panic!("one resolved revolution after unmatched prefix")
    };
    assert_eq!(resolved.revolution_index, 1);

    let mut ambiguous = stream.clone();
    ambiguous.splice(0..0, stream[..57].iter().copied());
    assert!(crate::families::b2::records::b2_resolved_revolutions(&ambiguous).is_empty());

    let mut mismatched = stream;
    mismatched[40..48].copy_from_slice(&10.0f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_resolved_revolutions(&mismatched).is_empty());

    let mut signed_zero_mismatch = b2_resolved_revolution_stream();
    signed_zero_mismatch[32..40].copy_from_slice(&0.0f64.to_le_bytes());
    signed_zero_mismatch[177..185].copy_from_slice(&(-0.0f64).to_le_bytes());
    assert!(
        crate::families::b2::records::b2_resolved_revolutions(&signed_zero_mismatch).is_empty()
    );
}

#[test]
fn b2_revolution_profile_identity_disambiguates_equal_intervals() {
    let mut first = b2_circle_stream();
    first[6..8].copy_from_slice(&0x9999u16.to_le_bytes());
    first[32..40].copy_from_slice(&(-4.0f64).to_le_bytes());
    first[40..48].copy_from_slice(&9.0f64.to_le_bytes());
    let mut second = b2_circle_stream();
    second[32..40].copy_from_slice(&(-4.0f64).to_le_bytes());
    second[40..48].copy_from_slice(&9.0f64.to_le_bytes());
    let second_pos = first.len();
    let mut stream = first;
    stream.extend_from_slice(&second);
    stream.extend_from_slice(&b2_revolution_stream());

    let resolved_revolutions = crate::families::b2::records::b2_resolved_revolutions(&stream);
    let [resolved] = resolved_revolutions.as_slice() else {
        panic!("one identity-bound revolution profile");
    };
    assert_eq!(resolved.profile.record_id, 0x1234);
    assert_eq!(resolved.profile.pos, second_pos);
}

#[test]
fn b2_revolution_profile_identity_mismatch_does_not_fall_back_to_interval() {
    let mut identity_mismatch = b2_circle_stream();
    identity_mismatch[32..40].copy_from_slice(&10.0f64.to_le_bytes());
    identity_mismatch[40..48].copy_from_slice(&20.0f64.to_le_bytes());
    let mut interval_match = b2_circle_stream();
    interval_match[6..8].copy_from_slice(&0x9999u16.to_le_bytes());
    interval_match[32..40].copy_from_slice(&(-4.0f64).to_le_bytes());
    interval_match[40..48].copy_from_slice(&9.0f64.to_le_bytes());
    let mut stream = identity_mismatch;
    stream.extend_from_slice(&interval_match);
    stream.extend_from_slice(&b2_revolution_stream());

    assert!(crate::families::b2::records::b2_resolved_revolutions(&stream).is_empty());
}

#[test]
fn b2_line_profile_parser_reads_exact_origin_direction_and_range() {
    let b2 = b2_line_profile_stream();
    for (family, header) in [
        (0xb2, vec![0x05]),
        (0xb3, vec![0x05, 0x00]),
        (0xb4, vec![0x05, 0x00, 0x00]),
    ] {
        let mut stream = vec![family, 0x03, 0x0e, 0x48];
        stream.extend(header);
        stream.extend_from_slice(&b2[5..]);
        let records = crate::families::b2::records::b2_line_profiles(&stream);
        let [line] = records.as_slice() else {
            panic!("one B-family line profile")
        };
        assert_eq!(line.pos, 0);
        assert_eq!(line.origin, [1.0, 2.0, 3.0]);
        assert_eq!(line.direction, [0.0, 0.6, 0.8]);
        assert_eq!(line.range, [-4.0, 9.0]);
    }
}

#[test]
fn b2_line_profile_parser_requires_its_complete_fixed_metric_grammar() {
    let valid = b2_line_profile_stream();
    for (offset, bytes) in [
        (3, vec![0x50]),
        (5 + 3 * 8, 2.0f64.to_le_bytes().to_vec()),
        (5 + 6 * 8, 0.0f64.to_le_bytes().to_vec()),
        (5 + 6 * 8, 2.5f64.to_le_bytes().to_vec()),
        (5 + 7 * 8, 10.0f64.to_le_bytes().to_vec()),
        (5, f64::NAN.to_le_bytes().to_vec()),
    ] {
        let mut invalid = valid.clone();
        invalid.splice(offset..offset + bytes.len(), bytes);
        assert!(crate::families::b2::records::b2_line_profiles(&invalid).is_empty());
    }
}

#[test]
fn b2_revolution_parser_requires_an_ordered_profile_and_right_handed_unit_frame() {
    let mut stream = b2_revolution_stream();
    stream[6..8].fill(0);
    assert!(crate::families::b2::records::b2_revolutions(&stream).is_empty());

    let mut stream = b2_revolution_stream();
    stream[5 + 3 + 8 * 3..5 + 3 + 8 * 3 + 8].copy_from_slice(&2.0f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_revolutions(&stream).is_empty());

    let mut stream = b2_revolution_stream();
    stream[5 + 3 + 8 * 6..5 + 3 + 8 * 6 + 8].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert!(crate::families::b2::records::b2_revolutions(&stream).is_empty());

    let mut stream = b2_revolution_stream();
    let start = 5 + 99 + 2 * 8;
    let bounds = [9.0f64, -4.0];
    for (index, value) in bounds.into_iter().enumerate() {
        stream[start + 8 * index..start + 8 * (index + 1)].copy_from_slice(&value.to_le_bytes());
    }
    assert!(crate::families::b2::records::b2_revolutions(&stream).is_empty());
}

#[test]
fn b2_torus_parser_reads_exact_frame_radii_and_parameter_scales() {
    let records = crate::families::b2::records::b2_tori(&b2_torus_stream());
    let [torus] = records.as_slice() else {
        panic!("one B2 torus")
    };
    assert_eq!(torus.pos, 0);
    assert_eq!(torus.center, [1.0, 2.0, 3.0]);
    assert_eq!(torus.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(torus.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(torus.axis, [0.0, 0.0, 1.0]);
    assert_eq!(torus.major_radius, 7.0);
    assert_eq!(torus.minor_radius, 2.0);
    assert_eq!(
        torus.major_angular_range,
        [
            std::f64::consts::FRAC_PI_2,
            3.0 * std::f64::consts::FRAC_PI_2
        ]
    );
    assert_eq!(torus.major_angular_domain, [0.0, std::f64::consts::TAU]);
    assert_eq!(torus.minor_angular_range, [0.0, std::f64::consts::PI]);
    assert_eq!(
        torus.minor_angular_domain,
        [
            -std::f64::consts::FRAC_PI_2,
            3.0 * std::f64::consts::FRAC_PI_2
        ]
    );
    assert_eq!(torus.major_scale, 14.0);
    assert_eq!(torus.minor_scale, 4.0);
}

#[test]
fn indexed_analytic_carrier_decoders_match_one_shot_wrappers() {
    let cases = [
        ("cone", b2_cone_stream()),
        ("sphere", b2_sphere_stream()),
        ("torus", b2_torus_stream()),
        ("cylinder", b2_cylinder_stream()),
    ];
    for (name, bytes) in cases {
        let consolidated = crate::wire::records::consolidated_records(&bytes);
        let expected = match name {
            "cone" => crate::families::b2::records::b2_cones(&bytes)
                .into_iter()
                .map(|record| record.pos)
                .collect::<Vec<_>>(),
            "sphere" => crate::families::b2::records::b2_spheres(&bytes)
                .into_iter()
                .map(|record| record.pos)
                .collect::<Vec<_>>(),
            "torus" => crate::families::b2::records::b2_tori(&bytes)
                .into_iter()
                .map(|record| record.pos)
                .collect::<Vec<_>>(),
            "cylinder" => crate::families::b2::records::b2_cylinders(&bytes)
                .into_iter()
                .map(|record| record.pos)
                .collect::<Vec<_>>(),
            _ => unreachable!("unknown analytic carrier fixture: {name}"),
        };
        let actual = match name {
            "cone" => crate::families::b2::records::b2_cones_from_records(&bytes, &consolidated)
                .into_iter()
                .map(|record| record.pos)
                .collect::<Vec<_>>(),
            "sphere" => {
                crate::families::b2::records::b2_spheres_from_records(&bytes, &consolidated)
                    .into_iter()
                    .map(|record| record.pos)
                    .collect::<Vec<_>>()
            }
            "torus" => crate::families::b2::records::b2_tori_from_records(&bytes, &consolidated)
                .into_iter()
                .map(|record| record.pos)
                .collect::<Vec<_>>(),
            "cylinder" => {
                crate::families::b2::records::b2_cylinders_from_records(&bytes, &consolidated)
                    .into_iter()
                    .map(|record| record.pos)
                    .collect::<Vec<_>>()
            }
            _ => unreachable!("unknown analytic carrier fixture: {name}"),
        };
        assert_eq!(actual, expected, "carrier decoder changed for {name}");
    }

    let bytes = b2_resolved_revolution_stream();
    let consolidated = crate::wire::records::consolidated_records(&bytes);
    assert_eq!(
        crate::families::b2::records::b2_resolved_revolutions_from_records(&bytes, &consolidated,),
        crate::families::b2::records::b2_resolved_revolutions(&bytes)
    );
}

#[test]
fn indexed_native_record_decoders_match_one_shot_wrappers() {
    let compare = |name: &str, one_shot: Vec<usize>, indexed: Vec<usize>| {
        assert_eq!(indexed, one_shot, "indexed decoder changed for {name}");
    };

    let bytes = b2_reference_list_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "reference lists",
        crate::families::b2::records::b2_reference_lists(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_reference_lists_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_owner_packet_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "owner packets",
        crate::families::b2::records::b2_owner_packets(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_owner_packets_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_width_coded_owner_packet_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "width-coded owner packets",
        crate::families::b2::records::b2_owner_packets(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_owner_packets_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_counted_61_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "counted class 61",
        crate::families::b2::records::b2_counted_61(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_counted_61_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_long_61_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "long class 61",
        crate::families::b2::records::b2_long_61(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_long_61_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_face_node_5f_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "class 5f face nodes",
        crate::families::b2::records::b2_face_nodes_5f(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_face_nodes_5f_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_adjacent_face_owner_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_eq!(
        crate::families::b2::records::b2_adjacent_face_owners_from_records(&bytes, &records),
        crate::families::b2::records::b2_adjacent_face_owners(&bytes)
    );

    let bytes = b2_adjacent_face_counted_owner_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_eq!(
        crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(
            &bytes, &records
        ),
        crate::families::b2::records::b2_adjacent_face_counted_owners(&bytes)
    );

    let bytes = b2_parameter_point_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "parameter points",
        crate::families::b2::records::b2_parameter_points(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_parameter_points_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_line_profile_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "line profiles",
        crate::families::b2::records::b2_line_profiles(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_line_profiles_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_spatial_circle_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "spatial circles",
        crate::families::b2::records::b2_spatial_circles(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_spatial_circles_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_nurbs_curve_stream([1.0, 0.72, 1.31, 0.93]);
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "NURBS curves",
        crate::families::b2::records::b2_nurbs_curves(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_nurbs_curves_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_construction_use_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let construction_signature = |record: &crate::families::b2::records::B2ConstructionUse| {
        (
            record.pos,
            record.support_id,
            record.distance,
            record.kind,
            record.domain,
        )
    };
    assert_eq!(
        crate::families::b2::records::b2_construction_uses(&bytes)
            .iter()
            .map(construction_signature)
            .collect::<Vec<_>>(),
        crate::families::b2::records::b2_construction_uses_from_records(&bytes, &records)
            .iter()
            .map(construction_signature)
            .collect::<Vec<_>>()
    );
    let offset_signature = |record: &crate::families::b2::records::B2OffsetSupport| {
        (
            record.pos,
            record.support_id,
            record.distance,
            record.domain,
        )
    };
    assert_eq!(
        crate::families::b2::records::b2_offset_supports(&bytes)
            .iter()
            .map(offset_signature)
            .collect::<Vec<_>>(),
        crate::families::b2::records::b2_offset_supports_from_records(&bytes, &records)
            .iter()
            .map(offset_signature)
            .collect::<Vec<_>>()
    );
}

#[test]
fn b2_torus_parser_rejects_invalid_frames_and_nonpositive_scales() {
    let mut stream = b2_torus_stream();
    stream[5 + 6 * 8..5 + 7 * 8].copy_from_slice(&1.0f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_tori(&stream).is_empty());

    let mut stream = b2_torus_stream();
    stream[5 + 23 * 8..5 + 24 * 8].copy_from_slice(&0.0f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_tori(&stream).is_empty());

    let mut stream = b2_torus_stream();
    stream[5 + 15 * 8..5 + 16 * 8].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::families::b2::records::b2_tori(&stream).is_empty());

    let mut stream = b2_torus_stream();
    stream[5 + 16 * 8..5 + 17 * 8].copy_from_slice(&std::f64::consts::FRAC_PI_4.to_le_bytes());
    assert!(crate::families::b2::records::b2_tori(&stream).is_empty());
}

#[test]
fn b2_sphere_parser_reads_radius_scaled_frame_and_active_ranges() {
    let records = crate::families::b2::records::b2_spheres(&b2_sphere_stream());
    let [sphere] = records.as_slice() else {
        panic!("one B2 sphere")
    };
    assert_eq!(sphere.pos, 0);
    assert_eq!(sphere.center, [1.0, 2.0, 3.0]);
    assert_eq!(sphere.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(sphere.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(sphere.axis, [0.0, 0.0, 1.0]);
    assert_eq!(sphere.radius, 5.0);
    assert_eq!(sphere.azimuth_range, [-2.0, 4.0]);
    assert_eq!(sphere.latitude_range, [-1.0, std::f64::consts::FRAC_PI_2]);
}

#[test]
fn b2_sphere_parser_validates_tiny_radius_scaled_frame() {
    let tiny = 1e-200_f64;
    let mut stream = b2_sphere_stream();
    for (index, value) in [tiny, 0.0, 0.0, 0.0, tiny, 0.0, 0.0, 0.0, tiny]
        .into_iter()
        .enumerate()
    {
        stream[5 + (3 + index) * 8..5 + (4 + index) * 8].copy_from_slice(&value.to_le_bytes());
    }
    stream[5 + 12 * 8..5 + 13 * 8].copy_from_slice(&tiny.to_le_bytes());
    stream[5 + 17 * 8..5 + 18 * 8].copy_from_slice(&tiny.to_le_bytes());
    let chart_origin = tiny * (1.0 - std::f64::consts::PI);
    stream[5 + 18 * 8..5 + 19 * 8].copy_from_slice(&chart_origin.to_le_bytes());
    let [sphere] = crate::families::b2::records::b2_spheres(&stream)
        .try_into()
        .expect("tiny sphere frame");
    assert_eq!(sphere.radius, tiny);
    assert_eq!(sphere.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(sphere.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(sphere.axis, [0.0, 0.0, 1.0]);

    stream[5 + 3 * 8..5 + 4 * 8].copy_from_slice(&(2.0 * tiny).to_le_bytes());
    assert!(crate::families::b2::records::b2_spheres(&stream).is_empty());
}

#[test]
fn b2_sphere_parser_rejects_invalid_scaled_frames_and_bounds() {
    let mut stream = b2_sphere_stream();
    stream[5 + 3 * 8..5 + 4 * 8].copy_from_slice(&4.0f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_spheres(&stream).is_empty());

    let mut stream = b2_sphere_stream();
    stream[5 + 14 * 8..5 + 15 * 8].copy_from_slice(&(-3.0f64).to_le_bytes());
    assert!(crate::families::b2::records::b2_spheres(&stream).is_empty());

    let mut stream = b2_sphere_stream();
    stream[5 + 18 * 8..5 + 19 * 8].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::families::b2::records::b2_spheres(&stream).is_empty());

    let mut stream = b2_sphere_stream();
    stream[5 + 17 * 8..5 + 18 * 8].copy_from_slice(&7.0f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_spheres(&stream).is_empty());

    let mut stream = b2_sphere_stream();
    stream[5 + 18 * 8..5 + 19 * 8].copy_from_slice(&0.25f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_spheres(&stream).is_empty());
}

#[test]
fn b2_group_parser_reads_separator_and_typed_opener() {
    let bytes = b2_group_stream();
    let separators = crate::families::b2::records::b2_group_separators(&bytes);
    let groups = crate::families::b2::records::b2_groups(&bytes);
    assert_eq!(separators.len(), 1);
    assert_eq!(separators[0].token, 0x05);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].group_type, 3);

    let mut invalid = bytes;
    invalid[14] = 0x85;
    assert!(crate::families::b2::records::b2_groups(&invalid).is_empty());
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
fn offset_support_binding_scales_each_nurbs_parameter_domain() {
    let tiny = 1e-200_f64;
    let mut carriers = crate::families::a5a8::records::a5_surfaces(&a5_surface_stream());
    let SurfaceGeometry::Nurbs(surface) = &mut carriers[0].geometry else {
        panic!("NURBS fixture");
    };
    for knots in [&mut surface.u_knots, &mut surface.v_knots] {
        let lower = knots[0];
        let span = knots.last().copied().expect("nonempty knots") - lower;
        for knot in knots {
            *knot = (*knot - lower) / span * tiny;
        }
    }
    let exact = crate::families::b2::records::B2OffsetSupport {
        pos: 0,
        support_id: 1,
        distance: tiny,
        domain: [0.0, 0.0, tiny, tiny],
    };
    assert_eq!(
        crate::families::b2::records::offset_support_carriers(
            std::slice::from_ref(&exact),
            &carriers
        ),
        [Some(0)]
    );

    let mut outside_u = exact.clone();
    outside_u.domain[2] = 2.0 * tiny;
    assert_eq!(
        crate::families::b2::records::offset_support_carriers(&[outside_u], &carriers),
        [None]
    );
    let mut outside_v = exact;
    outside_v.domain[3] = 2.0 * tiny;
    assert_eq!(
        crate::families::b2::records::offset_support_carriers(&[outside_v], &carriers),
        [None]
    );
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
fn b2_edge_parameter_parser_rejects_nonincreasing_ranges() {
    assert!(
        crate::families::b2::records::b2_edge_parameters(&b2_edge_parameter_stream_for(7.0, 2.0))
            .is_empty()
    );
    assert!(
        crate::families::b2::records::b2_edge_parameters(&b2_edge_parameter_stream_for(2.0, 2.0))
            .is_empty()
    );
}

#[test]
fn b2_circle_parser_reads_arc_length_parameterization() {
    let circles = crate::families::b2::records::b2_circles(&b2_circle_stream());
    assert_eq!(circles.len(), 1);
    assert_eq!(circles[0].record_id, 0x1234);
    assert_eq!(circles[0].center_pair, [4.0, -2.0]);
    assert_eq!(circles[0].radius, 3.0);
    assert_eq!(circles[0].chart_shift, 0.0);
    assert!(circles[0].full_circle);

    let mut malformed = b2_circle_stream();
    malformed[49..57].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::families::b2::records::b2_circles(&malformed).is_empty());

    let mut zero_radius = b2_circle_stream();
    zero_radius[24..32].copy_from_slice(&0.0_f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_circles(&zero_radius).is_empty());

    let mut large = b2_circle_stream();
    let radius = 2_000_000.0_f64;
    large[24..32].copy_from_slice(&radius.to_le_bytes());
    large[40..48].copy_from_slice(&(std::f64::consts::TAU * radius).to_le_bytes());
    assert_eq!(
        crate::families::b2::records::b2_circles(&large)[0].radius,
        radius
    );

    let tiny = 1e-200_f64;
    let mut tiny_full = b2_circle_stream();
    tiny_full[24..32].copy_from_slice(&tiny.to_le_bytes());
    tiny_full[40..48].copy_from_slice(&(std::f64::consts::TAU * tiny).to_le_bytes());
    assert!(crate::families::b2::records::b2_circles(&tiny_full)[0].full_circle);

    tiny_full[40..48].copy_from_slice(&1e-10_f64.to_le_bytes());
    assert!(!crate::families::b2::records::b2_circles(&tiny_full)[0].full_circle);
}

#[test]
fn b2_cylinder_parser_reads_arc_length_carrier() {
    let cylinders = crate::families::b2::records::b2_cylinders(&b2_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].u_range, [0.0, 4.0 * std::f64::consts::PI]);
    assert_eq!(cylinders[0].v_range, [-4.0, 5.0]);
    match &cylinders[0].geometry {
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } => {
            assert_eq!([origin.x, origin.y, origin.z], [1.0, 2.0, 3.0]);
            assert_eq!([axis.x, axis.y, axis.z], [1.0, 0.0, 0.0]);
            assert_eq!(*radius, 2.0);
        }
        other => panic!("expected cylinder, got {other:?}"),
    }

    for range in [5..13, 78..86] {
        let mut malformed = b2_cylinder_stream();
        malformed[range].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(crate::families::b2::records::b2_cylinders(&malformed).is_empty());
    }

    let mut large = b2_cylinder_stream();
    let radius = 2_000_000.0_f64;
    large[54..62].copy_from_slice(&radius.to_le_bytes());
    large[70..78].copy_from_slice(&(std::f64::consts::TAU * radius).to_le_bytes());
    assert!(matches!(
        crate::families::b2::records::b2_cylinders(&large)[0].geometry,
        SurfaceGeometry::Cylinder {
            radius: 2_000_000.0,
            ..
        }
    ));

    let tiny = 1e-200_f64;
    let mut tiny_full = b2_cylinder_stream();
    tiny_full[54..62].copy_from_slice(&tiny.to_le_bytes());
    tiny_full[70..78].copy_from_slice(&(std::f64::consts::TAU * tiny).to_le_bytes());
    assert_eq!(
        crate::families::b2::records::b2_cylinders(&tiny_full)[0].radius,
        tiny
    );

    tiny_full[70..78].copy_from_slice(&1e-10_f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_cylinders(&tiny_full).is_empty());
}

#[test]
fn analytic_point_lifts_bound_tiny_parameter_domains_by_span() {
    let tiny = 1e-200_f64;
    let mut cylinder = crate::families::b2::records::b2_cylinders(&b2_cylinder_stream()).remove(0);
    cylinder.u_range = [0.0, tiny];
    cylinder.v_range = [0.0, tiny];
    assert!(crate::families::b2::records::b2_cylinder_point(&cylinder, [tiny, tiny]).is_some());
    assert!(
        crate::families::b2::records::b2_cylinder_point(&cylinder, [2.0 * tiny, tiny]).is_none()
    );
    assert!(
        crate::families::b2::records::b2_cylinder_point(&cylinder, [tiny, 2.0 * tiny]).is_none()
    );

    let mut cone = crate::families::b2::records::b2_cones(&b2_cone_stream()).remove(0);
    cone.slant_range = [0.0, tiny];
    assert!(crate::families::b2::records::b2_cone_point(&cone, [0.0, tiny]).is_some());
    assert!(crate::families::b2::records::b2_cone_point(&cone, [0.0, 2.0 * tiny]).is_none());
}

#[test]
fn consolidated_cylinder_parser_reads_width2_frame() {
    let cylinders = crate::families::b2::records::b2_cylinders(&b3_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].layout, 0x5a);
    assert!(matches!(
        cylinders[0].geometry,
        SurfaceGeometry::Cylinder { .. }
    ));
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
        SurfaceGeometry::Cylinder { axis, .. } if [axis.x, axis.y, axis.z] == [1.0, 0.0, 0.0]
    ));

    let mut malformed = b2_implicit_axis_cylinder_stream();
    malformed[70..78].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::families::b2::records::b2_cylinders(&malformed).is_empty());
}

#[test]
fn b2_cylinder_parser_resolves_and_validates_partial_range_origin() {
    let cylinders = crate::families::b2::records::b2_cylinders(&b2_range_origin_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].layout, 0x62);
    assert!(matches!(
        cylinders[0].geometry,
        SurfaceGeometry::Cylinder {
            axis,
            ref_direction,
            ..
        } if [axis.x, axis.y, axis.z] == [0.0, 1.0, 0.0]
            && [ref_direction.x, ref_direction.y, ref_direction.z] == [0.0, 0.0, 1.0]
    ));
    assert_eq!(cylinders[0].stored_vector, Some([0.0, 1.0]));
    assert_eq!(
        cylinders[0].range_origin.map(f64::to_bits),
        Some(((0.0 + 8.0) * 0.5 - std::f64::consts::PI * 4.0).to_bits())
    );

    for range in [30..38, 46..54, 95..103] {
        let mut malformed = b2_range_origin_cylinder_stream();
        malformed[range].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(crate::families::b2::records::b2_cylinders(&malformed).is_empty());
    }
    let mut inconsistent = b2_range_origin_cylinder_stream();
    inconsistent[95..103].copy_from_slice(&0.0_f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_cylinders(&inconsistent).is_empty());
}

#[test]
fn b2_cone_parser_reads_orthonormal_slant_chart() {
    let cones = crate::families::b2::records::b2_cones(&b2_cone_stream());
    assert_eq!(cones.len(), 1);
    assert_eq!(cones[0].apex, [1.0, 2.0, 3.0]);
    assert_eq!(cones[0].axis, [0.0, 0.0, 1.0]);
    assert_eq!(cones[0].half_angle, 0.25);
    assert_eq!(cones[0].pre_angular_range_scalar, 4.0);
    assert_eq!(cones[0].angular_range, [0.5, 0.5 + std::f64::consts::PI]);
    assert_eq!(cones[0].slant_range, [2.0, 8.0]);
    assert_eq!(cones[0].angular_scale, 3.0);
    assert_eq!(
        cones[0].angular_domain,
        [
            0.5 - std::f64::consts::FRAC_PI_2,
            0.5 + 3.0 * std::f64::consts::FRAC_PI_2
        ]
    );

    let mut large = b2_cone_stream();
    large[141..149].copy_from_slice(&2_000_000.0_f64.to_le_bytes());
    large[149..157].copy_from_slice(&3_000_000.0_f64.to_le_bytes());
    let cones = crate::families::b2::records::b2_cones(&large);
    assert_eq!(cones[0].slant_range, [2.0, 2_000_000.0]);
    assert_eq!(cones[0].angular_scale, 3_000_000.0);
}

#[test]
fn b2_cone_parser_accepts_and_canonicalizes_an_apex_origin() {
    let mut stream = b2_cone_stream();
    stream[133..141].copy_from_slice(&(-5e-13f64).to_le_bytes());
    let cones = crate::families::b2::records::b2_cones(&stream);
    assert_eq!(cones.len(), 1);
    assert_eq!(cones[0].slant_range, [0.0, 8.0]);

    stream[133..141].copy_from_slice(&(-2e-12f64).to_le_bytes());
    assert!(crate::families::b2::records::b2_cones(&stream).is_empty());
}

#[test]
fn b2_cone_parser_rejects_a_left_handed_or_nonfinite_payload() {
    let mut stream = b2_cone_stream();
    stream[93..101].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert!(crate::families::b2::records::b2_cones(&stream).is_empty());

    let mut stream = b2_cone_stream();
    stream[157..165].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::families::b2::records::b2_cones(&stream).is_empty());

    let mut stream = b2_cone_stream();
    stream[157..165].copy_from_slice(&2.0f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_cones(&stream).is_empty());

    let mut stream = b2_cone_stream();
    stream[173..181].copy_from_slice(&0.0f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_cones(&stream).is_empty());

    let mut stream = b2_cone_stream();
    stream[101..109].copy_from_slice(&0.0_f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_cones(&stream).is_empty());

    let mut stream = b2_cone_stream();
    stream[149..157].copy_from_slice(&0.0_f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_cones(&stream).is_empty());
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
fn b2_offset_support_parser_rejects_nonincreasing_domains() {
    let mut direct = b2_offset_support_stream();
    direct[32..40].copy_from_slice(&0.0f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_offset_supports(&direct).is_empty());

    let mut construction = b2_construction_use_stream();
    construction[26..34].copy_from_slice(&(-1.0f64).to_le_bytes());
    let uses = crate::families::b2::records::b2_construction_uses(&construction);
    let [use_record] = uses.as_slice() else {
        panic!("one construction-use record");
    };
    assert_eq!(use_record.kind, 0x01);
    assert_eq!(use_record.domain, None);
    assert!(crate::families::b2::records::b2_offset_supports(&construction).is_empty());
}

#[test]
fn offset_support_binding_rejects_nonincreasing_domains() {
    let mut offset = crate::families::b2::records::B2OffsetSupport {
        pos: 0,
        support_id: 1,
        distance: 2.0,
        domain: [0.0, 0.0, 1.0, 1.0],
    };
    let carriers = crate::families::a5a8::records::a5_surfaces(&a5_surface_stream());
    assert_eq!(
        crate::families::b2::records::offset_support_carriers(&[offset.clone()], &carriers),
        [Some(0)]
    );
    offset.domain[2] = offset.domain[0];
    assert_eq!(
        crate::families::b2::records::offset_support_carriers(&[offset], &carriers),
        [None]
    );
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

fn b2_nurbs_curve_stream(weights: [f64; 4]) -> Vec<u8> {
    let knot_end = 41.693_759_535_8_f64;
    let mut payload = vec![0x0d, 0x09, 0x0c];
    payload.extend_from_slice(&0.0f64.to_le_bytes());
    payload.extend_from_slice(&knot_end.to_le_bytes());
    payload.push(0x05);
    for point in [
        [11.0, 23.0, 37.0],
        [15.0, 31.0, 41.0],
        [24.0, 17.0, 43.0],
        [31.0, 29.0, 37.0],
    ] {
        for coordinate in point {
            payload.extend_from_slice(&f64::to_le_bytes(coordinate));
        }
    }
    for weight in weights {
        payload.extend_from_slice(&weight.to_le_bytes());
    }
    payload.extend_from_slice(&[0x05, 0x05]);
    for value in [0.0, knot_end, 1.0, 0.0] {
        payload.extend_from_slice(&f64::to_le_bytes(value));
    }
    payload.extend_from_slice(&[0x00, 0x07]);
    assert_eq!(payload.len(), 184);
    let mut record = vec![0xb2, 0x03, 0x16, 184, 0x19];
    record.extend(payload);
    record
}

#[test]
fn b2_nurbs_curve_parser_preserves_asymmetric_weights_in_source_order() {
    let curves = crate::families::b2::records::b2_nurbs_curves(&b2_nurbs_curve_stream([
        1.0, 0.72, 1.31, 0.93,
    ]));
    let [curve] = curves.as_slice() else {
        panic!("one rational curve");
    };
    assert_eq!(curve.geometry.degree, 3);
    assert_eq!(curve.geometry.control_points.len(), 4);
    assert_eq!(
        curve.geometry.weights.as_deref(),
        Some(&[1.0, 0.72, 1.31, 0.93][..])
    );
    assert_eq!(curve.geometry.knots.len(), 8);
    assert_eq!(curve.geometry.knots[..4], [0.0; 4]);
    assert_eq!(curve.geometry.knots[4..], [41.693_759_535_8; 4]);
}

#[test]
fn b2_nurbs_curve_parser_rejects_broken_frame_invariants() {
    let valid = b2_nurbs_curve_stream([1.0, 0.72, 1.31, 0.93]);
    for offset in [6, 7, 8, 16, 24, 153, 154, 155, 163, 171, 179, 187, 188] {
        let mut broken = valid.clone();
        broken[offset] ^= 1;
        assert!(
            crate::families::b2::records::b2_nurbs_curves(&broken).is_empty(),
            "offset {offset}"
        );
    }
    let mut nonpositive_weight = valid;
    nonpositive_weight[5 + 3 + 16 + 1 + 4 * 24..5 + 3 + 16 + 1 + 4 * 24 + 8]
        .copy_from_slice(&0.0f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_nurbs_curves(&nonpositive_weight).is_empty());
}

#[test]
fn b2_nurbs_curve_parser_rejects_nonfinite_knots_poles_and_weights() {
    let mut nonfinite_knot = b2_nurbs_curve_stream([1.0, 0.72, 1.31, 0.93]);
    nonfinite_knot[8..16].copy_from_slice(&f64::NAN.to_le_bytes());
    nonfinite_knot[155..163].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::families::b2::records::b2_nurbs_curves(&nonfinite_knot).is_empty());

    let mut nonfinite_pole = b2_nurbs_curve_stream([1.0, 0.72, 1.31, 0.93]);
    nonfinite_pole[25..33].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::families::b2::records::b2_nurbs_curves(&nonfinite_pole).is_empty());

    let mut infinite_weight = b2_nurbs_curve_stream([1.0, 0.72, 1.31, 0.93]);
    infinite_weight[121..129].copy_from_slice(&f64::INFINITY.to_le_bytes());
    assert!(crate::families::b2::records::b2_nurbs_curves(&infinite_weight).is_empty());
}

fn b2_spatial_circle_stream() -> Vec<u8> {
    let cosine = 0.696_706_709_347_165_3_f64;
    let sine = 0.717_356_090_899_522_8_f64;
    let values = [
        17.0,
        23.0,
        13.0,
        cosine,
        -sine,
        0.0,
        sine,
        cosine,
        -0.0,
        7.0,
        0.0,
        11.2,
        1.0,
        -16.391_148_575_128_55,
    ];
    let mut record = vec![0xb2, 0x03, 0x0f, 112, 0x05];
    for value in values {
        record.extend_from_slice(&value.to_le_bytes());
    }
    record
}

#[test]
fn b2_spatial_circle_parser_reads_the_model_space_frame_and_range() {
    let circles = crate::families::b2::records::b2_spatial_circles(&b2_spatial_circle_stream());
    let [circle] = circles.as_slice() else {
        panic!("one spatial circle");
    };
    assert_eq!(
        circle.center,
        cadmpeg_ir::math::Point3::new(17.0, 23.0, 13.0)
    );
    assert!((circle.axis.z - 1.0).abs() < 1e-12);
    assert_eq!(circle.radius, 7.0);
    assert_eq!(circle.range, [0.0, 11.2]);
    assert_eq!(circle.chart_shift, -16.391_148_575_128_55);
}

#[test]
fn b2_spatial_circle_parser_rejects_nonorthonormal_invalid_charts_and_nonfinite_payload() {
    for scalar in [3usize, 6, 9, 11, 12] {
        let mut broken = b2_spatial_circle_stream();
        let offset = 5 + scalar * 8;
        broken[offset..offset + 8].copy_from_slice(&0.0f64.to_le_bytes());
        assert!(
            crate::families::b2::records::b2_spatial_circles(&broken).is_empty(),
            "scalar {scalar}"
        );
    }

    for scalar in [0usize, 3, 9, 10, 13] {
        let mut broken = b2_spatial_circle_stream();
        let offset = 5 + scalar * 8;
        broken[offset..offset + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(
            crate::families::b2::records::b2_spatial_circles(&broken).is_empty(),
            "nonfinite scalar {scalar}"
        );
    }
}

#[test]
fn b2_composite_parser_reads_embedded_cylinder_frame() {
    let bytes = b2_embedded_cylinder_stream();
    let cylinders = crate::families::b2::records::b2_embedded_cylinders(&bytes);
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].object_id, 0x5678);
    assert_eq!(cylinders[0].wrapper_pos, 0);
    assert_eq!(
        cylinders[0].cylinder.u_range,
        [0.0, 4.0 * std::f64::consts::PI]
    );
    assert!(crate::families::b2::records::b2_cylinders(&bytes).is_empty());
}

#[test]
fn b2_composite_parser_reads_the_complete_type_three_group() {
    let one = b2_embedded_cylinder_stream();
    let frame = one[7..].to_vec();
    let mut bytes = one;
    for _ in 0..30 {
        bytes.extend_from_slice(&frame);
    }

    let cylinders = crate::families::b2::records::b2_embedded_cylinders(&bytes);
    assert_eq!(cylinders.len(), 31);
    assert!(cylinders.iter().all(|cylinder| cylinder.wrapper_pos == 0));
}

#[test]
fn decode_inner_no_directory_transfers_b2_cylinder() {
    assert_eq!(
        crate::container::scan_bytes(inner_no_directory_b2_catpart()).variant,
        Variant::InnerNoDirectory
    );
    let mut cur = Cursor::new(inner_no_directory_b2_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Cylinder { radius: 2.0, .. }
    ));
}

#[test]
fn offset_support_binds_by_native_domain_knot_limits() {
    let mut carriers = crate::families::a5a8::records::a5_surfaces(&a5_surface_stream());
    let mut decoy = carriers[0].clone();
    let SurfaceGeometry::Nurbs(surface) = &mut decoy.geometry else {
        panic!("NURBS fixture");
    };
    for knot in &mut surface.v_knots {
        *knot += 10.0;
    }
    carriers.push(decoy);
    let SurfaceGeometry::Nurbs(surface) = &carriers[0].geometry else {
        panic!("NURBS fixture");
    };
    let offset = crate::families::b2::records::B2OffsetSupport {
        pos: 0,
        support_id: 7,
        distance: 2.0,
        domain: [
            surface.u_knots[0],
            surface.v_knots[0],
            *surface.u_knots.last().unwrap(),
            *surface.v_knots.last().unwrap(),
        ],
    };

    assert_eq!(
        crate::families::b2::records::offset_support_carriers(&[offset], &carriers),
        [Some(0)]
    );
}

#[test]
fn consolidated_edge_nodes_require_canonical_headers_and_terminal_controls() {
    let bytes = b2_edge_node_stream();
    assert_eq!(crate::families::b2::records::b2_edge_nodes(&bytes).len(), 1);

    let mut noncanonical_header = bytes.clone();
    noncanonical_header[0] = 0xb3;
    noncanonical_header[4] = 0x04;
    noncanonical_header.insert(5, 1);
    assert!(crate::families::b2::records::b2_edge_nodes(&noncanonical_header).is_empty());

    let mut wide_header = bytes.clone();
    wide_header[0] = 0xb3;
    wide_header[4] = 0x04;
    wide_header.insert(5, 0x40);
    let wide_nodes = crate::families::b2::records::b2_edge_nodes(&wide_header);
    let [wide_node] = wide_nodes.as_slice() else {
        panic!("canonical wide-header edge node")
    };
    assert_eq!(wide_node.header_token, 0x4004);

    let mut invalid_terminal = bytes;
    *invalid_terminal.last_mut().expect("edge terminal") = 0x03;
    assert!(crate::families::b2::records::b2_edge_nodes(&invalid_terminal).is_empty());
}
