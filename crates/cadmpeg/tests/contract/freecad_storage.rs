// SPDX-License-Identifier: Apache-2.0
//! `FreeCAD` storage rules for §5.1(c) edits (#238 §5.5, #232 §5.2): each rule
//! is the value `FreeCAD` stores for an edited value it cannot hold, matching
//! the writer's `REDUCED_FIELDS` row and loss.

use cadmpeg_ir::codec::write::ReducedStorage;
use cadmpeg_test_support::cross_closure::ReducedProbe;
use serde_json::{json, Map, Value};

/// Distance within which a frame is orthonormal and right-handed.
const EPS_RIGID: f64 = 1.0e-9;

/// `TechDraw` dimension properties and the scalar type each stores.
const BOOL_PARAMETERS: [&str; 10] = [
    "AngleOverride",
    "Arbitrary",
    "ArbitraryTolerances",
    "EqualTolerance",
    "Inverted",
    "LockPosition",
    "ShowUnits",
    "TheoreticalExact",
    "UseActualArea",
    "Visibility",
];
const FLOAT_PARAMETERS: [&str; 8] = [
    "ExtensionAngle",
    "LineAngle",
    "OverTolerance",
    "UnderTolerance",
    "Rotation",
    "Scale",
    "X",
    "Y",
];
const INTEGER_PARAMETERS: [&str; 3] = ["Type", "MeasureType", "ScaleType"];
const LIST_PARAMETERS: [&str; 2] = ["ExpressionEngine", "References3D"];

fn probe(path: &'static str, loss: &'static str, normalize: fn(&Value) -> Value) -> ReducedProbe {
    ReducedProbe {
        path,
        storage: ReducedStorage::Normalized { loss },
        normalize: Some(normalize),
    }
}

/// The `FreeCAD` edit probes, exact paths before the fields and records that
/// contain them.
pub(crate) fn probes() -> Vec<ReducedProbe> {
    vec![
        probe(
            "model.parameters[].driving",
            "fcstd/parameter.driving-not-transferred",
            |_| json!(true),
        ),
        probe(
            "model.presentation_documents[].schema_version",
            "fcstd/presentation.schema-normalized",
            |_| json!(1),
        ),
        probe(
            "model.assembly_joints[].operands",
            "fcstd/joint.frame-not-rigid",
            joint_operands,
        ),
        probe(
            "model.sketches[].placement",
            "fcstd/sketch.placement-normalized",
            sketch_placement,
        ),
        probe(
            "model.sketches[].profiles",
            "fcstd/sketch.profile-orientation-derived",
            sketch_profiles,
        ),
        probe(
            "model.presentation_documents[].states",
            "fcstd/presentation.state-order-normalized",
            states,
        ),
        probe(
            "model.semantic_annotations[].parameters",
            "fcstd/drawing.parameter-not-transferred",
            annotation_parameters,
        ),
        probe(
            "model.occurrences[]",
            "fcstd/product.placement-not-rigid",
            occurrence,
        ),
        probe(
            "model.view_presentations[]",
            "fcstd/presentation.view-mode-not-transferred",
            view,
        ),
        probe(
            "model.appearances[]",
            "fcstd/appearance.name-not-transferred",
            appearance,
        ),
        probe(
            "model.appearance_bindings[]",
            "fcstd/appearance.channel-derived",
            binding,
        ),
        probe(
            "model.semantic_annotations[]",
            "fcstd/annotation.position-planar",
            annotation,
        ),
    ]
}

type Matrix = [[f64; 4]; 3];

fn matrix(value: &Value) -> Option<Matrix> {
    let rows = value.as_array()?;
    let mut matrix = [[0.0; 4]; 3];
    for (row, items) in matrix.iter_mut().zip(rows) {
        let items = items.as_array()?;
        for (slot, item) in row.iter_mut().zip(items) {
            *slot = item.as_f64()?;
        }
    }
    (rows.len() == 3).then_some(matrix)
}

