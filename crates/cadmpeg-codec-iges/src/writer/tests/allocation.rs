// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::units::FiniteVector;

#[test]
fn nurbs_writer_weights_refuse_one_below_collection_need() {
    let curve = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .expect("fixture constructor admission")
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
        &cadmpeg_test_support::service_decode_context(),
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
    .expect("fixture constructor admission")
    .expect("polynomial surface");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let error = super::super::encode_nurbs_surface(&ctx, &surface, None)
        .err()
        .expect("four weights exceed three slots");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "iges NURBS surface weights"));
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).expect("root");
    assert!(super::super::encode_nurbs_surface(&ctx, &surface, None).is_ok());
}

#[test]
fn polyline_nurbs_construction_preserves_caller_refusal_in_both_orientations() {
    use cadmpeg_ir::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::topology::Sense;

    let start = Point3::new(0.0, 0.0, 0.0);
    let end = Point3::new(1.0, 0.0, 0.0);
    let polyline = PolylineCurve::new(
        PolylineSamples::Parameterized {
            vertices: vec![
                PolylineVertex {
                    parameter: 0.0,
                    point: start,
                },
                PolylineVertex {
                    parameter: 1.0,
                    point: end,
                },
            ]
            .try_into()
            .expect("nonempty samples"),
        },
        0.0,
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("polyline construction admission")
    .expect("polyline");
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Polyline(polyline));
    let span = super::super::CurveSpan {
        range: FiniteVector::new([0.0, 1.0]).expect("range"),
        start,
        end,
    };
    for sense in [Sense::Forward, Sense::Reversed] {
        for dimension in [
            ResourceDimension::RetainedBytes,
            ResourceDimension::CollectionItems,
            ResourceDimension::WorkUnits,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = super::super::oriented_curve_entity(
                &ctx,
                &geometry,
                &span,
                sense,
                crate::IgesVersion::V5_3,
            );
            if dimension == ResourceDimension::RetainedBytes {
                assert!(
                    result.is_ok(),
                    "admitted polynomial poles move without retained storage"
                );
                assert!(ctx.finish_session().is_ok());
                continue;
            }
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("constructor refusal must stay outside the polyline geometry error");
            };
            assert_eq!(limit.dimension, dimension);
            let operation = match dimension {
                ResourceDimension::CollectionItems => "iges NURBS weights",
                ResourceDimension::WorkUnits => "IR NURBS knot finiteness",
                _ => unreachable!(),
            };
            assert_eq!(limit.operation, operation);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
    }
}
