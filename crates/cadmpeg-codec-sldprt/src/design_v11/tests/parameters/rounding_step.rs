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
    let (ir, parameters, notes) = parameters_with_scalars(
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
        &[],
        &[],
        Some(UnitSymbol::M),
        Some(2),
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
             of the part's length unit, a step coarser than the 1e-5 mm linear \
             resolution ("
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

#[test]
fn a_zero_display_whose_step_is_the_linear_resolution_is_exact() {
    use cadmpeg_ir::design::parameter::{ParameterOwner, ParameterValue};
    use cadmpeg_ir::design::units::UnitSymbol;
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::Length;

    let (ir, parameters, notes) = parameters_with_scalars(
        br#"<Keywords Name="Bracket">
            <Extrusion Name="Zero" Type="BossExtrude" id="7">
                <Dimension Name="D1">0</Dimension>
            </Extrusion>
        </Keywords>"#,
        &[("Zero", "D1", V10::Length(Length::new(0.0).unwrap()), None)],
        &[],
        &[],
        &[],
        Some(UnitSymbol::Mm),
        Some(5),
    );
    let owner = ParameterOwner::Feature(feature_named(&ir, "Zero").id.clone());
    let parameter = parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&owner))
        .unwrap();
    assert_eq!(expression_json(parameter)["value"]["text"], "0[mm]");
    let Some(ParameterValue::Number(value)) = parameter.value else {
        panic!("the zero display has no value: {parameter:#?}");
    };
    assert_eq!(value.get(), 0.0);
    assert!(note_of(&notes, SldprtLossCode::DesignParameterValueUnread).is_none());
}

#[test]
fn withholding_reads_the_fewest_places_and_a_coarse_display_reports_coarse() {
    use super::super::super::units::LengthPlaces;
    use cadmpeg_ir::design::parameter::{DesignParameter, ParameterOwner, ParameterValue};

    // One configuration shows lengths at 2 places, the other at 0. `1` is one
    // 1 mm step from zero at 0 places, so Step's display holds no value; at 2
    // places alone it is exact to 0.01 mm. Coarse's line gives 0.7 mm, which
    // `1` shows at its own places but not at 2, the largest: both reasons
    // describe Coarse, and the note names it as Coarse.
    let keywords = br#"<Keywords Name="Bracket">
            <Extrusion Name="Coarse" Type="BossExtrude" id="7">
                <Dimension Name="D1">1</Dimension>
            </Extrusion>
            <Extrusion Name="Step" Type="BossExtrude" id="8">
                <Dimension Name="D1">1</Dimension>
            </Extrusion>
        </Keywords>"#;
    let ir = part_with_history(keywords, &[]);
    let owned = |parameters: &[DesignParameter], feature: &str| {
        let owner = ParameterOwner::Feature(feature_named(&ir, feature).id.clone());
        parameters
            .iter()
            .find(|parameter| parameter.owner.as_ref() == Some(&owner))
            .unwrap_or_else(|| panic!("{feature}: {parameters:#?}"))
            .clone()
    };
    let manager = [("Coarse", "D1", 0.0007)];
    let coarse = "whose display rounds the length their equation gives at fewer decimal places \
                  than the document's length precision";

    let mixed = LengthPlaces {
        largest: 2,
        fewest: 0,
    };
    let (parameters, notes) = parameters_with_manager(keywords, &manager, true, &[], Some(mixed));
    let (coarse_parameter, step) = (owned(&parameters, "Coarse"), owned(&parameters, "Step"));
    assert_eq!(coarse_parameter.value, None);
    assert_eq!(step.value, None);
    let unvalued = note_of(&notes, SldprtLossCode::DesignParameterValueUnread)
        .expect("both displays are charged");
    assert!(
        unvalued.message.contains(&format!(
            "1 lengths displayed at most one rounding step from zero, at the 0 decimal places \
             of the part's length unit, a step coarser than the 1e-5 mm linear resolution ({})",
            step.id.as_str()
        )) && unvalued
            .message
            .contains(&format!("1 {coarse} ({})", coarse_parameter.id.as_str())),
        "{}",
        unvalued.message
    );

    // Every configuration at 2 places: `1` is no step from zero, so each
    // display is its literal, rounded, Coarse's too, and no dimension is
    // unvalued.
    let (parameters, notes) =
        parameters_with_manager(keywords, &manager, true, &[], Some(uniform(2)));
    for feature in ["Coarse", "Step"] {
        let Some(ParameterValue::Number(value)) = owned(&parameters, feature).value else {
            panic!("{feature} has no value: {parameters:#?}");
        };
        assert_eq!(value.get(), 1.0, "{feature}");
    }
    assert!(
        note_of(&notes, SldprtLossCode::DesignParameterValueUnread).is_none(),
        "{notes:#?}"
    );
}
