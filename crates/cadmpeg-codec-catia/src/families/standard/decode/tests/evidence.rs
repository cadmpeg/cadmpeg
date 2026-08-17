use super::*;

#[test]
fn targeted_face_surface_evidence_follows_an_analytic_offset() {
    let append = |bytes: &mut Vec<u8>, class, object_id: u32, payload: &[u8]| {
        bytes.extend_from_slice(&[
            0xb5,
            0x03,
            class,
            u8::try_from(payload.len()).expect("small payload"),
        ]);
        bytes.extend_from_slice(&object_id.to_le_bytes());
        bytes.extend_from_slice(payload);
    };
    let plane_payload = |origin_z: f64| {
        let mut payload = vec![0; 121];
        payload[0] = 0x80;
        for (offset, value) in [
            (17usize, origin_z),
            (25, 1.0),
            (57, 1.0),
            (73, 1.0),
            (81, 1.0),
            (89, -1.0),
            (97, 1.0),
            (105, -1.0),
            (113, 1.0),
        ] {
            payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        payload
    };
    let mut stream = Vec::new();
    append(&mut stream, 0x27, 2, &plane_payload(0.0));
    append(&mut stream, 0x27, 3, &plane_payload(0.5));
    let mut offset = vec![0x82, 0x82, 0x83];
    offset.extend_from_slice(&(-0.5f64).to_le_bytes());
    offset.push(0x15);
    for value in [-2.0f64, 3.0, -4.0, 5.0] {
        offset.extend_from_slice(&value.to_le_bytes());
    }
    append(&mut stream, 0x30, 9, &offset);
    append(&mut stream, 0x5f, 10, &[0x82, 0x89, 0x8b, 0x05]);

    let evidence = standard_object_evidence_from_streams(
        [stream.clone(), stream.clone()],
        &HashSet::from([10]),
        &HashSet::new(),
    );
    assert!(matches!(
        evidence.surface_geometries.get(&10),
        Some(SurfaceGeometry::Plane { origin, .. }) if *origin == Point3::new(0.0, 0.0, 0.0)
    ));

    let mut conflicting = stream.clone();
    let face_payload = conflicting.len() - 4;
    conflicting[face_payload + 1] = 0x8d;
    let evidence = standard_object_evidence_from_streams(
        [stream, conflicting],
        &HashSet::from([10]),
        &HashSet::new(),
    );
    assert!(!evidence.surface_geometries.contains_key(&10));
}

#[test]
fn targeted_surface_evidence_retains_revolution_construction() {
    let angular_range = [0.0, std::f64::consts::PI];
    let graph = B5Graph {
        complete: true,
        faces: Vec::new(),
        face_records: BTreeMap::new(),
        loops: BTreeMap::new(),
        pcurves: BTreeMap::new(),
        opaque_pcurves: BTreeMap::new(),
        implicit_pcurves: BTreeMap::new(),
        surfaces: BTreeMap::from([(
            10,
            B5Surface::Revolution {
                profile_curve: 11,
                axis_origin: [0.0, 0.0, 0.0],
                reference_x: [1.0, 0.0, 0.0],
                reference_y: [0.0, 1.0, 0.0],
                axis_direction: [0.0, 0.0, 1.0],
                profile_range: [-1.0, 1.0],
                angular_range,
                angular_scale: 1.0,
            },
        )]),
        surface_aliases: BTreeMap::new(),
        offset_surfaces: BTreeMap::new(),
        extrusion_surfaces: BTreeMap::new(),
        supported_surfaces: BTreeMap::new(),
        parameter_incidences: BTreeMap::new(),
        edges: BTreeMap::new(),
        vertex_incidence_links: BTreeMap::new(),
        vertex_points: Vec::new(),
        logical_vertex_points: Vec::new(),
        logical_vertex_refs: Vec::new(),
        edge_vertices: BTreeMap::new(),
        edge_parameter_incidences: BTreeMap::new(),
        vertex_tolerances: BTreeMap::new(),
        profiles: BTreeMap::from([(
            11,
            B5Profile::Line {
                point: [2.0, 0.0, 0.0],
                direction: [0.0, 0.0, 1.0],
                parameter_range: [-1.0, 1.0],
            },
        )]),
    };

    let evidence = standard_surface_evidence(&graph, 10).expect("revolution evidence");
    let Some(StandardSurfaceProcedure::Revolution(revolution)) = evidence.procedure.as_ref() else {
        panic!("surface-of-revolution evidence must retain its construction");
    };
    assert!(matches!(evidence.geometry, Some(SurfaceGeometry::Nurbs(_))));
    assert_eq!(revolution.angular_interval, angular_range);
    assert_eq!(revolution.parameter_interval, [-1.0, 1.0]);
    assert_eq!(revolution.directrix.control_points.len(), 2);
}

#[test]
fn object_evidence_exports_revolution_cache_and_construction() {
    let mut stream = b5_closed_triangle_stream();
    let mut profile = vec![0; 73];
    profile[0] = 0x80;
    for (offset, value) in [
        (1usize, 2.0f64),
        (9, 0.0),
        (17, 0.0),
        (25, 0.0),
        (33, 0.0),
        (41, 1.0),
    ] {
        profile[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    profile[49..57].copy_from_slice(&le_f64(1.0));
    profile[57..65].copy_from_slice(&le_f64(-1.0));
    profile[65..73].copy_from_slice(&le_f64(1.0));
    append_b5_record(&mut stream, 0x0e, 110, &profile);

    let mut revolution = vec![0; 176];
    revolution[0] = 0x81;
    revolution[1] = 0x38;
    revolution[2..5].copy_from_slice(&[110, 0, 0]);
    revolution[29..37].copy_from_slice(&le_f64(1.0));
    revolution[61..69].copy_from_slice(&le_f64(1.0));
    revolution[93..101].copy_from_slice(&le_f64(1.0));
    for (offset, value) in [
        (101usize, 0.0f64),
        (109, std::f64::consts::PI),
        (117, -1.0),
        (125, 1.0),
        (135, 1.0),
        (143, 1.0),
        (151, 1.0),
        (159, 0.0),
        (168, std::f64::consts::PI),
    ] {
        revolution[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    revolution[133..135].copy_from_slice(&[0x05, 0x05]);
    revolution[167] = 0x01;
    append_b5_record(&mut stream, 0x2d, 120, &revolution);

    let evidence =
        standard_object_evidence_from_streams([stream], &HashSet::from([120]), &HashSet::new());
    assert!(matches!(
        evidence.surface_geometries.get(&120),
        Some(SurfaceGeometry::Nurbs(_))
    ));
    let Some(StandardSurfaceProcedure::Revolution(revolution)) =
        evidence.procedural_surfaces.get(&120)
    else {
        panic!("object evidence must retain revolution construction");
    };
    assert_eq!(revolution.angular_interval, [0.0, std::f64::consts::PI]);
    assert_eq!(revolution.parameter_interval, [-1.0, 1.0]);
    assert_eq!(revolution.directrix.control_points.len(), 2);
}

#[test]
fn analytic_surface_uv_accepts_finite_nonzero_carrier_scales() {
    let tiny = 1e-200;
    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
        ratio: tiny,
        half_angle: std::f64::consts::FRAC_PI_6,
    };
    let cone_point = surface_point(&cone, 0.5, 1.0).expect("cone point");
    let cone_uv = analytic_surface_uv(&cone, cone_point).expect("cone parameters");
    assert!((cone_uv.u - 0.5).abs() < 1e-12);
    assert_eq!(cone_uv.v, 1.0);

    let sphere = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: -tiny,
    };
    let sphere_point = surface_point(&sphere, 0.5, 0.25).expect("sphere point");
    let sphere_uv = analytic_surface_uv(&sphere, sphere_point).expect("sphere parameters");
    assert!(sphere_uv.u.is_finite());
    assert!((sphere_uv.v - 0.25).abs() < 1e-12);

    let signed_sphere = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: -2.0,
    };
    let signed_sphere_point =
        surface_point(&signed_sphere, 0.5, 0.25).expect("signed sphere point");
    assert!(point_on_surface(signed_sphere_point, &signed_sphere));

    let torus = SurfaceGeometry::Torus {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 5.0,
        minor_radius: -2.0,
    };
    let torus_point = surface_point(&torus, 0.5, 0.25).expect("torus point");
    assert!(point_on_surface(torus_point, &torus));
}

#[test]
fn mesh_retry_runs_only_after_exact_rejection() {
    use crate::solve::incidence::IncidenceRejection;
    use crate::solve::mesh_quotient::{
        MeshCandidateAmbiguity, MeshCandidateExhaustion, MeshCandidateRejection,
        MeshCandidateSolve, MeshEndpointIncidenceRejection,
    };

    let called = Cell::new(false);
    let outcome = retry_rejected_mesh_solution(
        MeshCandidateSolve::Exhausted(MeshCandidateExhaustion::IncidenceEnumeration),
        || {
            called.set(true);
            MeshCandidateSolve::Rejected(MeshCandidateRejection::EndpointIncidence(
                MeshEndpointIncidenceRejection::NoAssignment(
                    IncidenceRejection::ComponentComposition,
                ),
            ))
        },
    );
    assert!(matches!(
        outcome,
        MeshCandidateSolve::Exhausted(MeshCandidateExhaustion::IncidenceEnumeration)
    ));
    assert!(!called.get());

    let outcome = retry_rejected_mesh_solution(
        MeshCandidateSolve::Exhausted(MeshCandidateExhaustion::PreferredSolutionSearch),
        || {
            called.set(true);
            MeshCandidateSolve::Rejected(MeshCandidateRejection::InputStructure)
        },
    );
    assert!(matches!(
        outcome,
        MeshCandidateSolve::Rejected(MeshCandidateRejection::InputStructure)
    ));
    assert!(called.get());

    let outcome = retry_rejected_mesh_solution(
        MeshCandidateSolve::Rejected(MeshCandidateRejection::InputStructure),
        || {
            called.set(true);
            MeshCandidateSolve::Ambiguous(MeshCandidateAmbiguity::EndpointResolution)
        },
    );
    assert!(matches!(
        outcome,
        MeshCandidateSolve::Ambiguous(MeshCandidateAmbiguity::EndpointResolution)
    ));
    assert!(called.get());
}

#[test]
fn non_collinear_circle_endpoints_determine_the_carrier_plane() {
    let axis = circle_axis_from_endpoints(
        Point3::new(1.0, 2.0, 3.0),
        2.0,
        Point3::new(3.0, 2.0, 3.0),
        Point3::new(1.0, 4.0, 3.0),
    )
    .expect("non-collinear radii determine an axis");
    assert_eq!(axis, Vector3::new(0.0, 0.0, 1.0));
    assert!(circle_axis_from_endpoints(
        Point3::new(1.0, 2.0, 3.0),
        2.0,
        Point3::new(3.0, 2.0, 3.0),
        Point3::new(-1.0, 2.0, 3.0),
    )
    .is_none());
}

#[test]
fn circular_face_intervals_allow_seams_but_reject_crossing_boundaries() {
    let tau = std::f64::consts::TAU;
    assert!(circular_ranges_are_nonoverlapping_or_coincident(&[
        [0.0, 1.0],
        [1.0, 3.0],
        [3.0, tau],
    ]));
    assert!(circular_ranges_are_nonoverlapping_or_coincident(&[
        [0.0, std::f64::consts::PI],
        [0.0, std::f64::consts::PI],
        [std::f64::consts::PI, tau],
    ]));
    assert!(!circular_ranges_are_nonoverlapping_or_coincident(&[
        [0.0, 4.0],
        [2.0, 5.0],
    ]));
    assert!(circular_ranges_are_nonoverlapping_or_coincident(&[
        [5.0, 7.0],
        [7.0 - tau, 5.0],
    ]));
}

#[test]
fn standard_line_pair_preference_rejects_partial_collinear_overlap() {
    let points = [0.0, 1.0, 2.0, 3.0]
        .into_iter()
        .enumerate()
        .map(|(index, x)| Point {
            id: PointId(format!("p{index}")),
            position: Point3::new(x, 0.0, 0.0),
            source_object: None,
        })
        .collect::<Vec<_>>();
    let supports = (0..3)
        .map(|tag| StandardCurveSupport {
            pos: tag,
            tag: tag as u32,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Line,
        })
        .collect::<Vec<_>>();
    let options = vec![vec![[0, 1], [0, 2]]; 3];
    let simple = [Some([0, 1]), Some([1, 2]), Some([2, 3])];
    let overlapping = [Some([0, 2]), Some([2, 3]), Some([1, 3])];

    assert!(standard_line_pair_solution_is_simple(
        &points, &supports, &options, &simple,
    ));
    assert!(!standard_line_pair_solution_is_simple(
        &points,
        &supports,
        &options,
        &overlapping,
    ));
}

#[test]
fn standard_plane_normals_require_signed_face_frame_vectors() {
    let plane = |target| {
        StandardSurfaceRecord::Analytic(SurfacePrefix {
            pos: 0,
            target,
            kind: 0x32,
        })
    };
    let records = vec![plane(10), plane(20), plane(30)];

    assert!(standard_plane_normals_from_face_frames(&records, &[None, None, None]).is_empty());
    assert_eq!(
        standard_plane_normals_from_face_frames(
            &records,
            &[Some([0.0, 0.0, 1.0]), None, Some([0.0, 0.0, -1.0])],
        ),
        HashMap::from([(10, [0.0, 0.0, 1.0]), (30, [0.0, 0.0, -1.0])]),
    );

    let conflicting = vec![plane(10), plane(10)];
    assert!(standard_plane_normals_from_face_frames(
        &conflicting,
        &[Some([0.0, 0.0, 1.0]), Some([0.0, 0.0, -1.0])],
    )
    .is_empty());
}

#[test]
fn standard_planar_spline_edge_solves_line_and_retains_intersection_construction() {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    for (index, position) in [Point3::new(1.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)]
        .into_iter()
        .enumerate()
    {
        ir.model.points.push(Point {
            id: PointId(format!("p{index}")),
            position,
            source_object: None,
        });
    }
    for index in 0..2 {
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("surface-{index}")),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: if index == 0 {
                    Vector3::new(0.0, 0.0, 1.0)
                } else {
                    Vector3::new(0.0, 1.0, 0.0)
                },
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
    }
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let (id, range) = build_standard_edge_curve(
        &mut ir,
        &mut annotations,
        &[
            (SurfaceId("surface-0".to_string()), false, 0),
            (SurfaceId("surface-1".to_string()), false, 1),
        ],
        &HashMap::from([
            (SurfaceId("surface-0".to_string()), 0),
            (SurfaceId("surface-1".to_string()), 1),
        ]),
        &[],
        &support,
        [0, 1],
        None,
        None,
    );
    let id = id.expect("spline support identifies a curve carrier");
    assert_eq!(range, Some([0.0, 3.0]));
    assert_eq!(ir.model.curves[0].id, id);
    assert_eq!(
        ir.model.curves[0].geometry,
        CurveGeometry::Line {
            origin: Point3::new(1.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        }
    );
    assert!(matches!(
        ir.model.procedural_curves.as_slice(),
        [ProceduralCurve {
            curve,
            definition: ProceduralCurveDefinition::Intersection { context, .. },
            ..
        }] if curve == &id
            && context.sides[0].surface.as_ref().is_some_and(|id| id.0 == "surface-0")
            && context.sides[1].surface.as_ref().is_some_and(|id| id.0 == "surface-1")
            && context.parameter_range == [0.0, 3.0]
    ));
}

#[test]
fn standard_spline_uses_identity_bound_native_support_pcurves() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.points.extend(
        [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]
            .into_iter()
            .enumerate()
            .map(|(index, position)| Point {
                id: PointId(format!("point-{index}")),
                position,
                source_object: None,
            }),
    );
    let support = StandardCurveSupport {
        pos: 12,
        tag: 40,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    };
    let pcurve = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    let native = StandardEdgeSupport {
        surface_object_ids: [20, 21],
        carriers: [
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
                SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
            ),
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
                SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 1.0, 0.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
            ),
        ],
        pcurves: [pcurve.clone(), pcurve],
        parameter_range: [2.0, 5.0],
    };
    let (curve, range) = build_standard_edge_curve(
        &mut ir,
        &mut AnnotationBuilder::new(),
        &[],
        &HashMap::new(),
        &[],
        &support,
        [0, 1],
        Some(&native),
        None,
    );
    let curve = curve.expect("native support identifies the curve");
    assert_eq!(range, Some([2.0, 5.0]));
    assert_eq!(ir.model.surfaces.len(), 2);
    assert!(matches!(
        ir.model.procedural_curves.as_slice(),
        [ProceduralCurve {
            curve: bound_curve,
            definition: ProceduralCurveDefinition::Intersection { context, .. },
            ..
        }] if bound_curve == &curve
            && context.parameter_range == [2.0, 5.0]
            && context.sides.iter().all(|side| side.pcurve.is_some())
    ));
}

