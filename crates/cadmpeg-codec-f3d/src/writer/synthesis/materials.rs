// SPDX-License-Identifier: Apache-2.0
//! Create the Design identities needed by a neutral body material.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::native::F3dNative;
use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
use crate::records::identity::{RecordedValue, ReferenceRun};
use crate::records::references::DesignMaterialAssignment;

pub(super) fn author_neutral(
    target: &mut CadIr,
    native: &mut F3dNative,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut used = native
        .body_visibilities
        .iter()
        .map(|row| row.entity_suffix)
        .collect::<std::collections::BTreeSet<_>>();
    let registered = native.design_types.iter().flat_map(|kind| kind.entities.values().copied()).collect::<std::collections::BTreeSet<_>>();
    used.extend(registered.iter().copied());
    let mut presentations = Vec::new();
    let mut scenes = Vec::new();
    let mut container = native.design_types.iter().find(|kind| {
        kind.type_guid.as_str().eq_ignore_ascii_case(crate::design::presentation::BREP_CONTAINER_TYPE_GUID)
            && kind.version == crate::design::presentation::BREP_CONTAINER_TYPE_VERSION
    }).and_then(|kind| {
        let mut values = kind.entities.values().copied();
        let first = values.next()?;
        values.next().is_none().then_some(first)
    });
    for binding in &mut target.model.appearance_bindings {
        if binding.source_entity_id.is_some() {
            continue;
        }
        let cadmpeg_ir::appearance::AppearanceTarget::Body(body_id) = &binding.target else {
            continue;
        };
        let Some(body) = target.model.bodies.iter().find(|body| &body.id == body_id) else {
            continue;
        };
        let Some(visibility_index) = body.native_ref.as_deref().and_then(|id| {
            native
                .body_visibilities
                .iter()
                .position(|row| row.body.as_str() == id)
        }) else {
            continue;
        };
        let Some(appearance) = target
            .model
            .appearances
            .iter()
            .find(|appearance| appearance.id == binding.appearance)
        else {
            continue;
        };
        let Some(visual) = appearance
            .visual_guid
            .as_deref()
            .or(appearance.asset_guid.as_deref())
        else {
            continue;
        };
        let Some(physical) = appearance.physical_token.as_deref() else {
            continue;
        };
        let old = native.body_visibilities[visibility_index].entity_suffix;
        let available = old.checked_add(2).is_some_and(|end| {
            old.checked_add(3).is_some()
                && !registered.contains(&old)
                && !used.contains(&(old + 1))
                && (container.is_some() || !used.contains(&end))
        });
        let entity = if available {
            old
        } else {
            let Some(value) = used.last().copied().unwrap_or(0).checked_add(1) else {
                continue;
            };
            value
        };
        let Some(last) = entity.checked_add(3) else {
            continue;
        };
        let scene = entity + 1;
        let container_id = *container.get_or_insert(entity + 2);
        if entity != old && !registered.contains(&old) {
            used.remove(&old);
        }
        used.extend([entity, scene, container_id, last]);
        native.body_visibilities[visibility_index].entity_suffix = entity;
        let visibility = &native.body_visibilities[visibility_index];
        let entity_id = format!("0_{entity}");
        binding.source_entity_id = Some(entity_id.clone());
        let ordinal = presentations.len();
        native
            .design_material_assignments
            .push(DesignMaterialAssignment {
                id: format!("f3d:generated/Design1/BulkStream.dat:material-assignment#{ordinal}"),
                body_binding_id: format!(
                    "f3d:generated/Design1/BulkStream.dat:design-body-binding#{ordinal}"
                )
                .try_into()
                .map_err(CodecError::Malformed)?,
                asm_body_key: visibility.asm_body_key(),
                asm_body_key_offset: None,
                entity_suffix_offset: None,
                entity_id: entity_id.try_into().map_err(CodecError::Malformed)?,
                entity_id_offset: None,
                visual_guid: visual
                    .to_owned()
                    .try_into()
                    .map_err(CodecError::malformed)?,
                visual_guid_offset: None,
                physical_token: Some(RecordedValue {
                    value: physical.to_owned(),
                    offset: None,
                }),
                visual_preset: appearance
                    .name
                    .as_ref()
                    .filter(|name| name.starts_with("Prism-"))
                    .map(|value| RecordedValue {
                        value: value.clone(),
                        offset: None,
                    }),
            });
        presentations.push(entity);
        scenes.push(scene);
        losses.push(
            crate::loss::F3dLossCode::WriterAppearancePropertiesOmitted.note(format!(
                "appearance binding {} receives a generated Design entity identity and object type",
                binding.id
            )),
        );
    }
    target
        .model
        .appearance_bindings
        .retain(|binding| binding.source_entity_id.is_some());
    if presentations.is_empty() {
        return Ok(());
    }
    let Some(container) = container else {
        return Ok(());
    };
    let registrations = [
        (
            crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
            crate::design::presentation::BODY_PRESENTATION_BASE_TYPE_GUID,
            19,
            "Body",
            presentations,
        ),
        (
            crate::design::presentation::BODY_SCENE_NODE_TYPE_GUID,
            "EB7847AF-E60D-4AB0-A736-4AC00C1F1D21",
            1,
            "Scene",
            scenes,
        ),
        (
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
            crate::records::body_ownership::CONTAINER_BASE_GUID,
            4,
            "Body",
            vec![container],
        ),
    ];
    for (guid, base, version, module, entities) in registrations {
        if let Some(existing) = native.design_types.iter_mut().find(|kind| kind.type_guid.as_str().eq_ignore_ascii_case(guid) && kind.version == version) {
            let mut combined = existing.entities.values().copied().collect::<Vec<_>>();
            for entity in entities {
                if !combined.contains(&entity) { combined.push(entity); }
            }
            existing.set_entities(ReferenceRun::unlocated(combined));
            continue;
        }
        let ordinal = u32::try_from(native.design_types.len()).map_err(CodecError::malformed)?;
        native.design_types.push(
            SegmentType::try_new(
                format!("f3d:generated/Design1/MetaStream.dat:design-type#{ordinal}"),
                SegmentTypeData {
                    class_tag: (256 + ordinal)
                        .to_string()
                        .try_into()
                        .map_err(CodecError::Malformed)?,
                    byte_offset: Some(u64::from(ordinal)),
                    type_guid: guid.to_owned().try_into().map_err(CodecError::Malformed)?,
                    type_guid_offset: None,
                    base_type_guid: BaseTypeGuid::Guid {
                        value: base.to_owned().try_into().map_err(CodecError::Malformed)?,
                        offset: None,
                    },
                    version,
                    version_offset: None,
                    module: module.to_owned(),
                    entities: ReferenceRun::unlocated(entities),
                },
            )
            .map_err(CodecError::Malformed)?,
        );
    }
    Ok(())
}
