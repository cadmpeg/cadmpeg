// SPDX-License-Identifier: Apache-2.0
//! Stored-body Base features and their current-model body joins.

use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_core::decode::{unique_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::evaluation::{FeatureResultMembers, FeatureResultTopology};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::ids::FeatureResultTopologyId;
use cadmpeg_ir::report::loss::LossNote;

use super::history_views::{ResultStates, StateLimit, StateModel};
use super::{
    FeatureFamily, FeatureInventory, PmDcFeature, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcKernelEntityLink,
};
use crate::design::DesignInventory;
use crate::kernel::ActiveCarrier;
use crate::layout::meta_external_revision as revision;
use crate::loss::InventorLossCode;
use crate::pmdc::PmDcReference;
use crate::reader;
use crate::records::ReferenceTable;
use crate::rse::{
    RecordFrameState, RseInventory, SegmentBulkState, SegmentDescriptor, SegmentKind,
    SegmentMetaState,
};

pub(crate) const RESULT_LINK_PROPERTY: &str = "result_body_link";

fn one<T>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

pub(super) fn reference_counts(
    ctx: &DecodeContext<'_>,
    revisions: &ReferenceTable<'_>,
    references: &ReferenceTable<'_>,
) -> Result<Option<Vec<u32>>, CodecError> {
    let Some(count) = revisions.count else {
        return Ok(None);
    };
    if references.count.is_none() {
        return Ok(None);
    }
    let mut counts = ctx.alloc_filled(count, 0_u32, "count Inventor external references")?;
    for index in 0..references.declared_count {
        let Some(mut node) = references.entry(index) else {
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
            .and_then(|index| revisions.entry(index))
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

pub(super) fn external_link<'a>(
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
    let [pairs, revisions, external] = &meta.tables.references;
    let Some(mut node) = reference
        .index()
        .checked_sub(1)
        .and_then(|index| external.entry(index))
    else {
        return Ok(None);
    };
    let revision_index = reader::u32(&mut node, "external revision index")?;
    let extension = reader::u16(&mut node, "external key extension")?;
    let header_id = reader::u16(&mut node, "external header identifier")?;
    if extension != 0 {
        return Ok(None);
    }
    let Some(mut revision_entry) = revisions.entry(revision_index) else {
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
    let Some(mut segment_pair) = pairs.entry(u32::from(pair_index)) else {
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

/// Whether `link` selects the active carrier record from the Base result
/// wrapper `wrapper`. The body key then names a body of that carrier.
fn carrier_link(
    wrapper: &PmDcFeatureProperty,
    link: &PmDcKernelEntityLink,
    carrier_token: &str,
    carrier_ordinal: u32,
) -> bool {
    let Some(reference) = surface_reference(wrapper) else {
        return false;
    };
    reference.index() != 0
        && !reference.qualified()
        && link.header_value == 0
        && link.state == 0
        && link.selection_source == wrapper.header.source_index
        && link.identity.segment_token.as_str() == carrier_token
        && link.body.qualified()
        && link.body.record_ordinal() == Some(carrier_ordinal)
}

/// Bodies of the final model that carry the key of `link`.
fn keyed_bodies<'a>(
    link: &PmDcKernelEntityLink,
    carrier_token: &str,
    carrier_ordinal: u32,
    keys: &'a [BodyNativeKey],
) -> impl Iterator<Item = &'a BodyNativeKey> {
    let source = format!("B{carrier_token}:record:{carrier_ordinal}");
    let entity = u64::from(link.entity_id);
    keys.iter().filter(move |key| {
        key.source_brep.as_deref() == Some(source.as_str()) && key.asm_body_key == Some(entity)
    })
}

/// Body key of the result that `link` selects.
pub(crate) fn result_key(link: &PmDcKernelEntityLink) -> i64 {
    i64::from(link.entity_id)
}

pub(crate) fn current_body<'a>(
    wrapper: &PmDcFeatureProperty,
    link: &PmDcKernelEntityLink,
    carrier_token: &str,
    carrier_ordinal: u32,
    keys: &'a [BodyNativeKey],
) -> Option<&'a BodyNativeKey> {
    if !carrier_link(wrapper, link, carrier_token, carrier_ordinal) {
        return None;
    }
    one(keyed_bodies(link, carrier_token, carrier_ordinal, keys))
}

/// Whether the result of a Base feature can only be a body of a history
/// state: the link selects the active carrier record and no body of the final
/// model has its key.
pub(crate) fn history_candidate(
    wrapper: &PmDcFeatureProperty,
    link: &PmDcKernelEntityLink,
    carrier_token: &str,
    carrier_ordinal: u32,
    keys: &[BodyNativeKey],
) -> bool {
    carrier_link(wrapper, link, carrier_token, carrier_ordinal)
        && keyed_bodies(link, carrier_token, carrier_ordinal, keys)
            .next()
            .is_none()
}

/// What the history state of a Base feature shows of its result body.
enum StateBody {
    /// The state holds the result body, and the state before it does not.
    Held,
    /// No state introduces the key.
    Absent,
    /// The state does not decode, or does not hold a body with the key.
    Failed,
    /// A limit gave up the state of the result body.
    Bounded(StateLimit),
}

/// Read the state that first lists `source_index` for the result key of
/// `link` (spec section 13.4).
fn state_body(
    ctx: &DecodeContext<'_>,
    states: Option<&dyn ResultStates>,
    source_index: u32,
    link: &PmDcKernelEntityLink,
) -> Result<StateBody, CodecError> {
    let Some(states) = states else {
        return Ok(StateBody::Absent);
    };
    if !states.introduces(source_index, result_key(link)) {
        return Ok(StateBody::Absent);
    }
    let state = states.after(ctx, source_index)?;
    let model = match &*state {
        StateModel::Model(model) => model,
        StateModel::Unavailable | StateModel::Failed => return Ok(StateBody::Failed),
        StateModel::Bounded(limit) => return Ok(StateBody::Bounded(*limit)),
    };
    let key = u64::from(link.entity_id);
    let held = one(model
        .body_keys
        .iter()
        .filter(|candidate| candidate.asm_body_key == Some(key)))
    .is_some_and(|candidate| {
        model
            .geometry
            .bodies
            .iter()
            .any(|body| body.id == candidate.body)
    });
    Ok(if held {
        StateBody::Held
    } else {
        StateBody::Failed
    })
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

#[derive(Clone, Copy)]
enum SourceBody {
    Absent,
    Present,
    Bounded(cadmpeg_core::decode::ResourceLimit),
}

fn source_body(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
    inventory: &FeatureInventory,
    wrapper: &PmDcFeatureProperty,
) -> Result<SourceBody, CodecError> {
    let Some(reference) = surface_reference(wrapper) else {
        return Ok(SourceBody::Absent);
    };
    if reference.index() == 0 {
        return Ok(if reference.qualified() {
            SourceBody::Absent
        } else {
            SourceBody::Present
        });
    }
    if !reference.qualified() {
        return Ok(SourceBody::Absent);
    }
    let Some(link) = one(inventory.kernel_entity_links.iter().filter(|link| {
        link.identity.segment_token == wrapper.identity.segment_token
            && Some(link.identity.record_ordinal) == reference.record_ordinal()
    })) else {
        return Ok(SourceBody::Absent);
    };
    if link.state != 0
        || link.selection_source != wrapper.header.source_index
        || !link.body.qualified()
    {
        return Ok(SourceBody::Absent);
    }
    let Some(segment) = one(document
        .segments
        .iter()
        .filter(|segment| *segment.pair.token.key() == wrapper.identity.segment_token))
    else {
        return Ok(SourceBody::Absent);
    };
    let Some(record) = framed_records(segment).and_then(|records| {
        one(records.iter().filter(|record| {
            Some(record.ordinal) == link.body.record_ordinal()
                && record.type_id == crate::kernel::KERNEL_RECORD_TYPE_ID
        }))
    }) else {
        return Ok(SourceBody::Absent);
    };
    let Some(version) = segment.registry.map(|registry| registry.version_major) else {
        return Ok(SourceBody::Absent);
    };
    let Ok(carrier) = crate::kernel::parse_carrier(
        record.payload,
        segment.pair.token.key(),
        record.ordinal,
        record.payload_offset,
        version,
    ) else {
        return Ok(SourceBody::Absent);
    };
    if PmDcReference::from_packed(carrier.selected_key) != link.body {
        return Ok(SourceBody::Absent);
    }
    inspect_source_body(ctx, &carrier, link.entity_id)
}

fn inspect_source_body(
    ctx: &DecodeContext<'_>,
    carrier: &ActiveCarrier<'_>,
    entity_id: u32,
) -> Result<SourceBody, CodecError> {
    let Ok(header) = carrier.header.as_ref() else {
        return Ok(SourceBody::Absent);
    };
    let result = ctx.scratch(carrier.bytes.window().len(), |scratch| {
        match crate::kernel::inspect_kernel_carrier(scratch, carrier, header) {
            Ok(brep) => Ok(
                if one(brep
                    .body_native_keys
                    .iter()
                    .filter(|key| key.asm_body_key == Some(u64::from(entity_id))))
                .is_some()
                {
                    SourceBody::Present
                } else {
                    SourceBody::Absent
                },
            ),
            Err(error @ CodecError::ResourceLimit(_)) => Err(error),
            Err(_) => Ok(SourceBody::Absent),
        }
    })?;
    Ok(result.unwrap_or_else(SourceBody::Bounded))
}

#[derive(Default)]
pub(crate) struct Projection {
    pub(crate) transferred: usize,
    pub(crate) losses: Vec<LossNote>,
}

/// Records that the Base projection reads.
#[derive(Clone, Copy)]
pub(crate) struct Inputs<'a> {
    pub(crate) document: &'a RseInventory<'a>,
    pub(crate) inventory: &'a FeatureInventory,
    pub(crate) design: &'a DesignInventory,
    pub(crate) transforms: &'a [crate::sketch::PmDcTransform],
    /// Body keys of the final model.
    pub(crate) keys: &'a [BodyNativeKey],
    /// Views of history states, for a result that the final model no longer
    /// holds.
    pub(crate) states: Option<&'a dyn ResultStates>,
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    inputs: &Inputs<'_>,
    ir: &mut cadmpeg_ir::CadIr,
) -> Result<Projection, CodecError> {
    let Inputs {
        document,
        inventory,
        design,
        transforms,
        keys,
        states,
    } = *inputs;
    let crate::kernel::ActiveCarrierState::Selected(carrier) = &document.active_carrier else {
        return Ok(Projection::default());
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
        return Ok(Projection::default());
    }
    let Some(target) = one(document.segments.iter().filter(|segment| {
        segment.kind == SegmentKind::PmBRep && segment.pair.token.key() == &carrier.segment_token
    })) else {
        return Ok(Projection::default());
    };
    let SegmentMetaState::Parsed(target_meta) = &target.meta else {
        return Ok(Projection::default());
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
        return Ok(Projection::default());
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
    let mut projection = Projection::default();
    let mut sources = std::collections::HashMap::new();
    let mut external_counts = std::collections::HashMap::new();
    for feature in &mut ir.model.features {
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
        let Some((input, output)) = wrappers(source, label.class_id(), |ordinal| {
            properties
                .get(&(token, ordinal))
                .and_then(Option::as_ref)
                .copied()
        }) else {
            continue;
        };
        let derived = if label.class_id() == super::derived::CLASS_ID {
            if !label.participants.references().is_empty() {
                continue;
            }
            let Some(segment) = one(document
                .segments
                .iter()
                .filter(|segment| segment.pair.token.key().as_str() == token))
            else {
                continue;
            };
            let Some(records) = framed_records(segment) else {
                continue;
            };
            let Some(operands) = super::derived::operands(
                source,
                |ordinal| {
                    properties
                        .get(&(token, ordinal))
                        .and_then(Option::as_ref)
                        .copied()
                },
                |ordinal| {
                    one(records.iter().filter(|record| record.ordinal == ordinal)).map(|record| {
                        (
                            crate::pmdc::type_id_string(record.type_id),
                            cadmpeg_core::decode::u64_from_index(record.payload.remaining()),
                        )
                    })
                },
                transforms,
            ) else {
                continue;
            };
            Some(operands)
        } else {
            None
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
        match source_valid {
            SourceBody::Present => {}
            SourceBody::Absent => continue,
            SourceBody::Bounded(limit) => {
                projection
                    .losses
                    .push(InventorLossCode::FeatureBaseSourceBounded.note(format!(
                        "{} source body check: {} during {}",
                        feature.id,
                        CodecError::ResourceLimit(limit),
                        limit.operation
                    )));
                continue;
            }
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
                    SegmentMetaState::Parsed(meta) => reference_counts(
                        ctx,
                        &meta.tables.references[1],
                        &meta.tables.references[2],
                    )?,
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
        match current_body(
            output,
            link,
            carrier.segment_token.as_str(),
            carrier.record_ordinal,
            keys,
        ) {
            Some(body) => {
                if !ir
                    .geometry
                    .bodies
                    .iter()
                    .any(|candidate| candidate.id == body.body)
                {
                    continue;
                }
            }
            None => {
                let unresolved = |projection: &mut Projection| {
                    if derived.is_some() {
                        projection.losses.push(crate::loss::InventorLossCode::FeatureDerivedResultNative.note(
                            "Derived result-body operand (slot 1) has an unresolved current-model body join; the feature remains native.",
                        ));
                    }
                };
                if !history_candidate(
                    output,
                    link,
                    carrier.segment_token.as_str(),
                    carrier.record_ordinal,
                    keys,
                ) {
                    unresolved(&mut projection);
                    continue;
                }
                let (code, noun) = if derived.is_some() {
                    (
                        crate::loss::InventorLossCode::FeatureDerivedResultNative,
                        "Derived",
                    )
                } else {
                    (
                        crate::loss::InventorLossCode::FeatureBaseResultStateNative,
                        "Base",
                    )
                };
                match state_body(ctx, states, source.header.source_index, link)? {
                    StateBody::Held => {}
                    StateBody::Absent => {
                        unresolved(&mut projection);
                        continue;
                    }
                    StateBody::Failed => {
                        projection.losses.push(code.note(format!(
                            "{noun} result body is absent from the current model and its history state does not decode to a body with the result key; the feature remains native.",
                        )));
                        continue;
                    }
                    StateBody::Bounded(limit) => {
                        projection.losses.push(code.note(format!(
                            "{noun} {} history-state result: {limit}; the feature remains native.",
                            feature.id,
                        )));
                        continue;
                    }
                }
            }
        }
        let Some(body_id) = cadmpeg_core::text::NonBlankString::new(output.id()) else {
            continue;
        };
        let members = FeatureResultMembers::new(vec![body_id], Vec::new(), Vec::new(), Vec::new())
            .map_err(CodecError::malformed)?;
        ctx.charge_entities(1, "project Inventor Base result identity")?;
        ir.model
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
        if let Some(derived) = derived {
            feature.source_properties.insert(
                cadmpeg_core::nonblank_literal!("derived_source"),
                derived.source.id(),
            );
            feature.source_properties.insert(
                cadmpeg_core::nonblank_literal!("derived_transform"),
                derived.transform.id(),
            );
        }
        feature.source_tag = Some(if label.class_id() == super::derived::CLASS_ID {
            "derived_part".into()
        } else {
            "base".into()
        });
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::StoredGeometry {},
            ));
        projection.transferred += 1;
    }
    Ok(projection)
}

pub(crate) fn wrappers<'a>(
    source: &PmDcFeature,
    class: super::ClassId,
    mut property: impl FnMut(u32) -> Option<&'a PmDcFeatureProperty>,
) -> Option<(&'a PmDcFeatureProperty, &'a PmDcFeatureProperty)> {
    let slots = source.properties.references();
    if slots.len() != 3 {
        return None;
    }
    if class == super::BASE_CLASS_ID {
        if slots[2] != PmDcReference::from_packed(0) {
            return None;
        }
    } else if class == super::derived::CLASS_ID {
        if slots
            .iter()
            .any(|reference| !reference.qualified() || reference.index() == 0)
        {
            return None;
        }
    } else {
        return None;
    }
    let input = property(slots[0].record_ordinal()?)?;
    let output = property(slots[1].record_ordinal()?)?;
    if class == super::derived::CLASS_ID
        && (input.save_version_major != source.save_version_major
            || output.save_version_major != source.save_version_major
            || surface_reference(input).is_none_or(|body| !body.qualified() || body.index() == 0))
    {
        return None;
    }
    if input.id() == output.id()
        || input.identity.type_id.as_str() != crate::pmdc::type_id_string(super::SURFACE_BODY_TYPE)
        || output.identity.type_id.as_str() != crate::pmdc::type_id_string(super::SURFACE_BODY_TYPE)
        || surface_reference(input).is_none()
        || surface_reference(output).is_none()
    {
        return None;
    }
    Some((input, output))
}

#[cfg(test)]
mod tests;
