// SPDX-License-Identifier: Apache-2.0

use super::line_nurbs;

#[test]
fn reused_c2_curve_copy_refuses_retained_limit_one_byte_below_full_copy() {
    let curve = line_nurbs(0.0, 1.0, false);
    let bytes = curve.knots().len() * std::mem::size_of::<f64>()
        + curve.pole_count() * std::mem::size_of::<cadmpeg_ir::features::FinitePoint3>();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(bytes - 1).expect("copy size");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test input");
    let error = curve
        .try_clone_for_decode(&ctx, "Rhino Brep reused C2 curve")
        .expect_err("full C2 copy exceeds limit by one byte");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "Rhino Brep reused C2 curve"
                && refusal.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
    ));
}

#[test]
fn cached_c2_curve_copy_refuses_retained_limit_one_byte_below_full_copy() {
    let curve = line_nurbs(0.0, 1.0, false);
    let bytes = curve.knots().len() * std::mem::size_of::<f64>()
        + curve.pole_count() * std::mem::size_of::<cadmpeg_ir::features::FinitePoint3>();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(bytes - 1).expect("copy size");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test input");
    let error = curve
        .try_clone_for_decode(&ctx, "Rhino Brep cached C2 curve")
        .expect_err("full cached C2 copy exceeds limit by one byte");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "Rhino Brep cached C2 curve"
                && refusal.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
    ));
}
