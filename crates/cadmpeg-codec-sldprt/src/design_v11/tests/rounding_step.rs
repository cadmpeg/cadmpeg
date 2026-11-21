// SPDX-License-Identifier: Apache-2.0
//! Length displays within one rounding step of zero.
#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn a_length_display_at_most_one_rounding_step_from_zero_holds_no_value() {
    use crate::records::ArchiveParameterClass;
    use cadmpeg_ir::design::parameter::{DimensionDisplay, ParameterOwner, ParameterValue};
    use cadmpeg_ir::design::units::{Quantity, UnitSymbol, ValueKind};
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::Length;

    let length = |millimetres: f64| V10::Length(Length::new(millimetres).unwrap());
    let (ir, parameters, notes) = parameters_with_readings(
        br#"<Keywords Name="Bracket">
            <Extrusion Name="Zero" Type="BossExtrude" id="7">
                <Dimension Name="D1">0</Dimension>
            </Extrusion>
            <Fillet Name="Round" id="8">
                <Dimension Name="D1">R0</Dimension>
            </Fillet>
            <Extrusion Name="Step" Type="BossExtrude" id="9">
                <Dimension Name="D1">0.01</Dimension>
            </Extrusion>
            <Extrusion Name="Above" Type="BossExtrude" id="10">
                <Dimension Name="D1">0.02</Dimension>
            </Extrusion>
            <Extrusion Name="Stored" Type="BossExtrude" id="11">
                <Dimension Name="D1">0.01</Dimension>
            </Extrusion>
        </Keywords>"#,
        &[
            ("Zero", "D1", length(0.0), None),
            ("Round", "D1", length(0.0), None),
            ("Step", "D1", length(0.01), None),
            ("Above", "D1", length(0.02), None),
            ("Stored", "D1", length(8.0), Some("scalar-stored")),
        ],
        &[("scalar-stored", ArchiveParameterClass::Length)],
        Some(two_places(UnitSymbol::M)),
    );
    let named = |feature: &str| {
        let owner = ParameterOwner::Feature(feature_named(&ir, feature).id.clone());
        parameters
            .iter()
            .find(|parameter| parameter.owner.as_ref() == Some(&owner))
            .unwrap_or_else(|| panic!("{feature}: {parameters:#?}"))
    };
    for feature in ["Zero", "Round", "Step"] {
        let parameter = named(feature);
        assert_eq!(
            parameter.kind,
            ValueKind::Quantity(Quantity::LENGTH),
            "{feature}"
        );
        assert_eq!(parameter.value, None, "{feature}");
        assert_eq!(expression_json(parameter)["kind"], "source", "{feature}");
    }
    assert_eq!(named("Round").display, Some(DimensionDisplay::Radius));
    for (feature, literal, millimetres) in [("Above", "0.02[m]", 20.0), ("Stored", "0.008[m]", 8.0)]
    {
        let parameter = named(feature);
        assert_eq!(
            expression_json(parameter)["value"]["text"],
            literal,
            "{feature}"
        );
        let Some(ParameterValue::Number(value)) = parameter.value else {
            panic!("{feature} has no value");
        };
        assert!(
            (value.get() - millimetres).abs() <= 1e-9 * millimetres,
            "{feature}"
        );
    }
    let unvalued = note_of(&notes, SldprtLossCode::DesignParameterValueUnread)
        .expect("the withheld displays are charged");
    assert!(
        unvalued.message.contains(
            ": 3 lengths displayed at most one rounding step from zero, at the 2 decimal places \
             of the part's length unit ("
        ),
        "{}",
        unvalued.message
    );
    for feature in ["Zero", "Round", "Step"] {
        assert!(
            unvalued.message.contains(named(feature).id.as_str()),
            "{feature}: {}",
            unvalued.message
        );
    }
    let rounded = note_of(&notes, SldprtLossCode::DesignParameterDisplayRounded)
        .expect("the displayed literal is charged");
    assert!(
        rounded.message.starts_with("1 parameters")
            && rounded
                .message
                .ends_with(&format!(": {}", named("Above").id.as_str())),
        "{}",
        rounded.message
    );
}
