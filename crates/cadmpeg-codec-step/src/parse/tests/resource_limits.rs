// SPDX-License-Identifier: Apache-2.0
//! Local depth and expansion ceilings refuse the input as malformed.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodePolicy;

use crate::parse::{
    AnchorResolver, ReferenceEntry, ReferenceName, ReferenceResolver, ResolveError, Value,
};
use crate::test_support::{with_policy_context, with_service_context};

fn assert_malformed(error: &ResolveError, expected: &str) {
    assert!(
        matches!(error, ResolveError::Syntax(message) if message == expected),
        "{expected}: {error:?}"
    );
}

#[test]
fn parameter_local_depth_omits_the_record() {
    std::thread::Builder::new().stack_size(16 * 1024 * 1024).spawn(|| {
        let nested = format!("{}1{}", "(".repeat(257), ")".repeat(257));
        let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM({nested});ENDSEC;END-ISO-10303-21;");
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1024;
        with_policy_context(source.as_bytes(), &policy, |source, ctx| {
            let (exchange, diagnostics) = crate::parse::parse(source, ctx)
                .expect("an over-deep record is omitted");
            assert!(exchange.records().is_empty());
            assert!(diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == crate::parse::ParseDiagnosticKind::RecordOmitted
                    && diagnostic.message.contains("parameter nesting exceeds 256 levels")
            }));
        });
    }).expect("fixture operation succeeds").join().expect("fixture operation succeeds");
}

#[test]
fn anchor_local_depth_is_malformed() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    with_policy_context(b"", &policy, |_, ctx| {
        let anchors = BTreeMap::new();
        let error = AnchorResolver::new(&anchors, ctx)
            .resolve(&Value::Integer(1), &mut Vec::new(), 10, 256)
            .expect_err("local ceiling refuses");
        assert_malformed(&error, "anchor reference nesting exceeds 256 levels");
    });
}

#[test]
fn reference_local_depth_is_malformed() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    with_policy_context(b"", &policy, |_, ctx| {
        let anchors = BTreeMap::new();
        let error = ReferenceResolver::new(&[], &anchors, ctx)
            .resolve_value(&Value::Integer(1), 256)
            .expect_err("local ceiling refuses");
        assert_malformed(&error, "external reference nesting exceeds 256 levels");
    });
}

#[test]
fn anchor_memo_output_node_slice_is_malformed() {
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([(
            "a".into(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        )]);
        let mut resolver = AnchorResolver::new(&anchors, ctx);
        let value = Value::Resource("#a".into());
        resolver
            .resolve_root(&value)
            .expect("fixture operation succeeds");
        resolver.remaining_nodes = 2;
        assert_malformed(
            &resolver
                .resolve_root(&value)
                .expect_err("local ceiling refuses"),
            "anchor expansion exceeds 1000000 value nodes",
        );
    });
}

#[test]
fn anchor_first_expansion_output_node_slice_is_malformed() {
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([(
            "a".into(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        )]);
        let mut resolver = AnchorResolver::new(&anchors, ctx);
        resolver.remaining_nodes = 2;
        assert_malformed(
            &resolver
                .resolve_root(&Value::Resource("#a".into()))
                .expect_err("local ceiling refuses"),
            "anchor expansion exceeds 1000000 value nodes",
        );
    });
}

#[test]
fn reference_output_node_slice_refuses_before_leaf_copy() {
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([("a".into(), Value::Enumeration("TEXT".into()))]);
        let references = [ReferenceEntry {
            name: ReferenceName::Value(2),
            uri: "#a".into(),
        }];
        let mut resolver = ReferenceResolver::new(&references, &anchors, ctx);
        resolver.remaining_nodes = 1;
        assert_eq!(
            resolver
                .resolve_value(&Value::ExternalReference(2), 0)
                .expect("fixture operation succeeds"),
            anchors["a"]
        );
        assert_malformed(
            &resolver
                .resolve_value(&Value::ExternalReference(2), 0)
                .expect_err("local ceiling refuses"),
            "external reference expansion exceeds 1000000 value nodes",
        );
    });
}

#[test]
fn reference_output_node_slice_refuses_before_list_storage() {
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([(
            "a".into(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        )]);
        let references = [ReferenceEntry {
            name: ReferenceName::Value(2),
            uri: "#a".into(),
        }];
        let mut resolver = ReferenceResolver::new(&references, &anchors, ctx);
        resolver.remaining_nodes = 2;
        assert_malformed(
            &resolver
                .resolve_value(&Value::ExternalReference(2), 0)
                .expect_err("local ceiling refuses"),
            "external reference expansion exceeds 1000000 value nodes",
        );
    });
}

#[test]
fn value_node_count_depth_is_malformed() {
    let mut value = Value::Integer(1);
    for _ in 0..256 {
        value = Value::List(vec![value]);
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    with_policy_context(b"", &policy, |_, ctx| {
        assert_malformed(
            &crate::parse::value_node_count(&value, 1_000_000, ctx)
                .expect_err("local ceiling refuses"),
            "anchor value nesting exceeds 256 levels",
        );
    });
}

#[test]
fn value_node_count_nodes_are_malformed() {
    with_service_context(b"", |_, ctx| {
        assert_malformed(
            &crate::parse::value_node_count(&Value::Integer(1), 0, ctx)
                .expect_err("local ceiling refuses"),
            "anchor value exceeds the expansion node limit",
        );
    });
}

#[test]
fn unknown_record_omits_non_finite_real_at_lex_admission() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=UNKNOWN_ITEM(1.E9999);ENDSEC;END-ISO-10303-21;";
    with_service_context(SOURCE, |source, ctx| {
        let (exchange, diagnostics) =
            crate::parse::parse(source, ctx).expect("literal failure has a bounded record");
        assert!(exchange.records().is_empty());
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == crate::parse::ParseDiagnosticKind::RecordOmitted
                && diagnostic.message.contains("finite binary64 range")
        }));
    });
}
