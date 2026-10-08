// SPDX-License-Identifier: Apache-2.0
//! face evidence tests.

use super::{
    attach_free_vertices, copy_standard_extrusion_definition, native_support_circle_param_range,
    resolve_standard_endpoint_pairs, standard_extrusion_support_id,
    standard_native_support_endpoint_pair, AnalyticSurfaceKind, AnnotationBuilder, CadIr, HashMap,
    HashSet, PcurveGeometry, Point, Point2, Point3, PointId, SolvedSurfaceGeometry,
    StandardCurveGeometry, StandardCurveSupport, StandardEdgeSupport, StandardSurfaceRecord,
    Surface, SurfaceGeometry, SurfaceId, SurfacePrefix, Vector3, Vertex, VertexId,
};

#[test]
fn standard_empty_vertex_population_creates_no_owner_or_annotations() {
    let mut ir = CadIr::empty();
    let before = ir.clone();
    let mut annotations = AnnotationBuilder::new();
    crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        attach_free_vertices(ctx, &mut ir, &mut annotations, &mut admission)
    })
    .expect("service profile admits free vertex owner");
    assert_eq!(ir, before);
    assert_eq!(annotations.build(), AnnotationBuilder::new().build());
}

#[test]
fn standard_surface_entity_limit_refuses_before_first_surface_append() {
    let bytes = crate::test_support::test_container::standard_catpart();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, bytes.clone())
    })
    .expect("service resource budget");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_entities = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("standard fixture fits the input-byte limit");
    let result = crate::families::standard::decode::try_decode_standard(
        &ctx,
        &scan,
        &mut crate::nurbs::LaneRefusals::new(),
    );
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
        panic!("first standard surface must exceed the entity limit");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::Entities
    );
    assert_eq!(limit.used, 1);
    assert_eq!(limit.operation, "admit CATIA family model entity");
}

#[test]
fn standard_free_vertex_owner_limit_refuses_before_shell_creation() {
    let mut ir = CadIr::empty();
    ir.model.vertices.push(Vertex {
        id: VertexId::mint("catia:test:vertex#v").expect("identity grammar"),
        point: PointId::mint("catia:test:point#p").expect("identity grammar"),
        tolerance: None,
    });
    let mut annotations = AnnotationBuilder::new();
    crate::test_support::with_entity_limit(0, |ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            attach_free_vertices(ctx, &mut ir, &mut annotations, &mut admission)
        else {
            panic!("free-vertex owner must exceed the entity limit");
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::Entities
        );
        assert_eq!(limit.operation, "admit CATIA family model entity");
    });
    assert!(ir.model.bodies.is_empty());
    assert!(ir.model.regions.is_empty());
    assert!(ir.model.shells.is_empty());
}

#[test]
fn standard_unbound_vertices_receive_one_free_vertex_owner() {
    let mut ir = CadIr::empty();
    ir.model.vertices.push(Vertex {
        id: VertexId::mint("catia:test:vertex#v".to_string()).expect("identity grammar"),
        point: PointId::mint("catia:test:point#p".to_string()).expect("identity grammar"),
        tolerance: None,
    });
    let mut annotations = AnnotationBuilder::new();
    crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        attach_free_vertices(ctx, &mut ir, &mut annotations, &mut admission)
    })
    .expect("service profile admits free vertex owner");
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.regions.len(), 1);
    assert_eq!(ir.model.shells.len(), 1);
    assert_eq!(
        ir.model.shells[0].free_vertices(),
        [VertexId::mint("catia:test:vertex#v".to_string()).expect("identity grammar")]
    );
}

#[test]
fn standard_free_vertex_members_refuse_collection_limit() {
    let mut ir = CadIr::empty();
    ir.model.vertices.push(Vertex {
        id: VertexId::mint("catia:test:vertex#limit").expect("identity grammar"),
        point: PointId::mint("catia:test:point#limit").expect("identity grammar"),
        tolerance: None,
    });
    let limited = crate::test_support::with_collection_limit(1, |ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        attach_free_vertices(ctx, &mut ir, &mut AnnotationBuilder::new(), &mut admission)
    });
    assert!(matches!(
        limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
    crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        attach_free_vertices(ctx, &mut ir, &mut AnnotationBuilder::new(), &mut admission)
    })
    .expect("service context admits the free vertex");
    assert_eq!(ir.model.shells[0].free_vertices().len(), 1);
}

