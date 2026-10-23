// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::{FRAC_PI_2, PI};

use crate::eval::admission::EvaluationAdmission;
use crate::eval::decode::curve_point;
use crate::features::FinitePoint3;
use crate::geometry::analytic::{CircleCurve, EllipseCurve, LineCurve};
use crate::geometry::curve_payloads::SubsetCurveConstruction;
use crate::geometry::nurbs::{NurbsCurve, NurbsPoles3};
use crate::geometry::{CurveGeometry, PlacedCurve, SolvedCurveGeometry};
use crate::ids::CurveId;
use crate::math::{Point3, Vector3};
use crate::transform::Transform;

const EPS_LOCAL_POINT: f64 = 1.0e-9;

fn subset(range: [f64; 2], sense: bool) -> SubsetCurveConstruction {
    SubsetCurveConstruction::try_new(
        CurveId::mint("test:model:curve#source").expect("identity"),
        range,
        sense,
        None,
    )
    .expect("ordered range")
}

fn point(geometry: &SolvedCurveGeometry, parameter: f64) -> Point3 {
    curve_point(
        EvaluationAdmission::Standard,
        &CurveGeometry::Solved(geometry.clone()),
        parameter,
    )
    .map(FinitePoint3::get)
    .expect("finite point")
}

/// The cache at `s` is the source at `start + s` for a true sense and at
/// `end - s` for a false sense, over the whole local domain.
fn assert_local(source: &SolvedCurveGeometry, range: [f64; 2]) {
    let [start, end] = range;
    for sense in [true, false] {
        let cache = subset(range, sense)
            .local_cache(source)
            .expect("reparameterizable source");
        for fraction in [0.0, 0.25, 0.5, 1.0] {
            let local = fraction * (end - start);
            let source_parameter = if sense { start + local } else { end - local };
            let distance = point(&cache, local).distance(point(source, source_parameter));
            assert!(
                distance < EPS_LOCAL_POINT,
                "sense {sense} at {local}: {distance}"
            );
        }
    }
}

fn circle() -> SolvedCurveGeometry {
    SolvedCurveGeometry::Circle(
        CircleCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, -1.0, 0.0),
            2.0,
        )
        .expect("circle"),
    )
}

#[test]
fn a_line_subset_cache_runs_from_its_start_or_end() {
    let line = SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(1.0, 1.0, 0.0), Vector3::new(0.6, 0.8, 0.0)).expect("line"),
    );
    assert_local(&line, [-1.5, 2.5]);
}

#[test]
fn a_circle_subset_cache_turns_its_frame_to_the_trim() {
    assert_local(&circle(), [3.0 * FRAC_PI_2, 2.0 * PI]);
    assert_local(&circle(), [0.4, 5.9]);
}

#[test]
fn an_ellipse_subset_cache_turns_or_places_its_ellipse() {
    let ellipse = SolvedCurveGeometry::Ellipse(
        EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            1.0,
        )
        .expect("ellipse"),
    );
    assert_local(&ellipse, [PI, 2.0 * PI]);
    assert_local(&ellipse, [0.0, PI]);
    assert!(matches!(
        subset([PI, 2.0 * PI], true).local_cache(&ellipse),
        Some(SolvedCurveGeometry::Ellipse(_))
    ));
    assert_local(&ellipse, [-FRAC_PI_2, 0.0]);
    assert_local(&ellipse, [0.7, 4.1]);
}

#[test]
fn a_nurbs_subset_cache_moves_or_reflects_its_knots() {
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 2.0, 0.0),
        Point3::new(3.0, 2.0, 1.0),
        Point3::new(4.0, 0.0, 2.0),
    ]
    .into_iter()
    .map(|point| FinitePoint3::new(point).expect("finite pole"))
    .collect();
    let nurbs = SolvedCurveGeometry::Nurbs(
        NurbsCurve::new(
            2,
            vec![1.0, 1.0, 1.0, 2.0, 3.0, 3.0, 3.0],
            NurbsPoles3::Polynomial { points },
            false,
        )
        .expect("nurbs"),
    );
    assert_local(&nurbs, [1.25, 2.75]);
}

#[test]
fn a_placed_subset_cache_keeps_its_placement() {
    let placed = SolvedCurveGeometry::Transformed(
        PlacedCurve::try_new(
            Box::new(circle()),
            Transform::affine([
                [0.0, -1.0, 0.0, 5.0],
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, -2.0],
            ])
            .expect("rigid placement"),
        )
        .expect("placed circle"),
    );
    assert_local(&placed, [1.0, 2.5]);
}
