// SPDX-License-Identifier: Apache-2.0
//! Dependency gates and progressive scalar and knot ambiguity resolution.

use super::{Arrays, CompactArray, FramedArray};
use crate::test_support::parasolid::{
    compact_f64_array, f64_array, nurbs_curve_carrier, nurbs_surface_carrier, u16_array,
};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::cell::OnceCell;

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy
}

fn require_armed(ctx: &DecodeContext<'_>, operation: &'static str) {
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, operation) else {
        panic!("unused operation probe remains armed");
    };
    assert_eq!(limit.operation, operation);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn spline_control_header_rejection_skips_later_dependencies() {
    for surface in [false, true] {
        for missing in [false, true] {
            let mut bytes = if surface {
                nurbs_surface_carrier(170, 171, 20)
            } else {
                nurbs_curve_carrier(170, 171)
            };
            let mut at = bytes
                .windows(2)
                .position(|window| window == if surface { [0, 0x7e] } else { [0, 0x88] })
                .unwrap()
                + 2;
            if bytes[at] == 0xff {
                at += 1;
            }
            if missing {
                let reference = if surface {
                    crate::layout::bspline_surface_descriptor::ARRAY_REFS
                } else {
                    19
                };
                bytes[at + reference..at + reference + 2].copy_from_slice(&65000_u16.to_be_bytes());
            } else {
                let count = if surface {
                    crate::layout::bspline_surface_descriptor::U_POLE_COUNT
                } else {
                    4
                };
                bytes[at + count..at + count + 4].copy_from_slice(&4_u32.to_be_bytes());
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
            let operation = "collect Parasolid integer arrays";
            let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            let mut losses = Vec::new();
            let empty = if surface {
                super::scan_surface_carriers(&ctx, &bytes, &mut losses)
                    .unwrap()
                    .is_empty()
            } else {
                super::scan_curve_carriers(&ctx, &bytes, &mut losses)
                    .unwrap()
                    .is_empty()
            };
            assert!(empty);
            assert!(losses.is_empty());
            require_armed(&ctx, operation);
            drop(probe);
        }
    }
}

#[test]
fn rejected_surface_u_knots_skip_v_array_discovery() {
    let mut bytes = nurbs_surface_carrier(170, 171, 20);
    let mut body = bytes
        .windows(2)
        .position(|window| window == [0, 0x7e])
        .unwrap()
        + 2;
    if bytes[body] == 0xff {
        body += 1;
    }
    let reference = body + crate::layout::bspline_surface_descriptor::ARRAY_REFS + 2;
    bytes[reference..reference + 2].copy_from_slice(&65000_u16.to_be_bytes());
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let operation = "collect Parasolid integer arrays";
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    let mut losses = Vec::new();
    assert!(super::scan_surface_carriers(&ctx, &bytes, &mut losses)
        .unwrap()
        .is_empty());
    assert!(losses.is_empty());
    require_armed(&ctx, operation);
    drop(probe);
}

fn framed<'ctx, T>(bytes: &[u8], offset: usize, operation: &'static str) -> FramedArray<'ctx, T> {
    let (_, _, span) = super::framed_array_span(bytes, offset).unwrap();
    FramedArray {
        span,
        operation,
        values: OnceCell::new(),
    }
}

#[test]
fn missing_or_disjoint_knot_headers_skip_both_payload_lanes() {
    for missing in [false, true] {
        for operation in [
            "decode Parasolid scalar array values",
            "decode Parasolid integer array values",
        ] {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
            let mut bytes = u16_array(41, &[2; 4096]);
            let knot_at = bytes.len();
            bytes.extend(f64_array(0x80, 40, &[0.0, 1.0, 2.0]));
            let mut arrays = Arrays::default();
            arrays.u16s.insert(
                41,
                framed(&bytes, 0, "decode Parasolid integer array values"),
            );
            if !missing {
                arrays.f64s.insert(
                    40,
                    framed(&bytes, knot_at, "decode Parasolid scalar array values"),
                );
            }
            let mut output = ctx.reserve_scoped(0, "test knot output").unwrap();
            let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            assert!(
                super::surface_knot_values(&ctx, &mut output, &bytes, &arrays, (40, 41, 2))
                    .unwrap()
                    .is_none()
            );
            assert!(arrays.u16s[&41].values.get().is_none());
            if !missing {
                assert!(arrays.f64s[&40].values.get().is_none());
            }
            require_armed(&ctx, operation);
            drop(probe);
        }
    }
}

