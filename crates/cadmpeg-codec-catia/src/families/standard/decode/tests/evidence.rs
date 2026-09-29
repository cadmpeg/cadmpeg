use crate::families::b5::graph::B5Graph;
use crate::families::b5::graph::B5Profile;
use crate::families::b5::graph::B5Surface;
use crate::families::standard::decode::attach_free_vertices;
use crate::families::standard::decode::copy_standard_extrusion_definition;
use crate::families::standard::decode::edge_geometry::analytic_surface_uv;
use crate::families::standard::decode::edge_geometry::build_standard_edge_curve;
use crate::families::standard::decode::edge_geometry::circle_axis_from_endpoints;
use crate::families::standard::decode::edge_geometry::circular_range_choices_have_simple_selection;
use crate::families::standard::decode::edge_geometry::circular_ranges_are_nonoverlapping_or_coincident;
use crate::families::standard::decode::edge_geometry::native_support_circle_param_range;
use crate::families::standard::decode::edge_geometry::point_on_surface;
use crate::families::standard::decode::edge_geometry::standard_circle_param_range;
use crate::families::standard::decode::edge_geometry::standard_oriented_native_support_pcurves;
use crate::families::standard::decode::refine_repeated_face_domains_by_geometry_and_bounds;
use crate::families::standard::decode::resolve_standard_endpoint_pairs;
use crate::families::standard::decode::resolve_standard_limit_curve_binding;
use crate::families::standard::decode::retry_rejected_mesh_solution;
use crate::families::standard::decode::standard_edge_identity_is_admitted;
use crate::families::standard::decode::standard_extrusion_support_id;
use crate::families::standard::decode::standard_limit_curve_bindings;
use crate::families::standard::decode::standard_limit_curve_point_parameter;
use crate::families::standard::decode::standard_line_pair_solution_is_simple;
use crate::families::standard::decode::standard_line_pair_solution_is_simple_cached;
use crate::families::standard::decode::standard_native_support_endpoint_pair;
use crate::families::standard::decode::standard_object_evidence_from_streams;
use crate::families::standard::decode::standard_plane_normals_from_face_frames;
use crate::families::standard::decode::standard_shared_boundary_group_domains as charged_shared_boundary_group_domains;
use crate::families::standard::decode::standard_shared_nurbs_boundary_pair_options as charged_shared_nurbs_boundary_pair_options;
use crate::families::standard::decode::standard_surface_evidence;
use crate::families::standard::decode::StandardEdgeSupport;
use crate::families::standard::decode::StandardSurfaceProcedure;
use crate::families::standard::records::AnalyticSurfaceKind;
use crate::families::standard::records::StandardCurveGeometry;
use crate::families::standard::records::StandardCurveSupport;
use crate::families::standard::records::StandardFaceBounds;
use crate::families::standard::records::StandardSurfaceRecord;
use crate::families::standard::records::SurfacePrefix;
use crate::test_support::test_b5::{append_b5_record, b5_closed_triangle_stream};
use crate::test_support::test_bytes::le_f64;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::surface_point;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::nurbs::NurbsSurface;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::ProceduralCurveDefinition;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::ids::VertexId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::topology::Vertex;
use cadmpeg_ir::AnnotationBuilder;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;

#[test]
fn standard_evidence_store_refuses_each_collection_before_retaining_geometry() {
    use crate::families::standard::decode::{StandardEvidenceStore, StandardSurfaceEvidence};

    let build = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let mut store = StandardEvidenceStore::default();
        store.add(
            ctx,
            17,
            StandardSurfaceEvidence::Geometry(SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Unknown { record: None },
            )),
        )?;
        store.into_outputs(ctx, &HashSet::new())
    };
    for (limit, operation) in [
        (0, "catia_standard_evidence_records"),
        (1, "catia_standard_surface_candidates"),
        (2, "catia_standard_surface_candidate_evidence"),
        (3, "catia_standard_procedure_validity"),
        (4, "catia_standard_surface_geometries"),
    ] {
        assert!(
            matches!(
                crate::test_support::with_collection_limit(limit, build),
                Err(cadmpeg_core::CodecError::ResourceLimit(error)) if error.operation == operation
            ),
            "missing admission at {operation}"
        );
    }
    let (geometries, procedures) =
        crate::test_support::with_service_context(build).expect("service context admits evidence");
    assert_eq!(geometries.len(), 1);
    assert!(matches!(
        geometries.get(&17),
        Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: None
        }))
    ));
    assert!(procedures.is_empty());
}

#[test]
fn standard_population_object_copy_refuses_retained_limit() {
    let stream = b5_closed_triangle_stream();
    let mut cap = 0;
    let mut reached = false;
    for _ in 0..128 {
        let refusal = crate::test_support::with_retained_limit(cap, |ctx| {
            standard_object_evidence_from_streams(
                ctx,
                [stream.clone()],
                &HashSet::new(),
                &HashSet::new(),
                &mut crate::nurbs::LaneRefusals::new(),
            )
        });
        match refusal {
            Err(cadmpeg_core::CodecError::ResourceLimit(error))
                if error.operation == "catia_standard_population_object_bytes" =>
            {
                reached = true;
                break;
            }
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) => {
                cap = error
                    .used
                    .checked_add(error.additional)
                    .expect("bounded fixture");
            }
            Ok(_) => panic!("population copy admitted before its limit"),
            Err(error) => panic!("unexpected population copy refusal: {error}"),
        }
    }
    assert!(reached, "population copy limit was not reached");
}

