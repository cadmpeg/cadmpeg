// SPDX-License-Identifier: Apache-2.0
//! Nesting follows the session depth; value-node limits omit the bounded item.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::parse::{
    AnchorResolver, ParseDiagnosticKind, ReferenceEntry, ReferenceName, ReferenceResolver,
    ResolveError, Value,
};
use crate::test_support::{with_policy_context, with_service_context};

const ANCHOR_BOUND: &str = "anchor expansion exceeds 1000000 value nodes";
const REFERENCE_BOUND: &str = "external reference expansion exceeds 1000000 value nodes";

fn assert_bounded(error: &ResolveError, expected: &str) {
    assert!(
        matches!(error, ResolveError::Bounded(message) if message == expected),
        "{expected}: {error:?}"
    );
}

fn assert_depth_refusal(error: &ResolveError) {
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth),
        "{error:?}"
    );
}

fn depth_policy(depth: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = depth;
    policy
}

fn on_large_stack(run: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .expect("fixture thread starts")
        .join()
        .expect("fixture thread completes");
}

/// Anchors `a0` to `a{last}`; each holds ten copies of the one before, so
/// `a{k}` expands to `(10^(k+2) - 1) / 9` value nodes.
fn fan_out_anchors(last: usize) -> String {
    let mut anchors = String::from("<a0>=(1,1,1,1,1,1,1,1,1,1);");
    for index in 1..=last {
        let copy = format!("<#a{}>", index - 1);
        write!(anchors, "<a{index}>=({});", vec![copy; 10].join(","))
            .expect("write anchor fixture");
    }
    anchors
}

fn exchange_source(anchors: &str, references: &str, data: &str) -> String {
    let references = if references.is_empty() {
        String::new()
    } else {
        format!("REFERENCE;{references}ENDSEC;")
    };
    format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{anchors}ENDSEC;{references}DATA;{data}ENDSEC;END-ISO-10303-21;"
    )
}

fn parse_with_service(
    source: &str,
) -> (crate::parse::Exchange, Vec<crate::parse::ParseDiagnostic>) {
    with_service_context(source.as_bytes(), crate::parse::parse)
        .expect("a bounded expansion omits items instead of refusing the decode")
}

fn has_diagnostic(
    diagnostics: &[crate::parse::ParseDiagnostic],
    kind: ParseDiagnosticKind,
    message: &str,
) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind == kind && diagnostic.message == message)
}

#[test]
fn parameter_nesting_follows_the_session_depth() {
    on_large_stack(|| {
        let nested = format!("{}1{}", "(".repeat(300), ")".repeat(300));
        let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM({nested});ENDSEC;END-ISO-10303-21;");
        with_policy_context(source.as_bytes(), &depth_policy(1024), |source, ctx| {
            let (exchange, _) =
                crate::parse::parse(source, ctx).expect("the session admits 300 levels");
            assert!(exchange.records().contains_key(&1));
        });
        with_policy_context(source.as_bytes(), &depth_policy(200), |source, ctx| {
            let error = crate::parse::parse(source, ctx).expect_err("the session refuses");
            assert!(matches!(error, CodecError::ResourceLimit(refusal)
                if refusal.dimension == ResourceDimension::RecursionDepth
                    && refusal.operation == "step_parse_parameter_nesting"));
        });
    });
}

#[test]
fn anchor_reference_depth_follows_the_session_depth() {
    on_large_stack(|| {
        let mut anchors = BTreeMap::from([("a0".to_owned(), Value::Integer(1))]);
        for index in 1..300 {
            anchors.insert(
                format!("a{index}"),
                Value::List(vec![Value::Resource(format!("#a{}", index - 1))]),
            );
        }
        let root = Value::Resource("#a299".into());
        with_policy_context(b"", &depth_policy(4096), |_, ctx| {
            let mut resolver = AnchorResolver::new(&anchors, ctx);
            let mut value = resolver.resolve_root(&root).expect("the session admits");
            let mut levels = 0;
            while let Value::List(mut values) = value {
                value = values.pop().expect("one nested value");
                levels += 1;
            }
            assert_eq!(levels, 299);
        });
        with_policy_context(b"", &depth_policy(64), |_, ctx| {
            let error = AnchorResolver::new(&anchors, ctx)
                .resolve_root(&root)
                .expect_err("the session refuses");
            assert_depth_refusal(&error);
        });
    });
}

