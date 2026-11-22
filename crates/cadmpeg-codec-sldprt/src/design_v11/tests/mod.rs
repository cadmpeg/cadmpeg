// SPDX-License-Identifier: Apache-2.0
//! Decode tests of the v11 design of a part.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::design::container::{Container, Folder, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::names::{
    CodecId, NameEntry, NameKind, NativeName, NativeNameValue, TopoName,
};
use cadmpeg_ir::design::ops::datum::DatumElement;
use cadmpeg_ir::design::refs::{EntityRef, Member, Position};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::check::Check;

use crate::test_support::container::{make_block, native_envelope_fixture, sldprt_with_body};
use crate::test_support::history::resolved_feature_classes_with_ids;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

const ROOT: &str = "sldprt:model:product-definition#root";

fn decode(source: &[u8]) -> CadIr {
    SldprtCodec
        .decode(
            &mut Cursor::new(native_envelope_fixture(source)),
            &DecodeOptions::default(),
        )
        .unwrap()
        .ir()
        .clone()
}

fn part_with_history(keywords: &[u8], classes: &[(&str, &str, u32)]) -> CadIr {
    decode(&with_history(
        sldprt_with_body(&triangle_body()),
        keywords,
        classes,
    ))
}

fn with_history(mut source: Vec<u8>, keywords: &[u8], classes: &[(&str, &str, u32)]) -> Vec<u8> {
    source.extend(make_block(0x42, "Contents/Keywords", keywords));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(classes),
    ));
    source
}

fn body_ids(ir: &CadIr) -> Vec<String> {
    ir.geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str().to_owned())
        .collect()
}

fn feature_named<'a>(ir: &'a CadIr, name: &str) -> &'a cadmpeg_ir::design::feature::Feature {
    ir.design
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap()
}

fn assert_no_design_findings(ir: &CadIr) {
    let report = cadmpeg_ir::validate_neutral(ir, Vec::new()).unwrap();
    let design = report
        .findings
        .iter()
        .filter(|finding| finding.check == Check::Design)
        .collect::<Vec<_>>();
    assert!(design.is_empty(), "{design:#?}");
}

const BRACKET: &[u8] = br#"<Keywords Name="Bracket">
            <Feature Name="Annotations" Type="Annotations" id="101"/>
            <Sketch Name="Origin" Type="OriginProfileFeature" id="106"/>
            <Feature Name="Group" Type="FtrFolder" id="110">
                <Extrusion Name="Boss" Type="BossExtrude" id="7">
                    <Dimension Name="D1">12.5mm</Dimension>
                    <Sketch Name="Sketch1" Type="ProfileFeature" id="8"/>
                </Extrusion>
            </Feature>
            <Fillet Name="Fillet1" id="9" Suppressed="1"/>
            <Feature Name="Odd" Type="CustomOperation" id="11"/>
        </Keywords>"#;

const BRACKET_CLASSES: &[(&str, &str, u32)] = &[
    ("moDetailCabinet_c", "Annotations", 101),
    ("moOriginProfileFeature_c", "Origin", 106),
    ("moFtrFolder_c", "Group", 110),
];

