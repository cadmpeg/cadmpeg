// SPDX-License-Identifier: Apache-2.0
//! Decode-scope tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use cadmpeg_test_support::{wire, EditableDecodeResult};

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::modeling_graph_scope;
use crate::native::{CatiaObjectGraph, CatiaOuterContainerBinding};
use crate::test_support::test_container::standard_catpart;
use crate::test_support::test_formula::standard_catpart_with_two_selector_value;
use crate::test_support::test_object_graph::outer_container_object_graph_catpart;
use crate::CatiaCodec;

#[test]
fn standard_alias_route_propagates_entity_candidate_limit() {
    let bytes = standard_catpart_with_two_selector_value("Range", "CstAttr_Dimension", &[0xfe]);
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let options = DecodeOptions {
        policy,
        ..DecodeOptions::default()
    };
    let error = CatiaCodec
        .decode(&mut Cursor::new(bytes), &options)
        .expect_err("7C05 identity candidate exceeds zero collection items");
    assert!(matches!(error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "admit CATIA 7C05 identity candidate"));
}

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
        super::ModelingGraphScope::Scoped("part-graph".to_string())
    );
}

#[test]
fn modeling_scope_does_not_promote_application_extension_graphs() {
    let graphs = vec![
        graph("shape-graph", "shape", "CATSm_Nom_User_Container"),
        graph("design-graph", "design", "CATSmd_Nom_User_Container"),
    ];

    assert_eq!(
        modeling_graph_scope(true, &graphs),
        super::ModelingGraphScope::Unresolved
    );
}

#[test]
fn modeling_scope_rejects_multiple_graphs_in_one_part_stream() {
    let graphs = vec![
        graph("first", "part", "CATPrtCont"),
        graph("second", "part", "CATPrtCont"),
    ];

    assert_eq!(
        modeling_graph_scope(true, &graphs),
        super::ModelingGraphScope::Unresolved
    );
}

#[test]
fn modeling_scope_rejects_multiple_declared_part_graphs() {
    let graphs = vec![
        graph("first", "first-part", "CATPrtCont"),
        graph("second", "second-part", "CATPrtCont"),
    ];

    assert_eq!(
        modeling_graph_scope(true, &graphs),
        super::ModelingGraphScope::Unresolved
    );
}

#[test]
fn modeling_scope_without_outer_declarations_remains_unbounded() {
    let graphs = vec![graph("fragment-graph", "part", "CATPrtCont")];

    assert_eq!(
        modeling_graph_scope(false, &graphs),
        super::ModelingGraphScope::Unscoped
    );
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
        wire::coverage_count(
            decoded.report(),
            crate::coverage::DECODED_OBJECT_GRAPH_COUNT.as_str()
        ),
        1
    );
    assert_eq!(
        wire::coverage_count(
            decoded.report(),
            crate::coverage::DECODED_OBJECT_RECORD_COUNT.as_str()
        ),
        2
    );
    assert_eq!(
        wire::coverage_count(
            decoded.report(),
            crate::coverage::MODELING_OBJECT_GRAPH_COUNT.as_str()
        ),
        0
    );
    assert_eq!(
        wire::coverage_count(
            decoded.report(),
            crate::coverage::MODELING_OBJECT_RECORD_COUNT.as_str()
        ),
        0
    );
    assert_eq!(
        wire::coverage_count(
            decoded.report(),
            crate::coverage::RETAINED_UNSCOPED_OBJECT_GRAPH_COUNT.as_str()
        ),
        1
    );
    assert_eq!(
        wire::coverage_count(
            decoded.report(),
            crate::coverage::RETAINED_UNSCOPED_OBJECT_RECORD_COUNT.as_str()
        ),
        2
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::loss::LossCategory::DesignIntent
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
    let result = EditableDecodeResult::from(CatiaCodec.decode(&mut cur, &opts).unwrap());
    assert!(!result.report().geometry_transferred());
    assert!(result.report().container_only());
    // The reconstructed BREP stream is preserved as an unknown passthrough.
    let unknowns = result.ir().native_unknowns("catia").unwrap();
    assert_eq!(unknowns.len(), 1);
    let retained = &result
        .source_fidelity()
        .retained_records()
        .values()
        .next()
        .expect("retained record");
    assert_eq!(retained.sha256().len(), 64);
    assert!(retained.data().is_some());
}

