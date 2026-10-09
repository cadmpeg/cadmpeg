// SPDX-License-Identifier: Apache-2.0
//! Demanded source reuse, alias identity, progressive refusal and retry order.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

use crate::test_support::parasolid::{f64_array, nurbs_curve_carrier, nurbs_surface_carrier};
use super::Sources;
use crate::brep::spline::{array_body, scan_curve_carriers, scan_surface_carriers};

fn unlimited_work() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy
}

fn require_armed(ctx: DecodeContext<'_>, operation: &'static str) {
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, operation) else {
        panic!("unvisited source operation probe remains armed");
    };
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn descriptor_demands_reuse_positive_and_negative_results() {
    for surface in [false, true] {
        for attr in [171, 65000] {
            let bytes = if surface { nurbs_surface_carrier(170, 171, 20) } else { nurbs_curve_carrier(170, 171) };
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &unlimited_work()).unwrap();
            let mut sources = Sources::new(&ctx, &bytes, true).unwrap();
            let operation = if surface { "scan Parasolid surface descriptors" } else { "scan Parasolid curve descriptors" };
            if surface {
                assert_eq!(sources.surface(attr).unwrap().is_some(), attr == 171);
            } else {
                assert_eq!(sources.curve(attr).unwrap().is_some(), attr == 171);
            }
            let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, Some(1));
            if surface {
                assert_eq!(sources.surface(attr).unwrap().is_some(), attr == 171);
            } else {
                assert_eq!(sources.curve(attr).unwrap().is_some(), attr == 171);
            }
            drop(sources);
            require_armed(ctx, operation);
            drop(probe);
        }
    }
}

#[test]
fn array_demands_reuse_positive_and_negative_headers() {
    let bytes = nurbs_surface_carrier(180, 181, 20);
    for attr in [182, 65000] {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &unlimited_work()).unwrap();
        let mut sources = Sources::new(&ctx, &bytes, true).unwrap();
        sources.array(attr).unwrap();
        assert_eq!(sources.arrays.f64s.contains_key(&attr), attr == 182);
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "scan Parasolid spline arrays", Some(1));
        sources.array(attr).unwrap();
        drop(sources);
        require_armed(ctx, "scan Parasolid spline arrays");
        drop(probe);
    }
}

#[test]
fn exact_control_demands_cache_framed_missing_and_nan_ambiguity() {
    for kind in 0..3 {
        let mut bytes = nurbs_surface_carrier(180, 181, 20);
        let attr = if kind == 1 { 65000 } else { 182 };
        if kind == 2 {
            let array = bytes.windows(3).position(|window| window == [0, 0x2d, 0x2b]).unwrap();
            let values = array_body(&bytes, array, 0x2d).unwrap() + crate::layout::bspline_array_header::LEN;
            // The framed control and its compact alias both contain NaN;
            // PartialEq keeps them distinct, so resolution is ambiguous.
            bytes[values..values + 8].copy_from_slice(&f64::NAN.to_be_bytes());
        }
        for operation in ["decode Parasolid compact values", "compare Parasolid exact scalar array candidates"] {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &unlimited_work()).unwrap();
            let mut sources = Sources::new(&ctx, &bytes, true).unwrap();
            sources.array(attr).unwrap();
            sources.prepare_control(attr, 12).unwrap();
            assert_eq!(sources.control(attr, 12).unwrap().is_some(), kind == 0);
            let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            sources.prepare_control(attr, 12).unwrap();
            assert_eq!(sources.control(attr, 12).unwrap().is_some(), kind == 0);
            drop(sources);
            require_armed(ctx, operation);
            drop(probe);
        }
    }
}

#[test]
fn knot_pair_demands_cache_positive_missing_and_nan_ambiguity() {
    for kind in 0..3 {
        let mut bytes = nurbs_surface_carrier(180, 181, 20);
        let (knot_attr, mult_attr) = if kind == 1 { (65000, 65001) } else { (185, 183) };
        if kind == 2 {
            let original = f64_array(0x80, 185, &[0.0, 1.0]);
            let at = bytes.windows(original.len()).position(|window| window == original).unwrap();
            bytes.splice(at..at + original.len(), f64_array(0x80, 185, &[f64::NAN, 1.0]));
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &unlimited_work()).unwrap();
        let mut sources = Sources::new(&ctx, &bytes, true).unwrap();
        for attr in [knot_attr, mult_attr] {
            sources.array(attr).unwrap();
        }
        sources.prepare_knots(knot_attr, mult_attr, 2).unwrap();
        assert_eq!(sources.knots(knot_attr, mult_attr, 2).unwrap().is_some(), kind == 0);
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "collect Parasolid knot candidates", None);
        sources.prepare_knots(knot_attr, mult_attr, 2).unwrap();
        assert_eq!(sources.knots(knot_attr, mult_attr, 2).unwrap().is_some(), kind == 0);
        drop(sources);
        require_armed(ctx, "collect Parasolid knot candidates");
        drop(probe);
    }
}

