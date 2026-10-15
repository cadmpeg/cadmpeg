// SPDX-License-Identifier: Apache-2.0
//! Each case builds a native analytic surface record and a linear pcurve in
//! its native chart, computes the edge vertices from the native surface
//! definition, converts the pcurve, and requires the decoded neutral surface
//! evaluated at the converted pcurve endpoints to reach those vertices.

use super::{NativeChart, SupportChart};
use crate::brep::geometry::decode_surface;
use crate::sab::{Record, Token};
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::sync::Arc;

/// Millimetre distance within which a converted endpoint reaches its vertex.
const EPS_VERTEX_MM: f64 = 1.0e-9;
const CM_TO_MM: f64 = 10.0;

fn record(head: &str, tokens: Vec<Token>) -> Record {
    Record {
        index: 1,
        name: format!("{head}-surface"),
        tokens: Arc::from(tokens),
        offset: 0,
        len: 0,
    }
}

fn sense(reversed: bool) -> Token {
    if reversed {
        Token::True
    } else {
        Token::False
    }
}

fn unit(v: [f64; 3]) -> Vector3 {
    Vector3::from(v).unit().expect("nonzero direction")
}

fn mm(point: Vector3) -> Point3 {
    Point3::new(point.x * CM_TO_MM, point.y * CM_TO_MM, point.z * CM_TO_MM)
}

fn linear_pcurve(points: [Point2; 2]) -> PcurveNurbs {
    PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        points.into(),
        None,
        false,
    )
    .expect("fixture pcurve construction admission")
    .expect("valid linear pcurve")
}

/// Convert native pcurve `ends` on `surface` and require the decoded surface
/// at the converted endpoints to reach `vertices`.
fn assert_endpoints_hit_vertices(surface: &Record, ends: [Point2; 2], vertices: [Point3; 2]) {
    let (geometry, _) = decode_surface(surface).expect("analytic surface decodes");
    let mut pcurve = linear_pcurve(ends);
    SupportChart::of_surface_record(surface.head(), &surface.tokens)
        .expect("analytic chart fields parse")
        .normalize(&mut pcurve)
        .expect("finite converted poles");
    let ctx = cadmpeg_test_support::service_decode_context();
    for (uv, vertex) in pcurve.control_points().into_iter().zip(vertices) {
        let uv = uv.get();
        let point = cadmpeg_ir::eval::decode::surface_point_solved(&ctx, &geometry, uv.u, uv.v)
            .expect("finite surface point")
            .get();
        let miss = point.distance(vertex);
        assert!(
            miss <= EPS_VERTEX_MM,
            "{}: endpoint {point:?} misses vertex {vertex:?} by {miss}",
            surface.name
        );
    }
}

fn plane_case(reverse_v: bool) {
    let origin = [1.0, -2.0, 0.5];
    let normal = [0.0, 0.0, 1.0];
    let u_dir = [0.6, 0.8, 0.0];
    let surface = record(
        "plane",
        vec![
            Token::Position(origin),
            Token::Vector3(normal),
            Token::Vector3(u_dir),
            sense(reverse_v),
            Token::False,
            Token::False,
            Token::False,
            Token::False,
        ],
    );
    let (u, n) = (unit(u_dir), unit(normal));
    let v = n.cross(u).scale(if reverse_v { -1.0 } else { 1.0 });
    let ends = [Point2::new(0.25, -1.5), Point2::new(-2.0, 3.0)];
    let vertices = ends.map(|end| mm(Vector3::from(origin) + u.scale(end.u) + v.scale(end.v)));
    assert_endpoints_hit_vertices(&surface, ends, vertices);
}

#[test]
fn forward_v_plane_second_coordinate_runs_along_normal_cross_u_dir() {
    plane_case(false);
}

#[test]
fn reverse_v_plane_second_coordinate_runs_opposite_normal_cross_u_dir() {
    plane_case(true);
}

