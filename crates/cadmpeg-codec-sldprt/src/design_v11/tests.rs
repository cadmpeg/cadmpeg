// SPDX-License-Identifier: Apache-2.0
//! Decode tests of the v11 design of a part.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::design::container::{Container, Folder, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::datum::DatumElement;
use cadmpeg_ir::design::refs::{Member, Position};
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
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", keywords));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(classes),
    ));
    decode(&source)
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

#[test]
fn feature_tree_is_one_timeline_of_the_root_definition() {
    let ir = part_with_history(
        br#"<Keywords Name="Bracket">
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
        </Keywords>"#,
        &[
            ("moDetailCabinet_c", "Annotations", 101),
            ("moOriginProfileFeature_c", "Origin", 106),
            ("moFtrFolder_c", "Group", 110),
        ],
    );
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
    assert!(ir.model.product_definitions.is_empty());
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
        Some(cadmpeg_ir::features::ParameterValue::Length(length)) if length.get() == 25.0
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
        cadmpeg_ir::features::ParameterValue,
        Option<&str>,
    )],
    classes: &[(&str, crate::records::ArchiveParameterClass)],
) -> (CadIr, Vec<cadmpeg_ir::design::parameter::DesignParameter>) {
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
    let parameters = super::parameters::parameters(
        &native.feature_histories,
        &timeline,
        &ir.design.scopes[0].definition,
        &decoded,
    )
    .unwrap();
    (ir, parameters)
}

#[test]
fn a_dimension_caches_the_stored_value_not_its_display() {
    use crate::records::ArchiveParameterClass;
    use cadmpeg_ir::design::parameter::{DimensionDisplay, ParameterValue};
    use cadmpeg_ir::design::units::{Quantity, ValueKind};
    use cadmpeg_ir::features::ParameterValue as V10;
    use cadmpeg_ir::scalar::Length;

    let length = |millimetres: f64| V10::Length(Length::new(millimetres).unwrap());
    let (ir, parameters) = parameters_with_readings(
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

fn timeline_feature(name: &str, suppressed: bool) -> cadmpeg_ir::design::feature::Feature {
    cadmpeg_ir::design::feature::Feature {
        id: crate::history::project::neutral_feature_id(name).unwrap(),
        name: Some(name.to_owned()),
        suppressed: Some(Dimension::Literal(suppressed)),
        definition: cadmpeg_ir::design::feature::FeatureDefinition {
            operation: super::operation::origin(),
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    }
}

fn rolled_back(after: &str, later: &[&str]) -> crate::rollback::Reading {
    crate::rollback::Reading::RolledBack {
        after: after.to_owned(),
        later: later.iter().map(|name| (*name).to_owned()).collect(),
    }
}

#[test]
fn stored_marks_state_how_many_members_are_built() {
    use crate::rollback::{Reading, Unread};
    let features = [
        timeline_feature("Sketch1", false),
        timeline_feature("Boss-Extrude1", false),
        timeline_feature("Sketch2", false),
        timeline_feature("Cut-Extrude1", false),
        timeline_feature("Fillet1", true),
    ];
    assert_eq!(super::stated(&features, Reading::End), Ok(5));
    assert_eq!(
        super::stated(
            &features,
            rolled_back("Boss-Extrude1", &["Cut-Extrude1", "Fillet1"])
        ),
        Ok(2)
    );
    assert_eq!(
        super::stated(&features, rolled_back("Cut-Extrude1", &["Fillet1"])),
        Ok(5),
        "suppressed later features leave the tree built through its end"
    );
    assert_eq!(
        super::stated(&features, rolled_back("Boss-Extrude2", &["Fillet1"])),
        Err(Unread::FeatureUnresolved {
            name: "Boss-Extrude2".to_owned()
        })
    );
    assert_eq!(
        super::stated(&features, rolled_back("Cut-Extrude1", &["Boss-Extrude1"])),
        Err(Unread::FeatureOutOfOrder {
            name: "Boss-Extrude1".to_owned()
        })
    );
    assert_eq!(
        super::stated(&features, Reading::Unread(Unread::NoMarkClass)),
        Err(Unread::NoMarkClass)
    );
}

#[test]
fn a_rolled_back_timeline_states_the_member_after_its_bar() {
    let id = super::container_id(cadmpeg_ir::identity_key!("timeline"));
    let members = ["Sketch1", "Boss-Extrude1", "Sketch2"]
        .map(|name| Member::Feature(timeline_feature(name, false).id));
    assert_eq!(
        super::built_through(&id, &members, 2),
        (
            Rollback::Before(members[2].clone()),
            Position::After(members[1].clone())
        )
    );
    assert_eq!(
        super::built_through(&id, &members, 3),
        (Rollback::End, Position::After(members[2].clone()))
    );
    assert_eq!(
        super::built_through(&id, &members, 0),
        (
            Rollback::Before(members[0].clone()),
            Position::Start(id.clone())
        )
    );
}
