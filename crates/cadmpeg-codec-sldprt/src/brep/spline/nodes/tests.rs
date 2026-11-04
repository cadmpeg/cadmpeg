// SPDX-License-Identifier: Apache-2.0

use super::carriers;
use crate::brep::topology::nodes::sequence;
use crate::test_support::parasolid::sequence::{sequence_of, F};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};

fn wrapper(nurbs: u16) -> Vec<F> {
    use F::{C, I, P};
    vec![I(1), P(1), P(1), P(1), P(1), P(1), C(b'+'), P(nurbs), P(1)]
}

/// `NURBS_CURVE` fields: degree 1, two poles of dimension `dimension`.
fn curve_descriptor(dimension: u16, arrays: [u16; 3]) -> Vec<F> {
    use F::{B, I, L, P, S};
    vec![
        S(1),
        I(2),
        S(dimension),
        I(2),
        B(0),
        L(false),
        L(false),
        L(false),
        B(0),
        P(arrays[0]),
        P(arrays[1]),
        P(arrays[2]),
    ]
}

/// `NURBS_SURF` fields: degree 1 by 1, two by two poles of dimension 3.
fn surface_descriptor(arrays: [u16; 5]) -> Vec<F> {
    use F::{B, I, L, P, S};
    vec![
        L(false),
        L(false),
        S(1),
        S(1),
        I(2),
        I(2),
        B(0),
        B(0),
        I(2),
        I(2),
        L(false),
        L(false),
        L(false),
        B(0),
        S(3),
        P(arrays[0]),
        P(arrays[1]),
        P(arrays[2]),
        P(arrays[3]),
        P(arrays[4]),
    ]
}

fn line_and_patch() -> Vec<(u16, u16, Vec<F>)> {
    let poles = |values: &[f64]| vec![F::Reals(values.to_vec())];
    vec![
        (134, 3, wrapper(4)),
        (136, 4, curve_descriptor(3, [5, 6, 7])),
        (45, 5, poles(&[0.0, 0.0, 0.0, 0.001, 0.0, 0.0])),
        (127, 6, vec![F::Shorts(vec![2, 2])]),
        (128, 7, vec![F::Reals(vec![0.0, 1.0])]),
        (124, 10, wrapper(11)),
        (126, 11, surface_descriptor([12, 13, 14, 15, 16])),
        (
            45,
            12,
            poles(&[
                0.0, 0.0, 0.0, 0.0, 0.001, 0.0, 0.001, 0.0, 0.0, 0.001, 0.001, 0.0,
            ]),
        ),
        (127, 13, vec![F::Shorts(vec![2, 2])]),
        (127, 14, vec![F::Shorts(vec![2, 2])]),
        (128, 15, vec![F::Reals(vec![0.0, 1.0])]),
        (128, 16, vec![F::Reals(vec![0.0, 1.0])]),
    ]
}

#[test]
fn b_curves_and_b_surfaces_assemble_from_their_descriptor_arrays() {
    let (payload, body_offset) = sequence_of(&line_and_patch());
    let records = sequence(&payload).unwrap();
    let mut refusals = Vec::new();
    let splines = carriers(&records, body_offset, &mut refusals).unwrap();
    assert!(refusals.is_empty());
    let curve = &splines.curves[&3];
    assert!(matches!(
        curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_))
    ));
    let start = records.nodes().find(|node| node.kind() == 134).unwrap();
    assert_eq!(curve.offset, start.range().start - body_offset);
    assert!(matches!(
        splines.surfaces[&10].geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
    ));
}

#[test]
fn descriptors_outside_their_guards_and_dangling_links_omit_the_carrier() {
    let mut nodes = line_and_patch();
    nodes[1] = (136, 4, curve_descriptor(5, [5, 6, 7]));
    nodes[5] = (124, 10, wrapper(99));
    let (payload, body_offset) = sequence_of(&nodes);
    let records = sequence(&payload).unwrap();
    let splines = carriers(&records, body_offset, &mut Vec::new()).unwrap();
    assert!(splines.curves.is_empty());
    assert!(splines.surfaces.is_empty());
}

#[test]
fn repeated_spline_indices_leave_the_stream_to_the_scanners() {
    let mut nodes = line_and_patch();
    nodes.push((128, 7, vec![F::Reals(vec![0.0, 2.0])]));
    let (payload, body_offset) = sequence_of(&nodes);
    let records = sequence(&payload).unwrap();
    assert!(carriers(&records, body_offset, &mut Vec::new()).is_none());
}

#[test]
fn parameter_space_b_curves_give_pcurves() {
    let mut nodes = line_and_patch();
    nodes[1] = (136, 4, curve_descriptor(2, [5, 6, 7]));
    nodes[2] = (45, 5, vec![F::Reals(vec![0.0, 0.0, 0.5, 1.0])]);
    let (payload, body_offset) = sequence_of(&nodes);
    let records = sequence(&payload).unwrap();
    let splines = carriers(&records, body_offset, &mut Vec::new()).unwrap();
    assert!(splines.curves.is_empty());
    let points = splines.pcurves[&3].control_points();
    assert_eq!(points.len(), 2);
    assert_eq!(points[1].get(), cadmpeg_ir::math::Point2::new(0.5, 1.0));
}
