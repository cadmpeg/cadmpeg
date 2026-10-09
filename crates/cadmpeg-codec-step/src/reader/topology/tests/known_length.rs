// SPDX-License-Identifier: Apache-2.0
//! Empty known-length topology sources preserve the refusal fuse without work.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn with_empty_bound_source(run: impl FnOnce(&crate::parse::Exchange, &crate::reader::index::CarrierIndex)) {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("valid empty exchange");
    let setup_ctx = cadmpeg_test_support::service_decode_context();
    let ir = cadmpeg_ir::CadIr::empty();
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup_ctx)
        .expect("empty carrier index");
    run(&exchange, &carriers);
}

#[test]
fn empty_implicit_bounds_visit_no_terminal_step() {
    with_empty_bound_source(|exchange, carriers| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(super::super::implicit_face_points(&[], exchange, &BTreeMap::new(), carriers, &ctx)
            .expect("empty source has no work or storage").is_none());
        assert_eq!(ctx.resource_refusal(), None);
        ctx.finish_session().expect("empty source completed without refusal");
    });
}

#[test]
fn empty_implicit_bounds_preserve_original_refusal() {
    with_empty_bound_source(|exchange, carriers| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let CodecError::ResourceLimit(original) = ctx.charge_work(1, "test original topology refusal")
            .expect_err("seed original refusal") else { panic!("resource refusal"); };
        let CodecError::ResourceLimit(actual) = super::super::implicit_face_points(
            &[], exchange, &BTreeMap::new(), carriers, &ctx
        ).expect_err("empty path preserves fuse") else { panic!("resource refusal"); };
        assert_eq!(actual, original);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    });
}

fn polyline_domain_fixture(count: u32, parameterized: bool) -> cadmpeg_ir::geometry::SolvedCurveGeometry {
    use cadmpeg_ir::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
    use cadmpeg_ir::math::Point3;
    let samples = if parameterized {
        PolylineSamples::Parameterized {
            vertices: (0..count).map(|index| PolylineVertex {
                parameter: f64::from(index) * 2.0 - 7.0,
                point: Point3::new(f64::from(index), 0.0, 0.0),
            }).collect::<Vec<_>>().try_into().expect("at least two vertices"),
        }
    } else {
        PolylineSamples::Unparameterized {
            points: (0..count).map(|index| Point3::new(f64::from(index), 0.0, 0.0))
                .collect::<Vec<_>>().try_into().expect("at least two points"),
        }
    };
    let setup = cadmpeg_test_support::service_decode_context();
    cadmpeg_ir::geometry::SolvedCurveGeometry::Polyline(
        PolylineCurve::new(samples, 0.0, &setup)
            .expect("fixture resources").expect("finite ordered sample lane"),
    )
}

#[test]
fn parameterized_polyline_domain_uses_first_and_last_source_parameters() {
    for count in [2, 257] {
        let geometry = polyline_domain_fixture(count, true);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The existing geometry-node admission is independent of sample count.
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let domain = super::super::curve_selection_parameter_domain_from_geometry(&geometry, &ctx)
            .expect("endpoint access requires no sample scan");
        assert_eq!(domain, Some([-7.0, f64::from(count - 1) * 2.0 - 7.0]));
        assert_eq!(ctx.resource_refusal(), None);
        ctx.finish_session().expect("no scratch remains");
    }
}

#[test]
fn unparameterized_polyline_domain_does_not_synthesize_sample_index_parameters() {
    let geometry = polyline_domain_fixture(257, false);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert_eq!(super::super::curve_selection_parameter_domain_from_geometry(&geometry, &ctx)
        .expect("unparameterized sample lane"), None);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().expect("no scratch remains");
}

fn knot_break_fixture(polar: bool) -> cadmpeg_ir::geometry::pcurve::PcurveGeometry {
    use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs, PolarNurbsPole, PolarPcurveNurbs};
    use cadmpeg_ir::math::Point2;
    let setup = cadmpeg_test_support::service_decode_context();
    let knots = vec![0.0, 0.0, 1.0, 1.0];
    if polar {
        PcurveGeometry::PolarNurbs {
            nurbs: PolarPcurveNurbs::from_lanes(&setup, 1, knots, vec![
                PolarNurbsPole { radial: Point2::new(1.0, 0.0), axial: 0.0 },
                PolarNurbsPole { radial: Point2::new(0.0, 1.0), axial: 1.0 },
            ], None, false).expect("fixture resources").expect("valid polar knots and poles"),
        }
    } else {
        PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(&setup, 1, knots,
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)], None, false)
                .expect("fixture resources").expect("valid knots and poles"),
        }
    }
}

fn first_interior_knot_refuses_output_before_unvisited_suffix(polar: bool) {
    use cadmpeg_core::decode::ResourceDimension;
    let geometry = knot_break_fixture(polar);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One existing geometry-node unit and the first three actual knot visits.
    policy.limits.max_work_units = 4;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut fractions = Vec::new();
    let CodecError::ResourceLimit(refusal) = super::super::pcurve_parameter_break_fractions(
        &geometry, [0.0, 2.0], &mut fractions, &ctx
    ).expect_err("first interior knot requires an output slot") else { panic!("resource refusal"); };
    assert_eq!(refusal.dimension, ResourceDimension::CollectionItems);
    assert_eq!(refusal.operation, "step_pcurve_break_fractions");
    assert_eq!((refusal.used, refusal.additional), (0, 1));
    assert!(fractions.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == refusal));
}

#[test]
fn nurbs_knot_break_refuses_first_output_before_unvisited_suffix() {
    first_interior_knot_refuses_output_before_unvisited_suffix(false);
}

#[test]
fn polar_nurbs_knot_break_refuses_first_output_before_unvisited_suffix() {
    first_interior_knot_refuses_output_before_unvisited_suffix(true);
}

#[test]
fn knot_breaks_admit_each_visited_knot_without_a_terminal_step() {
    for polar in [false, true] {
        let geometry = knot_break_fixture(polar);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One existing geometry-node unit plus all four knots; first push moves no old values.
        policy.limits.max_work_units = 5;
        policy.limits.max_collection_items = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let (fractions, storage) = ctx.with_scoped_storage("test knot output", || {
            let mut fractions = Vec::new();
            super::super::pcurve_parameter_break_fractions(&geometry, [0.0, 2.0], &mut fractions, &ctx)?;
            Ok::<_, CodecError>(fractions)
        }).expect("all four knots fit the actual work and two output slots");
        assert_eq!(fractions, vec![0.5, 0.5]);
        drop(fractions);
        drop(storage);
        ctx.finish_session().expect("output scratch released");
    }
}

#[test]
fn empty_connected_faces_require_no_traversal_or_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(super::super::connected_face_components(&[], &[], &[], &BTreeMap::new(), &ctx)
        .expect("empty topology has no steps or storage").is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().expect("empty scratch released");
}
