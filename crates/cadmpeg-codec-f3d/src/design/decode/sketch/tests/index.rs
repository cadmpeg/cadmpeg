// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::sketch::{
    decode_headers_for_indices_from_stream, insert_entity_module, insert_legacy_candidate,
    native_scope_scoped, want_record_index, IndexedRecordOffsets,
};

#[test]
fn sketch_stream_output_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut output = Vec::new();
    let error = ctx
        .append_vec(
            &mut output,
            &mut { vec![17, 18] },
            "f3d sketch stream output",
        )
        .expect_err("collection limit must refuse per-stream output");
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d sketch stream output"));
    assert!(output.is_empty());

    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    ctx.append_vec(
        &mut output,
        &mut { vec![17, 18] },
        "f3d sketch stream output",
    )
    .expect("two output records");
    assert_eq!(output, [17, 18]);
}
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn indexed_header(index: u32) -> [u8; 11] {
    let mut bytes = [0; 11];
    bytes[..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"001");
    bytes[7..].copy_from_slice(&index.to_le_bytes());
    bytes
}

#[test]
fn indexed_record_offsets_charge_each_key_and_offset() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&indexed_header(7));
    bytes.extend_from_slice(&indexed_header(7));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    // Two offsets in byte order, one order slot, one key and two grouped
    // offsets.
    policy.limits.max_collection_items = 6;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let records = IndexedRecordOffsets::build(&ctx, &bytes).unwrap();
    assert_eq!(records.offsets(7), &[0, 11]);
    assert_eq!(records.headers_in(&ctx, 0, 22).unwrap(), &[0, 11]);
    assert!(records.headers_in(&ctx, 1, 11).unwrap().is_empty());
    assert_eq!(records.headers_in(&ctx, 1, 12).unwrap(), &[11]);

    policy.limits.max_collection_items = 5;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert!(matches!(
        IndexedRecordOffsets::build(&ctx, &bytes),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn scoped_record_offsets_retain_nothing() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&indexed_header(7));
    bytes.extend_from_slice(&indexed_header(7));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let (records, _storage) = IndexedRecordOffsets::build_scoped(&ctx, &bytes).unwrap();
    assert_eq!(records.offsets(7), &[0, 11]);
    let mut cache = crate::design::decode::sketch::RecordOffsetCache::new(&ctx).unwrap();
    assert_eq!(
        cache.get(&ctx, "stream", &bytes).unwrap().offsets(7),
        &[0, 11]
    );

    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert!(matches!(
        IndexedRecordOffsets::build_scoped(&ctx, &bytes).map(|_| ()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes
    ));
}

#[test]
fn indexed_record_offsets_charge_distinct_keys() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&indexed_header(7));
    bytes.extend_from_slice(&indexed_header(8));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert!(matches!(
        IndexedRecordOffsets::build(&ctx, &bytes),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn borrowed_record_cache_refuses_map_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = std::collections::HashMap::new();
    assert!(matches!(
        crate::design::decode::sketch::cached_borrowed_record_offsets(
            &ctx, &mut cache, "stream", &[]
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn owned_record_cache_refuses_retained_key() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = std::collections::HashMap::new();
    assert!(matches!(
        crate::design::decode::sketch::cached_owned_record_offsets(
            &ctx, &mut cache, "stream", &[]
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
    ));
}

#[test]
fn scoped_stream_copy_refuses_materialized_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = 5;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        ctx.format_scoped(format_args!("{}", "stream"), "f3d scoped stream identity").map(|(text, reservation)| (reservation, text)),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes
    ));
}

#[test]
fn charged_native_scope_matches_identity_encoding() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for name in ["Design/BulkStream.dat", "a:b#c%d e", "α\u{2003}β"] {
        assert_eq!(
            crate::design::decode::sketch::native_scope_charged(&ctx, name).unwrap(),
            crate::test_support::with_decode_context(|ctx| crate::ids::native_scope(
                ctx,
                name,
                "retain F3D native scope"
            )
            .expect("test F3D native identity"))
        );
    }
}

#[test]
fn native_scope_key_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::decode::sketch::native_scope_charged(&ctx, "a"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
    ));
}

#[test]
fn scoped_native_scope_key_refuses_materialized_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    let expected = crate::test_support::with_decode_context(|ctx| {
        crate::ids::native_scope(ctx, "a:b", "retain F3D native scope")
            .expect("test F3D native identity")
    });
    policy.limits.max_materialized_bytes = u64_from_index(expected.len()) - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        native_scope_scoped(&ctx, "a:b"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes
    ));

    policy.limits.max_materialized_bytes = u64_from_index(expected.len());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (_reservation, actual) = native_scope_scoped(&ctx, "a:b").unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn wanted_record_index_charges_its_stream_and_index_slots() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut wanted = std::collections::HashMap::new();
    assert!(matches!(
        want_record_index(&ctx, &mut wanted, "stream", 7),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d wanted record index"
    ));
}

