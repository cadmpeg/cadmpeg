// SPDX-License-Identifier: Apache-2.0
//! Current bindings select material-list positions in the emitted face order.
use super::Provider;
use crate::native::{entry_contents::EntryContents, EntryRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{
    appearance::{Appearance, AppearanceTarget},
    CadIr,
};
use std::collections::{BTreeMap, HashMap, HashSet};

pub(super) fn write(
    ir: &CadIr,
    names: &HashMap<String, String>,
    body_names: &HashMap<String, String>,
    providers: &mut BTreeMap<String, Provider>,
    entries: &mut BTreeMap<String, Vec<u8>>,
    losses: &mut Vec<LossNote>,
    outcomes: &mut cadmpeg_ir::codec::write::NativeDispositions,
) -> Result<(), CodecError> {
    if !ir.scope.model {
        return Ok(());
    }
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(());
    };
    let members = namespace.arena_as::<EntryRecord>("entries")?;
    for (provider_name, provider) in providers.iter_mut() {
        let Some(property) = provider.native.get("ShapeAppearance").cloned() else {
            continue;
        };
        let Some(file) = property
            .values
            .iter()
            .find_map(|value| value.attributes.get("file"))
        else {
            continue;
        };
        let Some(entry) = members.iter().find(|entry| entry.name() == file) else {
            continue;
        };
        let Some(source_version) = entry.contents().iter().find_map(|content| match content {
            EntryContents::Material { version, .. } => Some(*version),
            _ => None,
        }) else {
            continue;
        };
        let shared = super::super::property_payloads::appearances::claims(
            namespace,
            entry,
            &property.type_name,
        )? > 1;
        let all = ir
            .model
            .appearances
            .iter()
            .filter(|appearance| appearance.native_ref.as_deref() == Some(entry.id()))
            .collect::<Vec<_>>();
        let mut faces = BTreeMap::<usize, &Appearance>::new();
        let mut body = None;
        for binding in &ir.model.appearance_bindings {
            let Some(appearance) = all
                .iter()
                .copied()
                .find(|appearance| appearance.id == binding.appearance)
            else {
                continue;
            };
            if super::colors::target_name(ir, &binding.target, names, body_names)
                != Some(provider_name)
            {
                continue;
            }
            let face = match &binding.target {
                AppearanceTarget::Face(id) => Some(id.as_str()),
                AppearanceTarget::Record { arena, id } if arena.as_str() == "faces" => {
                    Some(id.as_str())
                }
                _ => None,
            };
            if let Some(face) = face {
                if let Some(index) = names
                    .get(&format!("{face}:subelement"))
                    .and_then(|name| name.strip_prefix("Face"))
                    .and_then(|index| index.parse::<usize>().ok())
                    .filter(|index| *index > 0)
                {
                    faces.entry(index).or_insert(appearance);
                } else {
                    losses.push(
                        crate::loss::FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                            "appearance binding {} has no emitted material face index",
                            binding.id
                        )),
                    );
                }
            } else if match &binding.target {
                AppearanceTarget::Body(_) => true,
                AppearanceTarget::Record { arena, .. } => arena.as_str() == "bodies",
                _ => false,
            } {
                body.get_or_insert(appearance);
            }
        }
        let selected = if !faces.is_empty() {
            let count = ir
                .model
                .faces
                .iter()
                .filter(|face| names.get(face.id.as_str()) == Some(provider_name))
                .filter_map(|face| {
                    names
                        .get(&format!("{}:subelement", face.id))
                        .and_then(|name| name.strip_prefix("Face"))
                        .and_then(|index| index.parse::<usize>().ok())
                })
                .max()
                .unwrap_or(0);
            if faces.len() != count || (1..=count).any(|index| !faces.contains_key(&index)) {
                provider.properties.remove("ShapeAppearance");
                losses.push(cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!("GUI property {} has incomplete current material face assignments; the material array is omitted",property.id)));
                continue;
            }
            faces.values().copied().collect::<Vec<_>>()
        } else if let Some(body) = body {
            vec![body]
        } else if shared {
            provider.properties.remove("ShapeAppearance");
            losses.push(cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!("GUI property {} shares entry {} and has no current topology binding that selects its material assets; the list is omitted", property.id, entry.id())));
            continue;
        } else {
            all.clone()
        };
        let used = selected
            .iter()
            .map(|appearance| appearance.id.as_str())
            .collect::<HashSet<_>>();
        if !faces.is_empty() || body.is_some() {
            for asset in all.iter().filter(|asset| {
                !used.contains(asset.id.as_str())
                    && !ir.model.appearance_bindings.iter().any(|binding| {
                        binding.appearance == asset.id
                            && super::colors::target_name(ir, &binding.target, names, body_names)
                                .is_some_and(|owner| owner != provider_name)
                    })
            }) {
                losses.push(crate::loss::FreecadLossCode::ExportAppearanceNotTransferred.note(format!("unbound appearance {} cannot be appended to a material array with current topology assignments",asset.id)));
            }
        }
        let mut has_strings = false;
        for appearance in &selected {
            if super::super::property_payloads::appearances::needs_strings(ir, appearance)? {
                has_strings = true;
                break;
            }
        }
        let version = if has_strings {
            3
        } else if source_version == 3 {
            2
        } else {
            source_version
        };
        if source_version == 3 && version == 2 {
            losses.push(crate::loss::FreecadLossCode::ExportPresentationNotTransferred.note(format!("GUI property {} entry {} regenerates material version2 without current string Maps or GUIDs; native version3 string-channel layout is omitted",property.id,entry.id())));
            if let Some(values) = entry.contents().iter().find_map(|content| match content {
                EntryContents::Material { version: 3, values } => Some(values),
                _ => None,
            }) {
                let partial = values
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| {
                        (value.fixed.is_none()
                            || value.image.is_none()
                            || value.image_path.is_none()
                            || value.uuid.is_none())
                        .then_some(index)
                    })
                    .collect::<Vec<_>>();
                if !partial.is_empty() {
                    losses.push(cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!("GUI property {} entry {} omits incomplete native material positions {partial:?} while regenerating current numeric materials as version2",property.id,entry.id())));
                }
            }
        }
        let mut values = Vec::new();
        let mut writable = true;
        for appearance in selected {
            if let Some(mut value) = super::super::property_payloads::appearances::material(
                ir,
                appearance,
                &property.name,
                version,
                losses,
            )? {
                if source_version < 3 && version == 3 {
                    super::super::property_payloads::appearances::normalize_legacy_strings(
                        ir, appearance, &mut value, losses,
                    )?;
                }
                values.push(value);
            } else if !faces.is_empty() || body.is_some() {
                provider.properties.remove("ShapeAppearance");
                writable = false;
                break;
            }
        }
        if !writable {
            continue;
        }
        let content = EntryContents::Material { version, values };
        if let Some(bytes) =
            super::super::property_payloads::encode(&content, &property.id, losses)?
        {
            let filename = if shared {
                let mut suffix = 0_usize;
                loop {
                    let candidate = format!("{provider_name}.ShapeAppearance.{suffix}");
                    if !entries.contains_key(&candidate) {
                        break candidate;
                    }
                    suffix = suffix
                        .checked_add(1)
                        .ok_or_else(|| CodecError::malformed("material member suffix overflows"))?;
                }
            } else {
                file.clone()
            };
            let mut output = property.clone();
            for value in &mut output.values {
                if value.tag == "MaterialList" && version != source_version {
                    value
                        .attributes
                        .insert("version".into(), version.to_string());
                }
                if value.attributes.get("file") == Some(file) {
                    value.attributes.insert("file".into(), filename.clone());
                }
            }
            for side_entry in &mut output.side_entries {
                if side_entry == file {
                    *side_entry = filename.clone();
                }
            }
            let payloads = std::collections::BTreeSet::from([filename.clone()]);
            let Some(xml) = super::xml::native_gui_payloads(&output, names, &payloads)? else {
                losses.push(
                    cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
                        "GUI property {} has no writable current material pointer",
                        property.id
                    )),
                );
                continue;
            };
            provider.properties.insert("ShapeAppearance".into(), xml);
            entries.insert(filename, bytes);
            super::super::dispositions::record(
                outcomes,
                "entries",
                entry.id(),
                cadmpeg_ir::codec::write::NativeDisposition::Regenerated,
            );
        }
    }
    Ok(())
}
