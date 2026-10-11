// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts over synthesized CATPart byte images.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use cadmpeg_test_support::{wire, EditableDecodeResult};

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::report::{loss::LossCategory, Severity};

use crate::test_support::test_a5a8::{a8_catpart, inner_no_directory_a8_catpart};
use crate::test_support::test_annotations::assert_every_entity_has_v1_annotation;
use crate::test_support::test_container::{
    fbb_only_catpart, fbb_only_quad_catpart, fbb_only_quad_unmatched_edge_catpart,
    standard_catpart, tetrahedron_topology_catpart, zero_entity_catpart,
    zero_entity_cylinder_catpart, zero_entity_cylinder_parametric_support_catpart,
};
use crate::test_support::test_e5::e5_catpart;
use crate::test_support::test_topology::fbb_only_quad_unmatched_edge_topology_stream;
use crate::variant::Variant;
use crate::CatiaCodec;

fn decode(bytes: Vec<u8>) -> EditableDecodeResult {
    EditableDecodeResult::from(
        CatiaCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("synthesized CATPart should decode"),
    )
}

fn assert_valid(result: &EditableDecodeResult) {
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
    assert_every_entity_has_v1_annotation(result.ir(), &result.source_fidelity().annotations);
    assert!(result.ir().native.namespace("catia").is_some());
}

#[test]
fn standard_nested_pipeline_aligns_detection_inspection_and_decode() {
    let bytes = standard_catpart();
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&CatiaCodec, &bytes),
        Confidence::High
    );

    let summary = CatiaCodec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("standard CATPart inspection");
    assert_eq!(summary.format(), "catia");
    assert!(summary
        .entries
        .iter()
        .any(|entry| entry.name == "MainDataStream"));
    assert!(summary
        .entries
        .iter()
        .any(|entry| entry.name == "SurfacicReps"));
    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("standard nested")));

    let result = decode(bytes);
    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.points.len(), 3);
    assert_eq!(result.ir().model.surfaces.len(), 2);
    assert!(result
        .source_fidelity()
        .retained_records()
        .values()
        .any(|record| record.data().is_some()));
    assert_valid(&result);
}

#[test]
fn standard_nested_pipeline_builds_a_valid_radial_topology_graph() {
    let result = decode(tetrahedron_topology_catpart());
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 4);
    assert_eq!(result.ir().model.loops.len(), 4);
    assert_eq!(result.ir().model.edges.len(), 6);
    assert_eq!(result.ir().model.coedges.len(), 12);
    assert_eq!(
        wire::coverage_count(
            result.report(),
            crate::coverage::ATTEMPTED_STANDARD_TOPOLOGY_COUNT.as_str()
        ),
        1
    );
    assert_eq!(
        wire::coverage_count(
            result.report(),
            crate::coverage::ATTACHED_STANDARD_TOPOLOGY_COUNT.as_str()
        ),
        1
    );
    assert!(!result.report().losses.iter().any(|loss| {
        loss.severity == Severity::Blocking
            && matches!(
                loss.code.category(),
                LossCategory::Geometry | LossCategory::Topology
            )
    }));
    assert_valid(&result);
}

#[test]
fn fbb_only_pipeline_transfers_carriers_without_inventing_topology() {
    let bytes = fbb_only_catpart();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, bytes.clone())
    })
    .expect("service resource budget");
    assert_eq!(scan.variant, Variant::FbbOnly);
    assert!(scan.census.fbb_runs > 0);
    assert_eq!(scan.census.edge_delimiters, 0);

    let result = decode(bytes);
    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.surfaces.len(), 2);
    assert!(result.ir().model.faces.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.category() == LossCategory::Topology && loss.severity == Severity::Blocking
    }));
    assert_valid(&result);
}

