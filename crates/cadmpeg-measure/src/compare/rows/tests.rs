// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::math::Point3;

use super::{bbox_rows, quantity_rows, Context, Quantum};
use crate::compare::{Operand, Rule};
use crate::face::box_value;
use crate::geom::Aabb;
use crate::reason::{CompareCause, UnavailableReason};
use crate::value::{
    measured, BoxKind, BoxValue, Errors, Method, Quantity, Reading, Scalar, Unavailable, Unit,
};

const CONTEXT: Context = Context {
    floor: 1.0e-9,
    scale: 10.0,
};

const AREA: Quantum<'static> = Quantum {
    name: "area",
    unit: Unit::SquareMillimeter,
    dimension: 2,
};

fn scalar(value: f64, numeric: f64) -> Quantity<Scalar> {
    measured(
        Scalar::new(value),
        Unit::SquareMillimeter,
        Method::BoundaryQuadrature,
        Errors::new(numeric, 0.0),
        "test:model:body#a",
    )
}

#[test]
fn a_scalar_row_compares_within_errors_and_the_floor() {
    let mut rows = Vec::new();
    let (a, b) = (scalar(100.0, 0.25), scalar(100.6, 0.25));
    quantity_rows(AREA, [Some(&a), Some(&b)], &CONTEXT, &mut rows);
    let [row] = rows.as_slice() else {
        panic!("one row");
    };
    assert_eq!(row.rule(), Rule::Equal);
    let tolerance = 0.5 + 1.0e-9 * 100.6;
    let computed = row.tolerance().expect("tolerance");
    assert!((computed - tolerance).abs() <= f64::EPSILON * tolerance);
    assert_eq!(row.agrees(), Some(false));
    let mut rows = Vec::new();
    let (a, b) = (scalar(100.0, 0.25), scalar(100.4, 0.25));
    quantity_rows(AREA, [Some(&a), Some(&b)], &CONTEXT, &mut rows);
    assert_eq!(rows[0].agrees(), Some(true));
}

#[test]
fn a_one_sided_value_disagrees_or_is_not_compared_by_category() {
    let open: Quantity<Scalar> = Quantity::missing(
        UnavailableReason::VolumeShellOpen,
        Some("test:model:body#b"),
        "open",
    );
    let mut rows = Vec::new();
    let closed = scalar(1000.0, 0.0);
    quantity_rows(AREA, [Some(&closed), Some(&open)], &CONTEXT, &mut rows);
    assert_eq!(rows[0].rule(), Rule::Exact);
    assert_eq!(rows[0].agrees(), Some(false));
    let exhausted: Quantity<Scalar> = Quantity::missing(
        UnavailableReason::EvaluationWorkExhausted,
        Some("test:model:body#b"),
        "exhausted",
    );
    let mut rows = Vec::new();
    quantity_rows(AREA, [Some(&closed), Some(&exhausted)], &CONTEXT, &mut rows);
    assert_eq!(rows[0].rule(), Rule::NotCompared);
    assert_eq!(rows[0].cause(), Some(CompareCause::Unavailable));
}

#[test]
fn unavailable_values_compare_by_their_candidates() {
    let candidate = |value: f64| {
        scalar(value, 0.0)
            .measured()
            .cloned()
            .expect("measured")
            .into_candidate(Reading::FaceSense)
    };
    let unavailable = |value: f64| {
        Quantity::Unavailable(
            Unavailable::new(
                UnavailableReason::VolumeOrientationUnverified,
                Some("test:model:body#a"),
                "unverified",
            )
            .with_candidates(vec![candidate(value)]),
        )
    };
    let mut rows = Vec::new();
    let (a, b) = (unavailable(500.0), unavailable(500.0));
    quantity_rows(
        Quantum {
            name: "volume",
            unit: Unit::CubicMillimeter,
            dimension: 3,
        },
        [Some(&a), Some(&b)],
        &CONTEXT,
        &mut rows,
    );
    let [row] = rows.as_slice() else {
        panic!("one row");
    };
    assert_eq!(row.quantity(), "volume.face_sense");
    assert_eq!(row.agrees(), Some(true));
}

fn bbox(min: [f64; 3], max: [f64; 3], kind: BoxKind, boundary: Option<Aabb>) -> Quantity<BoxValue> {
    let mut aabb = Aabb::of_point(Point3::new(min[0], min[1], min[2]));
    aabb.include(Point3::new(max[0], max[1], max[2]));
    measured(
        box_value(aabb, kind, boundary),
        Unit::Millimeter,
        Method::BoundaryExtremum,
        Errors::ZERO,
        "test:model:body#a",
    )
}

#[test]
fn a_tight_box_inside_a_conservative_box_is_contained() {
    let tight = bbox([0.0; 3], [1.0; 3], BoxKind::Tight, None);
    let mut boundary = Aabb::of_point(Point3::new(0.0, 0.0, 0.0));
    boundary.include(Point3::new(1.0, 1.0, 1.0));
    let hull = bbox([-1.0; 3], [2.0; 3], BoxKind::Conservative, Some(boundary));
    let mut rows = Vec::new();
    bbox_rows([&tight, &hull], &CONTEXT, &mut rows);
    let [row] = rows.as_slice() else {
        panic!("one row");
    };
    assert_eq!(row.rule(), Rule::Containment);
    assert_eq!(row.difference(), Some(0.0));
    assert_eq!(row.agrees(), Some(true));
    assert_eq!(row.a(), Some(&Operand::Text("tight".to_owned())));
    let small = bbox([0.25; 3], [0.5; 3], BoxKind::Conservative, None);
    let mut rows = Vec::new();
    bbox_rows([&tight, &small], &CONTEXT, &mut rows);
    assert_eq!(rows[0].difference(), Some(0.5));
    assert_eq!(rows[0].agrees(), Some(false));
}