fn control_resolution(ctx: &DecodeContext<'_>, suffix: usize) -> Result<bool, CodecError> {
    let mut bytes = f64_array(0x2d, 40, &[0.0; 12]);
    let mut arrays = Arrays::default();
    arrays.f64s.insert(
        40,
        framed(&bytes, 0, "decode Parasolid scalar array values"),
    );
    let mut compact = Vec::new();
    for _ in 0..suffix {
        let offset = bytes.len();
        bytes.extend(compact_f64_array(40, &[1.0; 12]));
        compact.push(CompactArray { offset, count: 12 });
    }
    arrays.compact.insert(40, compact);
    Ok(super::exact_f64_array(ctx, &bytes, &arrays, 40, 12)?.is_some())
}

#[test]
fn exact_control_ambiguity_skips_a_long_candidate_suffix() {
    // Equality admits each operand separately. Locate the complete first
    // conflict prefix, including both operands, before adding the suffix.
    let operation = "test completed control ambiguity prefix";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = control_resolution(&ctx, 1)?;
            ctx.charge_work(1, operation)?;
            Ok(result)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("comparison boundary");
    };
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = limit.used;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(!control_resolution(&ctx, 128).unwrap());
    ctx.finish_session().unwrap();
}

fn knot_resolution(ctx: &DecodeContext<'_>, suffix: usize) -> Result<bool, CodecError> {
    let mut bytes = f64_array(0x80, 40, &[0.0, 1.0]);
    let mut arrays = Arrays::default();
    arrays.f64s.insert(
        40,
        framed(&bytes, 0, "decode Parasolid scalar array values"),
    );
    let at = bytes.len();
    bytes.extend(u16_array(41, &[2, 2]));
    arrays.u16s.insert(
        41,
        framed(&bytes, at, "decode Parasolid integer array values"),
    );
    let mut compact = Vec::new();
    for _ in 0..suffix {
        let offset = bytes.len();
        bytes.extend(compact_f64_array(40, &[1.0, 2.0]));
        compact.push(CompactArray { offset, count: 2 });
    }
    arrays.compact.insert(40, compact);
    let mut output = ctx.reserve_scoped(0, "test knot output").unwrap();
    Ok(super::surface_knot_values(ctx, &mut output, &bytes, &arrays, (40, 41, 2))?.is_some())
}

#[test]
fn surface_knot_ambiguity_skips_a_long_payload_suffix() {
    let operation = "test completed knot ambiguity prefix";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(!knot_resolution(&ctx, 1)?);
            ctx.charge_work(1, operation)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("completed conflicting knot prefix");
    };
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = limit.used;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(!knot_resolution(&ctx, 128).unwrap());
    ctx.finish_session().unwrap();
    crate::test_support::work_refusal_at("scan Parasolid compact attribute arrays", |ctx| {
        knot_resolution(ctx, 128)
    });
}

#[test]
fn absent_multiplicity_headers_skip_knot_count_index_storage() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let bytes = f64_array(0x80, 40, &vec![0.0; 4096]);
    let mut arrays = Arrays::default();
    arrays.f64s.insert(
        40,
        framed(&bytes, 0, "decode Parasolid scalar array values"),
    );
    let mut output = ctx.reserve_scoped(0, "test knot output").unwrap();
    assert!(
        super::surface_knot_values(&ctx, &mut output, &bytes, &arrays, (40, 41, 2))
            .unwrap()
            .is_none()
    );
    assert!(arrays.f64s[&40].values.get().is_none());
    drop(arrays);
    drop(output);
    ctx.finish_session().unwrap();
}

