// SPDX-License-Identifier: Apache-2.0
//! Boundaries from pcurve uses: pcurve-only edges, pcurves witnessed by
//! their 3D curves, a pcurve chain that leaves its curve, a closed coedge on
//! a decreasing use interval, a closed coedge whose image starts at a corner,
//! a pcurve chain listed from its end to its start, a seam-cut cylinder side
//! and a cone tip closed by a pole edge.

use std::f64::consts::{PI, TAU};

use cadmpeg_ir::ids::{EdgeId, FaceId, VertexId};
use cadmpeg_ir::topology::{BodyKind, Sense};

use crate::reason::{Category, CaveatCode, UnavailableReason};
use crate::report::{FaceMeasures, MeasureReport, UvSource};
use crate::select::Selection;
use crate::test_support::builders::{
    assert_measured, assert_tight_box, measure, square_with_split_side, Builder, Frame,
};
use crate::value::Quantity;

fn everything() -> Selection {
    Selection {
        all_faces: true,
        all_edges: true,
        ..Selection::default()
    }
}

fn only_face(report: &MeasureReport) -> &FaceMeasures {
    let [face] = report.faces().as_slice() else {
        panic!("one face row");
    };
    face
}

fn uv_sources(face: &FaceMeasures) -> Vec<Option<UvSource>> {
    face.measures()
        .loops()
        .iter()
        .flat_map(|entry| entry.coedges().iter().map(|coedge| *coedge.uv_source()))
        .collect()
}

/// The corners of the unit square at `z = 0`, counterclockwise.
fn corners(builder: &mut Builder) -> [VertexId; 4] {
    [
        builder.vertex([0.0, 0.0, 0.0]),
        builder.vertex([1.0, 0.0, 0.0]),
        builder.vertex([1.0, 1.0, 0.0]),
        builder.vertex([0.0, 1.0, 0.0]),
    ]
}

/// The unit square face on the plane `z = 0` over `sides`, each forward.
fn square_face(builder: &mut Builder, sides: Vec<EdgeId>) -> FaceId {
    let surface = builder.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    builder.face(
        &surface,
        Sense::Forward,
        &[sides
            .into_iter()
            .map(|edge| (edge, Sense::Forward))
            .collect()],
        true,
    )
}

/// Line pcurves of the unit square's sides in traversal order.
const SIDES: [([f64; 2], [f64; 2]); 4] = [
    ([0.0, 0.0], [1.0, 0.0]),
    ([1.0, 0.0], [0.0, 1.0]),
    ([1.0, 1.0], [-1.0, 0.0]),
    ([0.0, 1.0], [0.0, -1.0]),
];

#[test]
fn pcurve_only_edges_bound_the_face_and_trace_their_lengths() {
    let mut builder = Builder::new();
    let [a, b, c, d] = corners(&mut builder);
    let sides = vec![
        builder.bare_edge(&a, &b),
        builder.bare_edge(&b, &c),
        builder.bare_edge(&c, &d),
        builder.bare_edge(&d, &a),
    ];
    let face = square_face(&mut builder, sides);
    for (position, (origin, direction)) in SIDES.into_iter().enumerate() {
        if position == 1 {
            builder.line_pcurve(&face, [0, position], [1.0, 1.0], [0.0, -1.0], [0.0, 1.0]);
        } else {
            builder.line_pcurve(&face, [0, position], origin, direction, [0.0, 1.0]);
        }
    }
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &everything());
    let face = only_face(&report);
    assert_measured(face.measures().area(), 1.0, 1.0);
    assert_measured(face.measures().boundary_length(), 4.0, 4.0);
    assert_tight_box(face.measures().bbox(), [0.0; 3], [1.0, 1.0, 0.0], 1.0);
    assert!(uv_sources(face)
        .iter()
        .all(|source| *source == Some(UvSource::Pcurve)));
    assert_eq!(report.edges().len(), 4);
    for edge in report.edges() {
        assert_measured(edge.length(), 1.0, 1.0);
        assert!(!edge.degenerate());
    }
}

#[test]
fn a_stale_use_range_is_recovered_from_the_vertices() {
    let mut builder = Builder::new();
    let [a, b, c, d] = corners(&mut builder);
    let sides = vec![
        builder.bare_edge(&a, &b),
        builder.bare_edge(&b, &c),
        builder.bare_edge(&c, &d),
        builder.bare_edge(&d, &a),
    ];
    let face = square_face(&mut builder, sides);
    for (position, (origin, direction)) in SIDES.into_iter().enumerate() {
        let range = if position == 0 {
            [5.0, 6.0]
        } else {
            [0.0, 1.0]
        };
        builder.line_pcurve(&face, [0, position], origin, direction, range);
    }
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &everything());
    let face = only_face(&report);
    assert_measured(face.measures().area(), 1.0, 1.0);
    assert_measured(face.measures().boundary_length(), 4.0, 4.0);
}

