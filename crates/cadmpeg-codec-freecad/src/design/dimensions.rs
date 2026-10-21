// SPDX-License-Identifier: Apache-2.0
//! Source-property identities bound into operation dimensions.

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DesignParameter, Dimension, DimensionValue, Expression, ExpressionToken, FeatureDefinition,
    FeatureId, FeatureOperation, ParameterId,
};
use serde_json::Value;
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

/// Each mapping names a persistence operand, rather than comparing scalar
/// values. Equal dimensions on two different source properties stay distinct.
fn operand(
    operation: &str,
    path: &[String],
    parameters: &BTreeMap<&str, &ParameterId>,
) -> Option<&'static str> {
    let field = if path.last()?.as_str() == "value" {
        path.get(path.len().checked_sub(2)?)?.as_str()
    } else {
        path.last()?.as_str()
    };
    let second = path
        .iter()
        .any(|part| matches!(part.as_str(), "second" | "side_two"))
        || (operation == "pattern"
            && path
                .windows(2)
                .any(|parts| parts[0] == "stages" && parts[1] == "1"));
    match (operation, field) {
        ("extrude", "length") => Some(if second { "Length2" } else { "Length" }),
        ("extrude", "draft") => Some(if second { "TaperAngle2" } else { "TaperAngle" }),
        ("extrude", "offset") => Some("Offset"),
        ("revolve", "angle") => Some(if second { "Angle2" } else { "Angle" }),
        ("revolve", "offset") => Some("Offset"),
        ("block", "0") if path.iter().any(|part| part == "dimensions") => Some("Length"),
        ("block", "1") if path.iter().any(|part| part == "dimensions") => Some("Width"),
        ("block", "2") if path.iter().any(|part| part == "dimensions") => Some("Height"),
        ("primitive", "length") => Some("Length"),
        ("primitive", "width") => Some("Width"),
        ("primitive", "height") => Some("Height"),
        ("primitive", "radius") => Some("Radius"),
        ("primitive", "radius1") => Some("Radius1"),
        ("primitive", "radius2") => Some("Radius2"),
        ("primitive", "major_radius") => Some("Radius1"),
        ("primitive", "minor_radius") => Some("Radius2"),
        ("primitive", "x_radius") => Some("Radius2"),
        ("primitive", "y_radius") => Some("Radius3"),
        ("primitive", "z_radius") => Some("Radius1"),
        ("primitive", "angle") => Some("Angle"),
        ("primitive", "latitude1") => Some("Angle1"),
        ("primitive", "latitude2") => Some("Angle2"),
        ("primitive", "longitude") => Some("Angle3"),
        ("primitive", "circumradius") => Some("Circumradius"),
        ("primitive", "sides") => Some("Polygon"),
        ("primitive", "xmin") => Some("Xmin"),
        ("primitive", "ymin") => Some("Ymin"),
        ("primitive", "zmin") => Some("Zmin"),
        ("primitive", "xmax") => Some("Xmax"),
        ("primitive", "ymax") => Some("Ymax"),
        ("primitive", "zmax") => Some("Zmax"),
        ("primitive", "x2min") => Some("X2min"),
        ("primitive", "z2min") => Some("Z2min"),
        ("primitive", "x2max") => Some("X2max"),
        ("primitive", "z2max") => Some("Z2max"),
        ("circular_arc", "radius") => Some("Radius"),
        ("elliptic_arc", "0") if path.iter().any(|part| part == "radii") => Some("MajorRadius"),
        ("elliptic_arc", "1") if path.iter().any(|part| part == "radii") => Some("MinorRadius"),
        ("circular_arc", "start_angle") if parameters.contains_key("Angle0") => Some("Angle0"),
        ("circular_arc", "end_angle") if parameters.contains_key("Angle0") => Some("Angle1"),
        ("circular_arc" | "elliptic_arc", "start_angle") => Some("Angle1"),
        ("circular_arc" | "elliptic_arc", "end_angle") => Some("Angle2"),
        ("regular_polygon_curve", "sides") => Some("Polygon"),
        ("regular_polygon_curve", "circumradius") => Some("Circumradius"),
        ("planar_patch", "length") => Some("Length"),
        ("planar_patch", "width") => Some("Width"),
        ("scale", "factor") => Some("UniformScale"),
        ("scale", "0") if path.iter().any(|part| part == "factors") => Some("XScale"),
        ("scale", "1") if path.iter().any(|part| part == "factors") => Some("YScale"),
        ("scale", "2") if path.iter().any(|part| part == "factors") => Some("ZScale"),
        ("loft", "max_degree") => Some("MaxDegree"),
        ("fillet", "radius") if path.iter().any(|part| part == "radius") => Some("Radius"),
        ("chamfer", "distance" | "distance_one" | "first") => Some("Size"),
        ("chamfer", "distance_two" | "second") => Some("Size2"),
        ("chamfer", "angle") => Some("Angle"),
        ("hole", "diameter") if path.iter().any(|part| part == "kind") => Some("HoleCutDiameter"),
        ("hole", "diameter") => Some("Diameter"),
        ("hole", "length") => Some("Depth"),
        ("hole", "depth") if path.iter().any(|part| part == "specification") => Some("ThreadDepth"),
        ("hole", "depth") => Some("HoleCutDepth"),
        ("hole", "pitch") => Some("ThreadPitch"),
        ("hole", "major_diameter") => Some("ThreadDiameter"),
        ("hole", "clearance") => Some("CustomThreadClearance"),
        ("hole", "taper_angle") => Some("TaperedAngle"),
        ("hole", "included_angle") => Some("DrillPointAngle"),
        ("hole", "angle") => Some("HoleCutCountersinkAngle"),
        ("offset" | "offset_shape", "distance") | ("thicken" | "shell", "thickness") => {
            Some("Value")
        }
        ("draft", "angle") => Some("Angle"),
        ("binder", "distance") => Some("Offset"),
        ("helical_sweep", "pitch") | ("helix", "pitch") => Some("Pitch"),
        ("helical_sweep", "height") => Some("Height"),
        ("helical_sweep", "radial_growth") => Some("Growth"),
        ("helical_sweep", "turns") => Some("Turns"),
        ("helical_sweep", "cone_angle") | ("helix", "cone_angle") => Some("Angle"),
        ("helical_sweep", "tolerance") => Some("Tolerance"),
        ("helix", "radius") => Some("Radius"),
        ("helix", "radial_growth") => Some("Growth"),
        ("helix", "revolutions") => Some("Rotations"),
        ("helix", "height") => Some("Height"),
        ("helix", "segment_turns") => Some("SegmentLength"),
        ("pattern", "count") => Some(if second {
            "Occurrences2"
        } else {
            "Occurrences"
        }),
        ("pattern", "distance") if path.iter().any(|part| part == "@span") => {
            Some(if second { "Length2" } else { "Length" })
        }
        ("pattern", "distance") if path.iter().any(|part| part == "@spacing") => {
            Some(if second { "Offset2" } else { "Offset" })
        }
        ("pattern", "angle") if path.iter().any(|part| part == "@span") => Some("Angle"),
        ("pattern", "angle") if path.iter().any(|part| part == "@spacing") => Some("Offset"),
        ("pattern", "final_factor") => Some("Factor"),
        _ => None,
    }
}