#[test]
fn standard_object_record_scan_refuses_caller_collection_limit() {
    let stream = b5_closed_triangle_stream();
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        standard_object_evidence_from_streams(
            ctx,
            [stream],
            &HashSet::new(),
            &HashSet::new(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_record_source_ranges")
    );
}

fn standard_shared_boundary_group_domains(
    supports: &[StandardCurveSupport],
    original: &[Vec<[usize; 2]>],
    filtered: &mut [Vec<[usize; 2]>],
    edge_identity_evidence: &[bool],
    boundary_witnesses: &[bool],
) {
    crate::test_support::with_service_context(|ctx| {
        charged_shared_boundary_group_domains(
            ctx,
            supports,
            original,
            filtered,
            edge_identity_evidence,
            boundary_witnesses,
        )
    })
    .expect("service context admits shared boundary groups");
}

fn standard_shared_nurbs_boundary_pair_options(
    left: &SurfaceGeometry,
    right: &SurfaceGeometry,
    points: &[Point3],
    options: &[[usize; 2]],
) -> Option<Vec<[usize; 2]>> {
    crate::test_support::with_service_context(|ctx| {
        charged_shared_nurbs_boundary_pair_options(ctx, left, right, points, options)
    })
    .expect("service context admits NURBS boundary pairs")
}

#[test]
fn repeated_face_domain_geometry_and_bounds_keep_only_a_unique_winner() {
    let bounds = |center: [f64; 3], half_extents: [f64; 3]| StandardFaceBounds {
        aabb_center: crate::test_support::test_b5::finite_array(center),
        aabb_half_extents: half_extents.map(crate::test_support::test_b5::nonnegative_length),
        sphere_center: crate::test_support::test_b5::finite_array(center),
        sphere_radius: crate::test_support::test_b5::nonnegative_length(10.0),
    };
    let edge_faces = [[0, 0]];
    let face_bounds = [
        Some(bounds([0.0, 0.0, 0.0], [4.0, 4.0, 4.0])),
        Some(bounds([0.0, 0.0, 0.0], [1.0, 1.0, 1.0])),
        Some(bounds([0.0, 5.0, 0.0], [1.0, 1.0, 1.0])),
    ];
    let mut allowed_faces = vec![vec![1, 2]];

    refine_repeated_face_domains_by_geometry_and_bounds(
        &edge_faces,
        &mut allowed_faces,
        Some(&face_bounds),
        None,
        &[],
    );
    assert_eq!(allowed_faces, vec![vec![1]]);

    let face_bounds = [
        Some(bounds([0.0, 0.0, 0.0], [4.0, 4.0, 4.0])),
        Some(bounds([0.0, 0.0, 0.0], [1.0, 1.0, 1.0])),
        Some(bounds([0.0, 0.0, 0.0], [1.0, 1.0, 1.0])),
    ];
    let mut tied = vec![vec![1, 2]];
    refine_repeated_face_domains_by_geometry_and_bounds(
        &edge_faces,
        &mut tied,
        Some(&face_bounds),
        None,
        &[],
    );
    assert_eq!(tied, vec![vec![1, 2]]);

    let face_bounds = [
        Some(bounds([0.0, 0.0, 0.0], [4.0, 4.0, 4.0])),
        Some(bounds([0.0, 0.0, 0.0], [1.0, 1.0, 1.0])),
        None,
    ];
    let mut incomplete = vec![vec![1, 2]];
    refine_repeated_face_domains_by_geometry_and_bounds(
        &edge_faces,
        &mut incomplete,
        Some(&face_bounds),
        None,
        &[],
    );
    assert_eq!(incomplete, vec![vec![1, 2]]);
}

#[test]
fn repeated_circle_face_domain_prefers_a_distinct_carrier_before_bounds() {
    let bounds = |center: [f64; 3], half_extents: [f64; 3]| StandardFaceBounds {
        aabb_center: crate::test_support::test_b5::finite_array(center),
        aabb_half_extents: half_extents.map(crate::test_support::test_b5::nonnegative_length),
        sphere_center: crate::test_support::test_b5::finite_array(center),
        sphere_radius: crate::test_support::test_b5::nonnegative_length(10.0),
    };
    let plane = |origin: Point3| {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                origin,
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        ))
    };
    let edge_faces = [[0, 0]];
    let face_bounds = [
        Some(bounds([0.0, 0.0, 0.0], [4.0, 4.0, 4.0])),
        Some(bounds([0.0, 5.0, 0.0], [1.0, 1.0, 1.0])),
        Some(bounds([0.0, 0.0, 0.0], [1.0, 1.0, 1.0])),
    ];
    let face_geometries = [
        plane(Point3::new(0.0, 0.0, 0.0)),
        plane(Point3::new(0.0, 1.0, 0.0)),
        plane(Point3::new(0.0, 0.0, 0.0)),
    ];
    let edge_geometries = [super::checked_circle(Point3::new(0.0, 0.0, 0.0), 1.0)];
    let mut allowed_faces = vec![vec![1, 2]];

    refine_repeated_face_domains_by_geometry_and_bounds(
        &edge_faces,
        &mut allowed_faces,
        Some(&face_bounds),
        Some(&face_geometries.iter().collect::<Vec<_>>()),
        &edge_geometries.iter().collect::<Vec<_>>(),
    );
    assert_eq!(allowed_faces, vec![vec![1]]);
}

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

    let evidence = crate::test_support::with_service_context(|ctx| {
        standard_object_evidence_from_streams(
            ctx,
            [stream.clone(), stream.clone()],
            &HashSet::from([10]),
            &HashSet::new(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service resource budget");
    assert!(
        matches!(evidence.surface_geometries.get(&10), Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)))
        if {
            let origin = plane_surface.origin();
            *origin == Point3::new(0.0, 0.0, 0.0)
        })
    );

    let mut conflicting = stream.clone();
    let face_payload = conflicting.len() - 4;
    conflicting[face_payload + 1] = 0x8d;
    let evidence = crate::test_support::with_service_context(|ctx| {
        standard_object_evidence_from_streams(
            ctx,
            [stream, conflicting],
            &HashSet::from([10]),
            &HashSet::new(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service resource budget");
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
                axis_origin: crate::test_support::test_b5::point([0.0, 0.0, 0.0]),
                axis_direction: crate::test_support::test_b5::unit([0.0, 0.0, 1.0]),
                profile_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
                angular_range: crate::test_support::test_b5::increasing(angular_range),
                angular_scale: crate::test_support::test_b5::positive(1.0),
            },
        )]),
        surface_aliases: BTreeMap::new(),
        offset_surfaces: BTreeMap::new(),
        extrusion_surfaces: BTreeMap::new(),
        supported_surfaces: BTreeMap::new(),
        parameter_incidences: BTreeMap::new(),
        edges: BTreeMap::new(),
        vertex_incidence_links: BTreeMap::new(),
        vertices: crate::families::b5::graph::vertex_refs::B5Vertices::try_new(
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
        )
        .expect("valid vertex bindings"),
        edge_parameter_incidences: BTreeMap::new(),
        vertex_tolerances: BTreeMap::new(),
        profiles: BTreeMap::from([(
            11,
            B5Profile::Line {
                point: crate::test_support::test_b5::point([2.0, 0.0, 0.0]),
                direction: crate::test_support::test_b5::exact_unit([0.0, 0.0, 1.0]),
                parameter_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
            },
        )]),
    };

    let evidence = crate::test_support::with_service_context(|ctx| {
        standard_surface_evidence(ctx, &graph, 10, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("revolution evidence");
    let Some(StandardSurfaceProcedure::Revolution(revolution)) = evidence.procedure_ref() else {
        panic!("surface-of-revolution evidence must retain its construction");
    };
    assert!(matches!(
        evidence.geometry_ref(),
        Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_)))
    ));
    assert_eq!(revolution.angular_interval, angular_range);
    assert_eq!(revolution.parameter_interval, [-1.0, 1.0]);
    assert_eq!(revolution.directrix.control_points().len(), 2);
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

    let evidence = crate::test_support::with_service_context(|ctx| {
        standard_object_evidence_from_streams(
            ctx,
            [stream],
            &HashSet::from([120]),
            &HashSet::new(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service resource budget");
    assert!(matches!(
        evidence.surface_geometries.get(&120),
        Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_)))
    ));
    let Some(StandardSurfaceProcedure::Revolution(revolution)) =
        evidence.procedural_surfaces.get(&120)
    else {
        panic!("object evidence must retain revolution construction");
    };
    assert_eq!(revolution.angular_interval, [0.0, std::f64::consts::PI]);
    assert_eq!(revolution.parameter_interval, [-1.0, 1.0]);
    assert_eq!(revolution.directrix.control_points().len(), 2);
}

