// SPDX-License-Identifier: Apache-2.0
//! Synthetic region mutations shared by reader and writer regressions.
use cadmpeg_core::serde_erase::json;
use serde_json::{json, Value};

#[allow(clippy::panic)]
fn compact(value: &Value) -> String {
    match value {
        Value::String(s) => format!("S{:x}${s}", s.len()),
        Value::Number(n) => {
            let n = n.as_i64().unwrap();
            if n < 0 {
                format!("D-{:x}", n.unsigned_abs())
            } else {
                format!("D{n:x}")
            }
        }
        Value::Array(a) => format!(
            "A{:x}{}",
            a.len(),
            a.iter().map(compact).collect::<String>()
        ),
        Value::Object(o) => format!(
            "M{:x}{}",
            o.len(),
            o.iter()
                .map(|(k, v)| compact(&json!(k)) + &compact(v))
                .collect::<String>()
        ),
        Value::Bool(b) => if *b { "T" } else { "F" }.to_owned(),
        Value::Null => panic!("synthetic region queries cannot contain null"),
    }
}

pub(crate) fn frame(response: &mut Value, divider: bool) {
    response["features"][0]["constraints"] = json!([]);
    let entities = response["features"][0]["entities"].as_array_mut().unwrap();
    entities.clear();
    for (i, (x, y, dx, dy)) in [
        (0.0, 0.0, 1.0, 0.0),
        (0.02, 0.0, 0.0, 1.0),
        (0.02, 0.02, -1.0, 0.0),
        (0.0, 0.02, 0.0, -1.0),
    ]
    .into_iter()
    .enumerate()
    {
        entities.push(
            json!({"btType":"BTMSketchCurveSegment-155","entityId":format!("frame{i}"),
            "startParam":0.0,"endParam":0.02,"geometry":{"btType":"BTCurveGeometryLine-117",
            "pntX":x,"pntY":y,"dirX":dx,"dirY":dy}}),
        );
    }
    entities.push(json!({"btType":"BTMSketchCurve-4","entityId":"hole","geometry":{
        "btType":"BTCurveGeometryCircle-115","radius":0.002,"xCenter":0.005,"yCenter":0.01,"xDir":1.0}}));
    if divider {
        entities.push(json!({"btType":"BTMSketchCurveSegment-155","entityId":"divider","startParam":0.0,
            "endParam":0.02,"geometry":{"btType":"BTCurveGeometryLine-117","pntX":0.01,"pntY":0.0,"dirY":1.0}}));
    }
    let native = response["features"][0]["featureId"].as_str().unwrap();
    let query = json!({"entityType":"FACE","historyType":"CREATION","queryType":"IMPRINT",
        "operationId":[format!("{native}.imprint")],"disambiguationData":[{"disambiguationType":"TOPOLOGY","entities":[[
        {"entityType":"EDGE","historyType":"CREATION","queryType":"IMPRINT","operationId":[format!("{native}.imprint")],
        "derivedFrom":{"entityType":"EDGE","historyType":"CREATION","queryType":"SKETCH_ENTITY",
        "operationId":[format!("{native}.wireOp")],"sketchEntityId":"hole"}},-1]]}]});
    response["features"][1]["parameters"][2]["queries"][0]["queryString"] = json!(format!(
        "query=qCompressed(1.0,{},id);",
        json::to_string(&format!("%{}", compact(&query))).unwrap()
    ));
}

pub(crate) fn arc_divider(response: &mut Value) {
    frame(response, false);
    response["features"][0]["entities"]
        .as_array_mut()
        .unwrap()
        .push(json!({
        "btType":"BTMSketchCurveSegment-155","entityId":"curved-divider",
        "startParam":-std::f64::consts::FRAC_PI_2,"endParam":std::f64::consts::FRAC_PI_2,
        "geometry":{"btType":"BTCurveGeometryCircle-115","xCenter":0.010,"yCenter":0.010,
            "radius":0.010,"xDir":1.0}}));
}

pub(crate) fn coincident_dividers(response: &mut Value) {
    frame(response, true);
    let entities = response["features"][0]["entities"].as_array_mut().unwrap();
    let mut duplicate = entities.last().unwrap().clone();
    duplicate["entityId"] = json!("overlapping-divider");
    duplicate["geometry"]["pntY"] = json!(0.02);
    duplicate["geometry"]["dirY"] = json!(-1.0);
    entities.push(duplicate);
}
