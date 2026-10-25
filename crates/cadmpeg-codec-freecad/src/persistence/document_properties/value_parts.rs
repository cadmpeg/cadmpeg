// SPDX-License-Identifier: Apache-2.0
//! Value fields are constructed inside their own element transactions.

use crate::native::{PropertyBody, PropertyFamily, PropertyRecord, ValueRecord, ValueSlots};
use crate::xml_framing::XmlSource;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};
use std::collections::BTreeMap;
use std::ops::Range;

pub(crate) mod links;
pub(crate) mod spreadsheet;

type ParsedValues = (Vec<ValueRecord>, Vec<Range<usize>>, usize);

pub(crate) type ParsedRoot = (
    Vec<ValueRecord>,
    Vec<Range<usize>>,
    Vec<Option<crate::native::LinkTarget>>,
    usize,
);

pub(crate) fn prepare_header(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'_>,
    index: usize,
    owner: &str,
    order: usize,
) -> Result<PropertyRecord, CodecError> {
    let name = required(ctx, source, index, "name")?;
    let type_name = required(ctx, source, index, "type")?;
    let peek = source.peek_attributes(ctx, index)?;
    let status = if peek
        .get("status")
        .is_some_and(|text| text.parse::<u64>().is_ok())
    {
        source
            .attribute(ctx, index, "status")?
            .and_then(|text| text.parse().ok())
    } else {
        None
    };
    let transient = source.name(index)? == "_Property";
    let dynamic = if transient {
        None
    } else {
        crate::persistence::source::dynamic(ctx, source, index, &peek)?
    };
    let property = PropertyRecord {
        id: crate::native::native_child_id("property", owner, &name),
        owner: owner.to_owned(),
        family: crate::persistence::property_family(&type_name),
        name,
        type_name,
        status,
        dynamic,
        body: transient.then_some(PropertyBody::Transient),
        order,
    };
    if !transient
        && property.family != PropertyFamily::Unknown
        && !source.peek_text(ctx, index)?.trim().is_empty()
    {
        return Err(CodecError::malformed(
            "registered property has character content outside its value elements",
        ));
    }
    let cardinality_one = cardinality_one(&property);
    let direct_count = source.children(index).count();
    if !transient && cardinality_one && direct_count == 0 {
        return Err(CodecError::malformed(format_args!(
            "{} requires one value element",
            property.type_name
        )));
    }
    Ok(property)
}

fn cardinality_one(property: &PropertyRecord) -> bool {
    crate::gui::value_grammar::requires_single_root(&property.type_name)
        || property.type_name != "App::PropertyEnumeration"
            && crate::persistence::scalar_value_tag(&property.type_name).is_some()
        || crate::persistence::link_grammar(&property.type_name).is_some()
}

