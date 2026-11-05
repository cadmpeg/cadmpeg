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
    if !native.design_types.is_empty()
        || !native.design_material_assignments.is_empty()
        || target.model.appearance_bindings.len() != 1
    {
        return Ok(());
    }
    let binding = &mut target.model.appearance_bindings[0];
    if binding.source_entity_id.is_some() {
        return Ok(());
    }
    let cadmpeg_ir::appearance::AppearanceTarget::Body(body_id) = &binding.target else {
        return Ok(());
    };
    let Some(body) = target.model.bodies.iter().find(|body| &body.id == body_id) else {
        return Ok(());
    };
    let Some(visibility_index) = body.native_ref.as_deref().and_then(|id| {
        native
            .body_visibilities
            .iter()
            .position(|visibility| visibility.body.as_str() == id)
    }) else {
        return Ok(());
    };
    let Some(appearance) = target
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
    else {
        return Ok(());
    };
    let Some(visual) = appearance
        .visual_guid
        .as_deref()
        .or(appearance.asset_guid.as_deref())
    else {
        return Ok(());
    };
    let Some(physical) = appearance.physical_token.as_deref() else {
        return Ok(());
    };
    let original_entity = native.body_visibilities[visibility_index].entity_suffix;
    let occupied = |value| native.body_visibilities.iter().enumerate().any(|(index, visibility)| {
        index != visibility_index && visibility.entity_suffix == value
    });
    let entity = if original_entity.checked_add(1).is_some_and(occupied)
        || original_entity.checked_add(2).is_some_and(occupied)
    {
        native.body_visibilities.iter().map(|visibility| visibility.entity_suffix)
            .max().and_then(|value| value.checked_add(1)).unwrap_or(u64::MAX)
    } else {
        original_entity
    };
    let Some((scene, container)) = entity.checked_add(3).map(|_| (entity + 1, entity + 2)) else {
        let binding_id = binding.id.clone();
        let appearance_id = binding.appearance.clone();
        target.model.appearance_bindings.clear();
        if let Some(appearance) = target
            .model
            .appearances
            .iter_mut()
            .find(|appearance| appearance.id == appearance_id)
        {
            appearance.physical_token = None;
        }
        losses.push(crate::loss::F3dLossCode::WriterAppearancePropertiesOmitted.note(format!(
            "appearance binding {binding_id} and appearance {appearance_id} physical token exceed the Design entity identity range"
        )));
        return Ok(());
    };
    native.body_visibilities[visibility_index].entity_suffix = entity;
    let visibility = &native.body_visibilities[visibility_index];
    let registrations = [
        (
            crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
            crate::design::presentation::BODY_PRESENTATION_BASE_TYPE_GUID,
            19,
            "Body",
            entity,
        ),
        (
            crate::design::presentation::BODY_SCENE_NODE_TYPE_GUID,
            "EB7847AF-E60D-4AB0-A736-4AC00C1F1D21",
            1,
            "Scene",
            scene,
        ),
        (
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
            crate::records::body_ownership::CONTAINER_BASE_GUID,
            4,
            "Body",
            container,
        ),
    ];
    for (ordinal, (guid, base, version, module, entity)) in registrations.into_iter().enumerate() {
        let ordinal = u32::try_from(ordinal).map_err(CodecError::malformed)?;
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
                    entities: ReferenceRun::unlocated(vec![entity]),
                },
            )
            .map_err(CodecError::Malformed)?,
        );
    }
    let entity_id = format!("0_{entity}");
    binding.source_entity_id = Some(entity_id.clone());
    native
        .design_material_assignments
        .push(DesignMaterialAssignment {
            id: "f3d:generated/Design1/BulkStream.dat:material-assignment#0".into(),
            body_binding_id: "f3d:generated/Design1/BulkStream.dat:design-body-binding#0"
                .to_owned()
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
    losses.push(
        crate::loss::F3dLossCode::WriterAppearancePropertiesOmitted.note(format!(
            "appearance binding {} receives a generated Design entity identity and object type",
            binding.id
        )),
    );
    Ok(())
}