#[test]
fn invalid_surface_multiplicities_skip_output_copy() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let mut bytes = f64_array(0x80, 40, &[0.0, 1.0]);
    let mut arrays = Arrays::default();
    arrays.f64s.insert(
        40,
        framed(&bytes, 0, "decode Parasolid scalar array values"),
    );
    let at = bytes.len();
    bytes.extend(u16_array(41, &[0, 2]));
    arrays.u16s.insert(
        41,
        framed(&bytes, at, "decode Parasolid integer array values"),
    );
    let mut output = ctx.reserve_scoped(0, "test knot output").unwrap();
    let operation = "copy Parasolid knot multiplicities";
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    assert!(
        super::surface_knot_values(&ctx, &mut output, &bytes, &arrays, (40, 41, 2))
            .unwrap()
            .is_none()
    );
    require_armed(&ctx, operation);
    drop(probe);
}

#[test]
fn surface_knot_ambiguity_skips_both_output_copies() {
    for operation in [
        "copy Parasolid distinct knots",
        "copy Parasolid knot multiplicities",
    ] {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        assert!(!knot_resolution(&ctx, 128).unwrap());
        require_armed(&ctx, operation);
        drop(probe);
    }
}

#[test]
fn invalid_multiplicity_count_skips_later_knot_payloads() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let mut bytes = f64_array(0x80, 40, &[0.0, 1.0]);
    let mut arrays = Arrays::default();
    arrays.f64s.insert(
        40,
        framed(&bytes, 0, "decode Parasolid scalar array values"),
    );
    let at = bytes.len();
    bytes.extend(u16_array(41, &[0, 2]));
    arrays.u16s.insert(
        41,
        framed(&bytes, at, "decode Parasolid integer array values"),
    );
    let mut compact = Vec::new();
    for _ in 0..128 {
        let offset = bytes.len();
        bytes.extend(compact_f64_array(40, &[1.0, 2.0]));
        compact.push(CompactArray { offset, count: 2 });
    }
    arrays.compact.insert(40, compact);
    let mut output = ctx.reserve_scoped(0, "test knot output").unwrap();
    let operation = "decode Parasolid compact values";
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    assert!(
        super::surface_knot_values(&ctx, &mut output, &bytes, &arrays, (40, 41, 2))
            .unwrap()
            .is_none()
    );
    assert!(arrays.f64s[&40].values.get().is_none());
    require_armed(&ctx, operation);
    drop(probe);
}

fn assert_invalid_large_multiplicities_skip_knots(trailing: bool) {
    let mut multiplicities = vec![0; 100_000];
    multiplicities[0] = if trailing { 2 } else { 0 };
    multiplicities[1] = 2;
    if trailing {
        multiplicities[99_999] = 1;
    }
    let mut bytes = u16_array(41, &multiplicities);
    let knot_at = bytes.len();
    bytes.extend(f64_array(0x80, 40, &vec![0.0; 100_000]));
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut arrays = Arrays::default();
    arrays.u16s.insert(
        41,
        framed(&bytes, 0, "decode Parasolid integer array values"),
    );
    arrays.f64s.insert(
        40,
        framed(&bytes, knot_at, "decode Parasolid scalar array values"),
    );
    let mut output = ctx.reserve_scoped(0, "test invalid knot output").unwrap();
    let operation = "decode Parasolid scalar array values";
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    assert!(
        super::surface_knot_values(&ctx, &mut output, &bytes, &arrays, (40, 41, 2))
            .unwrap()
            .is_none()
    );
    assert!(arrays.f64s[&40].values.get().is_none());
    assert!(arrays.u16s[&41].values.get().is_none());
    require_armed(&ctx, operation);
    drop(probe);
}