#[test]
fn native_support_pcurves_bind_standard_edge_endpoints() {
    let mut points = [Point3::new(1.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)]
        .into_iter()
        .enumerate()
        .map(|(index, position)| Point {
            id: PointId(format!("point-{index}")),
            position,
            source_object: None,
        })
        .collect::<Vec<_>>();
    let pcurve = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    let native = StandardEdgeSupport {
        surface_object_ids: [20, 21],
        carriers: [
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
                SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
            ),
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
                SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 1.0, 0.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
            ),
        ],
        pcurves: [pcurve.clone(), pcurve],
        parameter_range: [1.0, 4.0],
    };

    assert_eq!(
        standard_native_support_endpoint_pair(&native, &points, &[0, 1], None),
        Some([0, 1])
    );
    assert_eq!(
        standard_native_support_endpoint_pair(&native, &points, &[0, 1], Some([0, 2])),
        None
    );

    let mut reversed = native.clone();
    reversed.pcurves[1] = PcurveGeometry::Line {
        origin: Point2::new(5.0, 0.0),
        direction: Point2::new(-1.0, 0.0),
    };
    assert_eq!(
        standard_native_support_endpoint_pair(&reversed, &points, &[0, 1], None),
        Some([0, 1])
    );

    points.push(Point {
        id: PointId("ambiguous-start".to_string()),
        position: Point3::new(1.0, 0.0, 0.0),
        source_object: None,
    });
    assert_eq!(
        standard_native_support_endpoint_pair(&native, &points, &[0, 1, 2], None),
        None
    );

    let mut disagreeing = native.clone();
    disagreeing.pcurves[1] = PcurveGeometry::Line {
        origin: Point2::new(0.0, 1.0),
        direction: Point2::new(1.0, 0.0),
    };
    assert_eq!(
        standard_native_support_endpoint_pair(&disagreeing, &points, &[0, 1], None),
        None
    );
}