#[test]
fn analytic_surface_uv_accepts_finite_nonzero_carrier_scales() {
    let tiny = 1e-200;
    let cone = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
            tiny,
            std::f64::consts::FRAC_PI_6,
        )
        .expect("valid ConeSurface fixture"),
    ));
    let cone_point = surface_point(&cone, 0.5, 1.0).expect("cone point").get();
    let cone_uv = analytic_surface_uv(&cone, cone_point).expect("cone parameters");
    assert!((cone_uv.u - 0.5).abs() < 1.0e-12);
    assert_eq!(cone_uv.v, 1.0);

    let sphere = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            -tiny,
        )
        .expect("valid SphereSurface fixture"),
    ));
    let sphere_point = surface_point(&sphere, 0.5, 0.25)
        .expect("sphere point")
        .get();
    let sphere_uv = analytic_surface_uv(&sphere, sphere_point).expect("sphere parameters");
    assert!(sphere_uv.u.is_finite());
    assert!((sphere_uv.v - 0.25).abs() < 1.0e-12);

    let signed_sphere = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            -2.0,
        )
        .expect("valid SphereSurface fixture"),
    ));
    let signed_sphere_point = surface_point(&signed_sphere, 0.5, 0.25)
        .expect("signed sphere point")
        .get();
    assert!(point_on_surface(signed_sphere_point, &signed_sphere));

    let torus = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            5.0,
            -2.0,
        )
        .expect("valid TorusSurface fixture"),
    ));
    let torus_point = surface_point(&torus, 0.5, 0.25).expect("torus point").get();
    assert!(point_on_surface(torus_point, &torus));
}

