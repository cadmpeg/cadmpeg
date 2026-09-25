// SPDX-License-Identifier: Apache-2.0
//! Part 21 parser work-budget and storage-accounting tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{
    AnchorEntry, AnchorResolver, ReferenceEntry, ReferenceName, ReferenceResolver, ResolveError,
    Value,
};

#[test]
fn anchor_list_slots_are_admitted_before_vector_allocation() {
    let value = Value::List((0..8).map(Value::Integer).collect());
    let anchors = BTreeMap::new();
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &service)
        .expect("root fits service profile");
    assert_eq!(
        AnchorResolver::new(&anchors, Some(&ctx))
            .resolve_root(&value)
            .expect("service admits list"),
        value
    );
    let mut limited = service;
    limited.limits.max_collection_items = 8;
    let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &limited)
        .expect("root fits selected profile");
    let error = AnchorResolver::new(&anchors, Some(&ctx))
        .resolve_root(&value)
        .expect_err("one list node plus eight slots exceed eight items");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_anchor_list_items")
    );
}

#[test]
fn anchor_memo_entry_is_admitted_before_the_clone() {
    let anchors = BTreeMap::from([(
        "a".to_string(),
        Value::List(vec![Value::Integer(1), Value::Integer(2)]),
    )]);
    let value = Value::Resource("a".into());
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &service)
        .expect("root fits service profile");
    assert_eq!(
        AnchorResolver::new(&anchors, Some(&ctx))
            .resolve_root(&value)
            .expect("service admits memo"),
        anchors["a"]
    );
    let mut limited = service;
    limited.limits.max_collection_items = 9;
    let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &limited)
        .expect("root fits selected profile");
    let error = AnchorResolver::new(&anchors, Some(&ctx))
        .resolve_root(&value)
        .expect_err("memo entry exceeds nine prior admitted items");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_anchor_memo_entry")
    );
}

#[test]
fn anchor_typed_wrapper_is_charged_before_its_clone() {
    let value = Value::Typed("LENGTH_MEASURE".into(), Box::new(Value::Integer(7)));
    let anchors = BTreeMap::new();
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &service)
        .expect("root fits service profile");
    assert_eq!(
        AnchorResolver::new(&anchors, Some(&ctx))
            .resolve_root(&value)
            .expect("service admits typed value"),
        value
    );
    let mut limited = service;
    limited.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Value>());
    let (ctx, _) = DecodeContext::from_root_bytes(b"anchor", &arena, &limited)
        .expect("root fits selected profile");
    let error = AnchorResolver::new(&anchors, Some(&ctx))
        .resolve_root(&value)
        .expect_err("typed wrapper exceeds the leaf's retained bytes");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_anchor_materialization_storage")
    );
}

#[test]
fn reference_bindings_are_admitted_before_map_allocation() {
    let anchors = BTreeMap::new();
    let references = [ReferenceEntry {
        name: ReferenceName::Value(2),
        uri: "#a".into(),
    }];
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &service)
        .expect("root fits service profile");
    ReferenceResolver::new(&references, &anchors, Some(&ctx))
        .expect("service admits the binding map");
    let mut limited = service;
    limited.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &limited)
        .expect("root fits selected profile");
    let error = ReferenceResolver::new(&references, &anchors, Some(&ctx))
        .err()
        .expect("binding must be admitted before collection");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_reference_binding_items")
    );
}

#[test]
fn reference_stack_item_is_admitted_before_push() {
    let anchors = BTreeMap::from([("a".into(), Value::Integer(7))]);
    let references = [ReferenceEntry {
        name: ReferenceName::Value(2),
        uri: "#a".into(),
    }];
    let value = Value::ValueReference(2);
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &service)
        .expect("root fits service profile");
    assert_eq!(
        ReferenceResolver::new(&references, &anchors, Some(&ctx))
            .expect("create resolver")
            .resolve_value(&value, 0)
            .expect("service admits reference"),
        Value::Integer(7)
    );
    let mut limited = service;
    limited.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &limited)
        .expect("root fits selected profile");
    let error = ReferenceResolver::new(&references, &anchors, Some(&ctx))
        .expect("binding fits selected profile")
        .resolve_value(&value, 0)
        .expect_err("stack item exceeds the remaining collection allowance");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_reference_stack")
    );
}