#[test]
fn record_header_stream_charges_emitted_index_output_and_id() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&indexed_header(7));
    bytes.extend_from_slice(&indexed_header(7));
    let wanted = [7];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    for (items, operation) in [
        (0, "f3d record header emitted index"),
        (1, "f3d record header output"),
    ] {
        policy.limits.max_collection_items = items;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let mut out = Vec::new();
        assert!(matches!(
            decode_headers_for_indices_from_stream(&ctx, "BulkStream.dat", &bytes, &wanted, &mut out),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation
        ));
        assert!(out.is_empty());
    }

    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut out = Vec::new();
    decode_headers_for_indices_from_stream(&ctx, "BulkStream.dat", &bytes, &wanted, &mut out)
        .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(
        out[0].id,
        crate::test_support::with_decode_context(|ctx| crate::ids::native_design_record_header_id(
            ctx,
            "BulkStream.dat",
            0
        )
        .expect("test F3D native identity"))
    );
}

#[test]
fn entity_header_type_indices_refuse_collection_limits() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    let mut modules = std::collections::HashMap::new();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        insert_entity_module(&ctx, &mut modules, "f3d:MetaStream.dat", 7, "MSketch"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d entity module stream"
    ));
    policy.limits.max_collection_items = 1;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        insert_entity_module(&ctx, &mut modules, "f3d:MetaStream.dat", 7, "MSketch"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d entity module index"
    ));

    let mut candidates = std::collections::HashMap::new();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        insert_legacy_candidate(&ctx, &mut candidates, "f3d:", 7),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d legacy sketch stream"
    ));
    policy.limits.max_collection_items = 1;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        insert_legacy_candidate(&ctx, &mut candidates, "f3d:", 7),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d legacy sketch candidate"
    ));
}

#[test]
fn entity_header_output_refuses_collection_limit() {
    use crate::records::entity_header::{DesignEntityHeader, DesignEntityRegistration};
    use crate::records::identity::{DesignEntityId, ReferenceRun};
    let header = DesignEntityHeader {
        id: "f3d:design-entity-header#0".to_owned(),
        byte_offset: 0,
        entity_id: DesignEntityId::from_parts("Sketch", 7),
        class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: DesignEntityRegistration::new(
            None,
            None,
            ReferenceRun::unlocated(Vec::new()),
        )
        .unwrap(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut out = Vec::new();
    assert!(matches!(
        ctx.push_vec(&mut out, header, "f3d entity header output"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d entity header output"
    ));
    assert!(out.is_empty());
}

#[test]
fn indexed_record_search_refuses_work_before_header_scanning() {
    let bytes = indexed_header(7);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::decode::sketch::next_indexed_record_offset(&ctx, &bytes, 0),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "find F3D indexed record header"
                && limit.additional == 1
    ));
}

#[test]
fn indexed_record_frames_preserve_work_refusal() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&indexed_header(7));
    bytes.extend_from_slice(&indexed_header(7));
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(records.frames(&ctx, 7),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "scan F3D indexed record frames"
                && limit.additional == 2));
}

#[test]
fn indexed_record_groups_preserve_work_refusal() {
    let records = crate::design::test_support::indexed_record_offsets_for_test(&indexed_header(7));
    // The traversal visits one record index.
    let expected = 1;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(records.records(&ctx),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "scan F3D indexed record groups"
                && limit.additional == expected));
}

#[test]
fn native_scope_encoder_preserves_each_iteration_work_refusal() {
    let name = "A:B";
    // Measuring reads three bytes; writing appends the scheme prefix, reads
    // the name and writes `A`, then the three characters of the escape.
    for (operation, skip, additional) in [
        ("measure F3D native stream key", 0, 3),
        ("write F3D native stream key", 0, 4),
        ("write F3D native stream key", 1, 3),
        ("write F3D native stream key", 2, 1),
        ("write F3D native stream key", 3, 1),
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            skip,
            |ctx| native_scope_scoped(ctx, name).map(|(_, text)| text),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == operation
                && limit.additional == additional));
    }
    let (_, text) =
        native_scope_scoped(&cadmpeg_test_support::service_decode_context(), name).unwrap();
    assert_eq!(text, "f3d:A%3AB");
}
