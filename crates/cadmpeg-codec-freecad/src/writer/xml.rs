// SPDX-License-Identifier: Apache-2.0
//! Structured XML output. Retained XML is never read for content or layout.

mod enumerations;
mod expression_links;
mod reference_fields;

use crate::native::{
    DynamicPropertyMeta, GuiPropertyRecord, PropertyBody, PropertyFamily, PropertyRecord,
    ValueRecord,
};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

pub(crate) fn escaped(value: &str) -> String {
    let mut result = String::new();
    escaped_into(value, &mut result, true);
    result
}

// Only generated synthesis XML uses this numeric entity. Literal source text
// spelling it has its ampersand escaped, so accounting can distinguish an
// actual reduction from an authored replacement character.
const REPLACEMENT: &str = "&#xFFFD;";

fn escaped_into(value: &str, output: &mut String, attribute: bool) {
    for character in value.chars() {
        if matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
        {
            super::escape_xml(character.encode_utf8(&mut [0; 4]), output, attribute);
        } else {
            output.push_str(REPLACEMENT);
        }
    }
}

/// Report reductions in generated XML, including their property/element path.
/// XML-forbidden characters are reduced before intermediate XML is parsed.
pub(super) fn report_reductions(
    value: &str,
    file: &str,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<bool, CodecError> {
    if !value.contains(REPLACEMENT) {
        return Ok(false);
    }
    let document = roxmltree::Document::parse(value).map_err(CodecError::malformed)?;
    for node in document.descendants().filter(roxmltree::Node::is_element) {
        // Restrict counting to this element's opening tag and direct text;
        // descendant properties produce their own precise diagnostic.
        let range = node.range();
        let raw = &value[range.clone()];
        let opening = raw.find('>').map_or(raw, |end| &raw[..=end]);
        let count = opening.matches(REPLACEMENT).count()
            + node
                .children()
                .filter(roxmltree::Node::is_text)
                .map(|text| value[text.range()].matches(REPLACEMENT).count())
                .sum::<usize>();
        if count == 0 {
            continue;
        }
        let path = node
            .ancestors()
            .filter(roxmltree::Node::is_element)
            .map(|ancestor| {
                ancestor.attribute("name").map_or_else(
                    || ancestor.tag_name().name().to_owned(),
                    |name| format!("{}[{name}]", ancestor.tag_name().name()),
                )
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("/");
        losses.push(
            crate::loss::FreecadLossCode::ExportArenaNotTransferred.note(format!(
                "{file} {path}: {count} XML-forbidden character(s) replaced with U+FFFD"
            )),
        );
    }
    Ok(true)
}

pub(crate) fn element(tag: &str, attributes: &[(&str, String)]) -> String {
    let mut result = format!("<{tag}");
    for (key, value) in attributes {
        result.push(' ');
        result.push_str(key);
        result.push_str("=\"");
        result.push_str(&escaped(value));
        result.push('"');
    }
    result.push_str("/>");
    result
}

pub(crate) fn property(name: &str, kind: &str, content: &str) -> String {
    format!(
        "<Property name=\"{}\" type=\"{}\">{content}</Property>",
        escaped(name),
        escaped(kind)
    )
}

pub(crate) fn scalar(name: &str, kind: &str, tag: &str, value: impl ToString + Copy) -> String {
    property(name, kind, &element(tag, &[("value", value.to_string())]))
}

pub(crate) fn string(name: &str, value: &str) -> String {
    scalar(name, "App::PropertyString", "String", value)
}

fn valid_name(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ':')
        })
}

/// Emit the admitted typed value tree. Parent edges must point backward,
/// which proves that the traversal terminates and that each row has one owner.
pub(crate) fn native(property_record: &PropertyRecord) -> Result<Option<String>, CodecError> {
    native_named(property_record, &std::collections::HashMap::new())
}

pub(crate) fn validate(property_record: &PropertyRecord) -> Result<(), CodecError> {
    if !matches!(property_record.body, Some(PropertyBody::Persisted { .. })) {
        return Ok(());
    }
    let Some(xml) = native(property_record)? else {
        return Ok(());
    };
    validate_links(
        &property_record.id,
        &property_record.type_name,
        &xml,
        property_record.links(),
        has_omitted_value_parts(property_record),
    )
}

