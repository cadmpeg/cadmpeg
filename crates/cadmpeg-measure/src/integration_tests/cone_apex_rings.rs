// SPDX-License-Identifier: Apache-2.0
//! A ring whose coedges are all degenerate at the cone apex: alone, it bounds
//! no area, and with one loop around the axis the region reaches the apex.
//!
//! Every face lies on the cone with apex `(0, 0, 0)`, axis `+z`, stored origin
//! `(0, 0, 10)`, radius 5 at the origin and `tan α = 1/2`.

use std::f64::consts::PI;

use cadmpeg_ir::ids::{EdgeId, VertexId};
use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::CadIr;

use crate::reason::UnavailableReason;
use crate::report::{FaceMeasures, LoopRole, MeasureReport, RegionRule};
use crate::select::Selection;
use crate::test_support::builders::{assert_measured, assert_range, measure, Builder, Frame};

fn faces() -> Selection {
    Selection {
        all_faces: true,
        ..Selection::default()
    }
}

fn only_face(report: &MeasureReport) -> &FaceMeasures {
    let [face] = report.faces().as_slice() else {
        panic!("one face row");
    };
    face
}

fn cone(builder: &mut Builder) -> cadmpeg_ir::ids::SurfaceId {
    builder.cone(Frame::z_up([0.0, 0.0, 10.0]), 5.0, 0.5_f64.atan())
}

fn section(builder: &mut Builder, z: f64) -> EdgeId {
    let rho = z.abs() / 2.0;
    let vertex = builder.vertex([rho, 0.0, z]);
    builder.circle(
        [0.0, 0.0, z],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        rho,
        &vertex,
    )
}

/// A closed edge with a degenerate curve at the apex.
fn apex_edge(builder: &mut Builder) -> EdgeId {
    let apex: VertexId = builder.vertex([0.0, 0.0, 0.0]);
    builder.degenerate_edge(&apex)
}

/// The lateral area between the apex and the circle of radius `rho`.
fn tip_area(rho: f64) -> f64 {
    PI * rho * rho * 5.0_f64.sqrt()
}

fn reason(quantity: &crate::value::Quantity<crate::value::Scalar>) -> UnavailableReason {
    quantity
        .unavailable()
        .as_ref()
        .expect("unavailable")
        .reason()
}

#[test]
fn a_rim_and_an_apex_ring_bound_the_tip_on_either_nappe() {
    for z in [20.0, -20.0] {
        let mut builder = Builder::new();
        let rim = section(&mut builder, z);
        let apex = apex_edge(&mut builder);
        let surface = cone(&mut builder);
        builder.face(
            &surface,
            Sense::Forward,
            &[vec![(rim, Sense::Reversed)], vec![(apex, Sense::Forward)]],
            false,
        );
        let ir = builder.finish(BodyKind::Sheet);
        let report = measure(&ir, &faces());
        let measures = only_face(&report).measures();
        assert_measured(measures.area(), tip_area(10.0), 400.0);
        assert_eq!(*measures.region(), Some(RegionRule::Nesting));
        assert_eq!(*measures.loops()[0].role(), Some(LoopRole::Outer));
        assert_eq!(*measures.loops()[1].role(), Some(LoopRole::Vertex));
        assert_measured(measures.boundary_length(), 20.0 * PI, 20.0);
        assert_range(measures.radius_range().as_ref(), [0.0, 10.0], 20.0);
    }
}

