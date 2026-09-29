// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::test_support::{build_prt, assert_annotation, build_prt_raw, jpeg_payload, visibgeom_payload};
use crate::{container, CreoCodec};
use crate::container::{Layout, SectionRole, UnknownLayout};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use cadmpeg_ir::Exactness;
use cadmpeg_test_support::EditableDecodeResult;
use std::io::Cursor;





mod aggregation;
mod sections;

fn assert_summary_limit(
    operation: &'static str,
    dimension: cadmpeg_core::decode::ResourceDimension,
    unknown_layout: bool,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let bytes = if unknown_layout {
        crate::test_support::build_prt_raw("c", &[("VisibGeom", b"payload".to_vec())])
    } else {
        build_prt("c", &[("ND:0:VisibGeom", b"payload".to_vec())])
    };
    let scan = container::scan_bytes_ok(bytes);
    let admitted = crate::decode::with_test_decode_ctx(|ctx| {
        let classification = crate::dialect::classify(ctx, &scan)?;
        container::summarize(ctx, &scan, classification)
    });
    assert!(admitted.is_ok(), "service profile admits the summary");

    let found = (0..4096).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = limit;
            }
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = limit;
            }
            _ => panic!("this summary test uses a collection or retained-byte limit"),
        }
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
        let result = crate::dialect::classify(&ctx, &scan)
            .and_then(|classification| container::summarize(&ctx, &scan, classification));
        matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == dimension && refusal.operation == operation
        )
    });
    assert!(found, "{operation} has a just-below-need refusal");
}

#[test]
fn dialect_declared_nodes_refuse_collection_limit() {
    assert_summary_limit(
        "creo declared dialect nodes",
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        false,
    );
}

#[test]
fn dialect_declared_key_refuses_retained_limit() {
    assert_summary_limit(
        "creo declared dialect key",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        false,
    );
}

#[test]
fn dialect_declared_version_refuses_retained_limit() {
    assert_summary_limit(
        "creo declared version line",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        false,
    );
}

#[test]
fn summary_attribute_nodes_refuse_collection_limit() {
    assert_summary_limit(
        "creo summary attribute nodes",
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        false,
    );
}

#[test]
fn summary_attribute_key_refuses_retained_limit() {
    assert_summary_limit(
        "creo summary attribute key",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        false,
    );
}

#[test]
fn summary_offset_refuses_retained_limit() {
    assert_summary_limit(
        "creo summary offset",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        false,
    );
}

#[test]
fn summary_raw_name_refuses_retained_limit() {
    assert_summary_limit(
        "creo summary raw name",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        false,
    );
}

#[test]
fn summary_entry_name_refuses_retained_limit() {
    assert_summary_limit(
        "creo summary entry name",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        false,
    );
}

#[test]
fn summary_entries_refuse_collection_limit() {
    assert_summary_limit(
        "creo summary entries",
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        false,
    );
}

#[test]
fn container_note_text_refuses_retained_limit() {
    assert_summary_limit(
        "creo container note text",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        false,
    );
}

#[test]
fn container_notes_refuse_collection_limit() {
    assert_summary_limit(
        "creo container notes",
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        false,
    );
}

#[test]
fn unverified_dialect_loss_text_refuses_retained_limit() {
    assert_summary_limit(
        "creo unverified dialect loss text",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        true,
    );
}

#[test]
fn summary_losses_refuse_collection_limit() {
    assert_summary_limit(
        "creo summary losses",
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        true,
    );
}

#[test]
fn summary_expanded_size_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let compressed = crate::test_support::unix_compress_literals(b"ABC");
    let bytes = crate::test_support::build_toc_section_prt("ND:0:VisibGeom", &compressed, 3);
    let scan = container::scan_bytes_ok(bytes);
    assert_eq!(scan.framing.expanded_sections.len(), 1);
    let found = (0..4096).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
        let result = crate::dialect::classify(&ctx, &scan)
            .and_then(|classification| container::summarize(&ctx, &scan, classification));
        matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "creo summary expanded size"
        )
    });
    assert!(found, "expanded size text refuses below its byte need");
}

#[test]
fn topology_face_ids_refuse_before_distinct_node_insertion() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let input = [7_u32, 7, 8];
    let run = |items| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = items;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        super::topology_face_ids(&ctx, input).map(|ids| ids.into_iter().collect::<Vec<_>>())
    };
    assert_eq!(run(2).expect("two distinct nodes admitted"), [7, 8]);
    let error = run(1).expect_err("second distinct node needs admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo topology face ids")
    );
}

#[test]
fn named_datum_plane_refuses_before_aggregate_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let payload = b"\xe0\x01geom_id\0\x02\xe0\x01feat_id\0\x01outline\0\xf9\x02\x03\x18\x46\x08\0\0\0\0\0\0\x46\x08\0\0\0\0\0\0\x18\x46\x08\0\0\0\0\0\0\x46\x08\0\0\0\0\0\0";
    let section = super::Section::scan("ActDatums".to_string(), 0, payload.len(), None, payload)
        .expect("bounded datum section");
    let run = |items| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = items;
        let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
            .expect("datum fixture fits root limit");
        super::datum_planes(&ctx, std::slice::from_ref(&section)).map(|rows| rows.len())
    };
    assert_eq!(run(14).expect("one named plane admitted"), 1);
    let error = run(12).expect_err("named plane aggregate needs admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo named datum plane aggregation")
    );
}

fn scan_primitives_with_limits(
    name: &str,
    bytes: &[u8],
    items: u64,
    retained: u64,
) -> Result<super::PrimitiveScan, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = items;
    policy.limits.max_retained_bytes = retained;
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("the primitive fixture fits the root limit");
    let section = super::ExpandedSection {
        name: name.to_string(),
        source_offset: 0,
        compressed_length: bytes.len(),
        data: bytes.to_vec(),
    };
    super::scan_primitives(&ctx, &[section])
}

#[test]
fn model_double_xar_tables_refuse_before_aggregate_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = b"double_xar\0\xf8\x02\x10\xe0";
    assert_eq!(
        scan_primitives_with_limits("Body", bytes, 4, 4)
            .expect("model dictionary admitted")
            .double_xar_tables
            .len(),
        1
    );
    let error = scan_primitives_with_limits("Body", bytes, 3, 4)
        .err()
        .expect("model dictionary aggregate needs admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo model double_xar tables")
    );
}

#[test]
fn model_double_xar_section_name_refuses_before_copy() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = b"double_xar\0\xf8\x02\x10\xe0";
    let error = scan_primitives_with_limits("Body", bytes, 4, 3)
        .err()
        .expect("model dictionary name needs admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo model double_xar section names")
    );
}

#[test]
fn model_primitive_scalar_arrays_refuse_before_aggregate_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = b"\xe0\x06p1\0\xf8\x01\0";
    assert_eq!(
        scan_primitives_with_limits("SolidPrimdata", bytes, 3, u64::MAX)
            .expect("scalar array admitted")
            .scalar_arrays
            .len(),
        1
    );
    let error = scan_primitives_with_limits("SolidPrimdata", bytes, 2, u64::MAX)
        .err()
        .expect("scalar aggregate needs admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo model primitive scalar arrays")
    );
}

