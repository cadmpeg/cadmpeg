// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::design::ops::sketch::{Planar, SketchConstraint};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{SketchConstraintId, SketchEntityId, SketchId};

use super::*;

fn parameter(
    key: &str,
    name: &str,
    expression: &str,
    value: Option<LegacyValue>,
) -> LegacyParameter {
    LegacyParameter {
        product_definition: None,
        id: ParameterId::mint(format!("creo:featdefs:parameter#{key}")).expect("parameter id"),
        owner: Some(FeatureId::mint("creo:model:feature#40").expect("owner id")),
        ordinal: 0,
        name: name.to_owned(),
        expression: expression.to_owned(),
        display: None,
        value,
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("creo:featdefs:sketch#917".to_owned()),
    }
}

fn length(value: f64) -> LegacyValue {
    LegacyValue::Length(Length::new(value).expect("length"))
}

fn angle(value: f64) -> LegacyValue {
    LegacyValue::Angle(Angle::new(value).expect("angle"))
}

fn with_property(mut parameter: LegacyParameter, key: &str, value: &str) -> LegacyParameter {
    parameter.properties.insert(
        NonBlankString::new(key.to_owned()).expect("key"),
        value.to_owned(),
    );
    parameter
}

fn scope() -> ProductDefinitionId {
    ProductDefinitionId::mint("creo:model:product_definition#root").expect("scope id")
}

fn project_one(parameter: &LegacyParameter) -> DesignParameter {
    project_with(parameter, &[parameter])
}

fn project_with(parameter: &LegacyParameter, all: &[&LegacyParameter]) -> DesignParameter {
    let names = all
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.name.as_str()))
        .collect();
    let kinds = all
        .iter()
        .map(|parameter| (parameter.id.clone(), kind_and_value(parameter).0))
        .collect();
    project_parameter(parameter, &scope(), &names, &kinds)
}

fn neutral_text(parameter: &DesignParameter) -> String {
    match &parameter.definition {
        ParameterDefinition::Expression(expression @ Expression::Neutral(_)) => {
            json::to_value(expression).expect("serialize")["value"]["text"]
                .as_str()
                .expect("canonical text")
                .to_owned()
        }
        other => panic!("not a neutral expression: {other:?}"),
    }
}

fn source_tokens(parameter: &DesignParameter) -> &[ExpressionToken<ParameterRef>] {
    match &parameter.definition {
        ParameterDefinition::Expression(Expression::Source {
            dialect: ExpressionDialect::Creo,
            tokens,
        }) => tokens,
        other => panic!("not a Creo source expression: {other:?}"),
    }
}

#[test]
fn a_bare_literal_with_no_declared_unit_stays_source_text() {
    let projected = project_one(&parameter("1", "d1", "1", Some(length(25.4))));
    assert_eq!(
        source_tokens(&projected),
        [ExpressionToken::Text("1".to_owned())]
    );
    assert_eq!(projected.kind, ValueKind::Quantity(Quantity::LENGTH));
    assert_eq!(
        projected.value,
        Some(ParameterValue::Number(
            FiniteReal::new(25.4).expect("value")
        ))
    );
    assert_eq!(projected.scope, scope());
    assert_eq!(
        projected.owner,
        Some(ParameterOwner::Feature(
            FeatureId::mint("creo:model:feature#40").expect("owner")
        ))
    );
    let projected = project_one(&parameter("2", "d2", "30", Some(angle(30f64.to_radians()))));
    assert_eq!(
        source_tokens(&projected),
        [ExpressionToken::Text("30".to_owned())]
    );
}

#[test]
fn a_bracketed_unit_and_a_declared_unit_name_the_literal_unit() {
    let projected = project_one(&parameter("1", "copy", "25.4[mm]", Some(length(25.4))));
    assert_eq!(neutral_text(&projected), "25.4[mm]");
    let projected = project_one(&parameter("3", "copy", "1[in]", Some(length(25.0))));
    assert_eq!(
        source_tokens(&projected),
        [ExpressionToken::Text("1[in]".to_owned())],
        "a written unit the stored value does not confirm stays source text"
    );
    let declared = with_property(
        parameter("2", "span", "2", Some(length(50.8))),
        "declared_unit",
        "inch",
    );
    let projected = project_one(&declared);
    assert_eq!(neutral_text(&projected), "2[in]");
    assert_eq!(projected.unit, Some(single(UnitSymbol::In)));
}

