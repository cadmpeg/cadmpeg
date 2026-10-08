// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn face_admission_diagnostics_bound_samples_and_record_counts() {
    let mut diagnostics = BrepTransferDiagnostics {
        candidate_face_count: 6,
        admitted_face_count: 1,
        emitted_face_count: 1,
        ..BrepTransferDiagnostics::default()
    };
    crate::decode::with_test_decode_ctx(|ctx| {
        for face_id in 10..16 {
            diagnostics
                .reject_face(ctx, FaceAdmissionRejection::MissingLoops, face_id)
                .expect("service rejection admitted");
        }
    });

    let (count, samples) = crate::decode::with_test_decode_ctx(|ctx| {
        let (count, samples) = diagnostics.evidence(ctx, FaceAdmissionRejection::MissingLoops)?;
        Ok::<_, cadmpeg_core::CodecError>((count, samples.collect::<Vec<_>>()))
    })
    .expect("service rejection evidence admitted");
    assert_eq!(count, 6);
    assert_eq!(
        samples
            .iter()
            .map(|detail| detail.face_id)
            .collect::<Vec<_>>(),
        vec![10, 11, 12, 13]
    );
    let records = crate::decode::with_test_decode_ctx(|ctx| {
        diagnostics.face_admission_rejection_records(ctx)
    })
    .expect("service rejection records admitted");
    assert_eq!(records.len(), 6);
    assert_eq!(records[0].id, "creo:brep:face_admission_rejection#10");
    assert_eq!(records[0].face_id, 10);
    assert_eq!(records[0].reason, "missing_loops");
    assert_eq!(records[5].face_id, 15);
    let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
    crate::decode::with_test_decode_ctx(|ctx| diagnostics.record_coverage(ctx, &mut coverage))
        .expect("service coverage admitted");
    assert_eq!(coverage["brep_candidate_face_count"], 6);
    assert_eq!(coverage["brep_admitted_face_count"], 1);
    assert_eq!(coverage["brep_emitted_face_count"], 1);
    assert_eq!(coverage["brep_rejected_face_count"], 6);
    assert_eq!(coverage["brep_rejected_face_missing_loops_count"], 6);
}

#[test]
fn brep_face_rejection_diagnostics_refuse_collection_limit() {
    let limit = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo B-rep face rejection diagnostics"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            BrepTransferDiagnostics::default().reject_face(
                &ctx,
                FaceAdmissionRejection::MissingLoops,
                17,
            )
        },
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut diagnostics = BrepTransferDiagnostics::default();
    let error = diagnostics
        .reject_face(&ctx, FaceAdmissionRejection::MissingLoops, 17)
        .expect_err("rejection diagnostic refused");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep face rejection diagnostics"));
    assert!(diagnostics.face_rejection_diagnostics.is_empty());
}

fn rejection_detail_limit_error(operation: &'static str) -> CodecError {
    let half_edge = crate::topology::HalfEdgeId {
        curve_id: 4,
        side: crate::topology::Side::Zero,
    };
    let loop_record =
        crate::test_support::closed_loop(std::num::NonZeroU32::new(17), vec![half_edge]);
    let binding = crate::topology::HalfEdgeVertexIncidence {
        half_edge,
        start_vertex_id: std::num::NonZeroU32::new(9).expect("one-based vertex fixture"),
        end_vertex_id: None,
    };
    let incidence = BTreeMap::from([(half_edge, &binding)]);
    crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |ctx| {
            FaceAdmissionDetail::unresolved_boundary(
                ctx,
                17,
                &[&loop_record],
                &BTreeMap::new(),
                &incidence,
            )
        },
    )
}

#[test]
fn brep_rejection_boundary_samples_refuse_collection_limit() {
    let error = rejection_detail_limit_error("creo B-rep rejection boundary samples");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep rejection boundary samples"));
}

#[test]
fn brep_rejection_vertex_samples_refuse_collection_limit() {
    let error = rejection_detail_limit_error("creo B-rep rejection vertex samples");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep rejection vertex samples"));
}

fn rejection_record_limit_error(
    dimension: ResourceDimension,
    operation: &'static str,
) -> CodecError {
    let mut diagnostics = BrepTransferDiagnostics::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        diagnostics.reject_face_with_detail(
            ctx,
            FaceAdmissionRejection::UnresolvedBoundaryVertices,
            FaceAdmissionDetail {
                face_id: 17,
                boundary_half_edges: vec![crate::topology::HalfEdgeId {
                    curve_id: 4,
                    side: crate::topology::Side::Zero,
                }],
                vertex_ids: vec![9],
            },
        )
    })
    .expect("service rejection admitted");
    crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
        diagnostics.face_admission_rejection_records(ctx)
    })
}

