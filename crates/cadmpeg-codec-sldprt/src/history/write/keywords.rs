// SPDX-License-Identifier: Apache-2.0
//! The Keywords member of a native history: the retained source text, or a
//! regenerated text that the reader reads back, with what it leaves out
//! charged.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashSet;

use super::xml::{valid_xml_name, xml_char};
use crate::loss::{NamedCount, SldprtLossCode};
use crate::records::{
    Configuration, Feature, FeatureContent, FeatureHistory, HistoryContent, HistoryElement,
    HistoryElementContent,
};

/// A history as the source document held it: the record, its member name and
/// the member payload.
pub(crate) type RetainedHistory = (FeatureHistory, String, Vec<u8>);

/// Attributes the writer writes itself on each element it generates. A
/// property with one of these keys is left out: it would repeat the attribute,
/// or the reader would read it as the field.
const ROOT_ATTRIBUTES: &[&str] = &["Name"];
const CONFIGURATION_ATTRIBUTES: &[&str] = &["Name", "SourceIndex", "Material"];
const FEATURE_ATTRIBUTES: &[&str] = &["id", "Name", "Type", "Suppressed"];
const DIMENSION_ATTRIBUTES: &[&str] = &["Name"];

const ROOT: &str = "the Keywords root";

/// The bytes of the Keywords member `section` for `history`, or `None` when
/// no member is written.
///
/// An unchanged history writes its retained source member. A changed one is
/// regenerated: a name XML cannot hold, or one the writer writes itself, is
/// left out, a feature tag the reader takes for another record is written as
/// `Feature`, and a character XML cannot hold is left out of its value. When
/// the regenerated text cannot be built or does not read back, the retained
/// member, else `source_block` (the retained source block of the member), is
/// written in its place, and without either no member is written. Every name,
/// character or edit the bytes do not hold is charged to `losses`.
pub(crate) fn history_member(
    history: &FeatureHistory,
    section: &str,
    retained: &[RetainedHistory],
    source_block: Option<&[u8]>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<Vec<u8>>, CodecError> {
    let mut originals = retained
        .iter()
        .filter(|(original, name, _)| original.id == history.id && name == section);
    let original = originals.next();
    if originals.next().is_some() {
        return Err(CodecError::NotImplemented(
            "ambiguous retained SLDPRT history".into(),
        ));
    }
    if let Some((original, _, payload)) = original {
        if original == history {
            return Ok(Some(payload.clone()));
        }
    }
    let source = original
        .map(|(_, _, payload)| payload.as_slice())
        .or(source_block);
    let mut text = KeywordsXml::new(section);
    let built = write_history(&mut text, history)
        .map(|()| text)
        .map_err(|error| format!("cannot be built: {error}"));
    Ok(checked_member(section, built, source, losses))
}

/// The regenerated text when it reads back, else `source`, charging what is
/// not written.
fn checked_member(
    section: &str,
    built: Result<KeywordsXml<'_>, String>,
    source: Option<&[u8]>,
    losses: &mut Vec<LossNote>,
) -> Option<Vec<u8>> {
    let refusal = match built {
        Ok(text) => match read_back(&text.out) {
            Ok(()) => return Some(text.finish(losses).into_bytes()),
            Err(error) => format!("does not read back: {error}"),
        },
        Err(refusal) => refusal,
    };
    let written = match source {
        Some(_) => "the source member is written in its place, so the edits of the history are not written",
        None => "no source member is held, so the member is not written and the history is not in the file",
    };
    losses.push(
        SldprtLossCode::WriterHistoryRegenerationRefused.note(format!(
            "Keywords member {section}: the regenerated history {refusal}; {written}"
        )),
    );
    source.map(<[u8]>::to_vec)
}

/// Parses `text` as the decoder parses a Keywords member. The root is written
/// as `Keywords`, so only the parse can refuse.
fn read_back(text: &str) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(text.as_bytes(), &arena, &DecodePolicy::desktop())?;
    ctx.parse_xml(text, "read back a regenerated Keywords text")
        .map(drop)
}

