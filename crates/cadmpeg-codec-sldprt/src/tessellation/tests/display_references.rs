// SPDX-License-Identifier: Apache-2.0
use super::{
    decoded_references, framed_surface_reference, persistent_identity,
    persistent_surface_references, ByteRange, DisplayFace, FeatureSourceId, Mesh,
    PersistentSurfaceReference,
};

fn reference_limit_error(
    payload: &[u8],
    policy: &cadmpeg_core::decode::DecodePolicy,
) -> cadmpeg_core::CodecError {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(payload, &arena, policy)
        .expect("root");
    persistent_surface_references(
        &ctx,
        payload,
        ByteRange::new(0, payload.len()).expect("ordered range"),
    )
    .expect_err("reference allocation exceeds the limit")
}

#[test]
fn display_reference_scan_refuses_before_prefix_search() {
    let payload = framed_surface_reference("moContent3IntSurfIdRep_c,300,4,-1,0,");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    assert!(matches!(reference_limit_error(&payload, &policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "scan display-list reference names"));
}

#[test]
fn display_reference_units_need_no_collection_slots() {
    let payload = framed_surface_reference("moContent3IntSurfIdRep_c,300,4,-1,0,");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 6;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let references = persistent_surface_references(
        &ctx,
        &payload,
        ByteRange::new(0, payload.len()).expect("range"),
    )
    .expect("only field and result slots");
    assert_eq!(references.len(), 1);
}

#[test]
fn display_reference_text_refuses_materialized_limit_before_allocation() {
    let payload = framed_surface_reference("moContent3IntSurfIdRep_c,300,4,-1,0,");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::from(payload[3]) - 1;
    assert!(matches!(reference_limit_error(&payload, &policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && limit.operation == "decode display-list reference text"));
}

fn reference_collection_refusal(operation: &'static str) {
    let payload = framed_surface_reference("moContent3IntSurfIdRep_c,300,4,-1,0,");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy)?;
            persistent_surface_references(
                &ctx,
                &payload,
                ByteRange::new(0, payload.len()).expect("ordered range"),
            )
        },
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
}

#[test]
fn display_reference_numeric_fields_refuse_collection_limit_before_allocation() {
    reference_collection_refusal("decode display-list reference fields");
}

#[test]
fn display_references_refuse_collection_limit_before_insertion() {
    reference_collection_refusal("collect display-list references");
}

#[test]
fn overlapping_display_face_tables_narrow_to_an_empty_metadata_range() {
    // Display-face metadata is narrowed to where the following table starts. Two
    // overlapping tables put that end below the metadata start; the range then
    // collapses at its own start instead of inverting and silently reading nothing.
    assert!(ByteRange::new(64, 32).is_none());
    let metadata = ByteRange::new(64, 128).expect("ordered range");
    let overlapped = metadata.truncated(32);
    assert_eq!((overlapped.start(), overlapped.end()), (64, 64));
    let mut payload = vec![0; 192];
    let reference = framed_surface_reference("moPlaneSurfIdRep_c,7,3,");
    payload[64..64 + reference.len()].copy_from_slice(&reference);
    assert!(decoded_references(&payload, overlapped).is_empty());
    let narrowed = metadata.truncated(120);
    assert_eq!((narrowed.start(), narrowed.end()), (64, 120));
    assert_eq!(
        decoded_references(&payload, narrowed).len(),
        decoded_references(&payload, metadata).len()
    );
}

#[test]
fn persistent_surface_reference_decodes_signed_tail() {
    let payload = framed_surface_reference("moContent3IntSurfIdRep_c,300,4,-1,0,");
    let references = decoded_references(
        &payload,
        ByteRange::new(0, payload.len()).expect("ordered range"),
    );
    assert_eq!(
        references,
        vec![PersistentSurfaceReference::Complete(persistent_identity(
            300,
            4,
            &[u32::MAX, 0],
        ))]
    );
}

#[test]
fn opaque_surface_suffix_remains_source_only() {
    let payload = framed_surface_reference("moFromSktEntSurfIdRep_c,7,3,opaque");
    let references = decoded_references(
        &payload,
        ByteRange::new(0, payload.len()).expect("ordered range"),
    );
    assert_eq!(
        references,
        vec![PersistentSurfaceReference::SourceOnly {
            feature_source_id: 7_u32.try_into().unwrap(),
            local_surface_id: 3,
        }]
    );
    let face = DisplayFace {
        mesh: Mesh::default(),
        table: ByteRange::new(0, 1).expect("ordered range"),
        metadata: ByteRange::new(1, 2).expect("ordered range"),
        surface_references: references,
    };
    assert_eq!(
        face.feature_source_id(&cadmpeg_test_support::service_decode_context())
            .unwrap()
            .map(FeatureSourceId::value),
        Some(7)
    );
    assert_eq!(
        face.persistent_surface_identity(&cadmpeg_test_support::service_decode_context())
            .unwrap(),
        None
    );
}