#[test]
fn mesh_retry_runs_only_after_exact_rejection() {
    use crate::solve::incidence::IncidenceRejection;
    use crate::solve::mesh_quotient::{
        MeshCandidateAmbiguity, MeshCandidateExhaustion, MeshCandidateFailure,
        MeshCandidateRejection, MeshEndpointIncidenceRejection, MeshSolve,
    };

    let called = Cell::new(false);
    let outcome = retry_rejected_mesh_solution(
        MeshSolve::Failed(MeshCandidateFailure::Exhausted(
            MeshCandidateExhaustion::IncidenceEnumeration,
        )),
        || {
            called.set(true);
            MeshSolve::Failed(MeshCandidateFailure::Rejected(
                MeshCandidateRejection::EndpointIncidence(
                    MeshEndpointIncidenceRejection::NoAssignment(
                        IncidenceRejection::ComponentComposition,
                    ),
                ),
            ))
        },
    );
    assert!(matches!(
        outcome,
        MeshSolve::Failed(MeshCandidateFailure::Exhausted(
            MeshCandidateExhaustion::IncidenceEnumeration
        ))
    ));
    assert!(!called.get());

    let outcome = retry_rejected_mesh_solution(
        MeshSolve::Failed(MeshCandidateFailure::Exhausted(
            MeshCandidateExhaustion::PreferredSolutionSearch,
        )),
        || {
            called.set(true);
            MeshSolve::Failed(MeshCandidateFailure::Rejected(
                MeshCandidateRejection::InputStructure,
            ))
        },
    );
    assert!(matches!(
        outcome,
        MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::InputStructure
        ))
    ));
    assert!(called.get());

    let outcome = retry_rejected_mesh_solution(
        MeshSolve::Failed(MeshCandidateFailure::Rejected(
            MeshCandidateRejection::InputStructure,
        )),
        || {
            called.set(true);
            MeshSolve::Failed(MeshCandidateFailure::Ambiguous(
                MeshCandidateAmbiguity::EndpointResolution,
            ))
        },
    );
    assert!(matches!(
        outcome,
        MeshSolve::Failed(MeshCandidateFailure::Ambiguous(
            MeshCandidateAmbiguity::EndpointResolution
        ))
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
    assert_eq!(*axis.as_raw(), Vector3::new(0.0, 0.0, 1.0));
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
fn circular_face_interval_choices_select_disjoint_arc_branches() {
    let tau = std::f64::consts::TAU;
    let simple = vec![
        vec![[0.0, 0.125], [0.125, tau]],
        vec![
            [3.0, std::f64::consts::PI],
            [std::f64::consts::PI, tau + 3.0],
        ],
    ];
    let crossing = vec![
        vec![
            [0.125, std::f64::consts::PI],
            [std::f64::consts::PI, tau + 0.125],
        ],
        vec![[0.0, 3.0], [3.0, tau]],
    ];

    assert!(circular_range_choices_have_simple_selection(&simple));
    assert!(!circular_range_choices_have_simple_selection(&crossing));
}

#[test]
fn circular_face_interval_budget_cannot_admit_an_unproved_selection() {
    let mut choices = vec![vec![[0.0, 1.0], [2.0, 3.0]]; 12];
    choices.push(vec![[0.5, 2.5]]);
    assert!(!circular_range_choices_have_simple_selection(&choices));
}

#[test]
fn standard_line_interval_constraint_rejects_partial_collinear_overlap() {
    let points = [0.0, 1.0, 2.0, 3.0]
        .into_iter()
        .enumerate()
        .map(|(index, x)| {
            Point::new(
                PointId::mint(format!("catia:test:point#p{index}")).expect("identity grammar"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(x, 0.0, 0.0))
                    .expect("a finite position is a point"),
                None,
            )
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
    let zero_length = [Some([1, 1]), None, None];
    assert!(!standard_line_pair_solution_is_simple(
        &points,
        &supports,
        &options,
        &zero_length,
    ));
    assert!(!standard_line_pair_solution_is_simple_cached(
        &points,
        &supports,
        &options,
        &zero_length,
    ));
}

#[test]
fn standard_edge_identity_requires_coordinate_bound_evidence() {
    assert!(!standard_edge_identity_is_admitted(
        None, None, false, false
    ));
    assert!(standard_edge_identity_is_admitted(
        Some([0, 1]),
        None,
        false,
        false,
    ));
    assert!(standard_edge_identity_is_admitted(
        None,
        Some([0, 1]),
        false,
        false,
    ));
    assert!(standard_edge_identity_is_admitted(None, None, true, false,));
    assert!(standard_edge_identity_is_admitted(None, None, false, true,));
}

#[test]
fn shared_nurbs_boundary_filters_identity_free_endpoint_pairs() {
    let surface = |reverse_shared_boundary: bool, offset: f64| {
        let shared = if reverse_shared_boundary {
            [Point3::new(offset, 1.0, 0.0), Point3::new(offset, 0.0, 0.0)]
        } else {
            [Point3::new(offset, 0.0, 0.0), Point3::new(offset, 1.0, 0.0)]
        };
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            NurbsSurface::from_lanes(
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                    vec![
                        vec![shared[0], shared[1]],
                        vec![
                            Point3::new(
                                offset + if reverse_shared_boundary { 1.0 } else { -1.0 },
                                shared[0].y,
                                0.0,
                            ),
                            Point3::new(
                                offset + if reverse_shared_boundary { 1.0 } else { -1.0 },
                                shared[1].y,
                                0.0,
                            ),
                        ],
                    ],
                    None,
                ),
                false,
            )
            .expect("valid bilinear NURBS"),
        ))
    };
    let left = surface(false, 0.0);
    let right = surface(true, 0.0);
    let points = vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 0.5, 1.0),
    ];
    let options = [[0, 1], [0, 2], [2, 3], [0, 3]];

    assert_eq!(
        standard_shared_nurbs_boundary_pair_options(&left, &right, &points, &options),
        Some(vec![[0, 1]])
    );
    assert!(standard_shared_nurbs_boundary_pair_options(
        &left,
        &surface(false, 2.0),
        &points,
        &options,
    )
    .is_none());
}

#[test]
fn repeated_shared_boundary_rows_keep_domains_when_one_witness_cannot_cover_them() {
    let supports = [0, 1]
        .into_iter()
        .map(|pos| StandardCurveSupport {
            pos,
            tag: pos as u32,
            faces: [3, 7],
            geometry: StandardCurveGeometry::Bspline,
        })
        .collect::<Vec<_>>();
    let original = vec![
        vec![[2, 8], [2, 9], [3, 8], [3, 9]],
        vec![[2, 8], [2, 9], [3, 8], [3, 9]],
    ];
    let mut filtered = vec![vec![[2, 8]], vec![[2, 8]]];

    standard_shared_boundary_group_domains(
        &supports,
        &original,
        &mut filtered,
        &[false, false],
        &[true, true],
    );

    assert_eq!(filtered, original);
}

