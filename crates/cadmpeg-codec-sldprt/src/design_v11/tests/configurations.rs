// SPDX-License-Identifier: Apache-2.0
//! The design configurations of a part (§5.11) and what a CADIR write of
//! them keeps.
#![allow(clippy::unwrap_used)]

use super::*;

use cadmpeg_ir::design::configuration::{DesignConfiguration, Overrides};
use cadmpeg_ir::design::expr::{Expression, Neutral, Node};
use cadmpeg_ir::design::parameter::DesignParameter;
use cadmpeg_ir::design::parameter::ParameterValue;
use cadmpeg_ir::design::units::{Unit, UnitFactor, UnitSymbol};
use cadmpeg_ir::features::parameters::{
    DesignConfiguration as ModelConfiguration, ParameterValue as ModelValue,
};

use crate::test_support::cadir::round_trip;
use crate::test_support::history::resolved_features_payload_with_names_relation_and_scalar;

/// A `Contents/SolidWorks` block whose active configuration is `active`.
fn active_configuration(active: &str) -> Vec<u8> {
    make_block(
        0x43,
        "Contents/SolidWorks",
        format!(r#"<swSolidWorks><swModel swConfigurationName="{active}"/></swSolidWorks>"#)
            .as_bytes(),
    )
}

/// A part with the configurations `Default` and `Long`, no feature, and the
/// active configuration `active`.
fn without_features(active: &str) -> (CadIr, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let mut source = with_history(
        sldprt_with_body(&triangle_body()),
        br#"<Keywords Name="Bracket">
            <Configuration Name="Default" SourceIndex="0"/>
            <Configuration Name="Long" SourceIndex="1"/>
        </Keywords>"#,
        &[],
    );
    source.extend(active_configuration(active));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(native_envelope_fixture(&source)),
            &DecodeOptions::default(),
        )
        .unwrap();
    (result.ir().clone(), result.report().losses.clone())
}

fn named<'a>(ir: &'a CadIr, name: &str) -> &'a DesignConfiguration {
    ir.design
        .configurations
        .iter()
        .find(|configuration| configuration.name == name)
        .unwrap()
}

#[test]
fn the_configurations_of_a_part_with_no_feature_are_those_of_its_scope() {
    let (ir, _) = without_features("Default");
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one scope: {:#?}", ir.design.scopes);
    };
    assert!(scope.roots.is_empty());
    assert!(scope.current.is_empty());
    let bodies = body_ids(&ir);
    assert!(!bodies.is_empty());
    assert_eq!(
        scope
            .unproduced
            .iter()
            .map(|body| body.as_str().to_owned())
            .collect::<Vec<_>>(),
        bodies
    );
    assert_eq!(ir.design.configurations.len(), 2);
    assert_eq!(
        scope.active_configuration.as_ref(),
        Some(&named(&ir, "Default").id)
    );
    assert_no_design_findings(&ir);

    let read = round_trip(&ir);
    assert_eq!(read.design.configurations, ir.design.configurations);
    assert_eq!(read.design.scopes, ir.design.scopes);
}

#[test]
fn configurations_whose_active_one_does_not_resolve_are_named_in_a_note() {
    let (ir, losses) = without_features("Missing");
    assert!(ir.design.configurations.is_empty());
    let [message] = losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::DesignConfigurationActiveUnread.kind())
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>()[..]
    else {
        panic!("one note: {losses:#?}");
    };
    for configuration in &ir.model.configurations {
        assert!(message.contains(configuration.id.as_str()), "{message}");
    }
    assert!(message.starts_with("2 configurations"), "{message}");
}

/// The configurations of [`with_configured_radius`], in lane order.
const CONFIGURATIONS: [&str; 3] = ["Default", "Large", "Twin"];