/// A cone or cylinder record and the native point of each generator and
/// azimuth pair: `d = s·|cosine|·u_scale·first`, `ρ = r + d·sine/cosine`,
/// `P = origin + ρ(cos θ x + ratio sin θ (axis × x)) + d axis`.
fn cone_case(sine: f64, cosine: f64, ratio: f64, reverse_u: bool) {
    let origin = [0.5, 1.0, -1.5];
    let axis = [0.0, 0.6, 0.8];
    let major = [2.0, 0.0, 0.0];
    let u_scale = 1.75;
    let surface = record(
        "cone",
        vec![
            Token::Position(origin),
            Token::Vector3(axis),
            Token::Vector3(major),
            Token::Double(ratio),
            Token::False,
            Token::False,
            Token::Double(sine),
            Token::Double(cosine),
            Token::Double(u_scale),
            sense(reverse_u),
            Token::False,
            Token::False,
            Token::False,
            Token::False,
        ],
    );
    let (z, x) = (unit(axis), unit(major));
    let y = z.cross(x);
    let radius = Vector3::from(major).norm();
    let u_sign = if reverse_u { -1.0 } else { 1.0 };
    let ends = [Point2::new(0.4, 0.3), Point2::new(-0.2, 2.5)];
    let vertices = ends.map(|end| {
        let d = u_sign * cosine.abs() * u_scale * end.u;
        let rho = radius + d * sine / cosine;
        let theta = end.v;
        mm(Vector3::from(origin)
            + x.scale(rho * theta.cos())
            + y.scale(rho * ratio * theta.sin())
            + z.scale(d))
    });
    assert_endpoints_hit_vertices(&surface, ends, vertices);
}

#[test]
fn outward_cylinder_generator_runs_along_its_axis() {
    cone_case(0.0, 1.0, 1.0, false);
}

#[test]
fn inward_cylinder_generator_runs_along_its_axis() {
    cone_case(0.0, -1.0, 1.0, false);
}

#[test]
fn cone_chart_holds_for_every_sine_and_cosine_sign() {
    let (sine, cosine) = (0.5_f64, 0.75_f64.sqrt());
    for (sine, cosine) in [
        (sine, cosine),
        (-sine, cosine),
        (sine, -cosine),
        (-sine, -cosine),
    ] {
        cone_case(sine, cosine, 1.0, false);
        cone_case(sine, cosine, 0.6, false);
    }
}

#[test]
fn reversed_cone_negates_the_generator_coordinate() {
    cone_case(0.0, 1.0, 1.0, true);
    cone_case(0.5, 0.75_f64.sqrt(), 1.0, true);
}

/// A sphere record and its native point at latitude `first` and longitude
/// `second`, negated by `reverse_v`.
fn sphere_case(radius: f64, reverse_v: bool) {
    let center = [1.0, 2.0, 3.0];
    let reference = [0.0, 1.0, 0.0];
    let pole = [0.0, 0.0, 1.0];
    let surface = record(
        "sphere",
        vec![
            Token::Position(center),
            Token::Double(radius),
            Token::Vector3(reference),
            Token::Vector3(pole),
            sense(reverse_v),
            Token::False,
            Token::False,
            Token::False,
            Token::False,
        ],
    );
    let (z, x) = (unit(pole), unit(reference));
    let y = z.cross(x);
    let v_sign = if reverse_v { -1.0 } else { 1.0 };
    let ends = [Point2::new(0.3, 1.1), Point2::new(-1.2, -2.4)];
    let vertices = ends.map(|end| {
        let (latitude, longitude) = (end.u, v_sign * end.v);
        mm(Vector3::from(center)
            + (x.scale(longitude.cos()) + y.scale(longitude.sin())).scale(radius * latitude.cos())
            + z.scale(radius * latitude.sin()))
    });
    assert_endpoints_hit_vertices(&surface, ends, vertices);
}

#[test]
fn sphere_pcurve_is_latitude_first() {
    sphere_case(2.5, false);
    sphere_case(-2.5, false);
}

#[test]
fn reverse_v_sphere_negates_the_longitude() {
    sphere_case(2.5, true);
}

