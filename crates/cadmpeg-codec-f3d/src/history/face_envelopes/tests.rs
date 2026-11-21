// SPDX-License-Identifier: Apache-2.0
use super::{conic_points, envelope};
use cadmpeg_ir::math::{Point3, Vector3};
use std::f64::consts::PI;

const TOLERANCE: f64 = 1.0e-12;

fn rational_edge_bound(
    weight: f64,
    range: Option<[f64; 2]>,
) -> Option<crate::history_records::AsmHistoricalEnvelope> {
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3, WeightedPole3};
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::{CurveId, EdgeId};
    use cadmpeg_ir::scalar::NonZeroReal;
    use cadmpeg_ir::topology::{Edge, EdgeCarrier};
    use std::collections::HashMap;

    let points = [
        (Point3::new(0.0, 0.0, 0.0), 1.0),
        (Point3::new(2.0, 3.0, 1.0), weight),
        (Point3::new(4.0, 0.0, 0.0), 2.0),
    ]
    .into_iter()
    .map(|(point, weight)| WeightedPole3 {
        point,
        weight: NonZeroReal::new(weight).unwrap(),
    })
    .collect();
    let nurbs = NurbsCurve::new(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        NurbsPoles3::Rational { points },
        false,
    )
    .unwrap();
    let curve = Curve {
        id: CurveId::mint("test:brep:curve#2").unwrap(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
        parameter_range: None,
        source_object: None,
        native_ref: None,
    };
    let edge = Edge {
        id: EdgeId::mint("test:brep:edge#1").unwrap(),
        carrier: EdgeCarrier::new(Some(curve.id.clone()), range).unwrap(),
        ends: None,
        tolerance: None,
        native_ref: None,
    };
    let curves = HashMap::from([(&curve.id, Some(&curve))]);
    super::edge_envelope(&edge, &curves, &mut HashMap::new(), None)
}

#[test]
fn a_positive_weight_nurbs_is_bounded_by_its_control_hull() {
    let bound = rational_edge_bound(3.0, Some([0.0, 1.0])).unwrap();
    assert_eq!(bound.low, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(bound.high, Point3::new(4.0, 3.0, 1.0));
}

#[test]
fn a_signed_weight_or_extrapolated_nurbs_has_no_hull_certificate() {
    assert!(rational_edge_bound(-1.0, Some([0.0, 1.0])).is_none());
    assert!(rational_edge_bound(1.0, Some([-0.1, 1.0])).is_none());
    assert!(rational_edge_bound(1.0, None).is_none());
}

#[test]
fn circular_envelope_contains_interior_coordinate_extrema() {
    let bounds = envelope(
        1,
        conic_points(
            Point3::new(2.0, 3.0, 4.0),
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::new(0.0, 5.0, 0.0),
            [0.25 * PI, 1.75 * PI],
            false,
        ),
    )
    .unwrap();
    assert!((bounds.low.x + 3.0).abs() <= TOLERANCE);
    assert!((bounds.low.y + 2.0).abs() <= TOLERANCE);
    assert!((bounds.high.y - 8.0).abs() <= TOLERANCE);
    assert_eq!(bounds.low.z, 4.0);
    assert_eq!(bounds.high.z, 4.0);
}

#[test]
fn hyperbolic_envelope_contains_its_interior_minimum() {
    let bounds = envelope(
        2,
        conic_points(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            [-1.0, 1.0],
            true,
        ),
    )
    .unwrap();
    assert!((bounds.low.x - 2.0).abs() <= TOLERANCE);
    assert!((bounds.high.x - 2.0 * 1.0_f64.cosh()).abs() <= TOLERANCE);
    assert!((bounds.low.y + 1.0_f64.sinh()).abs() <= TOLERANCE);
}

#[test]
fn a_large_circular_parameter_uses_the_full_carrier_bound() {
    let bounds = envelope(
        6,
        conic_points(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 0.0),
            Vector3::new(0.0, 3.0, 0.0),
            [1.0e30, 1.0e30],
            false,
        ),
    )
    .unwrap();
    assert_eq!(bounds.low, Point3::new(-2.0, -3.0, 0.0));
    assert_eq!(bounds.high, Point3::new(2.0, 3.0, 0.0));
}

#[test]
fn an_unwrapped_circular_interval_keeps_its_partial_arc_bound() {
    let phase = 8.0 * PI;
    let bounds = envelope(
        7,
        conic_points(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            [phase - PI / 6.0, phase + PI / 6.0],
            false,
        ),
    )
    .unwrap();
    assert!((bounds.low.x - 3.0_f64.sqrt() / 2.0).abs() <= TOLERANCE);
    assert!((bounds.high.x - 1.0).abs() <= TOLERANCE);
    assert!((bounds.low.y + 0.5).abs() <= TOLERANCE);
    assert!((bounds.high.y - 0.5).abs() <= TOLERANCE);
}

#[test]
fn an_empty_or_nonfinite_envelope_is_unavailable() {
    assert!(envelope(3, []).is_none());
    assert!(envelope(3, [Point3::new(f64::NAN, 0.0, 0.0)]).is_none());
}

#[test]
fn a_cone_envelope_includes_an_enclosed_pole() {
    let cone = cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        1.0,
        1.0,
        PI / 4.0,
    )
    .unwrap();
    let boundary = envelope(
        4,
        [Point3::new(-1.0, -1.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
    )
    .unwrap();
    let bounds = super::cone_envelope(boundary, &cone).unwrap();
    assert!((bounds.low.z + 1.0).abs() <= TOLERANCE);
    assert_eq!(bounds.high.z, 0.0);
}

#[test]
fn a_cone_patch_in_one_radial_half_plane_excludes_the_pole() {
    let cone = cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        1.0,
        1.0,
        PI / 4.0,
    )
    .unwrap();
    let boundary = envelope(5, [Point3::new(0.5, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0)]).unwrap();
    assert_eq!(
        super::cone_envelope(boundary.clone(), &cone),
        Some(boundary)
    );
}
