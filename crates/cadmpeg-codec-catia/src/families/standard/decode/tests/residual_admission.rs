// SPDX-License-Identifier: Apache-2.0
use crate::families::standard::decode::{
    apply_standard_native_edge_faces, corroborate_successor_endpoint_points,
    include_native_endpoint_pairs, refine_consolidated_analytic_surfaces,
    refine_repeated_face_domains_by_geometry_and_bounds, resolve_standard_endpoint_pairs,
    standard_face_point_membership, AttachStandardTopologyInputs, EdgeTableForm,
    FamilyEntityAdmission, StandardTopologyDiagnostics, StandardTopologyError,
    StandardTopologyFailure,
};
use crate::families::standard::records::{
    AnalyticSurfaceKind, StandardCurveGeometry, StandardCurveSupport, StandardFaceBounds,
    StandardSurfaceRecord, SurfacePrefix,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::HashMap;

fn tetrahedron_with_endpoint_roster_and_b5_edges() -> Vec<u8> {
    use crate::test_support::test_b5::{append_b5_record, b5_object_ref};

    let original = crate::test_support::test_container::tetrahedron_topology_catpart();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, &original).expect("service profile admits tetrahedron")
    });
    let main = scan
        .main_data_stream
        .as_deref()
        .expect("tetrahedron owns a standard MainDataStream");
    let brep = scan
        .brep
        .as_deref()
        .expect("tetrahedron owns a BREP stream");
    assert!(brep.starts_with(main));
    let surf = &brep[main.len()..];

    let mut main = main.to_vec();
    for identity in 100u32..104 {
        main.push(0x54);
        main.extend_from_slice(&identity.to_le_bytes()[..3]);
        main.extend_from_slice(&[0, 0, 0]);
    }
    for (edge, [start, end]) in [
        (1u32, [100u32, 101u32]),
        (2, [101, 102]),
        (3, [102, 100]),
        (4, [100, 103]),
        (5, [101, 103]),
        (6, [102, 103]),
    ] {
        let mut payload = vec![0x85];
        payload.extend_from_slice(&b5_object_ref(900));
        payload.extend_from_slice(&b5_object_ref(start));
        payload.extend_from_slice(&b5_object_ref(end));
        payload.extend_from_slice(&b5_object_ref(901));
        payload.extend_from_slice(&b5_object_ref(902));
        payload.push(0x2a);
        append_b5_record(&mut main, 0x5e, edge, &payload);
    }

    crate::test_support::test_container::standard_catpart_from_streams(&main, surf)
}

fn tetrahedron_with_open_tag_six_and_b5_roster() -> Vec<u8> {
    let bytes = tetrahedron_with_endpoint_roster_and_b5_edges();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, &bytes)
            .expect("service profile admits the endpoint tetrahedron")
    });
    let main_stream = scan
        .main_data_stream
        .as_deref()
        .expect("tetrahedron owns a standard MainDataStream");
    let brep = scan
        .brep
        .as_deref()
        .expect("tetrahedron owns a BREP stream");
    assert!(brep.starts_with(main_stream));
    let surf = brep[main_stream.len()..].to_vec();
    let mut main = main_stream.to_vec();
    let row = [0x02, 4, 0, 102, 0, 15, 0, 25, 0, 103];
    let offsets = main
        .windows(row.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == row.as_slice()).then_some(offset))
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 1, "one tag-six edge row is serialized");
    main[offsets[0] + 5] = 16;
    crate::test_support::test_container::standard_catpart_from_streams(&main, &surf)
}

