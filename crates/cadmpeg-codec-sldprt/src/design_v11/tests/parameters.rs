// SPDX-License-Identifier: Apache-2.0
//! Parameter decode tests of the v11 design of a part.
#![allow(clippy::unwrap_used)]

use super::*;

fn parameter_named<'a>(
    ir: &'a CadIr,
    name: &str,
) -> &'a cadmpeg_ir::design::parameter::DesignParameter {
    ir.design
        .parameters
        .iter()
        .find(|parameter| {
            parameter
                .name
                .as_ref()
                .map(cadmpeg_core::text::NonBlankString::as_str)
                == Some(name)
        })
        .unwrap()
}

#[test]
fn dimensions_and_global_variables_are_parameters_of_the_scope() {
    use cadmpeg_ir::design::parameter::{ParameterOwner, ParameterValue};
    use cadmpeg_ir::design::units::{Quantity, ValueKind};

    let ir = part_with_history(
        br#"<Keywords Name="Bracket">
            <Feature Name="Equations" Type="EquationDriven" id="16">
                <Dimension Name="wall">5</Dimension>
            </Feature>
            <Extrusion Name="Boss" Type="BossExtrude" id="7">
                <Dimension Name="D1">12.5mm</Dimension>
                <Dimension Name="D2" Value="10mm">"wall" * 2</Dimension>
                <Dimension Name="D3">0.5in</Dimension>
                <Dimension Name="D4">90deg</Dimension>
                <Dimension Name="D5">D1@Boss * 2</Dimension>
                <Dimension Name="D6">25</Dimension>
            </Extrusion>
        </Keywords>"#,
        &[],
    );
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one scope");
    };
    let boss = feature_named(&ir, "Boss").id.clone();
    assert!(ir
        .design
        .features
        .iter()
        .all(|feature| feature.name.as_deref() != Some("Equations")));
    for parameter in &ir.design.parameters {
        assert_eq!(parameter.scope, scope.definition);
    }

    let wall = parameter_named(&ir, "wall");
    assert_eq!(wall.owner, None);
    assert_eq!(wall.kind, ValueKind::Quantity(Quantity::REAL));
    assert_eq!(
        wall.value,
        Some(ParameterValue::Number(5.0.try_into().unwrap()))
    );
    assert_eq!(expression_json(wall)["value"]["text"], "5");

    let d1 = parameter_named(&ir, "D1");
    assert_eq!(d1.owner, Some(ParameterOwner::Feature(boss.clone())));
    assert_eq!(
        d1.id.as_str(),
        ir.model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "D1")
            .unwrap()
            .id
            .as_str()
    );
    assert_eq!(d1.kind, ValueKind::Quantity(Quantity::LENGTH));
    assert_eq!(
        d1.value,
        Some(ParameterValue::Number(12.5.try_into().unwrap()))
    );
    assert_eq!(expression_json(d1)["value"]["text"], "12.5[mm]");

    let d3 = parameter_named(&ir, "D3");
    assert_eq!(expression_json(d3)["value"]["text"], "0.5[in]");
    assert!(matches!(
        d3.value,
        Some(ParameterValue::Number(value)) if (value.get() - 12.7).abs() < 1.0e-9
    ));

    let d4 = parameter_named(&ir, "D4");
    assert_eq!(d4.kind, ValueKind::Quantity(Quantity::ANGLE));
    assert_eq!(expression_json(d4)["value"]["text"], "90[deg]");
    assert!(matches!(
        d4.value,
        Some(ParameterValue::Number(value))
            if (value.get() - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12
    ));

    let d2 = parameter_named(&ir, "D2");
    assert_eq!(d2.kind, ValueKind::Quantity(Quantity::LENGTH));
    assert_eq!(
        d2.value,
        Some(ParameterValue::Number(10.0.try_into().unwrap()))
    );
    let source = expression_json(d2);
    assert_eq!(source["kind"], "source");
    assert_eq!(source["value"]["dialect"]["kind"], "solid_works");
    let tokens = &source["value"]["tokens"];
    assert_eq!(tokens[0]["kind"], "reference");
    assert_eq!(tokens[0]["value"]["target"]["value"], wall.id.as_str());
    assert_eq!(tokens[1]["value"], " * 2");

    let d5 = parameter_named(&ir, "D5");
    let tokens = &expression_json(d5)["value"]["tokens"];
    assert_eq!(tokens[0]["value"]["target"]["value"], d1.id.as_str());
    assert_eq!(d5.value, None);

    let d6 = parameter_named(&ir, "D6");
    assert!(matches!(&d6.kind, ValueKind::Native(kind) if kind.as_str() == "sldprt:dimension"));
    assert_eq!(d6.value, None);
    assert_eq!(expression_json(d6)["value"]["tokens"][0]["value"], "25");
    assert_no_design_findings(&ir);
}