#[test]
fn an_edge_with_no_curve_at_the_apex_is_an_apex_ring() {
    let mut builder = Builder::new();
    let rim = section(&mut builder, 20.0);
    let apex = builder.vertex([0.0, 0.0, 0.0]);
    let apex = builder.bare_edge(&apex, &apex);
    let surface = cone(&mut builder);
    builder.face(
        &surface,
        Sense::Forward,
        &[vec![(rim, Sense::Reversed)], vec![(apex, Sense::Forward)]],
        false,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    assert_measured(only_face(&report).measures().area(), tip_area(10.0), 400.0);
}

/// A spike: the tip below `z = 20`, a cylinder to `z = 40`, and a cap. The apex
/// ring lies beside ordinary loops in one closed body.
fn spike() -> CadIr {
    let mut builder = Builder::new();
    let rim = section(&mut builder, 20.0);
    let top_vertex = builder.vertex([10.0, 0.0, 40.0]);
    let top = builder.circle(
        [0.0, 0.0, 40.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        10.0,
        &top_vertex,
    );
    let apex = apex_edge(&mut builder);
    let tip = cone(&mut builder);
    builder.face(
        &tip,
        Sense::Forward,
        &[
            vec![(rim.clone(), Sense::Reversed)],
            vec![(apex, Sense::Forward)],
        ],
        false,
    );
    let wall = builder.cylinder(Frame::z_up([0.0, 0.0, 20.0]), 10.0);
    builder.face(
        &wall,
        Sense::Forward,
        &[
            vec![(rim, Sense::Forward)],
            vec![(top.clone(), Sense::Reversed)],
        ],
        false,
    );
    let cap = builder.plane([0.0, 0.0, 40.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    builder.face(&cap, Sense::Forward, &[vec![(top, Sense::Forward)]], false);
    builder.finish(BodyKind::Solid)
}

#[test]
fn an_apex_ring_beside_ordinary_loops_closes_the_body() {
    let ir = spike();
    let report = measure(&ir, &Selection::default());
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    let expected = PI * 100.0 * 20.0 + PI * 100.0 * 20.0 / 3.0;
    assert_measured(body.volume().as_ref().expect("volume"), expected, expected);
}

#[test]
fn an_apex_ring_needs_exactly_one_loop_around_the_axis() {
    // A band: the apex is not on the face.
    let mut builder = Builder::new();
    let upper = section(&mut builder, 20.0);
    let lower = section(&mut builder, 10.0);
    let apex = apex_edge(&mut builder);
    let surface = cone(&mut builder);
    builder.face(
        &surface,
        Sense::Forward,
        &[
            vec![(upper, Sense::Reversed)],
            vec![(lower, Sense::Forward)],
            vec![(apex, Sense::Forward)],
        ],
        false,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    assert_eq!(
        reason(only_face(&report).measures().area()),
        UnavailableReason::BoundaryNestingInvalid
    );

    // Alone: nothing bounds the face.
    let mut builder = Builder::new();
    let apex = apex_edge(&mut builder);
    let surface = cone(&mut builder);
    builder.face(
        &surface,
        Sense::Forward,
        &[vec![(apex, Sense::Forward)]],
        false,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    assert_eq!(
        reason(only_face(&report).measures().area()),
        UnavailableReason::BoundaryUnboundedFace
    );
}

#[test]
fn a_degenerate_ring_off_the_apex_or_off_a_cone_stays_unresolved() {
    // On the cone, away from the apex.
    let mut builder = Builder::new();
    let rim = section(&mut builder, 20.0);
    let off_apex = builder.vertex([5.0, 0.0, 10.0]);
    let off_apex = builder.degenerate_edge(&off_apex);
    let surface = cone(&mut builder);
    builder.face(
        &surface,
        Sense::Forward,
        &[
            vec![(rim, Sense::Reversed)],
            vec![(off_apex, Sense::Forward)],
        ],
        false,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    assert!(only_face(&report).measures().area().unavailable().is_some());

    // At a sphere pole, where one ring around the axis leaves two regions.
    let mut builder = Builder::new();
    let rim_vertex = builder.vertex([5.0, 0.0, 0.0]);
    let rim = builder.circle(
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        5.0,
        &rim_vertex,
    );
    let pole = builder.vertex([0.0, 0.0, 5.0]);
    let pole = builder.degenerate_edge(&pole);
    let surface = builder.sphere(Frame::z_up([0.0, 0.0, 0.0]), 5.0);
    builder.face(
        &surface,
        Sense::Forward,
        &[vec![(rim, Sense::Forward)], vec![(pole, Sense::Forward)]],
        false,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    assert_eq!(
        reason(only_face(&report).measures().area()),
        UnavailableReason::BoundaryCoedgeUnresolved
    );
}

#[test]
fn only_a_closed_degenerate_edge_has_length_zero() {
    let edge_length = |ir: &CadIr| {
        let report = measure(
            ir,
            &Selection {
                all_edges: true,
                ..Selection::default()
            },
        );
        let [edge] = report.edges().as_slice() else {
            panic!("one edge row");
        };
        edge.length().clone()
    };
    let mut builder = Builder::new();
    let closed = builder.vertex([0.0, 0.0, 0.0]);
    builder.degenerate_edge(&closed);
    assert_measured(&edge_length(&builder.finish(BodyKind::Wire)), 0.0, 1.0);

    let mut builder = Builder::new();
    let start = builder.vertex([0.0, 0.0, 0.0]);
    let end = builder.vertex([1.0, 0.0, 0.0]);
    builder.degenerate_open_edge(&start, &end);
    assert!(edge_length(&builder.finish(BodyKind::Wire))
        .unavailable()
        .is_some());
}

#[test]
fn a_ring_with_an_ordinary_coedge_is_not_a_vertex_loop() {
    use crate::test_support::builders::Circle;
    let circle = Circle {
        frame: Frame::z_up([0.0, 0.0, 20.0]),
        radius: 10.0,
    };
    let slant = 10.0 * 5.0_f64.sqrt();
    for degenerate_first in [true, false] {
        let mut builder = Builder::new();
        let apex = builder.vertex([0.0, 0.0, 0.0]);
        let start = builder.vertex([10.0, 0.0, 20.0]);
        let end = builder.vertex([0.0, -10.0, 20.0]);
        let collapsed = builder.degenerate_edge(&apex);
        let rising = builder.line(&apex, &start);
        let rim = builder.arc(circle, [0.0, 1.5 * PI], &start, &end);
        let falling = builder.line(&apex, &end);
        let mut ring = vec![
            (rising, Sense::Forward),
            (rim, Sense::Forward),
            (falling, Sense::Reversed),
        ];
        if degenerate_first {
            ring.insert(0, (collapsed, Sense::Forward));
        } else {
            ring.push((collapsed, Sense::Forward));
        }
        let surface = cone(&mut builder);
        builder.face(&surface, Sense::Reversed, &[ring], true);
        let ir = builder.finish(BodyKind::Sheet);
        let report = measure(&ir, &faces());
        let measures = only_face(&report).measures();
        assert_eq!(
            *measures.loops()[0].role(),
            Some(LoopRole::Outer),
            "{degenerate_first}"
        );
        assert_measured(measures.area(), 0.75 * tip_area(10.0), 400.0);
        assert_measured(measures.boundary_length(), 2.0 * slant + 15.0 * PI, 20.0);
    }
}