pub(crate) fn decode_value<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    property_index: usize,
    child: usize,
    property: &PropertyRecord,
    root_held: bool,
    previous: &[ValueRecord],
    context: &crate::element_map::adopted::Context<'_, 'source>,
    value_stage: &mut PartOutput,
) -> Result<ParsedRoot, CodecError> {
    if property.family == PropertyFamily::Unknown {
        return Ok((Vec::new(), Vec::new(), Vec::new(), 0));
    }
    source.validate(child)?;
    if cardinality_one(property) && root_held {
        return Err(CodecError::malformed(format_args!(
            "{} requires exactly one value element",
            property.type_name
        )));
    }
    let header = crate::persistence::source::peek_value_attributes(ctx, source, child)?;
    let root_tags = source
        .children(property_index)
        .map(|index| source.name(index))
        .collect::<Result<Vec<_>, _>>()?;
    crate::gui::value_grammar::validate_value_roots(
        &property.type_name,
        source.name(child)?,
        &header,
        &root_tags,
    )?;
    let recognized = crate::element_map::adopted::decode(
        ctx,
        source,
        child,
        Some(property),
        Some(previous),
        context,
        value_stage,
    )?;
    if !recognized {
        validate_scalar(&property.type_name, source.name(child)?, &header)?;
        crate::gui::value_grammar::validate_root(
            &property.type_name,
            source.name(child)?,
            &header,
            source.children(child).count(),
        )?;
    }
    let (children, child_ranges, child_slots) = read_element(
        ctx,
        source,
        child,
        property,
        value_stage,
        context,
        !recognized,
    )?;
    let opaque_python = property.family == PropertyFamily::PythonObject
        && python_json(source.name(child)?, &header).is_none();
    if !recognized && !opaque_python {
        if let Some(value) = children.first() {
            validate_scalar(&property.type_name, source.name(child)?, &value.attributes)?;
            crate::gui::value_grammar::validate_root(
                &property.type_name,
                source.name(child)?,
                &value.attributes,
                source.children(child).count(),
            )?;
        }
    }
    if let Some(value) = children.first() {
        crate::gui::value_grammar::validate_value_roots(
            &property.type_name,
            source.name(child)?,
            &value.attributes,
            &root_tags,
        )?;
        crate::gui::value_grammar::validate_previous(
            &property.type_name,
            source.name(child)?,
            None,
            &value.attributes,
            previous,
        )?;
    }
    let targets =
        if children.is_empty() && crate::persistence::link_grammar(&property.type_name).is_some() {
            vec![None]
        } else {
            links::from_values(&property.type_name, &children, source, child, &child_ranges)?
        };
    for entry in side_entry_candidates(property, &children)
        .into_iter()
        .filter(|entry| !context.has_declared_entry(entry))
    {
        if !recognized {
            value_stage.push_loss(ctx, crate::loss::FreecadLossCode::PersistenceSideEntryUnresolved.note(format!(
            "property {} references undeclared side entry {entry}; filename remains in the readable value", property.id
        )))?;
        }
    }
    let mut complete = property.clone();
    complete.body = Some(PropertyBody::Persisted {
        side_entries: side_entries(ctx, property, &children, context)?,
        values: ValueSlots::new(children.clone(), child_slots).map_err(CodecError::malformed)?,
        links: targets.clone(),
    });
    let payloads = crate::gui::payloads::decode(
        ctx,
        (&complete).into(),
        crate::gui::payloads::SourceRequest {
            xml: source,
            value: child,
            entries: context.entries,
            parents: context.parents,
            owners: context.entry_owners,
            base: None,
        },
        &mut context.entry_interpretations.borrow_mut(),
        value_stage,
    )?;
    context
        .entry_contents
        .borrow_mut()
        .extend(payloads.entry_contents);
    context
        .payload_sources
        .borrow_mut()
        .append(payloads.sources)?;
    Ok((children, child_ranges, targets, child_slots))
}

pub(crate) fn finish_property<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    index: usize,
    mut property: PropertyRecord,
    mut roots: BTreeMap<usize, ParsedRoot>,
    context: &crate::element_map::adopted::Context<'_, 'source>,
    stage: &mut PartOutput,
) -> Result<(PropertyRecord, Vec<Range<usize>>), CodecError> {
    let mut ranges = Vec::new();
    if !property.is_transient() {
        let mut values = Vec::new();
        let mut links = Vec::new();
        let mut slots = 0;
        let any = !roots.is_empty();
        for child in source.children(index) {
            if let Some((mut children, child_ranges, targets, child_slots)) = roots.remove(&child) {
                for child in &mut children {
                    child.order += slots;
                    child.parent = child.parent.map(|parent| parent + slots);
                }
                values.extend(children);
                ranges.extend(child_ranges);
                links.extend(targets);
                slots += child_slots;
            } else if property.family != PropertyFamily::Unknown {
                slots += slot_arity(source, child);
            }
        }
        if any || source.children(index).next().is_none() {
            property.body = Some(PropertyBody::Persisted {
                side_entries: side_entries(ctx, &property, &values, context)?,
                values: ValueSlots::new(values, slots).map_err(CodecError::malformed)?,
                links,
            });
        }
    }
    if property.type_name == "App::PropertyString" {
        stage.insert_native(
            ctx,
            "properties",
            crate::native::document_string::DocumentString::new(property.clone())?,
        )?;
    } else {
        stage.insert_native(
            ctx,
            "properties",
            crate::native::document_value::DocumentValue::new(property.clone())?,
        )?;
    }
    Ok((property, ranges))
}