#[test]
fn feature_tree_is_one_timeline_of_the_root_definition() {
    let ir = part_with_history(BRACKET, BRACKET_CLASSES);
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one scope: {:#?}", ir.design.scopes);
    };
    assert_eq!(scope.definition.as_str(), ROOT);
    let [definition] = ir.model.product_definitions.as_slice() else {
        panic!("one root definition");
    };
    assert_eq!(definition.id.as_str(), ROOT);
    let bodies = body_ids(&ir);
    assert!(!bodies.is_empty());
    assert_eq!(
        definition
            .bodies
            .iter()
            .map(|body| body.as_str().to_owned())
            .collect::<Vec<_>>(),
        bodies
    );

    let [Container::Ordered {
        id,
        kind: OrderedKind::Timeline,
        members,
        folders,
        rollback: Rollback::End,
        placement: None,
    }] = ir.design.containers.as_slice()
    else {
        panic!("one timeline: {:#?}", ir.design.containers);
    };
    assert_eq!(scope.roots, vec![id.clone()]);
    let names = members
        .iter()
        .map(|member| {
            let Member::Feature(feature) = member else {
                panic!("feature member");
            };
            ir.design
                .features
                .iter()
                .find(|candidate| &candidate.id == feature)
                .and_then(|feature| feature.name.clone())
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(names, ["Origin", "Sketch1", "Boss", "Fillet1", "Odd"]);
    assert_eq!(
        folders,
        &[Folder {
            name: Some("Group".to_owned()),
            first: 1,
            last: 2,
        }]
    );

    let family = |name: &str| match &feature_named(&ir, name).definition.operation {
        FeatureOperation::Unresolved { family } => Some(*family),
        _ => None,
    };
    assert!(matches!(
        &feature_named(&ir, "Origin").definition.operation,
        FeatureOperation::Datum(datum) if matches!(&datum.element, DatumElement::Points(points) if points.len() == 1)
    ));
    assert_eq!(family("Sketch1"), Some(OperationFamily::Sketch));
    assert_eq!(family("Boss"), Some(OperationFamily::Extrude));
    assert_eq!(family("Fillet1"), Some(OperationFamily::Fillet));
    assert!(matches!(
        &feature_named(&ir, "Odd").definition.operation,
        FeatureOperation::Native { kind, .. } if kind.as_str() == "sldprt:CustomOperation"
    ));
    assert_eq!(
        feature_named(&ir, "Fillet1").suppressed,
        Some(Dimension::Literal(true))
    );
    assert_eq!(
        feature_named(&ir, "Boss").suppressed,
        Some(Dimension::Literal(false))
    );
    assert_eq!(
        feature_named(&ir, "Boss").id.as_str(),
        ir.model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Boss"))
            .unwrap()
            .id
            .as_str()
    );

    let [current] = scope.current.as_slice() else {
        panic!("one current state");
    };
    assert_eq!(
        current.at,
        Position::After(Member::Feature(feature_named(&ir, "Odd").id.clone()))
    );
    assert_eq!(
        current
            .bodies
            .iter()
            .map(|body| body.as_str().to_owned())
            .collect::<Vec<_>>(),
        bodies
    );
    assert_no_design_findings(&ir);
}

#[test]
fn part_without_a_history_states_no_design() {
    let ir = decode(&sldprt_with_body(&triangle_body()));
    assert!(!ir.geometry.bodies.is_empty());
    assert!(ir.design.scopes.is_empty());
    assert!(ir.design.containers.is_empty());
    assert!(ir.design.features.is_empty());
    let [root] = ir.model.product_definitions.as_slice() else {
        panic!("one root definition");
    };
    assert!(root.is_root());
    assert_eq!(
        root.bodies,
        ir.geometry
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>()
    );
}

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

/// The canonical text of a parameter's neutral expression, or its source
/// tokens' JSON.
fn expression_json(
    parameter: &cadmpeg_ir::design::parameter::DesignParameter,
) -> serde_json::Value {
    serde_json::to_value(&parameter.definition).unwrap()["value"].clone()
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
/// classes `classes`.
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
    let decoded = super::parameters::Decoded {
        parameters: v10
            .iter()
            .map(|parameter| (parameter.id.as_str(), parameter))
            .collect(),
        classes: classes
            .iter()
            .map(|(scalar, class)| (*scalar, Some(*class)))
            .collect(),
    };
    let timeline = super::tree::timeline(&native.feature_histories).unwrap();
    let (parameters, rounded) =
        super::parameters::Entries::new(&native.feature_histories, &timeline)
            .unwrap()
            .parameters(
                &ir.design.scopes[0].definition,
                &super::parameters::Readings {
                    decoded,
                    length_unit,
                    bound: std::collections::HashMap::new(),
                    measured: std::collections::HashMap::new(),
                    driven: std::collections::HashSet::new(),
                },
            );
    (ir, parameters, rounded)
}

#[test]
fn a_dimension_caches_the_stored_value_not_its_display() {
    use crate::records::ArchiveParameterClass;
    use cadmpeg_ir::design::parameter::{DimensionDisplay, ParameterValue};
    use cadmpeg_ir::design::units::{Quantity, ValueKind};
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::Length;

    let length = |millimetres: f64| V10::Length(Length::new(millimetres).unwrap());
    let (ir, parameters, rounded) = parameters_with_readings(
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
    assert!(rounded.is_empty(), "{rounded:?}");
}

#[test]
fn a_length_display_is_a_literal_in_the_part_length_unit() {
    use crate::records::ArchiveParameterClass;
    use cadmpeg_ir::design::parameter::{DimensionDisplay, ParameterValue};
    use cadmpeg_ir::design::units::{Quantity, UnitSymbol, ValueKind};
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::Length;

    let length = |millimetres: f64| V10::Length(Length::new(millimetres).unwrap());
    let (ir, parameters, rounded) = parameters_with_readings(
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
    let [rounded] = rounded.as_slice() else {
        panic!("the displayed literal is charged: {rounded:?}");
    };
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

/// The neutral expression `text` over `targets`, as a parameter definition.
fn neutral_definition(
    text: &str,
    targets: &[&cadmpeg_ir::features::ParameterId],
) -> cadmpeg_ir::design::parameter::ParameterDefinition {
    use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
    use cadmpeg_ir::design::expr::{parse, Expression, Neutral};

    let references = targets
        .iter()
        .map(|target| ParameterRef {
            target: RefTarget::Parameter((*target).clone()),
            through: None,
        })
        .collect::<Vec<_>>();
    cadmpeg_ir::design::parameter::ParameterDefinition::Expression(Expression::Neutral(Neutral(
        parse(text, &references, &[]).unwrap(),
    )))
}

#[test]
fn a_bare_reference_of_the_same_kind_is_a_neutral_link() {
    let ir = part_with_history(
        br#"<Keywords Name="Bracket">
            <Feature Name="Equations" Type="EquationDriven" id="16">
                <Dimension Name="wall">5</Dimension>
                <Dimension Name="rib">"wall"</Dimension>
            </Feature>
            <Extrusion Name="Boss" Type="BossExtrude" id="7">
                <Dimension Name="D1">12.5mm</Dimension>
                <Dimension Name="D2" Value="12.5mm">"D1@Boss"</Dimension>
                <Dimension Name="D3" Value="12.5mm"> D1 </Dimension>
            </Extrusion>
        </Keywords>"#,
        &[],
    );
    let d1 = parameter_named(&ir, "D1").id.clone();
    for name in ["D2", "D3"] {
        let parameter = parameter_named(&ir, name);
        assert_eq!(
            parameter.definition,
            neutral_definition("$0", &[&d1]),
            "{name}"
        );
    }
    let wall = parameter_named(&ir, "wall").id.clone();
    assert_eq!(
        parameter_named(&ir, "rib").definition,
        neutral_definition("$0", &[&wall])
    );
    assert_no_design_findings(&ir);
}

#[test]
fn a_reference_to_a_parameter_of_another_kind_is_charged_text() {
    use cadmpeg_ir::design::expr::{Expression, ExpressionDialect, ExpressionToken};
    use cadmpeg_ir::design::parameter::ParameterDefinition;
    use cadmpeg_ir::design::units::{Quantity, UnitSymbol, ValueKind};
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::{Angle, Length};

    let (_, parameters, losses) = parameters_with_readings(
        br#"<Keywords Name="Bracket">
            <Feature Name="Equations" Type="EquationDriven" id="16">
                <Dimension Name="wall">5</Dimension>
            </Feature>
            <Extrusion Name="Boss" Type="BossExtrude" id="7">
                <Dimension Name="D1">"wall"</Dimension>
                <Dimension Name="D2">"wall"</Dimension>
                <Dimension Name="D3">"D1@Boss"</Dimension>
                <Dimension Name="D4"> "D1@Boss" </Dimension>
            </Extrusion>
        </Keywords>"#,
        &[
            ("Boss", "D1", V10::Length(Length::new(5.0).unwrap()), None),
            ("Boss", "D2", V10::Angle(Angle::new(0.1).unwrap()), None),
            ("Boss", "D3", V10::Angle(Angle::new(0.1).unwrap()), None),
            ("Boss", "D4", V10::Angle(Angle::new(0.1).unwrap()), None),
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
    let text = |text: &str| {
        ParameterDefinition::Expression(Expression::Source {
            dialect: ExpressionDialect::SolidWorks,
            tokens: vec![ExpressionToken::Text(text.to_owned())],
        })
    };
    for (name, kind, whole) in [
        ("D1", Quantity::LENGTH, "\"wall\""),
        ("D2", Quantity::ANGLE, "\"wall\""),
        ("D3", Quantity::ANGLE, "\"D1@Boss\""),
        ("D4", Quantity::ANGLE, "\"D1@Boss\""),
    ] {
        assert_eq!(named(name).kind, ValueKind::Quantity(kind), "{name}");
        assert_eq!(named(name).definition, text(whole), "{name}");
    }
    let [loss] = losses.as_slice() else {
        panic!("one note: {losses:?}");
    };
    assert_eq!(
        loss.code,
        crate::loss::SldprtLossCode::DesignParameterReferenceAsText.kind()
    );
    let ids = ["D1", "D2", "D3", "D4"].map(|name| named(name).id.as_str().to_owned());
    assert!(
        loss.message.starts_with("4 parameters")
            && loss.message.ends_with(&format!(": {}", ids.join(", "))),
        "{}",
        loss.message
    );
}

#[test]
fn a_bare_display_number_states_no_unit() {
    use super::parameters::bare_display_number;

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

#[test]
fn configurations_mark_the_active_one() {
    use cadmpeg_ir::design::configuration::MaterialRef;

    let ir = part_with_history(
        br#"<Keywords Name="Bracket">
            <Configuration Name="Default" SourceIndex="0" Material="Steel"/>
            <Configuration Name="Long" SourceIndex="1" Material="Aluminum"/>
            <Extrusion Name="Boss" Type="BossExtrude" id="7"/>
        </Keywords>"#,
        &[],
    );
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one scope");
    };
    let names = ir
        .design
        .configurations
        .iter()
        .map(|configuration| configuration.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Default", "Long"]);
    let active = ir
        .design
        .configurations
        .iter()
        .find(|configuration| configuration.name == "Default")
        .unwrap();
    assert_eq!(scope.active_configuration.as_ref(), Some(&active.id));
    assert!(active.materials.is_empty());
    assert_eq!(
        active.id.as_str(),
        ir.model
            .configurations
            .iter()
            .find(|configuration| configuration.active)
            .unwrap()
            .id
            .as_str()
    );
    let long = ir
        .design
        .configurations
        .iter()
        .find(|configuration| configuration.name == "Long")
        .unwrap();
    assert!(matches!(
        long.materials.as_slice(),
        [material] if material.target.is_none()
            && material.material == MaterialRef::Named(cadmpeg_core::nonblank_literal!("Aluminum"))
    ));
    assert_no_design_findings(&ir);
}

/// The triangle body whose face carries the `FACE_ID_2001` list `face` and
/// whose body carries the `ATOM_ID_2001` list `[body]`.
fn identified_triangle(face: &[u32], body: u32) -> Vec<u8> {
    use crate::test_support::parasolid::int_list_attribute;

    let mut bytes = triangle_body();
    bytes.extend(int_list_attribute(
        "FACE_ID_2001",
        &[9, 9, 1, 1],
        1100,
        10,
        face,
    ));
    bytes.extend(int_list_attribute(
        "ATOM_ID_2001",
        &[9, 1],
        1110,
        900,
        &[body],
    ));
    bytes
}

fn native_name(values: &[i64]) -> NativeName {
    NativeName {
        codec: CodecId::from(cadmpeg_core::nonblank_literal!("sldprt")),
        value: NativeNameValue::Integers(values.to_vec()),
    }
}

const BOSS: &[u8] = br#"<Keywords Name="Bracket">
    <Extrusion Name="Boss" Type="BossExtrude" id="7">
        <Sketch Name="Sketch1" Type="ProfileFeature" id="8"/>
    </Extrusion>
</Keywords>"#;

#[test]
fn faces_and_bodies_are_named_by_their_persistent_ids() {
    let ir = decode(&with_history(
        sldprt_with_body(&identified_triangle(&[72, 7, 0x6523_4178, 0, 3], 41)),
        BOSS,
        &[],
    ));
    let [face] = ir.geometry.faces.as_slice() else {
        panic!("one face");
    };
    let [body] = ir.geometry.bodies.as_slice() else {
        panic!("one body");
    };
    let entries = ir.design.names.entries();
    assert_eq!(entries.len(), 2, "{entries:#?}");
    assert_eq!(
        entries.get(&EntityRef::Face(face.id.clone())),
        Some(&NameEntry {
            name: Some(TopoName::Blend {
                feature: feature_named(&ir, "Boss").id.clone(),
                between: Vec::new(),
                index: Some(3),
            }),
            native: Some(native_name(&[72, 7, 0x6523_4178, 0, 3])),
        })
    );
    assert_eq!(
        entries.get(&EntityRef::Body(body.id.clone())),
        Some(&NameEntry {
            name: None,
            native: Some(native_name(&[41])),
        })
    );
    assert!(ir.design.evaluations.is_empty());
    assert_no_design_findings(&ir);
}

/// The name of the one face of a part whose face carries `face`.
fn face_name_of(face: &[u32]) -> (CadIr, Option<TopoName>) {
    let ir = decode(&with_history(
        sldprt_with_body(&identified_triangle(face, 41)),
        BOSS,
        &[],
    ));
    let entry = ir
        .design
        .names
        .entries()
        .get(&EntityRef::Face(ir.geometry.faces[0].id.clone()))
        .cloned();
    let entry = entry.unwrap_or_else(|| panic!("an entry for {face:?}"));
    assert_eq!(
        entry.native,
        Some(native_name(
            &face.iter().copied().map(i64::from).collect::<Vec<_>>()
        ))
    );
    assert_no_design_findings(&ir);
    (ir, entry.name)
}

#[test]
fn face_names_state_the_role_and_index_their_class_stores() {
    use cadmpeg_ir::design::names::GenerationRole;

    const STAMP: u32 = 0x6523_4178;
    let generated = |ir: &CadIr, role: Option<GenerationRole>, index: Option<u32>| {
        Some(TopoName::Generated {
            kind: NameKind::Face,
            feature: feature_named(ir, "Boss").id.clone(),
            role,
            generator: None,
            index,
        })
    };
    let other = |code: &str| {
        Some(GenerationRole::Other(
            cadmpeg_core::text::NonBlankString::new(code).unwrap(),
        ))
    };
    for (face, role, index) in [
        // A side swept from a sketch entity: the entity's id is an
        // allocation, not an index.
        (&[74, 7, STAMP, 0, 5][..], Some(GenerationRole::Side), None),
        (
            &[83, 7, STAMP, 0, 5, u32::MAX, 1][..],
            Some(GenerationRole::Side),
            None,
        ),
        // A cap: its end is the role, its region the index, zero included.
        (
            &[40, 7, STAMP, 0, 0, 0][..],
            Some(GenerationRole::Start),
            Some(0),
        ),
        (
            &[81, 7, STAMP, 0, 1, 4, 0][..],
            Some(GenerationRole::End),
            Some(4),
        ),
        // The four-word set names the producer and no role.
        (&[36, 7, STAMP, 0][..], None, None),
        // A class whose fields' meaning is not established.
        (&[49, 7, STAMP, 0, 2, 0][..], other("sldprt:49"), None),
        // A cap whose end is neither 0 nor 1 is no established cap.
        (&[40, 7, STAMP, 0, 2, 0][..], other("sldprt:40"), None),
    ] {
        let (ir, name) = face_name_of(face);
        assert_eq!(name, generated(&ir, role, index), "{face:?}");
    }
}

#[test]
fn a_face_whose_producer_is_no_feature_keeps_only_its_native_name() {
    let ir = decode(&with_history(
        sldprt_with_body(&identified_triangle(&[72, 99, 0x6523_4178, 0, 3], 41)),
        BOSS,
        &[],
    ));
    let face = &ir.geometry.faces[0];
    assert_eq!(
        ir.design
            .names
            .entries()
            .get(&EntityRef::Face(face.id.clone())),
        Some(&NameEntry {
            name: None,
            native: Some(native_name(&[72, 99, 0x6523_4178, 0, 3])),
        })
    );
    assert_no_design_findings(&ir);
}

#[test]
fn a_face_whose_producer_id_several_records_carry_keeps_only_its_native_name() {
    let ir = decode(&with_history(
        sldprt_with_body(&identified_triangle(&[72, 7, 0x6523_4178, 0, 3], 41)),
        br#"<Keywords Name="Bracket">
    <Extrusion Name="Boss" Type="BossExtrude" id="7">
        <Sketch Name="Sketch1" Type="ProfileFeature" id="7"/>
    </Extrusion>
</Keywords>"#,
        &[],
    ));
    let face = &ir.geometry.faces[0];
    assert_eq!(
        ir.design
            .names
            .entries()
            .get(&EntityRef::Face(face.id.clone())),
        Some(&NameEntry {
            name: None,
            native: Some(native_name(&[72, 7, 0x6523_4178, 0, 3])),
        })
    );
}

const TWO_CONFIGURATIONS: &[u8] = br#"<Keywords Name="Bracket">
    <Configuration Name="First" SourceIndex="0"/>
    <Configuration Name="Second" SourceIndex="1"/>
    <Extrusion Name="Boss" Type="BossExtrude" id="7"/>
    <Sketch Name="Sketch2" Type="ProfileFeature" id="12"/>
</Keywords>"#;

#[test]
fn a_configuration_that_is_not_active_has_its_stored_state() {
    use cadmpeg_ir::design::evaluation::{EvaluationStatus, StateBase};
    use cadmpeg_ir::design::refs::Evaluated;

    let mut source = with_history(
        crate::test_support::container::sldprt_with_colliding_sites(),
        TWO_CONFIGURATIONS,
        &[],
    );
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<swSolidWorks><swModel swConfigurationName="Second"/></swSolidWorks>"#,
    ));
    let ir = decode(&source);
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one scope");
    };
    let configuration = |name: &str| {
        ir.design
            .configurations
            .iter()
            .find(|configuration| configuration.name == name)
            .unwrap()
            .id
            .clone()
    };
    assert_eq!(scope.active_configuration, Some(configuration("Second")));
    let [current] = scope.current.as_slice() else {
        panic!("one current state");
    };
    let [evaluation] = ir.design.evaluations.as_slice() else {
        panic!("one evaluation: {:#?}", ir.design.evaluations);
    };
    assert_eq!(evaluation.scope, scope.definition);
    assert_eq!(evaluation.configuration, Some(configuration("First")));
    assert_eq!(
        evaluation.of,
        Evaluated::State(Position::After(Member::Feature(
            feature_named(&ir, "Boss").id.clone()
        )))
    );
    assert_eq!(evaluation.status, EvaluationStatus::Current);
    let content = evaluation.content.as_ref().unwrap();
    assert_eq!(content.base, StateBase::Empty);
    let records = &content.delta.records;
    let [body] = records.bodies.as_slice() else {
        panic!("one body: {records:#?}");
    };
    assert!(!current.bodies.contains(&body.id));
    assert_eq!(records.faces.len(), 1);
    assert_eq!(records.vertices.len(), 3);
    assert_eq!(records.points.len(), 3);
    assert!(records
        .points
        .iter()
        .all(|point| point.position().get().x < 10_000.0));
    assert!(content.delta.removed.is_empty());
    assert_no_design_findings(&ir);
}

#[test]
fn a_configuration_flagged_for_rebuild_without_a_partition_is_stale() {
    use cadmpeg_ir::design::evaluation::EvaluationStatus;

    let mut source = with_history(
        sldprt_with_body(&triangle_body()),
        br#"<Keywords Name="Bracket">
            <Configuration Name="Default" SourceIndex="0"/>
            <Configuration Name="Long" SourceIndex="1"/>
            <Extrusion Name="Boss" Type="BossExtrude" id="7"/>
        </Keywords>"#,
        &[],
    );
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<swSolidWorks><swModel swConfigurationName="Default"/><swConfigurationList><swConfiguration swName="Default" swID="0" swConfigurationNeedsUpdate="NO"/><swConfiguration swName="Long" swID="1" swConfigurationNeedsUpdate="YES"/></swConfigurationList></swSolidWorks>"#,
    ));
    let ir = decode(&source);
    let [evaluation] = ir.design.evaluations.as_slice() else {
        panic!("one evaluation: {:#?}", ir.design.evaluations);
    };
    let long = ir
        .design
        .configurations
        .iter()
        .find(|configuration| configuration.name == "Long")
        .unwrap();
    assert_eq!(evaluation.configuration.as_ref(), Some(&long.id));
    assert_eq!(evaluation.status, EvaluationStatus::Stale);
    assert!(evaluation.content.is_none());
    assert_no_design_findings(&ir);
}

/// A part with the three built-in planes, a sketch on the Top plane, and an
/// extrude of it, whose v10 records state a rectangle sketch with a
/// horizontal and a vertical dimension and a blind extrusion.
fn sketch_and_extrude() -> (CadIr, crate::native::SldprtNative) {
    sketch_and_extrude_from(
        br#"<Keywords Name="Block">
            <Feature Name="Front" id="2"/>
            <Feature Name="Top" id="3"/>
            <Feature Name="Right" id="4"/>
            <Sketch Name="Origin" Type="OriginProfileFeature" id="106"/>
            <Sketch Name="Sketch1" Type="ProfileFeature" id="8">
                <Dimension Name="D1">50</Dimension>
                <Dimension Name="D2">30</Dimension>
                <Dimension Name="D3">&lt;MOD-DIAM&gt;10</Dimension>
            </Sketch>
            <Extrusion Name="Boss" Type="BossExtrude" id="7">
                <Dimension Name="D1">10</Dimension>
            </Extrusion>
        </Keywords>"#,
    )
}

/// A part whose history `keywords` holds the built-in planes, the origin, a
/// sketch `Sketch1` and its extrude `Boss`, with the v10 records the decoder
/// types them from.
fn sketch_and_extrude_from(keywords: &[u8]) -> (CadIr, crate::native::SldprtNative) {
    let mut ir = part_with_history(
        keywords,
        &[
            ("moRefPlane_c", "Front", 2),
            ("moRefPlane_c", "Top", 3),
            ("moRefPlane_c", "Right", 4),
            ("moOriginProfileFeature_c", "Origin", 106),
            ("moProfileFeature_c", "Sketch1", 8),
            ("moExtrusion_c", "Boss", 7),
        ],
    );
    let native = crate::native::SldprtNative::load(ir.native.namespace("sldprt").unwrap()).unwrap();
    let id = |name: &str| {
        ir.model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap()
            .id
            .clone()
    };
    let (sketch_feature, boss) = (id("Sketch1"), id("Boss"));
    let parameter = |owner: &cadmpeg_ir::features::FeatureId, name: &str| {
        ir.model
            .parameters
            .iter()
            .find(|parameter| parameter.owner.as_ref() == Some(owner) && parameter.name == name)
            .unwrap()
            .id
            .as_str()
            .to_owned()
    };
    let (width, height) = (
        parameter(&sketch_feature, "D1"),
        parameter(&sketch_feature, "D2"),
    );
    let hole = parameter(&sketch_feature, "D3");
    let sketch = "sldprt:model:sketch#test";
    let entity = |key: &str| format!("sldprt:model:sketch-entity#test:{key}");
    let point = |key: &str, u: f64, v: f64| {
        serde_json::json!({"id": entity(key), "sketch": sketch, "construction": false,
            "native_ref": format!("native:{key}"), "endpoint_refs": [],
            "geometry": {"kind": "point", "position": {"u": u, "v": v}}})
    };
    let line = |key: &str, from: (&str, f64, f64), to: (&str, f64, f64)| {
        serde_json::json!({"id": entity(key), "sketch": sketch, "construction": false,
            "endpoint_refs": [format!("native:{}", from.0), format!("native:{}", to.0)],
            "geometry": {"kind": "line", "start": {"u": from.1, "v": from.2},
                "end": {"u": to.1, "v": to.2}}})
    };
    let corners = [
        ("a", 0.0, 0.0),
        ("b", 50.0, 0.0),
        ("c", 50.0, 30.0),
        ("d", 0.0, 30.0),
    ];
    let mut entities = corners
        .iter()
        .map(|(key, u, v)| point(key, *u, *v))
        .collect::<Vec<_>>();
    for (index, from) in corners.iter().enumerate() {
        let to = corners[(index + 1) % corners.len()];
        entities.push(line(&format!("l{index}"), *from, to));
    }
    entities.push(serde_json::json!({"id": entity("hole"), "sketch": sketch,
        "construction": false, "endpoint_refs": [],
        "geometry": {"kind": "circle", "center": {"u": 25.0, "v": 15.0}, "radius": 5.0}}));
    ir.model.sketch_entities = serde_json::from_value(serde_json::Value::Array(entities)).unwrap();
    ir.model.sketches = serde_json::from_value(serde_json::json!([{
        "id": sketch, "name": "Sketch1",
        "placement": {"kind": "resolved", "frame": {"origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 1.0, "z": 0.0}, "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}}},
        "profiles": []
    }]))
    .unwrap();
    ir.model.sketch_constraints = serde_json::from_value(serde_json::json!([
        {"id": "sldprt:model:sketch-constraint#test:0", "sketch": sketch,
            "definition": {"kind": "coincident_loci", "loci": [
                {"kind": "end", "value": entity("l0")}, {"kind": "start", "value": entity("l1")}]}},
        {"id": "sldprt:model:sketch-constraint#test:1", "sketch": sketch,
            "definition": {"kind": "horizontal_distance",
                "first": {"kind": "start", "value": entity("l1")},
                "second": {"kind": "start", "value": entity("l0")}, "parameter": width},
            "driving": true},
        {"id": "sldprt:model:sketch-constraint#test:2", "sketch": sketch,
            "definition": {"kind": "native", "native_kind": "sgPntPntVertDist",
                "native_properties": {}, "entities": [], "parameter": height,
                "operands": [{"native_kind": "5681", "object_index": 1},
                    {"native_kind": "5681", "object_index": 2}]},
            "driving": true},
        {"id": "sldprt:model:sketch-constraint#test:4", "sketch": sketch,
            "definition": {"kind": "diameter", "entity": entity("hole"), "parameter": hole},
            "driving": true},
        {"id": "sldprt:model:sketch-constraint#test:3", "sketch": sketch,
            "definition": {"kind": "native", "native_kind": "sldprt:sketch-relation:17",
                "native_properties": {}, "entities": [entity("l2")], "operands": []}}
    ]))
    .unwrap();
    for feature in &mut ir.model.features {
        let definition = if feature.id == sketch_feature {
            serde_json::json!({"kind": "operation", "value": {"kind": "sketch",
                "value": {"sketch": {"kind": "planar", "sketch": sketch}}}})
        } else if feature.id == boss {
            serde_json::json!({"kind": "operation", "value": {"kind": "extrude", "value": {
                "profile": {"kind": "planar", "value": {"kind": "feature",
                    "value": sketch_feature.as_str()}},
                "direction": {"kind": "profile_normal"}, "start": {"kind": "profile_plane"},
                "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 10.0}}},
                "op": "join", "solid": true}}})
        } else {
            continue;
        };
        let definition = serde_json::from_value(definition).unwrap();
        feature.evaluation.edit(|current, _| *current = definition);
    }
    (ir, native)
}

#[test]
fn principal_planes_are_datums_on_the_principal_planes() {
    use cadmpeg_ir::design::construction::InputReference;
    use cadmpeg_ir::design::selection::{PickTarget, Picks, PrincipalEntity};

    let (ir, _) = sketch_and_extrude();
    for (name, entity) in [
        ("Front", PrincipalEntity::XyPlane),
        ("Top", PrincipalEntity::XzPlane),
        ("Right", PrincipalEntity::YzPlane),
    ] {
        let FeatureOperation::Datum(datum) = &feature_named(&ir, name).definition.operation else {
            panic!("{name} is no datum");
        };
        let DatumElement::Plane(plane) = &datum.element else {
            panic!("{name} is no plane");
        };
        let [input] = plane.inputs.as_slice() else {
            panic!("{name} has one input");
        };
        let Some(InputReference::Picks(selection)) = &input.reference else {
            panic!("{name} picks");
        };
        let Picks::Named(picks) = &selection.picks else {
            panic!("{name} names its pick");
        };
        assert!(matches!(
            picks.as_slice(),
            [pick] if pick.target == PickTarget::Principal { entity, container: None }
        ));
    }
    assert_no_design_findings(&ir);
}

#[test]
fn a_sketch_and_its_extrude_are_typed_with_their_dimensions() {
    use cadmpeg_ir::design::dimension::Dimension;
    use cadmpeg_ir::design::ops::shared::{Limit, Span};
    use cadmpeg_ir::design::ops::sketch::{DefiningRole, DistanceAxis, Operand, SketchRelation};
    use cadmpeg_ir::design::options::Boolean;
    use cadmpeg_ir::design::units::{Quantity, UnitSymbol};

    let (mut ir, native) = sketch_and_extrude();
    let mut timeline = super::tree::timeline(&native.feature_histories).unwrap();
    let definition = ir.design.scopes[0].definition.clone();
    let entries = super::parameters::Entries::new(&native.feature_histories, &timeline).unwrap();
    let typed = super::typed::project(
        &ir.model,
        &native.feature_histories,
        &[],
        &mut timeline,
        &entries,
        &definition,
    );
    assert!(typed.loss.is_none(), "{:?}", typed.loss);
    let sketches = typed.sketches;
    let [sketch] = sketches.records.as_slice() else {
        panic!("one sketch");
    };
    let feature = |name: &str| {
        timeline
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap()
    };
    assert_eq!(
        feature("Sketch1").definition.operation,
        FeatureOperation::Sketch(sketch.id.clone())
    );
    let corner = sketches
        .entities
        .iter()
        .find(|entity| entity.id.as_str().ends_with(":b"))
        .unwrap();
    assert_eq!(corner.roles.len(), 2);
    assert!(corner
        .roles
        .iter()
        .any(|role| role.role == DefiningRole::End && role.of.as_str().ends_with(":l0")));
    let relation = |index: usize| {
        &sketches
            .constraints
            .iter()
            .find(|constraint| constraint.id.as_str().ends_with(&format!(":{index}")))
            .unwrap_or_else(|| panic!("constraint {index}"))
            .relation
    };
    assert!(sketches
        .constraints
        .iter()
        .all(|constraint| !constraint.id.as_str().ends_with(":0")));
    let point = |key: &str| {
        Operand::Entity(
            sketches
                .entities
                .iter()
                .find(|entity| entity.id.as_str().ends_with(key))
                .unwrap()
                .id
                .clone(),
        )
    };
    let width = entries.owned(&feature("Sketch1").id, "D1").unwrap().clone();
    assert_eq!(
        relation(1),
        &SketchRelation::Distance {
            a: point(":a"),
            b: point(":b"),
            along: Some(DistanceAxis::Horizontal),
            doubled: None,
            value: Dimension::Parameter(width.clone()),
        }
    );
    assert!(
        matches!(relation(2), SketchRelation::Native { kind, values, .. }
        if kind.as_str() == "sldprt:sgPntPntVertDist" && values.len() == 1)
    );
    assert!(
        matches!(relation(3), SketchRelation::Native { kind, operands, .. }
        if kind.as_str() == "sldprt:sketch-relation:17" && operands.len() == 1)
    );

    let FeatureOperation::Extrude(extrude) = &feature("Boss").definition.operation else {
        panic!(
            "the extrude is typed: {:?}",
            feature("Boss").definition.operation
        );
    };
    let depth = entries.owned(&feature("Boss").id, "D1").unwrap().clone();
    assert_eq!(
        extrude.span,
        Some(Span::Between {
            start: Box::new(Limit::AtOrigin),
            end: Box::new(Limit::Amount(Dimension::Parameter(depth.clone()))),
        })
    );
    assert_eq!(extrude.output.boolean, Some(Boolean::NewBody));
    for parameter in [&width, &depth] {
        assert_eq!(typed.bound.get(parameter), Some(&Quantity::LENGTH));
    }

    let (parameters, _) = entries.parameters(
        &definition,
        &super::parameters::Readings {
            decoded: super::parameters::Decoded::new(&ir.model.parameters, &[]),
            length_unit: Some(UnitSymbol::Mm),
            bound: typed.bound,
            measured: typed.measured,
            driven: typed.references,
        },
    );
    for parameter in &parameters {
        assert_eq!(
            expression_json(parameter)["kind"],
            "neutral",
            "{parameter:#?}"
        );
        assert!(parameter.value.is_some(), "{parameter:#?}");
    }
    let diameter = parameters
        .iter()
        .find(|parameter| {
            parameter
                .name
                .as_ref()
                .map(cadmpeg_core::text::NonBlankString::as_str)
                == Some("D3")
        })
        .unwrap();
    assert_eq!(expression_json(diameter)["value"]["text"], "10[mm]");
    assert_eq!(diameter.display, None);
    ir.design.features = timeline.features;
    ir.design.parameters = parameters;
    ir.design.sketches = sketches.records;
    ir.design.sketch_entities = sketches.entities;
    ir.design.sketch_constraints = sketches.constraints;
    assert_no_design_findings(&ir);
}

/// The timeline of `ir`'s design, with its sketches and extrudes typed, and
/// the parameter entries of its features.
fn typed_timeline<'a>(
    ir: &CadIr,
    native: &'a crate::native::SldprtNative,
) -> (
    super::tree::Timeline,
    super::parameters::Entries<'a>,
    super::typed::Typed,
) {
    let mut timeline = super::tree::timeline(&native.feature_histories).unwrap();
    let entries = super::parameters::Entries::new(&native.feature_histories, &timeline).unwrap();
    let typed = super::typed::project(
        &ir.model,
        &native.feature_histories,
        &[],
        &mut timeline,
        &entries,
        &ir.design.scopes[0].definition,
    );
    (timeline, entries, typed)
}

fn timeline_feature<'a>(
    timeline: &'a super::tree::Timeline,
    name: &str,
) -> &'a cadmpeg_ir::design::feature::Feature {
    timeline
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap()
}

