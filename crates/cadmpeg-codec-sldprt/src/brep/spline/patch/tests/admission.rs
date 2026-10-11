// SPDX-License-Identifier: Apache-2.0
//! Patch scratch lifetimes, progressive matching and descriptor gates.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};

use super::super::super::{scan_curve_carriers, scan_surface_carriers, Arrays};
use super::super::{
    array_span, array_spans, patch_f64_array, patch_nurbs_curve, patch_nurbs_surface,
    unique_control_span, unique_surface_knot_span,
};
use crate::test_support::parasolid::{
    compact_counted_nurbs_surface_carrier, compact_f64_array, f64_array, linear_nurbs_curve_carrier,
    nurbs_curve_carrier, nurbs_surface_carrier, rational_nurbs_surface_carrier, u16_array,
};

fn probe_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy
}

#[test]
fn in_place_nurbs_patches_release_all_scratch_without_retained_storage() {
    let baseline = cadmpeg_test_support::service_decode_context();
    let curve_bytes = nurbs_curve_carrier(10, 11);
    let surface_bytes = compact_counted_nurbs_surface_carrier(10, 11, 20);
    let curves = scan_curve_carriers(&baseline, &curve_bytes, &mut Vec::new()).unwrap();
    let surfaces = scan_surface_carriers(&baseline, &surface_bytes, &mut Vec::new()).unwrap();
    let Some(SolvedCurveGeometry::Nurbs(curve)) = curves[&10].geometry.solved() else {
        panic!("curve");
    };
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) = surfaces[&10].geometry.solved() else {
        panic!("surface");
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..3 {
        let mut bytes = curve_bytes.clone();
        assert_eq!(
            patch_nurbs_curve(&ctx, &mut bytes, 0, curve, curve, 0.001).unwrap(),
            Some(())
        );
        assert_eq!(bytes, curve_bytes);
        let mut bytes = surface_bytes.clone();
        assert_eq!(
            patch_nurbs_surface(&ctx, &mut bytes, 0, surface, surface, 0.001).unwrap(),
            Some(())
        );
        assert_eq!(bytes, surface_bytes);
        drop(
            ctx.reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test patch scratch release",
            )
            .unwrap(),
        );
    }
    ctx.finish_session().unwrap();
}

#[test]
fn patch_shape_rejection_propagates_the_original_refusal() {
    let baseline = cadmpeg_test_support::service_decode_context();
    let curve_bytes = nurbs_curve_carrier(10, 11);
    let linear_bytes = linear_nurbs_curve_carrier(10, 11);
    let curves = scan_curve_carriers(&baseline, &curve_bytes, &mut Vec::new()).unwrap();
    let linear = scan_curve_carriers(&baseline, &linear_bytes, &mut Vec::new()).unwrap();
    let Some(SolvedCurveGeometry::Nurbs(old)) = curves[&10].geometry.solved() else {
        panic!("curve")
    };
    let Some(SolvedCurveGeometry::Nurbs(new)) = linear[&10].geometry.solved() else {
        panic!("curve")
    };
    let surface_bytes = compact_counted_nurbs_surface_carrier(10, 11, 20);
    let rational_bytes = rational_nurbs_surface_carrier(10, 11, 20);
    let surfaces = scan_surface_carriers(&baseline, &surface_bytes, &mut Vec::new()).unwrap();
    let rational = scan_surface_carriers(&baseline, &rational_bytes, &mut Vec::new()).unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(old_surface)) = surfaces[&10].geometry.solved() else {
        panic!("surface")
    };
    let Some(SolvedSurfaceGeometry::Nurbs(new_surface)) = rational[&10].geometry.solved() else {
        panic!("surface")
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        patch_nurbs_curve(&ctx, &mut [], 0, old, new, 1.0).unwrap(),
        None
    );
    assert_eq!(
        patch_nurbs_surface(&ctx, &mut [], 0, old_surface, new_surface, 1.0).unwrap(),
        None
    );
    let CodecError::ResourceLimit(first) = ctx
        .charge_work(1, "test original patch refusal")
        .unwrap_err()
    else {
        panic!("refusal");
    };
    for result in [
        patch_nurbs_curve(&ctx, &mut [], 0, old, new, 1.0),
        patch_nurbs_surface(&ctx, &mut [], 0, old_surface, new_surface, 1.0),
    ] {
        assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == first));
    }
    assert!(
        matches!(unique_surface_knot_span(&ctx, &[], &Arrays::default(), (2, 3), 2, &[0.0], &[1]),
        Err(CodecError::ResourceLimit(actual)) if actual == first)
    );
}

