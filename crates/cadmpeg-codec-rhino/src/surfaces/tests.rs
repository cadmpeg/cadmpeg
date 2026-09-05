// SPDX-License-Identifier: Apache-2.0
#![allow(dead_code, clippy::disallowed_methods)]

use super::*;
use crate::chunks::{ArchiveVersion, BoundedReader};
use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

const EPS_EXACT_GEOMETRY: f64 = 1.0e-12;

fn push_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend(value.to_le_bytes());
}

fn push_f64(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend(value.to_le_bytes());
}

fn curve_payload(version: u8, rational: bool, knots: &[f64]) -> Vec<u8> {
    let mut bytes = vec![version];
    push_i32(&mut bytes, 3);
    push_i32(&mut bytes, i32::from(rational));
    push_i32(&mut bytes, 3);
    push_i32(&mut bytes, 6);
    push_i32(&mut bytes, 0);
    push_i32(&mut bytes, 0);
    bytes.extend([0; 48]);
    push_i32(&mut bytes, i32::try_from(knots.len()).expect("test count"));
    for knot in knots {
        push_f64(&mut bytes, *knot);
    }
    push_i32(&mut bytes, 6);
    for index in 0..6 {
        push_f64(&mut bytes, index as f64);
        push_f64(&mut bytes, 0.0);
        push_f64(&mut bytes, 0.0);
        if rational {
            push_f64(&mut bytes, if index == 0 { 2.0 } else { 1.0 });
        }
    }
    if version & 0x0f >= 1 {
        bytes.push(0);
    }
    bytes
}

fn curve_2d_payload(rational: bool) -> Vec<u8> {
    let mut bytes = vec![0x10];
    push_i32(&mut bytes, 2);
    push_i32(&mut bytes, i32::from(rational));
    push_i32(&mut bytes, 3);
    push_i32(&mut bytes, 6);
    push_i32(&mut bytes, 0);
    push_i32(&mut bytes, 0);
    bytes.extend([0; 48]);
    let knots = [0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0];
    push_i32(&mut bytes, knots.len() as i32);
    for knot in knots {
        push_f64(&mut bytes, knot);
    }
    push_i32(&mut bytes, 6);
    for index in 0..6 {
        push_f64(&mut bytes, index as f64);
        push_f64(&mut bytes, 2.0 * index as f64);
        if rational {
            push_f64(&mut bytes, if index == 0 { 2.0 } else { 1.0 });
        }
    }
    bytes
}

fn surface_payload(
    u_order: i32,
    v_order: i32,
    u_count: i32,
    v_count: i32,
    rational: bool,
    u_knots: &[f64],
    v_knots: &[f64],
) -> Vec<u8> {
    let mut bytes = vec![0x10];
    push_i32(&mut bytes, 3);
    push_i32(&mut bytes, i32::from(rational));
    push_i32(&mut bytes, u_order);
    push_i32(&mut bytes, v_order);
    push_i32(&mut bytes, u_count);
    push_i32(&mut bytes, v_count);
    push_i32(&mut bytes, 0);
    push_i32(&mut bytes, 0);
    bytes.extend([0; 48]);
    push_i32(
        &mut bytes,
        i32::try_from(u_knots.len()).expect("test count"),
    );
    for knot in u_knots {
        push_f64(&mut bytes, *knot);
    }
    push_i32(
        &mut bytes,
        i32::try_from(v_knots.len()).expect("test count"),
    );
    for knot in v_knots {
        push_f64(&mut bytes, *knot);
    }
    push_i32(&mut bytes, u_count * v_count);
    for i in 0..u_count {
        for j in 0..v_count {
            push_f64(&mut bytes, f64::from(i));
            push_f64(&mut bytes, f64::from(j));
            push_f64(&mut bytes, 0.0);
            if rational {
                push_f64(&mut bytes, f64::from(i + j + 1));
            }
        }
    }
    bytes
}