/// A part whose fillet radius `D1` the history states as `text`, with `D2`
/// defined as `D1 * 2`, and whose leading [`CONFIGURATIONS`], of which
/// `Default` is the active one, store `D1` as `radii` metres in their own
/// lanes.
fn with_configured_radius(text: &str, radii: &[f64]) -> CadIr {
    let mut source = sldprt_with_body(&triangle_body());
    let mut configurations = String::new();
    for name in &CONFIGURATIONS[..radii.len()] {
        configurations.push_str(r#"<Configuration Name=""#);
        configurations.push_str(name);
        configurations.push_str(r#""/>"#);
    }
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        format!(r#"<Keywords>{configurations}<Fillet Name="Round1" Type="Fillet"><Dimension Name="D1">{text}</Dimension><Dimension Name="D2">D1 * 2</Dimension></Fillet></Keywords>"#).as_bytes(),
    ));
    for (lane, &radius) in radii.iter().enumerate() {
        source.extend(make_block(
            0x45,
            &format!("Contents/Config-{lane}-ResolvedFeatures"),
            &resolved_features_payload_with_names_relation_and_scalar(
                &[0],
                &["Round1", "D1"],
                "sgPntPntDist",
                radius,
            ),
        ));
    }
    source.extend(active_configuration("Default"));
    decode(&source)
}

#[test]
fn a_dimension_another_configuration_stores_apart_is_an_override_of_it() {
    let ir = with_configured_radius("30mm", &[0.025, 0.050]);
    let parameter = |name: &str| {
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
    };
    let (radius, doubled) = (parameter("D1"), parameter("D2"));
    assert!(named(&ir, "Default").overrides.parameters.is_empty());
    let overrides = &named(&ir, "Large").overrides.parameters;
    assert_eq!(overrides.len(), 1, "{overrides:#?}");

    let literal = &overrides[&radius.id];
    let Some(Expression::Neutral(Neutral(Node::Number { value, unit }))) = &literal.expression
    else {
        panic!("a literal: {literal:#?}");
    };
    assert!((value.get() - 50.0).abs() < 1e-9);
    assert_eq!(
        *unit,
        Unit(vec![UnitFactor {
            symbol: UnitSymbol::Mm,
            power: std::num::NonZeroI8::new(1).unwrap(),
        }])
    );
    let Some(ParameterValue::Number(cached)) = &literal.value else {
        panic!("a number: {literal:#?}");
    };
    assert!((cached.get() - 50.0).abs() < 1e-9);

    assert!(!overrides.contains_key(&doubled.id));
    assert_eq!(
        radius.value,
        Some(ParameterValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(25.0).unwrap()
        ))
    );
    assert_no_design_findings(&ir);

    let read = round_trip(&ir);
    assert_eq!(read.design.configurations, ir.design.configurations);
}

fn parameter_named<'a>(ir: &'a CadIr, name: &str) -> &'a DesignParameter {
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

fn stored<'a>(ir: &'a CadIr, name: &str) -> &'a ModelConfiguration {
    ir.model
        .configurations
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some(name))
        .unwrap()
}

/// `Twin` stores the active configuration's radius, which differs from the
/// history's text at the literal's digits, and states no override; `Large`
/// stores another radius and states one.
#[test]
fn a_configuration_storing_the_active_configurations_values_states_no_override() {
    let active = 0.012_345_666_666_666_7;
    let ir = with_configured_radius("12.34567mm", &[active, 0.05, active]);
    let radius = parameter_named(&ir, "D1");
    assert!(stored(&ir, "Twin")
        .parameter_values
        .contains_key(&radius.id));
    assert!(named(&ir, "Default").overrides.parameters.is_empty());
    assert!(named(&ir, "Large")
        .overrides
        .parameters
        .contains_key(&radius.id));
    let overrides = &named(&ir, "Twin").overrides.parameters;
    assert!(overrides.is_empty(), "{overrides:#?}");
}

fn length(value: f64) -> ModelValue {
    ModelValue::Length(cadmpeg_ir::scalar::Length::new(value).unwrap())
}

fn number(value: f64) -> ParameterValue {
    ParameterValue::Number(cadmpeg_ir::scalar::FiniteReal::new(value).unwrap())
}

fn millimetres(value: f64) -> Expression {
    Expression::Neutral(Neutral(
        crate::design_v11::parameters::number_node(value, Some(UnitSymbol::Mm)).unwrap(),
    ))
}

