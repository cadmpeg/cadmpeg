use crate::design::construction::{
    kinds, AnyConstruction, Construction, ConstructionInput, FrameAxis, InputReference, PostStep,
    SourceMode,
};
use crate::design::dimension::Dimension;
use crate::design::names::NativeNameValue;
use crate::design::options::{NativeOption, NativeOptions};
use crate::design::refs::StateRef;
use crate::design::selection::OrderedSelection;
use crate::features::{ConfigurationId, ParameterId};
use crate::scalar::Length;
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities, TextReplacements};

#[test]
fn state_references_visit_and_rewrite_each_v11_evaluated_form() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    for (source, expected, target) in [
        (
            serde_json::json!({"kind":"state","value":{"kind":"start","value":"test:model:container#one"}}),
            serde_json::json!({"kind":"state","value":{"kind":"start","value":"test:placed:container#one"}}),
            "test:model:container#one",
        ),
        (
            serde_json::json!({"kind":"state","value":{"kind":"after","value":{"kind":"feature","value":"test:model:feature#one"}}}),
            serde_json::json!({"kind":"state","value":{"kind":"after","value":{"kind":"feature","value":"test:placed:feature#one"}}}),
            "test:model:feature#one",
        ),
        (
            serde_json::json!({"kind":"state","value":{"kind":"after","value":{"kind":"container","value":"test:model:container#one"}}}),
            serde_json::json!({"kind":"state","value":{"kind":"after","value":{"kind":"container","value":"test:placed:container#one"}}}),
            "test:model:container#one",
        ),
        (
            serde_json::json!({"kind":"tool","value":"test:model:feature#one"}),
            serde_json::json!({"kind":"tool","value":"test:placed:feature#one"}),
            "test:model:feature#one",
        ),
    ] {
        for configuration in [false, true] {
            let mut source = serde_json::json!({"of":source});
            let mut expected = serde_json::json!({"of":expected});
            let mut targets = vec![target.to_owned()];
            if configuration {
                source["configuration"] = "test:model:configuration#one".into();
                expected["configuration"] = "test:placed:configuration#one".into();
                targets.insert(0, "test:model:configuration#one".to_owned());
            }
            let state: StateRef =
                cadmpeg_core::serde_erase::json::from_value(source.clone()).unwrap();
            let mut seen = Vec::new();
            state
                .visit_identity_references(&ctx, &mut |id| {
                    seen.push(id.to_owned());
                    Ok(())
                })
                .unwrap();
            assert_eq!(seen, targets);
            let rewrite = |id: &str| Ok(id.replace("test:model:", "test:placed:"));
            let mut map = IdentityMap::new(rewrite);
            let state = state.rewrite_identities(&ctx, &mut map).unwrap();
            map.finish().unwrap();
            assert_eq!(
                cadmpeg_core::serde_erase::json::to_value(state).unwrap(),
                expected
            );
        }
    }
}

#[test]
fn construction_graph_walks_nested_selections_bindings_and_configuration_keys() {
    let literal = "test:model:parameter#one";
    let selection: OrderedSelection = cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
        "at":{"kind":"input"},
        "picks":{"kind":"named","value":[{
            "target":{"kind":"datum","value":{"feature":"test:model:feature#datum"}},
            "context":{"kind":"definition","value":{"definition":"test:model:definition#part","configuration":{"kind":"local","value":"test:model:configuration#one"}}},
            "native":{"codec":"f3d","value":{"kind":"text","value":literal}},
            "domain":{"kind":"limits","value":{"start":{"target":{"kind":"feature","value":"test:model:feature#limit"}}}}
        }]},
        "native":[literal]
    })).unwrap();
    let point = Construction::<kinds::Point> {
        inputs: vec![ConstructionInput {
            role: None,
            reference: Some(InputReference::Picks(selection)),
            value: None,
            reversed: None,
        }],
        after: vec![PostStep::Translate {
            axis: FrameAxis::X,
            distance: Dimension::Parameter(ParameterId::mint(literal).unwrap()),
        }],
        mode: Some(SourceMode {
            codec: crate::codec_id!("f3d"),
            method: NativeNameValue::Text(
                cadmpeg_core::text::NonBlankString::new(literal).unwrap(),
            ),
            native_options: std::collections::BTreeMap::from([(
                crate::design::names::CodecName::try_from(literal.to_owned()).unwrap(),
                NativeOption::Text(Dimension::Literal(
                    cadmpeg_core::text::NonBlankString::new(literal).unwrap(),
                )),
            )]),
            source_settings: NativeOptions::new(),
        }),
        resolved_in: std::collections::BTreeMap::from([(
            ConfigurationId::mint("test:model:configuration#two").unwrap(),
            crate::features::geometry::FinitePoint3::new(crate::math::Point3::new(1.0, 2.0, 3.0))
                .unwrap(),
        )]),
        solution: None,
        resolved: None,
    };
    let input = ConstructionInput {
        role: None,
        reference: Some(InputReference::Built(Box::new(AnyConstruction::Point(
            point,
        )))),
        value: Some(crate::design::construction::ScalarDimension::Length(
            Dimension::Literal(Length::new(4.0).unwrap()),
        )),
        reversed: None,
    };
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut seen = std::collections::BTreeSet::new();
    input
        .visit_identity_references(&ctx, &mut |id| {
            seen.insert(id.to_owned());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        seen,
        [
            "test:model:feature#datum",
            "test:model:definition#part",
            "test:model:configuration#one",
            "test:model:feature#limit",
            literal,
            "test:model:configuration#two"
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    );
    let replacements = std::collections::BTreeMap::from([(
        literal.to_owned(),
        "test:placed:parameter#one".to_owned(),
    )]);
    let replacements = TextReplacements::build(replacements.iter()).unwrap();
    let mut map = IdentityMap::new(|id: &str| Ok(id.replace("test:model:", "test:placed:")))
        .with_text_replacements(&replacements);
    let rewritten = input.rewrite_identities(&ctx, &mut map).unwrap();
    map.finish().unwrap();
    let text = cadmpeg_core::serde_erase::json::to_string(&rewritten).unwrap();
    assert!(text.contains("test:placed:feature#datum"));
    assert!(text.contains("test:placed:configuration#two"));
    let Some(InputReference::Built(built)) = rewritten.reference else {
        panic!("built input");
    };
    let AnyConstruction::Point(point) = *built else {
        panic!("point");
    };
    let mode = point.mode.unwrap();
    assert_eq!(
        mode.method,
        NativeNameValue::Text(cadmpeg_core::text::NonBlankString::new(literal).unwrap())
    );
    assert!(mode.native_options.contains_key(literal));
    let Some(InputReference::Picks(selection)) = &point.inputs[0].reference else {
        panic!("picks");
    };
    assert_eq!(selection.native.as_ref().unwrap().as_slice(), [literal]);
}
