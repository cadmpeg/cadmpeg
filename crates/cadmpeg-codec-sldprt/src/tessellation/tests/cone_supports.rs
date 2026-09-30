// SPDX-License-Identifier: Apache-2.0
//! Conical display support and trim bounds.

use super::{add_face, model_with_body, set_shell_faces};
use crate::tessellation::{analytic_surface_normal, ConicalTrim};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::Tessellation;

#[test]
fn cone_support_binds_display_list_face() {
    let mut model = model_with_body();
    let cone = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            0.5,
            std::f64::consts::FRAC_PI_4,
        )
        .unwrap(),
    ));
    let v = 2.0;
    let local_radius = 3.0 + v * std::f64::consts::FRAC_PI_4.tan();
    let face = add_face(
        &mut model,
        "cone",
        cone,
        [
            Point3::new(local_radius, 0.0, v),
            Point3::new(0.0, local_radius * 0.5, v),
            Point3::new(-local_radius, 0.0, v),
            Point3::new(0.0, -local_radius * 0.5, v),
        ],
    );
    set_shell_faces(&mut model, vec![face.clone()]);
    model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#cone-mesh",
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: vec![
                    Point3::new(local_radius, 0.0, v),
                    Point3::new(0.0, local_radius * 0.5, v),
                    Point3::new(-local_radius, 0.0, v),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .expect("valid tessellation"),
    );

    assert_eq!(
        assign_unique_surface_owners!(&mut model).unwrap(),
        vec!["synthetic:test:tessellation#cone-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert_eq!(
        model.tessellations[0].body,
        Some(BodyId::mint("synthetic:test:body#body").expect("identity grammar"))
    );
}

#[test]
fn cone_chordal_display_list_uses_analytic_normal_for_ownership() {
    let mut model = model_with_body();
    let cone = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            0.5,
            std::f64::consts::FRAC_PI_4,
        )
        .unwrap(),
    ));
    let axial = 2.0;
    let surface_radius = 3.0 + axial * std::f64::consts::FRAC_PI_4.tan();
    let face = add_face(
        &mut model,
        "cone-cache",
        cone.clone(),
        [
            Point3::new(surface_radius, 0.0, axial),
            Point3::new(0.0, surface_radius * 0.5, axial),
            Point3::new(-surface_radius, 0.0, axial),
            Point3::new(0.0, -surface_radius * 0.5, axial),
        ],
    );
    set_shell_faces(&mut model, vec![face.clone()]);
    let cache_radius = surface_radius - 0.1;
    let vertices = vec![
        Point3::new(cache_radius, 0.0, axial),
        Point3::new(0.0, cache_radius * 0.5, axial),
        Point3::new(-cache_radius, 0.0, axial),
    ];
    let normals = Some(
        vertices
            .iter()
            .map(|point| {
                analytic_surface_normal(cone.solved().expect("solved cone"), *point).unwrap()
            })
            .collect(),
    );
    model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#cone-cache-mesh",
            cadmpeg_ir::tessellation::TessellationMesh::from_list_lanes(
                vertices,
                vec![[0, 1, 2]],
                normals,
            )
            .expect("normals cover the mesh"),
            Vec::new(),
        )
        .expect("valid tessellation"),
    );

    assert_eq!(
        assign_unique_surface_owners!(&mut model).unwrap(),
        vec!["synthetic:test:tessellation#cone-cache-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert_eq!(
        model.tessellations[0].body,
        Some(BodyId::mint("synthetic:test:body#body").expect("identity grammar"))
    );
    assert!(model.tessellations[0]
        .chordal_deflection()
        .is_some_and(|deflection| deflection.get() > 0.09 && deflection.get() < 0.11));
}

#[test]
fn conical_trim_uses_scaled_angular_coordinate() {
    let trim = ConicalTrim {
        origin: Point3::new(0.0, 0.0, 0.0),
        frame: cadmpeg_ir::units::OrthonormalFrame3::new(
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        radius: 3.0,
        ratio: cadmpeg_ir::scalar::PositiveReal::new(0.5).unwrap(),
        slope: 1.0,
        min_axial: 0.0,
        max_axial: 2.0,
        angular_start: 0.0,
        angular_span: std::f64::consts::FRAC_PI_2,
    };
    let mesh = |point: Point3, id: &str| {
        Tessellation::new(
            id,
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: vec![point],
                triangles: Vec::new(),
            },
            Vec::new(),
        )
        .expect("valid tessellation")
    };
    let point_at = |angle: f64| {
        let local_radius = 4.0;
        Point3::new(
            local_radius * angle.cos(),
            local_radius * trim.ratio.get() * angle.sin(),
            1.0,
        )
    };

    assert!(trim.contains_mesh(
        &mesh(
            point_at(std::f64::consts::FRAC_PI_4),
            "synthetic:test:tessellation#inside"
        ),
        cadmpeg_ir::transform::Transform::identity(),
        0.0,
    ));
    assert!(!trim.contains_mesh(
        &mesh(
            point_at(3.0 * std::f64::consts::FRAC_PI_4),
            "synthetic:test:tessellation#outside"
        ),
        cadmpeg_ir::transform::Transform::identity(),
        0.0,
    ));
}