#[test]
fn a_one_sided_extrude_bound_to_its_only_dimension_names_it() {
    let (ir, native) = sketch_and_extrude_from(
        br#"<Keywords Name="Block">
            <Feature Name="Front" id="2"/>
            <Feature Name="Top" id="3"/>
            <Feature Name="Right" id="4"/>
            <Sketch Name="Origin" Type="OriginProfileFeature" id="106"/>
            <Sketch Name="Sketch1" Type="ProfileFeature" id="8">
                <Dimension Name="D1">50</Dimension>
                <Dimension Name="D2">30</Dimension>
                <Dimension Name="D3">&lt;MOD-DIAM&gt;10</Dimension>
            </Sketch>
            <Extrusion Name="Boss" Type="BossExtrude" id="7">
                <Dimension Name="D2">10</Dimension>
            </Extrusion>
        </Keywords>"#,
    );
    let (timeline, entries, typed) = typed_timeline(&ir, &native);
    let boss = timeline_feature(&timeline, "Boss");
    assert!(
        matches!(boss.definition.operation, FeatureOperation::Extrude(_)),
        "{:?}",
        boss.definition.operation
    );
    let depth = entries.owned(&boss.id, "D2").unwrap();
    let note = typed.assumed.expect("the assumed depth is named");
    assert_eq!(
        note.code,
        crate::loss::SldprtLossCode::DesignExtrudeDepthAssumed.kind()
    );
    assert!(
        note.message.contains(boss.id.as_str()) && note.message.contains(depth.as_str()),
        "{}",
        note.message
    );
    let (ir, native) = sketch_and_extrude();
    let (_, _, typed) = typed_timeline(&ir, &native);
    assert!(typed.assumed.is_none(), "a depth named D1 is not assumed");
}

