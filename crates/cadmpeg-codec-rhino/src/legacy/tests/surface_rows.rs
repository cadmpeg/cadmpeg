// SPDX-License-Identifier: Apache-2.0
//! Outer surface rows admit only the visited prefix before allocation.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::chunks::{chunk_at, ArchiveVersion};

fn surface(rational: bool) -> Vec<u8> {
    let mut body = vec![3, 0, 1, 1];
    body.extend(1_u16.to_le_bytes());
    body.extend(1_u16.to_le_bytes());
    body.extend([u8::from(rational), 0, 0, 0, 0, 0]);
    body.extend([0_u8; 48]);
    for _ in 0..2 {
        body.extend(0.0_f64.to_le_bytes());
        body.extend(1.0_f64.to_le_bytes());
    }
    for point in [[0.0_f64, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]] {
        for coordinate in point { body.extend(coordinate.to_le_bytes()); }
        if rational { body.extend(2.0_f64.to_le_bytes()); }
    }
    super::legacy_chunk(super::TCODE_LEGACY_SRF,
        &super::legacy_chunk(super::TCODE_LEGACY_SRFSTUFF, &body))
}

fn refuses_first_row(rational: bool, operation: &str) {
    let data = surface(rational);
    let wrapper = chunk_at(&data, 0, data.len(), ArchiveVersion::V1, false).unwrap();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
            let result = super::super::legacy_surface(&ctx, &data, wrapper.body(),
                super::super::MillimeterScale::IDENTITY);
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal(), Some(*refusal));
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *refusal));
            }
            result
        });
    assert!(matches!(error, CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::WorkUnits
            && refusal.operation == operation && refusal.additional == 1));
    let ctx = cadmpeg_test_support::service_decode_context();
    let result = super::super::legacy_surface(&ctx, &data, wrapper.body(),
        super::super::MillimeterScale::IDENTITY).unwrap();
    assert_eq!((result.pole_grid().u_count(), result.pole_grid().v_count()), (2, 2));
    assert_eq!(result.weight(0, 0).map(|weight| weight.get()), rational.then_some(2.0));
    ctx.finish_session().unwrap();
}

#[test]
fn v1_surface_pole_rows_refuse_only_first_actual_visit() {
    refuses_first_row(false, "Rhino V1 surface pole row traversal");
}

#[test]
fn v1_surface_weight_rows_refuse_only_first_actual_visit() {
    refuses_first_row(true, "Rhino V1 surface weight row traversal");
}
