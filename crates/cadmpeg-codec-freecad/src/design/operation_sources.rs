// SPDX-License-Identifier: Apache-2.0
//! Selected link and placement facts used by the shared operation selector.

use super::*;

#[derive(Default)]
pub(super) struct Selected {
    pub(super) properties: BTreeMap<String, PropertyRecord>,
    pub(super) arities: BTreeMap<String, usize>,
}

pub(super) fn selected(
    reader: &crate::property_reader::PropertyReader<'_, '_>,
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
    objects: &[ObjectRecord],
) -> Result<Selected, CodecError> {
    if !reader.has_feature_sources() {
        return Ok(Selected::default());
    }
    let names: &[&str] = if is_body(&object.type_name) {
        &["Group", "Model", "Tip"]
    } else if is_datum(&object.type_name) {
        if property(properties, "Placement").is_some() {
            &["Placement"]
        } else {
            &["AttachmentOffset"]
        }
    } else if object.type_name == "PartDesign::FeatureBase" {
        &["BaseFeature"]
    } else if matches!(
        object.type_name.as_str(),
        "Part::Compound" | "Part::Compound2"
    ) {
        if property(properties, "Links").is_some() {
            &["Links"]
        } else {
            &["Shape"]
        }
    } else if matches!(object.type_name.as_str(), "Part::Refine" | "Part::Reverse") {
        &["Source"]
    } else {
        &[]
    };
    let body_headers_only = is_body(&object.type_name)
        && (!body_membership_carrier_is_valid(properties)
            || property(properties, "Tip")
                .is_some_and(|property| property.type_name != "App::PropertyLink"));
    let mut selected = Selected::default();
    for name in names {
        if let Some(property) = property(properties, name) {
            let arity = matches!(
                object.type_name.as_str(),
                "Part::Compound" | "Part::Compound2" | "Part::Refine" | "Part::Reverse"
            ) && matches!(*name, "Links" | "Source");
            let header_only = body_headers_only
                || arity
                || *name == "Shape"
                || (is_datum(&object.type_name) && property.type_name != "App::PropertyPlacement")
                || (object.type_name == "PartDesign::FeatureBase"
                    && property.type_name != "App::PropertyLink");
            let current = if header_only {
                header(reader, property)?
            } else {
                fact(reader, property, objects)?
            };
            if arity {
                selected
                    .arities
                    .insert(current.id.clone(), link_arity(reader, &current)?);
            }
            selected.properties.insert(current.id.clone(), current);
        }
    }
    Ok(selected)
}

fn header(
    reader: &crate::property_reader::PropertyReader<'_, '_>,
    property: &PropertyRecord,
) -> Result<PropertyRecord, CodecError> {
    let mut current = property.clone();
    if let Some((name, kind)) = reader.feature_header(property)? {
        current.name = name;
        current.type_name = kind;
    }
    Ok(current)
}

fn link_arity(
    reader: &crate::property_reader::PropertyReader<'_, '_>,
    property: &PropertyRecord,
) -> Result<usize, CodecError> {
    use crate::persistence::LinkGrammar;
    if let Some(grammar) = crate::persistence::link_grammar(&property.type_name) {
        let Some(root) = property
            .values()
            .iter()
            .find(|value| value.parent.is_none())
        else {
            reader.unavailable(property)?;
            return Err(CodecError::malformed("operation link arity is unavailable"));
        };
        let counted = matches!(
            grammar,
            LinkGrammar::LinkList | LinkGrammar::LinkSubList | LinkGrammar::XLinkSubList
        );
        let fields = if counted { &["count"][..] } else { &[][..] };
        let source = reader
            .feature_fields(property, root.order, fields)?
            .ok_or_else(|| CodecError::malformed("operation link arity has no supplier"))?;
        if counted {
            source
                .attributes
                .get("count")
                .and_then(|count| count.parse::<usize>().ok())
                .filter(|count| *count == property.links().len())
                .ok_or_else(|| {
                    CodecError::malformed("operation link arity differs from admitted slots")
                })
        } else {
            Ok(1)
        }
    } else if crate::persistence::document_properties::value_parts::links::expression_container(
        &property.type_name,
    ) {
        if matches!(&property.body, Some(crate::native::PropertyBody::Persisted { values, .. }) if values.has_missing())
        {
            reader.unavailable(property)?;
            return Err(CodecError::malformed(
                "operation expression-link arity is unavailable",
            ));
        }
        let mut count = 0_usize;
        for row in property.values().iter().filter(|row| row.tag == "XLink") {
            reader
                .feature_fields(property, row.order, &[])?
                .ok_or_else(|| {
                    CodecError::malformed("operation expression link has no supplier")
                })?;
            count = count.checked_add(1).ok_or_else(|| {
                CodecError::malformed("operation expression-link arity overflows")
            })?;
        }
        Ok(count)
    } else {
        Ok(0)
    }
}

