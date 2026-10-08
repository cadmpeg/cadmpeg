// SPDX-License-Identifier: Apache-2.0
//! Native assembly placement admission and recovery tests.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;

use crate::assembly::AssemblyPlacement;
use crate::compact_matrix::CompactMatrix;
use crate::container::InventorContainer;
use crate::decode::{admit_assembly_placement, decode_container};
use crate::native::{AssemblyPlacementRecord, AssemblyPlacementRecordWire};
use crate::record_issue::{RecordIssueFamily, RecordIssueWire};
use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
use crate::test_support::test_fixtures::primary_envelope_fixture;

#[test]
fn assembly_placement_native_record_refuses_id_and_digest_before_creation() {
    let suffix = [1_u8];
    let placement = || AssemblyPlacement {
        segment_token: "segment".into(),
        record_ordinal: 1,
        header_id: 0,
        owner_reference: 0,
        attribute_reference: 0,
        state: 0,
        transform_prefix: false,
        transform: CompactMatrix::try_new(
            0,
            0,
            |_| Ok(cadmpeg_ir::scalar::FiniteReal::ZERO),
        )
        .expect("finite matrix"),
        branch: 0,
        graphics_state: 0,
        occurrence_id: 0,
        graphics_index: 0,
        object_reference: 0,
        suffix: View::over_retained(&suffix),
    };
    let arena = DecodeArena::new();
    let id_len = "inventor:assembly:placement#segment-1".len();
    let token_len = "segment".len();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(id_len - 1).expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        AssemblyPlacementRecord::from_placement(&ctx, placement()),
        Err(failure)
            if matches!(failure.error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor assembly placement id")
    ));
    // Preserve the former cap: without the redundant token copy, digest text
    // now fits under this same limit.
    policy.limits.max_retained_bytes =
        u64::try_from(id_len + token_len + 63).expect("digest budget fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let record = AssemblyPlacementRecord::from_placement(&ctx, placement())
        .expect("former digest refusal cap now admits the moved-token conversion");
    assert_eq!(record.id, "inventor:assembly:placement#segment-1");

    policy.limits.max_retained_bytes = u64::try_from(id_len + 63).expect("digest limit fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        AssemblyPlacementRecord::from_placement(&ctx, placement()),
        Err(failure)
            if matches!(failure.error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor assembly placement suffix digest"
                    && limit.used == cadmpeg_core::decode::u64_from_index(id_len)
                    && limit.additional == 64)
    ));

    policy.limits.max_retained_bytes = u64::try_from(id_len + 64).expect("exact digest fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("exact context");
    AssemblyPlacementRecord::from_placement(&ctx, placement())
        .expect("exact source digest storage fits");

    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("zero-materialized service context");
    AssemblyPlacementRecord::from_placement(&ctx, placement()).expect("admitted placement");
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    policy.limits.max_materialized_bytes = 0;
    let (limited, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let conversion = AssemblyPlacementRecord::from_placement(&limited, placement());
    let refusal = match admit_assembly_placement(&limited, conversion, &mut Vec::new()) {
        Err(CodecError::ResourceLimit(limit)) => limit,
        other => panic!("entity admission should refuse: {other:?}"),
    };
    assert_eq!(refusal.dimension, ResourceDimension::Entities);
    assert_eq!(
        refusal.operation,
        "admit Inventor native assembly placement"
    );
    assert!(matches!(
        limited.finish_session(),
        Err(CodecError::ResourceLimit(limit)) if limit == refusal
    ));
    let conversion = AssemblyPlacementRecord::from_placement(&ctx, placement());
    assert!(admit_assembly_placement(&ctx, conversion, &mut Vec::new())
        .expect("service placement admission")
        .is_some());
    ctx.finish_session()
        .expect("placement conversion uses no materialized token copy");
}

#[test]
fn nonfinite_assembly_placement_transform_is_rejected_at_parse() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("container fixture");
    let mut container = InventorContainer::open(&ctx, root).expect("container fixture");
    let mut payload = vec![0; 15];
    payload.extend_from_slice(&0_u16.to_le_bytes());
    payload.extend_from_slice(&0_u16.to_le_bytes());
    payload.extend_from_slice(&f64::INFINITY.to_le_bytes());
    let segment = &mut container.rse.segments[0];
    segment.kind = SegmentKind::AmGraphics;
    let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
        panic!("framed fixture");
    };
    let RecordFrameState::Framed(table) = &mut bulk.records else {
        panic!("record fixture");
    };
    table.records[0].type_id = [
        0xa2, 0x63, 0x71, 0xca, 0xd0, 0x11, 0xb2, 0xd3, 0x00, 0x08, 0xbf, 0xbb, 0x21, 0xed, 0xdc,
        0x09,
    ];
    table.records[0].payload = View::over_retained(&payload);
    let decoded =
        decode_container(&ctx, &container).expect("invalid placement must not fail decode");
    let namespace = decoded
        .ir
        .native
        .namespace("inventor")
        .expect("native namespace");
    let issues = namespace
        .arena_as::<RecordIssueWire>("assembly_record_issues")
        .expect("assembly issue wires")
        .into_iter()
        .map(|wire| wire.into_record(&ctx))
        .collect::<Result<Vec<_>, _>>()
        .expect("assembly issues");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].family, RecordIssueFamily::Assembly);
    assert!(issues[0].detail.contains("finite"), "{:?}", issues[0]);
    assert!(namespace
        .arena_as::<serde_json::Value>("assembly_placements")
        .expect("placements")
        .is_empty());
    assert!(super::super::validation_findings(&decoded.ir)
        .iter()
        .any(|finding| finding.message.contains(&issues[0].detail)));
}