#[test]
fn model_triangle_strips_refuse_before_aggregate_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    let mut bytes =
        b"value(prim_tristripsetwithatt)\0\xe0\x01p_accum_set_size\0\xf8\x01\x03".to_vec();
    bytes.extend_from_slice(b"\xe0\x06mv_p_xyz\0\xf8\x09");
    bytes.extend_from_slice(&[0; 9]);
    assert_eq!(
        scan_primitives_with_limits("SolidPrimdata", &bytes, 28, u64::MAX)
            .expect("triangle strip admitted")
            .triangle_strips
            .len(),
        1
    );
    let error = scan_primitives_with_limits("SolidPrimdata", &bytes, 27, u64::MAX)
        .err()
        .expect("strip aggregate needs admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo model triangle strips")
    );
}

fn feature_row_for_aggregate(body: &[u8]) -> crate::feature::rows::FeatureRow {
    crate::feature::rows::FeatureRow {
        feature_id: 7,
        root_schema_class: None,
        stream_offset: 10,
        body: body.to_vec().try_into().expect("two-byte feature row"),
        body_offset: 100,
        offset: 98,
    }
}

#[test]
fn feature_geometry_table_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let body = b"\xe0\x00dtm_id_tab\0\xf2\xf8\x01\xf7\x57\xfb\xe2\
        \xe0\x01dtm_id\0\x2a\xe0\x01dim_id\0\xf6";
    let row = feature_row_for_aggregate(body);
    let run = |items| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = items;
        let (ctx, _) = DecodeContext::from_root_bytes(body, &arena, &policy)
            .expect("root geometry table input is admitted");
        super::feature_geometry_tables(&ctx, &[], std::slice::from_ref(&row))
            .map(|tables| tables.len())
    };
    assert_eq!(run(5).expect("one aggregate table admitted"), 1);
    let error = run(4).expect_err("aggregate table needs another item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo feature geometry table aggregation"));
}

#[test]
fn feature_affected_id_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let body = b"\xe0\x01geoms_affected\0\xf8\x01\x2a";
    let row = feature_row_for_aggregate(body);
    let run = |items| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = items;
        let (ctx, _) = DecodeContext::from_root_bytes(body, &arena, &policy)
            .expect("root affected-id input is admitted");
        super::feature_affected_ids(&ctx, &[], std::slice::from_ref(&row))
            .map(|records| records.len())
    };
    assert_eq!(run(3).expect("one aggregate record admitted"), 1);
    let error = run(2).expect_err("aggregate record needs another item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo affected-id aggregation"));
}

#[test]
fn revolution_extent_aggregation_refuses_before_vec_growth() {
    use crate::feature::definitions::{DefinitionIdentity, FeatureDefinition};
    use crate::feature::operations::{
        FeatureOperation, FeatureRecipe, OperationKind, OperationName, RecipeResolution,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let body = [
        0x83, 0xdf, 0xf6, 0xe3, 0x00, 0x00, 0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00, 0x00,
        0x00, 0x00,
    ];
    let definition = FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(247),
            owner_feature_id: Some(247),
        },
        body: body.to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let operation = FeatureOperation {
        feature_id: 247,
        kind: OperationKind::Revolve,
        name: OperationName::Derived,
        recipe: RecipeResolution::Resolved(FeatureRecipe::ProtrudeRevolve),
        display_state_conflict: false,
        depdb: None,
        offset: 0,
        state_offset: 0,
    };
    let run = |items| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = items;
        let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy)
            .expect("root extent input is admitted");
        super::feature_revolution_extents(
            &ctx,
            &[],
            std::slice::from_ref(&definition),
            std::slice::from_ref(&operation),
        )
        .map(|records| records.len())
    };
    assert_eq!(run(2).expect("one aggregate extent admitted"), 1);
    let error = run(1).expect_err("aggregate extent needs another item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo revolution extent aggregation"));
}

#[test]
fn feature_definition_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let payload = b"feat_defs_917\0template\xe3S2D0004\0replay";
    let section = super::Section::scan("FeatDefs".to_string(), 0, payload.len(), None, payload)
        .expect("bounded feature section");
    let run = |items| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = items;
        let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
            .expect("root feature input is admitted");
        super::feature_definitions(&ctx, std::slice::from_ref(&section)).map(|rows| rows.len())
    };
    assert_eq!(run(7).expect("one definition admitted"), 1);
    let error = run(6).expect_err("aggregate definition needs an item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo feature definitions"));
}

fn depdb_recipe_rows_with_limits(
    items: u64,
    retained: u64,
) -> Result<usize, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let payload = b"\xe3\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";
    let section = super::Section::scan("DEPDB_DATA".to_string(), 0, payload.len(), None, payload)
        .expect("bounded recipe section");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = items;
    policy.limits.max_retained_bytes = retained;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("root recipe input is admitted");
    super::depdb_recipe_rows(&ctx, &[section]).map(|rows| rows.len())
}

#[test]
fn depdb_recipe_row_body_refuses_before_retained_copy() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    assert_eq!(
        depdb_recipe_rows_with_limits(6, u64::MAX).expect("one recipe row"),
        1
    );
    let error = depdb_recipe_rows_with_limits(6, 0).expect_err("row body needs retained bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo DEPDB recipe row body"));
}

#[test]
fn depdb_recipe_row_refuses_before_vec_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let error = depdb_recipe_rows_with_limits(5, u64::MAX).expect_err("row needs a vector item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo DEPDB recipe rows"));
}

fn reference_scan_with_limit(
    payload: &[u8],
    items: u64,
) -> Result<super::ReferenceScan, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let section = super::Section::scan("MdlRefInfo".to_string(), 0, payload.len(), None, payload)
        .expect("bounded reference section");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = items;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("root reference input is admitted");
    super::reference_scan(&ctx, &[section])
}

#[test]
fn reference_line_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let payload = b"ent_list(line3d)\0\x23\xe3\x23\x0d\xe2\x02\x48\x10\x00\
        \x0f\x0f\x0f\xe4\x0f\x0f\xe4";
    assert_eq!(
        reference_scan_with_limit(payload, 3)
            .expect("line admitted")
            .lines
            .len(),
        1
    );
    let error = reference_scan_with_limit(payload, 2)
        .err()
        .expect("line aggregate needs one item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo reference line aggregation"));
}

#[test]
fn reference_circle_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let payload = b"ent_list(arc_z)\0\xe2\x2d\xe3\x2d\x0f\xe2\x01\
        \xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\xe0\x00ent_list(line3d)\0";
    assert_eq!(
        reference_scan_with_limit(payload, 3)
            .expect("circle admitted")
            .circles
            .len(),
        1
    );
    let error = reference_scan_with_limit(payload, 2)
        .err()
        .expect("circle aggregate needs one item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo reference circle aggregation"));
}