#[test]
fn invalid_large_multiplicity_lanes_skip_the_first_knot_payload() {
    assert_invalid_large_multiplicities_skip_knots(false);
}

#[test]
fn nonzero_trailing_multiplicities_skip_the_first_knot_payload() {
    assert_invalid_large_multiplicities_skip_knots(true);
}

fn assert_ineligible_multiplicity_source_skips_knots(axis: usize, short: bool) {
    let mut bytes = nurbs_surface_carrier(170, 171, 20);
    let control = bytes.windows(3).position(|w| w == [0, 0x2d, 0x2b]).unwrap();
    bytes.truncate(control);
    let body = bytes.windows(2).position(|w| w == [0, 0x7e]).unwrap() + 2;
    let refs = body + crate::layout::bspline_surface_descriptor::ARRAY_REFS;
    for (index, attr) in [256_u16, 512, 768, 1024, 1280].into_iter().enumerate() {
        bytes[refs + index * 2..refs + index * 2 + 2].copy_from_slice(&attr.to_be_bytes());
    }
    let mult_ref = refs + (axis + 1) * 2;
    bytes[mult_ref..mult_ref + 2].copy_from_slice(&65000_u16.to_be_bytes());
    bytes.extend(f64_array(
        0x2d,
        256,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.5],
    ));
    for attr in [512, 768] {
        bytes.extend(u16_array(attr, &[2, 2]));
    }
    for attr in [1024, 1280] {
        bytes.extend(f64_array(0x80, attr, &[0.0, 1.0]));
    }
    if short {
        bytes.extend(u16_array(65000, &[2]));
    }
    let knot_attr = if axis == 0 { 1024 } else { 1280 };
    for _ in 0..128 {
        bytes.extend(compact_f64_array(knot_attr, &[0.0, 1.0]));
    }
    let mut baseline = bytes.clone();
    let knot_ref = refs + (axis + 3) * 2;
    baseline[knot_ref..knot_ref + 2].copy_from_slice(&65001_u16.to_be_bytes());
    let operation = "test completed ineligible axis discovery";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut losses = Vec::new();
            assert!(super::scan_surface_carriers(&ctx, &baseline, &mut losses)?.is_empty());
            assert!(losses.is_empty());
            ctx.charge_work(1, operation)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("completed multiplicity rejection boundary");
    };
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = limit.used;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut losses = Vec::new();
    assert!(super::scan_surface_carriers(&ctx, &bytes, &mut losses)
        .unwrap()
        .is_empty());
    assert!(losses.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn ineligible_axis_multiplicities_skip_the_corresponding_knot_source() {
    assert_ineligible_multiplicity_source_skips_knots(0, false);
}

#[test]
fn absent_v_multiplicity_headers_skip_v_knot_discovery() {
    assert_ineligible_multiplicity_source_skips_knots(1, false);
}

#[test]
fn undersized_u_multiplicity_headers_skip_u_knot_discovery() {
    assert_ineligible_multiplicity_source_skips_knots(0, true);
}

#[test]
fn undersized_v_multiplicity_headers_skip_v_knot_discovery() {
    assert_ineligible_multiplicity_source_skips_knots(1, true);
}

fn different_count_knot_resolution(
    ctx: &DecodeContext<'_>,
    suffix: usize,
    headers_only: bool,
) -> Result<bool, CodecError> {
    let mut bytes = f64_array(0x80, 40, &[0.0, 1.0]);
    let mut arrays = Arrays::default();
    arrays.f64s.insert(
        40,
        framed(&bytes, 0, "decode Parasolid scalar array values"),
    );
    let at = bytes.len();
    bytes.extend(u16_array(41, &[2, 2]));
    arrays.u16s.insert(
        41,
        framed(&bytes, at, "decode Parasolid integer array values"),
    );
    let offset = bytes.len();
    bytes.extend(compact_f64_array(40, &[1.0, 2.0, f64::NAN]));
    arrays
        .compact
        .insert(40, vec![CompactArray { offset, count: 3 }]);
    let mut headers = Vec::new();
    for _ in 0..suffix {
        let offset = bytes.len();
        bytes.extend([0, 3, 0, 41, 0, 2, 0, 2, 0, 0]);
        headers.push(CompactArray { offset, count: 3 });
    }
    arrays.compact.insert(41, headers);
    if headers_only {
        return super::visit_array_headers(
            ctx,
            &bytes,
            (&arrays.u16s, &arrays.compact, 41),
            |_, _| Ok(true),
        );
    }
    let mut output = ctx.reserve_scoped(0, "test knot output")?;
    Ok(super::surface_knot_values(ctx, &mut output, &bytes, &arrays, (40, 41, 2))?.is_some())
}

#[test]
fn different_count_knot_disagreement_skips_multiplicity_alias_suffix() {
    let operation = "test completed different-count knot disagreement";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            assert!(!different_count_knot_resolution(&ctx, 1, false)?);
            ctx.charge_work(1, operation)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("completed pair disagreement")
    };
    let arena = DecodeArena::new();
    let header_work = |suffix| {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "test completed multiplicity headers",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                assert!(different_count_knot_resolution(&ctx, suffix, true)?);
                ctx.charge_work(1, "test completed multiplicity headers")
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("completed header traversal")
        };
        limit.used
    };
    // The first physical count visits each mismatched compact header once.
    // The later scalar disagreement must skip their payloads and suffix.
    let mut policy = policy();
    policy.limits.max_work_units = limit.used + header_work(128) - header_work(1);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(!different_count_knot_resolution(&ctx, 128, false).unwrap());
    ctx.finish_session().unwrap();
    crate::test_support::work_refusal_at("scan Parasolid compact attribute arrays", |ctx| {
        different_count_knot_resolution(ctx, 128, false)
    });
}