#[test]
fn rejected_placement_digest_records_its_source_and_keeps_later_placements() {
    let wire = serde_json::json!({
        "id": "inventor:assembly:placement#segment-1", "segment_token": "segment", "record_ordinal": 1,
        "header_id": 0, "owner_reference": 0, "attribute_reference": 0, "state": 0,
        "transform_prefix": false, "transform_encoding": [0, 0],
        "transform": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
        "branch": 0, "graphics_state": 0, "occurrence_id": 1, "graphics_index": 0,
        "object_reference": 0, "suffix_len": 48, "suffix_sha256": "invalid"
    });
    let mut issues = Vec::new();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let bad: AssemblyPlacementRecordWire =
        serde_json::from_value(wire.clone()).expect("wire fixture");
    assert!(admit_assembly_placement(&ctx, bad.into_record(), &mut issues)
        .expect("service admission")
        .is_none());
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].segment_token.as_str(), "segment");
    assert_eq!(issues[0].record_ordinal, 1);
    assert!(issues[0].detail.contains("suffix_sha256"));
    let mut wire = wire;
    wire["suffix_sha256"] = serde_json::json!("0".repeat(64));
    let good: AssemblyPlacementRecordWire = serde_json::from_value(wire).expect("wire fixture");
    assert!(admit_assembly_placement(&ctx, good.into_record(), &mut issues)
        .expect("service admission")
        .is_some());
    assert_eq!(issues.len(), 1);
}

#[test]
fn placement_conversion_issue_moves_raw_wire_token_with_legacy_cap() {
    let fixture = serde_json::json!({
        "id": "inventor:assembly:placement#segment-1", "segment_token": "segment", "record_ordinal": 1,
        "header_id": 0, "owner_reference": 0, "attribute_reference": 0, "state": 0,
        "transform_prefix": false, "transform_encoding": [0, 0],
        "transform": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
        "branch": 0, "graphics_state": 0, "occurrence_id": 1, "graphics_index": 0,
        "object_reference": 0, "suffix_len": 0, "suffix_sha256": "0".repeat(64)
    });
    let wire: AssemblyPlacementRecordWire =
        serde_json::from_value(fixture.clone()).expect("placement wire");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let issue_detail = "suffix_len must not be zero";
    // Keep the old raw-wire budget formula while checking the moved-token path.
    let token_len = wire.segment_token.len();
    let retained_needed = 4 * std::mem::size_of::<crate::record_issue::RecordIssue>()
        + token_len
        + issue_detail.len();
    policy.limits.max_retained_bytes =
        u64::try_from(retained_needed - 1).expect("issue budget fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let mut issues = Vec::new();
    assert!(admit_assembly_placement(&ctx, wire.into_record(), &mut issues)
        .expect("legacy cap admits transferred token")
        .is_none());
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].family, RecordIssueFamily::Assembly);
    assert_eq!(issues[0].segment_token.as_str(), "segment");
    assert_eq!(issues[0].record_ordinal, 1);
    assert_eq!(issues[0].detail, issue_detail);
    ctx.finish_session()
        .expect("legacy one-under cap covers issue storage after transfer");

    policy.limits.max_retained_bytes =
        u64::try_from(retained_needed).expect("full issue budget fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("full context");
    let wire: AssemblyPlacementRecordWire =
        serde_json::from_value(fixture).expect("placement wire");
    let mut issues = Vec::new();
    assert!(admit_assembly_placement(&ctx, wire.into_record(), &mut issues)
        .expect("full admission")
        .is_none());
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].family, RecordIssueFamily::Assembly);
    assert_eq!(issues[0].segment_token.as_str(), "segment");
    assert_eq!(issues[0].record_ordinal, 1);
    assert_eq!(issues[0].detail, issue_detail);
    ctx.finish_session()
        .expect("legacy exact cap covers issue storage after transfer");
}

