// SPDX-License-Identifier: Apache-2.0
//! The image textures that drive surface shader inputs (spec §15.6): the
//! `UsdUVTexture` network of the universal render context, the `MaterialX`
//! image nodes, and the texture inputs of an `OmniPBR` material.

use crate::schema::{attribute, Issues};
use crate::stage::{Prim, Stage};

/// Longest chain of connections followed from a shader input to its
/// texture, its coordinates, or a value.
const MAX_NETWORK_HOPS: usize = 16;

/// What a texture slot receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotKind {
    Color,
    Scalar,
    Normal,
}

/// An image texture that drives one surface input.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Texture {
    /// The node that reads the image: `UsdUVTexture`, a `MaterialX` node
    /// id, or the `OmniPBR` subidentifier.
    pub(crate) node: String,
    /// The authored image asset path.
    pub(crate) file: String,
    /// The layer that authored the asset path.
    pub(crate) layer: Option<usize>,
    /// The channel a scalar input reads: `r`, `g`, `b`, or `a`.
    pub(crate) channel: Option<char>,
    /// The texture-coordinate primvar, without the `primvars:` namespace.
    pub(crate) coordinates: Option<String>,
    pub(crate) scale: [f64; 2],
    pub(crate) translation: [f64; 2],
    /// Counterclockwise rotation of the coordinates, in degrees.
    pub(crate) rotation: f64,
    /// Whether the image repeats along `u` and along `v`.
    pub(crate) repeat: [bool; 2],
}

impl Texture {
    fn new(node: String, file: String, layer: Option<usize>) -> Self {
        Self {
            node,
            file,
            layer,
            channel: None,
            coordinates: None,
            scale: [1.0, 1.0],
            translation: [0.0, 0.0],
            rotation: 0.0,
            repeat: [true, true],
        }
    }
}

/// The `info:id` of a shader prim.
fn shader_id(prim: &Prim<'_>, issues: &mut Issues) -> Option<String> {
    crate::schema::token(prim, "info:id", issues)
}

/// The output that input `name` of `prim` connects to: the shader prim and
/// the output name, after connections to interface inputs.
fn source<'s, 'l>(
    stage: &'s Stage<'l>,
    prim: &'s Prim<'l>,
    name: &str,
) -> Option<(&'s Prim<'l>, String)> {
    let (mut owner, mut property) = (prim, name.to_owned());
    for _ in 0..MAX_NETWORK_HOPS {
        let target = owner.connections(&property).into_iter().next()?;
        let (next, next_property) = stage.find_property(&target)?;
        if next_property.starts_with("outputs:") && next.type_name() == Some("Shader") {
            return Some((next, next_property.to_owned()));
        }
        (owner, property) = (next, next_property.to_owned());
    }
    None
}

/// The value of input `name` of `prim`, or of the interface input it
/// connects to, with the layer of the value.
fn value(
    stage: &Stage<'_>,
    prim: &Prim<'_>,
    name: &str,
    issues: &mut Issues,
) -> Option<(crate::text::value::Typed, Option<usize>)> {
    let (mut owner, mut property) = (prim, name.to_owned());
    for _ in 0..MAX_NETWORK_HOPS {
        match owner.connections(&property).into_iter().next() {
            None => {
                let value = attribute(owner, &property, issues)?;
                return Some((value, owner.attribute_layer(&property)));
            }
            Some(target) => {
                let (next, next_property) = stage.find_property(&target)?;
                (owner, property) = (next, next_property.to_owned());
            }
        }
    }
    None
}

fn text(stage: &Stage<'_>, prim: &Prim<'_>, name: &str, issues: &mut Issues) -> Option<String> {
    let (value, _) = value(stage, prim, name, issues)?;
    value.texts()?.first().cloned()
}

fn asset(
    stage: &Stage<'_>,
    prim: &Prim<'_>,
    name: &str,
    issues: &mut Issues,
) -> Option<(String, Option<usize>)> {
    let (value, layer) = value(stage, prim, name, issues)?;
    let path = value.texts()?.first()?.clone();
    (!path.is_empty()).then_some((path, layer))
}

fn reals<const N: usize>(
    stage: &Stage<'_>,
    prim: &Prim<'_>,
    name: &str,
    issues: &mut Issues,
) -> Option<[f64; N]> {
    let (value, _) = value(stage, prim, name, issues)?;
    value.numbers()?.try_into().ok()
}

/// Whether a wrap mode token repeats the image.
fn repeats(mode: Option<&str>) -> bool {
    !matches!(mode, Some("clamp" | "black" | "constant"))
}

/// The channel an output name reads, for an output of one channel.
fn channel(output: &str) -> Option<char> {
    match output.strip_prefix("outputs:")? {
        "r" => Some('r'),
        "g" => Some('g'),
        "b" => Some('b'),
        "a" => Some('a'),
        _ => None,
    }
}

