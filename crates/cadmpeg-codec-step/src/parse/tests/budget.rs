// SPDX-License-Identifier: Apache-2.0
//! Part 21 parser work-budget and storage-accounting tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::fmt::Write as _;

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
        if limit.context.operation == "step_parse_value_storage" {
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
        if limit.context.operation == "step_parse_record_table_storage" {
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
        if limit.context.operation == "step_anchor_tag_storage" {
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
        if limit.context.operation == "step_anchor_materialization" {
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
        if limit.context.operation == "step_reference_materialization" {
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