fn project_samples(
    ir: &mut CadIr,
    values: &[(&str, &[(&str, ModelValue)])],
) -> (
    DesignConfiguration,
    bool,
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    use super::super::configurations::{values::project, Configurations};

    let mut samples = crate::history::configuration::ConfigurationSamples::new();
    for &(configuration, values) in values {
        let parameters = values
            .iter()
            .map(|(name, value)| (parameter_named(ir, name).id.clone(), value.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let lane = ir
            .model
            .configurations
            .iter_mut()
            .find(|lane| lane.name.as_deref() == Some(configuration))
            .unwrap();
        samples.insert(lane.id.clone(), parameters.keys().cloned().collect());
        lane.parameter_values = parameters;
    }
    let mut configurations = Configurations {
        records: ir
            .design
            .configurations
            .iter()
            .cloned()
            .map(|mut record| {
                record.overrides = Overrides::default();
                record
            })
            .collect(),
        active: ir.design.scopes[0].active_configuration.clone(),
    };
    let (cuts, losses) = project(
        &mut configurations,
        &ir.model.configurations,
        &samples,
        &mut ir.design.parameters,
        Some(UnitSymbol::Mm),
    );
    let alternate = configurations
        .records
        .into_iter()
        .find(|record| record.name == "Large")
        .unwrap();
    let cut = cuts.contains(&alternate.id);
    (alternate, cut, losses)
}

#[test]
fn a_stored_result_overrides_the_expression_evaluated_with_the_alternate_inputs() {
    for (driving, result) in [(25.0, 70.0), (50.0, 50.0), (50.0, 70.0)] {
        let mut ir = with_configured_radius("30mm", &[0.025, 0.025]);
        let (large, cut, losses) = project_samples(
            &mut ir,
            &[
                ("Default", &[("D1", length(25.0)), ("D2", length(50.0))]),
                ("Large", &[("D1", length(driving)), ("D2", length(result))]),
            ],
        );
        let radius = parameter_named(&ir, "D1");
        assert_eq!(radius.value, Some(number(25.0)));
        assert_eq!(
            large.overrides.parameters.contains_key(&radius.id),
            driving != 25.0
        );
        let entry = &large.overrides.parameters[&parameter_named(&ir, "D2").id];
        assert_eq!(entry.expression, Some(millimetres(result)));
        assert_eq!(entry.value, Some(number(result)));
        assert!(!cut);
        assert!(losses.is_empty(), "{losses:#?}");
    }
}

#[test]
fn an_incompatible_value_repeats_the_base_when_the_inherited_result_agrees() {
    for (driving, reading, unread) in [(25.0, 50, false), (25.0, 70, true), (50.0, 50, true)] {
        let mut ir = with_configured_radius("25mm", &[0.025, 0.025]);
        let (large, cut, losses) = project_samples(
            &mut ir,
            &[
                (
                    "Default",
                    &[("D1", length(25.0)), ("D2", ModelValue::Integer(50))],
                ),
                (
                    "Large",
                    &[
                        ("D1", length(driving)),
                        ("D2", ModelValue::Integer(reading)),
                    ],
                ),
            ],
        );
        assert!(!large
            .overrides
            .parameters
            .contains_key(&parameter_named(&ir, "D2").id));
        assert_eq!(cut, unread, "{driving}, {reading}");
        assert_eq!(losses.len(), usize::from(unread), "{losses:#?}");
    }
}

#[test]
fn a_missing_intermediate_sample_uses_its_inherited_expression() {
    use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
    use cadmpeg_ir::design::expr::AddOp;
    use cadmpeg_ir::design::parameter::ParameterDefinition;

    let mut ir = with_configured_radius("10mm", &[0.010, 0.020]);
    let b = parameter_named(&ir, "D2");
    let mut c = b.clone();
    c.id = cadmpeg_ir::ParameterId::mint("sldprt:model:parameter#dependent-C").unwrap();
    c.name = Some(cadmpeg_core::nonblank_literal!("C"));
    let step = crate::design_v11::parameters::number_node(1.0, Some(UnitSymbol::Mm)).unwrap();
    c.definition = ParameterDefinition::Expression(Expression::Neutral(Neutral(Node::add(
        Node::Reference(ParameterRef {
            target: RefTarget::Parameter(b.id.clone()),
            through: None,
        }),
        AddOp::Add,
        step,
    ))));
    c.value = Some(number(21.0));
    ir.design.parameters.insert(0, c);
    let (large, cut, losses) = project_samples(
        &mut ir,
        &[
            (
                "Default",
                &[
                    ("D1", length(10.0)),
                    ("D2", length(20.0)),
                    ("C", length(21.0)),
                ],
            ),
            ("Large", &[("D1", length(20.0)), ("C", length(41.0))]),
        ],
    );
    assert_eq!(large.overrides.parameters.len(), 1);
    assert!(large
        .overrides
        .parameters
        .contains_key(&parameter_named(&ir, "D1").id));
    assert!(!cut);
    assert!(losses.is_empty(), "{losses:#?}");
}

#[test]
fn an_unread_expression_preserves_the_sample_as_a_charged_cache() {
    use cadmpeg_ir::design::expr::{ExpressionDialect, ExpressionToken};
    use cadmpeg_ir::design::parameter::ParameterDefinition;

    let mut ir = with_configured_radius("30mm", &[0.025, 0.050]);
    let id = parameter_named(&ir, "D2").id.clone();
    ir.design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == id)
        .unwrap()
        .definition = ParameterDefinition::Expression(Expression::Source {
        dialect: ExpressionDialect::SolidWorks,
        tokens: vec![ExpressionToken::Text("\"Unresolved\" * 2".to_owned())],
    });
    let (large, cut, losses) = project_samples(&mut ir, &[("Large", &[("D2", length(70.0))])]);
    let entry = &large.overrides.parameters[&id];
    assert_eq!(entry.expression, None);
    assert_eq!(entry.value, Some(number(70.0)));
    assert!(!cut);
    let [loss] = losses.as_slice() else {
        panic!("{losses:#?}")
    };
    assert_eq!(
        loss.code,
        SldprtLossCode::DesignConfigurationParameterUnread.kind()
    );
    assert!(loss.message.contains(large.id.as_str()));
    assert!(loss.message.contains(id.as_str()));
}

#[test]
fn disjoint_lane_names_share_the_combined_parameter_identities() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Large"/>
            <Fillet Name="Round1" Type="Fillet"/></Keywords>"#,
    ));
    for (lane, scalar) in [(0, 0.025), (1, 0.070)] {
        let mut payload = resolved_features_payload_with_names_relation_and_scalar(
            &[0],
            &["Round1", "D1"],
            "moLengthParameter_c",
            scalar,
        );
        if lane == 1 {
            let name = [0xff, 0xfe, 0xff, 2, b'D', 0, b'1', 0];
            let at = payload
                .windows(name.len())
                .position(|part| part == name)
                .unwrap();
            payload[at + 6] = b'2';
        }
        source.extend(make_block(
            0x45,
            &format!("Contents/Config-{lane}-ResolvedFeatures"),
            &payload,
        ));
    }
    source.extend(active_configuration("Default"));
    let ir = decode(&source);
    let d1 = &parameter_named(&ir, "D1").id;
    let d2 = &parameter_named(&ir, "D2").id;
    let active = &stored(&ir, "Default").parameter_values;
    let alternate = &stored(&ir, "Large").parameter_values;
    assert_eq!(active[d1], length(25.0));
    assert_eq!(alternate[d1], length(25.0));
    assert_eq!(alternate[d2], length(70.0));
    assert!(!named(&ir, "Large").overrides.parameters.contains_key(d1));
}

