// SPDX-License-Identifier: Apache-2.0
//! Neutral color/visibility overrides and regenerated topology color lists.

use super::Provider;
use crate::loss::FreecadLossCode;
use crate::writer::xml;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceTarget};
use cadmpeg_ir::codec::write::ExportLossCode;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Color;
use std::collections::{BTreeMap, HashMap, HashSet};

/// `transparency_byte` stores the low byte as transparency, as the
/// `ProgramVersion` the writer emits makes `FreeCAD` read it.
fn packed(color: Color, transparency_byte: bool) -> u32 {
    let byte = |value: f32| -> u32 {
        let stored = (f64::from(value) * 255.0).round();
        u32::from(
            (0_u8..=255)
                .find(|byte| f64::from(*byte) == stored)
                .unwrap_or(0),
        )
    };
    crate::gui::convert_packed_alpha(
        byte(color.r()) << 24 | byte(color.g()) << 16 | byte(color.b()) << 8 | byte(color.a()),
        transparency_byte,
    )
}

fn transparency(color: Color) -> u8 {
    let stored = 100.0 - (f64::from(color.a()) * 100.0).round();
    (0_u8..=100)
        .find(|value| f64::from(*value) == stored)
        .unwrap_or(0)
}

fn object_color(provider: &mut Provider, color: Color, transparency_byte: bool) {
    provider.set(
        "ShapeColor",
        "App::PropertyColor",
        "PropertyColor",
        packed(color, transparency_byte),
    );
    provider.set(
        "Transparency",
        "App::PropertyPercent",
        "Integer",
        transparency(color),
    );
}

/// The display precedence `FreeCAD` applies to a binding of one color lane.
fn derived_precedence(schema: Option<&str>, target: &AppearanceTarget) -> Option<&'static str> {
    Some(match (schema?, target) {
        ("FCStd ViewProvider line style", _) => "edge_over_object",
        ("FCStd ViewProvider point style", _) => "vertex_over_object",
        ("FCStd DiffuseColor", _) | (SHAPE_APPEARANCE_SCHEMA, AppearanceTarget::Face(_)) => {
            "face_over_object"
        }
        ("FCStd LineColorArray", _) => "edge_array_over_line",
        ("FCStd PointColorArray", _) => "vertex_array_over_point",
        (SHAPE_APPEARANCE_SCHEMA, AppearanceTarget::Body(_)) => "shape_over_object",
        _ => return None,
    })
}

/// Set a numeric attribute, in the source spelling when the source holds an
/// equal value.
fn set_number(
    attributes: &mut BTreeMap<String, String>,
    source: &BTreeMap<String, String>,
    key: &str,
    number: f64,
) {
    let spelled = source
        .get(key)
        .filter(|text| text.parse::<f64>().ok() == Some(number))
        .cloned()
        .unwrap_or_else(|| number.to_string());
    attributes.insert(key.into(), spelled);
}

fn material(
    provider: &mut Provider,
    appearance: &cadmpeg_ir::appearance::Appearance,
    whole_object: bool,
    transparency_byte: bool,
) {
    if !whole_object {
        return;
    }
    let scalar = |name: &str| {
        appearance
            .properties
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, value)| value.get())
    };
    let shininess = scalar("shininess");
    let transparency = scalar("material_transparency");
    if let Some(original) = provider.native.get("ShapeMaterial") {
        let mut property = original.clone();
        for value in &mut property.values {
            if value.tag == "PropertyMaterial" {
                // FreeCAD's material states every lane; one the neutral
                // appearance leaves unstated keeps its stored value.
                let source = value.attributes.clone();
                if let Some(color) = appearance.base_color {
                    set_number(
                        &mut value.attributes,
                        &source,
                        "diffuseColor",
                        f64::from(packed(color, transparency_byte)),
                    );
                }
                if let Some(number) = shininess {
                    set_number(&mut value.attributes, &source, "shininess", number);
                }
                if let Some(number) = transparency {
                    set_number(&mut value.attributes, &source, "transparency", number);
                }
            }
        }
        if let Some(value) = xml::native_gui(&property) {
            provider.properties.insert("ShapeMaterial".into(), value);
        }
    } else if let Some(color) = appearance.base_color.filter(|_| whole_object) {
        let attributes = [
            ("ambientColor", "0".to_owned()),
            ("diffuseColor", packed(color, transparency_byte).to_string()),
            ("specularColor", "0".to_owned()),
            ("emissiveColor", "0".to_owned()),
            ("shininess", shininess.unwrap_or(0.0).to_string()),
            ("transparency", transparency.unwrap_or(0.0).to_string()),
        ];
        provider.properties.insert(
            "ShapeMaterial".into(),
            xml::property(
                "ShapeMaterial",
                "App::PropertyMaterial",
                &xml::element("PropertyMaterial", &attributes),
            ),
        );
    }
}