#[test]
fn patch_array_search_pays_only_for_the_first_hit_and_write() {
    let mut bytes = f64_array(0x80, 40, &[0.0, 1.0]);
    bytes.extend_from_slice(&[0xff; 4096]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One start visit and two eight-byte writes.
    policy.limits.max_work_units = 17;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        patch_f64_array(&ctx, &mut bytes, 0x80, 40, &[2.0, 3.0]).unwrap(),
        Some(())
    );
    ctx.finish_session().unwrap();
    crate::test_support::work_refusal_at("find Parasolid patch scalar array", |ctx| {
        array_span(ctx, &bytes, 0x80, 40)
    });
}

#[test]
fn patch_span_scan_of_a_short_buffer_does_no_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(array_spans(&ctx, &[0; 9], &Arrays::default(), 0x80, 40)
        .unwrap()
        .is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn patch_control_matching_stops_at_each_first_mismatch() {
    let values = vec![1.0; 64];
    let mut bytes = f64_array(0x2d, 40, &values);
    bytes.extend(f64_array(0x2d, 40, &values));
    let operation = "test completed patch control matching";
    let boundary = |old: &[f64]| {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = probe_policy();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                assert_eq!(unique_control_span(&ctx, &bytes, &Arrays::default(), 40, old)?, None);
                ctx.charge_work(1, operation)
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("completed matching boundary");
        };
        limit.used
    };
    let cap = boundary(&[0.0; 64]);
    let mut second_mismatch = [1.0; 64];
    second_mismatch[1] = 0.0;
    // Each of the two spans visits one extra scalar before its mismatch.
    assert_eq!(boundary(&second_mismatch), cap + 2);
    let arena = DecodeArena::new();
    let mut policy = probe_policy();
    policy.limits.max_work_units = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        unique_control_span(&ctx, &bytes, &Arrays::default(), 40, &[0.0; 64]).unwrap(),
        None
    );
    ctx.finish_session().unwrap();
    crate::test_support::work_refusal_at("match Parasolid patch control span", |ctx| {
        unique_control_span(ctx, &bytes, &Arrays::default(), 40, &[0.0; 64])
    });
}

#[test]
fn patch_knot_matching_gates_counts_and_skips_the_second_lane_after_mismatch() {
    for short in [true, false] {
        let mut bytes = f64_array(0x80, 40, &[0.0, 1.0]);
        bytes.extend(u16_array(41, &[2, 2]));
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &probe_policy()).unwrap();
        let operation = if short {
            "match Parasolid patch knot spans"
        } else {
            "match Parasolid patch multiplicity values"
        };
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let knots: &[f64] = if short { &[0.0, 1.0, 2.0] } else { &[2.0, 3.0] };
        let mult: &[usize] = if short { &[2, 2, 2] } else { &[2, 2] };
        assert_eq!(
            unique_surface_knot_span(
                &ctx,
                &bytes,
                &Arrays::default(),
                (40, 41),
                knots.len(),
                knots,
                mult
            )
            .unwrap(),
            None
        );
        assert!(matches!(
            ctx.charge_work(1, operation),
            Err(CodecError::ResourceLimit(_))
        ));
        drop(probe);
    }
}

#[test]
fn patch_map_lookups_admit_compact_arrays_and_surface_descriptors() {
    let mut arrays = Arrays::default();
    arrays.compact.insert(40, Vec::new());
    crate::test_support::work_refusal_at("find Parasolid patch compact arrays", |ctx| {
        array_spans(ctx, &[], &arrays, 0x80, 40)
    });
    let bytes = compact_counted_nurbs_surface_carrier(10, 11, 20);
    let baseline = cadmpeg_test_support::service_decode_context();
    let carriers = scan_surface_carriers(&baseline, &bytes, &mut Vec::new()).unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) = carriers[&10].geometry.solved() else {
        panic!("surface")
    };
    crate::test_support::work_refusal_at("find Parasolid patch surface descriptor", |ctx| {
        patch_nurbs_surface(ctx, &mut bytes.clone(), 0, surface, surface, 0.001)
    });
    for operation in [
        "compare Parasolid patch U multiplicities",
        "compare Parasolid patch V multiplicities",
    ] {
        crate::test_support::work_refusal_at(operation, |ctx| {
            patch_nurbs_surface(ctx, &mut bytes.clone(), 0, surface, surface, 0.001)
        });
    }
}