#[test]
fn uppercase_placement_digest_moves_raw_wire_token_with_legacy_cap() {
    let fixture = serde_json::json!({
        "id": "inventor:assembly:placement#segment-1", "segment_token": "segment", "record_ordinal": 1,
        "header_id": 0, "owner_reference": 0, "attribute_reference": 0, "state": 0,
        "transform_prefix": false, "transform_encoding": [0, 0],
        "transform": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
        "branch": 0, "graphics_state": 0, "occurrence_id": 1, "graphics_index": 0,
        "object_reference": 0, "suffix_len": 48, "suffix_sha256": "A".repeat(64)
    });
    let wire: AssemblyPlacementRecordWire =
        serde_json::from_value(fixture.clone()).expect("placement wire");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let issue_detail =
        "suffix_sha256: sha256 digest must contain exactly 64 lowercase hexadecimal characters";
    // Keep the old raw-wire budget formula while checking the moved-token path.
    let token_len = wire.segment_token.len();
    let retained_needed = 4 * std::mem::size_of::<crate::record_issue::RecordIssue>()
        + token_len
        + issue_detail.len();
    policy.limits.max_retained_bytes =
        u64::try_from(retained_needed - 1).expect("issue budget fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let mut issues = Vec::new();
    assert!(admit_assembly_placement(&ctx, wire.into_record(), &mut issues)
        .expect("legacy cap admits transferred token")
        .is_none());
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].family, RecordIssueFamily::Assembly);
    assert_eq!(issues[0].segment_token.as_str(), "segment");
    assert_eq!(issues[0].record_ordinal, 1);
    assert_eq!(issues[0].detail, issue_detail);
    ctx.finish_session()
        .expect("legacy one-under cap covers issue storage after transfer");

    policy.limits.max_retained_bytes =
        u64::try_from(retained_needed).expect("full issue budget fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("full context");
    let wire: AssemblyPlacementRecordWire =
        serde_json::from_value(fixture).expect("placement wire");
    let mut issues = Vec::new();
    assert!(admit_assembly_placement(&ctx, wire.into_record(), &mut issues)
        .expect("full admission")
        .is_none());
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].family, RecordIssueFamily::Assembly);
    assert_eq!(issues[0].segment_token.as_str(), "segment");
    assert_eq!(issues[0].record_ordinal, 1);
    assert_eq!(issues[0].detail, issue_detail);
    ctx.finish_session()
        .expect("legacy exact cap covers issue storage after transfer");
}

