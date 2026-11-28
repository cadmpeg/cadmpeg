// SPDX-License-Identifier: Apache-2.0
//! Presentation layers whose names do not decode.

use crate::test_support::exchange::decode_inline;

/// A layer name that does not decode gives the layer up once, with the
/// decode error in its text, and no `metadata.string-invalid` stands beside.
#[test]
fn a_layer_name_that_does_not_decode_is_one_charge() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=PRESENTATION_LAYER_ASSIGNMENT('\\X2\\D800\\X0\\','',(#1));",
    );
    let losses = &result.report().losses;
    let charges = losses
        .iter()
        .filter(|loss| cadmpeg_ir::unknown::part::gives_up(&loss.code))
        .collect::<Vec<_>>();
    let [charge] = charges.as_slice() else {
        panic!("one give-up: {losses:?}");
    };
    assert_eq!(
        charge.code,
        cadmpeg_ir::unknown::part::record_unreadable_kind()
    );
    assert!(
        charge.message.contains("presentation layer name"),
        "{}",
        charge.message
    );
    assert!(!losses
        .iter()
        .any(|loss| loss.code == crate::loss::StepLossCode::MetadataStringInvalid.kind()));
}
