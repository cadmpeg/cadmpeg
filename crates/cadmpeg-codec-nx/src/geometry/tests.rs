// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use cadmpeg_ir::geometry::SurfaceGeometry;

use crate::test_support::*;

#[test]
fn nx_offset_surface_accepts_unbounded_representable_distance() {
    let mut stream = offset_surface_topology_partition_stream();
    let offset = stream
        .windows(4)
        .position(|window| window == [0, 60, 0, 12])
        .expect("offset record");
    put_f64(&mut stream, offset + 23, 1_001.0);
    let surfaces = crate::topology::offset_surfaces(&stream);
    let [surface] = surfaces.as_slice() else {
        panic!("offset surface")
    };
    assert_eq!(surface.distance, 1_001_000.0);

    put_f64(&mut stream, offset + 23, f64::INFINITY);
    assert!(crate::topology::offset_surfaces(&stream).is_empty());

    put_f64(&mut stream, offset + 23, f64::MAX);
    assert!(crate::topology::offset_surfaces(&stream).is_empty());
}

#[test]
fn offset_surface_envelope_does_not_consume_the_following_record() {
    let mut stream = offset_surface_topology_partition_stream();
    let offset_end = stream.len();
    let mut point = record(29, 40);
    put_ref(&mut point, 2, 20);
    put_vec3(&mut point, 16, [0.001, 0.002, 0.003]);
    stream.extend(point);

    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.get(60, 12).map(crate::topology::Node::end),
        Some(offset_end)
    );
    assert!(graph.get(29, 20).is_some());
}

#[test]
fn nx_blend_surface_requires_a_nonzero_rolling_ball_radius() {
    let mut stream = blend_surface_topology_partition_stream();
    let blend = stream
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("blend record");
    put_f64(&mut stream, blend + 26, 0.0);
    put_f64(&mut stream, blend + 34, 0.0);
    assert!(crate::topology::blend_surfaces(&stream).is_empty());

    put_f64(&mut stream, blend + 26, 0.5e-9);
    assert!(crate::topology::blend_surfaces(&stream).is_empty());

    put_f64(&mut stream, blend + 26, f64::MAX);
    put_f64(&mut stream, blend + 34, f64::MAX);
    assert!(crate::topology::blend_surfaces(&stream).is_empty());
}

#[test]
fn trimmed_curves_reject_nonfinite_endpoint_witnesses() {
    let mut stream = trimmed_topology_partition_stream();
    let trim = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("trimmed curve");
    put_f64(&mut stream, trim + 21, f64::NAN);
    assert!(crate::topology::trimmed_curves(&stream).is_empty());

    put_f64(&mut stream, trim + 21, f64::MAX);
    assert!(crate::topology::trimmed_curves(&stream).is_empty());
}

#[test]
fn analytic_scanner_accepts_positive_subnormal_radius() {
    let mut cy = record(0x33, 99);
    put_ref(&mut cy, 2, 2);
    cy[18] = b'+';
    put_vec3(&mut cy, 19, [0.003_175, 0.0, 0.0]);
    put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cy, 67, f64::from_bits(1)); // smallest positive subnormal
    put_vec3(&mut cy, 75, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::surfaces(&cy).len(), 1);
}

#[test]
fn graph_owned_analytic_geometry_has_no_scanner_magnitude_limit() {
    let mut cylinder = record(0x33, 99);
    put_ref(&mut cylinder, 2, 2);
    cylinder[18] = b'+';
    put_vec3(&mut cylinder, 19, [1_001.0, 0.0, 0.0]);
    put_vec3(&mut cylinder, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cylinder, 67, f64::from_bits(1));
    put_vec3(&mut cylinder, 75, [1.0, 0.0, 0.0]);

    assert_eq!(crate::geometry::surfaces(&cylinder).len(), 1);
    let geometry =
        crate::geometry::decode_surface_record(&cylinder, 0x33, 0).expect("graph-owned cylinder");
    let SurfaceGeometry::Cylinder { origin, radius, .. } = geometry else {
        panic!("cylinder")
    };
    assert_eq!(origin.x, 1_001_000.0);
    assert_eq!(radius, f64::from_bits(1) * 1000.0);

    put_f64(&mut cylinder, 67, f64::INFINITY);
    assert!(crate::geometry::decode_surface_record(&cylinder, 0x33, 0).is_none());
}

#[test]
fn ellipse_requires_ordered_serialized_radii() {
    let mut ellipse = record(0x20, 107);
    put_ref(&mut ellipse, 2, 2);
    ellipse[18] = b'+';
    put_vec3(&mut ellipse, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut ellipse, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut ellipse, 67, [1.0, 0.0, 0.0]);
    put_f64(&mut ellipse, 91, 0.01);
    put_f64(&mut ellipse, 99, 0.01 + 5.0e-10);

    assert!(crate::geometry::curves(&ellipse).is_empty());
    assert!(crate::geometry::decode_curve_record(&ellipse, 0x20, 0).is_none());

    put_f64(&mut ellipse, 99, 0.01);
    assert_eq!(crate::geometry::curves(&ellipse).len(), 1);
}

#[test]
fn graph_owned_point_has_no_scanner_magnitude_limit() {
    let mut stream = topology_partition_stream();
    let point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, point + 16, [1_001.0, f64::from_bits(1), 0.0]);

    assert_eq!(crate::geometry::points(&stream).len(), 1);
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph
            .get(29, 11)
            .and_then(crate::topology::Node::point_position),
        Some(cadmpeg_ir::math::Point3::new(
            1_001_000.0,
            f64::from_bits(1) * 1000.0,
            0.0,
        ))
    );

    put_vec3(&mut stream, point + 16, [f64::INFINITY, 0.0, 0.0]);
    assert!(crate::topology::Graph::parse(&stream).get(29, 11).is_none());
}