#[test]
fn demanded_scans_refuse_the_next_offset_without_suffix_precharge() {
    let bytes = [0xff; 64];
    for kind in 0..3 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut sources = Sources::new(&ctx, &bytes, true).unwrap();
        let (result, operation) = match kind {
            0 => (sources.curve(500).map(|_| ()), "scan Parasolid curve descriptors"),
            1 => (sources.surface(500).map(|_| ()), "scan Parasolid surface descriptors"),
            _ => (sources.array(500), "scan Parasolid spline arrays"),
        };
        let Err(CodecError::ResourceLimit(limit)) = result else { panic!("second offset refusal"); };
        assert_eq!(limit.operation, operation);
        assert_eq!((limit.used, limit.additional), (1, 1));
        assert_eq!(ctx.resource_refusal(), Some(limit));
        drop(sources);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    }
}

#[test]
fn compact_control_and_knot_cache_release_all_source_backing() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    {
        let mut bytes = vec![0, 3];
        bytes.extend_from_slice(&500u16.to_be_bytes());
        for value in [1.0f64, 2.0, 3.0] { bytes.extend_from_slice(&value.to_be_bytes()); }
        bytes.extend(nurbs_surface_carrier(180, 181, 20));
        let mut sources = Sources::new(&ctx, &bytes, true).unwrap();
        sources.array(500).unwrap();
        sources.prepare_control(500, 3).unwrap();
        assert_eq!(sources.control(500, 3).unwrap(), Some([1.0, 2.0, 3.0].as_slice()));
        assert!(matches!(sources.controls.get(&(500, 3)), Some(super::ControlValues::Compact { .. })));
        for attr in [185, 183] { sources.array(attr).unwrap(); }
        sources.prepare_knots(185, 183, 2).unwrap();
        let knots = sources.knots(185, 183, 2).unwrap().unwrap();
        assert_eq!(knots.unique, [0.0, 1.0]);
        assert_eq!(knots.multiplicities, [2, 2]);
    }
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test released demanded source backing").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn successful_wrapper_excludes_later_descriptor_and_reference_storage() {
    for surface in [false, true] {
        let first = if surface { nurbs_surface_carrier(180, 181, 20) } else { nurbs_curve_carrier(180, 181) };
        let second_wrapper = first.len();
        let mut duplicate = first;
        duplicate.extend(if surface { nurbs_surface_carrier(180, 401, 30) } else { nurbs_curve_carrier(180, 401) });
        let mut baseline = duplicate.clone();
        // Keep the descriptor, arrays and complete source extent identical.
        // Extra bytes can complete a compact alias that the shorter source
        // could not bound, so compare only the second wrapper's eligibility.
        // Its compact-header alias names180, outside the first descriptor's
        // demanded array attrs182..=186, on both sides of this comparison.
        baseline[second_wrapper + 1] = 0xff;
        let boundary = |bytes: &[u8]| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = u64::MAX;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut losses = Vec::new();
            let carriers = if surface {
                scan_surface_carriers(&ctx, bytes, &mut losses).unwrap().len()
            } else {
                scan_curve_carriers(&ctx, bytes, &mut losses).unwrap().len()
            };
            assert_eq!(carriers, 1);
            assert!(losses.is_empty());
            let operation = "test completed successful-wrapper collection usage";
            let probe = RefusalProbe::arm(ResourceDimension::CollectionItems, operation, Some(1));
            let Err(CodecError::ResourceLimit(limit)) = ctx.charge_collection_items(1, operation) else {
                panic!("completed scan collection boundary");
            };
            assert_eq!(limit.operation, operation);
            assert_eq!(limit.additional, 1);
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
            drop(probe);
            limit.used
        };
        // Inspect the completed scan, after the later wrapper. An eager
        // duplicate descriptor, reference or occurrence slot changes usage.
        assert_eq!(boundary(&duplicate), boundary(&baseline), "surface={surface}");
    }
}

