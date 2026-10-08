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
    let service = cadmpeg_test_support::service_decode_context();
    let mut carriers = super::scan_carriers(&service, &body).unwrap();
    let quadratic = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &service,
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(500.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1000.0, 0.0, 0.0),
        ],
        None,
        false,
    ).unwrap().unwrap();
    carriers.insert(&service, crate::brep::Carrier::Curve(crate::brep::CurveCarrier {
        attr: 70,
        offset: 0,
        end: 0,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(quadratic)),
        parameter_range: None,
    })).unwrap();
    let operation = "IR B-spline basis";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy)?;
            let result = super::super::subset::scan(&ctx, &body, &carriers);
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == operation));
    let policy = DecodePolicy::service();
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
        words: crate::container::NameWords::PARTITION_ONLY,
        schema: cadmpeg_parasolid::OwnedSchemaToken::parse(
            &cadmpeg_test_support::service_decode_context(),
            "SCH_TEST_1_9999".into(),
        )
        .expect("service token admission")
        .expect("the fixture text is a schema token"),
        body_offset: 0,
    };
    let bodies = [(&[][..], &header)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(error) = super::decode_bodies(
        &ctx,
        &bodies,
        &cadmpeg_ir::stream_name!("test-stream-index"),
    ) else {
        panic!("temporary stream index refusal");
    };
    let CodecError::ResourceLimit(limit) = error else {
        panic!("temporary stream index refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "order Parasolid body streams");
    assert!(limit.additional > 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert_eq!(header.description, "test partition");
}

#[test]
fn reference_qualifier_refusal_preserves_the_resource_limit() {
    let arena = DecodeArena::new();
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        super::qualified_reference(&service, "test#1", "scope").unwrap(),
        "test#1@scope"
    );
    assert_eq!(
        super::qualified_reference(&service, "plain", "scope").unwrap(),
        "plain"
    );
    let error = crate::test_support::work_refusal_at("qualify SLDPRT reference", |ctx| {
        super::qualified_reference(ctx, "test#1", "scope")
    });
    let CodecError::ResourceLimit(limit) = error else {
        panic!("qualified reference refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "qualify SLDPRT reference");
}

#[test]
fn emitted_face_identity_fields_use_retained_storage() {
    let mut body = crate::test_support::parasolid::triangle_body();
    body.extend([0x00, 0x4f]);
    body.extend(12_u32.to_be_bytes());
    body.extend(15_u16.to_be_bytes());
    body.extend(b"ATOM_ID_2001");
    body.extend([0x00, 0x50]);
    body.extend(2_u32.to_be_bytes());
    body.extend(16_u16.to_be_bytes());
    body.extend([0x00, 0x52]);
    body.extend(7_u32.to_be_bytes());
    body.extend(300_u16.to_be_bytes());
    for value in [49_u32, 266, 1_704_609_508, 0, 2, 10, 8] {
        body.extend(value.to_be_bytes());
    }
    body.extend([0x00, 0x51]);
    body.extend(4_u32.to_be_bytes());
    for value in [301_u16, 0, 302, 16, 10, 300] {
        body.extend(value.to_be_bytes());
    }
    let operation = "retain Parasolid face identity fields";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy)?;
            super::decode_body(&ctx, &body, &cadmpeg_ir::stream_name!("identity-storage"))
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == operation && limit.additional == 8));
    let ctx = cadmpeg_test_support::service_decode_context();
    let brep = super::decode_body(&ctx, &body, &cadmpeg_ir::stream_name!("identity-storage")).unwrap();
    assert_eq!(brep.face_atoms.len(), 1);
    assert_eq!(brep.face_atoms[0].identity.trailing_fields, [10, 8]);
    assert_eq!(brep.face_atoms[0].face, super::id_face(10));
}

#[test]
fn nurbs_isocurve_size_mismatch_needs_no_knot_comparison_work() {
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use cadmpeg_ir::math::Point3;
    let service = cadmpeg_test_support::service_decode_context();
    let surface = NurbsSurface::from_lanes(
        &service,
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(vec![
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
        ], None),
        false,
    ).unwrap().unwrap();
    let curve = NurbsCurve::from_lanes(
        &service,
        1,
        vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.5, 0.0), Point3::new(0.0, 1.0, 0.0)],
        None,
        false,
    ).unwrap().unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(super::nurbs_strict_isocurve_pcurve(&ctx, &surface, &curve).unwrap(), super::InverseResolution::NoMatch));
    ctx.finish_session().unwrap();
}