#[test]
fn an_unread_value_keeps_its_empty_text_and_states_no_cache() {
    let projected = project_one(&parameter("1", "d1", "", None));
    assert!(source_tokens(&projected).is_empty());
    assert_eq!(projected.value, None);
}

#[test]
fn resolved_dependencies_are_references_matched_without_case() {
    let target = parameter("1", "span", "2", Some(length(2.0)));
    let mut relation = parameter("2", "copy", "SPAN+d3:1+1[span]", Some(length(3.0)));
    relation.dependencies = DistinctMembers::try_from(vec![target.id.clone()]).expect("deps");
    let projected = project_with(&relation, &[&target, &relation]);
    assert_eq!(
        source_tokens(&projected),
        [
            ExpressionToken::Reference(ParameterRef {
                target: RefTarget::Parameter(target.id.clone()),
                through: None,
            }),
            ExpressionToken::Text("+d3:1+1[span]".to_owned()),
        ]
    );
}

#[test]
fn a_relation_that_is_one_reference_is_neutral_where_the_kinds_agree() {
    let target = parameter("1", "span", "2", Some(length(2.0)));
    let mut copy = parameter("2", "copy", "SPAN", Some(length(2.0)));
    copy.dependencies = DistinctMembers::try_from(vec![target.id.clone()]).expect("deps");
    let projected = project_with(&copy, &[&target, &copy]);
    assert_eq!(
        projected.definition,
        ParameterDefinition::Expression(Expression::Neutral(Neutral(Node::Reference(
            ParameterRef {
                target: RefTarget::Parameter(target.id.clone()),
                through: None,
            }
        ))))
    );

    let mut turn = parameter("3", "turn", "SPAN", Some(angle(2.0)));
    turn.dependencies = DistinctMembers::try_from(vec![target.id.clone()]).expect("deps");
    let projected = project_with(&turn, &[&target, &turn]);
    assert_eq!(
        source_tokens(&projected),
        [ExpressionToken::Text("SPAN".to_owned())],
        "a reference of another kind stays text, so the source holds a text token"
    );
}

#[test]
fn an_evaluated_quantity_states_its_exponents_and_a_temperature_is_native() {
    let stress = with_property(
        with_property(
            parameter("1", "stress", "2", None),
            "evaluated_dimension",
            "length:-1,mass:1,time:-2,angle:0,temperature:0",
        ),
        "evaluated_canonical_value",
        "2000",
    );
    let projected = project_one(&stress);
    assert_eq!(
        projected.kind,
        ValueKind::Quantity(Quantity {
            length: -1,
            mass: 1,
            angle: 0,
            time: -2
        })
    );
    assert_eq!(
        projected.value,
        Some(ParameterValue::Number(
            FiniteReal::new(2000.0).expect("value")
        ))
    );
    let freezing = with_property(
        parameter("2", "freezing", "0", None),
        "evaluated_dimension",
        "length:0,mass:0,time:0,angle:0,temperature:1",
    );
    let projected = project_one(&freezing);
    assert!(matches!(projected.kind, ValueKind::Native(_)));
    assert_eq!(projected.value, None);
    assert!(matches!(
        projected.definition,
        ParameterDefinition::Expression(Expression::Source { .. })
    ));
}

fn radial(key: &str, measure: RadialMeasure, parameter: &ParameterId) -> SketchConstraint<Planar> {
    SketchConstraint {
        id: SketchConstraintId::mint(format!("creo:model:sketch_constraint#917:{key}"))
            .expect("constraint id"),
        sketch: SketchId::mint("creo:model:sketch#917").expect("sketch id"),
        relation: SketchRelation::Radial {
            of: SketchEntityId::mint("creo:model:sketch_entity#917:arc").expect("entity id"),
            measure,
            value: Dimension::Parameter(parameter.clone()),
        },
        name: None,
        active: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: None,
    }
}

