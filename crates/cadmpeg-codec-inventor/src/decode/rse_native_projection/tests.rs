// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::container::InventorContainer;
use crate::rse::{SegmentBulkState, SegmentDescriptor, SegmentKind, SegmentMetaState};
use crate::test_support::test_fixtures::{primary_envelope_fixture_with, EnvelopeDeclarations};

#[test]
fn segment_projection_refuses_only_the_next_source_step() {
    let bytes = primary_envelope_fixture_with(EnvelopeDeclarations::default());
    for count in [1_usize, 512] {
        let arena = DecodeArena::new();
        let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("projection fixture context");
        let mut container = InventorContainer::open(&setup, root).expect("projection fixture");
        let pair = container.rse.segments[0].pair.clone();
        container.rse.segments = (0..count).map(|_| SegmentDescriptor {
            pair: pair.clone(), registry: None, kind: SegmentKind::Unresolved,
            identity_issues: Vec::new(),
            meta: SegmentMetaState::Malformed { declared: None, detail: "metadata".into() },
            bulk: SegmentBulkState::Malformed("bulk".into()),
        }).collect();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("limited projection context");
        let error = super::project(&ctx, &container).err().expect("first segment step refuses");
        assert!(matches!(&error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "visit Inventor decode/rse_native_projection items"
                && limit.used == 0 && limit.additional == 1));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit))
            if matches!(&error, CodecError::ResourceLimit(original) if original == &limit)));
    }
}

#[test]
fn first_identity_issue_refusal_does_not_prepay_the_issue_tail() {
    let bytes = primary_envelope_fixture_with(EnvelopeDeclarations::default());
    for count in [1_usize, 512] {
        let arena = DecodeArena::new();
        let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("issue fixture context");
        let mut container = InventorContainer::open(&setup, root).expect("issue fixture");
        container.rse.segments.truncate(1);
        container.rse.segments[0].identity_issues = (0..count).map(|_| "identity".into()).collect();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 2;
        policy.limits.max_entities = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("limited issue context");
        let error = super::project(&ctx, &container).err().expect("first issue entity refuses");
        assert!(matches!(&error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Inventor native structural records"
                && limit.used == 0 && limit.additional == 1));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit))
            if matches!(&error, CodecError::ResourceLimit(original) if original == &limit)));
    }
}

#[test]
fn empty_projection_admits_three_source_end_probes() {
    let bytes = primary_envelope_fixture_with(EnvelopeDeclarations::default());
    let arena = DecodeArena::new();
    let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("empty projection fixture context");
    let mut container = InventorContainer::open(&setup, root).expect("empty projection fixture");
    container.rse.segments.clear();
    container.rse.unpaired_metadata.clear();
    container.rse.unpaired_bulk.clear();
    for allowance in [2_u64, 3] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowance;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty projection context");
        let result = super::project(&ctx, &container);
        if allowance == 2 {
            let error = result.err().expect("last source end probe refuses");
            assert!(matches!(&error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "visit Inventor decode/rse_native_projection items"
                    && limit.used == 2 && limit.additional == 1));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit))
                if matches!(&error, CodecError::ResourceLimit(original) if original == &limit)));
        } else {
            let projection = result.expect("all three source end probes fit");
            assert!(projection.segment_pairs.is_empty());
            assert!(projection.unpaired_segments.is_empty());
            ctx.finish_session().expect("empty projection retains no bytes");
        }
    }
}