pub(crate) fn validate_gui(property: &GuiPropertyRecord) -> Result<(), CodecError> {
    if crate::persistence::link_grammar(&property.type_name).is_none()
        && !crate::persistence::document_properties::value_parts::links::expression_container(
            &property.type_name,
        )
    {
        if !property.links.is_empty() {
            return Err(CodecError::malformed(
                "non-link GUI property contains typed targets",
            ));
        }
        return Ok(());
    }
    let Some(xml) = native_gui(property)? else {
        return Ok(());
    };
    validate_links(
        &property.id,
        &property.type_name,
        &xml,
        &property.links,
        property.values.has_missing(),
    )
}

fn validate_links(
    id: &str,
    kind: &str,
    xml: &str,
    expected_links: &[Option<crate::native::LinkTarget>],
    partial: bool,
) -> Result<(), CodecError> {
    let parsed = roxmltree::Document::parse(&xml).map_err(CodecError::malformed)?;
    let mut links = crate::persistence::parse_link_targets(parsed.root_element(), kind)?;
    let expected: Vec<_> = expected_links
        .iter()
        .filter(|link| !partial || link.is_some())
        .cloned()
        .collect();
    if partial {
        links.retain(Option::is_some);
    }
    for (actual, expected) in links.iter_mut().zip(&expected) {
        if let (Some(actual), Some(expected)) = (actual, expected) {
            if actual.document().is_none() {
                if let Some(object) = actual.object() {
                    let native = crate::native::native_id("object", object);
                    if expected.object() == Some(native.as_str()) {
                        actual.set_object(
                            cadmpeg_core::text::NonBlankString::new(native).ok_or_else(|| {
                                CodecError::malformed("native object identity is blank")
                            })?,
                        );
                    }
                }
            }
        }
    }
    if links != expected {
        return Err(CodecError::malformed(format_args!(
            "property {} typed value or link graph disagrees",
            id
        )));
    }
    Ok(())
}

pub(crate) fn native_named(
    property_record: &PropertyRecord,
    names: &std::collections::HashMap<String, String>,
) -> Result<Option<String>, CodecError> {
    native_named_payloads(property_record, names, &std::collections::BTreeSet::new())
}

pub(crate) fn native_named_payloads(
    property_record: &PropertyRecord,
    names: &std::collections::HashMap<String, String>,
    payloads: &std::collections::BTreeSet<String>,
) -> Result<Option<String>, CodecError> {
    let Some(PropertyBody::Persisted {
        values,
        side_entries,
        ..
    }) = &property_record.body
    else {
        return Ok(
            matches!(property_record.body, Some(PropertyBody::Transient)).then(|| {
                format!(
                    "<_Property name=\"{}\" type=\"{}\"/>",
                    escaped(&property_record.name),
                    escaped(&property_record.type_name)
                )
            }),
        );
    };
    if values.is_empty()
        && (values.has_missing()
            || property_record.family == PropertyFamily::Unknown
                && !matches!(
                    property_record.type_name.as_str(),
                    "Part::PropertyShapeHistory" | "Part::PropertyShapeCache"
                ))
    {
        return Ok(None);
    }
    native_values(
        PersistedProperty {
            id: &property_record.id,
            name: &property_record.name,
            type_name: &property_record.type_name,
            family: property_record.family,
            status: property_record.status,
            values,
            side_entries,
            dynamic: property_record.dynamic.as_ref(),
            links: property_record.links(),
        },
        names,
        payloads,
    )
}

/// Serialize a GUI property's actual typed fields without a source-XML adapter.
pub(crate) fn native_gui(property: &GuiPropertyRecord) -> Result<Option<String>, CodecError> {
    native_gui_payloads(
        property,
        &std::collections::HashMap::new(),
        &std::collections::BTreeSet::new(),
    )
}

pub(crate) fn native_gui_payloads(
    property: &GuiPropertyRecord,
    names: &std::collections::HashMap<String, String>,
    payloads: &std::collections::BTreeSet<String>,
) -> Result<Option<String>, CodecError> {
    if property.values.is_empty()
        && !matches!(
            property.type_name.as_str(),
            "Part::PropertyShapeHistory" | "Part::PropertyShapeCache"
        )
    {
        return Ok(None);
    }
    native_values(
        PersistedProperty {
            id: &property.id,
            name: &property.name,
            type_name: &property.type_name,
            family: PropertyFamily::Unknown,
            status: property.status,
            values: &property.values,
            side_entries: &property.side_entries,
            dynamic: None,
            links: &property.links,
        },
        names,
        payloads,
    )
}