#[test]
fn limit_curve_point_binding_rejects_separated_occurrences_with_unequal_residuals() {
    let line_span = |offset: f64| {
        (0..6)
            .map(|index| Point3::new(-1.0 + 0.4 * f64::from(index) + offset, 0.0, 0.0))
            .collect::<Vec<_>>()
    };
    let curve = NurbsCurve {
        degree: 5,
        knots: [vec![0.0; 6], vec![0.5; 6], vec![1.0; 6]].concat(),
        control_points: [line_span(0.0), line_span(1e-3)].concat(),
        weights: None,
        periodic: false,
    };

    assert_eq!(
        standard_limit_curve_point_parameter(&curve, Point3::new(0.0, 0.0, 0.0), 2e-3),
        None
    );
}

#[test]
fn limit_curve_binding_retains_correlated_edge_candidates() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.points.extend(
        [Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)]
            .into_iter()
            .enumerate()
            .map(|(index, position)| Point {
                id: PointId(format!("point-{index}")),
                position,
                source_object: None,
            }),
    );
    let surface_id = SurfaceId("surface".to_string());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let support = StandardCurveSupport {
        pos: 10,
        tag: 20,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    };
    let limit_curve = NurbsCurve {
        degree: 5,
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        control_points: (0..6)
            .map(|index| Point3::new(-1.0 + 0.8 * f64::from(index), 0.0, 0.0))
            .collect(),
        weights: None,
        periodic: false,
    };
    let bindings = [(surface_id.clone(), false, 0)];
    let surface_indices = HashMap::from([(surface_id, 0)]);

    let limit_bindings = standard_limit_curve_bindings(
        &ir,
        &bindings,
        &surface_indices,
        std::slice::from_ref(&support),
        std::slice::from_ref(&limit_curve),
    );
    let [limit_candidates] = limit_bindings.as_slice() else {
        panic!("one edge limit-curve domain");
    };
    let [binding] = limit_candidates.as_slice() else {
        panic!("one limit-curve candidate");
    };
    assert_eq!((binding.curve, binding.points), (0, [0, 1]));
    assert!((binding.parameter_range[0] - 0.25).abs() <= 1e-6);
    assert!((binding.parameter_range[1] - 0.75).abs() <= 1e-6);
    let (curve, range) = build_standard_edge_curve(
        &mut ir,
        &mut AnnotationBuilder::new(),
        &bindings,
        &surface_indices,
        &[],
        &support,
        [0, 1],
        None,
        Some((&limit_curve, binding.parameter_range)),
    );
    assert_eq!(range, Some(binding.parameter_range));
    assert!(matches!(
        curve
            .and_then(|id| ir.model.curves.iter().find(|curve| curve.id == id))
            .map(|curve| &curve.geometry),
        Some(CurveGeometry::Nurbs(curve)) if curve == &limit_curve
    ));
    let duplicated = standard_limit_curve_bindings(
        &ir,
        &bindings,
        &surface_indices,
        &[support.clone(), support],
        &[limit_curve],
    );
    assert_eq!(duplicated, vec![vec![*binding], vec![*binding]]);
    let reversed = resolve_standard_limit_curve_binding(limit_candidates, [1, 0])
        .expect("the solved endpoint pair selects the limit curve");
    assert_eq!(reversed.points, [1, 0]);
    assert_eq!(
        reversed.parameter_range,
        [binding.parameter_range[1], binding.parameter_range[0]]
    );
    assert_eq!(
        resolve_standard_limit_curve_binding(&[*binding, *binding], [0, 1]),
        None
    );
}

