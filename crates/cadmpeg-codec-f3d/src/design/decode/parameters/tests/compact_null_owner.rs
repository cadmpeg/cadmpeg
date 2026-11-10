// SPDX-License-Identifier: Apache-2.0
//! Compact parameter frames with the null owner.

use super::super::parse_design_parameter_record;
use crate::test_support::lp_utf16;

/// A compact null-owner parameter frame with class tag `372`, record index
/// 901, and the twelve-byte tail naming collection 19.
pub(super) fn compact_null_owner_parameter_record(
    expression: &str,
    expression_trailer: [u8; 5],
) -> Vec<u8> {
    compact_null_owner_parameter_record_with_ordinal(expression, expression_trailer, 87)
}

/// [`compact_null_owner_parameter_record`] with `source_ordinal`.
fn compact_null_owner_parameter_record_with_ordinal(
    expression: &str,
    expression_trailer: [u8; 5],
    source_ordinal: u32,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"372");
    out.extend_from_slice(&901u32.to_le_bytes());
    out.extend_from_slice(&[0; 15]);
    out.extend_from_slice(&source_ordinal.to_le_bytes());
    out.push(0);
    lp_utf16(&mut out, expression);
    out.extend_from_slice(&expression_trailer);
    lp_utf16(&mut out, "Linear Dimension-2");
    lp_utf16(&mut out, "mm");
    lp_utf16(&mut out, "d87");
    out.extend_from_slice(&0.635f64.to_le_bytes());
    out.extend_from_slice(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out
}

#[test]
fn compact_null_owner_parameter_has_no_owner_binding() {
    for trailer in [[0, 0, 0, 1, 0], [0; 5]] {
        let parameter =
            parse_design_parameter_record(&compact_null_owner_parameter_record("6.35 mm", trailer))
                .expect("compact null-owner parameter");
        assert_eq!(parameter.class_tag.as_str(), "372");
        assert_eq!(parameter.record_index, 901);
        assert_eq!(parameter.source_ordinal, 87);
        assert_eq!(parameter.owner_record_index(), None);
        assert_eq!(parameter.expression(), "6.35 mm");
        assert_eq!(parameter.expression_offset(), 35);
        assert_eq!(parameter.source_kind(), "Linear Dimension-2");
        assert_eq!(
            parameter.unit().map(|field| field.value.as_str()),
            Some("mm")
        );
        assert_eq!(parameter.name(), "d87");
        assert_eq!(parameter.evaluated_value().get(), 0.635);
    }
}

#[test]
fn compact_null_owner_parameter_reads_a_low_source_ordinal() {
    for source_ordinal in [0, 1] {
        let parameter = parse_design_parameter_record(
            &compact_null_owner_parameter_record_with_ordinal("d87", [0; 5], source_ordinal),
        )
        .expect("compact null-owner parameter with a low ordinal");
        assert_eq!(parameter.source_ordinal, source_ordinal);
        assert_eq!(parameter.owner_record_index(), None);
        assert_eq!(parameter.expression(), "d87");
        assert_eq!(parameter.expression_offset(), 35);
    }
}

#[test]
fn compact_null_owner_parameter_requires_its_zero_run_trailer_and_tail() {
    let malformed = compact_null_owner_parameter_record("6.35 mm", [0, 0, 0, 2, 0]);
    assert!(parse_design_parameter_record(&malformed).is_none());

    let mut frame = compact_null_owner_parameter_record("6.35 mm", [0, 0, 0, 1, 0]);
    frame[23] = 1;
    assert!(parse_design_parameter_record(&frame).is_none());

    let mut frame = compact_null_owner_parameter_record("6.35 mm", [0, 0, 0, 1, 0]);
    frame[30] = 2;
    assert!(parse_design_parameter_record(&frame).is_none());

    let mut frame = compact_null_owner_parameter_record("6.35 mm", [0, 0, 0, 1, 0]);
    frame.push(0);
    assert!(parse_design_parameter_record(&frame).is_none());
}

#[test]
fn compact_null_owner_form_requires_class_372() {
    let mut frame = compact_null_owner_parameter_record("6.35 mm", [0, 0, 0, 1, 0]);
    frame[4..7].copy_from_slice(b"373");
    assert!(parse_design_parameter_record(&frame).is_none());
}
