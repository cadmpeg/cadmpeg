// SPDX-License-Identifier: Apache-2.0
//! F3D storage rules for §5.1(c) edits (#236, #209 §5.2): each rule is the
//! value F3D stores for an edited value it cannot hold, matching the writer's
//! `REDUCED_FIELDS` row and loss.

use cadmpeg_ir::codec::write::ReducedStorage;
use cadmpeg_test_support::cross_closure::ReducedProbe;
use serde_json::{json, Value};

/// Loss every F3D value normalization charges.
const VALUE_NORMALIZED: &str = "f3d/writer.value-normalized";

/// Localized feature-family names whose scopes store a suppression state.
const SUPPRESSIBLE_FAMILIES: [&str; 9] = [
    "Extrude",
    "Extrusion",
    "Extrusão",
    "Fillet",
    "Congé",
    "Abrundung",
    "Arredondamento",
    "Chamfer",
    "Chanfrein",
];

/// Operation tags of features whose scope stores a suppression state.
const SUPPRESSIBLE_DEFINITIONS: [&str; 4] = ["extrude", "fillet", "full_round_fillet", "chamfer"];

/// Localized feature-family names of sketch scopes.
const SKETCH_FAMILIES: [&str; 4] = ["Sketch", "Esquisse", "Skizze", "Esboço"];

/// Constraint kinds a sketch relation state bit selects, in bit order.
const POINT_RELATION_KINDS: [(u64, &str); 14] = [
    (0x1, "coincident"),
    (0x2, "collinear"),
    (0x4, "concentric"),
    (0x8, "equal_length"),
    (0x10, "parallel"),
    (0x20, "perpendicular"),
    (0x40, "horizontal"),
    (0x80, "vertical"),
    (0x100, "tangent"),
    (0x200, "curvature"),
    (0x400, "symmetry"),
    (0x800, "equal"),
    (0x1000, "midpoint"),
    (0x2000, "polygon"),
];

/// The successor reference a written creation timestamp names.
const TERMINAL_SUCCESSOR: &str = "f3d:brep:entity#-1";

/// Distance within which a placement's linear part is a proper rotation.
const EPS_RIGID: f64 = 1.0e-9;

fn probe(path: &'static str, normalize: fn(&Value) -> Value) -> ReducedProbe {
    ReducedProbe {
        path,
        storage: ReducedStorage::Normalized {
            loss: VALUE_NORMALIZED,
        },
        normalize: Some(normalize),
    }
}

/// The F3D edit probes, exact paths before the fields and records that
/// contain them.
pub(crate) fn probes() -> Vec<ReducedProbe> {
    vec![
        ReducedProbe {
            path: "model.features[].needs_recompute",
            storage: ReducedStorage::Normalized {
                loss: "export/evaluation.stale",
            },
            normalize: Some(|_| json!(false)),
        },
        probe("model.sketches[]", sketch),
        probe("model.sketch_constraints[]", sketch_constraint),
        probe("model.features[]", feature),
        probe("model.attributes[]", attribute),
        probe("model.occurrences[]", occurrence),
        probe("model.parameters[]", parameter),
        probe("model.assets[]", asset),
        probe("model.tessellations[]", tessellation),
    ]
}

/// The normalization of one declared F3D row.
pub(crate) fn normalizer(path: &str) -> Option<fn(&Value) -> Value> {
    probes()
        .into_iter()
        .find(|probe| probe.path == path)
        .and_then(|probe| probe.normalize)
}

/// The family name and family-local ordinal of an F3D neutral feature
/// identity: the length-prefixed stream and family components, then the
/// ordinal and the scope record (spec §3.1).
fn feature_family(id: &str) -> Option<(String, u64)> {
    let key = id.strip_prefix("f3d:model:feature#")?;
    let (length, rest) = key.split_once(':')?;
    let rest = rest.get(length.parse::<usize>().ok()?..)?;
    let (length, rest) = rest.split_once(':')?;
    let length = length.parse::<usize>().ok()?;
    let (family, rest) = (rest.get(..length)?, rest.get(length..)?);
    let (ordinal, _) = rest.split_once(':')?;
    Some((unescape(family)?, ordinal.parse().ok()?))
}

