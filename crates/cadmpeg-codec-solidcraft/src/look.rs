// SPDX-License-Identifier: Apache-2.0
//! Looks, materials, and visibility.
//!
//! A look is an appearance: its name, its colour and opacity, and its
//! physically based parameters. The document keeps one look for each body
//! name and one for each face, found by the face's persistent name. A look
//! binds to the bodies and faces it colours, once per distinct look. A body
//! that the document gives a material binds the material's name as the
//! `physical_material` channel; a user material is an appearance and a
//! physical material with its density. A hidden body is not visible. A look
//! of a whole component is retained without decoding. A named view is a
//! camera state of the document's presentation.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::topology::Body;
use serde_json::{json, Map, Value};

/// The identity of a record of `kind` keyed by `key`.
fn id(kind: &'static str, key: &str) -> String {
    let namespace = match kind {
        "appearance" => cadmpeg_ir::identity_namespace!("solidcraft", "design", "appearance"),
        "binding" => {
            cadmpeg_ir::identity_namespace!("solidcraft", "design", "appearance-binding")
        }
        _ => cadmpeg_ir::identity_namespace!("solidcraft", "design", "physical-material"),
    };
    cadmpeg_ir::features::FeatureId::compose(&namespace, IdentityKey::encode_segment(key))
        .into_string()
}

fn malformed(error: impl std::fmt::Display) -> CodecError {
    CodecError::malformed(format_args!("solidcraft look record: {error}"))
}

/// The wire record of the appearance of `look`, with the identity key that
/// the same look always gets.
fn appearance(look: &Value) -> Option<(String, Value)> {
    let channels = look.get("color")?.as_array()?;
    let [r, g, b] = channels.as_slice() else {
        return None;
    };
    let channel = |value: &Value| Some(f64::from(u8::try_from(value.as_u64()?).ok()?) / 255.0);
    let opacity = look.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
    let name = look.get("name").and_then(Value::as_str);
    let mut properties = Map::new();
    if let Some(pbr) = look.get("pbr").and_then(Value::as_object) {
        for (key, value) in pbr {
            if value.is_f64() {
                properties.insert(key.clone(), value.clone());
            }
        }
    }
    let mut record = json!({
        "base_color": {"r": channel(r)?, "g": channel(g)?, "b": channel(b)?, "a": opacity},
        "properties": properties,
        "schema": "solidcraft look",
        "textures": [],
    });
    if let Some(name) = name {
        record["name"] = json!(name);
    }
    Some((look.to_string(), record))
}

/// The appearances, bindings, and physical materials of one decode.
pub(crate) struct Looks<'a> {
    /// The document's appearances: bodies by name, and faces.
    appearances: Option<&'a Value>,
    /// The document's material of each body name.
    materials: Option<&'a Map<String, Value>>,
    /// The document's user materials.
    user_materials: &'a [Value],
    /// The names of the hidden bodies.
    hidden: BTreeSet<&'a str>,
    /// Whether the document records visibility.
    visibility: bool,
    /// The appearance records by identity.
    records: BTreeMap<String, Value>,
    /// The binding records.
    bindings: Vec<Value>,
}

