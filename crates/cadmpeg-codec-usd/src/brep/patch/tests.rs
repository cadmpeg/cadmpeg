// SPDX-License-Identifier: Apache-2.0

use super::{faces, natural_loop, segment, winding, Nurbs};

/// The square `[a, b]²` as a loop, counterclockwise when `ccw`.
fn square(a: f64, b: f64, ccw: bool) -> Vec<Nurbs<2>> {
    let corners = [[a, a], [b, a], [b, b], [a, b]];
    let mut curves: Vec<Nurbs<2>> = (0..4)
        .map(|side| segment(corners[side], corners[(side + 1) % 4]))
        .collect();
    if !ccw {
        curves = super::reversed_loop(&curves).unwrap();
    }
    curves
}

#[test]
fn the_natural_loop_runs_counterclockwise() {
    assert!(winding(&natural_loop([[0.0, 2.0], [1.0, 3.0]])) > 0.0);
}

#[test]
fn nested_loops_alternate_between_outer_and_inner() {
    let loops = vec![
        square(0.0, 1.0, true),
        square(0.2, 0.8, false),
        square(0.4, 0.6, true),
    ];
    let (faces, assumed) = faces(loops, true).unwrap();
    assert!(!assumed);
    assert_eq!(faces.iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 1]);
    assert!(winding(&faces[0][0]) > 0.0);
    assert!(winding(&faces[0][1]) < 0.0);
    assert!(winding(&faces[1][0]) > 0.0);
}

#[test]
fn loops_of_a_reversed_face_wind_the_other_way() {
    let (faces, assumed) =
        faces(vec![square(0.0, 1.0, true), square(0.2, 0.8, true)], false).unwrap();
    assert!(assumed);
    assert!(winding(&faces[0][0]) < 0.0);
    assert!(winding(&faces[0][1]) > 0.0);
}

#[test]
fn a_loop_with_no_area_is_refused() {
    let flat = vec![
        segment([0.0, 0.0], [1.0, 0.0]),
        segment([1.0, 0.0], [0.0, 0.0]),
    ];
    assert!(faces(vec![flat], true).is_err());
}
