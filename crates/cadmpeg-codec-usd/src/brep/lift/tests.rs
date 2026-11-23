// SPDX-License-Identifier: Apache-2.0

use super::lifted;
use crate::brep::convert::evaluate;
use crate::brep::nurbs::Nurbs;
use crate::brep::{Form, NurbsSurfaceRecord};

/// Agreement of a lifted point with its closed form.
const EXACT: f64 = 1e-12;

fn line(a: [f64; 2], b: [f64; 2]) -> Nurbs<2> {
    Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![a, b],
        weights: vec![1.0, 1.0],
    }
}

/// `S(u, v) = (u, v, u·v)` on `[0, 1]²`, with an interior u knot at 0.5.
fn saddle() -> NurbsSurfaceRecord {
    NurbsSurfaceRecord {
        u_order: 2,
        v_order: 2,
        u_count: 3,
        v_count: 2,
        u_form: Form::Open,
        v_form: Form::Open,
        points: vec![
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.5, 0.0, 0.0],
            [0.5, 1.0, 0.5],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
        ],
        weights: vec![1.0; 6],
        u_knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
    }
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3)
        .map(|axis| (a[axis] - b[axis]).powi(2))
        .sum::<f64>()
        .sqrt()
}

#[test]
fn a_diagonal_on_a_saddle_is_one_parabola() {
    let curve = lifted(&saddle(), &line([0.0, 0.0], [1.0, 1.0])).unwrap();
    assert_eq!(curve.order, 3);
    assert_eq!(curve.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    for t in [0.0, 0.1, 0.3, 0.5, 0.62, 0.9, 1.0] {
        let point = evaluate(&curve, t).unwrap();
        assert!(distance(point, [t, t, t * t]) <= EXACT, "{t}: {point:?}");
    }
}

#[test]
fn an_iso_line_keeps_the_degree_of_its_direction() {
    let curve = lifted(&saddle(), &line([0.0, 0.25], [1.0, 0.25])).unwrap();
    assert_eq!(curve.order, 2);
    for t in [0.0, 0.4, 0.75, 1.0] {
        let point = evaluate(&curve, t).unwrap();
        assert!(
            distance(point, [t, 0.25, 0.25 * t]) <= EXACT,
            "{t}: {point:?}"
        );
    }
}

#[test]
fn a_rational_trim_on_a_plane_is_its_circle() {
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let plane = NurbsSurfaceRecord {
        u_order: 2,
        v_order: 2,
        u_count: 2,
        v_count: 2,
        u_form: Form::Open,
        v_form: Form::Open,
        points: vec![
            [0.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.0, 2.0, 0.0],
        ],
        weights: vec![1.0; 4],
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
    };
    let quarter = Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        points: vec![[0.75, 0.5], [0.75, 0.75], [0.5, 0.75]],
        weights: vec![1.0, h, 1.0],
    };
    let curve = lifted(&plane, &quarter).unwrap();
    assert_eq!(curve.order, 3);
    for t in [0.0, 0.2, 0.5, 0.8, 1.0] {
        let [x, y, z] = evaluate(&curve, t).unwrap();
        assert!(((x - 1.0).hypot(y - 1.0) - 0.5).abs() <= EXACT, "{t}");
        assert!(z.abs() <= EXACT);
    }
}

#[test]
fn a_composition_above_the_degree_limit_is_refused() {
    let mut steep = saddle();
    steep.u_order = 3;
    steep.u_knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let curve = Nurbs {
        degree: 11,
        knots: [vec![0.0; 12], vec![1.0; 12]].concat(),
        points: (0..12)
            .map(|index| [f64::from(index) / 11.0, 0.5])
            .collect(),
        weights: vec![1.0; 12],
    };
    assert!(lifted(&steep, &curve).is_none());
}

#[test]
fn a_smooth_iso_line_keeps_the_knots_of_its_surface_only() {
    // A cubic surface in u with two interior knots: its v = 0.5 iso-line is
    // the C2 cubic of the u knot vector, whose breaks are removable down to
    // the surface's own single knots.
    let points: Vec<[f64; 3]> = (0..5)
        .flat_map(|i| {
            let x = f64::from(i);
            [[x, 0.0, (x * 1.3).sin()], [x, 1.0, (x * 0.7).cos()]]
        })
        .collect();
    let surface = NurbsSurfaceRecord {
        u_order: 4,
        v_order: 2,
        u_count: 5,
        v_count: 2,
        u_form: Form::Open,
        v_form: Form::Open,
        weights: vec![1.0; points.len()],
        points,
        u_knots: vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
    };
    let curve = lifted(&surface, &line([0.0, 0.5], [1.0, 0.5])).unwrap();
    assert_eq!(curve.order, 4);
    assert_eq!(
        curve.knots,
        vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0]
    );
    for t in [0.0, 0.2, 0.5, 0.65, 1.0] {
        let expected = crate::brep::convert::surface_point(&surface, [t, 0.5]).unwrap();
        assert!(
            distance(evaluate(&curve, t).unwrap(), expected) <= EXACT,
            "{t}"
        );
    }
}

#[test]
fn a_diagonal_across_a_crease_keeps_its_break() {
    // `S(u, v) = (u, v, 1 - |2u - 1|)`: a crease along u = 0.5.
    let mut tent = saddle();
    tent.points = vec![
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.5, 0.0, 1.0],
        [0.5, 1.0, 1.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    let curve = lifted(&tent, &line([0.0, 0.0], [1.0, 1.0])).unwrap();
    assert_eq!(curve.order, 2);
    assert_eq!(curve.knots, vec![0.0, 0.0, 0.5, 1.0, 1.0]);
    for t in [0.0, 0.2, 0.5, 0.8, 1.0] {
        let point = evaluate(&curve, t).unwrap();
        let expected = [t, t, 1.0 - (2.0 * t - 1.0).abs()];
        assert!(distance(point, expected) <= EXACT, "{t}: {point:?}");
    }
}