fn surface_2d_payload(rational: bool) -> Vec<u8> {
    let mut bytes = vec![0x10];
    push_i32(&mut bytes, 2);
    push_i32(&mut bytes, i32::from(rational));
    push_i32(&mut bytes, 2);
    push_i32(&mut bytes, 2);
    push_i32(&mut bytes, 3);
    push_i32(&mut bytes, 2);
    push_i32(&mut bytes, 0);
    push_i32(&mut bytes, 0);
    bytes.extend([0; 48]);
    let u_knots = [10.0, 11.0, 12.0];
    push_i32(&mut bytes, u_knots.len() as i32);
    for knot in u_knots {
        push_f64(&mut bytes, knot);
    }
    let v_knots = [20.0, 21.0];
    push_i32(&mut bytes, v_knots.len() as i32);
    for knot in v_knots {
        push_f64(&mut bytes, knot);
    }
    push_i32(&mut bytes, 6);
    for i in 0..3 {
        for j in 0..2 {
            let weight = if rational {
                1.0 + f64::from(i + j)
            } else {
                1.0
            };
            push_f64(&mut bytes, (100.0 + f64::from(i)) * weight);
            push_f64(&mut bytes, (200.0 + f64::from(j)) * weight);
            if rational {
                push_f64(&mut bytes, weight);
            }
        }
    }
    bytes
}

fn plane_payload(version: u8, bad_frame: bool, bad_range: bool) -> Vec<u8> {
    let mut bytes = vec![version];
    push_f64(&mut bytes, 1.0);
    push_f64(&mut bytes, 2.0);
    push_f64(&mut bytes, 3.0);
    for axis in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        for value in axis {
            push_f64(&mut bytes, value);
        }
    }
    for value in [0.0, 0.0, 1.0, -3.0] {
        push_f64(&mut bytes, value);
    }
    if bad_frame {
        bytes[(1 + 24)..=32].copy_from_slice(&2.0_f64.to_le_bytes());
    }
    for range in [[0.0, 1.0], [2.0, 3.0]] {
        push_f64(&mut bytes, range[0]);
        push_f64(&mut bytes, if bad_range { range[0] } else { range[1] });
    }
    if version & 0x0f == 1 {
        for range in [[4.0, 5.0], [6.0, 7.0]] {
            push_f64(&mut bytes, range[0]);
            push_f64(&mut bytes, range[1]);
        }
    }
    bytes
}

fn test_curve(points: Vec<Point3>, weights: Option<Vec<f64>>, domain: [f64; 2]) -> NurbsCurve {
    NurbsCurve::new(
        1,
        vec![domain[0], domain[0], domain[1], domain[1]],
        points,
        weights,
        false,
    )
    .expect("valid test curve")
}

fn revolution_prefix(version: u8) -> Vec<u8> {
    let mut bytes = vec![version];
    for value in [1.0, 2.0, 3.0, 1.0, 2.0, 5.0, 0.25, 1.25] {
        push_f64(&mut bytes, value);
    }
    if version >> 4 >= 2 {
        for value in [4.0, 9.0] {
            push_f64(&mut bytes, value);
        }
    }
    for value in [-10.0, -10.0, -10.0, 10.0, 10.0, 10.0] {
        push_f64(&mut bytes, value);
    }
    push_i32(&mut bytes, 0);
    bytes.push(0);
    bytes
}

fn long_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    bytes.extend((body.len() as i64).to_le_bytes());
    bytes.extend(body);
    bytes
}

fn crc_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut payload = body.to_vec();
    payload.extend(crc32fast::hash(body).to_le_bytes());
    long_chunk(typecode, &payload)
}

fn anonymous(minor: i32, body: &[u8]) -> Vec<u8> {
    let mut payload = 1_i32.to_le_bytes().to_vec();
    payload.extend(minor.to_le_bytes());
    payload.extend(body);
    crc_chunk(0x4000_8000, &payload)
}

