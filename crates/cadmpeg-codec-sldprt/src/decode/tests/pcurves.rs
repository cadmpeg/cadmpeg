// SPDX-License-Identifier: Apache-2.0
//! Derived pcurve, seam, and analytic-section decode tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_does_not_report_derived_pcurves_as_stored_geometry_loss() {
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("curve-on-surface")));
}

#[test]
fn closed_cylinder_gets_derived_seam() {
    use cadmpeg_ir::geometry::CurveGeometry;
    let f = sldprt_with_body(&closed_cylinder_body());
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.faces[0].loops.len(), 1);
    assert_eq!(result.ir().model.loops[0].coedges().len(), 4);
    assert_eq!(result.ir().model.pcurves.len(), 4);
    assert!(result
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| !coedge.pcurves.is_empty()));
    assert_eq!(result.ir().model.edges.len(), 3);
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Line { .. })));
}

#[test]
fn closed_cylinder_anchors_sentinel_vertices_to_the_surface_branch() {
    let mut body = closed_cylinder_body();
    for coedge_attr in [30u16, 31] {
        let offset = body
            .windows(4)
            .position(|window| {
                window[0..2] == [0x00, 0x11] && window[2..4] == coedge_attr.to_be_bytes()
            })
            .expect("coedge");
        body[offset + 12..offset + 14].copy_from_slice(&1u16.to_be_bytes());
    }

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let seam = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.0.contains("#seam:"))
        .expect("derived seam");
    let positions = [&seam.start, &seam.end].map(|vertex_id| {
        let vertex = decoded
            .ir()
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == *vertex_id)
            .unwrap();
        decoded
            .ir()
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .unwrap()
            .position
    });
    assert_eq!(
        positions[0],
        cadmpeg_ir::math::Point3::new(-1000.0, 0.0, 0.0)
    );
    assert_eq!(
        positions[1],
        cadmpeg_ir::math::Point3::new(-1000.0, 0.0, 1000.0)
    );
}

