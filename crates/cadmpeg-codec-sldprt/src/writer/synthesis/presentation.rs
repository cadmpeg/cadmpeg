// SPDX-License-Identifier: Apache-2.0
//! Preserve writable presentation and metadata while reporting unsupported fields.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{tessellation::Tessellation, CadIr};

use crate::loss::SldprtLossCode;

pub(super) fn reduce(ir: &mut CadIr, losses: &mut Vec<LossNote>) -> Result<(), CodecError> {
    let mut attributes = Vec::new();
    for attribute in std::mem::take(&mut ir.model.attributes) {
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
        if !attribute.id.as_str().starts_with("sldprt:") {
            continue;
        }
        let projected = if attribute.target == AttributeTarget::Document {
            super::super::MetadataRecord::project(&attribute, 0.001, |_| Ok(()))?
        } else {
            None
        };
        if matches!(
            projected,
            Some(super::super::MetadataRecord::TransformedReferenceEnclosure { .. })
        ) {
            losses.push(SldprtLossCode::WriterAttributesOmitted.note(format!(
                "attribute {} ({}): a synthesized enclosure has no source serializer prologue",
                attribute.id, attribute.name
            )));
        } else if projected.is_some() {
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
        let omitted = mesh.feature_edges().len()
            + mesh.triangle_groups().len()
            + mesh.texture_assignments().len()
            + mesh.channels().len()
            + usize::from(mesh.source_object.is_some());
        let clean = Tessellation::from_parts(mesh.id.clone(), mesh.mesh().clone(), Vec::new())
            .map_err(|error| CodecError::malformed(format_args!("invalid tessellation: {error}")))?
            .with_body(mesh.body.clone());
        let mut candidate = CadIr::empty();
        candidate.model.tessellations.push(clean.clone());
        match super::super::tessellation_payload(&candidate, 0.001) {
            Ok(_) => {
                if omitted != 0 {
                    losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                        "tessellation {}: {omitted} feature-edge, group, texture, source-association, or auxiliary-channel records omitted", mesh.id
                    )));
                }
                meshes.push(clean);
            }
            Err(CodecError::UnsupportedContent(reason)) => losses.push(
                SldprtLossCode::WriterEntityOmitted
                    .note(format!("tessellation {} omitted: {reason}", mesh.id)),
            ),
            Err(error) => return Err(error),
        }
    }
    ir.model.tessellations = meshes;
    Ok(())
}

fn appearances(ir: &mut CadIr, losses: &mut Vec<LossNote>) -> Result<(), CodecError> {
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
        Err(CodecError::UnsupportedContent(_)) => {}
        Err(error) => return Err(error),
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
        count += usize::from(omitted);
        !omitted
    });
    for body in &mut ir.model.bodies {
        count += usize::from(body.name.take().is_some());
        body.color = None;
    }
    losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!("distinct body materials were reduced to face colors; {count} body material binding/name records omitted")));
    Ok(())
}