#[test]
fn patch_curve_multiplicity_comparison_admits_work() {
    let bytes = nurbs_curve_carrier(10, 11);
    let baseline = cadmpeg_test_support::service_decode_context();
    let carriers = scan_curve_carriers(&baseline, &bytes, &mut Vec::new()).unwrap();
    let Some(SolvedCurveGeometry::Nurbs(curve)) = carriers[&10].geometry.solved() else {
        panic!("curve")
    };
    crate::test_support::work_refusal_at("compare Parasolid patch curve multiplicities", |ctx| {
        patch_nurbs_curve(ctx, &mut bytes.clone(), 0, curve, curve, 0.001)
    });
}

#[test]
fn missing_or_incompatible_patch_surface_descriptor_skips_array_discovery() {
    let original = compact_counted_nurbs_surface_carrier(10, 11, 20);
    let baseline = cadmpeg_test_support::service_decode_context();
    let carriers = scan_surface_carriers(&baseline, &original, &mut Vec::new()).unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) = carriers[&10].geometry.solved() else {
        panic!("surface")
    };
    for missing in [false, true] {
        let mut bytes = original.clone();
        if missing {
            let mut p = 2;
            if bytes[p] == 0xff {
                p += 1;
            }
            bytes[p + 17..p + 19].copy_from_slice(&65000_u16.to_be_bytes());
        } else {
            let mut body = bytes
                .windows(2)
                .position(|window| window == [0, 0x7e])
                .unwrap()
                + 2;
            if bytes[body] == 0xff {
                body += 1;
            }
            let count = body + crate::layout::bspline_surface_descriptor::U_POLE_COUNT;
            bytes[count..count + 4].copy_from_slice(&3_u32.to_be_bytes());
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &probe_policy()).unwrap();
        let operation = "scan Parasolid spline arrays";
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        assert_eq!(
            patch_nurbs_surface(&ctx, &mut bytes, 0, surface, surface, 0.001).unwrap(),
            None
        );
        assert!(matches!(
            ctx.charge_work(1, operation),
            Err(CodecError::ResourceLimit(_))
        ));
        drop(probe);
    }
}

#[test]
fn truncated_patch_surface_wrapper_skips_descriptor_index() {
    let mut bytes = compact_counted_nurbs_surface_carrier(10, 11, 20);
    let baseline = cadmpeg_test_support::service_decode_context();
    let carriers = scan_surface_carriers(&baseline, &bytes, &mut Vec::new()).unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) = carriers[&10].geometry.solved() else {
        panic!("surface")
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &probe_policy()).unwrap();
    let operation = "scan Parasolid surface descriptors";
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    let offset = bytes.len();
    assert_eq!(
        patch_nurbs_surface(&ctx, &mut bytes, offset, surface, surface, 0.001).unwrap(),
        None
    );
    assert!(matches!(
        ctx.charge_work(1, operation),
        Err(CodecError::ResourceLimit(_))
    ));
    drop(probe);
}


#[test]
fn patch_knots_without_paired_multiplicity_counts_skip_scalar_payloads() {
    for multiplicities in [None, Some([2, 2, 0])] {
        let mut bytes = f64_array(0x80, 40, &vec![0.0; 100_000]);
        if let Some(values) = multiplicities {
            bytes.extend(u16_array(41, &values));
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &probe_policy()).unwrap();
        let operation = "read Parasolid patch scalar values";
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        assert_eq!(unique_surface_knot_span(
            &ctx, &bytes, &Arrays::default(), (40, 41), 2, &[0.0, 1.0], &[2, 2]
        ).unwrap(), None);
        assert!(matches!(ctx.charge_work(1, operation), Err(CodecError::ResourceLimit(_))));
        drop(probe);
    }
}

