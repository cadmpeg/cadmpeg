// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{
    nurbs_weights_valid, pcurve_basis_is_valid, support_context_is_finite, valid_surface_basis,
};
use crate::examples::unit_cube;
use crate::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, PcurveGeometry,
    ProceduralSurface, ProceduralSurfaceDefinition, SurfaceGeometry,
};
use crate::ids::{CurveId, ProceduralSurfaceId};
use crate::math::{Point2, Point3, Vector3};
use crate::report::Check;
use crate::tessellation::{TessellationChannel, TessellationChannelDomain};
use crate::validate::validate_neutral;

fn context(pcurve: bool, pcurve_parameter_range: Option<[f64; 2]>) -> IntcurveSupportContext {
    IntcurveSupportContext {
        sides: [
            IntcurveSupportSide {
                surface: None,
                pcurve: pcurve.then_some(PcurveGeometry::Line {
                    origin: Point2::new(0.0, 0.0),
                    direction: Point2::new(1.0, 0.0),
                }),
                pcurve_parameter_range,
            },
            IntcurveSupportSide {
                surface: None,
                pcurve: None,
                pcurve_parameter_range: None,
            },
        ],
        parameter_range: [0.0, 1.0],
        discontinuities: std::array::from_fn(|_| Vec::new()),
    }
}

#[test]
fn support_pcurve_mapping_requires_a_finite_nonzero_pcurve_interval() {
    let mapped = context(true, Some([5.0, 2.0]));
    assert!(support_context_is_finite(&mapped));
    assert_eq!(
        mapped.sides[0].pcurve_parameter(mapped.parameter_range, 0.25),
        Some(4.25)
    );
    assert!(!support_context_is_finite(&context(
        false,
        Some([5.0, 2.0])
    )));
    assert!(!support_context_is_finite(&context(true, Some([2.0, 2.0]))));
    assert!(!support_context_is_finite(&context(
        true,
        Some([f64::NAN, 2.0])
    )));
}

#[test]
fn exact_geometry_scalars_require_finite_nonzero_values_without_a_size_floor() {
    let tiny = 1e-200;
    assert!(nurbs_weights_valid(Some(&[tiny, -tiny]), 2));
    assert!(!nurbs_weights_valid(Some(&[tiny, 0.0]), 2));
    assert!(!nurbs_weights_valid(Some(&[tiny, f64::NAN]), 2));

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
    use crate::math::{Point3, Vector3};
    use crate::tessellation::Tessellation;

    let mut ir = unit_cube();
    ir.model.tessellations.push(Tessellation {
        id: "synthetic:test:tessellation#invalid-counts".into(),
        body: None,
        faces: vec![FaceId("synthetic:test:face#missing".into())],
        chordal_deflection: Some(-1.0),
        source_object: None,
        vertices: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: vec![4],
        normals: vec![Vector3::new(0.0, 0.0, 1.0); 2],
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: vec![TessellationChannel {
            domain: TessellationChannelDomain::Corner,
            item_size: 1,
            kind: 0,
            flags: 0,
            count: 1,
            data: vec![0],
            indices: vec![0, 1, 0],
        }],
    });
    ir.model.tessellations.push(Tessellation {
        id: "synthetic:test:tessellation#invalid-strips".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        triangles: vec![[0, 2, 1]],
        feature_edges: Vec::new(),
        strip_lengths: vec![3],
        normals: vec![Vector3::new(0.0, 0.0, 1.0); 3],
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    });
    ir.finalize();
    let report = validate_neutral(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("normals do not match")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("strips do not match")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("triangles do not match strips")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("missing tessellation face")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("invalid tessellation deflection")));
    assert!(report.findings.iter().any(|finding| finding
        .message
        .contains("invalid tessellation channel indices")));
}