#[test]
fn standard_extrusion_support_refuses_before_surface_storage() {
    let mut surfaces = Vec::new();
    let mut supports = HashMap::new();
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        standard_extrusion_support_id(
            ctx,
            &mut AnnotationBuilder::new(),
            &mut surfaces,
            &mut supports,
            &mut ctx.reserve_scoped(0, "test support map").expect("scope"),
            7,
            geometry.clone(),
            &mut admission,
        )
    });
    assert!(matches!(
        limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
    crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        standard_extrusion_support_id(
            ctx,
            &mut AnnotationBuilder::new(),
            &mut surfaces,
            &mut supports,
            &mut ctx.reserve_scoped(0, "test support map").expect("scope"),
            7,
            geometry,
            &mut admission,
        )
    })
    .expect("service context admits support");
    assert_eq!(surfaces.len(), 1);
    assert_eq!(supports.len(), 1);
}

#[test]
fn standard_extrusion_definition_copy_refuses_retained_id_limit() {
    let directrix =
        cadmpeg_ir::ids::CurveId::mint("catia:test:directrix#7").expect("identity grammar");
    let direction =
        cadmpeg_ir::features::FiniteVector3::new(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0))
            .expect("finite direction");
    let definition = cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::legacy(
        directrix, None, direction, None, None,
    );
    let limited = crate::test_support::with_retained_limit(0, |ctx| {
        copy_standard_extrusion_definition(ctx, &definition)
    });
    assert!(matches!(
        limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
    let copied = crate::test_support::with_service_context(|ctx| {
        copy_standard_extrusion_definition(ctx, &definition)
    })
    .expect("service context admits the retained directrix identity");
    assert_eq!(
        copied,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(definition),
    );
}

#[test]
fn standard_native_edge_face_carrier_and_candidate_limits_refuse() {
    let supports = [StandardCurveSupport {
        pos: 0,
        tag: 700,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Bspline,
    }];
    let records = [10u32, 20].map(|target| {
        StandardSurfaceRecord::Analytic(SurfacePrefix {
            pos: 0,
            target,
            kind: AnalyticSurfaceKind::Cylinder,
        })
    });
    let owners = HashMap::from([(700, vec![20])]);
    let mut faces = [[0, 0]];
    let result = crate::test_support::with_collection_limit(0, |ctx| {
        crate::families::standard::decode::apply_standard_native_edge_faces(
            ctx, &mut faces, &supports, &records, &owners,
        )
    });
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
        if refusal.operation == "catia_standard_native_face_carriers")
    );
    let mut faces = [[0, 0]];
    crate::test_support::with_service_context(|ctx| {
        crate::families::standard::decode::apply_standard_native_edge_faces(
            ctx, &mut faces, &supports, &records, &owners,
        )
    })
    .expect("service context admits native face evidence");
    assert_eq!(faces, [[0, 1]]);
}