#[test]
fn persistent_surface_source_sentinels_are_absent() {
    for source in [0, u32::MAX] {
        let payload = framed_surface_reference(&format!("moPlaneSurfIdRep_c,{source},3,"));
        let references = decoded_references(
            &payload,
            ByteRange::new(0, payload.len()).expect("ordered range"),
        );
        assert!(references.is_empty());
    }
}

// One numeric value reserves the four-u32 minimum capacity. Five results
// grow the output from four to eight slots, with four old slots in scratch.
const OPAQUE_AFTER_NUMBER: &str = "moPlaneSurfIdRep_c,7,3,1,opaque";
const NUMERIC_TAIL: &str = "moPlaneSurfIdRep_c,7,3,-1,0,";

fn five_opaque_references() -> Vec<u8> {
    let mut payload = Vec::new();
    for _ in 0..5 {
        payload.extend(framed_surface_reference(OPAQUE_AFTER_NUMBER));
    }
    payload
}

fn reference_output_slots() -> u64 {
    u64::try_from(4 * std::mem::size_of::<PersistentSurfaceReference>())
        .expect("four reference slots")
}

#[test]
fn display_reference_rejected_tail_releases_before_result_reallocation() {
    let payload = five_opaque_references();
    let old_output = reference_output_slots();
    let parse_peak = u64::try_from(OPAQUE_AFTER_NUMBER.len() + 4 * std::mem::size_of::<u32>())
        .expect("ASCII text and four tail slots");
    let scratch = parse_peak.max(old_output);
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = scratch;
    policy.limits.max_retained_bytes = 2 * old_output;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("root context");
    let references = persistent_surface_references(&ctx, &payload,
        ByteRange::new(0, payload.len()).expect("ordered range"))
        .expect("discarded numeric tail and text do not overlap result growth");
    assert_eq!(references, vec![PersistentSurfaceReference::SourceOnly {
        feature_source_id: 7_u32.try_into().expect("source id"),
        local_surface_id: 3,
    }; 5]);
    drop(ctx.reserve_scoped(scratch, "test released display parsing workspace")
        .expect("full scratch capacity while references survive"));
    ctx.finish_session().expect("source-only recovery is accepted");
}

#[test]
fn display_reference_reallocation_refuses_the_exact_old_output_backing() {
    let payload = five_opaque_references();
    let old_output = reference_output_slots();
    let parse_peak = u64::try_from(OPAQUE_AFTER_NUMBER.len() + 4 * std::mem::size_of::<u32>())
        .expect("ASCII text and four tail slots");
    assert!(parse_peak < old_output, "the fixture reaches the larger output overlap");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = old_output - 1;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("root context");
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = persistent_surface_references(
        &ctx, &payload, ByteRange::new(0, payload.len()).expect("ordered range")) else {
        panic!("old result backing exceeds scratch by one byte");
    };
    assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect display-list references");
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, old_output);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(actual))
        if actual == limit));
}

#[test]
fn display_reference_complete_tail_retains_only_the_identity_backing() {
    let payload = framed_surface_reference(NUMERIC_TAIL);
    let tail_bytes = u64::try_from(4 * std::mem::size_of::<u32>()).expect("tail backing");
    let retained = reference_output_slots() + tail_bytes;
    let scratch = u64::try_from(NUMERIC_TAIL.len()).expect("ASCII text") + tail_bytes;
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = scratch;
    policy.limits.max_retained_bytes = retained;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("root context");
    let references = persistent_surface_references(&ctx, &payload,
        ByteRange::new(0, payload.len()).expect("ordered range"))
        .expect("only live output and numeric tail are retained");
    assert_eq!(references, vec![PersistentSurfaceReference::Complete(
        persistent_identity(7, 3, &[u32::MAX, 0]))]);
    drop(ctx.reserve_scoped(scratch, "test released complete reference text")
        .expect("full scratch capacity while the identity survives"));
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
        ctx.charge_retained(1, "test exact reference retained boundary") else {
        panic!("output retained bound is exact");
    };
    assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::RetainedBytes);
    assert_eq!(limit.used, retained);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(actual))
        if actual == limit));
}

#[test]
fn display_reference_tail_publication_preserves_the_original_retained_refusal() {
    let payload = framed_surface_reference(NUMERIC_TAIL);
    let output_bytes = reference_output_slots();
    let tail_bytes = u64::try_from(4 * std::mem::size_of::<u32>()).expect("tail backing");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = output_bytes + tail_bytes - 1;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("root context");
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = persistent_surface_references(
        &ctx, &payload, ByteRange::new(0, payload.len()).expect("ordered range")) else {
        panic!("actual tail publication exceeds retained bound by one byte");
    };
    assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "decode display-list reference fields");
    assert_eq!(limit.used, output_bytes);
    assert_eq!(limit.additional, tail_bytes);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(actual))
        if actual == limit));
}