#[test]
fn decoded_tolerance_has_no_model_magnitude_limit() {
    assert_eq!(crate::decode::decoded_tolerance(1_001.0), Some(1_001_000.0));
    assert_eq!(crate::decode::decoded_tolerance(0.0), None);
    assert_eq!(crate::decode::decoded_tolerance(f64::INFINITY), None);
    assert_eq!(crate::decode::decoded_tolerance(f64::MAX), None);
}

#[test]
fn analytic_frame_gate_rejects_nonorthogonal_reference_direction() {
    let mut plane = record(0x32, 91);
    put_ref(&mut plane, 2, 2);
    plane[18] = b'+';
    put_vec3(&mut plane, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [0.0, 0.0, 1.0]);
    assert!(crate::geometry::surfaces(&plane).is_empty());

    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::surfaces(&plane).len(), 1);
}

#[test]
fn analytic_scanner_does_not_rescan_a_complete_invalid_frame() {
    let mut stream = vec![0; 91];
    stream[1] = 0x32;
    put_ref(&mut stream, 2, 2);
    stream[18] = b'+';

    // A valid LINE-looking record begins inside the complete PLANE frame. The
    // outer plane remains invalid because its normal reads the line origin.
    stream[24] = 0;
    stream[25] = 0x1e;
    put_ref(&mut stream, 26, 3);
    stream[42] = b'+';
    put_vec3(&mut stream, 43, [0.0, 0.0, 0.0]);
    put_vec3(&mut stream, 67, [1.0, 0.0, 0.0]);

    assert!(crate::geometry::surfaces(&stream).is_empty());
    assert!(crate::geometry::curves(&stream).is_empty());
}

#[test]
fn cone_gate_rejects_nonfinite_or_degenerate_half_angle() {
    let mut cone = record(0x34, 115);
    put_ref(&mut cone, 2, 2);
    cone[18] = b'+';
    put_vec3(&mut cone, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cone, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cone, 67, 0.0);
    put_f64(&mut cone, 75, std::f64::consts::FRAC_1_SQRT_2);
    put_f64(&mut cone, 83, std::f64::consts::FRAC_1_SQRT_2);
    put_vec3(&mut cone, 91, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::surfaces(&cone).len(), 1);

    for (sine, cosine) in [(f64::NAN, 1.0), (0.0, 1.0), (1.0, 0.0)] {
        put_f64(&mut cone, 75, sine);
        put_f64(&mut cone, 83, cosine);
        assert!(crate::geometry::surfaces(&cone).is_empty());
    }
}

#[test]
fn analytic_scanners_include_extended_reference_shifts_in_record_ownership() {
    let mut surfaces = vec![0; 184];
    surfaces[1] = 0x32;
    surfaces[2..6].copy_from_slice(&encoded_xmt(32_768));
    surfaces[20] = b'+';
    put_vec3(&mut surfaces, 21, [0.0, 0.0, 0.0]);
    put_vec3(&mut surfaces, 45, [0.0, 0.0, 1.0]);
    put_vec3(&mut surfaces, 69, [1.0, 0.0, 0.0]);
    surfaces[93] = 0;
    surfaces[94] = 0x32;
    put_ref(&mut surfaces, 95, 3);
    surfaces[111] = b'+';
    put_vec3(&mut surfaces, 112, [0.0, 0.0, 0.0]);
    put_vec3(&mut surfaces, 136, [0.0, 0.0, 1.0]);
    put_vec3(&mut surfaces, 160, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::surfaces(&surfaces).len(), 2);

    let mut curves = vec![0; 136];
    curves[1] = 0x1e;
    curves[2..6].copy_from_slice(&encoded_xmt(32_768));
    curves[20] = b'+';
    put_vec3(&mut curves, 21, [0.0, 0.0, 0.0]);
    put_vec3(&mut curves, 45, [1.0, 0.0, 0.0]);
    curves[69] = 0;
    curves[70] = 0x1e;
    put_ref(&mut curves, 71, 3);
    curves[87] = b'+';
    put_vec3(&mut curves, 88, [0.0, 0.0, 0.0]);
    put_vec3(&mut curves, 112, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::curves(&curves).len(), 2);
}

#[test]
fn analytic_scanner_resolves_envelope_escape_framing() {
    let mut plane = vec![0; 92];
    plane[1] = 0x32;
    plane[2] = 0xff;
    put_ref(&mut plane, 3, 2);
    plane[19] = b'+';
    put_vec3(&mut plane, 20, [0.0, 0.0, 0.0]);
    put_vec3(&mut plane, 44, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 68, [1.0, 0.0, 0.0]);

    assert_eq!(crate::geometry::surfaces(&plane).len(), 1);
}

#[test]
fn analytic_record_ownership_is_shared_across_carrier_families() {
    let mut stream = vec![0; 158];
    stream[1] = 0x1e;
    put_ref(&mut stream, 2, 2);
    stream[18] = b'+';
    put_vec3(&mut stream, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut stream, 43, [1.0, 0.0, 0.0]);

    stream[67] = 0;
    stream[68] = 0x32;
    put_ref(&mut stream, 69, 3);
    stream[85] = b'+';
    put_vec3(&mut stream, 86, [0.0, 0.0, 0.0]);
    put_vec3(&mut stream, 110, [0.0, 0.0, 1.0]);
    put_vec3(&mut stream, 134, [1.0, 0.0, 0.0]);

    assert_eq!(crate::geometry::curves(&stream).len(), 1);
    assert_eq!(crate::geometry::surfaces(&stream).len(), 1);
    assert!(crate::geometry::points(&stream).is_empty());
}
