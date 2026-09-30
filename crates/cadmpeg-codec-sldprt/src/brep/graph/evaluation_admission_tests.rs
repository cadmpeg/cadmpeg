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
    policy.limits.max_materialized_bytes = 16;
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