#[test]
fn standard_spline_retains_a_procedural_rolling_ball_support() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.points.extend(
        [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]
            .into_iter()
            .enumerate()
            .map(|(index, position)| Point {
                id: PointId(format!("point-{index}")),
                position,
                source_object: None,
            }),
    );
    let support = StandardCurveSupport {
        pos: 12,
        tag: 40,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    };
    let pcurve = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    let plane =
        crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        });
    let rolling_ball_definition = ProceduralSurfaceDefinition::RollingBallJet {
        degree: 5,
        knots: vec![0.0],
        multiplicities: vec![6],
        sites: vec![RollingBallJetSite {
            first_limit: Point3::new(0.0, 0.0, 0.0),
            second_limit: Point3::new(0.0, 1.0, 0.0),
            center: Point3::new(0.0, 0.5, 0.0),
            angle: std::f64::consts::PI,
            first_derivative: RollingBallJetDerivative {
                first_limit: Vector3::new(0.0, 0.0, 0.0),
                second_limit: Vector3::new(0.0, 0.0, 0.0),
                center: Vector3::new(0.0, 0.0, 0.0),
                angle: 0.0,
            },
            second_derivative: RollingBallJetDerivative {
                first_limit: Vector3::new(0.0, 0.0, 0.0),
                second_limit: Vector3::new(0.0, 0.0, 0.0),
                center: Vector3::new(0.0, 0.0, 0.0),
                angle: 0.0,
            },
        }],
    };
    let native = StandardEdgeSupport {
        surface_object_ids: [20, 21],
        carriers: [
            plane,
            crate::families::b5::transfer::ResolvedPcurveSurface::RollingBall {
                carrier_object_id: 22,
                definition: Box::new(rolling_ball_definition.clone()),
            },
        ],
        pcurves: [pcurve.clone(), pcurve],
        parameter_range: [2.0, 5.0],
    };
    let (curve, _) = build_standard_edge_curve(
        &mut ir,
        &mut AnnotationBuilder::new(),
        &[],
        &HashMap::new(),
        &[],
        &support,
        [0, 1],
        Some(&native),
        None,
    );
    let curve = curve.expect("procedural support identifies the curve");
    assert_eq!(ir.model.surfaces.len(), 2);
    assert!(matches!(
        ir.model.procedural_surfaces.as_slice(),
        [ProceduralSurface {
            surface,
            definition,
            ..
        }] if surface.0 == "catia:standard:edge-support-surface#21"
            && definition == &rolling_ball_definition
    ));
    assert!(matches!(
        ir.model.procedural_curves.as_slice(),
        [ProceduralCurve { curve: bound, .. }] if bound == &curve
    ));
}