#[test]
fn shared_boundary_group_entries_refuse_before_inner_row_growth() {
    let supports = [StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [3, 7],
        geometry: StandardCurveGeometry::Bspline,
    }];
    let original = vec![vec![[2, 8]]];
    let mut filtered = original.clone();
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        charged_shared_boundary_group_domains(
            ctx,
            &supports,
            &original,
            &mut filtered,
            &[false],
            &[true],
        )
    });
    assert!(matches!(
        limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
    standard_shared_boundary_group_domains(&supports, &original, &mut filtered, &[false], &[true]);
    assert_eq!(filtered, original);
}

#[test]
fn repeated_shared_boundary_rows_keep_positive_narrowing_when_witnesses_cover_rows() {
    let supports = [0, 1]
        .into_iter()
        .map(|pos| StandardCurveSupport {
            pos,
            tag: pos as u32,
            faces: [3, 7],
            geometry: StandardCurveGeometry::Bspline,
        })
        .collect::<Vec<_>>();
    let original = vec![
        vec![[2, 8], [2, 9], [3, 8], [3, 9]],
        vec![[2, 8], [2, 9], [3, 8], [3, 9]],
    ];
    let mut filtered = vec![vec![[2, 8]], vec![[3, 9]]];

    standard_shared_boundary_group_domains(
        &supports,
        &original,
        &mut filtered,
        &[false, false],
        &[true, true],
    );

    assert_eq!(filtered, vec![vec![[2, 8]], vec![[3, 9]]]);
}

#[test]
fn repeated_shared_boundary_rows_keep_domains_when_a_witness_is_unavailable() {
    let supports = [0, 1]
        .into_iter()
        .map(|pos| StandardCurveSupport {
            pos,
            tag: pos as u32,
            faces: [3, 7],
            geometry: StandardCurveGeometry::Bspline,
        })
        .collect::<Vec<_>>();
    let original = vec![
        vec![[2, 8], [2, 9], [3, 8], [3, 9]],
        vec![[2, 8], [2, 9], [3, 8], [3, 9]],
    ];
    let mut filtered = vec![vec![[2, 8]], original[1].clone()];

    standard_shared_boundary_group_domains(
        &supports,
        &original,
        &mut filtered,
        &[false, false],
        &[true, false],
    );

    assert_eq!(filtered, original);
}

#[test]
fn cached_standard_line_pair_preference_matches_the_geometry_rule() {
    let points = [0.0, 1.0, 2.0, 3.0]
        .into_iter()
        .enumerate()
        .map(|(index, x)| {
            Point::new(
                PointId::mint(format!("catia:test:point#p{index}")).expect("identity grammar"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(x, 0.0, 0.0))
                    .expect("a finite position is a point"),
                None,
            )
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
    let options = vec![
        vec![[0, 1], [0, 2], [1, 2], [1, 3], [2, 3]],
        vec![[0, 1], [0, 2], [1, 2], [1, 3], [2, 3]],
        vec![[0, 1], [0, 2], [1, 2], [1, 3], [2, 3]],
    ];
    let simple = [Some([0, 1]), Some([1, 2]), Some([2, 3])];
    let overlapping = [Some([0, 2]), Some([2, 3]), Some([1, 3])];

    assert_eq!(
        standard_line_pair_solution_is_simple_cached(&points, &supports, &options, &simple),
        standard_line_pair_solution_is_simple(&points, &supports, &options, &simple),
    );
    assert_eq!(
        standard_line_pair_solution_is_simple_cached(&points, &supports, &options, &overlapping),
        standard_line_pair_solution_is_simple(&points, &supports, &options, &overlapping),
    );
}

#[test]
fn standard_plane_normals_require_signed_face_frame_vectors() {
    let plane = |target| {
        StandardSurfaceRecord::Analytic(SurfacePrefix {
            pos: 0,
            target,
            kind: AnalyticSurfaceKind::Plane,
        })
    };
    let records = vec![plane(10), plane(20), plane(30)];

    assert!(crate::test_support::with_service_context(|ctx| {
        standard_plane_normals_from_face_frames(ctx, &records, &[None, None, None])
    })
    .expect("service resource budget")
    .is_empty());
    assert_eq!(
        crate::test_support::with_service_context(|ctx| standard_plane_normals_from_face_frames(
            ctx,
            &records,
            &[
                Some(crate::test_support::test_b5::finite_vector([0.0, 0.0, 1.0])),
                None,
                Some(crate::test_support::test_b5::finite_vector([
                    0.0, 0.0, -1.0
                ]))
            ],
        ))
        .expect("service resource budget"),
        HashMap::from([
            (
                10,
                crate::test_support::test_b5::finite_vector([0.0, 0.0, 1.0])
            ),
            (
                30,
                crate::test_support::test_b5::finite_vector([0.0, 0.0, -1.0])
            ),
        ]),
    );

    let conflicting = vec![plane(10), plane(10)];
    assert!(crate::test_support::with_service_context(|ctx| {
        standard_plane_normals_from_face_frames(
            ctx,
            &conflicting,
            &[
                Some(crate::test_support::test_b5::finite_vector([0.0, 0.0, 1.0])),
                Some(crate::test_support::test_b5::finite_vector([
                    0.0, 0.0, -1.0,
                ])),
            ],
        )
    })
    .expect("service resource budget")
    .is_empty());
}

#[test]
fn plane_normal_candidate_and_result_maps_refuse_before_growth() {
    let records = [StandardSurfaceRecord::Analytic(SurfacePrefix {
        pos: 0,
        target: 10,
        kind: AnalyticSurfaceKind::Plane,
    })];
    let frames = [Some(crate::test_support::test_b5::finite_vector([
        0.0, 0.0, 1.0,
    ]))];
    let normals = crate::test_support::with_service_context(|ctx| {
        standard_plane_normals_from_face_frames(ctx, &records, &frames)
    })
    .expect("service resource budget");
    assert_eq!(normals.len(), 1);
    for (cap, operation) in [
        (0, "catia_plane_normal_candidates"),
        (1, "catia_plane_normals"),
    ] {
        assert!(matches!(
            crate::test_support::with_collection_limit(cap, |ctx| {
                standard_plane_normals_from_face_frames(ctx, &records, &frames)
            }),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == operation
        ));
    }
}

#[test]
fn standard_spline_uses_identity_bound_native_support_pcurves() {
    let mut ir = CadIr::empty();
    ir.model.points.extend(
        [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]
            .into_iter()
            .enumerate()
            .map(|(index, position)| {
                Point::new(
                    PointId::mint(format!("catia:test:point#point-{index}"))
                        .expect("identity grammar"),
                    cadmpeg_ir::features::FinitePoint3::new(position)
                        .expect("a finite position is a point"),
                    None,
                )
            }),
    );
    let support = StandardCurveSupport {
        pos: 12,
        tag: 40,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    };
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("valid LinePcurve fixture"),
    );
    let native = StandardEdgeSupport {
        surface_object_ids: [20, 21],
        carriers: [
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
            ),
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
            ),
        ],
        pcurves: [pcurve.clone(), pcurve],
        parameter_range: [2.0, 5.0],
    };
    let (curve, range) = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &[],
            &HashMap::new(),
            &[],
            &support,
            [0, 1],
            Some(&native),
            None,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut admission,
        )
    })
    .expect("valid source object identity");
    let curve = curve.expect("native support identifies the curve");
    assert_eq!(range, Some([2.0, 5.0]));
    assert_eq!(ir.model.surfaces.len(), 2);
    let [procedural] = ir.model.procedural_curves.as_slice() else {
        panic!("one procedural curve");
    };
    let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition() else {
        panic!("intersection construction");
    };
    assert_eq!(
        ir.model.procedural_curve_owner(&procedural.id),
        Some(&curve)
    );
    assert_eq!(context.parameter_range().endpoints(), [2.0, 5.0]);
    assert!(context.sides().iter().all(|side| side.pcurve.is_some()));
}

