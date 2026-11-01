// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::TAU;

use super::{regions, Frame, Node, Ring};
use crate::plane::signed_area;

fn nodes(points: &[[f64; 2]]) -> Vec<Node> {
    points
        .iter()
        .map(|uv| Node {
            uv: *uv,
            point: None,
            open: false,
        })
        .collect()
}

fn circle(v: f64, rising: bool) -> Ring {
    let steps = 8_u8;
    let mut points = Vec::new();
    for step in 0..steps {
        let u = TAU * f64::from(step) / f64::from(steps);
        points.push([if rising { u } else { -u }, v]);
    }
    Ring {
        nodes: nodes(&points),
        closure: [if rising { TAU } else { -TAU }, 0.0],
    }
}

fn cylinder() -> Frame {
    Frame {
        periods: [Some(TAU), None],
        domain: None,
        poles: Vec::new(),
    }
}

#[test]
fn two_wrapping_loops_bound_one_strip() {
    let found = regions(vec![circle(0.0, true), circle(2.0, false)], &cylinder()).unwrap();
    assert_eq!(found.len(), 1);
    let outer: Vec<_> = found[0].outer.iter().map(|node| node.uv).collect();
    assert!((signed_area(&outer) / 2.0 - TAU * 2.0).abs() < 1e-9);
}

#[test]
fn one_wrapping_loop_runs_to_a_pole() {
    let mut frame = cylinder();
    frame.poles.push(([0.0, 3.0], None));
    let found = regions(vec![circle(1.0, true)], &frame).unwrap();
    let outer: Vec<_> = found[0].outer.iter().map(|node| node.uv).collect();
    assert!((signed_area(&outer) / 2.0 - TAU * 2.0).abs() < 1e-9);
}

#[test]
fn a_wrapping_loop_with_no_far_side_is_refused() {
    assert!(regions(vec![circle(1.0, true)], &cylinder()).is_err());
}

#[test]
fn a_hole_lies_in_its_outer_loop() {
    let outer = Ring {
        nodes: nodes(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]]),
        closure: [0.0, 0.0],
    };
    let hole = Ring {
        nodes: nodes(&[[1.0, 1.0], [1.0, 2.0], [2.0, 2.0], [2.0, 1.0]]),
        closure: [0.0, 0.0],
    };
    let frame = Frame::default();
    let found = regions(vec![outer, hole], &frame).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].holes.len(), 1);
}

#[test]
fn a_clockwise_ring_around_a_whole_torus_cell_is_the_outer_loop() {
    let corners = [[0.0, 0.0], [-TAU, 0.0], [-TAU, TAU], [0.0, TAU]];
    let ring = Ring {
        nodes: nodes(&corners),
        closure: [0.0, 0.0],
    };
    let frame = Frame {
        periods: [Some(TAU), Some(TAU)],
        domain: Some([[0.0, TAU], [0.0, TAU]]),
        poles: Vec::new(),
    };
    let found = regions(vec![ring], &frame).unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].holes.is_empty());
    let outer: Vec<_> = found[0].outer.iter().map(|node| node.uv).collect();
    assert!((signed_area(&outer) / 2.0 - TAU * TAU).abs() < 1e-9);
}
