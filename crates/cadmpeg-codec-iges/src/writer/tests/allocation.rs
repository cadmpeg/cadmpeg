// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::units::FiniteVector;

#[test]
fn nurbs_writer_weights_refuse_one_below_collection_need() {
    let curve = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .expect("polynomial curve");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let range = FiniteVector::new([0.0, 1.0]).expect("finite range");
    let error = super::super::encode_nurbs(&ctx, &curve, range, "NURBS")
        .err()
        .expect("two weights exceed one slot");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "iges NURBS weights"));
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).expect("root");
    assert!(super::super::encode_nurbs(&ctx, &curve, range, "NURBS").is_ok());
}

#[test]
fn nurbs_surface_writer_weights_refuse_one_below_collection_need() {
    let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
    let surface = NurbsSurface::from_lanes(
        axis(),
        axis(),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
            None,
        ),
        false,
    )
    .expect("polynomial surface");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let error = super::super::encode_nurbs_surface(&ctx, &surface)
        .err()
        .expect("four weights exceed three slots");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "iges NURBS surface weights"));
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).expect("root");
    assert!(super::super::encode_nurbs_surface(&ctx, &surface).is_ok());
}
