// SPDX-License-Identifier: Apache-2.0
//! Exact tensor-product restriction keeps the original parameter chart.
#![allow(clippy::unwrap_used)]
use super::{
    homogeneous_bezier_extraction, insert_homogeneous_knot, trim_surface, BlendCheck,
    InsertedKnotNet, RefiningNet,
};
use cadmpeg_core::decode::index_from_u32;
use cadmpeg_ir::eval::{admission::EvaluationAdmission, decode::nurbs_surface_point};
use cadmpeg_ir::geometry::nurbs::bezier::homogeneous_spans;
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::math::Point3;

const EPS_TRIM_POINT: f64 = 1.0e-12;

/// A positive-weight homogeneous net whose values vary with the index.
fn varied_controls(count: u32) -> Vec<[f64; 4]> {
    (0..count)
        .map(|index| {
            let weight = 1.0 + f64::from(index % 7) / 10.0;
            let t = f64::from(index);
            [weight, weight * t.sin(), weight * t.cos(), weight * 0.5 * t]
        })
        .collect()
}

#[test]
fn refining_net_matches_strict_knot_insertion() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let degree = 3;
    let count = 40_u32;
    let mut knots = vec![0.0; degree + 1];
    knots.extend((1..count - 3).map(f64::from));
    knots.extend([f64::from(count - 3); 4]);
    let controls = varied_controls(count);
    let internal: Vec<f64> = (1..count - 3).map(f64::from).collect();

    let (mut strict_controls, mut strict_knots) = (controls.clone(), knots.clone());
    for &value in &internal {
        for _ in 1..degree {
            let InsertedKnotNet {
                control_points,
                knots: inserted_knots,
            } = insert_homogeneous_knot(&ctx, &strict_controls, &strict_knots, degree, value)
                .unwrap()
                .unwrap();
            (strict_controls, strict_knots) = (control_points, inserted_knots);
        }
    }
    for check in [BlendCheck::PositiveWeight, BlendCheck::Denominator] {
        let mut net = RefiningNet::new(&ctx, &knots, &controls, internal.len() * 2).unwrap();
        for &value in &internal {
            for _ in 1..degree {
                net.insert(value, degree, check).unwrap();
            }
        }
        let (refined_knots, refined_controls) = net.into_parts();
        assert_eq!(refined_knots, strict_knots);
        assert_eq!(refined_controls, strict_controls);
    }
}

#[test]
fn bezier_extraction_matches_the_ir_extraction() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let degree = 3;
    let controls = varied_controls(30);
    // Unclamped ends and internal knots of multiplicity one to four.
    let mut knots = vec![-3.0, -2.5, -1.0, 0.0];
    for value in 1..=8 {
        for _ in 0..=(value % 4) {
            knots.push(f64::from(value));
        }
    }
    while knots.len() < controls.len() + degree + 1 {
        let next = knots.last().unwrap() + 0.75;
        knots.push(next);
    }

    let expected = homogeneous_spans(degree, &knots, &controls).unwrap();
    let spans = homogeneous_bezier_extraction(&ctx, degree, &knots, &controls)
        .unwrap()
        .unwrap();
    assert_eq!(spans.len(), expected.len());
    for (span, expected) in spans.iter().zip(&expected) {
        assert_eq!(span.domain, expected.domain);
        assert_eq!(span.controls, expected.controls);
    }
}

#[test]
fn bezier_extraction_of_a_long_curve_inserts_each_knot_near_its_span() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let count = 1_u32 << 18;
    let degree = 2;
    let knots: Vec<f64> = (0..=count + 2).map(f64::from).collect();
    let controls = varied_controls(count);

    let spans = homogeneous_bezier_extraction(&ctx, degree, &knots, &controls)
        .unwrap()
        .unwrap();
    assert_eq!(spans.len(), index_from_u32(count) - degree);
    assert_eq!(spans[0].domain, [2.0, 3.0]);
    assert_eq!(
        spans.last().unwrap().domain,
        [f64::from(count - 1), f64::from(count)]
    );
    // Spans away from the prefix's own end see the same knots and controls.
    let prefix = homogeneous_spans(degree, &knots[..=64 + degree], &controls[..64]).unwrap();
    for (span, expected) in spans.iter().zip(&prefix).take(prefix.len() - 4) {
        assert_eq!(span.controls, expected.controls);
    }
}

fn rational_surface() -> NurbsSurface {
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            (0..4)
                .map(|u| {
                    (0..3)
                        .map(|v| Point3::new(f64::from(u), f64::from(v), f64::from(u * v)))
                        .collect()
                })
                .collect(),
            Some(vec![
                vec![1.0, 1.5, 1.0],
                vec![2.0, 1.0, 0.5],
                vec![1.0, 2.0, 1.0],
                vec![0.5, 1.0, 2.0],
            ]),
        ),
        true,
    )
    .unwrap()
}

#[test]
fn rational_multispan_surface_crop_preserves_points_and_normal_chart() {
    let source = rational_surface();
    let cropped = trim_surface(
        &cadmpeg_test_support::service_decode_context(),
        &source,
        [[0.2, 0.8], [0.1, 0.9]],
    )
    .unwrap()
    .unwrap();
    assert!(cropped.normal_reversed());
    assert_eq!(cropped.u_knots()[index_from_u32(cropped.u_degree())], 0.2);
    assert_eq!(cropped.u_knots()[cropped.u_count()], 0.8);
    assert_eq!(cropped.v_knots()[index_from_u32(cropped.v_degree())], 0.1);
    assert_eq!(cropped.v_knots()[cropped.v_count()], 0.9);
    assert!(cropped.weights().is_some());
    for u in [0.2, 0.35, 0.5, 0.65, 0.8] {
        for v in [0.1, 0.4, 0.7, 0.9] {
            let before = nurbs_surface_point(EvaluationAdmission::Standard, &source, u, v).unwrap();
            let after = nurbs_surface_point(EvaluationAdmission::Standard, &cropped, u, v).unwrap();
            assert!(
                before.get().distance(after.get()) <= EPS_TRIM_POINT,
                "{u}, {v}"
            );
        }
    }
}

#[test]
fn surface_crop_refuses_ranges_outside_the_basis() {
    let source = rational_surface();
    let ctx = cadmpeg_test_support::service_decode_context();
    for ranges in [
        [[0.0, 1.1], [0.0, 1.0]],
        [[0.0, 1.0], [-0.1, 1.0]],
        [[0.4, 0.4], [0.0, 1.0]],
    ] {
        assert!(trim_surface(&ctx, &source, ranges).unwrap().is_none());
    }
}

#[test]
fn surface_crop_refuses_a_discontinuous_terminal_boundary() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let source = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 0.5, 0.5, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            (0..4)
                .map(|u| {
                    vec![
                        Point3::new(f64::from(u), 0.0, 0.0),
                        Point3::new(f64::from(u), 1.0, 0.0),
                    ]
                })
                .collect(),
            None,
        ),
        false,
    )
    .unwrap();
    let source_endpoint =
        nurbs_surface_point(EvaluationAdmission::Standard, &source, 0.5, 0.5).unwrap();
    assert_eq!(source_endpoint.get(), Point3::new(2.0, 0.5, 0.0));
    assert!(trim_surface(&ctx, &source, [[0.0, 0.5], [0.0, 1.0]])
        .unwrap()
        .is_none());
}