fn selection(target: &AppearanceTarget) -> Option<(&str, &'static str)> {
    match target {
        AppearanceTarget::Record { arena, id } => match arena.as_str() {
            "faces" => Some((id.as_str(), "DiffuseColor")),
            "edges" => Some((id.as_str(), "LineColorArray")),
            "vertices" => Some((id.as_str(), "PointColorArray")),
            _ => None,
        },
        AppearanceTarget::Face(id) => Some((id.as_str(), "DiffuseColor")),
        AppearanceTarget::Edge(id) => Some((id.as_str(), "LineColorArray")),
        AppearanceTarget::Vertex(id) => Some((id.as_str(), "PointColorArray")),
        _ => None,
    }
}

fn target_name<'a>(
    ir: &CadIr,
    target: &'a AppearanceTarget,
    names: &'a HashMap<String, String>,
    body_names: &'a HashMap<String, String>,
) -> Option<&'a String> {
    if matches!(target, AppearanceTarget::Record { arena, id } if !ir.model.contains_record(*arena, id))
    {
        return None;
    }
    match target {
        AppearanceTarget::Body(body) => body_names.get(body.as_str()),
        AppearanceTarget::Record { arena, id }
            if arena.as_str() == "bodies" && ir.model.contains_record(*arena, id) =>
        {
            body_names.get(id)
        }
        AppearanceTarget::Record { arena, id }
            if arena.as_str() == "occurrences" && ir.model.contains_record(*arena, id) =>
        {
            names.get(id)
        }
        AppearanceTarget::Occurrence(id) => names.get(id.as_str()),
        _ => selection(target).and_then(|(id, _)| names.get(id)),
    }
}