#[test]
fn fbb_only_pipeline_attaches_complete_boundary_topology() {
    let bytes = fbb_only_quad_catpart();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, bytes.clone())
    })
    .expect("service resource budget");
    assert_eq!(scan.variant, Variant::FbbOnly);
    assert_eq!(scan.census.edge_delimiters, 0);

    let result = decode(bytes);
    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.points.len(), 4);
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 4);
    assert_eq!(result.ir().model.coedges.len(), 4);
    assert_eq!(
        wire::coverage_count(
            result.report(),
            crate::coverage::ATTACHED_STANDARD_TOPOLOGY_COUNT.as_str()
        ),
        1
    );
    assert!(!result.report().losses.iter().any(|loss| {
        loss.severity == Severity::Blocking
            && matches!(
                loss.code.category(),
                LossCategory::Geometry | LossCategory::Topology
            )
    }));
    assert_valid(&result);
}

#[test]
fn fbb_only_pipeline_solves_an_unmatched_complete_run_with_mesh_incidence() {
    let topology = fbb_only_quad_unmatched_edge_topology_stream();
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::standard::topology::parse_fbb(ctx, &topology)
    })
    .expect("service resource budget")
    .is_none());

    let result = decode(fbb_only_quad_unmatched_edge_catpart());
    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 4);
    assert_eq!(
        wire::coverage_count(
            result.report(),
            crate::coverage::ATTACHED_STANDARD_TOPOLOGY_COUNT.as_str()
        ),
        1
    );
    assert!(!result.report().losses.iter().any(|loss| {
        loss.severity == Severity::Blocking
            && matches!(
                loss.code.category(),
                LossCategory::Geometry | LossCategory::Topology
            )
    }));
    assert_valid(&result);
}

#[test]
fn zero_entity_pipeline_binds_parametric_support_without_a_cached_curve() {
    let bytes = zero_entity_cylinder_parametric_support_catpart();
    assert_eq!(
        crate::test_support::with_service_context(|ctx| crate::container::scan_bytes(
            ctx,
            bytes.clone()
        ))
        .expect("service resource budget")
        .variant,
        Variant::ZeroEntity
    );

    let result = decode(bytes);
    assert!(result.report().geometry_transferred());
    assert!(result.ir().model.surfaces.iter().any(|surface| {
        matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_))
        )
    }));
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| { matches!(curve.geometry, CurveGeometry::Procedural { .. }) }));
    assert_eq!(
        wire::coverage_count(
            result.report(),
            crate::coverage::TRANSFERRED_ZERO_ENTITY_PARAMETRIC_SURFACE_CURVE_COUNT.as_str()
        ),
        1
    );
    assert_valid(&result);
}

#[test]
fn e5_pipeline_uses_the_coherent_record_stream_over_the_nested_spine() {
    let bytes = e5_catpart();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, bytes.clone())
    })
    .expect("service resource budget");
    assert_eq!(scan.variant, Variant::E5Stream);
    assert!(scan.e5_record_range.is_some());

    let result = decode(bytes);
    assert!(result.report().geometry_transferred());
    assert!(result.ir().model.curves.iter().any(|curve| {
        matches!(
            curve.geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(_))
        )
    }));
    assert!(result.report().notes.iter().any(|note| note.contains("E5")));
    assert_valid(&result);
}

#[test]
fn float_packed_pipeline_recovers_the_external_a8_control_grid() {
    let bytes = a8_catpart();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, bytes.clone())
    })
    .expect("service resource budget");
    assert_eq!(scan.variant, Variant::FloatPackedInnerNoFbb);

    let result = decode(bytes);
    assert!(result.report().geometry_transferred());
    assert!(result.ir().model.surfaces.iter().any(|surface| {
        matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs { .. })
        )
    }));
    assert_eq!(
        result.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("cgm-surface:decafbad")
    );
    assert_valid(&result);
}

#[test]
fn container_only_pipeline_retains_each_variant_without_semantic_transfer() {
    let fixtures = [
        standard_catpart(),
        fbb_only_catpart(),
        zero_entity_cylinder_catpart(),
        e5_catpart(),
        a8_catpart(),
    ];
    for bytes in fixtures {
        let result = EditableDecodeResult::from(
            CatiaCodec
                .decode(
                    &mut Cursor::new(bytes),
                    &DecodeOptions {
                        container_only: true,
                        ..DecodeOptions::default()
                    },
                )
                .expect("container-only CATPart decode"),
        );
        assert!(result.report().container_only());
        assert!(!result.report().geometry_transferred());
        assert!(result.ir().model.points.is_empty());
        assert!(result.ir().model.surfaces.is_empty());
        assert!(result.ir().model.faces.is_empty());
        assert!(result.ir().source.is_some());
        assert!(result.ir().native.namespace("catia").is_some());
        assert_valid(&result);
    }
}

