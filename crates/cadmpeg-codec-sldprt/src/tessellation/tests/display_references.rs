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
fn display_reference_units_refuse_collection_limit_before_allocation() {
    let payload = framed_surface_reference("moContent3IntSurfIdRep_c,300,4,-1,0,");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = u64::from(payload[3]) - 1;
    assert!(matches!(reference_limit_error(&payload, &policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "decode display-list reference units"));
}

#[test]
fn display_reference_text_refuses_materialized_limit_before_allocation() {
    let payload = framed_surface_reference("moContent3IntSurfIdRep_c,300,4,-1,0,");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::from(payload[3]) * 3 - 1;
    assert!(matches!(reference_limit_error(&payload, &policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && limit.operation == "decode display-list reference text"));
}

fn reference_collection_refusal(extra: u64, operation: &'static str) {
    let payload = framed_surface_reference("moContent3IntSurfIdRep_c,300,4,-1,0,");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = u64::from(payload[3]) + extra;
    assert!(matches!(reference_limit_error(&payload, &policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == operation));
}

#[test]
fn display_reference_fields_refuse_collection_limit_before_scanning() {
    reference_collection_refusal(0, "scan display-list reference fields");
}

#[test]
fn display_reference_numeric_fields_refuse_collection_limit_before_allocation() {
    reference_collection_refusal(3, "decode display-list reference fields");
}

#[test]
fn display_references_refuse_collection_limit_before_insertion() {
    reference_collection_refusal(5, "collect display-list references");
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
        face.feature_source_id().map(FeatureSourceId::value),
        Some(7)
    );
    assert_eq!(face.persistent_surface_identity(), None);
}