#[test]
fn reference_conic_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let payload = b"ent_list(conic)\0\
        \xe0\x01id\0\x2a\xe0\x01type\0\x1e\
        \xe0\x00gen_info\0\xe2\xf7\x13\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\
        \xe0\x01flip\0\x01\
        \xe0\x02end1\0\xf8\x03\xe4\x0f\x0f\
        \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
        \xe0\x02t0\0\x0f\xe0\x02t1\0\x11\
        \xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
        \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
        \xf2\xf7\x0e\xe3";
    let error = reference_scan_with_limit(payload, 1)
        .err()
        .expect("conic aggregate needs one item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo reference conic aggregation"));
}

#[test]
fn feature_row_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let payload = crate::test_support::allfeatur_row(4, [0xeb, 0x04], 917, &[0xaa]);
    let section =
        container::Section::scan("AllFeatur".to_string(), 0, payload.len(), None, &payload)
            .expect("bounded AllFeatur section");
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
            .expect("root row is admitted");
        super::feature_rows(&ctx, std::slice::from_ref(&section), &BTreeSet::from([4]))
    };
    assert_eq!(run(7).expect("one aggregated row admitted").len(), 1);
    let error = run(6).expect_err("aggregate row needs another Vec item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo feature row aggregation"));
}

#[test]
fn section_result_collector_refuses_before_output_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = [0u8];
    let section =
        super::Section::scan("body".to_string(), 0, 1, None, &bytes).expect("one bounded section");
    let sections = [section];
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("root input is admitted");
        super::collect_section_records_result(
            &ctx,
            sections.iter(),
            |_| Ok(vec![42u32]),
            |_, _| {},
            |_| 0,
        )
    };
    assert_eq!(run(1).expect("one output item is admitted"), vec![42]);
    let error = run(0).expect_err("one output item exceeds the collection limit");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section record aggregation"
    ));
}

fn feature_entity_tables_with_limit(limit: u64) -> Result<usize, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let row = crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 4,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("root input is admitted");
    Ok(super::feature_entity_tables(&ctx, &[], &[4], &[row])?.len())
}

#[test]
fn feature_entity_owner_id_node_refuses_before_insertion() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    assert_eq!(
        feature_entity_tables_with_limit(2).expect("two ids admitted"),
        0
    );
    let error = feature_entity_tables_with_limit(0).expect_err("owner id needs a set node");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo feature entity owner ids"
    ));
}

#[test]
fn feature_entity_surface_id_node_refuses_before_insertion() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let error = feature_entity_tables_with_limit(1).expect_err("surface id follows owner id");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo feature entity surface ids"
    ));
}

#[test]
fn structural_feature_id_node_refuses_before_btree_insertion() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let row = crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 4,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("root input is admitted");
        super::structural_feature_ids(&ctx, &[], std::slice::from_ref(&row), &[])
    };
    assert_eq!(run(1).expect("one structural ID admitted").len(), 1);
    let error = run(0).expect_err("one structural ID needs a BTreeSet node");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo structural feature ids"));
}

#[test]
fn candidate_structural_feature_ids_refuse_before_btree_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("root input is admitted");
        super::candidate_feature_ids(&ctx, &BTreeSet::from([4]), std::iter::empty())
    };
    assert_eq!(
        run(1).expect("one clone node admitted"),
        BTreeSet::from([4])
    );
    let error = run(0).expect_err("cloned structural node needs an item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo candidate structural feature ids"));
}

#[test]
fn candidate_added_feature_id_refuses_before_btree_insertion() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("root input is admitted");
        super::candidate_feature_ids(&ctx, &BTreeSet::from([4]), [5])
    };
    assert_eq!(
        run(2).expect("two candidate nodes admitted"),
        BTreeSet::from([4, 5])
    );
    let error = run(1).expect_err("new candidate node needs an item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo candidate feature ids"));
}

#[test]
fn completed_feature_id_refuses_before_btree_insertion() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("root input is admitted");
        super::complete_feature_ids(&ctx, BTreeSet::from([4]), [5])
    };
    assert_eq!(run(3).expect("complete feature IDs admitted"), vec![4, 5]);
    let error = run(0).expect_err("new complete node needs an item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo complete feature ids"));
}

#[test]
fn ordered_feature_ids_refuse_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("root input is admitted");
        super::complete_feature_ids(&ctx, BTreeSet::from([4]), [5])
    };
    let error = run(2).expect_err("ordered vector needs two items after the new node");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo ordered feature ids"));
}

#[test]
fn feature_row_definition_refuses_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let row = feature_row_for_aggregate(b"prefix gsec2d_ptr\0\xe0\x0aname\0S2D0002\0");
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&row.body, &arena, &policy)
            .expect("feature row is admitted");
        super::feature_row_definitions(&ctx, std::slice::from_ref(&row))
    };
    assert_eq!(run(2).expect("one definition admitted").len(), 1);
    let error = run(1).expect_err("one definition needs another item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo feature row definitions"));
}

fn one_feature_definition() -> crate::feature::definitions::FeatureDefinition {
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::feature::definitions::depdb_section_definition(
            ctx,
            b"prefix gsec2d_ptr\0\xe0\x0aname\0S2D0002\0",
            None,
        )
    })
    .expect("section definition admitted")
    .expect("one bounded section definition")
}

fn append_definition_with_limit(
    limit: u64,
    operation: &'static str,
) -> Result<usize, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("definition input is admitted");
    let mut definitions = Vec::new();
    super::append_feature_definitions(
        &ctx,
        &mut definitions,
        vec![one_feature_definition()],
        operation,
    )?;
    Ok(definitions.len())
}

#[test]
fn feature_row_definition_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let operation = "creo feature row definition aggregation";
    assert_eq!(
        append_definition_with_limit(1, operation).expect("one definition admitted"),
        1
    );
    let error = append_definition_with_limit(0, operation)
        .expect_err("one definition needs an aggregate slot");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == operation));
}

#[test]
fn replay_definition_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let operation = "creo replay definition aggregation";
    assert_eq!(
        append_definition_with_limit(1, operation).expect("one definition admitted"),
        1
    );
    let error = append_definition_with_limit(0, operation)
        .expect_err("one replay definition needs an aggregate slot");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == operation));
}

#[test]
fn claimed_definition_owner_refuses_before_btree_insertion() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let mut definition = one_feature_definition();
    definition.identity = crate::feature::definitions::DefinitionIdentity::Parsed {
        schema_id: None,
        owner_feature_id: Some(7),
    };
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("definition input is admitted");
        super::claimed_definition_owners(&ctx, std::slice::from_ref(&definition))
    };
    assert_eq!(run(1).expect("one owner admitted").len(), 1);
    let error = run(0).expect_err("one owner needs a BTreeSet node");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo claimed definition owners"));
}

#[test]
fn version_line_refuses_before_lossy_retained_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = b" \t#UGC:2 P \xff \t\n";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &policy)
        .expect("version-line input is admitted");
    let error = super::line_at(&ctx, data, 0).expect_err("lossy line needs retained bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo version line"));
}

#[test]
fn version_line_preserves_lossy_unicode_trim_under_service_policy() {
    let data = b" \t#UGC:2 P \xff \t\n";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
        .expect("version-line input is admitted");
    assert_eq!(
        super::line_at(&ctx, data, 0).expect("lossy line admitted"),
        "#UGC:2 P �"
    );
    assert_eq!(
        super::line_at(&ctx, b" \t\n", 0).expect("blank line admitted"),
        ""
    );
}