#[test]
fn reference_expansion_depth_follows_the_session_depth() {
    on_large_stack(|| {
        let mut anchors = BTreeMap::from([("r0".to_owned(), Value::List(vec![Value::Integer(1)]))]);
        let mut references = vec![ReferenceEntry {
            name: ReferenceName::Value(1),
            uri: "#r0".into(),
        }];
        for index in 1..300 {
            anchors.insert(
                format!("r{index}"),
                Value::List(vec![Value::ExternalReference(index)]),
            );
            references.push(ReferenceEntry {
                name: ReferenceName::Value(index + 1),
                uri: format!("#r{index}"),
            });
        }
        let omitted = BTreeSet::new();
        let root = Value::ExternalReference(300);
        with_policy_context(b"", &depth_policy(4096), |_, ctx| {
            let mut value = ReferenceResolver::new(&references, &anchors, &omitted, ctx)
                .resolve_value(&root)
                .expect("the session admits");
            let mut levels = 0;
            while let Value::List(mut values) = value {
                value = values.pop().expect("one nested value");
                levels += 1;
            }
            assert_eq!(levels, 300);
        });
        with_policy_context(b"", &depth_policy(64), |_, ctx| {
            let error = ReferenceResolver::new(&references, &anchors, &omitted, ctx)
                .resolve_value(&root)
                .expect_err("the session refuses");
            assert_depth_refusal(&error);
        });
    });
}

#[test]
fn anchor_memo_output_node_slice_is_bounded() {
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
        assert_bounded(
            &resolver
                .resolve_root(&value)
                .expect_err("the node limit stops the expansion"),
            ANCHOR_BOUND,
        );
    });
}

#[test]
fn anchor_first_expansion_output_node_slice_is_bounded() {
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([(
            "a".into(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        )]);
        let mut resolver = AnchorResolver::new(&anchors, ctx);
        resolver.remaining_nodes = 2;
        assert_bounded(
            &resolver
                .resolve_root(&Value::Resource("#a".into()))
                .expect_err("the node limit stops the expansion"),
            ANCHOR_BOUND,
        );
    });
}

#[test]
fn bounded_anchor_item_gives_back_its_nodes() {
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([(
            "a".into(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        )]);
        let mut resolver = AnchorResolver::new(&anchors, ctx);
        resolver.remaining_nodes = 4;
        let mut item = [Value::Resource("#a".into()), Value::Resource("#a".into())];
        assert_eq!(
            resolver
                .resolve_item(item.iter_mut())
                .expect("a node limit is not an error")
                .as_deref(),
            Some(ANCHOR_BOUND)
        );
        assert_eq!(resolver.remaining_nodes, 4);

        let mut item = [Value::Resource("#a".into())];
        assert_eq!(
            resolver
                .resolve_item(item.iter_mut())
                .expect("the item fits"),
            None
        );
        assert_eq!(resolver.remaining_nodes, 1);
        assert_eq!(item[0], anchors["a"]);
    });
}

/// Anchors `a0` to `a{last}`; each holds two copies of the one before, so
/// `a{k}` expands to `2^(k+2) - 1` value nodes.
fn doubling_anchor_values(last: usize) -> BTreeMap<String, Value> {
    let mut anchors = BTreeMap::from([(
        "a0".to_owned(),
        Value::List(vec![Value::Integer(1), Value::Integer(1)]),
    )]);
    for index in 1..=last {
        let previous = Value::Resource(format!("#a{}", index - 1));
        anchors.insert(
            format!("a{index}"),
            Value::List(vec![previous.clone(), previous]),
        );
    }
    anchors
}