#[test]
fn a_radial_dimension_presentation_is_stated_only_where_the_dimension_differs() {
    let displayed = |key: &str, display| {
        let mut parameter = parameter(key, &format!("d{key}"), "5", Some(length(5.0)));
        parameter.display = Some(display);
        project_one(&parameter)
    };
    let mut ir = CadIr::empty();
    ir.design.parameters = vec![
        displayed("1", LegacyDisplay::Radius),
        displayed("2", LegacyDisplay::Diameter),
        displayed("3", LegacyDisplay::Diameter),
        displayed("4", LegacyDisplay::Radius),
    ];
    let ids = ir
        .design
        .parameters
        .iter()
        .map(|parameter| parameter.id.clone())
        .collect::<Vec<_>>();
    ir.design.sketch_constraints = vec![
        radial("1", RadialMeasure::Radius, &ids[0]),
        radial("2", RadialMeasure::Diameter, &ids[1]),
        radial("3", RadialMeasure::Radius, &ids[2]),
    ];
    omit_measured_display(&mut ir);
    let displays = ir
        .design
        .parameters
        .iter()
        .map(|parameter| parameter.display)
        .collect::<Vec<_>>();
    assert_eq!(
        displays,
        [
            None,
            None,
            Some(DimensionDisplay::Diameter),
            Some(DimensionDisplay::Radius)
        ]
    );
}

#[test]
fn the_parameters_that_drop_a_dependency_are_named_in_one_note() {
    let target = parameter("1", "span", "2", Some(length(2.0)));
    let depends = |mut parameter: LegacyParameter| {
        parameter.dependencies = DistinctMembers::try_from(vec![target.id.clone()]).expect("deps");
        parameter
    };
    let turns = (2..=10)
        .map(|key| {
            depends(parameter(
                &key.to_string(),
                &format!("turn{key}"),
                "SPAN",
                Some(angle(2.0)),
            ))
        })
        .collect::<Vec<_>>();
    let mut ir = CadIr::empty();
    ir.model.parameters = [target.clone()]
        .into_iter()
        .chain(turns.iter().cloned())
        .chain([
            depends(parameter("11", "copy", "SPAN", Some(length(2.0)))),
            depends(parameter("12", "sum", "span+1[mm]", Some(length(3.0)))),
        ])
        .collect();
    let mut losses = Vec::new();
    crate::decode::with_service_context(|ctx| project(ctx, &mut ir, &scope(), &mut losses))
        .expect("projection");
    let [note] = losses.as_slice() else {
        panic!("one note: {losses:?}");
    };
    assert_eq!(
        note.code,
        CreoLossCode::DesignParameterReferenceAsText.kind()
    );
    let named = turns[..8]
        .iter()
        .map(|turn| format!("{} ({})", turn.id, target.id))
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(
        note.message,
        format!(
            "9 parameters hold a parameter the decoder resolved as their dependency as text, \
             or omit it, and state no dependency on it (each with the dependencies it loses): \
             {named}, ..."
        )
    );
}

#[test]
fn the_names_a_relation_leaves_out_as_a_cycle_are_named_in_the_note() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x04r=a\0a=r\0theta=t*360\0z=1\0"
        .to_vec();
    let data = crate::test_support::build_prt("c", &[("DEPDB_DATA", payload)]);
    let result = crate::CreoCodec
        .decode(&mut std::io::Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let [r, a, _, _] = result.ir().model.parameters.as_slice() else {
        panic!("four curve-expression parameters");
    };
    let notes = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == CreoLossCode::DesignParameterReferenceAsText.kind())
        .collect::<Vec<_>>();
    let [note] = notes.as_slice() else {
        panic!("one note: {:?}", result.report().losses);
    };
    assert_eq!(
        note.message,
        format!(
            "2 parameters hold a parameter the decoder resolved as their dependency as text, \
             or omit it, and state no dependency on it (each with the dependencies it loses): \
             {} (cyclic a), {} (cyclic r)",
            r.id, a.id
        )
    );
}

#[test]
fn a_model_whose_parameters_hold_every_dependency_has_no_note() {
    let target = parameter("1", "span", "2", Some(length(2.0)));
    let mut copy = parameter("2", "copy", "SPAN", Some(length(2.0)));
    copy.dependencies = DistinctMembers::try_from(vec![target.id.clone()]).expect("deps");
    let mut ir = CadIr::empty();
    ir.model.parameters = vec![target, copy];
    let mut losses = Vec::new();
    crate::decode::with_service_context(|ctx| project(ctx, &mut ir, &scope(), &mut losses))
        .expect("projection");
    assert!(losses.is_empty(), "{losses:?}");
}