fn assert_entity_resource_limit(error: &cadmpeg_ir::DecodeFailure) {
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
        ),
        "{error:?}"
    );
}

#[test]
fn container_only_raw_payload_refuses_entity_before_copy() {
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_entities = 0;
    let error = CatiaCodec
        .decode(&mut Cursor::new(standard_catpart()), &options)
        .expect_err("one retained record exceeds zero entities");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
                && limit.operation == "admit CATIA retained source record"));
}

#[test]
fn container_only_raw_payload_refuses_retained_bytes_before_copy() {
    let bytes = standard_catpart();
    let retained_len = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, bytes.clone())
    })
    .expect("service resource budget")
    .brep
    .expect("standard B-rep")
    .len();
    let mut cap = u64::try_from(retained_len - 1).expect("small fixture");
    let mut reached = false;
    for _ in 0..512 {
        let mut options = DecodeOptions {
            container_only: true,
            ..DecodeOptions::default()
        };
        options.policy.limits.max_retained_bytes = cap;
        match CatiaCodec.decode(&mut Cursor::new(bytes.clone()), &options) {
            Err(cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(
                limit,
            ))) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "retain CATIA raw payload" =>
            {
                reached = true;
                break;
            }
            Err(cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(
                limit,
            ))) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                cap = limit
                    .used
                    .checked_add(limit.additional)
                    .expect("bounded fixture");
            }
            other => panic!("raw payload limit not reached: {other:?}"),
        }
    }
    assert!(reached, "raw payload limit was not reached");
}

#[test]
fn full_route_raw_payload_refuses_entity_before_copy() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 0;
    let error = CatiaCodec
        .decode(&mut Cursor::new(standard_catpart()), &options)
        .expect_err("one retained record exceeds zero entities");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
                && limit.operation == "admit CATIA retained source record"));
}

#[test]
fn full_route_raw_payload_refuses_retained_bytes_before_copy() {
    let bytes = standard_catpart();
    let retained_len = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, bytes.clone())
    })
    .expect("service resource budget")
    .brep
    .expect("standard B-rep")
    .len();
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes =
        u64::try_from(retained_len - 1).expect("small fixture");
    for _ in 0..1024 {
        let error = CatiaCodec
            .decode(&mut Cursor::new(bytes.clone()), &options)
            .expect_err("retained copy exceeds byte limit");
        let cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            error
        else {
            panic!("expected a retained-byte resource refusal");
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::RetainedBytes
        );
        if limit.operation == "retain CATIA raw payload" {
            return;
        }
        options.policy.limits.max_retained_bytes = limit.used + limit.additional;
    }
    panic!("adaptive retained-byte cap did not reach the raw payload copy");
}

#[test]
fn decode_refuses_when_max_entities_is_zero() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 0;
    let error = CatiaCodec
        .decode(&mut Cursor::new(standard_catpart()), &options)
        .expect_err("max_entities=0 must refuse at CATIA entity admission");
    assert_entity_resource_limit(&error);
}

#[test]
fn decode_refuses_when_max_entities_is_below_route_entity_count() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 1;
    let error = CatiaCodec
        .decode(&mut Cursor::new(standard_catpart()), &options)
        .expect_err("max_entities below route entity count must refuse at admission");
    assert_entity_resource_limit(&error);
}

#[test]
fn every_decode_path_populates_v1_annotations() {
    let fixtures = [
        standard_catpart(),
        fbb_only_catpart(),
        zero_entity_catpart(),
        zero_entity_cylinder_catpart(),
        e5_catpart(),
        a8_catpart(),
        inner_no_directory_a8_catpart(),
    ];
    for fixture in fixtures {
        let decoded = EditableDecodeResult::from(
            CatiaCodec
                .decode(&mut Cursor::new(fixture), &DecodeOptions::default())
                .unwrap(),
        );
        assert_every_entity_has_v1_annotation(decoded.ir(), &decoded.source_fidelity().annotations);
    }

    let container_only = EditableDecodeResult::from(
        CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart()),
                &DecodeOptions {
                    container_only: true,
                    ..DecodeOptions::default()
                },
            )
            .unwrap(),
    );
    assert_every_entity_has_v1_annotation(
        container_only.ir(),
        &container_only.source_fidelity().annotations,
    );
}