#[test]
fn a_bare_display_is_no_cache() {
    use cadmpeg_ir::design::units::{Quantity, ValueKind};

    let ir = part_with_history(
        br#"<Keywords Name="Bracket">
            <Extrusion Name="Boss" Type="BossExtrude" id="7">
                <Dimension Name="D1">25</Dimension>
            </Extrusion>
        </Keywords>"#,
        &[],
    );
    let v10 = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .unwrap();
    assert!(matches!(
        v10.value,
        Some(cadmpeg_ir::features::parameters::ParameterValue::Length(length)) if length.get() == 25.0
    ));
    let d1 = parameter_named(&ir, "D1");
    assert_eq!(d1.kind, ValueKind::Quantity(Quantity::LENGTH));
    assert_eq!(d1.value, None);
    assert_eq!(expression_json(d1)["kind"], "source");
    assert_eq!(expression_json(d1)["value"]["tokens"][0]["value"], "25");
    assert_no_design_findings(&ir);
}

/// The v11 parameters of a part whose decoder read each dimension named in
/// `readings` as the value and bound scalar given there, with the scalar
/// classes `classes`, and the notes of their reading.
fn parameters_with_readings(
    keywords: &[u8],
    readings: &[(
        &str,
        &str,
        cadmpeg_ir::features::parameters::ParameterValue,
        Option<&str>,
    )],
    classes: &[(&str, crate::records::ArchiveParameterClass)],
    length_unit: Option<cadmpeg_ir::design::units::UnitSymbol>,
) -> (
    CadIr,
    Vec<cadmpeg_ir::design::parameter::DesignParameter>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    parameters_with_scalars(keywords, readings, classes, &[], &[], length_unit)
}

/// The scalars a history feature holds under a dimension's name: feature
/// name, dimension name, and the scalar.
type NamedScalars<'a> = [(&'a str, &'a str, super::super::parameters::NamedScalar)];

