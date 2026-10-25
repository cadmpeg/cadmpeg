// SPDX-License-Identifier: Apache-2.0
//! GUI records are constructed in their source element transactions.

use super::super::{Graph, GuiSources};
use crate::fidelity::ValueSourceRanges;
use crate::gui::payloads::sources::EntryInterpretations;
use crate::native::{
    GuiDocumentRecord, GuiPropertyRecord, GuiStateRecord, GuiViewProviderRecord, ValueRecord,
};
use crate::xml_framing::XmlSource;
use cadmpeg_core::decode::{DecodeContext, RecordOwner, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ops::Range;
struct MemoTransaction<'a, 'source> {
    memo: &'a RefCell<EntryInterpretations<'source>>,
    checkpoint: usize,
    accepted: bool,
}
impl<'a, 'source> MemoTransaction<'a, 'source> {
    fn new(memo: &'a RefCell<EntryInterpretations<'source>>) -> Self {
        Self {
            memo,
            checkpoint: memo.borrow().checkpoint(),
            accepted: false,
        }
    }
    fn commit(mut self) {
        self.accepted = true;
    }
}
impl Drop for MemoTransaction<'_, '_> {
    fn drop(&mut self) {
        if !self.accepted {
            self.memo.borrow_mut().rollback(self.checkpoint);
        }
    }
}

pub(super) fn decode<'a>(
    ctx: &DecodeContext<'a>,
    view: View<'a>,
    sources: &GuiSources<'_, 'a>,
    output: &mut PartOutput,
) -> Result<(Graph, Vec<View<'a>>, EntryInterpretations<'a>), CodecError> {
    let level = PartLevel::parse("freecad.gui_document_xml")
        .ok_or_else(|| CodecError::malformed("missing GUI XML part level"))?;
    let mut member_spec = PartSpec::new(level, Vec::new());
    if let Some(parent) = sources
        .entry_parents
        .and_then(|parents| parents.get("GuiDocument.xml"))
    {
        member_spec = member_spec.with_parent(parent);
    }
    let graph = ctx.decode_part(output, view, member_spec, |stage| {
        let scan = std::rc::Rc::new(XmlSource::new(ctx,view)?);
        let xml = scan.as_ref();
        let memo=RefCell::new(EntryInterpretations::default());
        memo.borrow_mut().set_presentation_source(std::rc::Rc::clone(&scan));
        let roots = xml.elements().iter().enumerate().filter(|(_, node)| node.parent().is_none()).map(|(index, _)| index).collect::<Vec<_>>();
        if roots.is_empty() { return Err(CodecError::malformed("GuiDocument.xml has no root element")); }
        let mut graph = Graph::default();
        for root in roots {
            if let Some(parsed) = ctx.decode_part(stage, xml.extent(root)?, xml.spec(root, "freecad.gui_document")?, |stage| {
                xml.validate(root)?;
                if !matches!(xml.name(root)?, "Document" | "GuiDocument") { return Err(CodecError::malformed("GUI root is not Document")); }
                let (attributes, document_owner) = ctx.record_owner("fcstd:gui:document#0", xml.extent(root)?, || xml.attributes(ctx, root))?;
                {
                    if attributes.contains_key("schemaVersion") && !attributes.contains_key("SchemaVersion") { return Err(CodecError::malformed("unsupported schemaVersion; expected SchemaVersion")); }
                    let memo_guard=MemoTransaction::new(&memo);
                    let mut result = Graph::default();
                    let mut states = Vec::new();
                    let mut provider_order = 0;
                    let mut state_order = 0;
                    let mut provider_data_seen = false;
                    for child in xml.children(root) {
                        if xml.name(child)? == "ViewProviderData" {
                            if provider_data_seen {
                                ctx.decode_part(stage, xml.extent(child)?, xml.spec(child, "freecad.view_provider_data")?, |_stage| Err::<(), _>(CodecError::malformed("GUI document has an extra ViewProviderData container")))?;
                                continue;
                            }
                            provider_data_seen = true;
                            providers(ctx, view, xml, child, sources, &memo, stage, &mut result, &mut provider_order)?;
                        } else {
                            if let Some((state, ranges)) = state(ctx, view, xml, child, state_order, sources, stage, &document_owner)? {
                                bind_presentation(ctx,&memo,&state.id,child)?;
                                result.state_sources.insert(state.id.clone(), ranges);
                                states.push(state);
                            }
                            state_order += 1;
                        }
                    }
                    let declaration = attributes.get("SchemaVersion").cloned();
                    let admission = super::super::schema::classify(declaration.as_deref());
                    if let super::super::schema::Admission::Unverified { declaration } = admission {
                        result.losses.push(crate::loss::FreecadLossCode::SourceGuiSchemaUnverified.note(format!("GuiDocument.xml declares schema {}; attempted the schema-1 vocabulary", declaration.as_deref().unwrap_or("missing"))));
                    }
                    let document = GuiDocumentRecord { id: "fcstd:gui:document#0".into(), schema_version: declaration, attributes, states };
                    bind_presentation(ctx,&memo,&document.id,root)?;
                    stage.insert_native(ctx, "gui_documents", document.clone())?;
                    result.documents.push(document);
                    result.value_sources = result.property_sources.values().chain(result.state_sources.values()).filter(|source| source.has_values()).cloned().collect();
                    memo_guard.commit();
                    Ok(result)
                }
            })? { merge(&mut graph, parsed); }
        }
        Ok((graph,xml.insignificant(ctx)?,memo.into_inner()))
    })?;
    let (mut graph, whitespace, memo) = graph.unwrap_or_default();
    graph.untyped = graph.documents.is_empty();
    Ok((graph, whitespace, memo))
}

