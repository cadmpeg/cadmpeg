// SPDX-License-Identifier: Apache-2.0
//! Every Part Studio states its design layer (§5.1, §5.2, §5.10, §6), and a
//! CADIR write carries its features and sketches in that layer.
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;
use serde_json::Value;

use crate::reader::id;
use crate::test_support::fixture;
use crate::test_support::review::{
    cadir_round_trip, decode, design_operation, omitted, validate, wire,
};

#[test]
fn a_part_studio_without_a_thin_extrude_states_its_timeline() {
    let result = decode(&fixture("plate-fillet"));
    let ir = result.ir();
    let wire = wire(&result);
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one scope: {:?}", result.report().losses);
    };
    assert_eq!(
        json::to_value(&scope.definition).unwrap(),
        wire["model"]["product_definitions"][0]["id"]
    );
    assert_eq!(
        wire["design"]["containers"][0]["value"]["members"],
        json!(["Base sketch", "Plate", "Corner round"]
            .map(|name| json!({"kind":"feature","value":id("feature", name)})))
    );
    assert_eq!(
        wire["design"]["containers"][0]["value"]["rollback"],
        json!({"kind":"end"})
    );
    assert_eq!(
        design_operation(ir, "Base sketch"),
        json!({"kind":"sketch","value":id("sketch", "Base sketch")})
    );
    let plate = design_operation(ir, "Plate");
    assert_eq!(plate["kind"], "extrude");
    assert_eq!(
        plate["value"]["profile"]["picks"]["value"],
        json!([{"target":{"kind":"sketch","value":id("sketch", "Base sketch")}}])
    );
    assert_eq!(
        plate["value"]["span"],
        json!({"kind":"between","start":{"kind":"at_origin"},
            "end":{"kind":"amount","value":{"kind":"literal","value":2.0}}})
    );
    assert_eq!(
        plate["value"]["output"]["boolean"],
        json!({"kind":"new_body"})
    );
    let fillet = design_operation(ir, "Corner round");
    assert_eq!(fillet["kind"], "fillet");
    let set = &fillet["value"]["sets"][0];
    assert_eq!(
        set["section"]["size"],
        json!({"kind":"radius","value":{"kind":"literal","value":1.0}})
    );
    let [edge] = set["edges"]["picks"]["value"]
        .as_array()
        .unwrap()
        .as_slice()
    else {
        panic!("one edge pick: {set}");
    };
    assert_eq!(edge["target"], json!({"kind":"native","value":{}}));
    assert_eq!(
        edge["native"],
        json!({"codec":"onshape","value":{"kind":"text","value":"upright"}})
    );
    assert_eq!(edge["witness"]["point"], json!({"x":0.0,"y":0.0,"z":1.0}));
    assert_eq!(
        edge["witness"]["direction"],
        json!({"x":0.0,"y":0.0,"z":1.0})
    );
    assert_eq!(
        edge["witness"]["endpoints"],
        json!([{"x":0.0,"y":0.0,"z":0.0},{"x":0.0,"y":0.0,"z":2.0}])
    );
    assert!(!result.report().losses.iter().any(|loss| {
        crate::loss::OnshapeLossCode::DesignOperationUnresolved.matches(loss)
            && loss.message.ends_with("subject: Corner round")
    }));
    let names: Vec<_> = ir.design.names.entries().iter().collect();
    let [(cadmpeg_ir::design::refs::EntityRef::Body(body), entry)] = names.as_slice() else {
        panic!("one body name: {names:?}");
    };
    assert_eq!(body, &ir.geometry.bodies[0].id);
    assert_eq!(
        json::to_value(&entry.name).unwrap(),
        json!({"kind":"generated","value":{"kind":"body","feature":id("feature", "Plate"),"role":{"kind":"body"}}})
    );
    assert_eq!(ir.design.sketches.len(), 1);
    // The four lines, and the two ends of the line the length constraint
    // dimensions.
    assert_eq!(ir.design.sketch_entities.len(), 6);
    assert_eq!(
        ir.design.sketch_constraints.len(),
        ir.model.sketch_constraints.len()
    );
    validate(result);
}

#[test]
fn a_cadir_write_carries_the_restated_features_and_sketches() {
    let result = decode(&fixture("plate-fillet"));
    let (written, losses) = cadir_round_trip(&result);
    assert_eq!(written.design.features, result.ir().design.features);
    assert_eq!(written.design.sketches, result.ir().design.sketches);
    assert_eq!(
        written.design.sketch_entities,
        result.ir().design.sketch_entities
    );
    let model = &result.ir().model;
    for restated in model
        .features
        .iter()
        .map(|feature| feature.id.as_str())
        .chain(model.sketches.iter().map(|sketch| sketch.id.as_str()))
        .chain(
            model
                .sketch_entities
                .iter()
                .map(|entity| entity.id().as_str()),
        )
    {
        assert!(
            omitted(&losses, restated).is_empty(),
            "{restated}: {losses:#?}"
        );
    }
}