/// As [`parameters_with_readings`], with what the records of the scalars
/// `stated` state of their parameters, and the scalars `named` the features
/// hold under their dimensions' names.
fn parameters_with_scalars(
    keywords: &[u8],
    readings: &[(
        &str,
        &str,
        cadmpeg_ir::features::parameters::ParameterValue,
        Option<&str>,
    )],
    classes: &[(&str, crate::records::ArchiveParameterClass)],
    stated: &[(&str, crate::resolved_features::parameter_classes::Statement)],
    named: &NamedScalars<'_>,
    length_unit: Option<cadmpeg_ir::design::units::UnitSymbol>,
) -> (
    CadIr,
    Vec<cadmpeg_ir::design::parameter::DesignParameter>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let ir = part_with_history(keywords, &[]);
    let native = crate::native::SldprtNative::load(ir.native.namespace("sldprt").unwrap()).unwrap();
    let mut v10 = ir.model.parameters.clone();
    for (feature, name, value, scalar) in readings {
        let owner = &ir
            .model
            .features
            .iter()
            .find(|candidate| candidate.name.as_deref() == Some(*feature))
            .unwrap()
            .id;
        let parameter = v10
            .iter_mut()
            .find(|parameter| parameter.owner.as_ref() == Some(owner) && parameter.name == *name)
            .unwrap();
        parameter.value = Some(value.clone());
        parameter.native_ref = scalar.map(str::to_owned);
    }
    let decoded = super::super::parameters::Decoded {
        parameters: v10
            .iter()
            .map(|parameter| (parameter.id.as_str(), parameter))
            .collect(),
        classes: classes
            .iter()
            .map(|(scalar, class)| (*scalar, Some(*class)))
            .collect(),
        stated: stated.iter().copied().collect(),
        named: std::collections::HashMap::new(),
    };
    let mut decoded = decoded;
    for (feature, name, scalar) in named {
        let history = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .find(|candidate| candidate.name == *feature)
            .unwrap();
        decoded
            .named
            .entry((history.id.as_str(), *name))
            .or_default()
            .push(*scalar);
    }
    let timeline = super::super::tree::timeline(&native.feature_histories).unwrap();
    let (parameters, notes) =
        super::super::parameters::Entries::new(&native.feature_histories, &timeline)
            .unwrap()
            .parameters(
                &ir.design.scopes[0].definition,
                &super::super::parameters::Readings {
                    decoded,
                    length_unit,
                    bound: std::collections::HashMap::new(),
                    measured: std::collections::HashMap::new(),
                    driven: std::collections::HashSet::new(),
                },
            );
    (ir, parameters, notes)
}

/// The note of `code` among `notes`.
fn note_of(
    notes: &[cadmpeg_ir::report::loss::LossNote],
    code: crate::loss::SldprtLossCode,
) -> Option<&cadmpeg_ir::report::loss::LossNote> {
    notes.iter().find(|note| note.code == code.kind())
}

