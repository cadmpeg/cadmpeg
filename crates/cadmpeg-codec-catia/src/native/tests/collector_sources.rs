// SPDX-License-Identifier: Apache-2.0
//! Work admission of native collector sources.

use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;

fn assert_source_work_refusal<T>(
    operation: &'static str,
    mut run: impl FnMut(&DecodeContext<'_>) -> Result<T, CodecError>,
    populated: impl Fn(&T) -> bool,
) {
    let service_output = crate::test_support::with_service_context(|ctx| run(ctx))
        .expect("service source fixture decodes");
    assert!(populated(&service_output), "service output is nonempty");
    let refused = crate::test_support::with_work_refusal(operation, |ctx| {
        let result = run(ctx);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    });
    assert!(matches!(refused,
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == operation));
}

fn decoded_native(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[crate::wire::records::ConsolidatedRecord],
) -> Result<crate::native::CatiaNative, CodecError> {
    crate::native::CatiaNative::decode_with_records(
        ctx,
        bytes,
        records,
        &mut crate::nurbs::LaneRefusals::new(),
    )
}

fn converted_zero_support_runs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<crate::native::CatiaZeroEntitySupportRun>, CodecError> {
    let range = 0..bytes.len();
    let records = crate::native::zero_entity_records(ctx, bytes, range.clone())?;
    let mut refusal = crate::nurbs::LaneRefusals::new();
    let runs = crate::families::zero_entity::records::zero_entity_support_runs_in_range(
        ctx,
        bytes,
        range,
        &mut refusal,
    )?;
    crate::native::zero_entity_support_runs(ctx, runs, &records)
}

#[test]
fn native_relation_dependencies_collector_preserves_work_refusal() {
    let candidate = crate::native::CatiaParameterBinding {
        entity_id: 10,
        entity: "entity-10",
        class_name: Some("parameter"),
    };
    let bindings = std::collections::HashMap::from([(
        "graph",
        std::collections::HashMap::from([("#1_", vec![candidate])]),
    )]);
    assert_source_work_refusal(
        "catia_native_dependencies",
        |ctx| {
            crate::native::relation_parameter_dependencies(ctx, "#1_", "graph", &bindings)
                .map(|rows| rows.len())
        },
        |count| *count == 1,
    );
}

#[test]
fn native_legacy_runs_collector_preserves_work_refusal() {
    let mut bytes = vec![0xea];
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0x81, 0xfd, 0x8c]);
    bytes.extend_from_slice(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
    assert_source_work_refusal(
        "catia_native_legacy_runs",
        |ctx| crate::native::legacy_entity_runs(ctx, &bytes).map(|rows| rows.len()),
        |count| *count == 1,
    );
}

#[test]
fn native_class61_records_collector_preserves_work_refusal() {
    let mut bytes = crate::test_support::test_b2::b2_counted_61_stream();
    bytes.extend_from_slice(&crate::test_support::test_b2::b2_long_61_stream());
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal(
        "catia_native_class61_records",
        |ctx| {
            crate::native::consolidated_class61_records(ctx, &bytes, &records)
                .map(|rows| rows.len())
        },
        |count| *count == 2,
    );
}

#[test]
fn native_class5b5c_records_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_b2::b2_class5b5c_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal(
        "catia_native_class5b5c_records",
        |ctx| {
            crate::native::consolidated_class5b5c_records(ctx, &bytes, &records)
                .map(|rows| rows.len())
        },
        |count| *count > 0,
    );
}

#[test]
fn native_cone_faces_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_b2::b2_cone_face_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal(
        "catia_native_cone_faces",
        |ctx| {
            crate::native::consolidated_cone_faces(ctx, &bytes, &records, &[])
                .map(|rows| rows.len())
        },
        |count| *count > 0,
    );
}

#[test]
fn native_plane_carriers_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_b2::b2_plane_carrier_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal(
        "catia_native_plane_carriers",
        |ctx| {
            crate::native::consolidated_plane_carriers(ctx, &bytes, &records).map(|rows| rows.len())
        },
        |count| *count > 0,
    );
}

#[test]
fn native_reference_lists_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_b2::b2_reference_list_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal(
        "catia_native_reference_lists",
        |ctx| {
            crate::native::consolidated_reference_lists(ctx, &bytes, &records)
                .map(|rows| rows.len())
        },
        |count| *count > 0,
    );
}

#[test]
fn native_pcurves_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_a5a8::a5_pcurve_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal(
        "catia_native_pcurves",
        |ctx| crate::native::consolidated_pcurves(ctx, &bytes, &records).map(|rows| rows.len()),
        |count| *count == 1,
    );
}

#[test]
fn native_zero_support_runs_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_zero_entity::zero_entity_support_stream();
    assert_source_work_refusal(
        "catia_native_zero_support_runs",
        |ctx| converted_zero_support_runs(ctx, &bytes).map(|rows| rows.len()),
        |count| *count > 0,
    );
}