#[test]
fn standard_face_attachment_refuses_each_collection_boundary() {
    let brep = crate::test_support::test_topology::standard_quad_topology_stream();
    let bindings = [(
        SurfaceId::mint("catia:test:surface#face").expect("identity grammar"),
        true,
        0,
    )];
    let mut refusals = HashSet::new();
    for limit in 0..256 {
        let mut ir = CadIr::empty();
        let mut annotations = AnnotationBuilder::new();
        let result = crate::test_support::with_collection_limit(limit, |ctx| {
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            crate::families::standard::decode::attach_standard_faces(
                ctx,
                &mut ir,
                &mut annotations,
                &bindings,
                &brep,
                &mut admission,
            )
        });
        if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = result {
            refusals.insert(refusal.operation);
        }
    }
    for operation in [
        "catia_standard_shell_face_ids",
        "catia_standard_model_faces",
        "catia_standard_body_regions",
        "catia_standard_model_bodies",
        "catia_standard_region_shells",
        "catia_standard_model_regions",
        "catia_standard_model_shells",
    ] {
        assert!(
            refusals.contains(operation),
            "missing charge for {operation}"
        );
    }
    let mut ir = CadIr::empty();
    crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        crate::families::standard::decode::attach_standard_faces(
            ctx,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &bindings,
            &brep,
            &mut admission,
        )
    })
    .expect("service context admits the standard face");
    assert_eq!(ir.model.faces[0].id.as_str(), "catia:standard:face#0");
    assert_eq!(ir.model.bodies[0].id.as_str(), "catia:standard:body#0");
    assert_eq!(ir.model.regions[0].id.as_str(), "catia:standard:region#0-0");
    assert_eq!(ir.model.shells[0].id.as_str(), "catia:standard:shell#0-0");
    assert_eq!(ir.model.shells[0].faces().len(), 1);
}

#[test]
fn standard_face_partition_refuses_each_collection_boundary() {
    let brep = crate::test_support::test_topology::standard_quad_topology_stream();
    let bindings = [(
        SurfaceId::mint("catia:test:surface#partition").expect("identity grammar"),
        true,
        0,
    )];
    let mut base_ir = CadIr::empty();
    let mut base_annotations = AnnotationBuilder::new();
    crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        crate::families::standard::decode::attach_standard_faces(
            ctx,
            &mut base_ir,
            &mut base_annotations,
            &bindings,
            &brep,
            &mut admission,
        )
    })
    .expect("service context admits the initial face");
    let mut second = base_ir.model.faces[0].clone();
    second.id = cadmpeg_ir::ids::FaceId::mint("catia:standard:face#1").expect("identity grammar");
    crate::test_support::with_service_context(|ctx| {
        crate::assemble::annotate(
            ctx,
            &mut base_annotations,
            &second.id,
            "MainDataStream+SurfacicReps",
            0,
            "surfacic_reps_face_sense",
            cadmpeg_ir::Exactness::ByteExact,
        )
    })
    .expect("service profile admits second face annotation");
    base_ir.model.faces.push(second);
    let components = [vec![0], vec![1]];
    let mut refusals = HashSet::new();
    for limit in 0..128 {
        let mut ir = base_ir.clone();
        let mut annotations = base_annotations.clone();
        let result = crate::test_support::with_collection_limit(limit, |ctx| {
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            crate::families::standard::decode::partition_standard_face_components(
                ctx,
                &mut ir,
                &mut annotations,
                &components,
                &mut admission,
            )
        });
        if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = result {
            refusals.insert(refusal.operation);
        }
    }
    for operation in [
        "catia_standard_partition_region_ids",
        "catia_standard_partition_body_regions",
        "catia_standard_partition_face_ids",
        "catia_standard_partition_region_shells",
        "catia_standard_partition_regions",
        "catia_standard_partition_shells",
    ] {
        assert!(
            refusals.contains(operation),
            "missing charge for {operation}"
        );
    }
    let result = crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        crate::families::standard::decode::partition_standard_face_components(
            ctx,
            &mut base_ir,
            &mut base_annotations,
            &components,
            &mut admission,
        )
    })
    .expect("service context admits partitioned faces");
    assert!(result);
    assert_eq!(base_ir.model.regions.len(), 2);
    assert_eq!(
        base_ir.model.regions[1].id.as_str(),
        "catia:standard:region#0-1"
    );
    assert_eq!(
        base_ir.model.shells[1].id.as_str(),
        "catia:standard:shell#0-1"
    );
    assert_eq!(
        base_ir.model.shells[1].faces()[0].as_str(),
        "catia:standard:face#1"
    );
}

