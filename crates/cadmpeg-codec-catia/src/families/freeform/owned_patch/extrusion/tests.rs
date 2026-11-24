// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::eval::{admission::EvaluationAdmission, decode::nurbs_surface_point};

fn extrusion(bounds: [[f64; 2]; 2]) -> Extrusion {
    let mut bytes = vec![5];
    for value in [
        0.0_f64,
        0.0,
        1.0,
        bounds[1][0],
        bounds[1][1],
        1.0,
        0.0,
        0.0,
        0.0,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([5, 5]);
    Extrusion::read(&bytes, bounds).expect("extrusion chart")
}
fn offset_bytes(from: [f64; 2], result: [f64; 2], distance: f64) -> Vec<u8> {
    let mut bytes = vec![5];
    for value in from {
        bytes.extend(value.to_le_bytes());
    }
    bytes.push(5);
    for value in [distance, 0.0, 0.0, 1.0, result[0], result[1]] {
        bytes.extend(value.to_le_bytes());
    }
    bytes
}
#[test]
fn offset_extrusion_maps_the_result_interval_to_the_source_interval() {
    let chart = extrusion([[-0.2, 0.8], [0.0, 2.0]]);
    for distance in [-0.25, 0.25] {
        let (_, offset) = chart
            .offset(&offset_bytes([0.0, 1.0], [-0.2, 0.8], distance))
            .expect("extended result");
        let surface = chart
            .surface(&offset, [-1.0, 2.0], |t| {
                assert!((-1.0..=2.0).contains(&t));
                Some(Point3::new(t, 0.0, 0.0))
            })
            .expect("offset surface");
        for u in [-0.2, 0.1, 0.8] {
            for v in [0.0, 1.0, 2.0] {
                let point = nurbs_surface_point(EvaluationAdmission::Standard, &surface, u, v)
                    .expect("surface point");
                assert!(point.get().distance(Point3::new(u + 0.2, -distance, v)) < 1e-9);
            }
        }
    }
}
#[test]
fn offset_extrusion_rejects_oversized_and_out_of_knot_results() {
    let chart = extrusion([[-0.2, 0.8], [0.0, 2.0]]);
    assert!(chart
        .offset(&offset_bytes([0.0, 0.5], [-0.2, 0.8], 0.25))
        .is_none());
    let (_, offset) = chart
        .offset(&offset_bytes([0.0, 1.0], [-0.2, 0.8], 0.25))
        .expect("result interval");
    assert!(chart
        .surface(&offset, [0.1, 2.0], |t| Some(Point3::new(t, 0.0, 0.0)))
        .is_none());
    assert!(chart
        .offset(&offset_bytes([0.0, 1.0], [-0.2, 0.7], 0.25))
        .is_none());
    assert!(chart
        .offset(&offset_bytes([0.0, 1.0], [-0.2, 0.8], 0.0))
        .is_none());
}
#[test]
fn extrusion_ring_bounds_keep_the_directrix_and_linear_axis() {
    let original = extrusion([[0.0, 1.0], [0.0, 2.0]]);
    let (_, offset) = original
        .offset(&offset_bytes([0.0, 1.0], [0.0, 1.0], 0.25))
        .expect("offset");
    let extended = original.expanded_to_include([[-0.25, 1.25], [0.2, 1.8]]);
    assert_eq!(extended.directrix, original.directrix);
    assert_eq!(extended.bounds, [[-0.25, 1.25], [0.0, 2.0]]);
    assert!(extended.extends_source(&offset));
    let surface = extended
        .surface(&offset, [-1.0, 2.0], |t| Some(Point3::new(t, 0.0, 0.0)))
        .expect("ring interval");
    let point =
        nurbs_surface_point(EvaluationAdmission::Standard, &surface, -0.25, 1.8).expect("boundary");
    assert!(point.get().distance(Point3::new(-0.25, -0.25, 1.8)) < 1e-9);
}

#[test]
fn curved_offset_extrusion_maps_shifted_and_scaled_result_coordinates() {
    for result in [[-0.2, 0.8], [2.0, 2.5]] {
        let chart = extrusion([result, [0.0, 2.0]]);
        let (_, offset) = chart
            .offset(&offset_bytes([0.0, 1.0], result, 0.25))
            .expect("mapped offset");
        let surface = chart
            .surface(&offset, [0.0, 1.0], |t| {
                assert!((0.0..=1.0).contains(&t));
                Some(Point3::new(t, t * t, 0.0))
            })
            .expect("curved offset surface");
        for fraction in [0.1_f64, 0.4, 0.7] {
            let u = result[0] + fraction * (result[1] - result[0]);
            let normal_length = (4.0 * fraction * fraction + 1.0).sqrt();
            let expected = Point3::new(
                fraction + 0.5 * fraction / normal_length,
                fraction * fraction - 0.25 / normal_length,
                1.0,
            );
            let point = nurbs_surface_point(EvaluationAdmission::Standard, &surface, u, 1.0)
                .expect("mapped point")
                .get();
            assert!(
                point.distance(expected) < 0.0005,
                "{point:?} vs {expected:?}"
            );
        }
    }
}

#[test]
fn shifted_ring_bounds_preserve_both_ends_of_the_original_chart() {
    let original = extrusion([[0.0, 1.0], [0.0, 2.0]]);
    let (_, offset) = original
        .offset(&offset_bytes([0.0, 1.0], [0.0, 1.0], 0.25))
        .expect("offset");
    let expanded = original.expanded_to_include([[0.2, 1.2], [0.2, 1.8]]);
    assert_eq!(expanded.bounds, [[0.0, 1.2], [0.0, 2.0]]);
    let surface = expanded
        .surface(&offset, [-1.0, 2.0], |t| Some(Point3::new(t, 0.0, 0.0)))
        .expect("expanded chart");
    for u in [0.0, 0.1, 1.0, 1.2] {
        for v in [0.0, 2.0] {
            let p = nurbs_surface_point(EvaluationAdmission::Standard, &surface, u, v)
                .expect("retained chart point")
                .get();
            assert!(p.distance(Point3::new(u, -0.25, v)) < 1e-9);
        }
    }
    assert!(expanded
        .surface(&offset, [0.0, 1.1], |t| Some(Point3::new(t, 0.0, 0.0)))
        .is_none());
}

#[test]
fn offset_interval_keeps_the_existing_per_side_tolerance_band() {
    let result = [-0.9e-9, 1.0 + 0.9e-9];
    let chart = extrusion([result, [0.0, 2.0]]);
    let (_, offset) = chart
        .offset(&offset_bytes([0.0, 1.0], result, 0.25))
        .expect("per-side tolerance");
    assert!(chart
        .surface(&offset, [0.0, 1.0], |t| Some(Point3::new(t, 0.0, 0.0)))
        .is_some());
}

#[test]
fn contained_result_retains_the_existing_source_gauge() {
    let result = [0.2, 0.8];
    let chart = extrusion([result, [0.0, 2.0]]);
    let (_, offset) = chart
        .offset(&offset_bytes([0.0, 1.0], result, 0.25))
        .expect("contained result");
    assert!(!offset.maps_result());
    let surface = chart
        .surface(&offset, [0.0, 1.0], |t| Some(Point3::new(t, 0.0, 0.0)))
        .expect("cropped directrix");
    for u in result {
        let p = nurbs_surface_point(EvaluationAdmission::Standard, &surface, u, 1.0)
            .expect("crop point")
            .get();
        assert!(p.distance(Point3::new(u, -0.25, 1.0)) < 1e-9);
    }
}
