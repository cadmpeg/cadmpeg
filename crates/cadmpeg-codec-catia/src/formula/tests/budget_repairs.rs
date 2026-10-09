// SPDX-License-Identifier: Apache-2.0
//! Formula candidate and scope admission.

use crate::test_support::{with_materialized_limit, with_retained_limit, with_work_refusal};
use cadmpeg_core::CodecError;

#[test]
fn formula_conflicting_input_admits_its_surviving_index_key() {
    let native = crate::native::CatiaNative::decode(
        &crate::test_support::test_formula::standard_catpart_with_formula_relation(4, false),
    );
    let entity = native
        .entity_records
        .iter()
        .find(|entity| entity.parameter_value().is_some())
        .expect("typed input");
    let value = entity.parameter_value().expect("typed input");
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| -> Result<_, CodecError> {
        let existing =
            super::super::typed_entity_parameter_candidate(ctx, entity, value, "LENGTH")?
                .expect("candidate");
        let mut incoming =
            super::super::typed_entity_parameter_candidate(ctx, entity, value, "LENGTH")?
                .expect("candidate");
        incoming.parameter.value = None;
        let expected = existing.parameter.id.clone();
        let mut scratch = ctx.reserve_scoped(0, "test conflict index")?;
        let mut candidates = std::collections::BTreeMap::from([(expected.clone(), existing)]);
        let mut conflicts = std::collections::BTreeSet::new();
        super::super::merge_formula_parameter_candidate(
            ctx,
            &mut scratch,
            &mut candidates,
            &mut conflicts,
            incoming,
        )?;
        drop(candidates);
        assert_eq!(conflicts.into_iter().collect::<Vec<_>>(), [expected]);
        Ok(())
    };
    let refusal = with_work_refusal("catia_formula_conflict_id", |ctx| {
        let result = run(ctx);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    });
    assert!(matches!(refusal, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_formula_conflict_id"));
    with_retained_limit(0, run).expect("the surviving conflict key remains scoped");
}

#[test]
fn formula_speculative_candidates_release_scoped_storage() {
    let native = crate::native::CatiaNative::decode(
        &crate::test_support::test_formula::standard_catpart_with_formula_relation(4, false),
    );
    let entity = native
        .entity_records
        .iter()
        .find(|entity| entity.parameter_value().is_some())
        .expect("typed input");
    let value = entity.parameter_value().expect("typed input");
    with_materialized_limit(65536, |ctx| -> Result<_, CodecError> {
        for _ in 0..64 {
            let candidate =
                super::super::typed_entity_parameter_candidate(ctx, entity, value, "LENGTH")?
                    .expect("candidate");
            assert_eq!(candidate.parameter.name, value.name.value);
            drop(candidate);
            let storage = ctx.reserve_scoped(65536, "test released candidate")?;
            drop(storage);
        }
        Ok(())
    })
    .expect("candidate storage tracks live candidates");
    with_retained_limit(0, |ctx| -> Result<_, CodecError> {
        let candidate =
            super::super::typed_entity_parameter_candidate(ctx, entity, value, "LENGTH")?
                .expect("candidate");
        assert_eq!(
            candidate.parameter.native_ref.as_deref(),
            Some(entity.id.as_str())
        );
        Ok(())
    })
    .expect("speculative candidates consume no retained storage");
}

#[test]
fn formula_container_scope_refuses_variable_length_equality() {
    let binding = super::binding(&"stream".repeat(1024));
    let refused = with_work_refusal("catia_legacy_container_scope_match", |ctx| {
        super::super::outer_container_in_scope(
            ctx,
            Some(&binding),
            super::LegacyModelingScope::Container(&binding),
        )
    });
    assert!(
        matches!(refused, Err(CodecError::ResourceLimit(limit)) if limit.operation == "catia_legacy_container_scope_match")
    );
    let mut other = binding.clone();
    other.ordinal += 1;
    assert!(!crate::test_support::with_work_limit(
        0,
        |ctx| super::super::outer_container_in_scope(
            ctx,
            Some(&other),
            super::LegacyModelingScope::Container(&binding)
        )
    )
    .expect("scalar mismatch runs no string comparison"));
}