#[test]
fn two_chart_pcurve_count_node_refuses_before_insertion() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let samples = [
        0x0f, 0xe4, 0x0d, 0x18, 0xe4, 0x0f, 0x18, 0x0d, 0x0d, 0x18, 0xe4, 0x0f,
    ];
    let mut payload = b"topol_ref_data\0".to_vec();
    payload.extend_from_slice(&[7, 0, 4, 1, 0xf6, 0xfc, 3]);
    payload.extend_from_slice(&samples);
    payload.extend_from_slice(&[10, 11, 8, 9, 0, 0, 0xe3, 0xe1, 0xe3]);
    payload.extend_from_slice(&[8, 0, 4, 0xf6, 1]);
    payload.extend_from_slice(&samples);
    payload.extend_from_slice(&[10, 11, 9, 7, 0, 0, 0xe3, 0xe1, 0xe3]);
    let section = super::Section::scan("body".to_string(), 0, payload.len(), None, &payload)
        .expect("one bounded section");
    let sections = [section];
    let face_ids = BTreeSet::from([10, 11]);
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
            .expect("root input is admitted");
        super::two_chart_pcurves(&ctx, &sections, &face_ids)
    };
    assert_eq!(
        run(28)
            .expect("two rows and two count nodes admitted")
            .len(),
        2
    );
    let error = run(26).expect_err("count node follows two collected rows");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo two-chart pcurve counts"
    ));
}

fn current_feature_operations_with_limit(limit: u64) -> Result<usize, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let payload = b"Round id 4\0";
    let section = super::Section::scan("MdlStatus".to_string(), 0, payload.len(), None, payload)
        .expect("one bounded status section");
    let sections = [section];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root input is admitted");
    Ok(super::feature_operations(&ctx, &sections)?.len())
}

#[test]
fn current_feature_operation_node_refuses_before_insertion() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    assert_eq!(
        current_feature_operations_with_limit(8).expect("one operation admitted"),
        1
    );
    let error = current_feature_operations_with_limit(6)
        .expect_err("map node follows the aggregate operation item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo current feature operation nodes"
    ));
}

#[test]
fn current_feature_operation_order_refuses_before_vec_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let error = current_feature_operations_with_limit(7)
        .expect_err("ordered item follows aggregate and map nodes");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo current feature operation order"
    ));
}

#[test]
fn feature_reference_aggregation_refuses_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let payload = b"\xf7\x71\x01\x05\x02N\xff\0\x01\x01";
    let section = super::Section::scan("MdlRefInfo".to_string(), 0, payload.len(), None, payload)
        .expect("one bounded reference section");
    let sections = [section];
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
            .expect("root input is admitted");
        super::feature_reference_names(&ctx, &sections)
    };
    assert_eq!(run(2).expect("one reference admitted").len(), 1);
    let error = run(1).expect_err("one reference needs an aggregate Vec item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo feature reference names"
    ));
}

#[test]
fn detect_matches_ugc_magic_only() {
    let codec = CreoCodec;
    assert_eq!(codec.detect(b"#UGC:2 P foo"), Confidence::High);
    // A Siemens NX `.prt` (shares the extension) must not be claimed here.
    assert_eq!(codec.detect(b"\x0e\x93\x13\x01NX"), Confidence::No);
    assert_eq!(codec.detect(b"PK\x03\x04"), Confidence::No);
    assert_eq!(codec.detect(b""), Confidence::No);
}

#[test]
fn scan_decodes_length_prefixed_native_model_name() {
    let data = b"#UGC:2 PART test \\\n#- CMNM 00bwidget.prt                                      \\\n#-END_OF_UGC_HEADER\n"
        .to_vec();
    let scan = container::scan_bytes_ok(data.clone());

    assert_eq!(
        scan.framing
            .model_name
            .as_ref()
            .map(|model| model.name.as_str()),
        Some("widget.prt ")
    );
    let model_name_offset = data
        .windows(b"widget.prt ".len())
        .position(|window| window == b"widget.prt ")
        .expect("model name offset");
    assert_eq!(
        scan.framing.model_name.as_ref().map(|model| model.offset),
        Some(model_name_offset)
    );
    let result = EditableDecodeResult::from(
        CreoCodec
            .decode(&mut Cursor::new(data), &DecodeOptions::default())
            .expect("decode"),
    );
    assert_eq!(
        result
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("model_name"))
            .map(String::as_str),
        Some("widget.prt ")
    );
    let [product] = result.ir().model.product_definitions.as_slice() else {
        panic!("one part product");
    };
    assert_eq!(product.id.as_str(), "creo:model:product_definition#root");
    assert_eq!(product.part_number.as_deref(), Some("widget.prt "));
    assert_eq!(product.label.as_deref(), Some("widget.prt "));
    assert!(product.bodies.is_empty());
    let [occurrence] = result.ir().model.occurrences.as_slice() else {
        panic!("one root occurrence");
    };
    assert!(matches!(
        &occurrence.prototype,
        cadmpeg_ir::products::PrototypeReference::Local { definition }
            if definition == &product.id
    ));
    assert!(matches!(
        occurrence.parent,
        cadmpeg_ir::products::OccurrenceParent::Root {}
    ));
    assert_eq!(
        occurrence.transform,
        cadmpeg_ir::transform::Transform::identity()
    );
    assert_annotation(
        &result.source_fidelity().annotations,
        product.id.as_str(),
        "creo:archive_header",
        model_name_offset as u64,
        "part_product",
        Exactness::Derived,
    );
    assert_annotation(
        &result.source_fidelity().annotations,
        occurrence.id.as_str(),
        "creo:archive_header",
        model_name_offset as u64,
        "part_product_occurrence",
        Exactness::Derived,
    );
}

#[test]
fn scan_withholds_repeated_native_model_names() {
    let data = b"#UGC:2 PART test \\\n+#- CMNM 00awidget.prt                                      \\\n+#- CMNM 00bwidget2.prt                                     \\\n+#-END_OF_UGC_HEADER\n"
        .to_vec();

    let scan = container::scan_bytes_ok(data);
    assert!(scan.framing.model_name.is_none());
}

#[test]
fn scan_decodes_binary_model_name_field_without_cmnm_header() {
    let data = build_prt(
        "test",
        &[(
            "BasicData",
            b"e0\x0amodel_name\0\xf1WIDGET_ROOT\0e0\x00disp_outl_info\0".to_vec(),
        )],
    );

    let scan = container::scan_bytes_ok(data.clone());
    assert_eq!(
        scan.framing
            .model_name
            .as_ref()
            .map(|model| model.name.as_str()),
        Some("WIDGET_ROOT")
    );
    let model_name_offset = data
        .windows(b"WIDGET_ROOT".len())
        .position(|window| window == b"WIDGET_ROOT")
        .expect("model name offset");
    assert_eq!(
        scan.framing.model_name.as_ref().map(|model| model.offset),
        Some(model_name_offset)
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    assert_eq!(
        result
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("model_name"))
            .map(String::as_str),
        Some("WIDGET_ROOT")
    );
}