fn fact(
    reader: &crate::property_reader::PropertyReader<'_, '_>,
    property: &PropertyRecord,
    objects: &[ObjectRecord],
) -> Result<PropertyRecord, CodecError> {
    let mut current = header(reader, property)?;
    if property.body.is_none() {
        reader.unavailable(property)?;
        return Err(CodecError::malformed(
            "operation selector value is unavailable",
        ));
    }
    if crate::persistence::link_grammar(&property.type_name).is_some()
        && !property.values().iter().any(|value| value.parent.is_none())
    {
        reader.unavailable(property)?;
        return Err(CodecError::malformed(
            "operation selector link root is unavailable",
        ));
    }
    let mut supplied = false;
    let mut rows = Vec::new();
    for admitted in property.values() {
        let fields: &[&str] = match admitted.tag.as_str() {
            "PropertyPlacement" if admitted.attributes.contains_key("A") => {
                &["Px", "Py", "Pz", "Ox", "Oy", "Oz", "A"]
            }
            "PropertyPlacement" => &["Px", "Py", "Pz", "Q0", "Q1", "Q2", "Q3"],
            "LinkList" | "LinkSubList" | "XLinkSubList" => &["count"],
            "Link"
                if property.type_name == "App::PropertyLinkSubList"
                    && admitted.attributes.contains_key("shadowed") =>
            {
                &["obj", "shadowed"]
            }
            "Link" if property.type_name == "App::PropertyLinkSubList" => &["obj", "sub"],
            "Link" => &["value"],
            "LinkSub" => &["value", "count"],
            "XLink" if admitted.attributes.contains_key("sub") => {
                &["name", "file", "sub", "shadowed"]
            }
            "XLink" => &["name", "file", "count"],
            "Sub" if admitted.attributes.contains_key("shadowed") => &["shadowed"],
            "Sub" => &["value"],
            // Shape presence selects the existing stored-geometry fallback;
            // its actual header and root are checked by feature_fields.
            _ => &[],
        };
        let mut row = admitted.clone();
        if let Some(source) = reader.feature_fields(property, admitted.order, fields)? {
            supplied = true;
            current.name = source.name;
            current.type_name = source.kind;
            for field in fields {
                row.attributes.remove(*field);
            }
            row.attributes.extend(source.attributes);
        }
        rows.push(row);
    }
    if !supplied {
        return Ok(current);
    }
    let links = if crate::persistence::link_grammar(&current.type_name).is_some() {
        links(&current, &rows, objects)?
    } else {
        current.links().to_vec()
    };
    let count = match &property.body {
        Some(crate::native::PropertyBody::Persisted { values, .. }) => values.slot_count(),
        _ => 0,
    };
    current.body = Some(crate::native::PropertyBody::Persisted {
        values: crate::native::ValueSlots::new(rows, count).map_err(CodecError::malformed)?,
        links,
        side_entries: property.side_entries().to_vec(),
    });
    Ok(current)
}

fn links(
    property: &PropertyRecord,
    rows: &[crate::native::ValueRecord],
    objects: &[ObjectRecord],
) -> Result<Vec<Option<crate::native::LinkTarget>>, CodecError> {
    use crate::persistence::LinkGrammar;
    let grammar = crate::persistence::link_grammar(&property.type_name)
        .ok_or_else(|| CodecError::malformed("operation link grammar is absent"))?;
    let list = matches!(
        grammar,
        LinkGrammar::LinkList | LinkGrammar::LinkSubList | LinkGrammar::XLinkSubList
    );
    let count = if list {
        rows.iter()
            .find(|row| row.parent.is_none())
            .and_then(|row| row.attributes.get("count"))
            .and_then(|count| count.parse::<usize>().ok())
            .filter(|count| *count == property.links().len())
            .ok_or_else(|| {
                CodecError::malformed("operation link count differs from admitted slots")
            })?
    } else {
        1
    };
    let mut targets = Vec::new();
    for index in 0..count {
        let row = if list {
            rows.iter().find(|row| {
                row.link_index == Some(index) && matches!(row.tag.as_str(), "Link" | "XLink")
            })
        } else {
            rows.iter().find(|row| row.parent.is_none())
        };
        targets.push(
            row.map(|row| target(grammar, row, rows, objects))
                .transpose()?
                .flatten(),
        );
    }
    Ok(targets)
}

fn target(
    grammar: crate::persistence::LinkGrammar,
    row: &crate::native::ValueRecord,
    rows: &[crate::native::ValueRecord],
    objects: &[ObjectRecord],
) -> Result<Option<crate::native::LinkTarget>, CodecError> {
    use crate::persistence::LinkGrammar;
    let (key, external) = match grammar {
        LinkGrammar::Link | LinkGrammar::LinkList | LinkGrammar::LinkSub => ("value", false),
        LinkGrammar::LinkSubList => ("obj", false),
        LinkGrammar::XLink | LinkGrammar::XLinkSubList => ("name", true),
    };
    let raw = row
        .attributes
        .get(key)
        .ok_or_else(|| CodecError::malformed("operation link target is absent"))?;
    let document = external
        .then(|| row.attributes.get("file"))
        .flatten()
        .filter(|file| !file.is_empty())
        .cloned();
    let object = if document.is_none() && !raw.is_empty() {
        objects
            .iter()
            .find(|object| object.name() == raw)
            .map(|object| object.id().clone())
            .ok_or_else(|| CodecError::malformed("operation link target has no held declaration"))?
    } else {
        raw.clone()
    };
    let subelement = |row: &crate::native::ValueRecord, key| {
        row.attributes
            .get("shadowed")
            .or_else(|| row.attributes.get(key))
            .cloned()
            .ok_or_else(|| CodecError::malformed("operation link subelement is absent"))
    };
    let subelements = if row.attributes.contains_key("sub") {
        vec![subelement(row, "sub")?]
    } else if matches!(
        grammar,
        LinkGrammar::LinkSub | LinkGrammar::XLink | LinkGrammar::XLinkSubList
    ) {
        rows.iter()
            .filter(|child| child.parent == Some(row.order))
            .map(|child| subelement(child, "value"))
            .collect::<Result<_, _>>()?
    } else {
        Vec::new()
    };
    if row
        .attributes
        .get("count")
        .is_some_and(|count| count.parse::<usize>().ok() != Some(subelements.len()))
    {
        return Err(CodecError::malformed(
            "operation link subelement count differs from its supplied fields",
        ));
    }
    crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
        document,
        document_attribute: external.then(|| "file".to_owned()),
        object: Some(object),
        subelements,
    })
    .map_err(CodecError::malformed)
}
