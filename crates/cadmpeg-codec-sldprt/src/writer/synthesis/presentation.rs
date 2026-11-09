// SPDX-License-Identifier: Apache-2.0
//! Preserve writable presentation and metadata while reporting unsupported fields.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{
    tessellation::{Tessellation, TessellationChannel},
    CadIr,
};

use crate::loss::SldprtLossCode;

pub(super) fn reduce(
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), super::super::write_error::WriteError> {
    let mut attributes = Vec::new();
    for attribute in std::mem::take(&mut ir.model.attributes) {
        if super::super::accounting::requires_attribute_omission(&attribute) {
            continue;
        }
        if matches!(&attribute.target, AttributeTarget::Pmi(id) if ir.model.pmi.iter().any(|annotation| &annotation.id == id))
        {
            if crate::pmi::write::attribute_supported(&attribute) {
                attributes.push(attribute);
            } else {
                losses.push(SldprtLossCode::WriterAttributesOmitted.note(format!(
                    "attribute {}: this PMI metadata field has no semantic database encoding",
                    attribute.id
                )));
            }
            continue;
        }
        if super::super::custom_properties::represents(&attribute) {
            attributes.push(attribute);
            continue;
        }
        if !attribute.id.as_str().starts_with("sldprt:")
            && !super::super::supports_metadata_attribute(&attribute)
        {
            continue;
        }
        let projected = if attribute.target == AttributeTarget::Document {
            super::super::MetadataRecord::project(&attribute, 0.001, |_| Ok(()))?
        } else {
            None
        };
        if projected.is_some() {
            attributes.push(attribute);
        } else {
            losses.push(SldprtLossCode::WriterAttributesOmitted.note(format!(
                "attribute {} ({}) has no SLDPRT document-metadata encoding",
                attribute.id, attribute.name
            )));
        }
    }
    ir.model.attributes = attributes;
    appearances(ir, losses)?;
    let mut meshes = Vec::new();
    let multiple_placed_bodies = ir.model.bodies.len() > 1
        && ir.model.bodies.iter().any(|body| {
            body.transform
                .is_some_and(|transform| transform != cadmpeg_ir::transform::Transform::identity())
        });
    for mesh in std::mem::take(&mut ir.model.tessellations) {
        if multiple_placed_bodies && mesh.body.is_none() {
            losses.push(SldprtLossCode::WriterEntityOmitted.note(format!(
                "tessellation {} omitted: no body ownership resolves its placement",
                mesh.id
            )));
            continue;
        }
        let unwritable = mesh.feature_edges().len()
            + mesh.triangle_groups().len()
            + mesh.texture_assignments().len()
            + usize::from(mesh.source_object.is_some());
        let auxiliary_start =
            usize::from(matches!(mesh.channels(), [strips, positions, normals, ..]
                if (strips.item_size(), strips.kind()) == (4, 8)
                    && (positions.item_size(), positions.kind()) == (12, 100)
                    && (normals.item_size(), normals.kind()) == (12, 100)))
                * 3;
        let mut normalized = Vec::new();
        let channels = mesh
            .channels()
            .iter()
            .enumerate()
            .map(|(index, channel)| {
                let fixed = index
                    .checked_sub(auxiliary_start)
                    .and_then(|position| crate::tessellation::AUXILIARY_DESCRIPTORS.get(position))
                    .filter(|(item_size, _)| {
                        usize::try_from(*item_size)
                            .is_ok_and(|size| channel.data().len() % size == 0)
                    });
                let (item_size, kind) =
                    fixed.map_or((channel.item_size(), channel.kind()), |fixed| *fixed);
                for (field, from, to) in [
                    ("item_size", channel.item_size(), item_size),
                    ("kind", channel.kind(), kind),
                ] {
                    if from != to {
                        normalized.push((index, field, from, to));
                    }
                }
                TessellationChannel::new(
                    channel.addressing().clone(),
                    item_size,
                    kind,
                    2,
                    channel.data().to_vec(),
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                crate::writer::write_error::WriteError::Unwritable(format!(
                    "invalid tessellation channel: {error}"
                ))
            })?;
        let strip_lengths = mesh
            .strip_lengths()
            .iter()
            .map(|run| usize::try_from(*run))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| {
                crate::writer::write_error::WriteError::Unwritable(
                    "tessellation strip length overflow".into(),
                )
            })?;
        let auxiliary = &channels[auxiliary_start.min(channels.len())..];
        let keep_channels = unwritable == 0
            && (auxiliary.is_empty()
                || crate::tessellation::auxiliary_channels_are_consistent(
                    &strip_lengths,
                    auxiliary,
                ));
        let clean = Tessellation::from_parts(
            mesh.id.clone(),
            mesh.mesh().clone(),
            if keep_channels { channels } else { Vec::new() },
        )
        .map_err(|error| {
            crate::writer::write_error::WriteError::Unwritable(format!(
                "invalid tessellation: {error}"
            ))
        })?
        .with_body(mesh.body.clone());
        let mut candidate = CadIr::empty();
        candidate.model.tessellations.push(clean.clone());
        match super::super::tessellation_payload(&candidate, 0.001) {
            Ok(_) => {
                if keep_channels {
                    for (index, channel) in mesh.channels().iter().enumerate() {
                        if channel.flags() != 2 {
                            losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                                "tessellation {} channel {index}: model.tessellations[].channels[].flags normalized from {} to 2",
                                mesh.id, channel.flags()
                            )));
                        }
                    }
                    for (index, field, from, to) in &normalized {
                        losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                            "tessellation {} channel {index}: model.tessellations[].channels[].{field} normalized from {from} to {to}",
                            mesh.id
                        )));
                    }
                }
                let omitted = if keep_channels {
                    0
                } else {
                    unwritable + mesh.channels().len()
                };
                if omitted != 0 {
                    losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                        "tessellation {}: {omitted} feature-edge, group, texture, source-association, or auxiliary-channel records omitted", mesh.id
                    )));
                }
                meshes.push(clean);
            }
            Err(super::super::write_error::WriteError::Decline(reason)) => losses.push(
                SldprtLossCode::WriterEntityOmitted
                    .note(format!("tessellation {} omitted: {reason}", mesh.id)),
            ),
            Err(error) => return Err(error),
        }
    }
    ir.model.tessellations = meshes;
    Ok(())
}

