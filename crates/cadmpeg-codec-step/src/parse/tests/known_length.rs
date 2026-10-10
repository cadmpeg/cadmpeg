// SPDX-License-Identifier: Apache-2.0
//! Exact-length parser traversals visit source items without an end probe.

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{
    schema_names_for_matching, schema_object_identifier_diagnostics, try_clone_value,
    validate_header_data_references, AdmittedSchemaIdentifier, EntityIds, EntityIndex, ParseError,
    ValidationError, Value,
};

fn empty_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy
}

#[test]
fn empty_value_copy_visits_only_its_root() {
    let policy = empty_policy(1);
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        assert_eq!(
            try_clone_value(&Value::List(Vec::new()), ctx, "STEP empty value copy")
                .expect("one root visit fits"),
            Value::List(Vec::new()),
        );
        let Err(CodecError::ResourceLimit(refusal)) = ctx.charge_work(1, "STEP next root visit")
        else {
            panic!("the single permitted root visit was consumed");
        };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.used, 1);
        assert_eq!(refusal.additional, 1);
        assert_eq!(refusal.operation, "STEP next root visit");
    });
}

#[test]
fn empty_value_copy_preserves_original_refusal() {
    let policy = empty_policy(0);
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let original = ctx
            .charge_work(1, "STEP original refusal")
            .expect_err("zero work");
        let error = try_clone_value(&Value::List(Vec::new()), ctx, "STEP empty value copy")
            .expect_err("refused session stays refused");
        let (CodecError::ResourceLimit(original), CodecError::ResourceLimit(refusal)) =
            (original, error)
        else {
            panic!("both errors must carry the original resource refusal");
        };
        assert_eq!(refusal, original);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn empty_entity_union_has_no_terminal_visit_or_storage() {
    let policy = empty_policy(0);
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let ids =
            EntityIndex::ordered_ids(&[], ctx).expect("an empty union needs no visits or backing");
        assert_eq!(ids.len(), 0);
        assert!(matches!(&ids, EntityIds::Borrowed(_)));
        assert!(ctx.resource_refusal().is_none());
        drop(ids);
    });
}

#[test]
fn empty_header_data_references_have_no_terminal_visit() {
    let policy = empty_policy(0);
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        assert!(validate_header_data_references(ctx, &[], &BTreeSet::new()).is_ok());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn empty_schema_matching_names_have_no_terminal_visit_or_storage() {
    let policy = empty_policy(0);
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let names = schema_names_for_matching(&[], ctx)
            .expect("no schema identifiers need no visits or backing");
        assert!(names.is_empty());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn empty_parser_traversals_preserve_original_refusal() {
    let policy = empty_policy(0);
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let CodecError::ResourceLimit(original) = ctx
            .charge_work(1, "STEP original refusal")
            .expect_err("zero work")
        else {
            panic!("the original failure must be a resource refusal");
        };
        let Err(CodecError::ResourceLimit(union)) = EntityIndex::ordered_ids(&[], ctx) else {
            panic!("empty union must keep the original refusal");
        };
        let Err(ValidationError::Resource(CodecError::ResourceLimit(header))) =
            validate_header_data_references(ctx, &[], &BTreeSet::new())
        else {
            panic!("empty header references must keep the original refusal");
        };
        let Err(ParseError::Resource(CodecError::ResourceLimit(schema))) =
            schema_names_for_matching(&[], ctx)
        else {
            panic!("empty schema names must keep the original refusal");
        };
        assert_eq!(union, original);
        assert_eq!(header, original);
        assert_eq!(schema, original);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn schema_diagnostics_have_no_empty_or_valid_terminal_visit() {
    let setup = cadmpeg_test_support::service_decode_context();
    let valid = AdmittedSchemaIdentifier::admit(&setup, String::from("AP242"))
        .expect("schema admission fits")
        .expect("a schema name alone is valid");
    for (admitted, work) in [(&[][..], 0), (std::slice::from_ref(&valid), 1)] {
        let policy = empty_policy(work);
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut diagnostics = schema_object_identifier_diagnostics(admitted, 17, ctx);
            assert!(diagnostics.next().is_none());
            assert!(diagnostics.next().is_none());
            assert!(ctx.resource_refusal().is_none());
        });
    }
}

#[test]
fn empty_schema_diagnostics_report_original_refusal_once() {
    let policy = empty_policy(0);
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let CodecError::ResourceLimit(original) = ctx
            .charge_work(1, "STEP original refusal")
            .expect_err("zero work")
        else {
            panic!("the original failure must be a resource refusal");
        };
        let mut diagnostics = schema_object_identifier_diagnostics(&[], 17, ctx);
        let Some(Err(CodecError::ResourceLimit(refusal))) = diagnostics.next() else {
            panic!("empty diagnostic source must keep the original refusal");
        };
        assert_eq!(refusal, original);
        assert!(diagnostics.next().is_none());
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}