pub(super) fn write(
    ir: &CadIr,
    names: &HashMap<String, String>,
    body_names: &HashMap<String, String>,
    providers: &mut BTreeMap<String, Provider>,
    entries: &mut BTreeMap<String, Vec<u8>>,
    losses: &mut Vec<LossNote>,
    transparency_byte: bool,
) -> Result<(), CodecError> {
    // The neutral arenas own the color lists. A material states every lane,
    // so a stored one stays whole until a neutral appearance overrides its
    // lanes; a material with no stored source is absent, FreeCAD's default.
    for provider in providers.values_mut() {
        for name in [
            "ShapeColor",
            "Transparency",
            "DiffuseColor",
            "LineColorArray",
            "PointColorArray",
            "ShapeAppearance",
        ] {
            provider.properties.remove(name);
        }
    }
    let mut defaults = HashMap::new();
    // Bodies sharing one view provider have one `ShapeColor` and
    // `Visibility`: the writer writes the first body's and charges the rest
    // (#238 §5.5).
    let mut first_by_provider: HashMap<&String, &cadmpeg_ir::topology::Body> = HashMap::new();
    let mut charged: HashSet<&String> = HashSet::new();
    let mut primary_uses = HashSet::new();
    if let Some(native) = ir.native.namespace("fcstd") {
        let properties = native.arena_as::<crate::native::PropertyRecord>("properties")?;
        let payloads = native.arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")?;
        let uses =
            native.arena_as::<crate::native::source_use::SourceUseSelector>("topology_uses")?;
        let primary_properties: HashSet<_> = properties
            .iter()
            .filter(|property| property.name == "Shape")
            .map(|property| property.id.as_str())
            .collect();
        let primary_payloads: HashSet<_> = payloads
            .iter()
            .filter(|payload| primary_properties.contains(payload.property.as_str()))
            .map(|payload| payload.id.as_str())
            .collect();
        primary_uses.extend(
            uses.into_iter()
                .filter(|source| primary_payloads.contains(source.owner.as_str()))
                .map(|source| source.id),
        );
    }
    let mut bodies = ir.model.bodies.iter().collect::<Vec<_>>();
    bodies.sort_by_key(|body| {
        !body
            .native_ref
            .as_ref()
            .is_some_and(|id| primary_uses.contains(id))
    });
    for body in bodies {
        let Some(name) = body_names.get(body.id.as_str()) else {
            continue;
        };
        if body.visible.is_none() && body.color.is_none() && !providers.contains_key(name) {
            first_by_provider.entry(name).or_insert(body);
            continue;
        }
        if let Some(color) = body.color.filter(|color| {
            first_by_provider
                .get(name)
                .is_none_or(|first| first.color == Some(*color))
        }) {
            let bytes = packed(color, false).to_be_bytes();
            let stored = [
                f32::from(bytes[0]) / 255.0,
                f32::from(bytes[1]) / 255.0,
                f32::from(bytes[2]) / 255.0,
                1.0 - f32::from(transparency(color)) / 100.0,
            ];
            let paths = [
                ("model.bodies[].color.r", color.r(), stored[0]),
                ("model.bodies[].color.g", color.g(), stored[1]),
                ("model.bodies[].color.b", color.b(), stored[2]),
                ("model.bodies[].color.a", color.a(), stored[3]),
            ]
            .into_iter()
            .filter_map(|(path, input, output)| (input != output).then_some(path))
            .collect::<Vec<_>>();
            if !paths.is_empty() {
                losses.push(ExportLossCode::ValueQuantized.note(format!(
                    "body {} stores reduced values for {}",
                    body.id,
                    paths.join(", ")
                )));
            }
        }
        let Some(first) = first_by_provider.get(name) else {
            first_by_provider.insert(name, body);
            let provider = providers
                .entry(name.clone())
                .or_insert_with(|| Provider::new(name.clone()));
            provider.properties.remove("ShapeColor");
            provider.properties.remove("Transparency");
            // `ShapeColor` and `Transparency` are the independent body
            // style's native home; a bound appearance travels in
            // `ShapeMaterial` alone and never overwrites them (#238 §5.5).
            provider.properties.remove("Visibility");
            if let Some(visible) = body.visible {
                provider.visible(visible);
            }
            if let Some(color) = body.color {
                object_color(provider, color, transparency_byte);
                defaults.insert(name.clone(), packed(color, transparency_byte));
            }
            continue;
        };
        if (body.color != first.color || body.visible != first.visible) && charged.insert(name) {
            let ids = ir
                .model
                .bodies
                .iter()
                .filter(|body| body_names.get(body.id.as_str()) == Some(name))
                .map(|body| body.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            losses.push(
                FreecadLossCode::AppearanceBodyStyleNotTransferred.note(format!(
                    "bodies {ids} share one view provider; the first body's style is written"
                )),
            );
        }
    }
    let mut arrays = BTreeMap::<(String, &str), BTreeMap<usize, u32>>::new();
    let mut array_sources = BTreeMap::<(String, &str), HashSet<&str>>::new();
    let mut material_lists = BTreeMap::<String, BTreeMap<usize, &Appearance>>::new();
    let mut bound = HashSet::new();
    for binding in &ir.model.appearance_bindings {
        let Some(appearance) = ir
            .model
            .appearances
            .iter()
            .find(|appearance| appearance.id == binding.appearance)
        else {
            losses.push(
                FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                    "appearance binding {} names an absent appearance and is omitted",
                    binding.id
                )),
            );
            continue;
        };
        let Some(name) = target_name(ir, &binding.target, names, body_names) else {
            losses.push(
                FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                    "appearance binding {} has no writable presentation target",
                    binding.id
                )),
            );
            continue;
        };
        if matches!(&binding.target, AppearanceTarget::Occurrence(_))
            || matches!(&binding.target, AppearanceTarget::Record { arena, .. } if arena.as_str() == "occurrences")
        {
            losses.push(
                FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                    "appearance binding {} targets an occurrence; FreeCAD holds a link's material on its view provider without a binding",
                    binding.id
                )),
            );
        }
        let provider = providers
            .entry(name.clone())
            .or_insert_with(|| Provider::new(name.clone()));
        bound.insert(appearance.id.as_str());
        let material_slot = (appearance.schema.as_deref() == Some(SHAPE_APPEARANCE_SCHEMA))
            .then(|| match selection(&binding.target) {
                None => Some(WHOLE_OBJECT_SLOT),
                Some((id, "DiffuseColor")) => names
                    .get(&format!("{id}:subelement"))
                    .and_then(|value| value.strip_prefix("Face")?.parse::<usize>().ok())
                    .filter(|index| *index > 0),
                Some(_) => None,
            })
            .flatten();
        if first_by_provider.contains_key(&name) {
            if binding.visible.is_some() {
                losses.push(
                    FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                        "appearance binding {} visibility has no independent FreeCAD property",
                        binding.id
                    )),
                );
            }
        } else if let Some(visible) = ir.effective_visibility(&binding.target) {
            provider.visible(visible);
        }
        if let Some(slot) = material_slot {
            material_lists
                .entry(name.clone())
                .or_default()
                .insert(slot, appearance);
        } else if let Some(color) = ir.effective_display_color(&binding.target) {
            if let Some((id, property)) = selection(&binding.target) {
                let index = names
                    .get(&format!("{id}:subelement"))
                    .and_then(|value| {
                        value.strip_prefix(if property == "DiffuseColor" {
                            "Face"
                        } else if property == "LineColorArray" {
                            "Edge"
                        } else {
                            "Vertex"
                        })
                    })
                    .and_then(|value| value.parse::<usize>().ok())
                    .filter(|value| *value > 0);
                if let Some(index) = index {
                    arrays
                        .entry((name.clone(), property))
                        .or_default()
                        .insert(index, packed(color, transparency_byte));
                    array_sources
                        .entry((name.clone(), property))
                        .or_default()
                        .insert(appearance.id.as_str());
                } else {
                    losses.push(
                        FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                            "appearance binding {} has no synthesized topology index",
                            binding.id
                        )),
                    );
                }
            } else if let Some(color) = appearance.base_color.or(Some(color)) {
                // A bound appearance's color travels in `ShapeMaterial`
                // alone and never overwrites an independent body style in
                // `ShapeColor` (#238 §5.5). The bound color remains the
                // array fallback for topology color lists naming this
                // provider.
                defaults.insert(name.clone(), packed(color, transparency_byte));
            }
        }
        material(
            provider,
            appearance,
            selection(&binding.target).is_none() && material_slot.is_none(),
            transparency_byte,
        );
        if selection(&binding.target).is_none()
            && material_slot.is_none()
            && !provider.properties.contains_key("ShapeMaterial")
        {
            losses.push(
                FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                    "appearance {} has no complete material encoding and is omitted",
                    appearance.id
                )),
            );
        }
        // FreeCAD's display precedence follows from the color lane and the
        // element kind; the re-decode derives it.
        let derived = derived_precedence(appearance.schema.as_deref(), &binding.target);
        if !binding.channels.is_empty()
            && (binding.channels.len() != 1
                || binding.channels.iter().any(|(key, value)| {
                    key.as_str() != "precedence" || Some(value.as_str()) != derived
                }))
        {
            losses.push(FreecadLossCode::AppearanceChannelDerived.note(format!(
                "appearance binding {} channels are derived from its color lane and element kind",
                binding.id
            )));
        }
        for (key, value) in &appearance.properties {
            match key.as_str() {
                "line_width" => provider.set(
                    "LineWidth",
                    "App::PropertyFloatConstraint",
                    "Float",
                    value.get(),
                ),
                "point_size" => provider.set(
                    "PointSize",
                    "App::PropertyFloatConstraint",
                    "Float",
                    value.get(),
                ),
                "shininess" | "material_transparency" => {}
                _ => losses.push(
                    FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                        "appearance {} property {key} has no FreeCAD GUI encoding",
                        appearance.id
                    )),
                ),
            }
        }
        let expected_schema = if material_slot.is_some() {
            SHAPE_APPEARANCE_SCHEMA.to_owned()
        } else if let Some((_, property)) = selection(&binding.target) {
            format!("FCStd {property}")
        } else {
            "FCStd ViewProvider ShapeMaterial".to_owned()
        };
        if appearance
            .schema
            .as_ref()
            .is_some_and(|value| value != &expected_schema)
            || (appearance.asset_guid.is_some() && material_slot.is_none())
            || appearance.library_id.is_some()
            || appearance.visual_guid.is_some()
            || appearance.physical_token.is_some()
            || appearance.category.is_some()
        {
            losses.push(
                FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                    "appearance {} catalog identity metadata has no FreeCAD GUI encoding",
                    appearance.id
                )),
            );
        }
        if !appearance.textures.is_empty() {
            losses.push(
                FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                    "appearance {} texture assets are omitted",
                    appearance.id
                )),
            );
        }
    }
    for appearance in &ir.model.appearances {
        if appearance.name.is_some() {
            losses.push(FreecadLossCode::AppearanceNameNotTransferred.note(format!(
                "appearance {} name has no FreeCAD GUI home",
                appearance.id
            )));
        }
    }
    // An unbound appearance read from a view provider's own material (an
    // origin feature's, which owns no body) returns to that provider.
    let (gui_properties, gui_providers) = ir
        .native
        .namespace("fcstd")
        .map(|namespace| {
            Ok::<_, CodecError>((
                namespace.arena_as::<crate::native::GuiPropertyRecord>("gui_properties")?,
                namespace.arena_as::<crate::native::GuiViewProviderRecord>("gui_view_providers")?,
            ))
        })
        .transpose()?
        .unwrap_or_default();
    for appearance in &ir.model.appearances {
        if bound.contains(appearance.id.as_str()) {
            continue;
        }
        let owner = appearance
            .native_ref
            .as_deref()
            .and_then(|id| gui_properties.iter().find(|property| property.id == id))
            .and_then(|property| {
                gui_providers
                    .iter()
                    .find(|provider| provider.id == property.owner)
            })
            .map(|provider| provider.name.clone())
            .filter(|name| providers.contains_key(name));
        match (owner, appearance.schema.as_deref()) {
            (Some(owner), Some(SHAPE_APPEARANCE_SCHEMA)) => {
                material_lists
                    .entry(owner)
                    .or_default()
                    .insert(WHOLE_OBJECT_SLOT, appearance);
                continue;
            }
            (Some(owner), Some("FCStd ViewProvider ShapeMaterial")) => {
                if let Some(provider) = providers.get_mut(&owner) {
                    material(provider, appearance, true, transparency_byte);
                    // A body-less provider's own color has no neutral home;
                    // the decoder reads the material diffuse as a separate
                    // lane only beside it.
                    if appearance
                        .properties
                        .keys()
                        .any(|key| key.as_str() == "diffuse_packed")
                    {
                        for name in ["ShapeColor", "Transparency"] {
                            if let Some(value) = provider.native.get(name).and_then(xml::native_gui)
                            {
                                provider.properties.insert(name.into(), value);
                            }
                        }
                    }
                    continue;
                }
            }
            (Some(owner), Some(schema)) => {
                // An entry of the provider's own color list with no element
                // to bind returns to its list position.
                // Appearances read from one list keep their entry order.
                let entry = ["DiffuseColor", "LineColorArray", "PointColorArray"]
                    .into_iter()
                    .find(|property| schema.strip_prefix("FCStd ") == Some(*property))
                    .and_then(|property| {
                        let index = ir
                            .model
                            .appearances
                            .iter()
                            .filter(|other| other.native_ref == appearance.native_ref)
                            .position(|other| other.id == appearance.id)?
                            + 1;
                        Some((property, index, appearance.base_color?))
                    });
                if let Some((property, index, color)) = entry {
                    arrays
                        .entry((owner.clone(), property))
                        .or_default()
                        .insert(index, packed(color, transparency_byte));
                    array_sources
                        .entry((owner, property))
                        .or_default()
                        .insert(appearance.id.as_str());
                    continue;
                }
            }
            _ => {}
        }
        losses.push(
            FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                "unbound appearance {} has no FreeCAD GUI owner and is omitted",
                appearance.id
            )),
        );
    }
    for ((owner, property), overrides) in arrays {
        let prefix = if property == "DiffuseColor" {
            "Face"
        } else if property == "LineColorArray" {
            "Edge"
        } else {
            "Vertex"
        };
        let count = names
            .iter()
            .filter(|(id, _)| id.ends_with(":subelement"))
            .filter(|(id, _)| names.get(id.trim_end_matches(":subelement")) == Some(&owner))
            .filter_map(|(_, value)| value.strip_prefix(prefix)?.parse::<usize>().ok())
            .chain(overrides.keys().copied())
            .max()
            .unwrap_or(0);
        // One appearance bound to every element is FreeCAD's one-entry
        // list, which applies its color to the whole element kind.
        let uniform = count != 0
            && overrides.len() == count
            && (1..=count).all(|index| overrides.contains_key(&index))
            && array_sources
                .get(&(owner.clone(), property))
                .is_some_and(|sources| sources.len() == 1);
        let colors = if uniform {
            overrides.values().take(1).copied().collect::<Vec<_>>()
        } else {
            (1..=count)
                .map(|index| {
                    overrides.get(&index).copied().unwrap_or_else(|| {
                        defaults.get(&owner).copied().unwrap_or_else(|| {
                            crate::gui::convert_packed_alpha(DEFAULT_DIFFUSE, transparency_byte)
                        })
                    })
                })
                .collect()
        };
        let Ok(stored) = u32::try_from(colors.len()) else {
            debug_assert!(false, "one color per topology element fits the count lane");
            continue;
        };
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&stored.to_le_bytes());
        for color in colors {
            bytes.extend_from_slice(&color.to_le_bytes());
        }
        let Some(provider) = providers.get_mut(&owner) else {
            debug_assert!(false, "topology color provider is absent");
            continue;
        };
        // A list the source stored keeps its entry name and status.
        let native = provider.native.get(property).and_then(|native| {
            let [entry] = native.side_entries.as_slice() else {
                return None;
            };
            let value = xml::native_gui_with(native, &HashSet::from([entry.clone()]))?;
            Some((entry.clone(), value))
        });
        let (filename, value) = native.unwrap_or_else(|| {
            let filename = format!("{owner}.{property}");
            let value = xml::property(
                property,
                "App::PropertyColorList",
                &xml::element("ColorList", &[("file", filename.clone())]),
            );
            (filename, value)
        });
        entries.insert(filename, bytes);
        provider.properties.insert(property.into(), value);
    }
    for (owner, slots) in material_lists {
        let Some(provider) = providers.get_mut(&owner) else {
            continue;
        };
        let materials: Vec<&Appearance> = match slots.get(&WHOLE_OBJECT_SLOT) {
            Some(whole) if slots.len() == 1 => vec![*whole],
            _ => {
                let faces: BTreeMap<_, _> = slots
                    .into_iter()
                    .filter(|(slot, _)| *slot != WHOLE_OBJECT_SLOT)
                    .collect();
                let count = faces.keys().max().copied().unwrap_or(0);
                let Some(fallback) = faces.values().next().copied() else {
                    continue;
                };
                (1..=count)
                    .map(|index| faces.get(&index).copied().unwrap_or(fallback))
                    .collect()
            }
        };
        let Some(list) = material_list(&materials, transparency_byte) else {
            debug_assert!(false, "material counts and strings fit their lanes");
            continue;
        };
        let filename = format!("{owner}.ShapeAppearance");
        entries.insert(filename.clone(), list);
        provider.properties.remove("ShapeMaterial");
        provider.properties.insert(
            "ShapeAppearance".into(),
            xml::property(
                "ShapeAppearance",
                "App::PropertyMaterialList",
                &xml::element(
                    "MaterialList",
                    &[("file", filename), ("version", "3".to_owned())],
                ),
            ),
        );
    }
    Ok(())
}