#[test]
fn a_two_sided_extrude_binds_only_the_depth_its_record_lists() {
    use cadmpeg_ir::design::ops::shared::{Limit, Span};
    use cadmpeg_ir::design::units::Quantity;
    use cadmpeg_ir::scalar::Length;

    let (mut ir, native) = sketch_and_extrude_from(
        br#"<Keywords Name="Block">
            <Feature Name="Front" id="2"/>
            <Feature Name="Top" id="3"/>
            <Feature Name="Right" id="4"/>
            <Sketch Name="Origin" Type="OriginProfileFeature" id="106"/>
            <Sketch Name="Sketch1" Type="ProfileFeature" id="8">
                <Dimension Name="D1">50</Dimension>
                <Dimension Name="D2">30</Dimension>
                <Dimension Name="D3">&lt;MOD-DIAM&gt;10</Dimension>
            </Sketch>
            <Extrusion Name="Boss" Type="BossExtrude" id="7">
                <Dimension Name="D2">5</Dimension>
            </Extrusion>
        </Keywords>"#,
    );
    let sketch_feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .unwrap()
        .id
        .clone();
    let definition = serde_json::from_value(serde_json::json!({"kind": "operation",
        "value": {"kind": "extrude", "value": {
            "profile": {"kind": "planar", "value": {"kind": "feature",
                "value": sketch_feature.as_str()}},
            "direction": {"kind": "profile_normal"}, "start": {"kind": "profile_plane"},
            "extent": {"kind": "two_sided",
                "first": {"termination": {"kind": "blind", "length": 10.0}},
                "second": {"termination": {"kind": "blind", "length": 5.0}}},
            "op": "join", "solid": true}}}))
    .unwrap();
    ir.model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap()
        .evaluation
        .edit(|current, _| *current = definition);
    let (timeline, entries, typed) = typed_timeline(&ir, &native);
    let boss = timeline_feature(&timeline, "Boss");
    let FeatureOperation::Extrude(extrude) = &boss.definition.operation else {
        panic!("the extrude is typed: {:?}", boss.definition.operation);
    };
    let second = entries.owned(&boss.id, "D2").unwrap().clone();
    assert_eq!(
        extrude.span,
        Some(Span::Between {
            start: Box::new(Limit::Amount(Dimension::Parameter(second.clone()))),
            end: Box::new(Limit::Amount(Dimension::Literal(
                Length::new(10.0).unwrap()
            ))),
        })
    );
    assert_eq!(typed.bound.get(&second), Some(&Quantity::LENGTH));
}

