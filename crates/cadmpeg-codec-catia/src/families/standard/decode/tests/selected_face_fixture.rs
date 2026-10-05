// SPDX-License-Identifier: Apache-2.0
//! Serialized endpoint sources for repeated-face admission tests.

use crate::families::standard::decode::{
    attach_standard_topology, AttachStandardTopologyInputs, EdgeTableForm, FamilyEntityAdmission,
    StandardTopologyDiagnostics, StandardTopologyError, StandardTopologyFailure,
};
use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{FaceId, PointId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Face, FaceLoops, Point, Sense};
use cadmpeg_ir::AnnotationBuilder;
use std::collections::HashMap;

fn repeated_triangle_spine() -> Vec<u8> {
    let mut bytes = Vec::new();
    let boundary = [30u16, 10, 20, 31, 11, 21, 32, 12, 22];
    for face in 0u16..3 {
        bytes.extend_from_slice(&[0x01, 0x44, 0x01, 0xff, 11, 0, 0, 0, 11]);
        bytes.extend_from_slice(&(500 + face).to_be_bytes());
        for handle in boundary {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
        bytes.extend_from_slice(&boundary[0].to_be_bytes());
    }
    for _ in 0..3 {
        bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    }
    bytes.extend_from_slice(&[0x01, 0x01, 6]);
    for row in [
        [100u16, 10, 20, 101],
        [100, 10, 20, 101],
        [101, 11, 21, 102],
        [101, 11, 21, 102],
        [102, 12, 22, 100],
        [102, 12, 22, 100],
    ] {
        bytes.extend_from_slice(&[0x02, 4]);
        for handle in row {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    bytes.extend_from_slice(&[0x01, 0x06, 3]);
    for position in [
        [0.0_f32, 0.0, 0.0],
        [1.0_f32, 0.0, 0.0],
        [0.0_f32, 1.0, 0.0],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for coordinate in position {
            bytes.extend_from_slice(&coordinate.to_le_bytes());
        }
    }
    bytes
}

fn repeated_triangle_endpoint_source() -> Vec<u8> {
    use crate::test_support::test_b5::{append_b5_record, b5_object_ref};

    let mut source = Vec::new();
    for identity in 100u32..103 {
        source.push(0x54);
        source.extend_from_slice(&identity.to_le_bytes()[..3]);
        source.extend_from_slice(&[0, 0, 0]);
    }
    for (edge, [start, end]) in [
        (1u32, [100u32, 101u32]),
        (2, [100, 101]),
        (3, [101, 102]),
        (4, [101, 102]),
        (5, [102, 100]),
        (6, [102, 100]),
    ] {
        let mut payload = vec![0x85];
        payload.extend_from_slice(&b5_object_ref(900));
        payload.extend_from_slice(&b5_object_ref(start));
        payload.extend_from_slice(&b5_object_ref(end));
        payload.extend_from_slice(&b5_object_ref(901));
        payload.extend_from_slice(&b5_object_ref(902));
        payload.push(0x2a);
        append_b5_record(&mut source, 0x5e, edge, &payload);
    }
    source
}

struct RepeatedTriangleAttach {
    outcome: Result<(), StandardTopologyFailure>,
    diagnostics: StandardTopologyDiagnostics,
    face_count: usize,
}

fn repeated_triangle_fixture(
    ctx: &DecodeContext<'_>,
) -> Result<RepeatedTriangleAttach, CodecError> {
    let mut ir = CadIr::empty();
    let shell = ShellId::mint("catia:test:shell#repeated-face-domain").expect("identity grammar");
    let surface_ids = [
        SurfaceId::mint("catia:test:surface#repeated-face-domain-0").expect("identity grammar"),
        SurfaceId::mint("catia:test:surface#repeated-face-domain-1").expect("identity grammar"),
        SurfaceId::mint("catia:test:surface#repeated-face-domain-2").expect("identity grammar"),
    ];
    let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid repeated-domain plane"),
    ));
    for (face, surface_id) in surface_ids.iter().cloned().enumerate() {
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: plane.clone(),
            source_object: None,
        });
        ir.model.faces.push(Face {
            id: FaceId::mint(format!("catia:standard:face#repeated-face-domain-{face}"))
                .expect("identity grammar"),
            shell: shell.clone(),
            surface: surface_id,
            sense: Sense::Forward,
            loops: FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        });
    }
    for (point, position) in [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#repeated-face-domain-{point}"))
                .expect("identity grammar"),
            FinitePoint3::new(position).expect("finite repeated-domain point"),
            None,
        ));
    }

    let bindings = [
        (surface_ids[0].clone(), false, 0),
        (surface_ids[1].clone(), false, 0),
        (surface_ids[2].clone(), false, 0),
    ];
    let records = [];
    let face_bounds = [None, None, None];
    // Each face repeats the same three edge interiors. Every interior then
    // occurs on a third face outside each candidate's two-face incidence.
    let supports = [
        StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 0],
            geometry: StandardCurveGeometry::Bspline,
        },
        StandardCurveSupport {
            pos: 1,
            tag: 2,
            faces: [1, 1],
            geometry: StandardCurveGeometry::Bspline,
        },
        StandardCurveSupport {
            pos: 2,
            tag: 3,
            faces: [0, 0],
            geometry: StandardCurveGeometry::Bspline,
        },
        StandardCurveSupport {
            pos: 3,
            tag: 4,
            faces: [2, 2],
            geometry: StandardCurveGeometry::Bspline,
        },
        StandardCurveSupport {
            pos: 4,
            tag: 5,
            faces: [1, 1],
            geometry: StandardCurveGeometry::Bspline,
        },
        StandardCurveSupport {
            pos: 5,
            tag: 6,
            faces: [2, 2],
            geometry: StandardCurveGeometry::Bspline,
        },
    ];
    let spine = repeated_triangle_spine();
    let source = repeated_triangle_endpoint_source();
    let native_edge_faces = HashMap::<u32, Vec<u32>>::new();
    let native_edge_supports = HashMap::new();
    let mut annotations = AnnotationBuilder::new();
    let work_budget = ctx.work_budget(1_000_000);
    let mut diagnostics = StandardTopologyDiagnostics::default();
    let mut bound_limit_curve_count = 0;
    let mut refusal = crate::nurbs::LaneRefusals::new();
    let mut admission = FamilyEntityAdmission::new(ctx);

    let outcome = attach_standard_topology(
        ctx,
        AttachStandardTopologyInputs {
            ir: &mut ir,
            annotations: &mut annotations,
            bindings: &bindings,
            records: &records,
            face_bounds: &face_bounds,
            spine: &spine,
            edge_table_form: EdgeTableForm::Standard,
            brep: &[],
            support_override: Some(&supports),
            source: &source,
            e5_record_range: None,
            use_vertex_roster: true,
            native_edge_faces: &native_edge_faces,
            native_edge_supports: &native_edge_supports,
            limit_curves: &[],
            work_budget: &work_budget,
            diagnostics: &mut diagnostics,
            bound_limit_curve_count: &mut bound_limit_curve_count,
            refusal: &mut refusal,
            admission: &mut admission,
        },
    );
    let outcome = match outcome {
        Ok(()) => Ok(()),
        Err(StandardTopologyError::Semantic(failure)) => Err(failure),
        Err(StandardTopologyError::Resource(error)) => return Err(error),
    };
    Ok(RepeatedTriangleAttach {
        outcome,
        diagnostics,
        face_count: ir.model.faces.len(),
    })
}

