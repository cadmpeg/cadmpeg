// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{pcurve_basis_is_valid, support_context_is_finite, valid_surface_basis};
use crate::examples::unit_cube;
use crate::geometry::{
    Curve, CurveGeometry, DirectedParameterRange, IntcurveSupportContext, IntcurveSupportSide,
    PcurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SupportPcurve, SurfaceGeometry,
};
use crate::ids::{CurveId, ProceduralSurfaceId};
use crate::math::{Point2, Point3, Vector3};
use crate::report::Check;
use crate::tessellation::{Tessellation, TessellationNormals, TessellationTopology};
use crate::validate::validate_neutral;

#[test]
fn explicit_support_mapping_requires_a_nonzero_solved_interval() {
    let mut context = IntcurveSupportContext {
        sides: [
            IntcurveSupportSide {
                surface: None,
                pcurve: Some(SupportPcurve::new(
                    PcurveGeometry::Line {
                        origin: Point2::new(0.0, 0.0),
                        direction: Point2::new(1.0, 0.0),
                    },
                    Some(DirectedParameterRange::new([5.0, 2.0]).unwrap()),
                )),
            },
            IntcurveSupportSide {
                surface: None,
                pcurve: None,
            },
        ],
        parameter_range: [0.0, 1.0],
        discontinuities: std::array::from_fn(|_| Vec::new()),
    };
    assert!(support_context_is_finite(&context));
    context.parameter_range = [1.0, 1.0];
    assert!(!support_context_is_finite(&context));
    context.sides[0].pcurve.as_mut().unwrap().parameter_range = None;
    assert!(support_context_is_finite(&context));
}

#[test]
fn exact_geometry_scalars_require_finite_nonzero_values_without_a_size_floor() {
    let tiny = 1e-200;
    assert!(pcurve_basis_is_valid(
        &PcurveGeometry::SphericalGreatCircle {
            azimuth_origin: 0.0,
            azimuth_rate: tiny,
            plane_phase: 0.0,
            plane_slope: 0.0,
        }
    ));

    let axis = Vector3::new(0.0, 0.0, 1.0);
    let ref_direction = Vector3::new(1.0, 0.0, 0.0);
    assert!(valid_surface_basis(&SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis,
        ref_direction,
        radius: tiny,
    }));
    assert!(valid_surface_basis(&SurfaceGeometry::Torus {
        center: Point3::new(0.0, 0.0, 0.0),
        axis,
        ref_direction,
        major_radius: tiny,
        minor_radius: -tiny,
    }));
}

#[test]
fn tessellation_counts_must_be_consistent() {
    use crate::ids::FaceId;
    use crate::math::Point3;

    let mut ir = unit_cube();
    ir.model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#invalid-counts",
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            vec![[0, 1, 2]],
            TessellationTopology::List,
            TessellationNormals::None,
            Vec::new(),
        )
        .expect("valid tessellation")
        .with_faces(vec![
            FaceId::mint("synthetic:test:face#missing").expect("valid identity")
        ])
        .with_chordal_deflection(Some(-1.0)),
    );
    ir.finalize();
    let report = validate_neutral(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("missing tessellation face")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("invalid tessellation deflection")));
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
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        vec![[0, 1, 2], [1, 3, 2]],
        TessellationTopology::List,
        TessellationNormals::None,
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
    invalid_texture.id = "synthetic:test:tessellation#missing-texture".into();

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
    ir.model.surfaces[0].geometry = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: -1e-200,
    };
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn degenerate_plane_normal_is_flagged() {
    let mut ir = unit_cube();
    if let SurfaceGeometry::Plane { normal, .. } = &mut ir.model.surfaces[0].geometry {
        *normal = Vector3::new(0.0, 0.0, 0.0);
    }
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|f| f.check == Check::Bounds));
}

#[test]
fn topology_tolerance_and_new_conics_are_bounds_checked() {
    let mut ir = unit_cube();
    ir.model.curves.push(Curve {
        id: CurveId::mint("synthetic:test:curve#bad-parabola").expect("valid identity"),
        geometry: CurveGeometry::Parabola {
            vertex: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            focal_distance: 0.0,
        },
        source_object: None,
    });
    ir.model.curves.push(Curve {
        id: CurveId::mint("synthetic:test:curve#bad-hyperbola").expect("valid identity"),
        geometry: CurveGeometry::Hyperbola {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: -1.0,
            minor_radius: 1.0,
        },
        source_object: None,
    });

    let report = validate_neutral(&ir, Vec::new());
    for entity in [
        "synthetic:test:curve#bad-parabola",
        "synthetic:test:curve#bad-hyperbola",
    ] {
        assert!(report
            .findings
            .iter()
            .any(
                |finding| (finding.check == Check::Bounds || finding.check == Check::Tolerances)
                    && finding.entity.as_deref() == Some(entity)
            ));
    }
}

#[test]
fn revolution_rejects_equal_intervals() {
    let mut ir = unit_cube();
    let owner = ir.model.surfaces[0].id.clone();
    ir.model
        .add_procedural_surface(
            owner,
            ProceduralSurface::new(
                ProceduralSurfaceId::mint("synthetic:test:procedural-surface#equal")
                    .expect("valid identity"),
                ProceduralSurfaceDefinition::Revolution {
                    directrix: ir.model.curves[0].id.clone(),
                    axis_origin: Point3::new(0.0, 0.0, 0.0),
                    axis_direction: Vector3::new(0.0, 0.0, 1.0),
                    angular_interval: [1.0, 1.0],
                    angular_parameter_interval: None,
                    parameter_interval: Some([0.0, 1.0]),
                    transposed: false,
                    revision_form: None,
                },
                None,
            ),
        )
        .unwrap();
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("revolution interval")));
}