#[test]
fn pcurves_witnessed_by_their_curves_resolve_the_face() {
    let mut builder = Builder::new();
    let [a, b, c, d] = corners(&mut builder);
    let sides = vec![
        builder.line(&a, &b),
        builder.line(&b, &c),
        builder.line(&c, &d),
        builder.line(&d, &a),
    ];
    let face = square_face(&mut builder, sides);
    for (position, (origin, direction)) in SIDES.into_iter().enumerate() {
        builder.line_pcurve(&face, [0, position], origin, direction, [0.0, 1.0]);
    }
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &everything());
    let face = only_face(&report);
    assert_measured(face.measures().area(), 1.0, 1.0);
    assert!(uv_sources(face)
        .iter()
        .all(|source| *source == Some(UvSource::Pcurve)));
}

#[test]
fn a_pcurve_chain_that_leaves_its_curve_is_a_mismatch() {
    let mut builder = Builder::new();
    let [a, b, c, d] = corners(&mut builder);
    let sides = vec![
        builder.line(&a, &b),
        builder.line(&b, &c),
        builder.line(&c, &d),
        builder.line(&d, &a),
    ];
    let face = square_face(&mut builder, sides);
    builder.line_pcurve(&face, [0, 0], [0.0, 0.0], [0.5, 0.5], [0.0, 1.0]);
    builder.line_pcurve(&face, [0, 0], [0.5, 0.5], [0.5, -0.5], [0.0, 1.0]);
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &everything());
    let face = only_face(&report);
    let Quantity::Unavailable(area) = face.measures().area() else {
        panic!("the area is unavailable");
    };
    assert_eq!(
        area.reason(),
        UnavailableReason::BoundaryPcurveCurveMismatch
    );
    assert_eq!(area.category(), Category::Input);
    let coedge = &face.measures().loops()[0].coedges()[0];
    assert_eq!(area.entity(), Some(coedge.coedge().as_str()));
}

#[test]
fn a_seam_cut_cylinder_side_resolves_from_its_pcurves() {
    let radius = 10.0;
    let height = 30.0;
    let mut builder = Builder::new();
    let low = builder.vertex([radius, 0.0, 0.0]);
    let high = builder.vertex([radius, 0.0, height]);
    let bottom = builder.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], radius, &low);
    let top = builder.circle(
        [0.0, 0.0, height],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        radius,
        &high,
    );
    let seam = builder.line(&low, &high);
    let surface = builder.cylinder(Frame::z_up([0.0; 3]), radius);
    let face = builder.face(
        &surface,
        Sense::Forward,
        &[vec![
            (bottom, Sense::Forward),
            (seam.clone(), Sense::Forward),
            (top, Sense::Reversed),
            (seam, Sense::Reversed),
        ]],
        true,
    );
    builder.line_pcurve(&face, [0, 0], [0.0, 0.0], [1.0, 0.0], [0.0, TAU]);
    builder.line_pcurve(&face, [0, 1], [TAU, 0.0], [0.0, 1.0], [0.0, height]);
    builder.line_pcurve(&face, [0, 2], [0.0, height], [1.0, 0.0], [0.0, TAU]);
    builder.line_pcurve(&face, [0, 3], [0.0, 0.0], [0.0, 1.0], [0.0, height]);
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &everything());
    let face = only_face(&report);
    let measures = face.measures();
    assert_measured(
        measures.area(),
        2.0 * PI * radius * height,
        2.0 * radius * height,
    );
    assert_measured(measures.seam_length(), height, height);
    let sweep = measures
        .angular_sweep()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("sweep");
    assert!(sweep.value().full);
    assert_eq!(*face.orientation().consistent(), Some(true));
    assert!(uv_sources(face)
        .iter()
        .all(|source| *source == Some(UvSource::Pcurve)));
}