/// Direct field producers use the same header, value callbacks, and one final insertion.
#[cfg(test)]
pub(crate) fn decode_scanned<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    index: usize,
    owner: &str,
    order: usize,
    context: &crate::element_map::adopted::Context<'_, 'source>,
    stage: &mut PartOutput,
) -> Result<(PropertyRecord, Vec<Range<usize>>), CodecError> {
    let property = prepare_header(ctx, source, index, owner, order)?;
    let mut roots = BTreeMap::new();
    if !property.is_transient() {
        let mut children: Vec<_> = source.children(index).collect();
        children.sort_by_key(|child| {
            (
                !source
                    .name(*child)
                    .is_ok_and(|name| matches!(name, "StringHasher" | "StringHasher2")),
                *child,
            )
        });
        for child in children {
            let previous: Vec<_> = roots
                .values()
                .flat_map(|root: &ParsedRoot| root.0.iter().cloned())
                .collect();
            if let Some(parsed) = ctx.decode_part(
                stage,
                source.extent(child)?,
                source.spec(child, "freecad.property_value")?,
                |stage| {
                    decode_value(
                        ctx,
                        source,
                        index,
                        child,
                        &property,
                        !roots.is_empty(),
                        &previous,
                        context,
                        stage,
                    )
                },
            )? {
                roots.insert(child, parsed);
            }
        }
    }
    finish_property(ctx, source, index, property, roots, context, stage)
}

fn side_entry_candidates(property: &PropertyRecord, values: &[ValueRecord]) -> Vec<String> {
    let mut entries = Vec::new();
    for value in values {
        for (key, entry) in &value.attributes {
            let selected = (matches!(key.as_str(), "file" | "File")
                && !crate::persistence::is_xlink_type(&property.type_name)
                && !(links::expression_container(&property.type_name) && value.tag == "XLink"))
                || (property.family == PropertyFamily::File
                    && matches!(key.as_str(), "name" | "Name"));
            if selected && !entry.is_empty() && !entries.contains(entry) {
                entries.push(entry.to_owned());
            }
        }
    }
    entries
}

fn side_entries(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    values: &[ValueRecord],
    context: &crate::element_map::adopted::Context<'_, '_>,
) -> Result<Vec<String>, CodecError> {
    side_entry_candidates(property, values)
        .into_iter()
        .filter(|entry| context.has_declared_entry(entry))
        .map(|entry| {
            let Some(owners) = context.entry_owners else {
                // A typed-only writer adapter has no source name authority.
                return Ok(entry);
            };
            let name = owners
                .get(&entry)
                .ok_or_else(|| {
                    CodecError::malformed("declared side entry has no original header name source")
                })?
                .read_name(ctx)?;
            if name != entry {
                return Err(CodecError::malformed(
                    "side entry name differs from its original source",
                ));
            }
            Ok(name)
        })
        .collect()
}

fn required(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'_>,
    index: usize,
    name: &str,
) -> Result<String, CodecError> {
    source
        .attribute(ctx, index, name)?
        .ok_or_else(|| CodecError::malformed(format_args!("Property has no {name} attribute")))
}

fn validate_scalar(
    type_name: &str,
    tag: &str,
    attributes: &BTreeMap<String, String>,
) -> Result<(), CodecError> {
    if type_name == "App::PropertyEnumeration" {
        return Ok(());
    }
    let Some(expected) = crate::persistence::scalar_value_tag(type_name) else {
        return Ok(());
    };
    if tag != expected {
        return Err(CodecError::malformed(format_args!(
            "{type_name} requires {expected}"
        )));
    }
    let value = attributes
        .get("value")
        .ok_or_else(|| CodecError::malformed("scalar value is absent"))?;
    let valid = match tag {
        "Float" => value.parse::<f64>().is_ok_and(f64::is_finite),
        "Integer" => value.parse::<i64>().is_ok(),
        "Bool" => crate::native::parse_bool(value).is_some(),
        _ => true,
    };
    if !valid {
        return Err(CodecError::malformed(format_args!(
            "{type_name} {tag} value {value:?} is invalid"
        )));
    }
    Ok(())
}