/// The `ShapeAppearance` material lane of an appearance a `FreeCAD` 1.0
/// material list produced.
const SHAPE_APPEARANCE_SCHEMA: &str = "FCStd ShapeAppearance";
/// The slot of a material that applies to the whole object.
const WHOLE_OBJECT_SLOT: usize = 0;

/// A version-3 `App::PropertyMaterialList` side file: the packed colors and
/// scalars of every material, then each material's image, image path and
/// UUID strings.
fn material_list(materials: &[&Appearance], transparency_byte: bool) -> Option<Vec<u8>> {
    let scalar = |appearance: &Appearance, name: &str| {
        appearance
            .properties
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, value)| value.get())
    };
    let packed_property = |appearance: &Appearance, name: &str| {
        let stored = scalar(appearance, name)
            .and_then(cadmpeg_core::convert::truncate_f64_to_u32)
            .unwrap_or(0);
        crate::gui::convert_packed_alpha(stored, transparency_byte)
    };
    let mut bytes = Vec::new();
    let count = u32::try_from(materials.len()).ok()?;
    bytes.extend_from_slice(&count.to_le_bytes());
    for appearance in materials {
        let diffuse = appearance
            .base_color
            .map_or(DEFAULT_DIFFUSE, |color| packed(color, false));
        for value in [
            packed_property(appearance, "ambient_packed"),
            crate::gui::convert_packed_alpha(diffuse, transparency_byte),
            packed_property(appearance, "specular_packed"),
            packed_property(appearance, "emissive_packed"),
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let opacity = appearance
            .base_color
            .map_or(1.0, |color| f64::from(color.a()));
        let shininess = scalar(appearance, "shininess").unwrap_or(DEFAULT_SHININESS);
        let transparency = scalar(appearance, "transparency").unwrap_or(1.0 - opacity);
        for value in [shininess, transparency] {
            let stored = cadmpeg_core::convert::f32_from_f64(value).unwrap_or(0.0);
            bytes.extend_from_slice(&stored.to_le_bytes());
        }
    }
    for appearance in materials {
        for text in ["", "", appearance.asset_guid.as_deref().unwrap_or("")] {
            let length = u32::try_from(text.len()).ok()?;
            bytes.extend_from_slice(&length.to_le_bytes());
            bytes.extend_from_slice(text.as_bytes());
        }
    }
    Some(bytes)
}

/// `FreeCAD`'s default shape diffuse color, opaque light grey.
const DEFAULT_DIFFUSE: u32 = 0xcccc_ccff;
/// `FreeCAD`'s default material shininess.
const DEFAULT_SHININESS: f64 = 0.9;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_channels_follow_declared_storage_rounding() {
        let color = Color::new(0.1, 0.0, 1.0, 0.375).unwrap();
        assert_eq!(packed(color, false).to_be_bytes(), [26, 0, 255, 96]);
        assert_eq!(transparency(color), 62);
    }

    #[test]
    fn absent_material_facts_are_not_invented_as_default_values() {
        let appearance =
            serde_json::from_value(serde_json::json!({"id":"test:model:appearance#empty"}))
                .unwrap();
        let mut provider = Provider::new("Shape".into());
        material(&mut provider, &appearance, true, false);
        assert!(!provider.properties.contains_key("ShapeMaterial"));
    }
}
