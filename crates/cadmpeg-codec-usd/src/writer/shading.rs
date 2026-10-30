// SPDX-License-Identifier: Apache-2.0
//! Writes appearances as materials with a `UsdPreviewSurface` shader, a
//! `MaterialX` surface shader, and, for an `OmniPBR` source, an MDL shader
//! (spec §15).
//!
//! An appearance's properties are the inputs of its own shader, named
//! without a prefix, and the inputs of the material's other shaders, named
//! `<shader>/<input>`. Each shader takes its own inputs as written; the
//! inputs it lacks come from the base color and from the parameters that
//! the shaders share: metalness, roughness, index of refraction, coat, and
//! specular color.

use std::collections::BTreeSet;

use cadmpeg_ir::appearance::Appearance;

use super::text::{asset, quoted, real32, tuple32};
use super::Writer;

const PREVIEW: &str = "UsdPreviewSurface";
const OPEN_PBR: &str = "ND_open_pbr_surface_surfaceshader";
/// The `MaterialX` shaders written with their own inputs.
const MATERIALX: &[&str] = &[
    OPEN_PBR,
    "ND_standard_surface_surfaceshader",
    "ND_standard_surface_surfaceshader_100",
    "ND_UsdPreviewSurface_surfaceshader",
];
const OMNI_PBR: &[&str] = &["OmniPBR", "OmniPBR_ClearCoat"];
/// Inputs that mark another MDL material as one with the `OmniPBR`
/// parameters.
const OMNI_PBR_INPUTS: &[&str] = &[
    "diffuse_color_constant",
    "diffuse_tint",
    "diffuse_texture",
    "reflection_roughness_constant",
    "metallic_constant",
];

/// A parameter that the shaders share, with its input name in each shader
/// family: preview, `OpenPBR`, Standard Surface, and `OmniPBR`.
struct Shared {
    names: [&'static str; 4],
    color: bool,
}

const SHARED: &[Shared] = &[
    Shared {
        names: [
            "metallic",
            "base_metalness",
            "metalness",
            "metallic_constant",
        ],
        color: false,
    },
    Shared {
        names: [
            "roughness",
            "specular_roughness",
            "specular_roughness",
            "reflection_roughness_constant",
        ],
        color: false,
    },
    Shared {
        names: ["ior", "specular_ior", "specular_IOR", ""],
        color: false,
    },
    Shared {
        names: ["clearcoat", "coat_weight", "coat", ""],
        color: false,
    },
    Shared {
        names: ["clearcoatRoughness", "coat_roughness", "coat_roughness", ""],
        color: false,
    },
    Shared {
        names: ["specularColor", "specular_color", "specular_color", ""],
        color: true,
    },
];

/// The USD type of a scalar input: `bool` for switches, `int` for the
/// specular workflow, and `float` otherwise.
fn scalar_type(input: &str) -> &'static str {
    if input.starts_with("enable_")
        || input.starts_with("flip_")
        || matches!(
            input,
            "project_uvw" | "world_or_object" | "geometry_thin_walled" | "thin_walled"
        )
    {
        "bool"
    } else if input == "useSpecularWorkflow" {
        "int"
    } else {
        "float"
    }
}

/// The text of a scalar input of type `kind`.
fn scalar_text(kind: &str, value: f64) -> String {
    match kind {
        "bool" => if value == 0.0 { "0" } else { "1" }.to_owned(),
        "int" => format!("{}", value.round()),
        _ => real32(crate::text::value::round_f32(value)),
    }
}

/// The base color input of each family, and its opacity input.
fn base_names(column: usize) -> (&'static str, &'static str) {
    match column {
        0 => ("diffuseColor", "opacity"),
        1 => ("base_color", "geometry_opacity"),
        2 => ("base_color", "opacity"),
        _ => ("diffuse_color_constant", "opacity_constant"),
    }
}

/// One input to write: its USD type and its value text.
type Input = (String, String, String);