/// A torus record and its native point at minor angle `first` and major
/// angle `second`, negated by `reverse_v`.
fn torus_case(minor: f64, reverse_v: bool) {
    let center = [-1.0, 0.5, 2.0];
    let axis = [0.0, 1.0, 0.0];
    let reference = [1.0, 0.0, 0.0];
    let major = 4.0;
    let surface = record(
        "torus",
        vec![
            Token::Position(center),
            Token::Vector3(axis),
            Token::Double(major),
            Token::Double(minor),
            Token::Vector3(reference),
            sense(reverse_v),
            Token::False,
            Token::False,
            Token::False,
            Token::False,
        ],
    );
    let (z, x) = (unit(axis), unit(reference));
    let y = z.cross(x);
    let v_sign = if reverse_v { -1.0 } else { 1.0 };
    let ends = [Point2::new(0.7, 0.2), Point2::new(-2.1, 2.9)];
    let vertices = ends.map(|end| {
        let (minor_angle, major_angle) = (end.u, v_sign * end.v);
        mm(Vector3::from(center)
            + (x.scale(major_angle.cos()) + y.scale(major_angle.sin()))
                .scale(major + minor * minor_angle.cos())
            + z.scale(minor * minor_angle.sin()))
    });
    assert_endpoints_hit_vertices(&surface, ends, vertices);
}

#[test]
fn torus_pcurve_is_latitude_first() {
    torus_case(1.5, false);
    torus_case(-1.5, false);
}

#[test]
fn reverse_v_torus_negates_the_major_angle() {
    torus_case(1.5, true);
}

#[test]
fn embedded_support_chart_reads_the_same_fields_as_the_record() {
    let fields = vec![
        Token::Position([0.0; 3]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([1.0, 0.0, 0.0]),
        Token::Double(1.0),
        Token::True,
        Token::Double(-2.0),
        Token::False,
        Token::Double(-0.5),
        Token::Double(0.75_f64.sqrt()),
        Token::Double(1.5),
        Token::False,
    ];
    let mut embedded = vec![Token::Ident("cone".into())];
    embedded.extend(fields.iter().cloned());
    assert_eq!(
        SupportChart::embedded_at(&embedded, 0),
        SupportChart::of_surface_record("cone", &fields)
    );
    assert_eq!(
        SupportChart::embedded_at(&embedded, 0),
        Some(SupportChart::Native {
            chart: NativeChart::cone(0.75_f64.sqrt(), 1.5, false).expect("nonzero scale"),
            axis_reversed: true,
        })
    );
    let spline = [Token::Ident("spline".into())];
    assert_eq!(
        SupportChart::embedded_at(&spline, 0),
        Some(SupportChart::Canonical)
    );
    assert_eq!(SupportChart::embedded_at(&embedded[..4], 0), None);
}

#[test]
fn native_point_inverts_frame_point() {
    let native = Point2::new(0.375, -1.25);
    for chart in [
        NativeChart::Plane { reverse_v: false },
        NativeChart::Plane { reverse_v: true },
        NativeChart::cone(-0.5, 1.75, true).expect("nonzero scale"),
        NativeChart::LatitudeFirst { reverse_v: false },
        NativeChart::LatitudeFirst { reverse_v: true },
    ] {
        let back = chart.native_point(chart.frame_point(native));
        let miss = (back.u - native.u).abs().max((back.v - native.v).abs());
        assert!(miss <= EPS_VERTEX_MM, "{chart:?}: {back:?} != {native:?}");
    }
    assert_eq!(NativeChart::cone(0.0, 1.0, false), None);
    assert_eq!(NativeChart::cone(1.0, f64::INFINITY, false), None);
}

#[test]
fn chart_overflow_refuses_conversion_without_changing_the_pcurve() {
    let chart = SupportChart::Native {
        chart: NativeChart::Plane { reverse_v: false },
        axis_reversed: false,
    };
    let mut pcurve = linear_pcurve([Point2::new(f64::MAX, 0.0), Point2::new(1.0, 1.0)]);
    let original = pcurve.clone();
    assert!(chart.normalize(&mut pcurve).is_none());
    assert_eq!(pcurve, original);
}