/// An identity component with each `%HH` escape replaced by its byte.
fn unescape(component: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut rest = component.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let hex = std::str::from_utf8(tail.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &tail[2..];
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}

/// A scope stores its family and ordinal, from which the reader derives the
/// name, and a suppression state only in the Extrude, Fillet and Chamfer
/// families.
fn feature(record: &Value) -> Value {
    let mut record = record.clone();
    if let Some((family, ordinal)) = record
        .get("id")
        .and_then(Value::as_str)
        .and_then(feature_family)
    {
        record["name"] = json!(format!("{family} {ordinal}"));
    }
    let suppressible = match record.get("source_tag").and_then(Value::as_str) {
        Some(tag) => SUPPRESSIBLE_FAMILIES.contains(&tag),
        None => record
            .pointer("/definition/definition")
            .and_then(Value::as_str)
            .is_some_and(|tag| SUPPRESSIBLE_DEFINITIONS.contains(&tag)),
    };
    if record.get("suppressed") == Some(&Value::Bool(true)) && !suppressible {
        record["suppressed"] = json!(false);
    }
    record
}

/// A creation timestamp writes its terminal successor reference.
fn attribute(record: &Value) -> Value {
    let mut record = record.clone();
    let Some(values) = record.get_mut("values").and_then(Value::as_array_mut) else {
        return record;
    };
    if values.iter().any(Value::is_null) {
        return record;
    }
    let family = values.iter().position(|value| {
        value.get("kind").and_then(Value::as_str) == Some("string")
            && value.get("value").and_then(Value::as_str) == Some("Timestamp_attrib_def")
    });
    let timestamp = family.is_some_and(|family| {
        values.get(family + 1) == Some(&json!({"kind": "integer", "value": 1}))
            && values
                .get(family + 2)
                .and_then(|value| value.get("kind"))
                .and_then(Value::as_str)
                == Some("float")
    });
    if let Some(first) = values.first_mut().filter(|_| timestamp) {
        let terminal = first.get("value").and_then(Value::as_str).is_some_and(|value| {
            value.ends_with(":entity#-1")
        });
        if first.get("kind").and_then(Value::as_str) == Some("reference") && !terminal {
            first["value"] = json!(TERMINAL_SUCCESSOR);
        }
    }
    record
}

type Rotation = [[f64; 3]; 3];

fn proper_rotation(columns: Rotation) -> bool {
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let [a, b, c] = columns;
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    columns
        .iter()
        .all(|axis| (dot(*axis, *axis) - 1.0).abs() <= EPS_RIGID)
        && dot(a, b).abs() <= EPS_RIGID
        && dot(a, c).abs() <= EPS_RIGID
        && dot(b, c).abs() <= EPS_RIGID
        && (dot(cross, c) - 1.0).abs() <= EPS_RIGID
}

/// An occurrence placement is rigid: a linear part that is not a proper
/// rotation stores the rotation of its unit columns, with the third column
/// reversed when that makes it right-handed, or the identity.
fn occurrence(record: &Value) -> Value {
    let mut record = record.clone();
    let Some(transform) = record.get_mut("transform") else {
        return record;
    };
    let Some(rows) = transform.as_array().filter(|rows| rows.len() == 3) else {
        return record;
    };
    let mut matrix = [[0.0; 4]; 3];
    for (row, items) in matrix.iter_mut().zip(rows) {
        let Some(items) = items.as_array().filter(|items| items.len() == 4) else {
            return record;
        };
        for (slot, item) in row.iter_mut().zip(items) {
            let Some(value) = item.as_f64() else {
                return record;
            };
            *slot = value;
        }
    }
    let mut columns: Rotation = std::array::from_fn(|column| std::array::from_fn(|row| matrix[row][column]));
    if proper_rotation(columns) {
        return record;
    }
    let unit = columns.iter_mut().all(|column| {
        let length = column.iter().map(|value| value * value).sum::<f64>().sqrt();
        if !length.is_finite() || length == 0.0 {
            return false;
        }
        for value in column.iter_mut() {
            *value /= length;
        }
        true
    });
    let rotation = if !unit {
        None
    } else if proper_rotation(columns) {
        Some(columns)
    } else {
        for value in &mut columns[2] {
            *value = -*value;
        }
        proper_rotation(columns).then_some(columns)
    }
    .unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    *transform = json!((0..3)
        .map(|row| vec![rotation[0][row], rotation[1][row], rotation[2][row], matrix[row][3]])
        .collect::<Vec<_>>());
    record
}

/// A parameter a non-sketch feature owns is driving.
fn parameter(record: &Value) -> Value {
    let mut record = record.clone();
    let feature_parameter = record
        .get("owner")
        .and_then(Value::as_str)
        .and_then(feature_family)
        .is_some_and(|(family, _)| !SKETCH_FAMILIES.contains(&family.as_str()));
    if feature_parameter {
        record["driving"] = json!(true);
    }
    record
}

/// A sketch placement stores unit axes. A legacy sketch container stores
/// its Design entity ID, `prefix_suffix`, as the sketch name; any other name
/// stores the generated ID of the container its identity names.
fn sketch(record: &Value) -> Value {
    let mut record = record.clone();
    if let Some(placement) = record.get_mut("placement") {
        *placement = sketch_placement(placement);
    }
    let entity_id = record
        .get("name")
        .and_then(Value::as_str)
        .and_then(|name| name.rsplit_once('_'))
        .is_some_and(|(_, suffix)| suffix.parse::<u64>().is_ok());
    let container = record
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| id.starts_with("f3d:model:sketch#"))
        .and_then(|id| id.rsplit_once('@'))
        .and_then(|(_, suffix)| suffix.parse::<u32>().ok());
    if let (Some(container), false) = (container, entity_id) {
        record["name"] = json!(format!("Sketch_{container}"));
    }
    record
}

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