#[test]
fn corner_normals_and_feature_edges_have_explicit_domains() {
    use crate::math::{Point3, Vector3};
    use crate::report::{Check, Severity};
    use crate::tessellation::Tessellation;

    let mesh = |id: &str| Tessellation {
        id: id.into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        triangles: vec![[0, 1, 2]],
        feature_edges: vec![[0, 1]],
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        corner_normals: vec![Vector3::new(0.0, 0.0, 1.0); 3],
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    };
    let mut invalid_normals = mesh("synthetic:test:tessellation#invalid-corner-normals");
    invalid_normals.corner_normals.pop();
    let mut invalid_edge = mesh("synthetic:test:tessellation#invalid-feature-edge");
    invalid_edge.feature_edges = vec![[1, 2], [0, 1]];
    let valid = mesh("synthetic:test:tessellation#valid-domains");

    let mut ir = unit_cube();
    ir.model
        .tessellations
        .extend([invalid_normals, invalid_edge, valid]);
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
    assert_eq!(
        errors_for("synthetic:test:tessellation#invalid-corner-normals"),
        1
    );
    assert_eq!(
        errors_for("synthetic:test:tessellation#invalid-feature-edge"),
        1
    );
    assert_eq!(errors_for("synthetic:test:tessellation#valid-domains"), 0);
}

#[test]
fn tessellation_triangle_groups_and_texture_assignments_validate() {
    use crate::assets::{Asset, AssetContent, AssetId};
    use crate::math::Point3;
    use crate::report::{Check, Severity};
    use crate::tessellation::{
        Tessellation, TessellationTextureAssignment, TessellationTriangleGroup,
    };

    let texture = AssetId("synthetic:test:asset#mesh-texture".into());
    let valid = Tessellation {
        id: "synthetic:test:tessellation#valid-groups".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        triangles: vec![[0, 1, 2], [1, 3, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        corner_normals: Vec::new(),
        triangle_groups: vec![
            TessellationTriangleGroup {
                source_id: Some("group-a".into()),
                triangles: vec![0],
            },
            TessellationTriangleGroup {
                source_id: Some("group-b".into()),
                triangles: vec![1],
            },
        ],
        texture_assignments: vec![
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
        ],
        channels: Vec::new(),
    };
    let mut invalid = valid.clone();
    invalid.id = "synthetic:test:tessellation#invalid-groups".into();
    invalid.triangle_groups.push(TessellationTriangleGroup {
        source_id: Some("group-b".into()),
        triangles: vec![0],
    });
    invalid.texture_assignments[0].texture = AssetId("synthetic:test:asset#missing".into());
    let mut duplicate_group_id = valid.clone();
    duplicate_group_id.id = "synthetic:test:tessellation#duplicate-group-id".into();
    duplicate_group_id.triangle_groups[1].source_id = Some("group-a".into());
    let mut duplicate_texture = valid.clone();
    duplicate_texture.id = "synthetic:test:tessellation#duplicate-texture".into();
    duplicate_texture.texture_assignments[1].source_id = Some("texture-resource-a".into());

    let mut ir = unit_cube();
    ir.model.assets.push(Asset {
        id: texture,
        name: None,
        media_type: None,
        content: AssetContent::Embedded { data: vec![0] },
        native_ref: None,
    });
    ir.model
        .tessellations
        .extend([valid, invalid, duplicate_group_id, duplicate_texture]);
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
    assert_eq!(errors_for("synthetic:test:tessellation#invalid-groups"), 2);
    assert_eq!(
        errors_for("synthetic:test:tessellation#duplicate-group-id"),
        1
    );
    assert_eq!(
        errors_for("synthetic:test:tessellation#duplicate-texture"),
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
    let edge_id = ir.model.edges[0].id.0.clone();
    ir.model.edges[0].tolerance = Some(-1.0);
    ir.model.curves.push(Curve {
        id: CurveId("synthetic:test:curve#bad-parabola".into()),
        geometry: CurveGeometry::Parabola {
            vertex: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            focal_distance: 0.0,
        },
        source_object: None,
    });
    ir.model.curves.push(Curve {
        id: CurveId("synthetic:test:curve#bad-hyperbola".into()),
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
        edge_id.as_str(),
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
                ProceduralSurfaceId("synthetic:test:procedural-surface#equal".into()),
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

#[test]
fn document_and_entity_tolerances_are_checked() {
    let mut ir = unit_cube();
    ir.tolerances.angular = f64::NAN;
    ir.model.faces[0].tolerance = Some(0.0);
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::Tolerances));
}