#[test]
fn native_zero_face_loops_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_zero_entity::zero_entity_face_loop_support_stream();
    assert_source_work_refusal(
        "catia_native_zero_face_loops",
        |ctx| {
            converted_zero_support_runs(ctx, &bytes).map(|rows| {
                rows.iter()
                    .any(|run| run.face.as_ref().is_some_and(|face| !face.loops.is_empty()))
            })
        },
        |has_loops| *has_loops,
    );
}

#[test]
fn native_zero_support_occurrences_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_zero_entity::zero_entity_support_stream();
    assert_source_work_refusal(
        "catia_native_zero_support_occurrences",
        |ctx| {
            converted_zero_support_runs(ctx, &bytes)
                .map(|runs| runs.iter().any(|run| !run.supports.is_empty()))
        },
        |has_supports| *has_supports,
    );
}

#[test]
fn native_zero_ownership_roots_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_zero_entity::zero_entity_ownership_stream(1);
    assert_source_work_refusal(
        "catia_native_zero_ownership_roots",
        |ctx| {
            crate::native::zero_entity_ownership_roots(ctx, &bytes, 0..bytes.len())
                .map(|rows| rows.len())
        },
        |count| *count == 1,
    );
}

#[test]
fn native_edge_wires_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_a5_bound::a5_native_edge_run_stream(6, 139, 142);
    let native = crate::native::CatiaNative::decode(&bytes);
    let nodes = native.consolidated_edge_nodes.clone();
    let identities = native.consolidated_vertex_identities.clone();
    assert_source_work_refusal(
        "catia_native_edge_wires",
        |ctx| {
            crate::native::edge_node::edge_node_wires_charged(ctx, nodes.clone(), &identities)
                .map(|rows| rows.len())
        },
        |count| *count > 0,
    );
}

#[test]
fn native_owner_packets_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_b2::b2_owner_packet_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal(
        "catia_native_owner_packet_output",
        |ctx| {
            crate::native::projection::consolidated_owner_packets(ctx, &bytes, &records)
                .map(|rows| rows.len())
        },
        |count| *count > 0,
    );
}

fn record(class: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![
        0xb2,
        0x03,
        class,
        u8::try_from(payload.len()).expect("fixture payload fits u8"),
        0x05,
    ];
    bytes.extend_from_slice(payload);
    bytes
}

fn class25_edge_run() -> Vec<u8> {
    let mut descriptor = vec![0x08, 0x34, 0x12, 0x02];
    descriptor.extend_from_slice(&3.0_f64.to_le_bytes());
    descriptor.extend_from_slice(&7.0_f64.to_le_bytes());
    let mut definition = vec![0x82, 0x05, 0xe7, 0x0a, 0x87, 0x0d];
    for value in [1.0_f64, 2.0, 0.001, 3.0, 4.0, 1.0, 5.0, 0.001] {
        definition.extend_from_slice(&value.to_le_bytes());
    }
    let mut bytes = record(0x18, &descriptor);
    bytes.extend_from_slice(&record(0x25, &definition));
    bytes.extend_from_slice(
        &crate::test_support::test_a5a8::a5_native_edge_identity_stream(6, 139, 142),
    );
    bytes
}

#[test]
fn native_class25_edge_descriptors_collector_preserves_work_refusal() {
    let bytes = class25_edge_run();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal(
        "catia_native_class25_edge_descriptors",
        |ctx| {
            crate::native::projection::consolidated_edge_nodes(ctx, &bytes, &records, &[])
                .map(|rows| rows.len())
        },
        |count| *count > 0,
    );
}

#[test]
fn native_catalog_entries_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_object_graph::catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "schema",
    ]);
    assert_source_work_refusal(
        "catia_native_catalog_entries",
        |ctx| {
            let mut catalogs = crate::native::catalog::parse(ctx, &bytes)?.into_iter();
            let Some(catalog) = catalogs.next() else {
                return Ok(0);
            };
            crate::native::CatiaCatalog::from_source(ctx, catalog)
                .map(|catalog| catalog.entries.len())
        },
        |count| *count > 0,
    );
}

#[test]
fn native_finjpl_segments_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_container::summary_preview_segment();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal(
        "catia_native_finjpl_segments",
        |ctx| decoded_native(ctx, &bytes, &records).map(|native| native.finjpl_segments.len()),
        |count| *count > 0,
    );
}

#[test]
fn native_alias_rows_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_container::surface_alias_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal(
        "catia_native_alias_rows",
        |ctx| decoded_native(ctx, &bytes, &records).map(|native| native.alias_rows.len()),
        |count| *count > 0,
    );
}

#[test]
fn native_catalogs_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_object_graph::catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "schema",
    ]);
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal(
        "catia_native_catalogs",
        |ctx| decoded_native(ctx, &bytes, &records).map(|native| native.catalogs.len()),
        |count| *count > 0,
    );
}