#[test]
fn same_surface_spline_requires_an_exact_ruled_surface_generator() {
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    };
    let solve = |geometry, points: [Point3; 2]| {
        let mut ir = CadIr::empty(Units::default());
        ir.model.surfaces.push(Surface {
            id: SurfaceId("surface".to_string()),
            geometry,
            source_object: None,
        });
        ir.model.points.extend(
            points
                .into_iter()
                .enumerate()
                .map(|(index, position)| Point {
                    id: PointId(format!("point-{index}")),
                    position,
                    source_object: None,
                }),
        );
        standard_spline_line(
            &ir,
            &[(SurfaceId("surface".to_string()), false, 0)],
            &HashMap::from([(SurfaceId("surface".to_string()), 0)]),
            &support,
            [0, 1],
        )
    };
    let cylinder = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    assert!(solve(
        cylinder.clone(),
        [Point3::new(2.0, 0.0, -3.0), Point3::new(2.0, 0.0, 4.0)]
    )
    .is_some());
    assert!(solve(
        cylinder.clone(),
        [Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1e-200)]
    )
    .is_some());
    assert!(solve(
        cylinder,
        [Point3::new(2.0, 0.0, 0.0), Point3::new(0.0, 2.0, 0.0)]
    )
    .is_none());

    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(2.0, 0.0, 2.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    };
    assert!(solve(
        cone.clone(),
        [Point3::new(3.0, 0.0, 1.0), Point3::new(5.0, 0.0, 3.0)]
    )
    .is_some());
    assert!(solve(
        cone,
        [Point3::new(3.0, 0.0, 1.0), Point3::new(2.0, 2.0, 2.0)]
    )
    .is_none());
}