#[test]
fn native_support_pcurves_bind_standard_edge_endpoints() {
    let mut points = [Point3::new(1.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)]
        .into_iter()
        .enumerate()
        .map(|(index, position)| {
            Point::new(
                PointId::mint(format!("catia:test:point#point-{index}")).expect("identity grammar"),
                cadmpeg_ir::features::FinitePoint3::new(position)
                    .expect("a finite position is a point"),
                None,
            )
        })
        .collect::<Vec<_>>();
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("valid LinePcurve fixture"),
    );
    let native = StandardEdgeSupport {
        surface_object_ids: [20, 21],
        carriers: [
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
            ),
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
            ),
        ],
        pcurves: [pcurve.clone(), pcurve],
        parameter_range: [1.0, 4.0],
    };

    assert_eq!(
        standard_native_support_endpoint_pair(&native, &points, &[0, 1], None)
            .expect("evaluator allocation succeeds"),
        Some([0, 1])
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| standard_oriented_native_support_pcurves(
            ctx,
            &native,
            &points,
            [1, 0],
            &mut crate::nurbs::LaneRefusals::new()
        ))
        .expect("service profile admits native support pcurve reversal"),
        Some([
            PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(5.0, 0.0),
                    Point2::new(-1.0, 0.0)
                )
                .expect("valid LinePcurve fixture")
            ),
            PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(5.0, 0.0),
                    Point2::new(-1.0, 0.0)
                )
                .expect("valid LinePcurve fixture")
            ),
        ])
    );
    assert_eq!(
        standard_native_support_endpoint_pair(&native, &points, &[0, 1], Some([0, 2]))
            .expect("evaluator allocation succeeds"),
        None
    );

    let mut reversed = native.clone();
    reversed.pcurves[1] = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(5.0, 0.0),
            Point2::new(-1.0, 0.0),
        )
        .expect("valid LinePcurve fixture"),
    );
    assert_eq!(
        standard_native_support_endpoint_pair(&reversed, &points, &[0, 1], None)
            .expect("evaluator allocation succeeds"),
        Some([0, 1])
    );

    points.push(Point::new(
        PointId::mint("catia:test:point#ambiguous-start".to_string()).expect("identity grammar"),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
            .expect("a finite position is a point"),
        None,
    ));
    assert_eq!(
        standard_native_support_endpoint_pair(&native, &points, &[0, 1, 2], None)
            .expect("evaluator allocation succeeds"),
        None
    );

    let mut disagreeing = native.clone();
    disagreeing.pcurves[1] = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 1.0),
            Point2::new(1.0, 0.0),
        )
        .expect("valid LinePcurve fixture"),
    );
    assert_eq!(
        standard_native_support_endpoint_pair(&disagreeing, &points, &[0, 1], None)
            .expect("evaluator allocation succeeds"),
        None
    );
}

#[test]
fn native_support_pcurve_copy_refuses_retained_and_collection_limits() {
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            None,
            false,
        )
        .expect("valid linear pcurve"),
    };
    let carrier = crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
    );
    let native = StandardEdgeSupport {
        surface_object_ids: [20, 21],
        carriers: [carrier.clone(), carrier],
        pcurves: [pcurve.clone(), pcurve],
        parameter_range: [0.0, 1.0],
    };
    let copy = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        standard_oriented_native_support_pcurves(
            ctx,
            &native,
            &[],
            [0, 1],
            &mut crate::nurbs::LaneRefusals::new(),
        )
    };
    for refused in [
        crate::test_support::with_retained_limit(0, copy),
        crate::test_support::with_collection_limit(0, copy),
    ] {
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_standard_native_support_pcurve_copy")
        );
    }
    let admitted = crate::test_support::with_service_context(copy)
        .expect("service profile admits native support copy")
        .expect("unbound endpoints still retain native pcurves");
    assert_eq!(admitted, native.pcurves);
}