fn coplanar_tetrahedron_with_repeated_edge_rows() -> Vec<u8> {
    use crate::test_support::test_b5::{append_b5_record, b5_object_ref};

    let original = crate::test_support::test_container::tetrahedron_topology_catpart();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, &original)
            .expect("service profile admits the tetrahedron source")
    });
    let main_stream = scan
        .main_data_stream
        .as_deref()
        .expect("tetrahedron owns a standard MainDataStream");
    let brep = scan
        .brep
        .as_deref()
        .expect("tetrahedron owns a BREP stream");
    assert!(brep.starts_with(main_stream));
    let surf = brep[main_stream.len()..].to_vec();
    let mut main = main_stream.to_vec();

    let point_rows = main
        .windows(3)
        .enumerate()
        .filter_map(|(offset, window)| (window == [0x05, 0x08, 0x01].as_slice()).then_some(offset))
        .collect::<Vec<_>>();
    assert_eq!(
        point_rows.len(),
        4,
        "tetrahedron serializes four point rows"
    );
    for (offset, position) in point_rows.into_iter().zip([
        [0.0_f32, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ]) {
        for (axis, coordinate) in position.into_iter().enumerate() {
            let start = offset + 3 + axis * 4;
            main[start..start + 4].copy_from_slice(&coordinate.to_le_bytes());
        }
    }

    for row in [
        [0x02, 4, 0, 102, 0, 12, 0, 22, 0, 100],
        [0x02, 4, 0, 100, 0, 13, 0, 23, 0, 103],
        [0x02, 4, 0, 101, 0, 14, 0, 24, 0, 103],
        [0x02, 4, 0, 102, 0, 15, 0, 25, 0, 103],
    ] {
        let offsets = main
            .windows(row.len())
            .enumerate()
            .filter_map(|(offset, window)| (window == row.as_slice()).then_some(offset))
            .collect::<Vec<_>>();
        assert_eq!(offsets.len(), 1, "one selected edge row is serialized");
        main[offsets[0] + 4..offsets[0] + 6].copy_from_slice(&0x7ffe_u16.to_be_bytes());
    }

    // Edge two remains exact on face two only; face zero uses a distinct trim handle.
    let face_zero_packet = [
        0x01, 0x44, 0x01, 0xff, 11, 0, 0, 0, 11, 0x01, 0xf4, 0x00, 0x1e, 0x00, 0x0a, 0x00, 0x14,
        0x00, 0x1f, 0x00, 0x0b, 0x00, 0x15, 0x00, 0x20, 0x00, 0x0c, 0x00, 0x16, 0x00, 0x1e,
    ];
    let face_zero_offsets = main
        .windows(face_zero_packet.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == face_zero_packet).then_some(offset))
        .collect::<Vec<_>>();
    assert_eq!(
        face_zero_offsets.len(),
        1,
        "face zero owns its serialized trim packet"
    );
    let edge_two_handle = face_zero_offsets[0] + 9 + 2 + 4 * 2;
    main[edge_two_handle..edge_two_handle + 2].copy_from_slice(&0x7ffd_u16.to_be_bytes());

    for identity in 100u32..104 {
        main.push(0x54);
        main.extend_from_slice(&identity.to_le_bytes()[..3]);
        main.extend_from_slice(&[0, 0, 0]);
    }
    for (edge, [start, end]) in [
        (1u32, [100u32, 101u32]),
        (2, [101, 102]),
        (3, [102, 100]),
        (4, [100, 103]),
        (5, [101, 103]),
        (6, [102, 103]),
    ] {
        let mut payload = vec![0x85];
        payload.extend_from_slice(&b5_object_ref(900));
        payload.extend_from_slice(&b5_object_ref(start));
        payload.extend_from_slice(&b5_object_ref(end));
        payload.extend_from_slice(&b5_object_ref(901));
        payload.extend_from_slice(&b5_object_ref(902));
        payload.push(0x2a);
        append_b5_record(&mut main, 0x5e, edge, &payload);
    }

    crate::test_support::test_container::standard_catpart_from_streams(&main, &surf)
}

#[derive(Clone, Copy)]
enum TetrahedronAttachShape {
    OneOpenTagSix,
    CoplanarRepeatedRows,
}

struct TetrahedronAttachResult {
    outcome: Result<(), StandardTopologyFailure>,
    diagnostics: StandardTopologyDiagnostics,
    face_count: usize,
    edge_count: usize,
    point_count: usize,
}

