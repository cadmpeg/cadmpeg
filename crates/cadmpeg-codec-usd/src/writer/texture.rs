// SPDX-License-Identifier: Apache-2.0
//! Writes the textures of an appearance (spec §15.6): a `UsdUVTexture`
//! network for the preview surface, `MaterialX` image nodes for the
//! `MaterialX` surface, and the texture inputs of an `OmniPBR` material.
//!
//! A texture reference's slot is the preview-surface input it drives; a
//! scalar input also names the image channel it reads, as `<input>.<c>`.

use std::collections::BTreeSet;

use cadmpeg_ir::appearance::{Appearance, TextureRef};
use cadmpeg_ir::assets::AssetContent;

use super::text::{asset, identifier, quoted, real32, tuple32};
use super::Writer;
use crate::schema::shading::{TextureSlot, TEXTURE_SLOTS};
use crate::schema::texture::SlotKind;
use crate::usdz;

/// The package directory of the images the writer embeds.
const TEXTURE_DIRECTORY: &str = "textures";

/// One texture to write: its slot, the channel a scalar slot reads, and
/// the asset path to write.
pub(super) struct Planned<'ir> {
    pub(super) slot: &'static TextureSlot,
    pub(super) channel: char,
    pub(super) file: String,
    pub(super) texture: &'ir TextureRef,
}

impl Planned<'_> {
    /// The preview-surface input name.
    fn input(&self) -> &'static str {
        self.slot.names[0]
    }

    fn coordinates(&self) -> String {
        self.texture
            .mapping
            .coordinates
            .as_ref()
            .map_or_else(|| "st".to_owned(), |name| name.as_str().to_owned())
    }

    fn transformed(&self) -> bool {
        let mapping = &self.texture.mapping;
        mapping.u_scale.get() != 1.0
            || mapping.v_scale.get() != 1.0
            || mapping.u_offset.get() != 0.0
            || mapping.v_offset.get() != 0.0
            || mapping.rotation.get() != 0.0
    }
}

fn float2(values: [f64; 2]) -> String {
    tuple32(&values.map(crate::text::value::round_f32))
}

fn wrap(repeat: bool) -> &'static str {
    if repeat {
        "repeat"
    } else {
        "clamp"
    }
}

fn address(repeat: bool) -> &'static str {
    if repeat {
        "periodic"
    } else {
        "clamp"
    }
}

impl<'ir> Writer<'ir> {
    /// The textures of `appearance` to write, each slot once, in slot
    /// order.
    pub(super) fn planned_textures(&mut self, appearance: &'ir Appearance) -> Vec<Planned<'ir>> {
        let mut out: Vec<Planned<'ir>> = Vec::new();
        for texture in &appearance.textures {
            let (input, channel) = match texture.slot.split_once('.') {
                Some((input, channel)) => (input, channel.chars().next().unwrap_or('r')),
                None => (texture.slot.as_str(), 'r'),
            };
            let slot = TEXTURE_SLOTS.iter().find(|slot| slot.names[0] == input);
            let file = self.texture_file(texture);
            let (Some(slot), Some(file)) = (slot, file) else {
                self.unwritten
                    .reduced_appearances
                    .insert(appearance.id.to_string());
                continue;
            };
            if out.iter().any(|known| known.input() == slot.names[0]) {
                continue;
            }
            out.push(Planned {
                slot,
                channel,
                file,
                texture,
            });
        }
        out.sort_by_key(|planned| {
            TEXTURE_SLOTS
                .iter()
                .position(|slot| std::ptr::eq(slot, planned.slot))
        });
        out
    }

    /// The asset path of the image of `texture`: a package member for an
    /// image that a package holds, and otherwise the image's location.
    fn texture_file(&mut self, texture: &TextureRef) -> Option<String> {
        let asset = self
            .ir
            .model
            .assets
            .iter()
            .find(|asset| asset.id.as_str() == texture.asset_guid)?;
        let named = asset
            .name
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str);
        match &asset.content {
            AssetContent::External { uri } => {
                if self.package.is_none() {
                    return Some(uri.to_string());
                }
                let opened = self.assets.map_or_else(
                    || Err("no asset source was given".to_owned()),
                    |assets| assets.open(None, uri.as_str(), usdz::MAX_MEMBER),
                );
                match opened {
                    Ok(file) if !file.bytes.is_empty() => {
                        self.member(named.unwrap_or(uri.as_str()), file.bytes)
                    }
                    Ok(_) => {
                        self.unwritten
                            .external_textures
                            .insert(format!("{uri} (empty)"));
                        Some(uri.to_string())
                    }
                    Err(reason) => {
                        self.unwritten
                            .external_textures
                            .insert(format!("{uri} ({reason})"));
                        Some(uri.to_string())
                    }
                }
            }
            AssetContent::Embedded { data } => {
                if self.package.is_none() {
                    let path = texture.paths.first()?.clone();
                    self.unwritten.external_textures.insert(path.clone());
                    return Some(path);
                }
                self.member(named.unwrap_or("texture"), data.as_bytes().to_vec())
            }
        }
    }