fn write_history(text: &mut KeywordsXml<'_>, history: &FeatureHistory) -> Result<(), CodecError> {
    crate::writer::validate_feature_graph(&history.features)?;
    text.open("Keywords");
    if let Some(name) = &history.part_name {
        text.attribute(ROOT, "Name", name);
    }
    for (name, value) in &history.properties {
        text.property(ROOT, ROOT_ATTRIBUTES, name.as_str(), value);
    }
    text.out.push('>');
    let mut roots = history
        .features
        .iter()
        .filter(|feature| feature.tree_parent.is_none())
        .collect::<Vec<_>>();
    roots.sort_by_key(|feature| feature.ordinal);
    let mut emitted_configurations = HashSet::new();
    let mut emitted_features = HashSet::new();
    for item in &history.content {
        match item {
            HistoryContent::Configuration(id) => {
                if let Some(configuration) = history
                    .configurations
                    .iter()
                    .find(|configuration| configuration.id == *id)
                {
                    write_configuration(text, configuration);
                    emitted_configurations.insert(configuration.id.as_str());
                }
            }
            HistoryContent::Feature(id) => {
                if let Some(feature) = roots.iter().find(|feature| feature.id == *id) {
                    write_feature(text, feature, &history.features, true);
                    emitted_features.insert(feature.id.as_str());
                }
            }
            HistoryContent::Text(value) => text.text(ROOT, value),
            HistoryContent::Element(element) => write_element(text, element, None),
        }
    }
    for configuration in &history.configurations {
        if emitted_configurations.insert(configuration.id.as_str()) {
            write_configuration(text, configuration);
        }
    }
    for feature in roots {
        if emitted_features.insert(feature.id.as_str()) {
            write_feature(text, feature, &history.features, true);
        }
    }
    text.end("Keywords");
    Ok(())
}

fn write_configuration(text: &mut KeywordsXml<'_>, configuration: &Configuration) {
    let owner = configuration.id.as_str();
    text.open("Configuration");
    text.attribute(owner, "Name", &configuration.name);
    if let Some(source_index) = configuration.source_index {
        text.attribute(owner, "SourceIndex", &source_index.to_string());
    }
    if let Some(material) = &configuration.material {
        text.attribute(owner, "Material", material);
    }
    for (name, value) in &configuration.properties {
        text.property(owner, CONFIGURATION_ATTRIBUTES, name.as_str(), value);
    }
    text.out.push_str("/>");
}

fn write_feature(text: &mut KeywordsXml<'_>, feature: &Feature, features: &[Feature], root: bool) {
    let owner = feature.id.as_str();
    let tag = text.feature_tag(feature, root);
    text.open(tag);
    if let Some(id) = feature.source_id {
        text.attribute(owner, "id", &String::from(id));
    }
    text.attribute(owner, "Name", &feature.name);
    text.attribute(owner, "Type", &feature.kind);
    if feature.suppressed {
        text.attribute(owner, "Suppressed", "true");
    }
    for (name, value) in &feature.properties {
        text.property(owner, FEATURE_ATTRIBUTES, name.as_str(), value);
    }
    text.out.push('>');
    let write_dimension = |text: &mut KeywordsXml<'_>, name: &str, value: &str| {
        let owner = format!("{owner} dimension {name:?}");
        text.open("Dimension");
        text.attribute(&owner, "Name", name);
        if let Some(properties) = feature.dimension_properties.get(name) {
            for (property, value) in properties {
                text.property(&owner, DIMENSION_ATTRIBUTES, property.as_str(), value);
            }
        }
        text.out.push('>');
        text.text(&owner, value);
        text.end("Dimension");
    };
    let mut children = features
        .iter()
        .filter(|child| {
            child.tree_parent_record_id() == Some(feature.id.as_str())
                || (child.tree_parent_record_id().is_none()
                    && child.parent_source_id() == feature.source_id
                    && feature.source_id.is_some())
        })
        .collect::<Vec<_>>();
    children.sort_by_key(|child| child.ordinal);
    let mut emitted_dimensions = HashSet::new();
    let mut emitted_children = HashSet::new();
    if feature.content.is_empty() {
        for (name, value) in &feature.parameters {
            write_dimension(text, name.as_str(), value);
            emitted_dimensions.insert(name.as_str());
        }
        if let Some(value) = &feature.text {
            text.text(owner, value);
        }
    } else {
        for item in &feature.content {
            match item {
                FeatureContent::Dimension(name) => {
                    if let Some(value) = feature.parameters.get(name.as_str()) {
                        write_dimension(text, name, value);
                        emitted_dimensions.insert(name.as_str());
                    }
                }
                FeatureContent::Feature(id) => {
                    if let Some(child) = children.iter().find(|child| child.id == *id) {
                        write_feature(text, child, features, false);
                        emitted_children.insert(child.id.as_str());
                    }
                }
                FeatureContent::Text(value) => text.text(owner, value),
            }
        }
    }
    for (name, value) in &feature.parameters {
        if emitted_dimensions.insert(name.as_str()) {
            write_dimension(text, name.as_str(), value);
        }
    }
    for child in children {
        if emitted_children.insert(child.id.as_str()) {
            write_feature(text, child, features, false);
        }
    }
    text.end(tag);
}