pub(super) fn assert_repeated_triangle_work_refusal(operation: &'static str) {
    let service = crate::test_support::with_service_context(|ctx| {
        let source = repeated_triangle_endpoint_source();
        assert_eq!(
            crate::families::standard::records::standard_vertex_roster(ctx, &source, 3)?,
            Some(vec![100, 101, 102])
        );
        let edge_vertices = crate::families::b5::graph::edge_vertex_references(ctx, &source)?;
        assert_eq!(edge_vertices.len(), 6);
        assert_eq!(edge_vertices.get(&1), Some(&[100, 101]));
        assert_eq!(edge_vertices.get(&6), Some(&[102, 100]));
        repeated_triangle_fixture(ctx)
    })
    .expect("service profile admits a populated repeated-domain topology");
    assert_eq!(
        service.outcome,
        Err(StandardTopologyFailure::NoTopologySolution),
        "each repeated-triangle edge interior occurs on a third face outside its two-face incidence"
    );
    assert_eq!(service.face_count, 3);
    assert_eq!(service.diagnostics.curve_supports, 6);
    assert_eq!(service.diagnostics.native_endpoint_pairs, 6);
    assert!(service.diagnostics.endpoint_domain_choices > 0);
    // Each candidate assigns every repeated row to at most two faces, while
    // every trim cycle contains every row's interior handle. The third-face
    // occurrence makes mesh_face_coverage return None, which the mesh solver
    // classifies as an input-structure rejection.
    assert_eq!(
        service.diagnostics.mesh_failure,
        Some(crate::solve::mesh_quotient::MeshCandidateFailure::Rejected(
            crate::solve::mesh_quotient::MeshCandidateRejection::InputStructure,
        )),
        "each candidate omits a face that contains a matched edge interior"
    );

    let mut cap = 0;
    for _ in 0..8192 {
        let (refusal, need) =
            crate::test_support::with_work_limit(cap, |ctx| match repeated_triangle_fixture(ctx) {
                Err(CodecError::ResourceLimit(refusal)) => {
                    assert_eq!(
                        refusal.dimension,
                        cadmpeg_core::decode::ResourceDimension::WorkUnits
                    );
                    assert_eq!(ctx.resource_refusal(), Some(refusal));
                    let need = refusal
                        .used
                        .checked_add(refusal.additional)
                        .expect("resource need fits");
                    assert!(need > cap, "{operation}: {refusal:?}");
                    (refusal, need)
                }
                Err(error) => panic!("unexpected refusal before {operation}: {error:?}"),
                Ok(_) => panic!("repeated-domain input did not reach {operation}"),
            });
        if refusal.operation == operation {
            assert_eq!(
                refusal.dimension,
                cadmpeg_core::decode::ResourceDimension::WorkUnits
            );
            return;
        }
        cap = need;
    }
    panic!("resource route exceeds the boundary count: {operation}");
}
