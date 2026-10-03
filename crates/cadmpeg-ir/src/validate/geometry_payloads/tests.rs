// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use crate::math::{Point3, Vector3};
use crate::tessellation::{Tessellation, TessellationMesh};
use crate::validate::validate_neutral;

#[test]
fn tessellation_counts_must_be_consistent() {
    use crate::ids::FaceId;
    use crate::math::Point3;

    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.model.tessellations.push(
        Tessellation::new(
            crate::tessellation::TessellationId::mint("synthetic:test:tessellation#invalid-counts")
                .expect("valid identity"),
            TessellationMesh::List {
                vertices: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .expect("valid tessellation")
        .with_faces(vec![
            FaceId::mint("synthetic:test:face#missing").expect("valid identity")
        ]),
    );
    ir.finalize(&cadmpeg_test_support::service_decode_context())
        .expect("fixture ordering is admitted");
    let report = validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail");
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("missing tessellation face")));
}

#[test]
fn tessellation_triangle_groups_and_texture_assignments_validate() {
    use crate::assets::{Asset, AssetContent, AssetId};
    use crate::math::Point3;
    use crate::report::{check::Check, Severity};
    use crate::tessellation::{
        Tessellation, TessellationTextureAssignment, TessellationTriangleGroup,
    };

    let texture = AssetId::mint("synthetic:test:asset#mesh-texture").expect("identity grammar");
    let valid = Tessellation::new(
        crate::tessellation::TessellationId::mint("synthetic:test:tessellation#valid-groups")
            .expect("valid identity"),
        TessellationMesh::List {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2], [1, 3, 2]],
        },
        Vec::new(),
    )
    .expect("valid tessellation")
    .with_triangle_groups(vec![
        TessellationTriangleGroup {
            source_id: Some("group-a".into()),
            triangles: vec![0],
        },
        TessellationTriangleGroup {
            source_id: Some("group-b".into()),
            triangles: vec![1],
        },
    ])
    .expect("valid triangle group partition")
    .with_texture_assignments(vec![
        TessellationTextureAssignment {
            source_id: Some("texture-resource-a".into()),
            texture: texture.clone(),
            triangles: vec![0],
        },
        TessellationTextureAssignment {
            source_id: Some("texture-resource-b".into()),
            texture: texture.clone(),
            triangles: vec![1],
        },
    ])
    .expect("valid texture assignments");
    let mut invalid_texture = valid
        .clone()
        .with_texture_assignments(vec![TessellationTextureAssignment {
            source_id: Some("texture-resource-a".into()),
            texture: AssetId::mint("synthetic:test:asset#missing").expect("identity grammar"),
            triangles: vec![0],
        }])
        .expect("valid local texture assignment");
    invalid_texture.id = "synthetic:test:tessellation#missing-texture"
        .try_into()
        .unwrap();

    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.model.assets.push(
        Asset::try_new(
            texture,
            None,
            None,
            AssetContent::Embedded {
                data: crate::assets::AssetData::new(vec![0]).expect("nonempty asset data"),
            },
            None,
        )
        .expect("valid asset"),
    );
    ir.model.tessellations.extend([valid, invalid_texture]);
    ir.finalize(&cadmpeg_test_support::service_decode_context())
        .expect("fixture ordering is admitted");
    let report = validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail");
    let errors_for = |entity: &str| {
        report
            .findings
            .iter()
            .filter(|finding| {
                finding.check == Check::Tessellation
                    && finding.severity == Severity::Error
                    && finding.entity.as_deref() == Some(entity)
            })
            .count()
    };
    assert_eq!(errors_for("synthetic:test:tessellation#valid-groups"), 0);
    assert_eq!(errors_for("synthetic:test:tessellation#missing-texture"), 1);
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|finding| finding
                .message
                .contains("missing tessellation texture asset"))
            .count(),
        1
    );
}

#[test]
fn finite_nonzero_signed_sphere_radius_is_valid_without_a_size_floor() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        crate::geometry::analytic::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            -1e-200,
        )
        .unwrap(),
    ));
    let report = validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail");
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn tessellation_reference_validation_preserves_resource_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut ir = unit_cube().unwrap();
    ir.model.tessellations.push(
        Tessellation::new(
            "test:model:tessellation#missing".try_into().unwrap(),
            TessellationMesh::List {
                vertices: vec![Point3::new(0.0, 0.0, 0.0)],
                triangles: Vec::new(),
            },
            Vec::new(),
        )
        .unwrap()
        .with_body(Some("test:model:body#missing".try_into().unwrap())),
    );
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
        ResourceDimension::RetainedBytes,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        let Err(CodecError::ResourceLimit(limit)) =
            super::check_tessellations(&ctx, &ir, &mut findings)
        else {
            panic!("tessellation check must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(findings.is_empty());
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}

#[test]
fn tessellation_reference_validation_preserves_finding_order_and_releases_indexes() {
    use crate::report::{check::Check, Severity};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let owner = "test:model:tessellation#missing";
    let mut ir = unit_cube().unwrap();
    ir.model.tessellations.push(
        Tessellation::new(
            owner.try_into().unwrap(),
            TessellationMesh::List {
                vertices: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .unwrap()
        .with_body(Some("test:model:body#missing".try_into().unwrap()))
        .with_faces(vec![
            "test:model:face#missing".try_into().unwrap(),
            "test:model:face#also-missing".try_into().unwrap(),
        ])
        .with_texture_assignments(vec![crate::tessellation::TessellationTextureAssignment {
            source_id: None,
            texture: "test:model:asset#missing".try_into().unwrap(),
            triangles: vec![0],
        }])
        .unwrap(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    super::check_tessellations(&ctx, &ir, &mut findings).unwrap();
    assert_eq!(findings.len(), 3);
    for (finding, message) in findings.iter().zip([
        "references a missing tessellation body",
        "references a missing tessellation face",
        "references a missing tessellation texture asset",
    ]) {
        assert_eq!(finding.check, Check::Tessellation);
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(finding.entity.as_deref(), Some(owner));
        assert_eq!(finding.message, message);
    }
    let released = ctx
        .reserve_scoped(4096, "tessellation reference indexes released")
        .unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}
