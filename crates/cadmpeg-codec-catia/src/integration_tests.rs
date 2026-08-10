// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts over synthesized CATPart byte images.

use super::*;
use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::report::{LossCategory, Severity};

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthesized CATPart should decode")
}

fn assert_valid(result: &cadmpeg_ir::codec::DecodeResult) {
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
    assert_every_entity_has_v1_annotation(&result.ir, &result.source_fidelity.annotations);
    assert!(result.ir.native.namespace("catia").is_some());
}

#[test]
fn standard_nested_pipeline_aligns_detection_inspection_and_decode() {
    let bytes = standard_catpart();
    assert_eq!(CatiaCodec.detect(&bytes), Confidence::High);

    let summary = CatiaCodec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("standard CATPart inspection");
    assert_eq!(summary.format, "catia");
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
    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.points.len(), 3);
    assert_eq!(result.ir.model.surfaces.len(), 2);
    assert!(result
        .source_fidelity
        .retained_records
        .iter()
        .any(|record| record.data.is_some()));
    assert_valid(&result);
}

#[test]
fn standard_nested_pipeline_builds_a_valid_radial_topology_graph() {
    let result = decode(tetrahedron_topology_catpart());
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 4);
    assert_eq!(result.ir.model.loops.len(), 4);
    assert_eq!(result.ir.model.edges.len(), 6);
    assert_eq!(result.ir.model.coedges.len(), 12);
    assert_eq!(
        result
            .report
            .coverage_count(crate::coverage::ATTEMPTED_STANDARD_TOPOLOGY_COUNT),
        1
    );
    assert_eq!(
        result
            .report
            .coverage_count(crate::coverage::ATTACHED_STANDARD_TOPOLOGY_COUNT),
        1
    );
    assert!(!result.report.losses.iter().any(|loss| {
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
    let scan = crate::container::scan_bytes(bytes.clone());
    assert_eq!(scan.variant, Variant::FbbOnly);
    assert!(scan.census.fbb_runs > 0);
    assert_eq!(scan.census.edge_delimiters, 0);

    let result = decode(bytes);
    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.surfaces.len(), 2);
    assert!(result.ir.model.faces.is_empty());
    assert!(result.report.losses.iter().any(|loss| {
        loss.code.category() == LossCategory::Topology && loss.severity == Severity::Blocking
    }));
    assert_valid(&result);
}

#[test]
fn fbb_only_pipeline_attaches_complete_boundary_topology() {
    let bytes = fbb_only_quad_catpart();
    let scan = crate::container::scan_bytes(bytes.clone());
    assert_eq!(scan.variant, Variant::FbbOnly);
    assert_eq!(scan.census.edge_delimiters, 0);

    let result = decode(bytes);
    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.points.len(), 4);
    assert_eq!(result.ir.model.surfaces.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 4);
    assert_eq!(result.ir.model.coedges.len(), 4);
    assert_eq!(
        result
            .report
            .coverage_count(crate::coverage::ATTACHED_STANDARD_TOPOLOGY_COUNT),
        1
    );
    assert!(!result.report.losses.iter().any(|loss| {
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
        crate::container::scan_bytes(bytes.clone()).variant,
        Variant::ZeroEntity
    );

    let result = decode(bytes);
    assert!(result.report.geometry_transferred);
    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| { matches!(surface.geometry, SurfaceGeometry::Cylinder { .. }) }));
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| { matches!(curve.geometry, CurveGeometry::Procedural { .. }) }));
    assert_eq!(
        result.report.coverage_count(
            crate::coverage::TRANSFERRED_ZERO_ENTITY_PARAMETRIC_SURFACE_CURVE_COUNT
        ),
        1
    );
    assert_valid(&result);
}

#[test]
fn e5_pipeline_uses_the_coherent_record_stream_over_the_nested_spine() {
    let bytes = e5_catpart();
    let scan = crate::container::scan_bytes(bytes.clone());
    assert_eq!(scan.variant, Variant::E5Stream);
    assert!(crate::container::e5_record_stream(&scan.data).is_some());

    let result = decode(bytes);
    assert!(result.report.geometry_transferred);
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| { matches!(curve.geometry, CurveGeometry::Circle { .. }) }));
    assert!(result.report.notes.iter().any(|note| note.contains("E5")));
    assert_valid(&result);
}

#[test]
fn float_packed_pipeline_recovers_the_external_a8_control_grid() {
    let bytes = a8_catpart();
    let scan = crate::container::scan_bytes(bytes.clone());
    assert_eq!(scan.variant, Variant::FloatPackedInnerNoFbb);

    let result = decode(bytes);
    assert!(result.report.geometry_transferred);
    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| { matches!(surface.geometry, SurfaceGeometry::Nurbs { .. }) }));
    assert_eq!(
        result.ir.model.surfaces[0]
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
        let result = CatiaCodec
            .decode(
                &mut Cursor::new(bytes),
                &DecodeOptions {
                    container_only: true,
                    ..DecodeOptions::default()
                },
            )
            .expect("container-only CATPart decode");
        assert!(result.report.container_only);
        assert!(!result.report.geometry_transferred);
        assert!(result.ir.model.points.is_empty());
        assert!(result.ir.model.surfaces.is_empty());
        assert!(result.ir.model.faces.is_empty());
        assert!(result.ir.source.is_some());
        assert!(result.ir.native.namespace("catia").is_some());
        assert_valid(&result);
    }
}
