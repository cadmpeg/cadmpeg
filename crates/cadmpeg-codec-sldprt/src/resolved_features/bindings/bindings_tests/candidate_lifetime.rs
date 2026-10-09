//! Candidate destruction keeps its copied text within the live reservation.

use super::super::{push_distinct_candidate, PATTERN_INPUTS};
use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;

const TEXT: &str = "candidate";
const TEXT_BYTES: u64 = TEXT.len() as u64;
const DROP_PROBE: &str = "probe SLDPRT binding candidate destruction";

struct ObservedText<'ctx, 'session> {
    text: Cow<'static, str>,
    ctx: &'ctx DecodeContext<'session>,
    observation: &'ctx RefCell<Option<CodecError>>,
    panic_on_compare: bool,
}

impl PartialEq for ObservedText<'_, '_> {
    fn eq(&self, other: &Self) -> bool {
        assert!(!self.panic_on_compare && !other.panic_on_compare, "comparison unwind");
        self.text == other.text
    }
}

impl DecodeCost for ObservedText<'_, '_> {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        self.text.as_ref().decode_cost(ctx, operation)
    }
}

impl Drop for ObservedText<'_, '_> {
    fn drop(&mut self) {
        if matches!(&self.text, Cow::Owned(_)) {
            // The owned String still exists during this destructor. Its exact
            // byte allowance leaves no room for one additional scratch byte.
            *self.observation.borrow_mut() = self.ctx.reserve_scoped(1, DROP_PROBE).err();
        }
    }
}

fn observe_candidate_drop(panic_on_compare: bool) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = TEXT_BYTES;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let observation = RefCell::new(None);
    // This existing vote borrows fixed text and has no dynamic child storage.
    // Its test fixture table is outside the candidate's allocation invariant.
    let existing = ObservedText {
        text: Cow::Borrowed(TEXT),
        ctx: &ctx,
        observation: &observation,
        panic_on_compare: false,
    };
    let mut groups = BTreeMap::from([(0, vec![(existing, None)])]);
    let mut table_storage = ctx.reserve_scoped(0, PATTERN_INPUTS).unwrap();
    let candidate_storage;
    let copied;
    (copied, candidate_storage) = ctx.with_scoped_storage(PATTERN_INPUTS, || {
        ctx.copy_retained_text(TEXT, PATTERN_INPUTS)
    }).unwrap();
    let candidate = ObservedText {
        text: Cow::Owned(copied),
        ctx: &ctx,
        observation: &observation,
        panic_on_compare,
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        push_distinct_candidate(
            &ctx,
            &mut table_storage,
            &mut groups,
            0,
            (candidate, Some(candidate_storage)),
        )
    }));
    if panic_on_compare {
        assert!(result.is_err());
    } else {
        result.unwrap().unwrap();
    }
    assert_eq!(groups[&0].len(), 1);
    assert_eq!(groups[&0][0].0.text, TEXT);
    let observation = observation.borrow();
    let Some(CodecError::ResourceLimit(limit)) = observation.as_ref() else {
        panic!("candidate storage must remain live during its destructor");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, DROP_PROBE);
    assert_eq!(limit.used, TEXT_BYTES);
    assert_eq!(limit.additional, 1);
    assert_eq!(limit.limit, TEXT_BYTES);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
}

#[test]
fn duplicate_binding_candidate_keeps_storage_during_destruction() {
    observe_candidate_drop(false);
}

#[test]
fn binding_comparison_unwind_keeps_candidate_storage_during_destruction() {
    observe_candidate_drop(true);
}