#[test]
fn reference_list_slots_are_admitted_before_vector_allocation() {
    let anchors = BTreeMap::new();
    let value = Value::List(vec![
        Value::Integer(1),
        Value::Integer(2),
        Value::Integer(3),
    ]);
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &service)
        .expect("root fits service profile");
    assert_eq!(
        ReferenceResolver::new(&[], &anchors, Some(&ctx))
            .expect("create resolver")
            .resolve_value(&value, 0)
            .expect("service admits list"),
        value
    );
    let mut limited = service;
    limited.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &limited)
        .expect("root fits selected profile");
    let error = ReferenceResolver::new(&[], &anchors, Some(&ctx))
        .expect("create resolver")
        .resolve_value(&value, 0)
        .expect_err("three list slots exceed two admitted items");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_reference_list_items")
    );
}

#[test]
fn reference_typed_wrapper_is_charged_before_its_clone() {
    let anchors = BTreeMap::new();
    let value = Value::Typed("LENGTH_MEASURE".into(), Box::new(Value::Integer(7)));
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &service)
        .expect("root fits service profile");
    assert_eq!(
        ReferenceResolver::new(&[], &anchors, Some(&ctx))
            .expect("create resolver")
            .resolve_value(&value, 0)
            .expect("service admits typed value"),
        value
    );
    let mut limited = service;
    limited.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Value>());
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &limited)
        .expect("root fits selected profile");
    let error = ReferenceResolver::new(&[], &anchors, Some(&ctx))
        .expect("create resolver")
        .resolve_value(&value, 0)
        .expect_err("typed wrapper exceeds the leaf's retained bytes");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_reference_materialization_storage")
    );
}

#[test]
fn reference_anchor_copy_is_charged_before_building_bindings() {
    let mut anchors = [AnchorEntry {
        name: "a".into(),
        value: Value::Integer(7),
        tags: Vec::new(),
    }];
    let mut records = BTreeMap::new();
    let references = [ReferenceEntry {
        name: ReferenceName::Value(2),
        uri: "#a".into(),
    }];
    let arena = DecodeArena::new();
    let mut limited = DecodePolicy::service();
    limited.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"reference", &arena, &limited)
        .expect("root fits selected profile");
    let error =
        super::super::resolve_local_references(&mut anchors, &mut records, &references, Some(&ctx))
            .expect_err("anchor copy must be admitted before cloning");
    assert!(
        matches!(error, ResolveError::Resource(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_reference_anchor_copies")
    );
}

#[test]
fn parser_propagates_binary_lexeme_resource_refusal() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(\"0A1F2\");ENDSEC;END-ISO-10303-21;";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits the test policy");
    let error = crate::parse::parse_with_context(source, &ctx)
        .expect_err("binary lexeme must refuse before its digit allocation");
    assert!(
        matches!(&error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes && limit.operation == "step_binary_lexeme_temp"),
        "{error:?}"
    );
}

#[test]
fn parser_uses_the_decode_session_work_budget() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','','',(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_work_units = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits the test policy");
    let error = crate::parse::parse_with_context(source, &ctx).expect_err("budget must refuse");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
    ));
}

#[test]
fn parser_accounts_for_owned_value_storage() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(1);ENDSEC;END-ISO-10303-21;";
    let mut value_storage_limit = None;
    for max_retained_bytes in 1..=4096 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source, &ctx)
            .expect_err("owned value storage must consume retained bytes");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.operation == "step_parse_value_storage" {
            value_storage_limit = Some(limit);
            break;
        }
    }
    let limit = value_storage_limit.expect("value storage must have a retained-byte gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert!(limit.additional > 0);
    assert!(limit.used <= limit.limit);
}