fn bind_presentation<'source>(
    ctx: &DecodeContext<'_>,
    memo: &RefCell<EntryInterpretations<'source>>,
    id: &str,
    index: usize,
) -> Result<(), CodecError> {
    let scan = memo.borrow().presentation_source()?;
    let control = super::presentation::Control::new(ctx, scan, index)?;
    memo.borrow_mut().bind_presentation(ctx, id, control)
}

fn merge(graph: &mut Graph, mut other: Graph) {
    graph.entry_contents.append(&mut other.entry_contents);
    graph.documents.append(&mut other.documents);
    graph.providers.append(&mut other.providers);
    graph.properties.append(&mut other.properties);

    graph.provider_sources.append(&mut other.provider_sources);
    graph.value_sources.append(&mut other.value_sources);
    graph.state_sources.append(&mut other.state_sources);
    graph.property_sources.append(&mut other.property_sources);
    graph.losses.append(&mut other.losses);
}

fn providers<'a>(
    ctx: &DecodeContext<'a>,
    view: View<'a>,
    xml: &XmlSource<'a>,
    index: usize,
    sources: &GuiSources<'_, 'a>,
    memo: &RefCell<EntryInterpretations<'a>>,
    output: &mut PartOutput,
    graph: &mut Graph,
    order: &mut usize,
) -> Result<(), CodecError> {
    let admitted = ctx.decode_part(
        output,
        xml.extent(index)?,
        xml.spec(index, "freecad.view_provider_data")?,
        |stage| {
            xml.validate(index)?;
            let attributes = selected_attributes(ctx, xml, index, &["Count"])?;
            let children = xml.children(index).collect::<Vec<_>>();
            let count = attributes
                .get("Count")
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| CodecError::malformed("invalid ViewProviderData Count"))?;
            if count != children.len() {
                return Err(CodecError::malformed(format_args!(
                    "ViewProviderData Count={count} but {} records were found",
                    children.len()
                )));
            }
            let memo_guard=MemoTransaction::new(memo);
            let mut result = Graph::default();
            for child in children {
                let current = *order;
                *order += 1;
                if let Some(parsed) = ctx.decode_part(
                    stage,
                    xml.extent(child)?,
                    xml.spec(child, "freecad.view_provider")?,
                    |stage| {
                        xml.validate(child)?;
                        if xml.name(child)? != "ViewProvider" {
                            return Err(CodecError::malformed("unexpected view-provider element"));
                        }
                        let locator = xml.peek_attributes(ctx, child)?;
                        let id = crate::native::native_id(
                            "gui-view-provider",
                            present(&locator, "name")?,
                        );
                        if graph
                            .providers
                            .iter()
                            .chain(result.providers.iter())
                            .any(|provider| provider.id == id)
                        {
                            return Err(CodecError::malformed("duplicate ViewProvider name"));
                        }
                        ctx.record(&id, xml.extent(child)?, || {
                            let attributes =
                                selected_attributes(ctx, xml, child, &["name", "expanded"])?;
                            let name = present(&attributes, "name")?;
                            let memo_guard=MemoTransaction::new(memo);
                            let mut parsed = Graph::default();
                            let object = sources
                                .objects
                                .iter()
                                .find(|object| object.name() == name)
                                .map(|object| {
                                    sources.object_sources.map_or_else(
                                        || Ok(object.id().to_owned()),
                                        |sources| {
                                            sources
                                                .get(object.id())
                                                .ok_or_else(|| CodecError::malformed("GUI provider has no actual application object source"))?
                                                .object_id(ctx)
                                        },
                                    )
                                })
                                .transpose()?
                                .or_else(|| {
                                    let id = crate::native::native_id("object", name);
                                    sources
                                        .declared_objects
                                        .is_some_and(|objects| objects.contains(&id))
                                        .then_some(id)
                                });
                            let provider = GuiViewProviderRecord {
                                id: id.clone(),
                                object: object
                                    .map(|id| {
                                        cadmpeg_core::text::NonBlankString::new(id).ok_or_else(
                                            || {
                                                CodecError::malformed(
                                                    "GUI provider object identity is blank",
                                                )
                                            },
                                        )
                                    })
                                    .transpose()?,
                                name: name.to_owned(),
                                expanded: attributes
                                    .get("expanded")
                                    .map(|value| {
                                        crate::native::parse_bool(value).ok_or_else(|| {
                                            CodecError::malformed(
                                                "invalid GUI provider expansion flag",
                                            )
                                        })
                                    })
                                    .transpose()?,
                                order: current,
                            };
                            bind_presentation(ctx,memo,&provider.id,child)?;
                            stage.insert_native(ctx, "gui_view_providers", provider.clone())?;
                            parsed
                                .provider_sources
                                .push((id.clone(), relative(view, xml.extent(child)?)?));
                            parsed.providers.push(provider);
                            let mut properties_seen = false;
                            let mut extensions_seen = false;
                            for owned in xml.children(child) {
                                let (seen, level) = match xml.name(owned)? {
                                    "Properties" => {
                                        (&mut properties_seen, "freecad.gui_properties")
                                    }
                                    "Extensions" => {
                                        (&mut extensions_seen, "freecad.gui_extensions")
                                    }
                                    _ => {
                                        return Err(CodecError::malformed(
                                            "unexpected GUI provider child",
                                        ))
                                    }
                                };
                                if *seen {
                                    ctx.decode_part(
                                        stage,
                                        xml.extent(owned)?,
                                        xml.spec(owned, level)?,
                                        |_stage| {
                                            Err::<(), _>(CodecError::malformed(
                                                "GUI provider has an extra child container",
                                            ))
                                        },
                                    )?;
                                    continue;
                                }
                                *seen = true;
                                match xml.name(owned)? {
                                    "Properties" => properties(
                                        ctx,
                                        view,
                                        xml,
                                        owned,
                                        &id,
                                        sources,
                                        memo,
                                        stage,
                                        &mut parsed,
                                        true,
                                    )?,
                                    "Extensions" => extensions(
                                        ctx,
                                        view,
                                        xml,
                                        owned,
                                        &id,
                                        sources,
                                        memo,
                                        stage,
                                        &mut parsed,
                                    )?,
                                    _ => {
                                        return Err(CodecError::malformed(
                                            "unexpected GUI provider child",
                                        ))
                                    }
                                }
                            }
                            if !properties_seen {
                                return Err(CodecError::malformed(
                                    "GUI provider has no direct Properties container",
                                ));
                            }
                            memo_guard.commit();
                            Ok(parsed)
                        })
                    },
                )? {
                    merge(&mut result, parsed);
                }
            }
            memo_guard.commit();
            Ok(result)
        },
    )?;
    if let Some(parsed) = admitted {
        merge(graph, parsed);
    }
    Ok(())
}