/// The semantic fields consumed by both application and GUI property writers.
struct PersistedProperty<'a> {
    id: &'a str,
    name: &'a str,
    type_name: &'a str,
    family: PropertyFamily,
    status: Option<u64>,
    values: &'a crate::native::ValueSlots,
    side_entries: &'a [String],
    dynamic: Option<&'a DynamicPropertyMeta>,
    links: &'a [Option<crate::native::LinkTarget>],
}

pub(super) fn expression_metadata_omitted(
    kind: &str,
    values: &crate::native::ValueSlots,
    links: &[Option<crate::native::LinkTarget>],
) -> Result<bool, CodecError> {
    let (current, references_omitted) =
        reference_fields::reconcile(kind, values, links, &std::collections::HashMap::new())?;
    expression_links::normalize(kind, &current)
        .map(|(_, omitted)| omitted || references_omitted || enumerations::omitted(kind, values))
}

fn native_values(
    property: PersistedProperty<'_>,
    names: &std::collections::HashMap<String, String>,
    payloads: &std::collections::BTreeSet<String>,
) -> Result<Option<String>, CodecError> {
    let (mut values, _) =
        reference_fields::reconcile(property.type_name, property.values, property.links, names)?;
    if !property.values.is_empty() && values.is_empty() {
        return Ok(None);
    }
    if !enumerations::normalize(
        property.type_name,
        &mut values,
        !property.values.has_missing(),
    ) {
        return Ok(None);
    }
    let mut omitted = std::collections::BTreeSet::new();
    for value in property.values {
        let incomplete_selector = matches!(value.tag.as_str(), "XLink" | "LinkSub")
            && value
                .attributes
                .get("count")
                .and_then(|count| count.parse::<usize>().ok())
                .is_some_and(|count| {
                    count
                        != property
                            .values
                            .iter()
                            .filter(|child| child.parent == Some(value.order))
                            .count()
                });
        if incomplete_selector || value.parent.is_some_and(|parent| omitted.contains(&parent)) {
            if value.parent.is_none() {
                return Ok(None);
            }
            omitted.insert(value.order);
        }
    }
    if !omitted.is_empty() {
        values = values
            .iter()
            .filter(|value| !omitted.contains(&value.order))
            .cloned()
            .collect::<Vec<_>>();
    }
    if crate::persistence::document_properties::value_parts::links::expression_container(
        property.type_name,
    ) {
        values = expression_links::normalize(property.type_name, &values)?.0;
    }
    let values = values.as_slice();
    let side_entries = property.side_entries;
    let dynamic = property.dynamic;
    // A filename carried in a typed property does not make its opaque payload
    // typed content. The owner must regenerate that payload before this path.
    if (property.type_name.contains("PropertyPythonObject")
        && !values.iter().any(|value| value.decoded_json.is_some()))
        || crate::persistence::opaque_python_values(property.type_name, values)
        || side_entries.iter().any(|entry| !payloads.contains(entry))
        || values.iter().any(|value| {
            value.attributes.iter().any(|(key, name)| {
                !name.is_empty()
                    && !payloads.contains(name)
                    && ((matches!(key.as_str(), "file" | "File")
                        && !crate::persistence::is_xlink_type(property.type_name)
                        && !(crate::persistence::document_properties::value_parts::links::expression_container(property.type_name) && value.tag == "XLink"))
                        || (property.family == crate::native::PropertyFamily::File
                            && matches!(key.as_str(), "name" | "Name")))
            })
        })
    {
        return Ok(None);
    }
    let content = value_content(property.id, values)?;
    let mut attributes = vec![
        ("name", property.name.to_owned()),
        ("type", property.type_name.to_owned()),
    ];
    if let Some(status) = property.status {
        attributes.push(("status", status.to_string()));
    }
    if let Some(dynamic) = dynamic {
        attributes.push(("group", dynamic.group.clone()));
        if let Some(value) = &dynamic.documentation {
            attributes.push(("doc", value.clone()));
        }
        if let Some(value) = dynamic.attributes {
            attributes.push(("attr", value.to_string()));
        }
        if let Some(value) = dynamic.read_only {
            attributes.push(("ro", u8::from(value).to_string()));
        }
        if let Some(value) = dynamic.hidden {
            attributes.push(("hide", u8::from(value).to_string()));
        }
    }
    let mut result = String::from("<Property");
    for (name, value) in attributes {
        result.push(' ');
        result.push_str(name);
        result.push_str("=\"");
        result.push_str(&escaped(&value));
        result.push('"');
    }
    result.push('>');
    result.push_str(&content);
    result.push_str("</Property>");
    Ok(Some(result))
}

