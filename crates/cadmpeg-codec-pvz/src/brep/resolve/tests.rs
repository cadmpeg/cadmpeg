// SPDX-License-Identifier: Apache-2.0
use std::f64::consts::TAU;

use super::{
    encloses, face_regions, pcurve_directions, sense_from_ends, signed_area, CoedgeGeometry,
    LoopPolygon, RegionLoops, Sense,
};
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
        coedge(ends, Some(Sense::Reversed)),
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

#[test]
fn nesting_parity_splits_holes_and_islands() {
    let polygons = vec![
        Some(square([0.0, 0.0], 10.0, false)),
        Some(square([1.0, 1.0], 8.0, true)),
        Some(square([2.0, 2.0], 2.0, false)),
        Some(square([20.0, 0.0], 1.0, false)),
    ];
    assert!(signed_area(&polygons[0].as_ref().unwrap().points) > 0.0);
    let regions = face_regions(&[0, 0, 0, 3], &polygons, [None, None]);
    assert_eq!(
        regions,
        [
            RegionLoops {
                loops: vec![0, 1],
                classified: true
            },
            RegionLoops {
                loops: vec![2],
                classified: true
            },
            RegionLoops {
                loops: vec![3],
                classified: true
            },
        ]
    );
}

#[test]
fn wrapping_loops_leave_their_group_unclassified() {
    let band = LoopPolygon {
        points: vec![[0.0, 0.0], [3.0, 0.0]],
        wrap: [TAU, 0.0],
    };
    let regions = face_regions(
        &[0, 0],
        &[Some(band.clone()), Some(band)],
        [Some(TAU), None],
    );
    assert_eq!(
        regions,
        [RegionLoops {
            loops: vec![0, 1],
            classified: false
        }]
    );
}

#[test]
fn containment_is_tested_modulo_the_period() {
    let polygon = square([6.0, 0.0], 1.0, false).points;
    assert!(encloses(&polygon, [6.5 - TAU, 0.5], [Some(TAU), None]));
    assert!(!encloses(&polygon, [6.5 - TAU, 0.5], [None, None]));
}