/// Writes `element` with its subtree, or leaves it out when XML cannot hold
/// its tag. `parent` names the element it sits in.
fn write_element(text: &mut KeywordsXml<'_>, element: &HistoryElement, parent: Option<&str>) {
    let named = match element.attributes.get("Name") {
        Some(name) => format!("{} {name:?}", element.tag),
        None => element.tag.clone(),
    };
    let owner = match parent {
        Some(parent) => format!("{parent} > {named}"),
        None => format!("the {named} element"),
    };
    if !valid_xml_name(&element.tag) {
        text.elements.push(&owner);
        return;
    }
    text.open(&element.tag);
    for (name, value) in &element.attributes {
        text.property(&owner, &[], name.as_str(), value);
    }
    if element.content.is_empty() {
        text.out.push_str("/>");
        return;
    }
    text.out.push('>');
    for item in &element.content {
        match item {
            HistoryElementContent::Element(child) => write_element(text, child, Some(&owner)),
            HistoryElementContent::Text(value) => text.text(&owner, value),
        }
    }
    text.end(&element.tag);
}

/// A Keywords text being written, with what it leaves out.
struct KeywordsXml<'a> {
    section: &'a str,
    out: String,
    /// Whether the last thing written is text. The next text then follows an
    /// empty comment, so the reader reads the two as two text items.
    after_text: bool,
    /// Features written under `Feature` instead of their tag.
    tags: NamedCount,
    /// History elements left out with their subtrees.
    elements: NamedCount,
    /// Attributes left out.
    attributes: NamedCount,
    /// Values characters are left out of, and the characters.
    values: NamedCount,
    characters: usize,
}

impl<'a> KeywordsXml<'a> {
    fn new(section: &'a str) -> Self {
        Self {
            section,
            out: String::new(),
            after_text: false,
            tags: NamedCount::default(),
            elements: NamedCount::default(),
            attributes: NamedCount::default(),
            values: NamedCount::default(),
            characters: 0,
        }
    }

