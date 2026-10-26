// SPDX-License-Identifier: Apache-2.0
//! Stored-body Base features and their current-model body joins.

use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_core::decode::{unique_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureOperation, FeatureResultMembers, FeatureResultTopology,
};
use cadmpeg_ir::ids::FeatureResultTopologyId;

use super::{
    FeatureFamily, FeatureInventory, PmDcFeature, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcKernelEntityLink,
};
use crate::design::DesignInventory;
use crate::layout::{
    meta_external_reference as external, meta_external_revision as revision,
    meta_segment_pair as pair,
};
use crate::pmdc::PmDcReference;
use crate::reader;
use crate::records::MetaSection;
use crate::rse::{
    RecordFrameState, RseInventory, SegmentBulkState, SegmentDescriptor, SegmentKind,
    SegmentMetaState,
};

pub(crate) const RESULT_LINK_PROPERTY: &str = "result_body_link";

fn one<T>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

fn section_count(section: &MetaSection<'_>, width: usize) -> Option<usize> {
    let count = section
        .payload
        .counted(u64::from(section.discriminator), width)?;
    (count.get().checked_mul(width)? == section.payload.remaining()).then_some(count.get())
}

fn entry<'a>(section: &MetaSection<'a>, index: u32, width: usize) -> Option<View<'a>> {
    section_count(section, width)?;
    if index >= section.discriminator {
        return None;
    }
    let offset = usize::try_from(index).ok()?.checked_mul(width)?;
    let start = section.payload.start().checked_add(offset)?;
    section.payload.child(start, start.checked_add(width)?)
}

fn reference_counts(
    ctx: &DecodeContext<'_>,
    revisions: &MetaSection<'_>,
    references: &MetaSection<'_>,
) -> Result<Option<Vec<u32>>, CodecError> {
    let Some(count) = section_count(revisions, revision::LEN) else {
        return Ok(None);
    };
    if section_count(references, external::LEN).is_none() {
        return Ok(None);
    }
    let mut counts = ctx.alloc_filled(count, 0_u32, "count Inventor external references")?;
    for index in 0..references.discriminator {
        let Some(mut node) = entry(references, index, external::LEN) else {
            return Ok(None);
        };
        let revision_index = reader::u32(&mut node, "external revision index")?;
        let Some(count) = usize::try_from(revision_index)
            .ok()
            .and_then(|index| counts.get_mut(index))
        else {
            return Ok(None);
        };
        *count = count
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("Inventor external reference count overflow"))?;
    }
    for (index, &count) in counts.iter().enumerate() {
        let Some(mut node) = u32::try_from(index)
            .ok()
            .and_then(|index| entry(revisions, index, revision::LEN))
        else {
            return Ok(None);
        };
        reader::take(
            &mut node,
            revision::SEGMENT_PAIR_INDEX,
            "external revision identifier",
        )?;
        reader::u16(&mut node, "external segment pair index")?;
        if u32::from(reader::u16(&mut node, "external revision reference count")?) != count {
            return Ok(None);
        }
    }
    Ok(Some(counts))
}

fn framed_records<'a, 'b>(
    segment: &'b SegmentDescriptor<'a>,
) -> Option<&'b [crate::records::RseRecordFrame<'a>]> {
    let SegmentBulkState::Framed(bulk) = &segment.bulk else {
        return None;
    };
    let RecordFrameState::Framed(records) = &bulk.records else {
        return None;
    };
    Some(&records.records)
}

fn external_link<'a>(
    source: &SegmentDescriptor<'_>,
    target: &SegmentDescriptor<'_>,
    reference: PmDcReference,
    reference_counts: &[u32],
    links: &'a [PmDcKernelEntityLink],
) -> Result<Option<&'a PmDcKernelEntityLink>, CodecError> {
    if reference.qualified() {
        return Ok(None);
    }
    let (SegmentMetaState::Parsed(meta), SegmentMetaState::Parsed(target_meta)) =
        (&source.meta, &target.meta)
    else {
        return Ok(None);
    };
    let sections = &meta.tables.sections;
    let Some(mut node) = reference
        .index()
        .checked_sub(1)
        .and_then(|index| entry(&sections[9], index, external::LEN))
    else {
        return Ok(None);
    };
    let revision_index = reader::u32(&mut node, "external revision index")?;
    let extension = reader::u16(&mut node, "external key extension")?;
    let header_id = reader::u16(&mut node, "external header identifier")?;
    if extension != 0 {
        return Ok(None);
    }
    let Some(mut revision_entry) = entry(&sections[7], revision_index, revision::LEN) else {
        return Ok(None);
    };
    reader::take(
        &mut revision_entry,
        revision::SEGMENT_PAIR_INDEX,
        "external revision identifier",
    )?;
    let pair_index = reader::u16(&mut revision_entry, "external segment pair index")?;
    let reference_count = reader::u16(&mut revision_entry, "external revision reference count")?;
    if usize::try_from(revision_index)
        .ok()
        .and_then(|index| reference_counts.get(index))
        .copied()
        != Some(u32::from(reference_count))
    {
        return Ok(None);
    }
    let Some(mut segment_pair) = entry(&sections[6], u32::from(pair_index), pair::LEN) else {
        return Ok(None);
    };
    let segment_id = reader::array::<16>(&mut segment_pair, "external segment identifier")?;
    if segment_id != target_meta.segment_id {
        return Ok(None);
    }
    Ok(one(links.iter().filter(|link| {
        link.identity.segment_token == *target.pair.token.key()
            && link.header_value == 0
            && link.header_id == header_id
    })))
}

