// SPDX-License-Identifier: Apache-2.0
//! Expected wires for records whose numeric fields are all authored dimensions.

pub(super) fn literal_fields(mut wire: serde_json::Value) -> serde_json::Value {
    fn literal(value: &mut serde_json::Value) {
        if value.is_number() {
            *value = serde_json::json!({"kind":"literal", "value":value.take()});
        }
    }
    for (key, value) in wire
        .as_object_mut()
        .expect("dimension fixture object")
        .iter_mut()
    {
        if let Some(values) = value.as_array_mut() {
            values.iter_mut().for_each(literal);
        } else {
            literal(value);
            if matches!(
                key.as_str(),
                "radius"
                    | "radius1"
                    | "radius2"
                    | "x_radius"
                    | "y_radius"
                    | "z_radius"
                    | "major_radius"
                    | "minor_radius"
                    | "circumradius"
            ) {
                *value = serde_json::json!({"kind":"radius", "value":value.take()});
            }
        }
    }
    wire
}

pub(super) fn value_mut<'a>(
    wire: &'a mut serde_json::Value,
    key: &str,
) -> &'a mut serde_json::Value {
    let radial = wire[key]["kind"] == "radius";
    if radial {
        &mut wire[key]["value"]["value"]
    } else {
        &mut wire[key]["value"]
    }
}
