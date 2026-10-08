// SPDX-License-Identifier: Apache-2.0
//! Mesh transfer and source-admission tests.

use super::*;

#[test]
fn mesh_feature_scope_search_and_tessellation_scan_preserve_work_refusal() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    let scope_id = "f3d:Design/BulkStream.dat:design-parameter-scope#10";
    let scope = DesignParameterScope::empty(
        scope_id,
        crate::records::feature::scope::DesignFeatureKind::BaseMeshFeature,
        10,
    );
    let feature = Feature {
        id: FeatureId::mint("f3d:test:feature#mesh-search").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Base Mesh Feature".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Base Mesh Feature".into(),
                parameters: Default::default(),
            }),
        ),
        native_ref: Some(scope_id.into()),
    };
    let projection = MeshProjection {
        count: 1,
        tessellations_by_scope: std::collections::HashMap::from([(
            ("f3d:Design/BulkStream.dat".into(), 10),
            vec!["tessellation:one".into()],
        )]),
    };
    for operation in [
        "find F3D mesh feature scope",
        "index F3D mesh feature scopes",
        "scan F3D mesh feature tessellations",
    ] {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| {
                bind_mesh_feature_definitions(
                    ctx,
                    &mut [feature.clone()],
                    std::slice::from_ref(&scope),
                    &projection,
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == operation)
        );
    }
}

#[test]
fn triangle_attribute_selector_range_refuses_work_after_valid_output() {
    let triangles = [[0, 1, 2]];
    crate::test_support::with_decode_context(|ctx| {
        let attribute = crate::paramesh::triangle_domain_test_attribute(ctx)
            .expect("triangle-domain attribute");
        let mut unresolved = std::collections::BTreeMap::new();
        let mut unresolved_storage = ctx.reserve_scoped(0, "test mesh attribute counts").unwrap();
        let channels = mesh_attribute_channels(
            ctx,
            std::slice::from_ref(&attribute),
            3,
            &triangles,
            &mut unresolved,
            &mut unresolved_storage,
        )
        .expect("valid triangle attribute");
        assert!(unresolved.is_empty());
        assert_eq!(channels.len(), 1);
        assert!(matches!(
            wire::field::<cadmpeg_ir::tessellation::ChannelAddressing>(&channels[0], "addressing"),
            cadmpeg_ir::tessellation::ChannelAddressing::Triangle { .. }
        ));
        assert_eq!(channels[0].indices(), [0]);
    });

    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan F3D mesh triangle attribute selectors",
        0,
        |ctx| {
            let attribute = crate::paramesh::triangle_domain_test_attribute(ctx)?;
            let mut unresolved = std::collections::BTreeMap::new();
            let mut unresolved_storage = ctx.reserve_scoped(0, "test mesh attribute counts")?;
            mesh_attribute_channels(
                ctx,
                std::slice::from_ref(&attribute),
                3,
                &triangles,
                &mut unresolved,
                &mut unresolved_storage,
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "scan F3D mesh triangle attribute selectors")
    );
}

#[test]
fn mesh_texture_assignment_source_scan_refuses_work_after_valid_output() {
    let textures = one_mesh_texture();
    with_test_ctx(|ctx| {
        let assignments = mesh_texture_assignments(ctx, Some(&[1]), &textures, 1)
            .expect("valid mesh texture assignment");
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].triangles, [0]);
    });

    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan F3D mesh texture assignment rows",
        0,
        |ctx| mesh_texture_assignments(ctx, Some(&[1]), &textures, 1).map(|_| ()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "scan F3D mesh texture assignment rows")
    );
}

#[test]
fn corner_shaded_mesh_admits_the_triangle_source() {
    use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
    use cadmpeg_ir::tessellation::{ShadedTriangle, TessellationMesh};

    let vertices = vec![FinitePoint3::ZERO; 3];
    let triangles = vec![[0, 1, 2]];
    let normals = vec![FiniteVector3::ZERO; 3];
    let expected = TessellationMesh::CornerShadedList {
        vertices: vertices.clone(),
        triangles: vec![ShadedTriangle {
            corners: [0, 1, 2],
            normals: [FiniteVector3::ZERO; 3],
        }],
    };
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            super::super::super::corner_shaded_mesh(
                ctx,
                "body#7",
                vertices.clone(),
                triangles.clone(),
                Some(normals.clone()),
            )
            .expect("valid corner-shaded mesh"),
            expected
        );
    });

    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan F3D corner-shaded triangle rows",
        0,
        |ctx| {
            super::super::super::corner_shaded_mesh(
                ctx,
                "body#7",
                vertices.clone(),
                triangles.clone(),
                Some(normals.clone()),
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "scan F3D corner-shaded triangle rows")
    );
}

#[test]
fn corner_shaded_mesh_admits_the_normal_source() {
    use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};

    let vertices = vec![FinitePoint3::ZERO; 3];
    let triangles = vec![[0, 1, 2]];
    let normals = vec![FiniteVector3::ZERO; 3];
    crate::test_support::with_decode_context(|ctx| {
        super::super::super::corner_shaded_mesh(
            ctx,
            "body#7",
            vertices.clone(),
            triangles.clone(),
            Some(normals.clone()),
        )
        .expect("valid corner-shaded mesh");
    });

    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan F3D corner-shaded normal values",
        0,
        |ctx| {
            super::super::super::corner_shaded_mesh(
                ctx,
                "body#7",
                vertices.clone(),
                triangles.clone(),
                Some(normals.clone()),
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "scan F3D corner-shaded normal values")
    );
}

#[test]
fn corner_shaded_mesh_output_refuses_retained_bytes() {
    use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};

    let vertices = vec![FinitePoint3::ZERO; 3];
    let triangles = vec![[0, 1, 2]];
    let normals = vec![FiniteVector3::ZERO; 3];
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "collect F3D corner-shaded triangles",
        0,
        |ctx| {
            super::super::super::corner_shaded_mesh(
                ctx,
                "body#7",
                vertices.clone(),
                triangles.clone(),
                Some(normals.clone()),
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D corner-shaded triangles")
    );
}

#[test]
fn corner_shaded_mesh_output_refuses_collection_items() {
    use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};

    let vertices = vec![FinitePoint3::ZERO; 3];
    let triangles = vec![[0, 1, 2]];
    let normals = vec![FiniteVector3::ZERO; 3];
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D corner-shaded triangles",
        0,
        |ctx| {
            super::super::super::corner_shaded_mesh(
                ctx,
                "body#7",
                vertices.clone(),
                triangles.clone(),
                Some(normals.clone()),
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D corner-shaded triangles")
    );
}

#[test]
fn corner_shaded_mesh_preserves_unshaded_and_malformed_lane_results() {
    use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
    use cadmpeg_ir::tessellation::TessellationMesh;

    let vertices = vec![FinitePoint3::ZERO; 3];
    let triangles = vec![[0, 1, 2]];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            super::super::super::corner_shaded_mesh(
                ctx,
                "body#7",
                vertices.clone(),
                triangles.clone(),
                None,
            )
            .expect("valid unshaded mesh"),
            TessellationMesh::List {
                vertices: vertices.clone(),
                triangles: triangles.clone(),
            }
        );

        let error = super::super::super::corner_shaded_mesh(
            ctx,
            "body#7",
            vertices,
            triangles,
            Some(Vec::<FiniteVector3>::new()),
        )
        .expect_err("a short corner-normal lane is malformed");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::Malformed(message)
                if message == "paramesh body record body#7: 3 triangle corner(s) against 0 corner normal(s)"
        ));
    });
}