#[test]
fn a_dimension_caches_the_stored_value_not_its_display() {
    use crate::records::ArchiveParameterClass;
    use cadmpeg_ir::design::parameter::{DimensionDisplay, ParameterValue};
    use cadmpeg_ir::design::units::{Quantity, ValueKind};
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::Length;

    let length = |millimetres: f64| V10::Length(Length::new(millimetres).unwrap());
    let (ir, parameters, notes) = parameters_with_readings(
        br#"<Keywords Name="Bracket">
            <Extrusion Name="Inch" Type="BossExtrude" id="7">
                <Dimension Name="D1">1</Dimension>
            </Extrusion>
            <Extrusion Name="Centimetre" Type="BossExtrude" id="8">
                <Dimension Name="D1">1.5</Dimension>
            </Extrusion>
            <Extrusion Name="Rounded" Type="BossExtrude" id="9">
                <Dimension Name="D1">14.39</Dimension>
            </Extrusion>
            <Extrusion Name="Display" Type="BossExtrude" id="10">
                <Dimension Name="D1">12</Dimension>
            </Extrusion>
            <Fillet Name="Round" id="11">
                <Dimension Name="D1">R0.25</Dimension>
            </Fillet>
            <Feature Name="Pattern" Type="LPattern" id="12">
                <Dimension Name="D1">4</Dimension>
                <Dimension Name="D2">2.5</Dimension>
            </Feature>
        </Keywords>"#,
        &[
            ("Inch", "D1", length(25.4), None),
            ("Centimetre", "D1", length(15.0), None),
            ("Rounded", "D1", length(365.4552), Some("scalar-rounded")),
            ("Display", "D1", length(12.0), None),
            ("Round", "D1", length(6.35), None),
            ("Pattern", "D1", V10::Integer(4), Some("scalar-count")),
            ("Pattern", "D2", V10::Real(2.5.try_into().unwrap()), None),
        ],
        &[
            ("scalar-rounded", ArchiveParameterClass::Length),
            ("scalar-count", ArchiveParameterClass::Integer),
        ],
        None,
    );
    let named = |feature: &str, name: &str| {
        let owner = cadmpeg_ir::design::parameter::ParameterOwner::Feature(
            feature_named(&ir, feature).id.clone(),
        );
        parameters
            .iter()
            .find(|parameter| {
                parameter.owner.as_ref() == Some(&owner)
                    && parameter
                        .name
                        .as_ref()
                        .map(cadmpeg_core::text::NonBlankString::as_str)
                        == Some(name)
            })
            .unwrap_or_else(|| panic!("{feature} {name}: {parameters:#?}"))
    };
    for (feature, millimetres) in [
        ("Inch", Some(25.4)),
        ("Centimetre", Some(15.0)),
        ("Rounded", Some(365.4552)),
        ("Display", None),
        ("Round", Some(6.35)),
    ] {
        let parameter = named(feature, "D1");
        assert_eq!(
            parameter.kind,
            ValueKind::Quantity(Quantity::LENGTH),
            "{feature}"
        );
        let value = match parameter.value {
            Some(ParameterValue::Number(value)) => Some(value.get()),
            _ => None,
        };
        assert_eq!(value, millimetres, "{feature}");
        assert_eq!(expression_json(parameter)["kind"], "source", "{feature}");
    }
    assert_eq!(named("Round", "D1").display, Some(DimensionDisplay::Radius));
    let count = named("Pattern", "D1");
    assert_eq!(count.kind, ValueKind::Integer);
    assert_eq!(count.value, Some(ParameterValue::Integer(4)));
    let unread = named("Pattern", "D2");
    assert!(matches!(&unread.kind, ValueKind::Native(kind) if kind.as_str() == "sldprt:dimension"));
    assert_eq!(unread.value, None);
    assert!(
        note_of(&notes, SldprtLossCode::DesignParameterDisplayRounded).is_none(),
        "{notes:?}"
    );
    let unvalued = note_of(&notes, SldprtLossCode::DesignParameterValueUnread)
        .expect("the dimensions with no value are named");
    assert_eq!(
        unvalued.message,
        format!(
            "Parameters whose dimension text is a bare number hold no value: 1 whose feature \
             holds no scalar under the dimension's name ({}); 1 lengths in a part whose length \
             unit is not read ({})",
            unread.id.as_str(),
            named("Display", "D1").id.as_str()
        )
    );
}

#[test]
fn an_unread_kind_keeps_the_number_its_bound_scalar_stores() {
    use cadmpeg_ir::design::parameter::ParameterValue;
    use cadmpeg_ir::design::units::{UnitSymbol, ValueKind};
    use cadmpeg_ir::features::parameters::ParameterValue as V10;

    let (_, parameters, notes) = parameters_with_readings(
        br#"<Keywords Name="Bracket">
            <Feature Name="Pattern" Type="LPattern" id="12">
                <Dimension Name="D2">12.7</Dimension>
                <Dimension Name="D4">3</Dimension>
                <Dimension Name="D5">7</Dimension>
            </Feature>
        </Keywords>"#,
        &[
            (
                "Pattern",
                "D2",
                V10::Real(0.0127.try_into().unwrap()),
                Some("scalar-spacing"),
            ),
            ("Pattern", "D4", V10::Integer(3), Some("scalar-count")),
            ("Pattern", "D5", V10::Integer(7), None),
        ],
        &[],
        Some(UnitSymbol::Mm),
    );
    let named = |name: &str| {
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
    };
    for (name, text, value) in [
        (
            "D2",
            "12.7",
            Some(ParameterValue::Number(0.0127.try_into().unwrap())),
        ),
        ("D4", "3", Some(ParameterValue::Integer(3))),
        ("D5", "7", None),
    ] {
        let parameter = named(name);
        assert!(
            matches!(&parameter.kind, ValueKind::Native(kind) if kind.as_str() == "sldprt:dimension"),
            "{name}"
        );
        assert_eq!(parameter.value, value, "{name}");
        let expression = expression_json(parameter);
        assert_eq!(expression["kind"], "source", "{name}");
        assert_eq!(expression["value"]["tokens"][0]["value"], text, "{name}");
    }
    let unvalued = note_of(&notes, SldprtLossCode::DesignParameterValueUnread)
        .expect("the dimension with no value is named");
    assert!(
        unvalued.message.ends_with(&format!(
            ": 1 whose feature holds no scalar under the dimension's name ({})",
            named("D5").id.as_str()
        )),
        "{}",
        unvalued.message
    );
}

