// SPDX-License-Identifier: Apache-2.0
//! Hole center selection record framing.

use crate::feature::{feature_property_parser, PmDcFeaturePropertyKind};
use crate::test_support::test_fixtures::{content, parse as view};

fn parse(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: cadmpeg_core::decode::View<'_>,
    version: u8,
) -> Result<crate::pmdc::PmDcReferenceList, cadmpeg_core::CodecError> {
    let parser = feature_property_parser(super::CENTERS_TYPE).expect("center selection parser");
    let property = parser(ctx, source, version)?;
    let PmDcFeaturePropertyKind::References { family, items } = property.kind else {
        panic!("center references");
    };
    assert_eq!(
        family,
        crate::feature::PmDcFeatureReferenceFamily::HoleCenters
    );
    Ok(items)
}

fn selection_bytes(count: u32) -> Vec<u8> {
    let mut bytes = content(0);
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(0x3000_u16.to_le_bytes());
    bytes.extend(count.to_le_bytes());
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(0x8000_0007_u32.to_le_bytes());
    bytes
}

#[test]
fn selection_records_require_complete_counts_and_exact_ends() {
    let bytes = selection_bytes(1);
    let parsed =
        view(&bytes, |ctx, source| parse(ctx, source, 16)).expect("complete selection record");
    assert_eq!(parsed.references().len(), 1);
    assert_eq!(parsed.references()[0].index(), 7);
    assert!(parsed.references()[0].qualified());
    for end in 0..bytes.len() {
        assert!(view(&bytes[..end], |ctx, source| parse(ctx, source, 16)).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(view(&trailing, |ctx, source| parse(ctx, source, 16)).is_err());
    assert!(view(&selection_bytes(2), |ctx, source| parse(ctx, source, 16)).is_err());
}

#[test]
fn selection_lists_require_type_two_markers() {
    for marker in [[8_u16, 0x3000_u16], [2, 0]] {
        let mut bytes = selection_bytes(1);
        let start = content(0).len();
        bytes[start..start + 2].copy_from_slice(&marker[0].to_le_bytes());
        bytes[start + 2..start + 4].copy_from_slice(&marker[1].to_le_bytes());
        assert!(view(&bytes, |ctx, source| parse(ctx, source, 16)).is_err());
    }
}