/// A route that refuses a carrier record and then leaves through a `?` between
/// the refusal and the report still delivers the note: the sink belongs to the
/// caller, so the router's fall-through to the next route is stated, not
/// silent.
#[test]
fn a_route_that_exits_after_a_refusal_still_delivers_both_notes() {
    fn refusing_route(refusal: &mut crate::nurbs::LaneRefusals) -> Option<()> {
        let short_weight_lane = || {
            cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![
                    cadmpeg_ir::math::Point2::new(0.0, 0.0),
                    cadmpeg_ir::math::Point2::new(1.0, 0.0),
                ],
                Some(vec![1.0]),
                false,
            )
        };
        crate::nurbs::note_refusal(
            short_weight_lane(),
            refusal,
            "e5 NURBS surface record at byte 16",
        );
        // The second refusal leaves through the `?`, which is the exit that
        // used to drop the cell.
        crate::nurbs::note_refusal(
            short_weight_lane(),
            refusal,
            "e5 NURBS pcurve record at byte 96",
        )?;
        Some(())
    }

    let mut refusal = crate::nurbs::LaneRefusals::new();
    assert!(
        refusing_route(&mut refusal).is_none(),
        "the route states no model for the refused stream"
    );
    let notes = refusal.take_notes();
    assert_eq!(notes.len(), 2, "one note per refused record: {notes:?}");
    for (note, record) in notes.iter().zip([
        "e5 NURBS surface record at byte 16",
        "e5 NURBS pcurve record at byte 96",
    ]) {
        assert!(
            note.message.contains(record),
            "the note names the record that stated the lanes: {}",
            note.message
        );
    }
}

/// The router states a fall-through by name. A route that refuses records and
/// then transfers no model leaves both its refusal notes and one fall-through
/// note in the report of whatever route or fallback finishes the decode.
#[test]
fn a_route_that_refuses_and_falls_through_states_both_notes_in_the_report() {
    fn refusing_route(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _scan: &crate::container::ContainerScan,
        refusal: &mut crate::nurbs::LaneRefusals,
    ) -> Result<Option<crate::families::FamilyOutput>, cadmpeg_core::CodecError> {
        ctx.charge_collection_items(7, "build test NURBS lanes")?;
        let output = (|| {
            crate::nurbs::note_refusal(
                cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![
                        cadmpeg_ir::math::Point2::new(0.0, 0.0),
                        cadmpeg_ir::math::Point2::new(1.0, 0.0),
                    ],
                    Some(vec![1.0]),
                    false,
                ),
                refusal,
                "e5 NURBS pcurve record at byte 96",
            )?;
            Some(crate::families::FamilyOutput {
                ir: cadmpeg_ir::CadIr::empty(),
                report: cadmpeg_ir::codec::DecodeBody::new(
                    cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
                ),
                annotations: cadmpeg_ir::Annotations::default(),
                unknowns: Vec::new(),
                admitted_model_entities: 0,
            })
        })();
        Ok(output)
    }

    const ROUTES: &[crate::families::Route] = &[crate::families::Route {
        name: "the test route",
        applicable: |_| true,
        decode: refusing_route,
        standard_face_population: false,
    }];

    let bytes = standard_catpart();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("a context over the synthetic container");
    let mut refusal = crate::nurbs::LaneRefusals::new();
    let decoded = super::decode_over_routes(&ctx, root, ROUTES, &mut refusal)
        .expect("the metadata fallback finishes the decode");

    let messages: Vec<&str> = decoded
        .body
        .losses
        .iter()
        .map(|note| note.message.as_str())
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("e5 NURBS pcurve record at byte 96")),
        "the refusal the fallen-through route stated: {messages:?}"
    );
    assert!(
        messages.iter().any(|message| {
            message.contains("the test route refused 1 CATIA record(s)")
                && message.contains("the decode continued to the metadata fallback")
        }),
        "the fall-through statement: {messages:?}"
    );
    assert!(
        refusal.take_notes().is_empty(),
        "the sink is drained into the report"
    );
}