/// The inputs of `shader`, of family `column`, from an appearance whose
/// own shader is `own`, of family `own_column`.
fn inputs(
    appearance: &Appearance,
    (own, own_column): (&str, Option<usize>),
    (shader, column): (&str, Option<usize>),
    written: &mut BTreeSet<String>,
) -> Vec<Input> {
    let prefix = if shader == own {
        String::new()
    } else {
        format!("{shader}/")
    };
    let property = |name: &str| {
        appearance
            .properties
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, value)| value.get())
    };
    let mut out: Vec<Input> = Vec::new();
    let mut names: BTreeSet<String> = BTreeSet::new();
    // The shader's own inputs, as written.
    for key in appearance.properties.keys() {
        let Some(name) = key.as_str().strip_prefix(prefix.as_str()) else {
            continue;
        };
        if !prefix.is_empty() || !name.contains('/') {
            let input = name.split('.').next().unwrap_or(name).to_owned();
            if names.insert(input.clone()) {
                let channels =
                    ["r", "g", "b"].map(|channel| property(&format!("{prefix}{input}.{channel}")));
                if let [Some(r), Some(g), Some(b)] = channels {
                    let kind = if input == "normal" {
                        "normal3f"
                    } else {
                        "color3f"
                    };
                    out.push((
                        kind.to_owned(),
                        input.clone(),
                        tuple32(&[r, g, b].map(crate::text::value::round_f32)),
                    ));
                    for channel in ["r", "g", "b"] {
                        written.insert(format!("{prefix}{input}.{channel}"));
                    }
                } else if let Some(value) = property(&format!("{prefix}{input}")) {
                    let kind = scalar_type(&input);
                    out.push((kind.to_owned(), input.clone(), scalar_text(kind, value)));
                    written.insert(format!("{prefix}{input}"));
                }
            }
        }
    }
    let Some(column) = column else {
        return out;
    };
    // The shared parameters the shader lacks.
    for shared in own_column.map_or(&[][..], |_| SHARED) {
        let Some(own_column) = own_column else {
            break;
        };
        let target = shared.names[column];
        if target.is_empty() || names.contains(target) {
            continue;
        }
        let source = shared.names[own_column];
        if source.is_empty() {
            continue;
        }
        if shared.color {
            let channels = ["r", "g", "b"].map(|channel| property(&format!("{source}.{channel}")));
            if let [Some(r), Some(g), Some(b)] = channels {
                out.push((
                    "color3f".to_owned(),
                    target.to_owned(),
                    tuple32(&[r, g, b].map(crate::text::value::round_f32)),
                ));
                names.insert(target.to_owned());
            }
        } else if let Some(value) = property(source) {
            out.push((
                "float".to_owned(),
                target.to_owned(),
                real32(crate::text::value::round_f32(value)),
            ));
            names.insert(target.to_owned());
        }
    }
    let (base, opacity) = base_names(column);
    if let Some(color) = appearance.base_color {
        if !names.contains(base) {
            out.push((
                "color3f".to_owned(),
                base.to_owned(),
                tuple32(&[color.r(), color.g(), color.b()]),
            ));
            names.insert(base.to_owned());
        }
        if color.a() < 1.0 && !names.contains(opacity) {
            out.push(("float".to_owned(), opacity.to_owned(), real32(color.a())));
            if column == 3 {
                out.push((
                    "bool".to_owned(),
                    "enable_opacity".to_owned(),
                    "1".to_owned(),
                ));
            }
        }
        if shader == own {
            for channel in ["r", "g", "b"] {
                written.insert(format!("{base}.{channel}"));
            }
            written.insert(opacity.to_owned());
        }
    }
    out.sort_by(|left, right| left.1.cmp(&right.1));
    out.dedup_by(|left, right| left.1 == right.1);
    out
}

impl<'ir> Writer<'ir> {
    fn shader(&mut self, name: &str, head: &[String], inputs: &[Input], output: &str) {
        self.prim_head("def Shader", name, &[]);
        self.out.open();
        for line in head {
            self.out.line(line);
        }
        for (kind, input, value) in inputs {
            self.out.line(&format!("{kind} inputs:{input} = {value}"));
        }
        self.out.line(&format!("token outputs:{output}"));
        self.out.close();
    }