pub(crate) fn surface_reference(property: &PmDcFeatureProperty) -> Option<PmDcReference> {
    match property.kind {
        PmDcFeaturePropertyKind::SurfaceBody { body } => Some(body),
        _ => None,
    }
}

pub(crate) fn current_body<'a>(
    wrapper: &PmDcFeatureProperty,
    link: &PmDcKernelEntityLink,
    carrier_token: &str,
    carrier_ordinal: u32,
    keys: &'a [BodyNativeKey],
) -> Option<&'a BodyNativeKey> {
    let reference = surface_reference(wrapper)?;
    if reference.index() == 0
        || reference.qualified()
        || link.header_value != 0
        || link.state != 0
        || link.selection_source != wrapper.header.source_index
        || link.identity.segment_token.as_str() != carrier_token
        || !link.body.qualified()
        || link.body.record_ordinal()? != carrier_ordinal
    {
        return None;
    }
    let source = format!("B{carrier_token}:record:{carrier_ordinal}");
    one(keys.iter().filter(|key| {
        key.source_brep.as_deref() == Some(source.as_str())
            && key.asm_body_key == Some(u64::from(link.entity_id))
    }))
}

pub(crate) fn unique_link_header(
    link: &PmDcKernelEntityLink,
    links: &[PmDcKernelEntityLink],
) -> bool {
    one(links.iter().filter(|candidate| {
        candidate.identity.segment_token == link.identity.segment_token
            && candidate.header_value == 0
            && candidate.header_id == link.header_id
    }))
    .is_some_and(|candidate| candidate.id() == link.id())
}

