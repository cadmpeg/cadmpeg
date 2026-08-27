// SPDX-License-Identifier: Apache-2.0
//! Dialect identification across the crate facade.
//!
//! The unit tests beside `NxDialect` pin classification against a hand-built
//! container. These pin that the real dispatch reaches the reports: that
//! `Codec::inspect` and `Codec::decode` carry exactly one match naming `nx`,
//! that the id agrees with the container parser that actually ran, and that
//! `SourceMeta` mirrors the same primary layer.

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_core::dialect::{primary_layer, Admission, DialectId, DialectMatch};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{decode, legacy_cfb_with_ug_part};
use crate::test_support::*;
use crate::NxCodec;

/// The one match naming the reporting format, and the assertion that it is one.
fn primary(dialects: &[DialectMatch]) -> &DialectMatch {
    assert_eq!(dialects.len(), 1, "NX reports exactly the primary layer");
    primary_layer(dialects, "nx").expect("exactly one entry names the reporting format")
}

fn inspect(bytes: Vec<u8>) -> cadmpeg_core::ContainerSummary {
    NxCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("synthesized NX part should inspect")
}

#[test]
fn the_modern_container_reports_the_splmsstr_row_at_inspect_and_decode() {
    let summary = inspect(prt_with_indexed_om_section());
    let matched = primary(&summary.dialects);
    assert_eq!(
        matched.dialect.as_ref().map(DialectId::as_str),
        Some("nx:splmsstr")
    );
    assert_eq!(matched.admission, Admission::Admitted);
    assert_eq!(matched.declared["splmsstr_version"], "6");
    assert_eq!(matched.declared["product_version"], "NX 2027.3102");
    assert!(!matched.declared.contains_key("ugii_version"));
    // The summary's container kind and its dialect id come from one enum.
    assert_eq!(summary.container_kind, "splmsstr");

    let result = decode(prt_with_indexed_om_section());
    assert_eq!(primary(&result.report().dialects), matched);
}

#[test]
fn the_legacy_container_reports_the_cfb_row_at_inspect_and_decode() {
    let summary = inspect(legacy_cfb_with_ug_part());
    let matched = primary(&summary.dialects);
    assert_eq!(
        matched.dialect.as_ref().map(DialectId::as_str),
        Some("nx:legacy-cfb")
    );
    assert_eq!(matched.admission, Admission::Admitted);
    assert!(matched.declared.contains_key("ugii_version"));
    assert!(!matched.declared.contains_key("splmsstr_version"));
    assert_eq!(summary.container_kind, "cfb");

    let result = decode(legacy_cfb_with_ug_part());
    assert_eq!(primary(&result.report().dialects), matched);
}

#[test]
fn source_meta_mirrors_the_primary_layer_on_every_decode_path() {
    for bytes in [prt_with_indexed_om_section(), legacy_cfb_with_ug_part()] {
        for options in [
            DecodeOptions::default(),
            DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        ] {
            let result = NxCodec
                .decode(&mut Cursor::new(bytes.clone()), &options)
                .expect("synthesized NX part should decode");
            let matched = primary(&result.report().dialects).clone();
            let source = result
                .ir()
                .source
                .as_ref()
                .expect("NX emits source metadata");

            assert_eq!(source.format, "nx");
            assert_eq!(source.dialect, matched.dialect);
            assert_eq!(source.declared, matched.declared);
            // The pre-existing attributes stay where they were.
            for (key, value) in &source.declared {
                assert_eq!(&source.attributes[key], value);
            }
        }
    }
}

#[test]
fn a_file_matching_neither_container_never_reaches_a_dialect_match() {
    // `nx:unknown` is declared in the registry and refused here: the scan
    // returns an error, so no summary and no report exist to carry the row.
    let mut bytes = single_part_prt();
    bytes[..8].copy_from_slice(b"NOTSPLMS");

    assert!(NxCodec
        .inspect(&mut Cursor::new(bytes.clone()), &InspectOptions::default())
        .is_err());
    assert!(NxCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .is_err());
}
