// SPDX-License-Identifier: Apache-2.0
//! Work admission of owner operations.

use crate::families::standard::decode::standard_object_evidence_from_streams;
use crate::test_support::test_b5::b5_closed_triangle_stream;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
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
fn standard_population_object_index_refuses_work_limit() {
    let stream = b5_closed_triangle_stream();
    let refused =
        crate::test_support::with_work_refusal("catia_standard_population_objects", |ctx| {
            standard_object_evidence_from_streams(
                ctx,
                std::slice::from_ref(&stream),
                &std::collections::BTreeSet::new(),
                &HashSet::new(),
                &mut crate::nurbs::LaneRefusals::new(),
                &mut crate::families::standard::decode::LimitCurveIndex::new(ctx)
                    .expect("empty index"),
            )
        });
    assert!(matches!(
        refused,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_standard_population_objects"
    ));
}

#[test]
fn standard_object_record_scan_refuses_caller_collection_limit() {
    let stream = b5_closed_triangle_stream();
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        standard_object_evidence_from_streams(
            ctx,
            &[stream],
            &std::collections::BTreeSet::new(),
            &HashSet::new(),
            &mut crate::nurbs::LaneRefusals::new(),
            &mut crate::families::standard::decode::LimitCurveIndex::new(ctx).expect("empty index"),
        )
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_record_source_ranges")
    );
}

#[test]
fn standard_edge_face_outputs_advance_one_candidate_before_child_work() {
    let stream = b5_closed_triangle_stream();
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        standard_object_evidence_from_streams(
            ctx,
            std::slice::from_ref(&stream),
            &std::collections::BTreeSet::new(),
            &HashSet::from([300, 301, 302]),
            &mut crate::nurbs::LaneRefusals::new(),
            &mut crate::families::standard::decode::LimitCurveIndex::new(ctx)?,
        )
    };
    let evidence = crate::test_support::with_service_context(run).expect("triangle evidence");
    assert_eq!(evidence.edge_owner_faces.len(), 3);
    for operation in [
        "catia_standard_edge_face_outputs",
        "catia_standard_edge_support_outputs",
    ] {
        let result = crate::test_support::with_work_refusal(operation, run);
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == operation && limit.additional == 1)
        );
    }
}

#[test]
fn standard_procedure_validity_advances_one_evidence_before_child_work() {
    use crate::families::standard::decode::{StandardEvidenceStore, StandardSurfaceEvidence};
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let mut store = StandardEvidenceStore::default();
        for tag in 0..64 {
            store.add(
                ctx,
                tag,
                StandardSurfaceEvidence::Geometry(SurfaceGeometry::Solved(
                    SolvedSurfaceGeometry::Unknown { record: None },
                )),
            )?;
        }
        store.into_outputs(ctx, &HashSet::new())
    };
    let outputs = crate::test_support::with_service_context(run).expect("evidence outputs");
    assert_eq!(outputs.0.len(), 64);
    assert!(outputs.1.is_empty());
    let result = crate::test_support::with_work_refusal("catia_standard_iteration", run);
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "catia_standard_iteration" && limit.additional == 1)
    );
}
