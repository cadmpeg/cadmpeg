// SPDX-License-Identifier: Apache-2.0

use super::{AnnotationBuilder, Exactness, StreamHandle};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

const FIRST: &str = "test:model:point#first";
const SECOND: &str = "test:model:point#second";

fn fixture() -> AnnotationBuilder {
    let mut builder = AnnotationBuilder::new();
    let stream = StreamHandle::new(crate::stream_name!("source"));
    builder.note(FIRST, &stream, 7, Some("first"));
    builder.note(SECOND, &stream, 9, Some("second"));
    builder.exactness(FIRST, Exactness::Derived);
    builder.exactness(SECOND, Exactness::Inferred);
    builder
}

#[test]
fn annotation_removal_uses_no_owned_or_temporary_storage() {
    let mut builder = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    builder.remove_entity(FIRST);
    assert!(!builder.annotations().provenance.contains_key(FIRST));
    assert!(!builder.annotations().exactness().contains_key(FIRST));
    assert_eq!(builder.annotations().provenance[SECOND].offset, 9);
    assert_eq!(
        builder.annotations().provenance[SECOND].tag.as_deref(),
        Some("second")
    );
    assert_eq!(
        builder.annotations().exactness()[SECOND].entity(),
        Exactness::Inferred
    );
    builder.remove_entity("absent");
    ctx.finish_session().unwrap();
}

#[test]
fn retention_keeps_selected_entries() {
    for provenance in [false, true] {
        let mut builder = fixture();
        let before = builder.annotations().clone();
        if provenance {
            builder
                .state
                .annotations
                .retain_provenance(|id| id == SECOND);
            assert_eq!(builder.annotations().provenance.len(), 1);
            assert_eq!(
                builder.annotations().provenance[SECOND],
                before.provenance[SECOND]
            );
            assert_eq!(builder.annotations().exactness(), before.exactness());
        } else {
            builder.retain_exactness(|id| id == SECOND);
            assert_eq!(builder.annotations().exactness().len(), 1);
            assert_eq!(
                builder.annotations().exactness()[SECOND],
                before.exactness()[SECOND]
            );
            assert_eq!(builder.annotations().provenance, before.provenance);
        }
    }
}
