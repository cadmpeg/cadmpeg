// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::curves::GeometryError;

#[test]
fn extrusion_knot_slice_equality_preserves_refusal() {
    let (start, end) = super::simple_extrusion_curves();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let error = super::super::extrusion_nurbs(
        &ctx, &start, &end,
        cadmpeg_ir::units::FiniteVector::new([0.0, 1.0]).unwrap(), false, 0,
    ).unwrap_err();
    let GeometryError::Codec(CodecError::ResourceLimit(refusal)) = error else {
        panic!("knot comparison must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "Rhino extrusion knot equality");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}