#[test]
fn a_reference_dimension_makes_the_tree_parameter_it_binds_driven() {
    use cadmpeg_ir::design::ops::sketch::SketchRelation;
    use cadmpeg_ir::design::parameter::{ParameterDefinition, ParameterValue};
    use cadmpeg_ir::design::units::UnitSymbol;
    use cadmpeg_ir::scalar::FiniteReal;

    let (mut ir, native) = sketch_and_extrude();
    for constraint in &mut ir.model.sketch_constraints {
        if constraint.id.as_str().ends_with(":4") {
            constraint.driving = Some(false);
        }
    }
    let (timeline, entries, typed) = typed_timeline(&ir, &native);
    assert!(typed.loss.is_none(), "{:?}", typed.loss);
    let hole = entries
        .owned(&timeline_feature(&timeline, "Sketch1").id, "D3")
        .unwrap()
        .clone();
    assert!(typed.sketches.constraints.iter().any(|constraint| matches!(
        &constraint.relation,
        SketchRelation::Radial { value: Dimension::Parameter(parameter), .. } if parameter == &hole
    )));
    let (parameters, _) = entries.parameters(
        &ir.design.scopes[0].definition,
        &super::parameters::Readings {
            decoded: super::parameters::Decoded::new(&ir.model.parameters, &[]),
            length_unit: Some(UnitSymbol::Mm),
            bound: typed.bound,
            measured: typed.measured,
            driven: typed.references,
        },
    );
    let diameter = parameters
        .iter()
        .find(|parameter| parameter.id == hole)
        .unwrap();
    assert_eq!(diameter.definition, ParameterDefinition::Driven);
    assert_eq!(
        diameter.value,
        Some(ParameterValue::Number(FiniteReal::new(10.0).unwrap()))
    );
    let sketches = typed.sketches;
    ir.design.features = timeline.features;
    ir.design.parameters = parameters;
    ir.design.sketches = sketches.records;
    ir.design.sketch_entities = sketches.entities;
    ir.design.sketch_constraints = sketches.constraints;
    assert_no_design_findings(&ir);
}