fn xml_part<T>(
    ctx: &DecodeContext<'_>,
    output: &mut PartOutput,
    view: View<'_>,
    spec: PartSpec,
    listed: bool,
    decode: impl FnOnce(&mut PartOutput) -> Result<T, CodecError>,
) -> Result<Option<T>, CodecError> {
    if listed {
        ctx.decode_part(output, view, spec, decode)
    } else {
        decode(output).map(Some)
    }
}

fn properties<'a>(
    ctx: &DecodeContext<'a>,
    view: View<'a>,
    xml: &XmlSource<'a>,
    index: usize,
    owner: &str,
    sources: &GuiSources<'_, 'a>,
    memo: &RefCell<EntryInterpretations<'a>>,
    output: &mut PartOutput,
    graph: &mut Graph,
    listed: bool,
) -> Result<(), CodecError> {
    let parsed = xml_part(
        ctx,
        output,
        xml.extent(index)?,
        xml.spec(index, "freecad.gui_properties")?,
        listed,
        |stage| {
            xml.validate(index)?;
            let attributes = selected_attributes(ctx, xml, index, &["Count"])?;
            let children = xml.children(index).collect::<Vec<_>>();
            let count = required(&attributes, "Count")?
                .parse::<usize>()
                .map_err(|_| CodecError::malformed("invalid GUI Properties Count"))?;
            if count != children.len() {
                return Err(CodecError::malformed(
                    "GUI Properties Count differs from child count",
                ));
            }
            let memo_guard = MemoTransaction::new(memo);
            let mut parsed = Graph::default();
            for (order, child) in children.into_iter().enumerate() {
                if let Some((property, ranges, payloads)) = xml_part(
                    ctx,
                    stage,
                    xml.extent(child)?,
                    xml.spec(child, "freecad.gui_property")?,
                    listed,
                    |stage| {
                        xml.validate(child)?;
                        if xml.name(child)? != "Property" {
                            return Err(CodecError::malformed("unexpected GUI property element"));
                        }
                        let locator = xml.peek_attributes(ctx, child)?;
                        let id = crate::native::native_child_id(
                            "gui-property",
                            owner,
                            present(&locator, "name")?,
                        );
                        if parsed
                            .properties
                            .iter()
                            .chain(graph.properties.iter())
                            .any(|property| property.id == id)
                        {
                            return Err(CodecError::malformed("duplicate GUI property name"));
                        }
                        ctx.record(&id, xml.extent(child)?, || {
                            let attributes =
                                selected_attributes(ctx, xml, child, &["name", "type", "status"])?;
                            let name = present(&attributes, "name")?;
                            let kind = required(&attributes, "type")?;
                            let mut values = Vec::new();
                            let mut ranges = Vec::new();
                            for (root_order, value) in xml.children(child).enumerate() {
                                if root_order > 0
                                    && (super::super::value_grammar::requires_single_root(kind) || super::super::is_visual_layer_list(name,kind))
                                {
                                    xml_part(
                                        ctx, stage,
                                        xml.extent(value)?,
                                        xml.spec(value, "freecad.gui_property_value")?,
                                        listed, |_stage| {
                                            Err::<(), _>(CodecError::malformed(
                                                "property has an extra value root",
                                            ))
                                        },
                                    )?;
                                    continue;
                                }
                                value_tree(
                                    ctx,
                                    view,
                                    xml,
                                    sources,
                                    value,
                                    child,
                                    "freecad.gui_property_value",
                                    stage,
                                    &mut values,
                                    &mut ranges,
                                    Some((child, name, kind)),
                                    listed,
                                )?;
                            }
                            let side_entries =
                                side_entries(ctx,&values, crate::persistence::is_xlink_type(kind) || crate::persistence::document_properties::value_parts::links::expression_container(kind), sources)?;
                            let links = if let Some(root) = xml.children(child).next().filter(|_| !values.is_empty()) {
                                let absolute_ranges = ranges.iter().map(|(_, range)| {
                                    let start = view.start().checked_add(range.start).ok_or_else(|| CodecError::malformed("GUI link source offset overflows"))?;
                                    let end = view.start().checked_add(range.end).ok_or_else(|| CodecError::malformed("GUI link source end overflows"))?;
                                    Ok(start..end)
                                }).collect::<Result<Vec<_>,CodecError>>()?;
                                crate::persistence::document_properties::value_parts::links::from_values(kind, &values, xml, root, &absolute_ranges)?
                            } else { Vec::new() };
                            let arity = if super::super::has_registered_property_grammar(name,kind) {
                                if kind.contains("PropertyPythonObject") && values.iter().all(|value| value.decoded_json.is_none()) {xml.children(child).count()} else {descendant_count(xml,child)}
                            } else {0};
                            let property_memo_guard=MemoTransaction::new(memo);
                            let property = GuiPropertyRecord {
                                id: id.clone(),
                                owner: owner.to_owned(),
                                name: name.to_owned(),
                                type_name: kind.to_owned(),
                                status: attributes.get("status").map(|value|value.parse::<u64>().map_err(|_|CodecError::malformed("invalid GUI property status bits"))).transpose()?,
                                order,
                                values: crate::native::ValueSlots::new(values,arity).map_err(CodecError::malformed)?,
                                side_entries,
                                links,
                            };
                            bind_presentation(ctx,memo,&property.id,child)?;
                            let value_index=property.values.iter().find(|value|value.parent.is_none() && value.attributes.get("file").is_some_and(|file|!file.is_empty())).and_then(|value|child.checked_add(1)?.checked_add(value.order)).unwrap_or(child);
                            let mut payloads = super::super::payloads::decode(
                                ctx, (&property).into(),
                                super::super::payloads::SourceRequest {xml,value:value_index,entries:sources.entries,parents:sources.entry_parents,owners:sources.entry_owners,base:sources.entry_interpretations},
                                &mut memo.borrow_mut(),stage,
                            )?;
                            payloads.entry_contents.extend(crate::application_geometry::parts::decode_gui(ctx,&property,
                                super::super::payloads::SourceRequest{xml,value:value_index,entries:sources.entries,parents:sources.entry_parents,owners:sources.entry_owners,base:sources.entry_interpretations},
                                &mut memo.borrow_mut(),stage)?);
                            stage.insert_native(ctx, "gui_properties", property.clone())?;
                            let source_ranges=ValueSourceRanges::new_slots(id.clone(),relative(view,xml.extent(child)?)?,ranges)?;
                            property_memo_guard.commit();
                            Ok((property,source_ranges,payloads.entry_contents))
                        })
                    },
                )? {
                    parsed.property_sources.insert(property.id.clone(), ranges);
                    parsed.properties.push(property);
                    parsed.entry_contents.extend(payloads);
                }
            }
            memo_guard.commit();
            Ok(parsed)
        },
    )?;
    if let Some(parsed) = parsed {
        merge(graph, parsed);
    }
    Ok(())
}

