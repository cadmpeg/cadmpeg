use crate::families::standard::decode::edge_geometry::build_standard_edge_curve;
use crate::families::standard::decode::edge_geometry::standard_pcurve_geometry as charged_standard_pcurve_geometry;
use crate::families::standard::records::StandardCurveSupport;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::AnnotationBuilder;
use std::collections::HashMap;

#[test]
fn standard_attached_circle_plans_refuse_before_growth() {
    let mut ir = CadIr::empty();
    let surface_id = SurfaceId::mint("catia:test:surface#sphere-0").expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .expect("valid sphere"),
        )),
        source_object: None,
    });
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 0],
        geometry: super::checked_circle(Point3::new(0.0, 0.0, 0.6), 0.8),
    };
    let bindings = [(surface_id, false, 0)];
    crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        crate::families::standard::decode::edge_geometry::attach_standard_circles(
            ctx,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &bindings,
            std::slice::from_ref(&support),
            &mut admission,
        )
    })
    .expect("service circle budget");
    assert_eq!(ir.model.curves.len(), 1);
    ir.model.curves.clear();
    for operation in [
        "catia_standard_bound_face_geometries",
        "catia_standard_attached_circle_plans",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            operation,
            |cap| {
                let mut limited_ir = ir.clone();
                crate::test_support::with_collection_limit(cap, |ctx| {
                    let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
                    crate::families::standard::decode::edge_geometry::attach_standard_circles(
                        ctx,
                        &mut limited_ir,
                        &mut AnnotationBuilder::new(),
                        &bindings,
                        std::slice::from_ref(&support),
                        &mut admission,
                    )
                })
            },
        );
    }
}

#[test]
fn standard_edge_circle_axes_use_fixed_slots_before_curve_growth() {
    let mut ir = CadIr::empty();
    let surface_id = SurfaceId::mint("catia:test:surface#sphere-0").expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .expect("valid sphere"),
        )),
        source_object: None,
    });
    for (index, position) in [Point3::new(0.8, 0.0, 0.6), Point3::new(0.0, 0.8, 0.6)]
        .into_iter()
        .enumerate()
    {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#{index}")).expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(position).expect("finite point"),
            None,
        ));
    }
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 0],
        geometry: super::checked_circle(Point3::new(0.0, 0.0, 0.6), 0.8),
    };
    let bindings = [(surface_id.clone(), false, 0)];
    let indices = HashMap::from([(surface_id, 0)]);
    let mut service_ir = ir.clone();
    let (curve, _) = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            crate::families::standard::decode::edge_geometry::BuildStandardEdgeCurveInputs {
                native_surfaces:
                    &mut crate::families::standard::decode::edge_geometry::NativeSurfaceIndex::new(
                        admission.context(),
                    )
                    .expect("index reservation"),
                ir: &mut service_ir,
                annotations: &mut AnnotationBuilder::new(),
                bindings: &bindings,
                surface_indices: &indices,
                brep: &[],
                support: &support,
                points: [0, 1],
                native_support: None,
                limit_curve: None,
                refusal: &mut crate::nurbs::LaneRefusals::new(),
                admission: &mut admission,
            },
        )
    })
    .expect("service edge circle budget");
    assert!(curve.is_some());
    // The face and native carrier axes occupy fixed slots, so the first
    // collection the circle edge grows is the model curve arena.
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            crate::families::standard::decode::edge_geometry::BuildStandardEdgeCurveInputs {
                native_surfaces:
                    &mut crate::families::standard::decode::edge_geometry::NativeSurfaceIndex::new(
                        admission.context(),
                    )
                    .expect("index reservation"),
                ir: &mut ir,
                annotations: &mut AnnotationBuilder::new(),
                bindings: &bindings,
                surface_indices: &indices,
                brep: &[],
                support: &support,
                points: [0, 1],
                native_support: None,
                limit_curve: None,
                refusal: &mut crate::nurbs::LaneRefusals::new(),
                admission: &mut admission,
            },
        )
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_family_emit_curves"
            && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

