// SPDX-License-Identifier: Apache-2.0

use std::num::NonZeroI8;

use cadmpeg_ir::codec::write::saved::HistoryText;
use cadmpeg_ir::design::units::{Unit, UnitFactor, UnitSymbol};

use super::{unwritten_losses, SolidWorksText};
use crate::loss::SldprtLossCode;

fn unit(symbol: UnitSymbol) -> Unit {
    Unit(vec![UnitFactor {
        symbol,
        power: NonZeroI8::new(1).expect("power"),
    }])
}

#[test]
fn a_literal_takes_its_unit_suffix() {
    assert_eq!(
        SolidWorksText.literal(20.0, &unit(UnitSymbol::Mm), "12mm"),
        Some("20mm".to_owned())
    );
    assert_eq!(
        SolidWorksText.literal(-2.5, &unit(UnitSymbol::Deg), "5deg"),
        Some("-2.5deg".to_owned())
    );
    assert_eq!(
        SolidWorksText.literal(4.0, &Unit::default(), "3"),
        Some("4".to_owned())
    );
}

#[test]
fn a_literal_keeps_the_diameter_display_of_the_stored_text() {
    assert_eq!(
        SolidWorksText.literal(3.5, &unit(UnitSymbol::Mm), "\u{00d8}12mm"),
        Some("\u{00d8}3.5mm".to_owned())
    );
}

#[test]
fn a_unit_with_no_suffix_has_no_literal() {
    assert_eq!(
        SolidWorksText.literal(1.0, &unit(UnitSymbol::Kg), "1"),
        None
    );
}

#[test]
fn each_kind_of_design_record_has_its_loss() {
    let losses = unwritten_losses(&[
        "design.features[f].name: no history feature restates it".into(),
        "design.parameters[p].comment".into(),
        "design.scopes[s].active_configuration".into(),
    ]);
    let codes = losses
        .iter()
        .map(|loss| (loss.code.clone(), loss.message.rsplit(": ").next()))
        .collect::<Vec<_>>();
    assert_eq!(
        codes,
        [
            (
                SldprtLossCode::WriterParameterEditUnwritten.kind(),
                Some("design.parameters[p].comment")
            ),
            (
                SldprtLossCode::WriterFeatureEditUnwritten.kind(),
                Some("no history feature restates it")
            ),
            (
                SldprtLossCode::WriterDesignEditUnwritten.kind(),
                Some("design.scopes[s].active_configuration")
            ),
        ]
    );
}