#[test]
fn brep_rejection_record_id_refuses_retained_limit() {
    let error = rejection_record_limit_error(
        ResourceDimension::RetainedBytes,
        "creo B-rep rejection record IDs",
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo B-rep rejection record IDs"));
}

#[test]
fn brep_rejection_half_edges_refuse_collection_limit() {
    let error = rejection_record_limit_error(
        ResourceDimension::CollectionItems,
        "creo B-rep rejection half edges",
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep rejection half edges"));
}

#[test]
fn brep_rejection_vertex_ids_refuse_collection_limit() {
    let error = rejection_record_limit_error(
        ResourceDimension::CollectionItems,
        "creo B-rep rejection vertex IDs",
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep rejection vertex IDs"));
}

#[test]
fn brep_rejection_record_rows_refuse_collection_limit() {
    let error = rejection_record_limit_error(
        ResourceDimension::CollectionItems,
        "creo B-rep rejection records",
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep rejection records"));
}

#[test]
fn brep_rejection_record_preserves_nested_operands_under_service_profile() {
    let mut diagnostics = BrepTransferDiagnostics::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        diagnostics.reject_face_with_detail(
            ctx,
            FaceAdmissionRejection::UnresolvedBoundaryVertices,
            FaceAdmissionDetail {
                face_id: 17,
                boundary_half_edges: vec![crate::topology::HalfEdgeId {
                    curve_id: 4,
                    side: crate::topology::Side::Zero,
                }],
                vertex_ids: vec![9],
            },
        )
    })
    .expect("service rejection admitted");
    let records = crate::decode::with_test_decode_ctx(|ctx| {
        diagnostics.face_admission_rejection_records(ctx)
    })
    .expect("service rejection records admitted");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "creo:brep:face_admission_rejection#17");
    assert_eq!(records[0].reason, "unresolved_boundary_vertices");
    assert_eq!(records[0].boundary_half_edges[0].curve_id, 4);
    assert_eq!(records[0].vertex_ids, [9]);
}

#[test]
fn face_admission_diagnostics_report_missing_surface_carrier() {
    let mut diagnostics = BrepTransferDiagnostics::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        diagnostics.reject_face(ctx, FaceAdmissionRejection::MissingSurfaceCarrier, 42)
    })
    .expect("service rejection admitted");

    let (count, samples) = crate::decode::with_test_decode_ctx(|ctx| {
        let (count, samples) =
            diagnostics.evidence(ctx, FaceAdmissionRejection::MissingSurfaceCarrier)?;
        Ok::<_, cadmpeg_core::CodecError>((count, samples.collect::<Vec<_>>()))
    })
    .expect("service rejection evidence admitted");
    assert_eq!(count, 1);
    assert_eq!(
        samples
            .iter()
            .map(|detail| detail.face_id)
            .collect::<Vec<_>>(),
        vec![42]
    );
    let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
    crate::decode::with_test_decode_ctx(|ctx| diagnostics.record_coverage(ctx, &mut coverage))
        .expect("service coverage admitted");
    assert_eq!(coverage["brep_rejected_face_count"], 1);
    assert_eq!(
        coverage["brep_rejected_face_missing_surface_carrier_count"],
        1
    );
}

#[test]
fn face_admission_diagnostics_record_unresolved_boundary_operands() {
    let resolved = crate::topology::HalfEdgeId {
        curve_id: 10,
        side: crate::topology::Side::Zero,
    };
    let unresolved = crate::topology::HalfEdgeId {
        curve_id: 11,
        side: crate::topology::Side::One,
    };
    let loop_record =
        crate::test_support::closed_loop(std::num::NonZeroU32::new(5), vec![resolved, unresolved]);
    let resolved_binding = crate::topology::HalfEdgeVertexIncidence {
        half_edge: resolved,
        start_vertex_id: std::num::NonZeroU32::new(1).expect("one-based vertex fixture"),
        end_vertex_id: std::num::NonZeroU32::new(2),
    };
    let unresolved_binding = crate::topology::HalfEdgeVertexIncidence {
        half_edge: unresolved,
        start_vertex_id: std::num::NonZeroU32::new(3).expect("one-based vertex fixture"),
        end_vertex_id: std::num::NonZeroU32::new(4),
    };
    let incidence = BTreeMap::from([
        (resolved, &resolved_binding),
        (unresolved, &unresolved_binding),
    ]);
    let detail = crate::decode::with_test_decode_ctx(|ctx| {
        FaceAdmissionDetail::unresolved_boundary(
            ctx,
            5,
            &[&loop_record],
            &BTreeMap::from([(10, [1, 2])]),
            &incidence,
        )
    })
    .expect("service rejection detail admitted");

    assert_eq!(detail.face_id, 5);
    assert_eq!(detail.boundary_half_edges, vec![unresolved]);
    assert_eq!(detail.vertex_ids, vec![3, 4]);

    let mut diagnostics = BrepTransferDiagnostics::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        diagnostics.reject_face_with_detail(
            ctx,
            FaceAdmissionRejection::UnresolvedBoundaryVertices,
            detail,
        )
    })
    .expect("service rejection admitted");
    let (count, samples) = crate::decode::with_test_decode_ctx(|ctx| {
        let (count, samples) =
            diagnostics.evidence(ctx, FaceAdmissionRejection::UnresolvedBoundaryVertices)?;
        Ok::<_, cadmpeg_core::CodecError>((count, samples.collect::<Vec<_>>()))
    })
    .expect("service rejection evidence admitted");
    assert_eq!(count, 1);
    assert_eq!(
        samples
            .iter()
            .map(|detail| detail.face_id)
            .collect::<Vec<_>>(),
        vec![5]
    );
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].boundary_half_edges, vec![unresolved]);
    assert_eq!(samples[0].vertex_ids, vec![3, 4]);
}