#[test]
fn scan_skips_empty_binary_model_name_fields() {
    let data = build_prt(
        "test",
        &[(
            "BasicData",
            b"e0\x0amodel_name\0\xe1e0\x0amodel_name\0ROOT\0".to_vec(),
        )],
    );

    let scan = container::scan_bytes_ok(data);
    assert_eq!(
        scan.framing
            .model_name
            .as_ref()
            .map(|model| model.name.as_str()),
        Some("ROOT")
    );
}

#[test]
fn relation_model_name_accepts_binary_root_name() {
    assert_eq!(
        super::relation_model_name("DRILL_BIT_10D0_SUPPRESSED_FEAT"),
        Some("DRILL_BIT_10D0_SUPPRESSED_FEAT")
    );
    assert_eq!(super::relation_model_name("widget.PrT "), Some("widget"));
    assert_eq!(super::relation_model_name("widget.step"), None);
}

#[test]
fn scan_enumerates_and_classifies_sections() {
    let data = build_prt(
        "test",
        &[
            ("VisibGeom", visibgeom_payload(5, 12)),
            ("AllFeatur", vec![0x01, 0x02, 0x03]),
            ("THMB_IMG_MAIN", jpeg_payload()),
        ],
    );
    let scan = container::scan_bytes_ok(data);

    assert_eq!(scan.framing.version_line, "#UGC:2 P test");
    assert_eq!(scan.framing.sections.len(), 3);
    assert_eq!(scan.framing.sections[0].name(), "VisibGeom");
    assert_eq!(scan.framing.sections[0].role(), SectionRole::PsbGeometry);
    assert_eq!(scan.framing.sections[1].name(), "AllFeatur");
    assert_eq!(scan.framing.sections[1].role(), SectionRole::ModelData);
    assert_eq!(scan.framing.sections[2].role(), SectionRole::Thumbnail);
    assert!(container::has_thumbnail(&scan));
}

#[test]
fn scan_finds_curve_expression_in_feature_definition_section() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01value=5\0"
        .to_vec();
    let scan = container::scan_bytes_ok(build_prt("c", &[("FeatDefs", payload)]));

    assert_eq!(scan.curves.expressions.len(), 1);
    assert_eq!(scan.curves.expressions[0].entity_id, 7);
    assert_eq!(scan.curves.expressions[0].lines[0].text, "value=5");
}

#[test]
fn scan_enumerates_toc_backed_compound_close_section_boundaries() {
    let mut data = b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n#UGC_TOC\n\
        DEPDB_DATA 1 2 3\nVisibGeom 4 5 6\nAllFeatur 7 8 9\n\
        #END_OF_TOC_HEADER\n#DEPDB_DATA\nopaque"
        .to_vec();
    data.extend_from_slice(b"\xf1#VisibGeom\npacked\xf1#not_in_toc\ninside");
    data.extend_from_slice(b"\xf1#AllFeatur\nfeatures");

    let scan = container::scan_bytes_ok(data);

    assert_eq!(
        scan.framing
            .sections
            .iter()
            .map(super::Section::name)
            .collect::<Vec<_>>(),
        ["DEPDB_DATA", "VisibGeom", "AllFeatur"]
    );
    assert_eq!(scan.framing.sections[1].role(), SectionRole::PsbGeometry);
    assert_eq!(scan.framing.sections[2].role(), SectionRole::ModelData);
}

#[test]
fn scan_uses_fixed_width_toc_offsets_for_adjacent_sections() {
    let mut data = b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n".to_vec();
    let header_base = data.len();
    data.extend_from_slice(format!("{:<80}\n", "#UGC_TOC 2 2 81 17").as_bytes());
    let first_offset = 3 * 81;
    let first = b"#SolidPrimdata\nabc";
    let second_offset = first_offset + first.len();
    let second = b"#VisibGeom\nxyz";
    data.extend_from_slice(
        format!(
            "{:<80}\n",
            format!("SolidPrimdata {first_offset:x} {:x} 0", first.len())
        )
        .as_bytes(),
    );
    data.extend_from_slice(
        format!(
            "{:<80}\n",
            format!("VisibGeom {second_offset:x} {:x} 0", second.len())
        )
        .as_bytes(),
    );
    assert_eq!(data.len(), header_base + first_offset);
    data.extend_from_slice(first);
    data.extend_from_slice(second);

    let scan = container::scan_bytes_ok(data);

    assert_eq!(scan.framing.sections.len(), 2);
    assert_eq!(scan.framing.sections[0].name(), "SolidPrimdata");
    assert_eq!(scan.framing.sections[0].length, first.len());
    assert_eq!(scan.framing.sections[1].name(), "VisibGeom");
    assert_eq!(scan.framing.sections[1].offset, header_base + second_offset);
}

#[test]
fn scan_expands_toc_sized_unix_compress_payload() {
    let mut data = b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n".to_vec();
    let header_base = data.len();
    data.extend_from_slice(format!("{:<80}\n", "#UGC_TOC 2 1 81 17").as_bytes());
    let section_offset = 2 * 81;
    let compressed = [0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01];
    let section_length = b"#SolidPrimdata\n".len() + compressed.len();
    data.extend_from_slice(
        format!(
            "{:<80}\n",
            format!("SolidPrimdata {section_offset:x} {section_length:x} 3")
        )
        .as_bytes(),
    );
    assert_eq!(data.len(), header_base + section_offset);
    data.extend_from_slice(b"#SolidPrimdata\n");
    data.extend_from_slice(&compressed);

    let scan = container::scan_bytes_ok(data);
    let classification =
        crate::decode::with_test_decode_ctx(|ctx| crate::dialect::classify(ctx, &scan))
            .expect("dialect classification admitted");

    assert_eq!(scan.framing.expanded_sections.len(), 1);
    assert_eq!(scan.framing.expanded_sections[0].data, b"ABC");
    let summary =
        crate::decode::with_test_decode_ctx(|ctx| container::summarize(ctx, &scan, classification))
            .expect("container summary admitted");
    let cadmpeg_core::container::EntryStorage::Compressed {
        method,
        stored,
        expanded,
    } = summary.entries[0].storage
    else {
        panic!("a unix-compressed section stores compressed bytes");
    };
    assert_eq!(
        method,
        cadmpeg_core::container::CompressionMethod::UnixCompress
    );
    assert_eq!(stored, Some(section_length as u64));
    assert_eq!(expanded, Some(18));
}

#[test]
fn scan_reads_namespace_counts() {
    let data = build_prt("c", &[("VisibGeom", visibgeom_payload(5, 12))]);
    let scan = container::scan_bytes_ok(data);
    assert_eq!(scan.framing.census.srf_array_count, Some(5));
    assert_eq!(scan.framing.census.crv_array_count, Some(12));
}