fn proper_rotation(columns: [[f64; 3]; 3]) -> bool {
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let orthonormal = (0..3).all(|i| {
        (0..3).all(|j| {
            let expected = if i == j { 1.0 } else { 0.0 };
            (dot(columns[i], columns[j]) - expected).abs() <= EPS_RIGID
        })
    });
    let [a, b, c] = columns;
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    orthonormal && (dot(cross, c) - 1.0).abs() <= EPS_RIGID
}

fn columns(matrix: &Matrix) -> [[f64; 3]; 3] {
    std::array::from_fn(|column| std::array::from_fn(|row| matrix[row][column]))
}

/// The rotation of `matrix` with its axis scales removed, as the writer
/// splits a placement.
fn rotation(matrix: &Matrix) -> Option<[[f64; 3]; 3]> {
    let mut columns = columns(matrix);
    for column in &mut columns {
        let scale = column.iter().map(|value| value * value).sum::<f64>().sqrt();
        if !scale.is_finite() || scale == 0.0 {
            return None;
        }
        for value in column.iter_mut() {
            *value /= scale;
        }
    }
    if !proper_rotation(columns) {
        for value in &mut columns[2] {
            *value = -*value;
        }
    }
    proper_rotation(columns).then_some(columns)
}

fn placed(rotation: [[f64; 3]; 3], matrix: &Matrix) -> Value {
    json!((0..3)
        .map(|row| {
            vec![
                rotation[0][row],
                rotation[1][row],
                rotation[2][row],
                matrix[row][3],
            ]
        })
        .collect::<Vec<_>>())
}

const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// A Placement is rigid: any other frame keeps its translation.
fn rigid_frame(value: &Value) -> Value {
    match matrix(value) {
        Some(frame) if proper_rotation(columns(&frame)) => value.clone(),
        Some(frame) => placed(IDENTITY, &frame),
        None => value.clone(),
    }
}

/// A link stores a rotation and an axis scale; any other object a rotation.
fn occurrence(record: &Value) -> Value {
    let mut record = record.clone();
    let link = record.get("link").is_some();
    if let Some(transform) = record.get_mut("transform") {
        if let Some(frame) = matrix(transform) {
            if !proper_rotation(columns(&frame)) {
                *transform = match rotation(&frame) {
                    Some(_) if link => transform.clone(),
                    Some(rotation) => placed(rotation, &frame),
                    None => placed(IDENTITY, &frame),
                };
            }
        }
    }
    record
}

fn joint_operands(value: &Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, child)| {
                    let child = match key.as_str() {
                        "frame" => rigid_frame(child),
                        "offset_frames" => Value::Array(
                            child
                                .as_array()
                                .into_iter()
                                .flatten()
                                .map(rigid_frame)
                                .collect(),
                        ),
                        _ => joint_operands(child),
                    };
                    (key.clone(), child)
                })
                .collect::<Map<_, _>>(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(joint_operands).collect()),
        other => other.clone(),
    }
}

/// A Placement stores unit frame directions.
fn sketch_placement(value: &Value) -> Value {
    let mut placement = value.clone();
    for key in ["normal", "u_axis"] {
        let Some(vector) = placement
            .pointer_mut(&format!("/frame/{key}"))
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        let components = ["x", "y", "z"].map(|axis| vector.get(axis).and_then(Value::as_f64));
        let [Some(x), Some(y), Some(z)] = components else {
            continue;
        };
        let length = x.hypot(y).hypot(z);
        for (axis, component) in ["x", "y", "z"].into_iter().zip([x, y, z]) {
            vector.insert(axis.to_owned(), json!(component / length));
        }
    }
    placement
}

/// A one-entity profile takes the orientation of its geometry.
fn sketch_profiles(value: &Value) -> Value {
    let mut profiles = value.clone();
    for profile in profiles.as_array_mut().into_iter().flatten() {
        if let Some([only]) = profile.as_array_mut().map(Vec::as_mut_slice) {
            only["reversed"] = json!(false);
        }
    }
    profiles
}

/// GUI states are stored in order at consecutive positions.
fn states(value: &Value) -> Value {
    let mut states = value.as_array().cloned().unwrap_or_default();
    states.sort_by_key(|state| state.get("order").and_then(Value::as_u64));
    for (position, state) in states.iter_mut().enumerate() {
        state["order"] = json!(position);
    }
    Value::Array(states)
}

