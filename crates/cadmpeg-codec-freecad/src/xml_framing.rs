// SPDX-License-Identifier: Apache-2.0
//! Source-backed XML headers and leaves over recovering element boundaries.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{
    u64_from_index, DecodeContext, ResourceDimension, View, XmlElement, XmlElementScan,
    XmlEncoding, XmlTermination,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{PartLevel, PartSpec};

mod attribute;
pub(crate) use attribute::XmlAttributeSource;

const STORAGE: &str = "FreeCAD XML semantic fields";

#[derive(Debug)]
pub(crate) struct XmlSource<'source> {
    view: View<'source>,
    scan: XmlElementScan<'source>,
}

impl<'source> XmlSource<'source> {
    pub(crate) fn new(ctx: &DecodeContext<'_>, view: View<'source>) -> Result<Self, CodecError> {
        Ok(Self {
            view,
            scan: ctx.scan_xml_elements(view, XmlEncoding::Utf8, STORAGE)?,
        })
    }

    pub(crate) fn view(&self) -> View<'source> {
        self.view
    }

    pub(crate) fn elements(&self) -> &[XmlElement<'source>] {
        self.scan.elements()
    }

    pub(crate) fn children(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        let start = index.checked_add(1).unwrap_or(self.elements().len());
        self.elements()
            .iter()
            .enumerate()
            .skip(start)
            .take_while(move |(_, element)| element.parent().is_some_and(|parent| parent >= index))
            .filter_map(move |(child, element)| (element.parent() == Some(index)).then_some(child))
    }

    pub(crate) fn name(&self, index: usize) -> Result<&'source str, CodecError> {
        std::str::from_utf8(self.element(index)?.name().window())
            .map(|name| name.rsplit(':').next().unwrap_or(name))
            .map_err(|_| CodecError::malformed("XML element name is not UTF-8"))
    }

    pub(crate) fn read_name(&self, index: usize) -> Result<&'source str, CodecError> {
        let source = self.element(index)?.name();
        let local = source
            .window()
            .iter()
            .rposition(|byte| *byte == b':')
            .map(|colon| {
                colon
                    .checked_add(1)
                    .ok_or_else(|| CodecError::malformed("XML local name offset overflows"))
            })
            .transpose()?
            .unwrap_or(0);
        let start = source
            .start()
            .checked_add(local)
            .ok_or_else(|| CodecError::malformed("XML local name source overflows"))?;
        let bytes = source
            .read_slice(start..source.end())
            .ok_or_else(|| CodecError::malformed("XML local name source is unreadable"))?;
        std::str::from_utf8(bytes).map_err(|_| CodecError::malformed("XML local name is not UTF-8"))
    }

    pub(crate) fn extent(&self, index: usize) -> Result<View<'source>, CodecError> {
        Ok(self.element(index)?.extent())
    }

    pub(crate) fn spec(&self, index: usize, level: &str) -> Result<PartSpec, CodecError> {
        let level = PartLevel::parse(level)
            .ok_or_else(|| CodecError::malformed("XML part level is absent from Appendix C"))?;
        let framing = self
            .element(index)?
            .framing()
            .map(|field| u64_from_index(field.start())..u64_from_index(field.end()))
            .collect();
        Ok(PartSpec::new(level, framing))
    }

    pub(crate) fn validate(&self, index: usize) -> Result<(), CodecError> {
        if !matches!(
            self.element(index)?.termination(),
            XmlTermination::EmptyElement | XmlTermination::EndTag
        ) {
            return Err(CodecError::malformed(
                "XML element has no matching end marker",
            ));
        }
        let element = self.element(index)?;
        let extent = element.extent();
        if self.scan.faults().iter().any(|fault| {
            let fault = fault.extent();
            fault.start() >= extent.start()
                && fault.end() <= extent.end()
                && !self.children(index).any(|child| {
                    let child = self.elements()[child].extent();
                    fault.start() >= child.start() && fault.end() <= child.end()
                })
        }) {
            return Err(CodecError::malformed(
                "XML element contains an unassigned framing fault",
            ));
        }
        if element.termination() == XmlTermination::EndTag {
            let closing = element
                .framing()
                .last()
                .ok_or_else(|| CodecError::malformed("XML element has no closing marker"))?;
            let name = element.name().window();
            let raw = closing.window();
            let after_name = 2_usize
                .checked_add(name.len())
                .ok_or_else(|| CodecError::malformed("XML closing name length overflows"))?;
            if raw.get(..2) != Some(b"</")
                || raw.get(2..after_name) != Some(name)
                || raw.last() != Some(&b'>')
                || raw
                    .get(after_name..raw.len() - 1)
                    .is_none_or(|tail| !tail.iter().all(|byte| xml_space(*byte)))
            {
                return Err(CodecError::malformed("XML closing marker is invalid"));
            }
        }
        Ok(())
    }

    pub(crate) fn insignificant(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<View<'source>>, CodecError> {
        let mut ranges = Vec::new();
        for (index, element) in self.elements().iter().enumerate() {
            if self.whitespace_container(index).is_none() {
                continue;
            }
            let mut children = self.children(index).peekable();
            if children.peek().is_none() {
                continue;
            }
            let fields: Vec<_> = element.framing().collect();
            let Some(opening) = fields.get(1) else {
                continue;
            };
            let mut at = opening.end();
            for child in children {
                let extent = self.extent(child)?;
                let range = self.view.child(at, extent.start()).ok_or_else(|| {
                    CodecError::malformed("XML inter-element extent escapes source")
                })?;
                if !range.window().is_empty() && range.window().iter().all(|byte| xml_space(*byte))
                {
                    push_whitespace(ctx, &mut ranges, range)?;
                }
                at = extent.end();
            }
            if let Some(closing) = fields.get(2) {
                let range = self.view.child(at, closing.start()).ok_or_else(|| {
                    CodecError::malformed("XML trailing inter-element extent escapes source")
                })?;
                if !range.window().is_empty() && range.window().iter().all(|byte| xml_space(*byte))
                {
                    push_whitespace(ctx, &mut ranges, range)?;
                }
            }
        }
        Ok(ranges)
    }

    fn whitespace_container(&self, index: usize) -> Option<&'static str> {
        let element = self.elements().get(index)?;
        let parent = element
            .parent()
            .and_then(|parent| self.whitespace_container(parent));
        match (parent, self.name(index).ok()?) {
            (None, "Document" | "GuiDocument") if element.parent().is_none() => Some("document"),
            (Some("document"), "Objects" | "Features") => Some("declarations"),
            (Some("document"), "ObjectData" | "FeatureData") => Some("data"),
            (Some("data"), "Object" | "Feature") => Some("object"),
            (Some("object"), "Extensions") => Some("extensions"),
            (Some("extensions"), "Extension") => Some("extension"),
            (Some("document"), "ViewProviderData") => Some("providers"),
            (Some("providers"), "ViewProvider") => Some("provider"),
            (Some("document" | "object" | "extension" | "provider"), "Properties") => {
                Some("properties")
            }
            (Some("properties"), "Property" | "_Property") => Some("property"),
            (Some("declarations"), "ObjectDeps") => Some("dependencies"),
            _ => None,
        }
    }

    pub(crate) fn attributes(
        &self,
        ctx: &DecodeContext<'_>,
        index: usize,
    ) -> Result<BTreeMap<String, String>, CodecError> {
        self.header_attributes(ctx, index, None, true)
    }

    pub(crate) fn attribute(
        &self,
        ctx: &DecodeContext<'_>,
        index: usize,
        name: &str,
    ) -> Result<Option<String>, CodecError> {
        Ok(self
            .header_attributes(ctx, index, Some(name), true)?
            .remove(name))
    }

    pub(crate) fn peek_attributes(
        &self,
        ctx: &DecodeContext<'_>,
        index: usize,
    ) -> Result<BTreeMap<String, String>, CodecError> {
        self.header_attributes(ctx, index, None, false)
    }

    pub(crate) fn peek_attribute(
        &self,
        ctx: &DecodeContext<'_>,
        index: usize,
        name: &str,
    ) -> Result<Option<String>, CodecError> {
        Ok(self
            .header_attributes(ctx, index, Some(name), false)?
            .remove(name))
    }

    fn header_attributes(
        &self,
        ctx: &DecodeContext<'_>,
        index: usize,
        selected: Option<&str>,
        tracked: bool,
    ) -> Result<BTreeMap<String, String>, CodecError> {
        self.header_attributes_with_source(ctx, index, selected, tracked, None)
    }

    pub(crate) fn attribute_source(
        &self,
        ctx: &DecodeContext<'_>,
        index: usize,
        name: &str,
    ) -> Result<Option<XmlAttributeSource<'source>>, CodecError> {
        let mut source = None;
        self.header_attributes_with_source(ctx, index, Some(name), false, Some(&mut source))?;
        Ok(source)
    }

    fn header_attributes_with_source(
        &self,
        ctx: &DecodeContext<'_>,
        index: usize,
        selected: Option<&str>,
        tracked: bool,
        mut supplied: Option<&mut Option<XmlAttributeSource<'source>>>,
    ) -> Result<BTreeMap<String, String>, CodecError> {
        let element = self.element(index)?;
        let fields: Vec<_> = element.framing().collect();
        let suffix = fields
            .get(1)
            .ok_or_else(|| CodecError::malformed("XML opening marker has no end"))?;
        let header = self
            .view
            .child(element.name().end(), suffix.start())
            .ok_or_else(|| CodecError::malformed("XML header escapes its source"))?;
        let raw = header.window();
        let mut at = 0;
        let mut result = BTreeMap::new();
        while at < raw.len() {
            let start = at;
            while raw.get(at).is_some_and(|byte| xml_space(*byte)) {
                at += 1;
            }
            if at == raw.len() {
                break;
            }
            if at == start {
                return Err(CodecError::malformed(
                    "XML attributes have no separating whitespace",
                ));
            }
            let name_start = at;
            while raw
                .get(at)
                .is_some_and(|byte| !xml_space(*byte) && *byte != b'=')
            {
                at += 1;
            }
            let name_end = at;
            let name = std::str::from_utf8(&raw[name_start..name_end])
                .map_err(|_| CodecError::malformed("XML attribute name is not UTF-8"))?;
            if !valid_name(name) {
                return Err(CodecError::malformed("XML attribute name is invalid"));
            }
            while raw.get(at).is_some_and(|byte| xml_space(*byte)) {
                at += 1;
            }
            if raw.get(at) != Some(&b'=') {
                return Err(CodecError::malformed("XML attribute has no equals marker"));
            }
            at += 1;
            while raw.get(at).is_some_and(|byte| xml_space(*byte)) {
                at += 1;
            }
            let quote = *raw
                .get(at)
                .filter(|byte| matches!(byte, b'\'' | b'"'))
                .ok_or_else(|| CodecError::malformed("XML attribute value is not quoted"))?;
            at += 1;
            let value_start = at;
            while raw.get(at).is_some_and(|byte| *byte != quote) {
                if raw[at] == b'<' {
                    return Err(CodecError::malformed(
                        "XML attribute contains an opening marker",
                    ));
                }
                at += 1;
            }
            if raw.get(at) != Some(&quote) {
                return Err(CodecError::malformed("XML attribute quote is not closed"));
            }
            let value_end = at;
            at += 1;
            if selected == Some(name) {
                if let Some(supplied) = supplied.as_deref_mut() {
                    let name = header
                        .child(header.start() + name_start, header.start() + name_end)
                        .ok_or_else(|| {
                            CodecError::malformed("attribute supplier name escapes header")
                        })?;
                    let value = header
                        .child(header.start() + value_start, header.start() + value_end)
                        .ok_or_else(|| {
                            CodecError::malformed("attribute supplier value escapes header")
                        })?;
                    *supplied = Some(XmlAttributeSource {
                        header,
                        name,
                        value,
                        proof: ctx.source_extent_proof(header)?,
                    });
                }
            }
            let (name_bytes, value_bytes) =
                if tracked && selected.is_none_or(|selected| selected == name) {
                    (
                        header
                            .read_slice(header.start() + name_start..header.start() + name_end)
                            .ok_or_else(|| {
                                CodecError::malformed("XML attribute name read escapes source")
                            })?,
                        header
                            .read_slice(header.start() + value_start..header.start() + value_end)
                            .ok_or_else(|| {
                                CodecError::malformed("XML attribute value read escapes source")
                            })?,
                    )
                } else {
                    (&raw[name_start..name_end], &raw[value_start..value_end])
                };
            let name = std::str::from_utf8(name_bytes)
                .map_err(|_| CodecError::malformed("XML attribute name is not UTF-8"))?;
            let value = std::str::from_utf8(value_bytes)
                .map_err(|_| CodecError::malformed("XML attribute value is not UTF-8"))?;
            check_allocation(
                ctx,
                name.len() + value.len() + size_of::<(String, String)>(),
            )?;
            let value = if selected.is_none_or(|selected| selected == name) {
                decode_text(value, true)?
            } else {
                value.to_owned()
            };
            if result.insert(name.to_owned(), value).is_some() {
                return Err(CodecError::malformed("XML attribute name is duplicated"));
            }
        }
        Ok(result)
    }

    pub(crate) fn text(&self, ctx: &DecodeContext<'_>, index: usize) -> Result<String, CodecError> {
        self.text_impl(ctx, index, true)
    }

    /// Navigates direct character content without claiming a retained semantic field.
    pub(crate) fn peek_text(
        &self,
        ctx: &DecodeContext<'_>,
        index: usize,
    ) -> Result<String, CodecError> {
        self.text_impl(ctx, index, false)
    }

    fn text_impl(
        &self,
        ctx: &DecodeContext<'_>,
        index: usize,
        tracked: bool,
    ) -> Result<String, CodecError> {
        self.validate(index)?;
        let element = self.element(index)?;
        if element.termination() == XmlTermination::EmptyElement {
            return Ok(String::new());
        }
        let fields: Vec<_> = element.framing().collect();
        let start = fields
            .get(1)
            .ok_or_else(|| CodecError::malformed("XML content has no opening marker"))?
            .end();
        let end = fields
            .get(2)
            .ok_or_else(|| CodecError::malformed("XML content has no closing marker"))?
            .start();
        let mut result = String::new();
        let mut at = start;
        for child in self.children(index) {
            let extent = self.extent(child)?;
            self.append_text(ctx, at, extent.start(), &mut result, tracked)?;
            at = extent.end();
        }
        self.append_text(ctx, at, end, &mut result, tracked)?;
        Ok(result)
    }

    fn append_text(
        &self,
        ctx: &DecodeContext<'_>,
        start: usize,
        end: usize,
        result: &mut String,
        tracked: bool,
    ) -> Result<(), CodecError> {
        let segment = self
            .view
            .child(start, end)
            .ok_or_else(|| CodecError::malformed("XML text escapes source"))?;
        let raw = segment.window();
        let mut at = 0;
        while at < raw.len() {
            if raw[at..].starts_with(b"<!--") {
                at = find_end(raw, at + 4, b"-->")?;
            } else if raw[at..].starts_with(b"<?") {
                at = find_end(raw, at + 2, b"?>")?;
            } else {
                let (text_start, text_end, next, cdata) = if raw[at..].starts_with(b"<![CDATA[") {
                    let next = find_end(raw, at + 9, b"]]>")?;
                    (at + 9, next - 3, next, true)
                } else {
                    let next = raw[at..]
                        .iter()
                        .position(|byte| *byte == b'<')
                        .map_or(raw.len(), |offset| at + offset);
                    if next == at {
                        return Err(CodecError::malformed(
                            "XML character content has invalid markup",
                        ));
                    }
                    (at, next, next, false)
                };
                let field = if tracked {
                    segment.read_slice(segment.start() + text_start..segment.start() + text_end)
                } else {
                    segment.window().get(text_start..text_end)
                }
                .ok_or_else(|| CodecError::malformed("XML text read escapes source"))?;
                let text = std::str::from_utf8(field)
                    .map_err(|_| CodecError::malformed("XML text is not UTF-8"))?;
                check_allocation(
                    ctx,
                    result.len().checked_add(text.len()).ok_or_else(|| {
                        CodecError::malformed("XML character content allocation overflows")
                    })?,
                )?;
                if cdata {
                    result.push_str(&normalize_lines(text));
                } else {
                    result.push_str(&decode_text(text, false)?);
                }
                at = next;
            }
        }
        Ok(())
    }

    fn element(&self, index: usize) -> Result<XmlElement<'source>, CodecError> {
        self.elements()
            .get(index)
            .copied()
            .ok_or_else(|| CodecError::malformed("XML element index is absent"))
    }
}