fn state<'a>(
    ctx: &DecodeContext<'a>,
    view: View<'a>,
    xml: &XmlSource<'a>,
    index: usize,
    order: usize,
    sources: &GuiSources<'_, 'a>,
    output: &mut PartOutput,
    document_owner: &RecordOwner<'_>,
) -> Result<Option<(GuiStateRecord, ValueSourceRanges)>, CodecError> {
    ctx.decode_part(
        output,
        xml.extent(index)?,
        xml.spec(index, "freecad.gui_state")?,
        |stage| {
            xml.validate(index)?;
            let kind = xml.name(index)?;
            let id = crate::native::native_id("gui-state", format!("{kind}:{order}"));
            ctx.record_fragment(document_owner, xml.extent(index)?, || {
                let kind = xml.read_name(index)?.to_owned();
                let attributes = xml.attributes(ctx, index)?;
                let mut values = Vec::new();
                let mut ranges = Vec::new();
                for child in xml.children(index) {
                    value_tree(
                        ctx,
                        view,
                        xml,
                        sources,
                        child,
                        index,
                        "freecad.gui_state_element",
                        stage,
                        &mut values,
                        &mut ranges,
                        None,
                        true,
                    )?;
                }
                let mut side_entries = side_entries(ctx, &values, false, sources)?;
                for (key, value) in &attributes {
                    if matches!(key.as_str(), "file" | "File") {
                        if let Some(name) = side_entry_name(ctx, value, sources)? {
                            side_entries.push(name);
                        }
                    }
                }
                let state = GuiStateRecord {
                    id: id.clone(),
                    kind,
                    attributes,
                    values: crate::native::ValueSlots::new(values, descendant_count(xml, index))
                        .map_err(CodecError::malformed)?,
                    side_entries,
                };
                Ok((
                    state,
                    ValueSourceRanges::new_slots(
                        id.clone(),
                        relative(view, xml.extent(index)?)?,
                        ranges,
                    )?,
                ))
            })
        },
    )
}

