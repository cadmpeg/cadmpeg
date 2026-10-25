// SPDX-License-Identifier: Apache-2.0
use std::f64::consts::TAU;

use super::{
    encloses, face_regions, pcurve_directions, sense_from_ends, signed_area, CoedgeGeometry,
    LoopPolygon, RegionLoops, Sense,
};
use cadmpeg_core::decode::WorkBudget;

use crate::brep::model::CurveGeometry;

fn coedge(ends: [[f64; 3]; 2], sense: Option<Sense>) -> CoedgeGeometry {
    CoedgeGeometry {
        pcurve: None,
        interval: None,
        uv_ends: None,
        ends: Some(ends),
        edge_ends: None,
        sense,
        residual: None,
    }
}

const LINE: CurveGeometry<3> = CurveGeometry::Line {
    origin: [0.0; 3],
    direction: [1.0, 0.0, 0.0],
};

#[test]
fn sense_compares_pcurve_image_ends_with_edge_ends() {
    let edge = [[0.0; 3], [1.0, 0.0, 0.0]];
    let (sense, residual) = sense_from_ends(edge, edge, None, &LINE);
    assert_eq!((sense, residual), (Sense::Forward, 0.0));
    let (sense, _) = sense_from_ends([edge[1], edge[0]], edge, None, &LINE);
    assert_eq!(sense, Sense::Reversed);
}

#[test]
fn closed_edge_sense_follows_an_interior_point() {
    let circle = CurveGeometry::Circle(crate::brep::model::Conic {
        center: [0.0; 3],
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        range: None,
    });
    let ends = [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let quarter = [0.0, 1.0, 0.0];
    let (sense, _) = sense_from_ends(ends, ends, Some(quarter), &circle);
    assert_eq!(sense, Sense::Forward);
    let (sense, _) = sense_from_ends(ends, ends, Some([0.0, -1.0, 0.0]), &circle);
    assert_eq!(sense, Sense::Reversed);
}

#[test]
fn pcurve_directions_follow_the_shared_ends() {
    let corners = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let coedges = vec![
        coedge([corners[0], corners[1]], None),
        coedge([corners[2], corners[1]], None),
        coedge([corners[2], corners[3]], None),
        coedge([corners[0], corners[3]], None),
    ];
    assert_eq!(
        pcurve_directions(&[1, 1, 1, 1], &coedges),
        [true, false, true, false]
    );
}

#[test]
fn short_loops_take_the_direction_from_the_orientation_byte() {
    let ends = [[0.0; 3], [1.0, 0.0, 0.0]];
    let coedges = vec![
        coedge(ends, Some(Sense::Forward)),
        coedge([ends[1], ends[0]], Some(Sense::Reversed)),
    ];
    assert_eq!(pcurve_directions(&[1, 0], &coedges), [true, true]);
    assert_eq!(pcurve_directions(&[0, 1], &coedges), [false, false]);
}

fn square(low: [f64; 2], size: f64, clockwise: bool) -> LoopPolygon {
    let mut points = vec![
        low,
        [low[0] + size, low[1]],
        [low[0] + size, low[1] + size],
        [low[0], low[1] + size],
    ];
    if clockwise {
        points.reverse();
    }
    LoopPolygon {
        points,
        wrap: [0.0, 0.0],
    }
}

fn regions(
    inner: &[bool],
    polygons: &[Option<LoopPolygon>],
    periods: [Option<f64>; 2],
) -> Vec<RegionLoops> {
    face_regions(inner, polygons, periods, &WorkBudget::new(usize::MAX)).unwrap()
}

fn region(loops: &[usize], classified: bool) -> RegionLoops {
    RegionLoops {
        loops: loops.to_vec(),
        classified,
    }
}

#[test]
fn inner_loops_join_the_innermost_outer_loop_that_contains_them() {
    let polygons = vec![
        Some(square([0.0, 0.0], 10.0, false)),
        Some(square([21.0, 1.0], 2.0, true)),
        Some(square([25.0, 5.0], 2.0, true)),
        Some(square([20.0, 0.0], 10.0, false)),
        Some(square([2.0, 2.0], 4.0, false)),
        Some(square([3.0, 3.0], 1.0, true)),
    ];
    assert!(signed_area(&polygons[0].as_ref().unwrap().points) > 0.0);
    let inner = [false, true, true, false, false, true];
    assert_eq!(
        regions(&inner, &polygons, [None, None]),
        [
            region(&[0], true),
            region(&[3, 1, 2], true),
            region(&[4, 5], true)
        ]
    );
}

#[test]
fn one_outer_loop_owns_every_inner_loop() {
    let polygons = vec![
        Some(square([0.0, 0.0], 1.0, false)),
        Some(square([5.0, 5.0], 1.0, true)),
    ];
    assert_eq!(
        regions(&[false, true], &polygons, [None, None]),
        [region(&[0, 1], true)]
    );
    assert_eq!(
        regions(&[false, true], &[None, None], [None, None]),
        [region(&[0, 1], false)]
    );
}

#[test]
fn uncontained_inner_loop_is_an_unclassified_region() {
    let polygons = vec![
        Some(square([0.0, 0.0], 1.0, false)),
        Some(square([2.0, 0.0], 1.0, false)),
        Some(square([5.0, 5.0], 1.0, true)),
    ];
    assert_eq!(
        regions(&[false, false, true], &polygons, [None, None]),
        [region(&[0], true), region(&[1], true), region(&[2], false)]
    );
}

#[test]
fn nesting_stops_when_the_work_budget_runs_out() {
    let polygons = vec![
        Some(square([0.0, 0.0], 1.0, false)),
        Some(square([2.0, 0.0], 1.0, false)),
        Some(square([2.25, 0.25], 0.5, true)),
    ];
    let budget = WorkBudget::new(1);
    assert_eq!(
        face_regions(&[false, false, true], &polygons, [None, None], &budget),
        None
    );
}

#[test]
fn wrapping_loops_leave_their_region_unclassified() {
    let band = LoopPolygon {
        points: vec![[0.0, 0.0], [3.0, 0.0]],
        wrap: [TAU, 0.0],
    };
    assert_eq!(
        regions(
            &[false, true],
            &[Some(band.clone()), Some(band)],
            [Some(TAU), None],
        ),
        [region(&[0, 1], false)]
    );
}

#[test]
fn containment_is_tested_modulo_the_period() {
    let polygon = square([6.0, 0.0], 1.0, false).points;
    assert!(encloses(&polygon, [6.5 - TAU, 0.5], [Some(TAU), None]));
    assert!(!encloses(&polygon, [6.5 - TAU, 0.5], [None, None]));
}

fn coedge_on(ends: [[f64; 3]; 2], uv_ends: [[f64; 2]; 2]) -> CoedgeGeometry {
    CoedgeGeometry {
        uv_ends: Some(uv_ends),
        ..coedge(ends, Some(Sense::Forward))
    }
}

#[test]
fn closed_edges_take_their_direction_from_the_domain_junctions() {
    let (bottom, top) = ([1.0, 0.0, 0.0], [1.0, 0.0, 1.0]);
    let coedges = vec![
        coedge_on([bottom, bottom], [[0.0, 0.0], [TAU, 0.0]]),
        coedge_on([bottom, top], [[TAU, 0.0], [TAU, 1.0]]),
        coedge_on([top, top], [[TAU, 1.0], [0.0, 1.0]]),
        coedge_on([top, bottom], [[0.0, 1.0], [0.0, 0.0]]),
    ];
    assert_eq!(
        pcurve_directions(&[0, 1, 0, 1], &coedges),
        [true, true, true, true]
    );
}