fn xml_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

fn check_allocation(ctx: &DecodeContext<'_>, bytes: usize) -> Result<(), CodecError> {
    let bytes = u64_from_index(bytes);
    let allowance = ctx.per_expand_allowance();
    if bytes > allowance {
        return Err(ctx.refuse_part_limit(
            ResourceDimension::RetainedBytes,
            allowance,
            0,
            bytes,
            STORAGE,
        ));
    }
    Ok(())
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(name_start)
        && chars.all(|character| {
            name_start(character)
                || character.is_ascii_digit()
                || matches!(character, '-' | '.' | '\u{b7}')
        })
}

fn name_start(character: char) -> bool {
    matches!(character, ':' | '_' | 'A'..='Z' | 'a'..='z' | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{2ff}' | '\u{370}'..='\u{37d}' | '\u{37f}'..='\u{1fff}' | '\u{200c}'..='\u{200d}' | '\u{2070}'..='\u{218f}' | '\u{2c00}'..='\u{2fef}' | '\u{3001}'..='\u{d7ff}' | '\u{f900}'..='\u{fdcf}' | '\u{fdf0}'..='\u{fffd}' | '\u{10000}'..='\u{effff}')
}

fn normalize_lines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn decode_text(text: &str, attribute: bool) -> Result<String, CodecError> {
    let text = normalize_lines(text);
    let mut result = String::new();
    let mut remaining = text.as_str();
    while !remaining.is_empty() {
        if let Some(entity) = remaining.strip_prefix('&') {
            let (entity, rest) = entity
                .split_once(';')
                .ok_or_else(|| CodecError::malformed("XML entity has no semicolon"))?;
            let character = match entity {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "apos" => '\'',
                "quot" => '"',
                _ => {
                    let value = if let Some(hex) = entity.strip_prefix("#x") {
                        u32::from_str_radix(hex, 16).ok()
                    } else if let Some(decimal) = entity.strip_prefix('#') {
                        decimal.parse::<u32>().ok()
                    } else {
                        None
                    };
                    value
                        .and_then(char::from_u32)
                        .filter(|character| xml_character(*character))
                        .ok_or_else(|| {
                            CodecError::malformed("XML entity is invalid or undeclared")
                        })?
                }
            };
            result.push(character);
            remaining = rest;
        } else {
            let character = remaining
                .chars()
                .next()
                .ok_or_else(|| CodecError::malformed("XML text has no character"))?;
            if !xml_character(character) {
                return Err(CodecError::malformed("XML character is invalid"));
            }
            result.push(if attribute && matches!(character, '\t' | '\n') {
                ' '
            } else {
                character
            });
            remaining = &remaining[character.len_utf8()..];
        }
    }
    Ok(result)
}

fn xml_character(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}

fn find_end(raw: &[u8], start: usize, marker: &[u8]) -> Result<usize, CodecError> {
    raw.get(start..)
        .and_then(|tail| tail.windows(marker.len()).position(|bytes| bytes == marker))
        .and_then(|offset| start.checked_add(offset)?.checked_add(marker.len()))
        .ok_or_else(|| CodecError::malformed("XML shielded markup has no closing marker"))
}

#[cfg(test)]
mod tests;

impl<'source> XmlSource<'source> {
    pub(crate) fn attribute_names(&self, index: usize) -> Result<Vec<&'source str>, CodecError> {
        let element = self.element(index)?;
        let fields: Vec<_> = element.framing().collect();
        let suffix = fields
            .get(1)
            .ok_or_else(|| CodecError::malformed("XML opening marker has no end"))?;
        let header = self
            .view
            .child(element.name().end(), suffix.start())
            .ok_or_else(|| CodecError::malformed("XML header escapes its source"))?;
        let raw = header.window();
        let mut names = Vec::new();
        let mut at = 0;
        while at < raw.len() {
            while raw.get(at).is_some_and(|byte| xml_space(*byte)) {
                at += 1;
            }
            if at == raw.len() {
                break;
            }
            let start = at;
            while raw
                .get(at)
                .is_some_and(|byte| !xml_space(*byte) && *byte != b'=')
            {
                at += 1;
            }
            let name = std::str::from_utf8(&raw[start..at])
                .map_err(|_| CodecError::malformed("XML attribute name is not UTF-8"))?;
            if !valid_name(name) {
                return Err(CodecError::malformed("XML attribute name is invalid"));
            }
            while raw.get(at).is_some_and(|byte| xml_space(*byte)) {
                at += 1;
            }
            if raw.get(at) != Some(&b'=') {
                return Err(CodecError::malformed("XML attribute has no equals marker"));
            }
            at += 1;
            while raw.get(at).is_some_and(|byte| xml_space(*byte)) {
                at += 1;
            }
            let quote = *raw
                .get(at)
                .filter(|byte| matches!(byte, b'\'' | b'"'))
                .ok_or_else(|| CodecError::malformed("XML attribute value is not quoted"))?;
            at += 1;
            while raw.get(at).is_some_and(|byte| *byte != quote) {
                at += 1;
            }
            if raw.get(at) != Some(&quote) {
                return Err(CodecError::malformed("XML attribute quote is not closed"));
            }
            at += 1;
            names.push(name);
        }
        Ok(names)
    }
}

