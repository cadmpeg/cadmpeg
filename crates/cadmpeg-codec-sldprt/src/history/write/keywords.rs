// SPDX-License-Identifier: Apache-2.0
//! The Keywords member of a native history: the retained source text, or a
//! regenerated text that the decoder reads back from the written file. The
//! names and texts of the history are made writable before it gets here
//! (`writer::keywords_text`); this module escapes.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashSet;

use crate::loss::SldprtLossCode;
use crate::records::{
    Configuration, Feature, FeatureContent, FeatureHistory, HistoryContent, HistoryElement,
    HistoryElementContent,
};

/// A history as the source document held it: the record, its member name and
/// the member payload.
pub(crate) type RetainedHistory = (FeatureHistory, String, Vec<u8>);

/// What precedes the root element of a generated Keywords member with no
/// source member: lead byte `86`, the XML declaration and CRLF, a form of a
/// stored member (sldprt.md §2).
const DEFAULT_PREFIX: &[u8] = b"\x86<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n";

/// The bytes of the Keywords member `section` for `history`, or `None` when
/// no member is generated. After `None` with a retained block that holds
/// bytes, the caller writes the retained blocks of `section` as stored and no
/// generated member of `section`.
///
/// An unchanged history writes its retained source member. A changed one is
/// regenerated after the lead byte and XML declaration of its source member,
/// else of its one retained source block, else [`DEFAULT_PREFIX`]. When the
/// regenerated text cannot be built or the decoder would not read it back
/// from the written file, the source member or the one retained block is
/// written in its place; with two or more retained blocks no member is
/// generated and the blocks are written as stored. `source_blocks` holds the
/// bytes of each retained source block of `section`, `None` for a block the
/// source fidelity holds no bytes of. What the bytes do not hold is charged to
/// `losses`.
pub(crate) fn history_member(
    history: &FeatureHistory,
    section: &str,
    retained: &[RetainedHistory],
    source_blocks: &[Option<&[u8]>],
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
    let source = match (original, source_blocks) {
        (Some((_, _, payload)), _) => Source::Member(payload),
        (None, [Some(block)]) => Source::Block(block),
        (None, blocks) => Source::Stored(blocks.iter().flatten().count()),
    };
    let mut text = KeywordsXml::default();
    let built = write_history(&mut text, history)
        .map(|()| text.out)
        .map_err(|error| format!("cannot be built: {}", detail(&error)));
    Ok(checked_member(section, built, source, losses))
}

/// What of a Keywords member's source the output can hold.
#[derive(Clone, Copy)]
enum Source<'a> {
    /// The retained source member.
    Member(&'a [u8]),
    /// The one retained source block of the member.
    Block(&'a [u8]),
    /// The member has no retained source member and not exactly one retained
    /// block; this many retained blocks with bytes are written as stored.
    Stored(usize),
}

impl<'a> Source<'a> {
    fn bytes(self) -> Option<&'a [u8]> {
        match self {
            Self::Member(bytes) | Self::Block(bytes) => Some(bytes),
            Self::Stored(_) => None,
        }
    }
}

/// The lead byte, XML declaration and whitespace before the root element of
/// `member`, when it is UTF-8 XML text after an optional lead byte.
fn prefix(member: &[u8]) -> Option<&[u8]> {
    let lead = usize::from(matches!(member.first(), Some(0x85 | 0x86)));
    let text = &member[lead..];
    if !text.starts_with(b"<?xml") {
        return text.starts_with(b"<").then(|| &member[..lead]);
    }
    let declaration = text.windows(2).position(|pair| pair == b"?>")? + 2;
    let space = text[declaration..]
        .iter()
        .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
        .count();
    Some(&member[..lead + declaration + space])
}

/// The member of the regenerated text when the decoder reads it back, else
/// the source, charging what is not written.
fn checked_member(
    section: &str,
    built: Result<String, String>,
    source: Source<'_>,
    losses: &mut Vec<LossNote>,
) -> Option<Vec<u8>> {
    let refusal = match built {
        Ok(text) => {
            let mut member = source
                .bytes()
                .and_then(prefix)
                .unwrap_or(DEFAULT_PREFIX)
                .to_vec();
            member.extend_from_slice(text.as_bytes());
            match read_back(section, &member, &text) {
                Ok(()) => return Some(member),
                Err(error) => format!("does not read back: {}", detail(&error)),
            }
        }
        Err(refusal) => refusal,
    };
    let written = match source {
        Source::Member(_) => {
            "the source member is written in its place, so the edits of the history are not written"
                .to_owned()
        }
        Source::Block(_) => "the retained source block of the member is written in its place, so the edits of the history are not written".to_owned(),
        Source::Stored(0) => {
            "no source bytes of the member are held, so no member is written and the history is not in the file".to_owned()
        }
        Source::Stored(blocks) => format!(
            "no member is generated; the {blocks} retained source block(s) of the member are written as stored, so the edits of the history are not written"
        ),
    };
    losses.push(
        SldprtLossCode::WriterHistoryRegenerationRefused.note(format!(
            "Keywords member {section}: the regenerated history {refusal}; {written}"
        )),
    );
    source.bytes().map(<[u8]>::to_vec)
}