#[test]
fn standard_spline_retains_complete_surface_incidence_pair_domain() {
    let mut ir = CadIr::empty();
    for index in 0..138 {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#p{index}")).expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(f64::from(index), 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ));
    }
    for index in 0..2 {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("catia:test:surface#s{index}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });
    }
    let bindings = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            true,
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            true,
            0,
        ),
    ];
    let indices = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            1,
        ),
    ]
    .into_iter()
    .collect();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    for (limit, operation) in [
        (0, "catia_standard_resolved_endpoint_rows"),
        (1, "catia_standard_fallback_endpoint_pairs"),
    ] {
        assert!(matches!(
            crate::test_support::with_collection_limit(limit, |ctx| resolve_standard_endpoint_pairs(
                ctx, &ir, &bindings, &indices, std::slice::from_ref(&support), &[(0..138).collect()])),
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) if error.operation == operation
        ));
    }
    let choices = crate::test_support::with_service_context(|ctx| {
        resolve_standard_endpoint_pairs(
            ctx,
            &ir,
            &bindings,
            &indices,
            &[support],
            &[(0..138).collect()],
        )
    })
    .expect("service budget")
    .expect("endpoint option pass");
    assert_eq!(choices[0].len(), 9_453);
    assert_eq!(choices[0].first(), Some(&[0, 1]));
    assert_eq!(choices[0].last(), Some(&[136, 137]));
}

#[test]
fn standard_planar_intersection_spline_uses_the_common_line_domain() {
    let mut ir = CadIr::empty();
    for (index, position) in [
        Point3::new(-2.0, 0.0, 0.0),
        Point3::new(3.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, 0.0, 1.0),
    ]
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
    for (index, normal) in [Vector3::new(0.0, 0.0, 1.0), Vector3::new(0.0, 1.0, 0.0)]
        .into_iter()
        .enumerate()
    {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("catia:test:surface#s{index}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    normal,
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
    }
    let bindings = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            true,
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            true,
            0,
        ),
    ];
    let indices = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            1,
        ),
    ]
    .into_iter()
    .collect();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };

    let mut operations = HashSet::new();
    for limit in 0..=32 {
        match crate::test_support::with_collection_limit(limit, |ctx| {
            resolve_standard_endpoint_pairs(
                ctx,
                &ir,
                &bindings,
                &indices,
                std::slice::from_ref(&support),
                &[vec![0, 1, 2, 3]],
            )
        }) {
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) => {
                operations.insert(error.operation);
            }
            Ok(Some(_)) => break,
            outcome => panic!("unexpected planar endpoint result: {outcome:?}"),
        }
    }
    assert!(operations.contains("catia_standard_line_singleton_pair"));
    let choices = crate::test_support::with_service_context(|ctx| {
        resolve_standard_endpoint_pairs(
            ctx,
            &ir,
            &bindings,
            &indices,
            &[support],
            &[vec![0, 1, 2, 3]],
        )
    })
    .expect("service budget")
    .expect("endpoint option pass");

    assert_eq!(choices, [vec![[0, 1]]]);
}

#[test]
fn standard_antipodal_circle_candidates_admit_full_circle_seams() {
    let mut ir = CadIr::empty();
    for (index, position) in [Point3::new(5.0, 0.0, 0.0), Point3::new(-5.0, 0.0, 0.0)]
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
    for index in 0..2 {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("catia:test:surface#s{index}")).expect("identity grammar"),
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
    }
    let bindings = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            true,
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            true,
            0,
        ),
    ];
    let indices = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            1,
        ),
    ]
    .into_iter()
    .collect();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: super::super::checked_circle(Point3::new(0.0, 0.0, 0.0), 5.0),
    };

    for (limit, operation) in [
        (0, "catia_standard_resolved_endpoint_rows"),
        (1, "catia_standard_initial_endpoint_pair"),
        (2, "catia_standard_circle_endpoint_pairs"),
    ] {
        assert!(matches!(
            crate::test_support::with_collection_limit(limit, |ctx| resolve_standard_endpoint_pairs(
                ctx, &ir, &bindings, &indices, std::slice::from_ref(&support), &[vec![0, 1]])),
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) if error.operation == operation
        ));
    }
    let choices = crate::test_support::with_service_context(|ctx| {
        resolve_standard_endpoint_pairs(ctx, &ir, &bindings, &indices, &[support], &[vec![0, 1]])
    })
    .expect("service budget")
    .expect("endpoint option pass");

    assert_eq!(choices, [vec![[0, 0], [0, 1], [1, 1]]]);
}