fn clipping_plane_payload(item_order_valid: bool) -> Vec<u8> {
    let carrier = crc_chunk(0x4000_8000, &plane_payload(0x11, false, false));
    let mut clipping = [0x11; 16].to_vec();
    clipping.extend([0x22; 16]);
    clipping.extend(&plane_payload(0x11, false, false)[1..129]);
    clipping.push(1);
    let mut viewports = 1_i32.to_le_bytes().to_vec();
    viewports.extend([0x33; 16]);
    clipping.extend(anonymous(0, &viewports));
    clipping.extend(2.5_f64.to_le_bytes());
    clipping.push(1);
    clipping.push(if item_order_valid { 10 } else { 13 });
    if item_order_valid {
        clipping.extend(1_i32.to_le_bytes());
        clipping.extend([0x44; 16]);
        clipping.push(11);
        clipping.extend(1_i32.to_le_bytes());
        clipping.extend(7_i32.to_le_bytes());
        clipping.push(12);
        clipping.push(0);
        clipping.push(13);
        clipping.push(1);
    } else {
        clipping.push(1);
        clipping.push(10);
        clipping.extend(0_i32.to_le_bytes());
    }
    clipping.push(0);
    clipping.extend([0xaa, 0xbb]);
    let clipping = anonymous(5, &clipping);
    let mut outer = 1_i32.to_le_bytes().to_vec();
    outer.extend(2_i32.to_le_bytes());
    outer.extend(carrier);
    outer.extend(clipping);
    outer.extend([0xcc, 0xdd]);
    crc_chunk(0x4000_8000, &outer)
}

#[test]
fn clipping_plane_decodes_plane_carrier_and_all_v8_suffix_items() {
    let bytes = clipping_plane_payload(true);
    let decoded = decode(
        &bytes,
        CLIPPING_PLANE_SURFACE,
        0..bytes.len(),
        25.4,
        ArchiveVersion::V8,
        0,
    )
    .expect("clipping plane");
    let DecodedSurface::Typed {
        geometry: SurfaceGeometry::Plane { origin, .. },
        derived,
        ..
    } = decoded
    else {
        panic!("typed plane carrier");
    };
    assert_eq!(origin, Point3::new(25.4, 50.8, 76.199_999_999_999_99));
    assert!(derived);

    let invalid = clipping_plane_payload(false);
    assert!(decode(
        &invalid,
        CLIPPING_PLANE_SURFACE,
        0..invalid.len(),
        1.0,
        ArchiveVersion::V8,
        0,
    )
    .is_err());
}

fn line_wrapper(scale_source: f64) -> Vec<u8> {
    let mut line = vec![0x10];
    for value in [
        2.0 * scale_source,
        0.0,
        0.0,
        3.0 * scale_source,
        0.0,
        0.0,
        6.0,
        8.0,
    ] {
        push_f64(&mut line, value);
    }
    push_i32(&mut line, 3);
    let wire_uuid = [
        0xdb, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ];
    let mut uuid_body = wire_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&wire_uuid).to_le_bytes());
    let mut class_body = long_chunk(0x0002_fffb, &uuid_body);
    class_body.extend(crc_chunk(0x0002_fffc, &line));
    class_body.extend(0x8002_7fff_u32.to_le_bytes());
    class_body.extend(0_i64.to_le_bytes());
    long_chunk(0x0002_7ffa, &class_body)
}

fn nil_object_wrapper() -> Vec<u8> {
    long_chunk(0x0002_7ffa, &long_chunk(0x0002_fffb, &[0; 16]))
}

pub(crate) fn valid_revolution_payload(version: u8) -> Vec<u8> {
    let mut bytes = revolution_prefix(version);
    *bytes.last_mut().expect("required invariant") = 1;
    bytes.extend(line_wrapper(1.0));
    bytes
}

fn valid_sum_payload() -> Vec<u8> {
    let mut bytes = vec![0x10];
    for value in [1.0, 2.0, 3.0, -10.0, -10.0, -10.0, 10.0, 10.0, 10.0] {
        push_f64(&mut bytes, value);
    }
    bytes.extend(line_wrapper(1.0));
    bytes.extend(line_wrapper(2.0));
    bytes
}

#[test]
fn reconstructs_spec_examples_and_one_sided_vectors() {
    assert_eq!(
        reconstruct_knots(&[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0], 3, 6).expect("required invariant"),
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0]
    );
    assert_eq!(
        reconstruct_knots(&[0.0, 1.0, 2.0, 3.0, 5.0, 6.0, 7.0], 3, 6).expect("required invariant"),
        vec![-2.0, 0.0, 1.0, 2.0, 3.0, 5.0, 6.0, 7.0, 9.0]
    );
    assert_eq!(
        reconstruct_knots(&[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0], 3, 6).expect("required invariant")
            [0],
        0.0
    );
    assert_eq!(
        reconstruct_knots(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 5.0], 3, 6).expect("required invariant")
            [8],
        5.0
    );
}