/// Reads `text`, the text of `member`, as the decoder reads it from a written
/// file: under the allowances of a decode whose input is no larger than the
/// compressed block the writer writes for the member. The written file holds
/// that block, so its decode has at least these allowances.
fn read_back(section: &str, member: &[u8], text: &str) -> Result<(), CodecError> {
    let block = crate::writer::block(member, section, 0)?;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&block, &arena, &DecodePolicy::desktop())?;
    let allowance = ctx.per_expand_allowance();
    if u64::try_from(member.len())
        .ok()
        .is_none_or(|length| length > allowance)
    {
        return Err(CodecError::NotImplemented(format!(
            "the {}-byte member expands past the {allowance}-byte per-expansion allowance of its {}-byte block",
            member.len(),
            block.len()
        )));
    }
    ctx.parse_xml(text, "the Keywords text").map(drop)
}

/// The message of `error` without the prefix its kind adds.
fn detail(error: &CodecError) -> String {
    match error {
        CodecError::NotImplemented(message) | CodecError::Malformed(message) => message.clone(),
        error => error.to_string(),
    }
}

fn write_history(text: &mut KeywordsXml, history: &FeatureHistory) -> Result<(), CodecError> {
    crate::writer::validate_feature_graph(&history.features)?;
    text.open("Keywords");
    if let Some(name) = &history.part_name {
        text.attribute("Name", name);
    }
    for (name, value) in &history.properties {
        text.attribute(name.as_str(), value);
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
                    write_feature(text, feature, &history.features);
                    emitted_features.insert(feature.id.as_str());
                }
            }
            HistoryContent::Text(value) => text.text(value),
            HistoryContent::Element(element) => write_element(text, element),
        }
    }
    for configuration in &history.configurations {
        if emitted_configurations.insert(configuration.id.as_str()) {
            write_configuration(text, configuration);
        }
    }
    for feature in roots {
        if emitted_features.insert(feature.id.as_str()) {
            write_feature(text, feature, &history.features);
        }
    }
    text.end("Keywords");
    Ok(())
}

fn write_configuration(text: &mut KeywordsXml, configuration: &Configuration) {
    text.open("Configuration");
    text.attribute("Name", &configuration.name);
    if let Some(source_index) = configuration.source_index {
        text.attribute("SourceIndex", &source_index.to_string());
    }
    if let Some(material) = &configuration.material {
        text.attribute("Material", material);
    }
    for (name, value) in &configuration.properties {
        text.attribute(name.as_str(), value);
    }
    text.out.push_str("/>");
}

fn write_feature(text: &mut KeywordsXml, feature: &Feature, features: &[Feature]) {
    let tag = feature.xml_tag.as_str();
    text.open(tag);
    if let Some(id) = feature.source_id {
        text.attribute("id", &String::from(id));
    }
    text.attribute("Name", &feature.name);
    text.attribute("Type", &feature.kind);
    if feature.suppressed {
        text.attribute("Suppressed", "true");
    }
    for (name, value) in &feature.properties {
        text.attribute(name.as_str(), value);
    }
    text.out.push('>');
    let write_dimension = |text: &mut KeywordsXml, name: &str, value: &str| {
        text.open("Dimension");
        text.attribute("Name", name);
        if let Some(properties) = feature.dimension_properties.get(name) {
            for (property, value) in properties {
                text.attribute(property.as_str(), value);
            }
        }
        text.out.push('>');
        text.text(value);
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
            text.text(value);
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
                        write_feature(text, child, features);
                        emitted_children.insert(child.id.as_str());
                    }
                }
                FeatureContent::Text(value) => text.text(value),
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
            write_feature(text, child, features);
        }
    }
    text.end(tag);
}

fn write_element(text: &mut KeywordsXml, element: &HistoryElement) {
    text.open(&element.tag);
    for (name, value) in &element.attributes {
        text.attribute(name.as_str(), value);
    }
    if element.content.is_empty() {
        text.out.push_str("/>");
        return;
    }
    text.out.push('>');
    for item in &element.content {
        match item {
            HistoryElementContent::Element(child) => write_element(text, child),
            HistoryElementContent::Text(value) => text.text(value),
        }
    }
    text.end(&element.tag);
}

/// A Keywords text being written.
#[derive(Default)]
struct KeywordsXml {
    out: String,
    /// Whether the last thing written is text. The next text then follows an
    /// empty comment, so the reader reads the two as two text items.
    after_text: bool,
}

impl KeywordsXml {
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

    fn attribute(&mut self, name: &str, value: &str) {
        self.out.push(' ');
        self.out.push_str(name);
        self.out.push_str("=\"");
        self.escaped(value, true);
        self.out.push('"');
    }

    fn text(&mut self, value: &str) {
        if value.is_empty() {
            return;
        }
        if self.after_text {
            self.out.push_str("<!---->");
        }
        self.escaped(value, false);
        self.after_text = true;
    }