/// An image asset's media type follows its entry-name extension.
fn asset(record: &Value) -> Value {
    let mut record = record.clone();
    let derived = record
        .get("name")
        .and_then(Value::as_str)
        .and_then(|name| std::path::Path::new(name).extension())
        .and_then(|extension| extension.to_str())
        .and_then(|extension| {
            if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
                Some("image/jpeg")
            } else if extension.eq_ignore_ascii_case("png") {
                Some("image/png")
            } else {
                None
            }
        });
    let Some(fields) = record.as_object_mut() else {
        return record;
    };
    match derived {
        Some(media_type) => {
            fields.insert("media_type".to_owned(), json!(media_type));
        }
        None => {
            fields.remove("media_type");
        }
    }
    record
}

/// A four-point parallel or concentric relation stores its state, its two
/// member and two return records: the reader derives the native kind from
/// the state, the operand fields from their positions, and each operand's
/// record from its member, whose first operand states that record.
fn sketch_constraint(record: &Value) -> Value {
    let mut record = record.clone();
    let Some(definition) = record.get_mut("definition") else {
        return record;
    };
    if definition.get("kind").and_then(Value::as_str) != Some("native") {
        return record;
    }
    let Some(state) = definition.get("native_state").and_then(Value::as_u64) else {
        return record;
    };
    let known = POINT_RELATION_KINDS
        .iter()
        .fold(0, |known, (bit, _)| known | bit);
    let entities: Vec<Value> = definition
        .get("entities")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(operands) = definition.get("operands").and_then(Value::as_array) else {
        return record;
    };
    if state & !known != 0 || state & (0x10 | 0x4) == 0 || entities.len() != 4 || operands.len() != 4
    {
        return record;
    }
    let kind = POINT_RELATION_KINDS
        .iter()
        .filter(|(bit, _)| state & bit != 0)
        .map(|(_, name)| *name)
        .collect::<Vec<_>>()
        .join("+");
    let mut first = std::collections::BTreeMap::<String, Value>::new();
    let operands: Vec<Value> = operands
        .iter()
        .zip(&entities)
        .enumerate()
        .map(|(ordinal, (operand, entity))| {
            let mut operand = operand.clone();
            operand["native_kind"] = json!("point");
            operand["field"] = json!({"name": if ordinal < 2 { "member" } else { "return" }});
            let index = first
                .entry(entity.to_string())
                .or_insert_with(|| operand.get("object_index").cloned().unwrap_or(Value::Null))
                .clone();
            operand["object_index"] = index;
            operand
        })
        .collect();
    definition["native_kind"] = json!(kind);
    definition["operands"] = json!(operands);
    record
}

/// A mesh container's first corner is implicit: a stored mesh references its
/// first and last vertex, and one whose corners miss either keeps only its
/// referenced vertices, in order.
fn tessellation(record: &Value) -> Value {
    let mut record = record.clone();
    let carries_vertex_rows = ["feature_edges", "texture_coordinates"].iter().any(|key| {
        record
            .get(*key)
            .and_then(Value::as_array)
            .is_some_and(|rows| !rows.is_empty())
    });
    let Some(mesh) = record.get_mut("mesh") else {
        return record;
    };
    if carries_vertex_rows
        || !matches!(
            mesh.get("kind").and_then(Value::as_str),
            Some("list" | "shaded_list")
        )
    {
        return record;
    }
    let (Some(vertices), Some(triangles)) = (
        mesh.get("vertices").and_then(Value::as_array).cloned(),
        mesh.get("triangles").and_then(Value::as_array).cloned(),
    ) else {
        return record;
    };
    let Some(corners) = triangles
        .iter()
        .flat_map(|triangle| triangle.as_array().cloned().unwrap_or_default())
        .map(|corner| corner.as_u64())
        .collect::<Option<std::collections::BTreeSet<u64>>>()
    else {
        return record;
    };
    let last = u64::try_from(vertices.len())
        .ok()
        .and_then(|count| count.checked_sub(1));
    if corners.first() == Some(&0) && corners.last() == last.as_ref() {
        return record;
    }
    let remap: std::collections::BTreeMap<u64, usize> =
        corners.iter().enumerate().map(|(new, old)| (*old, new)).collect();
    mesh["vertices"] = json!(corners
        .iter()
        .filter_map(|index| vertices.get(usize::try_from(*index).ok()?).cloned())
        .collect::<Vec<_>>());
    mesh["triangles"] = json!(triangles
        .iter()
        .map(|triangle| triangle
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|corner| remap.get(&corner.as_u64()?).copied())
            .collect::<Vec<_>>())
        .collect::<Vec<_>>());
    record
}