#[test]
fn a_length_display_is_a_literal_in_the_part_length_unit() {
    use crate::records::ArchiveParameterClass;
    use cadmpeg_ir::design::parameter::{DimensionDisplay, ParameterValue};
    use cadmpeg_ir::design::units::{Quantity, UnitSymbol, ValueKind};
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::Length;

    let length = |millimetres: f64| V10::Length(Length::new(millimetres).unwrap());
    let (ir, parameters, notes) = parameters_with_readings(
        br#"<Keywords Name="Bracket">
            <Extrusion Name="Stored" Type="BossExtrude" id="7">
                <Dimension Name="D1">1</Dimension>
            </Extrusion>
            <Extrusion Name="Rounded" Type="BossExtrude" id="9">
                <Dimension Name="D1">14.39</Dimension>
            </Extrusion>
            <Extrusion Name="Display" Type="BossExtrude" id="10">
                <Dimension Name="D1">12</Dimension>
            </Extrusion>
            <Fillet Name="Round" id="11">
                <Dimension Name="D1">R0.25</Dimension>
            </Fillet>
            <Feature Name="Pattern" Type="LPattern" id="12">
                <Dimension Name="D1">4</Dimension>
            </Feature>
        </Keywords>"#,
        &[
            ("Stored", "D1", length(25.4), None),
            ("Rounded", "D1", length(365.4552), Some("scalar-rounded")),
            ("Display", "D1", length(12.0), None),
            ("Round", "D1", length(6.35), None),
            ("Pattern", "D1", V10::Integer(4), Some("scalar-count")),
        ],
        &[
            ("scalar-rounded", ArchiveParameterClass::Length),
            ("scalar-count", ArchiveParameterClass::Integer),
        ],
        Some(UnitSymbol::In),
    );
    let named = |feature: &str| {
        let owner = cadmpeg_ir::design::parameter::ParameterOwner::Feature(
            feature_named(&ir, feature).id.clone(),
        );
        parameters
            .iter()
            .find(|parameter| parameter.owner.as_ref() == Some(&owner))
            .unwrap_or_else(|| panic!("{feature}: {parameters:#?}"))
    };
    for (feature, literal, millimetres) in [
        ("Stored", "1[in]", 25.4),
        ("Rounded", "14.388[in]", 365.4552),
        ("Display", "12[in]", 12.0 * 25.4),
        ("Round", "0.25[in]", 6.35),
    ] {
        let parameter = named(feature);
        assert_eq!(
            parameter.kind,
            ValueKind::Quantity(Quantity::LENGTH),
            "{feature}"
        );
        let expression = expression_json(parameter);
        assert_eq!(expression["kind"], "neutral", "{feature}");
        assert_eq!(expression["value"]["text"], literal, "{feature}");
        let Some(ParameterValue::Number(value)) = parameter.value else {
            panic!("{feature} has no value");
        };
        assert!(
            (value.get() - millimetres).abs() <= 1e-9 * millimetres,
            "{feature}"
        );
    }
    assert_eq!(named("Round").display, Some(DimensionDisplay::Radius));
    assert_eq!(expression_json(named("Pattern"))["kind"], "source");
    let rounded = note_of(&notes, SldprtLossCode::DesignParameterDisplayRounded)
        .expect("the displayed literal is charged");
    assert_eq!(
        rounded.code,
        crate::loss::SldprtLossCode::DesignParameterDisplayRounded.kind()
    );
    assert!(
        rounded.message.starts_with("1 parameters")
            && rounded
                .message
                .ends_with(&format!(": {}", named("Display").id.as_str())),
        "{}",
        rounded.message
    );
}

