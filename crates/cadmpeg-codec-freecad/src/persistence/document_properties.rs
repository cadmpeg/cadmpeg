// SPDX-License-Identifier: Apache-2.0
//! Property records and values from recovering source element boundaries.

use crate::native::PropertyRecord;
use crate::xml_framing::XmlSource;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};
use std::collections::BTreeMap;

pub(crate) mod pending;
pub(crate) mod value_parts;
pub(crate) use pending::finish_pending;

#[derive(Default)]
pub(crate) struct RootProperties {
    pub(crate) properties: Vec<PropertyRecord>,
    pub(crate) sources: BTreeMap<String, crate::fidelity::ValueSourceRanges>,
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

pub(crate) fn prepare_scanned<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    root: usize,
    context: &crate::element_map::adopted::Context<'_, 'source>,
    output: &mut PartOutput,
) -> Result<Option<RootProperties>, CodecError> {
    let containers: Vec<_> = source
        .children(root)
        .filter(|index| source.name(*index).is_ok_and(|name| name == "Properties"))
        .collect();
    if containers.is_empty() {
        return Ok(None);
    }
    if containers.len() != 1 {
        for index in containers {
            ctx.decode_part(
                output,
                source.extent(index)?,
                source.spec(index, "freecad.properties")?,
                |_stage| {
                    Err::<(), _>(CodecError::malformed(
                        "Document.xml has multiple root Properties containers",
                    ))
                },
            )?;
        }
        return Ok(Some(RootProperties {
            ..RootProperties::default()
        }));
    }
    prepare_owned_scanned(
        ctx,
        source,
        containers[0],
        &crate::native::native_id("document", "0"),
        context,
        output,
    )
    .map(Some)
}

pub(crate) fn prepare_owned_scanned<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    index: usize,
    owner: &str,
    context: &crate::element_map::adopted::Context<'_, 'source>,
    output: &mut PartOutput,
) -> Result<RootProperties, CodecError> {
    let result = ctx.decode_part(
        output,
        source.extent(index)?,
        source.spec(index, "freecad.properties")?,
        |stage| {
            source.validate(index)?;
            let children: Vec<_> = source
                .children(index)
                .filter(|index| {
                    source
                        .name(*index)
                        .is_ok_and(|name| matches!(name, "Property" | "_Property"))
                })
                .collect();
            let mut names = BTreeMap::<String, usize>::new();
            for &child in &children {
                match source.peek_attribute(ctx, child, "name") {
                    Ok(Some(name)) => {
                        *names.entry(name).or_default() += 1;
                    }
                    Ok(None) => {}
                    Err(error @ CodecError::Io(_)) => return Err(error),
                    Err(CodecError::ResourceLimit(limit))
                        if limit.scope == cadmpeg_core::decode::LimitScope::Session =>
                    {
                        return Err(CodecError::ResourceLimit(limit));
                    }
                    Err(_) => {}
                }
            }
            let mut result = RootProperties::default();
            let mut pending = Vec::new();
            let transient = children
                .iter()
                .filter(|index| source.name(**index).is_ok_and(|name| name == "_Property"))
                .count();
            let attributes = source.peek_attributes(ctx, index)?;
            for (field, actual) in [
                ("Count", children.len() - transient),
                ("TransientCount", transient),
            ] {
                if field == "TransientCount" && transient == 0 && !attributes.contains_key(field) {
                    continue;
                }
                if attributes
                    .get(field)
                    .and_then(|text| text.parse::<usize>().ok())
                    != Some(actual)
                {
                    let loss =
                        crate::loss::FreecadLossCode::PersistenceCountNoncanonical.note(format!(
                            "Properties {field}={:?}; reading {actual} framed elements",
                            attributes.get(field)
                        ));
                    result.losses.push(loss);
                }
            }
            let mut persisted_order = 0;
            let mut transient_order = 0;
            for child in children {
                let transient = source.name(child)? == "_Property";
                let order = if transient {
                    let order = transient_order;
                    transient_order += 1;
                    order
                } else {
                    let order = persisted_order;
                    persisted_order += 1;
                    order
                };
                let parsed = ctx.decode_part(
                    stage,
                    source.extent(child)?,
                    source.spec(child, "freecad.property")?,
                    |_property_stage| {
                        let name = source
                            .peek_attribute(ctx, child, "name")?
                            .ok_or_else(|| CodecError::malformed("Property has no name"))?;
                        let id = crate::native::native_child_id("property", owner, &name);
                        ctx.record_owner(&id, source.extent(child)?, || {
                            source.validate(child)?;
                            if names.get(&name).is_some_and(|count| *count > 1) {
                                let kind = if owner == crate::native::native_id("document", "0") {
                                    "document"
                                } else {
                                    "object"
                                };
                                return Err(CodecError::malformed(format_args!(
                                    "duplicate property name {name} for {kind} {owner}"
                                )));
                            }
                            value_parts::prepare_header(ctx, source, child, owner, order)
                        })
                    },
                )?;
                if let Some((property, record_owner)) = parsed {
                    let extent = source.extent(child)?;
                    let offset = source.view().start();
                    result.sources.insert(
                        property.id.clone(),
                        crate::fidelity::ValueSourceRanges::new_slots(
                            property.id.clone(),
                            extent.start() - offset..extent.end() - offset,
                            Vec::new(),
                        )?,
                    );
                    let parent = stage.parent(ctx, extent, "freecad.property")?;
                    pending.push(pending::PendingProperty::new(
                        child,
                        property.clone(),
                        record_owner,
                        parent,
                    ));
                    result.properties.push(property);
                }
            }
            Ok((result, pending))
        },
    )?;
    match result {
        Some((result, pending)) => {
            context.pending_properties.borrow_mut().extend(pending);
            Ok(result)
        }
        None => Ok(RootProperties::default()),
    }
}

/// Completes a standalone root property collection through the global producer scheduler.
#[cfg(test)]
pub(crate) fn decode_scanned<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    root: usize,
    context: &crate::element_map::adopted::Context<'_, 'source>,
    output: &mut PartOutput,
) -> Result<Option<RootProperties>, CodecError> {
    let Some(mut result) = prepare_scanned(ctx, source, root, context, output)? else {
        return Ok(None);
    };
    let mut finished = finish_pending(ctx, source, context, output)?;
    if let Some(complete) = finished.remove(&crate::native::native_id("document", "0")) {
        result.properties = complete.properties;
        result.sources = complete.sources;
    }
    Ok(Some(result))
}

/// Completes one owner when no other document owner remains to be prepared.
#[cfg(test)]
pub(crate) fn decode_owned_scanned<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    index: usize,
    owner: &str,
    context: &crate::element_map::adopted::Context<'_, 'source>,
    output: &mut PartOutput,
) -> Result<RootProperties, CodecError> {
    let mut result = prepare_owned_scanned(ctx, source, index, owner, context, output)?;
    let mut finished = finish_pending(ctx, source, context, output)?;
    if let Some(complete) = finished.remove(owner) {
        result.properties = complete.properties;
        result.sources = complete.sources;
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
