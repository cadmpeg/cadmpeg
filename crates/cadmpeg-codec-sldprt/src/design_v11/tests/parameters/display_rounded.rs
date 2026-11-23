// SPDX-License-Identifier: Apache-2.0
//! Bare length displays that no stored record values, near zero included.
#![allow(clippy::unwrap_used)]

use super::*;

/// One bare length display per feature: a boss extrusion depth or a fillet
/// radius, each at most a few rounding steps from zero at two decimal places.
const KEYWORDS: &[u8] = br#"<Keywords Name="Bracket">
    <Extrusion Name="Zero" Type="BossExtrude" id="7">
        <Dimension Name="D1">0</Dimension>
    </Extrusion>
    <Extrusion Name="One" Type="BossExtrude" id="8">
        <Dimension Name="D1">1</Dimension>
    </Extrusion>
    <Extrusion Name="Step" Type="BossExtrude" id="9">
        <Dimension Name="D1">0.01</Dimension>
    </Extrusion>
    <Fillet Name="Round" id="10">
        <Dimension Name="D1">R0</Dimension>
    </Fillet>
    <Fillet Name="Fine" id="11">
        <Dimension Name="D1">R0.01</Dimension>
    </Fillet>
</Keywords>"#;

/// The features of [`KEYWORDS`] and the number each displays.
const DISPLAYS: [(&str, f64); 5] = [
    ("Zero", 0.0),
    ("One", 1.0),
    ("Step", 0.01),
    ("Round", 0.0),
    ("Fine", 0.01),
];

/// Asserts that, in a part in `unit` whose units table states two decimal
/// places and whose stored records value none of [`KEYWORDS`]' dimensions
/// (the v10 reading of each is its bare displayed number), each feature of
/// `expected` (feature, literal, millimetres) holds its displayed number as
/// a literal in `unit` and as its value, that the display-rounded note names
/// exactly those parameters, and that no parameter is charged as unvalued.
fn assert_display_rounded(
    unit: cadmpeg_ir::design::units::UnitSymbol,
    expected: &[(&str, &str, f64)],
) {
    use cadmpeg_ir::design::parameter::{DimensionDisplay, ParameterOwner, ParameterValue};
    use cadmpeg_ir::design::units::{Quantity, ValueKind};
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::Length;

    let readings = DISPLAYS.map(|(feature, number)| {
        (
            feature,
            "D1",
            V10::Length(Length::new(number).unwrap()),
            None::<&str>,
        )
    });
    let (ir, parameters, notes) =
        parameters_with_scalars(KEYWORDS, &readings, &[], &[], &[], Some(unit), Some(2));
    let rounded = note_of(&notes, SldprtLossCode::DesignParameterDisplayRounded)
        .expect("the displayed literals are charged");
    assert!(
        rounded
            .message
            .starts_with(&format!("{} parameters", expected.len())),
        "{}",
        rounded.message
    );
    for (feature, literal, millimetres) in expected {
        let owner = ParameterOwner::Feature(feature_named(&ir, feature).id.clone());
        let parameter = parameters
            .iter()
            .find(|parameter| parameter.owner.as_ref() == Some(&owner))
            .unwrap_or_else(|| panic!("{feature}: {parameters:#?}"));
        assert_eq!(
            parameter.kind,
            ValueKind::Quantity(Quantity::LENGTH),
            "{feature}"
        );
        assert_eq!(
            expression_json(parameter)["value"]["text"],
            *literal,
            "{feature}"
        );
        let Some(ParameterValue::Number(value)) = parameter.value else {
            panic!("{feature} has no value: {parameter:#?}");
        };
        assert!(
            (value.get() - millimetres).abs() <= 1e-9 * millimetres.max(1.0),
            "{feature}: {}",
            value.get()
        );
        assert!(
            rounded.message.contains(parameter.id.as_str()),
            "{feature}: {}",
            rounded.message
        );
        if matches!(*feature, "Round" | "Fine") {
            assert_eq!(
                parameter.display,
                Some(DimensionDisplay::Radius),
                "{feature}"
            );
        }
    }
    assert!(
        note_of(&notes, SldprtLossCode::DesignParameterValueUnread).is_none(),
        "{notes:#?}"
    );
}

#[test]
fn a_bare_millimetre_display_near_zero_states_its_displayed_number() {
    assert_display_rounded(
        cadmpeg_ir::design::units::UnitSymbol::Mm,
        &[
            ("Zero", "0[mm]", 0.0),
            ("One", "1[mm]", 1.0),
            ("Step", "0.01[mm]", 0.01),
            ("Round", "0[mm]", 0.0),
            ("Fine", "0.01[mm]", 0.01),
        ],
    );
}

#[test]
fn a_bare_inch_display_near_zero_states_its_displayed_number_in_inches() {
    assert_display_rounded(
        cadmpeg_ir::design::units::UnitSymbol::In,
        &[
            ("Zero", "0[in]", 0.0),
            ("One", "1[in]", 25.4),
            ("Step", "0.01[in]", 0.254),
            ("Round", "0[in]", 0.0),
            ("Fine", "0.01[in]", 0.254),
        ],
    );
}