fn appearances(
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), super::super::write_error::WriteError> {
    let colors = ir
        .model
        .appearances
        .iter()
        .filter_map(|appearance| {
            appearance
                .base_color
                .map(|color| (appearance.id.clone(), color))
        })
        .collect::<HashMap<_, _>>();
    let mut bindings = Vec::new();
    for binding in std::mem::take(&mut ir.model.appearance_bindings) {
        if !matches!(
            binding.target,
            AppearanceTarget::Body(_) | AppearanceTarget::Face(_)
        ) {
            losses.push(
                SldprtLossCode::WriterAppearanceBindingsOmitted.note(format!(
                    "appearance binding {} has no SLDPRT target encoding",
                    binding.id
                )),
            );
            continue;
        }
        if matches!(
            binding.target,
            AppearanceTarget::Body(_) | AppearanceTarget::Face(_)
        ) && !colors.contains_key(&binding.appearance)
        {
            losses.push(
                SldprtLossCode::WriterAppearanceBindingsOmitted.note(format!(
                    "appearance binding {} has no base color and was omitted",
                    binding.id
                )),
            );
        } else {
            bindings.push(binding);
        }
    }
    ir.model.appearance_bindings = bindings;
    ir.model.appearances.retain(|appearance| {
        if appearance.schema.as_deref() == Some("moVisualProperties_c")
            && appearance.base_color.is_none()
        {
            losses.push(
                SldprtLossCode::WriterUnboundAppearancesOmitted.note(format!(
                    "appearance {} has no base color and was omitted",
                    appearance.id
                )),
            );
            false
        } else {
            true
        }
    });
    match super::super::body_material(ir) {
        Ok(_) => return Ok(()),
        Err(super::super::MaterialError::Decline(_)) => {}
        Err(error) => return Err(error.into()),
    }
    let mut body_colors = ir
        .model
        .bodies
        .iter()
        .filter_map(|body| body.color.map(|color| (body.id.clone(), color)))
        .collect::<HashMap<_, _>>();
    for binding in &ir.model.appearance_bindings {
        if let AppearanceTarget::Body(body) = &binding.target {
            if let Some(color) = colors.get(&binding.appearance) {
                body_colors.insert(body.clone(), *color);
            }
        }
    }
    let region_colors = ir
        .model
        .bodies
        .iter()
        .flat_map(|body| {
            let color = body_colors.get(&body.id).copied();
            body.regions
                .iter()
                .filter_map(move |region| color.map(|color| (region.clone(), color)))
        })
        .collect::<HashMap<_, _>>();
    let shell_colors = ir
        .model
        .regions
        .iter()
        .flat_map(|region| {
            let color = region_colors.get(&region.id).copied();
            region
                .shells
                .iter()
                .filter_map(move |shell| color.map(|color| (shell.clone(), color)))
        })
        .collect::<HashMap<_, _>>();
    let bound_faces = ir
        .model
        .appearance_bindings
        .iter()
        .filter_map(|binding| match &binding.target {
            AppearanceTarget::Face(face) => Some(face.clone()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    for face in &mut ir.model.faces {
        if !bound_faces.contains(&face.id) && face.color.is_none() {
            face.color = shell_colors.get(&face.shell).copied();
        }
    }
    let mut count = 0;
    ir.model.appearance_bindings.retain(|binding| {
        let omitted = matches!(binding.target, AppearanceTarget::Body(_));
        if omitted {
            losses.push(
                SldprtLossCode::WriterAppearanceBindingsOmitted.note(format!(
                "appearance binding {} omitted when distinct body colors were projected onto faces",
                binding.id
            )),
            );
        }
        count += usize::from(omitted);
        !omitted
    });
    for body in &mut ir.model.bodies {
        let name = body.name.take().is_some();
        let color = body.color.take().is_some();
        count += usize::from(name);
        if let Some(fields) = match (name, color) {
            (true, true) => Some("name and color"),
            (true, false) => Some("name"),
            (false, true) => Some("color"),
            (false, false) => None,
        } {
            losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                "body {}: {fields} omitted during material reduction",
                body.id
            )));
        }
    }
    losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!("distinct body materials were reduced to face colors; {count} body material binding/name records omitted")));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::appearances;
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId, BodyId};
    use cadmpeg_ir::topology::Color;
    use std::collections::BTreeMap;

    #[test]
    fn distinct_body_material_reduction_names_each_binding(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut ir = cadmpeg_ir::examples::unit_cube()?;
        ir.model.bodies[0].name = Some("First body".into());
        ir.model.bodies[0].color = Color::new(1.0, 0.0, 0.0, 1.0);
        let body = ir.model.bodies[0].id.clone();
        let mut second = ir.model.bodies[0].clone();
        second.id = BodyId::mint("synthetic:test:body#second")?;
        second.regions.clear();
        ir.model.bodies.push(second);
        let mut ids = Vec::new();
        for (index, color) in [
            Color::new(1.0, 0.0, 0.0, 1.0).ok_or("red color")?,
            Color::new(0.0, 0.0, 1.0, 1.0).ok_or("blue color")?,
        ]
        .into_iter()
        .enumerate()
        {
            let appearance = AppearanceId::mint(format!("synthetic:test:appearance#{index}"))?;
            let binding = AppearanceBindingId::mint(format!("synthetic:test:binding#{index}"))?;
            ir.model.appearances.push(Appearance {
                native_ref: None,
                id: appearance.clone(),
                name: None,
                asset_guid: None,
                library_id: None,
                visual_guid: None,
                physical_token: None,
                schema: None,
                category: None,
                base_color: Some(color),
                properties: BTreeMap::new(),
                textures: Vec::new(),
            });
            ir.model.appearance_bindings.push(AppearanceBinding {
                native_ref: None,
                id: binding.clone(),
                target: AppearanceTarget::Body(ir.model.bodies[index].id.clone()),
                appearance,
                source_entity_id: None,
                object_type: None,
                visible: None,
                channels: BTreeMap::new(),
            });
            ids.push(binding);
        }
        let mut losses = Vec::new();
        appearances(&mut ir, &mut losses)?;
        for id in ids {
            assert!(losses.iter().any(|loss| {
                loss.code.local_code() == "writer.appearance-bindings-omitted"
                    && loss.message.contains(id.as_str())
            }));
        }
        assert!(losses.iter().any(|loss| {
            loss.code.local_code() == "writer.fields-omitted"
                && loss.message.contains(body.as_str())
        }));
        Ok(())
    }
}