#[test]
fn standard_parallel_line_rows_retain_domains_independent_of_allocation_order() {
    let mut ir = CadIr::empty();
    for (index, position) in [
        Point3::new(-2.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(-2.0, 0.0, 1.0),
        Point3::new(2.0, 0.0, 1.0),
    ]
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
    for index in 0..2 {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("catia:test:surface#s{index}")).expect("identity grammar"),
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
    }
    let bindings = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            true,
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            true,
            0,
        ),
    ];
    let indices = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            1,
        ),
    ]
    .into_iter()
    .collect();
    let supports = [(90, 900), (10, 100)].map(|(pos, tag)| StandardCurveSupport {
        pos,
        tag,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Line,
    });

    let mut operations = HashSet::new();
    for limit in 0..=32 {
        match crate::test_support::with_collection_limit(limit, |ctx| {
            resolve_standard_endpoint_pairs(
                ctx,
                &ir,
                &bindings,
                &indices,
                &supports,
                &[vec![0, 1, 2, 3], vec![0, 1, 2, 3]],
            )
        }) {
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) => {
                operations.insert(error.operation);
            }
            Ok(Some(_)) => break,
            outcome => panic!("unexpected parallel line endpoint result: {outcome:?}"),
        }
    }
    for operation in [
        "catia_standard_line_groups",
        "catia_standard_line_group_edges",
        "catia_standard_line_endpoint_pairs",
        "catia_standard_line_pair_copy",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
    let choices = crate::test_support::with_service_context(|ctx| {
        resolve_standard_endpoint_pairs(
            ctx,
            &ir,
            &bindings,
            &indices,
            &supports,
            &[vec![0, 1, 2, 3], vec![0, 1, 2, 3]],
        )
    })
    .expect("service budget")
    .expect("endpoint option pass");

    assert_eq!(choices, [vec![[0, 2], [1, 3]], vec![[0, 2], [1, 3]]]);
}

#[test]
fn line_pair_constraint_rejects_pairs_beyond_edge_roles() {
    let constraint = crate::test_support::with_service_context(|ctx| {
        super::super::super::StandardLinePairConstraint::new(ctx, &[], &[], &[])
    })
    .expect("empty constraint fits service budget");
    assert!(constraint.edge_pairs(&[None]).is_none());
}

#[test]
fn line_pair_constraint_refuses_collection_growth_before_face_edges() {
    let points = [Point::new(
        PointId::mint("catia:test:point#p0").expect("identity grammar"),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("finite point"),
        None,
    )];
    let point_refusal = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::super::StandardLinePairConstraint::new(ctx, &points, &[], &[])
    });
    assert!(
        matches!(point_refusal, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_standard_line_constraint_points")
    );
    let supports = [StandardCurveSupport {
        pos: 0,
        tag: 0,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Line,
    }];
    let options = [vec![[0, 1], [1, 2]]];
    let role_refusal = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::super::StandardLinePairConstraint::new(ctx, &[], &supports, &options)
    });
    assert!(
        matches!(role_refusal, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_standard_line_constraint_roles")
    );
    let face_refusal = crate::test_support::with_collection_limit(1, |ctx| {
        super::super::super::StandardLinePairConstraint::new(ctx, &[], &supports, &options)
    });
    assert!(
        matches!(face_refusal, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_standard_line_constraint_faces")
    );
    let refused = crate::test_support::with_collection_limit(2, |ctx| {
        super::super::super::StandardLinePairConstraint::new(ctx, &[], &supports, &options)
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_standard_line_constraint_face_edges")
    );
    crate::test_support::with_service_context(|ctx| {
        assert!(super::super::super::StandardLinePairConstraint::new(
            ctx,
            &[],
            &supports,
            &options
        )
        .is_ok());
    });
}

#[test]
fn circle_pair_constraint_refuses_nested_range_growth() {
    let supports = [StandardCurveSupport {
        pos: 0,
        tag: 0,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle {
            center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .expect("finite center"),
            radius: cadmpeg_ir::scalar::PositiveLength::new(1.0).expect("positive radius"),
        },
    }];
    let options = [vec![[0, 1], [1, 2]]];
    let face_refusal = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::super::StandardCirclePairConstraint::new(ctx, &supports, &options).map(|_| ())
    });
    assert!(
        matches!(face_refusal, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_standard_circle_constraint_faces")
    );
    let refused = crate::test_support::with_collection_limit(1, |ctx| {
        super::super::super::StandardCirclePairConstraint::new(ctx, &supports, &options).map(|_| ())
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_standard_circle_constraint_ranges")
    );
    crate::test_support::with_service_context(|ctx| {
        assert!(
            super::super::super::StandardCirclePairConstraint::new(ctx, &supports, &options)
                .is_ok()
        );
    });
}

/// A unit-radius cone about +Z whose cross-section radius overflows at
/// v = 1e308, with the pcurve that lifts onto its unit circle and the pcurve
/// that lifts to points without finite coordinates.
fn overflowing_cone_support(parameter_range: [f64; 2]) -> StandardEdgeSupport {
    let cone = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
            1.0,
            1.5,
        )
        .expect("valid ConeSurface fixture"),
    ));
    let line = |origin_v: f64| {
        PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.0, origin_v),
                Point2::new(1.0, 0.0),
            )
            .expect("valid LinePcurve fixture"),
        )
    };
    StandardEdgeSupport {
        surface_object_ids: [20, 21],
        carriers: [
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(cone.clone()),
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(cone),
        ],
        pcurves: [line(0.0), line(1.0e308)],
        parameter_range,
    }
}