#[test]
fn geometry_array_census_overflow_error_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let entry = b"srf_array\0\xf8\xbf\xff";
    let count = (u32::MAX / 16_383) + 1;
    let mut region = Vec::with_capacity(entry.len() * count as usize);
    for _ in 0..count {
        region.extend_from_slice(entry);
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = super::read_array_count(&ctx, &region, b"srf_array")
        .expect_err("sum exceeds the 32-bit census");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo geometry array census error")
    );
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = super::read_array_count(ctx, &region, b"srf_array")
            .expect_err("sum exceeds the 32-bit census");
        assert!(error.to_string().contains("32-bit census"));
        Ok::<(), cadmpeg_core::CodecError>(())
    })
    .expect("service error text admitted");
}

#[test]
fn scan_sums_concatenated_depdb_surface_namespaces() {
    let mut payload = visibgeom_payload(3, 4);
    payload.extend_from_slice(&visibgeom_payload(5, 6));
    let scan = container::scan_bytes_ok(build_prt("c", &[("DEPDB_DATA", payload)]));

    assert_eq!(scan.framing.layout, Layout::Depdb);
    assert_eq!(scan.framing.census.srf_array_count, Some(8));
    assert_eq!(scan.framing.census.crv_array_count, Some(10));
}

#[test]
fn scan_does_not_treat_unlabeled_depdb_bytes_as_geometry_rows() {
    let payload = vec![7, 0x22, 4, 0x01, 0, 8, 8, 0x24, 4, 0xf6, 0x01, 0];
    let scan = container::scan_bytes_ok(build_prt("c", &[("DEPDB_DATA", payload)]));

    assert!(scan.surfaces.rows.is_empty());
    assert!(scan.surfaces.parameters.is_empty());
}

#[test]
fn scan_reads_declared_geomlists_body_count() {
    let scan = container::scan_bytes_ok(build_prt(
        "c",
        &[("Geomlists", b"n_bodies\0\x83\x01".to_vec())],
    ));

    assert_eq!(scan.framing.declared_body_count, Some(769));
}

#[test]
fn scan_reads_geomlists_first_quilt_discriminator() {
    let scan = container::scan_bytes_ok(build_prt(
        "c",
        &[("Geomlists", b"first_quilt_ptr\0\x00".to_vec())],
    ));

    assert_eq!(scan.framing.first_quilt_ptr, Some(0));
}

#[test]
fn scan_reads_legacy_geom_depend_first_quilt_discriminator() {
    let data = b"#UGC:2 PART c\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
@Sld_GeomDepend 1 0\n0 1 ->\n\
@first_quilt_ptr 4 1\n1 4 0\n\
#END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n"
        .to_vec();

    let scan = container::scan_bytes_ok(data);

    assert!(matches!(scan.framing.layout, Layout::LegacyAscii(_)));
    assert_eq!(scan.framing.first_quilt_ptr, Some(0));
}

#[test]
fn legacy_geom_depend_discriminator_withholds_distinct_values() {
    let root = crate::legacy::ObjectRecord {
        name: "Sld_GeomDepend".to_string(),
        attribute_id: 1,
        scope_offset: 0,
        parent: None,
        depth: 0,
        payload: crate::legacy::ObjectPayload::Arrow,
        offset: 0,
    };
    let persistence = crate::legacy::Persistence {
        objects: vec![root],
        integer_values: crate::legacy::TypedValues {
            rows: vec![
                crate::legacy::IntegerRecord {
                    name: "first_quilt_ptr".to_string(),
                    attribute_id: 4,
                    scope_offset: 0,
                    parent: Some(0),
                    depth: 1,
                    payload: crate::legacy::NumericPayload::Scalar { value: 0 },
                    offset: 1,
                },
                crate::legacy::IntegerRecord {
                    name: "first_quilt_ptr".to_string(),
                    attribute_id: 4,
                    scope_offset: 0,
                    parent: Some(0),
                    depth: 1,
                    payload: crate::legacy::NumericPayload::Scalar { value: 7 },
                    offset: 2,
                },
            ],
            unresolved_count: 0,
        },
        ..Default::default()
    };

    assert_eq!(
        super::legacy_geom_depend_value(&persistence, "first_quilt_ptr"),
        None
    );
}

#[test]
fn nd_decoration_selects_nd_layout() {
    let data = build_prt("c", &[("ND:0:VisibGeom:1", visibgeom_payload(3, 4))]);
    let scan = container::scan_bytes_ok(data);
    assert_eq!(scan.framing.layout, Layout::Nd);
    // The decorated name is normalized for classification and census.
    assert_eq!(scan.framing.sections[0].name(), "VisibGeom");
    assert_eq!(scan.framing.sections[0].raw_name, "ND:0:VisibGeom:1");
    assert_eq!(scan.framing.census.srf_array_count, Some(3));
}

#[test]
fn depdb_root_record_overrides_embedded_nd_decoration() {
    let data = build_prt(
        "c",
        &[
            ("DEPDB_DATA", b"\xe0\x00p_dep_db\0\xe3".to_vec()),
            ("ND:0:Model_L05P:1", Vec::new()),
        ],
    );
    let scan = container::scan_bytes_ok(data);
    assert_eq!(scan.framing.layout, Layout::Depdb);
}

#[test]
fn depdb_layout_requires_root_record() {
    let data = build_prt_raw(
        "c",
        &[
            ("DEPDB_DATA", b"not-a-root".to_vec()),
            ("ND:0:Model_L05P:1", Vec::new()),
        ],
    );
    let scan = container::scan_bytes_ok(data);
    assert_eq!(
        scan.framing.layout,
        Layout::Unknown(UnknownLayout::DepdbRootMissing)
    );
}