#[test]
fn curve_versions_cross_archive_bands_and_consume_tag_gate() {
    for (archive, version) in [(ArchiveVersion::V5, 0x10), (ArchiveVersion::V8, 0x11)] {
        let bytes = curve_payload(version, false, &[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0]);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
        let curve = read_nurbs_curve(&mut reader, 1.0).expect("required invariant");
        assert_eq!(curve.control_points().len(), 6);
        assert_eq!(reader.remaining(), 0);
        assert!(matches!(archive, ArchiveVersion::V5 | ArchiveVersion::V8));
    }
}

#[test]
fn curve_payload_validates_rational_weights_counts_and_domain() {
    let mut bytes = curve_payload(0x10, true, &[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0]);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    let curve = read_nurbs_curve(&mut reader, 2.0).expect("required invariant");
    assert_eq!(curve.control_points()[0].x, 0.0);
    assert_eq!(curve.weights().expect("rational curve")[0], 2.0);
    let weight_offset = 1 + 28 + 48 + 4 + 7 * 8 + 4 + 24;
    bytes[weight_offset..weight_offset + 8].copy_from_slice(&0.0_f64.to_le_bytes());
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    assert!(read_nurbs_curve(&mut reader, 1.0).is_err());
}

#[test]
fn c2_nurbs_reads_two_dimensions_without_scaling_uv() {
    let bytes = curve_2d_payload(true);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    let curve = read_nurbs_curve_2d(&mut reader).expect("required invariant");
    assert_eq!(reader.remaining(), 0);
    assert_eq!(curve.control_points()[1].x, 1.0);
    assert_eq!(curve.control_points()[1].y, 2.0);
    assert_eq!(curve.weights().expect("rational curve")[0], 2.0);
    assert_eq!(
        curve.knots(),
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0]
    );
}

#[test]
fn top_level_nurbs_lifts_a_valid_two_dimensional_curve() {
    let bytes = curve_2d_payload(false);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    let curve = read_nurbs_curve(&mut reader, 2.0).expect("valid two-dimensional curve");
    assert_eq!(reader.remaining(), 0);
    assert_eq!(curve.control_points()[1], Point3::new(2.0, 4.0, 0.0));
    assert_eq!(
        curve.knots(),
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0]
    );
}

#[test]
fn c2_nurbs_preserves_periodic_parameterization() {
    let mut bytes = curve_2d_payload(false);
    let knots: [f64; 7] = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    let start = 1 + 24 + 48 + 4;
    for (index, knot) in knots.into_iter().enumerate() {
        bytes[start + index * 8..start + index * 8 + 8].copy_from_slice(&knot.to_le_bytes());
    }
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    assert!(read_nurbs_curve_2d(&mut reader)
        .expect("required invariant")
        .periodic());
}

#[test]
fn periodic_rule_matches_native_tolerance_and_rejects_clamping() {
    assert!(!periodic_knots(&[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0], 3, 6));
    assert!(periodic_knots(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 3, 6));
    assert!(!periodic_knots(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 7.0], 3, 6));
    assert!(!periodic_knots(&[0.0, 1.0, 2.0, 3.0], 2, 4));
    let mut near = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    near[6] += 1.0e-8;
    assert!(periodic_knots(&near, 3, 6));
}

#[test]
fn surface_periodicity_is_derived_independently_in_u_and_v() {
    let periodic = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    let nonperiodic = [0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0];
    let bytes = surface_payload(3, 3, 6, 6, false, &periodic, &nonperiodic);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    let surface = read_nurbs_surface(&mut reader, 1.0).expect("required invariant");
    assert!(surface.u_periodic());
    assert!(!surface.v_periodic());
}

#[test]
fn surface_bytes_preserve_asymmetric_u_major_rational_poles() {
    let bytes = surface_payload(2, 2, 2, 3, true, &[0.0, 1.0], &[0.0, 1.0, 2.0]);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    let surface = read_nurbs_surface(&mut reader, 1.0).expect("required invariant");
    assert_eq!(surface.control_points()[1].y, 1.0 / 2.0);
    assert_eq!(surface.control_points()[3].x, 1.0 / 2.0);
    assert_eq!(surface.weights().expect("rational surface")[5], 4.0);
}