#[test]
fn a_sketch_with_an_unbound_dimension_or_an_entity_without_a_curve_is_unresolved() {
    assert_sketch_unresolved(
        |ir| {
            let mut constraints = serde_json::to_value(&ir.model.sketch_constraints).unwrap();
            for constraint in constraints.as_array_mut().unwrap() {
                if constraint["id"].as_str().unwrap().ends_with(":1") {
                    constraint["definition"]["parameter"] =
                        serde_json::json!("sldprt:model:parameter#unlisted");
                }
            }
            ir.model.sketch_constraints = serde_json::from_value(constraints).unwrap();
        },
        "a sketch dimension binds no parameter",
    );
    assert_sketch_unresolved(
        |ir| {
            ir.model.sketch_entities.push(
                serde_json::from_value(serde_json::json!({
                    "id": "sldprt:model:sketch-entity#test:text",
                    "sketch": "sldprt:model:sketch#test",
                    "construction": false, "endpoint_refs": [],
                    "geometry": {"kind": "native", "native_kind": "sldprt:sketch-text"}}))
                .unwrap(),
            );
        },
        "a sketch entity has no planar curve",
    );
}

/// Assert that the sketch of [`sketch_and_extrude`], after `change`, stays
/// `Unresolved` for `reason`, and its extrude with it; that its vertical
/// dimension and the extrude's depth still state their parameters' quantity;
/// and that its reference dimensions still make their parameters `Driven`,
/// the tree's hole diameter and one minted for a dimension no tree node lists.
fn assert_sketch_unresolved(change: impl FnOnce(&mut CadIr), reason: &str) {
    use cadmpeg_ir::design::parameter::ParameterDefinition;
    use cadmpeg_ir::design::units::Quantity;

    const MINTED: &str = "sldprt:model:parameter#reference";
    let (mut ir, native) = sketch_and_extrude();
    let mut constraints = serde_json::to_value(&ir.model.sketch_constraints).unwrap();
    let constraints_list = constraints.as_array_mut().unwrap();
    let hole_constraint = constraints_list
        .iter_mut()
        .find(|constraint| constraint["id"].as_str().unwrap().ends_with(":4"))
        .unwrap();
    hole_constraint["driving"] = serde_json::json!(false);
    let hole_parameter = hole_constraint["definition"]["parameter"].clone();
    let mut reference = hole_constraint.clone();
    reference["id"] = serde_json::json!(format!("{}:reference", reference["id"].as_str().unwrap()));
    reference["definition"]["parameter"] = serde_json::json!(MINTED);
    constraints_list.push(reference);
    ir.model.sketch_constraints = serde_json::from_value(constraints).unwrap();
    let mut parameters = serde_json::to_value(&ir.model.parameters).unwrap();
    let parameters_list = parameters.as_array_mut().unwrap();
    let mut minted = parameters_list
        .iter()
        .find(|parameter| parameter["id"] == hole_parameter)
        .unwrap()
        .clone();
    minted["id"] = serde_json::json!(MINTED);
    minted["name"] = serde_json::json!("D9");
    parameters_list.push(minted);
    ir.model.parameters = serde_json::from_value(parameters).unwrap();
    change(&mut ir);
    let (timeline, entries, typed) = typed_timeline(&ir, &native);
    assert!(typed.sketches.records.is_empty(), "{reason}");
    let sketch = timeline_feature(&timeline, "Sketch1");
    let height = entries.owned(&sketch.id, "D2").unwrap();
    assert_eq!(typed.bound.get(height), Some(&Quantity::LENGTH), "{reason}");
    let hole = entries.owned(&sketch.id, "D3").unwrap();
    assert!(typed.references.contains(hole), "{reason}");
    assert!(
        typed
            .sketches
            .driven
            .iter()
            .any(|parameter| parameter.id.as_str() == MINTED
                && parameter.definition == ParameterDefinition::Driven),
        "{reason}"
    );
    let depth = entries
        .owned(&timeline_feature(&timeline, "Boss").id, "D1")
        .unwrap();
    assert_eq!(typed.bound.get(depth), Some(&Quantity::LENGTH), "{reason}");
    assert_eq!(
        sketch.definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Sketch
        },
        "{reason}"
    );
    assert!(matches!(
        timeline_feature(&timeline, "Boss").definition.operation,
        FeatureOperation::Unresolved { .. }
    ));
    let loss = typed.loss.expect("the sketch is reported");
    assert!(
        loss.message
            .contains(&format!("{} ({reason})", sketch.id.as_str())),
        "{}",
        loss.message
    );
}