#[test]
fn visible_geometry_namespace_excludes_invisible_and_depdb_rows() {
    let mut visible = visibgeom_payload(1, 0);
    visible.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0, 0xe4, 0xe3]);
    visible.extend_from_slice(b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04");
    visible
        .extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let mut invisible = visibgeom_payload(1, 0);
    invisible.extend_from_slice(&[8, 0x26, 5, 0x01, 0, 0, 0xe4, 0xe3]);
    invisible.extend_from_slice(b"srf_prim_ptr(cylinder)\0\xe0\x01radius\0\xe4");
    invisible.extend_from_slice(b"crv_array\0crv_id\0\x07type\0\x09feat_id\0\x05");
    invisible
        .extend_from_slice(b"topol_ref_data\0\x07\x09\x05\x01\xf6\x0c\x0d\x07\x07\0\0\xe3\xe1\xe3");
    let mut depdb = visibgeom_payload(1, 0);
    depdb.extend_from_slice(&[9, 0x26, 6, 0x01, 0, 0, 0xe4, 0xe3]);

    let scan = container::scan_bytes_ok(build_prt(
        "c",
        &[
            ("VisibGeom", visible),
            ("NovisGeom", invisible),
            ("DEPDB_DATA", depdb),
        ],
    ));

    assert_eq!(
        scan.surfaces
            .rows
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        [7]
    );
    assert_eq!(
        scan.surfaces
            .parameters
            .iter()
            .map(|record| record.surface_id)
            .collect::<Vec<_>>(),
        [7]
    );
    assert_eq!(
        scan.surfaces
            .nonvisible_rows
            .iter()
            .map(|row| (row.id, row.feature_id))
            .collect::<Vec<_>>(),
        [(8, 5)]
    );
    assert_eq!(scan.curves.prototypes.len(), 1);
    assert_eq!(scan.surfaces.nonvisible_parameters.len(), 1);
    assert_eq!(scan.surfaces.nonvisible_parameters[0].surface_id, 8);
    assert_eq!(
        scan.surfaces.nonvisible_parameters[0].scalar_values(),
        [1.0]
    );
    assert_eq!(scan.surfaces.nonvisible_prototype_records.len(), 1);
    assert_eq!(
        scan.surfaces.nonvisible_prototype_records[0].family.name(),
        "cylinder"
    );
    assert_eq!(scan.curves.nonvisible_prototypes.len(), 1);
    assert_eq!(scan.curves.nonvisible_prototypes[0].feature_id, Some(5));
    assert_eq!(scan.curves.parameters.len(), 1);
    assert_eq!(scan.curves.nonvisible_parameters.len(), 1);
    assert_eq!(
        scan.curves.topology_rows[0].faces,
        [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(11)]
    );
    assert_eq!(
        scan.curves.nonvisible_topology_rows[0].faces,
        [std::num::NonZeroU32::new(12), std::num::NonZeroU32::new(13)]
    );
    assert_eq!(scan.topology.half_edges.len(), 2);

    let result = CreoCodec
        .decode(
            &mut Cursor::new(scan.framing.data.clone()),
            &DecodeOptions::default(),
        )
        .expect("decode");
    let rows = &result.ir().native.namespace("creo").unwrap().arenas()["nonvisible_surface_rows"];
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id(), "creo:novisgeom:surface_row#8");
    assert_eq!(rows[0].fields()["source_section"], "NovisGeom");
    let namespace = result.ir().native.namespace("creo").unwrap();
    let surface_parameters = &namespace.arenas()["nonvisible_surface_parameters"];
    assert_eq!(
        surface_parameters[0].id(),
        "creo:novisgeom:surface_parameter#8"
    );
    assert_eq!(surface_parameters[0].fields()["slots"][0]["value"], 1.0);
    let surface_prototypes = &namespace.arenas()["nonvisible_surface_prototypes"];
    assert!(surface_prototypes[0]
        .id()
        .starts_with("creo:novisgeom:surface_prototype#"));
    assert_eq!(
        surface_prototypes[0].fields()["source_section"],
        "NovisGeom"
    );
    let prototypes = &namespace.arenas()["nonvisible_curve_prototypes"];
    assert_eq!(prototypes[0].fields()["curve_id"], 7);
    assert_eq!(prototypes[0].fields()["source_section"], "NovisGeom");
    let parameters = &namespace.arenas()["nonvisible_curve_parameters"];
    assert_eq!(parameters[0].id(), "creo:novisgeom:curve_parameter#7");
    let topology = &namespace.arenas()["nonvisible_curve_topology_rows"];
    assert_eq!(topology[0].id(), "creo:novisgeom:curve_topology#7");
    assert_eq!(topology[0].fields()["faces"][0], 12);
}

#[test]
fn depdb_data_with_sparse_sections_selects_depdb() {
    let depdb = b"srf_array\0geom_id\0\x07geom_type\0\x22feat_id\0\x04orient\0\x01boundary_type\0\0next_geom_ptr\0\0feat_defs_12\0protrevolve\0Revolve id 17\0".to_vec();
    let data = build_prt("c", &[("VisibGeom", vec![0x00]), ("DEPDB_DATA", depdb)]);
    let scan = container::scan_bytes_ok(data);
    assert_eq!(scan.framing.layout, Layout::Depdb);
    assert!(scan
        .surfaces
        .rows
        .iter()
        .any(|row| row.id == 7 && row.feature_id == 4));
    assert!(scan
        .features
        .definitions
        .iter()
        .any(|definition| definition.identity.id() == 12));
    assert_eq!(scan.features.operations.len(), 1);
    assert_eq!(scan.features.operations[0].feature_id, 17);
    assert_eq!(
        scan.features.operations[0].recipe.resolved(),
        Some(crate::feature::operations::FeatureRecipe::ProtrudeRevolve)
    );
}

#[test]
fn framing_names_are_not_mistaken_for_sections() {
    let data = build_prt("c", &[("VisibGeom", vec![0x00])]);
    let scan = container::scan_bytes_ok(data);
    // Only VisibGeom — the header/TOC framing markers are excluded.
    assert_eq!(scan.framing.sections.len(), 1);
    assert_eq!(scan.framing.sections[0].name(), "VisibGeom");
}

#[test]
fn inspect_summary_has_layout_and_census_notes() {
    let data = build_prt("c", &[("ND:0:VisibGeom:1", visibgeom_payload(7, 9))]);
    let mut reader = Cursor::new(data);
    let summary = CreoCodec
        .inspect(
            &mut reader,
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect("inspect");
    assert_eq!(summary.format(), "creo");
    assert_eq!(summary.container_kind, "psb");
    assert!(summary.notes.iter().any(|n| n.contains("layout: ND")));
    assert!(summary.notes.iter().any(|n| n.contains("srf_array=7")));
}

#[test]
fn a_section_extent_the_file_does_not_hold_is_not_a_section() {
    let data = b"#Geomlists\n0123";

    // One byte past the last byte of the file.
    assert!(
        container::Section::scan("Geomlists".to_string(), 0, data.len() + 1, None, data).is_none()
    );

    // An offset and a length that state no address between them: the end is
    // before the offset, which is what an overflowing `offset + length` leaves.
    assert!(
        container::Section::scan("Geomlists".to_string(), usize::MAX - 3, 12, None, data).is_none()
    );
}

#[test]
fn a_section_that_ends_on_the_last_byte_is_admitted() {
    let data = b"#Geomlists\n0123";
    let section = container::Section::scan("Geomlists".to_string(), 0, data.len(), None, data)
        .expect("section extent")
        .section;

    assert_eq!(section.offset(), 0);
    assert_eq!(section.length(), data.len());
    assert_eq!(section.end(), data.len());
    assert_eq!(
        container::section_region(data, &section).expect("the section is a region of `data`"),
        data.as_slice()
    );
}

/// Inside the scan a section carries the bytes it was admitted against, so a
/// reader reads them with no second bound and no `Option`. The reader below
/// takes `ScannedSection`; the owned `Section` carries no region and does not
/// compile in its place.
#[test]
fn an_in_scan_reader_reads_the_region_its_section_was_admitted_against() {
    let data = b"#VisibGeom\nsrf_array\0";
    let scanned = container::Section::scan("VisibGeom".to_string(), 0, data.len(), None, data)
        .expect("section extent");
    assert_eq!(scanned.region, data.as_slice());

    let selected = crate::decode::with_test_decode_ctx(|ctx| {
        super::model_geometry_sections(ctx, std::slice::from_ref(&scanned))
    })
    .expect("one model geometry section is admitted");
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].section.name(), "VisibGeom");
    assert_eq!(selected[0].region, data.as_slice());
}