#[test]
fn a_native_circle_range_reads_from_the_finite_support_when_its_partner_overflows() {
    let native = overflowing_cone_support([0.0, 1.5 * std::f64::consts::PI]);
    assert_eq!(
        native_support_circle_param_range(
            &cadmpeg_test_support::service_decode_context(),
            &native,
            Point3::new(0.0, 0.0, 0.0),
            1.0,
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            [Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, -1.0, 0.0)],
        ),
        Ok(Some([0.0, 1.5 * std::f64::consts::PI]))
    );
}

#[test]
fn a_native_endpoint_pair_reads_from_the_finite_support_when_its_partner_overflows() {
    let native = overflowing_cone_support([1.0, 4.0]);
    let points = [1.0_f64, 4.0]
        .into_iter()
        .enumerate()
        .map(|(index, angle)| {
            Point::new(
                PointId::mint(format!("catia:test:point#overflow-{index}"))
                    .expect("identity grammar"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(angle.cos(), angle.sin(), 0.0))
                    .expect("a finite position is a point"),
                None,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        standard_native_support_endpoint_pair(
            &cadmpeg_test_support::service_decode_context(),
            &native,
            &points,
            &[0, 1],
            None
        ),
        Ok(Some([0, 1]))
    );
}

#[test]
fn a_native_endpoint_pair_reads_from_the_finite_support_when_its_placed_partner_overflows() {
    let mut native = overflowing_cone_support([1.0, 4.0]);
    native.carriers = native.carriers.map(|carrier| match carrier {
        crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
            SurfaceGeometry::Solved(cone),
        ) => crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
                cadmpeg_ir::geometry::PlacedSurface::try_new(
                    Box::new(cone),
                    cadmpeg_ir::transform::Transform::identity(),
                )
                .expect("valid PlacedSurface fixture"),
            )),
        ),
        carrier => carrier,
    });
    let points = [1.0_f64, 4.0]
        .into_iter()
        .enumerate()
        .map(|(index, angle)| {
            Point::new(
                PointId::mint(format!("catia:test:point#placed-overflow-{index}"))
                    .expect("identity grammar"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(angle.cos(), angle.sin(), 0.0))
                    .expect("a finite position is a point"),
                None,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        standard_native_support_endpoint_pair(
            &cadmpeg_test_support::service_decode_context(),
            &native,
            &points,
            &[0, 1],
            None
        ),
        Ok(Some([0, 1]))
    );
}
