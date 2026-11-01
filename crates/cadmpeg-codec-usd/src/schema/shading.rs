// SPDX-License-Identifier: Apache-2.0
//! Material binding and the surface shaders of the universal, `MaterialX`,
//! and MDL render contexts (spec §15).

use crate::schema::texture::{SlotKind, Texture};
use crate::schema::{attribute, token, Issues};
use crate::stage::{Prim, Stage};

/// Relationship that binds a material for every purpose.
const BINDING: &str = "material:binding";
/// The surface shaders whose inputs the model reads, by `info:id`
/// (spec §15.3, §15.4).
pub(crate) const SURFACE_IDS: &[&str] = &[
    "UsdPreviewSurface",
    "ND_UsdPreviewSurface_surfaceshader",
    "ND_open_pbr_surface_surfaceshader",
    "ND_standard_surface_surfaceshader",
    "ND_standard_surface_surfaceshader_100",
];
/// The MDL materials whose inputs the model reads, by subidentifier
/// (spec §15.5).
pub(crate) const MDL_SURFACES: &[&str] = &["OmniPBR", "OmniPBR_ClearCoat"];
/// Inputs that mark another MDL material as one with the `OmniPBR`
/// parameters, such as a preset of an MDL material library (spec §15.5).
const OMNI_PBR_INPUTS: &[&str] = &[
    "diffuse_color_constant",
    "diffuse_tint",
    "diffuse_texture",
    "reflection_roughness_constant",
    "metallic_constant",
];
/// The surface outputs of a material, in the order the model prefers them.
const SURFACE_OUTPUTS: &[&str] = &[
    "outputs:mtlx:surface",
    "outputs:surface",
    "outputs:mdl:surface",
];
/// Longest chain of input connections followed to a value.
const MAX_CONNECTION_HOPS: usize = 16;

/// The column of a surface shader in [`TextureSlot::names`]: the preview
/// surface, `OpenPBR`, Standard Surface, or `OmniPBR`.
pub(crate) fn family(shader: &str) -> Option<usize> {
    match shader {
        "UsdPreviewSurface" | "ND_UsdPreviewSurface_surfaceshader" => Some(0),
        "ND_open_pbr_surface_surfaceshader" => Some(1),
        "ND_standard_surface_surfaceshader" | "ND_standard_surface_surfaceshader_100" => Some(2),
        shader if MDL_SURFACES.contains(&shader) => Some(3),
        _ => None,
    }
}

/// A surface input that a texture can drive, with its input name in each
/// shader family; an empty name is an input the family lacks.
pub(crate) struct TextureSlot {
    pub(crate) names: [&'static str; 4],
    pub(crate) kind: SlotKind,
}

/// The surface inputs that a texture can drive (spec §15.6).
pub(crate) const TEXTURE_SLOTS: &[TextureSlot] = &[
    TextureSlot {
        names: [
            "diffuseColor",
            "base_color",
            "base_color",
            "diffuse_texture",
        ],
        kind: SlotKind::Color,
    },
    TextureSlot {
        names: [
            "emissiveColor",
            "emission_color",
            "emission_color",
            "emissive_color_texture",
        ],
        kind: SlotKind::Color,
    },
    TextureSlot {
        names: [
            "metallic",
            "base_metalness",
            "metalness",
            "metallic_texture",
        ],
        kind: SlotKind::Scalar,
    },
    TextureSlot {
        names: [
            "roughness",
            "specular_roughness",
            "specular_roughness",
            "reflectionroughness_texture",
        ],
        kind: SlotKind::Scalar,
    },
    TextureSlot {
        names: ["opacity", "geometry_opacity", "opacity", "opacity_texture"],
        kind: SlotKind::Scalar,
    },
    TextureSlot {
        names: ["normal", "geometry_normal", "normal", "normalmap_texture"],
        kind: SlotKind::Normal,
    },
    TextureSlot {
        names: ["occlusion", "", "", ""],
        kind: SlotKind::Scalar,
    },
    TextureSlot {
        names: ["clearcoat", "coat_weight", "coat", ""],
        kind: SlotKind::Scalar,
    },
    TextureSlot {
        names: ["clearcoatRoughness", "coat_roughness", "coat_roughness", ""],
        kind: SlotKind::Scalar,
    },
    TextureSlot {
        names: ["specularColor", "specular_color", "specular_color", ""],
        kind: SlotKind::Color,
    },
];

/// The texture slot of input `input` of a shader in family `column`.
pub(crate) fn texture_slot(column: usize, input: &str) -> Option<&'static TextureSlot> {
    TEXTURE_SLOTS
        .iter()
        .find(|slot| !slot.names[column].is_empty() && slot.names[column] == input)
}