/// An enumeration stores its selected index.
fn view(record: &Value) -> Value {
    let mut record = record.clone();
    if let Some(fields) = record.as_object_mut() {
        for key in ["display_mode", "selection_style"] {
            if fields
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|text| text.parse::<i64>().is_err())
            {
                fields.remove(key);
            }
        }
    }
    record
}

/// A `FreeCAD` appearance has no name.
fn appearance(record: &Value) -> Value {
    let mut record = record.clone();
    if let Some(fields) = record.as_object_mut() {
        fields.remove("name");
    }
    record
}

/// The display precedence follows the color lane and the element kind.
fn binding(record: &Value) -> Value {
    let mut record = record.clone();
    let list_entry = record
        .get("appearance")
        .and_then(Value::as_str)
        .and_then(|id| id.split_once('#'))
        .is_some_and(|(_, key)| key.contains(':'));
    let precedence = match (
        record.get("object_type").and_then(Value::as_str),
        record.pointer("/target/kind").and_then(Value::as_str),
    ) {
        (Some("ViewProvider ShapeAppearance"), Some("body")) => "shape_over_object",
        (Some("ViewProvider ShapeAppearance" | "ViewProvider Face"), _) => "face_over_object",
        (Some("ViewProvider Edge"), _) if list_entry => "edge_array_over_line",
        (Some("ViewProvider Edge"), _) => "edge_over_object",
        (Some("ViewProvider Vertex"), _) if list_entry => "vertex_array_over_point",
        (Some("ViewProvider Vertex"), _) => "vertex_over_object",
        _ => return record,
    };
    if record.get("channels").is_some() {
        record["channels"] = json!({ "precedence": precedence });
    }
    record
}

/// The leading decimal number of `text`, as C `strtol` or `strtod` reads it,
/// or zero.
fn leading_number(text: &str, fractional: bool) -> String {
    let bytes = text.as_bytes();
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count()
    };
    let integral = digits(end);
    end += integral;
    let mut fraction = 0;
    if fractional && bytes.get(end) == Some(&b'.') {
        fraction = digits(end + 1);
        end += 1 + fraction;
    }
    if integral + fraction == 0 {
        return "0".to_owned();
    }
    if fractional && matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let exponent = digits(end + 1 + sign);
        if exponent > 0 {
            end += 1 + sign + exponent;
        }
    }
    text[..end].to_owned()
}

/// A dimension property stores the value `FreeCAD` restores from the text: a
/// Bool is true only for `true`, a number is its leading numeric text, and a
/// list that does not parse is empty.
fn annotation_parameters(value: &Value) -> Value {
    let mut parameters = value.as_object().cloned().unwrap_or_default();
    for (key, text) in &mut parameters {
        let Some(current) = text.as_str() else {
            continue;
        };
        let key = key.as_str();
        let stored = if BOOL_PARAMETERS.contains(&key) {
            if current == "true" { "true" } else { "false" }.to_owned()
        } else if FLOAT_PARAMETERS.contains(&key) && current.parse::<f64>().is_err() {
            leading_number(current, true)
        } else if INTEGER_PARAMETERS.contains(&key) && current.parse::<i64>().is_err() {
            leading_number(current, false)
        } else if LIST_PARAMETERS.contains(&key)
            && serde_json::from_str::<Vec<Value>>(current).is_err()
        {
            "[]".to_owned()
        } else {
            continue;
        };
        *text = Value::String(stored);
    }
    Value::Object(parameters)
}

/// A `TechDraw` annotation lies on its page.
fn annotation(record: &Value) -> Value {
    let mut record = record.clone();
    let techdraw = record
        .get("runtime_type")
        .and_then(Value::as_str)
        .is_some_and(|runtime| runtime.starts_with("TechDraw::"));
    if techdraw {
        if let Some(position) = record
            .get_mut("position")
            .and_then(Value::as_array_mut)
            .filter(|position| position.len() == 3)
        {
            position[2] = json!(0.0);
        }
    }
    record
}