#[test]
fn a_cone_tip_closes_through_its_pole_edge() {
    let mut builder = Builder::new();
    let rim = builder.vertex([10.0, 0.0, 20.0]);
    let apex = builder.vertex([0.0, 0.0, 0.0]);
    let circle = builder.circle(
        [0.0, 0.0, 20.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        10.0,
        &rim,
    );
    let seam = builder.line(&rim, &apex);
    let pole = builder.bare_edge(&apex, &apex);
    let surface = builder.cone(Frame::z_up([0.0, 0.0, 10.0]), 5.0, 0.5_f64.atan());
    let face = builder.face(
        &surface,
        Sense::Forward,
        &[vec![
            (circle, Sense::Reversed),
            (seam.clone(), Sense::Forward),
            (pole, Sense::Forward),
            (seam, Sense::Reversed),
        ]],
        true,
    );
    builder.line_pcurve(&face, [0, 0], [0.0, 10.0], [1.0, 0.0], [0.0, TAU]);
    builder.line_pcurve(&face, [0, 1], [0.0, 10.0], [0.0, -1.0], [0.0, 20.0]);
    builder.line_pcurve(&face, [0, 2], [0.0, -10.0], [1.0, 0.0], [0.0, TAU]);
    builder.line_pcurve(&face, [0, 3], [TAU, 10.0], [0.0, -1.0], [0.0, 20.0]);
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &everything());
    let face = only_face(&report);
    let measures = face.measures();
    let slant = 10.0 * 5.0_f64.sqrt();
    assert_measured(measures.area(), PI * 10.0 * slant, 400.0);
    assert_measured(measures.boundary_length(), 20.0 * PI, 20.0);
    assert_measured(measures.seam_length(), slant, slant);
    let sources = uv_sources(face);
    assert_eq!(sources[2], Some(UvSource::Pole));
    let pole_edge = report
        .edges()
        .iter()
        .find(|edge| *edge.degenerate())
        .expect("a degenerate edge row");
    assert_measured(pole_edge.length(), 0.0, 1.0);
}

/// A reversed closed coedge whose use runs its counterclockwise circle
/// pcurve over the decreasing interval `[2π, 0]` follows the coedge: the
/// disk bounded clockwise about `+z` is the face of the `−z` oriented normal.
#[test]
fn a_closed_coedge_follows_its_pcurve_over_a_decreasing_use_interval() {
    let radius = 2.0;
    let mut builder = Builder::new();
    let vertex = builder.vertex([radius, 0.0, 0.0]);
    let edge = builder.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], radius, &vertex);
    let surface = builder.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let face = builder.face(
        &surface,
        Sense::Reversed,
        &[vec![(edge, Sense::Reversed)]],
        true,
    );
    builder.circle_pcurve(&face, [0, 0], [0.0, 0.0], radius, [TAU, 0.0]);
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &everything());
    let face = only_face(&report);
    assert_measured(
        face.measures().area(),
        PI * radius * radius,
        radius * radius,
    );
    assert!(uv_sources(face)
        .iter()
        .all(|source| *source == Some(UvSource::Pcurve)));
}

/// A reversed closed coedge on the counterclockwise unit-square polyline,
/// whose polyline pcurve runs in the edge direction with no use interval,
/// starts at a corner: the image leaves the vertex across the 3D tangent in
/// its forward reading and along it in its backward reading. The coedge
/// reads the pcurve backward, and the clockwise square about `+z` bounds the
/// face of the `−z` oriented normal.
#[test]
fn a_closed_coedge_starting_at_a_corner_reads_its_pcurve_backward() {
    let corners = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let mut builder = Builder::new();
    let vertex = builder.vertex([0.0; 3]);
    let edge = builder.closed_polyline(&corners.map(|[x, y]| [x, y, 0.0]), &vertex);
    let surface = builder.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let face = builder.face(
        &surface,
        Sense::Reversed,
        &[vec![(edge, Sense::Reversed)]],
        true,
    );
    builder.closed_polyline_pcurve(&face, [0, 0], &corners, None);
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &everything());
    let face = only_face(&report);
    assert_measured(face.measures().area(), 1.0, 1.0);
    assert!(uv_sources(face)
        .iter()
        .all(|source| *source == Some(UvSource::Pcurve)));
}

/// A coedge's pcurve uses form one chain from its start to its end in
/// traversal order (`docs/cad-ir.md`). The chain listed from its end to its
/// start measures the same face and adds the `boundary.pcurve-order` caveat
/// on that coedge; the chain in traversal order adds none.
#[test]
fn a_pcurve_chain_listed_backward_is_a_face_caveat() {
    for listed_backward in [false, true] {
        let (ir, coedge, _) = square_with_split_side(listed_backward);
        let report = measure(&ir, &everything());
        let face = only_face(&report);
        assert_measured(face.measures().area(), 1.0, 1.0);
        assert_measured(face.measures().boundary_length(), 4.0, 4.0);
        let caveats: Vec<_> = face
            .caveats()
            .iter()
            .filter(|caveat| caveat.code() == CaveatCode::BoundaryPcurveOrder)
            .collect();
        if listed_backward {
            let [caveat] = caveats.as_slice() else {
                panic!("one pcurve-order caveat");
            };
            assert_eq!(caveat.entity(), Some(coedge.as_str()));
            assert_eq!(caveat.category(), Category::Input);
        } else {
            assert!(
                caveats.is_empty(),
                "a chain in traversal order has no caveat"
            );
        }
    }
}