#[test]
fn a_revolve_about_a_sketch_line_states_its_axis_pick() {
    let result = decode(&fixture("revolve"));
    let ring = design_operation(result.ir(), "Ring");
    assert_eq!(ring["kind"], "revolve", "{:?}", result.report().losses);
    assert_eq!(
        ring["value"]["span"],
        json!({"kind":"between","start":{"kind":"at_origin"},"end":{"kind":"through_all"}})
    );
    let axis = &ring["value"]["axis"];
    assert_eq!(
        axis["inputs"][0]["role"],
        json!({"subject":"whole","relation":{"kind":"coincident"}})
    );
    assert_eq!(
        axis["inputs"][0]["reference"]["value"]["picks"]["value"][0]["target"],
        json!({"kind":"sketch_entity","value":id("entity", "Ring sketch/axis")})
    );
    assert_eq!(
        axis["resolved"]["direction"],
        json!({"x":0.0,"y":1.0,"z":0.0})
    );
    validate(result);
}

fn variable_depth() -> Vec<u8> {
    crate::test_support::projection::with_variables(
        &fixture("plate-fillet"),
        &[crate::test_support::projection::variable(
            "width-variable",
            "width",
            "LENGTH",
            "lengthValue",
            "3 mm",
        )],
        |response| {
            for parameter in response["features"][1]["parameters"]
                .as_array_mut()
                .unwrap()
            {
                if parameter["parameterId"] == "depth" {
                    parameter["expression"] = json!("#width * 2");
                }
            }
        },
    )
}

#[test]
fn variables_and_owned_dimensions_are_design_parameters() {
    let result = decode(&variable_depth());
    let ir = result.ir();
    let wire = wire(&result);
    let parameter = |owner: Option<&str>| {
        wire["design"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p.pointer("/owner/value").and_then(|o| o.as_str()) == owner)
            .unwrap_or_else(|| panic!("{owner:?}: {:?}", result.report().losses))
            .clone()
    };
    let width = parameter(None);
    assert_eq!(width["id"], id("parameter", "width-variable"));
    assert_eq!(width["name"], "width");
    assert_eq!(
        width["definition"],
        json!({"kind":"expression","value":{"kind":"neutral","value":{"text":"3[mm]","refs":[],"measures":[]}}})
    );
    assert_eq!(width["value"], json!({"kind":"number","value":3.0}));
    let plate = id("feature", "Plate");
    let depth = parameter(Some(&plate));
    assert_eq!(
        depth["definition"]["value"]["value"],
        json!({"text":"$0 * 2","refs":[{"target":{"kind":"parameter","value":id("parameter", "width-variable")}}],"measures":[]})
    );
    assert_eq!(depth["value"], json!({"kind":"number","value":6.0}));
    assert_eq!(
        design_operation(ir, "Plate")["value"]["span"]["end"]["value"],
        json!({"kind":"parameter","value":depth["id"]})
    );
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { crate::loss::OnshapeLossCode::ParameterExpressionText.matches(loss) }));
    validate(result);
}

#[test]
fn a_cadir_write_carries_the_restated_parameters() {
    let result = decode(&variable_depth());
    let (written, losses) = cadir_round_trip(&result);
    assert_eq!(written.design.parameters, result.ir().design.parameters);
    assert_eq!(
        result.ir().design.parameters.len(),
        result.ir().model.parameters.len()
    );
    for parameter in &result.ir().model.parameters {
        assert!(
            omitted(&losses, parameter.id.as_str()).is_empty(),
            "{}: {losses:#?}",
            parameter.id
        );
    }
}

