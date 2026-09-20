use crate::families::standard::decode::build_standard_edge_curve;
use crate::families::standard::decode::standard_pcurve_geometry;
use crate::families::standard::records::StandardCurveGeometry;
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
                position,
                None,
            )
            .expect("a finite position is a point")
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
        geometry: StandardCurveGeometry::Circle { center, radius },
    };

    let (curve, range) = build_standard_edge_curve(
        &mut ir,
        &mut AnnotationBuilder::new(),
        &bindings,
        &surface_indices,
        &[],
        &support,
        [0, 1],
        None,
        None,
        &mut crate::nurbs::LaneRefusals::new(),
    )
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
        geometry: StandardCurveGeometry::Circle { center, radius },
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
    use crate::families::standard::decode::{
        circle_axis_from_carrier, point_on_surface_if_supported,
    };
    use cadmpeg_ir::geometry::analytic::{CylinderSurface, SphereSurface};
    let origin = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let x_axis = Vector3::new(1.0, 0.0, 0.0);
    let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(origin, axis, x_axis, 1.0).expect("unit cylinder frame is valid"),
    ));
    for axial in [0.0, 1e8, 1e200] {
        assert_eq!(
            point_on_surface_if_supported(Point3::new(1.0, 0.0, axial), &cylinder),
            Some(true)
        );
        assert_eq!(
            point_on_surface_if_supported(Point3::new(2.0, 0.0, axial), &cylinder),
            Some(false)
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
    use crate::families::standard::decode::circle_axis_from_carrier;
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
        circle_axis_from_carrier(Point3::new(0.0, 0.0, 1e-310), 1.0, &sphere),
        Some(Vector3::new(0.0, 0.0, 1.0))
    );
}