#[test]
fn a_sketch_and_an_extrude_left_unresolved_are_reported() {
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(native_envelope_fixture(&with_history(
                sldprt_with_body(&triangle_body()),
                br#"<Keywords Name="Bracket">
                    <Extrusion Name="Boss" Type="BossExtrude" id="7">
                        <Dimension Name="D1">12.5mm</Dimension>
                        <Sketch Name="Sketch1" Type="ProfileFeature" id="8"/>
                    </Extrusion>
                </Keywords>"#,
                &[],
            ))),
            &DecodeOptions::default(),
        )
        .unwrap();
    let notes = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::SldprtLossCode::DesignFeatureUnresolved.kind())
        .collect::<Vec<_>>();
    let [note] = notes.as_slice() else {
        panic!("one note: {:#?}", result.report().losses);
    };
    assert!(
        note.message.starts_with("1 sketch and 1 extrude features"),
        "{}",
        note.message
    );
    for name in ["Sketch1", "Boss"] {
        let feature = result
            .ir()
            .design
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap();
        assert!(matches!(
            feature.definition.operation,
            FeatureOperation::Unresolved { .. }
        ));
        assert!(note.message.contains(feature.id.as_str()));
    }
}

fn member_position(timeline: &super::tree::Timeline, name: &str) -> usize {
    timeline
        .features
        .iter()
        .position(|feature| feature.name.as_deref() == Some(name))
        .unwrap()
}