fn value_tree<'a>(
    ctx: &DecodeContext<'a>,
    view: View<'a>,
    xml: &XmlSource<'a>,
    sources: &GuiSources<'_, 'a>,
    index: usize,
    owner_index: usize,
    level: &str,
    output: &mut PartOutput,
    values: &mut Vec<ValueRecord>,
    ranges: &mut Vec<(usize, Range<usize>)>,
    property: Option<(usize, &str, &str)>,
    listed: bool,
) -> Result<(), CodecError> {
    let accepted = xml_part(
        ctx,
        output,
        xml.extent(index)?,
        xml.spec(index, level)?,
        listed,
        |stage| {
            xml.validate(index)?;
            let tag = xml.name(index)?;
            if let Some((_, name, kind)) = property.filter(|(_, name, kind)| {
                !super::super::has_registered_property_grammar(name, kind)
            }) {
                return Err(CodecError::NotImplemented(format!(
                    "FreeCAD GUI property {name} type {kind}"
                )));
            }
            let _depth = ctx.enter_nested("FreeCAD GUI value elements")?;
            if let Some((_, _, kind)) = property.filter(|(_, _, kind)| {
                crate::persistence::document_properties::value_parts::links::local_carrier(
                    kind, tag,
                )
            }) {
                let mut locator =
                    crate::persistence::source::peek_value_attributes(ctx, xml, index)?;
                crate::native::document_value::canonicalize_local_value(
                    xml.name(index)?,
                    &mut locator,
                );
                if crate::persistence::document_properties::value_parts::links::local_target_object(
                    kind,
                    xml.name(index)?,
                    &locator,
                )?
                .is_some_and(|id| {
                    sources
                        .declared_objects
                        .is_some_and(|objects| objects.contains(&id))
                        && sources
                            .admitted_objects
                            .is_some_and(|objects| !objects.contains(&id))
                }) {
                    return Ok((Vec::new(), Vec::new()));
                }
            }
            let python = property.is_some_and(|(_, _, kind)| kind.contains("PropertyPythonObject"));
            let peek = python
                .then(|| crate::persistence::source::peek_value_attributes(ctx, xml, index))
                .transpose()?;
            let decoded_json = peek.as_ref().and_then(|attributes| {
                crate::persistence::document_properties::value_parts::python_json(
                    xml.name(index).ok()?,
                    attributes,
                )
            });
            let opaque = python && decoded_json.is_none();
            let attributes = if property.is_some() {
                crate::persistence::source::value_attributes(ctx, xml, index, opaque)?
            } else {
                xml.attributes(ctx, index)?
            };
            let text = if opaque {
                None
            } else if property.is_none() || matches!(xml.name(index)?, "String" | "Unicode") {
                let text = xml.text(ctx, index)?;
                (!text.is_empty()).then_some(text)
            } else {
                if !xml.peek_text(ctx, index)?.trim().is_empty() {
                    return Err(CodecError::malformed(
                        "registered GUI value has unexpected direct character content",
                    ));
                }
                None
            };
            if let Some((property_index, name, kind)) = property {
                let root_value = xml
                    .elements()
                    .get(index)
                    .and_then(|element| element.parent())
                    == Some(property_index);
                if super::super::is_visual_layer_list(name, kind) {
                    let parent = xml
                        .elements()
                        .get(index)
                        .and_then(|element| element.parent())
                        .filter(|parent| *parent != property_index)
                        .map(|parent| xml.name(parent))
                        .transpose()?;
                    super::super::value_grammar::validate_visual_layer(
                        xml.name(index)?,
                        parent,
                        &attributes,
                        xml.children(index).count(),
                    )?;
                }
                if crate::persistence::link_grammar(kind).is_none()
                    && crate::persistence::document_properties::value_parts::links::local_carrier(
                        kind, tag,
                    )
                {
                    crate::persistence::document_properties::value_parts::links::validate(
                        kind,
                        tag,
                        &attributes,
                        xml.children(index).count(),
                    )?;
                }
                if let Some(grammar) = crate::persistence::link_grammar(kind) {
                    if root_value && xml.name(index)? != grammar.root_tag() {
                        return Err(CodecError::malformed(
                            "GUI link root differs from its registered grammar",
                        ));
                    }
                    crate::persistence::document_properties::value_parts::links::validate(
                        kind,
                        xml.name(index)?,
                        &attributes,
                        xml.children(index).count(),
                    )?;
                }
                let child_names = xml
                    .children(index)
                    .map(|child| xml.name(child))
                    .collect::<Result<Vec<_>, _>>()?;
                super::super::value_grammar::validate_layout(
                    kind,
                    xml.name(index)?,
                    &attributes,
                    &child_names,
                )?;
                if root_value {
                    if !super::super::has_registered_property_grammar(name, kind) {
                        return Err(CodecError::NotImplemented(format!(
                            "FreeCAD GUI property type {kind}"
                        )));
                    }
                    let root_tags = xml
                        .children(property_index)
                        .map(|root| xml.name(root))
                        .collect::<Result<Vec<_>, _>>()?;
                    super::super::value_grammar::validate_value_roots(
                        kind,
                        xml.name(index)?,
                        &attributes,
                        &root_tags,
                    )?;
                    super::super::value_grammar::validate_root(
                        kind,
                        xml.name(index)?,
                        if opaque {
                            peek.as_ref().ok_or_else(|| {
                                CodecError::malformed("opaque Python header is absent")
                            })?
                        } else {
                            &attributes
                        },
                        xml.children(index).count(),
                    )?;
                    if matches!(name, "LineWidth" | "PointSize")
                        && kind == "App::PropertyFloatConstraint"
                        && required(&attributes, "value")?
                            .parse::<f64>()
                            .is_ok_and(|value| value < 0.0)
                    {
                        return Err(CodecError::malformed("GUI primitive size is negative"));
                    }
                    if name == "Transparency" && kind == "App::PropertyPercent" {
                        let percent =
                            required(&attributes, "value")?
                                .parse::<i64>()
                                .map_err(|_| {
                                    CodecError::malformed("invalid transparency percentage")
                                })?;
                        if !(0..=100).contains(&percent) {
                            return Err(CodecError::malformed(
                                "transparency percentage is outside 0..=100",
                            ));
                        }
                    }
                } else {
                    let parent = xml
                        .elements()
                        .get(index)
                        .and_then(|element| element.parent())
                        .ok_or_else(|| CodecError::malformed("GUI value element has no parent"))?;
                    super::super::value_grammar::validate_child_of(
                        kind,
                        xml.name(index)?,
                        xml.name(parent)?,
                        &attributes,
                    )?;
                    let siblings = xml
                        .children(parent)
                        .map(|child| xml.name(child))
                        .collect::<Result<Vec<_>, _>>()?;
                    super::super::value_grammar::validate_branch(
                        kind,
                        xml.name(index)?,
                        &attributes,
                        &siblings,
                    )?;
                }
            }
            let order = index
                .checked_sub(owner_index)
                .and_then(|value| value.checked_sub(1))
                .ok_or_else(|| CodecError::malformed("GUI value is outside its owning element"))?;
            let parent = xml
                .elements()
                .get(index)
                .and_then(|element| element.parent())
                .filter(|parent| *parent != owner_index)
                .map(|parent| {
                    parent
                        .checked_sub(owner_index)
                        .and_then(|value| value.checked_sub(1))
                        .ok_or_else(|| {
                            CodecError::malformed("GUI value parent is outside its owning element")
                        })
                })
                .transpose()?;
            if let Some((_, _, kind)) = property {
                super::super::value_grammar::validate_previous(
                    kind,
                    xml.name(index)?,
                    parent,
                    &attributes,
                    values,
                )?;
            }
            if property.is_some_and(|(_, _, kind)| {
                crate::persistence::document_properties::value_parts::links::local_carrier(
                    kind, tag,
                )
            }) {
                let mut canonical = attributes.clone();
                crate::native::document_value::canonicalize_local_value(
                    xml.name(index)?,
                    &mut canonical,
                );
                if let Some((_, _, kind)) = property {
                    crate::persistence::document_properties::value_parts::links::validate_local_target(kind,xml.name(index)?,&canonical,|id|sources.declared_objects.is_some_and(|objects|objects.contains(id)))?;
                }
            }
            let mut local = vec![ValueRecord {
                item_index: property
                    .map(|(_, _, kind)| {
                        crate::persistence::document_properties::value_parts::enum_index(
                            kind, xml, index,
                        )
                    })
                    .transpose()?
                    .flatten(),
                link_index: property
                    .map(|(_, _, kind)| {
                        crate::persistence::document_properties::value_parts::links::source_index(
                            kind, xml, index,
                        )
                    })
                    .transpose()?
                    .flatten(),
                parent,
                tag: xml.read_name(index)?.to_owned(),
                order,
                attributes,
                decoded_json: decoded_json.map(Into::into),
                text,
            }];
            let mut local_ranges = vec![(order, relative(view, xml.extent(index)?)?)];
            for child in xml.children(index).filter(|_| !opaque) {
                let child_level = if level == "freecad.gui_state_element" {
                    level
                } else {
                    "freecad.gui_value_element"
                };
                value_tree(
                    ctx,
                    view,
                    xml,
                    sources,
                    child,
                    owner_index,
                    child_level,
                    stage,
                    &mut local,
                    &mut local_ranges,
                    property,
                    listed,
                )?;
            }
            Ok((local, local_ranges))
        },
    )?;
    if let Some((mut local, mut local_ranges)) = accepted {
        values.append(&mut local);
        ranges.append(&mut local_ranges);
    }
    Ok(())
}

