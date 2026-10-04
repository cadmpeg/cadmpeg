// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::test_support::build_prt;

use crate::container;

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
        scan_primitives_with_limits(
            "Body",
            bytes,
            4,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| scan_primitives_with_limits("Body", bytes, u64::MAX, cap)
            )
        )
        .expect("model dictionary admitted")
        .double_xar_tables
        .len(),
        1
    );
    let error = scan_primitives_with_limits(
        "Body",
        bytes,
        3,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            None,
            |cap| scan_primitives_with_limits("Body", bytes, u64::MAX, cap),
        ),
    )
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
    let error = scan_primitives_with_limits(
        "Body",
        bytes,
        4,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo model double_xar section names"),
            |cap| scan_primitives_with_limits("Body", bytes, 4, cap),
        ),
    )
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
    let error = depdb_recipe_rows_with_limits(
        6,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo DEPDB recipe row body"),
            |cap| depdb_recipe_rows_with_limits(6, cap),
        ),
    )
    .expect_err("row body needs retained bytes");
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
            sections.iter().map(Ok),
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

mod framing;

#[test]
fn duplicate_primitive_section_namespace_is_refused() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let section = |source_offset| super::ExpandedSection {
            name: "SolidPrimdata".to_owned(),
            source_offset,
            compressed_length: 0,
            data: Vec::new(),
        };
        let error = super::scan_primitives(ctx, &[section(1), section(2)])
            .map(|scan| scan.scalar_arrays.len())
            .expect_err("distinct sections cannot share primitive identity");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed { .. }));
    });
}

#[test]
fn expanded_section_local_ceiling_is_a_refusal() {
    let bytes = b"#Body\n\x1f\x9d\x10";
    let section = super::Section::scan(
        "Body".to_owned(),
        0,
        bytes.len(),
        Some(256 * 1024 * 1024 + 1),
        bytes,
    )
    .expect("section extent");
    crate::decode::with_test_decode_ctx(|ctx| {
        let error =
            super::expanded_sections(ctx, bytes, &[section]).expect_err("expansion ceiling");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "creo expanded section ceiling")
        );
    });
}

#[test]
fn container_framing_misses_and_text_copies_refuse_work() {
    let bytes = build_prt("test", &[("VisibGeom", vec![0; 64])]);
    crate::test_support::assert_work_boundaries(
        &[
            "creo container model-name scan",
            "creo version line scan",
            "creo version text work",
            "creo container header scans",
            "creo container TOC scans",
            "creo TOC discovery scan",
            "creo section framing scan",
        ],
        |ctx| super::scan_bytes(ctx, bytes.as_slice()),
    );
}

mod unit_selection;

mod work_admission;

#[test]
fn legacy_layout_equality_refuses_work_for_nested_strings_and_numeric_runs() {
    let data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        @root 1 0\n@numbers 2 5\n@names 3 10\n0 1 ->\n\
        1 2 [3]\n$0,2*144\n1 3 [2][81]\n2 3 first\n2 3 second\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n";
    let scan = container::scan_bytes_ok(data);
    let layout = &scan.framing.layout;
    let container::Layout::LegacyAscii(framing) = layout else {
        panic!("legacy layout fixture");
    };
    assert_eq!(framing.persistence.type_5_values.rows.len(), 1);
    let crate::legacy::NumericPayload::Array(numbers) =
        &framing.persistence.type_5_values.rows[0].payload
    else {
        panic!("numeric run array fixture");
    };
    assert_eq!(numbers.dimensions(), [3]);
    assert_eq!(numbers.runs().len(), 2);
    assert_eq!(framing.persistence.string_values.len(), 1);
    let crate::legacy::StringPayload::Array { values, .. } =
        &framing.persistence.string_values[0].payload
    else {
        panic!("nested string array fixture");
    };
    assert_eq!(values.len(), 2);
    assert!(matches!(&values[0], Ok(crate::legacy::StringValue::Utf8 { text }) if text == "first"));
    assert!(matches!(&values[1], Ok(crate::legacy::StringValue::Utf8 { text }) if text == "second"));

    use cadmpeg_core::decode::cost::DecodeCost;
    let mut expanded = layout.clone();
    let container::Layout::LegacyAscii(expanded_framing) = &mut expanded else {
        panic!("legacy layout copy");
    };
    let crate::legacy::StringPayload::Array { values, .. } =
        &mut expanded_framing.persistence.string_values[0].payload
    else {
        panic!("nested string array copy");
    };
    values[0] = Ok(crate::legacy::StringValue::Utf8 { text: "firsté".to_owned() });
    crate::decode::with_test_decode_ctx(|ctx| {
        let original_cost = layout.decode_cost(ctx, "creo legacy Layout equality")?;
        let expanded_cost = expanded.decode_cost(ctx, "creo legacy Layout equality")?;
        // The added UTF-8 character contributes two retained string bytes.
        assert_eq!(expanded_cost.checked_sub(original_cost), Some(2));
        assert!(!ctx.equal(layout, &expanded, "creo legacy Layout equality")?);
        Ok::<_, cadmpeg_core::CodecError>(())
    }).expect("nested string cost admitted");

    crate::test_support::assert_work_boundaries(&["creo legacy Layout equality"], |ctx| {
        ctx.equal(layout, layout, "creo legacy Layout equality")
    });
}