#[test]
fn fuzz_crash_container_bytes_do_not_panic() {
    let bytes: &[u8] = include_bytes!("test_support/fuzz_catia_container_crash.bin");
    let _ = cadmpeg_test_support::detection::confidence(&CatiaCodec, bytes);
    let _probe = CatiaCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default());
    let _probe = CatiaCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default());
}

#[test]
fn sketch_geometry_without_construction_state_stays_native() {
    use crate::test_support::test_object_graph::{
        catalog_stream, entity_table_record_with_value, object_graph_from_records,
        object_graph_record,
    };
    let frame = [
        0x82, 0x86, 0xe8, 0xe0, 0x07, 0x37, 0x8a, 0x81, 0xe8, 0xe7, 0xe8, 0xe8, 0xe8, 0xe7, 0xe8,
        0xe8, 0xe8, 0xfe,
    ];
    let mut pair = vec![0x91, 0x85, 0xe8, 0xe4, 0x07, 0x37, 0x83, 0x81, 0xe6];
    pair.extend_from_slice(&5.75_f64.to_le_bytes());
    pair.push(0xe6);
    pair.extend_from_slice(&12.64_f64.to_le_bytes());
    pair.extend_from_slice(&[0xfe, 0xfe]);
    let records = [
        object_graph_record(
            &[0x04, 0x01, 0x82, 0x84],
            &[0x3b, 0x82, 0x81, 0x85, 0x81, 0x83, 0xfe],
        ),
        object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
        object_graph_record(&[0x12, 0x83, 0x86], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
        object_graph_record(&[0x12, 0x81, 0x85], &[0xfe]),
    ];
    let mut stream = Vec::new();
    for (id, value) in [
        (1, &[0xfe][..]),
        (2, &[0xfe][..]),
        (3, pair.as_slice()),
        (4, &[0xfe][..]),
        (5, frame.as_slice()),
        (6, &[0xfe][..]),
    ] {
        stream.extend(entity_table_record_with_value(id, value));
    }
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
        "Length",
        "2DPoint",
    ]));
    let (bytes, _) = crate::test_support::test_container::outer_container_catpart(&stream);
    let result = decode(bytes);
    assert_valid(&result);
    let model = &result.ir().model;
    assert_eq!(model.sketches.len(), 1);
    assert!(
        model.sketch_entities.is_empty(),
        "unknown construction state cannot become standard geometry"
    );
    assert!(model.sketch_constraints.is_empty());
    assert!(matches!(
        model.sketches[0].placement,
        cadmpeg_ir::sketches::SketchPlacement::Resolved { .. }
    ));
    let native = result
        .ir()
        .native
        .namespace("catia")
        .expect("native namespace");
    let records: Vec<crate::native::CatiaObjectRecord> = native
        .arena_as("object_graph_records")
        .expect("typed object records");
    let entities: Vec<crate::native::entity_record::CatiaEntityRecord> = native
        .arena_as("entity_records")
        .expect("typed entity records");
    let point = records
        .iter()
        .find(|r| r.entity_id() == Some(4))
        .expect("retained point field");
    assert_eq!(point.entity_id(), Some(4));
    assert_eq!(point.owner_entity_id(), Some(3));
    assert_eq!(
        point
            .class
            .as_ref()
            .expect("retained class selector")
            .ordinal,
        6
    );
    let owner = entities
        .iter()
        .find(|e| e.entity_id == 3)
        .expect("retained coordinate owner");
    let crate::native::entity_record::CatiaEntityRecordBody::Nested { value_payload, .. } =
        &owner.body
    else {
        panic!("nested coordinate owner");
    };
    assert_eq!(value_payload, &pair);
    assert!(owner.numeric_pair().is_some());
}
