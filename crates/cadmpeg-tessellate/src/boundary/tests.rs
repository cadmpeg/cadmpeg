// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::ids::{CoedgeId, EdgeId, LoopId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Coedge, Sense};

use super::{along_one_parameter, junction_closure, Traced};

fn coedge() -> Coedge {
    let id = CoedgeId::mint("test:model:coedge#0").unwrap();
    Coedge {
        id: id.clone(),
        owner_loop: LoopId::mint("test:model:loop#0").unwrap(),
        edge: EdgeId::mint("test:model:edge#0").unwrap(),
        radial_next: id,
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
        native_ref: None,
    }
}

fn traced(coedge: &Coedge, uv: Vec<[f64; 2]>, points: Vec<Point3>) -> Traced<'_> {
    Traced {
        coedge,
        uv,
        points,
        params: Vec::new(),
        degenerate: false,
        from_pcurve: false,
    }
}

#[test]
fn a_small_closing_gap_at_one_vertex_is_a_junction() {
    let coedge = coedge();
    let vertex = Point3::new(0.0, 0.0, 0.0);
    let loop_ = [
        traced(
            &coedge,
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]],
            vec![
                vertex,
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
        ),
        traced(
            &coedge,
            vec![[10.0, 10.0], [0.01, 0.0]],
            vec![Point3::new(1.0, 1.0, 0.0), vertex],
        ),
    ];
    assert_eq!(
        junction_closure(&loop_, [0.01, 0.0], 1.0, [None, None]),
        Some([0.0, 0.0])
    );
}

#[test]
fn a_closing_gap_between_two_points_is_no_junction() {
    let coedge = coedge();
    let loop_ = [traced(
        &coedge,
        vec![[0.0, 0.0], [10.0, 0.0], [0.01, 10.0]],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 0.001, 0.0),
        ],
    )];
    assert_eq!(
        junction_closure(&loop_, [0.01, 0.0], 1.0, [None, None]),
        None
    );
}

#[test]
fn a_large_closing_gap_at_one_vertex_is_no_junction() {
    let coedge = coedge();
    let vertex = Point3::new(0.0, 0.0, 0.0);
    let loop_ = [traced(
        &coedge,
        vec![[0.0, 0.0], [10.0, 0.0], [5.0, 0.0]],
        vec![vertex, Point3::new(1.0, 0.0, 0.0), vertex],
    )];
    assert_eq!(
        junction_closure(&loop_, [5.0, 0.0], 1.0, [None, None]),
        None
    );
}

#[test]
fn a_closing_gap_near_a_whole_period_at_one_vertex_is_a_wrap() {
    let coedge = coedge();
    let vertex = Point3::new(1.0, 0.0, 0.0);
    let loop_ = [traced(
        &coedge,
        vec![[0.0, 0.0], [32.0, 0.0], [63.9996, 0.0]],
        vec![vertex, Point3::new(-1.0, 0.0, 0.0), vertex],
    )];
    assert_eq!(
        junction_closure(&loop_, [63.9996, 0.0], 1.0, [Some(64.0), None]),
        Some([64.0, 0.0])
    );
}

#[test]
fn a_displacement_with_a_small_across_residual_runs_along_one_parameter() {
    assert!(along_one_parameter([0.0, 0.5]));
    assert!(along_one_parameter([5e-7, 0.5]));
    assert!(along_one_parameter([-0.5, 5e-7]));
}

#[test]
fn a_displacement_across_both_parameters_runs_along_neither() {
    assert!(!along_one_parameter([0.03, -0.05]));
    assert!(!along_one_parameter([0.0, 0.0]));
}