impl<'a> Looks<'a> {
    pub(crate) fn new(object: Option<&'a Map<String, Value>>) -> Self {
        let get = |key: &str| object.and_then(|object| object.get(key));
        let visibility = get("visibility");
        Self {
            appearances: get("appearances"),
            materials: get("materials").and_then(Value::as_object),
            user_materials: get("user_materials")
                .and_then(Value::as_array)
                .map_or(&[][..], Vec::as_slice),
            hidden: visibility
                .and_then(|visibility| visibility.get("hidden_bodies"))
                .and_then(Value::as_array)
                .map(|names| names.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default(),
            visibility: visibility.is_some(),
            records: BTreeMap::new(),
            bindings: Vec::new(),
        }
    }

    /// The identity of the appearance of `look`, recording it.
    fn record(&mut self, look: &Value) -> Option<String> {
        let (key, record) = appearance(look)?;
        let appearance_id = id("appearance", &key);
        let mut record = record;
        record["id"] = json!(appearance_id);
        self.records.entry(appearance_id.clone()).or_insert(record);
        Some(appearance_id)
    }

    /// The identity of the appearance of the material `name`.
    fn material(&mut self, name: &str) -> String {
        let appearance_id = id("appearance", &format!("material:{name}"));
        if !self.records.contains_key(&appearance_id) {
            let user = self
                .user_materials
                .iter()
                .find(|user| user.get("name").and_then(Value::as_str) == Some(name));
            let mut record = user
                .and_then(|user| user.get("appearance"))
                .and_then(appearance)
                .map_or_else(
                    || json!({"properties": {}, "textures": []}),
                    |(_, record)| record,
                );
            record["id"] = json!(appearance_id);
            record["name"] = json!(name);
            record["physical_token"] = json!(name);
            record["schema"] = json!("solidcraft material");
            self.records.insert(appearance_id.clone(), record);
        }
        appearance_id
    }

    fn bind(&mut self, key: &str, target: &Value, appearance: &str, channels: &Value) {
        self.bindings.push(json!({
            "id": id("binding", key),
            "target": {"entity": target},
            "appearance": appearance,
            "channels": channels,
        }));
    }

    /// Binds the looks of one body and of its faces, and sets whether it is
    /// visible. `faces` are the ids of its faces in face order and
    /// `face_names` their persistent names.
    pub(crate) fn body(&mut self, body: &mut Body, faces: &[String], face_names: &[String]) {
        let Some(name) = body.name.clone() else {
            return;
        };
        if self.visibility {
            body.visible = Some(!self.hidden.contains(name.as_str()));
        }
        let material = self
            .materials
            .and_then(|materials| materials.get(&name))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let look = self
            .appearances
            .and_then(|appearances| appearances.get("bodies")?.get(&name))
            .and_then(|look| self.record(look));
        let channels = material.as_ref().map_or_else(
            || json!({}),
            |material| json!({"physical_material": material}),
        );
        let appearance = match (look, &material) {
            (Some(look), _) => Some(look),
            (None, Some(material)) => Some(self.material(material)),
            (None, None) => None,
        };
        if let Some(appearance) = appearance {
            let target = json!({"kind": "body", "value": body.id});
            self.bind(
                &format!("body:{}", body.id),
                &target,
                &appearance,
                &channels,
            );
        }
        let Some(looks) = self
            .appearances
            .and_then(|appearances| appearances.get("faces"))
            .and_then(Value::as_array)
        else {
            return;
        };
        for face in looks {
            if face.get("body").and_then(Value::as_str) != Some(name.as_str()) {
                continue;
            }
            let found = face
                .get("name")
                .and_then(Value::as_str)
                .and_then(|face_name| face_names.iter().position(|known| known == face_name))
                .and_then(|index| faces.get(index));
            let (Some(face_id), Some(appearance)) =
                (found, face.get("look").and_then(|look| self.record(look)))
            else {
                continue;
            };
            let target = json!({"kind": "face", "value": face_id});
            self.bind(&format!("face:{face_id}"), &target, &appearance, &json!({}));
        }
    }

    /// Adds the appearances, the bindings, and the physical materials of the
    /// user materials to `ir`.
    pub(crate) fn finish(
        mut self,
        ir: &mut CadIr,
        annotations: &mut AnnotationBuilder,
    ) -> Result<(), CodecError> {
        // The schema names are this decoder's own, not the document's.
        for appearance_id in self.records.keys() {
            annotations.field_exactness(
                appearance_id,
                "schema",
                cadmpeg_ir::Exactness::Inferred,
            )?;
        }
        let materials: Vec<String> = self
            .materials
            .into_iter()
            .flat_map(Map::values)
            .filter_map(|name| name.as_str().map(str::to_owned))
            .collect();
        for user in self.user_materials {
            let Some(name) = user.get("name").and_then(Value::as_str) else {
                continue;
            };
            if materials.iter().any(|material| material == name) {
                self.material(name);
            }
            let mut record = json!({"id": id("material", name), "name": name});
            if let Some(density) = user
                .get("density")
                .filter(|d| d.as_f64().is_some_and(|d| d > 0.0))
            {
                record["density"] = density.clone();
            }
            ir.model
                .physical_materials
                .push(serde_json::from_value(record).map_err(malformed)?);
        }
        for record in self.records.into_values() {
            ir.model
                .appearances
                .push(serde_json::from_value(record).map_err(malformed)?);
        }
        for binding in self.bindings {
            ir.model
                .appearance_bindings
                .push(serde_json::from_value(binding).map_err(malformed)?);
        }
        Ok(())
    }
}

/// The identity of the presentation of the named views.
fn presentation_id() -> String {
    let namespace = cadmpeg_ir::identity_namespace!("solidcraft", "design", "presentation");
    cadmpeg_ir::features::FeatureId::compose(&namespace, IdentityKey::encode_segment("views"))
        .into_string()
}

/// The camera of a named view: its eye is the target moved back by the
/// distance, along the direction that the yaw and pitch give in a Z-up frame.
fn camera(view: &Value) -> Option<Value> {
    let number = |key: &str| view.get(key).and_then(Value::as_f64);
    let target: Vec<f64> = view
        .get("target")?
        .as_array()?
        .iter()
        .map(Value::as_f64)
        .collect::<Option<_>>()?;
    let [x, y, z] = target.as_slice() else {
        return None;
    };
    let (yaw, pitch, distance) = (number("yaw")?, number("pitch")?, number("distance")?);
    let fov = number("fov").unwrap_or(0.0);
    let (cos_pitch, sin_pitch) = (pitch.portable_cos(), pitch.portable_sin());
    let eye = [
        x - yaw.portable_sin() * cos_pitch * distance,
        y - yaw.portable_cos() * cos_pitch * distance,
        z + sin_pitch * distance,
    ];
    Some(json!({"kind": "Camera", "value": {
        "position": eye,
        "properties": {
            "target": format!("{x} {y} {z}"),
            "yaw": yaw.to_string(),
            "pitch": pitch.to_string(),
            "distance": distance.to_string(),
            "fov": fov.to_string(),
            "projection": if fov > 0.0 { "perspective" } else { "orthographic" },
        },
    }}))
}

/// Adds the presentation of the document's named views to `ir`.
pub(crate) fn views(object: &Map<String, Value>, ir: &mut CadIr) -> Result<(), CodecError> {
    let Some(views) = object.get("named_views").and_then(Value::as_array) else {
        return Ok(());
    };
    let mut states = Vec::new();
    for (order, view) in views.iter().enumerate() {
        let Some(kind) = camera(view) else {
            continue;
        };
        let mut attributes = Map::new();
        if let Some(name) = view.get("name").and_then(Value::as_str) {
            attributes.insert("name".to_owned(), json!(name));
        }
        states.push(json!({
            "kind": kind,
            "order": order,
            "attributes": attributes,
            "assets": [],
        }));
    }
    if states.is_empty() {
        return Ok(());
    }
    let document = json!({"id": presentation_id(), "states": states});
    ir.model
        .presentation_documents
        .push(serde_json::from_value(document).map_err(malformed)?);
    Ok(())
}