#[test]
fn closed_circle_edge_gets_a_derived_seam_vertex() {
    let mut body = Vec::new();
    body.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(circle_carrier(200, [1.0, 2.0, 0.0], [0.0, 0.0, 1.0], 0.5));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 1, 0, 40, false));
    body.extend(edge_use(40, 200));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.loops[0].coedges().len(), 1);
    let edge = &decoded.ir().model.edges[0];
    assert_eq!(edge.start, edge.end);
    let vertex = decoded
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == edge.start)
        .unwrap();
    let point = decoded
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .unwrap();
    assert_eq!(
        [point.position.x, point.position.y, point.position.z],
        [1500.0, 2000.0, 0.0]
    );
    assert!(matches!(
        decoded.ir().model.pcurves[0].geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Circle {
            center,
            radius: 500.0,
            y_axis: cadmpeg_ir::math::Point2 { u: 0.0, v: 1.0 },
            ..
        } if center == cadmpeg_ir::math::Point2::new(1000.0, 2000.0)
    ));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn oblique_cylinder_section_gets_an_exact_polar_harmonic_pcurve() {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let mut body = Vec::new();
    body.extend(cylinder_carrier(100, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    body.extend(ellipse_carrier(
        200,
        [0.0, 0.0, 0.0],
        [-s, 0.0, s],
        [s, 0.0, s],
        std::f64::consts::SQRT_2,
        1.0,
    ));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 50, 0, 40, false));
    body.extend(edge_use(40, 200));
    body.extend(vertex_use(50, 60));
    body.extend(world_point(60, [1.0, 0.0, 1.0]));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.pcurves.len(), 1);
    assert!(matches!(
        decoded.ir().model.pcurves[0].geometry,
        cadmpeg_ir::geometry::PcurveGeometry::PolarHarmonic {
            radial_center,
            radial_cos,
            radial_sin,
            axial_origin: 0.0,
            axial_sin: 0.0,
            ..
        } if radial_center == cadmpeg_ir::math::Point2::new(0.0, 0.0)
            && (radial_cos.u - 1000.0).abs() < 1.0e-9
            && radial_cos.v.abs() < 1.0e-9
            && radial_sin.u.abs() < 1.0e-9
            && (radial_sin.v - 1000.0).abs() < 1.0e-9
    ));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn coaxial_cone_circle_preserves_parameter_direction() {
    let mut body = Vec::new();
    body.extend(cone_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        1.0,
        std::f64::consts::FRAC_PI_4,
        [1.0, 0.0, 0.0],
    ));
    body.extend(circle_carrier(200, [0.0, 0.0, 1.0], [0.0, 0.0, -1.0], 2.0));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 50, 0, 40, false));
    body.extend(edge_use(40, 200));
    body.extend(vertex_use(50, 60));
    body.extend(world_point(60, [2.0, 0.0, 1.0]));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction } =
        decoded.ir().model.pcurves[0].geometry
    else {
        panic!("expected line pcurve");
    };
    assert!(origin.u.abs() < 1.0e-12);
    assert!((origin.v - 1000.0).abs() < 1.0e-9);
    assert_eq!(direction, cadmpeg_ir::math::Point2::new(-1.0, 0.0));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn coaxial_torus_circle_gets_constant_minor_angle_pcurve() {
    let mut body = Vec::new();
    body.extend(torus_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        2.0,
        1.0,
        [1.0, 0.0, 0.0],
    ));
    body.extend(circle_carrier(200, [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 2.0));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 50, 0, 40, false));
    body.extend(edge_use(40, 200));
    body.extend(vertex_use(50, 60));
    body.extend(world_point(60, [2.0, 0.0, 1.0]));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction } =
        decoded.ir().model.pcurves[0].geometry
    else {
        panic!("expected line pcurve");
    };
    assert!(origin.u.abs() < 1.0e-12);
    assert!((origin.v - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
    assert_eq!(direction, cadmpeg_ir::math::Point2::new(1.0, 0.0));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn sphere_patch_gets_degenerate_meridian_seam() {
    let mut cur = Cursor::new(sldprt_with_body(&sphere_patch_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.edges.len(), 4);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.points.len(), 3);
    assert_eq!(result.ir().model.loops[0].coedges().len(), 4);
    assert_eq!(result.ir().model.pcurves.len(), 4);
    let pole = result
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.0.contains("sphere-seam"))
        .expect("sphere pole pcurve");
    assert!(matches!(
        pole.geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction }
            if origin == cadmpeg_ir::math::Point2::new(0.0, std::f64::consts::FRAC_PI_2)
                && direction == cadmpeg_ir::math::Point2::new(1.0, 0.0)
    ));
    assert_eq!(pole.parameter_range, Some([0.0, std::f64::consts::TAU]));
    let seam = result
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| {
            result
                .source_fidelity()
                .annotations
                .provenance
                .get(&edge.id.0)
                .and_then(|note| note.tag.as_deref())
                == Some("derived_sphere_seam")
        })
        .expect("sphere seam");
    assert_eq!(seam.start, seam.end);
    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| seam.curve.as_ref() == Some(&curve.id))
        .expect("sphere seam curve");
    assert!(matches!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Degenerate { point }
            if point == cadmpeg_ir::math::Point3::new(0.0, 0.0, 1000.0)
    ));
    let vertex = result
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == seam.start)
        .unwrap();
    let point = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .unwrap();
    assert_eq!(
        [point.position.x, point.position.y, point.position.z],
        [0.0, 0.0, 1000.0]
    );
}

#[test]
fn existing_sphere_seam_endpoint_is_normalized_to_axis_pole() {
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&sphere_existing_seam_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let seam_curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| {
            result
                .source_fidelity()
                .annotations
                .provenance
                .get(&curve.id.0)
                .and_then(|note| note.tag.as_deref())
                == Some("derived_sphere_seam")
        })
        .expect("existing sphere seam curve");
    let seam = result
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.as_ref() == Some(&seam_curve.id))
        .expect("existing sphere seam edge");
    let vertex = result
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == seam.start)
        .expect("sphere seam pole vertex");
    let point = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .expect("sphere seam pole point");

    assert_eq!(
        point.position,
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 1000.0)
    );
}