#[test]
fn parser_accounts_for_record_table_storage() {
    let records = (1..=64).fold(String::new(), |mut records, id| {
        write!(records, "#{id}=ITEM();").expect("write record fixture");
        records
    });
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;"
    );
    crate::parse::parse(source.as_bytes()).expect("record-table fixture must parse");
    let mut record_table_limit = None;
    for max_retained_bytes in (1..=131_072).step_by(64) {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            source.as_bytes(),
            &arena,
            &policy,
        )
        .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source.as_bytes(), &ctx)
            .expect_err("record-table storage must consume retained bytes");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.operation == "step_parse_record_table_storage" {
            record_table_limit = Some(limit);
            break;
        }
    }
    let limit = record_table_limit.expect("record-table storage must be charged");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert!(limit.additional > 0);
    assert!(limit.used <= limit.limit);
}

#[test]
fn parser_accounts_for_anchor_tag_collection_storage() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<a>=1 {tag:2};ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    crate::parse::parse(source).expect("anchor-tag fixture must parse");
    let mut tag_limit = None;
    for max_retained_bytes in 1..=8192 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source, &ctx)
            .expect_err("anchor-tag storage must consume retained bytes");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.operation == "step_anchor_tag_storage" {
            tag_limit = Some(limit);
            break;
        }
    }
    let limit = tag_limit.expect("anchor-tag storage must have a retained-byte gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert!(limit.additional > 0);
}

#[test]
fn anchor_materialization_uses_the_decode_session_budget() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<a>=(1,2,3,4,5,6,7,8);ENDSEC;DATA;#1=ITEM(<a>);ENDSEC;END-ISO-10303-21;";
    let mut materialization_limit = None;
    for max_work_units in 1..=1024 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source, &ctx)
            .expect_err("anchor materialization must consume shared work");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.operation == "step_anchor_materialization" {
            materialization_limit = Some(limit);
            break;
        }
    }
    let limit = materialization_limit.expect("anchor materialization must have a budget gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert!(limit.additional > 0);
    assert!(limit.used <= limit.limit);
}

#[test]
fn local_reference_materialization_uses_the_decode_session_budget() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<a>=(1,2,3,4,5,6,7,8);ENDSEC;REFERENCE;@2=<#a>;ENDSEC;DATA;#1=ITEM(@2);ENDSEC;END-ISO-10303-21;";
    crate::parse::parse(source).expect("local-reference fixture must parse");
    let mut materialization_limit = None;
    for max_work_units in 1..=2048 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source, &ctx)
            .expect_err("local reference materialization must consume shared work");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.operation == "step_reference_materialization" {
            materialization_limit = Some(limit);
            break;
        }
    }
    let limit = materialization_limit.expect("local references must have a budget gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert!(limit.additional > 0);
    assert!(limit.used <= limit.limit);
}

#[test]
fn parser_bounds_exponential_anchor_expansion() {
    let mut anchors = String::from("<a0>=(1,1);\n");
    for index in 1..40 {
        writeln!(anchors, "<a{index}>=(<a{}>,<a{}>);", index - 1, index - 1)
            .expect("write anchor fixture");
    }
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{anchors}ENDSEC;DATA;#1=ITEM(<a39>);ENDSEC;END-ISO-10303-21;"
    );
    let error = crate::parse::parse(source.as_bytes()).unwrap_err();
    assert!(error.to_string().contains("expanded anchor value exceeds"));
}

#[test]
fn parser_bounds_aggregate_anchor_materialization() {
    let mut anchors = String::from("<a0>=(1,1);\n");
    for index in 1..18 {
        writeln!(anchors, "<a{index}>=(<a{}>,<a{}>);", index - 1, index - 1)
            .expect("write anchor fixture");
    }
    let records = (1..=8).fold(String::new(), |mut records, id| {
        write!(records, "#{id}=ITEM(<a17>);").expect("write anchor record fixture");
        records
    });
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{anchors}ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;"
    );
    let error = crate::parse::parse(source.as_bytes()).unwrap_err();
    assert!(error.to_string().contains("expanded anchor"));
}
