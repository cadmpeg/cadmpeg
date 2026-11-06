// SPDX-License-Identifier: Apache-2.0
//! Keep encodable appearance fields without sacrificing their owner geometry.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use sha2::{Digest, Sha256};

use crate::loss::F3dLossCode;

pub(super) fn prepare(
    target: &mut CadIr,
    native: &crate::native::F3dNative,
    losses: &mut Vec<LossNote>,
) {
    for appearance in &mut target.model.appearances {
        if appearance.visual_guid.is_none() && appearance.asset_guid.is_none() {
            appearance.asset_guid = Some(asset_guid(appearance.id.as_str()));
        }
    }
    let appearances = &target.model.appearances;
    let bodies = &target.model.bodies;
    let mut entity_counts = std::collections::BTreeMap::new();
    for binding in &target.model.appearance_bindings {
        if let Some(entity) = &binding.source_entity_id {
            *entity_counts.entry(entity.clone()).or_insert(0usize) += 1;
        }
    }
    let mut neutral_targets = std::collections::BTreeSet::new();
    target.model.appearance_bindings.retain(|binding| {
        let cadmpeg_ir::appearance::AppearanceTarget::Body(body_id) = &binding.target else {
            return false;
        };
        let Some(body) = bodies.iter().find(|body| &body.id == body_id) else {
            return false;
        };
        let Some(appearance) = appearances
            .iter()
            .find(|appearance| appearance.id == binding.appearance)
        else {
            return false;
        };
        let valid_appearance = (appearance.base_color.is_some()
            || (appearance.schema.is_none()
                && appearance.library_id.is_none()
                && appearance.category.is_none()
                && appearance.properties.is_empty()
                && appearance.textures.is_empty()))
            && appearance.physical_token.as_deref().is_some_and(|token| {
                token.starts_with("PrismMaterial") && !token.contains("_physmat_aspects")
            })
            && appearance
                .visual_guid
                .as_deref()
                .or(appearance.asset_guid.as_deref())
                .is_some_and(|guid| crate::design::presentation::visual_token(guid).is_some());
        if binding.source_entity_id.is_none() {
            return body.visible.is_some()
                && binding.visible.is_none()
                && binding.channels.is_empty()
                && valid_appearance
                && neutral_targets.insert(body_id.clone());
        }
        let Some(entity) = binding.source_entity_id.as_deref() else {
            return false;
        };
        let Some(assignment) = native
            .design_material_assignments
            .iter()
            .find(|assignment| assignment.entity_id.as_str() == entity)
        else {
            return false;
        };
        let channels_match = match native
            .act_entities
            .iter()
            .find(|row| row.entity_id() == entity)
        {
            Some(row) => {
                let channels = row.channel_group().channels();
                channels.len() == binding.channels.len()
                    && channels.iter().all(|(name, channel)| {
                        binding
                            .channels
                            .get(name.as_str())
                            .is_some_and(|value| value == channel.value.as_str())
                    })
            }
            None => binding.channels.is_empty(),
        };
        body.visible.is_some()
            && channels_match
            && entity_counts.get(entity) == Some(&1)
            && valid_appearance
            && body.native_ref.as_deref().is_some_and(|id| {
                native.body_visibilities.iter().any(|visibility| {
                    visibility.body.as_str() == id
                        && visibility.entity_suffix == assignment.entity_id.suffix()
                })
            })
            && native.design_types.iter().any(|kind| {
                kind.type_guid
                    .as_str()
                    .eq_ignore_ascii_case(crate::design::presentation::BODY_PRESENTATION_TYPE_GUID)
                    && kind.version == crate::design::presentation::BODY_PRESENTATION_TYPE_VERSION
                    && kind
                        .entities
                        .values()
                        .any(|id| *id == assignment.entity_id.suffix())
            })
            && native.design_types.iter().any(|kind| {
                kind.type_guid
                    .as_str()
                    .eq_ignore_ascii_case(crate::design::presentation::BREP_CONTAINER_TYPE_GUID)
                    && kind.version == crate::design::presentation::BREP_CONTAINER_TYPE_VERSION
                    && kind.entities.values().count() == 1
            })
            && native.design_types.iter().any(|kind| {
                kind.type_guid
                    .as_str()
                    .eq_ignore_ascii_case(crate::design::presentation::BODY_SCENE_NODE_TYPE_GUID)
                    && kind.version == crate::design::presentation::BODY_SCENE_NODE_TYPE_VERSION
                    && kind
                        .entities
                        .values()
                        .any(|id| assignment.entity_id.suffix().checked_add(1) == Some(*id))
            })
            && binding.visible.is_none()
    });
    let bound = target
        .model
        .appearance_bindings
        .iter()
        .map(|binding| binding.appearance.clone())
        .collect::<std::collections::BTreeSet<_>>();
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
        if bound.contains(&appearance.id)
            && appearance.base_color.is_none()
            && appearance.name.as_deref().is_some_and(|name| !name.starts_with("Prism-"))
        {
            appearance.name = None;
            losses.push(F3dLossCode::WriterAppearancePropertiesOmitted.note(format!(
                "appearance {} omits its name because the Design material preset cannot encode it", appearance.id,
            )));
        }
        if !bound.contains(&appearance.id) && appearance.physical_token.take().is_some() {
            losses.push(F3dLossCode::WriterAppearancePropertiesOmitted.note(format!(
                "appearance {} omits its physical-material assignment token; the supported asset properties remain", appearance.id,
            )));
        }
        if appearance.base_color.is_none()
            && !bound.contains(&appearance.id)
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
    const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut hash = Sha256::new();
    hash.update(b"cadmpeg:f3d:appearance:v1\0");
    hash.update(identity.as_bytes());
    let digest = hash.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX_DIGITS[usize::from(byte >> 4)]),
                char::from(HEX_DIGITS[usize::from(byte & 0x0f)]),
            ]
        })
        .collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}