#[test]
fn repeated_bounded_anchor_items_stop_before_copying() {
    with_service_context(b"", |_, ctx| {
        let anchors = doubling_anchor_values(12);
        let mut resolver = AnchorResolver::new(&anchors, ctx);
        resolver.remaining_nodes = 10_000;
        let mut first = [Value::Resource("#a12".into())];
        assert_eq!(
            resolver
                .resolve_item(first.iter_mut())
                .expect("a node limit is not an error")
                .as_deref(),
            Some(ANCHOR_BOUND)
        );
        let allowance = resolver.discard_allowance;

        for _ in 0..10_000 {
            let mut item = [
                Value::Resource("#a11".into()),
                Value::Resource("#a12".into()),
            ];
            assert_eq!(
                resolver
                    .resolve_item(item.iter_mut())
                    .expect("a node limit is not an error")
                    .as_deref(),
                Some(ANCHOR_BOUND)
            );
            assert_eq!(item[0], Value::Resource("#a11".into()));
        }
        assert_eq!(resolver.discard_allowance, allowance);
        assert_eq!(resolver.remaining_nodes, 10_000);

        let mut item = [Value::Resource("#a11".into())];
        assert_eq!(
            resolver
                .resolve_item(item.iter_mut())
                .expect("the item fits"),
            None
        );
        assert_eq!(resolver.remaining_nodes, 10_000 - 8_191);
    });
}

#[test]
fn omitted_anchor_items_past_the_discard_allowance_are_skipped() {
    const DISCARD_BOUND: &str =
        "anchor expansions omitted at the node limit already copied 16000000 value nodes";
    with_service_context(b"", |_, ctx| {
        let mut anchors = doubling_anchor_values(4);
        for name in ["f", "g"] {
            anchors.insert(
                name.into(),
                Value::List(vec![
                    Value::Resource("#a4".into()),
                    Value::Resource("#a4".into()),
                ]),
            );
        }
        let mut resolver = AnchorResolver::new(&anchors, ctx);
        resolver.remaining_nodes = 100;
        resolver.discard_allowance = Some(1);
        let mut resolve = |value: Value| {
            let mut item = [value];
            resolver
                .resolve_item(item.iter_mut())
                .expect("a node limit is not an error")
        };

        assert_eq!(
            resolve(Value::Resource("#f".into())).as_deref(),
            Some(ANCHOR_BOUND)
        );
        assert_eq!(
            resolve(Value::Resource("#g".into())).as_deref(),
            Some(DISCARD_BOUND)
        );
        assert_eq!(
            resolve(Value::Resource("#f".into())).as_deref(),
            Some(ANCHOR_BOUND)
        );
        assert_eq!(resolve(Value::Resource("#a4".into())), None);
        assert_eq!(resolve(Value::Integer(1)), None);
        assert_eq!(resolver.discard_allowance, None);
        assert_eq!(resolver.remaining_nodes, 100 - 63);
    });
}

#[test]
fn repeated_bounded_reference_items_stop_before_copying() {
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([(
            "a".into(),
            Value::List(vec![
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3),
            ]),
        )]);
        let references = [ReferenceEntry {
            name: ReferenceName::Value(2),
            uri: "#a".into(),
        }];
        let omitted = BTreeSet::new();
        let mut resolver = ReferenceResolver::new(&references, &anchors, &omitted, ctx);
        resolver.remaining_nodes = 10;
        let allowance = resolver.discard_allowance;
        let mut first = [Value::ExternalReference(2), Value::ExternalReference(2)];
        assert_eq!(
            resolver
                .resolve_item(first.iter_mut())
                .expect("the item fits"),
            None
        );
        assert_eq!(resolver.remaining_nodes, 2);
        assert_eq!(resolver.discard_allowance, allowance);

        for _ in 0..10_000 {
            let mut item = [Value::Integer(7), Value::ExternalReference(2)];
            assert_eq!(
                resolver
                    .resolve_item(item.iter_mut())
                    .expect("a node limit is not an error")
                    .as_deref(),
                Some(REFERENCE_BOUND)
            );
            assert_eq!(item[1], Value::ExternalReference(2));
        }
        assert_eq!(resolver.discard_allowance, allowance);
        assert_eq!(resolver.remaining_nodes, 2);
    });
}