fn standard_pcurve_geometry(
    surface: &SurfaceGeometry,
    support: &StandardCurveSupport,
    start: Point3,
    end: Point3,
    witness: Option<cadmpeg_ir::features::FinitePoint3>,
    edge_curve: Option<&CurveGeometry>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<(cadmpeg_ir::geometry::pcurve::PcurveGeometry, [f64; 2])> {
    crate::test_support::with_service_context(|ctx| {
        charged_standard_pcurve_geometry(
            ctx,
            surface,
            support,
            (start, end),
            witness,
            edge_curve,
            refusal,
        )
    })
    .expect("service budget admits standard pcurve")
}

#[test]
fn standard_circle_without_an_admissible_plane_normal_retains_unknown_carrier() {
    let mut ir = CadIr::empty();
    let center = Point3::new(0.0, 2.0, 3.0);
    let radius = 2.0;
    ir.model.points.extend(
        [
            Point3::new(center.x, center.y, center.z - radius),
            Point3::new(center.x, center.y, center.z + radius),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, position)| {
            Point::new(
                PointId::mint(format!("catia:test:point#point-{index}")).expect("identity grammar"),
                cadmpeg_ir::features::FinitePoint3::new(position)
                    .expect("a finite position is a point"),
                None,
            )
        }),
    );
    let sphere_geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            center,
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            radius,
        )
        .expect("valid SphereSurface fixture"),
    ));
    let surface_ids = [
        SurfaceId::mint("catia:test:surface#sphere-0".to_string()).expect("identity grammar"),
        SurfaceId::mint("catia:test:surface#sphere-1".to_string()).expect("identity grammar"),
    ];
    ir.model
        .surfaces
        .extend(surface_ids.iter().cloned().map(|id| Surface {
            id,
            geometry: sphere_geometry.clone(),
            source_object: None,
        }));
    let bindings = [
        (surface_ids[0].clone(), false, 0),
        (surface_ids[1].clone(), false, 1),
    ];
    let surface_indices = HashMap::from([(surface_ids[0].clone(), 0), (surface_ids[1].clone(), 1)]);
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: super::checked_circle(center, radius),
    };

    let (curve, range) = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            crate::families::standard::decode::edge_geometry::BuildStandardEdgeCurveInputs {
                native_surfaces:
                    &mut crate::families::standard::decode::edge_geometry::NativeSurfaceIndex::new(
                        admission.context(),
                    )
                    .expect("index reservation"),
                ir: &mut ir,
                annotations: &mut AnnotationBuilder::new(),
                bindings: &bindings,
                surface_indices: &surface_indices,
                brep: &[],
                support: &support,
                points: [0, 1],
                native_support: None,
                limit_curve: None,
                refusal: &mut crate::nurbs::LaneRefusals::new(),
                admission: &mut admission,
            },
        )
    })
    .expect("valid source object identity");
    let curve = curve.expect("the serialized circle retains a carrier identity");
    assert_eq!(range, None);
    assert!(matches!(
        ir.model
            .curves
            .iter()
            .find(|candidate| candidate.id == curve),
        Some(Curve {
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. }),
            ..
        })
    ));
}

#[test]
fn unknown_standard_circle_carrier_does_not_create_a_sphere_pcurve() {
    let center = Point3::new(0.0, 2.0, 3.0);
    let radius = 2.0;
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            center,
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            radius,
        )
        .expect("valid SphereSurface fixture"),
    ));
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: super::checked_circle(center, radius),
    };
    let unknown = CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None });

    assert!(standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(center.x, center.y, center.z - radius),
        Point3::new(center.x, center.y, center.z + radius),
        None,
        Some(&unknown),
        &mut crate::nurbs::LaneRefusals::new(),
    )
    .is_none());
}

