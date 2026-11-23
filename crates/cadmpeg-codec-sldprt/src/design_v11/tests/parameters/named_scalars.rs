// SPDX-License-Identifier: Apache-2.0
//! Several scalars under one dimension's name that each show its display.
#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn scalars_that_show_the_display_state_one_length_within_the_linear_resolution() {
    use super::super::super::parameters::NamedScalar;
    use crate::resolved_features::parameter_classes::StatedParameter;
    use cadmpeg_ir::design::parameter::{DesignParameter, ParameterValue};
    use cadmpeg_ir::design::units::{Quantity, UnitSymbol, ValueKind};

    // Each display shows every scalar under its name at two places. D1's
    // lengths differ by one unit in the last place of their f64, D2's by
    // 1.2e-12 mm, D3's by 2e-5 mm, beyond the 1e-5 mm linear resolution,
    // and D4's angles by 1e-13 of their size.
    let keywords = br#"<Keywords Name="Bracket">
            <Feature Name="Sketch1" Type="ProfileFeature" id="7">
                <Dimension Name="D1">20</Dimension>
                <Dimension Name="D2">1.25</Dimension>
                <Dimension Name="D3">20</Dimension>
                <Dimension Name="D4">30</Dimension>
            </Feature>
        </Keywords>"#;
    let shown = |stated| NamedScalar {
        stated: Some(stated),
        driving: false,
    };
    let length = |metres| shown(StatedParameter::Length(metres));
    let angle = |degrees: f64| shown(StatedParameter::Angle(degrees.to_radians()));
    let named = [
        ("Sketch1", "D1", length(0.02)),
        ("Sketch1", "D1", length(0.02f64.next_up())),
        ("Sketch1", "D2", length(0.001_25)),
        ("Sketch1", "D2", length(0.001_250_000_000_001_2)),
        ("Sketch1", "D3", length(0.02)),
        ("Sketch1", "D3", length(0.020_000_02)),
        ("Sketch1", "D4", angle(30.0)),
        ("Sketch1", "D4", angle(30.0 * (1.0 + 1e-13))),
    ];
    let (_, parameters, notes) = parameters_with_scalars(
        keywords,
        &[],
        &[],
        &[],
        &named,
        Some(UnitSymbol::Mm),
        Some(2),
    );
    let named_in = |name: &str| -> DesignParameter {
        parameters
            .iter()
            .find(|parameter| {
                parameter
                    .name
                    .as_ref()
                    .map(cadmpeg_core::text::NonBlankString::as_str)
                    == Some(name)
            })
            .unwrap_or_else(|| panic!("{name}: {parameters:#?}"))
            .clone()
    };
    for (name, quantity, expected) in [
        ("D1", Quantity::LENGTH, 20.0),
        ("D2", Quantity::LENGTH, 1.25),
        ("D4", Quantity::ANGLE, 30f64.to_radians()),
    ] {
        let parameter = named_in(name);
        assert_eq!(parameter.kind, ValueKind::Quantity(quantity), "{name}");
        let Some(ParameterValue::Number(value)) = parameter.value else {
            panic!("{name} has no value: {parameter:#?}");
        };
        assert!(
            (value.get() - expected).abs() <= 1e-9,
            "{name}: {}",
            value.get()
        );
    }
    let d3 = named_in("D3");
    assert_eq!(d3.value, None);
    let note = note_of(&notes, SldprtLossCode::DesignParameterValueUnread).expect("D3 is named");
    assert!(
        note.message.contains(&format!(
            "1 whose stored values differ from the display or from each other ({})",
            d3.id.as_str()
        )),
        "{}",
        note.message
    );
}