#[test]
fn standard_line_edge_uses_distance_parameterization() {
    let mut ir = CadIr::empty(Units::default());
    for (index, position) in [Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 6.0, 3.0)]
        .into_iter()
        .enumerate()
    {
        ir.model.points.push(Point {
            id: PointId(format!("p{index}")),
            position,
            source_object: None,
        });
    }
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Line,
    };
    let (_, range) = build_standard_edge_curve(
        &mut ir,
        &mut AnnotationBuilder::new(),
        &[],
        &HashMap::new(),
        &[],
        &support,
        [0, 1],
        None,
        None,
    );
    assert_eq!(range, Some([0.0, 5.0]));
}

#[test]
fn standard_line_edge_accepts_a_finite_nonzero_distance() {
    let mut ir = CadIr::empty(Units::default());
    for (index, position) in [Point3::new(0.0, 0.0, 0.0), Point3::new(1e-200, 0.0, 0.0)]
        .into_iter()
        .enumerate()
    {
        ir.model.points.push(Point {
            id: PointId(format!("p{index}")),
            position,
            source_object: None,
        });
    }
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Line,
    };
    let (curve, range) = build_standard_edge_curve(
        &mut ir,
        &mut AnnotationBuilder::new(),
        &[],
        &HashMap::new(),
        &[],
        &support,
        [0, 1],
        None,
        None,
    );
    assert!(curve.is_some());
    assert_eq!(range, Some([0.0, 1e-200]));
}

#[test]
fn witnessed_cylinder_circle_edge_uses_complementary_angular_range() {
    let mut ir = CadIr::empty(Units::default());
    let surface_id = SurfaceId("cylinder".to_string());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        },
        source_object: None,
    });
    let bindings = [(surface_id.clone(), true, 0), (surface_id.clone(), true, 0)];
    let indices = [(surface_id, 0)].into_iter().collect();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 3.0),
            radius: 2.0,
        },
    };
    let mut brep = vec![0; 39];
    brep[..3].copy_from_slice(&[0x00, 0x33, 0x33]);
    brep[27..31].copy_from_slice(&(-2.0f32).to_le_bytes());
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = cadmpeg_ir::geometry::derive_reference_direction(axis);
    let range = standard_circle_param_range(
        &ir,
        &bindings,
        &indices,
        &brep,
        &support,
        Point3::new(0.0, 0.0, 3.0),
        2.0,
        axis,
        reference,
        Point3::new(2.0, 0.0, 3.0),
        Point3::new(0.0, 2.0, 3.0),
    )
    .expect("witnessed circle range");
    assert!(((range[1] - range[0]).abs() - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-12);
}

#[test]
fn native_support_pcurve_midpoint_selects_an_unwitnessed_circle_branch() {
    let cylinder = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
    };
    let pcurve = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    let native = StandardEdgeSupport {
        surface_object_ids: [20, 21],
        carriers: [
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(cylinder.clone()),
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(cylinder),
        ],
        pcurves: [pcurve.clone(), pcurve],
        parameter_range: [0.0, 1.5 * std::f64::consts::PI],
    };
    let start = Point3::new(1.0, 0.0, 0.0);
    let end = Point3::new(0.0, -1.0, 0.0);
    assert_eq!(
        native_support_circle_param_range(
            &native,
            Point3::new(0.0, 0.0, 0.0),
            1.0,
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            start,
            end,
        ),
        Some([0.0, 1.5 * std::f64::consts::PI])
    );
    let mut disagreeing = native.clone();
    disagreeing.pcurves[1] = PcurveGeometry::Line {
        origin: Point2::new(0.0, 1.0),
        direction: Point2::new(1.0, 0.0),
    };
    assert!(native_support_circle_param_range(
        &disagreeing,
        Point3::new(0.0, 0.0, 0.0),
        1.0,
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        start,
        end,
    )
    .is_none());
    assert!(native_support_circle_param_range(
        &native,
        Point3::new(0.0, 0.0, 0.0),
        1.0,
        Vector3::new(0.0, 0.0, -1.0),
        Vector3::new(1.0, 0.0, 0.0),
        start,
        end,
    )
    .is_none());

    let mut ir = CadIr::empty(Units::default());
    for (index, position) in [start, end].into_iter().enumerate() {
        ir.model.points.push(Point {
            id: PointId(format!("p{index}")),
            position,
            source_object: None,
        });
    }
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            radius: 1.0,
        },
    };
    let (_, range) = build_standard_edge_curve(
        &mut ir,
        &mut AnnotationBuilder::new(),
        &[],
        &HashMap::new(),
        &[],
        &support,
        [0, 1],
        Some(&native),
        None,
    );
    assert_eq!(range, Some([0.0, 1.5 * std::f64::consts::PI]));
}

#[test]
fn standard_unbound_vertices_receive_one_free_vertex_owner() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.vertices.push(Vertex {
        id: VertexId("v".to_string()),
        point: PointId("p".to_string()),
        tolerance: None,
    });
    let mut annotations = AnnotationBuilder::new();
    attach_free_vertices(
        &mut ir,
        &mut annotations,
        "standard",
        "MainDataStream+SurfacicReps",
    );
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.regions.len(), 1);
    assert_eq!(ir.model.shells.len(), 1);
    assert_eq!(
        ir.model.shells[0].free_vertices,
        [VertexId("v".to_string())]
    );
}