#[test]
fn omitted_reference_items_past_the_discard_allowance_are_skipped() {
    const DISCARD_BOUND: &str = "external reference expansions omitted at the node limit already copied 16000000 value nodes";
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([
            ("big".into(), Value::List(vec![Value::Integer(1); 20])),
            ("small".into(), Value::Integer(1)),
        ]);
        let references = [
            ReferenceEntry {
                name: ReferenceName::Value(1),
                uri: "#big".into(),
            },
            ReferenceEntry {
                name: ReferenceName::Value(2),
                uri: "#big".into(),
            },
            ReferenceEntry {
                name: ReferenceName::Value(3),
                uri: "#small".into(),
            },
        ];
        let omitted = BTreeSet::new();
        let mut resolver = ReferenceResolver::new(&references, &anchors, &omitted, ctx);
        resolver.remaining_nodes = 10;
        resolver.discard_allowance = Some(1);
        let mut resolve = |value: Value| {
            let mut item = [value];
            resolver
                .resolve_item(item.iter_mut())
                .expect("a node limit is not an error")
        };

        assert_eq!(
            resolve(Value::ExternalReference(1)).as_deref(),
            Some(REFERENCE_BOUND)
        );
        assert_eq!(
            resolve(Value::ExternalReference(2)).as_deref(),
            Some(DISCARD_BOUND)
        );
        assert_eq!(
            resolve(Value::ExternalReference(1)).as_deref(),
            Some(REFERENCE_BOUND)
        );
        assert_eq!(
            resolve(Value::ExternalReference(3)).as_deref(),
            Some(DISCARD_BOUND)
        );
        assert_eq!(resolve(Value::Integer(1)), None);
        assert_eq!(resolver.discard_allowance, None);
        assert_eq!(resolver.remaining_nodes, 10);
    });
}

#[test]
fn reference_output_node_slice_bounds_before_leaf_copy() {
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([("a".into(), Value::Enumeration("TEXT".into()))]);
        let references = [ReferenceEntry {
            name: ReferenceName::Value(2),
            uri: "#a".into(),
        }];
        let omitted = BTreeSet::new();
        let mut resolver = ReferenceResolver::new(&references, &anchors, &omitted, ctx);
        resolver.remaining_nodes = 1;
        assert_eq!(
            resolver
                .resolve_value(&Value::ExternalReference(2))
                .expect("fixture operation succeeds"),
            anchors["a"]
        );
        assert_bounded(
            &resolver
                .resolve_value(&Value::ExternalReference(2))
                .expect_err("the node limit stops the expansion"),
            REFERENCE_BOUND,
        );
    });
}

#[test]
fn reference_output_node_slice_bounds_before_list_storage() {
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([(
            "a".into(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        )]);
        let references = [ReferenceEntry {
            name: ReferenceName::Value(2),
            uri: "#a".into(),
        }];
        let omitted = BTreeSet::new();
        let mut resolver = ReferenceResolver::new(&references, &anchors, &omitted, ctx);
        resolver.remaining_nodes = 2;
        assert_bounded(
            &resolver
                .resolve_value(&Value::ExternalReference(2))
                .expect_err("the node limit stops the expansion"),
            REFERENCE_BOUND,
        );
    });
}

#[test]
fn bounded_reference_item_gives_back_its_nodes() {
    with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([("a".into(), Value::Enumeration("TEXT".into()))]);
        let references = [ReferenceEntry {
            name: ReferenceName::Value(2),
            uri: "#a".into(),
        }];
        let omitted = BTreeSet::new();
        let mut resolver = ReferenceResolver::new(&references, &anchors, &omitted, ctx);
        resolver.remaining_nodes = 1;
        let mut item = [Value::ExternalReference(2), Value::ExternalReference(2)];
        assert_eq!(
            resolver
                .resolve_item(item.iter_mut())
                .expect("a node limit is not an error")
                .as_deref(),
            Some(REFERENCE_BOUND)
        );
        assert_eq!(resolver.remaining_nodes, 1);
    });
}