    /// Writes the material of `appearance` at `path`.
    pub(super) fn material(&mut self, appearance: &'ir Appearance, name: &str, path: &str) {
        let own = appearance.schema.as_deref().unwrap_or(PREVIEW).to_owned();
        // The shaders of the source: its own, and those its prefixed
        // properties name.
        let mut families: Vec<String> = vec![own.clone()];
        for key in appearance.properties.keys() {
            if let Some((family, _)) = key.as_str().split_once('/') {
                if !families.iter().any(|known| known == family) {
                    families.push(family.to_owned());
                }
            }
        }
        let materialx = families
            .iter()
            .find(|family| MATERIALX.contains(&family.as_str()))
            .cloned()
            .unwrap_or_else(|| OPEN_PBR.to_owned());
        // The MDL shader: the source's own MDL material, an MDL material of
        // another of its contexts, or OmniPBR.
        let mdl = match &appearance.library_id {
            Some(module) => families
                .iter()
                .find(|family| {
                    family.as_str() != PREVIEW
                        && !MATERIALX.contains(&family.as_str())
                        && crate::schema::shading::family(family).is_none_or(|column| column == 3)
                })
                .map(|family| (module.clone(), family.clone())),
            None => families
                .iter()
                .find(|family| OMNI_PBR.contains(&family.as_str()))
                .map(|family| (format!("{family}.mdl"), family.clone())),
        };
        let column = |shader: &str| {
            crate::schema::shading::family(shader).or_else(|| {
                let prefix = if shader == own {
                    String::new()
                } else {
                    format!("{shader}/")
                };
                let omni_pbr = mdl.as_ref().is_some_and(|(_, sub)| sub == shader)
                    && appearance.properties.keys().any(|key| {
                        key.as_str()
                            .strip_prefix(prefix.as_str())
                            .and_then(|input| input.split('.').next())
                            .is_some_and(|input| OMNI_PBR_INPUTS.contains(&input))
                    });
                omni_pbr.then_some(3)
            })
        };
        let own_family = (own.as_str(), column(&own));
        let mut written = BTreeSet::new();
        let mut preview_inputs = inputs(
            appearance,
            own_family,
            (PREVIEW, column(PREVIEW)),
            &mut written,
        );
        let mut materialx_inputs = inputs(
            appearance,
            own_family,
            (&materialx, column(&materialx)),
            &mut written,
        );
        let mdl_column = mdl.as_ref().and_then(|(_, sub)| column(sub));
        let mut mdl_inputs = mdl
            .as_ref()
            .map(|(_, sub)| inputs(appearance, own_family, (sub, mdl_column), &mut written));
        let planned = self.planned_textures(appearance);
        self.prim_head("def Material", name, &[]);
        self.out.open();
        self.out.line(&format!(
            "token outputs:surface.connect = <{path}/PreviewSurface.outputs:surface>"
        ));
        self.out.line(&format!(
            "token outputs:mtlx:surface.connect = <{path}/MaterialX.outputs:out>"
        ));
        if mdl.is_some() {
            self.out.line(&format!(
                "token outputs:mdl:surface.connect = <{path}/Mdl.outputs:out>"
            ));
        }
        preview_inputs.extend(self.preview_textures(&planned, path));
        if let Some(column) = crate::schema::shading::family(&materialx) {
            materialx_inputs.extend(self.materialx_textures(&planned, path, column));
        }
        if let (Some(mdl_inputs), Some(3)) = (&mut mdl_inputs, mdl_column) {
            mdl_inputs.extend(Self::omni_pbr_textures(&planned));
        }
        self.shader(
            "PreviewSurface",
            &[format!("uniform token info:id = {}", quoted(PREVIEW))],
            &preview_inputs,
            "surface",
        );
        self.shader(
            "MaterialX",
            &[format!("uniform token info:id = {}", quoted(&materialx))],
            &materialx_inputs,
            "out",
        );
        if let (Some((module, mdl)), Some(mdl_inputs)) = (mdl, mdl_inputs) {
            self.shader(
                "Mdl",
                &[
                    "uniform token info:implementationSource = \"sourceAsset\"".to_owned(),
                    format!("uniform asset info:mdl:sourceAsset = {}", asset(&module)),
                    format!(
                        "uniform token info:mdl:sourceAsset:subIdentifier = {}",
                        quoted(&mdl)
                    ),
                ],
                &mdl_inputs,
                "out",
            );
        }
        self.out.close();
        let unwritten = appearance
            .properties
            .keys()
            .any(|key| !written.contains(key.as_str()));
        if unwritten {
            self.unwritten
                .reduced_appearances
                .insert(appearance.id.to_string());
        }
    }
}

#[cfg(test)]
mod tests;
