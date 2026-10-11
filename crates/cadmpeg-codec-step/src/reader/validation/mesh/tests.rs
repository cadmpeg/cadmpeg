// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::{
    ShadedTriangle, ShadedVertex, Strip, Strips, Tessellation, TessellationId, TessellationMesh,
};

fn meshes() -> Vec<Tessellation> {
    let points = vec![
        Point3::new(0., 0., 0.),
        Point3::new(1., 0., 0.),
        Point3::new(0., 1., 0.),
        Point3::new(1., 1., 0.),
    ];
    let triangles = vec![[0, 1, 2], [1, 3, 2]];
    let normal = Vector3::new(0., 0., 1.);
    let shaded = points
        .iter()
        .copied()
        .map(|position| ShadedVertex { position, normal })
        .collect::<Vec<_>>();
    let forms = [
        TessellationMesh::List {
            vertices: points.clone(),
            triangles: triangles.clone(),
        },
        TessellationMesh::ShadedList {
            vertices: shaded.clone(),
            triangles: triangles.clone(),
        },
        TessellationMesh::CornerShadedList {
            vertices: points.clone(),
            triangles: triangles
                .into_iter()
                .map(|corners| ShadedTriangle {
                    corners,
                    normals: [normal; 3],
                })
                .collect(),
        },
        TessellationMesh::Strips {
            strips: Strips::new(vec![Strip::new(points).expect("strip")]).expect("strips"),
        },
        TessellationMesh::ShadedStrips {
            strips: Strips::new(vec![Strip::new(shaded).expect("strip")]).expect("strips"),
        },
    ];
    forms
        .into_iter()
        .enumerate()
        .map(|(index, mesh)| {
            Tessellation::new(
                TessellationId::mint(format!("test:step:mesh#{index}")).expect("identity"),
                mesh,
                Vec::new(),
            )
            .expect("mesh")
        })
        .collect()
}

#[test]
fn borrowed_mesh_rows_preserve_every_shading_form_and_strip_winding() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    for mesh in meshes() {
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let origin = super::origin(&mesh).expect("origin");
            assert_eq!(origin.get(), Point3::new(0., 0., 0.));
            assert_eq!(
                super::extent(&mesh, origin, ctx).expect("borrowed extent"),
                1.0
            );
            let mut actual = Vec::new();
            assert!(super::triangles(&mesh, ctx, |indices, points| {
                actual.push((indices, points.map(cadmpeg_ir::features::FinitePoint3::get)));
                Ok(true)
            })
            .expect("borrowed triangles"));
            assert_eq!(
                actual,
                [
                    (
                        [0, 1, 2],
                        [
                            Point3::new(0., 0., 0.),
                            Point3::new(1., 0., 0.),
                            Point3::new(0., 1., 0.)
                        ]
                    ),
                    (
                        [1, 3, 2],
                        [
                            Point3::new(1., 0., 0.),
                            Point3::new(1., 1., 0.),
                            Point3::new(0., 1., 0.)
                        ]
                    ),
                ]
            );
        });
    }
}

#[test]
fn borrowed_mesh_triangle_stop_does_not_admit_unused_rows() {
    for (index, mesh) in meshes().into_iter().enumerate() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = if index < 3 { 1 } else { 2 };
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
        let mut visits = 0;
        assert!(!super::triangles(&mesh, &ctx, |_, _| {
            visits += 1;
            Ok(false)
        })
        .expect("first triangle fits"));
        assert_eq!(visits, 1);
        ctx.finish_session().expect("unused rows were not admitted");
    }
}

#[test]
fn borrowed_mesh_triangle_refusal_precedes_geometry_callback() {
    use cadmpeg_core::{decode::ResourceDimension, CodecError};
    for (index, mesh) in meshes().into_iter().enumerate() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::from(index >= 3);
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut visits = 0;
            let error = super::triangles(&mesh, ctx, |_, _| {
                visits += 1;
                Ok(true)
            })
            .expect_err("first triangle exceeds work cap");
            assert_eq!(visits, 0);
            assert!(
                matches!(error,CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::WorkUnits && refusal.operation == "step_validation_mesh_triangles")
            );
        });
    }
}

#[test]
fn borrowed_strip_stop_does_not_admit_unused_strips() {
    let strip = || {
        Strip::new(vec![
            Point3::new(0., 0., 0.),
            Point3::new(1., 0., 0.),
            Point3::new(0., 1., 0.),
        ])
        .expect("strip")
    };
    let mesh = Tessellation::new(
        TessellationId::mint("test:step:mesh#two-strips").expect("identity"),
        TessellationMesh::Strips {
            strips: Strips::new(vec![strip(), strip()]).expect("strips"),
        },
        Vec::new(),
    )
    .expect("mesh");
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let mut visits = 0;
    assert!(!super::triangles(&mesh, &ctx, |_, _| {
        visits += 1;
        Ok(false)
    })
    .expect("one strip and triangle fit"));
    assert_eq!(visits, 1);
    ctx.finish_session().expect("second strip was not admitted");
}
