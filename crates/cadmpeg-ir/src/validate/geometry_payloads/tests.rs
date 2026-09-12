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

    let mut ir = unit_cube();
    ir.model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#invalid-counts",
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
    ir.finalize();
    let report = validate_neutral(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("missing tessellation face")));
}

#[test]
fn tessellation_triangle_groups_and_texture_assignments_validate() {
    use crate::assets::{Asset, AssetContent, AssetId};
    use crate::math::Point3;
    use crate::report::{Check, Severity};
    use crate::tessellation::{
        Tessellation, TessellationTextureAssignment, TessellationTriangleGroup,
    };

    let texture = AssetId::mint("synthetic:test:asset#mesh-texture").expect("identity grammar");
    let valid = Tessellation::new(
        "synthetic:test:tessellation#valid-groups",
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

    let mut ir = unit_cube();
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
    ir.finalize();
    let report = validate_neutral(&ir, Vec::new());
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
    let mut ir = unit_cube();
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        crate::geometry::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            -1e-200,
        )
        .unwrap(),
    ));
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}
