// SPDX-License-Identifier: Apache-2.0
//! Work admission of owner operations.

use super::plane_bounds_record;
use crate::families::standard::fbb::FbbPopulationLayout;
use crate::families::standard::records::{AnalyticSurfaceKind, StandardCurveGeometry, StandardCurveSupport, StandardSurfacePopulation, StandardSurfaceRecord, SurfacePrefix};
use std::collections::HashMap;

#[test]
fn source_order_population_pair_copies_refuse_at_each_collection_boundary() {
    let layout = |start| FbbPopulationLayout {
        face_run: crate::families::standard::fbb::FbbFaceRun::try_new(start, 1).expect("one face"),
        edge_count: 1,
        vertex_count: 1,
        edge_table_form: crate::families::standard::fbb::EdgeTableForm::FbbOnly,
    };
    let population = || StandardSurfacePopulation {
        records: vec![StandardSurfaceRecord::Analytic(SurfacePrefix {
            pos: 10,
            target: 1,
            kind: AnalyticSurfaceKind::Cylinder,
        })],
        supports: vec![StandardCurveSupport {
            pos: 20,
            tag: 2,
            faces: [0, 0],
            geometry: StandardCurveGeometry::Line,
        }],
    };
    let layouts = [layout(0), layout(100)];
    let populations = [population(), population()];
    let result = crate::test_support::with_service_context(|ctx| {
        crate::families::standard::records::pair_standard_populations(ctx, &layouts, &populations)
    })
    .expect("service resource budget")
    .expect("matched populations");
    assert_eq!(result.rest.len(), 1);
    let mut operations = std::collections::HashSet::new();
    for limit in 0..5 {
        let result = crate::test_support::with_collection_limit(limit, |ctx| {
            crate::families::standard::records::pair_standard_populations(
                ctx,
                &layouts,
                &populations,
            )
        });
        let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = result else {
            panic!("expected resource refusal");
        };
        operations.insert(refusal.operation);
    }
    for operation in [
        "catia_population_pair_records",
        "catia_population_pair_supports",
        "catia_population_pairs",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn plane_parameter_target_sets_and_rows_refuse_before_growth() {
    let row = plane_bounds_record(
        0x0001_0203,
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
        [0.0, 0.0, 0.0],
        2.0,
    );
    let mut bytes = row.clone();
    bytes.extend(row);
    let normals = HashMap::from([(
        0x0001_0203,
        crate::test_support::test_b5::finite_vector([0.0, 0.0, 1.0]),
    )]);
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::standard::records::plane_params(ctx, &bytes, &normals)
    })
    .expect("service resource budget")
    .is_empty());
    let mut operations = std::collections::HashSet::new();
    for limit in 0..4 {
        let result = crate::test_support::with_collection_limit(limit, |ctx| {
            crate::families::standard::records::plane_params(ctx, &bytes, &normals)
        });
        if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = result {
            operations.insert(refusal.operation);
        }
    }
    for operation in [
        "catia_plane_seen_targets",
        "catia_plane_duplicate_targets",
        "catia_plane_params",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}