    /// The package member that holds `bytes`, added under the base name of
    /// `name` when no member holds them.
    fn member(&mut self, name: &str, bytes: Vec<u8>) -> Option<String> {
        let members = self.package.as_mut()?;
        if let Some((known, _)) = members.iter().find(|(_, held)| *held == bytes) {
            return Some(known.clone());
        }
        let base = name
            .rsplit(['/', '\\'])
            .find(|part| !part.is_empty())
            .unwrap_or("texture");
        let mut name = format!("{TEXTURE_DIRECTORY}/{base}");
        let mut ordinal = 1;
        while members.iter().any(|(known, _)| *known == name) {
            name = format!("{TEXTURE_DIRECTORY}/{ordinal}_{base}");
            ordinal += 1;
        }
        members.push((name.clone(), bytes));
        Some(name)
    }

    /// Writes the coordinate readers and the `UsdUVTexture` shaders of
    /// `planned` under the material at `path`, and returns the connection
    /// of each preview input.
    pub(super) fn preview_textures(
        &mut self,
        planned: &[Planned<'_>],
        path: &str,
    ) -> Vec<(String, String, String)> {
        let mut readers = BTreeSet::new();
        let mut connections = Vec::new();
        for texture in planned {
            let coordinates = texture.coordinates();
            let reader = format!("Primvar_{}", identifier(&coordinates));
            if readers.insert(reader.clone()) {
                self.prim_head("def Shader", &reader, &[]);
                self.out.open();
                self.out
                    .line("uniform token info:id = \"UsdPrimvarReader_float2\"");
                self.out
                    .line(&format!("string inputs:varname = {}", quoted(&coordinates)));
                self.out.line("float2 outputs:result");
                self.out.close();
            }
            let mut st = format!("{path}/{reader}.outputs:result");
            let input = texture.input();
            if texture.transformed() {
                let mapping = &texture.texture.mapping;
                let name = format!("Transform_{input}");
                self.prim_head("def Shader", &name, &[]);
                self.out.open();
                self.out.line("uniform token info:id = \"UsdTransform2d\"");
                self.out.line(&format!("float2 inputs:in.connect = <{st}>"));
                self.out.line(&format!(
                    "float2 inputs:scale = {}",
                    float2([mapping.u_scale.get(), mapping.v_scale.get()])
                ));
                self.out.line(&format!(
                    "float inputs:rotation = {}",
                    real32(crate::text::value::round_f32(
                        mapping.rotation.get().to_degrees()
                    ))
                ));
                self.out.line(&format!(
                    "float2 inputs:translation = {}",
                    float2([mapping.u_offset.get(), mapping.v_offset.get()])
                ));
                self.out.line("float2 outputs:result");
                self.out.close();
                st = format!("{path}/{name}.outputs:result");
            }
            let name = format!("Texture_{input}");
            self.prim_head("def Shader", &name, &[]);
            self.out.open();
            self.out.line("uniform token info:id = \"UsdUVTexture\"");
            self.out
                .line(&format!("asset inputs:file = {}", asset(&texture.file)));
            self.out.line(&format!("float2 inputs:st.connect = <{st}>"));
            self.out.line(&format!(
                "token inputs:wrapS = {}",
                quoted(wrap(texture.texture.mapping.repeat_u))
            ));
            self.out.line(&format!(
                "token inputs:wrapT = {}",
                quoted(wrap(texture.texture.mapping.repeat_v))
            ));
            let (kind, output) = match texture.slot.kind {
                SlotKind::Color => ("color3f", "rgb".to_owned()),
                SlotKind::Normal => {
                    self.out.line("token inputs:sourceColorSpace = \"raw\"");
                    self.out.line("float4 inputs:scale = (2, 2, 2, 1)");
                    self.out.line("float4 inputs:bias = (-1, -1, -1, 0)");
                    ("normal3f", "rgb".to_owned())
                }
                SlotKind::Scalar => {
                    self.out.line("token inputs:sourceColorSpace = \"raw\"");
                    ("float", texture.channel.to_string())
                }
            };
            let output_type = if output == "rgb" { "float3" } else { "float" };
            self.out.line(&format!("{output_type} outputs:{output}"));
            self.out.close();
            connections.push((
                kind.to_owned(),
                format!("{input}.connect"),
                format!("<{path}/{name}.outputs:{output}>"),
            ));
        }
        connections
    }

    /// Writes the `MaterialX` image nodes of `planned` under the material at
    /// `path` for a surface of family `column`, and returns the connection
    /// of each surface input.
    pub(super) fn materialx_textures(
        &mut self,
        planned: &[Planned<'_>],
        path: &str,
        column: usize,
    ) -> Vec<(String, String, String)> {
        let mut readers = BTreeSet::new();
        let mut connections = Vec::new();
        for texture in planned {
            let input = texture.slot.names[column];
            if input.is_empty() {
                continue;
            }
            let coordinates = texture.coordinates();
            let reader = format!("Geomprop_{}", identifier(&coordinates));
            if readers.insert(reader.clone()) {
                self.prim_head("def Shader", &reader, &[]);
                self.out.open();
                self.out
                    .line("uniform token info:id = \"ND_geompropvalue_vector2\"");
                self.out.line(&format!(
                    "string inputs:geomprop = {}",
                    quoted(&coordinates)
                ));
                self.out.line("float2 outputs:out");
                self.out.close();
            }
            let (image_type, out_type) = match (texture.slot.kind, texture.channel) {
                (SlotKind::Color, _) => ("color3", "color3f"),
                (SlotKind::Normal, _) => ("vector3", "vector3f"),
                (SlotKind::Scalar, 'r') => ("float", "float"),
                (SlotKind::Scalar, _) => ("color4", "color4f"),
            };
            let mapping = &texture.texture.mapping;
            let tiled = mapping.u_scale.get() != 1.0
                || mapping.v_scale.get() != 1.0
                || mapping.u_offset.get() != 0.0
                || mapping.v_offset.get() != 0.0;
            let preview = texture.input();
            let image = format!("Image_{preview}");
            self.prim_head("def Shader", &image, &[]);
            self.out.open();
            let node = if tiled { "tiledimage" } else { "image" };
            self.out.line(&format!(
                "uniform token info:id = \"ND_{node}_{image_type}\""
            ));
            self.out
                .line(&format!("asset inputs:file = {}", asset(&texture.file)));
            self.out.line(&format!(
                "float2 inputs:texcoord.connect = <{path}/{reader}.outputs:out>"
            ));
            if tiled {
                self.out.line(&format!(
                    "float2 inputs:uvtiling = {}",
                    float2([mapping.u_scale.get(), mapping.v_scale.get()])
                ));
                self.out.line(&format!(
                    "float2 inputs:uvoffset = {}",
                    float2([mapping.u_offset.get(), mapping.v_offset.get()])
                ));
            } else {
                self.out.line(&format!(
                    "string inputs:uaddressmode = {}",
                    quoted(address(mapping.repeat_u))
                ));
                self.out.line(&format!(
                    "string inputs:vaddressmode = {}",
                    quoted(address(mapping.repeat_v))
                ));
            }
            self.out.line(&format!("{out_type} outputs:out"));
            self.out.close();
            let mut source = format!("{path}/{image}.outputs:out");
            let kind = match texture.slot.kind {
                SlotKind::Color => "color3f",
                SlotKind::Normal => {
                    let name = format!("NormalMap_{preview}");
                    self.prim_head("def Shader", &name, &[]);
                    self.out.open();
                    self.out
                        .line("uniform token info:id = \"ND_normalmap_vector2\"");
                    self.out
                        .line(&format!("vector3f inputs:in.connect = <{source}>"));
                    self.out.line("vector3f outputs:out");
                    self.out.close();
                    source = format!("{path}/{name}.outputs:out");
                    "vector3f"
                }
                SlotKind::Scalar => {
                    if texture.channel != 'r' {
                        let index = match texture.channel {
                            'g' => 1,
                            'b' => 2,
                            _ => 3,
                        };
                        let name = format!("Extract_{preview}");
                        self.prim_head("def Shader", &name, &[]);
                        self.out.open();
                        self.out
                            .line("uniform token info:id = \"ND_extract_color4\"");
                        self.out
                            .line(&format!("color4f inputs:in.connect = <{source}>"));
                        self.out.line(&format!("int inputs:index = {index}"));
                        self.out.line("float outputs:out");
                        self.out.close();
                        source = format!("{path}/{name}.outputs:out");
                    }
                    "float"
                }
            };
            connections.push((
                kind.to_owned(),
                format!("{input}.connect"),
                format!("<{source}>"),
            ));
        }
        connections
    }

    /// The `OmniPBR` texture inputs of `planned`, with the transform of the
    /// first transformed texture.
    pub(super) fn omni_pbr_textures(planned: &[Planned<'_>]) -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        for texture in planned {
            let input = texture.slot.names[3];
            if !input.is_empty() {
                out.push(("asset".to_owned(), input.to_owned(), asset(&texture.file)));
            }
        }
        if let Some(texture) = planned.iter().find(|texture| texture.transformed()) {
            let mapping = &texture.texture.mapping;
            out.push((
                "float2".to_owned(),
                "texture_scale".to_owned(),
                float2([mapping.u_scale.get(), mapping.v_scale.get()]),
            ));
            out.push((
                "float2".to_owned(),
                "texture_translate".to_owned(),
                float2([mapping.u_offset.get(), mapping.v_offset.get()]),
            ));
            out.push((
                "float".to_owned(),
                "texture_rotate".to_owned(),
                real32(crate::text::value::round_f32(
                    mapping.rotation.get().to_degrees(),
                )),
            ));
        }
        out
    }
}