fn model_geometry_section_with_limits(
    collection_items: u64,
    retained_bytes: u64,
) -> Result<usize, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let data = b"#VisibGeom\nsrf_array\0";
    let scanned = container::Section::scan("VisibGeom".to_string(), 0, data.len(), None, data)
        .expect("section extent");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_items;
    policy.limits.max_retained_bytes = retained_bytes;
    let (ctx, _) =
        DecodeContext::from_root_bytes(data, &arena, &policy).expect("root input is admitted");
    Ok(super::model_geometry_sections(&ctx, &[scanned])?.len())
}

#[test]
fn model_geometry_section_vec_refuses_before_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    assert_eq!(
        model_geometry_section_with_limits(1, 32).expect("one section admitted"),
        1
    );
    let error = model_geometry_section_with_limits(0, 32)
        .expect_err("one selected section requires a vector item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo model geometry sections"
    ));
}

#[test]
fn copied_section_name_refuses_before_retained_text_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let error =
        model_geometry_section_with_limits(1, 0).expect_err("section name needs retained bytes");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo copied section names"
    ));
}

#[test]
fn nonvisible_geometry_section_refuses_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let data = b"nonvisible";
    let scanned = container::Section::scan("NovisGeom".to_string(), 0, data.len(), None, data)
        .expect("section extent");
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(data, &arena, &policy).expect("root input is admitted");
        super::nonvisible_geometry_sections(&ctx, std::slice::from_ref(&scanned))
    };
    assert_eq!(run(1).expect("one nonvisible section admitted").len(), 1);
    let error = run(0).expect_err("one section requires an output Vec item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo nonvisible geometry sections"
    ));
}

#[test]
fn loop_array_section_sources_refuse_before_vec_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    for (name, data, source) in [
        ("VisibGeom", b"model".as_slice(), 0),
        ("NovisGeom", b"nonvisible".as_slice(), 1),
        ("Xsections", b"Sld_Xsections\0".as_slice(), 2),
    ] {
        let scanned = container::Section::scan(name.to_string(), 0, data.len(), None, data)
            .expect("section extent");
        let run = |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &policy)
                .expect("root input is admitted");
            let sections = std::slice::from_ref(&scanned);
            let (model, nonvisible, all) = match source {
                0 => (sections, &[][..], &[][..]),
                1 => (&[][..], sections, &[][..]),
                _ => (&[][..], &[][..], sections),
            };
            super::loop_array_sections(&ctx, model, nonvisible, all)
        };
        assert_eq!(run(1).expect("one loop section admitted").len(), 1);
        let error = run(0).expect_err("one loop section requires an output Vec item");
        assert!(matches!(
            error,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo loop array sections"
        ));
    }
}

fn loop_array_aggregate_fixture() -> Vec<u8> {
    let mut payload = b"lo_array\0\xf3\xf8\x01\xf7\x2a\xfb\xe3".to_vec();
    for (name, value) in [
        (b"lo_id".as_slice(), 1),
        (b"lo_type".as_slice(), 2),
        (b"lo_subtype".as_slice(), 3),
        (b"feat_id".as_slice(), 4),
        (b"attributes".as_slice(), 5),
        (b"direction".as_slice(), 6),
        (b"next_lo_ptr".as_slice(), 7),
        (b"object_data".as_slice(), 8),
    ] {
        payload.extend_from_slice(&[0xe0, 0x01]);
        payload.extend_from_slice(name);
        payload.extend_from_slice(&[0, value]);
    }
    payload.extend_from_slice(&[0xf1, 0xf7, 0x2a, 0xe3]);
    payload.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 0xe2, 0x10, 0xe3]);
    payload.extend_from_slice(b"srf_array\0");
    payload
}

fn assert_loop_array_aggregate_refusal(limit: u64, operation: &'static str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let payload = loop_array_aggregate_fixture();
    let scanned =
        container::Section::scan("VisibGeom".to_string(), 0, payload.len(), None, &payload)
            .expect("loop array section extent");
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
            .expect("loop array input admitted");
        super::loop_array_scan(&ctx, std::slice::from_ref(&scanned))
    };
    assert_eq!(
        run(u64::MAX)
            .expect("service loop array aggregate")
            .records
            .len(),
        1
    );
    let error = run(limit).expect_err("loop array aggregate exceeds collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation));
}

#[test]
fn loop_array_aggregate_frame_refuses_before_growth() {
    assert_loop_array_aggregate_refusal(3, "creo loop array aggregate frames");
}

#[test]
fn loop_array_aggregate_record_refuses_before_growth() {
    assert_loop_array_aggregate_refusal(4, "creo loop array aggregate records");
}

#[test]
fn a_section_contains_its_own_offset_and_every_byte_before_its_end() {
    let data = b"0123#Geomlists\n0123";
    let section = container::Section::scan("Geomlists".to_string(), 4, data.len(), None, data)
        .expect("section extent")
        .section;

    assert!(!section.contains(section.offset() - 1));
    assert!(section.contains(section.offset()));
    assert!(section.contains(section.end() - 1));
    assert!(!section.contains(section.end()));
}

#[test]
fn a_section_region_is_exactly_the_bytes_between_the_section_offset_and_its_end() {
    let data = build_prt(
        "test",
        &[
            ("Geomlists", b"0123".to_vec()),
            ("Xsections", b"ABCD".to_vec()),
        ],
    );
    let scan = container::scan_bytes_ok(data.clone());
    let section = scan
        .framing
        .sections
        .iter()
        .find(|section| section.name() == "Xsections")
        .expect("the scan enumerates the second section");

    // The region is read through the scan that proved it, so it is the file's
    // own bytes over the section's own extent, with no refusal in between.
    assert_eq!(
        container::section_region(&scan.framing.data, section)
            .expect("the scan enumerated the section over this file"),
        &data[section.offset()..section.end()]
    );
    assert!(container::section_region(&scan.framing.data, section)
        .expect("the scan enumerated the section over this file")
        .starts_with(b"#Xsections\nABCD"));
}

/// `section_region` answers `None` on a slice the section's extent leaves. The
/// bound is the slice's own, so this is a statement about the function, not
/// about a decode route: no production caller passes anything but the scanned
/// file.
#[test]
fn a_section_region_is_absent_from_a_slice_shorter_than_the_section() {
    let data = build_prt(
        "test",
        &[
            ("Geomlists", b"0123".to_vec()),
            ("Xsections", b"ABCD".to_vec()),
        ],
    );
    let scan = container::scan_bytes_ok(data.clone());
    let section = scan
        .framing
        .sections
        .iter()
        .find(|section| section.name() == "Xsections")
        .expect("the scan enumerates the second section");

    let truncated = &data[..section.end() - 1];
    assert_eq!(container::section_region(truncated, section), None);
    assert_eq!(
        container::section_region(&data[..section.end()], section),
        Some(&data[section.offset()..section.end()])
    );
}

#[test]
fn legacy_framing_box_refuses_its_retained_slot() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let framing = super::LegacyAsciiFraming {
        schema: "6".into(),
        product_release: None,
        banner_offset: 0,
        object_offset: 0,
        persistence: crate::legacy::Persistence::default(),
    };
    assert!(
        matches!(super::identify_layout(&ctx, &[], &[], Some(framing)), Err(cadmpeg_core::CodecError::ResourceLimit(resource)) if resource.operation == "creo legacy framing box")
    );
}
