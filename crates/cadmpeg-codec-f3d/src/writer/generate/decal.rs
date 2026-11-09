// SPDX-License-Identifier: Apache-2.0
//! Author an unresolved Decal target and its embedded image.

use cadmpeg_ir::assets::{AssetContent, AssetId};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation};

use super::presentation::{GeneratedDesignRegistry, GeneratedDesignType};
use super::records::{encode_design_metastream, native_lp_ascii, native_lp_utf16};
use crate::native::F3dNative;
use crate::records::decal::DesignDecalImage;
use crate::records::entity_header::SegmentType;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::{
    body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
};

pub(in crate::writer) struct Decal<'a> {
    pub(in crate::writer) feature: FeatureId,
    pub(in crate::writer) asset: AssetId,
    image: &'a DesignDecalImage,
    scope: &'a DesignParameterScope,
    group: &'a DesignConstructionOperandGroup,
    operand: &'a DesignBodyRecipeOperand,
    types: Vec<&'a SegmentType>,
    name: String,
    path: String,
    data: &'a [u8],
}

pub(in crate::writer) fn select<'a>(target: &'a CadIr, native: &'a F3dNative) -> Option<Decal<'a>> {
    let mut features = target.model.features.iter().filter(|feature| matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Native { kind, .. }) if kind.as_str() == "Decal"));
    let feature = features.next()?;
    if features.next().is_some()
        || feature.ordinal != 0
        || feature.suppressed != Some(false)
        || feature.product_definition.is_some()
        || !feature.dependencies.is_empty()
        || feature.source_text.is_some()
        || !feature.source_content.is_empty()
        || !feature.evaluation.outputs().is_empty()
        || feature.source_tag.as_deref() != Some("Decal")
        || feature.source_properties.len() != 2
    {
        return None;
    }
    let FeatureDefinition::Operation(FeatureOperation::Native { parameters, .. }) =
        feature.evaluation.definition()
    else {
        return None;
    };
    if !parameters.is_empty() {
        return None;
    }
    let scope = native
        .design_parameter_scopes
        .iter()
        .find(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))?;
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Decal {
        return None;
    }
    let image = native.design_decal_images.iter().find(|image| {
        image.scope_record_index() == scope.record_index
            && crate::ids::design_segment(&image.id) == crate::ids::design_segment(&scope.id)
    })?;
    let resource = target
        .model
        .assets
        .iter()
        .find(|asset| asset.native_ref.as_deref() == Some(image.id.as_str()))?;
    let AssetContent::Embedded { data } = &resource.content else {
        return None;
    };
    let name = resource.name.as_ref()?.as_str();
    if name != image.asset.name()
        || name.contains(['/', '\\'])
        || name.encode_utf16().count() > 1024
    {
        return None;
    }
    let extension = std::path::Path::new(name).extension()?.to_str()?;
    let media = if extension.eq_ignore_ascii_case("png") {
        "image/png"
    } else if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
        "image/jpeg"
    } else {
        return None;
    };
    if resource.media_type.as_ref()?.as_str() != media {
        return None;
    }
    let path = format!(
        "{}/Design1/Images.BlobParts/{name}",
        crate::manifest::GENERATED_DESIGN_ASSET_FOLDER
    );
    if resource.id != crate::ids::neutral_asset_id(&path) {
        return None;
    }
    let group = native
        .design_construction_operand_groups
        .iter()
        .find(|group| {
            group.scope_record_index == scope.record_index
                && group.record_index == image.target_group_record_index
                && crate::ids::design_segment(&group.id) == crate::ids::design_segment(&scope.id)
        })?;
    if group.scope_reference_ordinal != 0
        || group.role() != crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A
        || group.members().len() != 1
        || !group.lost_edge_references.is_empty()
        || group.frame.variant
        || group.frame.closing_flag
    {
        return None;
    }
    let member = group.members()[0].value;
    let asset = image.asset.record_index();
    let indices = [
        asset,
        asset.checked_add(1)?,
        scope.record_index,
        group.record_index,
        group.record_index.checked_add(1)?,
        group.record_index.checked_add(2)?,
        member,
        member.checked_add(1)?,
        member.checked_add(2)?,
        member.checked_add(3)?,
        member.checked_add(4)?,
        image.asset.entity_suffix(),
    ];
    if indices.contains(&0)
        || indices
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != indices.len()
    {
        return None;
    }
    let operand = native.design_body_recipe_operands.iter().find(|operand| {
        operand.scope_record_index == scope.record_index
            && operand.owner.group() == Some((group.record_index, 0))
            && operand.record_index() == member
            && crate::ids::design_segment(&operand.id) == crate::ids::design_segment(&scope.id)
    })?;
    if operand.references().len() != 1
        || operand.references()[0].form != 3
        || !operand.references()[0].candidate_faces.is_empty()
        || !operand.references()[0].preceding_candidate_faces.is_empty()
        || !operand.references()[0].preceding_body_slots.is_empty()
        || operand.resolved_body_state_id.is_some()
        || operand.resolved_body_slot.is_some()
        || operand.resolved_face_slot.is_some()
        || !operand.resolved_body_face_slots.is_empty()
        || member.checked_add(4) != Some(operand.next_record_index())
        || !operand.selector_tail().is_some_and(|tail| {
            tail.byte_len() == 4
                && tail.digest() == &cadmpeg_ir::hash::digest::Sha256Digest::digest(&[0; 4])
        })
    {
        return None;
    }
    if !native.construction_recipes.iter().any(|recipe| {
        recipe.id == operand.recipe_id
            && recipe.kind == crate::records::recipes::ConstructionRecipeKind::Body
            && recipe.recipe_index == 0
            && recipe
                .record_index
                .as_ref()
                .is_some_and(|index| index.value == 0)
            && recipe.design.as_ref().is_some_and(|design| {
                design.id.value == (member + 3).to_string()
                    && design
                        .selector
                        .as_ref()
                        .is_some_and(|selector| selector.value == member + 4)
            })
    }) {
        return None;
    }
    if !scope
        .reference_members()
        .values()
        .copied()
        .eq([group.record_index, member])
    {
        return None;
    }
    let properties = feature
        .source_properties
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect::<Vec<_>>();
    if properties
        != [
            ("reference:0", group.record_index.to_string().as_str()),
            ("reference:1", member.to_string().as_str()),
        ]
    {
        return None;
    }
    let mut inputs = target
        .model
        .feature_input_topologies
        .iter()
        .filter(|input| input.input_of == feature.id);
    let input = inputs.next()?;
    if inputs.next().is_some()
        || !input.bodies.is_empty()
        || !input.faces.is_empty()
        || !input.edges.is_empty()
        || !input.vertices.is_empty()
        || !input.edge_geometry.is_empty()
        || input.operands.len() != 2
        || !input
            .operands
            .iter()
            .any(|input| input.native_ref.as_str() == group.id)
        || !input
            .operands
            .iter()
            .any(|input| input.native_ref.as_str() == operand.id)
    {
        return None;
    }
    let stream = crate::ids::design_segment(&scope.id)?;
    let mut types = native
        .design_types
        .iter()
        .filter(|ty| crate::ids::design_segment(ty.id()) == Some(stream))
        .collect::<Vec<_>>();
    types.sort_by_key(|ty| ty.class_tag.as_str().parse::<u32>().ok());
    if types.is_empty()
        || types.iter().enumerate().any(|(i, ty)| {
            u32::try_from(i)
                .ok()
                .and_then(|i| i.checked_add(256))
                .is_none_or(|index| ty.class_tag.as_str() != index.to_string())
        })
    {
        return None;
    }
    let type_guids: std::collections::BTreeSet<_> =
        types.iter().map(|ty| ty.type_guid.as_str()).collect();
    if type_guids.len() != types.len() {
        return None;
    }
    Some(Decal {
        feature: feature.id.clone(),
        asset: resource.id.clone(),
        image,
        scope,
        group,
        operand,
        types,
        name: name.to_owned(),
        path,
        data: data.as_bytes(),
    })
}