fn extensions<'a>(
    ctx: &DecodeContext<'a>,
    view: View<'a>,
    xml: &XmlSource<'a>,
    index: usize,
    owner: &str,
    sources: &GuiSources<'_, 'a>,
    memo: &RefCell<EntryInterpretations<'a>>,
    output: &mut PartOutput,
    graph: &mut Graph,
) -> Result<(), CodecError> {
    let admitted = ctx.decode_part(
        output,
        xml.extent(index)?,
        xml.spec(index, "freecad.gui_extensions")?,
        |stage| {
            xml.validate(index)?;
            let attributes = selected_attributes(ctx, xml, index, &["Count"])?;
            let count = required(&attributes, "Count")?
                .parse::<usize>()
                .map_err(|_| CodecError::malformed("invalid GUI Extensions Count"))?;
            if count != xml.children(index).count() {
                return Err(CodecError::malformed(
                    "GUI Extensions Count differs from child count",
                ));
            }
            let memo_guard = MemoTransaction::new(memo);
            let mut parsed = Graph::default();
            let mut types = std::collections::BTreeSet::new();
            let mut names = std::collections::BTreeSet::new();
            for (order, child) in xml.children(index).enumerate() {
                if let Some((nested, name, kind)) = ctx.decode_part(
                    stage,
                    xml.extent(child)?,
                    xml.spec(child, "freecad.gui_extension")?,
                    |stage| {
                        xml.validate(child)?;
                        if xml.name(child)? != "Extension" {
                            return Err(CodecError::malformed("unexpected GUI extension element"));
                        }
                        let locator = xml.peek_attributes(ctx, child)?;
                        let name = required(&locator, "name")?;
                        let kind = required(&locator, "type")?;
                        if names.contains(name) || types.contains(kind) {
                            return Err(CodecError::malformed(
                                "duplicate GUI extension name or runtime type",
                            ));
                        }
                        let id = crate::native::native_child_id("extension", owner, name);
                        ctx.record(&id, xml.extent(child)?, || {
                            let attributes =
                                selected_attributes(ctx, xml, child, &["name", "type"])?;
                            let name = required(&attributes, "name")?.to_owned();
                            let kind = required(&attributes, "type")?.to_owned();
                            stage.insert_native(
                                ctx,
                                "extensions",
                                crate::native::ExtensionRecord {
                                    id: id.clone(),
                                    owner: owner.to_owned(),
                                    name: name.clone(),
                                    type_name: kind.clone(),
                                    order,
                                },
                            )?;
                            let mut nested = Graph::default();
                            let mut properties_seen = false;
                            for container in xml.children(child) {
                                if xml.name(container)? != "Properties" {
                                    return Err(CodecError::malformed(
                                        "unexpected GUI extension child",
                                    ));
                                }
                                if properties_seen {
                                    return Err(CodecError::malformed(
                                        "GUI extension has extra Properties container",
                                    ));
                                }
                                properties_seen = true;
                                properties(
                                    ctx,
                                    view,
                                    xml,
                                    container,
                                    &id,
                                    sources,
                                    memo,
                                    stage,
                                    &mut nested,
                                    false,
                                )?;
                            }
                            Ok((nested, name, kind))
                        })
                    },
                )? {
                    names.insert(name);
                    types.insert(kind);
                    merge(&mut parsed, nested);
                }
            }
            memo_guard.commit();
            Ok(parsed)
        },
    )?;
    if let Some(parsed) = admitted {
        merge(graph, parsed);
    }
    Ok(())
}

