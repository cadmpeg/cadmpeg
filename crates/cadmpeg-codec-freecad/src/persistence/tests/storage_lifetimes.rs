// SPDX-License-Identifier: Apache-2.0
//! Persistence scratch retires with its last reader.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{parse_document, parse_properties, DependencyInfo};
use crate::dialect::FcstdDialect;

// Core's one-entry HashMap/HashSet admission uses four buckets, their control
// bytes and max(align, 16)-1 alignment padding. No removal frees the buckets.
fn one_table_bytes<T>() -> u64 {
    u64::try_from(4 * std::mem::size_of::<T>() + std::mem::align_of::<T>().max(16) - 1 + 4 + 16)
        .unwrap()
}

#[test]
fn dependency_framing_storage_retires_before_the_data_lookup() {
    let text = "<Document><Objects Count=\"1\" Dependencies=\"1\"><ObjectDeps Name=\"A\" Count=\"0\"/><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"1\"><Object name=\"A\"/></ObjectData></Document>";
    // parse_document borrows a tree already owned by its caller.
    let xml = roxmltree::Document::parse(text).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(
        ResourceDimension::MaterializedBytes,
        "FCStd object data lookup",
        None,
    );
    let CodecError::ResourceLimit(original) = parse_document(text, &xml, FcstdDialect::Schema4, &ctx)
        .err().expect("the data-table growth makes a new scratch peak")
        else { panic!("data lookup admission must refuse") };
    drop(probe);
    assert_eq!(original.operation, "FCStd object data lookup");
    // The dependency table remains; the framing Node Vec and its guard do not.
    assert_eq!(original.used, one_table_bytes::<(&str, DependencyInfo<'_, '_>)>());
    assert_eq!(original.additional, one_table_bytes::<(&str, roxmltree::Node<'_, '_>)>());
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(parse_document(text, &xml, FcstdDialect::Schema4, &ctx),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn property_name_set_retires_before_the_original_count_diagnostic() {
    let text = "<Properties Count=\"2\"><Property name=\"P\" type=\"T\"/></Properties>";
    let xml = roxmltree::Document::parse(text).unwrap();
    let owner = "A".repeat(4096);
    let expected = format!("Properties Count=2 but 1 properties were found for {owner}");
    let node_bytes = 4 * std::mem::size_of::<roxmltree::Node<'_, '_>>();
    assert!((2..=1024).contains(&std::mem::size_of::<roxmltree::Node<'_, '_>>()));
    for below in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One amortized Node Vec remains. The duplicate-name table is dead.
        policy.limits.max_materialized_bytes =
            u64::try_from(node_bytes + expected.len() - usize::from(below)).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut output = Vec::new();
        // The diagnostic is a scoped result owned together with its guard.
        let result = ctx.with_scoped_storage("property diagnostic result", || {
            Ok::<_, CodecError>(parse_properties(
                text, xml.root_element(), &owner, &mut output, &ctx,
            ).unwrap_err())
        }).unwrap();
        let diagnostic_storage = result.1;
        let error = result.0;
        assert!(output.is_empty());
        if below {
            let CodecError::ResourceLimit(original) = error else { panic!("diagnostic needs its bytes") };
            assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(original.operation, "FCStd persistence diagnostic");
            assert_eq!(original.used, u64::try_from(node_bytes).unwrap());
            assert_eq!(original.additional, u64::try_from(expected.len()).unwrap());
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(matches!(parse_properties(text, xml.root_element(), &owner, &mut output, &ctx),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else {
            assert!(matches!(error, CodecError::Malformed(message) if message == expected));
            assert_eq!(ctx.resource_refusal(), None);
        }
        drop(diagnostic_storage);
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
    text.push_str(&format!("</ObjectDeps><ObjectDeps Name=\"B\" Count=\"0\"/><Object name=\"A\" type=\"T\"/><Object name=\"B\" type=\"{long_type}\"/></Objects><ObjectData Count=\"2\"><Object name=\"A\"/><Object name=\"B\"/></ObjectData></Document>"));
    let xml = roxmltree::Document::parse(&text).unwrap();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let graph_result = ctx.with_scoped_storage("persistence graph result", || {
        parse_document(&text, &xml, FcstdDialect::Schema4, &ctx)
    }).unwrap();
    let graph_storage = graph_result.1;
    let graph = graph_result.0;
    assert_eq!(graph.objects.len(), 2);
    assert_eq!(graph.objects[0].dependencies.len(), DEPENDENCIES);
    assert!(graph.objects[0].dependencies.iter().all(|id| id == "fcstd:native:object#B"));
    assert!(graph.objects[1].dependencies.is_empty());
    assert_eq!(graph.objects[1].type_name, long_type);
    // The completed graph remains owned by its scope. Query its actual
    // admitted allocation, not a failing parse's observed output.
    let CodecError::ResourceLimit(materialized) = ctx.reserve_scoped(u64::MAX, "graph storage oracle")
        .unwrap_err() else { panic!("graph allocation oracle") };
    let scratch = one_table_bytes::<(&str, DependencyInfo<'_, '_>)>()
        + one_table_bytes::<(&str, roxmltree::Node<'_, '_>)>()
        + one_table_bytes::<&str>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Later object construction needs the three live physical tables. A's
    // consumed dependency Vec and the declaration-framing Vec no longer live.
    policy.limits.max_materialized_bytes = materialized.used + scratch;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let bounded_result = ctx.with_scoped_storage("bounded persistence graph result", || {
        parse_document(&text, &xml, FcstdDialect::Schema4, &ctx)
    }).unwrap();
    let bounded_storage = bounded_result.1;
    let bounded = bounded_result.0;
    assert_eq!(bounded.objects, graph.objects);
    assert_eq!(bounded.properties, graph.properties);
    assert_eq!(bounded.extensions, graph.extensions);
    assert_eq!(ctx.resource_refusal(), None);
    drop((bounded, bounded_storage, graph, graph_storage));
}