fn read_element<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    index: usize,
    property: &PropertyRecord,
    stage: &mut PartOutput,
    context: &crate::element_map::adopted::Context<'_, 'source>,
    frame_children: bool,
) -> Result<ParsedValues, CodecError> {
    let _depth = ctx.enter_nested("FreeCAD property value elements")?;
    source.validate(index)?;
    let peek = crate::persistence::source::peek_value_attributes(ctx, source, index)?;
    if property.family == PropertyFamily::Unknown {
        return Ok((Vec::new(), Vec::new(), 0));
    }
    if links::local_carrier(&property.type_name, source.name(index)?) {
        let mut attributes = crate::persistence::source::peek_value_attributes(ctx, source, index)?;
        crate::native::document_value::canonicalize_local_value(
            source.name(index)?,
            &mut attributes,
        );
        if links::local_target_object(&property.type_name, source.name(index)?, &attributes)?
            .is_some_and(|object| {
                context.has_declared_object(&object) && !context.has_admitted_object(&object)
            })
        {
            unavailable_children(ctx, source, index, stage)?;
            return Ok((Vec::new(), Vec::new(), slot_arity(source, index)));
        }
    }
    links::validate(
        &property.type_name,
        source.name(index)?,
        &peek,
        source.children(index).count(),
    )?;
    let child_tags = source
        .children(index)
        .map(|child| source.name(child))
        .collect::<Result<Vec<_>, _>>()?;
    crate::gui::value_grammar::validate_layout(
        &property.type_name,
        source.name(index)?,
        &peek,
        &child_tags,
    )?;
    let decoded_json = python_json(source.name(index)?, &peek);
    let opaque = property.family == PropertyFamily::PythonObject && decoded_json.is_none();
    let frame_children = frame_children && !opaque;
    let attributes = crate::persistence::source::value_attributes(ctx, source, index, opaque)?;
    if links::local_carrier(&property.type_name, source.name(index)?) {
        let mut canonical = attributes.clone();
        crate::native::document_value::canonicalize_local_value(
            source.name(index)?,
            &mut canonical,
        );
        links::validate_local_target(
            &property.type_name,
            source.name(index)?,
            &canonical,
            |object| context.has_declared_object(object),
        )?;
    }
    crate::gui::value_grammar::validate_layout(
        &property.type_name,
        source.name(index)?,
        &attributes,
        &child_tags,
    )?;
    let text = if opaque || !frame_children {
        None
    } else if matches!(source.name(index)?, "String" | "Unicode") {
        let text = source.text(ctx, index)?;
        (!text.is_empty()).then_some(text)
    } else {
        if !source.peek_text(ctx, index)?.trim().is_empty() {
            return Err(CodecError::malformed(
                "registered value has unexpected direct character content",
            ));
        }
        None
    };
    let extent = source.extent(index)?;
    let mut ranges = vec![extent.start()..extent.end()];
    let mut values = vec![ValueRecord {
        item_index: enum_index(&property.type_name, source, index)?,
        link_index: links::source_index(&property.type_name, source, index)?,
        tag: source.read_name(index)?.to_owned(),
        order: 0,
        parent: None,
        attributes,
        decoded_json: decoded_json.map(Into::into),
        text,
    }];
    let mut slots = 1;
    for child in source.children(index).filter(|_| frame_children) {
        let arity = slot_arity(source, child);
        let parsed = ctx.decode_part(
            stage,
            source.extent(child)?,
            source.spec(child, "freecad.value_element")?,
            |child_stage| {
                let attributes =
                    crate::persistence::source::peek_value_attributes(ctx, source, child)?;
                crate::gui::value_grammar::validate_branch(
                    &property.type_name,
                    source.name(child)?,
                    &attributes,
                    &child_tags,
                )?;
                crate::gui::value_grammar::validate_child_of(
                    &property.type_name,
                    source.name(child)?,
                    source.name(index)?,
                    &attributes,
                )?;
                let parsed =
                    read_element(ctx, source, child, property, child_stage, context, true)?;
                if let Some(value) = parsed.0.first() {
                    crate::gui::value_grammar::validate_child_of(
                        &property.type_name,
                        source.name(child)?,
                        source.name(index)?,
                        &value.attributes,
                    )?;
                }
                if let Some(value) = parsed.0.first() {
                    crate::gui::value_grammar::validate_branch(
                        &property.type_name,
                        source.name(child)?,
                        &value.attributes,
                        &child_tags,
                    )?;
                    crate::gui::value_grammar::validate_previous(
                        &property.type_name,
                        source.name(child)?,
                        Some(0),
                        &value.attributes,
                        &values,
                    )?;
                }
                Ok(parsed)
            },
        )?;
        if let Some((mut children, child_ranges, _)) = parsed {
            let base = slots;
            for value in &mut children {
                value.order += base;
                value.parent = Some(value.parent.map_or(0, |parent| parent + base));
            }
            values.extend(children);
            ranges.extend(child_ranges);
        }
        slots += arity;
    }
    Ok((values, ranges, slots))
}

