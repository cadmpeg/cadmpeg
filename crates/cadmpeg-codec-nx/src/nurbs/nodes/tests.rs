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

/// A `B_SURFACE` or `B_CURVE` node that names its descriptor and data nodes.
fn carrier(kind: u16, index: u32, nurbs: u32, data: u32) -> Vec<u8> {
    let mut carrier = 5u32.to_be_bytes().to_vec();
    carrier.extend(pointers(&[1, 1, 1, 1, 1]));
    carrier.push(b'+');
    carrier.extend(pointers(&[nurbs, data]));
    fixed(kind, index, &carrier)
}

/// A linear `NURBS_CURVE` over the knot nodes 14 and 15.
fn curve_descriptor(index: u32, vertices: u32) -> Vec<u8> {
    let mut nurbs = 1u16.to_be_bytes().to_vec();
    nurbs.extend(2u32.to_be_bytes());
    nurbs.extend(3u16.to_be_bytes());
    nurbs.extend(2u32.to_be_bytes());
    nurbs.extend([2, 0, 0, 0, 0]);
    nurbs.extend(pointers(&[vertices, 14, 15]));
    fixed(136, index, &nurbs)
}

/// A linear `B_CURVE` whose control points follow its knot nodes instead of
/// its `CURVE_DATA` node.
fn curve_stream() -> Vec<u8> {
    curve_stream_with(&[])
}