#[test]
fn surface_knot_decode_reads_only_the_declared_scalar_prefix() {
    let mut knots = vec![f64::NAN; 100_000];
    knots[0] = 0.0;
    knots[1] = 1.0;
    let mut multiplicities = vec![0; 100_000];
    multiplicities[0] = 2;
    multiplicities[1] = 2;
    let mut bytes = f64_array(0x80, 40, &knots);
    let multiplicity_at = bytes.len();
    bytes.extend(u16_array(41, &multiplicities));
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 512 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut arrays = Arrays::default();
    arrays.f64s.insert(
        40,
        framed(&bytes, 0, "decode Parasolid scalar array values"),
    );
    arrays.u16s.insert(
        41,
        framed(
            &bytes,
            multiplicity_at,
            "decode Parasolid integer array values",
        ),
    );
    let mut output = ctx.reserve_scoped(0, "test knot output").unwrap();
    let result = super::surface_knot_values(&ctx, &mut output, &bytes, &arrays, (40, 41, 2))
        .unwrap()
        .unwrap();
    assert_eq!(result.unique, [0.0, 1.0]);
    assert_eq!(result.multiplicities, [2, 2]);
    assert!(arrays.f64s[&40].values.get().is_none());
    drop(result);
    drop(output);
    drop(arrays);
    drop(
        ctx.reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test released knot prefix scratch",
        )
        .unwrap(),
    );
    ctx.finish_session().unwrap();
}

fn invalid_multiplicity_prefix_work(
    ctx: &DecodeContext<'_>,
    physical_count: usize,
    trailing: bool,
) -> Result<(), CodecError> {
    let mut multiplicities = vec![0; physical_count];
    multiplicities[1] = 2;
    if trailing {
        multiplicities[0] = 2;
        multiplicities[2] = 1;
    }
    let mut bytes = u16_array(41, &multiplicities);
    let knot_at = bytes.len();
    bytes.extend(f64_array(0x80, 40, &vec![0.0; physical_count]));
    let mut arrays = Arrays::default();
    arrays.u16s.insert(
        41,
        framed(&bytes, 0, "decode Parasolid integer array values"),
    );
    arrays.f64s.insert(
        40,
        framed(&bytes, knot_at, "decode Parasolid scalar array values"),
    );
    let mut output = ctx.reserve_scoped(0, "test knot output")?;
    assert!(super::surface_knot_values(ctx, &mut output, &bytes, &arrays, (40, 41, 2))?.is_none());
    Ok(())
}

