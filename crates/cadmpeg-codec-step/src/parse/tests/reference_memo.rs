// SPDX-License-Identifier: Apache-2.0
//! Shared occurrence chains reuse stable results across the resolution stage.

use crate::parse::{ReferenceEntry, ReferenceName, ReferenceResolver, ResolveError, Value};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn shared_reference_chains_do_not_repeat_descendant_resolution() {
    let work = |count: u64| {
        let anchors = (1..=count)
            .map(|id| {
                (
                    format!("a{id}"),
                    if id == count {
                        Value::Integer(7)
                    } else {
                        Value::ExternalReference(id + 1)
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let references = (1..=count)
            .map(|id| ReferenceEntry {
                name: ReferenceName::Value(id),
                uri: format!("#a{id}"),
            })
            .collect::<Vec<_>>();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1024;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut resolver = ReferenceResolver::new(&references, &anchors, ctx).unwrap();
            for id in 1..=count {
                assert_eq!(
                    resolver
                        .resolve_value(&Value::ExternalReference(id), 0)
                        .unwrap(),
                    Value::Integer(7)
                );
            }
            assert_eq!(resolver.memo.len(), usize::try_from(count).unwrap());
            let CodecError::ResourceLimit(refusal) = ctx
                .charge_work(u64::MAX, "test complete reference stage")
                .unwrap_err()
            else {
                panic!("work probe");
            };
            refusal.used
        })
    };
    // Doubling the graph permits linear output copies and logarithmic keyed
    // work. Replaying each chain from every root approaches quadratic work.
    assert!(work(128) < 3 * work(64));
}

#[test]
fn recovered_reference_cycle_keeps_independent_memo_entries() {
    let anchors = BTreeMap::from([
        (
            "a".into(),
            Value::List(vec![Value::ExternalReference(2), Value::Integer(1)]),
        ),
        ("b".into(), Value::ExternalReference(1)),
        ("c".into(), Value::Integer(7)),
    ]);
    let references = [
        ReferenceEntry {
            name: ReferenceName::Value(1),
            uri: "#a".into(),
        },
        ReferenceEntry {
            name: ReferenceName::Value(2),
            uri: "#b".into(),
        },
        ReferenceEntry {
            name: ReferenceName::Value(3),
            uri: "#c".into(),
        },
    ];
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut resolver = ReferenceResolver::new(&references, &anchors, ctx).unwrap();
        for id in [1, 3, 2, 3] {
            let expected = if id == 3 {
                Value::Integer(7)
            } else {
                Value::List(vec![Value::Omitted, Value::Integer(1)])
            };
            assert_eq!(
                resolver
                    .resolve_value(&Value::ExternalReference(id), 0)
                    .unwrap(),
                expected
            );
        }
        assert_eq!(resolver.memo.len(), 1);
        assert!(resolver.memo.contains_key(&ReferenceName::Value(3)));
    });
}

#[test]
fn reference_memo_preserves_expansion_depth_limit() {
    let anchors = BTreeMap::from([
        ("a".into(), Value::ExternalReference(2)),
        ("b".into(), Value::Integer(7)),
    ]);
    let references = [
        ReferenceEntry {
            name: ReferenceName::Value(1),
            uri: "#a".into(),
        },
        ReferenceEntry {
            name: ReferenceName::Value(2),
            uri: "#b".into(),
        },
    ];
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut resolver = ReferenceResolver::new(&references, &anchors, ctx).unwrap();
        assert_eq!(
            resolver
                .resolve_value(&Value::ExternalReference(2), 0)
                .unwrap(),
            Value::Integer(7)
        );
        assert!(
            matches!(resolver.resolve_value(&Value::ExternalReference(1), 254),
            Err(ResolveError::Resource(CodecError::ResourceLimit(refusal)))
                if refusal.operation == "step_reference_depth_limit"
                    && refusal.dimension == ResourceDimension::Codec("step_reference_depth_limit"))
        );
    });
}

#[test]
fn reference_memo_entries_preserve_materialized_refusal() {
    let anchors = BTreeMap::from([("a".into(), Value::Integer(7))]);
    let references = [ReferenceEntry {
        name: ReferenceName::Value(1),
        uri: "#a".into(),
    }];
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_reference_memo_entries",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
                ReferenceResolver::new(&references, &anchors, ctx)?
                    .resolve_value(&Value::ExternalReference(1), 0)
            })
            .map_err(|error| match error {
                ResolveError::Resource(error) => error,
                ResolveError::Syntax(message) => panic!("unexpected syntax error: {message}"),
            })
        },
    );
}

#[test]
fn shared_anchor_chains_use_keyed_cycle_search() {
    let work = |count: u64| {
        let anchors = (1..=count)
            .map(|id| {
                (
                    format!("a{id}"),
                    if id == count {
                        Value::Integer(7)
                    } else {
                        Value::Resource(format!("a{}", id + 1))
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1024;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut resolver = crate::parse::AnchorResolver::new(&anchors, ctx).unwrap();
            for id in 1..=count {
                assert_eq!(
                    resolver
                        .resolve_root(&Value::Resource(format!("a{id}")))
                        .unwrap(),
                    Value::Integer(7)
                );
            }
            assert_eq!(resolver.memo.len(), usize::try_from(count).unwrap());
            let CodecError::ResourceLimit(refusal) = ctx
                .charge_work(u64::MAX, "test complete anchor stage")
                .unwrap_err()
            else {
                panic!("work probe");
            };
            refusal.used
        })
    };
    assert!(work(128) < 3 * work(64));
}

#[test]
fn terminal_reference_uri_is_classified_once() {
    let additional_work = |length: usize| {
        let anchors = BTreeMap::new();
        let references = [ReferenceEntry {
            name: ReferenceName::Value(1),
            uri: format!("{}#unresolved", "a".repeat(length)),
        }];
        let work = |roots: usize| {
            crate::test_support::with_service_context(b"", |_, ctx| {
                let mut resolver = ReferenceResolver::new(&references, &anchors, ctx).unwrap();
                for _ in 0..roots {
                    assert_eq!(
                        resolver
                            .resolve_value(&Value::ExternalReference(1), 0)
                            .unwrap(),
                        Value::ExternalReference(1)
                    );
                }
                let CodecError::ResourceLimit(refusal) = ctx
                    .charge_work(u64::MAX, "test completed URI classification")
                    .unwrap_err()
                else {
                    panic!("work probe");
                };
                refusal.used
            })
        };
        work(8) - work(1)
    };
    assert_eq!(additional_work(32), additional_work(8192));
}

#[test]
fn discarded_reference_target_releases_its_backing() {
    // An entity occurrence cannot bind to an enumeration value. The candidate is
    // checked under scratch ownership and the replacement is an omitted node.
    let anchors = BTreeMap::from([("a".into(), Value::Enumeration("x".repeat(128)))]);
    let references = [ReferenceEntry {
        name: ReferenceName::Entity(1),
        uri: "#a".into(),
    }];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut resolver = ReferenceResolver::new(&references, &anchors, ctx).unwrap();
        for _ in 0..8 {
            assert_eq!(
                resolver.resolve_value(&Value::Reference(1), 0).unwrap(),
                Value::Omitted
            );
        }
        assert!(ctx.resource_refusal().is_none());
    });
}
