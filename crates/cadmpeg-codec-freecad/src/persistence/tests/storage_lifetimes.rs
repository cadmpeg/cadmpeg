// SPDX-License-Identifier: Apache-2.0
//! Persistence scratch retires with its last reader.

use std::fmt::Write as _;

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{parse_document, parse_properties, DependencyInfo};
use crate::dialect::FcstdDialect;

fn materialized_peak(control: impl FnOnce(&DecodeContext<'_>)) -> u64 {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::MaterializedBytes, "storage peak", None);
    control(&ctx);
    let CodecError::ResourceLimit(limit) =
        ctx.reserve_scoped(u64::MAX, "storage peak").unwrap_err()
    else {
        panic!("peak probe must refuse")
    };
    drop(probe);
    limit.limit
}

fn live_indexes(ctx: &DecodeContext<'_>, names: &[&str], node: roxmltree::Node<'_, '_>) {
    let mut storage = ctx.reserve_scoped(0, "live indexes control").unwrap();
    let dependencies = storage
        .with_storage(|| {
            ctx.collect_hash_map(
                names.iter().map(|&name| {
                    (
                        name,
                        DependencyInfo {
                            dependencies: Vec::new(),
                            storage: ctx.reserve_scoped(0, "empty dependencies control").unwrap(),
                            allow_partial: None,
                            order: 0,
                        },
                    )
                }),
                "dependency table control",
            )
        })
        .unwrap();
    let data = storage
        .with_storage(|| {
            ctx.collect_hash_map(names.iter().map(|&name| (name, node)), "data table control")
        })
        .unwrap();
    let declared = storage
        .with_storage(|| ctx.collect_hash_set(names.iter().copied(), "declared names control"))
        .unwrap();
    drop((declared, data, dependencies, storage));
}

#[test]
fn dependency_framing_storage_retires_before_the_data_lookup() {
    let text = "<Document><Objects Count=\"1\" Dependencies=\"1\"><ObjectDeps Name=\"A\" Count=\"0\"/><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"1\"><Object name=\"A\"/></ObjectData></Document>";
    // parse_document borrows a tree already owned by its caller.
    let xml = roxmltree::Document::parse(text).unwrap();
    let cap = materialized_peak(|ctx| live_indexes(ctx, &["A"], xml.root_element()));
    for below in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap - u64::from(below);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = parse_document(text, &xml, FcstdDialect::Schema4, &ctx);
        if below {
            let CodecError::ResourceLimit(original) =
                result.err().expect("live indexes need storage")
            else {
                panic!("scratch storage must refuse")
            };
            assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(
                matches!(parse_document(text, &xml, FcstdDialect::Schema4, &ctx),
                Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
        } else {
            let graph = result.unwrap();
            assert_eq!(graph.objects.len(), 1);
            assert_eq!(graph.objects[0].name(), "A");
            assert!(graph.objects[0].dependencies.is_empty());
            assert!(graph.properties.is_empty());
            assert!(graph.extensions.is_empty());
            assert_eq!(ctx.resource_refusal(), None);
        }
    }
}

#[test]
fn property_name_set_retires_before_the_original_count_diagnostic() {
    let text = "<Properties Count=\"2\"><Property name=\"P\" type=\"T\"/></Properties>";
    let xml = roxmltree::Document::parse(text).unwrap();
    let owner = "A".repeat(4096);
    let expected = format!("Properties Count=2 but 1 properties were found for {owner}");
    let cap = materialized_peak(|ctx| {
        let mut node_storage = ctx
            .reserve_scoped(0, "live property nodes control")
            .unwrap();
        let mut nodes = Vec::new();
        ctx.push_scoped_vec(
            &mut node_storage,
            &mut nodes,
            xml.root_element(),
            "property node control",
        )
        .unwrap();
        let diagnostic = ctx
            .with_scoped_storage("diagnostic control", || {
                ctx.format_retained(format_args!("{expected}"), "diagnostic text control")
            })
            .unwrap();
        drop((diagnostic, nodes, node_storage));
    });
    for below in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap - u64::from(below);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut output = Vec::new();
        // The diagnostic is a scoped result owned together with its guard.
        let result = ctx.with_scoped_storage("property diagnostic result", || {
            Ok::<_, CodecError>(
                parse_properties(text, xml.root_element(), &owner, &mut output, &ctx).unwrap_err(),
            )
        });
        assert!(output.is_empty());
        if below {
            let CodecError::ResourceLimit(original) = result.unwrap_err() else {
                panic!("diagnostic needs its bytes")
            };
            assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(
                matches!(parse_properties(text, xml.root_element(), &owner, &mut output, &ctx),
                Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
        } else {
            let (error, diagnostic_storage) = result.unwrap();
            assert!(matches!(error, CodecError::Malformed(message) if message == expected));
            assert_eq!(ctx.resource_refusal(), None);
            drop(diagnostic_storage);
        }
    }
}

#[test]
fn consumed_dependency_vector_retires_before_the_next_object() {
    const DEPENDENCIES: usize = 128;
    let long_type = "T".repeat(4096);
    let mut text = format!("<Document><Objects Count=\"2\" Dependencies=\"1\"><ObjectDeps Name=\"A\" Count=\"{DEPENDENCIES}\">");
    for _ in 0..DEPENDENCIES {
        text.push_str("<Dep Name=\"B\"/>");
    }
    write!(text, "</ObjectDeps><ObjectDeps Name=\"B\" Count=\"0\"/><Object name=\"A\" type=\"T\"/><Object name=\"B\" type=\"{long_type}\"/></Objects><ObjectData Count=\"2\"><Object name=\"A\"/><Object name=\"B\"/></ObjectData></Document>").expect("write fixture text");
    let xml = roxmltree::Document::parse(&text).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = ctx
        .with_scoped_storage("persistence graph control", || {
            parse_document(&text, &xml, FcstdDialect::Schema4, &ctx)
        })
        .unwrap();
    let graph_storage = result.1;
    let graph = result.0;
    assert_eq!(graph.objects.len(), 2);
    assert_eq!(graph.objects[0].dependencies.len(), DEPENDENCIES);
    assert!(graph.objects[0]
        .dependencies
        .iter()
        .all(|id| id == "fcstd:native:object#B"));
    assert!(graph.objects[1].dependencies.is_empty());
    assert_eq!(graph.objects[1].type_name, long_type);
    // Measure the completed graph with its live indexes, after parse scratch retired.
    let probe = RefusalProbe::arm(
        ResourceDimension::MaterializedBytes,
        "graph storage control",
        None,
    );
    live_indexes(&ctx, &["A", "B"], xml.root_element());
    let CodecError::ResourceLimit(limit) = ctx
        .reserve_scoped(u64::MAX, "graph storage control")
        .unwrap_err()
    else {
        panic!("graph storage control must refuse")
    };
    let cap = limit.limit;
    drop(probe);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let bounded_result = ctx
        .with_scoped_storage("bounded persistence graph result", || {
            parse_document(&text, &xml, FcstdDialect::Schema4, &ctx)
        })
        .unwrap();
    let bounded_storage = bounded_result.1;
    let bounded = bounded_result.0;
    assert_eq!(bounded.objects, graph.objects);
    assert_eq!(bounded.properties, graph.properties);
    assert_eq!(bounded.extensions, graph.extensions);
    assert_eq!(ctx.resource_refusal(), None);
    drop((bounded, bounded_storage, graph, graph_storage));
}