fn depends(features: &mut [cadmpeg_ir::features::Feature], consumer: &str, on: &str) {
    let on = features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(on))
        .unwrap()
        .id
        .clone();
    features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some(consumer))
        .unwrap()
        .dependencies
        .insert(on);
}

#[test]
fn members_follow_the_features_they_depend_on() {
    let (mut ir, native) = sketch_and_extrude_from(
        br#"<Keywords Name="Block">
            <Feature Name="Front" id="2"/>
            <Feature Name="Top" id="3"/>
            <Feature Name="Right" id="4"/>
            <Sketch Name="Origin" Type="OriginProfileFeature" id="106"/>
            <Extrusion Name="Boss" Type="BossExtrude" id="7">
                <Dimension Name="D1">10</Dimension>
            </Extrusion>
            <Sketch Name="Sketch1" Type="ProfileFeature" id="8">
                <Dimension Name="D1">50</Dimension>
                <Dimension Name="D2">30</Dimension>
                <Dimension Name="D3">&lt;MOD-DIAM&gt;10</Dimension>
            </Sketch>
        </Keywords>"#,
    );
    depends(&mut ir.model.features, "Boss", "Sketch1");
    let mut timeline = super::tree::timeline(&native.feature_histories).unwrap();
    assert!(member_position(&timeline, "Boss") < member_position(&timeline, "Sketch1"));
    let note = super::order::regeneration_order(&mut timeline, &ir.model);
    assert!(note.is_none(), "{note:?}");
    assert_eq!(
        member_position(&timeline, "Sketch1") + 1,
        member_position(&timeline, "Boss")
    );

    let definition = ir.design.scopes[0].definition.clone();
    let entries = super::parameters::Entries::new(&native.feature_histories, &timeline).unwrap();
    let typed = super::typed::project(
        &ir.model,
        &native.feature_histories,
        &[],
        &mut timeline,
        &entries,
        &definition,
    );
    assert!(typed.loss.is_none(), "{:?}", typed.loss);
    let boss = &timeline.features[member_position(&timeline, "Boss")];
    assert!(
        matches!(boss.definition.operation, FeatureOperation::Extrude(_)),
        "{:#?}",
        boss.definition.operation
    );
}

#[test]
fn a_folder_keeps_its_run_or_is_dropped_with_a_note() {
    let ir = part_with_history(BRACKET, BRACKET_CLASSES);
    let native = crate::native::SldprtNative::load(ir.native.namespace("sldprt").unwrap()).unwrap();
    let names = |timeline: &super::tree::Timeline| {
        timeline
            .features
            .iter()
            .map(|feature| feature.name.clone().unwrap())
            .collect::<Vec<_>>()
    };

    let mut model = ir.model.clone();
    depends(&mut model.features, "Sketch1", "Boss");
    let mut timeline = super::tree::timeline(&native.feature_histories).unwrap();
    let note = super::order::regeneration_order(&mut timeline, &model);
    assert!(note.is_none(), "{note:?}");
    assert_eq!(
        names(&timeline),
        ["Origin", "Boss", "Sketch1", "Fillet1", "Odd"]
    );
    assert_eq!(
        timeline.folders,
        [Folder {
            name: Some("Group".to_owned()),
            first: 1,
            last: 2,
        }]
    );

    let mut model = ir.model.clone();
    depends(&mut model.features, "Sketch1", "Odd");
    let mut timeline = super::tree::timeline(&native.feature_histories).unwrap();
    let note = super::order::regeneration_order(&mut timeline, &model).unwrap();
    assert_eq!(
        note.code,
        crate::loss::SldprtLossCode::DesignFolderDropped.kind()
    );
    assert!(note.message.contains("Group"), "{}", note.message);
    assert_eq!(
        names(&timeline),
        ["Origin", "Boss", "Fillet1", "Odd", "Sketch1"]
    );
    assert!(timeline.folders.is_empty());
}

#[test]
fn a_configuration_whose_bodies_are_current_bodies_charges_a_loss() {
    use std::collections::HashMap;

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};

    use crate::loss::SldprtLossCode;

    let mut source = with_history(
        crate::test_support::container::sldprt_with_colliding_sites(),
        TWO_CONFIGURATIONS,
        &[],
    );
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<swSolidWorks><swModel swConfigurationName="Second"/></swSolidWorks>"#,
    ));
    let ir = decode(&source);
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one scope");
    };
    let [Container::Ordered { id: timeline, .. }] = ir.design.containers.as_slice() else {
        panic!("one timeline: {:#?}", ir.design.containers);
    };
    // The current state as `active_partition_bodies` falls back to it when
    // the active partition is unresolved: every decoded body, so the other
    // configuration's stored bodies are among the current ones.
    let every_body = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    let features = HashMap::new();
    let names = super::names::Names::new(
        super::Persistent {
            faces: &[],
            bodies: &[],
        },
        &[],
        &features,
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let mut losses = Vec::new();
    let (evaluations, records) = super::states::configuration_states(
        &ctx,
        &ir,
        &super::states::Scope {
            definition: &scope.definition,
            timeline,
            features: &ir.design.features,
            configurations: &ir.design.configurations,
            active: scope.active_configuration.as_ref(),
            current: &every_body,
        },
        &names,
        &mut losses,
    )
    .unwrap();
    assert!(evaluations.is_empty(), "{evaluations:#?}");
    assert_eq!(records, 0);
    let first = ir
        .design
        .configurations
        .iter()
        .find(|configuration| configuration.name == "First")
        .unwrap();
    let [note] = losses.as_slice() else {
        panic!("one loss: {losses:#?}");
    };
    assert_eq!(note.code, SldprtLossCode::ConfigStateUnstated.kind());
    assert!(note.message.contains(&first.id.to_string()), "{note:#?}");
}

mod rollback;