#[test]
fn invalid_multiplicities_stop_at_the_first_invalid_cell() {
    for trailing in [false, true] {
        let operation = "test completed invalid multiplicity prefix";
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                invalid_multiplicity_prefix_work(&ctx, 3, trailing)?;
                ctx.charge_work(1, operation)
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("invalid prefix boundary")
        };
        let arena = DecodeArena::new();
        let mut policy = policy();
        policy.limits.max_work_units = limit.used;
        policy.limits.max_materialized_bytes = 4096;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        invalid_multiplicity_prefix_work(&ctx, 100_000, trailing).unwrap();
        ctx.finish_session().unwrap();
        crate::test_support::work_refusal_at("decode Parasolid integer array values", |ctx| {
            invalid_multiplicity_prefix_work(ctx, 100_000, trailing)
        });
    }
}

fn knot_pair<'ctx>(count: usize, compact: bool) -> (Vec<u8>, Arrays<'ctx>) {
    let mut arrays = Arrays::default();
    let knots = vec![0.0; count];
    let multiplicities = vec![2; count];
    let mut bytes = if compact {
        compact_f64_array(40, &knots)
    } else {
        f64_array(0x80, 40, &knots)
    };
    let multiplicity_at = bytes.len();
    if compact {
        bytes.extend([0, u8::try_from(count).unwrap(), 0, 41]);
        bytes.extend(multiplicities.iter().flat_map(|value: &u16| value.to_be_bytes()));
        arrays.compact.insert(40, vec![CompactArray { offset: 0, count }]);
        arrays.compact.insert(41, vec![CompactArray { offset: multiplicity_at, count }]);
    } else {
        bytes.extend(u16_array(41, &multiplicities));
        arrays.f64s.insert(40, framed(&bytes, 0, "decode Parasolid scalar array values"));
        arrays.u16s.insert(41, framed(&bytes, multiplicity_at, "decode Parasolid integer array values"));
    }
    (bytes, arrays)
}

fn scalar_conflict(ctx: &DecodeContext<'_>, mismatch: usize) -> Result<(), CodecError> {
    let (mut bytes, mut arrays) = knot_pair(200, false);
    let mut knots = vec![0.0; 200];
    knots[mismatch] = 1.0;
    let offset = bytes.len();
    bytes.extend(compact_f64_array(40, &knots));
    arrays.compact.insert(40, vec![CompactArray { offset, count: 200 }]);
    let mut output = ctx.reserve_scoped(0, "test scalar conflict output")?;
    assert!(super::surface_knot_values(ctx, &mut output, &bytes, &arrays, (40, 41, 200))?.is_none());
    Ok(())
}

#[test]
fn surface_knot_conflicts_stop_at_the_first_scalar_cell() {
    const PROBE: &str = "test completed scalar cell conflict";
    let boundary = |mismatch| {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits, PROBE, |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                scalar_conflict(&ctx, mismatch)?;
                ctx.charge_work(1, PROBE)
            },
        );
        let CodecError::ResourceLimit(limit) = error else { panic!("scalar conflict completion") };
        limit.used
    };
    let first = boundary(0);
    let last = boundary(199);
    assert!(first < last, "the unread scalar suffix must not be prepaid");
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = first;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    scalar_conflict(&ctx, 0).unwrap();
    ctx.finish_session().unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(scalar_conflict(&ctx, 199), Err(CodecError::ResourceLimit(limit))
        if limit.operation == "decode Parasolid compact values"));
    crate::test_support::work_refusal_at("decode Parasolid compact values", |ctx| scalar_conflict(ctx, 0));
}

