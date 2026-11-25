// SPDX-License-Identifier: Apache-2.0

use std::num::NonZeroI8;

use cadmpeg_ir::codec::write::saved::HistoryText;
use cadmpeg_ir::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, ValueKind};

use super::FusionText;

#[test]
fn a_literal_reads_back_as_the_decoder_reads_it() {
    let millimetres = Unit(vec![UnitFactor {
        symbol: UnitSymbol::Mm,
        power: NonZeroI8::new(1).expect("power"),
    }]);
    let text = FusionText
        .literal(3.5, &millimetres, "2 mm")
        .expect("a millimetre literal");
    assert_eq!(text, "3.5 mm");
    assert!(
        crate::design_v11::parameters::literal(&text, &ValueKind::Quantity(Quantity::LENGTH))
            .is_some()
    );
    assert_eq!(
        FusionText.literal(4.0, &Unit::default(), "3"),
        Some("4".to_owned())
    );
}
