// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};

#[test]
fn native_brep_nurbs_subset_evaluation_refuses_scoped_limit() {
    let mut body = crate::test_support::parasolid::triangle_body();
    body.extend(crate::test_support::parasolid::linear_nurbs_curve_carrier(
        70, 80,
    ));
    body.extend_from_slice(&[0x00, 0x85]);
    crate::test_support::parasolid::be16(&mut body, 90);
    crate::test_support::parasolid::be32(&mut body, 1);
    for reference in [1, 2, 3, 4, 1] {
        crate::test_support::parasolid::be16(&mut body, reference);
    }
    body.push(0x2b);
    crate::test_support::parasolid::be16(&mut body, 70);
    for value in [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] {
        crate::test_support::parasolid::bef64(&mut body, value);
    }
    body.extend(crate::test_support::parasolid::edge_use(40, 90));
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 15;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
    let Err(error) = super::decode_body(&ctx, &body, &cadmpeg_ir::stream_name!("basis-admission"))
    else {
        panic!("expected scoped basis refusal");
    };
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
    );
    policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
    let brep =
        super::decode_body(&ctx, &body, &cadmpeg_ir::stream_name!("basis-admission")).unwrap();
    assert_eq!(
        brep.curves
            .iter()
            .filter(|curve| matches!(
                &curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_))
            ))
            .count(),
        1
    );
}

#[test]
fn body_stream_index_storage_refusal_uses_the_scoped_dimension() {
    let header = crate::parasolid::StreamHeader {
        description: String::from("test partition"),
        schema: cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_TEST_1_9999").unwrap(),
        body_offset: 0,
    };
    let bodies = [(&[][..], &header)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(error) = super::decode_bodies(&ctx, &bodies, &cadmpeg_ir::stream_name!("test-stream-index")) else { panic!("temporary stream index refusal"); };
    let CodecError::ResourceLimit(limit) = error else { panic!("temporary stream index refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "order Parasolid body streams");
    assert!(limit.additional > 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert_eq!(header.description, "test partition");
}

#[test]
fn reference_qualifier_character_refusal_preserves_the_resource_limit() {
    let arena = DecodeArena::new();
    let (service, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(super::qualified_reference(&service, "test#1", "scope").unwrap(), "test#1@scope");
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::qualified_reference(&ctx, "test#1", "scope").unwrap_err();
    let CodecError::ResourceLimit(limit) = error else { panic!("character copy refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "append SLDPRT reference qualifier");
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