#[test]
fn standard_native_reverse_label_refuses_materialized_limit() {
    let points = [1.0, 4.0]
        .into_iter()
        .enumerate()
        .map(|(index, x)| {
            Point::new(
                PointId::mint(format!("catia:test:point#{index}")).expect("identity"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(x, 0.0, 0.0))
                    .expect("finite point"),
                None,
            )
        })
        .collect::<Vec<_>>();
    let plane = crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane fixture"),
        )),
    );
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("line pcurve"),
    );
    let native = StandardEdgeSupport {
        surface_object_ids: [20, 21],
        carriers: [plane.clone(), plane],
        pcurves: [pcurve.clone(), pcurve],
        parameter_range: [1.0, 4.0],
    };
    assert_eq!(
        standard_native_support_endpoint_pair(&native, &points, &[0, 1], Some([0, 1])),
        Ok(Some([0, 1]))
    );
    let limited = crate::test_support::with_materialized_limit(0, |ctx| {
        standard_oriented_native_support_pcurves(
            ctx,
            &native,
            &points,
            [1, 0],
            &mut crate::nurbs::LaneRefusals::new(),
        )
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_standard_native_pcurve_reverse_label")
    );
    assert!(crate::test_support::with_service_context(|ctx| {
        standard_oriented_native_support_pcurves(
            ctx,
            &native,
            &points,
            [1, 0],
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service profile admits reversal")
    .is_some());
}

#[test]
fn limit_curve_point_binding_rejects_separated_occurrences_with_unequal_residuals() {
    let line_span = |offset: f64| {
        (0..6)
            .map(|index| Point3::new(-1.0 + 0.4 * f64::from(index) + offset, 0.0, 0.0))
            .collect::<Vec<_>>()
    };
    let curve = NurbsCurve::from_lanes(
        5,
        [vec![0.0; 6], vec![0.5; 6], vec![1.0; 6]].concat(),
        [line_span(0.0), line_span(1e-3)].concat(),
        None,
        false,
    )
    .expect("valid degree-5 NURBS");

    assert_eq!(
        crate::test_support::with_service_context(|ctx| standard_limit_curve_point_parameter(
            ctx,
            &curve,
            Point3::new(0.0, 0.0, 0.0),
            2e-3
        ))
        .expect("service budget"),
        None
    );
}

#[test]
fn limit_curve_binding_retains_correlated_edge_candidates() {
    let mut ir = CadIr::empty();
    ir.model.points.extend(
        [Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)]
            .into_iter()
            .enumerate()
            .map(|(index, position)| {
                Point::new(
                    PointId::mint(format!("catia:test:point#point-{index}"))
                        .expect("identity grammar"),
                    cadmpeg_ir::features::FinitePoint3::new(position)
                        .expect("a finite position is a point"),
                    None,
                )
            }),
    );
    let surface_id =
        SurfaceId::mint("catia:test:surface#surface".to_string()).expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    });
    let support = StandardCurveSupport {
        pos: 10,
        tag: 20,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    };
    let limit_curve = NurbsCurve::from_lanes(
        5,
        vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        (0..6)
            .map(|index| Point3::new(-1.0 + 0.8 * f64::from(index), 0.0, 0.0))
            .collect(),
        None,
        false,
    )
    .expect("valid degree-5 NURBS");
    let bindings = [(surface_id.clone(), false, 0)];
    let surface_indices = HashMap::from([(surface_id, 0)]);

    let limit_bindings = crate::test_support::with_service_context(|ctx| {
        standard_limit_curve_bindings(
            ctx,
            &ir,
            &bindings,
            &surface_indices,
            std::slice::from_ref(&support),
            std::slice::from_ref(&limit_curve),
        )
    })
    .expect("service budget");
    let mut refusal_operations = std::collections::HashSet::new();
    let mut admitted = false;
    for limit in 0..=256 {
        match crate::test_support::with_collection_limit(limit, |ctx| {
            standard_limit_curve_bindings(
                ctx,
                &ir,
                &bindings,
                &surface_indices,
                std::slice::from_ref(&support),
                std::slice::from_ref(&limit_curve),
            )
        }) {
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) => {
                refusal_operations.insert(error.operation);
            }
            Ok(result) if result == limit_bindings => {
                admitted = true;
                break;
            }
            outcome => panic!("unexpected limit curve binding outcome: {outcome:?}"),
        }
    }
    assert!(
        admitted,
        "collection limit 256 must admit limit curve binding"
    );
    for operation in [
        "catia_limit_curve_point_rows",
        "catia_limit_curve_point_parameters",
        "catia_limit_curve_edge_rows",
        "catia_limit_curve_candidates",
        "catia_limit_curve_geometry_copy",
        "catia_limit_curve_edge_bindings",
    ] {
        assert!(
            refusal_operations.contains(operation),
            "no refusal at {operation}"
        );
    }
    let [limit_candidates] = limit_bindings.as_slice() else {
        panic!("one edge limit-curve domain");
    };
    let [binding] = limit_candidates.as_slice() else {
        panic!("one limit-curve candidate");
    };
    assert_eq!((binding.curve, binding.points), (0, [0, 1]));
    assert!((binding.parameter_range[0] - 0.25).abs() <= 1.0e-6);
    assert!((binding.parameter_range[1] - 0.75).abs() <= 1.0e-6);
    let (curve, range) = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &bindings,
            &surface_indices,
            &[],
            &support,
            [0, 1],
            None,
            Some((&limit_curve, binding.parameter_range)),
            &mut crate::nurbs::LaneRefusals::new(),
            &mut admission,
        )
    })
    .expect("valid source object identity");
    assert_eq!(range, Some(binding.parameter_range));
    assert!(matches!(
        curve
            .and_then(|id| ir.model.curves.iter().find(|curve| curve.id == id))
            .map(|curve| &curve.geometry),
        Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve))) if curve == &limit_curve
    ));
    let duplicated = crate::test_support::with_service_context(|ctx| {
        standard_limit_curve_bindings(
            ctx,
            &ir,
            &bindings,
            &surface_indices,
            &[support.clone(), support],
            &[limit_curve],
        )
    })
    .expect("service budget");
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
fn standard_edge_limit_curve_copy_refuses_collection_limit() {
    let mut ir = CadIr::empty();
    for (index, x) in [0.0, 1.0].into_iter().enumerate() {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#{index}")).expect("identity"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(x, 0.0, 0.0))
                .expect("finite point"),
            None,
        ));
    }
    let support = StandardCurveSupport {
        pos: 10,
        tag: 20,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    };
    let limit_curve = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .expect("valid linear NURBS");
    let mut limited_ir = ir.clone();
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            &mut limited_ir,
            &mut AnnotationBuilder::new(),
            &[],
            &HashMap::new(),
            &[],
            &support,
            [0, 1],
            None,
            Some((&limit_curve, [0.0, 1.0])),
            &mut crate::nurbs::LaneRefusals::new(),
            &mut admission,
        )
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_standard_limit_curve_copy")
    );
    let admitted = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &[],
            &HashMap::new(),
            &[],
            &support,
            [0, 1],
            None,
            Some((&limit_curve, [0.0, 1.0])),
            &mut crate::nurbs::LaneRefusals::new(),
            &mut admission,
        )
    })
    .expect("service profile admits the edge");
    assert!(admitted.0.is_some());
    assert!(matches!(&ir.model.curves[0].geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) if curve == &limit_curve));
}

