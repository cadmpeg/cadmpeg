// SPDX-License-Identifier: Apache-2.0
//! Edge-curve decode and rejected-loop topology tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn edge_uses_decoded_line_curve() {
    use cadmpeg_ir::geometry::CurveGeometry;

    let mut body = Vec::new();
    body.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 70)); // curve = line carrier 70
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));

    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.curves.len(), 1);
    match &result.ir().model.curves[0].geometry {
        CurveGeometry::Line { direction, .. } => assert_eq!(direction.x, 1.0),
        other => panic!("expected line, got {other:?}"),
    }
    assert_eq!(
        result
            .ir()
            .model
            .edges
            .iter()
            .filter(|e| e.curve.is_some())
            .count(),
        1
    );
    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert!(result
        .ir()
        .model
        .coedges
        .iter()
        .any(|coedge| !coedge.pcurves.is_empty()));
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn edge_uses_decode_nurbs_curve() {
    use cadmpeg_ir::geometry::CurveGeometry;

    let mut body = triangle_body();
    body.extend(nurbs_curve_carrier(170, 171));
    let edge = body
        .windows(2)
        .position(|w| w == [0x00, 0x10])
        .expect("edge-use");
    body[edge + 24..edge + 26].copy_from_slice(&170u16.to_be_bytes());
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    let nurbs = result
        .ir()
        .model
        .curves
        .iter()
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => Some(nurbs),
            _ => None,
        })
        .expect("NURBS curve");
    assert_eq!(nurbs.degree(), 2);
    assert_eq!(nurbs.control_points().len(), 3);
    assert_eq!(nurbs.knots(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
}

#[test]
fn edge_uses_decode_typed_reference_nurbs_curve() {
    use cadmpeg_ir::geometry::CurveGeometry;

    let mut body = triangle_body();
    body.extend(typed_nurbs_curve_carrier(170, 171));
    let edge = body
        .windows(2)
        .position(|window| window == [0x00, 0x10])
        .expect("edge-use");
    body[edge + 24..edge + 26].copy_from_slice(&170u16.to_be_bytes());

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let nurbs = result
        .ir()
        .model
        .curves
        .iter()
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => Some(nurbs),
            _ => None,
        })
        .expect("NURBS curve");
    assert_eq!(nurbs.degree(), 2);
    assert_eq!(nurbs.control_points().len(), 3);
}

#[test]
fn reused_carrier_attribute_resolves_by_geometry_kind() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    let mut body = triangle_body();
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&70u16.to_be_bytes());
    let edge = body
        .windows(2)
        .position(|window| window == [0x00, 0x10])
        .expect("edge-use");
    body[edge + 24..edge + 26].copy_from_slice(&70u16.to_be_bytes());
    body.extend(line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body.extend(plane_carrier(
        70,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(matches!(
        result.ir().model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
}

#[test]
fn false_later_loop_candidate_does_not_replace_owned_loop() {
    let mut body = triangle_body();
    body.extend(loop_head(20, 30, 999));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.loops[0].id.0, "sldprt:brep:loop#20");
}

#[test]
fn decode_removes_edges_and_vertices_from_a_rejected_loop() {
    let mut body = triangle_body();
    body.extend(plane_carrier(
        200,
        [2.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(bridge(110, 120, 200));
    body.extend(loop_head(120, 130, 110));
    body.extend(coedge(130, 120, 131, 150, 0, 140, false));
    body.extend(coedge(131, 120, 132, 151, 0, 141, false));
    body.extend(coedge(132, 120, 130, 152, 0, 142, false));
    body.extend(edge_use(140, 0));
    body.extend(edge_use(141, 0));
    body.extend(edge_use(142, 0));
    body.extend(vertex_use(150, 160));
    body.extend(vertex_use(151, 161));
    body.extend(vertex_use(152, 162));
    body.extend(world_point(160, [2.0, 0.0, 0.0]));
    body.extend(world_point(161, [3.0, 0.0, 0.0]));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.points.len(), 3);
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}