fn side_entry_name(
    ctx: &DecodeContext<'_>,
    name: &str,
    sources: &GuiSources<'_, '_>,
) -> Result<Option<String>, CodecError> {
    if !sources
        .declared_entries
        .is_some_and(|entries| entries.contains(name))
    {
        return Ok(None);
    }
    let Some(owner) = sources.entry_owners.and_then(|owners| owners.get(name)) else {
        return Ok(None);
    };
    let actual = owner.read_name(ctx)?;
    if actual != name {
        return Err(CodecError::malformed(
            "GUI side entry differs from its actual ZIP header name",
        ));
    }
    Ok(Some(actual))
}
fn side_entries(
    ctx: &DecodeContext<'_>,
    values: &[ValueRecord],
    external: bool,
    sources: &GuiSources<'_, '_>,
) -> Result<Vec<String>, CodecError> {
    let mut entries = Vec::new();
    if external {
        return Ok(entries);
    }
    for (key, value) in values.iter().flat_map(|value| value.attributes.iter()) {
        if matches!(key.as_str(), "file" | "File") {
            if let Some(name) = side_entry_name(ctx, value, sources)? {
                entries.push(name);
            }
        }
    }
    Ok(entries)
}

fn relative(view: View<'_>, extent: View<'_>) -> Result<Range<usize>, CodecError> {
    let start = extent
        .start()
        .checked_sub(view.start())
        .ok_or_else(|| CodecError::malformed("GUI source starts before its member"))?;
    let end = extent
        .end()
        .checked_sub(view.start())
        .ok_or_else(|| CodecError::malformed("GUI source ends before its member"))?;
    Ok(start..end)
}
fn present<'a>(
    attributes: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, CodecError> {
    attributes
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| CodecError::malformed(format!("GUI attribute {name} is absent")))
}

fn required<'a>(
    attributes: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, CodecError> {
    attributes
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| CodecError::malformed(format_args!("GUI element has no {name}")))
}

fn selected_attributes(
    ctx: &DecodeContext<'_>,
    xml: &XmlSource<'_>,
    index: usize,
    names: &[&str],
) -> Result<BTreeMap<String, String>, CodecError> {
    let mut values = BTreeMap::new();
    for name in names {
        if let Some(value) = xml.attribute(ctx, index, name)? {
            values.insert((*name).to_owned(), value);
        }
    }
    Ok(values)
}

fn descendant_count(xml: &XmlSource<'_>, index: usize) -> usize {
    xml.elements()
        .iter()
        .skip(index + 1)
        .take_while(|element| element.parent().is_some_and(|parent| parent >= index))
        .count()
}
