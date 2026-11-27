// SPDX-License-Identifier: Apache-2.0
//! Every Part Studio states its design layer (§5.1, §5.2, §5.10, §6), and a
//! CADIR write carries its features and sketches in that layer.
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;

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