/// An unavailable reference prevents semantic value construction, while genuine
/// descendant locators still own their Appendix C structural helper scopes.
fn unavailable_children<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    index: usize,
    output: &mut PartOutput,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("unavailable FreeCAD reference children")?;
    for child in source.children(index) {
        ctx.decode_part(
            output,
            source.extent(child)?,
            source.spec(child, "freecad.value_element")?,
            |stage| {
                source.validate(child)?;
                unavailable_children(ctx, source, child, stage)
            },
        )?;
    }
    Ok(())
}

/// Naming carriers own their nested fields in their specialized native arenas.
fn slot_arity(source: &XmlSource<'_>, index: usize) -> usize {
    let specialized = |index| {
        source.name(index).is_ok_and(|name| {
            matches!(
                name,
                "StringHasher" | "StringHasher2" | "ElementMap" | "ElementMap2"
            )
        })
    };
    if specialized(index) {
        return 1;
    }
    // Scanner element indices already form source preorder. Walk only its navigation
    // links, without adding a second recursive decode-depth limit before child admission.
    source
        .elements()
        .iter()
        .enumerate()
        .skip(index)
        .take_while(|(candidate, element)| {
            if *candidate == index {
                return true;
            }
            let mut parent = element.parent();
            while let Some(ancestor) = parent {
                if ancestor == index {
                    return true;
                }
                if ancestor < index {
                    return false;
                }
                parent = source.elements()[ancestor].parent();
            }
            false
        })
        .filter(|(candidate, element)| {
            if *candidate == index {
                return true;
            }
            let mut parent = element.parent();
            while let Some(ancestor) = parent.filter(|ancestor| *ancestor != index) {
                if specialized(ancestor) {
                    return false;
                }
                parent = source.elements()[ancestor].parent();
            }
            true
        })
        .count()
}

pub(crate) fn python_json(
    tag: &str,
    attributes: &BTreeMap<String, String>,
) -> Option<serde_json::Value> {
    use base64::Engine;
    if tag != "Python" || attributes.get("json").map(String::as_str) != Some("yes") {
        return None;
    }
    let raw = attributes.get("value")?;
    if attributes.get("encoded").map(String::as_str) == Some("yes") {
        let decoded = base64::engine::general_purpose::STANDARD.decode(raw).ok()?;
        serde_json::from_slice(&decoded).ok()
    } else {
        serde_json::from_str(raw).ok()
    }
}

pub(crate) fn validate_python_mode(
    tag: &str,
    attributes: &BTreeMap<String, String>,
) -> Result<(), CodecError> {
    if tag != "Python" {
        return Ok(());
    }
    match attributes.get("json").map(String::as_str) {
        None | Some("no") => return Ok(()),
        Some("yes") => {}
        Some(_) => return Err(CodecError::malformed("Python json mode is not yes or no")),
    }
    if !matches!(
        attributes.get("encoded").map(String::as_str),
        None | Some("yes" | "no")
    ) {
        return Err(CodecError::malformed(
            "Python JSON encoded mode is not yes or no",
        ));
    }
    if python_json(tag, attributes).is_none() {
        return Err(CodecError::malformed(
            "Python explicit JSON value is not valid JSON",
        ));
    }
    Ok(())
}

/// Enum positions are semantic list ordinals, independent of tree preorder slots.
pub(crate) fn enum_index(
    kind: &str,
    source: &XmlSource<'_>,
    index: usize,
) -> Result<Option<usize>, CodecError> {
    if kind != "App::PropertyEnumeration" || source.name(index)? != "Enum" {
        return Ok(None);
    }
    let Some(parent) = source.elements()[index].parent() else {
        return Ok(None);
    };
    if source.name(parent)? != "CustomEnumList" {
        return Ok(None);
    }
    Ok(source.children(parent).position(|child| child == index))
}