#[test]
fn placement_conversion_issue_transfers_admitted_token_without_materialization() {
    let suffix: [u8; 0] = [];
    let placement = || AssemblyPlacement {
        segment_token: "segment".into(),
        record_ordinal: 1,
        header_id: 0,
        owner_reference: 0,
        attribute_reference: 0,
        state: 0,
        transform_prefix: false,
        transform: CompactMatrix::try_new(0, 0, |_| {
            Ok(cadmpeg_ir::scalar::FiniteReal::ZERO)
        })
        .expect("finite matrix"),
        branch: 0,
        graphics_state: 0,
        occurrence_id: 1,
        graphics_index: 0,
        object_reference: 0,
        suffix: View::over_retained(&suffix),
    };
    let issue_detail = "suffix_len must not be zero";
    let id_len = "inventor:assembly:placement#segment-1".len();
    let source_token_len = "segment".len();
    // Preserve the former source-conversion cost formula and cap. The owned
    // input token is now admitted once by inventory and moved into the issue.
    let wire_retained = id_len + source_token_len + 64;
    let issue_vector_storage = 4 * std::mem::size_of::<crate::record_issue::RecordIssue>();
    // Empty-vector amortized growth reserves four RecordIssue slots; detail is copied next.
    let issue_storage = issue_vector_storage + issue_detail.len();
    let retained_needed = wire_retained + issue_storage;
    let source_issue_storage = source_token_len + issue_storage;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes =
        u64::try_from(retained_needed - 1).expect("issue budget fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let mut input = placement();
    input.segment_token = ctx
        .copy_retained_text(
            &input.segment_token,
            "retain Inventor assembly placement token",
        )
        .expect("inventory token fits before conversion");
    let conversion = AssemblyPlacementRecord::from_placement(&ctx, input);
    let mut issues = Vec::new();
    assert!(admit_assembly_placement(&ctx, conversion, &mut issues)
        .expect("former one-under cap admits issue after output copies are skipped")
        .is_none());
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].family, RecordIssueFamily::Assembly);
    assert_eq!(issues[0].segment_token.as_str(), "segment");
    assert_eq!(issues[0].record_ordinal, 1);
    assert_eq!(issues[0].detail, issue_detail);
    ctx.finish_session()
        .expect("former one-under cap now covers the moved-token issue");

    policy.limits.max_retained_bytes =
        u64::try_from(retained_needed).expect("exact issue budget fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("exact context");
    let mut input = placement();
    input.segment_token = ctx
        .copy_retained_text(
            &input.segment_token,
            "retain Inventor assembly placement token",
        )
        .expect("inventory token fits before conversion");
    let conversion = AssemblyPlacementRecord::from_placement(&ctx, input);
    let mut issues = Vec::new();
    assert!(admit_assembly_placement(&ctx, conversion, &mut issues)
        .expect("former exact cap admits issue storage with no materialization")
        .is_none());
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].family, RecordIssueFamily::Assembly);
    assert_eq!(issues[0].segment_token.as_str(), "segment");
    assert_eq!(issues[0].record_ordinal, 1);
    assert_eq!(issues[0].detail, issue_detail);
    ctx.finish_session()
        .expect("former exact cap still admits the transferred token and issue");

    policy.limits.max_retained_bytes =
        u64::try_from(source_issue_storage - 1).expect("one-under source issue budget fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("one-under context");
    let mut input = placement();
    input.segment_token = ctx
        .copy_retained_text(
            &input.segment_token,
            "retain Inventor assembly placement token",
        )
        .expect("source token fits before the issue boundary");
    let conversion = AssemblyPlacementRecord::from_placement(&ctx, input);
    let mut issues = Vec::new();
    let first_refusal = match admit_assembly_placement(&ctx, conversion, &mut issues) {
        Err(CodecError::ResourceLimit(limit)) => limit,
        other => panic!("one-under issue detail must refuse: {other:?}"),
    };
    assert_eq!(first_refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(
        first_refusal.operation,
        "retain Inventor placement issue detail"
    );
    assert_eq!(
        first_refusal.used,
        cadmpeg_core::decode::u64_from_index(source_token_len + issue_vector_storage)
    );
    assert_eq!(
        first_refusal.additional,
        cadmpeg_core::decode::u64_from_index(issue_detail.len())
    );
    assert!(issues.is_empty());
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(limit)) if limit == first_refusal
    ));

    policy.limits.max_retained_bytes =
        u64::try_from(source_issue_storage).expect("exact source issue budget fits");
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("source exact context");
    let mut input = placement();
    input.segment_token = ctx
        .copy_retained_text(
            &input.segment_token,
            "retain Inventor assembly placement token",
        )
        .expect("source token fits before exact issue storage");
    let conversion = AssemblyPlacementRecord::from_placement(&ctx, input);
    let mut issues = Vec::new();
    assert!(admit_assembly_placement(&ctx, conversion, &mut issues)
        .expect("exact source issue storage admitted with no materialization")
        .is_none());
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].family, RecordIssueFamily::Assembly);
    assert_eq!(issues[0].segment_token.as_str(), "segment");
    assert_eq!(issues[0].record_ordinal, 1);
    assert_eq!(issues[0].detail, issue_detail);
    ctx.finish_session()
        .expect("exact source storage admits its token and issue");
}
