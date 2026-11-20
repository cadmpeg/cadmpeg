// SPDX-License-Identifier: Apache-2.0

use super::{triangulate, Rings};
use crate::plane::{cross2, signed_area, Uv};

fn area(points: &[Uv], triangles: &[[usize; 3]]) -> f64 {
    triangles
        .iter()
        .map(|[a, b, c]| cross2(points[*a], points[*b], points[*c]) / 2.0)
        .sum()
}

#[test]
fn a_square_with_a_hole_covers_its_area() {
    let points: Vec<Uv> = vec![
        [0.0, 0.0],
        [4.0, 0.0],
        [4.0, 4.0],
        [0.0, 4.0],
        [1.0, 1.0],
        [1.0, 3.0],
        [3.0, 3.0],
        [3.0, 1.0],
    ];
    let triangles = triangulate(Rings {
        points: &points,
        outer: vec![0, 1, 2, 3],
        holes: vec![vec![4, 5, 6, 7]],
    })
    .unwrap();
    assert!((area(&points, &triangles) - 12.0).abs() < 1e-12);
    for [a, b, c] in &triangles {
        assert!(cross2(points[*a], points[*b], points[*c]) > 0.0);
    }
}

#[test]
fn a_clockwise_outer_ring_is_reversed() {
    let points: Vec<Uv> = vec![[0.0, 0.0], [0.0, 2.0], [3.0, 2.0], [3.0, 0.0]];
    assert!(signed_area(&points) < 0.0);
    let triangles = triangulate(Rings {
        points: &points,
        outer: vec![0, 1, 2, 3],
        holes: Vec::new(),
    })
    .unwrap();
    assert_eq!(triangles.len(), 2);
    assert!((area(&points, &triangles) - 6.0).abs() < 1e-12);
}

#[test]
fn a_concave_ring_keeps_its_notch() {
    let points: Vec<Uv> = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [2.0, 1.0], [0.0, 4.0]];
    let triangles = triangulate(Rings {
        points: &points,
        outer: vec![0, 1, 2, 3, 4],
        holes: Vec::new(),
    })
    .unwrap();
    assert!((area(&points, &triangles) - signed_area(&points) / 2.0).abs() < 1e-12);
}

/// The parameter-plane rings of a planar face of the STEP Tools st203-bapl
/// sample: an outer ring and 25 holes in aligned rows, where bridges pass
/// through other holes' corners and several meet at one vertex. The first
/// line is the outer ring; each ring lists `u v` pairs separated by `;`.
const PLATE_WITH_HOLES: &str = include_str!("plate_with_holes.txt");

#[test]
fn holes_bridged_to_one_vertex_leave_their_area_uncovered() {
    let mut points: Vec<Uv> = Vec::new();
    let mut rings: Vec<Vec<usize>> = Vec::new();
    for line in PLATE_WITH_HOLES.lines() {
        let mut ring = Vec::new();
        for pair in line.split(';') {
            let mut values = pair
                .split_whitespace()
                .map(|value| value.parse::<f64>().unwrap());
            points.push([values.next().unwrap(), values.next().unwrap()]);
            ring.push(points.len() - 1);
        }
        rings.push(ring);
    }
    let ring_area =
        |ring: &[usize]| signed_area(&ring.iter().map(|at| points[*at]).collect::<Vec<_>>()) / 2.0;
    let expected = ring_area(&rings[0]).abs()
        - rings[1..]
            .iter()
            .map(|ring| ring_area(ring).abs())
            .sum::<f64>();
    let outer = rings.remove(0);
    let triangles = triangulate(Rings {
        points: &points,
        outer,
        holes: rings,
    })
    .unwrap();
    assert!(
        (area(&points, &triangles) - expected).abs() < 1e-6 * expected,
        "area {} against {expected}",
        area(&points, &triangles)
    );
}
