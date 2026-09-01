// SPDX-License-Identifier: Apache-2.0
//! Decode-scope tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::collections::HashSet;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::modeling_graph_scope;
use crate::native::{CatiaObjectGraph, CatiaOuterContainerBinding};
use crate::test_support::*;
use crate::CatiaCodec;

fn graph(id: &str, stream_name: &str, class_name: &str) -> CatiaObjectGraph {
    CatiaObjectGraph {
        id: id.to_string(),
        byte_offset: 0,
        byte_len: 10,
        finjpl_segment: None,
        outer_container: Some(CatiaOuterContainerBinding {
            data_offset: 0,
            ordinal: 1,
            class_name: class_name.to_string(),
            base_class: "CATFeatCont".to_string(),
            stream_name: stream_name.to_string(),
        }),
        catalog_byte_offset: None,
        catalog: None,
        records: Vec::new(),
    }
}

#[test]
fn modeling_scope_includes_only_the_declared_part_graph() {
    let graphs = vec![
        graph("part-graph", "part", "CATPrtCont"),
        graph("shape-graph", "shape", "CATSm_Nom_User_Container"),
        graph("design-graph", "design", "CATSmd_Nom_User_Container"),
        graph("camera-graph", "camera", "CameraStartupContainer"),
    ];

    assert_eq!(
        modeling_graph_scope(true, &graphs),
        Some(HashSet::from(["part-graph".to_string()]))
    );
}

#[test]
fn modeling_scope_does_not_promote_application_extension_graphs() {
    let graphs = vec![
        graph("shape-graph", "shape", "CATSm_Nom_User_Container"),
        graph("design-graph", "design", "CATSmd_Nom_User_Container"),
    ];

    assert_eq!(modeling_graph_scope(true, &graphs), Some(HashSet::new()));
}

#[test]
fn modeling_scope_rejects_multiple_graphs_in_one_part_stream() {
    let graphs = vec![
        graph("first", "part", "CATPrtCont"),
        graph("second", "part", "CATPrtCont"),
    ];

    assert_eq!(modeling_graph_scope(true, &graphs), Some(HashSet::new()));
}

#[test]
fn modeling_scope_rejects_multiple_declared_part_graphs() {
    let graphs = vec![
        graph("first", "first-part", "CATPrtCont"),
        graph("second", "second-part", "CATPrtCont"),
    ];

    assert_eq!(modeling_graph_scope(true, &graphs), Some(HashSet::new()));
}

#[test]
fn modeling_scope_without_outer_declarations_remains_unbounded() {
    let graphs = vec![graph("fragment-graph", "part", "CATPrtCont")];

    assert_eq!(modeling_graph_scope(false, &graphs), None);
}

#[test]
fn nonfinite_constraint_scalar_is_not_reported_as_a_finite_quantity_loss() {
    let mut suffix = vec![0x84, 0x96, 0x82, 0xc1, 0xe6];
    suffix.extend_from_slice(&f64::NAN.to_bits().to_le_bytes());
    let file = standard_catpart_with_two_selector_value("Range", "CstAttr_Dimension", &suffix);

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode non-finite constraint scalar");

    assert!(decoded.report().losses.iter().all(|loss| {
        loss.code != crate::loss::CatiaLossCode::AttributesDimensionQuantityUnresolved.kind()
    }));
}

#[test]
fn finite_c1_constraint_scalar_reports_an_unresolved_quantity() {
    let mut suffix = vec![0x84, 0x96, 0x82, 0xc1, 0xe6];
    suffix.extend_from_slice(&25.4_f64.to_bits().to_le_bytes());
    let file = standard_catpart_with_two_selector_value("Range", "CstAttr_Dimension", &suffix);

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode finite C1 constraint scalar");

    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == crate::loss::CatiaLossCode::AttributesDimensionQuantityUnresolved.kind()
            && loss.message.contains("1 finite")
    }));
}

#[test]
fn unresolved_modeling_scope_accounts_for_every_retained_object_record() {
    let (mut bytes, _) = outer_container_object_graph_catpart();
    let class_offset = bytes
        .windows(b"CATPrtCont".len())
        .position(|window| window == b"CATPrtCont")
        .expect("part-container declaration");
    bytes[class_offset..class_offset + b"CATPrtCont".len()].copy_from_slice(b"CATFooCont");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode object graph without a declared part container");

    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_OBJECT_GRAPH_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_OBJECT_RECORD_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::MODELING_OBJECT_GRAPH_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::MODELING_OBJECT_RECORD_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::RETAINED_UNSCOPED_OBJECT_GRAPH_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::RETAINED_UNSCOPED_OBJECT_RECORD_COUNT),
        2
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
            && loss.message.contains("1 retained object graph(s)")
            && loss.message.contains("2 field record(s)")
    }));
}

#[test]
fn container_only_stops_before_geometry() {
    let f = standard_catpart();
    let mut cur = Cursor::new(f);
    let opts = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let result = CatiaCodec.decode(&mut cur, &opts).unwrap();
    assert!(!result.report().geometry_transferred());
    assert!(result.report().container_only());
    // The reconstructed BREP stream is preserved as an unknown passthrough.
    let unknowns = result.ir().native_unknowns("catia").unwrap();
    assert_eq!(unknowns.len(), 1);
    let retained = &result.source_fidelity().retained_records[0];
    assert_eq!(retained.sha256.len(), 64);
    assert!(retained.data.is_some());
}
