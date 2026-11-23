// SPDX-License-Identifier: Apache-2.0
//! The header flag at offset 21 of a discriminated parameter record.

use super::*;

/// A `Thickness` parameter with the null owner, family discriminator `6`,
/// the ten-byte expression trailer of zero bytes, and header flag `flag`.
fn thickness(flag: u8) -> Vec<u8> {
    let mut frame = parameter_record(None, "2.50 mm", "Thickness", Some("mm"), "d1", 0.25);
    frame[21] = flag;
    frame[22..26].copy_from_slice(&6u32.to_le_bytes());
    let expression_end = 36 + 4 + 2 * "2.50 mm".len();
    frame.splice(expression_end..expression_end + 9, [0; 10]);
    frame
}

#[test]
fn the_discriminated_form_reads_either_header_flag() {
    for flag in [0, 1] {
        let parsed = super::super::parse_design_parameter(&thickness(flag))
            .expect("no decode error")
            .expect("Thickness parameter");
        assert_eq!(parsed.header_flag, flag == 1);
        let parameter = parse_design_parameter_record(&thickness(flag)).expect("record");
        assert_eq!(parameter.source_kind().as_str(), "Thickness");
        assert_eq!(parameter.name(), "d1");
        assert_eq!(parameter.owner_record_index(), None);
        assert_ne!(parameter.kind(), DesignParameterKind::User);
        assert_eq!(
            parameter
                .family_discriminator()
                .map(|field| field.value.code()),
            Some(6)
        );
        assert_eq!(parameter.unit().map(|unit| unit.value.as_str()), Some("mm"));
        assert!((parameter.evaluated_value().get() - 0.25).abs() < 1e-12);
    }
}

#[test]
fn a_header_flag_other_than_zero_or_one_matches_no_grammar() {
    assert!(parse_design_parameter_record(&thickness(2)).is_none());
}

#[test]
fn a_set_header_flag_closes_no_compact_form() {
    let mut frame =
        compact_owned_parameter_record(428, 9, "3.40 mm", "HoleDiameter", Some("mm"), "d9", 0.34);
    assert!(parse_design_parameter_record(&frame).is_some());
    frame[21] = 1;
    assert!(parse_design_parameter_record(&frame).is_none());
}