#[test]
fn sketch_constraints_are_design_relations_on_stated_points() {
    let result = decode(&fixture("plate-fillet"));
    let wire = wire(&result);
    let constraint = |native: &str| {
        wire["design"]["sketch_constraints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id("constraint", native))
            .unwrap()
            .clone()
    };
    let line = id("entity", "Base sketch/line0");
    assert_eq!(
        constraint("Base sketch/horizontal")["relation"],
        json!({"kind":"horizontal","value":{"a":{"kind":"entity","value":line}}})
    );
    let length = constraint("Base sketch/length");
    assert_eq!(length["relation"]["kind"], "distance");
    assert_eq!(
        length["relation"]["value"]["a"],
        json!({"kind":"entity","value":format!("{line}.start")})
    );
    assert_eq!(
        length["relation"]["value"]["b"],
        json!({"kind":"entity","value":format!("{line}.end")})
    );
    assert_eq!(
        length["relation"]["value"]["value"],
        json!({"kind":"parameter","value":id("parameter", "Base sketch/length")})
    );
    let end = wire["design"]["sketch_entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == format!("{line}.end"))
        .unwrap();
    assert_eq!(
        end["definition"],
        json!({"kind":"free","value":{"kind":"point","value":{"u":20.0,"v":0.0}}})
    );
    assert_eq!(end["roles"], json!([{"of":line,"role":{"kind":"end"}}]));
    validate(result);
}

#[test]
fn a_cadir_write_carries_the_restated_constraints() {
    let result = decode(&fixture("plate-fillet"));
    let (written, losses) = cadir_round_trip(&result);
    assert_eq!(
        written.design.sketch_constraints,
        result.ir().design.sketch_constraints
    );
    for constraint in &result.ir().model.sketch_constraints {
        assert!(
            omitted(&losses, constraint.id.as_str()).is_empty(),
            "{}: {losses:#?}",
            constraint.id
        );
    }
}

/// The plate-fillet bundle with its fillet replaced by a chamfer of the
/// parameters `parameters`.
fn chamfer(parameters: &Value) -> Vec<u8> {
    crate::test_support::review::edit(&fixture("plate-fillet"), |path, value| {
        if path.ends_with("/features.json") {
            let feature = &mut value["features"][2];
            feature["featureType"] = json!("chamfer");
            let mut rows = vec![json!({"parameterId":"entities","queries":[
                {"btType":"BTMIndividualQuery-138","deterministicIds":["upright"]}]})];
            rows.extend(parameters.as_array().unwrap().iter().cloned());
            feature["parameters"] = Value::Array(rows);
        }
    })
}

#[test]
fn a_chamfer_states_its_setbacks_and_flip() {
    let result = decode(&chamfer(&json!([
        {"parameterId":"chamferType","value":"EQUAL_OFFSETS"},
        {"parameterId":"width","expression":"0.5 mm"}
    ])));
    let operation = design_operation(result.ir(), "Corner round");
    assert_eq!(operation["kind"], "chamfer");
    let set = &operation["value"]["sets"][0];
    assert_eq!(
        set["size"],
        json!({"kind":"distance","value":{"kind":"literal","value":0.5}})
    );
    assert_eq!(set["flip"], json!({"kind":"literal","value":false}));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| crate::loss::OnshapeLossCode::ChamferSideAssumed.matches(loss)));
    validate(result);

    let result = decode(&chamfer(&json!([
        {"parameterId":"chamferType","value":"TWO_OFFSETS"},
        {"parameterId":"width1","expression":"0.5 mm"},
        {"parameterId":"width2","expression":"0.25 mm"},
        {"parameterId":"oppositeDirection","value":true}
    ])));
    let operation = design_operation(result.ir(), "Corner round");
    let set = &operation["value"]["sets"][0];
    assert_eq!(
        set["size"],
        json!({"kind":"distances","value":{"first":{"kind":"literal","value":0.5},
            "second":{"kind":"literal","value":0.25}}})
    );
    assert_eq!(set["flip"], json!({"kind":"literal","value":true}));
    assert!(result.report().losses.iter().any(|loss| {
        crate::loss::OnshapeLossCode::ChamferSideAssumed.matches(loss)
            && loss.message.ends_with("subject: Corner round")
    }));
    validate(result);
}

#[test]
fn tangent_propagation_is_a_rule_on_the_selected_edges() {
    for stored in [true, false] {
        let bytes = crate::test_support::review::edit(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("/bodydetails/2.json") {
                let body = &mut value["bodies"][0];
                body["vertices"] = json!([{"id":"low"},{"id":"high"}]);
                body["edges"][0]["vertices"] = json!(["low", "high"]);
            }
            if path.ends_with("/features.json") {
                value["features"][2]["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"parameterId":"tangentPropagation","value":stored}));
            }
        });
        let result = decode(&bytes);
        let fillet = design_operation(result.ir(), "Corner round");
        let picks = &fillet["value"]["sets"][0]["edges"]["picks"];
        assert_eq!(picks["kind"], "rule", "{fillet}");
        let [term] = picks["value"]["terms"].as_array().unwrap().as_slice() else {
            panic!("one term: {picks}");
        };
        assert_eq!(term["rule"], json!({"kind":"explicit"}));
        assert_eq!(
            term["seeds"][0]["native"]["value"]["value"],
            json!("upright")
        );
        assert_eq!(
            term["propagation"],
            if stored {
                json!({"kind":"connected","value":{"continuity":{"kind":"g1"}}})
            } else {
                json!({"kind":"off"})
            }
        );
        assert!(picks["value"].get("resolved").is_none());
        validate(result);
    }
}