/// Each configuration that is not active is named for its unread
/// suppression, and each feature whose decoded definition there differs from
/// the active configuration's is named for its unstated definition.
#[test]
fn configurations_that_are_not_active_name_their_unread_suppression_and_definitions() {
    use cadmpeg_ir::features::parameters::{ConfigurationEvaluation, ConfigurationFeatureState};
    use cadmpeg_ir::features::selections::DistinctMembers;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, UnresolvedFamily};

    let ir = with_configured_radius("30mm", &[0.025, 0.050, 0.025]);
    let round = feature_named(&ir, "Round1");
    let state = |family| ConfigurationFeatureState {
        evaluation: ConfigurationEvaluation::Active {
            outputs: DistinctMembers::default(),
        },
        dependencies: DistinctMembers::default(),
        definition: FeatureDefinition::Operation(FeatureOperation::Unresolved { family }),
    };
    let mut decoded = ir.model.configurations.clone();
    for configuration in &mut decoded {
        let family = match configuration.name.as_deref() {
            Some("Large") => UnresolvedFamily::Cone,
            _ => UnresolvedFamily::Cylinder,
        };
        configuration
            .feature_states
            .insert(round.id.clone(), state(family));
    }
    let configurations = crate::design_v11::configurations::Configurations {
        records: ir.design.configurations.clone(),
        active: Some(named(&ir, "Default").id.clone()),
    };
    let notes = crate::design_v11::configurations::unread_states(
        &configurations,
        &decoded,
        std::slice::from_ref(round),
    );
    let message = |code: SldprtLossCode| {
        let [note] = notes
            .iter()
            .filter(|note| note.code == code.kind())
            .collect::<Vec<_>>()[..]
        else {
            panic!("one note: {notes:#?}");
        };
        note.message.clone()
    };
    let (large, twin) = (&named(&ir, "Large").id, &named(&ir, "Twin").id);
    let suppression = message(SldprtLossCode::DesignConfigurationSuppressionUnread);
    assert!(
        suppression.starts_with("2 configurations")
            && suppression.ends_with(&format!("{}, {}", large.as_str(), twin.as_str())),
        "{suppression}"
    );
    let definition = message(SldprtLossCode::DesignConfigurationDefinitionUnstated);
    assert!(
        definition.starts_with("1 features")
            && definition.ends_with(&format!("{} in {}", round.id.as_str(), large.as_str())),
        "{definition}"
    );
}
