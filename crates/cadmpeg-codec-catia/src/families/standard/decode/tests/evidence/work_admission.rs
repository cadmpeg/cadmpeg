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
fn standard_population_object_copy_refuses_retained_limit() {
    let stream = b5_closed_triangle_stream();
    let mut cap = 0;
    let mut reached = false;
    for _ in 0..128 {
        let refusal = crate::test_support::with_retained_limit(cap, |ctx| {
            standard_object_evidence_from_streams(
                ctx,
                std::slice::from_ref(&stream),
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
            &[stream],
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
