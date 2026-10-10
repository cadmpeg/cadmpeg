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
            let mut scratch = ctx.reserve_scoped(0, "test knot scratch").unwrap();
            let mut output = ctx.reserve_scoped(0, "test knot output").unwrap();
            let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
            assert!(super::surface_knot_values(
                &ctx,
                &mut scratch,
                &mut output,
                &bytes,
                &arrays,
                (40, 41, 2)
            )
            .unwrap()
            .is_none());
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
    let mut scratch = ctx.reserve_scoped(0, "test knot scratch").unwrap();
    let mut output = ctx.reserve_scoped(0, "test knot output").unwrap();
    Ok(
        super::surface_knot_values(ctx, &mut scratch, &mut output, &bytes, &arrays, (40, 41, 2))?
            .is_some(),
    )
}

#[test]
fn surface_knot_ambiguity_skips_a_long_payload_suffix() {
    // Header-count intersection necessarily reads every header. Its bound
    // scales with the roster, while payload visits stop after the conflict.
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "scan Parasolid compact attribute arrays",
        Some(128),
    );
    assert!(!knot_resolution(&ctx, 128).unwrap());
    let Err(CodecError::ResourceLimit(limit)) =
        ctx.charge_work(128, "scan Parasolid compact attribute arrays")
    else {
        panic!("candidate suffix precharge remains unused");
    };
    assert_eq!(limit.additional, 128);
    drop(probe);
}

#[test]
fn absent_multiplicity_headers_skip_knot_count_index_storage() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let bytes = f64_array(0x80, 40, &[0.0; 4096]);
    let mut arrays = Arrays::default();
    arrays.f64s.insert(
        40,
        framed(&bytes, 0, "decode Parasolid scalar array values"),
    );
    let mut scratch = ctx.reserve_scoped(0, "test knot scratch").unwrap();
    let mut output = ctx.reserve_scoped(0, "test knot output").unwrap();
    assert!(super::surface_knot_values(
        &ctx,
        &mut scratch,
        &mut output,
        &bytes,
        &arrays,
        (40, 41, 2)
    )
    .unwrap()
    .is_none());
    assert!(arrays.f64s[&40].values.get().is_none());
    drop(arrays);
    drop((scratch, output));
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
    let mut scratch = ctx.reserve_scoped(0, "test knot scratch").unwrap();
    let mut output = ctx.reserve_scoped(0, "test knot output").unwrap();
    let operation = "copy Parasolid knot multiplicities";
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    assert!(super::surface_knot_values(
        &ctx,
        &mut scratch,
        &mut output,
        &bytes,
        &arrays,
        (40, 41, 2)
    )
    .unwrap()
    .is_none());
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
    let mut scratch = ctx.reserve_scoped(0, "test knot scratch").unwrap();
    let mut output = ctx.reserve_scoped(0, "test knot output").unwrap();
    let operation = "decode Parasolid compact values";
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    assert!(super::surface_knot_values(
        &ctx,
        &mut scratch,
        &mut output,
        &bytes,
        &arrays,
        (40, 41, 2)
    )
    .unwrap()
    .is_none());
    require_armed(&ctx, operation);
    drop(probe);
}