#[test]
fn nurbs_boundary_curve_gets_isoparametric_pcurve() {
    let mut body = triangle_body();
    let bridge = body.windows(2).position(|w| w == [0x00, 0x0e]).unwrap();
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&190u16.to_be_bytes());
    body.extend(nurbs_surface_carrier(180, 181, 10));
    body.extend(linear_nurbs_curve_carrier(190, 191));
    let mut cur = Cursor::new(sldprt_with_body(&body));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.pcurves.iter().any(|pcurve| {
        result
            .source_fidelity()
            .annotations
            .provenance
            .get(&pcurve.id.0)
            .and_then(|note| note.tag.as_deref())
            == Some("derived_nurbs_isoparametric_pcurve")
    }));
}

#[test]
fn linear_nurbs_surface_boundary_gets_affine_line_pcurve() {
    let mut body = triangle_body();
    let bridge = body.windows(2).position(|w| w == [0x00, 0x0e]).unwrap();
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&192u16.to_be_bytes());
    body.extend(nurbs_surface_carrier(180, 181, 10));
    body.extend(line_carrier(190, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body.extend(bounded_curve_wrapper(
        192,
        190,
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        0.0,
        1.0,
    ));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.ir().model.pcurves.iter().any(|pcurve| {
        result
            .source_fidelity()
            .annotations
            .provenance
            .get(&pcurve.id.0)
            .and_then(|note| note.tag.as_deref())
            == Some("derived_nurbs_isoparametric_pcurve")
            && matches!(
                pcurve.geometry,
                cadmpeg_ir::geometry::PcurveGeometry::Line { direction, .. }
                    if direction.v == 0.0 && direction.u != 0.0
            )
    }));
    assert_eq!(
        result
            .ir()
            .model
            .edges
            .iter()
            .find(|edge| edge.curve.as_ref().is_some_and(|id| id.0.ends_with("#192")))
            .and_then(|edge| edge.param_range),
        Some([0.0, 1000.0])
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn bounded_planar_line_pcurve_keeps_the_curve_parameterization() {
    let mut body = triangle_body();
    let edge = body
        .windows(2)
        .position(|window| window == [0x00, 0x10])
        .unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&192u16.to_be_bytes());
    body.extend(line_carrier(190, [0.5, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body.extend(bounded_curve_wrapper(
        192,
        190,
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        -0.5,
        0.5,
    ));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result
            .ir()
            .model
            .edges
            .iter()
            .find(|edge| edge.curve.as_ref().is_some_and(|id| id.0.ends_with("#192")))
            .and_then(|edge| edge.param_range),
        Some([-500.0, 500.0])
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn rational_nurbs_surface_row_gets_isoparametric_pcurve() {
    let mut body = triangle_body();
    let end_point = body
        .windows(4)
        .position(|window| window == [0x00, 0x1d, 0x00, 0x3d])
        .expect("triangle endpoint point");
    let endpoint = [0.0_f64, 1.0, 0.0]
        .into_iter()
        .flat_map(f64::to_be_bytes)
        .collect::<Vec<_>>();
    body[end_point + 16..end_point + 40].copy_from_slice(&endpoint);
    let bridge = body.windows(2).position(|w| w == [0x00, 0x0e]).unwrap();
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&190u16.to_be_bytes());
    body.extend(rational_nurbs_surface_carrier(180, 181, 10));
    body.extend(rational_linear_nurbs_curve_carrier(190, 191));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.ir().model.pcurves.iter().any(|pcurve| {
        result
            .source_fidelity()
            .annotations
            .provenance
            .get(&pcurve.id.0)
            .and_then(|note| note.tag.as_deref())
            == Some("derived_nurbs_isoparametric_pcurve")
    }));
}