/// The linear `B_CURVE` stream with further nodes before its knot nodes.
fn curve_stream_with(extra: &[u8]) -> Vec<u8> {
    let mut data = vec![0];
    data.extend(pointers(&[1]));
    let mut bytes = header();
    bytes.extend(carrier(134, 10, 11, 12));
    bytes.extend(curve_descriptor(11, 13));
    bytes.extend(fixed(135, 12, &data));
    bytes.extend_from_slice(extra);
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

/// A bilinear `NURBS_SURF` over the knot nodes 14 and 15.
fn surface_descriptor(index: u32, vertices: u32) -> Vec<u8> {
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
    nurbs.extend(pointers(&[vertices, 14, 14, 15, 15]));
    fixed(126, index, &nurbs)
}

/// A bilinear `B_SURFACE` over a 2 by 2 grid in u-major order.
fn surface_stream() -> Vec<u8> {
    surface_stream_with(&[])
}

/// The bilinear `B_SURFACE` stream with further nodes before its knot nodes.
fn surface_stream_with(extra: &[u8]) -> Vec<u8> {
    let mut data = reals(&[0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
    data.push(1);
    data.extend(b"BBBB????????");
    data.extend(pointers(&[1, 1, 1, 1]));
    let mut bytes = header();
    bytes.extend(carrier(124, 10, 11, 12));
    bytes.extend(surface_descriptor(11, 13));
    bytes.extend(fixed(125, 12, &data));
    bytes.extend_from_slice(extra);
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

/// A second carrier shares `data` 12, and its descriptor names the absent
/// type-45 node 23.
fn shared_data_with_missing_controls(kind: u16, descriptor: Vec<u8>) -> Vec<u8> {
    let mut extra = carrier(kind, 20, 21, 12);
    extra.extend(descriptor);
    extra
}

#[test]
fn shared_curve_data_with_unresolved_controls_resolves_neither_carrier() {
    with_decode_context(|_ctx| {
        let bytes = curve_stream_with(&shared_data_with_missing_controls(
            134,
            curve_descriptor(21, 23),
        ));
        let graph = Graph::parse_source(&bytes).unwrap();
        let parsed = super::super::parse_with_graph(&bytes, &graph);
        assert!(parsed.curves.is_empty());
        assert!(parsed.pcurves.is_empty());
        assert_eq!(parsed.unresolved_controls.len(), 2);
        assert!(parsed
            .unresolved_controls
            .iter()
            .all(|carrier| carrier.family == "B_CURVE"));
    });
}

#[test]
fn shared_surface_data_with_unresolved_controls_resolves_neither_carrier() {
    with_decode_context(|_ctx| {
        let bytes = surface_stream_with(&shared_data_with_missing_controls(
            124,
            surface_descriptor(21, 23),
        ));
        let graph = Graph::parse_source(&bytes).unwrap();
        let parsed = super::super::parse_with_graph(&bytes, &graph);
        assert!(parsed.surfaces.is_empty());
        assert_eq!(parsed.unresolved_controls.len(), 2);
        assert!(parsed
            .unresolved_controls
            .iter()
            .all(|carrier| carrier.family == "B_SURFACE"));
    });
}

/// An embedded-schema header whose node types declare their field lists at
/// first use.
fn embedded_header() -> Vec<u8> {
    let mut bytes = b"PS".to_vec();
    for field in [": TRANSMIT FILE (partition) nurbs", "SCH_999_999_13006"] {
        bytes.extend(u32::try_from(field.len()).unwrap().to_be_bytes());
        bytes.extend(field.as_bytes());
    }
    bytes.extend(200u16.to_be_bytes());
    bytes.extend(0u32.to_be_bytes());
    bytes
}

/// Insert a first-use declaration after the node type.
fn declared(node: &[u8], declaration: &[u8]) -> Vec<u8> {
    let mut bytes = node[..2].to_vec();
    bytes.extend_from_slice(declaration);
    bytes.extend_from_slice(&node[2..]);
    bytes
}

/// An edit program that replaces the single base field with a transmitted
/// variable field `name` of the one-character type code `data_type`.
fn retyped(name: &str, data_type: u8) -> Vec<u8> {
    let mut program = vec![1, b'D', b'A', u8::try_from(name.len()).unwrap()];
    program.extend(name.as_bytes());
    program.extend(0u16.to_be_bytes());
    program.extend(encoded_xmt(2));
    program.extend([1, data_type, 1, b'Z']);
    program
}

/// The linear `B_CURVE` stream with first-use declarations. The node of type
/// `edited` declares `declaration` and holds the bytes `values`, counted one
/// per byte; every other node declares its base field list.
fn embedded_curve_stream(edited: u16, declaration: &[u8], values: &[u8]) -> Vec<u8> {
    let base = [0xff];
    let mut data = vec![0];
    data.extend(pointers(&[1]));
    let mut bytes = embedded_header();
    bytes.extend(declared(&carrier(134, 10, 11, 12), &base));
    bytes.extend(declared(&curve_descriptor(11, 13), &base));
    bytes.extend(declared(&fixed(135, 12, &data), &base));
    for (kind, count, index, base_values) in [
        (127, 2, 14, vec![0, 2, 0, 2]),
        (128, 2, 15, reals(&[0.0, 1.0])),
        (45, 6, 13, reals(&[0.0, 0.0, 0.0, 0.001, 0.002, 0.003])),
    ] {
        let node = if kind == edited {
            declared(
                &counted(kind, u32::try_from(values.len()).unwrap(), index, values),
                declaration,
            )
        } else {
            declared(&counted(kind, count, index, &base_values), &base)
        };
        bytes.extend(node);
    }
    bytes.extend(END);
    bytes
}

fn embedded_curve_count(bytes: &[u8]) -> usize {
    with_decode_context(|_ctx| {
        let graph = Graph::parse_source(bytes).unwrap();
        super::super::parse_with_graph(bytes, &graph).curves.len()
    })
}

#[test]
fn lanes_require_the_declared_field_type() {
    assert_eq!(embedded_curve_count(&embedded_curve_stream(0, &[], &[])), 1);
    let mut decoded = Vec::new();
    for (kind, name, data_type, values) in [
        (
            45,
            "vertices",
            b'c',
            reals(&[0.0, 0.0, 0.0, 0.001, 0.002, 0.003]),
        ),
        (127, "mult", b'u', vec![0, 2, 0, 2]),
        (128, "knots", b'c', reals(&[0.0, 1.0])),
    ] {
        let bytes = embedded_curve_stream(kind, &retyped(name, data_type), &values);
        if embedded_curve_count(&bytes) != 0 {
            decoded.push(name);
        }
    }
    assert!(decoded.is_empty(), "{decoded:?}");
}