#[test]
fn standard_spline_retains_complete_surface_incidence_pair_domain() {
    let mut ir = CadIr::empty(Units::default());
    for index in 0..138 {
        ir.model.points.push(Point {
            id: PointId(format!("p{index}")),
            position: Point3::new(index as f64, 0.0, 0.0),
            source_object: None,
        });
    }
    for index in 0..2 {
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("s{index}")),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });
    }
    let bindings = [
        (SurfaceId("s0".to_string()), true, 0),
        (SurfaceId("s1".to_string()), true, 0),
    ];
    let indices = [
        (SurfaceId("s0".to_string()), 0),
        (SurfaceId("s1".to_string()), 1),
    ]
    .into_iter()
    .collect();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let choices = resolve_standard_endpoint_pairs(
        &ir,
        &bindings,
        &indices,
        &[support],
        &[(0..138).collect()],
    )
    .expect("endpoint option pass");
    assert_eq!(choices[0].len(), 9_453);
    assert_eq!(choices[0].first(), Some(&[0, 1]));
    assert_eq!(choices[0].last(), Some(&[136, 137]));
}

#[test]
fn standard_planar_intersection_spline_uses_the_common_line_domain() {
    let mut ir = CadIr::empty(Units::default());
    for (index, position) in [
        Point3::new(-2.0, 0.0, 0.0),
        Point3::new(3.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, 0.0, 1.0),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.points.push(Point {
            id: PointId(format!("p{index}")),
            position,
            source_object: None,
        });
    }
    for (index, normal) in [Vector3::new(0.0, 0.0, 1.0), Vector3::new(0.0, 1.0, 0.0)]
        .into_iter()
        .enumerate()
    {
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("s{index}")),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal,
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
    }
    let bindings = [
        (SurfaceId("s0".to_string()), true, 0),
        (SurfaceId("s1".to_string()), true, 0),
    ];
    let indices = [
        (SurfaceId("s0".to_string()), 0),
        (SurfaceId("s1".to_string()), 1),
    ]
    .into_iter()
    .collect();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };

    let choices =
        resolve_standard_endpoint_pairs(&ir, &bindings, &indices, &[support], &[vec![0, 1, 2, 3]])
            .expect("endpoint option pass");

    assert_eq!(choices, [vec![[0, 1]]]);
}

#[test]
fn standard_antipodal_circle_candidates_admit_full_circle_seams() {
    let mut ir = CadIr::empty(Units::default());
    for (index, position) in [Point3::new(5.0, 0.0, 0.0), Point3::new(-5.0, 0.0, 0.0)]
        .into_iter()
        .enumerate()
    {
        ir.model.points.push(Point {
            id: PointId(format!("p{index}")),
            position,
            source_object: None,
        });
    }
    for index in 0..2 {
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("s{index}")),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
    }
    let bindings = [
        (SurfaceId("s0".to_string()), true, 0),
        (SurfaceId("s1".to_string()), true, 0),
    ];
    let indices = [
        (SurfaceId("s0".to_string()), 0),
        (SurfaceId("s1".to_string()), 1),
    ]
    .into_iter()
    .collect();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            radius: 5.0,
        },
    };

    let choices =
        resolve_standard_endpoint_pairs(&ir, &bindings, &indices, &[support], &[vec![0, 1]])
            .expect("endpoint option pass");

    assert_eq!(choices, [vec![[0, 0], [0, 1], [1, 1]]]);
}

#[test]
fn standard_parallel_line_rows_retain_mesh_resolvable_domains() {
    let mut ir = CadIr::empty(Units::default());
    for (index, position) in [
        Point3::new(-2.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(-2.0, 0.0, 1.0),
        Point3::new(2.0, 0.0, 1.0),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.points.push(Point {
            id: PointId(format!("p{index}")),
            position,
            source_object: None,
        });
    }
    for index in 0..2 {
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("s{index}")),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        });
    }
    let bindings = [
        (SurfaceId("s0".to_string()), true, 0),
        (SurfaceId("s1".to_string()), true, 0),
    ];
    let indices = [
        (SurfaceId("s0".to_string()), 0),
        (SurfaceId("s1".to_string()), 1),
    ]
    .into_iter()
    .collect();
    let supports = [0, 1].map(|index| StandardCurveSupport {
        pos: index,
        tag: index as u32,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Line,
    });

    let choices = resolve_standard_endpoint_pairs(
        &ir,
        &bindings,
        &indices,
        &supports,
        &[vec![0, 1, 2, 3], vec![0, 1, 2, 3]],
    )
    .expect("endpoint option pass");

    assert_eq!(choices, [vec![[0, 2], [1, 3]], vec![[0, 2], [1, 3]]]);
}

#[test]
fn standard_spline_rows_bind_complete_bipartite_domains_by_allocation_rank() {
    let supports = [10, 11].map(|tag| StandardCurveSupport {
        pos: tag as usize,
        tag,
        faces: [3, 7],
        geometry: StandardCurveGeometry::Bspline,
    });
    let domain = vec![[2, 8], [2, 9], [3, 8], [3, 9]];
    let mut candidates = [domain.clone(), domain];

    bind_ordered_standard_curve_branches(&supports, &mut candidates);

    assert_eq!(candidates, [vec![[2, 8]], vec![[3, 9]]]);
}