struct TetrahedronAttachFixture {
    ir: CadIr,
    bindings: Vec<(SurfaceId, bool, usize)>,
    spine: Vec<u8>,
    brep: Vec<u8>,
    source: Vec<u8>,
    supports: Vec<StandardCurveSupport>,
}

impl TetrahedronAttachFixture {
    fn from_bytes(decoded: &[u8], attached: &[u8], shape: TetrahedronAttachShape) -> Self {
        let (mut ir, bindings) = crate::test_support::with_service_context(|ctx| {
            let scan = crate::container::scan_bytes(ctx, decoded)
                .expect("service profile admits the endpoint tetrahedron");
            let output = crate::families::standard::decode::try_decode_standard(
                ctx,
                &scan,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service profile admits tetrahedron decode")
            .expect("tetrahedron selects the standard route");
            assert_eq!(output.ir.model.faces.len(), 4);
            assert_eq!(output.ir.model.edges.len(), 6);
            assert_eq!(output.ir.model.points.len(), 4);
            let bindings: Vec<(SurfaceId, bool, usize)> = output
                .ir
                .model
                .faces
                .iter()
                .enumerate()
                .map(|(face, value)| (value.surface.clone(), false, face))
                .collect();
            let mut ir = output.ir;
            for face in &mut ir.model.faces {
                face.loops = cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new());
            }
            // Keep the owner arenas used when the solved face components are partitioned.
            ir.model.loops.clear();
            ir.model.coedges.clear();
            ir.model.edges.clear();
            ir.model.vertices.clear();
            ir.model.curves.clear();
            ir.model.procedural_curves.clear();
            ir.model.pcurves.clear();
            (ir, bindings)
        });

        let face_vertices = [[0usize, 1, 2], [0, 3, 1], [1, 3, 2], [2, 3, 0]];
        if matches!(shape, TetrahedronAttachShape::CoplanarRepeatedRows) {
            for (point, position) in ir.model.points.iter_mut().zip([
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ]) {
                point.set_position(FinitePoint3::new(position).expect("finite square point"));
            }
        }
        let face_planes = face_vertices.map(|[a, b, c]| {
            let origin = ir.model.points[a].position().get();
            let first = ir.model.points[b].position().get().vector_from(origin);
            let second = ir.model.points[c].position().get().vector_from(origin);
            let (normal, u_axis) = if matches!(shape, TetrahedronAttachShape::CoplanarRepeatedRows)
            {
                (Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0))
            } else {
                (
                    first
                        .cross(second)
                        .unit()
                        .expect("tetrahedron face is nondegenerate"),
                    first.unit().expect("tetrahedron face edge is nonzero"),
                )
            };
            PlaneSurface::try_new(origin, normal, u_axis)
                .expect("tetrahedron fixture plane has an orthonormal frame")
        });
        for left in 0..bindings.len() {
            for right in left + 1..bindings.len() {
                assert_ne!(
                    bindings[left].0, bindings[right].0,
                    "each tetrahedron face has its own bound surface"
                );
            }
        }
        for (surface_id, _, face) in &bindings {
            let Some(surface) = ir
                .model
                .surfaces
                .iter_mut()
                .find(|surface| &surface.id == surface_id)
            else {
                panic!("face binding owns a serialized surface");
            };
            surface.geometry =
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(face_planes[*face]));
        }

        let scan = crate::test_support::with_service_context(|ctx| {
            crate::container::scan_bytes(ctx, attached)
                .expect("service profile admits the topology attach source")
        });
        let spine = scan
            .main_data_stream
            .as_deref()
            .expect("topology attach source has a standard spine")
            .to_vec();
        let brep = scan
            .brep
            .as_deref()
            .expect("topology attach source has a BREP")
            .to_vec();
        let source = scan.data.to_vec();
        let mut supports = vec![
            StandardCurveSupport {
                pos: 0,
                tag: 1,
                faces: [0, 1],
                geometry: StandardCurveGeometry::Bspline,
            },
            StandardCurveSupport {
                pos: 1,
                tag: 2,
                faces: [0, 2],
                geometry: StandardCurveGeometry::Bspline,
            },
            StandardCurveSupport {
                pos: 2,
                tag: 3,
                faces: [0, 3],
                geometry: StandardCurveGeometry::Bspline,
            },
            StandardCurveSupport {
                pos: 3,
                tag: 4,
                faces: [1, 3],
                geometry: StandardCurveGeometry::Bspline,
            },
            StandardCurveSupport {
                pos: 4,
                tag: 5,
                faces: [1, 2],
                geometry: StandardCurveGeometry::Bspline,
            },
            StandardCurveSupport {
                pos: 5,
                tag: 6,
                faces: [2, 3],
                geometry: StandardCurveGeometry::Bspline,
            },
        ];
        match shape {
            TetrahedronAttachShape::OneOpenTagSix => supports[5].faces = [2, 2],
            TetrahedronAttachShape::CoplanarRepeatedRows => {
                for (support, faces) in
                    supports
                        .iter_mut()
                        .zip([[0, 0], [0, 0], [0, 0], [1, 1], [1, 1], [2, 2]])
                {
                    support.faces = faces;
                }
            }
        }
        Self {
            ir,
            bindings,
            spine,
            brep,
            source,
            supports,
        }
    }

    fn attach(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<TetrahedronAttachResult, CodecError> {
        let mut ir = self.ir.clone();
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let records = [];
        let face_bounds = [];
        let native_edge_faces = HashMap::new();
        let native_edge_supports = HashMap::new();
        let limit_curves = [];
        let work_budget = ctx.work_budget(1_000_000);
        let mut diagnostics = StandardTopologyDiagnostics::default();
        let mut bound_limit_curve_count = 0;
        let mut refusal = crate::nurbs::LaneRefusals::new();
        let mut admission = FamilyEntityAdmission::new(ctx);
        let outcome = crate::families::standard::decode::attach_standard_topology(
            ctx,
            AttachStandardTopologyInputs {
                ir: &mut ir,
                annotations: &mut annotations,
                bindings: &self.bindings,
                records: &records,
                face_bounds: &face_bounds,
                spine: &self.spine,
                edge_table_form: EdgeTableForm::Standard,
                brep: &self.brep,
                support_override: Some(&self.supports),
                source: &self.source,
                e5_record_range: None,
                use_vertex_roster: true,
                native_edge_faces: &native_edge_faces,
                native_edge_supports: &native_edge_supports,
                limit_curves: &limit_curves,
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
        Ok(TetrahedronAttachResult {
            outcome,
            diagnostics,
            face_count: ir.model.faces.len(),
            edge_count: ir.model.edges.len(),
            point_count: ir.model.points.len(),
        })
    }
}

fn assert_tetrahedron_attach_work_refusal(operation: &'static str, shape: TetrahedronAttachShape) {
    let decoded = tetrahedron_with_endpoint_roster_and_b5_edges();
    let attached = match shape {
        TetrahedronAttachShape::OneOpenTagSix => tetrahedron_with_open_tag_six_and_b5_roster(),
        TetrahedronAttachShape::CoplanarRepeatedRows => {
            coplanar_tetrahedron_with_repeated_edge_rows()
        }
    };
    let fixture = TetrahedronAttachFixture::from_bytes(&decoded, &attached, shape);
    assert_work_refusal(
        operation,
        |ctx| fixture.attach(ctx),
        |result| {
            assert_eq!(
                result.outcome,
                Ok(()),
                "service fixture must attach topology: supports={}, native_pairs={}, domains={}, mesh={:?}",
                result.diagnostics.curve_supports,
                result.diagnostics.native_endpoint_pairs,
                result.diagnostics.endpoint_domain_choices,
                result.diagnostics.mesh_failure,
            );
            assert_eq!(result.face_count, 4);
            assert_eq!(result.edge_count, 6);
            assert_eq!(result.point_count, 4);
            assert_eq!(result.diagnostics.curve_supports, 6);
            assert_eq!(result.diagnostics.native_endpoint_pairs, 6);
            assert!(result.diagnostics.endpoint_domain_choices > 0);
            assert_eq!(result.diagnostics.mesh_failure, None);
        },
    );
}

fn assert_work_refusal<T>(
    operation: &'static str,
    mut run: impl FnMut(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, CodecError>,
    assert_service_output: impl FnOnce(T),
) {
    let service_output = crate::test_support::with_service_context(|ctx| {
        let output = run(ctx);
        assert_eq!(ctx.resource_refusal(), None);
        output
    })
    .unwrap_or_else(|error| panic!("service fixture failed before {operation}: {error}"));
    assert_service_output(service_output);

    walk_to_work_refusal(operation, run);
}

/// Finds the work cap at which `operation` refuses.
pub(super) fn walk_to_work_refusal<T>(
    operation: &'static str,
    mut run: impl FnMut(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, CodecError>,
) {
    use cadmpeg_core::decode::ResourceDimension;

    // A topology search slice charges the session, and a slice the session
    // cannot pay ends its search without an error, so a capped run can take
    // another route than an uncapped one. The refusal probe finds the uncapped
    // boundary in one run; the walk then raises the cap one refusal at a time
    // from there until the named operation refuses.
    let mut cap = {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            operation,
            None,
        );
        match crate::test_support::with_work_limit(u64::MAX, |ctx| run(ctx)) {
            Err(CodecError::ResourceLimit(refusal)) if refusal.operation == operation => {
                refusal.used
            }
            Err(error) => panic!("unexpected refusal before {operation}: {error}"),
            Ok(_) => panic!("fixture did not reach {operation}"),
        }
    };
    for _ in 0..4096 {
        let (refusal, need) = crate::test_support::with_work_limit(cap, |ctx| match run(ctx) {
            Err(CodecError::ResourceLimit(refusal)) => {
                assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                assert_eq!(ctx.resource_refusal(), Some(refusal));
                let need = refusal
                    .used
                    .checked_add(refusal.additional)
                    .expect("resource need fits");
                assert!(need > cap, "{operation}: {refusal:?}");
                (refusal, need)
            }
            Err(error) => panic!("unexpected error before {operation}: {error}"),
            Ok(_) => panic!("fixture did not reach {operation} under a {cap} work cap"),
        });
        if refusal.operation == operation {
            return;
        }
        cap = need;
    }
    panic!("{operation}: the walk from the uncapped boundary did not reach the operation");
}

#[test]
fn refined_surface_record_slots_propagate_work_refusal() {
    assert_work_refusal(
        "catia_standard_refined_surface_slots",
        |ctx| {
            let surface_records = [StandardSurfaceRecord::Analytic(SurfacePrefix {
                pos: 0,
                target: 1,
                kind: AnalyticSurfaceKind::Plane,
            })];
            let mut surfaces = [None];
            refine_consolidated_analytic_surfaces(ctx, &[], &[], &mut surfaces, &surface_records)
        },
        |refined| assert!(refined.is_empty()),
    );
}

#[test]
fn native_edge_face_support_rows_propagate_work_refusal() {
    assert_work_refusal(
        "catia_standard_native_edge_face_supports",
        |ctx| {
            let supports = [StandardCurveSupport {
                pos: 0,
                tag: 1,
                faces: [0, 0],
                geometry: StandardCurveGeometry::Line,
            }];
            let mut edge_faces = [[0, 0]];
            apply_standard_native_edge_faces(ctx, &mut edge_faces, &supports, &[], &HashMap::new())
                .map(|()| edge_faces)
        },
        |edge_faces| assert_eq!(edge_faces, [[0, 0]]),
    );
}

#[test]
fn native_edge_face_assignment_rows_propagate_work_refusal() {
    super::selected_face_fixture::assert_repeated_triangle_work_refusal(
        "catia_standard_native_edge_face_assignments",
    );
}

#[test]
fn native_endpoint_evidence_rows_propagate_work_refusal() {
    let bytes = tetrahedron_with_endpoint_roster_and_b5_edges();
    crate::test_support::with_service_context(|ctx| {
        let scan = crate::container::scan_bytes(ctx, bytes.clone())
            .expect("service profile admits the compatible endpoint fixture");
        assert_eq!(
            crate::families::standard::records::standard_vertex_roster(ctx, &scan.data, 4,)
                .expect("service budget admits roster scan"),
            Some(vec![100, 101, 102, 103])
        );
        let edges = crate::families::b5::graph::edge_vertex_references(ctx, &scan.data)
            .expect("service budget admits B5 edge scan");
        assert_eq!(edges.len(), 6);
        assert_eq!(edges.get(&1), Some(&[100, 101]));
        assert_eq!(edges.get(&6), Some(&[102, 103]));
        let output = crate::families::standard::decode::try_decode_standard(
            ctx,
            &scan,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service budget admits standard decode")
        .expect("the endpoint fixture reaches standard topology");
        assert_eq!(output.ir.model.faces.len(), 4);
        assert_eq!(output.ir.model.edges.len(), 6);
        assert_eq!(
            output
                .report
                .coverage
                .get("attached_standard_topology_count"),
            Some(&1),
            "tetrahedron service coverage: {:#?}",
            output.report.coverage
        );
        assert_eq!(
            output
                .report
                .coverage
                .get("standard_topology_native_endpoint_pair_count"),
            Some(&6),
            "tetrahedron service coverage: {:#?}",
            output.report.coverage
        );
        assert!(
            output
                .report
                .coverage
                .get("standard_topology_endpoint_domain_choice_count")
                .is_some_and(|count| *count > 0),
            "tetrahedron service coverage: {:#?}",
            output.report.coverage
        );
        assert!(output
            .ir
            .model
            .points
            .iter()
            .all(|point| point.source_object.is_some()));
    });
    super::selected_face_fixture::assert_repeated_triangle_work_refusal(
        "catia_standard_native_endpoint_evidence_rows",
    );
}

#[test]
fn propagated_endpoint_rows_propagate_work_refusal() {
    super::selected_face_fixture::assert_repeated_triangle_work_refusal(
        "catia_standard_propagated_endpoint_rows",
    );
}

#[test]
fn completed_edge_face_rows_propagate_work_refusal() {
    assert_tetrahedron_attach_work_refusal(
        "catia_standard_completed_edge_faces",
        TetrahedronAttachShape::OneOpenTagSix,
    );
}

#[test]
fn endpoint_option_support_rows_propagate_work_refusal() {
    super::selected_face_fixture::assert_repeated_triangle_work_refusal(
        "catia_standard_endpoint_option_supports",
    );
}

#[test]
fn endpoint_option_filter_support_rows_propagate_work_refusal() {
    super::selected_face_fixture::assert_repeated_triangle_work_refusal(
        "catia_standard_endpoint_option_filter_supports",
    );
}

#[test]
fn endpoint_candidate_option_rows_propagate_work_refusal() {
    super::selected_face_fixture::assert_repeated_triangle_work_refusal(
        "catia_standard_endpoint_candidate_option_rows",
    );
}

#[test]
fn unique_endpoint_option_rows_propagate_work_refusal() {
    assert_tetrahedron_attach_work_refusal(
        "catia_standard_unique_endpoint_option_rows",
        TetrahedronAttachShape::OneOpenTagSix,
    );
}

#[test]
fn selected_face_support_rows_propagate_work_refusal() {
    assert_tetrahedron_attach_work_refusal(
        "catia_standard_selected_face_supports",
        TetrahedronAttachShape::CoplanarRepeatedRows,
    );
}

#[test]
fn resolved_endpoint_candidate_rows_propagate_work_refusal() {
    assert_work_refusal(
        "catia_standard_resolved_endpoint_candidate_rows",
        |ctx| {
            let ir = CadIr::empty();
            let candidates = [Vec::new()];
            resolve_standard_endpoint_pairs(ctx, &ir, &[], &HashMap::new(), &[], &candidates)
        },
        |resolved| assert_eq!(resolved, Some(vec![Vec::new()])),
    );
}

#[test]
fn native_endpoint_candidate_rows_propagate_work_refusal() {
    assert_work_refusal(
        "catia_native_endpoint_candidate_rows",
        |ctx| {
            let mut candidates = [Vec::new()];
            let pairs = [Some([0, 1])];
            include_native_endpoint_pairs(ctx, &mut candidates, &pairs).map(|()| candidates)
        },
        |candidates| assert_eq!(candidates, [vec![0, 1]]),
    );
}

#[test]
fn successor_endpoint_evidence_rows_propagate_work_refusal() {
    assert_work_refusal(
        "catia_successor_endpoint_evidence_rows",
        |ctx| {
            let mut options = [vec![[0, 1]]];
            let points = [[Some(0), None]];
            corroborate_successor_endpoint_points(ctx, &mut options, &points).map(|()| options)
        },
        |options| assert_eq!(options, [vec![[0, 1]]]),
    );
}

#[test]
fn face_membership_binding_rows_propagate_work_refusal() {
    assert_work_refusal(
        "catia_face_membership_binding_rows",
        |ctx| {
            let ir = CadIr::empty();
            let bindings = [(
                SurfaceId::mint("catia:test:surface#missing").expect("identity grammar"),
                false,
                0,
            )];
            standard_face_point_membership(ctx, &ir, &bindings, &HashMap::new(), None)
        },
        |memberships| assert_eq!(memberships, [Vec::<bool>::new()]),
    );
}

#[test]
fn repeated_face_geometry_rows_propagate_work_refusal() {
    assert_work_refusal(
        "catia_standard_repeated_face_geometry_rows",
        |ctx| {
            let bounds = StandardFaceBounds {
                aabb_center: [
                    crate::test_support::test_b5::finite(0.0),
                    crate::test_support::test_b5::finite(0.0),
                    crate::test_support::test_b5::finite(0.0),
                ],
                aabb_half_extents: [
                    crate::test_support::test_b5::nonnegative_length(1.0),
                    crate::test_support::test_b5::nonnegative_length(1.0),
                    crate::test_support::test_b5::nonnegative_length(1.0),
                ],
                sphere_center: [
                    crate::test_support::test_b5::finite(0.0),
                    crate::test_support::test_b5::finite(0.0),
                    crate::test_support::test_b5::finite(0.0),
                ],
                sphere_radius: crate::test_support::test_b5::nonnegative_length(1.0),
            };
            let edge_faces = [[0, 0]];
            let mut allowed_faces = [vec![1]];
            let face_bounds = [Some(bounds), Some(bounds)];
            let line = StandardCurveGeometry::Line;
            let edge_geometries = [&line];
            refine_repeated_face_domains_by_geometry_and_bounds(
                ctx,
                &edge_faces,
                &mut allowed_faces,
                Some(&face_bounds),
                None,
                &edge_geometries,
            )
            .map(|()| allowed_faces)
        },
        |allowed_faces| assert_eq!(allowed_faces, [vec![1]]),
    );
}

fn assert_collection_refusal<T>(
    operation: &'static str,
    service: impl FnOnce(),
    mut run: impl FnMut(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, CodecError>,
) {
    service();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |cap| {
            crate::test_support::with_collection_limit(cap, |ctx| {
                let result = run(ctx);
                let refusal = match result {
                    Err(CodecError::ResourceLimit(refusal)) => refusal,
                    Err(error) => panic!("unexpected error before {operation}: {error}"),
                    Ok(_) => panic!("fixture did not reach {operation}"),
                };
                assert_eq!(ctx.resource_refusal(), Some(refusal));
                Err::<T, _>(CodecError::ResourceLimit(refusal))
            })
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(refusal)
        if refusal.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && refusal.operation == operation));
}

#[test]
fn standard_population_selection_collection_propagates_slot_refusal() {
    let bytes = two_source_closed_tetrahedron_populations();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, &bytes).expect("service profile admits population input")
    });
    let select = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::families::standard::decode::standard_population_selections(ctx, &scan)
    };
    assert_work_refusal(
        "catia_standard_population_selections",
        select,
        |selections| {
            let Some((first, rest)) = selections else {
                panic!("source-closed population pair fixture did not pair");
            };
            assert_eq!(first.records.len(), 4);
            assert_eq!(first.supports.len(), 6);
            assert_eq!(rest.len(), 1);
            assert_eq!(rest[0].records.len(), 4);
            assert_eq!(rest[0].supports.len(), 6);
        },
    );
    assert_collection_refusal(
        "catia_standard_population_selections",
        || {
            crate::test_support::with_service_context(|ctx| {
                let selections = select(ctx).expect("service profile admits population selection");
                let Some((first, rest)) = selections else {
                    panic!("source-closed population pair fixture did not pair");
                };
                assert_eq!(first.records.len(), 4);
                assert_eq!(first.supports.len(), 6);
                assert_eq!(rest.len(), 1);
                assert_eq!(rest[0].records.len(), 4);
                assert_eq!(rest[0].supports.len(), 6);
            });
        },
        |ctx| select(ctx).map(|_| ()),
    );
}

