// SPDX-License-Identifier: Apache-2.0
//! The Binary admission and renderer use the same Global card boundaries.

use super::{binary_point_file, lengths};
use crate::binary::{normalize_global, parse_sections, read_global};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn binary_global_admission_uses_the_actual_rendered_card_boundaries_and_keeps_real_point_values() {
    let source = binary_point_file();
    let original = source.clone();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service())
        .expect("actual source session");
    let sections = parse_sections(root).expect("Binary sections");
    let values = read_global(sections.global.window(), lengths()).expect("actual Global fields");
    let logical = normalize_global(&values).expect("logical Global text");
    let (crossed, _) = crate::global::parse_stream(&logical).expect("physical card interpretation");
    assert!(crossed.length_context().is_none());
    let (normalized, parts) =
        crate::binary::parts::normalize(root, &ctx).expect("actual Binary callbacks");
    let scan = crate::card::scan(&normalized.bytes).expect("rendered cards");
    let (expected, expected_losses) = crate::global::parse(&scan).expect("unchanged card parser");
    assert_eq!(normalized.admission.losses, expected_losses);
    let actual = normalized
        .admission
        .global
        .length_context()
        .expect("actual scale and units");
    let expected = expected.length_context().expect("rendered scale and units");
    assert_eq!(actual.length_factor_mm(), 1.0);
    assert_eq!(actual.length_factor_mm(), expected.length_factor_mm());
    assert_eq!(
        actual.minimum_resolution_mm(),
        expected.minimum_resolution_mm()
    );
    assert_eq!(
        normalized.admission.global.native_file_name(),
        Some("part.igs")
    );
    assert_eq!(normalized.admission.global.units_name(), Some("MM"));
    let parameters = normalized.admission.parameters;
    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters[0].directory_sequence, 1);
    for (index, expected) in [(1, 1.0), (2, 2.0), (3, 3.0)] {
        assert_eq!(parameters[0].number(index), Some(expected));
    }
    let (_, _, _, part_losses) = parts.into_parts(&ctx).expect("part closure").into_parts();
    assert!(part_losses.is_empty());
    assert_eq!(source, original);
}