#[test]
fn invalid_wrappers_retry_and_losses_complete_in_occurrence_order() {
    let mut bytes = Vec::new();
    for (attr, descriptor) in [(170, 171), (180, 181), (170, 401)] {
        let mut carrier = nurbs_curve_carrier(attr, descriptor);
        if descriptor != 401 {
            let control = carrier.windows(3).position(|window| window == [0, 0x2d, 0x2b]).unwrap();
            let values = array_body(&carrier, control, 0x2d).unwrap() + crate::layout::bspline_array_header::LEN;
            // Finite native coordinates overflow on millimetre conversion.
            carrier[values + 6 * 8..values + 7 * 8].copy_from_slice(&f64::MAX.to_be_bytes());
        }
        bytes.extend(carrier);
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut losses = Vec::new();
    let carriers = scan_curve_carriers(&ctx, &bytes, &mut losses).unwrap();
    assert_eq!(carriers.len(), 1);
    let Some(SolvedCurveGeometry::Nurbs(curve)) = carriers.get(&170).unwrap().geometry.solved() else { panic!("successful retry"); };
    assert_eq!(curve.knots().as_slice(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(losses.len(), 2);
    for (loss, attr) in losses.iter().zip([170, 180]) {
        assert_eq!(loss.code.local_code(), "geometry.spline-lanes-unpaired");
        assert_eq!(loss.message, format!("curve carrier attribute {attr}: control_points contains a non-finite point; the carrier is not emitted and the entities that reference it fall back to an untyped support."));
    }
    ctx.finish_session().unwrap();
}

#[test]
fn invalid_surface_wrappers_retry_with_ordered_losses() {
    let mut bytes = Vec::new();
    for (attr, descriptor) in [(170, 171), (180, 181), (170, 401)] {
        let mut carrier = nurbs_surface_carrier(attr, descriptor, 20);
        if descriptor != 401 {
            let control = carrier.windows(3).position(|window| window == [0, 0x2d, 0x2b]).unwrap();
            let values = array_body(&carrier, control, 0x2d).unwrap() + crate::layout::bspline_array_header::LEN;
            carrier[values + 9 * 8..values + 10 * 8].copy_from_slice(&f64::MAX.to_be_bytes());
        }
        bytes.extend(carrier);
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut losses = Vec::new();
    let carriers = scan_surface_carriers(&ctx, &bytes, &mut losses).unwrap();
    assert_eq!(carriers.len(), 1);
    let Some(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(surface)) = carriers.get(&170).unwrap().geometry.solved() else {
        panic!("successful surface retry");
    };
    assert_eq!((surface.u_count(), surface.v_count()), (2, 2));
    assert_eq!(surface.u_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(losses.len(), 2);
    for (loss, attr) in losses.iter().zip([170, 180]) {
        assert_eq!(loss.code.local_code(), "geometry.spline-lanes-unpaired");
        assert_eq!(loss.message, format!("surface carrier attribute {attr}: control_points contains a non-finite point; the carrier is not emitted and the entities that reference it fall back to an untyped support."));
    }
    ctx.finish_session().unwrap();
}

#[test]
fn descriptor_discovery_stops_at_first_valid_match() {
    for surface in [false, true] {
        let mut bytes = if surface { nurbs_surface_carrier(180, 181, 20) } else { nurbs_curve_carrier(180, 181) };
        let tag = if surface { 0x7e } else { 0x88 };
        let at = bytes.windows(2).position(|window| window == [0, tag]).unwrap();
        bytes.extend_from_slice(&[0xff; 4096]);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &unlimited_work()).unwrap();
        let mut sources = Sources::new(&ctx, &bytes, surface).unwrap();
        let operation = if surface { "collect Parasolid surface descriptors" } else { "collect Parasolid curve descriptors" };
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let error = if surface { sources.surface(181).map(|_| ()) } else { sources.curve(181).map(|_| ()) }.unwrap_err();
        let CodecError::ResourceLimit(limit) = error else { panic!("cache insertion refusal"); };
        // An empty cache lookup costs0 comparisons. Discovery visits exactly
        // offsets0..=at before its first cache node insertion, not the suffix.
        assert_eq!(limit.used, u64::try_from(at + 1).unwrap());
        assert_eq!(limit.operation, operation);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        drop(probe);
    }
}

#[test]
fn distinct_negative_descriptor_demands_pay_each_complete_source_scan() {
    use super::super::CurveDescriptor;

    let bytes = [0xff; 64];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &unlimited_work()).unwrap();
    let mut sources = Sources::new(&ctx, &bytes, false).unwrap();
    assert!(sources.curve(500).unwrap().is_none());
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "collect Parasolid curve descriptors", None);
    let Err(CodecError::ResourceLimit(limit)) = sources.curve(501) else { panic!("second cache insertion refusal"); };
    // Each absent curve demand visits len-29 offsets. Core's first tree
    // insertion pays3 node passes; the next demand's existing-key lookup
    // reads one u16. Refusal is the next insertion's u16 absence lookup,
    // after the second complete scan, before node growth or insertion.
    let alignment = std::mem::align_of::<u16>()
        .max(std::mem::align_of::<Option<CurveDescriptor>>())
        .max(std::mem::align_of::<usize>());
    let node = 11 * (std::mem::size_of::<u16>() + std::mem::size_of::<Option<CurveDescriptor>>())
        + 16 * std::mem::size_of::<usize>() + 2 * alignment;
    let visits = bytes.len() - 29;
    let work = 2 * visits + 3 * node + std::mem::size_of::<u16>();
    assert_eq!(limit.used, u64::try_from(work).unwrap());
    assert_eq!(limit.additional, u64::try_from(std::mem::size_of::<u16>()).unwrap());
    assert_eq!(limit.operation, "collect Parasolid curve descriptors");
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(sources);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    drop(probe);
}
