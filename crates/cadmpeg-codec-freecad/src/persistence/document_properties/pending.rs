// SPDX-License-Identifier: Apache-2.0
//! Property headers grant authority to schedule genuine value producers later.

use super::{value_parts, RootProperties};
use crate::native::PropertyRecord;
use crate::xml_framing::XmlSource;
use cadmpeg_core::decode::{DecodeContext, RecordOwner};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput, PartParent};
use std::collections::BTreeMap;

pub(crate) struct PendingProperty<'source> {
    pub(crate) index: usize,
    pub(crate) property: PropertyRecord,
    pub(crate) record: RecordOwner<'source>,
    pub(crate) parent: PartParent,
    roots: BTreeMap<usize, value_parts::ParsedRoot>,
}

impl<'source> PendingProperty<'source> {
    pub(crate) fn new(
        index: usize,
        property: PropertyRecord,
        record: RecordOwner<'source>,
        parent: PartParent,
    ) -> Self {
        Self {
            index,
            property,
            record,
            parent,
            roots: BTreeMap::new(),
        }
    }
}

pub(crate) fn finish_pending<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    context: &crate::element_map::adopted::Context<'_, 'source>,
    output: &mut PartOutput,
) -> Result<BTreeMap<String, RootProperties>, CodecError> {
    let mut pending = std::mem::take(&mut *context.pending_properties.borrow_mut());
    let mut values = Vec::new();
    for (property_index, property) in pending
        .iter()
        .enumerate()
        .filter(|(_, property)| !property.property.is_transient())
    {
        for child in source.children(property.index) {
            let hasher = source
                .name(child)
                .is_ok_and(|name| matches!(name, "StringHasher" | "StringHasher2"));
            values.push((!hasher, child, property_index));
        }
    }
    // Only source locators are scheduled here. Every field is read once inside
    // its actual helper, and no consumer transaction surrounds a donor producer.
    values.sort_by_key(|(consumer, child, _)| (*consumer, *child));
    for (_, child, property_index) in values {
        let pending = &mut pending[property_index];
        let previous: Vec<_> = pending
            .roots
            .values()
            .flat_map(|root| root.0.iter().cloned())
            .collect();
        let payload_checkpoint = context.pending_checkpoint();
        let parsed = ctx.record_fragment(&pending.record, source.extent(child)?, || {
            ctx.decode_part(
                output,
                source.extent(child)?,
                source
                    .spec(child, "freecad.property_value")?
                    .with_parent(&pending.parent),
                |stage| {
                    value_parts::decode_value(
                        ctx,
                        source,
                        pending.index,
                        child,
                        &pending.property,
                        !pending.roots.is_empty(),
                        &previous,
                        context,
                        stage,
                    )
                },
            )
        })?;
        if let Some(parsed) = parsed {
            pending.roots.insert(child, parsed);
        } else {
            context.rollback_pending(payload_checkpoint);
        }
        if pending.property.type_name == "Part::PropertyPartShape"
            && !pending.roots.contains_key(&child)
            && source.name(child)? == "Part"
        {
            let mut dependents = Vec::new();
            if source.children(pending.index).any(|index| {
                source
                    .name(index)
                    .is_ok_and(|name| matches!(name, "ElementMap" | "ElementMap2"))
            }) {
                dependents.push(crate::native::native_child_id(
                    "element-map",
                    &pending.property.id,
                    "map",
                ));
            }
            let filename = match source.peek_attribute(ctx, child, "file") {
                Ok(value) => value,
                Err(CodecError::Malformed(_)) => None,
                Err(error) => return Err(error),
            };
            if let Some(filename) = filename.filter(|name| {
                !name.is_empty()
                    && context.has_declared_entry(name)
                    && context.entries.contains_key(name)
            }) {
                dependents.push(crate::native::native_child_id(
                    "shape-payload",
                    &pending.property.id,
                    &filename,
                ));
            }
            if !dependents.is_empty() {
                let extent = ctx.capture_part_extent(source.extent(child)?)?;
                let home = output
                    .fidelity()
                    .parts()
                    .iter()
                    .find(|(_, part)| {
                        part.level().as_str() == "freecad.property_value"
                            && part.stream() == extent.stream()
                            && part.offset() == extent.offset()
                            && part.byte_len() == extent.byte_len()
                            && part.state()
                                == &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                    })
                    .map(|(id, _)| id.as_str().to_owned())
                    .ok_or_else(|| {
                        CodecError::malformed("failed Part has no actual given-up value home")
                    })?;
                output.drop_with_given_up(ctx, &home, dependents)?;
            }
        }
    }
    let mut owners = BTreeMap::<String, RootProperties>::new();
    for pending in pending {
        let extent = source.extent(pending.index)?;
        let (property, ranges) = ctx.record_fragment(&pending.record, extent, || {
            value_parts::finish_property(
                ctx,
                source,
                pending.index,
                pending.property,
                pending.roots,
                context,
                output,
            )
        })?;
        sponsor_missing_attribute(ctx, source, pending.index, &property, output)?;
        let offset = source.view().start();
        let ranges = ranges
            .into_iter()
            .zip(property.values())
            .map(|(range, value)| (value.order, range.start - offset..range.end - offset))
            .collect();
        let source_ranges = crate::fidelity::ValueSourceRanges::new_slots(
            property.id.clone(),
            extent.start() - offset..extent.end() - offset,
            ranges,
        )?;
        let result = owners.entry(property.owner.clone()).or_default();
        result.sources.insert(property.id.clone(), source_ranges);
        result.properties.push(property);
    }
    Ok(owners)
}

fn sponsor_missing_attribute<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    index: usize,
    property: &PropertyRecord,
    output: &mut PartOutput,
) -> Result<(), CodecError> {
    if ctx.container_only() {
        return Ok(());
    }
    if property.owner != crate::native::native_id("document", "0") {
        return Ok(());
    }
    let Some(attribute) =
        crate::attributes::source_property_identity(ctx, source, index, property)?
    else {
        return Ok(());
    };
    // A readable scalar can still project when an unrelated extra child fails.
    // Only an actually absent projection needs a dependent identity sponsor.
    if crate::attributes::values(property).is_some() {
        return Ok(());
    }
    let extent = ctx.capture_part_extent(source.extent(index)?)?;
    let homes: Vec<_> = output
        .fidelity()
        .parts()
        .iter()
        .filter(|(_, part)| {
            matches!(
                part.level().as_str(),
                "freecad.property_value" | "freecad.value_element"
            ) && part.state() == &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                && part.stream() == extent.stream()
                && part.offset() >= extent.offset()
                && part.offset() + part.byte_len() <= extent.offset() + extent.byte_len()
        })
        .map(|(id, _)| id.as_str().to_owned())
        .collect();
    for home in homes {
        output.drop_with_given_up(ctx, &home, [attribute.clone()])?;
    }
    Ok(())
}