fn source_body(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
    inventory: &FeatureInventory,
    wrapper: &PmDcFeatureProperty,
) -> Result<bool, CodecError> {
    let Some(reference) = surface_reference(wrapper) else {
        return Ok(false);
    };
    if reference.index() == 0 {
        return Ok(!reference.qualified());
    }
    if !reference.qualified() {
        return Ok(false);
    }
    let Some(link) = one(inventory.kernel_entity_links.iter().filter(|link| {
        link.identity.segment_token == wrapper.identity.segment_token
            && Some(link.identity.record_ordinal) == reference.record_ordinal()
    })) else {
        return Ok(false);
    };
    if link.state != 0
        || link.selection_source != wrapper.header.source_index
        || !link.body.qualified()
    {
        return Ok(false);
    }
    let Some(segment) = one(document
        .segments
        .iter()
        .filter(|segment| *segment.pair.token.key() == wrapper.identity.segment_token))
    else {
        return Ok(false);
    };
    let Some(record) = framed_records(segment).and_then(|records| {
        one(records.iter().filter(|record| {
            Some(record.ordinal) == link.body.record_ordinal()
                && record.type_id == crate::kernel::KERNEL_RECORD_TYPE_ID
        }))
    }) else {
        return Ok(false);
    };
    let Some(version) = segment.registry.map(|registry| registry.version_major) else {
        return Ok(false);
    };
    let Ok(carrier) = crate::kernel::parse_carrier(
        record.payload,
        segment.pair.token.key(),
        record.ordinal,
        record.payload_offset,
        version,
    ) else {
        return Ok(false);
    };
    if PmDcReference::from_packed(carrier.selected_key) != link.body {
        return Ok(false);
    }
    let Ok(header) = carrier.header.as_ref() else {
        return Ok(false);
    };
    let decoded = match crate::kernel::decode_kernel_carrier(ctx, &carrier, header) {
        Ok(decoded) => decoded,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(false),
    };
    Ok(one(decoded
        .brep
        .body_native_keys
        .iter()
        .filter(|key| key.asm_body_key == Some(u64::from(link.entity_id))))
    .is_some())
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
    inventory: &FeatureInventory,
    design: &DesignInventory,
    keys: &[BodyNativeKey],
    model: &mut cadmpeg_ir::document::Model,
) -> Result<usize, CodecError> {
    let crate::kernel::ActiveCarrierState::Selected(carrier) = &document.active_carrier else {
        return Ok(0);
    };
    if PmDcReference::from_packed(carrier.selected_key)
        != PmDcReference::new(
            carrier
                .record_ordinal
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("Inventor carrier reference overflow"))?,
            true,
        )
        .ok_or_else(|| CodecError::malformed("Inventor carrier reference overflow"))?
    {
        return Ok(0);
    }
    let Some(target) = one(document.segments.iter().filter(|segment| {
        segment.kind == SegmentKind::PmBRep && segment.pair.token.key() == &carrier.segment_token
    })) else {
        return Ok(0);
    };
    let SegmentMetaState::Parsed(target_meta) = &target.meta else {
        return Ok(0);
    };
    if document
        .segments
        .iter()
        .filter(|segment| {
            matches!(&segment.meta, SegmentMetaState::Parsed(meta)
                if meta.segment_id == target_meta.segment_id)
        })
        .count()
        != 1
    {
        return Ok(0);
    }
    let properties = unique_index(inventory.properties.iter().map(|property| {
        (
            (
                property.identity.segment_token.as_str(),
                property.identity.record_ordinal,
            ),
            property,
        )
    }));
    let raw = unique_index(
        inventory
            .features
            .iter()
            .map(|feature| (feature.id(), feature)),
    );
    let labels = unique_index(inventory.labels.iter().map(|label| {
        (
            (
                label.identity.segment_token.as_str(),
                label.header.owner.record_ordinal(),
            ),
            label,
        )
    }));
    let mut transferred = 0;
    let mut sources = std::collections::HashMap::new();
    let mut external_counts = std::collections::HashMap::new();
    for feature in &mut model.features {
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            continue;
        }
        let Some(source) = feature
            .native_ref
            .as_deref()
            .and_then(|id| raw.get(id))
            .and_then(Option::as_ref)
            .copied()
        else {
            continue;
        };
        let token = source.identity.segment_token.as_str();
        let Some(label) = labels
            .get(&(token, Some(source.identity.record_ordinal)))
            .and_then(Option::as_ref)
        else {
            continue;
        };
        if !FeatureFamily::Base.class_ids().contains(&label.class_id()) {
            continue;
        }
        let Some(root) = one(design.model_roots.iter().filter(|root| {
            root.identity.segment_token.as_str() == token
                && crate::design::scope::root_role(root, &design.brep_segments)
                    == Some(crate::design::scope::BrepSegmentRole::Document)
        })) else {
            continue;
        };
        if root.brep_segment != target_meta.display_name {
            continue;
        }
        let Some((input, output)) = wrappers(source, |ordinal| {
            properties
                .get(&(token, ordinal))
                .and_then(Option::as_ref)
                .copied()
        }) else {
            continue;
        };
        let source_key = (token, input.identity.record_ordinal);
        let source_valid = match sources.get(&source_key) {
            Some(&valid) => valid,
            None => {
                let valid = source_body(ctx, document, inventory, input)?;
                sources.insert(source_key, valid);
                valid
            }
        };
        if !source_valid {
            continue;
        }
        let Some(segment) = one(document
            .segments
            .iter()
            .filter(|segment| segment.pair.token.key().as_str() == token))
        else {
            continue;
        };
        let counts = match external_counts.entry(token) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let counts = match &segment.meta {
                    SegmentMetaState::Parsed(meta) => {
                        reference_counts(ctx, &meta.tables.sections[7], &meta.tables.sections[9])?
                    }
                    SegmentMetaState::Malformed { .. } => None,
                };
                entry.insert(counts)
            }
        };
        let Some(counts) = counts else {
            continue;
        };
        let Some(link) = external_link(
            segment,
            target,
            surface_reference(output).ok_or_else(|| {
                CodecError::malformed("Inventor Base result is not a surface body")
            })?,
            counts,
            &inventory.kernel_entity_links,
        )?
        else {
            continue;
        };
        let Some(body) = current_body(
            output,
            link,
            carrier.segment_token.as_str(),
            carrier.record_ordinal,
            keys,
        ) else {
            continue;
        };
        if !model
            .bodies
            .iter()
            .any(|candidate| candidate.id == body.body)
        {
            continue;
        }
        let Some(body_id) = cadmpeg_core::text::NonBlankString::new(output.id()) else {
            continue;
        };
        let members = FeatureResultMembers::new(vec![body_id], Vec::new(), Vec::new(), Vec::new())
            .map_err(CodecError::malformed)?;
        ctx.charge_entities(1, "project Inventor Base result identity")?;
        model
            .feature_result_topologies
            .push(FeatureResultTopology::new(
                FeatureResultTopologyId::compose(
                    &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
                    source.identity.key()?,
                ),
                feature.id.clone(),
                members,
                Some(output.id()),
            ));
        feature.source_properties.insert(
            cadmpeg_core::nonblank_literal!("result_body_link"),
            link.id(),
        );
        feature.source_tag = Some("base".into());
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::StoredGeometry {},
            ));
        transferred += 1;
    }
    Ok(transferred)
}

pub(crate) fn wrappers<'a>(
    source: &PmDcFeature,
    mut property: impl FnMut(u32) -> Option<&'a PmDcFeatureProperty>,
) -> Option<(&'a PmDcFeatureProperty, &'a PmDcFeatureProperty)> {
    let slots = source.properties.references();
    if slots.len() != 3 || slots[2] != PmDcReference::from_packed(0) {
        return None;
    }
    let input = property(slots[0].record_ordinal()?)?;
    let output = property(slots[1].record_ordinal()?)?;
    if input.id() == output.id()
        || surface_reference(input).is_none()
        || surface_reference(output).is_none()
    {
        return None;
    }
    Some((input, output))
}

#[cfg(test)]
mod tests;