/// A texture that drives a surface input.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SlotTexture {
    /// The preview-surface name of the input.
    pub(crate) slot: &'static str,
    pub(crate) kind: SlotKind,
    pub(crate) texture: Texture,
}

/// The inputs of a surface shader read as constants.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceShader {
    /// The material prim path.
    pub(crate) material: String,
    pub(crate) name: String,
    /// The shader's `info:id`, or the subidentifier of an MDL material.
    pub(crate) shader: String,
    /// Authored constant inputs, by input name without the `inputs:`
    /// namespace, each as its components.
    pub(crate) inputs: Vec<(String, Vec<f64>)>,
    /// Inputs connected to a shader output that is not a texture the
    /// model reads.
    pub(crate) connected: Vec<String>,
    /// The textures that drive inputs, in shader order, each slot and file
    /// once.
    pub(crate) textures: Vec<SlotTexture>,
    /// The family of the material's own shader in [`TextureSlot::names`].
    pub(crate) column: Option<usize>,
    /// The MDL module of the material's MDL shader, with the layer that
    /// names it.
    pub(crate) module: Option<(String, Option<usize>)>,
}

impl SurfaceShader {
    pub(crate) fn input(&self, name: &str) -> Option<&[f64]> {
        self.inputs
            .iter()
            .find(|(input, _)| input == name)
            .map(|(_, value)| value.as_slice())
    }
}

/// The material bound to a prim, as read.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Bound {
    /// A material whose surface is a shader the model reads.
    Surface(SurfaceShader),
    /// A material with another surface, or none: its path.
    Other(String),
}

/// The material bound to `prim` or its nearest ancestor with a binding.
pub(crate) fn bound_material(
    stage: &Stage<'_>,
    prim: &Prim<'_>,
    issues: &mut Issues,
) -> Option<Bound> {
    let mut cursor = Some(prim);
    while let Some(current) = cursor {
        if let Some(target) = current.relationship_targets(BINDING).into_iter().next() {
            return Some(material(stage, &target, issues));
        }
        cursor = current.parent.and_then(|parent| stage.prims.get(parent));
    }
    None
}