#[test]
fn standard_spline_group_binding_does_not_rank_unfocused_groups() {
    let supports = [10, 11, 20, 21].map(|tag| StandardCurveSupport {
        pos: tag as usize,
        tag,
        faces: if tag < 20 { [1, 2] } else { [3, 4] },
        geometry: StandardCurveGeometry::Bspline,
    });
    let unfocused_domain = vec![[0, 1], [0, 2], [3, 1], [3, 2]];
    let focused_domain = vec![[4, 5], [4, 6], [7, 5], [7, 6]];
    let mut candidates = [
        unfocused_domain.clone(),
        unfocused_domain.clone(),
        focused_domain.clone(),
        focused_domain,
    ];

    bind_ordered_standard_curve_branches_for_group(&supports, &mut candidates, &[2, 3]);

    assert_eq!(candidates[0], unfocused_domain);
    assert_eq!(candidates[1], unfocused_domain);
    assert_eq!(candidates[2], vec![[4, 5]]);
    assert_eq!(candidates[3], vec![[7, 6]]);
}

#[test]
fn standard_line_rows_bind_complete_bipartite_domains_by_allocation_rank() {
    let supports = [10, 11].map(|tag| StandardCurveSupport {
        pos: tag as usize,
        tag,
        faces: [3, 7],
        geometry: StandardCurveGeometry::Line,
    });
    let domain = vec![[2, 8], [2, 9], [3, 8], [3, 9]];
    let mut candidates = [domain.clone(), domain];

    bind_ordered_standard_curve_branches(&supports, &mut candidates);

    assert_eq!(candidates, [vec![[2, 8]], vec![[3, 9]]]);
}

#[test]
fn standard_line_rows_keep_complete_relation_before_face_frontiers() {
    let supports = [10, 11].map(|tag| StandardCurveSupport {
        pos: tag as usize,
        tag,
        faces: [3, 7],
        geometry: StandardCurveGeometry::Line,
    });
    let domain = vec![[2, 8], [2, 9], [3, 8], [3, 9]];
    let candidates = [domain.clone(), domain];
    let groups = standard_curve_branch_groups(&supports, &candidates);
    let assignment = [None, None];

    let constrained = standard_curve_branch_candidates_after_partial_assignment(
        &supports,
        &candidates,
        &groups,
        &assignment,
        None,
    )
    .expect("unresolved branch relation remains admissible");

    assert_eq!(constrained, candidates);
}

#[test]
fn standard_branch_ranking_stops_when_its_work_budget_is_exhausted() {
    let supports = [10, 11].map(|tag| StandardCurveSupport {
        pos: tag as usize,
        tag,
        faces: [3, 7],
        geometry: StandardCurveGeometry::Line,
    });
    let domain = vec![[2, 8], [2, 9], [3, 8], [3, 9]];
    let candidates = [domain.clone(), domain];
    let groups = standard_curve_branch_groups(&supports, &candidates);
    let budget = WorkBudget::new(1);

    assert!(standard_curve_branch_candidates_after_partial_assignment(
        &supports,
        &candidates,
        &groups,
        &[None, None],
        Some(&budget),
    )
    .is_none());
    assert!(budget.exhausted());
}

#[test]
fn standard_branch_ranking_defers_candidate_work_until_frontier_completion() {
    let supports = [[0, 1], [0, 1], [1, 2], [1, 2]]
        .into_iter()
        .enumerate()
        .map(|(tag, faces)| StandardCurveSupport {
            pos: tag,
            tag: tag as u32,
            faces,
            geometry: StandardCurveGeometry::Line,
        })
        .collect::<Vec<_>>();
    let first_domain = vec![[0, 2], [0, 3], [1, 2], [1, 3]];
    let second_domain = vec![[4, 6], [4, 7], [5, 6], [5, 7]];
    let candidates = [
        first_domain.clone(),
        first_domain,
        second_domain.clone(),
        second_domain,
    ];
    let groups = standard_curve_branch_groups(&supports, &candidates);
    let budget = WorkBudget::new(8);

    assert_eq!(
        standard_curve_branch_candidates_after_partial_assignment(
            &supports,
            &candidates,
            &groups,
            &[None, None, None, None],
            Some(&budget),
        ),
        Some(candidates.to_vec())
    );
    assert!(!budget.exhausted());
}

#[test]
fn standard_branch_group_binding_ignores_empty_unrelated_domains() {
    let supports = [
        StandardCurveSupport {
            pos: 10,
            tag: 10,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Line,
        },
        StandardCurveSupport {
            pos: 11,
            tag: 11,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Line,
        },
        StandardCurveSupport {
            pos: 12,
            tag: 12,
            faces: [8, 9],
            geometry: StandardCurveGeometry::Line,
        },
    ];
    let domain = vec![[0, 2], [0, 3], [1, 2], [1, 3]];
    let all_candidates = [domain.clone(), domain, Vec::new()];
    let mut group_candidates = all_candidates[..2].to_vec();

    bind_standard_curve_branch_group(
        &supports,
        &mut group_candidates,
        &[0, 1],
        &all_candidates,
        &[None, None, None],
    );

    assert_eq!(group_candidates, [vec![[0, 2]], vec![[1, 3]]]);
}
