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
fn a_diagonal_on_a_saddle_is_a_parabola_broken_at_the_knot_line() {
    let curve = lifted(&saddle(), &line([0.0, 0.0], [1.0, 1.0])).unwrap();
    assert_eq!(curve.order, 3);
    assert!(curve.knots.contains(&0.5));
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