/// The material at `path`.
pub(crate) fn material(stage: &Stage<'_>, path: &str, issues: &mut Issues) -> Bound {
    let Some(material) = stage.find(path) else {
        return Bound::Other(path.to_owned());
    };
    let mut found: Vec<(&Prim<'_>, Identified)> = Vec::new();
    for output in SURFACE_OUTPUTS {
        let Some(source) = material.connections(output).into_iter().next() else {
            continue;
        };
        let Some((shader, _)) = stage.find_property(&source) else {
            continue;
        };
        if let Some(identified) = shader_id(shader, issues) {
            if !found.iter().any(|(_, known)| known.id == identified.id) {
                found.push((shader, identified));
            }
        }
    }
    let mut shaders = found.into_iter();
    let Some((shader, own)) = shaders.next() else {
        return Bound::Other(path.to_owned());
    };
    let mut surface = SurfaceShader {
        material: path.to_owned(),
        name: material.name.clone(),
        shader: own.id.clone(),
        inputs: Vec::new(),
        connected: Vec::new(),
        textures: Vec::new(),
        column: own.column,
        module: own.module.clone(),
    };
    read_inputs(stage, shader, &own, "", &mut surface, issues);
    // The other contexts' shaders keep their inputs under their own name.
    for (shader, identified) in shaders {
        if surface.module.is_none() {
            surface.module.clone_from(&identified.module);
        }
        let prefix = format!("{}/", identified.id);
        read_inputs(stage, shader, &identified, &prefix, &mut surface, issues);
    }
    Bound::Surface(surface)
}

/// Adds the constant inputs of `shader`, whose surface is `id`, to
/// `surface`, each name after `prefix`, and the textures that drive its
/// inputs.
fn read_inputs(
    stage: &Stage<'_>,
    shader: &Prim<'_>,
    identified: &Identified,
    prefix: &str,
    surface: &mut SurfaceShader,
    issues: &mut Issues,
) {
    let (id, column) = (identified.id.as_str(), identified.column);
    for name in shader.property_names() {
        let Some(input) = name.strip_prefix("inputs:") else {
            continue;
        };
        let slot = column.and_then(|column| texture_slot(column, input));
        if column == Some(3) {
            if let Some(slot) = slot {
                if let Some(texture) =
                    crate::schema::texture::omni_pbr(stage, shader, name, id, issues)
                {
                    add_texture(surface, slot, texture);
                    continue;
                }
            }
        }
        match input_value(stage, shader, name, issues) {
            InputValue::Constant(value) => surface.inputs.push((format!("{prefix}{input}"), value)),
            InputValue::Connected(fallback) => {
                // A connected input's own value is its fallback.
                if let Some(value) = fallback {
                    surface.inputs.push((format!("{prefix}{input}"), value));
                }
                let texture = slot
                    .and_then(|_| crate::schema::texture::connected(stage, shader, name, issues));
                match (slot, texture) {
                    (Some(slot), Some(texture)) => add_texture(surface, slot, texture),
                    _ => surface.connected.push(format!("{prefix}{input}")),
                }
            }
            InputValue::None => {}
        }
    }
}

/// Adds `texture` for `slot` unless the slot already has a texture of the
/// same file.
fn add_texture(surface: &mut SurfaceShader, slot: &TextureSlot, texture: Texture) {
    let name = slot.names[0];
    if !surface
        .textures
        .iter()
        .any(|known| known.slot == name && known.texture.file == texture.file)
    {
        surface.textures.push(SlotTexture {
            slot: name,
            kind: slot.kind,
            texture,
        });
    }
}

/// A surface shader the model reads.
struct Identified {
    /// The `info:id`, or the subidentifier of an MDL material.
    id: String,
    /// The family in [`TextureSlot::names`].
    column: Option<usize>,
    /// The MDL module, with the layer that names it.
    module: Option<(String, Option<usize>)>,
}

/// The surface the model reads that a shader prim states: a surface shader
/// by its `info:id`, or an MDL material by the subidentifier of its source
/// asset. An MDL material with the `OmniPBR` inputs is in the `OmniPBR`
/// family.
fn shader_id(shader: &Prim<'_>, issues: &mut Issues) -> Option<Identified> {
    if let Some(id) = token(shader, "info:id", issues) {
        return SURFACE_IDS.contains(&id.as_str()).then(|| Identified {
            column: family(&id),
            id,
            module: None,
        });
    }
    let module = attribute(shader, "info:mdl:sourceAsset", issues)
        .and_then(|asset| asset.texts()?.first().cloned())
        .filter(|path| !path.is_empty());
    let id = token(shader, "info:mdl:sourceAsset:subIdentifier", issues).or_else(|| {
        let path = module.as_deref()?;
        Some(path.rsplit('/').next()?.strip_suffix(".mdl")?.to_owned())
    })?;
    let omni_pbr = MDL_SURFACES.contains(&id.as_str())
        || shader.property_names().into_iter().any(|name| {
            name.strip_prefix("inputs:")
                .is_some_and(|input| OMNI_PBR_INPUTS.contains(&input))
        });
    Some(Identified {
        id,
        column: omni_pbr.then_some(3),
        module: module.map(|path| (path, shader.attribute_layer("info:mdl:sourceAsset"))),
    })
}

enum InputValue {
    Constant(Vec<f64>),
    /// Connected to a shader output, with the input's own value.
    Connected(Option<Vec<f64>>),
    None,
}

/// The value of a shader input: its own value, or the value of the
/// interface input it connects to.
fn input_value(stage: &Stage<'_>, prim: &Prim<'_>, name: &str, issues: &mut Issues) -> InputValue {
    let numeric = |owner: &Prim<'_>, property: &str, issues: &mut Issues| {
        attribute(owner, property, issues).and_then(|value| {
            value.numbers().or_else(|| {
                value.bools().map(|flags| {
                    flags
                        .iter()
                        .map(|flag| f64::from(u8::from(*flag)))
                        .collect()
                })
            })
        })
    };
    let mut current = (prim, name.to_owned());
    for _ in 0..MAX_CONNECTION_HOPS {
        let (owner, property) = (current.0, current.1.clone());
        let Some(source) = owner.connections(&property).into_iter().next() else {
            return match numeric(owner, &property, issues) {
                Some(value) => InputValue::Constant(value),
                None => InputValue::None,
            };
        };
        let Some((next, next_property)) = stage.find_property(&source) else {
            return InputValue::None;
        };
        if next_property.starts_with("outputs:") && next.type_name() == Some("Shader") {
            return InputValue::Connected(numeric(owner, &property, issues));
        }
        current = (next, next_property.to_owned());
    }
    InputValue::Connected(None)
}

#[cfg(test)]
mod tests;
