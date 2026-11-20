// SPDX-License-Identifier: Apache-2.0
//! The upgrade of an earlier document to the semantics of the current format.
//!
//! `SolidCraft` reads an earlier design through one step for each format it
//! passes. Most steps change nothing that this decoder reads: a later format
//! adds a field and older designs keep its default. The steps below rewrite
//! the fields that the history decode reads: the appearance records, the joint
//! flip, the participants of a component design, the frame of the XZ origin
//! plane, the direction of a two-sided extrude, the sides of its second
//! distance, and the feature ids of pattern seeds and construction planes.
//! The upgrade works on a copy: the retained document stays as it was read.
//! The steps that only mark stored names (the legacy mark of a face name, the
//! piece order of a split) are not applied.

use serde_json::{json, Map, Value};

use crate::dialect::CURRENT_FORMAT;

/// A step of the upgrade and the format that introduced it.
type Step = (u32, fn(&mut Value));

/// The document `object` of format `version` upgraded to the current
/// format; `None` when it is current already.
pub(crate) fn upgrade(version: u32, object: &Map<String, Value>) -> Option<Map<String, Value>> {
    if version >= CURRENT_FORMAT {
        return None;
    }
    let mut value = Value::Object(object.clone());
    let steps: [Step; 7] = [
        (3, appearances),
        (4, joint_flips),
        (5, participants),
        (6, xz_frame),
        (28, two_sided_direction),
        (40, feature_ids),
        (51, second_follows_first),
    ];
    for (reaches, step) in steps {
        if version < reaches {
            step(&mut value);
        }
    }
    match value {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// Format 3: the appearances of a body are a look, not a bare colour.
fn appearances(value: &mut Value) {
    let Some(Value::Object(old)) = value.get("appearances") else {
        return;
    };
    if !old.values().all(Value::is_array) {
        return;
    }
    let bodies: Map<String, Value> = old
        .iter()
        .map(|(name, color)| {
            (
                name.clone(),
                json!({"name": "Custom", "color": color, "opacity": 1.0}),
            )
        })
        .collect();
    if let Some(object) = value.as_object_mut() {
        object.insert("appearances".to_owned(), json!({ "bodies": bodies }));
    }
}

/// Format 4: a joint mates the two origins as two faces meet, so the old
/// `flip` is the opposite sense.
fn joint_flips(value: &mut Value) {
    let Some(Value::Array(joints)) = value
        .get_mut("assembly")
        .and_then(|assembly| assembly.get_mut("joints"))
    else {
        return;
    };
    for joint in joints {
        if let Some(object) = joint.as_object_mut() {
            let flip = object.get("flip").and_then(Value::as_bool).unwrap_or(false);
            object.insert("flip".to_owned(), Value::Bool(!flip));
        }
    }
}

/// Format 5: a feature of a design with components names the components
/// that take part in it; an older feature takes part everywhere.
fn participants(value: &mut Value) {
    let has_components = value
        .get("components")
        .and_then(Value::as_array)
        .is_some_and(|components| !components.is_empty());
    if !has_components {
        return;
    }
    let Some(Value::Array(features)) = value.get_mut("features") else {
        return;
    };
    for feature in features {
        if let Some(object) = feature.as_object_mut() {
            if object.get("type").and_then(Value::as_str) != Some("sketch")
                && object.get("participants").is_none()
            {
                object.insert("participants".to_owned(), json!(["*"]));
            }
        }
    }
}

/// Format 6: the XZ origin plane has the normal +Y; an older reference keeps
/// its frame, with the x axis along +X and the y axis along +Z.
fn xz_frame(value: &mut Value) {
    match value {
        Value::Object(object) => {
            let old = object.get("type").and_then(Value::as_str) == Some("origin")
                && object
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| {
                        name.eq_ignore_ascii_case("XZ") || name.eq_ignore_ascii_case("FRONT")
                    });
            if old {
                // The marker says the frame came from the upgrade, not the document.
                *value = json!({"type": "custom", "migrated": "XZ", "plane": {
                    "origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}});
            } else {
                object.values_mut().for_each(xz_frame);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(xz_frame),
        _ => {}
    }
}

/// The features of `value` as a list of mutable records.
fn features(value: &mut Value) -> impl Iterator<Item = &mut Value> {
    value
        .get_mut("features")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
}

/// Format 28: a two-sided extrude follows its direction; an older one
/// ignored a negative direction, so the step makes it positive.
fn two_sided_direction(value: &mut Value) {
    for feature in features(value) {
        if !matches!(
            feature.get("type").and_then(Value::as_str),
            Some("extrude" | "surface_extrude")
        ) {
            continue;
        }
        if let Some(extent) = feature.get_mut("extent").and_then(Value::as_object_mut) {
            if extent.get("distance2").is_some_and(|d| !d.is_null())
                && extent.get("direction").and_then(Value::as_str) == Some("negative")
            {
                extent.insert("direction".to_owned(), Value::from("positive"));
            }
        }
    }
}

/// Format 51: the second distance of a two-sided extrude with entered
/// distances stays on its own side; an older one followed the first side
/// across the sketch plane, which the step marks.
fn second_follows_first(value: &mut Value) {
    for feature in features(value) {
        if !matches!(
            feature.get("type").and_then(Value::as_str),
            Some("extrude" | "surface_extrude")
        ) {
            continue;
        }
        let Some(extent) = feature.get_mut("extent").and_then(Value::as_object_mut) else {
            continue;
        };
        let plain = extent.get("distance2").is_some_and(|d| !d.is_null())
            && extent.get("to").is_none_or(Value::is_null)
            && extent.get("through_all").and_then(Value::as_bool) != Some(true)
            && extent.get("direction").and_then(Value::as_str) != Some("symmetric");
        if plain {
            extent.insert("second_follows_first".to_owned(), Value::Bool(true));
        }
    }
}

/// The id of the feature that `name` names: an id written as a number, then
/// a case-insensitive name.
fn find_feature(features: &[(u64, String, String)], name: &str) -> Option<u64> {
    name.parse::<u64>()
        .ok()
        .and_then(|id| features.iter().find(|(known, ..)| *known == id))
        .or_else(|| {
            features
                .iter()
                .find(|(_, known, _)| known.eq_ignore_ascii_case(name))
        })
        .map(|(id, ..)| *id)
}

/// The feature that a pattern seed named: an exact name on a feature that
/// is not a sketch or a construction plane, else [`find_feature`]; nothing is
/// id 0, which no feature has.
fn resolve_seed(features: &[(u64, String, String)], name: &str) -> u64 {
    features
        .iter()
        .find(|(_, known, kind)| {
            known == name && !matches!(kind.as_str(), "sketch" | "construction_plane")
        })
        .map(|(id, ..)| *id)
        .or_else(|| find_feature(features, name))
        .unwrap_or(0)
}

/// Rewrites every construction plane reference by name in `value` to carry
/// the feature's id. A revolve's axis has the same shape and names an axis.
fn rewrite_planes(value: &mut Value, features: &[(u64, String, String)], skip_axis: bool) {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("construction") {
                if let Some(Value::String(name)) = object.get("name") {
                    let id = find_feature(features, name).unwrap_or(0);
                    object.remove("name");
                    object.insert("feature".to_owned(), Value::from(id));
                }
                return;
            }
            for (key, item) in object.iter_mut() {
                if !(skip_axis && key == "axis") {
                    rewrite_planes(item, features, false);
                }
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| rewrite_planes(item, features, false)),
        _ => {}
    }
}

/// Format 40: the seeds of a pattern or mirror, and a construction plane
/// reference, are feature ids; an older design names them.
fn feature_ids(value: &mut Value) {
    let known: Vec<(u64, String, String)> = value
        .get("features")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|feature| {
            Some((
                feature.get("id")?.as_u64()?,
                feature.get("name")?.as_str()?.to_owned(),
                feature.get("type")?.as_str()?.to_owned(),
            ))
        })
        .collect();
    for feature in features(value) {
        let kind = feature
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if matches!(kind.as_str(), "pattern" | "mirror") {
            if let Some(Value::Array(seeds)) = feature.get_mut("features") {
                for seed in seeds {
                    if let Value::String(name) = seed {
                        *seed = Value::from(resolve_seed(&known, name));
                    }
                }
            }
        }
        rewrite_planes(
            feature,
            &known,
            matches!(kind.as_str(), "revolve" | "surface_revolve"),
        );
    }
    if let Some(object) = value.as_object_mut() {
        for (key, item) in object.iter_mut() {
            if key != "features" {
                rewrite_planes(item, &known, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upgraded(version: u32, value: &Value) -> Value {
        let Value::Object(object) = value else {
            panic!("an object");
        };
        Value::Object(upgrade(version, object).unwrap())
    }

    #[test]
    fn a_current_document_is_not_upgraded() {
        assert!(upgrade(CURRENT_FORMAT, &Map::new()).is_none());
    }

    #[test]
    fn each_step_applies_below_the_format_that_introduced_it() {
        let design = json!({
            "appearances": {"Body1": [1, 2, 3]},
            "assembly": {"joints": [{"flip": false}, {}]},
            "components": [{"id": 1}],
            "features": [
                {"id": 1, "name": "Boss", "type": "sketch",
                 "plane": {"type": "origin", "name": "XZ"}},
                {"id": 2, "name": "Boss", "type": "extrude",
                 "extent": {"distance": "10", "direction": "negative", "distance2": "5"}},
                {"id": 3, "name": "Mid", "type": "construction_plane"},
                {"id": 4, "name": "On", "type": "sketch",
                 "plane": {"type": "construction", "name": "MID"}},
                {"id": 5, "name": "Copies", "type": "pattern", "features": ["Boss", "gone"]}
            ]
        });
        let old = upgraded(1, &design);
        assert_eq!(
            old["appearances"]["bodies"]["Body1"]["color"],
            json!([1, 2, 3])
        );
        assert_eq!(
            old["assembly"]["joints"],
            json!([{"flip": true}, {"flip": true}])
        );
        assert_eq!(old["features"][1]["participants"], json!(["*"]));
        assert_eq!(old["features"][0]["plane"]["type"], "custom");
        let extent = &old["features"][1]["extent"];
        assert_eq!(extent["direction"], "positive");
        assert_eq!(extent["second_follows_first"], true);
        assert_eq!(
            old["features"][3]["plane"],
            json!({"type": "construction", "feature": 3})
        );
        assert_eq!(old["features"][4]["features"], json!([2, 0]));
        // A design of format 45 only gets the later steps.
        let later = upgraded(45, &design);
        assert_eq!(later["features"][0]["plane"]["type"], "origin");
        assert_eq!(later["features"][1]["extent"]["direction"], "negative");
        assert_eq!(later["features"][1]["extent"]["second_follows_first"], true);
        assert_eq!(later["features"][4]["features"], json!(["Boss", "gone"]));
    }
}