/// The texture that drives input `name` of `shader`, or `None` when the
/// input does not connect to an image.
pub(crate) fn connected(
    stage: &Stage<'_>,
    shader: &Prim<'_>,
    name: &str,
    issues: &mut Issues,
) -> Option<Texture> {
    let (mut node, mut output) = source(stage, shader, name)?;
    let mut extracted = None;
    for _ in 0..MAX_NETWORK_HOPS {
        let id = shader_id(node, issues)?;
        if id.starts_with("ND_normalmap") {
            (node, output) = source(stage, node, "inputs:in")?;
        } else if id.starts_with("ND_extract_") {
            let index = reals::<1>(stage, node, "inputs:index", issues).map_or(0.0, |[i]| i);
            extracted = ['r', 'g', 'b', 'a']
                .into_iter()
                .zip([0.0, 1.0, 2.0, 3.0])
                .find_map(|(channel, at)| (at == index).then_some(channel));
            (node, output) = source(stage, node, "inputs:in")?;
        } else {
            break;
        }
    }
    let id = shader_id(node, issues)?;
    if id == "UsdUVTexture" {
        let (file, layer) = asset(stage, node, "inputs:file", issues)?;
        let mut texture = Texture::new(id, file, layer);
        texture.channel = channel(&output);
        texture.repeat = [
            repeats(text(stage, node, "inputs:wrapS", issues).as_deref()),
            repeats(text(stage, node, "inputs:wrapT", issues).as_deref()),
        ];
        coordinates(stage, node, "inputs:st", &mut texture, issues);
        return Some(texture);
    }
    if id.starts_with("ND_image_") || id.starts_with("ND_tiledimage_") {
        let (file, layer) = asset(stage, node, "inputs:file", issues)?;
        let tiled = id.starts_with("ND_tiledimage_");
        let mut texture = Texture::new(id, file, layer);
        texture.channel = extracted;
        texture.repeat = [
            repeats(text(stage, node, "inputs:uaddressmode", issues).as_deref()),
            repeats(text(stage, node, "inputs:vaddressmode", issues).as_deref()),
        ];
        if tiled {
            if let Some(scale) = reals(stage, node, "inputs:uvtiling", issues) {
                texture.scale = scale;
            }
            if let Some(offset) = reals(stage, node, "inputs:uvoffset", issues) {
                texture.translation = offset;
            }
        }
        coordinates(stage, node, "inputs:texcoord", &mut texture, issues);
        return Some(texture);
    }
    None
}

/// Reads the coordinate network of input `name` of image node `node` into
/// `texture`: a transform node, then a primvar reader.
fn coordinates(
    stage: &Stage<'_>,
    node: &Prim<'_>,
    name: &str,
    texture: &mut Texture,
    issues: &mut Issues,
) {
    let Some((mut reader, _)) = source(stage, node, name) else {
        return;
    };
    for _ in 0..MAX_NETWORK_HOPS {
        let Some(id) = shader_id(reader, issues) else {
            return;
        };
        match id.as_str() {
            "UsdTransform2d" => {
                if let Some(scale) = reals(stage, reader, "inputs:scale", issues) {
                    texture.scale = scale;
                }
                if let Some(translation) = reals(stage, reader, "inputs:translation", issues) {
                    texture.translation = translation;
                }
                if let Some([rotation]) = reals(stage, reader, "inputs:rotation", issues) {
                    texture.rotation = rotation;
                }
                let Some((next, _)) = source(stage, reader, "inputs:in") else {
                    return;
                };
                reader = next;
            }
            "UsdPrimvarReader_float2" => {
                texture.coordinates = text(stage, reader, "inputs:varname", issues);
                return;
            }
            "ND_geompropvalue_vector2" => {
                texture.coordinates = text(stage, reader, "inputs:geomprop", issues);
                return;
            }
            "ND_texcoord_vector2" => {
                let index = reals::<1>(stage, reader, "inputs:index", issues).map_or(0.0, |[i]| i);
                texture.coordinates = Some(if index == 0.0 {
                    "st".to_owned()
                } else {
                    format!("st{index}")
                });
                return;
            }
            _ => return,
        }
    }
}

/// The texture of `OmniPBR` input `name`, an asset input, with the
/// material's texture transform.
pub(crate) fn omni_pbr(
    stage: &Stage<'_>,
    shader: &Prim<'_>,
    name: &str,
    node: &str,
    issues: &mut Issues,
) -> Option<Texture> {
    let (file, layer) = asset(stage, shader, name, issues)?;
    let mut texture = Texture::new(node.to_owned(), file, layer);
    if let Some(scale) = reals(stage, shader, "inputs:texture_scale", issues) {
        texture.scale = scale;
    }
    if let Some(translation) = reals(stage, shader, "inputs:texture_translate", issues) {
        texture.translation = translation;
    }
    if let Some([rotation]) = reals(stage, shader, "inputs:texture_rotate", issues) {
        texture.rotation = rotation;
    }
    Some(texture)
}

#[cfg(test)]
mod tests;