#[test]
fn surface_bytes_reconstruct_independent_knots_and_reject_count_mismatch() {
    let bytes = surface_payload(2, 2, 3, 2, false, &[0.0, 1.0, 2.0], &[0.0, 1.0]);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    let surface = read_nurbs_surface(&mut reader, 1.0).expect("required invariant");
    assert_eq!(surface.u_knots(), vec![0.0, 0.0, 1.0, 2.0, 2.0]);
    assert_eq!(surface.v_knots(), vec![0.0, 0.0, 1.0, 1.0]);
    let mut bad = bytes;
    let count_offset = bad.len() - 6 * 24 - 4;
    bad[count_offset..count_offset + 4].copy_from_slice(&99_i32.to_le_bytes());
    let mut reader = BoundedReader::new(&bad, 0, bad.len()).expect("required invariant");
    assert!(read_nurbs_surface(&mut reader, 1.0).is_err());
}

#[test]
fn surface_reads_a_valid_two_dimensional_lattice_and_lifts_zero_z() {
    let bytes = surface_2d_payload(false);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    let surface = read_nurbs_surface(&mut reader, 2.0).expect("valid two-dimensional surface");
    assert_eq!(reader.remaining(), 0);
    assert_eq!((surface.u_count(), surface.v_count()), (3, 2));
    assert_eq!(surface.control_points()[1], Point3::new(200.0, 402.0, 0.0));
    assert_eq!(surface.u_knots(), vec![10.0, 10.0, 11.0, 12.0, 12.0]);
    assert_eq!(surface.v_knots(), vec![20.0, 20.0, 21.0, 21.0]);
}

#[test]
fn surface_reads_a_rational_two_dimensional_lattice() {
    let bytes = surface_2d_payload(true);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    let surface = read_nurbs_surface(&mut reader, 2.0).expect("valid rational surface");
    assert_eq!(reader.remaining(), 0);
    assert_eq!(surface.control_points()[1], Point3::new(200.0, 402.0, 0.0));
    assert_eq!(surface.weights(), Some(&[1.0, 2.0, 2.0, 3.0, 3.0, 4.0][..]));
}

#[test]
fn plane_versions_consume_defaults_and_explicit_extents() {
    for version in [0x10, 0x11] {
        let bytes = plane_payload(version, false, false);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
        let (plane, _) =
            read_plane_surface_with_parameterization(&mut reader, 1.0).expect("required invariant");
        assert_eq!(reader.remaining(), 0);
        assert!(matches!(
            plane,
            cadmpeg_ir::geometry::SurfaceGeometry::Plane { .. }
        ));
    }
    for (bad_frame, bad_range) in [(true, false), (false, true)] {
        let bytes = plane_payload(0x11, bad_frame, bad_range);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
        assert!(read_plane_surface_with_parameterization(&mut reader, 1.0).is_err());
    }
}

#[test]
fn plane_parameterization_maps_domain_to_physical_extents() {
    let bytes = plane_payload(0x11, false, false);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    let (_, parameterization) =
        read_plane_surface_with_parameterization(&mut reader, 1.0).expect("plane surface");
    assert_eq!(
        parameterization.map_point(Point2::new(0.25, 2.5)),
        Point2::new(4.25, 6.5)
    );
}

