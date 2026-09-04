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
            id: PointId::mint(format!("point-{index}")).expect("identity grammar"),
            position,
            source_object: None,
        }),
    );
    let sphere_geometry = SurfaceGeometry::Sphere {
        center,
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius,
    };
    let surface_ids = [
        SurfaceId::mint("sphere-0".to_string()).expect("identity grammar"),
        SurfaceId::mint("sphere-1".to_string()).expect("identity grammar"),
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
    );
    let curve = curve.expect("the serialized circle retains a carrier identity");
    assert_eq!(range, None);
    assert!(matches!(
        ir.model
            .curves
            .iter()
            .find(|candidate| candidate.id == curve),
        Some(Curve {
            geometry: CurveGeometry::Unknown { .. },
            ..
        })
    ));
}

#[test]
fn unknown_standard_circle_carrier_does_not_create_a_sphere_pcurve() {
    let center = Point3::new(0.0, 2.0, 3.0);
    let radius = 2.0;
    let surface = SurfaceGeometry::Sphere {
        center,
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius,
    };
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle { center, radius },
    };
    let unknown = CurveGeometry::Unknown { record: None };

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
