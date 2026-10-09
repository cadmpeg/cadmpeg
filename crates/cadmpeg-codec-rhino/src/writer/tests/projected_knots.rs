// SPDX-License-Identifier: Apache-2.0

use crate::writer::{
    generated_projected_brep_c2_curve,
    model::{WritableEdge, WritableEdgeCurve},
    nurbs_curve_payload_dimension, NURBS_CURVE_CLASS,
};
use cadmpeg_ir::{
    geometry::nurbs::NurbsCurve,
    ids::{CurveId, EdgeId, VertexId},
    math::{Point3, Vector3},
    scalar::FiniteReal,
    topology::{Edge, EdgeCarrier, Sense},
};

fn projected_reversal(domain: [f64; 2], middle: f64, reflected_middle: f64) {
    assert!(!(domain[0] + domain[1]).is_finite());
    let ctx = cadmpeg_test_support::service_decode_context();
    let curve = NurbsCurve::from_lanes(
        &ctx,
        1,
        vec![domain[0], domain[0], middle, domain[1], domain[1]],
        vec![
            Point3::new(2.0, 3.0, 4.0),
            Point3::new(3.0, 4.0, 4.0),
            Point3::new(4.0, 3.0, 4.0),
        ],
        Some(vec![1.0, 0.5, 2.0]),
        false,
    )
    .expect("fixture constructor admission")
    .expect("finite curve lanes");
    let original = curve.clone();
    let source = Edge {
        id: EdgeId::mint("test:writer:edge#projected").expect("identity grammar"),
        carrier: EdgeCarrier::new(
            Some(CurveId::mint("test:writer:curve#projected").expect("identity grammar")),
            Some(domain),
        )
        .expect("finite domain"),
        start: VertexId::mint("test:writer:vertex#start").expect("identity grammar"),
        end: VertexId::mint("test:writer:vertex#end").expect("identity grammar"),
        tolerance: None,
    };
    let edge = WritableEdge {
        source: &source,
        start: 0,
        end: 1,
        domain: domain.map(|value| FiniteReal::new(value).expect("finite domain")),
        curve_id: "projected",
        curve: WritableEdgeCurve::Nurbs(&curve),
        uses: Vec::new(),
    };
    // The projected lane has x - 2, y - 3, z = 0. Reversal reflects knots
    // in the full domain and reverses both poles and their rational weights.
    for (sense, knot, points, weights) in [
        (
            Sense::Forward,
            middle,
            [
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
            ],
            [1.0, 0.5, 2.0],
        ),
        (
            Sense::Reversed,
            reflected_middle,
            [
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(0.0, 0.0, 0.0),
            ],
            [2.0, 0.5, 1.0],
        ),
    ] {
        let expected = NurbsCurve::from_lanes(
            &ctx,
            1,
            vec![domain[0], domain[0], knot, domain[1], domain[1]],
            points.to_vec(),
            Some(weights.to_vec()),
            false,
        )
        .expect("fixture constructor admission")
        .expect("specified reflected finite lanes");
        let actual = generated_projected_brep_c2_curve(
            &[],
            &edge,
            sense,
            Point3::new(2.0, 3.0, 4.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        )
        .expect("finite reflection is writable");
        assert_eq!(actual.0, NURBS_CURVE_CLASS);
        assert_eq!(
            actual.1,
            nurbs_curve_payload_dimension(&expected, 2).expect("native two-dimensional lanes")
        );
        assert_eq!(curve, original);
    }
}

#[test]
fn projected_trim_reflection_accepts_overflowing_positive_domain_sum() {
    let scale = 2.0_f64.powi(1023);
    projected_reversal([scale, 1.5 * scale], 1.125 * scale, 1.375 * scale);
}

#[test]
fn projected_trim_reflection_accepts_overflowing_negative_domain_sum() {
    let scale = 2.0_f64.powi(1023);
    projected_reversal([-1.5 * scale, -scale], -1.375 * scale, -1.125 * scale);
}

#[test]
fn projected_trim_reflection_refuses_a_nonfinite_reflected_outer_knot() {
    let scale = 2.0_f64.powi(1023);
    let domain = [scale, 1.5 * scale];
    let curve = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        2,
        vec![
            -f64::MAX,
            -f64::MAX,
            domain[0],
            domain[1],
            domain[1],
            domain[1],
        ],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("fixture constructor admission")
    .expect("finite nonclamped curve lanes");
    crate::writer::check_nurbs_curve("outer-knot", &curve)
        .expect("the original finite lane is native-canonical");
    let original = curve.clone();
    let source = Edge {
        id: EdgeId::mint("test:writer:edge#outer-knot").expect("identity grammar"),
        carrier: EdgeCarrier::new(
            Some(CurveId::mint("test:writer:curve#outer-knot").expect("identity grammar")),
            Some(domain),
        )
        .expect("finite domain"),
        start: VertexId::mint("test:writer:vertex#start").expect("identity grammar"),
        end: VertexId::mint("test:writer:vertex#end").expect("identity grammar"),
        tolerance: None,
    };
    let edge = WritableEdge {
        source: &source,
        start: 0,
        end: 1,
        domain: domain.map(|value| FiniteReal::new(value).expect("finite domain")),
        curve_id: "outer-knot",
        curve: WritableEdgeCurve::Nurbs(&curve),
        uses: Vec::new(),
    };
    let error = generated_projected_brep_c2_curve(
        &[],
        &edge,
        Sense::Reversed,
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    )
    .expect_err("start + end - (-MAX) exceeds the finite archive lane");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(message)
        if message == "curve outer-knot reflected knots are not finite"));
    assert_eq!(curve, original);
}