/// Bound both selected-pair growth and later integer-prefix growth. Each
/// growing vector can overlap its preceding half-capacity allocation.
fn live_knot_storage_cap(last_count: usize) -> u64 {
    const PROBE: &str = "test live selected knot pair and count markers";
    [false, true].map(|validating| {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes, PROBE, |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy();
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let mut storage = ctx.reserve_scoped(0, "test live knot layout")?;
                storage.with_storage(|| {
                    let multiplicities = super::read_array(&ctx, 200, "test selected integer slots", |_| Some(2_u16))?.unwrap();
                    let knots = super::read_array(&ctx, 200, "test selected scalar slots", |_| Some(0.0_f64))?.unwrap();
                    let mut counts = std::collections::BTreeMap::new();
                    let mut scratch = Vec::new();
                    let overlap = if validating {
                        for count in 200..=last_count {
                            ctx.insert_btree_map(&mut counts, count, true, "test count markers")?;
                        }
                        scratch = super::read_array(&ctx, 200, "test one validating prefix", |_| Some(2_u16))?.unwrap();
                        scratch.capacity() / 2 * std::mem::size_of::<u16>()
                    } else {
                        knots.capacity() / 2 * std::mem::size_of::<f64>()
                    };
                    let moving = ctx.reserve_scoped(u64::try_from(overlap).unwrap(), "test preceding vector allocation")?;
                    let result = ctx.reserve_scoped(1024 * 1024, PROBE).map(drop);
                    drop((knots, multiplicities, scratch, counts, moving));
                    result
                })
            },
        );
        let CodecError::ResourceLimit(limit) = error else { panic!("live knot storage boundary") };
        limit.used
    }).into_iter().max().unwrap()
}

fn agreeing_knot_counts(ctx: &DecodeContext<'_>, last_count: usize, aliases: usize) -> Result<(), CodecError> {
    let (mut bytes, mut arrays) = knot_pair(200, false);
    let mut knots = Vec::new();
    let mut multiplicities = Vec::new();
    for count in 201..=last_count {
        let offset = bytes.len();
        bytes.extend(compact_f64_array(40, &vec![0.0; count]));
        knots.push(CompactArray { offset, count });
        for _ in 0..aliases {
            let offset = bytes.len();
            bytes.extend([0, u8::try_from(count).unwrap(), 0, 41]);
            for index in 0..count {
                bytes.extend((if index < 200 { 2_u16 } else { 0 }).to_be_bytes());
            }
            multiplicities.push(CompactArray { offset, count });
        }
    }
    arrays.compact.insert(40, knots);
    arrays.compact.insert(41, multiplicities);
    let mut output = ctx.reserve_scoped(0, "test agreeing count output")?;
    let result = super::surface_knot_values(ctx, &mut output, &bytes, &arrays, (40, 41, 200))?.unwrap();
    assert_eq!(result.unique, vec![0.0; 200]);
    assert_eq!(result.multiplicities, vec![2; 200]);
    drop((result, output, arrays));
    Ok(())
}

#[test]
fn positive_knot_count_cache_keeps_only_agreement_markers() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = live_knot_storage_cap(255);
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    agreeing_knot_counts(&ctx, 255, 1).unwrap();
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test released count marker storage").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn later_knot_prefix_is_absent_during_multiplicity_alias_validation() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = live_knot_storage_cap(201);
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    agreeing_knot_counts(&ctx, 201, 2).unwrap();
    ctx.finish_session().unwrap();
}