#[test]
fn patch_knot_ambiguity_skips_payloads_after_the_second_distinct_pair() {
    use crate::brep::spline::CompactArray;

    let mut bytes = Vec::new();
    let mut headers = Vec::new();
    for _ in 0..128 {
        let offset = bytes.len();
        bytes.extend(compact_f64_array(40, &[0.0, 1.0]));
        headers.push(CompactArray { offset, count: 2 });
    }
    bytes.extend(u16_array(41, &[2, 2]));
    let arrays = |count| {
        let mut arrays = Arrays::default();
        arrays.compact.insert(40, headers[..count].to_vec());
        arrays
    };
    let boundary = |count, discovery_only| {
        let operation = "test completed patch knot prefix";
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits, operation, |cap| {
                let arena = DecodeArena::new();
                let mut policy = probe_policy();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let arrays = arrays(count);
                if discovery_only {
                    array_spans(&ctx, &bytes, &arrays, 0x80, 40)?;
                    array_spans(&ctx, &bytes, &arrays, 0x7f, 41)?;
                } else {
                    assert_eq!(unique_surface_knot_span(
                        &ctx, &bytes, &arrays, (40, 41), 2, &[0.0, 1.0], &[2, 2]
                    )?, None);
                }
                ctx.charge_work(1, operation)
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("completed patch prefix boundary");
        };
        limit.used
    };
    // Span discovery visits the complete roster. Only that necessary work
    // differs when the source gains a suffix after the ambiguous pair.
    let cap = boundary(2, false) + boundary(128, true) - boundary(2, true);
    let arena = DecodeArena::new();
    let mut policy = probe_policy();
    policy.limits.max_work_units = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(unique_surface_knot_span(
        &ctx, &bytes, &arrays(128), (40, 41), 2, &[0.0, 1.0], &[2, 2]
    ).unwrap(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn two_multiplicity_locations_make_one_patch_knot_span_ambiguous() {
    let mut bytes = f64_array(0x80, 40, &[0.0, 1.0]);
    bytes.extend(u16_array(41, &[2, 2]));
    bytes.extend(u16_array(41, &[2, 2]));
    assert_eq!(unique_surface_knot_span(
        &cadmpeg_test_support::service_decode_context(), &bytes, &Arrays::default(),
        (40, 41), 2, &[0.0, 1.0], &[2, 2]
    ).unwrap(), None);
}

#[test]
fn surface_patch_skips_framed_index_storage() {
    let bytes = nurbs_surface_carrier(10, 11, 20);
    let baseline = cadmpeg_test_support::service_decode_context();
    let carriers = scan_surface_carriers(&baseline, &bytes, &mut Vec::new()).unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) = carriers[&10].geometry.solved() else {
        panic!("surface");
    };
    for operation in ["collect Parasolid scalar arrays", "collect Parasolid integer arrays"] {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &probe_policy()).unwrap();
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let mut patched = bytes.clone();
        assert_eq!(patch_nurbs_surface(&ctx, &mut patched, 0, surface, surface, 0.001).unwrap(), Some(()));
        assert_eq!(patched, bytes);
        assert!(matches!(ctx.charge_work(1, operation), Err(CodecError::ResourceLimit(_))));
        drop(probe);
    }
}

#[test]
fn rejected_surface_spans_skip_replacement_pole_construction() {
    let original = nurbs_surface_carrier(10, 11, 20);
    let baseline = cadmpeg_test_support::service_decode_context();
    let carriers = scan_surface_carriers(&baseline, &original, &mut Vec::new()).unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) = carriers[&10].geometry.solved() else {
        panic!("surface");
    };
    let mut replacement = surface.clone();
    replacement.try_map_control_points(
        |_, _| Ok::<_, std::convert::Infallible>(cadmpeg_ir::features::FinitePoint3::new(
            cadmpeg_ir::math::Point3::new(f64::MAX, 0.0, 0.0)
        ).unwrap()), &baseline
    ).unwrap().unwrap();
    for reference in [0, 3, 4] {
        let mut bytes = original.clone();
        let body = bytes.windows(2).position(|w| w == [0, 0x7e]).unwrap() + 2;
        let at = body + crate::layout::bspline_surface_descriptor::ARRAY_REFS + reference * 2;
        bytes[at..at + 2].copy_from_slice(&65000_u16.to_be_bytes());
        let operation = "test completed rejected surface patch";
        let boundary = |new| {
            let error = cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits, operation, |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = probe_policy();
                    policy.limits.max_work_units = cap;
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    let mut patched = bytes.clone();
                    assert_eq!(patch_nurbs_surface(&ctx, &mut patched, 0, surface, new, 2.0)?, None);
                    assert_eq!(patched, bytes);
                    ctx.charge_work(1, operation)
                },
            );
            let CodecError::ResourceLimit(limit) = error else {
                panic!("completed rejection boundary");
            };
            limit.used
        };
        let cap = boundary(surface);
        // An overflowing replacement would stop serialization at its first
        // pole. Neither replacement grid is read after native span rejection.
        assert_eq!(boundary(&replacement), cap);
        let arena = DecodeArena::new();
        let mut policy = probe_policy();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(patch_nurbs_surface(&ctx, &mut bytes.clone(), 0, surface, surface, 2.0).unwrap(), None);
        ctx.finish_session().unwrap();
    }
    let mut bytes = original.clone();
    assert_eq!(patch_nurbs_surface(&baseline, &mut bytes, 0, surface, &replacement, 2.0).unwrap(), None);
    assert_eq!(bytes, original);
}