fn reserve_whitespace(
    ctx: &DecodeContext<'_>,
    ranges: &mut Vec<View<'_>>,
    additional: usize,
) -> Result<(), CodecError> {
    let required = ranges
        .len()
        .checked_add(additional)
        .ok_or_else(|| CodecError::malformed("XML whitespace inventory length overflows"))?;
    if required <= ranges.capacity() {
        return Ok(());
    }
    let extra = required
        .checked_sub(ranges.capacity())
        .ok_or_else(|| CodecError::malformed("XML whitespace capacity subtraction overflows"))?;
    let bytes = extra
        .checked_mul(size_of::<View<'_>>())
        .map(u64_from_index)
        .ok_or_else(|| CodecError::malformed("XML whitespace inventory storage overflows"))?;
    ranges.try_reserve_exact(additional).map_err(|_| {
        cadmpeg_core::decode::ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            bytes,
            "FreeCAD XML whitespace inventory",
        )
    })?;
    Ok(())
}

fn push_whitespace<'source>(
    ctx: &DecodeContext<'_>,
    ranges: &mut Vec<View<'source>>,
    range: View<'source>,
) -> Result<(), CodecError> {
    reserve_whitespace(ctx, ranges, 1)?;
    ranges.push(range);
    Ok(())
}

pub(crate) fn append_whitespace<'source>(
    ctx: &DecodeContext<'_>,
    ranges: &mut Vec<View<'source>>,
    mut additional: Vec<View<'source>>,
) -> Result<(), CodecError> {
    reserve_whitespace(ctx, ranges, additional.len())?;
    ranges.append(&mut additional);
    Ok(())
}