#[test]
fn a_bare_display_takes_the_class_and_value_its_scalar_record_states() {
    use super::super::parameters::NamedScalar;
    use crate::resolved_features::parameter_classes::{StatedParameter, Statement};
    use cadmpeg_ir::design::parameter::ParameterValue;
    use cadmpeg_ir::design::units::{Quantity, UnitSymbol, ValueKind};
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::Angle;

    let keywords = br#"<Keywords Name="Bracket">
            <Feature Name="Sketch1" Type="ProfileFeature" id="7">
                <Dimension Name="D1">17</Dimension>
                <Dimension Name="D2">25</Dimension>
                <Dimension Name="D3">6</Dimension>
                <Dimension Name="D4">30</Dimension>
                <Dimension Name="D5">12</Dimension>
                <Dimension Name="D6">30</Dimension>
                <Dimension Name="D7">8</Dimension>
                <Dimension Name="D8">240</Dimension>
                <Dimension Name="D9">5</Dimension>
            </Feature>
        </Keywords>"#;
    let driving = |stated| NamedScalar {
        stated: Some(Statement::Parameter(stated)),
        driving: true,
    };
    let shown = |stated| NamedScalar {
        stated: Some(Statement::Parameter(stated)),
        driving: false,
    };
    let named = [
        ("Sketch1", "D1", shown(StatedParameter::Length(0.009_156))),
        ("Sketch1", "D1", driving(StatedParameter::Length(0.017))),
        ("Sketch1", "D2", driving(StatedParameter::Length(0.026))),
        ("Sketch1", "D3", shown(StatedParameter::Count(6))),
        (
            "Sketch1",
            "D4",
            NamedScalar {
                stated: None,
                driving: true,
            },
        ),
        (
            "Sketch1",
            "D6",
            driving(StatedParameter::Angle(30f64.to_radians())),
        ),
        ("Sketch1", "D7", shown(StatedParameter::Length(0.008))),
        (
            "Sketch1",
            "D9",
            NamedScalar {
                stated: Some(Statement::Contested),
                driving: true,
            },
        ),
    ];
    let mistyped = [(
        "Sketch1",
        "D8",
        V10::Angle(Angle::from_assigned_real(0.24.try_into().unwrap())),
        Some("scalar-d8"),
    )];
    let stated = [(
        "scalar-d8",
        Statement::Parameter(StatedParameter::Length(0.24)),
    )];
    let read = |unit| parameters_with_scalars(keywords, &mistyped, &[], &stated, &named, unit);
    let named_in = |parameters: &[cadmpeg_ir::design::parameter::DesignParameter], name: &str| {
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
    let number = |parameter: &cadmpeg_ir::design::parameter::DesignParameter| match parameter.value
    {
        Some(ParameterValue::Number(value)) => Some(value.get()),
        _ => None,
    };
    let unread = |parameter: &cadmpeg_ir::design::parameter::DesignParameter| {
        matches!(&parameter.kind, ValueKind::Native(kind) if kind.as_str() == "sldprt:dimension")
            && parameter.value.is_none()
    };

    let (_, parameters, notes) = read(Some(UnitSymbol::Mm));
    let d1 = named_in(&parameters, "D1");
    assert_eq!(d1.kind, ValueKind::Quantity(Quantity::LENGTH));
    assert!((number(&d1).unwrap() - 17.0).abs() <= 1e-9);
    assert_eq!(expression_json(&d1)["value"]["text"], "17[mm]");
    let d3 = named_in(&parameters, "D3");
    assert_eq!(d3.kind, ValueKind::Integer);
    assert_eq!(d3.value, Some(ParameterValue::Integer(6)));
    let d6 = named_in(&parameters, "D6");
    assert_eq!(d6.kind, ValueKind::Quantity(Quantity::ANGLE));
    assert!((number(&d6).unwrap() - 30f64.to_radians()).abs() <= 1e-12);
    let d7 = named_in(&parameters, "D7");
    assert_eq!(d7.kind, ValueKind::Quantity(Quantity::LENGTH));
    assert!((number(&d7).unwrap() - 8.0).abs() <= 1e-9);
    let d8 = named_in(&parameters, "D8");
    assert_eq!(d8.kind, ValueKind::Quantity(Quantity::LENGTH));
    assert!((number(&d8).unwrap() - 240.0).abs() <= 1e-9);
    for name in ["D2", "D4", "D5", "D9"] {
        assert!(unread(&named_in(&parameters, name)), "{name}");
    }
    let note = note_of(
        &notes,
        crate::loss::SldprtLossCode::DesignParameterValueUnread,
    )
    .expect("the unread dimensions are named");
    for (name, why) in [
        (
            "D2",
            "whose stored values differ from the display or from each other",
        ),
        (
            "D4",
            "whose scalars' records state no parameter class the decoder reads",
        ),
        (
            "D5",
            "whose feature holds no scalar under the dimension's name",
        ),
        (
            "D9",
            "whose scalars' parameter classes the decoder's class sources name differently",
        ),
    ] {
        let id = named_in(&parameters, name).id;
        assert!(
            note.message.contains(&format!("1 {why} ({})", id.as_str())),
            "{name}: {}",
            note.message
        );
    }

    let (_, parameters, notes) = read(None);
    let d1 = named_in(&parameters, "D1");
    assert_eq!(d1.kind, ValueKind::Quantity(Quantity::LENGTH));
    assert!((number(&d1).unwrap() - 17.0).abs() <= 1e-9);
    assert_eq!(expression_json(&d1)["kind"], "source");
    let d2 = named_in(&parameters, "D2");
    assert!((number(&d2).unwrap() - 26.0).abs() <= 1e-9);
    assert!(unread(&named_in(&parameters, "D7")));
    let note = note_of(
        &notes,
        crate::loss::SldprtLossCode::DesignParameterValueUnread,
    )
    .unwrap();
    assert!(
        note.message.contains(&format!(
            "1 lengths in a part whose length unit is not read ({})",
            named_in(&parameters, "D7").id.as_str()
        )),
        "{}",
        note.message
    );
}

#[test]
fn a_contested_bound_scalar_gives_no_value() {
    use crate::records::ArchiveParameterClass;
    use crate::resolved_features::parameter_classes::Statement;
    use cadmpeg_ir::design::units::{UnitSymbol, ValueKind};
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::{Angle, Length};

    // D1's earlier reading is an angle and D2's archive walk types its
    // scalar a length; the class token of each bound scalar is contested.
    let (_, parameters, notes) = parameters_with_scalars(
        br#"<Keywords Name="Bracket">
            <Feature Name="Sketch1" Type="ProfileFeature" id="7">
                <Dimension Name="D1">7</Dimension>
                <Dimension Name="D2">12</Dimension>
            </Feature>
        </Keywords>"#,
        &[
            (
                "Sketch1",
                "D1",
                V10::Angle(Angle::from_assigned_real(0.007.try_into().unwrap())),
                Some("scalar-d1"),
            ),
            (
                "Sketch1",
                "D2",
                V10::Length(Length::new(12.0).unwrap()),
                Some("scalar-d2"),
            ),
        ],
        &[("scalar-d2", ArchiveParameterClass::Length)],
        &[
            ("scalar-d1", Statement::Contested),
            ("scalar-d2", Statement::Contested),
        ],
        &[],
        Some(UnitSymbol::Mm),
    );
    let named = |name: &str| {
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
    };
    for name in ["D1", "D2"] {
        let parameter = named(name);
        assert!(
            matches!(&parameter.kind, ValueKind::Native(kind) if kind.as_str() == "sldprt:dimension"),
            "{name}: {parameter:#?}"
        );
        assert_eq!(parameter.value, None, "{name}");
    }
    let unvalued = note_of(&notes, SldprtLossCode::DesignParameterValueUnread)
        .expect("the dimensions with no value are named");
    assert!(
        unvalued.message.ends_with(&format!(
            ": 2 whose scalars' parameter classes the decoder's class sources name differently \
             ({}, {})",
            named("D1").id.as_str(),
            named("D2").id.as_str()
        )),
        "{}",
        unvalued.message
    );
}

#[test]
fn an_archive_class_that_differs_from_the_stated_class_gives_no_value() {
    use crate::records::ArchiveParameterClass;
    use crate::resolved_features::parameter_classes::{StatedParameter, Statement};
    use cadmpeg_ir::design::units::{Quantity, UnitSymbol, ValueKind};
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::Length;

    // The archive walk types both bound scalars as lengths. D1's record
    // states an angle; D2's states a length.
    let (_, parameters, notes) = parameters_with_scalars(
        br#"<Keywords Name="Bracket">
            <Feature Name="Sketch1" Type="ProfileFeature" id="7">
                <Dimension Name="D1">7</Dimension>
                <Dimension Name="D2">12</Dimension>
            </Feature>
        </Keywords>"#,
        &[
            (
                "Sketch1",
                "D1",
                V10::Length(Length::new(7.0).unwrap()),
                Some("scalar-d1"),
            ),
            (
                "Sketch1",
                "D2",
                V10::Length(Length::new(12.0).unwrap()),
                Some("scalar-d2"),
            ),
        ],
        &[
            ("scalar-d1", ArchiveParameterClass::Length),
            ("scalar-d2", ArchiveParameterClass::Length),
        ],
        &[
            (
                "scalar-d1",
                Statement::Parameter(StatedParameter::Angle(7f64.to_radians())),
            ),
            (
                "scalar-d2",
                Statement::Parameter(StatedParameter::Length(0.012)),
            ),
        ],
        &[],
        Some(UnitSymbol::Mm),
    );
    let named = |name: &str| {
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
    };
    let d1 = named("D1");
    assert!(
        matches!(&d1.kind, ValueKind::Native(kind) if kind.as_str() == "sldprt:dimension"),
        "{d1:#?}"
    );
    assert_eq!(d1.value, None);
    assert_eq!(named("D2").kind, ValueKind::Quantity(Quantity::LENGTH));
    assert!(named("D2").value.is_some());
    let unvalued = note_of(&notes, SldprtLossCode::DesignParameterValueUnread)
        .expect("the dimension with no value is named");
    assert!(
        unvalued.message.ends_with(&format!(
            ": 1 whose scalars' parameter classes the decoder's class sources name differently \
             ({})",
            d1.id.as_str()
        )),
        "{}",
        unvalued.message
    );
}

#[test]
fn a_bare_display_number_states_no_unit() {
    use super::super::parameters::bare_display_number;

    assert_eq!(bare_display_number("14.39"), Some(14.39));
    assert_eq!(bare_display_number("R0.25"), Some(0.25));
    assert_eq!(bare_display_number("<MOD-DIAM>3.7(72t gear)"), Some(3.7));
    assert_eq!(bare_display_number("&lt;MOD-DIAM&gt;12"), Some(12.0));
    assert_eq!(bare_display_number("4X <MOD-DIAM>6"), Some(6.0));
    assert_eq!(bare_display_number("12.5mm"), None);
    assert_eq!(bare_display_number("<MOD-DIAM>12.7mm"), None);
    assert_eq!(bare_display_number("\"wall\" * 2"), None);
    assert_eq!(bare_display_number("17/32\""), None);
}