/// Reports absent value parts without confusing an authored null link with damage.
pub(crate) fn has_omitted_value_parts(property: &PropertyRecord) -> bool {
    matches!(&property.body, Some(PropertyBody::Persisted { values, .. }) if values.has_missing())
}

fn collection_count_field(tag: &str) -> Option<&'static str> {
    if matches!(
        tag,
        "LinkList"
            | "LinkSubList"
            | "XLinkSubList"
            | "LinkSub"
            | "XLink"
            | "StringList"
            | "IntegerList"
            | "IntegerSet"
            | "FloatList"
            | "VectorList"
            | "PlacementList"
            | "ColorList"
            | "MaterialList"
            | "Map"
            | "ExpressionEngine"
            | "XLinks"
            | "ConstraintList"
            | "GeometryList"
            | "ShapeList"
            | "FilletEdges"
            | "CustomEnum"
            | "CustomEnumList"
            | "GeomFormatList"
            | "CosmeticVertexList"
            | "CosmeticEdgeList"
            | "CenterLineList"
    ) {
        return Some("count");
    }
    match tag {
        "Cells" | "ColumnInfo" | "RowInfo" => Some("Count"),
        "Points" => Some("PointsCount"),
        "Faces" => Some("FaceCount"),
        "Edges" => Some("EdgeCount"),
        "CLPoints" => Some("CLPointCount"),
        _ => None,
    }
}

fn value_content(id: &str, values: &[ValueRecord]) -> Result<String, CodecError> {
    let mut children = BTreeMap::<Option<usize>, Vec<usize>>::new();
    let mut held = std::collections::BTreeSet::new();
    let mut previous = None;
    for (index, value) in values.iter().enumerate() {
        if previous.is_some_and(|order| order >= value.order)
            || value.parent.is_some_and(|parent| !held.contains(&parent))
            || !valid_name(&value.tag)
            || value.attributes.keys().any(|name| !valid_name(name))
        {
            return Err(CodecError::malformed(format_args!(
                "property {} has an invalid typed value tree",
                id
            )));
        }
        held.insert(value.order);
        previous = Some(value.order);
        children.entry(value.parent).or_default().push(index);
    }
    let mut content = String::new();
    // A stack keeps deeply nested, valid properties off the Rust call stack.
    let mut stack: Vec<(usize, bool)> = children
        .get(&None)
        .into_iter()
        .flatten()
        .rev()
        .map(|index| (*index, false))
        .collect();
    while let Some((index, close)) = stack.pop() {
        let value: &ValueRecord = &values[index];
        if close {
            content.push_str("</");
            content.push_str(&value.tag);
            content.push('>');
            continue;
        }
        content.push('<');
        content.push_str(&value.tag);
        let mut attributes = value_attributes(value)?;
        if let Some(field) =
            collection_count_field(&value.tag).filter(|field| attributes.contains_key(*field))
        {
            let count = children.get(&Some(value.order)).map_or(0, |children| {
                children
                    .iter()
                    .filter(|index| match value.tag.as_str() {
                        "Cells" => values[**index].tag == "Cell",
                        "ExpressionEngine" => values[**index].tag == "Expression",
                        "XLinks" => values[**index].tag == "XLink",
                        _ => true,
                    })
                    .count()
            });
            attributes.insert(field.into(), count.to_string());
        }
        for (key, text) in &attributes {
            content.push(' ');
            content.push_str(key);
            content.push_str("=\"");
            content.push_str(&escaped(text));
            content.push('"');
        }
        content.push('>');
        if let Some(text) = &value.text {
            escaped_into(text, &mut content, false);
        }
        stack.push((index, true));
        if let Some(children) = children.get(&Some(value.order)) {
            stack.extend(children.iter().rev().map(|index| (*index, false)));
        }
    }
    Ok(content)
}

/// Native GUI state descendants have their own semantic owner, not a property wrapper.
pub(crate) fn gui_state_content(
    state: &crate::native::GuiStateRecord,
) -> Result<String, CodecError> {
    if state.values.iter().any(|value| {
        value
            .attributes
            .iter()
            .any(|(key, name)| matches!(key.as_str(), "file" | "File") && !name.is_empty())
    }) {
        return Ok(String::new());
    }
    value_content(&state.id, &state.values)
}

#[derive(Debug, Clone)]
enum GeometryEmission {
    Unknown,
    None {
        kind: String,
        extensions: Vec<String>,
        properties: BTreeMap<String, String>,
    },
    Present,
}