    /// Writes `value` with the characters the reader would read otherwise
    /// as references.
    fn escaped(&mut self, value: &str, attribute: bool) {
        for character in value.chars() {
            match character {
                '&' => self.out.push_str("&amp;"),
                '<' => self.out.push_str("&lt;"),
                '"' if attribute => self.out.push_str("&quot;"),
                '>' if !attribute => self.out.push_str("&gt;"),
                '\r' => self.out.push_str("&#13;"),
                '\n' if attribute => self.out.push_str("&#10;"),
                '\t' if attribute => self.out.push_str("&#9;"),
                character => self.out.push(character),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{checked_member, Source, DEFAULT_PREFIX};
    use crate::loss::SldprtLossCode;

    const SOURCE: &[u8] =
        b"\x85<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<Keywords Name=\"Part\"/>";

    fn refused(built: Result<String, String>, source: Source<'_>) -> (Option<Vec<u8>>, String) {
        let mut losses = Vec::new();
        let member = checked_member("Contents/Keywords", built, source, &mut losses);
        assert_eq!(losses.len(), 1, "{losses:?}");
        assert_eq!(
            losses[0].code,
            SldprtLossCode::WriterHistoryRegenerationRefused.kind()
        );
        (member, losses[0].message.clone())
    }

    fn written(text: &str, source: Source<'_>) -> Vec<u8> {
        let mut losses = Vec::new();
        let member = checked_member(
            "Contents/Keywords",
            Ok(text.to_owned()),
            source,
            &mut losses,
        );
        assert!(losses.is_empty(), "{losses:?}");
        member.expect("the regenerated member")
    }

    #[test]
    fn a_regenerated_text_the_reader_refuses_is_replaced_by_the_source_member_or_block() {
        for (source, kept) in [
            (
                Source::Member(SOURCE),
                "the source member is written in its place",
            ),
            (
                Source::Block(SOURCE),
                "the retained source block of the member is written in its place",
            ),
        ] {
            for built in [
                Ok("<Keywords><Feature a b=\"1\"></Feature></Keywords>".to_owned()),
                Err("cannot be built: feature parent cycle".to_owned()),
            ] {
                let (member, message) = refused(built, source);
                assert_eq!(member.as_deref(), Some(SOURCE));
                assert!(message.contains(kept), "{message}");
            }
        }
    }

    #[test]
    fn a_refused_text_without_one_source_states_what_of_the_member_is_in_the_file() {
        let text = || Ok("<Keywords><a:b/></Keywords>".to_owned());
        let (member, message) = refused(text(), Source::Stored(0));
        assert!(member.is_none());
        assert!(
            message.contains("Contents/Keywords: the regenerated history does not read back")
                && message.contains("no source bytes of the member are held, so no member is written and the history is not in the file"),
            "{message}"
        );
        let (member, message) = refused(text(), Source::Stored(2));
        assert!(member.is_none());
        assert!(
            message.contains("no member is generated; the 2 retained source block(s) of the member are written as stored, so the edits of the history are not written"),
            "{message}"
        );
        assert!(!message.contains("not implemented"), "{message}");
    }

    #[test]
    fn a_regenerated_member_keeps_the_lead_byte_and_declaration_of_its_source() {
        let text = "<Keywords Name=\"Edited\"/>";
        let with = |prefix: &[u8]| [prefix, text.as_bytes()].concat();
        assert_eq!(
            written(text, Source::Member(SOURCE)),
            with(&b"\x85<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n"[..])
        );
        assert_eq!(
            written(
                text,
                Source::Block(b"\x86<?xml version=\"1.0\"?><Keywords/>")
            ),
            with(&b"\x86<?xml version=\"1.0\"?>"[..])
        );
        assert_eq!(written(text, Source::Member(b"<Keywords/>")), with(&[]));
        assert_eq!(
            written(text, Source::Member(b"\xff\xfe<\0")),
            with(DEFAULT_PREFIX)
        );
        assert_eq!(written(text, Source::Stored(0)), with(DEFAULT_PREFIX));
        assert_eq!(
            DEFAULT_PREFIX,
            b"\x86<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n"
        );
    }

    /// A Keywords text whose `Application` element holds `elements` empty
    /// elements: four bytes each, which deflate to almost nothing.
    fn wide(elements: usize) -> String {
        format!(
            "<Keywords><Application Name=\"Simulation\">{}</Application></Keywords>",
            "<a/>".repeat(elements)
        )
    }

    #[test]
    fn the_read_back_allows_what_the_decode_of_the_written_block_allows() {
        assert!(written(&wide(60_000), Source::Stored(0)).ends_with(b"</Keywords>"));
        let (member, message) = refused(Ok(wide(300_000)), Source::Member(SOURCE));
        assert_eq!(member.as_deref(), Some(SOURCE));
        assert!(
            message.contains(
                "does not read back: the Keywords text: the XML tree could need more than the"
            ) && message.contains("per-expansion allowance"),
            "{message}"
        );
    }
}
