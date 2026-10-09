// SPDX-License-Identifier: Apache-2.0

use super::super::{admit_surface_pole_count, angular_basis, bernstein_binomial,
    equal_arc_length_parameterization, homogeneous_curve_boundary_matches,
    homogeneous_product_control, interval_certified_linear_bezier,
    pair_admitted_surface_poles, projectively_shared_weights, ruled_surface_span_lanes,
    source_parameter_interval, split_homogeneous_bezier_span, SurfaceGridWeight,
    MAX_SURFACE_POLES};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::bezier::HomogeneousBezierSpan;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoleGrid};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::NonZeroReal;
use crate::global::ProjectedGlobal;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_ir::geometry::analytic::LineCurve;

fn rail(degree: u32, knots: Vec<f64>, poles: Vec<Point3>) -> NurbsCurve {
    crate::test_support::with_service_context(&[], |ctx| {
        NurbsCurve::from_lanes(ctx, degree, knots, poles, None, false)
            .expect("fixture admission").expect("valid fixture")
    })
}

fn linear_rail() -> NurbsCurve {
    rail(1, vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)])
}

fn line() -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
            .expect("valid line"),
    ))
}

fn global() -> ProjectedGlobal {
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    crate::test_support::with_service_context(&bytes, |ctx| {
        let scan = crate::card::scan_with_context(&bytes, ctx).expect("fixture cards");
        let (global, _, _storage) = crate::global::parse(&scan, ctx).expect("fixture global");
        global.length_context().expect("fixture length context")
    })
}

fn fixed_result<T>(result: Result<T, CodecError>, original: Option<ResourceLimit>) -> Option<T> {
    match original {
        Some(first) => {
            assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
            None
        }
        None => Some(result.expect("fixed recovery is free")),
    }
}

#[test]
fn surface_admitted_weight_preserves_original_refusal() {
    let weight = NonZeroReal::new(1.0).expect("nonzero weight");
    crate::test_support::with_entry_context(|ctx, original| {
        let result = weight.admit(0, ctx);
        if let Some(value) = fixed_result(result, original) {
            assert_eq!(value.expect("admitted weight").get(), 1.0);
        }
    });
}

#[test]
fn surface_valid_raw_weight_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let result = 1.0_f64.admit(0, ctx);
        if let Some(value) = fixed_result(result, original) {
            assert_eq!(value.expect("valid weight").get(), 1.0);
        }
    });
}

#[test]
fn surface_polynomial_grid_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let result = pair_admitted_surface_poles::<NonZeroReal>(ctx, Vec::new(), None, "test row", "test pole");
        if let Some(value) = fixed_result(result, original) {
            assert!(matches!(value, Ok(NurbsPoleGrid::Polynomial { rows }) if rows.is_empty()));
        }
    });
}

#[test]
fn surface_absent_bezier_record_preserves_original_refusal() {
    let geometry = linear_rail();
    let record = ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), 0, Vec::new(), Vec::new());
    let global = global();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = interval_certified_linear_bezier(&geometry, &record, &global, ctx);
        if let Some(value) = fixed_result(result, original) {
            assert!(!value);
        }
    });
}

#[test]
fn surface_invalid_arc_length_interval_preserves_original_refusal() {
    let geometry = line();
    let global = global();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = equal_arc_length_parameterization([(&geometry, 1), (&geometry, 3)], [0.0, 0.0], [0.0, 1.0], &[], &global, ctx);
        if let Some(value) = fixed_result(result, original) {
            assert!(!value);
        }
    });
}

#[test]
fn surface_constant_speed_rails_preserve_original_refusal() {
    let geometry = line();
    let global = global();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = equal_arc_length_parameterization([(&geometry, 1), (&geometry, 3)], [0.0, 1.0], [0.0, 1.0], &[], &global, ctx);
        if let Some(value) = fixed_result(result, original) {
            assert!(value);
        }
    });
}

#[test]
fn surface_plain_line_interval_preserves_original_refusal() {
    let geometry = line();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = source_parameter_interval(&geometry, [2.0, 3.0], ctx);
        if let Some(value) = fixed_result(result, original) {
            assert_eq!(value, [0.0, 1.0]);
        }
    });
}

#[test]
fn surface_invalid_binomial_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let result = bernstein_binomial(0, 1, ctx);
        if let Some(value) = fixed_result(result, original) {
            assert_eq!(value, None);
        }
    });
}

#[test]
fn surface_empty_product_controls_preserve_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let result = homogeneous_product_control(&[], &[], 0, ctx);
        if let Some(value) = fixed_result(result, original) {
            assert_eq!(value, None);
        }
    });
}

#[test]
fn surface_invalid_span_cut_preserves_original_refusal() {
    let span = HomogeneousBezierSpan { domain: [0.0, 1.0], controls: Vec::new() };
    crate::test_support::with_entry_context(|ctx, original| {
        let result = split_homogeneous_bezier_span(&span, 0.0, ctx);
        if let Some(value) = fixed_result(result, original) {
            assert!(value.is_none());
        }
    });
}

#[test]
fn surface_mismatched_weight_counts_preserve_original_refusal() {
    let first = linear_rail();
    let second = rail(1, vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.5, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]);
    crate::test_support::with_entry_context(|ctx, original| {
        let result = projectively_shared_weights(&first, &second, ctx);
        if let Some(value) = fixed_result(result, original) {
            assert!(value.is_none());
        }
    });
}

#[test]
fn surface_admitted_pole_count_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let result = admit_surface_pole_count(ctx, MAX_SURFACE_POLES);
        let _ = fixed_result(result, original);
    });
}

#[test]
fn surface_constant_rail_spans_preserve_original_refusal() {
    let constant = rail(0, vec![0.0, 1.0], vec![Point3::new(0.0, 0.0, 0.0)]);
    crate::test_support::with_entry_context(|ctx, original| {
        let result = ruled_surface_span_lanes(&constant, &constant, ctx);
        if let Some(value) = fixed_result(result, original) {
            assert!(value.is_none());
        }
    });
}

#[test]
fn surface_invalid_boundary_resolution_preserves_original_refusal() {
    let geometry = linear_rail();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = homogeneous_curve_boundary_matches(ctx, &geometry, &geometry, [0.0, 1.0], -1.0);
        if let Some(value) = fixed_result(result, original) {
            assert_eq!(value, None);
        }
    });
}

#[test]
fn surface_invalid_angular_span_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let result = angular_basis(0.0, 0.0, ctx);
        if let Some(value) = fixed_result(result, original) {
            assert!(value.is_none());
        }
    });
}