fn selected_pair_collection_cap(count: usize) -> u64 {
    const PROBE: &str = "test collection after selected knot pair";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems, PROBE, |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let mut storage = ctx.reserve_scoped(0, "test exclusive pair layout")?;
            storage.with_storage(|| {
                let knots = super::read_array(&ctx, count, "test scalar prefix slots", |_| Some(0.0_f64))?;
                let multiplicities = super::read_array(&ctx, count, "test integer prefix slots", |_| Some(2_u16))?;
                let mut markers = std::collections::BTreeMap::new();
                ctx.insert_btree_map(&mut markers, count, true, "test selected count marker")?;
                let result = ctx.collection_vec::<u8>(1, PROBE).map(drop);
                drop((knots, multiplicities, markers));
                result
            })
        },
    );
    let CodecError::ResourceLimit(limit) = error else { panic!("selected pair collection boundary") };
    limit.used
}

#[test]
fn owned_surface_knot_prefixes_move_without_second_collection_admission() {
    const PROBE: &str = "test transferred knot payload ownership";
    for (count, compact) in [(100_000, false), (200, true)] {
        let arena = DecodeArena::new();
        let mut policy = policy();
        policy.limits.max_collection_items = selected_pair_collection_cap(count);
        policy.limits.max_materialized_bytes = u64::MAX;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (bytes, arrays) = knot_pair(count, compact);
        let mut output = ctx.reserve_scoped(0, "test moved knot output").unwrap();
        let result = super::surface_knot_values(&ctx, &mut output, &bytes, &arrays, (40, 41, count)).unwrap().unwrap();
        assert_eq!(result.unique, vec![0.0; count]);
        assert_eq!(result.multiplicities, vec![2; count]);
        let live_bytes = result.unique.capacity() * std::mem::size_of::<f64>()
            + result.multiplicities.capacity() * std::mem::size_of::<u16>();
        let probe = RefusalProbe::arm(ResourceDimension::MaterializedBytes, PROBE, None);
        let Err(CodecError::ResourceLimit(limit)) = ctx.reserve_scoped(1024 * 1024 * 1024, PROBE) else { panic!("transferred payload ownership") };
        assert_eq!(limit.used, u64::try_from(live_bytes).unwrap());
        drop((probe, result, output, arrays));
    }
}

fn borrowed_knot_prefix(ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    let (bytes, arrays) = knot_pair(200, false);
    arrays.f64s[&40].read(ctx, |at| cadmpeg_core::decode::View::f64_be_at(&bytes, at))?;
    let mut output = ctx.reserve_scoped(0, "test borrowed knot output")?;
    let result = super::surface_knot_values(ctx, &mut output, &bytes, &arrays, (40, 41, 200));
    if let Err(CodecError::ResourceLimit(limit)) = &result {
        assert_eq!(ctx.resource_refusal(), Some(*limit));
    }
    let result = result?.unwrap();
    assert_eq!(result.unique, vec![0.0; 200]);
    assert_eq!(result.multiplicities, vec![2; 200]);
    drop((result, output, arrays));
    Ok(())
}

#[test]
fn borrowed_surface_knots_copy_the_declared_prefix() {
    const PROBE: &str = "copy Parasolid distinct knots";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems, PROBE, |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            borrowed_knot_prefix(&ctx)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == PROBE));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    borrowed_knot_prefix(&ctx).unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn matching_physical_knot_counts_reuse_multiplicity_validation() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_collection_items = selected_pair_collection_cap(200);
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (mut bytes, mut arrays) = knot_pair(200, false);
    let mut aliases = Vec::new();
    for _ in 0..128 {
        let offset = bytes.len();
        bytes.extend(compact_f64_array(40, &[0.0; 200]));
        aliases.push(CompactArray { offset, count: 200 });
    }
    arrays.compact.insert(40, aliases);
    let mut output = ctx.reserve_scoped(0, "test cached physical knot count output").unwrap();
    let result = super::surface_knot_values(&ctx, &mut output, &bytes, &arrays, (40, 41, 200)).unwrap().unwrap();
    assert_eq!(result.unique, vec![0.0; 200]);
    assert_eq!(result.multiplicities, vec![2; 200]);
    drop((result, output, arrays));
    ctx.finish_session().unwrap();
}
