use super::*;

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
        .map(|(index, position)| Point {
            id: PointId::mint(format!("catia:test:point#point-{index}")).expect("identity grammar"),
            position,
            source_object: None,
        }),
    );
    let sphere_geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::SphereSurface::try_new(
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
        cadmpeg_ir::geometry::SphereSurface::try_new(
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
    )
    .is_none());
}
