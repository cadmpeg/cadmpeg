// SPDX-License-Identifier: Apache-2.0
//! Catalog parser tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use super::*;
use crate::test_support::catalog_stream;

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
    let catalogs = crate::catalog::parse(&catalog_stream(&entries));

    assert_eq!(catalogs.len(), 1);
    assert_eq!(catalogs[0].declared_count(), 7);
    assert_eq!(catalogs[0].entries.len(), entries.len());
    assert_eq!(catalogs[0].entries[4].ordinal, 4);
    assert_eq!(catalogs[0].entries[4].value, "Sketch");
    assert_eq!(catalogs[0].entries[5].value, "Pad");
}
