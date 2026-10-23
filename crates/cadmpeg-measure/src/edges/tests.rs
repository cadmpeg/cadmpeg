// SPDX-License-Identifier: Apache-2.0
use std::f64::consts::PI;

use cadmpeg_ir::geometry::analytic::EllipseCurve;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::BodyKind;

use super::measure_edge;
use crate::budget::{PassAllowance, WorkProfile};
use crate::canonical::placement::Placement;
use crate::context::{Eval, Work};
use crate::owners::Owners;
use crate::reason::UnavailableReason;
use crate::test_support::{service_context, Builder};
use crate::value::Method;

/// One edge's interval and its length with the length's method.
type Measured = (super::Outcome<[f64; 2]>, Option<(f64, Method)>);

/// Measure every edge of `ir`.
fn edges(ir: &cadmpeg_ir::CadIr) -> Vec<Measured> {
    let ctx = service_context();
    let owners = Owners::new(ir);
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    let pass = PassAllowance::new(WorkProfile::Desktop.pass_allowance(1).unwrap());
    ir.model
        .edges
        .iter()
        .map(|edge| {
            let slice = pass.edge_slice();
            let eval = Eval {
                ctx: &ctx,
                index: &index,
                work: &slice,
            };
            let geometry =
                measure_edge(&eval, &owners, edge, &Ok(Placement::identity()), Work::Row).unwrap();
            (
                geometry.interval.clone(),
                geometry
                    .length
                    .as_ref()
                    .ok()
                    .map(|length| (length.value, length.method)),
            )
        })
        .collect()
}

#[test]
fn a_closed_circle_without_a_range_spans_one_period_from_its_vertex() {
    let mut builder = Builder::new();
    let vertex = builder.vertex([0.0, 3.0, 0.0]);
    builder.circle(
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        3.0,
        &vertex,
    );
    let ir = builder.finish(BodyKind::Wire);
    let [(interval, length)] = edges(&ir).try_into().ok().unwrap();
    let [t0, t1] = interval.unwrap();
    assert!((t0 - PI / 2.0).abs() < 1.0e-15);
    assert!((t1 - t0 - 2.0 * PI).abs() < 1.0e-15);
    let (value, method) = length.unwrap();
    assert!((value - 6.0 * PI).abs() < 1.0e-12);
    assert_eq!(method, Method::ClosedForm);
}

#[test]
fn an_open_arc_without_a_range_runs_in_the_increasing_parameter_direction() {
    let mut builder = Builder::new();
    let start = builder.vertex([0.0, -1.0, 0.0]);
    let end = builder.vertex([0.0, 1.0, 0.0]);
    let arc = builder.circle(
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        1.0,
        &start,
    );
    let mut ir = builder.finish(BodyKind::Wire);
    let edge = ir
        .model
        .edges
        .iter_mut()
        .find(|edge| edge.id == arc)
        .unwrap();
    edge.end = end;
    let [(interval, length)] = edges(&ir).try_into().ok().unwrap();
    let [t0, t1] = interval.unwrap();
    assert!((t0 + PI / 2.0).abs() < 1.0e-15, "{t0}");
    assert!((t1 - PI / 2.0).abs() < 1.0e-15, "{t1}");
    assert!((length.unwrap().0 - PI).abs() < 1.0e-12);
}

#[test]
fn a_closed_edge_on_a_curve_without_a_period_has_no_interval() {
    let mut builder = Builder::new();
    let a = builder.vertex([0.0, 0.0, 0.0]);
    let b = builder.vertex([1.0, 0.0, 0.0]);
    let line = builder.line(&a, &b);
    let mut ir = builder.finish(BodyKind::Wire);
    let edge = ir
        .model
        .edges
        .iter_mut()
        .find(|edge| edge.id == line)
        .unwrap();
    edge.carrier = cadmpeg_ir::topology::EdgeCarrier::unbounded(edge.carrier.curve().cloned());
    edge.end = edge.start.clone();
    let [(interval, length)] = edges(&ir).try_into().ok().unwrap();
    assert_eq!(
        interval.unwrap_err().reason,
        UnavailableReason::BoundaryEdgeRangeUnresolved
    );
    assert!(length.is_none());
}

#[test]
fn an_ellipse_quarter_integrates_to_its_series_length() {
    let mut builder = Builder::new();
    let start = builder.vertex([3.0, 0.0, 0.0]);
    let end = builder.vertex([0.0, 2.0, 0.0]);
    let arc = builder.circle(
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        1.0,
        &start,
    );
    let mut ir = builder.finish(BodyKind::Wire);
    let curve_id = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id == arc)
        .and_then(|edge| edge.carrier.curve().cloned())
        .unwrap();
    let curve = ir
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap();
    curve.geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
        EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            2.0,
        )
        .unwrap(),
    ));
    let edge = ir
        .model
        .edges
        .iter_mut()
        .find(|edge| edge.id == arc)
        .unwrap();
    edge.end = end;
    let [(interval, length)] = edges(&ir).try_into().ok().unwrap();
    let [t0, t1] = interval.unwrap();
    assert!((t1 - t0 - PI / 2.0).abs() < 1.0e-15);
    // Quarter perimeter of an ellipse with a = 3, b = 2: a·E(e), e² = 1 − b²/a²,
    // with E(m) = (π/2)·Σ ((2n)!/(2^{2n} n!²))² · m^n / (1 − 2n).
    let m = 1.0 - 4.0 / 9.0;
    let mut term = 1.0_f64;
    let mut sum = 0.0_f64;
    for n in 0..200 {
        let n = f64::from(n);
        if n > 0.0 {
            term *= ((2.0 * n - 1.0) / (2.0 * n)).powi(2) * m;
        }
        sum += term / (1.0 - 2.0 * n);
    }
    let expected = 3.0 * PI / 2.0 * sum;
    let (value, method) = length.unwrap();
    assert_eq!(method, Method::BoundaryQuadrature);
    assert!(
        (value - expected).abs() < 1.0e-9 * expected,
        "{value} {expected}"
    );
}