#[derive(Debug, Clone)]
pub(crate) struct Object {
    geometry: GeometryEmission,
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) properties: BTreeMap<String, String>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) extensions: Vec<String>,
    pub(crate) declaration_attributes: BTreeMap<String, String>,
}

impl Object {
    pub(crate) fn new(name: String, kind: &str, label: &str) -> Self {
        let mut object = Self::from_native(name, kind);
        object.set_text("Label", label);
        object
    }

    /// Native objects start with exactly the properties held by their typed records.
    pub(crate) fn from_native(name: String, kind: &str) -> Self {
        Self {
            geometry: GeometryEmission::Unknown,
            name,
            kind: kind.into(),
            properties: BTreeMap::new(),
            dependencies: Vec::new(),
            extensions: Vec::new(),
            declaration_attributes: BTreeMap::new(),
        }
    }

    pub(crate) fn set(&mut self, name: &str, property: String) {
        self.geometry = GeometryEmission::Unknown;
        self.properties.insert(name.into(), property);
    }

    pub(crate) fn mark_grouping_only(&mut self) {
        self.geometry = GeometryEmission::None {
            kind: self.kind.clone(),
            extensions: self.extensions.clone(),
            properties: self.properties.clone(),
        };
    }

    pub(crate) fn mark_geometry_written(&mut self) {
        self.geometry = GeometryEmission::Present;
    }

    pub(crate) fn geometry_empty(&self) -> bool {
        matches!(&self.geometry, GeometryEmission::None { kind, extensions, properties }
            if self.kind == *kind && self.extensions == *extensions && self.properties == *properties)
    }

    pub(crate) fn set_text(&mut self, name: &str, value: &str) {
        self.set_metadata(name, string(name, value));
    }

    pub(crate) fn set_boolean(&mut self, name: &str, value: bool) {
        self.set_metadata(
            name,
            scalar(name, "App::PropertyBool", "Bool", u8::from(value)),
        );
    }

    pub(crate) fn set_projection_text(&mut self, name: &str, value: &str) {
        self.projection_metadata(
            name,
            "App::PropertyString",
            &element("String", &[("value", value.to_owned())]),
        );
    }

    pub(crate) fn set_projection_boolean(&mut self, name: &str, value: bool) {
        self.projection_metadata(
            name,
            "App::PropertyBool",
            &element("Bool", &[("value", u8::from(value).to_string())]),
        );
    }

    pub(crate) fn set_projection_texts(&mut self, name: &str, values: &[String]) {
        let rows = values
            .iter()
            .map(|value| element("String", &[("value", value.clone())]))
            .collect::<String>();
        self.projection_metadata(
            name,
            "App::PropertyStringList",
            &format!("<StringList count=\"{}\">{rows}</StringList>", values.len()),
        );
    }

    fn set_metadata(&mut self, name: &str, property: String) {
        if let GeometryEmission::None { properties, .. } = &mut self.geometry {
            properties.insert(name.into(), property.clone());
        }
        self.properties.insert(name.into(), property);
    }

    fn projection_metadata(&mut self, name: &str, kind: &str, content: &str) {
        self.set_metadata(
            name,
            property(name, kind, content).replacen(
                "<Property ",
                "<Property group=\"CADIR\" hide=\"1\" ",
                1,
            ),
        );
    }
}

pub(crate) fn document(
    objects: &[Object],
    properties: &BTreeMap<String, String>,
    schema: &str,
) -> String {
    crate::builder::document_xml(objects, properties, schema)
}

#[cfg(test)]
mod tests;

/// Derive wire attributes from current canonical values before validating or
/// emitting them. Python's encoded attributes are not canonical authorities.
pub(super) fn value_attributes(
    value: &ValueRecord,
) -> Result<BTreeMap<String, String>, CodecError> {
    let mut attributes = value.attributes.clone();
    if let Some(json) = &value.decoded_json {
        use base64::Engine;
        if value.tag != "Python" {
            return Err(CodecError::malformed(
                "decoded JSON value must use a Python value element",
            ));
        }
        attributes.insert(
            "value".into(),
            base64::engine::general_purpose::STANDARD
                .encode(serde_json::to_vec(&json.value).map_err(CodecError::malformed)?),
        );
        attributes.insert("encoded".into(), "yes".into());
        attributes.insert("json".into(), "yes".into());
    }
    Ok(attributes)
}