#[test]
fn holes_state_their_bore_head_extent_and_thread() {
    let result = decode(&fixture("hole-forms"));
    let ir = result.ir();
    let counterbore = design_operation(ir, "Counterbore");
    assert_eq!(counterbore["kind"], "hole");
    assert_eq!(
        counterbore["value"]["positions"]["kind"], "points",
        "{counterbore}"
    );
    assert_eq!(counterbore["value"]["shape"]["head"]["kind"], "counterbore");
    assert_eq!(
        counterbore["value"]["extent"],
        json!({"kind":"through_all"})
    );
    let countersink = design_operation(ir, "Countersink");
    assert_eq!(countersink["value"]["shape"]["head"]["kind"], "countersink");
    let tapped = design_operation(ir, "Tapped");
    assert_eq!(tapped["kind"], "hole", "{:?}", result.report().losses);
    let thread = &tapped["value"]["thread"];
    assert_eq!(
        thread["major_diameter"],
        json!({"kind":"literal","value":4.0})
    );
    assert!(thread["standard"]["standard"].is_string(), "{thread}");
    assert!(tapped["value"]["standard"].is_null());
    validate(result);
}

/// The design parameter `native` of the decode of `bytes`, and the decode.
fn design_parameter(bytes: &[u8], native: &str) -> (Value, cadmpeg_ir::codec::DecodeResult) {
    let result = decode(bytes);
    let parameter = wire(&result)["design"]["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id("parameter", native))
        .unwrap_or_else(|| panic!("{native}: {:?}", result.report().losses))
        .clone();
    (parameter, result)
}

/// The plate-fillet bundle with the sketch length dimension stored as
/// `expression`.
fn sketch_length(expression: &str) -> Vec<u8> {
    crate::test_support::review::edit(&fixture("plate-fillet"), |path, value| {
        if path.ends_with("/features.json") {
            value["features"][0]["constraints"][1]["parameters"][1]["expression"] =
                json!(expression);
        }
    })
}

#[test]
fn a_sketch_dimension_keeps_the_unit_and_structure_of_its_expression() {
    for (expression, text, unit, value) in [
        ("1 in", "1[in]", "in", 25.4),
        ("10 mm + 10 mm", "10[mm] + 10[mm]", "mm", 20.0),
        ("2 cm", "2[cm]", "cm", 20.0),
    ] {
        let (parameter, result) =
            design_parameter(&sketch_length(expression), "Base sketch/length");
        assert_eq!(
            parameter["definition"],
            json!({"kind":"expression","value":{"kind":"neutral","value":{"text":text,"refs":[],"measures":[]}}}),
            "{expression}"
        );
        assert_eq!(
            parameter["unit"],
            json!([{"symbol":unit,"power":1}]),
            "{expression}"
        );
        let stored = parameter["value"]["value"].as_f64().unwrap();
        assert!((stored - value).abs() < 1e-9, "{expression}: {stored}");
        validate(result);
    }
}

#[test]
fn an_expression_the_reader_does_not_evaluate_keeps_its_source_text() {
    let (parameter, result) =
        design_parameter(&sketch_length("(10 + 10) mm"), "Base sketch/length");
    assert_eq!(parameter["definition"]["value"]["kind"], "source");
    assert_eq!(
        parameter["definition"]["value"]["value"]["tokens"],
        json!([{"kind":"text","value":"(10 + 10) mm"}])
    );
    assert!(parameter.get("unit").is_none());
    assert_eq!(parameter["value"], json!({"kind":"number","value":20.0}));
    validate(result);
}

#[test]
fn a_flat_chain_past_the_depth_cap_keeps_the_source_text() {
    let chain = vec!["1 mm"; 300].join(" + ");
    let bytes = crate::test_support::projection::with_variables(
        &fixture("plate-fillet"),
        &[crate::test_support::projection::variable(
            "long-variable",
            "long",
            "LENGTH",
            "lengthValue",
            &chain,
        )],
        |_| {},
    );
    let (parameter, result) = design_parameter(&bytes, "long-variable");
    assert_eq!(parameter["definition"]["value"]["kind"], "source");
    assert_eq!(
        parameter["definition"]["value"]["value"]["tokens"],
        json!([{"kind":"text","value":chain}])
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| crate::loss::OnshapeLossCode::ParameterExpressionText.matches(loss)));
}