#[test]
fn sum_surface_preserves_asymmetric_domains_and_u_major_order() {
    let first = test_curve(
        vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
        None,
        [2.0, 5.0],
    );
    let second = NurbsCurve::new(
        2,
        vec![7.0, 7.0, 7.0, 9.0, 9.0, 9.0],
        vec![
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(20.0, 0.0, 0.0),
            Point3::new(30.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("valid test curve");
    let surface =
        sum_nurbs(&first, &second, Vector3::new(0.5, 1.5, 2.5), 0).expect("required invariant");
    assert_eq!((surface.u_count(), surface.v_count()), (2, 3));
    assert_eq!(surface.u_knots(), first.knots());
    assert_eq!(surface.v_knots(), second.knots());
    assert_eq!(surface.control_points()[0], Point3::new(11.5, 3.5, 5.5));
    assert_eq!(surface.control_points()[3], Point3::new(14.5, 6.5, 8.5));
    assert!(surface.weights().is_none());
}

#[test]
fn sum_surface_multiplies_each_rational_weight_pair() {
    for (first_weights, second_weights, expected) in [
        (Some(vec![2.0, 3.0]), None, vec![2.0, 2.0, 3.0, 3.0]),
        (None, Some(vec![5.0, 7.0]), vec![5.0, 7.0, 5.0, 7.0]),
        (
            Some(vec![2.0, 3.0]),
            Some(vec![5.0, 7.0]),
            vec![10.0, 14.0, 15.0, 21.0],
        ),
    ] {
        let first = test_curve(
            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
            first_weights,
            [0.0, 1.0],
        );
        let second = test_curve(
            vec![Point3::new(0.0, 3.0, 0.0), Point3::new(0.0, 4.0, 0.0)],
            second_weights,
            [4.0, 8.0],
        );
        let surface =
            sum_nurbs(&first, &second, Vector3::new(9.0, 8.0, 7.0), 0).expect("required invariant");
        assert_eq!(surface.weights().expect("rational surface"), expected);
        assert_eq!(surface.control_points()[3], Point3::new(11.0, 12.0, 7.0));
    }
}

#[test]
fn extrusion_tensor_preserves_rational_profile_knots_weights_and_transpose() {
    let start = NurbsCurve::new(
        2,
        vec![2.0, 2.0, 2.0, 5.0, 5.0, 5.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
        ],
        Some(vec![1.0, 0.5, 1.0]),
        false,
    )
    .expect("valid test curve");
    let mut end = start.clone();
    for point in end.control_points_mut() {
        point.z = 7.0;
    }
    let plain =
        super::extrusion_nurbs(&start, &end, [10.0, 20.0], false, 0).expect("required invariant");
    assert_eq!((plain.u_degree(), plain.v_degree()), (2, 1));
    assert_eq!(plain.u_knots(), start.knots());
    assert_eq!(plain.v_knots(), vec![10.0, 10.0, 20.0, 20.0]);
    assert_eq!(plain.weights(), Some(&[1.0, 1.0, 0.5, 0.5, 1.0, 1.0][..]));
    assert_eq!(plain.control_points()[3], end.control_points()[1]);
    let transposed =
        super::extrusion_nurbs(&start, &end, [10.0, 20.0], true, 0).expect("required invariant");
    assert_eq!((transposed.u_degree(), transposed.v_degree()), (1, 2));
    assert_eq!((transposed.u_count(), transposed.v_count()), (2, 3));
    assert_eq!(transposed.u_knots(), vec![10.0, 10.0, 20.0, 20.0]);
    assert_eq!(transposed.control_points()[1], start.control_points()[1]);
    assert_eq!(transposed.control_points()[3], end.control_points()[0]);
}

#[test]
fn revolution_preserves_partial_angle_parameter_domain_and_product_weights() {
    let profile = test_curve(
        vec![Point3::new(3.0, 0.0, 1.0), Point3::new(4.0, 0.0, 2.0)],
        Some(vec![2.0, 3.0]),
        [11.0, 13.0],
    );
    let surface = revolution_nurbs(
        &profile,
        Point3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        [0.0, std::f64::consts::FRAC_PI_2],
        [20.0, 30.0],
        false,
        0,
    )
    .expect("required invariant");
    assert_eq!((surface.u_count(), surface.v_count()), (3, 2));
    assert_eq!(surface.u_knots(), vec![20.0, 20.0, 20.0, 30.0, 30.0, 30.0]);
    assert_eq!(surface.v_knots(), profile.knots());
    assert_eq!(surface.weights().expect("rational surface")[0], 2.0);
    assert!(
        (surface.weights().expect("rational surface")[2] - 2.0 / 2.0_f64.sqrt()).abs()
            < EPS_EXACT_GEOMETRY
    );
    assert_eq!(surface.control_points()[0], profile.control_points()[0]);
    assert!((surface.control_points()[4].x - 1.0).abs() < EPS_EXACT_GEOMETRY);
    assert!((surface.control_points()[4].y - 2.0).abs() < EPS_EXACT_GEOMETRY);
}

#[test]
fn revolution_moves_singular_control_rows_exactly_onto_axis() {
    let profile = test_curve(
        vec![
            Point3::new(1.0 + 1.0e-13, 0.0, 2.0),
            Point3::new(2.0, 0.0, 3.0),
        ],
        None,
        [0.0, 1.0],
    );
    let surface = revolution_nurbs(
        &profile,
        Point3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        [0.0, std::f64::consts::FRAC_PI_2],
        [0.0, 1.0],
        false,
        0,
    )
    .expect("required invariant");
    for point in surface.control_points().iter().step_by(2) {
        assert_eq!(*point, Point3::new(1.0, 0.0, 2.0));
    }
}

#[test]
fn revolution_transpose_swaps_shape_and_reindexes_u_major_poles() {
    let profile = test_curve(
        vec![Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)],
        None,
        [4.0, 6.0],
    );
    let plain = revolution_nurbs(
        &profile,
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        [0.0, std::f64::consts::FRAC_PI_2],
        [8.0, 9.0],
        false,
        0,
    )
    .expect("required invariant");
    let transposed = revolution_nurbs(
        &profile,
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        [0.0, std::f64::consts::FRAC_PI_2],
        [8.0, 9.0],
        true,
        0,
    )
    .expect("required invariant");
    assert_eq!((transposed.u_count(), transposed.v_count()), (2, 3));
    assert_eq!((transposed.u_degree(), transposed.v_degree()), (1, 2));
    assert_eq!(transposed.u_knots(), profile.knots());
    assert_eq!(transposed.control_points()[1], plain.control_points()[2]);
    assert_eq!(transposed.control_points()[3], plain.control_points()[1]);
}

#[test]
fn revolution_rejects_versions_axis_intervals_transpose_and_presence() {
    let bad_version = [0x30];
    let mut reader =
        BoundedReader::new(&bad_version, 0, bad_version.len()).expect("required invariant");
    assert!(super::read_revolution(&bad_version, &mut reader, 1.0, ArchiveVersion::V5, 0).is_err());

    let valid = revolution_prefix(0x20);
    let axis_end_offset = 1 + 3 * 8;
    let angle_end_offset = 1 + 6 * 8 + 8;
    let parameter_end_offset = 1 + 8 * 8 + 8;
    let transpose_offset = valid.len() - 5;
    let mut cases = Vec::new();
    let mut zero_axis = valid.clone();
    let start = zero_axis[1..=24].to_vec();
    zero_axis[axis_end_offset..axis_end_offset + 24].copy_from_slice(&start);
    cases.push(zero_axis);
    let mut bad_angle = valid.clone();
    bad_angle[angle_end_offset..angle_end_offset + 8].copy_from_slice(&0.25_f64.to_le_bytes());
    cases.push(bad_angle);
    let mut bad_parameter = valid.clone();
    bad_parameter[parameter_end_offset..parameter_end_offset + 8]
        .copy_from_slice(&4.0_f64.to_le_bytes());
    cases.push(bad_parameter);
    let mut bad_transpose = valid.clone();
    bad_transpose[transpose_offset..transpose_offset + 4].copy_from_slice(&2_i32.to_le_bytes());
    cases.push(bad_transpose);
    for bytes in cases {
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
        assert!(super::read_revolution(&bytes, &mut reader, 1.0, ArchiveVersion::V5, 0).is_err());
    }
    let mut reader = BoundedReader::new(&valid, 0, valid.len()).expect("required invariant");
    assert!(super::read_revolution(&valid, &mut reader, 1.0, ArchiveVersion::V5, 0).is_err());
}

#[test]
fn sum_surface_accepts_later_minor_version_and_skips_suffix() {
    let mut bytes = valid_sum_payload();
    bytes[0] = 0x11;
    bytes.push(0xaa);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    assert!(super::read_sum(&bytes, &mut reader, 1.0, ArchiveVersion::V5, 0).is_ok());
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn revolution_major_versions_decode_child_and_scale_coordinates_once() {
    for version in [0x10, 0x20] {
        let bytes = valid_revolution_payload(version);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
        let decoded = super::read_revolution(&bytes, &mut reader, 25.4, ArchiveVersion::V5, 0)
            .expect("required invariant");
        let super::DecodedSurface::Procedural {
            geometry,
            definition,
            children,
        } = decoded
        else {
            panic!("expected procedural revolution");
        };
        assert_eq!(reader.remaining(), 0);
        assert_eq!(children.len(), 1);
        let super::DecodedProceduralSurface::Revolution {
            axis_origin,
            axis_direction,
            angular_interval,
            parameter_interval,
            transposed,
        } = definition
        else {
            panic!("expected revolution fields");
        };
        assert!((axis_origin.x - 25.4).abs() < 1.0e-12);
        assert!((axis_origin.y - 50.8).abs() < 1.0e-12);
        assert!((axis_origin.z - 76.2).abs() < 1.0e-12);
        assert_eq!(axis_direction, Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(angular_interval, [0.25, 1.25]);
        assert!(!transposed);
        assert_eq!(
            parameter_interval,
            if version == 0x10 {
                [0.25, 1.25]
            } else {
                [4.0, 9.0]
            }
        );
        let CurveGeometry::Nurbs(child) = children[0].reported_geometry() else {
            panic!("expected NURBS child");
        };
        assert_eq!(child.control_points()[0].x, 2.0 * 25.4);
        assert_eq!(geometry.u_knots()[2], parameter_interval[0]);
    }
}

#[test]
fn sum_surface_decodes_ordered_children_and_scales_once() {
    let bytes = valid_sum_payload();
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    let decoded = super::read_sum(&bytes, &mut reader, 25.4, ArchiveVersion::V5, 0)
        .expect("required invariant");
    let super::DecodedSurface::Procedural {
        geometry,
        definition,
        children,
    } = decoded
    else {
        panic!("expected procedural sum");
    };
    assert_eq!(reader.remaining(), 0);
    assert_eq!(children.len(), 2);
    let super::DecodedProceduralSurface::Sum { basepoint } = definition else {
        panic!("expected sum fields");
    };
    assert!((basepoint.x - 25.4).abs() < 1.0e-12);
    assert!((basepoint.y - 50.8).abs() < 1.0e-12);
    assert!((basepoint.z - 76.2).abs() < 1.0e-12);
    assert!((geometry.control_points()[0].x - 177.8).abs() < EPS_EXACT_GEOMETRY);
    assert!((geometry.control_points()[0].y - 50.8).abs() < EPS_EXACT_GEOMETRY);
    assert!((geometry.control_points()[0].z - 76.2).abs() < EPS_EXACT_GEOMETRY);
    let CurveGeometry::Nurbs(first) = children[0].reported_geometry() else {
        panic!("expected first NURBS child");
    };
    let CurveGeometry::Nurbs(second) = children[1].reported_geometry() else {
        panic!("expected second NURBS child");
    };
    assert_eq!(first.control_points()[0].x, 2.0 * 25.4);
    assert_eq!(second.control_points()[0].x, 4.0 * 25.4);
}

#[test]
fn sum_surface_rejects_nil_child_object() {
    for (first, second) in [
        (nil_object_wrapper(), line_wrapper(1.0)),
        (line_wrapper(1.0), nil_object_wrapper()),
    ] {
        let mut bytes = vec![0x10];
        for value in [1.0, 2.0, 3.0, -10.0, -10.0, -10.0, 10.0, 10.0, 10.0] {
            push_f64(&mut bytes, value);
        }
        bytes.extend(first);
        bytes.extend(second);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
        assert!(super::read_sum(&bytes, &mut reader, 1.0, ArchiveVersion::V5, 0).is_err());
    }
}

#[test]
fn procedural_surface_dispatch_accepts_native_legacy_and_sum_uuids() {
    let native = Uuid::from_wire([
        0xd3, 0x20, 0x62, 0xa1, 0x3b, 0x16, 0xd4, 0x11, 0x80, 0x00, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ]);
    let legacy = Uuid::from_wire([
        0xb6, 0x01, 0x84, 0x0a, 0x34, 0x4d, 0x99, 0x4b, 0x86, 0x15, 0x1b, 0x4e, 0x72, 0x3d, 0xc4,
        0xe5,
    ]);
    let sum = Uuid::from_wire([
        0x59, 0x53, 0xcd, 0xc4, 0x6d, 0x44, 0x90, 0x46, 0x9f, 0xf5, 0x29, 0x05, 0x97, 0x32, 0x47,
        0x2b,
    ]);
    for uuid in [native, legacy, sum] {
        assert!(crate::curves::supported_class(uuid));
        assert!(crate::surfaces::is_procedural_class(uuid));
    }
    assert_ne!(native.to_string(), legacy.to_string());
}
