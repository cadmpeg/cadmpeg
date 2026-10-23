// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use crate::brep::model::{CurveGeometry, ExactModel, Precision, SurfaceGeometry};
use crate::test_support::exact::{
    Payload, Stream, EDGE_CURVE, NONE, PCURVE, SINGULAR_USE, SURFACE,
};

fn parse_with(id: u32, bytes: &[u8]) -> Result<ExactModel, CodecError> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    crate::brep::parse(&ctx, id, root)
}

fn parse(stream: Stream) -> Result<ExactModel, CodecError> {
    let id = stream.id();
    parse_with(id, &stream.finish())
}

fn plane_stream(double: bool) -> Stream {
    let mut stream = Stream::new(double);
    stream.plane(
        [1.0, 2.0, 3.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    );
    stream
}

#[test]
fn stream_identity_selects_scalar_width() {
    for double in [false, true] {
        let model = parse(plane_stream(double)).unwrap();
        let expected = if double {
            Precision::Double
        } else {
            Precision::Single
        };
        assert_eq!(model.precision, expected);
        let SurfaceGeometry::Plane { origin, normal, .. } = model.faces[0].surface.geometry else {
            panic!("plane");
        };
        assert_eq!(origin, [1.0, 2.0, 3.0]);
        assert_eq!(normal, [0.0, 0.0, 1.0]);
    }
}

#[test]
fn version_must_match_stream_identity() {
    let bytes = plane_stream(false).finish();
    assert!(parse_with(0x0100_0004, &bytes).is_err());
    assert!(parse_with(0x0100_0002, &bytes).is_err());
}

#[test]
fn stream_ends_with_its_terminator() {
    assert!(parse_with(0x0100_0003, &plane_stream(false).unterminated()).is_err());
    let mut trailing = plane_stream(false).finish();
    trailing.extend_from_slice(&0_u32.to_le_bytes());
    trailing.extend_from_slice(&0_u32.to_le_bytes());
    assert!(parse_with(0x0100_0003, &trailing).is_err());
}

#[test]
fn analytic_records_read_their_geometry_scalars_in_order() {
    let mut stream = Stream::new(true);
    let counting = |start: f64, count: usize| -> Vec<f64> {
        (0..count)
            .map(|index| start + f64::from(u32::try_from(index).unwrap()))
            .collect()
    };
    for (surface_type, count) in [(2_u8, 12), (1, 14), (8, 12), (9, 21)] {
        let payload = stream
            .payload()
            .u8(surface_type)
            .u8(0x80)
            .u8(1)
            .scalars(&counting(1.0, count))
            .scalars(&[10.0, 11.0, 12.0, 13.0])
            .u8(0)
            .bytes();
        stream.record(SURFACE, &payload);
    }
    let model = parse(stream).unwrap();
    for face in &model.faces {
        assert_eq!(face.surface.u_range, Some([10.0, 11.0]));
        assert_eq!(face.surface.v_range, Some([12.0, 13.0]));
        assert!(face.surface.embedded.is_none());
    }
    let SurfaceGeometry::Cylinder { origin, x, y, axis } = model.faces[0].surface.geometry else {
        panic!("cylinder");
    };
    assert_eq!(
        [origin, x, y, axis],
        [
            [1.0, 2.0, 3.0],
            [4.0, 5.0, 6.0],
            [7.0, 8.0, 9.0],
            [10.0, 11.0, 12.0]
        ]
    );
    let SurfaceGeometry::Cone { a, b, axis, .. } = model.faces[1].surface.geometry else {
        panic!("cone");
    };
    assert_eq!((axis, a, b), ([10.0, 11.0, 12.0], 13.0, 14.0));
    let SurfaceGeometry::Sphere { z, .. } = model.faces[2].surface.geometry else {
        panic!("sphere");
    };
    assert_eq!(z, [10.0, 11.0, 12.0]);
    let SurfaceGeometry::Torus(torus) = model.faces[3].surface.geometry else {
        panic!("torus");
    };
    assert_eq!(torus.axis, [10.0, 11.0, 12.0]);
    assert_eq!(torus.minor_sin, [19.0, 20.0, 21.0]);
}

/// The §8.9 body of a bilinear surface with one pole row per u.
fn bilinear_body(payload: Payload) -> Payload {
    payload
        .u32(2)
        .u32(4)
        .scalars(&[0.0, 0.0, 1.0, 1.0])
        .u32(2)
        .u32(4)
        .scalars(&[0.0, 0.0, 2.0, 2.0])
        .u32(2)
        .u32(2)
        .scalars(&[0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0])
        .scalars(&[1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0])
}

#[test]
fn bspline_surface_reads_body_ranges_and_periodic_bits() {
    let mut stream = Stream::new(false);
    let payload = bilinear_body(stream.payload().u8(4).u8(0x90).u8(1).u8(0))
        .scalars(&[0.0, 1.0, 0.5, 1.5])
        .u8(0)
        .bytes();
    stream.record(SURFACE, &payload);
    let model = parse(stream).unwrap();
    let face = &model.faces[0].surface;
    assert_eq!(face.u_range, Some([0.0, 1.0]));
    assert_eq!(face.v_range, Some([0.5, 1.5]));
    let SurfaceGeometry::Bspline(surface) = &face.geometry else {
        panic!("B-spline");
    };
    assert!(!surface.rational);
    assert_eq!((surface.u_order, surface.v_order), (2, 2));
    assert_eq!((surface.u_count, surface.v_count), (2, 2));
    assert_eq!(surface.v_knots, [0.0, 0.0, 2.0, 2.0]);
    assert_eq!(surface.poles[2], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(surface.periodic, [false, true]);
}

#[test]
fn pole_grid_must_follow_the_knots() {
    let mut stream = Stream::new(false);
    let payload = stream
        .payload()
        .u8(4)
        .u8(0)
        .u8(0)
        .u8(0)
        .u32(2)
        .u32(4)
        .scalars(&[0.0, 0.0, 1.0, 1.0])
        .u32(2)
        .u32(4)
        .scalars(&[0.0, 0.0, 1.0, 1.0])
        .u32(3)
        .u32(2)
        .scalars(&[0.0; 24])
        .u8(0)
        .bytes();
    stream.record(SURFACE, &payload);
    assert!(parse(stream).is_err());
}

#[test]
fn analytic_record_tail_can_hold_an_embedded_bspline() {
    let mut stream = Stream::new(true);
    let head = stream
        .payload()
        .u8(5)
        .u8(0)
        .u8(0)
        .scalars(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
        .u8(4)
        .u8(0)
        .u8(0)
        .u8(0);
    let payload = bilinear_body(head).bytes();
    stream.record(SURFACE, &payload);
    let model = parse(stream).unwrap();
    let embedded = model.faces[0].surface.embedded.as_ref().unwrap();
    assert_eq!(embedded.u_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(embedded.periodic, [false, false]);
}

#[test]
fn undefined_surface_type_is_kept_unread() {
    let mut stream = Stream::new(false);
    stream.record(SURFACE, &[3, 0, 0, 7, 7, 7]);
    let model = parse(stream).unwrap();
    assert_eq!(
        model.faces[0].surface.geometry,
        SurfaceGeometry::Undefined { surface_type: 3 }
    );
}

#[test]
fn loops_group_and_collect_their_coedges() {
    let mut stream = plane_stream(false);
    stream.loop_record(-2).coedge(0, 1, 0, NONE).terminator();
    stream.loop_record(-2).coedge(0, 0, NONE, 0).terminator();
    stream.loop_record(0).terminator();
    stream.line_edge([0.0; 3], [1.0, 0.0, 0.0]);
    stream.line_pcurve([0.0; 2], [1.0, 0.0]);
    let model = parse(stream).unwrap();
    assert_eq!(model.faces[0].loops, [0, 1, 2]);
    let groups: Vec<usize> = model.loops.iter().map(|record| record.group).collect();
    assert_eq!(groups, [0, 1, 0]);
    let coedge = model.loops[0].coedges[0];
    assert_eq!(
        (
            coedge.edge,
            coedge.orientation,
            coedge.pcurve,
            coedge.secondary
        ),
        (Some(0), 1, Some(0), None)
    );
    assert_eq!(model.loops[1].coedges[0].secondary, Some(0));
}

#[test]
fn loop_marker_names_an_earlier_group_start_of_its_face() {
    for marker in [5, -1] {
        let mut stream = plane_stream(false);
        stream.loop_record(marker).terminator();
        assert!(parse(stream).is_err(), "marker {marker}");
    }
    let mut stream = plane_stream(false);
    stream.loop_record(-2).terminator();
    stream.plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    stream.loop_record(0).terminator();
    assert!(parse(stream).is_err());
}

#[test]
fn coedge_outside_a_loop_is_malformed() {
    let mut stream = plane_stream(false);
    stream.coedge(NONE, 0, NONE, NONE);
    assert!(parse(stream).is_err());
}

#[test]
fn singular_use_opens_a_scope_that_holds_a_tag_0a_record() {
    let mut stream = plane_stream(false);
    let singular = stream.payload().u32(0).scalar(0.0).scalar(1.0).bytes();
    stream.record(SINGULAR_USE, &singular);
    let opaque = stream.payload().u32(0).u32(1).u32(7).bytes();
    stream.record(0x0a, &opaque).terminator();
    stream.loop_record(-2).coedge(0, 1, NONE, NONE).terminator();
    stream.line_edge([0.0; 3], [0.0; 3]);
    let model = parse(stream).unwrap();
    let singular = model.faces[0].singular_uses[0];
    assert_eq!((singular.edge, singular.range), (0, [0.0, 1.0]));
    assert_eq!(model.opaque.len(), 1);
    assert_eq!((model.opaque[0].tag, model.opaque[0].length), (0x0a, 12));
    assert_eq!(model.loops.len(), 1);
}

#[test]
fn edge_and_pcurve_records_interleave_after_the_faces() {
    let mut stream = plane_stream(false);
    stream.line_edge([0.0; 3], [1.0, 0.0, 0.0]);
    stream.line_pcurve([0.0; 2], [1.0, 0.0]);
    stream.line_edge([0.0; 3], [0.0, 1.0, 0.0]);
    let model = parse(stream).unwrap();
    assert_eq!((model.edges.len(), model.pcurves.len()), (2, 1));
    let mut late = plane_stream(false);
    late.line_edge([0.0; 3], [1.0, 0.0, 0.0]);
    late.plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    assert!(parse(late).is_err());
}

#[test]
fn indexes_stay_inside_their_sections() {
    let mut stream = plane_stream(false);
    stream.loop_record(-2).coedge(1, 1, NONE, NONE).terminator();
    stream.line_edge([0.0; 3], [1.0, 0.0, 0.0]);
    assert!(parse(stream).is_err());
    let mut composite = plane_stream(false);
    let payload = composite.payload().u8(2).u8(0).u32(1).u32(3).u8(0).bytes();
    composite.record(EDGE_CURVE, &payload);
    assert!(parse(composite).is_err());
}

#[test]
fn edge_curve_records_follow_their_layouts() {
    let mut stream = plane_stream(true);
    let circle = stream
        .payload()
        .u8(1)
        .u8(0x08)
        .scalars(&[0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.5, 1.5])
        .u8(0)
        .bytes();
    let full_circle = stream
        .payload()
        .u8(1)
        .u8(0x02)
        .scalars(&[0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0, 0.0])
        .u8(0)
        .bytes();
    let point = stream
        .payload()
        .u8(3)
        .u8(0)
        .scalars(&[1.0, 2.0, 3.0])
        .u8(0)
        .bytes();
    let composite = stream
        .payload()
        .u8(2)
        .u8(0)
        .u32(2)
        .u32(0)
        .u32(1)
        .u8(0)
        .bytes();
    let spline = stream
        .payload()
        .u8(6)
        .u8(0)
        .u8(1)
        .u32(2)
        .u32(4)
        .scalars(&[0.0, 0.0, 1.0, 1.0])
        .u32(2)
        .scalars(&[0.0, 0.0, 0.0, 1.0, 2.0, 0.0, 0.0, 2.0])
        .u8(0)
        .bytes();
    let polyline = stream
        .payload()
        .u8(7)
        .u8(0)
        .u32(2)
        .scalars(&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0])
        .u8(0)
        .bytes();
    for payload in [
        &circle,
        &full_circle,
        &point,
        &composite,
        &spline,
        &polyline,
    ] {
        stream.record(EDGE_CURVE, payload);
    }
    let model = parse(stream).unwrap();
    let CurveGeometry::Circle(conic) = &model.edges[0].geometry else {
        panic!("circle");
    };
    assert_eq!(conic.range, Some([0.5, 1.5]));
    let CurveGeometry::Circle(conic) = &model.edges[1].geometry else {
        panic!("circle");
    };
    assert_eq!(conic.range, None);
    assert_eq!(
        model.edges[2].geometry,
        CurveGeometry::Point([1.0, 2.0, 3.0])
    );
    assert_eq!(
        model.edges[3].geometry,
        CurveGeometry::Composite(vec![0, 1])
    );
    let CurveGeometry::Bspline(curve) = &model.edges[4].geometry else {
        panic!("B-spline");
    };
    assert!(curve.rational);
    assert_eq!(curve.weighted, [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
    assert_eq!(curve.weights, [1.0, 2.0]);
    assert_eq!(
        model.edges[5].geometry,
        CurveGeometry::Polyline(vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]])
    );
}

#[test]
fn composite_edge_curve_ends_with_its_end_byte() {
    let mut stream = plane_stream(false);
    let payload = stream.payload().u8(2).u8(0).u32(0).bytes();
    stream.record(EDGE_CURVE, &payload);
    assert!(parse(stream).is_err());
}

#[test]
fn pcurve_records_follow_their_layouts() {
    let mut stream = plane_stream(false);
    let ellipse = stream
        .payload()
        .u8(4)
        .u8(0x08)
        .scalars(&[1.0, 1.0, 2.0, 0.0, 0.0, 1.0, 0.0, 3.0])
        .u8(0)
        .bytes();
    let spline = stream
        .payload()
        .u8(6)
        .u8(0x08)
        .u8(0)
        .u32(2)
        .u32(4)
        .scalars(&[0.0, 0.0, 1.0, 1.0])
        .u32(2)
        .scalars(&[0.0, 0.0, 1.0, 1.0, 1.0, 1.0])
        .scalars(&[0.25, 0.75])
        .u8(0)
        .bytes();
    let polyline = stream
        .payload()
        .u8(7)
        .u8(0x04)
        .u32(3)
        .scalars(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0])
        .u8(0)
        .bytes();
    let circle = stream
        .payload()
        .u8(1)
        .u8(0)
        .scalars(&[0.0; 6])
        .u8(0)
        .bytes();
    for payload in [&ellipse, &spline, &polyline, &circle] {
        stream.record(PCURVE, payload);
    }
    let model = parse(stream).unwrap();
    let CurveGeometry::Ellipse(conic) = &model.pcurves[0].geometry else {
        panic!("ellipse");
    };
    assert_eq!(
        (conic.center, conic.y, conic.range),
        ([1.0, 1.0], [0.0, 1.0], Some([0.0, 3.0]))
    );
    let CurveGeometry::Bspline(curve) = &model.pcurves[1].geometry else {
        panic!("B-spline");
    };
    assert_eq!(curve.range, Some([0.25, 0.75]));
    assert_eq!(curve.weighted, [[0.0, 0.0], [1.0, 1.0]]);
    assert!(
        matches!(&model.pcurves[2].geometry, CurveGeometry::Polyline(points) if points.len() == 3)
    );
    assert_eq!(
        model.pcurves[3].geometry,
        CurveGeometry::Undefined { curve_type: 1 }
    );
}

#[test]
fn single_precision_singular_use_is_twelve_bytes() {
    let mut stream = plane_stream(false);
    let payload = stream.payload().u32(0).scalar(0.25).scalar(0.75).bytes();
    assert_eq!(payload.len(), 12);
    stream.record(SINGULAR_USE, &payload).terminator();
    stream.line_edge([0.0; 3], [0.0; 3]);
    let model = parse(stream).unwrap();
    assert_eq!(model.faces[0].singular_uses[0].range, [0.25, 0.75]);
}

#[test]
fn record_counts_cannot_exceed_their_payload() {
    let mut stream = plane_stream(false);
    let payload = stream.payload().u8(7).u8(0).u32(1_000_000).u8(0).bytes();
    stream.record(PCURVE, &payload);
    assert!(parse(stream).is_err());
}