pub(super) struct EncodedDecal<'a> {
    pub(super) bulk: Vec<u8>,
    pub(super) meta: Vec<u8>,
    pub(super) path: String,
    pub(super) data: &'a [u8],
    pub(super) feature: FeatureId,
    pub(super) asset: AssetId,
}
fn header(out: &mut Vec<u8>, tag: &str, index: u32) -> Result<(), crate::writer::WriteError> {
    native_lp_ascii(out, tag)?;
    out.extend_from_slice(&index.to_le_bytes());
    Ok(())
}
fn reference(out: &mut Vec<u8>, index: u32) {
    out.push(1);
    out.extend_from_slice(&u64::from(index).to_le_bytes());
    out.extend_from_slice(&[0; 2]);
}

pub(super) fn encode<'a>(
    decal: Decal<'a>,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<EncodedDecal<'a>, crate::writer::WriteError> {
    let scope = decal.scope.record_index;
    let group = decal.group.record_index;
    let member = decal.operand.record_index();
    let asset = decal.image.asset.record_index();
    let name_index = asset.checked_add(1).ok_or_else(|| {
        crate::writer::WriteError::Unwritable("Decal name index overflows".into())
    })?;
    let mut out = Vec::new();
    let mut primary = Vec::new();
    let mut secondary = Vec::new();
    let mark = |records: &mut Vec<crate::metastream::RecordIndexEntry>, index, offset| {
        records.push(crate::metastream::RecordIndexEntry {
            entity_id: u64::from(index),
            bulk_offset: cadmpeg_core::decode::u64_from_index(offset),
        });
    };
    let entity_tag = |index| {
        decal
            .types
            .iter()
            .find(|ty| {
                ty.entities
                    .values()
                    .any(|entity| *entity == u64::from(index))
            })
            .map(|ty| ty.class_tag.as_str())
            .ok_or_else(|| {
                crate::writer::WriteError::Unwritable("missing Decal type registration".into())
            })
    };
    ctx.charge_entities(9, "F3D Decal records")?;
    mark(&mut primary, asset, out.len());
    header(&mut out, &decal.image.asset.class_tags()[0], asset)?;
    out.extend_from_slice(&[0; 8]);
    reference(&mut out, decal.image.asset.entity_suffix());
    mark(&mut primary, name_index, out.len());
    header(&mut out, &decal.image.asset.class_tags()[1], name_index)?;
    out.extend_from_slice(&[0; 10]);
    native_lp_utf16(&mut out, &decal.name)?;
    mark(&mut primary, scope, out.len());
    header(&mut out, decal.scope.class_tag.as_str(), scope)?;
    out.extend_from_slice(&[0; 10]);
    reference(&mut out, asset);
    out.push(decal.image.mapping_mode.code());
    reference(&mut out, group);
    out.extend_from_slice(&2_u32.to_le_bytes());
    reference(&mut out, group);
    reference(&mut out, member);
    out.extend_from_slice(&u32::MAX.to_le_bytes());
    native_lp_utf16(&mut out, "Decal")?;
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&[0; 78]);
    mark(&mut secondary, scope, out.len());
    header(&mut out, decal.scope.paired_class_tag.as_str(), scope)?;
    out.extend_from_slice(&[0; 8]);
    mark(&mut primary, group, out.len());
    header(&mut out, decal.group.class_tag.as_str(), group)?;
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&1_u32.to_le_bytes());
    reference(&mut out, member);
    out.extend_from_slice(&[0; 2]);
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&0x0000_0004_0000_0000_u64.to_le_bytes());
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&decal.group.frame.opaque_index.get().to_le_bytes());
    out.extend_from_slice(&decal.group.frame.opaque_scalar().get().to_le_bytes());
    out.extend_from_slice(&decal.group.frame.opaque_index.get().to_le_bytes());
    reference(&mut out, group + 2);
    out.extend_from_slice(&[0; 2]);
    reference(&mut out, group + 1);
    out.push(0);
    reference(&mut out, scope);
    mark(&mut secondary, group, out.len());
    header(&mut out, decal.group.paired_class_tag().as_str(), group)?;
    mark(&mut primary, member, out.len());
    header(&mut out, decal.operand.class_tag.as_str(), member)?;
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&decal.operand.references()[0].design_reference.to_le_bytes());
    out.extend_from_slice(&3_u32.to_le_bytes());
    reference(&mut out, member + 3);
    out.extend_from_slice(&1_u32.to_le_bytes());
    native_lp_utf16(&mut out, decal.operand.asset_id.as_str())?;
    native_lp_utf16(&mut out, decal.operand.context_id.as_str())?;
    out.extend_from_slice(&2_u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    mark(&mut secondary, member, out.len());
    header(&mut out, decal.scope.paired_class_tag.as_str(), member)?;
    for index in member + 1..=member + 4 {
        mark(&mut primary, index, out.len());
        header(&mut out, entity_tag(index)?, index)?;
        if index == member + 3 {
            native_lp_ascii(&mut out, &index.to_string())?;
            out.extend_from_slice(&(member + 4).to_le_bytes());
            out.extend_from_slice(&[0; 12]);
            native_lp_ascii(&mut out, "body_recipe_data")?;
        }
    }
    let included: std::collections::BTreeSet<_> = [
        asset,
        name_index,
        scope,
        group,
        member,
        member + 1,
        member + 2,
        member + 3,
        member + 4,
        decal.image.asset.entity_suffix(),
    ]
    .into_iter()
    .map(u64::from)
    .collect();
    let types = decal
        .types
        .into_iter()
        .map(|source| {
            let mut ty = GeneratedDesignType::from(source);
            ty.entity_ids.retain(|id| included.contains(id));
            ty
        })
        .collect();
    let registry = GeneratedDesignRegistry {
        types,
        body_map: None,
        browser_nodes: None,
    };
    let meta = encode_design_metastream(&registry, &primary, &secondary)?
        .ok_or_else(|| crate::writer::WriteError::Unwritable("missing Decal metadata".into()))?;
    Ok(EncodedDecal {
        bulk: out,
        meta,
        path: decal.path,
        data: decal.data,
        feature: decal.feature,
        asset: decal.asset,
    })
}
