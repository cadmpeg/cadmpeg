// SPDX-License-Identifier: Apache-2.0
//! Keep encodable appearance fields without sacrificing their owner geometry.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use sha2::{Digest, Sha256};

use crate::loss::F3dLossCode;

const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

pub(super) fn prepare(target: &mut CadIr, losses: &mut Vec<LossNote>) {
    use cadmpeg_ir::appearance::AppearanceTarget;
    let mut bindings = std::collections::BTreeMap::new();
    for binding in &target.model.appearance_bindings {
        bindings
            .entry(binding.target.clone())
            .or_insert_with(Vec::new)
            .push(binding);
    }
    for (owner, assignments) in bindings {
        if let [binding] = assignments.as_slice() {
            if let Some(color) = target
                .model
                .appearances
                .iter()
                .find(|appearance| appearance.id == binding.appearance)
                .and_then(|appearance| appearance.base_color)
            {
                match owner {
                    AppearanceTarget::Body(id) => {
                        if let Some(body) =
                            target.model.bodies.iter_mut().find(|body| body.id == id)
                        {
                            body.color.get_or_insert(color);
                        }
                    }
                    AppearanceTarget::Face(id) => {
                        if let Some(face) = target.model.faces.iter_mut().find(|face| face.id == id)
                        {
                            face.color.get_or_insert(color);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    target.model.appearance_bindings.clear();
    let mut omitted = std::collections::BTreeSet::new();
    for appearance in &mut target.model.appearances {
        let schema = appearance.schema.as_deref().unwrap_or("GenericSchema");
        let unsupported_schema = !matches!(
            schema,
            "GenericSchema"
                | "PrismOpaqueSchema"
                | "PrismMetalSchema"
                | "PrismTransparentSchema"
                | "PhysMatSchema"
                | "StructuralMetalSchema"
                | "StructuralPlasticSchema"
                | "ThermalSolidSchema"
        );
        if unsupported_schema || !appearance.textures.is_empty() {
            losses.push(F3dLossCode::WriterAppearancePropertiesOmitted.note(format!(
                "appearance {} keeps its base color; unsupported schema or connected texture fields are omitted", appearance.id,
            )));
            appearance.textures.clear();
            if unsupported_schema {
                appearance.schema = Some("GenericSchema".into());
            }
        }
        if appearance.base_color.is_some()
            && matches!(
                appearance.schema.as_deref(),
                Some(
                    "PhysMatSchema"
                        | "StructuralMetalSchema"
                        | "StructuralPlasticSchema"
                        | "ThermalSolidSchema"
                )
            )
        {
            appearance.schema = Some("GenericSchema".into());
            losses.push(F3dLossCode::WriterAppearancePropertiesOmitted.note(format!(
                "appearance {} uses GenericSchema to retain its neutral color; the physical-only shader schema is omitted", appearance.id,
            )));
        }
        // A generated asset identity selects layout; it supplies no content.
        if appearance.visual_guid.is_none() && appearance.asset_guid.is_none() {
            appearance.asset_guid = Some(asset_guid(appearance.id.as_str()));
        }
        if appearance.physical_token.take().is_some() {
            losses.push(F3dLossCode::WriterAppearancePropertiesOmitted.note(format!(
                "appearance {} omits its physical-material assignment token; the supported asset properties remain", appearance.id,
            )));
        }
        if appearance.base_color.is_none()
            && matches!(
                appearance.schema.as_deref().unwrap_or("GenericSchema"),
                "GenericSchema"
                    | "PrismOpaqueSchema"
                    | "PrismMetalSchema"
                    | "PrismTransparentSchema"
            )
        {
            omitted.insert(appearance.id.clone());
            losses.push(F3dLossCode::WriterAppearancePropertiesOmitted.note(format!(
                "visual appearance {} has no base color for the Protein visual carrier and is omitted", appearance.id,
            )));
            continue;
        }
        let written: &[&str] = match appearance.schema.as_deref().unwrap_or("GenericSchema") {
            "GenericSchema" => &["reflectivity_at_0deg", "refraction_index"],
            "PrismOpaqueSchema" | "PrismMetalSchema" => &["surface_roughness"],
            "PrismTransparentSchema" => &["refraction_index"],
            _ => &[],
        };
        appearance.properties.retain(|key, value| {
            let value = value.get();
            let supported = written.contains(&key.as_str()) && match key.as_str() {
                "reflectivity_at_0deg" | "surface_roughness" => (0.0..=1.0).contains(&value),
                "refraction_index" => (1.0..=4.0).contains(&value),
                _ => false,
            };
            if !supported {
                losses.push(F3dLossCode::WriterAppearancePropertiesOmitted.note(format!(
                    "appearance {} omits unsupported shader property {key}; its base color and other supported properties remain", appearance.id,
                )));
            }
            supported
        });
    }
    target
        .model
        .appearances
        .retain(|appearance| !omitted.contains(&appearance.id));
    for body in &mut target.model.bodies {
        opaque(&mut body.color, body.id.as_str(), losses);
    }
    for face in &mut target.model.faces {
        opaque(&mut face.color, face.id.as_str(), losses);
    }
}

fn opaque(color: &mut Option<cadmpeg_ir::topology::Color>, id: &str, losses: &mut Vec<LossNote>) {
    if let Some(value) = color.filter(|color| color.a() != 1.0) {
        *color = value.with_alpha(1.0);
        losses.push(F3dLossCode::WriterAppearancePropertiesOmitted.note(format!(
            "{id} retains its RGB color; direct-color opacity is not encoded",
        )));
    }
}

fn asset_guid(identity: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"cadmpeg:f3d:appearance:v1\0");
    hash.update(identity.as_bytes());
    let digest = hash.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut hex = String::with_capacity(32);
    for byte in bytes {
        hex.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        hex.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}
