// SPDX-License-Identifier: Apache-2.0

use crate::test_support::{test_bytes::encoded_xmt, with_decode_context};
use crate::topology::Graph;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};

const END: [u8; 4] = [0, 1, 0, 1];
/// Millimetre coordinates converted from exact metre fixtures.
const COORDINATE_TOLERANCE_MM: f64 = 1e-12;

fn header() -> Vec<u8> {
    let mut bytes = b"PS".to_vec();
    for field in [": TRANSMIT FILE (partition) nurbs", "SCH_999_13006"] {
        bytes.extend(u32::try_from(field.len()).unwrap().to_be_bytes());
        bytes.extend(field.as_bytes());
    }
    bytes.extend(0u32.to_be_bytes());
    bytes
}

fn fixed(kind: u16, index: u32, fields: &[u8]) -> Vec<u8> {
    let mut bytes = kind.to_be_bytes().to_vec();
    bytes.extend(encoded_xmt(index));
    bytes.extend_from_slice(fields);
    bytes
}

fn counted(kind: u16, count: u32, index: u32, values: &[u8]) -> Vec<u8> {
    let mut bytes = kind.to_be_bytes().to_vec();
    bytes.extend(count.to_be_bytes());
    bytes.extend(encoded_xmt(index));
    bytes.extend_from_slice(values);
    bytes
}

fn pointers(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| encoded_xmt(*value))
        .collect()
}

fn reals(values: &[f64]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect()
}

/// A linear `B_CURVE` whose control points follow its knot nodes instead of
/// its `CURVE_DATA` node.
fn curve_stream() -> Vec<u8> {
    let mut curve = 5u32.to_be_bytes().to_vec();
    curve.extend(pointers(&[1, 1, 1, 1, 1]));
    curve.push(b'+');
    curve.extend(pointers(&[11, 12]));
    let mut nurbs = 1u16.to_be_bytes().to_vec();
    nurbs.extend(2u32.to_be_bytes());
    nurbs.extend(3u16.to_be_bytes());
    nurbs.extend(2u32.to_be_bytes());
    nurbs.extend([2, 0, 0, 0, 0]);
    nurbs.extend(pointers(&[13, 14, 15]));
    let mut data = vec![0];
    data.extend(pointers(&[1]));
    let mut bytes = header();
    bytes.extend(fixed(134, 10, &curve));
    bytes.extend(fixed(136, 11, &nurbs));
    bytes.extend(fixed(135, 12, &data));
    bytes.extend(counted(127, 2, 14, &[0, 2, 0, 2]));
    bytes.extend(counted(128, 2, 15, &reals(&[0.0, 1.0])));
    bytes.extend(counted(
        45,
        6,
        13,
        &reals(&[0.0, 0.0, 0.0, 0.001, 0.002, 0.003]),
    ));
    bytes.extend(END);
    bytes
}

#[test]
fn terminated_control_points_join_by_reference() {
    with_decode_context(|_ctx| {
        let bytes = curve_stream();
        let graph = Graph::parse_source(&bytes).unwrap();
        let parsed = super::super::parse_with_graph(&bytes, &graph);
        assert_eq!(parsed.curves.len(), 1);
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = &parsed.curves[0].geometry
        else {
            panic!("expected a NURBS curve");
        };
        assert_eq!(curve.degree(), 1);
        let points = curve.control_points();
        assert_eq!(points.len(), 2);
        assert!((points[1].x - 1.0).abs() < COORDINATE_TOLERANCE_MM);
        assert!((points[1].z - 3.0).abs() < COORDINATE_TOLERANCE_MM);
    });
}

/// A bilinear `B_SURFACE` over a 2 by 2 grid in u-major order.
fn surface_stream() -> Vec<u8> {
    let mut surface = 5u32.to_be_bytes().to_vec();
    surface.extend(pointers(&[1, 1, 1, 1, 1]));
    surface.push(b'+');
    surface.extend(pointers(&[11, 12]));
    let mut nurbs = vec![0, 0];
    nurbs.extend(1u16.to_be_bytes());
    nurbs.extend(1u16.to_be_bytes());
    nurbs.extend(2u32.to_be_bytes());
    nurbs.extend(2u32.to_be_bytes());
    nurbs.extend([2, 2]);
    nurbs.extend(2u32.to_be_bytes());
    nurbs.extend(2u32.to_be_bytes());
    nurbs.extend([0, 0, 0, 0]);
    nurbs.extend(3u16.to_be_bytes());
    nurbs.extend(pointers(&[13, 14, 14, 15, 15]));
    let mut data = reals(&[0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
    data.push(1);
    data.extend(b"BBBB????????");
    data.extend(pointers(&[1, 1, 1, 1]));
    let mut bytes = header();
    bytes.extend(fixed(124, 10, &surface));
    bytes.extend(fixed(126, 11, &nurbs));
    bytes.extend(fixed(125, 12, &data));
    bytes.extend(counted(127, 2, 14, &[0, 2, 0, 2]));
    bytes.extend(counted(128, 2, 15, &reals(&[0.0, 1.0])));
    bytes.extend(counted(
        45,
        12,
        13,
        &reals(&[
            0.0, 0.0, 0.0, 0.0, 0.001, 0.0, 0.001, 0.0, 0.0, 0.001, 0.001, 0.002,
        ]),
    ));
    bytes.extend(END);
    bytes
}

#[test]
fn terminated_surface_grid_joins_by_reference() {
    with_decode_context(|_ctx| {
        let bytes = surface_stream();
        let graph = Graph::parse_source(&bytes).unwrap();
        let parsed = super::super::parse_with_graph(&bytes, &graph);
        assert_eq!(parsed.surfaces.len(), 1);
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) =
            &parsed.surfaces[0].geometry
        else {
            panic!("expected a NURBS surface");
        };
        let poles = surface.poles();
        assert_eq!(poles.len(), 4);
        assert!((poles[1].y - 1.0).abs() < COORDINATE_TOLERANCE_MM);
        assert!((poles[2].x - 1.0).abs() < COORDINATE_TOLERANCE_MM);
        assert!((poles[3].z - 2.0).abs() < COORDINATE_TOLERANCE_MM);
    });
}
