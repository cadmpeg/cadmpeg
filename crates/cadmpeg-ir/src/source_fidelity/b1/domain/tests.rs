// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::source_fidelity::b1::{partition_root_stream, partition_stream};
use crate::{CadIr, RetainedBytes, RetainedSourceRecord, SourceFidelity};
use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodePolicy};
use std::collections::BTreeMap;

#[test]
fn bounded_root_partition_keeps_original_coordinates_without_claiming_enclosing_bytes() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"beforeABCDafter", &arena, &DecodePolicy::service())
            .unwrap();
    let root = source.child(6, 10).unwrap();
    let domain = SourceRootDomain::capture(&ctx, root).unwrap();
    let partition =
        partition_root_stream(&ctx, domain, &BTreeMap::new(), &[6..7], &[], &[]).unwrap();
    assert_eq!(partition.byte_len(), 15);
    assert_eq!(partition.domain(), 6..10);
    assert!(partition.reads().is_empty());
    assert_eq!(partition.exempt(), [6..7]);
    assert_eq!(partition.b1(), [7..10]);
    let mut fidelity = SourceFidelity::default();
    fidelity
        .insert_retained_record(
            "test:source:image#input-root".try_into().unwrap(),
            RetainedSourceRecord::from_bytes(
                "file",
                6,
                RetainedBytes::Inline {
                    data: b"ABCD".to_vec(),
                },
            )
            .unwrap(),
        )
        .unwrap();
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    let ir = CadIr::empty();
    let index = super::super::classification::ClassificationIndex::new(&ctx, &ir).unwrap();
    let mut losses = Vec::new();
    fidelity
        .finalize_b1(&ctx, &index, "fcstd", &[partition.clone()], &mut losses)
        .unwrap();
    fidelity
        .check_b1_closure(&ctx, &index, "fcstd", &[partition], &losses)
        .unwrap();
    assert_eq!(fidelity.b1_spans()["file"].spans()[0].range(), 7..10);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].provenance.as_ref().unwrap().offset, 7);
    assert_eq!(losses[0].provenance.as_ref().unwrap().byte_len(), Some(3));
}

#[test]
fn root_domains_reject_foreign_sessions_partial_members_and_new_proofs_after_seal() {
    let first_arena = DecodeArena::new();
    let second_arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"source", &first_arena, &DecodePolicy::service()).unwrap();
    let (other, foreign) =
        DecodeContext::from_root_bytes(b"source", &second_arena, &DecodePolicy::service()).unwrap();
    assert!(SourceRootDomain::capture(&ctx, foreign).is_err());
    let (batched, _) = ctx
        .record_batch(|| SourceRootDomain::capture(&ctx, root))
        .unwrap();
    assert!(partition_root_stream(&ctx, batched, &BTreeMap::new(), &[], &[], &[]).is_err());
    let domain = SourceRootDomain::capture(&ctx, root.child(1, 5).unwrap()).unwrap();
    assert!(partition_root_stream(&other, domain, &BTreeMap::new(), &[], &[], &[]).is_err());
    let member = ctx
        .register_named_slice(root, ByteRange { start: 1, end: 5 }, "member")
        .unwrap();
    assert!(SourceRootDomain::capture(&ctx, member.child(1, 3).unwrap()).is_err());
    assert!(SourceRootDomain::capture(&ctx, member).is_ok());
    ctx.take_record_captures().unwrap();
    assert!(SourceRootDomain::capture(&ctx, root).is_err());
}

#[test]
fn bounded_root_domain_rejects_out_of_domain_semantics_and_keeps_full_stream_behavior() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"source", &arena, &DecodePolicy::service()).unwrap();
    let domain = SourceRootDomain::capture(&ctx, root.child(1, 5).unwrap()).unwrap();
    assert!(partition_root_stream(&ctx, domain, &BTreeMap::new(), &[0..1], &[], &[]).is_err());
    let root_domain = SourceRootDomain::capture(&ctx, root).unwrap();
    let bounded =
        partition_root_stream(&ctx, root_domain, &BTreeMap::new(), &[0..1], &[], &[]).unwrap();
    let complete = partition_stream(&ctx, "file", 6, &BTreeMap::new(), &[0..1], &[], &[]).unwrap();
    assert_eq!(bounded.domain(), complete.domain());
    assert_eq!(bounded.reads(), complete.reads());
    assert_eq!(bounded.exempt(), complete.exempt());
    assert_eq!(bounded.b1(), complete.b1());
}