    /// The tag `feature` is written under: its own, unless XML cannot hold it
    /// or the reader reads an element of that tag as another record.
    fn feature_tag<'f>(&mut self, feature: &'f Feature, root: bool) -> &'f str {
        let tag = feature.xml_tag.as_str();
        let other_record = matches!(tag, "Keywords" | "Configuration" | "Dimension")
            || (root && tag == "Application");
        if valid_xml_name(tag) && !other_record {
            return tag;
        }
        self.tags.push(&format!("{} <{tag}>", feature.id));
        "Feature"
    }

    fn open(&mut self, tag: &str) {
        self.out.push('<');
        self.out.push_str(tag);
        self.after_text = false;
    }

    fn end(&mut self, tag: &str) {
        self.out.push_str("</");
        self.out.push_str(tag);
        self.out.push('>');
        self.after_text = false;
    }

    /// An attribute the writer names itself.
    fn attribute(&mut self, owner: &str, name: &str, value: &str) {
        self.out.push(' ');
        self.out.push_str(name);
        self.out.push_str("=\"");
        self.value(owner, name, value, true);
        self.out.push('"');
    }

    /// A stored attribute: left out when XML cannot hold its name, when it
    /// would declare a namespace, or when `owned` holds it.
    fn property(&mut self, owner: &str, owned: &[&str], name: &str, value: &str) {
        if valid_xml_name(name) && name != "xmlns" && !owned.contains(&name) {
            self.attribute(owner, name, value);
        } else {
            self.attributes.push(&format!("{owner} {name:?}"));
        }
    }

    fn text(&mut self, owner: &str, value: &str) {
        if value.is_empty() {
            return;
        }
        if self.after_text {
            self.out.push_str("<!---->");
        }
        self.value(owner, "text", value, false);
        self.after_text = true;
    }

    /// Writes `value` escaped, leaving out each character XML cannot hold.
    fn value(&mut self, owner: &str, what: &str, value: &str, attribute: bool) {
        let mut left_out = 0;
        for character in value.chars() {
            match character {
                '&' => self.out.push_str("&amp;"),
                '<' => self.out.push_str("&lt;"),
                '"' if attribute => self.out.push_str("&quot;"),
                '>' if !attribute => self.out.push_str("&gt;"),
                '\r' => self.out.push_str("&#13;"),
                '\n' if attribute => self.out.push_str("&#10;"),
                '\t' if attribute => self.out.push_str("&#9;"),
                character if xml_char(character) => self.out.push(character),
                _ => left_out += 1,
            }
        }
        if left_out != 0 {
            self.characters += left_out;
            self.values.push(&format!("{owner} {what}"));
        }
    }

    /// The text, after charging what it leaves out.
    fn finish(self, losses: &mut Vec<LossNote>) -> String {
        let section = self.section;
        if !self.tags.is_empty() {
            losses.push(SldprtLossCode::WriterHistoryNameNotWritten.note(format!(
                "Keywords member {section}: {} feature tag(s) are not XML element names or name another record, so the features are written as Feature elements and keep their kind in Type: {}",
                self.tags.count(),
                self.tags
            )));
        }
        if !self.elements.is_empty() {
            losses.push(SldprtLossCode::WriterHistoryNameNotWritten.note(format!(
                "Keywords member {section}: {} element(s) whose tag is not an XML element name are not written, with their subtrees: {}",
                self.elements.count(),
                self.elements
            )));
        }
        if !self.attributes.is_empty() {
            losses.push(SldprtLossCode::WriterHistoryNameNotWritten.note(format!(
                "Keywords member {section}: {} attribute(s) whose key is not an XML attribute name, or is one the writer writes itself, are not written: {}",
                self.attributes.count(),
                self.attributes
            )));
        }
        if self.characters != 0 {
            losses.push(SldprtLossCode::WriterHistoryCharactersNotWritten.note(format!(
                "Keywords member {section}: {} character(s) an XML document cannot hold are left out of {} value(s): {}",
                self.characters,
                self.values.count(),
                self.values
            )));
        }
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::{checked_member, KeywordsXml};
    use crate::loss::SldprtLossCode;

    fn built(text: &str) -> KeywordsXml<'static> {
        let mut written = KeywordsXml::new("Contents/Keywords");
        written.out.push_str(text);
        written
    }

    #[test]
    fn a_regenerated_text_the_reader_refuses_is_replaced_by_the_source_member() {
        let source = b"<Keywords Name=\"Part\"/>";
        for regenerated in [
            Ok(built("<Keywords><Feature a b=\"1\"></Feature></Keywords>")),
            Err("cannot be built: feature parent cycle".to_owned()),
        ] {
            let mut losses = Vec::new();
            let member =
                checked_member("Contents/Keywords", regenerated, Some(source), &mut losses);
            assert_eq!(member.as_deref(), Some(&source[..]));
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code,
                SldprtLossCode::WriterHistoryRegenerationRefused.kind()
            );
            assert!(
                losses[0]
                    .message
                    .contains("the source member is written in its place"),
                "{}",
                losses[0].message
            );
        }
    }

    #[test]
    fn a_refused_text_without_a_source_member_writes_no_member() {
        let mut losses = Vec::new();
        let member = checked_member(
            "Contents/Keywords-0",
            Ok(built("<Keywords><a:b/></Keywords>")),
            None,
            &mut losses,
        );
        assert!(member.is_none());
        assert_eq!(losses.len(), 1);
        let message = &losses[0].message;
        assert!(
            message.contains("Contents/Keywords-0: the regenerated history does not read back")
                && message.contains("the member is not written"),
            "{message}"
        );
    }

    #[test]
    fn a_text_that_reads_back_is_written_with_its_charges() {
        let mut text = built("");
        text.open("Keywords");
        text.property("the Keywords root", &["Name"], "Na me", "x");
        text.out.push('>');
        text.text("the Keywords root", "a\u{1}b");
        text.end("Keywords");
        let mut losses = Vec::new();
        let member = checked_member("Contents/Keywords", Ok(text), None, &mut losses).unwrap();
        assert_eq!(member, b"<Keywords>ab</Keywords>");
        assert_eq!(
            losses
                .iter()
                .map(|loss| loss.code.clone())
                .collect::<Vec<_>>(),
            [
                SldprtLossCode::WriterHistoryNameNotWritten.kind(),
                SldprtLossCode::WriterHistoryCharactersNotWritten.kind(),
            ]
        );
    }
}