#[test]
fn standard_line_edge_uses_distance_parameterization() {
    let mut ir = CadIr::empty();
    for (index, position) in [Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 6.0, 3.0)]
        .into_iter()
        .enumerate()
    {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#p{index}")).expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(position)
                .expect("a finite position is a point"),
            None,
        ));
    }
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Line,
    };
    let (_, range) = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &[],
            &HashMap::new(),
            &[],
            &support,
            [0, 1],
            None,
            None,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut admission,
        )
    })
    .expect("valid source object identity");
    assert_eq!(range, Some([0.0, 5.0]));
}

#[test]
fn standard_line_edge_accepts_a_finite_nonzero_distance() {
    let mut ir = CadIr::empty();
    for (index, position) in [Point3::new(0.0, 0.0, 0.0), Point3::new(1e-200, 0.0, 0.0)]
        .into_iter()
        .enumerate()
    {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#p{index}")).expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(position)
                .expect("a finite position is a point"),
            None,
        ));
    }
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Line,
    };
    let (curve, range) = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &[],
            &HashMap::new(),
            &[],
            &support,
            [0, 1],
            None,
            None,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut admission,
        )
    })
    .expect("valid source object identity");
    assert!(curve.is_some());
    assert_eq!(range, Some([0.0, 1e-200]));
}

#[test]
fn witnessed_cylinder_circle_edge_uses_complementary_angular_range() {
    let mut ir = CadIr::empty();
    let surface_id =
        SurfaceId::mint("catia:test:surface#cylinder".to_string()).expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .expect("valid CylinderSurface fixture"),
        )),
        source_object: None,
    });
    let bindings = [(surface_id.clone(), true, 0), (surface_id.clone(), true, 0)];
    let indices = [(surface_id, 0)].into_iter().collect();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: super::checked_circle(Point3::new(0.0, 0.0, 3.0), 2.0),
    };
    let mut brep = vec![0; 39];
    brep[..3].copy_from_slice(&[0x00, 0x33, 0x33]);
    brep[27..31].copy_from_slice(&(-2.0f32).to_le_bytes());
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = cadmpeg_ir::geometry::derive_reference_direction(axis);
    let range = crate::test_support::with_service_context(|ctx| {
        standard_circle_param_range(
            ctx,
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
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service budget admits circle range")
    .expect("witnessed circle range");
    assert!(((range[1] - range[0]).abs() - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
}

#[test]
fn native_support_pcurve_midpoint_selects_an_unwitnessed_circle_branch() {
    let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("valid LinePcurve fixture"),
    );
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
        )
        .expect("evaluator allocation succeeds"),
        Some([0.0, 1.5 * std::f64::consts::PI])
    );
    let mut disagreeing = native.clone();
    disagreeing.pcurves[1] = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 1.0),
            Point2::new(1.0, 0.0),
        )
        .expect("valid LinePcurve fixture"),
    );
    assert!(native_support_circle_param_range(
        &disagreeing,
        Point3::new(0.0, 0.0, 0.0),
        1.0,
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        start,
        end,
    )
    .expect("evaluator allocation succeeds")
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
    .expect("evaluator allocation succeeds")
    .is_none());

    let mut ir = CadIr::empty();
    for (index, position) in [start, end].into_iter().enumerate() {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#p{index}")).expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(position)
                .expect("a finite position is a point"),
            None,
        ));
    }
    let support = StandardCurveSupport {
        pos: 12,
        tag: 7,
        faces: [0, 0],
        geometry: super::checked_circle(Point3::new(0.0, 0.0, 0.0), 1.0),
    };
    let (_, range) = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        build_standard_edge_curve(
            ctx,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &[],
            &HashMap::new(),
            &[],
            &support,
            [0, 1],
            Some(&native),
            None,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut admission,
        )
    })
    .expect("valid source object identity");
    assert_eq!(range, Some([0.0, 1.5 * std::f64::consts::PI]));
}

mod face_evidence;
mod surface_intersections;

#[test]
fn line_pair_constraint_rejects_pairs_beyond_edge_roles() {
    let constraint = crate::test_support::with_service_context(|ctx| {
        super::super::StandardLinePairConstraint::new(ctx, &[], &[], &[])
    })
    .expect("service budget admits line constraint");
    assert!(constraint.edge_pairs(&[None]).is_none());
}

mod overflowing_support;