pub(super) fn bind(
    mut definition: FeatureDefinition,
    owner: &FeatureId,
    parameters: &mut Vec<DesignParameter>,
) -> Result<FeatureDefinition, CodecError> {
    bind_semantic_controls(&mut definition, owner, parameters)?;
    let mut ids: BTreeMap<&str, &ParameterId> = parameters
        .iter()
        .filter(|parameter| parameter.owner.as_ref() == Some(owner))
        .map(|parameter| (parameter.name.as_str(), &parameter.id))
        .collect();
    let mut value = serde_json::to_value(&definition).map_err(CodecError::malformed)?;
    let operation = if value["definition"] == "post_process" {
        if value["fuzzy_tolerance"]["kind"] == "explicit" {
            if let Some(id) = ids.get("FuzzyTolerance") {
                value["fuzzy_tolerance"]["value"] =
                    serde_json::json!({"kind":"parameter","value":id});
            }
        }
        &mut value["operation"]
    } else {
        &mut value
    };
    let tag = operation["definition"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if tag == "primitive" && operation["solid"]["kind"] == "ellipsoid" {
        if !ids.contains_key("Radius3") {
            if let Some(id) = ids.get("Radius2").copied() {
                ids.insert("Radius3", id);
            }
        }
    }
    if tag == "native" {
        if let Some(operands) = operation
            .get_mut("parameters")
            .and_then(Value::as_object_mut)
        {
            for (name, value) in operands {
                if let Some(id) = ids.get(name.as_str()) {
                    *value = serde_json::json!({"kind":"parameter","value":id});
                }
            }
        }
    } else {
        visit(operation, &tag, &mut Vec::new(), &ids);
    }
    serde_json::from_value(value).map_err(CodecError::malformed)
}

/// Native signs can encode direction separately from a neutral magnitude. Keep
/// the authored native driver and its formula intact: the operation consumes an
/// explicit computation, rather than a second independently editable value.
fn bind_semantic_controls(
    definition: &mut FeatureDefinition,
    owner: &FeatureId,
    parameters: &mut Vec<DesignParameter>,
) -> Result<(), CodecError> {
    let operation = match definition {
        FeatureDefinition::Operation(operation)
        | FeatureDefinition::PostProcess { operation, .. } => operation,
    };
    match operation {
        FeatureOperation::Shell {
            thickness: Some(dimension),
            ..
        } => {
            computed_control(dimension, owner, parameters, "Value", "magnitude", "abs(")?;
        }
        FeatureOperation::Draft {
            angle: Some(dimension),
            outward: Some(true),
            ..
        } => {
            computed_control(dimension, owner, parameters, "Angle", "oriented", "-(")?;
        }
        _ => {}
    }
    Ok(())
}

fn computed_control<Q: DimensionValue>(
    dimension: &mut Dimension<Q>,
    owner: &FeatureId,
    parameters: &mut Vec<DesignParameter>,
    source_name: &str,
    role: &str,
    prefix: &str,
) -> Result<(), CodecError> {
    let Some(value) = dimension
        .literal()
        .and_then(DimensionValue::to_parameter_value)
    else {
        return Ok(());
    };
    let Some(source) = parameters.iter().find(|parameter| {
        parameter.owner.as_ref() == Some(owner)
            && parameter.name == source_name
            && parameter.native_ref.is_some()
    }) else {
        return Ok(());
    };
    let source_id = source.id.clone();
    let expression_text = format!("{prefix}{source_name})");
    if let Some(computed) = parameters.iter_mut().find(|parameter| {
        parameter.id != source_id
            && parameter.owner.as_ref() == Some(owner)
            && parameter.value.as_ref() == Some(&value)
            && (parameter.expression.literal_text().as_deref() == Some(expression_text.as_str())
                || parameter.expression.tokens
                    == vec![
                        ExpressionToken::Text(prefix.into()),
                        ExpressionToken::Parameter(source_id.clone()),
                        ExpressionToken::Text(")".into()),
                    ])
    }) {
        // A native custom parameter can already express this exact K2
        // computation, including after source-less synthesis. Reuse its arena
        // identity instead of minting an equivalent second magnitude control.
        computed.driving = false;
        *dimension = Dimension::Parameter(computed.id.clone());
        return Ok(());
    }
    // At most N names/identities can already be occupied, so N+1 candidates
    // always include a free one without an unbounded search.
    let mut candidate = None;
    for index in 0..=parameters.len() {
        let suffix = if index == 0 {
            String::new()
        } else {
            format!("_{index}")
        };
        let name = format!("{source_name}_{role}{suffix}");
        let id = ParameterId::mint(format!("{source_id}:semantic-{role}{suffix}"))
            .map_err(CodecError::malformed)?;
        if parameters.iter().all(|parameter| {
            parameter.id != id
                && !(parameter.owner.as_ref() == Some(owner) && parameter.name == name)
        }) {
            candidate = Some((id, name));
            break;
        }
    }
    let (id, name) =
        candidate.ok_or_else(|| CodecError::malformed("no free semantic control identity"))?;
    let ordinal = parameters
        .iter()
        .filter(|parameter| parameter.owner.as_ref() == Some(owner))
        .map(|parameter| parameter.ordinal)
        .max()
        .map_or(Some(0), |ordinal| ordinal.checked_add(1))
        .ok_or_else(|| CodecError::malformed("semantic control ordinal overflow"))?;
    parameters.push(DesignParameter {
        id: id.clone(),
        owner: Some(owner.clone()),
        ordinal,
        name,
        expression: Expression {
            tokens: vec![
                ExpressionToken::Text(prefix.into()),
                ExpressionToken::Parameter(source_id),
                ExpressionToken::Text(")".into()),
            ],
        },
        driving: false,
        display: None,
        value: Some(value),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    *dimension = Dimension::Parameter(id);
    Ok(())
}

fn visit(
    value: &mut Value,
    operation: &str,
    path: &mut Vec<String>,
    parameters: &BTreeMap<&str, &ParameterId>,
) {
    if value["kind"] == "literal"
        && value
            .as_object()
            .is_some_and(|object| object.len() == 2 && object.contains_key("value"))
    {
        if let Some(id) = operand(operation, path, parameters).and_then(|name| parameters.get(name))
        {
            *value = serde_json::json!({"kind":"parameter","value":id});
        }
        return;
    }
    match value {
        Value::Object(object) => {
            let form = object
                .get("kind")
                .and_then(Value::as_str)
                .filter(|kind| matches!(*kind, "span" | "spacing"))
                .map(|kind| format!("@{kind}"));
            if let Some(form) = &form {
                path.push(form.clone());
            }
            for (key, value) in object {
                path.push(key.clone());
                visit(value, operation, path, parameters);
                path.pop();
            }
            if form.is_some() {
                path.pop();
            }
        }
        Value::Array(array) => {
            for (index, value) in array.iter_mut().enumerate() {
                path.push(index.to_string());
                visit(value, operation, path, parameters);
                path.pop();
            }
        }
        _ => {}
    }
}

/// Target controls use the current emitted dialect, independently of legacy
/// source property spellings. The path names the authored operand.
pub(crate) fn target_control(operation: &str, path: &[String]) -> Option<&'static str> {
    if operation == "post_process" && path.iter().any(|part| part == "fuzzy_tolerance") {
        return Some("FuzzyTolerance");
    }
    operand(operation, path, &BTreeMap::new())
}