fn two_source_closed_tetrahedron_populations() -> Vec<u8> {
    let original = crate::test_support::test_container::tetrahedron_topology_catpart();
    let (spine, surface_records, support_rows) = crate::test_support::with_service_context(|ctx| {
        let scan = crate::container::scan_bytes(ctx, &original)
            .expect("service profile admits source tetrahedron");
        let main = scan
            .main_data_stream
            .as_deref()
            .expect("tetrahedron has MainDataStream");
        let brep = scan.brep.as_deref().expect("tetrahedron has BREP");
        let supports =
            crate::families::standard::records::standard_curve_supports(ctx, brep, 4, Some(6))
                .expect("service profile admits source support scan");
        assert_eq!(supports.len(), 6);
        let support_start = supports[0].pos;
        assert!(support_start < main.len());
        assert!(supports
            .iter()
            .enumerate()
            .all(|(index, support)| support.pos == support_start + index * 11));
        let surface_records = brep
            .get(main.len()..)
            .expect("surface stream follows MainDataStream")
            .to_vec();
        assert_eq!(surface_records.len(), 4 * 65);
        (
            main[..support_start].to_vec(),
            surface_records,
            main[support_start..].to_vec(),
        )
    });

    let mut main = Vec::with_capacity(spine.len() * 2);
    main.extend_from_slice(&spine);
    main.extend_from_slice(&spine);
    let mut surf = Vec::with_capacity((surface_records.len() + support_rows.len()) * 2);
    for _ in 0..2 {
        surf.extend_from_slice(&surface_records);
        surf.extend_from_slice(&support_rows);
    }
    crate::test_support::test_container::standard_catpart_from_streams(&main, &surf)
}

#[test]
fn standard_plane_parameter_map_propagates_slot_refusal() {
    let bytes = crate::test_support::test_container::standard_catpart();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, &bytes).expect("service profile admits standard input")
    });
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::families::standard::decode::try_decode_standard(
            ctx,
            &scan,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    };
    assert_collection_refusal(
        "catia_plane_param_map",
        || {
            crate::test_support::with_service_context(|ctx| {
                let output = decode(ctx).expect("service profile admits standard decode");
                let output = output.expect("standard CATPart reaches the plane parameter map");
                assert_eq!(output.ir.model.surfaces.len(), 2);
                assert_eq!(output.ir.model.curves.len(), 1);
            });
        },
        decode,
    );
}