#[test]
fn value_node_count_depth_follows_the_session_depth() {
    let mut value = Value::Integer(1);
    for _ in 0..300 {
        value = Value::List(vec![value]);
    }
    with_policy_context(b"", &depth_policy(1024), |_, ctx| {
        assert_eq!(
            crate::parse::value_node_count(&value, 1_000_000, ctx).expect("the session admits"),
            301
        );
    });
}

#[test]
fn value_node_count_nodes_are_bounded() {
    with_service_context(b"", |_, ctx| {
        assert_bounded(
            &crate::parse::value_node_count(&Value::Integer(1), 0, ctx)
                .expect_err("the node limit stops the count"),
            "anchor value exceeds 0 value nodes",
        );
    });
}

#[test]
fn reference_expansion_limit_omits_the_record_and_its_dependents() {
    let source = exchange_source(
        &fan_out_anchors(4),
        "@10=<#a4>;",
        "#1=ITEM((@10,@10,@10,@10,@10,@10,@10,@10,@10,@10));#2=ITEM(@10);#3=ITEM(#1);",
    );
    let (exchange, diagnostics) = parse_with_service(&source);

    assert!(has_diagnostic(
        &diagnostics,
        ParseDiagnosticKind::ExpansionBounded,
        &format!("instance #1 omitted: {REFERENCE_BOUND}; exact source retained"),
    ));
    assert!(exchange
        .omitted_spans()
        .iter()
        .any(|span| source[span.clone()].starts_with("#1=ITEM((@10,")));
    assert!(exchange.records().contains_key(&2));
    assert!(has_diagnostic(
        &diagnostics,
        ParseDiagnosticKind::RecordOmitted,
        "instance #3 omitted: unresolved instance reference #1; exact source retained",
    ));
    assert!(!exchange.records().contains_key(&3));
}

#[test]
fn reference_to_a_bounded_anchor_omits_the_record() {
    let source = exchange_source(
        &fan_out_anchors(5),
        "@10=<#a5>;",
        "#1=ITEM(@10);#2=ITEM(1);",
    );
    let (exchange, diagnostics) = parse_with_service(&source);

    assert!(has_diagnostic(
        &diagnostics,
        ParseDiagnosticKind::ExpansionBounded,
        &format!("anchor <a5> omitted: {ANCHOR_BOUND}; exact source retained"),
    ));
    assert!(has_diagnostic(
        &diagnostics,
        ParseDiagnosticKind::ExpansionBounded,
        "instance #1 omitted: REFERENCE @10 names anchor <a5>, omitted at its expansion limit; exact source retained",
    ));
    assert!(!exchange.records().contains_key(&1));
    assert!(exchange.records().contains_key(&2));
}

#[test]
fn anchor_naming_a_bounded_instance_is_omitted_not_refused() {
    let source = exchange_source(
        &format!("{}<b>=#1;", fan_out_anchors(5)),
        "",
        "#1=ITEM(<#a5>);#2=ITEM(1);",
    );
    let (exchange, diagnostics) = parse_with_service(&source);

    assert!(has_diagnostic(
        &diagnostics,
        ParseDiagnosticKind::ExpansionBounded,
        &format!("instance #1 omitted: {ANCHOR_BOUND}; exact source retained"),
    ));
    assert!(has_diagnostic(
        &diagnostics,
        ParseDiagnosticKind::ExpansionBounded,
        "anchor <b> omitted: depends on instance #1, omitted at its expansion limit; exact source retained",
    ));
    assert!(exchange
        .omitted_spans()
        .iter()
        .any(|span| &source[span.clone()] == "<b>=#1;"));
    assert!(exchange.anchors().iter().all(|anchor| anchor.name != "b"));
    assert!(exchange.records().contains_key(&2));
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
