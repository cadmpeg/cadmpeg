// SPDX-License-Identifier: Apache-2.0
//! Catalog parser tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use super::PREFIX;
use crate::test_support::test_object_graph::catalog_stream;

fn parse(bytes: &[u8]) -> Vec<super::Catalog> {
    crate::test_support::with_service_context(|ctx| super::parse(ctx, bytes))
        .expect("catalog fixture fits the service limits")
}

#[test]
fn catalog_entries_refuse_count_limit_before_reservation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = catalog_stream(&PREFIX);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("catalog fixture fits the input limit");
    let error =
        super::parse(&ctx, &bytes).expect_err("four catalog entries exceed zero collection items");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia_catalog_entries"));
    assert_eq!(parse(&bytes).len(), 1);
}

#[test]
fn catalog_value_refuses_retained_limit_before_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = catalog_stream(&PREFIX);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "catia_catalog_entry_value",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("catalog input");
            super::parse(&ctx, &bytes)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "catia_catalog_entry_value"));
    assert_eq!(parse(&bytes).len(), 1);
}

#[test]
fn catalog_accepts_utf8_and_expression_line_feeds() {
    let entries = [
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "angle\n°",
    ];
    let mut body = vec![0x86];
    for entry in entries {
        body.push(u8::try_from(entry.len() + 1).expect("fixture entry fits in u8"));
        body.extend_from_slice(entry.as_bytes());
    }
    let total_len = 6 + body.len();
    let mut bytes = vec![0x7c, 0x02];
    bytes.extend_from_slice(
        &u32::try_from(total_len)
            .expect("fixture catalog length fits in u32")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&body);
    let catalogs = parse(&bytes);
    assert_eq!(catalogs.len(), 1);
    assert_eq!(catalogs[0].entries[4].value, "angle\n°");
}

#[test]
fn catalog_rejects_a_count_larger_than_the_framed_entry_bytes() {
    let bytes = [0x7c, 0x02, 8, 0, 0, 0, 0xe4, 0xff];
    assert!(parse(&bytes).is_empty());
}

#[test]
fn catalog_accepts_zero_tagged_u32_entry_lengths() {
    let long = "x".repeat(300);
    let entries = ["CATCatalogManager", "catalogManager", "catalogLinks", ""];
    let mut body = vec![0x86];
    for entry in entries {
        body.push(u8::try_from(entry.len() + 1).expect("fixture entry fits in u8"));
        body.extend_from_slice(entry.as_bytes());
    }
    body.push(0);
    body.extend_from_slice(
        &u32::try_from(long.len())
            .expect("fixture entry length fits in u32")
            .to_le_bytes(),
    );
    body.extend_from_slice(long.as_bytes());
    let total_len = 6 + body.len();
    let mut bytes = vec![0x7c, 0x02];
    bytes.extend_from_slice(
        &u32::try_from(total_len)
            .expect("fixture catalog length fits in u32")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&body);
    let catalogs = parse(&bytes);
    assert_eq!(catalogs.len(), 1);
    assert_eq!(catalogs[0].entries[4].value, long);
}

#[test]
fn catalog_owns_catalog_shaped_bytes_inside_an_entry() {
    let mut nested = vec![0x7c, 0x02, 0, 0, 0, 0, 0xd1, 0x80];
    for entry in PREFIX {
        nested.push(u8::try_from(entry.len() + 1).expect("fixture entry length"));
        nested.extend_from_slice(entry.as_bytes());
    }
    nested.extend(std::iter::repeat_n(1, 123));
    nested.push(79);
    nested.extend(std::iter::repeat_n(b'x', 78));
    assert_eq!(nested.len(), 257);
    nested[2..6].copy_from_slice(&257u32.to_le_bytes());
    assert!(std::str::from_utf8(&nested).is_ok());

    let mut outer = vec![0x7c, 0x02, 0, 0, 0, 0, 0x86];
    for entry in PREFIX {
        outer.push(u8::try_from(entry.len() + 1).expect("fixture entry length"));
        outer.extend_from_slice(entry.as_bytes());
    }
    outer.push(0);
    outer.extend_from_slice(&257u32.to_le_bytes());
    outer.extend_from_slice(&nested);
    let outer_len = u32::try_from(outer.len()).expect("fixture catalog length");
    outer[2..6].copy_from_slice(&outer_len.to_le_bytes());

    let catalogs = parse(&outer);
    assert_eq!(catalogs.len(), 1);
    assert_eq!(catalogs[0].pos, 0);
    assert_eq!(catalogs[0].entries.len(), 5);
}

#[test]
fn catalog_parser_reads_exact_inclusive_length_dictionary() {
    let entries = [
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
        "Pad",
    ];
    let catalogs = parse(&catalog_stream(&entries));

    assert_eq!(catalogs.len(), 1);
    assert_eq!(catalogs[0].entries.len() + 1, 7);
    assert_eq!(catalogs[0].entries.len(), entries.len());
    assert_eq!(catalogs[0].entries[4].ordinal, 4);
    assert_eq!(catalogs[0].entries[4].value, "Sketch");
    assert_eq!(catalogs[0].entries[5].value, "Pad");
}

#[test]
fn catalog_scan_refuses_marker_free_work() {
    let bytes = [0_u8; 64];
    crate::test_support::with_work_limit(0, |ctx| {
        let cadmpeg_core::CodecError::ResourceLimit(limit) =
            super::parse(ctx, &bytes).expect_err("catalog scan consumes work")
        else {
            panic!("resource refusal required")
        };
        assert_eq!(limit.operation, "catia_catalog_scan");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}