#[test]
fn analytic_membership_preserves_radial_distance_at_large_axial_offsets() {
    use crate::families::standard::decode::edge_geometry::circle_axis_from_carrier;
    use crate::families::standard::decode::point_on_surface_if_supported;
    use cadmpeg_ir::geometry::analytic::{CylinderSurface, SphereSurface};
    let origin = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let x_axis = Vector3::new(1.0, 0.0, 0.0);
    let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(origin, axis, x_axis, 1.0).expect("unit cylinder frame is valid"),
    ));
    for axial in [0.0, 1e8, 1e200] {
        assert_eq!(
            point_on_surface_if_supported(
                &cadmpeg_test_support::service_decode_context(),
                Point3::new(1.0, 0.0, axial),
                &cylinder
            ),
            Ok(Some(true))
        );
        assert_eq!(
            point_on_surface_if_supported(
                &cadmpeg_test_support::service_decode_context(),
                Point3::new(2.0, 0.0, axial),
                &cylinder
            ),
            Ok(Some(false))
        );
    }
    let sphere = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        SphereSurface::try_new(origin, axis, x_axis, 1.0).expect("unit sphere frame is valid"),
    ));
    assert!(circle_axis_from_carrier(Point3::new(1e200, 0.0, 0.0), 1.0, &sphere).is_none());
    assert!(circle_axis_from_carrier(Point3::new(0.0, 0.0, 0.6), 0.8, &sphere).is_some());
}

#[test]
fn sphere_section_axis_preserves_a_subnormal_center_offset() {
    use crate::families::standard::decode::edge_geometry::circle_axis_from_carrier;
    let sphere = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("unit sphere frame is valid"),
    ));
    assert_eq!(
        circle_axis_from_carrier(Point3::new(0.0, 0.0, 1e-310), 1.0, &sphere)
            .map(cadmpeg_ir::math::Vector3::from),
        Some(Vector3::new(0.0, 0.0, 1.0))
    );
}

#[test]
fn analytic_curve_angles_preserve_extreme_radii() {
    use cadmpeg_ir::geometry::{
        analytic::{CircleCurve, EllipseCurve},
        CurveGeometry, SolvedCurveGeometry,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    for radius in [1e-200, 1.0, 1e200] {
        let center = Point3::new(0.0, 0.0, 0.0);
        let axis = Vector3::new(0.0, 0.0, 1.0);
        let reference = Vector3::new(1.0, 0.0, 0.0);
        for geometry in [
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                CircleCurve::try_new(center, axis, reference, radius).expect("valid test circle"),
            )),
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                EllipseCurve::try_new(center, axis, reference, radius, 0.5 * radius)
                    .expect("valid test ellipse"),
            )),
        ] {
            assert_eq!(
                crate::families::standard::decode::edge_geometry::standard_analytic_curve_angle(
                    &geometry,
                    Point3::new(radius, 0.0, 0.0)
                ),
                Some(0.0)
            );
            assert!(
                crate::families::standard::decode::edge_geometry::standard_analytic_curve_angle(
                    &geometry,
                    Point3::new(2.0 * radius, 0.0, 0.0)
                )
                .is_none()
            );
        }
    }
}

#[test]
fn numerical_seventh_short_witnessed_arc_retains_its_sweep() {
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("valid test circle"),
    ));
    let point = |angle: f64| Point3::new(angle.cos(), angle.sin(), 0.0);
    let range =
        crate::families::standard::decode::edge_geometry::standard_analytic_curve_parameter_range;
    let actual = range(&geometry, point(0.0), point(0.001), Some(point(0.0005)))
        .expect("a witnessed short arc has a parameter range");
    assert_eq!(actual[0], 0.0);
    assert!((actual[1] - 0.001).abs() <= 8.0 * f64::EPSILON * 0.001);
    assert_eq!(
        range(&geometry, point(0.0), point(0.0), Some(point(1.0))),
        Some([0.0, std::f64::consts::TAU])
    );
}
